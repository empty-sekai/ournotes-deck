//! The historical envelope preserves alternatives within a lifetime and addition between lifetimes.
use super::*;
use crate::search::budget::SearchBudget;
use crate::search::gate_tests::common::{Rng, extend_table, replace_table, roster, set_column, synth_snaps};
use ournotes_sim::live::full::{JudgedNote, PlayFrame};
use ournotes_sim::replay::RankConfirmation;
use serde_json::json;

fn setup(late: bool) -> FullSetup {
    let times = [100, 120, 140, 140, 160, 180, 200, 220, 240, 1100, 1120, 1140, 1160, 1180, 1200, 1220, 1240];
    let notes: Vec<_> = times
        .into_iter()
        .enumerate()
        .map(|(id, time_ms)| LiveNote { note_id: id as i32, time_ms, note_operate_type: 1, judgement_type: 1 })
        .collect();
    let frames = (0..126)
        .map(|i| {
            let time_ms = i * 20;
            let judged = notes
                .iter()
                .filter(|note| (if late && note.note_id == 2 { 200 } else { note.time_ms }) == time_ms)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 6, judgement_time_ms: note.time_ms })
                .collect();
            PlayFrame { time_ms, judged }
        })
        .collect();
    let mut setup = FullSetup {
        params: LiveParams {
            skill_target_music_type: 0,
            total_power: 100_000,
            music_level: 1,
            converted_note_count: notes.len() as i32,
            music_length_ms: 2000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        },
        notes,
        events: Vec::new(),
        play: LivePlay { frames, base_seed: 0 },
        gk: None,
    };
    setup.set_gekisou(
        GekisouSetup { fevers: vec![(80, 280), (1080, 1280)], missions: vec![3, 3] },
        vec![0.02; setup.play.frames.len()],
        vec![0],
    );
    setup.gk.as_mut().unwrap().confirmations =
        Some((0..2).map(|range| RankConfirmation { frame: 0, range, rank: 1, percent: 250 }).collect());
    setup
}

