//! Synthetic regression for a sustained COMBO factor held across playing ranges.
//! The fixed evaluator is the oracle for these declared inputs; this is not native-game evidence.

use super::common::{extend_table, replace_table, set_column};
use super::{data_document, joint_request_json, roster_document, synthetic_master};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn inputs() -> (Value, Value, Value) {
    // Keep the original synthetic master so the regression retains its exact score and power.
    // The owned roster below has only five members and no Snaps: one team under the fixed leader.
    let mut synth = synthetic_master(6, 2, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_liveSkillID"] = json!(1);
        row["_gekisouSkillID"] = json!(1);
    });
    replace_table(&mut synth, "MasterLiveSkillEffect", json!([]));
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(1);
        row["_gekisouMission2"] = json!(1);
        row["_gekisouMission3"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(55));
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        vec![json!({"_id":9001,"_conditionType":2003,"_conditionValues":[990],
            "_isPositive":true,"_conditionTargetIDs":[]})],
    );
    extend_table(&mut synth, "MasterSkillConditionSet", vec![json!({"_id":9001,"_group":9001,"_conditionIds":[9001]})]);
    extend_table(&mut synth, "MasterSkillEffectSetting", vec![json!({"_id":9001,"_skillEffectType":2001,"_phase":2})]);
    replace_table(
        &mut synth,
        "MasterSkillCumulativeCondition",
        json!([{"_id":9001,"_skillCumulativeConditionType":7001,"_conditionValues":[1],
            "_conditionTargetIDs":[],"_maxCumulativeCount":10}]),
    );
    replace_table(
        &mut synth,
        "MasterGekisouSkillEffect",
        json!([{"_id":9001,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":2,
            "_skillTriggerConditionGroup":9001,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":2001,"_activationTimeSecond":0.0,
            "_effectValue":10000,"_maxEffectValue":100000,"_effectLimitCount":0,
            "_skillCumulativeConditionID":9001,"_effectExecuteLimitCount":0,
            "_effectExecuteLimitResetConditionGroup":0}]),
    );

    let times: Vec<i32> = [100, 200, 300, 350].into_iter().chain((750..1000).step_by(5)).chain([1150]).collect();
    let mut data = data_document(&synth, 6, 2, 5);
    data["provenance"]["source"] = json!("tests/fixtures/sustained_combo.rs + synthetic_master");
    data["charts"][0]["notes"] = json!({"id":(1..=times.len()).collect::<Vec<_>>(),
        "op":vec![1;times.len()],"judgementType":vec![1;times.len()],"timeMs":times});
    // The Miss starts LifeAtMost(990) in the first range. The later ranges have no
    // inside judgements, so their zero COMBO count must not erase the held factor.
    data["charts"][0]["fevers"] = json!({"startMs":[100,700,1100],"endMs":[400,710,1110]});
    let mut request = joint_request_json("mission", true, json!({"kind":"score"}));
    request["constraints"]["leader"] = json!(1);
    request["k"] = json!(1);
    request["strategy"] = json!({"kind":"exhaustive"});
    request["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":(0..=4000).step_by(40).collect::<Vec<_>>(),
        "judged":times.iter().enumerate().map(|(index,&ms)|
            json!([(ms+39)/40,index+1,if index==2 {1} else {5},ms])).collect::<Vec<_>>()}});
    (data, roster_document(5, 0, 5), request)
}

/// Include the counterexample in the ordinary synthetic suite and its exhaustive bound audit.
pub(super) fn export(out: &Path) -> String {
    let (data, roster, request) = inputs();
    let name = "sustained-combo-ramp.json";
    let case = json!({"id":"sustained-combo-ramp-freeze",
        "data":"sustained-combo-ramp-DeckData.json","roster":"sustained-combo-ramp-roster.json",
        "request":"sustained-combo-ramp-request.json","oracleMaxCandidates":1,"dominance":[],
        "experiments":[{"name":"exhaustive","patch":{},"repeats":1},
            {"name":"joint-bnb","patch":{"strategy":{"kind":"branchAndBound"}},"repeats":1}]});
    for (file, value) in [
        ("sustained-combo-ramp-DeckData.json", data),
        ("sustained-combo-ramp-roster.json", roster),
        ("sustained-combo-ramp-request.json", request),
        (name, case),
    ] {
        fs::write(out.join(file), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    name.into()
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn sustained_combo_held_factor_is_bounded_across_playing_ranges() {
    use ournotes_search::{
        auxiliary::evaluate_fixed,
        handler,
        search::{Completion, diagnostics},
        types::{RecommendationRequest, Strategy},
    };
    use ournotes_sim::{cards::Roster, data::DeckData};

    let (data, roster, request) = inputs();
    let data = DeckData::from_json(&data.to_string()).unwrap();
    let roster = Roster::from_json(&roster.to_string()).unwrap();
    let mut request: RecommendationRequest = serde_json::from_value(request).unwrap();
    let members = [2, 3, 1, 4, 5];
    let snaps = [None; 5];
    let fixed = evaluate_fixed(&data, &roster, &request, members, snaps).unwrap();
    assert_eq!(fixed.completion, Completion::Complete);
    let deck = &fixed.results[0];
    let payoff = deck.expected_payoff.as_ref().unwrap();
    let actual = payoff.numerator.parse::<i128>().unwrap();
    assert_eq!(deck.power, 119_687);
    assert_eq!(deck.order_outcomes.len(), 120);
    assert_eq!(payoff.denominator, "120");
    assert_eq!(actual, 421_578_360, "the sustained factor must remain active after the range changes");

    request.strategy = Strategy::BranchAndBound;
    let mut built = handler::build_card_pool(&data, &roster, &request).unwrap();
    assert!(diagnostics::prepare_class_audit(&mut built).unwrap(), "this fixture must compile class bounds");
    let mut scratch = diagnostics::PrefixAuditScratch::default();
    for bindings in [false, true] {
        for depth in 0..=5 {
            let (cap, power) = diagnostics::class_prefix_upper(&built, members, snaps, depth, bindings, &mut scratch)
                .unwrap()
                .expect("class bound must remain available");
            assert!(cap >= actual, "class bindings={bindings} depth={depth}: {cap} < {actual}");
            assert!(power >= i64::from(deck.power));
        }
    }
    for depth in 1..=5 {
        let (cap, power) = diagnostics::prefix_upper(&built, members, snaps, depth, &mut scratch)
            .unwrap()
            .expect("joint bound must remain available");
        assert!(cap >= actual, "joint depth={depth}: {cap} < {actual}");
        assert!(power >= i64::from(deck.power));
    }
    // Each leaf cap is checked against its own independently simulated order, not just the mean.
    let orders = diagnostics::audit_order_caps(&built, members, snaps).unwrap();
    assert_eq!(orders["orders"], 120);
    assert_eq!(orders["violations"], 0, "per-order bound: {}", orders["first"]);
}
