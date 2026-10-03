//! Snaps whose skills can convert judgements. Without them a domain keeps the raw judgement closure.
use super::*;

/// Pool Snap indexes among `snaps` with a support or Gekisou support row of a judgement conversion type.
/// This only partitions the physical domain and both parts are searched, so the split is always complete.
pub(crate) fn conversion_snaps(pool: &Pool, snaps: &[usize]) -> Result<Vec<usize>, Error> {
    let master = pool.master;
    let mut out = Vec::new();
    for &s in snaps {
        let v = &pool.snaps[s];
        let mut converts = false;
        for (id, level) in v.support_skills()? {
            converts |= master
                .support_skill_effects
                .iter()
                .any(|r| r.support_skill_id == id && r.level == level && is_conversion(r.skill_effect_type));
        }
        for (id, level) in v.gekisou_support_skills()? {
            converts |= master
                .gekisou_support_skill_effects
                .iter()
                .any(|r| r.skill_id == id && r.level == level && is_conversion(r.skill_effect_type));
        }
        if converts {
            out.push(s);
        }
    }
    Ok(out)
}

fn is_conversion(effect_type: i64) -> bool {
    matches!(effect_type, 12006 | 13005)
}
