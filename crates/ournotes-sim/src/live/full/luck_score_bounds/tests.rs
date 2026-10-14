use super::bounds::{QueryParts, snapshot_difference};
use super::*;
use serde_json::json;

#[test]
fn snapshot_difference_cancels_only_shared_fixed_bonus_coefficients() {
    let fixed = [
        (1, 1, F64Interval::new(-1000.0, 2000.0).unwrap(), I32Interval::new(-1000, 2000).unwrap()),
        (2, 0, F64Interval::new(10.0, 20.0).unwrap(), I32Interval::new(10, 20).unwrap()),
    ];
    let part = |sum: i32| QueryParts {
        note_mean: F64Interval::integer(i128::from(sum)),
        note_support: I32Interval::point(sum),
        fixed_coefficients: vec![2, 1],
        to: 0,
        executed_from: 0,
        notes: None,
    };
    let mut queries = vec![part(100), part(130)];
    let (mean, support) = snapshot_difference(Some(0), 1, &queries, &fixed).unwrap();
    assert_eq!(support, I32Interval::point(30));
    assert!(mean.contains(30.0) && mean.upper() - mean.lower() < 1e-10);
    // A rewind removes the second bonus from the prefix, while the first bonus's permanent
    // filing offset and in-prefix copy are present on both sides. Only the latter cancels.
    queries[1].fixed_coefficients[1] = 0;
    let (mean, support) = snapshot_difference(Some(0), 1, &queries, &fixed).unwrap();
    assert_eq!(support, I32Interval::new(10, 20).unwrap());
    assert!(mean.contains(10.0) && mean.contains(20.0));
    // Initial network zero has no symbols, so neither a filing offset nor a prefix may vanish.
    let (_, support) = snapshot_difference(None, 1, &queries, &fixed).unwrap();
    assert_eq!(support, I32Interval::new(-1870, 4130).unwrap());
}

#[test]
fn snapshot_difference_cancels_notes_that_no_later_query_executed_again() {
    // Two notes in frames 1 and 2 whose lottery leaves each between 100 and 200 points, summed by the start
    // snapshot at frame 2, and one note in frame 4 between 30 and 60 points.
    let early = |frame| (frame, I32Interval::new(100, 200).unwrap(), F64Interval::new(100.0, 200.0).unwrap());
    let late = (4, I32Interval::new(30, 60).unwrap(), F64Interval::new(30.0, 60.0).unwrap());
    let part = |to: i32, executed_from: i32, notes: Vec<(i32, I32Interval, F64Interval)>, rank: bool| {
        let (lower, upper) = notes.iter().fold((0, 0), |(l, u), n| (l + n.1.lower(), u + n.1.upper()));
        QueryParts {
            note_mean: notes.iter().fold(F64Interval::ZERO, |m, n| m.add(n.2)),
            note_support: I32Interval::new(lower, upper).unwrap(),
            fixed_coefficients: Vec::new(),
            to,
            executed_from,
            notes: rank.then_some(notes),
        }
    };
    let mut queries = vec![
        part(2, 0, vec![early(1), early(2)], true),
        part(3, 3, vec![early(1), early(2)], false),
        part(4, 4, vec![early(1), early(2), late], true),
    ];
    let (mean, support) = snapshot_difference(Some(0), 2, &queries, &[]).unwrap();
    assert_eq!(support, I32Interval::new(30, 60).unwrap());
    assert_eq!((mean.lower(), mean.upper()), (30.0, 60.0));
    // A query between them executed frame 2 again: its note may have changed, so it is enclosed on each side.
    queries[1].executed_from = 2;
    let (_, support) = snapshot_difference(Some(0), 2, &queries, &[]).unwrap();
    assert_eq!(support, I32Interval::new(130 - 200, 260 - 100).unwrap());
    // From frame 0 on, every note is enclosed on each side.
    queries[1].executed_from = 0;
    let (_, support) = snapshot_difference(Some(0), 2, &queries, &[]).unwrap();
    assert_eq!(support, I32Interval::new(230 - 400, 460 - 200).unwrap());
    // Without the start snapshot's notes, so are the whole sums.
    queries[1].executed_from = 3;
    queries[0].notes = None;
    let (_, support) = snapshot_difference(Some(0), 2, &queries, &[]).unwrap();
    assert_eq!(support, I32Interval::new(230 - 400, 460 - 200).unwrap());
}

#[test]
fn recorder_refuses_hidden_random_checks() {
    let condition =
        Checker::And { items: vec![Checker::Fixed(false), Checker::Probability(0.25)], resettable: vec![false, false] };
    assert!(!deterministic(&condition));
    assert!(!deterministic(&Checker::LuckRushPlaying(false)));
    assert!(deterministic(&Checker::LifeAtLeast(Some(700))));
}

