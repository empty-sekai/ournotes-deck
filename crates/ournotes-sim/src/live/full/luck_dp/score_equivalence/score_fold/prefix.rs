//! Share only complete, identical controller-output prefixes while executing the original native score tape.
//!
//! A checkpoint owns the entire calculator, including each frame's retained note scores and binary32 undo
//! differences, plus combo observations, rank snapshots and the exact event cursor. A terminal may itself
//! have descendants: its no-more-edges suffix is evaluated separately from those descendants.
use super::super::super::timeline_support::{TimelineSupport, TimelineTree};
use super::*;

const MAX_CHECKPOINT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct State {
    calc: IncrementalCalculator,
    combos: Vec<Vec<Option<(f32, f32)>>>,
    snapshots: Vec<i32>,
    event: usize,
    rush: bool,
    probe: bool,
    previous: i32,
    added: i32,
}

impl State {
    fn new(recipe: &Recipe) -> Self {
        Self {
            calc: recipe.initial.clone(),
            combos: recipe.note_counts.iter().map(|&n| vec![None; n]).collect(),
            snapshots: Vec::with_capacity(recipe.queries),
            event: 0,
            rush: false,
            probe: false,
            previous: -1,
            added: -1,
        }
    }

    fn bytes(&self) -> usize {
        let mut bytes = size_of::<Self>()
            .saturating_add(self.calc.recorded_storage_bytes().unwrap_or(usize::MAX))
            .saturating_add(self.combos.capacity().saturating_mul(size_of::<Vec<Option<(f32, f32)>>>()))
            .saturating_add(self.snapshots.capacity().saturating_mul(size_of::<i32>()));
        for row in &self.combos {
            bytes = bytes.saturating_add(row.capacity().saturating_mul(size_of::<Option<(f32, f32)>>()));
        }
        bytes
    }

    fn file(&mut self, time: i32, recipe: &Recipe) -> Checked<()> {
        let frame = get_frame(time).min(recipe.frames as i32 - 1);
        if frame < 0 {
            return refusal();
        }
        self.added = if self.added < 0 { frame } else { self.added.min(frame) };
        Ok(())
    }

