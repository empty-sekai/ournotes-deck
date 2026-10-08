//! The raw-first schedule must retain all 120 labelled caps and the complete fine-pass result.
use super::*;
use crate::search::uniform::{ORDERS, all_orders, positions_of};
use ournotes_sim::master::SkillTargetRow;

const POWER: i64 = 12_000;

fn fixture(conversion: bool) -> (JointBounds, CandidateDomain, PhysicalDeck, Vec<[usize; 5]>) {
    let (mut master, owned, request) = reward_family_fixture();
    if conversion {
        // A real reachable Perfect -> Great conversion defeats the raw-judgement envelope while the
        // per-note fine envelope remains available. Do not simulate unavailability with a missing fine bound.
        master.skill_targets.push(SkillTargetRow {
            id: 9951,
            skill_target_type: 4,
            judgement: 5,
            ..Default::default()
        });
        let mut converter = master.live_skill_effects[0].clone();
        converter.id = 9952;
        converter.skill_effect_type = 12006;
        converter.skill_target_ids = vec![9951];
        converter.effect_value = 4;
        converter.activation_time_second = 1.0;
        master.live_skill_effects.push(converter);
        master.reindex().unwrap();
    }
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    assert!(bounds.has_fine());
    let physical = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [None; 5] };
    let orders: Vec<_> = all_orders().iter().map(positions_of).collect();
    assert_eq!(orders.len(), ORDERS);
    (bounds, domain, physical, orders)
}

fn aggregate(caps: &[i128], best_order: bool) -> i128 {
    if best_order { caps.iter().max().copied().unwrap() * ORDERS as i128 } else { caps.iter().sum() }
}

#[test]
fn raw_pass_complete_caps_match_original_ordered_tightening_and_keep_existing_caps() {
    let (mut bounds, domain, physical, orders) = fixture(false);
    let mut complete = vec![i128::MAX / ORDERS as i128; ORDERS];
    // This unchanged routine is the original raw/fine interleaving, with every order completed.
    bounds.tighten_order_caps(&domain, &physical, POWER, &orders, &mut complete, &mut JointScratch::default());
    for best_order in [false, true] {
        bounds.best_order = best_order;
        for already_tight in [false, true] {
            let initial: Vec<_> = complete
                .iter()
                .enumerate()
                .map(|(index, &cap)| if already_tight && index % 3 == 0 { cap } else { cap + 10_000 })
                .collect();
            let mut expected = initial.clone();
            bounds.tighten_order_caps(&domain, &physical, POWER, &orders, &mut expected, &mut JointScratch::default());
            let mut actual = initial.clone();
            let mut computed = 7;
            assert!(!bounds.tighten_order_caps_until(
                &domain,
                &physical,
                POWER,
                &orders,
                &mut actual,
                &mut JointScratch::default(),
                |_| false,
                &mut computed,
            ));
            assert_eq!(actual, expected);
            assert_eq!(actual, complete);
            assert!(actual.iter().zip(&initial).all(|(cap, original)| cap <= original));
            assert_eq!(computed, 7 + ORDERS as u64);
        }
    }
}

#[test]
fn raw_pass_can_exclude_all_orders_without_any_fine_work() {
    let (mut bounds, domain, physical, orders) = fixture(false);
    let raw: Vec<_> = orders
        .iter()
        .map(|positions| bounds.raw_upper(&domain, &physical, POWER, positions).expect("fixed judgements"))
        .collect();
    for best_order in [false, true] {
        bounds.best_order = best_order;
        let mut caps: Vec<_> = raw.iter().map(|cap| cap + 10_000).collect();
        let cutoff = aggregate(&raw, best_order) + 1;
        assert!(aggregate(&caps, best_order) >= cutoff, "the initial bound cannot exclude this team");
        let mut computed = 0;
        assert!(bounds.tighten_order_caps_until(
            &domain,
            &physical,
            POWER,
            &orders,
            &mut caps,
            &mut JointScratch::default(),
            |cap| cap < cutoff,
            &mut computed,
        ));
        assert_eq!(caps, raw, "every original label receives its own raw cap before the cutoff");
        assert_eq!(computed, 0);
    }
}

#[test]
fn raw_pass_unavailable_raw_keeps_the_original_fine_fallback() {
    let (mut bounds, domain, physical, orders) = fixture(true);
    assert!(orders.iter().all(|positions| bounds.raw_upper(&domain, &physical, POWER, positions).is_none()));
    for best_order in [false, true] {
        bounds.best_order = best_order;
        let mut expected = vec![i128::MAX / ORDERS as i128; ORDERS];
        let mut actual = expected.clone();
        bounds.tighten_order_caps(&domain, &physical, POWER, &orders, &mut expected, &mut JointScratch::default());
        let mut computed = 0;
        assert!(!bounds.tighten_order_caps_until(
            &domain,
            &physical,
            POWER,
            &orders,
            &mut actual,
            &mut JointScratch::default(),
            |_| false,
            &mut computed,
        ));
        assert_eq!(actual, expected);
        assert_eq!(computed, ORDERS as u64);
    }
}

#[test]
fn raw_pass_checked_sum_overflow_keeps_complete_tightening() {
    let (bounds, domain, physical, orders) = fixture(false);
    let mut actual = vec![i128::MAX; ORDERS];
    let mut expected = actual.clone();
    bounds.tighten_order_caps(&domain, &physical, POWER, &orders, &mut expected, &mut JointScratch::default());
    let mut computed = 0;
    assert!(!bounds.tighten_order_caps_until(
        &domain,
        &physical,
        POWER,
        &orders,
        &mut actual,
        &mut JointScratch::default(),
        |_| false,
        &mut computed,
    ));
    assert_eq!(actual, expected);
    assert_eq!(computed, ORDERS as u64);
}

#[test]
fn raw_pass_respects_strict_ties_and_the_supplied_power_tie_decision() {
    let (mut bounds, domain, physical, orders) = fixture(false);
    let mut complete = vec![i128::MAX / ORDERS as i128; ORDERS];
    bounds.tighten_order_caps(&domain, &physical, POWER, &orders, &mut complete, &mut JointScratch::default());
    for best_order in [false, true] {
        bounds.best_order = best_order;
        let cutoff = aggregate(&complete, best_order);
        for lower_power in [false, true] {
            let mut caps = complete.clone();
            let mut computed = 0;
            let excluded = bounds.tighten_order_caps_until(
                &domain,
                &physical,
                POWER,
                &orders,
                &mut caps,
                &mut JointScratch::default(),
                |cap| cap < cutoff || (cap == cutoff && lower_power),
                &mut computed,
            );
            assert_eq!(excluded, lower_power, "equal score and equal power must preserve the canonical tie");
            assert_eq!(caps, complete);
            assert_eq!(computed, if lower_power { 0 } else { ORDERS as u64 });
        }
    }
}