pub(super) fn fixture() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
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
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":60}],
        "MasterLiveGekisouLuckBonusLot":lots
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
fn completed_native_score_and_real_range_queries_are_enclosed() {
    let (master, notes, params, setup, play, delta) = fixture();
    let bounds = luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta).unwrap();
    let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
    let score = native.run_timed(&play, &delta).unwrap();
    assert!(bounds.final_mean.lower <= f64::from(score) && f64::from(score) <= bounds.final_mean.upper);
    assert_eq!(bounds.actual_queries as u64, 2 * play.frames.len() as u64 + 2);
    assert_eq!(bounds.ranges.len(), 1);
    let range = &bounds.ranges[0];
    assert_eq!(bounds.queries[range.start_query.unwrap()].time_ms, setup.fevers[0].0);
    assert_eq!(bounds.queries[range.end_query].time_ms, setup.fevers[0].1);
    assert_eq!(bounds.final_notes.len(), notes.len());
    assert_eq!(bounds.queries.iter().filter(|query| !query.notes.is_empty()).count(), 2);
    assert!(range.mean.upper - range.mean.lower < 1e-8);
    assert!(bounds.queries[range.end_query].notes.iter().all(|note| note.probability.is_some()));
}

#[test]
fn probabilistic_start_guarantees_match_the_full_nominal_law() {
    for (minimum, percent) in [(2, 5), (2, 60), (3, 5), (3, 60)] {
        let (mut master, notes, params, setup, play, delta) = fixture();
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
        for (id, kind, values) in [(1, 7010, vec![]), (2, 4011, vec![percent]), (3, 7013, vec![])] {
            master.skill_conditions.push(
                serde_json::from_value(json!({
                    "_id": id, "_conditionType": kind, "_conditionValues": values,
                    "_conditionTargetIDs": [], "_isPositive": true,
                }))
                .unwrap(),
            );
            master.skill_condition_sets.push(
                serde_json::from_value(json!({
                    "_id": id, "_group": id, "_conditionIds": [id],
                }))
                .unwrap(),
            );
        }
        master.skill_effect_settings.push(
            serde_json::from_value(json!({
                "_id": 1, "_skillEffectType": 11005, "_phase": 1,
            }))
            .unwrap(),
        );
        let skill = crate::master::SkillRow { id: 1, gekisou_mission_type: 2, ..Default::default() };
        master.gekisou_skills.push(skill.clone());
        master.gekisou_support_skills.push(skill);
        master.gekisou_support_skill_effects.push(
            serde_json::from_value(json!({
                "_id": 1, "_gekisouSupportSkillID": 1, "_level": 1,
                "_skillTriggerType": 1, "_skillTriggerConditionGroup": 1,
                "_skillConditionGroup": 2, "_skillReleaseConditionGroup": 3,
                "_skillEffectType": 11005, "_effectValue": minimum, "_effectLimitCount": 1,
            }))
            .unwrap(),
        );
        master.reindex().unwrap();
        let deck = [Performer {
            gekisou_skill: Some((1, 1)),
            gekisou_mission_type: 2,
            gekisou_support_skills: vec![(1, 1)],
            ..Default::default()
        }];
        let skills = luck_skills(&master).unwrap();
        let dp = luck_score_expectation(&master, &skills, &deck, &notes, &[], params, &setup, &play, &delta).unwrap();
        let mut expected = 0.0;
        for threshold in [0, 1001] {
            let mut branch_master = master.clone();
            let condition = branch_master.skill_conditions.iter_mut().find(|row| row.id == 2).unwrap();
            condition.condition_type = 2001;
            condition.condition_values = vec![threshold];
            branch_master.reindex().unwrap();
            let attempt = super::super::luck_exact::luck_exact_law_with_ranking(
                &branch_master,
                &deck,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                None,
                &mut super::super::luck_exact::LuckExactBudget::default(),
                || false,
            )
            .unwrap();
            let law = attempt.law.expect("the finite nominal tree completes");
            let probability = f64::from(percent as f32 / 100.0);
            expected += (if threshold == 0 { probability } else { 1.0 - probability })
                * law
                    .atoms()
                    .iter()
                    .map(|atom| f64::from(atom.score) * atom.mass.numerator as f64 / atom.mass.denominator as f64)
                    .sum::<f64>();
        }
        assert!(
            dp.final_mean.lower <= expected && expected <= dp.final_mean.upper,
            "minimum={minimum}, percent={percent}, nominal={expected}, dp={:?}",
            dp.final_mean
        );
    }
}

