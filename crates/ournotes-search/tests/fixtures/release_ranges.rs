//! Gekisou support timers started at a range start and ended by a release checker, over a synthetic 20 ms frame
//! stream. The fixed leader, six cards across five characters and one optional Snap give twelve teams.

use super::common::{extend_table, replace_table, set_column};
use super::{data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Optimality, RecommendationOutcome, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::{Value, json};

/// Release groups: range complete, always true, and one Perfect judged while the release is checked.
const RANGE_COMPLETE: i64 = 9003;
const ALWAYS: i64 = 9004;
const ONE_PERFECT: i64 = 9005;

/// The range starts at 50 and its start is seen in the frame at 60. A Perfect is judged in the frame at 160 and only
/// Greats in the frames at 100 to 140.
fn inputs(act: f64, release: i64) -> (DeckData, Roster, Value) {
    let mut synth = synthetic_master(6, 1, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| row["_gekisouSkillID"] = json!(1));
    set_column(&mut synth, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(0);
        row["_supportSkillId02"] = json!(0);
        row["_gekisouSupportSkillId01"] = json!(2);
        row["_gekisouSupportSkillId02"] = json!(0);
    });
    set_column(&mut synth, "MasterSupportCardRank", &mut |row| row["_gekisouSupportSkill01Level"] = json!(1));
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        for key in ["_gekisouMission1", "_gekisouMission2", "_gekisouMission3"] {
            row[key] = json!(1);
        }
    });
    set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(10));
    replace_table(&mut synth, "MasterGekisouSkill", json!([{"_id":1,"_gekisouMissionType":1}]));
    replace_table(&mut synth, "MasterGekisouSupportSkill", json!([{"_id":2,"_gekisouMissionType":1}]));
    extend_table(
        &mut synth,
        "MasterSkillTarget",
        vec![
            json!({"_id":9001,"_skillTargetType":5,"_gekisouMissionType":1}),
            json!({"_id":9002,"_skillTargetType":4,"_judgement":5}),
        ],
    );
    let condition = |id: i64, kind: i64, values: Value, positive: bool, targets: Value| {
        json!({"_id":id,"_conditionType":kind,"_conditionValues":values,"_isPositive":positive,
            "_conditionTargetIDs":targets})
    };
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        vec![
            condition(9001, 7010, json!([]), true, json!([9001])),
            condition(9003, 7013, json!([]), true, json!([9001])),
            condition(9004, 8000, json!([]), false, json!([])),
            condition(9005, 1030, json!([1]), true, json!([9002])),
        ],
    );
    extend_table(
        &mut synth,
        "MasterSkillConditionSet",
        [9001, 9003, 9004, 9005].into_iter().map(|id| json!({"_id":id,"_group":id,"_conditionIds":[id]})).collect(),
    );
    for table in [
        "MasterLiveSkillEffect",
        "MasterSupportSkillEffect",
        "MasterGekisouSkillEffect",
        "MasterGekisouSupportSkillEffect",
    ] {
        replace_table(&mut synth, table, json!([]));
    }
    replace_table(
        &mut synth,
        "MasterGekisouSupportSkillEffect",
        json!([{"_id":9001,"_gekisouSupportSkillID":2,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":9001,"_skillConditionGroup":0,"_skillReleaseConditionGroup":release,
            "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":act,
            "_effectValue":10000,"_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]),
    );
    let mut document = data_document(&synth, 6, 1, 5);
    // (chart time, judging frame, judgement)
    let notes = [
        (60, 3, 5),
        (80, 4, 5),
        (100, 5, 4),
        (120, 6, 4),
        (140, 7, 4),
        (155, 8, 5),
        (170, 9, 5),
        (300, 15, 5),
        (1100, 55, 5),
        (1300, 65, 5),
    ];
    document["charts"][0]["notes"] = json!({"id":(1..=10).collect::<Vec<_>>(),"op":vec![1;10],
        "judgementType":vec![1;10],"timeMs":notes.iter().map(|n| n.0).collect::<Vec<_>>()});
    document["charts"][0]["fevers"] = json!({"startMs":[50],"endMs":[1150]});
    document["charts"][0]["skillEvents"]["timeMs"] = json!([0, 20, 40, 60, 80]);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 1, 5).to_string()).unwrap();
    let mut request = joint_request_json("mission", true, json!({"kind":"score"}));
    request["k"] = json!(12);
    request["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":(0..=2000).step_by(20).collect::<Vec<_>>(),
        "judged":notes.iter().enumerate().map(|(i, n)| json!([n.1, i + 1, n.2, n.0])).collect::<Vec<_>>()}});
    (data, roster, request)
}

