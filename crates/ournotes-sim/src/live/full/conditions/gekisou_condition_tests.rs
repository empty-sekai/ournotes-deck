use super::*;

#[test]
fn interval_counts_all_crossings_without_replaying_same_count() {
    let mut old = 0;
    for (value, expected) in [(Some(2), 0), (Some(11), 3), (Some(11), 0), (Some(14), 1)] {
        assert_eq!(gk_interval(&mut old, 3, value), expected);
    }
}

#[test]
fn interval_preserves_gap_but_rebases_decrease_and_nonpositive_count() {
    let mut old = 12;
    assert_eq!(gk_interval(&mut old, 3, None), 0);
    assert_eq!(old, 12);
    assert_eq!(gk_interval(&mut old, 3, Some(5)), 0);
    assert_eq!(gk_interval(&mut old, 3, Some(6)), 1);
    assert_eq!(gk_interval(&mut old, 3, Some(-2)), 0);
    assert_eq!(old, 0);
    assert_eq!(gk_interval(&mut old, 0, Some(9)), 0);
    assert_eq!(old, 0);
}

#[test]
fn edge_rearms_after_falling_or_switching_range_and_on_gap() {
    let (mut old, mut range) = (0, -1);
    for (input, expected) in [
        (Some((0, 5)), true),
        (Some((0, 8)), false),
        (Some((0, 4)), false),
        (Some((0, 5)), true),
        (Some((1, 5)), true),
        (None, false),
        (Some((1, 5)), true),
    ] {
        assert_eq!(gk_just_edge(&mut old, &mut range, 5, input), expected);
    }
    assert!(!gk_just_edge(&mut old, &mut range, 0, None));
    assert_eq!((old, range), (5, 1));
}

#[test]
fn once_latches_only_eligible_events_and_can_reset() {
    let mut triggered = false;
    assert!(!gk_once(&mut triggered, false));
    assert!(gk_once(&mut triggered, true));
    assert!(!gk_once(&mut triggered, true));
    triggered = false;
    assert!(gk_once(&mut triggered, true));
}
