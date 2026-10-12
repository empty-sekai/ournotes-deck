use super::dp::{Chain, Dp, State};
use super::record::{consume_bound, record_native};
use super::*;

mod state_identity_tests {
    include!("state_identity_tests.rs");
}

mod effect_identity_tests {
    include!("effect_identity_tests.rs");
}

mod kinds_tests {
    include!("kinds_tests.rs");
}
use crate::live::certified::F64Interval;
use serde_json::{Value, json};

fn assert_certified_encloses_nominal(certified: &LuckDpCertifiedResult, nominal: &LuckDpResult) {
    for &(time, joint) in certified.steps.iter() {
        let total = joint.iter().fold(F64Interval::ZERO, |sum, mass| sum.add(mass.interval()));
        assert!(total.contains(1.0), "t={time} total={total:?}");
        let values = &nominal.steps[nominal.steps.partition_point(|step| step.0 <= time) - 1].1;
        let rush = joint[2].merge_disjoint(joint[3]).interval();
        assert!((rush.lower() as f32..=rush.upper() as f32).contains(&values[0]));
        for (shape, &enabled) in certified.probes.iter().enumerate() {
            let score = if enabled { joint[1].merge_disjoint(joint[3]) } else { ProbabilityMass::ZERO }.interval();
            let both = if enabled { joint[3] } else { ProbabilityMass::ZERO }.interval();
            assert!((score.lower() as f32..=score.upper() as f32).contains(&values[1 + 2 * shape]));
            assert!((both.lower() as f32..=both.upper() as f32).contains(&values[2 + 2 * shape]));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn row(id: i64, key: &str, skill: i64, effect: i64, value: i64, trigger: i64, condition: i64, release: i64) -> Value {
    json!({"_id":id,key:skill,"_level":1,"_skillTriggerType":1,
        "_skillTriggerConditionGroup":trigger,"_skillConditionGroup":condition,"_skillReleaseConditionGroup":release,
        "_skillTargetIDs":[],"_skillEffectType":effect,"_activationTimeSecond":0.0,"_effectValue":value,
        "_maxEffectValue":0,"_effectLimitCount":1,"_skillCumulativeConditionID":0,
        "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0})
}

#[test]
fn ordinary_and_gekisou_conversions_prevent_a_chart_only_luck_curve() {
    let tables = json!({
        "MasterLiveSkillEffect":[row(1,"_liveSkillID",41,12006,10000,0,0,0)],
        "MasterSupportSkillEffect":[row(2,"_supportSkillID",42,13005,10000,0,0,0)],
        "MasterGekisouSkillEffect":[row(3,"_gekisouSkillID",43,12006,10000,0,0,0)],
        "MasterGekisouSupportSkillEffect":[row(4,"_gekisouSupportSkillID",44,12006,10000,0,0,0)]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
            .unwrap();
    for performer in [
        Performer { live_skill: Some((41, 1)), ..Default::default() },
        Performer { support_skills: vec![(42, 1)], ..Default::default() },
        Performer { gekisou_skill: Some((43, 1)), ..Default::default() },
        Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(44, 1)], ..Default::default() },
    ] {
        assert!(luck_has_judgement_conversion(&master, &[performer]));
    }
    assert!(!luck_has_judgement_conversion(&master, &[Performer::default()]));
    assert!(!luck_has_judgement_conversion(&master, &[Performer { live_skill: Some((41, 2)), ..Default::default() }]));
    // Gekisou Snap effects have no controller to run on when their member has no Gekisou skill.
    assert!(!luck_has_judgement_conversion(
        &master,
        &[Performer { gekisou_support_skills: vec![(44, 1)], ..Default::default() }]
    ));
}

#[test]
fn only_conversions_that_can_change_a_judgement_of_the_play_count() {
    let mut great = row(1, "_supportSkillID", 42, 12006, 5, 0, 0, 0);
    great["_skillTargetIDs"] = json!([1]);
    let mut perfect = row(2, "_supportSkillID", 43, 13005, 0, 0, 0, 0);
    perfect["_skillTargetIDs"] = json!([2]);
    let mut missing = row(3, "_supportSkillID", 44, 12006, 5, 0, 0, 0);
    missing["_skillTargetIDs"] = json!([99]);
    let tables = json!({
        "MasterSupportSkillEffect":[great, perfect, missing],
        "MasterSkillTarget":[{"_id":1,"_skillTargetType":2,"_judgement":4},{"_id":2,"_skillTargetType":2,"_judgement":5}],
        "MasterLiveJudgementTiming":[
            {"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6},
            {"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":5},
            {"_id":3,"_noteJudgementType":2,"_noteSimulateJudgement":5}]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
            .unwrap();
    // Note 1 has a Just timing, note 2 does not.
    let notes = [1, 2].map(|note_id| LiveNote {
        note_id,
        time_ms: 100 * note_id,
        note_operate_type: 1,
        judgement_type: note_id,
    });
    let play = |judgements: [i32; 2]| LivePlay {
        frames: vec![PlayFrame {
            time_ms: 300,
            judged: notes
                .iter()
                .zip(judgements)
                .map(|(note, judgement)| JudgedNote {
                    note_id: note.note_id,
                    judgement,
                    judgement_time_ms: note.time_ms,
                })
                .collect(),
        }],
        base_seed: 0,
    };
    let holding = |skill: i64| [Performer { support_skills: vec![(skill, 1)], ..Default::default() }];
    // To Perfect from Great: only a Great changes.
    assert!(!luck_play_converts(&master, &holding(42), &notes, &play([6, 5])));
    assert!(luck_play_converts(&master, &holding(42), &notes, &play([4, 5])));
    // To Just from Perfect: not a note without a Just timing.
    assert!(!luck_play_converts(&master, &holding(43), &notes, &play([6, 5])));
    assert!(luck_play_converts(&master, &holding(43), &notes, &play([5, 5])));
    // A target missing from the master may be any judgement.
    assert!(luck_play_converts(&master, &holding(44), &notes, &play([6, 6])));
    assert!(!luck_play_converts(&master, &[Performer::default()], &notes, &play([4, 4])));
}

fn fixture(result: i64, base: i64) -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let mut speed = row(1, "_gekisouSkillID", 1, 11001, 20000, 7010, 0, 0);
    speed["_activationTimeSecond"] = json!(0.2);
    let mut sustained = row(3, "_gekisouSkillID", 3, 11001, 20000, 7020, 0, 7013);
    sustained["_skillTriggerType"] = json!(2);
    let mut score = row(31, "_gekisouSupportSkillID", 31, 2000, 10000, 7021, 0, 0);
    score["_skillTriggerType"] = json!(2);
    let mut miss = row(96, "_gekisouSupportSkillID", 96, 11003, 10000, 7000, 0, 7013);
    miss["_effectExecuteLimitCount"] = json!(1);
    miss["_effectExecuteLimitResetConditionGroup"] = json!(7013);
    let lots: Vec<_> =
        (0..5).map(|kind| json!({"_id":kind+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1})).collect();
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
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":base}],
        "MasterLiveGekisouLuckBonusLot":lots,
        "MasterSkillTarget":[{"_id":56,"_skillTargetType":5,"_gekisouMissionType":2}],
        "MasterSkillCondition":[
            {"_id":4011,"_conditionType":4011,"_conditionValues":[100],"_conditionTargetIDs":[],"_isPositive":true},
            {"_id":7000,"_conditionType":7000,"_conditionValues":[0],"_conditionTargetIDs":[],"_isPositive":true},
            {"_id":7010,"_conditionType":7010,"_conditionValues":[],"_conditionTargetIDs":[56],"_isPositive":true},
            {"_id":7013,"_conditionType":7013,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true},
            {"_id":7020,"_conditionType":7020,"_conditionValues":[],"_conditionTargetIDs":[56],"_isPositive":true},
            {"_id":7021,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}],
        "MasterSkillConditionSet":[
            {"_id":1,"_group":4011,"_conditionIds":[4011]},
            {"_id":2,"_group":7000,"_conditionIds":[7000]},
            {"_id":3,"_group":7010,"_conditionIds":[7010]},
            {"_id":4,"_group":7013,"_conditionIds":[7013]},
            {"_id":5,"_group":7021,"_conditionIds":[7021]},
            {"_id":6,"_group":7020,"_conditionIds":[7020]}],
        "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2},{"_id":2,"_gekisouMissionType":2},{"_id":3,"_gekisouMissionType":2}],
        "MasterGekisouSkillEffect":[speed,sustained,row(2,"_gekisouSkillID",2,11003,10000,7010,4011,7013)],
        "MasterGekisouSupportSkill":[{"_id":31,"_gekisouMissionType":2},{"_id":66,"_gekisouMissionType":2},{"_id":96,"_gekisouMissionType":2}],
        "MasterGekisouSupportSkillEffect":[score,miss,row(66,"_gekisouSupportSkillID",66,11005,4,7010,4011,7013)]
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
    let notes: Vec<_> = [100, 110, 120, 200, 260, 300, 310, 400, 500, 800]
        .into_iter()
        .enumerate()
        .map(|(i, time_ms)| LiveNote { note_id: i as i32, note_operate_type: 1, judgement_type: 1, time_ms })
        .collect();
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 1000,
        music_level: 20,
        converted_note_count: notes.len() as i32,
        music_length_ms: 2000,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers: vec![(100, 500)], missions: vec![2, 2, 2] };
    let mut frames: Vec<_> = (0..=20).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
    for note in &notes {
        frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
            note_id: note.note_id,
            judgement: 5,
            judgement_time_ms: note.time_ms,
        });
    }
    let delta = vec![0.1; frames.len()];
    (master, notes, params, setup, LivePlay { frames, base_seed: 0 }, delta)
}

#[test]
fn deterministic_curves_match_native_with_same_frame_notes_and_probe_delay() {
    for result in [0, 3] {
        let (master, notes, params, setup, play, delta) = fixture(result, 60);
        let skills = luck_skills(&master).unwrap();
        for skill in [1, 2, 3] {
            let deck = [Performer {
                gekisou_skill: Some((skill, 1)),
                gekisou_support_skills: vec![(31, 1), (96, 1)],
                ..Default::default()
            }];
            let dp = luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
            let samples = luck::luck_rush_samples(
                &master,
                &skills,
                &notes,
                params,
                &setup,
                &play,
                &delta,
                &deck,
                None,
                &[0, 1, 7],
            )
            .unwrap();
            assert_eq!(dp.steps, samples, "result={result} skill={skill}");
            assert!(dp.peak_states >= 1 && dp.transitions > 0);
        }
    }
}

#[test]
fn range_moments_match_complete_lottery_indicators_and_critical_skills() {
    for result in 0..4 {
        let (mut master, notes, params, mut setup, play, delta) = fixture(result, 60);
        master.skill_conditions.push(
            serde_json::from_value(json!({
                "_id": 7003, "_conditionType": 7000, "_conditionValues": [3],
                "_conditionTargetIDs": [], "_isPositive": true,
            }))
            .unwrap(),
        );
        master.skill_condition_sets.push(
            serde_json::from_value(json!({
                "_id": 7, "_group": 7003, "_conditionIds": [7003],
            }))
            .unwrap(),
        );
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id: 77,
            gekisou_mission_type: 2,
            ..Default::default()
        });
        master
            .gekisou_support_skill_effects
            .push(serde_json::from_value(row(77, "_gekisouSupportSkillID", 77, 11002, 7, 7003, 0, 0)).unwrap());
        master.reindex().unwrap();
        setup.fevers = vec![(100, 300), (1100, 1200)];
        let deck = [Performer {
            gekisou_skill: Some((2, 1)),
            gekisou_mission_type: 2,
            gekisou_support_skills: vec![(77, 1)],
            ..Default::default()
        }];
        let skills = luck_skills(&master).unwrap();
        let dp = luck_rush_dp_certified_with_moments(
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
        let compact =
            luck_rush_dp_certified(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
        assert!(compact.range_moments.is_empty());
        let mut plain = master.clone();
        plain.gekisou_support_skill_effects.retain(|row| row.skill_id != 77);
        plain.reindex().unwrap();
        let without = luck_rush_dp_certified(
            &plain,
            &luck_skills(&plain).unwrap(),
            &notes,
            params,
            &setup,
            &play,
            &delta,
            &deck,
            None,
        )
        .unwrap();
        assert_eq!(curve_words(&compact), curve_words(&without));
        assert_eq!((compact.peak_states, compact.transitions), (without.peak_states, without.transitions));
        let mut cache = LuckDpCache::new(1 << 20);
        cache.certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None).unwrap();
        cache
            .certified(
                &plain,
                &luck_skills(&plain).unwrap(),
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
        assert_eq!(cache.stats().hits, 1);
        let indicators = cache
            .certified_with_moments(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None)
            .unwrap();
        assert!(!indicators.range_moments.is_empty());
        assert_eq!(cache.len(), 2);
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        native.run_timed(&play, &delta).unwrap();
        for (moments, range) in dp.range_moments.iter().zip(native.gekisou_ranges()) {
            assert!(
                moments.luck_points.contains(f64::from(range.luck_points)),
                "result={result}, range={range:?}, moments={moments:?}"
            );
            assert!(moments.luck_points.upper() - moments.luck_points.lower() < 1e-8);
            for (count, expected) in moments.lot_results.iter().zip(range.lot_results) {
                assert!(count.contains(f64::from(expected)));
                assert!(count.upper() - count.lower() < 1e-8);
            }
        }
    }
}

#[test]
fn one_consumed_lottery_has_exact_nominal_rewards_and_critical_points() {
    let (mut master, _, mut params, mut setup, _, _) = fixture(0, 140);
    master.gekisou_skill_effects.clear();
    master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            (0..4).map(move |result| crate::master::LuckBonusLotRow {
                id: kind * 4 + result + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: 1,
            })
        })
        .collect();
    master.skill_conditions.push(
        serde_json::from_value(json!({
            "_id": 7003, "_conditionType": 7000, "_conditionValues": [3],
            "_conditionTargetIDs": [], "_isPositive": true,
        }))
        .unwrap(),
    );
    master
        .skill_condition_sets
        .push(serde_json::from_value(json!({"_id": 7, "_group": 7003, "_conditionIds": [7003]})).unwrap());
    master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 77,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    master
        .gekisou_support_skill_effects
        .push(serde_json::from_value(row(77, "_gekisouSupportSkillID", 77, 11002, 7, 7003, 0, 0)).unwrap());
    master.reindex().unwrap();
    let notes = [LiveNote { note_id: 1, time_ms: 200, note_operate_type: 1, judgement_type: 1 }];
    params.converted_note_count = 1;
    setup.fevers = vec![(100, 400)];
    let mut frames: Vec<_> = (0..=20).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
    frames[2].judged.push(JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 200 });
    let delta = vec![0.1; frames.len()];
    let play = LivePlay { frames, base_seed: 0 };
    let deck = [Performer {
        gekisou_skill: Some((2, 1)),
        gekisou_mission_type: 2,
        gekisou_support_skills: vec![(77, 1)],
        ..Default::default()
    }];
    let skills = luck_skills(&master).unwrap();
    let dp = luck_rush_dp_certified_with_moments(
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
    let range = &dp.range_moments[0];
    for count in range.lot_results {
        assert!(count.contains(0.25), "{range:?}");
        assert!(count.upper() - count.lower() < 1e-10);
    }
    // One equally weighted outcome awards (0 + 5 + 10 + 10)/4, plus a seven-point
    // one-shot bonus in the next frame on the Critical branch only.
    assert!(range.luck_points.contains(8.0), "{range:?}");
    assert!(range.luck_points.upper() - range.luck_points.lower() < 1e-10);
}

#[test]
fn separate_luck_ranges_reset_the_chain_and_miss_limit_like_native() {
    for result in [0, 3] {
        let (master, mut notes, mut params, mut setup, _, _) = fixture(result, 60);
        setup.fevers.extend([(1500, 1900), (2900, 3300)]);
        params.music_length_ms = 4000;
        for start in [1500, 2900] {
            for offset in [0, 10, 20, 100, 160, 200, 210, 300, 400] {
                notes.push(LiveNote {
                    note_id: notes.len() as i32,
                    note_operate_type: 1,
                    judgement_type: 1,
                    time_ms: start + offset,
                });
            }
        }
        params.converted_note_count = notes.len() as i32;
        let mut frames: Vec<_> = (0..=40).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
        for note in &notes {
            frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
                note_id: note.note_id,
                judgement: 5,
                judgement_time_ms: note.time_ms,
            });
        }
        let play = LivePlay { frames, base_seed: 0 };
        let delta = vec![0.1; play.frames.len()];
        let skills = luck_skills(&master).unwrap();
        let deck = [Performer {
            gekisou_skill: Some((2, 1)),
            gekisou_support_skills: vec![(31, 1), (96, 1)],
            ..Default::default()
        }];
        let dp = luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
        let native =
            luck::luck_rush_samples(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None, &[0, 1, 7])
                .unwrap();
        assert_eq!(dp.steps, native, "result={result}");
    }
}

