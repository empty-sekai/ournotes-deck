//! The joint member and Snap traversal of the exact search.

use super::*;

/// The best payoff any completion through the depth-0 joint choices from `from` can reach: each choice's depth-1
/// node bound, the bound the traversal itself would check there. None when no legal choice remains.
pub(super) fn joint_root_upper(
    pool: &Pool,
    domain: &crate::domain::CandidateDomain,
    bounds: &crate::search::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    from: usize,
) -> Result<Option<i128>, Error> {
    use crate::search::joint::SLOTS;
    let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    let mut upper = None;
    for &(m, choice) in &bounds.choices[from..] {
        let character = pool.members[m].character_id;
        if domain.leader().is_some_and(|l| l != m)
            || domain.required().iter().any(|&r| r != m && pool.members[r].character_id == character)
            || !bounds.allows(SLOTS[0], choice)
        {
            continue;
        }
        p.members[SLOTS[0]] = m;
        p.snaps[SLOTS[0]] = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
        let (cap, _) = node_upper(pool, domain, bounds, &p, 1, orders)?;
        upper = Some(upper.map_or(cap, |u: i128| u.max(cap)));
    }
    Ok(upper)
}

/// The (payoff, power) bound a joint node at `depth > 0` checks first: the cheap bound of the Gekisou combo carrier
/// level its completions can reach, with the envelope keyed by its placed carriers (the pool-wide cheap bound without
/// carrier levels). It covers every completion of the prefix.
pub(super) fn node_upper(
    pool: &Pool,
    domain: &crate::domain::CandidateDomain,
    bounds: &crate::search::joint::JointBounds,
    p: &PhysicalDeck,
    depth: usize,
    orders: &[([usize; 5], u128)],
) -> Result<(i128, i64), Error> {
    bounds.node_bound(pool, domain, p, depth, orders)
}

