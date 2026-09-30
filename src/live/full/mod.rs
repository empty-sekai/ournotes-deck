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
//! cache, which a command in an earlier frame does not fully invalidate. Confirming a range's rank bonus computes
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

mod combo;
mod conditions;
mod convert;
mod engine;
mod gekisou;
pub use gekisou::{
    BrokenGekisouRanking, NetworkGekisouRanking, NetworkGekisouResult, calculate_network_gekisou_ranking,
    gekisou_no_input,
};
mod life;
mod raw_runtime;
pub use raw_runtime::{RELAX_TARGET_JUDGEMENTS, RawJudgedNote, RawJudgementRuntime};
mod range_frames;
mod scorecalc;
pub use range_frames::RangeFrames;

use std::collections::{HashMap, HashSet, VecDeque};

use conditions::{CheckCtx, Checker, Cumulative, Factory, GkView};
use convert::{Conversion, ConvertEffect};
use engine::{
    CondEffect, ConditionSkillUpdater, END_FRAME, EXECUTE_FRAME, EXECUTING, EffectState, FrameInput, ONE_SHOT, STAY,
    SUSTAINED, TriggerResult, UpdateCheckers, effect_update,
};
use gekisou::{Controller, Env, FeverUpdater, GkNote, LuckHandle, S_COMPLETE};
use life::LifeController;
use scorecalc::{IncrementalCalculator, NoteCommand};

use crate::error::Error;
use crate::live::random::LiveRandom;
use crate::live::score::{
    ComboTable, GekisouComboInfo, LiveScoreCalculator, LiveScoreSettings, ScoreFactorState, convert_score_type,
};
use crate::live::skill::{FactorCommand, OWNER_MEMBER, OWNER_SNAP, judgement_factor_mill, note_factor_mill};
use crate::master::{GekisouSkillEffectRow, Master, SupportSkillEffectRow};
use crate::num::floor_to_i32;

/// Skill type digits in effect keys.
const SKILL_TYPE_SUPPORT: i64 = 3;
const SKILL_TYPE_GEKISOU: i64 = 4;
const SKILL_TYPE_GEKISOU_SUPPORT: i64 = 5;
/// Skill update phases, in order.
const PHASES: [i64; 2] = [1, 2];

/// One performance position of the deck, in skill order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    pub(crate) fn matches_skill_target(&self, target: &crate::master::SkillTargetRow) -> bool {
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

/// The effect row data the appliers read.
#[derive(Clone, Debug)]
struct EffectRow {
    id: i64,
    effect_type: i64,
    effect_value: i64,
    max_effect_value: i64,
    effect_limit_count: i64,
    /// Judgements of the effect's targets, or the first target id that is not in the master.
    targets: Result<Vec<i64>, i64>,
}

impl EffectRow {
    fn targets(&self) -> Result<&[i64], Error> {
        self.targets.as_deref().map_err(|&id| Error::Master(format!("unknown skill target {id}")))
    }
}

/// Identity of an effect state for the appliers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StateKey {
    Live { row_id: i64, member: usize, index: usize },
    Cond { effect_id: i64, index: usize },
}

#[derive(Clone, Debug)]
struct LiveEffect {
    row: usize,
    act: f32,
    phase: i64,
    state: EffectState,
    condition: Option<Checker>,
    release: Option<Checker>,
    cumulative: Option<Cumulative>,
}

impl LiveEffect {
    fn update(&mut self, inp: FrameInput, trigger: TriggerResult, ctx: &mut CheckCtx) -> Result<bool, Error> {
        let before = self.state.state;
        let checkers = UpdateCheckers { condition: self.condition.as_mut(), release: self.release.as_mut() };
        let changed = effect_update(&mut self.state, self.act, inp, trigger, checkers, false, ctx)?;
        if let Some(c) = self.cumulative.as_mut() {
            let after = self.state.state;
            if (before == STAY && after == EXECUTE_FRAME)
                || before == EXECUTE_FRAME
                || (before == EXECUTING && after == EXECUTING)
            {
                self.state.cumulative_count = c.update_count(ctx)?;
            } else if before == END_FRAME {
                c.reset();
            }
        }
        Ok(changed)
    }
}

#[derive(Clone, Debug)]
struct LiveSkill {
    key: i64,
    member: usize,
    index: usize,
    parent_state: u8,
    trigger: TriggerResult,
    effects: Vec<LiveEffect>,
}

#[derive(Clone, Debug)]
struct LivePool {
    key: i64,
    member: usize,
    available: VecDeque<usize>,
}

fn aggregate_live_state(effects: &[LiveEffect]) -> u8 {
    for state in [EXECUTING, EXECUTE_FRAME, END_FRAME] {
        if effects.iter().any(|e| e.state.state == state) {
            return state;
        }
    }
    STAY
}

/// A condition skill of one member: snap (3), Gekisou (4) or Gekisou support (5).
#[derive(Clone, Debug)]
struct CondSkill {
    member: usize,
    skill_type: i64,
    updater: ConditionSkillUpdater,
}

#[derive(Clone, Copy, Debug)]
enum Listed {
    Live { skill: usize, effect: usize },
    Cond { updater: usize, u: usize },
}

/// Score factor handles: note / combo score up by id and the Gekisou luck (rush) bonus.
#[derive(Clone, Debug, Default)]
struct ScoreCtl {
    counter: i32,
    cmds: HashMap<i32, FactorCommand>,
}

impl ScoreCtl {
    fn put(&mut self, score: &mut IncrementalCalculator, cmd: FactorCommand) -> i32 {
        self.counter = self.counter.wrapping_add(1);
        self.cmds.insert(self.counter, cmd);
        score.add_factor(cmd);
        self.counter
    }

    fn take(&mut self, id: i32) -> Result<FactorCommand, Error> {
        self.cmds.remove(&id).ok_or_else(|| Error::Game(format!("score factor {id} not found")))
    }

    fn add_note_score_up(&mut self, score: &mut IncrementalCalculator, owner: i32, t: i32, factor: f32) -> i32 {
        let cmd =
            FactorCommand { time_ms: t, owner_id: owner, note_mill: note_factor_mill(factor), ..Default::default() };
        self.put(score, cmd)
    }

    fn disable_note_score_up(&mut self, score: &mut IncrementalCalculator, t: i32, id: i32) -> Result<(), Error> {
        let c = self.take(id)?;
        score.add_factor(FactorCommand {
            time_ms: t,
            owner_id: c.owner_id,
            note_mill: c.note_mill.wrapping_neg(),
            ..Default::default()
        });
        Ok(())
    }

    fn add_combo_bonus(&mut self, score: &mut IncrementalCalculator, owner: i32, t: i32, factor: f32) -> i32 {
        let cmd = FactorCommand {
            time_ms: t,
            owner_id: owner,
            combo_mill: judgement_factor_mill(factor),
            ..Default::default()
        };
        self.put(score, cmd)
    }

    fn disable_combo_bonus(&mut self, score: &mut IncrementalCalculator, t: i32, id: i32) -> Result<(), Error> {
        let c = self.take(id)?;
        score.add_factor(FactorCommand {
            time_ms: t,
            owner_id: c.owner_id,
            combo_mill: c.combo_mill.wrapping_neg(),
            ..Default::default()
        });
        Ok(())
    }

    fn combo_bonus_factor(&self, id: i32) -> f32 {
        match self.cmds.get(&id) {
            Some(c) if c.combo_mill != 0 => c.combo_mill as f32 / 100000f32,
            _ => 0f32,
        }
    }

    fn score_up_factor(&self, id: i32) -> f32 {
        match self.cmds.get(&id) {
            Some(c) if c.note_mill != 0 => c.note_mill as f32 / 100000f32,
            _ => 0f32,
        }
    }
}

/// The luck handle of the Gekisou controller: rush bonus commands through the score controller.
struct Handle<'a> {
    sc: &'a mut ScoreCtl,
    score: &'a mut IncrementalCalculator,
}

impl LuckHandle for Handle<'_> {
    fn add(&mut self, t: i32, percent: i32) -> i32 {
        self.sc.put(self.score, FactorCommand { time_ms: t, owner_id: -1, luck: percent, ..Default::default() })
    }

    fn disable(&mut self, t: i32, id: i32) -> Result<(), Error> {
        let c = self.sc.take(id)?;
        self.score.add_factor(FactorCommand {
            time_ms: t,
            owner_id: -1,
            luck: c.luck.wrapping_neg(),
            ..Default::default()
        });
        Ok(())
    }
}

