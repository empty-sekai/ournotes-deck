//! Whole-live simulation with Gekisou on synthetic tables. Every number here is made up.

use ournotes_deck::Error;
use ournotes_deck::live::full::{
    GekisouSetup, JudgedNote, LiveModel, LiveNote, LiveParams, LivePlay, Performer, PlayFrame,
};
use ournotes_deck::live::score::{
    ComboTable, GekisouComboInfo, JUST, LiveScoreCalculator, LiveScoreSettings, PERFECT, get_frame,
};
use ournotes_deck::master::Master;
use serde_json::{Value, json};

const POWER: i32 = 200_000;
const LEVEL: i32 = 25;
const LENGTH: i32 = 8000;
const START: i32 = 2000;
const END: i32 = 4000;
/// Rank bonus percent of every range in the synthetic table.
const RANK_PCT: i64 = 250;

fn master_from(tables: &Value) -> Master {
    let texts: Vec<(String, String)> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({ "_allData": v }).to_string())).collect();
    Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, t)| t.as_str())).unwrap()
}

fn effect(id: i64, key: &str, skill: i64, effect_type: i64, value: i64, extra: Value) -> Value {
    let mut r = json!({"_id": id, key: skill, "_level": 1, "_skillTriggerType": 1,
        "_skillTriggerConditionGroup": 253, "_skillConditionGroup": 0, "_skillReleaseConditionGroup": 0,
        "_skillTargetIDs": [], "_skillEffectType": effect_type, "_activationTimeSecond": 1.0, "_effectValue": value,
        "_maxEffectValue": 0, "_effectLimitCount": 0, "_skillCumulativeConditionID": 0,
        "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0});
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

fn tables() -> Value {
    let timing: Vec<Value> = [6, 5, 4, 3]
        .iter()
        .enumerate()
        .map(|(i, &j)| {
            json!({"_id": i + 1, "_noteJudgementType": 1, "_noteSimulateJudgement": j,
                   "_afterMs": if j == 6 { 251 } else { 100 }})
        })
        .collect();
    let mut combo = vec![
        json!({"_id": 1, "_comboBonusType": 0, "_requiredComboCount": 10, "_bonusFactor": 0.01}),
        json!({"_id": 2, "_comboBonusType": 0, "_requiredComboCount": 20, "_bonusFactor": 0.01}),
    ];
    for i in 0..8 {
        combo.push(json!({"_id": 10 + i, "_comboBonusType": 1, "_requiredComboCount": 5 * (i + 1),
                          "_bonusFactor": 0.02}));
    }
    let lots: Vec<Value> =
        (0..5).map(|k| json!({"_id": 1 + k, "_chanceLotType": k, "_lotResult": 3, "_weight": 1})).collect();
    let ranks: Vec<Value> = (1..=3)
        .map(|c| json!({"_id": c, "_missionPattern": 1, "_count": c, "_rank": 1, "_scoreBonusPercent": RANK_PCT}))
        .collect();
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
            {"_id": 4, "_key": "life_denger", "_value": "300"},
            {"_id": 5, "_key": "gekisou_luck_gauge_max", "_value": "140"},
            {"_id": 6, "_key": "gekisou_luck_gauge_max_rush", "_value": "70"},
            {"_id": 7, "_key": "gekisou_luck_rush_score_bonus_percent", "_value": "10"}],
        "MasterLiveComboScoreBonus": combo,
        "MasterSkillEffectSetting": [
            {"_id": 1, "_skillEffectType": 12000, "_phase": 2}, {"_id": 2, "_skillEffectType": 2001, "_phase": 2}],
        "MasterLiveJudgementTiming": timing,
        "MasterLiveGekisouRankingScoreBonus": ranks,
        "MasterLiveGekisouLuckBasePoint": [
            {"_id": 1, "_noteCategory": 0, "_noteSimulateJudgement": 5, "_weight": 1, "_basePoint": 10}],
        "MasterLiveGekisouLuckBonusLot": lots,
        "MasterSkillTarget": [
            {"_id": 55, "_skillTargetType": 5, "_gekisouMissionType": 1},
            {"_id": 57, "_skillTargetType": 5, "_gekisouMissionType": 3}],
        "MasterSkillCondition": [
            {"_id": 80, "_conditionType": 7010, "_conditionValues": [], "_isPositive": true,
             "_conditionTargetIDs": [55]},
            {"_id": 81, "_conditionType": 7010, "_conditionValues": [], "_isPositive": true,
             "_conditionTargetIDs": [57]},
            {"_id": 82, "_conditionType": 7013, "_conditionValues": [], "_isPositive": true,
             "_conditionTargetIDs": []}],
        "MasterSkillConditionSet": [
            {"_id": 20, "_group": 253, "_conditionIds": [80]}, {"_id": 21, "_group": 254, "_conditionIds": [81]},
            {"_id": 22, "_group": 258, "_conditionIds": [82]}],
        "MasterSkillCumulativeCondition": [
            {"_id": 21, "_skillCumulativeConditionType": 7001, "_conditionValues": [10], "_conditionTargetIDs": [],
             "_maxCumulativeCount": 999999}],
        "MasterGekisouSkill": [
            {"_id": 1, "_gekisouMissionType": 1}, {"_id": 2, "_gekisouMissionType": 3},
            {"_id": 3, "_gekisouMissionType": 3}],
        "MasterGekisouSkillEffect": [
            effect(1, "_gekisouSkillID", 1, 12000, 3, json!({})),
            effect(2, "_gekisouSkillID", 2, 12000, 3, json!({"_skillTriggerConditionGroup": 254})),
            effect(3, "_gekisouSkillID", 3, 13000, 1, json!({"_skillTriggerConditionGroup": 254}))],
        "MasterGekisouSupportSkill": [{"_id": 1, "_gekisouMissionType": 1}],
        "MasterGekisouSupportSkillEffect": [
            effect(1, "_gekisouSupportSkillID", 1, 2001, 100, json!({"_skillReleaseConditionGroup": 258,
                "_skillCumulativeConditionID": 21, "_activationTimeSecond": 0.0, "_maxEffectValue": 1000}))],
        "MasterSupportSkillEffect": [
            effect(1, "_supportSkillID", 1, 2000, 1000, json!({}))],
    })
}

