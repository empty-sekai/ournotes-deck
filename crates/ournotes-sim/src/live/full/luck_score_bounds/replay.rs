//! Binary32 enclosures of the native factor state over every lottery path.
//!
//! The native calculator keeps one binary32 value per factor field. Executing a score frame applies the frame's
//! factor commands in (chart time, owner, filing) order, scores its notes in between, and records the binary32 sum
//! of the commands it applied; undoing the frame subtracts that sum. A score query undoes from the last executed
//! frame down to the earliest frame that received a filing since the previous query, then executes up to the
//! query's frame. Lottery paths differ in two ways: the direct score probes file their signed score-up pairs at
//! different frame times, and Rush or probe filings can move the rewind target of a query.
//!
//! Binary32 addition and subtraction rounded to nearest are monotone in each operand, so for operands within
//! binary32 enclosures every result lies between the native results at the enclosure endpoints. The replay keeps
//! one enclosure per probe class (probes off, probes on). All paths of a class have the same real sums of their
//! applied commands, so a class enclosure spans only the rounding differences of their histories, even where it
//! joins paths that switched class at different times. At every possible probe filing a path may switch class; at
//! every query each possible rewind target is replayed and the resulting states are joined. A note keeps the
//! enclosure of its last execution: a frame every path executes replaces it, a frame only some paths execute
//! joins the new execution to it, and a frame's recorded sum is treated the same way.
//!
//! Undoing a frame from separate enclosures of the end state and the recorded sum loses their correlation: both
//! come from the same path. While every path still holds the end state of its last execution of the most recent
//! frame, the undo of that frame is also enclosed path by path: each path's end state and recorded sum are
//! monotone in its start state, so the difference at the start enclosure's endpoints bounds it. The two
//! enclosures of the undo are intersected.

use super::FIELDS;
use crate::error::Error;
use crate::live::certified::F32Interval;
use crate::live::skill::FactorCommand;

/// Enclosures of the six binary32 factor fields: combo, note, Just, Perfect, Great, Good.
pub(crate) type Fields = [F32Interval; FIELDS];
/// Per probe class (0: the 7021 predicate off, 1: on, with every probe row's score-up applied), the enclosure of
/// the paths in that class; None when no path is.
pub(crate) type Classes = [Option<Fields>; 2];

/// A direct score probe row: the owner of its commands and its signed score-up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProbeRow {
    pub owner: i32,
    pub value: f32,
}

fn mill(value: i32) -> f32 {
    if value != 0 { value as f32 / 100000f32 } else { 0f32 }
}

/// The binary32 changes a factor command applies, in field order. A zero change leaves its field unchanged.
pub(crate) fn command_deltas(command: &FactorCommand) -> [f32; FIELDS] {
    let mut out = [0f32; FIELDS];
    out[0] = mill(command.combo_mill);
    out[1] = mill(command.note_mill);
    if (3..=6).contains(&command.judgement) {
        out[8 - command.judgement as usize] = mill(command.judge_mill);
    }
    out
}

fn point(value: f32) -> Result<F32Interval, Error> {
    F32Interval::point(value)
}

fn zero() -> Fields {
    [F32Interval::point(0.0).expect("zero is a binary32 point"); FIELDS]
}

pub(crate) fn initial_state() -> Fields {
    let mut state = zero();
    state[1] = F32Interval::point(1.0).expect("one is a binary32 point");
    state
}

fn hull_fields(a: Fields, b: Fields) -> Fields {
    std::array::from_fn(|field| a[field].hull(b[field]))
}

fn hull(a: Option<Fields>, b: Option<Fields>) -> Option<Fields> {
    match (a, b) {
        (Some(a), Some(b)) => Some(hull_fields(a, b)),
        (a, None) => a,
        (None, b) => b,
    }
}

pub(crate) fn hull_classes(a: Classes, b: Classes) -> Classes {
    [hull(a[0], b[0]), hull(a[1], b[1])]
}

fn add_deltas(fields: &mut Fields, deltas: &[f32; FIELDS]) -> Result<(), Error> {
    for (field, &delta) in deltas.iter().enumerate() {
        if delta != 0.0 {
            fields[field] = fields[field].add(point(delta)?)?;
        }
    }
    Ok(())
}

fn finite(fields: &Fields) -> Result<(), Error> {
    if fields.iter().all(|f| f.lower().is_finite() && f.upper().is_finite()) {
        Ok(())
    } else {
        Err(Error::Unsupported("native LUCK score bounds: a factor state may leave the finite binary32 range".into()))
    }
}

/// One element of a probe group: an ordinary command or a probe row, in native order.
#[derive(Clone, Copy)]
enum Step {
    Command([f32; FIELDS]),
    Probe(f32),
}

#[derive(Clone, Copy)]
struct Op {
    time: i32,
    owner: i32,
    deltas: [f32; FIELDS],
}

#[derive(Clone, Default)]
struct Frame {
    /// Ordinary float commands by (time, owner), equal keys in filing order (the native stable sort).
    ops: Vec<Op>,
    /// Distinct chart times of possible probe filings, ascending.
    probes: Vec<i32>,
    /// Allowed old/new probe-class transitions at each historical probe time.
    probe_masks: Vec<u8>,
    /// Notes by (time, note id).
    notes: Vec<usize>,
    /// Per class at the frame's start, per class at its end: the enclosure of the binary32 sum the last execution
    /// recorded. None until the frame is executed.
    diff: Option<[Classes; 2]>,
    /// Per class at the frame's start: the enclosure of the state an undo returns while every path still holds the
    /// end state of its last execution of this frame. None until the frame is executed.
    undo: Option<Classes>,
}

/// A filed note: its chart time and id, and the enclosure of its last execution's factor state per class.
#[derive(Clone, Debug)]
pub(crate) struct ReplayNote {
    pub time_ms: i32,
    pub note_id: i32,
    pub executed: Classes,
}

