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
//! that belong to some of the orders and in the owners and values of their probe rows, and the orders share the
//! possible lottery transitions. A group then covers a set of orders whose histories have the same states, and
//! every other part of the group is common to them. Groups split where their orders part: at a command of some of
//! them, at a probe switch of orders with different probe rows, and where a query rewinds them to different frames.
//! Groups of the same schedule ahead merge again: an order in several of them takes the widest states, exactly as
//! a replay of that order alone merges its histories, and orders with the same states share one group. Monotone
//! rounding makes the widest states of merged groups evolve as the groups would separately, so where the merges
//! happen changes no order's note values.

use super::{BoundsEvent, FIELDS, ProbeRow};
use crate::error::Error;
use crate::live::full::luck_dp::RUSH_FINISH;
use crate::live::skill::FactorCommand;
use crate::num::FxHashMap;
use std::cmp::Ordering;
use std::ops::Range;

/// Per order of a replay, the most path groups and note values.
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

/// Splits each set of `cells` into its orders in `by` and the rest.
pub(super) fn refine(cells: &mut Vec<Orders>, by: Orders) {
    for index in 0..cells.len() {
        let (cell, inside) = (cells[index], cells[index] & by);
        if inside != 0 && inside != cell {
            cells[index] = inside;
            cells.push(cell & !inside);
        }
    }
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

    fn apply(&self, sub: &mut Sub) {
        for field in 0..FIELDS {
            let delta = self.deltas[field];
            if self.always >> field & 1 != 0 || delta != 0.0 {
                sub.low[field] += delta;
                sub.high[field] += delta;
                sub.sum[field] += delta;
            }
        }
    }
}

fn apply_probe(value: f32, sub: &mut Sub) {
    if value != 0.0 {
        sub.low[1] += value;
        sub.high[1] += value;
        sub.sum[1] += value;
    }
}

/// Widens `low..=high` to cover `other_low..=other_high`, field by field.
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

fn same(a: &State, b: &State) -> bool {
    a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}

fn finite(states: [&State; 2]) -> Result<(), Error> {
    if states.iter().all(|state| state.iter().all(|value| value.is_finite())) {
        Ok(())
    } else {
        Err(refuse("a factor state may leave the finite binary32 range"))
    }
}

/// A group of orders whose histories share the least and the greatest state.
trait Bounded: Clone {
    fn orders(&self) -> Orders;
    fn set_orders(&mut self, orders: Orders);
    fn low(&self) -> &State;
    fn high(&self) -> &State;
    fn set_bounds(&mut self, low: State, high: State);
}

macro_rules! bounded {
    ($($kind:ty),*) => {$(
        impl Bounded for $kind {
            fn orders(&self) -> Orders {
                self.orders
            }
            fn set_orders(&mut self, orders: Orders) {
                self.orders = orders;
            }
            fn low(&self) -> &State {
                &self.low
            }
            fn high(&self) -> &State {
                &self.high
            }
            fn set_bounds(&mut self, low: State, high: State) {
                (self.low, self.high) = (low, high);
            }
        }
    )*};
}

bounded!(Path, Lane, Sub, NoteValue);

/// Appends `items`, which share everything but their orders and states, to `out` merged: an order of several
/// items takes the widest states of its items, and orders with bitwise equal states share one item.
fn cover<T: Bounded>(items: impl IntoIterator<Item = T>, out: &mut Vec<T>) {
    let start = out.len();
    for mut item in items {
        let mut rest = item.orders();
        let end = out.len();
        for index in start..end {
            let shared = out[index].orders() & rest;
            if shared == 0 {
                continue;
            }
            rest &= !shared;
            let (mut low, mut high) = (*out[index].low(), *out[index].high());
            widen(&mut low, &mut high, item.low(), item.high());
            if same(&low, out[index].low()) && same(&high, out[index].high()) {
                continue;
            }
            if shared == out[index].orders() {
                out[index].set_bounds(low, high);
            } else {
                let mut part = out[index].clone();
                let left = out[index].orders() & !shared;
                out[index].set_orders(left);
                part.set_orders(shared);
                part.set_bounds(low, high);
                out.push(part);
            }
        }
        if rest != 0 {
            item.set_orders(rest);
            out.push(item);
        }
    }
    let mut index = start;
    while index < out.len() {
        let mut other = index + 1;
        while other < out.len() {
            if same(out[other].low(), out[index].low()) && same(out[other].high(), out[index].high()) {
                let orders = out.swap_remove(other).orders() | out[index].orders();
                out[index].set_orders(orders);
            } else {
                other += 1;
            }
        }
        index += 1;
    }
}

