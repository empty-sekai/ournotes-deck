//! All-path enclosures for the native solo score calculator.
//!
//! The recorder supplies a proved deterministic command/query schedule. Possible direct-7021 probe filings and
//! Rush filings are added to it, and [`replay`] encloses the native binary32 factor state of every path through
//! that schedule. A note links its chart-time joint masses once the native frame has filed its lottery-dependent
//! commands. Earlier queries keep support bounds. Combo histories retain every possible prior execution,
//! replacing their hull only on a proved mandatory replay. This is not an exact expectation or search completion.

use super::*;
use crate::live::certified::{F32Interval, F64Interval, I32Interval, ProbabilityMass, rank_mean_bounds};
use crate::live::score::get_luck_factor_percent;
use serde::Serialize;

mod bounds;
mod exact;
mod exact_orders;
#[cfg(test)]
mod exact_orders_tests;
mod exact_paths;
#[cfg(test)]
mod exact_tests;
mod joint;
#[cfg(test)]
mod joint_tests;
#[cfg(test)]
mod owner_order_tests;
#[cfg(test)]
mod probe_lifecycle_tests;
#[cfg(feature = "search-diagnostics")]
mod profile;
mod rank_residues;
#[cfg(test)]
mod rank_residues_tests;
mod recorder_checks;
mod replay;
#[cfg(test)]
mod tests;
mod trace;
pub(super) use bounds::complete_bounds_recording;
use bounds::{checked_support, luck_score_bounds_internal};
pub use exact::{LuckExactNote, LuckExactProfile, LuckExactRange, LuckExactScore, luck_exact_score};
pub use exact_orders::{
    LuckExactOrders, LuckExactOrdersProfile, lottery_free_orders, luck_exact_orders, luck_exact_orders_power_twin,
    luck_exact_orders_with_laws, luck_order_laws,
};
pub use joint::luck_joint_score;
#[cfg(feature = "search-diagnostics")]
pub use profile::{LuckScoreProfile, take_luck_score_profile};
pub use rank_residues::luck_exact_score_refined;
pub(super) use recorder_checks::{
    bind_probe_phase, check_conditioned_recorder, check_recorder, deterministic, deterministic_cumulative,
    probes_in_native_order,
};
use recorder_checks::{check_probe_filings, check_probe_music_boundary};
pub(super) use trace::{BoundsEvent, BoundsTrace};

const FIELDS: usize = 6;

pub(super) use replay::ProbeRow;
use replay::{Classes, Replay};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct RealBounds {
    pub lower: f64,
    pub upper: f64,
}