/// The native factor state of every lottery path through the recorder's command and query schedule.
pub(crate) struct Replay {
    frames: Vec<Frame>,
    pub notes: Vec<ReplayNote>,
    /// Probe rows by owner, filing order on equal owners.
    rows: Vec<ProbeRow>,
    /// The steps of a probe time without ordinary commands.
    row_steps: Vec<Vec<Step>>,
    state: Classes,
    prev: i32,
    /// Every path holds the end state of its last execution of frame `prev`.
    fresh: bool,
    /// Earliest frame that received a filing every path makes since the previous query.
    mandatory: Option<i32>,
    /// Frames that may have received a lottery-dependent filing since the previous query.
    potential: Vec<i32>,
    shuffle_budget: ShuffleBudget,
}

impl Replay {
    pub(crate) fn new(frames: usize, rows: &[ProbeRow]) -> Self {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|row| row.owner);
        let row_steps = vec![steps(&[], &rows, true)];
        Self {
            frames: vec![Frame::default(); frames],
            notes: Vec::new(),
            rows,
            row_steps,
            state: [Some(initial_state()), None],
            prev: -1,
            fresh: false,
            mandatory: None,
            potential: Vec::new(),
            shuffle_budget: ShuffleBudget::default(),
        }
    }

    fn frame(&mut self, frame: usize) -> Result<&mut Frame, Error> {
        self.frames.get_mut(frame).ok_or_else(|| Error::Input("score frame outside the calculator".into()))
    }

    /// A note filing; returns the note's index in [`Self::notes`].
    pub(crate) fn file_note(&mut self, frame: usize, time_ms: i32, note_id: i32) -> Result<usize, Error> {
        let index = self.notes.len();
        self.notes.push(ReplayNote { time_ms, note_id, executed: [None, None] });
        let notes = &self.notes;
        let entry =
            self.frames.get_mut(frame).ok_or_else(|| Error::Input("note frame outside the calculator".into()))?;
        let at = entry.notes.partition_point(|&n| (notes[n].time_ms, notes[n].note_id) <= (time_ms, note_id));
        entry.notes.insert(at, index);
        self.mandatory = Some(self.mandatory.map_or(frame as i32, |old| old.min(frame as i32)));
        Ok(index)
    }

    /// A factor command filing. A Rush command changes no float field and may differ between paths: it only
    /// moves the rewind target. Every other command is filed by every path.
    pub(crate) fn file_command(&mut self, frame: usize, command: &FactorCommand) -> Result<(), Error> {
        let deltas = command_deltas(command);
        if command.luck != 0 {
            if deltas.iter().any(|&d| d != 0.0) {
                return Err(Error::Unsupported(
                    "native LUCK score bounds: a Rush command changes a float factor".into(),
                ));
            }
            self.potential(frame);
            return Ok(());
        }
        let entry = self.frame(frame)?;
        if deltas.iter().any(|&d| d != 0.0) {
            let key = (command.time_ms, command.owner_id);
            let at = entry.ops.partition_point(|op| (op.time, op.owner) <= key);
            entry.ops.insert(at, Op { time: command.time_ms, owner: command.owner_id, deltas });
        }
        self.mandatory = Some(self.mandatory.map_or(frame as i32, |old| old.min(frame as i32)));
        Ok(())
    }

    /// A possible lottery-dependent filing (Rush) in `frame`.
    pub(crate) fn potential(&mut self, frame: usize) {
        self.potential.push(frame as i32);
    }

    /// A possible probe filing at frame time `time_ms` in `frame`: every probe row may switch on or off there.
    #[cfg(test)]
    pub(crate) fn probe(&mut self, frame: usize, time_ms: i32) -> Result<(), Error> {
        self.probe_with_mask(frame, time_ms, 0b1111).map(|_| ())
    }

    /// File a proof bound to this probe's original skill boundary. Returns whether any admitted path
    /// files a command and can therefore move the rewind target. Historical replays retain this mask.
    pub(crate) fn probe_with_mask(&mut self, frame: usize, time_ms: i32, mask: u8) -> Result<bool, Error> {
        if self.rows.is_empty() {
            return Ok(false);
        }
        if mask == 0 || mask > 0b1111 {
            return Err(Error::Input("invalid probe transition support".into()));
        }
        let entry = self.frame(frame)?;
        let applied_mask = match entry.probes.binary_search(&time_ms) {
            Ok(at) => {
                entry.probe_masks[at] = 0b1111;
                0b1111
            }
            Err(at) => {
                entry.probes.insert(at, time_ms);
                entry.probe_masks.insert(at, mask);
                mask
            }
        };
        let may_file = applied_mask & 0b0110 != 0;
        if may_file {
            self.potential(frame);
        }
        Ok(may_file)
    }

    /// The native `calculate` up to score frame `to`.
    pub(crate) fn query(&mut self, to: i32) -> Result<(), Error> {
        let prev = self.prev;
        let shallowest = self.mandatory.map_or(to, |frame| to.min(frame - 1));
        let mut targets: Vec<i32> =
            self.potential.iter().map(|&frame| to.min(frame - 1)).filter(|&u| u < shallowest).collect();
        targets.push(shallowest);
        targets.sort_unstable();
        targets.dedup();
        let start = |u: i32| if u < prev { u + 1 } else { prev + 1 };
        let common = start(shallowest);
        let lowest = start(targets[0]);
        // States at the start of frames lowest..=prev+1 after undoing from prev.
        let mut undone = vec![[None, None]; (prev + 2 - lowest).max(1) as usize];
        if lowest <= prev {
            let top = (prev + 1 - lowest) as usize;
            undone[top] = self.state;
            for frame in (lowest..=prev).rev() {
                let at = (frame - lowest) as usize;
                undone[at] = self.undo(frame as usize, undone[at + 1], frame == prev && self.fresh)?;
            }
        }
        let at = |frame: i32| (frame - lowest) as usize;
        let mut entry = if common <= prev { undone[at(common)] } else { self.state };
        for &target in &targets {
            let first = start(target);
            if first >= common {
                continue;
            }
            let mut state = undone[at(first)];
            for frame in first..common {
                state = self.execute(frame as usize, state, false)?;
            }
            entry = hull_classes(entry, state);
        }
        let mut state = entry;
        for frame in common..=to {
            state = self.execute(frame as usize, state, true)?;
        }
        // Without a common segment, the paths that replay no frame keep their state; they undid nothing only when
        // the query starts after prev.
        self.fresh = common <= to || (self.fresh && common == prev + 1);
        self.state = state;
        self.prev = to;
        self.mandatory = None;
        self.potential.clear();
        Ok(())
    }

    /// Undo `frame` from `end`. `last` when every path holds the end state of its last execution of the frame.
    fn undo(&self, frame: usize, end: Classes, last: bool) -> Result<Classes, Error> {
        let diff = self.frames[frame].diff.ok_or_else(|| Error::Input("undo of a frame never executed".into()))?;
        let mut out: Classes = [None, None];
        for (from, row) in diff.iter().enumerate() {
            for (to, sum) in row.iter().enumerate() {
                let (Some(sum), Some(state)) = (sum, end[to]) else { continue };
                let mut start = state;
                for field in 0..FIELDS {
                    start[field] = state[field].subtract(sum[field])?;
                }
                finite(&start)?;
                out[from] = hull(out[from], Some(start));
            }
        }
        if last {
            let paired =
                self.frames[frame].undo.ok_or_else(|| Error::Input("undo of a frame never executed".into()))?;
            for (out, paired) in out.iter_mut().zip(paired) {
                *out = match (*out, paired) {
                    (Some(a), Some(b)) => Some(intersect_fields(a, b)?),
                    _ => None,
                };
            }
        }
        Ok(out)
    }

    /// The enclosure, per class at the frame's start, of the state an undo of `frame` returns right after its
    /// execution from `start`. Every path through the frame's probe groups is followed from both endpoints of its
    /// start class with its end state and recorded sum together.
    fn paired_undo(&mut self, frame: usize, start: &Classes) -> Result<Classes, Error> {
        let entry = &self.frames[frame];
        let (ops, probes) = (&entry.ops, &entry.probes);
        let mut paths = Vec::with_capacity(8);
        for (class, fields) in start.iter().enumerate() {
            let Some(fields) = fields else { continue };
            for state in [fields.map(|f| f.lower()), fields.map(|f| f.upper())] {
                paths.push(PairedPath { start: class, class, state, sum: [0.0; FIELDS] });
            }
        }
        dedup_paths(&mut paths);
        let (mut op, mut probe) = (0, 0);
        loop {
            let op_time = ops.get(op).map(|o| o.time);
            let probe_time = probes.get(probe).copied();
            match (op_time, probe_time) {
                (None, None) => break,
                (Some(a), b) if b.is_none_or(|b| a < b) => {
                    for path in &mut paths {
                        path.apply(&[Step::Command(ops[op].deltas)], 1.0);
                    }
                    op += 1;
                }
                (_, Some(t)) => {
                    let end = ops[op..].partition_point(|o| o.time == t);
                    let group = &ops[op..op + end];
                    let mask = entry.probe_masks[probe];
                    let alternatives = if mask & 0b0110 == 0 {
                        Some(Vec::new())
                    } else if group.is_empty() {
                        None
                    } else {
                        Some(variants(group, &self.rows, &mut self.shuffle_budget)?)
                    };
                    let alternatives = alternatives.as_deref().unwrap_or(&self.row_steps);
                    let stays = paths.iter().filter(|path| mask & (1 << (3 * path.class)) != 0).count();
                    let switches = paths.iter().filter(|path| mask & (1 << (path.class + 1)) != 0).count();
                    let next_count = stays
                        .checked_add(shuffle_product(switches, alternatives.len())?)
                        .ok_or_else(shuffle_capacity)?;
                    if next_count > MAX_PAIRED_PATHS {
                        return Err(shuffle_capacity());
                    }
                    let variant_steps = alternatives
                        .iter()
                        .try_fold(0usize, |total, steps| total.checked_add(steps.len()).ok_or_else(shuffle_capacity))?;
                    // The unique-order execution is ordinary replay work; charge only additional interleavings.
                    let extra_steps = variant_steps - alternatives.first().map_or(0, Vec::len);
                    let work = shuffle_product(switches, extra_steps)?;
                    self.shuffle_budget.charge(work)?;
                    let stay: Vec<Step> = group.iter().map(|op| Step::Command(op.deltas)).collect();
                    let mut next = Vec::new();
                    next.try_reserve_exact(next_count).map_err(|_| shuffle_capacity())?;
                    for path in &paths {
                        if mask & (1 << (3 * path.class)) != 0 {
                            let mut kept = *path;
                            kept.apply(&stay, 1.0);
                            next.push(kept);
                        }
                        if mask & (1 << (path.class + 1)) != 0 {
                            for steps in alternatives {
                                let mut switched = *path;
                                switched.apply(steps, if path.class == 0 { 1.0 } else { -1.0 });
                                switched.class = 1 - path.class;
                                next.push(switched);
                            }
                        }
                    }
                    dedup_paths(&mut next);
                    paths = next;
                    op += end;
                    probe += 1;
                }
                (Some(_), None) => unreachable!("covered by the command arm"),
            }
        }
        let mut out: Classes = [None, None];
        for path in &paths {
            let mut fields = zero();
            for (field, out) in fields.iter_mut().enumerate() {
                *out = point(path.state[field] - path.sum[field])?;
            }
            finite(&fields)?;
            out[path.start] = hull(out[path.start], Some(fields));
        }
        Ok(out)
    }

    /// Execute `frame` from `state` (per class at its start). `all` when every path executes it.
    fn execute(&mut self, frame: usize, mut state: Classes, all: bool) -> Result<Classes, Error> {
        let paired = self.paired_undo(frame, &state)?;
        let entry = &self.frames[frame];
        let (ops, probes, notes) = (&entry.ops, &entry.probes, &entry.notes);
        // Per class at the frame's start: the running binary32 sum of the applied commands, per current class.
        let mut sums: [Classes; 2] = [[state[0].map(|_| zero()), None], [None, state[1].map(|_| zero())]];
        let (mut op, mut probe, mut note) = (0, 0, 0);
        let mut executed: Vec<(usize, Classes)> = Vec::with_capacity(notes.len());
        loop {
            let op_time = ops.get(op).map(|o| o.time);
            let probe_time = probes.get(probe).copied();
            let factor_time = match (op_time, probe_time) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let note_time = notes.get(note).map(|&n| self.notes[n].time_ms);
            match (factor_time, note_time) {
                (Some(t), n) if n.is_none_or(|n| n >= t) => {
                    if probe_time == Some(t) {
                        let end = ops[op..].partition_point(|o| o.time == t);
                        let group = &ops[op..op + end];
                        let mask = entry.probe_masks[probe];
                        let variants = if mask & 0b0110 == 0 {
                            Some(Vec::new())
                        } else if group.is_empty() {
                            None
                        } else {
                            Some(variants(group, &self.rows, &mut self.shuffle_budget)?)
                        };
                        let variants = variants.as_deref().unwrap_or(&self.row_steps);
                        state = transition(state, group, variants, mask)?;
                        for row in &mut sums {
                            *row = transition(*row, group, variants, mask)?;
                        }
                        op += end;
                        probe += 1;
                    } else {
                        let deltas = ops[op].deltas;
                        for class in state.iter_mut().chain(sums.iter_mut().flatten()).flatten() {
                            add_deltas(class, &deltas)?;
                            finite(class)?;
                        }
                        op += 1;
                    }
                }
                (_, Some(_)) => {
                    // The probe class follows the 7021 predicate; without probe rows both classes are one state.
                    executed.push((notes[note], if self.rows.is_empty() { [state[0], state[0]] } else { state }));
                    note += 1;
                }
                (None, None) => break,
                (Some(_), None) => unreachable!("covered by the first arm"),
            }
        }
        for (index, classes) in executed {
            let note = &mut self.notes[index];
            note.executed = if all { classes } else { hull_classes(note.executed, classes) };
        }
        let entry = &mut self.frames[frame];
        entry.diff = Some(match (all, entry.diff) {
            (false, Some(old)) => [hull_classes(old[0], sums[0]), hull_classes(old[1], sums[1])],
            _ => sums,
        });
        entry.undo = Some(match (all, entry.undo) {
            (false, Some(old)) => hull_classes(old, paired),
            _ => paired,
        });
        Ok(state)
    }
}

