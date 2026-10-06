//! Differential recording checks. The reference always executes the original native numeric calculator.
use super::*;

fn armed(input: &RushCase) -> LiveModel {
    let skills = luck_skills(&input.master).unwrap();
    let mut model = input.native();
    let gate = check_recorder(&model, &skills).unwrap();
    let probes = model
        .luck_score_rows(&skills)
        .into_iter()
        .filter(|row| row.may_hold)
        .map(|row| ProbeRow { owner: row.owner, value: row.value })
        .collect();
    model.set_luck_weights(&skills, Vec::new()).unwrap();
    model.score.begin_bounds(probes, true);
    model.score.certify_bounds_filings(gate);
    model
}

fn assert_trace_equal(left: &BoundsTrace, right: &BoundsTrace) {
    assert_eq!((left.frames, left.queries, left.has_luck), (right.frames, right.queries, right.has_luck));
    assert_eq!(left.filing_gate, right.filing_gate);
    assert_eq!(
        left.probes.iter().map(|probe| (probe.owner, probe.value.to_bits())).collect::<Vec<_>>(),
        right.probes.iter().map(|probe| (probe.owner, probe.value.to_bits())).collect::<Vec<_>>()
    );
    assert_eq!(left.events.len(), right.events.len());
    for (ordinal, (left, right)) in left.events.iter().zip(&right.events).enumerate() {
        match (left, right) {
            (
                BoundsEvent::Note { frame: a, index: b, note: c },
                BoundsEvent::Note { frame: x, index: y, note: z },
            ) => {
                assert_eq!((a, b, c.bounds_identity()), (x, y, z.bounds_identity()), "event {ordinal}");
                // Notes are copied at filing, before any execution. Their private added/factor diagnostics
                // are constructor zeros in both traces; include them in the complete equality check.
                assert_eq!(format!("{c:?}"), format!("{z:?}"), "event {ordinal}");
            }
            (
                BoundsEvent::Factor { frame: a, command: b },
                BoundsEvent::Factor { frame: x, command: y },
            ) => assert_eq!((a, b), (x, y), "event {ordinal}"),
            (BoundsEvent::Potential { frame: a }, BoundsEvent::Potential { frame: x }) => {
                assert_eq!(a, x, "event {ordinal}");
            }
            (
                BoundsEvent::Probe { frame: a, time_ms: b },
                BoundsEvent::Probe { frame: x, time_ms: y },
            ) => assert_eq!((a, b), (x, y), "event {ordinal}"),
            (
                BoundsEvent::Query { time_ms: a, to: b },
                BoundsEvent::Query { time_ms: x, to: y },
            ) => assert_eq!((a, b), (x, y), "event {ordinal}"),
            (
                BoundsEvent::Combo { frame: a, index: b, ordinary: c, gekisou: d },
                BoundsEvent::Combo { frame: x, index: y, ordinary: z, gekisou: w },
            ) => assert_eq!((a, b, c.to_bits(), d.to_bits()), (x, y, z.to_bits(), w.to_bits()), "event {ordinal}"),
            (BoundsEvent::ProbabilityReady(a), BoundsEvent::ProbabilityReady(x)) => {
                assert_eq!(a, x, "event {ordinal}");
            }
            (
                BoundsEvent::Rank { range: a, time_ms: b, percent: c, start: d, end: e },
                BoundsEvent::Rank { range: v, time_ms: w, percent: x, start: y, end: z },
            ) => assert_eq!((a, b, c, d, e), (v, w, x, y, z), "event {ordinal}"),
            _ => panic!("event kind changed at {ordinal}: {left:?} != {right:?}"),
        }
    }
    assert_eq!(left.combo.seen, right.combo.seen);
    assert_eq!(left.combo.consistent, right.combo.consistent);
    assert_eq!(left.combo.judgements, right.combo.judgements);
    assert_eq!(left.combo.windows, right.combo.windows);
    assert_eq!(left.combo.current, right.combo.current);
    assert_eq!(left.combo.filed, right.combo.filed);
    assert_eq!(left.combo.stale, right.combo.stale);
}