fn master() -> Master {
    master_from(&tables())
}

fn chart() -> (Vec<LiveNote>, Vec<(i32, i32)>) {
    let notes = (0..60)
        .map(|i| LiveNote { note_id: i + 1, time_ms: 1000 + 100 * i, note_operate_type: 1, judgement_type: 1 })
        .collect();
    // no live skill events in the Gekisou window
    (notes, (0..5).map(|k| (k, 20_000 + k)).collect())
}

/// 16 ms frames; every note judged in the first frame reaching its chart time.
fn play(notes: &[LiveNote], judge: impl Fn(&LiveNote) -> i32) -> (LivePlay, Vec<f32>) {
    let mut frames: Vec<PlayFrame> =
        (0..LENGTH / 16).map(|k| PlayFrame { time_ms: 16 * k, judged: Vec::new() }).collect();
    for n in notes {
        let fi = frames.iter().position(|f| f.time_ms >= n.time_ms).unwrap();
        frames[fi].judged.push(JudgedNote { note_id: n.note_id, judgement: judge(n), judgement_time_ms: n.time_ms });
    }
    let dts = vec![0.016f32; frames.len()];
    (LivePlay { frames, base_seed: 7 }, dts)
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

fn setup(missions: [i64; 3]) -> GekisouSetup {
    GekisouSetup { fevers: vec![(START, END)], missions: missions.to_vec() }
}

fn run(m: &Master, deck: &[Performer], missions: [i64; 3], judge: impl Fn(&LiveNote) -> i32) -> LiveModel {
    let (notes, events) = chart();
    let (p, dts) = play(&notes, judge);
    let mut lm = LiveModel::new_gekisou(m, deck, &notes, &events, params(&notes), &setup(missions)).unwrap();
    lm.run_timed(&p, &dts).unwrap();
    lm
}

fn gekisou_performer(skill: i64, support: &[(i64, i64)]) -> Performer {
    Performer { gekisou_skill: Some((skill, 1)), gekisou_support_skills: support.to_vec(), ..Default::default() }
}

/// A Gekisou combo as a function of the note time.
struct Combo<F: Fn(i32) -> Option<i32>>(F);

impl<F: Fn(i32) -> Option<i32>> GekisouComboInfo for Combo<F> {
    fn gekisou_combo(&self, time_ms: i32) -> Option<i32> {
        (self.0)(time_ms)
    }
}

fn prev_end(t: i32) -> i32 {
    (t + 39) / 40 * 40 - 40
}

/// Independent sum: the score combo counts earlier notes, the Gekisou combo comes from `gk`, `luck(t)` is the luck
/// bonus at the note; plus the rank bonus of the range.
fn expected(m: &Master, gk: Option<&dyn GekisouComboInfo>, luck: impl Fn(i32) -> i32, st: i32) -> i32 {
    let (notes, _) = chart();
    let settings = LiveScoreSettings::from_master(m).unwrap();
    let mut c = LiveScoreCalculator::new(
        POWER,
        LEVEL,
        notes.len() as i32,
        &settings,
        1.0,
        1.0,
        ComboTable::from_master(m).ok(),
    );
    let mut per = Vec::new();
    for (i, n) in notes.iter().enumerate() {
        c.state.added_luck_bonus = luck(n.time_ms);
        per.push((n.time_ms, c.note_score(i as i32, 1000, n.time_ms, 1, st, gk).unwrap()));
    }
    let upto = |x: i32| -> i32 { per.iter().filter(|p| get_frame(p.0) <= get_frame(x)).map(|p| p.1).sum() };
    let range = upto(END) - upto(START);
    per.iter().map(|p| p.1).sum::<i32>() + (range as i64 * RANK_PCT / 100) as i32
}

fn range_notes() -> Vec<i32> {
    chart().0.iter().map(|n| n.time_ms).filter(|&t| (START..=END).contains(&t)).collect()
}

#[test]
fn combo_range_scales_the_combo_factor_and_adds_the_rank_bonus() {
    let m = master();
    let lm = run(&m, &vec![Performer::default(); 5], [1, 1, 1], |_| 5);
    let rng = range_notes();
    let gk =
        Combo(|t: i32| (START..=END).contains(&t).then(|| rng.iter().filter(|&&x| x <= prev_end(t)).count() as i32));
    assert_eq!(lm.score(), expected(&m, Some(&gk), |_| 0, PERFECT));
    let r = lm.gekisou_ranges();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].state, 8);
    assert_eq!(r[0].combo, 21);
    assert!(r[0].rank_bonus.unwrap() > 0);
}

