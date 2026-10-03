//! Leader plus unordered nonleader compositions, followed by all physical layouts
//! and a unique-Snap branch-and-bound. No skill order is selected by this traversal.
use super::{Engine, Error, PhysicalDeck, slot};
use crate::{
    domain::CandidateDomain,
    search::joint::{JointBounds, SLOTS},
};
#[path = "composition/classes.rs"]
mod classes;

pub(super) fn solve(
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    let candidates = bounds.member_order(domain);
    let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    e.rec.clock.lap(slot::COMPOSITION);
    for (index, &leader) in candidates.iter().enumerate() {
        if e.expired() {
            unexplored_members(0, index, &mut p, &candidates, domain, bounds, orders, e)?;
            return Ok(false);
        }
        if domain.leader().is_some_and(|m| m != leader)
            || domain
                .required()
                .iter()
                .any(|&m| m != leader && e.pool.members[m].character_id == e.pool.members[leader].character_id)
        {
            continue;
        }
        p.members[2] = leader;
        e.rec.frontier.set(0, index, candidates.len());
        if !members(1, 0, &mut p, &candidates, domain, bounds, orders, e)? {
            unexplored_members(0, index + 1, &mut p, &candidates, domain, bounds, orders, e)?;
            return Ok(false);
        }
    }
    Ok(true)
}
fn inferior(cap: i128, power: i64, p: Option<(&PhysicalDeck, usize)>, e: &Engine<'_, '_>) -> bool {
    if e.top.len() != e.request.k {
        return false;
    }
    let kth = e.top.last().expect("K incumbents");
    let threshold = kth.evaluation.expected_payoff.numerator;
    if cap != threshold {
        return cap < threshold;
    }
    if power != i64::from(kth.power) {
        return power < i64::from(kth.power);
    }
    if let Some((p, depth)) = p {
        let members = p.members.map(|m| e.pool.members[m].id);
        if members != kth.members {
            return members > kth.members;
        }
        // None is legal in every unassigned slot and is the smallest public Snap key.
        let mut snaps = [None; 5];
        for &slot in &SLOTS[..depth] {
            snaps[slot] = p.snaps[slot].map(|s| e.pool.snaps[s].id);
        }
        return snaps > kth.snaps;
    }
    false
}

