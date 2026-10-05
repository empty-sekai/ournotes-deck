//! A bounded, complete tree of independent nominal LUCK outcomes.
//!
//! Each prefix replays a fresh full model. There is deliberately no state merging, seeded sampling or
//! partial-mass normalization. A law is returned only after every positive-mass branch has terminated.
use super::{GekisouSetup, LiveModel, LiveNote, LiveParams, LivePlay, Performer};
use crate::{Error, live::random::LiveRandom, master::Master, replay::RankConfirmation};
use std::collections::BTreeMap;

const MAX_NOTES: usize = 32;
const MAX_FRAMES: usize = 512;
const MAX_BRANCH_DEPTH: usize = 32;
const MAX_ORDER_RUNS: u64 = 32_768;

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
    /// Admission is request-wide: every candidate/order shares the declared chart and frame schedule.
    pub fn admits_chart(notes: usize, frames: usize) -> bool {
        notes <= MAX_NOTES && frames <= MAX_FRAMES
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

/// Enumerate the full nominal law of ONE specified performer order within an explicit finite work allowance.
///
/// Only the native base-point and bonus LUCK draws are intercepted. Any other random draw, including a
/// probability skill that happens to return a constant, declines this backend. Lottery probabilities come
/// from the actual native binary32-buffed integer tables at each draw, independently conditional on the past.
/// Unsupported or over-budget orders publish no atoms. Ordinary invalid input/native execution errors remain
/// errors; failing paths are never discarded and the surviving mass is never renormalized.
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
    mut cancelled: impl FnMut() -> bool,
) -> Result<LuckExactAttempt, Error> {
    let mut stats = LuckExactStats::default();
    if delta_times.len() != play.frames.len() {
        return Err(Error::Input("one delta time per frame".into()));
    }
    if !LuckExactBudget::admits_chart(notes.len(), play.frames.len()) {
        return Ok(declined(stats, LuckExactDecline::Domain));
    }
    if cancelled() {
        return Ok(declined(stats, LuckExactDecline::Cancelled));
    }
    if budget.exhausted() {
        return Ok(declined(stats, LuckExactDecline::WorkBudget));
    }
    let fresh = if let Some(ranking) = ranking {
        let mut model = LiveModel::new_gekisou_external(master, deck, notes, events, params, setup)?;
        model.set_rank_confirmation_timeline(ranking)?;
        model
    } else {
        LiveModel::new_gekisou(master, deck, notes, events, params, setup)?
    };
    if fresh.draws() != 0 {
        return Ok(declined(stats, LuckExactDecline::UnhandledRandom));
    }
    let mut pending = vec![(Vec::<usize>::new(), LuckExactMass::ONE)];
    let mut atoms = BTreeMap::<(i32, i32), LuckExactMass>::new();
    while let Some((prefix, mass)) = pending.pop() {
        if cancelled() {
            return Ok(declined(stats, LuckExactDecline::Cancelled));
        }
        if budget.exhausted() || stats.replay_runs >= MAX_ORDER_RUNS {
            return Ok(declined(stats, LuckExactDecline::WorkBudget));
        }
        budget.remaining_runs -= 1;
        stats.replay_runs += 1;
        let mut model = fresh.clone();
        model.set_random(LiveRandom::with_nominal_prefix(prefix.clone()));
        let mut execution_error = None;
        for (frame, &dt) in play.frames.iter().zip(delta_times) {
            if cancelled() {
                return Ok(declined(stats, LuckExactDecline::Cancelled));
            }
            if budget.remaining_frames == 0 {
                return Ok(declined(stats, LuckExactDecline::WorkBudget));
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
                pending.push((next, next_mass));
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
            // Only the late-SKILL test equips this support. Parse these rows with the master so its
            // lookup indexes include them; mutating the public row vectors would not rebuild indexes.
            "MasterSkillCondition":[
                {"_id":1,"_conditionType":7000,"_conditionValues":[0]},
                {"_id":2,"_conditionType":4011,"_conditionValues":[50]}],
            "MasterSkillConditionSet":[{"_id":1,"_group":1,"_conditionIds":[1,2]}],
            "MasterSupportSkillEffect":[{"_id":1,"_supportSkillID":1,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillEffectType":2001,
                "_activationTimeSecond":1,"_effectValue":0}]
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
        assert_eq!(result.stats.replay_runs, 7);
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
        assert_eq!(result.stats.terminal_paths, 1, "one completed sibling is deliberately insufficient");
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
