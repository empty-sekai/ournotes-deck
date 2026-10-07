use super::*;

#[path = "terminal_recipe_tests.rs"]
mod terminal_recipe_tests;

fn assert_same_terminal(input: &RushCase, actual: &LuckTerminalRush, expected: &LuckTerminalRush) {
    let mut times: Vec<_> = input.notes.iter().map(|note| note.time_ms).collect();
    times.sort_unstable();
    assert_eq!(actual.probe_gate(), expected.probe_gate());
    assert_eq!(
        actual.note_score_up_upper(&times).map(field_bits),
        expected.note_score_up_upper(&times).map(field_bits)
    );
    let summary_bits = |summary: Option<LuckScoreSummary>| {
        summary.map(|value| {
            (
                value.final_mean.lower.to_bits(),
                value.final_mean.upper.to_bits(),
                value.final_support.lower,
                value.final_support.upper,
                value.exact_constant_score,
                value.exact_final_life,
                value.probability_peak_states,
                value.probability_transitions,
            )
        })
    };
    for power in [-1, i64::from(input.params.total_power), i64::from(input.params.total_power) + 1, i64::MAX] {
        assert_eq!(summary_bits(actual.terminal_summary(power)), summary_bits(expected.terminal_summary(power)));
        assert_eq!(actual.native_note_bucket_caps(power, &times), expected.native_note_bucket_caps(power, &times));
        assert_eq!(
            actual.native_score_mean_upper(power).map(f64::to_bits),
            expected.native_score_mean_upper(power).map(f64::to_bits)
        );
    }
    let caps: Vec<_> = times.iter().map(|&time| (time, [3, 3, 31, 31])).collect();
    assert_eq!(
        actual.weighted_note_bucket_upper(&caps).map(f64::to_bits),
        expected.weighted_note_bucket_upper(&caps).map(f64::to_bits)
    );
    let curve_bits = |terminal: &LuckTerminalRush| {
        let curve = terminal.cache_curve();
        (
            curve
                .steps
                .iter()
                .map(|&(time, masses)| {
                    (time, masses.map(|mass| [mass.interval().lower().to_bits(), mass.interval().upper().to_bits()]))
                })
                .collect::<Vec<_>>(),
            curve.probes.clone(),
            curve.peak_states,
            curve.transitions,
        )
    };
    assert_eq!(curve_bits(actual), curve_bits(expected));
}

fn replay_summary(input: &RushCase, cache: Option<&mut LuckDpCache>) -> LuckScoreSummary {
    let skills = luck_skills(&input.master).unwrap();
    LuckScoreSession::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        input.ranking.as_deref(),
    )
    .summary(&input.deck, cache, || false)
    .unwrap()
    .unwrap()
}

#[test]
fn terminal_cache_preserves_all_120_order_occurrences_and_skips_both_recorders() {
    let mut input = RushCase::new(2400, 2, 3);
    let active = input.deck[0].clone();
    let mut performers = vec![active.clone()];
    performers.extend((11..=14).map(|character_id| Performer { character_id, ..Default::default() }));
    let mut reference = Vec::new();
    for position in 0..5 {
        input.deck = performers[1..].to_vec();
        input.deck.insert(position, active.clone());
        reference.push(input.ready(None));
    }
    fn orders(prefix: &mut Vec<usize>, used: u8, output: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            output.push(prefix.as_slice().try_into().unwrap());
        } else {
            for position in 0..5 {
                if used & (1 << position) == 0 {
                    prefix.push(position);
                    orders(prefix, used | (1 << position), output);
                    prefix.pop();
                }
            }
        }
    }
    let mut permutations = Vec::new();
    orders(&mut Vec::new(), 0, &mut permutations);
    assert_eq!(permutations.len(), 120);
    let mut cache = LuckDpCache::new(8 << 20);
    for order in permutations {
        input.deck = order.iter().map(|&index| performers[index].clone()).collect();
        let active_position = order.iter().position(|&index| index == 0).unwrap();
        assert_same_terminal(&input, &input.ready(Some(&mut cache)), &reference[active_position]);
    }
    let first = cache.stats();
    assert_eq!(first.terminal_lookups, 120);
    assert_eq!(first.terminal_hits, 115);
    assert_eq!(first.terminal_builds, 5);
    assert_eq!(first.program_compilations, 0);
    let actual = input.ready(Some(&mut cache));
    let second = cache.stats();
    assert_eq!(second.terminal_hits, first.terminal_hits + 1);
    assert_eq!(second.terminal_builds, first.terminal_builds);
    assert_eq!(second.recording_lookups, first.recording_lookups);
    assert_eq!(second.shared_recording_lookups, first.shared_recording_lookups);
    assert_eq!(second.life_recording_lookups, first.life_recording_lookups);
    assert_eq!(second.lookups, first.lookups);
    assert_eq!(second.propagated_curves, first.propagated_curves);

    // Complete native nominal branches provide an independent score/life oracle for the reused result.
    let summary = actual.terminal_summary(i64::from(input.params.total_power)).unwrap();
    let mean = native_branches(&input).iter().fold(Fraction::ZERO, |mean, branch| {
        assert!(summary.final_support.lower <= branch.total_score && branch.total_score <= summary.final_support.upper);
        assert_eq!(summary.exact_final_life, Some(branch.final_life));
        mean.plus(branch.mass.times(branch.total_score as u128, 1))
    });
    assert!(mean.at_least(summary.final_mean.lower) && mean.at_most(summary.final_mean.upper));
}

