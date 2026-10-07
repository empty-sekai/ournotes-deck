//! Cross-song ordering uses the same certified comparisons as the team frontier.
use crate::{
    search::{
        expectation::ExactExpectation,
        interval_topk::{
            CandidateInterval, CanonicalTie, IntervalTopK, RemainingDomain, compare_exact, exact_in_interval,
        },
    },
    types::RecommendedDeck,
};
use ournotes_sim::{Error, live::certified::F64Interval};

pub(super) struct SongValue {
    id: i64,
    power: i32,
    exact: Option<ExactExpectation>,
    bounds: F64Interval,
}
impl SongValue {
    pub(super) fn from_deck(id: i64, deck: &RecommendedDeck) -> Result<Self, Error> {
        let invalid = || Error::Domain("complete song evaluation has no valid payoff certificate".into());
        let exact = deck
            .objective_value
            .as_ref()
            .or(deck.expected_payoff.as_ref())
            .map(|f| -> Result<ExactExpectation, Error> {
                Ok(ExactExpectation {
                    numerator: f.numerator.parse().map_err(|_| invalid())?,
                    denominator: f.denominator.parse().map_err(|_| invalid())?,
                })
            })
            .transpose()?;
        if exact.is_some_and(|v| v.denominator == 0) {
            return Err(invalid());
        }
        let bounds = match (&deck.payoff_interval, exact) {
            (Some(v), _) => F64Interval::new(v.lower_f64(), v.upper_f64())?,
            (_, Some(v)) => {
                let d = v.denominator as f64;
                let denominator = if v.denominator <= 9_007_199_254_740_992 {
                    F64Interval::point(d)?
                } else {
                    F64Interval::new(d.next_down(), d.next_up())?
                };
                F64Interval::integer(v.numerator).divide(denominator)?
            }
            _ => return Err(invalid()),
        };
        if let Some(value) = exact
            && !exact_in_interval(value, bounds)?
        {
            return Err(invalid());
        }
        Ok(Self { id, power: deck.power, exact, bounds })
    }
}

/// A proved prefix among evaluated songs. Overlapping rows are kept separately by the caller.
pub(super) fn ranked_prefix(values: &[SongValue]) -> Result<Vec<usize>, Error> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    // Keep the common deterministic path O(N log N), including unequal exact denominators.
    if values.iter().all(|v| v.exact.is_some()) {
        let mut indexes: Vec<_> = (0..values.len()).collect();
        indexes.sort_by(|&a, &b| {
            compare_exact(values[b].exact.unwrap(), values[a].exact.unwrap())
                .expect("nonzero exact denominators")
                .then_with(|| values[b].power.cmp(&values[a].power))
                .then_with(|| values[a].id.cmp(&values[b].id))
        });
        return Ok(indexes);
    }
    let mut frontier = IntervalTopK::new(values.len())?;
    for (index, v) in values.iter().enumerate() {
        frontier.insert(CandidateInterval {
            id: index as u64,
            tie: CanonicalTie { power: v.power, key: vec![v.id] },
            score: F64Interval::ZERO,
            payoff: v.bounds,
            exact_score: None,
            exact_payoff: v.exact,
            equality: None,
            revision: 0,
        })?;
    }
    Ok(frontier.proof(RemainingDomain::Exhausted)?.ordered_prefix.into_iter().map(|v| v as usize).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Fraction;
    fn interval(id: i64, lower: f64, upper: f64) -> SongValue {
        SongValue { id, power: 100, exact: None, bounds: F64Interval::new(lower, upper).unwrap() }
    }
    #[test]
    fn selected_objective_ranks_songs_with_different_average_and_peak_values() {
        let deck = |expected: i32, maximum: Option<i32>| RecommendedDeck {
            members: [1, 2, 3, 4, 5],
            snaps: [None; 5],
            power: 100,
            objective_value: maximum.map(|value| Fraction { numerator: value.to_string(), denominator: "1".into() }),
            maximum_score: maximum,
            expected_score: Some(Fraction { numerator: expected.to_string(), denominator: "1".into() }),
            expected_payoff: Some(Fraction { numerator: expected.to_string(), denominator: "1".into() }),
            score_interval: None,
            payoff_interval: None,
            rank_certified: None,
            score_summary: None,
            best_order: None,
            order_outcomes: Vec::new(),
        };
        let expected =
            [SongValue::from_deck(1, &deck(100, None)).unwrap(), SongValue::from_deck(2, &deck(90, None)).unwrap()];
        let maximum = [
            SongValue::from_deck(1, &deck(100, Some(110))).unwrap(),
            SongValue::from_deck(2, &deck(90, Some(140))).unwrap(),
        ];
        assert_eq!(ranked_prefix(&expected).unwrap(), [0, 1]);
        assert_eq!(ranked_prefix(&maximum).unwrap(), [1, 0]);
    }
    #[test]
    fn separated_intervals_rank_without_fake_exact_means() {
        assert_eq!(
            ranked_prefix(&[interval(7, 1.0, 2.0), interval(3, 8.0, 9.0), interval(5, 4.0, 5.0)]).unwrap(),
            [1, 2, 0]
        );
    }
    #[test]
    fn ambiguous_or_touching_intervals_never_use_midpoints_or_song_ids() {
        assert_eq!(ranked_prefix(&[interval(1, 1.0, 3.0), interval(2, 2.0, 4.0), interval(3, 8.0, 9.0)]).unwrap(), [2]);
        assert!(ranked_prefix(&[interval(1, 1.0, 2.0), interval(2, 2.0, 3.0)]).unwrap().is_empty());
    }
    #[test]
    fn exact_rational_values_compare_before_power_and_public_song_id() {
        let mut a = interval(4, 0.0, 1.0);
        a.exact = Some(ExactExpectation { numerator: 1, denominator: 3 });
        let mut b = interval(2, 0.0, 1.0);
        b.exact = Some(ExactExpectation { numerator: 2, denominator: 6 });
        assert_eq!(ranked_prefix(&[a, b]).unwrap(), [1, 0]);
        let mut a = interval(4, 0.0, 1.0);
        a.exact = Some(ExactExpectation { numerator: 1, denominator: 3 });
        let mut b = interval(2, 0.0, 1.0);
        b.exact = Some(ExactExpectation { numerator: 3, denominator: 8 });
        assert_eq!(ranked_prefix(&[a, b]).unwrap(), [1, 0]);
    }
}
