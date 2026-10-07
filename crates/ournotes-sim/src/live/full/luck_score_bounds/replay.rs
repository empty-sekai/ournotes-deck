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
    #[cfg(test)]
    full_paired_undo: bool,
}

impl Replay {
    pub(crate) fn new(frames: usize, rows: &[ProbeRow]) -> Self {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|row| row.owner);
        let row_steps = vec![steps(&[], &rows)];
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
            #[cfg(test)]
            full_paired_undo: false,
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
    pub(crate) fn probe(&mut self, frame: usize, time_ms: i32) -> Result<(), Error> {
        if self.rows.is_empty() {
            return Ok(());
        }
        let entry = self.frame(frame)?;
        if let Err(at) = entry.probes.binary_search(&time_ms) {
            entry.probes.insert(at, time_ms);
        }
        self.potential(frame);
        Ok(())
    }

    /// The native `calculate` up to score frame `to`.
    pub(crate) fn query(&mut self, to: i32) -> Result<(), Error> {
        if to == self.prev && self.mandatory.is_none() && self.potential.is_empty() {
            return Ok(());
        }
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
    fn paired_undo(&self, frame: usize, start: &Classes) -> Result<Classes, Error> {
        #[cfg(test)]
        if self.full_paired_undo {
            return self.paired_undo_full(frame, start);
        }
        let entry = &self.frames[frame];
        let (ops, probes) = (&entry.ops, &entry.probes);
        // In a note-only frame the other fields never change: native apply skips both signs of zero,
        // the recorded sum stays +0, and undo subtracts +0. Each original endpoint admits every probe
        // branch, so retaining its unchanged marginal hull loses no field endpoint or class.
        // Keep the full traversal when another field changes, or endpoint visitation could affect
        // signed-zero bits or the original nonfinite refusal.
        if ops.iter().any(|op| op.deltas.iter().enumerate().any(|(field, &delta)| field != 1 && delta != 0.0))
            || start.iter().flatten().flatten().any(|field| {
                !field.lower().is_finite()
                    || !field.upper().is_finite()
                    || (field.lower() == 0.0
                        && field.upper() == 0.0
                        && field.lower().to_bits() != field.upper().to_bits())
            })
        {
            return self.paired_undo_full(frame, start);
        }
        let mut out = *start;
        let mut paths = Vec::with_capacity(4);
        for (class, fields) in start.iter().enumerate() {
            let Some(fields) = fields else { continue };
            for state in [fields[1].lower(), fields[1].upper()] {
                paths.push(PairedNotePath { start: class, class, state, sum: 0.0 });
            }
        }
        dedup_note_paths(&mut paths);
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
                    let variants = if group.is_empty() { None } else { Some(variants(group, &self.rows)?) };
                    let variants = variants.as_deref().unwrap_or(&self.row_steps);
                    let stay: Vec<Step> = group.iter().map(|op| Step::Command(op.deltas)).collect();
                    let mut next = Vec::with_capacity(paired_capacity(paths.len(), variants.len())?);
                    for path in &paths {
                        let mut kept = *path;
                        kept.apply(&stay, 1.0);
                        next.push(kept);
                        for steps in variants {
                            let mut switched = *path;
                            switched.apply(steps, if path.class == 0 { 1.0 } else { -1.0 });
                            switched.class = 1 - path.class;
                            next.push(switched);
                        }
                    }
                    dedup_note_paths(&mut next);
                    paths = next;
                    op += end;
                    probe += 1;
                }
                (Some(_), None) => unreachable!("covered by the command arm"),
            }
        }
        // Projection changes path visitation order. Finite nonzero extrema have unique bit patterns;
        // if either zero sign could win a hull tie, keep the full vector order instead.
        let mut notes = [None::<F32Interval>; 2];
        let mut zeros = [0u8; 2];
        for path in paths {
            let value = path.state - path.sum;
            if value == 0.0 {
                zeros[path.start] |= if value.is_sign_negative() { 1 } else { 2 };
            }
            if !value.is_finite() || zeros[path.start] == 3 {
                return self.paired_undo_full(frame, start);
            }
            let value = point(value)?;
            notes[path.start] = Some(notes[path.start].map_or(value, |old| old.hull(value)));
        }
        for (class, fields) in out.iter_mut().enumerate() {
            if let Some(fields) = fields {
                fields[1] = notes[class].expect("every initial class retains its stay path");
            }
        }
        Ok(out)
    }

    /// Full vector traversal preserves native visitation and exceptional arithmetic, and is the
    /// retained reference for the note-only projection.
    fn paired_undo_full(&self, frame: usize, start: &Classes) -> Result<Classes, Error> {
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
                    let variants = if group.is_empty() { None } else { Some(variants(group, &self.rows)?) };
                    let variants = variants.as_deref().unwrap_or(&self.row_steps);
                    let stay: Vec<Step> = group.iter().map(|op| Step::Command(op.deltas)).collect();
                    let mut next = Vec::with_capacity(paired_capacity(paths.len(), variants.len())?);
                    for path in &paths {
                        let mut kept = *path;
                        kept.apply(&stay, 1.0);
                        next.push(kept);
                        for steps in variants {
                            let mut switched = *path;
                            switched.apply(steps, if path.class == 0 { 1.0 } else { -1.0 });
                            switched.class = 1 - path.class;
                            next.push(switched);
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
    fn execute(&mut self, frame: usize, state: Classes, all: bool) -> Result<Classes, Error> {
        if self.frames[frame].ops.is_empty() && self.frames[frame].probes.is_empty() {
            // The full endpoint hull chooses zero signs in path order. Retain that order for intervals
            // containing both zero bit patterns, even though their numerical endpoints are equal.
            if state.iter().flatten().flatten().any(|field| {
                field.lower() == 0.0 && field.upper() == 0.0 && field.lower().to_bits() != field.upper().to_bits()
            }) {
                return self.execute_full(frame, state, all);
            }
            // No float command or class switch can occur. Native execution leaves each field unchanged,
            // records +0 in every diff field, and its immediate undo subtracts that same +0.
            for fields in state.iter().flatten() {
                finite(fields)?;
            }
            let observed = if self.rows.is_empty() { [state[0], state[0]] } else { state };
            for &index in &self.frames[frame].notes {
                let note = &mut self.notes[index];
                note.executed = if all { observed } else { hull_classes(note.executed, observed) };
            }
            let sums = [[state[0].map(|_| zero()), None], [None, state[1].map(|_| zero())]];
            self.retain_frame(frame, all, sums, state);
            return Ok(state);
        }
        self.execute_full(frame, state, all)
    }

    fn execute_full(&mut self, frame: usize, mut state: Classes, all: bool) -> Result<Classes, Error> {
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
                        let variants = if group.is_empty() { None } else { Some(variants(group, &self.rows)?) };
                        let variants = variants.as_deref().unwrap_or(&self.row_steps);
                        state = transition(state, group, variants)?;
                        for row in &mut sums {
                            *row = transition(*row, group, variants)?;
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
        self.retain_frame(frame, all, sums, paired);
        Ok(state)
    }

    fn retain_frame(&mut self, frame: usize, all: bool, sums: [Classes; 2], paired: Classes) {
        let entry = &mut self.frames[frame];
        entry.diff = Some(match (all, entry.diff) {
            (false, Some(old)) => [hull_classes(old[0], sums[0]), hull_classes(old[1], sums[1])],
            _ => sums,
        });
        entry.undo = Some(match (all, entry.undo) {
            (false, Some(old)) => hull_classes(old, paired),
            _ => paired,
        });
    }
}

/// The note field of one endpoint path; every future note operation depends only on this key.
#[derive(Clone, Copy)]
struct PairedNotePath {
    start: usize,
    class: usize,
    state: f32,
    sum: f32,
}

impl PairedNotePath {
    fn apply(&mut self, steps: &[Step], sign: f32) {
        for step in steps {
            let delta = match *step {
                Step::Command(deltas) => deltas[1],
                Step::Probe(value) => sign * value,
            };
            if delta != 0.0 {
                self.state += delta;
                self.sum += delta;
            }
        }
    }

    fn key(&self) -> (usize, usize, u32, u32) {
        (self.start, self.class, self.state.to_bits(), self.sum.to_bits())
    }
}

fn dedup_note_paths(paths: &mut Vec<PairedNotePath>) {
    paths.sort_unstable_by_key(PairedNotePath::key);
    paths.dedup_by_key(|path| path.key());
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
fn transition(state: Classes, group: &[Op], variants: &[Vec<Step>]) -> Result<Classes, Error> {
    let mut stay = state;
    for op in group {
        for class in stay.iter_mut().flatten() {
            add_deltas(class, &op.deltas)?;
        }
    }
    let mut on = None;
    let mut off = None;
    for steps in variants {
        on = hull(on, state[0].map(|s| apply_steps(s, steps, 1.0)).transpose()?);
        off = hull(off, state[1].map(|s| apply_steps(s, steps, -1.0)).transpose()?);
    }
    let out = [hull(stay[0], off), hull(stay[1], on)];
    for class in out.iter().flatten() {
        finite(class)?;
    }
    Ok(out)
}

const MAX_SHUFFLE_VARIANTS: usize = 4096;
const MAX_SHUFFLE_STORAGE: usize = 131_072;
const MAX_PAIRED_PATHS: usize = 4096;

fn shuffle_capacity() -> Error {
    Error::Capacity("native score probe-order enclosure exceeds its complete-work budget".into())
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

fn paired_capacity(paths: usize, alternatives: usize) -> Result<usize, Error> {
    let count = paths.checked_add(shuffle_product(paths, alternatives)?).ok_or_else(shuffle_capacity)?;
    if count > MAX_PAIRED_PATHS {
        return Err(shuffle_capacity());
    }
    Ok(count)
}

/// Every owner-local merge that preserves ordinary filing order and probe-row order. Owners remain in native
/// score order; their independent interleavings form a Cartesian product. A finite work limit refuses inputs
/// whose complete enclosure cannot be represented, without selecting only a subset of their orders.
fn variants(group: &[Op], rows: &[ProbeRow]) -> Result<Vec<Vec<Step>>, Error> {
    let length = group.len().checked_add(rows.len()).ok_or_else(shuffle_capacity)?;
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
    if count == 1 {
        return Ok(vec![steps(group, rows)]);
    }
    if shuffle_product(count, length)? > MAX_SHUFFLE_STORAGE {
        return Err(shuffle_capacity());
    }
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

fn steps(group: &[Op], rows: &[ProbeRow]) -> Vec<Step> {
    let mut out = Vec::with_capacity(group.len() + rows.len());
    let (mut g, mut r) = (0, 0);
    while g < group.len() || r < rows.len() {
        let take_row = match (group.get(g), rows.get(r)) {
            (Some(op), Some(row)) => row.owner <= op.owner,
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

    fn owner_order_op(owner: i32, delta: f32) -> Op {
        let mut deltas = [0.0; FIELDS];
        deltas[1] = delta;
        Op { time: 13, owner, deltas }
    }

    fn owner_order_words(steps: &[super::Step]) -> Vec<u32> {
        steps
            .iter()
            .map(|step| match step {
                super::Step::Command(values) => values[1].to_bits(),
                super::Step::Probe(value) => value.to_bits(),
            })
            .collect()
    }

    #[test]
    fn owner_orders_preserve_both_filing_sequences_and_combine_independent_owners() {
        let group = [owner_order_op(1, 0.3), owner_order_op(2, 0.1), owner_order_op(2, 0.2)];
        let rows = [
            ProbeRow { owner: 1, value: 0.0001 },
            ProbeRow { owner: 1, value: 0.0002 },
            ProbeRow { owner: 2, value: 0.8 },
        ];
        let actual: std::collections::BTreeSet<_> =
            variants(&group, &rows).unwrap().iter().map(|steps| owner_order_words(steps)).collect();
        let mut expected = std::collections::BTreeSet::new();
        for first in [[0.3f32, 0.0001, 0.0002], [0.0001, 0.3, 0.0002], [0.0001, 0.0002, 0.3]] {
            for second in [[0.1f32, 0.2, 0.8], [0.1, 0.8, 0.2], [0.8, 0.1, 0.2]] {
                expected.insert(first.into_iter().chain(second).map(f32::to_bits).collect::<Vec<_>>());
            }
        }
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 9);
    }

    #[test]
    fn a_same_owner_command_between_probe_rows_is_enclosed() {
        for full in [false, true] {
            let mut native = calculator();
            let mut replay = Replay::new(
                native.executed_states().0,
                &[ProbeRow { owner: 1, value: 0.1 }, ProbeRow { owner: 1, value: -0.1 }],
            );
            replay.full_paired_undo = full;
            let ordinary = FactorCommand { time_ms: 0, owner_id: 1, note_mill: 30_000, ..Default::default() };
            replay.file_command(0, &ordinary).unwrap();
            replay.probe(0, 0).unwrap();
            for command in [
                FactorCommand { time_ms: 0, owner_id: 1, note_mill: 10_000, ..Default::default() },
                ordinary,
                FactorCommand { time_ms: 0, owner_id: 1, note_mill: -10_000, ..Default::default() },
            ] {
                native.add_factor(command);
            }
            native.add_note(NoteCommand::new(0, 1000, 1, 1, 2));
            let index = replay.file_note(0, 0, 1).unwrap();
            native.calculate(0, &ComboCounter::new(64), None).unwrap();
            replay.query(0).unwrap();
            let actual = native.executed_states().1.into_iter().find(|(id, _)| *id == 1).unwrap().1[1];
            let enclosure = replay.notes[index].executed[1].unwrap()[1];
            assert!(enclosure.contains(actual), "native={actual:?}, enclosure={enclosure:?}");
        }
    }

    fn native_fields(native: &IncrementalCalculator) -> [f32; FIELDS] {
        let state = native.calc.state;
        [state.combo_score_up, state.note_score_up, state.just, state.perfect, state.great, state.good]
    }

    fn assert_fields_enclosed(fields: Fields, actual: [f32; FIELDS]) {
        for (field, (enclosure, value)) in fields.into_iter().zip(actual).enumerate() {
            assert!(enclosure.contains(value), "field={field} native={value:?}, enclosure={enclosure:?}");
        }
    }

    #[test]
    fn signed_owner_interleavings_enclose_native_execution_undo_and_reexecution() {
        let combo = ComboCounter::new(64);
        let mut checked = 0;
        for full in [false, true] {
            for other_fields in [false, true] {
                for probe_mill in [-10, 10] {
                    for ordinary_mill in [-30_000, 30_000] {
                        for undo_probe in [false, true] {
                            let rows = [ProbeRow { owner: 101, value: mill(probe_mill) }; 2];
                            let ordinary = FactorCommand {
                                time_ms: 13,
                                owner_id: 101,
                                note_mill: ordinary_mill,
                                combo_mill: if other_fields { 12_345 } else { 0 },
                                judgement: 3,
                                judge_mill: if other_fields { -7_777 } else { 0 },
                                ..Default::default()
                            };
                            let group = [Op { time: 13, owner: 101, deltas: command_deltas(&ordinary) }];
                            let programs = variants(&group, &rows).unwrap();
                            assert_eq!(programs.len(), 3);
                            let mut replay = Replay::new(calculator().executed_states().0, &rows);
                            replay.full_paired_undo = full;
                            if undo_probe {
                                replay.probe(0, 0).unwrap();
                            }
                            replay.file_command(1, &ordinary).unwrap();
                            replay.probe(1, 13).unwrap();
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
                                    native.add_factor(match step {
                                        super::Step::Command(_) => ordinary,
                                        super::Step::Probe(_) => FactorCommand {
                                            time_ms: 13,
                                            owner_id: 101,
                                            note_mill: if undo_probe { -probe_mill } else { probe_mill },
                                            ..Default::default()
                                        },
                                    });
                                }
                                native.add_note(NoteCommand::new(14, 1000, 0, 1, 2));
                                native.calculate(39, &combo, None).unwrap();
                                assert_fields_enclosed(end, native_fields(&native));
                                native.calculate(0, &combo, None).unwrap();
                                assert_fields_enclosed(undone, native_fields(&native));
                                native.calculate(39, &combo, None).unwrap();
                                assert_fields_enclosed(repeated, native_fields(&native));
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 96);
    }

    #[test]
    fn owner_order_limits_refuse_a_partial_enclosure() {
        let mut group: Vec<_> = (0..12).map(|owner| owner_order_op(owner, 0.3)).collect();
        let mut rows: Vec<_> = (0..12).map(|owner| ProbeRow { owner, value: 0.0001 }).collect();
        assert_eq!(variants(&group, &rows).unwrap().len(), MAX_SHUFFLE_VARIANTS);
        for full in [false, true] {
            let mut replay = Replay::new(3, &rows);
            replay.full_paired_undo = full;
            for op in &group {
                replay
                    .file_command(
                        1,
                        &FactorCommand {
                            time_ms: op.time,
                            owner_id: op.owner,
                            note_mill: 30_000,
                            ..Default::default()
                        },
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
        }
        group.push(owner_order_op(12, 0.3));
        rows.push(ProbeRow { owner: 12, value: 0.0001 });
        assert!(matches!(variants(&group, &rows), Err(Error::Capacity(_))));
        assert!(matches!(shuffle_choose(usize::MAX, 1), Err(Error::Capacity(_))));
    }

    #[test]
    fn unique_owner_merges_keep_their_complete_order_without_a_length_limit() {
        let group: Vec<_> = (0..900).map(|index| owner_order_op(index * 2 + 1, 0.3)).collect();
        let rows: Vec<_> = (0..900).map(|index| ProbeRow { owner: index * 2, value: 0.0001 }).collect();
        let programs = variants(&group, &rows).unwrap();
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].len(), 1800);
        assert!(
            programs[0]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| { matches!(pair, [super::Step::Probe(_), super::Step::Command(_)]) })
        );
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

    fn class_bits(classes: Classes) -> [Option<[(u32, u32); FIELDS]>; 2] {
        classes
            .map(|fields| fields.map(|fields| fields.map(|value| (value.lower().to_bits(), value.upper().to_bits()))))
    }

    #[track_caller]
    fn compare_paired_projection(replay: &Replay, start: &Classes) -> Result<Classes, Error> {
        let actual = replay.paired_undo(0, start);
        let expected = replay.paired_undo_full(0, start);
        match (&actual, &expected) {
            (Ok(actual), Ok(expected)) => assert_eq!(class_bits(*actual), class_bits(*expected)),
            (Err(actual), Err(expected)) => assert_eq!(actual, expected),
            _ => panic!("projected={actual:?}, full={expected:?}"),
        }
        actual
    }

    #[test]
    fn note_only_paired_undo_matches_full_paths_over_finite_combinations() {
        let tiny = f32::from_bits(1);
        let note_bounds = [
            (0.0, 0.0),
            (-0.0, -0.0),
            (tiny, f32::from_bits(2)),
            (-1.25, 0.13),
            (1.0, 1.0f32.next_up()),
            (16_777_216.0, 16_777_216.0f32.next_up()),
        ];
        let row_sets: [&[ProbeRow]; 6] = [
            &[],
            &[ProbeRow { owner: 1, value: 0.0 }, ProbeRow { owner: 1, value: -0.0 }],
            &[ProbeRow { owner: 1, value: tiny }],
            &[ProbeRow { owner: 2, value: -0.5 }, ProbeRow { owner: 0, value: 0.5 }],
            &[ProbeRow { owner: 1, value: 0.12345 }, ProbeRow { owner: 1, value: -0.77777 }],
            &[ProbeRow { owner: 1, value: -0.77777 }, ProbeRow { owner: 1, value: 0.12345 }],
        ];
        let schedules: [&[(i32, i32, i32)]; 6] = [
            &[],
            &[(0, 1, 12_345)],
            &[(1, 1, 12_345), (0, 1, -7_777), (1, 1, -12_345)],
            &[(0, 0, i32::MAX), (1, 2, i32::MIN), (2, 1, 3_333)],
            &[(1, 1, 0), (0, 0, 0)],
            &[(2, 1, 50_000), (2, 1, -12_345), (2, 1, -50_000)],
        ];
        let mut checked = 0;
        for rows in row_sets {
            for schedule in schedules {
                for reverse_filing in [false, true] {
                    for probe_mask in 0..8 {
                        let mut replay = Replay::new(1, rows);
                        let mut commands: Vec<_> = schedule
                            .iter()
                            .map(|&(time_ms, owner_id, note_mill)| FactorCommand {
                                time_ms,
                                owner_id,
                                note_mill,
                                ..Default::default()
                            })
                            .collect();
                        if reverse_filing {
                            commands.reverse();
                        }
                        for command in commands {
                            replay.file_command(0, &command).unwrap();
                        }
                        for time in 0..3 {
                            if probe_mask & (1 << time) != 0 {
                                replay.probe(0, time).unwrap();
                            }
                        }
                        for mask in 0..4 {
                            for bound in 0..note_bounds.len() {
                                let start = std::array::from_fn(|class| {
                                    if mask & (1 << class) == 0 {
                                        return None;
                                    }
                                    // Wide unchanged fields keep distinct vector endpoints even when the
                                    // projected note field is a point. The classes also have distinct starts.
                                    let mut fields = std::array::from_fn(|field| {
                                        let value = (field as f32 - 2.0) * (class as f32 + 1.0);
                                        F32Interval::new(value - 0.25, value + 0.375).unwrap()
                                    });
                                    let (lower, upper) = note_bounds[(bound + class) % note_bounds.len()];
                                    fields[1] = F32Interval::new(lower, upper).unwrap();
                                    Some(fields)
                                });
                                compare_paired_projection(&replay, &start).unwrap();
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 13_824);
    }

    #[test]
    fn note_only_paired_undo_keeps_zero_bits_and_full_refusal_behavior() {
        for field in 0..FIELDS {
            for (lower, upper) in [(-0.0, 0.0), (0.0, -0.0), (-0.0, -0.0), (0.0, 0.0)] {
                let mut fields = initial_state();
                fields[field] = F32Interval::new(lower, upper).unwrap();
                for mask in 1..4 {
                    let start = std::array::from_fn(|class| (mask & (1 << class) != 0).then_some(fields));
                    let mut replay = Replay::new(1, &[ProbeRow { owner: 1, value: f32::from_bits(1) }]);
                    replay.probe(0, 0).unwrap();
                    replay.probe(0, 1).unwrap();
                    compare_paired_projection(&replay, &start).unwrap();
                }
            }
            for (lower, upper) in [
                (f32::NEG_INFINITY, f32::NEG_INFINITY),
                (f32::INFINITY, f32::INFINITY),
                (f32::NEG_INFINITY, 1.0),
                (1.0, f32::INFINITY),
            ] {
                let mut fields = initial_state();
                fields[field] = F32Interval::new(lower, upper).unwrap();
                for mask in 1..4 {
                    let start = std::array::from_fn(|class| (mask & (1 << class) != 0).then_some(fields));
                    let replay = Replay::new(1, &[]);
                    compare_paired_projection(&replay, &start).unwrap_err();
                }
            }
            for delta in [f32::MAX, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
                let mut deltas = [0.0; FIELDS];
                deltas[field] = delta;
                let mut replay = Replay::new(1, &[ProbeRow { owner: 1, value: f32::MAX }]);
                replay.frames[0].ops = vec![Op { time: 0, owner: 1, deltas }; 2];
                replay.probe(0, 0).unwrap();
                replay.probe(0, 1).unwrap();
                for mask in 1..4 {
                    let start = std::array::from_fn(|class| (mask & (1 << class) != 0).then_some(initial_state()));
                    compare_paired_projection(&replay, &start).unwrap_err();
                }
            }
        }
        for value in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            let mut replay = Replay::new(1, &[ProbeRow { owner: 1, value }]);
            replay.probe(0, 0).unwrap();
            compare_paired_projection(&replay, &[Some(initial_state()), None]).unwrap_err();
        }
    }

    #[test]
    fn paired_undo_retains_full_traversal_for_other_factor_commands() {
        for field in 0..FIELDS {
            for delta in [0.0, -0.0, f32::from_bits(1), -0.12345, 0.5] {
                let mut deltas = [0.0; FIELDS];
                deltas[1] = 0.33333;
                deltas[field] = delta;
                let mut replay = Replay::new(1, &[ProbeRow { owner: 1, value: 0.12345 }]);
                replay.frames[0].ops = vec![Op { time: 0, owner: 1, deltas }];
                replay.probe(0, 0).unwrap();
                replay.probe(0, 1).unwrap();
                for mask in 0..4 {
                    let start = std::array::from_fn(|class| {
                        (mask & (1 << class) != 0).then(|| {
                            std::array::from_fn(|field| {
                                let value = (field + class) as f32;
                                F32Interval::new(value - 0.25, value + 0.375).unwrap()
                            })
                        })
                    });
                    compare_paired_projection(&replay, &start).unwrap();
                }
            }
        }
    }

    #[track_caller]
    fn compare_replay_bits(actual: &Replay, expected: &Replay) {
        assert_eq!(class_bits(actual.state), class_bits(expected.state));
        assert_eq!(actual.prev, expected.prev);
        assert_eq!(actual.fresh, expected.fresh);
        assert_eq!(actual.mandatory, expected.mandatory);
        assert_eq!(actual.potential, expected.potential);
        assert_eq!(actual.frames.len(), expected.frames.len());
        for (actual, expected) in actual.frames.iter().zip(&expected.frames) {
            assert_eq!(actual.diff.map(|rows| rows.map(class_bits)), expected.diff.map(|rows| rows.map(class_bits)));
            assert_eq!(actual.undo.map(class_bits), expected.undo.map(class_bits));
        }
        assert_eq!(actual.notes.len(), expected.notes.len());
        for (actual, expected) in actual.notes.iter().zip(&expected.notes) {
            assert_eq!((actual.time_ms, actual.note_id), (expected.time_ms, expected.note_id));
            assert_eq!(class_bits(actual.executed), class_bits(expected.executed));
        }
    }

    #[test]
    fn note_projection_matches_full_replay_through_late_filings_and_optional_rewinds() {
        let row_sets: [&[ProbeRow]; 3] = [
            &[],
            &[ProbeRow { owner: 1, value: 0.12345 }],
            &[ProbeRow { owner: 2, value: 0.5 }, ProbeRow { owner: 1, value: -0.33333 }],
        ];
        for rows in row_sets {
            for value in [-0.0, 0.0, 1.0000001, 16_777_216.0] {
                for reverse_filing in [false, true] {
                    let mut actual = Replay::new(5, rows);
                    let mut expected = Replay::new(5, rows);
                    expected.full_paired_undo = true;
                    for replay in [&mut actual, &mut expected] {
                        replay.state[0].as_mut().unwrap()[1] = point(value).unwrap();
                        for (frame, time, id) in [(0, 0, 1), (1, 17, 2), (2, 35, 3), (3, 50, 4), (4, 67, 5)] {
                            replay.file_note(frame, time, id).unwrap();
                        }
                        let mut commands = [
                            (0, FactorCommand { time_ms: 0, owner_id: 1, note_mill: 12_345, ..Default::default() }),
                            (2, FactorCommand { time_ms: 32, owner_id: 2, note_mill: -7_777, ..Default::default() }),
                            (2, FactorCommand { time_ms: 32, owner_id: 2, note_mill: 3_333, ..Default::default() }),
                        ];
                        if reverse_filing {
                            commands.reverse();
                        }
                        for (frame, command) in commands {
                            replay.file_command(frame, &command).unwrap();
                        }
                        replay.probe(1, 16).unwrap();
                        replay.probe(2, 32).unwrap();
                    }
                    for to in [3, 3] {
                        actual.query(to).unwrap();
                        expected.query(to).unwrap();
                        compare_replay_bits(&actual, &expected);
                    }
                    for replay in [&mut actual, &mut expected] {
                        replay.file_note(1, 19, 6).unwrap();
                        replay
                            .file_command(
                                1,
                                &FactorCommand { time_ms: 18, owner_id: 1, note_mill: 33_333, ..Default::default() },
                            )
                            .unwrap();
                        replay.probe(1, 18).unwrap();
                        // The mandatory late filing rewinds every path to frame 1; this potential filing
                        // additionally executes frame 0 on only some paths, retaining optional history.
                        replay.potential(0);
                    }
                    for to in [3, 1, 1] {
                        actual.query(to).unwrap();
                        expected.query(to).unwrap();
                        compare_replay_bits(&actual, &expected);
                    }
                    actual.potential(1);
                    expected.potential(1);
                    for to in [4, 0, 4, 4] {
                        actual.query(to).unwrap();
                        expected.query(to).unwrap();
                        compare_replay_bits(&actual, &expected);
                    }
                    for all in [false, true] {
                        let state = [Some(initial_state()), Some([point(2.0).unwrap(); FIELDS])];
                        let result = actual.execute(2, state, all).unwrap();
                        let reference = expected.execute(2, state, all).unwrap();
                        assert_eq!(class_bits(result), class_bits(reference));
                        compare_replay_bits(&actual, &expected);
                    }
                }
            }
        }
    }

    #[test]
    fn empty_frame_reuse_matches_full_arithmetic_for_each_class_and_field() {
        let probe_rows = [ProbeRow { owner: 1, value: 0.125 }];
        for rows in [&[][..], &probe_rows[..]] {
            for mask in 0..4 {
                for all in [false, true] {
                    for field in 0..FIELDS {
                        for (lower, upper) in
                            [(-0.0, -0.0), (0.0, 0.0), (-0.0, 0.0), (0.0, -0.0), (-1.25, 2.5), (-f32::MAX, f32::MAX)]
                        {
                            let mut fields = initial_state();
                            fields[field] = F32Interval::new(lower, upper).unwrap();
                            let state = std::array::from_fn(|class| ((mask >> class) & 1 != 0).then_some(fields));
                            let mut actual = Replay::new(3, rows);
                            let mut expected = Replay::new(3, rows);
                            for replay in [&mut actual, &mut expected] {
                                replay.file_note(1, 40, 1).unwrap();
                                replay.execute_full(1, [Some(initial_state()), None], true).unwrap();
                            }
                            let result = actual.execute(1, state, all).unwrap();
                            let reference = expected.execute_full(1, state, all).unwrap();
                            assert_eq!(class_bits(result), class_bits(reference));
                            assert_eq!(
                                actual.frames[1].diff.unwrap().map(class_bits),
                                expected.frames[1].diff.unwrap().map(class_bits),
                            );
                            assert_eq!(
                                class_bits(actual.frames[1].undo.unwrap()),
                                class_bits(expected.frames[1].undo.unwrap()),
                                "rows={} mask={mask} all={all} field={field} lower={lower:?} upper={upper:?}",
                                rows.len(),
                            );
                            assert_eq!(class_bits(actual.notes[0].executed), class_bits(expected.notes[0].executed));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn empty_frames_reject_nonfinite_fields_before_recording_history() {
        for class in 0..2 {
            for field in 0..FIELDS {
                for (lower, upper) in [
                    (f32::NEG_INFINITY, f32::NEG_INFINITY),
                    (f32::INFINITY, f32::INFINITY),
                    (f32::NEG_INFINITY, 1.0),
                    (1.0, f32::INFINITY),
                ] {
                    for all in [false, true] {
                        let mut fields = initial_state();
                        fields[field] = F32Interval::new(lower, upper).unwrap();
                        let mut state = [None, None];
                        state[class] = Some(fields);
                        let mut actual = Replay::new(3, &[]);
                        let mut expected = Replay::new(3, &[]);
                        for replay in [&mut actual, &mut expected] {
                            replay.file_note(1, 40, 1).unwrap();
                            replay.execute_full(1, [Some(initial_state()), None], true).unwrap();
                        }
                        let before_diff = actual.frames[1].diff.unwrap().map(class_bits);
                        let before_undo = class_bits(actual.frames[1].undo.unwrap());
                        let before_note = class_bits(actual.notes[0].executed);
                        let result = actual.execute(1, state, all).unwrap_err();
                        let reference = expected.execute_full(1, state, all).unwrap_err();
                        assert!(matches!(result, Error::Unsupported(_)));
                        assert_eq!(result.to_string(), reference.to_string());
                        for replay in [&actual, &expected] {
                            assert_eq!(replay.frames[1].diff.unwrap().map(class_bits), before_diff);
                            assert_eq!(class_bits(replay.frames[1].undo.unwrap()), before_undo);
                            assert_eq!(class_bits(replay.notes[0].executed), before_note);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn empty_frames_keep_signed_zero_and_optional_note_histories() {
        let mut replay = Replay::new(4, &[]);
        let note = replay.file_note(1, 40, 1).unwrap();
        let mut fields = initial_state();
        fields[0] = point(-0.0).unwrap();
        replay.state = [Some(fields), None];
        replay.query(2).unwrap();
        let before = replay.state;
        replay.query(2).unwrap();
        assert_eq!(replay.state, before);
        let undone = replay.undo(2, replay.state, true).unwrap()[0].unwrap();
        assert_eq!(undone[0].lower().to_bits(), (-0.0f32).to_bits());
        assert_eq!(undone[0].upper().to_bits(), (-0.0f32).to_bits());
        let mut alternate = fields;
        alternate[1] = point(2.0).unwrap();
        replay.execute(1, [Some(alternate), None], false).unwrap();
        let observed = replay.notes[note].executed[0].unwrap();
        assert_eq!((observed[1].lower(), observed[1].upper()), (1.0, 2.0));
        let paired = replay.frames[1].undo.unwrap()[0].unwrap();
        assert_eq!((paired[1].lower(), paired[1].upper()), (1.0, 2.0));
        replay.execute(1, [Some(alternate), None], true).unwrap();
        assert_eq!(replay.notes[note].executed[0].unwrap()[1], point(2.0).unwrap());
    }

    #[test]
    fn quiet_queries_still_execute_late_commands_and_rank_rewinds() {
        let mut native = calculator();
        let frames = native.executed_states().0;
        let mut replay = Replay::new(frames, &[]);
        let note = NoteCommand::new(80, 1000, 1, 1, 2);
        native.add_note(note);
        let index = replay.file_note(frame_of(80, frames), 80, 1).unwrap();
        let combo = ComboCounter::new(64);
        for time in [120, 120] {
            native.calculate(time, &combo, None).unwrap();
            replay.query(frame_of(time, frames) as i32).unwrap();
        }
        let command = FactorCommand { time_ms: 0, owner_id: 1, note_mill: 12_345, ..Default::default() };
        native.add_factor(command);
        replay.file_command(0, &command).unwrap();
        for time in [120, 40, 40, 120, 120] {
            native.calculate(time, &combo, None).unwrap();
            replay.query(frame_of(time, frames) as i32).unwrap();
        }
        let actual = native.executed_states().1.into_iter().find(|(id, _)| *id == 1).unwrap().1;
        for (field, value) in actual.into_iter().enumerate() {
            let enclosure = replay.notes[index].executed[0].unwrap()[field];
            assert_eq!(enclosure.lower().to_bits(), value.to_bits());
            assert_eq!(enclosure.upper().to_bits(), value.to_bits());
        }
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
}
