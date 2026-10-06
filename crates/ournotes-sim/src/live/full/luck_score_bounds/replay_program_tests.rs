use super::*;

mod filing_tests {
    include!("filing_tests.rs");
}

#[derive(Clone)]
struct RecordedCase {
    initialized: String,
    trace: BoundsTrace,
    calc: LiveScoreCalculator,
    rush: i32,
    query_limit: u64,
    life: i32,
}

impl RecordedCase {
    fn key(&self) -> program::RecordedIdentity {
        program::recorded_identity(
            &self.trace,
            &self.calc,
            self.rush,
            self.query_limit,
            self.life,
            8 << 20,
            &mut || false,
        )
        .unwrap()
    }
}

fn recorded(input: &ProgramCase) -> RecordedCase {
    let skills = luck_skills(&input.master).unwrap();
    let mut model = if input.ranking.is_some() {
        LiveModel::new_gekisou_external(
            &input.master,
            &input.deck,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
        )
    } else {
        LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup)
    }
    .unwrap();
    if let Some(ranking) = &input.ranking {
        model.set_rank_confirmation_timeline(ranking).unwrap();
    }
    let _ = check_recorder(&model, &skills).unwrap();
    let power = std::mem::replace(&mut model.score.calc.state.band_total_power, 0);
    let initialized = crate::live::full::luck_exact::initialized_identity(&mut model).unwrap();
    model.score.calc.state.band_total_power = power;
    let calc = model.score.calc.clone();
    let probes = model
        .luck_score_rows(&skills)
        .into_iter()
        .filter(|row| row.may_hold)
        .map(|row| ProbeRow { owner: row.owner, value: row.value })
        .collect();
    model.set_luck_weights(&skills, Vec::new()).unwrap();
    model.score.begin_bounds(probes, true);
    model.random.set_seed(input.play.base_seed);
    for (frame, &delta) in input.play.frames.iter().zip(&input.delta) {
        model.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
    }
    assert_eq!(model.random.draws(), 0);
    assert!(model.gk.as_ref().unwrap().ctrl.states.iter().all(|state| state.state == gekisou::S_FINISH));
    let trace = model.score.bounds_trace.take().unwrap();
    let query_limit = 2 * input.play.frames.len() as u64
        + if input.ranking.is_none() { 2 * input.setup.fevers.len() as u64 } else { 0 };
    assert!(trace.queries as u64 <= query_limit);
    RecordedCase {
        initialized,
        trace,
        calc,
        rush: setting(&input.master, "gekisou_luck_rush_score_bonus_percent").unwrap() as i32,
        query_limit,
        life: model.current_life(),
    }
}

fn with_dormant_skills(mut input: ProgramCase) -> ProgramCase {
    for (id, value, time) in [(901, 12999, 0.17), (902, 1700, 0.2), (903, 3700, 0.7)] {
        input.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
            id,
            live_skill_id: id,
            level: 1,
            skill_effect_type: 2000,
            effect_value: value,
            activation_time_second: time,
            ..Default::default()
        });
    }
    input.master.reindex().unwrap();
    input.events = vec![(0, 80)];
    input.deck = (0..5).map(|owner| Performer { character_id: owner + 7, ..Default::default() }).collect();
    input.deck[0].live_skill = Some((901, 1));
    input.deck[1].live_skill = Some((902, 1));
    input
}

#[test]
fn distinct_initialized_models_share_only_equal_completed_recordings() {
    for fine in [false, true] {
        let base = with_dormant_skills(ProgramCase::new(lottery_wide_ranges(fine)));
        let base_recording = recorded(&base);
        let base_key = base_recording.key();
        let mut initialized = std::collections::BTreeSet::new();
        let mut cache = LuckDpCache::new(8 << 20);
        for (owner, skill, power) in [
            (1, 902, 1000),
            (1, 903, 999),
            (2, 902, 1001),
            (2, 903, 16_777_215),
            (3, 902, 16_777_217),
            (3, 903, 0),
            (4, 902, 1),
            (4, 903, 1000),
        ] {
            let mut input = base.clone();
            input.params.total_power = power;
            input.deck[1].live_skill = None;
            input.deck[owner].live_skill = Some((skill, 1));
            let recording = recorded(&input);
            assert!(initialized.insert(recording.initialized.clone()));
            assert!(base_key.same(&recording.key()), "only dormant owner/skill and initial power changed");
            let before = cache.stats();
            assert_program_summary(&input.cached(&mut cache), &input.direct());
            let after = cache.stats();
            assert_eq!(after.program_hits, before.program_hits, "initialized-model cache must miss");
            assert_eq!(
                after.program_recorded_hits - before.program_recorded_hits,
                u64::from(before.program_compilations != 0),
                "fine={fine}, owner={owner}, skill={skill}: {after:?}",
            );
        }
        assert_eq!(cache.stats().program_compilations, 1);
        assert_eq!(cache.stats().program_recorded_hits, 7);
    }
}

