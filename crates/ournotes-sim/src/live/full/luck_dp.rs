//! A deck-local lottery curve under independent nominal lottery/skill probabilities.
//!
//! The fast entry approximates probabilities numerically; the certified entry encloses them. Neither
//! computes whole-score paths. Binary32 gauge factors and native range/
//! effect timing are recorded by the reduced native interpreter; only the small lottery state is propagated.
//! The seeded System.Random streams are not independent draws over the finite seed space. No claim of exact
//! averaging over all seeds is made. The diagnostic entry uses binary64 accumulation and binary32 output;
//! the certified entry propagates outward binary64 bounds from original integer weights and binary32 chances.
//!
//! The supported mechanics are range-start/playing gauge speed, probabilistic start gauge additions, one-draw
//! start guarantees, a once-per-range Miss gauge addition, and Critical bonus points (which do not feed back).
//! Supported score probes are untimed sustained direct 7021 score-ups. Other shapes fail explicitly.

use super::gekisou::{LotteryMachine, LuckScore, M_LUCK, S_COMPLETE, S_END, S_FINISH, S_PLAYING, S_START};
use super::*;
use crate::live::certified::{F64Interval, ProbabilityMass};
use crate::num::{FxHashMap, floor_to_i32};

mod cache;
mod dp;
mod life_recorder;
mod mass;
mod plan;
mod record;
mod state_identity;
mod transcript;
pub(crate) use cache::OrderClass;
pub(super) use cache::RecordingCache;
pub use cache::{LuckDpCache, LuckDpCacheStats, LuckOrderClassStats, LuckOrderClasses};
use dp::{Distribution, DpResult, Phase, propagate, propagate_cancellable};
use life_recorder::life_recorder;
pub use life_recorder::luck_has_judgement_conversion;
pub(crate) use life_recorder::luck_play_converts;
use mass::{Draws, Mass};
pub(super) use plan::fixed_condition;
use plan::{Action, Plan, chance, compile, reads_life, unsupported};
pub use record::{LuckRecordProfile, take_luck_record_profile};
use record::{
    PreparedRecording, Skeleton, check_recording, file, prepare_recording, record, record_skeleton, reduced_deck,
    skeleton_key, timed,
};
use state_identity::check_held_state_identities;
pub(super) use state_identity::check_model_state_identities;
use transcript::{Frame, Hit, Judged, Transcript, action_words};
#[cfg(test)]
mod tests;

/// A complete nominal lottery curve and the size of its sparse computation.
#[derive(Clone, Debug)]
pub struct LuckDpResult {
    pub steps: Vec<(i32, Vec<f32>)>,
    pub peak_states: usize,
    pub transitions: u64,
}

/// Outward probability bounds under independent nominal lotteries. This certifies lottery-state
/// probabilities only; a whole-score certificate also needs the game's binary32/floor score pipeline.
#[derive(Clone, Debug)]
pub struct LuckDpCertifiedResult {
    /// At each original play frame's skill boundary, the possible old/new direct-probe classes.
    /// Bit `2 * old + new` denotes a transition with positive possible mass. Quiet and repeated frames
    /// each retain their own entry.
    pub probe_transitions: Vec<u8>,
    /// For each original play frame, the possible changes of the Rush handle. Bit `4 * before + 2 * after + filed`
    /// denotes a transition with positive possible mass of the frame's lotteries, from the Rush once the frame's
    /// start is processed to the Rush at the frame's end, where `filed` says whether a lottery started or ended a
    /// Rush on the way. [`RUSH_FINISH`] marks a frame whose start finishes a range and disables an active Rush, and
    /// [`RUSH_PROBE`] a frame after whose skill boundary every state's direct-probe class equals its Rush. Quiet
    /// and repeated frames each retain their own entry.
    pub rush_transitions: Vec<u16>,
    /// Joint probabilities in the order [neither, score only, Rush only, Rush and score]. Every bucket is
    /// accumulated directly from mutually exclusive DP states, never by subtracting rounded marginals.
    pub steps: Vec<(i32, [ProbabilityMass; 4])>,
    /// For each play frame that judges notes, the joint probabilities, in the order of [`Self::steps`], of what
    /// those notes read when a score query of that same frame runs before the frame's lottery: a note before the
    /// frame's time, a note at the frame's time after the frame's skill boundary, and a note at the frame's time
    /// before it.
    pub frame_queries: Vec<(i32, [[ProbabilityMass; 4]; 3])>,
    /// All supported score probes use the same 7021 predicate; this identifies the shapes with a holder.
    pub probes: Vec<bool>,
    /// The expected number of play frames whose skill boundary turns the direct-probe class on. A frame counts in
    /// [`Self::probe_transitions`] when this happens with positive possible mass.
    pub probe_starts: F64Interval,
    /// Optional additive indicators, one per range when requested; empty for score-only curves.
    pub range_moments: Vec<LuckRangeMoments>,
    pub peak_states: usize,
    pub transitions: u64,
}

/// Expectations of additive lottery indicators under the independent nominal law.
#[derive(Clone, Copy, Debug)]
pub struct LuckRangeMoments {
    pub luck_points: F64Interval,
    /// Expected result counts in Miss, Hit, Super Hit, Critical order.
    pub lot_results: [F64Interval; 4],
}

