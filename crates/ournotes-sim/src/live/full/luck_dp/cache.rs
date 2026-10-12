//! Caches of certified lottery laws, recordings and order classes.

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

/// The order classes ([`LuckDpCache::certified_order_class`]) of formations in one live, keyed by the lottery
/// conditions of the formation in its order, so a formation whose key was seen takes its class without a recording.
///
/// The class is a function of the reduced recording, and the recording of the reduced model and the live: the master,
/// the lottery skills, the chart, the frame schedule and its delta times, the live parameters and the rank arrivals.
/// The reduced performers hold no live or support skill, so the build of the reduced model reads them only to make
/// their lottery-related Gekisou condition skills, and the key holds those skills with their effect rows
/// ([`lottery_conditions_text`]). It leaves out the condition skills without an effect: such a skill only reads the
/// Gekisou gate when it updates, and changes nothing. The live is held whole except for the total power: it sets the
/// reduced score calculator alone, which the recording never reads (the controller is given a zero current score, no
/// condition reads the score, and the appliers only file score commands into it). A formation whose performers may
/// convert a judgement of the play ([`luck_play_converts`]) records the life of the complete formation and takes no
/// key, and so does a class whose actions read the life. The checks of the complete formation that precede a
/// recording run on every lookup, and a formation they refuse takes no key.
///
/// A recording without a life recorder takes its native part from the skeleton of its lot and gauge factor skills
/// ([`record_skeleton`]), which the classes hold for the formations of the live that share those skills.
pub struct LuckOrderClasses<'m> {
    pub(super) master: &'m Master,
    live: Option<ClassLive>,
    pub(super) entries: FxHashMap<std::sync::Arc<[u8]>, Option<OrderClass>>,
    pub(super) order: std::collections::VecDeque<std::sync::Arc<[u8]>>,
    bytes: usize,
    capacity_bytes: usize,
    /// The skeletons of recordings without a life recorder, keyed by [`skeleton_key`], at most [`SKELETONS`].
    skeletons: FxHashMap<Vec<u8>, std::sync::Arc<Skeleton>>,
    skeleton_order: std::collections::VecDeque<Vec<u8>>,
    pub(super) stats: LuckOrderClassStats,
}

/// The skeletons a [`LuckOrderClasses`] holds.
const SKELETONS: usize = 32;

/// Use of a [`LuckOrderClasses`].
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckOrderClassStats {
    pub lookups: u64,
    pub hits: u64,
    /// Formations that took no key.
    pub unkeyed: u64,
    /// Lives that replaced the held one.
    pub lives: u64,
    pub evictions: u64,
    pub peak_entries: usize,
    pub peak_key_bytes: usize,
    /// Recordings of a formation without a key hit that looked up their skeleton, the lookups that found it, the
    /// skeletons dropped, and the most held at once.
    pub skeleton_lookups: u64,
    pub skeleton_hits: u64,
    pub skeleton_evictions: u64,
    pub skeleton_peak_entries: usize,
}

/// The live of a [`LuckOrderClasses`]: the total power cleared, binary32 values by bit pattern.
#[derive(PartialEq)]
struct ClassLive {
    pub(super) skills: LuckSkills,
    pub(super) notes: Vec<LiveNote>,
    pub(super) events: Vec<(i32, i32)>,
    pub(super) params: (i64, i32, i32, i32, Option<i32>, u32),
    pub(super) setup: GekisouSetup,
    pub(super) play: LivePlay,
    pub(super) delta_times: Vec<u32>,
    pub(super) ranking: Option<Vec<crate::replay::RankConfirmation>>,
}