#[test]
fn completed_recordings_keep_active_timing_life_rank_and_calculator_inputs() {
    let base = with_dormant_skills(ProgramCase::new(fixture()));
    let base_key = recorded(&base).key();
    let mut cache = LuckDpCache::new(8 << 20);
    assert_program_summary(&base.cached(&mut cache), &base.direct());
    for change in 0..7 {
        let mut input = base.clone();
        match change {
            0 => input.events[0].1 = 180,
            1 => {
                input.events[0].0 = 2;
                input.deck[2].live_skill = input.deck[0].live_skill.take();
            }
            2 => input.events.push((1, 80)),
            3 => {
                input.master.live_settings.iter_mut().find(|row| row.key == "life_base").unwrap().value = "999".into();
            }
            4 => {
                input.ranking =
                    Some(vec![crate::replay::RankConfirmation { frame: 8, range: 0, rank: 2, percent: 23 }]);
            }
            5 => input.params.assist_factor = 0.75,
            6 => input.master.note_parameters[0].score_percent += 1,
            _ => unreachable!(),
        }
        input.master.reindex().unwrap();
        assert!(!base_key.same(&recorded(&input).key()), "changed replay input {change}");
        let before = cache.stats().program_recorded_hits;
        assert_program_summary(&input.cached(&mut cache), &input.direct());
        assert_eq!(cache.stats().program_recorded_hits, before, "changed replay input {change} reused a program");
        // This new context also admits reuse, but only after its own complete recording was installed.
        input.params.total_power += 1;
        input.deck[4].live_skill = Some((903, 1));
        let before = cache.stats().program_recorded_hits;
        assert_program_summary(&input.cached(&mut cache), &input.direct());
        assert_eq!(cache.stats().program_recorded_hits, before + 1, "new context {change} did not reuse its recording");
    }
}

#[test]
fn equal_recordings_still_require_the_same_certified_probability_curve() {
    let base = with_dormant_skills(ProgramCase::new(fixture()));
    let mut input = base.clone();
    input.master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            [0, 3].map(move |result| crate::master::LuckBonusLotRow {
                id: kind * 4 + result + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: 1,
            })
        })
        .collect();
    input.master.reindex().unwrap();
    input.deck[1].live_skill = Some((903, 1));
    assert!(recorded(&base).key().same(&recorded(&input).key()));
    let mut cache = LuckDpCache::new(8 << 20);
    assert_program_summary(&base.cached(&mut cache), &base.direct());
    let before = cache.stats().program_recorded_hits;
    assert_program_summary(&input.cached(&mut cache), &input.direct());
    assert_eq!(cache.stats().program_recorded_hits, before, "different lottery law reused a recorded program");
    input.params.total_power += 1;
    input.deck[4].live_skill = Some((902, 1));
    assert_program_summary(&input.cached(&mut cache), &input.direct());
    assert_eq!(cache.stats().program_recorded_hits, before + 1);
}

