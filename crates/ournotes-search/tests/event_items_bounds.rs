//! Exact-grade event items: branch-and-bound agrees with complete small-domain enumeration.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, Synth, extend_table, replace_table, roster, set_column, short_chart, synth_snaps};
use ournotes_search::search::{
    Completion, Constraints, GekisouObjective, Objective, PlayInput, SearchRequest, SeedSet, solve_physical,
};
use ournotes_search::types::{Limits, Metric, Optimality, RecommendationOutcome, SimulationInput, Strategy};
use ournotes_sim::live::model::{JudgementStream, JustRule};
use ournotes_sim::master::Master;
use ournotes_sim::replay::RankConfirmation;
use ournotes_sim::scenario::{ContextInput, ResolvedContext, Scenario};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const EVENT: i64 = 7;
const CHALLENGE: i64 = 70;
const ARENA: i64 = 80;
const TEAMS: usize = 30;

/// Five member sets around a fixed leader, each with six placements of zero or one Snap.
fn fixture() -> Synth {
    let mut synth = synth_snaps(&mut Rng::new(7101), 6, 1, &[3]);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_characterID"] = row["_id"].clone();
        row["_cardType"] = json!(1);
        row["_leaderSkillID"] = json!(4);
        row["_liveSkillID"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        row["_liveScoreRankGroup"] = json!(1);
        for key in ["_gekisouMission1", "_gekisouMission2", "_gekisouMission3"] {
            row[key] = json!(1);
        }
    });
    extend_table(
        &mut synth,
        "MasterLiveSettings",
        vec![
            json!({"_id":20,"_key":"gekisou_luck_gauge_max","_value":"140"}),
            json!({"_id":21,"_key":"gekisou_luck_gauge_max_rush","_value":"70"}),
            json!({"_id":22,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    replace_table(
        &mut synth,
        "MasterLiveGekisouRankingScoreBonus",
        Value::Array(
            (1..=3)
                .flat_map(|count| {
                    (1..=5).map(move |rank| {
                        json!({
                            "_id":count*10+rank,"_missionPattern":1,"_count":count,"_rank":rank,
                            "_scoreBonusPercent":if rank == 1 {25} else {0}
                        })
                    })
                })
                .collect(),
        ),
    );
    replace_table(
        &mut synth,
        "MasterChallengeMusic",
        json!([{"_id":CHALLENGE,"_eventId":EVENT,"_liveMusicId":10,"_musicType":1}]),
    );
    replace_table(&mut synth, "MasterArenaMusic", json!([{"_id":ARENA,"_liveMusicId":10,"_liveMusicType":1}]));
    replace_table(
        &mut synth,
        "MasterEvent",
        json!([{"_id":EVENT,"_liveEventRewardGroup":37,"_challengeLiveEventRewardGroup":41}]),
    );
    let effect = |id: i64, constraint: i64, card: (&str, i64), value: i64| {
        let mut row = json!({"_id":id,"_eventId":EVENT,"_eventBonusType":1,"_resourceTypeConstraint":constraint,
            "_rank1EffectValue":value,"_rank2EffectValue":value,"_rank3EffectValue":value,
            "_rank4EffectValue":value,"_rank5EffectValue":value});
        row[card.0] = json!(card.1);
        row
    };
    replace_table(
        &mut synth,
        "MasterEventEffect",
        json!([
            effect(1, 2, ("_memberCardId", 2), 2500),
            effect(2, 2, ("_memberCardId", 6), 4000),
            effect(3, 3, ("_supportCardId", 1), 1500),
        ]),
    );
    set_ranks(&mut synth, [10, 20, 30]);
    for table in ["MasterLiveEventPoint", "MasterChallengeLiveEventPoint", "MasterLiveChallengePoint"] {
        replace_table(&mut synth, table, json!([]));
    }
    for (table, group) in [("MasterLiveEventReward", 37), ("MasterChallengeLiveEventReward", 41)] {
        replace_table(
            &mut synth,
            table,
            Value::Array(
                [7, 29, 3, 100]
                    .into_iter()
                    .enumerate()
                    .map(|(i, count)| {
                        json!({
                            "_id":i+1,"_eventGroup":group,"_group":5,"_scoreRank":i+2,"_probability":10000,
                            "_resourceType":4,"_resourceId":if i == 3 { 89 } else { 88 },"_resourceCount":count
                        })
                    })
                    .chain(std::iter::once(json!({
                        "_id":100,"_eventGroup":99,"_group":group,"_scoreRank":2,"_probability":10000,
                        "_resourceType":4,"_resourceId":88,"_resourceCount":500
                    })))
                    .collect(),
            ),
        );
    }
    replace_table(
        &mut synth,
        "MasterLiveMusicBoostBonus",
        json!([
            {"_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":3,"_eventPointRate":17}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterChallengeMusicBoostBonus",
        json!([
            {"_id":1,"_consumedChallengePointCount":400,"_liveMusicRewardRate":2,"_eventPointRate":19}
        ]),
    );
    set_column(&mut synth, "MasterParameter", &mut |row| {
        if row["_id"] == "live_skip_result_score_rank" {
            row["_value"] = json!("C");
        }
    });
    synth
}

fn set_ranks(synth: &mut Synth, thresholds: [i32; 3]) {
    replace_table(
        synth,
        "MasterLiveScoreRank",
        Value::Array(std::iter::once(0).chain(thresholds).enumerate().map(|(i, score)| json!({
            "_id":i+1,"_group":1,"_liveScoreRank":i+2,"_requiredScore":score,"_battleLiveRequiredScore":score
        })).collect()),
    );
}

fn input(scenario: Scenario, skip: bool) -> ContextInput {
    serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[EVENT],"capturedJstTicks":50},
        "resultClock":if skip { json!({"execution":"skip","serverNowJstTicks":100}) }
            else { json!({"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100}) },
        "eventPayoff":{
            "consumedCount":if matches!(scenario, Scenario::Challenge(_)) { 400 } else { 1 },
            "eventWindows":[{"eventId":EVENT,"startJstTicks":90,"endJstTicks":110}],
            "multiplayerScorePolicy":if matches!(scenario, Scenario::Battle(_) | Scenario::Arena(_)) {
                json!({"kind":"sameScore","players":5})
            } else { Value::Null }
        }
    }))
    .unwrap()
}

fn request(master: &Master, context: &ResolvedContext, k: usize, skip: bool) -> SearchRequest {
    let (mut chart, mut judgement_types) = short_chart(&mut Rng::new(7103), 8, false);
    for (i, note) in chart.notes.iter_mut().enumerate() {
        note.time_ms = 80 * (i as i32 + 1);
        note.note_type = 1;
    }
    for (i, event) in chart.skill_events.iter_mut().enumerate() {
        event.time_ms = 120 * i as i32;
    }
    chart.last_timing_note_ms = 640;
    judgement_types.fill(1);
    let objective = if skip {
        Objective::SkipScore { score_id: 1004, chart }
    } else {
        let multiplayer = matches!(context.scenario, Scenario::Battle(_) | Scenario::Arena(_));
        let stream = if multiplayer {
            let rule = JustRule::new(master, &context.gekisou).unwrap();
            JudgementStream::theoretical_best_gekisou(&chart, &judgement_types, &rule).unwrap()
        } else {
            JudgementStream::theoretical_best(&chart)
        };
        Objective::LiveScore {
            score_id: 1004,
            play: PlayInput::Stream { stream, judgement_types },
            chart,
            event: false,
            exclude_snap_skills: false,
            gekisou: multiplayer
                .then(|| GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: context.gekisou.fevers.clone() }),
        }
    };
    SearchRequest {
        objective: objective.in_scenario(context.clone()),
        k,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    }
}

fn run(
    synth: &Synth,
    scenario: Scenario,
    input: &ContextInput,
    skip: bool,
    metric: &Metric,
    k: usize,
    strategy: Strategy,
) -> Result<RecommendationOutcome, ournotes_sim::Error> {
    let master = synth.master();
    let owned = roster(&mut Rng::new(7102), &master);
    let multiplayer = matches!(scenario, Scenario::Battle(_) | Scenario::Arena(_));
    let fevers = if multiplayer { vec![(160, 320)] } else { Vec::new() };
    // Frame 48 is 800 ms on the theoretical 60 Hz clock, after the declared COMBO range.
    let network = multiplayer.then(|| vec![RankConfirmation { frame: 48, range: 0, rank: 1, percent: 25 }]);
    let mut context = input.resolve(&master, scenario, Some(1004), &fevers)?;
    context.rank_confirmations = network.clone();
    let pool = context.pool(&master, &owned)?;
    solve_physical(
        &pool,
        &request(&master, &context, k, skip),
        metric,
        if matches!(metric, Metric::RankedEventItems { .. }) { input.event_payoff.as_ref() } else { None },
        &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 32 },
        &strategy,
        network.as_deref(),
        &SimulationInput::default(),
    )
}

fn target() -> Metric {
    Metric::RankedEventItems { event_id: EVENT, resource_type: 4, resource_id: 88 }
}

fn compare(synth: &Synth, scenario: Scenario, input: &ContextInput, skip: bool) -> RecommendationOutcome {
    let exhaustive = run(synth, scenario, input, skip, &target(), TEAMS, Strategy::Exhaustive).unwrap();
    assert_eq!(exhaustive.results.len(), TEAMS);
    for k in [5, TEAMS] {
        let bounded = run(synth, scenario, input, skip, &target(), k, Strategy::BranchAndBound).unwrap();
        for out in [&bounded, &exhaustive] {
            assert_eq!(out.completion, Completion::Complete);
            assert_eq!(out.optimality, Optimality::Proven);
        }
        let bounds = &bounded.telemetry.environment.bounds;
        assert!(bounds.compiled && bounds.fallback.is_none(), "{:?}", bounds.fallback);
        assert_eq!(bounded.results, exhaustive.results[..k], "{scenario:?}: canonical Top-K including ties");
    }
    exhaustive
}

#[test]
fn played_item_steps_match_complete_domains_with_interior_peaks_and_other_resources() {
    for scenario in [Scenario::Free(10), Scenario::Challenge(CHALLENGE), Scenario::Battle(10), Scenario::Arena(ARENA)] {
        let mut synth = fixture();
        let input = input(scenario, false);
        let scores = run(&synth, scenario, &input, false, &Metric::Score, TEAMS, Strategy::Exhaustive).unwrap();
        let distinct: Vec<_> = scores
            .results
            .iter()
            .flat_map(|r| {
                let summary = r.score_summary.as_ref().unwrap();
                [summary.minimum, summary.maximum]
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert!(distinct.len() >= 4);
        set_ranks(
            &mut synth,
            [distinct[distinct.len() / 3], distinct[2 * distinct.len() / 3], *distinct.last().unwrap()],
        );
        let exhaustive = compare(&synth, scenario, &input, false);
        let payoffs: BTreeSet<_> =
            exhaustive.results.iter().map(|team| team.expected_payoff.as_ref().unwrap().numerator.clone()).collect();
        assert!(payoffs.len() > 1);
    }
}

#[test]
fn skip_items_use_the_fixed_grade_and_reward_rate_without_point_tables_or_balances() {
    let synth = fixture();
    for scenario in [Scenario::Free(10), Scenario::Challenge(CHALLENGE)] {
        let input = input(scenario, true);
        let exhaustive = compare(&synth, scenario, &input, true);
        assert!(exhaustive.results.iter().all(|team| team.expected_payoff.is_some()));
    }
}

#[test]
fn inactive_events_have_zero_item_payoff_without_reward_rows() {
    let mut synth = fixture();
    replace_table(&mut synth, "MasterLiveEventReward", json!([]));
    for skip in [false, true] {
        let mut input = input(Scenario::Free(10), skip);
        input.event_payoff.as_mut().unwrap().event_windows = Some(Vec::new());
        let exhaustive = compare(&synth, Scenario::Free(10), &input, skip);
        assert!(exhaustive.results.iter().all(|team| team.expected_payoff.as_ref().unwrap().numerator == "0"));
    }
}