impl ClassLive {
    pub(super) fn params(params: LiveParams) -> (i64, i32, i32, i32, Option<i32>, u32) {
        let LiveParams {
            skill_target_music_type,
            total_power: _,
            music_level,
            converted_note_count,
            music_length_ms,
            score_music_length_ms,
            assist_factor,
        } = params;
        (
            skill_target_music_type,
            music_level,
            converted_note_count,
            music_length_ms,
            score_music_length_ms,
            assist_factor.to_bits(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn is(
        &self,
        skills: &LuckSkills,
        notes: &[LiveNote],
        events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        ranking: Option<&[crate::replay::RankConfirmation]>,
    ) -> bool {
        self.params == Self::params(params)
            && self.notes == notes
            && self.events == events
            && self.setup == *setup
            && self.ranking.as_deref() == ranking
            && self.delta_times.len() == delta_times.len()
            && self.delta_times.iter().zip(delta_times).all(|(&bits, delta)| bits == delta.to_bits())
            && self.play == *play
            && self.skills == *skills
    }
}

impl<'m> LuckOrderClasses<'m> {
    /// Classes of formations played with `master`, holding keys of at most `capacity_bytes` in total; zero stores
    /// nothing.
    pub fn new(master: &'m Master, capacity_bytes: usize) -> Self {
        Self {
            master,
            live: None,
            entries: FxHashMap::default(),
            order: std::collections::VecDeque::new(),
            bytes: 0,
            capacity_bytes,
            skeletons: FxHashMap::default(),
            skeleton_order: std::collections::VecDeque::new(),
            stats: LuckOrderClassStats::default(),
        }
    }

    pub fn stats(&self) -> LuckOrderClassStats {
        self.stats
    }

    /// The key of `deck` in the live, or None when it takes none. Classes of another live are dropped first.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn key(
        &mut self,
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        delta_times: &[f32],
        ranking: Option<&[crate::replay::RankConfirmation]>,
        deck: &[Performer],
    ) -> Option<std::sync::Arc<[u8]>> {
        // The held master stays borrowed, so no other master can be at its address.
        if self.capacity_bytes == 0 || !std::ptr::eq(master, self.master) {
            return None;
        }
        if !self
            .live
            .as_ref()
            .is_some_and(|live| live.is(skills, notes, events, params, setup, play, delta_times, ranking))
        {
            self.stats.lives += 1;
            self.entries.clear();
            self.order.clear();
            self.bytes = 0;
            self.skeletons.clear();
            self.skeleton_order.clear();
            self.live = Some(ClassLive {
                skills: skills.clone(),
                notes: notes.to_vec(),
                events: events.to_vec(),
                params: ClassLive::params(params),
                setup: setup.clone(),
                play: play.clone(),
                delta_times: delta_times.iter().map(|delta| delta.to_bits()).collect(),
                ranking: ranking.map(<[_]>::to_vec),
            });
        }
        let keyed = check_recording(master, skills, play, delta_times, None, deck).is_ok()
            && !luck_play_converts(master, deck, notes, play);
        let key = keyed
            .then(|| lottery_conditions_text(master, &reduced_deck(deck), params.skill_target_music_type, skills).ok())
            .flatten();
        if key.is_none() {
            self.stats.unkeyed += 1;
        }
        key.map(|key| key.into_bytes().into())
    }

    /// The skeleton under `key` in the held live, recorded by `record` when not held.
    pub(super) fn skeleton(
        &mut self,
        key: Vec<u8>,
        record: impl FnOnce() -> Result<Skeleton, Error>,
    ) -> Result<std::sync::Arc<Skeleton>, Error> {
        self.stats.skeleton_lookups += 1;
        if let Some(found) = self.skeletons.get(&key) {
            self.stats.skeleton_hits += 1;
            return Ok(found.clone());
        }
        let skeleton = std::sync::Arc::new(record()?);
        if self.skeleton_order.len() == SKELETONS {
            let oldest = self.skeleton_order.pop_front().expect("held skeletons");
            self.skeletons.remove(&oldest);
            self.stats.skeleton_evictions += 1;
        }
        self.skeleton_order.push_back(key.clone());
        self.skeletons.insert(key, skeleton.clone());
        self.stats.skeleton_peak_entries = self.stats.skeleton_peak_entries.max(self.skeletons.len());
        Ok(skeleton)
    }

    pub(super) fn insert(&mut self, key: std::sync::Arc<[u8]>, class: Option<OrderClass>) {
        if key.len() > self.capacity_bytes {
            return;
        }
        while self.bytes + key.len() > self.capacity_bytes {
            let oldest = self.order.pop_front().expect("held keys account for the held bytes");
            self.bytes -= oldest.len();
            self.entries.remove(&oldest);
            self.stats.evictions += 1;
        }
        self.bytes += key.len();
        self.order.push_back(key.clone());
        self.entries.insert(key, class);
        self.stats.peak_entries = self.stats.peak_entries.max(self.entries.len());
        self.stats.peak_key_bytes = self.stats.peak_key_bytes.max(self.bytes);
    }
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
    ///
    /// `classes` gives the class of a formation whose key it holds, and keeps the classes recorded here.
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
        classes: Option<&mut LuckOrderClasses<'_>>,
    ) -> Result<Option<OrderClass>, Error> {
        let mut keyed = classes.and_then(|classes| {
            let key =
                classes.key(master, skills, notes, skill_events, params, setup, play, delta_times, ranking, deck)?;
            Some((classes, key))
        });
        if let Some((classes, key)) = keyed.as_mut() {
            classes.stats.lookups += 1;
            if let Some(class) = classes.entries.get(&key[..]) {
                classes.stats.hits += 1;
                return Ok(class.clone());
            }
        }
        let Some((class, life)) = self.record_order_class(
            master,
            skills,
            notes,
            skill_events,
            params,
            setup,
            play,
            delta_times,
            deck,
            ranking,
            keyed.as_mut().map(|(classes, _)| &mut **classes),
        )?
        else {
            return Ok(None);
        };
        if let Some((classes, key)) = keyed
            && !life
        {
            classes.insert(key, class.clone());
        }
        Ok(class)
    }

    /// The class [`LuckDpCache::certified_order_class`] records, and whether the recording read a life recorder;
    /// None when the actions read the life. `classes` holds the skeletons of recordings without a life recorder.
    #[allow(clippy::too_many_arguments)]
    fn record_order_class(
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
        classes: Option<&mut LuckOrderClasses<'_>>,
    ) -> Result<Option<(Option<OrderClass>, bool)>, Error> {
        let prepared =
            prepare_recording::<ProbabilityMass>(master, skills, notes, params, play, delta_times, deck, None, false)?;
        let plan = &prepared.plan;
        if plan.actions.iter().any(|(_, _, checker)| checker.as_ref().is_some_and(reads_life)) {
            return Ok(None);
        }
        let life = prepared.life_deck.is_some();
        if prepared.cond.iter().any(|condition| condition.member >= PAIR_POSITIONS) {
            return Ok(Some((None, life)));
        }
        let record = || -> Result<Skeleton, Error> {
            Ok(record_skeleton(
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
            .expect("an uncancelled recording"))
        };
        let skeleton = match classes.filter(|_| !life) {
            Some(classes) => classes.skeleton(skeleton_key(&prepared), record)?,
            None => std::sync::Arc::new(record()?),
        };
        let Some(pairs) = skeleton.pairs.filter(|_| skeleton.failure.is_none()) else {
            return Ok(Some((None, life)));
        };
        let transcript = file(&skeleton, plan, false);
        let law = self.law_of(&transcript, &mut || false)?.expect("an uncancelled propagation");
        Ok(Some((Some(OrderClass { law, pairs: plan.order_pairs | pairs }), life)))
    }

    /// The certified law of a transcript, from the cache or propagated.
    fn law_of(
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
        #[cfg(feature = "search-diagnostics")]
        let started = std::time::Instant::now();
        let result = propagate_cancellable(transcript, cancelled);
        #[cfg(feature = "search-diagnostics")]
        {
            self.stats.propagate_ms += started.elapsed().as_secs_f64() * 1e3;
        }
        let Some(result) = result? else { return Ok(None) };
        let result = std::sync::Arc::new(LuckDpCertifiedResult {
            probe_transitions: result.probe_transitions,
            rush_transitions: result.rush_transitions,
            steps: result.steps,
            frame_queries: result.frame_queries,
            probes: result.probes,
            probe_starts: result.probe_starts,
            range_moments: result.range_moments,
            peak_states: result.peak_states,
            transitions: result.transitions,
        });
        if let Some(key) = key {
            self.insert(key, result.clone());
        }
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