/// One path through a frame from a start endpoint: its start class, current class, binary32 state and the binary32
/// sum of the commands it applied.
#[derive(Clone, Copy, PartialEq)]
struct PairedPath {
    start: usize,
    class: usize,
    state: [f32; FIELDS],
    sum: [f32; FIELDS],
}

impl PairedPath {
    fn apply(&mut self, steps: &[Step], sign: f32) {
        for step in steps {
            match *step {
                Step::Command(deltas) => {
                    for (field, &delta) in deltas.iter().enumerate() {
                        if delta != 0.0 {
                            self.state[field] += delta;
                            self.sum[field] += delta;
                        }
                    }
                }
                Step::Probe(value) => {
                    if value != 0.0 {
                        self.state[1] += sign * value;
                        self.sum[1] += sign * value;
                    }
                }
            }
        }
    }

    fn key(&self) -> (usize, usize, [u32; FIELDS], [u32; FIELDS]) {
        (self.start, self.class, self.state.map(f32::to_bits), self.sum.map(f32::to_bits))
    }
}

fn dedup_paths(paths: &mut Vec<PairedPath>) {
    paths.sort_unstable_by_key(PairedPath::key);
    paths.dedup_by_key(|path| path.key());
}

fn intersect_fields(a: Fields, b: Fields) -> Result<Fields, Error> {
    let mut out = a;
    for (out, (a, b)) in out.iter_mut().zip(a.iter().zip(b)) {
        *out = F32Interval::new(a.lower().max(b.lower()), a.upper().min(b.upper()))
            .map_err(|_| Error::Input("disjoint enclosures of one native undo".into()))?;
    }
    Ok(out)
}

