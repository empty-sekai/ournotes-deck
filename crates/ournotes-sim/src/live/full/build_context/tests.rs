use super::*;
use crate::master::{SkillConditionSetRow, SupportSkillEffectRow};
use serde_json::json;

pub(super) fn fixture() -> (Master, Vec<Performer>, Vec<LiveNote>, LiveParams, GekisouSetup) {
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
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":0}],
        "MasterLiveGekisouLuckBonusLot": (0..5).map(|kind| json!({
            "_id":kind+1,"_chanceLotType":kind,"_lotResult":0,"_weight":1
        })).collect::<Vec<_>>(),
        "MasterSkillTarget":[{"_id":56,"_skillTargetType":5,"_gekisouMissionType":2}],
        "MasterSkillCondition":[
            {"_id":1,"_conditionType":4010,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true},
            {"_id":2,"_conditionType":4000,"_conditionValues":[1],"_conditionTargetIDs":[],"_isPositive":true},
            {"_id":3,"_conditionType":8000,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":false},
            {"_id":4,"_conditionType":7010,"_conditionValues":[],"_conditionTargetIDs":[56],"_isPositive":true}],
        "MasterSkillConditionSet":[
            {"_id":30,"_group":1,"_conditionIds":[1]},
            {"_id":20,"_group":2,"_conditionIds":[2,3]},
            {"_id":10,"_group":2,"_conditionIds":[1]},
            {"_id":40,"_group":4,"_conditionIds":[4]}],
        "MasterLiveSkillEffect":[
            {"_id":3,"_liveSkillID":1,"_level":1,"_skillEffectType":2000,"_effectValue":3333,"_activationTimeSecond":0.2},
            {"_id":2,"_liveSkillID":1,"_level":2,"_skillEffectType":2000,"_effectValue":9911,"_activationTimeSecond":0.1},
            {"_id":1,"_liveSkillID":1,"_level":1,"_skillEffectType":2000,"_effectValue":997,"_activationTimeSecond":0.3}],
        "MasterSupportSkillEffect":[
            {"_id":7,"_supportSkillID":2,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":1,
                "_skillReleaseConditionGroup":2,"_skillEffectType":2000,"_effectValue":1234,"_activationTimeSecond":0.2},
            {"_id":5,"_supportSkillID":2,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":1,
                "_skillEffectType":3001,"_effectValue":7}],
        "MasterGekisouSkill":[{"_id":3,"_gekisouMissionType":2},{"_id":4,"_gekisouMissionType":4}],
        "MasterGekisouSkillEffect":[
            {"_id":11,"_gekisouSkillID":3,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":4,
                "_skillEffectType":11001,"_effectValue":333,"_activationTimeSecond":0.2},
            {"_id":12,"_gekisouSkillID":4,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":4,
                "_skillEffectType":2000,"_effectValue":1234,"_activationTimeSecond":0.1}],
        "MasterGekisouSupportSkill":[{"_id":5,"_gekisouMissionType":2}],
        "MasterGekisouSupportSkillEffect":[
            {"_id":21,"_gekisouSupportSkillID":5,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":4,
                "_skillEffectType":2000,"_effectValue":995,"_activationTimeSecond":0.3}]
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
    let deck = vec![
        Performer {
            live_skill: Some((1, 1)),
            support_skills: vec![(2, 1)],
            gekisou_skill: Some((3, 1)),
            gekisou_support_skills: vec![(5, 1)],
            character_id: 1,
            ..Default::default()
        },
        Performer { live_skill: Some((1, 2)), gekisou_skill: Some((4, 1)), character_id: 2, ..Default::default() },
        Performer { live_skill: Some((1, 1)), support_skills: vec![(2, 1)], character_id: 3, ..Default::default() },
        Performer { character_id: 4, ..Default::default() },
        Performer { character_id: 5, ..Default::default() },
    ];
    let notes = (1..=5)
        .map(|note_id| LiveNote { note_id, time_ms: note_id * 100, note_operate_type: 1, judgement_type: 1 })
        .collect();
    let params = LiveParams {
        total_power: 12345,
        music_level: 20,
        converted_note_count: 5,
        music_length_ms: 1600,
        skill_target_music_type: 0,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers: vec![(100, 500)], missions: vec![2, 2, 2] };
    (master, deck, notes, params, setup)
}

fn identity(mut model: LiveModel) -> String {
    super::super::luck_exact::initialized_identity(&mut model).expect("complete finite native identity")
}

#[test]
fn source_indexes_keep_complete_initialized_state_and_native_frame_history() {
    let (master, mut deck, notes, params, setup) = fixture();
    let contexts = [BuildContext::new(&master), BuildContext::try_bounded(&master, 1 << 20).unwrap()];
    let events = [(0, 100), (1, 100), (2, 220), (0, 400)];
    for _ in 0..5 {
        for setup in [None, Some(&setup)] {
            for context in &contexts {
                let mut reference =
                    LiveModel::build(&master, &deck, &notes, &events, params, setup, false, None, None).unwrap();
                let mut indexed =
                    LiveModel::build_with_context(context, &deck, &notes, &events, params, setup, false, None, None)
                        .unwrap();
                assert_eq!(identity(indexed.clone()), identity(reference.clone()));
                for time in (0..=1600).step_by(20) {
                    let judged: Vec<_> = notes
                        .iter()
                        .filter(|note| note.time_ms == time)
                        .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                        .collect();
                    reference.frame_timed(time, &judged, 0.02).unwrap();
                    indexed.frame_timed(time, &judged, 0.02).unwrap();
                    assert_eq!(indexed.trace(), reference.trace());
                    assert_eq!(indexed.current_life(), reference.current_life());
                    assert_eq!(indexed.draws(), reference.draws());
                }
                assert_eq!(identity(indexed), identity(reference));
            }
        }
        deck.rotate_left(1);
    }
}

#[test]
fn source_indexes_keep_duplicate_rows_and_constructor_error_priority() {
    let (mut master, deck, notes, params, setup) = fixture();
    master.support_skill_effects[0].skill_release_condition_group = 999;
    master.skill_condition_sets.push(SkillConditionSetRow { id: 999, group: 999, condition_ids: vec![999] });
    master.support_skill_effects.push(master.support_skill_effects[0].clone());
    let check = |master: &Master| {
        let reference = LiveModel::new_gekisou(master, &deck, &notes, &[], params, &setup).unwrap_err();
        for context in [BuildContext::new(master), BuildContext::try_bounded(master, 1 << 20).unwrap()] {
            let indexed =
                LiveModel::build_with_context(&context, &deck, &notes, &[], params, Some(&setup), false, None, None)
                    .unwrap_err();
            assert_eq!(indexed, reference);
        }
        reference
    };
    assert_eq!(check(&master), Error::Master("condition skill with a duplicate effect id".into()));
    master.support_skill_effects.pop();
    assert_eq!(check(&master), Error::Master("unknown skill condition 999".into()));
    master.support_skill_effects[0].skill_trigger_type = 91;
    assert_eq!(check(&master), Error::Game("ArgumentOutOfRangeException: skill trigger type 91".into()));
}

#[test]
fn edited_and_filtered_masters_receive_fresh_indexes_and_complete_identities() {
    let (mut master, deck, notes, params, setup) = fixture();
    let built = |master: &Master| {
        let reference = identity(LiveModel::new_gekisou(master, &deck, &notes, &[], params, &setup).unwrap());
        for context in [BuildContext::new(master), BuildContext::try_bounded(master, 1 << 20).unwrap()] {
            let indexed =
                LiveModel::build_with_context(&context, &deck, &notes, &[], params, Some(&setup), false, None, None)
                    .unwrap();
            let key = identity(indexed);
            assert_eq!(key, reference);
        }
        reference
    };
    let before = built(&master);
    // Effect rows and condition-set groups are selected from the current tables, without relying on reindex.
    master.live_skill_effects[0].effect_value += 1;
    master.skill_condition_sets.swap(1, 2);
    let edited = built(&master);
    assert_ne!(before, edited);
    let mut filtered = master.clone();
    filtered.live_skill_effects.retain(|row| row.skill_effect_type == 3001);
    filtered.support_skill_effects.retain(|row| row.skill_effect_type == 3001);
    filtered.gekisou_skill_effects.retain(|row| row.skill_effect_type == 11001);
    filtered.gekisou_support_skill_effects.clear();
    assert_ne!(edited, built(&filtered));
    assert_eq!(edited, built(&master));
    master.support_skill_effects.push(SupportSkillEffectRow {
        id: 70,
        support_skill_id: 2,
        level: 1,
        skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: 1,
        skill_effect_type: 3001,
        effect_value: 3,
        ..Default::default()
    });
    assert_ne!(edited, built(&master));
}

#[test]
fn release_templates_keep_each_pool_counter_independent_and_duplicate_errors_first() {
    let effect = |effect_id| CondEffect {
        effect_id,
        trigger_type: ONE_SHOT,
        act: 1.0,
        phase: 1,
        trigger: Some(Checker::SameMemberLiveSkill(0)),
        condition: None,
        execute_limit: 0,
        reset: None,
        row: 0,
        cumulative: None,
    };
    let mut calls = Vec::new();
    let mut updater = ConditionSkillUpdater::new(
        vec![effect(1), effect(2)],
        |index| {
            calls.push(index);
            Ok(Some(Checker::NoteJudgementCount {
                n: 2,
                consecutive: false,
                targets: vec![5],
                count: 0,
                override_ms: None,
            }))
        },
        None,
    )
    .unwrap();
    assert_eq!(calls, [0, 1]);
    let mut life = LifeController::new(1000, FxHashMap::default(), 1000).unwrap();
    let mut random = LiveRandom::new(0);
    let judged = [(1, 5, 100)];
    let mut ctx = CheckCtx {
        life: &mut life,
        random: &mut random,
        frame_time: 100,
        current_combo: 1,
        judged: &judged,
        events: &[],
        gk: None,
        prev_confirmed_rank: None,
    };
    assert_eq!(updater.updaters[0].release.as_mut().unwrap().check(&mut ctx).unwrap(), (false, 0));
    for (index, pool) in updater.updaters.iter().enumerate() {
        let Some(Checker::NoteJudgementCount { count, .. }) = &pool.release else { panic!("counter checker") };
        assert_eq!(*count, i32::from(index == 0));
    }
    calls.clear();
    let failure = ConditionSkillUpdater::new(
        vec![effect(1), effect(1)],
        |index| {
            calls.push(index);
            Err(Error::Master("release unavailable".into()))
        },
        None,
    )
    .unwrap_err();
    assert_eq!(failure, Error::Master("condition skill with a duplicate effect id".into()));
    assert!(calls.is_empty());
}
