//! The lottery-state distribution and its propagation through a transcript.

use super::*;

pub(super) struct DpResult<W> {
    pub(super) probe_transitions: Vec<u8>,
    pub(super) rush_transitions: Vec<u16>,
    pub(super) steps: Vec<(i32, W)>,
    pub(super) frame_queries: Vec<(i32, [W; 3])>,
    pub(super) probes: Vec<bool>,
    pub(super) probe_starts: F64Interval,
    pub(super) range_moments: Vec<LuckRangeMoments>,
    pub(super) peak_states: usize,
    pub(super) transitions: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Chain {
    pub(super) gauge: i32,
    pub(super) lots: i32,
    pub(super) next: i8,
    pub(super) rush: u8,
    pub(super) maximum: i64,
}

impl Chain {
    pub(super) fn of(score: &LuckScore) -> Self {
        Self {
            gauge: score.gauge,
            lots: score.lot_count,
            next: score.next as i8,
            // From the fourth Critical onwards the table is always CHANCE_LOW. No retained mechanism reads
            // the exact count; saturation avoids retaining an unbounded, irrelevant history.
            rush: score.rush_combo.min(4) as u8,
            maximum: score.gauge_max,
        }
    }

    pub(super) fn score(self, template: &LuckScore) -> LuckScore {
        let mut score = template.clone();
        score.gauge = self.gauge;
        score.lot_count = self.lots;
        score.next = i64::from(self.next);
        score.rush_combo = i32::from(self.rush);
        score.gauge_max = self.maximum;
        score
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct State {
    /// At most one Luck range can be active in the supported scheduling domain. Finished ranges are
    /// reset before the next range starts, and future ranges still have their deterministic templates.
    /// Keeping only the active chain therefore forgets no information used by a future transition.
    pub(super) chain: Chain,
    /// All supported guarantees last one draw and share their release. A draw consumes every enabled one,
    /// so their maximum is a sufficient state (not an independent guarantee for each holder).
    pub(super) minimum: i8,
    pub(super) miss_used: bool,
    pub(super) previous_miss: bool,
    pub(super) frame_lot: bool,
    pub(super) frame_miss: bool,
    pub(super) previous_critical: bool,
    pub(super) frame_critical: bool,
    pub(super) rush: bool,
    /// Chart-time rush while scanning this frame's note times. A FINISH disable at frame time must not
    /// remove the rush from earlier chart times in that same frame.
    pub(super) query_rush: bool,
    pub(super) score: bool,
    pub(super) score_before: bool,
    /// Within a frame: the Rush once the frame's start is processed, and whether a lottery of the frame changed it.
    pub(super) rush_start: bool,
    pub(super) rush_filed: bool,
}

impl std::hash::Hash for State {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        // Preserve every Eq field but feed the hasher three words instead of separately hashing each
        // byte-sized flag. The signed fields keep all their original bits; this imposes no gauge or lot cap.
        hasher.write_i64(self.chain.maximum);
        hasher.write_u64(u64::from(self.chain.gauge as u32) | (u64::from(self.chain.lots as u32) << 32));
        hasher.write_u32(
            u32::from(self.chain.next as u8)
                | (u32::from(self.chain.rush) << 8)
                | (u32::from(self.minimum as u8) << 16)
                | ((self.miss_used as u32) << 24)
                | ((self.previous_miss as u32) << 25)
                | ((self.frame_lot as u32) << 26)
                | ((self.frame_miss as u32) << 27)
                | ((self.rush as u32) << 28)
                | ((self.query_rush as u32) << 29)
                | ((self.score as u32) << 30)
                | ((self.score_before as u32) << 31),
        );
        if self.previous_critical || self.frame_critical {
            hasher.write_u8(u8::from(self.previous_critical) | (u8::from(self.frame_critical) << 1));
        }
    }
}

pub(super) type Distribution<M = f64> = FxHashMap<State, M>;

/// The lottery state a note's score reads, relative to its native frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// A note before its frame's time, or a note at the frame's time scored before the frame's skill boundary: the
    /// Rush seen by the frame's score queries and the probe class before the frame.
    BeforeFrame,
    /// A note at its frame's time once the frame's lottery has filed its commands: the Rush and probe class after
    /// the frame.
    AtFrame,
    /// A note at its frame's time scored by a query of that frame before its skill boundary and lottery: the Rush
    /// at the frame's time and the probe class before the frame.
    BeforeSkills,
}

impl Phase {
    pub(super) fn reads(self, state: &State) -> (bool, bool) {
        match self {
            Phase::BeforeFrame => (state.query_rush, state.score_before),
            Phase::AtFrame => (state.rush, state.score),
            Phase::BeforeSkills => (state.rush, state.score_before),
        }
    }
}

pub(super) struct Dp<'a, M: Mass> {
    pub(super) templates: Vec<LuckScore>,
    active_range: Option<usize>,
    pub(super) machine: &'a LotteryMachine,
    pub(super) draws: FxHashMap<(usize, i32, i8), Draws<M>>,
    pub(super) dist: Distribution<M>,
    spare: Distribution<M>,
    peak: usize,
    pub(super) transitions: u64,
    pub(super) range_moments: Vec<LuckRangeMoments>,
    track_critical: bool,
}

