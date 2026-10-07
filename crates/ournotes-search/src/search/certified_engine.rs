//! The interval frontier of the ordinary production traversal. Exact incumbents are never synthesized from bounds.
use super::*;
use crate::search::{
    certified_search::{
        BestOrderWitness, CertifiedEvaluation, PayoffMap, aggregate_orders, canonicalize_performers,
        canonicalize_performers_with_basis, refine_order_with_exact_law, refinement_uncertainty,
    },
    interval_topk::{CandidateInterval, CanonicalTie, IntervalTopK, RankingProof, RemainingDomain},
};
use ournotes_sim::live::certified::F64Interval;
use std::collections::BTreeSet;

/// How a played Gekisou request treats the lottery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LotteryMode {
    /// No candidate reads a probability: every live is deterministic as it is.
    Absent,
    /// A candidate reads a probability in a live without a LUCK range: its decks play lottery-free
    /// ([`ournotes_sim::live::full::prepare_lottery_free`]), deterministic again.
    Free,
    /// A LUCK range draws lotteries: the certified interval traversal.
    Certified,
}

pub(super) fn lottery_mode(
    pool: &Pool,
    request: &SearchRequest,
    domain: &crate::domain::CandidateDomain,
) -> Result<LotteryMode, Error> {
    if !matches!(request.objective.inner(), Objective::LiveScore { gekisou: Some(_), .. }) {
        return Ok(LotteryMode::Absent);
    }
    let declared = request.objective.context().map(|c| &c.gekisou);
    if declared.is_some_and(|g| g.missions.iter().take(g.fevers.len()).any(|&mission| mission == 2)) {
        return Ok(LotteryMode::Certified);
    }
    let probability: HashSet<_> =
        pool.master.skill_conditions.iter().filter(|c| c.condition_type == 4011).map(|c| c.id).collect();
    let groups: HashSet<_> = pool
        .master
        .skill_condition_sets
        .iter()
        .filter(|set| set.condition_ids.iter().any(|id| probability.contains(id)))
        .map(|set| set.group)
        .collect();
    let members: HashSet<_> = domain
        .members()
        .iter()
        .map(|&m| (pool.members[m].gekisou_skill_id, pool.members[m].gekisou_skill_level))
        .collect();
    let mut supports = HashSet::new();
    for &snap in domain.snaps() {
        supports.extend(pool.snaps[snap].gekisou_support_skills()?);
    }
    let reads = |row: &ournotes_sim::master::GekisouSkillEffectRow| {
        [
            row.skill_trigger_condition_group,
            row.skill_condition_group,
            row.skill_release_condition_group,
            row.effect_execute_limit_reset_condition_group,
        ]
        .iter()
        .any(|group| groups.contains(group))
    };
    let reads_probability =
        pool.master.gekisou_skill_effects.iter().any(|row| members.contains(&(row.skill_id, row.level)) && reads(row))
            || pool
                .master
                .gekisou_support_skill_effects
                .iter()
                .any(|row| supports.contains(&(row.skill_id, row.level)) && reads(row));
    Ok(match (reads_probability, declared) {
        (false, _) => LotteryMode::Absent,
        // Only a resolved scenario declares the ranges; without one a LUCK range cannot be ruled out.
        (true, None) => LotteryMode::Certified,
        (true, Some(_)) => LotteryMode::Free,
    })
}

pub(super) struct CertifiedEntry {
    physical: PhysicalDeck,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    refinement: Option<Box<RetainedRefinement>>,
    best_order: Option<Box<BestOrderWitness>>,
}

struct RetainedRefinement {
    /// Retained independently of the optional score cache: partial order refinements are proof state.
    evaluation: CertifiedEvaluation,
    map: PayoffMap,
    program: Vec<u8>,
}

/// Complete canonical Performer programs at the same power determine the same all-order score objective.
/// Each Performer retains its paired support; other live inputs and the metric are fixed by the owning Engine.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ScoreCapKey {
    program: Vec<u8>,
    power: i32,
}

impl ScoreCapKey {
    fn new(program: Vec<u8>, power: i32) -> Self {
        Self { program, power }
    }
}

/// Aggregate equality is separate from the original performer basis retained for per-order refinement.
/// The frontier scopes the immutable request, exact power and complete payoff mapping independently.
fn uniform_score_equality_identity(
    master: &ournotes_sim::master::Master,
    metric: &Metric,
    map: &PayoffMap,
    performers: &[ournotes_sim::live::full::Performer; 5],
) -> Option<Vec<u8>> {
    if !matches!(metric, Metric::Score) || !matches!(map, PayoffMap::Score) {
        return None;
    }
    ournotes_sim::live::full::character_blind_uniform_score_identity(master, performers)
}

#[derive(Default)]
struct ScoreCapCache {
    rows: BTreeMap<ScoreCapKey, i128>,
}

impl ScoreCapCache {
    fn get(&self, program: &[u8], power: i32, telemetry: &mut telemetry::CacheUse) -> Option<i128> {
        telemetry.lookups += 1;
        let cap = self.rows.get(&ScoreCapKey::new(program.to_vec(), power)).copied();
        telemetry.hits += u64::from(cap.is_some());
        cap
    }

    fn insert(
        &mut self,
        program: Vec<u8>,
        power: i32,
        cap: i128,
        capacity: usize,
        telemetry: &mut telemetry::CacheUse,
    ) {
        let capacity = capacity.min(64);
        if capacity == 0 {
            return;
        }
        let key = ScoreCapKey::new(program, power);
        if let Some(old) = self.rows.get_mut(&key) {
            *old = (*old).min(cap);
            return;
        }
        if self.rows.len() >= capacity {
            self.rows.pop_first();
            telemetry.evictions += 1;
        }
        self.rows.insert(key, cap);
        telemetry.peak_entries = telemetry.peak_entries.max(self.rows.len());
    }
}

impl RetainedRefinement {
    fn new(admitted: bool, evaluation: CertifiedEvaluation, map: PayoffMap, program: Vec<u8>) -> Option<Box<Self>> {
        admitted.then(|| Box::new(Self { evaluation, map, program }))
    }
}

pub(super) struct CertifiedState {
    frontier: IntervalTopK,
    entries: BTreeMap<u64, CertifiedEntry>,
    cutoff: Option<(i128, i32)>,
    score_cache: BTreeMap<(Vec<u8>, i32), CertifiedEvaluation>,
    /// Whole-program mean-score caps remain useful when a losing team stops before all order evaluations.
    score_caps: ScoreCapCache,
    /// Refinement can hit its deadline after the physical traversal already closed the domain.
    domain_exhausted: bool,
    /// Small charts retain order state eagerly; other charts materialize one boundary candidate at a time.
    retain_refinement: Option<bool>,
    /// The master's lottery-related skills, classified once per request.
    luck_skills: Option<std::sync::Arc<ournotes_sim::live::full::LuckSkills>>,
    /// Certified lottery curves of this request, shared by every performance order and team.
    pub(super) luck_curves: ournotes_sim::live::full::LuckDpCache,
}

/// Key bytes the request's lottery-curve cache may hold.
pub(super) const LUCK_CURVE_CACHE_BYTES: usize = 32 * 1024 * 1024;

