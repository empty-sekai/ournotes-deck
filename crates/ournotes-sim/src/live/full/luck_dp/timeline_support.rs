//! Complete support of score-observable LUCK histories under an admitted native controller transcript.
//!
//! Every live key retains the original controller State and its entire ordered Rush/probe timeline. Only
//! terminal states are grouped by timeline. Integer-table branches are never sampled or dropped: outward
//! masses only establish possible support, and equal numerical masses never establish a common law.
//! The caller separately admits the ordinary score recorder, probe emissions and exact controller coupling.
use super::*;
use std::mem::size_of;

const MAX_STATES: usize = 100_000;
const MAX_PREFIXES: usize = 100_000;
const MAX_TIMELINES: usize = 1024;
const MAX_PATH_EDGES: usize = 4096;
const MAX_FRAMES: usize = 100_000;
const MAX_TRANSITIONS: u64 = 10_000_000;
const MAX_ESTIMATED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TimelineKind {
    Rush,
    Probe,
}

/// Original playback frame, native phase and unclamped command chart time. Repeated edges at the same
/// time remain distinct ordered entries, including an on/off pair within one native note or frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TimelineEdge {
    pub frame: usize,
    pub stage: u8,
    pub chart_time: i32,
    pub kind: TimelineKind,
    pub on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Prefix {
    previous: u32,
    edge: TimelineEdge,
}

/// Private construction requires complete transition, original-clock and range-finish coverage.
/// This is a support capability only; it contains no native score or claim that two decks are equal.
pub(super) struct TimelineSupport {
    prefixes: Vec<Prefix>,
    terminals: Vec<u32>,
    pub transitions: u64,
}

impl TimelineSupport {
    pub fn len(&self) -> usize {
        self.terminals.len()
    }

    pub fn path(&self, index: usize) -> Result<Vec<TimelineEdge>, Error> {
        let mut id = *self
            .terminals
            .get(index)
            .ok_or_else(|| Error::Domain("LUCK timeline index outside complete support".into()))?;
        let mut path = Vec::new();
        while id != 0 {
            if path.len() >= MAX_PATH_EDGES {
                return Err(Error::Capacity("LUCK observable timeline path cap".into()));
            }
            let prefix = self
                .prefixes
                .get(id as usize - 1)
                .ok_or_else(|| Error::Domain("LUCK timeline prefix missing".into()))?;
            if prefix.previous >= id {
                return Err(Error::Domain("LUCK timeline prefix is not acyclic".into()));
            }
            path.try_reserve(1).map_err(|_| Error::Capacity("LUCK timeline path allocation".into()))?;
            path.push(prefix.edge);
            id = prefix.previous;
        }
        path.reverse();
        Ok(path)
    }
}

struct Trace<'a> {
    times: &'a [i32],
    cancelled: &'a mut dyn FnMut() -> bool,
    stopped: bool,
    next_frame: usize,
    frame_time: i32,
    chart_time: i32,
    stage: u8,
    has_probes: bool,
    edges: FxHashMap<Prefix, u32>,
    prefixes: Vec<Prefix>,
    terminals: Vec<u32>,
    transitions: u64,
}

