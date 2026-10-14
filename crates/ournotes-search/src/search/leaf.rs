//! Leaf evaluation of a played-live team: its exact payoff numerator over the 120 performance orders, or a proof that
//! it cannot enter the Top-K.
//!
//! Contract of [`Engine::evaluate_orders`]: given a team in its canonical layout, its power and optionally the joint
//! bounds, return either the exact evaluation (one simulation per performance order, every outcome kept) or `Pruned`
//! only when the team provably ranks after the current K-th (payoff below it, or equal with a smaller power), or
//! `Stopped` when the budget ran out. Per-order caps (`JointBounds::order_cheap_caps`, `tighten_order_caps`) and the
//! cutoff tables are the upper bounds it may use.
//!
//! The orders play as one tree ([`OrderedLive::simulate_orders_bounded`]): the frames before a member acts are the
//! same in every order that agrees on the members that acted, so they play once. A node's bound is the sum, over its
//! orders, of the order's cap and of its cutoff table at the node's settled prefix: until a member of an open
//! position acts, the node's live is the live of each of its orders, so each table applies to the shared prefix.
use super::*;
use crate::search::certified_search::LuckLaw;
use crate::search::joint::{JointBounds, TeamCaps};
use ournotes_sim::live::full::{LiveModel, OrdersOutcome, Settled};
use ournotes_sim::live::random::LiveRandom;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::Arc;

/// Frames between two bound checks of the node being played.
const CUTOFF_EVERY: usize = 30;

/// Sets of laws whose bounds one domain keeps.
const LAW_BOUNDS: usize = 16;

/// The joint bounds of one domain compiled under the lottery laws of single teams
/// ([`JointBounds::compile_with_laws`]). Bounds compiled under the laws of every order of a team cover that team,
/// and any other team of the domain whose orders have the same laws.
#[derive(Default)]
pub(super) struct LawBounds {
    /// The members and Snaps of the domain.
    members: Vec<usize>,
    snaps: Vec<usize>,
    /// The distinct laws of a team in address order with their bounds (None: they do not compile), the latest used
    /// last.
    sets: Vec<(Vec<LuckLaw>, Option<Rc<JointBounds>>)>,
}

impl LawBounds {
    /// The bounds of `domain` under `laws`, from `compile` when the domain keeps none.
    fn get(
        &mut self,
        domain: &crate::domain::CandidateDomain,
        laws: &[LuckLaw],
        compile: impl FnOnce(&[LuckLaw]) -> Option<JointBounds>,
    ) -> Option<Rc<JointBounds>> {
        if (self.members.as_slice(), self.snaps.as_slice()) != (domain.members(), domain.snaps()) {
            (self.members, self.snaps) = (domain.members().to_vec(), domain.snaps().to_vec());
            self.sets.clear();
        }
        let mut distinct = Vec::<LuckLaw>::new();
        for law in laws {
            if !distinct.iter().any(|other| Arc::ptr_eq(other, law)) {
                distinct.push(law.clone());
            }
        }
        distinct.sort_unstable_by_key(|law| Arc::as_ptr(law) as usize);
        let same = |(set, _): &(Vec<LuckLaw>, Option<Rc<JointBounds>>)| {
            set.len() == distinct.len() && set.iter().zip(&distinct).all(|(a, b)| Arc::ptr_eq(a, b))
        };
        let entry = match self.sets.iter().position(same) {
            Some(at) => self.sets.remove(at),
            None => {
                if self.sets.len() == LAW_BOUNDS {
                    self.sets.remove(0);
                }
                let bounds = compile(&distinct).map(Rc::new);
                (distinct, bounds)
            }
        };
        let bounds = entry.1.clone();
        self.sets.push(entry);
        bounds
    }
}

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

