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

mod paths;
mod undo;
use paths::{
    PairedNotePath, PairedPath, dedup_note_paths, dedup_paths, intersect_fields, paired_capacity, steps, transition,
    variants,
};

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
}

#[cfg(test)]
mod tests;