fn compare(data: &DeckData, roster: &Roster, wire: &Value) -> RecommendationOutcome {
    let mut request: RecommendationRequest = serde_json::from_value(wire.clone()).unwrap();
    request.strategy = Strategy::Exhaustive;
    request.k = 100;
    let oracle = engine::recommend(data, roster, &request).unwrap();
    assert_eq!(oracle.completion, Completion::Complete);
    assert_eq!(oracle.results.len(), 12);
    request.strategy = Strategy::BranchAndBound;
    for k in [1, 3, 12] {
        request.k = k;
        let bounded = engine::recommend(data, roster, &request).unwrap();
        assert_eq!(bounded.completion, Completion::Complete);
        assert_eq!(bounded.optimality, Optimality::Proven);
        assert!(bounded.telemetry.environment.bounds.compiled, "{:?}", bounded.telemetry.environment.bounds);
        assert_eq!(bounded.results, oracle.results.iter().take(k).cloned().collect::<Vec<_>>());
    }
    oracle
}

#[cfg(feature = "search-diagnostics")]
fn audit(data: &DeckData, roster: &Roster, wire: &Value, oracle: &RecommendationOutcome) {
    use ournotes_search::{handler, search::diagnostics};
    let request: RecommendationRequest = serde_json::from_value(wire.clone()).unwrap();
    let built = handler::build_card_pool(data, roster, &request).unwrap();
    for row in &oracle.results {
        let orders = diagnostics::audit_order_caps(&built, row.members, row.snaps).unwrap();
        assert_eq!(orders["orders"], 120);
        assert_eq!(orders["violations"], 0, "{orders}");
    }
}

/// Every order of a team with the Snap scores strictly more with `held` than with `ended`.
fn holds_longer(held: &RecommendationOutcome, ended: &RecommendationOutcome) -> bool {
    held.results.iter().filter(|deck| deck.snaps.iter().any(Option::is_some)).all(|deck| {
        let other = ended.results.iter().find(|d| d.members == deck.members && d.snaps == deck.snaps).unwrap();
        deck.order_outcomes.len() == 120
            && deck.order_outcomes.iter().zip(&other.order_outcomes).all(|(a, b)| a.0 == b.0 && a.1 > b.1)
    })
}

#[test]
fn range_start_release_timers_keep_exact_rankings_and_order_caps() {
    for act in [0.004, 0.03, 0.1] {
        let mut outcomes = Vec::new();
        for release in [0, RANGE_COMPLETE, ALWAYS, ONE_PERFECT] {
            let (data, roster, wire) = inputs(act, release);
            let oracle = compare(&data, &roster, &wire);
            #[cfg(feature = "search-diagnostics")]
            audit(&data, &roster, &wire, &oracle);
            outcomes.push(oracle);
        }
        // A release in the frame at 100 holds the factor over the note at 80 that an ordinary 4 ms timer ends before.
        if act == 0.004 {
            assert!(holds_longer(&outcomes[2], &outcomes[0]));
        }
        // A release in the frame at 160, where the 100 ms timer first expires, files 160 instead of 150.
        if act == 0.1 {
            assert!(holds_longer(&outcomes[3], &outcomes[0]));
        }
    }
}
