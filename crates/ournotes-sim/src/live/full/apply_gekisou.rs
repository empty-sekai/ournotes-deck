//! The Gekisou appliers and the per-frame Gekisou range updates of a Live model.

use super::*;

impl LiveModel {
    /// The Gekisou update after the frame: the ranges take the judged notes; the first range that completed this
    /// frame gets its start and end scores and its rank bonus.
    pub(super) fn gekisou_after(&mut self, t: i32, results: &[(LiveNote, i32)]) -> Result<(), Error> {
        let Some(gk) = self.gk.as_mut() else { return Ok(()) };
        let mut judged = std::mem::take(&mut self.scratch.gk_judged);
        judged.clear();
        judged.extend(results.iter().map(|(n, j)| (n.note_id, n.note_operate_type, n.time_ms, *j)));
        let current = self.score.score;
        {
            let mut h = Handle { sc: &mut self.scorectl, score: &mut self.score };
            let mut env = Env { random: &mut self.random, handle: &mut h };
            gk.ctrl.update(t, &judged, &gk.fever_updates, current, &mut env)?;
        }
        if gk.external_ranking && !gk.solo_score_queries {
            let expression = self.score.program_snapshot();
            for &(idx, state) in &gk.fever_updates {
                if state == gekisou::FEVER_END {
                    gk.program_rank_snapshots[idx].1 = expression;
                }
            }
        }
        if gk.external_ranking && !gk.solo_score_queries {
            let query = self.score.bounds_last_query();
            for &(idx, state) in &gk.fever_updates {
                if state == gekisou::FEVER_END {
                    gk.rank_snapshot_queries[idx].1 = query;
                }
            }
        }
        if self.score.bounds_trace.is_some() {
            // A lottery may file Rush commands at any judged chart time, or at this frame's pending draw.
            // These are possible filings, not observations of the recorder's particular lottery trajectory.
            for &(_, _, time, _) in &judged {
                self.score.bounds_potential_rush(time, false);
            }
            self.score.bounds_potential_rush(t, false);
            self.score.bounds_probability_ready(t);
        }
        self.scratch.gk_judged = judged;
        gk.prev_lots.clear();
        gk.prev_lots.extend_from_slice(&gk.ctrl.lot_results);
        gk.prev_lot_ms = if gk.prev_lots.is_empty() { 0 } else { t };
        if gk.external_ranking {
            // Network ranking reads the range's score snapshots taken by the controller. Unlike solo
            // ranking, it does not rewind the score calculator to the range's start and end timestamps.
            for idx in gk.ctrl.state_updates.iter().copied() {
                if gk.ctrl.states[idx].state != S_COMPLETE || gk.completed_scores[idx] {
                    continue;
                }
                // The fixed-rank solo adapter retains solo's timestamp queries explicitly; network
                // ranking keeps the controller snapshots and never enters this branch.
                if gk.solo_score_queries {
                    let r = &gk.ctrl.ranges[idx];
                    let info = Some(&gk.ctrl as &dyn GekisouComboInfo);
                    let s0 = self.score.calculate(r.start_ms, &self.combo, info)?;
                    gk.program_rank_snapshots[idx].0 = self.score.program_snapshot();
                    gk.rank_snapshot_queries[idx].0 = self.score.bounds_last_query();
                    let s1 = self.score.calculate(r.end_ms, &self.combo, info)?;
                    gk.program_rank_snapshots[idx].1 = self.score.program_snapshot();
                    gk.rank_snapshot_queries[idx].1 = self.score.bounds_last_query();
                    gk.ctrl.states[idx].start_score = s0;
                    gk.ctrl.states[idx].end_score = s1;
                }
                gk.completed_scores[idx] = true;
                // SoloGekisouRankingUpdater returns after the first completed range, including when
                // overlapping ranges complete together. Keep that schedule in the solo adapter.
                if gk.solo_score_queries {
                    break;
                }
            }
            for idx in 0..gk.pending_ranks.len() {
                if !gk.completed_scores[idx] {
                    continue;
                }
                let Some((rank, pct)) = gk.pending_ranks[idx].take() else {
                    continue;
                };
                let bonus = ((i128::from(gk.ctrl.range_score(idx)) * i128::from(pct)) / 100) as i32;
                self.score.add_fixed(gk.ctrl.ranges[idx].end_ms, bonus);
                let (start, end) = gk.program_rank_snapshots[idx];
                self.score.record_rank_bonus(start, end, pct)?;
                let (start, end) = gk.rank_snapshot_queries[idx];
                self.score.bounds_rank(idx, gk.ctrl.ranges[idx].end_ms, pct, start, end);
                gk.rank_bonus.push((idx, rank, bonus, pct));
                gk.rank_applications.push((self.trace.len(), idx));
                self.prev_confirmed_rank = Some(rank);
            }
            return Ok(());
        }
        let Some(idx) = gk.ctrl.state_updates.iter().copied().find(|&i| gk.ctrl.states[i].state == S_COMPLETE) else {
            return Ok(());
        };
        let (start, end) = (gk.ctrl.ranges[idx].start_ms, gk.ctrl.ranges[idx].end_ms);
        let info = Some(&gk.ctrl as &dyn GekisouComboInfo);
        let s0 = self.score.calculate(start, &self.combo, info)?;
        let program_start = self.score.program_snapshot();
        let bounds_start = self.score.bounds_last_query();
        let s1 = self.score.calculate(end, &self.combo, info)?;
        let program_end = self.score.program_snapshot();
        let bounds_end = self.score.bounds_last_query();
        gk.ctrl.states[idx].start_score = s0;
        gk.ctrl.states[idx].end_score = s1;
        if idx < gk.factors.len() {
            let (rank, bonus, pct) = gekisou::solo_rank_bonus(idx, gk.ctrl.range_score(idx), &gk.factors);
            self.score.add_fixed(end, bonus);
            self.score.bounds_rank(idx, end, pct, bounds_start, bounds_end);
            self.score.record_rank_bonus(program_start, program_end, pct)?;
            gk.rank_bonus.push((idx, rank, bonus, pct));
            self.prev_confirmed_rank = Some(rank); // feed actual confirmation into the NEXT frame
        }
        Ok(())
    }