#[test]
fn later_native_ranges_cancel_old_random_bonuses_and_keep_all_seed_scores() {
    let (mut master, _, mut params, mut setup, _, _) = fixture();
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
    master.gekisou_ranking_score_bonuses = (1..=3)
        .map(|count| crate::master::GekisouRankingBonusRow {
            id: count,
            mission_pattern: gekisou::mission_pattern(2, 2, 2),
            rank: 1,
            count,
            score_bonus_percent: 250,
        })
        .collect();
    // Leave the native completion stopwatch enough frames to finish before the next range.
    setup.fevers = vec![(100, 500), (3600, 4100), (7200, 7700)];
    let notes: Vec<_> = (1..=18)
        .map(|i| LiveNote {
            note_id: i,
            note_operate_type: 1,
            judgement_type: 1,
            time_ms: i * 100
                + if i <= 6 {
                    0
                } else if i <= 12 {
                    3000
                } else {
                    6000
                },
        })
        .collect();
    params.converted_note_count = notes.len() as i32;
    params.music_length_ms = 11000;
    let frames: Vec<_> = (0..=110)
        .map(|i| PlayFrame {
            time_ms: i * 100,
            judged: notes
                .iter()
                .filter(|note| note.time_ms == i * 100)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let delta = vec![0.1; frames.len()];
    let play = LivePlay { frames, base_seed: 0 };
    let bounds = luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta).unwrap();
    assert_eq!(bounds.ranges.len(), 3);
    let mut strict = false;
    for range in bounds.ranges.iter().skip(1) {
        let start = &bounds.queries[range.start_query.unwrap()];
        let end = &bounds.queries[range.end_query];
        let naive_width = (end.mean.upper - end.mean.lower) + (start.mean.upper - start.mean.lower);
        let width = range.mean.upper - range.mean.lower;
        assert!(width <= naive_width + 1e-8);
        strict |= width + 0.1 < naive_width;
    }
    assert!(strict, "the fixture must expose a shared uncertain old bonus");
    let mut scores = std::collections::BTreeSet::new();
    for seed in -16..48 {
        let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
        native.set_seed(seed);
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        }
        assert!(bounds.final_support.lower <= native.score() && native.score() <= bounds.final_support.upper);
        scores.insert(native.score());
    }
    assert!(scores.len() > 1);
}

/// Three ranges with Rank bonuses of 250%: three notes at each of five or six times in the first two ranges,
/// whose Rush may or may not run, and a single note in the third. `fine` plays 10 ms frames and spreads each
/// three notes over 20 ms (one more note lies just before the second range), so that later filings execute
/// score frames of earlier snapshots again.
fn lottery_wide_ranges(fine: bool) -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let (mut master, _, mut params, mut setup, _, _) = fixture();
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
    master.gekisou_ranking_score_bonuses = (1..=3)
        .map(|count| crate::master::GekisouRankingBonusRow {
            id: count,
            mission_pattern: gekisou::mission_pattern(2, 2, 2),
            rank: 1,
            count,
            score_bonus_percent: 250,
        })
        .collect();
    setup.fevers = vec![(100, 500), (3600, 4100), (7200, 7700)];
    let spread = if fine { [0, 10, 20] } else { [0; 3] };
    let mut times: Vec<i32> = (1..=5).chain(36..=41).flat_map(|t| spread.map(|d| t * 100 + d)).collect();
    if fine {
        times.push(3590);
    }
    times.push(7300);
    times.sort_unstable();
    let notes: Vec<_> = times
        .iter()
        .enumerate()
        .map(|(i, &time_ms)| LiveNote { note_id: i as i32 + 1, note_operate_type: 1, judgement_type: 1, time_ms })
        .collect();
    params.converted_note_count = notes.len() as i32;
    params.music_length_ms = 11000;
    let step = if fine { 10 } else { 100 };
    let frames: Vec<_> = (0..=11000 / step)
        .map(|i| PlayFrame {
            time_ms: i * step,
            judged: notes
                .iter()
                .filter(|note| note.time_ms == i * step)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let delta = vec![step as f32 / 1000.0; frames.len()];
    (master, notes, params, setup, LivePlay { frames, base_seed: 0 }, delta)
}

#[test]
fn range_gains_after_lottery_wide_earlier_ranges_stay_enclosed_and_nonnegative() {
    let (master, notes, params, setup, play, delta) = lottery_wide_ranges(false);
    let bounds = luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta).unwrap();
    assert_eq!(bounds.ranges.len(), 3);
    for range in &bounds.ranges {
        assert!(range.support.lower >= 0, "{range:?}");
    }
    let mut gains = std::collections::BTreeSet::new();
    for seed in -16..48 {
        let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
        native.set_seed(seed);
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        }
        let gk = native.gk.as_ref().unwrap();
        for range in &bounds.ranges {
            let gain = gk.ctrl.range_score(range.range);
            assert!(range.support.lower <= gain && gain <= range.support.upper, "{range:?} {gain}");
            gains.insert((range.range, gain));
        }
        assert!(bounds.final_support.lower <= native.score() && native.score() <= bounds.final_support.upper);
    }
    assert!(gains.len() > bounds.ranges.len(), "the lottery must change some range gain");
}

#[test]
fn network_range_gains_over_executed_again_frames_stay_enclosed_and_nonnegative() {
    let (master, notes, params, setup, play, delta) = lottery_wide_ranges(true);
    let confirmations: Vec<_> =
        (0..3).map(|range| crate::replay::RankConfirmation { frame: 0, range, rank: 1, percent: 250 }).collect();
    let bounds =
        luck_score_bounds_with_ranking(&master, &[], &notes, &[], params, &setup, &play, &delta, Some(&confirmations))
            .unwrap();
    assert_eq!(bounds.ranges.len(), 3);
    for range in &bounds.ranges {
        assert!(range.support.lower >= 0, "{range:?}");
    }
    let mut gains = std::collections::BTreeSet::new();
    for seed in -16..48 {
        let mut native = LiveModel::new_gekisou_external(&master, &[], &notes, &[], params, &setup).unwrap();
        native.set_rank_confirmation_timeline(&confirmations).unwrap();
        native.set_seed(seed);
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        }
        let gk = native.gk.as_ref().unwrap();
        for range in &bounds.ranges {
            let gain = gk.ctrl.range_score(range.range);
            assert!(range.support.lower <= gain && gain <= range.support.upper, "{range:?} {gain}");
            gains.insert((range.range, gain));
        }
        assert!(bounds.final_support.lower <= native.score() && native.score() <= bounds.final_support.upper);
    }
    assert!(gains.len() > bounds.ranges.len(), "the lottery must change some range gain");
}

