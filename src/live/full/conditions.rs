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
use crate::num::floor_to_i32;

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
    pub life: &'a mut LifeController,
    pub random: &'a mut LiveRandom,
    /// Music time of the frame.
    pub frame_time: i32,
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

/// Target type of member attribute targets (band, character, card type).
const TARGET_MEMBER: i64 = 3;
/// Target type of Gekisou mission targets.
const TARGET_MISSION: i64 = 5;
/// The judgement value of targets without a judgement.
const NO_JUDGEMENT: i64 = -1;

#[derive(Clone, Debug)]
pub(crate) enum Checker {
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
    /// A chart skill event of this performer fired in the frame.
    SameMemberLiveSkill(i32),
    /// One draw of the skill random stream below the rate.
    Probability(f32),
    /// A fixed answer (member targets are decided when the checker is built; score rank up never fires).
    Fixed(bool),
    /// Every `n` judged notes whose judgement is a target; the trigger time is the chart time of the note that
    /// completed the count (when its id is not negative).
    NoteJudgementCount {
        n: i64,
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

impl Checker {
    /// Whether an AND resets this item's count when another item fails.
    fn count_resettable(&self) -> bool {
        matches!(self, Checker::NoteJudgementCount { .. } | Checker::Not(_))
    }

    pub(crate) fn check(&mut self, ctx: &mut CheckCtx) -> Result<(bool, i64), Error> {
        match self {
            Checker::And { items, resettable } => {
                let mut hit = 0i64;
                for i in 0..items.len() {
                    let (ok, h) = items[i].check(ctx)?;
                    hit = hit.max(h);
                    if !ok {
                        if !resettable[i] {
                            for (c, &r) in items.iter_mut().zip(resettable.iter()) {
                                if r {
                                    c.reset_count();
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
            Checker::SameMemberLiveSkill(k) => {
                let ok = ctx.events.iter().any(|&(index, _)| index == *k);
                Ok((ok, ok as i64))
            }
            Checker::Probability(rate) => Ok((ctx.random.value(SKILL) < *rate, 0)),
            Checker::Fixed(ok) => Ok((*ok, *ok as i64)),
            Checker::NoteJudgementCount { n, targets, count, override_ms } => {
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
            Checker::NoteJudgementCount { override_ms, .. }
            | Checker::LuckLotResult { override_ms, .. }
            | Checker::ComboAtLeast { override_ms, .. }
            | Checker::RangeStart { override_ms, .. }
            | Checker::RangePlaying { override_ms, .. } => *override_ms,
            Checker::Not(inner) => inner.override_time(),
            _ => None,
        }
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Checker::And { items, .. } | Checker::Or(items) => items.iter_mut().for_each(Checker::reset),
            Checker::Not(inner) => inner.reset(),
            Checker::NoteJudgementCount { count, override_ms, .. } => {
                *count = 0;
                *override_ms = None;
            }
            Checker::LuckLotResult { override_ms, .. } | Checker::ComboAtLeast { override_ms, .. } => {
                *override_ms = None;
            }
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
    }

    fn reset_count(&mut self) {
        match self {
            Checker::NoteJudgementCount { .. } => self.reset(),
            Checker::Not(inner) if inner.count_resettable() => inner.reset_count(),
            _ => {}
        }
    }
}

/// A cumulative condition: the count an effect state carries while it runs (read by Gekisou effects).
#[derive(Clone, Debug)]
pub(crate) enum Cumulative {
    /// `floor(Gekisou combo of the playing range / n)`, 0 outside a range (and without Gekisou).
    ComboPerN { n: i64, values_empty: bool, max: i64 },
    /// `floor(judged target notes counted while the effect runs / n)`.
    JudgementPerN { n: i64, values_empty: bool, targets: Vec<i64>, max: i64, count: i32 },
}

impl Cumulative {
    pub(crate) fn unit(&self) -> i64 {
        match self {
            Cumulative::ComboPerN { n, .. } | Cumulative::JudgementPerN { n, .. } => *n,
        }
    }

    pub(crate) fn max(&self) -> i64 {
        match self {
            Cumulative::ComboPerN { max, .. } | Cumulative::JudgementPerN { max, .. } => *max,
        }
    }

    pub(crate) fn update_count(&mut self, ctx: &CheckCtx) -> Result<i64, Error> {
        match self {
            Cumulative::ComboPerN { n, values_empty, max } => {
                let Some(g) = ctx.gk else { return Ok(0) };
                let idx = g.ctrl.current_playing_index;
                let mut v = 0i64;
                if idx >= 0 {
                    if *values_empty {
                        return Err(no_value());
                    }
                    v = floor_to_i32(g.ctrl.states[idx as usize].combo as f32 / *n as f32) as i64;
                }
                Ok(if v >= *max { *max } else { v })
            }
            Cumulative::JudgementPerN { n, values_empty, targets, max, count } => {
                for &(_, j, _) in ctx.judged {
                    if targets.contains(&(j as i64)) {
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
        if let Cumulative::JudgementPerN { count, .. } = self {
            *count = 0;
        }
    }
}

fn no_value() -> Error {
    Error::Master("skill condition without a value".into())
}

fn missing_target(id: i64) -> Error {
    Error::Master(format!("unknown skill target {id}"))
}

/// Member target match: only member attribute targets are modelled; the first set key among character, band and card
/// type decides.
fn target_matches(tg: &SkillTargetRow, p: &Performer) -> Result<bool, Error> {
    if tg.skill_target_type != TARGET_MEMBER {
        return Err(Error::Unsupported(format!("member target of type {}", tg.skill_target_type)));
    }
    for (key, val) in [(tg.character_id, p.character_id), (tg.band_id, p.band_id), (tg.card_type, p.card_type)] {
        if key != 0 {
            return Ok(key == val);
        }
    }
    Ok(false)
}

/// Builds checkers for the performer at `member` of `deck`.
pub(crate) struct Factory<'a> {
    pub master: &'a Master,
    pub deck: &'a [Performer],
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
            2001 => Checker::LifeAtLeast(v0),
            2003 => Checker::LifeAtMost(v0),
            4010 => Checker::SameMemberLiveSkill(k as i32),
            4011 => Checker::Probability(v0.ok_or_else(no_value)? as f32 / 100f32),
            5000 => {
                let mut fixed = false;
                if let Some(p) = self.deck.get(k) {
                    for i in 0..targets.len() {
                        if target_matches(target(i)?, p)? {
                            fixed = true;
                            break;
                        }
                    }
                }
                Checker::Fixed(fixed)
            }
            8000 => Checker::Fixed(false),
            1030 => {
                let n = v0.ok_or_else(no_value)?;
                let mut judgements = Vec::with_capacity(targets.len());
                for i in 0..targets.len() {
                    let j = target(i)?.judgement;
                    if j != NO_JUDGEMENT {
                        judgements.push(j);
                    }
                }
                Checker::NoteJudgementCount { n, targets: judgements, count: 0, override_ms: None }
            }
            7000 => Checker::LuckLotResult { target: v0.ok_or_else(no_value)?, override_ms: None },
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
        if gid == 0 {
            return Ok(None);
        }
        let mut ors = Vec::new();
        for s in self.master.skill_condition_sets.iter().filter(|s| s.group == gid) {
            let mut items = Vec::new();
            for &cid in &s.condition_ids {
                if let Some(c) = self.one(cid, k)? {
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
    pub(crate) fn cumulative(&self, cid: i64) -> Result<Option<Cumulative>, Error> {
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
            7001 => Ok(Some(Cumulative::ComboPerN { n, values_empty, max })),
            1000 => {
                let mut targets = Vec::with_capacity(c.condition_target_ids.len());
                for &i in &c.condition_target_ids {
                    let j = m.skill_target(i).ok_or_else(|| missing_target(i))?.judgement;
                    if j != NO_JUDGEMENT {
                        targets.push(j);
                    }
                }
                Ok(Some(Cumulative::JudgementPerN { n, values_empty, targets, max, count: 0 }))
            }
            t => Err(Error::Unsupported(format!("cumulative condition type {t}"))),
        }
    }
}