    /// The Gekisou appliers; `Ok(false)` when the effect type is not one of them.
    pub(super) fn apply_gekisou(
        &mut self,
        ri: usize,
        st: &mut EffectState,
        owner: i32,
        key: StateKey,
    ) -> Result<bool, Error> {
        let row = &self.rows[ri];
        let (et, v) = (row.effect_type, row.effect_value);
        let frame_t = self.frame_time;
        let ga = &mut self.gk_appliers;
        let c = &mut self.gk.as_mut().ok_or_else(|| Error::Game("Gekisou effect without Gekisou".into()))?.ctrl;
        let s = st.state;
        match et {
            12000 | 13000 => {
                if s == EXECUTE_FRAME {
                    let id = if et == 12000 {
                        c.add_combo_bonus(st.execute_ms, v as f32)
                    } else {
                        c.add_just_bonus(st.execute_ms, v as f32)
                    };
                    ga.add(key, id)?;
                } else if s == END_FRAME {
                    let id = ga.pop(key)?;
                    if et == 12000 {
                        c.subtract_combo_bonus(st.finish_ms, id)?;
                    } else {
                        c.subtract_just_bonus(st.finish_ms, id)?;
                    }
                }
            }
            13002 => {
                if s == END_FRAME {
                    if let Some(id) = ga.ids.remove(&key) {
                        c.remove_cumulative_rule(id);
                    }
                } else if s == EXECUTE_FRAME && !ga.ids.contains_key(&key) {
                    let id = c.add_cumulative_rule(st.cumulative_unit, v, st.cumulative_max, row.max_effect_value);
                    ga.ids.insert(key, id);
                }
            }
            11000 | 11001 => {
                if s == EXECUTE_FRAME {
                    let f = v as f32 / 10000f32;
                    let id = if et == 11000 {
                        c.add_lot_probability_up(st.execute_ms, f)
                    } else {
                        c.add_gauge_up(st.execute_ms, f)
                    };
                    ga.add(key, id)?;
                } else if s == END_FRAME {
                    let id = ga.pop(key)?;
                    if et == 11000 {
                        c.subtract_lot_probability_up(st.finish_ms, id)?;
                    } else {
                        c.subtract_gauge_up(st.finish_ms, id)?;
                    }
                }
            }
            11002 | 11004 | 12002 | 12003 | 13003 | 13004 => {
                if s == EXECUTE_FRAME {
                    let mut count = v as i32;
                    if matches!(et, 11004 | 12003 | 13004) {
                        count = count.wrapping_mul(st.cumulative_count as i32);
                        let cap = row.max_effect_value as i32;
                        if cap > 0 {
                            count = count.min(cap);
                        }
                    }
                    match et {
                        11002 | 11004 => c.add_luck_point(i64::from(count)),
                        12002 | 12003 => c.add_gekisou_combo(frame_t, count),
                        _ => c.add_just_count(frame_t, count),
                    }
                }
            }
            11003 => {
                if s == EXECUTE_FRAME {
                    let g = (c.current_gauge_max() as i128 * v as i128) as i32;
                    c.add_luck_gauge_percent(floor_to_i32(g as f32 / 10000f32))?;
                }
            }
            12004 => {
                ga.limit(key, st);
                let s = st.state;
                if s == EXECUTE_FRAME {
                    let mut mask = 0i64;
                    for &j in row.targets()? {
                        if j != 0 && j != -1 {
                            mask |= 1i64 << (j & 31);
                        }
                    }
                    let id = c.enable_combo_protect(st.execute_ms, row.effect_limit_count, (mask & 0xFF) as i32);
                    ga.add(key, id)?;
                } else if s == END_FRAME
                    && !ga.exhausted.remove(&key)
                    && let Some(id) = ga.ids.remove(&key)
                {
                    c.disable_combo_protect(st.finish_ms, id);
                }
            }
            11005 => {
                ga.limit(key, st);
                let s = st.state;
                if s == EXECUTE_FRAME {
                    let id = c.machine.enable_minimum(minimum_result_of(v), row.effect_limit_count);
                    ga.add(key, id)?;
                } else if s == EXECUTING {
                    if let Some(&id) = ga.ids.get(&key)
                        && !c.machine.is_minimum_active(id)
                    {
                        let mut t = c.machine.take_last_consumed(id);
                        if t < 0 {
                            t = frame_t;
                        }
                        c.machine.disable_minimum(id);
                        ga.ids.remove(&key);
                        ga.exhausted.insert(key);
                        ga.limit_finished.insert(key, t);
                    }
                } else if s == END_FRAME
                    && !ga.exhausted.remove(&key)
                    && let Some(id) = ga.ids.remove(&key)
                {
                    c.machine.disable_minimum(id);
                }
            }
            2001 => {
                let mut n = (v as i128 * st.cumulative_count as i128) as i32;
                if row.max_effect_value > 0 && row.max_effect_value < n as i64 {
                    n = row.max_effect_value as i32;
                }
                let f = n as f32 / 10000f32;
                let (sc, score) = (&mut self.scorectl, &mut self.score);
                if s == EXECUTE_FRAME {
                    let id = sc.add_note_score_up(score, owner, st.execute_ms, f);
                    ga.add(key, id)?;
                } else if s == EXECUTING {
                    let id = *ga.ids.get(&key).ok_or_else(|| Error::Game("effect state not registered".into()))?;
                    if !approximately(sc.score_up_factor(id), f) {
                        sc.disable_note_score_up(score, frame_t, id)?;
                        let id = sc.add_note_score_up(score, owner, frame_t, f);
                        ga.ids.insert(key, id);
                    }
                } else if s == END_FRAME {
                    let id = ga.pop(key)?;
                    sc.disable_note_score_up(score, st.finish_ms, id)?;
                }
            }
            4004 => {}
            _ => return Ok(false),
        }
        Ok(true)
    }
}
