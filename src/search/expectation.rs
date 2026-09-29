//! Conditional native-root simulation and an exhaustive physical-deck expectation oracle.
//! A finite root law is supplied by the caller, NOT the unknown distribution of TickCount.
//! No sign reduction, order optimization, snap equivalence classes or pruning is used.
//! Exact checked fractions are conditional on the full simulator's supported domain.

use super::{
    Completion, Constraints, Deck, Objective, Pool, SearchRequest, full_setup, objective_song, resolve_allowed,
};
use crate::Error;
use crate::live::full::{GekisouSetup, LiveModel, LiveNote, LiveParams, LivePlay, Performer};
use crate::live::random::{LiveRandom, MEMBER_SHUFFLE};
use crate::master::Master;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

/// Pool indexes in physical slot order; slot 2 is leader. No controllable skill order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PhysicalDeck {
    pub members: [usize; 5],
    pub snaps: [Option<usize>; 5],
}
impl PhysicalDeck {
    pub fn as_deck(&self) -> Deck {
        Deck { members: self.members, snaps: self.snaps, performance_order: [0, 1, 2, 3, 4] }
    }
}

/// Positive integer masses; duplicates and signed roots are retained.
/// Rational probabilities can be represented with common-denominator integer masses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FiniteSeedLaw {
    atoms: Vec<(i32, u64)>,
    total_weight: u128,
}
fn overflow() -> Error {
    Error::Domain("finite-law exact arithmetic overflow (i128 numerator/u128 mass)".into())
}
impl FiniteSeedLaw {
    pub fn new(atoms: Vec<(i32, u64)>) -> Result<Self, Error> {
        if atoms.is_empty() || atoms.iter().any(|a| a.1 == 0) {
            return Err(Error::Input("finite root law requires nonempty positive integer masses".into()));
        }
        let total_weight = atoms.iter().try_fold(0u128, |s, a| s.checked_add(a.1 as u128).ok_or_else(overflow))?;
        Ok(Self { atoms, total_weight })
    }
    pub fn atoms(&self) -> &[(i32, u64)] {
        &self.atoms
    }
    pub fn total_weight(&self) -> u128 {
        self.total_weight
    }
}

/// Exact, not necessarily reduced fraction. Ranking never uses a floating approximation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ExactExpectation {
    pub numerator: i128,
    pub denominator: u128,
}

/// Native descending Fisher–Yates over ALL five slots. Paired snaps follow these indices.
/// Sources: MemberDataContainer 0x55cf050 (loop 0x55cf300..340), DeriveSubSeed 0x55cd778.
/// Returns the complete post-shuffle random state for subsequent Skill/Luck draws.
pub fn native_member_order(root_seed: i32) -> Result<([usize; 5], LiveRandom), Error> {
    let mut random = LiveRandom::new(root_seed);
    let mut order = [0, 1, 2, 3, 4];
    for i in (1..5).rev() {
        let j = random.range(MEMBER_SHUFFLE, (i + 1) as i32)? as usize;
        order.swap(i, j);
    }
    Ok((order, random))
}

/// Performers and their paired support skills in PHYSICAL slot order.
/// Power/leader bonuses must already be calculated on the physical deck.
#[derive(Clone, Debug)]
pub struct FiniteSeedContext {
    physical: PhysicalDeck,
    pub performers: [Performer; 5],
    pub notes: Vec<LiveNote>,
    pub events: Vec<(i32, i32)>,
    pub play: LivePlay,
    pub params: LiveParams,
    pub gekisou: Option<GekisouSetup>,
    pub delta_times: Vec<f32>,
}

/// Full terminal model for event-payoff, mission/rank and life inspection.
#[derive(Clone, Debug)]
pub struct ConditionalOutcome {
    pub root_seed: i32,
    pub performance_order: [usize; 5],
    pub final_score: i32,
    pub model: LiveModel,
}
impl FiniteSeedContext {
    /// Identity captured by context(); payoff calls must use this physical deck.
    pub fn physical(&self) -> PhysicalDeck {
        self.physical
    }
    fn check_physical(&self, physical: &PhysicalDeck) -> Result<(), Error> {
        if self.physical != *physical {
            return Err(Error::Input("physical deck differs from the simulation context".into()));
        }
        Ok(())
    }

