//! Independent native score-law oracle for a complete recorder/transcript equality capability.
use super::*;
use crate::live::full::{
    LuckExactBudget, LuckScoreEquivalenceAttempt, LuckScoreEquivalenceDecline, certify_uniform_score_equivalence,
};

#[path = "terminal_payoff_tests.rs"]
mod terminal_payoff_tests;

fn fixture_pair() -> (FamilyFixture, [Performer; 5], [Performer; 5]) {
    let mut fixture = FamilyFixture::new();
    // Match the difficult structure: active GK score probes share owners with live score producers.
    fixture.input.deck[1].live_skill = Some((9402, 1));
    fixture.input.deck[2].gekisou_skill = Some((FAMILY_SCORE, 1));
    fixture.input.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":9880,"_supportSkillID":9880,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":FAMILY_LIVE,"_skillEffectType":3001,"_effectValue":100
        }))
        .unwrap(),
    );
    // Use a supported formation predicate that is exactly false for every physical member/order.
    assert!(fixture.input.deck.iter().all(|member| member.band_id != 9876));
    fixture.input.master.skill_targets.push(crate::master::SkillTargetRow {
        id: 9881,
        skill_target_type: 3,
        band_id: 9876,
        ..Default::default()
    });
    fixture.input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 9881,
        condition_type: 5000,
        condition_values: Vec::new(),
        condition_target_ids: vec![9881],
        is_positive: true,
    });
    fixture.input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 9881,
        group: 9881,
        condition_ids: vec![9881],
    });
    fixture.input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 9882,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    fixture.input.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 9882,
        skill_id: 9882,
        level: 1,
        skill_trigger_type: SUSTAINED,
        skill_trigger_condition_group: FAMILY_PROBE,
        skill_condition_group: 9881,
        skill_effect_type: 2000,
        effect_value: 1537,
        ..Default::default()
    });
    fixture.input.master.reindex().unwrap();
    let mut left: [Performer; 5] = fixture.input.deck.clone().try_into().unwrap();
    let mut right = left.clone();
    left[0].support_skills.push((9880, 1));
    left[0].gekisou_support_skills.push((9882, 1));
    right[2].support_skills.push((9880, 1));
    right[2].gekisou_support_skills.push((9882, 1));
    (fixture, left, right)
}

fn attempt(
    fixture: &FamilyFixture,
    left: &[Performer; 5],
    right: &[Performer; 5],
    budget: &mut LuckExactBudget,
    cancelled: impl FnMut() -> bool,
) -> LuckScoreEquivalenceAttempt {
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    certify_uniform_score_equivalence(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        left,
        right,
        budget,
        cancelled,
    )
    .unwrap()
}

// No bounds, reduced recorder or proposed equality supplies this oracle. Enumerate native nominal branches
// and accumulate exact rational terminal (score, LIFE) masses directly from every completed native playback.
fn native_scores(input: &RushCase, deck: &[Performer]) -> BTreeMap<(i32, i32), Fraction> {
    let mut pending = vec![(Vec::new(), Fraction::ONE)];
    let mut out = BTreeMap::<(i32, i32), Fraction>::new();
    let mut visits = 0;
    while let Some((prefix, mass)) = pending.pop() {
        visits += 1;
        assert!(visits < 4096 && prefix.len() <= 16);
        let mut model =
            LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        let result = model.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix.clone()));
        assert!(model.random.nominal_covers_draws());
        if let Some(outcomes) = model.random.nominal_branch() {
            assert!(result.is_err());
            let total = outcomes[0].total;
            assert_eq!(outcomes.iter().map(|outcome| outcome.weight).sum::<u64>(), total);
            for (choice, outcome) in outcomes.iter().enumerate() {
                assert_eq!(outcome.total, total);
                let mut next = prefix.clone();
                next.push(choice);
                pending.push((next, mass.times(u128::from(outcome.weight), u128::from(total))));
            }
        } else {
            result.unwrap();
            assert!(model.random.nominal_prefix_consumed());
            out.entry((model.score(), model.current_life())).and_modify(|old| *old = old.plus(mass)).or_insert(mass);
        }
    }
    assert_eq!(out.values().copied().fold(Fraction::ZERO, Fraction::plus), Fraction::ONE);
    out
}

