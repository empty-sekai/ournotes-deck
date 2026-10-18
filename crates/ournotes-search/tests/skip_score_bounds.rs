//! Skip combined metrics that read the Skip score: the deck payoff bound ranks the canonical teams and the
//! branch-and-bound answer equals full exhaustive play. A score whose monotone power interval cannot be certified is
//! refused, and the request keeps the exhaustive traversal.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, Synth, roster, set_column, synth};
use ournotes_search::search::{Completion, Constraints, Objective, SearchRequest, solve_physical_with_aggregation};
use ournotes_search::types::{
    Aggregation, Limits, Metric, MetricLevel, MetricTerm, Optimality, RecommendationOutcome, SimulationInput, Strategy,
};
use ournotes_sim::live::skip::{Chart, ChartNote};
use ournotes_sim::master::Master;
use ournotes_sim::scenario::{ContextInput, ResolvedContext, Scenario};
use serde_json::json;

const EVENT: i64 = 7;
const SCORE_ID: i64 = 1004;

/// Six members and two Snaps. Member 6 is weak but carries the largest point bonus, so the payoff order is not the
/// power order. `adjustment` is the configured note score adjustment factor.
fn fixture(adjustment: &str) -> Synth {
    let mut data = synth(&mut Rng::new(735), 6, 2);
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        let id = row["_id"].as_i64().unwrap();
        row["_characterID"] = json!(id);
        row["_leaderSkillID"] = json!(4);
        if id == 6 {
            for field in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                row[field] = json!(100);
            }
        }
    });
    set_column(&mut data, "MasterLiveMusic", &mut |row| row["_liveScoreRankGroup"] = json!(1));
    set_column(&mut data, "MasterLiveSettings", &mut |row| {
        if row["_key"] == "note_score_adjustment_factor" {
            row["_value"] = json!(adjustment);
        }
    });
    data.tables.extend([
        ("MasterChallengeMusic".into(), json!([{"_id":70,"_eventId":EVENT,"_liveMusicId":10}])),
        ("MasterEvent".into(), json!([{"_id":EVENT,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}])),
        (
            "MasterEventEffect".into(),
            json!([
                {"_id":1,"_eventId":EVENT,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":6,
                 "_rank1EffectValue":5000,"_rank2EffectValue":5000,"_rank3EffectValue":5000,
                 "_rank4EffectValue":5000,"_rank5EffectValue":5000},
                {"_id":2,"_eventId":EVENT,"_eventBonusType":0,"_resourceTypeConstraint":3,"_supportCardId":1,
                 "_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,
                 "_rank4EffectValue":1000,"_rank5EffectValue":1000}
            ]),
        ),
        ("MasterLiveScoreRank".into(), json!([{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0}])),
        ("MasterLiveEventPoint".into(), json!([{"_id":1,"_group":1,"_scoreRank":2,"_value":100}])),
        ("MasterChallengeLiveEventPoint".into(), json!([{"_id":1,"_group":2,"_scoreRank":2,"_value":300}])),
        (
            "MasterLiveChallengePoint".into(),
            json!([
                {"_id":1,"_scoreRank":2,"_value":5},
                {"_id":2,"_scoreRank":3,"_value":5},
                {"_id":3,"_scoreRank":4,"_value":5}
            ]),
        ),
    ]);
    data
}

fn input() -> ContextInput {
    serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[EVENT],"capturedJstTicks":50},
        "resultClock":{"execution":"skip","serverNowJstTicks":99},
        "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":EVENT,"points":0,"challengePoints":30,"added":[]}],
            "eventWindows":[{"eventId":EVENT,"startJstTicks":90,"endJstTicks":100}]}
    }))
    .unwrap()
}

fn request(master: &Master, context: ResolvedContext, k: usize) -> SearchRequest {
    // Eight ordinary notes: the Skip score reads all of them, so the chart's power response is dense.
    let chart = Chart {
        converted_note_count: 8,
        last_timing_note_ms: 800,
        notes: (1..=8).map(|id| ChartNote { id, time_ms: id * 100, note_type: 1 }).collect(),
        skill_events: Vec::new(),
    };
    assert!(master.live_music_score(SCORE_ID).is_some());
    SearchRequest {
        objective: Objective::SkipScore { score_id: SCORE_ID, chart }.in_scenario(context),
        k,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    }
}