    /// Execute exactly one ordinary event. Its anchor is still open until this function runs, so a child
    /// prefix can append another edge at the very same Query or ProbabilityReady event.
    fn event(&mut self, recipe: &Recipe, work: &mut Work, cancelled: &mut impl FnMut() -> bool) -> Checked<()> {
        if self.event.is_multiple_of(64) {
            poll(cancelled)?;
        }
        if work.events >= MAX_EVENTS {
            return declined(LuckScoreEquivalenceDecline::WorkBudget);
        }
        work.events += 1;
        match &recipe.events[self.event] {
            BoundsEvent::Note { note, .. } => {
                self.file(note.time_ms, recipe)?;
                self.calc.add_note(*note);
            }
            BoundsEvent::Factor { command, .. } if command.owner_id != -1 => {
                self.file(command.time_ms, recipe)?;
                self.calc.add_factor(*command);
            }
            BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                self.combos[*frame][*index] = Some((*ordinary, *gekisou));
            }
            BoundsEvent::Query { time_ms, to } => {
                let target = get_frame(*time_ms).max(0).min(recipe.frames as i32 - 1);
                if target != *to {
                    return refusal();
                }
                let undo_to = if self.added < 0 { target } else { target.min(self.added - 1) };
                let start = if undo_to < self.previous { undo_to + 1 } else { self.previous + 1 };
                let visits = (self.previous - undo_to).max(0) as u64 + (target - start + 1).max(0) as u64;
                if work.queries >= MAX_QUERIES || work.frame_steps.saturating_add(visits) > MAX_FRAME_STEPS {
                    return declined(LuckScoreEquivalenceDecline::WorkBudget);
                }
                work.queries += 1;
                work.frame_steps += visits;
                self.snapshots.push(native(
                    self.calc.calculate_recorded(*time_ms, &self.combos),
                    LuckScoreEquivalenceDecline::ScoreTrace,
                )?);
                self.previous = target;
                self.added = -1;
            }
            BoundsEvent::Rank { time_ms, percent, start, end, .. } => {
                let begin = start.map_or(0, |query| self.snapshots[query]);
                let end = self.snapshots[end.ok_or(Failure::Decline(LuckScoreEquivalenceDecline::ScoreTrace))?];
                let bonus = ((i128::from(end.wrapping_sub(begin)) * i128::from(*percent)) / 100) as i32;
                self.calc.add_fixed(*time_ms, bonus);
            }
            BoundsEvent::Factor { .. }
            | BoundsEvent::Potential { .. }
            | BoundsEvent::Probe { .. }
            | BoundsEvent::ProbabilityReady(_) => {}
        }
        self.event += 1;
        Ok(())
    }

    fn edge(
        &mut self,
        recipe: &Recipe,
        next: TimelineEdge,
        work: &mut Work,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Checked<()> {
        poll(cancelled)?;
        let anchor = match next.stage {
            0 => 0,
            1 => 1,
            2 | 3 => 2,
            _ => return refusal(),
        };
        if recipe.clock.get(next.frame).is_none_or(|&time| next.chart_time > time) {
            return refusal();
        }
        loop {
            if self.event >= recipe.events.len() {
                return refusal();
            }
            if let Some(position) = recipe.anchors[self.event] {
                if position > (next.frame, anchor) {
                    return refusal();
                }
                if position == (next.frame, anchor) {
                    break;
                }
            }
            self.event(recipe, work, cancelled)?;
        }
        match next.kind {
            TimelineKind::Rush => {
                if !matches!(next.stage, 0 | 2 | 3) || next.on == self.rush {
                    return refusal();
                }
                self.rush = next.on;
                self.file(next.chart_time, recipe)?;
                self.calc.add_factor(FactorCommand {
                    time_ms: next.chart_time,
                    owner_id: -1,
                    luck: if next.on { recipe.rush_percent } else { recipe.rush_percent.wrapping_neg() },
                    ..Default::default()
                });
            }
            TimelineKind::Probe => {
                if next.stage != 1
                    || next.on == self.probe
                    || next.chart_time != recipe.clock[next.frame]
                    || recipe.probes.is_empty()
                {
                    return refusal();
                }
                self.probe = next.on;
                let time = if !next.on && recipe.music_length > 0 {
                    next.chart_time.min(recipe.music_length)
                } else {
                    next.chart_time
                };
                for row in &recipe.probes {
                    self.file(time, recipe)?;
                    self.calc.add_factor(FactorCommand {
                        time_ms: time,
                        owner_id: row.owner,
                        note_mill: if next.on { row.mill } else { row.mill.wrapping_neg() },
                        ..Default::default()
                    });
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self, recipe: &Recipe, work: &mut Work, cancelled: &mut impl FnMut() -> bool) -> Checked<i32> {
        while self.event < recipe.events.len() {
            self.event(recipe, work, cancelled)?;
        }
        poll(cancelled)?;
        if self.rush || self.probe || self.snapshots.len() != recipe.queries {
            return refusal();
        }
        Ok(self.calc.score)
    }
}

struct Branch<const N: usize> {
    states: [State; N],
    child: u32,
    bytes: usize,
}

fn state_bytes<const N: usize>(states: &[State; N]) -> usize {
    states.iter().fold(0usize, |bytes, state| bytes.saturating_add(state.bytes()))
}

/// False means the optional checkpoint allowance was reached. The caller releases this traversal's state
/// and completes every unvisited terminal with the independent single-path evaluator.
#[allow(clippy::too_many_arguments)]
fn walk<const N: usize>(
    recipes: [&Recipe; N],
    tree: &TimelineTree<'_>,
    work: &mut Work,
    cancelled: &mut impl FnMut() -> bool,
    limit: usize,
    visited: &mut [bool],
    visit: &mut impl FnMut(usize, [i32; N]) -> Checked<()>,
) -> Checked<bool> {
    let mut states = recipes.map(State::new);
    let mut stack = Vec::<Branch<N>>::new();
    let mut retained = 0usize;
    let mut node = 0u32;
    loop {
        poll(cancelled)?;
        let base = tree
            .allocated_bytes()
            .saturating_add(visited.len())
            .saturating_add(stack.capacity().saturating_mul(size_of::<Branch<N>>()))
            .saturating_add(retained);
        let current = state_bytes(&states);
        if base.saturating_add(current) > limit {
            return Ok(false);
        }
        #[cfg(test)]
        {
            work.checkpoint_peak_bytes = work.checkpoint_peak_bytes.max(base.saturating_add(current));
        }
        let mut child = tree.first_child(node);
        if let Some(index) = tree.terminal(node) {
            let mut scores = [0; N];
            if child == 0 {
                for ((state, recipe), score) in states.iter_mut().zip(recipes).zip(&mut scores) {
                    *score = state.finish(recipe, work, cancelled)?;
                }
                if base.saturating_add(state_bytes(&states)) > limit {
                    return Ok(false);
                }
            } else {
                if base.saturating_add(current.saturating_mul(2)) > limit {
                    return Ok(false);
                }
                let mut terminal = states.clone();
                for ((state, recipe), score) in terminal.iter_mut().zip(recipes).zip(&mut scores) {
                    *score = state.finish(recipe, work, cancelled)?;
                }
                let bytes = base.saturating_add(current).saturating_add(state_bytes(&terminal));
                if bytes > limit {
                    return Ok(false);
                }
                #[cfg(test)]
                {
                    work.checkpoint_peak_bytes = work.checkpoint_peak_bytes.max(bytes);
                }
            }
            poll(cancelled)?;
            visit(index, scores)?;
            visited[index] = true;
        }
        if child == 0 {
            let Some(branch) = stack.pop() else {
                return Ok(true);
            };
            retained = retained.saturating_sub(branch.bytes);
            states = branch.states;
            child = branch.child;
        }
        let next = tree.next_sibling(child);
        if next != 0 {
            let current = state_bytes(&states);
            let bytes = tree
                .allocated_bytes()
                .saturating_add(visited.len())
                .saturating_add(stack.capacity().saturating_add(1).saturating_mul(size_of::<Branch<N>>()))
                .saturating_add(retained)
                .saturating_add(current.saturating_mul(2));
            if bytes > limit {
                return Ok(false);
            }
            stack.try_reserve_exact(1).map_err(|_| Failure::Decline(LuckScoreEquivalenceDecline::Capacity))?;
            if tree
                .allocated_bytes()
                .saturating_add(visited.len())
                .saturating_add(stack.capacity().saturating_mul(size_of::<Branch<N>>()))
                .saturating_add(retained)
                .saturating_add(current.saturating_mul(2))
                > limit
            {
                return Ok(false);
            }
            retained = retained.saturating_add(current);
            stack.push(Branch { states: states.clone(), child: next, bytes: current });
            #[cfg(test)]
            {
                work.checkpoint_peak_bytes = work.checkpoint_peak_bytes.max(bytes);
            }
        }
        for (state, recipe) in states.iter_mut().zip(recipes) {
            state.edge(recipe, tree.edge(child), work, cancelled)?;
        }
        node = child;
    }
}

pub(in super::super) fn evaluate_support<const N: usize>(
    recipes: [&Recipe; N],
    support: &TimelineSupport,
    work: &mut Work,
    cancelled: &mut impl FnMut() -> bool,
    visit: impl FnMut(usize, [i32; N]) -> Checked<()>,
) -> Checked<()> {
    evaluate_support_with_limit(recipes, support, work, cancelled, MAX_CHECKPOINT_BYTES, visit)
}

pub(in super::super) fn evaluate_support_with_limit<const N: usize>(
    recipes: [&Recipe; N],
    support: &TimelineSupport,
    work: &mut Work,
    cancelled: &mut impl FnMut() -> bool,
    limit: usize,
    mut visit: impl FnMut(usize, [i32; N]) -> Checked<()>,
) -> Checked<()> {
    poll(cancelled)?;
    let tree = native(support.tree(cancelled), LuckScoreEquivalenceDecline::ProbabilityDomain)?
        .ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Cancelled))?;
    let mut visited = Vec::new();
    visited.try_reserve_exact(support.len()).map_err(|_| Failure::Decline(LuckScoreEquivalenceDecline::Capacity))?;
    visited.resize(support.len(), false);
    if !walk(recipes, &tree, work, cancelled, limit, &mut visited, &mut visit)? {
        drop(tree);
        for (index, done) in visited.iter_mut().enumerate() {
            if *done {
                continue;
            }
            poll(cancelled)?;
            let path = native(support.path(index), LuckScoreEquivalenceDecline::Capacity)?;
            let mut scores = [0; N];
            for (recipe, score) in recipes.into_iter().zip(&mut scores) {
                *score = recipe.evaluate(&path, work, cancelled)?;
            }
            poll(cancelled)?;
            visit(index, scores)?;
            *done = true;
        }
    }
    if visited.contains(&false) {
        return refusal();
    }
    poll(cancelled)
}
