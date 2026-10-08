//! Independent native witnesses for the optional cumulative-COMBO work certificate.
use super::*;
use ournotes_sim::live::full::{JudgedNote, PlayFrame};
use ournotes_sim::replay::RankConfirmation;
use serde_json::json;

fn master(protection_seconds: f32) -> Master {
    let tables = json!({
        "MasterLiveSettings":[
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}
        ],
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[
            {"_id":1,"_noteSimulateJudgement":1,"_scorePercent":0,"_damage":0},
            {"_id":2,"_noteSimulateJudgement":6,"_scorePercent":100,"_damage":0}
        ],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":0}],
        "MasterSkillEffectSetting":[
            {"_id":1,"_skillEffectType":2001,"_phase":2},
            {"_id":2,"_skillEffectType":12000,"_phase":1},
            {"_id":3,"_skillEffectType":12004,"_phase":1}
        ],
        "MasterSkillTarget":[{"_id":1,"_skillTargetType":5,"_gekisouMissionType":1}],
        "MasterSkillCondition":[
            {"_id":1,"_conditionType":7010,"_conditionTargetIDs":[1],"_isPositive":true},
            {"_id":2,"_conditionType":7005,"_conditionValues":[1],"_isPositive":true}
        ],
        "MasterSkillConditionSet":[
            {"_id":1,"_group":1,"_conditionIds":[1]},
            {"_id":2,"_group":2,"_conditionIds":[2]}
        ],
        "MasterSkillCumulativeCondition":[{
            "_id":1,"_skillCumulativeConditionType":7001,"_conditionValues":[1],"_maxCumulativeCount":100
        }],
        "MasterGekisouSkill":[
            {"_id":1,"_gekisouMissionType":1},{"_id":2,"_gekisouMissionType":1},
            {"_id":3,"_gekisouMissionType":1},{"_id":4,"_gekisouMissionType":1},
            {"_id":5,"_gekisouMissionType":1}
        ],
        "MasterGekisouSkillEffect":[
            {"_id":1,"_gekisouSkillID":1,"_level":1,"_skillEffectType":2001,
             "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_activationTimeSecond":9999.0,
             "_effectValue":100,"_maxEffectValue":300,"_skillCumulativeConditionID":1},
            {"_id":2,"_gekisouSkillID":2,"_level":1,"_skillEffectType":12000,
             "_skillTriggerType":2,"_skillTriggerConditionGroup":2,"_effectValue":4},
            {"_id":3,"_gekisouSkillID":3,"_level":1,"_skillEffectType":12004,
             "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_activationTimeSecond":protection_seconds,
             "_effectLimitCount":1}
        ]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    Master::from_json_tables(|name| texts.iter().find(|(n, _)| n == name).map(|(_, text)| text.as_str())).unwrap()
}

fn setup(judgements: &[(i32, i32)], fevers: Vec<(i32, i32)>) -> FullSetup {
    let notes: Vec<_> = judgements
        .iter()
        .enumerate()
        .map(|(id, &(time_ms, _))| LiveNote { note_id: id as i32, time_ms, note_operate_type: 1, judgement_type: 1 })
        .collect();
    let frames = (0..=80)
        .map(|i| {
            let time_ms = i * 20;
            let judged = notes
                .iter()
                .zip(judgements)
                .filter(|(note, _)| note.time_ms == time_ms)
                .map(|(note, &(_, judgement))| JudgedNote {
                    note_id: note.note_id,
                    judgement,
                    judgement_time_ms: note.time_ms,
                })
                .collect();
            PlayFrame { time_ms, judged }
        })
        .collect();
    let nr = fevers.len();
    let mut setup = FullSetup {
        notes,
        events: Vec::new(),
        play: LivePlay { frames, base_seed: 0 },
        params: LiveParams {
            total_power: 10_000,
            music_level: 1,
            converted_note_count: judgements.len() as i32,
            music_length_ms: 2000,
            score_music_length_ms: None,
            skill_target_music_type: 0,
            assist_factor: 1.0,
        },
        gk: None,
    };
    setup.set_gekisou(GekisouSetup { fevers, missions: vec![1; nr] }, vec![0.02; 81], vec![0]);
    setup.gk.as_mut().unwrap().confirmations =
        Some((0..nr).map(|range| RankConfirmation { frame: 0, range, rank: 1, percent: 0 }).collect());
    setup
}

fn entries(setup: &FullSetup) -> Vec<(usize, LiveNote, i32)> {
    setup
        .play
        .frames
        .iter()
        .enumerate()
        .flat_map(|(frame, input)| {
            input
                .judged
                .iter()
                .map(move |j| (frame, *setup.notes.iter().find(|note| note.note_id == j.note_id).unwrap(), j.judgement))
        })
        .collect()
}

fn env<'a>(master: &'a Master, setup: &FullSetup, entries: &[(usize, LiveNote, i32)]) -> Env<'a> {
    let schedule = Schedule::new(master, setup, setup.gk.as_ref().unwrap()).unwrap();
    let frames: Vec<_> = setup.play.frames.iter().map(|f| f.time_ms).collect();
    Env {
        master,
        events: &[],
        sets: master.skill_condition_sets.iter().map(|s| (s.group, vec![s.condition_ids.as_slice()])).collect(),
        life_lo: 1000,
        life_hi: 1000,
        life_rigid: true,
        raw: entries.iter().map(|e| e.2).collect(),
        count_reach: std::array::from_fn(|j| 1u8 << j),
        entry_reach: entries.iter().map(|e| 1u8 << e.2).collect(),
        gk: Some(GkEnv {
            missions: schedule.ranges.iter().map(|r| r.mission).collect(),
            completes: true,
            breaks: true,
        }),
        gkf: Some(Rc::new(GkFrames::new(&schedule, &frames, entries))),
        rush_cache: RefCell::new(HashMap::new()),
        gk_cache: RefCell::new(HashMap::new()),
        budget_cache: RefCell::new(HashMap::new()),
        ramp_cache: RefCell::new(HashMap::new()),
    }
}