#[test]
fn score_equivalence_inactive_owner_move_keeps_all_120_complete_native_laws() {
    let (fixture, left, right) = fixture_pair();
    let mut budget = LuckExactBudget::default();
    let before = budget.clone();
    // A different physical layout must not change the complete uniform law or prevent the fixed alignment.
    let permuted = [right[1].clone(), right[2].clone(), right[0].clone(), right[4].clone(), right[3].clone()];
    let result = attempt(&fixture, &left, &permuted, &mut budget, || false);
    assert!(result.certificate.is_some(), "{result:?}");
    assert_eq!(result.orders_compared, 120);
    assert_eq!(result.recording_runs, 4 * 120);
    assert_eq!(result.recording_frames, result.recording_runs * fixture.input.play.frames.len() as u64);
    assert_eq!(result.recording_runs, before.remaining_runs - budget.remaining_runs);
    assert_eq!(result.recording_frames, before.remaining_frames - budget.remaining_frames);
    for order in physical_orders() {
        let left: Vec<_> = order.iter().map(|&slot| left[slot].clone()).collect();
        let right: Vec<_> = order.iter().map(|&slot| right[slot].clone()).collect();
        assert_eq!(native_scores(&fixture.input, &left), native_scores(&fixture.input, &right));
    }
}

#[test]
fn score_equivalence_cancellation_and_work_budget_report_partial_recording_without_a_certificate() {
    let (fixture, left, right) = fixture_pair();
    let mut budget = LuckExactBudget { remaining_runs: 10, remaining_frames: 17 };
    let result = attempt(&fixture, &left, &right, &mut budget, || false);
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::WorkBudget));
    assert_eq!(result.orders_compared, 0);
    assert_eq!(result.recording_runs, 1);
    assert_eq!(result.recording_frames, 17);
    assert_eq!(budget.remaining_frames, 0);
    let mut budget = LuckExactBudget::default();
    let before = budget.clone();
    let mut calls = 0;
    let result = attempt(&fixture, &left, &right, &mut budget, || {
        calls += 1;
        calls >= 240
    });
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::Cancelled));
    assert!(result.recording_frames > 0 && result.orders_compared < 120);
    assert_eq!(result.recording_runs, before.remaining_runs - budget.remaining_runs);
    assert_eq!(result.recording_frames, before.remaining_frames - budget.remaining_frames);
    let result = attempt(&fixture, &left, &right, &mut budget, || true);
    assert_eq!((result.recording_runs, result.recording_frames, result.orders_compared), (0, 0, 0));
}

#[test]
fn score_equivalence_refuses_changed_commands_controllers_and_nonexact_action_chances() {
    let (mut fixture, mut left, mut right) = fixture_pair();
    right[2].support_skills.push((FAMILY_REWARD, 1));
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::ScoreTrace));
    right[2].support_skills.pop();
    left[0].gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
    let result = attempt(&fixture, &left, &left, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::ActionChance));
    fixture
        .input
        .master
        .gekisou_support_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == FAMILY_GUARANTEE)
        .unwrap()
        .skill_condition_group = 0;
    fixture.input.master.reindex().unwrap();
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::ControllerTrace));
}

#[test]
fn score_equivalence_same_owner_tie_guard_keeps_zero_probes_and_rejects_later_live_phases() {
    let (mut fixture, left, right) = fixture_pair();
    for row in &mut fixture.input.master.gekisou_skill_effects {
        if row.skill_id == FAMILY_SCORE {
            row.effect_value = 0;
        }
    }
    fixture.input.master.reindex().unwrap();
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_some(), "zero-valued active probes still have a proved emission order: {result:?}");
    for row in &mut fixture.input.master.gekisou_skill_effects {
        if row.skill_id == FAMILY_SCORE {
            row.skill_effect_type = 2005;
        }
    }
    fixture.input.master.skill_effect_settings.push(
        serde_json::from_value(json!({
            "_id":9883,"_skillEffectType":2005,"_phase":1
        }))
        .unwrap(),
    );
    fixture.input.master.reindex().unwrap();
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::OrdinaryTie));
    assert_eq!((result.recording_runs, result.recording_frames), (0, 0));
}

