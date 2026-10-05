//! The played-live search target: the expected payoff when the five members perform in a uniformly random order.
//!
//! A played live shuffles the performers: each of the 120 performance orders is taken with probability 1/120. The
//! value of a team is the mean of its payoff over the 120 orders. Only the leader's slot is distinguished (the leader
//! skill); the power does not depend on where the other four members sit, and the performance order is a uniform
//! shuffle of the five slots. So a team is the leader (member, Snap) pair and an unordered set of four (member, Snap)
//! pairs, and every value of the search is a numerator over [`ORDERS`].
//!
//! The canonical layout of a team puts the leader in slot 2 and the other four pairs in slots 0, 1, 3, 4 in ascending
//! member card id order (the members of a team are distinct cards, so this order is total). Every result, cache and
//! comparison of the search uses this layout.
use super::expectation::PhysicalDeck;
use ournotes_sim::pool::Pool;

/// Number of performance orders, and the denominator of every played-live value of the search.
pub const ORDERS: usize = 120;

/// Non-leader slots in canonical order.
pub(crate) const NONLEADER: [usize; 4] = [0, 1, 3, 4];

/// The 120 performance orders in lexicographic order; `order[k]` is the slot performing at position `k`.
pub fn all_orders() -> Vec<[usize; 5]> {
    let mut out = Vec::with_capacity(ORDERS);
    let mut order = [0, 1, 2, 3, 4];
    loop {
        out.push(order);
        if !super::live::next_permutation(&mut order) {
            return out;
        }
    }
}

/// Lexicographic index of a performance order in [`all_orders`].
pub fn order_index(order: &[usize; 5]) -> usize {
    let mut index = 0;
    for i in 0..5 {
        index = index * (5 - i) + (i + 1..5).filter(|&j| order[j] < order[i]).count();
    }
    index
}

/// The position of each slot in a performance order.
pub(crate) fn positions_of(order: &[usize; 5]) -> [usize; 5] {
    let mut positions = [0; 5];
    for (k, &slot) in order.iter().enumerate() {
        positions[slot] = k;
    }
    positions
}

/// The positions of every slot in each of the 120 orders, each with mass 1, in [`all_orders`] order.
pub(crate) fn order_positions() -> Vec<([usize; 5], u128)> {
    all_orders().iter().map(|o| (positions_of(o), 1)).collect()
}

/// The single pseudo-order of the position-mean bounds: identity positions carrying the mass of all 120 orders.
/// With position-mean gains every position of a pair reads the same gain, so a cap read at these positions, times
/// [`ORDERS`], bounds the sum of the payoffs over the orders (see `docs/search.md`, "Position-mean bounds").
pub(crate) const MEAN_ORDERS: [([usize; 5], u128); 1] = [([0, 1, 2, 3, 4], ORDERS as u128)];

/// The mean of five non-negative gains, rounded up (each addition and the division round outward).
pub(crate) fn mean_up(g: &[f64; 5]) -> f64 {
    let mut s = 0.0f64;
    for &v in g {
        s = (s + v).next_up();
    }
    (s / 5.0).next_up()
}

/// A gain row with every position replaced by the row's rounded-up mean.
pub(crate) fn mean_row(g: &[f64; 5]) -> [f64; 5] {
    [mean_up(g); 5]
}

/// The canonical layout of a team: the leader stays in slot 2, the other pairs fill slots 0, 1, 3, 4 in ascending
/// member card id order.
pub(crate) fn canonical(pool: &Pool, p: &PhysicalDeck) -> PhysicalDeck {
    let mut pairs = NONLEADER.map(|s| (p.members[s], p.snaps[s]));
    pairs.sort_unstable_by_key(|&(m, _)| pool.members[m].id);
    let mut out = *p;
    for (i, &s) in NONLEADER.iter().enumerate() {
        (out.members[s], out.snaps[s]) = pairs[i];
    }
    out
}

