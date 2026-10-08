//! Complete controller-law families for a fixed five-member formation.
//!
//! This optional capability certifies controller probabilities and a common terminal-note mapping, not a score,
//! an exact payoff law, a candidate value or a completed search. Every supplied physical Snap choice is checked.
//! Ordinary programs may differ only after their complete deterministic dependency closure has been admitted.
//! The original member order, source order and native binary32 writer operations are retained in every one of the
//! 120 performance orders. A family is published only after every writer-placement profile and order completed.

use super::*;
use crate::live::skip::is_judgement_note;
use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::Arc;

const SLOTS: usize = 5;
const ORDERS: usize = 120;
// The optional context recorder is bounded separately from per-family work. Larger contexts retain the full
// scorer; these limits never remove candidates or return a partial controller family.
const MAX_CONTEXT_ITEMS: usize = 1_000_000;
const MAX_CONTEXT_FRAMES: usize = 100_000;

/// One legal physical Snap choice for a fixed member. `None` has no support skill sources and consumes no
/// resource. A resource number denotes the same complete ordered Snap sources in every member's choice list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuckFamilyChoice {
    pub resource: Option<usize>,
    pub performer: Performer,
}

/// Work and retained-payload limits for an optional family certificate. Exhaustion is an explicit refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LuckFamilyLimits {
    pub max_pair_models: usize,
    pub max_profiles: usize,
    pub max_order_evaluations: usize,
    pub max_frame_work: u64,
    pub max_retained_bytes: usize,
}

impl Default for LuckFamilyLimits {
    fn default() -> Self {
        Self {
            max_pair_models: 256,
            max_profiles: 6,
            max_order_evaluations: 720,
            max_frame_work: 20_000_000,
            max_retained_bytes: 32 * 1024 * 1024,
        }
    }
}

/// An optional controller-family refusal. None of these states authorizes pruning from a partial cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuckFamilyDecline {
    Context,
    TerminalMapping,
    PairDomain,
    RecorderAdmission,
    LifeFeedback,
    JudgementFeedback,
    WriterProfiles,
    ProbabilityDomain,
    Budget,
    Capacity,
    IncompleteCoverage,
}

impl LuckFamilyDecline {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::TerminalMapping => "terminalMapping",
            Self::PairDomain => "pairDomain",
            Self::RecorderAdmission => "recorderAdmission",
            Self::LifeFeedback => "lifeFeedback",
            Self::JudgementFeedback => "judgementFeedback",
            Self::WriterProfiles => "writerProfiles",
            Self::ProbabilityDomain => "probabilityDomain",
            Self::Budget => "budget",
            Self::Capacity => "capacity",
            Self::IncompleteCoverage => "incompleteCoverage",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuckFamilyError {
    pub reason: LuckFamilyDecline,
    pub error: Error,
}

fn fail(reason: LuckFamilyDecline, message: &str) -> LuckFamilyError {
    let error = if matches!(reason, LuckFamilyDecline::Budget | LuckFamilyDecline::Capacity) {
        Error::Capacity(format!("LUCK controller family: {message}"))
    } else {
        Error::Unsupported(format!("LUCK controller family: {message}"))
    };
    LuckFamilyError { reason, error }
}

fn source_error(reason: LuckFamilyDecline, error: Error) -> LuckFamilyError {
    LuckFamilyError { reason, error }
}

fn reserve<T>(length: usize) -> Result<Vec<T>, LuckFamilyError> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|_| fail(LuckFamilyDecline::Capacity, "allocation capacity"))?;
    Ok(values)
}

/// The original declared judgement and its immutable chart note. This includes unscored controller inputs.
#[derive(Clone, Copy, Debug)]
struct InputNote {
    note: LiveNote,
    judgement: i32,
}

/// Geometry only. Successful construction is not itself whole-domain recorder admission: `prepare` separately
/// checks every possible row before this mapping can authorize a returned family.
#[derive(Debug)]
struct DomainTerminalMapping {
    times: Vec<i32>,
    inputs: Vec<InputNote>,
    last_luck_note: i32,
    frame_work: u64,
}

/// One immutable request context. The master, selected skill catalogue, declared frame stream, deltas and setup
/// remain borrowed for its lifetime. This first capability is solo-only and has no raw-judgement/runtime setter.
#[derive(Debug)]
pub struct LuckFamilyContext<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    /// `related` retains every 11000..11005 row even with an empty shape catalogue. Removed score probes
    /// are admitted against `skills` first. Certified masses always retain the virtual probe bit.
    writer_skills: LuckSkills,
    notes: &'a [LiveNote],
    events: &'a [(i32, i32)],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    deltas: &'a [f32],
    mapping: Arc<DomainTerminalMapping>,
}

/// One complete native controller law. `positions[physical_member]` is that member's original performance
/// position. Equal laws may share an Arc after the existing exact-transcript cache proves equality; no order is
/// deleted from this list, and no writer accumulation is treated as commutative.
#[derive(Clone, Debug)]
pub struct LuckFamilyOrderLaw {
    pub positions: [usize; SLOTS],
    pub profile: usize,
    probability: Arc<LuckDpCertifiedResult>,
}

impl LuckFamilyOrderLaw {
    /// Directly accumulated joint masses [neither, virtual direct-7021 probe, Rush, both] at chart time.
    /// These are under the declared independent nominal probability model, not the finite PRNG seed space.
    pub fn joint_at(&self, time_ms: i32) -> [ProbabilityMass; 4] {
        let index = self.probability.steps.partition_point(|(time, _)| *time <= time_ms);
        index.checked_sub(1).map_or(
            [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO],
            |index| self.probability.steps[index].1,
        )
    }
}

