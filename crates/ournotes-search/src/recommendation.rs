//! The page recommendation: an account (`ournotes.account/1`) and a request (`ournotes-deck.recommendation-request/2`)
//! in, one answer (`ournotes-deck.account-recommendation/1`) out. The request names a goal (a scene of the game, or
//! the deck power) and what to maximize; the answer ranks teams (a leader and four other members, each with a Snap)
//! by their value and reports every input problem with its JSON path. See `docs/recommendation.md`.
//!
//! A played live is valued by the mean over its 120 equally likely performance orders, or by its best order
//! (`aggregation`). Each goal and metric is
//! supported with a proof, supported without a guaranteed proof, or not supported yet ([`capabilities`]).

use crate::clock::Instant;
use crate::search::Constraints;
use crate::search::physical::ProgressHook;
use crate::types::{
    Aggregation, Execution, Fraction, FractionInterval, Limits, MAX_K, MAX_METRIC_LEVELS, MAX_METRIC_TERMS,
    MAX_TERM_WEIGHT, MaximumModel, Metric, MetricLevel, MetricTerm, Optimality, PlayPolicy, RecommendationOutcome,
    RecommendationRequest, RecommendedDeck, Scene, SimulationInput, Strategy,
};
use ournotes_sim::Error;
use ournotes_sim::account::{AccountInput, Exclusions, Issue};
use ournotes_sim::data::DeckData;
use ournotes_sim::event::{EventWindow, LocalEvent};
use ournotes_sim::live::model::{Accuracy, JudgementStream, JustRule};
use ournotes_sim::scenario::{
    ContextInput, EventPayoffInput, MultiplayerRankInput, MultiplayerResultPanelInput, MultiplayerScorePolicy,
    PowerSnapshotInput, ResultClockInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

mod answer;
mod goals;
mod metrics;
mod parse;
mod play;
mod results;
mod wire;
pub use answer::{
    Answer, AnswerProgress, AnswerResult, BestOrder, Interval, Layout, OptimalityReport, OrderPoint, Orders, Pair,
    PayoffValue, Status, Team, TeamEventRewards, TeamLevel, TeamValue,
};
pub use goals::{GoalKind, Support, support};
use metrics::{combined_metric, metric_fields, simple_metric};
use parse::{Issues, Parsed, complete_stream, fraction, parse_request};
use results::{issue, result_of, status_of};
use wire::{MetricTermWire, MetricWire, RequestWire};

pub const REQUEST_FORMAT: &str = "ournotes-deck.recommendation-request/2";
pub const ANSWER_FORMAT: &str = "ournotes-deck.account-recommendation/1";
/// Teams returned when the request does not say.
pub const DEFAULT_K: usize = 5;
/// The performance orders of a played live: every order of the five members, equally likely.
pub const ORDERS: usize = 120;
// ---------------------------------------------------------------------------------------------------------------
// Request

/// Lexicographic index of a performance order among the 120 orders.
pub fn order_index(order: &[usize; 5]) -> usize {
    let mut index = 0;
    for i in 0..5 {
        index = index * (5 - i) + (i + 1..5).filter(|&j| order[j] < order[i]).count();
    }
    index
}

/// The performance order of a lexicographic index (inverse of [`order_index`]).
pub fn order_of_index(mut index: usize) -> [usize; 5] {
    let mut left = vec![0, 1, 2, 3, 4];
    let mut order = [0; 5];
    for (i, o) in order.iter_mut().enumerate() {
        let block = (1..5 - i).product::<usize>();
        *o = left.remove(index / block);
        index %= block;
    }
    order
}

// ---------------------------------------------------------------------------------------------------------------
// Answer

/// See [`crate::engine::recommend_account`].
pub(crate) fn recommend(
    data: &DeckData,
    account_json: &str,
    request_json: &str,
    progress: Option<AnswerProgress<'_>>,
) -> Answer {
    recommend_started(data, account_json, request_json, progress, Instant::now())
}

/// Keep one origin through parsing, resolution, problem construction and search. A supplied origin is private to
/// this module; tests use it to represent elapsed request work without sleeping or exposing a clock override.
fn recommend_started(
    data: &DeckData,
    account_json: &str,
    request_json: &str,
    progress: Option<AnswerProgress<'_>>,
    start: Instant,
) -> Answer {
    let mut answer = Answer {
        format: ANSWER_FORMAT,
        dataset_id: data.sha256.clone(),
        is_final: true,
        status: Status::Invalid,
        missing: Vec::new(),
        errors: Vec::new(),
        result: None,
    };
    let mut issues = Issues(Vec::new());
    let wire = serde_json::from_str::<RequestWire>(request_json)
        .map_err(|e| issues.add("request", "parse", e.to_string()))
        .ok();
    let account = AccountInput::from_json(account_json)
        .map_err(|e| issues.add("account", "parse", e.to_string().trim_start_matches("input: ").to_string()))
        .ok();
    let parsed = wire.and_then(|w| parse_request(data, w, &mut issues));
    // The account is resolved for the goal's facts whenever the goal is known.
    let resolved = match (&parsed, &account) {
        (Some(p), Some(account)) => {
            let resolution = account.resolve(data, crate::account::goal_of(&p.search.execution), &p.exclusions);
            answer.missing = resolution.missing;
            issues.0.extend(resolution.errors);
            resolution.resolved
        }
        _ => None,
    };
    answer.errors = issues.0;
    answer.status = status_of(&answer.missing, &answer.errors);
    let (Some(parsed), Some(resolved), Status::Ok) = (parsed, resolved, answer.status) else { return answer };

    let scope = resolved.scope();
    let dataset_id = answer.dataset_id.clone();
    let mut forward;
    let hook = match progress {
        Some(AnswerProgress { interval, report }) => {
            let parsed = &parsed;
            let scope = &scope;
            forward = move |out: RecommendationOutcome| {
                if let Ok(result) = result_of(&out, parsed, scope, start, false) {
                    report(&Answer {
                        format: ANSWER_FORMAT,
                        dataset_id: dataset_id.clone(),
                        is_final: false,
                        status: Status::Ok,
                        missing: Vec::new(),
                        errors: Vec::new(),
                        result: Some(result),
                    });
                }
            };
            Some(ProgressHook { interval, report: &mut forward })
        }
        None => None,
    };
    let outcome = crate::engine::recommend_hooked_started(data, resolved.roster(), &parsed.search, hook, start)
        .and_then(|outcome| result_of(&outcome, &parsed, &scope, start, true));
    match outcome {
        Ok(result) => answer.result = Some(result),
        Err(error) => {
            answer.errors.push(issue("request", error));
            answer.status = status_of(&answer.missing, &answer.errors);
        }
    }
    answer
}

/// What this solver computes, for a page to enable its controls. See `docs/recommendation.md`.
pub fn capabilities() -> Value {
    let goals: Vec<GoalKind> = GoalKind::ALL
        .into_iter()
        .filter(|k| k.metrics().iter().any(|m| support(*k, m) != Support::Unsupported))
        .collect();
    let mut metrics = serde_json::Map::new();
    let mut event_ids = serde_json::Map::new();
    for kind in &goals {
        metrics.insert(
            kind.name().into(),
            json!(kind.metrics().iter().filter(|m| support(*kind, m) != Support::Unsupported).collect::<Vec<_>>()),
        );
        event_ids.insert(kind.name().into(), kind.reads_event_ids());
    }
    let support_by_goal: std::collections::BTreeMap<_, _> = GoalKind::ALL
        .iter()
        .map(|kind| {
            let metrics: std::collections::BTreeMap<_, _> =
                kind.metrics().iter().map(|metric| (*metric, support(*kind, metric).name())).collect();
            (kind.name(), metrics)
        })
        .collect();
    json!({
        "requestFormat": REQUEST_FORMAT,
        "answerFormat": ANSWER_FORMAT,
        "accountFormat": ournotes_sim::account::FORMAT,
        "goals": goals.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "metrics": metrics,
        "defaultAggregation": "expected",
        "aggregations": {
            "expected": {
                "goals": goals.iter().map(|k| k.name()).collect::<Vec<_>>(),
                "law": "mean payoff over the 120 equally likely performance orders and the declared lottery law",
                "tieBreak": ["expectedPayoff", "power", "canonicalTeamKey"],
            },
            "maximum": {
                "goals": goals.iter().map(|k| k.name()).collect::<Vec<_>>(),
                "law": "maximum over the 120 performance orders of the expected payoff over the random draws of the order",
                "model": MaximumModel::for_execution(true),
                "tieBreak": ["maximumPayoff", "power", "canonicalTeamKey"],
                "bestOrder": "the first performance order, in lexicographic order, with the greatest value",
                "luckMissions": false,
            },
        },
        "accuracy": {"great": true, "just": true},
        "play": {"completeStream":true,"field":"goal.play","mutuallyExclusiveWith":"goal.accuracy",
            "judgements":"NoteSimulateJudgement: 1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just"},
        "patternPlay": {"field":"goal.play","kind":"pattern",
            "required":["greatFraction","justFraction","missEvery"],
            "goals":goals.iter().filter(|k| k.live()).map(|k| k.name()).collect::<Vec<_>>(),
            "fractions":{"minimum":0,"maximum":1,"justRequiresGekisou":true},
            "missEvery":{"minimum":0,"integer":true,"zero":"no Miss",
                "positive":"every Nth judged note, in native stream order (chart time, note id)"},
            "accuracyBeforeMisses":true,"completeStream":true,"survivalProbability":false},
        "scoreAndLife": {"requiresCompleteJudgementStream":true,"survivalProbability":false},
        "skip": {"musicIdOrChallengeMusicId":true,"mutuallyExclusive":true,
            "challengeMetrics":["score","scoreAtLeast","cappedScore","eventPoints","eventItems"]},
        "ranks": [1],
        "rankConfirmation": "onCompletion",
        "rankAssumption": "player-declared rank 1 for each completed Battle/Arena range; native network frame snapshots",
        "eventIds": event_ids,
        "luckMissions": true,
        "luckGreatConversions": false,
        "provenGoals": goals.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "support": support_by_goal,
        "memoryBudgetBytes": null,
        "lottery": "certified expectations; only proved rank separation or equality certifies TopK",
        "accuracyLaw": "deterministic evenly spread Greats; Just share of remaining eligible notes",
        "tieBreak": ["expectedPayoff", "power", "canonicalTeamKey"],
        "challengePointPriorities": {
            "priorities": ["eventPointsFirst", "eventItemsFirst"],
            "objective": "lexicographicExpected", "primary": "challengePoints",
            "lotteryFree": true,
        },
        "combinedMetric": {
            "kind": "combined", "field": "metric.levels", "objective": "lexicographicWeightedExpectedSums",
            "termMetrics": "metrics", "weights": "integer",
            "maxLevels": MAX_METRIC_LEVELS, "maxTerms": MAX_METRIC_TERMS, "maxWeight": MAX_TERM_WEIGHT,
            "consumption": "metric.consumption", "lotteryFree": true,
        },
        "eventItemRewards": {
            "selection": "exactResultGrade", "eventGroupField": "eventGroup",
            "rowsPerGrade": 1, "probabilityMarker": 10000,
        },
        "eventRewardProjection": {
            "field": "eventContext.rewardProjection", "metrics": ["eventPoints", "challengePoints", "eventItems"],
            "balanceValidation": false, "cumulativeRewards": false,
        },
        "eventMusicRanking": {
            "goal": "challengeLive", "metric": "score", "key": "challengeMusicId",
            "record": "maximum solo score across difficulties",
            "optimization": "expected solo score for the selected challenge song and difficulty",
            "serverRankPrediction": false,
        },
    })
}

#[cfg(test)]
#[path = "../tests/fixtures/request_budget.rs"]
mod request_budget;

#[cfg(test)]
#[path = "../tests/fixtures/account_bounds.rs"]
mod account_bounds;

#[cfg(test)]
mod tests;
