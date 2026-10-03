//! Correlated power/coefficient caps retaining the unique Snap assignment.
use super::add_up;

/// Choice zero is an optional empty slot; positive choices share a resource.
/// Each row stores an upper coefficient and exact nonnegative slot power.
/// Returns infinity if the optional numerical relaxation is unavailable.
pub(super) fn product_upper(
    rows: &[Vec<(i64, f64)>; 5],
    allowed: &[Vec<bool>; 5],
    none: [bool; 5],
    base: f64,
    margin: f64,
    scales: [f64; 3],
) -> i128 {
    // Keep the assignment solver's lexicographic i128 weights far from overflow.
    // A skipped optional cap never removes a resource or a candidate.
    if rows[0].len() > 4097 {
        return i128::MAX;
    }
    let mut best = i128::MAX;
    for r in scales {
        if !r.is_finite() || !(1e-100..=1e100).contains(&r) {
            continue;
        }
        // Round every absolute edge upward before taking integer differences.
        // The row shifts cancel exactly, including when None is disallowed.
        let mut shift = [0i64; 5];
        let mut weights: [Vec<i64>; 5] = std::array::from_fn(|_| Vec::new());
        let mut valid = true;
        for slot in 0..5 {
            for (choice, &(power, gain)) in rows[slot].iter().enumerate() {
                let q = add_up(power as f64, (r * gain).next_up()).ceil();
                // Five shifted rows still sum exactly in f64 and safely in i64.
                if power < 0 || gain < 0.0 || !q.is_finite() || !(0.0..=1e15).contains(&q) {
                    valid = false;
                    break;
                }
                if choice == 0 {
                    shift[slot] = q as i64;
                } else {
                    weights[slot].push(q as i64 - shift[slot]);
                }
            }
            if !valid {
                break;
            }
        }
        if !valid {
            continue;
        }
        let Some((increment, _)) = super::super::matching::constrained_assignment(
            weights.each_ref().map(|w| w.as_slice()),
            allowed.each_ref().map(|a| a.as_slice()),
            none,
        ) else {
            return i128::MAX;
        };
        let sum = increment + shift.iter().sum::<i64>();
        let upper = add_up(sum as f64, (r * base).next_up());
        // For every feasible binding, 4 r P A <= (P + r A)^2.
        let cap =
            (((upper * upper).next_up() / (4.0 * r)).next_up() * (1.0 + margin).next_up()).next_up().ceil() as i128;
        best = best.min(cap);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_every_legal_binding_with_forbidden_empty_and_negative_increments() {
        for offset in 0..20 {
            let rows = std::array::from_fn(|slot| {
                (0..4)
                    .map(|choice| {
                        (
                            (11 + (slot * 13 + choice * 7 + offset) % 41) as i64,
                            ((slot * 7 + choice * 19 + offset) % 37) as f64 / 7.0,
                        )
                    })
                    .collect::<Vec<_>>()
            });
            let allowed = std::array::from_fn(|slot| (0..3).map(|s| (slot + s + offset) % 4 != 0).collect());
            let none = std::array::from_fn(|slot| (slot + offset) % 4 != 0);
            let upper = product_upper(&rows, &allowed, none, 0.3, 0.0, [0.5, 8.0, 64.0]);
            for key in 0..4usize.pow(5) {
                let mut k = key;
                let choices: [usize; 5] = std::array::from_fn(|_| {
                    let c = k % 4;
                    k /= 4;
                    c
                });
                if (0..5).any(|s| {
                    if choices[s] == 0 {
                        !none[s]
                    } else {
                        !allowed[s][choices[s] - 1] || choices[..s].contains(&choices[s])
                    }
                }) {
                    continue;
                }
                let power = (0..5).map(|s| rows[s][choices[s]].0).sum::<i64>();
                let gain = 0.3 + (0..5).map(|s| rows[s][choices[s]].1).sum::<f64>();
                assert!(upper as f64 >= power as f64 * gain, "{offset} {choices:?}");
            }
        }
    }
    #[test]
    fn shared_resource_tightens_the_independent_power_gain_product() {
        let rows = std::array::from_fn(|_| vec![(10, 1.0), (100, 10.0)]);
        let allowed = std::array::from_fn(|_| vec![true]);
        let upper = product_upper(&rows, &allowed, [true; 5], 0.0, 0.0, [5.0, 10.0, 20.0]);
        assert!(upper >= 140 * 14);
        assert!(upper < 2100);
        assert!(upper < 500 * 50);
    }
    #[test]
    fn unavailable_numerics_never_form_an_exclusion_certificate() {
        let rows = std::array::from_fn(|_| vec![(10, 1.0), (100, 10.0)]);
        let allowed = std::array::from_fn(|_| vec![true]);
        assert_eq!(product_upper(&rows, &allowed, [true; 5], 0.0, 0.0, [0.0, f64::NAN, f64::INFINITY]), i128::MAX);
        assert_eq!(product_upper(&rows, &allowed, [false; 5], 0.0, 0.0, [5.0, 10.0, 20.0]), i128::MAX);
        let huge = std::array::from_fn(|_| vec![(10, 1.0); 4098]);
        assert_eq!(product_upper(&huge, &allowed, [true; 5], 0.0, 0.0, [5.0, 10.0, 20.0]), i128::MAX);
    }
}