impl CertifiedState {
    pub(super) fn new(k: usize, curve_bytes: usize) -> Result<Self, Error> {
        Ok(Self {
            frontier: IntervalTopK::new(k)?,
            entries: BTreeMap::new(),
            cutoff: None,
            score_cache: BTreeMap::new(),
            score_caps: ScoreCapCache::default(),
            domain_exhausted: false,
            retain_refinement: None,
            luck_skills: None,
            luck_curves: ournotes_sim::live::full::LuckDpCache::new(curve_bytes),
        })
    }
    pub(super) fn contains(&self, p: &PhysicalDeck) -> bool {
        self.entries.values().any(|e| e.physical == *p)
    }
    /// The exact payoff (None: not proved) and power of an evaluated deck still on the frontier.
    pub(super) fn evaluated(&self, p: &PhysicalDeck) -> Option<(Option<super::expectation::ExactExpectation>, i32)> {
        let (&id, entry) = self.entries.iter().find(|(_, e)| e.physical == *p)?;
        Some((self.frontier.get(id)?.exact_payoff, entry.power))
    }

    fn proof(&self, exhausted: bool, upper: Option<f64>) -> Result<RankingProof, Error> {
        self.frontier.proof(if exhausted || self.domain_exhausted {
            RemainingDomain::Exhausted
        } else {
            RemainingDomain::Open { upper }
        })
    }

    /// Keep equality-class restrictions when a fresh per-order certificate narrows this candidate.
    fn install_refinement(&mut self, id: u64, evaluation: CertifiedEvaluation) -> Result<(), Error> {
        let current = self.frontier.get(id).expect("live boundary candidate");
        let score = current
            .score
            .intersect(evaluation.score)
            .ok_or_else(|| Error::Domain("conflicting refined score certificates".into()))?;
        let payoff = current
            .payoff
            .intersect(evaluation.payoff)
            .ok_or_else(|| Error::Domain("conflicting refined payoff certificates".into()))?;
        self.frontier.refine(id, current.revision, score, payoff, evaluation.exact_score, evaluation.exact_payoff)?;
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.best_order = evaluation.best_order.clone();
            entry.refinement.as_mut().expect("retained boundary candidate").evaluation = evaluation;
        }
        self.entries.retain(|id, _| self.frontier.get(*id).is_some());
        self.cutoff = self.frontier.grid_cutoff(ORDERS as u128);
        Ok(())
    }
}

/// Storage policy for eagerly retained order rows. Other charts use boundary materialization.
fn retain_order_state(notes: usize, frames: usize) -> bool {
    notes <= 32 && frames <= 512
}

