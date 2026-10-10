//! Exact binary32 factor histories of the lottery paths through the recorder's command and query schedule.
//!
//! The native calculator keeps one binary32 value per factor field. Executing a score frame applies the frame's
//! factor commands in (chart time, owner, filing) order, scores its notes in between, and records the binary32 sum
//! of the commands it applied; undoing the frame subtracts that sum. A score query undoes from the last executed
//! frame down to the earliest frame that received a filing since the previous query, then executes up to the
//! query's frame.
//!
//! Lottery paths differ in two ways: the direct score probes file their signed score-up pairs at different skill
//! boundaries, and Rush filings can move the rewind target of a query. Native files a Rush command exactly when the
//! Rush handle flips, so each path keeps its handle: a range FINISH files where the path's Rush runs, a play
//! frame's lotteries change it along the transitions the certified lottery law admits for it, and where the law
//! ties the probe class to the Rush the path's switches follow its handle. Otherwise a path switches along any
//! probe transition the law admits at that boundary. Where the schedule leaves a path's history open (which of a
//! play frame's lottery places files its Rush command, and the owner-local order of a switch among ordinary
//! commands of the same owner and time), every alternative is followed, so the paths cover every lottery history
//! with positive probability.
//!
//! Each [`Path`] is a group of histories with the same schedule ahead: the same probe class and Rush, the same start
//! class and recorded sum of every frame a later query may still undo, and the same probe switches filed. A recorded sum
//! starts from zero, so it does not depend on the state. From here on every history of the group therefore applies
//! the same sequence of binary32 additions of constants to each field, and rounded addition of a constant is
//! monotone: the least and the greatest state of the group, which are states of member histories, stay the least
//! and the greatest under every such step. A group keeps exactly these two states per field.
//!
//! A note keeps, per probe class and combo inputs, the least and the greatest factor state of its last execution
//! over all histories: a frame every history executes replaces them, a frame only some histories execute widens
//! them.
//!
//! One replay follows several orders of a formation at once when their schedules differ only in factor commands
//! that belong to some of the orders, and the orders share the lottery law and the probe rows. A group then covers a
//! set of orders and keeps its states and recorded sums once per order; its other key parts are common to them. A
//! query rewinds every order to its own target, so the orders of a group may part there. Groups with the same common
//! key share one entry as long as the records of the orders both cover agree; such an order's histories are then
//! merged exactly as a replay of that order alone merges them.

use super::{BoundsEvent, FIELDS, ProbeRow};
use crate::error::Error;
use crate::live::full::luck_dp::RUSH_FINISH;
use crate::live::skill::FactorCommand;
use crate::num::FxHashMap;
use std::ops::Range;

const MAX_PATHS: usize = 4096;
const MAX_MERGES: usize = 4096;
const MAX_NOTE_VALUES: usize = 4096;
/// The most orders one replay follows.
pub(super) const MAX_LANES: usize = 128;

type State = [f32; FIELDS];
type Bits = [u32; FIELDS];
/// A set of orders: bit k is order k.
pub(super) type Orders = u128;

/// The orders of a set, ascending.
pub(super) fn each(mut orders: Orders) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        (orders != 0).then(|| {
            let order = orders.trailing_zeros() as usize;
            orders &= orders - 1;
            order
        })
    })
}

/// The set of orders `0..count`.
pub(super) fn first_orders(count: usize) -> Orders {
    if count >= MAX_LANES { Orders::MAX } else { (1 << count) - 1 }
}

fn refuse(why: &str) -> Error {
    Error::Unsupported(format!("native LUCK exact score: {why}"))
}

fn capacity() -> Error {
    Error::Capacity("native LUCK exact score: the lottery path set exceeds its complete-work budget".into())
}

/// An ordinary factor command: the changes native apply adds, in field order. The combo and note fields change
/// only for a nonzero value; a judgement field (`always`) is added even when its value is zero.
#[derive(Clone, Copy, Debug)]
struct Op {
    time: i32,
    owner: i32,
    deltas: [f32; FIELDS],
    always: u8,
    /// The orders whose schedule holds the command.
    orders: Orders,
}

impl Op {
    fn of(command: &FactorCommand, orders: Orders) -> Self {
        let mill = |m: i32| if m != 0 { m as f32 / 100000f32 } else { 0f32 };
        let mut deltas = [0f32; FIELDS];
        deltas[0] = mill(command.combo_mill);
        deltas[1] = mill(command.note_mill);
        let mut always = 0;
        if (3..=6).contains(&command.judgement) {
            let field = 8 - command.judgement as usize;
            deltas[field] = mill(command.judge_mill);
            always = 1 << field;
        }
        Self { time: command.time_ms, owner: command.owner_id, deltas, always, orders }
    }

