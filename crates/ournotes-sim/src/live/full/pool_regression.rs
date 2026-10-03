//! Test-only probes of effect-pool scheduling.
use super::*;
use serde_json::{Value, json};

fn pool_model(events: &[(i32, i32)], second: bool) -> LiveModel {
    let mut tables = json!({
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveSettings":[{"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},{"_id":4,"_key":"life_denger","_value":"300"}],
        "MasterSkillEffectSetting":[{"_id":1,"_skillEffectType":2001,"_phase":1},{"_id":2,"_skillEffectType":2003,"_phase":2}],
        "MasterSkillTarget":[{"_id":1,"_judgement":5}],
        "MasterSkillCumulativeCondition":[{"_id":1,"_skillCumulativeConditionType":1000,"_conditionValues":[1],"_conditionTargetIDs":[1],"_maxCumulativeCount":0}]
    });
    tables["MasterLiveSkillEffect"] = Value::Array(
        (0..if second { 2 } else { 1 })
            .map(|i| {
                json!({
                    "_id":i+1,"_liveSkillID":1,"_level":1,"_skillEffectType":if i==0 {2001}else{2003},
                    "_effectValue":1000,"_activationTimeSecond":if i==0 {0.01}else{0.1},"_skillCumulativeConditionID":1
                })
            })
            .collect(),
    );
    let texts: Vec<_> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    let master = Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str())).unwrap();
    let notes: Vec<_> =
        (0..6).map(|i| LiveNote { note_id: i + 1, time_ms: i * 40, note_operate_type: 1, judgement_type: 1 }).collect();
    LiveModel::new(
        &master,
        &[Performer { live_skill: Some((1, 1)), ..Default::default() }],
        &notes,
        events,
        LiveParams {
            total_power: 200000,
            music_level: 25,
            converted_note_count: 6,
            music_length_ms: 12000,
            skill_target_music_type: 0,
            score_music_length_ms: None,
            assist_factor: 1.0,
        },
    )
    .unwrap()
}
fn judged(id: i32, time: i32) -> [JudgedNote; 1] {
    [JudgedNote { note_id: id, judgement: 5, judgement_time_ms: time }]
}
fn raw(effect: &LiveEffect) -> i32 {
    match effect.cumulative.as_ref().unwrap() {
        Cumulative::JudgementPerN { count, .. } => *count,
        _ => panic!("wrong checker"),
    }
}

#[test]
fn sixth_sequential_execution_is_fifo_and_preserves_counter() {
    let events: Vec<_> = (0..6).map(|i| (0, i * 20)).collect();
    let mut m = pool_model(&events, false);
    for i in 0..6 {
        let time = i * 20;
        m.frame(time, &judged(i + 1, time)).unwrap();
        assert_eq!(m.enabled_live, vec![(i % 5) as usize]);
        let e = &m.live[(i % 5) as usize].effects[0];
        assert_eq!(e.state.cumulative_count, if i == 5 { 2 } else { 1 });
        assert_eq!(raw(e), if i == 5 { 2 } else { 1 });
        m.frame(time + 1, &[]).unwrap();
        m.frame(time + 11, &[]).unwrap();
        assert!(m.enabled_live.is_empty());
    }
    assert_eq!(m.live_pools[0].available, VecDeque::from([1, 2, 3, 4, 0]));
}

#[test]
fn early_child_reset_and_parent_stay_have_distinct_cumulative_history() {
    let mut m = pool_model(&[(0, 0)], true);
    m.frame(0, &judged(1, 0)).unwrap();
    m.frame(1, &[]).unwrap();
    m.frame(11, &[]).unwrap();
    assert_eq!(m.live[0].effects[0].state.state, END_FRAME);
    assert_eq!(m.live[0].effects[1].state.state, EXECUTING);
    m.frame(12, &[]).unwrap();
    let a = &m.live[0].effects[0];
    let b = &m.live[0].effects[1];
    assert_eq!((a.state.state, raw(a), a.state.cumulative_count), (STAY, 0, 1));
    assert_eq!((raw(b), b.state.cumulative_count), (1, 1));
    m.frame(101, &[]).unwrap();
    assert_eq!(m.live[0].parent_state, END_FRAME);
    m.frame(102, &[]).unwrap();
    let b = &m.live[0].effects[1];
    assert_eq!((m.live[0].parent_state, b.state.state, raw(b), b.state.cumulative_count), (STAY, STAY, 1, 1));
}

#[test]
fn release_during_frame_cannot_rescue_an_empty_pool_at_trigger_time() {
    let events = [(0, 0), (0, 1), (0, 2), (0, 3), (0, 4), (0, 15)];
    let mut m = pool_model(&events, false);
    for time in 0..5 {
        m.frame(time, &[]).unwrap();
    }
    assert!(m.live_pools[0].available.is_empty());
    assert!(matches!(m.frame(15, &[]), Err(Error::Game(_))));
    let mut control = pool_model(&events, false);
    for time in 0..5 {
        control.frame(time, &[]).unwrap();
    }
    control.frame(14, &[]).unwrap();
    assert!(!control.live_pools[0].available.is_empty());
    control.frame(15, &[]).unwrap();
}

#[test]
fn extension_survives_fifo_reuse_without_being_applied_again() {
    let events: Vec<_> = (0..6).map(|i| (0, i * 40)).collect();
    let mut m = pool_model(&events, false);
    for i in 0..5 {
        let time = i * 40;
        m.frame(time, &[]).unwrap();
        if i == 0 {
            m.extend(0, 10.0);
        }
        m.frame(time + 1, &[]).unwrap();
        m.frame(time + 21, &[]).unwrap();
        assert!(m.enabled_live.is_empty());
    }
    m.frame(200, &[]).unwrap();
    assert_eq!(m.enabled_live, vec![0]);
    assert_eq!(m.live[0].effects[0].state.extended_ms, 10.0);
    m.frame(201, &[]).unwrap();
    m.frame(220, &[]).unwrap();
    assert_eq!(m.live[0].effects[0].state.state, EXECUTING);
    m.frame(221, &[]).unwrap();
    assert!(m.enabled_live.is_empty());
    assert_eq!(m.live[0].effects[0].state.extended_ms, 10.0);
}

#[test]
fn coalesced_same_member_events_use_last_matching_time() {
    let mut m = pool_model(&[(0, 0), (0, 10)], false);
    m.frame(10, &[]).unwrap();
    assert_eq!(m.enabled_live.len(), 1);
    assert_eq!(m.live[0].effects[0].state.execute_ms, 10);
}
