//! Whole-live simulation on synthetic tables. Every number here is made up.

use ournotes_deck::Error;
use ournotes_deck::live::full::{JudgedNote, LiveModel, LiveNote, LiveParams, LivePlay, Performer, PlayFrame};
use ournotes_deck::live::score::{ComboTable, GREAT, LiveScoreCalculator, LiveScoreSettings, PERFECT, get_frame};
use ournotes_deck::live::skill::{NotePlay, live_skill_commands, score_with_factors};
use ournotes_deck::master::Master;
use serde_json::{Value, json};

const POWER: i32 = 200_000;
const LEVEL: i32 = 25;
const LENGTH: i32 = 12_000;

fn master_from(tables: &Value) -> Master {
    let texts: Vec<(String, String)> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({ "_allData": v }).to_string())).collect();
    Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, t)| t.as_str())).unwrap()
}

fn support_row(id: i64, skill: i64, effect_type: i64, value: i64, extra: Value) -> Value {
    let mut r = json!({"_id": id, "_supportSkillID": skill, "_level": 1, "_skillTriggerType": 1,
        "_skillTriggerConditionGroup": 53, "_skillConditionGroup": 0, "_skillReleaseConditionGroup": 0,
        "_skillTargetIDs": [], "_skillEffectType": effect_type, "_activationTimeSecond": 0.0, "_effectValue": value,
        "_maxEffectValue": 0, "_effectLimitCount": 0, "_skillCumulativeConditionID": 0,
        "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0});
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

fn tables() -> Value {
    json!({
        "MasterLiveNoteParameter": [{"_id": 1, "_noteOperateType": 1, "_scorePercent": 100}],
        "MasterLiveJudgementParameter": [
            {"_id": 1, "_noteSimulateJudgement": 6, "_scorePercent": 200, "_damage": 0},
            {"_id": 2, "_noteSimulateJudgement": 5, "_scorePercent": 100, "_damage": 0},
            {"_id": 3, "_noteSimulateJudgement": 4, "_scorePercent": 80, "_damage": 0},
            {"_id": 4, "_noteSimulateJudgement": 3, "_scorePercent": 50, "_damage": 0},
            {"_id": 5, "_noteSimulateJudgement": 2, "_scorePercent": 0, "_damage": 50},
            {"_id": 6, "_noteSimulateJudgement": 1, "_scorePercent": 0, "_damage": 100}],
        "MasterLiveSettings": [
            {"_id": 1, "_key": "note_score_adjustment_factor", "_value": "3"},
            {"_id": 2, "_key": "note_score_life_onus_factor", "_value": "0.5"},
            {"_id": 3, "_key": "life_base", "_value": "1000"},
            {"_id": 4, "_key": "life_denger", "_value": "300"}],
        "MasterLiveComboScoreBonus": [
            {"_id": 1, "_comboBonusType": 0, "_requiredComboCount": 10, "_bonusFactor": 0.01},
            {"_id": 2, "_comboBonusType": 0, "_requiredComboCount": 20, "_bonusFactor": 0.01}],
        "MasterSkillEffectSetting": [
            {"_id": 1, "_skillEffectType": 2000, "_phase": 2}, {"_id": 2, "_skillEffectType": 2004, "_phase": 2},
            {"_id": 3, "_skillEffectType": 3001, "_phase": 1}, {"_id": 4, "_skillEffectType": 3003, "_phase": 2},
            {"_id": 5, "_skillEffectType": 12006, "_phase": 2}, {"_id": 6, "_skillEffectType": 15000, "_phase": 2}],
        "MasterLiveJudgementTiming": [
            {"_id": 1, "_noteJudgementType": 1, "_noteSimulateJudgement": 6},
            {"_id": 2, "_noteJudgementType": 1, "_noteSimulateJudgement": 5},
            {"_id": 3, "_noteJudgementType": 1, "_noteSimulateJudgement": 4},
            {"_id": 4, "_noteJudgementType": 2, "_noteSimulateJudgement": 5}],
        "MasterSkillTarget": [
            {"_id": 3, "_skillTargetType": 3, "_bandID": 1},
            {"_id": 42, "_skillTargetType": 4, "_judgement": 4}],
        "MasterSkillCondition": [
            {"_id": 61, "_conditionType": 4010, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": []},
            {"_id": 70, "_conditionType": 5000, "_conditionValues": [], "_isPositive": true,
             "_conditionTargetIDs": [3]}],
        "MasterSkillConditionSet": [
            {"_id": 10, "_group": 53, "_conditionIds": [61]},
            {"_id": 11, "_group": 70, "_conditionIds": [70]}],
        "MasterLiveSkillEffect": [
            {"_id": 1, "_liveSkillID": 1, "_level": 1, "_skillConditionGroup": 0, "_skillReleaseConditionGroup": 0,
             "_skillTargetIDs": [], "_skillEffectType": 2000, "_activationTimeSecond": 5.0, "_effectValue": 1000,
             "_maxEffectValue": 0, "_effectLimitCount": 0, "_skillCumulativeConditionID": 0,
             "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0},
            {"_id": 2, "_liveSkillID": 2, "_level": 1, "_skillConditionGroup": 0, "_skillReleaseConditionGroup": 0,
             "_skillTargetIDs": [], "_skillEffectType": 2000, "_activationTimeSecond": 5.0, "_effectValue": 1000,
             "_maxEffectValue": 0, "_effectLimitCount": 2, "_skillCumulativeConditionID": 0,
             "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0}],
        "MasterSupportSkillEffect": [
            support_row(1, 1, 15000, 1000, json!({"_skillConditionGroup": 70})),
            support_row(2, 2, 3001, 300, json!({})),
            support_row(3, 3, 12006, 5, json!({"_activationTimeSecond": 5.0, "_skillTargetIDs": [42],
                                              "_effectLimitCount": 3})),
            support_row(4, 4, 11000, 100, json!({}))],
    })
}

fn master() -> Master {
    master_from(&tables())
}

fn chart() -> (Vec<LiveNote>, Vec<(i32, i32)>) {
    let notes = (0..60)
        .map(|i| LiveNote { note_id: i + 1, time_ms: 1000 + 100 * i, note_operate_type: 1, judgement_type: 1 })
        .collect();
    (notes, vec![(0, 1500), (1, 2600), (2, 3700), (3, 4800), (4, 5900)])
}

/// 16 ms frames; every note judged in the first frame reaching its chart time (plus `delay` frames).
fn play(notes: &[LiveNote], judge: impl Fn(&LiveNote) -> i32, delay: impl Fn(&LiveNote) -> usize) -> LivePlay {
    let mut frames: Vec<PlayFrame> =
        (0..LENGTH / 16).map(|k| PlayFrame { time_ms: 16 * k, judged: Vec::new() }).collect();
    for n in notes {
        let fi = frames.iter().position(|f| f.time_ms >= n.time_ms).unwrap() + delay(n);
        frames[fi].judged.push(JudgedNote { note_id: n.note_id, judgement: judge(n), judgement_time_ms: n.time_ms });
    }
    LivePlay { frames, base_seed: 7 }
}

fn params(notes: &[LiveNote]) -> LiveParams {
    LiveParams {
        total_power: POWER,
        music_level: LEVEL,
        converted_note_count: notes.len() as i32,
        music_length_ms: LENGTH,
        score_music_length_ms: None,
        assist_factor: 1.0,
        skill_target_music_type: 0,
    }
}

fn calculator(m: &Master, notes: &[LiveNote]) -> LiveScoreCalculator {
    let settings = LiveScoreSettings::from_master(m).unwrap();
    LiveScoreCalculator::new(POWER, LEVEL, notes.len() as i32, &settings, 1.0, 1.0, ComboTable::from_master(m).ok())
}

fn run(m: &Master, deck: &[Performer], notes: &[LiveNote], events: &[(i32, i32)], p: &LivePlay) -> LiveModel {
    let mut lm = LiveModel::new(m, deck, notes, events, params(notes)).unwrap();
    lm.run(p).unwrap();
    lm
}

fn performer(live: Option<(i64, i64)>, support: &[(i64, i64)], band: i64) -> Performer {
    Performer { live_skill: live, support_skills: support.to_vec(), band_id: band, ..Default::default() }
}

#[test]
fn no_skills_is_the_plain_sum_with_combo() {
    let m = master();
    let (notes, events) = chart();
    let deck = vec![Performer::default(); 5];
    let lm = run(&m, &deck, &notes, &events, &play(&notes, |_| 5, |_| 0));
    let c = calculator(&m, &notes);
    let want: i32 =
        notes.iter().enumerate().map(|(i, n)| c.note_score(i as i32, 1000, n.time_ms, 1, PERFECT, None).unwrap()).sum();
    assert_eq!(lm.score(), want);
    assert_eq!(lm.trace().len(), (LENGTH / 16) as usize);
    assert_eq!(lm.trace().last().unwrap().1, want);
}

#[test]
fn late_judgements_are_rewound() {
    let m = master();
    let (notes, events) = chart();
    let deck = vec![Performer::default(); 5];
    let on_time = run(&m, &deck, &notes, &events, &play(&notes, |_| 5, |_| 0)).score();
    let late = run(&m, &deck, &notes, &events, &play(&notes, |_| 5, |n| if n.note_id % 7 == 0 { 9 } else { 0 }));
    assert_eq!(late.score(), on_time);
}

#[test]
fn live_skills_without_rewinds_match_the_batch_score() {
    let m = master();
    let (notes, events) = chart();
    let deck = vec![performer(Some((1, 1)), &[], 0); 5];
    let got = run(&m, &deck, &notes, &events, &play(&notes, |_| 5, |_| 0)).score();
    let cmds = live_skill_commands(&m, &events, &[(1, 1); 5], &[1000; 5], LENGTH, None).unwrap();
    let plays: Vec<NotePlay> = notes
        .iter()
        .enumerate()
        .map(|(i, n)| NotePlay {
            note_id: n.note_id,
            time_ms: n.time_ms,
            note_type: 1,
            score_type: PERFECT,
            life: 1000,
            combo: i as i32,
        })
        .collect();
    let mut c = calculator(&m, &notes);
    assert_eq!(got, score_with_factors(&mut c, &plays, &cmds, get_frame(LENGTH) + 50).unwrap());
}

#[test]
fn duration_extension_needs_the_member_target() {
    let m = master();
    let (notes, events) = chart();
    let p = play(&notes, |_| 5, |_| 0);
    let in_band = run(&m, &vec![performer(Some((1, 1)), &[(1, 1)], 1); 5], &notes, &events, &p).score();
    let other = run(&m, &vec![performer(Some((1, 1)), &[(1, 1)], 2); 5], &notes, &events, &p).score();
    let plain = run(&m, &vec![performer(Some((1, 1)), &[], 0); 5], &notes, &events, &p).score();
    assert_eq!(other, plain);
    assert!(in_band > plain);
}

#[test]
fn recovery_over_heals_and_life_zero_is_final() {
    let m = master();
    let (notes, events) = chart();
    let deck = vec![performer(Some((1, 1)), &[(2, 1)], 0); 5];
    let misses = |n: &LiveNote| if n.note_id <= 12 { 1 } else { 5 };
    let p = play(&notes, misses, |_| 0);
    // 1000 - 6 misses + 300 (event 0) - 6 misses + 4 * 300, over the base life
    assert_eq!(run(&m, &deck, &notes, &events, &p).current_life(), 1300);
    let late: Vec<(i32, i32)> = (0..5).map(|k| (k, 3000 + 1000 * k)).collect();
    assert_eq!(run(&m, &deck, &notes, &late, &p).current_life(), 0);
}

#[test]
fn conversion_limit_converts_exactly_the_next_three() {
    let m = master();
    let (notes, events) = chart();
    let mut deck = vec![Performer::default(); 5];
    deck[0] = performer(None, &[(3, 1)], 0);
    let lm = run(&m, &deck, &notes, &events, &play(&notes, |_| 4, |_| 0));
    assert_eq!(lm.converted_judgements(), 3);
    let c = calculator(&m, &notes);
    let want: i32 = notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let st = if (7..=9).contains(&n.note_id) { PERFECT } else { GREAT };
            c.note_score(i as i32, 1000, n.time_ms, 1, st, None).unwrap()
        })
        .sum();
    assert_eq!(lm.score(), want);
}

#[test]
fn unmodelled_parts_are_unsupported() {
    let m = master();
    let (notes, events) = chart();
    let p = play(&notes, |_| 5, |_| 0);
    let limited = vec![performer(Some((2, 1)), &[], 0)];
    assert!(LiveModel::new(&m, &limited, &notes, &events, params(&notes)).is_ok());
    let mut deck = vec![Performer::default(); 5];
    deck[0] = performer(None, &[(4, 1)], 0);
    // 11000 has no applier without Gekisou (AppendGekisouSkillApplier is not called): the snap does nothing.
    let mut lm = LiveModel::new(&m, &deck, &notes, &events, params(&notes)).unwrap();
    let mut plain = LiveModel::new(&m, &vec![Performer::default(); 5], &notes, &events, params(&notes)).unwrap();
    assert_eq!(lm.run(&p).unwrap(), plain.run(&p).unwrap());
    // judgement window effects stay with the raw judgement bridge
    let mut t = tables();
    t["MasterSupportSkillEffect"][3]["_skillEffectType"] = json!(4001);
    let m = master_from(&t);
    let mut lm = LiveModel::new(&m, &deck, &notes, &events, params(&notes)).unwrap();
    assert!(matches!(lm.run(&p), Err(Error::Unsupported(_))));
}

#[test]
fn unknown_notes_in_the_stream_are_rejected() {
    let m = master();
    let (notes, events) = chart();
    let mut p = play(&notes, |_| 5, |_| 0);
    p.frames[10].judged.push(JudgedNote { note_id: 999, judgement: 5, judgement_time_ms: 160 });
    let mut lm = LiveModel::new(&m, &[], &notes, &events, params(&notes)).unwrap();
    assert!(matches!(lm.run(&p), Err(Error::Input(_))));
}

fn effect_model(effects: &[(i64, i64)], snap: bool, note_time_ms: i32) -> LiveModel {
    let mut t = tables();
    let mut rows = Vec::new();
    for (i, &(kind, value)) in effects.iter().enumerate() {
        let mut row = if snap {
            support_row(i as i64 + 100, 9, kind, value, json!({"_activationTimeSecond": 1.0}))
        } else {
            let mut row = t["MasterLiveSkillEffect"][0].clone();
            row["_id"] = json!(i + 100);
            row["_liveSkillID"] = json!(9);
            row["_skillEffectType"] = json!(kind);
            row["_effectValue"] = json!(value);
            row["_activationTimeSecond"] = json!(1.0);
            row
        };
        row["_skillConditionGroup"] = json!(0);
        rows.push(row);
    }
    t["MasterLiveSkillEffect"] = if snap { json!([]) } else { json!(rows) };
    t["MasterSupportSkillEffect"] = if snap { json!(rows) } else { json!([]) };
    let m = master_from(&t);
    let p = if snap { performer(None, &[(9, 1)], 1) } else { performer(Some((9, 1)), &[], 1) };
    let notes = [LiveNote { note_id: 1, time_ms: note_time_ms, note_operate_type: 1, judgement_type: 1 }];
    LiveModel::new(&m, &[p], &notes, &[(0, 0)], params(&notes)).unwrap()
}

#[test]
fn skill_damage_uses_the_safe_life_floor_for_members_and_snaps() {
    for snap in [false, true] {
        let mut lm = effect_model(&[(3002, 5000)], snap, 1100);
        lm.frame(0, &[]).unwrap();
        assert_eq!(lm.current_life(), 1);
        lm.frame(16, &[]).unwrap();
        assert_eq!(lm.current_life(), 1);
    }
}

#[test]
fn skill_reduction_ends_before_later_note_damage() {
    for snap in [false, true] {
        let mut lm = effect_model(&[(3004, 5000), (3002, 300)], snap, 1100);
        lm.frame(0, &[]).unwrap();
        assert_eq!(lm.current_life(), 850);
        lm.frame(1008, &[]).unwrap();
        lm.frame(1100, &[JudgedNote { note_id: 1, judgement: 1, judgement_time_ms: 1100 }]).unwrap();
        assert_eq!(lm.current_life(), 750);
    }
}

#[test]
fn skill_life_limit_changes_the_overheal_cap() {
    for snap in [false, true] {
        let mut lm = effect_model(&[(3000, 200), (3001, 5000)], snap, 1100);
        lm.frame(0, &[]).unwrap();
        assert_eq!(lm.current_life(), 2400);
        lm.frame(1008, &[]).unwrap();
        assert_eq!(lm.current_life(), 2400);
    }
}

#[test]
fn combo_score_up_and_note_score_down_apply_to_their_own_factors() {
    let m = master();
    for snap in [false, true] {
        for (kind, value) in [(2002, 5000), (2005, 2500)] {
            for note_time in [200, 1100] {
                let mut lm = effect_model(&[(kind, value)], snap, note_time);
                lm.frame(0, &[]).unwrap();
                if note_time > 1000 {
                    lm.frame(1008, &[]).unwrap();
                }
                lm.frame(note_time, &[JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: note_time }]).unwrap();
                let notes = [LiveNote { note_id: 1, time_ms: note_time, note_operate_type: 1, judgement_type: 1 }];
                let mut c = calculator(&m, &notes);
                if note_time < 1000 {
                    if kind == 2002 {
                        c.state.combo_score_up = 0.5;
                    } else {
                        c.state.note_score_up = 0.75;
                    }
                }
                let expected = c.note_score(0, 1000, note_time, 1, PERFECT, None).unwrap();
                assert_eq!(lm.score(), expected, "kind {kind}, snap {snap}, time {note_time}");
            }
        }
    }
}

#[test]
fn snap_cumulative_score_factors_update_cap_and_release() {
    // Uses the already-established judgement-equals cumulative checker (1000), not new WIP kinds.
    for kind in [2001, 2003] {
        for (value, cap, factors) in [(5000, 7500, [0.5f32, 0.75]), (-5000, 0, [-0.5f32, -1.0])] {
            let mut t = tables();
            t["MasterSkillTarget"]
                .as_array_mut()
                .unwrap()
                .push(json!({"_id": 99, "_skillTargetType": 4, "_judgement": 5}));
            t["MasterSkillCumulativeCondition"] = json!([{
                "_id": 90, "_skillCumulativeConditionType": 1000, "_conditionValues": [1],
                "_conditionTargetIDs": [99], "_maxCumulativeCount": 0
            }]);
            t["MasterSkillEffectSetting"]
                .as_array_mut()
                .unwrap()
                .push(json!({"_id": 99, "_skillEffectType": kind, "_phase": 2}));
            t["MasterLiveSkillEffect"] = json!([]);
            t["MasterSupportSkillEffect"] = json!([support_row(
                100,
                9,
                kind,
                value,
                json!({
                    "_activationTimeSecond": 0.5, "_skillCumulativeConditionID": 90,
                    "_maxEffectValue": cap
                })
            )]);
            let m = master_from(&t);
            let notes = [0, 100].map(|time_ms| LiveNote {
                note_id: time_ms / 100 + 1,
                time_ms,
                note_operate_type: 1,
                judgement_type: 1,
            });
            let p = performer(None, &[(9, 1)], 1);
            let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0)], params(&notes)).unwrap();
            for (i, time_ms) in [0, 100].into_iter().enumerate() {
                lm.frame(time_ms, &[JudgedNote { note_id: i as i32 + 1, judgement: 5, judgement_time_ms: time_ms }])
                    .unwrap();
                let f = lm.factor_state();
                assert_eq!(f.note_score_up, if kind == 2001 { 1.0 + factors[i] } else { 1.0 });
                assert_eq!(f.combo_score_up, if kind == 2003 { factors[i] } else { 0.0 });
            }
            lm.frame(200, &[]).unwrap();
            let active = *lm.factor_state();
            lm.frame(500, &[]).unwrap(); // Executing ends only after, not at, the duration boundary.
            assert_eq!(lm.factor_state().note_score_up, active.note_score_up);
            assert_eq!(lm.factor_state().combo_score_up, active.combo_score_up);
            lm.frame(501, &[]).unwrap();
            assert_eq!(lm.factor_state().note_score_up, 1.0);
            assert_eq!(lm.factor_state().combo_score_up, 0.0);
        }
    }
}

#[test]
fn live_cumulative_unknown_rows_are_rejected() {
    let mut t = tables();
    t["MasterLiveSkillEffect"][0]["_skillEffectType"] = json!(2001);
    t["MasterLiveSkillEffect"][0]["_skillCumulativeConditionID"] = json!(90);
    let m = master_from(&t);
    let (notes, events) = chart();
    let p = performer(Some((1, 1)), &[], 1);
    assert!(matches!(LiveModel::new(&m, &[p], &notes, &events, params(&notes)), Err(Error::Master(_))));
}

#[test]
fn overlapping_live_executions_keep_separate_expirations() {
    let mut t = tables();
    t["MasterLiveSkillEffect"][0]["_activationTimeSecond"] = json!(0.5);
    let m = master_from(&t);
    let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
    let p = performer(Some((1, 1)), &[], 1);
    let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0), (0, 100)], params(&notes)).unwrap();
    for (time, want) in [(0, 1.1f32), (100, 1.2), (500, 1.2), (501, 1.1), (601, 1.0)] {
        lm.frame(time, &[]).unwrap();
        assert!((lm.factor_state().note_score_up - want).abs() < 1e-6, "at {time}");
    }
}

