//! Effect-type applier dispatch, live effect limits, condition trigger types, Gekisou mission All and fever counts,
//! on synthetic tables. Every number here is made up.

use ournotes_deck::Error;
use ournotes_deck::live::full::{
    GekisouSetup, JudgedNote, LiveModel, LiveNote, LiveParams, LivePlay, Performer, PlayFrame,
};
use ournotes_deck::master::Master;
use serde_json::{Value, json};

const LENGTH: i32 = 1600;
const PERCENT: [i64; 7] = [0, 0, 0, 50, 80, 100, 200];

fn master_from(tables: &Value) -> Master {
    let texts: Vec<(String, String)> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({ "_allData": v }).to_string())).collect();
    Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, t)| t.as_str())).unwrap()
}

/// An effect row of `key` (`_liveSkillID` rows have no trigger columns); trigger group 1 is 4010, 2 is 7010 (Combo).
fn row(key: &str, sid: i64, effect_type: i64, value: i64, extra: Value) -> Value {
    let mut r = json!({"_id": 100, key: sid, "_level": 1, "_skillConditionGroup": 0,
        "_skillReleaseConditionGroup": 0, "_skillTargetIDs": [], "_skillEffectType": effect_type,
        "_activationTimeSecond": 1.0, "_effectValue": value, "_maxEffectValue": 0, "_effectLimitCount": 0,
        "_skillCumulativeConditionID": 0, "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0});
    if key != "_liveSkillID" {
        r["_skillTriggerType"] = json!(1);
        r["_skillTriggerConditionGroup"] = json!(1);
    }
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

fn tables() -> Value {
    let judgement: Vec<Value> = (1..=6)
        .map(|j| {
            json!({"_id": j, "_noteSimulateJudgement": j, "_scorePercent": PERCENT[j],
                        "_damage": if j == 1 { 100 } else { 0 }})
        })
        .collect();
    let ranks: Vec<Value> = (1..=3)
        .flat_map(|p| {
            (1..=3).map(move |c| {
                json!({"_id": 10 * p + c, "_missionPattern": p, "_count": c, "_rank": 1,
                                                   "_scoreBonusPercent": 100})
            })
        })
        .collect();
    json!({
        "MasterLiveNoteParameter": [{"_id": 1, "_noteOperateType": 1, "_scorePercent": 100}],
        "MasterLiveJudgementParameter": judgement,
        "MasterLiveSettings": [
            {"_id": 1, "_key": "note_score_adjustment_factor", "_value": "3"},
            {"_id": 2, "_key": "note_score_life_onus_factor", "_value": "0.5"},
            {"_id": 3, "_key": "life_base", "_value": "1000"},
            {"_id": 4, "_key": "life_denger", "_value": "300"},
            {"_id": 5, "_key": "gekisou_luck_gauge_max", "_value": "140"},
            {"_id": 6, "_key": "gekisou_luck_gauge_max_rush", "_value": "70"},
            {"_id": 7, "_key": "gekisou_luck_rush_score_bonus_percent", "_value": "10"}],
        "MasterLiveComboScoreBonus": [
            {"_id": 1, "_comboBonusType": 1, "_requiredComboCount": 5, "_bonusFactor": 0.02}],
        "MasterSkillEffectSetting": [{"_id": 1, "_skillEffectType": 12000, "_phase": 2}],
        "MasterLiveJudgementTiming": [
            {"_id": 1, "_noteJudgementType": 1, "_noteSimulateJudgement": 6, "_afterMs": 50},
            {"_id": 2, "_noteJudgementType": 1, "_noteSimulateJudgement": 5, "_afterMs": 100}],
        "MasterLiveGekisouRankingScoreBonus": ranks,
        "MasterLiveGekisouLuckBasePoint": [
            {"_id": 1, "_noteCategory": 0, "_noteSimulateJudgement": 5, "_weight": 1, "_basePoint": 10}],
        "MasterLiveGekisouLuckBonusLot": [{"_id": 1, "_chanceLotType": 0, "_lotResult": 1, "_weight": 1}],
        "MasterSkillTarget": [{"_id": 2, "_skillTargetType": 5, "_gekisouMissionType": 1}],
        "MasterSkillCondition": [
            {"_id": 1, "_conditionType": 4010, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": []},
            {"_id": 2, "_conditionType": 7010, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": [2]}],
        "MasterSkillConditionSet": [{"_id": 1, "_group": 1, "_conditionIds": [1]},
                                    {"_id": 2, "_group": 2, "_conditionIds": [2]}],
        "MasterGekisouSkill": [{"_id": 1, "_gekisouMissionType": 1}, {"_id": 3, "_gekisouMissionType": 3},
                               {"_id": 4, "_gekisouMissionType": 4}, {"_id": 5, "_gekisouMissionType": 0},
                               {"_id": 6, "_gekisouMissionType": 5}],
    })
}

fn notes() -> Vec<LiveNote> {
    (0..20)
        .map(|i| LiveNote { note_id: i + 1, time_ms: 40 + 60 * i, note_operate_type: 1, judgement_type: 1 })
        .collect()
}

/// Member 0's chart skill events: 100 is before the first range, with no range changing state in that frame.
const EVENTS: [(i32, i32); 2] = [(0, 100), (0, 500)];

fn params() -> LiveParams {
    LiveParams {
        total_power: 200_000,
        music_level: 25,
        converted_note_count: 20,
        music_length_ms: LENGTH,
        skill_target_music_type: 0,
        score_music_length_ms: None,
        assist_factor: 1.0,
    }
}

fn play() -> LivePlay {
    let mut frames: Vec<PlayFrame> = (0..LENGTH / 16).map(|k| PlayFrame { time_ms: 16 * k, judged: vec![] }).collect();
    for n in notes() {
        let f = frames.iter().position(|f| f.time_ms >= n.time_ms).unwrap();
        frames[f].judged.push(JudgedNote { note_id: n.note_id, judgement: 5, judgement_time_ms: n.time_ms });
    }
    LivePlay { frames, base_seed: 3 }
}

fn setup(fevers: &[(i32, i32)]) -> GekisouSetup {
    GekisouSetup { fevers: fevers.to_vec(), missions: vec![1, 1, 1] }
}

fn run(t: &Value, deck: &[Performer], gk: Option<&GekisouSetup>) -> Result<(i32, Vec<i32>), Error> {
    let m = master_from(t);
    let mut lm = match gk {
        None => LiveModel::new(&m, deck, &notes(), &EVENTS, params())?,
        Some(s) => LiveModel::new_gekisou(&m, deck, &notes(), &EVENTS, params(), s)?,
    };
    let p = play();
    let score = lm.run_timed(&p, &vec![0.016; p.frames.len()])?;
    Ok((score, lm.gekisou_ranges().iter().map(|r| r.max_combo).collect()))
}

#[test]
fn live_effect_limit_is_not_a_construction_gate() {
    for effect_type in [1000, 0, 2000, 3003] {
        let mut t = tables();
        t["MasterLiveSkillEffect"] = json!([row("_liveSkillID", 1, effect_type, 500, json!({"_effectLimitCount": 3}))]);
        let limited = run(&t, &[Performer { live_skill: Some((1, 1)), ..Default::default() }], None).unwrap();
        t["MasterLiveSkillEffect"][0]["_effectLimitCount"] = json!(0);
        let unlimited = run(&t, &[Performer { live_skill: Some((1, 1)), ..Default::default() }], None).unwrap();
        assert_eq!(limited, unlimited, "effect type {effect_type}");
    }
    // types without a live applier do nothing
    let mut t = tables();
    t["MasterLiveSkillEffect"] = json!([row("_liveSkillID", 1, 1000, 500, json!({"_effectLimitCount": 3}))]);
    let plain = run(&t, &[Performer::default()], None).unwrap();
    assert_eq!(run(&t, &[Performer { live_skill: Some((1, 1)), ..Default::default() }], None).unwrap(), plain);
}

#[test]
fn gekisou_only_effect_types_depend_on_gekisou_not_on_the_skill_type() {
    let fevers = setup(&[(200, 800)]);
    let snap = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
    let live = [Performer { live_skill: Some((1, 1)), ..Default::default() }];
    let mut t = tables();
    t["MasterSupportSkillEffect"] = json!([row("_supportSkillID", 1, 12000, 3, json!({}))]);
    t["MasterLiveSkillEffect"] = json!([row("_liveSkillID", 1, 12000, 3, json!({}))]);
    let plain = run(&t, &[Performer::default()], None).unwrap();
    let plain_gk = run(&t, &[Performer::default()], Some(&fevers)).unwrap();
    for deck in [&snap[..], &live[..]] {
        // no applier without Gekisou
        assert_eq!(run(&t, deck, None).unwrap(), plain);
        // with Gekisou the Gekisou combo bonus applier runs for a live or snap skill too
        let (score, combo) = run(&t, deck, Some(&fevers)).unwrap();
        assert!(combo[0] > plain_gk.1[0], "{combo:?} vs {:?}", plain_gk.1);
        assert!(score > plain_gk.0);
    }
    // 13005 (Just conversion) is registered only with Gekisou
    let mut t = tables();
    t["MasterSkillTarget"] = json!([{"_id": 2, "_skillTargetType": 4, "_judgement": 5}]);
    t["MasterSupportSkillEffect"] =
        json!([row("_supportSkillID", 1, 13005, 0, json!({"_skillTargetIDs": [2], "_effectLimitCount": 2}))]);
    assert_eq!(run(&t, &snap, None).unwrap(), run(&t, &[Performer::default()], None).unwrap());
}

#[test]
fn condition_trigger_types_other_than_one_shot_and_sustained_fail_at_construction() {
    for trigger_type in [0, 3] {
        let mut t = tables();
        t["MasterSupportSkillEffect"] =
            json!([row("_supportSkillID", 1, 2000, 1000, json!({"_skillTriggerType": trigger_type}))]);
        let deck = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
        assert!(matches!(run(&t, &deck, None), Err(Error::Game(_))), "trigger type {trigger_type}");
    }
}

#[test]
fn mission_all_passes_every_gate_and_invalid_missions_fail() {
    let fevers = setup(&[(200, 800)]);
    let mut t = tables();
    // 12000 triggered by the member's chart events at 100 (before the range) and 500 (inside the Combo range)
    t["MasterGekisouSkillEffect"] = json!([
        row("_gekisouSkillID", 1, 12000, 3, json!({"_id": 101})),
        row("_gekisouSkillID", 3, 12000, 3, json!({"_id": 103})),
        row("_gekisouSkillID", 4, 12000, 3, json!({"_id": 104}))
    ]);
    let gk = |sid| [Performer { gekisou_skill: Some((sid, 1)), ..Default::default() }];
    let none = run(&t, &[Performer::default()], Some(&fevers)).unwrap().1[0];
    let combo = run(&t, &gk(1), Some(&fevers)).unwrap().1[0];
    let just = run(&t, &gk(3), Some(&fevers)).unwrap().1[0];
    let all = run(&t, &gk(4), Some(&fevers)).unwrap().1[0];
    assert_eq!(just, none); // a JustCount skill never opens its gate in a Combo range
    assert!(combo > none); // the Combo skill starts at 500, inside its range
    assert!(all > combo); // All starts already at 100 (its gate is always open), so it covers more range notes
    for sid in [5, 6] {
        assert!(matches!(run(&t, &gk(sid), Some(&fevers)), Err(Error::Game(_))), "skill {sid}");
        assert!(matches!(run(&t, &gk(sid), None), Err(Error::Game(_))), "skill {sid} without Gekisou");
    }
}

#[test]
fn only_three_fevers_are_ranges() {
    let t = tables();
    let three = run(&t, &[Performer::default()], Some(&setup(&[(200, 400), (500, 700), (800, 1000)]))).unwrap();
    // a fourth fever after the last frame never starts: the live is the three-range live
    let late = setup(&[(200, 400), (500, 700), (800, 1000), (5000, 6000)]);
    assert_eq!(run(&t, &[Performer::default()], Some(&late)).unwrap(), three);
    // a fourth fever inside the live fails when it starts
    let early = setup(&[(200, 400), (500, 700), (800, 1000), (1100, 1200)]);
    assert!(matches!(run(&t, &[Performer::default()], Some(&early)), Err(Error::Game(_))));
}

#[test]
fn unknown_cumulative_condition_types_fail_at_construction() {
    for kind in [0, 1003, 8000] {
        let mut t = tables();
        t["MasterSkillCumulativeCondition"] = json!([{"_id": 9, "_skillCumulativeConditionType": kind,
            "_conditionValues": [1], "_conditionTargetIDs": [], "_maxCumulativeCount": 0}]);
        t["MasterLiveSkillEffect"] =
            json!([row("_liveSkillID", 1, 2001, 100, json!({"_skillCumulativeConditionID": 9}))]);
        let deck = [Performer { live_skill: Some((1, 1)), ..Default::default() }];
        assert!(matches!(run(&t, &deck, None), Err(Error::Game(_))), "cumulative type {kind}");
    }
}
