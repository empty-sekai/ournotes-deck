//! Lottery-related skill effects, classified from the master: the luck chain effects that feed the lottery and the
//! score-ups that read it. A LUCK weighted live ([`super::LiveModel::set_luck_weights`]) and the LUCK table of the
//! chart statistics both take their skills from here, so a new skill joins them by its rows alone, and a row the
//! model cannot take is refused by name.
//!
//! - A luck chain row has an effect type of 11000..=11005.
//! - A lottery-dependent score-up row is any other row whose trigger, condition, release or limit reset group reads
//!   the lot result (7000) or the rush (7021); its effect must be a note score-up (2000 or 2005). Its shape is what
//!   decides when it runs: source table, mission gate, trigger type, duration, limits and condition groups, with
//!   the formation predicates (5000) dropped since they are fixed per deck.
//! - Both may only read conditions the LUCK-only replay answers as the complete live does on the theoretical best
//!   play ([`LUCK_REPLAY_CONDITIONS`]), and only Gekisou skills and Gekisou support skills may have them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Performer;
use crate::error::Error;
use crate::master::{GekisouSkillEffectRow, Master};

/// Condition types that read the lottery: the lot result and the rush.
pub const LOTTERY_CONDITIONS: [i64; 2] = [7000, 7021];
/// The formation predicate: fixed per deck.
pub const FORMATION: i64 = 5000;
/// Condition types the LUCK-only replay answers as the complete live does on the theoretical best play: life
/// comparisons (life stays full), probability, formation, the lottery and the Gekisou range states.
pub const LUCK_REPLAY_CONDITIONS: [i64; 11] = [2000, 2001, 2002, 2003, 4011, 5000, 7000, 7010, 7013, 7020, 7021];

/// Whether an effect type feeds the lottery.
pub fn is_luck_chain(effect_type: i64) -> bool {
    (11000..=11005).contains(&effect_type)
}

/// The skill table of a lottery-related skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LuckSource {
    /// A member's Gekisou skill.
    Gekisou,
    /// A snap's Gekisou support skill.
    GekisouSupport,
}

impl LuckSource {
    /// The effect rows of this source.
    pub fn rows(self, master: &Master) -> &[GekisouSkillEffectRow] {
        match self {
            LuckSource::Gekisou => &master.gekisou_skill_effects,
            LuckSource::GekisouSupport => &master.gekisou_support_skill_effects,
        }
    }

    /// The mission that gates a skill's triggers.
    fn mission(self, master: &Master, id: i64) -> Result<i64, Error> {
        let row = match self {
            LuckSource::Gekisou => master.gekisou_skill(id),
            LuckSource::GekisouSupport => master.gekisou_support_skill(id),
        };
        row.map(|r| r.gekisou_mission_type).ok_or_else(|| Error::Master(format!("unknown {self:?} skill {id}")))
    }
}

/// A skill at a level and whether its holder matches its formation predicates (`None`: its rows have none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckSkillKey {
    pub source: LuckSource,
    pub id: i64,
    pub level: i64,
    pub matched: Option<bool>,
}

/// A condition: type, values and whether it is positive.
pub type LuckCondition = (i64, Vec<i64>, bool);

/// What decides when a lottery-dependent score-up runs, its value and formation predicates aside.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreShape {
    pub source: LuckSource,
    /// The mission gate of the skill's triggers.
    pub mission: i64,
    pub trigger_type: i64,
    /// `activationTimeSecond` as its binary32 bits.
    pub activation_time_bits: u32,
    pub effect_limit_count: i64,
    pub effect_execute_limit_count: i64,
    pub cumulative_condition_id: i64,
    /// Trigger, condition, release and limit reset groups: their condition sets, formation predicates dropped.
    pub groups: [Vec<Vec<LuckCondition>>; 4],
}

/// A lottery-dependent score-up shape and its probe: the first skill (source, id, level) with a row of the shape,
/// held so that row runs.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckShapeProbe {
    pub shape: LuckScoreShape,
    pub probe: LuckSkillKey,
}

/// The lottery-related skills of a master.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckSkills {
    /// Every luck chain skill at every level, each formation variant.
    pub chain: Vec<LuckSkillKey>,
    /// The lottery-dependent score-up shapes, in order.
    pub shapes: Vec<LuckShapeProbe>,
    /// The shape of each lottery-dependent score-up row by source and row id.
    #[serde(skip)]
    pub rows: BTreeMap<(LuckSource, i64), usize>,
}

impl LuckSkills {
    /// Whether a Gekisou (support) skill effect row feeds the lottery or reads it.
    pub fn related(&self, source: LuckSource, r: &GekisouSkillEffectRow) -> bool {
        is_luck_chain(r.skill_effect_type) || self.rows.contains_key(&(source, r.id))
    }
}

/// What a row is to the lottery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RowLuck {
    Unrelated,
    Chain,
    Score(LuckScoreShape),
}

