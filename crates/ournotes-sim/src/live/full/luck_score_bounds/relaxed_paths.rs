//! Binary32 bounds of the native factor histories of every lottery path through the recorder's command and query
//! schedule, for schedules whose exact path groups ([`ExactReplay`]) exceed their budget.
//!
//! [`RelaxedReplay`] follows the schedule as [`ExactReplay`] does and gives note values and first frames of the same
//! meaning, with wider states. For each set of orders whose histories share their bounds it keeps, per probe class,
//! the least and the greatest state of the histories in that class; for each score frame, set of orders and pair of
//! probe classes at the frame's start and end, it keeps the least and the greatest binary32 sum the frame's last
//! executions recorded. Binary32 addition and subtraction rounded to nearest are monotone in each operand, so a
//! command applied to the bounds of a state, and a frame undone by subtracting the bounds of its recorded sum from
//! those of the state, bound the result of every history between them. A recorded sum starts from zero and adds
//! constants, so it does not depend on the state. The histories of one probe class have the same real sums of their
//! applied commands (ordinary commands are common to them, a Rush command changes no float field, and the probe rows
//! are filed in signed pairs), so the bounds of a class only spread over the rounding differences of its histories.
//!
//! Subtracting the bounds of a sum from those of a state takes each history's state with any history's sum, so each
//! undo would add the rounding of the subtraction to the bounds. While every history still holds the end state of its
//! last execution of the most recent frame, the undo of that frame is also bounded history by history: an execution
//! applies one sequence of constants to the state and to the sum, so the state its undo returns is monotone in the
//! state the execution started from, and lies between the undos of the executions from the bounds at the frame's
//! start. Each alternative of an execution keeps the one sum of its sequence for this, and the frame records the
//! bounds of these undos, which the undo of the frame intersects with the subtraction.
//!
//! Every lottery filing a path of [`ExactReplay`] may make is followed here in every history: whenever a frame
//! executes, a history may keep or switch its probe class at each skill boundary of the frame at which a path may file
//! a probe switch, and a query may rewind a history to the frame before each frame in which a path may have filed a
//! Rush command or a probe switch since the previous query. The replay keeps every pair of probe class and Rush a path
//! may have. The histories followed therefore include every native history, and their bounds hold its states.
//!
//! [`ExactReplay`]: super::exact_paths::ExactReplay

use super::exact_paths::{
    Configs, ExactNote, Filings, MAX_LANES, NoteValue, Op, Orders, ReplayWork, State, Step, add_op, add_probe, each,
    finite, first_orders, merges, refine, refuse, sorted_configs, widen,
};
use super::{FIELDS, ProbeRow};
use crate::error::Error;
use crate::live::skill::FactorCommand;
use crate::num::FxHashMap;

/// The least and the greatest state of a set of histories, field by field.
type Hull = (State, State);
/// Per probe class, the bounds of the histories in it; None when no history is.
type Classes = [Option<Hull>; 2];
/// Per probe class at a frame's start and at its end, the bounds of the sums its executions recorded.
type Sums = [[Option<Hull>; 2]; 2];

/// What a frame keeps of its last executions: the bounds of their recorded sums, and per probe class at the frame's
/// start, the bounds of the states their undos return.
#[derive(Clone, Copy)]
struct Record {
    sums: Sums,
    undo: Classes,
}

impl Record {
    fn join(&mut self, other: &Record) {
        for (sum, other) in self.sums.iter_mut().flatten().zip(other.sums.iter().flatten()) {
            if let Some(other) = other {
                join(sum, other);
            }
        }
        for (undo, other) in self.undo.iter_mut().zip(&other.undo) {
            if let Some(other) = other {
                join(undo, other);
            }
        }
    }
}

/// The modes (bit `2 * rush + now`) with the Rush handle set.
const RUSHING: u8 = 0b1100;

fn join(into: &mut Option<Hull>, other: &Hull) {
    match into {
        Some((low, high)) => widen(low, high, &other.0, &other.1),
        None => *into = Some(*other),
    }
}

/// The common part of two bounds of one set of states; None when they are disjoint, so that the set is empty.
fn intersect(a: &Hull, b: &Hull) -> Option<Hull> {
    let (mut low, mut high) = *a;
    for field in 0..FIELDS {
        low[field] = low[field].max(b.0[field]);
        high[field] = high[field].min(b.1[field]);
        if low[field] > high[field] {
            return None;
        }
    }
    Some((low, high))
}

