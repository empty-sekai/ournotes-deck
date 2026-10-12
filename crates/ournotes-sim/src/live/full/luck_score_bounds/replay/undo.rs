//! Undo and re-execution of the filed frames of a replay.

use super::*;

impl Replay {
    /// Undo `frame` from `end`. `last` when every path holds the end state of its last execution of the frame.
    pub(super) fn undo(&self, frame: usize, end: Classes, last: bool) -> Result<Classes, Error> {
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
    pub(super) fn paired_undo(&self, frame: usize, start: &Classes) -> Result<Classes, Error> {
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
    pub(super) fn paired_undo_full(&self, frame: usize, start: &Classes) -> Result<Classes, Error> {
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
    pub(super) fn execute(&mut self, frame: usize, state: Classes, all: bool) -> Result<Classes, Error> {
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

    pub(super) fn execute_full(&mut self, frame: usize, mut state: Classes, all: bool) -> Result<Classes, Error> {
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