/// The commands at one probe time: ordinary commands of that time and the probe rows, ordered by owner. A path
/// that keeps its class applies only the ordinary commands; a path that switches on (off) also applies every row's
/// score-up (its negation) at its place in one of `variants`.
fn transition(state: Classes, group: &[Op], variants: &[Vec<Step>], mask: u8) -> Result<Classes, Error> {
    let mut stay = state;
    for (class, value) in stay.iter_mut().enumerate() {
        if mask & (1 << (3 * class)) == 0 {
            *value = None;
        }
    }
    for op in group {
        for class in stay.iter_mut().flatten() {
            add_deltas(class, &op.deltas)?;
        }
    }
    let mut on = None;
    let mut off = None;
    for steps in variants {
        if mask & 0b0010 != 0 {
            on = hull(on, state[0].map(|s| apply_steps(s, steps, 1.0)).transpose()?);
        }
        if mask & 0b0100 != 0 {
            off = hull(off, state[1].map(|s| apply_steps(s, steps, -1.0)).transpose()?);
        }
    }
    let out = [hull(stay[0], off), hull(stay[1], on)];
    for class in out.iter().flatten() {
        finite(class)?;
    }
    Ok(out)
}

const MAX_SHUFFLE_VARIANTS: usize = 4096;
const MAX_SHUFFLE_STORAGE: usize = 131072;
const MAX_PAIRED_PATHS: usize = 4096;
const SHUFFLE_WORK: usize = 4_000_000;

fn shuffle_capacity() -> Error {
    Error::Capacity("native score probe-order enclosure exceeds its complete-work budget".into())
}

struct ShuffleBudget {
    remaining: usize,
}

impl Default for ShuffleBudget {
    fn default() -> Self {
        Self { remaining: SHUFFLE_WORK }
    }
}

impl ShuffleBudget {
    fn charge(&mut self, steps: usize) -> Result<(), Error> {
        self.remaining = self.remaining.checked_sub(steps).ok_or_else(shuffle_capacity)?;
        Ok(())
    }
}

fn shuffle_product(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or_else(shuffle_capacity)
}

fn shuffle_choose(a: usize, b: usize) -> Result<usize, Error> {
    let total = a.checked_add(b).ok_or_else(shuffle_capacity)?;
    let k = a.min(b);
    let mut count = 1;
    for i in 1..=k {
        count = shuffle_product(count, total - k + i)? / i;
        if count > MAX_SHUFFLE_VARIANTS {
            return Err(shuffle_capacity());
        }
    }
    Ok(count)
}