    pub fn simulate(&self, master: &Master, root_seed: i32) -> Result<ConditionalOutcome, Error> {
        let (order, random) = native_member_order(root_seed)?;
        self.simulate_order(master, root_seed, order, random)
    }
    fn simulate_order(
        &self,
        master: &Master,
        root_seed: i32,
        order: [usize; 5],
        random: LiveRandom,
    ) -> Result<ConditionalOutcome, Error> {
        let performers = order.map(|slot| self.performers[slot].clone());
        let mut model = match &self.gekisou {
            None => LiveModel::new(master, &performers, &self.notes, &self.events, self.params)?,
            Some(g) => LiveModel::new_gekisou(master, &performers, &self.notes, &self.events, self.params, g)?,
        };
        let final_score = model.run_with_random(&self.play, &self.delta_times, random)?;
        Ok(ConditionalOutcome { root_seed, performance_order: order, final_score, model })
    }
}

// Remove obsolete random inputs before legacy objective validation. The explicit law
// exclusively owns roots, including for Gekisou; this is not a second distribution.
fn normalized_objective(objective: &Objective) -> Objective {
    let mut objective = objective.clone();
    if let Objective::InScenario { objective: inner, .. } = &mut objective {
        **inner = normalized_objective(inner);
    }
    if let Objective::LiveScore { play, gekisou, .. } = &mut objective {
        if let super::PlayInput::Stream { stream, .. } = play {
            stream.base_seed = 0;
        }
        if let Some(g) = gekisou {
            g.seeds = super::SeedSet::List(vec![0]);
        }
    }
    objective
}

/// Adapt a whole-live objective. Its legacy Gekisou seed list is NOT the root law here;
/// the explicit root/law argument is authoritative.
pub fn context(pool: &Pool, physical: &PhysicalDeck, objective: &Objective) -> Result<FiniteSeedContext, Error> {
    let objective = normalized_objective(objective);
    let objective = &objective;
    let deck = physical.as_deck();
    pool.check_deck(&deck)?;
    let (song, event, _) = objective_song(pool, objective)?;
    let setup = full_setup(pool, objective)?.ok_or_else(|| {
        Error::Input("native expectation requires a whole-live Stream objective with snap skills enabled".into())
    })?;
    let power = pool.deck_power(&deck, song.as_ref(), event)?.power();
    let performers = super::snaps::deck_performers(pool, &deck)?
        .try_into()
        .map_err(|_| Error::Input("exactly five physical performers required".into()))?;
    let (gekisou, delta_times) = match setup.gk {
        Some(g) => (Some(g.setup), g.dt),
        None => (None, vec![0.0; setup.play.frames.len()]),
    };
    Ok(FiniteSeedContext {
        physical: *physical,
        performers,
        notes: setup.notes,
        events: setup.events,
        play: setup.play,
        params: LiveParams { total_power: power, ..setup.params },
        gekisou,
        delta_times,
    })
}
pub fn evaluate_seed(
    pool: &Pool,
    physical: &PhysicalDeck,
    objective: &Objective,
    root_seed: i32,
) -> Result<ConditionalOutcome, Error> {
    context(pool, physical, objective)?.simulate(pool.master, root_seed)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SeedOutcome {
    pub root_seed: i32,
    pub weight: u64,
    pub performance_order: [usize; 5],
    pub final_score: i32,
    pub terminal_payoff: i128,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FiniteEvaluation {
    pub expected_score: ExactExpectation,
    pub expected_payoff: ExactExpectation,
    /// Unnormalized masses; divide by the expectation denominator.
    pub score_mass: BTreeMap<i32, u128>,
    pub payoff_mass: BTreeMap<i128, u128>,
    /// Joint score/payoff correlation and input order, including duplicate roots.
    pub outcomes: Vec<SeedOutcome>,
}

/// Primary payoff entrypoint: builds mutable state separately for EACH law atom.
/// `fresh_state` MUST return state whose mutable storage is independent of every other
/// invocation and of caller-owned initial state. Neither callback may use captured shared
/// mutable storage to accumulate gameplay/payoff across atoms. Rust cannot enforce these
/// semantic requirements: a factory returning the same Rc<Cell<_>> violates this contract.
/// Factory failures and payoff failures propagate; no partial expectation is returned.
pub fn evaluate_finite_with_factory<C, S, F>(
    pool: &Pool,
    physical: &PhysicalDeck,
    objective: &Objective,
    law: &FiniteSeedLaw,
    fresh_state: S,
    terminal_payoff: F,
) -> Result<FiniteEvaluation, Error>
where
    S: FnMut() -> Result<C, Error>,
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    let input = context(pool, physical, objective)?;
    evaluate_context_with_factory(pool.master, physical, &input, law, fresh_state, terminal_payoff)
}

/// Low-level factory entrypoint; rejects a PhysicalDeck different from the identity
/// captured when `context` was resolved. Fields remain explicit simulation inputs:
/// callers editing performers must keep member/snap pairing consistent with that identity.
/// The fresh-state and no-cross-atom-capture contract is [`evaluate_finite_with_factory`]'s.
pub fn evaluate_context_with_factory<C, S, F>(
    master: &Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    law: &FiniteSeedLaw,
    mut fresh_state: S,
    mut terminal_payoff: F,
) -> Result<FiniteEvaluation, Error>
where
    S: FnMut() -> Result<C, Error>,
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    input.check_physical(physical)?;
    let mut outcomes = Vec::with_capacity(law.atoms.len());
    for &(root_seed, weight) in &law.atoms {
        let mut local = fresh_state()?;
        let outcome = input.simulate(master, root_seed)?;
        let payoff = terminal_payoff(physical, &outcome, &mut local)?;
        outcomes.push(SeedOutcome {
            root_seed,
            weight,
            performance_order: outcome.performance_order,
            final_score: outcome.final_score,
            terminal_payoff: payoff,
        });
    }
    aggregate(outcomes)
}

