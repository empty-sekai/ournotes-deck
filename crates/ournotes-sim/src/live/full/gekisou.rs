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
const FEVER_END: u8 = 3;

// lottery results and lot types
const INVALID: i64 = -1;
const MISS_R: i64 = 0;
const CRITICAL: i64 = 3;
const NONE_LOT: usize = 0;
const CHANCE_LOW: usize = 1;
/// Lot type by rush combo 0..=3 (none, first, second, third rush).
const LOT_TYPE_BY_RUSH: [usize; 4] = [NONE_LOT, 4, 3, 2];
/// Bonus points of a Hit, Super Hit and Critical.
const BONUS_POINT_BY_RESULT: [i32; 3] = [5, 10, 10];

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

/// One lottery item: `(weight, value)`.
type Item = (i64, i64);

/// A weighted draw over items in order: `|r| % total`, the first item whose running weight exceeds it.
fn lottery_table(items: &[Item], r: i32) -> Result<Option<i64>, Error> {
    if items.is_empty() {
        return Ok(None);
    }
    let total = items.iter().fold(0i32, |t, it| t.wrapping_add(it.0 as i32));
    if total < 1 {
        return Err(game("lottery table with a total weight below 1"));
    }
    let m = srem(r.wrapping_abs(), total);
    let mut cum = 0i32;
    for it in items {
        cum = cum.wrapping_add(it.0 as i32);
        if m < cum {
            return Ok(Some(it.1));
        }
    }
    Ok(None)
}

/// The luck bonus table of one lot type: items by descending result, weights scaled by the lot probability buff.
#[derive(Clone, Debug)]
struct LuckSkillTable {
    items: Vec<Item>,
    buff: f32,
    total_weight: i32,
}

impl LuckSkillTable {
    fn new(mut items: Vec<Item>) -> Result<LuckSkillTable, Error> {
        items.sort_by_key(|it| std::cmp::Reverse(it.1));
        let total = items.iter().fold(0i32, |t, it| t.wrapping_add(it.0 as i32));
        if !items.is_empty() && total < 1 {
            return Err(game("luck bonus table with a total weight below 1"));
        }
        Ok(LuckSkillTable { items, buff: 0f32, total_weight: total })
    }

    fn lottery(&self, r: i32) -> Option<i64> {
        if self.items.is_empty() {
            return None;
        }
        let m = srem(r.wrapping_abs(), self.total_weight);
        let mut cum = 0i32;
        for it in &self.items {
            cum = cum.wrapping_add(buffed(self.buff, it.0));
            if m < cum {
                return Some(it.1);
            }
        }
        None
    }

    /// The draw restricted to results `>= minimum`; the other items' weight is shared out over them.
    fn lottery_with_minimum(&self, r: i32, minimum: i64) -> Option<i64> {
        if self.items.is_empty() {
            return None;
        }
        let (mut inc_n, mut inc_sum, mut exc_sum) = (0i32, 0i32, 0i32);
        for it in &self.items {
            let w = buffed(self.buff, it.0);
            if minimum <= it.1 {
                inc_n += 1;
                inc_sum = inc_sum.wrapping_add(w);
            } else {
                exc_sum = exc_sum.wrapping_add(w);
            }
        }
        if inc_n == 0 {
            return None;
        }
        let per = sdiv(exc_sum, inc_n);
        let total = inc_sum.wrapping_add(per.wrapping_mul(inc_n));
        let m = srem(r.wrapping_abs(), total);
        let mut cum = 0i32;
        for it in &self.items {
            if minimum <= it.1 {
                cum = cum.wrapping_add(per).wrapping_add(buffed(self.buff, it.0));
                if m < cum {
                    return Some(it.1);
                }
            }
        }
        None
    }
}

/// The luck lottery: base points per judged note, bonus results per lot, minimum-result guarantees.
#[derive(Clone, Debug)]
pub(crate) struct LotteryMachine {
    good: Option<Vec<Item>>,
    great: Option<Vec<Item>>,
    perfect: Option<Vec<Item>>,
    hold: Vec<Item>,
    tables: Vec<LuckSkillTable>,
    /// `(id, result, remaining)`; remaining -1 is unlimited.
    minimum: Vec<(i32, i64, i64)>,
    minimum_counter: i32,
    last_consumed: FxHashMap<i32, i32>,
}

impl LotteryMachine {
    fn from_master(master: &Master) -> Result<LotteryMachine, Error> {
        let (mut good, mut great, mut perfect, mut hold) = (None, None, None, Vec::new());
        for r in master.gekisou_luck_base_points.iter().filter(|r| r.weight > 0) {
            let it = (r.weight, r.base_point);
            if r.note_category == 0 {
                match r.note_simulate_judgement {
                    3 => good.get_or_insert_with(Vec::new).push(it),
                    4 => great.get_or_insert_with(Vec::new).push(it),
                    5 => perfect.get_or_insert_with(Vec::new).push(it),
                    _ => {}
                }
            } else if r.note_category == 1 && r.note_simulate_judgement == J_PERFECT as i64 {
                hold.push(it);
            }
        }
        let mut bonus: Vec<Vec<Item>> = vec![Vec::new(); 5];
        for r in master.gekisou_luck_bonus_lots.iter().filter(|r| r.weight > 0) {
            if (0..5).contains(&r.chance_lot_type) {
                bonus[r.chance_lot_type as usize].push((r.weight, r.lot_result));
            }
        }
        let tables = bonus.into_iter().map(LuckSkillTable::new).collect::<Result<Vec<_>, _>>()?;
        Ok(LotteryMachine {
            good,
            great,
            perfect,
            hold,
            tables,
            minimum: Vec::new(),
            minimum_counter: 0,
            last_consumed: FxHashMap::default(),
        })
    }

    fn set_weight_buff(&mut self, percent: i32) {
        let mul = percent as f32 / 100f32;
        for t in self.tables.iter_mut() {
            t.buff = mul;
        }
    }