#[test]
fn recorded_programs_preserve_conditional_snap_ownership() {
    let mut base = with_dormant_skills(ProgramCase::new(fixture()));
    base.master
        .skill_targets
        .push(serde_json::from_value(json!({"_id":901,"_skillTargetType":1,"_characterID":7})).unwrap());
    for (id, kind, targets) in [(901, 5000, vec![901]), (902, 4010, Vec::new())] {
        base.master.skill_conditions.push(
            serde_json::from_value(json!({"_id":id,"_conditionType":kind,"_conditionValues":[],
                "_conditionTargetIDs":targets,"_isPositive":true}))
            .unwrap(),
        );
        base.master
            .skill_condition_sets
            .push(serde_json::from_value(json!({"_id":id,"_group":id,"_conditionIds":[id]})).unwrap());
    }
    base.master
        .skill_effect_settings
        .push(serde_json::from_value(json!({"_id":901,"_skillEffectType":2000,"_phase":2})).unwrap());
    base.master.support_skill_effects.push(
        serde_json::from_value(json!({"_id":901,"_supportSkillID":901,"_level":1,
            "_skillTriggerType":1,"_skillTriggerConditionGroup":902,"_skillConditionGroup":901,
            "_skillEffectType":2000,"_effectValue":5000,"_activationTimeSecond":0.4}))
        .unwrap(),
    );
    base.master.reindex().unwrap();
    base.events.push((2, 80));
    let mut cache = LuckDpCache::new(8 << 20);
    let mut keys = Vec::new();
    let mut means = Vec::new();
    for owner in [0, 2] {
        let mut input = base.clone();
        input.deck[owner].support_skills.push((901, 1));
        let initial = recorded(&input);
        let key = initial.key();
        assert!(keys.iter().all(|old: &program::RecordedIdentity| !old.same(&key)));
        keys.push(key);
        let before = cache.stats().program_recorded_hits;
        let value = input.cached(&mut cache);
        assert_program_summary(&value, &input.direct());
        assert_eq!(cache.stats().program_recorded_hits, before);
        means.push(value.final_mean);
        input.params.total_power += 1;
        input.deck[1].live_skill = Some((903, 1));
        let equivalent = recorded(&input);
        assert_ne!(initial.initialized, equivalent.initialized);
        assert!(keys.last().unwrap().same(&equivalent.key()));
        assert_program_summary(&input.cached(&mut cache), &input.direct());
        assert_eq!(cache.stats().program_recorded_hits, before + 1);
    }
    assert!(means[0].lower > means[1].upper, "a satisfied paired-member condition must raise the score");
}

#[test]
fn completed_recording_reuse_keeps_joint_score_probe_and_rush_buckets() {
    let mut input = with_dormant_skills(ProgramCase::new(fixture()));
    input.master.gekisou_skills.push(crate::master::SkillRow {
        id: 901,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 902,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 7021,
        condition_type: 7021,
        is_positive: true,
        condition_values: Vec::new(),
        condition_target_ids: Vec::new(),
    });
    input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 7021,
        group: 7021,
        condition_ids: vec![7021],
    });
    input.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 902,
        skill_id: 902,
        level: 1,
        skill_effect_type: 2000,
        effect_value: 10000,
        skill_trigger_type: SUSTAINED,
        skill_trigger_condition_group: 7021,
        ..Default::default()
    });
    input.master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            (0..4).map(move |result| crate::master::LuckBonusLotRow {
                id: kind * 4 + result + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: result + 1,
            })
        })
        .collect();
    input.master.reindex().unwrap();
    input.deck[0].gekisou_skill = Some((901, 1));
    input.deck[0].gekisou_support_skills.push((902, 1));
    let recording = recorded(&input);
    assert_eq!(recording.trace.probes.len(), 1);
    assert!(recording.trace.events.iter().any(|event| matches!(event, BoundsEvent::Probe { .. })));
    let mut cache = LuckDpCache::new(8 << 20);
    assert_program_summary(&input.cached(&mut cache), &input.direct());
    let first = cache.stats();
    for (owner, skill, power) in [(2, 902, 999), (3, 903, 1001), (4, 902, 16_777_217)] {
        let mut equivalent = input.clone();
        equivalent.params.total_power = power;
        equivalent.deck[1].live_skill = None;
        equivalent.deck[owner].live_skill = Some((skill, 1));
        let new_recording = recorded(&equivalent);
        assert_ne!(recording.initialized, new_recording.initialized);
        assert!(recording.key().same(&new_recording.key()));
        assert_program_summary(&equivalent.cached(&mut cache), &equivalent.direct());
    }
    let last = cache.stats();
    assert_eq!(last.program_hits, first.program_hits);
    assert_eq!(last.program_compilations, first.program_compilations);
    assert_eq!(last.program_recorded_hits - first.program_recorded_hits, 3);
}