fn rows(env: &Env) -> Vec<Vec<Row>> {
    (1..=5)
        .map(|id| gekisou_rows(env, &env.master.gekisou_skill_effects, RowSource::Gekisou, id, 1, 1).unwrap())
        .collect()
}

fn next_order(order: &mut [usize; 5]) -> bool {
    let Some(i) = (0..4).rev().find(|&i| order[i] < order[i + 1]) else { return false };
    let j = (i + 1..5).rev().find(|&j| order[i] < order[j]).unwrap();
    order.swap(i, j);
    order[i + 1..].reverse();
    true
}

#[test]
fn combo_epoch_native_all_120_labels_enclose_breaks_protection_bonus_and_range_switches() {
    let master = master(0.5);
    let setup = setup(
        &[
            (100, 6),
            (120, 6),
            (140, 1),
            (160, 6),
            (180, 6),
            (200, 6),
            (280, 1),
            (300, 6),
            (300, 6),
            (320, 1),
            (340, 6),
            (400, 6),
            (420, 6),
            (500, 6),
            (520, 6),
        ],
        vec![(80, 360), (260, 560)],
    );
    let entries = entries(&setup);
    let mut env = env(&master, &setup, &entries);
    let rows = rows(&env);
    let bonus = combo_triggers::maximum_bonus(&rows, &[]).unwrap();
    let certificate = combo_epochs::compile(&env, &setup, &entries, rows.iter().flatten(), bonus).unwrap();
    assert!(certificate.epochs > 1);
    Rc::get_mut(env.gkf.as_mut().unwrap()).unwrap().combo_epochs = Some(certificate);
    let bound = churn_max(&env, &rows[0][0]).unwrap();
    assert!(bound < setup.play.frames.len() as f64, "the optional work bound must actually be finite and tighter");
    let mut order = [0, 1, 2, 3, 4];
    let mut labels = std::collections::BTreeSet::new();
    let mut saw_break = false;
    let mut saw_bonus = false;
    loop {
        assert!(labels.insert(order));
        let deck = order.map(|slot| Performer { gekisou_skill: Some((slot as i64 + 1, 1)), ..Default::default() });
        let mut native = setup.gekisou_model(&master, &deck, 10_000).unwrap();
        let mut previous = 0.0f32.to_bits();
        let mut changes = 0usize;
        let mut old_combo = [0; 2];
        for (frame, &dt) in setup.play.frames.iter().zip(&setup.gk.as_ref().unwrap().dt) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            let factor = native.factor_state().note_score_up.to_bits();
            changes += usize::from(factor != previous);
            previous = factor;
            for (i, range) in native.gekisou_ranges().iter().enumerate() {
                assert!((0..=certificate.max_combo).contains(&range.combo));
                saw_break |= range.combo < old_combo[i];
                saw_bonus |= range.combo > 5;
                old_combo[i] = range.combo;
            }
        }
        // The same row can start one distinct lifetime at each range start. This
        // observes native factor state, not a reconstruction of the epoch logic.
        assert!(changes > 0 && changes as f64 <= 2.0 * (bound + 2.0));
        #[cfg(feature = "search-diagnostics")]
        {
            // The two controller writers file no score factors; every command,
            // including each zero-valued start, belongs to the cumulative row.
            // Count original filings once after play, preserving both halves
            // of same-frame replacements and every overlapping lifetime.
            let mut positive = 0;
            let mut negative = 0;
            let commands = native
                .filed_factor_commands()
                .inspect(|command| {
                    assert_eq!(
                        (
                            command.combo_mill,
                            command.judgement,
                            command.judge_mill,
                            command.band_total_power,
                            command.luck
                        ),
                        (0, 0, 0, 0, 0)
                    );
                    positive += usize::from(command.note_mill > 0);
                    negative += usize::from(command.note_mill < 0);
                })
                .count();
            assert!(positive > 0 && negative > 0);
            assert!(commands as f64 <= 2.0 * 2.0 * (1.0 + bound));
            assert_eq!(native.filed_factor_commands().count(), commands, "reading diagnostics changes no native state");
        }
        assert_eq!(native.rank_confirmation_applications().len(), 2);
        assert_eq!(native.draws(), 0);
        if !next_order(&mut order) {
            break;
        }
    }
    assert_eq!(labels.len(), 120);
    assert!(saw_bonus && saw_break, "the native oracle must exercise both recount directions");
}