#[test]
fn combo_bonus_skill_counts_more_while_it_runs() {
    let m = master();
    let deck = [gekisou_performer(1, &[]), Performer::default(), Performer::default()];
    let lm = run(&m, &deck, [1, 1, 1], |_| 5);
    let rng = range_notes();
    let gk = Combo(|t: i32| {
        (START..=END)
            .contains(&t)
            .then(|| rng.iter().filter(|&&x| x <= prev_end(t)).map(|&x| if x < START + 1000 { 4 } else { 1 }).sum())
    });
    assert_eq!(lm.score(), expected(&m, Some(&gk), |_| 0, PERFECT));
}

#[test]
fn a_skill_of_another_mission_never_passes_its_gate() {
    let m = master();
    let other = run(&m, &[gekisou_performer(2, &[])], [1, 1, 1], |_| 5).score();
    let none = run(&m, &[Performer::default()], [1, 1, 1], |_| 5).score();
    assert_eq!(other, none);
}

#[test]
fn just_bonus_skill_raises_the_just_count() {
    let m = master();
    let plain = run(&m, &[Performer::default()], [3, 3, 3], |_| 6);
    assert_eq!(plain.gekisou_ranges()[0].just_count, 21);
    let lm = run(&m, &[gekisou_performer(3, &[])], [3, 3, 3], |_| 6);
    // Just bonus 1 from the range start for one second: ten notes count twice
    assert_eq!(lm.gekisou_ranges()[0].just_count, 31);
    let gk = Combo(|_| None);
    assert_eq!(lm.score(), expected(&m, Some(&gk), |_| 0, JUST));
}

#[test]
fn luck_rush_bonus_lasts_until_the_range_finishes() {
    let m = master();
    let lm = run(&m, &vec![Performer::default(); 5], [2, 2, 2], |_| 5);
    // fourteen range notes of 10 points fill the 140 gauge at 3300 ms: a Critical starts the rush (+10 %); it lasts
    // to the range's finish: fever end 4000, +256 ms (>= 251), +512 ms (>= 500), finish one frame later
    let luck = |t: i32| if (3300..4784).contains(&t) { 10 } else { 0 };
    assert_eq!(lm.score(), expected(&m, None, luck, PERFECT));
    let r = lm.gekisou_ranges()[0];
    assert_eq!(r.lot_results, [0, 0, 0, 2]);
    assert_eq!(r.luck_points, 20);
    assert_eq!(run(&m, &vec![Performer::default(); 5], [2, 2, 2], |_| 5).score(), lm.score());
}

