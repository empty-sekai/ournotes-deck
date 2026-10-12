//! Checks that the native updater state identities of a formation are distinct.

use super::*;

fn state_identity_error() -> Error {
    Error::Unsupported("LUCK certificates require distinct effect state identities".into())
}

/// The dependency reduction preserves applier state only when held effects have disjoint native keys.
/// Each executable condition effect has a pool containing index zero, so a repeated wrapped effect id
/// already proves that the two pools overlap. Live and condition keys occupy distinct namespaces.
pub(super) fn check_held_state_identities(master: &Master, deck: &[Performer]) -> Result<(), Error> {
    let mut seen = FxHashSet::default();
    let mut insert_condition = |id: i64, kind: i64, member: usize, trigger: i64| {
        !matches!(trigger, ONE_SHOT | SUSTAINED)
            || seen.insert(StateKey::Cond {
                effect_id: id.wrapping_mul(100).wrapping_add(kind * 10).wrapping_add(member as i64),
                index: 0,
            })
    };
    for (member, performer) in deck.iter().enumerate() {
        for &(id, level) in &performer.support_skills {
            if !master
                .support_skill_effects
                .iter()
                .filter(|row| row.support_skill_id == id && row.level == level)
                .all(|row| insert_condition(row.id, SKILL_TYPE_SUPPORT, member, row.skill_trigger_type))
            {
                return Err(state_identity_error());
            }
        }
        let Some((id, level)) = performer.gekisou_skill else { continue };
        if !master
            .gekisou_skill_effects
            .iter()
            .filter(|row| row.skill_id == id && row.level == level)
            .all(|row| insert_condition(row.id, SKILL_TYPE_GEKISOU, member, row.skill_trigger_type))
        {
            return Err(state_identity_error());
        }
        for &(id, level) in &performer.gekisou_support_skills {
            if !master
                .gekisou_support_skill_effects
                .iter()
                .filter(|row| row.skill_id == id && row.level == level)
                .all(|row| insert_condition(row.id, SKILL_TYPE_GEKISOU_SUPPORT, member, row.skill_trigger_type))
            {
                return Err(state_identity_error());
            }
        }
    }
    for (member, performer) in deck.iter().enumerate() {
        let Some((id, level)) = performer.live_skill else { continue };
        if !master
            .live_skill_effects
            .iter()
            .filter(|row| row.live_skill_id == id && row.level == level)
            .all(|row| seen.insert(StateKey::Live { row_id: row.id, member, index: 0 }))
        {
            return Err(state_identity_error());
        }
    }
    Ok(())
}

/// Check the actual initialized updater pools, including their native indices and wrapped ids.
pub(crate) fn check_model_state_identities(model: &LiveModel) -> Result<(), Error> {
    let mut seen = FxHashSet::default();
    for skill in &model.live {
        for effect in &skill.effects {
            if !seen.insert(StateKey::Live {
                row_id: model.rows[effect.row].id,
                member: skill.member,
                index: skill.index,
            }) {
                return Err(state_identity_error());
            }
        }
    }
    for skill in &model.cond {
        for updater in &skill.updater.updaters {
            if !seen.insert(StateKey::Cond {
                effect_id: skill.updater.effect(updater.effect).effect_id,
                index: updater.index,
            }) {
                return Err(state_identity_error());
            }
        }
    }
    Ok(())
}
