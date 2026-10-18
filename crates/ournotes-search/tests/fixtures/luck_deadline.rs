//! The request deadline over a certified LUCK search on synthetic inputs.
use super::luck_refinement::inputs;
use ournotes_search::{
    engine,
    search::{Completion, telemetry::Phase},
    types::Optimality,
};

/// Every phase of the request work lies in time order inside the request's elapsed time and, together, accounts for
/// all of it.
fn assert_phases_time_the_request(elapsed_ms: f64, phases: &[Phase]) {
    let timed: f64 = phases.iter().map(|phase| phase.wall_ms).sum();
    assert!(timed <= elapsed_ms + 1.0, "phases time {timed} ms of {elapsed_ms} ms");
    assert!(timed >= elapsed_ms - (elapsed_ms * 0.1 + 50.0), "phases time {timed} ms of {elapsed_ms} ms",);
    assert!(phases.iter().map(|phase| phase.start_ms).is_sorted(), "phases start in time order");
}

#[test]
fn an_expired_deadline_reports_no_proof_and_starts_no_refinement() {
    let (data, roster, mut request) = inputs(611_236, 12, 0);
    request.limits.time_limit_ms = Some(0);
    let out = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(out.completion, Completion::TimedOut);
    assert_eq!(out.optimality, Optimality::Unproven);
    assert_eq!(out.telemetry.lottery_refinement.installed_orders, 0);
    assert!(!out.telemetry.proof.complete);
    assert_phases_time_the_request(out.elapsed_ms, &out.telemetry.phases);
}

#[test]
fn a_tiny_deadline_returns_within_a_bounded_multiple_and_never_claims_a_proof() {
    const LIMIT_MS: u64 = 1;
    let (data, roster, mut request) = inputs(611_236, 12, 0);
    request.limits.time_limit_ms = Some(LIMIT_MS);
    let out = engine::recommend(&data, &roster, &request).unwrap();
    let bound = LIMIT_MS as f64 * 2000.0 + 2_500.0;
    assert!(out.elapsed_ms <= bound, "elapsed {} ms exceeds {bound} ms", out.elapsed_ms);
    assert_phases_time_the_request(out.elapsed_ms, &out.telemetry.phases);
    match out.completion {
        // A request that would have completed in time keeps its proof.
        Completion::Complete => {
            assert_eq!(out.optimality, Optimality::Proven);
            assert!(out.telemetry.proof.complete);
        }
        _ => {
            assert_eq!(out.optimality, Optimality::Unproven);
            assert!(!out.telemetry.proof.complete);
        }
    }
}

#[test]
fn a_generous_deadline_changes_no_result() {
    let (data, roster, request) = inputs(611_236, 12, 0);
    let reference = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(reference.completion, Completion::Complete);
    assert_eq!(reference.optimality, Optimality::Proven);
    let mut limited = request;
    limited.limits.time_limit_ms = Some(600_000);
    let actual = engine::recommend(&data, &roster, &limited).unwrap();
    assert_eq!(actual.completion, Completion::Complete);
    assert_eq!(actual.results, reference.results);
    assert!(actual.telemetry.phases.iter().any(|phase| phase.name == "refine"));
    assert!(actual.telemetry.lottery_refinement.installed_orders > 0);
    assert_phases_time_the_request(actual.elapsed_ms, &actual.telemetry.phases);
}
