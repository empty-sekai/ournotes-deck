//! Maximum aggregation of a played live: a team is worth its best performance order.
//!
//! The value of one order is the expectation of its terminal payoff over the random draws it contains. Orders that
//! draw no lottery have one outcome, so their value is that outcome's payoff. [`Engine::evaluate_maximum`] simulates
//! the orders as one prefix-sharing tree whose node bounds combine by maximum, and omits the orders whose bound lies
//! strictly below a payoff already attained, so every order that ties the maximum is evaluated. [`search`] walks the
//! domain with the prefix bound [`JointBounds::maximum_upper`]. The result holds the maximum as the payoff
//! numerator over denominator one.
use super::*;
use crate::domain::CandidateDomain;
use crate::search::joint::{JointBounds, SLOTS};
use ournotes_sim::live::full::{LiveModel, MaximumOrdersOutcome, Settled};
use ournotes_sim::live::random::LiveRandom;
use std::ops::ControlFlow;

/// Frames between two bound checks of the node being played.
const CUTOFF_EVERY: usize = 30;

#[cfg(test)]
#[path = "maximum/bound_reuse_tests.rs"]
mod bound_reuse_tests;

/// The table, payoff map and order are fixed for one leaf. Only an identical settled prefix reuses a cap.
fn settled_cap(previous: &mut Option<(Settled, i128)>, settled: Settled, calculate: impl FnOnce() -> i128) -> i128 {
    if let Some((point, cap)) = *previous
        && point == settled
    {
        return cap;
    }
    let cap = calculate();
    *previous = Some((settled, cap));
    cap
}

/// Whether the greatest cap lies below `threshold`: then no order reaches it.
fn maximum_cap_below(caps: &[i128], threshold: i128) -> bool {
    caps.iter().copied().max().is_some_and(|upper| upper < threshold)
}

/// Lowers every cap by `tighten`, which receives the order and its cap; Break when the search must stop.
fn tighten_maximum_caps(
    caps: &mut [i128],
    mut tighten: impl FnMut(usize, i128) -> ControlFlow<(), i128>,
) -> ControlFlow<()> {
    for (index, cap) in caps.iter_mut().enumerate() {
        *cap = (*cap).min(tighten(index, *cap)?);
    }
    ControlFlow::Continue(())
}

/// The evaluation of a team from the exact payoffs of its orders (those a strict bound omitted are absent): the
/// greatest payoff over denominator one is the team's payoff.
fn aggregate_maximum(outcomes: Vec<SeedOutcome>) -> Result<FiniteEvaluation, Error> {
    let best = outcomes.iter().map(|outcome| outcome.terminal_payoff).max();
    let mut evaluation = expectation::aggregate(outcomes)?;
    evaluation.expected_payoff = expectation::ExactExpectation {
        numerator: best.ok_or_else(|| Error::Domain("a team has no evaluated order".into()))?,
        denominator: 1,
    };
    Ok(evaluation)
}

impl Engine<'_, '_> {
    /// The payoff and power a team must reach to enter the Top-K: `(numerator, power of the K-th)` once it is full.
    fn maximum_cutoff(&self) -> Option<(i128, i32)> {
        (self.top.len() == self.request.k).then(|| {
            let kth = self.top.last().expect("full Top-K");
            (kth.evaluation.expected_payoff.numerator, kth.power)
        })
    }