impl<'a, M: Mass> Dp<'a, M> {
    pub(super) fn new(templates: Vec<LuckScore>, machine: &'a LotteryMachine, collect_moments: bool) -> Self {
        let mut dist = Distribution::<M>::default();
        dist.insert(State::default(), M::ONE);
        Self {
            range_moments: if collect_moments {
                vec![LuckRangeMoments::default(); templates.len()]
            } else {
                Vec::new()
            },
            track_critical: false,
            templates,
            active_range: None,
            machine,
            draws: FxHashMap::default(),
            dist,
            spare: Distribution::default(),
            peak: 1,
            transitions: 0,
        }
    }

    pub(super) fn push(&mut self, out: &mut Distribution<M>, state: State, probability: M) -> Result<(), Error> {
        if probability.possible() {
            let existing = out.entry(state).or_insert(M::ZERO);
            *existing = existing.merge(probability);
            self.transitions = self
                .transitions
                .checked_add(1)
                .ok_or_else(|| Error::Capacity("LUCK DP transitions overflow".into()))?;
        }
        Ok(())
    }

    fn replace(&mut self, out: Distribution<M>) -> Result<(), Error> {
        self.peak = self.peak.max(out.len());
        if out.len() > 2_000_000 {
            return Err(Error::Capacity("LUCK DP exceeds two million states".into()));
        }
        self.dist = out;
        Ok(())
    }

    pub(super) fn map(&mut self, mut transform: impl FnMut(State) -> Result<State, Error>) -> Result<(), Error> {
        self.map_masses(|state, _| transform(state))
    }

