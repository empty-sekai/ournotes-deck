//! A bounded, complete tree of independent nominal LUCK outcomes.
//!
//! Branches resume from complete frame checkpoints with the selected outcomes and draw cursor preserved.
//! A law is returned after every positive-mass branch has terminated and its masses sum to one.
use super::{GekisouSetup, LiveModel, LiveNote, LiveParams, LivePlay, Performer};
use crate::{Error, live::random::LiveRandom, master::Master, replay::RankConfirmation};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt::{self, Write},
    rc::Rc,
};

/// Distance between complete frame checkpoints. An interrupted frame is replayed from the latest checkpoint.
const CHECKPOINT_FRAMES: usize = 256;
const MAX_BRANCH_DEPTH: usize = 32;
const MAX_ORDER_RUNS: u64 = 32_768;
const MAX_CACHED_LAWS: usize = 64;
const MAX_CACHED_BYTES: usize = 32 * 1024 * 1024;
const MAX_IDENTITY_BYTES: usize = 512 * 1024;

/// A request shares this work allowance across all candidate orders. Work exhaustion is not a proof.
#[derive(Clone, Debug)]
pub struct LuckExactBudget {
    pub remaining_runs: u64,
    pub remaining_frames: u64,
}

impl Default for LuckExactBudget {
    fn default() -> Self {
        Self { remaining_runs: 240_000, remaining_frames: 8_000_000 }
    }
}

impl LuckExactBudget {
    /// Whether one complete playback fits the default request-wide frame allowance.
    /// Note density does not restrict the probability law's domain.
    pub fn admits_chart(_notes: usize, frames: usize) -> bool {
        u64::try_from(frames).is_ok_and(|frames| frames <= Self::default().remaining_frames)
    }

