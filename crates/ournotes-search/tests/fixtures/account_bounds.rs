//! Account-level upper bounds must cover the whole domain, not just the branches still open.
use super::{Exclusions, GoalKind, Parsed, result_of};
use crate::{
    clock::Instant,
    search::{Completion, telemetry::Target},
    types::{ExitReason, FractionInterval, Metric, Optimality, RecommendationOutcome, RecommendedDeck, Strategy},
};
use serde_json::{Value, json};

fn fixture() -> (Parsed, RecommendationOutcome) {
    let search = serde_json::from_value(json!({
        "format":crate::types::REQUEST_FORMAT,
        "execution":{"kind":"live","scoreId":1,"gekisou":true,"play":{"kind":"theoreticalBest"}},
        "metric":{"kind":"score"},"k":1
    }))
    .unwrap();
    let parsed = Parsed {
        kind: GoalKind::MissionLive,
        goal: Value::Null,
        metric: None,
        exclusions: Exclusions::default(),
        k: 1,
        search,
    };
    let mut outcome = RecommendationOutcome {
        format: crate::types::RESULT_FORMAT,
        completion: Completion::TimedOut,
        optimality: Optimality::Unproven,
        exit_reason: ExitReason::TimeLimit,
        result_identity: "team",
        metric: Metric::Score,
        player_goal: None,
        strategy: Strategy::BranchAndBound,
        probability_law: json!({"kind":"uniformMemberOrder","orders":120,"lottery":"certifiedNativeLotteryIntervals"}),
        proof_scope: "synthetic wire semantics",
        resolved_context: Value::Null,
        telemetry: Default::default(),
        elapsed_ms: 0.0,
        results: vec![RecommendedDeck {
            members: [1, 2, 3, 4, 5],
            snaps: [None; 5],
            power: 100,
            expected_score: None,
            expected_payoff: None,
            event_rewards: None,
            term_payoffs: None,
            score_interval: Some(FractionInterval::from_f64(150.0, 151.0).unwrap()),
            payoff_interval: Some(FractionInterval::from_f64(150.0, 151.0).unwrap()),
            rank_certified: Some(false),
            score_summary: None,
            best_order: None,
            order_outcomes: vec![],
        }],
    };
    outcome.telemetry.environment.target = Some(Target { orders: 120, denominator: "120".into() });
    // Unexplored candidates pay at most 100, while one evaluated candidate pays at least 150.
    outcome.telemetry.proof.upper_bound = Some("12000".into());
    (parsed, outcome)
}

#[test]
fn open_branch_bound_never_becomes_an_account_global_bound() {
    let (parsed, outcome) = fixture();
    for is_final in [false, true] {
        let answer = result_of(&outcome, &parsed, &Value::Null, Instant::now(), is_final).unwrap();
        assert_eq!(answer.optimality.lower_bound, Some(json!(150)));
        assert_eq!(answer.optimality.upper_bound, None);
        assert!(!answer.optimality.proven);
        // Keep the scoped diagnostic instead of relabeling or discarding it.
        assert_eq!(answer.telemetry["proof"]["upperBound"], "12000");
    }
}

#[test]
fn explicit_global_bound_keeps_its_target_denominator() {
    let (parsed, mut outcome) = fixture();
    outcome.telemetry.proof.global_upper_bound = Some("24000".into());
    let answer = result_of(&outcome, &parsed, &Value::Null, Instant::now(), true).unwrap();
    assert_eq!(answer.optimality.lower_bound, Some(json!(150)));
    assert_eq!(answer.optimality.upper_bound, Some(json!(200)));
    assert!(!answer.optimality.proven);
}

#[test]
fn certified_optimum_keeps_the_winners_interval_without_a_scalar_global_bound() {
    let (parsed, mut outcome) = fixture();
    outcome.completion = Completion::Complete;
    outcome.optimality = Optimality::Proven;
    outcome.exit_reason = ExitReason::Exhausted;
    outcome.results[0].rank_certified = Some(true);
    outcome.telemetry.proof.upper_bound = None;
    let answer = result_of(&outcome, &parsed, &Value::Null, Instant::now(), true).unwrap();
    assert!(answer.optimality.proven);
    assert_eq!(answer.optimality.lower_bound, Some(json!(150)));
    assert_eq!(answer.optimality.upper_bound, Some(json!(151)));
}