    fn changes(&self) -> bool {
        self.always != 0 || self.deltas.iter().any(|&delta| delta != 0.0)
    }

    fn apply(&self, sub: &mut Sub, orders: Orders) {
        for order in each(orders & self.orders) {
            for field in 0..FIELDS {
                let delta = self.deltas[field];
                if self.always >> field & 1 != 0 || delta != 0.0 {
                    sub.low[order][field] += delta;
                    sub.high[order][field] += delta;
                    sub.sum[order][field] += delta;
                }
            }
        }
    }
}

fn apply_probe(value: f32, sub: &mut Sub, orders: Orders) {
    if value != 0.0 {
        for order in each(orders) {
            sub.low[order][1] += value;
            sub.high[order][1] += value;
            sub.sum[order][1] += value;
        }
    }
}

/// Widens `low..=high` of one order to cover `other_low..=other_high`, field by field.
fn widen(low: &mut State, high: &mut State, other_low: &State, other_high: &State) {
    for field in 0..FIELDS {
        if other_low[field] < low[field] {
            low[field] = other_low[field];
        }
        if other_high[field] > high[field] {
            high[field] = other_high[field];
        }
    }
}

fn finite(states: &[State], orders: Orders) -> Result<(), Error> {
    if each(orders).all(|order| states[order].iter().all(|value| value.is_finite())) {
        Ok(())
    } else {
        Err(refuse("a factor state may leave the finite binary32 range"))
    }
}

/// One element of a probe switch: an ordinary command of the switch's time or a probe row, by index.
#[derive(Clone, Copy)]
enum Step {
    Op(usize),
    Row(usize),
}

#[derive(Clone, Default)]
struct Frame {
    /// Ordinary commands by (time, owner), equal keys in filing order (the native stable sort).
    ops: Vec<Op>,
    /// Notes by (time, note id).
    notes: Vec<usize>,
}

/// A group of native histories with the same schedule ahead in each of its orders; see the module documentation.
#[derive(Clone)]
struct Path {
    orders: Orders,
    /// Probe class of the executed state.
    class: u8,
    /// Per order and field, the least and the greatest state of the group's histories.
    low: Vec<State>,
    high: Vec<State>,
    /// Probe class after the latest skill boundary.
    now: u8,
    /// Whether the Rush handle is set.
    rush: bool,
    /// Earliest frame of a probe switch or Rush command filed since the previous query.
    filed: Option<i32>,
    /// From the replay's `base` frame to the last executed frame: each frame's start class, and its recorded sum
    /// in each order (frame by frame, one entry per order).
    starts: Vec<u8>,
    sums: Vec<Bits>,
    /// The (frame, time) of each probe switch filed in frames from `base` on.
    switches: Vec<(i32, i32)>,
}

type Shape<'a> = (u8, u8, bool, Option<i32>, &'a [u8], &'a [(i32, i32)]);

impl Path {
    /// Everything common to the orders that decides the group's native behavior ahead.
    fn shape(&self) -> Shape<'_> {
        (self.class, self.now, self.rush, self.filed, &self.starts, &self.switches)
    }

    /// Whether every order both groups cover has the same recorded sums in both.
    fn agrees(&self, other: &Path) -> bool {
        let lanes = self.low.len();
        each(self.orders & other.orders)
            .all(|order| self.sums.iter().skip(order).step_by(lanes).eq(other.sums.iter().skip(order).step_by(lanes)))
    }

    /// Takes over the orders of `other`, merging the histories of the orders both cover.
    fn absorb(&mut self, other: Path) {
        let lanes = self.low.len();
        for order in each(other.orders) {
            if self.orders >> order & 1 != 0 {
                widen(&mut self.low[order], &mut self.high[order], &other.low[order], &other.high[order]);
            } else {
                self.low[order] = other.low[order];
                self.high[order] = other.high[order];
                for at in (order..self.sums.len()).step_by(lanes) {
                    self.sums[at] = other.sums[at];
                }
            }
        }
        self.orders |= other.orders;
    }

    fn file(&mut self, frame: i32) {
        self.filed = Some(self.filed.map_or(frame, |old| old.min(frame)));
    }
}

/// A note's probe class, (ordinary, Gekisou) combo inputs as binary32 bits, and the least and the greatest factor
/// state of its executions with them in one order.
#[derive(Clone, Copy, Debug)]
pub(super) struct NoteValue {
    pub class: u8,
    pub combo: (u32, u32),
    pub low: State,
    pub high: State,
}