    /// [`Self::map`] with the mass of each state.
    fn map_masses(&mut self, mut transform: impl FnMut(State, M) -> Result<State, Error>) -> Result<(), Error> {
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            self.push(&mut out, transform(state, probability)?, probability)?;
        }
        self.spare = previous;
        self.replace(out)
    }

    pub(super) fn action(&mut self, action: Action<M>, target: usize) -> Result<(), Error> {
        debug_assert_eq!(self.active_range, Some(target));
        if let Action::CriticalPoints { value, chance } | Action::StartPoints { value, chance } = action {
            let mass = self
                .dist
                .iter()
                .filter(|(state, _)| matches!(action, Action::StartPoints { .. }) || state.previous_critical)
                .fold(M::ZERO, |sum, (_, &probability)| sum.merge(probability));
            let points = mass.multiply(chance).interval().multiply(F64Interval::integer(i128::from(value)));
            self.range_moments[target].luck_points = self.range_moments[target].luck_points.add(points);
            return Ok(());
        }
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            // All accepted Miss rows share the same once/reset trigger. Leave their used flag untouched
            // until every row has run so that multiple eligible rows each apply once in native order.
            if matches!(action, Action::MissGauge { .. }) && (!state.previous_miss || state.miss_used) {
                let existing = out.entry(state).or_insert(M::ZERO);
                *existing = existing.merge(probability);
                continue;
            }
            let mut next = state;
            let chance = match action {
                Action::StartMinimum { result, chance } => {
                    next.minimum = next.minimum.max(result);
                    chance
                }
                Action::StartGauge { value, .. } | Action::MissGauge { value } => {
                    let mut score = state.chain.score(&self.templates[target]);
                    let gauge = (score.gauge_max as i128 * value as i128) as i32;
                    score.add_gauge(floor_to_i32(gauge as f32 / 10000f32))?;
                    next.chain = Chain::of(&score);
                    if let Action::StartGauge { chance, .. } = action { chance } else { M::ONE }
                }
                Action::CriticalPoints { .. } | Action::StartPoints { .. } => unreachable!(),
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

    pub(super) fn draw(&mut self, kind: usize, buff: i32, minimum: i8) -> Result<Draws<M>, Error> {
        let key = (kind, buff, minimum);
        if let Some(draws) = self.draws.get(&key) {
            return Ok(*draws);
        }
        let draws = Draws::of(&M::bonus(self.machine, kind, buff, minimum)?)?;
        self.draws.insert(key, draws);
        Ok(draws)
    }

    pub(super) fn consume(
        &mut self,
        mut state: State,
        range: usize,
        buff: i32,
        p: M,
        out: &mut Distribution<M>,
    ) -> Result<(), Error> {
        state.chain.lots -= 1;
        let first = if state.chain.next == -1 {
            let draws = self.draw(0, buff, state.minimum)?;
            state.minimum = 0;
            draws
        } else {
            Draws::one(i64::from(state.chain.next))
        };
        for (pn, result) in first.iter() {
            let mut next = state;
            let mut score = next.chain.score(&self.templates[range]);
            if score.rush_combo == 0 || result != 3 {
                next.rush = result == 3;
                next.query_rush = next.rush;
            }
            // Native files a Rush command exactly when a lottery adds or disables the Rush handle.
            next.rush_filed |= next.rush != state.rush;
            score.add_score(result)?;
            if let Some(moments) = self.range_moments.get_mut(range) {
                let mass = p.multiply(pn).interval();

                moments.lot_results[result as usize] = moments.lot_results[result as usize].add(mass);
                let points = if result == 0 { 0 } else { super::gekisou::BONUS_POINT_BY_RESULT[result as usize - 1] };
                moments.luck_points = moments.luck_points.add(mass.multiply(F64Interval::integer(i128::from(points))));
            }
            next.chain = Chain::of(&score);
            next.frame_lot = true;
            next.frame_miss |= result == 0;
            next.frame_critical |= self.track_critical && result == 3;
            let draws = self.draw(score.current_lot_type(), buff, next.minimum)?;
            next.minimum = 0;
            for (pr, result) in draws.iter() {
                let mut after = next;
                after.chain.next = result as i8;
                self.push(out, after, p.multiply(pn).multiply(pr))?;
            }
        }
        Ok(())
    }

    pub(super) fn note(
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
                let mut score = next.chain.score(&self.templates[range]);
                score.add_gauge(gauge)?;
                next.chain = Chain::of(&score);
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

    pub(super) fn pending(&mut self, range: usize, buff: i32) -> Result<(), Error> {
        debug_assert_eq!(self.active_range, Some(range));
        let mut out = std::mem::take(&mut self.spare);
        let mut previous = std::mem::take(&mut self.dist);
        for (state, probability) in previous.drain() {
            if state.chain.lots > 0 && !state.frame_lot {
                self.consume(state, range, buff, probability, &mut out)?;
            } else {
                self.push(&mut out, state, probability)?;
            }
        }
        self.spare = previous;
        self.replace(out)
    }

    pub(super) fn weights(&self, probes: &[bool], phase: Phase) -> M::Weights {
        M::weights(&self.dist, probes, phase)
    }
}

/// Propagate the lottery-state distribution through a transcript.
pub(super) fn propagate<M: Mass>(transcript: &Transcript<M>) -> Result<DpResult<M::Weights>, Error> {
    Ok(propagate_cancellable(transcript, &mut || false)?.expect("complete propagation"))
}

pub(super) fn propagate_cancellable<M: Mass>(
    transcript: &Transcript<M>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<DpResult<M::Weights>>, Error> {
    let t = transcript;
    let mut dp = Dp::<M>::new(t.templates.clone(), &t.machine, t.collect_moments);
    dp.track_critical = t.actions.iter().any(|a| matches!(a, Action::CriticalPoints { .. }));
    // Each note hit and each pending frame consumes at most one result. Critical point commands fire at
    // most once per recorded action. These bounds prove that additive indicators cannot wrap native i32.
    if t.collect_moments {
        let draws = t.hits.len() as i128 + t.frames.iter().map(|f| i128::from(f.repeat)).sum::<i128>();
        let mut lower = 0i128;
        let mut upper = 10 * draws;
        let mut from = 0;
        for frame in &t.frames {
            for action in &t.actions[from..frame.actions] {
                if let Action::CriticalPoints { value, chance } | Action::StartPoints { value, chance } = *action
                    && chance.possible()
                {
                    let value = i128::from(value) * i128::from(frame.repeat);
                    lower += value.min(0);
                    upper += value.max(0);
                }
            }
            from = frame.actions;
        }
        if t.templates.iter().any(|s| {
            i128::from(s.total_bonus_point) + lower < i128::from(i32::MIN)
                || i128::from(s.total_bonus_point) + upper > i128::from(i32::MAX)
        }) {
            return Err(Error::Unsupported("LUCK DP: additive lottery points may wrap".into()));
        }
    }
    let mut steps: Vec<(i32, M::Weights)> = Vec::new();
    let mut frame_queries: Vec<(i32, [M::Weights; 3])> = Vec::new();
    let mut probe_transitions = Vec::new();
    let mut probe_starts = F64Interval::ZERO;
    let mut rush_transitions = Vec::new();
    let mut identity = 1u8;
    // The Rush transitions of a frame in which no lottery runs: every state keeps its Rush.
    let mut kept = 1u16;
    let mut previous_lot = false;
    let mut queued = vec![false; t.luck.len()];
    let (mut notes_from, mut actions_from, mut pending_from) = (0usize, 0usize, 0usize);
    for frame in &t.frames {
        if cancelled() {
            return Ok(None);
        }
        let hits_from = notes_from.checked_sub(1).map_or(0, |i| t.notes[i].hits);
        let notes = &t.notes[notes_from..frame.notes];
        let actions = &t.actions[actions_from..frame.actions];
        let pending = &t.pending[pending_from..frame.pending];
        (notes_from, actions_from, pending_from) = (frame.notes, frame.actions, frame.pending);
        let (finish, complete, gate, current_luck) = (frame.finish, frame.complete, frame.gate, frame.current_luck);
        for repeat in 0..frame.repeat {
            if repeat.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            if notes.is_empty()
                && frame.start.is_none()
                && !complete
                && !finish
                && !previous_lot
                && !pending.iter().any(|&(range, _)| queued[range])
            {
                // No lottery or dependent skill state can change here. A consumed lot forces the FOLLOWING
                // frame through the DP for 7021 and previous-frame 7000.
                probe_transitions.push(identity);
                rush_transitions.push(kept);
                continue;
            }
            if let Some(range) = frame.start {
                debug_assert!(dp.active_range.is_none());
                dp.active_range = Some(range);
            }
            let starting_chain = frame.start.map(|range| Chain::of(&dp.templates[range]));
            let mut edges = 0u8;
            let mut follows = true;
            let mut started = M::ZERO;
            // The certified distribution retains every state with positive possible probability.
            dp.map_masses(|mut state, probability| {
                let before = state.score;
                if let Some(chain) = starting_chain {
                    state.chain = chain;
                }
                state.query_rush = state.rush;
                state.score_before = state.score;
                state.frame_lot = false;
                state.frame_miss = false;
                state.frame_critical = false;
                if finish {
                    state.rush = false;
                }
                if gate {
                    if current_luck {
                        state.score = state.chain.rush != 0;
                    }
                    if complete || finish {
                        state.score = false;
                    }
                }
                edges |= 1 << (2 * usize::from(before) + usize::from(state.score));
                if !before && state.score {
                    started = started.merge(probability);
                }
                follows &= state.score == state.rush;
                state.rush_start = state.rush;
                state.rush_filed = false;
                if complete {
                    state.minimum = 0;
                    state.miss_used = false;
                }
                Ok(state)
            })?;
            probe_transitions.push(edges);
            probe_starts = probe_starts.add(started.interval());
            // Subsequent actions and lotteries do not alter this direct-probe class.
            identity = u8::from(edges & 0b0101 != 0) | (u8::from(edges & 0b1010 != 0) << 3);
            // The frame's score queries run after the FINISH disable and the skill boundary, before any lottery.
            if !notes.is_empty() {
                frame_queries.push((
                    frame.time_ms,
                    [Phase::BeforeFrame, Phase::AtFrame, Phase::BeforeSkills].map(|phase| dp.weights(&t.probes, phase)),
                ));
            }
            if gate && frame.target >= 0 {
                for &action in actions {
                    dp.action(action, frame.target as usize)?;
                }
                if t.miss_rows {
                    dp.map(|mut state| {
                        state.miss_used |= state.previous_miss;
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
                    for h in &t.hits[hit..note.hits] {
                        dp.note(h.range, note.note_type, note.judgement, h.buff, h.speed, h.consumes)?;
                    }
                    hit = note.hits;
                }
                if time < frame.time_ms {
                    let values = dp.weights(&t.probes, Phase::BeforeFrame);
                    if steps.last().is_none_or(|last| last.1 != values) {
                        steps.push((time, values));
                    }
                }
                i = end;
            }
            for &(range, buff) in pending {
                dp.pending(range, buff)?;
            }
            let mut lottery = 0u16;
            for state in dp.dist.keys() {
                lottery |=
                    1 << (4 * u16::from(state.rush_start) + 2 * u16::from(state.rush) + u16::from(state.rush_filed));
            }
            rush_transitions
                .push(lottery | if finish { RUSH_FINISH } else { 0 } | if follows { RUSH_PROBE } else { 0 });
            kept = u16::from(lottery & 0x33 != 0) | u16::from(lottery & 0xcc != 0) << 6;
            if notes.last().is_some_and(|note| note.time_ms == frame.time_ms) {
                let values = dp.weights(&t.probes, Phase::AtFrame);
                if steps.last().is_none_or(|last| last.1 != values) {
                    steps.push((frame.time_ms, values));
                }
            }
            previous_lot = dp.dist.keys().any(|state| state.frame_lot);
            dp.map(|mut state| {
                state.previous_miss = state.frame_miss;
                state.previous_critical = state.frame_critical;
                state.query_rush = state.rush;
                state.score_before = state.score;
                state.frame_miss = false;
                state.frame_critical = false;
                state.frame_lot = false;
                state.rush_start = false;
                state.rush_filed = false;
                if finish {
                    state.chain = Chain::default();
                }
                Ok(state)
            })?;
            if finish {
                dp.active_range = None;
            }
            queued.fill(false);
            if let Some(range) = dp.active_range {
                queued[range] = dp.dist.keys().any(|state| state.chain.lots > 0);
            }
        }
    }
    if let Some(failure) = &t.failure {
        return Err(failure.clone());
    }
    Ok(Some(DpResult {
        probe_transitions,
        rush_transitions,
        steps,
        frame_queries,
        probes: t.probes.clone(),
        probe_starts,
        range_moments: dp.range_moments,
        peak_states: dp.peak,
        transitions: dp.transitions,
    }))
}