    pub fn exhausted(&self) -> bool {
        self.remaining_runs == 0 || self.remaining_frames == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LuckExactStats {
    pub replay_runs: u64,
    pub frames: u64,
    pub terminal_paths: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuckExactDecline {
    /// Chart size is fixed across every candidate/order of a request.
    Domain,
    BranchDepth,
    WorkBudget,
    Arithmetic,
    UnhandledRandom,
    Cancelled,
    Unsupported,
}

/// A nonnegative reduced rational. The provider publishes these only as atoms of a complete law.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LuckExactMass {
    pub numerator: u128,
    pub denominator: u128,
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl LuckExactMass {
    const ZERO: Self = Self { numerator: 0, denominator: 1 };
    const ONE: Self = Self { numerator: 1, denominator: 1 };

    fn reduced(numerator: u128, denominator: u128) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let common = gcd(numerator, denominator);
        Some(Self { numerator: numerator / common, denominator: denominator / common })
    }

    fn multiply(self, other: Self) -> Option<Self> {
        let (a, b) = (gcd(self.numerator, other.denominator), gcd(other.numerator, self.denominator));
        Self::reduced(
            (self.numerator / a).checked_mul(other.numerator / b)?,
            (self.denominator / b).checked_mul(other.denominator / a)?,
        )
    }

    fn add(self, other: Self) -> Option<Self> {
        let common = gcd(self.denominator, other.denominator);
        let (a, b) = (other.denominator / common, self.denominator / common);
        Self::reduced(
            self.numerator.checked_mul(a)?.checked_add(other.numerator.checked_mul(b)?)?,
            self.denominator.checked_mul(a)?,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LuckExactAtom {
    pub score: i32,
    pub final_life: i32,
    pub mass: LuckExactMass,
}

/// The atoms sum to exactly one. No constructor accepts a partial tree or a caller-supplied probability law.
#[derive(Clone, Debug)]
pub struct LuckExactLaw {
    atoms: Vec<LuckExactAtom>,
}

impl LuckExactLaw {
    pub fn atoms(&self) -> &[LuckExactAtom] {
        &self.atoms
    }
}

#[derive(Clone, Debug)]
pub struct LuckExactAttempt {
    pub law: Option<LuckExactLaw>,
    pub stats: LuckExactStats,
    pub decline: Option<LuckExactDecline>,
}

fn declined(stats: LuckExactStats, why: LuckExactDecline) -> LuckExactAttempt {
    LuckExactAttempt { law: None, stats, decline: Some(why) }
}

struct CachedLaw {
    identity: String,
    law: LuckExactLaw,
    bytes: usize,
}

/// Complete nominal laws for orders of one immutable simulation context.
///
/// Orders share a law only when their fully initialized models have equal state. The context borrows
/// every input that can affect subsequent frames. Entry and payload limits bound retained laws; a
/// capacity of zero evaluates each order independently. Cache hits execute no frames or replay segments.
pub struct LuckExactSession<'a> {
    master: &'a Master,
    notes: &'a [LiveNote],
    events: &'a [(i32, i32)],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    delta_times: &'a [f32],
    ranking: Option<&'a [RankConfirmation]>,
    capacity: usize,
    cached_bytes: usize,
    laws: VecDeque<CachedLaw>,
}

impl<'a> LuckExactSession<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        master: &'a Master,
        notes: &'a [LiveNote],
        events: &'a [(i32, i32)],
        params: LiveParams,
        setup: &'a GekisouSetup,
        play: &'a LivePlay,
        delta_times: &'a [f32],
        ranking: Option<&'a [RankConfirmation]>,
        cache_entries: usize,
    ) -> Result<Self, Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        Ok(Self {
            master,
            notes,
            events,
            params,
            setup,
            play,
            delta_times,
            ranking,
            capacity: cache_entries.min(MAX_CACHED_LAWS),
            cached_bytes: 0,
            laws: VecDeque::new(),
        })
    }

    pub fn law(
        &mut self,
        deck: &[Performer],
        budget: &mut LuckExactBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<LuckExactAttempt, Error> {
        let stats = LuckExactStats::default();
        if cancelled() {
            return Ok(declined(stats, LuckExactDecline::Cancelled));
        }
        if self.laws.is_empty()
            && (budget.exhausted() || self.play.frames.len() as u128 > budget.remaining_frames as u128)
        {
            return Ok(declined(stats, LuckExactDecline::WorkBudget));
        }
        let mut fresh = if let Some(ranking) = self.ranking {
            let mut model =
                LiveModel::new_gekisou_external(self.master, deck, self.notes, self.events, self.params, self.setup)?;
            model.set_rank_confirmation_timeline(ranking)?;
            model
        } else {
            LiveModel::new_gekisou(self.master, deck, self.notes, self.events, self.params, self.setup)?
        };
        if fresh.draws() != 0 {
            return Ok(declined(stats, LuckExactDecline::UnhandledRandom));
        }
        // These constructors compile owned condition checkers and initialize score recording to None.
        // Their complete derived Debug state includes resolved formation predicates and cumulative
        // counts. Finite float formatting is round-tripping and preserves signed zero. The identity is
        // local to this context and executable; no hash or persistent representation establishes equality.
        let identity = (self.capacity != 0).then(|| initialized_identity(&mut fresh)).flatten();
        if cancelled() {
            return Ok(declined(stats, LuckExactDecline::Cancelled));
        }
        if let Some(identity) = &identity
            && let Some(cached) = self.laws.iter().find(|cached| &cached.identity == identity)
        {
            return Ok(LuckExactAttempt { law: Some(cached.law.clone()), stats, decline: None });
        }
        let result = enumerate_law(fresh, self.play, self.delta_times, budget, &mut cancelled)?;
        if let (Some(identity), Some(law)) = (identity, &result.law) {
            let bytes = identity.len() + law.atoms.len() * std::mem::size_of::<LuckExactAtom>();
            if bytes <= MAX_CACHED_BYTES {
                while self.laws.len() >= self.capacity || self.cached_bytes + bytes > MAX_CACHED_BYTES {
                    self.cached_bytes -= self.laws.pop_front().expect("retained law").bytes;
                }
                self.cached_bytes += bytes;
                self.laws.push_back(CachedLaw { identity, law: law.clone(), bytes });
            }
        }
        Ok(result)
    }
}

fn initialized_identity(model: &mut LiveModel) -> Option<String> {
    // These two lookup tables use randomized hash iteration. Include all entries in sorted order and
    // retain their original storage for execution. All other model maps use deterministic hashing.
    let notes = std::mem::take(&mut model.score.calc.note_factor_percent);
    let judgements = std::mem::take(&mut model.score.calc.judgement_score_factor_percent);
    let ordered_notes: BTreeMap<_, _> = notes.iter().collect();
    let ordered_judgements: BTreeMap<_, _> = judgements.iter().collect();
    let identity = state_identity(&(&*model, ordered_notes, ordered_judgements));
    model.score.calc.note_factor_percent = notes;
    model.score.calc.judgement_score_factor_percent = judgements;
    identity
}

// Every reachable model type derives Debug. Oversized and opaque states simply use independent replay.
fn state_identity(state: &impl fmt::Debug) -> Option<String> {
    struct Bounded(String);
    impl Write for Bounded {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            if self.0.len().saturating_add(value.len()) > MAX_IDENTITY_BYTES {
                return Err(fmt::Error);
            }
            self.0.push_str(value);
            Ok(())
        }
    }
    let mut identity = Bounded(String::new());
    write!(identity, "{state:?}").ok()?;
    // NaN formatting omits payload and sign; a borrowed RefCell omits its value.
    (!identity.0.contains("NaN") && !identity.0.contains("<borrowed>")).then_some(identity.0)
}

/// Enumerate the full nominal law of one specified performer order within an explicit work allowance.
///
/// Lottery probabilities use the current binary32-buffed integer tables, independently conditional on
/// the past. Base-point and bonus LUCK draws provide the semantic event partition. A checkpoint retains
/// every controller, score command, life value, condition updater and random draw count at a completed
/// frame. Sibling outcomes share that checkpoint and replay its suffix with the complete selected
/// outcome prefix. Deterministic stretches can contain any number of notes or frames within the work allowance.
///
/// A complete law accounts for every positive-mass path. Unsupported draws, arithmetic exhaustion and
/// interrupted work produce a declined attempt; ordinary input or simulation errors remain errors.
#[allow(clippy::too_many_arguments)]
pub fn luck_exact_law_with_ranking(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[RankConfirmation]>,
    budget: &mut LuckExactBudget,
    cancelled: impl FnMut() -> bool,
) -> Result<LuckExactAttempt, Error> {
    LuckExactSession::new(master, notes, events, params, setup, play, delta_times, ranking, 0)?
        .law(deck, budget, cancelled)
}

fn enumerate_law(
    mut fresh: LiveModel,
    play: &LivePlay,
    delta_times: &[f32],
    budget: &mut LuckExactBudget,
    mut cancelled: impl FnMut() -> bool,
) -> Result<LuckExactAttempt, Error> {
    let mut stats = LuckExactStats::default();
    if cancelled() {
        return Ok(declined(stats, LuckExactDecline::Cancelled));
    }
    if budget.exhausted() || play.frames.len() as u128 > budget.remaining_frames as u128 {
        return Ok(declined(stats, LuckExactDecline::WorkBudget));
    }
    fresh.set_random(LiveRandom::with_nominal_prefix(Vec::new()));
    let mut pending = vec![(Vec::<usize>::new(), LuckExactMass::ONE, 0usize, Rc::new(fresh))];
    let mut atoms = BTreeMap::<(i32, i32), LuckExactMass>::new();
    while let Some((prefix, mass, mut checkpoint_frame, mut checkpoint)) = pending.pop() {
        if cancelled() {
            return Ok(declined(stats, LuckExactDecline::Cancelled));
        }
        if budget.exhausted() || stats.replay_runs >= MAX_ORDER_RUNS {
            return Ok(declined(stats, LuckExactDecline::WorkBudget));
        }
        budget.remaining_runs -= 1;
        stats.replay_runs += 1;
        let mut model = (*checkpoint).clone();
        model.random.extend_nominal_prefix(prefix.clone())?;
        let mut execution_error = None;
        let start_frame = checkpoint_frame;
        for (frame_index, (frame, &dt)) in play.frames.iter().zip(delta_times).enumerate().skip(start_frame) {
            if cancelled() {
                return Ok(declined(stats, LuckExactDecline::Cancelled));
            }
            if budget.remaining_frames == 0 {
                return Ok(declined(stats, LuckExactDecline::WorkBudget));
            }
            if frame_index - checkpoint_frame >= CHECKPOINT_FRAMES {
                checkpoint_frame = frame_index;
                checkpoint = Rc::new(model.clone());
            }
            budget.remaining_frames -= 1;
            stats.frames += 1;
            if let Err(error) = model.frame_timed(frame.time_ms, &frame.judged, dt) {
                execution_error = Some(error);
                break;
            }
        }
        // This check also applies to the interrupted draw that requests another branch. A late skill draw
        // invalidates the entire law, including any already completed sibling paths.
        if !model.random.nominal_covers_draws() {
            return Ok(declined(stats, LuckExactDecline::UnhandledRandom));
        }
        if let Some(branch) = model.random.nominal_branch() {
            if execution_error.is_none() {
                return Err(Error::Domain("nominal LUCK branch request did not stop playback".into()));
            }
            if prefix.len() >= MAX_BRANCH_DEPTH {
                return Ok(declined(stats, LuckExactDecline::BranchDepth));
            }
            if branch.len().saturating_add(pending.len()) > MAX_ORDER_RUNS as usize {
                return Ok(declined(stats, LuckExactDecline::WorkBudget));
            }
            // Branches are disjoint and exhaustive by nominal_lottery's exact partition check. Push in
            // reverse so replay order is stable in the original semantic table order.
            for (choice, outcome) in branch.iter().enumerate().rev() {
                let Some(next_mass) = mass
                    .multiply(LuckExactMass { numerator: outcome.weight as u128, denominator: outcome.total as u128 })
                else {
                    return Ok(declined(stats, LuckExactDecline::Arithmetic));
                };
                let mut next = prefix.clone();
                next.push(choice);
                pending.push((next, next_mass, checkpoint_frame, Rc::clone(&checkpoint)));
            }
            continue;
        }
        if let Some(error) = execution_error {
            match error {
                Error::Unsupported(_) | Error::Capacity(_) => {
                    return Ok(declined(stats, LuckExactDecline::Unsupported));
                }
                error => return Err(error),
            }
        }
        if !model.random.nominal_prefix_consumed() {
            return Err(Error::Domain("nominal LUCK terminal did not consume its outcome prefix".into()));
        }
        let key = (model.score(), model.current_life());
        let previous = atoms.get(&key).copied().unwrap_or(LuckExactMass::ZERO);
        let Some(combined) = previous.add(mass) else {
            return Ok(declined(stats, LuckExactDecline::Arithmetic));
        };
        atoms.insert(key, combined);
        stats.terminal_paths += 1;
    }
    if cancelled() {
        return Ok(declined(stats, LuckExactDecline::Cancelled));
    }
    let Some(total) = atoms.values().try_fold(LuckExactMass::ZERO, |sum, &mass| sum.add(mass)) else {
        return Ok(declined(stats, LuckExactDecline::Arithmetic));
    };
    if total != LuckExactMass::ONE {
        return Err(Error::Domain("completed nominal LUCK tree does not have probability one".into()));
    }
    let atoms =
        atoms.into_iter().map(|((score, final_life), mass)| LuckExactAtom { score, final_life, mass }).collect();
    Ok(LuckExactAttempt { law: Some(LuckExactLaw { atoms }), stats, decline: None })
}

#[cfg(test)]
mod tests {
    use super::super::{JudgedNote, PlayFrame};
    use super::*;
    use serde_json::json;

