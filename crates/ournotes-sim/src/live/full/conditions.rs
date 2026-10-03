//! Skill condition checkers and the factory that builds them from the condition tables.
//!
//! A checker answers `(hit, hit count)` and may carry an override time (the trigger time of a condition that counts
//! notes is the chart time of the note that completed the count). Condition groups are an OR over their condition
//! sets, each set an AND over its conditions; both stop at the first decisive item.
//!
//! The Gekisou conditions (7005, 7010, 7013, 7020, 7021) read the Gekisou controller and fail when asked in a live
//! without Gekisou; the lottery result condition (7000) never hits there.

use super::Performer;
use super::gekisou::{Controller, M_ALL, M_LUCK, S_COMPLETE, S_FINISH, S_PLAYING, S_START};
use super::life::LifeController;
use crate::error::Error;
use crate::live::random::{LiveRandom, SKILL};
use crate::master::{Master, SkillTargetRow};
use crate::num::{ceil_to_i32, floor_to_i32};

/// The Gekisou state the checkers read in a frame.
#[derive(Clone, Copy)]
pub(crate) struct GkView<'a> {
    pub ctrl: &'a Controller,
    /// The previous frame's lottery results.
    pub prev_lots: &'a [i64],
    /// The previous frame's time when it drew lottery results, else 0.
    pub prev_lot_ms: i32,
}

/// What a checker may read or advance while it is asked.
pub(crate) struct CheckCtx<'a> {
    /// Previous-frame rank confirmation, independent of Gekisou-controller presence.
    pub prev_confirmed_rank: Option<i32>,
    pub life: &'a mut LifeController,
    pub random: &'a mut LiveRandom,
    /// Music time of the frame.
    pub frame_time: i32,
    pub current_combo: i32,
    /// The frame's judged notes: `(note id, converted judgement, chart time)`.
    pub judged: &'a [(i32, i32, i32)],
    /// The frame's fired skill events: `(event index, event time)`.
    pub events: &'a [(i32, i32)],
    /// The Gekisou state (`None` without Gekisou).
    pub gk: Option<GkView<'a>>,
}

impl CheckCtx<'_> {
    fn ctrl(&self, t: i64) -> Result<&Controller, Error> {
        self.gk.map(|g| g.ctrl).ok_or_else(|| {
            Error::Unsupported(format!("condition type {t} reads the Gekisou state of a live without Gekisou"))
        })
    }
}

/// Target type of Gekisou mission targets.
const TARGET_MISSION: i64 = 5;
/// The judgement value of targets without a judgement.
const NO_JUDGEMENT: i64 = -1;

#[derive(Clone, Debug)]
pub(crate) enum Checker {
    /// Nondeterministic input used only by the conservative LUCK replay. Static formation predicates retain
    /// their first answer across resets; life/probability predicates may answer independently at each check.
    Scripted {
        script: super::luck::SharedScript,
        sticky: bool,
        memo: Option<bool>,
        true_hits: i64,
        /// A life comparison `(condition type 2000..=2003, value)`, answered without a branch where the script's
        /// proven life range decides it.
        life: Option<(i64, i64)>,
    },
    LiveComboMultiple {
        threshold: i32,
        previous: i32,
    },
    LiveComboAtLeast(i32),
    ElapsedTime {
        period: i32,
        elapsed: i32,
        previous: i32,
    },
    SnapGekisouStart {
        previous: i32,
    },
    And {
        items: Vec<Checker>,
        resettable: Vec<bool>,
    },
    Or(Vec<Checker>),
    Not(Box<Checker>),
    /// Life at the frame time `>= value` (`None`: the condition has no value).
    LifeAtLeast(Option<i64>),
    /// Life at the frame time `<= value`.
    LifeAtMost(Option<i64>),
    LifeGreater(Option<i64>),
    LifeLess(Option<i64>),
    /// A change since the last check; this checker returns a zero hit count even when it hits.
    LifeChanged {
        previous: i32,
    },
    /// A downward crossing of the threshold captured at initialization.
    LifePercent {
        threshold: i32,
        previous: i32,
    },
    /// Positive changes in one direction, accumulated with a remainder between checks.
    LifeDelta {
        threshold: i32,
        increase: bool,
        count: i32,
        previous: i32,
    },
    /// A chart skill event of this performer fired in the frame.
    SameMemberLiveSkill(i32),
    /// One draw of the skill random stream below the rate.
    Probability(f32),
    /// A fixed answer (member targets are decided when the checker is built; score rank up never fires).
    Fixed(bool),
    /// Every `n` judged notes whose judgement is a target; the trigger time is the chart time of the note that
    /// completed the count (when its id is not negative).
    NoteJudgementMatch {
        kind: i64,
        targets: Vec<i64>,
        override_ms: Option<i32>,
    },
    NoteJudgementCount {
        n: i64,
        consecutive: bool,
        targets: Vec<i64>,
        count: i32,
        override_ms: Option<i32>,
    },
    /// The previous frame drew this lottery result (hit count: how often); trigger time: that frame's time.
    LuckLotResult {
        target: i64,
        override_ms: Option<i32>,
    },
    /// The playing range's Gekisou combo is at least the threshold; trigger time: its last combo judgement.
    ComboAtLeast {
        threshold: i64,
        override_ms: Option<i32>,
    },
    /// 7001/7002/7004: crossings of positive multiples of a controller counter.
    GkInterval {
        kind: i64,
        n: i32,
        previous: i32,
        override_ms: Option<i32>,
    },
    /// 7003: the inflated Just count meets a positive threshold.
    JustAtLeast {
        threshold: i32,
        override_ms: Option<i32>,
    },
    /// 7006/7007: one rising edge, rearmed by falling below or changing range.
    GkJustEdge {
        raw: bool,
        threshold: i32,
        previous: i32,
        last_range: i32,
        override_ms: Option<i32>,
    },
    GkLiveStart(bool),
    GkReachRank {
        threshold: i32,
        triggered: bool,
    },
    /// A range of a target mission (none or 4: any) started this frame, once per range until it finishes.
    RangeStart {
        missions: Vec<i64>,
        triggered: Vec<usize>,
        override_ms: Option<i32>,
    },
    /// A range completed this frame.
    RangeComplete,
    /// A range of a target mission is active (started, not yet complete).
    RangePlaying {
        missions: Vec<i64>,
        active: Vec<(usize, i64)>,
        override_ms: Option<i32>,
    },
    /// The playing luck range is in a rush.
    LuckRushPlaying(bool),
}

