#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, roster, short_chart, synth_snaps};
use ournotes_search::search::{
    Completion, Constraints, Objective, PlayInput, SearchRequest, solve_physical_with_aggregation,
};
use ournotes_search::types::{Aggregation, ExitReason, Limits, Metric, Optimality, SimulationInput, Strategy};
use ournotes_sim::{live::model::JudgementStream, pool::Pool};

#[test]
fn maximum_bounds_preserve_full_team_and_snap_ranking() {
    let mut rng = Rng::new(8613);
    let mut master = synth_snaps(&mut rng, 6, 2, &[2000]).master();
    for member in &mut master.member_cards {
        member.character_id = member.id;
        member.leader_skill_id = 4;
    }
    master.reindex().unwrap();
    let roster = roster(&mut rng, &master);
    let pool = Pool::new(&master, &roster).unwrap();
    let (chart, judgement_types) = short_chart(&mut rng, 8, false);
    let request = SearchRequest {
        objective: Objective::LiveScore {
            score_id: 1004,
            play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
            chart,
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        },
        k: 5,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    };
    let limits = Limits { time_limit_ms: None, max_candidates: None, cache_entries: 32 };
    for metric in [Metric::Score, Metric::CappedScore { threshold: 1 }, Metric::ScoreAtLeast { threshold: 1 }] {
        let run = |strategy| {
            solve_physical_with_aggregation(
                &pool,
                &request,
                &metric,
                None,
                &limits,
                &strategy,
                None,
                &SimulationInput::default(),
                Aggregation::Maximum,
            )
            .unwrap()
        };
        let exhaustive = run(Strategy::Exhaustive);
        let bounded = run(Strategy::BranchAndBound);
        assert_eq!(bounded.completion, Completion::Complete);
        assert_eq!(bounded.optimality, Optimality::Proven);
        assert_eq!(exhaustive.results, bounded.results);
        assert!(bounded.telemetry.environment.bounds.compiled);
        assert!(bounded.telemetry.leaves.evaluated < exhaustive.telemetry.leaves.evaluated);
        assert!(bounded.results.iter().all(|row| row.expected_payoff.is_none() && row.expected_score.is_none()));
        assert!(bounded.results.iter().all(|row| row.objective_value.as_ref().unwrap().denominator == "1"));
    }
    let Objective::LiveScore { chart, .. } = &request.objective else { unreachable!() };
    let skip =
        SearchRequest { objective: Objective::SkipScore { score_id: 1004, chart: chart.clone() }, ..request.clone() };
    let deterministic = |aggregation| {
        solve_physical_with_aggregation(
            &pool,
            &skip,
            &Metric::Score,
            None,
            &limits,
            &Strategy::BranchAndBound,
            None,
            &SimulationInput::default(),
            aggregation,
        )
        .unwrap()
    };
    let expected = deterministic(Aggregation::Expected);
    let maximum = deterministic(Aggregation::Maximum);
    assert_eq!(maximum.completion, Completion::Complete);
    for (expected, maximum) in expected.results.iter().zip(&maximum.results) {
        assert_eq!(maximum.members, expected.members);
        assert_eq!(maximum.snaps, expected.snaps);
        assert_eq!(maximum.objective_value, expected.expected_payoff);
        assert_eq!(maximum.maximum_score, expected.maximum_score);
        assert!(maximum.best_order.is_none());
    }
    let stopped = solve_physical_with_aggregation(
        &pool,
        &request,
        &Metric::Score,
        None,
        &Limits { time_limit_ms: Some(0), ..limits },
        &Strategy::BranchAndBound,
        None,
        &SimulationInput::default(),
        Aggregation::Maximum,
    )
    .unwrap();
    assert_eq!(stopped.completion, Completion::TimedOut);
    assert_eq!(stopped.exit_reason, ExitReason::TimeLimit);
    assert_eq!(stopped.optimality, Optimality::Unproven);
    assert!(stopped.results.is_empty());
}