/// The entry of a partition of some orders that holds each order.
struct Owners([u8; MAX_LANES]);

impl Owners {
    fn of(sets: impl IntoIterator<Item = Orders>) -> Self {
        let mut owner = [u8::MAX; MAX_LANES];
        for (index, orders) in sets.into_iter().enumerate() {
            for order in each(orders) {
                owner[order] = index as u8;
            }
        }
        Self(owner)
    }

    /// `orders` by the entry that holds them, None for orders the partition does not hold, into `out`.
    fn split(&self, orders: Orders, out: &mut Vec<(Option<usize>, Orders)>) {
        out.clear();
        for order in each(orders) {
            let entry = (self.0[order] != u8::MAX).then_some(usize::from(self.0[order]));
            match out.iter_mut().find(|(other, _)| *other == entry) {
                Some((_, all)) => *all |= 1 << order,
                None => out.push((entry, 1 << order)),
            }
        }
    }
}

/// Sets of orders whose histories share their bounds, partitioning some orders. The sets only split as the orders'
/// schedules part, so there are at most as many as orders.
type Pieces = Vec<(Orders, Classes)>;

/// The pieces of two partitions of the same orders joined: each order takes the bounds of both.
fn join_pieces(a: &Pieces, b: &Pieces, parts: &mut Vec<(Option<usize>, Orders)>) -> Pieces {
    let owners = Owners::of(a.iter().map(|(orders, _)| *orders));
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    for (orders, other) in b {
        owners.split(*orders, parts);
        for &(entry, orders) in parts.iter() {
            let mut classes = a[entry.expect("partitions of the same orders")].1;
            for (class, other) in classes.iter_mut().zip(other) {
                if let Some(other) = other {
                    join(class, other);
                }
            }
            out.push((orders, classes));
        }
    }
    out
}

/// Undoes a frame from `pieces` at the frame's end with the frame's `records`; returns the pieces at its start. The
/// histories of the orders `last` hold the end states of their last executions of the frame.
fn undo(
    pieces: &Pieces,
    records: &[(Orders, Record)],
    last: Orders,
    parts: &mut Vec<(Option<usize>, Orders)>,
) -> Result<Pieces, Error> {
    let owners = Owners::of(records.iter().map(|(orders, _)| *orders));
    let mut out = Vec::with_capacity(pieces.len());
    for (orders, state) in pieces {
        owners.split(*orders, parts);
        for &(entry, orders) in parts.iter() {
            let entry = entry.ok_or_else(|| refuse("a query undoes a frame without a retained record"))?;
            let Record { sums, undo } = &records[entry].1;
            let mut classes: Classes = [None, None];
            for (end, state) in state.iter().enumerate() {
                let Some((low, high)) = state else { continue };
                let mut recorded = false;
                for (start, class) in classes.iter_mut().enumerate() {
                    let Some((sum_low, sum_high)) = &sums[start][end] else { continue };
                    recorded = true;
                    let (mut a, mut b) = (*low, *high);
                    for field in 0..FIELDS {
                        a[field] -= sum_high[field];
                        b[field] -= sum_low[field];
                    }
                    finite([&a, &b])?;
                    join(class, &(a, b));
                }
                if !recorded {
                    return Err(refuse("a query undoes a probe class its frame did not record"));
                }
            }
            let mut split = |orders: Orders, paired: bool| -> Result<(), Error> {
                if orders == 0 {
                    return Ok(());
                }
                let mut classes = classes;
                if paired {
                    for (class, undo) in classes.iter_mut().zip(undo) {
                        *class = class.as_ref().zip(undo.as_ref()).and_then(|(a, b)| intersect(a, b));
                    }
                    if classes.iter().all(Option::is_none) {
                        return Err(Error::Input("disjoint bounds of one native undo".into()));
                    }
                }
                out.push((orders, classes));
                Ok(())
            };
            split(orders & last, true)?;
            split(orders & !last, false)?;
        }
    }
    Ok(out)
}