/// The Gekisou part of a live.
#[derive(Clone, Debug)]
struct GekisouLive {
    ctrl: Controller,
    fever: FeverUpdater,
    factors: [[i64; 5]; 3],
    external_ranking: bool,
    solo_score_queries: bool,
    completed_scores: Vec<bool>,
    pending_ranks: Vec<Option<(i32, i64)>>,
    fever_updates: Vec<(usize, u8)>,
    prev_lots: Vec<i64>,
    prev_lot_ms: i32,
    /// `(range, rank, bonus, percent)` of each confirmed range.
    rank_bonus: Vec<(usize, i32, i32, i64)>,
}

/// Per-state bookkeeping of the Gekisou appliers.
#[derive(Clone, Debug, Default)]
struct GkAppliers {
    ids: HashMap<StateKey, i32>,
    exhausted: HashSet<StateKey>,
    limit_finished: HashMap<StateKey, i32>,
}

impl GkAppliers {
    fn add(&mut self, key: StateKey, id: i32) -> Result<(), Error> {
        if self.ids.contains_key(&key) {
            return Err(Error::Game("effect state registered twice".into()));
        }
        self.ids.insert(key, id);
        Ok(())
    }

    fn pop(&mut self, key: StateKey) -> Result<i32, Error> {
        self.ids.remove(&key).ok_or_else(|| Error::Game("effect state not registered".into()))
    }

    /// Ends the state at the time its limit ran out, when that was recorded.
    fn limit(&mut self, key: StateKey, st: &mut EffectState) {
        if let Some(t) = self.limit_finished.remove(&key) {
            st.state = END_FRAME;
            st.finish_ms = t;
        }
    }
}

/// Approximate float equality: `|b - a| < max(1e-6 * max(|a|, |b|), 8 * smallest subnormal)`.
fn approximately(a: f32, b: f32) -> bool {
    let tol = (1e-6f32 * a.abs().max(b.abs())).max(f32::from_bits(1) * 8f32);
    (b - a).abs() < tol
}

/// The minimum lottery result of an 11005 effect value: 2..=4 give Hit, Super Hit, Critical; else 0.
fn minimum_result_of(v: i64) -> i64 {
    if (0..=2).contains(&(v.wrapping_sub(2) as i32)) { v - 1 } else { 0 }
}

/// The columns of a condition skill effect row (snap or Gekisou table).
struct CondRow<'a> {
    id: i64,
    trigger_type: i64,
    trigger_group: i64,
    condition_group: i64,
    release_group: i64,
    target_ids: &'a [i64],
    effect_type: i64,
    act: f32,
    value: i64,
    max_value: i64,
    limit: i64,
    cumulative_id: i64,
    execute_limit: i64,
    reset_group: i64,
}

impl<'a> From<&'a SupportSkillEffectRow> for CondRow<'a> {
    fn from(r: &'a SupportSkillEffectRow) -> CondRow<'a> {
        CondRow {
            id: r.id,
            trigger_type: r.skill_trigger_type,
            trigger_group: r.skill_trigger_condition_group,
            condition_group: r.skill_condition_group,
            release_group: r.skill_release_condition_group,
            target_ids: &r.skill_target_ids,
            effect_type: r.skill_effect_type,
            act: r.activation_time_second,
            value: r.effect_value,
            max_value: r.max_effect_value,
            limit: r.effect_limit_count,
            cumulative_id: r.skill_cumulative_condition_id,
            execute_limit: r.effect_execute_limit_count,
            reset_group: r.effect_execute_limit_reset_condition_group,
        }
    }
}

impl<'a> From<&'a GekisouSkillEffectRow> for CondRow<'a> {
    fn from(r: &'a GekisouSkillEffectRow) -> CondRow<'a> {
        CondRow {
            id: r.id,
            trigger_type: r.skill_trigger_type,
            trigger_group: r.skill_trigger_condition_group,
            condition_group: r.skill_condition_group,
            release_group: r.skill_release_condition_group,
            target_ids: &r.skill_target_ids,
            effect_type: r.skill_effect_type,
            act: r.activation_time_second,
            value: r.effect_value,
            max_value: r.max_effect_value,
            limit: r.effect_limit_count,
            cumulative_id: r.skill_cumulative_condition_id,
            execute_limit: r.effect_execute_limit_count,
            reset_group: r.effect_execute_limit_reset_condition_group,
        }
    }
}

/// A live in progress.
#[derive(Clone, Debug)]
pub struct LiveModel {
    notes: HashMap<i32, LiveNote>,
    events: Vec<(i32, i32)>,
    fired: Vec<bool>,
    music_length_ms: i32,
    is_live_finished: bool,
    random: LiveRandom,
    life: LifeController,
    combo: combo::ComboCounter,
    score: IncrementalCalculator,
    conversion: Conversion,
    rows: Vec<EffectRow>,
    live: Vec<LiveSkill>,
    live_pools: Vec<LivePool>,
    enabled_live: Vec<usize>,
    cond: Vec<CondSkill>,
    guards: HashMap<StateKey, i32>,
    life_reductions: HashMap<StateKey, i32>,
    life_limits: HashMap<StateKey, i32>,
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
}

fn effect_row(master: &Master, r: &CondRow) -> EffectRow {
    let targets = r.target_ids.iter().map(|&t| master.skill_target(t).map(|x| x.judgement).ok_or(t)).collect();
    EffectRow {
        id: r.id,
        effect_type: r.effect_type,
        effect_value: r.value,
        max_effect_value: r.max_value,
        effect_limit_count: r.limit,
        targets,
    }
}

fn setting(master: &Master, key: &str) -> Result<i64, Error> {
    let v = master.live_setting(key).ok_or_else(|| Error::Master(format!("missing live setting {key}")))?;
    v.trim().parse::<i64>().map_err(|_| Error::Master(format!("live setting {key} is not an integer: {v}")))
}

/// The mission list a Gekisou skill joins in `SkillStatus.CreateGekisouSkillDictionary` (0x56173b0): 1..=3 their
/// own list, All (4) every list, so its skill (added once, `TryAdd`) takes the Combo list's place; any other value
/// is `get_Item` on the three-key dictionary, a KeyNotFoundException.
fn gekisou_mission_group(mission: i64) -> Result<i64, Error> {
    match mission {
        1..=3 => Ok(mission),
        4 => Ok(1),
        m => Err(Error::Game(format!("KeyNotFoundException: Gekisou mission type {m}"))),
    }
}

/// Effect types whose appliers exist only with Gekisou on (`LivePlayingSkillUtility.AppendGekisouSkillApplier`
/// 0x61291c8); the applier container is keyed by effect type alone, whatever the skill type (13005 is handled with
/// 12006, 13001 belongs to the raw judgement bridge).
const GEKISOU_APPLIER_TYPES: [i64; 14] =
    [11000, 11001, 11002, 11003, 11004, 11005, 12000, 12002, 12003, 12004, 13000, 13002, 13003, 13004];

/// Builds the condition skill of one member from its effect rows (sorted by id): effect key = row id * 100 + skill
/// type * 10 + member index.
#[allow(clippy::too_many_arguments)]
fn condition_skill(
    master: &Master,
    factory: &Factory,
    phase: &dyn Fn(i64) -> i64,
    mut rs: Vec<CondRow>,
    k: usize,
    skill_type: i64,
    gate: Option<i64>,
    rows: &mut Vec<EffectRow>,
) -> Result<CondSkill, Error> {
    rs.sort_by_key(|r| r.id);
    let mut effects = Vec::with_capacity(rs.len());
    let mut release_groups = Vec::with_capacity(rs.len());
    for r in rs {
        if r.trigger_type != ONE_SHOT && r.trigger_type != SUSTAINED {
            // ConditionSkillUpdater..ctor 0x561b8b8: cmp 1 / cmp 2, else ArgumentOutOfRangeException (0x561ca00).
            return Err(Error::Game(format!("ArgumentOutOfRangeException: skill trigger type {}", r.trigger_type)));
        }
        let effect_id = r.id.wrapping_mul(100).wrapping_add(skill_type * 10).wrapping_add(k as i64);
        let trigger = factory.group(r.trigger_group, k)?;
        let condition = factory.group(r.condition_group, k)?;
        let reset = if r.execute_limit > 0 { factory.group(r.reset_group, k)? } else { None };
        let cumulative = if r.trigger_type == ONE_SHOT || r.trigger_type == SUSTAINED {
            factory.cumulative(r.cumulative_id, k)?
        } else {
            None
        };
        rows.push(effect_row(master, &r));
        effects.push(CondEffect {
            effect_id,
            trigger_type: r.trigger_type,
            act: r.act,
            phase: phase(r.effect_type),
            trigger,
            condition,
            execute_limit: r.execute_limit,
            reset,
            row: rows.len() - 1,
            cumulative,
        });
        release_groups.push(r.release_group);
    }
    let updater = ConditionSkillUpdater::new(effects, |e| factory.group(release_groups[e], k), gate)?;
    Ok(CondSkill { member: k, skill_type, updater })
}

