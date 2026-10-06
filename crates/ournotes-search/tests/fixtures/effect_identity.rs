//! Synthetic effect-identity certificates for optional live bounds and Snap classes.

use super::common::{extend_table, replace_table, set_column};
use super::{SCORE_ID, data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::{self, Completion},
    types::{Execution, PlayPolicy, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, live::model::JudgementStream, pool::Pool};
use serde_json::{Value, json};

const TEAMS: usize = 31;
const WRAPPED_ROW: i64 = 2_674_777_890_687_884_984;

#[derive(Clone, Copy, Debug)]
enum Case {
    Safe,
    WrappedSupport,
    WrappedGekisou,
    DuplicateSourceId,
    ConversionAlias,
    EqualConversionTargets,
    RepeatedCumulativeSource,
    RepeatedSupportProgram,
    RepeatedGekisouProgram,
}

fn row(parent: &str, id: i64, skill: i64, level: i64, effect: i64, value: i64, targets: Value) -> Value {
    json!({"_id":id,parent:skill,"_level":level,"_skillTriggerType":1,
        "_skillTriggerConditionGroup":53,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
        "_skillTargetIDs":targets,"_skillEffectType":effect,"_activationTimeSecond":1.0,
        "_effectValue":value,"_maxEffectValue":0,"_effectLimitCount":0,
        "_skillCumulativeConditionID":0,"_effectExecuteLimitCount":0,
        "_effectExecuteLimitResetConditionGroup":0})
}

fn inputs(case: Case) -> (DeckData, Roster, RecommendationRequest, JudgementStream) {
    let mut synth = synthetic_master(5, 2, 5);
    let gekisou = matches!(case, Case::WrappedGekisou | Case::RepeatedCumulativeSource | Case::RepeatedGekisouProgram);
    set_column(&mut synth, "MasterMemberCard", &mut |r| {
        r["_liveSkillID"] = json!(1);
        r["_gekisouSkillID"] = json!(if gekisou { 1 } else { 0 });
    });
    set_column(&mut synth, "MasterSupportCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_supportSkillId01"] = json!(100 + id);
        r["_supportSkillId02"] = json!(0);
        r["_gekisouSupportSkillId01"] = json!(if matches!(case, Case::RepeatedCumulativeSource) { 203 } else { 0 });
        r["_gekisouSupportSkillId02"] = json!(0);
        if matches!(case, Case::RepeatedSupportProgram) {
            r["_supportSkillId02"] = json!(if id == 1 { 101 } else { 103 });
        }
        if matches!(case, Case::RepeatedGekisouProgram) {
            r["_gekisouSupportSkillId01"] = json!(202 + id);
            r["_gekisouSupportSkillId02"] = json!(if id == 1 { 203 } else { 205 });
        }
    });
    set_column(&mut synth, "MasterSupportCardRank", &mut |r| {
        r["_supportSkill01Level"] = json!(3);
        r["_supportSkill02Level"] = json!(3);
        r["_gekisouSupportSkill01Level"] = json!(1);
        r["_gekisouSupportSkill02Level"] = json!(1);
    });
    replace_table(&mut synth, "MasterLiveSkillEffect", json!([]));
    let mut support = vec![
        row("_supportSkillID", 101, 101, 3, 12006, 5, json!([42])),
        row("_supportSkillID", 102, 101, 3, 12006, 6, json!([42])),
        row("_supportSkillID", 201, 102, 3, 12006, 5, json!([42])),
        row("_supportSkillID", 202, 102, 3, 12006, 6, json!([42])),
    ];
    if matches!(case, Case::WrappedSupport) {
        support[3]["_id"] = json!(WRAPPED_ROW);
    }
    if matches!(case, Case::DuplicateSourceId) {
        support[2]["_id"] = json!(101);
    }
    if matches!(case, Case::RepeatedSupportProgram) {
        support.push(row("_supportSkillID", 301, 103, 3, 2000, 100, json!([])));
        support.push(row("_supportSkillID", 302, 103, 3, 2000, 100, json!([])));
        for effect in &mut support {
            effect["_skillEffectType"] = json!(2000);
            effect["_effectValue"] = json!(100);
            effect["_skillTargetIDs"] = json!([]);
        }
    }
    // This skill is outside the candidate domain and does not constrain its classes.
    support.push(row("_supportSkillID", WRAPPED_ROW, 999, 3, 12006, 5, json!([43])));
    replace_table(&mut synth, "MasterSupportSkillEffect", json!(support));
    if matches!(case, Case::ConversionAlias | Case::EqualConversionTargets) {
        let target = if matches!(case, Case::ConversionAlias) { 43 } else { 42 };
        replace_table(
            &mut synth,
            "MasterLiveSkillEffect",
            json!([row("_liveSkillID", 101, 1, 4, 12006, 5, json!([target]))]),
        );
    }
    if gekisou {
        set_column(&mut synth, "MasterLiveMusic", &mut |r| {
            for key in ["_gekisouMission1", "_gekisouMission2", "_gekisouMission3"] {
                r[key] = json!(1);
            }
        });
        replace_table(&mut synth, "MasterGekisouSkill", json!([{"_id":1,"_gekisouMissionType":1}]));
        set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |r| r["_scoreBonusPercent"] = json!(0));
        let id = if matches!(case, Case::WrappedGekisou) { WRAPPED_ROW } else { 901 };
        replace_table(
            &mut synth,
            "MasterGekisouSkillEffect",
            json!([row("_gekisouSkillID", id, 1, 1, 2000, 100, json!([]))]),
        );
    }
    if matches!(case, Case::RepeatedCumulativeSource) {
        replace_table(&mut synth, "MasterGekisouSupportSkill", json!([{"_id":203,"_gekisouMissionType":1}]));
        extend_table(
            &mut synth,
            "MasterSkillCumulativeCondition",
            vec![json!({"_id":9003,"_skillCumulativeConditionType":7001,"_conditionValues":[1],
                "_conditionTargetIDs":[],"_maxCumulativeCount":3})],
        );
        extend_table(
            &mut synth,
            "MasterSkillEffectSetting",
            vec![json!({"_id":9003,"_skillEffectType":2001,"_phase":2})],
        );
        let mut effect = row("_gekisouSupportSkillID", 902, 203, 1, 2001, 100, json!([]));
        effect["_skillCumulativeConditionID"] = json!(9003);
        effect["_maxEffectValue"] = json!(300);
        replace_table(&mut synth, "MasterGekisouSupportSkillEffect", json!([effect]));
    }
    if matches!(case, Case::RepeatedGekisouProgram) {
        replace_table(
            &mut synth,
            "MasterGekisouSupportSkill",
            json!((203..=205).map(|id| json!({"_id":id,"_gekisouMissionType":1})).collect::<Vec<_>>()),
        );
        replace_table(
            &mut synth,
            "MasterGekisouSupportSkillEffect",
            json!(
                (203..=205)
                    .map(|skill| row("_gekisouSupportSkillID", 699 + skill, skill, 1, 2000, 100, json!([])))
                    .collect::<Vec<_>>()
            ),
        );
    }
    set_column(&mut synth, "MasterLiveMusicScore", &mut |r| r["_fullComboCount"] = json!(2));
    let mut document = data_document(&synth, 5, 2, 5);
    document["provenance"]["source"] = json!("tests/fixtures/effect_identity.rs + synthetic_master");
    document["charts"][0]["notes"] = json!({"id":[1,2],"op":[1,1],"judgementType":[1,1],"timeMs":[100,200]});
    document["charts"][0]["skillEvents"] = json!({"timeMs":[0,0,0,0,0]});
    document["charts"][0]["fevers"] =
        if gekisou { json!({"startMs":[0],"endMs":[300]}) } else { json!({"startMs":[],"endMs":[]}) };
    let stream: JudgementStream =
        serde_json::from_value(json!({"frames":[0,20,100,200,300,320],"judged":[[2,1,4,100],[3,2,4,200]]})).unwrap();
    let mut request = joint_request(if gekisou { "mission" } else { "free" }, gekisou, json!({"kind":"score"}));
    request.execution =
        Execution::Live { score_id: SCORE_ID, gekisou, play: PlayPolicy::Stream { stream: stream.clone() } };
    request.constraints.leader = Some(1);
    request.k = TEAMS;
    (
        DeckData::from_json(&document.to_string()).unwrap(),
        Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap(),
        request,
        stream,
    )
}