    fn max_minimum_result(&self) -> i64 {
        let mut m = 0i64;
        for &(_, res, _) in &self.minimum {
            if m <= res {
                m = res;
            }
        }
        m
    }

    fn consume_minimum(&mut self, applied: i64, t: i32) {
        let last = &mut self.last_consumed;
        self.minimum.retain_mut(|e| {
            if e.1 > applied || e.2 < 0 {
                return true;
            }
            e.2 = (e.2 as i32).wrapping_sub(1) as i64;
            if e.2 != 0 {
                return true;
            }
            last.insert(e.0, t);
            false
        });
    }

    pub(crate) fn enable_minimum(&mut self, result: i64, limit: i64) -> i32 {
        self.minimum_counter = self.minimum_counter.wrapping_add(1);
        self.minimum.push((self.minimum_counter, result, if limit >= 1 { limit } else { -1 }));
        self.minimum_counter
    }

    pub(crate) fn disable_minimum(&mut self, id: i32) {
        self.minimum.retain(|e| e.0 != id);
    }

    pub(crate) fn is_minimum_active(&self, id: i32) -> bool {
        self.minimum.iter().any(|e| e.0 == id)
    }

    pub(crate) fn take_last_consumed(&mut self, id: i32) -> i32 {
        self.last_consumed.remove(&id).unwrap_or(-1)
    }

    fn luck_bonus(&mut self, lot_type: usize, t: i32, random: &mut LiveRandom) -> Result<i64, Error> {
        let r = random.next_int(LUCK);
        let applied = self.max_minimum_result();
        let table = self.tables.get(lot_type).ok_or_else(|| game("lot type out of range"))?;
        if applied < 1 {
            return table.lottery(r).ok_or_else(|| game("no luck lottery item"));
        }
        let it = table.lottery_with_minimum(r, applied).ok_or_else(|| game("no luck lottery item"))?;
        self.consume_minimum(applied, t);
        Ok(it)
    }

    fn base_point(&mut self, note_type: i32, j: i32, random: &mut LiveRandom) -> Result<i64, Error> {
        if NON_LUCK_NOTE_TYPES.contains(&note_type) {
            return Ok(0);
        }
        let u = j.wrapping_add(1) as u32;
        if u < 9 && (0x107u32 >> (u & 31)) & 1 != 0 {
            return Ok(0);
        }
        let r = random.next_int(LUCK);
        let items = if is_sub_note(note_type) {
            &self.hold
        } else if (j.wrapping_sub(5) as u32) < 2 {
            self.perfect.as_ref().ok_or_else(|| game("no Perfect base point table"))?
        } else if j == J_GREAT {
            self.great.as_ref().ok_or_else(|| game("no Great base point table"))?
        } else if j == J_GOOD {
            self.good.as_ref().ok_or_else(|| game("no Good base point table"))?
        } else {
            return Ok(0);
        };
        lottery_table(items, r)?.ok_or_else(|| game("empty base point table"))
    }
}

/// The luck state of one range.
#[derive(Clone, Debug)]
pub(crate) struct LuckScore {
    pub total_bonus_point: i32,
    pub gauge: i32,
    pub lot_count: i32,
    pub rush_combo: i32,
    /// Lottery results counted: Miss, Hit, Super Hit, Critical.
    pub results: [i32; 4],
    next: i64,
    gauge_max: i64,
    gauge_max_default: i64,
    gauge_max_rush: i64,
}

impl Default for LuckScore {
    fn default() -> LuckScore {
        LuckScore {
            total_bonus_point: 0,
            gauge: 0,
            lot_count: 0,
            rush_combo: 0,
            results: [0; 4],
            next: INVALID,
            gauge_max: 100,
            gauge_max_default: 100,
            gauge_max_rush: 50,
        }
    }
}

impl LuckScore {
    fn add_gauge(&mut self, v: i32) -> Result<(), Error> {
        let g = self.gauge.wrapping_add(v) as i64;
        let m = self.gauge_max;
        self.gauge = if m <= g {
            if m <= 0 {
                return Err(game("luck gauge maximum below 1"));
            }
            self.lot_count = self.lot_count.wrapping_add((g / m) as i32);
            (g % m) as i32
        } else {
            g as i32
        };
        Ok(())
    }

    fn change_max(&mut self, m: i64) {
        if m < 1 || self.gauge_max == m {
            return;
        }
        self.gauge_max = m;
        let g = self.gauge as i64;
        if g <= m {
            return;
        }
        self.lot_count = self.lot_count.wrapping_add((g / m) as i32);
        self.gauge = (g % m) as i32;
    }

    fn initialize(&mut self, gauge_max: i64, rush_max: i64) {
        if gauge_max > 0 && rush_max > 0 {
            self.gauge_max_default = gauge_max;
            self.gauge_max_rush = rush_max;
            self.change_max(gauge_max);
        }
    }

    fn current_lot_type(&self) -> usize {
        let rc = self.rush_combo as u32;
        if rc < 4 { LOT_TYPE_BY_RUSH[rc as usize] } else { CHANCE_LOW }
    }

    fn add_score(&mut self, r: i64) -> Result<(), Error> {
        let i = r.wrapping_sub(1);
        if (i as u32) < 3 {
            let p = usize::try_from(i).ok().and_then(|i| BONUS_POINT_BY_RESULT.get(i));
            let p = *p.ok_or_else(|| game("lottery result out of range"))?;
            self.total_bonus_point = self.total_bonus_point.wrapping_add(p);
        }
        if (MISS_R..=CRITICAL).contains(&r) {
            let c = &mut self.results[r as usize];
            *c = c.wrapping_add(1);
        }
        if (r as u32) < 3 {
            self.rush_combo = 0;
            self.change_max(self.gauge_max_default);
        } else if r == CRITICAL {
            self.rush_combo = self.rush_combo.wrapping_add(1);
            self.change_max(self.gauge_max_rush);
        }
        Ok(())
    }
}

