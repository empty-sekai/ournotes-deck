//! Exact item ranking preserves each selected team's independent score and order objective.
use super::common::{replace_table, set_column};
use super::{EVENT_ID, correctness_matrix, data_document, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Aggregation, ExitReason, Metric, Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

fn fixture() -> (DeckData, Roster) {
    let mut synth = synthetic_master(6, 1, 6);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        if row["_id"] == 6 {
            for field in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                row[field] = json!(100);
            }
        }
    });
    replace_table(
        &mut synth,
        "MasterEventEffect",
        json!([{"_id":1,"_eventId":EVENT_ID,"_eventBonusType":1,"_resourceTypeConstraint":2,
            "_memberCardId":6,"_rank1EffectValue":30000,"_rank2EffectValue":30000,
            "_rank3EffectValue":30000,"_rank4EffectValue":30000,"_rank5EffectValue":30000}]),
    );
    set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |row| {
        row["_scoreBonusPercent"] = json!(match row["_count"].as_i64().unwrap() {
            1 => 250,
            2 => 125,
            _ => 0,
        });
    });
    let data = DeckData::from_json(&data_document(&synth, 6, 1, 6).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 1, 6).to_string()).unwrap();
    (data, roster)
}

fn request(data: &DeckData, scene: &str) -> RecommendationRequest {
    let mut request = correctness_matrix::request(
        data,
        scene,
        false,
        json!({"kind":"conditionalClientEventItems","eventId":EVENT_ID,"resourceType":11,"resourceId":9}),
    );
    request.aggregation = Aggregation::Maximum;
    request
}

#[test]
fn maximum_item_ranking_materializes_full_scores_and_order_ties() {
    let (data, roster) = fixture();
    for scene in ["free"] {
        let mut current = request(&data, scene);
        current.strategy = Strategy::Exhaustive;
        current.k = 100;
        let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
        assert_eq!(exhaustive.completion, Completion::Complete);
        assert_eq!(exhaustive.results.len(), 30);
        assert!(exhaustive.results[..5].iter().all(|team| team.members.contains(&6)));
        assert!(
            exhaustive
                .results
                .iter()
                .any(|team| !team.members.contains(&6) && team.maximum_score > exhaustive.results[0].maximum_score)
        );
        current.strategy = Strategy::BranchAndBound;
        for (k, cache) in [(1, 0), (3, 32), (5, 32), (100, 0)] {
            current.k = k;
            current.limits.cache_entries = cache;
            let result = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(result.completion, Completion::Complete, "{scene} k={k}");
            assert_eq!(result.optimality, Optimality::Proven);
            assert_eq!(result.results, exhaustive.results[..k.min(30)], "{scene} k={k} cache={cache}");
            assert_eq!(result.telemetry.leaves.evaluated, k.min(30) as u64);
            assert_eq!(result.telemetry.environment.target.as_ref().unwrap().denominator, "1");
            for team in &result.results {
                assert!(team.expected_payoff.is_none() && team.expected_score.is_none());
                let best = team.best_order.as_ref().expect("the best score on the constant-payoff support");
                assert_eq!(Some(best.score), team.maximum_score);
                assert_eq!(best.payoff, team.objective_value.as_ref().unwrap().numerator);
                assert_eq!(team.objective_value.as_ref().unwrap().denominator, "1");
            }
        }
    }
}

#[test]
fn maximum_item_stop_bounds_keep_single_payoff_units() {
    let (data, roster) = fixture();
    let mut current = request(&data, "free");
    current.k = 5;
    current.limits.max_candidates = Some(2);
    let maximum = engine::recommend(&data, &roster, &current).unwrap();
    current.aggregation = Aggregation::Expected;
    let expected = engine::recommend(&data, &roster, &current).unwrap();
    for result in [&maximum, &expected] {
        assert_eq!(result.completion, Completion::TimedOut);
        assert_eq!(result.exit_reason, ExitReason::CandidateLimit);
        assert_eq!(result.optimality, Optimality::Unproven);
        assert_eq!(result.results.len(), 2);
        assert!(!result.telemetry.proof.complete);
    }
    let numerator = |value: &Option<String>| value.as_ref().unwrap().parse::<i128>().unwrap();
    assert_eq!(
        numerator(&expected.telemetry.proof.global_upper_bound),
        120 * numerator(&maximum.telemetry.proof.global_upper_bound)
    );
    assert_eq!(maximum.telemetry.environment.target.as_ref().unwrap().denominator, "1");
    assert_eq!(expected.telemetry.environment.target.as_ref().unwrap().denominator, "120");
    current.aggregation = Aggregation::Maximum;
    current.limits.max_candidates = None;
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(maximum.results, exhaustive.results[..2]);
    assert!(
        numerator(&maximum.telemetry.proof.global_upper_bound)
            >= exhaustive.results[0].objective_value.as_ref().unwrap().numerator.parse::<i128>().unwrap()
    );
}

#[test]
fn maximum_score_steps_and_unavailable_item_bounds_keep_full_search() {
    let (mut data, roster) = fixture();
    let mut current = request(&data, "free");
    current.metric = Metric::ScoreAtLeast { threshold: 450_000 };
    current.k = 5;
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    current.strategy = Strategy::BranchAndBound;
    let bounded = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(bounded.completion, Completion::Complete);
    assert_eq!(bounded.results, exhaustive.results);
    let steps = bounded.telemetry.environment.bounds.deck_payoff.as_ref().expect("score-step enclosure");
    assert!(steps.score_cap.is_some());
    assert_eq!(steps.rounds, 0, "a score bound is not an exact deck payoff");

    // Signed bonuses are legal terminal inputs, but the monotone deck bound must decline them.
    for effect in &mut data.master.event_effects {
        effect.rank3_effect_value = -1000;
    }
    data.master.reindex().unwrap();
    current = request(&data, "free");
    current.k = 5;
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    current.strategy = Strategy::BranchAndBound;
    let fallback = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(fallback.completion, Completion::Complete);
    assert_eq!(fallback.results, exhaustive.results);
    assert!(fallback.telemetry.environment.bounds.fallback.is_some());
    assert_eq!(fallback.telemetry.leaves.evaluated, 30);
}