#[test]
fn guarantee_is_consumed_by_the_initial_predraw_not_the_following_result() {
    let (mut master, notes, params, setup, play, delta) = fixture(0, 60);
    for kind in 0..5 {
        master.gekisou_luck_bonus_lots.push(crate::master::LuckBonusLotRow {
            id: 100 + kind,
            chance_lot_type: kind,
            lot_result: 3,
            weight: 1,
        });
    }
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer {
        gekisou_skill: Some((2, 1)),
        gekisou_support_skills: vec![(31, 1), (66, 1)],
        ..Default::default()
    }];
    let dp = luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
    let seeds: Vec<_> = (0..4096).collect();
    let samples =
        luck::luck_rush_samples(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None, &seeds).unwrap();
    for &(time, ref values) in &dp.steps {
        let sample = &samples[samples.partition_point(|step| step.0 <= time) - 1].1;
        for (expected, observed) in values.iter().zip(sample) {
            assert!((expected - observed).abs() < 0.04, "t={time} expected={expected} sample={observed}");
        }
    }
}

#[test]
fn certified_first_draw_encloses_exact_weight_and_start_chance_fractions() {
    let (mut master, notes, params, setup, play, delta) = fixture(0, 60);
    // The initial 60 gauge cannot draw unless the 1/4 start-gauge effect succeeds. The first
    // independent lottery is Critical with probability 2/3, so Rush at t=100 is exactly 1/6.
    master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![25];
    for kind in 0..5 {
        master.gekisou_luck_bonus_lots.push(crate::master::LuckBonusLotRow {
            id: 100 + kind,
            chance_lot_type: kind,
            lot_result: 3,
            weight: 2,
        });
    }
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let certified =
        luck_rush_dp_certified(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
    let nominal = luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
    let joint = certified.steps.iter().find(|step| step.0 == 100).unwrap().1;
    assert!(joint[0].interval().contains(5.0 / 6.0));
    assert_eq!(joint[1], ProbabilityMass::ZERO);
    assert!(joint[2].interval().contains(1.0 / 6.0));
    assert_eq!(joint[3], ProbabilityMass::ZERO);
    assert!(joint[2].interval().upper() - joint[2].interval().lower() < 1e-12);
    assert_certified_encloses_nominal(&certified, &nominal);
}

#[test]
fn certified_deterministic_curves_keep_exact_zero_and_one() {
    for result in [0, 3] {
        let (master, notes, params, setup, play, delta) = fixture(result, 60);
        let skills = luck_skills(&master).unwrap();
        let deck = [Performer {
            gekisou_skill: Some((2, 1)),
            gekisou_support_skills: vec![(31, 1), (96, 1)],
            ..Default::default()
        }];
        let certified =
            luck_rush_dp_certified(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
        let native =
            luck::luck_rush_samples(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None, &[0, 1, 7])
                .unwrap();
        for &(time, joint) in certified.steps.iter() {
            let weights = &native[native.partition_point(|step| step.0 <= time) - 1].1;
            let index = 2 * usize::from(weights[0] == 1.0) + usize::from(weights[1] == 1.0);
            for (bucket, mass) in joint.into_iter().enumerate() {
                assert_eq!(mass, if bucket == index { ProbabilityMass::ONE } else { ProbabilityMass::ZERO });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_native_life_curve(
    master: &Master,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta: &[f32],
    deck: &[Performer],
) -> LuckDpResult {
    let skills = luck_skills(master).unwrap();
    let nominal =
        luck_rush_dp_with_events(master, &skills, notes, events, params, setup, play, delta, deck, None).unwrap();
    let certified =
        luck_rush_dp_certified_with_events(master, &skills, notes, events, params, setup, play, delta, deck, None)
            .unwrap();
    assert_certified_encloses_nominal(&certified, &nominal);
    let expected =
        luck_score_summary_with_ranking(master, &skills, deck, notes, events, params, setup, play, delta, None)
            .unwrap();
    let mut cache = LuckDpCache::new(1 << 20);
    let mut session = LuckScoreSession::new(master, &skills, notes, events, params, setup, play, delta, None);
    for _ in 0..2 {
        let cached = session.summary(deck, Some(&mut cache), || false).unwrap().unwrap();
        assert_eq!(summary_words(&cached), summary_words(&expected));
    }
    let mut native = LiveModel::new_gekisou(master, deck, notes, events, params, setup).unwrap();
    native.run_timed(play, delta).unwrap();
    let spans = native.rush_command_spans();
    for note in notes {
        let index = nominal.steps.partition_point(|step| step.0 <= note.time_ms);
        let probability = if index == 0 { 0.0 } else { nominal.steps[index - 1].1[0] };
        let expected = spans.iter().any(|&(start, end)| start <= note.time_ms && note.time_ms < end);
        assert_eq!(probability, if expected { 1.0 } else { 0.0 }, "note {} at {}", note.note_id, note.time_ms);
    }
    nominal
}

#[test]
fn range_start_life_conditions_use_native_damage_and_exact_thresholds() {
    for kind in [2000, 2001, 2002, 2003] {
        for positive in [true, false] {
            let (mut master, notes, params, setup, play, delta) = fixture(3, 0);
            let condition = master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
            condition.condition_type = kind;
            condition.condition_values = vec![700];
            condition.is_positive = positive;
            master.judgement_parameters[0].damage = 300;
            let deck = [Performer { gekisou_skill: Some((2, 1)), ..Default::default() }];
            let curve = assert_native_life_curve(&master, &notes, &[], params, &setup, &play, &delta, &deck);
            let starts = matches!(kind, 2001 | 2003) == positive;
            assert_eq!(curve.steps.iter().any(|step| step.1[0] > 0.0), starts, "type {kind}, positive {positive}");
        }
    }
}

#[test]
fn range_start_life_reads_before_its_phase_and_after_previous_phase_healing() {
    for phase in [1, 2] {
        let (mut master, notes, params, setup, play, delta) = fixture(3, 0);
        let condition = master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
        condition.condition_type = 2001;
        condition.condition_values = vec![700];
        // Reuse this fixture's unused Miss condition group as a native member-skill event trigger.
        master.skill_conditions.iter_mut().find(|row| row.id == 7000).unwrap().condition_type = 4010;
        master.gekisou_support_skill_effects.retain(|row| row.id != 96);
        master.judgement_parameters[0].damage = 600;
        master
            .skill_effect_settings
            .push(serde_json::from_value(json!({"_id":1,"_skillEffectType":11003,"_phase":phase})).unwrap());
        master
            .skill_effect_settings
            .push(serde_json::from_value(json!({"_id":2,"_skillEffectType":3001,"_phase":1})).unwrap());
        master
            .support_skill_effects
            .push(serde_json::from_value(row(501, "_supportSkillID", 501, 3001, 600, 7000, 0, 0)).unwrap());
        let deck = [Performer { gekisou_skill: Some((2, 1)), support_skills: vec![(501, 1)], ..Default::default() }];
        let events = [(0, 100)];
        let mut recorder = life_recorder(&master, &deck, &notes, &events, params, &setup, None).unwrap();
        for (frame, &dt) in play.frames.iter().zip(&delta).take_while(|(frame, _)| frame.time_ms <= 100) {
            recorder.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        }
        assert_eq!(recorder.phase_life, Some([400, 1000]));
        let curve = assert_native_life_curve(&master, &notes, &events, params, &setup, &play, &delta, &deck);
        assert_eq!(curve.steps.iter().any(|step| step.1[0] > 0.0), phase == 2);
        let without_event = assert_native_life_curve(&master, &notes, &[], params, &setup, &play, &delta, &deck);
        assert!(!without_event.steps.iter().any(|step| step.1[0] > 0.0));
    }
}

#[test]
fn life_range_needs_harmless_judgements_and_no_skill_that_leaves_it() {
    let (mut master, _, _, _, play, _) = fixture(3, 0);
    let top = i64::from(i32::MAX);
    let effects =
        [(501, 3001, 600), (502, 3001, top - 2000), (503, 3001, top - 1999), (504, 3000, 100), (505, 3002, 100)];
    for (id, effect, value) in effects.into_iter().chain([(506, 3003, 0), (507, 3004, 5000)]) {
        let support = row(id, "_supportSkillID", id, effect, value, 7000, 0, 0);
        master.support_skill_effects.push(serde_json::from_value(support).unwrap());
    }
    let damage = row(508, "_gekisouSupportSkillID", 508, 3002, 100, 7000, 0, 0);
    master.gekisou_support_skill_effects.push(serde_json::from_value(damage).unwrap());
    let holding = |skill| Performer { support_skills: vec![(skill, 1)], ..Default::default() };
    let range = (1000, 2000);
    assert_eq!(play_life_range(&master, &play), Some(range));
    let leaves = |skill| leaves_life_range(&master, &holding(skill), range);
    assert_eq!([501, 502, 503, 504, 505, 506, 507].map(leaves), [false, false, true, true, true, false, false]);
    // A Gekisou support skill runs only beside a Gekisou skill.
    let snap = Performer { gekisou_support_skills: vec![(508, 1)], ..Default::default() };
    assert!(!leaves_life_range(&master, &snap, range));
    assert!(leaves_life_range(&master, &Performer { gekisou_skill: Some((2, 1)), ..snap }, range));
    assert_eq!(life_range(&master, &[holding(501), holding(506)], &play), Some(range));
    assert_eq!(life_range(&master, &[holding(501), holding(505)], &play), None);
    // A judgement with damage or without a damage entry gives no range; Wait and Pass file no damage.
    let judged = |judgement| {
        let mut play = play.clone();
        play.frames[1].judged[0].judgement = judgement;
        play
    };
    assert_eq!([0, 6, 7].map(|judgement| play_life_range(&master, &judged(judgement)).is_some()), [true, false, true]);
    master.judgement_parameters[0].damage = 1;
    assert_eq!(play_life_range(&master, &play), None);
}

#[test]
fn life_conditions_fold_over_the_life_range_as_the_native_life_answers_them() {
    // No note deals damage, and a recovery in the range-start frame takes the life from 1000 to 1600 between the two
    // phases. A recording reads the life only for a comparison that the life range 1000..=2000 does not answer.
    for phase in [1, 2] {
        for kind in [2000, 2001, 2002, 2003] {
            for value in [700, 1000, 1500, 1600, 2000, 2500] {
                for positive in [true, false] {
                    let (mut master, notes, params, setup, play, delta) = fixture(3, 0);
                    let condition = master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
                    (condition.condition_type, condition.is_positive) = (kind, positive);
                    condition.condition_values = vec![value];
                    master.skill_conditions.iter_mut().find(|row| row.id == 7000).unwrap().condition_type = 4010;
                    master.gekisou_support_skill_effects.retain(|row| row.id != 96);
                    for (id, effect, phase) in [(1, 11003, phase), (2, 3001, 1)] {
                        let setting = json!({"_id":id,"_skillEffectType":effect,"_phase":phase});
                        master.skill_effect_settings.push(serde_json::from_value(setting).unwrap());
                    }
                    let recovery = row(501, "_supportSkillID", 501, 3001, 600, 7000, 0, 0);
                    master.support_skill_effects.push(serde_json::from_value(recovery).unwrap());
                    let deck = [Performer {
                        gekisou_skill: Some((2, 1)),
                        support_skills: vec![(501, 1)],
                        ..Default::default()
                    }];
                    let events = [(0, 100)];
                    let mut recorder = life_recorder(&master, &deck, &notes, &events, params, &setup, None).unwrap();
                    for (frame, &dt) in play.frames.iter().zip(&delta).take_while(|(frame, _)| frame.time_ms <= 100) {
                        recorder.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
                    }
                    assert_eq!(recorder.phase_life, Some([1000, 1600]));
                    let answer = |life: i64| match kind {
                        2000 => life > value,
                        2001 => life >= value,
                        2002 => life < value,
                        _ => life <= value,
                    };
                    let name = format!("phase {phase}, type {kind}, value {value}, positive {positive}");
                    let skills = luck_skills(&master).unwrap();
                    let prepared = prepare_recording::<ProbabilityMass>(
                        &master, &skills, &notes, params, &play, &delta, &deck, None, false,
                    )
                    .unwrap();
                    assert_eq!(prepared.life_deck.is_none(), answer(1000) == answer(2000), "{name}");
                    let curve =
                        assert_native_life_curve(&master, &notes, &events, params, &setup, &play, &delta, &deck);
                    let starts = answer(if phase == 1 { 1000 } else { 1600 }) == positive;
                    assert_eq!(curve.steps.iter().any(|step| step.1[0] > 0.0), starts, "{name}");
                }
            }
        }
    }
}

#[test]
fn lottery_kinds_share_folded_life_conditions_only_inside_the_life_range() {
    // A range-start gauge for a life of 700 or more. Performers whose life stays in the range of the play share the
    // kind of the folded condition, with or without a recovery; one that a skill damages takes no kind, and its
    // recording reads its life.
    let (mut master, notes, params, setup, play, delta) = fixture(3, 0);
    let condition = master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
    (condition.condition_type, condition.condition_values) = (2001, vec![700]);
    master.skill_conditions.iter_mut().find(|row| row.id == 7000).unwrap().condition_type = 4010;
    master.gekisou_support_skill_effects.retain(|row| row.id != 96);
    for (id, effect, phase) in [(1, 11003, 2), (2, 3001, 1), (3, 3002, 1)] {
        let setting = json!({"_id":id,"_skillEffectType":effect,"_phase":phase});
        master.skill_effect_settings.push(serde_json::from_value(setting).unwrap());
    }
    for (id, effect) in [(501, 3001), (502, 3002)] {
        let support = row(id, "_supportSkillID", id, effect, 600, 7000, 0, 0);
        master.support_skill_effects.push(serde_json::from_value(support).unwrap());
    }
    let skills = luck_skills(&master).unwrap();
    let gauge = Performer { gekisou_skill: Some((2, 1)), ..Default::default() };
    let healed = Performer { support_skills: vec![(501, 1)], ..gauge.clone() };
    let damaged = Performer { support_skills: vec![(502, 1)], ..gauge.clone() };
    let events = [(0, 100)];
    let live = LotteryLive {
        master: &master,
        skills: &skills,
        notes: &notes,
        events: &events,
        params,
        setup: &setup,
        play: &play,
        delta_times: &delta,
        ranking: None,
    };
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    let mut curves = LuckDpCache::new(usize::MAX);
    let formation = |performer: &Performer| {
        let mut deck = vec![Performer::default(); 5];
        deck[0] = performer.clone();
        deck
    };
    let mut laws = |performer: &Performer, kinds: &mut LotteryKinds<'_>| {
        kinds.order_laws(&live, &formation(performer), &[[0, 1, 2, 3, 4]], &mut curves).unwrap()
    };
    let first = laws(&gauge, &mut kinds).unwrap();
    let shared = laws(&healed, &mut kinds).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first[0], &shared[0]));
    assert_eq!((kinds.stats().composed, kinds.stats().law_hits), (1, 1));
    assert!(laws(&damaged, &mut kinds).is_none());
    assert_eq!(kinds.role(&live, &damaged), LotteryRole::Breaks(LotteryBreak::LeavesLifeRange));
    let class = |performer: &Performer| {
        LuckDpCache::new(usize::MAX)
            .certified_order_class(
                &master,
                &skills,
                &notes,
                &events,
                params,
                &setup,
                &play,
                &delta,
                &formation(performer),
                None,
            )
            .unwrap()
    };
    assert!(class(&damaged).is_none());
    for (performer, starts) in [(&gauge, true), (&healed, true), (&damaged, false)] {
        let deck = formation(performer);
        let curve = assert_native_life_curve(&master, &notes, &events, params, &setup, &play, &delta, &deck);
        assert_eq!(curve.steps.iter().any(|step| step.1[0] > 0.0), starts);
        let certified = luck_rush_dp_certified_with_events(
            &master, &skills, &notes, &events, params, &setup, &play, &delta, &deck, None,
        )
        .unwrap();
        if starts {
            assert_eq!(format!("{:?}", first[0]), format!("{certified:?}"));
            assert_eq!(format!("{:?}", class(performer).unwrap().law), format!("{certified:?}"));
        }
    }
}

#[test]
fn native_consume_count_bound_proves_rush_saturation_cannot_wrap() {
    assert!(consume_bound(10, [20, 30].into_iter()).is_ok());
    assert!(consume_bound(i32::MAX as usize, [0].into_iter()).is_ok());
    assert!(matches!(consume_bound(i32::MAX as usize, [1].into_iter()), Err(Error::Capacity(_))));
    assert!(matches!(consume_bound(usize::MAX, [1].into_iter()), Err(Error::Capacity(_))));
}

#[test]
fn rush_saturation_preserves_table_and_maximum_changes() {
    let mut score = LuckScore::default();
    for _ in 0..8 {
        score.add_score(3).unwrap();
        let reduced = Chain::of(&score).score(&LuckScore::default());
        assert_eq!(score.current_lot_type(), reduced.current_lot_type());
        assert_eq!(score.gauge_max, reduced.gauge_max);
    }
}

#[test]
fn before_frame_probe_is_distinct_from_new_rush() {
    let (master, notes, params, setup, _, _) = fixture(3, 60);
    let model = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
    let machine = &model.gk.as_ref().unwrap().ctrl.machine;
    let mut dp = Dp::<f64>::new(vec![LuckScore::default(); 3], machine, false);
    dp.set_states(vec![(
        State { rush: true, query_rush: true, score: true, score_before: false, ..State::default() },
        1.0,
    )]);
    assert_eq!(dp.weights(&[true], Phase::BeforeFrame), [1.0, 0.0, 0.0]);
    assert_eq!(dp.weights(&[true], Phase::AtFrame), [1.0, 1.0, 1.0]);
    assert_eq!(dp.weights(&[true], Phase::BeforeSkills), [1.0, 0.0, 0.0]);
    dp.set_states(vec![(
        State { rush: true, query_rush: false, score: true, score_before: false, ..State::default() },
        1.0,
    )]);
    assert_eq!(dp.weights(&[true], Phase::BeforeFrame), [0.0, 0.0, 0.0]);
    assert_eq!(dp.weights(&[true], Phase::BeforeSkills), [1.0, 0.0, 0.0]);
}

#[test]
fn future_probe_shapes_and_non_theoretical_judgements_are_rejected() {
    let (mut master, notes, params, setup, mut play, delta) = fixture(3, 60);
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    master.gekisou_skill_effects.iter_mut().find(|row| row.id == 1).unwrap().skill_condition_group = 4011;
    let skills = luck_skills(&master).unwrap();
    assert!(matches!(
        luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None),
        Err(Error::Unsupported(_))
    ));
    master.gekisou_skill_effects.iter_mut().find(|row| row.id == 1).unwrap().skill_condition_group = 0;
    master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_type = 2001;
    master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![700];
    master.gekisou_support_skill_effects.iter_mut().find(|row| row.id == 31).unwrap().skill_condition_group = 4011;
    let skills = luck_skills(&master).unwrap();
    assert!(matches!(
        luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None),
        Err(Error::Unsupported(_))
    ));
    master.gekisou_support_skill_effects.iter_mut().find(|row| row.id == 31).unwrap().skill_condition_group = 0;
    let skills = luck_skills(&master).unwrap();
    play.frames.iter_mut().find(|frame| !frame.judged.is_empty()).unwrap().judged[0].judgement_time_ms += 1;
    assert!(matches!(
        luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn overlapping_other_missions_do_not_silently_release_luck_mechanisms() {
    let (master, notes, params, mut setup, play, delta) = fixture(3, 60);
    setup.fevers.push((200, 600));
    setup.missions = vec![2, 1, 2];
    let skills = luck_skills(&master).unwrap();
    assert!(matches!(
        luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &[], None),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn finish_and_new_start_in_one_frame_need_historical_span_state() {
    let (master, mut notes, params, mut setup, mut play, delta) = fixture(3, 60);
    setup.fevers.push((1150, 1550));
    notes.push(LiveNote { note_id: 10, note_operate_type: 1, judgement_type: 1, time_ms: 1150 });
    play.frames.iter_mut().find(|frame| frame.time_ms == 1200).unwrap().judged.push(JudgedNote {
        note_id: 10,
        judgement: 5,
        judgement_time_ms: 1150,
    });
    let skills = luck_skills(&master).unwrap();
    assert!(matches!(
        luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &[], None),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn quiet_frames_preserve_pending_lots_and_next_frame_miss_effects() {
    let (mut master, notes, params, setup, _, _) = fixture(3, 60);
    // The deterministic lottery alternates Critical -> Miss -> Critical. Large additions create
    // queued lots consumed in otherwise empty frames; a Miss then adds more gauge on the NEXT frame.
    master.gekisou_luck_bonus_lots.iter_mut().find(|row| row.chance_lot_type == 4).unwrap().lot_result = 0;
    master.gekisou_skill_effects.iter_mut().find(|row| row.id == 2).unwrap().effect_value = 40_000;
    master.gekisou_support_skill_effects.iter_mut().find(|row| row.id == 96).unwrap().effect_value = 30_000;
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer {
        gekisou_skill: Some((2, 1)),
        gekisou_support_skills: vec![(31, 1), (96, 1)],
        ..Default::default()
    }];
    let mut frames: Vec<_> = (0..=200).map(|i| PlayFrame { time_ms: i * 10, judged: Vec::new() }).collect();
    for note in &notes {
        frames.iter_mut().find(|frame| frame.time_ms == note.time_ms).unwrap().judged.push(JudgedNote {
            note_id: note.note_id,
            judgement: 5,
            judgement_time_ms: note.time_ms,
        });
    }
    let play = LivePlay { frames, base_seed: 0 };
    let delta = vec![0.01; play.frames.len()];
    let dp = luck_rush_dp(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None).unwrap();
    let samples =
        luck::luck_rush_samples(&master, &skills, &notes, params, &setup, &play, &delta, &deck, None, &[0, 1, 7])
            .unwrap();
    assert_eq!(dp.steps, samples);
}

fn curve_words(curve: &LuckDpCertifiedResult) -> Vec<u64> {
    let mut out = vec![curve.probe_transitions.len() as u64];
    out.extend(curve.probe_transitions.iter().map(|&mask| u64::from(mask)));
    for (time, joint) in curve.steps.iter() {
        out.push(u64::from(*time as u32));
        joint.iter().for_each(|mass| out.extend(mass.bits()));
    }
    out.extend(curve.probes.iter().map(|&probe| u64::from(probe)));
    out.extend([curve.peak_states as u64, curve.transitions]);
    out
}

/// A chance-gated start gauge and a 2:1 Critical table: the curve carries many inexact masses.
fn random_fixture() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let (mut master, notes, params, setup, play, delta) = fixture(0, 60);
    master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![25];
    for kind in 0..5 {
        master.gekisou_luck_bonus_lots.push(crate::master::LuckBonusLotRow {
            id: 100 + kind,
            chance_lot_type: kind,
            lot_result: 3,
            weight: 2,
        });
    }
    (master, notes, params, setup, play, delta)
}

#[test]
fn factor_skeletons_record_the_transcripts_of_the_complete_reduced_model() {
    // The reduced model with every lottery condition skill and its skeleton with only the lot and gauge factor
    // skills file the same transcript, with a range-start gauge that the reduced model applies natively or only
    // by chance.
    for unconditional in [false, true] {
        let (mut master, notes, params, setup, play, delta) = random_fixture();
        if unconditional {
            master.gekisou_skill_effects.iter_mut().find(|row| row.id == 2).unwrap().skill_condition_group = 0;
            master.reindex().unwrap();
        }
        let skills = luck_skills(&master).unwrap();
        let performer = |skill, supports: &[i64]| Performer {
            gekisou_skill: Some((skill, 1)),
            gekisou_support_skills: supports.iter().map(|&support| (support, 1)).collect(),
            ..Default::default()
        };
        let (speed, gauge, sustained) = (performer(1, &[66]), performer(2, &[31, 96]), performer(3, &[66, 96]));
        let decks = [
            vec![gauge.clone(), Performer::default()],
            vec![speed.clone(), gauge.clone()],
            vec![gauge.clone(), sustained.clone(), speed],
            vec![sustained, gauge],
        ];
        for deck in &decks {
            for collect_moments in [false, true] {
                let prepared = prepare_recording::<ProbabilityMass>(
                    &master,
                    &skills,
                    &notes,
                    params,
                    &play,
                    &delta,
                    deck,
                    None,
                    collect_moments,
                )
                .unwrap();
                let model = LiveModel::build(
                    &master,
                    &prepared.reduced,
                    &notes,
                    &[],
                    params,
                    Some(&setup),
                    false,
                    None,
                    Some(&skills),
                )
                .unwrap();
                let complete = record_native(model, None, &notes, &play, &delta, &mut || false).unwrap().unwrap();
                let skeleton = record_skeleton(
                    &master,
                    &skills,
                    &notes,
                    &[],
                    params,
                    &setup,
                    &play,
                    &delta,
                    &prepared.reduced,
                    None,
                    None,
                    &mut || false,
                )
                .unwrap()
                .unwrap();
                assert!(skeleton.failure.is_none() && complete.failure.is_none());
                let words = |skeleton: &Skeleton| file(skeleton, &prepared.plan, collect_moments).key().unwrap();
                assert_eq!(words(&skeleton), words(&complete), "unconditional={unconditional} deck={deck:?}");
            }
        }
    }
}

fn summary_words(summary: &LuckScoreSummary) -> Vec<u64> {
    vec![
        summary.final_mean.lower.to_bits(),
        summary.final_mean.upper.to_bits(),
        summary.final_support.lower as u64,
        summary.final_support.upper as u64,
        summary.exact_constant_score.map_or(u64::MAX, |value| value as u64),
        summary.exact_final_life.map_or(u64::MAX, |value| value as u64),
        summary.probability_peak_states as u64,
        summary.probability_transitions,
    ]
}

#[test]
fn score_sessions_reuse_compiled_recordings_and_preserve_every_bound() {
    let (mut master, notes, params, setup, play, delta) = random_fixture();
    master.gekisou_skills.push(serde_json::from_value(json!({"_id":9,"_gekisouMissionType":2})).unwrap());
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let idle = Performer { gekisou_skill: Some((9, 1)), ..Default::default() };
    let holder =
        Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1), (66, 1)], ..Default::default() };
    let speed = Performer { gekisou_skill: Some((1, 1)), ..Default::default() };
    let decks = [
        vec![Performer::default(), idle.clone()],
        vec![idle.clone(), Performer::default()],
        vec![holder.clone(), idle.clone()],
        vec![idle.clone(), holder.clone()],
        vec![speed.clone(), holder.clone()],
        vec![holder, speed],
    ];
    for capacity in [0, 1 << 20] {
        let mut cache = LuckDpCache::new(capacity);
        let mut session = LuckScoreSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
        for deck in decks.iter().chain(&decks) {
            let expected = luck_score_summary_with_ranking(
                &master,
                &skills,
                deck,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                None,
            )
            .unwrap();
            let actual = session.summary(deck, Some(&mut cache), || false).unwrap().unwrap();
            assert_eq!(summary_words(&actual), summary_words(&expected));
        }
        if capacity == 0 {
            assert_eq!(cache.stats().recording_lookups, 0);
            assert!(cache.is_empty());
        } else {
            assert!(cache.stats().recording_hits >= 7);
        }
    }
}

#[test]
fn score_session_scopes_preserve_context_and_lottery_input_changes() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let deck = [Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let mut cache = LuckDpCache::new(1 << 20);
    for change in 0..8 {
        let (mut master, mut notes, mut params, mut setup, mut play, mut delta) =
            (master.clone(), notes.clone(), params, setup.clone(), play.clone(), delta.clone());
        let mut ranking = None;
        let mut events = Vec::new();
        match change {
            1 => master.gekisou_luck_bonus_lots.last_mut().unwrap().weight += 1,
            2 => master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![75],
            3 => {
                params.total_power = 2000;
                params.assist_factor = 0.75;
            }
            4 => setup.fevers[0].0 = 0,
            5 => {
                notes[0].time_ms += 1;
                let mut first = play.frames[1].judged.remove(0);
                first.judgement_time_ms += 1;
                play.frames[2].judged.insert(0, first);
            }
            6 => {
                delta[0] = f32::from_bits(delta[0].to_bits() + 1);
                events.push((0, 300));
            }
            7 => {
                setup.fevers[0].0 = 0;
                ranking = Some(vec![crate::replay::RankConfirmation { frame: 0, range: 0, rank: 1, percent: 50 }]);
            }
            _ => {}
        }
        master.reindex().unwrap();
        let skills = luck_skills(&master).unwrap();
        let ranking = ranking.as_deref();
        let expected = luck_score_summary_with_ranking(
            &master, &skills, &deck, &notes, &events, params, &setup, &play, &delta, ranking,
        )
        .unwrap();
        let mut session =
            LuckScoreSession::new(&master, &skills, &notes, &events, params, &setup, &play, &delta, ranking);
        for _ in 0..2 {
            let actual = session.summary(&deck, Some(&mut cache), || false).unwrap().unwrap();
            assert_eq!(summary_words(&actual), summary_words(&expected), "context {change}");
        }
    }
}

#[test]
fn score_sessions_keep_resolved_member_targets_in_the_recording_identity() {
    let (mut master, notes, params, setup, play, delta) = random_fixture();
    master.skill_targets.push(serde_json::from_value(json!({"_id":90,"_skillTargetType":1,"_bandID":1})).unwrap());
    let condition = master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
    condition.condition_type = 5000;
    condition.condition_target_ids = vec![90];
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let mut cache = LuckDpCache::new(1 << 20);
    let mut session = LuckScoreSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    let mut values = Vec::new();
    for band_id in [1, 2, 1, 2] {
        let deck = [Performer { band_id, gekisou_skill: Some((2, 1)), ..Default::default() }];
        let expected =
            luck_score_summary_with_ranking(&master, &skills, &deck, &notes, &[], params, &setup, &play, &delta, None)
                .unwrap();
        let actual = session.summary(&deck, Some(&mut cache), || false).unwrap().unwrap();
        assert_eq!(summary_words(&actual), summary_words(&expected));
        values.push(summary_words(&actual));
    }
    assert_ne!(values[0], values[1]);
    assert_eq!(cache.stats().recording_hits, 2);
}

#[test]
fn score_session_cancellation_keeps_completed_laws_reusable() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((2, 1)), ..Default::default() }];
    let expected =
        luck_score_summary_with_ranking(&master, &skills, &deck, &notes, &[], params, &setup, &play, &delta, None)
            .unwrap();
    let mut checks = 0;
    let mut session = LuckScoreSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    session
        .summary(&deck, None, || {
            checks += 1;
            false
        })
        .unwrap()
        .unwrap();
    for stop_at in [0, 2, checks / 2, checks - 1] {
        let mut cache = LuckDpCache::new(1 << 20);
        let mut session = LuckScoreSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
        let mut seen = 0;
        let partial = session
            .summary(&deck, Some(&mut cache), || {
                let stop = seen >= stop_at;
                seen += 1;
                stop
            })
            .unwrap();
        assert!(partial.is_none(), "cooperative check {stop_at}");
        let completed = session.summary(&deck, Some(&mut cache), || false).unwrap().unwrap();
        assert_eq!(summary_words(&completed), summary_words(&expected));
    }
}

