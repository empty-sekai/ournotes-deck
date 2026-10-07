//! Leaf evaluation of a played-live team: its exact payoff numerator over the 120 performance orders, or a proof that
//! it cannot enter the Top-K.
//!
//! Contract of [`Engine::evaluate_orders`]: given a team in its canonical layout, its power and optionally the joint
//! bounds, return a complete evaluation (finite payoffs or certified LUCK intervals for all performance orders),
//! or `Pruned` only when the team provably ranks after the current K-th (payoff below it, or equal with a smaller
//! power), or `Stopped` when the budget ran out. Original per-order caps, admitted terminal Rush mean caps for
//! Score and the cutoff tables may prove exclusion; an upper-only preparation never supplies a candidate value.
//!
//! The orders play as one tree ([`OrderedLive::simulate_orders_bounded`]): the frames before a member acts are the
//! same in every order that agrees on the members that acted, so they play once. A node's bound is the sum, over its
//! orders, of the order's cap and of its cutoff table at the node's settled prefix: until a member of an open
//! position acts, the node's live is the live of each of its orders, so each table applies to the shared prefix.
use super::*;
use crate::search::certified_search::{LuckContextEvent, LuckContextOutcome};
use ournotes_sim::live::full::{LiveModel, LuckRushPreparation, OrdersOutcome, Settled};
use ournotes_sim::live::random::LiveRandom;
use std::ops::ControlFlow;

/// Frames between two bound checks of the node being played.
const CUTOFF_EVERY: usize = 30;

/// How the evaluation of a team's performance orders ended.
pub(super) enum Leaf {
    Evaluated(FiniteEvaluation),
    Certified(
        crate::search::certified_search::CertifiedEvaluation,
        Vec<u8>,
        crate::search::certified_search::PayoffMap,
    ),
    /// Provably below the K-th: not a Top-K deck.
    Pruned,
    /// The budget ran out.
    Stopped,
}

/// Saturating sum of payoff caps.
fn cap_sum(caps: &[i128]) -> i128 {
    caps.iter().fold(0i128, |a, &c| a.saturating_add(c))
}

/// Integer caps on each order's expected score, indexed in the canonical performer basis.
/// The sum bounds 120 times the team's expected score, including every unfinished order.
struct CertifiedOrderCutoff {
    caps: Vec<i128>,
    threshold: i128,
    power: i32,
    kth_power: i32,
}

fn canonical_order_caps(caps: &[i128], basis: [usize; 5]) -> Vec<i128> {
    uniform::all_orders().iter().map(|order| caps[uniform::order_index(&order.map(|slot| basis[slot]))]).collect()
}

/// Event positions in the original physical deck for each canonical performer order.
fn canonical_order_positions(basis: [usize; 5]) -> Vec<[usize; 5]> {
    uniform::all_orders().iter().map(|order| uniform::positions_of(&order.map(|slot| basis[slot]))).collect()
}

impl CertifiedOrderCutoff {
    fn offer(&mut self, index: usize, upper: f64) -> bool {
        // The admitted score route is nonnegative and its score support is i32. Keep the original
        // cap if a provider cannot supply this conversion certificate.
        if !(0.0..=f64::from(i32::MAX)).contains(&upper) {
            return false;
        }
        self.caps[index] = self.caps[index].min(upper.ceil() as i128);
        let total = cap_sum(&self.caps);
        total < self.threshold || (total == self.threshold && self.power < self.kth_power)
    }
}