impl Trace<'_> {
    fn poll(&mut self) -> Result<(), Error> {
        if (self.cancelled)() {
            self.stopped = true;
            return Err(Error::Unsupported("LUCK timeline support cancelled".into()));
        }
        Ok(())
    }

    fn begin_frame(&mut self) -> Result<(), Error> {
        self.poll()?;
        self.frame_time = *self
            .times
            .get(self.next_frame)
            .ok_or_else(|| Error::Domain("LUCK timeline original frame clock overflow".into()))?;
        self.chart_time = self.frame_time;
        self.next_frame += 1;
        Ok(())
    }

    fn edge(&mut self, previous: u32, kind: TimelineKind, on: bool) -> Result<u32, Error> {
        let prefix = Prefix {
            previous,
            edge: TimelineEdge { frame: self.next_frame - 1, stage: self.stage, chart_time: self.chart_time, kind, on },
        };
        if let Some(&id) = self.edges.get(&prefix) {
            return Ok(id);
        }
        if self.edges.len() >= MAX_PREFIXES {
            return Err(Error::Capacity("LUCK observable timeline prefix cap".into()));
        }
        let id = u32::try_from(self.prefixes.len() + 1).map_err(|_| Error::Capacity("LUCK timeline ID".into()))?;
        self.edges.try_reserve(1).map_err(|_| Error::Capacity("LUCK timeline prefix allocation".into()))?;
        self.prefixes.try_reserve(1).map_err(|_| Error::Capacity("LUCK timeline prefix allocation".into()))?;
        self.prefixes.push(prefix);
        self.edges.insert(prefix, id);
        Ok(id)
    }

    fn estimate(&mut self, current: usize, spare: usize, draws: usize) -> Result<(), Error> {
        // Conservative retained-storage estimate for both full-state maps, interning map, prefix vector
        // and draw cache. It is not a process RSS bound; the immutable native transcript is owned outside.
        let bytes = current
            .saturating_add(spare)
            .saturating_add(self.edges.capacity())
            .saturating_mul(128)
            .saturating_add(self.prefixes.capacity().saturating_mul(size_of::<Prefix>()))
            .saturating_add(draws.saturating_mul(256))
            .saturating_add(MAX_TIMELINES * size_of::<u32>())
            .saturating_add(MAX_PATH_EDGES * size_of::<TimelineEdge>());
        if bytes > MAX_ESTIMATED_BYTES {
            return Err(Error::Capacity("LUCK observable timeline retained-byte cap".into()));
        }
        Ok(())
    }
}

/// The shared controller State remains intact; the extra prefix is never substituted for hidden gauge,
/// prefetched result, minimum guarantee, once-Miss history or any other future-transition input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct State {
    inner: super::State,
    timeline: u32,
}