fn master(release: bool) -> Master {
    let mut source = synth_snaps(&mut Rng::new(9011), 5, 1, &[3]);
    set_column(&mut source, "MasterMemberCard", &mut |row| {
        row["_gekisouSkillID"] = json!(1);
    });
    replace_table(&mut source, "MasterLiveSkillEffect", json!([]));
    replace_table(&mut source, "MasterSupportSkillEffect", json!([]));
    set_column(&mut source, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(0);
        row["_supportSkillId02"] = json!(0);
        row["_gekisouSupportSkillId01"] = json!(1);
        row["_gekisouSupportSkillId02"] = json!(0);
    });
    set_column(&mut source, "MasterSupportCardRank", &mut |row| {
        row["_gekisouSupportSkill01Level"] = json!(1);
    });
    extend_table(
        &mut source,
        "MasterLiveSettings",
        vec![
            json!({"_id":901,"_key":"gekisou_luck_gauge_max","_value":"140"}),
            json!({"_id":902,"_key":"gekisou_luck_gauge_max_rush","_value":"70"}),
            json!({"_id":903,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    extend_table(
        &mut source,
        "MasterSkillTarget",
        vec![
            json!({"_id":9001,"_skillTargetType":5,"_gekisouMissionType":3}),
            json!({"_id":9002,"_skillTargetType":4,"_judgement":6}),
        ],
    );
    extend_table(&mut source, "MasterSkillEffectSetting", vec![json!({"_id":9001,"_skillEffectType":2001,"_phase":2})]);
    extend_table(
        &mut source,
        "MasterSkillCondition",
        vec![
            json!({"_id":9001,"_conditionType":7010,"_conditionTargetIDs":[9001],"_isPositive":true}),
            json!({"_id":9002,"_conditionType":7013,"_conditionTargetIDs":[],"_isPositive":true}),
        ],
    );
    extend_table(
        &mut source,
        "MasterSkillConditionSet",
        vec![
            json!({"_id":9001,"_group":9001,"_conditionIds":[9001]}),
            json!({"_id":9002,"_group":9002,"_conditionIds":[9002]}),
        ],
    );
    replace_table(
        &mut source,
        "MasterSkillCumulativeCondition",
        json!([{"_id":9001,"_skillCumulativeConditionType":1000,"_conditionValues":[1],
            "_conditionTargetIDs":[9002],"_maxCumulativeCount":100}]),
    );
    replace_table(&mut source, "MasterGekisouSkill", json!([{"_id":1,"_gekisouMissionType":3}]));
    replace_table(&mut source, "MasterGekisouSkillEffect", json!([]));
    replace_table(&mut source, "MasterGekisouSupportSkill", json!([{"_id":1,"_gekisouMissionType":3}]));
    replace_table(
        &mut source,
        "MasterGekisouSupportSkillEffect",
        json!([{"_id":9001,"_gekisouSupportSkillID":1,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":9001,"_skillConditionGroup":0,
            "_skillReleaseConditionGroup":if release {9002} else {0},
            "_skillTargetIDs":[],"_skillEffectType":2001,
            "_activationTimeSecond":if release {0.0} else {9999.0},
            "_effectValue":1000,"_maxEffectValue":5000,"_effectLimitCount":0,
            "_skillCumulativeConditionID":9001,"_effectExecuteLimitCount":0,
            "_effectExecuteLimitResetConditionGroup":0}]),
    );
    source.master()
}

fn legacy_windows(geo: &Geo<'_>, executions: &[Vec<(i64, i64, i64)>]) -> Vec<Window> {
    executions
        .iter()
        .flatten()
        .filter_map(|&(a, b, value)| {
            let (lo, hi) = geo.range(a, b);
            let note = note_factor_mill(value as f32 / 10000f32).max(0) as f64 / 1e5;
            (lo < hi && note != 0.0).then_some(Window { lo, hi, note, judge: [0.0; 4], ramp: 0, rush: 0 })
        })
        .collect()
}

#[test]
fn cumulative_history_max_keeps_closed_endpoints_and_distinct_lifetimes() {
    let setup = setup(false);
    let frames: Vec<_> = setup.play.frames.iter().map(|frame| frame.time_ms).collect();
    let times = [100, 119, 120, 121, 139, 140, 160, 161];
    let exec = Exec::new(&setup, &times, None, &[]);
    let mut geo =
        Geo { frames: &frames, times: &times, exec: &exec, music_length_ms: 2000, snapshot_frame_limit: Some(100) };
    let pieces = [(80, 120, 1000), (120, 140, 2000), (140, 161, 3000)];
    let maximum = exclusive_ramp_ranges(&geo, &pieces);
    let legacy = legacy_windows(&geo, &[pieces.to_vec()]);
    let mut strict = false;
    for (entry, &time) in times.iter().enumerate() {
        let value: i64 = maximum.iter().filter(|w| w.0 <= entry as u32 && (entry as u32) < w.1).map(|w| w.2).sum();
        let wanted = pieces
            .iter()
            .filter(|&&(a, b, _)| a <= time as i64 && get_frame(time) <= get_frame(b as i32))
            .map(|p| p.2)
            .max()
            .unwrap_or(0);
        assert_eq!(value, wanted);
        let old: f64 = legacy.iter().filter(|w| w.lo <= entry as u32 && (entry as u32) < w.hi).map(|w| w.note).sum();
        strict |= value as f64 / 10000.0 < old;
    }
    assert!(strict);
    // The caller adds separate executions, even when they reuse the same native pool slot.
    let second = exclusive_ramp_ranges(&geo, &[(80, 200, 4000)]);
    let entry = 5;
    let sum: i64 = maximum.iter().chain(&second).filter(|w| w.0 <= entry && entry < w.1).map(|w| w.2).sum();
    assert_eq!(sum, 7000);
    geo.snapshot_frame_limit = None;
    let plain = exclusive_ramp_ranges(&geo, &pieces);
    for entry in 0..times.len() as u32 {
        let actual: i64 = plain.iter().filter(|w| w.0 <= entry && entry < w.1).map(|w| w.2).sum();
        let expected: i64 = pieces
            .iter()
            .filter(|&&(a, b, _)| a <= times[entry as usize] as i64 && (times[entry as usize] as i64) < b)
            .map(|p| p.2)
            .sum();
        assert_eq!(actual, expected, "half-open terminal-only geometry is unchanged");
    }
}

#[test]
fn cumulative_history_max_encloses_native_network_rank_and_overlapping_executions() {
    for release in [false, true] {
        let master = master(release);
        let mut owned = roster(&mut Rng::new(9012), &master);
        for member in &mut owned.members {
            member.gekisou_skill_level = 1;
        }
        let pool = Pool::new(&master, &owned).unwrap();
        let tables =
            Tables::new(&pool, None, false, &[0], SearchBudget::new(Instant::now(), None).unwrap()).unwrap().unwrap();
        for late in [false, true] {
            let setup = setup(late);
            let live = SnapLive::new(&pool, &tables, &[true; 5], &setup).unwrap();
            assert!(live.fine.network_ranking);
            let class = live.class_of[0][0] as usize;
            let row = live.classes[0][class].rows.iter().find(|row| row.effect_type == 2001).unwrap();
            let executions = row.cumulative_ramp.as_ref().expect("the production compiler must admit the ramp");
            assert_eq!(executions.len(), 2);
            let frames: Vec<_> = setup.play.frames.iter().map(|frame| frame.time_ms).collect();
            let exec = Exec::new(&setup, &live.coef.times, None, &[]);
            let geo = Geo {
                frames: &frames,
                times: &live.coef.times,
                exec: &exec,
                music_length_ms: setup.params.music_length_ms,
                snapshot_frame_limit: Some(ScoreFrames::new(&setup.params).last()),
            };
            let part = &live.contrib[0][class][0];
            let mut old = part.clone();
            old.windows = legacy_windows(&geo, executions);
            let view = FineView { coef: &live.coef, fine: &live.fine, chain_extra: live.chain_extra };
            assert_eq!(view.cand_drift([part; 5], None).to_bits(), view.cand_drift([&old; 5], None).to_bits());
            let mut strict = false;
            for power in [1, 12_345, 100_000] {
                let upper = view.fine_bound(power, [part; 5], [0; 5], CandLife::Unknown, &mut Scratch::default(), None);
                let old_upper =
                    view.fine_bound(power, [&old; 5], [0; 5], CandLife::Unknown, &mut Scratch::default(), None);
                assert!(upper <= old_upper);
                strict |= upper < old_upper;
                let performers = vec![
                    Performer {
                        gekisou_skill: Some((1, 1)),
                        gekisou_support_skills: vec![(1, 1)],
                        ..Default::default()
                    };
                    5
                ];
                let mut native = setup.gekisou_model(&master, &performers, power as i32).unwrap();
                for (frame, &dt) in setup.play.frames.iter().zip(&setup.gk.as_ref().unwrap().dt) {
                    native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
                    assert!(i64::from(native.score()) <= upper, "release={release} late={late} t={}", frame.time_ms);
                    #[cfg(feature = "search-diagnostics")]
                    for (time, _, _, factors) in native.filed_scores().0 {
                        let entry = live.coef.times.iter().position(|&t| t == time).unwrap() as u32;
                        let bound: f64 =
                            part.windows.iter().filter(|w| w.lo <= entry && entry < w.hi).map(|w| w.note).sum();
                        assert!(f64::from(factors[2]) <= 1.0 + 5.0 * bound + view.cand_drift([part; 5], None));
                    }
                }
                // rank_bonus_score counts already-filed bonuses executed again after a rewind;
                // an ordinary first award can leave that diagnostic at zero. Check the actual
                // network confirmation receipts and the controller's historical snapshots instead.
                let ranges = native.gekisou_ranges();
                let bonuses = native.gekisou_rank_bonuses();
                assert_eq!(bonuses.len(), 2);
                assert_eq!(native.rank_confirmation_applications().len(), 2);
                for (index, (range, &(awarded_range, rank, bonus, percent))) in ranges.iter().zip(bonuses).enumerate() {
                    assert_eq!(awarded_range, index);
                    assert_eq!(native.rank_confirmation_applications()[index].1, index);
                    assert!(native.rank_confirmation_applications()[index].0 + 1 < setup.play.frames.len());
                    assert_eq!((rank, percent), (1, 250));
                    let gain = range.end_score.wrapping_sub(range.start_score);
                    let expected = (i128::from(gain) * i128::from(percent) / 100) as i32;
                    assert_eq!(bonus, expected);
                    assert_eq!(range.rank_bonus, Some(expected));
                    if power > 1 {
                        assert!(gain > 0 && bonus > 0, "the oracle must award real historical rank snapshots");
                    }
                }
                assert_eq!(native.draws(), 0);
            }
            assert!(strict, "the new geometry must remove a nonzero historical overlap");
            if !release {
                let entry = live.coef.times.iter().position(|&t| t == 1240).unwrap() as u32;
                let bound: f64 = part.windows.iter().filter(|w| w.lo <= entry && entry < w.hi).map(|w| w.note).sum();
                assert!(bound > 0.5, "two simultaneous lifetime executions must still add");
            }
        }
    }
}
