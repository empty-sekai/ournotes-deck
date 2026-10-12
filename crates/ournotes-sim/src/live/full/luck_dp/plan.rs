//! The plan of the lottery condition skills: their actions, score probes and order pairs.

use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Action<M> {
    StartGauge { value: i64, chance: M },
    StartMinimum { result: i8, chance: M },
    MissGauge { value: i64 },
    CriticalPoints { value: i32, chance: M },
    StartPoints { value: i32, chance: M },
}

pub(super) struct Plan<M> {
    pub(super) actions: Vec<(i64, Action<M>, Option<Checker>)>,
    pub(super) probes: Vec<bool>,
    /// The pairs of performers whose condition skills hold different actions in one phase: the phase's actions are
    /// listed in updater order, so their list follows the relative order of these pairs and of no other.
    pub(super) order_pairs: PositionPairs,
}

impl<M> Default for Plan<M> {
    fn default() -> Self {
        Self { actions: Vec::new(), probes: Vec::new(), order_pairs: 0 }
    }
}

pub(super) fn unsupported(row: &EffectRow, why: &str) -> Error {
    Error::Unsupported(format!("LUCK DP effect row {} (type {}): {why}", row.id, row.effect_type))
}

/// These predicates contain no mutable checkers. Their nominal probability is sufficient because every
/// accepted start row is checked only once per range. Formation was already resolved by the native factory.
/// Life comparisons read the complete native recorder at this frame and skill phase, never initial life.
pub(super) fn chance<M: Mass>(checker: &Checker, life: i32) -> Option<M> {
    let fixed = |value| Some(if value { M::ONE } else { M::ZERO });
    match checker {
        Checker::Fixed(value) => fixed(*value),
        Checker::Probability(value) if value.is_finite() => M::from_f32(value.clamp(0.0, 1.0)),
        Checker::LifeGreater(Some(value)) => fixed(i64::from(life) > *value),
        Checker::LifeAtLeast(Some(value)) => fixed(i64::from(life) >= *value),
        Checker::LifeLess(Some(value)) => fixed(i64::from(life) < *value),
        Checker::LifeAtMost(Some(value)) => fixed(i64::from(life) <= *value),
        Checker::Not(inner) => chance(inner, life).map(M::complement),
        Checker::And { items, .. } => {
            items.iter().try_fold(M::ONE, |p, item| chance(item, life).map(|q| p.multiply(q)))
        }
        Checker::Or(items) => items
            .iter()
            .try_fold(M::ONE, |p, item| chance::<M>(item, life).map(|q| p.multiply(q.complement())))
            .map(M::complement),
        _ => None,
    }
}

pub(crate) fn fixed_condition(checker: &Checker) -> Option<bool> {
    match checker {
        Checker::Fixed(value) => Some(*value),
        Checker::Not(inner) => fixed_condition(inner).map(|value| !value),
        Checker::And { items, .. } => {
            items.iter().try_fold(true, |value, item| fixed_condition(item).map(|next| value && next))
        }
        Checker::Or(items) => {
            items.iter().try_fold(false, |value, item| fixed_condition(item).map(|next| value || next))
        }
        _ => None,
    }
}

fn has_probability(checker: &Checker) -> bool {
    match checker {
        Checker::Probability(_) => true,
        Checker::Not(inner) => has_probability(inner),
        Checker::And { items, .. } | Checker::Or(items) => items.iter().any(has_probability),
        _ => false,
    }
}

fn complete(checker: Option<&Checker>) -> bool {
    matches!(checker, Some(Checker::RangeComplete))
}

fn luck_missions(missions: &[i64]) -> bool {
    // The verified JP factory resolves target 56 (skillTargetType 5, mission 2) to [2]. Empty
    // missions are the native unrestricted variant, equivalent in this non-overlapping Luck domain.
    missions.iter().all(|&mission| mission == M_LUCK)
}

pub(super) fn reads_life(checker: &Checker) -> bool {
    checker.any(&|c| {
        matches!(c, Checker::LifeGreater(_) | Checker::LifeAtLeast(_) | Checker::LifeLess(_) | Checker::LifeAtMost(_))
    })
}