#[test]
fn sixth_concurrent_live_execution_reports_pool_exhaustion() {
    let m = master();
    let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
    let p = performer(Some((1, 1)), &[], 1);
    let events: Vec<_> = (0..6).map(|i| (0, i * 100)).collect();
    let mut lm = LiveModel::new(&m, &[p], &notes, &events, params(&notes)).unwrap();
    for i in 0..5 {
        lm.frame(i * 100, &[]).unwrap();
    }
    assert!(matches!(lm.frame(500, &[]), Err(Error::Game(_))));
}

#[test]
fn live_release_is_first_checked_in_executing_not_execute_frame() {
    let mut t = tables();
    t["MasterLiveSkillEffect"][0]["_activationTimeSecond"] = json!(0.0);
    t["MasterLiveSkillEffect"][0]["_skillReleaseConditionGroup"] = json!(70);
    let m = master_from(&t);
    let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
    let p = performer(Some((1, 1)), &[], 1);
    let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0)], params(&notes)).unwrap();
    for (time, want) in [(0, 1.1f32), (1, 1.1), (2, 1.0)] {
        lm.frame(time, &[]).unwrap();
        assert!((lm.factor_state().note_score_up - want).abs() < 1e-6, "at {time}");
    }
}

#[test]
fn overlapping_live_cumulative_score_uses_independent_counters_and_handles() {
    let mut t = tables();
    t["MasterLiveSkillEffect"][0]["_skillEffectType"] = json!(2001);
    t["MasterLiveSkillEffect"][0]["_activationTimeSecond"] = json!(0.5);
    t["MasterLiveSkillEffect"][0]["_skillCumulativeConditionID"] = json!(90);
    t["MasterSkillCumulativeCondition"] = json!([
        {"_id":90,"_skillCumulativeConditionType":1000,"_conditionValues":[1],"_conditionTargetIDs":[42],"_maxCumulativeCount":100}
    ]);
    let m = master_from(&t);
    let notes = vec![
        LiveNote { note_id: 1, time_ms: 0, note_operate_type: 1, judgement_type: 1 },
        LiveNote { note_id: 2, time_ms: 100, note_operate_type: 1, judgement_type: 1 },
    ];
    let p = performer(Some((1, 1)), &[], 1);
    let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0), (0, 100)], params(&notes)).unwrap();
    lm.frame(0, &[JudgedNote { note_id: 1, judgement: 4, judgement_time_ms: 0 }]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.1).abs() < 1e-6);
    lm.frame(100, &[JudgedNote { note_id: 2, judgement: 4, judgement_time_ms: 100 }]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.3).abs() < 1e-6);
    lm.frame(501, &[]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.1).abs() < 1e-6);
    lm.frame(601, &[]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.0).abs() < 1e-6);
}

