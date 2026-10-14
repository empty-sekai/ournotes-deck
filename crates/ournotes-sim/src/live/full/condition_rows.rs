//! Condition skills and their effect rows, read from the master data.

use super::*;

/// The columns of a condition skill effect row (snap or Gekisou table).
pub(super) struct CondRow<'a> {
    id: i64,
    trigger_type: i64,
    trigger_group: i64,
    condition_group: i64,
    release_group: i64,
    target_ids: &'a [i64],
    effect_type: i64,
    act: f32,
    value: i64,
    max_value: i64,
    limit: i64,
    cumulative_id: i64,
    execute_limit: i64,
    reset_group: i64,
}

impl<'a> From<&'a SupportSkillEffectRow> for CondRow<'a> {
    fn from(r: &'a SupportSkillEffectRow) -> CondRow<'a> {
        CondRow {
            id: r.id,
            trigger_type: r.skill_trigger_type,
            trigger_group: r.skill_trigger_condition_group,
            condition_group: r.skill_condition_group,
            release_group: r.skill_release_condition_group,
            target_ids: &r.skill_target_ids,
            effect_type: r.skill_effect_type,
            act: r.activation_time_second,
            value: r.effect_value,
            max_value: r.max_effect_value,
            limit: r.effect_limit_count,
            cumulative_id: r.skill_cumulative_condition_id,
            execute_limit: r.effect_execute_limit_count,
            reset_group: r.effect_execute_limit_reset_condition_group,
        }
    }
}

impl<'a> From<&'a GekisouSkillEffectRow> for CondRow<'a> {
    fn from(r: &'a GekisouSkillEffectRow) -> CondRow<'a> {
        CondRow {
            id: r.id,
            trigger_type: r.skill_trigger_type,
            trigger_group: r.skill_trigger_condition_group,
            condition_group: r.skill_condition_group,
            release_group: r.skill_release_condition_group,
            target_ids: &r.skill_target_ids,
            effect_type: r.skill_effect_type,
            act: r.activation_time_second,
            value: r.effect_value,
            max_value: r.max_effect_value,
            limit: r.effect_limit_count,
            cumulative_id: r.skill_cumulative_condition_id,
            execute_limit: r.effect_execute_limit_count,
            reset_group: r.effect_execute_limit_reset_condition_group,
        }
    }
}

/// A lottery-dependent note score-up of a live ([`LiveModel::luck_score_rows`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LuckScoreRow {
    /// The effect row.
    pub row: usize,
    pub member: usize,
    /// The owner id of the row's score commands.
    pub owner: i32,
    /// The shape index in [`LuckSkills::shapes`].
    pub shape: usize,
    /// The note score-up.
    pub value: f32,
    /// Whether its formation predicates let it run.
    pub may_hold: bool,
}

fn effect_row(master: &Master, r: &CondRow) -> EffectRow {
    let targets: Result<Vec<i64>, i64> =
        r.target_ids.iter().map(|&t| master.skill_target(t).map(|x| x.judgement).ok_or(t)).collect();
    EffectRow {
        id: r.id,
        effect_type: r.effect_type,
        effect_value: r.value,
        max_effect_value: r.max_value,
        effect_limit_count: r.limit,
        applier: applier_plan::ApplierPlan::compile(r.effect_type, targets.is_ok()),
        targets,
    }
}

pub(super) fn setting(master: &Master, key: &str) -> Result<i64, Error> {
    let v = master.live_setting(key).ok_or_else(|| Error::Master(format!("missing live setting {key}")))?;
    v.trim().parse::<i64>().map_err(|_| Error::Master(format!("live setting {key} is not an integer: {v}")))
}

/// The mission list a Gekisou skill joins when the skill status builds its Gekisou lists: 1..=3 their
/// own list, All (4) every list, so its skill (added once, `TryAdd`) takes the Combo list's place; any other value
/// is `get_Item` on the three-key dictionary, a KeyNotFoundException.
pub(super) fn gekisou_mission_group(mission: i64) -> Result<i64, Error> {
    match mission {
        1..=3 => Ok(mission),
        4 => Ok(1),
        m => Err(Error::Game(format!("KeyNotFoundException: Gekisou mission type {m}"))),
    }
}

/// Effect types whose appliers exist only with Gekisou on; the applier container is keyed by effect type alone, whatever the skill type (13005 is handled with
/// 12006, 13001 belongs to the raw judgement bridge).
pub(super) const GEKISOU_APPLIER_TYPES: [i64; 14] =
    [11000, 11001, 11002, 11003, 11004, 11005, 12000, 12002, 12003, 12004, 13000, 13002, 13003, 13004];