/// Every owner-local merge that preserves the recorded ordinary order and the proved probe order.
/// Owners remain in native score order; their independent interleavings form a Cartesian product.
fn variants(group: &[Op], rows: &[ProbeRow], budget: &mut ShuffleBudget) -> Result<Vec<Vec<Step>>, Error> {
    let length = group.len().checked_add(rows.len()).ok_or_else(shuffle_capacity)?;
    let (mut ordinary_at, mut probe_at) = (0, 0);
    let mut tied = false;
    while let (Some(op), Some(row)) = (group.get(ordinary_at), rows.get(probe_at)) {
        match op.owner.cmp(&row.owner) {
            std::cmp::Ordering::Less => ordinary_at += 1,
            std::cmp::Ordering::Greater => probe_at += 1,
            std::cmp::Ordering::Equal => {
                tied = true;
                break;
            }
        }
    }
    // A unique native merge has no combinatorial expansion or shuffle-budget cost.
    if !tied {
        return Ok(vec![steps(group, rows, true)]);
    }
    let (mut g, mut r, mut count) = (0, 0, 1);
    while g < group.len() || r < rows.len() {
        let owner = match (group.get(g), rows.get(r)) {
            (Some(op), Some(row)) => op.owner.min(row.owner),
            (Some(op), None) => op.owner,
            (None, Some(row)) => row.owner,
            (None, None) => unreachable!("a group item remains"),
        };
        let ordinary = group[g..].partition_point(|op| op.owner == owner);
        let probes = rows[r..].partition_point(|row| row.owner == owner);
        count = shuffle_product(count, shuffle_choose(ordinary, probes)?)?;
        if count > MAX_SHUFFLE_VARIANTS {
            return Err(shuffle_capacity());
        }
        g += ordinary;
        r += probes;
    }
    let storage = shuffle_product(count, length)?;
    if storage > MAX_SHUFFLE_STORAGE {
        return Err(shuffle_capacity());
    }
    budget.charge(storage - length)?;
    let mut out = Vec::new();
    out.try_reserve_exact(count).map_err(|_| shuffle_capacity())?;
    let mut pending = Vec::new();
    pending.try_reserve_exact(count).map_err(|_| shuffle_capacity())?;
    pending.push((0usize, 0usize, Vec::new()));
    while let Some((g, r, mut path)) = pending.pop() {
        match (group.get(g), rows.get(r)) {
            (None, None) => out.push(path),
            (Some(op), Some(row)) if op.owner == row.owner => {
                let mut with_probe = Vec::new();
                with_probe.try_reserve_exact(length).map_err(|_| shuffle_capacity())?;
                with_probe.extend_from_slice(&path);
                with_probe.push(Step::Probe(row.value));
                pending.push((g, r + 1, with_probe));
                path.try_reserve(1).map_err(|_| shuffle_capacity())?;
                path.push(Step::Command(op.deltas));
                pending.push((g + 1, r, path));
            }
            (Some(op), row) if row.is_none_or(|row| op.owner < row.owner) => {
                path.try_reserve(1).map_err(|_| shuffle_capacity())?;
                path.push(Step::Command(op.deltas));
                pending.push((g + 1, r, path));
            }
            (_, Some(row)) => {
                path.try_reserve(1).map_err(|_| shuffle_capacity())?;
                path.push(Step::Probe(row.value));
                pending.push((g, r + 1, path));
            }
            (Some(_), None) => unreachable!("ordinary-only case"),
        }
    }
    debug_assert_eq!(out.len(), count);
    Ok(out)
}

fn steps(group: &[Op], rows: &[ProbeRow], rows_first: bool) -> Vec<Step> {
    let mut out = Vec::with_capacity(group.len() + rows.len());
    let (mut g, mut r) = (0, 0);
    while g < group.len() || r < rows.len() {
        let take_row = match (group.get(g), rows.get(r)) {
            (Some(op), Some(row)) => row.owner < op.owner || (row.owner == op.owner && rows_first),
            (None, Some(_)) => true,
            _ => false,
        };
        if take_row {
            out.push(Step::Probe(rows[r].value));
            r += 1;
        } else {
            out.push(Step::Command(group[g].deltas));
            g += 1;
        }
    }
    out
}

