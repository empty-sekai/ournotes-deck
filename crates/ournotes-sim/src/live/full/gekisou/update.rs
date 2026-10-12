//! The per-frame update of the ranges: their states, the judged notes and the luck lottery.

use super::*;

impl Controller {
    // --- luck -----------------------------------------------------------------------------------------------

    fn update_luck(&mut self, idx: usize, nt: i32, tn: i32, nid: i32, j: i32, env: &mut Env) -> Result<(), Error> {
        if self.luck_weighted {
            return Ok(());
        }
        let buff = floor_to_i32(factor_at(&self.storage.lot_cmds, tn) * 100f32);
        self.machine.set_weight_buff(buff);
        let gup = factor_at(&self.storage.gauge_cmds, tn);
        let base = self.machine.base_point(nt, j, env.random)?;
        let add = floor_to_i32((gup + 1f32) * base as f32);
        self.states[idx].luck.add_gauge(add)?;
        self.consume_lot(idx, tn, nid, env)
    }

    fn disable_rush(&mut self, t: i32, env: &mut Env) -> Result<(), Error> {
        if self.rush_id != 0 {
            env.handle.disable(t, self.rush_id)?;
            self.rush_id = 0;
            if let Some(open) = self.rush_log.iter_mut().rev().find(|x| x.1 == i32::MAX) {
                open.1 = t;
            }
        }
        Ok(())
    }

    fn consume_lot(&mut self, idx: usize, t: i32, _note_id: i32, env: &mut Env) -> Result<(), Error> {
        if self.states[idx].state > S_END || self.states[idx].luck.lot_count < 1 {
            return Ok(());
        }
        self.states[idx].luck.lot_count -= 1;
        if self.states[idx].luck.next == INVALID {
            self.states[idx].luck.next = self.machine.luck_bonus(NONE_LOT, t, env.random)?;
        }
        let (next, rush) = (self.states[idx].luck.next, self.states[idx].luck.rush_combo);
        if rush == 0 {
            if next == CRITICAL {
                self.rush_id = env.handle.add(t, self.rush_percent as i32);
                self.rush_log.push((t, i32::MAX));
            } else {
                self.disable_rush(t, env)?;
            }
        } else if next != CRITICAL {
            self.disable_rush(t, env)?;
        }
        self.states[idx].luck.add_score(next)?;
        if next != INVALID {
            self.lot_result = next;
            self.lot_results.push(next);
        }
        let lot_type = self.states[idx].luck.current_lot_type();
        self.states[idx].luck.next = self.machine.luck_bonus(lot_type, t, env.random)?;
        Ok(())
    }

    fn pending_lots(&mut self, t: i32, env: &mut Env) -> Result<(), Error> {
        if self.luck_weighted {
            return Ok(());
        }
        for pi in 0..self.playing.len() {
            let idx = self.playing[pi];
            let rs = &self.states[idx];
            if self.ranges[idx].mission == M_LUCK
                && rs.state == S_PLAYING
                && rs.luck.lot_count != 0
                && self.lot_result == INVALID
            {
                let buff = floor_to_i32(factor_at(&self.storage.lot_cmds, t) * 100f32);
                self.machine.set_weight_buff(buff);
                self.consume_lot(idx, t, -1, env)?;
            }
        }
        Ok(())
    }

    fn judge(&mut self, idx: usize, nt: i32, tn: i32, nid: i32, j: i32, env: &mut Env) -> Result<(), Error> {
        if j == J_PASS || j == J_WAIT {
            return Ok(());
        }
        self.history[idx].push((tn, j, self.seq));
        self.combo_versions[idx] = self.combo_versions[idx].wrapping_add(1);
        self.seq = self.seq.wrapping_add(1);
        self.needs_recalc = true;
        if (j.wrapping_sub(3) as u32) < 4 {
            self.states[idx].last_combo_ms = tn;
        }
        if j == J_JUST {
            self.states[idx].last_just_ms = tn;
        }
        if self.ranges[idx].mission == M_LUCK {
            if self.states[idx].state < S_START {
                self.pending[idx].push((nt, tn, nid, j));
            } else {
                self.update_luck(idx, nt, tn, nid, j, env)?;
            }
        }
        Ok(())
    }

    /// The controller's update after the frame's skills and score: judged notes of the ranges, the recount, the
    /// fever-end scores and the pending lots.
    pub(crate) fn update(
        &mut self,
        t: i32,
        judged: &[GkNote],
        fever_updates: &[(usize, u8)],
        current_score: i32,
        env: &mut Env,
    ) -> Result<(), Error> {
        for idx in 0..self.states.len() {
            if self.states[idx].state > S_STANDBY && !self.pending[idx].is_empty() {
                let pending = std::mem::take(&mut self.pending[idx]);
                for (nt, tn, nid, j) in pending {
                    self.update_luck(idx, nt, tn, nid, j, env)?;
                }
            }
        }
        for idx in 0..self.states.len() {
            for &(nid, nt, tn, j) in judged {
                if self.ranges[idx].targets.contains(&nid) {
                    self.judge(idx, nt, tn, nid, j, env)?;
                }
            }
        }
        if self.needs_recalc {
            self.recalculate();
            self.needs_recalc = false;
        }
        for &(idx, fs) in fever_updates {
            let rs = self.states.get_mut(idx).ok_or_else(|| game(FEVER_WITHOUT_RANGE))?;
            if fs == FEVER_END {
                rs.end_score = current_score;
            }
        }
        self.pending_lots(t, env)
    }

