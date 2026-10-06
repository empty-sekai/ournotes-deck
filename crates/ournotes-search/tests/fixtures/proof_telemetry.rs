//! End-to-end checks of the whole-domain payoff bound at search completion and deterministic stops.
use super::{data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{engine, search::Completion, types::Strategy};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

fn inputs() -> (DeckData, Roster) {
    let synth = synthetic_master(6, 2, 5);
    (
        DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap(),
        Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap(),
    )
}

fn payoff(deck: &ournotes_search::types::RecommendedDeck) -> i128 {
    deck.expected_payoff.as_ref().expect("deterministic fixture").numerator.parse().unwrap()
}

#[test]
fn completed_scalar_proof_converges_after_results_move_in_every_part() {
    let (data, roster) = inputs();
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        let mut request = joint_request(mode, gekisou, json!({"kind":"score"}));
        let bounded = engine::recommend(&data, &roster, &request).unwrap();
        request.strategy = Strategy::Exhaustive;
        let exhaustive = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(bounded.results, exhaustive.results);
        for outcome in [&bounded, &exhaustive] {
            assert_eq!(outcome.completion, Completion::Complete);
            let proof = &outcome.telemetry.proof;
            let best = payoff(&outcome.results[0]).to_string();
            assert!(proof.complete && proof.upper_bound.is_none());
            assert_eq!(proof.best.as_deref(), Some(best.as_str()));
            assert_eq!(proof.global_upper_bound, proof.best, "{mode}: final best moved out of scalar Top-K");
            assert_eq!(proof.parts_done, proof.parts);
        }
        if gekisou {
            assert!(bounded.telemetry.proof.parts > 1, "exercise all conversion partitions");
        } else {
            assert_eq!(bounded.telemetry.proof.parts, 1);
        }
    }
}

#[test]
fn stopped_global_bound_covers_retained_decks_and_the_full_domain() {
    let (data, roster) = inputs();
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        let mut request = joint_request(mode, gekisou, json!({"kind":"score"}));
        request.strategy = Strategy::Exhaustive;
        let exhaustive = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(exhaustive.completion, Completion::Complete);
        let optimum = payoff(&exhaustive.results[0]);
        request.strategy = Strategy::BranchAndBound;
        // Candidate counts stop reproducibly, independent of machine speed or wall-clock scheduling.
        for limit in [2, 9] {
            request.limits.max_candidates = Some(limit);
            let stopped = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(stopped.completion, Completion::TimedOut, "{mode}: limit {limit}");
            let proof = &stopped.telemetry.proof;
            assert!(!proof.complete && proof.upper_bound.is_some());
            assert!(!stopped.results.is_empty(), "exercise an incumbent after results have moved");
            let global = proof.global_upper_bound.as_ref().expect("bounded final traversal").parse::<i128>().unwrap();
            assert!(global >= optimum, "{mode}: limit {limit}, bound {global} < full-domain optimum {optimum}");
            assert!(stopped.results.iter().all(|deck| global >= payoff(deck)), "the retained domain is covered");
            if gekisou {
                assert!(proof.parts > 1 && proof.parts_done < proof.parts, "include later conversion partitions");
            }
        }
    }
}

#[test]
fn empty_domain_does_not_fabricate_a_zero_best_or_global_bound() {
    let (data, roster) = inputs();
    let mut request = joint_request("free", false, json!({"kind":"score"}));
    request.constraints.exclude_members = vec![4]; // No card of character 4 remains.
    let outcome = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(outcome.completion, Completion::Complete);
    assert!(outcome.results.is_empty());
    let proof = &outcome.telemetry.proof;
    assert!(proof.complete);
    assert!(proof.best.is_none() && proof.kth.is_none() && proof.global_upper_bound.is_none());
}

#[test]
fn certified_interval_frontier_does_not_publish_a_scalar_global_bound() {
    let mut synth = synthetic_master(5, 0, 5);
    super::common::set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_gekisouMission1"] = json!(2));
    let mut document = data_document(&synth, 5, 0, 5);
    document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    let mut data = DeckData::from_json(&document.to_string()).unwrap();
    for effect in &mut data.master.leader_skill_effects {
        effect.effect_value = 0;
    }
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let mut request = joint_request("mission", true, json!({"kind":"score"}));
    request.constraints.leader = None;
    request.constraints.no_snaps = true;
    request.k = 5;
    request.limits.cache_entries = 64;
    let outcome = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(outcome.completion, Completion::Complete);
    assert_eq!(outcome.results.len(), 5);
    assert!(outcome.results.iter().all(|deck| deck.rank_certified == Some(true) && deck.payoff_interval.is_some()));
    let proof = &outcome.telemetry.proof;
    assert!(proof.best.is_none() && proof.kth.is_none());
    assert!(proof.global_upper_bound.is_none(), "scalar top is empty; intervals belong to the certified frontier");
}

#[test]
fn conversion_envelopes_are_prepared_between_traversals_and_stops_cover_later_parts() {
    let (data, roster) = inputs();
    let mut request = joint_request("mission", true, json!({"kind":"score"}));
    request.strategy = Strategy::Exhaustive;
    let oracle = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(oracle.completion, Completion::Complete);
    request.strategy = Strategy::BranchAndBound;
    let complete = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(complete.results, oracle.results);
    let phases = &complete.telemetry.phases;
    let compilations: Vec<_> = phases
        .iter()
        .enumerate()
        .filter_map(|(index, phase)| (phase.name == "conversionCompile").then_some(index))
        .collect();
    assert!(compilations.len() > 1);
    for pair in compilations.windows(2) {
        assert!(phases[pair[0] + 1..pair[1]].iter().any(|phase| phase.name == "search"));
    }
    for limit in [1, 2, 9] {
        request.limits.max_candidates = Some(limit);
        let stopped = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(stopped.completion, Completion::TimedOut);
        let count = stopped.telemetry.phases.iter().filter(|phase| phase.name == "conversionCompile").count();
        assert!(count <= compilations.len());
        if limit == 1 {
            assert!(count < compilations.len());
        }
        assert!(stopped.telemetry.proof.parts_done < stopped.telemetry.proof.parts);
        let upper: i128 = stopped.telemetry.proof.global_upper_bound.as_ref().unwrap().parse().unwrap();
        assert!(upper >= payoff(&oracle.results[0]));
    }
}