#[test]
fn score_sessions_validate_retained_probability_and_reset_rows() {
    let (mut master, notes, params, setup, play, delta) = random_fixture();
    master.gekisou_skills.push(serde_json::from_value(json!({"_id":9,"_gekisouMissionType":2})).unwrap());
    let mut row = master.gekisou_skill_effects.iter().find(|row| row.skill_id == 2).unwrap().clone();
    row.id = 9;
    row.skill_id = 9;
    row.effect_execute_limit_count = 1;
    row.effect_execute_limit_reset_condition_group = 4011;
    master.gekisou_skill_effects.push(row);
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let mut cache = LuckDpCache::new(1 << 20);
    let mut session = LuckScoreSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    let valid = [Performer { gekisou_skill: Some((2, 1)), ..Default::default() }];
    let held = [Performer { gekisou_skill: Some((9, 1)), ..Default::default() }];
    let first = session.summary(&valid, Some(&mut cache), || false).unwrap().unwrap();
    assert!(matches!(session.summary(&held, Some(&mut cache), || false), Err(Error::Unsupported(_))));
    let again = session.summary(&valid, Some(&mut cache), || false).unwrap().unwrap();
    assert_eq!(summary_words(&first), summary_words(&again));
}

#[test]
fn recording_cache_keeps_requested_range_indicators_separate() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((2, 1)), ..Default::default() }];
    let mut curves = LuckDpCache::new(1 << 20);
    let mut recordings = RecordingCache::default();
    let mut get = |moments| {
        curves
            .certified_cancellable_mode(
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
                moments,
                Some(&mut recordings),
                &mut || false,
            )
            .unwrap()
            .unwrap()
    };
    let plain = get(false);
    let measured = get(true);
    let again = get(false);
    assert!(plain.range_moments.is_empty());
    assert_eq!(measured.range_moments.len(), setup.fevers.len());
    assert_eq!(curve_words(&plain), curve_words(&measured));
    assert!(std::sync::Arc::ptr_eq(&plain, &again));
    assert_eq!(curves.stats().recording_hits, 1);
}

