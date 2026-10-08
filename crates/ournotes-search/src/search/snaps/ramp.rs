//! Reachable cumulative score factors of one-shot Gekisou executions.
use super::*;

pub(super) type RampKey = (GkWindowKey, i64, i64, i64);
/// Each inner vector belongs to one possible lifetime execution, not to a recycled pool slot. Its values are
/// alternatives of the same updater; different executions may overlap in chart time and must still be added.
pub(super) type RampWindows = Rc<Vec<Vec<(i64, i64, i64)>>>;

pub(super) fn windows(env: &Env, r: &Row, event_bound: bool) -> Option<RampWindows> {
    if !r.gk || event_bound || r.effect_type != 2001 || r.trigger_type != 1 || r.value <= 0 {
        return None;
    }
    let key = ((r.trigger, r.trigger_type, r.gate, r.act.to_bits(), r.release), r.cumulative, r.value, r.max_value);
    if let Some(v) = env.ramp_cache.borrow().get(&key) {
        return v.clone();
    }
    let result = compile(env, r).map(Rc::new);
    env.ramp_cache.borrow_mut().insert(key, result.clone());
    result
}

fn compile(env: &Env, r: &Row) -> Option<Vec<Vec<(i64, i64, i64)>>> {
    let g = env.gkf.as_ref()?;
    let c = env.master.cumulative_condition(r.cumulative)?;
    if !(1000..=1002).contains(&c.condition_type) || g.ent.len() > i32::MAX as usize {
        return None;
    }
    let unit = *c.condition_values.first()?;
    if unit < 1 {
        return None;
    }
    let max_count = if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count };
    let max_product = (r.value as i128) * (max_count as i128);
    if max_product > i32::MAX as i128 {
        return None;
    }
    let ceiling = if r.max_value > 0 { r.max_value.min(max_product as i64) } else { max_product as i64 };
    let targets: Vec<i64> = c
        .condition_target_ids
        .iter()
        .map(|&id| env.master.skill_target(id).map(|t| t.judgement))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .filter(|&j| j != -1)
        .collect();
    let mut prefix = vec![0i64; g.times.len() + 1];
    for (i, &(frame, raw)) in g.ent.iter().enumerate() {
        if !(0..8).contains(&raw) {
            return None;
        }
        let reach = env.reach_of(i, raw);
        let hit = (0..8).any(|j| {
            reach & (1u8 << j) != 0
                && targets.iter().any(|&t| match c.condition_type {
                    1001 => j as i64 >= t,
                    1002 => j as i64 <= t,
                    _ => j as i64 == t,
                })
        });
        // The native cumulative checker uses any(target), not one increment per duplicate
        // target, over the entry's reachable final judgements. Ignore gates and other
        // condition failures after start.
        prefix[frame + 1] += i64::from(hit);
    }
    for f in 0..g.times.len() {
        prefix[f + 1] += prefix[f];
    }
    let (starts, possible) = g.starts(env, r);
    // The existing timing envelope uses one execution per possible start here.
    // Its pooled many-start fallback remains unchanged outside this domain.
    if starts.len() > 8 {
        return None;
    }
    let ends = g.ends(env, r, &starts, &possible);
    let mut out = Vec::new();
    for (&start, end) in starts.iter().zip(ends) {
        out.push(execution(&g.times, &prefix, start, g.wlo[start], end.0, unit, max_count, r.value, ceiling));
    }
    Some(out)
}

/// A new one-shot updater starts at zero and is reset before returning to its
/// available stack. It counts at most once per processing frame. Its initial
/// factor may be backdated; replacements are filed at their current frame time.
#[allow(clippy::too_many_arguments)]
fn execution(
    frames: &[i32],
    prefix: &[i64],
    start: usize,
    begin: i64,
    end: i64,
    unit: i64,
    max_count: i64,
    step: i64,
    ceiling: i64,
) -> Vec<(i64, i64, i64)> {
    let mut out: Vec<(i64, i64, i64)> = Vec::new();
    let mut a = begin;
    for f in start..frames.len() {
        if a >= end {
            break;
        }
        let count = ournotes_sim::num::floor_to_i32((prefix[f + 1] - prefix[start]) as f32 / unit as f32) as i64;
        let value = (step * count.min(max_count)).min(ceiling);
        let b = if value >= ceiling { end } else { frames.get(f + 1).map_or(end, |&t| end.min(t as i64)) };
        if a < b {
            if let Some(last) = out.last_mut().filter(|last| last.1 == a && last.2 == value) {
                last.1 = b;
            } else {
                out.push((a, b, value));
            }
        }
        a = b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_count_is_backdated_then_only_grows_at_update_times() {
        let frames = [0, 40, 80, 120, 160];
        let prefix = [0, 1, 3, 6, 6, 10];
        let actual = execution(&frames, &prefix, 1, 10, 200, 1, 100, 3, 12);
        assert_eq!(actual, vec![(10, 80, 6), (80, 200, 12)]);
    }
    #[test]
    fn new_execution_excludes_previous_counts_and_respects_unit_and_max() {
        // Prefix entries represent already deduplicated target matches; each
        // execution has a fresh baseline even when another execution came before.
        let frames = [0, 40, 80, 120, 160];
        let prefix = [0, 4, 9, 10, 12, 15];
        assert_eq!(
            execution(&frames, &prefix, 2, 80, 200, 2, 2, 5, 100),
            vec![(80, 120, 0), (120, 160, 5), (160, 200, 10)]
        );
    }
}
