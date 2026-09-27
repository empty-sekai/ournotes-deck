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
    assert!(matches!(LiveModel::new(&m, &limited, &notes, &events, params(&notes)), Err(Error::Unsupported(_))));
    let mut deck = vec![Performer::default(); 5];
    deck[0] = performer(None, &[(4, 1)], 0);
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