#[test]
fn recorded_reuse_keeps_cancellation_and_recorder_admission() {
    let base = with_dormant_skills(ProgramCase::new(fixture()));
    let mut input = base.clone();
    input.params.total_power = 1001;
    input.deck[1].live_skill = Some((903, 1));
    let mut cache = LuckDpCache::new(8 << 20);
    base.cached(&mut cache);
    let before = cache.stats();
    assert!(input.run(Some(&mut cache), || true).unwrap().is_none());
    assert_eq!(cache.stats().program_recorded_hits, before.program_recorded_hits);
    let mut checks = 0usize;
    let completed = input
        .run(Some(&mut cache), || {
            checks += 1;
            false
        })
        .unwrap()
        .unwrap();
    assert_program_summary(&completed, &input.direct());
    assert_eq!(cache.stats().program_recorded_hits, before.program_recorded_hits + 1);
    assert!(checks > 1);
    let mut interrupted = 0usize;
    assert!(
        input
            .run(Some(&mut cache), || {
                interrupted += 1;
                interrupted >= checks - 1
            })
            .unwrap()
            .is_none()
    );
    assert_program_summary(&input.cached(&mut cache), &completed);
    assert_eq!(cache.stats().program_compilations, 1);

    input.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
        id: 904,
        live_skill_id: 904,
        level: 1,
        skill_effect_type: 3000,
        effect_value: 100,
        ..Default::default()
    });
    input.master.reindex().unwrap();
    input.deck[4].live_skill = Some((904, 1));
    let before = cache.stats().program_recorded_lookups;
    assert!(matches!(input.run(Some(&mut cache), || false), Err(Error::Unsupported(_))));
    assert_eq!(cache.stats().program_recorded_lookups, before);

    for capacity in [0, 8] {
        let mut bounded = LuckDpCache::new(capacity);
        assert_program_summary(&base.cached(&mut bounded), &base.direct());
        let stats = bounded.stats();
        assert_eq!(stats.program_hits + stats.program_recorded_hits, 0);
        assert_eq!(stats.program_peak_entries, 0);
        if capacity == 0 {
            assert_eq!(stats.program_recorded_lookups, 0);
            assert_eq!(stats.program_recorded_key_declines, 0);
        } else {
            assert!(stats.program_recorded_key_declines > 0);
        }
    }
}

fn all_event_kinds() -> RecordedCase {
    let mut case = recorded(&ProgramCase::new(fixture()));
    case.calc.note_factor_percent.extend([(7, 91), (8, 103)]);
    case.calc.judgement_score_factor_percent.extend([(7, 73), (8, 81)]);
    case.trace.probes = vec![ProbeRow { owner: 3, value: 0.3 }];
    case.trace.events = vec![
        BoundsEvent::Note { frame: 2, index: 1, note: NoteCommand::new(80, 900, 4, 1, 2) },
        BoundsEvent::Factor {
            frame: 2,
            command: FactorCommand {
                time_ms: 79,
                owner_id: 3,
                note_mill: 100,
                combo_mill: 200,
                judgement: 5,
                judge_mill: 300,
                band_total_power: 0,
                luck: 0,
            },
        },
        BoundsEvent::Potential { frame: 3 },
        BoundsEvent::Probe { frame: 3, time_ms: 120 },
        BoundsEvent::Combo { frame: 2, index: 1, ordinary: 1.0, gekisou: 1.25 },
        BoundsEvent::ProbabilityReady(120),
        BoundsEvent::Rank { range: 1, time_ms: 120, percent: 23, start: Some(0), end: Some(1) },
        BoundsEvent::Query { time_ms: 120, to: 3 },
    ];
    case
}

