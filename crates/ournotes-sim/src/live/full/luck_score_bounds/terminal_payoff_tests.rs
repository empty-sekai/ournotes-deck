//! Independent full-native nominal branches supply every reference probability and terminal value.
use super::*;
use crate::live::full::{LuckTerminalPayoff, LuckTerminalPayoffAttempt, LuckTerminalPayoffSession};

fn fold(
    input: &RushCase,
    deck: &[Performer; 5],
    map: LuckTerminalPayoff,
    budget: &mut LuckExactBudget,
    cancelled: impl FnMut() -> bool,
) -> LuckTerminalPayoffAttempt {
    let skills = luck_skills(&input.master).unwrap();
    LuckTerminalPayoffSession::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
    )
    .payoff(deck, map, budget, cancelled)
    .unwrap()
}

fn mapped_value(map: LuckTerminalPayoff, score: i32, life: i32) -> i32 {
    match map {
        LuckTerminalPayoff::ScoreAtLeast { threshold } => i32::from(score >= threshold),
        LuckTerminalPayoff::CappedScore { threshold } => score.min(threshold),
        LuckTerminalPayoff::ScoreAndLifeAtLeast { threshold, min_final_life } => {
            i32::from(score >= threshold && life >= min_final_life)
        }
    }
}

fn assert_native_payoff(
    input: &RushCase,
    deck: &[Performer; 5],
    law: &BTreeMap<(i32, i32), Fraction>,
    map: LuckTerminalPayoff,
) -> LuckTerminalPayoffAttempt {
    let mut values = std::collections::BTreeSet::new();
    let expected = law.iter().fold(Fraction::ZERO, |total, (&(score, life), &mass)| {
        let value = mapped_value(map, score, life);
        values.insert(value);
        total.plus(mass.times(u128::try_from(value).expect("nonnegative oracle mapping"), 1))
    });
    let result = fold(input, deck, map, &mut LuckExactBudget::default(), || false);
    let payoff = result.payoff.as_ref().unwrap_or_else(|| panic!("complete native mapping declined: {result:?}"));
    assert_eq!(payoff.map(), map);
    assert!(expected.at_least(payoff.bounds().lower()) && expected.at_most(payoff.bounds().upper()),
        "exact native payoff {expected:?} outside {:?}", payoff.bounds());
    assert_eq!(payoff.exact_constant(), (values.len() == 1).then(|| *values.first().unwrap()));
    assert!(payoff.bounds().upper() - payoff.bounds().lower() < 1e-5);
    assert!(result.timeline_paths > 0 && result.score_fold_queries > 0);
    result
}

#[test]
fn terminal_payoff_matches_native_cdf_caps_and_life_for_nondyadic_distinct_controllers() {
    let (mut fixture, left, _) = fixture_pair();
    for row in &mut fixture.input.master.gekisou_luck_bonus_lots {
        row.weight = if row.lot_result == 3 { 2 } else { 1 };
    }
    fixture.input.master.gekisou_support_skill_effects.iter_mut()
        .find(|row| row.skill_id == FAMILY_GUARANTEE).unwrap().skill_condition_group = 0;
    fixture.input.master.reindex().unwrap();
    let mut guaranteed = left.clone();
    guaranteed[0].gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
    let first = native_scores(&fixture.input, &left);
    let other = native_scores(&fixture.input, &guaranteed);
    assert_ne!(first, other, "the physical support changes the controller, not only its score tape");
    assert!(first.values().any(|mass| !mass.denominator.is_power_of_two()));
    for deck in [&left, &guaranteed] {
        for order in [[0, 1, 2, 3, 4], [2, 0, 4, 1, 3]] {
            let deck = order.map(|slot| deck[slot].clone());
            let law = native_scores(&fixture.input, &deck);
            let scores: Vec<_> = law.keys().map(|&(score, _)| score).collect();
            let middle = scores[scores.len() / 2];
            let life = law.keys().next().unwrap().1;
            assert!(law.keys().all(|&(_, value)| value == life));
            for map in [
                LuckTerminalPayoff::ScoreAtLeast { threshold: scores[0] },
                LuckTerminalPayoff::ScoreAtLeast { threshold: middle },
                LuckTerminalPayoff::ScoreAtLeast { threshold: scores.last().unwrap() + 1 },
                LuckTerminalPayoff::CappedScore { threshold: 0 },
                LuckTerminalPayoff::CappedScore { threshold: middle },
                LuckTerminalPayoff::CappedScore { threshold: *scores.last().unwrap() },
                LuckTerminalPayoff::ScoreAndLifeAtLeast { threshold: middle, min_final_life: life },
                LuckTerminalPayoff::ScoreAndLifeAtLeast { threshold: middle, min_final_life: life + 1 },
            ] {
                assert_native_payoff(&fixture.input, &deck, &law, map);
            }
        }
    }
    // Deduplicated observable timelines are not equiprobable. Find an actual threshold at which a
    // uniform count of those timelines gives a different answer from the native rational branch law.
    let audit = crate::live::full::luck_dp::audit_score_fold(
        &fixture.input.master, &luck_skills(&fixture.input.master).unwrap(), &fixture.input.notes,
        &fixture.input.events, fixture.input.params, &fixture.input.setup, &fixture.input.play,
        &fixture.input.delta, &left, None,
    ).unwrap();
    let witness = first.keys().map(|&(score, _)| score).find(|&threshold| {
        let true_mass = first.iter().filter(|(key, _)| key.0 >= threshold)
            .fold(Fraction::ZERO, |sum, (_, &mass)| sum.plus(mass));
        let uniform = Fraction::new(audit.paths.iter().filter(|(_, score)| *score >= threshold).count() as u128,
            audit.paths.len() as u128);
        true_mass != uniform
    }).expect("nonuniform native timeline mass has a CDF witness");
    let result = assert_native_payoff(&fixture.input, &left, &first,
        LuckTerminalPayoff::ScoreAtLeast { threshold: witness });
    assert!(result.payoff.unwrap().exact_constant().is_none(), "different terminal values never claim constancy");
}