/// Timed lot probability and gauge factors added by skills.
#[derive(Clone, Debug, Default)]
struct FactorStorage {
    lot_cmds: Vec<(i32, f32)>,
    gauge_cmds: Vec<(i32, f32)>,
    lot_ids: FxHashMap<i32, f32>,
    gauge_ids: FxHashMap<i32, f32>,
    current_id: i32,
}

impl FactorStorage {
    fn at(cmds: &[(i32, f32)], t: i32) -> f32 {
        let mut s = 0f32;
        for &(ct, d) in cmds {
            if ct <= t {
                s += d;
            }
        }
        s
    }

    fn add(&mut self, gauge: bool, t: i32, f: f32) -> i32 {
        self.current_id = self.current_id.wrapping_add(1);
        let (cmds, ids) =
            if gauge { (&mut self.gauge_cmds, &mut self.gauge_ids) } else { (&mut self.lot_cmds, &mut self.lot_ids) };
        ids.insert(self.current_id, f);
        cmds.push((t, f));
        self.current_id
    }

    fn subtract(&mut self, gauge: bool, t: i32, id: i32) -> Result<(), Error> {
        let (cmds, ids) =
            if gauge { (&mut self.gauge_cmds, &mut self.gauge_ids) } else { (&mut self.lot_cmds, &mut self.lot_ids) };
        let f = ids.remove(&id).ok_or_else(|| game("luck factor id not found"))?;
        cmds.push((t, -f));
        Ok(())
    }
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

/// Where the incremental recount of a range stopped.
#[derive(Clone, Copy, Debug, Default)]
struct Resume {
    processed: usize,
    combo: i32,
    max_combo: i32,
    just: i32,
    raw_just: i32,
    cum_base: i32,
    combo_bonus: f32,
    just_bonus: f32,
    i_cb: usize,
    i_jb: usize,
    i_add_just: usize,
    i_add_combo: usize,
}

/// A cumulative Just rule: every `unit` Just counts add `effect`, at most `max_cum` steps and `max_eff` in total.
#[derive(Clone, Copy, Debug)]
struct Rule {
    range_index: i32,
    unit: i64,
    effect: i64,
    max_cum: i64,
    max_eff: i64,
}

fn cumulative_just_bonus<'a>(rules: impl Iterator<Item = &'a Rule>, range_index: i32, x: i32) -> i32 {
    let mut total = 0i32;
    for r in rules {
        if r.range_index == range_index && r.unit > 0 {
            let mut steps = (x as i64 / r.unit) as i32 as i64;
            if !(steps <= r.max_cum || r.max_cum == 0 || r.max_cum < 0) {
                steps = r.max_cum;
            }
            let mut v = (steps as i128 * r.effect as i128) as i32 as i64;
            if r.max_eff > 0 && r.max_eff <= v {
                v = r.max_eff;
            }
            total = total.wrapping_add(v as i32);
        }
    }
    total
}

fn score_frame(ms: i32) -> i32 {
    if ms > 0 { ceil_to_i32(ms as f32 / 40f32) } else { 0 }
}

fn previous_frame_end_ms(t: i32) -> i32 {
    score_frame(t).wrapping_mul(SCORE_FRAME_MS).wrapping_sub(SCORE_FRAME_MS)
}