/// Clone convenience wrapper. PRECONDITION: initial.clone() must deeply isolate ALL
/// mutable storage touched by payoff, from initial and from every other clone. C: Clone
/// alone does NOT guarantee this (Rc<Cell<_>> and Arc<Mutex<_>> usually violate it).
/// Prefer [`evaluate_finite_with_factory`]; captured gameplay state must not accumulate.
pub fn evaluate_finite<C: Clone, F>(
    pool: &Pool,
    physical: &PhysicalDeck,
    objective: &Objective,
    law: &FiniteSeedLaw,
    initial: &C,
    terminal_payoff: F,
) -> Result<FiniteEvaluation, Error>
where
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    evaluate_finite_with_factory(pool, physical, objective, law, || Ok(initial.clone()), terminal_payoff)
}

/// Clone convenience wrapper with the deep-isolation precondition of [`evaluate_finite`].
/// Arbitrary Clone implementations are not automatically isolated.
pub fn evaluate_context<C: Clone, F>(
    master: &Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    law: &FiniteSeedLaw,
    initial: &C,
    terminal_payoff: F,
) -> Result<FiniteEvaluation, Error>
where
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    evaluate_context_with_factory(master, physical, input, law, || Ok(initial.clone()), terminal_payoff)
}

/// Also accepts externally/native-evaluated atoms. All additions/multiplications are
/// checked. Overflow is an error, not saturation, midpoint approximation or f64 fallback.
pub fn aggregate(outcomes: Vec<SeedOutcome>) -> Result<FiniteEvaluation, Error> {
    if outcomes.is_empty() || outcomes.iter().any(|o| o.weight == 0) {
        return Err(Error::Input("nonempty positive-mass outcomes required".into()));
    }
    let mut score_mass = BTreeMap::<i32, u128>::new();
    let mut payoff_mass = BTreeMap::<i128, u128>::new();
    let (mut score, mut payoff, mut denominator) = (0i128, 0i128, 0u128);
    for o in &outcomes {
        let w = o.weight as i128;
        score = score.checked_add((o.final_score as i128).checked_mul(w).ok_or_else(overflow)?).ok_or_else(overflow)?;
        payoff = payoff.checked_add(o.terminal_payoff.checked_mul(w).ok_or_else(overflow)?).ok_or_else(overflow)?;
        denominator = denominator.checked_add(o.weight as u128).ok_or_else(overflow)?;
        for entry in [score_mass.entry(o.final_score).or_default(), payoff_mass.entry(o.terminal_payoff).or_default()] {
            *entry = entry.checked_add(o.weight as u128).ok_or_else(overflow)?;
        }
    }
    Ok(FiniteEvaluation {
        expected_score: ExactExpectation { numerator: score, denominator },
        expected_payoff: ExactExpectation { numerator: payoff, denominator },
        score_mass,
        payoff_mass,
        outcomes,
    })
}

