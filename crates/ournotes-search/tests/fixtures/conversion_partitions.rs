//! Canonical ranking and stopped-domain coverage across conversion partitions.
use super::{data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{engine, search::Completion, types::Strategy};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

#[test]
fn conversion_parts_interleave_preparation_with_complete_traversals() {
    let synth = synthetic_master(5, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    let mut request = joint_request("mission", true, json!({"kind":"score"}));
    request.k = 31;
    let actual = engine::recommend(&data, &roster, &request).unwrap();
    request.strategy = Strategy::Exhaustive;
    let reference = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(actual.completion, Completion::Complete);
    assert_eq!(actual.results, reference.results);
    assert_eq!(actual.telemetry.proof.parts_done, actual.telemetry.proof.parts);
    let phases = &actual.telemetry.phases;
    let preparation: Vec<_> = phases
        .iter()
        .enumerate()
        .filter_map(|(index, phase)| (phase.name == "conversionCompile").then_some(index))
        .collect();
    assert!(preparation.len() > 1);
    assert!(
        preparation
            .windows(2)
            .all(|indices| { phases[indices[0] + 1..indices[1]].iter().any(|phase| phase.name == "search") })
    );
}

#[test]
fn conversion_parts_stopped_before_preparation_keep_the_full_domain_bound() {
    let synth = synthetic_master(6, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let mut request = joint_request("mission", true, json!({"kind":"score"}));
    request.strategy = Strategy::Exhaustive;
    let reference = engine::recommend(&data, &roster, &request).unwrap();
    let optimum: i128 = reference.results[0].expected_payoff.as_ref().unwrap().numerator.parse().unwrap();
    request.strategy = Strategy::BranchAndBound;
    request.limits.max_candidates = Some(2);
    let stopped = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(stopped.completion, Completion::TimedOut);
    assert!(stopped.telemetry.proof.parts > 1);
    assert_eq!(stopped.telemetry.proof.parts_done, 0);
    assert!(stopped.telemetry.phases.iter().all(|phase| phase.name != "conversionCompile"));
    let upper: i128 = stopped.telemetry.proof.global_upper_bound.as_ref().unwrap().parse().unwrap();
    assert!(upper >= optimum);
}