#[test]
fn curve_cache_reuses_equal_recordings_and_returns_the_computed_curve() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let skills = luck_skills(&master).unwrap();
    let holder =
        Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1), (96, 1)], ..Default::default() };
    let speed = Performer { gekisou_skill: Some((1, 1)), ..Default::default() };
    let decks = [
        vec![holder.clone(), Performer::default()],
        // The same members in another order: the recording is the same.
        vec![Performer::default(), holder.clone()],
        // A timed gauge-speed holder changes the binary32 speed of the range's first notes.
        vec![holder.clone(), speed.clone()],
        vec![speed, holder],
    ];
    let mut cache = LuckDpCache::new(1 << 24);
    let mut direct = Vec::new();
    for (i, deck) in decks.iter().enumerate() {
        let expected =
            luck_rush_dp_certified(&master, &skills, &notes, params, &setup, &play, &delta, deck, None).unwrap();
        let cached =
            cache.certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, deck, None, None).unwrap();
        assert_eq!(curve_words(&cached), curve_words(&expected), "deck {i}");
        direct.push(curve_words(&expected));
    }
    assert_ne!(direct[0], direct[2]);
    let stats = cache.stats();
    assert_eq!((stats.lookups, stats.hits, cache.len()), (4, 2, 2));
    let again =
        cache.certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, &decks[2], None, None).unwrap();
    assert_eq!(curve_words(&again), direct[2]);
    assert_eq!(cache.stats().hits, 3);
}