/// A complete law family for all supplied physical Snap choices and all 120 orders. Its virtual probe bit can
/// condition only matching direct untimed LUCK-gated score windows; it says nothing about a window's magnitude.
/// All ordinary score history, native rounding, rank and conversion-budget remainders remain the reward bound's
/// responsibility. A mean score cap cannot replace the expectation of a nonlinear payoff.
#[derive(Debug)]
pub struct LuckControllerFamily {
    mapping: Arc<DomainTerminalMapping>,
    allowed: [Vec<Option<usize>>; SLOTS],
    writer: Option<usize>,
    profiles: Vec<Option<usize>>,
    orders: Vec<LuckFamilyOrderLaw>,
    bytes: usize,
}

impl LuckControllerFamily {
    pub fn orders(&self) -> &[LuckFamilyOrderLaw] {
        &self.orders
    }

    /// Complete sorted scored-occurrence multiset, including repeated chart times.
    pub fn note_times(&self) -> &[i32] {
        &self.mapping.times
    }

    /// Whole-domain admission establishes one virtual direct-7021 predicate under LUCK, even for profiles with
    /// no actual holder. A reward bound must still prove that each discounted source window matches it.
    pub fn probe_gate(&self) -> Option<i64> {
        Some(M_LUCK)
    }

    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// Retained container capacities and referenced curve payloads, charging each distinct Arc once here.
    /// Caller-owned masters, the borrowed context and independent DP cache capacity are not process RSS.
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    /// The certified profile for this exact legal physical resource assignment. This validates every slot and
    /// the no-duplicate-Snap rule; callers must not select a profile by searching for a similar numeric curve.
    pub fn profile_for(&self, resources: &[Option<usize>; SLOTS]) -> Option<usize> {
        let mut seen = BTreeSet::new();
        let mut writer_owner = None;
        for (slot, &resource) in resources.iter().enumerate() {
            if !self.allowed[slot].contains(&resource) {
                return None;
            }
            if let Some(resource) = resource {
                if !seen.insert(resource) {
                    return None;
                }
                if Some(resource) == self.writer {
                    writer_owner = Some(slot);
                }
            }
        }
        self.profiles.iter().position(|&owner| owner == writer_owner)
    }
}

/// Complete coverage of the original labelled orders. A duplicate mark cannot pay for a missing order.
struct FamilyCoverage {
    covered: Vec<[u64; 2]>,
}

impl FamilyCoverage {
    fn new(profiles: usize) -> Result<Self, LuckFamilyError> {
        let mut covered = reserve(profiles)?;
        covered.resize(profiles, [0; 2]);
        Ok(Self { covered })
    }

    fn mark(&mut self, profile: usize, order: usize) -> bool {
        let Some(bits) = self.covered.get_mut(profile) else {
            return false;
        };
        if order >= ORDERS {
            return false;
        }
        let bit = 1u64 << (order % 64);
        let word = &mut bits[order / 64];
        let fresh = *word & bit == 0;
        *word |= bit;
        fresh
    }

    fn complete(&self) -> bool {
        !self.covered.is_empty() && self.covered.iter().all(|bits| *bits == [u64::MAX, (1u64 << 56) - 1])
    }
}

fn next_order(order: &mut [usize; SLOTS]) -> bool {
    let Some(left) = (0..SLOTS - 1).rev().find(|&i| order[i] < order[i + 1]) else {
        return false;
    };
    let right = (left + 1..SLOTS).rev().find(|&i| order[left] < order[i]).expect("larger suffix entry");
    order.swap(left, right);
    order[left + 1..].reverse();
    true
}

fn member(performer: &Performer) -> Performer {
    let mut member = performer.clone();
    member.support_skills.clear();
    member.gekisou_support_skills.clear();
    member
}

fn projected(master: &Master, performer: &Performer) -> Performer {
    let mut out = performer.clone();
    out.live_skill = None;
    out.support_skills.clear();
    out.gekisou_support_skills.retain(|&(id, level)| {
        master
            .gekisou_support_skill_effects
            .iter()
            .any(|row| row.skill_id == id && row.level == level && is_luck_chain(row.skill_effect_type))
    });
    out
}

fn life_reader(checker: &Checker) -> bool {
    checker.any(&|checker| {
        matches!(
            checker,
            Checker::LifeAtLeast(_)
                | Checker::LifeAtMost(_)
                | Checker::LifeGreater(_)
                | Checker::LifeLess(_)
                | Checker::LifeChanged { .. }
                | Checker::LifePercent { .. }
                | Checker::LifeDelta { .. }
        )
    })
}

fn check_runtime_checker(checker: &Checker) -> Result<(), LuckFamilyError> {
    if checker.any(&|checker| match checker {
        Checker::LifeAtLeast(value)
        | Checker::LifeAtMost(value)
        | Checker::LifeGreater(value)
        | Checker::LifeLess(value) => value.is_none(),
        Checker::LifeDelta { threshold, .. } => *threshold <= 0,
        Checker::NoteJudgementMatch { kind, targets, .. } => {
            *kind != 1000 && targets.iter().any(|target| !(1..=6).contains(target))
        }
        _ => false,
    }) {
        return Err(fail(LuckFamilyDecline::RecorderAdmission, "a checker has a deferred runtime error"));
    }
    Ok(())
}

