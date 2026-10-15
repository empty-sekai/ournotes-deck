//! Exhausted LUCK frontiers over a synthetic 31-team domain.
//! These inputs declare independent nominal lottery tables.
use super::common::set_column;
use super::{data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{RecommendationRequest, RecommendedDeck, Strategy},
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
fn a_reached_threshold_ranks_by_the_expected_score() {
    for (k, cache) in [(1, 0), (12, 64)] {
        let (data, roster, request) = inputs(545_749, k, cache);
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.telemetry.leaves.visited, 31);
        assert_eq!(
            result.telemetry.lottery_refinement.attempted_orders, 0,
            "the expected-score enclosures already separate these teams"
        );
        // Every order of these teams reaches the threshold at its expected score, so they rank as by the score.
        let mut by_score = joint_request("mission", true, json!({"kind":"score"}));
        (by_score.k, by_score.strategy, by_score.limits.cache_entries) = (k, Strategy::Exhaustive, cache);
        let by_score = engine::recommend(&data, &roster, &by_score).unwrap();
        assert_eq!(by_score.completion, Completion::Complete);
        let teams = |decks: &[RecommendedDeck]| decks.iter().map(|d| (d.members, d.snaps, d.power)).collect::<Vec<_>>();
        assert_eq!(teams(&result.results), teams(&by_score.results));
        for deck in &result.results {
            assert_eq!(deck.rank_certified, Some(true));
            let payoff = deck.expected_payoff.as_ref().expect("the threshold is reached in every order");
            assert_eq!(payoff.numerator, payoff.denominator);
            let interval = deck.payoff_interval.as_ref().unwrap();
            assert!(interval.lower_f64() <= 1.0 && interval.upper_f64() == 1.0);
        }
        assert_ranked(&result.results);
    }
}

#[test]
fn exact_refinement_settles_an_expected_score_on_the_threshold_without_a_cache() {
    let (data, roster, request) = inputs(611_236, 12, 0);
    let result = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.telemetry.leaves.visited, 31);
    assert_eq!(result.results.len(), 12);
    let winner = &result.results[0];
    assert_eq!(winner.members, [1, 2, 3, 4, 5]);
    assert_eq!(winner.snaps, [Some(1), Some(2), None, None, None]);
    assert_eq!(winner.power, 127_480);
    assert!(result.results.iter().all(|deck| deck.rank_certified == Some(true)));
    let counters = &result.telemetry.lottery_refinement;
    assert!(counters.installed_orders > 0, "the enclosures alone leave an expected score on the threshold");
    assert_eq!(counters.completed_orders, counters.installed_orders);
    assert_eq!(counters.declined_orders, 0);
    assert!(counters.terminal_paths > counters.completed_orders);
    // 54 of the winner's 120 orders reach the threshold at their expected scores.
    let reward = winner.expected_payoff.as_ref().expect("an exact reward");
    let (n, d): (i128, i128) = (reward.numerator.parse().unwrap(), reward.denominator.parse().unwrap());
    assert_eq!(n * 120, 54 * d);
    assert_ranked(&result.results);
}

#[test]
fn a_long_frame_schedule_loads_boundary_orders_and_certifies_the_threshold_ranking() {
    use ournotes_search::types::{Execution, PlayPolicy};
    use ournotes_sim::live::model::JudgementStream;

    for cache_entries in [0, 64] {
        let (data, roster, mut request) = inputs(611_236, 12, cache_entries);
        let mut stream = JudgementStream::theoretical_best(&data.chart(super::SCORE_ID).unwrap());
        let first = stream.frames.len() as i32;
        stream.frames.extend((first..=600).map(|frame| frame * 1000 / 60));
        request.execution =
            Execution::Live { score_id: super::SCORE_ID, gekisou: true, play: PlayPolicy::Stream { stream } };
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(
            result.completion,
            Completion::Complete,
            "{:#?}; results: {:#?}",
            result.telemetry.lottery_refinement,
            result.results
        );
        assert_eq!(result.results.len(), 12);
        let winner = &result.results[0];
        assert_eq!(winner.members, [1, 2, 3, 4, 5]);
        assert_eq!(winner.snaps, [Some(1), Some(2), None, None, None]);
        assert!(result.results.iter().all(|deck| deck.rank_certified == Some(true)));
        assert!(result.telemetry.lottery_refinement.installed_orders > 0);
        assert_eq!(result.telemetry.lottery_refinement.declined_orders, 0);
        assert_ranked(&result.results);
    }
}

