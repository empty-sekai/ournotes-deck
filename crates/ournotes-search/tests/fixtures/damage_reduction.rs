//! Damage reduction bounds over synthetic member skills, Snap skills and judgement streams.
//! The fixed leader, six cards across five characters and one optional Snap give twelve teams.

use super::common::{extend_table, replace_table, set_column};
use super::{data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
enum Source {
    Gekisou,
    GekisouSupport,
    Live,
    Support,
}

fn inputs(source: Source, sustained: bool, mission: i64, damage: bool, life_factor: &str) -> (DeckData, Roster, Value) {
    let mut synth = synthetic_master(6, 1, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        let first = row["_id"] == 1;
        row["_liveSkillID"] = json!(if first && matches!(source, Source::Live) { 2 } else { 1 });
        row["_gekisouSkillID"] = json!(if first && matches!(source, Source::Gekisou) { 2 } else { 1 });
    });
    set_column(&mut synth, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(if matches!(source, Source::Support) { 1 } else { 0 });
        row["_supportSkillId02"] = json!(0);
        row["_gekisouSupportSkillId01"] = json!(if matches!(source, Source::GekisouSupport) { 2 } else { 0 });
        row["_gekisouSupportSkillId02"] = json!(0);
    });
    set_column(&mut synth, "MasterSupportCardRank", &mut |row| {
        row["_gekisouSupportSkill01Level"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(mission);
        row["_gekisouMission2"] = json!(mission);
        row["_gekisouMission3"] = json!(mission);
    });
    set_column(&mut synth, "MasterLiveSettings", &mut |row| {
        if row["_key"] == "note_score_life_onus_factor" {
            row["_value"] = json!(life_factor);
        }
    });
    replace_table(
        &mut synth,
        "MasterGekisouSkill",
        json!([{"_id":1,"_gekisouMissionType":mission},{"_id":2,"_gekisouMissionType":mission}]),
    );
    replace_table(&mut synth, "MasterGekisouSupportSkill", json!([{"_id":2,"_gekisouMissionType":mission}]));
    extend_table(&mut synth, "MasterSkillEffectSetting", vec![json!({"_id":9001,"_skillEffectType":3004,"_phase":2})]);
    extend_table(
        &mut synth,
        "MasterSkillTarget",
        vec![json!({"_id":9001,"_skillTargetType":5,"_gekisouMissionType":mission})],
    );
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        [7010, 7020, 7013]
            .into_iter()
            .enumerate()
            .map(|(i, kind)| {
                json!({"_id":9001+i,"_conditionType":kind,"_conditionValues":[],
                "_isPositive":true,"_conditionTargetIDs":[9001]})
            })
            .collect(),
    );
    extend_table(
        &mut synth,
        "MasterSkillConditionSet",
        (9001..=9003).map(|id| json!({"_id":id,"_group":id,"_conditionIds":[id]})).collect(),
    );
    for table in [
        "MasterLiveSkillEffect",
        "MasterSupportSkillEffect",
        "MasterGekisouSkillEffect",
        "MasterGekisouSupportSkillEffect",
    ] {
        replace_table(&mut synth, table, json!([]));
    }
    let (table, key, skill, level) = match source {
        Source::Gekisou => ("MasterGekisouSkillEffect", "_gekisouSkillID", 2, 1),
        Source::GekisouSupport => ("MasterGekisouSupportSkillEffect", "_gekisouSupportSkillID", 2, 1),
        Source::Live => ("MasterLiveSkillEffect", "_liveSkillID", 2, 4),
        Source::Support => ("MasterSupportSkillEffect", "_supportSkillID", 1, 3),
    };
    let trigger = match source {
        Source::Gekisou | Source::GekisouSupport => {
            if sustained {
                9002
            } else {
                9001
            }
        }
        Source::Live | Source::Support => 53,
    };
    replace_table(
        &mut synth,
        table,
        json!([{"_id":9001,key:skill,"_level":level,"_skillTriggerType":if sustained {2} else {1},
            "_skillTriggerConditionGroup":trigger,"_skillConditionGroup":0,
            "_skillReleaseConditionGroup":if sustained {9003} else {0},
            "_skillTargetIDs":[],"_skillEffectType":3004,"_activationTimeSecond":if sustained {0.0} else {1.4},
            "_effectValue":5000,"_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]),
    );
    let mut document = data_document(&synth, 6, 1, 5);
    document["charts"][0]["fevers"] = json!({"startMs":[50],"endMs":[1150]});
    document["charts"][0]["skillEvents"]["timeMs"] = json!([0, 20, 40, 60, 80]);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 1, 5).to_string()).unwrap();
    let mut request = joint_request_json("mission", true, json!({"kind":"score"}));
    request["k"] = json!(12);
    request["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":(0..=2000).step_by(20).collect::<Vec<_>>(),
        "judged":(1..=12).map(|i| json!([i*5,i,if damage && i%3 != 0 {1} else {5},i*100])).collect::<Vec<_>>()}});
    (data, roster, request)
}

