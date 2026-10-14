//! The Gekisou (fever section) controller: range states, the Gekisou combo and Just counts with their bonuses and
//! protections, the luck gauge with its lottery, and the solo rank bonus.
//!
//! Every chart fever is one Gekisou range with a mission (1 combo, 2 luck, 3 Just count). A range moves Wait ->
//! Standby (less than 4001 ms before its start) -> Start (fever start) -> Playing -> End (fever end) -> Delay (after
//! the longest judgement window, counted with the frame delta times) -> Complete (after 500 ms more) -> Finish.
//! Judged notes of a range are kept in a history; the combo and Just counts are recomputed from it whenever a skill
//! changes a bonus, a protection or a cumulative rule. Luck ranges draw lottery results from the luck random stream.

use crate::error::Error;
use crate::live::random::{LUCK, LiveRandom};
use crate::live::score::GekisouComboInfo;
use crate::master::Master;
use crate::num::{FxHashMap, FxHashSet, ceil_to_i32, floor_to_i32};

mod commands;
mod factors;
mod lottery;
mod luck_score;
mod rank;
mod recount;
mod update;
use factors::FactorStorage;
pub(crate) use factors::{exact_sums, factor_at};
pub(crate) use lottery::LotteryMachine;
pub(crate) use luck_score::LuckScore;
pub use rank::{
    BrokenGekisouRanking, NetworkGekisouRanking, NetworkGekisouResult, calculate_network_gekisou_ranking,
    gekisou_no_input,
};
pub(crate) use rank::{mission_pattern, ranking_factors, solo_rank_bonus};
use recount::{Resume, Rule};

// judgements
const J_WAIT: i32 = 0;
const J_GOOD: i32 = 3;
const J_GREAT: i32 = 4;
const J_PERFECT: i32 = 5;
const J_JUST: i32 = 6;
const J_PASS: i32 = 7;

// missions
pub(crate) const M_COMBO: i64 = 1;
pub(crate) const M_LUCK: i64 = 2;
pub(crate) const M_ALL: i64 = 4;

// range states
pub(crate) const S_WAIT: u8 = 1;
pub(crate) const S_STANDBY: u8 = 2;
pub(crate) const S_START: u8 = 3;
pub(crate) const S_PLAYING: u8 = 4;
pub(crate) const S_END: u8 = 5;
pub(crate) const S_DELAY: u8 = 6;
pub(crate) const S_COMPLETE: u8 = 7;
pub(crate) const S_FINISH: u8 = 8;

// fever states
const FEVER_WAIT: u8 = 1;
const FEVER_FEVER: u8 = 2;
pub(crate) const FEVER_END: u8 = 3;

// lottery results and lot types
const INVALID: i64 = -1;
const MISS_R: i64 = 0;
const CRITICAL: i64 = 3;
const NONE_LOT: usize = 0;
const CHANCE_LOW: usize = 1;
/// Lot type by rush combo 0..=3 (none, first, second, third rush).
const LOT_TYPE_BY_RUSH: [usize; 4] = [NONE_LOT, 4, 3, 2];
/// Bonus points of a Hit, Super Hit and Critical.
pub(super) const BONUS_POINT_BY_RESULT: [i32; 3] = [5, 10, 10];

const COMPLETE_DELAY_MS: i64 = 500;
const STANDBY_MS: i32 = 4001;
const SCORE_FRAME_MS: i32 = 40;
/// Gekisou ranges of a live.
pub(crate) const MAX_RANGES: usize = 3;
/// A fever beyond the ranges changed state: the controller's range-state bounds check fails.
const FEVER_WITHOUT_RANGE: &str = "IndexOutOfRangeException: fever without a Gekisou range";

/// Note types that never add luck.
const NON_LUCK_NOTE_TYPES: [i32; 11] = [0, 80, 82, 100, 101, 102, 103, 104, 105, 121, 122];

fn sdiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

fn srem(a: i32, b: i32) -> i32 {
    a.wrapping_sub(sdiv(a, b).wrapping_mul(b))
}

/// A weight scaled by a buff: `floor((buff + 1) * weight)`.
fn buffed(buff: f32, weight: i64) -> i32 {
    floor_to_i32((buff + 1f32) * weight as f32)
}

fn game(msg: &str) -> Error {
    Error::Game(msg.into())
}

