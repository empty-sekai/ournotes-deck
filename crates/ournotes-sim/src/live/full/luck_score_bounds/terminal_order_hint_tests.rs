use super::*;

fn order_labels() -> Vec<[usize; 5]> {
    fn visit(prefix: &mut Vec<usize>, used: u8, labels: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            labels.push(prefix.as_slice().try_into().unwrap());
            return;
        }
        for slot in 0..5 {
            if used & (1 << slot) == 0 {
                prefix.push(slot);
                visit(prefix, used | (1 << slot), labels);
                prefix.pop();
            }
        }
    }
    let mut labels = Vec::new();
    visit(&mut Vec::new(), 0, &mut labels);
    assert_eq!(labels.len(), 120);
    labels
}

fn performers(input: &RushCase) -> [Performer; 5] {
    std::array::from_fn(|slot| {
        if slot == 0 {
            input.deck[0].clone()
        } else {
            Performer { character_id: 100 + slot as i64, ..Default::default() }
        }
    })
}

#[test]
fn terminal_order_hint_stable_partition_keeps_all_120_labels_and_reuses_after_power_and_basis_change() {
    let mut input = RushCase::new(2400, 2, 3);
    let original = performers(&input);
    let labels = order_labels();
    // Distinct native positions retain three different complete recipes. Every performer includes its
    // paired skills; the new physical basis below only relabels these same five complete inputs.
    let warm: Vec<_> =
        [1, 3, 4].map(|position| *labels.iter().find(|order| order[position] == 0).unwrap()).into_iter().collect();
    let mut cache = LuckDpCache::new(8 << 20);
    for order in &warm {
        input.deck = order.iter().map(|&slot| original[slot].clone()).collect();
        input.ready(Some(&mut cache));
    }
    let before = cache.stats();
    assert_eq!(before.terminal_recipe_builds, 3);
    let basis = [2, 4, 1, 0, 3];
    let rebased = basis.map(|slot| original[slot].clone());
    let mut inverse = [0; 5];
    for (new, old) in basis.into_iter().enumerate() {
        inverse[old] = new;
    }
    let resident: Vec<_> = warm
        .iter()
        .map(|order| labels.iter().position(|label| *label == order.map(|slot| inverse[slot])).unwrap())
        .collect();
    // Stand in for an existing cap-priority schedule; both partitions must retain this exact order.
    let original_schedule: Vec<_> = (0..120).rev().collect();
    let mut schedule = original_schedule.clone();
    let bytes = cache.programs.test_workspace_accounting().0;
    assert_eq!(cache.prioritize_terminal_orders(&rebased, &labels, &mut schedule), 3);
    let expected: Vec<_> = original_schedule
        .iter()
        .copied()
        .filter(|index| resident.contains(index))
        .chain(original_schedule.iter().copied().filter(|index| !resident.contains(index)))
        .collect();
    assert_eq!(schedule, expected);
    let mut all_labels = schedule.clone();
    all_labels.sort_unstable();
    assert_eq!(all_labels, (0..120).collect::<Vec<_>>());
    assert_eq!(cache.programs.test_workspace_accounting().0, bytes);
    input.params.total_power += 1;
    for &index in &schedule[..3] {
        input.deck = labels[index].iter().map(|&slot| rebased[slot].clone()).collect();
        let actual = input.ready(Some(&mut cache));
        assert_same_terminal(&input, &actual, &input.ready(None));
    }
    assert_eq!(cache.stats().terminal_recipe_hits, before.terminal_recipe_hits + 3);
    assert_eq!(cache.stats().terminal_builds, before.terminal_builds);
    assert_eq!(cache.stats().terminal_recipe_builds, before.terminal_recipe_builds);
}