#[test]
fn support_score_up_follows_the_cumulative_combo() {
    let m = master();
    let with = run(&m, &[gekisou_performer(2, &[(1, 1)])], [1, 1, 1], |_| 5).score();
    let without = run(&m, &[gekisou_performer(2, &[])], [1, 1, 1], |_| 5).score();
    assert!(with > without);
    // without a Gekisou skill the support skill is not built
    let no_skill = Performer { gekisou_support_skills: vec![(1, 1)], ..Default::default() };
    assert_eq!(run(&m, &[no_skill], [1, 1, 1], |_| 5).score(), without);
}

#[test]
fn gekisou_off_ignores_gekisou_skills() {
    let m = master();
    let (notes, events) = chart();
    let (p, _) = play(&notes, |_| 5);
    let deck = [gekisou_performer(1, &[(1, 1)])];
    let mut a = LiveModel::new(&m, &deck, &notes, &events, params(&notes)).unwrap();
    let mut b = LiveModel::new(&m, &[Performer::default()], &notes, &events, params(&notes)).unwrap();
    assert_eq!(a.run(&p).unwrap(), b.run(&p).unwrap());
    assert!(a.gekisou_ranges().is_empty());
    assert_eq!(a.rank_bonus_score(), 0);
}

#[test]
fn gekisou_conditions_fail_without_gekisou() {
    // a snap skill triggered by a Gekisou range start
    let m = master();
    let (notes, events) = chart();
    let (p, _) = play(&notes, |_| 5);
    let deck = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
    let mut lm = LiveModel::new(&m, &deck, &notes, &events, params(&notes)).unwrap();
    assert!(matches!(lm.run(&p), Err(Error::Unsupported(_))));
    let mut on = LiveModel::new_gekisou(&m, &deck, &notes, &events, params(&notes), &setup([1, 1, 1])).unwrap();
    assert!(on.run(&p).is_ok());
}

#[test]
fn invalid_gekisou_inputs_are_rejected() {
    let m = master();
    let (notes, events) = chart();
    let (p, _) = play(&notes, |_| 5);
    // three ranges; the fourth fever's start fails in the controller (native IndexOutOfRangeException)
    let four = GekisouSetup { fevers: vec![(0, 1), (2, 3), (4, 5), (6, 7)], missions: vec![1, 1, 1, 1] };
    let mut lm = LiveModel::new_gekisou(&m, &[], &notes, &events, params(&notes), &four).unwrap();
    assert_eq!(lm.gekisou_ranges().len(), 3);
    assert!(matches!(lm.run(&p), Err(Error::Game(_))));
    let short = GekisouSetup { fevers: vec![(START, END)], missions: vec![1] };
    assert!(LiveModel::new_gekisou(&m, &[], &notes, &events, params(&notes), &short).is_err());
    let unknown = [gekisou_performer(99, &[])];
    assert!(matches!(
        LiveModel::new_gekisou(&m, &unknown, &notes, &events, params(&notes), &setup([1, 1, 1])),
        Err(Error::Master(_))
    ));
    let mut lm = LiveModel::new_gekisou(&m, &[], &notes, &events, params(&notes), &setup([1, 1, 1])).unwrap();
    assert!(matches!(lm.run_timed(&p, &[0.016]), Err(Error::Input(_))));
}

