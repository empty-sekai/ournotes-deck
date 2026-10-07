//! Independent complete-order oracle and transport coverage for the best conditional score objective.
use super::common::set_column;
use super::{
    account_recommendation_data, account_recommendation_fixture, account_request, data_document, joint_request,
    roster_document, synthetic_master,
};
use ournotes_search::{
    auxiliary, engine, handler,
    search::Completion,
    types::{Metric, Optimality, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

#[test]
fn best_order_search_matches_every_leader_and_snap_binding_from_native_order_outcomes() {
    let mut source = synthetic_master(5, 1, 5);
    set_column(&mut source, "MasterLiveSkillEffect", &mut |row| row["_activationTimeSecond"] = json!(0.24));
    let mut document = data_document(&source, 5, 1, 5);
    document["charts"][0]["skillEvents"]["timeMs"] = json!([160, 500, 750, 1000, 1250]);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 1, 5).to_string()).unwrap();
    let mut request = joint_request("free", false, json!({"kind":"score"}));
    request.constraints.leader = None;
    request.strategy = Strategy::Exhaustive;
    let built = handler::build_card_pool(&data, &roster, &request).unwrap();
    let mut oracle = Vec::new();
    let mut varies = false;
    for leader in 1..=5 {
        let others: Vec<_> = (1..=5).filter(|&id| id != leader).collect();
        let members = [others[0], others[1], leader, others[2], others[3]];
        for binding in 0..=5 {
            let mut snaps = [None; 5];
            if binding < 5 {
                snaps[binding] = Some(1);
            }
            let fixed = auxiliary::evaluate_built(&built, members, snaps).unwrap();
            let deck = &fixed.results[0];
            assert_eq!(deck.order_outcomes.len(), 120);
            assert!(deck.best_expected_order.is_none());
            let maximum = deck.order_outcomes.iter().map(|row| row.1).max().unwrap();
            let order = deck.order_outcomes.iter().filter(|row| row.1 == maximum).map(|row| row.0).min().unwrap();
            varies |= deck.order_outcomes.iter().any(|row| row.1 != maximum);
            oracle.push((maximum, deck.power, members, snaps, order));
        }
    }
    assert!(varies, "different performer positions must affect at least one team");
    oracle.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
    request.metric = Metric::BestOrderExpectedScore;
    for (strategy, cache, k) in
        [(Strategy::Exhaustive, 0, 30), (Strategy::BranchAndBound, 0, 1), (Strategy::BranchAndBound, 64, 7)]
    {
        request.strategy = strategy;
        request.limits.cache_entries = cache;
        request.k = k;
        let answer = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(answer.completion, Completion::Complete);
        assert_eq!(answer.results.len(), k);
        assert_eq!(answer.probability_law["kind"], "bestMemberOrderExpectedScore");
        for (deck, expected) in answer.results.iter().zip(&oracle) {
            assert_eq!((deck.power, deck.members, deck.snaps), (expected.1, expected.2, expected.3));
            let score = deck.expected_score.as_ref().unwrap();
            assert_eq!(
                score.numerator.parse::<i128>().unwrap(),
                i128::from(expected.0) * score.denominator.parse::<i128>().unwrap()
            );
            let interval = deck.score_interval.as_ref().unwrap();
            assert!(interval.lower_f64() <= f64::from(expected.0) && f64::from(expected.0) <= interval.upper_f64());
            let witness = deck.best_expected_order.as_ref().unwrap();
            assert_eq!(witness.performance_order, expected.4);
            assert_eq!(witness.members, expected.4.map(|slot| deck.members[slot]));
            assert_eq!(witness.optimality, Optimality::Proven);
            assert!((1..=120).contains(&witness.evaluated_orders));
        }
    }
    let built = handler::build_card_pool(&data, &roster, &request).unwrap();
    let fixed = auxiliary::evaluate_built(&built, oracle[0].2, oracle[0].3).unwrap();
    assert_eq!(fixed.optimality, Optimality::NotApplicable);
    assert_eq!(fixed.results[0].best_expected_order.as_ref().unwrap().performance_order, oracle[0].4);
    request.limits.max_candidates = Some(1);
    request.strategy = Strategy::Exhaustive;
    let interrupted = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(interrupted.completion, Completion::TimedOut);
    assert_eq!(interrupted.optimality, Optimality::Unproven);
    assert_eq!(interrupted.results[0].best_expected_order.as_ref().unwrap().optimality, Optimality::Proven);
}