/// The stop leaves the member choices `candidates[from..]` at `depth` unexplored (leaders at depth 0): bound each
/// by its child's composition bound, the bound the traversal itself checks there.
#[allow(clippy::too_many_arguments)]
fn unexplored_members(
    depth: usize,
    from: usize,
    p: &mut PhysicalDeck,
    candidates: &[usize],
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<(), Error> {
    if e.stop.is_none() {
        return Ok(());
    }
    let started = e.bound_start();
    let mut upper: Option<i128> = None;
    for &m in &candidates[from.min(candidates.len())..] {
        let character = e.pool.members[m].character_id;
        if (depth == 0 && domain.leader().is_some_and(|l| l != m))
            || SLOTS[..depth].iter().any(|&s| e.pool.members[p.members[s]].character_id == character)
            || domain.required().iter().any(|&r| r != m && e.pool.members[r].character_id == character)
        {
            continue;
        }
        p.members[SLOTS[depth]] = m;
        let (cap, _) = bounds.composition_expected_upper(e.pool, domain, p, depth + 1, 0, true, orders)?;
        upper = Some(upper.map_or(cap, |u| u.max(cap)));
    }
    e.fold_unexplored(upper, started);
    Ok(())
}

/// The stop leaves this composition node's whole subtree unexplored.
fn unexplored_node(
    depth: usize,
    p: &PhysicalDeck,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<(), Error> {
    if e.stop.is_none() {
        return Ok(());
    }
    let started = e.bound_start();
    let (cap, _) = bounds.composition_expected_upper(e.pool, domain, p, depth, 0, true, orders)?;
    e.fold_unexplored(Some(cap), started);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn members(
    depth: usize,
    start: usize,
    p: &mut PhysicalDeck,
    candidates: &[usize],
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    e.tel.composition.member_nodes[depth] += 1;
    if e.expired() {
        unexplored_node(depth, p, domain, bounds, orders, e)?;
        return Ok(false);
    }
    let missing: Vec<_> =
        domain.required().iter().filter(|m| !SLOTS[..depth].iter().any(|&s| p.members[s] == **m)).collect();
    if missing.len() > 5 - depth || missing.iter().any(|m| !candidates[start..].contains(m)) {
        return Ok(true);
    }
    if e.top.len() == e.request.k {
        e.tel.composition.composition.checks += 1;
        let (cap, power) = bounds.composition_expected_upper(e.pool, domain, p, depth, 0, true, orders)?;
        if inferior(cap, power, None, e) {
            e.tel.composition.composition.pruned += 1;
            return Ok(true);
        }
    }
    if depth == 5 {
        e.tel.composition.compositions += 1;
        let mut seeds = std::collections::HashSet::new();
        let more = (!bounds.uses_class_search() || seed_layouts(1, p, &mut seeds, domain, bounds, orders, e)?)
            && layouts(1, p, &seeds, domain, bounds, orders, e)?;
        if !more {
            unexplored_node(depth, p, domain, bounds, orders, e)?;
        }
        return Ok(more);
    }
    for (index, &m) in candidates.iter().enumerate().skip(start) {
        let character = e.pool.members[m].character_id;
        if SLOTS[..depth].iter().any(|&s| e.pool.members[p.members[s]].character_id == character)
            || domain.required().iter().any(|&r| r != m && e.pool.members[r].character_id == character)
        {
            continue;
        }
        p.members[SLOTS[depth]] = m;
        e.rec.frontier.set(depth, index - start, candidates.len() - start);
        if !members(depth + 1, index + 1, p, candidates, domain, bounds, orders, e)? {
            unexplored_members(depth, index + 1, p, candidates, domain, bounds, orders, e)?;
            return Ok(false);
        }
    }
    Ok(true)
}
#[allow(clippy::too_many_arguments)]
fn seed_layouts(
    depth: usize,
    p: &mut PhysicalDeck,
    seen: &mut std::collections::HashSet<PhysicalDeck>,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    if e.expired() {
        return Ok(false);
    }
    if depth == 5 {
        p.snaps = [None; 5];
        let (cap, power) = bounds.composition_expected_upper(e.pool, domain, p, 5, 0, false, orders)?;
        if inferior(cap, power, Some((p, 0)), e) {
            return Ok(true);
        }
        let (_, first) = bounds.layout_power(domain, p, 0);
        for proposal in std::iter::once(first).chain(bounds.weighted_layout_seeds(domain, p, orders)) {
            if seen.contains(&proposal) {
                continue;
            }
            e.tel.composition.seeds.preseed += 1;
            if !e.consider(proposal)? {
                return Ok(false);
            }
            seen.insert(proposal);
        }
        return Ok(true);
    }
    for next in depth..5 {
        p.members.swap(SLOTS[depth], SLOTS[next]);
        let more = seed_layouts(depth + 1, p, seen, domain, bounds, orders, e)?;
        p.members.swap(SLOTS[depth], SLOTS[next]);
        if !more {
            return Ok(false);
        }
    }
    Ok(true)
}
fn layouts(
    depth: usize,
    p: &mut PhysicalDeck,
    seeds: &std::collections::HashSet<PhysicalDeck>,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    if e.expired() {
        return Ok(false);
    }
    if depth == 5 {
        p.snaps = [None; 5];
        let mut evaluated: std::collections::HashSet<_> =
            seeds.iter().filter(|d| d.members == p.members).map(|d| d.snaps).collect();
        e.tel.composition.layout.checks += 1;
        let (cap, power) = bounds.composition_expected_upper(e.pool, domain, p, 5, 0, false, orders)?;
        if inferior(cap, power, Some((p, 0)), e) {
            e.tel.composition.layout.pruned += 1;
            return Ok(true);
        }
        let (_, proposal) = bounds.layout_power(domain, p, 0);
        if !evaluated.contains(&proposal.snaps) {
            e.tel.composition.seeds.layout += 1;
            if !e.consider(proposal)? {
                return Ok(false);
            }
            evaluated.insert(proposal.snaps);
        }
        if domain.snaps().is_empty() {
            e.tel.composition.power_frontier_closed += 1;
            return Ok(true);
        }
        if bounds.uses_class_search() {
            for proposal in bounds.weighted_layout_seeds(domain, p, orders) {
                if evaluated.contains(&proposal.snaps) {
                    continue;
                }
                e.tel.composition.seeds.weighted += 1;
                if !e.consider(proposal)? {
                    return Ok(false);
                }
                evaluated.insert(proposal.snaps);
            }
            return classes::solve(p, &evaluated, domain, bounds, orders, e);
        }
        // Pay for the Top-K assignment DP only when the best-power seed actually
        // attains the layout's primary cap. Skipping it merely retains full DFS.
        let cap_attained =
            e.top.iter().any(|entry| entry.physical == proposal && entry.evaluation.expected_payoff.numerator == cap);
        if bounds.is_pt() && cap_attained {
            let proposals = bounds.layout_power_frontier(domain, p, e.request.k);
            let exhausted = proposals.len() < e.request.k;
            let last = proposals.last().copied();
            for proposal in proposals {
                if evaluated.contains(&proposal.snaps) {
                    continue;
                }
                e.tel.composition.seeds.power_frontier += 1;
                if !e.consider(proposal)? {
                    return Ok(false);
                }
                evaluated.insert(proposal.snaps);
            }
            if exhausted {
                e.tel.composition.power_frontier_closed += 1;
                return Ok(true);
            }
            if let Some(last) = last {
                let power = i64::from(e.pool.deck_power(&last.as_deck(), e.song, e.event)?.power());
                let equal = e.top.len() == e.request.k
                    && e.top.last().is_some_and(|kth| {
                        cap == kth.evaluation.expected_payoff.numerator
                            && power == i64::from(kth.power)
                            && last.members.map(|m| e.pool.members[m].id) == kth.members
                            && last.snaps.map(|s| s.map(|s| e.pool.snaps[s].id)) == kth.snaps
                    });
                // All unexamined bindings rank strictly after the last power-ranked
                // proposal. Equal cutoff identity was already evaluated above.
                if equal || inferior(cap, power, Some((&last, 5)), e) {
                    e.tel.composition.power_frontier_closed += 1;
                    return Ok(true);
                }
            }
        }
        let choices = std::array::from_fn(|slot| bounds.slot_choices(domain, p, slot, orders));
        return snaps(0, p, &choices, &evaluated, domain, bounds, orders, e);
    }
    for next in depth..5 {
        p.members.swap(SLOTS[depth], SLOTS[next]);
        let more = layouts(depth + 1, p, seeds, domain, bounds, orders, e)?;
        p.members.swap(SLOTS[depth], SLOTS[next]);
        if !more {
            return Ok(false);
        }
    }
    Ok(true)
}
#[allow(clippy::too_many_arguments)]
fn snaps(
    depth: usize,
    p: &mut PhysicalDeck,
    choices: &[Vec<usize>; 5],
    evaluated: &std::collections::HashSet<[Option<usize>; 5]>,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    e.tel.composition.snap_nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if depth == 5 && evaluated.contains(&p.snaps) {
        return Ok(true);
    }
    if e.top.len() == e.request.k {
        e.tel.composition.layout.checks += 1;
        let (cap, power) = bounds.composition_expected_upper(e.pool, domain, p, 5, depth, false, orders)?;
        if inferior(cap, power, Some((p, depth)), e) {
            e.tel.composition.layout.pruned += 1;
            return Ok(true);
        }
        if depth == 5 && bounds.has_fine() {
            let (_, resume) = e.rec.clock.lap(slot::FINE);
            let fine = bounds.fine_expected_upper(domain, p, power, orders, &mut e.bound_scratch, None)?;
            e.rec.clock.lap(resume);
            if let Some(fine) = fine {
                e.tel.composition.fine.checks += 1;
                if inferior(fine, power, Some((p, depth)), e) {
                    e.tel.composition.fine.pruned += 1;
                    return Ok(true);
                }
            }
        }
    }
    if depth == 5 {
        return e.consider(*p);
    }
    let slot = SLOTS[depth];
    for &choice in &choices[slot] {
        let snap = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
        if snap.is_some() && SLOTS[..depth].iter().any(|&s| p.snaps[s] == snap) {
            continue;
        }
        p.snaps[slot] = snap;
        if !snaps(depth + 1, p, choices, evaluated, domain, bounds, orders, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}