fn compare_with_exhaustive(case: Case, reason: Option<&str>) {
    let (data, roster, mut request, _) = inputs(case);
    let actual = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(actual.completion, Completion::Complete, "{case:?}");
    assert_eq!(actual.results.len(), TEAMS, "{case:?}");
    assert_eq!(actual.telemetry.environment.bounds.compiled, reason.is_none(), "{case:?}");
    match reason {
        Some(expected) => assert!(
            actual.telemetry.environment.bounds.fallback.as_deref().is_some_and(|value| value.contains(expected)),
            "{case:?}: {:?}",
            actual.telemetry.environment.bounds.fallback
        ),
        None => assert!(actual.telemetry.environment.bounds.fallback.is_none(), "{case:?}"),
    }
    request.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete, "{case:?}");
    assert!(actual.results.iter().all(|deck| deck.order_outcomes.len() == 120), "{case:?}");
    assert_eq!(actual.results, exhaustive.results, "{case:?}");
}

#[test]
fn effect_identity_refusals_keep_every_team_and_order() {
    for (case, reason) in [
        (Case::WrappedSupport, "condition effect key outside the certified integer range"),
        (Case::WrappedGekisou, "condition effect key outside the certified integer range"),
        (Case::DuplicateSourceId, "distinct skill effect rows share a native identity"),
        (Case::ConversionAlias, "conversion effect ID has inconsistent native targets"),
    ] {
        compare_with_exhaustive(case, Some(reason));
    }
}

