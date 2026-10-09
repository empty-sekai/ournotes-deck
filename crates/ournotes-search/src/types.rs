//! Explicit request, objective, completion and result contracts.
use crate::search::expectation::ExactExpectation;
use crate::search::telemetry::Telemetry;
use crate::search::{Completion, Constraints};
use ournotes_sim::live::model::{Accuracy, JudgementStream};
use ournotes_sim::replay::RankConfirmation;
use ournotes_sim::scenario::{ContextInput, Scenario};
use serde::{Deserialize, Serialize};

pub const REQUEST_FORMAT: &str = "ournotes-deck.search-request/1";
pub const RESULT_FORMAT: &str = "ournotes-deck.recommendation-result/3";
pub const MAX_K: usize = 100;
/// Most decks a request may supply as initial incumbents.
pub const MAX_INITIAL_DECKS: usize = 100;
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
    #[serde(default)]
    pub aggregation: Aggregation,
    /// Optional player intent. It is checked against execution/metric, never cosmetic.
    pub goal: Option<PlayerGoal>,
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
    /// Legal decks of the domain evaluated exactly before the search (for example a fast heuristic's best decks).
    /// They only fill the Top-K earlier; the result and its proof do not depend on them.
    #[serde(default)]
    pub initial_decks: Vec<DeckInput>,
}

/// How reachable terminal payoffs are combined into the value of a team.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Aggregation {
    #[default]
    Expected,
    Maximum,
}

/// A deck by public IDs: member cards in physical slots (slot 2 leads) and each slot's Snap.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckInput {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
}

