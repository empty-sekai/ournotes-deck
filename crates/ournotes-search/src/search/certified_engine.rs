//! The interval frontier of the ordinary production traversal. Exact incumbents are never synthesized from bounds.
use super::*;
use crate::search::{
    certified_search::{
        CertifiedEvaluation, OrderScoreInterval, PayoffMap, aggregate_orders, canonicalize_performers,
        refine_order_with_exact_law, refinement_uncertainty,
    },
    interval_topk::{CandidateInterval, CanonicalTie, IntervalTopK, RankingProof, RemainingDomain},
};
use ournotes_sim::live::certified::F64Interval;

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

/// The Great note judgement of a judgement stream (`NoteSimulateJudgement`).
const GREAT_JUDGEMENT: i32 = 4;

/// The milliseconds since `started` on the search clock.
fn elapsed_ms(started: crate::clock::Instant) -> f64 {
    crate::search::budget::now().saturating_duration_since(started).as_secs_f64() * 1000.0
}

/// Whether a judgement conversion that some team of `domain` holds can change a Great judgement of the objective's
/// play. Certified lottery evaluation does not support such plays yet.
///
/// Each member performs alone, and each Snap beside a member with a Gekisou skill when the domain has one: a
/// performer holds its member's skills, its Snap's support skills, and its Snap's Gekisou support skills beside a
/// Gekisou skill, so every conversion a team of the domain holds is held here, and each one held here is held by some
/// team.
pub(super) fn converts_great(
    pool: &Pool,
    objective: &Objective,
    domain: &crate::domain::CandidateDomain,
) -> Result<bool, Error> {
    let Some(setup) = crate::search::full_setup(pool, objective)? else { return Ok(false) };
    let mut play = setup.play.clone();
    for frame in &mut play.frames {
        frame.judged.retain(|note| note.judgement == GREAT_JUDGEMENT);
    }
    if play.frames.iter().all(|frame| frame.judged.is_empty()) {
        return Ok(false);
    }
    let members = domain.members();
    let beside = members.iter().find(|&&m| pool.members[m].gekisou_skill_id != 0).or(members.first());
    let mut performers = Vec::with_capacity(members.len() + domain.snaps().len());
    for &m in members {
        performers.push(crate::search::snaps::performer(&pool.members[m], None)?);
    }
    if let Some(&m) = beside {
        for &s in domain.snaps() {
            performers.push(crate::search::snaps::performer(&pool.members[m], Some(&pool.snaps[s]))?);
        }
    }
    Ok(ournotes_sim::live::full::luck_play_changes_judgements(pool.master, &performers, &setup.notes, &play))
}

pub(super) struct CertifiedEntry {
    physical: PhysicalDeck,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    /// The identity of the candidate's terminal payoff mapping.
    payoff: Vec<u8>,
    refinement: Option<Box<RetainedRefinement>>,
}

struct RetainedRefinement {
    /// Retained independently of the optional score cache: partial order refinements are proof state.
    evaluation: CertifiedEvaluation,
    map: PayoffMap,
    program: Vec<u8>,
}

impl RetainedRefinement {
    fn new(admitted: bool, evaluation: CertifiedEvaluation, map: PayoffMap, program: Vec<u8>) -> Option<Box<Self>> {
        admitted.then(|| Box::new(Self { evaluation, map, program }))
    }
}

pub(super) struct CertifiedState<'m> {
    frontier: IntervalTopK,
    entries: BTreeMap<u64, CertifiedEntry>,
    cutoff: Option<(i128, i32)>,
    score_cache: BTreeMap<(Vec<u8>, i32), CertifiedEvaluation>,
    /// Refinement can hit its deadline after the physical traversal already closed the domain.
    domain_exhausted: bool,
    /// Small charts retain order state eagerly; other charts materialize one boundary candidate at a time.
    retain_refinement: Option<bool>,
    /// The master's lottery-related skills, classified once per request.
    luck_skills: Option<std::sync::Arc<ournotes_sim::live::full::LuckSkills>>,
    /// Certified lottery curves of this request, shared by every performance order and team.
    pub(super) luck_curves: ournotes_sim::live::full::LuckDpCache,
    /// Lottery kinds of this request's performers and the laws composed from them, made at their first use.
    pub(super) luck_kinds: Option<ournotes_sim::live::full::LotteryKinds<'m>>,
    pub(super) kind_bytes: usize,
    /// Joint bounds under the lottery laws of single teams.
    pub(super) law_bounds: super::leaf::LawBounds,
    /// The lottery census of the domain the traversal searches, whose multisets of kinds give the laws of its teams
    /// (see [`super::domain_laws::LotteryCensus::team_laws`]).
    pub(super) census: Option<super::domain_laws::LotteryCensus>,
}