    fn fixture() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
        let lots: Vec<_> = (0..5)
            .flat_map(|kind| {
                [0, 3].map(
                    move |result| json!({"_id":kind*4+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1}),
                )
            })
            .collect();
        let tables = json!({
            "MasterLiveSettings":[
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"},
                {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"10"},
                {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"10"},
                {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
            "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
            "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
            "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
            "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":10}],
            "MasterLiveGekisouLuckBonusLot":lots,
            // Each support's conditions are parsed with the master and its lookup indexes.
            "MasterSkillCondition":[
                {"_id":1,"_conditionType":7000,"_conditionValues":[0]},
                {"_id":2,"_conditionType":4011,"_conditionValues":[50]},
                {"_id":3,"_conditionType":5000,"_conditionTargetIDs":[1]}],
            "MasterSkillTarget":[{"_id":1,"_characterID":7}],
            "MasterSkillConditionSet":[
                {"_id":1,"_group":1,"_conditionIds":[1,2]},
                {"_id":2,"_group":2,"_conditionIds":[1]},
                {"_id":3,"_group":3,"_conditionIds":[3]}],
            "MasterSupportSkillEffect":[
                {"_id":1,"_supportSkillID":1,"_level":1,
                    "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillEffectType":2001,
                    "_activationTimeSecond":1,"_effectValue":0},
                {"_id":2,"_supportSkillID":2,"_level":1,
                    "_skillTriggerType":1,"_skillTriggerConditionGroup":2,"_skillEffectType":2001,
                    "_activationTimeSecond":1,"_effectValue":5000},
                {"_id":3,"_supportSkillID":3,"_level":1,
                    "_skillTriggerType":1,"_skillTriggerConditionGroup":3,"_skillEffectType":2001,
                    "_activationTimeSecond":1,"_effectValue":5000}]
        });
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        let master =
            Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, rows)| rows.as_str()))
                .unwrap();
        let notes = vec![LiveNote { note_id: 1, note_operate_type: 1, judgement_type: 1, time_ms: 100 }];
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 1000,
            music_level: 20,
            converted_note_count: 1,
            music_length_ms: 3200,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let setup = GekisouSetup { fevers: vec![(0, 150)], missions: vec![2, 2, 2] };
        let frames: Vec<_> = (0..=30)
            .map(|index| PlayFrame {
                time_ms: index * 100,
                judged: if index == 1 {
                    vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 100 }]
                } else {
                    Vec::new()
                },
            })
            .collect();
        let delta = vec![0.1; frames.len()];
        (master, notes, params, setup, LivePlay { frames, base_seed: 918 }, delta)
    }

    #[test]
    fn checkpoints_after_lottery_draws_match_fresh_cartesian_playback() {
        let (master, mut notes, mut params, mut setup, mut play, _) = fixture();
        notes.push(LiveNote { note_id: 2, note_operate_type: 1, judgement_type: 1, time_ms: 60_100 });
        setup.fevers[0].1 = 60_200;
        params.music_length_ms = 63_200;
        params.converted_note_count = 2;
        play.frames = (0..=630)
            .map(|index| PlayFrame {
                time_ms: index * 100,
                judged: match index {
                    1 => vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 100 }],
                    601 => vec![JudgedNote { note_id: 2, judgement: 5, judgement_time_ms: 60_100 }],
                    _ => Vec::new(),
                },
            })
            .collect();
        let delta = vec![0.1; play.frames.len()];
        let attempt = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        let law = attempt.law.expect("complete separated-draw law");
        let mut counts = BTreeMap::new();
        // Four independent fair choices cover each possible draw. Unread suffix choices replicate
        // a shorter terminal path with exactly its cylinder probability.
        for bits in 0..16 {
            let prefix = (0..4).map(|index| (bits >> index) & 1).collect();
            let mut model = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
            model.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(prefix)).unwrap();
            assert!(model.random.nominal_covers_draws());
            *counts.entry((model.score(), model.current_life())).or_insert(0u128) += 1;
        }
        assert_eq!(law.atoms().len(), counts.len());
        for atom in law.atoms() {
            assert_eq!(atom.mass, LuckExactMass::reduced(counts[&(atom.score, atom.final_life)], 16).unwrap());
        }
    }

    #[test]
    fn long_chart_checkpoints_preserve_the_complete_cartesian_law() {
        let (master, mut notes, mut params, mut setup, mut play, _) = fixture();
        let shift = 60_000;
        for note in &mut notes {
            note.time_ms += shift;
        }
        for frame in &mut play.frames {
            frame.time_ms += shift;
            for note in &mut frame.judged {
                note.judgement_time_ms += shift;
            }
        }
        for (start, end) in &mut setup.fevers {
            *start += shift;
            *end += shift;
        }
        params.music_length_ms += shift;
        params.converted_note_count = 40;
        let mut prefix = Vec::new();
        for index in 0..600 {
            let judged = if (1..40).contains(&index) {
                let note_id = index + 1;
                notes.push(LiveNote { note_id, note_operate_type: 1, judgement_type: 1, time_ms: index * 100 });
                vec![JudgedNote { note_id, judgement: 5, judgement_time_ms: index * 100 }]
            } else {
                Vec::new()
            };
            prefix.push(PlayFrame { time_ms: index * 100, judged });
        }
        prefix.append(&mut play.frames);
        play.frames = prefix;
        let delta = vec![0.1; play.frames.len()];
        let ranking = [RankConfirmation { frame: 600, range: 0, rank: 1, percent: 23 }];
        assert!(LuckExactBudget::admits_chart(notes.len(), play.frames.len()));
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            Some(&ranking),
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        let law = result.law.expect("complete long-chart law");
        assert_eq!(result.stats.terminal_paths, 4);
        assert_eq!(result.stats.replay_runs, 7);
        assert!(result.stats.frames < result.stats.replay_runs * play.frames.len() as u64);
        let mut counts = BTreeMap::new();
        for first in 0..2 {
            for next in 0..2 {
                let mut model = LiveModel::new_gekisou_external(&master, &[], &notes, &[], params, &setup).unwrap();
                model.set_rank_confirmation_timeline(&ranking).unwrap();
                model.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(vec![first, next])).unwrap();
                assert!(model.random.nominal_prefix_consumed() && model.random.nominal_covers_draws());
                *counts.entry((model.score(), model.current_life())).or_insert(0u128) += 1;
            }
        }
        assert_eq!(law.atoms().len(), counts.len());
        for atom in law.atoms() {
            assert_eq!(atom.mass, LuckExactMass::reduced(counts[&(atom.score, atom.final_life)], 4).unwrap());
        }
    }

    #[test]
    fn initialized_identity_preserves_float_bits_and_complete_lookup_tables() {
        let (master, notes, params, setup, _, _) = fixture();
        let mut model = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
        let values32 = [
            0.0f32,
            -0.0,
            f32::from_bits(1),
            f32::MIN_POSITIVE,
            1.0,
            f32::from_bits(1.0f32.to_bits() + 1),
            f32::MAX,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut keys = std::collections::BTreeSet::new();
        for value in values32 {
            model.score.calc.assist_factor = value;
            assert!(keys.insert(initialized_identity(&mut model).unwrap()));
            assert_eq!(state_identity(&value).unwrap().parse::<f32>().unwrap().to_bits(), value.to_bits());
        }
        for bits in [0x7fc00000, 0x7fc00001, 0xffc00001] {
            model.score.calc.assist_factor = f32::from_bits(bits);
            assert!(initialized_identity(&mut model).is_none());
        }
        let values64 = [
            0.0f64,
            -0.0,
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            1.0,
            f64::from_bits(1.0f64.to_bits() + 1),
            f64::MAX,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        let keys: std::collections::BTreeSet<_> = values64
            .iter()
            .map(|value| {
                let key = state_identity(value).unwrap();
                assert_eq!(key.parse::<f64>().unwrap().to_bits(), value.to_bits());
                key
            })
            .collect();
        assert_eq!(keys.len(), values64.len());
        assert!(state_identity(&f64::from_bits(0xfff8000000000001)).is_none());
        assert!(state_identity(&vec![0; MAX_IDENTITY_BYTES]).is_none());
        model.score.calc.assist_factor = 1.0;
        model.score.calc.note_factor_percent.extend([(2, 90), (3, 80)]);
        let key = initialized_identity(&mut model).unwrap();
        let entries: Vec<_> = model.score.calc.note_factor_percent.drain().collect();
        model.score.calc.note_factor_percent.extend(entries.into_iter().rev());
        assert_eq!(initialized_identity(&mut model).unwrap(), key);
        model.score.calc.note_factor_percent.insert(2, 91);
        assert_ne!(initialized_identity(&mut model).unwrap(), key);
        let value = std::cell::RefCell::new(1);
        let _borrow = value.borrow_mut();
        assert!(state_identity(&value).is_none());
    }

    #[test]
    fn complete_laws_share_only_equal_initialized_models_in_their_fixed_context() {
        let (master, notes, params, setup, play, delta) = fixture();
        let mut session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 1).unwrap();
        let mut budget = LuckExactBudget::default();
        let deck = [Performer { character_id: 1, support_skills: vec![(2, 1)], ..Default::default() }];
        let first = session.law(&deck, &mut budget, || false).unwrap();
        assert!(first.stats.frames > 0);
        let mut equivalent = deck.clone();
        equivalent[0].character_id = 2;
        let hit = session.law(&equivalent, &mut budget, || false).unwrap();
        assert_eq!(hit.stats, LuckExactStats::default());
        assert_eq!(hit.law.unwrap().atoms(), first.law.as_ref().unwrap().atoms());
        let independent = luck_exact_law_with_ranking(
            &master,
            &equivalent,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        assert_eq!(independent.law.unwrap().atoms(), first.law.as_ref().unwrap().atoms());
        let cancelled = session.law(&deck, &mut budget, || true).unwrap();
        assert_eq!(cancelled.decline, Some(LuckExactDecline::Cancelled));
        assert!(cancelled.law.is_none());
        let mut empty = LuckExactBudget { remaining_runs: 0, remaining_frames: 0 };
        assert!(session.law(&deck, &mut empty, || false).unwrap().law.is_some());

        let matching = [Performer { character_id: 7, support_skills: vec![(3, 1)], ..Default::default() }];
        let mut other = matching.clone();
        other[0].character_id = 8;
        let matched = session.law(&matching, &mut budget, || false).unwrap();
        let unmatched = session.law(&other, &mut budget, || false).unwrap();
        assert!(matched.stats.frames > 0 && unmatched.stats.frames > 0);
        assert_ne!(matched.law.unwrap().atoms(), unmatched.law.unwrap().atoms());
        assert_eq!(session.laws.len(), 1);
        assert!(session.cached_bytes <= MAX_CACHED_BYTES);
        let evicted = session.law(&deck, &mut empty, || false).unwrap();
        assert_eq!(evicted.decline, Some(LuckExactDecline::WorkBudget));
        assert!(evicted.law.is_none());
        assert_eq!(session.laws.len(), 1);
        let mut separate = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 1).unwrap();
        assert_eq!(separate.law(&other, &mut empty, || false).unwrap().decline, Some(LuckExactDecline::WorkBudget));
    }

    #[test]
    fn complete_two_draw_law_matches_the_full_cartesian_oracle_with_rank_arrival() {
        let (master, notes, params, setup, play, delta) = fixture();
        let ranking = [RankConfirmation { frame: 0, range: 0, rank: 1, percent: 23 }];
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            Some(&ranking),
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        let law = result.law.expect("finite two-draw law");
        assert_eq!(result.stats.terminal_paths, 4);
        // Independent fixed Cartesian enumeration: the first bonus is consumed, the second is pre-drawn.
        // Both are fair Miss/Critical tables. No adaptive branch discovery or law accumulator is reused.
        let mut counts = BTreeMap::<(i32, i32), u128>::new();
        for first in 0..2 {
            for next in 0..2 {
                let mut native = LiveModel::new_gekisou_external(&master, &[], &notes, &[], params, &setup).unwrap();
                native.set_rank_confirmation_timeline(&ranking).unwrap();
                native.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(vec![first, next])).unwrap();
                assert!(native.random.nominal_prefix_consumed() && native.random.nominal_covers_draws());
                assert_eq!(native.rank_confirmation_applications().len(), 1);
                *counts.entry((native.score(), native.current_life())).or_default() += 1;
            }
        }
        assert_eq!(law.atoms.len(), counts.len());
        assert!(counts.len() >= 2, "fixture must have different native terminal scores");
        for atom in law.atoms() {
            assert_eq!(atom.mass, LuckExactMass::reduced(counts[&(atom.score, atom.final_life)], 4).unwrap());
        }
    }

    fn cartesian_counts(
        master: &Master,
        notes: &[LiveNote],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta: &[f32],
        lottery: (u32, [u128; 2]),
    ) -> BTreeMap<(i32, i32), u128> {
        let (draws, weights) = lottery;
        let mut counts = BTreeMap::new();
        for bits in 0..1usize << draws {
            let prefix = (0..draws).map(|index| (bits >> index) & 1).collect();
            let weight: u128 = (0..draws).map(|index| weights[(bits >> index) & 1]).product();
            let mut model = LiveModel::new_gekisou(master, &[], notes, &[], params, setup).unwrap();
            model.run_with_random(play, delta, LiveRandom::with_nominal_prefix(prefix)).unwrap();
            assert!(model.random.nominal_prefix_consumed() && model.random.nominal_covers_draws());
            *counts.entry((model.score(), model.current_life())).or_default() += weight;
        }
        counts
    }

    #[test]
    fn several_nominal_draws_in_one_frame_preserve_the_weighted_joint_law() {
        let (mut master, mut notes, mut params, setup, mut play, delta) = fixture();
        for row in &mut master.gekisou_luck_bonus_lots {
            row.weight = if row.lot_result == 3 { 3 } else { 1 };
        }
        notes.push(LiveNote { note_id: 2, ..notes[0] });
        play.frames[1].judged.push(JudgedNote { note_id: 2, judgement: 5, judgement_time_ms: 100 });
        params.converted_note_count = 2;
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        let counts = cartesian_counts(&master, &notes, params, &setup, &play, &delta, (3, [3, 1]));
        let law = result.law.expect("complete three-draw law");
        assert_eq!(result.stats.terminal_paths, 8);
        assert_eq!(law.atoms.len(), counts.len());
        for atom in law.atoms() {
            assert_eq!(atom.mass, LuckExactMass::reduced(counts[&(atom.score, atom.final_life)], 64).unwrap());
        }
    }

    #[test]
    fn long_schedules_follow_the_nominal_event_partition() {
        let (master, mut notes, mut params, setup, mut play, _) = fixture();
        notes.extend((0..40).map(|index| LiveNote {
            note_id: index + 2,
            note_operate_type: 1,
            judgement_type: 1,
            time_ms: 3300 + index * 100,
        }));
        params.converted_note_count = notes.len() as i32;
        params.music_length_ms = 8200;
        play.frames = (0..=800)
            .map(|index| PlayFrame {
                time_ms: index * 10,
                judged: notes
                    .iter()
                    .filter(|note| note.time_ms == index * 10)
                    .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                    .collect(),
            })
            .collect();
        let delta = vec![0.01; play.frames.len()];
        assert!(LuckExactBudget::admits_chart(notes.len(), play.frames.len()));
        let mut budget = LuckExactBudget::default();
        let initial = budget.clone();
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut budget,
            || false,
        )
        .unwrap();
        let counts = cartesian_counts(&master, &notes, params, &setup, &play, &delta, (2, [1, 1]));
        let law = result.law.expect("complete long-schedule law");
        assert_eq!(result.stats.terminal_paths, 4);
        assert_eq!(result.stats.replay_runs, initial.remaining_runs - budget.remaining_runs);
        assert_eq!(result.stats.frames, initial.remaining_frames - budget.remaining_frames);
        assert_eq!(law.atoms.len(), counts.len());
        for atom in law.atoms() {
            assert_eq!(atom.mass, LuckExactMass::reduced(counts[&(atom.score, atom.final_life)], 4).unwrap());
        }
        let mut session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 1).unwrap();
        let first = session.law(&[], &mut LuckExactBudget::default(), || false).unwrap();
        assert_eq!(first.law.unwrap().atoms(), law.atoms());
        let mut empty = LuckExactBudget { remaining_runs: 0, remaining_frames: 0 };
        let reused = session.law(&[], &mut empty, || false).unwrap();
        assert_eq!(reused.law.unwrap().atoms(), law.atoms());
        assert_eq!(reused.stats, LuckExactStats::default());
    }

    #[test]
    fn conditional_score_state_is_local_to_each_frame_continuation() {
        let (master, mut notes, mut params, setup, mut play, delta) = fixture();
        notes.push(LiveNote { note_id: 2, time_ms: 200, ..notes[0] });
        play.frames[2].judged.push(JudgedNote { note_id: 2, judgement: 5, judgement_time_ms: 200 });
        params.converted_note_count = 2;
        let deck = [Performer { support_skills: vec![(2, 1)], ..Default::default() }];
        let result = luck_exact_law_with_ranking(
            &master,
            &deck,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        let mut counts = BTreeMap::<(i32, i32), u128>::new();
        for first in 0..2 {
            for second in 0..2 {
                let mut oracle = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
                oracle.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(vec![first, second])).unwrap();
                assert!(oracle.random.nominal_prefix_consumed() && oracle.random.nominal_covers_draws());
                *counts.entry((oracle.score(), oracle.current_life())).or_default() += 1;
            }
        }
        let law = result.law.expect("complete conditional score law");
        assert_eq!(law.atoms.len(), counts.len());
        for atom in law.atoms() {
            assert_eq!(atom.mass, LuckExactMass::reduced(counts[&(atom.score, atom.final_life)], 4).unwrap());
        }

        let mut checkpoint = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        checkpoint.set_random(LiveRandom::with_nominal_prefix(vec![1, 0]));
        checkpoint.play_frames(&play, &delta, 2).unwrap();
        assert!(checkpoint.random.nominal_prefix_consumed());
        let state = format!("{:?}", checkpoint.cond);
        let factors = format!("{:?}", checkpoint.factor_state());
        let trace = checkpoint.trace().to_vec();
        let checkpoint = Rc::new(checkpoint);
        let mut scores = Vec::new();
        for _ in 0..2 {
            let mut continuation = (*checkpoint).clone();
            continuation.random.extend_nominal_prefix(vec![1, 0]).unwrap();
            continuation.play_frames(&play, &delta, play.frames.len()).unwrap();
            assert_ne!(format!("{:?}", continuation.cond), state);
            assert_eq!(format!("{:?}", checkpoint.cond), state);
            assert_eq!(format!("{:?}", checkpoint.factor_state()), factors);
            assert_eq!(checkpoint.trace(), trace);
            assert_eq!(checkpoint.frames_played(), 2);
            scores.push(continuation.score());
        }
        assert_eq!(scores[0], scores[1]);
    }

    #[test]
    fn the_semantic_path_depth_bounds_retained_frame_checkpoints() {
        let (master, _, mut params, mut setup, mut play, _) = fixture();
        let notes: Vec<_> = (1..=40)
            .map(|index| LiveNote { note_id: index, note_operate_type: 1, judgement_type: 1, time_ms: index * 100 })
            .collect();
        params.converted_note_count = notes.len() as i32;
        params.music_length_ms = 7000;
        setup.fevers = vec![(0, 4500)];
        play.frames = (0..=650)
            .map(|index| PlayFrame {
                time_ms: index * 10,
                judged: notes
                    .iter()
                    .filter(|note| note.time_ms == index * 10)
                    .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                    .collect(),
            })
            .collect();
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &vec![0.01; play.frames.len()],
            None,
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        assert_eq!(result.decline, Some(LuckExactDecline::BranchDepth));
        assert!(result.law.is_none());
        assert_eq!(result.stats.terminal_paths, 0);
        assert!(result.stats.replay_runs <= 2 * MAX_BRANCH_DEPTH as u64 + 1);
    }

    #[test]
    fn partial_mass_budget_and_cancellation_publish_no_law() {
        let (master, notes, params, setup, play, delta) = fixture();
        let mut budget = LuckExactBudget { remaining_runs: 3, remaining_frames: 10_000 };
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut budget,
            || false,
        )
        .unwrap();
        assert_eq!(result.stats.terminal_paths, 1, "completed siblings carry only part of the probability mass");
        assert_eq!(result.decline, Some(LuckExactDecline::WorkBudget));
        assert!(result.law.is_none());
        let mut checks = 0;
        let result = luck_exact_law_with_ranking(
            &master,
            &[],
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut LuckExactBudget::default(),
            || {
                checks += 1;
                checks == 12
            },
        )
        .unwrap();
        assert!(result.stats.replay_runs > 1, "cancellation should interrupt a discovered probability tree");
        assert_eq!(result.decline, Some(LuckExactDecline::Cancelled));
        assert!(result.law.is_none());
    }

    #[test]
    fn a_late_skill_draw_on_only_one_luck_branch_rejects_completed_siblings() {
        let (master, notes, params, setup, play, delta) = fixture();
        let deck = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
        let result = luck_exact_law_with_ranking(
            &master,
            &deck,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            None,
            &mut LuckExactBudget::default(),
            || false,
        )
        .unwrap();
        assert!(result.stats.terminal_paths > 0, "Critical siblings complete before the Miss-only probability skill");
        assert_eq!(result.decline, Some(LuckExactDecline::UnhandledRandom));
        assert!(result.law.is_none());
    }

    #[test]
    fn rational_partition_is_exact_and_capacity_failure_has_no_truncated_value() {
        let weights = [5, 4, 2, 1];
        let mut total = LuckExactMass::ZERO;
        for a in weights {
            for b in weights {
                let mass =
                    LuckExactMass::reduced(a, 12).unwrap().multiply(LuckExactMass::reduced(b, 12).unwrap()).unwrap();
                total = total.add(mass).unwrap();
            }
        }
        assert_eq!(total, LuckExactMass::ONE);
        assert!(
            LuckExactMass::reduced(1, u128::MAX).unwrap().multiply(LuckExactMass::reduced(1, 2).unwrap()).is_none()
        );
        // Cross-cancellation is performed before products, so representable fractions are not lost here.
        assert_eq!(
            LuckExactMass::reduced(u128::MAX - 1, u128::MAX)
                .unwrap()
                .multiply(LuckExactMass::reduced(u128::MAX, u128::MAX - 1).unwrap()),
            Some(LuckExactMass::ONE)
        );
    }
}
