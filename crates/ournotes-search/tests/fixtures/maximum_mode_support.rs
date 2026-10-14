//! Maximum aggregation of a Gekisou live: a lottery-free live is searched and matches the per-order oracle; a live
//! with a LUCK range ranks teams by their best order under the certified lottery law of each order.
use super::{correctness_matrix, maximum_matrix};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Aggregation, RecommendedDeck, Strategy},
};
use serde_json::json;

#[test]
fn maximum_gekisou_without_lottery_matches_the_order_oracle() {
    let (data, roster) = correctness_matrix::fixture(5, 1, 5, false);
    for metric in [
        json!({"kind":"score"}),
        json!({"kind":"scoreAtLeast","threshold":450000}),
        json!({"kind":"cappedScore","threshold":450000}),
    ] {
        let request = maximum_matrix::request(&data, "mission", metric);
        assert_eq!(request.aggregation, Aggregation::Maximum);
        maximum_matrix::verify(&data, &roster, &request, 6, &format!("mission {:?}", request.metric));
    }
}

type Row = (([i64; 5], [Option<i64>; 5]), serde_json::Value);

fn rows(decks: &[RecommendedDeck]) -> Vec<Row> {
    decks
        .iter()
        .map(|deck| ((deck.members, deck.snaps), serde_json::to_value(&deck.payoff_interval).unwrap()))
        .collect()
}

fn fraction(value: &ournotes_search::types::Fraction) -> (i128, i128) {
    (value.numerator.parse().unwrap(), value.denominator.parse().unwrap())
}

#[test]
fn maximum_gekisou_with_a_lottery_range_ranks_by_the_best_certified_order() {
    let (data, roster) = correctness_matrix::fixture(7, 0, 7, true);
    for metric in [json!({"kind":"score"}), json!({"kind":"scoreAtLeast","threshold":450000})] {
        let mut request = maximum_matrix::request(&data, "mission", metric.clone());
        request.constraints.leader = None;
        request.constraints.no_snaps = true;
        request.limits.cache_entries = 64;
        let mut expected_request = request.clone();
        expected_request.aggregation = Aggregation::Expected;
        expected_request.strategy = Strategy::Exhaustive;
        expected_request.k = 100;
        let expected = engine::recommend(&data, &roster, &expected_request).unwrap();
        assert_eq!(expected.completion, Completion::Complete, "{metric}");
        for k in [1, 3, 10] {
            request.k = k;
            request.strategy = Strategy::Exhaustive;
            let oracle = engine::recommend(&data, &roster, &request).unwrap();
            request.strategy = Strategy::BranchAndBound;
            let bounded = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!((oracle.completion, bounded.completion), (Completion::Complete, Completion::Complete));
            assert_eq!(rows(&bounded.results), rows(&oracle.results), "{metric} k={k}");
            assert_eq!(oracle.aggregation, Aggregation::Maximum);
            assert_eq!(oracle.telemetry.environment.target.as_ref().unwrap().denominator, "1");
            for deck in &oracle.results {
                assert_eq!(deck.rank_certified, Some(true));
                assert!(deck.expected_payoff.is_none() && deck.expected_score.is_none());
                let interval = deck.payoff_interval.as_ref().unwrap();
                if let Some(value) = &deck.objective_value {
                    let (n, d) = fraction(value);
                    assert!(interval.lower_f64() <= n as f64 / d as f64 && n as f64 / d as f64 <= interval.upper_f64());
                    assert!(deck.best_order.is_some(), "an exact maximum names the order that attains it");
                }
                if let Some(best) = &deck.best_order {
                    let mut members = best.members;
                    members.sort_unstable();
                    let mut own = deck.members;
                    own.sort_unstable();
                    assert_eq!(members, own);
                    assert_eq!(best.performance_order.map(|slot| deck.members[slot]), best.members);
                }
                // The greatest order value is at least the mean over the orders.
                let mean = expected
                    .results
                    .iter()
                    .find(|row| (row.members, row.snaps) == (deck.members, deck.snaps))
                    .expect("the same domain");
                let mean = mean.payoff_interval.as_ref().unwrap();
                assert!(interval.upper_f64() >= mean.lower_f64(), "{metric} {:?}", deck.members);
            }
            assert!(oracle.results.windows(2).all(|rows| {
                let (a, b) = (rows[0].payoff_interval.as_ref().unwrap(), rows[1].payoff_interval.as_ref().unwrap());
                a.upper_f64() >= b.lower_f64()
            }));
        }
    }
}

#[test]
fn maximum_gekisou_with_a_lottery_range_keeps_the_exhaustive_ranking_with_two_members_per_character() {
    let (data, roster) = correctness_matrix::fixture(10, 0, 5, true);
    for metric in [json!({"kind":"score"}), json!({"kind":"scoreAtLeast","threshold":450000})] {
        let mut request = maximum_matrix::request(&data, "mission", metric.clone());
        request.constraints.leader = None;
        request.constraints.no_snaps = true;
        request.limits.cache_entries = 64;
        for k in [1, 4, 12] {
            request.k = k;
            request.strategy = Strategy::Exhaustive;
            let oracle = engine::recommend(&data, &roster, &request).unwrap();
            request.strategy = Strategy::BranchAndBound;
            let bounded = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!((oracle.completion, bounded.completion), (Completion::Complete, Completion::Complete));
            assert_eq!(rows(&bounded.results), rows(&oracle.results), "{metric} k={k}");
            assert!(oracle.results.iter().all(|deck| deck.rank_certified == Some(true)));
            // The joint traversal of the score weighs the lottery laws of its domain.
            if metric["kind"] == "score" {
                let laws = bounded.telemetry.environment.bounds.domain_laws.as_ref().expect("domain laws");
                assert_eq!(laws.weighed, 1, "k={k}");
            }
        }
    }
}

#[test]
fn maximum_gekisou_with_a_certain_threshold_is_exact_in_the_first_order() {
    let (data, roster) = correctness_matrix::fixture(5, 0, 5, true);
    for (threshold, value) in [(1, 1), (2147483647, 0)] {
        let mut request =
            maximum_matrix::request(&data, "mission", json!({"kind":"scoreAtLeast","threshold":threshold}));
        request.constraints.leader = None;
        request.constraints.no_snaps = true;
        request.k = 5;
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.results.len(), 5);
        for deck in &result.results {
            assert_eq!(fraction(deck.objective_value.as_ref().unwrap()), (value, 1));
            let best = deck.best_order.as_ref().unwrap();
            assert_eq!(best.payoff, value.to_string());
        }
    }
}