impl LiveModel {
    /// Supply the previous frame's confirmed rank for condition 7012.
    /// The event is cleared after the next frame consumes it. This does not calculate or award ranking bonuses.
    pub fn set_previous_gekisou_rank_confirmation(&mut self, rank: Option<i32>) {
        self.prev_confirmed_rank = rank;
    }

    /// Builds a live without Gekisou: `deck` in skill order, the chart notes, the chart's skill events
    /// `(performer index, time)` in chart order.
    pub fn new(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
    ) -> Result<LiveModel, Error> {
        LiveModel::build(master, deck, notes, skill_events, params, None, false)
    }

    /// Builds a live with Gekisou (see [`LiveModel::new`]); frame delta times drive the ranges' end delays, see
    /// [`LiveModel::run_timed`].
    pub fn new_gekisou(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
    ) -> Result<LiveModel, Error> {
        LiveModel::build(master, deck, notes, skill_events, params, Some(setup), false)
    }

    /// Dynamic controller with explicit external ranking. No native three-range network limit is imposed.
    /// This is a simulation adapter, not a claim that the native network calculator accepts fourth ranges.
    pub fn new_gekisou_external(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
    ) -> Result<LiveModel, Error> {
        LiveModel::build(master, deck, notes, skill_events, params, Some(setup), true)
    }

    /// Counterfactual solo scoring with explicitly queued ranks and percentages. Range scores use
    /// the solo updater's exact start/end timestamp queries. This is an adapter for chart statistics
    /// and rank sensitivity; like solo, only the first completion in a frame is ranked. Native solo
    /// always confirms rank 1, and this does not emulate network play.
    pub fn new_gekisou_ranked(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
    ) -> Result<LiveModel, Error> {
        let mut model = LiveModel::build(master, deck, notes, skill_events, params, Some(setup), false)?;
        let gk = model.gk.as_mut().expect("Gekisou setup supplied");
        gk.external_ranking = true;
        gk.solo_score_queries = true;
        Ok(model)
    }

    /// Queue a confirmed rank and its explicit bonus percentage for an externally ranked adapter.
    /// `new_gekisou_external` uses native network frame snapshots; `new_gekisou_ranked` uses solo timestamp
    /// queries for counterfactual fixed ranks. Applied after the range has completed.
    /// Identical retries are idempotent; conflicting confirmations are errors. These are adapter policies.
    /// Multiple confirmations applied together publish the last range's rank through the native scalar event.
    pub fn queue_gekisou_rank_confirmation(&mut self, range: usize, rank: i32, percent: i64) -> Result<(), Error> {
        let g = self.gk.as_mut().ok_or_else(|| Error::Input("rank confirmation without Gekisou".into()))?;
        if !g.external_ranking {
            return Err(Error::Input("external ranking mode is required".into()));
        }
        if rank <= 0 {
            return Err(Error::Input("confirmed rank must be positive".into()));
        }
        let pending = g.pending_ranks.get_mut(range).ok_or_else(|| Error::Input("rank range out of bounds".into()))?;
        if let Some(&(_, old_rank, _, old_percent)) = g.rank_bonus.iter().find(|r| r.0 == range) {
            return if (old_rank, old_percent) == (rank, percent) {
                Ok(())
            } else {
                Err(Error::Input("conflicting completed rank confirmation".into()))
            };
        }
        if pending.is_some_and(|old| old != (rank, percent)) {
            return Err(Error::Input("conflicting pending rank confirmation".into()));
        }
        *pending = Some((rank, percent));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: Option<&GekisouSetup>,
        external_ranking: bool,
    ) -> Result<LiveModel, Error> {
        if !matches!(params.skill_target_music_type, 0..=5 | 99) {
            return Err(Error::Input("unknown skill target music type".into()));
        }
        let mut map = HashMap::with_capacity(notes.len());
        for n in notes {
            map.insert(n.note_id, *n);
        }
        let gk = match setup {
            None => None,
            Some(s) => Some(LiveModel::gekisou_live(master, s, &map, external_ranking)?),
        };
        let settings = LiveScoreSettings::from_master(master)?;
        let table = ComboTable::from_master(master)?;
        let calc = LiveScoreCalculator::new(
            params.total_power,
            params.music_level,
            params.converted_note_count,
            &settings,
            1.0,
            params.assist_factor,
            Some(table),
        );
        let (base, damage) = life::life_settings(master)?;
        let life_length = notes.iter().map(|n| n.time_ms).max().map_or(1000, |t| t.wrapping_add(1000));
        let life = LifeController::new(base, damage, life_length)?;
        let score_length = match params.score_music_length_ms {
            Some(l) if l != 0 => l,
            _ => params.music_length_ms,
        };
        let have_just: HashSet<i64> = master
            .live_judgement_timings
            .iter()
            .filter(|r| r.note_simulate_judgement == 6)
            .map(|r| r.note_judgement_type)
            .collect();
        let no_just: HashSet<i64> = master
            .live_judgement_timings
            .iter()
            .map(|r| r.note_judgement_type)
            .filter(|t| !have_just.contains(t))
            .collect();
        let phase_of: HashMap<i64, i64> =
            master.skill_effect_settings.iter().map(|r| (r.skill_effect_type, r.phase)).collect();
        let phase = |t: i64| phase_of.get(&t).copied().unwrap_or(1);
        let factory = Factory {
            master,
            deck,
            initial_life: base,
            initial_time_ms: 0,
            skill_target_music_type: params.skill_target_music_type,
        };

        let mut rows = Vec::new();
        let mut live = Vec::new();
        let mut live_pools = Vec::new();
        for (k, p) in deck.iter().enumerate() {
            let Some((sid, lv)) = p.live_skill else { continue };
            let mut rs: Vec<_> =
                master.live_skill_effects.iter().filter(|r| r.live_skill_id == sid && r.level == lv).collect();
            rs.sort_by_key(|r| r.id);
            let mut effects = Vec::with_capacity(rs.len());
            for r in rs {
                // `EffectLimitCount` is read only by the appliers that use it (12004, 11005, 12006 / 13005,
                // 4000..=4003, 13001), whatever the skill type; no build-time gate exists natively.
                let condition = factory.group(r.skill_condition_group, k)?;
                let release = factory.group(r.skill_release_condition_group, k)?;
                let cumulative = factory.cumulative(r.skill_cumulative_condition_id, k)?;
                let mut state = EffectState { execute_ms: -1, finish_ms: -1, ..Default::default() };
                if let Some(c) = &cumulative {
                    state.cumulative_count = c.init_count();
                    state.cumulative_unit = c.unit();
                    state.cumulative_max = c.max();
                }
                let targets =
                    r.skill_target_ids.iter().map(|&t| master.skill_target(t).map(|x| x.judgement).ok_or(t)).collect();
                rows.push(EffectRow {
                    id: r.id,
                    effect_type: r.skill_effect_type,
                    effect_value: r.effect_value,
                    max_effect_value: r.max_effect_value,
                    effect_limit_count: r.effect_limit_count,
                    targets,
                });
                effects.push(LiveEffect {
                    row: rows.len() - 1,
                    act: r.activation_time_second,
                    phase: phase(r.skill_effect_type),
                    state,
                    condition,
                    release,
                    cumulative,
                });
            }
            let key = sid.wrapping_mul(1000).wrapping_add(100).wrapping_add(k as i64);
            let mut available = VecDeque::new();
            for index in 0..5 {
                available.push_back(live.len());
                live.push(LiveSkill {
                    key,
                    member: k,
                    index,
                    parent_state: STAY,
                    trigger: TriggerResult::default(),
                    effects: effects.clone(),
                });
            }
            live_pools.push(LivePool { key, member: k, available });
        }
        live_pools.sort_by_key(|p| p.key);

        let mut cond = Vec::new();
        for (k, p) in deck.iter().enumerate() {
            for &(sid, lv) in &p.support_skills {
                let rs = master
                    .support_skill_effects
                    .iter()
                    .filter(|r| r.support_skill_id == sid && r.level == lv)
                    .map(CondRow::from)
                    .collect();
                cond.push(condition_skill(master, &factory, &phase, rs, k, SKILL_TYPE_SUPPORT, None, &mut rows)?);
            }
        }
        if gk.is_none() {
            // SkillStatus builds the Gekisou skill dictionary whether or not Gekisou is on, so a mission outside
            // 1..=4 fails there too (see `gekisou_mission_group`).
            for p in deck {
                if let Some(row) = p.gekisou_skill.and_then(|(sid, _)| master.gekisou_skill(sid)) {
                    gekisou_mission_group(row.gekisou_mission_type)?;
                }
            }
        }
        if gk.is_some() {
            let mut mission = vec![None; deck.len()];
            for (k, p) in deck.iter().enumerate() {
                let Some((sid, _)) = p.gekisou_skill else { continue };
                let row =
                    master.gekisou_skill(sid).ok_or_else(|| Error::Master(format!("unknown Gekisou skill {sid}")))?;
                gekisou_mission_group(row.gekisou_mission_type)?;
                mission[k] = Some(row.gekisou_mission_type);
            }
            for m in 1..=3 {
                for (k, p) in deck.iter().enumerate() {
                    let (Some((sid, lv)), Some(mk)) = (p.gekisou_skill, mission[k]) else { continue };
                    if gekisou_mission_group(mk)? != m {
                        continue;
                    }
                    let rs = master
                        .gekisou_skill_effects
                        .iter()
                        .filter(|r| r.skill_id == sid && r.level == lv)
                        .map(CondRow::from)
                        .collect();
                    cond.push(condition_skill(
                        master,
                        &factory,
                        &phase,
                        rs,
                        k,
                        SKILL_TYPE_GEKISOU,
                        Some(mk),
                        &mut rows,
                    )?);
                }
            }
            for (k, p) in deck.iter().enumerate() {
                if mission[k].is_none() {
                    continue;
                }
                for &(sid, lv) in &p.gekisou_support_skills {
                    let row = master
                        .gekisou_support_skill(sid)
                        .ok_or_else(|| Error::Master(format!("unknown Gekisou support skill {sid}")))?;
                    let rs = master
                        .gekisou_support_skill_effects
                        .iter()
                        .filter(|r| r.skill_id == sid && r.level == lv)
                        .map(CondRow::from)
                        .collect();
                    let gate = Some(row.gekisou_mission_type);
                    cond.push(condition_skill(
                        master,
                        &factory,
                        &phase,
                        rs,
                        k,
                        SKILL_TYPE_GEKISOU_SUPPORT,
                        gate,
                        &mut rows,
                    )?);
                }
            }
        }

        Ok(LiveModel {
            notes: map,
            events: skill_events.to_vec(),
            fired: vec![false; skill_events.len()],
            music_length_ms: params.music_length_ms,
            is_live_finished: false,
            random: LiveRandom::new(0),
            life,
            combo: combo::ComboCounter::new(notes.len()),
            score: IncrementalCalculator::new(calc, score_length),
            conversion: Conversion::new(no_just),
            rows,
            live,
            live_pools,
            enabled_live: Vec::new(),
            cond,
            guards: HashMap::new(),
            life_reductions: HashMap::new(),
            life_limits: HashMap::new(),
            frame_events: Vec::new(),
            judged: Vec::new(),
            trace: Vec::new(),
            frame_time: 0,
            frame_score: 0,
            prev_confirmed_rank: None,
            simulator_previous_combo: 0,
            current_combo: 0,
            scorectl: ScoreCtl::default(),
            gk,
            gk_appliers: GkAppliers::default(),
            raw_runtime: None,
            raw_pending: None,
            frame_rank_confirmation: None,
        })
    }

