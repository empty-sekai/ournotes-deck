//! Caches of certified lottery laws and recordings, and the order classes of formations.

use super::*;

/// Compiled recorder states for one immutable live context. The owning score session fixes the master,
/// chart, frame schedule, parameters and rank arrivals. Rows and their resolved checkers retain source row
/// identity, performer position and effect order. A life interpreter is identified by its complete ordered
/// performer input; every other interpreter input is fixed by the session.
#[derive(Default)]
pub(crate) struct RecordingCache {
    pub(super) entries: FxHashMap<Vec<u8>, std::sync::Arc<LuckDpCertifiedResult>>,
    bytes: usize,
}

/// The lottery law of a formation in one order, and the pairs of performer positions whose relative order it may
/// depend on: every order of the same performers that keeps the relative order of each pair has this law.
#[derive(Clone, Debug)]
pub(crate) struct OrderClass {
    pub law: std::sync::Arc<LuckDpCertifiedResult>,
    pub pairs: PositionPairs,
}

impl RecordingCache {
    pub(super) fn key(prepared: &PreparedRecording<ProbabilityMass>) -> Vec<u8> {
        let conditions = &prepared.cond;
        let activation_bits: Vec<_> = conditions
            .iter()
            .map(|skill| skill.updater.effects().iter().map(|effect| effect.act.to_bits()).collect::<Vec<_>>())
            .collect();
        let actions: Vec<_> = prepared
            .plan
            .actions
            .iter()
            .map(|(phase, action, condition)| {
                let (tag, value, bits) = match action {
                    Action::StartGauge { value, chance } => (0, *value, chance.bits()),
                    Action::StartMinimum { result, chance } => (1, i64::from(*result), chance.bits()),
                    Action::MissGauge { value } => (2, *value, ProbabilityMass::ONE.bits()),
                    Action::CriticalPoints { value, chance } => (3, i64::from(*value), chance.bits()),
                    Action::StartPoints { value, chance } => (4, i64::from(*value), chance.bits()),
                };
                (*phase, tag, value, bits, condition)
            })
            .collect();
        let mut key = format!(
            "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}",
            prepared.rows, conditions, prepared.life_deck, prepared.plan.probes, actions, activation_bits,
        )
        .into_bytes();
        if prepared.collect_moments {
            key.extend_from_slice(b"/moments");
        }
        key
    }

    pub(super) fn insert(&mut self, key: Vec<u8>, value: std::sync::Arc<LuckDpCertifiedResult>, capacity: usize) {
        if key.len() > capacity || capacity == 0 {
            return;
        }
        if self.entries.len() >= 32 || self.bytes + key.len() > capacity {
            self.entries.clear();
            self.bytes = 0;
        }
        self.bytes += key.len();
        self.entries.insert(key, value);
    }

    fn limit(&mut self, capacity: usize) {
        if self.bytes > capacity {
            self.entries.clear();
            self.bytes = 0;
        }
    }
}

/// Certified curves reused within one request, across performance orders and decks.
///
/// A curve is keyed by the complete transcript its propagation reads: the reduced native recording (range
/// templates, lottery tables, probes, every frame's range transitions, judged notes with their lot buffs and
/// binary32 gauge speeds, start-row chances and pending lots) encoded word for word. A hit compares the whole key,
/// so it returns exactly the curve [`luck_rush_dp_certified_with_ranking`] computes for the same arguments. The
/// recording runs for each new compiled recorder state in a score session. Complete recordings and complete
/// propagations enter the cache. Entries are dropped oldest first once the keys exceed the byte capacity.
#[derive(Default)]
pub struct LuckDpCache {
    pub(super) entries: FxHashMap<std::sync::Arc<[u64]>, std::sync::Arc<LuckDpCertifiedResult>>,
    pub(super) order: std::collections::VecDeque<std::sync::Arc<[u64]>>,
    pub(super) words: usize,
    capacity_words: usize,
    pub(super) stats: LuckDpCacheStats,
    /// The step tables of the propagations ([`propagate_sharing`]).
    tables: StepTables<ProbabilityMass>,
}

/// Use of a [`LuckDpCache`]. Timings are filled only in diagnostic builds.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckDpCacheStats {
    pub lookups: u64,
    pub hits: u64,
    /// Compiled recorder states considered and reused by score sessions.
    pub recording_lookups: u64,
    pub recording_hits: u64,
    pub evictions: u64,
    pub peak_entries: usize,
    /// The largest total key size held, in bytes.
    pub peak_key_bytes: usize,
    pub record_ms: f64,
    pub propagate_ms: f64,
}

impl LuckDpCache {
    /// A cache holding keys of at most `capacity_bytes` in total; zero stores nothing.
    pub fn new(capacity_bytes: usize) -> Self {
        Self { capacity_words: capacity_bytes / std::mem::size_of::<u64>(), ..Default::default() }
    }