#[test]
fn parameter_effects_have_no_dynamic_power_applier() {
    for kind in [1000, 1001, 1002, 1003, 1500, 1501, 1502, 1503] {
        for snap in [false, true] {
            let mut lm = effect_model(&[(kind, 10000)], snap, 200);
            lm.frame(0, &[]).unwrap();
            lm.frame(200, &[JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 200 }]).unwrap();
            assert_eq!(lm.factor_state().band_total_power, POWER);
            assert_eq!(lm.factor_state().note_score_up, 1.0);
            assert_eq!(lm.factor_state().combo_score_up, 0.0);
            lm.frame(1001, &[]).unwrap();
            assert_eq!(lm.factor_state().band_total_power, POWER);
        }
    }
}

#[test]
fn live_trigger_uses_last_matching_event_in_list_order() {
    for events in [vec![(0, 0), (0, 10)], vec![(0, 10), (0, 0)]] {
        let mut t = tables();
        t["MasterLiveSkillEffect"][0]["_activationTimeSecond"] = json!(0.5);
        let m = master_from(&t);
        let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
        let p = performer(Some((1, 1)), &[], 1);
        let mut lm = LiveModel::new(&m, &[p], &notes, &events, params(&notes)).unwrap();
        lm.frame(10, &[]).unwrap();
        assert!((lm.factor_state().note_score_up - 1.1).abs() < 1e-6);
        lm.frame(505, &[]).unwrap();
        let want = if events[1].1 == 10 { 1.1f32 } else { 1.0 };
        assert!((lm.factor_state().note_score_up - want).abs() < 1e-6);
    }
}