/// Builds the condition skill of one member from its effect rows (sorted by id): effect key = row id * 100 + skill
/// type * 10 + member index.
#[allow(clippy::too_many_arguments)]
pub(super) fn condition_skill(
    master: &Master,
    factory: &Factory,
    phase: &dyn Fn(i64) -> i64,
    mut rs: Vec<CondRow>,
    k: usize,
    skill_type: i64,
    gate: Option<i64>,
    rows: &mut Vec<EffectRow>,
    luck: Option<&luck::SharedScript>,
) -> Result<CondSkill, Error> {
    let group = |gid| match luck {
        Some(script) => factory.luck_group(gid, k, script),
        None => factory.group(gid, k),
    };
    rs.sort_by_key(|r| r.id);
    let mut effects = Vec::with_capacity(rs.len());
    let mut release_groups = Vec::with_capacity(rs.len());
    for r in rs {
        if r.trigger_type != ONE_SHOT && r.trigger_type != SUSTAINED {
            // The client's condition updater accepts trigger types 1 and 2 only and throws for any other.
            return Err(Error::Game(format!("ArgumentOutOfRangeException: skill trigger type {}", r.trigger_type)));
        }
        let effect_id = r.id.wrapping_mul(100).wrapping_add(skill_type * 10).wrapping_add(k as i64);
        let trigger = group(r.trigger_group)?;
        let condition = group(r.condition_group)?;
        let reset = if r.execute_limit > 0 { group(r.reset_group)? } else { None };
        let cumulative = if r.trigger_type == ONE_SHOT || r.trigger_type == SUSTAINED {
            factory.cumulative(r.cumulative_id, k)?
        } else {
            None
        };
        rows.push(effect_row(master, &r));
        effects.push(CondEffect {
            effect_id,
            trigger_type: r.trigger_type,
            act: r.act,
            phase: phase(r.effect_type),
            trigger,
            condition,
            execute_limit: r.execute_limit,
            reset,
            row: rows.len() - 1,
            cumulative,
        });
        release_groups.push(r.release_group);
    }
    let updater = ConditionSkillUpdater::new(effects, |e| group(release_groups[e]), gate)?;
    let list_group = if skill_type == SKILL_TYPE_GEKISOU { gekisou_mission_group(gate.unwrap_or(0))? } else { 0 };
    Ok(CondSkill { member: k, skill_type, group: list_group, updater })
}

/// Appends the condition skills of the members' Gekisou skills and Gekisou support skills of a live with Gekisou, in
/// updater order: the Gekisou skills by mission list then member, then each member's Gekisou support skills in their
/// order. A member without a Gekisou skill holds no Gekisou support skill. `luck` keeps the rows the LUCK-only replay
/// runs and `lottery` the lottery-related rows; the effect rows are appended to `rows`.
pub(super) fn gekisou_conditions(
    factory: &Factory,
    phase: &dyn Fn(i64) -> i64,
    luck: Option<&luck::SharedScript>,
    lottery: Option<&LuckSkills>,
    cond: &mut Vec<CondSkill>,
    rows: &mut Vec<EffectRow>,
) -> Result<(), Error> {
    let (master, deck) = (factory.master, factory.deck());
    let mut mission = vec![None; deck.len()];
    for (k, p) in deck.iter().enumerate() {
        let Some((sid, _)) = p.gekisou_skill else { continue };
        let row = master.gekisou_skill(sid).ok_or_else(|| Error::Master(format!("unknown Gekisou skill {sid}")))?;
        gekisou_mission_group(row.gekisou_mission_type)?;
        mission[k] = Some(row.gekisou_mission_type);
    }
    for m in 1..=3 {
        for (k, p) in deck.iter().enumerate() {
            let (Some((sid, lv)), Some(mk)) = (p.gekisou_skill, mission[k]) else { continue };
            if gekisou_mission_group(mk)? != m {
                continue;
            }
            let rs = master
                .gekisou_skill_effects
                .iter()
                .filter(|r| r.skill_id == sid && r.level == lv)
                .filter(|r| luck.is_none() || luck::retained(r.skill_effect_type))
                .filter(|r| lottery.is_none_or(|l| l.related(LuckSource::Gekisou, r)))
                .map(CondRow::from)
                .collect();
            cond.push(condition_skill(master, factory, phase, rs, k, SKILL_TYPE_GEKISOU, Some(mk), rows, luck)?);
        }
    }
    for (k, p) in deck.iter().enumerate() {
        if mission[k].is_none() {
            continue;
        }
        for &(sid, lv) in &p.gekisou_support_skills {
            let row = master
                .gekisou_support_skill(sid)
                .ok_or_else(|| Error::Master(format!("unknown Gekisou support skill {sid}")))?;
            let rs = master
                .gekisou_support_skill_effects
                .iter()
                .filter(|r| r.skill_id == sid && r.level == lv)
                .filter(|r| luck.is_none() || luck::retained(r.skill_effect_type))
                .filter(|r| lottery.is_none_or(|l| l.related(LuckSource::GekisouSupport, r)))
                .map(CondRow::from)
                .collect();
            let gate = Some(row.gekisou_mission_type);
            cond.push(condition_skill(master, factory, phase, rs, k, SKILL_TYPE_GEKISOU_SUPPORT, gate, rows, luck)?);
        }
    }
    Ok(())
}

