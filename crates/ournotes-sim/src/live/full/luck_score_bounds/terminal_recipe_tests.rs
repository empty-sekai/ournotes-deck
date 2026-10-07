use super::*;

fn assert_native_expectation(input: &RushCase, terminal: &LuckTerminalRush) {
    let summary = terminal.terminal_summary(i64::from(input.params.total_power)).unwrap();
    let mean = native_branches(input).iter().fold(Fraction::ZERO, |mean, branch| {
        assert!(summary.final_support.lower <= branch.total_score && branch.total_score <= summary.final_support.upper);
        assert_eq!(summary.exact_final_life, Some(branch.final_life));
        mean.plus(branch.mass.times(branch.total_score as u128, 1))
    });
    assert!(mean.at_least(summary.final_mean.lower) && mean.at_most(summary.final_mean.upper));
}

#[test]
fn terminal_recipe_recomputes_native_floors_at_every_power_and_reuses_one_history_entry() {
    let mut input = four_bucket_case(2400, 2, false);
    let mut cache = LuckDpCache::new(8 << 20);
    let mut scores = Vec::new();
    for (index, power) in [1, 999, 1000, 1001, 12345, 999].into_iter().enumerate() {
        input.params.total_power = power;
        let actual = input.ready(Some(&mut cache));
        assert_same_terminal(&input, &actual, &input.ready(None));
        let summary = actual.terminal_summary(i64::from(power)).unwrap();
        scores.push(summary.final_mean.lower.to_bits());
        assert!(actual.terminal_summary(i64::from(power) + 1).is_none());
        if index == 1 || index == 3 {
            assert_native_expectation(&input, &actual);
        }
        let stats = cache.stats();
        assert_eq!(stats.terminal_builds, 1);
        assert_eq!(stats.terminal_recipe_builds, 1);
        assert_eq!(stats.terminal_recipe_hits, index as u64);
        assert_eq!(stats.terminal_hits, 0);
        assert_eq!(stats.program_peak_entries, 1);
        assert!(stats.program_peak_bytes <= 8 << 20);
    }
    assert_ne!(scores[0], scores[1]);
    assert_ne!(scores[1], scores[3]);
    assert_eq!(scores[1], scores[5]);
    // Only the most recent exact-P result is retained beside the power-independent recipe.
    let before = cache.stats();
    assert_same_terminal(&input, &input.ready(Some(&mut cache)), &input.ready(None));
    assert_eq!(cache.stats().terminal_hits, before.terminal_hits + 1);
    assert_eq!(cache.stats().terminal_recipe_hits, before.terminal_recipe_hits);
}

#[test]
fn terminal_recipe_numeric_refusal_at_old_power_never_poisoned_new_power_or_keeps_old_caps() {
    let mut input = four_bucket_case(2400, 2, false);
    let mut cache = LuckDpCache::new(8 << 20);
    for (index, power) in [-1, 1001, -1, 999].into_iter().enumerate() {
        input.params.total_power = power;
        let actual = input.ready(Some(&mut cache));
        assert_same_terminal(&input, &actual, &input.ready(None));
        assert_eq!(actual.terminal_summary(i64::from(power)).is_some(), power >= 0);
        assert!(actual.terminal_summary(if power == 1001 { 999 } else { 1001 }).is_none());
        assert_eq!(cache.stats().terminal_builds, 1);
        assert_eq!(cache.stats().terminal_recipe_hits, index as u64);
        assert_eq!(cache.stats().program_peak_entries, 1);
        if power < 0 {
            let times: Vec<_> = input.notes.iter().map(|note| note.time_ms).collect();
            assert!(actual.native_note_bucket_caps(i64::from(power), &times).is_none());
            let before = cache.stats();
            let again = input.ready(Some(&mut cache));
            assert_same_terminal(&input, &again, &actual);
            assert_eq!(cache.stats().terminal_hits, before.terminal_hits + 1);
        }
    }
}

#[test]
fn terminal_recipe_keeps_native_source_order_and_controller_writer_identity() {
    let mut base = four_bucket_case(2400, 2, false);
    let mut skill = base.master.gekisou_support_skills.last().unwrap().clone();
    skill.id = 907;
    base.master.gekisou_support_skills.push(skill);
    let mut row = base.master.gekisou_support_skill_effects[0].clone();
    row.id = 908;
    row.skill_id = 907;
    row.effect_value = 6250;
    base.master.gekisou_support_skill_effects.push(row);
    base.deck[0].gekisou_support_skills.push((907, 1));
    base.master.skill_targets.push(
        serde_json::from_value(serde_json::json!({"_id":910,"_skillTargetType":5,"_gekisouMissionType":2})).unwrap(),
    );
    base.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 911,
        condition_type: 7020,
        condition_values: Vec::new(),
        condition_target_ids: vec![910],
        is_positive: true,
    });
    base.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 911,
        group: 911,
        condition_ids: vec![911],
    });
    base.master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 909,
        skill_id: 901,
        level: 1,
        skill_trigger_type: SUSTAINED,
        skill_trigger_condition_group: 911,
        skill_effect_type: 11001,
        effect_value: 1000,
        ..Default::default()
    });
    base.master.reindex().unwrap();
    let mut cache = LuckDpCache::new(8 << 20);
    base.ready(Some(&mut cache));
    assert_eq!(cache.stats().terminal_recipe_builds, 1);
    for change in 0..3 {
        let mut input = base.clone();
        input.params.total_power += change + 1;
        match change {
            0 => input.deck[0].gekisou_support_skills.reverse(),
            1 => input.master.gekisou_skill_effects.last_mut().unwrap().effect_value += 1000,
            2 => input.deck.swap(0, 1),
            _ => unreachable!(),
        }
        input.master.reindex().unwrap();
        let before = cache.stats();
        let actual = input.ready(Some(&mut cache));
        assert_same_terminal(&input, &actual, &input.ready(None));
        assert_eq!(cache.stats().terminal_hits, before.terminal_hits, "changed native input {change}");
        assert_eq!(cache.stats().terminal_recipe_hits, before.terminal_recipe_hits, "changed native input {change}");
        assert_eq!(cache.stats().terminal_recipe_builds, before.terminal_recipe_builds + 1);
    }
}