#[test]
fn curve_cache_misses_on_any_recorded_input_change() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let deck = [Performer {
        gekisou_skill: Some((2, 1)),
        gekisou_support_skills: vec![(31, 1), (96, 1)],
        ..Default::default()
    }];
    let mut later = setup.clone();
    later.fevers[0] = (200, 500);
    let mut chance = master.clone();
    chance.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
    let mut weights = master.clone();
    weights.gekisou_luck_bonus_lots.iter_mut().filter(|row| row.id >= 100).for_each(|row| row.weight = 3);
    let cases: [(&Master, &GekisouSetup); 4] =
        [(&master, &setup), (&master, &later), (&chance, &setup), (&weights, &setup)];
    let mut cache = LuckDpCache::new(1 << 24);
    for (i, &(master, setup)) in cases.iter().enumerate() {
        let skills = luck_skills(master).unwrap();
        let expected =
            luck_rush_dp_certified(master, &skills, &notes, params, setup, &play, &delta, &deck, None).unwrap();
        let cached =
            cache.certified(master, &skills, &notes, &[], params, setup, &play, &delta, &deck, None, None).unwrap();
        assert_eq!(curve_words(&cached), curve_words(&expected), "case {i}");
        assert_eq!(cache.stats().hits, 0, "case {i}");
    }
    assert_eq!(cache.len(), cases.len());
}