fn is_sub_note(nt: i32) -> bool {
    nt == 21 || nt == 120 || (nt as u32 & 0xFFFF_FFFC) == 0x3C
}

/// One Gekisou range: its fever, mission and target notes.
#[derive(Clone, Debug)]
pub(crate) struct Range {
    pub start_ms: i32,
    pub end_ms: i32,
    pub mission: i64,
    targets: FxHashSet<i32>,
}

/// The state of one range.
#[derive(Clone, Debug)]
pub(crate) struct RangeState {
    pub state: u8,
    time_to: i32,
    pub combo: i32,
    pub max_combo: i32,
    pub just: i32,
    pub raw_just: i32,
    pub cum_base: i32,
    pub start_score: i32,
    pub end_score: i32,
    pub last_combo_ms: i32,
    pub last_just_ms: i32,
    pub luck: LuckScore,
    sw_elapsed: f32,
    sw_stopped: bool,
}

impl RangeState {
    fn score(&self) -> i32 {
        self.end_score.wrapping_sub(self.start_score)
    }
}

/// The luck bonus handle of the score (the rush bonus).
pub(crate) trait LuckHandle {
    fn add(&mut self, t: i32, percent: i32) -> i32;
    fn disable(&mut self, t: i32, id: i32) -> Result<(), Error>;
}

/// What the controller draws from and reports to.
pub(crate) struct Env<'a> {
    pub random: &'a mut LiveRandom,
    pub handle: &'a mut dyn LuckHandle,
}

/// A judged note as the controller reads it: `(note id, note type, chart time, converted judgement)`.
pub(crate) type GkNote = (i32, i32, i32, i32);

#[derive(Clone, Debug)]
pub(crate) struct Controller {
    pub ranges: Vec<Range>,
    pub states: Vec<RangeState>,
    pub machine: LotteryMachine,
    storage: FactorStorage,
    rush_percent: i64,
    luck_gauge_max: i64,
    last_note_delay_ms: i64,
    rush_id: i32,
    /// The chart-time spans of the rush score bonus commands: `(added at, disabled at)`, `i32::MAX` while running.
    rush_log: Vec<(i32, i32)>,
    /// LUCK weighted lives (see [`super::LiveModel::set_luck_weights`]): no lottery is drawn and every luck range runs
    /// the rush from its start to its finish.
    pub luck_weighted: bool,
    playing: Vec<usize>,
    combo_bonus_ids: FxHashMap<i32, f32>,
    just_bonus_ids: FxHashMap<i32, f32>,
    rules: FxHashMap<i32, Rule>,
    handle_id: i32,
    needs_recalc: bool,
    full_recalc: bool,
    seq: i32,
    /// Per range: `(chart time, judgement, sequence)` of the judged target notes.
    history: Vec<Vec<(i32, i32, i32)>>,
    /// Per range: the combo after each history entry.
    snapshot: Vec<Vec<i32>>,
    /// Bumped whenever a range's history or snapshot may change.
    combo_versions: Vec<u64>,
    resume: Vec<Resume>,
    /// `(time, delta, judgement sequence)`.
    cb_stack: Vec<(i32, f32, i32)>,
    jb_stack: Vec<(i32, f32, i32)>,
    /// Fixed additions: (music time, float-converted int delta, captured range; -1 is wildcard).
    add_just_stack: Vec<(i32, f32, i32)>,
    add_combo_stack: Vec<(i32, f32, i32)>,
    /// `(time, delta, id, mask)`.
    pr_stack: Vec<(i32, i32, i32, i32)>,
    unlimited: Vec<(i32, i32)>,
    limited: Vec<(i32, i32, i32)>,
    limited_ids: FxHashSet<i32>,
    pending: Vec<Vec<(i32, i32, i32, i32)>>,
    lot_result: i64,
    /// This frame's lottery results.
    pub lot_results: Vec<i64>,
    /// Ranges whose state changed this frame.
    pub state_updates: Vec<usize>,
    /// The range that is playing this frame, or -1.
    pub current_playing_index: i32,
}