#[test]
fn terminal_recipe_power_command_refuses_recipe_without_suppressing_native_fallback() {
    let mut input = four_bucket_case(2400, 2, false);
    input.master.live_skill_effects[0].skill_effect_type = 2002;
    input.master.reindex().unwrap();
    let mut cache = LuckDpCache::new(8 << 20);
    for (index, power) in [999, 1000].into_iter().enumerate() {
        input.params.total_power = power;
        let actual = input.ready(Some(&mut cache));
        assert_same_terminal(&input, &actual, &input.ready(None));
        assert_eq!(cache.stats().terminal_builds, index as u64 + 1);
        assert_eq!(cache.stats().terminal_recipe_builds, 0);
        assert_eq!(cache.stats().terminal_recipe_hits, 0);
    }
}

#[test]
fn terminal_recipe_zero_capacity_and_shared_budget_only_change_recomputation() {
    let mut input = four_bucket_case(2400, 2, false);
    for capacity in [0, 1, 8] {
        let mut cache = LuckDpCache::new(capacity);
        for power in [999, 1000, 999] {
            input.params.total_power = power;
            assert_same_terminal(&input, &input.ready(Some(&mut cache)), &input.ready(None));
        }
        let stats = cache.stats();
        assert_eq!(stats.terminal_builds, 3);
        assert_eq!(stats.terminal_recipe_hits, 0);
        assert_eq!(stats.program_peak_entries, 0);
        assert_eq!(stats.program_peak_bytes, 0);
        if capacity == 0 {
            assert_eq!(stats.terminal_recipe_lookups, 0);
            assert_eq!(stats.terminal_recipe_builds, 0);
        }
    }
    let mut measured = LuckDpCache::new(8 << 20);
    input.ready(Some(&mut measured));
    let capacity = measured.stats().program_peak_bytes;
    let mut cache = LuckDpCache::new(capacity);
    for index in 0..12 {
        input.params.assist_factor = 1.0 - index as f32 / 32.0;
        input.params.total_power += 1;
        assert_same_terminal(&input, &input.ready(Some(&mut cache)), &input.ready(None));
        assert!(cache.stats().program_peak_bytes <= capacity);
        assert!(cache.stats().program_peak_entries <= 128);
    }
    assert!(cache.stats().program_evictions > 0);
}

#[test]
fn terminal_recipe_cancellation_preserves_only_complete_history_and_latest_power_results() {
    let base = four_bucket_case(2400, 2, false);
    let mut next = base.clone();
    next.params.total_power += 1;
    let reference = next.ready(None);
    let mut measured = LuckDpCache::new(8 << 20);
    base.ready(Some(&mut measured));
    let mut checks = 0;
    assert!(matches!(
        next.prepare(Some(&mut measured), || {
            checks += 1;
            false
        }),
        LuckRushPreparation::Ready(_)
    ));
    assert!(checks > 4);
    assert_eq!(measured.stats().terminal_recipe_hits, 1);
    for stop in [1, checks / 4, checks / 2, checks - 1, checks] {
        let mut cache = LuckDpCache::new(8 << 20);
        let old = base.ready(Some(&mut cache));
        let before = cache.stats();
        let mut seen = 0;
        assert!(matches!(
            next.prepare(Some(&mut cache), || {
                seen += 1;
                seen >= stop
            }),
            LuckRushPreparation::Stopped
        ));
        assert_eq!(cache.stats().terminal_builds, before.terminal_builds);
        assert_eq!(cache.stats().terminal_recipe_builds, before.terminal_recipe_builds);
        assert_eq!(cache.stats().program_peak_entries, before.program_peak_entries);
        assert_eq!(cache.stats().program_peak_bytes, before.program_peak_bytes);
        assert_same_terminal(&base, &base.ready(Some(&mut cache)), &old);
        assert_eq!(cache.stats().terminal_hits, before.terminal_hits + 1);
        let hits = cache.stats().terminal_recipe_hits;
        assert_same_terminal(&next, &next.ready(Some(&mut cache)), &reference);
        assert_eq!(cache.stats().terminal_recipe_hits, hits + 1);
        assert_eq!(cache.stats().terminal_builds, 1);
    }
}