#[test]
fn terminal_cache_keeps_exact_power_lottery_tables_and_every_declared_scope_input() {
    let mut base = RushCase::new(2400, 2, 3);
    // Equal chart times admit either judgement order in the declared DP scheduling domain.
    base.notes[1].time_ms = base.notes[0].time_ms;
    base.play
        .frames
        .iter_mut()
        .flat_map(|frame| &mut frame.judged)
        .find(|note| note.note_id == base.notes[1].note_id)
        .unwrap()
        .judgement_time_ms = base.notes[1].time_ms;
    let mut cache = LuckDpCache::new(8 << 20);
    base.ready(Some(&mut cache));
    for change in 0..12 {
        let mut input = base.clone();
        match change {
            0 => input.params.total_power += 1,
            1 => input.master.gekisou_luck_bonus_lots[1].weight += 1,
            2 => input.master.gekisou_luck_base_points[0].base_point += 1,
            3 => input.delta[0] = f32::from_bits(input.delta[0].to_bits() + 1),
            4 => input.play.base_seed = 19,
            5 => input.events[0].1 += 1,
            6 => input.play.frames.iter_mut().find(|frame| frame.judged.len() == 2).unwrap().judged.reverse(),
            7 => {
                // Keep the changed chart time in the same first play frame and keep its native time equal.
                input.notes[0].time_ms -= 1;
                input
                    .play
                    .frames
                    .iter_mut()
                    .flat_map(|frame| &mut frame.judged)
                    .find(|note| note.note_id == input.notes[0].note_id)
                    .unwrap()
                    .judgement_time_ms = input.notes[0].time_ms;
            }
            8 => input.setup.fevers[0].0 += 1,
            9 => input.params.assist_factor = 0.75,
            10 => input.master.note_parameters[0].score_percent += 1,
            11 => {
                input
                    .master
                    .live_settings
                    .iter_mut()
                    .find(|row| row.key == "gekisou_luck_rush_score_bonus_percent")
                    .unwrap()
                    .value = "48".into();
            }
            _ => unreachable!(),
        }
        input.master.reindex().unwrap();
        let before = cache.stats();
        let actual = input.ready(Some(&mut cache));
        assert_eq!(cache.stats().terminal_hits, before.terminal_hits, "changed input {change} reused a capability");
        if change == 0 {
            assert_eq!(cache.stats().terminal_recipe_hits, before.terminal_recipe_hits + 1);
            assert_eq!(cache.stats().terminal_builds, before.terminal_builds);
        } else {
            assert_eq!(cache.stats().terminal_recipe_hits, before.terminal_recipe_hits);
            assert_eq!(cache.stats().terminal_builds, before.terminal_builds + 1);
        }
        assert_same_terminal(&input, &actual, &input.ready(None));
        let completed = cache.stats();
        assert_same_terminal(&input, &input.ready(Some(&mut cache)), &actual);
        assert_eq!(cache.stats().terminal_hits, completed.terminal_hits + 1);
        assert_eq!(cache.stats().terminal_builds, completed.terminal_builds);
    }
}