#[test]
fn compact_summary_preserves_the_same_proof_without_diagnostic_rows() {
    let (master, notes, params, setup, play, delta) = fixture();
    let full = luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta).unwrap();
    let skills = luck_skills(&master).unwrap();
    let compact = luck_score_bounds_internal(
        &master,
        &skills,
        &[],
        &notes,
        &[],
        params,
        &setup,
        &play,
        &delta,
        None,
        false,
        None,
        None,
        &mut || false,
        false,
        None,
    )
    .unwrap()
    .unwrap();
    assert!(compact.queries.is_empty() && compact.final_notes.is_empty());
    assert_eq!(compact.final_mean.lower, full.final_mean.lower);
    assert_eq!(compact.final_mean.upper, full.final_mean.upper);
    let summary = luck_score_summary_with_ranking(
        &master,
        &luck_skills(&master).unwrap(),
        &[],
        &notes,
        &[],
        params,
        &setup,
        &play,
        &delta,
        None,
    )
    .unwrap();
    assert_eq!(summary.exact_constant_score, Some(full.final_support.lower));
    assert_eq!(summary.final_support.lower, summary.final_support.upper);
    assert_eq!(summary.exact_final_life, full.exact_final_life);
}

#[test]
fn dynamic_max_does_not_inherit_a_constant_life_or_score_certificate() {
    let (mut master, notes, params, setup, play, delta) = fixture();
    master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
        id: 901,
        live_skill_id: 901,
        level: 1,
        skill_effect_type: 3000,
        effect_value: 100,
        ..Default::default()
    });
    master.reindex().unwrap();
    let deck = [Performer { live_skill: Some((901, 1)), ..Default::default() }];
    let events = [(0, 150)];
    let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap();
    native.run_timed(&play, &delta).unwrap();
    let certificate = luck_score_summary_with_ranking(
        &master,
        &luck_skills(&master).unwrap(),
        &deck,
        &notes,
        &events,
        params,
        &setup,
        &play,
        &delta,
        None,
    );
    assert!(matches!(certificate, Err(Error::Unsupported(message)) if message.contains("reader/cache")));
    // The native effect stays implemented; only the unjustified single-path certificate is refused.
    assert!(native.life.max_life() > 1000);
}

#[test]
fn exact_life_certificate_covers_misses_recovery_and_random_luck_paths() {
    let (mut master, notes, params, setup, mut play, delta) = fixture();
    master.judgement_parameters.push(crate::master::JudgementParameterRow {
        id: 2,
        note_simulate_judgement: 1,
        score_percent: 0,
        damage: 350,
    });
    master.gekisou_luck_base_points.push(crate::master::LuckBasePointRow {
        id: 2,
        note_simulate_judgement: 1,
        weight: 1,
        base_point: 0,
        ..Default::default()
    });
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
    master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 1,
        condition_type: 4010,
        condition_values: vec![],
        is_positive: true,
        condition_target_ids: vec![],
    });
    master.skill_condition_sets.push(crate::master::SkillConditionSetRow { id: 1, group: 1, condition_ids: vec![1] });
    master.support_skill_effects.push(crate::master::SupportSkillEffectRow {
        id: 1,
        support_skill_id: 1,
        level: 1,
        skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: 1,
        skill_effect_type: 3001,
        effect_value: 125,
        ..Default::default()
    });
    master.reindex().unwrap();
    play.frames
        .iter_mut()
        .flat_map(|frame| &mut frame.judged)
        .find(|note| note.note_id == notes[0].note_id)
        .unwrap()
        .judgement = 1;
    let deck = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
    let events = [(0, 300)];
    let summary = luck_score_summary_with_ranking(
        &master,
        &luck_skills(&master).unwrap(),
        &deck,
        &notes,
        &events,
        params,
        &setup,
        &play,
        &delta,
        None,
    )
    .unwrap();
    assert_eq!(summary.exact_final_life, Some(775));
    let mut common_life = None;
    let mut scores = std::collections::BTreeSet::new();
    for seed in -16..48 {
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap();
        native.set_seed(seed);
        let mut life = Vec::new();
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            life.push(native.current_life());
        }
        assert!(native.draws() > 0);
        assert_eq!(Some(native.current_life()), summary.exact_final_life);
        assert!(life.windows(2).any(|v| v[1] < v[0]), "the Miss must reduce life");
        assert!(life.windows(2).any(|v| v[1] > v[0]), "the support event must recover life");
        if let Some(reference) = &common_life {
            assert_eq!(&life, reference, "seed {seed}: the whole life trajectory is shared");
        } else {
            common_life = Some(life);
        }
        scores.insert(native.score());
    }
    assert!(scores.len() > 1, "the fixture must exercise distinct lottery-dependent scores");
    // This proof does not admit a recovery whose trigger reads Rush, even if one sampled path ends full.
    master.skill_conditions[0].condition_type = 7021;
    master.reindex().unwrap();
    assert!(
        luck_skills(&master)
            .and_then(|skills| luck_score_summary_with_ranking(
                &master, &skills, &deck, &notes, &events, params, &setup, &play, &delta, None,
            ))
            .is_err()
    );
}

