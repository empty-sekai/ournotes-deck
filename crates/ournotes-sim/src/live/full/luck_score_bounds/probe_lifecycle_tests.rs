//! Sustained score probes across the declared music boundary.
use super::*;
use crate::live::full::{JudgedNote, PlayFrame};
use serde_json::json;

fn fixture(
    value: i64,
    music_length_ms: i32,
) -> (Master, Vec<Performer>, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let lots: Vec<_> = (0..5)
        .map(|kind| {
            json!({
                "_id":kind+1,"_chanceLotType":kind,"_lotResult":3,"_weight":1
            })
        })
        .collect();
    let tables = json!({
        "MasterLiveSettings":[
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"0"}],
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":140}],
        "MasterLiveGekisouLuckBonusLot":lots,
        "MasterSkillCondition":[{"_id":1,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}],
        "MasterSkillConditionSet":[{"_id":1,"_group":1,"_conditionIds":[1]}],
        "MasterSkillEffectSetting":[{"_id":1,"_skillEffectType":2000,"_phase":1}],
        "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2}],
        "MasterGekisouSkillEffect":[{
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":2,
            "_skillTriggerConditionGroup":1,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.0,"_effectValue":value,
            "_maxEffectValue":0,"_effectLimitCount":1,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]
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
    let deck = vec![Performer { gekisou_skill: Some((1, 1)), ..Default::default() }];
    let notes = vec![LiveNote { note_id: 1, time_ms: 1050, note_operate_type: 1, judgement_type: 1 }];
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 1000,
        music_level: 20,
        converted_note_count: 1,
        music_length_ms,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers: vec![(100, 1300)], missions: vec![2, 2, 2] };
    let frames = (0..=22)
        .map(|i| PlayFrame {
            time_ms: i * 100,
            judged: if i == 11 {
                vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 1050 }]
            } else {
                Vec::new()
            },
        })
        .collect();
    (master, deck, notes, params, setup, LivePlay { frames, base_seed: 0 }, vec![0.1; 23])
}

#[test]
fn probe_end_can_precede_start_after_music_length() {
    for value in [-8000, 8000] {
        let (master, deck, notes, params, setup, play, delta) = fixture(value, 1000);
        let skills = luck_skills(&master).unwrap();
        let mut model = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        check_recorder(&model, &skills).unwrap();
        let mut edges = Vec::new();
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            model.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            let state = &model.cond[0].updater.updaters[0].state;
            if matches!(state.state, super::super::EXECUTE_FRAME | super::super::END_FRAME) {
                edges.push((frame.time_ms, state.state, state.execute_ms, state.finish_ms));
            }
        }
        assert_eq!(edges.len(), 2, "{edges:?}");
        assert_eq!(edges[0].2, 1200);
        assert_eq!(edges[1].3, 1000);
        assert!(edges[1].3 < edges[0].2);
        assert_eq!(model.gk.as_ref().unwrap().ctrl.states[0].state, super::super::gekisou::S_FINISH);
        let (_, states) = model.score.executed_states();
        assert_eq!(states.len(), 1);
        let factor = states[0].1[1];
        assert!((factor - (1.0 - value as f32 / 10000.0)).abs() < 1e-5, "{states:?}");
        eprintln!("value={value} score={} factor={factor} edges={edges:?}", model.score());
    }
}

#[test]
fn probe_enclosures_cover_declared_music_boundary() {
    let mut misses = Vec::new();
    for music_length in [2400, 1000] {
        for value in [-8000, 8000] {
            let (master, deck, notes, params, setup, play, delta) = fixture(value, music_length);
            let mut model = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
            let native = model.run_timed(&play, &delta).unwrap();
            let certificate = luck_score_bounds(&master, &deck, &notes, &[], params, &setup, &play, &delta);
            match certificate {
                Ok(bounds) => {
                    eprintln!(
                        "music={music_length} value={value} native={native} mean={:?} support={:?}",
                        bounds.final_mean, bounds.final_support
                    );
                    if !(bounds.final_support.lower <= native
                        && native <= bounds.final_support.upper
                        && bounds.final_mean.lower <= f64::from(native)
                        && f64::from(native) <= bounds.final_mean.upper)
                    {
                        misses.push((music_length, value, native, bounds.final_support, bounds.final_mean));
                    }
                }
                Err(Error::Unsupported(reason)) if music_length == 1000 => {
                    eprintln!("music={music_length} value={value} native={native} unsupported={reason}");
                }
                Err(error) => panic!("{error}"),
            }
        }
    }
    assert!(misses.is_empty(), "{misses:?}");
}

#[test]
fn terminal_support_retains_clamped_probe_commands() {
    for value in [-8000, 8000] {
        let (master, deck, notes, params, setup, play, delta) = fixture(value, 1000);
        let mut model = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        let native = model.run_timed(&play, &delta).unwrap();
        let mut session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
        let support = session.support(&deck, &mut LuckExactBudget::default(), None, || false).unwrap();
        assert_eq!(support.decline, None);
        assert_eq!(support.outcomes, [(native, model.current_life())]);
    }
}