/// The last history index whose time is `<= max_ms` (a binary search over the first `count` entries).
fn last_judgement_index(h: &[(i32, i32, i32)], count: usize, max_ms: i32) -> i32 {
    let mut hi = (count as i32).wrapping_sub(1);
    if hi < 1 {
        return 0;
    }
    let mut lo = 0i32;
    loop {
        let mid = lo.wrapping_add(sdiv(hi.wrapping_sub(lo).wrapping_add(1), 2));
        if h[mid as usize].0 <= max_ms {
            lo = mid;
        } else {
            hi = mid.wrapping_sub(1);
        }
        if lo >= hi {
            return lo;
        }
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

    pub(crate) fn add_just_count(&mut self, time_ms: i32, count: i32) {
        self.add_just_stack.push((time_ms, count as f32, self.current_playing_range_index()));
        self.flag();
    }

    pub(crate) fn add_gekisou_combo(&mut self, time_ms: i32, count: i32) {
        self.add_combo_stack.push((time_ms, count as f32, self.current_playing_range_index()));
        self.flag();
    }

    fn apply_fixed_count(&mut self, idx: usize, delta: f32, range: i32, just: bool) {
        if range != -1 && range != idx as i32 {
            return;
        }
        let n = delta as i32;
        let rs = &mut self.states[idx];
        if just {
            rs.just = rs.just.wrapping_add(n);
            rs.cum_base = rs.cum_base.wrapping_add(n);
        } else {
            rs.combo = rs.combo.wrapping_add(n);
            rs.max_combo = rs.max_combo.max(rs.combo);
        }
    }

    pub(crate) fn add_luck_point(&mut self, p: i64) {
        let i = self.current_playing_range_index();
        if i < 0 {
            return;
        }
        let ls = &mut self.states[i as usize].luck;
        ls.total_bonus_point = ls.total_bonus_point.wrapping_add(p as i32);
    }

    pub(crate) fn add_luck_gauge_percent(&mut self, g: i32) -> Result<(), Error> {
        let i = self.current_playing_range_index();
        if i >= 0 {
            self.states[i as usize].luck.add_gauge(g)?;
        }
        Ok(())
    }

    pub(crate) fn current_gauge_max(&self) -> i64 {
        let i = self.current_playing_range_index();
        if i >= 0 { self.states[i as usize].luck.gauge_max } else { self.luck_gauge_max }
    }

    /// The chart-time spans of the rush score bonus commands `(added at, disabled at)`, `i32::MAX` while running.
    pub(crate) fn rush_log(&self) -> &[(i32, i32)] {
        &self.rush_log
    }

    /// Diagnostics only: every Gekisou combo bonus command `(time, change)` in filing order.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn combo_bonus_commands(&self) -> Vec<(i32, f32)> {
        self.cb_stack.iter().map(|&(t, d, _)| (t, d)).collect()
    }

    pub(crate) fn add_combo_bonus(&mut self, t: i32, bonus: f32) -> i32 {
        let i = self.next_id();
        self.combo_bonus_ids.insert(i, bonus);
        self.cb_stack.push((t, bonus, self.seq.wrapping_sub(1)));
        self.flag();
        i
    }

    pub(crate) fn subtract_combo_bonus(&mut self, t: i32, id: i32) -> Result<(), Error> {
        let v = self.combo_bonus_ids.remove(&id).ok_or_else(|| game("combo bonus id not found"))?;
        self.cb_stack.push((t, -v, self.seq.wrapping_sub(1)));
        self.flag();
        Ok(())
    }

    pub(crate) fn add_just_bonus(&mut self, t: i32, bonus: f32) -> i32 {
        let i = self.next_id();
        self.just_bonus_ids.insert(i, bonus);
        self.jb_stack.push((t, bonus, self.seq.wrapping_sub(1)));
        self.flag();
        i
    }

    pub(crate) fn subtract_just_bonus(&mut self, t: i32, id: i32) -> Result<(), Error> {
        let v = self.just_bonus_ids.remove(&id).ok_or_else(|| game("Just bonus id not found"))?;
        self.jb_stack.push((t, -v, self.seq.wrapping_sub(1)));
        self.flag();
        Ok(())
    }

    pub(crate) fn add_cumulative_rule(&mut self, unit: i64, effect: i64, max_cum: i64, max_eff: i64) -> i32 {
        let i = self.next_id();
        let range_index = self.current_playing_range_index();
        self.rules.insert(i, Rule { range_index, unit, effect, max_cum, max_eff });
        self.flag();
        i
    }

    pub(crate) fn remove_cumulative_rule(&mut self, id: i32) {
        if self.rules.remove(&id).is_some() {
            self.flag();
        }
    }

    pub(crate) fn enable_combo_protect(&mut self, t: i32, limit: i64, mask: i32) -> i32 {
        let i = self.next_id();
        if limit < 1 {
            self.pr_stack.push((t, 1, i, mask & 0xFF));
        } else {
            self.limited_ids.insert(i);
            self.pr_stack.push((t, (limit as i32).wrapping_add(1), i, mask & 0xFF));
        }
        self.flag();
        i
    }

    pub(crate) fn disable_combo_protect(&mut self, t: i32, id: i32) {
        let d = if self.limited_ids.remove(&id) { -2 } else { -1 };
        self.pr_stack.push((t, d, id, 0));
        self.flag();
    }

    pub(crate) fn add_lot_probability_up(&mut self, t: i32, f: f32) -> i32 {
        self.storage.add(false, t, f)
    }

    pub(crate) fn add_gauge_up(&mut self, t: i32, f: f32) -> i32 {
        self.storage.add(true, t, f)
    }

    pub(crate) fn subtract_lot_probability_up(&mut self, t: i32, id: i32) -> Result<(), Error> {
        self.storage.subtract(false, t, id)
    }

    pub(crate) fn subtract_gauge_up(&mut self, t: i32, id: i32) -> Result<(), Error> {
        self.storage.subtract(true, t, id)
    }

    // --- counts ---------------------------------------------------------------------------------------------

    /// Recounts the combo and Just counts of every range that is not past its end delay from its history.
    fn recalculate(&mut self) {
        for idx in 0..self.states.len() {
            if self.states[idx].state >= S_DELAY || self.history[idx].is_empty() {
                continue;
            }
            let rr = self.resume[idx];
            let (mut cb, mut jb, mut i_cb, mut i_jb, mut i_pr, mut k);
            let (mut i_add_just, mut i_add_combo);
            let hlen = self.history[idx].len();
            if !self.full_recalc && self.pr_stack.is_empty() && 1 <= rr.processed && rr.processed <= hlen {
                let rs = &mut self.states[idx];
                (rs.combo, rs.max_combo, rs.just, rs.raw_just, rs.cum_base) =
                    (rr.combo, rr.max_combo, rr.just, rr.raw_just, rr.cum_base);
                (cb, jb, i_cb, i_jb, i_pr, k) = (rr.combo_bonus, rr.just_bonus, rr.i_cb, rr.i_jb, 0usize, rr.processed);
                (i_add_just, i_add_combo) = (rr.i_add_just, rr.i_add_combo);
            } else {
                let rs = &mut self.states[idx];
                (rs.combo, rs.max_combo, rs.just, rs.raw_just, rs.cum_base) = (0, 0, 0, 0, 0);
                self.snapshot[idx].clear();
                self.unlimited.clear();
                self.limited.clear();
                (cb, jb, i_cb, i_jb, i_pr, k) = (1f32, 1f32, 0, 0, 0, 0);
                (i_add_just, i_add_combo) = (0, 0);
            }
            while k < hlen {
                let (et, j, es) = self.history[idx][k];
                while i_cb < self.cb_stack.len() {
                    let (ct, d, cs) = self.cb_stack[i_cb];
                    if et <= ct && (et < ct || es < cs) {
                        break;
                    }
                    cb += d;
                    i_cb += 1;
                }
                while i_jb < self.jb_stack.len() {
                    let (ct, d, cs) = self.jb_stack[i_jb];
                    if et <= ct && (et < ct || es < cs) {
                        break;
                    }
                    jb += d;
                    i_jb += 1;
                }
                while i_pr < self.pr_stack.len() && et >= self.pr_stack[i_pr].0 {
                    let (_, d, pid, mask) = self.pr_stack[i_pr];
                    if d < 2 {
                        if d == 1 {
                            self.unlimited.push((pid, mask));
                        } else if d < -1 {
                            if let Some(p) = self.limited.iter().position(|x| x.0 == pid) {
                                self.limited.remove(p);
                            }
                        } else if let Some(p) = self.unlimited.iter().position(|x| x.0 == pid) {
                            self.unlimited.remove(p);
                        }
                    } else {
                        self.limited.push((pid, d.wrapping_sub(1), mask));
                    }
                    i_pr += 1;
                }
                while i_add_just < self.add_just_stack.len() && self.add_just_stack[i_add_just].0 <= et {
                    let (_, d, range) = self.add_just_stack[i_add_just];
                    self.apply_fixed_count(idx, d, range, true);
                    i_add_just += 1;
                }
                while i_add_combo < self.add_combo_stack.len() && self.add_combo_stack[i_add_combo].0 <= et {
                    let (_, d, range) = self.add_combo_stack[i_add_combo];
                    self.apply_fixed_count(idx, d, range, false);
                    i_add_combo += 1;
                }
                self.entry(idx, j, floor_to_i32(cb), floor_to_i32(jb));
                let c = self.states[idx].combo;
                self.snapshot[idx].push(c);
                k += 1;
            }
            let rs = &self.states[idx];
            self.resume[idx] = Resume {
                processed: hlen,
                combo: rs.combo,
                max_combo: rs.max_combo,
                just: rs.just,
                raw_just: rs.raw_just,
                cum_base: rs.cum_base,
                combo_bonus: cb,
                just_bonus: jb,
                i_cb,
                i_jb,
                i_add_just,
                i_add_combo,
            };
            // Native saves resume BEFORE trailing additions. On the next recount these are replayed once.
            for i in i_add_just..self.add_just_stack.len() {
                let (_, d, range) = self.add_just_stack[i];
                self.apply_fixed_count(idx, d, range, true);
            }
            for i in i_add_combo..self.add_combo_stack.len() {
                let (_, d, range) = self.add_combo_stack[i];
                self.apply_fixed_count(idx, d, range, false);
            }
        }
        self.full_recalc = false;
    }

    /// One history entry of the recount.
    fn entry(&mut self, idx: usize, j: i32, i_combo: i32, i_just: i32) {
        let rs = &mut self.states[idx];
        if (j.wrapping_sub(3) as u32) < 0xFFFF_FFFE {
            if (j.wrapping_sub(7) as u32) > 0xFFFF_FFFB {
                rs.combo = rs.combo.wrapping_add(i_combo);
                rs.max_combo = rs.max_combo.max(rs.combo);
            } else {
                return;
            }
        } else {
            let bit = 1i32 << (j as u32 & 31);
            let eff = |m: i32| if m != 0 { m } else { 6 };
            let mut prot = self.unlimited.iter().any(|&(_, m)| eff(m) & bit != 0);
            let mut i = self.limited.len();
            while i > 0 {
                i -= 1;
                let (pid, rem, m) = self.limited[i];
                if eff(m) & bit != 0 {
                    let rem = rem.wrapping_sub(1);
                    if rem < 1 {
                        self.limited.remove(i);
                    } else {
                        self.limited[i] = (pid, rem, m);
                    }
                    prot = true;
                }
            }
            if !prot {
                rs.max_combo = rs.max_combo.max(rs.combo);
                rs.combo = 0;
            }
        }
        if j == J_JUST {
            rs.raw_just = rs.raw_just.wrapping_add(1);
            rs.cum_base = rs.cum_base.wrapping_add(i_just);
            let c = cumulative_just_bonus(self.rules.values(), idx as i32, rs.cum_base);
            rs.just = c.wrapping_add(i_just).wrapping_add(rs.just);
        }
    }

    fn timing_combo_in_range(&self, idx: usize, t: i32) -> i32 {
        let (h, sn) = (&self.history[idx], &self.snapshot[idx]);
        let n = h.len().min(sn.len());
        if n == 0 {
            return 0;
        }
        let prev_end = previous_frame_end_ms(t);
        if prev_end < h[0].0 {
            return 0;
        }
        sn[last_judgement_index(h, n, prev_end) as usize]
    }

    // --- luck -----------------------------------------------------------------------------------------------

    fn update_luck(&mut self, idx: usize, nt: i32, tn: i32, nid: i32, j: i32, env: &mut Env) -> Result<(), Error> {
        let buff = floor_to_i32(FactorStorage::at(&self.storage.lot_cmds, tn) * 100f32);
        self.machine.set_weight_buff(buff);
        let gup = FactorStorage::at(&self.storage.gauge_cmds, tn);
        let base = self.machine.base_point(nt, j, env.random)?;
        let add = floor_to_i32((gup + 1f32) * base as f32);
        self.states[idx].luck.add_gauge(add)?;
        self.consume_lot(idx, tn, nid, env)
    }

    fn disable_rush(&mut self, t: i32, env: &mut Env) -> Result<(), Error> {
        if self.rush_id != 0 {
            env.handle.disable(t, self.rush_id)?;
            self.rush_id = 0;
            if let Some(open) = self.rush_log.iter_mut().rev().find(|x| x.1 == i32::MAX) {
                open.1 = t;
            }
        }
        Ok(())
    }

    fn consume_lot(&mut self, idx: usize, t: i32, _note_id: i32, env: &mut Env) -> Result<(), Error> {
        if self.states[idx].state > S_END || self.states[idx].luck.lot_count < 1 {
            return Ok(());
        }
        self.states[idx].luck.lot_count -= 1;
        if self.states[idx].luck.next == INVALID {
            self.states[idx].luck.next = self.machine.luck_bonus(NONE_LOT, t, env.random)?;
        }
        let (next, rush) = (self.states[idx].luck.next, self.states[idx].luck.rush_combo);
        if rush == 0 {
            if next == CRITICAL {
                self.rush_id = env.handle.add(t, self.rush_percent as i32);
                self.rush_log.push((t, i32::MAX));
            } else {
                self.disable_rush(t, env)?;
            }
        } else if next != CRITICAL {
            self.disable_rush(t, env)?;
        }
        self.states[idx].luck.add_score(next)?;
        if next != INVALID {
            self.lot_result = next;
            self.lot_results.push(next);
        }
        let lot_type = self.states[idx].luck.current_lot_type();
        self.states[idx].luck.next = self.machine.luck_bonus(lot_type, t, env.random)?;
        Ok(())
    }

    fn pending_lots(&mut self, t: i32, env: &mut Env) -> Result<(), Error> {
        for pi in 0..self.playing.len() {
            let idx = self.playing[pi];
            let rs = &self.states[idx];
            if self.ranges[idx].mission == M_LUCK
                && rs.state == S_PLAYING
                && rs.luck.lot_count != 0
                && self.lot_result == INVALID
            {
                let buff = floor_to_i32(FactorStorage::at(&self.storage.lot_cmds, t) * 100f32);
                self.machine.set_weight_buff(buff);
                self.consume_lot(idx, t, -1, env)?;
            }
        }
        Ok(())
    }

    fn judge(&mut self, idx: usize, nt: i32, tn: i32, nid: i32, j: i32, env: &mut Env) -> Result<(), Error> {
        if j == J_PASS || j == J_WAIT {
            return Ok(());
        }
        self.history[idx].push((tn, j, self.seq));
        self.seq = self.seq.wrapping_add(1);
        self.needs_recalc = true;
        if (j.wrapping_sub(3) as u32) < 4 {
            self.states[idx].last_combo_ms = tn;
        }
        if j == J_JUST {
            self.states[idx].last_just_ms = tn;
        }
        if self.ranges[idx].mission == M_LUCK {
            if self.states[idx].state < S_START {
                self.pending[idx].push((nt, tn, nid, j));
            } else {
                self.update_luck(idx, nt, tn, nid, j, env)?;
            }
        }
        Ok(())
    }

    /// The controller's update after the frame's skills and score: judged notes of the ranges, the recount, the
    /// fever-end scores and the pending lots.
    pub(crate) fn update(
        &mut self,
        t: i32,
        judged: &[GkNote],
        fever_updates: &[(usize, u8)],
        current_score: i32,
        env: &mut Env,
    ) -> Result<(), Error> {
        for idx in 0..self.states.len() {
            if self.states[idx].state > S_STANDBY && !self.pending[idx].is_empty() {
                let pending = std::mem::take(&mut self.pending[idx]);
                for (nt, tn, nid, j) in pending {
                    self.update_luck(idx, nt, tn, nid, j, env)?;
                }
            }
        }
        for idx in 0..self.states.len() {
            for &(nid, nt, tn, j) in judged {
                if self.ranges[idx].targets.contains(&nid) {
                    self.judge(idx, nt, tn, nid, j, env)?;
                }
            }
        }
        if self.needs_recalc {
            self.recalculate();
            self.needs_recalc = false;
        }
        for &(idx, fs) in fever_updates {
            let rs = self.states.get_mut(idx).ok_or_else(|| game(FEVER_WITHOUT_RANGE))?;
            if fs == FEVER_END {
                rs.end_score = current_score;
            }
        }
        self.pending_lots(t, env)
    }

    fn state_update(&mut self, idx: usize) {
        if !self.state_updates.contains(&idx) {
            self.state_updates.push(idx);
        }
    }

    fn elapsed_ms(rs: &RangeState) -> i64 {
        let v = rs.sw_elapsed * 1000f32;
        if v == f32::INFINITY { i64::MIN } else { v as i64 }
    }

    /// The controller's update before the frame: range state machine and stopwatches.
    pub(crate) fn before_update(
        &mut self,
        dt: f32,
        t: i32,
        fever_updates: &[(usize, u8)],
        current_score: i32,
        env: &mut Env,
    ) -> Result<(), Error> {
        self.state_updates.clear();
        self.lot_result = INVALID;
        self.lot_results.clear();
        self.current_playing_index = -1;
        for (rs, r) in self.states.iter_mut().zip(&self.ranges) {
            rs.time_to = r.start_ms.wrapping_sub(t);
        }
        let mut remove = Vec::new();
        for pi in 0..self.playing.len() {
            let idx = self.playing[pi];
            let st = self.states[idx].state;
            if st < S_END {
                if st == S_START {
                    self.states[idx].state = S_PLAYING;
                    self.state_update(idx);
                    self.current_playing_index = idx as i32;
                } else if st == S_PLAYING {
                    self.current_playing_index = idx as i32;
                }
            } else if st == S_END {
                self.current_playing_index = idx as i32;
                let rs = &mut self.states[idx];
                if !rs.sw_stopped {
                    rs.sw_elapsed += dt;
                }
                if self.last_note_delay_ms <= Self::elapsed_ms(rs) {
                    rs.sw_elapsed = 0f32;
                    rs.sw_stopped = false;
                    rs.state = S_DELAY;
                    self.state_update(idx);
                }
            } else if st == S_DELAY {
                let rs = &mut self.states[idx];
                if !rs.sw_stopped {
                    rs.sw_elapsed += dt;
                }
                if COMPLETE_DELAY_MS <= Self::elapsed_ms(rs) {
                    rs.state = S_COMPLETE;
                    rs.sw_stopped = true;
                    self.state_update(idx);
                }
            } else if st == S_COMPLETE {
                self.states[idx].state = S_FINISH;
                self.state_update(idx);
                remove.push(idx);
                self.disable_rush(t, env)?;
            }
        }
        for idx in remove {
            if let Some(p) = self.playing.iter().position(|&x| x == idx) {
                self.playing.remove(p);
            }
        }
        for &(idx, fs) in fever_updates {
            let rs = self.states.get_mut(idx).ok_or_else(|| game(FEVER_WITHOUT_RANGE))?;
            if fs == FEVER_END {
                rs.state = S_END;
                rs.sw_elapsed = 0f32;
                rs.sw_stopped = false;
                self.state_update(idx);
            } else if fs == FEVER_FEVER {
                rs.state = S_START;
                rs.start_score = current_score;
                self.playing.push(idx);
                self.state_update(idx);
                self.current_playing_index = idx as i32;
            }
        }
        for idx in 0..self.states.len() {
            if self.states[idx].state == S_WAIT && self.states[idx].time_to < STANDBY_MS {
                self.states[idx].state = S_STANDBY;
                self.state_update(idx);
            }
        }
        Ok(())
    }
}