#[test]
fn score_equivalence_projects_positive_note_life_but_never_erases_a_death() {
    let (mut fixture, left, right) = fixture_pair();
    for row in &mut fixture.input.master.judgement_parameters {
        if row.note_simulate_judgement == 5 {
            row.damage = 50;
        }
    }
    fixture
        .input
        .master
        .support_skill_effects
        .iter_mut()
        .find(|row| row.support_skill_id == 9880)
        .unwrap()
        .effect_value = 1000;
    // Every possible owner gets this second native event. After the last scored note, both positive
    // histories reach the same LIFE cap, regardless of which original performance position owned the heal.
    fixture.input.events.extend((0..5).map(|position| (position, 600)));
    fixture.input.master.reindex().unwrap();
    let filed_life = |input: &RushCase, deck: &[Performer; 5]| {
        let mut model =
            LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        model.score.begin_bounds(Vec::new(), false);
        model.run_timed(&input.play, &input.delta).unwrap();
        let final_life = model.current_life();
        let trace = model.score.bounds_trace.take().unwrap();
        let notes: BTreeMap<_, _> = trace
            .events
            .iter()
            .filter_map(|event| match event {
                BoundsEvent::Note { note, .. } => Some((note.note_id, note.life)),
                _ => None,
            })
            .collect();
        (notes, final_life)
    };
    // Under this physical order the left heal fires at 80 ms, the right one at 200 ms. The 180 ms note
    // therefore witnesses the actual numeric LIFE distinction that the certificate projects to positive.
    let witness = fixture.input.notes.iter().find(|note| note.time_ms == 180).unwrap().note_id;
    let (left_notes, left_final) = filed_life(&fixture.input, &left);
    let (right_notes, right_final) = filed_life(&fixture.input, &right);
    assert_ne!(left_notes[&witness], right_notes[&witness]);
    assert!(left_notes[&witness] > 0 && right_notes[&witness] > 0);
    assert!(left_notes.values().chain(right_notes.values()).all(|&life| life > 0));
    assert_eq!(left_final, right_final);
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_some(), "positive numeric LIFE is not a score input: {result:?}");
    assert_eq!(result.orders_compared, 120);
    for order in physical_orders() {
        let left: Vec<_> = order.iter().map(|&slot| left[slot].clone()).collect();
        let right: Vec<_> = order.iter().map(|&slot| right[slot].clone()).collect();
        assert_eq!(native_scores(&fixture.input, &left), native_scores(&fixture.input, &right));
    }

    // Change only native judgement damage. At that SAME note the early-healed side still lives and the
    // later-healed side is dead; even the common final LIFE no longer licenses erasing this score input.
    for row in &mut fixture.input.master.judgement_parameters {
        if row.note_simulate_judgement == 5 {
            row.damage = 600;
        }
    }
    fixture.input.master.reindex().unwrap();
    let (left_notes, left_final) = filed_life(&fixture.input, &left);
    let (right_notes, right_final) = filed_life(&fixture.input, &right);
    assert!(left_notes[&witness] > 0 && right_notes[&witness] <= 0);
    assert_eq!(left_final, right_final);
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_none());
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::ScoreTrace));
    let left_law = native_scores(&fixture.input, &left);
    let right_law = native_scores(&fixture.input, &right);
    let score_law = |law: BTreeMap<(i32, i32), Fraction>| {
        let mut scores = BTreeMap::<i32, Fraction>::new();
        for ((score, _), mass) in law {
            scores.entry(score).and_modify(|old| *old = old.plus(mass)).or_insert(mass);
        }
        scores
    };
    assert_ne!(score_law(left_law), score_law(right_law), "native score, not only LIFE metadata, differs");
}