#[test]
fn lottery_free_live_without_luck_range_plays_every_native_seed() {
    let (mut master, notes, params, mut setup, play, delta) = fixture();
    setup.missions = vec![1, 3, 1];
    master.skill_conditions.extend([
        crate::master::SkillConditionRow {
            id: 1,
            condition_type: 4011,
            condition_values: vec![50],
            is_positive: true,
            condition_target_ids: vec![],
        },
        crate::master::SkillConditionRow {
            id: 2,
            condition_type: 4010,
            condition_values: vec![],
            is_positive: true,
            condition_target_ids: vec![],
        },
    ]);
    master.skill_condition_sets.extend([
        crate::master::SkillConditionSetRow { id: 1, group: 1, condition_ids: vec![1] },
        crate::master::SkillConditionSetRow { id: 2, group: 2, condition_ids: vec![2] },
    ]);
    master.skill_effect_settings.push(crate::master::SkillEffectSettingRow {
        id: 1,
        skill_effect_type: 11003,
        phase: 1,
    });
    master.gekisou_skills.push(crate::master::SkillRow { id: 1, gekisou_mission_type: 1, ..Default::default() });
    // A probability-gated lottery chain: it feeds only the luck machine of a LUCK range.
    master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 1,
        skill_id: 1,
        level: 1,
        skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: 1,
        skill_effect_type: 11003,
        effect_value: 1,
        ..Default::default()
    });
    // A deterministic score-up on the skill event keeps the score nontrivial.
    master.support_skill_effects.push(crate::master::SupportSkillEffectRow {
        id: 1,
        support_skill_id: 1,
        level: 1,
        skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: 2,
        skill_effect_type: 2000,
        activation_time_second: 1.0,
        effect_value: 30_000,
        ..Default::default()
    });
    master.reindex().unwrap();
    let deck = [Performer {
        gekisou_skill: Some((1, 1)),
        gekisou_mission_type: 1,
        support_skills: vec![(1, 1)],
        ..Default::default()
    }];
    let events = [(0, 300)];
    let skills = luck_skills(&master).unwrap();
    let mut free = LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap();
    assert!(free.reads_lottery());
    prepare_lottery_free(&mut free, &skills).unwrap();
    let score = free.run_timed(&play, &delta).unwrap();
    assert_eq!(free.draws(), 0);
    let summary =
        luck_score_summary_with_ranking(&master, &skills, &deck, &notes, &events, params, &setup, &play, &delta, None)
            .unwrap();
    assert_eq!(summary.exact_constant_score, Some(score));
    assert_eq!(summary.exact_final_life, Some(free.current_life()));
    let plain = LiveModel::new_gekisou(&master, &[], &notes, &events, params, &setup).unwrap().run_timed(&play, &delta);
    assert!(score > plain.unwrap(), "the score-up must count");
    for seed in -16..48 {
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap();
        native.set_seed(seed);
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        }
        assert!(native.draws() > 0, "seed {seed}: the probability must draw natively");
        assert_eq!(native.score(), score, "seed {seed}");
        assert_eq!(native.current_life(), free.current_life(), "seed {seed}");
    }
    // A deck reading no probability plays as it is; a LUCK range draws lotteries.
    let mut quiet = LiveModel::new_gekisou(&master, &[], &notes, &events, params, &setup).unwrap();
    prepare_lottery_free(&mut quiet, &skills).unwrap();
    assert!(quiet.score.calc.luck_weight.is_none());
    setup.missions = vec![2, 3, 1];
    let mut luck = LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap();
    assert!(prepare_lottery_free(&mut luck, &skills).is_err());
}