fn check_runtime_cumulative(cumulative: &Cumulative) -> Result<(), LuckFamilyError> {
    let missing = match cumulative {
        Cumulative::Fixed(_) | Cumulative::ElapsedTime { .. } => false,
        Cumulative::ComboPerN { values_empty, .. }
        | Cumulative::JudgementPerN { values_empty, .. }
        | Cumulative::LifePerN { values_empty, .. } => *values_empty,
    };
    if missing {
        return Err(fail(LuckFamilyDecline::RecorderAdmission, "a cumulative has a deferred missing value"));
    }
    Ok(())
}

/// A positive RangePlaying conjunct cannot trigger before a matching fever starts. OR and NOT are not such
/// certificates; fixed booleans are deliberately not used to infer unobserved runtime state.
fn playing_after(checker: &Checker, after: i32, setup: &GekisouSetup) -> bool {
    match checker {
        Checker::RangePlaying { missions, .. } => setup.fevers.iter().zip(&setup.missions).all(|(&(start, _), &m)| {
            !missions.is_empty() && !missions.contains(&gekisou::M_ALL) && !missions.contains(&m) || start > after
        }),
        Checker::And { items, .. } => items.iter().any(|item| playing_after(item, after, setup)),
        _ => false,
    }
}

/// A native conversion's `context` memo is keyed only by its raw row ID. Two simultaneously legal sources
/// can therefore share targets even when their expanded updater/state IDs differ. Every pair-domain row must
/// use the same exact target vector for an aliased raw ID, including rows omitted by the later timing graph.
/// Equality makes the native first-registration choice irrelevant; mismatches keep the original scorer.
fn remember_conversion_targets(model: &LiveModel, known: &mut Vec<(i64, Vec<i64>)>) -> Result<(), LuckFamilyError> {
    for row in model.rows.iter().filter(|row| matches!(row.effect_type, 12006 | 13005)) {
        let targets = row.targets().map_err(|e| source_error(LuckFamilyDecline::RecorderAdmission, e))?;
        if let Some((_, previous)) = known.iter().find(|(id, _)| *id == row.id) {
            if previous.as_slice() != targets {
                return Err(fail(
                    LuckFamilyDecline::JudgementFeedback,
                    "aliased conversion rows have different targets",
                ));
            }
        } else {
            let mut owned = reserve(targets.len())?;
            owned.extend_from_slice(targets);
            known.try_reserve(1).map_err(|_| fail(LuckFamilyDecline::Capacity, "conversion identity capacity"))?;
            known.push((row.id, owned));
        }
    }
    Ok(())
}

/// Add every potentially reachable conversion, ignoring all duration/limit/window restrictions. A discarded
/// conversion must have a positive RangePlaying trigger which cannot run until every LUCK note has passed.
fn conversion_edges(model: &LiveModel, after: i32, setup: &GekisouSetup) -> Result<[[bool; 7]; 7], LuckFamilyError> {
    let mut edges = [[false; 7]; 7];
    let mut add = |row: &EffectRow| -> Result<(), LuckFamilyError> {
        if !matches!(row.effect_type, 12006 | 13005) {
            return Ok(());
        }
        let to = if row.effect_type == 13005 { 6 } else { row.effect_value as i32 };
        if !(1..=6).contains(&to) {
            return Ok(());
        }
        for &from in row.targets().map_err(|e| source_error(LuckFamilyDecline::RecorderAdmission, e))? {
            if (1..=6).contains(&from) {
                edges[from as usize][to as usize] = true;
            }
        }
        Ok(())
    };
    for skill in &model.live {
        for effect in &skill.effects {
            add(&model.rows[effect.row])?;
        }
    }
    for skill in &model.cond {
        for effect in skill.updater.effects() {
            if !effect.trigger.as_ref().is_some_and(|trigger| playing_after(trigger, after, setup)) {
                add(&model.rows[effect.row])?;
            }
        }
    }
    Ok(edges)
}

fn close_conversions(edges: &mut [[bool; 7]; 7]) {
    for k in 1..=6 {
        for from in 1..=6 {
            for to in 1..=6 {
                edges[from][to] |= edges[from][k] && edges[k][to];
            }
        }
    }
}