#[test]
fn ordinary_live_conversion_consumes_its_own_effect_limit() {
    let mut t = tables();
    t["MasterLiveSkillEffect"][0]["_skillEffectType"] = json!(12006);
    t["MasterLiveSkillEffect"][0]["_effectValue"] = json!(5);
    t["MasterLiveSkillEffect"][0]["_skillTargetIDs"] = json!([42]);
    t["MasterLiveSkillEffect"][0]["_effectLimitCount"] = json!(2);
    let m = master_from(&t);
    let notes: Vec<_> =
        (1..=3).map(|i| LiveNote { note_id: i, time_ms: i * 100, note_operate_type: 1, judgement_type: 1 }).collect();
    let p = performer(Some((1, 1)), &[], 1);
    let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0)], params(&notes)).unwrap();
    lm.frame(0, &[]).unwrap();
    for i in 1..=3 {
        lm.frame(i * 100, &[JudgedNote { note_id: i, judgement: 4, judgement_time_ms: i * 100 }]).unwrap();
    }
    assert_eq!(lm.converted_judgements(), 2);
}

#[test]
fn score_effect_does_not_treat_effect_limit_as_an_execution_limit() {
    let mut t = tables();
    t["MasterLiveSkillEffect"][0]["_effectLimitCount"] = json!(1);
    let m = master_from(&t);
    let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
    let p = performer(Some((1, 1)), &[], 1);
    let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0), (0, 100)], params(&notes)).unwrap();
    for time in [0, 100, 200] {
        lm.frame(time, &[]).unwrap();
    }
    assert!((lm.factor_state().note_score_up - 1.2).abs() < 1e-6);
}