    fn gekisou_live(
        master: &Master,
        setup: &GekisouSetup,
        notes: &HashMap<i32, LiveNote>,
        external_ranking: bool,
    ) -> Result<GekisouLive, Error> {
        // LiveMusicScore.GetGekisouRanges 0x55d0694: min(FeverList.Count, _gekisouRangeMaxCount = 3) ranges; every
        // fever still has its FeverEventUpdater, and a later fever's first state change indexes the range states out
        // of bounds in GekisouController.BeforeUpdate (0x55d4e30), which the controller reports as an error.
        let n = if external_ranking { setup.fevers.len() } else { setup.fevers.len().min(gekisou::MAX_RANGES) };
        let mut ranges = Vec::with_capacity(n);
        for (i, &(s, e)) in setup.fevers[..n].iter().enumerate() {
            let m = *setup.missions.get(i).ok_or_else(|| Error::Input("fewer missions than fevers".into()))?;
            ranges.push((s, e, m));
        }
        let after = master.live_judgement_timings.iter().map(|r| r.after_ms).max().unwrap_or(0);
        let gauge_max = setting(master, "gekisou_luck_gauge_max")?;
        let gauge_max_rush = setting(master, "gekisou_luck_gauge_max_rush")?;
        let rush_percent = setting(master, "gekisou_luck_rush_score_bonus_percent")?;
        let ctrl = Controller::new(
            ranges,
            notes.values().map(|n| (n.note_id, n.time_ms)),
            master,
            gauge_max,
            gauge_max_rush,
            rush_percent,
            after,
        )?;
        let factors = if external_ranking {
            [[0; 5]; 3]
        } else {
            let [m0, m1, m2] = match setup.missions.get(..3) {
                Some(&[a, b, c]) => [a, b, c],
                _ => return Err(Error::Input("a Gekisou song has three missions".into())),
            };
            gekisou::ranking_factors(master, gekisou::mission_pattern(m0, m1, m2))?
        };
        Ok(GekisouLive {
            ctrl,
            fever: FeverUpdater::new(&setup.fevers),
            factors,
            external_ranking,
            solo_score_queries: false,
            completed_scores: vec![false; n],
            pending_ranks: vec![None; n],
            fever_updates: Vec::new(),
            prev_lots: Vec::new(),
            prev_lot_ms: 0,
            rank_bonus: Vec::new(),
        })
    }

    /// Plays every frame of `play` (frame delta times 0); returns the final score.
    pub fn run(&mut self, play: &LivePlay) -> Result<i32, Error> {
        self.random.set_seed(play.base_seed);
        for f in &play.frames {
            self.frame_timed(f.time_ms, &f.judged, 0f32)?;
        }
        Ok(self.score.score)
    }