// An explicitly supplied null declares an empty network timeline (validated as
// incomplete when the chart has ranges). Absence does not invent peer ranks.
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
    /// The theoretical best play with a stated share of Great and Just judgements
    /// (`JudgementStream::with_accuracy`).
    Accuracy(Accuracy),
    Stream {
        stream: JudgementStream,
    },
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
    #[serde(default)]
    great_fraction: RequestField<f64>,
    #[serde(default)]
    just_fraction: RequestField<f64>,
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
                w.great_fraction.reject("greatFraction", &["kind"])?;
                w.just_fraction.reject("justFraction", &["kind"])?;
                Ok(Self::TheoreticalBest)
            }
            "accuracy" => {
                w.stream.reject("stream", &["kind", "greatFraction", "justFraction"])?;
                Ok(Self::Accuracy(Accuracy {
                    great_fraction: w.great_fraction.required("greatFraction")?,
                    just_fraction: w.just_fraction.required("justFraction")?,
                }))
            }
            "stream" => {
                w.great_fraction.reject("greatFraction", &["kind", "stream"])?;
                w.just_fraction.reject("justFraction", &["kind", "stream"])?;
                Ok(Self::Stream { stream: w.stream.required("stream")? })
            }
            other => Err(serde::de::Error::unknown_variant(other, &["theoreticalBest", "accuracy", "stream"])),
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
    /// Challenge points earned by an ordinary played/skip result, before any later challenge consumption.
    ClientChallengePoints {
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
            Self::ClientEventPoints { event_id }
            | Self::ClientChallengePoints { event_id }
            | Self::ConditionalClientEventItems { event_id, .. } => Some(event_id),
            _ => None,
        }
    }
    pub(crate) fn upper(&self) -> Option<i128> {
        match self {
            Self::Score | Self::ClientEventPoints { .. } | Self::ClientChallengePoints { .. } => Some(i32::MAX as i128),
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
    RefinementRequired,
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
/// Certified endpoints are exact binary64 rationals, not estimates of the expectation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FractionInterval {
    pub lower: Fraction,
    pub upper: Fraction,
    #[serde(skip)]
    binary: [u64; 2],
}
impl FractionInterval {
    pub fn from_f64(lower: f64, upper: f64) -> Result<Self, ournotes_sim::Error> {
        if !lower.is_finite() || !upper.is_finite() || lower > upper {
            return Err(ournotes_sim::Error::Domain("invalid certified interval endpoints".into()));
        }
        Ok(Self {
            lower: binary_fraction(lower),
            upper: binary_fraction(upper),
            binary: [lower.to_bits(), upper.to_bits()],
        })
    }
    pub fn lower_f64(&self) -> f64 {
        f64::from_bits(self.binary[0])
    }
    pub fn upper_f64(&self) -> f64 {
        f64::from_bits(self.binary[1])
    }
}

fn binary_fraction(value: f64) -> Fraction {
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mut mantissa = bits & ((1u64 << 52) - 1);
    let mut shift = if exponent == 0 {
        -1074
    } else {
        mantissa |= 1u64 << 52;
        exponent - 1023 - 52
    };
    if mantissa == 0 {
        return Fraction { numerator: "0".into(), denominator: "1".into() };
    }
    let trailing = mantissa.trailing_zeros() as i32;
    mantissa >>= trailing;
    shift += trailing;
    fn double_decimal(value: &mut String) {
        let mut carry = 0;
        let mut digits = value.as_bytes().to_vec();
        for digit in digits.iter_mut().rev() {
            let next = (*digit - b'0') * 2 + carry;
            *digit = b'0' + next % 10;
            carry = next / 10;
        }
        if carry != 0 {
            digits.insert(0, b'0' + carry);
        }
        *value = String::from_utf8(digits).expect("decimal digits");
    }
    let mut numerator = mantissa.to_string();
    let mut denominator = "1".to_string();
    for _ in 0..shift.max(0) {
        double_decimal(&mut numerator);
    }
    for _ in 0..(-shift).max(0) {
        double_decimal(&mut denominator);
    }
    if value.is_sign_negative() {
        numerator.insert(0, '-');
    }
    Fraction { numerator, denominator }
}

#[cfg(test)]
mod certified_wire_tests {
    use super::*;
    #[test]
    fn binary_endpoints_are_exact_rationals_even_outside_i128_capacity() {
        let tenth = binary_fraction(0.1);
        assert_eq!(tenth.numerator, "3602879701896397");
        assert_eq!(tenth.denominator, "36028797018963968");
        assert_eq!(binary_fraction(-1.5), Fraction { numerator: "-3".into(), denominator: "2".into() });
        let smallest = binary_fraction(f64::from_bits(1));
        assert_eq!(smallest.numerator, "1");
        assert_eq!(smallest.denominator.len(), 324);
        for (lo, hi) in [(-1.5, 0.1), (f64::from_bits(1), f64::from_bits(2)), (f64::MAX, f64::MAX)] {
            let v = FractionInterval::from_f64(lo, hi).unwrap();
            assert_eq!(v.lower_f64().to_bits(), lo.to_bits());
            assert_eq!(v.upper_f64().to_bits(), hi.to_bits());
            let json = serde_json::to_value(v).unwrap();
            assert!(json.get("binary").is_none());
            assert_eq!(json["lower"]["numerator"], binary_fraction(lo).numerator);
        }
        assert!(FractionInterval::from_f64(f64::NAN, 1.0).is_err());
        assert!(FractionInterval::from_f64(2.0, 1.0).is_err());
    }
}
/// One performance order of a played-live result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderResult {
    /// The physical slots of the result's `members` in performance order.
    pub performance_order: [usize; 5],
    /// The member cards in performance order.
    pub members: [i64; 5],
    pub score: i32,
    pub payoff: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSummary {
    pub minimum: i32,
    pub maximum: i32,
    /// Lower quantiles over the outcomes of the value (the 120 equally likely performance orders of a played live).
    pub p10: i32,
    pub p50: i32,
    pub p90: i32,
    pub target_score: Option<i32>,
    pub probability_at_least: Option<Fraction>,
    pub expected_shortfall: Option<Fraction>,
}
/// One result. For a played live it is a team in its canonical layout (the leader in slot 2, the other members in
/// ascending card ID order, each with its Snap), valued by the selected aggregation of reachable outcomes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedDeck {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
    pub power: i32,
    /// The value used for ranking under the declared aggregation.
    pub objective_value: Option<Fraction>,
    /// The greatest reachable final score, when evaluated exactly.
    pub maximum_score: Option<i32>,
    pub expected_score: Option<Fraction>,
    pub expected_payoff: Option<Fraction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_interval: Option<FractionInterval>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payoff_interval: Option<FractionInterval>,
    /// True only for a rank individually established by the certified frontier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank_certified: Option<bool>,
    pub score_summary: Option<ScoreSummary>,
    /// Played lives: the performance order with the highest payoff (then score; the first in lexicographic order).
    pub best_order: Option<OrderResult>,
    /// Played lives: each of the 120 performance orders (the result's slots in performance order) with its score
    /// and payoff; empty otherwise. Not part of the JSON result.
    #[serde(skip)]
    pub order_outcomes: Vec<([usize; 5], i32, i128)>,
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
    pub aggregation: Aggregation,
    pub player_goal: Option<GoalDescription>,
    pub strategy: Strategy,
    pub probability_law: serde_json::Value,
    pub proof_scope: &'static str,
    pub resolved_context: serde_json::Value,
    pub results: Vec<RecommendedDeck>,
    pub telemetry: Telemetry,
    pub elapsed_ms: f64,
}