#[test]
fn long_stream_refinement_materializes_the_boundary_candidate() {
    let (data, roster, _) = inputs(583_086, 31, 0);
    let mut wire = super::joint_request_json("mission", true, json!({"kind":"scoreAtLeast","threshold":583_086}));
    wire["k"] = json!(31);
    wire["strategy"] = json!({"kind":"exhaustive"});
    wire["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":(0..=2000).step_by(20).collect::<Vec<_>>(),
        "judged":(1..=12).map(|id| json!([id*5,id,5,id*100])).collect::<Vec<_>>()
    }});
    let short = engine::recommend(&data, &roster, &serde_json::from_value(wire.clone()).unwrap()).unwrap();
    assert_eq!(short.completion, Completion::Complete);
    assert_ranked(&short.results);
    wire["execution"]["play"]["stream"]["frames"] = json!((0..=10400).step_by(20).collect::<Vec<_>>());
    let long = engine::recommend(&data, &roster, &serde_json::from_value(wire).unwrap()).unwrap();
    assert!(matches!(long.completion, Completion::Complete | Completion::RefinementRequired));
    assert_eq!(long.telemetry.leaves.visited, 31);
    let counters = &long.telemetry.lottery_refinement;
    assert!(counters.installed_orders > 0);
    assert_eq!(counters.completed_orders, counters.installed_orders);
    if long.completion == Completion::RefinementRequired {
        assert_eq!(long.optimality, ournotes_search::types::Optimality::Unproven);
        assert!(counters.declined_orders > 0);
        // The allowance runs out: the declines name the work budget.
        assert_eq!(counters.declines.work_budget, counters.declined_orders);
        assert_eq!(counters.frames, ournotes_sim::live::full::LuckExactBudget::default().remaining_frames);
    } else {
        assert!(long.results.iter().all(|row| row.rank_certified == Some(true)));
        assert_ranked(&long.results);
    }
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

/// Certified teams rank by their reward, the share of their orders whose expected score reaches the threshold, and
/// teams with equal rewards by their expected scores.
fn assert_ranked(decks: &[RecommendedDeck]) {
    let reward = |deck: &RecommendedDeck| {
        let p = deck.payoff_interval.as_ref().unwrap();
        (p.lower_f64(), p.upper_f64())
    };
    let score = |deck: &RecommendedDeck| {
        let s = deck.score_interval.as_ref().unwrap();
        (s.lower_f64(), s.upper_f64())
    };
    for pair in decks.windows(2) {
        let ((a_low, a_high), (b_low, b_high)) = (reward(&pair[0]), reward(&pair[1]));
        assert!(a_high >= b_low, "a lower reward ranks first");
        if a_low <= b_high {
            assert!(score(&pair[0]).0 >= score(&pair[1]).1, "equal rewards rank by the expected score");
        }
    }
}

#[test]
fn certified_seed_budget_preserves_the_complete_canonical_ranking() {
    for (k, cache) in [(1, 0), (3, 64), (31, 0)] {
        let (data, roster, _) = inputs(610_000, k, cache);
        let mut request = joint_request("mission", true, json!({"kind":"score"}));
        request.k = k;
        request.limits.cache_entries = cache;
        request.strategy = Strategy::Exhaustive;
        let reference = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(reference.completion, Completion::Complete);
        request.strategy = Strategy::BranchAndBound;
        let actual = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(actual.completion, Completion::Complete);
        assert_eq!(actual.results.len(), reference.results.len());
        for (a, b) in actual.results.iter().zip(&reference.results) {
            assert_eq!((a.members, a.snaps, a.power), (b.members, b.snaps, b.power));
            assert_eq!(a.rank_certified, Some(true));
            let x = a.payoff_interval.as_ref().unwrap();
            let y = b.payoff_interval.as_ref().unwrap();
            assert!(x.lower_f64() <= y.upper_f64() && y.lower_f64() <= x.upper_f64());
        }
        // The certified warm start evaluates at most 16 proposals.
        let proposals = actual.telemetry.incumbents.warm_start.evaluations;
        assert!(
            proposals > 0 && proposals <= 16,
            "score k={k} proposals={proposals} bounds={:?}",
            actual.telemetry.environment.bounds
        );
        assert!(actual.telemetry.phases.iter().any(|phase| phase.name == "search"));
    }
}