/// A bounded support traversal of the original admitted nominal transition graph. Cancellation returns
/// None. Capacity or unsupported histories never expose a partial TimelineSupport to the score consumer.
pub(super) fn complete_timeline_support(
    transcript: &Transcript<ProbabilityMass>,
    original_frame_times: &[i32],
    has_probes: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<TimelineSupport>, Error> {
    if cancelled() {
        return Ok(None);
    }
    if original_frame_times.is_empty() || original_frame_times.len() > MAX_FRAMES {
        return Err(Error::Capacity("LUCK observable timeline original-frame cap".into()));
    }
    if let Some(failure) = &transcript.failure {
        return Err(failure.clone());
    }
    for action in &transcript.actions {
        let chance = match action {
            Action::StartGauge { chance, .. } | Action::StartMinimum { chance, .. } => *chance,
            Action::MissGauge { .. } => ProbabilityMass::ONE,
        };
        if chance != ProbabilityMass::ZERO && chance != ProbabilityMass::ONE {
            return Err(Error::Unsupported("LUCK timeline coupling requires exact action truth".into()));
        }
    }
    let mut trace = Trace {
        times: original_frame_times,
        cancelled,
        stopped: false,
        next_frame: 0,
        frame_time: 0,
        chart_time: 0,
        stage: 0,
        has_probes,
        edges: FxHashMap::default(),
        prefixes: Vec::new(),
        terminals: Vec::new(),
        transitions: 0,
    };
    let result = propagate_support(transcript, &mut trace);
    if trace.stopped {
        return Ok(None);
    }
    result?;
    if (trace.cancelled)() {
        return Ok(None);
    }
    Ok(Some(TimelineSupport { prefixes: trace.prefixes, terminals: trace.terminals, transitions: trace.transitions }))
}

type Distribution<M = f64> = FxHashMap<State, M>;

struct Dp<'a, 'c, M: Mass> {
    trace: &'a mut Trace<'c>,
    templates: Vec<LuckScore>,
    active_range: Option<usize>,
    machine: &'a LotteryMachine,
    draws: FxHashMap<(usize, i32, i8), Draws<M>>,
    dist: Distribution<M>,
    spare: Distribution<M>,
    transitions: u64,
}

impl<'a, 'c, M: Mass> Dp<'a, 'c, M> {
    fn new(templates: Vec<LuckScore>, machine: &'a LotteryMachine, trace: &'a mut Trace<'c>) -> Self {
        let mut dist = Distribution::<M>::default();
        dist.insert(State::default(), M::ONE);
        Self {
            trace,
            templates,
            active_range: None,
            machine,
            draws: FxHashMap::default(),
            dist,
            spare: Distribution::default(),
            transitions: 0,
        }
    }

    fn push(&mut self, out: &mut Distribution<M>, state: State, probability: M) -> Result<(), Error> {
        if probability.possible() {
            if out.len() >= MAX_STATES && !out.contains_key(&state) {
                return Err(Error::Capacity("LUCK observable timeline state cap".into()));
            }
            if !out.contains_key(&state) {
                out.try_reserve(1).map_err(|_| Error::Capacity("LUCK timeline state allocation".into()))?;
            }
            let existing = out.entry(state).or_insert(M::ZERO);
            *existing = existing.merge(probability);
            self.transitions = self
                .transitions
                .checked_add(1)
                .ok_or_else(|| Error::Capacity("LUCK timeline transitions overflow".into()))?;
        }
        self.trace.transitions = self.transitions;
        if self.transitions.is_multiple_of(1024) {
            self.trace.poll()?;
        }
        if self.transitions > MAX_TRANSITIONS {
            return Err(Error::Capacity("LUCK observable timeline transition cap".into()));
        }
        Ok(())
    }

    fn replace(&mut self, out: Distribution<M>) -> Result<(), Error> {
        self.trace.estimate(out.capacity(), self.spare.capacity(), self.draws.capacity())?;
        self.dist = out;
        Ok(())
    }

    fn map(&mut self, mut transform: impl FnMut(State, &mut Trace<'_>) -> Result<State, Error>) -> Result<(), Error> {
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            let state = transform(state, self.trace)?;
            self.push(&mut out, state, probability)?;
        }
        self.spare = previous;
        self.replace(out)
    }

    fn action(&mut self, action: Action<M>, target: usize) -> Result<(), Error> {
        debug_assert_eq!(self.active_range, Some(target));
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            // All accepted Miss rows share the same once/reset trigger. Leave their used flag untouched
            // until every row has run so that multiple eligible rows each apply once in native order.
            if matches!(action, Action::MissGauge { .. }) && (!state.inner.previous_miss || state.inner.miss_used) {
                self.push(&mut out, state, probability)?;
                continue;
            }
            let mut next = state;
            let chance = match action {
                Action::StartMinimum { result, chance } => {
                    next.inner.minimum = next.inner.minimum.max(result);
                    chance
                }
                Action::StartGauge { value, .. } | Action::MissGauge { value } => {
                    let mut score = state.inner.chain.score(&self.templates[target]);
                    let gauge = (score.gauge_max as i128 * value as i128) as i32;
                    score.add_gauge(floor_to_i32(gauge as f32 / 10000f32))?;
                    next.inner.chain = Chain::of(&score);
                    if let Action::StartGauge { chance, .. } = action { chance } else { M::ONE }
                }
            };
            self.push(&mut out, next, probability.multiply(chance))?;
            let complement = chance.complement();
            if complement.possible() {
                self.push(&mut out, state, probability.multiply(complement))?;
            }
        }
        self.spare = previous;
        self.replace(out)
    }

    fn draw(&mut self, kind: usize, buff: i32, minimum: i8) -> Result<Draws<M>, Error> {
        let key = (kind, buff, minimum);
        if let Some(draws) = self.draws.get(&key) {
            return Ok(*draws);
        }
        let draws = Draws::of(&M::bonus(self.machine, kind, buff, minimum)?)?;
        self.draws.try_reserve(1).map_err(|_| Error::Capacity("LUCK timeline draw allocation".into()))?;
        self.draws.insert(key, draws);
        Ok(draws)
    }

    fn consume(
        &mut self,
        mut state: State,
        range: usize,
        buff: i32,
        p: M,
        out: &mut Distribution<M>,
    ) -> Result<(), Error> {
        state.inner.chain.lots -= 1;
        let first = if state.inner.chain.next == -1 {
            let draws = self.draw(0, buff, state.inner.minimum)?;
            state.inner.minimum = 0;
            draws
        } else {
            Draws::one(i64::from(state.inner.chain.next))
        };
        for (pn, result) in first.iter() {
            let mut next = state;
            let mut score = next.inner.chain.score(&self.templates[range]);
            if score.rush_combo == 0 || result != 3 {
                next.inner.rush = result == 3;
                next.inner.query_rush = next.inner.rush;
            }
            if next.inner.rush != state.inner.rush {
                next.timeline = self.trace.edge(next.timeline, TimelineKind::Rush, next.inner.rush)?;
            }
            score.add_score(result)?;
            next.inner.chain = Chain::of(&score);
            next.inner.frame_lot = true;
            next.inner.frame_miss |= result == 0;
            let draws = self.draw(score.current_lot_type(), buff, next.inner.minimum)?;
            next.inner.minimum = 0;
            for (pr, result) in draws.iter() {
                let mut after = next;
                after.inner.chain.next = result as i8;
                self.push(out, after, p.multiply(pn).multiply(pr))?;
            }
        }
        Ok(())
    }

    fn note(
        &mut self,
        range: usize,
        note_type: i32,
        judgement: i32,
        buff: i32,
        speed: f32,
        consumes: bool,
    ) -> Result<(), Error> {
        if matches!(judgement, 0 | 7) {
            return Ok(());
        }
        debug_assert_eq!(self.active_range, Some(range));
        let base: Vec<_> = M::base(self.machine, note_type, judgement)?
            .into_iter()
            .map(|(probability, point)| (probability, floor_to_i32((speed + 1f32) * point as f32)))
            .collect();
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            for &(pb, gauge) in &base {
                let mut next = state;
                let mut score = next.inner.chain.score(&self.templates[range]);
                score.add_gauge(gauge)?;
                next.inner.chain = Chain::of(&score);
                if consumes && score.lot_count > 0 {
                    self.consume(next, range, buff, probability.multiply(pb), &mut out)?;
                } else {
                    self.push(&mut out, next, probability.multiply(pb))?;
                }
            }
        }
        self.spare = previous;
        self.replace(out)
    }

    fn pending(&mut self, range: usize, buff: i32) -> Result<(), Error> {
        debug_assert_eq!(self.active_range, Some(range));
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            if state.inner.chain.lots > 0 && !state.inner.frame_lot {
                self.consume(state, range, buff, probability, &mut out)?;
            } else {
                self.push(&mut out, state, probability)?;
            }
        }
        self.spare = previous;
        self.replace(out)
    }
}

