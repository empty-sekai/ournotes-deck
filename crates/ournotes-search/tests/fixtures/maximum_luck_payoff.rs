//! Terminal payoff certificates compared with an independent complete-support execution route.
use super::common::{replace_table, set_column};
use super::{EVENT_ID, SCORE_ID, data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Aggregation, Execution, Metric, PlayPolicy, RecommendationOutcome, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, live::model::JudgementStream};
use serde_json::json;

fn inputs() -> (DeckData, Roster, RecommendationRequest) {
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_gekisouMission1"] = json!(2));
    set_column(&mut synth, "MasterLiveSettings", &mut |row| {
        if matches!(row["_key"].as_str(), Some("gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush")) {
            row["_value"] = json!("10");
        }
    });
    replace_table(&mut synth, "MasterSupportSkillEffect", json!([]));
    replace_table(&mut synth, "MasterGekisouSkillEffect", json!([]));
    set_column(&mut synth, "MasterLiveGekisouLuckBonusLot", &mut |row| {
        row["_weight"] = json!(if matches!(row["_lotResult"].as_i64(), Some(0 | 3)) { 1 } else { 0 });
    });
    let mut document = data_document(&synth, 5, 0, 5);
    document["charts"][0]["skillEvents"]["timeMs"] = json!([]);
    document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let mut request = joint_request("mission", true, json!({"kind":"score"}));
    request.execution = Execution::Live {
        score_id: SCORE_ID,
        gekisou: true,
        play: PlayPolicy::Stream { stream: JudgementStream::theoretical_best(&data.chart(SCORE_ID).unwrap()) },
    };
    request.aggregation = Aggregation::Maximum;
    request.constraints.no_snaps = true;
    request.strategy = Strategy::Exhaustive;
    request.k = 1;
    request.limits.cache_entries = 0;
    (data, roster, request)
}

fn complete_support(data: &DeckData, roster: &Roster, request: &RecommendationRequest) -> RecommendationOutcome {
    // An unselected effect makes the optional score-envelope compiler inapplicable. The native model
    // and its reachable outcomes are identical, and every order therefore enumerates its full support.
    let mut full = data.clone();
    full.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":9901,"_supportSkillID":9901,"_level":1,
            "_skillTriggerType":1,"_skillEffectType":11000,"_effectValue":1000,
        }))
        .unwrap(),
    );
    full.master.reindex().unwrap();
    assert!(ournotes_sim::live::full::luck_skills(&full.master).is_err());
    let result = engine::recommend(&full, roster, request).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    result
}

#[test]
fn luck_score_target_and_constant_life_certificates_preserve_full_support_results() {
    let (data, roster, mut request) = inputs();
    let score_reference = complete_support(&data, &roster, &request);
    let maximum = score_reference.results[0].maximum_score.unwrap();
    let mut metrics = vec![Metric::Score];
    for threshold in [1, maximum - 1, maximum, maximum + 1] {
        metrics.push(Metric::ScoreAtLeast { threshold });
        metrics.push(Metric::CappedScore { threshold });
        metrics.push(Metric::ScoreAndLifeAtLeast { threshold, min_final_life: 1 });
        metrics.push(Metric::ScoreAndLifeAtLeast { threshold, min_final_life: 10_000 });
    }
    for metric in metrics {
        request.metric = metric;
        let reference = complete_support(&data, &roster, &request);
        let actual = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(actual.completion, Completion::Complete, "{:?}", request.metric);
        assert_eq!(actual.results, reference.results, "{:?}", request.metric);
        assert!(actual.telemetry.leaves.simulations < reference.telemetry.leaves.simulations, "{:?}", request.metric);
        assert_eq!(actual.results[0].maximum_score, Some(maximum));
    }
}

#[test]
fn native_luck_payoffs_preserve_nonmonotone_rewards_and_separate_score_maxima() {
    let (mut data, roster, mut request) = inputs();
    let maximum = complete_support(&data, &roster, &request).results[0].maximum_score.unwrap();
    for row in &mut data.master.live_score_ranks {
        row.required_score = match row.live_score_rank {
            2 => 0,
            3 => i64::from(maximum) - 1,
            4 => i64::from(maximum),
            _ => i64::from(maximum) + 1,
        };
        row.battle_live_required_score = row.required_score;
    }
    for monotone in [true, false] {
        for row in &mut data.master.live_event_points {
            row.value = if monotone {
                row.score_rank * 10
            } else if row.score_rank == 4 {
                1
            } else {
                100
            };
        }
        for row in &mut data.master.live_challenge_points {
            row.value = if monotone {
                row.score_rank * 10
            } else if row.score_rank == 4 {
                1
            } else {
                100
            };
        }
        data.master.reindex().unwrap();
        for metric in [
            Metric::ClientEventPoints { event_id: EVENT_ID },
            Metric::ClientChallengePoints { event_id: EVENT_ID },
            Metric::ConditionalClientEventItems { event_id: EVENT_ID, resource_type: 11, resource_id: 9 },
        ] {
            request.metric = metric;
            let reference = complete_support(&data, &roster, &request);
            let actual = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(actual.completion, Completion::Complete, "{:?}", request.metric);
            assert_eq!(actual.results, reference.results, "{:?}", request.metric);
            assert_eq!(actual.results[0].maximum_score, Some(maximum));
            if monotone || matches!(request.metric, Metric::ConditionalClientEventItems { .. }) {
                assert!(
                    actual.telemetry.leaves.simulations < reference.telemetry.leaves.simulations,
                    "{:?}",
                    request.metric
                );
            } else {
                assert!(
                    actual.results[0].best_order.as_ref().unwrap().score < maximum,
                    "the payoff maximum needs a lower-score path"
                );
            }
        }
    }
}