/// The records of a frame after executions of it by the orders of `executed`, each with what it recorded: they
/// replace the records of their orders when every history of those orders executed the frame (`all`), and widen them
/// otherwise.
fn record(
    records: &mut Vec<(Orders, Record)>,
    executed: &[(Orders, Classes, Record)],
    all: bool,
    parts: &mut Vec<(Option<usize>, Orders)>,
) {
    let ran = executed.iter().fold(0, |all, (orders, _, _)| all | orders);
    let old = std::mem::take(records);
    let owners = (!all).then(|| Owners::of(old.iter().map(|(orders, _)| *orders)));
    records.extend(old.iter().filter(|(orders, _)| orders & !ran != 0).map(|(orders, kept)| (orders & !ran, *kept)));
    for (orders, _, recorded) in executed {
        let Some(owners) = &owners else {
            records.push((*orders, *recorded));
            continue;
        };
        owners.split(*orders, parts);
        for &(entry, orders) in parts.iter() {
            let mut recorded = *recorded;
            if let Some(entry) = entry {
                recorded.join(&old[entry].1);
            }
            records.push((orders, recorded));
        }
    }
}

/// One alternative of a frame's execution: its orders, its probe classes at the frame's start and now, the bounds of
/// its state, and the sum its sequence of constants records.
#[derive(Clone, Copy)]
struct Alt {
    orders: Orders,
    start: u8,
    class: u8,
    state: Hull,
    sum: State,
}

impl Alt {
    fn key(&self) -> (Orders, u8, u8, [u32; FIELDS]) {
        (self.orders, self.start, self.class, self.sum.map(f32::to_bits))
    }

    fn add(&mut self, op: &Op) {
        for state in [&mut self.state.0, &mut self.state.1, &mut self.sum] {
            add_op(op, state);
        }
    }

    fn add_probe(&mut self, value: f32) {
        for state in [&mut self.state.0, &mut self.state.1, &mut self.sum] {
            add_probe(value, state);
        }
    }

    fn check(&self) -> Result<(), Error> {
        finite([&self.state.0, &self.state.1])?;
        finite([&self.sum, &self.sum])
    }
}

/// Applies `op` to the alternatives of its orders, splitting those it applies to in part.
fn apply(alts: &mut Vec<Alt>, op: &Op) -> Result<(), Error> {
    for index in 0..alts.len() {
        let orders = alts[index].orders;
        let applies = orders & op.orders;
        if applies == 0 {
            continue;
        }
        if applies != orders {
            let mut rest = alts[index];
            rest.orders = orders & !applies;
            alts.push(rest);
            alts[index].orders = applies;
        }
        alts[index].add(op);
        alts[index].check()?;
    }
    Ok(())
}

/// Splits the alternatives along the finest partition of their orders, whose sets each alternative's orders are a
/// union of, and merges those of the same set of orders, probe classes and sum into one with the bounds of all.
/// `cells` is a buffer.
fn merge_alts(alts: &mut Vec<Alt>, cells: &mut Vec<Orders>) {
    cells.clear();
    cells.push(alts.iter().fold(0, |orders, alt| orders | alt.orders));
    for alt in alts.iter() {
        refine(cells, alt.orders);
    }
    if cells.len() > 1 {
        for index in 0..alts.len() {
            let alt = alts[index];
            let mut parts = cells.iter().copied().filter(|&cell| cell & alt.orders != 0);
            alts[index].orders = parts.next().expect("an alternative's orders are cells");
            alts.extend(parts.map(|orders| Alt { orders, ..alt }));
        }
    }
    alts.sort_unstable_by_key(Alt::key);
    alts.dedup_by(|later, kept| {
        if later.key() != kept.key() {
            return false;
        }
        widen(&mut kept.state.0, &mut kept.state.1, &later.state.0, &later.state.1);
        true
    });
}

/// Buffers of frame executions.
#[derive(Default)]
struct Scratch {
    alts: Vec<Alt>,
    next: Vec<Alt>,
    kept: Vec<Alt>,
    executed: Vec<(Orders, Classes, Record)>,
    parts: Vec<(Option<usize>, Orders)>,
    cells: Vec<Orders>,
    applied: Vec<Op>,
    merges: Vec<Step>,
    steps: [Vec<Step>; 3],
    values: Vec<NoteValue>,
    merged_values: Vec<NoteValue>,
}

