//! Physical-deck search: deterministic power/skip search and bounded finite-law native
//! physical-deck optimization. Candidate proposals are heuristic; every returned value is
//! evaluated exactly under the declared model. Only exhaustion/proven pruning certifies K.

use super::expectation::{self, FiniteEvaluation, FiniteSeedContext, FiniteSeedLaw, PhysicalDeck, SeedOutcome};
use super::telemetry::{self, Recorder, Telemetry, Traversal, slot};
use super::{Completion, Objective, Pool, SearchRequest};
use crate::clock::Instant;
use crate::handler::{BuiltProblem, build_card_pool, reject_unsupported_lifecycle, validate};
use crate::types::*;
use ournotes_sim::replay::RankConfirmation;
use ournotes_sim::scenario::EventPayoffInput;
use ournotes_sim::{Error, cards::Roster, data::DeckData};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::time::Duration;

#[path = "composition.rs"]
mod composition;
#[path = "progress.rs"]
mod progress;
#[path = "session.rs"]
mod session;
#[path = "warm.rs"]
mod warm;
pub(crate) use progress::ProgressHook;
pub use session::{
    GOAL_SPEC_VERSION, GoalSpec, SESSION_FORMAT, SearchSession, SessionBinding, SessionProgress, SessionStatus,
    StepBudget,
};

pub(crate) fn session_start_clock() -> Instant {
    super::budget::now()
}

#[derive(Clone)]
struct Entry {
    physical: PhysicalDeck,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    evaluation: FiniteEvaluation,
    network_applications: BTreeMap<i32, Vec<(usize, usize)>>,
    terminal_details: BTreeMap<i32, (i32, u64)>,
}
fn compare(a: &Entry, b: &Entry) -> Ordering {
    b.evaluation
        .expected_payoff
        .numerator
        .cmp(&a.evaluation.expected_payoff.numerator)
        .then_with(|| b.power.cmp(&a.power))
        .then_with(|| a.members.cmp(&b.members))
        .then_with(|| a.snaps.cmp(&b.snaps))
}
impl Entry {
    fn wire(self, metric: &Metric) -> Result<RecommendedDeck, Error> {
        if matches!(metric, Metric::Power) {
            return Ok(RecommendedDeck {
                members: self.members,
                snaps: self.snaps,
                power: self.power,
                expected_score: None,
                expected_payoff: self.evaluation.expected_payoff.into(),
                score_summary: None,
                atoms: Vec::new(),
            });
        }
        let applications = self.network_applications;
        let details = self.terminal_details;
        let score_summary = score_summary(&self.evaluation.score_mass, metric.target())?;
        Ok(RecommendedDeck {
            members: self.members,
            snaps: self.snaps,
            power: self.power,
            expected_score: Some(self.evaluation.expected_score.into()),
            expected_payoff: self.evaluation.expected_payoff.into(),
            score_summary: Some(score_summary),
            atoms: self
                .evaluation
                .outcomes
                .into_iter()
                .map(|o| AtomResult {
                    root_seed: o.root_seed,
                    weight: o.weight.to_string(),
                    performance_order: o.performance_order,
                    score: o.final_score,
                    payoff: o.terminal_payoff.to_string(),
                    network_applications: applications.get(&o.root_seed).cloned().unwrap_or_default(),
                    final_life: details.get(&o.root_seed).map(|d| d.0),
                    converted_judgements: details.get(&o.root_seed).map(|d| d.1),
                })
                .collect(),
        })
    }
}

/// Simulation cutoff tables of one candidate: one per distinct native order, and each atom's table index.
struct CutoffPlan {
    tables: Vec<Option<super::joint::CutoffTable>>,
    index: Vec<usize>,
}

/// Frames between two cutoff checks of a running simulation.
const CUTOFF_EVERY: usize = 30;

struct Engine<'a, 'm> {
    pool: &'a Pool<'m>,
    request: &'a SearchRequest,
    law: &'a FiniteSeedLaw,
    metric: &'a Metric,
    event_input: Option<&'a EventPayoffInput>,
    network: Option<&'a [RankConfirmation]>,
    simulation: &'a SimulationInput,
    limits: &'a Limits,
    budget: super::budget::SearchBudget,
    stop: Option<ExitReason>,
    tel: Telemetry,
    rec: Recorder,
    /// Correlated and resource node bounds are worth their cost for the current search part.
    correlated: bool,
    resource: bool,
    bound_scratch: super::snaps::JointScratch,
    luck: Option<super::luck::LuckOracle>,
    bonus_scratch: super::joint::BonusScratch,
    top: Vec<Entry>,
    seen: HashSet<PhysicalDeck>,
    fifo: VecDeque<PhysicalDeck>,
    input: Option<FiniteSeedContext>,
    song: Option<&'a ournotes_sim::cards::SongView>,
    event: bool,
    skip: Option<&'a ournotes_sim::live::skip::SkipEvaluator>,
    /// Decks the warm start evaluated; the traversal treats them as considered (see `warm.rs`).
    seeded: HashSet<PhysicalDeck>,
    /// Visit order of the next root loop of `joint_rec`; None keeps the static choice order.
    root_order: Option<warm::RootOrder>,
    /// Whole-domain context of the warm start and of polishing new best decks (joint searches only).
    warm: Option<warm::Warm<'a>>,
    /// Optional progress reports (`progress.rs`).
    progress: Option<progress::Reporter<'a>>,
}
impl Engine<'_, '_> {
    fn expired(&mut self) -> bool {
        if self.stop.is_some() {
            return true;
        }
        if self.budget.deadline().is_none() && self.progress.is_none() {
            return false;
        }
        // One clock read serves the deadline and the progress reports.
        self.expired_at(super::budget::now())
    }
    /// `expired` with a clock value the caller has just read; a progress report is due at the same checks.
    fn expired_at(&mut self, now: Instant) -> bool {
        if self.stop.is_some() {
            return true;
        }
        if self.budget.expired_at(now) {
            self.stop = Some(ExitReason::TimeLimit);
            return true;
        }
        self.progress_at(now);
        false
    }
    fn remember(&mut self, p: PhysicalDeck) {
        if self.limits.cache_entries == 0 {
            return;
        }
        let cache = &mut self.tel.caches.candidates;
        if self.seen.len() >= self.limits.cache_entries
            && let Some(old) = self.fifo.pop_front()
        {
            self.seen.remove(&old);
            cache.evictions += 1;
        }
        self.seen.insert(p);
        self.fifo.push_back(p);
        cache.peak_entries = cache.peak_entries.max(self.seen.len());
    }
    fn payoff(&self, p: &PhysicalDeck, score: i32, power: i32, final_life: Option<i32>) -> Result<i128, Error> {
        match *self.metric {
            Metric::Power => Ok(power as i128),
            Metric::Score => Ok(score as i128),
            Metric::ScoreAtLeast { threshold } => Ok(i128::from(score >= threshold)),
            Metric::CappedScore { threshold } => Ok(score.min(threshold) as i128),
            Metric::ScoreAndLifeAtLeast { threshold, min_final_life } => Ok(i128::from(
                score >= threshold
                    && final_life.ok_or_else(|| Error::Input("terminal life requires played Live".into()))?
                        >= min_final_life,
            )),
            Metric::ClientEventPoints { event_id } => Ok(self
                .request
                .objective
                .context()
                .expect("validated context")
                .preview_event_points(
                    self.pool,
                    &p.as_deck(),
                    self.event_input.expect("validated event input"),
                    event_id,
                    score,
                )?
                .points_for(event_id) as i128),
            Metric::ConditionalClientEventItems { event_id, resource_type, resource_id } => {
                let items = self.request.objective.context().expect("validated context").preview_event_items(
                    self.pool,
                    &p.as_deck(),
                    self.event_input.expect("validated event input"),
                    event_id,
                    score,
                )?;
                ournotes_sim::scenario::item_payoff(&items, event_id, resource_type, resource_id)
            }
        }
    }
    fn consider(&mut self, physical: PhysicalDeck) -> Result<bool, Error> {
        self.consider_with(physical, None)
    }