/// Merges the items of `items` that `order` ranks equal, see [`cover`].
fn merge_equal<T: Bounded>(items: &mut Vec<T>, order: impl Fn(&T, &T) -> Ordering) {
    if items.len() < 2 {
        return;
    }
    items.sort_by(&order);
    let mut sorted = std::mem::take(items).into_iter().peekable();
    let mut group = Vec::new();
    while let Some(item) = sorted.next() {
        group.push(item);
        while sorted.peek().is_some_and(|next| order(&group[0], next) == Ordering::Equal) {
            group.extend(sorted.next());
        }
        if group.len() == 1 {
            items.extend(group.pop());
        } else {
            cover(group.drain(..), items);
        }
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
    /// Per field, the least and the greatest state of the group's histories.
    low: State,
    high: State,
    /// Probe class after the latest skill boundary.
    now: u8,
    /// Whether the Rush handle is set.
    rush: bool,
    /// Earliest frame of a probe switch or Rush command filed since the previous query.
    filed: Option<i32>,
    /// From the replay's `base` frame to the last executed frame: each frame's start class and recorded sum.
    starts: Vec<u8>,
    sums: Vec<Bits>,
    /// The (frame, time) of each probe switch filed in frames from `base` on.
    switches: Vec<(i32, i32)>,
}

impl Path {
    /// Orders paths by everything that decides their native behavior ahead.
    fn schedule(a: &Path, b: &Path) -> Ordering {
        (a.class, a.now, a.rush, a.filed, &a.starts, &a.switches, &a.sums).cmp(&(
            b.class,
            b.now,
            b.rush,
            b.filed,
            &b.starts,
            &b.switches,
            &b.sums,
        ))
    }

    fn file(&mut self, frame: i32) {
        self.filed = Some(self.filed.map_or(frame, |old| old.min(frame)));
    }
}

/// A note's probe class, (ordinary, Gekisou) combo inputs as binary32 bits, and the least and the greatest factor
/// state of its executions with them in each of `orders`.
#[derive(Clone, Copy, Debug)]
pub(super) struct NoteValue {
    pub orders: Orders,
    pub class: u8,
    pub combo: (u32, u32),
    pub low: State,
    pub high: State,
}

/// One alternative of a frame's execution: its orders, probe class, state bounds and recorded sum.
#[derive(Clone, Copy)]
struct Sub {
    orders: Orders,
    class: u8,
    low: State,
    high: State,
    sum: State,
}

impl Sub {
    fn order(a: &Sub, b: &Sub) -> Ordering {
        (a.class, a.sum.map(f32::to_bits)).cmp(&(b.class, b.sum.map(f32::to_bits)))
    }
}

/// A filed note and the values of its last execution over every path.
#[derive(Clone, Debug)]
pub(super) struct ExactNote {
    pub time_ms: i32,
    pub note_id: i32,
    frame: usize,
    /// The recorder's (frame, index in the frame) of the note, which keys its combo observations.
    key: (usize, usize),
    /// The values by (class, combo inputs); those of one key cover disjoint orders.
    pub values: Vec<NoteValue>,
    /// How many times a query replaced `values`.
    pub revision: u32,
}

impl ExactNote {
    /// The values of one order.
    #[cfg(test)]
    pub(super) fn values_of(&self, order: usize) -> impl Iterator<Item = &NoteValue> + '_ {
        self.values.iter().filter(move |value| value.orders >> order & 1 != 0)
    }
}

/// A group being executed: its orders, the probe class, the state bounds, and the records of the frames executed
/// so far.
#[derive(Clone)]
struct Lane {
    orders: Orders,
    class: u8,
    low: State,
    high: State,
    starts: Vec<u8>,
    sums: Vec<Bits>,
}

