//! The evaluation of a condition in a frame.

use super::*;

impl Checker {
    pub(crate) fn check(&mut self, ctx: &mut CheckCtx) -> Result<(bool, i64), Error> {
        match self {
            Checker::Scripted { script, sticky, memo, true_hits, life } => {
                let hit = match *memo {
                    Some(hit) if *sticky => hit,
                    _ => {
                        let decided = life.and_then(|(ty, v)| script.borrow().life_decides(ty, v));
                        let hit = match decided {
                            Some(hit) => hit,
                            None => script.borrow_mut().answer(ctx.frame_time)?,
                        };
                        if *sticky {
                            *memo = Some(hit);
                        }
                        hit
                    }
                };
                Ok((hit, if hit { *true_hits } else { 0 }))
            }
            Checker::LiveComboAtLeast(threshold) => {
                let hit = *threshold > 0 && ctx.current_combo >= *threshold;
                Ok((hit, i64::from(hit)))
            }
            Checker::LiveComboMultiple { threshold, previous } => {
                let combo = ctx.current_combo;
                if combo < 1 || *threshold < 1 {
                    *previous = 0;
                    return Ok((false, 0));
                }
                if combo <= *previous {
                    *previous = combo;
                    return Ok((false, 0));
                }
                let hit = combo / *threshold - *previous / *threshold;
                *previous = combo;
                Ok((hit > 0, i64::from(hit)))
            }
            Checker::ElapsedTime { period, elapsed, previous } => {
                *elapsed = elapsed.wrapping_add(ctx.frame_time.wrapping_sub(*previous));
                *previous = ctx.frame_time;
                let hit = *elapsed >= *period;
                if hit {
                    *elapsed = elapsed.wrapping_sub(*period);
                }
                Ok((hit, i64::from(hit)))
            }
            Checker::SnapGekisouStart { previous } => {
                if let Some(g) = ctx.gk {
                    for &index in &g.ctrl.state_updates {
                        if g.ctrl.states[index].state == S_START && index as i32 != *previous {
                            *previous = index as i32;
                            return Ok((true, 1));
                        }
                    }
                }
                Ok((false, 0))
            }
            Checker::And { items, resettable } => {
                let mut hit = 0i64;
                for i in 0..items.len() {
                    let (ok, h) = items[i].check(ctx)?;
                    hit = hit.max(h);
                    if !ok {
                        if !resettable[i] {
                            for (c, &r) in items.iter_mut().zip(resettable.iter()) {
                                if r {
                                    c.reset_count(ctx)?;
                                }
                            }
                        }
                        return Ok((false, 0));
                    }
                }
                Ok((true, hit))
            }
            Checker::Or(items) => {
                let mut hit = 0i64;
                for c in items.iter_mut() {
                    let (ok, h) = c.check(ctx)?;
                    hit = hit.max(h);
                    if ok {
                        return Ok((true, hit));
                    }
                }
                Ok((false, 0))
            }
            Checker::Not(inner) => {
                let (ok, h) = inner.check(ctx)?;
                Ok((!ok, h))
            }
            Checker::LifeAtLeast(v) => {
                let v = v.ok_or_else(no_value)?;
                let ok = v <= ctx.life.get_life_at_ms(ctx.frame_time)? as i64;
                Ok((ok, ok as i64))
            }
            Checker::LifeAtMost(v) => {
                let v = v.ok_or_else(no_value)?;
                let ok = (ctx.life.get_life_at_ms(ctx.frame_time)? as i64) <= v;
                Ok((ok, ok as i64))
            }
            Checker::LifeGreater(v) => {
                let ok = v.ok_or_else(no_value)? < ctx.life.get_life_at_ms(ctx.frame_time)? as i64;
                Ok((ok, i64::from(ok)))
            }
            Checker::LifeLess(v) => {
                let ok = (ctx.life.get_life_at_ms(ctx.frame_time)? as i64) < v.ok_or_else(no_value)?;
                Ok((ok, i64::from(ok)))
            }
            Checker::LifeChanged { previous } => {
                let life = ctx.life.get_life_at_ms(ctx.frame_time)?;
                let ok = *previous != life;
                *previous = life;
                Ok((ok, 0))
            }
            Checker::LifePercent { threshold, previous } => {
                let life = ctx.life.get_life_at_ms(ctx.frame_time)?;
                let ok = *previous > *threshold && life <= *threshold;
                *previous = life;
                Ok((ok, i64::from(ok)))
            }
            Checker::LifeDelta { threshold, increase, count, previous } => {
                let life = ctx.life.get_life_at_ms(ctx.frame_time)?;
                let change = if *increase { life.wrapping_sub(*previous) } else { previous.wrapping_sub(life) };
                *previous = life;
                if change > 0 {
                    *count = count.wrapping_add(change);
                }
                if *threshold <= 0 {
                    return Err(Error::Unsupported("nonpositive life change threshold".into()));
                }
                let hits = if *count >= *threshold { *count / *threshold } else { 0 };
                if hits > 0 {
                    *count %= *threshold;
                }
                Ok((hits > 0, hits as i64))
            }
            Checker::SameMemberLiveSkill(k) => {
                let ok = ctx.events.iter().any(|&(index, _)| index == *k);
                Ok((ok, ok as i64))
            }
            // LUCK coefficient lives draw no lottery, so the luck chain effects a probability gates do nothing
            // (`LiveModel::set_luck_weights` admits no other gated effect).
            Checker::Probability(_) if ctx.gk.is_some_and(|g| g.ctrl.luck_weighted) => Ok((false, 0)),
            Checker::Probability(rate) => Ok((ctx.random.value(SKILL) < *rate, 0)),
            Checker::ConditionedProbability(rate) => Ok((ctx.random.nominal_skill_probability(*rate)?, 0)),
            Checker::Fixed(ok) => Ok((*ok, *ok as i64)),
            Checker::NoteJudgementMatch { kind, targets, override_ms } => {
                *override_ms = None;
                let mut hit = 0;
                for &(nid, j, tn) in ctx.judged {
                    if *kind == 1020 && matches!(j, -1 | 7) {
                        continue;
                    }
                    for &target in targets.iter() {
                        let matches = if *kind == 1000 {
                            target == j as i64
                        } else {
                            if !(1..=6).contains(&target) || !(1..=6).contains(&j) {
                                return Err(Error::Game("unknown judgement comparison rank".into()));
                            }
                            if *kind == 1010 { j as i64 >= target } else { j as i64 <= target }
                        };
                        if matches {
                            hit += 1;
                            *override_ms = (nid >= 0).then_some(tn);
                            break;
                        }
                    }
                }
                Ok((hit > 0, hit))
            }
            Checker::NoteJudgementCount { n, consecutive, targets, count, override_ms } => {
                *override_ms = None;
                let (mut trig, mut trig_ms, mut ok, mut hit) = (-1i32, 0i32, false, 0i64);
                for &(nid, j, tn) in ctx.judged {
                    for &tj in targets.iter() {
                        if tj == j as i64 {
                            *count = count.wrapping_add(1);
                            if *count as i64 >= *n {
                                ok = true;
                                hit += 1;
                                *count = 0;
                                trig = nid;
                                trig_ms = tn;
                            }
                        } else if *consecutive {
                            *count = 0;
                        }
                    }
                }
                if ok && trig >= 0 {
                    *override_ms = Some(trig_ms);
                }
                Ok((ok, hit))
            }
            Checker::LuckLotResult { target, override_ms } => {
                *override_ms = None;
                let (lots, ms) = ctx.gk.map_or((&[][..], 0), |g| (g.prev_lots, g.prev_lot_ms));
                let hit = lots.iter().filter(|&&r| r == *target).count() as i64;
                if hit > 0 {
                    *override_ms = Some(ms);
                    return Ok((true, hit));
                }
                Ok((false, 0))
            }
            Checker::ComboAtLeast { threshold, override_ms } => {
                *override_ms = None;
                let c = ctx.ctrl(7005)?;
                let idx = c.current_playing_index;
                if idx < 0 || *threshold <= 0 {
                    return Ok((false, 0));
                }
                let rs = &c.states[idx as usize];
                if (rs.combo as i64) < *threshold {
                    return Ok((false, 0));
                }
                if rs.last_combo_ms >= 0 {
                    *override_ms = Some(rs.last_combo_ms);
                }
                Ok((true, 1))
            }
            Checker::GkInterval { kind, n, previous, override_ms } => {
                *override_ms = None;
                let rs = ctx.gk.and_then(|g| {
                    let idx = g.ctrl.current_playing_index;
                    (idx >= 0).then(|| &g.ctrl.states[idx as usize])
                });
                let current = rs.map(|s| match *kind {
                    7001 => s.combo,
                    7002 => s.just,
                    _ => s.raw_just,
                });
                let hits = gk_interval(previous, *n, current);
                if hits > 0 {
                    let t = rs.map_or(-1, |s| if *kind == 7001 { s.last_combo_ms } else { s.last_just_ms });
                    *override_ms = (t >= 0).then_some(t);
                }
                Ok((hits > 0, hits))
            }
            Checker::JustAtLeast { threshold, override_ms } => {
                *override_ms = None;
                let rs = ctx.gk.and_then(|g| {
                    let idx = g.ctrl.current_playing_index;
                    (idx >= 0).then(|| &g.ctrl.states[idx as usize])
                });
                let hit = *threshold > 0 && rs.is_some_and(|s| s.just >= *threshold);
                if hit {
                    *override_ms = rs.and_then(|s| (s.last_just_ms >= 0).then_some(s.last_just_ms));
                }
                Ok((hit, i64::from(hit)))
            }
            Checker::GkJustEdge { raw, threshold, previous, last_range, override_ms } => {
                *override_ms = None;
                let rs = ctx.gk.and_then(|g| {
                    let idx = g.ctrl.current_playing_index;
                    (idx >= 0).then(|| (idx, &g.ctrl.states[idx as usize]))
                });
                let current = rs.map(|(idx, s)| (idx, if *raw { s.raw_just } else { s.just }));
                let hit = gk_just_edge(previous, last_range, *threshold, current);
                if hit {
                    *override_ms = rs.and_then(|(_, s)| (s.last_just_ms >= 0).then_some(s.last_just_ms));
                }
                Ok((hit, i64::from(hit)))
            }
            Checker::GkLiveStart(triggered) => {
                let hit = gk_once(triggered, ctx.gk.is_some());
                Ok((hit, i64::from(hit)))
            }
            Checker::GkReachRank { threshold, triggered } => {
                let eligible = ctx.prev_confirmed_rank.is_some_and(|rank| rank > 0 && rank <= *threshold);
                let hit = gk_once(triggered, eligible);
                Ok((hit, i64::from(hit)))
            }
            Checker::RangeStart { missions, triggered, override_ms } => {
                *override_ms = None;
                let c = ctx.ctrl(7010)?;
                for &idx in &c.state_updates {
                    let s = c.states[idx].state;
                    if s == S_FINISH {
                        triggered.retain(|&x| x != idx);
                    } else if s == S_START {
                        if !mission_ok(missions, c.ranges[idx].mission) {
                            continue;
                        }
                        if !triggered.contains(&idx) {
                            triggered.push(idx);
                            *override_ms = Some(c.ranges[idx].start_ms);
                            return Ok((true, 1));
                        }
                    }
                }
                Ok((false, 0))
            }
            Checker::RangeComplete => {
                let c = ctx.ctrl(7013)?;
                let ok = c.state_updates.iter().any(|&i| c.states[i].state == S_COMPLETE);
                Ok((ok, ok as i64))
            }
            Checker::RangePlaying { missions, active, override_ms } => {
                *override_ms = None;
                let c = ctx.ctrl(7020)?;
                let mut first = -1i32;
                for &idx in &c.state_updates {
                    let s = c.states[idx].state;
                    if s == S_START || s == S_PLAYING {
                        if !active.iter().any(|a| a.0 == idx) {
                            active.push((idx, c.ranges[idx].mission));
                            if first < 0 {
                                first = c.ranges[idx].start_ms;
                            }
                        }
                    } else if s == S_COMPLETE || s == S_FINISH {
                        active.retain(|a| a.0 != idx);
                    }
                }
                if active.is_empty() {
                    return Ok((false, 0));
                }
                if !missions.is_empty() && !missions.contains(&M_ALL) && !active.iter().any(|a| missions.contains(&a.1))
                {
                    return Ok((false, 0));
                }
                if first >= 0 {
                    *override_ms = Some(first);
                }
                Ok((true, 1))
            }
            Checker::LuckRushPlaying(rush) => {
                let c = ctx.ctrl(7021)?;
                let idx = c.current_playing_index;
                if idx >= 0 && c.ranges[idx as usize].mission == M_LUCK {
                    *rush = c.luck_weighted || c.states[idx as usize].luck.rush_combo != 0;
                }
                for &i in &c.state_updates {
                    let s = c.states[i].state;
                    if c.ranges[i].mission == M_LUCK && (s == S_COMPLETE || s == S_FINISH) {
                        *rush = false;
                    }
                }
                Ok((*rush, *rush as i64))
            }
        }
    }
}