    /// The simulation cutoff tables of a candidate (None for an order without a finite cap), when the Top-K is full
    /// and the payoff is the score or the bounded event points.
    fn cutoff_tables(
        &mut self,
        bounds: &super::joint::JointBounds,
        domain: &crate::domain::CandidateDomain,
        physical: &PhysicalDeck,
        power: i32,
    ) -> Result<Option<CutoffPlan>, Error> {
        let payoff_bounded = match self.metric {
            Metric::Score => !bounds.is_pt(),
            Metric::ClientEventPoints { .. } => bounds.is_pt(),
            _ => false,
        };
        if !payoff_bounded
            || self.top.len() != self.request.k
            || self.network.is_some()
            || self.simulation.live_finished_from_frame.is_some()
        {
            return Ok(None);
        }
        let mut by_order: Vec<([usize; 5], Option<super::joint::CutoffTable>)> = Vec::new();
        let mut index = Vec::with_capacity(self.law.atoms().len());
        for &(root, _) in self.law.atoms() {
            let positions = super::joint::positions(root)?;
            let i = match by_order.iter().position(|(p, _)| *p == positions) {
                Some(i) => i,
                None => {
                    // The leaf fine bound of this candidate read the relaxed power; its soundness needs it to cover
                    // the exact power that the simulation and these tables use.
                    let (_, relaxed) = bounds.upper(self.pool, domain, physical, 5, &positions);
                    if relaxed < i64::from(power) {
                        return Err(Error::Domain(format!("relaxed power {relaxed} below exact power {power}")));
                    }
                    self.tel.leaves.cutoff.relaxed_power_above += u64::from(relaxed > i64::from(power));
                    let masks = match (&mut self.luck, bounds.rush_eligible()) {
                        (Some(oracle), true) => {
                            oracle.site = super::luck::Site::Cutoff;
                            oracle.masks(self.pool, physical, &positions)?
                        }
                        _ => None,
                    };
                    let table = bounds.cutoff_table(
                        domain,
                        physical,
                        i64::from(power),
                        &positions,
                        &mut self.bound_scratch,
                        masks.as_ref(),
                    );
                    by_order.push((positions, table));
                    by_order.len() - 1
                }
            };
            index.push(i);
        }
        let tables: Vec<_> = by_order.into_iter().map(|(_, t)| t).collect();
        if tables.iter().all(Option::is_none) {
            self.tel.leaves.cutoff.unavailable += 1;
            return Ok(None);
        }
        self.tel.leaves.cutoff.tables += 1;
        Ok(Some(CutoffPlan { tables, index }))
    }

    fn consider_with(
        &mut self,
        physical: PhysicalDeck,
        cut: Option<(&super::joint::JointBounds, &crate::domain::CandidateDomain)>,
    ) -> Result<bool, Error> {
        self.tel.leaves.proposed += 1;
        if self.expired() {
            return Ok(false);
        }
        self.tel.caches.candidates.lookups += 1;
        if self.seen.contains(&physical)
            || self.seeded.contains(&physical)
            || self.top.iter().any(|e| e.physical == physical)
        {
            self.tel.caches.candidates.hits += 1;
            return Ok(true);
        }
        if self.limits.max_candidates.is_some_and(|n| self.tel.leaves.visited >= n) {
            self.stop = Some(ExitReason::CandidateLimit);
            return Ok(false);
        }
        self.tel.leaves.visited += 1;
        let power = self.pool.deck_power(&physical.as_deck(), self.song, self.event)?.power();
        let mut applications = BTreeMap::new();
        let mut terminal_details = BTreeMap::new();
        let evaluation = if matches!(self.request.objective.inner(), Objective::LiveScore { .. }) {
            // Current CoreAPI: fresh context for this exact physical candidate.
            // Reuse no gameplay/random state between candidates or atoms.
            self.input = Some(expectation::context(self.pool, &physical, &self.request.objective)?);
            let input = self.input.as_mut().expect("context");
            if let Some(v) = self.simulation.music_length_ms {
                input.params.music_length_ms = v;
            }
            if let Some(v) = self.simulation.score_music_length_ms {
                input.params.score_music_length_ms = Some(v);
            }
            let plan = match cut {
                Some((bounds, domain)) => {
                    let (_, resume) = self.rec.clock.lap(slot::CUTOFF_TABLE);
                    let plan = self.cutoff_tables(bounds, domain, &physical, power)?;
                    self.rec.clock.lap(resume);
                    plan
                }
                None => None,
            };
            let mut outcomes = Vec::with_capacity(self.law.atoms().len());
            let mut duplicates = BTreeMap::<i32, SeedOutcome>::new();
            let (mut partial, mut consumed) = (0i128, 0u128);
            for (a, &(root, weight)) in self.law.atoms().iter().enumerate() {
                if self.expired() {
                    self.tel.leaves.partial += 1;
                    return Ok(false);
                }
                let atom = if let Some(atom) = duplicates.get(&root) {
                    self.tel.leaves.duplicate_atoms += 1;
                    let mut a = atom.clone();
                    a.weight = weight;
                    a
                } else {
                    // The payoff of the other atoms: simulated roots exactly, later roots by their whole cap, and the
                    // later atoms of this root share its cap.
                    let cutoff = plan.as_ref().zip(cut).and_then(|(plan, (bounds, _))| {
                        let table = plan.tables[plan.index[a]].as_ref()?;
                        let (mut fixed, mut same) = (partial, weight as i128);
                        for (b, &(rb, wb)) in self.law.atoms().iter().enumerate().skip(a + 1) {
                            if rb == root {
                                same = same.checked_add(wb as i128)?;
                                continue;
                            }
                            let payoff = match duplicates.get(&rb) {
                                Some(d) => d.terminal_payoff,
                                None => plan.tables[plan.index[b]].as_ref()?.full,
                            };
                            fixed = fixed.checked_add(payoff.checked_mul(wb as i128)?)?;
                        }
                        let kth = self.top.last()?;
                        Some((bounds, table, fixed, same, kth.evaluation.expected_payoff.numerator, kth.power))
                    });
                    let terminal = match cutoff {
                        Some((bounds, table, fixed, same, threshold, kth_power)) => {
                            let input = self.input.as_ref().expect("context");
                            let frames = input.play.frames.len();
                            let mut checks = 0usize;
                            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                            let outcome = input.simulate_with_cutoff(self.pool.master, root, CUTOFF_EVERY, |s| {
                                checks += 1;
                                // Strict inequality preserves every possible power/ID tie.
                                table
                                    .payoff_cap(bounds, s)
                                    .and_then(|cap| cap.checked_mul(same)?.checked_add(fixed))
                                    .is_some_and(|total| total < threshold || (total == threshold && power < kth_power))
                            })?;
                            let Some(terminal) = outcome else {
                                self.rec.clock.lap_as(slot::STOPPED, resume);
                                telemetry::record_stop(&mut self.tel.leaves.cutoff, checks * CUTOFF_EVERY, frames);
                                self.remember(physical);
                                return Ok(true);
                            };
                            self.rec.clock.lap(resume);
                            CurrentDeclaredOutcome { terminal, network_applications: Vec::new() }
                        }
                        None => {
                            let budget = self.budget;
                            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                            let terminal = simulate_current(
                                self.input.as_ref().expect("context"),
                                self.pool.master,
                                root,
                                self.network,
                                self.simulation.live_finished_from_frame,
                                || budget.expired(),
                            )?;
                            self.rec.clock.lap(resume);
                            let Some(terminal) = terminal else {
                                self.stop = Some(ExitReason::TimeLimit);
                                self.tel.leaves.partial += 1;
                                return Ok(false);
                            };
                            terminal
                        }
                    };
                    applications.insert(root, terminal.network_applications);
                    let terminal = terminal.terminal;
                    let final_life = terminal.model.current_life();
                    terminal_details.insert(root, (final_life, terminal.model.converted_judgements()));
                    self.tel.leaves.simulations += 1;
                    let atom = SeedOutcome {
                        root_seed: root,
                        weight,
                        performance_order: terminal.performance_order,
                        final_score: terminal.final_score,
                        terminal_payoff: self.payoff(&physical, terminal.final_score, power, Some(final_life))?,
                    };
                    duplicates.insert(root, atom.clone());
                    atom
                };
                partial = partial
                    .checked_add(atom.terminal_payoff.checked_mul(weight as i128).ok_or_else(arithmetic)?)
                    .ok_or_else(arithmetic)?;
                consumed = consumed.checked_add(weight as u128).ok_or_else(arithmetic)?;
                outcomes.push(atom);
                if self.top.len() == self.request.k
                    && consumed < self.law.total_weight()
                    && let Some(upper) = self.metric.upper()
                {
                    let remaining = i128::try_from(self.law.total_weight() - consumed).map_err(|_| arithmetic())?;
                    let bound = partial
                        .checked_add(remaining.checked_mul(upper).ok_or_else(arithmetic)?)
                        .ok_or_else(arithmetic)?;
                    // Strict inequality preserves every possible power/ID tie.
                    if bound < self.top.last().expect("top k").evaluation.expected_payoff.numerator {
                        self.tel.leaves.atom_bound_pruned += 1;
                        self.remember(physical);
                        return Ok(true);
                    }
                }
            }
            expectation::aggregate(outcomes)?
        } else {
            let score = match &self.skip {
                Some(skip) => skip.score(power).0,
                None => power,
            };
            let payoff = self.payoff(&physical, score, power, None)?;
            expectation::aggregate(vec![SeedOutcome {
                root_seed: 0,
                weight: 1,
                performance_order: [0, 1, 2, 3, 4],
                final_score: score,
                terminal_payoff: payoff,
            }])?
        };
        self.tel.leaves.evaluated += 1;
        self.remember(physical);
        let entry = Entry {
            physical,
            members: physical.members.map(|i| self.pool.members[i].id),
            snaps: physical.snaps.map(|i| i.map(|i| self.pool.snaps[i].id)),
            power,
            evaluation,
            network_applications: applications,
            terminal_details,
        };
        let pos = self.top.iter().position(|e| compare(&entry, e) == Ordering::Less).unwrap_or(self.top.len());
        // A strictly higher best payoff (not a power or ID tie-break) starts a polish round below.
        let better = self
            .top
            .first()
            .is_none_or(|best| entry.evaluation.expected_payoff.numerator > best.evaluation.expected_payoff.numerator);
        if pos < self.request.k {
            self.top.insert(pos, entry);
            self.top.truncate(self.request.k);
            let (best, kth, filled) = self.standing();
            self.rec.incumbent(&mut self.tel, best, kth, filled);
            if better {
                self.polish()?;
            }
            self.report_progress();
        }
        self.tel.leaves.peak_retained = self.tel.leaves.peak_retained.max(self.top.len());
        Ok(true)
    }

