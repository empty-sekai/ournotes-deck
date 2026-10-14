//! Whole-live simulation, frame by frame, with live skills, snap (support) skills and, optionally, Gekisou.
//!
//! Each frame, in order: with Gekisou, the fevers advance and the Gekisou ranges step their state machine; the
//! chart's skill events whose time has come fire (each once); the frame's judged notes, in stream order, go through
//! judgement conversion, the combo counter (at their chart time), note damage and the note score command (with the
//! life at their chart time frozen into it); the score is brought to the frame time; live skills triggered by the
//! fired events restart; skills update in phase 1 then phase 2 (live skills by ascending skill key, then condition
//! skills: snap skills in deck order, Gekisou skills grouped by mission type, Gekisou support skills), each phase
//! followed by its appliers in list order; the current life is synced and the score is brought to the frame time
//! again; with Gekisou, the ranges take the frame's judged notes (combo, Just count, luck lottery) and a range that
//! completed gets its rank bonus.
//!
//! A command that lands in a 40 ms frame the score has already executed (a late judgement, a skill starting at its
//! trigger time) undoes the frames down to it and executes them again; the factor state after such a rewind is the
//! game's, which can differ in the last bits from applying every command once. The life keeps the game's frame
//! cache, which a command in a completed frame fully invalidates. Confirming a range's rank bonus computes
//! the score at the range's start and end, which undoes the frames after the end; they are executed again with the
//! next frame.
//!
//! Appliers are chosen by effect type alone, as the game's applier container is: live, snap, Gekisou and Gekisou
//! support skills (with any cumulative condition; live skills keep one counter per effect state) share 2000 (note
//! score up), 2001 / 2003 (cumulative note / combo score up), 2002 (combo score up), 2004 (judgement score up), 2005
//! (note score down), 3000 (life limit), 3001 (life recovery with over-heal), 3002 (safe skill damage), 3003 (damage
//! guard), 3004 (damage reduction), 12006 (judgement conversion, with a conversion limit) and 15000 (extension of the
//! member's running live skills); with Gekisou on also 11000..=11005 (luck), 12000, 12002..=12004 (Gekisou combo),
//! 13000, 13002..=13005 (Just count and Just conversion); without Gekisou those types have no applier and do nothing.
//! 0, 1000..=1003 and 1500..=1503 have no live applier either. Gekisou and Gekisou support skills treat 4004 as a
//! no-op (judgement windows act on the judgement stream). Every condition type and every cumulative condition type
//! is built (the Gekisou ones fail when asked in a live without Gekisou, except 7000, which never hits there). A
//! Gekisou skill of mission All (4) takes the Combo list's place and passes every trigger gate; more than three
//! fevers give three ranges and fail when the fourth fever starts. 13001 (Just windows) is a Gekisou applier too:
//! without Gekisou it does nothing.
//!
//! Raw judgement input (optional, [`LiveModel::enable_raw_runtime`]): FT results go through the conversion, the
//! Assist hook and the diff converter note by note ([`LiveModel::submit_raw_judgement`]); after FT the Assist level
//! updates, then the executor consumes the results with the 4000..=4003 / 13001 window limit callbacks, then the
//! skills update, where 4000..=4004 and 13001 drive the mutable judgement windows. Without the runtime these effect
//! types are [`Error::Unsupported`] (4004 of Gekisou and Gekisou support skills keeps its no-op on a judged stream).