    /// The maximum over the performance orders of a team's payoff, or `Pruned` when it provably ranks after the
    /// K-th, or `Stopped` when the budget ran out. Per-order caps (cheap, then raw and fine) and the cutoff tables
    /// bound the orders; their maximum, not their sum, bounds the team.
    pub(super) fn evaluate_maximum(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        cut: Option<(&JointBounds, &CandidateDomain)>,
    ) -> Result<Leaf, Error> {
        let cutoff = self.maximum_cutoff();
        // The payoff the team must exceed to rank before the K-th: an equal payoff needs a power at least the K-th's.
        let threshold =
            cutoff.map(|(payoff, kth_power)| if power < kth_power { payoff.saturating_add(1) } else { payoff });
        let caps =
            cut.map(|(bounds, domain)| bounds.order_cheap_caps(domain, physical, i64::from(power), &self.positions));
        if let (Some(caps), Some(threshold)) = (&caps, threshold)
            && maximum_cap_below(caps, threshold)
        {
            self.tel.leaves.cheap_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        let mut input = expectation::context(self.pool, physical, &self.request.objective)?;
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        input.lottery_free = self.lottery_free.clone();
        if let Some(scores) = self.team_scores.get(physical, power, &mut self.tel.caches.team_scores) {
            let outcomes = self
                .orders
                .iter()
                .zip(scores)
                .map(|(&performance_order, (final_score, final_life))| {
                    Ok(SeedOutcome {
                        root_seed: 0,
                        weight: 1,
                        performance_order,
                        final_score,
                        terminal_payoff: self.payoff(physical, final_score, power, Some(final_life))?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            return Ok(Leaf::Evaluated(aggregate_maximum(outcomes)?));
        }
        let performers = input.performers.clone();
        let cached = self.programs.get_partial(physical.members, &performers, power, &mut self.tel.caches.programs);
        self.tel.caches.program_evaluation_ms = self.programs.evaluation_ms();
        let mut outcomes: Vec<Option<SeedOutcome>> = vec![None; ORDERS];
        let mut final_lives = [0; ORDERS];
        let mut cached_best = i128::MIN;
        let mut cached_count = 0usize;
        if let Some(cached) = cached {
            for (index, score) in cached.into_iter().enumerate() {
                if let Some((final_score, final_life)) = score {
                    let payoff = self.payoff(physical, final_score, power, Some(final_life))?;
                    cached_best = cached_best.max(payoff);
                    cached_count += 1;
                    final_lives[index] = final_life;
                    outcomes[index] = Some(SeedOutcome {
                        root_seed: 0,
                        weight: 1,
                        performance_order: self.orders[index],
                        final_score,
                        terminal_payoff: payoff,
                    });
                }
            }
            self.tel.caches.program_orders_reused += cached_count as u64;
        }
        if cached_count == ORDERS {
            let evaluation = aggregate_maximum(outcomes.into_iter().map(Option::unwrap).collect())?;
            self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
            return Ok(Leaf::Evaluated(evaluation));
        }
        // A cached order already reaching the threshold keeps the team: it is evaluated in full.
        let stop_below = threshold.filter(|&threshold| cached_best < threshold).unwrap_or(i128::MIN);
        let mut caps = caps.unwrap_or_else(|| vec![self.metric.upper().unwrap_or(i128::MAX); ORDERS]);
        if stop_below != i128::MIN
            && let Some((bounds, domain)) = cut.filter(|(bounds, _)| bounds.has_fine())
        {
            let (_, resume) = self.rec.clock.lap(slot::FINE);
            let tightened = tighten_maximum_caps(&mut caps, |index, mut cap| {
                if self.expired() {
                    return ControlFlow::Break(());
                }
                // A strict upper bound below the threshold already excludes this order.
                if cap < stop_below {
                    return ControlFlow::Continue(cap);
                }
                let positions = &self.positions[index];
                if let Some(raw) = bounds.raw_upper(domain, physical, i64::from(power), positions) {
                    cap = cap.min(raw);
                }
                if cap < stop_below {
                    return ControlFlow::Continue(cap);
                }
                if let Some(fine) =
                    bounds.fine_upper(domain, physical, i64::from(power), positions, &mut self.bound_scratch)
                {
                    cap = cap.min(fine);
                }
                self.tel.leaves.fine_orders += 1;
                ControlFlow::Continue(cap)
            });
            self.rec.clock.lap(resume);
            if tightened.is_break() || self.expired() {
                return Ok(Leaf::Stopped);
            }
            if maximum_cap_below(&caps, stop_below) {
                self.tel.leaves.fine_pruned += 1;
                return Ok(Leaf::Pruned);
            }
        }
        let missing: Vec<usize> = (0..ORDERS).filter(|&index| outcomes[index].is_none()).collect();
        if stop_below != i128::MIN && missing.iter().all(|&index| caps[index] < stop_below) {
            self.tel.leaves.order_bound_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        let performance_orders = self.orders.clone();
        let missing_orders: Vec<[usize; 5]> = missing.iter().map(|&index| performance_orders[index]).collect();
        let (pool, request, metric, event_input) = (self.pool, self.request, self.metric, self.event_input);
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        let spliced = self.spliced_scores(&input, &missing_orders);
        self.rec.clock.lap(resume);
        if let Some(scores) = spliced? {
            for (&index, final_score) in missing.iter().zip(scores) {
                outcomes[index] = Some(SeedOutcome {
                    root_seed: 0,
                    weight: 1,
                    performance_order: performance_orders[index],
                    final_score,
                    terminal_payoff: payoff_of(pool, request, metric, event_input, physical, final_score, power, None)?,
                });
            }
            let evaluation =
                aggregate_maximum(outcomes.into_iter().map(|o| o.expect("every order evaluated")).collect())?;
            // The metric reads no final life (`spliced_scores`): the lives of the spliced orders stay unknown.
            self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
            return Ok(Leaf::Evaluated(evaluation));
        }
        let orders: Vec<Vec<usize>> = missing_orders.iter().map(|order| order.to_vec()).collect();
        let capture_budget =
            self.programs.capture_budget_for(physical.members, &performers, &mut self.tel.caches.program_admissions);
        self.tel.caches.program_bytes = self.programs.allocated_bytes();
        self.tel.caches.program_recordings += u64::from(capture_budget > 0);
        let live = input.into_ordered();
        let master = pool.master;
        let mut visit = |local: usize, model: &LiveModel| -> Result<i128, Error> {
            let index = missing[local];
            if model.draws() != 0 {
                return Err(Error::Unsupported(
                    "a skill or mission of this team draws a lottery; maximum aggregation covers lottery-free lives \
                     only"
                        .into(),
                ));
            }
            let final_score = model.score();
            final_lives[index] = model.current_life();
            let payoff = payoff_of(
                pool,
                request,
                metric,
                event_input,
                physical,
                final_score,
                power,
                Some(model.current_life()),
            )?;
            outcomes[index] = Some(SeedOutcome {
                root_seed: 0,
                weight: 1,
                performance_order: performance_orders[index],
                final_score,
                terminal_payoff: payoff,
            });
            Ok(payoff)
        };
        let fine = cut.filter(|(bounds, _)| stop_below != i128::MIN && bounds.has_fine());
        let mut tables: Vec<Option<Option<_>>> = (0..ORDERS).map(|_| None).collect();
        let mut settled_caps = [None; ORDERS];
        self.tel.leaves.started += 1;
        let recording_started = (capture_budget > 0).then(Instant::now);
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        let upper = |ids: &[usize], model: &LiveModel, settled: Settled| {
            if self.expired() {
                return Ok(ControlFlow::Break(()));
            }
            let Some((bounds, domain)) = fine.filter(|_| model.frames_played() > 0) else {
                return Ok(ControlFlow::Continue(
                    ids.iter().map(|&local| caps[missing[local]]).max().expect("node orders"),
                ));
            };
            let (_, resume) = self.rec.clock.lap(slot::CUTOFF_TABLE);
            let mut maximum = i128::MIN;
            for &local in ids {
                let index = missing[local];
                let cap = settled_cap(&mut settled_caps[index], settled, || {
                    let table = tables[index].get_or_insert_with(|| {
                        let table = bounds.cutoff_table(
                            domain,
                            physical,
                            i64::from(power),
                            &self.positions[index],
                            &mut self.bound_scratch,
                        );
                        self.tel.leaves.cutoff.tables += u64::from(table.is_some());
                        self.tel.leaves.cutoff.unavailable += u64::from(table.is_none());
                        table
                    });
                    table
                        .as_ref()
                        .and_then(|table| table.payoff_cap(bounds, settled))
                        .map_or(caps[index], |cap| cap.min(caps[index]))
                });
                maximum = maximum.max(cap);
            }
            self.rec.clock.lap(resume);
            Ok(ControlFlow::Continue(maximum))
        };
        let result = live.maximize_orders_bounded_recorded_partial(
            master,
            &orders,
            LiveRandom::new(0),
            capture_budget,
            stop_below,
            CUTOFF_EVERY,
            (cached_count > 0).then_some(cached_best),
            upper,
            &mut visit,
        );
        self.rec.clock.lap(resume);
        if let Some(started) = recording_started {
            self.tel.caches.program_recording_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        let (outcome, programs) = result?;
        if let Some(programs) = programs {
            self.programs.insert_partial(
                physical.members,
                &performers,
                &missing_orders,
                programs,
                &mut self.tel.caches.programs,
            );
            self.tel.caches.program_bytes = self.programs.allocated_bytes();
            (self.tel.caches.program_recorded_nodes, self.tel.caches.program_recorded_bytes) =
                self.programs.recorded_work();
        }
        let (MaximumOrdersOutcome::AllOrders(shared)
        | MaximumOrdersOutcome::MaximumProven { sharing: shared, .. }
        | MaximumOrdersOutcome::BelowThreshold(shared)
        | MaximumOrdersOutcome::Interrupted(shared)) = outcome;
        self.tel.leaves.order_tree.add(&shared);
        self.tel.leaves.simulations += (outcomes.iter().flatten().count() - cached_count) as u64;
        Ok(match outcome {
            MaximumOrdersOutcome::AllOrders(_) => {
                let evaluation =
                    aggregate_maximum(outcomes.into_iter().map(|o| o.expect("completed order")).collect())?;
                self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
                Leaf::Evaluated(evaluation)
            }
            // A strict bound omitted only orders below an attained payoff; their outcomes stay absent from the
            // complete-vector caches.
            MaximumOrdersOutcome::MaximumProven { .. } => {
                Leaf::Evaluated(aggregate_maximum(outcomes.into_iter().flatten().collect())?)
            }
            MaximumOrdersOutcome::BelowThreshold(shared) => {
                telemetry::record_stop(
                    &mut self.tel.leaves.cutoff,
                    shared.frames as usize,
                    shared.separate_frames as usize,
                );
                self.tel.leaves.order_bound_pruned += 1;
                Leaf::Pruned
            }
            MaximumOrdersOutcome::Interrupted(_) => Leaf::Stopped,
        })
    }
}

/// Every team of the domain, depth first, pruned by the prefix bound when bounds are compiled.
pub(super) fn search(
    engine: &mut Engine<'_, '_>,
    domain: &CandidateDomain,
    bounds: Option<&JointBounds>,
) -> Result<(), Error> {
    let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    let _ = match bounds {
        Some(bounds) => visit(0, None, &mut physical, domain, bounds, engine)?,
        None => members_rec(
            0,
            0,
            &mut physical,
            domain.members(),
            domain.required(),
            domain.leader(),
            domain.snaps(),
            engine,
        )?,
    };
    Ok(())
}

/// Ok(false) when the search must stop.
fn visit(
    depth: usize,
    after: Option<i64>,
    physical: &mut PhysicalDeck,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    engine: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    engine.tel.nodes += 1;
    if engine.expired() {
        return Ok(false);
    }
    let selected = &SLOTS[..depth];
    if domain
        .required()
        .iter()
        .filter(|&&member| !selected.iter().any(|&slot| physical.members[slot] == member))
        .count()
        > 5 - depth
    {
        return Ok(true);
    }
    if depth > 0
        && let Some((threshold, kth_power)) = engine.maximum_cutoff()
    {
        let (upper, power) = bounds.maximum_upper(engine.pool, domain, physical, depth);
        // An equal payoff with a power below the K-th's ranks after it; an equal power may still win on identity.
        if upper < threshold || (upper == threshold && power < i64::from(kth_power)) {
            return Ok(true);
        }
    }
    if depth == 5 {
        return engine.consider_with(*physical, Some((bounds, domain)));
    }
    let slot = SLOTS[depth];
    for &(member, choice) in &bounds.choices {
        let card = &engine.pool.members[member];
        if (depth == 0 && domain.leader().is_some_and(|leader| leader != member))
            || (depth > 0 && after.is_some_and(|after| card.id <= after))
            || selected
                .iter()
                .any(|&slot| engine.pool.members[physical.members[slot]].character_id == card.character_id)
            || domain
                .required()
                .iter()
                .any(|&required| required != member && engine.pool.members[required].character_id == card.character_id)
        {
            continue;
        }
        let snap = (choice > 0).then(|| domain.snaps()[choice - 1]);
        if snap.is_some() && selected.iter().any(|&slot| physical.snaps[slot] == snap) {
            continue;
        }
        physical.members[slot] = member;
        physical.snaps[slot] = snap;
        if !visit(depth + 1, if depth == 0 { None } else { Some(card.id) }, physical, domain, bounds, engine)? {
            return Ok(false);
        }
    }
    Ok(true)
}