#[test]
fn timed_sustained_support_stacks_and_finished_gate_freezes_it() {
    let mut t = tables();
    t["MasterLiveSkillEffect"] = json!([]);
    t["MasterSupportSkillEffect"] = json!([support_row(
        80,
        80,
        2000,
        1000,
        json!({
            "_skillTriggerType": 2, "_skillConditionGroup": 70, "_activationTimeSecond": 0.5
        })
    )]);
    let m = master_from(&t);
    let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
    let p = performer(None, &[(80, 1)], 1);
    let mut lm = LiveModel::new(&m, &[p], &notes, &[(0, 0), (0, 100)], params(&notes)).unwrap();
    lm.frame(0, &[]).unwrap();
    lm.frame(100, &[]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.2).abs() < 1e-6);
    lm.set_live_finished(true);
    lm.frame(200, &[]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.2).abs() < 1e-6);
    lm.set_live_finished(false);
    lm.frame(300, &[]).unwrap();
    assert!((lm.factor_state().note_score_up - 1.0).abs() < 1e-6);
}

#[test]
fn reset_checker_is_only_constructed_for_a_positive_execute_limit() {
    let mut t = tables();
    t["MasterSkillCondition"].as_array_mut().unwrap().push(json!({
        "_id":90,"_conditionType":999999,"_conditionValues":[],"_isPositive":true,"_conditionTargetIDs":[]
    }));
    t["MasterSkillConditionSet"].as_array_mut().unwrap().push(json!({"_id":90,"_group":90,"_conditionIds":[90]}));
    t["MasterSupportSkillEffect"] = json!([support_row(
        80,
        80,
        2000,
        1000,
        json!({
            "_effectExecuteLimitResetConditionGroup":90
        })
    )]);
    let notes = vec![LiveNote { note_id: 1, time_ms: 1000, note_operate_type: 1, judgement_type: 1 }];
    let p = performer(None, &[(80, 1)], 1);
    let m = master_from(&t);
    let mut lm = LiveModel::new(&m, std::slice::from_ref(&p), &notes, &[(0, 0)], params(&notes)).unwrap();
    lm.frame(0, &[]).unwrap();
    t["MasterSupportSkillEffect"][0]["_effectExecuteLimitCount"] = json!(1);
    let m = master_from(&t);
    assert!(matches!(LiveModel::new(&m, &[p], &notes, &[(0, 0)], params(&notes)), Err(Error::Unsupported(_))));
}
