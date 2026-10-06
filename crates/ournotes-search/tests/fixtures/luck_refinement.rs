//! Reproduce three exhausted 31-team LUCK frontiers using the existing synthetic generator.
//! These inputs declare nominal lotteries, not a distribution of native PRNG seeds.
use super::common::set_column;
use super::{data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

fn inputs(threshold: i32, k: usize, cache_entries: usize) -> (DeckData, Roster, RecommendationRequest) {
    let mut synth = synthetic_master(5, 2, 5);
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(2);
        row["_gekisouMission2"] = json!(3);
        row["_gekisouMission3"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveSettings", &mut |row| {
        if matches!(row["_key"].as_str(), Some("gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush")) {
            row["_value"] = json!("10");
        }
    });
    let mut document = data_document(&synth, 5, 2, 5);
    document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    let mut request = joint_request("mission", true, json!({"kind":"scoreAtLeast","threshold":threshold}));
    request.k = k;
    request.strategy = Strategy::Exhaustive;
    request.limits.cache_entries = cache_entries;
    (data, roster, request)
}

#[test]
fn probability_range_settles_the_k1_and_k12_certain_event_witnesses() {
    let powers = [127480, 125551, 125292, 125100, 124371, 124190, 124054, 123871, 123296, 122261, 122183, 122125];
    for (k, cache) in [(1, 0), (12, 64)] {
        let (data, roster, request) = inputs(545_749, k, cache);
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.telemetry.leaves.visited, 31);
        assert_eq!(result.results.iter().map(|deck| deck.power).collect::<Vec<_>>(), powers[..k]);
        assert_eq!(
            result.telemetry.lottery_refinement.attempted_orders, 0,
            "the closed probability range already proves these power ties"
        );
        for deck in &result.results {
            assert_eq!(deck.rank_certified, Some(true));
            let payoff = deck.expected_payoff.as_ref().expect("the threshold is reached on every path");
            assert_eq!(payoff.numerator, payoff.denominator);
            let interval = deck.payoff_interval.as_ref().unwrap();
            assert!(interval.lower_f64() >= 0.0);
            assert_eq!(interval.upper_f64(), 1.0);
        }
    }
}

#[test]
fn exact_nominal_refinement_settles_the_remaining_threshold_witness_without_a_cache() {
    let (data, roster, request) = inputs(610_000, 1, 0);
    let result = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.telemetry.leaves.visited, 31);
    assert_eq!(result.results.len(), 1);
    let winner = &result.results[0];
    assert_eq!(winner.members, [1, 2, 3, 4, 5]);
    assert_eq!(winner.snaps, [Some(1), Some(2), None, None, None]);
    assert_eq!(winner.power, 127_480);
    assert_eq!(winner.rank_certified, Some(true));
    let counters = &result.telemetry.lottery_refinement;
    assert!(counters.installed_orders > 0, "moment bounds alone cannot settle this witness");
    assert_eq!(counters.completed_orders, counters.installed_orders);
    assert_eq!(counters.declined_orders, 0);
    assert!(counters.terminal_paths > counters.completed_orders);
    let probability = winner.payoff_interval.as_ref().unwrap();
    assert!(
        probability.lower_f64() > 0.388_928_092_021_209_54,
        "the provider must add information beyond the original first-moment lower bound"
    );
    assert!(probability.upper_f64() <= 1.0);
    // Complete certifies the ranking. Numeric expectations remain absent until all 120 relevant order
    // expectations are exact; no midpoint is filled in just because the winner has been identified.
    if winner.expected_payoff.is_none() {
        assert!(probability.lower_f64() < probability.upper_f64());
    }
}

#[test]
fn long_stream_refinement_materializes_the_boundary_candidate() {
    let (data, roster, _) = inputs(610_000, 1, 0);
    let mut wire = super::joint_request_json("mission", true, json!({"kind":"scoreAtLeast","threshold":610_000}));
    wire["k"] = json!(1);
    wire["strategy"] = json!({"kind":"exhaustive"});
    wire["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":(0..=2000).step_by(20).collect::<Vec<_>>(),
        "judged":(1..=12).map(|id| json!([id*5,id,5,id*100])).collect::<Vec<_>>()
    }});
    let short = engine::recommend(&data, &roster, &serde_json::from_value(wire.clone()).unwrap()).unwrap();
    assert_eq!(short.completion, Completion::Complete);
    wire["execution"]["play"]["stream"]["frames"] = json!((0..=10400).step_by(20).collect::<Vec<_>>());
    let long = engine::recommend(&data, &roster, &serde_json::from_value(wire).unwrap()).unwrap();
    assert_eq!(long.completion, Completion::RefinementRequired);
    assert_eq!(long.optimality, ournotes_search::types::Optimality::Unproven);
    assert_eq!(long.telemetry.leaves.visited, 31);
    let counters = &long.telemetry.lottery_refinement;
    assert!(counters.installed_orders > 0);
    assert_eq!(counters.completed_orders, counters.installed_orders);
    assert!(counters.declined_orders > 0);
    assert_eq!(counters.frames, ournotes_sim::live::full::LuckExactBudget::default().remaining_frames);
    // The finite work allowance preserves the ambiguous frontier and the true winning candidate.
    let expected = &short.results[0];
    let actual = long
        .results
        .iter()
        .find(|row| row.members == expected.members && row.snaps == expected.snaps)
        .expect("the certified short-schedule winner remains represented");
    assert_eq!(actual.power, expected.power);
    let a = actual.payoff_interval.as_ref().unwrap();
    let b = expected.payoff_interval.as_ref().unwrap();
    assert!(a.lower_f64() <= b.upper_f64() && b.lower_f64() <= a.upper_f64());
    for row in &long.results {
        let p = row.payoff_interval.as_ref().unwrap();
        assert!(p.lower_f64() >= 0.0 && p.upper_f64() <= 1.0);
    }
}
