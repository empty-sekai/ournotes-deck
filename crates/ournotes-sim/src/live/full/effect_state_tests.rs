use super::*;
use serde_json::json;

fn guard_model() -> LiveModel {
    let tables = json!({
        "MasterLiveSettings": [
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"}],
        "MasterLiveSkillEffect": [{"_id":1,"_liveSkillID":1,"_level":1,
            "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
            "_skillEffectType":3003,"_activationTimeSecond":1.0,"_effectValue":0,
            "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]
    });
    let texts: Vec<_> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())).unwrap();
    let p = Performer { live_skill: Some((1, 1)), ..Default::default() };
    let params = LiveParams {
        total_power: 1,
        music_level: 1,
        converted_note_count: 1,
        music_length_ms: 1000,
        skill_target_music_type: 0,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    LiveModel::new(&master, &[p], &[], &[], params).unwrap()
}

#[test]
fn guard_applier_rejects_missing_and_duplicate_states() {
    let mut model = guard_model();
    let item = Listed::Live { skill: 0, effect: 0 };
    model.state_mut(item).state = END_FRAME;
    assert!(matches!(model.apply(item), Err(Error::Game(_))));
    model.state_mut(item).state = EXECUTE_FRAME;
    model.apply(item).unwrap();
    let ids = model.guards.clone();
    assert!(matches!(model.apply(item), Err(Error::Game(_))));
    assert_eq!(model.guards, ids); // Dictionary.Add keeps the old value when a duplicate is rejected.
    model.state_mut(item).state = END_FRAME;
    model.apply(item).unwrap();
    assert!(model.guards.is_empty());
    assert!(matches!(model.apply(item), Err(Error::Game(_))));
}