impl Engine<'_, '_> {
    /// Whether the caps of a team under the lottery `laws` of its own orders prove it below the K-th of `cutoff`
    /// (its payoff numerator and power): the caps of the domain's bounds compiled under those laws
    /// ([`JointBounds::team_below`]). False when the laws compile no bounds.
    fn below_under_laws(
        &mut self,
        domain: &crate::domain::CandidateDomain,
        physical: &PhysicalDeck,
        power: i32,
        laws: &[LuckLaw],
        (threshold, kth_power): (i128, i32),
    ) -> bool {
        let (pool, request, event_input, simulation) = (self.pool, self.request, self.event_input, self.simulation);
        let metric = self.metric;
        let compiled = &mut self.tel.leaves.law_bounds;
        let (_, resume) = self.rec.clock.lap(slot::FINE);
        let bounds = self.certified.as_mut().expect("certified request").law_bounds.get(domain, laws, |laws| {
            *compiled += 1;
            JointBounds::compile_with_laws(pool, request, domain, metric, event_input, simulation, laws, None).ok()
        });
        let below = |total: i128| total < threshold || (total == threshold && power < kth_power);
        let pruned = bounds.is_some_and(|bounds| {
            bounds
                .team_below(
                    domain,
                    physical,
                    i64::from(power),
                    &self.positions,
                    &mut self.bound_scratch,
                    below,
                    (&mut self.tel.leaves.order_sums, &mut self.tel.leaves.fine_orders),
                )
                .is_some()
        });
        self.rec.clock.lap(resume);
        pruned
    }

    /// Whether a played-live team of a certified evaluation, whose power is at most `upper`, is below the K-th by its
    /// fine caps summed over the performance orders at the power `upper` ([`JointBounds::fine_upper_orders`]): the
    /// caps do not decrease with the power, so the team's own sum is at most that one, and a sum strictly below the
    /// K-th payoff needs no power tie-break. This check runs before the team's power is computed.
    pub(super) fn below_at_power_bound(
        &mut self,
        physical: &PhysicalDeck,
        upper: i64,
        (bounds, domain): (&JointBounds, &crate::domain::CandidateDomain),
    ) -> bool {
        if self.certified.is_none() || !self.live || !bounds.has_fine() {
            return false;
        }
        let Some((threshold, _)) = self.safe_cutoff() else { return false };
        let below = |total: i128| total < threshold;
        let (_, resume) = self.rec.clock.lap(slot::FINE);
        let sum =
            bounds.fine_upper_orders(domain, physical, upper, &self.positions, &mut self.bound_scratch, Some(&below));
        self.rec.clock.lap(resume);
        let Some(sum) = sum else { return false };
        self.tel.leaves.order_sums += 1;
        let pruned = below(sum);
        self.tel.leaves.fine_pruned += u64::from(pruned);
        pruned
    }

    /// The laws of the orders of `physical` read from the multiset of its performers' lottery kinds, when the
    /// traversal of a search whose bounds weigh laws holds the census of its domain (see `LotteryCensus::team_laws`).
    fn kind_laws(&self, physical: &PhysicalDeck) -> Option<Vec<LuckLaw>> {
        if !self.metric.weighs_laws() {
            return None;
        }
        self.certified.as_ref()?.census.as_ref()?.team_laws(physical)
    }

    /// Whether the caps of a team under the laws read from its multiset of lottery kinds prove it below the K-th of
    /// `cutoff` ([`Engine::below_under_laws`]), in a traversal whose bounds `traversal` weigh the laws of its domain.
    /// Those are the laws of its own orders, which weigh no law of another team of the domain. False without such
    /// laws.
    fn below_under_kind_laws(
        &mut self,
        domain: &crate::domain::CandidateDomain,
        traversal: &JointBounds,
        physical: &PhysicalDeck,
        power: i32,
        cutoff: (i128, i32),
    ) -> bool {
        if traversal.laws().is_empty() {
            return false;
        }
        let Some(laws) = self.kind_laws(physical) else { return false };
        self.tel.leaves.kind_laws += 1;
        self.below_under_laws(domain, physical, power, &laws, cutoff)
    }