#[test]
fn recorded_identity_preserves_every_event_field_and_its_filing_order() {
    let base = all_event_kinds();
    let key = base.key();
    for change in 0..38 {
        let mut input = base.clone();
        match change {
            0 => input.trace.frames += 1,
            1 => input.trace.queries += 1,
            2 => input.trace.has_luck = false,
            3 => input.trace.probes[0].owner += 1,
            4 => input.trace.probes[0].value = f32::from_bits(input.trace.probes[0].value.to_bits() + 1),
            5 => input.trace.probes.push(ProbeRow { owner: 4, value: 0.3 }),
            6..=12 => {
                let BoundsEvent::Note { frame, index, note } = &mut input.trace.events[0] else { unreachable!() };
                match change {
                    6 => *frame += 1,
                    7 => *index += 1,
                    8 => note.time_ms += 1,
                    9 => note.note_id += 1,
                    10 => note.note_type += 1,
                    11 => note.score_type += 1,
                    12 => note.life += 1,
                    _ => unreachable!(),
                }
            }
            13..=21 => {
                let BoundsEvent::Factor { frame, command } = &mut input.trace.events[1] else { unreachable!() };
                match change {
                    13 => *frame += 1,
                    14 => command.owner_id += 1,
                    15 => command.time_ms += 1,
                    16 => command.note_mill += 1,
                    17 => command.combo_mill += 1,
                    18 => command.judgement += 1,
                    19 => command.judge_mill += 1,
                    20 => command.band_total_power += 1,
                    21 => command.luck += 1,
                    _ => unreachable!(),
                }
            }
            22 => input.trace.events[2] = BoundsEvent::Potential { frame: 4 },
            23 => input.trace.events[3] = BoundsEvent::Probe { frame: 4, time_ms: 120 },
            24 => input.trace.events[3] = BoundsEvent::Probe { frame: 3, time_ms: 121 },
            25 => input.trace.events[7] = BoundsEvent::Query { time_ms: 121, to: 3 },
            26 => input.trace.events[7] = BoundsEvent::Query { time_ms: 120, to: 4 },
            27..=30 => {
                let BoundsEvent::Combo { frame, index, ordinary, gekisou } = &mut input.trace.events[4] else {
                    unreachable!()
                };
                match change {
                    27 => *frame += 1,
                    28 => *index += 1,
                    29 => *ordinary = f32::from_bits(ordinary.to_bits() + 1),
                    30 => *gekisou = f32::from_bits(gekisou.to_bits() + 1),
                    _ => unreachable!(),
                }
            }
            31 => input.trace.events[5] = BoundsEvent::ProbabilityReady(121),
            32..=36 => {
                let BoundsEvent::Rank { range, time_ms, percent, start, end } = &mut input.trace.events[6] else {
                    unreachable!()
                };
                match change {
                    32 => *range += 1,
                    33 => *time_ms += 1,
                    34 => *percent += 1,
                    35 => *start = None,
                    36 => *end = None,
                    _ => unreachable!(),
                }
            }
            37 => input.trace.events.swap(1, 2),
            _ => unreachable!(),
        }
        assert!(!key.same(&input.key()), "different event field/order {change} shared a key");
    }
    // Inserting factors at the same dense-stream position must also preserve their own order.
    let mut ordered = base.clone();
    let mut factor = ordered.trace.events[1].clone();
    if let BoundsEvent::Factor { command, .. } = &mut factor {
        command.owner_id += 1;
    }
    ordered.trace.events.insert(2, factor);
    let first = ordered.key();
    ordered.trace.events.swap(1, 2);
    assert!(!first.same(&ordered.key()));
}

#[test]
fn recorded_identity_keeps_calculator_bits_metadata_and_bounded_key_work() {
    let base = all_event_kinds();
    let key = base.key();
    assert!(key.encoded_len() > 0);
    for change in 0..21 {
        let mut input = base.clone();
        match change {
            0 => input.calc.score_adjustment_factor = f32::from_bits(input.calc.score_adjustment_factor.to_bits() + 1),
            1 => input.calc.music_difficulty_factor = f32::from_bits(input.calc.music_difficulty_factor.to_bits() + 1),
            2 => input.calc.converted_note_count += 1,
            3 => input.calc.life_onus_factor = f32::from_bits(input.calc.life_onus_factor.to_bits() + 1),
            4 => input.calc.event_bonus_factor = f32::from_bits(input.calc.event_bonus_factor.to_bits() + 1),
            5 => input.calc.assist_factor = f32::from_bits(input.calc.assist_factor.to_bits() + 1),
            6 => {
                input.calc.note_factor_percent.insert(9, 1);
            }
            7 => {
                input.calc.judgement_score_factor_percent.insert(9, 1);
            }
            8 => input.calc.state.combo_score_up = -0.0,
            9 => input.calc.state.note_score_up += 1.0,
            10 => input.calc.state.just = 0.3,
            11 => input.calc.state.perfect = 0.3,
            12 => input.calc.state.great = 0.3,
            13 => input.calc.state.good = 0.3,
            14 => input.calc.state.added_luck_bonus += 1,
            15 => input.calc.state.gekisou_rank_bonus_score += 1,
            16 => input.calc.combo_table = Some(crate::live::score::ComboTable::default()),
            17 => {
                input.calc.luck_weight = Some(std::sync::Arc::new(crate::live::score::LuckWeights {
                    values: vec![0.5],
                    steps: vec![(0, vec![0.5])],
                }));
            }
            18 => input.rush += 1,
            19 => input.query_limit += 1,
            20 => input.life -= 1,
            _ => unreachable!(),
        }
        assert!(!key.same(&input.key()), "different calculator or result field {change} shared a key");
    }
    let mut equivalent = base.clone();
    equivalent.calc.state.band_total_power += 1;
    let entries: Vec<_> = equivalent.calc.note_factor_percent.drain().collect();
    equivalent.calc.note_factor_percent.extend(entries.into_iter().rev());
    let entries: Vec<_> = equivalent.calc.judgement_score_factor_percent.drain().collect();
    equivalent.calc.judgement_score_factor_percent.extend(entries.into_iter().rev());
    assert!(key.same(&equivalent.key()), "power is parameterized and map allocation order is irrelevant");
    for (capacity, cancel) in [(1, false), (8 << 20, true)] {
        let result = program::recorded_identity(
            &base.trace,
            &base.calc,
            base.rush,
            base.query_limit,
            base.life,
            capacity,
            &mut || cancel,
        );
        match result {
            Err(program::RecordedKeyError::TooLarge) if !cancel => {}
            Err(program::RecordedKeyError::Cancelled) if cancel => {}
            _ => panic!("recorded identity did not stop at its declared bound"),
        }
    }
}