#[test]
fn combo_epoch_native_same_time_inverse_rewrites_old_miss_and_is_refused() {
    let master = master(0.1);
    let setup = setup(&[(100, 6), (120, 6), (180, 1), (240, 6)], vec![(80, 400)]);
    let entries = entries(&setup);
    let mut env = env(&master, &setup, &entries);
    let rows = rows(&env);
    assert!(combo_epochs::compile(&env, &setup, &entries, rows.iter().flatten(), 4).is_none());
    assert!(churn_max(&env, &rows[0][0]).is_none());
    env.gk.as_mut().unwrap().breaks = false;
    assert!(churn_max(&env, &rows[0][0]).is_none(), "the old raw-grade flag cannot authorize an absent history proof");
    let deck = [Performer { gekisou_skill: Some((3, 1)), ..Default::default() }];
    let mut native = setup.gekisou_model(&master, &deck, 10_000).unwrap();
    let mut protected = None;
    let mut revoked = None;
    for (frame, &dt) in setup.play.frames.iter().zip(&setup.gk.as_ref().unwrap().dt) {
        native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        if frame.time_ms == 180 {
            protected = Some(native.gekisou_ranges()[0].combo);
        }
        if frame.time_ms == 200 {
            assert!(frame.judged.is_empty(), "the decrease must come from replaying old history");
            revoked = Some(native.gekisou_ranges()[0].combo);
        }
    }
    assert_eq!(protected, Some(2));
    assert_eq!(revoked, Some(0));
}

