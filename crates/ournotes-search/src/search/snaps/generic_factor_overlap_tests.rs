//! A native regression for chart-time factors of repeatedly recycled ordinary support effects.
use super::*;
use crate::search::budget::SearchBudget;
use crate::search::gate_tests::common::{Rng, extend_table, replace_table, roster, set_column, synth_snaps};
use ournotes_sim::live::full::{JudgedNote, PlayFrame};
use serde_json::json;

const HITS: usize = 12;

fn master(effect_type: i64) -> Master {
    let mut source = synth_snaps(&mut Rng::new(787), 5, 1, &[3]);
    replace_table(&mut source, "MasterLiveSkillEffect", json!([]));
    set_column(&mut source, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(1);
        row["_supportSkillId02"] = json!(0);
    });
    set_column(&mut source, "MasterSupportCardRank", &mut |row| {
        row["_supportSkill01Level"] = json!(1);
        row["_supportSkill02Level"] = json!(0);
    });
    replace_table(&mut source, "MasterSupportSkill", json!([{"_id":1}]));
    // Keep the synthesized member/leader/power target references valid for full Pool admission.
    extend_table(&mut source, "MasterSkillTarget", vec![json!({"_id":9001,"_skillTargetType":4,"_judgement":5})]);
    replace_table(
        &mut source,
        "MasterSkillCondition",
        json!([
            {"_id":1,"_conditionType":1030,"_conditionValues":[1],"_conditionTargetIDs":[9001],"_isPositive":true},
            {"_id":2,"_conditionType":8000,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}
        ]),
    );
    replace_table(
        &mut source,
        "MasterSkillConditionSet",
        json!([
            {"_id":1,"_group":1,"_conditionIds":[1]},
            {"_id":2,"_group":2,"_conditionIds":[2]}
        ]),
    );
    replace_table(
        &mut source,
        "MasterSupportSkillEffect",
        json!([{
            "_id":1,"_supportSkillID":1,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":1,"_skillConditionGroup":0,"_skillReleaseConditionGroup":2,
            "_skillTargetIDs":[9001],"_skillEffectType":effect_type,"_activationTimeSecond":0.05,
            "_effectValue":5000,"_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
        }]),
    );
    source.master()
}