/// The bounds of the native factor histories of every lottery path through the recorder's command and query schedule,
/// in each of one or more orders; see the module documentation.
pub(super) struct RelaxedReplay {
    filed: Filings,
    /// By frame: the chart time of each skill boundary at which a path may switch its probe class, one per filing and
    /// ascending, and the records of the frame's last executions by the orders that made them.
    switches: Vec<Vec<i32>>,
    records: Vec<Vec<(Orders, Record)>>,
    /// The orders of each probe row configuration and its rows by owner, filing order on equal owners.
    configs: Configs,
    /// Whether the orders have probe rows.
    rows: bool,
    /// The bounds of the executed state, partitioning the orders.
    pieces: Pieces,
    lanes: usize,
    /// First frame whose records the replay retains.
    base: i32,
    prev: i32,
    /// The orders whose histories hold the end states of their last executions of frame `prev`.
    last: Orders,
    /// Bit `2 * rush + now` for each pair of probe class after the latest skill boundary and Rush handle a path may
    /// have.
    modes: u8,
    /// Frames in which a path may have filed a Rush command or a probe switch since the previous query.
    potential: Vec<i32>,
    /// [`super::exact_paths::undo_floors`] of the schedule.
    floors: Vec<i32>,
    queries: usize,
    /// The orders the last query may have rewound to each first executed frame.
    firsts: Vec<(i32, Orders)>,
    /// The note executions of the current query.
    observed: Vec<(usize, NoteValue)>,
    scratch: Scratch,
    pub work: ReplayWork,
}

impl RelaxedReplay {
    /// A replay of `lanes` orders, whose probe rows `configs` give by the orders that have them.
    pub(super) fn new(
        frames: usize,
        configs: &[(Orders, Vec<ProbeRow>)],
        floors: Vec<i32>,
        lanes: usize,
    ) -> Result<Self, Error> {
        let (configs, rows) = sorted_configs(configs, lanes)?;
        let mut state = [0f32; FIELDS];
        state[1] = 1.0;
        Ok(Self {
            filed: Filings::new(frames, lanes),
            switches: vec![Vec::new(); frames],
            records: vec![Vec::new(); frames],
            configs,
            rows,
            pieces: vec![(first_orders(lanes), [Some((state, state)), None])],
            lanes,
            base: 0,
            prev: -1,
            last: 0,
            modes: 1,
            potential: Vec::new(),
            floors,
            queries: 0,
            firsts: Vec::new(),
            observed: Vec::new(),
            scratch: Scratch::default(),
            work: ReplayWork::default(),
        })
    }

    /// Without probe rows both probe classes have the same factor history.
    pub(super) fn shared_classes(&self) -> bool {
        !self.rows
    }

    /// The first frame whose records the replay retains: no later query executes an earlier frame.
    pub(super) fn base(&self) -> i32 {
        self.base
    }

    /// The filed notes and the bounds of their last executions.
    pub(super) fn notes(&self) -> &[ExactNote] {
        &self.filed.notes
    }

    /// A note filing; returns the note's index in [`Self::notes`].
    pub(super) fn file_note(
        &mut self,
        frame: usize,
        time_ms: i32,
        note_id: i32,
        key: (usize, usize),
    ) -> Result<usize, Error> {
        self.filed.file_note(frame, time_ms, note_id, key)
    }

    /// A factor command filing in the schedules of `orders` ([`Filings::file_command`]); the Rush commands file
    /// through [`Self::finish`] and [`Self::lotteries`].
    pub(super) fn file_command(&mut self, frame: usize, command: &FactorCommand, orders: Orders) -> Result<(), Error> {
        self.filed.file_command(frame, command, orders)
    }

    /// A range FINISH at the start of a play frame, filing in `frame`: a running Rush stops.
    pub(super) fn finish(&mut self, frame: usize) -> Result<(), Error> {
        if self.modes & RUSHING != 0 {
            self.modes = (self.modes | self.modes >> 2) & !RUSHING;
            self.potential.push(frame as i32);
        }
        Ok(())
    }

    /// The lotteries of one play frame, which may file Rush commands in `frames`. `mask` holds bit
    /// `4 * before + 2 * after + filed` for each Rush transition the lottery law admits; a filing transition files
    /// in any of `frames`.
    pub(super) fn lotteries(&mut self, frames: &[i32], mask: u16) -> Result<(), Error> {
        let (mut next, mut files) = (0u8, false);
        for mode in (0..4u8).filter(|mode| self.modes >> mode & 1 != 0) {
            let (now, rush) = (mode & 1, mode >> 1);
            for bit in (0..4u16).filter(|bit| mask >> (4 * u16::from(rush) + bit) & 1 != 0) {
                next |= 1 << (2 * (bit >> 1) as u8 + now);
                files |= bit & 1 != 0;
            }
        }
        if next == 0 {
            return Err(refuse("no native path follows the certified Rush transitions"));
        }
        if files {
            if frames.is_empty() {
                return Err(refuse("a lottery Rush transition has no filing place"));
            }
            self.potential.extend_from_slice(frames);
        }
        self.modes = next;
        Ok(())
    }