mod applier_plan;
mod chart_cache_identity;
pub(crate) use chart_cache_identity::{chart_program_identity, initialized_chart_program_identity};
mod combo;
#[cfg(feature = "search-diagnostics")]
pub use applier_plan::with_applier_plan_disabled;
mod conditions;
mod convert;
mod engine;
#[cfg(any(test, feature = "search-diagnostics"))]
mod idle_plan;
#[cfg(feature = "search-diagnostics")]
pub use idle_plan::{IdlePlanStats, take_idle_plan_stats, with_idle_plan_disabled};
mod gekisou;
pub use gekisou::{
    BrokenGekisouRanking, NetworkGekisouRanking, NetworkGekisouResult, calculate_network_gekisou_ranking,
    gekisou_no_input,
};
mod life;
mod luck;
mod luck_dp;
pub(crate) use luck_dp::luck_rush_dp_certified_with_moments_cached;
mod luck_exact;
pub use luck_dp::{
    KindCensus, LotteryBreak, LotteryKind, LotteryKindStats, LotteryKinds, LotteryLive, LotteryRole, LuckDpCache,
    LuckDpCacheStats, LuckDpCertifiedResult, LuckDpResult, LuckRangeMoments, LuckRecordProfile,
    luck_has_judgement_conversion, luck_play_changes_judgements, luck_rush_dp, luck_rush_dp_certified,
    luck_rush_dp_certified_with_events, luck_rush_dp_certified_with_moments, luck_rush_dp_certified_with_ranking,
    luck_rush_dp_with_events, luck_rush_dp_with_ranking, take_luck_record_profile,
};
pub use luck_exact::{
    LuckExactAtom, LuckExactAttempt, LuckExactBudget, LuckExactDecline, LuckExactLaw, LuckExactMass, LuckExactSession,
    LuckExactStats, luck_exact_law_with_ranking,
};
mod luck_score_bounds;
pub(crate) use luck_score_bounds::IntegerBounds;
pub(crate) use luck_score_bounds::luck_score_expectation_for_chart;
mod nominal_expectation;
pub use luck_score_bounds::{
    LuckExactNote, LuckExactOrders, LuckExactOrdersProfile, LuckExactProfile, LuckExactRange, LuckExactScore,
    LuckRangeScoreBounds, LuckScoreBounds, LuckScoreExpectation, LuckScoreSession, LuckScoreSummary, RealBounds,
    lottery_free_orders, luck_exact_orders, luck_exact_orders_power_twin, luck_exact_orders_with_laws,
    luck_exact_score, luck_exact_score_refined, luck_joint_score, luck_order_laws, luck_score_bounds,
    luck_score_bounds_with_ranking, luck_score_expectation, luck_score_expectation_with_curves,
    luck_score_summary_with_curves, luck_score_summary_with_ranking, prepare_lottery_free,
};
#[cfg(feature = "search-diagnostics")]
pub use luck_score_bounds::{LuckScoreProfile, take_luck_score_profile};
pub(crate) use nominal_expectation::{has_nominal_score_probabilities, nominal_score_expectation_for_chart};
mod orders;
#[cfg(feature = "search-diagnostics")]
#[doc(hidden)]
pub use luck::LuckSignature;
#[cfg(feature = "search-diagnostics")]
#[doc(hidden)]
pub use luck::{RushDecline, rush_frames};
#[doc(hidden)]
pub use luck::{RushMasks, luck_judgement_class, luck_rush_samples, luck_signature, rush_branches_why};
mod luck_shapes;
pub use luck_shapes::{
    FORMATION, LOTTERY_CONDITIONS, LUCK_REPLAY_CONDITIONS, LuckCondition, LuckScoreShape, LuckShapeProbe, LuckSkillKey,
    LuckSkills, LuckSource, formation_targets, is_luck_chain, luck_holder, luck_skill_key, luck_skills,
};
pub use orders::{MaximumOrdersOutcome, OrderSharing, OrderedLive, OrdersOutcome, RecordedOrder};
mod raw_runtime;
mod replay_rank;
mod score_program;
pub use raw_runtime::{RELAX_TARGET_JUDGEMENTS, RawJudgedNote, RawJudgementRuntime};
pub use score_program::ScoreProgram;
mod range_frames;
mod scorecalc;
pub use range_frames::RangeFrames;
mod access;
mod apply;
mod apply_gekisou;
mod build;
mod condition_rows;
mod effects;
mod frames;
pub(crate) use condition_rows::LuckScoreRow;
use condition_rows::{
    CondRow, GEKISOU_APPLIER_TYPES, condition_skill, effect_phases, gekisou_conditions, gekisou_mission_group,
    lottery_conditions, lottery_conditions_read, luck_score_rows, setting,
};
use effects::{
    CondSkill, EffectRow, FrameScratch, GekisouLive, GkAppliers, Handle, Listed, LiveEffect, LivePool, LiveSkill,
    ScoreCtl, StateKey, aggregate_live_state,
};

use std::collections::{HashMap, VecDeque};

use conditions::{CheckCtx, Checker, Cumulative, Factory, GkView};
use convert::{Conversion, ConvertEffect};
use engine::{
    CondEffect, ConditionSkillUpdater, END_FRAME, EXECUTE_FRAME, EXECUTING, EffectState, FrameInput, FrameSleep,
    ONE_SHOT, STAY, SUSTAINED, TriggerResult, UpdateCheckers, effect_update,
};
use gekisou::{Controller, Env, FeverUpdater, GkNote, LuckHandle, S_COMPLETE};
use life::LifeController;
use scorecalc::{IncrementalCalculator, NoteCommand};

use crate::error::Error;
use crate::live::random::LiveRandom;
use crate::live::score::{
    ComboTable, GekisouComboInfo, LiveScoreCalculator, LiveScoreSettings, LuckWeights, ScoreFactorState,
    convert_score_type, get_frame,
};
use crate::live::skill::{FactorCommand, OWNER_MEMBER, OWNER_SNAP, judgement_factor_mill, note_factor_mill};
use crate::master::{GekisouSkillEffectRow, Master, SupportSkillEffectRow};
use crate::num::{FxHashMap, FxHashSet, floor_to_i32};