#[test]
fn curve_cache_capacity_bounds_the_held_keys() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let skills = luck_skills(&master).unwrap();
    let decks: Vec<_> = [(2, vec![(31, 1), (96, 1)]), (2, vec![(31, 1)]), (3, vec![(31, 1)])]
        .into_iter()
        .map(|(skill, supports)| {
            [Performer { gekisou_skill: Some((skill, 1)), gekisou_support_skills: supports, ..Default::default() }]
        })
        .collect();
    let mut empty = LuckDpCache::new(0);
    for deck in &decks {
        let expected =
            luck_rush_dp_certified(&master, &skills, &notes, params, &setup, &play, &delta, deck, None).unwrap();
        let cached =
            empty.certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, deck, None, None).unwrap();
        assert_eq!(curve_words(&cached), curve_words(&expected));
    }
    assert!(empty.is_empty() && empty.stats().hits == 0);
    // Room for the first key only: a later key replaces it or, when larger, is not held.
    let mut one = LuckDpCache::new(usize::MAX);
    one.certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, &decks[0], None, None).unwrap();
    let capacity = one.stats().peak_key_bytes;
    assert!(capacity > 0);
    let mut small = LuckDpCache::new(capacity);
    for deck in &decks {
        small.certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, deck, None, None).unwrap();
        assert_eq!(small.len(), 1);
        assert!(small.stats().peak_key_bytes <= capacity);
    }
}

