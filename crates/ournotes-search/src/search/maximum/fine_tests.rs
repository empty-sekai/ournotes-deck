use super::{ORDERS, conditional_bases_are_injective, maximum_cap_below, tighten_maximum_caps};
use std::ops::ControlFlow;

#[test]
fn maximum_caps_keep_equal_thresholds_and_every_order() {
    let mut caps = [0; ORDERS];
    caps[ORDERS - 1] = 7;
    assert!(!maximum_cap_below(&caps, 7));
    assert!(maximum_cap_below(&caps, 8));
    for index in 0..ORDERS {
        let mut caps = [-20; ORDERS];
        caps[index] = -5;
        assert!(!maximum_cap_below(&caps, -5));
        assert!(maximum_cap_below(&caps, -4));
    }
    assert!(!maximum_cap_below(&[], 1));
}

#[test]
fn maximum_cap_cutoff_preserves_equal_power_ties() {
    let caps = [37; ORDERS];
    let cutoff = |power: i32, kth_power: i32| 37 + i128::from(power < kth_power);
    assert!(!maximum_cap_below(&caps, cutoff(100, 100)));
    assert!(!maximum_cap_below(&caps, cutoff(101, 100)));
    assert!(maximum_cap_below(&caps, cutoff(99, 100)));
}

#[test]
fn interrupted_cap_refinement_retains_upper_bounds_without_completing() {
    let mut caps = [100; ORDERS];
    let result =
        tighten_maximum_caps(
            &mut caps,
            |index, _| {
                if index == 7 { ControlFlow::Break(()) } else { ControlFlow::Continue(20) }
            },
        );
    assert!(result.is_break());
    assert_eq!(&caps[..7], &[20; 7]);
    assert!(caps[7..].iter().all(|&cap| cap == 100));
    assert!(!maximum_cap_below(&caps, 100));
    assert!(tighten_maximum_caps(&mut caps, |_, _| ControlFlow::Continue(200)).is_continue());
    assert_eq!(&caps[..7], &[20; 7]);
    assert!(caps[7..].iter().all(|&cap| cap == 100));
}

#[test]
fn conditional_identity_admission_covers_every_member_position() {
    assert!(conditional_bases_are_injective([(1, 3), (1, 4), (1, 5), (2, 3)]));
    assert!(!conditional_bases_are_injective([(1, 3), (1, 3)]));
    let wrapped_row = 1_106_804_644_422_573_097i64;
    assert_eq!(wrapped_row.wrapping_mul(100), 4);
    assert!(!conditional_bases_are_injective([(0, 3), (wrapped_row, 3)]));
    assert!(!conditional_bases_are_injective([(i64::MAX, 3)]));
    assert!(!conditional_bases_are_injective([(i64::MIN, 5)]));
    for left_source in 3..=5 {
        for right_source in 3..=5 {
            for left_slot in 0..5 {
                for right_slot in 0..5 {
                    let left = 100 + 10 * left_source + left_slot;
                    let right = 100 + 10 * right_source + right_slot;
                    assert_eq!(left == right, left_source == right_source && left_slot == right_slot);
                }
            }
        }
    }
}