#[test]
fn fixed_scores_outside_the_frame_array_remain_in_later_range_snapshots() {
    let (mut master, _, mut params, mut setup, _, _) = fixture();
    master.gekisou_ranking_score_bonuses = (1..=2)
        .map(|count| crate::master::GekisouRankingBonusRow {
            id: count,
            mission_pattern: 1,
            rank: 1,
            count,
            score_bonus_percent: 50,
        })
        .collect();
    params.music_level = 5;
    params.converted_note_count = 2;
    params.music_length_ms = 5000;
    params.score_music_length_ms = Some(1);
    setup.fevers = vec![(100, 3000), (500, 3500)];
    setup.missions = vec![1, 1, 1];
    let notes: Vec<_> = [200, 1000]
        .into_iter()
        .enumerate()
        .map(|(id, time_ms)| LiveNote { note_id: id as i32, note_operate_type: 1, judgement_type: 1, time_ms })
        .collect();
    let frames = (0..=50)
        .map(|frame| PlayFrame {
            time_ms: frame * 100,
            judged: notes
                .iter()
                .filter(|note| note.time_ms == frame * 100)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let play = LivePlay { frames, base_seed: 0 };
    let delta = vec![0.1; play.frames.len()];
    let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
    assert_eq!(native.run_timed(&play, &delta).unwrap(), 5250);
    let bounds = luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta).unwrap();
    assert!(bounds.final_mean.lower <= 5250.0 && bounds.final_mean.upper >= 5250.0);
    assert!(bounds.final_mean.upper < 5300.0);
    let second = &bounds.ranges[1];
    let start = &bounds.queries[second.start_query.unwrap()];
    assert!(start.mean.lower <= 3000.0 && start.mean.upper >= 3000.0);
    assert!(second.bonus_mean.lower <= 750.0 && second.bonus_mean.upper >= 750.0);
}

#[test]
fn deterministic_extension_cumulative_combo_and_conversion_keep_native_trace_enclosed() {
    let (mut master, notes, params, setup, play, delta) = fixture();
    for (id, kind) in [(101, 4010), (102, 7010), (103, 7013)] {
        master.skill_conditions.push(crate::master::SkillConditionRow {
            id,
            condition_type: kind,
            condition_values: vec![],
            is_positive: true,
            condition_target_ids: vec![],
        });
        master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id,
            group: id,
            condition_ids: vec![id],
        });
    }
    master.skill_targets.push(serde_json::from_value(json!({"_id":101,"_skillTargetType":4,"_judgement":5})).unwrap());
    master.cumulative_conditions.push(
        serde_json::from_value(json!({"_id":101,"_skillCumulativeConditionType":1000,
        "_conditionValues":[1],"_conditionTargetIDs":[101],"_maxCumulativeCount":99}))
        .unwrap(),
    );
    for (id, ty) in [(101, 2000), (102, 2002), (103, 15000), (104, 12006), (105, 2001)] {
        master
            .skill_effect_settings
            .push(serde_json::from_value(json!({"_id":id,"_skillEffectType":ty,"_phase":2})).unwrap());
    }
    master.live_skill_effects.push(
        serde_json::from_value(json!({"_id":101,"_liveSkillID":101,"_level":1,
        "_skillEffectType":2000,"_effectValue":2500,"_activationTimeSecond":0.35}))
        .unwrap(),
    );
    master.live_skill_effects.push(
        serde_json::from_value(json!({"_id":102,"_liveSkillID":101,"_level":1,
        "_skillEffectType":2002,"_effectValue":1200,"_activationTimeSecond":0.35}))
        .unwrap(),
    );
    master.support_skill_effects.push(
        serde_json::from_value(json!({"_id":103,"_supportSkillID":101,"_level":1,
        "_skillTriggerType":1,"_skillTriggerConditionGroup":101,"_skillEffectType":15000,"_effectValue":400}))
        .unwrap(),
    );
    master.support_skill_effects.push(
        serde_json::from_value(json!({"_id":104,"_supportSkillID":102,"_level":1,
        "_skillTriggerType":1,"_skillTriggerConditionGroup":101,"_skillEffectType":12006,"_effectValue":4,
        "_effectLimitCount":2,"_skillTargetIDs":[101],"_activationTimeSecond":0.5}))
        .unwrap(),
    );
    master.gekisou_skills.push(serde_json::from_value(json!({"_id":101,"_gekisouMissionType":2})).unwrap());
    master.gekisou_skill_effects.push(
        serde_json::from_value(json!({"_id":105,"_gekisouSkillID":101,"_level":1,
        "_skillTriggerType":1,"_skillTriggerConditionGroup":102,"_skillReleaseConditionGroup":103,
        "_skillEffectType":2001,"_effectValue":100,"_maxEffectValue":5000,"_skillCumulativeConditionID":101}))
        .unwrap(),
    );
    master.judgement_parameters.push(
        serde_json::from_value(json!({"_id":4,"_noteSimulateJudgement":4,
        "_scorePercent":80,"_damage":30}))
        .unwrap(),
    );
    master.gekisou_luck_base_points.push(
        serde_json::from_value(json!({"_id":4,"_noteCategory":0,"_noteSimulateJudgement":4,
        "_weight":1,"_basePoint":60}))
        .unwrap(),
    );
    master.reindex().unwrap();
    let deck = [Performer {
        live_skill: Some((101, 1)),
        support_skills: vec![(101, 1), (102, 1)],
        gekisou_skill: Some((101, 1)),
        gekisou_mission_type: 2,
        ..Default::default()
    }];
    let events = [(0, 80)];
    let bounds = luck_score_bounds(&master, &deck, &notes, &events, params, &setup, &play, &delta).unwrap();
    let skills = luck_skills(&master).unwrap();
    let mut cache = LuckDpCache::new(1 << 20);
    let mut session = LuckScoreSession::new(&master, &skills, &notes, &events, params, &setup, &play, &delta, None);
    for converted in [true, false, true, false] {
        let mut candidate = deck.clone();
        if !converted {
            candidate[0].support_skills.retain(|&(skill, _)| skill != 102);
        }
        let direct = luck_score_summary_with_ranking(
            &master, &skills, &candidate, &notes, &events, params, &setup, &play, &delta, None,
        )
        .unwrap();
        let cached = session.summary(&candidate, Some(&mut cache), || false).unwrap().unwrap();
        assert_eq!(serde_json::to_value(cached).unwrap(), serde_json::to_value(direct).unwrap());
    }
    let mut reference = None;
    for seed in -16..32 {
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap();
        native.set_seed(seed);
        let mut trace = Vec::new();
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            trace.push((
                native.current_life(),
                native.frame_judgements().to_vec(),
                native.gekisou_ranges().iter().map(|range| (range.combo, range.just_count)).collect::<Vec<_>>(),
            ));
        }
        assert!(bounds.final_support.lower <= native.score() && native.score() <= bounds.final_support.upper);
        assert_eq!(Some(native.current_life()), bounds.exact_final_life);
        assert!(
            trace.iter().flat_map(|(_, judgements, _)| judgements).any(|(_, j, _)| *j == 4),
            "conversion must execute"
        );
        if let Some(reference) = &reference {
            assert_eq!(&trace, reference);
        } else {
            reference = Some(trace);
        }
    }
    let condition = master.skill_conditions.iter_mut().find(|c| c.id == 101).unwrap();
    condition.condition_type = 4011;
    condition.condition_values = vec![50];
    master.reindex().unwrap();
    assert!(
        luck_score_bounds(&master, &deck, &notes, &events, params, &setup, &play, &delta).is_err(),
        "random conversion/extension cannot inherit the deterministic closure proof"
    );
}

