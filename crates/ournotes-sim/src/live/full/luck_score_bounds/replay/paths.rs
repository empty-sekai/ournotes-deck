//! The paths through a group of filed commands: their orders, variants and limits.

use super::*;

/// The note field of one endpoint path; every future note operation depends only on this key.
#[derive(Clone, Copy)]
pub(super) struct PairedNotePath {
    pub(super) start: usize,
    pub(super) class: usize,
    pub(super) state: f32,
    pub(super) sum: f32,
}

impl PairedNotePath {
    pub(super) fn apply(&mut self, steps: &[Step], sign: f32) {
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

pub(super) fn dedup_note_paths(paths: &mut Vec<PairedNotePath>) {
    paths.sort_unstable_by_key(PairedNotePath::key);
    paths.dedup_by_key(|path| path.key());
}

/// One path through a frame from a start endpoint: its start class, current class, binary32 state and the binary32
/// sum of the commands it applied.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct PairedPath {
    pub(super) start: usize,
    pub(super) class: usize,
    pub(super) state: [f32; FIELDS],
    pub(super) sum: [f32; FIELDS],
}

impl PairedPath {
    pub(super) fn apply(&mut self, steps: &[Step], sign: f32) {
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

pub(super) fn dedup_paths(paths: &mut Vec<PairedPath>) {
    paths.sort_unstable_by_key(PairedPath::key);
    paths.dedup_by_key(|path| path.key());
}

pub(super) fn intersect_fields(a: Fields, b: Fields) -> Result<Fields, Error> {
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
pub(super) fn transition(state: Classes, group: &[Op], variants: &[Vec<Step>]) -> Result<Classes, Error> {
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

pub(super) const MAX_SHUFFLE_VARIANTS: usize = 4096;

const MAX_SHUFFLE_STORAGE: usize = 131_072;

const MAX_PAIRED_PATHS: usize = 4096;

fn shuffle_capacity() -> Error {
    Error::Capacity("native score probe-order enclosure exceeds its complete-work budget".into())
}

fn shuffle_product(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or_else(shuffle_capacity)
}

pub(super) fn shuffle_choose(a: usize, b: usize) -> Result<usize, Error> {
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

pub(super) fn paired_capacity(paths: usize, alternatives: usize) -> Result<usize, Error> {
    let count = paths.checked_add(shuffle_product(paths, alternatives)?).ok_or_else(shuffle_capacity)?;
    if count > MAX_PAIRED_PATHS {
        return Err(shuffle_capacity());
    }
    Ok(count)
}

/// Every owner-local merge that preserves ordinary filing order and probe-row order. Owners remain in native
/// score order; their independent interleavings form a Cartesian product. A finite work limit refuses inputs
/// whose complete enclosure cannot be represented, without selecting only a subset of their orders.
pub(super) fn variants(group: &[Op], rows: &[ProbeRow]) -> Result<Vec<Vec<Step>>, Error> {
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

pub(super) fn steps(group: &[Op], rows: &[ProbeRow]) -> Vec<Step> {
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