#[test]
fn combo_epoch_refuses_incomplete_closure_late_history_clamp_and_integer_overflow() {
    let master = master(0.5);
    let setup = setup(&[(100, 6), (120, 6), (180, 1), (240, 6)], vec![(80, 400)]);
    let entries = entries(&setup);
    let mut env = env(&master, &setup, &entries);
    let rows = rows(&env);
    assert!(combo_epochs::compile(&env, &setup, &entries, rows.iter().flatten(), 4).is_some());
    env.entry_reach.pop();
    assert!(combo_epochs::compile(&env, &setup, &entries, rows.iter().flatten(), 4).is_none());
    env.entry_reach = entries.iter().map(|e| 1u8 << e.2).collect();
    env.entry_reach[0] = 1 << 1; // Closure must retain its original grade as well.
    assert!(combo_epochs::compile(&env, &setup, &entries, rows.iter().flatten(), 4).is_none());
    env.entry_reach[0] = 1 << 6;
    let mut clamped = setup.clone();
    clamped.params.music_length_ms = 180;
    assert!(combo_epochs::compile(&env, &clamped, &entries, rows.iter().flatten(), 4).is_none());
    assert!(combo_epochs::compile(&env, &setup, &entries, rows.iter().flatten(), 1 << 24).is_none());
    let mut late = setup.clone();
    let note = late.play.frames[5].judged.pop().unwrap();
    late.play.frames[10].judged.push(note);
    let late_entries = entries_for_late(&late);
    let late_env = env_for_late(&master, &late, &late_entries);
    assert!(combo_epochs::compile(&late_env, &late, &late_entries, rows.iter().flatten(), 4).is_none());
}

// Avoid shadowing the fixture builders with local evidence named `entries` / `env`.
fn entries_for_late(setup: &FullSetup) -> Vec<(usize, LiveNote, i32)> {
    entries(setup)
}
fn env_for_late<'a>(master: &'a Master, setup: &FullSetup, entries: &[(usize, LiveNote, i32)]) -> Env<'a> {
    env(master, setup, entries)
}

#[test]
fn combo_epoch_churn_keeps_native_product_and_stable_quantization_guards() {
    let mut master = master(0.5);
    let setup = setup(&[(100, 6), (120, 6)], vec![(80, 400)]);
    let entries = entries(&setup);
    let certificate = combo_epochs::ComboEpochs { epochs: 4, max_combo: i32::MAX };
    let mut evidence = env(&master, &setup, &entries);
    let row = rows(&evidence)[0][0].clone();
    assert!(certificate.churn(&evidence, &row, 31.0).is_some());
    drop(evidence);
    master.cumulative_conditions[0].max_cumulative_count = 0;
    evidence = env(&master, &setup, &entries);
    assert!(certificate.churn(&evidence, &row, 31.0).is_none(), "the cap applies after the native i32 cast");
    drop(evidence);
    master.cumulative_conditions[0].condition_values = vec![0];
    evidence = env(&master, &setup, &entries);
    assert!(certificate.churn(&evidence, &row, 31.0).is_none());
    assert!(stable_cumulative_churn(45, 45).is_none(), "a repeated quantized replacement is not stable");
}