    /// The laws read from a team's multiset of lottery kinds can prune the teams that hold the same multiset in a
    /// traversal whose bounds `traversal` weigh the laws of its domain: they must cover the laws `found` of the
    /// team's own orders. An error when they do not.
    fn audit_kind_laws(
        &self,
        traversal: &JointBounds,
        physical: &PhysicalDeck,
        found: &Result<Vec<LuckLaw>, Error>,
    ) -> Result<(), Error> {
        if traversal.laws().is_empty() {
            return Ok(());
        }
        let Some(kind) = self.kind_laws(physical) else { return Ok(()) };
        match found {
            Ok(own) if crate::search::snaps::LuckLaws::new(&kind).is_some_and(|laws| laws.cover(own)) => Ok(()),
            _ => Err(Error::Domain("a team's lottery laws are not those of its multiset of lottery kinds".into())),
        }
    }

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
        if self.certified.is_some() {
            // The caps prove a team below the K-th whether or not its scores are cached.
            if let (Some((bounds, domain)), Some((threshold, kth_power))) = (cut, self.safe_cutoff()) {
                let below = |total: i128| total < threshold || (total == threshold && power < kth_power);
                let (_, resume) = self.rec.clock.lap(slot::FINE);
                let pruned = bounds.team_below(
                    domain,
                    physical,
                    i64::from(power),
                    &self.positions,
                    &mut self.bound_scratch,
                    below,
                    (&mut self.tel.leaves.order_sums, &mut self.tel.leaves.fine_orders),
                );
                self.rec.clock.lap(resume);
                match pruned {
                    Some(TeamCaps::Cheap) => self.tel.leaves.cheap_pruned += 1,
                    Some(TeamCaps::Fine) => self.tel.leaves.fine_pruned += 1,
                    None => {}
                }
                if pruned.is_some() {
                    return Ok(Leaf::Pruned);
                }
                if self.below_under_kind_laws(domain, bounds, physical, power, (threshold, kth_power)) {
                    self.tel.leaves.law_pruned += 1;
                    return Ok(Leaf::Pruned);
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
            let program = crate::search::certified_search::canonicalize_performers(&mut input);
            let master = self.pool.master;
            let score = if let Some(score) = self.cached_certified_score(&program, power) {
                score
            } else {
                let skills = self.certified_luck_skills()?;
                let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
                // The laws of the team's own orders: caps under them can prove the team below the K-th, and the
                // evaluation takes them as they are. Bounds that weigh the laws of the team's domain must cover them.
                let mut laws = None;
                let cutoff = self.safe_cutoff();
                if let Some((bounds, domain)) = cut
                    && self.metric.weighs_laws()
                    && (cutoff.is_some() || !bounds.laws().is_empty())
                {
                    let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                    let mut kinds = self.take_luck_kinds();
                    let found = crate::search::certified_search::luck_order_laws(
                        master,
                        &skills,
                        &input,
                        &mut curves,
                        Some(&mut kinds),
                    );
                    self.keep_luck_kinds(kinds);
                    self.rec.clock.lap(resume);
                    if !bounds.laws().is_empty() {
                        match &found {
                            Ok(own) if bounds.cover_laws(own) => self.tel.leaves.law_audits += 1,
                            Ok(_) => {
                                return Err(Error::Domain(
                                    "a team's lottery laws are not among the laws its domain's bounds weigh".into(),
                                ));
                            }
                            Err(error) => {
                                return Err(Error::Domain(format!(
                                    "a team under the lottery laws of its domain has no laws of its own: {error}"
                                )));
                            }
                        }
                    }
                    self.audit_kind_laws(bounds, physical, &found)?;
                    laws = found.ok();
                    if let (Some(laws), Some(cutoff)) = (&laws, cutoff)
                        && self.below_under_laws(domain, physical, power, laws, cutoff)
                    {
                        self.tel.caches.luck_curves.record(curves.stats());
                        self.certified.as_mut().expect("certified request").luck_curves = curves;
                        self.tel.leaves.law_pruned += 1;
                        return Ok(Leaf::Pruned);
                    }
                }
                self.tel.leaves.started += 1;
                let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                let score_map = crate::search::certified_search::PayoffMap::Score;
                let result = match crate::search::certified_search::evaluate_luck_exact(
                    master,
                    &skills,
                    &input,
                    &score_map,
                    laws.as_deref(),
                    Some(&mut curves),
                    || self.expired(),
                ) {
                    Ok(result) => {
                        self.tel.leaves.exact_teams += u64::from(result.is_some());
                        Ok(result.map(|(evaluation, report)| {
                            self.tel.leaves.record_exact(report);
                            evaluation
                        }))
                    }
                    Err(error) => {
                        *self.tel.leaves.exact_declined.entry(error.to_string()).or_default() += 1;
                        crate::search::certified_search::evaluate_luck_context(
                            master,
                            &skills,
                            &input,
                            &score_map,
                            Some(&mut curves),
                            || self.expired(),
                        )
                    }
                };
                self.rec.clock.lap(resume);
                self.tel.caches.luck_curves.record(curves.stats());
                self.certified.as_mut().expect("certified request").luck_curves = curves;
                let Some(score) = result? else {
                    return Ok(Leaf::Stopped);
                };
                self.tel.leaves.simulations += ORDERS as u64;
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
        let kth = self.safe_cutoff();
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
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        let spliced = self.spliced_scores(&input, &missing_orders);
        self.rec.clock.lap(resume);
        if let Some(scores) = spliced? {
            for (&i, final_score) in missing.iter().zip(scores) {
                outcomes[i] = Some(SeedOutcome {
                    root_seed: 0,
                    weight: 1,
                    performance_order: performance_orders[i],
                    final_score,
                    terminal_payoff: payoff_of(pool, request, metric, event_input, physical, final_score, power, None)?,
                });
            }
            let evaluation =
                expectation::aggregate(outcomes.into_iter().map(|o| o.expect("every order evaluated")).collect())?;
            // The metric reads no final life (`spliced_scores`): the lives of the spliced orders stay unknown.
            self.team_scores.insert(physical, power, &evaluation, &final_lives, &mut self.tel.caches.team_scores);
            return Ok(Leaf::Evaluated(evaluation));
        }
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

    /// The native scores of `orders` of a Gekisou team in a live without a LUCK range, spliced from the recordings of
    /// its five rotations ([`ournotes_sim::live::full::lottery_free_orders`]). None when the live has no Gekisou or a
    /// LUCK range, when the metric reads the final life (the splice follows scores only), or when the splice does not
    /// cover the orders; they are then simulated.
    fn spliced_scores(
        &mut self,
        input: &expectation::FiniteSeedContext,
        orders: &[[usize; 5]],
    ) -> Result<Option<Vec<i32>>, Error> {
        let Some(setup) = &input.gekisou else { return Ok(None) };
        if self.metric.any(|m| matches!(m, Metric::ScoreAndLifeAtLeast { .. })) {
            return Ok(None);
        }
        if self.lottery_free.is_none() && self.splice_skills.is_none() {
            self.splice_skills = Some(Arc::new(ournotes_sim::live::full::luck_skills(self.pool.master)?));
        }
        let skills = self.lottery_free.as_ref().or(self.splice_skills.as_ref()).expect("filled above").clone();
        let spliced = ournotes_sim::live::full::lottery_free_orders(
            self.pool.master,
            &skills,
            &input.performers,
            &input.notes,
            &input.events,
            input.params,
            setup,
            &input.play,
            &input.delta_times,
            input.rank_confirmations.as_deref(),
            orders,
        );
        match spliced {
            Ok(scores) => {
                self.tel.leaves.free_spliced += 1;
                Ok(Some(scores))
            }
            Err(Error::Unsupported(why) | Error::Capacity(why)) => {
                *self.tel.leaves.free_unspliced.entry(why).or_default() += 1;
                Ok(None)
            }
            Err(error) => Err(error),
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
