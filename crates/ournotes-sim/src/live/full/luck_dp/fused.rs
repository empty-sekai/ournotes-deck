//! Collect the unchanged DP input transcript during the admitted full score recording.
//!
//! Mechanism admission still uses the original reduced constructor and compiler. Only its second native
//! frame pass is omitted. The full recorder supplies the same controller observation after both skill
//! phases, before controller.update; shared field extraction preserves every binary32 factor bit.
use super::*;
use std::mem::size_of;
use std::sync::Arc;

const MAX_BYTES: usize = 1 << 20;

pub(in super::super) struct Recorder {
    transcript: Transcript<ProbabilityMass>,
    actions: Vec<(i64, Action<ProbabilityMass>, Option<Checker>)>,
    ranges: Vec<(i32, i32, i64)>,
    expected_frames: usize,
    observed_frames: usize,
    previous_frame: i32,
}

impl Recorder {
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn prepare(
        master: &Master,
        skills: &LuckSkills,
        notes: &[LiveNote],
        events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
        play: &LivePlay,
        deltas: &[f32],
        deck: &[Performer],
        full: &LiveModel,
        record_only: bool,
    ) -> Result<Option<Self>, Error> {
        // Keep the original record-only observer gate intact. In particular no phase-LIFE observer is
        // enabled after admission: this first route cannot consume a LIFE-dependent action at all.
        if !record_only
            || setup.fevers.is_empty()
            || setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission != M_LUCK)
            || !unchanged_judgements(full, play)
        {
            return Ok(None);
        }
        // Condition appliers share a native StateKey registry across effect kinds. An omitted ordinary
        // row must not collide with a retained lottery row, including the native wrapping ID expansion.
        let mut identities = FxHashSet::default();
        for skill in &full.cond {
            for effect in skill.updater.effects() {
                if !identities.insert(effect.effect_id) {
                    return Ok(None);
                }
            }
        }
        let prepared = prepare_recording::<ProbabilityMass>(
            master, skills, notes, events, params, setup, play, deltas, deck, None, None,
        )?;
        if prepared.life.is_some()
            || prepared.life_deck.is_some()
            || prepared.plan.actions.iter().any(|(_, _, condition)| condition.as_ref().is_some_and(reads_life))
        {
            return Ok(None);
        }
        let PreparedRecording { model, plan, .. } = prepared;
        let ctrl = &model.gk.as_ref().expect("admitted Gekisou recorder").ctrl;
        let ranges = ctrl.ranges.iter().map(|range| (range.start_ms, range.end_ms, range.mission)).collect();
        let transcript = Transcript {
            // Capture these before any full-native frame: weighted 11003/11005 appliers may still alter
            // gauge/minimum state. The DP applies the immutable Plan itself, exactly once.
            templates: ctrl.states.iter().map(|state| state.luck.clone()).collect(),
            machine: ctrl.machine.clone(),
            luck: ctrl.ranges.iter().map(|range| range.mission == M_LUCK).collect(),
            probes: plan.probes,
            miss_rows: plan.actions.iter().any(|(_, action, _)| matches!(action, Action::MissGauge { .. })),
            frames: Vec::new(),
            notes: Vec::new(),
            hits: Vec::new(),
            actions: Vec::new(),
            pending: Vec::new(),
            failure: None,
        };
        Ok(Some(Self {
            transcript,
            actions: plan.actions,
            ranges,
            expected_frames: play.frames.len(),
            observed_frames: 0,
            previous_frame: i32::MIN,
        }))
    }

    /// Called at the same logical observation point as record_frame, using the full model's actual
    /// converted note results. It neither applies a skill nor reads/writes score or the native LIFE cache.
    pub(in super::super) fn observe(
        &mut self,
        model: &LiveModel,
        frame: &PlayFrame,
        delta: f32,
        results: &[(LiveNote, i32)],
    ) -> Result<(), Error> {
        let mut judged = declared_judgements(|id| model.notes.get(&id), frame, delta, self.previous_frame)?;
        if judged.len() != results.len() {
            return Err(Error::Unsupported("fused LUCK recording changed the note stream".into()));
        }
        for (declared, (note, converted)) in judged.iter_mut().zip(results) {
            if (declared.0, declared.1, declared.2, declared.3)
                != (note.note_id, note.note_operate_type, note.time_ms, *converted)
            {
                return Err(Error::Unsupported("fused LUCK recording changed an admitted judgement".into()));
            }
            declared.3 = *converted;
        }
        self.reserve_frame(judged.len())?;
        let ctrl = &model.gk.as_ref().expect("admitted Gekisou recorder").ctrl;
        // LIFE is unread by this Plan, so these placeholders are never consulted by chance().
        append_frame(ctrl, &self.actions, &self.ranges, &judged, [0; 2], frame.time_ms, &mut self.transcript)?;
        self.previous_frame = frame.time_ms;
        self.observed_frames += 1;
        Ok(())
    }

    fn reserve_frame(&mut self, notes: usize) -> Result<(), Error> {
        let t = &mut self.transcript;
        let additions = [1, notes, notes.saturating_mul(self.ranges.len()), self.actions.len(), self.ranges.len()];
        let sizes = [
            size_of::<Frame>(),
            size_of::<Judged>(),
            size_of::<Hit>(),
            size_of::<Action<ProbabilityMass>>(),
            size_of::<(usize, i32)>(),
        ];
        let lengths = [t.frames.len(), t.notes.len(), t.hits.len(), t.actions.len(), t.pending.len()];
        let capacities =
            [t.frames.capacity(), t.notes.capacity(), t.hits.capacity(), t.actions.capacity(), t.pending.capacity()];
        let mut desired = [0usize; 5];
        for i in 0..5 {
            let needed = lengths[i].saturating_add(additions[i]);
            desired[i] =
                if needed <= capacities[i] { capacities[i] } else { needed.max(capacities[i].saturating_mul(2)) };
        }
        let estimated =
            |desired: &[usize; 5]| (0..5).try_fold(0usize, |sum, i| sum.checked_add(desired[i].checked_mul(sizes[i])?));
        if estimated(&desired).is_none_or(|bytes| bytes > MAX_BYTES) {
            for i in 0..5 {
                desired[i] = lengths[i].saturating_add(additions[i]).max(capacities[i]);
            }
        }
        let bytes = estimated(&desired);
        if bytes.is_none_or(|bytes| bytes > MAX_BYTES) {
            return Err(Error::Capacity("fused LUCK recording transcript cap".into()));
        }
        let reserve = || Error::Capacity("fused LUCK recording allocation".into());
        t.frames.try_reserve_exact(desired[0].saturating_sub(lengths[0])).map_err(|_| reserve())?;
        t.notes.try_reserve_exact(desired[1].saturating_sub(lengths[1])).map_err(|_| reserve())?;
        t.hits.try_reserve_exact(desired[2].saturating_sub(lengths[2])).map_err(|_| reserve())?;
        t.actions.try_reserve_exact(desired[3].saturating_sub(lengths[3])).map_err(|_| reserve())?;
        t.pending.try_reserve_exact(desired[4].saturating_sub(lengths[4])).map_err(|_| reserve())?;
        let actual = t
            .frames
            .capacity()
            .saturating_mul(sizes[0])
            .saturating_add(t.notes.capacity().saturating_mul(sizes[1]))
            .saturating_add(t.hits.capacity().saturating_mul(sizes[2]))
            .saturating_add(t.actions.capacity().saturating_mul(sizes[3]))
            .saturating_add(t.pending.capacity().saturating_mul(sizes[4]));
        if actual > MAX_BYTES {
            return Err(reserve());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in super::super) fn test_complete_key(&self, model: &LiveModel) -> Result<Vec<u64>, Error> {
        self.complete(model)?;
        self.transcript.key().ok_or_else(|| Error::Unsupported("fused transcript key refused".into()))
    }

    fn complete(&self, model: &LiveModel) -> Result<(), Error> {
        if self.observed_frames != self.expected_frames
            || model.random.draws() != 0
            || model.gk.as_ref().is_none_or(|gk| gk.ctrl.states.iter().any(|state| state.state != S_FINISH))
        {
            return Err(Error::Unsupported("fused LUCK recording has incomplete native coverage".into()));
        }
        Ok(())
    }

    fn finish(self, model: &LiveModel) -> Result<Transcript<ProbabilityMass>, Error> {
        self.complete(model)?;
        Ok(self.transcript)
    }
}