/// Skill type digits in effect keys.
const SKILL_TYPE_SUPPORT: i64 = 3;
const SKILL_TYPE_GEKISOU: i64 = 4;
const SKILL_TYPE_GEKISOU_SUPPORT: i64 = 5;
/// Skill update phases, in order.
const PHASES: [i64; 2] = [1, 2];

/// One performance position of the deck, in skill order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Performer {
    /// The member's live skill `(id, level)`.
    pub live_skill: Option<(i64, i64)>,
    /// The paired snap's support skills `(id, level)`.
    pub support_skills: Vec<(i64, i64)>,
    /// Member attributes read by member targets.
    pub band_id: i64,
    pub character_id: i64,
    pub card_type: i64,
    /// Tags and categories exposed by the member card's skill target view.
    pub tag_ids: Vec<i64>,
    pub live_skill_categories: Vec<i64>,
    pub gekisou_skill_categories: Vec<i64>,
    pub gekisou_mission_type: i64,
    /// The member's Gekisou skill `(id, level)`; used only with Gekisou.
    pub gekisou_skill: Option<(i64, i64)>,
    /// The paired snap's Gekisou support skills `(id, level)`; used only with Gekisou and a Gekisou skill.
    pub gekisou_support_skills: Vec<(i64, i64)>,
}

impl Performer {
    /// Matches the live skill member-target predicate, not the power/leader target predicate.
    #[doc(hidden)]
    pub fn matches_skill_target(&self, target: &crate::master::SkillTargetRow) -> bool {
        let categories_match = |targets: &[i64], member: &[i64]| {
            targets.iter().any(|&category| category != 0 && member.contains(&category))
        };
        (target.band_id > 0 && target.band_id == self.band_id)
            || (target.card_type != 0 && target.card_type == self.card_type)
            || (target.character_id > 0 && target.character_id == self.character_id)
            || (target.tag_id > 0 && self.tag_ids.contains(&target.tag_id))
            || categories_match(&target.live_skill_categories, &self.live_skill_categories)
            || categories_match(&target.gekisou_skill_categories, &self.gekisou_skill_categories)
            || (target.gekisou_mission_type != 0 && target.gekisou_mission_type == self.gekisou_mission_type)
    }
}

/// A chart note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveNote {
    pub note_id: i32,
    pub time_ms: i32,
    pub note_operate_type: i32,
    /// Note judgement type (selects the judgement windows; types without a Just window are never converted to Just).
    pub judgement_type: i32,
}

/// A note judged in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgedNote {
    pub note_id: i32,
    /// The note judgement before conversion (1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just).
    pub judgement: i32,
    pub judgement_time_ms: i32,
}

/// One frame of a play: its music time and the notes judged in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayFrame {
    pub time_ms: i32,
    pub judged: Vec<JudgedNote>,
}

/// The settled part of a live's score after a frame (see [`LiveModel::settle`]): score frames below `frame` are final
/// and hold `total` points, `fixed` of them fixed Gekisou rank bonus scores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settled {
    pub frame: i32,
    pub total: i64,
    pub fixed: i64,
}

/// A play: the frame schedule with the judgements, and the live's random seed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LivePlay {
    pub frames: Vec<PlayFrame>,
    pub base_seed: i32,
}

/// The live's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveParams {
    /// Resolved music target for ordinary condition 4012, distinct from parameter fallback.
    pub skill_target_music_type: i64,
    pub total_power: i32,
    pub music_level: i32,
    pub converted_note_count: i32,
    /// Music length (caps effect finish times).
    pub music_length_ms: i32,
    /// Length the score's frame table covers; `None` or 0: the music length.
    pub score_music_length_ms: Option<i32>,
    /// Assist factor (1 without assist).
    pub assist_factor: f32,
}

/// Gekisou of a live: the chart's fevers and the song's missions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GekisouSetup {
    /// Fever ranges `(start, end)` in chart order; the first three are the Gekisou ranges (every one with external
    /// ranking). A fourth fever makes the live fail when it starts, as the game's controller does.
    pub fevers: Vec<(i32, i32)>,
    /// The song's three mission types (1 combo, 2 luck, 3 Just count); the first ones go to the fevers in order.
    pub missions: Vec<i64>,
}

/// The solo rank bonus percentages `[range][rank - 1]` of a song's three missions (their mission pattern's rows of
/// `MasterLiveGekisouRankingScoreBonus`), as a solo live with Gekisou reads them.
pub fn gekisou_rank_factors(master: &Master, missions: &[i64; 3]) -> Result<[[i64; 5]; 3], Error> {
    gekisou::ranking_factors(master, gekisou::mission_pattern(missions[0], missions[1], missions[2]))
}