impl GekisouComboInfo for Controller {
    fn gekisou_combo(&self, time_ms: i32) -> Option<i32> {
        let i =
            self.ranges.iter().position(|r| r.mission == M_COMBO && r.start_ms <= time_ms && time_ms <= r.end_ms)?;
        Some(self.timing_combo_in_range(i, time_ms))
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

/// The song's mission pattern: 0 when a mission is missing, 1 all the same, 2 all different, 3 otherwise.
pub(crate) fn mission_pattern(t0: i64, t1: i64, t2: i64) -> i64 {
    if t0 == 0 || t1 == 0 || t2 == 0 {
        return 0;
    }
    if t0 == t1 {
        return if t0 == t2 { 1 } else { 3 };
    }
    if t1 != t2 && t0 != t2 {
        return 2;
    }
    3
}

/// Rank bonus percentages `[range][rank - 1]` of a mission pattern.
pub(crate) fn ranking_factors(master: &Master, pattern: i64) -> Result<[[i64; 5]; 3], Error> {
    let mut f = [[0i64; 5]; 3];
    for r in &master.gekisou_ranking_score_bonuses {
        let (c, k) = (r.count.wrapping_sub(1), r.rank.wrapping_sub(1));
        if r.mission_pattern == pattern && (c as u32) < 3 && (k as u32) <= 4 {
            let cell = usize::try_from(c)
                .ok()
                .zip(usize::try_from(k).ok())
                .and_then(|(c, k)| f.get_mut(c).and_then(|row| row.get_mut(k)))
                .ok_or_else(|| game("rank bonus row out of range"))?;
            *cell = r.score_bonus_percent;
        }
    }
    Ok(f)
}

/// The solo rank bonus of a completed range: `(rank, bonus, percent)`; a solo player is rank 1.
pub(crate) fn solo_rank_bonus(range_index: usize, range_score: i32, factors: &[[i64; 5]; 3]) -> (i32, i32, i64) {
    let rank = 1;
    if range_index >= factors.len() {
        return (rank, 0, 0);
    }
    let pct = factors[range_index][0];
    let p = range_score as i128 * pct as i128;
    let q = p.abs() / 100;
    (rank, (if p >= 0 { q } else { -q }) as i32, pct)
}

impl Controller {
    /// The score of a range (end score minus start score).
    pub(crate) fn range_score(&self, idx: usize) -> i32 {
        self.states[idx].score()
    }
}

#[cfg(test)]
mod fixed_addition_tests {
    use super::*;
    fn controller() -> Controller {
        Controller::new(vec![(0, 1000, 1), (1100, 2000, 3)], std::iter::empty(), &Master::default(), 100, 100, 0, 0)
            .unwrap()
    }

    #[test]
    fn fixed_additions_capture_range_and_feed_inflated_not_raw_just() {
        let mut c = controller();
        c.playing.push(0);
        c.history[0] = vec![(100, J_JUST, 0), (200, J_JUST, 1)];
        c.history[1] = vec![(1200, J_JUST, 2)];
        c.add_just_count(150, 5);
        c.add_gekisou_combo(150, 7);
        c.recalculate();
        assert_eq!((c.states[0].just, c.states[0].raw_just, c.states[0].cum_base, c.states[0].combo), (7, 2, 7, 9));
        assert_eq!((c.states[1].just, c.states[1].combo), (1, 1));
        assert_eq!(c.snapshot[0], [1, 9]);
    }

    #[test]
    fn trailing_commands_are_visible_but_not_repeated_in_resume_or_score_snapshot() {
        let mut c = controller();
        c.playing.push(0);
        c.history[0] = vec![(100, J_JUST, 0)];
        c.add_just_count(150, 4);
        c.add_gekisou_combo(150, 6);
        c.recalculate();
        assert_eq!((c.states[0].just, c.states[0].combo), (5, 7));
        assert_eq!(c.snapshot[0], [1]);
        c.recalculate();
        assert_eq!((c.states[0].just, c.states[0].combo), (5, 7));
        c.history[0].push((200, J_JUST, 1));
        c.recalculate();
        assert_eq!((c.states[0].just, c.states[0].combo), (6, 8));
        assert_eq!(c.snapshot[0], [1, 8]);
    }

    #[test]
    fn outside_range_is_wildcard_and_empty_history_ignores_command_until_note_exists() {
        let mut c = controller();
        c.add_just_count(0, 3);
        c.add_gekisou_combo(0, 4);
        c.recalculate();
        assert_eq!(c.states[0].just, 0);
        c.history[0].push((100, J_JUST, 0));
        c.history[1].push((1200, J_JUST, 1));
        c.recalculate();
        for s in &c.states {
            assert_eq!((s.just, s.combo), (4, 5));
        }
    }

    #[test]
    fn fixed_command_uses_binary32_delta_and_cumulative_base_without_changing_last_just() {
        let mut c = controller();
        c.playing.push(0);
        c.history[0].push((100, J_JUST, 0));
        c.add_just_count(100, 16_777_217);
        c.recalculate();
        assert_eq!(c.states[0].just, 16_777_217); // f32 command is 16_777_216, then the note adds one
        assert_eq!(c.states[0].raw_just, 1);
        assert_eq!(c.states[0].last_just_ms, -1);
    }
}

/// Player-indexed network input. The calculator reads the mission from the song, not this row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkGekisouResult {
    pub range_index: i32,
    pub combo: i32,
    pub luck_total_point: i32,
    pub just_count: i32,
    pub perfect_count: i32,
    pub score: i32,
    pub has_no_input: bool,
}

/// The native network calculator's failures, distinct from a dynamic controller's range capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrokenGekisouRanking {
    NoValidData,
    RangeOutOfBounds(i32),
    InvalidMission(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkGekisouRanking {
    pub groups: Vec<Vec<usize>>,
    pub broken: Option<BrokenGekisouRanking>,
    /// Confirmed ranks use competition ranking, unlike GetPlayerRank's group ordinals.
    pub confirmed_ranks: Vec<i32>,
}

impl NetworkGekisouRanking {
    pub fn player_rank(&self, player: usize) -> i32 {
        if self.broken.is_some() {
            return 1;
        }
        self.groups.iter().position(|g| g.contains(&player)).map_or(0, |i| i as i32 + 1)
    }
    pub fn confirmed_rank(&self, player: usize) -> i32 {
        self.confirmed_ranks.get(player).copied().unwrap_or(5)
    }
}

/// A no-input flag alone is not sufficient; skill-added counts can make a player eligible.
pub fn gekisou_no_input(result: Option<&NetworkGekisouResult>) -> bool {
    result.is_none_or(|r| r.has_no_input && r.just_count == 0 && r.combo == 0 && r.luck_total_point == 0)
}

fn compare_network_result(
    mission: i64,
    a: Option<&NetworkGekisouResult>,
    b: Option<&NetworkGekisouResult>,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let point = |r: Option<&NetworkGekisouResult>| {
        r.map_or(0, |r| match mission {
            1 => r.combo,
            2 => r.luck_total_point,
            _ => r.just_count,
        })
    };
    // The primary comparison precedes null handling; a negative primary value sorts AFTER a missing player.
    let primary = point(b).cmp(&point(a));
    if primary != Ordering::Equal {
        return primary;
    }
    match (a, b) {
        (Some(a), Some(b)) => b.score.cmp(&a.score).then_with(|| b.perfect_count.cmp(&a.perfect_count)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Uses the explicit configured player count and player-indexed results.
/// The first non-null row supplies the range index. Other rows are not silently re-keyed by token/index.
/// A missing song mission is an input error (native list-index exception), not a Broken ranking.
/// Tied players' order within a group is not part of this API's contract.
pub fn calculate_network_gekisou_ranking(
    missions: &[i64],
    player_count: usize,
    results: &[Option<NetworkGekisouResult>],
) -> Result<NetworkGekisouRanking, Error> {
    let broken =
        |reason| NetworkGekisouRanking { groups: Vec::new(), broken: Some(reason), confirmed_ranks: vec![1; 5] };
    let Some(first) = results.iter().flatten().next() else {
        return Ok(broken(BrokenGekisouRanking::NoValidData));
    };
    if !(0..3).contains(&first.range_index) {
        return Ok(broken(BrokenGekisouRanking::RangeOutOfBounds(first.range_index)));
    }
    let mission = *missions
        .get(first.range_index as usize)
        .ok_or_else(|| Error::Input("network ranking mission index out of range".into()))?;
    if !(1..=3).contains(&mission) {
        return Ok(broken(BrokenGekisouRanking::InvalidMission(mission)));
    }
    if player_count == 0 {
        return Err(Error::Input("native network ranking requires a non-empty player array".into()));
    }
    let row = |i: usize| results.get(i).and_then(Option::as_ref);
    let mut sorted: Vec<usize> = (0..player_count).collect();
    sorted.sort_by(|&a, &b| compare_network_result(mission, row(a), row(b)));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in sorted {
        if groups.last().is_some_and(|g| compare_network_result(mission, row(g[0]), row(i)).is_eq()) {
            groups.last_mut().unwrap().push(i);
        } else {
            groups.push(vec![i]);
        }
    }
    let mut confirmed_ranks = vec![5; results.len().max(player_count)];
    // The constructor selects its filtering branch by scanning the result list, not the configured player slots.
    let filter = results.iter().any(|r| gekisou_no_input(r.as_ref()));
    let mut next = 1i32;
    for group in &groups {
        let eligible: Vec<_> = group.iter().copied().filter(|&i| !filter || !gekisou_no_input(row(i))).collect();
        for &i in &eligible {
            confirmed_ranks[i] = next;
        }
        next = next.wrapping_add(eligible.len() as i32);
    }
    Ok(NetworkGekisouRanking { groups, broken: None, confirmed_ranks })
}

#[cfg(test)]
mod network_ranking_tests {
    use super::*;
    fn r(combo: i32, score: i32, perfect: i32) -> Option<NetworkGekisouResult> {
        Some(NetworkGekisouResult { combo, score, perfect_count: perfect, ..Default::default() })
    }
    #[test]
    fn group_rank_and_confirmed_rank_are_different() {
        let n = calculate_network_gekisou_ranking(&[1, 2, 3], 3, &[r(10, 5, 3), r(10, 5, 3), r(8, 99, 99)]).unwrap();
        assert_eq!(n.groups, [vec![0, 1], vec![2]]);
        assert_eq!(n.player_rank(2), 2);
        assert_eq!(n.confirmed_rank(2), 3);
        assert_eq!(n.confirmed_rank(20), 5);
    }
    #[test]
    fn null_comparison_follows_primary_and_noinput_needs_zero_counts() {
        let mut added = r(1, 0, 0).unwrap();
        added.has_no_input = true;
        assert!(!gekisou_no_input(Some(&added)));
        let n = calculate_network_gekisou_ranking(&[1], 4, &[r(-1, 90, 90), None, Some(added), r(0, 0, 0)]).unwrap();
        assert_eq!(n.groups, [vec![2], vec![3], vec![1], vec![0]]);
        assert_eq!(n.confirmed_ranks, [3, 5, 1, 2]);
    }
    #[test]
    fn score_precedes_perfect_and_broken_is_not_controller_limit() {
        let n = calculate_network_gekisou_ranking(&[1], 3, &[r(1, 3, 9), r(1, 4, 0), r(1, 3, 10)]).unwrap();
        assert_eq!(n.groups, [vec![1], vec![2], vec![0]]);
        let mut fourth = r(1, 0, 0).unwrap();
        fourth.range_index = 3;
        let n = calculate_network_gekisou_ranking(&[1; 4], 1, &[Some(fourth)]).unwrap();
        assert_eq!(n.broken, Some(BrokenGekisouRanking::RangeOutOfBounds(3)));
        assert_eq!(n.confirmed_rank(4), 1);
        assert_eq!(n.confirmed_rank(5), 5);
        assert_eq!(
            calculate_network_gekisou_ranking(&[4], 1, &[r(1, 0, 0)]).unwrap().broken,
            Some(BrokenGekisouRanking::InvalidMission(4))
        );
    }
}