fn compare_recorders(input: &RushCase) -> BoundsTrace {
    let mut native = armed(input);
    let initial_score = native.score.clone();
    let initial_fields = native.score.calc.state;
    let mut recording = native.clone();
    assert!(recording.try_enable_bounds_record_only());
    for (frame, &delta) in input.play.frames.iter().zip(&input.delta) {
        native.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
        recording.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
    }
    assert!(native.score() > 0, "the reference must execute nonconstant native scoring");
    assert_eq!(recording.score(), 0, "the private structural model must not execute native scoring");
    assert_eq!(recording.score.calc.state, initial_fields);
    assert_eq!(native.draws(), 0);
    assert_eq!(recording.draws(), 0);
    assert_eq!(native.current_life(), recording.current_life());
    let reference = native.score.bounds_trace.take().unwrap();
    let recorded = recording.score.bounds_trace.take().unwrap();
    assert_trace_equal(&reference, &recorded);

    // Compare every remaining model field after removing exactly the deliberately unobserved numeric
    // calculator and controller snapshots/bonus amounts. Query/filing/Combo contents were compared above.
    // This catches a new indirect control reader or changed condition/life/controller cache state.
    for model in [&mut native, &mut recording] {
        model.score = initial_score.clone();
        model.frame_score = 0;
        for (_, value) in &mut model.trace {
            *value = 0;
        }
        if let Some(gk) = &mut model.gk {
            for state in &mut gk.ctrl.states {
                state.start_score = 0;
                state.end_score = 0;
            }
            for (_, _, bonus, _) in &mut gk.rank_bonus {
                *bonus = 0;
            }
        }
    }
    assert_eq!(format!("{native:?}"), format!("{recording:?}"));
    recorded
}

#[test]
fn record_only_matches_complete_short_long_and_late_native_traces() {
    for (gap, members) in [(2400, false), (12_000, true)] {
        let mut input = four_bucket_case(gap, 2, members);
        if !members {
            let mut down = input.master.live_skill_effects[0].clone();
            down.id = 930;
            down.skill_effect_type = 2005;
            down.effect_value = 1200;
            input.master.live_skill_effects.push(down);
            input.events = vec![(0, 90), (0, 300), (0, 390), (0, gap + 90)];
        }
        input.master.reindex().unwrap();
        let trace = compare_recorders(&input);
        assert_eq!(trace.events.iter().filter(|event| matches!(event, BoundsEvent::Rank { .. })).count(), 3);
        assert!(trace.events.iter().any(|event| matches!(event, BoundsEvent::Probe { .. })));
        assert!(trace.events.iter().any(|event| matches!(event, BoundsEvent::Potential { .. })));
    }
}

#[test]
fn record_only_keeps_conversion_life_conditions_and_next_frame_rank_triggers() {
    let mut input = four_bucket_case(2400, 2, false);
    input.master.live_judgement_timings.push(crate::master::LiveJudgementTimingRow {
        id: 6, assist_level: 0, judgement_priority: 0, note_judgement_type: 1,
        note_simulate_judgement: 6, before_ms: 0, after_ms: 0,
    });
    input.master.judgement_parameters.push(crate::master::JudgementParameterRow {
        id: 6, note_simulate_judgement: 6, score_percent: 137, damage: 250,
    });
    input.master.skill_targets.push(crate::master::SkillTargetRow {
        id: 920, skill_target_type: 4, judgement: 5, ..Default::default()
    });
    input.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
        id: 920, live_skill_id: 903, level: 1, skill_effect_type: 12006, effect_value: 6,
        effect_limit_count: 1, skill_target_ids: vec![920], activation_time_second: 0.5,
        ..Default::default()
    });
    for (id, kind, values) in [(930, 2002, vec![900]), (931, 7012, vec![1])] {
        input.master.skill_conditions.push(crate::master::SkillConditionRow {
            id, condition_type: kind, condition_values: values,
            condition_target_ids: Vec::new(), is_positive: true,
        });
        input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id, group: id, condition_ids: vec![id],
        });
    }
    input.master.support_skill_effects.push(crate::master::SupportSkillEffectRow {
        id: 930, support_skill_id: 930, level: 1, skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: 930, skill_effect_type: 3001, effect_value: 125,
        ..Default::default()
    });
    input.master.support_skill_effects.push(crate::master::SupportSkillEffectRow {
        id: 931, support_skill_id: 930, level: 1, skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: 931, skill_effect_type: 2000, effect_value: 7531,
        activation_time_second: 0.45, ..Default::default()
    });
    input.deck[0].support_skills.push((930, 1));
    input.master.reindex().unwrap();
    let trace = compare_recorders(&input);
    let notes: Vec<_> = trace.events.iter().filter_map(|event| {
        if let BoundsEvent::Note { note, .. } = event { Some(note) } else { None }
    }).collect();
    assert!(notes.iter().any(|note| note.score_type == crate::live::score::JUST));
    assert!(notes.iter().any(|note| note.score_type == crate::live::score::PERFECT));
    assert!(notes.iter().any(|note| note.life < 1000), "converted note damage must be retained");
    assert!(trace.events.iter().any(|event| matches!(event,
        BoundsEvent::Factor { command, .. } if command.note_mill == 75310
    )), "the rank-1 confirmation must reach its later score-up trigger");
}