impl Lane {
    fn order(a: &Lane, b: &Lane) -> Ordering {
        (a.class, &a.starts, &a.sums).cmp(&(b.class, &b.starts, &b.sums))
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
pub(super) fn undo_floors<'a>(
    events: impl IntoIterator<Item = &'a BoundsEvent>,
    rush_transitions: &[u16],
) -> Result<Vec<i32>, Error> {
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
    /// The orders of each probe row configuration and its rows by owner, filing order on equal owners.
    configs: Vec<(Orders, Vec<ProbeRow>)>,
    /// Whether the orders have probe rows.
    rows: bool,
    paths: Vec<Path>,
    lanes: usize,
    /// First frame whose record the paths retain.
    base: i32,
    prev: i32,
    /// Earliest frame of a filing every path makes in every order since the previous query, and the frames of the
    /// commands only some orders filed since then, with those orders.
    mandatory: Option<i32>,
    partial: Vec<(i32, Orders)>,
    /// [`undo_floors`] of the schedule.
    floors: Vec<i32>,
    queries: usize,
    /// The orders the last query rewound to each first executed frame.
    firsts: Vec<(i32, Orders)>,
    /// The note executions of the current query.
    observed: Vec<(usize, NoteValue)>,
    pub peak_paths: usize,
    /// Histories executed by queries, the score frames they undid and executed, and the queries that executed no
    /// frame.
    pub runs: u64,
    pub undone_frames: u64,
    pub executed_frames: u64,
    pub idle_queries: u64,
}

impl ExactReplay {
    /// A replay of `lanes` orders, whose probe rows `configs` give by the orders that have them.
    pub(super) fn new(
        frames: usize,
        configs: &[(Orders, Vec<ProbeRow>)],
        floors: Vec<i32>,
        lanes: usize,
    ) -> Result<Self, Error> {
        if !(1..=MAX_LANES).contains(&lanes) {
            return Err(Error::Input(format!("an exact replay follows 1..={MAX_LANES} orders")));
        }
        let every = first_orders(lanes);
        let mut covered = 0;
        let mut sorted = Vec::with_capacity(configs.len());
        for (orders, rows) in configs {
            if *orders == 0 || orders & (covered | !every) != 0 {
                return Err(Error::Input("probe row configurations partition the replay's orders".into()));
            }
            covered |= orders;
            let mut rows = rows.clone();
            rows.sort_by_key(|row| row.owner);
            sorted.push((*orders, rows));
        }
        let rows = sorted.iter().any(|(_, rows)| !rows.is_empty());
        if covered != every || sorted.iter().any(|(_, config)| config.is_empty() == rows) {
            return Err(Error::Input("every order of a replay or none has probe rows".into()));
        }
        let mut state = [0f32; FIELDS];
        state[1] = 1.0;
        let path = Path {
            orders: every,
            class: 0,
            low: state,
            high: state,
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
            configs: sorted,
            rows,
            paths: vec![path],
            lanes,
            base: 0,
            prev: -1,
            mandatory: None,
            partial: Vec::new(),
            floors,
            queries: 0,
            firsts: Vec::new(),
            observed: Vec::new(),
            peak_paths: 1,
            runs: 0,
            undone_frames: 0,
            executed_frames: 0,
            idle_queries: 0,
        })
    }

    /// Without probe rows both probe classes have the same factor history.
    pub(super) fn shared_classes(&self) -> bool {
        !self.rows
    }

