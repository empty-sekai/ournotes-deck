//! Enumerate bound classes, then recover their individual legal resource bindings.
use super::*;
use std::collections::HashSet;

pub(super) fn solve(
    p: &mut PhysicalDeck,
    evaluated: &HashSet<[Option<usize>; 5]>,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    let groups = std::array::from_fn(|slot| {
        bounds.effect_groups(p.members[slot], &bounds.slot_choices(domain, p, slot, orders))
    });
    let mut allowed = std::array::from_fn(|_| (0..=domain.snaps().len()).collect());
    classes(0, p, &groups, &mut allowed, evaluated, domain, bounds, orders, e)
}
#[allow(clippy::too_many_arguments)]
fn classes(
    depth: usize,
    p: &mut PhysicalDeck,
    groups: &[Vec<Vec<usize>>; 5],
    allowed: &mut [Vec<usize>; 5],
    evaluated: &HashSet<[Option<usize>; 5]>,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    e.tel.composition.class_nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    e.tel.composition.class.checks += 1;
    let Some(bound) = bounds.class_bound(domain, p, allowed, orders, depth == 5, &mut e.bound_scratch)? else {
        e.tel.composition.class_infeasible += 1;
        return Ok(true);
    };
    e.tel.composition.class_resource_checks += bound.resource_checks;
    e.tel.composition.class_resource_tightened += bound.resource_tightened;
    if inferior(bound.payoff, bound.power, Some((p, 0)), e) {
        e.tel.composition.class.pruned += 1;
        return Ok(true);
    }
    if depth == 5 {
        let mut known = evaluated.clone();
        if !known.contains(&bound.proposal.snaps) {
            e.tel.composition.seeds.class += 1;
            if !e.consider(bound.proposal)? {
                return Ok(false);
            }
            known.insert(bound.proposal.snaps);
        }
        p.snaps = [None; 5];
        return bindings(0, p, allowed, &known, domain, bounds, orders, e);
    }
    let slot = SLOTS[depth];
    let original = allowed[slot].clone();
    for group in &groups[slot] {
        allowed[slot] = group.clone();
        let more = classes(depth + 1, p, groups, allowed, evaluated, domain, bounds, orders, e)?;
        if !more {
            allowed[slot] = original;
            return Ok(false);
        }
    }
    allowed[slot] = original;
    Ok(true)
}
#[allow(clippy::too_many_arguments)]
fn bindings(
    depth: usize,
    p: &mut PhysicalDeck,
    allowed: &mut [Vec<usize>; 5],
    evaluated: &HashSet<[Option<usize>; 5]>,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    e.tel.composition.binding_nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if depth == 5 && evaluated.contains(&p.snaps) {
        return Ok(true);
    }
    e.tel.composition.class_binding.checks += 1;
    let Some(bound) = bounds.class_bound(domain, p, allowed, orders, true, &mut e.bound_scratch)? else {
        e.tel.composition.class_infeasible += 1;
        return Ok(true);
    };
    e.tel.composition.class_resource_checks += bound.resource_checks;
    e.tel.composition.class_resource_tightened += bound.resource_tightened;
    if inferior(bound.payoff, bound.power, Some((p, depth)), e) {
        e.tel.composition.class_binding.pruned += 1;
        return Ok(true);
    }
    if depth == 5 {
        return e.consider(*p);
    }
    let slot = SLOTS[depth];
    let original = allowed[slot].clone();
    for &choice in &original {
        let snap = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
        if snap.is_some() && SLOTS[..depth].iter().any(|&s| p.snaps[s] == snap) {
            continue;
        }
        p.snaps[slot] = snap;
        allowed[slot] = vec![choice];
        let more = bindings(depth + 1, p, allowed, evaluated, domain, bounds, orders, e)?;
        if !more {
            allowed[slot] = original;
            return Ok(false);
        }
    }
    allowed[slot] = original;
    Ok(true)
}
