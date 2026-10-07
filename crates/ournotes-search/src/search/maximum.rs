//! Maximum reachable terminal payoff, with canonical team identity and per-order score bounds.
use super::*;
use crate::domain::CandidateDomain;
use crate::search::expectation::ExactExpectation;
use crate::search::joint::{JointBounds, SLOTS};
use ournotes_sim::live::full::{
    LiveModel, LuckExactBudget, LuckExactDecline, LuckExactSession, LuckScoreSession, OrdersOutcome, Settled,
};
use ournotes_sim::live::random::LiveRandom;
use std::cell::Cell;
use std::ops::ControlFlow;

const CUTOFF_EVERY: usize = 30;

fn aggregate_maximum(outcomes: Vec<SeedOutcome>) -> Result<FiniteEvaluation, Error> {
    let best = outcomes.iter().map(|outcome| outcome.terminal_payoff).max();
    let mut evaluation = expectation::aggregate(outcomes)?;
    evaluation.expected_payoff = ExactExpectation { numerator: best.expect("nonempty outcomes"), denominator: 1 };
    Ok(evaluation)
}

impl Engine<'_, '_> {
    pub(super) fn evaluate_maximum(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        cut: Option<(&JointBounds, &CandidateDomain)>,
    ) -> Result<Leaf, Error> {
        let caps =
            cut.map(|(bounds, domain)| bounds.order_cheap_caps(domain, physical, i64::from(power), &self.positions));
        if let Some(caps) = &caps
            && self.top.len() == self.request.k
            && let Some(last) = self.top.last()
        {
            let upper = caps.iter().copied().max().expect("performance orders");
            if upper < last.evaluation.expected_payoff.numerator
                || (upper == last.evaluation.expected_payoff.numerator && power < last.power)
            {
                self.tel.leaves.cheap_pruned += 1;
                return Ok(Leaf::Pruned);
            }
        }
        let mut input = expectation::context(self.pool, physical, &self.request.objective)?;
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        input.lottery_free = self.lottery_free.clone();
        if input
            .gekisou
            .as_ref()
            .is_none_or(|setup| !setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission == 2))
            && let Some(result) = self.evaluate_maximum_deterministic(physical, power, &input, caps.as_deref(), cut)?
        {
            return Ok(result);
        }
        let master = self.pool.master;
        let mut exact = match &input.gekisou {
            Some(setup) => LuckExactSession::new(
                master,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
                self.limits.cache_entries,
            )?,
            None => LuckExactSession::new_live(
                master,
                &input.notes,
                &input.events,
                input.params,
                &input.play,
                &input.delta_times,
                self.limits.cache_entries,
            )?,
        };
        let skills = input.gekisou.as_ref().and_then(|_| ournotes_sim::live::full::luck_skills(master).ok());
        let mut score_bounds = input.gekisou.as_ref().zip(skills.as_ref()).map(|(setup, skills)| {
            LuckScoreSession::new(
                master,
                skills,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
            )
        });
        let score_metric = matches!(self.metric, Metric::Score);
        let mut budget = LuckExactBudget::default();
        let mut outcomes = Vec::with_capacity(ORDERS);
        let mut support = BTreeMap::new();
        let mut best = None::<i128>;
        self.tel.leaves.started += 1;
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        for (index, order) in uniform::all_orders().into_iter().enumerate() {
            if self.expired() {
                self.rec.clock.lap(resume);
                return Ok(Leaf::Stopped);
            }
            if score_metric && best.is_some_and(|best| caps.as_ref().is_some_and(|caps| caps[index] < best)) {
                continue;
            }
            let performers = order.map(|slot| input.performers[slot].clone());
            let mut ceiling =
                if score_metric { caps.as_ref().and_then(|caps| i32::try_from(caps[index]).ok()) } else { None };
            if score_metric && let Some(bounds) = &mut score_bounds {
                // Optional bounds are used only to close the search when a simulated path attains them.
                if let Ok(Some(summary)) = bounds.summary(&performers, None, || self.expired()) {
                    ceiling = Some(ceiling.map_or(summary.final_support.upper, |c| c.min(summary.final_support.upper)));
                }
                if self.expired() {
                    self.rec.clock.lap(resume);
                    return Ok(Leaf::Stopped);
                }
            }
            let result = exact.support(&performers, &mut budget, ceiling, || self.expired())?;
            self.tel.leaves.simulations += result.stats.replay_runs;
            if let Some(reason) = result.decline {
                if reason != LuckExactDecline::Cancelled || self.stop.is_none() {
                    self.stop = Some(ExitReason::RefinementRequired);
                }
                self.rec.clock.lap(resume);
                return Ok(Leaf::Stopped);
            }
            let mut chosen = None;
            for (score, life) in result.outcomes {
                support.insert(score, 1);
                let payoff = self.payoff(physical, score, power, Some(life))?;
                if chosen.is_none_or(|(previous, previous_score)| (payoff, score) > (previous, previous_score)) {
                    chosen = Some((payoff, score));
                }
            }
            let (payoff, score) = chosen.ok_or_else(|| Error::Domain("reachable outcome set is empty".into()))?;
            best = Some(best.map_or(payoff, |current| current.max(payoff)));
            outcomes.push(SeedOutcome {
                root_seed: 0,
                weight: 1,
                performance_order: order,
                final_score: score,
                terminal_payoff: payoff,
            });
        }
        self.rec.clock.lap(resume);
        let mut evaluation = expectation::aggregate(outcomes)?;
        evaluation.expected_payoff = ExactExpectation { numerator: best.expect("performance orders"), denominator: 1 };
        evaluation.score_mass = support;
        Ok(Leaf::Evaluated(evaluation))
    }

    /// Completed zero-draw orders have a singleton score/life support. Their common prefixes and score
    /// programs can be shared without changing the maximum over orders. Any observed random draw declines
    /// this path before an evaluation or cache entry is published.
    fn evaluate_maximum_deterministic(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        input: &FiniteSeedContext,
        caps: Option<&[i128]>,
        cut: Option<(&JointBounds, &CandidateDomain)>,
    ) -> Result<Option<Leaf>, Error> {
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
            return Ok(Some(Leaf::Evaluated(aggregate_maximum(outcomes)?)));
        }
        let performers = &input.performers;
        let cached = self.programs.get_partial(physical.members, performers, power, &mut self.tel.caches.programs);
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
            return Ok(Some(Leaf::Evaluated(evaluation)));
        }
        let threshold = if self.top.len() == self.request.k {
            self.top.last().map(|last| {
                let threshold = last.evaluation.expected_payoff.numerator;
                if power < last.power { threshold.saturating_add(1) } else { threshold }
            })
        } else {
            None
        };
        let stop_below = threshold.filter(|&threshold| cached_best < threshold).unwrap_or(i128::MIN);
        let caps = caps.map(<[i128]>::to_vec).unwrap_or_else(|| vec![self.metric.upper().unwrap_or(i128::MAX); ORDERS]);
        let missing: Vec<usize> = (0..ORDERS).filter(|&index| outcomes[index].is_none()).collect();
        if missing.iter().all(|&index| caps[index] < stop_below) {
            self.tel.leaves.order_bound_pruned += 1;
            return Ok(Some(Leaf::Pruned));
        }
        let performance_orders = self.orders.clone();
        let missing_orders: Vec<[usize; 5]> = missing.iter().map(|&index| performance_orders[index]).collect();
        let orders: Vec<Vec<usize>> = missing_orders.iter().map(|order| order.to_vec()).collect();
        let capture_budget =
            self.programs.capture_budget_for(physical.members, performers, &mut self.tel.caches.program_admissions);
        self.tel.caches.program_bytes = self.programs.allocated_bytes();
        self.tel.caches.program_recordings += u64::from(capture_budget > 0);
        let live = input.clone().into_ordered();
        let random = Cell::new(false);
        let (pool, request, metric, event_input) = (self.pool, self.request, self.metric, self.event_input);
        let master = pool.master;
        let mut visit = |local: usize, model: &LiveModel| -> Result<i128, Error> {
            if model.draws() != 0 {
                random.set(true);
                return Err(Error::Unsupported("terminal order contains random draws".into()));
            }
            let index = missing[local];
            let final_score = model.score();
            final_lives[index] = model.current_life();
            let payoff =
                payoff_of(pool, request, metric, event_input, physical, final_score, power, Some(final_lives[index]))?;
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
        self.tel.leaves.started += 1;
        let recording_started = (capture_budget > 0).then(Instant::now);
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        let upper = |ids: &[usize], model: &LiveModel, settled: Settled| {
            if model.draws() != 0 {
                random.set(true);
                return Ok(ControlFlow::Break(()));
            }
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
                let cap = table
                    .as_ref()
                    .and_then(|table| table.payoff_cap(bounds, settled))
                    .map_or(caps[index], |cap| cap.min(caps[index]));
                maximum = maximum.max(cap);
            }
            self.rec.clock.lap(resume);
            Ok(ControlFlow::Continue(maximum))
        };
        let result = live.simulate_orders_maximum_bounded_recorded_partial(
            master,
            &orders,
            LiveRandom::new(0),
            capture_budget,
            stop_below,
            CUTOFF_EVERY,
            upper,
            &mut visit,
        );
        self.rec.clock.lap(resume);
        if let Some(started) = recording_started {
            self.tel.caches.program_recording_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        if random.get() || (input.lottery_free.is_some() && matches!(&result, Err(Error::Unsupported(_)))) {
            return Ok(None);
        }
        let (outcome, programs) = result?;
        if let Some(programs) = programs {
            self.programs.insert_partial(
                physical.members,
                performers,
                &missing_orders,
                programs,
                &mut self.tel.caches.programs,
            );
            self.tel.caches.program_bytes = self.programs.allocated_bytes();
            (self.tel.caches.program_recorded_nodes, self.tel.caches.program_recorded_bytes) =
                self.programs.recorded_work();
        }
        let (OrdersOutcome::Complete(shared) | OrdersOutcome::Stopped(shared) | OrdersOutcome::Interrupted(shared)) =
            outcome;
        self.tel.leaves.order_tree.add(&shared);
        self.tel.leaves.simulations += (outcomes.iter().flatten().count() - cached_count) as u64;
        Ok(Some(match outcome {
            OrdersOutcome::Complete(_) => {
                let evaluation =
                    aggregate_maximum(outcomes.into_iter().map(|outcome| outcome.expect("completed order")).collect())?;
                self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
                Leaf::Evaluated(evaluation)
            }
            OrdersOutcome::Stopped(shared) => {
                telemetry::record_stop(
                    &mut self.tel.leaves.cutoff,
                    shared.frames as usize,
                    shared.separate_frames as usize,
                );
                self.tel.leaves.order_bound_pruned += 1;
                Leaf::Pruned
            }
            OrdersOutcome::Interrupted(_) => Leaf::Stopped,
        }))
    }
}

pub(super) fn search(
    engine: &mut Engine<'_, '_>,
    domain: &CandidateDomain,
    bounds: Option<&JointBounds>,
) -> Result<(), Error> {
    if let Some(bounds) = bounds {
        let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        visit(0, None, &mut physical, domain, bounds, engine)?;
    } else {
        let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        members_rec(0, 0, &mut physical, domain.members(), domain.required(), domain.leader(), domain.snaps(), engine)?;
    }
    Ok(())
}

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
    if depth > 0 && engine.top.len() == engine.request.k {
        let (upper, power) = bounds.maximum_upper(engine.pool, domain, physical, depth);
        if let Some(last) = engine.top.last()
            && (upper < last.evaluation.expected_payoff.numerator
                || (upper == last.evaluation.expected_payoff.numerator && power < i64::from(last.power)))
        {
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
                .any(|&r| r != member && engine.pool.members[r].character_id == card.character_id)
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
