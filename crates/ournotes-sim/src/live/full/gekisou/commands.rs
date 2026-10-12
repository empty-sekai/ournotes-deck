//! The commands skills give the controller: counts, bonuses, rules, protections and factors.

use super::*;

impl Controller {
    pub(crate) fn add_just_count(&mut self, time_ms: i32, count: i32) {
        self.add_just_stack.push((time_ms, count as f32, self.current_playing_range_index()));
        self.flag();
    }

    pub(crate) fn add_gekisou_combo(&mut self, time_ms: i32, count: i32) {
        self.add_combo_stack.push((time_ms, count as f32, self.current_playing_range_index()));
        self.flag();
    }

    pub(super) fn apply_fixed_count(&mut self, idx: usize, delta: f32, range: i32, just: bool) {
        if range != -1 && range != idx as i32 {
            return;
        }
        let n = delta as i32;
        let rs = &mut self.states[idx];
        if just {
            rs.just = rs.just.wrapping_add(n);
            rs.cum_base = rs.cum_base.wrapping_add(n);
        } else {
            rs.combo = rs.combo.wrapping_add(n);
            rs.max_combo = rs.max_combo.max(rs.combo);
        }
    }

    pub(crate) fn add_luck_point(&mut self, p: i64) {
        let i = self.current_playing_range_index();
        if i < 0 {
            return;
        }
        let ls = &mut self.states[i as usize].luck;
        ls.total_bonus_point = ls.total_bonus_point.wrapping_add(p as i32);
    }

    pub(crate) fn add_luck_gauge_percent(&mut self, g: i32) -> Result<(), Error> {
        let i = self.current_playing_range_index();
        if i >= 0 {
            self.states[i as usize].luck.add_gauge(g)?;
        }
        Ok(())
    }

    pub(crate) fn current_gauge_max(&self) -> i64 {
        let i = self.current_playing_range_index();
        if i >= 0 { self.states[i as usize].luck.gauge_max } else { self.luck_gauge_max }
    }

    /// The chart-time spans of the rush score bonus commands `(added at, disabled at)`, `i32::MAX` while running.
    pub(crate) fn rush_log(&self) -> &[(i32, i32)] {
        &self.rush_log
    }

    /// Diagnostics only: every Gekisou combo bonus command `(time, change)` in filing order.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn combo_bonus_commands(&self) -> Vec<(i32, f32)> {
        self.cb_stack.iter().map(|&(t, d, _)| (t, d)).collect()
    }

    pub(crate) fn add_combo_bonus(&mut self, t: i32, bonus: f32) -> i32 {
        let i = self.next_id();
        self.combo_bonus_ids.insert(i, bonus);
        self.cb_stack.push((t, bonus, self.seq.wrapping_sub(1)));
        self.flag();
        i
    }

    pub(crate) fn subtract_combo_bonus(&mut self, t: i32, id: i32) -> Result<(), Error> {
        let v = self.combo_bonus_ids.remove(&id).ok_or_else(|| game("combo bonus id not found"))?;
        self.cb_stack.push((t, -v, self.seq.wrapping_sub(1)));
        self.flag();
        Ok(())
    }

    pub(crate) fn add_just_bonus(&mut self, t: i32, bonus: f32) -> i32 {
        let i = self.next_id();
        self.just_bonus_ids.insert(i, bonus);
        self.jb_stack.push((t, bonus, self.seq.wrapping_sub(1)));
        self.flag();
        i
    }

    pub(crate) fn subtract_just_bonus(&mut self, t: i32, id: i32) -> Result<(), Error> {
        let v = self.just_bonus_ids.remove(&id).ok_or_else(|| game("Just bonus id not found"))?;
        self.jb_stack.push((t, -v, self.seq.wrapping_sub(1)));
        self.flag();
        Ok(())
    }

    pub(crate) fn add_cumulative_rule(&mut self, unit: i64, effect: i64, max_cum: i64, max_eff: i64) -> i32 {
        let i = self.next_id();
        let range_index = self.current_playing_range_index();
        self.rules.insert(i, Rule { range_index, unit, effect, max_cum, max_eff });
        self.flag();
        i
    }

    pub(crate) fn remove_cumulative_rule(&mut self, id: i32) {
        if self.rules.remove(&id).is_some() {
            self.flag();
        }
    }

    pub(crate) fn enable_combo_protect(&mut self, t: i32, limit: i64, mask: i32) -> i32 {
        let i = self.next_id();
        if limit < 1 {
            self.pr_stack.push((t, 1, i, mask & 0xFF));
        } else {
            self.limited_ids.insert(i);
            self.pr_stack.push((t, (limit as i32).wrapping_add(1), i, mask & 0xFF));
        }
        self.flag();
        i
    }

    pub(crate) fn disable_combo_protect(&mut self, t: i32, id: i32) {
        let d = if self.limited_ids.remove(&id) { -2 } else { -1 };
        self.pr_stack.push((t, d, id, 0));
        self.flag();
    }

    /// Whether the combo and Just bonus changes filed between two judgements share one time. The recount then
    /// takes such changes together, in whatever order they were filed.
    pub(crate) fn bonus_changes_share_times(&self) -> bool {
        [&self.cb_stack, &self.jb_stack]
            .into_iter()
            .all(|stack| stack.windows(2).all(|pair| pair[0].2 != pair[1].2 || pair[0].0 == pair[1].0))
    }

    /// Whether every binary32 sum of filed lot and gauge factors is exact in any filing order: the factors of each
    /// kind are integer multiples of one power of two, and their absolute values sum to at most 2^24 of it.
    pub(crate) fn factor_sums_exact(&self) -> bool {
        [&self.storage.lot_cmds, &self.storage.gauge_cmds].into_iter().all(|cmds| exact_sums(cmds))
    }

    pub(crate) fn add_lot_probability_up(&mut self, t: i32, f: f32) -> i32 {
        self.storage.add(false, t, f)
    }

    pub(crate) fn add_gauge_up(&mut self, t: i32, f: f32) -> i32 {
        self.storage.add(true, t, f)
    }

    pub(crate) fn subtract_lot_probability_up(&mut self, t: i32, id: i32) -> Result<(), Error> {
        self.storage.subtract(false, t, id)
    }

    pub(crate) fn subtract_gauge_up(&mut self, t: i32, id: i32) -> Result<(), Error> {
        self.storage.subtract(true, t, id)
    }
}