    /// Plays every frame of `play` with its delta time in seconds (`delta_times[i]` for frame `i`); returns the final
    /// score.
    pub fn run_timed(&mut self, play: &LivePlay, delta_times: &[f32]) -> Result<i32, Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        self.random.set_seed(play.base_seed);
        for (f, &dt) in play.frames.iter().zip(delta_times) {
            self.frame_timed(f.time_ms, &f.judged, dt)?;
        }
        Ok(self.score.score)
    }

    /// Runs a fresh model with a complete post-shuffle random state, without reseeding.
    /// Ignores play.base_seed. This does not reset gameplay state: construct a new model per root.
    pub fn run_with_random(&mut self, play: &LivePlay, delta_times: &[f32], random: LiveRandom) -> Result<i32, Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        self.random = random;
        for (frame, &dt) in play.frames.iter().zip(delta_times) {
            self.frame_timed(frame.time_ms, &frame.judged, dt)?;
        }
        Ok(self.score.score)
    }

    /// The current score.
    pub fn score(&self) -> i32 {
        self.score.score
    }

    /// Seeds the live's random streams, as [`LiveModel::run`] does with the play's base seed. Before any value has
    /// been drawn ([`LiveModel::draws`] is 0) this equals building the live with that seed.
    pub fn set_seed(&mut self, seed: i32) {
        self.random.set_seed(seed);
    }

    /// Supplies the live-finished input before a frame. This is a lifecycle signal, not a guessed music-time cutoff.
    /// It suppresses new condition-skill triggers; already running one-shot effects continue updating.
    pub fn set_live_finished(&mut self, finished: bool) {
        self.is_live_finished = finished;
    }

    /// The number of random values drawn since the streams were last seeded.
    pub fn draws(&self) -> u64 {
        self.random.draws()
    }

    /// `(frame time, score after the frame)` of every frame played.
    pub fn trace(&self) -> &[(i32, i32)] {
        &self.trace
    }

    /// The score factor state.
    pub fn factor_state(&self) -> &ScoreFactorState {
        &self.score.calc.state
    }

    /// The life stored at the end of the last frame.
    pub fn current_life(&self) -> i32 {
        self.life.current_life
    }

    /// Live combo controller's current value, after this frame's converted results.
    pub fn current_combo(&self) -> i32 {
        self.current_combo
    }

    /// This frame's converted `(note id, judgement, chart time_ms)` results, including unscored Pass events.
    pub fn frame_judgements(&self) -> &[(i32, i32, i32)] {
        &self.judged
    }

    /// Native frame-result score, captured after skill updates and before Gekisou ranking's timestamp queries.
    /// [`LiveModel::score`] remains the calculator's settled value after ranking.
    pub fn frame_score(&self) -> i32 {
        self.frame_score
    }

    /// Number of judgements converted by skills so far.
    pub fn converted_judgements(&self) -> u64 {
        self.conversion.converted
    }

    /// Applied confirmations as (range, rank, awarded bonus, explicit percentage).
    pub fn gekisou_rank_bonuses(&self) -> &[(usize, i32, i32, i64)] {
        self.gk.as_ref().map_or(&[], |g| g.rank_bonus.as_slice())
    }

    /// Rank bonus score counted again by frames executed after score rewind.
    pub fn rank_bonus_score(&self) -> i32 {
        self.score.rank_bonus
    }

    /// The Gekisou ranges (empty without Gekisou).
    pub fn gekisou_ranges(&self) -> Vec<GekisouRange> {
        let Some(gk) = &self.gk else { return Vec::new() };
        let c = &gk.ctrl;
        c.ranges
            .iter()
            .zip(&c.states)
            .enumerate()
            .map(|(i, (r, s))| GekisouRange {
                mission: r.mission,
                state: s.state,
                combo: s.combo,
                max_combo: s.max_combo,
                just_count: s.just,
                start_score: s.start_score,
                end_score: s.end_score,
                luck_points: s.luck.total_bonus_point,
                luck_gauge: s.luck.gauge,
                rush_combo: s.luck.rush_combo,
                lot_results: s.luck.results,
                rank_bonus: gk.rank_bonus.iter().find(|b| b.0 == i).map(|b| b.2),
            })
            .collect()
    }

    /// Plays one frame at music time `t` (delta time 0).
    pub fn frame(&mut self, t: i32, judged: &[JudgedNote]) -> Result<(), Error> {
        self.frame_timed(t, judged, 0f32)
    }

    /// Plays one frame at music time `t` with delta time `dt` in seconds.
    pub fn frame_timed(&mut self, t: i32, judged: &[JudgedNote], dt: f32) -> Result<(), Error> {
        if self.raw_pending.is_some() {
            return Err(Error::Input("raw frame is open".into()));
        }
        if self.raw_runtime.is_some() && !judged.is_empty() {
            return Err(Error::Input(
                "raw window runtime requires complete raw results, not a judgement stream".into(),
            ));
        }
        self.begin_frame_internal(t, dt)?;
        // judgement conversion and the combo counter, note by note
        let mut results = Vec::with_capacity(judged.len());
        for j in judged {
            let n = *self.notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
            let conv = self.conversion.convert(j.judgement, n.judgement_type, j.judgement_time_ms)?;
            self.combo.add_judgement(n.time_ms, conv)?;
            results.push((n, conv));
            self.judged.push((n.note_id, conv, n.time_ms));
        }
        self.finish_frame_internal(t, results, &[])
    }

    /// Configures raw timing before the first frame. Runtime timings are explicit client data.
    pub fn enable_raw_runtime(&mut self, runtime: RawJudgementRuntime) -> Result<(), Error> {
        if self.raw_pending.is_some() || !self.trace.is_empty() {
            return Err(Error::Input("raw runtime must be configured before play".into()));
        }
        self.raw_runtime = Some(runtime);
        Ok(())
    }

    /// The raw judgement runtime, if configured.
    pub fn raw_runtime(&self) -> Option<&RawJudgementRuntime> {
        self.raw_runtime.as_ref()
    }

    /// The raw judgement runtime, if configured, for callers that set its client data (diff converter, Assist).
    pub fn raw_runtime_mut(&mut self) -> Option<&mut RawJudgementRuntime> {
        self.raw_runtime.as_mut()
    }

    /// Starts FT's frame. Submit each updater result immediately to feed converted grades back to NoteLine.
    pub fn begin_raw_frame(&mut self, t: i32, dt: f32) -> Result<(), Error> {
        if self.raw_runtime.is_none() {
            return Err(Error::Input("raw timing runtime is not configured".into()));
        }
        if self.raw_pending.is_some() {
            return Err(Error::Input("raw frame is already open".into()));
        }
        self.begin_frame_internal(t, dt)?;
        self.raw_pending = Some(Vec::new());
        Ok(())
    }

    /// Converter, then the Assist tail hook (only when no skill converter changed the grade), then DiffMsConverter;
    /// origin fields are preserved. Window limit callbacks run later in [`LiveModel::finish_raw_frame`], not while
    /// FT is still running its updaters.
    pub fn submit_raw_judgement(&mut self, mut j: RawJudgedNote) -> Result<crate::live::raw::NoteResult, Error> {
        if self.raw_pending.is_none() {
            return Err(Error::Input("no raw frame is open".into()));
        }
        let n = *self.notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
        if n.judgement_type != j.result.judgement_type {
            return Err(Error::Input("raw judgement type differs from chart".into()));
        }
        let incoming_grade = j.result.judgement;
        j.result.judgement = self.conversion.convert(incoming_grade, n.judgement_type, j.result.time_ms)?;
        let runtime =
            self.raw_runtime.as_mut().ok_or_else(|| Error::Input("raw timing runtime is not configured".into()))?;
        let unchanged_by_skill = j.result.judgement == incoming_grade;
        runtime.convert_assist(&mut j, unchanged_by_skill)?;
        j.result.diff_ms = (runtime.diff_converter)(j.result.judgement, j.result.diff_ms);
        self.combo.add_judgement(n.time_ms, j.result.judgement)?;
        self.judged.push((n.note_id, j.result.judgement, n.time_ms));
        self.raw_pending.get_or_insert_with(Vec::new).push(j);
        Ok(j.result)
    }

    /// Finishes FT, then consumes the results in LiveExecutor order, including the window callbacks and their
    /// deferred dictionary deletion before the skill appliers update.
    pub fn finish_raw_frame(&mut self) -> Result<(), Error> {
        let raw = self.raw_pending.take().ok_or_else(|| Error::Input("no raw frame is open".into()))?;
        let results = raw.iter().map(|j| (self.notes[&j.note_id], j.result.judgement)).collect();
        self.finish_frame_internal(self.frame_time, results, &raw)
    }

    /// Convenience only: for callers not requiring per-note NoteLine feedback.
    pub fn frame_raw_timed(
        &mut self,
        t: i32,
        judged: &[RawJudgedNote],
        dt: f32,
    ) -> Result<Vec<crate::live::raw::NoteResult>, Error> {
        self.begin_raw_frame(t, dt)?;
        let mut out = Vec::with_capacity(judged.len());
        for &note in judged {
            out.push(self.submit_raw_judgement(note)?);
        }
        self.finish_raw_frame()?;
        Ok(out)
    }

    fn begin_frame_internal(&mut self, t: i32, dt: f32) -> Result<(), Error> {
        self.frame_time = t;
        self.frame_rank_confirmation = self.prev_confirmed_rank.take();
        if let Some(gk) = self.gk.as_mut() {
            gk.fever.update(t, &mut gk.fever_updates);
            let current = self.score.score;
            let mut h = Handle { sc: &mut self.scorectl, score: &mut self.score };
            let mut env = Env { random: &mut self.random, handle: &mut h };
            gk.ctrl.before_update(dt, t, &gk.fever_updates, current, &mut env)?;
            // PlayingState.OnBeforeUpdateGekisou dispatches each changed range before FT updates notes.
            // Just missions enable grade 6 at Start, then restore the client's force setting at End.
            if let Some(runtime) = self.raw_runtime.as_mut() {
                for &idx in &gk.ctrl.state_updates {
                    if gk.ctrl.ranges[idx].mission == 3 {
                        let enabled = match gk.ctrl.states[idx].state {
                            gekisou::S_START => true,
                            gekisou::S_END => runtime.force_enable_just_judgement,
                            _ => continue,
                        };
                        runtime.set_just_judgement_enabled(enabled);
                    }
                }
            }
        }
        self.frame_events.clear();
        for (i, &(index, ev_t)) in self.events.iter().enumerate() {
            if !self.fired[i] && ev_t <= t {
                self.fired[i] = true;
                self.frame_events.push((index, ev_t));
            }
        }
        self.judged.clear();
        Ok(())
    }

    fn finish_frame_internal(
        &mut self,
        t: i32,
        results: Vec<(LiveNote, i32)>,
        raw: &[RawJudgedNote],
    ) -> Result<(), Error> {
        let frame_rank_confirmation = self.frame_rank_confirmation.take();
        // LiveExecutor.OnUpdate (Assist level) runs after FT, before UpdateCurrentFrameParameters.
        if let Some(runtime) = self.raw_runtime.as_mut() {
            runtime.after_ft(&results)?;
        }
        // The live combo controller consumes the simulator's frame delta, not the score combo directly.
        let simulator_combo = self.combo.timing_combo(t)?;
        let added_combo = simulator_combo.wrapping_sub(self.simulator_previous_combo).max(0);
        self.simulator_previous_combo = simulator_combo;
        if results.iter().any(|(_, judgement)| matches!(judgement, 1 | 2)) {
            self.current_combo = 0;
        }
        self.current_combo = self.current_combo.wrapping_add(added_combo);
        // note damage, frozen life and the note score commands
        for (result_index, &(n, conv)) in results.iter().enumerate() {
            self.life.add_note_damage(n.time_ms, conv)?;
            let life = self.life.get_life_at_ms(n.time_ms)?;
            // LiveScoreController.AddNoteScore checks the operation table before converting the score type.
            // Unscored simulator results (including type 122 / Pass) still reach life, combo and callbacks.
            if self.score.calc.note_factor_percent.contains_key(&n.note_operate_type) {
                let score_type = convert_score_type(conv as i64)?;
                self.score.add_note(NoteCommand::new(n.time_ms, life, n.note_id, n.note_operate_type, score_type));
            }
            // Window limit callbacks per consumed result (UpdateCurrentFrameParameters).
            if let (Some(note), Some(runtime)) = (raw.get(result_index), self.raw_runtime.as_mut()) {
                runtime.on_executor_judgement(*note)?;
            }
        }
        if let Some(runtime) = self.raw_runtime.as_mut() {
            runtime.frame_finish();
        }
        let info = self.gk.as_ref().map(|g| &g.ctrl as &dyn GekisouComboInfo);
        self.score.calculate(t, &self.combo, info)?;
        // skills
        let inp =
            FrameInput { time_ms: t, music_length_ms: self.music_length_ms, is_live_finished: self.is_live_finished };
        for &si in &self.enabled_live {
            self.live[si].trigger = TriggerResult::default();
        }
        for pool in &mut self.live_pools {
            if let Some(&(_, time_ms)) =
                self.frame_events.iter().rev().find(|&&(index, _)| index as i64 == pool.member as i64)
            {
                let si = pool.available.pop_front().ok_or_else(|| Error::Game("live skill pool is empty".into()))?;
                let s = &mut self.live[si];
                s.trigger = TriggerResult { is_trigger: true, time_ms };
                // Execute calls Stay, not UpdateEndFrame: counters and checker state survive.
                for e in &mut s.effects {
                    e.state.state = STAY;
                }
                self.enabled_live.push(si);
            }
        }
        self.enabled_live.sort_by_key(|&i| self.live[i].key.wrapping_mul(10).wrapping_add(self.live[i].index as i64));
        for c in self.cond.iter_mut() {
            c.updater.begin_frame();
        }
        let mut listed = Vec::new();
        for ph in PHASES {
            listed.clear();
            let gk_view =
                self.gk.as_ref().map(|g| GkView { ctrl: &g.ctrl, prev_lots: &g.prev_lots, prev_lot_ms: g.prev_lot_ms });
            let mut ctx = CheckCtx {
                life: &mut self.life,
                random: &mut self.random,
                frame_time: t,
                current_combo: self.current_combo,
                judged: &self.judged,
                events: &self.frame_events,
                gk: gk_view,
                prev_confirmed_rank: frame_rank_confirmation,
            };
            for &si in &self.enabled_live {
                let s = &mut self.live[si];
                if s.parent_state == END_FRAME {
                    s.parent_state = STAY;
                    // Parent cleanup calls Stay; it must not reset cumulative counters.
                    for e in &mut s.effects {
                        e.state.state = STAY;
                    }
                    continue;
                }
                let mut changed = false;
                for e in &mut s.effects {
                    if ph < 1 || e.phase == ph {
                        changed |= e.update(inp, s.trigger, &mut ctx)?;
                    }
                }
                if changed {
                    s.parent_state = aggregate_live_state(&s.effects);
                }
                for (ei, e) in s.effects.iter().enumerate() {
                    if e.state.state != STAY && (ph < 1 || e.phase == ph) {
                        listed.push(Listed::Live { skill: si, effect: ei });
                    }
                }
            }
            for (ui, c) in self.cond.iter_mut().enumerate() {
                for x in c.updater.update(ph, inp, &mut ctx)? {
                    if c.updater.updaters[x].state.state != STAY {
                        listed.push(Listed::Cond { updater: ui, u: x });
                    }
                }
            }
            for &item in &listed {
                self.apply(item)?;
            }
        }
        // EndFrame: a newly freed instance is not available to this frame's triggers.
        for &si in &self.enabled_live {
            let s = &self.live[si];
            if s.parent_state == STAY {
                let pool = self
                    .live_pools
                    .iter_mut()
                    .find(|p| p.key == s.key && p.member == s.member)
                    .ok_or_else(|| Error::Game("missing live skill pool".into()))?;
                pool.available.push_back(si);
            }
        }
        self.enabled_live.retain(|&si| self.live[si].parent_state != STAY);
        self.life.sync_current_life(t)?;
        let info = self.gk.as_ref().map(|g| &g.ctrl as &dyn GekisouComboInfo);
        self.score.calculate(t, &self.combo, info)?;
        self.frame_score = self.score.score;
        if self.gk.is_some() {
            self.gekisou_after(t, &results)?;
        }
        self.trace.push((t, self.score.score));
        Ok(())
    }

    /// The Gekisou update after the frame: the ranges take the judged notes; the first range that completed this
    /// frame gets its start and end scores and its rank bonus.
    fn gekisou_after(&mut self, t: i32, results: &[(LiveNote, i32)]) -> Result<(), Error> {
        let Some(gk) = self.gk.as_mut() else { return Ok(()) };
        let judged: Vec<GkNote> =
            results.iter().map(|(n, j)| (n.note_id, n.note_operate_type, n.time_ms, *j)).collect();
        let current = self.score.score;
        {
            let mut h = Handle { sc: &mut self.scorectl, score: &mut self.score };
            let mut env = Env { random: &mut self.random, handle: &mut h };
            gk.ctrl.update(t, &judged, &gk.fever_updates, current, &mut env)?;
        }
        gk.prev_lots.clear();
        gk.prev_lots.extend_from_slice(&gk.ctrl.lot_results);
        gk.prev_lot_ms = if gk.prev_lots.is_empty() { 0 } else { t };
        if gk.external_ranking {
            // Network ranking reads the range's score snapshots taken by the controller. Unlike solo
            // ranking, it does not rewind the score calculator to the range's start and end timestamps.
            for idx in gk.ctrl.state_updates.iter().copied() {
                if gk.ctrl.states[idx].state != S_COMPLETE || gk.completed_scores[idx] {
                    continue;
                }
                // The fixed-rank solo adapter retains solo's timestamp queries explicitly; network
                // ranking keeps the controller snapshots and never enters this branch.
                if gk.solo_score_queries {
                    let r = &gk.ctrl.ranges[idx];
                    let info = Some(&gk.ctrl as &dyn GekisouComboInfo);
                    let s0 = self.score.calculate(r.start_ms, &self.combo, info)?;
                    let s1 = self.score.calculate(r.end_ms, &self.combo, info)?;
                    gk.ctrl.states[idx].start_score = s0;
                    gk.ctrl.states[idx].end_score = s1;
                }
                gk.completed_scores[idx] = true;
                // SoloGekisouRankingUpdater returns after the first completed range, including when
                // overlapping ranges complete together. Keep that schedule in the solo adapter.
                if gk.solo_score_queries {
                    break;
                }
            }
            for idx in 0..gk.pending_ranks.len() {
                if !gk.completed_scores[idx] {
                    continue;
                }
                let Some((rank, pct)) = gk.pending_ranks[idx].take() else {
                    continue;
                };
                let bonus = ((i128::from(gk.ctrl.range_score(idx)) * i128::from(pct)) / 100) as i32;
                self.score.add_fixed(gk.ctrl.ranges[idx].end_ms, bonus);
                gk.rank_bonus.push((idx, rank, bonus, pct));
                self.prev_confirmed_rank = Some(rank);
            }
            return Ok(());
        }
        let Some(idx) = gk.ctrl.state_updates.iter().copied().find(|&i| gk.ctrl.states[i].state == S_COMPLETE) else {
            return Ok(());
        };
        let (start, end) = (gk.ctrl.ranges[idx].start_ms, gk.ctrl.ranges[idx].end_ms);
        let info = Some(&gk.ctrl as &dyn GekisouComboInfo);
        let s0 = self.score.calculate(start, &self.combo, info)?;
        let s1 = self.score.calculate(end, &self.combo, info)?;
        gk.ctrl.states[idx].start_score = s0;
        gk.ctrl.states[idx].end_score = s1;
        if idx < gk.factors.len() {
            let (rank, bonus, pct) = gekisou::solo_rank_bonus(idx, gk.ctrl.range_score(idx), &gk.factors);
            self.score.add_fixed(end, bonus);
            gk.rank_bonus.push((idx, rank, bonus, pct));
            self.prev_confirmed_rank = Some(rank); // feed actual confirmation into the NEXT frame
        }
        Ok(())
    }

    fn state_mut(&mut self, item: Listed) -> &mut EffectState {
        match item {
            Listed::Live { skill, effect } => &mut self.live[skill].effects[effect].state,
            Listed::Cond { updater, u } => &mut self.cond[updater].updater.updaters[u].state,
        }
    }

    fn apply(&mut self, item: Listed) -> Result<(), Error> {
        let (ri, k, owner_type, skill_type, key) = match item {
            Listed::Live { skill, effect } => {
                let s = &self.live[skill];
                let ri = s.effects[effect].row;
                (
                    ri,
                    s.member,
                    OWNER_MEMBER,
                    1,
                    StateKey::Live { row_id: self.rows[ri].id, member: s.member, index: s.index },
                )
            }
            Listed::Cond { updater, u } => {
                let c = &self.cond[updater];
                let upd = &c.updater.updaters[u];
                let ef = &c.updater.effects[upd.effect];
                let owner_type = if c.skill_type == SKILL_TYPE_GEKISOU { OWNER_MEMBER } else { OWNER_SNAP };
                (
                    ef.row,
                    c.member,
                    owner_type,
                    c.skill_type,
                    StateKey::Cond { effect_id: ef.effect_id, index: upd.index },
                )
            }
        };
        let mut st = *self.state_mut(item);
        let owner = (k as i32).wrapping_mul(100).wrapping_add(owner_type);
        let gk_skill = skill_type == SKILL_TYPE_GEKISOU || skill_type == SKILL_TYPE_GEKISOU_SUPPORT;
        let effect_type = self.rows[ri].effect_type;
        let gk_type = GEKISOU_APPLIER_TYPES.contains(&effect_type);
        if (gk_type || effect_type == 13001) && !gk_skill && self.gk.is_none() {
            // No applier is registered for this type without Gekisou (AppendGekisouSkillApplier 0x61291c8 also
            // registers 13001 only with a Gekisou controller): SkillApplierController.Update 0x560da10 skips an
            // effect state whose type TryGetApplier does not find.
            return Ok(());
        }
        if matches!(effect_type, 4000..=4004 | 13001)
            && let Some(runtime) = self.raw_runtime.as_mut()
        {
            if let Some(time) = runtime.update_effect(key, st.state, &self.rows[ri])? {
                let state = self.state_mut(item);
                state.state = END_FRAME;
                state.finish_ms = time;
            }
            return Ok(());
        }
        if gk_skill || gk_type {
            let done = self.apply_gekisou(ri, &mut st, owner, key);
            *self.state_mut(item) = st;
            if done? {
                return Ok(());
            }
        }
        let (effect_type, value) = (self.rows[ri].effect_type, self.rows[ri].effect_value);
        match effect_type {
            // No live applier is registered for these types (AppendCommonSkillApplier 0x6128cd0 /
            // AppendGekisouSkillApplier 0x61291c8); SkillApplierController.Update 0x560da10 skips them. The parameter
            // types are read by pre-live power consumers. Their updater / condition phases still run.
            0 | 1000..=1003 | 1500..=1503 => {}
            13005 if self.gk.is_none() => {}
            2000 | 2005 => {
                let divisor = if effect_type == 2005 { -10000f32 } else { 10000f32 };
                let m = note_factor_mill(value as f32 / divisor);
                if st.state == EXECUTE_FRAME {
                    self.score.add_factor(FactorCommand {
                        time_ms: st.execute_ms,
                        owner_id: owner,
                        note_mill: m,
                        ..Default::default()
                    });
                } else if st.state == END_FRAME {
                    self.score.add_factor(FactorCommand {
                        time_ms: st.finish_ms,
                        owner_id: owner,
                        note_mill: m.wrapping_neg(),
                        ..Default::default()
                    });
                }
            }
            2001 | 2003 => self.apply_cumulative_score(ri, &st, owner, key)?,
            2002 => {
                let m = judgement_factor_mill(value as f32 / 10000f32);
                let command = match st.state {
                    EXECUTE_FRAME => Some((st.execute_ms, m)),
                    END_FRAME => Some((st.finish_ms, m.wrapping_neg())),
                    _ => None,
                };
                if let Some((time_ms, combo_mill)) = command {
                    self.score.add_factor(FactorCommand { time_ms, owner_id: owner, combo_mill, ..Default::default() });
                }
            }
            2004 => {
                let m = judgement_factor_mill(value as f32 / 10000f32);
                let targets = self.rows[ri].targets()?.to_vec();
                for j in targets {
                    let (time_ms, judge_mill) = match st.state {
                        EXECUTE_FRAME => (st.execute_ms, m),
                        END_FRAME => (st.finish_ms, m.wrapping_neg()),
                        _ => continue,
                    };
                    self.score.add_factor(FactorCommand {
                        time_ms,
                        owner_id: owner,
                        judgement: j as i32,
                        judge_mill,
                        ..Default::default()
                    });
                }
            }
            15000 => {
                if st.state == EXECUTE_FRAME {
                    self.extend(k, value as f32);
                }
            }
            3000 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.add_life_limit(value)?;
                    if self.life_limits.contains_key(&key) {
                        return Err(Error::Game("duplicate life limit effect state".into()));
                    }
                    self.life_limits.insert(key, id);
                } else if st.state == END_FRAME {
                    let id = self
                        .life_limits
                        .remove(&key)
                        .ok_or_else(|| Error::Game("missing life limit effect state".into()))?;
                    self.life.subtract_life_limit(id);
                }
            }
            3002 => {
                if st.state == EXECUTE_FRAME {
                    self.life.damage(st.execute_ms, value, true)?;
                }
            }
            3004 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.enable_damage_reduction(st.execute_ms, value)?;
                    if self.life_reductions.contains_key(&key) {
                        return Err(Error::Game("duplicate damage reduction effect state".into()));
                    }
                    self.life_reductions.insert(key, id);
                } else if st.state == END_FRAME
                    && let Some(id) = self.life_reductions.remove(&key)
                {
                    self.life.disable_damage_reduction(st.finish_ms, id)?;
                }
            }
            3001 => {
                if st.state == EXECUTE_FRAME {
                    self.life.recovery(st.execute_ms, value, true)?;
                }
            }
            3003 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.enable_guard(st.execute_ms)?;
                    if self.guards.contains_key(&key) {
                        return Err(Error::Game("duplicate guard effect state".into()));
                    }
                    self.guards.insert(key, id);
                } else if st.state == END_FRAME {
                    let id = *self.guards.get(&key).ok_or_else(|| Error::Game("missing guard effect state".into()))?;
                    self.life.disable_guard(st.finish_ms, id)?;
                    self.guards.remove(&key);
                }
            }
            12006 | 13005 => {
                let row = &self.rows[ri];
                let effect = ConvertEffect {
                    effect_type,
                    effect_value: value,
                    effect_limit_count: row.effect_limit_count,
                    targets: row.targets()?,
                    effect_id: row.id,
                };
                if let Some(t) = self.conversion.update(key, st.state, &effect)? {
                    let s = self.state_mut(item);
                    s.state = END_FRAME;
                    s.finish_ms = t;
                }
            }
            t => return Err(Error::Unsupported(format!("skill effect type {t}"))),
        }
        Ok(())
    }

    /// Common cumulative score factors. The established Gekisou 2001 route stays in `apply_gekisou`.
    fn apply_cumulative_score(&mut self, ri: usize, st: &EffectState, owner: i32, key: StateKey) -> Result<(), Error> {
        let row = &self.rows[ri];
        let combo = row.effect_type == 2003;
        let mut value = (row.effect_value as i32).wrapping_mul(st.cumulative_count as i32);
        let max = row.max_effect_value as i32;
        if max > 0 {
            value = value.min(max);
        }
        let factor = value as f32 / 10000f32;
        let (ids, sc, score) = (&mut self.gk_appliers, &mut self.scorectl, &mut self.score);
        let add = |sc: &mut ScoreCtl, score: &mut IncrementalCalculator, t| {
            if combo {
                sc.add_combo_bonus(score, owner, t, factor)
            } else {
                sc.add_note_score_up(score, owner, t, factor)
            }
        };
        let disable = |sc: &mut ScoreCtl, score: &mut IncrementalCalculator, t, id| {
            if combo { sc.disable_combo_bonus(score, t, id) } else { sc.disable_note_score_up(score, t, id) }
        };
        match st.state {
            EXECUTE_FRAME => {
                let id = add(sc, score, st.execute_ms);
                ids.add(key, id)?;
            }
            EXECUTING => {
                let id = *ids.ids.get(&key).ok_or_else(|| Error::Game("effect state not registered".into()))?;
                let old = if combo { sc.combo_bonus_factor(id) } else { sc.score_up_factor(id) };
                if !approximately(old, factor) {
                    disable(sc, score, self.frame_time, id)?;
                    let id = add(sc, score, self.frame_time);
                    ids.ids.insert(key, id);
                }
            }
            END_FRAME => {
                let id = ids.pop(key)?;
                disable(sc, score, st.finish_ms, id)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The Gekisou appliers; `Ok(false)` when the effect type is not one of them.
    fn apply_gekisou(&mut self, ri: usize, st: &mut EffectState, owner: i32, key: StateKey) -> Result<bool, Error> {
        let row = &self.rows[ri];
        let (et, v) = (row.effect_type, row.effect_value);
        let frame_t = self.frame_time;
        let ga = &mut self.gk_appliers;
        let c = &mut self.gk.as_mut().ok_or_else(|| Error::Game("Gekisou effect without Gekisou".into()))?.ctrl;
        let s = st.state;
        match et {
            12000 | 13000 => {
                if s == EXECUTE_FRAME {
                    let id = if et == 12000 {
                        c.add_combo_bonus(st.execute_ms, v as f32)
                    } else {
                        c.add_just_bonus(st.execute_ms, v as f32)
                    };
                    ga.add(key, id)?;
                } else if s == END_FRAME {
                    let id = ga.pop(key)?;
                    if et == 12000 {
                        c.subtract_combo_bonus(st.finish_ms, id)?;
                    } else {
                        c.subtract_just_bonus(st.finish_ms, id)?;
                    }
                }
            }
            13002 => {
                if s == END_FRAME {
                    if let Some(id) = ga.ids.remove(&key) {
                        c.remove_cumulative_rule(id);
                    }
                } else if s == EXECUTE_FRAME && !ga.ids.contains_key(&key) {
                    let id = c.add_cumulative_rule(st.cumulative_unit, v, st.cumulative_max, row.max_effect_value);
                    ga.ids.insert(key, id);
                }
            }
            11000 | 11001 => {
                if s == EXECUTE_FRAME {
                    let f = v as f32 / 10000f32;
                    let id = if et == 11000 {
                        c.add_lot_probability_up(st.execute_ms, f)
                    } else {
                        c.add_gauge_up(st.execute_ms, f)
                    };
                    ga.add(key, id)?;
                } else if s == END_FRAME {
                    let id = ga.pop(key)?;
                    if et == 11000 {
                        c.subtract_lot_probability_up(st.finish_ms, id)?;
                    } else {
                        c.subtract_gauge_up(st.finish_ms, id)?;
                    }
                }
            }
            11002 | 11004 | 12002 | 12003 | 13003 | 13004 => {
                if s == EXECUTE_FRAME {
                    let mut count = v as i32;
                    if matches!(et, 11004 | 12003 | 13004) {
                        count = count.wrapping_mul(st.cumulative_count as i32);
                        let cap = row.max_effect_value as i32;
                        if cap > 0 {
                            count = count.min(cap);
                        }
                    }
                    match et {
                        11002 | 11004 => c.add_luck_point(i64::from(count)),
                        12002 | 12003 => c.add_gekisou_combo(frame_t, count),
                        _ => c.add_just_count(frame_t, count),
                    }
                }
            }
            11003 => {
                if s == EXECUTE_FRAME {
                    let g = (c.current_gauge_max() as i128 * v as i128) as i32;
                    c.add_luck_gauge_percent(floor_to_i32(g as f32 / 10000f32))?;
                }
            }
            12004 => {
                ga.limit(key, st);
                let s = st.state;
                if s == EXECUTE_FRAME {
                    let mut mask = 0i64;
                    for &j in row.targets()? {
                        if j != 0 && j != -1 {
                            mask |= 1i64 << (j & 31);
                        }
                    }
                    let id = c.enable_combo_protect(st.execute_ms, row.effect_limit_count, (mask & 0xFF) as i32);
                    ga.add(key, id)?;
                } else if s == END_FRAME
                    && !ga.exhausted.remove(&key)
                    && let Some(id) = ga.ids.remove(&key)
                {
                    c.disable_combo_protect(st.finish_ms, id);
                }
            }
            11005 => {
                ga.limit(key, st);
                let s = st.state;
                if s == EXECUTE_FRAME {
                    let id = c.machine.enable_minimum(minimum_result_of(v), row.effect_limit_count);
                    ga.add(key, id)?;
                } else if s == EXECUTING {
                    if let Some(&id) = ga.ids.get(&key)
                        && !c.machine.is_minimum_active(id)
                    {
                        let mut t = c.machine.take_last_consumed(id);
                        if t < 0 {
                            t = frame_t;
                        }
                        c.machine.disable_minimum(id);
                        ga.ids.remove(&key);
                        ga.exhausted.insert(key);
                        ga.limit_finished.insert(key, t);
                    }
                } else if s == END_FRAME
                    && !ga.exhausted.remove(&key)
                    && let Some(id) = ga.ids.remove(&key)
                {
                    c.machine.disable_minimum(id);
                }
            }
            2001 => {
                let mut n = (v as i128 * st.cumulative_count as i128) as i32;
                if row.max_effect_value > 0 && row.max_effect_value < n as i64 {
                    n = row.max_effect_value as i32;
                }
                let f = n as f32 / 10000f32;
                let (sc, score) = (&mut self.scorectl, &mut self.score);
                if s == EXECUTE_FRAME {
                    let id = sc.add_note_score_up(score, owner, st.execute_ms, f);
                    ga.add(key, id)?;
                } else if s == EXECUTING {
                    let id = *ga.ids.get(&key).ok_or_else(|| Error::Game("effect state not registered".into()))?;
                    if !approximately(sc.score_up_factor(id), f) {
                        sc.disable_note_score_up(score, frame_t, id)?;
                        let id = sc.add_note_score_up(score, owner, frame_t, f);
                        ga.ids.insert(key, id);
                    }
                } else if s == END_FRAME {
                    let id = ga.pop(key)?;
                    sc.disable_note_score_up(score, st.finish_ms, id)?;
                }
            }
            4004 => {}
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Extends every running live skill effect of the member.
    fn extend(&mut self, member: usize, ms: f32) {
        for s in self.live.iter_mut() {
            if s.member != member {
                continue;
            }
            for e in s.effects.iter_mut() {
                if e.state.state == EXECUTE_FRAME || e.state.state == EXECUTING {
                    e.state.extended_ms += ms;
                }
            }
        }
    }
}

#[cfg(test)]
mod pool_regression;

#[cfg(test)]
mod gaps_tests;

#[cfg(test)]
mod effect_state_tests {
    use super::*;
    use serde_json::json;

    fn guard_model() -> LiveModel {
        let tables = json!({
            "MasterLiveSettings": [
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"}],
            "MasterLiveSkillEffect": [{"_id":1,"_liveSkillID":1,"_level":1,
                "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
                "_skillEffectType":3003,"_activationTimeSecond":1.0,"_effectValue":0,
                "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
                "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]
        });
        let texts: Vec<_> =
            tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
        let master =
            Master::from_json_tables(|name| texts.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())).unwrap();
        let p = Performer { live_skill: Some((1, 1)), ..Default::default() };
        let params = LiveParams {
            total_power: 1,
            music_level: 1,
            converted_note_count: 1,
            music_length_ms: 1000,
            skill_target_music_type: 0,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        LiveModel::new(&master, &[p], &[], &[], params).unwrap()
    }

    #[test]
    fn guard_applier_rejects_missing_and_duplicate_states() {
        let mut model = guard_model();
        let item = Listed::Live { skill: 0, effect: 0 };
        model.state_mut(item).state = END_FRAME;
        assert!(matches!(model.apply(item), Err(Error::Game(_))));
        model.state_mut(item).state = EXECUTE_FRAME;
        model.apply(item).unwrap();
        let ids = model.guards.clone();
        assert!(matches!(model.apply(item), Err(Error::Game(_))));
        assert_eq!(model.guards, ids); // Dictionary.Add keeps the old value when a duplicate is rejected.
        model.state_mut(item).state = END_FRAME;
        model.apply(item).unwrap();
        assert!(model.guards.is_empty());
        assert!(matches!(model.apply(item), Err(Error::Game(_))));
    }
}
