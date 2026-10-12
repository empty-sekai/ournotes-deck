use super::lottery::{Item, LuckSkillTable, nominal_bonus_weights};
use super::*;

fn machine(items: Vec<Item>) -> LotteryMachine {
    LotteryMachine {
        good: None,
        great: None,
        perfect: Some(vec![(1, 25), (3, 50)]),
        hold: vec![(1, 5)],
        tables: vec![LuckSkillTable::new(items).unwrap()],
        minimum: Vec::new(),
        minimum_counter: 0,
        last_consumed: FxHashMap::default(),
    }
}

#[test]
fn nominal_hooks_count_native_draws_and_consume_minimum_only_after_a_choice() {
    let mut machine = machine(vec![(5, 0), (2, 2), (4, 3)]);
    let mut random = LiveRandom::with_nominal_prefix(vec![0]);
    assert_eq!(machine.base_point(122, 5, &mut random).unwrap(), 0);
    assert_eq!(machine.base_point(1, 1, &mut random).unwrap(), 0);
    assert_eq!(random.draws(), 0);
    assert_eq!(machine.base_point(1, 2, &mut random).unwrap(), 0);
    assert_eq!(random.draws(), 1, "an unhandled judgement still consumes the native base draw");
    assert_eq!(machine.base_point(21, 6, &mut random).unwrap(), 5);
    assert_eq!(random.draws(), 2, "a deterministic table still consumes a draw");
    let minimum = machine.enable_minimum(2, 1);
    assert_eq!(machine.luck_bonus(0, 77, &mut random).unwrap(), 3);
    assert_eq!(random.draws(), 3);
    assert!(!machine.is_minimum_active(minimum));
    assert_eq!(machine.take_last_consumed(minimum), 77);
    assert!(random.nominal_covers_draws() && random.nominal_prefix_consumed());

    let minimum = machine.enable_minimum(2, 1);
    let mut pending = LiveRandom::with_nominal_prefix(Vec::new());
    assert!(machine.luck_bonus(0, 88, &mut pending).is_err());
    assert!(pending.nominal_branch().is_some() && pending.nominal_covers_draws());
    assert!(machine.is_minimum_active(minimum), "a missing choice stops before consuming a guarantee");
    assert_eq!(machine.take_last_consumed(minimum), -1);
}

#[test]
fn buffed_lottery_keeps_the_unbuffed_modulus() {
    let machine = machine(vec![(3, 0), (1, 1), (2, 2), (4, 3)]);
    let probabilities = machine.bonus_probabilities(0, 100, 0).unwrap();
    assert_eq!(probabilities, vec![(0.8, 3), (0.2, 2)]);
    let mut table = machine.tables[0].clone();
    table.buff = 1.0;
    let mut counts = [0; 4];
    for r in 0..table.total_weight {
        counts[table.lottery(r).unwrap() as usize] += 1;
    }
    assert_eq!(counts, [0, 0, 2, 8]);
}

#[test]
fn minimum_shares_excluded_weights_with_integer_remainder_dropped() {
    let machine = machine(vec![(5, 0), (2, 2), (4, 3)]);
    assert_eq!(machine.bonus_probabilities(0, 0, 2).unwrap(), vec![(0.6, 3), (0.4, 2)]);
    let table = &machine.tables[0];
    let mut counts = [0; 4];
    for r in 0..10 {
        counts[table.lottery_with_minimum(r, 2).unwrap() as usize] += 1;
    }
    assert_eq!(counts, [0, 0, 4, 6]);
    assert_eq!(machine.base_point_probabilities(1, 5).unwrap(), vec![(0.25, 25), (0.75, 50)]);
    assert_eq!(machine.base_point_probabilities(21, 6).unwrap(), vec![(1.0, 5)]);
    assert_eq!(machine.base_point_probabilities(122, 5).unwrap(), vec![(1.0, 0)]);
}

#[test]
fn unknown_lottery_results_are_refused_before_state_compression() {
    let machine = machine(vec![(1, 4)]);
    assert!(matches!(machine.bonus_probabilities(0, 0, 0), Err(Error::Unsupported(_))));
}

#[test]
fn integer_masses_match_every_native_modulus_position() {
    for items in [vec![(3, 0), (1, 1), (2, 2), (4, 3)], vec![(2, 0), (3, 2), (1, 3), (4, 3)]] {
        let machine = machine(items);
        for buff in [0, 25, 53, 100, 250] {
            for minimum in 0..=3 {
                let weights = machine.bonus_probability_weights(0, buff, minimum).unwrap();
                let total = weights[0].1;
                assert_eq!(weights.iter().map(|w| w.0).sum::<u64>(), total);
                let mut expected = [0u64; 4];
                for &(weight, denominator, result) in &weights {
                    assert_eq!(denominator, total);
                    expected[result as usize] += weight;
                }
                let mut table = machine.tables[0].clone();
                table.buff = buff as f32 / 100.0;
                assert_eq!(nominal_bonus_weights(&table, minimum).unwrap(), weights);
                let mut actual = [0u64; 4];
                for r in 0..total as i32 {
                    let result = if minimum == 0 { table.lottery(r) } else { table.lottery_with_minimum(r, minimum) };
                    actual[result.unwrap() as usize] += 1;
                }
                assert_eq!(actual, expected, "buff={buff} minimum={minimum}");
            }
        }
    }
}

#[test]
fn integer_base_masses_preserve_duplicate_rows_and_reject_wrapped_totals() {
    let mut machine = machine(vec![(1, 3)]);
    machine.perfect = Some(vec![(1, 25), (3, 50), (2, 25)]);
    assert_eq!(machine.base_point_probability_weights(1, 5).unwrap(), vec![(1, 6, 25), (3, 6, 50), (2, 6, 25)]);
    machine.perfect = Some(vec![(i32::MAX as i64, 25), (i32::MAX as i64, 50), (3, 75)]);
    assert!(matches!(machine.base_point_probability_weights(1, 5), Err(Error::Capacity(_))));
    machine.tables[0].items = vec![(i32::MAX as i64, 3), (i32::MAX as i64, 2), (3, 1)];
    assert!(matches!(machine.bonus_probability_weights(0, 0, 0), Err(Error::Capacity(_))));
}