impl LuckDpCache {
    /// Only a complete privately constructed transcript can enter the unchanged curve-key cache. The
    /// existing compiled/LIFE recording caches remain owned by certified_cancellable's fallback route.
    pub(in super::super) fn certified_fused(
        &mut self,
        recording: Recorder,
        model: &LiveModel,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<Arc<LuckDpCertifiedResult>>, Error> {
        if cancelled() {
            return Ok(None);
        }
        let transcript = recording.finish(model)?;
        let key = (self.capacity_words > 0).then(|| transcript.key()).flatten();
        if let Some(key) = &key {
            self.stats.lookups += 1;
            if let Some(found) = self.entries.get(&key[..]) {
                if cancelled() {
                    return Ok(None);
                }
                self.stats.hits += 1;
                return Ok(Some(Arc::clone(found)));
            }
        }
        #[cfg(feature = "search-diagnostics")]
        let started = std::time::Instant::now();
        let result = propagate_cancellable(&transcript, cancelled, Some(&mut self.stats));
        #[cfg(feature = "search-diagnostics")]
        {
            self.stats.propagate_ms += started.elapsed().as_secs_f64() * 1e3;
        }
        let Some(result) = result? else { return Ok(None) };
        let result = Arc::new(LuckDpCertifiedResult {
            steps: result.steps,
            probes: result.probes,
            peak_states: result.peak_states,
            transitions: result.transitions,
        });
        if cancelled() {
            return Ok(None);
        }
        if let Some(key) = key {
            self.insert(key, Arc::clone(&result));
        }
        Ok(Some(result))
    }
}