#[test]
fn terminal_payoff_complete_constants_keep_exact_life_and_signed_caps() {
    let (mut fixture, with_heal, _) = fixture_pair();
    for row in &mut fixture.input.master.judgement_parameters {
        if row.note_simulate_judgement == 5 { row.damage = 100; }
    }
    fixture.input.events.retain(|&(owner, _)| owner != 0);
    fixture.input.events.push((0, 200));
    fixture.input.events.sort_by_key(|&(_, time)| time);
    fixture.input.master.reindex().unwrap();
    let mut without_heal = with_heal.clone();
    without_heal[0].support_skills.retain(|&(skill, _)| skill != 9880);
    let healed = native_scores(&fixture.input, &with_heal);
    let unhealed = native_scores(&fixture.input, &without_heal);
    let high_life = healed.keys().next().unwrap().1;
    let low_life = unhealed.keys().next().unwrap().1;
    assert!(high_life > low_life);
    assert!(healed.keys().all(|&(_, life)| life == high_life));
    assert!(unhealed.keys().all(|&(_, life)| life == low_life));
    for (deck, law, expected) in [(&with_heal, &healed, 1), (&without_heal, &unhealed, 0)] {
        let result = assert_native_payoff(&fixture.input, deck, law,
            LuckTerminalPayoff::ScoreAndLifeAtLeast { threshold: 0, min_final_life: high_life });
        assert_eq!(result.payoff.unwrap().exact_constant(), Some(expected));
        for threshold in [i32::MIN, -1, 0] {
            let result = fold(&fixture.input, deck, LuckTerminalPayoff::CappedScore { threshold },
                &mut LuckExactBudget::default(), || false);
            let payoff = result.payoff.unwrap();
            assert_eq!(payoff.exact_constant(), Some(threshold));
            assert_eq!(payoff.bounds(), F64Interval::integer(i128::from(threshold)));
        }
    }
}

#[test]
fn terminal_payoff_cancellation_budget_capacity_and_incomplete_support_publish_nothing() {
    let (fixture, deck, _) = fixture_pair();
    let map = LuckTerminalPayoff::ScoreAtLeast { threshold: i32::MAX };
    let mut polls = 0;
    let complete = fold(&fixture.input, &deck, map, &mut LuckExactBudget::default(), || { polls += 1; false });
    assert_eq!(complete.payoff.unwrap().exact_constant(), Some(0));
    let mut cancelled_polls = 0;
    let cancelled = fold(&fixture.input, &deck, map, &mut LuckExactBudget::default(), || {
        cancelled_polls += 1;
        cancelled_polls >= polls - 1
    });
    assert!(cancelled.payoff.is_none());
    assert_eq!(cancelled.decline, Some(LuckScoreEquivalenceDecline::Cancelled));
    assert!(cancelled.score_fold_queries > 0, "cancellation after useful work cannot publish a constant");
    let mut zero = LuckExactBudget { remaining_runs: 0, remaining_frames: 999 };
    let declined = fold(&fixture.input, &deck, map, &mut zero, || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::WorkBudget));
    assert_eq!((declined.recording_runs, declined.recording_frames), (0, 0));
    let mut one = LuckExactBudget { remaining_runs: 10, remaining_frames: 1 };
    let declined = fold(&fixture.input, &deck, map, &mut one, || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::WorkBudget));
    assert_eq!(declined.recording_frames, 1);
    let mut unfinished = fixture.input.clone();
    unfinished.play.frames.truncate(6);
    unfinished.delta.truncate(6);
    let declined = fold(&unfinished, &deck, map, &mut LuckExactBudget::default(), || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::Unfinished));
    let mut large = fixture.input.clone();
    large.params.score_music_length_ms = Some(400_000);
    let declined = fold(&large, &deck, map, &mut LuckExactBudget::default(), || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::Capacity));
    let mut probabilistic = deck.clone();
    probabilistic[0].gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
    let declined = fold(&fixture.input, &probabilistic, map, &mut LuckExactBudget::default(), || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::ActionChance));
}