/// The [`NoteValue`] of one class and combo inputs in each of `orders`.
#[derive(Clone, Debug)]
pub(super) struct NoteValues {
    pub orders: Orders,
    pub class: u8,
    pub combo: (u32, u32),
    pub low: Vec<State>,
    pub high: Vec<State>,
}

impl NoteValues {
    /// Adds the value of `orders` to `values`, widening the value of the same class and combo inputs.
    fn join(values: &mut Vec<NoteValues>, class: u8, combo: (u32, u32), orders: Orders, low: &[State], high: &[State]) {
        match values.iter_mut().find(|value| (value.class, value.combo) == (class, combo)) {
            Some(value) => {
                for order in each(orders) {
                    if value.orders >> order & 1 != 0 {
                        widen(&mut value.low[order], &mut value.high[order], &low[order], &high[order]);
                    } else {
                        value.low[order] = low[order];
                        value.high[order] = high[order];
                    }
                }
                value.orders |= orders;
            }
            None => values.push(NoteValues { orders, class, combo, low: low.to_vec(), high: high.to_vec() }),
        }
    }

    pub(super) fn at(&self, order: usize) -> NoteValue {
        NoteValue { class: self.class, combo: self.combo, low: self.low[order], high: self.high[order] }
    }
}

/// One alternative of a frame's execution: probe class, state bounds and recorded sum, per order.
#[derive(Clone)]
struct Sub {
    class: u8,
    low: Vec<State>,
    high: Vec<State>,
    sum: Vec<State>,
}

/// A filed note and the values of its last execution over every path.
#[derive(Clone, Debug)]
pub(super) struct ExactNote {
    pub time_ms: i32,
    pub note_id: i32,
    frame: usize,
    /// The recorder's (frame, index in the frame) of the note, which keys its combo observations.
    key: (usize, usize),
    pub values: Vec<NoteValues>,
}

impl ExactNote {
    /// The values of one order.
    pub(super) fn values_of(&self, order: usize) -> impl Iterator<Item = NoteValue> + '_ {
        self.values.iter().filter(move |value| value.orders >> order & 1 != 0).map(move |value| value.at(order))
    }
}

/// A group being executed: the probe class, the state bounds, and the records of the frames executed so far.
struct Lane {
    class: u8,
    low: Vec<State>,
    high: Vec<State>,
    starts: Vec<u8>,
    sums: Vec<Bits>,
}

/// One note execution: the note, its probe class, combo inputs and orders, and the offset of its low states in
/// [`Observed::states`]; the high states follow them.
struct Execution {
    note: usize,
    class: u8,
    combo: (u32, u32),
    orders: Orders,
    at: usize,
}

/// The note executions of one query.
#[derive(Default)]
struct Observed {
    entries: Vec<Execution>,
    states: Vec<State>,
}

impl Observed {
    fn push(&mut self, note: usize, class: u8, combo: (u32, u32), orders: Orders, sub: &Sub) {
        self.entries.push(Execution { note, class, combo, orders, at: self.states.len() });
        self.states.extend_from_slice(&sub.low);
        self.states.extend_from_slice(&sub.high);
    }
}

/// The [`LuckDpCertifiedResult::rush_transitions`] entry of play frame `ready`. The potentials of a play frame
/// precede its readiness mark.
///
/// [`LuckDpCertifiedResult::rush_transitions`]: crate::live::full::LuckDpCertifiedResult::rush_transitions
pub(super) fn rush_mask(rush_transitions: &[u16], ready: usize) -> Result<u16, Error> {
    rush_transitions.get(ready).copied().ok_or_else(|| refuse("a Rush potential follows the last play frame"))
}

/// Whether a Rush potential may file under its play frame's `mask`: at the frame's start only a range FINISH files,
/// and the frame's lotteries only along a filing transition.
pub(super) fn rush_may_file(mask: u16, start: bool) -> bool {
    if start { mask & RUSH_FINISH != 0 } else { mask & 0xaa != 0 }
}