#[test]
fn frame_queries_read_the_probe_class_on_their_side_of_the_skill_boundary() {
    // Probability that the probe class is on (classes 1 and 3: probe bit 0, Rush bit 1).
    let on = |mass: &[ProbabilityMass; 4]| mass[1].interval().upper() + mass[3].interval().upper();
    let mut switched_on = 0;
    for result in [0, 3] {
        let (master, notes, params, setup, play, delta) = fixture(result, 60);
        let skills = luck_skills(&master).unwrap();
        let deck =
            [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
        let certified = luck_rush_dp_certified_with_ranking(
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
        for (time, masses) in certified.frame_queries.iter() {
            let index = play.frames.iter().position(|frame| frame.time_ms == *time).unwrap();
            // Bit 2 * before + after of each probe class transition at the frame's skill boundary.
            let mask = certified.probe_transitions[index];
            if mask & 0b1100 == 0 {
                assert_eq!(on(&masses[0]) + on(&masses[2]), 0.0, "result={result} frame={index}");
            }
            if mask & 0b1010 == 0 {
                assert_eq!(on(&masses[1]), 0.0, "result={result} frame={index}");
            }
            switched_on += usize::from(mask & 0b1100 == 0 && on(&masses[1]) > 0.0);
        }
    }
    assert!(switched_on > 0);
}

#[test]
fn probe_mask_tracks_native_skill_boundary_and_expands_quiet_frames() {
    for result in [0, 3] {
        let (master, notes, params, setup, play, delta) = fixture(result, 60);
        let skills = luck_skills(&master).unwrap();
        let deck =
            [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
        let certified = luck_rush_dp_certified_with_ranking(
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
        assert_eq!(certified.probe_transitions.len(), play.frames.len());
        assert!(certified.probe_transitions.iter().all(|&mask| (1..=15).contains(&mask)));
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        native.score.begin_bounds(Vec::new(), true);
        let mut applied = 0i32;
        let mut switched = 0;
        let mut starts = 0usize;
        for (index, (frame, &dt)) in play.frames.iter().zip(&delta).enumerate() {
            let old = applied != 0;
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            for event in native.score.bounds_trace.as_mut().unwrap().events.drain(..) {
                if let super::super::luck_score_bounds::BoundsEvent::Factor { command, .. } = event {
                    applied += command.note_mill;
                }
            }
            let new = applied != 0;
            if frame.time_ms < params.music_length_ms {
                assert_eq!(
                    certified.probe_transitions[index],
                    1 << (2 * usize::from(old) + usize::from(new)),
                    "result={result} frame={index}"
                );
            }
            switched += usize::from(old != new);
            starts += usize::from(!old && new);
        }
        assert_eq!(switched > 0, result == 3);
        // every lottery of the fixture has one result: the expected number of starts is the native count
        assert_eq!(starts > 0, result == 3);
        let possible = certified.probe_transitions.iter().filter(|&&mask| mask & 0b10 != 0).count();
        assert_eq!(possible, starts, "result={result}");
        assert!(certified.probe_starts.lower() <= starts as f64, "result={result}");
        assert!(certified.probe_starts.upper() >= starts as f64, "result={result}");
        assert!(certified.probe_starts.upper() - certified.probe_starts.lower() < 1e-9, "result={result}");
    }
}

#[test]
fn rush_transitions_match_the_native_rush_commands_of_each_frame() {
    for result in [0, 3] {
        let (master, notes, params, setup, play, delta) = fixture(result, 60);
        let skills = luck_skills(&master).unwrap();
        let deck =
            [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
        let certified = luck_rush_dp_certified_with_ranking(
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
        assert_eq!(certified.rush_transitions.len(), play.frames.len());
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        native.score.begin_bounds(Vec::new(), true);
        // Native files a Rush command exactly when the handle flips; a frame's start files before its first
        // score query, its lotteries after. The direct-probe class is the accumulated probe filing.
        let (mut rush, mut finishes, mut lotteries, mut applied) = (false, 0, 0, 0i32);
        let (mut diverged, mut marked) = (0, 0);
        for (index, (frame, &dt)) in play.frames.iter().zip(&delta).enumerate() {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            let (mut queried, mut at_start, mut in_lotteries) = (false, 0, 0);
            for event in native.score.bounds_trace.as_mut().unwrap().events.drain(..) {
                match event {
                    super::super::luck_score_bounds::BoundsEvent::Query { .. } => queried = true,
                    super::super::luck_score_bounds::BoundsEvent::Factor { command, .. } => {
                        applied += command.note_mill;
                        if command.luck != 0 {
                            if queried {
                                in_lotteries += 1;
                            } else {
                                at_start += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mask = certified.rush_transitions[index];
            if at_start > 0 {
                assert!(rush && at_start == 1 && mask & RUSH_FINISH != 0, "result={result} frame={index}");
                rush = false;
            }
            let start = rush;
            if frame.time_ms < params.music_length_ms {
                // In every marked frame the probe class follows the Rush; a range's COMPLETE turns the probe
                // off one frame before its FINISH disables the Rush.
                let follows = (applied != 0) == start;
                assert!(mask & RUSH_PROBE == 0 || follows, "result={result} frame={index}");
                diverged += usize::from(!follows);
                marked += usize::from(mask & RUSH_PROBE != 0);
            }
            rush ^= in_lotteries % 2 == 1;
            let bit = 4 * u16::from(start) + 2 * u16::from(rush) + u16::from(in_lotteries > 0);
            assert_eq!(mask & 0xff, 1 << bit, "result={result} frame={index}");
            finishes += at_start;
            lotteries += in_lotteries;
        }
        // A Critical starts the Rush and the range's FINISH disables it.
        assert_eq!((finishes, lotteries), if result == 3 { (1, 1) } else { (0, 0) });
        assert_eq!(diverged > 0, result == 3);
        assert!(marked > 0);
    }
}

#[test]
fn probe_mask_cached_and_uncached_certificates_keep_original_frame_ordinals() {
    let (master, notes, params, setup, mut play, mut delta) = fixture(3, 60);
    // Quiet frames have unequal times; compact propagation must still produce one mask per original frame.
    play.frames.insert(1, PlayFrame { time_ms: 31, judged: Vec::new() });
    delta[1] = 0.069;
    delta.insert(1, 0.031);
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let expected = luck_rush_dp_certified_with_ranking(
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
    let mut cache = LuckDpCache::new(1 << 20);
    for _ in 0..2 {
        let found = cache
            .certified_cancellable(
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
                None,
                &mut || false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(found.probe_transitions, expected.probe_transitions);
        assert_eq!(found.probe_transitions.len(), play.frames.len());
        assert_eq!(found.rush_transitions, expected.rush_transitions);
    }
    assert_eq!(expected.probe_transitions[0], 1);
    assert_eq!(expected.probe_transitions[1], 1);
}

#[test]
fn probe_mask_native_alternating_critical_miss_restarts_without_an_extra_cleanup_frame() {
    let (mut master, _, mut params, mut setup, mut play, delta) = fixture(3, 70);
    master.live_settings.iter_mut().find(|row| row.key == "gekisou_luck_gauge_max_rush").unwrap().value = "140".into();
    for kind in 0..5 {
        master.gekisou_luck_bonus_lots.push(crate::master::LuckBonusLotRow {
            id: 100 + kind,
            chance_lot_type: kind,
            lot_result: 0,
            weight: 1,
        });
    }
    let notes: Vec<_> = (1..=8)
        .map(|i| LiveNote { note_id: i - 1, note_operate_type: 1, judgement_type: 1, time_ms: i * 100 })
        .collect();
    params.converted_note_count = notes.len() as i32;
    setup.fevers = vec![(100, 900)];
    for frame in &mut play.frames {
        frame.judged.clear();
    }
    for note in &notes {
        play.frames.iter_mut().find(|frame| frame.time_ms == note.time_ms).unwrap().judged.push(JudgedNote {
            note_id: note.note_id,
            judgement: 5,
            judgement_time_ms: note.time_ms,
        });
    }
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((3, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let curve = luck_rush_dp_certified_with_ranking(
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
    let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    native.set_random(LiveRandom::with_nominal_prefix((0..64).map(|i| i % 2).collect()));
    native.score.begin_bounds(Vec::new(), true);
    let mut applied = 0i32;
    let mut observed = Vec::new();
    let mut lots = Vec::new();
    for (index, (frame, &dt)) in play.frames.iter().zip(&delta).enumerate() {
        let old = applied != 0;
        native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        lots.extend(native.gk.as_ref().unwrap().ctrl.lot_results.iter().copied());
        for event in native.score.bounds_trace.as_mut().unwrap().events.drain(..) {
            if let super::super::luck_score_bounds::BoundsEvent::Factor { command, .. } = event {
                applied += command.note_mill;
            }
        }
        let edge = 1 << (2 * usize::from(old) + usize::from(applied != 0));
        if frame.time_ms < params.music_length_ms {
            assert_ne!(curve.probe_transitions[index] & edge, 0, "frame={index} edge={edge}");
        }
        observed.push(edge);
    }
    assert!(lots.windows(3).any(|values| values == [3, 0, 3]), "lots={lots:?}");
    assert!(observed.windows(3).any(|edges| edges == [2, 4, 2]), "edges={observed:?}");
    assert!(native.random.nominal_covers_draws());
}