fn ordinary_and_probe_fixture() -> (FamilyFixture, [Performer; 5]) {
    let (mut fixture, mut deck, _) = fixture_pair();
    fixture.input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 9894, gekisou_mission_type: 2, ..Default::default()
    });
    fixture.input.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 9894, skill_id: 9894, level: 1, skill_trigger_type: SUSTAINED,
        skill_trigger_condition_group: FAMILY_PROBE, skill_effect_type: 2000, effect_value: 7000,
        ..Default::default()
    });
    fixture.input.master.support_skill_effects.iter_mut()
        .find(|row| row.support_skill_id == FAMILY_REWARD).unwrap().effect_value = 2500;
    deck[0].support_skills.push((FAMILY_REWARD, 1));
    deck[0].gekisou_support_skills.push((9894, 1));
    fixture.input.events.retain(|&(owner, _)| owner != 0);
    fixture.input.events.push((0, 140));
    fixture.input.events.sort_by_key(|&(_, time)| time);
    fixture.input.master.reindex().unwrap();
    (fixture, deck)
}

// Observe both same-owner positive filings in the actual native frame on a possible nominal branch.
fn native_same_frame_ordinary_precedes_probe(input: &RushCase, deck: &[Performer; 5]) {
    let mut pending = vec![Vec::new()];
    let mut witnessed = false;
    let mut visits = 0;
    while let Some(prefix) = pending.pop() {
        visits += 1;
        assert!(visits < 4096 && prefix.len() <= 16);
        let mut model = LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events,
            input.params, &input.setup).unwrap();
        model.score.begin_bounds(Vec::new(), true);
        model.set_random(LiveRandom::with_nominal_prefix(prefix.clone()));
        let mut cursor = 0;
        let mut failed = false;
        for (frame, &delta) in input.play.frames.iter().zip(&input.delta) {
            if model.frame_timed(frame.time_ms, &frame.judged, delta).is_err() { failed = true; break; }
            let trace = model.score.bounds_trace.as_ref().unwrap();
            let commands: Vec<_> = trace.events[cursor..].iter().filter_map(|event| match event {
                BoundsEvent::Factor { command, .. } if command.owner_id == 2 => Some(command.note_mill),
                _ => None,
            }).collect();
            if let (Some(ordinary), Some(probe)) = (commands.iter().position(|&value| value == 25000),
                commands.iter().position(|&value| value == 70000)) {
                assert!(ordinary < probe);
                witnessed = true;
            }
            cursor = trace.events.len();
        }
        assert!(model.random.nominal_covers_draws());
        if let Some(outcomes) = model.random.nominal_branch() {
            assert!(failed);
            for choice in 0..outcomes.len() {
                let mut next = prefix.clone(); next.push(choice); pending.push(next);
            }
        } else {
            assert!(!failed && model.random.nominal_prefix_consumed());
        }
    }
    assert!(witnessed, "the fixture must exercise two actual same-owner filings in one native frame");
}

#[test]
fn terminal_payoff_same_owner_early_conditional_source_matches_native_and_late_sources_refuse() {
    let (fixture, deck) = ordinary_and_probe_fixture();
    native_same_frame_ordinary_precedes_probe(&fixture.input, &deck);
    let law = native_scores(&fixture.input, &deck);
    let threshold = law.keys().nth(law.len() / 2).unwrap().0;
    for map in [LuckTerminalPayoff::ScoreAtLeast { threshold }, LuckTerminalPayoff::CappedScore { threshold }] {
        assert_native_payoff(&fixture.input, &deck, &law, map);
    }
    let mut later = fixture.input.clone();
    later.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 9895, gekisou_mission_type: 2, ..Default::default()
    });
    later.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 9895, skill_id: 9895, level: 1, skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: FAMILY_LIVE, skill_effect_type: 2000, effect_value: 2500,
        activation_time_second: 0.1, ..Default::default()
    });
    later.master.reindex().unwrap();
    let mut later_deck = deck.clone();
    later_deck[0].gekisou_support_skills.push((9895, 1));
    let map = LuckTerminalPayoff::CappedScore { threshold };
    let declined = fold(&later, &later_deck, map, &mut LuckExactBudget::default(), || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::OrdinaryTie));
    let mut later_phase = fixture.input.clone();
    later_phase.master.skill_effect_settings.push(serde_json::from_value(json!({
        "_id":9896,"_skillEffectType":2005,"_phase":1
    })).unwrap());
    later_phase.master.gekisou_support_skill_effects.iter_mut()
        .find(|row| row.skill_id == 9894).unwrap().skill_effect_type = 2005;
    later_phase.master.reindex().unwrap();
    let declined = fold(&later_phase, &deck, map, &mut LuckExactBudget::default(), || false);
    assert!(declined.payoff.is_none());
    assert_eq!(declined.decline, Some(LuckScoreEquivalenceDecline::OrdinaryTie));
}