    fn state_update(&mut self, idx: usize) {
        if !self.state_updates.contains(&idx) {
            self.state_updates.push(idx);
        }
    }

    fn elapsed_ms(rs: &RangeState) -> i64 {
        let v = rs.sw_elapsed * 1000f32;
        if v == f32::INFINITY { i64::MIN } else { v as i64 }
    }

    /// The controller's update before the frame: range state machine and stopwatches.
    pub(crate) fn before_update(
        &mut self,
        dt: f32,
        t: i32,
        fever_updates: &[(usize, u8)],
        current_score: i32,
        env: &mut Env,
    ) -> Result<(), Error> {
        self.state_updates.clear();
        self.lot_result = INVALID;
        self.lot_results.clear();
        self.current_playing_index = -1;
        for (rs, r) in self.states.iter_mut().zip(&self.ranges) {
            rs.time_to = r.start_ms.wrapping_sub(t);
        }
        let mut remove = Vec::new();
        for pi in 0..self.playing.len() {
            let idx = self.playing[pi];
            let st = self.states[idx].state;
            if st < S_END {
                if st == S_START {
                    self.states[idx].state = S_PLAYING;
                    self.state_update(idx);
                    self.current_playing_index = idx as i32;
                } else if st == S_PLAYING {
                    self.current_playing_index = idx as i32;
                }
            } else if st == S_END {
                self.current_playing_index = idx as i32;
                let rs = &mut self.states[idx];
                if !rs.sw_stopped {
                    rs.sw_elapsed += dt;
                }
                if self.last_note_delay_ms <= Self::elapsed_ms(rs) {
                    rs.sw_elapsed = 0f32;
                    rs.sw_stopped = false;
                    rs.state = S_DELAY;
                    self.state_update(idx);
                }
            } else if st == S_DELAY {
                let rs = &mut self.states[idx];
                if !rs.sw_stopped {
                    rs.sw_elapsed += dt;
                }
                if COMPLETE_DELAY_MS <= Self::elapsed_ms(rs) {
                    rs.state = S_COMPLETE;
                    rs.sw_stopped = true;
                    self.state_update(idx);
                }
            } else if st == S_COMPLETE {
                self.states[idx].state = S_FINISH;
                self.state_update(idx);
                remove.push(idx);
                self.disable_rush(t, env)?;
            }
        }
        for idx in remove {
            if let Some(p) = self.playing.iter().position(|&x| x == idx) {
                self.playing.remove(p);
            }
        }
        for &(idx, fs) in fever_updates {
            let rs = self.states.get_mut(idx).ok_or_else(|| game(FEVER_WITHOUT_RANGE))?;
            if fs == FEVER_END {
                rs.state = S_END;
                rs.sw_elapsed = 0f32;
                rs.sw_stopped = false;
                self.state_update(idx);
            } else if fs == FEVER_FEVER {
                rs.state = S_START;
                rs.start_score = current_score;
                self.playing.push(idx);
                self.state_update(idx);
                self.current_playing_index = idx as i32;
                if self.luck_weighted && self.ranges[idx].mission == M_LUCK {
                    if self.rush_id != 0 {
                        return Err(Error::Unsupported("LUCK coefficient: a luck range starts during a rush".into()));
                    }
                    self.rush_id = env.handle.add(t, self.rush_percent as i32);
                    self.rush_log.push((t, i32::MAX));
                }
            }
        }
        for idx in 0..self.states.len() {
            if self.states[idx].state == S_WAIT && self.states[idx].time_to < STANDBY_MS {
                self.states[idx].state = S_STANDBY;
                self.state_update(idx);
            }
        }
        Ok(())
    }
}

impl GekisouComboInfo for Controller {
    fn gekisou_combo(&self, time_ms: i32) -> Option<i32> {
        let i =
            self.ranges.iter().position(|r| r.mission == M_COMBO && r.start_ms <= time_ms && time_ms <= r.end_ms)?;
        Some(self.timing_combo_in_range(i, time_ms))
    }

    fn combo_windows(&self, out: &mut Vec<(i32, i32, u64)>) {
        for (r, &version) in self.ranges.iter().zip(&self.combo_versions) {
            if r.mission == M_COMBO {
                out.push((r.start_ms, r.end_ms, version));
            }
        }
    }
}