    /// The first frame whose record the paths retain: no later query executes an earlier frame.
    pub(super) fn base(&self) -> i32 {
        self.base
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
        self.notes.push(ExactNote { time_ms, note_id, frame, key, values: Vec::new(), revision: 0 });
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
            match self.partial.iter_mut().find(|(other, _)| *other == frame as i32) {
                Some((_, filed)) => *filed |= orders,
                None => self.partial.push((frame as i32, orders)),
            }
        }
        Ok(())
    }

    /// A range FINISH at the start of a play frame, filing in `frame`: every path whose Rush runs disables it.
    pub(super) fn finish(&mut self, frame: usize) -> Result<(), Error> {
        if self.paths.iter().all(|path| !path.rush) {
            return Ok(());
        }
        for path in self.paths.iter_mut().filter(|path| path.rush) {
            path.rush = false;
            path.file(frame as i32);
        }
        self.settle()
    }

    /// The lotteries of one play frame, which may file Rush commands in `frames`. `mask` holds bit
    /// `4 * before + 2 * after + filed` for each Rush transition the lottery law admits; a filing transition files
    /// in any of `frames`.
    pub(super) fn lotteries(&mut self, frames: &[i32], mask: u16) -> Result<(), Error> {
        // Paths whose only admitted transition keeps their Rush without a filing are unchanged.
        let rush = |path: &Path| u16::from(path.rush);
        if self.paths.iter().all(|path| mask >> (4 * rush(path)) & 0xf == 1 << (2 * rush(path))) {
            return Ok(());
        }
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
        self.paths = next;
        self.settle()
    }

    /// A skill boundary at which every probe row may switch together, filing at `time_ms` in `frame`. `mask` holds
    /// bit `2 * old + new` for each probe class transition the lottery law admits there; with `follows`, the new
    /// probe class equals the Rush.
    pub(super) fn probe(&mut self, frame: usize, time_ms: i32, mask: u8, follows: bool) -> Result<(), Error> {
        if !self.rows {
            return Ok(());
        }
        let admits =
            |path: &Path, new: u8| mask >> (2 * path.now + new) & 1 != 0 && (!follows || new == u8::from(path.rush));
        if self.paths.iter().all(|path| admits(path, path.now) && !admits(path, 1 - path.now)) {
            return Ok(());
        }
        let mut next = Vec::with_capacity(self.paths.len() * 2);
        for path in self.paths.drain(..) {
            let class = path.now;
            if admits(&path, 1 - class) {
                let mut switched = path.clone();
                switched.now = 1 - class;
                switched.switches.push((frame as i32, time_ms));
                switched.file(frame as i32);
                next.push(switched);
            }
            if admits(&path, class) {
                next.push(path);
            }
        }
        if next.is_empty() {
            return Err(refuse("no native path follows the certified probe transitions"));
        }
        self.paths = next;
        self.settle()
    }

    /// Paths with the same schedule ahead merge.
    fn settle(&mut self) -> Result<(), Error> {
        merge_equal(&mut self.paths, Path::schedule);
        if self.paths.len() > MAX_PATHS * self.lanes {
            return Err(capacity());
        }
        self.peak_paths = self.peak_paths.max(self.paths.len());
        Ok(())
    }

    /// The native `calculate` up to score frame `to`, with the combo inputs every filed note reads at this query.
    /// Returns each first frame some path executed, with the orders of those paths; every order has a path.
    pub(super) fn query(
        &mut self,
        to: i32,
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
    ) -> Result<&[(i32, Orders)], Error> {
        let prev = self.prev;
        let every = first_orders(self.lanes);
        let earliest = |a: Option<i32>, b: Option<i32>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        // The orders of each distinct earliest filing since the previous query.
        let mut filings = Vec::<(Option<i32>, Orders)>::with_capacity(self.partial.len() + 1);
        self.partial.sort_unstable_by_key(|&(frame, _)| frame);
        let mut rest = every;
        for &(frame, orders) in &self.partial {
            let orders = orders & rest;
            if orders == 0 {
                continue;
            }
            rest &= !orders;
            let filing = earliest(self.mandatory, Some(frame));
            match filings.iter_mut().find(|(other, _)| *other == filing) {
                Some((_, all)) => *all |= orders,
                None => filings.push((filing, orders)),
            }
        }
        if rest != 0 {
            match filings.iter_mut().find(|(other, _)| *other == self.mandatory) {
                Some((_, all)) => *all |= rest,
                None => filings.push((self.mandatory, rest)),
            }
        }
        let floor = self.floors.get(self.queries).copied().unwrap_or(i32::MAX).min(to + 1).max(self.base);
        let later = |filing: Option<i32>| filing.is_none_or(|frame| frame > prev);
        self.firsts.clear();
        if to == prev && filings.iter().all(|&(filing, _)| later(filing)) && self.paths.iter().all(|p| later(p.filed)) {
            // Nothing to undo or execute.
            self.idle_queries += 1;
            let drop = (floor - self.base) as usize;
            let filed = self.paths.iter().any(|path| path.filed.is_some());
            for path in &mut self.paths {
                let drop = drop.min(path.starts.len());
                path.starts.drain(..drop);
                path.sums.drain(..drop);
                path.switches.retain(|&(frame, _)| frame >= floor);
                path.filed = None;
            }
            if (drop > 0 || filed) && self.paths.len() > 1 {
                self.settle()?;
            }
            self.firsts.push((prev + 1, every));
        } else {
            self.execute_query(to, floor, &filings, combos)?;
        }
        self.base = floor;
        self.prev = to;
        self.mandatory = None;
        self.partial.clear();
        self.queries += 1;
        Ok(&self.firsts)
    }

    fn execute_query(
        &mut self,
        to: i32,
        floor: i32,
        filings: &[(Option<i32>, Orders)],
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
    ) -> Result<(), Error> {
        let prev = self.prev;
        let start = |u: i32| if u < prev { u + 1 } else { prev + 1 };
        let earliest = |a: Option<i32>, b: Option<i32>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        // (path, orders, first frame executed): the orders of a path rewound to the same frame, path by path.
        let mut forks: Vec<(usize, Orders, i32)> = Vec::with_capacity(self.paths.len());
        for (index, path) in self.paths.iter().enumerate() {
            let at = forks.len();
            for &(filing, orders) in filings {
                let orders = orders & path.orders;
                if orders == 0 {
                    continue;
                }
                let first = start(earliest(filing, path.filed).map_or(to, |frame| to.min(frame - 1)));
                match forks[at..].iter_mut().find(|fork| fork.2 == first) {
                    Some(fork) => fork.1 |= orders,
                    None => forks.push((index, orders, first)),
                }
            }
        }
        for &(_, orders, first) in &forks {
            match self.firsts.iter_mut().find(|(other, _)| *other == first) {
                Some((_, all)) => *all |= orders,
                None => self.firsts.push((first, orders)),
            }
        }
        if self.firsts.iter().all(|&(first, _)| first > to) {
            self.idle_queries += 1;
        }
        let mut observed = std::mem::take(&mut self.observed);
        observed.clear();
        let mut next = Vec::with_capacity(forks.len());
        let mut lanes = Vec::new();
        let mut forks = forks.as_slice();
        for (index, mut path) in std::mem::take(&mut self.paths).into_iter().enumerate() {
            let (own, rest) = forks.split_at(forks.partition_point(|fork| fork.0 == index));
            forks = rest;
            for (fork, &(_, orders, first)) in own.iter().enumerate() {
                if first < self.base {
                    return Err(refuse("a query undoes a frame below the retained history"));
                }
                let kept = ((first - self.base) as usize).min(path.starts.len());
                self.runs += 1;
                self.undone_frames += (prev + 1 - first).max(0) as u64;
                self.executed_frames += (to + 1 - first).max(0) as u64;
                if first > prev && first > to {
                    lanes.push(Lane {
                        orders,
                        class: path.class,
                        low: path.low,
                        high: path.high,
                        starts: Vec::new(),
                        sums: Vec::new(),
                    });
                } else {
                    self.run(&path, orders, first, to, combos, &mut observed, &mut lanes)?;
                }
                // The path's last fork takes over its records.
                let last_fork = fork + 1 == own.len();
                let count = lanes.len();
                for (execution, lane) in lanes.drain(..).enumerate() {
                    let drop = ((floor - self.base) as usize).min(kept + lane.starts.len());
                    let skip = drop.saturating_sub(kept);
                    let (mut starts, mut sums, mut switches) = if last_fork && execution + 1 == count {
                        let (mut starts, mut sums) = (std::mem::take(&mut path.starts), std::mem::take(&mut path.sums));
                        starts.truncate(kept);
                        sums.truncate(kept);
                        starts.drain(..drop.min(kept));
                        sums.drain(..drop.min(kept));
                        (starts, sums, std::mem::take(&mut path.switches))
                    } else {
                        let mut starts = Vec::with_capacity(kept + lane.starts.len() - drop);
                        let mut sums = Vec::with_capacity(starts.capacity());
                        starts.extend_from_slice(&path.starts[drop.min(kept)..kept]);
                        sums.extend_from_slice(&path.sums[drop.min(kept)..kept]);
                        (starts, sums, path.switches.clone())
                    };
                    starts.extend_from_slice(&lane.starts[skip..]);
                    sums.extend_from_slice(&lane.sums[skip..]);
                    switches.retain(|&(frame, _)| frame >= floor);
                    next.push(Path {
                        orders: lane.orders,
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
        }
        // The executions of each note replace the values of its executing orders. An order with a path that
        // started after the note's frame keeps the values of its previous execution too.
        observed.sort_by_key(|&(note, _)| note);
        let mut values = Vec::new();
        let mut at = 0;
        while at < observed.len() {
            let index = observed[at].0;
            let end = at + observed[at..].partition_point(|&(note, _)| note == index);
            let note = &mut self.notes[index];
            let seen = observed[at..end].iter().fold(0, |orders, (_, value)| orders | value.orders);
            let keep = self
                .firsts
                .iter()
                .filter(|&&(first, _)| first > note.frame as i32)
                .fold(0, |orders: Orders, &(_, firsts)| orders | firsts);
            let carried = !seen | (seen & keep);
            values.clear();
            values.extend(observed[at..end].iter().map(|&(_, value)| value));
            values.extend(
                note.values
                    .iter()
                    .filter(|old| old.orders & carried != 0)
                    .map(|old| NoteValue { orders: old.orders & carried, ..*old }),
            );
            merge_equal(&mut values, |a, b| (a.class, a.combo).cmp(&(b.class, b.combo)));
            if values.len() > MAX_NOTE_VALUES * self.lanes {
                return Err(capacity());
            }
            note.values.clone_from(&values);
            note.revision += 1;
            at = end;
        }
        self.observed = observed;
        self.base = floor;
        self.paths = next;
        self.settle()
    }

    /// Undo the histories of `orders` in `path` from the last executed frame down to `first`, then execute
    /// `first..=to`; appends the executed groups to `out`.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        path: &Path,
        orders: Orders,
        first: i32,
        to: i32,
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
        observed: &mut Vec<(usize, NoteValue)>,
        out: &mut Vec<Lane>,
    ) -> Result<(), Error> {
        let (mut low, mut high, mut class) = (path.low, path.high, path.class);
        for frame in (first..=self.prev).rev() {
            let at = (frame - self.base) as usize;
            let (Some(&start), Some(sum)) = (path.starts.get(at), path.sums.get(at)) else {
                return Err(refuse("a query undoes a frame without a retained record"));
            };
            for field in 0..FIELDS {
                low[field] -= f32::from_bits(sum[field]);
                high[field] -= f32::from_bits(sum[field]);
            }
            finite([&low, &high])?;
            class = start;
        }
        let mut lanes = vec![Lane { orders, class, low, high, starts: Vec::new(), sums: Vec::new() }];
        let mut switches = Vec::new();
        for frame in first..=to {
            switches.clear();
            switches.extend(path.switches.iter().filter(|&&(f, _)| f == frame).map(|&(_, time)| time));
            switches.sort_unstable();
            let mut next = Vec::with_capacity(lanes.len());
            for lane in lanes {
                self.execute(frame as usize, lane, &switches, combos, observed, &mut next)?;
            }
            merge_equal(&mut next, Lane::order);
            if next.len() > MAX_PATHS * self.lanes {
                return Err(capacity());
            }
            lanes = next;
        }
        out.extend(lanes);
        Ok(())
    }

    fn execute(
        &self,
        frame: usize,
        lane: Lane,
        switches: &[i32],
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
        observed: &mut Vec<(usize, NoteValue)>,
        out: &mut Vec<Lane>,
    ) -> Result<(), Error> {
        let entry = self.frames.get(frame).ok_or_else(|| Error::Input("score frame outside the calculator".into()))?;
        let (ops, notes) = (&entry.ops, &entry.notes);
        let start = lane.class;
        let mut subs =
            vec![Sub { orders: lane.orders, class: lane.class, low: lane.low, high: lane.high, sum: [0f32; FIELDS] }];
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
                        subs = self.switch(&subs, &ops[op..op + end])?;
                        op += end;
                        switch += 1;
                    } else {
                        let command = &ops[op];
                        let (count, mut changed) = (subs.len(), false);
                        for index in 0..count {
                            let orders = subs[index].orders;
                            let applies = orders & command.orders;
                            if applies == 0 {
                                changed = true;
                                continue;
                            }
                            if applies != orders {
                                let mut rest = subs[index];
                                rest.orders = orders & !applies;
                                subs.push(rest);
                                subs[index].orders = applies;
                                changed = true;
                            }
                            command.apply(&mut subs[index]);
                            finite([&subs[index].low, &subs[index].high])?;
                        }
                        // A command of some orders may give them the state of the others.
                        if changed && subs.len() > 1 {
                            merge_equal(&mut subs, Sub::order);
                        }
                        op += 1;
                    }
                }
                (_, Some(_)) => {
                    let index = notes[note];
                    let &(ordinary, gekisou) = combos
                        .get(&self.notes[index].key)
                        .ok_or_else(|| refuse("note combo observation is missing"))?;
                    let combo = (ordinary.to_bits(), gekisou.to_bits());
                    for sub in &subs {
                        let (orders, class, low, high) = (sub.orders, sub.class, sub.low, sub.high);
                        observed.push((index, NoteValue { orders, class, combo, low, high }));
                    }
                    note += 1;
                }
                (None, None) => break,
                (Some(_), None) => unreachable!("covered by the factor arm"),
            }
        }
        // The last alternative takes over the lane's records.
        let count = subs.len();
        let (mut starts, mut sums) = (lane.starts, lane.sums);
        for (index, sub) in subs.into_iter().enumerate() {
            let (mut starts, mut sums) = if index + 1 == count {
                (std::mem::take(&mut starts), std::mem::take(&mut sums))
            } else {
                (starts.clone(), sums.clone())
            };
            starts.push(start);
            sums.push(sub.sum.map(f32::to_bits));
            out.push(Lane { orders: sub.orders, class: sub.class, low: sub.low, high: sub.high, starts, sums });
        }
        Ok(())
    }

    /// A probe switch among the ordinary commands `group` of its time: every owner-local merge of the commands with
    /// the probe rows of each order, which flips the probe class.
    fn switch(&self, subs: &[Sub], group: &[Op]) -> Result<Vec<Sub>, Error> {
        let mut next = Vec::with_capacity(subs.len() * 2);
        let mut cells = Vec::new();
        let mut applied = Vec::new();
        for sub in subs {
            let sign = if sub.class == 0 { 1f32 } else { -1f32 };
            for (config, rows) in &self.configs {
                // Orders with the same rows and the same commands of this time.
                cells.clear();
                cells.push(sub.orders & config);
                if cells[0] == 0 {
                    continue;
                }
                for command in group {
                    refine(&mut cells, command.orders);
                }
                for &cell in &cells {
                    // Commands of other orders change nothing here and need no place among the rows.
                    applied.clear();
                    applied.extend(group.iter().filter(|command| command.orders & cell != 0).copied());
                    for merge in merges(&applied, rows)? {
                        let mut alternative = Sub { orders: cell, ..*sub };
                        for step in merge {
                            match step {
                                Step::Op(i) => applied[i].apply(&mut alternative),
                                Step::Row(r) => apply_probe(sign * rows[r].value, &mut alternative),
                            }
                        }
                        finite([&alternative.low, &alternative.high])?;
                        alternative.class = 1 - alternative.class;
                        next.push(alternative);
                    }
                }
            }
        }
        merge_equal(&mut next, Sub::order);
        if next.len() > MAX_PATHS * self.lanes {
            return Err(capacity());
        }
        Ok(next)
    }
}
