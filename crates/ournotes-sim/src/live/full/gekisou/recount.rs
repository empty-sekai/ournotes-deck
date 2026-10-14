//! The incremental recount of the combo and Just counts of the ranges from their judgement history.

use super::*;

/// Where the incremental recount of a range stopped.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Resume {
    processed: usize,
    combo: i32,
    max_combo: i32,
    just: i32,
    raw_just: i32,
    cum_base: i32,
    combo_bonus: f32,
    just_bonus: f32,
    i_cb: usize,
    i_jb: usize,
    i_add_just: usize,
    i_add_combo: usize,
}

/// A cumulative Just rule: every `unit` Just counts add `effect`, at most `max_cum` steps and `max_eff` in total.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rule {
    pub(super) range_index: i32,
    pub(super) unit: i64,
    pub(super) effect: i64,
    pub(super) max_cum: i64,
    pub(super) max_eff: i64,
}

fn cumulative_just_bonus<'a>(rules: impl Iterator<Item = &'a Rule>, range_index: i32, x: i32) -> i32 {
    let mut total = 0i32;
    for r in rules {
        if r.range_index == range_index && r.unit > 0 {
            let mut steps = (x as i64 / r.unit) as i32 as i64;
            if !(steps <= r.max_cum || r.max_cum == 0 || r.max_cum < 0) {
                steps = r.max_cum;
            }
            let mut v = (steps as i128 * r.effect as i128) as i32 as i64;
            if r.max_eff > 0 && r.max_eff <= v {
                v = r.max_eff;
            }
            total = total.wrapping_add(v as i32);
        }
    }
    total
}

fn score_frame(ms: i32) -> i32 {
    if ms > 0 { ceil_to_i32(ms as f32 / 40f32) } else { 0 }
}

fn previous_frame_end_ms(t: i32) -> i32 {
    score_frame(t).wrapping_mul(SCORE_FRAME_MS).wrapping_sub(SCORE_FRAME_MS)
}

/// The last history index whose time is `<= max_ms` (a binary search over the first `count` entries).
fn last_judgement_index(h: &[(i32, i32, i32)], count: usize, max_ms: i32) -> i32 {
    let mut hi = (count as i32).wrapping_sub(1);
    if hi < 1 {
        return 0;
    }
    let mut lo = 0i32;
    loop {
        let mid = lo.wrapping_add(sdiv(hi.wrapping_sub(lo).wrapping_add(1), 2));
        if h[mid as usize].0 <= max_ms {
            lo = mid;
        } else {
            hi = mid.wrapping_sub(1);
        }
        if lo >= hi {
            return lo;
        }
    }
}

impl Controller {
    // --- counts ---------------------------------------------------------------------------------------------