/// For each score query of `events`, the earliest frame any later query may undo or execute again, `i32::MAX`
/// after the last query. Every filing since a query's predecessor may set that query's rewind target. A factor
/// command of some orders only counts as a filing too, so the floors hold for each order.
pub(super) fn undo_floors(events: &[BoundsEvent], rush_transitions: &[u16]) -> Result<Vec<i32>, Error> {
    let mut lowest = Vec::new();
    let (mut prev, mut filed, mut ready) = (-1i32, None::<i32>, 0usize);
    for event in events {
        let frame = match event {
            BoundsEvent::Potential { start, .. } if !rush_may_file(rush_mask(rush_transitions, ready)?, *start) => {
                continue;
            }
            BoundsEvent::ProbabilityReady(_) => {
                ready += 1;
                continue;
            }
            BoundsEvent::Note { frame, .. }
            | BoundsEvent::Factor { frame, .. }
            | BoundsEvent::Potential { frame, .. }
            | BoundsEvent::Probe { frame, .. } => *frame as i32,
            BoundsEvent::Query { to, .. } => {
                let u = filed.map_or(*to, |frame| (*to).min(frame - 1));
                lowest.push(if u < prev { u + 1 } else { prev + 1 });
                prev = *to;
                filed = None;
                continue;
            }
            _ => continue,
        };
        filed = Some(filed.map_or(frame, |old| old.min(frame)));
    }
    let mut floors = vec![i32::MAX; lowest.len()];
    let mut floor = i32::MAX;
    for (out, &lowest) in floors.iter_mut().zip(&lowest).rev() {
        *out = floor;
        floor = floor.min(lowest);
    }
    Ok(floors)
}

/// Every owner-local merge of the commands of one time with the probe rows that preserves the filing order of each.
/// Owners stay in native score order; their independent interleavings form a Cartesian product.
fn merges(group: &[Op], rows: &[ProbeRow]) -> Result<Vec<Vec<Step>>, Error> {
    fn interleavings(ops: Range<usize>, rows: Range<usize>, out: &mut Vec<Vec<Step>>, prefix: &mut Vec<Step>) {
        if ops.is_empty() || rows.is_empty() {
            let mut merge = prefix.clone();
            merge.extend(ops.map(Step::Op).chain(rows.map(Step::Row)));
            out.push(merge);
            return;
        }
        prefix.push(Step::Op(ops.start));
        interleavings(ops.start + 1..ops.end, rows.clone(), out, prefix);
        prefix.pop();
        prefix.push(Step::Row(rows.start));
        interleavings(ops, rows.start + 1..rows.end, out, prefix);
        prefix.pop();
    }
    fn choose(a: usize, b: usize) -> Option<usize> {
        let (n, k) = (a.checked_add(b)?, a.min(b));
        let mut count = 1usize;
        for i in 1..=k {
            count = count.checked_mul(n - k + i)? / i;
            if count > MAX_MERGES {
                return None;
            }
        }
        Some(count)
    }
    let mut merged = vec![Vec::with_capacity(group.len() + rows.len())];
    let (mut g, mut r) = (0, 0);
    while g < group.len() || r < rows.len() {
        let owner = match (group.get(g), rows.get(r)) {
            (Some(op), Some(row)) => op.owner.min(row.owner),
            (Some(op), None) => op.owner,
            (None, Some(row)) => row.owner,
            (None, None) => unreachable!("a group item remains"),
        };
        let ops = g..g + group[g..].partition_point(|op| op.owner == owner);
        let probes = r..r + rows[r..].partition_point(|row| row.owner == owner);
        let count = choose(ops.len(), probes.len()).ok_or_else(capacity)?;
        if merged.len().checked_mul(count).is_none_or(|total| total > MAX_MERGES) {
            return Err(capacity());
        }
        let mut blocks = Vec::with_capacity(count);
        interleavings(ops.clone(), probes.clone(), &mut blocks, &mut Vec::new());
        merged = merged
            .iter()
            .flat_map(|prefix| {
                blocks.iter().map(move |block| {
                    let mut merge = prefix.clone();
                    merge.extend_from_slice(block);
                    merge
                })
            })
            .collect();
        (g, r) = (ops.end, probes.end);
    }
    Ok(merged)
}

/// The native factor histories of every lottery path through the recorder's command and query schedule, in each of
/// one or more orders.
pub(super) struct ExactReplay {
    frames: Vec<Frame>,
    pub notes: Vec<ExactNote>,
    /// Probe rows by owner, filing order on equal owners.
    rows: Vec<ProbeRow>,
    paths: Vec<Path>,
    lanes: usize,
    /// First frame whose record the paths retain.
    base: i32,
    prev: i32,
    /// Earliest frame of a filing every path makes in every order since the previous query, and per order the
    /// earliest frame of a command only some orders file.
    mandatory: Option<i32>,
    partial: Vec<Option<i32>>,
    /// [`undo_floors`] of the schedule.
    floors: Vec<i32>,
    queries: usize,
    pub peak_paths: usize,
    /// Histories executed by queries, the score frames they undid and executed, and the queries that executed no
    /// frame.
    pub runs: u64,
    pub undone_frames: u64,
    pub executed_frames: u64,
    pub idle_queries: u64,
}

