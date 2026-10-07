//! A narrow identity for complete, uniform-order score laws.
//!
//! Unlike a reduced lottery program this retains every ordinary and Gekisou source. Character is erased
//! only after every selected native condition/cumulative/formation reader is proved independent of it.
use super::{CondRow, Master, Performer};

/// An optional full-score program multiset identity, for one immutable request and its exact power.
/// This proves equality of the score law averaged over all original performance orders. It does not
/// identify physical decks or their per-order labels. The caller keeps exact power, the immutable request
/// and payoff mapping separately scoped, and keeps its original identity when this admission declines.
#[doc(hidden)]
pub fn character_blind_uniform_score_identity(master: &Master, performers: &[Performer; 5]) -> Option<Vec<u8>> {
    for performer in performers {
        if let Some((id, level)) = performer.live_skill {
            master.live_skill(id)?;
            let mut found = false;
            for row in master.live_skill_effects.iter().filter(|row| row.live_skill_id == id && row.level == level) {
                found = true;
                if !effect(row.skill_effect_type)
                    || !targets(master, &row.skill_target_ids)
                    || !cumulative(master, row.skill_cumulative_condition_id)
                    || ![
                        row.skill_condition_group,
                        row.skill_release_condition_group,
                        row.effect_execute_limit_reset_condition_group,
                    ]
                    .into_iter()
                    .all(|id| group(master, id))
                {
                    return None;
                }
            }
            if !found {
                return None;
            }
        }
        for &(id, level) in &performer.support_skills {
            let mut found = false;
            for row in
                master.support_skill_effects.iter().filter(|row| row.support_skill_id == id && row.level == level)
            {
                found = true;
                if !condition_row(master, row.into()) {
                    return None;
                }
            }
            if !found {
                return None;
            }
        }
        if let Some((id, level)) = performer.gekisou_skill {
            master.gekisou_skill(id)?;
            let mut found = false;
            for row in master.gekisou_skill_effects.iter().filter(|row| row.skill_id == id && row.level == level) {
                found = true;
                if !condition_row(master, row.into()) {
                    return None;
                }
            }
            if !found {
                return None;
            }
        }
        for &(id, level) in &performer.gekisou_support_skills {
            master.gekisou_support_skill(id)?;
            let mut found = false;
            for row in
                master.gekisou_support_skill_effects.iter().filter(|row| row.skill_id == id && row.level == level)
            {
                found = true;
                if !condition_row(master, row.into()) {
                    return None;
                }
            }
            if !found {
                return None;
            }
        }
    }
    let mut keys = Vec::new();
    keys.try_reserve_exact(5).ok()?;
    let mut bytes = 0usize;
    for performer in performers {
        let mut normalized = performer.clone();
        normalized.character_id = 0;
        // Preserve every other field, every source ID/level and the order within both source vectors.
        let key = super::luck_exact::state_identity(&normalized)?;
        bytes = bytes.checked_add(key.len())?;
        if bytes > 256 * 1024 {
            return None;
        }
        keys.push(key);
    }
    keys.sort_unstable();
    Some(format!("uniform120/full-score-character-unread/v1/{keys:?}").into_bytes())
}

fn targets(master: &Master, ids: &[i64]) -> bool {
    ids.iter().all(|&id| master.skill_target(id).is_some_and(|target| target.character_id <= 0))
}

fn effect(kind: i64) -> bool {
    // Closed native applier set. These read compiled rows, owner positions and controller state, never a
    // raw character. Unknown/new appliers require a separate review before sharing this capability.
    matches!(kind, 0 | 2000..=2005 | 3000..=3004 | 4004 | 11000..=11005
        | 12000 | 12002..=12004 | 12006 | 13000 | 13002..=13005 | 15000)
}

fn condition_row(master: &Master, row: CondRow<'_>) -> bool {
    matches!(row.trigger_type, 1 | 2)
        && effect(row.effect_type)
        && targets(master, row.target_ids)
        && cumulative(master, row.cumulative_id)
        && [row.trigger_group, row.condition_group, row.release_group, row.reset_group]
            .into_iter()
            .all(|id| group(master, id))
}

fn group(master: &Master, id: i64) -> bool {
    if id == 0 {
        return true;
    }
    let mut found = false;
    for set in master.skill_condition_sets.iter().filter(|set| set.group == id) {
        found = true;
        for &id in &set.condition_ids {
            let Some(row) = master.skill_condition(id) else { return false };
            // Factory::one: only 3000/3001/5000 match member attributes; the same targets also cover
            // luck_shapes' formation predicates. Keep every other known condition in a closed list.
            if !matches!(row.condition_type, 0 | 1000 | 1010 | 1020 | 1030 | 1040 | 2000..=2004
                | 3000 | 3001 | 4000..=4002 | 4007..=4009 | 4010..=4012 | 5000 | 5020 | 5021
                | 7000..=7007 | 7010..=7013 | 7020 | 7021 | 8000)
                || !targets(master, &row.condition_target_ids)
            {
                return false;
            }
        }
    }
    found
}

fn cumulative(master: &Master, id: i64) -> bool {
    if id == 0 {
        return true;
    }
    let Some(row) = master.cumulative_condition(id) else { return false };
    // Factory::cumulative: 3000/3001 use target_matches; the remaining cases use only the preserved
    // band/type fields, immutable time/life or judgement counters. No future primitive is admitted.
    matches!(row.condition_type, 1000..=1002 | 2000 | 2001 | 3000..=3005 | 6000 | 7000 | 7001)
        && targets(master, &row.condition_target_ids)
}