/// The effect phase of each skill effect type (1 when the master does not set it).
pub(super) fn effect_phases(master: &Master) -> FxHashMap<i64, i64> {
    master.skill_effect_settings.iter().map(|r| (r.skill_effect_type, r.phase)).collect()
}

/// The condition skills that hold an effect and the effect rows of a live with Gekisou built for `deck` with the
/// lottery-related rows of `lottery` ([`LiveModel::build`]): `deck` holds no live or support skill, so these are
/// everything the build reads of its performers. A condition skill without an effect only reads the Gekisou gate.
pub(super) fn lottery_conditions(
    master: &Master,
    deck: &[Performer],
    skill_target_music_type: i64,
    lottery: &LuckSkills,
) -> Result<(Vec<CondSkill>, Vec<EffectRow>), Error> {
    let (cond, rows, _) = lottery_conditions_read(master, deck, skill_target_music_type, lottery)?;
    Ok((cond, rows))
}

/// [`lottery_conditions`] and whether one of their checkers read the formation beyond its holder
/// ([`Factory::formation_read`]).
pub(super) fn lottery_conditions_read(
    master: &Master,
    deck: &[Performer],
    skill_target_music_type: i64,
    lottery: &LuckSkills,
) -> Result<(Vec<CondSkill>, Vec<EffectRow>, bool), Error> {
    if deck.iter().any(|p| p.live_skill.is_some() || !p.support_skills.is_empty()) {
        return Err(Error::Input(
            "the lottery conditions are those of performers without live or support skills".into(),
        ));
    }
    let (initial_life, _) = life::life_settings(master)?;
    let factory = Factory::new(master, deck, initial_life, 0, skill_target_music_type);
    let phase_of = effect_phases(master);
    let phase = |t: i64| phase_of.get(&t).copied().unwrap_or(1);
    let (mut cond, mut rows) = (Vec::new(), Vec::new());
    gekisou_conditions(&factory, &phase, None, Some(lottery), &mut cond, &mut rows)?;
    cond.retain(|skill| !skill.updater.effects().is_empty());
    Ok((cond, rows, factory.formation_read()))
}

/// The lottery-dependent note score-ups of the Gekisou (support) condition skills `cond` with effect rows `rows`
/// ([`luck_skills`]).
pub(super) fn luck_score_rows(cond: &[CondSkill], rows: &[EffectRow], skills: &LuckSkills) -> Vec<LuckScoreRow> {
    let mut out = Vec::new();
    for c in cond {
        let (source, owner_type) = match c.skill_type {
            SKILL_TYPE_GEKISOU => (LuckSource::Gekisou, OWNER_MEMBER),
            SKILL_TYPE_GEKISOU_SUPPORT => (LuckSource::GekisouSupport, OWNER_SNAP),
            _ => continue,
        };
        for e in c.updater.effects() {
            let row = &rows[e.row];
            let Some(&shape) = skills.rows.get(&(source, row.id)) else { continue };
            let divisor = if row.effect_type == 2005 { -10000f32 } else { 10000f32 };
            let m = note_factor_mill(row.effect_value as f32 / divisor);
            out.push(LuckScoreRow {
                row: e.row,
                member: c.member,
                owner: (c.member as i32).wrapping_mul(100).wrapping_add(owner_type),
                shape,
                value: if m != 0 { m as f32 / 100000f32 } else { 0f32 },
                may_hold: [&e.trigger, &e.condition].into_iter().flatten().all(Checker::may_hold),
            });
        }
    }
    out
}