fn group(master: &Master, g: i64) -> Result<Vec<Vec<LuckCondition>>, Error> {
    if g == 0 {
        return Ok(Vec::new());
    }
    master
        .skill_condition_sets
        .iter()
        .filter(|s| s.group == g)
        .map(|s| {
            s.condition_ids
                .iter()
                .map(|&cid| {
                    let c = master
                        .skill_condition(cid)
                        .ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
                    Ok((c.condition_type, c.condition_values.clone(), c.is_positive))
                })
                .collect()
        })
        .collect()
}

fn reads(groups: &[Vec<Vec<LuckCondition>>], types: &[i64]) -> bool {
    groups.iter().flatten().flatten().any(|c| types.contains(&c.0))
}

/// The condition groups of a Gekisou (support) skill effect row: trigger, condition, release, limit reset.
fn row_groups(master: &Master, r: &GekisouSkillEffectRow) -> Result<[Vec<Vec<LuckCondition>>; 4], Error> {
    Ok([
        group(master, r.skill_trigger_condition_group)?,
        group(master, r.skill_condition_group)?,
        group(master, r.skill_release_condition_group)?,
        group(master, r.effect_execute_limit_reset_condition_group)?,
    ])
}

/// What a Gekisou (support) skill effect row is to the lottery; an error for a lottery-related row the model cannot
/// take.
pub(crate) fn classify(master: &Master, source: LuckSource, r: &GekisouSkillEffectRow) -> Result<RowLuck, Error> {
    let groups = row_groups(master, r)?;
    let chain = is_luck_chain(r.skill_effect_type);
    if !chain && !reads(&groups, &LOTTERY_CONDITIONS) {
        return Ok(RowLuck::Unrelated);
    }
    let what = || format!("{source:?} skill {} level {} (effect row {})", r.skill_id, r.level, r.id);
    if let Some(c) = groups.iter().flatten().flatten().find(|c| !LUCK_REPLAY_CONDITIONS.contains(&c.0)) {
        return Err(Error::Unsupported(format!(
            "{}: condition type {} beside the lottery is outside the LUCK-only replay",
            what(),
            c.0
        )));
    }
    if chain {
        return Ok(RowLuck::Chain);
    }
    if !matches!(r.skill_effect_type, 2000 | 2005) {
        return Err(Error::Unsupported(format!(
            "{}: effect type {} reads the lottery; only note score-ups (2000, 2005) are modeled",
            what(),
            r.skill_effect_type
        )));
    }
    let groups =
        groups.map(|g| g.into_iter().map(|set| set.into_iter().filter(|c| c.0 != FORMATION).collect()).collect());
    Ok(RowLuck::Score(LuckScoreShape {
        source,
        mission: source.mission(master, r.skill_id)?,
        trigger_type: r.skill_trigger_type,
        activation_time_bits: r.activation_time_second.to_bits(),
        effect_limit_count: r.effect_limit_count,
        effect_execute_limit_count: r.effect_execute_limit_count,
        cumulative_condition_id: r.skill_cumulative_condition_id,
        groups,
    }))
}

/// The target ids of the formation predicates of a row, sorted, with whether each predicate is positive.
fn formation(master: &Master, r: &GekisouSkillEffectRow) -> Result<Vec<(Vec<i64>, bool)>, Error> {
    let mut out = Vec::new();
    for g in [r.skill_trigger_condition_group, r.skill_condition_group] {
        for s in master.skill_condition_sets.iter().filter(|s| g != 0 && s.group == g) {
            for &cid in &s.condition_ids {
                let c = master
                    .skill_condition(cid)
                    .ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
                if c.condition_type == FORMATION {
                    let mut t = c.condition_target_ids.clone();
                    t.sort_unstable();
                    out.push((t, c.is_positive));
                }
            }
        }
    }
    Ok(out)
}

/// The one formation target set of a skill at a level (`None`: no formation predicate); an error when its rows test
/// different sets.
pub fn formation_targets(master: &Master, source: LuckSource, id: i64, level: i64) -> Result<Option<Vec<i64>>, Error> {
    let mut set: Option<Vec<i64>> = None;
    for r in source.rows(master).iter().filter(|r| r.skill_id == id && r.level == level) {
        for (t, _) in formation(master, r)? {
            match &set {
                None => set = Some(t),
                Some(s) if *s == t => {}
                Some(_) => {
                    return Err(Error::Unsupported(format!(
                        "{source:?} skill {id} level {level}: formation predicates on different targets"
                    )));
                }
            }
        }
    }
    Ok(set)
}

/// The key of a skill held by a performer: whether the holder matches its formation targets.
pub fn luck_skill_key(
    master: &Master,
    source: LuckSource,
    id: i64,
    level: i64,
    holder: &Performer,
) -> Result<LuckSkillKey, Error> {
    let matched = formation_targets(master, source, id, level)?
        .map(|targets| targets.iter().any(|&t| master.skill_target(t).is_some_and(|t| holder.matches_skill_target(t))));
    Ok(LuckSkillKey { source, id, level, matched })
}

