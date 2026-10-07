#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, roster, set_column, synth_snaps};
use ournotes_search::search::{
    Completion, Constraints, Objective, PlayInput, SearchRequest,
    expectation::{PhysicalDeck, context},
    solve_physical_with_aggregation,
    uniform::all_orders,
};
use ournotes_search::types::{Aggregation, Limits, Metric, Optimality, SimulationInput, Strategy};
use ournotes_sim::{
    live::{
        model::JudgementStream,
        skip::{Chart, ChartNote, SkillEvent},
    },
    pool::Pool,
};
use serde_json::json;

#[test]
fn random_live_support_preserves_exact_ranking_and_cache_admission() {
    let mut rng = Rng::new(4537);
    let mut synth = synth_snaps(&mut rng, 5, 1, &[10]);
    set_column(&mut synth, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    set_column(&mut synth, "MasterLeaderSkillEffect", &mut |row| row["_effectValue"] = json!(0));
    set_column(&mut synth, "MasterSupportSkillEffect", &mut |row| {
        if row["_supportSkillID"] == 10 {
            row["_skillTriggerConditionGroup"] = json!(53);
            row["_skillConditionGroup"] = json!(66);
            row["_effectExecuteLimitCount"] = json!(1);
            row["_effectValue"] = json!(100_000);
        }
    });
    let master = synth.master();
    let roster = roster(&mut rng, &master);
    let pool = Pool::new(&master, &roster).unwrap();
    let chart = Chart {
        converted_note_count: 2,
        last_timing_note_ms: 200,
        notes: vec![ChartNote { id: 1, time_ms: 100, note_type: 1 }, ChartNote { id: 2, time_ms: 200, note_type: 1 }],
        skill_events: (0..5).map(|index| SkillEvent { index, time_ms: 0 }).collect(),
    };
    let request = SearchRequest {
        objective: Objective::LiveScore {
            score_id: 1004,
            play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types: vec![1; 2] },
            chart,
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        },
        k: 30,
        constraints: Constraints::default(),
        time_limit: None,
    };
    let run = |cache_entries| {
        solve_physical_with_aggregation(
            &pool,
            &request,
            &Metric::Score,
            None,
            &Limits { time_limit_ms: None, max_candidates: None, cache_entries },
            &Strategy::Exhaustive,
            None,
            &SimulationInput::default(),
            Aggregation::Maximum,
        )
        .unwrap()
    };
    let uncached = run(0);
    let cached = run(64);
    assert_eq!(cached.completion, Completion::Complete);
    assert_eq!(cached.optimality, Optimality::Proven);
    assert_eq!(cached.results.len(), 30);
    assert_eq!(cached.results, uncached.results);
    assert_eq!(cached.telemetry.caches.programs.peak_entries, 0);
    assert_eq!(cached.telemetry.caches.program_orders_reused, 0);
    assert_eq!(cached.telemetry.caches.team_scores.peak_entries, 1);
    assert!(cached.telemetry.caches.program_admissions.hits > 0);

    // One binary skill predicate is read once per Snap-bearing order. Setting its threshold to each
    // endpoint gives fresh native executions of the complete two-outcome support.
    let mut endpoint_masters = [master.clone(), master.clone()];
    for (endpoint, master) in endpoint_masters.iter_mut().enumerate() {
        master.skill_conditions.iter_mut().find(|condition| condition.id == 76).unwrap().condition_values =
            vec![if endpoint == 0 { 0 } else { 100 }];
        master.reindex().unwrap();
    }
    for row in &cached.results {
        let physical = PhysicalDeck {
            members: row.members.map(|id| pool.member_index(id).unwrap()),
            snaps: row.snaps.map(|id| id.map(|id| pool.snap_index(id).unwrap())),
        };
        let random_input = context(&pool, &physical, &request.objective).unwrap();
        let observed = random_input.simulate_performance_order(&master, [0, 1, 2, 3, 4]).unwrap();
        assert_eq!(observed.model.draws(), u64::from(row.snaps.iter().any(Option::is_some)));
        let mut best = None::<(i32, std::cmp::Reverse<[usize; 5]>)>;
        let mut support = std::collections::BTreeSet::new();
        for endpoint_master in &endpoint_masters {
            let endpoint_pool = Pool::new(endpoint_master, &roster).unwrap();
            let input = context(&endpoint_pool, &physical, &request.objective).unwrap();
            for order in all_orders() {
                let native = input.simulate_performance_order(endpoint_master, order).unwrap();
                assert_eq!(native.model.draws(), observed.model.draws());
                support.insert(native.final_score);
                let candidate = (native.final_score, std::cmp::Reverse(order));
                best = Some(best.map_or(candidate, |current| current.max(candidate)));
            }
        }
        if row.snaps.iter().any(Option::is_some) {
            assert!(support.len() > 1);
        }
        let (score, std::cmp::Reverse(order)) = best.unwrap();
        assert_eq!(row.maximum_score, Some(score));
        assert_eq!(row.objective_value.as_ref().unwrap().numerator, score.to_string());
        assert_eq!(row.best_order.as_ref().unwrap().performance_order, order);
    }
}