/// The smallest concave non-decreasing function above the step function `x -> max of the values of the corners at or
/// below x` (0 below the first corner) on `[0, inf)`. `steps` are the corners `(x, value)` with ascending `x >= 0` and
/// non-negative values. The upper concave hull of the corners that raise the running maximum, flat after the last
/// one, lies above every step: it is non-decreasing, so on `[x_i, x_{i+1})` it is at least its value at the corner
/// `x_i`. Returns the hull's corners, the first at `x = 0`.
pub(crate) fn concave_majorant(steps: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut hull: Vec<(i64, i64)> = Vec::new();
    let mut best = i64::MIN;
    let start = (steps.first().is_none_or(|&(x, _)| x > 0)).then_some((0, 0));
    for &(x, v) in start.iter().chain(steps) {
        // Only corners raising the running maximum can be on the hull: the step function is the prefix maximum.
        if v <= best {
            continue;
        }
        best = v;
        while hull.len() >= 2 {
            let (x1, v1) = hull[hull.len() - 2];
            let (x2, v2) = hull[hull.len() - 1];
            // Drop the middle corner when it lies on or below the chord from (x1, v1) to (x, v).
            if (i128::from(v2) - i128::from(v1)) * (i128::from(x) - i128::from(x1))
                <= (i128::from(v) - i128::from(v1)) * (i128::from(x2) - i128::from(x1))
            {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push((x, v));
    }
    hull
}

/// `ceil(scale * f(x) / divisor)` for the piecewise-linear concave function through `hull` (flat after its last
/// corner), with `scale >= 0` and `x >= 0`. Exact integer arithmetic; None on overflow.
pub(crate) fn concave_value_ceil(hull: &[(i64, i64)], x: i128, scale: i128, divisor: i128) -> Option<i128> {
    let last = *hull.last()?;
    let ceil_div = |n: i128, d: i128| -> Option<i128> { Some(n.checked_add(d - 1)?.div_euclid(d)) };
    if x >= i128::from(last.0) {
        return ceil_div(scale.checked_mul(i128::from(last.1))?, divisor);
    }
    let i = hull.partition_point(|&(corner, _)| i128::from(corner) <= x).max(1);
    let (x1, v1) = (i128::from(hull[i - 1].0), i128::from(hull[i - 1].1));
    let (x2, v2) = (i128::from(hull[i].0), i128::from(hull[i].1));
    // f(x) = (v1 (x2 - x1) + (v2 - v1)(x - x1)) / (x2 - x1)
    let numerator = v1.checked_mul(x2 - x1)?.checked_add((v2 - v1).checked_mul(x - x1)?)?;
    ceil_div(scale.checked_mul(numerator)?, divisor.checked_mul(x2 - x1)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_are_the_permutations_in_lexicographic_order() {
        let orders = all_orders();
        assert_eq!(orders.len(), ORDERS);
        for (i, o) in orders.iter().enumerate() {
            assert_eq!(order_index(o), i);
            let mut sorted = *o;
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2, 3, 4]);
        }
        let positions = order_positions();
        // every slot takes every position in 24 orders
        for slot in 0..5 {
            for k in 0..5 {
                assert_eq!(positions.iter().filter(|(p, _)| p[slot] == k).count(), 24);
            }
        }
    }

    #[test]
    fn mean_rounds_up() {
        let g = [0.1, 0.2, 0.3, 0.4, 0.5];
        let m = mean_up(&g);
        assert!(m * 5.0 >= g.iter().sum::<f64>());
        assert!(m < 0.3 + 1e-12);
        assert_eq!(mean_row(&[1.0; 5]), [mean_up(&[1.0; 5]); 5]);
    }

    #[test]
    fn concave_majorant_lies_above_every_step_and_is_concave() {
        // A lower grade can pay more: the step function is the running maximum.
        let steps = [(0, 80), (300, 100), (450, 900), (600, 20)];
        let hull = concave_majorant(&steps);
        assert_eq!(hull, vec![(0, 80), (450, 900)]);
        let step = |x: i128| steps.iter().filter(|&&(c, _)| i128::from(c) <= x).map(|&(_, v)| v).max().unwrap() as i128;
        let mut previous_slope = None;
        for x in (0..800).step_by(7) {
            let f = concave_value_ceil(&hull, x, 1, 1).unwrap();
            assert!(f >= step(x), "x {x}");
            if x > 0 {
                let slope = f - concave_value_ceil(&hull, x - 7, 1, 1).unwrap();
                if let Some(p) = previous_slope {
                    // rounding up can raise a chord by one at most
                    assert!(slope <= p + 1, "x {x}");
                }
                previous_slope = Some(slope);
            }
        }
        assert_eq!(concave_value_ceil(&hull, 225, 1, 1), Some(80 + (820 * 225 + 449) / 450));
        assert_eq!(concave_value_ceil(&hull, 10_000, 3, 2), Some(1350));
        assert_eq!(concave_majorant(&[(0, 5)]), vec![(0, 5)]);
        assert_eq!(concave_value_ceil(&[(0, 5)], 0, 10_007, 10_000), Some(6));
    }
}