    /// Best and K-th payoff numerators, and the decks held.
    fn standing(&self) -> (i128, Option<i128>, usize) {
        let numerator = |e: &Entry| e.evaluation.expected_payoff.numerator;
        let kth = (self.top.len() == self.request.k).then(|| numerator(self.top.last().expect("K decks")));
        (self.top.first().map_or(0, numerator), kth, self.top.len())
    }

    /// The stop leaves this joint node's whole subtree unexplored.
    fn unexplored_node(
        &mut self,
        p: &PhysicalDeck,
        depth: usize,
        domain: &crate::domain::CandidateDomain,
        bounds: &super::joint::JointBounds,
        orders: &[([usize; 5], u128)],
    ) -> Result<(), Error> {
        if self.stop.is_none() {
            return Ok(());
        }
        let started = self.bound_start();
        let upper = if depth == 0 {
            joint_root_upper(self.pool, domain, bounds, orders, 0)?
        } else {
            Some(node_upper(self.pool, domain, bounds, p, depth, orders)?.0)
        };
        self.fold_unexplored(upper, started);
        Ok(())
    }

    /// The stop leaves the choices of this joint node from `offset` on unexplored; `root` is the bound order the
    /// root loop follows, if any.
    #[allow(clippy::too_many_arguments)]
    fn unexplored_choices(
        &mut self,
        p: &PhysicalDeck,
        depth: usize,
        offset: usize,
        tail: Option<(&super::joint::JointBounds, &super::joint::TailState)>,
        root: Option<&warm::RootOrder>,
        domain: &crate::domain::CandidateDomain,
        bounds: &super::joint::JointBounds,
        orders: &[([usize; 5], u128)],
    ) -> Result<(), Error> {
        let choices = root.map_or(bounds.choices.len(), |r| r.children.len());
        if self.stop.is_none() || offset >= choices {
            return Ok(());
        }
        let started = self.bound_start();
        // Each bound covers every child at or after `offset` (the node bound covers all of its children; ordered root
        // children are in descending order of their depth-1 bound).
        let upper = match (tail, root) {
            (_, Some(r)) => Some(r.caps[offset].0),
            (Some((tail_bounds, state)), None) => Some(tail_bounds.tail_upper(state, offset)?.0),
            (None, None) if depth > 0 => Some(node_upper(self.pool, domain, bounds, p, depth, orders)?.0),
            (None, None) => joint_root_upper(self.pool, domain, bounds, orders, offset)?,
        };
        self.fold_unexplored(upper, started);
        Ok(())
    }

    /// Start bounding what the stop leaves unexplored. Its time goes to `proof.boundMs` and `otherMs`, not to the
    /// search activity the stop interrupted; pass the returned mark to `fold_unexplored`.
    fn bound_start(&mut self) -> (Instant, usize) {
        self.rec.clock.lap(slot::OTHER)
    }

    fn fold_unexplored(&mut self, upper: Option<i128>, (started, resume): (Instant, usize)) {
        if let Some(upper) = upper {
            self.rec.unexplored(upper);
        }
        let (now, _) = self.rec.clock.lap(resume);
        self.tel.proof.bound_ms += now.saturating_duration_since(started).as_secs_f64() * 1000.0;
    }

    /// Close the recording into the wire document.
    /// `standing` is the final Top-K's (best, K-th, decks held), taken before the results leave the engine.
    fn finish_telemetry(&mut self, standing: (i128, Option<i128>, usize)) {
        self.rec.end_open(&mut self.tel);
        self.rec.clock.add_to(&mut self.tel.time);
        let mut tel = std::mem::take(&mut self.tel);
        self.close_telemetry(&mut tel, standing, true);
        self.tel = tel;
    }

    /// The counters kept outside the document, the last incumbent and the proof. `stopped` is true when the search
    /// has ended (completed or stopped, with what a stop leaves unexplored bounded); a progress report is neither.
    fn close_telemetry(&self, tel: &mut Telemetry, standing: (i128, Option<i128>, usize), stopped: bool) {
        if let Some(oracle) = &self.luck {
            tel.luck_replay = oracle.stats.clone();
            tel.caches.luck_replay = oracle.cache_use;
            tel.caches.luck_branches = oracle.branch_cache_use;
            #[cfg(feature = "search-diagnostics")]
            {
                tel.luck_replay.diagnostics = Some(oracle.diag.report(oracle.variant_count(), oracle.distinct_masks()));
            }
        }
        (tel.caches.bonus_rows, tel.caches.bonus_rows_refused) = self.bonus_scratch.cache_use();
        tel.caches.rush_windows = self.bound_scratch.rush_windows();
        let (best, kth, filled) = standing;
        self.rec.close_timeline(tel, best, kth, filled);
        let complete = stopped && self.stop.is_none();
        let proof = &mut tel.proof;
        proof.complete = complete;
        proof.parts = self.rec.parts;
        proof.parts_done = if complete { self.rec.parts } else { self.rec.parts_done };
        proof.fraction = if complete { Some(1.0) } else { self.rec.progress() };
        if !complete
            && self.rec.tracked
            && let Some((done, total)) = self.rec.frontier.top_level()
        {
            (proof.top_level_done, proof.top_level_total) = (Some(done), Some(total));
        }
        if filled > 0 {
            proof.best = Some(best.to_string());
        }
        proof.kth = kth.map(|v| v.to_string());
        if stopped && !complete && self.rec.bounded {
            proof.upper_bound = self.rec.unexplored.map(|v| v.to_string());
            let gap = |x: i128| match self.rec.unexplored {
                Some(upper) => telemetry::gap(upper, x),
                None => Some(0.0),
            };
            proof.best_gap = (filled > 0).then(|| gap(best)).flatten();
            proof.kth_gap = kth.and_then(gap);
        }
    }

    /// Decks of the Top-K the warm start evaluated first.
    fn seeded_in_top(&self) -> usize {
        self.top.iter().filter(|t| self.seeded.contains(&t.physical)).count()
    }