#[test]
fn terminal_cache_cancels_before_every_hit_return_and_never_commits_partial_preparations() {
    let input = RushCase::new(2400, 2, 3);
    let mut cache = LuckDpCache::new(8 << 20);
    let mut cold_checks = 0;
    let LuckRushPreparation::Ready(reference) = input.prepare(Some(&mut cache), || {
        cold_checks += 1;
        false
    }) else {
        panic!("uncancelled preparation must finish")
    };
    let mut hit_checks = 0;
    assert!(matches!(
        input.prepare(Some(&mut cache), || {
            hit_checks += 1;
            false
        }),
        LuckRushPreparation::Ready(_)
    ));
    assert!(cold_checks > hit_checks && hit_checks > 2);
    let complete = cache.stats();
    for stop in 1..=hit_checks {
        let mut seen = 0;
        assert!(matches!(
            input.prepare(Some(&mut cache), || {
                seen += 1;
                seen >= stop
            }),
            LuckRushPreparation::Stopped
        ));
        assert_eq!(cache.stats().terminal_builds, complete.terminal_builds);
        assert_eq!(cache.stats().program_peak_bytes, complete.program_peak_bytes);
    }
    for stop in [1, cold_checks / 2, cold_checks - 1, cold_checks] {
        let mut cold = LuckDpCache::new(8 << 20);
        let mut seen = 0;
        assert!(matches!(
            input.prepare(Some(&mut cold), || {
                seen += 1;
                seen >= stop
            }),
            LuckRushPreparation::Stopped
        ));
        assert_eq!(cold.stats().terminal_builds, 0);
        assert_eq!(cold.stats().program_peak_entries, 0);
        assert_same_terminal(&input, &input.ready(Some(&mut cold)), &reference);
        assert_eq!(cold.stats().terminal_hits, 0);
    }
    assert_same_terminal(&input, &input.ready(Some(&mut cache)), &reference);
}

#[test]
fn terminal_cache_preserves_optional_kernel_refusals_and_never_caches_unavailable() {
    let mut input = four_bucket_case(2400, 2, false);
    input.master.live_skill_effects[0].skill_effect_type = 2002;
    input.master.reindex().unwrap();
    let mut cache = LuckDpCache::new(8 << 20);
    let first = input.ready(Some(&mut cache));
    assert!(first.terminal_summary(i64::from(input.params.total_power)).is_none());
    assert_same_terminal(&input, &input.ready(Some(&mut cache)), &first);
    assert_eq!(cache.stats().terminal_hits, 1);
    assert_eq!(cache.stats().terminal_builds, 1);

    let complete = cache.stats();
    input.params.assist_factor = -1.0;
    for _ in 0..2 {
        assert!(matches!(
            input.prepare(Some(&mut cache), || false),
            LuckRushPreparation::Unavailable { reason: LuckRushDecline::ScoreArithmetic, .. }
        ));
    }
    assert_eq!(cache.stats().terminal_hits, complete.terminal_hits);
    assert_eq!(cache.stats().terminal_builds, complete.terminal_builds);
    assert_eq!(cache.stats().program_peak_entries, complete.program_peak_entries);
}

#[test]
fn terminal_and_replay_programs_share_the_existing_byte_budget_and_zero_capacity() {
    let base = RushCase::new(2400, 2, 3);
    let mut probe = LuckDpCache::new(8 << 20);
    base.ready(Some(&mut probe));
    replay_summary(&base, Some(&mut probe));
    let measured = probe.stats();
    assert_eq!(measured.terminal_builds, 1);
    assert_eq!(measured.program_compilations, 1);
    assert_eq!(measured.program_peak_entries, 2);
    let capacity = measured.program_peak_bytes + measured.program_peak_bytes / 4;
    let mut bounded = LuckDpCache::new(capacity);
    for assist_factor in [1.0, 0.75, 0.5, 0.25, 1.0] {
        let mut input = base.clone();
        input.params.assist_factor = assist_factor;
        assert_same_terminal(&input, &input.ready(Some(&mut bounded)), &input.ready(None));
        let actual = replay_summary(&input, Some(&mut bounded));
        let expected = replay_summary(&input, None);
        assert_eq!(actual.final_mean.lower.to_bits(), expected.final_mean.lower.to_bits());
        assert_eq!(actual.final_mean.upper.to_bits(), expected.final_mean.upper.to_bits());
        assert_eq!(actual.final_support.lower, expected.final_support.lower);
        assert_eq!(actual.final_support.upper, expected.final_support.upper);
        assert_eq!(actual.exact_final_life, expected.exact_final_life);
        assert!(bounded.stats().program_peak_bytes <= capacity);
        assert!(bounded.stats().program_peak_entries <= 128);
    }
    assert!(bounded.stats().program_evictions > 0);
    for capacity in [0, 1, 8] {
        let mut disabled = LuckDpCache::new(capacity);
        for _ in 0..2 {
            assert_same_terminal(&base, &base.ready(Some(&mut disabled)), &base.ready(None));
        }
        let stats = disabled.stats();
        assert_eq!(stats.terminal_builds, 2);
        assert_eq!(stats.terminal_hits, 0);
        assert_eq!(stats.program_peak_entries, 0);
        assert_eq!(stats.program_peak_bytes, 0);
        if capacity < 8 {
            assert_eq!(stats.terminal_lookups, 0);
        }
    }
}
