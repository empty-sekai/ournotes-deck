//! Maximum aggregation of a Gekisou live: a lottery-free live is searched and matches the per-order oracle; a live
//! with a LUCK range is rejected as unsupported.
use super::{correctness_matrix, maximum_matrix};
use ournotes_search::{
    engine,
    types::{Aggregation, Strategy},
};
use ournotes_sim::Error;
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

#[test]
fn maximum_gekisou_with_a_lottery_range_is_unsupported() {
    let (data, roster) = correctness_matrix::fixture(7, 0, 7, true);
    for strategy in [Strategy::Exhaustive, Strategy::BranchAndBound] {
        let mut request = maximum_matrix::request(&data, "mission", json!({"kind":"score"}));
        request.strategy = strategy;
        let error = engine::recommend(&data, &roster, &request).unwrap_err();
        assert!(matches!(error, Error::Unsupported(_)), "{error:?}");
    }
}