fn active_counter_fixture() -> (FamilyFixture, [Performer; 5], [Performer; 5]) {
    let (mut fixture, mut left, mut right) = fixture_pair();
    // Every score-up delta in this fixture is a small exact binary fraction. The changed native owner
    // order therefore has equal arithmetic, while the actual command tapes are deliberately different.
    for row in &mut fixture.input.master.live_skill_effects {
        if row.live_skill_id == 9401 {
            row.effect_value = 2500;
        }
        if row.live_skill_id == 9402 {
            row.effect_value = 5000;
        }
    }
    for row in &mut fixture.input.master.gekisou_skill_effects {
        if row.skill_id == FAMILY_SCORE {
            row.effect_value = 1250;
        }
    }
    fixture.input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 9890,
        condition_type: 1030,
        condition_values: vec![1],
        condition_target_ids: vec![9251],
        is_positive: true,
    });
    fixture.input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 9890,
        group: 9890,
        condition_ids: vec![9890],
    });
    fixture.input.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":9890,"_supportSkillID":9890,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":9890,"_skillEffectType":2000,"_effectValue":3750,
            "_activationTimeSecond":0.3
        }))
        .unwrap(),
    );
    // A second source changes only the counter's amplitude for the rejection witness below.
    fixture.input.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":9891,"_supportSkillID":9891,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":9890,"_skillEffectType":2000,"_effectValue":5000,
            "_activationTimeSecond":0.3
        }))
        .unwrap(),
    );
    fixture.input.master.reindex().unwrap();
    left[0].support_skills.push((9890, 1));
    right[2].support_skills.push((9890, 1));
    assert_ne!(left[0].live_skill, right[2].live_skill, "the relocated source crosses distinct member programs");
    (fixture, left, right)
}

