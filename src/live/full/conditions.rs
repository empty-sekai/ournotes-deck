//! Skill condition checkers and the factory that builds them from the condition tables.
//!
//! A checker answers `(hit, hit count)` and may carry an override time (the trigger time of a condition that counts
//! notes is the chart time of the note that completed the count). Condition groups are an OR over their condition
//! sets, each set an AND over its conditions; both stop at the first decisive item.

use super::Performer;
use super::life::LifeController;
use crate::error::Error;
use crate::live::random::{LiveRandom, SKILL};
use crate::master::{Master, SkillTargetRow};

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
}

/// Target type of member attribute targets (band, character, card type).
const TARGET_MEMBER: i64 = 3;
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
    /// Reads the Gekisou state, which a live without Gekisou does not have: asking it fails.
    NeedsGekisou(i64),
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
            Checker::NeedsGekisou(t) => {
                Err(Error::Unsupported(format!("condition type {t} reads the Gekisou state of a live without Gekisou")))
            }
        }
    }

    /// The trigger time override of the last check, if any.
    pub(crate) fn override_time(&self) -> Option<i32> {
        match self {
            Checker::NoteJudgementCount { override_ms, .. } => *override_ms,
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
            // No lottery results in a live without Gekisou: never hits.
            7000 => {
                v0.ok_or_else(no_value)?;
                Checker::Fixed(false)
            }
            7005 => {
                v0.ok_or_else(no_value)?;
                Checker::NeedsGekisou(7005)
            }
            // The mission targets are read when the checker is built.
            t @ (7010 | 7020) => {
                for i in 0..targets.len() {
                    target(i)?;
                }
                Checker::NeedsGekisou(t)
            }
            t @ (7013 | 7021) => Checker::NeedsGekisou(t),
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

    /// Validates a cumulative condition (id 0: none). Its count only feeds Gekisou effects, so it is not kept; the
    /// result tells whether counting fails (a judgement count without a value), which happens when the effect first
    /// runs.
    pub(crate) fn cumulative(&self, cid: i64) -> Result<bool, Error> {
        if cid == 0 {
            return Ok(false);
        }
        let m = self.master;
        let c =
            m.cumulative_condition(cid).ok_or_else(|| Error::Master(format!("unknown cumulative condition {cid}")))?;
        match c.condition_type {
            // Gekisou combo per N: 0 without Gekisou.
            7001 => Ok(false),
            1000 => {
                for &i in &c.condition_target_ids {
                    m.skill_target(i).ok_or_else(|| missing_target(i))?;
                }
                Ok(c.condition_values.is_empty())
            }
            t => Err(Error::Unsupported(format!("cumulative condition type {t}"))),
        }
    }
}