impl From<F64Interval> for RealBounds {
    fn from(value: F64Interval) -> Self {
        Self { lower: value.lower(), upper: value.upper() }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct IntegerBounds {
    pub lower: i32,
    pub upper: i32,
}

impl From<I32Interval> for IntegerBounds {
    fn from(value: I32Interval) -> Self {
        Self { lower: value.lower(), upper: value.upper() }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckNoteBounds {
    pub note_id: i32,
    pub time_ms: i32,
    pub life: i32,
    /// Conditional integer supports for the factors visible on the note's last execution: 00/01/10/11 (probe
    /// bit 0, Rush bit 1); None when no path reaches the bucket's probe class.
    pub buckets: [Option<IntegerBounds>; 4],
    pub combo: RealBounds,
    /// Present once all lottery-dependent commands at this note's chart time have been filed.
    pub probability: Option<[RealBounds; 4]>,
    pub mean: Option<RealBounds>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreQueryBounds {
    pub time_ms: i32,
    pub score_frame: i32,
    /// Probability-linked notes and the support bounds of notes whose lottery filing is still pending.
    pub mean: RealBounds,
    pub support: IntegerBounds,
    /// [combo, note, Just, Perfect, Great, Good]: the widest enclosure of a measured note's factor over all paths.
    pub factor_width: [f64; FIELDS],
    pub note_count: usize,
    /// Detailed cells are retained only for actual range snapshot queries; other queries keep aggregates.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<LuckNoteBounds>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckRangeScoreBounds {
    pub range: usize,
    pub start_query: Option<usize>,
    pub end_query: usize,
    pub percent: i64,
    pub mean: RealBounds,
    pub support: IntegerBounds,
    pub bonus_mean: RealBounds,
    pub bonus_support: IntegerBounds,
    pub luck_points_mean: Option<RealBounds>,
    /// Expected Miss, Hit, Super Hit and Critical counts.
    pub lot_results_mean: Option<[RealBounds; 4]>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreBounds {
    pub model: &'static str,
    pub queries: Vec<LuckScoreQueryBounds>,
    pub ranges: Vec<LuckRangeScoreBounds>,
    pub final_mean: RealBounds,
    pub final_note_mean: RealBounds,
    pub final_rank_mean: RealBounds,
    pub final_support: IntegerBounds,
    /// Constant across the admitted lottery paths; established by `check_recorder`'s life dependency closure.
    pub exact_final_life: Option<i32>,
    pub final_notes: Vec<LuckNoteBounds>,
    pub query_limit: u64,
    pub actual_queries: usize,
    pub probability_peak_states: usize,
    pub probability_transitions: u64,
}

/// Compact production result. A singleton *native integer support* proves a constant score; a narrow
/// floating expectation interval alone never becomes an exact rational or a completion certificate.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreSummary {
    pub final_mean: RealBounds,
    pub final_support: IntegerBounds,
    pub exact_constant_score: Option<i32>,
    /// Some only when every admitted path has this same native terminal life.
    pub exact_final_life: Option<i32>,
    pub probability_peak_states: usize,
    pub probability_transitions: u64,
}

fn refuse(why: &str) -> Error {
    Error::Unsupported(format!("native LUCK score bounds: {why}"))
}

/// Run a diagnostic certificate of native score expectations under the independent nominal lottery law.
/// Preconditions are checked on the complete physical deck: deterministic ordinary score/life rows,
/// the reduced DP's admitted lottery rows and untimed direct-7021 score rows; a fully finished solo schedule.
/// Widths remain explicit. This function never constructs an ExactExpectation or a search completion state.
#[allow(clippy::too_many_arguments)]
pub fn luck_score_bounds(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
) -> Result<LuckScoreBounds, Error> {
    luck_score_bounds_with_ranking(master, deck, notes, events, params, setup, play, delta_times, None)
}

/// The same native arithmetic enclosure using an explicitly declared external rank-confirmation timeline.
/// External ranks use the controller's actual frame score snapshots, without solo timestamp rewinds.
#[allow(clippy::too_many_arguments)]
pub fn luck_score_bounds_with_ranking(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckScoreBounds, Error> {
    let skills = luck_skills(master)?;
    luck_score_bounds_internal(
        master,
        &skills,
        deck,
        notes,
        events,
        params,
        setup,
        play,
        delta_times,
        ranking,
        true,
        None,
        None,
        &mut || false,
        false,
        None,
    )
    .map(|value| value.expect("complete score bounds"))
}

/// The same arithmetic and native snapshot proof without retaining diagnostic query/note reports. `skills` is
/// [`luck_skills`] of `master`.
#[allow(clippy::too_many_arguments)]
pub fn luck_score_summary_with_ranking(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckScoreSummary, Error> {
    luck_score_summary_with_curves(master, skills, deck, notes, events, params, setup, play, delta_times, ranking, None)
}

/// [`luck_score_summary_with_ranking`] whose lottery curve comes from, and enters, `curves` when given.
#[allow(clippy::too_many_arguments)]
pub fn luck_score_summary_with_curves(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: Option<&mut LuckDpCache>,
) -> Result<LuckScoreSummary, Error> {
    let mut session = LuckScoreSession::new(master, skills, notes, events, params, setup, play, delta_times, ranking);
    Ok(session.summary(deck, curves, || false)?.expect("complete score summary"))
}

/// Certified score evaluations sharing one immutable master and live context. The session reuses a lottery
/// recording when the compiled effect rows, resolved checkers, life interpreter and probe flags coincide.
/// Each returned summary evaluates the requested deck's complete score command schedule.
pub struct LuckScoreSession<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    notes: &'a [LiveNote],
    events: &'a [(i32, i32)],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    delta_times: &'a [f32],
    ranking: Option<&'a [crate::replay::RankConfirmation]>,
    recordings: luck_dp::RecordingCache,
}

impl<'a> LuckScoreSession<'a> {
    /// Fix the context whose frames and lottery probabilities every evaluation uses.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        master: &'a Master,
        skills: &'a LuckSkills,
        notes: &'a [LiveNote],
        events: &'a [(i32, i32)],
        params: LiveParams,
        setup: &'a GekisouSetup,
        play: &'a LivePlay,
        delta_times: &'a [f32],
        ranking: Option<&'a [crate::replay::RankConfirmation]>,
    ) -> Self {
        Self {
            master,
            skills,
            notes,
            events,
            params,
            setup,
            play,
            delta_times,
            ranking,
            recordings: Default::default(),
        }
    }

    /// Complete all-path bounds for `deck`, or `None` when cancellation interrupts a frame or replay step.
    /// The optional curve cache bounds the session's recorder-key storage to at most one MiB and 32 entries.
    pub fn summary(
        &mut self,
        deck: &[Performer],
        curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckScoreSummary>, Error> {
        let bounds = luck_score_bounds_internal(
            self.master,
            self.skills,
            deck,
            self.notes,
            self.events,
            self.params,
            self.setup,
            self.play,
            self.delta_times,
            self.ranking,
            false,
            curves,
            Some(&mut self.recordings),
            &mut cancelled,
            false,
            None,
        )?;
        Ok(bounds.map(|bounds| LuckScoreSummary {
            final_mean: bounds.final_mean,
            final_support: bounds.final_support,
            exact_constant_score: (bounds.final_support.lower == bounds.final_support.upper)
                .then_some(bounds.final_support.lower),
            exact_final_life: bounds.exact_final_life,
            probability_peak_states: bounds.probability_peak_states,
            probability_transitions: bounds.probability_transitions,
        }))
    }
}

/// The expected final score and range scores of a solo play under the independent nominal lottery law.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreExpectation {
    pub final_mean: RealBounds,
    /// A singleton proves a constant score.
    pub final_support: IntegerBounds,
    /// One per Gekisou range, in range order: its score and rank bonus.
    pub ranges: Vec<LuckRangeScoreBounds>,
}

/// [`luck_score_summary_with_ranking`] of a solo play with the range enclosures. `skills` is [`luck_skills`] of
/// `master`.
#[allow(clippy::too_many_arguments)]
pub fn luck_score_expectation(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
) -> Result<LuckScoreExpectation, Error> {
    luck_score_expectation_with_curves(
        master,
        skills,
        deck,
        notes,
        events,
        params,
        setup,
        play,
        delta_times,
        None,
        None,
    )
}

/// Range expectations under an explicit rank-arrival timeline, sharing certified lottery curves when supplied.
#[allow(clippy::too_many_arguments)]
pub fn luck_score_expectation_with_curves(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: Option<&mut LuckDpCache>,
) -> Result<LuckScoreExpectation, Error> {
    let mut bounds = luck_score_bounds_internal(
        master,
        skills,
        deck,
        notes,
        events,
        params,
        setup,
        play,
        delta_times,
        ranking,
        false,
        curves,
        None,
        &mut || false,
        false,
        None,
    )?
    .expect("complete score expectation");
    bounds.ranges.sort_by_key(|r| r.range);
    if bounds.ranges.len() != setup.fevers.len() || bounds.ranges.iter().enumerate().any(|(i, r)| r.range != i) {
        return Err(refuse("a Gekisou range has no rank enclosure"));
    }
    Ok(LuckScoreExpectation {
        final_mean: bounds.final_mean,
        final_support: bounds.final_support,
        ranges: bounds.ranges,
    })
}

/// Chart measurements vary ordinary score-only rows while retaining this curve's lottery dependencies.
/// The caller keeps the formation, play, power and rank timeline identical to the curve's inputs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn luck_score_expectation_for_chart(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    probability: std::sync::Arc<LuckDpCertifiedResult>,
) -> Result<LuckScoreExpectation, Error> {
    let mut bounds = luck_score_bounds_internal(
        master,
        skills,
        deck,
        notes,
        events,
        params,
        setup,
        play,
        delta_times,
        ranking,
        false,
        None,
        None,
        &mut || false,
        true,
        Some(probability),
    )?
    .expect("complete score expectation");
    bounds.ranges.sort_by_key(|r| r.range);
    if bounds.ranges.len() != setup.fevers.len() || bounds.ranges.iter().enumerate().any(|(i, r)| r.range != i) {
        return Err(refuse("a Gekisou range has no rank enclosure"));
    }
    Ok(LuckScoreExpectation {
        final_mean: bounds.final_mean,
        final_support: bounds.final_support,
        ranges: bounds.ranges,
    })
}

/// Prepares a live for a run that draws no random value. Without Gekisou, all ordinary skill predicates
/// must be independent of probability and lottery state. With Gekisou, the live must have no LUCK range. A deck that reads no lottery
/// or probability is left as it is; any other deck becomes a LUCK weighted live without probabilities
/// ([`LiveModel::set_luck_weights`] with no steps). Without a LUCK range the controller consumes no lottery
/// (`luck_score_bounds_internal`): a probability predicate gates only luck chain effects, which write the unread luck
/// machine, and lottery-dependent score-ups, which need a running rush. The recorder conditions admit no other reader
/// of these states. The prepared live therefore has the native score, judgements, life and rank arrivals of every
/// seed. Call it before the first frame.
pub fn prepare_lottery_free(model: &mut LiveModel, skills: &LuckSkills) -> Result<(), Error> {
    let Some(gk) = model.gk.as_ref() else {
        return if model.reads_lottery() {
            Err(refuse("ordinary skill conditions read an unproved probability or lottery state"))
        } else {
            Ok(())
        };
    };
    if gk.ctrl.ranges.iter().any(|range| range.mission == gekisou::M_LUCK) {
        return Err(refuse("a LUCK range draws lotteries"));
    }
    if !model.reads_lottery() {
        return Ok(());
    }
    check_recorder(model, skills)?;
    model.set_luck_weights(skills, Vec::new())
}
