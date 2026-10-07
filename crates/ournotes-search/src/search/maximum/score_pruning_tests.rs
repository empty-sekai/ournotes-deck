use super::score_order_dominated;
use std::cmp::Reverse;

#[test]
fn strict_score_order_bounds_preserve_finite_support_maxima_and_canonical_ties() {
    let values = [i32::MIN, -1, 0, 1, i32::MAX];
    let mut compared = 0;
    let mut pruned = 0;
    let mut retained_equal = 0;
    for a in values {
        for b in values {
            for c in values {
                let supports = [[a, b], [b, c], [c, a]];
                // The oracle reduces every outcome before any bound or incumbent is consulted.
                let oracle = supports
                    .iter()
                    .enumerate()
                    .flat_map(|(order, scores)| scores.iter().map(move |&score| (score, Reverse(order))))
                    .max()
                    .unwrap();
                for slack in [None, Some(0), Some(1)] {
                    let mut best = None;
                    let mut retained = Vec::new();
                    for (order, scores) in supports.iter().enumerate() {
                        let maximum = *scores.iter().max().unwrap();
                        let ceiling = slack.map(|slack| maximum.saturating_add(slack));
                        if score_order_dominated(ceiling, best) {
                            pruned += 1;
                            continue;
                        }
                        retained_equal +=
                            usize::from(ceiling.zip(best).is_some_and(|(upper, best)| i128::from(upper) == best));
                        retained.extend(scores.iter().map(|&score| (score, Reverse(order))));
                        best = Some(best.map_or(i128::from(maximum), |old| old.max(i128::from(maximum))));
                    }
                    assert_eq!(retained.into_iter().max(), Some(oracle));
                    assert_eq!(best, Some(i128::from(oracle.0)));
                    compared += 1;
                }
            }
        }
    }
    assert_eq!(compared, 375);
    assert!(pruned > 0);
    assert!(retained_equal > 0);
}

#[test]
fn score_order_bounds_keep_equal_unknown_and_first_order_cases() {
    for score in [i32::MIN, -1, 0, 1, i32::MAX] {
        assert!(!score_order_dominated(Some(score), None));
        assert!(!score_order_dominated(None, Some(i128::from(score))));
        assert!(!score_order_dominated(Some(score), Some(i128::from(score))));
    }
    assert!(!score_order_dominated(None, None));
    assert!(score_order_dominated(Some(i32::MIN), Some(i128::from(i32::MAX))));
}