fn mission_ok(missions: &[i64], mission: i64) -> bool {
    missions.is_empty() || missions.contains(&M_ALL) || missions.contains(&mission)
}

/// Native interval checkers do not reset on a range gap or an index change.
fn gk_interval(previous: &mut i32, n: i32, current: Option<i32>) -> i64 {
    if n <= 0 {
        return 0;
    }
    let Some(current) = current else { return 0 };
    if current <= 0 {
        *previous = 0;
        return 0;
    }
    let old = *previous;
    *previous = current;
    if current <= old {
        return 0;
    }
    i64::from((current / n).wrapping_sub(old / n).max(0))
}

fn gk_just_edge(previous: &mut i32, last_range: &mut i32, threshold: i32, current: Option<(i32, i32)>) -> bool {
    if threshold <= 0 {
        return false;
    }
    let Some((range, count)) = current else {
        *previous = 0;
        *last_range = -1;
        return false;
    };
    if range != *last_range {
        *previous = 0;
        *last_range = range;
    }
    let hit = *previous < threshold && count >= threshold;
    *previous = count;
    hit
}

fn gk_once(triggered: &mut bool, eligible: bool) -> bool {
    let hit = !*triggered && eligible;
    *triggered |= hit;
    hit
}

impl Checker {
    /// Whether an AND resets this item's count when another item fails.
    fn count_resettable(&self) -> bool {
        matches!(
            self,
            Checker::NoteJudgementCount { .. }
                | Checker::LifeDelta { .. }
                | Checker::ElapsedTime { .. }
                | Checker::Not(_)
        )
    }

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
            Checker::Probability(rate) => Ok((ctx.random.value(SKILL) < *rate, 0)),
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
                    *rush = c.states[idx as usize].luck.rush_combo != 0;
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

    /// The trigger time override of the last check, if any.
    pub(crate) fn override_time(&self) -> Option<i32> {
        match self {
            Checker::NoteJudgementMatch { override_ms, .. }
            | Checker::NoteJudgementCount { override_ms, .. }
            | Checker::LuckLotResult { override_ms, .. }
            | Checker::ComboAtLeast { override_ms, .. }
            | Checker::GkInterval { override_ms, .. }
            | Checker::JustAtLeast { override_ms, .. }
            | Checker::GkJustEdge { override_ms, .. }
            | Checker::RangeStart { override_ms, .. }
            | Checker::RangePlaying { override_ms, .. } => *override_ms,
            Checker::Not(inner) => inner.override_time(),
            _ => None,
        }
    }

    pub(crate) fn reset(&mut self, ctx: &mut CheckCtx) -> Result<(), Error> {
        match self {
            Checker::LiveComboMultiple { previous, .. } => *previous = 0,
            Checker::ElapsedTime { elapsed, previous, .. } => {
                *elapsed = 0;
                *previous = ctx.frame_time;
            }
            Checker::SnapGekisouStart { previous } => *previous = -1,
            Checker::And { items, .. } | Checker::Or(items) => {
                for item in items {
                    item.reset(ctx)?;
                }
            }
            Checker::Not(inner) => inner.reset(ctx)?,
            Checker::LifeDelta { count, previous, .. } => {
                *count = 0;
                *previous = ctx.life.get_life_at_ms(ctx.frame_time)?;
            }
            Checker::NoteJudgementMatch { override_ms, .. } => *override_ms = None,
            Checker::NoteJudgementCount { count, .. } => *count = 0,
            Checker::LuckLotResult { override_ms, .. } | Checker::ComboAtLeast { override_ms, .. } => {
                *override_ms = None;
            }
            Checker::GkInterval { previous, override_ms, .. } => {
                *previous = 0;
                *override_ms = None;
            }
            Checker::JustAtLeast { override_ms, .. } => *override_ms = None,
            Checker::GkJustEdge { previous, last_range, override_ms, .. } => {
                *previous = 0;
                *last_range = -1;
                *override_ms = None;
            }
            Checker::GkLiveStart(triggered) | Checker::GkReachRank { triggered, .. } => *triggered = false,
            Checker::RangeStart { triggered, override_ms, .. } => {
                triggered.clear();
                *override_ms = None;
            }
            Checker::RangePlaying { active, override_ms, .. } => {
                active.clear();
                *override_ms = None;
            }
            Checker::LuckRushPlaying(rush) => *rush = false,
            _ => {}
        }
        Ok(())
    }

    fn reset_count(&mut self, ctx: &mut CheckCtx) -> Result<(), Error> {
        match self {
            Checker::NoteJudgementCount { .. } | Checker::LifeDelta { .. } | Checker::ElapsedTime { .. } => {
                self.reset(ctx)?
            }
            Checker::Not(inner) if inner.count_resettable() => inner.reset_count(ctx)?,
            _ => {}
        }
        Ok(())
    }
}

/// A cumulative condition: the count an effect state carries while it runs (read by Gekisou effects).
#[derive(Clone, Debug)]
pub(crate) enum Cumulative {
    Fixed(i64),
    ElapsedTime {
        period: i32,
        elapsed: i32,
        previous: i32,
        count: i32,
        max: i64,
    },
    /// `floor(Gekisou combo of the playing range / n)`, 0 outside a range (and without Gekisou).
    ComboPerN {
        n: i64,
        values_empty: bool,
        max: i64,
        just: bool,
    },
    /// `floor(judged target notes counted while the effect runs / n)`.
    JudgementPerN {
        n: i64,
        values_empty: bool,
        targets: Vec<i64>,
        max: i64,
        count: i32,
        kind: i64,
    },
    LifePerN {
        init: i64,
        n: i64,
        values_empty: bool,
        max: i64,
        limit: bool,
    },
}

impl Cumulative {
    pub(crate) fn init_count(&self) -> i64 {
        match self {
            Self::LifePerN { init, .. } | Self::Fixed(init) => *init,
            _ => 0,
        }
    }

    pub(crate) fn unit(&self) -> i64 {
        match self {
            Cumulative::ComboPerN { n, .. } | Cumulative::JudgementPerN { n, .. } => *n,
            Cumulative::LifePerN { .. } | Cumulative::Fixed(_) | Cumulative::ElapsedTime { .. } => 0,
        }
    }

    pub(crate) fn max(&self) -> i64 {
        match self {
            Cumulative::ComboPerN { max, .. } | Cumulative::JudgementPerN { max, .. } => *max,
            Cumulative::LifePerN { .. } | Cumulative::Fixed(_) | Cumulative::ElapsedTime { .. } => 0,
        }
    }