#[test]
fn fixed_and_cumulative_additions_run_through_real_appliers() {
    for et in [12002, 13003, 11004, 12003, 13004] {
        let mut t = tables();
        t["MasterSkillEffectSetting"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_id": 99, "_skillEffectType": et, "_phase": 2}));
        let cumulative = matches!(et, 11004 | 12003 | 13004);
        t["MasterSkillCumulativeCondition"].as_array_mut().unwrap().push(json!({
            "_id": 99, "_skillCumulativeConditionType": 2001,
            "_conditionValues": [100], "_conditionTargetIDs": [], "_maxCumulativeCount": 1000
        }));
        t["MasterGekisouSkillEffect"][0] = effect(
            1,
            "_gekisouSkillID",
            1,
            et,
            if cumulative { 100 } else { 7 },
            json!({
                "_skillCumulativeConditionID": if cumulative { 99 } else { 0 }, "_maxEffectValue": 250
            }),
        );
        let lm = run(&master_from(&t), &[gekisou_performer(1, &[])], [1, 1, 1], |_| 6);
        let r = &lm.gekisou_ranges()[0];
        match et {
            11004 => assert_eq!(r.luck_points, 250),
            12002 => assert_eq!(r.combo, 28),
            12003 => assert_eq!(r.combo, 271),
            13003 => assert_eq!(r.just_count, 28),
            13004 => assert_eq!(r.just_count, 271),
            _ => unreachable!(),
        }
    }
}

#[test]
fn dynamic_ranges_wait_for_explicit_confirmation_and_retry_idempotently() {
    let m = master();
    let (notes, events) = chart();
    let (p, dts) = play(&notes, |_| 6);
    let setup = GekisouSetup {
        fevers: vec![(1200, 1600), (2100, 2500), (3100, 3500), (4100, 4500)],
        missions: vec![1, 2, 3, 4],
    };
    let mut lm = LiveModel::new_gekisou_external(&m, &[], &notes, &events, params(&notes), &setup).unwrap();
    lm.run_timed(&p, &dts).unwrap();
    assert_eq!(lm.gekisou_ranges().len(), 4);
    assert!(lm.gekisou_rank_bonuses().is_empty());
    // Fourth range is All, not an implicit Luck range, and requires no fabricated fourth master factor.
    assert_eq!(lm.gekisou_ranges()[3].luck_points, 0);
    let base = lm.score();
    lm.queue_gekisou_rank_confirmation(3, 2, 175).unwrap();
    lm.queue_gekisou_rank_confirmation(3, 2, 175).unwrap();
    assert!(lm.queue_gekisou_rank_confirmation(3, 1, 175).is_err());
    lm.frame_timed(8100, &[], 0.016).unwrap();
    lm.frame_timed(8200, &[], 0.016).unwrap();
    let bonuses = lm.gekisou_rank_bonuses();
    assert_eq!(bonuses.len(), 1);
    assert_eq!((bonuses[0].0, bonuses[0].1, bonuses[0].3), (3, 2, 175));
    assert_eq!(lm.score(), base.wrapping_add(bonuses[0].2));
    lm.queue_gekisou_rank_confirmation(3, 2, 175).unwrap();
    lm.frame_timed(8300, &[], 0.016).unwrap();
    assert_eq!(lm.gekisou_rank_bonuses().len(), 1);
    if let Ok(path) = std::env::var("GEKISOU_EXTERNAL_FIXTURE") {
        let fixture = json!({"tables": tables(), "score": lm.score(), "trace": lm.trace(),
            "bonuses": lm.gekisou_rank_bonuses(), "fevers": setup.fevers, "missions": setup.missions});
        std::fs::write(path, serde_json::to_vec(&fixture).unwrap()).unwrap();
    }
}

#[test]
fn external_confirmations_wait_for_completion_and_capture_all_completed_ranges() {
    let m = master();
    let (notes, events) = chart();
    let (p, dts) = play(&notes, |_| 6);
    let setup = GekisouSetup { fevers: vec![(START, END); 4], missions: vec![1, 3, 4, 1] };
    let mut lm = LiveModel::new_gekisou_external(&m, &[], &notes, &events, params(&notes), &setup).unwrap();
    for idx in 0..4 {
        lm.queue_gekisou_rank_confirmation(idx, idx as i32 + 1, 100).unwrap();
    }
    lm.frame_timed(0, &[], 0.0).unwrap();
    assert!(lm.gekisou_rank_bonuses().is_empty());
    lm.run_timed(&p, &dts).unwrap();
    assert_eq!(lm.gekisou_rank_bonuses().len(), 4);
    for (idx, &(range, rank, _, percent)) in lm.gekisou_rank_bonuses().iter().enumerate() {
        assert_eq!((range, rank, percent), (idx, idx as i32 + 1, 100));
    }
    assert!(lm.queue_gekisou_rank_confirmation(4, 1, 100).is_err());
    assert!(lm.queue_gekisou_rank_confirmation(0, 0, 100).is_err());
}

#[test]
fn snap_gekisou_active_is_a_range_start_event_not_member_skill_state() {
    let mut t = tables();
    t["MasterSkillCondition"].as_array_mut().unwrap().push(json!({"_id":999,"_conditionType":5021,
        "_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}));
    t["MasterSkillConditionSet"].as_array_mut().unwrap().push(json!({"_id":999,"_group":999,"_conditionIds":[999]}));
    t["MasterSupportSkillEffect"] = json!([effect(
        999,
        "_supportSkillID",
        999,
        3001,
        10,
        json!({"_skillTriggerConditionGroup":999,"_activationTimeSecond":0.0})
    )]);
    let master = master_from(&t);
    let deck = [Performer { support_skills: vec![(999, 1)], ..Default::default() }];
    let model = run(&master, &deck, [1, 1, 1], |_| 5);
    assert_eq!(model.current_life(), 1010);
}