#[test]
fn deterministic_combo_queries_replace_the_global_table_hull() {
    let (mut master, notes, params, mut setup, play, delta) = fixture();
    setup.missions = vec![1, 1, 1];
    master.combo_score_bonuses = vec![
        crate::master::ComboScoreBonusRow { id: 1, combo_bonus_type: 0, required_combo_count: 1, bonus_factor: 0.3 },
        crate::master::ComboScoreBonusRow { id: 2, combo_bonus_type: 1, required_combo_count: 2, bonus_factor: 0.4 },
    ];
    let bounds = luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta).unwrap();
    let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
    let score = native.run_timed(&play, &delta).unwrap();
    assert!(bounds.final_mean.lower <= f64::from(score) && bounds.final_mean.upper >= f64::from(score));
    assert!(bounds.final_mean.upper - bounds.final_mean.lower < 1e-8);
    assert!(bounds.final_notes.iter().all(|note| note.combo.lower == note.combo.upper));
}

#[test]
fn external_rank_uses_frame_snapshots_and_initial_zero_without_solo_queries() {
    let (master, notes, params, mut setup, play, delta) = fixture();
    setup.fevers[0].0 = 0;
    let confirmations = [crate::replay::RankConfirmation { frame: 0, range: 0, rank: 1, percent: 50 }];
    let bounds =
        luck_score_bounds_with_ranking(&master, &[], &notes, &[], params, &setup, &play, &delta, Some(&confirmations))
            .unwrap();
    let mut native = LiveModel::new_gekisou_external(&master, &[], &notes, &[], params, &setup).unwrap();
    native.set_rank_confirmation_timeline(&confirmations).unwrap();
    let score = native.run_timed(&play, &delta).unwrap();
    assert_eq!(bounds.actual_queries, 2 * play.frames.len());
    assert_eq!(bounds.ranges.len(), 1);
    assert_eq!(bounds.ranges[0].start_query, None);
    assert!(bounds.final_mean.lower <= f64::from(score) && bounds.final_mean.upper >= f64::from(score));
}

