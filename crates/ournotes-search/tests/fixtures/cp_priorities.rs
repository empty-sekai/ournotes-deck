//! Lexicographic event rewards over independently evaluated complete synthetic domains.
use super::{
    EVENT_ID, SCORE_ID, account_recommendation_fixture, account_request, context_document, data_document,
    joint_request_json, replace_table, roster_document, set_column, synthetic_master,
};
use ournotes_search::{
    auxiliary::evaluate_built,
    engine,
    handler::build_card_pool,
    recommendation::{Status, capabilities},
    search::Completion,
    types::{EventRewardPriority, Fraction, Metric, Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::{Value, json};

fn inputs() -> (DeckData, Roster) {
    let mut synth = synthetic_master(6, 1, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_characterID"] = if row["_id"] == 6 { json!(1) } else { row["_id"].clone() };
    });
    set_column(&mut synth, "MasterLiveChallengePoint", &mut |row| row["_value"] = json!(9));
    set_column(&mut synth, "MasterLiveEventPoint", &mut |row| row["_value"] = json!(100));
    set_column(&mut synth, "MasterLiveEventReward", &mut |row| row["_resourceCount"] = json!(10));
    replace_table(
        &mut synth,
        "MasterEventEffect",
        json!([
            {"_id":1,"_eventId":EVENT_ID,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":1,
             "_rank1EffectValue":10000,"_rank2EffectValue":10000,"_rank3EffectValue":10000,
             "_rank4EffectValue":10000,"_rank5EffectValue":10000},
            {"_id":2,"_eventId":EVENT_ID,"_eventBonusType":1,"_resourceTypeConstraint":2,"_memberCardId":6,
             "_rank1EffectValue":20000,"_rank2EffectValue":20000,"_rank3EffectValue":20000,
             "_rank4EffectValue":20000,"_rank5EffectValue":20000}
        ]),
    );
    (
        DeckData::from_json(&data_document(&synth, 6, 1, 5).to_string()).unwrap(),
        Roster::from_json(&roster_document(6, 1, 5).to_string()).unwrap(),
    )
}

fn request(skip: bool) -> RecommendationRequest {
    let mut value = joint_request_json("free", false, json!({"kind":"clientChallengePoints","eventId":EVENT_ID}));
    if skip {
        value["execution"] = json!({"kind":"skip","scoreId":SCORE_ID});
        value["context"] = context_document(true, false, false);
    }
    value["k"] = json!(100);
    value["strategy"] = json!({"kind":"exhaustive"});
    serde_json::from_value(value).unwrap()
}

fn numerator(value: &Fraction) -> i128 {
    value.numerator.parse().unwrap()
}

struct Reference {
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    cp: Fraction,
    pt: Fraction,
    items: Fraction,
}

#[test]
fn complete_cp_layer_matches_independent_point_and_item_evaluations() {
    let (data, roster) = inputs();
    for skip in [false, true] {
        let primary = request(skip);
        let complete = engine::recommend(&data, &roster, &primary).unwrap();
        assert_eq!(complete.completion, Completion::Complete);
        assert_eq!(complete.results.len(), 12);
        let best_cp = numerator(complete.results[0].expected_payoff.as_ref().unwrap());
        let mut points = primary.clone();
        points.metric = Metric::ClientEventPoints { event_id: EVENT_ID };
        let point_problem = build_card_pool(&data, &roster, &points).unwrap();
        let mut items = primary.clone();
        items.metric = Metric::RankedEventItems { event_id: EVENT_ID, resource_type: 11, resource_id: 9 };
        let item_problem = build_card_pool(&data, &roster, &items).unwrap();
        let mut expected = complete
            .results
            .iter()
            .filter(|row| numerator(row.expected_payoff.as_ref().unwrap()) == best_cp)
            .map(|row| {
                let pt = evaluate_built(&point_problem, row.members, row.snaps).unwrap();
                let items = evaluate_built(&item_problem, row.members, row.snaps).unwrap();
                Reference {
                    members: row.members,
                    snaps: row.snaps,
                    power: row.power,
                    cp: row.expected_payoff.clone().unwrap(),
                    pt: pt.results[0].expected_payoff.clone().unwrap(),
                    items: items.results[0].expected_payoff.clone().unwrap(),
                }
            })
            .collect::<Vec<_>>();
        for priority in [EventRewardPriority::EventPointsFirst, EventRewardPriority::EventItemsFirst] {
            let key = |row: &Reference| match priority {
                EventRewardPriority::EventPointsFirst => [numerator(&row.pt), numerator(&row.items)],
                EventRewardPriority::EventItemsFirst => [numerator(&row.items), numerator(&row.pt)],
            };
            expected.sort_by(|a, b| {
                key(b)
                    .cmp(&key(a))
                    .then(b.power.cmp(&a.power))
                    .then(a.members.cmp(&b.members))
                    .then(a.snaps.cmp(&b.snaps))
            });
            let mut combined = primary.clone();
            combined.metric = Metric::ClientChallengePointsWithBonuses {
                event_id: EVENT_ID,
                priority,
                resource_type: 11,
                resource_id: 9,
            };
            combined.k = 5;
            for (strategy, cache_entries) in
                [(Strategy::Exhaustive, 0), (Strategy::BranchAndBound, 0), (Strategy::BranchAndBound, 64)]
            {
                combined.strategy = strategy;
                combined.limits.cache_entries = cache_entries;
                let actual = engine::recommend(&data, &roster, &combined).unwrap();
                assert_eq!(actual.completion, Completion::Complete);
                assert_eq!(actual.optimality, Optimality::Proven);
                assert_eq!(actual.results.len(), expected.len().min(5));
                for (row, expected) in actual.results.iter().zip(&expected) {
                    assert_eq!((row.members, row.snaps, row.power), (expected.members, expected.snaps, expected.power));
                    assert_eq!(row.expected_payoff.as_ref().unwrap(), &expected.cp);
                    let rewards = row.event_rewards.as_ref().expect("exact reward expectations");
                    assert_eq!(rewards.challenge_points, expected.cp);
                    assert_eq!(rewards.event_points, expected.pt);
                    assert_eq!(rewards.event_items, expected.items);
                }
                let preferred = match priority {
                    EventRewardPriority::EventPointsFirst => 1,
                    EventRewardPriority::EventItemsFirst => 6,
                };
                assert!(actual.results[0].members.contains(&preferred));
            }
            combined.limits.max_candidates = Some(1);
            let stopped = engine::recommend(&data, &roster, &combined).unwrap();
            assert_eq!(stopped.completion, Completion::TimedOut);
            assert_eq!(stopped.optimality, Optimality::Unproven);
            assert!(stopped.telemetry.proof.best_gap.is_none());
            assert!(stopped.telemetry.proof.kth_gap.is_none());
            for row in stopped.results {
                assert!(
                    expected.iter().any(|candidate| candidate.members == row.members && candidate.snaps == row.snaps)
                );
                assert!(row.event_rewards.is_some());
            }
        }
    }
}

fn account_priority(priority: &str) -> Value {
    let mut request = account_request(json!({"kind":"freeLive","musicId":10,"difficulty":"expert"}));
    request["metric"] = json!({"kind":"challengePoints","eventId":EVENT_ID,"consumption":1,
        "secondaryPriority":priority,"resourceType":11,"resourceId":9});
    request["eventContext"] = json!({"rewardProjection":true,
        "resultClock":{"kind":"played","serverNowJstTicks":150},
        "eventWindows":[{"eventId":EVENT_ID,"startJstTicks":100,"endJstTicks":200}]});
    request
}

#[test]
fn account_priority_contract_echoes_exact_rewards_and_rejects_incomplete_fields() {
    let (data, account) = account_recommendation_fixture();
    let capabilities = capabilities();
    assert_eq!(
        capabilities["challengePointPriorities"],
        json!({
            "priorities":["eventPointsFirst","eventItemsFirst"],"objective":"lexicographicExpected",
            "primary":"challengePoints","bestPrimaryOnly":true,"lotteryFree":true
        })
    );
    for priority in ["eventPointsFirst", "eventItemsFirst"] {
        let request = account_priority(priority);
        let answer = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
        assert!(matches!(answer.status, Status::Ok), "{:?}", answer.errors);
        let value = serde_json::to_value(answer).unwrap();
        assert_eq!(value["result"]["metric"]["secondaryPriority"], priority);
        assert_eq!(value["result"]["metric"]["rewardProjection"], true);
        let teams = value["result"]["teams"].as_array().unwrap();
        assert!(!teams.is_empty() && teams.len() <= 5);
        for team in teams {
            assert_eq!(team["eventRewards"]["challengePoints"]["exact"], team["value"]["payoff"]["exact"]);
            for resource in ["challengePoints", "eventPoints", "eventItems"] {
                assert!(team["eventRewards"][resource]["score"].is_number());
                assert!(team["eventRewards"][resource]["exact"]["numerator"].is_string());
                assert!(team["eventRewards"][resource]["interval"].is_null());
            }
        }
    }
    let mut invalid = Vec::new();
    let mut request = account_priority("unknown");
    invalid.push((request, "metric.secondaryPriority"));
    for field in ["resourceType", "resourceId"] {
        request = account_priority("eventPointsFirst");
        request["metric"].as_object_mut().unwrap().remove(field);
        invalid.push((request, if field == "resourceType" { "metric.resourceType" } else { "metric.resourceId" }));
    }
    request = account_priority("eventPointsFirst");
    request["metric"]["kind"] = json!("eventPoints");
    invalid.push((request, "metric.secondaryPriority"));
    for (request, path) in invalid {
        let answer = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
        assert!(matches!(answer.status, Status::Invalid));
        assert!(answer.errors.iter().any(|issue| issue.path == path), "{path}: {:?}", answer.errors);
    }
}

#[test]
fn primary_layer_excludes_lower_cp_even_when_k_has_space() {
    let (mut data, roster) = inputs();
    let mut all = request(false);
    all.metric = Metric::Score;
    let scores = engine::recommend(&data, &roster, &all).unwrap();
    let maximum = scores.results.iter().flat_map(|team| &team.order_outcomes).map(|order| order.1).max().unwrap();
    data.master.live_score_ranks.truncate(2);
    data.master.live_score_ranks[1].required_score = i64::from(maximum);
    data.master.live_score_ranks[1].battle_live_required_score = i64::from(maximum);
    for row in &mut data.master.live_challenge_points {
        row.value = if row.score_rank == 2 { 0 } else { 1 };
    }
    all.metric = Metric::ClientChallengePoints { event_id: EVENT_ID };
    let reference = engine::recommend(&data, &roster, &all).unwrap();
    let best = reference.results[0].expected_payoff.as_ref().unwrap();
    let expected =
        reference.results.iter().filter(|team| team.expected_payoff.as_ref() == Some(best)).collect::<Vec<_>>();
    assert!(!expected.is_empty() && expected.len() < reference.results.len());
    for priority in [EventRewardPriority::EventPointsFirst, EventRewardPriority::EventItemsFirst] {
        all.metric = Metric::ClientChallengePointsWithBonuses {
            event_id: EVENT_ID,
            priority,
            resource_type: 11,
            resource_id: 9,
        };
        for strategy in [Strategy::Exhaustive, Strategy::BranchAndBound] {
            all.strategy = strategy;
            let result = engine::recommend(&data, &roster, &all).unwrap();
            assert_eq!(result.optimality, Optimality::Proven);
            assert_eq!(result.results.len(), expected.len());
            assert!(result.results.len() < all.k);
            for team in result.results {
                assert_eq!(team.expected_payoff.as_ref(), Some(best));
                assert!(expected.iter().any(|row| row.members == team.members && row.snaps == team.snaps));
            }
        }
    }
}

#[test]
fn played_priorities_require_proven_lottery_free_conditions() {
    let (mut data, roster) = inputs();
    let mut request = request(false);
    request.metric = Metric::ClientChallengePointsWithBonuses {
        event_id: EVENT_ID,
        priority: EventRewardPriority::EventItemsFirst,
        resource_type: 11,
        resource_id: 9,
    };
    data.master.live_musics[0].gekisou_mission_1 = 2;
    // Static mission metadata leaves the Free Live's actual play deterministic.
    assert_eq!(engine::recommend(&data, &roster, &request).unwrap().optimality, Optimality::Proven);
    data.master.skill_conditions.push(
        serde_json::from_value(json!({
            "_id":9001,"_conditionType":4011,"_conditionValues":[50],"_isPositive":true
        }))
        .unwrap(),
    );
    data.master.skill_condition_sets.push(
        serde_json::from_value(json!({
            "_id":9001,"_group":9001,"_conditionIds":[9001]
        }))
        .unwrap(),
    );
    for rank in &mut data.master.support_card_ranks {
        rank.support_skill_01_level = 1;
    }
    let skill_id = data.master.support_cards[0].support_skill_id_01;
    data.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":9001,"_supportSkillID":skill_id,"_level":1,"_skillTriggerType":1,
            "_activationTimeSecond":5.0,"_skillTriggerConditionGroup":53,
            "_skillConditionGroup":9001,"_skillEffectType":2000,"_effectValue":10000,
            "_skillTargetIDs":[]
        }))
        .unwrap(),
    );
    data.master.reindex().unwrap();
    let error = engine::recommend(&data, &roster, &request).unwrap_err();
    assert!(matches!(error, ournotes_sim::Error::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("ordinary skill conditions"), "{error}");
}

#[test]
fn active_luck_ranges_reject_reward_priorities() {
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_gekisouMission1"] = json!(2));
    let mut document = data_document(&synth, 5, 0, 5);
    document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let mut request = super::joint_request("mission", true, json!({"kind":"clientChallengePoints","eventId":EVENT_ID}));
    request.metric = Metric::ClientChallengePointsWithBonuses {
        event_id: EVENT_ID,
        priority: EventRewardPriority::EventPointsFirst,
        resource_type: 11,
        resource_id: 9,
    };
    let error = engine::recommend(&data, &roster, &request).unwrap_err();
    assert!(matches!(error, ournotes_sim::Error::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("lottery-free terminal outcomes"), "{error}");
}
