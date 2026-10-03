//! Explicit request, objective, completion and result contracts.
use crate::search::expectation::ExactExpectation;
use crate::search::telemetry::Telemetry;
use crate::search::{Completion, Constraints};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::replay::RankConfirmation;
use ournotes_sim::scenario::{ContextInput, Scenario};
use serde::{Deserialize, Serialize};

pub const REQUEST_FORMAT: &str = "ournotes-deck.recommendation-request/1";
pub const RESULT_FORMAT: &str = "ournotes-deck.recommendation-result/2";
pub const MAX_K: usize = 100;
pub const MAX_ATOMS: usize = 4096;
pub(crate) const MAX_RESULT_ATOMS: usize = 65_536;
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
    /// Optional player intent. It is checked against execution/metric, never cosmetic.
    pub goal: Option<PlayerGoal>,
    pub seed_law: Option<SeedLawInput>,
    #[serde(default, deserialize_with = "strict_constraints")]
    pub constraints: Constraints,
    #[serde(default = "five")]
    pub k: usize,
    #[serde(default)]
    pub strategy: Strategy,
    #[serde(default)]
    pub limits: Limits,
    /// Conditional input; never a prediction of opponents' native ranks.
    #[serde(default, deserialize_with = "explicit_network_confirmations")]
    pub network_confirmations: Option<Vec<RankConfirmation>>,
    #[serde(default)]
    pub simulation: SimulationInput,
}

// An explicitly supplied null still declares unsupported network input. Absence
// alone means this adapter should use the current core's Solo behavior.
fn explicit_network_confirmations<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<RankConfirmation>>, D::Error> {
    Ok(Some(Option::<Vec<RankConfirmation>>::deserialize(deserializer)?.unwrap_or_default()))
}

// Validate only the new JSON transport; the shared core Constraints API is unchanged.
fn strict_constraints<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Constraints, D::Error> {
    #[derive(Default, Deserialize)]
    #[serde(rename_all = "camelCase", default, deny_unknown_fields)]
    struct Wire {
        leader: Option<i64>,
        include_members: Vec<i64>,
        exclude_members: Vec<i64>,
        exclude_snaps: Vec<i64>,
        no_snaps: bool,
    }
    let wire = Wire::deserialize(deserializer)?;
    Ok(Constraints {
        leader: wire.leader,
        include_members: wire.include_members,
        exclude_members: wire.exclude_members,
        exclude_snaps: wire.exclude_snaps,
        no_snaps: wire.no_snaps,
    })
}

fn explicit_finished_frame<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<usize>, D::Error> {
    // Null is an explicit lifecycle declaration too; it cannot opt into implicit settlement.
    Ok(Some(Option::<usize>::deserialize(deserializer)?.unwrap_or_default()))
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
    #[serde(default, deserialize_with = "strict_stream")]
    stream: RequestField<JudgementStream>,
}
fn strict_stream<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<RequestField<JudgementStream>, D::Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Wire {
        frames: Vec<i32>,
        #[serde(default)]
        judged: Vec<[i32; 4]>,
        #[serde(default)]
        base_seed: i32,
        #[serde(default)]
        assist: bool,
        #[serde(default)]
        delta_times: Option<Vec<f32>>,
    }
    let wire = Wire::deserialize(deserializer)?;
    Ok(RequestField::Present(JudgementStream {
        frames: wire.frames,
        judged: wire.judged,
        base_seed: wire.base_seed,
        assist: wire.assist,
        delta_times: wire.delta_times,
    }))
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    pub(crate) fn scenario(&self) -> Scenario {
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
    /// E(min(final score, threshold)): additional score above the target has no utility.
    CappedScore {
        threshold: i32,
    },
    /// Probability of both a score target and terminal life; not a clear/failure oracle.
    ScoreAndLifeAtLeast {
        threshold: i32,
        #[serde(rename = "minFinalLife")]
        min_final_life: i32,
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
    pub(crate) fn event(&self) -> Option<i64> {
        match *self {
            Self::ClientEventPoints { event_id } | Self::ConditionalClientEventItems { event_id, .. } => Some(event_id),
            _ => None,
        }
    }
    pub(crate) fn upper(&self) -> Option<i128> {
        match self {
            Self::Score | Self::ClientEventPoints { .. } => Some(i32::MAX as i128),
            Self::ScoreAtLeast { .. } | Self::ScoreAndLifeAtLeast { .. } => Some(1),
            Self::CappedScore { threshold } => Some(*threshold as i128),
            _ => None,
        }
    }
    pub(crate) fn target(&self) -> Option<i32> {
        match *self {
            Self::ScoreAtLeast { threshold }
            | Self::CappedScore { threshold }
            | Self::ScoreAndLifeAtLeast { threshold, .. } => Some(threshold),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayerGoal {
    DailyHighScore,
    StableTarget,
    EventFarming,
    SkipFarming,
    GekisouScore,
    Power,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalDescription {
    pub kind: PlayerGoal,
    pub title: &'static str,
    pub payoff_meaning: &'static str,
    pub assumptions: Vec<&'static str>,
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
    Exhaustive,
    /// Exact joint member/Snap branch-and-bound, with exhaustive fallback outside the bound domain.
    #[default]
    BranchAndBound,
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
        Self { time_limit_ms: Some(3000), max_candidates: None, cache_entries: 2048 }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SimulationInput {
    pub music_length_ms: Option<i32>,
    pub score_music_length_ms: Option<i32>,
    #[serde(deserialize_with = "explicit_finished_frame")]
    pub live_finished_from_frame: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Optimality {
    Proven,
    Unproven,
    Heuristic,
    /// A requested deck was evaluated; no alternative decks were searched.
    NotApplicable,
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
    /// Present for played lives. Zero does not infer the server's failure/continue route.
    pub final_life: Option<i32>,
    pub converted_judgements: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSummary {
    pub minimum: i32,
    pub maximum: i32,
    /// Lower weighted quantiles, under the SAME declared finite law as the objective.
    pub p10: i32,
    pub p50: i32,
    pub p90: i32,
    pub target_score: Option<i32>,
    pub probability_at_least: Option<Fraction>,
    pub expected_shortfall: Option<Fraction>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedDeck {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
    pub power: i32,
    pub expected_score: Option<Fraction>,
    pub expected_payoff: Fraction,
    pub score_summary: Option<ScoreSummary>,
    pub atoms: Vec<AtomResult>,
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
    pub player_goal: Option<GoalDescription>,
    pub strategy: Strategy,
    pub probability_law: serde_json::Value,
    pub proof_scope: &'static str,
    pub resolved_context: serde_json::Value,
    pub results: Vec<RecommendedDeck>,
    pub telemetry: Telemetry,
    pub elapsed_ms: f64,
}