/// The plan of the lottery condition skills `cond` with effect rows `rows` ([`lottery_conditions`]): their actions,
/// the score probes and the pairs of performers whose actions in one phase differ.
pub(super) fn compile<M: Mass>(
    master: &Master,
    cond: &[CondSkill],
    rows: &[EffectRow],
    skills: &LuckSkills,
    probes: Option<&[Option<usize>]>,
    collect_moments: bool,
) -> Result<Plan<M>, Error> {
    let mut plan = Plan::default();
    // Per phase: the performer and the actions of each condition skill that holds some.
    let mut phase_actions: [Vec<(usize, Vec<String>)>; 2] = [Vec::new(), Vec::new()];
    for condition in cond {
        let first = plan.actions.len();
        for (effect_index, effect) in condition.updater.effects().iter().enumerate() {
            let row = &rows[effect.row];
            let fail = |why| unsupported(row, why);
            if !matches!(effect.phase, 1 | 2) {
                return Err(fail("effect phase is outside the native two phases"));
            }
            let release = condition
                .updater
                .updaters
                .iter()
                .find(|updater| updater.effect == effect_index)
                .and_then(|updater| updater.release.as_ref());
            let source = if condition.skill_type == SKILL_TYPE_GEKISOU {
                LuckSource::Gekisou
            } else {
                LuckSource::GekisouSupport
            };
            let native = source
                .rows(master)
                .iter()
                .find(|native| native.id == row.id)
                .ok_or_else(|| Error::Master(format!("LUCK DP cannot find effect row {}", row.id)))?;
            let mission = match source {
                LuckSource::Gekisou => master.gekisou_skill(native.skill_id).map(|skill| skill.gekisou_mission_type),
                LuckSource::GekisouSupport => {
                    master.gekisou_support_skill(native.skill_id).map(|skill| skill.gekisou_mission_type)
                }
            };
            if mission != Some(M_LUCK) {
                return Err(fail("only Luck mission mechanisms and score probes are supported"));
            }
            if effect.cumulative.is_some() {
                return Err(fail("a cumulative lottery mechanism"));
            }
            let probability = match effect.condition.as_ref() {
                Some(checker) => chance::<f64>(checker, 0)
                    .ok_or_else(|| fail("a condition outside deterministic-life/independent-probability predicates"))?,
                None => 1.0,
            };
            let mass = match effect.condition.as_ref() {
                Some(checker) => chance::<M>(checker, 0)
                    .ok_or_else(|| fail("a condition outside deterministic-life/independent-probability predicates"))?,
                None => M::ONE,
            };
            let start = matches!(effect.trigger.as_ref(), Some(Checker::RangeStart { missions, .. }) if luck_missions(missions));
            let playing = matches!(effect.trigger.as_ref(), Some(Checker::RangePlaying { missions, .. }) if luck_missions(missions));
            let miss = matches!(effect.trigger.as_ref(), Some(Checker::LuckLotResult { target: 0, .. }));
            let critical = matches!(effect.trigger.as_ref(), Some(Checker::LuckLotResult { target: 3, .. }));
            let rush = matches!(effect.trigger.as_ref(), Some(Checker::LuckRushPlaying(_)));
            if row.effect_type == 11002 && !collect_moments {
                continue;
            }
            match row.effect_type {
                11001 => {
                    let timed = start && effect.trigger_type == ONE_SHOT && effect.act > 0.0 && release.is_none();
                    let sustained = playing
                        && effect.trigger_type == SUSTAINED
                        && effect.act == 0.0
                        && (release.is_none() || complete(release));
                    if !(timed || sustained)
                        || !matches!(probability, 0.0 | 1.0)
                        || effect.condition.as_ref().is_some_and(has_probability)
                        || effect.condition.as_ref().is_some_and(|checker| fixed_condition(checker).is_none())
                        || effect.execute_limit != 0
                        || effect.reset.is_some()
                    {
                        return Err(fail("an unknown gauge-speed shape"));
                    }
                    // The native recorder installs and ends these deterministic binary32 commands.
                }
                11003 if start => {
                    if effect.trigger_type != ONE_SHOT
                        || effect.act != 0.0
                        || !complete(release)
                        || effect.execute_limit != 0
                        || effect.reset.is_some()
                    {
                        return Err(fail("an unknown range-start gauge shape"));
                    }
                    plan.actions.push((
                        effect.phase,
                        Action::StartGauge { value: row.effect_value, chance: mass },
                        effect.condition.clone(),
                    ));
                }
                11005 if start => {
                    if effect.trigger_type != ONE_SHOT
                        || effect.act != 0.0
                        || !complete(release)
                        || row.effect_limit_count != 1
                        || effect.execute_limit != 0
                        || effect.reset.is_some()
                        || !(2..=4).contains(&row.effect_value)
                    {
                        return Err(fail("only one-draw range-start guarantees are supported"));
                    }
                    plan.actions.push((
                        effect.phase,
                        Action::StartMinimum { result: (row.effect_value - 1) as i8, chance: mass },
                        effect.condition.clone(),
                    ));
                }
                11003 if miss => {
                    if effect.trigger_type != ONE_SHOT
                        || effect.act != 0.0
                        || !complete(release)
                        || effect.execute_limit != 1
                        || !complete(effect.reset.as_ref())
                        || !matches!(probability, 0.0 | 1.0)
                        || effect.condition.as_ref().is_some_and(|checker| fixed_condition(checker).is_none())
                    {
                        return Err(fail("an unknown once-per-range Miss gauge shape"));
                    }
                    if probability == 1.0 {
                        plan.actions.push((effect.phase, Action::MissGauge { value: row.effect_value }, None));
                    }
                }
                11002 if start => {
                    if effect.trigger_type != ONE_SHOT
                        || effect.act != 0.0
                        || effect.execute_limit != 0
                        || effect.reset.is_some()
                        || !(release.is_none() || complete(release))
                    {
                        return Err(fail("an unknown range-start bonus-point shape"));
                    }
                    plan.actions.push((
                        effect.phase,
                        Action::StartPoints { value: row.effect_value as i32, chance: mass },
                        effect.condition.clone(),
                    ));
                }
                11002 if critical => {
                    if effect.trigger_type != ONE_SHOT
                        || effect.act != 0.0
                        || release.is_some()
                        || effect.execute_limit != 0
                        || effect.reset.is_some()
                        || !matches!(probability, 0.0 | 1.0)
                        || effect.condition.as_ref().is_some_and(|checker| fixed_condition(checker).is_none())
                    {
                        return Err(fail("an unknown Critical bonus-point shape"));
                    }
                    plan.actions.push((
                        effect.phase,
                        Action::CriticalPoints { value: row.effect_value as i32, chance: mass },
                        effect.condition.clone(),
                    ));
                }
                2000 | 2005 if rush => {
                    if effect.trigger_type != SUSTAINED
                        || effect.act != 0.0
                        || release.is_some()
                        || effect.execute_limit != 0
                        || effect.reset.is_some()
                        || !matches!(probability, 0.0 | 1.0)
                        || effect.condition.as_ref().is_some_and(|checker| fixed_condition(checker).is_none())
                    {
                        return Err(fail("only untimed sustained direct Rush score probes are supported"));
                    }
                }
                _ => return Err(fail("an unknown lottery or score-probe shape")),
            }
        }
        for phase in 1..=2 {
            let actions: Vec<String> = plan.actions[first..]
                .iter()
                .filter(|action| action.0 == phase)
                .map(|(_, action, checker)| format!("{:?}/{checker:?}", action_words(action)))
                .collect();
            if !actions.is_empty() {
                phase_actions[(phase - 1) as usize].push((condition.member, actions));
            }
        }
    }
    for held in &phase_actions {
        for (index, (member, actions)) in held.iter().enumerate() {
            for (other, other_actions) in &held[..index] {
                if member != other && actions != other_actions && *member.max(other) < PAIR_POSITIONS {
                    plan.order_pairs |= pairs_with(*member, 1 << other);
                }
            }
        }
    }
    // Stable phase ordering matches the interpreter: all updater checks, then appliers in list/row order,
    // separately for phase 1 and phase 2. Rows within one condition skill are already sorted by effect id.
    plan.actions.sort_by_key(|action| action.0);
    let rows = luck_score_rows(cond, rows, skills);
    for shape in 0..skills.shapes.len() {
        let member = match probes {
            Some(probes) => probes[shape],
            None => rows.iter().find(|row| row.shape == shape && row.may_hold).map(|row| row.member),
        };
        if let Some(member) = member
            && !rows.iter().any(|row| row.shape == shape && row.member == member && row.may_hold)
        {
            return Err(Error::Input(format!("LUCK DP: position {member} holds no score-up of shape {shape}")));
        }
        plan.probes.push(member.is_some());
    }
    Ok(plan)
}
