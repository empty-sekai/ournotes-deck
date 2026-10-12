//! The answer format.

use super::*;

/// How far a recommendation got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// `result` holds the recommendation (progress reports too).
    Ok,
    /// The account lacks facts the goal reads (`missing`); nothing else is wrong.
    Incomplete,
    /// The account or the request is invalid (`errors`, possibly with `missing`).
    Invalid,
    /// The inputs are valid but the computation is unsupported or failed (`errors`).
    Failed,
}

/// The answer to one recommendation, final or a progress report.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub format: &'static str,
    /// The lowercase hex SHA-256 of the deck data text, which an account's `datasetId` must name.
    pub dataset_id: Option<String>,
    /// False for progress reports.
    #[serde(rename = "final")]
    pub is_final: bool,
    pub status: Status,
    pub missing: Vec<Issue>,
    pub errors: Vec<Issue>,
    pub result: Option<AnswerResult>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    /// The goal as computed, with its defaults and the chart's score id.
    pub goal: Value,
    /// The metric as computed (null for the power goal).
    pub metric: Option<Value>,
    /// `search` in progress reports, `done` in the final answer.
    pub phase: &'static str,
    pub elapsed_ms: f64,
    pub optimality: OptimalityReport,
    /// Best first.
    pub teams: Vec<Team>,
    /// The account's scope.
    pub account: Value,
    /// Diagnostics of the search, with `memory`.
    pub telemetry: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimalityReport {
    /// Whether the teams are proven to be the best `k`.
    pub proven: bool,
    /// The best team's value (rounded down; a probability for probability metrics).
    pub lower_bound: Option<Value>,
    /// A value no team can exceed (rounded up; a probability for probability metrics), when known.
    pub upper_bound: Option<Value>,
    pub best_gap: Option<f64>,
    pub kth_gap: Option<f64>,
    /// Share of the search tree decided, for a progress bar.
    pub fraction: Option<f64>,
}

/// A member card and its Snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Pair {
    pub member: i64,
    pub snap: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub rank: usize,
    pub leader: Pair,
    /// The other four, in ascending member card id order; where they sit does not change the value.
    pub others: [Pair; 4],
    pub power: i32,
    /// Null for the power goal.
    pub value: Option<TeamValue>,
    /// A played live's order results; null for skip, power and progress reports.
    pub orders: Option<Orders>,
    /// The canonical layout: the leader in slot 2, the others in slots 0, 1, 3, 4.
    pub layout: Layout,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank_certified: Option<bool>,
    /// Exact expectations for a Challenge-point priority objective.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_rewards: Option<TeamEventRewards>,
    /// A combined metric: the team's value on each level, in the order of `metric.levels`. `value.payoff` is that
    /// of the first level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<TeamLevel>>,
}

/// A team's value on one level of a combined metric.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamLevel {
    /// The exact expectation of the level's weighted sum.
    pub value: PayoffValue,
    /// The exact expectation of each term's payoff, unweighted, in the order of the level's terms. Null for a
    /// term that reads terminal life.
    pub terms: Vec<Option<PayoffValue>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamEventRewards {
    pub challenge_points: PayoffValue,
    pub event_points: PayoffValue,
    pub event_items: PayoffValue,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamValue {
    /// The expected score, rounded down.
    pub score: i64,
    pub exact: Option<Fraction>,
    /// Bounds of the expected score when it is only known within them.
    pub interval: Option<Interval>,
    /// The expected metric value when the metric is not the score.
    pub payoff: Option<PayoffValue>,
}

pub type Interval = FractionInterval;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayoffValue {
    pub score: f64,
    pub exact: Option<Fraction>,
    pub interval: Option<Interval>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Orders {
    pub count: usize,
    pub min: OrderPoint,
    /// The 60th smallest score (lower median), a real order.
    pub median: OrderPoint,
    pub max: OrderPoint,
    /// The order with the highest metric value (then score).
    pub best: BestOrder,
    /// The score of each order, by lexicographic order index over the layout's slots.
    pub values: Vec<i64>,
    /// The metric value of each order when the metric is not the score.
    pub payoff_values: Option<Vec<i64>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderPoint {
    pub score: i64,
    /// Member card ids in performance order.
    pub order: [i64; 5],
}

#[derive(Clone, Debug, Serialize)]
pub struct BestOrder {
    pub score: i64,
    pub payoff: Option<i64>,
    pub order: [i64; 5],
}

#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
}

/// Progress reports of [`crate::engine::recommend_account`]: each is a complete answer (`final: false`).
pub struct AnswerProgress<'a> {
    pub interval: Duration,
    pub report: &'a mut dyn FnMut(&Answer),
}