#[test]
fn score_equivalence_active_counter_owner_move_folds_every_timeline_and_all_120_native_laws() {
    let (fixture, left, mut right) = active_counter_fixture();
    let counter_commands = |deck: &[Performer; 5]| {
        let input = &fixture.input;
        let mut model =
            LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        model.score.begin_bounds(Vec::new(), false);
        model.run_timed(&input.play, &input.delta).unwrap();
        model
            .score
            .bounds_trace
            .take()
            .unwrap()
            .events
            .into_iter()
            .filter_map(|event| match event {
                BoundsEvent::Factor { frame, command } if command.note_mill.abs() == 37500 => Some((frame, command)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let left_commands = counter_commands(&left);
    let right_commands = counter_commands(&right);
    assert!(!left_commands.is_empty(), "the global Perfect counter really emitted native factors");
    assert!(left_commands.iter().any(|(_, command)| command.note_mill > 0));
    assert!(left_commands.iter().any(|(_, command)| command.note_mill < 0));
    assert!(left_commands.iter().all(|(_, command)| command.owner_id == 2));
    assert!(right_commands.iter().all(|(_, command)| command.owner_id == 202));
    assert_ne!(left_commands, right_commands, "strict command identity cannot certify this pair");
    let without_owner = |commands: Vec<(usize, FactorCommand)>| {
        commands
            .into_iter()
            .map(|(frame, mut command)| {
                command.owner_id = 0;
                (frame, command)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(without_owner(left_commands), without_owner(right_commands));

    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_some(), "complete exact native folding should establish equality: {result:?}");
    assert_eq!(result.orders_compared, 120);
    assert_eq!(result.timeline_orders, 120, "all orders have different active owner commands");
    assert!(result.timeline_paths > 120 && result.score_fold_queries > 0);
    for order in physical_orders() {
        let ordered_left: Vec<_> = order.iter().map(|&slot| left[slot].clone()).collect();
        let ordered_right: Vec<_> = order.iter().map(|&slot| right[slot].clone()).collect();
        assert_eq!(native_scores(&fixture.input, &ordered_left), native_scores(&fixture.input, &ordered_right));
    }

    let last = right[2].support_skills.last_mut().unwrap();
    assert_eq!(*last, (9890, 1));
    *last = (9891, 1);
    let result = attempt(&fixture, &left, &right, &mut LuckExactBudget::default(), || false);
    assert!(result.certificate.is_none(), "changed integer rewards cannot be hidden by identical controller paths");
    assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::ScoreTrace));
    assert!(result.score_fold_queries > 0, "the differing scalar score is checked by the exact fold");
    assert_ne!(native_scores(&fixture.input, &left), native_scores(&fixture.input, &right));
}

type NativeTimeline = Vec<(usize, u8, i32, bool, bool)>;

// This oracle discovers timelines from actual unweighted native Factor filings. It does not read the
// reduced controller, the support traversal or its reconstructed commands. Different hidden nominal
// branches with the same observable timeline must each have the same native terminal integer score.
fn native_timeline_scores(input: &RushCase, deck: &[Performer; 5]) -> BTreeMap<NativeTimeline, i32> {
    let probe_member = deck.iter().position(|member| member.gekisou_skill == Some((FAMILY_SCORE, 1))).unwrap();
    let probe_owner = probe_member as i32 * 100 + 1;
    let mut pending = vec![Vec::new()];
    let mut scores = BTreeMap::new();
    let mut visits = 0;
    let mut complete_paths = 0;
    while let Some(prefix) = pending.pop() {
        visits += 1;
        assert!(visits < 4096 && prefix.len() <= 16);
        let mut model =
            LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        model.score.begin_bounds(Vec::new(), true);
        model.set_random(LiveRandom::with_nominal_prefix(prefix.clone()));
        let mut path = Vec::new();
        let mut cursor = 0;
        let mut failed = false;
        for (frame, (input_frame, &delta)) in input.play.frames.iter().zip(&input.delta).enumerate() {
            if model.frame_timed(input_frame.time_ms, &input_frame.judged, delta).is_err() {
                failed = true;
                break;
            }
            let trace = model.score.bounds_trace.as_ref().unwrap();
            let mut queries = 0;
            for event in &trace.events[cursor..] {
                match event {
                    BoundsEvent::Query { .. } => queries += 1,
                    BoundsEvent::Factor { command, .. } if command.owner_id == -1 => {
                        assert_eq!(command.luck.abs(), 47);
                        let stage = if queries == 0 {
                            0
                        } else {
                            assert_eq!(queries, 2);
                            2
                        };
                        path.push((frame, stage, command.time_ms, true, command.luck > 0));
                    }
                    BoundsEvent::Factor { command, .. }
                        if command.owner_id == probe_owner && command.note_mill.abs() == 70000 =>
                    {
                        assert_eq!(queries, 1, "native probes file between the two primary queries");
                        path.push((frame, 1, command.time_ms, false, command.note_mill > 0));
                    }
                    _ => {}
                }
            }
            cursor = trace.events.len();
        }
        assert!(model.random.nominal_covers_draws());
        if let Some(outcomes) = model.random.nominal_branch() {
            assert!(failed);
            for choice in 0..outcomes.len() {
                let mut next = prefix.clone();
                next.push(choice);
                pending.push(next);
            }
        } else {
            assert!(!failed);
            assert!(model.random.nominal_prefix_consumed());
            complete_paths += 1;
            let score = model.score();
            if let Some(previous) = scores.insert(path, score) {
                assert_eq!(previous, score, "hidden lottery state cannot change a fixed complete reward timeline");
            }
        }
    }
    assert!(complete_paths > 1 && scores.len() > 1);
    assert!(scores.keys().any(|path| path.iter().any(|edge| edge.3 && edge.4)));
    assert!(scores.keys().any(|path| path.iter().any(|edge| !edge.3 && edge.4)));
    scores
}

#[test]
fn score_equivalence_nondyadic_fold_matches_every_independent_native_timeline() {
    let (fixture, left, _) = fixture_pair();
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    assert_eq!(
        input.master.live_skill_effects.iter().find(|row| row.live_skill_id == 9401).unwrap().effect_value,
        3500
    );
    assert_eq!(
        input.master.live_skill_effects.iter().find(|row| row.live_skill_id == 9402).unwrap().effect_value,
        9000
    );
    assert_eq!(
        input.master.gekisou_skill_effects.iter().find(|row| row.skill_id == FAMILY_SCORE).unwrap().effect_value,
        7000
    );
    for order in [[0, 1, 2, 3, 4], [2, 0, 4, 1, 3]] {
        let deck = order.map(|slot| left[slot].clone());
        let audit = crate::live::full::luck_dp::audit_score_fold(
            &input.master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
            &input.play,
            &input.delta,
            &deck,
            None,
        )
        .unwrap();
        let mut folded = BTreeMap::<NativeTimeline, i32>::new();
        for (mut path, score) in audit.paths {
            // Both note-driven and pending consumes file after the second primary query and before
            // ProbabilityReady. Their order remains explicit, while native Factor logs need no subtype.
            for edge in &mut path {
                if edge.3 && edge.1 == 3 {
                    edge.1 = 2;
                }
            }
            if let Some(previous) = folded.insert(path, score) {
                assert_eq!(previous, score);
            }
        }
        assert_eq!(folded, native_timeline_scores(input, &deck), "complete per-path integer scores, order={order:?}");
    }
}

#[test]
fn score_equivalence_fold_clamps_late_probe_end_to_actual_music_boundary() {
    let (mut fixture, deck, _) = fixture_pair();
    fixture.input.params.music_length_ms = 500;
    // Score-frame storage can have a separate horizon; effect lifecycles still clamp to actual music length.
    fixture.input.params.score_music_length_ms = Some(1600);
    // Produce additional non-dyadic ordinary endings at that same boundary, so the native FrameDiff
    // must combine those filings with a later, backdated probe ending in its original sorted order.
    fixture.input.events.extend((0..5).map(|position| (position, 380)));
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let audit = crate::live::full::luck_dp::audit_score_fold(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        &deck,
        None,
    )
    .unwrap();
    assert!(
        audit
            .paths
            .iter()
            .any(|(path, _)| path.iter().any(|edge| { !edge.3 && !edge.4 && edge.2 > input.params.music_length_ms })),
        "complete support must contain a probe-off after the music boundary"
    );
    let native = native_timeline_scores(input, &deck);
    assert!(
        native.keys().any(|path| path.iter().any(|edge| {
            !edge.3
                && !edge.4
                && input.play.frames[edge.0].time_ms > input.params.music_length_ms
                && edge.2 == input.params.music_length_ms
        })),
        "the independent native recorder must actually file the late ending at the music boundary"
    );
    let mut folded = BTreeMap::<NativeTimeline, i32>::new();
    for (mut path, score) in audit.paths {
        for edge in &mut path {
            if edge.3 && edge.1 == 3 {
                edge.1 = 2;
            }
            if !edge.3 && !edge.4 {
                edge.2 = edge.2.min(input.params.music_length_ms);
            }
        }
        if let Some(previous) = folded.insert(path, score) {
            assert_eq!(previous, score);
        }
    }
    assert_eq!(folded, native, "every actual filed timeline and exact terminal score must match");
}

#[test]
fn score_equivalence_cancellation_after_native_fold_work_never_publishes_a_certificate() {
    let (fixture, left, right) = active_counter_fixture();
    let mut witnessed_fold = false;
    // Discover a stop inside the new route using a small bounded set of poll thresholds. This avoids
    // fixing the test to an incidental number of native-recorder or controller-support polls.
    for stop_at in [256, 512, 1024, 2048, 4096] {
        let mut polls = 0;
        let mut budget = LuckExactBudget::default();
        let before = budget.clone();
        let result = attempt(&fixture, &left, &right, &mut budget, || {
            polls += 1;
            polls >= stop_at
        });
        assert!(result.certificate.is_none(), "a stopped partial uniform proof must never escape: {result:?}");
        assert_eq!(result.decline, Some(LuckScoreEquivalenceDecline::Cancelled));
        assert!(result.orders_compared < 120);
        assert_eq!(result.recording_runs, before.remaining_runs - budget.remaining_runs);
        assert_eq!(result.recording_frames, before.remaining_frames - budget.remaining_frames);
        if result.score_fold_queries > 0 {
            witnessed_fold = true;
            assert!(result.recording_frames > 0);
            break;
        }
    }
    assert!(witnessed_fold, "the cancellation must happen after exact score-fold queries have run");
}

#[test]
fn score_equivalence_shared_prefixes_reduce_queries_and_preserve_complete_capacity_fallback() {
    let (fixture, deck, _) = active_counter_fixture();
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let audit = |limit| {
        crate::live::full::luck_dp::audit_score_fold(
            &input.master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
            &input.play,
            &input.delta,
            &deck,
            limit,
        )
        .unwrap()
    };
    let shared = audit(None);
    assert!(shared.paths.len() > 1);
    assert!(shared.shared_queries < shared.independent_queries, "common native prefixes must execute once");
    assert!(shared.checkpoint_peak_bytes > 0 && shared.checkpoint_peak_bytes <= 32 * 1024 * 1024);
    let bounded = audit(Some(shared.checkpoint_peak_bytes / 2));
    assert_eq!(bounded.paths, shared.paths, "checkpoint exhaustion must preserve every complete timeline");
    let disabled = audit(Some(0));
    assert_eq!(disabled.paths, shared.paths);
    assert_eq!(disabled.shared_queries, disabled.independent_queries, "zero checkpoint capacity uses the native tape");
}
