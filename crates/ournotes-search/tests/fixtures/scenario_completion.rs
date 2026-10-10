//! Canonical ranking across the public played-live scenes and payoff contracts.
use super::{EVENT_ID, data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, scenario::MultiplayerScorePolicy};
use serde_json::{Value, json};

const SCENES: [(&str, i64, bool); 5] =
    [("free", 10, false), ("mission", 10, true), ("challenge", 70, true), ("battle", 10, true), ("arena", 80, true)];

fn request(scene: (&str, i64, bool), metric: Value, mixed: bool) -> RecommendationRequest {
    let (kind, music, gekisou) = scene;
    let explicit_life = metric["kind"] == "scoreAndLifeAtLeast";
    let mut wire = joint_request_json(kind, gekisou, metric);
    wire["scenario"]["musicId"] = json!(music);
    wire["context"]["eventPayoff"]["consumedCount"] = json!(if kind == "challenge" { 201 } else { 1 });
    if matches!(kind, "battle" | "arena") {
        wire["networkConfirmations"] = json!(
            (0..3)
                .map(|range| json!({
                    "frame":0,"range":range,"rank":1,"percent":10
                }))
                .collect::<Vec<_>>()
        );
        wire["context"]["eventPayoff"]["multiplayerScorePolicy"] =
            serde_json::to_value(MultiplayerScorePolicy::SameScore { players: 3 }).unwrap();
    }
    if mixed || explicit_life {
        wire["execution"]["play"] = json!({"kind":"stream","stream":{
            "frames":(0..=2000).step_by(20).collect::<Vec<_>>(),
            "judged":(1..=12).map(|id| json!([id*5,id,if mixed { match id%6 {0=>1,2=>4,_=>5} } else { 5 },id*100]))
                .collect::<Vec<_>>()
        }});
    }
    serde_json::from_value(wire).unwrap()
}

fn metrics(kind: &str) -> Vec<Value> {
    let mut metrics = vec![
        json!({"kind":"score"}),
        json!({"kind":"scoreAtLeast","threshold":400_000}),
        json!({"kind":"cappedScore","threshold":400_000}),
        json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":500}),
        json!({"kind":"clientEventPoints","eventId":EVENT_ID}),
        json!({"kind":"rankedEventItems","eventId":EVENT_ID,"resourceType":11,"resourceId":9}),
    ];
    if kind != "challenge" {
        metrics.push(json!({"kind":"clientChallengePoints","eventId":EVENT_ID}));
    }
    metrics
}

fn inputs() -> (DeckData, Roster) {
    let synth = synthetic_master(6, 1, 5);
    (
        DeckData::from_json(&data_document(&synth, 6, 1, 5).to_string()).unwrap(),
        Roster::from_json(&roster_document(6, 1, 5).to_string()).unwrap(),
    )
}

#[test]
fn scene_payoff_matrix_matches_full_enumeration() {
    let (data, roster) = inputs();
    let mut cases = 0;
    for scene in SCENES {
        for mixed in [false, true] {
            for metric in metrics(scene.0) {
                let mut request = request(scene, metric.clone(), mixed);
                request.strategy = Strategy::Exhaustive;
                request.k = 100;
                let oracle = engine::recommend(&data, &roster, &request).unwrap();
                assert_eq!(oracle.completion, Completion::Complete);
                assert_eq!(oracle.optimality, Optimality::Proven);
                assert_eq!(oracle.results.len(), 12);
                request.strategy = Strategy::BranchAndBound;
                for k in [1, 5] {
                    request.k = k;
                    let bounded = engine::recommend(&data, &roster, &request).unwrap();
                    assert_eq!(bounded.completion, Completion::Complete, "{scene:?} {metric} mixed={mixed}");
                    assert_eq!(bounded.optimality, Optimality::Proven);
                    assert_eq!(bounded.results, oracle.results[..k], "{scene:?} {metric} mixed={mixed} k={k}");
                    assert_eq!(bounded.telemetry.proof.parts_done, bounded.telemetry.proof.parts);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 68);
}

#[test]
fn scene_constraint_matrix_preserves_full_domain_stops() {
    let (data, roster) = inputs();
    for scene in SCENES {
        for required in [1, 6] {
            let mut request = request(scene, json!({"kind":"score"}), true);
            request.constraints.include_members = vec![required];
            request.k = 100;
            request.strategy = Strategy::Exhaustive;
            let oracle = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(oracle.completion, Completion::Complete);
            assert_eq!(oracle.results.len(), 6);
            request.strategy = Strategy::BranchAndBound;
            request.k = 5;
            let complete = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(complete.results, oracle.results[..5]);
            request.limits.max_candidates = Some(1);
            let stopped = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(stopped.completion, Completion::TimedOut);
            assert_eq!(stopped.optimality, Optimality::Unproven);
            for result in &stopped.results {
                assert!(oracle.results.contains(result));
            }
            let optimum: i128 = oracle.results[0].expected_payoff.as_ref().unwrap().numerator.parse().unwrap();
            let upper = stopped.telemetry.proof.global_upper_bound.as_ref().expect("complete-domain upper bound");
            assert!(upper.parse::<i128>().unwrap() >= optimum);
        }
    }
}