fn compare(data: &DeckData, roster: &Roster, wire: Value) -> ournotes_search::types::RecommendationOutcome {
    let mut request: RecommendationRequest = serde_json::from_value(wire).unwrap();
    request.strategy = Strategy::Exhaustive;
    request.k = 100;
    let oracle = engine::recommend(data, roster, &request).unwrap();
    assert_eq!(oracle.completion, Completion::Complete);
    assert_eq!(oracle.results.len(), if request.constraints.no_snaps { 2 } else { 12 });
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
fn audit(data: &DeckData, roster: &Roster, wire: Value, oracle: &ournotes_search::types::RecommendationOutcome) {
    use ournotes_search::{handler, search::diagnostics};
    let request: RecommendationRequest = serde_json::from_value(wire).unwrap();
    let mut built = handler::build_card_pool(data, roster, &request).unwrap();
    let classes = diagnostics::prepare_class_audit(&mut built).unwrap();
    assert_eq!(classes, matches!(request.metric, ournotes_search::types::Metric::Score));
    let mut scratch = diagnostics::PrefixAuditScratch::default();
    for row in &oracle.results {
        let orders = diagnostics::audit_order_caps(&built, row.members, row.snaps).unwrap();
        assert_eq!(orders["orders"], 120);
        assert_eq!(orders["violations"], 0, "{orders}");
        let numerator = row.expected_payoff.as_ref().unwrap().numerator.parse::<i128>().unwrap();
        for members in [false, true] {
            for depth in usize::from(!members)..=5 {
                let (cap, power) =
                    diagnostics::split_prefix_upper(&built, row.members, row.snaps, depth, members).unwrap().unwrap();
                assert!(cap >= numerator && power >= i64::from(row.power), "composition depth={depth}");
            }
        }
        for bindings in [false, true].into_iter().filter(|_| classes) {
            for depth in 0..=5 {
                let (cap, power) =
                    diagnostics::class_prefix_upper(&built, row.members, row.snaps, depth, bindings, &mut scratch)
                        .unwrap()
                        .unwrap();
                assert!(cap >= numerator && power >= i64::from(row.power), "class depth={depth}");
            }
        }
    }
}

#[test]
fn damage_reduction_bounds_cover_skill_sources_and_activation_windows() {
    for (source, sustained) in [
        (Source::Gekisou, false),
        (Source::Gekisou, true),
        (Source::GekisouSupport, false),
        (Source::GekisouSupport, true),
        (Source::Live, false),
        (Source::Support, false),
    ] {
        let (data, roster, mut wire) = inputs(source, sustained, 1, true, "0.25");
        let scores = compare(&data, &roster, wire.clone());
        assert_eq!(scores.results.len(), 12);
        #[cfg(feature = "search-diagnostics")]
        audit(&data, &roster, wire.clone(), &scores);
        wire["metric"] = json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":1});
        let life = compare(&data, &roster, wire.clone());
        assert!(life.results.iter().any(|r| r.expected_payoff.as_ref().unwrap().numerator != "0"), "{source:?}");
        assert!(life.results.iter().any(|r| r.expected_payoff.as_ref().unwrap().numerator == "0"), "{source:?}");
        #[cfg(feature = "search-diagnostics")]
        audit(&data, &roster, wire, &life);
    }
}

#[test]
fn damage_reduction_with_recovery_guard_and_life_conditions_preserves_topk() {
    use ournotes_sim::master::{LiveSkillEffectRow, SupportSkillEffectRow};
    for effects in [&[3001][..], &[3003][..], &[3001, 3003][..]] {
        let (mut data, roster, mut wire) = inputs(Source::Gekisou, true, 1, true, "1");
        data.sha256 = None;
        data.charts[0].skill_event_ms = vec![0, 220, 420, 620, 820];
        data.master.support_cards[0].support_skill_id_01 = 1;
        for (i, &effect) in effects.iter().enumerate() {
            data.master.support_skill_effects.push(SupportSkillEffectRow {
                id: 9101 + i as i64,
                support_skill_id: 1,
                level: 3,
                skill_trigger_type: 1,
                skill_trigger_condition_group: 53,
                skill_effect_type: effect,
                activation_time_second: if effect == 3003 { 0.24 } else { 0.0 },
                effect_value: if effect == 3001 { 300 } else { 0 },
                ..Default::default()
            });
        }
        data.master.live_skill_effects.push(LiveSkillEffectRow {
            id: 9101,
            live_skill_id: 1,
            level: 4,
            skill_condition_group: 4,
            skill_effect_type: 2000,
            activation_time_second: 0.4,
            effect_value: 5000,
            ..Default::default()
        });
        data.master.reindex().unwrap();
        let judged = wire["execution"]["play"]["stream"]["judged"].as_array_mut().unwrap();
        for (i, row) in judged.iter_mut().enumerate() {
            let offset = match i % 4 {
                1 => 2,
                3 => 1,
                _ => 0,
            };
            row[0] = json!(row[0].as_i64().unwrap() + offset);
        }
        judged.sort_by_key(|row| row[0].as_i64().unwrap());
        #[cfg(feature = "search-diagnostics")]
        {
            let score = compare(&data, &roster, wire.clone());
            audit(&data, &roster, wire.clone(), &score);
        }
        wire["metric"] = json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":500});
        let result = compare(&data, &roster, wire.clone());
        assert!(result.results.iter().any(|r| r.expected_payoff.as_ref().unwrap().numerator != "0"), "{effects:?}");
        assert!(result.results.iter().any(|r| r.expected_payoff.as_ref().unwrap().numerator == "0"), "{effects:?}");
        #[cfg(feature = "search-diagnostics")]
        audit(&data, &roster, wire, &result);
    }
}

#[test]
fn damage_reduction_early_judgements_preserve_complete_ranking() {
    let (data, roster, mut wire) = inputs(Source::Gekisou, true, 1, true, "1");
    let judged = wire["execution"]["play"]["stream"]["judged"].as_array_mut().unwrap();
    for (i, row) in judged.iter_mut().enumerate() {
        let offset = match i % 4 {
            1 => 2,
            3 => -1,
            _ => 0,
        };
        row[0] = json!(row[0].as_i64().unwrap() + offset);
    }
    judged.sort_by_key(|row| row[0].as_i64().unwrap());
    for metric in [json!({"kind":"score"}), json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":500})] {
        wire["metric"] = metric;
        let mut request: RecommendationRequest = serde_json::from_value(wire.clone()).unwrap();
        request.k = 100;
        request.strategy = Strategy::Exhaustive;
        let oracle = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(oracle.completion, Completion::Complete);
        assert_eq!(oracle.results.len(), 12);
        request.strategy = Strategy::BranchAndBound;
        let actual = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(actual.completion, Completion::Complete);
        assert_eq!(actual.optimality, Optimality::Proven);
        assert!(!actual.telemetry.environment.bounds.compiled);
        assert_eq!(actual.results, oracle.results);
    }
}

#[test]
fn damage_reduction_final_life_target_covers_constant_life_score_factor() {
    for source in [Source::Gekisou, Source::GekisouSupport] {
        let (data, roster, mut wire) = inputs(source, false, 1, true, "1");
        wire["metric"] = json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":1});
        let result = compare(&data, &roster, wire);
        assert!(result.results.iter().any(|r| r.expected_payoff.as_ref().unwrap().numerator != "0"));
        assert!(result.results.iter().any(|r| r.expected_payoff.as_ref().unwrap().numerator == "0"));
    }
}

#[test]
fn luck_damage_reduction_compiles_for_positive_life_streams() {
    for sustained in [false, true] {
        let (data, roster, mut wire) = inputs(Source::Gekisou, sustained, 2, false, "0.25");
        wire["constraints"]["noSnaps"] = json!(true);
        wire["k"] = json!(2);
        let result = compare(&data, &roster, wire);
        assert_eq!(result.results.len(), 2);
        assert!(result.results.iter().all(|r| r.rank_certified == Some(true)));
    }
}