impl Controller {
    pub(crate) fn new(
        ranges: Vec<(i32, i32, i64)>,
        notes: impl Iterator<Item = (i32, i32)> + Clone,
        master: &Master,
        gauge_max: i64,
        gauge_max_rush: i64,
        rush_percent: i64,
        last_note_delay_ms: i64,
    ) -> Result<Controller, Error> {
        let ranges: Vec<Range> = ranges
            .into_iter()
            .map(|(s, e, mission)| Range {
                start_ms: s,
                end_ms: e,
                mission,
                targets: notes.clone().filter(|&(_, t)| s <= t && t <= e).map(|(id, _)| id).collect(),
            })
            .collect();
        let states = ranges
            .iter()
            .map(|r| {
                let mut luck = LuckScore::default();
                if r.mission == M_LUCK {
                    luck.initialize(gauge_max, gauge_max_rush);
                }
                RangeState {
                    state: S_WAIT,
                    time_to: 0,
                    combo: 0,
                    max_combo: 0,
                    just: 0,
                    raw_just: 0,
                    cum_base: 0,
                    start_score: 0,
                    end_score: 0,
                    last_combo_ms: -1,
                    last_just_ms: -1,
                    luck,
                    sw_elapsed: 0f32,
                    sw_stopped: true,
                }
            })
            .collect();
        let n = ranges.len();
        Ok(Controller {
            ranges,
            states,
            machine: LotteryMachine::from_master(master)?,
            storage: FactorStorage::default(),
            rush_percent,
            luck_gauge_max: gauge_max,
            last_note_delay_ms,
            rush_id: 0,
            rush_log: Vec::new(),
            luck_weighted: false,
            playing: Vec::new(),
            combo_bonus_ids: FxHashMap::default(),
            just_bonus_ids: FxHashMap::default(),
            rules: FxHashMap::default(),
            handle_id: 0,
            needs_recalc: false,
            full_recalc: false,
            seq: 0,
            history: vec![Vec::new(); n],
            snapshot: vec![Vec::new(); n],
            combo_versions: vec![0; n],
            resume: vec![Resume::default(); n],
            cb_stack: Vec::new(),
            jb_stack: Vec::new(),
            add_just_stack: Vec::new(),
            add_combo_stack: Vec::new(),
            pr_stack: Vec::new(),
            unlimited: Vec::new(),
            limited: Vec::new(),
            limited_ids: FxHashSet::default(),
            pending: vec![Vec::new(); n],
            lot_result: INVALID,
            lot_results: Vec::new(),
            state_updates: Vec::new(),
            current_playing_index: -1,
        })
    }

    // --- handles --------------------------------------------------------------------------------------------

    fn next_id(&mut self) -> i32 {
        self.handle_id = self.handle_id.wrapping_add(1);
        self.handle_id
    }

    fn flag(&mut self) {
        self.needs_recalc = true;
        self.full_recalc = true;
    }

    /// The last range that started and has not finished, or -1.
    fn current_playing_range_index(&self) -> i32 {
        self.playing.last().map_or(-1, |&i| i as i32)
    }

    pub(super) fn dp_playing_range_index(&self) -> i32 {
        self.current_playing_range_index()
    }

    /// The commands installed SO FAR, accumulated in native filing order with binary32 arithmetic.
    pub(super) fn dp_factors_at(&self, t: i32) -> (i32, f32) {
        (floor_to_i32(factor_at(&self.storage.lot_cmds, t) * 100f32), factor_at(&self.storage.gauge_cmds, t))
    }
}
/// The chart's fevers: each goes Wait -> Fever at its start and Fever -> End at its end (chart time), at most one step
/// per frame.
#[derive(Clone, Debug)]
pub(crate) struct FeverUpdater {
    fevers: Vec<(i32, i32)>,
    state: Vec<u8>,
}

impl FeverUpdater {
    pub(crate) fn new(fevers: &[(i32, i32)]) -> FeverUpdater {
        FeverUpdater { fevers: fevers.to_vec(), state: vec![FEVER_WAIT; fevers.len()] }
    }

    pub(crate) fn update(&mut self, t: i32, out: &mut Vec<(usize, u8)>) {
        out.clear();
        for (i, &(s, e)) in self.fevers.iter().enumerate() {
            let st = self.state[i];
            let next = if st == FEVER_WAIT && t >= s {
                FEVER_FEVER
            } else if st == FEVER_FEVER && t >= e {
                FEVER_END
            } else {
                continue;
            };
            self.state[i] = next;
            out.push((i, next));
        }
    }
}

#[cfg(test)]
mod fixed_addition_tests;
#[cfg(test)]
mod network_ranking_tests;
#[cfg(test)]
mod nominal_lottery_tests;