fn apply_steps(mut fields: Fields, steps: &[Step], sign: f32) -> Result<Fields, Error> {
    for step in steps {
        match *step {
            Step::Command(deltas) => add_deltas(&mut fields, &deltas)?,
            Step::Probe(value) => {
                if value != 0.0 {
                    fields[1] = fields[1].add(point(sign * value)?)?;
                }
            }
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::full::combo::ComboCounter;
    use crate::live::full::scorecalc::{IncrementalCalculator, NoteCommand};
    use crate::live::score::{LiveScoreCalculator, LiveScoreSettings, get_frame};

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn chance(&mut self, percent: u64) -> bool {
            self.below(100) < percent
        }
    }

    #[derive(Clone, Copy)]
    enum Step {
        Note(NoteCommand),
        Command(FactorCommand),
        /// A frame time where every probe row may switch.
        Probe(i32),
        /// A chart time where a Rush command may be filed.
        Rush(i32),
        Query(i32),
    }

    const MUSIC_MS: i32 = 1000;
    const ROWS: [(i32, i32); 2] = [(101, 50_000), (201, 80_000)];

    fn calculator() -> IncrementalCalculator {
        let settings = LiveScoreSettings {
            score_adjustment_factor: 3.0,
            life_onus_factor: 0.5,
            note_factor_percent: [(1, 100)].into(),
            judgement_score_factor_percent: [(1, 100), (2, 90), (3, 70), (4, 50)].into(),
        };
        IncrementalCalculator::new(LiveScoreCalculator::new(100_000, 20, 64, &settings, 1.0, 1.0, None), MUSIC_MS)
    }

    fn schedule(rng: &mut Rng) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut note_id = 0;
        let mut t = 0;
        while t < MUSIC_MS - 100 {
            if rng.chance(25) {
                let note = [12_345, -12_345, 50_000, 3_333, -7_777, 0][rng.below(6) as usize];
                let combo = [0, 1_200, -1_200, 4_321][rng.below(4) as usize];
                steps.push(Step::Command(FactorCommand {
                    time_ms: (t - rng.below(120) as i32).max(0),
                    // Owner 101 also holds a probe row: equal keys have no recorded filing order.
                    owner_id: [1, 101, 150, 300][rng.below(4) as usize],
                    note_mill: note,
                    combo_mill: combo,
                    judgement: 3 + rng.below(4) as i32,
                    judge_mill: [0, 777, -777, 20_000][rng.below(4) as usize],
                    ..Default::default()
                }));
            }
            if rng.chance(50) {
                let time = (t - rng.below(30) as i32).max(0);
                steps.push(Step::Note(NoteCommand::new(time, 100, note_id, 1, 1 + rng.below(4) as i32)));
                note_id += 1;
            }
            if rng.chance(30) {
                steps.push(Step::Rush((t - rng.below(40) as i32).max(0)));
            }
            steps.push(Step::Probe(t));
            steps.push(Step::Query(t));
            if rng.chance(5) {
                // A solo rank snapshot rewinds the calculator to an earlier time.
                steps.push(Step::Query((t - rng.below(200) as i32).max(0)));
            }
            t += 13;
        }
        steps.push(Step::Query(MUSIC_MS));
        steps
    }

    fn frame_of(time: i32, frames: usize) -> usize {
        (get_frame(time).max(0) as usize).min(frames - 1)
    }

    /// Every lottery path's native factor state at each note's last execution lies in the replay's enclosure of the
    /// note's probe class at its chart time.
    #[test]
    fn replay_encloses_every_path_through_probe_switches_rush_rewinds_and_rank_rewinds() {
        let rows: Vec<_> = ROWS.iter().map(|&(owner, m)| ProbeRow { owner, value: m as f32 / 100000f32 }).collect();
        let mut checked = 0;
        let mut widest = 0f32;
        for seed in 1..=12u64 {
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x2545_f491_4f6c_dd1d));
            let steps = schedule(&mut rng);
            let frames = calculator().executed_states().0;
            let mut replay = Replay::new(frames, &rows);
            let mut index = Vec::new();
            for step in &steps {
                match *step {
                    Step::Note(note) => index
                        .push(replay.file_note(frame_of(note.time_ms, frames), note.time_ms, note.note_id).unwrap()),
                    Step::Command(command) => replay.file_command(frame_of(command.time_ms, frames), &command).unwrap(),
                    Step::Probe(t) => replay.probe(frame_of(t, frames), t).unwrap(),
                    Step::Rush(t) => replay.potential(frame_of(t, frames)),
                    Step::Query(t) => replay.query(frame_of(t, frames) as i32).unwrap(),
                }
            }
            for _path in 0..40 {
                let mut native = calculator();
                let combo = ComboCounter::new(64);
                let mut on = false;
                let mut switches = Vec::new();
                for step in &steps {
                    match *step {
                        Step::Note(note) => native.add_note(note),
                        Step::Command(command) => native.add_factor(command),
                        Step::Probe(t) => {
                            if rng.chance(30) {
                                on = !on;
                                switches.push(t);
                                for &(owner, m) in &ROWS {
                                    let note_mill = if on { m } else { -m };
                                    native.add_factor(FactorCommand {
                                        time_ms: t,
                                        owner_id: owner,
                                        note_mill,
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                        Step::Rush(t) => {
                            if rng.chance(50) {
                                native.add_factor(FactorCommand {
                                    time_ms: t,
                                    owner_id: -1,
                                    luck: 10,
                                    ..Default::default()
                                });
                            }
                        }
                        Step::Query(t) => {
                            native.calculate(t, &combo, None).unwrap();
                        }
                    }
                }
                for (note_id, state) in native.executed_states().1 {
                    let note = &replay.notes[index[note_id as usize]];
                    let class = switches.iter().filter(|&&t| t <= note.time_ms).count() % 2;
                    let enclosure = note.executed[class].expect("every path class is reachable");
                    for (field, value) in state.into_iter().enumerate() {
                        assert!(
                            enclosure[field].contains(value),
                            "seed {seed} note {note_id} class {class} field {field}: {value} outside {:?}",
                            enclosure[field]
                        );
                        widest = widest.max(enclosure[field].upper() - enclosure[field].lower());
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 10_000, "{checked}");
        // Rounding differences only: the classes never mix their real sums.
        assert!(widest < 1e-5, "{widest}");
    }

    #[test]
    fn probe_mask_history_encloses_every_allowed_native_path_through_late_and_optional_rewinds() {
        let times = [1, 13, 27, 41, 53, 67];
        let masks = [1u8, 3, 15, 12, 9, 5];
        let combo = ComboCounter::new(64);
        let mut checked = 0;
        for probe_mill in [-80000, 80000] {
            let rows = [ProbeRow { owner: 101, value: probe_mill as f32 / 100000f32 }];
            for switches in 0usize..64 {
                let mut class = 0;
                let mut valid = true;
                for (index, &mask) in masks.iter().enumerate() {
                    let next = class ^ ((switches >> index) & 1);
                    valid &= mask & (1 << (2 * class + next)) != 0;
                    class = next;
                }
                if !valid {
                    continue;
                }
                for filing in 0usize..64 {
                    for rush in [false, true] {
                        let mut native = calculator();
                        let mut replay = Replay::new(native.executed_states().0, &rows);
                        let mut class = false;
                        let mut changed = Vec::new();
                        for (index, &time) in times.iter().enumerate() {
                            let command = FactorCommand {
                                time_ms: time,
                                owner_id: 101,
                                note_mill: [333333, 1000000, -12345, 3333, -7777, 4321][index],
                                combo_mill: [1200, -1200, 4321, 0, -1200, 1200][index],
                                judgement: 3 + index as i32 % 4,
                                judge_mill: if index % 2 == 0 { 777 } else { -777 },
                                ..Default::default()
                            };
                            let change = switches >> index & 1 != 0;
                            let probe = FactorCommand {
                                time_ms: time,
                                owner_id: 101,
                                note_mill: if class { -probe_mill } else { probe_mill },
                                ..Default::default()
                            };
                            if change && filing >> index & 1 != 0 {
                                native.add_factor(probe);
                            }
                            native.add_factor(command);
                            if change && filing >> index & 1 == 0 {
                                native.add_factor(probe);
                            }
                            if change {
                                class = !class;
                                changed.push(time);
                            }
                            let frame = frame_of(time, replay.frames.len());
                            replay.file_command(frame, &command).unwrap();
                            replay.probe_with_mask(frame, time, masks[index]).unwrap();
                        }
                        for (id, time) in [8, 48].into_iter().enumerate() {
                            native.add_note(NoteCommand::new(time, 1000, id as i32, 1, 2));
                            replay.file_note(frame_of(time, replay.frames.len()), time, id as i32).unwrap();
                        }
                        for (index, time) in [79, 79, 79, 39, 79, 0, 79].into_iter().enumerate() {
                            if index == 1 {
                                replay.potential(1);
                                if rush {
                                    native.add_factor(FactorCommand { time_ms: 1, luck: 10, ..Default::default() });
                                }
                            }
                            if index == 2 {
                                let late =
                                    FactorCommand { time_ms: 1, owner_id: 3, note_mill: -12345, ..Default::default() };
                                native.add_factor(late);
                                replay.file_command(1, &late).unwrap();
                            }
                            native.calculate(time, &combo, None).unwrap();
                            replay.query(get_frame(time)).unwrap();
                            for (id, state) in native.executed_states().1 {
                                let note = &replay.notes[id as usize];
                                let class = changed.iter().filter(|&&time| time <= note.time_ms).count() % 2;
                                let bounds = note.executed[class].unwrap();
                                for (field, value) in state.into_iter().enumerate() {
                                    assert!(
                                        bounds[field].contains(value),
                                        "switches={switches} filing={filing} rush={rush} query={index} note={id} field={field}"
                                    );
                                }
                            }
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 1024);
    }

    #[test]
    fn probe_mask_identity_elides_only_probe_rewinds_and_collision_restores_full_support() {
        let rows = [ProbeRow { owner: 101, value: -0.8 }];
        let mut replay = Replay::new(4, &rows);
        assert!(!replay.probe_with_mask(1, 13, 1).unwrap());
        assert!(replay.potential.is_empty());
        replay.potential(0);
        assert_eq!(replay.potential, [0]);
        assert!(replay.probe_with_mask(1, 13, 8).unwrap());
        assert_eq!(replay.frames[1].probe_masks, [15]);
        assert_eq!(replay.potential, [0, 1]);
        assert!(replay.probe_with_mask(1, 27, 2).unwrap());
        assert_eq!(replay.mandatory, None);
    }

    fn owner_shuffle_op(owner: i32, delta: f32) -> Op {
        let mut deltas = [0.0; FIELDS];
        deltas[1] = delta;
        Op { time: 13, owner, deltas }
    }

    fn owner_shuffle_words(steps: &[super::Step]) -> Vec<u32> {
        steps
            .iter()
            .map(|step| match step {
                super::Step::Command(values) => values[1].to_bits(),
                super::Step::Probe(value) => value.to_bits(),
            })
            .collect()
    }

    #[test]
    fn owner_shuffle_combines_every_independent_owner_interleaving() {
        let group = [owner_shuffle_op(1, 0.3), owner_shuffle_op(2, 0.1), owner_shuffle_op(2, 0.2)];
        let rows = [
            ProbeRow { owner: 1, value: 0.0001 },
            ProbeRow { owner: 1, value: 0.0002 },
            ProbeRow { owner: 2, value: 0.8 },
        ];
        let actual: std::collections::BTreeSet<_> = variants(&group, &rows, &mut ShuffleBudget::default())
            .unwrap()
            .iter()
            .map(|steps| owner_shuffle_words(steps))
            .collect();
        let mut expected = std::collections::BTreeSet::new();
        for a in [[0.3f32, 0.0001, 0.0002], [0.0001, 0.3, 0.0002], [0.0001, 0.0002, 0.3]] {
            for b in [[0.1f32, 0.2, 0.8], [0.1, 0.8, 0.2], [0.8, 0.1, 0.2]] {
                expected.insert(a.into_iter().chain(b).map(f32::to_bits).collect::<Vec<_>>());
            }
        }
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 9);
    }

    #[test]
    fn owner_shuffle_signed_native_execution_and_paired_undo_remain_enclosed() {
        let combo = ComboCounter::new(64);
        let mut checked = 0;
        for probe_mill in [-10, 10] {
            for ordinary_mill in [-30000, 30000] {
                for undo_probe in [false, true] {
                    let value = mill(probe_mill);
                    let rows = [ProbeRow { owner: 101, value }, ProbeRow { owner: 101, value }];
                    let group = [owner_shuffle_op(101, mill(ordinary_mill))];
                    let programs = variants(&group, &rows, &mut ShuffleBudget::default()).unwrap();
                    assert_eq!(programs.len(), 3);
                    let mut replay = Replay::new(calculator().executed_states().0, &rows);
                    if undo_probe {
                        replay.probe_with_mask(0, 0, 0b0010).unwrap();
                    }
                    replay
                        .file_command(
                            1,
                            &FactorCommand {
                                time_ms: 13,
                                owner_id: 101,
                                note_mill: ordinary_mill,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                    replay.probe_with_mask(1, 13, if undo_probe { 0b0100 } else { 0b0010 }).unwrap();
                    replay.file_note(1, 14, 0).unwrap();
                    replay.query(1).unwrap();
                    let end = replay.notes[0].executed[usize::from(!undo_probe)].unwrap();
                    replay.query(0).unwrap();
                    let undone = replay.state[usize::from(undo_probe)].unwrap();
                    replay.query(1).unwrap();
                    let repeated = replay.notes[0].executed[usize::from(!undo_probe)].unwrap();
                    for program in programs {
                        let mut native = calculator();
                        if undo_probe {
                            for _ in 0..2 {
                                native.add_factor(FactorCommand {
                                    time_ms: 0,
                                    owner_id: 101,
                                    note_mill: probe_mill,
                                    ..Default::default()
                                });
                            }
                        }
                        for step in program {
                            let note_mill = match step {
                                super::Step::Command(_) => ordinary_mill,
                                super::Step::Probe(_) => {
                                    if undo_probe {
                                        -probe_mill
                                    } else {
                                        probe_mill
                                    }
                                }
                            };
                            native.add_factor(FactorCommand {
                                time_ms: 13,
                                owner_id: 101,
                                note_mill,
                                ..Default::default()
                            });
                        }
                        native.add_note(NoteCommand::new(14, 1000, 0, 1, 2));
                        native.calculate(39, &combo, None).unwrap();
                        assert!(end[1].contains(native.calc.state.note_score_up));
                        native.calculate(0, &combo, None).unwrap();
                        assert!(undone[1].contains(native.calc.state.note_score_up));
                        native.calculate(39, &combo, None).unwrap();
                        assert!(repeated[1].contains(native.calc.state.note_score_up));
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 24);
    }

    #[test]
    fn owner_shuffle_identity_edges_do_not_build_unreachable_variants() {
        let rows = [ProbeRow { owner: 1, value: 0.0001 }; 10];
        let mut replay = Replay::new(3, &rows);
        replay.shuffle_budget.remaining = 0;
        replay
            .file_command(1, &FactorCommand { time_ms: 13, owner_id: 1, note_mill: 30000, ..Default::default() })
            .unwrap();
        replay.probe_with_mask(1, 13, 0b0001).unwrap();
        replay.file_note(1, 14, 0).unwrap();
        replay.query(1).unwrap();
        assert_eq!(replay.notes[0].executed[0].unwrap()[1].lower().to_bits(), 1.3f32.to_bits());
        assert_eq!(replay.shuffle_budget.remaining, 0);
    }

    #[test]
    fn owner_shuffle_checked_limits_refuse_complete_work_without_committing_partial_frames() {
        let mut group: Vec<_> = (0..12).map(|owner| owner_shuffle_op(owner, 0.3)).collect();
        let mut rows: Vec<_> = (0..12).map(|owner| ProbeRow { owner, value: 0.0001 }).collect();
        assert_eq!(variants(&group, &rows, &mut ShuffleBudget::default()).unwrap().len(), MAX_SHUFFLE_VARIANTS);
        let mut replay = Replay::new(3, &rows);
        for op in &group {
            replay
                .file_command(
                    1,
                    &FactorCommand { time_ms: op.time, owner_id: op.owner, note_mill: 30000, ..Default::default() },
                )
                .unwrap();
        }
        replay.probe(1, 13).unwrap();
        replay.file_note(1, 14, 0).unwrap();
        for _ in 0..2 {
            assert!(matches!(replay.query(1), Err(Error::Capacity(_))));
            assert!(replay.frames[1].diff.is_none() && replay.frames[1].undo.is_none());
            assert!(replay.notes[0].executed.iter().all(Option::is_none));
            assert_eq!(replay.prev, -1);
        }
        group.push(owner_shuffle_op(12, 0.3));
        rows.push(ProbeRow { owner: 12, value: 0.0001 });
        assert!(matches!(variants(&group, &rows, &mut ShuffleBudget::default()), Err(Error::Capacity(_))));
        assert!(matches!(shuffle_choose(usize::MAX, 1), Err(Error::Capacity(_))));
        assert!(matches!(
            variants(&group[..1], &rows[..1], &mut ShuffleBudget { remaining: 0 }),
            Err(Error::Capacity(_))
        ));
    }

    #[test]
    fn owner_shuffle_unique_long_merges_and_ordinary_replay_need_no_shuffle_credit() {
        let group: Vec<_> = (0..900).map(|index| owner_shuffle_op(index * 2 + 1, 0.3)).collect();
        let rows: Vec<_> = (0..900).map(|index| ProbeRow { owner: index * 2, value: 0.0001 }).collect();
        let mut budget = ShuffleBudget { remaining: 0 };
        let programs = variants(&group, &rows, &mut budget).unwrap();
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].len(), 1800);
        assert!(
            programs[0]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| matches!(pair, [super::Step::Probe(_), super::Step::Command(_)]))
        );
        assert_eq!(budget.remaining, 0);
        let mut replay = Replay::new(3, &rows);
        replay.shuffle_budget.remaining = 0;
        let mut native = calculator();
        for (op, row) in group.iter().zip(&rows) {
            let command = FactorCommand { time_ms: 13, owner_id: op.owner, note_mill: 30000, ..Default::default() };
            replay.file_command(1, &command).unwrap();
            native.add_factor(command);
            native.add_factor(FactorCommand { time_ms: 13, owner_id: row.owner, note_mill: 10, ..Default::default() });
        }
        replay.probe_with_mask(1, 13, 0b0010).unwrap();
        replay.file_note(1, 14, 0).unwrap();
        native.add_note(NoteCommand::new(14, 1000, 0, 1, 2));
        let combo = ComboCounter::new(64);
        replay.query(1).unwrap();
        native.calculate(39, &combo, None).unwrap();
        assert!(replay.notes[0].executed[1].unwrap()[1].contains(native.calc.state.note_score_up));
        replay.query(0).unwrap();
        native.calculate(0, &combo, None).unwrap();
        assert!(replay.state[0].unwrap()[1].contains(native.calc.state.note_score_up));
        assert_eq!(replay.shuffle_budget.remaining, 0);

        let mut ordinary = Replay::new(302, &[]);
        ordinary.shuffle_budget.remaining = 0;
        for frame in 1..=300 {
            ordinary
                .file_command(
                    frame,
                    &FactorCommand { time_ms: frame as i32 * 40, owner_id: 1, note_mill: 10, ..Default::default() },
                )
                .unwrap();
            ordinary.query(frame as i32).unwrap();
        }
        ordinary.query(0).unwrap();
        assert_eq!(ordinary.shuffle_budget.remaining, 0);
    }
}
