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
