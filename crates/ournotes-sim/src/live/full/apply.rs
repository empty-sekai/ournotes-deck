//! The appliers of live, snap and Gekisou support effects.

use super::*;

impl LiveModel {
    pub(super) fn state_mut(&mut self, item: Listed) -> &mut EffectState {
        match item {
            Listed::Live { skill, effect } => &mut self.live[skill].effects[effect].state,
            Listed::Cond { updater, u } => &mut self.cond[updater].updater.updaters[u].state,
        }
    }

    pub(super) fn apply(&mut self, item: Listed) -> Result<(), Error> {
        let (ri, k, owner_type, skill_type, key) = match item {
            Listed::Live { skill, effect } => {
                let s = &self.live[skill];
                let ri = s.effects[effect].row;
                (
                    ri,
                    s.member,
                    OWNER_MEMBER,
                    1,
                    StateKey::Live { row_id: self.rows[ri].id, member: s.member, index: s.index },
                )
            }
            Listed::Cond { updater, u } => {
                let c = &self.cond[updater];
                let upd = &c.updater.updaters[u];
                let ef = c.updater.effect(upd.effect);
                let owner_type = if c.skill_type == SKILL_TYPE_GEKISOU { OWNER_MEMBER } else { OWNER_SNAP };
                (
                    ef.row,
                    c.member,
                    owner_type,
                    c.skill_type,
                    StateKey::Cond { effect_id: ef.effect_id, index: upd.index },
                )
            }
        };
        let mut st = *self.state_mut(item);
        let owner = (k as i32).wrapping_mul(100).wrapping_add(owner_type);
        let gk_skill = skill_type == SKILL_TYPE_GEKISOU || skill_type == SKILL_TYPE_GEKISOU_SUPPORT;
        let effect_type = self.rows[ri].effect_type;
        let gk_type = GEKISOU_APPLIER_TYPES.contains(&effect_type);
        if (gk_type || effect_type == 13001) && !gk_skill && self.gk.is_none() {
            // No applier is registered for this type without Gekisou (13001 is also registered only with a
            // Gekisou controller): the applier controller skips an effect state whose type has no applier.
            return Ok(());
        }
        if matches!(effect_type, 4000..=4004 | 13001)
            && let Some(runtime) = self.raw_runtime.as_mut()
        {
            if let Some(time) = runtime.update_effect(key, st.state, &self.rows[ri])? {
                let state = self.state_mut(item);
                state.state = END_FRAME;
                state.finish_ms = time;
            }
            return Ok(());
        }
        if gk_skill || gk_type {
            let done = self.apply_gekisou(ri, &mut st, owner, key);
            *self.state_mut(item) = st;
            if done? {
                return Ok(());
            }
        }
        let (effect_type, value) = (self.rows[ri].effect_type, self.rows[ri].effect_value);
        match effect_type {
            // No live applier is registered for these types, so the applier controller skips them. The parameter
            // types are read by pre-live power consumers. Their updater / condition phases still run.
            0 | 1000..=1003 | 1500..=1503 => {}
            13005 if self.gk.is_none() => {}
            2000 | 2005 => {
                let divisor = if effect_type == 2005 { -10000f32 } else { 10000f32 };
                let m = note_factor_mill(value as f32 / divisor);
                let command = match st.state {
                    EXECUTE_FRAME => Some((st.execute_ms, m, 1)),
                    END_FRAME => Some((st.finish_ms, m.wrapping_neg(), -1)),
                    _ => None,
                };
                if let Some((time_ms, note_mill, edge)) = command {
                    if !self.luck_suppressed.get(ri).copied().unwrap_or(false) {
                        self.score.add_factor(FactorCommand {
                            time_ms,
                            owner_id: owner,
                            note_mill,
                            ..Default::default()
                        });
                    }
                    if matches!(item, Listed::Cond { .. })
                        && let Some(log) = &mut self.rush_effect_log
                    {
                        log.push((ri, time_ms, edge));
                    }
                }
            }
            2001 | 2003 => self.apply_cumulative_score(ri, &st, owner, key)?,
            2002 => {
                let m = judgement_factor_mill(value as f32 / 10000f32);
                let command = match st.state {
                    EXECUTE_FRAME => Some((st.execute_ms, m)),
                    END_FRAME => Some((st.finish_ms, m.wrapping_neg())),
                    _ => None,
                };
                if let Some((time_ms, combo_mill)) = command {
                    self.score.add_factor(FactorCommand { time_ms, owner_id: owner, combo_mill, ..Default::default() });
                }
            }
            2004 => {
                let m = judgement_factor_mill(value as f32 / 10000f32);
                let targets = self.rows[ri].targets()?.to_vec();
                for j in targets {
                    let (time_ms, judge_mill) = match st.state {
                        EXECUTE_FRAME => (st.execute_ms, m),
                        END_FRAME => (st.finish_ms, m.wrapping_neg()),
                        _ => continue,
                    };
                    self.score.add_factor(FactorCommand {
                        time_ms,
                        owner_id: owner,
                        judgement: j as i32,
                        judge_mill,
                        ..Default::default()
                    });
                }
            }
            15000 => {
                if st.state == EXECUTE_FRAME {
                    self.extend(k, value as f32);
                }
            }
            3000 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.add_life_limit(value)?;
                    if self.life_limits.contains_key(&key) {
                        return Err(Error::Game("duplicate life limit effect state".into()));
                    }
                    self.life_limits.insert(key, id);
                } else if st.state == END_FRAME {
                    let id = self
                        .life_limits
                        .remove(&key)
                        .ok_or_else(|| Error::Game("missing life limit effect state".into()))?;
                    self.life.subtract_life_limit(id);
                }
            }
            3002 => {
                if st.state == EXECUTE_FRAME {
                    self.life.damage(st.execute_ms, value, true)?;
                }
            }
            3004 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.enable_damage_reduction(st.execute_ms, value)?;
                    if self.life_reductions.contains_key(&key) {
                        return Err(Error::Game("duplicate damage reduction effect state".into()));
                    }
                    self.life_reductions.insert(key, id);
                } else if st.state == END_FRAME
                    && let Some(id) = self.life_reductions.remove(&key)
                {
                    self.life.disable_damage_reduction(st.finish_ms, id)?;
                }
            }
            3001 => {
                if st.state == EXECUTE_FRAME {
                    self.life.recovery(st.execute_ms, value, true)?;
                }
            }
            3003 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.enable_guard(st.execute_ms)?;
                    if self.guards.contains_key(&key) {
                        return Err(Error::Game("duplicate guard effect state".into()));
                    }
                    self.guards.insert(key, id);
                } else if st.state == END_FRAME {
                    let id = *self.guards.get(&key).ok_or_else(|| Error::Game("missing guard effect state".into()))?;
                    self.life.disable_guard(st.finish_ms, id)?;
                    self.guards.remove(&key);
                }
            }
            12006 | 13005 => {
                let row = &self.rows[ri];
                let effect = ConvertEffect {
                    effect_type,
                    effect_value: value,
                    effect_limit_count: row.effect_limit_count,
                    targets: row.targets()?,
                    effect_id: row.id,
                };
                if let Some(t) = self.conversion.update(key, st.state, &effect)? {
                    let s = self.state_mut(item);
                    s.state = END_FRAME;
                    s.finish_ms = t;
                }
            }
            t => return Err(Error::Unsupported(format!("skill effect type {t}"))),
        }
        Ok(())
    }

    /// Common cumulative score factors. The established Gekisou 2001 route stays in `apply_gekisou`.
    fn apply_cumulative_score(&mut self, ri: usize, st: &EffectState, owner: i32, key: StateKey) -> Result<(), Error> {
        let row = &self.rows[ri];
        let combo = row.effect_type == 2003;
        let mut value = (row.effect_value as i32).wrapping_mul(st.cumulative_count as i32);
        let max = row.max_effect_value as i32;
        if max > 0 {
            value = value.min(max);
        }
        let factor = value as f32 / 10000f32;
        let (ids, sc, score) = (&mut self.gk_appliers, &mut self.scorectl, &mut self.score);
        let add = |sc: &mut ScoreCtl, score: &mut IncrementalCalculator, t| {
            if combo {
                sc.add_combo_bonus(score, owner, t, factor)
            } else {
                sc.add_note_score_up(score, owner, t, factor)
            }
        };
        let disable = |sc: &mut ScoreCtl, score: &mut IncrementalCalculator, t, id| {
            if combo { sc.disable_combo_bonus(score, t, id) } else { sc.disable_note_score_up(score, t, id) }
        };
        match st.state {
            EXECUTE_FRAME => {
                let id = add(sc, score, st.execute_ms);
                ids.add(key, id)?;
            }
            EXECUTING => {
                let id = *ids.ids.get(&key).ok_or_else(|| Error::Game("effect state not registered".into()))?;
                let old = if combo { sc.combo_bonus_factor(id) } else { sc.score_up_factor(id) };
                if !approximately(old, factor) {
                    disable(sc, score, self.frame_time, id)?;
                    let id = add(sc, score, self.frame_time);
                    ids.ids.insert(key, id);
                }
            }
            END_FRAME => {
                let id = ids.pop(key)?;
                disable(sc, score, st.finish_ms, id)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Extends every running live skill effect of the member.
    pub(super) fn extend(&mut self, member: usize, ms: f32) {
        for s in self.live.iter_mut() {
            if s.member != member {
                continue;
            }
            for e in s.effects.iter_mut() {
                if e.state.state == EXECUTE_FRAME || e.state.state == EXECUTING {
                    e.state.extended_ms += ms;
                }
            }
        }
    }
}