#[test]
fn record_only_keeps_changing_gekisou_combo_observations() {
    let mut input = four_bucket_case(2400, 2, false);
    input.setup.missions[0] = gekisou::M_COMBO;
    for row in &mut input.master.gekisou_ranking_score_bonuses {
        row.mission_pattern = gekisou::mission_pattern(1, 2, 2);
    }
    input.master.combo_score_bonuses = [(0, 1, 0.125), (0, 2, 0.25), (1, 1, 0.25), (1, 2, 0.375)]
        .into_iter()
        .enumerate()
        .map(|(index, (kind, count, bonus))| crate::master::ComboScoreBonusRow {
            id: index as i64 + 1, combo_bonus_type: kind, required_combo_count: count, bonus_factor: bonus,
        })
        .collect();
    input.notes.insert(1, LiveNote { note_id: 99, note_operate_type: 1, judgement_type: 1, time_ms: 280 });
    rebuild_rush_frames(&mut input, |_| 5);
    input.master.reindex().unwrap();
    let trace = compare_recorders(&input);
    let mut seen = std::collections::BTreeMap::new();
    let mut changed = false;
    for event in trace.events {
        if let BoundsEvent::Combo { frame, index, ordinary, gekisou } = event {
            let bits = (ordinary.to_bits(), gekisou.to_bits());
            changed |= seen.insert((frame, index), bits).is_some_and(|previous| previous != bits);
        }
    }
    assert!(changed, "the same note must have distinct historical combo observations");
}

#[test]
fn record_only_preserves_missing_percentage_error_at_its_execution_query() {
    for (note_type, score_type, expected) in [(99, 2, "note type 99"), (1, 99, "score type 99")] {
        let input = four_bucket_case(2400, 2, false);
        let mut native = armed(&input);
        let mut recording = native.clone();
        assert!(recording.try_enable_bounds_record_only());
        for model in [&mut native, &mut recording] {
            // A future note may be filed early. It must not fail at an earlier query.
            model.score.add_note(NoteCommand::new(400, 1000, 10, note_type, score_type));
            model.score.calculate(40, &model.combo, None).unwrap();
            // Missing percentage validation precedes duplicate pending-fixed errors at the execution query.
            model.score.add_fixed(80, 0);
            model.score.calculate(80, &model.combo, None).unwrap();
            model.score.add_fixed(80, 0);
        }
        let a = native.score.calculate(400, &native.combo, None).unwrap_err().to_string();
        let b = recording.score.calculate(400, &recording.combo, None).unwrap_err().to_string();
        assert_eq!(a, b);
        assert!(a.contains(expected));
        assert_trace_equal(native.score.bounds_trace.as_ref().unwrap(), recording.score.bounds_trace.as_ref().unwrap());
    }
}

#[test]
fn record_only_keeps_pending_overwrite_raw_frame_identity_and_duplicate_errors() {
    let input = four_bucket_case(2400, 2, false);
    let mut native = armed(&input);
    let mut recording = native.clone();
    assert!(recording.try_enable_bounds_record_only());
    for model in [&mut native, &mut recording] {
        model.score.add_fixed(40, 0);
        model.score.add_fixed(80, 0);
        model.score.calculate(0, &model.combo, None).unwrap();
        // The overwritten frame was never filed.
        model.score.add_fixed(40, 0);
        model.score.calculate(120, &model.combo, None).unwrap();
        // Fixed frames use raw get_frame, not the score array's clamped terminal frame.
        model.score.add_fixed(1_000_000, 0);
        model.score.calculate(1_000_000, &model.combo, None).unwrap();
        model.score.add_fixed(1_000_040, 0);
        model.score.calculate(1_000_040, &model.combo, None).unwrap();
        model.score.add_fixed(1_000_000, 0);
    }
    let a = native.score.calculate(1_000_040, &native.combo, None).unwrap_err().to_string();
    let b = recording.score.calculate(1_000_040, &recording.combo, None).unwrap_err().to_string();
    assert_eq!(a, b);
    assert!(a.contains("two fixed scores in one frame"));
    assert_trace_equal(native.score.bounds_trace.as_ref().unwrap(), recording.score.bounds_trace.as_ref().unwrap());
}

#[test]
fn record_only_refuses_numeric_observers_unadmitted_and_nonfresh_models() {
    let input = four_bucket_case(2400, 2, false);
    let fresh = armed(&input);
    let mut model = fresh.clone();
    model.track_score_up_factors();
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.begin_score_program_recording().unwrap();
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.score.bounds_trace.as_mut().unwrap().filing_gate = None;
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.score.bounds_trace.as_mut().unwrap().has_luck = false;
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.score.calc.combo_table = Some(Default::default());
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.score.calc.assist_factor = f32::INFINITY;
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.set_previous_gekisou_rank_confirmation(Some(1));
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.phase_life = Some([0; 2]);
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.rush_effect_log = Some(Vec::new());
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.raw_pending = Some(Vec::new());
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.frame_timed(0, &[], 0.1).unwrap();
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh.clone();
    model.score.add_note(NoteCommand::new(40, 1000, 1, 1, crate::live::score::PERFECT));
    model.score.calculate(80, &model.combo, None).unwrap();
    model.score.settle(2);
    assert!(!model.try_enable_bounds_record_only());
    let mut model = fresh;
    assert!(model.try_enable_bounds_record_only());
    model.track_score_up_factors();
    assert!(model.score.calculate(0, &model.combo, None).is_err());
}