#[test]
fn compacted_note_factors_keep_native_rounding_for_every_joint_bucket() {
    struct RunningCombo;
    impl crate::live::score::GekisouComboInfo for RunningCombo {
        fn gekisou_combo(&self, _: i32) -> Option<i32> {
            Some(7)
        }
        fn combo_windows(&self, out: &mut Vec<(i32, i32, u64)>) {
            out.push((0, 1000, 0));
        }
    }
    let mut calc = recorded(&ProgramCase::new(fixture())).calc;
    calc.note_factor_percent.insert(1, 137);
    calc.judgement_score_factor_percent.extend([(1, 100), (2, 83), (3, 51), (4, 29), (5, 0), (6, 0)]);
    calc.assist_factor = 0.77;
    calc.event_bonus_factor = 1.013;
    calc.combo_table = Some(
        crate::live::score::ComboTable::build([
            (crate::live::score::COMBO as i64, 0, 0.29999998),
            (crate::live::score::GEKISOU_COMBO as i64, 0, 0.69999999),
        ])
        .unwrap(),
    );
    let ordinary = calc.combo_table.as_ref().unwrap().get_cumulative_factor(crate::live::score::COMBO, 7).unwrap();
    let ordinary = F32Interval::point(ordinary.min(1.0) + 1.0).unwrap();
    let gekisou = F32Interval::point(calc.gekisou_combo_bonus_factor(Some(&RunningCombo), 100).unwrap()).unwrap();
    for bonus in [0.0, f32::from_bits(1), 0.12999998, 1.9999999] {
        let raw = [
            [bonus, 1.4999999, 0.30000004, 0.11000001, 0.07000001, 0.030000001],
            [bonus + 0.07, 1.7099999, 0.40000004, 0.17000002, 0.09000001, 0.050000004],
        ];
        let executed = raw.map(|fields| Some(fields.map(|value| F32Interval::point(value).unwrap())));
        for score_type in 1..=6 {
            let factors = note_factors(score_type, executed, ordinary, gekisou).unwrap();
            for power in [0, 1, 999, 1000, 16_777_215, 16_777_217] {
                calc.state.band_total_power = power;
                for life in [0, 900] {
                    let note = NoteCommand::new(100, life, 1, 1, score_type);
                    let (bounds, _) =
                        note_bounds_with_factors(&calc, power, &note, 47, |class| Ok(factors[class])).unwrap();
                    for (bucket, expected) in bounds.buckets.into_iter().enumerate() {
                        let fields = raw[bucket & 1];
                        calc.state.combo_score_up = fields[0];
                        calc.state.note_score_up = fields[1];
                        calc.state.just = fields[2];
                        calc.state.perfect = fields[3];
                        calc.state.great = fields[4];
                        calc.state.good = fields[5];
                        calc.state.added_luck_bonus = if bucket & 2 == 0 { 0 } else { 47 };
                        let native = calc.note_score(7, life, 100, 1, score_type, Some(&RunningCombo)).unwrap();
                        let expected = expected.unwrap();
                        assert_eq!((expected.lower, expected.upper), (native, native));
                    }
                }
            }
        }
    }
}