    /// A skill boundary at which every probe row may switch together, filing at `time_ms` in `frame`. `mask` holds
    /// bit `2 * old + new` for each probe class transition the lottery law admits there; with `follows`, the new
    /// probe class equals the Rush.
    pub(super) fn probe(&mut self, frame: usize, time_ms: i32, mask: u8, follows: bool) -> Result<(), Error> {
        if !self.rows {
            return Ok(());
        }
        let (mut next, mut switches) = (0u8, false);
        for mode in (0..4u8).filter(|mode| self.modes >> mode & 1 != 0) {
            let (now, rush) = (mode & 1, mode >> 1);
            for new in [now, 1 - now] {
                if mask >> (2 * now + new) & 1 != 0 && (!follows || new == rush) {
                    next |= 1 << (2 * rush + new);
                    switches |= new != now;
                }
            }
        }
        if next == 0 {
            return Err(refuse("no native path follows the certified probe transitions"));
        }
        if switches {
            let times = self
                .switches
                .get_mut(frame)
                .ok_or_else(|| Error::Input("score frame outside the calculator".into()))?;
            times.insert(times.partition_point(|&time| time <= time_ms), time_ms);
            self.potential.push(frame as i32);
        }
        self.modes = next;
        Ok(())
    }

    /// The native `calculate` up to score frame `to`, with the combo inputs every filed note reads at this query.
    /// Returns each first frame a history may have executed, with the orders of those histories; every order has one.
    pub(super) fn query(
        &mut self,
        to: i32,
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
    ) -> Result<&[(i32, Orders)], Error> {
        let prev = self.prev;
        let filings = self.filed.earliest();
        let floor = self.floors.get(self.queries).copied().unwrap_or(i32::MAX).min(to + 1).max(self.base);
        self.firsts.clear();
        if to == prev
            && filings.iter().all(|&(filing, _)| filing.is_none_or(|frame| frame > prev))
            && self.potential.iter().all(|&frame| frame > prev)
        {
            // Nothing to undo or execute.
            self.work.idle_queries += 1;
            self.firsts.push((prev + 1, first_orders(self.lanes)));
        } else {
            self.execute_query(to, &filings, combos)?;
        }
        // No later query undoes a frame below the floor.
        let frames = self.records.len() as i32;
        for records in &mut self.records[self.base.clamp(0, frames) as usize..floor.clamp(0, frames) as usize] {
            *records = Vec::new();
        }
        self.base = floor;
        self.prev = to;
        self.filed.clear();
        self.potential.clear();
        self.queries += 1;
        Ok(&self.firsts)
    }