impl Engine<'_, '_> {
    /// The payoff numerator of a team over the performance orders: each order simulated once, the sum over all of
    /// them. With a full Top-K and bounds, the team's per-order caps (cheap, then raw and fine) are summed first; the
    /// orders then run in descending cap order, each with a cutoff table when the fine bound is compiled. The team
    /// is dropped as soon as the exact payoffs of the orders run plus the caps of the others fall below the K-th (an
    /// equal total with a smaller power too, which keeps every power/ID tie).
    pub(super) fn evaluate_orders(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        cut: Option<(&crate::search::joint::JointBounds, &crate::domain::CandidateDomain)>,
    ) -> Result<Leaf, Error> {
        if matches!(self.metric, Metric::BestOrderExpectedScore) {
            return self.evaluate_best_expected_order(physical, power, cut);
        }
        if self.certified.is_some() {
            let mut score_cutoff = None;
            // The caps prove a team below the K-th whether or not its scores are cached.
            if let (Some((bounds, domain)), Some((threshold, kth_power))) = (cut, self.safe_cutoff()) {
                let below = |total: i128| total < threshold || (total == threshold && power < kth_power);
                let mut caps = bounds.order_cheap_caps(domain, physical, i64::from(power), &self.positions);
                if below(cap_sum(&caps)) {
                    self.tel.leaves.cheap_pruned += 1;
                    return Ok(Leaf::Pruned);
                }
                if bounds.has_fine() {
                    let (_, resume) = self.rec.clock.lap(slot::FINE);
                    let pruned = bounds.tighten_order_caps_until(
                        domain,
                        physical,
                        i64::from(power),
                        &self.positions,
                        &mut caps,
                        &mut self.bound_scratch,
                        below,
                        &mut self.tel.leaves.fine_orders,
                    );
                    self.rec.clock.lap(resume);
                    if pruned {
                        self.tel.leaves.fine_pruned += 1;
                        return Ok(Leaf::Pruned);
                    }
                }
                if matches!(self.metric, crate::types::Metric::Score) {
                    score_cutoff = Some((caps, threshold, kth_power));
                }
            }
            let mut input = expectation::context(self.pool, physical, &self.request.objective)?;
            if let Some(v) = self.simulation.music_length_ms {
                input.params.music_length_ms = v;
            }
            if let Some(v) = self.simulation.score_music_length_ms {
                input.params.score_music_length_ms = Some(v);
            }
            self.admit_certified_refinement(input.notes.len(), input.play.frames.len());
            let (program, basis) = crate::search::certified_search::canonicalize_performers_with_basis(&mut input);
            if matches!(self.metric, crate::types::Metric::Score)
                && let Some((threshold, kth_power)) = self.safe_cutoff()
                && self
                    .cached_certified_score_cap(&program, power)
                    .is_some_and(|cap| cap < threshold || (cap == threshold && power < kth_power))
            {
                self.tel.leaves.order_bound_pruned += 1;
                return Ok(Leaf::Pruned);
            }
            let master = self.pool.master;
            let score = if let Some(score) = self.cached_certified_score(&program, power) {
                score
            } else {
                self.tel.leaves.started += 1;
                let skills = self.certified_luck_skills()?;
                let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
                let mut order_cutoff = score_cutoff.map(|(caps, threshold, kth_power)| {
                    let canonical_caps = canonical_order_caps(&caps, basis);
                    CertifiedOrderCutoff { caps: canonical_caps, threshold, power, kth_power }
                });
                let mut schedule: Vec<_> = (0..ORDERS).collect();
                if let Some(cutoff) = &order_cutoff {
                    // This changes only the evaluation sequence. Every unfinished order
                    // keeps its proved cap, and complete aggregation restores the canonical order.
                    schedule.sort_by(|&a, &b| cutoff.caps[b].cmp(&cutoff.caps[a]).then(a.cmp(&b)));
                }
                let prepare_upper =
                    order_cutoff.is_some() && cut.is_some_and(|(bounds, _)| bounds.supports_rush_mean_upper());
                // `basis[canonical slot]` is the original physical slot. The exact native DP deck and the fine
                // cap must put that same complete performer, including its Snap, at the same event position.
                let physical_positions = canonical_order_positions(basis);
                let mut bound_scratch = std::mem::take(&mut self.bound_scratch);
                let mut upper_work = telemetry::LotteryUpper::default();
                let mut completed_orders = 0u64;
                let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                let complete_terminal = matches!(self.metric, Metric::Score);
                let result = crate::search::certified_search::evaluate_luck_context_bounded_policy(
                    master,
                    &skills,
                    &input,
                    &crate::search::certified_search::PayoffMap::Score,
                    Some(&mut curves),
                    complete_terminal,
                    (&schedule, prepare_upper, |event| match event {
                        LuckContextEvent::UpperAttempt => {
                            upper_work.attempted_orders += 1;
                            true
                        }
                        LuckContextEvent::UpperPrepared { index, preparation } => match preparation {
                            LuckRushPreparation::Ready(terminal) => {
                                upper_work.prepared_orders += 1;
                                let (bounds, domain) = cut.expect("Score prepass requires compiled bounds");
                                let Some(upper) = bounds.rush_mean_upper(
                                    domain,
                                    physical,
                                    i64::from(power),
                                    &physical_positions[index],
                                    &mut bound_scratch,
                                    terminal,
                                ) else {
                                    upper_work.incompatible_caps += 1;
                                    return true;
                                };
                                upper_work.bounded_orders += 1;
                                let cutoff = order_cutoff.as_mut().expect("Score prepass requires a cutoff");
                                if upper.ceil() < cutoff.caps[index] as f64 {
                                    upper_work.tightened_orders += 1;
                                }
                                let excluded = cutoff.offer(index, upper);
                                upper_work.pruned_teams += u64::from(excluded);
                                !excluded
                            }
                            LuckRushPreparation::Unavailable { reason, .. } => {
                                upper_work.declined_orders += 1;
                                upper_work.declines.record(*reason);
                                true
                            }
                            LuckRushPreparation::Stopped => {
                                upper_work.stopped_orders += 1;
                                true
                            }
                        },
                        LuckContextEvent::UpperFinished { elapsed_ms } => {
                            upper_work.elapsed_ms += elapsed_ms;
                            true
                        }
                        LuckContextEvent::Scored { index, order } => {
                            completed_orders += 1;
                            !order_cutoff.as_mut().is_some_and(|cutoff| cutoff.offer(index, order.mean.upper()))
                        }
                    }),
                    || self.expired(),
                );
                self.rec.clock.lap(resume);
                self.bound_scratch = bound_scratch;
                self.tel.leaves.lottery_upper.add(upper_work);
                self.tel.leaves.simulations += completed_orders;
                self.tel.caches.luck_curves.record(curves.stats());
                self.certified.as_mut().expect("certified request").luck_curves = curves;
                let score = match result? {
                    LuckContextOutcome::Full(score) => score,
                    LuckContextOutcome::UpperOnly => {
                        self.cache_certified_score_cap(
                            program,
                            power,
                            cap_sum(&order_cutoff.as_ref().expect("certified exclusion").caps),
                        );
                        self.tel.leaves.order_bound_pruned += 1;
                        return Ok(Leaf::Pruned);
                    }
                    LuckContextOutcome::Stopped => return Ok(Leaf::Stopped),
                };
                self.cache_certified_score(program.clone(), power, &score);
                score
            };
            let support = (
                score.orders.iter().map(|o| o.support.0).min().expect("120 orders"),
                score.orders.iter().map(|o| o.support.1).max().expect("120 orders"),
            );
            let map = crate::search::certified_payoff::payoff_map(
                self.pool,
                self.request,
                self.metric,
                self.event_input,
                physical,
                power,
                support,
            )?;
            let evaluation = crate::search::certified_search::aggregate_orders(score.orders, &map)?;
            return Ok(Leaf::Certified(evaluation, program, map));
        }
        if let Some(threshold) = crate::search::snaps::census() {
            return self.census_leaf(physical, power, cut, threshold);
        }
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
            return Ok(Leaf::Evaluated(expectation::aggregate(outcomes)?));
        }
        let mut input = expectation::context(self.pool, physical, &self.request.objective)?;
        if let Some(v) = self.simulation.music_length_ms {
            input.params.music_length_ms = v;
        }
        if let Some(v) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(v);
        }
        input.lottery_free = self.lottery_free.clone();
        let performers = input.performers.clone();
        let cached = self.programs.get_partial(physical.members, &performers, power, &mut self.tel.caches.programs);
        self.tel.caches.program_evaluation_ms = self.programs.evaluation_ms();
        let mut outcomes: Vec<Option<SeedOutcome>> = vec![None; ORDERS];
        let mut final_lives = [0; ORDERS];
        let mut cached_sum = 0i128;
        let mut cached_count = 0usize;
        if let Some(cached) = cached {
            for (i, score) in cached.into_iter().enumerate() {
                if let Some((final_score, final_life)) = score {
                    let payoff = self.payoff(physical, final_score, power, Some(final_life))?;
                    cached_sum += payoff;
                    cached_count += 1;
                    final_lives[i] = final_life;
                    outcomes[i] = Some(SeedOutcome {
                        root_seed: 0,
                        weight: 1,
                        performance_order: self.orders[i],
                        final_score,
                        terminal_payoff: payoff,
                    });
                }
            }
            self.tel.caches.program_orders_reused += cached_count as u64;
        }
        if cached_count == ORDERS {
            let evaluation = expectation::aggregate(outcomes.into_iter().map(Option::unwrap).collect())?;
            self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
            return Ok(Leaf::Evaluated(evaluation));
        }
        let kth = (self.top.len() == self.request.k)
            .then(|| self.top.last().map(|e| (e.evaluation.expected_payoff.numerator, e.power)))
            .flatten();
        let below = |total: i128| {
            kth.is_some_and(|(threshold, kth_power)| total < threshold || (total == threshold && power < kth_power))
        };
        let bounds = cut.filter(|_| kth.is_some());
        let caps = match bounds {
            Some((b, domain)) => {
                let mut caps = b.order_cheap_caps(domain, physical, i64::from(power), &self.positions);
                if below(cap_sum(&caps)) {
                    self.tel.leaves.cheap_pruned += 1;
                    return Ok(Leaf::Pruned);
                }
                if b.has_fine() {
                    let (_, resume) = self.rec.clock.lap(slot::FINE);
                    let pruned = b.tighten_order_caps_until(
                        domain,
                        physical,
                        i64::from(power),
                        &self.positions,
                        &mut caps,
                        &mut self.bound_scratch,
                        below,
                        &mut self.tel.leaves.fine_orders,
                    );
                    self.rec.clock.lap(resume);
                    if pruned {
                        self.tel.leaves.fine_pruned += 1;
                        return Ok(Leaf::Pruned);
                    }
                }
                Some(caps)
            }
            None => None,
        };
        // The cap of each order: its own cap, else the metric's (None: unbounded).
        let caps = caps.or_else(|| self.metric.upper().map(|c| vec![c; ORDERS]));
        if caps.as_ref().is_some_and(|caps| {
            below(
                caps.iter().enumerate().fold(cached_sum, |sum, (i, cap)| {
                    if outcomes[i].is_some() { sum } else { sum.saturating_add(*cap) }
                }),
            )
        }) {
            self.tel.leaves.order_bound_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        let fine = bounds.filter(|(b, _)| b.has_fine());
        let (pool, request, metric, event_input) = (self.pool, self.request, self.metric, self.event_input);
        let master = self.pool.master;
        let performance_orders = self.orders.clone();
        let missing: Vec<usize> = (0..ORDERS).filter(|&i| outcomes[i].is_none()).collect();
        let missing_orders: Vec<[usize; 5]> = missing.iter().map(|&i| performance_orders[i]).collect();
        let orders: Vec<Vec<usize>> = missing_orders.iter().map(|o| o.to_vec()).collect();
        let capture_budget =
            self.programs.capture_budget_for(physical.members, &performers, &mut self.tel.caches.program_admissions);
        self.tel.caches.program_bytes = self.programs.allocated_bytes();
        self.tel.caches.program_recordings += u64::from(capture_budget > 0);
        let live = input.into_ordered();
        let mut visit = |local: usize, model: &LiveModel| -> Result<i128, Error> {
            let i = missing[local];
            if model.draws() != 0 {
                return Err(Error::Unsupported(
                    "a skill or mission of this team draws a lottery; the uniform member-order target covers \
                     lottery-free lives only"
                        .into(),
                ));
            }
            let final_score = model.score();
            final_lives[i] = model.current_life();
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
            outcomes[i] = Some(SeedOutcome {
                root_seed: 0,
                weight: 1,
                performance_order: performance_orders[i],
                final_score,
                terminal_payoff: payoff,
            });
            Ok(payoff)
        };
        self.tel.leaves.started += 1;
        let recording_started = (capture_budget > 0).then(Instant::now);
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        let (outcome, programs) = match (caps, kth) {
            (Some(caps), Some((threshold, kth_power))) => {
                // `below(total)` holds exactly when `total < stop_below`
                let stop_below = (if power < kth_power { threshold.saturating_add(1) } else { threshold })
                    .saturating_sub(cached_sum);
                let mut tables: Vec<Option<Option<_>>> = (0..ORDERS).map(|_| None).collect();
                let upper = |ids: &[usize], model: &LiveModel, s: Settled| {
                    if self.expired() {
                        return Ok(ControlFlow::Break(()));
                    }
                    let Some((b, domain)) = fine.filter(|_| model.frames_played() > 0) else {
                        return Ok(ControlFlow::Continue(
                            ids.iter().fold(0i128, |a, &local| a.saturating_add(caps[missing[local]])),
                        ));
                    };
                    let (_, resume) = self.rec.clock.lap(slot::CUTOFF_TABLE);
                    let mut sum = 0i128;
                    for &local in ids {
                        let i = missing[local];
                        let table = tables[i].get_or_insert_with(|| {
                            let t = b.cutoff_table(
                                domain,
                                physical,
                                i64::from(power),
                                &self.positions[i],
                                &mut self.bound_scratch,
                            );
                            self.tel.leaves.cutoff.tables += u64::from(t.is_some());
                            self.tel.leaves.cutoff.unavailable += u64::from(t.is_none());
                            t
                        });
                        let cap = table.as_ref().and_then(|t| t.payoff_cap(b, s)).map_or(caps[i], |c| c.min(caps[i]));
                        sum = sum.saturating_add(cap);
                    }
                    self.rec.clock.lap(resume);
                    Ok(ControlFlow::Continue(sum))
                };
                live.simulate_orders_bounded_recorded_partial(
                    master,
                    &orders,
                    LiveRandom::new(0),
                    capture_budget,
                    stop_below,
                    CUTOFF_EVERY,
                    upper,
                    &mut visit,
                )?
            }
            _ => {
                let (shared, programs) =
                    live.simulate_orders_recorded(master, &orders, LiveRandom::new(0), capture_budget, |i, m| {
                        visit(i, m).map(|_| ())
                    })?;
                (OrdersOutcome::Complete(shared), programs)
            }
        };
        if let Some(started) = recording_started {
            self.tel.caches.program_recording_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
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
        let (OrdersOutcome::Complete(shared) | OrdersOutcome::Stopped(shared) | OrdersOutcome::Interrupted(shared)) =
            outcome;
        self.tel.leaves.order_tree.add(&shared);
        self.tel.leaves.simulations += (outcomes.iter().filter(|o| o.is_some()).count() - cached_count) as u64;
        match outcome {
            OrdersOutcome::Complete(_) => {
                self.rec.clock.lap(resume);
                let evaluation =
                    expectation::aggregate(outcomes.into_iter().map(|o| o.expect("every order evaluated")).collect())?;
                self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
                Ok(Leaf::Evaluated(evaluation))
            }
            OrdersOutcome::Stopped(s) => {
                self.rec.clock.lap_as(slot::STOPPED, resume);
                telemetry::record_stop(&mut self.tel.leaves.cutoff, s.frames as usize, s.separate_frames as usize);
                self.tel.leaves.order_bound_pruned += 1;
                Ok(Leaf::Pruned)
            }
            OrdersOutcome::Interrupted(_) => {
                self.rec.clock.lap(resume);
                Ok(Leaf::Stopped)
            }
        }
    }

    /// Census leaf (`snaps::census`): the team's cheap, then raw and fine caps against the fixed threshold; a team
    /// they leave at or above it is counted, not simulated.
    fn census_leaf(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        cut: Option<(&crate::search::joint::JointBounds, &crate::domain::CandidateDomain)>,
        threshold: i128,
    ) -> Result<Leaf, Error> {
        let below = |total: i128| total < threshold;
        let census = self.tel.leaves.census.get_or_insert_with(|| telemetry::Census::new(threshold));
        let Some((b, domain)) = cut else {
            census.unbounded += 1;
            return Ok(Leaf::Pruned);
        };
        let mut caps = b.order_cheap_caps(domain, physical, i64::from(power), &self.positions);
        let cheap = cap_sum(&caps);
        if below(cheap) {
            self.tel.leaves.cheap_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        if b.has_fine() {
            let (_, resume) = self.rec.clock.lap(slot::FINE);
            let pruned = b.tighten_order_caps_until(
                domain,
                physical,
                i64::from(power),
                &self.positions,
                &mut caps,
                &mut self.bound_scratch,
                below,
                &mut self.tel.leaves.fine_orders,
            );
            self.rec.clock.lap(resume);
            if pruned {
                self.tel.leaves.fine_pruned += 1;
                return Ok(Leaf::Pruned);
            }
        }
        let fine = cap_sum(&caps);
        let team = telemetry::CensusTeam {
            members: physical.members.map(|i| self.pool.members[i].id),
            snaps: physical.snaps.map(|i| i.map(|i| self.pool.snaps[i].id)),
            power,
            cheap: cheap.to_string(),
            fine: fine.to_string(),
        };
        let census = self.tel.leaves.census.as_mut().expect("census started");
        census.record(physical.members, team, cheap, fine);
        Ok(Leaf::Pruned)
    }
}

impl Engine<'_, '_> {
    fn evaluate_best_expected_order(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        cut: Option<(&crate::search::joint::JointBounds, &crate::domain::CandidateDomain)>,
    ) -> Result<Leaf, Error> {
        use crate::search::certified_search::{PayoffMap, aggregate_orders, canonicalize_performers_with_basis};
        let cutoff = self.safe_cutoff();
        let below = |cap: i128| {
            cutoff.is_some_and(|(threshold, kth_power)| cap < threshold || (cap == threshold && power < kth_power))
        };
        let max_grid = |caps: &[i128]| caps.iter().copied().max().unwrap_or(i128::MAX).saturating_mul(ORDERS as i128);
        let mut caps = match cut {
            Some((bounds, domain)) => bounds.order_cheap_caps(domain, physical, i64::from(power), &self.positions),
            None => vec![i128::from(i32::MAX); ORDERS],
        };
        if below(max_grid(&caps)) {
            self.tel.leaves.cheap_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        if let Some((bounds, domain)) = cut
            && bounds.has_fine()
            && cutoff.is_some()
        {
            let (_, resume) = self.rec.clock.lap(slot::FINE);
            let pruned = bounds.tighten_order_caps_until(
                domain,
                physical,
                i64::from(power),
                &self.positions,
                &mut caps,
                &mut self.bound_scratch,
                below,
                &mut self.tel.leaves.fine_orders,
            );
            self.rec.clock.lap(resume);
            if pruned {
                self.tel.leaves.fine_pruned += 1;
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
        self.admit_certified_refinement(input.notes.len(), input.play.frames.len());
        let (program, basis) = canonicalize_performers_with_basis(&mut input);
        if self.cached_certified_score_cap(&program, power).is_some_and(below) {
            self.tel.leaves.order_bound_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        let map = PayoffMap::BestOrderExpectedScore;
        let evaluation = if let Some(cached) = self.cached_certified_score(&program, power) {
            let orders = cached
                .orders
                .into_iter()
                .map(|mut order| {
                    order.order = order.order.map(|slot| basis[slot]);
                    order
                })
                .collect();
            aggregate_orders(orders, &map)?
        } else {
            let skills = if self.lottery_mode == certified_engine::LotteryMode::Certified {
                Some(self.certified_luck_skills()?)
            } else {
                None
            };
            let mut curves = std::mem::take(&mut self.certified.as_mut().expect("best-order request").luck_curves);
            let master = self.pool.master;
            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
            self.tel.leaves.started += 1;
            let result = crate::search::certified_search::evaluate_best_order_context(
                master,
                skills.as_deref(),
                &input,
                basis,
                &canonical_order_caps(&caps, basis),
                cutoff.map(|(threshold, kth_power)| (threshold, power, kth_power)),
                Some(&mut curves),
                || self.expired(),
            );
            self.rec.clock.lap(resume);
            self.tel.caches.luck_curves.record(curves.stats());
            self.certified.as_mut().expect("best-order request").luck_curves = curves;
            let Some(evaluation) = result? else { return Ok(Leaf::Stopped) };
            self.tel.leaves.simulations += evaluation.orders.iter().filter(|order| order.evaluated).count() as u64;
            let mut inverse = [0; 5];
            for (canonical, physical) in basis.into_iter().enumerate() {
                inverse[physical] = canonical;
            }
            let canonical = evaluation
                .orders
                .iter()
                .cloned()
                .map(|mut order| {
                    order.order = order.order.map(|slot| inverse[slot]);
                    order
                })
                .collect();
            self.cache_certified_score(program.clone(), power, &aggregate_orders(canonical, &map)?);
            evaluation
        };
        let cap = (evaluation.score.upper().ceil() as i128).saturating_mul(ORDERS as i128);
        self.cache_certified_score_cap(program.clone(), power, cap);
        if below(cap) {
            self.tel.leaves.order_bound_pruned += 1;
            return Ok(Leaf::Pruned);
        }
        Ok(Leaf::Certified(evaluation, program, map))
    }
}

#[cfg(test)]
mod certified_cutoff_tests {
    use super::*;

    #[test]
    fn every_performer_basis_preserves_unequal_physical_order_caps_and_positions() {
        let powers = [2i128, 3, 5, 7, 11];
        let positions = [13i128, 17, 19, 23, 29];
        let orders = uniform::all_orders();
        let caps: Vec<_> = orders
            .iter()
            .map(|order| order.iter().enumerate().map(|(position, &slot)| powers[slot] * positions[position]).sum())
            .collect();
        for basis in &orders {
            let remapped = canonical_order_caps(&caps, *basis);
            let physical_positions = canonical_order_positions(*basis);
            let canonical_powers = basis.map(|slot| powers[slot]);
            for ((order, actual), at) in orders.iter().zip(remapped).zip(physical_positions) {
                let expected: i128 = order
                    .iter()
                    .enumerate()
                    .map(|(position, &slot)| canonical_powers[slot] * positions[position])
                    .sum();
                assert_eq!(actual, expected);
                // The native DP uses canonical performers; FineView uses physical member/Snap pairs. Both
                // must place the same complete performer at every event position for all 120 orders.
                for (position, &slot) in order.iter().enumerate() {
                    assert_eq!(at[basis[slot]], position);
                }
                let physical: i128 = (0..5).map(|slot| powers[slot] * positions[at[slot]]).sum();
                assert_eq!(actual, physical);
            }
        }
    }

    #[test]
    fn fractional_order_bounds_keep_every_possible_winner_and_power_tie() {
        // Each order independently pays 99 or 101 with masses 1/4 and 3/4: its exact mean is 100.5.
        // Finishing the other orders at their caps witnesses why a partial low order cannot discard a team.
        for power in [99, 100, 101] {
            let mut cutoff = CertifiedOrderCutoff { caps: vec![101; ORDERS], threshold: 12_060, power, kth_power: 100 };
            for index in 0..ORDERS {
                assert!(!cutoff.offer(index, 100.5), "ceil must preserve the fractional mean");
            }
            let mut cutoff = CertifiedOrderCutoff { caps: vec![101; ORDERS], threshold: 12_000, power, kth_power: 100 };
            for index in 0..ORDERS - 1 {
                assert!(!cutoff.offer(index, 100.0), "an unfinished order can still exceed the cutoff");
            }
            assert_eq!(cutoff.offer(ORDERS - 1, 100.0), power < 100);
        }
    }

    #[test]
    fn a_partial_order_sum_can_certify_exclusion_without_an_aggregate() {
        let mut cutoff =
            CertifiedOrderCutoff { caps: vec![101; ORDERS], threshold: 12_120, power: 100, kth_power: 100 };
        assert!(cutoff.offer(0, 99.5));
        assert_eq!(cutoff.caps[0], 100);
        assert!(cutoff.caps[1..].iter().all(|&cap| cap == 101));
        for unavailable in [f64::NAN, f64::INFINITY, -1.0, f64::from(i32::MAX) + 1.0] {
            let mut fallback =
                CertifiedOrderCutoff { caps: vec![101; ORDERS], threshold: 12_120, power: 100, kth_power: 100 };
            assert!(!fallback.offer(0, unavailable));
            assert!(fallback.caps.iter().all(|&cap| cap == 101));
        }
    }
}
