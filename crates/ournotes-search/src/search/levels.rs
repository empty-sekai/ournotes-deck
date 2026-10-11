//! The levels of a combined metric as one integer payoff.
//!
//! A combined metric compares two teams by the expectation of its first level and, when those are equal, by each
//! later level in turn. The search ranks one integer per terminal result instead: the levels' weighted sums, each
//! counted `multiplier` times. With `cap` the largest folded payoff of the levels after a level in one terminal
//! result, that level's multiplier is `ORDERS * cap + 1`: the levels after it sum to at most `ORDERS * cap` over the
//! performance orders, less than one unit of the level's own sum, so the folded sums of two teams compare as their
//! levels do. The payoff of every evaluated terminal result is checked against the caps (`physical::payoff_of`), and
//! an upper bound of the folded payoff is the weighted sum of the terms' bounds, so a node whose first-level bound
//! ties the K-th team is still closed by the bounds of the later levels.
use super::uniform::ORDERS;
use crate::types::{LevelScale, Metric};
use ournotes_sim::Error;

/// Room above the folded cap for the sums over the performance orders and for the bounds' weighted sums.
const HEADROOM: i128 = 1 << 20;

/// `metric` with its levels folded; `cap` is at least a term's payoff of one terminal result. None for a metric
/// without levels.
pub(crate) fn fold(metric: &Metric, cap: impl Fn(&Metric) -> i128) -> Result<Option<Metric>, Error> {
    let Metric::Combined { levels } = metric else {
        return Ok(None);
    };
    let wide = || {
        Error::Input(
            "the weights and payoff ranges of the combined metric's levels exceed the exact integer range".into(),
        )
    };
    let mut levels = levels.clone();
    let mut below = 0i128;
    for level in levels.iter_mut().rev() {
        let multiplier = below.checked_mul(ORDERS as i128).and_then(|v| v.checked_add(1)).ok_or_else(wide)?;
        let own = level
            .terms
            .iter()
            .try_fold(0i128, |sum, term| {
                sum.checked_add(cap(&term.metric).max(0).checked_mul(i128::from(term.weight))?)
            })
            .ok_or_else(wide)?;
        let cap = own.checked_mul(multiplier).and_then(|v| v.checked_add(below)).ok_or_else(wide)?;
        level.scale = LevelScale { multiplier, cap };
        below = cap;
    }
    if below > i128::MAX / HEADROOM {
        return Err(wide());
    }
    Ok(Some(Metric::Combined { levels }))
}

/// The levels' own sums of a folded sum over at most [`ORDERS`] terminal results (None for a metric without
/// levels).
pub(crate) fn unfold(metric: &Metric, folded: i128) -> Option<Vec<i128>> {
    let mut rest = folded;
    Some(
        metric
            .levels()?
            .iter()
            .map(|level| {
                let unit = level.scale.multiplier.max(1);
                let own = rest.div_euclid(unit);
                rest = rest.rem_euclid(unit);
                own
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MetricLevel, MetricTerm};

    fn metric(levels: &[&[u32]]) -> Metric {
        Metric::Combined {
            levels: levels
                .iter()
                .map(|weights| {
                    MetricLevel::new(
                        weights.iter().map(|&weight| MetricTerm { metric: Metric::Score, weight }).collect(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn folded_sums_order_as_their_levels() {
        let folded = fold(&metric(&[&[3], &[2, 1], &[5]]), |_| 7).unwrap().unwrap();
        let levels = folded.levels().unwrap();
        // One terminal result pays at most 35 on the last level and 21 on the one before it.
        assert_eq!(levels[2].scale.multiplier, 1);
        assert_eq!(levels[2].scale.cap, 35);
        assert_eq!(levels[1].scale.multiplier, 120 * 35 + 1);
        assert_eq!(levels[1].scale.cap, 21 * (120 * 35 + 1) + 35);
        assert_eq!(levels[0].scale.multiplier, 120 * levels[1].scale.cap + 1);
        let sum =
            |own: [i128; 3]| own.iter().zip(levels).map(|(own, level)| own * level.scale.multiplier).sum::<i128>();
        // Sums over 120 results at the caps of the later levels stay below one unit of the level before.
        let rows = [[0, 0, 0], [0, 0, 120 * 35], [0, 1, 0], [0, 120 * 21, 120 * 35], [1, 0, 0], [1, 0, 1], [-1, 5, 9]];
        for a in rows {
            assert_eq!(unfold(&folded, sum(a)).unwrap(), a);
            for b in rows {
                assert_eq!(sum(a).cmp(&sum(b)), a.cmp(&b), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn one_level_counts_once_and_wide_levels_are_refused() {
        let folded = fold(&metric(&[&[4, 9]]), |_| 100).unwrap().unwrap();
        assert_eq!(folded.primary_unit(), 1);
        assert_eq!(folded.levels().unwrap()[0].scale.cap, 1300);
        assert!(fold(&Metric::Score, |_| 1).unwrap().is_none());
        let wide = metric(&[&[1_000_000_000], &[1_000_000_000], &[1_000_000_000], &[1_000_000_000]]);
        assert!(matches!(fold(&wide, |_| i128::from(i32::MAX)), Err(Error::Input(_))));
    }
}