impl Default for LuckRangeMoments {
    fn default() -> Self {
        Self { luck_points: F64Interval::ZERO, lot_results: [F64Interval::ZERO; 4] }
    }
}

/// See [`LuckDpCertifiedResult::rush_transitions`].
pub const RUSH_FINISH: u16 = 1 << 8;
/// See [`LuckDpCertifiedResult::rush_transitions`].
pub const RUSH_PROBE: u16 = 1 << 9;

/// A set of pairs of performer positions below [`PAIR_POSITIONS`], bit `PAIR_POSITIONS * a + b` for `a < b`.
pub(crate) type PositionPairs = u64;

pub(crate) const PAIR_POSITIONS: usize = 8;

/// The pairs of `position` with each other position of `positions` (a bit per position).
fn pairs_with(position: usize, positions: u8) -> PositionPairs {
    (0..PAIR_POSITIONS)
        .filter(|&other| other != position && positions & (1 << other) != 0)
        .fold(0, |pairs, other| pairs | 1 << (PAIR_POSITIONS * position.min(other) + position.max(other)))
}

/// Compute the same `(note chart time, [rush, score_0, rush_and_score_0, ...])` curve as `luck_rush_samples`.
/// This prototype requires the theoretical-play scheduling contract: each note is judged in the first declared
/// frame at or after its chart time, with ascending chart times inside a frame. Cross-frame early/late judgements
/// and overlapping Luck ranges are explicitly unsupported rather than approximated.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
) -> Result<LuckDpResult, Error> {
    luck_rush_dp_with_events(master, skills, notes, &[], params, setup, play, delta_times, deck, probes)
}

/// Nominal diagnostic curve including the complete deterministic life/converted-judgement schedule.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_with_events(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
) -> Result<LuckDpResult, Error> {
    luck_rush_dp_with_ranking(master, skills, notes, skill_events, params, setup, play, delta_times, deck, probes, None)
}

/// Nominal diagnostic curve under the caller's explicit external rank-arrival scenario.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_with_ranking(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckDpResult, Error> {
    let result = run::<f64>(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        ranking,
        false,
    )?;
    Ok(LuckDpResult { steps: result.steps, peak_states: result.peak_states, transitions: result.transitions })
}

/// Run the same supported transition graph with outward probability arithmetic, emitting the four
/// joint Rush/7021 states directly. Probability sources are original integer lottery weights and exact
/// binary32 skill chances. This does not certify the subsequent whole-score calculation.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_certified(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
) -> Result<LuckDpCertifiedResult, Error> {
    luck_rush_dp_certified_with_events(master, skills, notes, &[], params, setup, play, delta_times, deck, probes)
}

/// Certified curve including native skill events, life commands and the two-phase life checks.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_certified_with_events(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
) -> Result<LuckDpCertifiedResult, Error> {
    luck_rush_dp_certified_with_ranking(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        None,
    )
}

/// Certified probabilities under an explicit external rank-arrival scenario. Retained lottery effects
/// read no rank/score counters; the deterministic life recorder uses the same native external lifecycle.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_certified_with_ranking(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckDpCertifiedResult, Error> {
    certified_mode(master, skills, notes, skill_events, params, setup, play, delta_times, deck, probes, ranking, false)
}

/// Certified curve with expected additive range points and consumed result counts.
#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_certified_with_moments(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckDpCertifiedResult, Error> {
    certified_mode(master, skills, notes, skill_events, params, setup, play, delta_times, deck, probes, ranking, true)
}

/// Chart measurements persist the propagation by the complete recorded transcript. Recording still validates
/// the current dependency closure; a hit may share the same curve between different ordinary score programs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn luck_rush_dp_certified_with_moments_cached(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    cache: &crate::chartstats::ChartStatsCache,
) -> Result<LuckDpCertifiedResult, Error> {
    let transcript = record::<ProbabilityMass>(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        ranking,
        true,
    )?;
    let identity = transcript.key().map(|words| words.into_iter().flat_map(u64::to_le_bytes).collect::<Vec<_>>());
    cache.curve(identity.as_deref(), play.frames.len(), transcript.probes.len(), setup.fevers.len(), || {
        let result = propagate(&transcript)?;
        Ok(LuckDpCertifiedResult {
            probe_transitions: result.probe_transitions,
            rush_transitions: result.rush_transitions,
            steps: result.steps,
            frame_queries: result.frame_queries,
            probes: result.probes,
            probe_starts: result.probe_starts,
            range_moments: result.range_moments,
            peak_states: result.peak_states,
            transitions: result.transitions,
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn certified_mode(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    collect_moments: bool,
) -> Result<LuckDpCertifiedResult, Error> {
    let result = run::<ProbabilityMass>(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        ranking,
        collect_moments,
    )?;
    Ok(LuckDpCertifiedResult {
        probe_transitions: result.probe_transitions,
        rush_transitions: result.rush_transitions,
        steps: result.steps,
        frame_queries: result.frame_queries,
        probes: result.probes,
        probe_starts: result.probe_starts,
        range_moments: result.range_moments,
        peak_states: result.peak_states,
        transitions: result.transitions,
    })
}

#[allow(clippy::too_many_arguments)]
fn run<M: Mass>(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    collect_moments: bool,
) -> Result<DpResult<M::Weights>, Error> {
    propagate(&record::<M>(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        ranking,
        collect_moments,
    )?)
}