impl ExactReplay {
    /// A replay of `lanes` orders.
    pub(super) fn new(frames: usize, rows: &[ProbeRow], floors: Vec<i32>, lanes: usize) -> Result<Self, Error> {
        if !(1..=MAX_LANES).contains(&lanes) {
            return Err(Error::Input(format!("an exact replay follows 1..={MAX_LANES} orders")));
        }
        let mut rows = rows.to_vec();
        rows.sort_by_key(|row| row.owner);
        let mut state = [0f32; FIELDS];
        state[1] = 1.0;
        let path = Path {
            orders: first_orders(lanes),
            class: 0,
            low: vec![state; lanes],
            high: vec![state; lanes],
            now: 0,
            rush: false,
            filed: None,
            starts: Vec::new(),
            sums: Vec::new(),
            switches: Vec::new(),
        };
        Ok(Self {
            frames: vec![Frame::default(); frames],
            notes: Vec::new(),
            rows,
            paths: vec![path],
            lanes,
            base: 0,
            prev: -1,
            mandatory: None,
            partial: vec![None; lanes],
            floors,
            queries: 0,
            peak_paths: 1,
            runs: 0,
            undone_frames: 0,
            executed_frames: 0,
            idle_queries: 0,
        })
    }

    /// Without probe rows both probe classes have the same factor history.
    pub(super) fn shared_classes(&self) -> bool {
        self.rows.is_empty()
    }

    fn mandatory(&mut self, frame: usize) {
        self.mandatory = Some(self.mandatory.map_or(frame as i32, |old| old.min(frame as i32)));
    }

    /// A note filing; returns the note's index in [`Self::notes`].
    pub(super) fn file_note(
        &mut self,
        frame: usize,
        time_ms: i32,
        note_id: i32,
        key: (usize, usize),
    ) -> Result<usize, Error> {
        let index = self.notes.len();
        self.notes.push(ExactNote { time_ms, note_id, frame, key, values: Vec::new() });
        let notes = &self.notes;
        let entry =
            self.frames.get_mut(frame).ok_or_else(|| Error::Input("note frame outside the calculator".into()))?;
        let at = entry.notes.partition_point(|&n| (notes[n].time_ms, notes[n].note_id) <= (time_ms, note_id));
        entry.notes.insert(at, index);
        self.mandatory(frame);
        Ok(index)
    }

    /// A factor command filing in the schedules of `orders`. A Rush command changes no float field and differs
    /// between paths: it only moves the rewind target, and the paths file it through [`Self::finish`] and
    /// [`Self::lotteries`]. Every other command is filed by every path.
    pub(super) fn file_command(&mut self, frame: usize, command: &FactorCommand, orders: Orders) -> Result<(), Error> {
        let every = first_orders(self.lanes);
        let orders = orders & every;
        let op = Op::of(command, orders);
        if command.luck != 0 {
            if op.changes() {
                return Err(refuse("a Rush command changes a float factor"));
            }
            if orders != every {
                return Err(refuse("a Rush command belongs to some orders only"));
            }
            return Ok(());
        }
        if orders == 0 {
            return Ok(());
        }
        let entry =
            self.frames.get_mut(frame).ok_or_else(|| Error::Input("score frame outside the calculator".into()))?;
        if op.changes() {
            let key = (op.time, op.owner);
            let at = entry.ops.partition_point(|o| (o.time, o.owner) <= key);
            entry.ops.insert(at, op);
        }
        if orders == every {
            self.mandatory(frame);
        } else {
            for order in each(orders) {
                let partial = &mut self.partial[order];
                *partial = Some(partial.map_or(frame as i32, |old| old.min(frame as i32)));
            }
        }
        Ok(())
    }

    /// A range FINISH at the start of a play frame, filing in `frame`: every path whose Rush runs disables it.
    pub(super) fn finish(&mut self, frame: usize) -> Result<(), Error> {
        let mut paths = std::mem::take(&mut self.paths);
        for path in paths.iter_mut().filter(|path| path.rush) {
            path.rush = false;
            path.file(frame as i32);
        }
        self.settle(paths)
    }