#[test]
fn record_only_refuses_free_no_luck_and_external_ranking_paths() {
    let input = four_bucket_case(2400, 2, false);
    let mut free = LiveModel::new(&input.master, &[], &input.notes, &[], input.params).unwrap();
    assert!(!free.try_enable_bounds_record_only());
    let mut no_luck = input.clone();
    no_luck.setup.missions = vec![1, 1, 1];
    let mut model = armed(&no_luck);
    assert!(!model.try_enable_bounds_record_only());
    for (frame, &delta) in no_luck.play.frames.iter().zip(&no_luck.delta) {
        model.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
    }
    assert!(model.score() > 0);
    let skills = luck_skills(&input.master).unwrap();
    let mut external = LiveModel::new_gekisou_external(
        &input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup,
    ).unwrap();
    let gate = check_recorder(&external, &skills).unwrap();
    external.set_luck_weights(&skills, Vec::new()).unwrap();
    external.score.begin_bounds(Vec::new(), true);
    external.score.certify_bounds_filings(gate);
    assert!(!external.try_enable_bounds_record_only());
}


#[cfg(feature = "search-diagnostics")]
#[test]
fn record_only_diagnostics_count_entered_active_and_refused_queries() {
    take_luck_score_profile();
    let input = four_bucket_case(2400, 2, false);
    let mut recording = armed(&input);
    assert!(recording.try_enable_bounds_record_only());
    let unused = take_luck_score_profile();
    assert_eq!(unused.recorder_trace_only_runs, 0);
    assert_eq!(unused.recorder_trace_only_queries, 0);

    recording.score.calculate(40, &recording.combo, None).unwrap();
    recording.score.calculate(40, &recording.combo, None).unwrap();
    recording.score.add_note(NoteCommand::new(400, 1000, 10, 99, 2));
    let error = recording.score.calculate(400, &recording.combo, None).unwrap_err();
    assert!(error.to_string().contains("note type 99"));
    let work = take_luck_score_profile();
    assert_eq!(work.recorder_trace_only_runs, 1);
    assert_eq!(work.recorder_trace_only_queries, 3);
    assert_eq!(work.recorder_trace_only_active_queries, 2);
    assert_eq!(work.evaluations, 0);

    // Ordinary native calculation is not a structural run, even with a bounds trace attached.
    let mut native = armed(&input);
    native.score.calculate(40, &native.combo, None).unwrap();
    let work = take_luck_score_profile();
    assert_eq!(work.recorder_trace_only_runs, 0);
    assert_eq!(work.recorder_trace_only_queries, 0);
    assert_eq!(work.recorder_trace_only_active_queries, 0);
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn record_only_diagnostics_keep_partial_work_after_cancellation() {
    for upper_only in [false, true] {
        take_luck_score_profile();
        let input = four_bucket_case(2400, 2, false);
        let mut observed = [0u64; 3];
        // Stop at the first cancellation poll after actual structural work. Polling may happen many
        // times in the earlier probability phase, so no assumed callback count is used.
        let mut stop = || {
            let partial = take_luck_score_profile();
            assert_eq!(partial.evaluations, 0);
            observed[0] += partial.recorder_trace_only_runs;
            observed[1] += partial.recorder_trace_only_queries;
            observed[2] += partial.recorder_trace_only_active_queries;
            observed[1] > 0
        };
        if upper_only {
            assert!(matches!(input.prepare(None, &mut stop), LuckRushPreparation::Stopped));
        } else {
            let skills = luck_skills(&input.master).unwrap();
            let mut session = LuckScoreSession::new(
                &input.master, &skills, &input.notes, &input.events, input.params,
                &input.setup, &input.play, &input.delta, None,
            );
            assert!(session.summary(&input.deck, None, &mut stop).unwrap().is_none());
        }
        let final_work = take_luck_score_profile();
        observed[0] += final_work.recorder_trace_only_runs;
        observed[1] += final_work.recorder_trace_only_queries;
        observed[2] += final_work.recorder_trace_only_active_queries;
        assert_eq!(observed[0], 1);
        assert!(observed[1] > 0 && observed[2] > 0 && observed[2] <= observed[1]);
        assert_eq!(final_work.evaluations, 0);
        assert!(final_work.recorder_run_ms > 0.0, "partial recorder time must survive cancellation");
    }
}