/// Every legal PHYSICAL ordered five-character deck and every snap injection (including
/// empty slots). Arbitrary finite root laws do not allow canonical nonleader reduction.
/// Callback false stops enumeration. Returns number of visited decks.
pub fn visit_physical_decks<F>(pool: &Pool, constraints: &Constraints, mut visit: F) -> Result<u64, Error>
where
    F: FnMut(PhysicalDeck) -> Result<bool, Error>,
{
    let (allowed, snaps) = resolve_allowed(pool, constraints)?;
    let candidates: Vec<usize> = (0..pool.members.len()).filter(|&m| allowed.members[m]).collect();
    let mut count = 0u64;
    fn snap_rec<F>(
        slot: usize,
        deck: &mut PhysicalDeck,
        snaps: &[usize],
        count: &mut u64,
        visit: &mut F,
    ) -> Result<bool, Error>
    where
        F: FnMut(PhysicalDeck) -> Result<bool, Error>,
    {
        if slot == 5 {
            *count = count.checked_add(1).ok_or_else(overflow)?;
            return visit(*deck);
        }
        deck.snaps[slot] = None;
        if !snap_rec(slot + 1, deck, snaps, count, visit)? {
            return Ok(false);
        }
        for &s in snaps {
            if deck.snaps[..slot].contains(&Some(s)) {
                continue;
            }
            deck.snaps[slot] = Some(s);
            if !snap_rec(slot + 1, deck, snaps, count, visit)? {
                return Ok(false);
            }
        }
        deck.snaps[slot] = None;
        Ok(true)
    }
    struct Members<'a, 'm> {
        pool: &'a Pool<'m>,
        candidates: &'a [usize],
        required: &'a [usize],
        leader: Option<usize>,
    }
    fn member_rec<F>(
        slot: usize,
        deck: &mut PhysicalDeck,
        m: &Members<'_, '_>,
        snaps: &[usize],
        count: &mut u64,
        visit: &mut F,
    ) -> Result<bool, Error>
    where
        F: FnMut(PhysicalDeck) -> Result<bool, Error>,
    {
        if slot == 5 {
            if !m.required.iter().all(|r| deck.members.contains(r)) {
                return Ok(true);
            }
            return snap_rec(0, deck, snaps, count, visit);
        }
        for &candidate in m.candidates {
            if slot == 2 && m.leader.is_some_and(|l| l != candidate) {
                continue;
            }
            if deck.members[..slot]
                .iter()
                .any(|&i| m.pool.members[i].character_id == m.pool.members[candidate].character_id)
            {
                continue;
            }
            deck.members[slot] = candidate;
            if !member_rec(slot + 1, deck, m, snaps, count, visit)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    let mut deck = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    let m = Members { pool, candidates: &candidates, required: &allowed.required, leader: allowed.leader };
    member_rec(0, &mut deck, &m, &snaps, &mut count, &mut visit)?;
    Ok(count)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RankedPhysicalDeck {
    pub physical: PhysicalDeck,
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
    pub power: i32,
    pub evaluation: FiniteEvaluation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OracleOutcome {
    pub completion: Completion,
    pub evaluated: u64,
    pub law: FiniteSeedLaw,
    /// Top k PHYSICAL decks, not one representative per member set.
    pub results: Vec<RankedPhysicalDeck>,
}

/// Correctness-first default live objective: E(final score) under the caller's law.
pub fn oracle(pool: &Pool, request: &SearchRequest, law: &FiniteSeedLaw) -> Result<OracleOutcome, Error> {
    oracle_with_payoff_factory(pool, request, law, || Ok(()), |_, o, _| Ok(o.final_score as i128))
}
/// Clone convenience wrapper with [`evaluate_finite`]'s deep-isolation precondition.
/// Prefer [`oracle_with_payoff_factory`] for mutable terminal-payoff state.
pub fn oracle_with_payoff<C: Clone, F>(
    pool: &Pool,
    request: &SearchRequest,
    law: &FiniteSeedLaw,
    initial: &C,
    terminal_payoff: F,
) -> Result<OracleOutcome, Error>
where
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    oracle_with_payoff_factory(pool, request, law, || Ok(initial.clone()), terminal_payoff)
}

/// Exhaustive terminal-payoff optimizer. Ranking uses numerators under one common law,
/// then power descending, physical member/snap IDs ascending. No best-performance-order max.
/// Fresh state is built for each (deck, atom); see [`evaluate_finite_with_factory`].
pub fn oracle_with_payoff_factory<C, S, F>(
    pool: &Pool,
    request: &SearchRequest,
    law: &FiniteSeedLaw,
    mut fresh_state: S,
    mut terminal_payoff: F,
) -> Result<OracleOutcome, Error>
where
    S: FnMut() -> Result<C, Error>,
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    let normalized = normalized_objective(&request.objective);
    objective_song(pool, &normalized)?;
    if full_setup(pool, &normalized)?.is_none() {
        return Err(Error::Input("native expectation needs whole-live Stream objective".into()));
    }
    let start = Instant::now();
    let mut completion = Completion::Complete;
    let mut evaluated = 0u64;
    let mut results = Vec::new();
    visit_physical_decks(pool, &request.constraints, |physical| {
        if request.time_limit.is_some_and(|limit| start.elapsed() >= limit) {
            completion = Completion::TimedOut;
            return Ok(false);
        }
        let input = context(pool, &physical, &request.objective)?;
        let evaluation =
            evaluate_context_with_factory(pool.master, &physical, &input, law, &mut fresh_state, &mut terminal_payoff)?;
        evaluated = evaluated.checked_add(1).ok_or_else(overflow)?;
        results.push(RankedPhysicalDeck {
            physical,
            members: physical.members.map(|i| pool.members[i].id),
            snaps: physical.snaps.map(|s| s.map(|i| pool.snaps[i].id)),
            power: input.params.total_power,
            evaluation,
        });
        Ok(true)
    })?;
    results.sort_by(|a, b| {
        b.evaluation
            .expected_payoff
            .numerator
            .cmp(&a.evaluation.expected_payoff.numerator)
            .then_with(|| b.power.cmp(&a.power))
            .then_with(|| a.members.cmp(&b.members))
            .then_with(|| a.snaps.cmp(&b.snaps))
    });
    results.truncate(request.k);
    Ok(OracleOutcome { completion, evaluated, law: law.clone(), results })
}

/// Explicitly NON-NATIVE probability model. Only the member order is independent and
/// uniform over 5! permutations. Skill/Luck retain their common lottery-root correlation;
/// this neither claims native TickCount nor independent uniform lottery outcomes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndependentUniformOrderAssumption {
    pub evaluation: FiniteEvaluation,
}

/// Clone convenience wrapper with [`evaluate_finite`]'s deep-isolation precondition.
pub fn evaluate_independent_uniform_order_assumption<C: Clone, F>(
    master: &Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    lottery_law: &FiniteSeedLaw,
    initial: &C,
    terminal_payoff: F,
) -> Result<IndependentUniformOrderAssumption, Error>
where
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    evaluate_independent_uniform_order_assumption_with_factory(
        master,
        physical,
        input,
        lottery_law,
        || Ok(initial.clone()),
        terminal_payoff,
    )
}

