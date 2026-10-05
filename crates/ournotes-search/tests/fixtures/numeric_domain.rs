//! Synthetic numeric-domain regressions. These preserve the evaluator's wrapping
//! arithmetic; they do not claim that these extreme bonuses occur in game data.

use super::common::set_column;
use super::{
    EVENT_ID, SCORE_ID, context_document, data_document, joint_request_json, roster_document, synthetic_master,
};
use ournotes_search::{
    engine, handler,
    search::{self, Completion, telemetry::Traversal},
    types::{ExitReason, Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, pool::Pool, power::BP_UNIT};
use serde_json::{Value, json};

const WRAPPED_FLAT_BONUS: i64 = 970_298_738_277_122_416;
const TEAMS: usize = 31; // No Snap, one of two Snaps in five slots, or both: 1 + 10 + 20.

fn inputs(wrapped: bool) -> (DeckData, Roster) {
    let mut synth = synthetic_master(5, 2, 5);
    if wrapped {
        set_column(&mut synth, "MasterCharacterRank", &mut |row| {
            if row["_rank"] == 10 {
                row["_bonus"] = json!(WRAPPED_FLAT_BONUS);
            }
        });
    }
    let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    (data, roster)
}

fn requests() -> [(&'static str, RecommendationRequest); 3] {
    let live = joint_request_json("free", false, json!({"kind":"score"}));
    let mut power = joint_request_json("free", false, json!({"kind":"power"}));
    power["execution"] = json!({"kind":"power","musicId":10,"eventParameter":false});
    power["context"] = Value::Null;
    let mut skip = joint_request_json("free", false, json!({"kind":"clientEventPoints","eventId":EVENT_ID}));
    skip["execution"] = json!({"kind":"skip","scoreId":SCORE_ID});
    skip["context"] = context_document(true, false, false);
    [("live-score", live), ("power", power), ("skip-event-points", skip)].map(|(name, mut value)| {
        value["constraints"]["leader"] = json!(1);
        value["k"] = json!(TEAMS);
        (name, serde_json::from_value(value).unwrap())
    })
}

#[test]
fn nonintegral_flat_bonus_keeps_native_wrapping_and_fractional_carry() {
    let (data, roster) = inputs(true);
    let pool = Pool::new(&data.master, &roster).unwrap();
    let deck = pool.deck([2, 3, 1, 4, 5], [None; 5], [0, 1, 2, 3, 4]).unwrap();
    let song = pool.song(10).unwrap();
    let native = pool.deck_power(&deck, Some(&song), false).unwrap();

    assert_eq!(WRAPPED_FLAT_BONUS.wrapping_mul(BP_UNIT), 9_984);
    for slot in &native.slots {
        assert_eq!(slot.character_rank.to_array(), [9_984; 3]);
        assert_eq!(slot.total.to_array().map(|stat| stat % BP_UNIT), [9_984; 3]);
    }
    // The native evaluator sums five slots per stat before taking whole points.
    // Flooring each slot's three-stat BP sum first loses two carried points.
    let prematurely_floored: i64 = native.slots.iter().map(|slot| slot.total.bp_total() / BP_UNIT).sum();
    assert_eq!(i64::from(native.power()), prematurely_floored + 2);
}

#[test]
fn nonintegral_power_falls_back_on_all_three_routes_without_dropping_teams() {
    let (data, roster) = inputs(true);
    for (name, mut request) in requests() {
        // Build separately so the guard must preserve the actual resource IDs,
        // not merely report unchanged candidate counts in the final telemetry.
        let built = handler::build_card_pool(&data, &roster, &request).unwrap();
        assert!(built.domain().is_feasible(), "{name}");
        assert_eq!(
            built.domain().members().iter().map(|&index| built.pool().members[index].id).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5],
            "{name}"
        );
        assert_eq!(
            built.domain().snaps().iter().map(|&index| built.pool().snaps[index].id).collect::<Vec<_>>(),
            [1, 2],
            "{name}"
        );
        assert_eq!(built.domain().leader().map(|index| built.pool().members[index].id), Some(1), "{name}");
        let bounded = search::recommend_built(&built).unwrap();
        assert_eq!(bounded.completion, Completion::Complete, "{name}");
        assert_eq!(bounded.optimality, Optimality::Proven, "{name}");
        assert_eq!(bounded.exit_reason, ExitReason::Exhausted, "{name}");
        assert_eq!(bounded.telemetry.environment.traversal, Traversal::Exhaustive, "{name}");
        assert!(!bounded.telemetry.environment.bounds.compiled, "{name}");
        assert!(
            bounded
                .telemetry
                .environment
                .bounds
                .fallback
                .as_deref()
                .is_some_and(|reason| reason.contains("nonintegral")),
            "{name}: {:?}",
            bounded.telemetry.environment.bounds.fallback
        );
        assert_eq!(bounded.telemetry.leaves.visited, TEAMS as u64, "{name}");
        assert_eq!(bounded.results.len(), TEAMS, "{name}");
        let domain = bounded.telemetry.environment.domain.as_ref().unwrap();
        assert_eq!((domain.members, domain.snaps, domain.required, domain.leader_fixed), (5, 2, 1, true), "{name}");

        request.strategy = Strategy::Exhaustive;
        let oracle = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(oracle.completion, Completion::Complete, "{name}");
        assert_eq!(oracle.telemetry.leaves.visited, TEAMS as u64, "{name}");
        // K spans the complete domain, including None and both unique Snaps, so
        // equality checks every identity, native value, and tie-breaking rank.
        assert_eq!(bounded.results, oracle.results, "{name}");
    }
}

#[test]
fn nonintegral_power_fallback_respects_zero_budget_on_all_three_routes() {
    let (data, roster) = inputs(true);
    for (name, mut request) in requests() {
        request.limits.time_limit_ms = Some(0);
        let stopped = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(stopped.completion, Completion::TimedOut, "{name}");
        assert_eq!(stopped.optimality, Optimality::Unproven, "{name}");
        assert_eq!(stopped.exit_reason, ExitReason::TimeLimit, "{name}");
        assert!(!stopped.telemetry.proof.complete, "{name}");
        assert!(!stopped.telemetry.environment.bounds.compiled, "{name}");
        assert!(
            stopped
                .telemetry
                .environment
                .bounds
                .fallback
                .as_deref()
                .is_some_and(|reason| reason.contains("nonintegral")),
            "{name}: {:?}",
            stopped.telemetry.environment.bounds.fallback
        );
        assert_eq!(stopped.telemetry.leaves.visited, 0, "{name}");
        assert!(stopped.results.is_empty(), "{name}");
    }
}

#[test]
fn integral_resolved_power_keeps_all_three_optimized_routes() {
    let (data, roster) = inputs(false);
    for (name, request) in requests() {
        let bounded = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(bounded.completion, Completion::Complete, "{name}");
        assert_eq!(bounded.optimality, Optimality::Proven, "{name}");
        assert!(bounded.telemetry.environment.bounds.compiled, "{name}");
        assert!(
            bounded.telemetry.environment.bounds.fallback.is_none(),
            "{name}: {:?}",
            bounded.telemetry.environment.bounds.fallback
        );
        assert_eq!(bounded.results.len(), TEAMS, "{name}");
        // Skip event points use the separate deck-payoff builder; ordinary Live
        // score and Power exercise the Joint and TeamPower builders respectively.
        assert_eq!(bounded.telemetry.environment.bounds.deck_payoff.is_some(), name == "skip-event-points", "{name}");
    }
}