/// Key bytes the request's lottery-curve cache may hold.
pub(super) const LUCK_CURVE_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// Bytes the composed lottery laws of a request may hold with their keys. The joint bounds of a domain hold the laws
/// of all its teams, so the composed laws they share take little more.
const LOTTERY_LAW_CACHE_BYTES: usize = 1024 * 1024 * 1024;

impl CertifiedState<'_> {
    /// `cache_bytes` bounds the keys of the lottery curves; the composed lottery laws with their keys take at most
    /// [`LOTTERY_LAW_CACHE_BYTES`], and none when `cache_bytes` is zero.
    pub(super) fn new(k: usize, cache_bytes: usize) -> Result<Self, Error> {
        Ok(Self {
            frontier: IntervalTopK::new(k)?,
            entries: BTreeMap::new(),
            cutoff: None,
            score_cache: BTreeMap::new(),
            domain_exhausted: false,
            retain_refinement: None,
            luck_skills: None,
            luck_curves: ournotes_sim::live::full::LuckDpCache::new(cache_bytes),
            luck_kinds: None,
            kind_bytes: if cache_bytes == 0 { 0 } else { LOTTERY_LAW_CACHE_BYTES },
            law_bounds: super::leaf::LawBounds::default(),
            census: None,
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

    /// Installs `evaluation`, a certificate of candidate `id`, within every restriction the frontier holds for the
    /// candidate already (another member of its proven program class may have narrowed it), and keeps the entries of
    /// the candidates the frontier still holds. A candidate the frontier no longer holds stays out: returns whether
    /// the candidate took the certificate.
    fn install(&mut self, id: u64, evaluation: &CertifiedEvaluation) -> Result<bool, Error> {
        let Some(current) = self.frontier.get(id) else { return Ok(false) };
        let score = current
            .score
            .intersect(evaluation.score)
            .ok_or_else(|| Error::Domain("conflicting refined score certificates".into()))?;
        let payoff = current
            .payoff
            .intersect(evaluation.payoff)
            .ok_or_else(|| Error::Domain("conflicting refined payoff certificates".into()))?;
        self.frontier.refine(id, current.revision, score, payoff, evaluation.exact_score, evaluation.exact_payoff)?;
        self.entries.retain(|id, _| self.frontier.get(*id).is_some());
        self.cutoff = self.frontier.grid_cutoff(ORDERS as u128);
        Ok(true)
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

/// Whether two contexts play the same live but for their band power.
fn same_live_but_power(a: &expectation::FiniteSeedContext, b: &expectation::FiniteSeedContext) -> bool {
    ournotes_sim::live::full::LiveParams { total_power: b.params.total_power, ..a.params } == b.params
        && a.notes == b.notes
        && a.events == b.events
        && a.play == b.play
        && a.gekisou == b.gekisou
        && a.rank_confirmations == b.rank_confirmations
        && a.delta_times.iter().map(|delta| delta.to_bits()).eq(b.delta_times.iter().map(|delta| delta.to_bits()))
        && a.lottery_free.is_none()
        && b.lottery_free.is_none()
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
        let payoff_identity = format!("{map:?}").into_bytes();
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
        let retained_program = (state.retain_refinement == Some(true)).then(|| program_identity.clone());
        let equality = state.frontier.certify_equal_program(program_identity, power, payoff_identity.clone());
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
        state
            .entries
            .insert(id, CertifiedEntry { physical, members, snaps, power, payoff: payoff_identity, refinement });
        state.entries.retain(|id, _| state.frontier.get(*id).is_some());
        state.cutoff = state.frontier.grid_cutoff(ORDERS as u128);
        self.tel.leaves.peak_retained = self.tel.leaves.peak_retained.max(state.entries.len());
        self.tel.leaves.evaluated += 1;
        self.remember(physical);
        self.report_progress();
        Ok(())
    }

    /// Once the physical domain is exhausted, spend bounded work only on candidates whose ordering still overlaps:
    /// first the complete nominal laws of single orders ([`Self::refine_certified_orders`]), then the equality and
    /// rank bonus certificates of whole candidates ([`Self::refine_certified_candidates`]), then the joint
    /// expectations of single orders ([`Self::refine_joint_orders`]).
    pub(super) fn refine_certified_frontier(&mut self) -> Result<(), Error> {
        self.refine_certified_orders()?;
        self.refine_certified_candidates()?;
        self.refine_joint_orders()
    }

    /// Every installed order is a complete nominal law; declined/partial trees keep their old bounds.
    fn refine_certified_orders(&mut self) -> Result<(), Error> {
        use ournotes_sim::live::full::{LuckExactBudget, LuckExactDecline, LuckExactSession};
        let state = self.certified.as_mut().expect("certified request");
        state.domain_exhausted = true;
        let mut work = LuckExactBudget::default();
        let mut attempted = std::collections::BTreeSet::<(u64, usize)>::new();
        let mut materialized = std::collections::BTreeSet::new();
        loop {
            let state = self.certified.as_ref().expect("certified request");
            let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
            let Some(proof) = pending_refinement(proof, || self.expired() || work.exhausted()) else {
                return Ok(());
            };
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
            let selected = candidates.iter().find_map(|&id| {
                let entry = &state.entries[&id];
                let retained = entry.refinement.as_ref()?;
                let mut indices: Vec<_> = if matches!(retained.map, PayoffMap::Score) {
                    retained
                        .evaluation
                        .orders
                        .iter()
                        .enumerate()
                        .filter_map(|(index, order)| {
                            (order.exact_mean.is_none() && !attempted.contains(&(id, index))).then_some(index)
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
                if !self.materialize_certified_refinement(id)? {
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
                .map(|index| Ok((index, refinement_uncertainty(&orders[index], &map)?)))
                .collect::<Result<Vec<_>, Error>>()?;
            // Uniform-order averaging gives every enclosure width the same aggregate weight.
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
                let performers = order.map(|slot| input.performers[slot].clone());
                self.tel.lottery_refinement.attempted_orders += 1;
                let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                let started = crate::search::budget::now();
                let result = session.law(&performers, &mut work, || self.expired());
                self.tel.lottery_refinement.exact_ms += elapsed_ms(started);
                self.rec.clock.lap(resume);
                let result = result?;
                let telemetry = &mut self.tel.lottery_refinement;
                telemetry.replay_runs += result.stats.replay_runs;
                telemetry.frames += result.stats.frames;
                telemetry.terminal_paths += result.stats.terminal_paths;
                let Some(law) = result.law else {
                    telemetry.declined_orders += 1;
                    telemetry.declines.count(result.decline.expect("a declined attempt gives its reason"));
                    if result.decline == Some(LuckExactDecline::Cancelled) {
                        return Ok(());
                    }
                    if result.decline == Some(LuckExactDecline::Domain) {
                        // Every physical candidate has the same declared notes/frame schedule. Do not
                        // rebuild all live candidates just to rediscover this request-wide refusal.
                        return Ok(());
                    }
                    if result.decline == Some(LuckExactDecline::BranchDepth) {
                        // The chart's lotteries branch along the paths of every formation: once one order nests
                        // more branch points than the provider follows, the request's other orders are left to the
                        // later refinements. This only schedules work; the frontier certifies every rank.
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
                if !state.install(id, &evaluation)? {
                    continue;
                }
                if let Some(entry) = state.entries.get_mut(&id) {
                    entry.refinement.as_mut().expect("admitted candidate").evaluation = evaluation;
                }
                self.tel.lottery_refinement.installed_orders += 1;
                self.report_progress();
            }
        }
    }

    /// Reconstruct the fixed candidate's complete order enclosures from immutable request data.
    fn materialize_certified_refinement(&mut self, id: u64) -> Result<bool, Error> {
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
        let program = canonicalize_performers(&mut input);
        let score = if let Some(cached) = self.cached_certified_score(&program, power) {
            cached
        } else {
            let skills = self.certified_luck_skills()?;
            let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
            let master = self.pool.master;
            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
            let result = crate::search::certified_search::evaluate_luck_context(
                master,
                &skills,
                &input,
                &PayoffMap::Score,
                Some(&mut curves),
                || self.expired(),
            );
            self.rec.clock.lap(resume);
            self.tel.caches.luck_curves.record(curves.stats());
            self.certified.as_mut().expect("certified request").luck_curves = curves;
            let Some(score) = result? else { return Ok(false) };
            self.tel.leaves.simulations += ORDERS as u64;
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
        let evaluation = aggregate_orders(score.orders, &map)?;
        self.certified
            .as_mut()
            .expect("certified request")
            .entries
            .get_mut(&id)
            .expect("boundary candidate")
            .refinement = RetainedRefinement::new(true, evaluation, map, program);
        Ok(true)
    }

    /// The context of `physical` as the certified evaluation plays it, its performers in the canonical order of its
    /// program identity, which comes with it.
    fn certified_context(&self, physical: &PhysicalDeck) -> Result<(expectation::FiniteSeedContext, Vec<u8>), Error> {
        let mut input = expectation::context(self.pool, physical, &self.request.objective)?;
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        let program = canonicalize_performers(&mut input);
        Ok((input, program))
    }

    /// Whole-candidate certificates for the candidates whose ordering still overlaps: equal payoffs of the same
    /// performers at different deck powers ([`Self::join_power_twins`]), then the exact rank bonus expectations of
    /// each candidate ([`Self::refine_rank_residues`]), first for the candidate and competitor of the proof's
    /// refinement request.
    fn refine_certified_candidates(&mut self) -> Result<(), Error> {
        let state = self.certified.as_ref().expect("certified request");
        let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
        if pending_refinement(proof, || self.expired()).is_none() {
            return Ok(());
        }
        self.join_power_twins()?;
        let mut tried = std::collections::BTreeSet::new();
        loop {
            let state = self.certified.as_ref().expect("certified request");
            let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
            let Some(proof) = pending_refinement(proof, || self.expired()) else {
                return Ok(());
            };
            let next = proof
                .refinement
                .iter()
                .flat_map(|request| [Some(request.candidate), request.competitor])
                .flatten()
                .chain(proof.ambiguous.iter().copied())
                .find(|id| !tried.contains(id));
            let Some(id) = next else { return Ok(()) };
            tried.insert(id);
            self.refine_rank_residues(id)?;
        }
    }

    /// Joins the equality classes of overlapping candidates that have the same performers and payoff mapping at
    /// different deck powers when their note scores agree at both powers on every outcome of every performance
    /// order ([`ournotes_sim::live::full::luck_exact_orders_power_twin`]). Their lives differ in the band power alone,
    /// so their scores, and with them their payoffs, agree on every outcome.
    fn join_power_twins(&mut self) -> Result<(), Error> {
        let state = self.certified.as_ref().expect("certified request");
        let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
        let overlapping: Vec<_> = proof
            .ambiguous
            .iter()
            .map(|id| {
                let entry = &state.entries[id];
                (*id, entry.physical, entry.power, entry.payoff.clone())
            })
            .collect();
        let mut candidates = Vec::with_capacity(overlapping.len());
        for (id, physical, power, payoff) in overlapping {
            let (input, program) = self.certified_context(&physical)?;
            candidates.push((id, power, payoff, program, input));
        }
        let skills = self.certified_luck_skills()?;
        let orders = uniform::all_orders();
        for (index, a) in candidates.iter().enumerate() {
            for b in &candidates[index + 1..] {
                if a.1 == b.1 || a.2 != b.2 || a.3 != b.3 || !same_live_but_power(&a.4, &b.4) {
                    continue;
                }
                let state = self.certified.as_ref().expect("certified request");
                if state.frontier.get(a.0).is_none() || state.frontier.get(b.0).is_none() {
                    continue;
                }
                if self.expired() {
                    return Ok(());
                }
                let Some(setup) = a.4.gekisou.as_ref() else { return Ok(()) };
                self.tel.lottery_refinement.power_pairs += 1;
                let mut curves = std::mem::take(&mut self.certified.as_mut().expect("certified request").luck_curves);
                let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
                let started = crate::search::budget::now();
                let equal = ournotes_sim::live::full::luck_exact_orders_power_twin(
                    self.pool.master,
                    &skills,
                    &a.4.performers,
                    &a.4.notes,
                    &a.4.events,
                    a.4.params,
                    setup,
                    &a.4.play,
                    &a.4.delta_times,
                    a.4.rank_confirmations.as_deref(),
                    &orders,
                    None,
                    Some(&mut curves),
                    b.1,
                );
                self.tel.lottery_refinement.twin_ms += elapsed_ms(started);
                self.rec.clock.lap(resume);
                self.tel.caches.luck_curves.record(curves.stats());
                let state = self.certified.as_mut().expect("certified request");
                state.luck_curves = curves;
                match equal {
                    Ok(true) => {
                        state.frontier.certify_equal_candidates(a.0, b.0)?;
                        state.entries.retain(|id, _| state.frontier.get(*id).is_some());
                        state.cutoff = state.frontier.grid_cutoff(ORDERS as u128);
                        self.tel.lottery_refinement.power_twins += 1;
                        self.report_progress();
                    }
                    Ok(false) | Err(Error::Unsupported(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    /// Refines candidate `id` with the rank bonus expectations that the residue laws of its range scores give in
    /// every performance order ([`ournotes_sim::live::full::luck_exact_score_refined`]). A candidate with an order
    /// whose notes may score several integers in one lottery class keeps its enclosure.
    fn refine_rank_residues(&mut self, id: u64) -> Result<(), Error> {
        let state = self.certified.as_ref().expect("certified request");
        let Some(entry) = state.entries.get(&id) else { return Ok(()) };
        let (physical, power) = (entry.physical, entry.power);
        let (input, _) = self.certified_context(&physical)?;
        let Some(setup) = input.gekisou.as_ref() else { return Ok(()) };
        let skills = self.certified_luck_skills()?;
        self.tel.lottery_refinement.residue_candidates += 1;
        let mut orders = Vec::with_capacity(ORDERS);
        for order in uniform::all_orders() {
            if self.expired() {
                return Ok(());
            }
            let performers = order.map(|slot| input.performers[slot].clone());
            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
            let started = crate::search::budget::now();
            let score = ournotes_sim::live::full::luck_exact_score_refined(
                self.pool.master,
                &skills,
                &performers,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
            );
            self.tel.lottery_refinement.residue_ms += elapsed_ms(started);
            self.rec.clock.lap(resume);
            let score = match score {
                Ok(score) if score.wide_notes == 0 => score,
                Ok(_) | Err(Error::Unsupported(_)) => return Ok(()),
                Err(error) => return Err(error),
            };
            let support = (score.final_support.lower, score.final_support.upper);
            let mean = F64Interval::new(score.final_mean.lower, score.final_mean.upper)?
                .intersect(F64Interval::new(support.0 as f64, support.1 as f64)?)
                .ok_or_else(|| Error::Domain("LUCK mean and support disagree".into()))?;
            orders.push(OrderScoreInterval {
                order,
                mean,
                support,
                exact_mean: (support.0 == support.1)
                    .then(|| expectation::ExactExpectation { numerator: support.0.into(), denominator: 1 }),
                final_life: Some((score.final_life.lower, score.final_life.upper)),
                tails: BTreeMap::new(),
                refined_payoff: None,
            });
        }
        let support = (
            orders.iter().map(|order| order.support.0).min().expect("complete order set"),
            orders.iter().map(|order| order.support.1).max().expect("complete order set"),
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
        let evaluation = aggregate_orders(orders, &map)?;
        if !self.certified.as_mut().expect("certified request").install(id, &evaluation)? {
            return Ok(());
        }
        self.tel.lottery_refinement.residue_installed += 1;
        self.report_progress();
        Ok(())
    }

    /// Refines the order enclosures of the candidates whose ordering still overlaps with the joint expectation of
    /// their note scores and lottery classes ([`ournotes_sim::live::full::luck_joint_score`]): the widest order of
    /// any such candidate first, each order once. Every such candidate keeps its order enclosures for this.
    fn refine_joint_orders(&mut self) -> Result<(), Error> {
        let skills = self.certified_luck_skills()?;
        let mut attempted = std::collections::BTreeSet::<(u64, usize)>::new();
        let mut contexts = BTreeMap::<u64, expectation::FiniteSeedContext>::new();
        loop {
            let state = self.certified.as_ref().expect("certified request");
            let proof = state.frontier.proof(RemainingDomain::Exhausted)?;
            let Some(proof) = pending_refinement(proof, || self.expired()) else {
                return Ok(());
            };
            let state = self.certified.as_ref().expect("certified request");
            let unkept = proof
                .ambiguous
                .iter()
                .copied()
                .find(|id| state.entries.get(id).is_some_and(|entry| entry.refinement.is_none()));
            if let Some(id) = unkept {
                if !self.materialize_certified_refinement(id)? {
                    return Ok(());
                }
                continue;
            }
            // The candidate and competitor the proof names come first, then every overlapping candidate.
            let named: Vec<u64> = proof
                .refinement
                .iter()
                .flat_map(|request| [Some(request.candidate), request.competitor])
                .flatten()
                .collect();
            let mut widest = None::<(bool, f64, u64, usize)>;
            for &id in &proof.ambiguous {
                let Some(retained) = state.entries.get(&id).and_then(|entry| entry.refinement.as_ref()) else {
                    continue;
                };
                if !matches!(retained.map, PayoffMap::Score) {
                    continue;
                }
                let first = named.contains(&id);
                for (index, order) in retained.evaluation.orders.iter().enumerate() {
                    if order.exact_mean.is_some() || attempted.contains(&(id, index)) {
                        continue;
                    }
                    let width = refinement_uncertainty(order, &retained.map)?;
                    if widest.is_none_or(|(named, most, _, _)| (first, width) > (named, most)) {
                        widest = Some((first, width, id, index));
                    }
                }
            }
            let Some((_, _, id, index)) = widest else { return Ok(()) };
            attempted.insert((id, index));
            if let std::collections::btree_map::Entry::Vacant(vacant) = contexts.entry(id) {
                let entry = &state.entries[&id];
                let kept = entry.refinement.as_ref().expect("kept above").program.clone();
                let (input, program) = self.certified_context(&entry.physical)?;
                if program != kept {
                    return Err(Error::Domain("certified refinement changed the performer order basis".into()));
                }
                vacant.insert(input);
            }
            let input = &contexts[&id];
            let Some(setup) = input.gekisou.as_ref() else { return Ok(()) };
            let state = self.certified.as_ref().expect("certified request");
            let order = state.entries[&id].refinement.as_ref().expect("kept above").evaluation.orders[index].order;
            let performers = order.map(|slot| input.performers[slot].clone());
            self.tel.lottery_refinement.joint_orders += 1;
            let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
            let started = crate::search::budget::now();
            let score = ournotes_sim::live::full::luck_joint_score(
                self.pool.master,
                &skills,
                &performers,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
                &mut || self.expired(),
            );
            self.tel.lottery_refinement.joint_ms += elapsed_ms(started);
            self.rec.clock.lap(resume);
            let score = match score {
                Ok(Some(score)) => score,
                // The deadline stopped it: the proof stays pending.
                Ok(None) => return Ok(()),
                Err(Error::Unsupported(_) | Error::Capacity(_)) => {
                    self.tel.lottery_refinement.joint_declined += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let joint = F64Interval::new(score.final_mean.lower, score.final_mean.upper)?;
            let state = self.certified.as_mut().expect("certified request");
            let retained = state.entries.get_mut(&id).and_then(|entry| entry.refinement.as_mut()).expect("kept above");
            let row = &mut retained.evaluation.orders[index];
            let mean = row
                .mean
                .intersect(joint)
                .ok_or_else(|| Error::Domain("conflicting joint order certificates".into()))?;
            if mean == row.mean {
                continue;
            }
            row.mean = mean;
            let evaluation = aggregate_orders(retained.evaluation.orders.clone(), &retained.map)?;
            if !state.install(id, &evaluation)? {
                continue;
            }
            if let Some(entry) = state.entries.get_mut(&id) {
                entry.refinement.as_mut().expect("kept above").evaluation = evaluation;
            }
            self.tel.lottery_refinement.joint_narrowed += 1;
            self.report_progress();
        }
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
                    objective_value: None,
                    expected_score: value.exact_score.map(Into::into),
                    expected_payoff: value.exact_payoff.map(Into::into),
                    term_payoffs: None,
                    level_payoffs: None,
                    score_interval: Some(FractionInterval::from_f64(value.score.lower(), value.score.upper())?),
                    payoff_interval: Some(FractionInterval::from_f64(value.payoff.lower(), value.payoff.upper())?),
                    rank_certified: Some(ranked),
                    score_summary: None,
                    best_order: None,
                    order_outcomes: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok((results, proof))
    }
}

impl<'m> Engine<'_, 'm> {
    /// The request's lottery kinds, made at their first use; [`Self::keep_luck_kinds`] puts them back.
    pub(super) fn take_luck_kinds(&mut self) -> ournotes_sim::live::full::LotteryKinds<'m> {
        let state = self.certified.as_mut().expect("certified request");
        let bytes = state.kind_bytes;
        state.luck_kinds.take().unwrap_or_else(|| ournotes_sim::live::full::LotteryKinds::new(self.pool.master, bytes))
    }
    pub(super) fn keep_luck_kinds(&mut self, kinds: ournotes_sim::live::full::LotteryKinds<'m>) {
        self.tel.caches.luck_kinds.record(kinds.stats());
        self.certified.as_mut().expect("certified request").luck_kinds = Some(kinds);
    }
}

#[cfg(test)]
mod refinement_tests {
    use super::*;

    fn frontier(k: usize) -> CertifiedState<'static> {
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
                payoff: Vec::new(),
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