#[test]
fn fixed_rank_solo_timestamp_bounds_and_programs_match_complete_runs() {
    let (master, mut notes, mut params, mut setup, _, _) = fixture();
    setup.fevers = vec![(105, 255), (1205, 1455), (2505, 2855)];
    params.music_length_ms = 4000;
    let mut frames: Vec<_> = (0..=40).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
    for (note, time_ms) in notes.iter_mut().zip([100, 110, 120, 200, 260, 1250, 1310, 1400, 2600, 2800]) {
        note.time_ms = time_ms;
        frames.iter_mut().find(|frame| frame.time_ms >= time_ms).unwrap().judged.push(JudgedNote {
            note_id: note.note_id,
            judgement: 5,
            judgement_time_ms: time_ms,
        });
    }
    let delta = vec![0.1; frames.len()];
    let play = LivePlay { frames, base_seed: 7 };
    let confirmations = [
        crate::replay::RankConfirmation { frame: 0, range: 0, rank: 5, percent: 137 },
        crate::replay::RankConfirmation { frame: 35, range: 1, rank: 3, percent: 43 },
        crate::replay::RankConfirmation { frame: 0, range: 2, rank: 2, percent: 211 },
    ];
    let skills = luck_skills(&master).unwrap();
    for mission in [2, 3] {
        setup.missions = vec![mission; 3];
        let model_at = |power| {
            let mut model = LiveModel::new_gekisou_ranked(
                &master,
                &[],
                &notes,
                &[],
                LiveParams { total_power: power, ..params },
                &setup,
            )
            .unwrap();
            model.set_rank_confirmation_timeline(&confirmations).unwrap();
            model.track_score_up_factors();
            model
        };
        let mut origin = model_at(params.total_power);
        origin.score.begin_bounds(Vec::new(), mission == 2);
        let (program, terminal) = origin.compile_score_program(&play, &delta, LiveRandom::new(7)).unwrap();
        let trace = terminal.score.bounds_trace.as_ref().unwrap();
        let query_times: Vec<_> = trace
            .events
            .iter()
            .filter_map(|event| match event {
                BoundsEvent::Query { time_ms, .. } => Some(*time_ms),
                _ => None,
            })
            .collect();
        let rank_queries: Vec<_> = trace
            .events
            .iter()
            .filter_map(|event| match event {
                BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                    Some((*range, *time_ms, *percent, start.unwrap(), end.unwrap()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(query_times.len(), 2 * play.frames.len() + 2 * setup.fevers.len());
        assert_eq!(rank_queries.len(), setup.fevers.len());
        let mut ranked_ranges: Vec<_> = rank_queries.iter().map(|row| row.0).collect();
        ranked_ranges.sort_unstable();
        assert_eq!(ranked_ranges, vec![0, 1, 2]);
        for &(range, time_ms, percent, start, end) in &rank_queries {
            assert_eq!((query_times[start], query_times[end]), setup.fevers[range]);
            assert_eq!(time_ms, setup.fevers[range].1);
            assert_eq!(percent, confirmations[range].percent);
            assert!(start < end);
        }

        for power in [i32::MIN, -123_457, -1, 0, 1, 1000, 123_457, 16_777_217, i32::MAX] {
            let mut native = model_at(power);
            let score = native.run_with_random(&play, &delta, LiveRandom::new(7)).unwrap();
            assert_eq!(program.evaluate(power), score, "mission={mission} power={power}");
            assert_eq!(terminal.current_life(), native.current_life());
            assert_eq!(terminal.minimum_score_up(), native.minimum_score_up());
            for (range, confirmation) in native.gekisou_ranges().iter().zip(&confirmations) {
                let gain = range.end_score.wrapping_sub(range.start_score);
                let bonus = (i128::from(gain) * i128::from(confirmation.percent) / 100) as i32;
                assert_eq!(range.rank_bonus, Some(bonus));
            }
            if !(1..=123_457).contains(&power) {
                continue;
            }
            let bounds = luck_score_bounds_internal(
                &master,
                &skills,
                &[],
                &notes,
                &[],
                LiveParams { total_power: power, ..params },
                &setup,
                &play,
                &delta,
                Some(&confirmations),
                true,
                None,
                None,
                &mut || false,
                true,
                None,
            )
            .unwrap()
            .unwrap();
            assert_eq!(bounds.actual_queries, query_times.len());
            assert_eq!(bounds.queries.iter().map(|q| q.time_ms).collect::<Vec<_>>(), query_times);
            assert_eq!(bounds.ranges.len(), setup.fevers.len());
            for range in &bounds.ranges {
                let native_range = native.gekisou_ranges()[range.range];
                assert_eq!(
                    (bounds.queries[range.start_query.unwrap()].time_ms, bounds.queries[range.end_query].time_ms),
                    setup.fevers[range.range]
                );
                assert_eq!(range.percent, confirmations[range.range].percent);
                let score = native_range.end_score.wrapping_sub(native_range.start_score);
                let bonus = native_range.rank_bonus.unwrap();
                assert!(range.support.lower <= score && score <= range.support.upper);
                assert!(range.mean.lower <= f64::from(score) && f64::from(score) <= range.mean.upper);
                assert!(range.bonus_support.lower <= bonus && bonus <= range.bonus_support.upper);
                assert!(range.bonus_mean.lower <= f64::from(bonus) && f64::from(bonus) <= range.bonus_mean.upper);
            }
            assert!(bounds.final_support.lower <= score && score <= bounds.final_support.upper);
            assert!(bounds.final_mean.lower <= f64::from(score) && f64::from(score) <= bounds.final_mean.upper);
            if mission == 3 {
                assert_eq!((bounds.final_support.lower, bounds.final_support.upper), (score, score));
                assert_eq!(bounds.probability_transitions, 0);
            }
        }
    }
}

#[test]
fn terminal_probability_link_requires_a_finished_native_schedule() {
    let (master, notes, params, setup, mut play, mut delta) = fixture();
    play.frames.truncate(6);
    delta.truncate(6);
    assert!(matches!(
        luck_score_bounds(&master, &[], &notes, &[], params, &setup, &play, &delta),
        Err(Error::Unsupported(_))
    ));
}