    /// Recounts the combo and Just counts of every range that is not past its end delay from its history.
    pub(super) fn recalculate(&mut self) {
        for idx in 0..self.states.len() {
            if self.states[idx].state >= S_DELAY || self.history[idx].is_empty() {
                continue;
            }
            self.combo_versions[idx] = self.combo_versions[idx].wrapping_add(1);
            let rr = self.resume[idx];
            let (mut cb, mut jb, mut i_cb, mut i_jb, mut i_pr, mut k);
            let (mut i_add_just, mut i_add_combo);
            let hlen = self.history[idx].len();
            if !self.full_recalc && self.pr_stack.is_empty() && 1 <= rr.processed && rr.processed <= hlen {
                let rs = &mut self.states[idx];
                (rs.combo, rs.max_combo, rs.just, rs.raw_just, rs.cum_base) =
                    (rr.combo, rr.max_combo, rr.just, rr.raw_just, rr.cum_base);
                (cb, jb, i_cb, i_jb, i_pr, k) = (rr.combo_bonus, rr.just_bonus, rr.i_cb, rr.i_jb, 0usize, rr.processed);
                (i_add_just, i_add_combo) = (rr.i_add_just, rr.i_add_combo);
            } else {
                let rs = &mut self.states[idx];
                (rs.combo, rs.max_combo, rs.just, rs.raw_just, rs.cum_base) = (0, 0, 0, 0, 0);
                self.snapshot[idx].clear();
                self.unlimited.clear();
                self.limited.clear();
                (cb, jb, i_cb, i_jb, i_pr, k) = (1f32, 1f32, 0, 0, 0, 0);
                (i_add_just, i_add_combo) = (0, 0);
            }
            while k < hlen {
                let (et, j, es) = self.history[idx][k];
                while i_cb < self.cb_stack.len() {
                    let (ct, d, cs) = self.cb_stack[i_cb];
                    if et <= ct && (et < ct || es < cs) {
                        break;
                    }
                    cb += d;
                    i_cb += 1;
                }
                while i_jb < self.jb_stack.len() {
                    let (ct, d, cs) = self.jb_stack[i_jb];
                    if et <= ct && (et < ct || es < cs) {
                        break;
                    }
                    jb += d;
                    i_jb += 1;
                }
                while i_pr < self.pr_stack.len() && et >= self.pr_stack[i_pr].0 {
                    let (_, d, pid, mask) = self.pr_stack[i_pr];
                    if d < 2 {
                        if d == 1 {
                            self.unlimited.push((pid, mask));
                        } else if d < -1 {
                            if let Some(p) = self.limited.iter().position(|x| x.0 == pid) {
                                self.limited.remove(p);
                            }
                        } else if let Some(p) = self.unlimited.iter().position(|x| x.0 == pid) {
                            self.unlimited.remove(p);
                        }
                    } else {
                        self.limited.push((pid, d.wrapping_sub(1), mask));
                    }
                    i_pr += 1;
                }
                while i_add_just < self.add_just_stack.len() && self.add_just_stack[i_add_just].0 <= et {
                    let (_, d, range) = self.add_just_stack[i_add_just];
                    self.apply_fixed_count(idx, d, range, true);
                    i_add_just += 1;
                }
                while i_add_combo < self.add_combo_stack.len() && self.add_combo_stack[i_add_combo].0 <= et {
                    let (_, d, range) = self.add_combo_stack[i_add_combo];
                    self.apply_fixed_count(idx, d, range, false);
                    i_add_combo += 1;
                }
                self.entry(idx, j, floor_to_i32(cb), floor_to_i32(jb));
                let c = self.states[idx].combo;
                self.snapshot[idx].push(c);
                k += 1;
            }
            let rs = &self.states[idx];
            self.resume[idx] = Resume {
                processed: hlen,
                combo: rs.combo,
                max_combo: rs.max_combo,
                just: rs.just,
                raw_just: rs.raw_just,
                cum_base: rs.cum_base,
                combo_bonus: cb,
                just_bonus: jb,
                i_cb,
                i_jb,
                i_add_just,
                i_add_combo,
            };
            // Native saves resume BEFORE trailing additions. On the next recount these are replayed once.
            for i in i_add_just..self.add_just_stack.len() {
                let (_, d, range) = self.add_just_stack[i];
                self.apply_fixed_count(idx, d, range, true);
            }
            for i in i_add_combo..self.add_combo_stack.len() {
                let (_, d, range) = self.add_combo_stack[i];
                self.apply_fixed_count(idx, d, range, false);
            }
        }
        self.full_recalc = false;
    }

    /// One history entry of the recount.
    fn entry(&mut self, idx: usize, j: i32, i_combo: i32, i_just: i32) {
        let rs = &mut self.states[idx];
        if (j.wrapping_sub(3) as u32) < 0xFFFF_FFFE {
            if (j.wrapping_sub(7) as u32) > 0xFFFF_FFFB {
                rs.combo = rs.combo.wrapping_add(i_combo);
                rs.max_combo = rs.max_combo.max(rs.combo);
            } else {
                return;
            }
        } else {
            let bit = 1i32 << (j as u32 & 31);
            let eff = |m: i32| if m != 0 { m } else { 6 };
            let mut prot = self.unlimited.iter().any(|&(_, m)| eff(m) & bit != 0);
            let mut i = self.limited.len();
            while i > 0 {
                i -= 1;
                let (pid, rem, m) = self.limited[i];
                if eff(m) & bit != 0 {
                    let rem = rem.wrapping_sub(1);
                    if rem < 1 {
                        self.limited.remove(i);
                    } else {
                        self.limited[i] = (pid, rem, m);
                    }
                    prot = true;
                }
            }
            if !prot {
                rs.max_combo = rs.max_combo.max(rs.combo);
                rs.combo = 0;
            }
        }
        if j == J_JUST {
            rs.raw_just = rs.raw_just.wrapping_add(1);
            rs.cum_base = rs.cum_base.wrapping_add(i_just);
            let c = cumulative_just_bonus(self.rules.values(), idx as i32, rs.cum_base);
            rs.just = c.wrapping_add(i_just).wrapping_add(rs.just);
        }
    }

    pub(super) fn timing_combo_in_range(&self, idx: usize, t: i32) -> i32 {
        let (h, sn) = (&self.history[idx], &self.snapshot[idx]);
        let n = h.len().min(sn.len());
        if n == 0 {
            return 0;
        }
        let prev_end = previous_frame_end_ms(t);
        if prev_end < h[0].0 {
            return 0;
        }
        sn[last_judgement_index(h, n, prev_end) as usize]
    }
}