    pub(crate) fn update_count(&mut self, ctx: &mut CheckCtx) -> Result<i64, Error> {
        match self {
            Cumulative::Fixed(value) => Ok(*value),
            Cumulative::ElapsedTime { period, elapsed, previous, count, max } => {
                *elapsed = elapsed.wrapping_add(ctx.frame_time.wrapping_sub(*previous));
                *previous = ctx.frame_time;
                if *elapsed >= *period {
                    *elapsed = elapsed.wrapping_sub(*period);
                    *count = count.wrapping_add(1);
                }
                Ok((*count as i64).min(*max))
            }
            Cumulative::ComboPerN { n, values_empty, max, just } => {
                let Some(g) = ctx.gk else { return Ok(0) };
                let idx = g.ctrl.current_playing_index;
                let mut v = 0i64;
                if idx >= 0 {
                    if *values_empty {
                        return Err(no_value());
                    }
                    let state = &g.ctrl.states[idx as usize];
                    let count = if *just { state.just } else { state.combo };
                    v = floor_to_i32(count as f32 / *n as f32) as i64;
                }
                Ok(if v >= *max { *max } else { v })
            }
            Cumulative::LifePerN { n, values_empty, max, limit, .. } => {
                if *values_empty {
                    return Err(no_value());
                }
                let life = if *limit { ctx.life.max_life() } else { ctx.life.get_life_at_ms(ctx.frame_time)? };
                Ok((floor_to_i32(life as f32 / *n as f32) as i64).min(*max))
            }
            Cumulative::JudgementPerN { n, values_empty, targets, max, count, kind } => {
                for &(_, j, _) in ctx.judged {
                    if targets.iter().any(|&target| match *kind {
                        1001 => j as i64 >= target,
                        1002 => j as i64 <= target,
                        _ => j as i64 == target,
                    }) {
                        *count = count.wrapping_add(1);
                    }
                }
                if *values_empty {
                    return Err(no_value());
                }
                Ok((floor_to_i32(*count as f32 / *n as f32) as i64).min(*max))
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Cumulative::JudgementPerN { count, .. } => *count = 0,
            Cumulative::ElapsedTime { elapsed, count, .. } => {
                *elapsed = 0;
                *count = 0;
            }
            _ => {}
        }
    }
}

fn life_percent_threshold(value: i64, initial_life: i32) -> i32 {
    ceil_to_i32((value as f32 / 10f32) * initial_life as f32)
}

fn no_value() -> Error {
    Error::Master("skill condition without a value".into())
}

fn missing_target(id: i64) -> Error {
    Error::Master(format!("unknown skill target {id}"))
}

/// The live member-target predicate deliberately ignores the target's discriminator.
fn target_matches(tg: &SkillTargetRow, p: &Performer) -> bool {
    p.matches_skill_target(tg)
}

/// Builds checkers for the performer at `member` of `deck`.
pub(crate) struct Factory<'a> {
    pub master: &'a Master,
    pub deck: &'a [Performer],
    pub initial_life: i32,
    pub initial_time_ms: i32,
    pub skill_target_music_type: i64,
}