fn run(data: &Synth, metric: &Metric, strategy: Strategy, aggregation: Aggregation) -> RecommendationOutcome {
    let master = data.master();
    let owned = roster(&mut Rng::new(739), &master);
    let input = input();
    let context = input.resolve(&master, Scenario::Free(10), Some(SCORE_ID), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    solve_physical_with_aggregation(
        &pool,
        &request(&master, context.clone(), 5),
        metric,
        input.event_payoff.as_ref(),
        &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 0 },
        &strategy,
        None,
        &SimulationInput::default(),
        aggregation,
    )
    .unwrap()
}

fn points() -> Metric {
    Metric::ClientEventPoints { event_id: EVENT }
}

fn combined(levels: &[&[(Metric, u32)]]) -> Metric {
    Metric::Combined {
        levels: levels
            .iter()
            .map(|terms| {
                let terms = terms
                    .iter()
                    .map(|(metric, weight)| MetricTerm { metric: metric.clone(), weight: *weight })
                    .collect();
                MetricLevel::new(terms)
            })
            .collect(),
    }
}

/// Branch-and-bound ranks the canonical teams with the deck payoff bound, and its whole answer equals the
/// exhaustive reference.
fn assert_deck_payoff_ranking(data: &Synth, metric: &Metric, aggregation: Aggregation) {
    let bounded = run(data, metric, Strategy::BranchAndBound, aggregation);
    let exhaustive = run(data, metric, Strategy::Exhaustive, aggregation);
    for out in [&bounded, &exhaustive] {
        assert_eq!(out.completion, Completion::Complete, "{metric:?} {aggregation:?}");
        assert_eq!(out.optimality, Optimality::Proven, "{metric:?} {aggregation:?}");
    }
    let bounds = &bounded.telemetry.environment.bounds;
    assert!(bounds.compiled && bounds.fallback.is_none(), "{:?}", bounds.fallback);
    let setup = bounds.deck_payoff.as_ref().expect("a deck payoff bound");
    assert!(setup.refusal.is_none(), "{:?}", setup.refusal);
    assert!(bounded.telemetry.joint.modules["deckPayoff"].checks > 0, "{metric:?} {aggregation:?}");
    assert_eq!(bounded.results.len(), 5, "{metric:?} {aggregation:?}");
    assert_eq!(bounded.results, exhaustive.results, "{metric:?} {aggregation:?}");
}

#[test]
fn skip_combined_score_requests_are_ranked_by_deck_payoff() {
    let data = fixture("2.5");
    for aggregation in [Aggregation::Expected, Aggregation::Maximum] {
        // One weighted level, then the same terms split over two levels.
        assert_deck_payoff_ranking(&data, &combined(&[&[(Metric::Score, 1), (points(), 1)]]), aggregation);
        assert_deck_payoff_ranking(&data, &combined(&[&[(Metric::Score, 1), (points(), 1000)]]), aggregation);
        assert_deck_payoff_ranking(&data, &combined(&[&[(points(), 1), (Metric::Score, 1)]]), aggregation);
        assert_deck_payoff_ranking(&data, &combined(&[&[(Metric::Score, 1)], &[(points(), 1)]]), aggregation);
        assert_deck_payoff_ranking(&data, &combined(&[&[(points(), 2)], &[(Metric::Score, 1)]]), aggregation);
        assert_deck_payoff_ranking(&data, &combined(&[&[(Metric::Score, 2)], &[(points(), 3)]]), aggregation);
    }
}

#[test]
fn a_skip_score_without_a_provable_monotone_interval_falls_back() {
    // This note score adjustment factor sends the power-to-score chain past the binary32 and Int32 ranges at the top
    // of the compiled power interval, so the Skip score cannot be certified nondecreasing there. No deck payoff bound
    // is built and the request keeps the exhaustive traversal.
    let data = fixture("3e38");
    let metric = combined(&[&[(Metric::Score, 1), (points(), 1)]]);
    for aggregation in [Aggregation::Expected, Aggregation::Maximum] {
        let bounded = run(&data, &metric, Strategy::BranchAndBound, aggregation);
        let exhaustive = run(&data, &metric, Strategy::Exhaustive, aggregation);
        assert_eq!(bounded.completion, Completion::Complete, "{aggregation:?}");
        assert_eq!(bounded.optimality, Optimality::Proven, "{aggregation:?}");
        assert_eq!(bounded.results, exhaustive.results, "{aggregation:?}");
        let bounds = &bounded.telemetry.environment.bounds;
        assert!(!bounds.compiled && bounds.fallback.is_none(), "{aggregation:?}");
        let setup = bounds.deck_payoff.as_ref().expect("a deck payoff refusal");
        let refusal = setup.refusal.as_deref().expect("a refusal reason");
        assert!(refusal.contains("score chain"), "{refusal}");
    }
}