fn propagate_support(transcript: &Transcript<ProbabilityMass>, trace: &mut Trace<'_>) -> Result<(), Error> {
    let t = transcript;
    let mut dp = Dp::<ProbabilityMass>::new(t.templates.clone(), &t.machine, trace);
    let mut previous_lot = false;
    let mut queued = vec![false; t.luck.len()];
    let (mut notes_from, mut actions_from, mut pending_from) = (0usize, 0usize, 0usize);
    for frame in &t.frames {
        dp.trace.poll()?;
        if dp.trace.times.get(dp.trace.next_frame).copied() != Some(frame.time_ms) {
            return Err(Error::Domain("LUCK timeline original frame origin mismatch".into()));
        }
        let hits_from = notes_from.checked_sub(1).map_or(0, |i| t.notes[i].hits);
        let notes = &t.notes[notes_from..frame.notes];
        let actions = &t.actions[actions_from..frame.actions];
        let pending = &t.pending[pending_from..frame.pending];
        (notes_from, actions_from, pending_from) = (frame.notes, frame.actions, frame.pending);
        let (finish, complete, gate, current_luck) = (frame.finish, frame.complete, frame.gate, frame.current_luck);
        for _ in 0..frame.repeat {
            dp.trace.begin_frame()?;
            if notes.is_empty()
                && frame.start.is_none()
                && !complete
                && !finish
                && !previous_lot
                && !pending.iter().any(|&(range, _)| queued[range])
            {
                // No lottery or dependent skill state can change here. A consumed lot forces the FOLLOWING
                // frame through the DP for 7021 and previous-frame 7000.
                continue;
            }
            if let Some(range) = frame.start {
                if dp.active_range.is_some() {
                    return Err(Error::Unsupported("LUCK timeline ranges overlap".into()));
                }
                dp.active_range = Some(range);
            }
            let starting_chain = frame.start.map(|range| Chain::of(&dp.templates[range]));
            dp.map(|mut state, trace| {
                if let Some(chain) = starting_chain {
                    state.inner.chain = chain;
                }
                state.inner.query_rush = state.inner.rush;
                state.inner.score_before = state.inner.score;
                state.inner.frame_lot = false;
                state.inner.frame_miss = false;
                trace.stage = 0;
                trace.chart_time = trace.frame_time;
                if finish && state.inner.rush {
                    state.timeline = trace.edge(state.timeline, TimelineKind::Rush, false)?;
                    state.inner.rush = false;
                }
                let was_score = state.inner.score;
                if gate {
                    if current_luck {
                        state.inner.score = state.inner.chain.rush != 0;
                    }
                    if complete || finish {
                        state.inner.score = false;
                    }
                }
                if was_score != state.inner.score && trace.has_probes {
                    trace.stage = 1;
                    state.timeline = trace.edge(state.timeline, TimelineKind::Probe, state.inner.score)?;
                }
                if complete {
                    state.inner.minimum = 0;
                    state.inner.miss_used = false;
                }
                Ok(state)
            })?;
            if gate && frame.target >= 0 {
                for &action in actions {
                    dp.action(action, frame.target as usize)?;
                }
                if t.miss_rows {
                    dp.map(|mut state, _trace| {
                        state.inner.miss_used |= state.inner.previous_miss;
                        Ok(state)
                    })?;
                }
            }
            let mut hit = hits_from;
            let mut i = 0;
            while i < notes.len() {
                let time = notes[i].time_ms;
                let mut end = i + 1;
                while end < notes.len() && notes[end].time_ms == time {
                    end += 1;
                }
                for note in &notes[i..end] {
                    dp.trace.stage = 2;
                    dp.trace.chart_time = note.time_ms;
                    for h in &t.hits[hit..note.hits] {
                        dp.note(h.range, note.note_type, note.judgement, h.buff, h.speed, h.consumes)?;
                    }
                    hit = note.hits;
                }
                i = end;
            }
            dp.trace.stage = 3;
            dp.trace.chart_time = dp.trace.frame_time;
            for &(range, buff) in pending {
                dp.pending(range, buff)?;
            }
            previous_lot = dp.dist.keys().any(|state| state.inner.frame_lot);
            dp.map(|mut state, _trace| {
                state.inner.previous_miss = state.inner.frame_miss;
                state.inner.query_rush = state.inner.rush;
                state.inner.score_before = state.inner.score;
                state.inner.frame_miss = false;
                state.inner.frame_lot = false;
                if finish {
                    state.inner.chain = Chain::default();
                }
                Ok(state)
            })?;
            if finish {
                dp.active_range = None;
            }
            queued.fill(false);
            if let Some(range) = dp.active_range {
                queued[range] = dp.dist.keys().any(|state| state.inner.chain.lots > 0);
            }
        }
    }
    if let Some(failure) = &t.failure {
        return Err(failure.clone());
    }
    if dp.trace.next_frame != dp.trace.times.len() {
        return Err(Error::Domain("LUCK observable timeline compressed-frame coverage mismatch".into()));
    }
    if dp.active_range.is_some() {
        return Err(Error::Unsupported("LUCK observable timeline unfinished LUCK range".into()));
    }
    let mut seen = vec![false; dp.trace.edges.len() + 1];
    dp.trace
        .terminals
        .try_reserve_exact(MAX_TIMELINES)
        .map_err(|_| Error::Capacity("LUCK timeline terminal allocation".into()))?;
    let mut mass = ProbabilityMass::ZERO;
    for (state, probability) in &dp.dist {
        if !seen[state.timeline as usize] {
            seen[state.timeline as usize] = true;
            if dp.trace.terminals.len() >= MAX_TIMELINES {
                return Err(Error::Capacity("LUCK observable terminal timeline cap".into()));
            }
            dp.trace.terminals.push(state.timeline);
        }
        mass = mass.merge_disjoint(*probability);
    }
    if mass.interval().lower() > 1.0 || mass.interval().upper() < 1.0 {
        return Err(Error::Domain("LUCK observable timeline total mass excludes one".into()));
    }
    if dp.trace.terminals.is_empty() {
        return Err(Error::Domain("LUCK observable terminal support is empty".into()));
    }
    dp.trace.terminals.sort_unstable();
    dp.trace.poll()?;
    Ok(())
}
