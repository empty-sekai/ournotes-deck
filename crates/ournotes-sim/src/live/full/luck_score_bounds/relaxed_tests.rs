//! The relaxed histories against the exact path groups.
use super::exact::{LuckExactProfile, Observers, Stopwatch, evaluate_schedule, order_schedule};
use super::exact_paths::Orders;
use super::exact_tests::{ORDER_ROWS, ROWS, Rng, cases, luck_live, nominal, replay_schedule, schedule};
use super::*;

/// Every value the exact replay gives a note in an order lies within the relaxed replay's value of that order, probe
/// class and combo inputs, in replays of one and of several orders.
#[test]
fn relaxed_values_hold_the_exact_values_of_every_order() {
    let (mut compared, mut wider) = (0, 0);
    for seed in 1..=12u64 {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ seed.wrapping_mul(0x0051_7cc1_b727_220a));
        for lanes in [1, ORDER_ROWS.len()] {
            let steps = schedule(&mut rng, lanes);
            let rows = if lanes == 1 { &[ROWS][..] } else { &ORDER_ROWS[..] };
            let select = |mask: Orders| Some(mask);
            let (exact, index) = replay_schedule(&steps, rows, select, false);
            let (relaxed, relaxed_index) = replay_schedule(&steps, rows, select, true);
            assert_eq!(index, relaxed_index);
            for &at in &index {
                let (exact, relaxed) = (&exact.notes()[at], &relaxed.notes()[at]);
                for order in 0..lanes {
                    for value in exact.values_of(order) {
                        let bound = relaxed
                            .values_of(order)
                            .find(|bound| (bound.class, bound.combo) == (value.class, value.combo))
                            .unwrap_or_else(|| panic!("seed {seed} note {at} order {order}: no relaxed value"));
                        for field in 0..FIELDS {
                            assert!(
                                bound.low[field] <= value.low[field] && value.high[field] <= bound.high[field],
                                "seed {seed} note {at} order {order} field {field}: {value:?} outside {bound:?}"
                            );
                        }
                        compared += 1;
                        wider += usize::from(bound.low != value.low || bound.high != value.high);
                    }
                }
            }
        }
    }
    assert!(compared > 2_000, "{compared}");
    // The relaxed bounds join histories the exact groups keep apart.
    assert!(wider > 0);
}

/// An evaluation with the relaxed histories encloses the exact evaluation and the complete nominal law.
#[test]
fn relaxed_evaluations_enclose_the_exact_ones() {
    for (name, case) in cases() {
        let confirmations: Vec<_> = (0..case.4.fevers.len())
            .map(|range| crate::replay::RankConfirmation { frame: 0, range, rank: 1, percent: 250 })
            .collect();
        for ranking in [None, Some(confirmations.as_slice())] {
            let skills = luck_skills(&case.0).unwrap();
            let live = luck_live(&case, &skills, ranking);
            let order = order_schedule(&live, &case.1, None, None).unwrap();
            let [exact, relaxed] = [false, true].map(|relaxed| {
                let mut scores = evaluate_schedule(
                    &order.schedule,
                    &order.calc,
                    order.rush_percent,
                    std::slice::from_ref(&order.law),
                    live.play,
                    order.probe_phase_bound,
                    live.setup.fevers.len(),
                    ranking.is_none(),
                    None,
                    false,
                    LuckExactProfile::default(),
                    Stopwatch::start(),
                    relaxed,
                    Observers::default(),
                )
                .unwrap();
                scores.pop().unwrap()
            });
            let network = ranking.is_some();
            assert!(relaxed.relaxed && !exact.relaxed);
            let (outer, inner) = (relaxed.final_mean, exact.final_mean);
            assert!(outer.lower <= inner.lower && inner.upper <= outer.upper, "{name} network={network}");
            let (outer, inner) = (relaxed.final_support, exact.final_support);
            assert!(outer.lower <= inner.lower && inner.upper <= outer.upper, "{name} network={network}");
            let (expected, _) = nominal(name, &case, ranking);
            let slack = 1e-9 * expected.abs();
            assert!(
                relaxed.final_mean.lower - slack <= expected && expected <= relaxed.final_mean.upper + slack,
                "{name} network={network}: nominal {expected} outside {:?}",
                relaxed.final_mean
            );
        }
    }
}