    /// The lotteries of one play frame, which may file Rush commands in `frames`. `mask` holds bit
    /// `4 * before + 2 * after + filed` for each Rush transition the lottery law admits; a filing transition files
    /// in any of `frames`.
    pub(super) fn lotteries(&mut self, frames: &[i32], mask: u16) -> Result<(), Error> {
        let mut frames = frames.to_vec();
        frames.sort_unstable();
        frames.dedup();
        let mut next = Vec::with_capacity(self.paths.len() * 2);
        for path in self.paths.drain(..) {
            for bit in 0..4u16 {
                let (after, filed) = (bit >> 1 == 1, bit & 1 == 1);
                if mask >> (4 * u16::from(path.rush) + bit) & 1 == 0 {
                    continue;
                }
                if !filed {
                    let mut kept = path.clone();
                    kept.rush = after;
                    next.push(kept);
                    continue;
                }
                if frames.is_empty() {
                    return Err(refuse("a lottery Rush transition has no filing place"));
                }
                for &frame in &frames {
                    let mut forked = path.clone();
                    forked.rush = after;
                    forked.file(frame);
                    next.push(forked);
                }
            }
        }
        if next.is_empty() {
            return Err(refuse("no native path follows the certified Rush transitions"));
        }
        self.settle(next)
    }

    /// A skill boundary at which every probe row may switch together, filing at `time_ms` in `frame`. `mask` holds
    /// bit `2 * old + new` for each probe class transition the lottery law admits there; with `follows`, the new
    /// probe class equals the Rush.
    pub(super) fn probe(&mut self, frame: usize, time_ms: i32, mask: u8, follows: bool) -> Result<(), Error> {
        if self.rows.is_empty() {
            return Ok(());
        }
        let mut next = Vec::with_capacity(self.paths.len() * 2);
        for path in self.paths.drain(..) {
            let class = path.now;
            let admits = |new: u8| mask >> (2 * class + new) & 1 != 0 && (!follows || new == u8::from(path.rush));
            let stay = admits(class);
            let switch = admits(1 - class);
            if switch {
                let mut switched = path.clone();
                switched.now = 1 - class;
                switched.switches.push((frame as i32, time_ms));
                switched.file(frame as i32);
                next.push(switched);
            }
            if stay {
                next.push(path);
            }
        }
        if next.is_empty() {
            return Err(refuse("no native path follows the certified probe transitions"));
        }
        self.settle(next)
    }

    /// Groups of the same shape share an entry while the orders they both cover agree on their records.
    fn settle(&mut self, mut paths: Vec<Path>) -> Result<(), Error> {
        paths.sort_by(|a, b| a.shape().cmp(&b.shape()));
        let mut settled: Vec<Path> = Vec::with_capacity(paths.len());
        let mut shape_start = 0;
        for path in paths {
            if settled.get(shape_start).is_none_or(|first| first.shape() != path.shape()) {
                shape_start = settled.len();
            }
            match settled[shape_start..].iter().position(|entry| entry.agrees(&path)) {
                Some(at) => settled[shape_start + at].absorb(path),
                None => settled.push(path),
            }
        }
        if settled.len() > MAX_PATHS {
            return Err(capacity());
        }
        self.peak_paths = self.peak_paths.max(settled.len());
        self.paths = settled;
        Ok(())
    }

