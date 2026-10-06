//! Synthetic binary32 COMBO threshold regression; the fixed scorer defines the expected result.

use super::common::{extend_table, replace_table, set_column};
use super::{data_document, joint_request_json, roster_document, synthetic_master};
use serde_json::{Value, json};

fn inputs(bonus: i64) -> (Value, Value, Value) {
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_gekisouSkillID"] = json!(if row["_id"].as_i64().unwrap() == 1 { 1 } else { 0 });
    });
    replace_table(&mut synth, "MasterLiveSkillEffect", json!([]));
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        for key in ["_gekisouMission1", "_gekisouMission2", "_gekisouMission3"] {
            row[key] = json!(1);
        }
    });
    set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(2));
    set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |row| row["_scoreBonusPercent"] = json!(0));
    // The negated fixed-false score-rank condition starts the sustained effect.
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        vec![json!({"_id":9001,"_conditionType":8000,"_isPositive":false,
            "_conditionValues":[],"_conditionTargetIDs":[]})],
    );
    extend_table(&mut synth, "MasterSkillConditionSet", vec![json!({"_id":9001,"_group":9001,"_conditionIds":[9001]})]);
    extend_table(&mut synth, "MasterSkillEffectSetting", vec![json!({"_id":9001,"_skillEffectType":12000,"_phase":2})]);
    replace_table(
        &mut synth,
        "MasterLiveComboScoreBonus",
        json!([
            {"_id":1,"_comboBonusType":0,"_requiredComboCount":1,"_bonusFactor":0.0},
            {"_id":2,"_comboBonusType":1,"_requiredComboCount":33_554_440,"_bonusFactor":1.0}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterGekisouSkillEffect",
        json!([{"_id":9001,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":2,
            "_skillTriggerConditionGroup":9001,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":12000,"_activationTimeSecond":0.0,
            "_effectValue":bonus,"_maxEffectValue":0,"_effectLimitCount":0,
            "_skillCumulativeConditionID":0,"_effectExecuteLimitCount":0,
            "_effectExecuteLimitResetConditionGroup":0}]),
    );
    let mut data = data_document(&synth, 5, 0, 5);
    data["provenance"]["source"] = json!("tests/fixtures/combo_integer.rs + synthetic_master");
    data["charts"][0]["notes"] = json!({"id":[1,2],"op":[1,1],"judgementType":[1,1],"timeMs":[100,200]});
    data["charts"][0]["skillEvents"] = json!({"timeMs":[0,0,0,0,0]});
    data["charts"][0]["fevers"] = json!({"startMs":[0],"endMs":[300]});
    let mut request = joint_request_json("mission", true, json!({"kind":"score"}));
    request["constraints"]["leader"] = json!(1);
    request["k"] = json!(1);
    (data, roster_document(5, 0, 5), request)
}

#[test]
fn combo_integer_rounding_refuses_bounds_and_retains_exact_results() {
    use ournotes_search::{
        engine,
        search::Completion,
        types::{RecommendationRequest, Strategy},
    };
    use ournotes_sim::{cards::Roster, data::DeckData};

    // Binary32 maps this bonus to 33_554_440 and adding one still gives
    // 33_554_440. The exact integer 1 + bonus is only 33_554_439: using it as
    // a count cap misses the table step which doubles the second note's score.
    let (data, roster, request) = inputs(33_554_438);
    let data = DeckData::from_json(&data.to_string()).unwrap();
    let roster = Roster::from_json(&roster.to_string()).unwrap();
    let mut request: RecommendationRequest = serde_json::from_value(request).unwrap();
    request.strategy = Strategy::Exhaustive;
    let exact = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(exact.completion, Completion::Complete);
    assert_eq!(exact.results.len(), 1);
    let deck = &exact.results[0];
    assert_eq!(deck.power, 119_687);
    assert_eq!(deck.order_outcomes.len(), 120);
    assert!(deck.order_outcomes.iter().all(|&(_, score, payoff)| score == 491_464 && payoff == 491_464));
    let mean = deck.expected_payoff.as_ref().unwrap();
    assert_eq!(mean.numerator, "58975680");
    assert_eq!(mean.denominator, "120");

    request.strategy = Strategy::BranchAndBound;
    let bounded = engine::recommend(&data, &roster, &request).unwrap();
    assert!(!bounded.telemetry.environment.bounds.compiled, "unsafe integer COMBO refinements must not be installed");
    assert!(
        bounded
            .telemetry
            .environment
            .bounds
            .fallback
            .as_deref()
            .is_some_and(|reason| reason.contains("COMBO bonus arithmetic outside the certified integer range"))
    );
    assert_eq!(bounded.completion, Completion::Complete);
    assert_eq!(bounded.results, exact.results, "the optional-bound refusal must retain all exact values and orders");
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn exact_combo_integer_stack_keeps_optional_bounds() {
    use ournotes_search::{handler, search::diagnostics};
    use ournotes_sim::{cards::Roster, data::DeckData};

    let (data, roster, request) = inputs(3);
    let data = DeckData::from_json(&data.to_string()).unwrap();
    let roster = Roster::from_json(&roster.to_string()).unwrap();
    let request = serde_json::from_value(request).unwrap();
    let built = handler::build_card_pool(&data, &roster, &request).unwrap();
    assert!(diagnostics::describe_bound(&built, [2, 3, 1, 4, 5], [None; 5], [0, 1, 2, 3, 4]).unwrap().is_some());
    let audit = diagnostics::audit_order_caps(&built, [2, 3, 1, 4, 5], [None; 5]).unwrap();
    assert_eq!(audit["orders"], 120);
    assert_eq!(audit["violations"], 0, "{}", audit["first"]);
}