/// A separately named assumption model for comparison, never used by the native oracle.
/// Each (uniform order, lottery-law atom) gets a fresh model and factory-created state.
/// The factory/callback contract is [`evaluate_finite_with_factory`].
pub fn evaluate_independent_uniform_order_assumption_with_factory<C, S, F>(
    master: &Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    lottery_law: &FiniteSeedLaw,
    mut fresh_state: S,
    mut terminal_payoff: F,
) -> Result<IndependentUniformOrderAssumption, Error>
where
    S: FnMut() -> Result<C, Error>,
    F: FnMut(&PhysicalDeck, &ConditionalOutcome, &mut C) -> Result<i128, Error>,
{
    input.check_physical(physical)?;
    let mut outcomes = Vec::new();
    let mut order = [0, 1, 2, 3, 4];
    loop {
        for &(root_seed, weight) in lottery_law.atoms() {
            let outcome = input.simulate_order(master, root_seed, order, LiveRandom::new(root_seed))?;
            let mut local = fresh_state()?;
            let payoff = terminal_payoff(physical, &outcome, &mut local)?;
            outcomes.push(SeedOutcome {
                root_seed,
                weight,
                performance_order: order,
                final_score: outcome.final_score,
                terminal_payoff: payoff,
            });
        }
        if !super::live::next_permutation(&mut order) {
            break;
        }
    }
    Ok(IndependentUniformOrderAssumption { evaluation: aggregate(outcomes)? })
}