    /// The native `calculate` up to score frame `to`, with the combo inputs every filed note reads at this query.
    /// Returns the earliest frame any path executed.
    pub(super) fn query(&mut self, to: i32, combos: &FxHashMap<(usize, usize), (f32, f32)>) -> Result<i32, Error> {
        let prev = self.prev;
        let start = |u: i32| if u < prev { u + 1 } else { prev + 1 };
        let filings: Vec<Option<i32>> = self
            .partial
            .iter()
            .map(|partial| match (self.mandatory, *partial) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            })
            .collect();
        // (path, orders, first frame executed): the orders of a path rewound to the same frame.
        let mut forks: Vec<(usize, Orders, i32)> = Vec::new();
        for (index, path) in self.paths.iter().enumerate() {
            let at = forks.len();
            for order in each(path.orders) {
                let own = match (filings[order], path.filed) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                let first = start(own.map_or(to, |frame| to.min(frame - 1)));
                match forks[at..].iter_mut().find(|fork| fork.2 == first) {
                    Some(fork) => fork.1 |= 1 << order,
                    None => forks.push((index, 1 << order, first)),
                }
            }
        }
        let lowest = forks.iter().map(|&(_, _, first)| first).min().unwrap_or(prev + 1);
        let mut highest = vec![prev + 1; self.lanes];
        for &(_, orders, first) in &forks {
            for order in each(orders) {
                highest[order] = highest[order].max(first);
            }
        }
        let floor = self.floors.get(self.queries).copied().unwrap_or(i32::MAX).min(to + 1).max(self.base);
        let mut observed = Observed::default();
        let mut next = Vec::with_capacity(forks.len());
        if lowest > to {
            self.idle_queries += 1;
        }
        for &(index, orders, first) in &forks {
            let path = &self.paths[index];
            if first < self.base {
                return Err(refuse("a query undoes a frame below the retained history"));
            }
            let kept = ((first - self.base) as usize).min(path.starts.len());
            self.runs += 1;
            self.undone_frames += (self.prev + 1 - first).max(0) as u64;
            self.executed_frames += (to + 1 - first).max(0) as u64;
            let lanes = self.lanes;
            for lane in self.run(path, orders, first, to, combos, &mut observed)? {
                let drop = ((floor - self.base) as usize).min(kept + lane.starts.len());
                let mut starts = Vec::with_capacity(kept + lane.starts.len() - drop);
                let mut sums = Vec::with_capacity(starts.capacity() * lanes);
                starts.extend_from_slice(&path.starts[drop.min(kept)..kept]);
                sums.extend_from_slice(&path.sums[drop.min(kept) * lanes..kept * lanes]);
                let skip = drop.saturating_sub(kept);
                starts.extend_from_slice(&lane.starts[skip..]);
                sums.extend_from_slice(&lane.sums[skip * lanes..]);
                let mut switches = path.switches.clone();
                switches.retain(|&(frame, _)| frame >= floor);
                next.push(Path {
                    orders,
                    class: lane.class,
                    low: lane.low,
                    high: lane.high,
                    now: path.now,
                    rush: path.rush,
                    filed: None,
                    starts,
                    sums,
                    switches,
                });
            }
        }
        let mut fresh = FxHashMap::<usize, Vec<NoteValues>>::default();
        for &Execution { note, class, combo, orders, at } in &observed.entries {
            let lanes = self.lanes;
            let (low, high) = observed.states[at..at + 2 * lanes].split_at(lanes);
            NoteValues::join(fresh.entry(note).or_default(), class, combo, orders, low, high);
        }
        for (index, mut values) in fresh {
            let note = &mut self.notes[index];
            let observed = values.iter().fold(0, |orders, value| orders | value.orders);
            // An order with a path that started after this frame keeps the values of its previous execution.
            let keep = each(observed)
                .filter(|&order| (note.frame as i32) < highest[order])
                .fold(0, |orders: Orders, order| orders | 1 << order);
            for old in &note.values {
                let carried = old.orders & (!observed | keep);
                if carried != 0 {
                    NoteValues::join(&mut values, old.class, old.combo, carried, &old.low, &old.high);
                }
            }
            if values.len() > MAX_NOTE_VALUES {
                return Err(capacity());
            }
            note.values = values;
        }
        self.base = floor;
        self.settle(next)?;
        self.prev = to;
        self.mandatory = None;
        self.partial.iter_mut().for_each(|partial| *partial = None);
        self.queries += 1;
        Ok(lowest)
    }

    /// Undo `orders` of `path` from the last executed frame down to `first`, then execute `first..=to`.
    fn run(
        &self,
        path: &Path,
        orders: Orders,
        first: i32,
        to: i32,
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
        observed: &mut Observed,
    ) -> Result<Vec<Lane>, Error> {
        let (mut low, mut high) = (path.low.clone(), path.high.clone());
        let mut class = path.class;
        for frame in (first..=self.prev).rev() {
            let at = (frame - self.base) as usize;
            let (Some(&start), Some(sums)) =
                (path.starts.get(at), path.sums.get(at * self.lanes..(at + 1) * self.lanes))
            else {
                return Err(refuse("a query undoes a frame without a retained record"));
            };
            for order in each(orders) {
                for field in 0..FIELDS {
                    low[order][field] -= f32::from_bits(sums[order][field]);
                    high[order][field] -= f32::from_bits(sums[order][field]);
                }
            }
            finite(&low, orders)?;
            finite(&high, orders)?;
            class = start;
        }
        let mut lanes = vec![Lane { class, low, high, starts: Vec::new(), sums: Vec::new() }];
        for frame in first..=to {
            let mut switches: Vec<i32> =
                path.switches.iter().filter(|&&(f, _)| f == frame).map(|&(_, time)| time).collect();
            switches.sort_unstable();
            let mut next = Vec::with_capacity(lanes.len());
            for lane in lanes {
                self.execute(frame as usize, lane, orders, &switches, combos, observed, &mut next)?;
            }
            if next.len() > 1 {
                next.sort_unstable_by(|a, b| (a.class, &a.starts, &a.sums).cmp(&(b.class, &b.starts, &b.sums)));
                next.dedup_by(|lane, kept| {
                    let same = (lane.class, &lane.starts, &lane.sums) == (kept.class, &kept.starts, &kept.sums);
                    if same {
                        for order in each(orders) {
                            widen(&mut kept.low[order], &mut kept.high[order], &lane.low[order], &lane.high[order]);
                        }
                    }
                    same
                });
            }
            if next.len() > MAX_PATHS {
                return Err(capacity());
            }
            lanes = next;
        }
        Ok(lanes)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        frame: usize,
        lane: Lane,
        orders: Orders,
        switches: &[i32],
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
        observed: &mut Observed,
        out: &mut Vec<Lane>,
    ) -> Result<(), Error> {
        let entry = self.frames.get(frame).ok_or_else(|| Error::Input("score frame outside the calculator".into()))?;
        let (ops, notes) = (&entry.ops, &entry.notes);
        let start = lane.class;
        // Alternative orders of a switch split the group.
        let mut subs =
            vec![Sub { class: lane.class, low: lane.low, high: lane.high, sum: vec![[0f32; FIELDS]; self.lanes] }];
        let (mut op, mut switch, mut note) = (0, 0, 0);
        loop {
            let op_time = ops.get(op).map(|o| o.time);
            let switch_time = switches.get(switch).copied();
            let factor_time = match (op_time, switch_time) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let note_time = notes.get(note).map(|&n| self.notes[n].time_ms);
            match (factor_time, note_time) {
                (Some(t), n) if n.is_none_or(|n| n >= t) => {
                    if switch_time == Some(t) {
                        let end = ops[op..].partition_point(|o| o.time == t);
                        // Commands of other orders change nothing here and need no place among the rows.
                        let group: Vec<Op> =
                            ops[op..op + end].iter().filter(|o| o.orders & orders != 0).copied().collect();
                        let merged = merges(&group, &self.rows)?;
                        let mut next: Vec<Sub> = Vec::with_capacity(subs.len() * merged.len());
                        for sub in &subs {
                            let sign = if sub.class == 0 { 1f32 } else { -1f32 };
                            for merge in &merged {
                                let mut sub = sub.clone();
                                for step in merge {
                                    match *step {
                                        Step::Op(i) => group[i].apply(&mut sub, orders),
                                        Step::Row(r) => apply_probe(sign * self.rows[r].value, &mut sub, orders),
                                    }
                                }
                                finite(&sub.low, orders)?;
                                finite(&sub.high, orders)?;
                                sub.class = 1 - sub.class;
                                next.push(sub);
                            }
                        }
                        if next.len() > 1 {
                            let key = |sub: &Sub| (sub.class, bits(&sub.sum));
                            next.sort_by_cached_key(key);
                            next.dedup_by(|sub, kept| {
                                let same = key(sub) == key(kept);
                                if same {
                                    for order in each(orders) {
                                        widen(
                                            &mut kept.low[order],
                                            &mut kept.high[order],
                                            &sub.low[order],
                                            &sub.high[order],
                                        );
                                    }
                                }
                                same
                            });
                        }
                        if next.len() > MAX_PATHS {
                            return Err(capacity());
                        }
                        subs = next;
                        op += end;
                        switch += 1;
                    } else {
                        for sub in &mut subs {
                            ops[op].apply(sub, orders);
                            finite(&sub.low, orders)?;
                            finite(&sub.high, orders)?;
                        }
                        op += 1;
                    }
                }
                (_, Some(_)) => {
                    let index = notes[note];
                    let &(ordinary, gekisou) = combos
                        .get(&self.notes[index].key)
                        .ok_or_else(|| refuse("note combo observation is missing"))?;
                    for sub in &subs {
                        observed.push(index, sub.class, (ordinary.to_bits(), gekisou.to_bits()), orders, sub);
                    }
                    note += 1;
                }
                (None, None) => break,
                (Some(_), None) => unreachable!("covered by the factor arm"),
            }
        }
        for sub in subs {
            let mut starts = lane.starts.clone();
            starts.push(start);
            let mut sums = lane.sums.clone();
            sums.extend(sub.sum.iter().map(|sum| sum.map(f32::to_bits)));
            out.push(Lane { class: sub.class, low: sub.low, high: sub.high, starts, sums });
        }
        Ok(())
    }
}

fn bits(states: &[State]) -> Vec<Bits> {
    states.iter().map(|state| state.map(f32::to_bits)).collect()
}