    /// The result ended by `exit_reason` with these decks and telemetry, `elapsed` after the search start; `fixed`
    /// marks the evaluation of one requested deck. The final result and the progress reports share it.
    fn outcome(
        &self,
        strategy: &Strategy,
        exit_reason: ExitReason,
        fixed: bool,
        results: Vec<RecommendedDeck>,
        telemetry: Telemetry,
        elapsed: Duration,
    ) -> RecommendationOutcome {
        let proven = exit_reason == ExitReason::Exhausted;
        let optimality = if fixed {
            Optimality::NotApplicable
        } else if proven {
            Optimality::Proven
        } else if matches!(strategy, Strategy::Candidate { .. }) {
            Optimality::Heuristic
        } else {
            Optimality::Unproven
        };
        let (request, law) = (self.request, self.law);
        let probability_law = if matches!(request.objective.inner(), Objective::LiveScore { .. }) {
            serde_json::json!({"kind":"explicitFiniteNativeRoots","atoms":law.atoms().iter().map(|(r,w)|serde_json::json!([r,w.to_string()])).collect::<Vec<_>>(),"totalWeight":law.total_weight().to_string(),"populationLaw":"unknown; no TickCount population law inferred"})
        } else {
            serde_json::json!({"kind":"deterministic"})
        };
        RecommendationOutcome {
            format: RESULT_FORMAT,
            completion: if proven { Completion::Complete } else { Completion::TimedOut },
            optimality,
            exit_reason,
            result_identity: if fixed { "fixedPhysicalDeck" } else { "physicalDeck" },
            metric: self.metric.clone(),
            player_goal: None,
            strategy: strategy.clone(),
            probability_law,
            proof_scope: "conditional on declared master, roster, complete judgement/clock inputs, finite native-root law and optional external confirmations; client counters are not server reward authority",
            resolved_context: serde_json::Value::Null,
            results,
            telemetry,
            elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        }
    }

    /// A progress report at `now`: the result the search would return if its time limit expired now. The telemetry
    /// so far is closed on a copy (the recording stays open); it has no bound of the unexplored part, which only a
    /// stop computes.
    fn report_outcome(
        &self,
        strategy: &Strategy,
        start: Instant,
        now: Instant,
    ) -> Result<RecommendationOutcome, Error> {
        let mut tel = self.tel.clone();
        tel.incumbents.warm_start.final_top_k = self.seeded_in_top();
        self.rec.peek_open(&mut tel, now);
        self.rec.clock.peek_into(now, &mut tel.time);
        self.close_telemetry(&mut tel, self.standing(), false);
        let results = self.top.iter().cloned().map(|e| e.wire(self.metric)).collect::<Result<Vec<_>, _>>()?;
        let elapsed = now.saturating_duration_since(start);
        Ok(self.outcome(strategy, ExitReason::TimeLimit, false, results, tel, elapsed))
    }
}