#[test]
fn combo_epoch_complete_snap_live_entry_publishes_the_same_certificate() {
    use crate::search::budget::SearchBudget;
    use crate::search::gate_tests::common::{Rng, roster, set_column, synth_snaps};

    let mut mechanics = master(0.5);
    let mut source = synth_snaps(&mut Rng::new(9713), 5, 0, &[1]);
    let mut skill = 0;
    set_column(&mut source, "MasterMemberCard", &mut |row| {
        skill += 1;
        row["_gekisouSkillID"] = json!(skill);
    });
    let mut master = source.master();
    // Formation/card tables still reference the skeleton's targets. Keep all
    // of them and allocate a separate mission target for the native mechanics.
    let mission_target = master.skill_targets.iter().map(|target| target.id).max().unwrap_or(0).checked_add(1).unwrap();
    mechanics.skill_targets[0].id = mission_target;
    for condition in &mut mechanics.skill_conditions {
        for target in &mut condition.condition_target_ids {
            assert_eq!(*target, 1);
            *target = mission_target;
        }
    }
    master.live_skill_effects.clear();
    master.support_skill_effects.clear();
    master.gekisou_support_skill_effects.clear();
    master.gekisou_skills = mechanics.gekisou_skills;
    master.gekisou_skill_effects = mechanics.gekisou_skill_effects;
    master.skill_conditions = mechanics.skill_conditions;
    master.skill_condition_sets = mechanics.skill_condition_sets;
    master.cumulative_conditions = mechanics.cumulative_conditions;
    master.skill_targets.extend(mechanics.skill_targets);
    master.skill_effect_settings = mechanics.skill_effect_settings;
    master.live_settings = mechanics.live_settings;
    master.note_parameters = mechanics.note_parameters;
    master.judgement_parameters = mechanics.judgement_parameters;
    master.live_judgement_timings = mechanics.live_judgement_timings;
    master.reindex().unwrap();
    let setup = setup(&[(100, 6), (120, 6), (140, 1), (160, 6), (180, 1), (240, 6)], vec![(80, 400)]);
    let entries = entries(&setup);
    let evidence = env(&master, &setup, &entries);
    let rows = rows(&evidence);
    let expected = combo_epochs::compile(
        &evidence,
        &setup,
        &entries,
        rows.iter().flatten(),
        combo_triggers::maximum_bonus(&rows, &[]).unwrap(),
    )
    .unwrap();
    let mut owned = roster(&mut Rng::new(9714), &master);
    for member in &mut owned.members {
        member.gekisou_skill_level = 1;
    }
    let pool = Pool::new(&master, &owned).unwrap();
    let tables =
        Tables::new(&pool, None, false, &[], SearchBudget::new(Instant::now(), None).unwrap()).unwrap().unwrap();
    let live = SnapLive::new(&pool, &tables, &[true; 5], &setup).unwrap();
    assert_eq!(live.factor_diagnostics.combo_monotone_epochs, Some(expected.epochs));
    assert_eq!(live.factor_diagnostics.combo_value_upper, Some(expected.max_combo));
    let active = live.classes.iter().flat_map(|classes| &classes[0].rows).find(|row| row.churn).unwrap();
    assert!(active.churn_max.is_some_and(|count| count < setup.play.frames.len() as f64));
    let view = FineView { coef: &live.coef, fine: &live.fine, chain_extra: live.chain_extra };
    let mut order = [0, 1, 2, 3, 4];
    let mut checked = 0;
    loop {
        let parts = std::array::from_fn(|position| &live.contrib[order[position]][0][position]);
        let upper = view.fine_bound(10_000, parts, [0; 5], CandLife::Unknown, &mut Scratch::default(), None);
        let deck: Vec<_> = order.iter().map(|&member| performer(&pool.members[member], None).unwrap()).collect();
        let mut native = setup.gekisou_model(&master, &deck, 10_000).unwrap();
        for (frame, &dt) in setup.play.frames.iter().zip(&setup.gk.as_ref().unwrap().dt) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            assert!(i64::from(native.score()) <= upper);
        }
        assert!(native.score() > 0);
        checked += 1;
        if !next_order(&mut order) {
            break;
        }
    }
    assert_eq!(checked, 120);
}