impl<'a> LuckFamilyContext<'a> {
    /// Build immutable terminal geometry using the native solo frame/query state machine. The empty-reward
    /// recording is combined with whole-domain row admission in `prepare`; it alone is never authority to reuse
    /// a candidate curve. Cancellation returns None, and every unsupported input returns an explicit error.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        master: &'a Master,
        skills: &'a LuckSkills,
        notes: &'a [LiveNote],
        events: &'a [(i32, i32)],
        mut params: LiveParams,
        setup: &'a GekisouSetup,
        play: &'a LivePlay,
        deltas: &'a [f32],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<Self>, LuckFamilyError> {
        if cancelled() {
            return Ok(None);
        }
        if setup.fevers.is_empty()
            || setup.fevers.len() > 3
            || setup.missions.len() != 3
            || !setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission == M_LUCK)
            || setup.fevers.iter().any(|&(start, end)| start < 0 || end <= start)
            || play.frames.len() < 2
            || play.frames.len() > MAX_CONTEXT_FRAMES
            || notes.len().saturating_add(play.frames.len()) > MAX_CONTEXT_ITEMS
            || deltas.len() != play.frames.len()
            || params.converted_note_count <= 0
            || !params.assist_factor.is_finite()
        {
            return Err(fail(LuckFamilyDecline::Context, "unsupported bounded solo context"));
        }
        let mut event_counts = [0usize; SLOTS];
        for &(position, time) in events {
            if time < 0 {
                return Err(fail(LuckFamilyDecline::Context, "negative ordinary skill event timestamp"));
            }
            let Some(count) = usize::try_from(position).ok().and_then(|position| event_counts.get_mut(position)) else {
                return Err(fail(LuckFamilyDecline::Context, "skill event outside the five original positions"));
            };
            *count += 1;
        }
        if event_counts.iter().any(|&count| count > 5) {
            return Err(fail(LuckFamilyDecline::RecorderAdmission, "ordinary live-pool lifetime is not bounded"));
        }
        let maximum_note = notes.iter().map(|note| note.time_ms).max().unwrap_or(0);
        let score_length = params.score_music_length_ms.filter(|&length| length != 0).unwrap_or(params.music_length_ms);
        if notes.iter().any(|note| note.time_ms < 0)
            || params.music_length_ms <= maximum_note
            || params.music_length_ms as usize > MAX_CONTEXT_FRAMES * 40
            || score_length <= 0
            || score_length as usize > MAX_CONTEXT_FRAMES * 40
        {
            return Err(fail(LuckFamilyDecline::TerminalMapping, "unproved finish clamp or score-frame geometry"));
        }
        let last = play.frames.last().expect("two frames checked");
        let before_last = &play.frames[play.frames.len() - 2];
        if !last.judged.is_empty() || maximum_note > before_last.time_ms {
            return Err(fail(LuckFamilyDecline::TerminalMapping, "terminal frame is not a complete empty tail"));
        }
        let mut by_id = FxHashMap::default();
        by_id.try_reserve(notes.len()).map_err(|_| fail(LuckFamilyDecline::Capacity, "note identity table"))?;
        for (index, note) in notes.iter().enumerate() {
            if by_id.insert(note.note_id, index).is_some() {
                return Err(fail(LuckFamilyDecline::Context, "duplicate chart note identity"));
            }
        }
        let mut seen = reserve(notes.len())?;
        seen.resize(notes.len(), false);
        let mut inputs = reserve(notes.len())?;
        let mut previous = i32::MIN;
        for (frame_index, (frame, &delta)) in play.frames.iter().zip(deltas).enumerate() {
            if frame_index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            if frame.time_ms < 0 || frame.time_ms <= previous || !delta.is_finite() || delta < 0.0 {
                return Err(fail(LuckFamilyDecline::Context, "frame/delta stream leaves the native DP domain"));
            }
            let mut prior_note = previous;
            for judgement in &frame.judged {
                let Some(&index) = by_id.get(&judgement.note_id) else {
                    return Err(fail(LuckFamilyDecline::Context, "unknown judged note"));
                };
                let note = notes[index];
                if !is_judgement_note(note.note_operate_type) {
                    return Err(fail(
                        LuckFamilyDecline::Context,
                        "declared structural note judgement has no common terminal mapping",
                    ));
                }
                if seen[index]
                    || note.time_ms <= previous
                    || note.time_ms > frame.time_ms
                    || note.time_ms < prior_note
                    || judgement.judgement_time_ms != note.time_ms
                    || !(1..=6).contains(&judgement.judgement)
                {
                    return Err(fail(LuckFamilyDecline::Context, "judgements must be one complete first-due stream"));
                }
                seen[index] = true;
                prior_note = note.time_ms;
                inputs.push(InputNote { note, judgement: judgement.judgement });
            }
            previous = frame.time_ms;
        }
        // The theoretical stream omits structural chart nodes. They remain in `notes`, including the native
        // controller's range targets and the full-chart end/score-frame checks above. Only actual judgement
        // notes require an input; the terminal mapping below still comes from native scored-note filings.
        if notes.iter().zip(&seen).any(|(note, &seen)| is_judgement_note(note.note_operate_type) && !seen) {
            return Err(fail(LuckFamilyDecline::Context, "unjudged judgement notes have no common terminal mapping"));
        }
        // In the reduced native interpreter before/after controller calls pass literal score zero. The native
        // constructor uses total_power only in calc.state.band_total_power. Every remaining field is preserved.
        params.total_power = 0;
        let empty: [Performer; SLOTS] = std::array::from_fn(|_| Performer::default());
        let mut model = LiveModel::new_gekisou(master, &empty, notes, events, params, setup)
            .map_err(|e| source_error(LuckFamilyDecline::Context, e))?;
        super::super::luck_score_bounds::check_recorder(&model, skills)
            .map_err(|e| source_error(LuckFamilyDecline::RecorderAdmission, e))?;
        model.set_luck_weights(skills, Vec::new()).map_err(|e| source_error(LuckFamilyDecline::Context, e))?;
        model.score.begin_bounds(Vec::new(), true);
        model.score.certify_bounds_filings(Some(M_LUCK));
        if !model.try_enable_bounds_record_only() {
            return Err(fail(LuckFamilyDecline::TerminalMapping, "fresh structural recorder was not admitted"));
        }
        for (index, (frame, &delta)) in play.frames.iter().zip(deltas).enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            if index + 1 == play.frames.len()
                && model.gk.as_ref().is_none_or(|gk| gk.ctrl.states.iter().any(|state| state.state != S_FINISH))
            {
                return Err(fail(LuckFamilyDecline::TerminalMapping, "all ranges must finish before the empty tail"));
            }
            model
                .frame_timed(frame.time_ms, &frame.judged, delta)
                .map_err(|e| source_error(LuckFamilyDecline::TerminalMapping, e))?;
        }
        if cancelled() {
            return Ok(None);
        }
        if model.random.draws() != 0 {
            return Err(fail(LuckFamilyDecline::TerminalMapping, "geometry recording consumed a random value"));
        }
        let trace = model
            .score
            .bounds_trace
            .take()
            .ok_or_else(|| fail(LuckFamilyDecline::TerminalMapping, "native query geometry was not recorded"))?;
        let mut ready = i32::MIN;
        let mut last_query = None;
        let mut pending = false;
        let mut queries = 0usize;
        let mut filed = reserve(inputs.len())?;
        for (index, event) in trace.events.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            use super::super::luck_score_bounds::BoundsEvent;
            match event {
                BoundsEvent::Note { frame, note, .. } => filed.push((*frame, note.time_ms)),
                BoundsEvent::ProbabilityReady(time) => ready = ready.max(*time),
                BoundsEvent::Query { time_ms, to } => {
                    last_query = Some((*time_ms, *to, ready));
                    pending = false;
                    queries += 1;
                }
                BoundsEvent::Rank { .. } => pending = true,
                _ => {}
            }
        }
        let Some((at, to, ready)) = last_query else {
            return Err(fail(LuckFamilyDecline::TerminalMapping, "missing terminal query"));
        };
        let query_limit = play.frames.len().saturating_mul(2).saturating_add(setup.fevers.len().saturating_mul(2));
        if pending
            || queries != trace.queries
            || queries > query_limit
            || at != last.time_ms
            || to < 0
            || filed.iter().any(|&(frame, time)| frame as i32 > to || time > ready)
        {
            return Err(fail(LuckFamilyDecline::TerminalMapping, "terminal query has incomplete native filings"));
        }
        let mut times = reserve(filed.len())?;
        times.extend(filed.into_iter().map(|(_, time)| time));
        times.sort_unstable();
        let last_luck_note = setup
            .fevers
            .iter()
            .zip(&setup.missions)
            .filter(|(_, mission)| **mission == M_LUCK)
            .map(|(&(start, end), _)| (start, end))
            .flat_map(|(start, end)| {
                inputs.iter().filter(move |input| start <= input.note.time_ms && input.note.time_ms <= end)
            })
            .map(|input| input.note.time_ms)
            .max()
            .unwrap_or(i32::MIN);
        let mapping =
            Arc::new(DomainTerminalMapping { times, inputs, last_luck_note, frame_work: play.frames.len() as u64 });
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(Self {
            master,
            skills,
            writer_skills: LuckSkills::default(),
            notes,
            events,
            params,
            setup,
            play,
            deltas,
            mapping,
        }))
    }

    /// Check the full allowed pair domain and complete all original order/profile computations. The only
    /// variable controller resource admitted here is one physical Snap whose selected GK sources contain a
    /// writer. Its absent/owner partitions cover every legal binding; multiple writers retain the original path.
    /// No life-reading writer is admitted. Ordinary 15000 changes only ordinary live-pool durations, while the
    /// accepted GK writers have their own range/lot trigger and lifetime and read neither live activity nor score.
    pub fn prepare(
        &self,
        choices: &[Vec<LuckFamilyChoice>; SLOTS],
        curves: Option<&mut LuckDpCache>,
        limits: LuckFamilyLimits,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckControllerFamily>, LuckFamilyError> {
        if cancelled() {
            return Ok(None);
        }
        let pair_count = choices
            .iter()
            .try_fold(0usize, |count, choices| count.checked_add(choices.len()))
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "pair count overflow"))?;
        if pair_count == 0 || pair_count > limits.max_pair_models || limits.max_retained_bytes == 0 {
            return Err(fail(LuckFamilyDecline::Budget, "pair-model or retained-byte budget"));
        }
        let mut base: [Performer; SLOTS] = std::array::from_fn(|_| Performer::default());
        let mut allowed: [Vec<Option<usize>>; SLOTS] = std::array::from_fn(|_| Vec::new());
        let mut resources: Vec<(usize, &Performer)> = reserve(pair_count)?;
        let mut writer = None;
        for slot in 0..SLOTS {
            let Some(empty) = choices[slot].iter().find(|choice| choice.resource.is_none()) else {
                return Err(fail(LuckFamilyDecline::PairDomain, "each slot must include the empty Snap choice"));
            };
            if !empty.performer.support_skills.is_empty() || !empty.performer.gekisou_support_skills.is_empty() {
                return Err(fail(LuckFamilyDecline::PairDomain, "empty resource has support sources"));
            }
            base[slot] = empty.performer.clone();
            allowed[slot] = reserve(choices[slot].len())?;
            for choice in &choices[slot] {
                if member(&choice.performer) != base[slot] || allowed[slot].contains(&choice.resource) {
                    return Err(fail(
                        LuckFamilyDecline::PairDomain,
                        "choice changes fixed member or duplicates a resource",
                    ));
                }
                allowed[slot].push(choice.resource);
                if let Some(resource) = choice.resource {
                    if let Some((_, previous)) = resources.iter().find(|(id, _)| *id == resource) {
                        if previous.support_skills != choice.performer.support_skills
                            || previous.gekisou_support_skills != choice.performer.gekisou_support_skills
                        {
                            return Err(fail(
                                LuckFamilyDecline::PairDomain,
                                "one physical resource has different source programs",
                            ));
                        }
                    } else {
                        resources.push((resource, &choice.performer));
                    }
                    let writes = choice.performer.gekisou_support_skills.iter().any(|&(id, level)| {
                        self.master
                            .gekisou_support_skill_effects
                            .iter()
                            .any(|row| row.skill_id == id && row.level == level && is_luck_chain(row.skill_effect_type))
                    });
                    if writes && writer.is_some_and(|writer| writer != resource) {
                        return Err(fail(LuckFamilyDecline::WriterProfiles, "multiple physical writer resources"));
                    }
                    if writes {
                        writer = Some(resource);
                    }
                }
            }
        }
        let mut edges = [[false; 7]; 7];
        let mut all_edges = [[false; 7]; 7];
        let mut row_maxima = [0usize; SLOTS];
        // Conversion memoizes its targets by the raw effect row ID, across source tables and owners. Keep
        // that whole-domain identity before dropping any late conversion from the judgement graph.
        let mut conversion_targets = Vec::new();
        for slot in 0..SLOTS {
            for choice in &choices[slot] {
                if cancelled() {
                    return Ok(None);
                }
                let mut deck = base.clone();
                deck[slot] = choice.performer.clone();
                let model =
                    LiveModel::new_gekisou(self.master, &deck, self.notes, self.events, self.params, self.setup)
                        .map_err(|e| source_error(LuckFamilyDecline::RecorderAdmission, e))?;
                self.admit_pair(&model, &deck)?;
                remember_conversion_targets(&model, &mut conversion_targets)?;
                let pair_edges = conversion_edges(&model, self.mapping.last_luck_note, self.setup)?;
                let all_pair_edges = conversion_edges(&model, i32::MAX, self.setup)?;
                row_maxima[slot] = row_maxima[slot].max(model.rows.len());
                for from in 1..=6 {
                    for to in 1..=6 {
                        edges[from][to] |= pair_edges[from][to];
                        all_edges[from][to] |= all_pair_edges[from][to];
                    }
                }
            }
        }
        // Every physical completion has at most this conservative row total. There are five native instances
        // per effect and two phases; 32 filings/row/frame exceeds the two-command cumulative replacement and
        // handle lifecycle work. The extra note/frame terms cover controller commands. Thus deleting reward
        // handlers cannot expose a native handle counter wrap, zero sentinel or key collision in the law.
        let row_total = row_maxima
            .into_iter()
            .try_fold(0usize, usize::checked_add)
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "row total overflow"))?;
        let handles = self
            .play
            .frames
            .len()
            .checked_mul(row_total)
            .and_then(|count| count.checked_mul(32))
            .and_then(|count| count.checked_add(self.play.frames.len().saturating_mul(4)))
            .and_then(|count| count.checked_add(self.notes.len().saturating_mul(4)));
        if handles.is_none_or(|count| count > i32::MAX as usize) {
            return Err(fail(LuckFamilyDecline::RecorderAdmission, "native handle counters may wrap"));
        }
        close_conversions(&mut edges);
        close_conversions(&mut all_edges);
        for (index, input) in self.mapping.inputs.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            let from = input.judgement as usize;
            for to in 1..=6 {
                if (to == from || all_edges[from][to])
                    && !self.master.judgement_parameters.iter().any(|row| row.note_simulate_judgement == to as i64)
                {
                    return Err(fail(
                        LuckFamilyDecline::RecorderAdmission,
                        "a possible converted grade has no damage/score row",
                    ));
                }
            }
            if self.setup.fevers.iter().zip(&self.setup.missions).any(|(&(start, end), &mission)| {
                mission == M_LUCK && start <= input.note.time_ms && input.note.time_ms <= end
            }) && (1..=6).any(|to| {
                edges[from][to] && luck::luck_judgement_class(from as i32) != luck::luck_judgement_class(to as i32)
            }) {
                return Err(fail(LuckFamilyDecline::JudgementFeedback, "conversion changes a LUCK controller class"));
            }
        }
        let mut profiles = reserve(SLOTS + 1)?;
        profiles.push(None);
        if let Some(writer) = writer {
            profiles.extend((0..SLOTS).filter(|&slot| allowed[slot].contains(&Some(writer))).map(Some));
        }
        let required = profiles
            .len()
            .checked_mul(ORDERS)
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "order/profile count overflow"))?;
        let frame_work = (required as u64)
            .checked_add(1)
            .and_then(|count| count.checked_mul(self.mapping.frame_work))
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "frame-work overflow"))?;
        if profiles.len() > limits.max_profiles
            || required > limits.max_order_evaluations
            || frame_work > limits.max_frame_work
        {
            return Err(fail(LuckFamilyDecline::Budget, "complete order/profile cover exceeds its work budget"));
        }
        let mut family = LuckControllerFamily {
            mapping: Arc::clone(&self.mapping),
            allowed,
            writer,
            profiles,
            orders: reserve(required)?,
            bytes: 0,
        };
        family.bytes = family.allocated_bytes()?;
        if family.bytes > limits.max_retained_bytes {
            return Err(fail(LuckFamilyDecline::Capacity, "family containers exceed the byte budget"));
        }
        let mut coverage = FamilyCoverage::new(family.profiles.len())?;
        let mut temporary = LuckDpCache::new(0);
        let curves = curves.unwrap_or(&mut temporary);
        // Retain the existing exact recording/curve caches. Capacity zero remains zero; no family cache is
        // introduced here. An interrupted family can leave only already completed individual curves in them.
        let mut recordings = RecordingCache::default();
        for profile in 0..family.profiles.len() {
            let mut physical = base.clone();
            if let Some(owner) = family.profiles[profile] {
                let resource = family.writer.expect("owner profile has a writer resource");
                physical[owner] = choices[owner]
                    .iter()
                    .find(|choice| choice.resource == Some(resource))
                    .expect("profile was built from allowed resources")
                    .performer
                    .clone();
            }
            let physical = physical.map(|performer| projected(self.master, &performer));
            let mut order = [0, 1, 2, 3, 4];
            for ordinal in 0..ORDERS {
                if cancelled() {
                    return Ok(None);
                }
                let deck = order.map(|slot| physical[slot].clone());
                let curve = curves
                    .certified_cancellable(
                        self.master,
                        &self.writer_skills,
                        self.notes,
                        self.events,
                        self.params,
                        self.setup,
                        self.play,
                        self.deltas,
                        &deck,
                        None,
                        None,
                        Some(&mut recordings),
                        &mut cancelled,
                    )
                    .map_err(|e| source_error(LuckFamilyDecline::ProbabilityDomain, e))?;
                let Some(probability) = curve else {
                    return Ok(None);
                };
                let mut positions = [0; SLOTS];
                for (position, &slot) in order.iter().enumerate() {
                    positions[slot] = position;
                }
                if !coverage.mark(profile, ordinal) {
                    return Err(fail(LuckFamilyDecline::IncompleteCoverage, "duplicate original order"));
                }
                // The orders vector was fully reserved; only a previously unreferenced curve can increase
                // retained payload now. Check exact Arc identity once, not every pair on every insertion.
                if !family.orders.iter().any(|old| Arc::ptr_eq(&old.probability, &probability)) {
                    family.bytes = family
                        .bytes
                        .checked_add(curve_bytes(&probability)?)
                        .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "curve payload overflow"))?;
                }
                family.orders.push(LuckFamilyOrderLaw { positions, profile, probability });
                if family.bytes > limits.max_retained_bytes {
                    return Err(fail(LuckFamilyDecline::Capacity, "complete family payload exceeds its byte budget"));
                }
                if next_order(&mut order) != (ordinal + 1 < ORDERS) {
                    return Err(fail(
                        LuckFamilyDecline::IncompleteCoverage,
                        "original order enumeration is incomplete",
                    ));
                }
            }
        }
        if cancelled() {
            return Ok(None);
        }
        if !coverage.complete() || family.orders.len() != required {
            return Err(fail(LuckFamilyDecline::IncompleteCoverage, "incomplete writer/order cover"));
        }
        Ok(Some(family))
    }

    fn admit_pair(&self, model: &LiveModel, deck: &[Performer; SLOTS]) -> Result<(), LuckFamilyError> {
        let gate = super::super::luck_score_bounds::check_recorder(model, self.skills)
            .map_err(|e| source_error(LuckFamilyDecline::RecorderAdmission, e))?;
        if gate.is_some_and(|gate| gate != M_LUCK) {
            return Err(fail(LuckFamilyDecline::RecorderAdmission, "direct probes use a different mission gate"));
        }
        for row in model.luck_score_rows(self.skills) {
            if !matches!(model.rows[row.row].effect_type, 2000 | 2005) {
                return Err(fail(
                    LuckFamilyDecline::RecorderAdmission,
                    "a removed probe is not a direct note-score effect",
                ));
            }
            if !row.value.is_finite() || row.value <= i32::MIN as f32 / 100000f32 {
                return Err(fail(LuckFamilyDecline::RecorderAdmission, "a probe command has no paired signed inverse"));
            }
        }
        for row in &model.rows {
            if row.effect_type == 15000 && row.effect_value < 0 {
                return Err(fail(LuckFamilyDecline::RecorderAdmission, "negative ordinary duration extension"));
            }
            if matches!(row.effect_type, 2004 | 12004 | 12006 | 13005) {
                row.targets().map_err(|e| source_error(LuckFamilyDecline::RecorderAdmission, e))?;
            }
        }
        if model
            .live
            .iter()
            .any(|skill| skill.effects.iter().any(|effect| is_luck_chain(model.rows[effect.row].effect_type)))
        {
            return Err(fail(LuckFamilyDecline::RecorderAdmission, "ordinary live source writes the controller"));
        }
        for skill in &model.live {
            let mut identities = BTreeSet::new();
            for effect in &skill.effects {
                let row = &model.rows[effect.row];
                if !identities.insert(row.id) || !effect.act.is_finite() || effect.act < 0.0 || row.effect_type == 4004
                {
                    return Err(fail(
                        LuckFamilyDecline::RecorderAdmission,
                        "ordinary live identity/lifetime/runtime domain",
                    ));
                }
                for checker in [&effect.condition, &effect.release].into_iter().flatten() {
                    check_runtime_checker(checker)?;
                }
                if let Some(cumulative) = &effect.cumulative {
                    check_runtime_cumulative(cumulative)?;
                }
            }
        }
        let mut identities = BTreeSet::new();
        for skill in &model.cond {
            for effect in skill.updater.effects() {
                let row = &model.rows[effect.row];
                if !(SKILL_TYPE_SUPPORT..=SKILL_TYPE_GEKISOU_SUPPORT).contains(&skill.skill_type)
                    || (row.effect_type == 4004 && skill.skill_type == SKILL_TYPE_SUPPORT)
                {
                    return Err(fail(LuckFamilyDecline::RecorderAdmission, "a source requires the raw runtime"));
                }
                let exact_id = row
                    .id
                    .checked_mul(100)
                    .and_then(|id| skill.skill_type.checked_mul(10).and_then(|kind| id.checked_add(kind)))
                    .and_then(|id| id.checked_add(skill.member as i64));
                if row.id < 0
                    || exact_id != Some(effect.effect_id)
                    || !identities.insert(effect.effect_id)
                    || !effect.act.is_finite()
                    || effect.act < 0.0
                {
                    return Err(fail(
                        LuckFamilyDecline::RecorderAdmission,
                        "effect identity or lifetime is outside the typed domain",
                    ));
                }
                // A timed sustained ordinary updater can exhaust its separate finite queue. This initial
                // domain admits ordinary one-shot and untimed sustained rows; the full scorer retains others.
                if effect.trigger_type == SUSTAINED && effect.act != 0.0 {
                    return Err(fail(
                        LuckFamilyDecline::RecorderAdmission,
                        "timed sustained ordinary queue is not certified",
                    ));
                }
                for checker in [&effect.trigger, &effect.condition, &effect.reset].into_iter().flatten() {
                    check_runtime_checker(checker)?;
                }
                if let Some(cumulative) = &effect.cumulative {
                    check_runtime_cumulative(cumulative)?;
                }
                if is_luck_chain(row.effect_type) {
                    if skill.updater.gate_mission() != Some(M_LUCK) {
                        return Err(fail(
                            LuckFamilyDecline::RecorderAdmission,
                            "a controller writer has no LUCK mission gate",
                        ));
                    }
                    if skill.skill_type == SKILL_TYPE_SUPPORT {
                        return Err(fail(
                            LuckFamilyDecline::RecorderAdmission,
                            "ordinary support source writes the controller",
                        ));
                    }
                    if [&effect.trigger, &effect.condition, &effect.reset]
                        .iter()
                        .any(|checker| checker.as_ref().is_some_and(life_reader))
                        || skill
                            .updater
                            .updaters
                            .iter()
                            .any(|updater| updater.release.as_ref().is_some_and(life_reader))
                    {
                        return Err(fail(LuckFamilyDecline::LifeFeedback, "a controller writer reads life"));
                    }
                }
            }
            for updater in &skill.updater.updaters {
                if let Some(release) = &updater.release {
                    check_runtime_checker(release)?;
                }
            }
        }
        // Compile the complete reduced program, not a hand-written writer signature. This checks exact native
        // phases, trigger/condition/release/reset shapes, support owner, row tables and every probability value.
        let projected = deck.clone().map(|performer| projected(self.master, &performer));
        let prepared = prepare_recording::<ProbabilityMass>(
            self.master,
            &self.writer_skills,
            self.notes,
            self.events,
            self.params,
            self.setup,
            self.play,
            self.deltas,
            &projected,
            None,
            None,
            false,
        )
        .map_err(|e| source_error(LuckFamilyDecline::ProbabilityDomain, e))?;
        if prepared.life.is_some() || prepared.life_deck.is_some() {
            return Err(fail(LuckFamilyDecline::LifeFeedback, "reduced family still requires a life interpreter"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "family/coverage_tests.rs"]
mod coverage_tests;

impl LuckControllerFamily {
    fn allocated_bytes(&self) -> Result<usize, LuckFamilyError> {
        let overflow = || fail(LuckFamilyDecline::Capacity, "retained payload arithmetic overflow");
        let mut bytes = size_of::<Self>()
            .checked_add(2 * size_of::<usize>() + size_of::<DomainTerminalMapping>())
            .ok_or_else(overflow)?
            .checked_add(self.mapping.times.capacity().checked_mul(size_of::<i32>()).ok_or_else(overflow)?)
            .ok_or_else(overflow)?
            .checked_add(self.mapping.inputs.capacity().checked_mul(size_of::<InputNote>()).ok_or_else(overflow)?)
            .ok_or_else(overflow)?
            .checked_add(self.profiles.capacity().checked_mul(size_of::<Option<usize>>()).ok_or_else(overflow)?)
            .ok_or_else(overflow)?
            .checked_add(self.orders.capacity().checked_mul(size_of::<LuckFamilyOrderLaw>()).ok_or_else(overflow)?)
            .ok_or_else(overflow)?;
        for choices in &self.allowed {
            bytes = bytes
                .checked_add(choices.capacity().checked_mul(size_of::<Option<usize>>()).ok_or_else(overflow)?)
                .ok_or_else(overflow)?;
        }
        debug_assert!(self.orders.is_empty(), "curve payloads are accounted once on insertion");
        Ok(bytes)
    }
}

fn curve_bytes(curve: &LuckDpCertifiedResult) -> Result<usize, LuckFamilyError> {
    let overflow = || fail(LuckFamilyDecline::Capacity, "curve payload arithmetic overflow");
    (2 * size_of::<usize>() + size_of::<LuckDpCertifiedResult>())
        .checked_add(curve.steps.capacity().checked_mul(size_of::<(i32, [ProbabilityMass; 4])>()).ok_or_else(overflow)?)
        .ok_or_else(overflow)?
        .checked_add(curve.probes.capacity().checked_mul(size_of::<bool>()).ok_or_else(overflow)?)
        .ok_or_else(overflow)?
        .checked_add(curve.probe_transitions.capacity().checked_mul(size_of::<u8>()).ok_or_else(overflow)?)
        .ok_or_else(overflow)?
        .checked_add(curve.range_moments.capacity().checked_mul(size_of::<LuckRangeMoments>()).ok_or_else(overflow)?)
        .ok_or_else(overflow)
}