/// Gekisou score: partition the physical domain by the Snaps selected from its compiled conversion reach.
/// - none: the sub-domain contains all choices outside the selected set;
/// - exactly one, `c` in physical slot `s`: the sub-domain contains `c` and those other choices, and `s` is forced
///   to `c`;
/// - two or more: split by the first two slots, in search order, that hold selected Snaps. Both are forced to
///   selected Snaps and the other earlier slots exclude them.
///
/// The parts are disjoint and cover the domain. They share one Top-K and its canonical order. A failed part compile
/// falls back to one pool-wide search.
///
/// Certified LUCK score: the Snaps with which a member takes no lottery kind are selected as well, and the bounds of
/// every part whose members take a lottery kind with each of its Snap choices weigh the lottery laws of its teams (see
/// `domain_laws`). Only the bounds of a search in one part hold clusters of those laws: the traversals of the clusters
/// repeat the prefixes their multisets share, once in every part and slot rule.
///
/// Certified LUCK score under the maximum aggregation: every part's bounds are turned into bounds of each order's
/// payoff (`JointBounds::order_maximum`) and traversed with one pseudo-order of mass 1.
pub(super) fn joint_regimes(
    p: &mut PhysicalDeck,
    plan: &crate::handler::ExecutionPlan,
    bounds: &crate::search::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<(), Error> {
    use crate::search::joint::{JointBounds, SLOTS, SlotRules};
    let mut census = e.lottery_census(&plan.domain);
    let mut converting = bounds.conversion_snaps(&plan.domain);
    let conversion = converting.len();
    for s in census.as_ref().map_or_else(Vec::new, |census| census.roles.breaking_snaps()) {
        if !converting.contains(&s) {
            converting.push(s);
        }
    }
    (e.rec.tracked, e.rec.bounded) = (true, true);
    // The traversals hold the census, whose multisets of kinds give the laws of their teams; a domain's census is read
    // before its traversal and handed back for the next one.
    let lend = |e: &mut Engine<'_, '_>, census: &mut Option<super::domain_laws::LotteryCensus>| {
        if let Some(state) = e.certified.as_mut() {
            state.census = census.take();
        }
    };
    if converting.is_empty() {
        // Under the maximum aggregation one traversal of the whole domain's bounds, whose teams' leaves read the
        // bounds of their own laws, outruns the traversals of law clusters, which repeat the shared prefixes.
        let clusters = !certified_maximum(e);
        let mut weighed = census.as_mut().and_then(|census| e.weighed_bounds(census, &plan.domain, clusters));
        if certified_maximum(e) {
            // bounds of each order's payoff, weighing no law when the census reads none
            if weighed.is_none() {
                weighed =
                    JointBounds::compile(e.pool, e.request, &plan.domain, e.metric, e.event_input, e.simulation).ok();
            }
            if let Some(weighed) = &mut weighed {
                weighed.order_maximum(e.pool, &plan.domain);
            }
        }
        let bounds = weighed.as_ref().unwrap_or(bounds);
        let orders = masses(e, bounds, orders);
        if weighed.is_some() {
            e.correlated = bounds.correlation_worthwhile(e.pool, &plan.domain, orders)?;
            e.resource = !bounds.prefers_compositions()
                && bounds.resource_worthwhile(e.pool, &plan.domain, orders, e.correlated)?;
        }
        lend(e, &mut census);
        e.rec.begin(&mut e.tel, "search", None);
        traverse(p, &plan.domain, bounds, orders, e)?;
        e.rec.end(&mut e.tel);
        return Ok(());
    }
    let is_converting = |s: usize| converting.contains(&s);
    let mask = |domain: &crate::domain::CandidateDomain, keep: &dyn Fn(usize) -> bool| -> Vec<bool> {
        std::iter::once(false).chain(domain.snaps().iter().map(|&s| keep(s))).collect()
    };
    type Rules = Vec<(Option<SlotRules>, String)>;
    let mut parts: Vec<(crate::domain::CandidateDomain, Rules)> = Vec::new();
    parts.push((plan.domain.retain_snaps(|s| !is_converting(s)), vec![(None, "free".into())]));
    for &c in &converting {
        let domain = plan.domain.retain_snaps(|s| !is_converting(s) || s == c);
        let only = mask(&domain, &|s| s == c);
        let rules = SLOTS
            .iter()
            .map(|&slot| {
                let mut r = SlotRules::default();
                for &other in &SLOTS {
                    if other == slot {
                        r.forced[other] = Some(only.clone());
                    } else {
                        r.excluded[other] = Some(only.clone());
                    }
                }
                (Some(r), format!("snap {} slot {slot}", e.pool.snaps[c].id))
            })
            .collect();
        parts.push((domain, rules));
    }
    if converting.len() >= 2 {
        let conv = mask(&plan.domain, &is_converting);
        let mut rules = Vec::new();
        for j in 1..5 {
            for i in 0..j {
                let mut r = SlotRules::default();
                r.forced[SLOTS[i]] = Some(conv.clone());
                r.forced[SLOTS[j]] = Some(conv.clone());
                for k in (0..j).filter(|&k| k != i) {
                    r.excluded[SLOTS[k]] = Some(conv.clone());
                }
                rules.push((Some(r), format!("pair slots {},{}", SLOTS[i], SLOTS[j])));
            }
        }
        parts.push((plan.domain.clone(), rules));
    }
    let total = parts.iter().map(|(_, rules)| rules.len() as u64).sum();
    e.tel.environment.bounds.conversion =
        Some(telemetry::Conversion { snaps: conversion, parts: total as usize, ..Default::default() });
    e.rec.parts = total;
    let global_flags = (e.correlated, e.resource);
    // Parts are grouped by their shared envelope. Within a group each slot rule is visited once;
    // the envelope can be released as soon as that group's last traversal is complete.
    let mut priority = std::collections::HashMap::new();
    if !warm::static_order() {
        for t in &e.top {
            let key = warm::traversal_of(&t.physical, &converting);
            priority.entry(key).or_insert((t.evaluation.expected_payoff.numerator, t.power));
        }
    }
    let mut schedule: Vec<_> = parts.into_iter().enumerate().collect();
    schedule.sort_by_key(|(i, (_, rules))| {
        std::cmp::Reverse((0..rules.len()).filter_map(|j| priority.get(&(*i, j)).copied()).max())
    });
    let bound_pending = |e: &mut Engine<'_, '_>| -> Result<(), Error> {
        // The pool-wide bound covers the active group and every group still awaiting compilation.
        let started = e.bound_start();
        let upper = if certified_maximum(e) {
            maximum_root_upper(e.pool, &plan.domain, bounds)
        } else {
            joint_root_upper(e.pool, &plan.domain, bounds, orders, 0)?
        };
        e.fold_unexplored(upper, started);
        Ok(())
    };
    for (i, (domain, rules)) in schedule {
        e.rec.frontier.clear();
        if e.expired() {
            bound_pending(e)?;
            return Ok(());
        }
        e.rec.begin(&mut e.tel, "conversionCompile", None);
        let prepare = Instant::now();
        if census.is_none() {
            census = e.certified.as_mut().and_then(|state| state.census.take());
        }
        let weighed = census.as_mut().and_then(|census| e.weighed_bounds(census, &domain, false));
        let result = match weighed {
            Some(weighed) => Ok(weighed),
            None => JointBounds::compile(e.pool, e.request, &domain, e.metric, e.event_input, e.simulation),
        };
        let result = result.map(|mut part| {
            if certified_maximum(e) {
                part.order_maximum(e.pool, &domain);
            }
            part
        });
        e.tel.environment.bounds.conversion.as_mut().expect("conversion context").compile_ms +=
            prepare.elapsed().as_secs_f64() * 1000.0;
        e.rec.end(&mut e.tel);
        lend(e, &mut census);
        let mut part = match result {
            Ok(part) => {
                e.tel.environment.bounds.conversion.as_mut().expect("conversion context").prepared_domains += 1;
                part
            }
            Err(error) => {
                e.tel.environment.bounds.conversion.as_mut().expect("conversion context").fallback =
                    Some(error.to_string());
                // One whole-domain traversal also covers any groups already completed.
                (e.correlated, e.resource) = global_flags;
                e.rec.parts = 1;
                e.rec.parts_done = 0;
                *p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
                e.rec.begin(&mut e.tel, "search", None);
                ordered_root(p, &plan.domain, bounds, orders, e)?;
                e.rec.end(&mut e.tel);
                return Ok(());
            }
        };
        if e.expired() {
            bound_pending(e)?;
            return Ok(());
        }
        let part_orders = masses(e, &part, orders);
        e.correlated = part.correlation_worthwhile(e.pool, &domain, part_orders)?;
        e.resource = part.resource_worthwhile(e.pool, &domain, part_orders, e.correlated)?;
        let mut rule_order: Vec<_> = (0..rules.len()).collect();
        rule_order.sort_by_key(|&j| std::cmp::Reverse(priority.get(&(i, j)).copied()));
        for j in rule_order {
            e.rec.frontier.clear();
            if e.expired() {
                bound_pending(e)?;
                return Ok(());
            }
            let (r, label) = &rules[j];
            part.set_rules(e.pool, &domain, r.clone());
            *p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
            e.rec.begin(&mut e.tel, "search", Some(label.clone()));
            let more = traverse(p, &domain, &part, part_orders, e)?;
            e.rec.end(&mut e.tel);
            if !more {
                if e.rec.parts_done + 1 < total {
                    bound_pending(e)?;
                }
                return Ok(());
            }
            e.rec.parts_done += 1;
        }
    }
    Ok(())
}

/// One whole-domain traversal of `bounds`. With law clusters, every cluster's bounds bound the teams of the multisets
/// of kinds that belong to it: the leader pairs of all clusters are visited in one order of their depth-1 bounds
/// under their own cluster's bounds, largest first, each one's subtree under its cluster's bounds, so that the best
/// teams of every cluster raise the cutoff as soon as the bound order meets them. Under the validation ablation
/// `STATIC_ORDER` the clusters are traversed one after another.
pub(super) fn traverse(
    p: &mut PhysicalDeck,
    domain: &crate::domain::CandidateDomain,
    bounds: &crate::search::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    use crate::search::joint::SLOTS;
    let Some(clusters) = bounds.law_clusters() else { return ordered_root(p, domain, bounds, orders, e) };
    if warm::static_order() {
        for (c, cluster) in clusters.bounds.iter().enumerate() {
            e.rec.frontier.clear();
            *p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
            if !ordered_root(p, domain, cluster, orders, e)? {
                for later in &clusters.bounds[c + 1..] {
                    let started = e.bound_start();
                    let upper = joint_root_upper(e.pool, domain, later, orders, 0)?;
                    e.fold_unexplored(upper, started);
                }
                return Ok(false);
            }
        }
        return Ok(true);
    }
    // Root-level bound work: each cluster's leader pairs with their depth-1 bounds, in one descending order (a stable
    // sort keeps the clusters' order among equal bounds).
    e.rec.clock.lap(0);
    let mut rows = Vec::new();
    for (c, cluster) in clusters.bounds.iter().enumerate() {
        let root = warm::RootOrder::new(domain, cluster, orders, e)?;
        let prefix = cluster.part_prefix(p, 0, &[0; 5]);
        for (i, (&(m, choice), &cap)) in root.children.iter().zip(&root.caps).enumerate() {
            if cluster.part_allows(prefix.as_ref(), 0, m, choice)? {
                rows.push((cap, c, (m, choice), root.keys[i]));
            }
        }
    }
    rows.sort_by_key(|&(_, _, _, key)| std::cmp::Reverse(key));
    let covers = warm::suffix_maxima(rows.iter().map(|row| row.0));
    if let Some(threshold) = crate::search::snaps::census() {
        let open = rows.iter().filter(|r| r.0.0 >= threshold).count();
        let census = e.tel.leaves.census.get_or_insert_with(|| telemetry::Census::new(threshold));
        census.root.push([open, rows.len()]);
    }
    e.tel.nodes += 1;
    e.tel.joint.nodes[0] += 1;
    let levels = clusters.bounds.iter().map(|b| b.carrier_level_count()).max().unwrap_or(0);
    e.tel.joint.carriers.levels = e.tel.joint.carriers.levels.max(levels);
    if covers.first().is_some_and(|&cover| warm::inferior(cover, e)) {
        e.tel.joint.root_order.traversals_pruned += 1;
    }
    for (offset, &(cap, c, (m, choice), _)) in rows.iter().enumerate() {
        // Each cover bounds its child and every later one.
        e.note_open(Some(covers[offset].0));
        if warm::inferior(covers[offset], e) {
            e.tel.joint.root_order.skipped += (rows.len() - offset) as u64;
            break;
        }
        if warm::inferior(cap, e) {
            e.tel.joint.root_order.skipped += 1;
            continue;
        }
        if e.expired() {
            let started = e.bound_start();
            e.fold_unexplored(Some(cap.0), started);
            return Ok(false);
        }
        *p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        p.members[SLOTS[0]] = m;
        p.snaps[SLOTS[0]] = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
        e.rec.frontier.set(0, offset, rows.len());
        let more = joint_rec(1, 0, p, domain, &clusters.bounds[c], orders, e, None)?;
        e.rec.clock.lap(0);
        if !more {
            if let Some(&next) = covers.get(offset + 1) {
                let started = e.bound_start();
                e.fold_unexplored(Some(next.0), started);
            }
            return Ok(false);
        }
    }
    Ok(true)
}

/// One whole-domain traversal, root children in bound order (static under the validation ablation `STATIC_ORDER`).
pub(super) fn ordered_root(
    p: &mut PhysicalDeck,
    domain: &crate::domain::CandidateDomain,
    bounds: &crate::search::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    if !warm::static_order() {
        // Root-level bound work.
        e.rec.clock.lap(0);
        let root = warm::RootOrder::new(domain, bounds, orders, e)?;
        if let Some(threshold) = crate::search::snaps::census() {
            let open = root.caps.iter().filter(|c| c.0 >= threshold).count();
            let census = e.tel.leaves.census.get_or_insert_with(|| telemetry::Census::new(threshold));
            census.root.push([open, root.caps.len()]);
        }
        e.root_order = Some(root);
    }
    joint_rec(0, 0, p, domain, bounds, orders, e, None)
}

/// The recent checks of the correlated node bound at a depth: its counts there halve at this many checks.
const CORRELATED_WINDOW: u64 = 2048;
/// A check of the correlated node bound costs about this fraction of a node of the traversal.
const CORRELATED_COST: u64 = 4;
/// While it does not pay, the correlated node bound checks every this many nodes of a depth.
const CORRELATED_SAMPLE: u64 = 16;

/// Whether the node at a depth checks the correlated node bound, `recent` its recent checks and prunes there and
/// `nodes` the traversal's node counts by depth. The bound costs more than the other node bounds and, after the
/// order-step bounds, may seldom prune; it runs for its first checks at the depth, then while the nodes its recent
/// prunes cut off (a mean subtree below the depth each, read from the node counts) are at least a
/// [`CORRELATED_COST`]th of its recent checks, and at every [`CORRELATED_SAMPLE`]th node otherwise, so its yield
/// stays measured as the traversal moves to weaker prefixes. Skipping a valid bound changes no result.
fn correlated_due(recent: [u64; 2], nodes: &[u64; telemetry::DEPTHS], depth: usize) -> bool {
    let [checks, pruned] = recent;
    let below: u64 = nodes[depth + 1..].iter().sum();
    checks < CORRELATED_WINDOW / 2
        || pruned.saturating_mul(below).saturating_mul(CORRELATED_COST) >= checks.saturating_mul(nodes[depth])
        || nodes[depth].is_multiple_of(CORRELATED_SAMPLE)
}

/// Counts a check of the correlated node bound in `recent`, halving the counts at [`CORRELATED_WINDOW`] checks.
fn correlated_count(recent: &mut [u64; 2], pruned: bool) {
    recent[0] += 1;
    recent[1] += u64::from(pruned);
    if recent[0] >= CORRELATED_WINDOW {
        *recent = recent.map(|count| count / 2);
    }
}

/// Whether a bound and its power fall below a cutoff (a threshold and the power of a proved tie-break, `i32::MIN`
/// without one).
fn below_cutoff(bound: i128, power: i64, (threshold, cutoff_power): (i128, i32)) -> bool {
    bound < threshold || (cutoff_power > i32::MIN && bound == threshold && power < i64::from(cutoff_power))
}

/// What a choice loop knows of the order-step bounds of its suffixes ([`JointBounds::order_steps_suffix`] from an
/// offset on), which do not increase with the offset: at the cutoff `at`, none is below it up to offset `clear`, and
/// the bound from `cut.0` on is at most `cut.1` (with its power), which was below it. The loop checks the offsets
/// that are multiples of a step: a search from a check offset finds the first one whose bound is below the cutoff,
/// probing at doubling distances and then halving the interval, and the loop stops there. A higher cutoff later
/// starts a new search; the cut stands as long as it stays below.
///
/// [`JointBounds::order_steps_suffix`]: crate::search::joint::JointBounds::order_steps_suffix
#[derive(Default)]
struct StepsTail {
    at: Option<(i128, i32)>,
    clear: usize,
    cut: Option<(usize, i128, i64)>,
}

impl StepsTail {
    /// Whether the order-step bound of the suffix from `offset` is known to be below `cutoff`.
    fn cut_below(&self, offset: usize, cutoff: (i128, i32)) -> bool {
        self.cut.is_some_and(|(at, bound, power)| offset >= at && below_cutoff(bound, power, cutoff))
    }

    /// Whether the check offset `offset` of a loop with check step `step` starts a search.
    fn due(&self, offset: usize, step: usize, cutoff: (i128, i32)) -> bool {
        offset.is_multiple_of(step) && (self.at != Some(cutoff) || offset > self.clear)
    }

    /// The search from the check offset `offset` over the check offsets before the cut (or `width`, where the loop
    /// ends), reading the prepared order steps of the node at `depth`.
    fn search(
        &mut self,
        bounds: &crate::search::joint::JointBounds,
        e: &mut Engine<'_, '_>,
        depth: usize,
        (offset, step, width): (usize, usize, usize),
        cutoff: (i128, i32),
    ) {
        let below = |e: &mut Engine<'_, '_>, at: usize| {
            e.tel.joint.modules.entry("orderStepsTail").or_default().checks += 1;
            let bound = bounds.order_steps_suffix(&e.positions, &mut e.order_steps[depth], at);
            // The suffix summaries hold the open pairs' gains and frontier: the bound of one pass over the suffix.
            debug_assert_eq!(
                bound,
                bounds.order_steps_open(&e.positions, &mut e.order_steps[depth], &bounds.choices[at..])
            );
            bound.filter(|&(bound, power)| below_cutoff(bound, power, cutoff)).map(|(bound, power)| (at, bound, power))
        };
        self.cut = self.cut.filter(|&(_, bound, power)| below_cutoff(bound, power, cutoff));
        // The check offsets in (lo, hi) are unknown; lo is below the search's first offset or not below the cutoff,
        // hi the cut or the loop's end.
        let (mut lo, mut hi) = (offset - step, self.cut.map_or(width, |(at, ..)| at));
        let mut span = step;
        while lo + span < hi {
            let probe = lo + span;
            if let Some(cut) = below(e, probe) {
                (hi, self.cut) = (probe, Some(cut));
                break;
            }
            (lo, span) = (probe, span * 2);
        }
        // The check offsets strictly between lo and hi are lo + j * step for 1 <= j <= inside.
        loop {
            let inside = (hi - lo - 1) / step;
            if inside == 0 {
                break;
            }
            let mid = lo + inside.div_ceil(2) * step;
            match below(e, mid) {
                Some(cut) => (hi, self.cut) = (mid, Some(cut)),
                None => lo = mid,
            }
        }
        (self.at, self.clear) = (Some(cutoff), lo);
    }
}

/// One node of the joint member/Snap traversal: the leader pair at depth 0, then the other slots in `SLOTS` order.
/// The pairs of the other slots take ascending indices of the choice order (`start` is the first index left to the
/// next one), so every team is visited once, in one layout; its canonical layout is the one evaluated.
#[allow(clippy::too_many_arguments)]
pub(super) fn joint_rec(
    depth: usize,
    start: usize,
    p: &mut PhysicalDeck,
    domain: &crate::domain::CandidateDomain,
    bounds: &crate::search::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
    seed_bonus: Option<i64>,
) -> Result<bool, Error> {
    use crate::search::joint::SLOTS;
    // The root loop may follow the bound order of `warm::RootOrder`.
    let root = if depth == 0 { e.root_order.take() } else { None };
    e.tel.nodes += 1;
    e.tel.joint.nodes[depth] += 1;
    if depth == 0 {
        let carriers = &mut e.tel.joint.carriers;
        carriers.levels = carriers.levels.max(bounds.carrier_level_count());
    }
    // One clock read serves both the deadline and the per-depth time.
    let (now, _) = e.rec.clock.lap(depth);
    if e.expired_at(now) {
        e.unexplored_node(p, depth, domain, bounds, orders)?;
        return Ok(false);
    }
    if domain.required().iter().filter(|r| !SLOTS[..depth].iter().any(|&slot| p.members[slot] == **r)).count()
        > 5 - depth
    {
        return Ok(true);
    }
    if let Some(target) = seed_bonus
        && bounds.bonus_upper(e.pool, domain, p, depth).is_some_and(|b| b < target)
    {
        e.tel.joint.seed_bonus_skipped[depth] += 1;
        return Ok(true);
    }
    // Every completion of this prefix has at most `placed + 5 - depth` Gekisou combo carriers; the cheap bounds of
    // that carrier level cover it (raw, fine and cutoff caps read the candidate's own factors). The envelope keyed by
    // the placed performers (their carriers' windows and their factor commands) tightens its `A0` and the placed
    // slots' gains.
    let choices = crate::search::joint::JointBounds::prefix_choices(domain, p, depth);
    let placed = bounds.carriers_placed(p, depth, &choices);
    let level = (placed + 5 - depth).min(5);
    let node_bounds = bounds.carrier_level(level);
    let keyed = if depth > 0 { bounds.keyed(p, depth, &choices, 5 - depth, 5 - depth) } else { None };
    // The order-step bounds bound sums over the orders, which bounds of each order's payoff compare with no sum.
    let sums = !bounds.is_order_maximum();
    e.order_steps[depth].reset();
    // At a complete team, the power of the node's bound: at least the team's power.
    let mut power_upper = None;
    if depth > 0
        && let Some((threshold, cutoff_power)) = e.safe_cutoff()
    {
        // i32::MIN marks a certified cutoff without a proved power tie-break.
        let exact_ties = cutoff_power > i32::MIN;
        let joint = &mut e.tel.joint;
        joint.branch.check(depth);
        joint.carriers.nodes[level] += 1;
        let (numerator, power) = node_bounds.expected_upper_keyed(e.pool, domain, p, depth, orders, keyed.as_ref())?;
        power_upper = (!orders.is_empty()).then_some(power);
        if numerator < threshold || (exact_ties && numerator == threshold && power < i64::from(cutoff_power)) {
            joint.branch.prune(depth);
            return Ok(true);
        }
        if numerator == threshold {
            joint.node_ties[depth] += 1;
        }
        // The open slots take ascending candidate indices from `start` on. The suffix summaries this check collects
        // serve the order-step checks of the choice loop too.
        if depth < 5
            && sums
            && node_bounds.order_steps_prepare(
                e.pool,
                domain,
                p,
                depth,
                &e.positions,
                keyed.as_ref(),
                &mut e.order_steps[depth],
            )
            && let Some((steps, steps_power)) =
                node_bounds.order_steps_suffix(&e.positions, &mut e.order_steps[depth], start)
        {
            let module = joint.modules.entry("orderSteps").or_default();
            module.checks += 1;
            if steps < threshold || (exact_ties && steps == threshold && steps_power < i64::from(cutoff_power)) {
                module.pruned += 1;
                return Ok(true);
            }
        }
        if exact_ties
            && numerator == threshold
            && let Some(assigned_power) = bounds.assignment_power_upper(e.pool, domain, p, depth)
        {
            joint.assignment.check(depth);
            if assigned_power < i64::from(cutoff_power) {
                joint.assignment.prune(depth);
                return Ok(true);
            }
        }
        if depth < 5
            && let Some(cap) = bounds.carrier_split_expected_upper(domain, p, depth, start, orders, threshold)
        {
            let module = joint.modules.entry("carrierSplit").or_default();
            module.checks += 1;
            if cap < threshold || (exact_ties && cap == threshold && power < i64::from(cutoff_power)) {
                module.pruned += 1;
                return Ok(true);
            }
        }
        if depth < 5 && e.correlated && correlated_due(e.correlated_recent[depth], &joint.nodes, depth) {
            joint.correlated.check(depth);
            let correlated =
                node_bounds.correlated_expected_upper_keyed(e.pool, domain, p, depth, orders, keyed.as_ref())?;
            let pruned =
                correlated < threshold || (exact_ties && correlated == threshold && power < i64::from(cutoff_power));
            correlated_count(&mut e.correlated_recent[depth], pruned);
            if pruned {
                joint.correlated.prune(depth);
                return Ok(true);
            }
        }
        if depth < 5 && e.resource {
            joint.resource.check(depth);
            if let Some(cap) = node_bounds.resource_expected_upper(e.pool, domain, p, depth, orders)
                && (cap < threshold || (exact_ties && cap == threshold && power < i64::from(cutoff_power)))
            {
                joint.resource.prune(depth);
                return Ok(true);
            }
        }
        if depth < 5 {
            for module in bounds.modules() {
                let Some(cap) = module.node_upper(e.pool, p, depth, true, orders) else { continue };
                joint.modules.entry(module.name()).or_default().checks += 1;
                if cap < threshold || (exact_ties && cap == threshold && power < i64::from(cutoff_power)) {
                    joint.modules.entry(module.name()).or_default().pruned += 1;
                    return Ok(true);
                }
            }
        }
        if seed_bonus.is_none() && depth < 5 && bounds.is_pt() {
            joint.bonus.check(depth);
            if let Some((cap, cap_power)) =
                node_bounds.bonus_expected_upper_with_power(e.pool, domain, p, depth, orders, &mut e.bonus_scratch)?
            {
                if cap < threshold || (exact_ties && cap == threshold && power.min(cap_power) < i64::from(cutoff_power))
                {
                    joint.bonus.prune(depth);
                    return Ok(true);
                }
                if cap == threshold {
                    joint.node_ties[depth] += 1;
                }
            } else {
                joint.bonus_unavailable[depth] += 1;
            }
        }
    }
    if depth == 5 {
        let more = e.consider_within(*p, Some((bounds, domain)), power_upper)?;
        if !more {
            e.unexplored_node(p, depth, domain, bounds, orders)?;
        }
        return Ok(more && !(seed_bonus.is_some() && e.safe_cutoff().is_some()));
    }
    let slot = SLOTS[depth];
    // A child that is not a carrier leaves at most `placed + 4 - depth` carriers to its completions, a carrier one
    // more; the tail check covers both kinds of children. Each reads the keyed envelope of as many carriers to come
    // (the child's own commands are not known yet).
    let low = bounds.carrier_level((placed + 4 - depth).min(5));
    let high = bounds.carrier_level((placed + 5 - depth).min(5));
    let (tail, tail_high) = if std::ptr::eq(low, high) {
        (low.tail_state_keyed(e.pool, domain, p, depth, orders, keyed.as_ref()), None)
    } else {
        let keyed_low = bounds.keyed(p, depth, &choices, 4 - depth, 5 - depth);
        (
            low.tail_state_keyed(e.pool, domain, p, depth, orders, keyed_low.as_ref()),
            high.tail_state_keyed(e.pool, domain, p, depth, orders, keyed.as_ref()),
        )
    };
    // The tail bound of every kind of child, also for what a stop leaves unexplored.
    let tail_check = tail_high.as_ref().or(tail.as_ref()).map(|state| (high, state));
    // With correlated bounds, the node's correlated bound of each child with its pair fixed (see
    // `JointBounds::character_pairs`): it drops children below the cutoff before they are visited.
    let character_pairs = if e.correlated && (1..5).contains(&depth) {
        node_bounds.character_pairs(e.pool, domain, p, depth, orders, keyed.as_ref())
    } else {
        None
    };
    // With bounds of a cluster of laws, the kinds of the prefix its children extend.
    let part_prefix = bounds.part_prefix(p, depth, &choices);
    // There is no tail state at the root, so its loop may run in any order.
    let children = root.as_ref().map_or(&bounds.choices[..], |r| &r.children[..]);
    let width = children.len();
    if root.as_ref().and_then(|r| r.best()).is_some_and(|cap| warm::inferior(cap, e)) {
        e.tel.joint.root_order.traversals_pruned += 1;
    }
    // The children before this offset lie in a block whose bound failed.
    let mut skip_to = 0;
    let mut steps_tail = StepsTail::default();
    for (offset, &(m, choice)) in children.iter().enumerate().skip(start) {
        if offset < skip_to {
            continue;
        }
        if let Some(r) = &root
            && seed_bonus.is_none()
        {
            // Ordered root children: each cover bounds its child and every later one.
            e.note_open(Some(r.covers[offset].0));
        }
        if let Some(r) = &root {
            if warm::inferior(r.covers[offset], e) {
                e.tel.joint.root_order.skipped += (width - offset) as u64;
                break;
            }
            if warm::inferior(r.caps[offset], e) {
                e.tel.joint.root_order.skipped += 1;
                continue;
            }
        }
        if offset % 16 == 0
            && let Some((threshold, cutoff_power)) = e.safe_cutoff()
            && let Some(state) = tail_high.as_ref().or(tail.as_ref())
        {
            e.tel.joint.tail.check(depth);
            let (upper, power) = high.tail_upper(state, offset)?;
            if upper < threshold || (upper == threshold && power < i64::from(cutoff_power)) {
                e.tel.joint.tail.prune(depth);
                e.tel.joint.tail_choices_skipped[depth] += (width - offset) as u64;
                break;
            }
        }
        // The children whose pairs share a block of the choice order with this one: the block's maxima bound them
        // all (also the ones before `start` of a first, partial block).
        if (offset == start || offset % crate::search::joint::PAIR_BLOCK == 0)
            && let Some((threshold, cutoff_power)) = e.safe_cutoff()
            && let Some(state) = tail_high.as_ref().or(tail.as_ref())
        {
            let block = offset / crate::search::joint::PAIR_BLOCK;
            let (upper, power) = high.block_upper(state, block)?;
            let module = e.tel.joint.modules.entry("pairBlocks").or_default();
            module.checks += 1;
            if upper < threshold || (upper == threshold && power < i64::from(cutoff_power)) {
                module.pruned += 1;
                skip_to = (block + 1) * crate::search::joint::PAIR_BLOCK;
                continue;
            }
        }
        // Below the leader every child and its completions take candidates from `offset` on: their order steps bound
        // all the children left (the node's own check covered `start`), read at the check offsets by `StepsTail`.
        if depth > 0
            && sums
            && offset > start
            && let Some(cutoff) = e.safe_cutoff()
        {
            let step = if depth < 4 { 4 } else { 16 };
            if !steps_tail.cut_below(offset, cutoff)
                && steps_tail.due(offset, step, cutoff)
                && node_bounds.order_steps_prepare(
                    e.pool,
                    domain,
                    p,
                    depth,
                    &e.positions,
                    keyed.as_ref(),
                    &mut e.order_steps[depth],
                )
            {
                steps_tail.search(node_bounds, e, depth, (offset, step, width), cutoff);
            }
            if steps_tail.cut_below(offset, cutoff) {
                e.tel.joint.modules.entry("orderStepsTail").or_default().pruned += 1;
                e.tel.joint.tail_choices_skipped[depth] += (width - offset) as u64;
                break;
            }
        }
        // Every child from `offset` on and its completions take candidates from `offset` on: the carrier split of
        // that suffix bounds them all.
        if depth > 0
            && offset > start
            && offset % 16 == 0
            && let Some((threshold, _)) = e.safe_cutoff()
            && let Some(cap) = bounds.carrier_split_expected_upper(domain, p, depth, offset, orders, threshold)
        {
            let module = e.tel.joint.modules.entry("carrierSplitTail").or_default();
            module.checks += 1;
            if cap < threshold {
                module.pruned += 1;
                e.tel.joint.tail_choices_skipped[depth] += (width - offset) as u64;
                break;
            }
        }
        // The node itself checked the deadline; inside the choice loop the clock is read every 32 offsets.
        if offset % 32 == 31 && e.expired() {
            e.unexplored_choices(p, depth, offset, tail_check, root.as_ref(), domain, bounds, orders)?;
            return Ok(false);
        }
        // The correlated bounds of the children from this offset on and of this block of children.
        if let Some(pairs) = &character_pairs
            && (offset == start || offset % crate::search::joint::PAIR_BLOCK == 0)
            && let Some((threshold, _)) = e.safe_cutoff()
        {
            let joint = &mut e.tel.joint;
            if let Some(cap) = node_bounds.character_set_upper(pairs, offset, false) {
                joint.character_tail.check(depth);
                if cap < threshold {
                    joint.character_tail.prune(depth);
                    joint.tail_choices_skipped[depth] += (width - offset) as u64;
                    break;
                }
            }
            let block = offset / crate::search::joint::PAIR_BLOCK;
            if let Some(cap) = node_bounds.character_set_upper(pairs, block, true) {
                joint.character_blocks.check(depth);
                if cap < threshold {
                    joint.character_blocks.prune(depth);
                    skip_to = (block + 1) * crate::search::joint::PAIR_BLOCK;
                    continue;
                }
            }
        }
        if slot == 2 && domain.leader().is_some_and(|l| l != m) {
            continue;
        }
        let character = e.pool.members[m].character_id;
        if SLOTS[..depth].iter().any(|&s| e.pool.members[p.members[s]].character_id == character)
            || domain.required().iter().any(|&r| r != m && e.pool.members[r].character_id == character)
        {
            continue;
        }
        let snap = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
        if snap.is_some() && SLOTS[..depth].iter().any(|&s| p.snaps[s] == snap) {
            continue;
        }
        if !bounds.allows(slot, choice) {
            continue;
        }
        if let Some(pairs) = &character_pairs
            && let Some((threshold, _)) = e.safe_cutoff()
            && let Some(cap) = node_bounds.character_pair_upper(pairs, e.pool, m, choice)
        {
            e.tel.joint.character_pairs.check(depth);
            if cap < threshold {
                e.tel.joint.character_pairs.prune(depth);
                continue;
            }
        }
        if !bounds.part_allows(part_prefix.as_ref(), depth, m, choice)? {
            continue;
        }
        let (pair_bounds, pair_tail) = match &tail_high {
            Some(state) if bounds.is_carrier(m, choice) => (high, Some(state)),
            _ => (low, tail.as_ref()),
        };
        if let Some((threshold, cutoff_power)) = e.safe_cutoff()
            && let Some(state) = pair_tail
        {
            e.tel.joint.pair.check(depth);
            let (upper, power) = pair_bounds.pair_upper(state, m, choice)?;
            if upper < threshold || (upper == threshold && power < i64::from(cutoff_power)) {
                e.tel.joint.pair.prune(depth);
                continue;
            }
            if upper == threshold {
                e.tel.joint.pair_ties[depth] += 1;
            }
        }
        // The order steps of the child's completions: it takes this slot, the slots after it take pairs from
        // `offset + 1` on (the node's suffix summaries).
        if (1..4).contains(&depth)
            && sums
            && let Some((threshold, cutoff_power)) = e.safe_cutoff()
            && node_bounds.order_steps_prepare(
                e.pool,
                domain,
                p,
                depth,
                &e.positions,
                keyed.as_ref(),
                &mut e.order_steps[depth],
            )
            && let Some((steps, steps_power)) =
                node_bounds.order_steps_pair(&e.positions, &mut e.order_steps[depth], offset, m, choice)
        {
            let exact_ties = cutoff_power > i32::MIN;
            let module = e.tel.joint.modules.entry("orderStepsPair").or_default();
            module.checks += 1;
            if steps < threshold || (exact_ties && steps == threshold && steps_power < i64::from(cutoff_power)) {
                module.pruned += 1;
                continue;
            }
        }
        p.members[slot] = m;
        p.snaps[slot] = snap;
        e.rec.frontier.set(depth, offset, width);
        // The leader's children start the ascending run of the other slots.
        let next = if depth == 0 { 0 } else { offset + 1 };
        let more = joint_rec(depth + 1, next, p, domain, bounds, orders, e, seed_bonus)?;
        e.rec.clock.lap(depth);
        if !more {
            e.unexplored_choices(p, depth, offset + 1, tail_check, root.as_ref(), domain, bounds, orders)?;
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether the search is certified and values a team by its best performance order.
fn certified_maximum(e: &Engine<'_, '_>) -> bool {
    e.certified.as_ref().is_some_and(|state| state.reduction == crate::search::certified_search::Reduction::Maximum)
}

/// The order masses of a traversal of `bounds`: one pseudo-order of mass 1 for bounds of each order's payoff (see
/// `JointBounds::order_maximum`), otherwise the position-mean pseudo-order of the search.
fn masses<'o>(
    e: &Engine<'_, '_>,
    bounds: &crate::search::joint::JointBounds,
    orders: &'o [([usize; 5], u128)],
) -> &'o [([usize; 5], u128)] {
    if certified_maximum(e) && bounds.is_order_maximum() { &ORDER_MAXIMUM } else { orders }
}

/// The pseudo-order of bounds of each order's payoff: any positions, mass 1.
static ORDER_MAXIMUM: [([usize; 5], u128); 1] = [([0, 1, 2, 3, 4], 1)];

/// The best payoff of any order of any completion through the legal depth-0 choices of `bounds`: the largest
/// depth-1 [`JointBounds::maximum_upper`]. None when no legal choice remains.
///
/// [`JointBounds::maximum_upper`]: crate::search::joint::JointBounds::maximum_upper
fn maximum_root_upper(
    pool: &Pool,
    domain: &crate::domain::CandidateDomain,
    bounds: &crate::search::joint::JointBounds,
) -> Option<i128> {
    use crate::search::joint::SLOTS;
    let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    let mut upper = None;
    for &(m, choice) in &bounds.choices {
        let character = pool.members[m].character_id;
        if domain.leader().is_some_and(|l| l != m)
            || domain.required().iter().any(|&r| r != m && pool.members[r].character_id == character)
            || !bounds.allows(SLOTS[0], choice)
        {
            continue;
        }
        p.members[SLOTS[0]] = m;
        p.snaps[SLOTS[0]] = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
        let (cap, _) = bounds.maximum_upper(pool, domain, &p, 1);
        upper = Some(upper.map_or(cap, |u: i128| u.max(cap)));
    }
    upper
}
