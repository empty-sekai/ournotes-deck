//! Production boundary: deterministic power/skip search and bounded finite-law native
//! physical-deck optimization. Candidate proposals are heuristic; every returned value is
//! evaluated exactly under the declared model. Only exhaustion/proven pruning certifies K.

use super::expectation::{
    self, ExactExpectation, FiniteEvaluation, FiniteSeedContext, FiniteSeedLaw, PhysicalDeck, SeedOutcome,
};
use super::{Completion, Constraints, GekisouObjective, Objective, PlayInput, Pool, SearchRequest, SeedSet};
use crate::live::model::{JudgementStream, JustRule};
use crate::live::skip::is_judgement_note;
use crate::replay::RankConfirmation;
use crate::scenario::{ContextInput, EventPayoffInput, PowerSnapshotInput, Scenario};
use crate::{Error, cards::Roster, data::DeckData};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

pub const REQUEST_FORMAT: &str = "ournotes-deck.recommendation-request/1";
pub const RESULT_FORMAT: &str = "ournotes-deck.recommendation-result/1";
pub const MAX_K: usize = 100;
pub const MAX_ATOMS: usize = 4096;
const MAX_RESULT_ATOMS: usize = 65_536;
fn five() -> usize {
    5
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecommendationRequest {
    pub format: String,
    pub execution: Execution,
    pub scenario: Option<Scene>,
    pub context: Option<ContextInput>,
    pub metric: Metric,
    pub seed_law: Option<SeedLawInput>,
    #[serde(default)]
    pub constraints: Constraints,
    #[serde(default = "five")]
    pub k: usize,
    #[serde(default)]
    pub strategy: Strategy,
    #[serde(default)]
    pub limits: Limits,
    /// Conditional input; never a prediction of opponents' native ranks.
    pub network_confirmations: Option<Vec<RankConfirmation>>,
    #[serde(default)]
    pub simulation: SimulationInput,
}

#[derive(Clone, Debug)]
pub enum Execution {
    Power { music_id: Option<i64>, event_parameter: bool },
    Skip { score_id: i64 },
    Live { score_id: i64, gekisou: bool, play: PlayPolicy },
}
#[derive(Clone, Debug)]
pub enum PlayPolicy {
    TheoreticalBest,
    Stream { stream: JudgementStream },
}

// Internally tagged enums buffer their contents before dispatch. With
// serde_json/arbitrary_precision, a decimal is buffered as a private number map,
// which cannot deserialize as f32. Read the typed fields directly instead. This
// also preserves duplicate-field rejection and rejects objects posing as numbers.
#[derive(Default)]
enum RequestField<T> {
    #[default]
    Absent,
    Present(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for RequestField<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(Self::Present)
    }
}
impl<T> RequestField<T> {
    fn required<E: serde::de::Error>(self, name: &'static str) -> Result<T, E> {
        match self {
            Self::Absent => Err(E::missing_field(name)),
            Self::Present(value) => Ok(value),
        }
    }
    fn or_default(self) -> T
    where
        T: Default,
    {
        match self {
            Self::Absent => T::default(),
            Self::Present(value) => value,
        }
    }
    fn reject<E: serde::de::Error>(self, name: &'static str, fields: &'static [&'static str]) -> Result<(), E> {
        match self {
            Self::Absent => Ok(()),
            Self::Present(_) => Err(E::unknown_field(name, fields)),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionWire {
    kind: String,
    #[serde(default)]
    music_id: RequestField<Option<i64>>,
    #[serde(default)]
    event_parameter: RequestField<bool>,
    #[serde(default)]
    score_id: RequestField<i64>,
    #[serde(default)]
    gekisou: RequestField<bool>,
    #[serde(default)]
    play: RequestField<PlayPolicy>,
}
impl<'de> Deserialize<'de> for Execution {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = ExecutionWire::deserialize(d)?;
        match w.kind.as_str() {
            "power" => {
                let fields = &["kind", "musicId", "eventParameter"];
                w.score_id.reject("scoreId", fields)?;
                w.gekisou.reject("gekisou", fields)?;
                w.play.reject("play", fields)?;
                Ok(Self::Power { music_id: w.music_id.or_default(), event_parameter: w.event_parameter.or_default() })
            }
            "skip" => {
                let fields = &["kind", "scoreId"];
                w.music_id.reject("musicId", fields)?;
                w.event_parameter.reject("eventParameter", fields)?;
                w.gekisou.reject("gekisou", fields)?;
                w.play.reject("play", fields)?;
                Ok(Self::Skip { score_id: w.score_id.required("scoreId")? })
            }
            "live" => {
                let fields = &["kind", "scoreId", "gekisou", "play"];
                w.music_id.reject("musicId", fields)?;
                w.event_parameter.reject("eventParameter", fields)?;
                Ok(Self::Live {
                    score_id: w.score_id.required("scoreId")?,
                    gekisou: w.gekisou.required("gekisou")?,
                    play: w.play.required("play")?,
                })
            }
            other => Err(serde::de::Error::unknown_variant(other, &["power", "skip", "live"])),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlayPolicyWire {
    kind: String,
    #[serde(default)]
    stream: RequestField<JudgementStream>,
}
impl<'de> Deserialize<'de> for PlayPolicy {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = PlayPolicyWire::deserialize(d)?;
        match w.kind.as_str() {
            "theoreticalBest" => {
                w.stream.reject("stream", &["kind"])?;
                Ok(Self::TheoreticalBest)
            }
            "stream" => Ok(Self::Stream { stream: w.stream.required("stream")? }),
            other => Err(serde::de::Error::unknown_variant(other, &["theoreticalBest", "stream"])),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Scene {
    Free {
        #[serde(rename = "musicId")]
        music_id: i64,
    },
    Mission {
        #[serde(rename = "musicId")]
        music_id: i64,
    },
    Battle {
        #[serde(rename = "musicId")]
        music_id: i64,
    },
    Arena {
        #[serde(rename = "musicId")]
        music_id: i64,
    },
    Challenge {
        #[serde(rename = "musicId")]
        music_id: i64,
    },
}
impl Scene {
    fn scenario(&self) -> Scenario {
        match *self {
            Self::Free { music_id } => Scenario::Free(music_id),
            Self::Mission { music_id } => Scenario::Mission(music_id),
            Self::Battle { music_id } => Scenario::Battle(music_id),
            Self::Arena { music_id } => Scenario::Arena(music_id),
            Self::Challenge { music_id } => Scenario::Challenge(music_id),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Metric {
    Power,
    Score,
    ScoreAtLeast {
        threshold: i32,
    },
    ClientEventPoints {
        #[serde(rename = "eventId")]
        event_id: i64,
    },
    ConditionalClientEventItems {
        #[serde(rename = "eventId")]
        event_id: i64,
        #[serde(rename = "resourceType")]
        resource_type: i64,
        #[serde(rename = "resourceId")]
        resource_id: i64,
    },
}
impl Metric {
    fn event(&self) -> Option<i64> {
        match *self {
            Self::ClientEventPoints { event_id } | Self::ConditionalClientEventItems { event_id, .. } => Some(event_id),
            _ => None,
        }
    }
    fn upper(&self) -> Option<i128> {
        match self {
            Self::Score | Self::ClientEventPoints { .. } => Some(i32::MAX as i128),
            Self::ScoreAtLeast { .. } => Some(1),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedLawInput {
    pub atoms: Vec<(i32, u64)>,
    pub provenance: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Strategy {
    #[default]
    Exhaustive,
    Candidate {
        #[serde(rename = "powerSeeds")]
        power_seeds: usize,
        proposals: u64,
        #[serde(rename = "proposalSeed")]
        proposal_seed: u64,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Limits {
    pub time_limit_ms: Option<u64>,
    pub max_candidates: Option<u64>,
    pub cache_entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { time_limit_ms: Some(3000), max_candidates: Some(1000), cache_entries: 2048 }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SimulationInput {
    pub music_length_ms: Option<i32>,
    pub score_music_length_ms: Option<i32>,
    pub live_finished_from_frame: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Optimality {
    Proven,
    Unproven,
    Heuristic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExitReason {
    Exhausted,
    TimeLimit,
    CandidateLimit,
    ProposalLimit,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fraction {
    pub numerator: String,
    pub denominator: String,
}
impl From<ExactExpectation> for Fraction {
    fn from(v: ExactExpectation) -> Self {
        Self { numerator: v.numerator.to_string(), denominator: v.denominator.to_string() }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomResult {
    pub root_seed: i32,
    pub weight: String,
    pub performance_order: [usize; 5],
    pub score: i32,
    pub payoff: String,
    pub network_applications: Vec<(usize, usize)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedDeck {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
    pub power: i32,
    pub expected_score: Option<Fraction>,
    pub expected_payoff: Fraction,
    pub atoms: Vec<AtomResult>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub nodes: u64,
    pub proposed: u64,
    pub visited_candidates: u64,
    pub evaluated: u64,
    pub simulations: u64,
    pub cache_hits: u64,
    pub cache_evictions: u64,
    pub duplicate_atom_hits: u64,
    pub bound_pruned: u64,
    pub partial_candidates: u64,
    pub peak_cache_entries: usize,
    pub peak_retained_decks: usize,
    pub warmup_ms: f64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationOutcome {
    pub format: &'static str,
    pub completion: Completion,
    pub optimality: Optimality,
    pub exit_reason: ExitReason,
    pub result_identity: &'static str,
    pub metric: Metric,
    pub strategy: Strategy,
    pub probability_law: serde_json::Value,
    pub proof_scope: &'static str,
    pub resolved_context: serde_json::Value,
    pub results: Vec<RecommendedDeck>,
    pub stats: Stats,
    pub elapsed_ms: f64,
}

#[derive(Clone)]
struct Entry {
    physical: PhysicalDeck,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    evaluation: FiniteEvaluation,
    network_applications: BTreeMap<i32, Vec<(usize, usize)>>,
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
    fn wire(self) -> RecommendedDeck {
        let applications = self.network_applications;
        RecommendedDeck {
            members: self.members,
            snaps: self.snaps,
            power: self.power,
            expected_score: Some(self.evaluation.expected_score.into()),
            expected_payoff: self.evaluation.expected_payoff.into(),
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
                })
                .collect(),
        }
    }
}

struct Engine<'a, 'm> {
    pool: &'a Pool<'m>,
    request: &'a SearchRequest,
    law: &'a FiniteSeedLaw,
    metric: &'a Metric,
    event_input: Option<&'a EventPayoffInput>,
    network: Option<&'a [RankConfirmation]>,
    simulation: &'a SimulationInput,
    limits: &'a Limits,
    start: Instant,
    stop: Option<ExitReason>,
    stats: Stats,
    top: Vec<Entry>,
    seen: HashSet<PhysicalDeck>,
    fifo: VecDeque<PhysicalDeck>,
    input: Option<FiniteSeedContext>,
    song: Option<crate::cards::SongView>,
    event: bool,
    skip: Option<super::SkipModel<'a>>,
}
impl Engine<'_, '_> {
    fn expired(&mut self) -> bool {
        if self.stop.is_some() {
            return true;
        }
        if self.limits.time_limit_ms.is_some_and(|ms| self.start.elapsed() >= Duration::from_millis(ms)) {
            self.stop = Some(ExitReason::TimeLimit);
            true
        } else {
            false
        }
    }
    fn remember(&mut self, p: PhysicalDeck) {
        if self.limits.cache_entries == 0 {
            return;
        }
        if self.seen.len() >= self.limits.cache_entries
            && let Some(old) = self.fifo.pop_front()
        {
            self.seen.remove(&old);
            self.stats.cache_evictions += 1;
        }
        self.seen.insert(p);
        self.fifo.push_back(p);
        self.stats.peak_cache_entries = self.stats.peak_cache_entries.max(self.seen.len());
    }
    fn payoff(&self, p: &PhysicalDeck, score: i32, power: i32) -> Result<i128, Error> {
        match *self.metric {
            Metric::Power => Ok(power as i128),
            Metric::Score => Ok(score as i128),
            Metric::ScoreAtLeast { threshold } => Ok(i128::from(score >= threshold)),
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
                crate::scenario::item_payoff(&items, event_id, resource_type, resource_id)
            }
        }
    }
    fn consider(&mut self, physical: PhysicalDeck) -> Result<bool, Error> {
        self.stats.proposed += 1;
        if self.expired() {
            return Ok(false);
        }
        if self.seen.contains(&physical) || self.top.iter().any(|e| e.physical == physical) {
            self.stats.cache_hits += 1;
            return Ok(true);
        }
        if self.limits.max_candidates.is_some_and(|n| self.stats.visited_candidates >= n) {
            self.stop = Some(ExitReason::CandidateLimit);
            return Ok(false);
        }
        self.stats.visited_candidates += 1;
        let power = self.pool.deck_power(&physical.as_deck(), self.song.as_ref(), self.event)?.power();
        let mut applications = BTreeMap::new();
        let evaluation = if matches!(self.request.objective.inner(), Objective::LiveScore { .. }) {
            if let Some(input) = &mut self.input {
                input.rebind(self.pool, physical, power)?;
            } else {
                self.input = Some(expectation::context(self.pool, &physical, &self.request.objective)?);
            }
            let input = self.input.as_mut().expect("context");
            if let Some(v) = self.simulation.music_length_ms {
                input.params.music_length_ms = v;
            }
            if let Some(v) = self.simulation.score_music_length_ms {
                input.params.score_music_length_ms = Some(v);
            }
            let mut outcomes = Vec::with_capacity(self.law.atoms().len());
            let mut duplicates = BTreeMap::<i32, SeedOutcome>::new();
            let (mut partial, mut consumed) = (0i128, 0u128);
            for &(root, weight) in self.law.atoms() {
                if self.expired() {
                    self.stats.partial_candidates += 1;
                    return Ok(false);
                }
                let atom = if let Some(atom) = duplicates.get(&root) {
                    self.stats.duplicate_atom_hits += 1;
                    let mut a = atom.clone();
                    a.weight = weight;
                    a
                } else {
                    let start = self.start;
                    let ms = self.limits.time_limit_ms;
                    let terminal = self.input.as_ref().expect("context").simulate_declared(
                        self.pool.master,
                        root,
                        self.network,
                        self.simulation.live_finished_from_frame,
                        || ms.is_some_and(|m| start.elapsed() >= Duration::from_millis(m)),
                    )?;
                    let Some(terminal) = terminal else {
                        self.stop = Some(ExitReason::TimeLimit);
                        self.stats.partial_candidates += 1;
                        return Ok(false);
                    };
                    applications.insert(root, terminal.network_applications);
                    let terminal = terminal.terminal;
                    self.stats.simulations += 1;
                    let atom = SeedOutcome {
                        root_seed: root,
                        weight,
                        performance_order: terminal.performance_order,
                        final_score: terminal.final_score,
                        terminal_payoff: self.payoff(&physical, terminal.final_score, power)?,
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
                        self.stats.bound_pruned += 1;
                        self.remember(physical);
                        return Ok(true);
                    }
                }
            }
            expectation::aggregate(outcomes)?
        } else {
            let score = match &self.skip {
                Some(skip) => skip.score(power)?,
                None => power,
            };
            let payoff = self.payoff(&physical, score, power)?;
            expectation::aggregate(vec![SeedOutcome {
                root_seed: 0,
                weight: 1,
                performance_order: [0, 1, 2, 3, 4],
                final_score: score,
                terminal_payoff: payoff,
            }])?
        };
        self.stats.evaluated += 1;
        self.remember(physical);
        let entry = Entry {
            physical,
            members: physical.members.map(|i| self.pool.members[i].id),
            snaps: physical.snaps.map(|i| i.map(|i| self.pool.snaps[i].id)),
            power,
            evaluation,
            network_applications: applications,
        };
        let pos = self.top.iter().position(|e| compare(&entry, e) == Ordering::Less).unwrap_or(self.top.len());
        if pos < self.request.k {
            self.top.insert(pos, entry);
            self.top.truncate(self.request.k);
        }
        self.stats.peak_retained_decks = self.stats.peak_retained_decks.max(self.top.len());
        Ok(true)
    }
}
fn arithmetic() -> Error {
    Error::Domain("finite-law checked exact arithmetic overflow".into())
}

/// Deterministic evaluator used by the production solver, exposed for independent native
/// harnesses. Context identity must match; rank inputs denote aggregate packet ARRIVAL,
/// not application. Actual confirmations are scheduled once per eligible frame/range order.
/// It uses no wall clock and is suitable for numeric cross-architecture checks.
pub fn evaluate_declared_context(
    master: &crate::master::Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    root_seed: i32,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
) -> Result<(expectation::ConditionalOutcome, Vec<(usize, usize)>), Error> {
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
        let factors = crate::live::full::gekisou_rank_factors(master, &missions)?;
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
    let outcome = input
        .simulate_declared(master, root_seed, network, simulation.live_finished_from_frame, || false)?
        .ok_or_else(|| Error::Domain("unbounded declared evaluator cancelled unexpectedly".into()))?;
    Ok((outcome.terminal, outcome.network_applications))
}

fn validate(k: usize, law: &FiniteSeedLaw, limits: &Limits, strategy: &Strategy) -> Result<(), Error> {
    if !(1..=MAX_K).contains(&k) {
        return Err(Error::Input(format!("k must be in 1..={MAX_K}")));
    }
    if law.atoms().len() > MAX_ATOMS || k.saturating_mul(law.atoms().len()) > MAX_RESULT_ATOMS {
        return Err(Error::Capacity("law/results exceed bounded atom capacity".into()));
    }
    if limits.cache_entries > 100_000 {
        return Err(Error::Capacity("cacheEntries exceeds 100000".into()));
    }
    if let Strategy::Candidate { power_seeds, proposals, .. } = strategy
        && (*power_seeds > 128 || *proposals > 10_000_000)
    {
        return Err(Error::Capacity("candidate powerSeeds<=128 and proposals<=10000000 required".into()));
    }
    Ok(())
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
    let start = Instant::now();
    let mut limits = limits.clone();
    if let Some(d) = request.time_limit {
        let ms = d.as_millis().min(u64::MAX as u128) as u64;
        limits.time_limit_ms = Some(limits.time_limit_ms.map_or(ms, |m| m.min(ms)));
    }
    validate(request.k, law, &limits, strategy)?;
    let mut normalized = request.clone();
    normalized.objective = expectation::normalized_objective(&request.objective);
    let request = &normalized;
    let (song, event, skip) = super::objective_song(pool, &request.objective)?;
    let (allowed, snaps) = super::resolve_allowed(pool, &request.constraints)?;
    validate_payoff(pool, request, metric, event_input)?;
    if matches!(request.objective.inner(), Objective::LiveScore { .. }) {
        validate_play(pool, request, network, simulation)?;
    } else if network.is_some()
        || simulation.music_length_ms.is_some()
        || simulation.score_music_length_ms.is_some()
        || simulation.live_finished_from_frame.is_some()
    {
        return Err(Error::Input("simulation/network inputs apply only to played live".into()));
    }
    let mut candidates: Vec<usize> = (0..pool.members.len()).filter(|&i| allowed.members[i]).collect();
    candidates.sort_by_key(|&i| pool.members[i].id);
    let mut required = allowed.required.clone();
    if let Some(l) = allowed.leader
        && !required.contains(&l)
    {
        required.push(l)
    }
    let distinct: HashSet<_> = required.iter().map(|&i| pool.members[i].character_id).collect();
    let feasible = required.len() <= 5
        && distinct.len() == required.len()
        && candidates.iter().map(|&i| pool.members[i].character_id).collect::<HashSet<_>>().len() >= 5;
    let mut engine = Engine {
        pool,
        request,
        law,
        metric,
        event_input,
        network,
        simulation,
        limits: &limits,
        start,
        stop: None,
        stats: Stats::default(),
        top: Vec::new(),
        seen: HashSet::new(),
        fifo: VecDeque::new(),
        input: None,
        song,
        event,
        skip,
    };
    if feasible {
        match strategy {
            Strategy::Exhaustive => {
                let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
                members_rec(0, &mut physical, &candidates, &required, allowed.leader, &snaps, &mut engine)?;
            }
            Strategy::Candidate { power_seeds, proposals, proposal_seed } => {
                let warm = Instant::now();
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
                    engine.stats.warmup_ms = warm.elapsed().as_secs_f64() * 1000.0;
                    'seeds: for seed in seeds.results {
                        let d = pool.deck(seed.members, seed.snaps, [0, 1, 2, 3, 4])?;
                        let mut order = [0, 1, 2, 3, 4];
                        loop {
                            let p =
                                PhysicalDeck { members: order.map(|i| d.members[i]), snaps: order.map(|i| d.snaps[i]) };
                            if allowed.leader.is_none_or(|i| p.members[2] == i) && !engine.consider(p)? {
                                break 'seeds;
                            }
                            if !super::live::next_permutation(&mut order) {
                                break;
                            }
                        }
                    }
                }
                let mut rng = ProposalRandom(*proposal_seed | 1);
                for n in 0..*proposals {
                    if engine.expired() {
                        break;
                    }
                    let mut p = if n % 3 != 0 && !engine.top.is_empty() {
                        let mut p = engine.top[rng.index(engine.top.len())].physical;
                        if rng.index(2) == 0 {
                            let slot = rng.index(5);
                            let m = candidates[rng.index(candidates.len())];
                            if (slot != 2 || allowed.leader.is_none()) && !required.contains(&p.members[slot]) {
                                p.members[slot] = m;
                            }
                        } else {
                            let slot = rng.index(5);
                            let s = rng.index(snaps.len() + 1);
                            p.snaps[slot] = if s == snaps.len() { None } else { Some(snaps[s]) };
                        }
                        p
                    } else {
                        random_deck(pool, &candidates, &required, allowed.leader, &snaps, &mut rng)
                    };
                    // Additional physical swaps preserve member/snap pairs and fixed leader.
                    if n % 4 == 0 {
                        let a = rng.index(5);
                        let b = rng.index(5);
                        if allowed.leader.is_none() || (a != 2 && b != 2) {
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
                if engine.stop.is_none() {
                    engine.stop = Some(ExitReason::ProposalLimit)
                }
            }
        }
    }
    let exit_reason = engine.stop.unwrap_or(ExitReason::Exhausted);
    let proven = exit_reason == ExitReason::Exhausted;
    let optimality = if proven {
        Optimality::Proven
    } else if matches!(strategy, Strategy::Candidate { .. }) {
        Optimality::Heuristic
    } else {
        Optimality::Unproven
    };
    let probability_law = if matches!(request.objective.inner(), Objective::LiveScore { .. }) {
        serde_json::json!({"kind":"explicitFiniteNativeRoots","atoms":law.atoms().iter().map(|(r,w)|serde_json::json!([r,w.to_string()])).collect::<Vec<_>>(),"totalWeight":law.total_weight().to_string(),"populationLaw":"unknown; no TickCount population law inferred"})
    } else {
        serde_json::json!({"kind":"deterministic"})
    };
    Ok(RecommendationOutcome {
        format: RESULT_FORMAT,
        completion: if proven { Completion::Complete } else { Completion::TimedOut },
        optimality,
        exit_reason,
        result_identity: "physicalDeck",
        metric: metric.clone(),
        strategy: strategy.clone(),
        probability_law,
        proof_scope: "conditional on declared master, roster, complete judgement/clock inputs, finite native-root law and optional external confirmations; client counters are not server reward authority",
        resolved_context: serde_json::Value::Null,
        results: engine.top.into_iter().map(Entry::wire).collect(),
        stats: engine.stats,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
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
    e.stats.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if required.iter().filter(|r| !p.members[..slot].contains(r)).count() > 5 - slot {
        return Ok(true);
    }
    if slot == 5 {
        return snaps_rec(0, p, snaps, e);
    }
    for &m in candidates {
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
        if !members_rec(slot + 1, p, candidates, required, leader, snaps, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn snaps_rec(slot: usize, p: &mut PhysicalDeck, snaps: &[usize], e: &mut Engine<'_, '_>) -> Result<bool, Error> {
    e.stats.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if slot == 5 {
        return e.consider(*p);
    }
    p.snaps[slot] = None;
    if !snaps_rec(slot + 1, p, snaps, e)? {
        return Ok(false);
    }
    for &s in snaps {
        if p.snaps[..slot].contains(&Some(s)) {
            continue;
        }
        p.snaps[slot] = Some(s);
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

fn validate_payoff(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    input: Option<&EventPayoffInput>,
) -> Result<(), Error> {
    if let Some(id) = metric.event() {
        let ctx = request
            .objective
            .context()
            .ok_or_else(|| Error::Input("event metrics require resolved scenario".into()))?;
        let input = input.ok_or_else(|| Error::Input("event metrics require context.eventPayoff".into()))?;
        ctx.event_request(pool.master, input, id)?;
        if matches!(metric, Metric::ConditionalClientEventItems { .. }) && input.selected_rewards.is_none() {
            return Err(Error::Unsupported(
                "UnknownServerAuthority: selectedRewards are required for conditional items".into(),
            ));
        }
    }
    match (request.objective.inner(), metric) {
        (Objective::Power { .. }, Metric::Power)
        | (
            Objective::SkipScore { .. } | Objective::LiveScore { .. },
            Metric::Score
            | Metric::ScoreAtLeast { .. }
            | Metric::ClientEventPoints { .. }
            | Metric::ConditionalClientEventItems { .. },
        ) => Ok(()),
        _ => Err(Error::Input("metric does not match execution".into())),
    }
}
fn validate_play(
    pool: &Pool,
    request: &SearchRequest,
    network: Option<&[RankConfirmation]>,
    sim: &SimulationInput,
) -> Result<(), Error> {
    let Objective::LiveScore {
        chart,
        play: PlayInput::Stream { stream, judgement_types },
        exclude_snap_skills: false,
        gekisou,
        ..
    } = request.objective.inner()
    else {
        return Err(Error::Input("production expectation requires whole-live Stream including snap skills".into()));
    };
    let setup =
        super::full_setup(pool, &request.objective)?.ok_or_else(|| Error::Input("whole-live setup missing".into()))?;
    if stream.frames.is_empty() {
        return Err(Error::Input("complete play requires nonempty frames".into()));
    }
    let expected: HashSet<_> = chart.notes.iter().filter(|n| is_judgement_note(n.note_type)).map(|n| n.id).collect();
    let judged: HashSet<_> = stream.judged.iter().map(|r| r[1]).collect();
    if !expected.is_subset(&judged) || judged.len() != stream.judged.len() {
        return Err(Error::Input(
            "complete play must include every judged chart note exactly once, including explicit Miss results".into(),
        ));
    }
    let last =
        chart.notes.iter().map(|n| n.time_ms).chain(chart.skill_events.iter().map(|s| s.time_ms)).max().unwrap_or(0);
    if stream.frames.last().copied().unwrap_or(-1) < last {
        return Err(Error::Input("complete clock ends before chart notes/events".into()));
    }
    if sim.music_length_ms.is_some_and(|v| v <= 0)
        || sim.score_music_length_ms.is_some_and(|v| v <= 0)
        || sim.live_finished_from_frame.is_some_and(|v| v >= stream.frames.len())
    {
        return Err(Error::Input("invalid explicit simulation lengths/lifecycle frame".into()));
    }
    if let Some(ctx) = request.objective.context() {
        if matches!(ctx.scenario, Scenario::Mission(_) | Scenario::Battle(_) | Scenario::Arena(_)) && gekisou.is_none()
        {
            return Err(Error::Game(
                "native Mission/Battle/Arena force Gekisou on (1.0.1-25 GetIsGekisouEnabled)".into(),
            ));
        }
        if matches!(ctx.scenario, Scenario::Battle(_) | Scenario::Arena(_)) && network.is_none() {
            return Err(Error::Unsupported(
                "Battle/Arena Gekisou require explicit networkConfirmations; Solo rank-1 substitution is invalid"
                    .into(),
            ));
        }
        if !matches!(ctx.scenario, Scenario::Battle(_) | Scenario::Arena(_)) && network.is_some() {
            return Err(Error::Input("external confirmations are only the declared Battle/Arena adapter".into()));
        }
    } else if network.is_some() {
        return Err(Error::Input("network confirmations require explicit Battle/Arena scenario".into()));
    }
    if let Some(cs) = network {
        let g = setup.gk.as_ref().ok_or_else(|| Error::Input("confirmations require Gekisou".into()))?;
        let missions: [i64; 3] =
            g.setup.missions.clone().try_into().map_err(|_| Error::Input("three missions required".into()))?;
        let factors = crate::live::full::gekisou_rank_factors(pool.master, &missions)?;
        let mut seen = HashSet::new();
        for c in cs {
            if c.frame >= stream.frames.len()
                || c.range >= g.setup.fevers.len()
                || !(1..=5).contains(&c.rank)
                || !seen.insert(c.range)
                || c.percent != factors[c.range][c.rank as usize - 1]
            {
                return Err(Error::Input(
                    "network confirmation requires unique known range, valid frame/rank, and matching master percent"
                        .into(),
                ));
            }
        }
        if seen.len() != g.setup.fevers.len() {
            return Err(Error::Input("complete network play requires one confirmation per fever range".into()));
        }
    }
    // Keep this check explicit even if all constraints eliminate decks.
    if judgement_types.len() != chart.notes.len() {
        return Err(Error::Input("chart judgement type count differs".into()));
    }
    Ok(())
}

/// JSON API boundary shared by the standalone production CLI and integration adapters.
pub fn recommend(data: &DeckData, roster: &Roster, r: &RecommendationRequest) -> Result<RecommendationOutcome, Error> {
    if r.format != REQUEST_FORMAT {
        return Err(Error::Input(format!("unsupported recommendation format {}", r.format)));
    }
    let start = Instant::now();
    let score_id = match r.execution {
        Execution::Power { .. } => None,
        Execution::Skip { score_id } | Execution::Live { score_id, .. } => Some(score_id),
    };
    if score_id.is_some() && r.scenario.is_none() {
        return Err(Error::Input("skip/live require an explicit scenario".into()));
    }
    let context_input = r.context.clone().unwrap_or(ContextInput {
        power_snapshot: PowerSnapshotInput { event_ids: roster.player.events.clone(), captured_jst_ticks: None },
        result_clock: None,
        event_payoff: None,
    });
    let fevers = score_id.and_then(|i| data.data_chart(i)).map(|c| c.fevers.as_slice()).unwrap_or(&[]);
    let context =
        r.scenario.as_ref().map(|s| context_input.resolve(&data.master, s.scenario(), score_id, fevers)).transpose()?;
    let pool = match &context {
        Some(c) => c.pool(&data.master, roster)?,
        None => {
            let mut frozen = roster.clone();
            frozen.player.events = context_input.power_snapshot.event_ids.clone();
            Pool::new(&data.master, &frozen)?
        }
    };
    let objective = match &r.execution {
        Execution::Power { music_id, event_parameter } => {
            if context.is_some() && *event_parameter {
                return Err(Error::Input("explicit scenario determines event parameters".into()));
            }
            Objective::Power { music_id: *music_id, event: *event_parameter }
        }
        Execution::Skip { score_id } => Objective::SkipScore { score_id: *score_id, chart: data.chart(*score_id)? },
        Execution::Live { score_id, gekisou, play } => {
            let chart = data.chart(*score_id)?;
            let dc = data.data_chart(*score_id).ok_or_else(|| Error::Input("chart is absent".into()))?;
            let ctx = context.as_ref().expect("explicit live scene");
            let stream = match play {
                PlayPolicy::Stream { stream } => stream.clone(),
                PlayPolicy::TheoreticalBest if *gekisou => JudgementStream::theoretical_best_gekisou(
                    &chart,
                    &dc.judgement_types,
                    &JustRule::new(&data.master, &ctx.gekisou)?,
                )?,
                PlayPolicy::TheoreticalBest => JudgementStream::theoretical_best(&chart),
            };
            Objective::LiveScore {
                score_id: *score_id,
                chart,
                play: PlayInput::Stream { stream, judgement_types: dc.judgement_types.clone() },
                event: false,
                exclude_snap_skills: false,
                gekisou: gekisou.then(|| GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: dc.fevers.clone() }),
            }
        }
    };
    let objective = match &context {
        Some(c) => objective.in_scenario(c.clone()),
        None => objective,
    };
    let request = SearchRequest { objective, k: r.k, constraints: r.constraints.clone(), time_limit: None };
    let law = match (&r.execution, &r.seed_law) {
        (Execution::Live { .. }, Some(l)) => {
            if l.provenance.trim().is_empty() {
                return Err(Error::Input("seedLaw.provenance must declare its assumption/source".into()));
            }
            FiniteSeedLaw::new(l.atoms.clone())?
        }
        (Execution::Live { .. }, None) => {
            return Err(Error::Input(
                "live requires explicit positive-mass seedLaw; native TickCount population law is unknown".into(),
            ));
        }
        (_, Some(_)) => return Err(Error::Input("seedLaw applies only to played live".into())),
        _ => FiniteSeedLaw::new(vec![(0, 1)])?,
    };
    validate(r.k, &law, &r.limits, &r.strategy)?;
    validate_payoff(&pool, &request, &r.metric, context_input.event_payoff.as_ref())?;
    let mut limits = r.limits.clone();
    if let Some(ms) = limits.time_limit_ms {
        limits.time_limit_ms = Some(ms.saturating_sub(start.elapsed().as_millis().min(u64::MAX as u128) as u64));
    }
    let mut out = if matches!(
        (&r.execution, &r.metric),
        (Execution::Power { .. }, Metric::Power) | (Execution::Skip { .. }, Metric::Score)
    ) {
        if !matches!(r.strategy, Strategy::Exhaustive) {
            return Err(Error::Input(
                "power/skip score use exact canonical search; candidate strategy applies to physical metrics".into(),
            ));
        }
        if r.network_confirmations.is_some()
            || r.simulation.music_length_ms.is_some()
            || r.simulation.score_music_length_ms.is_some()
            || r.simulation.live_finished_from_frame.is_some()
        {
            return Err(Error::Input("simulation/network inputs apply only to live".into()));
        }
        let mut exact = request.clone();
        exact.time_limit = limits.time_limit_ms.map(Duration::from_millis);
        let s = super::search(&pool, &exact)?;
        let complete = s.completion == Completion::Complete;
        RecommendationOutcome {
            format: RESULT_FORMAT,
            completion: s.completion,
            optimality: if complete { Optimality::Proven } else { Optimality::Unproven },
            exit_reason: if complete { ExitReason::Exhausted } else { ExitReason::TimeLimit },
            result_identity: "canonicalMemberSet",
            metric: r.metric.clone(),
            strategy: r.strategy.clone(),
            probability_law: serde_json::json!({"kind":"deterministic"}),
            proof_scope: "exact canonical member-set TopK under existing proven nonnegative nonoverflow domain; maxCandidates applies to physical search, not legacy branch search",
            resolved_context: serde_json::Value::Null,
            results: s
                .results
                .into_iter()
                .map(|d| RecommendedDeck {
                    members: d.members,
                    snaps: d.snaps,
                    power: d.power,
                    expected_score: d.score.map(|n| ExactExpectation { numerator: n as i128, denominator: 1 }.into()),
                    expected_payoff: ExactExpectation { numerator: d.score.unwrap_or(d.power) as i128, denominator: 1 }
                        .into(),
                    atoms: Vec::new(),
                })
                .collect(),
            stats: Stats {
                nodes: s.stats.nodes,
                visited_candidates: s.stats.leaves,
                evaluated: s.stats.matchings,
                peak_retained_decks: r.k,
                ..Default::default()
            },
            elapsed_ms: s.elapsed.as_secs_f64() * 1000.0,
        }
    } else {
        solve_physical(
            &pool,
            &request,
            &law,
            &r.metric,
            context_input.event_payoff.as_ref(),
            &limits,
            &r.strategy,
            r.network_confirmations.as_deref(),
            &r.simulation,
        )?
    };
    if let Some(l) = &r.seed_law {
        out.probability_law["provenance"] = serde_json::Value::String(l.provenance.clone());
    }
    out.resolved_context = serde_json::json!({"dataProvenance":data.provenance,"scenario":context.as_ref().map(|c|format!("{:?}",c.scenario)),"baseLiveMusicId":context.as_ref().map(|c|c.resolved.live_music_id),"scoreId":score_id,"calcEventParameter":context.as_ref().map(|c|c.resolved.calc_event_parameter),"skillTargetMusicType":context.as_ref().map(|c|c.resolved.skill_target_music_type),"gekisouMissions":context.as_ref().map(|c|c.resolved.gekisou_missions),"context":context_input,"simulation":r.simulation,"networkConfirmations":r.network_confirmations,"playPolicy":match &r.execution {Execution::Live{play:PlayPolicy::TheoreticalBest,..}=>"explicit theoretical AP/Just scenario",Execution::Live{..}=>"declared judgement stream",_=>"deterministic"},"eligibility":"unlock/progression eligibility not inferred","nativeEvidence":"1.0.1-25 AArch64; selected regional master is explicit data, online patch/version parity not inferred"});
    out.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(out)
}