#[test]
fn terminal_order_hint_collision_changes_priority_but_never_authorizes_identity_or_scope_reuse() {
    let mut input = RushCase::new(2400, 2, 3);
    let performers = performers(&input);
    let labels = order_labels();
    let resident = 96;
    input.deck = labels[resident].iter().map(|&slot| performers[slot].clone()).collect();
    let mut cache = LuckDpCache::new(8 << 20);
    input.ready(Some(&mut cache));
    // These distinct full proof keys intentionally have equal hints: hints omit the model and scope.
    // An actual hash collision between unequal performer vectors has the same scheduling-only effect.
    for change_scope in [false, true] {
        if change_scope {
            input.play.base_seed += 1;
        } else {
            input.params.assist_factor = 0.75;
        }
        let before = cache.stats();
        let mut schedule: Vec<_> = (0..120).collect();
        assert_eq!(cache.prioritize_terminal_orders(&performers, &labels, &mut schedule), 1);
        assert_eq!(schedule[0], resident);
        let actual = input.ready(Some(&mut cache));
        assert_same_terminal(&input, &actual, &input.ready(None));
        assert_eq!(cache.stats().terminal_hits, before.terminal_hits);
        assert_eq!(cache.stats().terminal_recipe_hits, before.terminal_recipe_hits);
        assert_eq!(cache.stats().terminal_builds, before.terminal_builds + 1);
    }
}

#[test]
fn terminal_order_hint_obeys_original_byte_boundary_zero_capacity_and_cancellation() {
    let mut input = RushCase::new(2400, 2, 3);
    let performers = performers(&input);
    let labels = order_labels();
    input.deck = labels[96].iter().map(|&slot| performers[slot].clone()).collect();
    let original: Vec<_> = (0..120).collect();
    let mut cache = LuckDpCache::new(8 << 20);
    input.ready(Some(&mut cache));
    let (bytes, _) = cache.programs.test_workspace_accounting();
    assert_eq!(bytes, cache.stats().program_peak_bytes);
    cache.programs.limit(bytes);
    let mut schedule = original.clone();
    assert_eq!(cache.prioritize_terminal_orders(&performers, &labels, &mut schedule), 1);
    assert_eq!(schedule[0], 96);
    assert_eq!(cache.programs.test_workspace_accounting().0, bytes);
    // The fixed hint lives in the measured Entry allocation, including spare queue slots. Dropping
    // below the full measured byte charge evicts the entry and its hint together, without another table.
    cache.programs.limit(bytes - 1);
    schedule.clone_from(&original);
    assert_eq!(cache.prioritize_terminal_orders(&performers, &labels, &mut schedule), 0);
    assert_eq!(schedule, original);
    assert_eq!(cache.programs.test_workspace_accounting().0, 0);
    let mut zero = LuckDpCache::new(0);
    input.ready(Some(&mut zero));
    assert_eq!(zero.prioritize_terminal_orders(&performers, &labels, &mut schedule), 0);
    assert_eq!(schedule, original);
    assert_eq!(zero.stats().program_peak_bytes, 0);
    let mut cancelled = LuckDpCache::new(8 << 20);
    let mut polls = 0;
    assert!(matches!(
        input.prepare(Some(&mut cancelled), || {
            polls += 1;
            polls >= 8
        }),
        LuckRushPreparation::Stopped
    ));
    assert_eq!(cancelled.prioritize_terminal_orders(&performers, &labels, &mut schedule), 0);
    assert_eq!(schedule, original);
    assert_eq!(cancelled.stats().program_peak_bytes, 0);
}

#[test]
fn terminal_order_hint_invalid_schedule_is_left_untouched() {
    let mut input = RushCase::new(2400, 2, 3);
    let performers = performers(&input);
    let labels = order_labels();
    input.deck = labels[96].iter().map(|&slot| performers[slot].clone()).collect();
    let mut cache = LuckDpCache::new(8 << 20);
    input.ready(Some(&mut cache));
    for mut schedule in [(0..119).collect::<Vec<_>>(), vec![96; 120], (1..121).collect::<Vec<_>>()] {
        let before = schedule.clone();
        assert_eq!(cache.prioritize_terminal_orders(&performers, &labels, &mut schedule), 0);
        assert_eq!(schedule, before);
    }
}