/// The state of one Gekisou range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GekisouRange {
    pub mission: i64,
    /// 1 Wait, 2 Standby, 3 Start, 4 Playing, 5 End, 6 Delay, 7 Complete, 8 Finish.
    pub state: u8,
    pub combo: i32,
    pub max_combo: i32,
    pub just_count: i32,
    /// Controller score snapshots at range start and end. Solo ranking replaces these with timestamp queries
    /// for the completion it ranks; the native-network adapter retains the original frame snapshots.
    pub start_score: i32,
    pub end_score: i32,
    pub luck_points: i32,
    pub luck_gauge: i32,
    pub rush_combo: i32,
    /// Lottery results drawn: Miss, Hit, Super Hit, Critical.
    pub lot_results: [i32; 4],
    /// The rank bonus added when the range completed.
    pub rank_bonus: Option<i32>,
}

/// Approximate float equality: `|b - a| < max(1e-6 * max(|a|, |b|), 8 * smallest subnormal)`.
fn approximately(a: f32, b: f32) -> bool {
    let tol = (1e-6f32 * a.abs().max(b.abs())).max(f32::from_bits(1) * 8f32);
    (b - a).abs() < tol
}

/// Whether a predicate reads the lottery (a LUCK rush or lot result) or draws a probability.
fn is_lottery_predicate(checker: &Checker) -> bool {
    matches!(checker, Checker::LuckRushPlaying(_) | Checker::LuckLotResult { .. } | Checker::Probability(_))
}

/// The bit of a performance position in a position mask (none outside `0..32`).
fn position_bit(index: i32) -> u32 {
    if (0..32).contains(&index) { 1 << index } else { 0 }
}

/// The minimum lottery result of an 11005 effect value: 2..=4 give Hit, Super Hit, Critical; else 0.
fn minimum_result_of(v: i64) -> i64 {
    if (0..=2).contains(&(v.wrapping_sub(2) as i32)) { v - 1 } else { 0 }
}

/// A live in progress.
#[derive(Clone, Debug)]
pub struct LiveModel {
    #[cfg(feature = "search-diagnostics")]
    rush_probes: Option<luck::RushProbes>,
    /// Rush samples ([`luck_rush_samples`]): the edges `(row, time, +1 / -1)` of the condition skills' note
    /// score-ups.
    rush_effect_log: Option<Vec<(usize, i32, i32)>>,
    /// LUCK weighted lives: by effect row, whether its note score-up reads the lottery and is taken from the weights.
    luck_suppressed: Vec<bool>,
    program_has_started: bool,
    notes: FxHashMap<i32, LiveNote>,
    events: Vec<(i32, i32)>,
    fired: Vec<bool>,
    music_length_ms: i32,
    is_live_finished: bool,
    random: LiveRandom,
    life: LifeController,
    /// Optional read-only life values after note damage, before each native skill phase's appliers.
    phase_life: Option<[i32; 2]>,
    combo: combo::ComboCounter,
    score: IncrementalCalculator,
    conversion: Conversion,
    rows: Vec<EffectRow>,
    live: Vec<LiveSkill>,
    live_pools: Vec<LivePool>,
    enabled_live: Vec<usize>,
    cond: Vec<CondSkill>,
    guards: FxHashMap<StateKey, i32>,
    life_reductions: FxHashMap<StateKey, i32>,
    life_limits: FxHashMap<StateKey, i32>,
    frame_events: Vec<(i32, i32)>,
    judged: Vec<(i32, i32, i32)>,
    trace: Vec<(i32, i32)>,
    frame_time: i32,
    frame_score: i32,
    prev_confirmed_rank: Option<i32>,
    /// Simulator timing-combo snapshot and the separately reset live combo controller.
    simulator_previous_combo: i32,
    current_combo: i32,
    scorectl: ScoreCtl,
    gk: Option<GekisouLive>,
    gk_appliers: GkAppliers,
    /// Mutable raw judgement windows, Assist and window limit callbacks; `None` for a judged stream.
    raw_runtime: Option<RawJudgementRuntime>,
    /// The open raw frame's results, in FT order.
    raw_pending: Option<Vec<RawJudgedNote>>,
    /// The Gekisou rank confirmation taken when the current frame began.
    frame_rank_confirmation: Option<i32>,
    rank_timeline: Vec<crate::replay::RankConfirmation>,
    next_rank_confirmation: usize,
    scratch: FrameScratch,
    /// Positions (bit k: performer k) whose chart skill events have fired.
    touched_events: u32,
    /// Positions whose condition skills have started an effect or drawn a random value.
    touched_skills: u32,
}

#[cfg(test)]
mod pool_regression;

#[cfg(test)]
mod life_reads_tests;

#[cfg(test)]
mod gaps_tests;

#[cfg(test)]
mod effect_state_tests;