fn setup() -> FullSetup {
    FullSetup {
        // Distinct, nonnegative IDs and matching judgement timestamps remain inside the admitted search domain.
        notes: (0..HITS)
            .map(|id| LiveNote { note_id: id as i32, time_ms: 0, note_operate_type: 1, judgement_type: 1 })
            .collect(),
        events: Vec::new(),
        play: LivePlay {
            frames: (0..HITS * 5 + 5)
                .map(|frame| PlayFrame {
                    time_ms: frame as i32 * 40,
                    judged: if frame < HITS * 5 && frame % 5 == 0 {
                        vec![JudgedNote { note_id: (frame / 5) as i32, judgement: 5, judgement_time_ms: 0 }]
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
            base_seed: 0,
        },
        params: LiveParams {
            skill_target_music_type: 0,
            total_power: 100_000,
            music_level: 1,
            converted_note_count: HITS as i32,
            music_length_ms: 10_000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        },
        gk: None,
    }
}

fn bounds(master: &Master, setup: &FullSetup) -> (f64, f64, f64) {
    let frames: Vec<_> = setup.play.frames.iter().map(|frame| frame.time_ms).collect();
    let entries: Vec<_> = setup.notes.iter().enumerate().map(|(i, &note)| (i * 5, note, 5)).collect();
    let schedule = Schedule { states: vec![Vec::new(); frames.len()], ranges: Vec::new() };
    let mut env = Env {
        master,
        events: &setup.events,
        sets: HashMap::new(),
        life_lo: 0,
        life_hi: 1000,
        life_rigid: true,
        raw: vec![5; HITS],
        count_reach: std::array::from_fn(|j| 1u8 << j),
        entry_reach: Vec::new(),
        gk: None,
        gkf: Some(Rc::new(GkFrames::new(&schedule, &frames, &entries))),
        rush_cache: RefCell::new(HashMap::new()),
        gk_cache: RefCell::new(HashMap::new()),
        budget_cache: RefCell::new(HashMap::new()),
        ramp_cache: RefCell::new(HashMap::new()),
    };
    for set in &master.skill_condition_sets {
        env.sets.entry(set.group).or_default().push(&set.condition_ids);
    }
    let rows = support_rows(&env, 1, 1).unwrap();
    assert_eq!(rows.len(), 1);
    let active = active_row(&env, &rows[0], true, false).unwrap();
    assert!(active.count_win.is_none(), "a release checker must use the generic fallback");
    assert!(active.gk_win.is_none() && !active.event_bound && !active.churn);
    assert!(active.start_limit.unwrap() >= HITS as f64);
    let times = vec![0; HITS];
    let exec = Exec::new(setup, &times, None, &[]);
    let geo = Geo {
        frames: &frames,
        times: &times,
        exec: &exec,
        music_length_ms: setup.params.music_length_ms,
        snapshot_frame_limit: None,
    };
    let (windows, commands, norm, _, spans, _, _, _, _) = windows(&geo, 0, &[], &[active], &[]);
    assert!(commands >= (2 * HITS) as f64 && commands < (2 * HITS + 1) as f64);
    assert_eq!(windows.len(), 1);
    assert_eq!((windows[0].lo, windows[0].hi), (0, HITS as u32));
    assert_eq!(spans.len(), 1);
    (windows[0].note + windows[0].judge[2], norm[2], spans[0].2)
}

#[test]
fn false_release_and_delayed_judgements_keep_every_native_historical_factor() {
    let setup = setup();
    for effect_type in [2000, 2004] {
        let master = master(effect_type);
        // Exercise full admission too: this is a supported ordinary game law, not merely a handmade ActiveRow.
        let owned = roster(&mut Rng::new(788), &master);
        let pool = Pool::new(&master, &owned).unwrap();
        let tables =
            Tables::new(&pool, None, false, &[0], SearchBudget::new(Instant::now(), None).unwrap()).unwrap().unwrap();
        let _envelope = SnapLive::new(&pool, &tables, &[true; 5], &setup).unwrap();
        let (gain, norm, span) = bounds(&master, &setup);
        for bound in [gain, norm, span] {
            assert!(bound >= HITS as f64 * 0.5 && bound < HITS as f64 * 0.5 + 0.001);
        }

        let mut deck = vec![Performer::default(); 5];
        let mut baseline = LiveModel::new(&master, &deck, &setup.notes, &setup.events, setup.params).unwrap();
        let base = baseline.run(&setup.play).unwrap() as f64;
        deck[0].support_skills.push((1, 1));
        let mut native = LiveModel::new(&master, &deck, &setup.notes, &setup.events, setup.params).unwrap();
        let score = native.run(&setup.play).unwrap() as f64;
        // Native conditions backdate all twelve starts to 0. The false release checker skips the first timer
        // check, so each recycled instance ends at 50: all twelve historical factors cover every note.
        // Integer note floors only change these totals by at most a few units, far below this strict gap.
        assert!(base > 0.0);
        assert!(score > base * (1.0 + POOL * 0.5) + 100.0, "the old pool-only gain underbounds native {effect_type}");
        assert!(score <= (base + HITS as f64) * (1.0 + gain) * (1.0 + CHAIN_EPS));
        #[cfg(feature = "search-diagnostics")]
        for (_, _, _, factors) in native.filed_scores().0 {
            assert_eq!(factors[2], 1.0 + HITS as f32 * 0.5);
            assert!((factors[2] - 1.0) as f64 <= gain);
        }
    }
}

fn with_free_envelope(master: &Master, setup: &FullSetup, check: impl FnOnce(&SnapLive<'_>)) {
    assert!(setup.gk.is_none());
    let owned = roster(&mut Rng::new(789), master);
    let pool = Pool::new(master, &owned).unwrap();
    let tables =
        Tables::new(&pool, None, false, &[0], SearchBudget::new(Instant::now(), None).unwrap()).unwrap().unwrap();
    let live = SnapLive::new(&pool, &tables, &[true; 5], setup).unwrap();
    check(&live);
}

fn native_score(master: &Master, setup: &FullSetup, enabled: bool) -> i32 {
    let mut deck = vec![Performer::default(); 5];
    if enabled {
        deck[0].support_skills.push((1, 1));
    }
    LiveModel::new(master, &deck, &setup.notes, &setup.events, setup.params).unwrap().run(&setup.play).unwrap()
}

/// Native parsing, updater recycling and score-frame undo supply the oracle. The search is only asked to
/// enclose that independent score and, with diagnostics, each actual filed factor at the note's chart time.
fn assert_native_enclosed(live: &SnapLive<'_>, expected_factor: f32) -> i32 {
    assert!(expected_factor.is_finite() && expected_factor >= 1.0);
    let class = live.class_of[0][0] as usize;
    let contribution = &live.contrib[0][class][0];
    let mut deck = vec![Performer::default(); 5];
    deck[0].support_skills.push((1, 1));
    let setup = live.setup;
    let mut native = LiveModel::new(live.master, &deck, &setup.notes, &setup.events, setup.params).unwrap();
    let score = native.run(&setup.play).unwrap();
    assert!(score > 0);
    assert!(i64::from(score) <= live.gain_bound(i64::from(setup.params.total_power), contribution.gain));
    #[cfg(feature = "search-diagnostics")]
    {
        let filed = native.filed_scores().0;
        assert_eq!(filed.len(), setup.notes.len());
        for (time, _, _, factors) in filed {
            assert_eq!(factors[2], expected_factor);
            let entry = live.coef.times.iter().position(|&t| t == time).unwrap() as u32;
            let gain: f64 = contribution
                .windows
                .iter()
                .filter(|window| window.lo <= entry && entry < window.hi)
                .map(|window| window.note + window.judge.iter().copied().fold(0.0, f64::max))
                .sum();
            assert!((factors[2] - 1.0) as f64 <= gain);
        }
    }
    score
}

#[test]
fn free_count_windows_tighten_timely_notes_without_losing_late_recycled_starts() {
    for effect_type in [2000, 2004] {
        let mut master = master(effect_type);
        master.support_skill_effects[0].skill_release_condition_group = 0;
        for late in [false, true] {
            let mut setup = setup();
            if !late {
                for (i, note) in setup.notes.iter_mut().enumerate() {
                    note.time_ms = i as i32 * 200;
                }
                for frame in &mut setup.play.frames {
                    for judgement in &mut frame.judged {
                        judgement.judgement_time_ms = frame.time_ms;
                    }
                }
            }
            with_free_envelope(&master, &setup, |live| {
                let class = live.class_of[0][0] as usize;
                let row = &live.classes[0][class].rows[0];
                let windows = row.count_win.as_ref().expect("production Free construction must retain count frames");
                assert_eq!(windows.len(), HITS);
                assert!(row.start_limit.unwrap() >= HITS as f64);
                assert!(row.gk_win.is_none() && !row.event_bound);
                let contribution = &live.contrib[0][class][0];
                assert_eq!(contribution.windows.len(), HITS);
                for (entry, &time) in live.coef.times.iter().enumerate() {
                    let overlap = windows.iter().filter(|&&(a, b, _)| a <= time as i64 && (time as i64) < b).count();
                    assert_eq!(overlap, if late { HITS } else { 1 });
                    let gain: f64 = contribution
                        .windows
                        .iter()
                        .filter(|w| w.lo <= entry as u32 && (entry as u32) < w.hi)
                        .map(|w| w.note + w.judge[2])
                        .sum();
                    assert_eq!(gain, overlap as f64 * 0.5);
                }
                let expected = 1.0 + if late { HITS as f32 * 0.5 } else { 0.5 };
                let score = assert_native_enclosed(live, expected) as f64;
                let baseline = native_score(&master, &setup, false) as f64;
                assert!((score - baseline * expected as f64).abs() < HITS as f64 * expected as f64);
                if late {
                    // Even without a release checker, recycled executions start at chart time 0 and their
                    // later first timer update ends them after 0. Physical concurrency is not a history cap.
                    assert!(score > baseline * (1.0 + POOL * 0.5) + 100.0);
                    assert!(contribution.fac >= HITS as f64 * 0.5);
                } else {
                    assert!(contribution.fac < 1.0, "the production factor norm must use the separated windows");
                }
            });
        }
    }
}

#[test]
fn free_counter_duplicates_and_same_frame_hits_preserve_residue_across_miss() {
    let mut master = master(2000);
    master.support_skill_effects[0].skill_release_condition_group = 0;
    master.skill_conditions[0].condition_values = vec![3];
    master.skill_conditions[0].condition_target_ids = vec![9001, 9001];
    let mut setup = setup();
    setup.notes.clear();
    for frame in &mut setup.play.frames {
        frame.judged.clear();
    }
    // Counts: 2, unchanged by Miss, then 6, then 2. The middle frame crosses two thresholds, but starts one
    // updater; its residue 2 makes the final frame start another. Treating 1030 as consecutive loses that start.
    for (frame, judgements) in [(5, vec![5]), (7, vec![1]), (10, vec![5, 5, 5]), (15, vec![5])] {
        for judgement in judgements {
            let note_id = setup.notes.len() as i32;
            setup.notes.push(LiveNote { note_id, time_ms: 0, note_operate_type: 1, judgement_type: 1 });
            setup.play.frames[frame].judged.push(JudgedNote { note_id, judgement, judgement_time_ms: 0 });
        }
    }
    setup.params.converted_note_count = setup.notes.len() as i32;
    with_free_envelope(&master, &setup, |live| {
        let class = live.class_of[0][0] as usize;
        let row = &live.classes[0][class].rows[0];
        let windows = row.count_win.as_ref().unwrap();
        assert_eq!(windows.len(), 2, "native cached trigger truth starts at most once per processing frame");
        assert!(windows.iter().all(|&(a, b, m)| a <= 0 && b > 0 && m == 1.0));
        assert!(row.start_limit.unwrap() >= 3.0 && row.start_limit.unwrap() < 4.0);
        let score = assert_native_enclosed(live, 2.0) as f64;
        let baseline = native_score(&master, &setup, false) as f64;
        assert!((score - 2.0 * baseline).abs() < 2.0 * setup.notes.len() as f64);
        assert!(score > 1.5 * baseline + 100.0, "the extra native execution must be observable in real scores");
    });
}

#[test]
fn free_conversion_changes_counter_hits_and_declines_fixed_hit_frames() {
    let mut master = master(2000);
    master.support_skill_effects[0].skill_release_condition_group = 0;
    master.skill_targets.push(SkillTargetRow { id: 9002, skill_target_type: 4, judgement: 4, ..Default::default() });
    master.skill_conditions[1].condition_type = 4010;
    let mut converter = master.support_skill_effects[0].clone();
    converter.id = 2;
    converter.skill_trigger_condition_group = 2;
    converter.skill_effect_type = 12006;
    converter.skill_target_ids = vec![9002];
    converter.effect_value = 5;
    converter.activation_time_second = 10.0;
    master.support_skill_effects.push(converter);
    master.reindex().unwrap();
    let mut setup = setup();
    setup.events = vec![(0, 0)];
    for frame in &mut setup.play.frames {
        frame.judged.clear();
    }
    for id in 0..HITS {
        setup.play.frames[(id + 1) * 5].judged.push(JudgedNote {
            note_id: id as i32,
            judgement: 4,
            judgement_time_ms: 0,
        });
    }
    with_free_envelope(&master, &setup, |live| {
        let class = live.class_of[0][0] as usize;
        let row = live.classes[0][class].rows.iter().find(|r| r.effect_type == 2000).unwrap();
        // The admitted pool allows Great unchanged or converted to Perfect. Equal raw marginals cannot fix
        // the trigger frames: all twelve native executions below are created by the real converter.
        assert!(row.count_win.is_none());
        assert!(row.start_limit.unwrap() >= HITS as f64 && row.start_limit.unwrap() < HITS as f64 + 1.0);
        let score = assert_native_enclosed(live, 1.0 + HITS as f32 * 0.5) as f64;
        let mut without_counter = master.clone();
        without_counter.support_skill_effects[0].effect_value = 0;
        let baseline = native_score(&without_counter, &setup, true) as f64;
        assert!(score > baseline * (1.0 + POOL * 0.5) + 100.0);
        assert!((score - baseline * (1.0 + HITS as f64 * 0.5)).abs() < HITS as f64 * 7.0);
    });
}

#[test]
fn free_counter_targets_keep_raw_i64_identity_before_native_grade_comparison() {
    let mut master = master(2000);
    master.support_skill_effects[0].skill_release_condition_group = 0;
    master.skill_targets.iter_mut().find(|target| target.id == 9001).unwrap().judgement = (1i64 << 32) + 5;
    let setup = setup();
    with_free_envelope(&master, &setup, |live| {
        let class = live.class_of[0][0] as usize;
        let row = &live.classes[0][class].rows[0];
        assert_eq!(row.count_win.as_deref(), Some([].as_slice()));
        assert!(row.start_limit.unwrap() < 1.0);
        assert!(live.contrib[0][class][0].windows.is_empty());
        assert_eq!(assert_native_enclosed(live, 1.0), native_score(&master, &setup, false));
    });
}
