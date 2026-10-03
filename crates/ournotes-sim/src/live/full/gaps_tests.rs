//! Construction order of Gekisou condition skills (SkillStatus merge order) and the All mission gate.

use super::*;
use serde_json::json;

fn master() -> Master {
    let row = |id: i64, sid: i64| {
        json!({"_id": id, "_gekisouSkillID": sid, "_level": 1, "_skillTriggerType": 1,
            "_skillTriggerConditionGroup": 0, "_skillConditionGroup": 0, "_skillReleaseConditionGroup": 0,
            "_skillTargetIDs": [], "_skillEffectType": 12000, "_activationTimeSecond": 1.0, "_effectValue": 1,
            "_maxEffectValue": 0, "_effectLimitCount": 0, "_skillCumulativeConditionID": 0,
            "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0})
    };
    let tables = json!({
        "MasterLiveSettings": [
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
        "MasterGekisouSkill": [{"_id":1,"_gekisouMissionType":1},{"_id":2,"_gekisouMissionType":2},
                               {"_id":3,"_gekisouMissionType":3},{"_id":4,"_gekisouMissionType":4}],
        "MasterGekisouSkillEffect": [row(1, 1), row(2, 2), row(3, 3), row(4, 4)],
        "MasterGekisouSupportSkill": [{"_id":1,"_gekisouMissionType":2}],
        "MasterGekisouSupportSkillEffect": [{"_id": 9, "_gekisouSupportSkillID": 1, "_level": 1,
            "_skillTriggerType": 1, "_skillTriggerConditionGroup": 0, "_skillConditionGroup": 0,
            "_skillReleaseConditionGroup": 0, "_skillTargetIDs": [], "_skillEffectType": 12000,
            "_activationTimeSecond": 1.0, "_effectValue": 1, "_maxEffectValue": 0, "_effectLimitCount": 0,
            "_skillCumulativeConditionID": 0, "_effectExecuteLimitCount": 0,
            "_effectExecuteLimitResetConditionGroup": 0}],
    });
    let texts: Vec<_> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    Master::from_json_tables(|name| texts.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())).unwrap()
}

fn params() -> LiveParams {
    LiveParams {
        total_power: 1,
        music_level: 1,
        converted_note_count: 1,
        music_length_ms: 1000,
        skill_target_music_type: 0,
        score_music_length_ms: None,
        assist_factor: 1.0,
    }
}

#[test]
fn mission_all_takes_the_combo_list_place_and_keeps_its_own_gate() {
    let m = master();
    let gk = |sid| Performer { gekisou_skill: Some((sid, 1)), ..Default::default() };
    // members in skill order: JustCount, All, Luck, Combo (the last one with a Gekisou support skill)
    let mut deck = vec![gk(3), gk(4), gk(2), gk(1)];
    deck[3].gekisou_support_skills = vec![(1, 1)];
    let setup = GekisouSetup { fevers: vec![(100, 200)], missions: vec![1, 2, 3] };
    let lm = LiveModel::new_gekisou(&m, &deck, &[], &[], params(), &setup).unwrap();
    let order: Vec<(usize, i64, Option<i64>)> =
        lm.cond.iter().map(|c| (c.member, c.skill_type, c.updater.gate_mission())).collect();
    // Combo list in member order (All at member 1, Combo at member 3), then Luck, then JustCount; supports last
    assert_eq!(
        order,
        [
            (1, SKILL_TYPE_GEKISOU, Some(4)),
            (3, SKILL_TYPE_GEKISOU, Some(1)),
            (2, SKILL_TYPE_GEKISOU, Some(2)),
            (0, SKILL_TYPE_GEKISOU, Some(3)),
            (3, SKILL_TYPE_GEKISOU_SUPPORT, Some(2)),
        ]
    );
}

#[test]
fn gekisou_mission_groups() {
    assert_eq!(gekisou_mission_group(1).unwrap(), 1);
    assert_eq!(gekisou_mission_group(3).unwrap(), 3);
    assert_eq!(gekisou_mission_group(4).unwrap(), 1);
    for m in [0, 5, -1] {
        assert!(matches!(gekisou_mission_group(m), Err(Error::Game(_))));
    }
}