#[test]
fn equal_conversion_targets_and_repeated_sources_keep_bounds() {
    for case in [Case::Safe, Case::EqualConversionTargets, Case::RepeatedCumulativeSource] {
        compare_with_exhaustive(case, None);
    }
}

#[test]
fn repeated_snap_programs_refuse_classes_and_keep_every_exact_outcome() {
    const REASON: &str = "a Snap skill program repeats a native effect identity";
    for case in [Case::RepeatedSupportProgram, Case::RepeatedGekisouProgram] {
        // Score-factor rows remain valid in the unrestricted native evaluator,
        // including two copies of each score effect at one position.
        compare_with_exhaustive(case, Some(REASON));
    }
    let (data, roster, _, stream) = inputs(Case::RepeatedSupportProgram);
    let settings = ournotes_sim::live::score::LiveScoreSettings::from_master(&data.master).unwrap();
    let source = &data.charts[0];
    let request = search::SearchRequest {
        objective: search::Objective::LiveScore {
            score_id: SCORE_ID,
            chart: source.chart(&settings).unwrap(),
            play: search::PlayInput::Stream { stream, judgement_types: source.judgement_types.clone() },
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        },
        k: 1,
        constraints: search::Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    };
    let pool = Pool::new(&data.master, &roster).unwrap();
    assert!(matches!(
        search::search_best_order_diagnostic(&pool, &request),
        Err(ournotes_sim::Error::Domain(message)) if message.contains(REASON)
    ));
}

#[test]
fn ordered_native_keys_preserve_snap_class_merging() {
    let (data, roster, _, stream) = inputs(Case::Safe);
    let settings = ournotes_sim::live::score::LiveScoreSettings::from_master(&data.master).unwrap();
    let source = &data.charts[0];
    let request = search::SearchRequest {
        objective: search::Objective::LiveScore {
            score_id: SCORE_ID,
            chart: source.chart(&settings).unwrap(),
            play: search::PlayInput::Stream { stream, judgement_types: source.judgement_types.clone() },
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        },
        k: 1,
        constraints: search::Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    };
    let pool = Pool::new(&data.master, &roster).unwrap();
    let result = search::search_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.classes.len(), 5);
    // The two programs have different row IDs and identical ordered effects.
    assert!(result.classes.iter().all(|&(_, count)| count == 2), "{:?}", result.classes);
}