#[test]
fn best_order_account_and_native_json_preserve_the_objective_and_reject_incompatible_fields() {
    let (data, account) = account_recommendation_fixture();
    let mut request = account_request(json!({"kind":"freeLive","musicId":10,"difficulty":"expert"}));
    request["metric"] = json!({"kind":"bestOrderExpectedScore"});
    let answer = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
    let json = serde_json::to_value(answer).unwrap();
    assert_eq!(json["status"], "ok", "{json}");
    assert_eq!(json["result"]["metric"]["kind"], "bestOrderExpectedScore");
    let witness = &json["result"]["teams"][0]["bestExpectedOrder"];
    assert_eq!(witness["optimality"], "proven");
    assert!(witness["expectedScore"].is_object());
    assert!(witness["scoreInterval"]["lower"]["numerator"].is_string());
    let caps = engine::capabilities();
    assert!(caps["metrics"]["freeLive"].as_array().unwrap().contains(&json!("bestOrderExpectedScore")));
    assert!(!caps["metrics"]["skip"].as_array().unwrap().contains(&json!("bestOrderExpectedScore")));
    for invalid in [
        json!({"kind":"bestOrderExpectedScore","threshold":1}),
        json!({"kind":"bestOrderExpectedScore","eventId":7}),
        json!({"kind":"bestOrderExpectedScore","aggregation":"maximum"}),
    ] {
        request["metric"] = invalid;
        let rejected = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
        assert!(rejected.result.is_none());
        assert!(!rejected.errors.is_empty());
    }
    request["metric"] = json!({"kind":"bestOrderExpectedScore"});
    request["goal"]["kind"] = json!("skip");
    let rejected = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
    assert!(rejected.result.is_none());
    assert!(rejected.errors.iter().any(|issue| issue.path == "metric.kind"));
    let raw = json!({"format":"ournotes-deck.search-request/1", "execution":{"kind":"skip","scoreId":1004},
        "scenario":{"kind":"free","musicId":10},"metric":{"kind":"bestOrderExpectedScore"}});
    let parsed = serde_json::from_value(raw).unwrap();
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    assert!(handler::build_card_pool(&data, &roster, &parsed).is_err());
}

#[test]
fn best_order_account_ties_use_the_returned_canonical_layout_with_leader_at_slot_two() {
    let mut document = account_recommendation_data();
    document["charts"][0]["skillEvents"]["timeMs"] = json!([]);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let (_, mut account) = account_recommendation_fixture();
    account["datasetId"] = json!(data.sha256);
    let mut request = account_request(json!({"kind":"freeLive","musicId":10,"difficulty":"expert"}));
    request["constraints"]["leader"] = json!(6);
    request["k"] = json!(1);
    let uniform =
        serde_json::to_value(engine::recommend_account(&data, &account.to_string(), &request.to_string(), None))
            .unwrap();
    assert_eq!(uniform["status"], "ok", "{uniform}");
    let scores = uniform["result"]["teams"][0]["orders"]["values"].as_array().unwrap();
    assert_eq!(scores.len(), 120);
    assert!(scores.iter().all(|score| score == &scores[0]), "fixture must tie every conditional order");
    request["metric"] = json!({"kind":"bestOrderExpectedScore"});
    let answer =
        serde_json::to_value(engine::recommend_account(&data, &account.to_string(), &request.to_string(), None))
            .unwrap();
    assert_eq!(answer["status"], "ok", "{answer}");
    let team = &answer["result"]["teams"][0];
    assert_eq!(team["layout"]["members"][2], 6);
    assert_eq!(team["bestExpectedOrder"]["performanceOrder"], json!([0, 1, 2, 3, 4]));
    assert_eq!(team["bestExpectedOrder"]["members"], team["layout"]["members"]);
    assert_eq!(team["bestExpectedOrder"]["optimality"], "proven");
    assert!(
        engine::capabilities()["bestOrderExpectedScore"]["orderTieBreak"]
            .as_str()
            .unwrap()
            .contains("leader at slot 2")
    );
}