/// A completed certificate starts no further work and therefore does not open a new stop reason.
fn pending_refinement(proof: RankingProof, stopped: impl FnOnce() -> bool) -> Option<RankingProof> {
    if proof.complete || stopped() { None } else { Some(proof) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryStage {
    FactorHistory,
    RankResidue,
}

enum EqualityRefinement {
    Merged,
    Declined,
    Stopped,
}

impl SummaryStage {
    fn record_order(self, telemetry: &mut telemetry::LotteryRefinement) {
        match self {
            Self::FactorHistory => telemetry.summary_orders += 1,
            Self::RankResidue => telemetry.residue_orders += 1,
        }
    }

    fn record_refinement(self, telemetry: &mut telemetry::LotteryRefinement) {
        match self {
            Self::FactorHistory => telemetry.summary_refinements += 1,
            Self::RankResidue => telemetry.residue_refinements += 1,
        }
    }

    fn record_decline(self, telemetry: &mut telemetry::LotteryRefinement) {
        match self {
            Self::FactorHistory => telemetry.summary_declines += 1,
            Self::RankResidue => telemetry.residue_declines += 1,
        }
    }
}

/// Every currently overlapping candidate gets the cheaper history pass before any residue pass.
fn next_summary_refinement(
    candidates: &[u64],
    summarized: &BTreeSet<u64>,
    residue_attempted: &BTreeSet<u64>,
) -> Option<(u64, SummaryStage)> {
    candidates.iter().copied().find(|id| !summarized.contains(id)).map(|id| (id, SummaryStage::FactorHistory)).or_else(
        || {
            candidates
                .iter()
                .copied()
                .find(|id| !residue_attempted.contains(id))
                .map(|id| (id, SummaryStage::RankResidue))
        },
    )
}

impl Engine<'_, '_> {
    pub(super) fn admit_certified_refinement(&mut self, notes: usize, frames: usize) {
        self.certified
            .as_mut()
            .expect("certified request")
            .retain_refinement
            .get_or_insert(retain_order_state(notes, frames));
    }

    pub(super) fn certified_luck_skills(
        &mut self,
    ) -> Result<std::sync::Arc<ournotes_sim::live::full::LuckSkills>, Error> {
        let state = self.certified.as_mut().expect("certified request");
        if state.luck_skills.is_none() {
            state.luck_skills = Some(std::sync::Arc::new(ournotes_sim::live::full::luck_skills(self.pool.master)?));
        }
        Ok(state.luck_skills.clone().expect("classified above"))
    }
    pub(super) fn cached_certified_score(&self, key: &[u8], power: i32) -> Option<CertifiedEvaluation> {
        self.certified.as_ref()?.score_cache.get(&(key.to_vec(), power)).cloned()
    }
    pub(super) fn cache_certified_score(&mut self, key: Vec<u8>, power: i32, value: &CertifiedEvaluation) {
        let capacity = self.limits.cache_entries.min(64);
        if capacity == 0 {
            return;
        }
        let cache = &mut self.certified.as_mut().expect("certified request").score_cache;
        if cache.len() >= capacity {
            cache.pop_first();
        }
        cache.insert((key, power), value.clone());
    }

    pub(super) fn cached_certified_score_cap(&mut self, program: &[u8], power: i32) -> Option<i128> {
        self.certified.as_ref()?.score_caps.get(program, power, &mut self.tel.caches.luck_score_caps)
    }

    pub(super) fn cache_certified_score_cap(&mut self, program: Vec<u8>, power: i32, cap: i128) {
        self.certified.as_mut().expect("certified request").score_caps.insert(
            program,
            power,
            cap,
            self.limits.cache_entries,
            &mut self.tel.caches.luck_score_caps,
        );
    }
    /// Integer node threshold over 120 orders. On the certified frontier an equal node upper is closed only below
    /// the returned power (i32::MIN when no K candidates prove that tie); public-ID ties are not used.
    pub(super) fn safe_cutoff(&self) -> Option<(i128, i32)> {
        // A census prunes against its fixed threshold, without a power tie-break.
        if let (None, Some(threshold)) = (&self.certified, crate::search::snaps::census()) {
            return Some((threshold, i32::MIN));
        }
        match &self.certified {
            Some(state) => state.cutoff,
            None => (self.top.len() == self.request.k).then(|| {
                let kth = self.top.last().expect("full exact Top-K");
                (kth.evaluation.expected_payoff.numerator, kth.power)
            }),
        }
    }

    pub(super) fn offer_certified(
        &mut self,
        physical: PhysicalDeck,
        power: i32,
        evaluation: CertifiedEvaluation,
        program_identity: Vec<u8>,
        map: PayoffMap,
    ) -> Result<(), Error> {
        // The leaf transfers its already constructed map; never rebuild a deck's native payoff steps.
        let (_, resume) = self.rec.clock.lap(slot::INTERVAL_FRONTIER);
        let payoff_identity = format!("{map:?}").into_bytes();
        let equality_identity = if matches!(self.metric, Metric::Score) && matches!(&map, PayoffMap::Score) {
            let input = expectation::context(self.pool, &physical, &self.request.objective)?;
            uniform_score_equality_identity(self.pool.master, self.metric, &map, &input.performers)
        } else {
            None
        };
        let state = self.certified.as_mut().expect("certified request");
        let members = physical.members.map(|i| self.pool.members[i].id);
        let snaps = physical.snaps.map(|i| i.map(|i| self.pool.snaps[i].id));
        let mut key = members.to_vec();
        for snap in snaps {
            key.extend(match snap {
                None => [0, 0],
                Some(id) => [1, id],
            });
        }
        let id = self.tel.leaves.visited;
        let best_order = evaluation.best_order.clone();
        let partial = best_order.as_ref().is_some_and(|best| best.evaluated_orders < ORDERS && !best.optimal);
        let retained_program = (state.retain_refinement == Some(true)).then(|| program_identity.clone());
        let equality =
            state.frontier.certify_equal_program(equality_identity.unwrap_or(program_identity), power, payoff_identity);
        state.frontier.insert(CandidateInterval {
            id,
            tie: CanonicalTie { power, key },
            score: evaluation.score,
            payoff: evaluation.payoff,
            exact_score: evaluation.exact_score,
            exact_payoff: evaluation.exact_payoff,
            equality: Some(equality),
            revision: 0,
        })?;
        // Its equality class may already have settled the payoff further.
        let exact = state.frontier.get(id).map_or(evaluation.exact_payoff, |c| c.exact_payoff);
        self.offered = Some(super::Offered { payoff: exact.map(|x| (x.numerator, x.denominator)), power, score: None });
        let refinement = RetainedRefinement::new(
            state.retain_refinement == Some(true) && state.frontier.get(id).is_some(),
            evaluation,
            map,
            retained_program.unwrap_or_default(),
        );
        state.entries.insert(id, CertifiedEntry { physical, members, snaps, power, refinement, best_order });
        state.entries.retain(|id, _| state.frontier.get(*id).is_some());
        state.cutoff = state.frontier.grid_cutoff(ORDERS as u128);
        self.tel.leaves.peak_retained = self.tel.leaves.peak_retained.max(state.entries.len());
        self.tel.leaves.evaluated += u64::from(!partial);
        self.tel.leaves.partial += u64::from(partial);
        self.remember(physical);
        self.rec.clock.lap(resume);
        self.report_progress();
        Ok(())
    }

    /// Once the physical domain is exhausted, spend bounded work only on candidates whose ordering still
    /// overlaps. Complete score enclosures precede native path expansion; partial trees keep the proved bounds.
    pub(super) fn refine_certified_frontier(&mut self) -> Result<(), Error> {
        let (_, resume) = self.rec.clock.lap(slot::INTERVAL_FRONTIER);
        let result = self.refine_certified_frontier_inner();
        self.rec.clock.lap(resume);
        result
    }

    fn refine_certified_frontier_inner(&mut self) -> Result<(), Error> {
        use ournotes_sim::live::full::{LuckExactBudget, LuckExactDecline, LuckExactSession};
        let state = self.certified.as_mut().expect("certified request");
        state.domain_exhausted = true;
        let mut work = LuckExactBudget::default();
        let mut attempted = std::collections::BTreeSet::<(u64, usize)>::new();
        let mut materialized = std::collections::BTreeSet::new();
        let mut summarized = std::collections::BTreeSet::new();
        let mut residue_attempted = std::collections::BTreeSet::new();
        let mut equality_attempted = false;
        loop {
            let state = self.certified.as_ref().expect("certified request");
            let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
            let Some(proof) = pending_refinement(proof, || self.expired() || work.exhausted()) else {
                return Ok(());
            };
            // One current boundary pair gets a bounded complete-law equality attempt before numerical
            // refinement. This is independent of per-order state and never changes the physical domain.
            if matches!(self.metric, Metric::Score) && !equality_attempted {
                equality_attempted = true;
                if let Some(pair) = &proof.refinement
                    && let Some(other) = pair.competitor
                {
                    match self.refine_certified_equality(pair.candidate, other, &mut work)? {
                        EqualityRefinement::Merged => continue,
                        EqualityRefinement::Declined => {}
                        EqualityRefinement::Stopped => return Ok(()),
                    }
                }
            }
            let state = self.certified.as_ref().expect("certified request");
            let mut candidates = proof.ambiguous.clone();
            // Smaller upper bounds identify contenders closer to exclusion by a proved lower bound.
            // This is a work priority; the frontier alone establishes every returned rank.
            candidates.sort_by(|a, b| {
                state
                    .frontier
                    .get(*a)
                    .expect("live candidate")
                    .payoff
                    .upper()
                    .total_cmp(&state.frontier.get(*b).expect("live candidate").payoff.upper())
                    .then(a.cmp(b))
            });
            // Finish factor histories across the overlapping score frontier, then refine their native
            // rank residues before any candidate can consume the shared native path budget.
            if matches!(self.metric, Metric::Score | Metric::BestOrderExpectedScore)
                && let Some((id, stage)) = next_summary_refinement(&candidates, &summarized, &residue_attempted)
            {
                match stage {
                    SummaryStage::FactorHistory => summarized.insert(id),
                    SummaryStage::RankResidue => residue_attempted.insert(id),
                };
                let retained = state.entries[&id].refinement.is_some();
                let complete = if retained {
                    self.refine_certified_summary(id, stage)?
                } else {
                    for entry in self.certified.as_mut().expect("certified request").entries.values_mut() {
                        entry.refinement = None;
                    }
                    let fresh_summary = stage == SummaryStage::FactorHistory;
                    if !self.materialize_certified_refinement(id, fresh_summary)? {
                        return Ok(());
                    }
                    fresh_summary || self.refine_certified_summary(id, stage)?
                };
                if !complete {
                    return Ok(());
                }
                continue;
            }
            let selected = candidates.iter().find_map(|&id| {
                let entry = &state.entries[&id];
                let retained = entry.refinement.as_ref()?;
                let mut indices: Vec<_> =
                    if matches!(retained.map, PayoffMap::Score | PayoffMap::BestOrderExpectedScore) {
                        retained
                            .evaluation
                            .orders
                            .iter()
                            .enumerate()
                            .filter_map(|(index, order)| {
                                (order.exact_mean.is_none()
                                    && !attempted.contains(&(id, index))
                                    && (!matches!(retained.map, PayoffMap::BestOrderExpectedScore)
                                        || order.mean.upper() >= retained.evaluation.score.lower()))
                                .then_some(index)
                            })
                            .collect()
                    } else {
                        retained
                            .evaluation
                            .refinements
                            .iter()
                            .filter_map(|refinement| {
                                (!attempted.contains(&(id, refinement.order_index))).then_some(refinement.order_index)
                            })
                            .collect()
                    };
                indices.sort_unstable();
                indices.dedup();
                (!indices.is_empty())
                    .then(|| (id, entry.physical, retained.program.clone(), retained.map.clone(), indices))
            });
            let Some((id, physical, program, map, indices)) = selected else {
                let next = candidates
                    .iter()
                    .copied()
                    .find(|id| state.entries[id].refinement.is_none() && !materialized.contains(id));
                let Some(id) = next else { return Ok(()) };
                materialized.insert(id);
                // Installed frontier intervals survive releasing the detailed order rows.
                // Only the selected boundary candidate requires full per-order storage.
                let state = self.certified.as_mut().expect("certified request");
                for entry in state.entries.values_mut() {
                    entry.refinement = None;
                }
                if !self.materialize_certified_refinement(id, false)? {
                    return Ok(());
                }
                continue;
            };
            let mut input = expectation::context(self.pool, &physical, &self.request.objective)?;
            if let Some(value) = self.simulation.music_length_ms {
                input.params.music_length_ms = value;
            }
            if let Some(value) = self.simulation.score_music_length_ms {
                input.params.score_music_length_ms = Some(value);
            }
            let physical_performers = input.performers.clone();
            if canonicalize_performers(&mut input) != program {
                return Err(Error::Domain("certified refinement changed the performer order basis".into()));
            }
            let setup =
                input.gekisou.as_ref().ok_or_else(|| Error::Domain("LUCK refinement requires Gekisou".into()))?;
            let orders = &self.certified.as_ref().expect("certified request").entries[&id]
                .refinement
                .as_ref()
                .expect("boundary candidate")
                .evaluation
                .orders;
            let mut priorities = indices
                .into_iter()
                .map(|index| {
                    let priority = if matches!(map, PayoffMap::BestOrderExpectedScore) {
                        orders[index].mean.upper()
                    } else {
                        refinement_uncertainty(&orders[index], &map)?
                    };
                    Ok((index, priority))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            // Uniform averaging gives every enclosure width the same aggregate weight. A maximum only
            // needs contenders above its proved floor, prioritized by their remaining upper bounds.
            // This schedules exact work; only the interval frontier certifies the ranking.
            priorities.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            let mut session = LuckExactSession::new(
                self.pool.master,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
                self.limits.cache_entries.min(64),
            )?;
            for (index, _) in priorities {
                if attempted.contains(&(id, index)) {
                    continue;
                }
                let state = self.certified.as_ref().expect("certified request");
                let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
                let Some(proof) = pending_refinement(proof, || self.expired()) else {
                    return Ok(());
                };
                if !proof.ambiguous.contains(&id) {
                    break;
                }
                let state = self.certified.as_ref().expect("certified request");
                let order =
                    state.entries[&id].refinement.as_ref().expect("admitted candidate").evaluation.orders[index].order;
                attempted.insert((id, index));
                let performers = order.map(|slot| {
                    if matches!(map, PayoffMap::BestOrderExpectedScore) {
                        physical_performers[slot].clone()
                    } else {
                        input.performers[slot].clone()
                    }
                });
                self.tel.lottery_refinement.attempted_orders += 1;
                let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                let result = session.law(&performers, &mut work, || self.expired());
                self.rec.clock.lap(resume);
                let result = result?;
                let telemetry = &mut self.tel.lottery_refinement;
                telemetry.budget_exhausted |= work.exhausted();
                telemetry.replay_runs += result.stats.replay_runs;
                telemetry.frames += result.stats.frames;
                telemetry.terminal_paths += result.stats.terminal_paths;
                let Some(law) = result.law else {
                    telemetry.declined_orders += 1;
                    if let Some(decline) = result.decline {
                        telemetry.declines.record(decline);
                    }
                    if result.decline == Some(LuckExactDecline::Cancelled) {
                        return Ok(());
                    }
                    if result.decline == Some(LuckExactDecline::Domain) {
                        // Every physical candidate has the same declared notes/frame schedule. Do not
                        // rebuild all live candidates just to rediscover this request-wide refusal.
                        return Ok(());
                    }
                    continue;
                };
                telemetry.completed_orders += 1;
                let state = self.certified.as_mut().expect("certified request");
                let retained = state
                    .entries
                    .get_mut(&id)
                    .expect("live candidate selected above")
                    .refinement
                    .as_mut()
                    .expect("admitted candidate");
                if !refine_order_with_exact_law(&mut retained.evaluation.orders[index], &map, &law)? {
                    self.tel.lottery_refinement.arithmetic_declines += 1;
                    continue;
                }
                let evaluation = aggregate_orders(retained.evaluation.orders.clone(), &map)?;
                state.install_refinement(id, evaluation)?;
                self.tel.lottery_refinement.installed_orders += 1;
                self.report_progress();
            }
        }
    }

    fn refine_certified_equality(
        &mut self,
        left: u64,
        right: u64,
        work: &mut ournotes_sim::live::full::LuckExactBudget,
    ) -> Result<EqualityRefinement, Error> {
        use ournotes_sim::live::full::{LuckScoreEquivalenceDecline, certify_uniform_score_equivalence};
        let state = self.certified.as_ref().expect("certified request");
        let (a, b) = (&state.entries[&left], &state.entries[&right]);
        if a.power != b.power {
            return Ok(EqualityRefinement::Declined);
        }
        let (a, b) = (a.physical, b.physical);
        self.tel.lottery_refinement.equality_attempts += 1;
        let mut input = expectation::context(self.pool, &a, &self.request.objective)?;
        let other = expectation::context(self.pool, &b, &self.request.objective)?;
        // The provider currently proves the native solo-rank law only. All other context fields come
        // from the same immutable request; context() varies only performers and the separately checked power.
        if input.rank_confirmations.is_some() || other.rank_confirmations.is_some() || input.gekisou.is_none() {
            self.tel.lottery_refinement.equality_declines += 1;
            self.tel.lottery_refinement.equality_decline_reason = Some(LuckScoreEquivalenceDecline::Context);
            return Ok(EqualityRefinement::Declined);
        }
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        let skills = self.certified_luck_skills()?;
        let before = (work.remaining_runs, work.remaining_frames);
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        let result = certify_uniform_score_equivalence(
            self.pool.master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            input.gekisou.as_ref().expect("admitted native solo law"),
            &input.play,
            &input.delta_times,
            &input.performers,
            &other.performers,
            work,
            || self.expired(),
        );
        self.rec.clock.lap(resume);
        let telemetry = &mut self.tel.lottery_refinement;
        // The provider deducts real work even if native execution fails. Count it exactly once here,
        // without labelling recordings as complete nominal paths or exact-law evaluations.
        telemetry.replay_runs += before.0 - work.remaining_runs;
        telemetry.frames += before.1 - work.remaining_frames;
        telemetry.budget_exhausted |= work.exhausted();
        let result = match result {
            Ok(result) => result,
            Err(Error::Unsupported(_)) => {
                telemetry.equality_declines += 1;
                telemetry.equality_decline_reason = Some(LuckScoreEquivalenceDecline::RecorderAdmission);
                return Ok(EqualityRefinement::Declined);
            }
            Err(Error::Capacity(_)) => {
                telemetry.equality_declines += 1;
                telemetry.equality_decline_reason = Some(LuckScoreEquivalenceDecline::Capacity);
                return Ok(EqualityRefinement::Declined);
            }
            Err(error) => return Err(error),
        };
        telemetry.equality_orders += result.orders_compared as u64;
        telemetry.equality_timeline_orders += result.timeline_orders;
        telemetry.equality_timeline_paths += result.timeline_paths;
        telemetry.equality_timeline_transitions += result.timeline_transitions;
        telemetry.equality_score_fold_queries += result.score_fold_queries;
        telemetry.equality_decline_reason = result.decline;
        if result.certificate.is_none() {
            telemetry.equality_declines += 1;
            return Ok(if result.decline == Some(LuckScoreEquivalenceDecline::Cancelled) {
                EqualityRefinement::Stopped
            } else {
                EqualityRefinement::Declined
            });
        }
        if result.orders_compared != ORDERS {
            return Err(Error::Domain("uniform score equality certificate omitted original order labels".into()));
        }
        let state = self.certified.as_mut().expect("certified request");
        state.frontier.merge_equal_classes(left, right)?;
        state.entries.retain(|id, _| state.frontier.get(*id).is_some());
        state.cutoff = state.frontier.grid_cutoff(ORDERS as u128);
        self.tel.lottery_refinement.equality_merges += 1;
        self.report_progress();
        Ok(EqualityRefinement::Merged)
    }

    /// Existing order rows allow each completed summary to survive a later cancellation.
    fn refine_certified_summary(&mut self, id: u64, stage: SummaryStage) -> Result<bool, Error> {
        let entry = &self.certified.as_ref().expect("certified request").entries[&id];
        let retained = entry.refinement.as_ref().expect("retained boundary candidate");
        let (physical, program, map) = (entry.physical, retained.program.clone(), retained.map.clone());
        let mut indices: Vec<_> = retained
            .evaluation
            .orders
            .iter()
            .enumerate()
            .filter_map(|(index, order)| order.exact_mean.is_none().then_some(index))
            .collect();
        indices.sort_by(|&a, &b| {
            let priority = |index: usize| {
                let order = &retained.evaluation.orders[index];
                if matches!(map, PayoffMap::BestOrderExpectedScore) {
                    order.mean.upper()
                } else {
                    order.mean.upper() - order.mean.lower()
                }
            };
            priority(b).total_cmp(&priority(a)).then(a.cmp(&b))
        });
        let mut input = expectation::context(self.pool, &physical, &self.request.objective)?;
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        let physical_performers = input.performers.clone();
        if canonicalize_performers(&mut input) != program {
            return Err(Error::Domain("certified summary changed the performer order basis".into()));
        }
        let setup = input.gekisou.as_ref().ok_or_else(|| Error::Domain("LUCK refinement requires Gekisou".into()))?;
        let skills = self.certified_luck_skills()?;
        let master = self.pool.master;
        let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
        let result = (|| {
            let mut session = ournotes_sim::live::full::LuckScoreSession::new(
                master,
                &skills,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
            );
            for index in indices {
                let state = self.certified.as_ref().expect("certified request");
                let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
                if proof.complete || !proof.ambiguous.contains(&id) {
                    return Ok(true);
                }
                if self.expired() {
                    return Ok(false);
                }
                let state = self.certified.as_ref().expect("certified request");
                let retained = state.entries[&id].refinement.as_ref().expect("retained boundary candidate");
                let order = &retained.evaluation.orders[index];
                if matches!(map, PayoffMap::BestOrderExpectedScore)
                    && order.mean.upper() < retained.evaluation.score.lower()
                {
                    continue;
                }
                let performers = order.order.map(|slot| {
                    if matches!(map, PayoffMap::BestOrderExpectedScore) {
                        physical_performers[slot].clone()
                    } else {
                        input.performers[slot].clone()
                    }
                });
                let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                let summary = match stage {
                    SummaryStage::FactorHistory => session.summary(&performers, Some(&mut curves), || self.expired()),
                    SummaryStage::RankResidue => {
                        session.rank_summary(&performers, Some(&mut curves), || self.expired())
                    }
                };
                self.rec.clock.lap(resume);
                let summary = match summary {
                    Ok(Some(summary)) => summary,
                    Ok(None) => return Ok(false),
                    Err(Error::Unsupported(_) | Error::Capacity(_)) => {
                        stage.record_decline(&mut self.tel.lottery_refinement);
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                self.tel.leaves.simulations += 1;
                stage.record_order(&mut self.tel.lottery_refinement);
                let state = self.certified.as_mut().expect("certified request");
                let retained = state
                    .entries
                    .get_mut(&id)
                    .expect("live boundary candidate")
                    .refinement
                    .as_mut()
                    .expect("retained boundary candidate");
                retained.evaluation.orders[index].refine_summary(summary)?;
                let evaluation = aggregate_orders(retained.evaluation.orders.clone(), &map)?;
                state.install_refinement(id, evaluation)?;
                stage.record_refinement(&mut self.tel.lottery_refinement);
                self.report_progress();
            }
            Ok(true)
        })();
        self.tel.caches.luck_curves.record(curves.stats());
        self.certified.as_mut().expect("certified request").luck_curves = curves;
        result
    }

    /// Reconstruct the fixed candidate's complete order enclosures from immutable request data.
    fn materialize_certified_refinement(&mut self, id: u64, fresh_summary: bool) -> Result<bool, Error> {
        let entry = &self.certified.as_ref().expect("certified request").entries[&id];
        let (physical, power) = (entry.physical, entry.power);
        let mut input = expectation::context(self.pool, &physical, &self.request.objective)?;
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        if !ournotes_sim::live::full::LuckExactBudget::admits_chart(input.notes.len(), input.play.frames.len()) {
            return Ok(false);
        }
        let (program, basis) = canonicalize_performers_with_basis(&mut input);
        let cached = (!fresh_summary).then(|| self.cached_certified_score(&program, power)).flatten();
        let score = if let Some(cached) = cached {
            cached
        } else {
            let skills = self.certified_luck_skills()?;
            let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
            let master = self.pool.master;
            let mut completed_orders = 0;
            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
            let result = crate::search::certified_search::evaluate_luck_context_bounded_policy(
                master,
                &skills,
                &input,
                &PayoffMap::Score,
                Some(&mut curves),
                !fresh_summary && matches!(self.metric, Metric::Score | Metric::BestOrderExpectedScore),
                (&(0..ORDERS).collect::<Vec<_>>(), false, |event| {
                    if matches!(event, crate::search::certified_search::LuckContextEvent::Scored { .. }) {
                        completed_orders += 1;
                    }
                    true
                }),
                || self.expired(),
            );
            self.rec.clock.lap(resume);
            self.tel.caches.luck_curves.record(curves.stats());
            self.certified.as_mut().expect("certified request").luck_curves = curves;
            self.tel.leaves.simulations += completed_orders;
            if fresh_summary {
                self.tel.lottery_refinement.summary_orders += completed_orders;
                if matches!(&result, Err(Error::Unsupported(_) | Error::Capacity(_))) {
                    self.tel.lottery_refinement.summary_declines += 1;
                    return self.materialize_certified_refinement(id, false);
                }
            }
            let crate::search::certified_search::LuckContextOutcome::Full(score) = result? else { return Ok(false) };
            if fresh_summary {
                self.cache_certified_score(program.clone(), power, &score);
            }
            score
        };
        let support = (
            score.orders.iter().map(|order| order.support.0).min().expect("complete order set"),
            score.orders.iter().map(|order| order.support.1).max().expect("complete order set"),
        );
        let map = crate::search::certified_payoff::payoff_map(
            self.pool,
            self.request,
            self.metric,
            self.event_input,
            &physical,
            power,
            support,
        )?;
        let orders = score
            .orders
            .into_iter()
            .map(|mut order| {
                if matches!(map, PayoffMap::BestOrderExpectedScore) {
                    order.order = order.order.map(|slot| basis[slot]);
                }
                order
            })
            .collect();
        let evaluation = aggregate_orders(orders, &map)?;
        let state = self.certified.as_mut().expect("certified request");
        let entry = state.entries.get_mut(&id).expect("boundary candidate");
        entry.best_order = evaluation.best_order.clone();
        entry.refinement = RetainedRefinement::new(true, evaluation.clone(), map, program);
        if fresh_summary {
            state.install_refinement(id, evaluation)?;
            self.tel.lottery_refinement.summary_refinements += 1;
            self.report_progress();
        }
        Ok(true)
    }

    pub(super) fn certified_results(&self, exhausted: bool) -> Result<(Vec<RecommendedDeck>, RankingProof), Error> {
        let state = self.certified.as_ref().expect("certified request");
        let upper = if self.rec.bounded {
            self.rec.unexplored.map(|upper| {
                F64Interval::integer(upper)
                    .divide(F64Interval::integer(ORDERS as i128))
                    .expect("positive divisor")
                    .upper()
            })
        } else {
            None
        };
        let proof = state.proof(exhausted, upper)?;
        let results = proof
            .ordered_prefix
            .iter()
            .map(|&id| (id, true))
            .chain(proof.ambiguous.iter().map(|&id| (id, false)))
            .map(|(id, ranked)| {
                let entry = &state.entries[&id];
                // Equality classes may have narrowed since this candidate's original evaluation.
                let value = state.frontier.get(id).expect("live candidate");
                Ok(RecommendedDeck {
                    members: entry.members,
                    snaps: entry.snaps,
                    power: entry.power,
                    expected_score: value.exact_score.map(Into::into),
                    expected_payoff: value.exact_payoff.map(Into::into),
                    score_interval: Some(FractionInterval::from_f64(value.score.lower(), value.score.upper())?),
                    payoff_interval: Some(FractionInterval::from_f64(value.payoff.lower(), value.payoff.upper())?),
                    rank_certified: Some(ranked),
                    score_summary: None,
                    best_order: None,
                    best_expected_order: entry
                        .best_order
                        .as_ref()
                        .map(|order| {
                            Ok::<_, Error>(ExpectedOrderResult {
                                performance_order: order.order,
                                members: order.order.map(|slot| entry.members[slot]),
                                expected_score: order.exact_mean.map(Into::into),
                                score_interval: FractionInterval::from_f64(order.mean.lower(), order.mean.upper())?,
                                optimality: if order.optimal { Optimality::Proven } else { Optimality::Unproven },
                                evaluated_orders: order.evaluated_orders,
                            })
                        })
                        .transpose()?,
                    order_outcomes: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok((results, proof))
    }
}

#[cfg(test)]
mod refinement_tests {
    use super::*;

    #[test]
    fn summary_stages_cover_the_current_frontier_before_allowing_path_expansion() {
        let (mut summarized, mut residue_attempted) = (BTreeSet::new(), BTreeSet::new());
        for (id, stage) in [
            (3, SummaryStage::FactorHistory),
            (2, SummaryStage::FactorHistory),
            (1, SummaryStage::FactorHistory),
            (3, SummaryStage::RankResidue),
        ] {
            assert_eq!(next_summary_refinement(&[3, 2, 1], &summarized, &residue_attempted), Some((id, stage)));
            match stage {
                SummaryStage::FactorHistory => summarized.insert(id),
                SummaryStage::RankResidue => residue_attempted.insert(id),
            };
        }
        // Candidates outside the current overlap need no further work. A newly overlapping candidate
        // gets its history pass before an existing candidate's pending residue pass.
        assert_eq!(
            next_summary_refinement(&[2, 4], &summarized, &residue_attempted),
            Some((4, SummaryStage::FactorHistory))
        );
        summarized.insert(4);
        assert_eq!(
            next_summary_refinement(&[2, 4], &summarized, &residue_attempted),
            Some((2, SummaryStage::RankResidue))
        );
        // An optional refusal counts as an attempt and must not prevent the next candidate's pass.
        residue_attempted.insert(2);
        assert_eq!(
            next_summary_refinement(&[2, 4], &summarized, &residue_attempted),
            Some((4, SummaryStage::RankResidue))
        );
        residue_attempted.insert(4);
        assert_eq!(next_summary_refinement(&[2, 4], &summarized, &residue_attempted), None);
        assert_eq!(next_summary_refinement(&[], &summarized, &residue_attempted), None);
    }

    #[test]
    fn residue_telemetry_is_separate_from_history_and_exact_law_work() {
        let mut telemetry = telemetry::LotteryRefinement::default();
        SummaryStage::FactorHistory.record_order(&mut telemetry);
        SummaryStage::FactorHistory.record_refinement(&mut telemetry);
        SummaryStage::FactorHistory.record_decline(&mut telemetry);
        for _ in 0..2 {
            SummaryStage::RankResidue.record_order(&mut telemetry);
            SummaryStage::RankResidue.record_refinement(&mut telemetry);
            SummaryStage::RankResidue.record_decline(&mut telemetry);
        }
        let values = serde_json::to_value(telemetry).unwrap();
        for field in ["summaryOrders", "summaryRefinements", "summaryDeclines"] {
            assert_eq!(values[field], 1);
        }
        for field in ["residueOrders", "residueRefinements", "residueDeclines"] {
            assert_eq!(values[field], 2);
        }
        for field in [
            "equalityAttempts",
            "equalityOrders",
            "equalityMerges",
            "equalityDeclines",
            "attemptedOrders",
            "completedOrders",
            "installedOrders",
            "declinedOrders",
            "arithmeticDeclines",
            "replayRuns",
            "terminalPaths",
            "frames",
        ] {
            assert_eq!(values[field], 0);
        }
        assert_eq!(values["budgetExhausted"], false);
        assert!(values["equalityDeclineReason"].is_null());
    }

    fn score_rows(lower: f64, upper: f64) -> CertifiedEvaluation {
        use crate::search::certified_search::OrderScoreInterval;
        aggregate_orders(
            uniform::all_orders()
                .into_iter()
                .map(|order| OrderScoreInterval {
                    order,
                    evaluated: true,
                    mean: F64Interval::new(lower, upper).unwrap(),
                    support: (0, 100),
                    exact_mean: None,
                    final_life: None,
                    tails: BTreeMap::new(),
                    refined_payoff: None,
                })
                .collect(),
            &PayoffMap::Score,
        )
        .unwrap()
    }

    fn retain(state: &mut CertifiedState, id: u64, evaluation: CertifiedEvaluation) {
        state.entries.insert(
            id,
            CertifiedEntry {
                physical: PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] },
                members: [1, 2, 3, 4, 5],
                snaps: [None; 5],
                power: 100,
                best_order: None,
                refinement: RetainedRefinement::new(true, evaluation, PayoffMap::Score, vec![1]),
            },
        );
    }

    #[test]
    fn summary_refinement_can_finish_ranking_without_an_exact_score_law() {
        let mut state = frontier(2);
        state.domain_exhausted = true;
        retain(&mut state, 2, score_rows(3.0, 5.0));
        assert!(!state.proof(false, None).unwrap().complete);
        state.install_refinement(2, score_rows(3.0, 3.25)).unwrap();
        let proof = state.proof(false, None).unwrap();
        assert!(proof.complete);
        assert_eq!(proof.ordered_prefix, [1, 3]);
        assert!(state.frontier.get(2).is_none());
        assert!(state.frontier.get(1).unwrap().exact_score.is_none());
        assert!(pending_refinement(proof, || panic!("a proved rank needs no path expansion")).is_none());
    }

    #[test]
    fn summary_refinement_keeps_a_tighter_equality_class_certificate() {
        let mut state = CertifiedState::new(2, 0).unwrap();
        let equality = state.frontier.certify_equal_program(vec![1], 100, vec![0]);
        for (id, lower, upper) in [(1, 30.0, 50.0), (2, 40.0, 41.0)] {
            let value = F64Interval::new(lower, upper).unwrap();
            state
                .frontier
                .insert(CandidateInterval {
                    id,
                    tie: CanonicalTie { power: 100, key: vec![id as i64] },
                    score: value,
                    payoff: value,
                    exact_score: None,
                    exact_payoff: None,
                    equality: Some(equality.clone()),
                    revision: 0,
                })
                .unwrap();
        }
        retain(&mut state, 1, score_rows(30.0, 50.0));
        let tight = state.frontier.get(1).unwrap().score;
        state.install_refinement(1, score_rows(35.0, 45.0)).unwrap();
        assert_eq!(state.frontier.get(1).unwrap().score, tight);
        assert_eq!(state.frontier.get(2).unwrap().score, tight);
    }

    #[test]
    fn uniform_score_equality_preserves_physical_canonical_ties_and_the_unseen_domain() {
        use ournotes_sim::live::full::Performer;
        use ournotes_sim::master::Master;

        let master = Master::from_json_tables(|name| match name {
            "MasterLiveSkill" => Some(r#"{"_allData":[{"_id":1},{"_id":2}]}"#),
            "MasterLiveSkillEffect" => Some(
                r#"{"_allData":[
                {"_id":1,"_liveSkillID":1,"_level":1,"_skillEffectType":2000,"_effectValue":5000},
                {"_id":2,"_liveSkillID":2,"_level":1,"_skillEffectType":2000,"_effectValue":3000}]}"#,
            ),
            "MasterSupportSkillEffect" => Some(
                r#"{"_allData":[
                {"_id":1,"_supportSkillID":1,"_level":1,"_skillTriggerType":2,
                 "_skillEffectType":2000,"_effectValue":1000}]}"#,
            ),
            _ => None,
        })
        .unwrap();
        let mut original: [Performer; 5] = std::array::from_fn(|slot| Performer {
            character_id: slot as i64 + 1,
            live_skill: Some((if slot == 0 { 1 } else { 2 }, 1)),
            ..Default::default()
        });
        original[0].support_skills.push((1, 1));
        let mut alias = original.clone();
        for performer in &mut alias {
            performer.character_id += 100;
        }
        alias.rotate_left(2);
        assert_ne!(original, alias, "physical performer identities must remain distinct");
        let identity = |deck| uniform_score_equality_identity(&master, &Metric::Score, &PayoffMap::Score, deck);
        let original_identity = identity(&original).unwrap();
        assert_eq!(identity(&alias), Some(original_identity.clone()));

        // The aggregate proof must never replace an order-labelled program for Best or another payoff.
        assert!(
            uniform_score_equality_identity(
                &master,
                &Metric::BestOrderExpectedScore,
                &PayoffMap::BestOrderExpectedScore,
                &alias,
            )
            .is_none()
        );
        assert!(
            uniform_score_equality_identity(
                &master,
                &Metric::Score,
                &PayoffMap::ScoreAtLeast { threshold: 10 },
                &alias,
            )
            .is_none()
        );
        let mut unknown = alias.clone();
        unknown[0].support_skills.push((999, 1));
        assert!(identity(&unknown).is_none(), "an unproved selected source keeps its original identity");

        let mut frontier = IntervalTopK::new(2).unwrap();
        for (id, deck) in [(30, &original), (10, &alias), (20, &original)] {
            let equality = frontier.certify_equal_program(identity(deck).unwrap(), 100, b"Score".to_vec());
            frontier
                .insert(CandidateInterval {
                    id,
                    tie: CanonicalTie { power: 100, key: vec![id as i64] },
                    score: F64Interval::new(10.0, 12.0).unwrap(),
                    payoff: F64Interval::new(10.0, 12.0).unwrap(),
                    exact_score: None,
                    exact_payoff: None,
                    equality: Some(equality),
                    revision: 0,
                })
                .unwrap();
        }
        assert!(frontier.get(30).is_none(), "the larger physical canonical key loses the proved tie");
        assert_eq!(frontier.len(), 2);
        for remaining in [RemainingDomain::Open { upper: None }, RemainingDomain::Open { upper: Some(12.0) }] {
            let proof = frontier.proof(remaining).unwrap();
            assert!(!proof.complete && proof.ordered_prefix.is_empty(), "equal seen programs do not close unseen work");
        }
        let proof = frontier.proof(RemainingDomain::Exhausted).unwrap();
        assert!(proof.complete);
        assert_eq!(proof.ordered_prefix, [10, 20]);
        assert!(proof.ordered_prefix.iter().all(|&id| frontier.get(id).unwrap().exact_payoff.is_none()));

        // A different complete source-owner binding, or the same program at another exact power,
        // must remain an unresolved competitor when only overlapping intervals are available.
        let mut reattached = original.clone();
        reattached[1].support_skills = std::mem::take(&mut reattached[0].support_skills);
        let other_identity = identity(&reattached).unwrap();
        assert_ne!(other_identity, original_identity);
        for (program, power) in [(other_identity, 100), (original_identity, 101)] {
            let mut with_competitor = IntervalTopK::new(2).unwrap();
            for (id, program, power) in [(1, identity(&original).unwrap(), 100), (2, program, power)] {
                let equality = with_competitor.certify_equal_program(program, power, b"Score".to_vec());
                with_competitor
                    .insert(CandidateInterval {
                        id,
                        tie: CanonicalTie { power, key: vec![id as i64] },
                        score: F64Interval::new(10.0, 12.0).unwrap(),
                        payoff: F64Interval::new(10.0, 12.0).unwrap(),
                        exact_score: None,
                        exact_payoff: None,
                        equality: Some(equality),
                        revision: 0,
                    })
                    .unwrap();
            }
            let proof = with_competitor.proof(RemainingDomain::Exhausted).unwrap();
            assert!(!proof.complete && proof.ordered_prefix.is_empty());
            assert_eq!(proof.ambiguous.len(), 2);
        }
    }

    #[test]
    fn a_cancelled_summary_stage_keeps_completed_orders_and_exhausted_domain() {
        use ournotes_sim::live::certified::I32Interval;
        let mut state = frontier(2);
        state.domain_exhausted = true;
        retain(&mut state, 2, score_rows(3.0, 5.0));
        let retained = state.entries.get_mut(&2).unwrap().refinement.as_mut().unwrap();
        retained.evaluation.orders[0]
            .refine_summary(ournotes_sim::live::full::LuckScoreSummary {
                final_mean: F64Interval::integer(4).into(),
                final_support: I32Interval::point(4).into(),
                exact_constant_score: Some(4),
                exact_final_life: Some(1000),
                probability_peak_states: 1,
                probability_transitions: 0,
            })
            .unwrap();
        let evaluation = aggregate_orders(retained.evaluation.orders.clone(), &PayoffMap::Score).unwrap();
        state.install_refinement(2, evaluation).unwrap();
        let proof = state.proof(false, None).unwrap();
        assert_eq!(proof.ordered_prefix, [1]);
        assert!(!proof.complete);
        assert!(pending_refinement(proof, || true).is_none());
        let retained = state.entries[&2].refinement.as_ref().unwrap();
        assert_eq!(retained.evaluation.orders[0].support, (4, 4));
        assert!(retained.evaluation.orders[0].exact_mean.is_some());
        assert!(retained.evaluation.orders[1..].iter().all(|order| order.exact_mean.is_none()));
        let score = state.frontier.get(2).unwrap().score;
        assert!(score.lower() > 3.0 && score.upper() < 5.0);
        assert!(state.domain_exhausted);
    }

    fn cap_contexts() -> [FiniteSeedContext; 2] {
        use crate::search::PlayInput;
        use crate::search::gate_tests::common::{Rng, roster, set_column, synth_snaps};
        use ournotes_sim::live::model::JudgementStream;
        use ournotes_sim::live::score::LiveScoreSettings;
        use ournotes_sim::live::skip::{Chart, ChartNote};
        use serde_json::json;

        let mut source = synth_snaps(&mut Rng::new(381), 6, 1, &[2000]);
        let mut first = None;
        set_column(&mut source, "MasterMemberCard", &mut |row| {
            let id = row["_id"].clone();
            row["_characterID"] = id.clone();
            if id == 1 {
                first = Some(row.clone());
            } else if id == 6 {
                *row = first.clone().expect("first card");
                row["_id"] = json!(6);
            }
        });
        let master = source.master();
        let mut owned = roster(&mut Rng::new(382), &master);
        let mut alias = owned.members.iter().find(|member| member.id == 1).unwrap().clone();
        alias.id = 6;
        *owned.members.iter_mut().find(|member| member.id == 6).unwrap() = alias;
        let pool = Pool::new(&master, &owned).unwrap();
        assert_ne!(pool.members[0].id, pool.members[5].id);
        let chart = Chart::from_notes(
            vec![ChartNote { id: 1, time_ms: 100, note_type: 1 }],
            vec![],
            &LiveScoreSettings::from_master(&master).unwrap(),
        )
        .unwrap();
        let objective = Objective::LiveScore {
            score_id: 1004,
            play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types: vec![1] },
            chart,
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        };
        let original = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [Some(0), None, None, None, None] };
        let alias = PhysicalDeck { members: [5, 1, 2, 3, 4], snaps: original.snaps };
        let contexts = [original, alias].map(|deck| expectation::context(&pool, &deck, &objective).unwrap());
        assert_ne!(contexts[0].physical(), contexts[1].physical());
        assert_eq!(contexts[0].performers, contexts[1].performers);
        assert_eq!(contexts[0].params.total_power, contexts[1].params.total_power);
        // Verify this physical-card substitution against native execution before deriving the cache keys.
        for order in uniform::all_orders() {
            let scores =
                contexts.each_ref().map(|input| input.simulate_performance_order(&master, order).unwrap().final_score);
            assert_eq!(scores[0], scores[1]);
        }
        contexts
    }

    #[test]
    fn score_cap_cache_reuses_complete_performers_across_distinct_card_ids_and_layouts() {
        let [mut original, mut alias] = cap_contexts();
        let power = original.params.total_power;
        let program = canonicalize_performers(&mut original);
        let alias_program = canonicalize_performers(&mut alias);
        let mut cache = ScoreCapCache::default();
        let mut telemetry = telemetry::CacheUse::default();
        cache.insert(program.clone(), power, 12_345, 64, &mut telemetry);
        assert_eq!(cache.get(&alias_program, power, &mut telemetry), Some(12_345));
        for order in uniform::all_orders() {
            let mut rearranged = alias.clone();
            rearranged.performers = order.map(|slot| alias.performers[slot].clone());
            let key = canonicalize_performers(&mut rearranged);
            assert_eq!(cache.get(&key, power, &mut telemetry), Some(12_345));
        }
        assert_eq!(telemetry.hits, 121);
        assert_eq!(cache.get(&program, power + 1, &mut telemetry), None);

        let mut changed = original.clone();
        changed.performers[0].live_skill = Some((999, 1));
        assert_eq!(cache.get(&canonicalize_performers(&mut changed), power, &mut telemetry), None);
        let mut changed = original.clone();
        changed.performers[0].gekisou_support_skills.push((998, 2));
        assert_eq!(cache.get(&canonicalize_performers(&mut changed), power, &mut telemetry), None);
        let mut changed = original.clone();
        changed.performers[0].tag_ids.push(997);
        assert_eq!(cache.get(&canonicalize_performers(&mut changed), power, &mut telemetry), None);
        let mut reattached = original.clone();
        let carrier = reattached.performers.iter().position(|performer| !performer.support_skills.is_empty()).unwrap();
        let support = std::mem::take(&mut reattached.performers[carrier].support_skills);
        reattached.performers[(carrier + 1) % 5].support_skills = support;
        assert_eq!(cache.get(&canonicalize_performers(&mut reattached), power, &mut telemetry), None);
    }

    #[test]
    fn score_cap_cache_keeps_tight_caps_and_request_local_capacity() {
        let mut cache = ScoreCapCache::default();
        let mut telemetry = telemetry::CacheUse::default();
        cache.insert(vec![1, 2, 3], 100, 900, 0, &mut telemetry);
        assert_eq!(cache.get(&[1, 2, 3], 100, &mut telemetry), None);
        assert_eq!(telemetry.hits, 0);
        assert_eq!(telemetry.peak_entries, 0);
        cache.insert(vec![1, 2, 3], 100, 900, 1, &mut telemetry);
        cache.insert(vec![1, 2, 3], 100, 800, 1, &mut telemetry);
        cache.insert(vec![1, 2, 3], 100, 850, 1, &mut telemetry);
        assert_eq!(cache.get(&[1, 2, 3], 100, &mut telemetry), Some(800));
        let other_request = ScoreCapCache::default();
        assert_eq!(other_request.get(&[1, 2, 3], 100, &mut telemetry), None);
        cache.insert(vec![4, 5, 6], 100, 700, 1, &mut telemetry);
        assert_eq!(cache.get(&[1, 2, 3], 100, &mut telemetry), None);
        assert_eq!(cache.get(&[4, 5, 6], 100, &mut telemetry), Some(700));
        assert_eq!(telemetry.evictions, 1);
        assert_eq!(telemetry.peak_entries, 1);
    }

    fn frontier(k: usize) -> CertifiedState {
        let mut state = CertifiedState::new(k, 0).unwrap();
        for (id, lo, hi) in [(1, 10.0, 11.0), (2, 3.0, 5.0), (3, 4.0, 6.0)] {
            let bounds = F64Interval::new(lo, hi).unwrap();
            state
                .frontier
                .insert(CandidateInterval {
                    id,
                    tie: CanonicalTie { power: 100, key: vec![id as i64] },
                    score: bounds,
                    payoff: bounds,
                    exact_score: None,
                    exact_payoff: None,
                    equality: None,
                    revision: 0,
                })
                .unwrap();
        }
        state
    }

    #[test]
    fn eager_order_storage_is_independent_of_long_chart_refinement() {
        use crate::search::certified_search::OrderScoreInterval;
        use ournotes_sim::live::full::LuckExactBudget;
        for (notes, frames, expected) in [(33, 193, false), (12, 513, false), (12, 193, true)] {
            let orders = uniform::all_orders()
                .into_iter()
                .map(|order| OrderScoreInterval {
                    order,
                    evaluated: true,
                    mean: F64Interval::ONE,
                    support: (1, 1),
                    exact_mean: None,
                    final_life: Some((1000, 1000)),
                    tails: BTreeMap::new(),
                    refined_payoff: None,
                })
                .collect();
            let evaluation = aggregate_orders(orders, &PayoffMap::Score).unwrap();
            let entry = CertifiedEntry {
                physical: PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] },
                members: [1, 2, 3, 4, 5],
                snaps: [None; 5],
                power: 1,
                best_order: None,
                refinement: RetainedRefinement::new(
                    retain_order_state(notes, frames),
                    evaluation,
                    PayoffMap::Score,
                    vec![1, 2, 3],
                ),
            };
            assert!(LuckExactBudget::admits_chart(notes, frames));
            assert_eq!(entry.refinement.is_some(), expected);
            if let Some(retained) = entry.refinement {
                assert_eq!(retained.evaluation.orders.len(), ORDERS);
            }
        }
        eprintln!(
            "retained LUCK base order rows: {} bytes per admitted frontier candidate, excluding nested allocations",
            ORDERS * std::mem::size_of::<OrderScoreInterval>()
        );
    }

    #[test]
    fn a_refinement_timeout_retains_the_exhausted_domain_and_proved_prefix() {
        let mut state = frontier(2);
        assert!(state.proof(false, None).unwrap().ordered_prefix.is_empty());
        // The physical traversal closed before the refinement deadline, although payoff overlap remains.
        state.domain_exhausted = true;
        let proof = state.proof(false, None).unwrap();
        assert_eq!(proof.ordered_prefix, [1]);
        assert_eq!(proof.ambiguous.len(), 2);
        assert!(!proof.complete);
        assert!(pending_refinement(proof, || true).is_none());
        assert_eq!(state.proof(false, None).unwrap().ordered_prefix, [1]);
    }

    #[test]
    fn an_already_complete_frontier_does_not_read_a_later_refinement_deadline() {
        let proof = frontier(1).proof(true, None).unwrap();
        assert!(proof.complete);
        assert!(pending_refinement(proof, || panic!("a completed proof must not become a new timeout")).is_none());
    }
}