/// The best payoff any completion through the depth-0 joint choices from `from` can reach: each choice's depth-1
/// node bound, the bound the traversal itself would check there. None when no legal choice remains.
fn joint_root_upper(
    pool: &Pool,
    domain: &crate::domain::CandidateDomain,
    bounds: &super::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    from: usize,
) -> Result<Option<i128>, Error> {
    use super::joint::SLOTS;
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
fn node_upper(
    pool: &Pool,
    domain: &crate::domain::CandidateDomain,
    bounds: &super::joint::JointBounds,
    p: &PhysicalDeck,
    depth: usize,
    orders: &[([usize; 5], u128)],
) -> Result<(i128, i64), Error> {
    let choices = super::joint::JointBounds::prefix_choices(domain, p, depth);
    let placed = bounds.carriers_placed(p, depth, &choices);
    let keyed = bounds.keyed(p, depth, &choices, 5 - depth, 5 - depth);
    bounds.carrier_level((placed + 5 - depth).min(5)).expected_upper_keyed(
        pool,
        domain,
        p,
        depth,
        orders,
        keyed.as_ref(),
    )
}
pub(crate) fn arithmetic() -> Error {
    Error::Domain("finite-law checked exact arithmetic overflow".into())
}

/// A score distribution summary preserves integer masses and does not use f64 ranks.
/// Quantile p is the smallest score with cumulative mass >= ceil(p * total mass).
pub fn score_summary(mass: &BTreeMap<i32, u128>, target: Option<i32>) -> Result<ScoreSummary, Error> {
    if mass.is_empty() || mass.values().any(|&w| w == 0) {
        return Err(Error::Input("score summary requires positive nonempty masses".into()));
    }
    let total = mass.values().try_fold(0u128, |a, &w| a.checked_add(w).ok_or_else(arithmetic))?;
    let quantile = |tenth: u128| -> Result<i32, Error> {
        // Dividing first keeps valid u128 totals safe, including u128::MAX.
        let rank = (total / 10)
            .checked_mul(tenth)
            .and_then(|n| n.checked_add(((total % 10) * tenth).div_ceil(10)))
            .ok_or_else(arithmetic)?;
        let mut cumulative = 0u128;
        for (&score, &weight) in mass {
            cumulative = cumulative.checked_add(weight).ok_or_else(arithmetic)?;
            if cumulative >= rank {
                return Ok(score);
            }
        }
        Err(Error::Domain("score quantile exceeds mass".into()))
    };
    let (probability_at_least, expected_shortfall) = if let Some(target) = target {
        let mut success = 0u128;
        let mut shortfall = 0i128;
        for (&score, &weight) in mass {
            if score >= target {
                success = success.checked_add(weight).ok_or_else(arithmetic)?;
            } else {
                shortfall = shortfall
                    .checked_add(
                        (target as i128 - score as i128)
                            .checked_mul(i128::try_from(weight).map_err(|_| arithmetic())?)
                            .ok_or_else(arithmetic)?,
                    )
                    .ok_or_else(arithmetic)?;
            }
        }
        (
            Some(Fraction { numerator: success.to_string(), denominator: total.to_string() }),
            Some(Fraction { numerator: shortfall.to_string(), denominator: total.to_string() }),
        )
    } else {
        (None, None)
    };
    Ok(ScoreSummary {
        minimum: *mass.first_key_value().expect("nonempty").0,
        maximum: *mass.last_key_value().expect("nonempty").0,
        p10: quantile(1)?,
        p50: quantile(5)?,
        p90: quantile(9)?,
        target_score: target,
        probability_at_least,
        expected_shortfall,
    })
}

/// Fixed-deck evaluator over the shared core, exposed for independent checks.
/// Context identity must match. Network arrivals and explicit finished lifecycle are
/// Unsupported at entry; this build does not model their new settlement semantics.
/// No wall clock is used by this unbounded numeric evaluator.
pub fn evaluate_declared_context(
    master: &ournotes_sim::master::Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    root_seed: i32,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
) -> Result<(expectation::ConditionalOutcome, Vec<(usize, usize)>), Error> {
    reject_unsupported_lifecycle(network, simulation.live_finished_from_frame)?;
    if input.physical() != *physical {
        return Err(Error::Input("physical deck differs from declared context".into()));
    }
    let mut input = input.clone();
    if let Some(v) = simulation.music_length_ms {
        if v <= 0 {
            return Err(Error::Input("musicLengthMs must be positive".into()));
        }
        input.params.music_length_ms = v;
    }
    if let Some(v) = simulation.score_music_length_ms {
        if v <= 0 {
            return Err(Error::Input("scoreMusicLengthMs must be positive".into()));
        }
        input.params.score_music_length_ms = Some(v);
    }
    if input.delta_times.len() != input.play.frames.len() {
        return Err(Error::Input("one deltaTime per declared frame required".into()));
    }
    if simulation.live_finished_from_frame.is_some_and(|v| v >= input.play.frames.len()) {
        return Err(Error::Input("lifecycle frame outside declared play".into()));
    }
    if let Some(cs) = network {
        let g = input.gekisou.as_ref().ok_or_else(|| Error::Input("network confirmations require Gekisou".into()))?;
        let missions: [i64; 3] =
            g.missions.clone().try_into().map_err(|_| Error::Input("three native missions required".into()))?;
        let factors = ournotes_sim::live::full::gekisou_rank_factors(master, &missions)?;
        let mut ranges = HashSet::new();
        for c in cs {
            if c.frame >= input.play.frames.len()
                || c.range >= g.fevers.len().min(3)
                || !(1..=5).contains(&c.rank)
                || !ranges.insert(c.range)
                || c.percent != factors[c.range][c.rank as usize - 1]
            {
                return Err(Error::Input(
                    "invalid aggregate network packet arrival/range/group rank/percentage".into(),
                ));
            }
        }
        if ranges.len() != g.fevers.len() {
            return Err(Error::Input("one aggregate packet per fever required".into()));
        }
    }
    let outcome = simulate_current(&input, master, root_seed, network, simulation.live_finished_from_frame, || false)?
        .ok_or_else(|| Error::Domain("unbounded declared evaluator cancelled unexpectedly".into()))?;
    Ok((outcome.terminal, outcome.network_applications))
}

/// Low-level exact-model search; request.time_limit is respected in addition to Limits.
/// Played input is COMPLETE declared judgement/clock data. Built-in pure metrics permit
/// request-local duplicate-root reuse. It does not inherit generic payoff callback state.
#[allow(clippy::too_many_arguments)] // The audit inputs stay separately borrowed; JSON callers use RecommendationRequest.
pub fn solve_physical(
    pool: &Pool,
    request: &SearchRequest,
    law: &FiniteSeedLaw,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    limits: &Limits,
    strategy: &Strategy,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
) -> Result<RecommendationOutcome, Error> {
    let origin = Instant::now();
    solve_physical_impl(
        pool,
        request,
        law,
        metric,
        event_input,
        limits,
        strategy,
        network,
        simulation,
        None,
        None,
        origin,
        None,
    )
}

/// `origin` is the request start; telemetry phases and incumbents are timed from it. `progress` receives reports
/// (`progress.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_physical_impl(
    pool: &Pool,
    request: &SearchRequest,
    law: &FiniteSeedLaw,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    limits: &Limits,
    strategy: &Strategy,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
    fixed: Option<PhysicalDeck>,
    compiled: Option<&crate::handler::ExecutionPlan>,
    origin: Instant,
    progress: Option<ProgressHook<'_>>,
) -> Result<RecommendationOutcome, Error> {
    reject_unsupported_lifecycle(network, simulation.live_finished_from_frame)?;
    let start = Instant::now();
    let mut limits = limits.clone();
    if let Some(d) = request.time_limit {
        let ms = d.as_millis().min(u64::MAX as u128) as u64;
        limits.time_limit_ms = Some(limits.time_limit_ms.map_or(ms, |m| m.min(ms)));
    }
    validate(request.k, law, &limits, strategy)?;
    let normalized;
    let request = if compiled.is_none() {
        normalized =
            SearchRequest { objective: expectation::normalized_objective(&request.objective), ..request.clone() };
        &normalized
    } else {
        request
    };
    let owned_plan;
    let plan = match compiled {
        Some(plan) => plan,
        None => {
            owned_plan =
                crate::handler::compile_execution(pool, request, metric, event_input, network, simulation, strategy)?;
            &owned_plan
        }
    };
    let candidates = plan.domain.members();
    let required = plan.domain.required();
    let snaps = plan.domain.snaps();
    let leader = plan.domain.leader();
    if let Some(p) = fixed {
        plan.domain.check_fixed(pool, &p)?;
    }
    let feasible = plan.domain.is_feasible();
    let song = &plan.song;
    let event = plan.event;
    let skip = &plan.skip;
    let mut tel = Telemetry::default();
    let env = &mut tel.environment;
    env.k = request.k;
    env.time_limit_ms = limits.time_limit_ms;
    env.max_candidates = limits.max_candidates;
    env.cache_entries = limits.cache_entries;
    env.law = Some(telemetry::Law {
        atoms: law.atoms().len(),
        orders: super::joint::order_law(law)?.len(),
        total_weight: law.total_weight().to_string(),
    });
    env.domain = Some(telemetry::Domain {
        members: candidates.len(),
        snaps: snaps.len(),
        required: required.len(),
        leader_fixed: leader.is_some(),
    });
    env.bounds.compiled = plan.joint.is_some();
    env.bounds.fallback = plan.bound_fallback.clone();
    env.bounds.compile_ms = plan.bound_compile_ms;
    if let Some(b) = &plan.joint {
        env.bounds.choices = b.choices.len();
        env.bounds.fine = b.has_fine();
        env.bounds.rush = b.rush_eligible();
        env.bounds.class_search = b.uses_class_search();
    }
    let mut engine = Engine {
        pool,
        request,
        law,
        metric,
        event_input,
        network,
        simulation,
        limits: &limits,
        budget: super::budget::SearchBudget::new(start, limits.time_limit_ms.map(Duration::from_millis))?,
        stop: None,
        tel,
        rec: Recorder::new(origin),
        correlated: false,
        resource: false,
        bound_scratch: super::snaps::JointScratch::default(),
        luck: None,
        bonus_scratch: super::joint::BonusScratch::default(),
        top: Vec::new(),
        seen: HashSet::new(),
        fifo: VecDeque::new(),
        input: None,
        song: song.as_ref(),
        event,
        skip: skip.as_ref(),
        seeded: HashSet::new(),
        root_order: None,
        warm: None,
        progress: progress.map(|hook| progress::Reporter::new(hook.interval, hook.report, strategy, start)),
    };
    if let Some(p) = fixed {
        engine.tel.environment.traversal = Traversal::Fixed;
        engine.rec.begin(&mut engine.tel, "evaluate", None);
        engine.consider(p)?;
        engine.rec.end(&mut engine.tel);
    } else if feasible {
        match strategy {
            Strategy::Exhaustive | Strategy::BranchAndBound => {
                let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
                if let Some(bounds) = &plan.joint {
                    engine.rec.begin(&mut engine.tel, "setup", None);
                    let orders = super::joint::order_law(law)?;
                    engine.luck = super::luck::LuckOracle::new(pool, request, &plan.domain, law, bounds.luck_life())?;
                    engine.tel.environment.bounds.luck_oracle = engine.luck.is_some();
                    engine.correlated = bounds.correlation_worthwhile(pool, &plan.domain, &orders)?;
                    engine.resource = !bounds.prefers_compositions()
                        && bounds.resource_worthwhile(pool, &plan.domain, &orders, engine.correlated)?;
                    engine.warm = warm::Warm::new(&plan.domain, bounds, &orders);
                    engine.rec.end(&mut engine.tel);
                    if bounds.is_pt() && !bounds.prefers_compositions() {
                        engine.tel.environment.traversal = Traversal::Joint;
                        engine.rec.begin(&mut engine.tel, "ptWarmStart", None);
                        let seed_bonus = bounds.bonus_upper(pool, &plan.domain, &physical, 0);
                        joint_rec(0, &mut physical, &plan.domain, bounds, &orders, &mut engine, seed_bonus)?;
                        engine.rec.end(&mut engine.tel);
                        // After the maximum-bonus warmup, whose full Top-K ends it.
                        warm::seed(&mut engine)?;
                        let mut searched = false;
                        if !engine.expired() {
                            engine.rec.begin(&mut engine.tel, "ptRegimeCompile", None);
                            let restricted = if engine.top.len() == request.k {
                                let numerator =
                                    engine.top.last().expect("K incumbents").evaluation.expected_payoff.numerator;
                                bounds.qualifying_pt_domain(pool, &plan.domain, numerator, law.total_weight())?
                            } else {
                                None
                            };
                            let mut regime = telemetry::PtRegime::default();
                            let compiled = if let Some(domain) = &restricted {
                                let prepare = Instant::now();
                                let result = super::joint::JointBounds::compile(
                                    pool,
                                    request,
                                    domain,
                                    metric,
                                    event_input,
                                    simulation,
                                );
                                regime.compile_ms = prepare.elapsed().as_secs_f64() * 1000.0;
                                match result {
                                    Ok(b) => Some(b),
                                    Err(error) => {
                                        regime.fallback = Some(error.to_string());
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            engine.rec.end(&mut engine.tel);
                            engine.rec.begin(&mut engine.tel, "search", None);
                            engine.rec.frontier.clear();
                            (engine.rec.tracked, engine.rec.bounded, engine.rec.unexplored) = (true, true, None);
                            searched = true;
                            if let (Some(domain), Some(refined)) = (&restricted, &compiled) {
                                regime.members_removed = plan.domain.members().len() - domain.members().len();
                                engine.correlated = refined.correlation_worthwhile(pool, domain, &orders)?;
                                engine.tel.environment.bounds.pt_regime = Some(regime);
                                ordered_root(&mut physical, domain, refined, &orders, &mut engine)?;
                            } else {
                                engine.tel.environment.bounds.pt_regime = Some(regime);
                                ordered_root(&mut physical, &plan.domain, bounds, &orders, &mut engine)?;
                            }
                            engine.rec.end(&mut engine.tel);
                        }
                        if engine.stop.is_some() && !searched {
                            // The warm start's bonus filter skipped prefixes: the whole domain remains to be proven.
                            engine.rec.frontier.clear();
                            (engine.rec.tracked, engine.rec.bounded) = (true, true);
                            let started = engine.bound_start();
                            let upper = joint_root_upper(pool, &plan.domain, bounds, &orders, 0)?;
                            engine.fold_unexplored(upper, started);
                        }
                    } else if bounds.prefers_compositions() {
                        engine.tel.environment.traversal = Traversal::Composition;
                        warm::seed(&mut engine)?;
                        engine.rec.begin(&mut engine.tel, "search", None);
                        (engine.rec.tracked, engine.rec.bounded) = (true, true);
                        composition::solve(&plan.domain, bounds, &orders, &mut engine)?;
                        engine.rec.end(&mut engine.tel);
                    } else {
                        engine.tel.environment.traversal = Traversal::Joint;
                        warm::seed(&mut engine)?;
                        joint_regimes(&mut physical, plan, bounds, &orders, &mut engine)?;
                    }
                    engine.tel.environment.bounds.correlated = engine.correlated;
                    engine.tel.environment.bounds.resource = engine.resource;
                } else {
                    engine.tel.environment.traversal = Traversal::Exhaustive;
                    engine.rec.begin(&mut engine.tel, "search", None);
                    engine.rec.tracked = true;
                    members_rec(0, &mut physical, candidates, required, leader, snaps, &mut engine)?;
                    engine.rec.end(&mut engine.tel);
                }
            }
            Strategy::Candidate { power_seeds, proposals, proposal_seed } => {
                engine.tel.environment.traversal = Traversal::Candidate;
                engine.rec.begin(&mut engine.tel, "warmStart", None);
                if *power_seeds > 0 && !engine.expired() {
                    let power = Objective::Power { music_id: None, event };
                    let power = match request.objective.context() {
                        Some(c) => power.in_scenario(c.clone()),
                        None => match &engine.song {
                            Some(s) => Objective::Power { music_id: Some(s.id), event },
                            None => power,
                        },
                    };
                    let duration = limits.time_limit_ms.map(|m| Duration::from_millis((m / 4).min(1000)));
                    let seeds = super::search(
                        pool,
                        &SearchRequest {
                            objective: power,
                            k: *power_seeds,
                            constraints: request.constraints.clone(),
                            time_limit: duration,
                        },
                    )?;
                    let seeds = seeds
                        .results
                        .into_iter()
                        .map(|seed| pool.deck(seed.members, seed.snaps, [0, 1, 2, 3, 4]))
                        .collect::<Result<Vec<_>, _>>()?;
                    // Cover every power seed BEFORE refining any one's physical layout.
                    // The former 120 permutations of seed 0 could consume the entire
                    // browser budget before another member set or snap layout was tried.
                    // Five rounds cap warmup at 5 * powerSeeds; the remaining budget
                    // explores skills, pairings and member substitutions below.
                    'seeds: for round in 0..5 {
                        for d in &seeds {
                            let mut p = PhysicalDeck { members: d.members, snaps: d.snaps };
                            match round {
                                0 => {}
                                1 => p.snaps = [None; 5],
                                _ => {
                                    let nonleaders = [0, 1, 3, 4];
                                    for (at, &slot) in nonleaders.iter().enumerate() {
                                        let from = nonleaders[(at + round - 1) % 4];
                                        p.members[slot] = d.members[from];
                                        p.snaps[slot] = d.snaps[from];
                                    }
                                }
                            }
                            engine.tel.candidate.warmup_proposals += 1;
                            if !engine.consider(p)? {
                                break 'seeds;
                            }
                            if round == 0 {
                                engine.tel.candidate.warmup_member_sets += 1;
                            }
                        }
                    }
                }
                engine.rec.end(&mut engine.tel);
                engine.rec.begin(&mut engine.tel, "proposals", None);
                let mut rng = ProposalRandom(*proposal_seed | 1);
                for n in 0..*proposals {
                    if engine.expired() {
                        break;
                    }
                    engine.tel.candidate.exploration_proposals += 1;
                    let mut p = if n % 3 != 0 && !engine.top.is_empty() {
                        let mut p = engine.top[rng.index(engine.top.len())].physical;
                        if rng.index(2) == 0 {
                            let slot = rng.index(5);
                            let m = candidates[rng.index(candidates.len())];
                            if (slot != 2 || leader.is_none()) && !required.contains(&p.members[slot]) {
                                p.members[slot] = m;
                            }
                        } else {
                            let slot = rng.index(5);
                            let s = rng.index(snaps.len() + 1);
                            p.snaps[slot] = if s == snaps.len() { None } else { Some(snaps[s]) };
                        }
                        p
                    } else {
                        random_deck(pool, candidates, required, leader, snaps, &mut rng)
                    };
                    // Additional physical swaps preserve member/snap pairs and fixed leader.
                    if n % 4 == 0 {
                        let a = rng.index(5);
                        let b = rng.index(5);
                        if leader.is_none() || (a != 2 && b != 2) {
                            p.members.swap(a, b);
                            p.snaps.swap(a, b);
                        }
                    }
                    if pool.check_deck(&p.as_deck()).is_err() {
                        continue;
                    }
                    if !engine.consider(p)? {
                        break;
                    }
                }
                engine.rec.end(&mut engine.tel);
                if engine.stop.is_none() {
                    engine.stop = Some(ExitReason::ProposalLimit)
                }
            }
        }
    }
    engine.tel.incumbents.warm_start.final_top_k = engine.seeded_in_top();
    let exit_reason = engine.stop.unwrap_or(ExitReason::Exhausted);
    let standing = engine.standing();
    engine.rec.begin(&mut engine.tel, "finish", None);
    let results = std::mem::take(&mut engine.top).into_iter().map(|e| e.wire(metric)).collect::<Result<Vec<_>, _>>()?;
    engine.rec.end(&mut engine.tel);
    engine.finish_telemetry(standing);
    let telemetry = std::mem::take(&mut engine.tel);
    Ok(engine.outcome(strategy, exit_reason, fixed.is_some(), results, telemetry, start.elapsed()))
}

/// Gekisou score: partition the physical domain by its converting Snaps.
/// - none: the conversion-free sub-domain keeps the raw judgement reach;
/// - exactly one, `c` in physical slot `s`: the sub-domain holds no other converting Snap, so per-entry reach
///   widens only by the windows of `c`, and `s` is forced to `c`;
/// - two or more: split by the first two slots, in search order, that hold converting Snaps. Both are forced to
///   converting Snaps and the other earlier slots exclude them.
///
/// The parts are disjoint and cover the domain. They share one Top-K and its canonical order. A failed part compile
/// falls back to one pool-wide search.
fn joint_regimes(
    p: &mut PhysicalDeck,
    plan: &crate::handler::ExecutionPlan,
    bounds: &super::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<(), Error> {
    use super::joint::{JointBounds, SLOTS, SlotRules};
    let converting = super::snaps::conversion_snaps(e.pool, plan.domain.snaps())?;
    (e.rec.tracked, e.rec.bounded) = (true, true);
    if converting.is_empty() {
        e.rec.begin(&mut e.tel, "search", None);
        ordered_root(p, &plan.domain, bounds, orders, e)?;
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
    e.rec.begin(&mut e.tel, "conversionCompile", None);
    let prepare = Instant::now();
    let mut conversion = telemetry::Conversion { snaps: converting.len(), ..Default::default() };
    let mut compiled = Vec::with_capacity(parts.len());
    for (domain, rules) in parts {
        match JointBounds::compile(e.pool, e.request, &domain, e.metric, e.event_input, e.simulation) {
            Ok(b) => compiled.push((domain, rules, b)),
            Err(error) => {
                conversion.compile_ms = prepare.elapsed().as_secs_f64() * 1000.0;
                conversion.fallback = Some(error.to_string());
                e.tel.environment.bounds.conversion = Some(conversion);
                e.rec.end(&mut e.tel);
                e.rec.begin(&mut e.tel, "search", None);
                ordered_root(p, &plan.domain, bounds, orders, e)?;
                e.rec.end(&mut e.tel);
                return Ok(());
            }
        }
    }
    conversion.compile_ms = prepare.elapsed().as_secs_f64() * 1000.0;
    conversion.parts = compiled.iter().map(|(_, rules, _)| rules.len()).sum();
    let total = conversion.parts as u64;
    e.tel.environment.bounds.conversion = Some(conversion);
    e.rec.end(&mut e.tel);
    e.rec.parts = total;
    // One root traversal per (part, slot rules); every traversal runs. With the visit order on, the traversals that
    // hold Top-K incumbents run first, best incumbent first, the others in the static order, and each root loop
    // follows its children's bound order (warm.rs).
    let mut flags = Vec::with_capacity(compiled.len());
    let mut traversals = Vec::new();
    for (i, (domain, rules, part)) in compiled.iter().enumerate() {
        let correlated = part.correlation_worthwhile(e.pool, domain, orders)?;
        flags.push((correlated, part.resource_worthwhile(e.pool, domain, orders, correlated)?));
        traversals.extend((0..rules.len()).map(|j| (i, j)));
    }
    if !warm::static_order() {
        let mut best = std::collections::HashMap::new();
        for t in &e.top {
            let key = warm::traversal_of(&t.physical, &converting);
            best.entry(key).or_insert((t.evaluation.expected_payoff.numerator, t.power));
        }
        traversals.sort_by_key(|t| std::cmp::Reverse(best.get(t).copied()));
    }
    for (i, j) in traversals {
        let (domain, rules, part) = &mut compiled[i];
        (e.correlated, e.resource) = flags[i];
        let (r, label) = &rules[j];
        part.set_rules(e.pool, domain, r.clone());
        *p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        e.rec.frontier.clear();
        e.rec.begin(&mut e.tel, "search", Some(label.clone()));
        let more = ordered_root(p, domain, part, orders, e)?;
        e.rec.end(&mut e.tel);
        if !more {
            if e.rec.parts_done + 1 < total {
                // The later parts: the pool-wide bounds hold for every deck of the domain.
                let started = e.bound_start();
                let upper = joint_root_upper(e.pool, &plan.domain, bounds, orders, 0)?;
                e.fold_unexplored(upper, started);
            }
            return Ok(());
        }
        e.rec.parts_done += 1;
    }
    Ok(())
}

/// One whole-domain traversal, root children in bound order (static under the validation ablation `STATIC_ORDER`).
fn ordered_root(
    p: &mut PhysicalDeck,
    domain: &crate::domain::CandidateDomain,
    bounds: &super::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    if !warm::static_order() {
        // Root-level bound work.
        e.rec.clock.lap(0);
        e.root_order = Some(warm::RootOrder::new(domain, bounds, orders, e)?);
    }
    joint_rec(0, p, domain, bounds, orders, e, None)
}

/// Diagnostics only: the LUCK mask coverage of one leaf fine check and whether it pruned.
#[cfg(feature = "search-diagnostics")]
fn note_leaf(e: &mut Engine<'_, '_>, before: Option<(u64, u64)>, pruned: bool) {
    if let (Some(o), Some((queries, unavailable))) = (e.luck.as_mut(), before) {
        let (q, u) = (o.stats.queries - queries, o.stats.unavailable.total - unavailable);
        let cover = if q == 0 {
            "skipped"
        } else if u == 0 {
            "all"
        } else if u == q {
            "none"
        } else {
            "some"
        };
        *o.diag.leaves.entry((cover, pruned)).or_default() += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn joint_rec(
    depth: usize,
    p: &mut PhysicalDeck,
    domain: &crate::domain::CandidateDomain,
    bounds: &super::joint::JointBounds,
    orders: &[([usize; 5], u128)],
    e: &mut Engine<'_, '_>,
    seed_bonus: Option<i64>,
) -> Result<bool, Error> {
    use super::joint::SLOTS;
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
    let choices = super::joint::JointBounds::prefix_choices(domain, p, depth);
    let placed = bounds.carriers_placed(p, depth, &choices);
    let level = (placed + 5 - depth).min(5);
    let node_bounds = bounds.carrier_level(level);
    let keyed = if depth > 0 { bounds.keyed(p, depth, &choices, 5 - depth, 5 - depth) } else { None };
    if depth > 0 && e.top.len() == e.request.k {
        let joint = &mut e.tel.joint;
        joint.branch.check(depth);
        joint.carriers.nodes[level] += 1;
        let (numerator, power) = node_bounds.expected_upper_keyed(e.pool, domain, p, depth, orders, keyed.as_ref())?;
        let cutoff = e.top.last().expect("full Top-K");
        let threshold = cutoff.evaluation.expected_payoff.numerator;
        if numerator < threshold || (numerator == threshold && power < i64::from(cutoff.power)) {
            joint.branch.prune(depth);
            return Ok(true);
        }
        if numerator == threshold {
            joint.node_ties[depth] += 1;
        }
        if numerator == threshold
            && let Some(assigned_power) = bounds.assignment_power_upper(e.pool, domain, p, depth)
        {
            joint.assignment.check(depth);
            if assigned_power < i64::from(cutoff.power) {
                joint.assignment.prune(depth);
                return Ok(true);
            }
        }
        if depth < 5 && e.correlated {
            joint.correlated.check(depth);
            let correlated =
                node_bounds.correlated_expected_upper_keyed(e.pool, domain, p, depth, orders, keyed.as_ref())?;
            if correlated < threshold || (correlated == threshold && power < i64::from(cutoff.power)) {
                joint.correlated.prune(depth);
                return Ok(true);
            }
        }
        if depth < 5 && e.resource {
            joint.resource.check(depth);
            if let Some(cap) = node_bounds.resource_expected_upper(e.pool, domain, p, depth, orders)
                && (cap < threshold || (cap == threshold && power < i64::from(cutoff.power)))
            {
                joint.resource.prune(depth);
                return Ok(true);
            }
        }
        if seed_bonus.is_none() && depth < 5 && bounds.is_pt() {
            joint.bonus.check(depth);
            if let Some((cap, cap_power)) =
                node_bounds.bonus_expected_upper_with_power(e.pool, domain, p, depth, orders, &mut e.bonus_scratch)?
            {
                if cap < threshold || (cap == threshold && power.min(cap_power) < i64::from(cutoff.power)) {
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
        if depth == 5
            && let Some(cap) = bounds.raw_expected_upper(domain, p, power, orders)
        {
            joint.raw.check(depth);
            if cap < threshold || (cap == threshold && power < i64::from(cutoff.power)) {
                joint.raw.prune(depth);
                return Ok(true);
            }
        }
        // Without fine bounds there is nothing to time (the call would return None).
        if depth == 5 && bounds.has_fine() {
            let kth_power = i64::from(cutoff.power);
            #[cfg(feature = "search-diagnostics")]
            let luck_before = e.luck.as_ref().map(|o| (o.stats.queries, o.stats.unavailable.total));
            if let Some(oracle) = e.luck.as_mut() {
                oracle.site = super::luck::Site::Fine;
            }
            e.rec.clock.lap(slot::FINE);
            let fine = bounds.fine_expected_upper(
                domain,
                p,
                power,
                orders,
                &mut e.bound_scratch,
                e.luck.as_mut().map(|oracle| (e.pool, oracle)),
            )?;
            e.rec.clock.lap(depth);
            if let Some(fine) = fine {
                e.tel.joint.fine.check(depth);
                if fine < threshold || (fine == threshold && power < kth_power) {
                    e.tel.joint.fine.prune(depth);
                    #[cfg(feature = "search-diagnostics")]
                    note_leaf(e, luck_before, true);
                    return Ok(true);
                }
                #[cfg(feature = "search-diagnostics")]
                note_leaf(e, luck_before, false);
                // Only a survivor pays for the per-branch caps.
                if let Some(oracle) = e.luck.as_mut() {
                    oracle.site = super::luck::Site::Branch;
                    e.rec.clock.lap(slot::FINE);
                    let refined =
                        bounds.fine_branch_upper(domain, p, power, orders, &mut e.bound_scratch, e.pool, oracle)?;
                    e.rec.clock.lap(depth);
                    if let Some(refined) = refined {
                        oracle.stats.branch_bound.checks += 1;
                        if refined < threshold || (refined == threshold && power < kth_power) {
                            oracle.stats.branch_bound.pruned += 1;
                            return Ok(true);
                        }
                    }
                }
                if fine == threshold {
                    e.tel.joint.node_ties[depth] += 1;
                }
                #[cfg(feature = "search-diagnostics")]
                if let Some(o) = e.luck.as_mut() {
                    let diag = &mut o.diag;
                    if diag.survivors_seen % 64 == 0 && diag.survivors.len() < 256 {
                        diag.survivors.push(serde_json::json!({
                            "members": p.members.map(|m| e.pool.members[m].id),
                            "snaps": p.snaps.map(|s| s.map(|s| e.pool.snaps[s].id)),
                            "fine": fine.to_string(), "threshold": threshold.to_string(),
                        }));
                    }
                    diag.survivors_seen += 1;
                }
            }
        }
    }
    if depth == 5 {
        let more = e.consider_with(*p, Some((bounds, domain)))?;
        if !more {
            e.unexplored_node(p, depth, domain, bounds, orders)?;
        }
        return Ok(more && !(seed_bonus.is_some() && e.top.len() == e.request.k));
    }
    let slot = SLOTS[depth];
    let rush_caps: Option<super::joint::RushCaps> = if depth == 4 && e.top.len() == e.request.k {
        if let Some(oracle) = &mut e.luck {
            e.tel.joint.rush_prefix.checks += 1;
            oracle.site = super::luck::Site::Prefix;
            e.rec.clock.lap(slot::RUSH_PREFIX);
            let caps = bounds.rush_prefix_caps(e.pool, domain, p, orders, oracle, e.budget)?;
            e.rec.clock.lap(depth);
            let rush = &mut e.tel.joint.rush_prefix;
            if let Some(caps) = &caps {
                rush.variants += caps.variants.iter().filter(|v| v.is_some()).count() as u64;
                if let Some((cap, power)) = caps.whole {
                    let kth = e.top.last().expect("full Top-K");
                    let threshold = kth.evaluation.expected_payoff.numerator;
                    if cap < threshold || (cap == threshold && power < i64::from(kth.power)) {
                        rush.pruned += 1;
                        return Ok(true);
                    }
                }
            } else {
                rush.unavailable += 1;
            }
            caps
        } else {
            None
        }
    } else {
        None
    };
    if depth == 4 && e.luck.is_some() && e.expired() {
        e.unexplored_node(p, depth, domain, bounds, orders)?;
        return Ok(false);
    }
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
    // There is no tail state at the root, so its loop may run in any order.
    let children = root.as_ref().map_or(&bounds.choices[..], |r| &r.children[..]);
    let width = children.len();
    if root.as_ref().and_then(|r| r.best()).is_some_and(|cap| warm::inferior(cap, e)) {
        e.tel.joint.root_order.traversals_pruned += 1;
    }
    for (offset, &(m, choice)) in children.iter().enumerate() {
        if let Some(r) = &root
            && warm::inferior(r.caps[offset], e)
        {
            e.tel.joint.root_order.skipped += (width - offset) as u64;
            break;
        }
        if offset % 16 == 0
            && e.top.len() == e.request.k
            && let Some(state) = tail_high.as_ref().or(tail.as_ref())
        {
            e.tel.joint.tail.check(depth);
            let (upper, power) = high.tail_upper(state, offset)?;
            let kth = e.top.last().expect("full Top-K");
            let threshold = kth.evaluation.expected_payoff.numerator;
            if upper < threshold || (upper == threshold && power < i64::from(kth.power)) {
                e.tel.joint.tail.prune(depth);
                e.tel.joint.tail_choices_skipped[depth] += (width - offset) as u64;
                break;
            }
        }
        // The node itself checked the deadline; inside the choice loop the clock is read every 32 offsets.
        if offset % 32 == 31 && e.expired() {
            e.unexplored_choices(p, depth, offset, tail_check, root.as_ref(), domain, bounds, orders)?;
            return Ok(false);
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
        if let Some(caps) = &rush_caps
            && let Some(v) = e.luck.as_ref().and_then(|oracle| oracle.variant(m, snap))
            && let Some((cap, power)) = caps.variants[v as usize]
        {
            let kth = e.top.last().expect("full Top-K");
            let threshold = kth.evaluation.expected_payoff.numerator;
            if cap < threshold || (cap == threshold && power < i64::from(kth.power)) {
                e.tel.joint.rush_prefix.choices_pruned += 1;
                continue;
            }
        }
        let (pair_bounds, pair_tail) = match &tail_high {
            Some(state) if bounds.is_carrier(m, choice) => (high, Some(state)),
            _ => (low, tail.as_ref()),
        };
        if e.top.len() == e.request.k
            && let Some(state) = pair_tail
        {
            e.tel.joint.pair.check(depth);
            let (upper, power) = pair_bounds.pair_upper(state, m, choice)?;
            let kth = e.top.last().expect("full Top-K");
            let threshold = kth.evaluation.expected_payoff.numerator;
            if upper < threshold || (upper == threshold && power < i64::from(kth.power)) {
                e.tel.joint.pair.prune(depth);
                continue;
            }
            if upper == threshold {
                e.tel.joint.pair_ties[depth] += 1;
            }
        }
        p.members[slot] = m;
        p.snaps[slot] = snap;
        e.rec.frontier.set(depth, offset, width);
        let more = joint_rec(depth + 1, p, domain, bounds, orders, e, seed_bonus)?;
        e.rec.clock.lap(depth);
        if !more {
            e.unexplored_choices(p, depth, offset + 1, tail_check, root.as_ref(), domain, bounds, orders)?;
            return Ok(false);
        }
    }
    Ok(true)
}

fn members_rec(
    slot: usize,
    p: &mut PhysicalDeck,
    candidates: &[usize],
    required: &[usize],
    leader: Option<usize>,
    snaps: &[usize],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if required.iter().filter(|r| !p.members[..slot].contains(r)).count() > 5 - slot {
        return Ok(true);
    }
    if slot == 5 {
        return snaps_rec(0, p, snaps, e);
    }
    for (index, &m) in candidates.iter().enumerate() {
        if slot == 2 && leader.is_some_and(|l| l != m) {
            continue;
        }
        if p.members[..slot].iter().any(|&i| e.pool.members[i].character_id == e.pool.members[m].character_id) {
            continue;
        }
        // Picking another card of a required character can never satisfy that requirement.
        if required.iter().any(|&r| r != m && e.pool.members[r].character_id == e.pool.members[m].character_id) {
            continue;
        }
        p.members[slot] = m;
        e.rec.frontier.set(slot, index, candidates.len());
        if !members_rec(slot + 1, p, candidates, required, leader, snaps, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn snaps_rec(slot: usize, p: &mut PhysicalDeck, snaps: &[usize], e: &mut Engine<'_, '_>) -> Result<bool, Error> {
    e.tel.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if slot == 5 {
        return e.consider(*p);
    }
    p.snaps[slot] = None;
    e.rec.frontier.set(5 + slot, 0, snaps.len() + 1);
    if !snaps_rec(slot + 1, p, snaps, e)? {
        return Ok(false);
    }
    for (index, &s) in snaps.iter().enumerate() {
        if p.snaps[..slot].contains(&Some(s)) {
            continue;
        }
        p.snaps[slot] = Some(s);
        e.rec.frontier.set(5 + slot, index + 1, snaps.len() + 1);
        if !snaps_rec(slot + 1, p, snaps, e)? {
            return Ok(false);
        }
    }
    p.snaps[slot] = None;
    Ok(true)
}
struct ProposalRandom(u64);
impl ProposalRandom {
    fn index(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}
fn random_deck(
    pool: &Pool,
    candidates: &[usize],
    required: &[usize],
    leader: Option<usize>,
    snaps: &[usize],
    r: &mut ProposalRandom,
) -> PhysicalDeck {
    let mut selected = required.to_vec();
    let offset = r.index(candidates.len());
    if selected.len() < 5 {
        for i in 0..candidates.len() {
            let m = candidates[(i + offset) % candidates.len()];
            if selected.iter().any(|&a| pool.members[a].character_id == pool.members[m].character_id) {
                continue;
            }
            selected.push(m);
            if selected.len() == 5 {
                break;
            }
        }
    }
    // Required may already contain five cards.
    selected.truncate(5);
    for i in (1..5).rev() {
        let j = r.index(i + 1);
        selected.swap(i, j);
    }
    if let Some(l) = leader {
        let at = selected.iter().position(|&i| i == l).expect("leader is required");
        selected.swap(2, at);
    }
    let mut p = PhysicalDeck { members: selected.try_into().expect("feasible five characters"), snaps: [None; 5] };
    for i in 0..5 {
        let s = r.index(snaps.len() + 1);
        if s < snaps.len() && !p.snaps[..i].contains(&Some(snaps[s])) {
            p.snaps[i] = Some(snaps[s]);
        }
    }
    p
}

struct CurrentDeclaredOutcome {
    terminal: expectation::ConditionalOutcome,
    network_applications: Vec<(usize, usize)>,
}
fn simulate_current<F: FnMut() -> bool>(
    input: &FiniteSeedContext,
    master: &ournotes_sim::master::Master,
    root_seed: i32,
    confirmations: Option<&[RankConfirmation]>,
    finished_from_frame: Option<usize>,
    mut cancelled: F,
) -> Result<Option<CurrentDeclaredOutcome>, Error> {
    if confirmations.is_some() || finished_from_frame.is_some() {
        return Err(Error::Unsupported("network/finished lifecycle is unsupported".into()));
    }
    if cancelled() {
        return Ok(None);
    }
    let terminal = input.simulate(master, root_seed)?;
    if cancelled() {
        return Ok(None);
    }
    Ok(Some(CurrentDeclaredOutcome { terminal, network_applications: Vec::new() }))
}