    pub fn stats(&self) -> LuckDpCacheStats {
        self.stats
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// [`luck_rush_dp_certified_with_ranking`] of the same arguments, from the cache when an equal transcript was
    /// propagated before.
    #[allow(clippy::too_many_arguments)]
    pub fn certified(
        &mut self,
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        deck: &[Performer],
        probes: Option<&[Option<usize>]>,
        ranking: Option<&[crate::replay::RankConfirmation]>,
    ) -> Result<std::sync::Arc<LuckDpCertifiedResult>, Error> {
        Ok(self
            .certified_cancellable(
                master,
                skills,
                notes,
                skill_events,
                params,
                setup,
                play,
                delta_times,
                deck,
                probes,
                ranking,
                None,
                &mut || false,
            )?
            .expect("complete certified curve"))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn certified_cancellable(
        &mut self,
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        deck: &[Performer],
        probes: Option<&[Option<usize>]>,
        ranking: Option<&[crate::replay::RankConfirmation]>,
        recordings: Option<&mut RecordingCache>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        self.certified_cancellable_mode(
            master,
            skills,
            notes,
            skill_events,
            params,
            setup,
            play,
            delta_times,
            deck,
            probes,
            ranking,
            false,
            recordings,
            cancelled,
        )
    }

    /// Cached certified curve with additive range indicators.
    #[allow(clippy::too_many_arguments)]
    pub fn certified_with_moments(
        &mut self,
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        deck: &[Performer],
        probes: Option<&[Option<usize>]>,
        ranking: Option<&[crate::replay::RankConfirmation]>,
    ) -> Result<std::sync::Arc<LuckDpCertifiedResult>, Error> {
        Ok(self
            .certified_cancellable_mode(
                master,
                skills,
                notes,
                skill_events,
                params,
                setup,
                play,
                delta_times,
                deck,
                probes,
                ranking,
                true,
                None,
                &mut || false,
            )?
            .expect("complete certified curve"))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn certified_cancellable_mode(
        &mut self,
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        deck: &[Performer],
        probes: Option<&[Option<usize>]>,
        ranking: Option<&[crate::replay::RankConfirmation]>,
        collect_moments: bool,
        mut recordings: Option<&mut RecordingCache>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        if cancelled() {
            return Ok(None);
        }
        if let Some(recordings) = recordings.as_mut() {
            recordings.limit(self.recording_capacity());
        }
        #[cfg(feature = "search-diagnostics")]
        let started = std::time::Instant::now();
        let prepared = prepare_recording::<ProbabilityMass>(
            master,
            skills,
            notes,
            params,
            play,
            delta_times,
            deck,
            probes,
            collect_moments,
        )?;
        let recording_key =
            recordings.as_ref().filter(|_| self.capacity_words > 0).map(|_| RecordingCache::key(&prepared));
        if let (Some(recordings), Some(key)) = (recordings.as_ref(), recording_key.as_ref()) {
            self.stats.recording_lookups += 1;
            if let Some(found) = recordings.entries.get(key) {
                self.stats.recording_hits += 1;
                #[cfg(feature = "search-diagnostics")]
                {
                    self.stats.record_ms += started.elapsed().as_secs_f64() * 1e3;
                }
                return Ok(Some(found.clone()));
            }
        }
        let transcript = timed(
            |p| &mut p.total_ms,
            || {
                let skeleton = record_skeleton(
                    master,
                    skills,
                    notes,
                    skill_events,
                    params,
                    setup,
                    play,
                    delta_times,
                    &prepared.reduced,
                    prepared.life_deck.as_deref(),
                    ranking,
                    cancelled,
                )?;
                Ok::<_, Error>(skeleton.map(|skeleton| file(&skeleton, &prepared.plan, collect_moments)))
            },
        );
        #[cfg(feature = "search-diagnostics")]
        {
            self.stats.record_ms += started.elapsed().as_secs_f64() * 1e3;
        }
        let Some(transcript) = transcript? else { return Ok(None) };
        if cancelled() {
            return Ok(None);
        }
        let Some(result) = self.law_of(&transcript, cancelled)? else { return Ok(None) };
        if let (Some(recordings), Some(key)) = (recordings, recording_key) {
            recordings.insert(key, result.clone(), self.recording_capacity());
        }
        Ok(Some(result))
    }

    /// The certified lottery law of the performers of `deck` in its order, with the pairs of performer positions
    /// whose relative order the law may depend on: every other order of the same performers that keeps the relative
    /// order of each pair has the same law. None when the recording does not show that.
    ///
    /// The recording is the native run of the condition skills that hold a lot or gauge factor, with the actions of
    /// every lottery condition skill filed from the plan ([`record_skeleton`], [`file`]). A model built for another
    /// order is the model of this order with its performers moved, as [`OrderedLive`] uses: their skills keep their own
    /// state, and an effect reads no performer's position. Within a phase every updater checks before any effect
    /// applies, so another order applies the same effects, in another order only between performers. Effects whose
    /// order the recording does not observe commute with every other effect: score-ups, which go to the reduced
    /// calculator the recording never reads, result guarantees, which only a lottery draw reads and the recording draws
    /// no random value, and lot and gauge factors when their binary32 sums are exact and so do not depend on the order
    /// of their filings. Any two other effects of two performers in one phase are taken not to commute, and so are the
    /// lot and gauge factors of two performers in one phase when their sums may round. Two application orders of one
    /// phase that keep the relative order of every two effects that do not commute leave the same state, so an order
    /// that keeps the relative order of every pair of performers whose such effects one phase applied records the same
    /// frame, and by induction over the frames the same recording. The actions of each phase are listed in updater
    /// order, so their list is the same in an order that also keeps the relative order of every two performers with
    /// different actions in one phase.
    ///
    /// The recording also reads the life recorder of the whole formation: its converted judgements, and its phase
    /// lives for actions whose conditions read the life. No action condition may read the life, and the recording
    /// must convert no judgement; the caller establishes that no other order converts one either.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn certified_order_class(
        &mut self,
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        deck: &[Performer],
        ranking: Option<&[crate::replay::RankConfirmation]>,
    ) -> Result<Option<OrderClass>, Error> {
        let prepared =
            prepare_recording::<ProbabilityMass>(master, skills, notes, params, play, delta_times, deck, None, false)?;
        let plan = &prepared.plan;
        if plan.actions.iter().any(|(_, _, checker)| checker.as_ref().is_some_and(reads_life))
            || prepared.cond.iter().any(|condition| condition.member >= PAIR_POSITIONS)
        {
            return Ok(None);
        }
        let skeleton = record_skeleton(
            master,
            skills,
            notes,
            skill_events,
            params,
            setup,
            play,
            delta_times,
            &prepared.reduced,
            prepared.life_deck.as_deref(),
            ranking,
            &mut || false,
        )?
        .expect("an uncancelled recording");
        let Some(pairs) = skeleton.pairs.filter(|_| skeleton.failure.is_none()) else {
            return Ok(None);
        };
        let transcript = file(&skeleton, plan, false);
        let law = self.law_of(&transcript, &mut || false)?.expect("an uncancelled propagation");
        Ok(Some(OrderClass { law, pairs: plan.order_pairs | pairs }))
    }

    /// The certified law of a transcript, from the cache or propagated.
    pub(super) fn law_of(
        &mut self,
        transcript: &Transcript<ProbabilityMass>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        let key = (self.capacity_words > 0).then(|| transcript.key()).flatten();
        if let Some(key) = &key {
            self.stats.lookups += 1;
            if let Some(found) = self.entries.get(&key[..]) {
                self.stats.hits += 1;
                return Ok(Some(found.clone()));
            }
        }
        let result = self.propagated(transcript, cancelled)?;
        if let (Some(key), Some(result)) = (key, &result) {
            self.insert(key, result.clone());
        }
        Ok(result)
    }

    /// The certified law of a transcript, propagated without a cache lookup and kept by the caller.
    pub(super) fn propagated(
        &mut self,
        transcript: &Transcript<ProbabilityMass>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        #[cfg(feature = "search-diagnostics")]
        let started = std::time::Instant::now();
        let result = propagate_sharing(transcript, &mut self.tables, cancelled);
        #[cfg(feature = "search-diagnostics")]
        {
            self.stats.propagate_ms += started.elapsed().as_secs_f64() * 1e3;
        }
        let Some(result) = result? else { return Ok(None) };
        let result = std::sync::Arc::new(LuckDpCertifiedResult {
            probe_transitions: result.probe_transitions,
            rush_transitions: result.rush_transitions,
            steps: result.steps.into(),
            frame_queries: result.frame_queries.into(),
            probes: result.probes,
            probe_starts: result.probe_starts,
            range_moments: result.range_moments,
            peak_states: result.peak_states,
            transitions: result.transitions,
        });
        Ok(Some(result))
    }

    fn recording_capacity(&self) -> usize {
        self.capacity_words.saturating_mul(std::mem::size_of::<u64>()).min(1 << 20)
    }

    pub(super) fn insert(&mut self, key: Vec<u64>, value: std::sync::Arc<LuckDpCertifiedResult>) {
        if key.len() > self.capacity_words {
            return;
        }
        while self.words + key.len() > self.capacity_words {
            let oldest = self.order.pop_front().expect("held keys account for the held words");
            self.words -= oldest.len();
            self.entries.remove(&oldest[..]);
            self.stats.evictions += 1;
        }
        let key: std::sync::Arc<[u64]> = key.into();
        self.words += key.len();
        self.order.push_back(key.clone());
        self.entries.insert(key, value);
        self.stats.peak_entries = self.stats.peak_entries.max(self.entries.len());
        self.stats.peak_key_bytes = self.stats.peak_key_bytes.max(self.words * std::mem::size_of::<u64>());
    }
}