/// The lottery-related skills of a master; an error for a lottery-related row the model cannot take.
pub fn luck_skills(master: &Master) -> Result<LuckSkills, Error> {
    for r in &master.live_skill_effects {
        let groups =
            [r.skill_condition_group, r.skill_release_condition_group, r.effect_execute_limit_reset_condition_group]
                .map(|g| group(master, g));
        let groups = groups.into_iter().collect::<Result<Vec<_>, _>>()?;
        if is_luck_chain(r.skill_effect_type) || reads(&groups, &LOTTERY_CONDITIONS) {
            return Err(Error::Unsupported(format!(
                "live skill {} level {}: a lottery-related effect",
                r.live_skill_id, r.level
            )));
        }
    }
    for r in &master.support_skill_effects {
        let groups = [
            r.skill_trigger_condition_group,
            r.skill_condition_group,
            r.skill_release_condition_group,
            r.effect_execute_limit_reset_condition_group,
        ]
        .map(|g| group(master, g));
        let groups = groups.into_iter().collect::<Result<Vec<_>, _>>()?;
        if is_luck_chain(r.skill_effect_type) || reads(&groups, &LOTTERY_CONDITIONS) {
            return Err(Error::Unsupported(format!(
                "support skill {} level {}: a lottery-related effect",
                r.support_skill_id, r.level
            )));
        }
    }
    let mut chain = BTreeMap::new();
    let mut shapes: BTreeMap<LuckScoreShape, Vec<LuckSkillKey>> = BTreeMap::new();
    let mut rows_of: Vec<((LuckSource, i64), LuckScoreShape)> = Vec::new();
    for source in [LuckSource::Gekisou, LuckSource::GekisouSupport] {
        let mut rows: Vec<&GekisouSkillEffectRow> = source.rows(master).iter().collect();
        rows.sort_by_key(|r| (r.skill_id, r.level, r.id));
        for r in rows {
            match classify(master, source, r)? {
                RowLuck::Unrelated => {}
                RowLuck::Chain => {
                    let variants = match formation_targets(master, source, r.skill_id, r.level)? {
                        None => vec![None],
                        Some(_) => vec![Some(true), Some(false)],
                    };
                    for matched in variants {
                        chain.insert(LuckSkillKey { source, id: r.skill_id, level: r.level, matched }, ());
                    }
                }
                RowLuck::Score(shape) => {
                    let f = formation(master, r)?;
                    formation_targets(master, source, r.skill_id, r.level)?;
                    let matched = if f.is_empty() { None } else { Some(f.iter().all(|x| x.1)) };
                    rows_of.push(((source, r.id), shape.clone()));
                    shapes.entry(shape).or_default().push(LuckSkillKey {
                        source,
                        id: r.skill_id,
                        level: r.level,
                        matched,
                    });
                }
            }
        }
    }
    let index: BTreeMap<&LuckScoreShape, usize> = shapes.keys().enumerate().map(|(i, s)| (s, i)).collect();
    let rows = rows_of.iter().map(|(k, s)| (*k, index[s])).collect();
    // A probe feeds no lottery, so the probes leave the rush of a table deck as it is.
    let feeds = |k: &LuckSkillKey| chain.keys().any(|c| (c.source, c.id, c.level) == (k.source, k.id, k.level));
    let shapes = shapes
        .into_iter()
        .map(|(shape, keys)| {
            let probe = keys.iter().copied().find(|k| !feeds(k)).ok_or_else(|| {
                Error::Unsupported(format!("a lottery-dependent score-up held only by luck chain skills: {shape:?}"))
            })?;
            Ok(LuckShapeProbe { shape, probe })
        })
        .collect::<Result<_, Error>>()?;
    Ok(LuckSkills { chain: chain.into_keys().collect(), shapes, rows })
}

/// A holder of a skill key: a performer whose attributes match the skill's formation targets when the key is
/// matched (the first target), else none of them.
pub fn luck_holder(master: &Master, key: LuckSkillKey) -> Result<Performer, Error> {
    let mut p = Performer::default();
    if key.matched == Some(true) {
        let targets = formation_targets(master, key.source, key.id, key.level)?.unwrap_or_default();
        let t = targets
            .first()
            .and_then(|&t| master.skill_target(t))
            .ok_or_else(|| Error::Master(format!("{:?} skill {}: no formation target", key.source, key.id)))?;
        p.band_id = t.band_id.max(0);
        p.character_id = t.character_id.max(0);
        p.card_type = t.card_type;
        if t.tag_id > 0 {
            p.tag_ids.push(t.tag_id);
        }
        if !p.matches_skill_target(t) {
            return Err(Error::Unsupported(format!(
                "{:?} skill {}: no performer attributes match its formation target {}",
                key.source, key.id, t.id
            )));
        }
    }
    Ok(p)
}
