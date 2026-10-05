//! Played-Live selected item rewards: the deck-determined branch-and-bound Top-K equals full exhaustive play.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, Synth, replace_table, roster, set_column, short_chart, synth_snaps};
use ournotes_search::search::{Completion, Constraints, Objective, PlayInput, SearchRequest, solve_physical};
use ournotes_search::types::{Limits, Metric, Optimality, SimulationInput, Strategy};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::scenario::{ContextInput, ResolvedContext, Scenario};
use serde_json::json;
use std::collections::HashSet;

const EVENT: i64 = 7;
const CHALLENGE: i64 = 70;

/// Seven one-character members and two Snaps; item effects on three members and on both Snaps.
fn fixture() -> Synth {
    let mut synth = synth_snaps(&mut Rng::new(7101), 7, 2, &[3]);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_characterID"] = row["_id"].clone();
        row["_cardType"] = json!(1);
        row["_leaderSkillID"] = json!(4);
        row["_liveSkillID"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_liveScoreRankGroup"] = json!(1));
    replace_table(
        &mut synth,
        "MasterChallengeMusic",
        json!([{"_id":CHALLENGE,"_eventId":EVENT,"_liveMusicId":10,"_musicType":1}]),
    );
    replace_table(
        &mut synth,
        "MasterEvent",
        json!([{"_id":EVENT,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}]),
    );
    let effect = |id: i64, constraint: i64, card: (&str, i64), value: i64| {
        let mut row = json!({"_id":id,"_eventId":EVENT,"_eventBonusType":1,"_resourceTypeConstraint":constraint,
            "_rank1EffectValue":value,"_rank2EffectValue":value,"_rank3EffectValue":value,
            "_rank4EffectValue":value,"_rank5EffectValue":value});
        if card.1 > 0 {
            row[card.0] = json!(card.1);
        }
        row
    };
    replace_table(
        &mut synth,
        "MasterEventEffect",
        json!([
            effect(1, 2, ("_memberCardId", 2), 2500),
            effect(2, 2, ("_memberCardId", 6), 4000),
            effect(3, 2, ("_memberCardId", 7), 1000),
            effect(4, 3, ("_supportCardId", 1), 1500),
            effect(5, 3, ("_supportCardId", 2), 3500),
        ]),
    );
    replace_table(
        &mut synth,
        "MasterLiveScoreRank",
        json!([{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0}]),
    );
    for (table, group) in
        [("MasterLiveEventPoint", 1), ("MasterChallengeLiveEventPoint", 2), ("MasterLiveChallengePoint", 0)]
    {
        replace_table(&mut synth, table, json!([{"_id":1,"_group":group,"_scoreRank":2,"_value":10}]));
    }
    replace_table(
        &mut synth,
        "MasterLiveEventReward",
        json!([
            {"_id":11,"_resourceType":4,"_resourceId":88,"_resourceCount":30},
            {"_id":12,"_resourceType":4,"_resourceId":88,"_resourceCount":70},
            {"_id":13,"_resourceType":1,"_resourceId":5,"_resourceCount":100}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterChallengeLiveEventReward",
        json!([{"_id":11,"_resourceType":4,"_resourceId":88,"_resourceCount":20}]),
    );
    synth
}

fn input(scenario: Scenario) -> ContextInput {
    serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[EVENT],"capturedJstTicks":50},
        "resultClock":{"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100},
        "eventPayoff":{
            "consumedCount":0,
            "localEvents":[{"eventId":EVENT,"points":123,"challengePoints":37,"added":[]}],
            "eventWindows":if matches!(scenario, Scenario::Challenge(_)) { json!([]) } else {
                json!([{"eventId":EVENT,"startJstTicks":90,"endJstTicks":110}])
            },
            "selectedRewards":[{"eventId":EVENT,"rewardId":11},{"eventId":EVENT,"rewardId":12},{"eventId":EVENT,"rewardId":13}]
        }
    }))
    .unwrap()
}

fn request(context: &ResolvedContext, k: usize) -> SearchRequest {
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
    SearchRequest {
        objective: Objective::LiveScore {
            score_id: 1004,
            play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
            chart,
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        }
        .in_scenario(context.clone()),
        k,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    }
}

fn compare(scenario: Scenario, k: usize) -> usize {
    let synth = fixture();
    let master = synth.master();
    let owned = roster(&mut Rng::new(7102), &master);
    let input = input(scenario);
    let event_input = input.event_payoff.as_ref().unwrap();
    let context = input.resolve(&master, scenario, Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    let request = request(&context, k);
    let metric = Metric::ConditionalClientEventItems { event_id: EVENT, resource_type: 4, resource_id: 88 };
    let limits = Limits { time_limit_ms: None, max_candidates: None, cache_entries: 32 };
    let run = |strategy| {
        solve_physical(
            &pool,
            &request,
            &metric,
            Some(event_input),
            &limits,
            &strategy,
            None,
            &SimulationInput::default(),
        )
        .unwrap()
    };
    let bounded = run(Strategy::BranchAndBound);
    let exhaustive = run(Strategy::Exhaustive);
    for out in [&bounded, &exhaustive] {
        assert_eq!(out.completion, Completion::Complete);
        assert_eq!(out.optimality, Optimality::Proven);
        assert_eq!(out.results.len(), k);
    }
    let bounds = &bounded.telemetry.environment.bounds;
    assert!(bounds.compiled && bounds.fallback.is_none(), "{:?}", bounds.fallback);
    let module = &bounded.telemetry.joint.modules["deckPayoff"];
    assert!(module.checks > 0 && module.pruned > 0);
    assert!(bounded.telemetry.leaves.simulations <= 120 * k as u64, "only the final Top-K is played");
    assert_eq!(bounded.results, exhaustive.results, "{scenario:?}: full canonical Top-K including ties");
    let payoffs: HashSet<_> =
        bounded.results.iter().map(|team| team.expected_payoff.as_ref().unwrap().numerator.clone()).collect();
    payoffs.len()
}

#[test]
fn played_item_rewards_keep_the_exhaustive_canonical_top_k() {
    for scenario in [Scenario::Free(10), Scenario::Challenge(CHALLENGE)] {
        // Sixty teams (three layouts, twenty Snap placements) share the best payoff; K = 100 crosses a level.
        assert_eq!(compare(scenario, 5), 1);
        assert!(compare(scenario, 100) > 1);
    }
}
