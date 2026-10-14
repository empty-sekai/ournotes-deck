use super::*;

#[test]
fn settled_cap_is_recomputed_only_when_the_settled_point_changes() {
    let a = Settled { frame: 7, total: 19, fixed: 3 };
    let history = [
        a,
        a,
        Settled { total: 23, ..a },
        Settled { fixed: 11, ..a },
        Settled { frame: 8, ..a },
        a,
        a,
        Settled { frame: 0, total: -9, fixed: -2 },
    ];
    let mut entries = [None; ORDERS];
    let mut computed = 0;
    for point in history {
        for (order, entry) in entries.iter_mut().enumerate() {
            // Exhaustive finite continuations give an independent upper endpoint for each prefix.
            let outcomes: Vec<i128> = (-3..=4)
                .map(|continuation| {
                    i128::from(point.total)
                        + i128::from(point.fixed) * continuation
                        + i128::from(point.frame) * continuation * continuation
                        + order as i128 * (continuation - 1)
                })
                .collect();
            let expected = *outcomes.iter().max().unwrap();
            let actual = settled_cap(entry, point, || {
                computed += 1;
                outcomes.into_iter().max().unwrap()
            });
            assert_eq!(actual, expected);
        }
    }
    assert_eq!(computed, 6 * ORDERS);
    let mut next_leaf = None;
    assert_eq!(settled_cap(&mut next_leaf, a, || -37), -37);
    assert_eq!(settled_cap(&mut next_leaf, a, || panic!("unchanged prefix")), -37);
}

#[test]
fn settled_cap_keeps_integer_extremes() {
    let point = Settled { frame: i32::MAX, total: i64::MIN, fixed: i64::MAX };
    for ceiling in [i128::MIN, -1, 0, i128::MAX] {
        let mut previous = None;
        let mut attempts = 0;
        for _ in 0..2 {
            let cap = settled_cap(&mut previous, point, || {
                attempts += 1;
                ceiling
            });
            assert_eq!(cap, ceiling);
        }
        assert_eq!(attempts, 1);
    }
}

#[test]
fn a_cap_below_the_threshold_excludes_the_team_only_when_every_order_is_below() {
    assert!(maximum_cap_below(&[3, 5, 4], 6));
    assert!(!maximum_cap_below(&[3, 6, 4], 6));
    assert!(!maximum_cap_below(&[], 6));
}

#[test]
fn tightening_keeps_the_lower_cap_and_stops_at_the_first_break() {
    let mut caps = [10, 20, 30];
    let flow = tighten_maximum_caps(&mut caps, |index, cap| ControlFlow::Continue(cap.min(15 + index as i128)));
    assert!(flow.is_continue());
    assert_eq!(caps, [10, 16, 17]);
    let mut caps = [10, 20, 30];
    let flow = tighten_maximum_caps(&mut caps, |index, cap| {
        if index == 1 { ControlFlow::Break(()) } else { ControlFlow::Continue(cap - 1) }
    });
    assert!(flow.is_break());
    assert_eq!(caps, [9, 20, 30]);
}
