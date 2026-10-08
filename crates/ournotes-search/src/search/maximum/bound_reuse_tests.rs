use super::{ORDERS, Settled, settled_cap};

#[test]
fn settled_cap_reuse_covers_distinct_orders_and_nonmonotone_prefix_visits() {
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
fn settled_cap_reuse_keeps_fallbacks_and_integer_extremes() {
    let point = Settled { frame: i32::MAX, total: i64::MIN, fixed: i64::MAX };
    for ceiling in [i128::MIN, -1, 0, i128::MAX] {
        let mut previous = None;
        let mut attempts = 0;
        for available in [None, Some(ceiling)] {
            let cap = settled_cap(&mut previous, point, || {
                attempts += 1;
                available.map_or(ceiling, |value| ceiling.min(value))
            });
            assert_eq!(cap, ceiling);
        }
        assert_eq!(attempts, 1);
    }
}