    fn execute_query(
        &mut self,
        to: i32,
        filings: &[(Option<i32>, Orders)],
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
    ) -> Result<(), Error> {
        let prev = self.prev;
        let start = |u: i32| if u < prev { u + 1 } else { prev + 1 };
        // The rewind targets of the possible lottery filings, ascending.
        let mut lottery: Vec<i32> = self.potential.iter().map(|&frame| to.min(frame - 1)).collect();
        lottery.sort_unstable();
        lottery.dedup();
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut observed = std::mem::take(&mut self.observed);
        observed.clear();
        let mut next = Vec::with_capacity(self.pieces.len());
        let mut last = 0;
        for &(filing, orders) in filings {
            // Every history of these orders executes from `common` on; some start at one of the earlier `firsts`.
            let shallowest = filing.map_or(to, |frame| to.min(frame - 1));
            let common = start(shallowest);
            let mut firsts: Vec<i32> = lottery.iter().map(|&u| start(u)).filter(|&first| first < common).collect();
            firsts.dedup();
            let lowest = firsts.first().copied().unwrap_or(common);
            if lowest < self.base {
                return Err(refuse("a query undoes a frame below the retained history"));
            }
            // Histories that execute the query's last frame end with their last execution of it; without such an
            // execution only histories that undid nothing keep theirs.
            if common <= to || (to == prev && common == prev + 1) {
                last |= orders & if common <= to { first_orders(self.lanes) } else { self.last };
            }
            for &first in firsts.iter().chain([&common]) {
                match self.firsts.iter_mut().find(|(other, _)| *other == first) {
                    Some((_, all)) => *all |= orders,
                    None => self.firsts.push((first, orders)),
                }
            }
            // `undone[prev + 1 - frame]`: the pieces at the start of `frame`, undone from the executed state.
            let mut undone = vec![
                self.pieces
                    .iter()
                    .filter_map(|&(own, classes)| (own & orders != 0).then_some((own & orders, classes)))
                    .collect::<Pieces>(),
            ];
            for frame in (lowest..=prev).rev() {
                let end = undone.last().expect("the executed state");
                let last = if frame == prev { self.last } else { 0 };
                undone.push(undo(end, &self.records[frame as usize], last, &mut scratch.parts)?);
            }
            let at = |frame: i32| &undone[(prev + 1 - frame) as usize];
            let mut entry = at(common).clone();
            for &first in &firsts {
                let pieces =
                    self.run(at(first).clone(), first, common - 1, false, combos, &mut observed, &mut scratch)?;
                entry = join_pieces(&entry, &pieces, &mut scratch.parts);
            }
            next.extend(self.run(entry, common, to, true, combos, &mut observed, &mut scratch)?);
            self.work.runs += firsts.len() as u64 + 1;
            self.work.undone_frames += (prev + 1 - lowest).max(0) as u64;
        }
        self.pieces = next;
        self.last = last;
        if self.firsts.iter().all(|&(first, _)| first > to) {
            self.work.idle_queries += 1;
        }
        self.filed.observe(&mut observed, &self.firsts, &mut scratch.values, &mut scratch.merged_values)?;
        self.observed = observed;
        self.scratch = scratch;
        Ok(())
    }

