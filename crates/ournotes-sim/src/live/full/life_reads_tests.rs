//! Backdated commands fully invalidate the native life cache; observation stays read-only.
use super::*;
use serde_json::json;

fn model(random_chain: bool, heal_phase: i64) -> LiveModel {
    let lots: Vec<_> =
        (0..5).map(|kind| json!({"_id":kind+1,"_chanceLotType":kind,"_lotResult":3,"_weight":1})).collect();
    let tables = json!({
        "MasterLiveSettings":[
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":1,"_scorePercent":0,"_damage":300}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":1,"_afterMs":100}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":1,"_weight":1,"_basePoint":0}],
        "MasterLiveGekisouLuckBonusLot":lots,
        "MasterSkillEffectSetting":[
            {"_id":1,"_skillEffectType":3001,"_phase":heal_phase},
            {"_id":2,"_skillEffectType":11003,"_phase":2}],
        "MasterLiveSkillEffect":[{"_id":1,"_liveSkillID":1,"_level":1,"_skillEffectType":3001,
            "_effectValue":125,"_activationTimeSecond":0.0}],
        "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2}],
        "MasterSkillTarget":[{"_id":56,"_skillTargetType":5,"_gekisouMissionType":2}],
        "MasterSkillCondition":[
            {"_id":1,"_conditionType":7010,"_conditionTargetIDs":[56]},
            {"_id":2,"_conditionType":4011,"_conditionValues":[50]},
            {"_id":3,"_conditionType":2000,"_conditionValues":[700]},
            {"_id":4,"_conditionType":7013}],
        "MasterSkillConditionSet":[
            {"_id":1,"_group":1,"_conditionIds":[1]},
            {"_id":2,"_group":2,"_conditionIds":[2,3]},
            {"_id":3,"_group":3,"_conditionIds":[4]}],
        "MasterGekisouSkillEffect":[{"_id":1,"_gekisouSkillID":1,"_level":1,
            "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillConditionGroup":2,
            "_skillReleaseConditionGroup":3,"_skillEffectType":11003,"_activationTimeSecond":0.0,"_effectValue":10000}]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()))
            .unwrap();
    let deck = [Performer {
        live_skill: Some((1, 1)),
        gekisou_skill: random_chain.then_some((1, 1)),
        gekisou_mission_type: 2,
        ..Default::default()
    }];
    let notes = [LiveNote { note_id: 1, time_ms: 145, note_operate_type: 1, judgement_type: 1 }];
    let params = LiveParams {
        total_power: 1000,
        music_level: 20,
        converted_note_count: 1,
        music_length_ms: 2000,
        score_music_length_ms: None,
        skill_target_music_type: 0,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers: vec![(155, 500)], missions: vec![2, 2, 2] };
    LiveModel::new_gekisou(&master, &deck, &notes, &[(0, 155)], params, &setup).unwrap()
}

fn damage_frame(model: &mut LiveModel) {
    model.frame_timed(149, &[JudgedNote { note_id: 1, judgement: 1, judgement_time_ms: 149 }], 0.016).unwrap();
    assert_eq!(model.current_life(), 700);
}

#[test]
fn extra_current_life_read_preserves_later_backdated_healing() {
    let mut native = model(false, 1);
    let mut extra_read = model(false, 1);
    damage_frame(&mut native);
    damage_frame(&mut extra_read);
    assert_eq!(extra_read.life.get_life_at_ms(166).unwrap(), 700);
    native.frame_timed(166, &[], 0.017).unwrap();
    extra_read.frame_timed(166, &[], 0.017).unwrap();
    assert_eq!(native.current_life(), 825);
    assert_eq!(extra_read.current_life(), 825);
}

#[test]
fn stochastic_chain_short_circuit_preserves_backdated_healing() {
    let mut lives = std::collections::BTreeSet::new();
    for seed in 0..64 {
        let mut native = model(true, 2);
        native.set_seed(seed);
        damage_frame(&mut native);
        native.frame_timed(166, &[], 0.017).unwrap();
        lives.insert(native.current_life());
    }
    assert_eq!(lives, std::collections::BTreeSet::from([825]));
}

#[test]
fn phase_life_snapshot_must_not_mutate_the_native_cache() {
    let mut native = model(false, 1);
    let mut observed = model(false, 1);
    observed.phase_life = Some([0; 2]);
    damage_frame(&mut native);
    damage_frame(&mut observed);
    assert_eq!(format!("{:?}", observed.life), format!("{:?}", native.life));
    native.frame_timed(166, &[], 0.017).unwrap();
    observed.frame_timed(166, &[], 0.017).unwrap();
    assert_eq!(observed.current_life(), native.current_life(), "a diagnostic phase snapshot is read-only");
    assert_eq!(observed.current_life(), 825);
    assert_eq!(format!("{:?}", observed.life), format!("{:?}", native.life));
}