impl Factory<'_> {
    fn one(&self, cid: i64, k: usize) -> Result<Option<Checker>, Error> {
        let m = self.master;
        let c = m.skill_condition(cid).ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
        let v0 = c.condition_values.first().copied();
        let targets: Vec<Option<&SkillTargetRow>> = c.condition_target_ids.iter().map(|&i| m.skill_target(i)).collect();
        let target = |i: usize| targets[i].ok_or_else(|| missing_target(c.condition_target_ids[i]));
        let missions = || -> Result<Vec<i64>, Error> {
            let mut out = Vec::new();
            for i in 0..targets.len() {
                let t = target(i)?;
                if t.skill_target_type == TARGET_MISSION && t.gekisou_mission_type != 0 {
                    out.push(t.gekisou_mission_type);
                }
            }
            Ok(out)
        };
        let ch = match c.condition_type {
            0 => return Ok(None),
            2000 => Checker::LifeGreater(v0),
            2001 => Checker::LifeAtLeast(v0),
            2002 => Checker::LifeLess(v0),
            2003 => Checker::LifeAtMost(v0),
            2004 => Checker::LifeChanged { previous: self.initial_life },
            4007 => Checker::LifePercent {
                threshold: life_percent_threshold(v0.ok_or_else(no_value)?, self.initial_life),
                previous: 0,
            },
            4008 | 4009 => Checker::LifeDelta {
                threshold: v0.ok_or_else(no_value)? as i32,
                increase: c.condition_type == 4008,
                count: 0,
                previous: self.initial_life,
            },
            3000 | 3001 => {
                let mut count = 0;
                for member in self.deck {
                    for i in 0..targets.len() {
                        if target_matches(target(i)?, member) {
                            count += 1;
                            break;
                        }
                    }
                }
                Checker::Fixed(if c.condition_type == 3000 { count > 0 } else { count == self.deck.len() })
            }
            4001 => Checker::LiveComboMultiple { threshold: v0.ok_or_else(no_value)? as i32, previous: 0 },
            4002 => Checker::LiveComboAtLeast(v0.ok_or_else(no_value)? as i32),
            4000 => Checker::ElapsedTime {
                period: (v0.ok_or_else(no_value)? as i32).wrapping_mul(1000),
                elapsed: 0,
                previous: self.initial_time_ms,
            },
            4010 | 5020 => Checker::SameMemberLiveSkill(k as i32),
            5021 => Checker::SnapGekisouStart { previous: -1 },
            4012 => {
                let mut hit = false;
                for i in 0..targets.len() {
                    if target(i)?.live_music_type == self.skill_target_music_type {
                        hit = true;
                        break;
                    }
                }
                Checker::Fixed(hit)
            }
            4011 => Checker::Probability(v0.ok_or_else(no_value)? as f32 / 100f32),
            5000 => {
                let mut fixed = false;
                if let Some(p) = self.deck.get(k) {
                    for i in 0..targets.len() {
                        if target_matches(target(i)?, p) {
                            fixed = true;
                            break;
                        }
                    }
                }
                Checker::Fixed(fixed)
            }
            8000 => Checker::Fixed(false),
            1000 | 1010 | 1020 | 1030 | 1040 => {
                let mut judgements = Vec::with_capacity(targets.len());
                for i in 0..targets.len() {
                    let j = target(i)?.judgement;
                    if j != NO_JUDGEMENT {
                        judgements.push(j);
                    }
                }
                if matches!(c.condition_type, 1030 | 1040) {
                    Checker::NoteJudgementCount {
                        n: v0.ok_or_else(no_value)?,
                        consecutive: c.condition_type == 1040,
                        targets: judgements,
                        count: 0,
                        override_ms: None,
                    }
                } else {
                    Checker::NoteJudgementMatch { kind: c.condition_type, targets: judgements, override_ms: None }
                }
            }
            7000 => Checker::LuckLotResult { target: v0.ok_or_else(no_value)?, override_ms: None },
            7001 | 7002 | 7004 => Checker::GkInterval {
                kind: c.condition_type,
                n: v0.ok_or_else(no_value)? as i32,
                previous: 0,
                override_ms: None,
            },
            7003 => Checker::JustAtLeast { threshold: v0.ok_or_else(no_value)? as i32, override_ms: None },
            7006 | 7007 => Checker::GkJustEdge {
                raw: c.condition_type == 7007,
                threshold: v0.ok_or_else(no_value)? as i32,
                previous: 0,
                last_range: -1,
                override_ms: None,
            },
            7011 => Checker::GkLiveStart(false),
            7012 => Checker::GkReachRank { threshold: v0.unwrap_or(1) as i32, triggered: false },
            7005 => Checker::ComboAtLeast { threshold: v0.ok_or_else(no_value)?, override_ms: None },
            7010 => Checker::RangeStart { missions: missions()?, triggered: Vec::new(), override_ms: None },
            7013 => Checker::RangeComplete,
            7020 => Checker::RangePlaying { missions: missions()?, active: Vec::new(), override_ms: None },
            7021 => Checker::LuckRushPlaying(false),
            t => return Err(Error::Unsupported(format!("skill condition type {t}"))),
        };
        Ok(Some(if c.is_positive { ch } else { Checker::Not(Box::new(ch)) }))
    }

    /// The checker of a condition group (`None` for group 0 or a group without a non-empty set).
    pub(crate) fn group(&self, gid: i64, k: usize) -> Result<Option<Checker>, Error> {
        self.group_from(gid, |cid| self.one(cid, k))
    }

    pub(crate) fn luck_group(
        &self,
        gid: i64,
        k: usize,
        script: &super::luck::SharedScript,
    ) -> Result<Option<Checker>, Error> {
        self.group_from(gid, |cid| {
            let row = self
                .master
                .skill_condition(cid)
                .ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
            match row.condition_type {
                2000..=2004 | 4011 | 5000 | 3000 | 3001 => {
                    let checker = Checker::Scripted {
                        script: script.clone(),
                        sticky: matches!(row.condition_type, 5000 | 3000 | 3001),
                        memo: None,
                        true_hits: if matches!(row.condition_type, 2004 | 4011) { 0 } else { 1 },
                        life: (2000..=2003)
                            .contains(&row.condition_type)
                            .then(|| row.condition_values.first().map(|&v| (row.condition_type, v)))
                            .flatten(),
                    };
                    Ok(Some(if row.is_positive { checker } else { Checker::Not(Box::new(checker)) }))
                }
                0 | 7000 | 7010 | 7013 | 7020 | 7021 => self.one(cid, k),
                ty => Err(Error::Unsupported(format!("LUCK replay condition {ty}"))),
            }
        })
    }

    fn group_from(
        &self,
        gid: i64,
        mut one: impl FnMut(i64) -> Result<Option<Checker>, Error>,
    ) -> Result<Option<Checker>, Error> {
        if gid == 0 {
            return Ok(None);
        }
        let mut ors = Vec::new();
        for s in self.master.skill_condition_sets.iter().filter(|s| s.group == gid) {
            let mut items = Vec::new();
            for &cid in &s.condition_ids {
                if let Some(c) = one(cid)? {
                    items.push(c);
                }
            }
            match items.len() {
                0 => {}
                1 => ors.push(items.pop().expect("one item")),
                _ => {
                    let resettable = items.iter().map(Checker::count_resettable).collect();
                    ors.push(Checker::And { items, resettable });
                }
            }
        }
        Ok(match ors.len() {
            0 => None,
            1 => ors.pop(),
            _ => Some(Checker::Or(ors)),
        })
    }

    /// The cumulative condition of an effect (id 0: none).
    pub(crate) fn cumulative(&self, cid: i64, k: usize) -> Result<Option<Cumulative>, Error> {
        if cid == 0 {
            return Ok(None);
        }
        let m = self.master;
        let c =
            m.cumulative_condition(cid).ok_or_else(|| Error::Master(format!("unknown cumulative condition {cid}")))?;
        let max = if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count };
        let n = c.condition_values.first().copied().unwrap_or(0);
        let values_empty = c.condition_values.is_empty();
        match c.condition_type {
            7000 | 7001 => Ok(Some(Cumulative::ComboPerN { n, values_empty, max, just: c.condition_type == 7000 })),
            2000 | 2001 => {
                if c.condition_type == 2000 && values_empty {
                    return Err(no_value());
                }
                let init =
                    if c.condition_type == 2000 { floor_to_i32(self.initial_life as f32 / n as f32) as i64 } else { 0 };
                Ok(Some(Cumulative::LifePerN { init, n, values_empty, max, limit: c.condition_type == 2001 }))
            }
            1000..=1002 => {
                let mut targets = Vec::with_capacity(c.condition_target_ids.len());
                for &i in &c.condition_target_ids {
                    let j = m.skill_target(i).ok_or_else(|| missing_target(i))?.judgement;
                    if j != NO_JUDGEMENT {
                        targets.push(j);
                    }
                }
                Ok(Some(Cumulative::JudgementPerN { n, values_empty, targets, max, count: 0, kind: c.condition_type }))
            }
            3000..=3005 => {
                let count = match c.condition_type {
                    3000 | 3001 => {
                        let targets = c
                            .condition_target_ids
                            .iter()
                            .map(|&id| m.skill_target(id).ok_or_else(|| missing_target(id)))
                            .collect::<Result<Vec<_>, _>>()?;
                        self.deck
                            .iter()
                            .enumerate()
                            .filter(|(index, member)| {
                                (c.condition_type != 3001 || *index != k)
                                    && targets.iter().any(|target| member.matches_skill_target(target))
                            })
                            .count()
                    }
                    3002 | 3003 => {
                        let band = self
                            .deck
                            .get(k)
                            .ok_or_else(|| Error::Game("cumulative self member index out of range".into()))?
                            .band_id;
                        if c.condition_type == 3002 {
                            self.deck.iter().filter(|p| p.band_id == band).count()
                        } else {
                            self.deck
                                .iter()
                                .map(|p| p.band_id)
                                .filter(|&b| b != band)
                                .collect::<std::collections::HashSet<_>>()
                                .len()
                        }
                    }
                    3004 => self.deck.iter().map(|p| p.band_id).collect::<std::collections::HashSet<_>>().len(),
                    _ => self.deck.iter().map(|p| p.card_type).collect::<std::collections::HashSet<_>>().len(),
                };
                Ok(Some(Cumulative::Fixed((count as i64).min(max))))
            }
            6000 => {
                let n = c.condition_values.first().copied().ok_or_else(no_value)? as i32;
                Ok(Some(Cumulative::ElapsedTime {
                    period: n.wrapping_mul(1000),
                    elapsed: 0,
                    previous: self.initial_time_ms,
                    count: 0,
                    max,
                }))
            }
            // The client's checker conversion throws for any other value, 0 included.
            t => Err(Error::Game(format!("ArgumentOutOfRangeException: cumulative condition type {t}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_life<T>(life: i32, f: impl FnOnce(&mut CheckCtx) -> T) -> T {
        let mut life = LifeController::new(life, Default::default(), 1000).unwrap();
        let mut random = LiveRandom::new(0);
        f(&mut CheckCtx {
            life: &mut life,
            random: &mut random,
            frame_time: 0,
            current_combo: 0,
            judged: &[],
            events: &[],
            gk: None,
            prev_confirmed_rank: None,
        })
    }

    #[test]
    fn ordinary_combo_crossings_are_not_count_resettable() {
        with_life(1000, |ctx| {
            let mut c = Checker::LiveComboMultiple { threshold: 10, previous: 25 };
            assert!(!c.count_resettable());
            for (combo, hits) in [(29, 0), (35, 1), (35, 0), (7, 0), (25, 2)] {
                ctx.current_combo = combo;
                assert_eq!(c.check(ctx).unwrap(), (hits > 0, hits));
            }
            c.reset(ctx).unwrap();
            assert_eq!(c.check(ctx).unwrap(), (true, 2));
            let mut disabled = Checker::LiveComboAtLeast(0);
            assert_eq!(disabled.check(ctx).unwrap(), (false, 0));
        });
    }

    #[test]
    fn factory_team_music_and_snap_targets_preserve_native_quantifiers() {
        use serde_json::json;
        let mut tables = json!({
            "MasterSkillTarget": [{"_id":1,"_skillTargetType":1,"_bandID":1,"_liveMusicType":99},
                                  {"_id":2,"_skillTargetType":4,"_bandID":2,"_liveMusicType":1}],
            "MasterSkillCondition": [],
        });
        for (id, kind, targets) in [
            (1, 3000, vec![1]),
            (2, 3001, vec![1, 2]),
            (3, 3001, vec![1]),
            (4, 4012, vec![1]),
            (5, 4012, vec![2]),
            (6, 5020, vec![]),
        ] {
            tables["MasterSkillCondition"].as_array_mut().unwrap().push(json!({"_id":id,
                "_conditionType":kind,"_conditionValues":[],"_conditionTargetIDs":targets,"_isPositive":true}));
        }
        let text: Vec<(String, String)> =
            tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
        let master =
            Master::from_json_tables(|name| text.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())).unwrap();
        let deck = [Performer { band_id: 1, ..Default::default() }, Performer { band_id: 2, ..Default::default() }];
        let f = Factory {
            master: &master,
            deck: &deck,
            initial_life: 1000,
            initial_time_ms: 0,
            skill_target_music_type: 1,
        };
        with_life(1000, |ctx| {
            for (id, want) in [(1, true), (2, true), (3, false), (4, false), (5, true)] {
                assert_eq!(f.one(id, 0).unwrap().unwrap().check(ctx).unwrap(), (want, i64::from(want)));
            }
            ctx.events = &[(0, 10), (0, 20)];
            assert_eq!(f.one(6, 0).unwrap().unwrap().check(ctx).unwrap(), (true, 1));
            assert_eq!(f.one(6, 1).unwrap().unwrap().check(ctx).unwrap(), (false, 0));
            let empty = Factory { deck: &[], ..f };
            assert_eq!(empty.one(2, 0).unwrap().unwrap().check(ctx).unwrap(), (true, 1));
        });
    }

    #[test]
    fn failed_not_preserves_other_and_counters_and_or_hit_count() {
        with_life(1000, |ctx| {
            let mut c = Checker::And {
                items: vec![
                    Checker::Not(Box::new(Checker::Fixed(true))),
                    Checker::ElapsedTime { period: 1000, elapsed: 700, previous: 0 },
                ],
                resettable: vec![true, true],
            };
            assert_eq!(c.check(ctx).unwrap(), (false, 0));
            if let Checker::And { items, .. } = c {
                assert!(matches!(items[1], Checker::ElapsedTime { elapsed: 700, .. }));
            }
            let mut c = Checker::Or(vec![Checker::Not(Box::new(Checker::Fixed(true))), Checker::Probability(2.0)]);
            assert_eq!(c.check(ctx).unwrap(), (true, 1));
            assert_eq!(c.override_time(), None);
        });
    }

    #[test]
    fn elapsed_time_catches_up_one_period_and_reset_rebases() {
        with_life(1000, |ctx| {
            let mut c = Checker::ElapsedTime { period: 1000, elapsed: 0, previous: 100 };
            ctx.frame_time = 3600;
            assert_eq!(c.check(ctx).unwrap(), (true, 1));
            assert_eq!(c.check(ctx).unwrap(), (true, 1));
            assert_eq!(c.check(ctx).unwrap(), (true, 1));
            assert_eq!(c.check(ctx).unwrap(), (false, 0));
            ctx.frame_time = 4000;
            c.reset_count(ctx).unwrap();
            ctx.frame_time = 4500;
            assert_eq!(c.check(ctx).unwrap(), (false, 0));
            ctx.frame_time = 5000;
            assert_eq!(c.check(ctx).unwrap(), (true, 1));
        });
    }

    #[test]
    fn member_targets_are_ordered_alternatives_not_first_nonzero() {
        let p = Performer {
            band_id: 1,
            character_id: 2,
            card_type: 3,
            tag_ids: vec![7],
            live_skill_categories: vec![0, 10],
            gekisou_skill_categories: vec![20],
            gekisou_mission_type: 2,
            ..Default::default()
        };
        let t = SkillTargetRow { skill_target_type: 4, character_id: 999, band_id: 1, ..Default::default() };
        assert!(target_matches(&t, &p));
        for t in [
            SkillTargetRow { band_id: 999, card_type: 3, ..Default::default() },
            SkillTargetRow { band_id: 999, character_id: 2, ..Default::default() },
            SkillTargetRow { tag_id: 7, ..Default::default() },
            SkillTargetRow { tag_id: 999, live_skill_categories: vec![10], ..Default::default() },
            SkillTargetRow { tag_id: 999, gekisou_skill_categories: vec![20], ..Default::default() },
            SkillTargetRow { gekisou_mission_type: 2, ..Default::default() },
        ] {
            assert!(target_matches(&t, &p));
        }
        assert!(!target_matches(&SkillTargetRow::default(), &p));
        assert!(!target_matches(&SkillTargetRow { live_skill_categories: vec![0], ..Default::default() }, &p));
        assert!(!target_matches(&SkillTargetRow { gekisou_mission_type: 4, ..Default::default() }, &p));
        assert!(!target_matches(
            &SkillTargetRow { band_id: -1, ..Default::default() },
            &Performer { band_id: -1, ..Default::default() }
        ));
    }

    #[test]
    fn judgement_matches_count_notes_not_duplicate_targets() {
        with_life(1000, |ctx| {
            ctx.judged = &[(1, 5, 100), (2, 4, 200), (3, 5, 300)];
            for (kind, expected) in [(1000, 2), (1010, 2), (1020, 3)] {
                let mut c = Checker::NoteJudgementMatch { kind, targets: vec![5, 5], override_ms: None };
                assert_eq!(c.check(ctx).unwrap(), (true, expected));
                assert_eq!(c.override_time(), Some(300));
            }
        });
    }

    #[test]
    fn judgement_comparisons_preserve_special_value_asymmetry() {
        with_life(1000, |ctx| {
            ctx.judged = &[(1, 7, 100)];
            let mut less = Checker::NoteJudgementMatch { kind: 1020, targets: vec![5], override_ms: None };
            assert_eq!(less.check(ctx).unwrap(), (false, 0));
            let mut greater = Checker::NoteJudgementMatch { kind: 1010, targets: vec![5], override_ms: None };
            assert!(greater.check(ctx).is_err());
            ctx.judged = &[(1, 5, 100), (-1, 5, 200)];
            assert_eq!(greater.check(ctx).unwrap(), (true, 2));
            assert_eq!(greater.override_time(), None);
        });
    }

    #[test]
    fn counted_judgement_reset_only_clears_the_count() {
        with_life(1000, |ctx| {
            let mut c = Checker::NoteJudgementCount {
                n: 2,
                consecutive: true,
                targets: vec![5],
                count: 1,
                override_ms: Some(100),
            };
            c.reset(ctx).unwrap();
            assert_eq!(c.override_time(), Some(100));
            ctx.judged = &[(2, 5, 200)];
            assert_eq!(c.check(ctx).unwrap(), (false, 0));
            assert_eq!(c.override_time(), None);
        });
    }

    #[test]
    fn consecutive_judgements_reset_for_each_nonmatching_target() {
        with_life(1000, |ctx| {
            ctx.judged = &[(1, 5, 100), (2, 5, 200)];
            for (targets, expected) in [(vec![5], 1), (vec![4, 5], 0), (vec![5, 5], 2)] {
                let mut c =
                    Checker::NoteJudgementCount { n: 2, consecutive: true, targets, count: 0, override_ms: None };
                assert_eq!(c.check(ctx).unwrap(), (expected > 0, expected));
            }
        });
    }

    #[test]
    fn strict_life_comparisons_exclude_the_boundary() {
        let mut greater = Checker::LifeGreater(Some(1000));
        let mut less = Checker::LifeLess(Some(1000));
        assert_eq!(with_life(1000, |c| greater.check(c)).unwrap(), (false, 0));
        assert_eq!(with_life(1001, |c| greater.check(c)).unwrap(), (true, 1));
        assert_eq!(with_life(1000, |c| less.check(c)).unwrap(), (false, 0));
        assert_eq!(with_life(999, |c| less.check(c)).unwrap(), (true, 1));
    }

    #[test]
    fn life_change_reports_zero_hit_count_and_keeps_history_on_reset() {
        let mut changed = Checker::LifeChanged { previous: 1000 };
        assert_eq!(with_life(900, |c| changed.check(c)).unwrap(), (true, 0));
        with_life(800, |c| changed.reset(c)).unwrap();
        assert_eq!(with_life(900, |c| changed.check(c)).unwrap(), (false, 0));
    }

    #[test]
    fn life_percentage_only_reports_downward_crossings() {
        assert_eq!(life_percent_threshold(3, 999), 300);
        assert_eq!(life_percent_threshold(3, 1), 1);
        assert_eq!(life_percent_threshold(-3, 999), -299);
        let mut c = Checker::LifePercent { threshold: 300, previous: 0 };
        for (life, want) in [(200, false), (1000, false), (300, true), (200, false), (301, false), (299, true)] {
            assert_eq!(with_life(life, |ctx| c.check(ctx)).unwrap(), (want, i64::from(want)));
        }
    }

    #[test]
    fn life_delta_keeps_remainders_and_resets_to_current_life() {
        let mut c = Checker::LifeDelta { threshold: 100, increase: false, count: 0, previous: 1000 };
        for (life, hits) in [(750, 2), (800, 0), (650, 2)] {
            assert_eq!(with_life(life, |ctx| c.check(ctx)).unwrap(), (hits > 0, hits));
        }
        with_life(500, |ctx| c.reset(ctx)).unwrap();
        assert_eq!(with_life(450, |ctx| c.check(ctx)).unwrap(), (false, 0));
        assert_eq!(with_life(400, |ctx| c.check(ctx)).unwrap(), (true, 1));
    }

    #[test]
    fn failed_and_resets_life_delta_to_the_current_life() {
        let mut c = Checker::And {
            items: vec![
                Checker::LifeDelta { threshold: 100, increase: true, count: 50, previous: 1000 },
                Checker::Fixed(false),
            ],
            resettable: vec![true, false],
        };
        // The counting check fails first, so the remainder survives.
        assert_eq!(with_life(1040, |ctx| c.check(ctx)).unwrap(), (false, 0));
        // It now hits, but the next non-counting condition fails and resets both count and baseline.
        assert_eq!(with_life(1100, |ctx| c.check(ctx)).unwrap(), (false, 0));
        if let Checker::And { items, .. } = &mut c {
            assert_eq!(with_life(1150, |ctx| items[0].check(ctx)).unwrap(), (false, 0));
            assert_eq!(with_life(1200, |ctx| items[0].check(ctx)).unwrap(), (true, 1));
        }
    }
    fn cumulative_master(kind: i64, values: &[i64], max: i64) -> Master {
        let rows = serde_json::json!({"_allData": [{"_id": 1, "_skillCumulativeConditionType": kind,
            "_conditionValues": values, "_conditionTargetIDs": [1, 1], "_maxCumulativeCount": max}]})
        .to_string();
        let targets = serde_json::json!({"_allData": [{"_id": 1, "_characterID": 7,
            "_judgement": 5}]})
        .to_string();
        Master::from_json_tables(|name| match name {
            "MasterSkillCumulativeCondition" => Some(rows.as_str()),
            "MasterSkillTarget" => Some(targets.as_str()),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn cumulative_factory_formation_counts_slots_and_never_divides_by_n() {
        let deck: Vec<_> = [(1, 1), (1, 1), (2, 2), (2, 0), (3, 99)]
            .into_iter()
            .map(|(band_id, card_type)| Performer { band_id, card_type, character_id: 7, ..Default::default() })
            .collect();
        for (kind, expected) in [(3000, 5), (3001, 4), (3002, 2), (3003, 2), (3004, 3), (3005, 4)] {
            for values in [vec![], vec![99], vec![0], vec![-1]] {
                for cap in [-1, 0, 1, 3, 99] {
                    let master = cumulative_master(kind, &values, cap);
                    let f = Factory {
                        master: &master,
                        deck: &deck,
                        initial_life: 1000,
                        initial_time_ms: 0,
                        skill_target_music_type: 0,
                    };
                    let mut c = f.cumulative(1, 1).unwrap().unwrap();
                    let want = if cap > 0 { expected.min(cap) } else { expected };
                    assert_eq!((c.init_count(), c.unit(), c.max()), (want, 0, 0));
                    assert_eq!(with_life(1000, |ctx| c.update_count(ctx)).unwrap(), want);
                    c.reset();
                    assert_eq!(with_life(250, |ctx| c.update_count(ctx)).unwrap(), want);
                }
            }
        }
        for kind in 3000..=3005 {
            let master = cumulative_master(kind, &[], 0);
            let f = Factory {
                master: &master,
                deck: &deck,
                initial_life: 1000,
                initial_time_ms: 0,
                skill_target_music_type: 0,
            };
            assert_eq!(f.cumulative(1, 99).is_err(), kind == 3002 || kind == 3003);
            let f = Factory {
                master: &master,
                deck: &[],
                initial_life: 1000,
                initial_time_ms: 0,
                skill_target_music_type: 0,
            };
            assert_eq!(f.cumulative(1, 0).is_err(), kind == 3002 || kind == 3003);
        }
    }

    #[test]
    fn cumulative_factory_elapsed_backlog_reset_and_wrapping() {
        let master = cumulative_master(6000, &[1], 2);
        let f = Factory {
            master: &master,
            deck: &[],
            initial_life: 1000,
            initial_time_ms: 100,
            skill_target_music_type: 0,
        };
        let mut c = f.cumulative(1, 99).unwrap().unwrap();
        assert_eq!((c.init_count(), c.unit(), c.max()), (0, 0, 0));
        with_life(1000, |ctx| {
            for (now, want) in [(3600, 1), (3600, 2), (3600, 2)] {
                ctx.frame_time = now;
                assert_eq!(c.update_count(ctx).unwrap(), want);
            }
            c.reset();
            ctx.frame_time = 4000;
            assert_eq!(c.update_count(ctx).unwrap(), 0);
            ctx.frame_time = 4600;
            assert_eq!(c.update_count(ctx).unwrap(), 1);
            ctx.frame_time = 3500;
            assert_eq!(c.update_count(ctx).unwrap(), 1);
        });
        for n in [0, -1, i32::MAX as i64] {
            let master = cumulative_master(6000, &[n], 0);
            let f = Factory {
                master: &master,
                deck: &[],
                initial_life: 0,
                initial_time_ms: i32::MAX,
                skill_target_music_type: 0,
            };
            let mut c = f.cumulative(1, 0).unwrap().unwrap();
            with_life(0, |ctx| {
                ctx.frame_time = i32::MIN;
                assert_eq!(c.update_count(ctx).unwrap(), 1);
            });
        }
    }

    #[test]
    fn cumulative_factory_life_initial_cap_and_signed_judgements() {
        for kind in [2000, 2001] {
            let master = cumulative_master(kind, &[100], 3);
            let f = Factory {
                master: &master,
                deck: &[],
                initial_life: 1000,
                initial_time_ms: 0,
                skill_target_music_type: 0,
            };
            let mut c = f.cumulative(1, 0).unwrap().unwrap();
            assert_eq!(c.init_count(), if kind == 2000 { 10 } else { 0 });
            assert_eq!((c.unit(), c.max()), (0, 0));
            assert_eq!(with_life(1000, |ctx| c.update_count(ctx)).unwrap(), 3);
            c.reset();
            assert_eq!(c.init_count(), if kind == 2000 { 10 } else { 0 });
        }
        for (kind, want) in [(1000, 1), (1001, 3), (1002, 3)] {
            let master = cumulative_master(kind, &[1], 0);
            let f = Factory {
                master: &master,
                deck: &[],
                initial_life: 1000,
                initial_time_ms: 0,
                skill_target_music_type: 0,
            };
            let mut c = f.cumulative(1, 0).unwrap().unwrap();
            with_life(1000, |ctx| {
                ctx.judged = &[(1, -1, 0), (2, 0, 0), (3, 5, 0), (4, 6, 0), (5, 7, 0)];
                assert_eq!(c.update_count(ctx).unwrap(), want);
                assert_eq!(c.update_count(ctx).unwrap(), want * 2);
                c.reset();
                assert_eq!(c.update_count(ctx).unwrap(), want);
            });
        }
        let master = cumulative_master(0, &[], 0);
        let f = Factory { master: &master, deck: &[], initial_life: 0, initial_time_ms: 0, skill_target_music_type: 0 };
        assert!(f.cumulative(0, 0).unwrap().is_none());
        assert!(f.cumulative(1, 0).is_err());
    }
}

#[cfg(test)]
mod gekisou_condition_tests {
    use super::*;

    #[test]
    fn interval_counts_all_crossings_without_replaying_same_count() {
        let mut old = 0;
        for (value, expected) in [(Some(2), 0), (Some(11), 3), (Some(11), 0), (Some(14), 1)] {
            assert_eq!(gk_interval(&mut old, 3, value), expected);
        }
    }

    #[test]
    fn interval_preserves_gap_but_rebases_decrease_and_nonpositive_count() {
        let mut old = 12;
        assert_eq!(gk_interval(&mut old, 3, None), 0);
        assert_eq!(old, 12);
        assert_eq!(gk_interval(&mut old, 3, Some(5)), 0);
        assert_eq!(gk_interval(&mut old, 3, Some(6)), 1);
        assert_eq!(gk_interval(&mut old, 3, Some(-2)), 0);
        assert_eq!(old, 0);
        assert_eq!(gk_interval(&mut old, 0, Some(9)), 0);
        assert_eq!(old, 0);
    }

    #[test]
    fn edge_rearms_after_falling_or_switching_range_and_on_gap() {
        let (mut old, mut range) = (0, -1);
        for (input, expected) in [
            (Some((0, 5)), true),
            (Some((0, 8)), false),
            (Some((0, 4)), false),
            (Some((0, 5)), true),
            (Some((1, 5)), true),
            (None, false),
            (Some((1, 5)), true),
        ] {
            assert_eq!(gk_just_edge(&mut old, &mut range, 5, input), expected);
        }
        assert!(!gk_just_edge(&mut old, &mut range, 0, None));
        assert_eq!((old, range), (5, 1));
    }

    #[test]
    fn once_latches_only_eligible_events_and_can_reset() {
        let mut triggered = false;
        assert!(!gk_once(&mut triggered, false));
        assert!(gk_once(&mut triggered, true));
        assert!(!gk_once(&mut triggered, true));
        triggered = false;
        assert!(gk_once(&mut triggered, true));
    }
}

#[cfg(test)]
mod gekisou_factory_tests {
    use super::*;

    fn master(kind: i64, values: &[i32]) -> Master {
        let rows = serde_json::json!({"_allData": [{"_id": 1, "_conditionType": kind,
            "_conditionValues": values, "_conditionTargetIDs": [], "_isPositive": true}]})
        .to_string();
        Master::from_json_tables(|name| (name == "MasterSkillCondition").then_some(rows.as_str())).unwrap()
    }

    fn ask(checker: &mut Checker, ctrl: Option<&Controller>, rank: Option<i32>) -> ((bool, i64), Option<i32>) {
        let mut life = LifeController::new(1000, Default::default(), 1000).unwrap();
        let mut random = LiveRandom::new(0);
        let mut ctx = CheckCtx {
            life: &mut life,
            random: &mut random,
            frame_time: 999,
            current_combo: 0,
            judged: &[],
            events: &[],
            prev_confirmed_rank: rank,
            gk: ctrl.map(|ctrl| GkView { ctrl, prev_lots: &[], prev_lot_ms: 0 }),
        };
        (checker.check(&mut ctx).unwrap(), checker.override_time())
    }

    #[test]
    fn factory_intervals_read_distinct_controller_counters_and_last_times() {
        for (kind, expected, time) in [(7001, 4, 120), (7002, 3, 110), (7004, 1, 110)] {
            let m = master(kind, &[3]);
            let mut c =
                Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                    .one(1, 0)
                    .unwrap()
                    .unwrap();
            let mut ctrl = Controller::new(vec![(0, 200, 1)], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
            ctrl.current_playing_index = 0;
            ctrl.states[0].combo = 12;
            ctrl.states[0].just = 9;
            ctrl.states[0].raw_just = 3;
            ctrl.states[0].last_combo_ms = 120;
            ctrl.states[0].last_just_ms = 110;
            assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, expected), Some(time)));
            assert_eq!(ask(&mut c, Some(&ctrl), None), ((false, 0), None));
            assert_eq!(ask(&mut c, None, None), ((false, 0), None));
        }
    }

    #[test]
    fn factory_edges_distinguish_raw_and_inflated_and_rearm_on_gap() {
        for (kind, first) in [(7006, true), (7007, false)] {
            let m = master(kind, &[5]);
            let mut c =
                Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                    .one(1, 0)
                    .unwrap()
                    .unwrap();
            let mut ctrl = Controller::new(vec![(0, 200, 1)], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
            ctrl.current_playing_index = 0;
            ctrl.states[0].just = 10;
            ctrl.states[0].raw_just = 4;
            ctrl.states[0].last_just_ms = 123;
            assert_eq!(ask(&mut c, Some(&ctrl), None).0, (first, i64::from(first)));
            ctrl.states[0].raw_just = 5;
            assert_eq!(ask(&mut c, Some(&ctrl), None).0, (!first, i64::from(!first)));
            assert_eq!(ask(&mut c, None, None).0, (false, 0));
            assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, 1), Some(123)));
        }
    }

    #[test]
    fn factory_start_and_rank_use_presence_and_actual_confirmed_rank() {
        let m = master(7011, &[]);
        let ctrl = Controller::new(vec![], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
        let mut start =
            Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                .one(1, 0)
                .unwrap()
                .unwrap();
        assert_eq!(ask(&mut start, None, None).0, (false, 0));
        assert_eq!(ask(&mut start, Some(&ctrl), None).0, (true, 1));
        assert_eq!(ask(&mut start, Some(&ctrl), None).0, (false, 0));
        let m = master(7012, &[]); // native empty list defaults to first place
        let mut rank =
            Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                .one(1, 0)
                .unwrap()
                .unwrap();
        for r in [None, Some(0), Some(-1), Some(2)] {
            assert_eq!(ask(&mut rank, Some(&ctrl), r).0, (false, 0));
        }
        assert_eq!(ask(&mut rank, None, Some(1)).0, (true, 1));
        assert_eq!(ask(&mut rank, Some(&ctrl), Some(1)).0, (false, 0));
    }

    #[test]
    fn factory_requires_interval_value_and_just_level_is_not_an_edge() {
        let m = master(7001, &[]);
        assert!(
            Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                .one(1, 0)
                .is_err()
        );
        let m = master(7003, &[5]);
        let mut c =
            Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                .one(1, 0)
                .unwrap()
                .unwrap();
        let mut ctrl = Controller::new(vec![(0, 200, 1)], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
        ctrl.current_playing_index = 0;
        ctrl.states[0].just = 5;
        assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, 1), None));
        assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, 1), None));
    }
}