    /// Executes `first..=last` from `pieces` at the start of `first`; `all` when every history of their orders
    /// executes those frames. Returns the pieces at the end of `last`.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        mut pieces: Pieces,
        first: i32,
        last: i32,
        all: bool,
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
        observed: &mut Vec<(usize, NoteValue)>,
        scratch: &mut Scratch,
    ) -> Result<Pieces, Error> {
        let mut executed = std::mem::take(&mut scratch.executed);
        for frame in first..=last {
            executed.clear();
            for &(orders, classes) in &pieces {
                self.execute(frame as usize, orders, &classes, combos, observed, &mut executed, scratch)?;
            }
            self.work.peak_paths = self.work.peak_paths.max(executed.len());
            self.work.executed_frames += 1;
            record(&mut self.records[frame as usize], &executed, all, &mut scratch.parts);
            pieces.clear();
            pieces.extend(executed.iter().map(|&(orders, classes, _)| (orders, classes)));
        }
        scratch.executed = executed;
        Ok(pieces)
    }

    /// Executes score frame `frame` for the histories of `orders` with the bounds `classes`; appends each part of the
    /// orders, with the bounds at the frame's end and what the frame records of it, to `out`.
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        frame: usize,
        orders: Orders,
        classes: &Classes,
        combos: &FxHashMap<(usize, usize), (f32, f32)>,
        observed: &mut Vec<(usize, NoteValue)>,
        out: &mut Vec<(Orders, Classes, Record)>,
        scratch: &mut Scratch,
    ) -> Result<(), Error> {
        let entry = self.filed.frame(frame)?;
        let (ops, notes, switches) = (&entry.ops, &entry.notes, &self.switches[frame]);
        let zero = [0f32; FIELDS];
        if ops.is_empty() && switches.is_empty() {
            // The frame keeps every state, records the sum +0, and its undo returns every state.
            for &index in notes {
                let combo = self.combo(index, combos)?;
                for (class, state) in classes.iter().enumerate() {
                    if let &Some((low, high)) = state {
                        observed.push((index, NoteValue { orders, class: class as u8, combo, low, high }));
                    }
                }
            }
            let mut sums: Sums = [[None; 2]; 2];
            for (class, state) in classes.iter().enumerate() {
                sums[class][class] = state.map(|_| (zero, zero));
            }
            out.push((orders, *classes, Record { sums, undo: *classes }));
            return Ok(());
        }
        let mut alts = std::mem::take(&mut scratch.alts);
        alts.clear();
        for (class, state) in classes.iter().enumerate() {
            if let Some(state) = state {
                let class = class as u8;
                alts.push(Alt { orders, start: class, class, state: *state, sum: zero });
            }
        }
        let (mut op, mut switch, mut note) = (0, 0, 0);
        loop {
            let op_time = ops.get(op).map(|o| o.time);
            let switch_time = switches.get(switch).copied();
            let factor_time = match (op_time, switch_time) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let note_time = notes.get(note).map(|&n| self.filed.notes[n].time_ms);
            match (factor_time, note_time) {
                (Some(t), n) if n.is_none_or(|n| n >= t) => {
                    if switch_time == Some(t) {
                        let end = ops[op..].partition_point(|o| o.time == t);
                        self.switch(&mut alts, &ops[op..op + end], scratch)?;
                        op += end;
                        switch += 1;
                    } else {
                        apply(&mut alts, &ops[op])?;
                        op += 1;
                    }
                }
                (_, Some(_)) => {
                    let index = notes[note];
                    let combo = self.combo(index, combos)?;
                    for alt in &alts {
                        let (low, high) = alt.state;
                        observed.push((index, NoteValue { orders: alt.orders, class: alt.class, combo, low, high }));
                    }
                    note += 1;
                }
                (None, None) => break,
                (Some(_), None) => unreachable!("covered by the factor arm"),
            }
        }
        // The alternatives of one set of orders now partition the piece's orders.
        merge_alts(&mut alts, &mut scratch.cells);
        for part in alts.chunk_by(|a, b| a.orders == b.orders) {
            let (mut classes, mut sums, mut undo): (Classes, Sums, Classes) =
                ([None, None], [[None; 2]; 2], [None, None]);
            for alt in part {
                join(&mut classes[usize::from(alt.class)], &alt.state);
                join(&mut sums[usize::from(alt.start)][usize::from(alt.class)], &(alt.sum, alt.sum));
                let (mut low, mut high) = alt.state;
                for field in 0..FIELDS {
                    low[field] -= alt.sum[field];
                    high[field] -= alt.sum[field];
                }
                finite([&low, &high])?;
                join(&mut undo[usize::from(alt.start)], &(low, high));
            }
            out.push((part[0].orders, classes, Record { sums, undo }));
        }
        scratch.alts = alts;
        Ok(())
    }

    /// The combo inputs the note `index` reads at this query, as binary32 bits.
    fn combo(&self, index: usize, combos: &FxHashMap<(usize, usize), (f32, f32)>) -> Result<(u32, u32), Error> {
        let &(ordinary, gekisou) =
            combos.get(&self.filed.notes[index].key).ok_or_else(|| refuse("note combo observation is missing"))?;
        Ok((ordinary.to_bits(), gekisou.to_bits()))
    }

    /// A skill boundary among the ordinary commands `group` of its time at which a history may switch its probe
    /// class: each alternative of `alts` keeps its class and applies the commands, or flips its class with every
    /// owner-local merge of the commands with the probe rows of its orders.
    fn switch(&self, alts: &mut Vec<Alt>, group: &[Op], scratch: &mut Scratch) -> Result<(), Error> {
        let Scratch { next, kept, cells, applied, merges: steps, steps: buffers, .. } = scratch;
        next.clear();
        for alt in alts.iter() {
            kept.clear();
            kept.push(*alt);
            for op in group {
                apply(kept, op)?;
            }
            next.extend_from_slice(kept);
            let sign = if alt.class == 0 { 1f32 } else { -1f32 };
            for (config, rows) in &self.configs {
                // Orders with the same commands of this time.
                cells.clear();
                cells.push(alt.orders & config);
                if cells[0] == 0 {
                    continue;
                }
                for command in group {
                    refine(cells, command.orders);
                }
                for &cell in cells.iter() {
                    // Commands of other orders change nothing here and need no place among the rows.
                    applied.clear();
                    applied.extend(group.iter().filter(|command| command.orders & cell != 0).copied());
                    merges(applied, rows, steps, buffers)?;
                    let width = applied.len() + rows.len();
                    let count = steps.len().checked_div(width).unwrap_or(1);
                    for merge in 0..count {
                        let mut alternative = Alt { orders: cell, ..*alt };
                        for &step in &steps[merge * width..(merge + 1) * width] {
                            match step {
                                Step::Op(i) => alternative.add(&applied[i]),
                                Step::Row(r) => alternative.add_probe(sign * rows[r].value),
                            }
                        }
                        alternative.check()?;
                        alternative.class = 1 - alternative.class;
                        next.push(alternative);
                    }
                }
            }
        }
        merge_alts(next, cells);
        std::mem::swap(alts, next);
        Ok(())
    }
}
