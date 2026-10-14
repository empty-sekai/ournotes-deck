//! Playing a Live model frame by frame, from judged notes or from raw judgements.

use super::*;

impl LiveModel {
    /// Plays every frame of `play` (frame delta times 0); returns the final score.
    pub fn run(&mut self, play: &LivePlay) -> Result<i32, Error> {
        self.random.set_seed(play.base_seed);
        for f in &play.frames {
            self.frame_timed(f.time_ms, &f.judged, 0f32)?;
        }
        Ok(self.score.score)
    }

    /// Plays every frame of `play` with its delta time in seconds (`delta_times[i]` for frame `i`); returns the final
    /// score.
    pub fn run_timed(&mut self, play: &LivePlay, delta_times: &[f32]) -> Result<i32, Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        self.random.set_seed(play.base_seed);
        for (f, &dt) in play.frames.iter().zip(delta_times) {
            self.frame_timed(f.time_ms, &f.judged, dt)?;
        }
        Ok(self.score.score)
    }

    /// Runs a fresh model with a complete post-shuffle random state, without reseeding.
    /// Ignores play.base_seed. This does not reset gameplay state: construct a new model per root.
    pub fn run_with_random(&mut self, play: &LivePlay, delta_times: &[f32], random: LiveRandom) -> Result<i32, Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        self.random = random;
        for (frame, &dt) in play.frames.iter().zip(delta_times) {
            self.frame_timed(frame.time_ms, &frame.judged, dt)?;
        }
        Ok(self.score.score)
    }

    /// Plays like [`LiveModel::run_with_random`], but after every `every` frames (not after the last) asks `stop` with
    /// the settled score [`LiveModel::settle`]; `Ok(None)` when it stops the play early.
    pub fn run_with_cutoff(
        &mut self,
        play: &LivePlay,
        delta_times: &[f32],
        random: LiveRandom,
        every: usize,
        mut stop: impl FnMut(Settled) -> bool,
    ) -> Result<Option<i32>, Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        // future[i]: the earliest chart time of a note judged after frame i (its note command's filing time).
        let mut future = vec![i32::MAX; play.frames.len()];
        let mut earliest = i32::MAX;
        for (i, frame) in play.frames.iter().enumerate().rev() {
            future[i] = earliest;
            for j in &frame.judged {
                if let Some(n) = self.notes.get(&j.note_id) {
                    earliest = earliest.min(n.time_ms);
                }
            }
        }
        self.random = random;
        let every = every.max(1);
        let last = play.frames.len().saturating_sub(1);
        for (i, (frame, &dt)) in play.frames.iter().zip(delta_times).enumerate() {
            self.frame_timed(frame.time_ms, &frame.judged, dt)?;
            if i < last && (i + 1) % every == 0 && stop(self.settle(future[i])) {
                return Ok(None);
            }
        }
        Ok(Some(self.score.score))
    }

    /// Search cutoff after a frame: every score frame below `frame` is final, holding `total` points (`fixed` of them
    /// fixed Gekisou rank bonus scores). `future_note_ms` is the earliest chart time of a note judged in a later frame.
    ///
    /// A duration-adjustment row (15000) with a value below zero keeps this prefix empty: its later execution can
    /// move a running effect's finish before the current frame.
    ///
    /// A later command lands at a time no earlier than the least of: this frame's time (skill execution and
    /// re-application times; live skill events not yet fired), pending timed finishes, the positive music length
    /// (finish-time clamp), `future_note_ms` (note commands, and checker override times taken from notes judged
    /// later), the start of every Gekisou range not yet finished (range-start and range-state override times,
    /// override times read from the playing range's judged notes and the solo ranking rewind), and the end of every
    /// externally ranked range awaiting its confirmation (the fixed rank bonus). Commands file at `get_frame` of
    /// their time, capped to the final addressable frame, and a rewind keeps the frames up to its own. The calculator
    /// keeps that final frame open and settles only frames strictly below the horizon, so every settled frame
    /// retains its final score.
    pub fn settle(&mut self, future_note_ms: i32) -> Settled {
        if self.rows.iter().any(|row| row.effect_type == 15000 && row.effect_value < 0) {
            return Settled { frame: 0, total: 0, fixed: 0 };
        }
        let mut horizon = self.frame_time.min(future_note_ms);
        if self.music_length_ms > 0 {
            horizon = horizon.min(self.music_length_ms);
        }
        for skill in &self.live {
            for effect in &skill.effects {
                if let Some(finish) = effect.state.pending_finish_ms(effect.act) {
                    horizon = horizon.min(finish);
                }
            }
        }
        for skill in &self.cond {
            for updater in &skill.updater.updaters {
                let effect = skill.updater.effect(updater.effect);
                if let Some(finish) = updater.state.pending_finish_ms(effect.act) {
                    horizon = horizon.min(finish);
                }
            }
        }
        if let Some(gk) = &self.gk {
            for (idx, (range, state)) in gk.ctrl.ranges.iter().zip(&gk.ctrl.states).enumerate() {
                if state.state < gekisou::S_FINISH {
                    horizon = horizon.min(range.start_ms);
                }
                if gk.external_ranking && !gk.rank_bonus.iter().any(|bonus| bonus.0 == idx) {
                    horizon = horizon.min(range.end_ms);
                }
            }
        }
        let frame = get_frame(horizon).max(0) as usize;
        let (total, fixed) = self.score.settle(frame);
        Settled { frame: self.score.settled_frame().min(frame) as i32, total, fixed }
    }

    /// Continues a play: plays frames `frames_played()..end` of `play`, frame `i` with delta time `delta_times[i]`.
    /// A clone taken between frames continues exactly as the original does.
    pub fn play_frames(&mut self, play: &LivePlay, delta_times: &[f32], end: usize) -> Result<(), Error> {
        if delta_times.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        let start = self.frames_played();
        if start > end || end > play.frames.len() {
            return Err(Error::Input(format!("frames {start}..{end} outside a play of {}", play.frames.len())));
        }
        for i in start..end {
            let f = &play.frames[i];
            self.frame_timed(f.time_ms, &f.judged, delta_times[i])?;
        }
        Ok(())
    }

    /// Plays one frame at music time `t` (delta time 0).
    pub fn frame(&mut self, t: i32, judged: &[JudgedNote]) -> Result<(), Error> {
        self.frame_timed(t, judged, 0f32)
    }

    /// Plays one frame at music time `t` with delta time `dt` in seconds.
    pub fn frame_timed(&mut self, t: i32, judged: &[JudgedNote], dt: f32) -> Result<(), Error> {
        if self.raw_pending.is_some() {
            return Err(Error::Input("raw frame is open".into()));
        }
        if self.raw_runtime.is_some() && !judged.is_empty() {
            return Err(Error::Input(
                "raw window runtime requires complete raw results, not a judgement stream".into(),
            ));
        }
        self.begin_frame_internal(t, dt)?;
        // judgement conversion and the combo counter, note by note
        let mut results = std::mem::take(&mut self.scratch.results);
        results.clear();
        for j in judged {
            let n = *self.notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
            let conv = self.conversion.convert(j.judgement, n.judgement_type, j.judgement_time_ms)?;
            self.combo.add_judgement(n.time_ms, conv)?;
            results.push((n, conv));
            self.judged.push((n.note_id, conv, n.time_ms));
        }
        let finished = self.finish_frame_results(t, &results, &[]);
        self.scratch.results = results;
        finished
    }

    /// Configures raw timing before the first frame. Runtime timings are explicit client data.
    pub fn enable_raw_runtime(&mut self, runtime: RawJudgementRuntime) -> Result<(), Error> {
        if self.raw_pending.is_some() || !self.trace.is_empty() {
            return Err(Error::Input("raw runtime must be configured before play".into()));
        }
        self.raw_runtime = Some(runtime);
        Ok(())
    }

    /// The raw judgement runtime, if configured.
    pub fn raw_runtime(&self) -> Option<&RawJudgementRuntime> {
        self.raw_runtime.as_ref()
    }

    /// The raw judgement runtime, if configured, for callers that set its client data (diff converter, Assist).
    pub fn raw_runtime_mut(&mut self) -> Option<&mut RawJudgementRuntime> {
        self.raw_runtime.as_mut()
    }

    /// Starts FT's frame. Submit each updater result immediately to feed converted grades back to NoteLine.
    pub fn begin_raw_frame(&mut self, t: i32, dt: f32) -> Result<(), Error> {
        if self.raw_runtime.is_none() {
            return Err(Error::Input("raw timing runtime is not configured".into()));
        }
        if self.raw_pending.is_some() {
            return Err(Error::Input("raw frame is already open".into()));
        }
        self.begin_frame_internal(t, dt)?;
        self.raw_pending = Some(Vec::new());
        Ok(())
    }

    /// Converter, then the Assist tail hook (only when no skill converter changed the grade), then DiffMsConverter;
    /// origin fields are preserved. Window limit callbacks run later in [`LiveModel::finish_raw_frame`], not while
    /// FT is still running its updaters.
    pub fn submit_raw_judgement(&mut self, mut j: RawJudgedNote) -> Result<crate::live::raw::NoteResult, Error> {
        if self.raw_pending.is_none() {
            return Err(Error::Input("no raw frame is open".into()));
        }
        let n = *self.notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
        if n.judgement_type != j.result.judgement_type {
            return Err(Error::Input("raw judgement type differs from chart".into()));
        }
        let incoming_grade = j.result.judgement;
        j.result.judgement = self.conversion.convert(incoming_grade, n.judgement_type, j.result.time_ms)?;
        let runtime =
            self.raw_runtime.as_mut().ok_or_else(|| Error::Input("raw timing runtime is not configured".into()))?;
        let unchanged_by_skill = j.result.judgement == incoming_grade;
        runtime.convert_assist(&mut j, unchanged_by_skill)?;
        j.result.diff_ms = (runtime.diff_converter)(j.result.judgement, j.result.diff_ms);
        self.combo.add_judgement(n.time_ms, j.result.judgement)?;
        self.judged.push((n.note_id, j.result.judgement, n.time_ms));
        self.raw_pending.get_or_insert_with(Vec::new).push(j);
        Ok(j.result)
    }

    /// Finishes FT, then consumes the results in LiveExecutor order, including the window callbacks and their
    /// deferred dictionary deletion before the skill appliers update.
    pub fn finish_raw_frame(&mut self) -> Result<(), Error> {
        let raw = self.raw_pending.take().ok_or_else(|| Error::Input("no raw frame is open".into()))?;
        let results = raw.iter().map(|j| (self.notes[&j.note_id], j.result.judgement)).collect();
        self.finish_frame_internal(self.frame_time, results, &raw)
    }

    /// Convenience only: for callers not requiring per-note NoteLine feedback.
    pub fn frame_raw_timed(
        &mut self,
        t: i32,
        judged: &[RawJudgedNote],
        dt: f32,
    ) -> Result<Vec<crate::live::raw::NoteResult>, Error> {
        self.begin_raw_frame(t, dt)?;
        let mut out = Vec::with_capacity(judged.len());
        for &note in judged {
            out.push(self.submit_raw_judgement(note)?);
        }
        self.finish_raw_frame()?;
        Ok(out)
    }

    pub(super) fn begin_frame_internal(&mut self, t: i32, dt: f32) -> Result<(), Error> {
        while let Some(c) = self.rank_timeline.get(self.next_rank_confirmation) {
            if c.frame > self.frames_played() {
                break;
            }
            let (range, rank, percent) = (c.range, c.rank, c.percent);
            self.queue_gekisou_rank_confirmation(range, rank, percent)?;
            self.next_rank_confirmation += 1;
        }
        self.program_has_started = true;
        self.frame_time = t;
        self.score.bounds_potential_rush(t, true);
        self.frame_rank_confirmation = self.prev_confirmed_rank.take();
        if let Some(gk) = self.gk.as_mut() {
            gk.fever.update(t, &mut gk.fever_updates);
            let current = self.score.score;
            let mut h = Handle { sc: &mut self.scorectl, score: &mut self.score };
            let mut env = Env { random: &mut self.random, handle: &mut h };
            gk.ctrl.before_update(dt, t, &gk.fever_updates, current, &mut env)?;
            if gk.external_ranking && !gk.solo_score_queries {
                let expression = self.score.program_snapshot();
                for &idx in &gk.ctrl.state_updates {
                    if gk.ctrl.states[idx].state == gekisou::S_START {
                        gk.program_rank_snapshots[idx].0 = expression;
                    }
                }
            }
            if gk.external_ranking && !gk.solo_score_queries {
                let query = self.score.bounds_last_query();
                for &idx in &gk.ctrl.state_updates {
                    if gk.ctrl.states[idx].state == gekisou::S_START {
                        gk.rank_snapshot_queries[idx].0 = query;
                    }
                }
            }
            // PlayingState.OnBeforeUpdateGekisou dispatches each changed range before FT updates notes.
            // Just missions enable grade 6 at Start, then restore the client's force setting at End.
            if let Some(runtime) = self.raw_runtime.as_mut() {
                for &idx in &gk.ctrl.state_updates {
                    if gk.ctrl.ranges[idx].mission == 3 {
                        let enabled = match gk.ctrl.states[idx].state {
                            gekisou::S_START => true,
                            gekisou::S_END => runtime.force_enable_just_judgement,
                            _ => continue,
                        };
                        runtime.set_just_judgement_enabled(enabled);
                    }
                }
            }
        }
        self.frame_events.clear();
        for (i, &(index, ev_t)) in self.events.iter().enumerate() {
            if !self.fired[i] && ev_t <= t {
                self.fired[i] = true;
                self.frame_events.push((index, ev_t));
                self.touched_events |= position_bit(index);
            }
        }
        self.judged.clear();
        Ok(())
    }

    pub(super) fn finish_frame_internal(
        &mut self,
        t: i32,
        results: Vec<(LiveNote, i32)>,
        raw: &[RawJudgedNote],
    ) -> Result<(), Error> {
        self.finish_frame_results(t, &results, raw)
    }

    fn finish_frame_results(
        &mut self,
        t: i32,
        results: &[(LiveNote, i32)],
        raw: &[RawJudgedNote],
    ) -> Result<(), Error> {
        let frame_rank_confirmation = self.frame_rank_confirmation.take();
        // LiveExecutor.OnUpdate (Assist level) runs after FT, before UpdateCurrentFrameParameters.
        if let Some(runtime) = self.raw_runtime.as_mut() {
            runtime.after_ft(results)?;
        }
        // The live combo controller consumes the simulator's frame delta, not the score combo directly.
        let simulator_combo = self.combo.timing_combo(t)?;
        let added_combo = simulator_combo.wrapping_sub(self.simulator_previous_combo).max(0);
        self.simulator_previous_combo = simulator_combo;
        if results.iter().any(|(_, judgement)| matches!(judgement, 1 | 2)) {
            self.current_combo = 0;
        }
        self.current_combo = self.current_combo.wrapping_add(added_combo);
        // note damage, frozen life and the note score commands
        for (result_index, &(n, conv)) in results.iter().enumerate() {
            self.life.add_note_damage(n.time_ms, conv)?;
            let life = self.life.get_life_at_ms(n.time_ms)?;
            // LiveScoreController.AddNoteScore checks the operation table before converting the score type.
            // Unscored simulator results (including type 122 / Pass) still reach life, combo and callbacks.
            if self.score.calc.note_factor_percent.contains_key(&n.note_operate_type) {
                let score_type = convert_score_type(conv as i64)?;
                self.score.add_note(NoteCommand::new(n.time_ms, life, n.note_id, n.note_operate_type, score_type));
            }
            // Window limit callbacks per consumed result (UpdateCurrentFrameParameters).
            if let (Some(note), Some(runtime)) = (raw.get(result_index), self.raw_runtime.as_mut()) {
                runtime.on_executor_judgement(*note)?;
            }
        }
        if let Some(runtime) = self.raw_runtime.as_mut() {
            runtime.frame_finish();
        }
        let info = self.gk.as_ref().map(|g| &g.ctrl as &dyn GekisouComboInfo);
        self.score.calculate(t, &self.combo, info)?;
        // skills
        #[cfg(feature = "search-diagnostics")]
        if let Some(mut probes) = self.rush_probes.take() {
            let observed = probes.observe(self);
            self.rush_probes = Some(probes);
            observed?;
        }
        let inp =
            FrameInput { time_ms: t, music_length_ms: self.music_length_ms, is_live_finished: self.is_live_finished };
        for &si in &self.enabled_live {
            self.live[si].trigger = TriggerResult::default();
        }
        for pool in &mut self.live_pools {
            if let Some(&(_, time_ms)) =
                self.frame_events.iter().rev().find(|&&(index, _)| index as i64 == pool.member as i64)
            {
                let si = pool.available.pop_front().ok_or_else(|| Error::Game("live skill pool is empty".into()))?;
                let s = &mut self.live[si];
                s.trigger = TriggerResult { is_trigger: true, time_ms };
                // Execute calls Stay, not UpdateEndFrame: counters and checker state survive.
                for e in &mut s.effects {
                    e.state.state = STAY;
                }
                self.enabled_live.push(si);
            }
        }
        self.enabled_live.sort_by_key(|&i| self.live[i].key.wrapping_mul(10).wrapping_add(self.live[i].index as i64));
        for c in self.cond.iter_mut() {
            c.updater.begin_frame();
        }
        let (mut listed, mut updated) =
            (std::mem::take(&mut self.scratch.listed), std::mem::take(&mut self.scratch.updated));
        let mut sleeping = FrameSleep::default();
        for ph in PHASES {
            listed.clear();
            if let Some(phase_life) = self.phase_life.as_mut() {
                phase_life[(ph - 1) as usize] = self.life.peek_life_at_ms(t)?;
            }
            let gk_view =
                self.gk.as_ref().map(|g| GkView { ctrl: &g.ctrl, prev_lots: &g.prev_lots, prev_lot_ms: g.prev_lot_ms });
            let mut ctx = CheckCtx {
                life: &mut self.life,
                random: &mut self.random,
                frame_time: t,
                current_combo: self.current_combo,
                judged: &self.judged,
                events: &self.frame_events,
                gk: gk_view,
                prev_confirmed_rank: frame_rank_confirmation,
            };
            for &si in &self.enabled_live {
                let s = &mut self.live[si];
                if s.parent_state == END_FRAME {
                    s.parent_state = STAY;
                    // Parent cleanup calls Stay; it must not reset cumulative counters.
                    for e in &mut s.effects {
                        e.state.state = STAY;
                    }
                    continue;
                }
                let mut changed = false;
                for e in &mut s.effects {
                    if ph < 1 || e.phase == ph {
                        changed |= e.update(inp, s.trigger, &mut ctx)?;
                    }
                }
                if changed {
                    s.parent_state = aggregate_live_state(&s.effects);
                }
                for (ei, e) in s.effects.iter().enumerate() {
                    if e.state.state != STAY
                        && (ph < 1 || e.phase == ph)
                        && self.rows[e.row].applier.observes(e.state.state)
                    {
                        listed.push(Listed::Live { skill: si, effect: ei });
                    }
                }
            }
            for (ui, c) in self.cond.iter_mut().enumerate() {
                if sleeping.skip(ph, ui, &c.updater, ctx.events, inp.is_live_finished, ctx.gk.map(|g| g.ctrl)) {
                    continue;
                }
                let draws = ctx.random.draws();
                c.updater.update_into(ph, inp, &mut ctx, &mut updated)?;
                if !updated.is_empty() || ctx.random.draws() != draws {
                    self.touched_skills |= position_bit(c.member as i32);
                }
                for &x in &updated {
                    let updater = &c.updater.updaters[x];
                    if updater.state.state != STAY
                        && self.rows[c.updater.effect(updater.effect).row].applier.observes(updater.state.state)
                    {
                        listed.push(Listed::Cond { updater: ui, u: x });
                    }
                }
            }
            for &item in &listed {
                self.apply(item)?;
            }
        }
        (self.scratch.listed, self.scratch.updated) = (listed, updated);
        // EndFrame: a newly freed instance is not available to this frame's triggers.
        for &si in &self.enabled_live {
            let s = &self.live[si];
            if s.parent_state == STAY {
                let pool = self
                    .live_pools
                    .iter_mut()
                    .find(|p| p.key == s.key && p.member == s.member)
                    .ok_or_else(|| Error::Game("missing live skill pool".into()))?;
                pool.available.push_back(si);
            }
        }
        self.enabled_live.retain(|&si| self.live[si].parent_state != STAY);
        self.life.sync_current_life(t)?;
        self.score.bounds_potential_skills(t);
        // An untimed sustained score-up that ends at or after the music length files its end there.
        if self.music_length_ms > 0 && self.music_length_ms < t {
            self.score.bounds_potential_skills(self.music_length_ms);
        }
        let info = self.gk.as_ref().map(|g| &g.ctrl as &dyn GekisouComboInfo);
        self.score.calculate(t, &self.combo, info)?;
        self.frame_score = self.score.score;
        if self.gk.is_some() {
            self.gekisou_after(t, results)?;
        }
        self.trace.push((t, self.score.score));
        Ok(())
    }
}