#[test]
fn probe_started_before_music_retains_later_note_clamp_semantics() {
    let mut misses = Vec::new();
    for value in [-8000, 8000] {
        let (master, deck, mut notes, mut params, setup, mut play, delta) = fixture(value, 1000);
        notes.insert(0, LiveNote { note_id: 0, time_ms: 500, note_operate_type: 1, judgement_type: 1 });
        params.converted_note_count = notes.len() as i32;
        for frame in &mut play.frames {
            frame.judged = notes
                .iter()
                .filter(|note| frame.time_ms - 100 < note.time_ms && note.time_ms <= frame.time_ms)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect();
        }
        let skills = luck_skills(&master).unwrap();
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        let mut starts = Vec::new();
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            let state = &native.cond[0].updater.updaters[0].state;
            if state.state == super::super::EXECUTE_FRAME {
                starts.push(state.execute_ms);
            }
        }
        assert_eq!(starts, [600]);
        let curve = super::super::luck_rush_dp_certified_with_ranking(
            &master,
            &skills,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            &deck,
            None,
            None,
        )
        .unwrap();
        let score = native.score();
        let (_, native_states) = native.score.executed_states();
        let result = luck_score_bounds(&master, &deck, &notes, &[], params, &setup, &play, &delta);
        match result {
            Ok(bounds) => {
                eprintln!(
                    "value={value} native={score} states={native_states:?} curve={:?} mean={:?} support={:?}",
                    curve.steps, bounds.final_mean, bounds.final_support
                );
                if !(bounds.final_support.lower <= score
                    && score <= bounds.final_support.upper
                    && bounds.final_mean.lower <= f64::from(score)
                    && f64::from(score) <= bounds.final_mean.upper)
                {
                    misses.push((value, score, bounds.final_support, bounds.final_mean));
                }
            }
            Err(Error::Unsupported(reason)) => eprintln!("value={value} native={score} unsupported={reason}"),
            Err(error) => panic!("{error}"),
        }
    }
    assert!(misses.is_empty(), "{misses:?}");
}

#[test]
fn probe_boundary_guard_requires_every_late_transition_to_stay_off_and_all_native_events() {
    let play = LivePlay {
        frames: [0, 100, 200, 300].into_iter().map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }).collect(),
        base_seed: 0,
    };
    let mut curve = LuckDpCertifiedResult {
        rush_filings: Vec::new(),
        probe_transitions: vec![2, 4, 1, 1],
        steps: Vec::new(),
        probes: vec![true],
        peak_states: 1,
        transitions: 0,
    };
    let mut trace = BoundsTrace {
        events: [0, 100, 200, 300, 200]
            .into_iter()
            .map(|time_ms| BoundsEvent::Probe { frame: get_frame(time_ms) as usize, time_ms })
            .collect(),
        queries: 0,
        frames: 100,
        probes: vec![ProbeRow { owner: 1, value: 0.1 }],
        combo: Default::default(),
        has_luck: true,
        rush_before: None,
    };
    assert!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_ok());
    for mask in 0..=16 {
        curve.probe_transitions[3] = mask;
        assert_eq!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_ok(), mask == 1, "mask={mask}");
    }
    curve.probe_transitions[3] = 1;
    curve.probe_transitions[0] = 0;
    assert!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_err());
    curve.probe_transitions[0] = 2;
    assert!(check_probe_music_boundary(&play, &curve, 200, false, &trace).is_err());
    curve.probe_transitions.pop();
    assert!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_err());
    curve.probe_transitions.push(1);
    trace.events.pop();
    assert!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_err());
    trace.events.push(BoundsEvent::Probe { frame: 0, time_ms: 200 });
    assert!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_err());
    trace.events.pop();
    trace.events.push(BoundsEvent::Probe { frame: get_frame(200) as usize, time_ms: 200 });
    trace.events.push(BoundsEvent::Probe { frame: get_frame(200) as usize, time_ms: 200 });
    assert!(check_probe_music_boundary(&play, &curve, 200, true, &trace).is_err());
    // An absent probe set has no probe lifetime to invert; ordinary commands keep their own admission.
    trace.probes.clear();
    assert!(check_probe_music_boundary(&play, &curve, 200, false, &trace).is_ok());
    let ending = LivePlay { frames: play.frames[..3].to_vec(), base_seed: 0 };
    trace.probes.push(ProbeRow { owner: 1, value: 0.1 });
    assert!(check_probe_music_boundary(&ending, &curve, 200, false, &trace).is_ok());
}

#[test]
fn probe_boundary_guard_preserves_native_normal_and_proved_inactive_tail_scores() {
    for music_length in [2400, 2100] {
        for value in [-8000, 8000] {
            let (master, deck, notes, params, setup, play, delta) = fixture(value, music_length);
            let skills = luck_skills(&master).unwrap();
            let curve = super::super::luck_rush_dp_certified_with_ranking(
                &master,
                &skills,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                &deck,
                None,
                None,
            )
            .unwrap();
            assert!(
                play.frames
                    .iter()
                    .zip(&curve.probe_transitions)
                    .filter(|(frame, _)| frame.time_ms > music_length)
                    .all(|(_, &mask)| mask == 1)
            );
            let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
            let score = native.run_timed(&play, &delta).unwrap();
            let bound = luck_score_bounds(&master, &deck, &notes, &[], params, &setup, &play, &delta).unwrap();
            assert!(bound.final_support.lower <= score && score <= bound.final_support.upper);
            assert!(bound.final_mean.lower <= f64::from(score) && f64::from(score) <= bound.final_mean.upper);
        }
    }
}

#[test]
fn probe_boundary_guard_refuses_mixed_phases_even_when_the_tail_is_physically_inactive() {
    let (mut master, deck, notes, params, setup, play, delta) = fixture(8000, 2100);
    let mut second = master.gekisou_skill_effects[0].clone();
    second.id = 2;
    second.skill_effect_type = 2005;
    master.gekisou_skill_effects.push(second);
    master
        .skill_effect_settings
        .push(serde_json::from_value(json!({"_id":2,"_skillEffectType":2005,"_phase":2})).unwrap());
    master.reindex().unwrap();
    let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    native.run_timed(&play, &delta).unwrap();
    assert!(matches!(
        luck_score_bounds(&master, &deck, &notes, &[], params, &setup, &play, &delta),
        Err(Error::Unsupported(_))
    ));
}
