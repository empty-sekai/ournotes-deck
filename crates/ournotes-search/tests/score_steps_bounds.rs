//! Played solo Live payoffs that step with the score: ranked by deck under the score cap, the branch-and-bound
//! Top-K equals full exhaustive play.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, Synth, replace_table, roster, set_column, short_chart, synth_snaps};
use ournotes_search::search::{Completion, Constraints, Objective, PlayInput, SearchRequest, solve_physical};
use ournotes_search::types::{Limits, Metric, Optimality, RecommendationOutcome, SimulationInput, Strategy};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::scenario::{ContextInput, ResolvedContext, Scenario};
use serde_json::json;

const EVENT: i64 = 7;

/// Seven one-character members and two Snaps; point effects on two members and one Snap. Result ranks 3 and 4 start
/// at `middle` and `high`.
fn fixture(middle: i32, high: i32) -> Synth {
    let mut synth = synth_snaps(&mut Rng::new(7201), 7, 2, &[3]);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_characterID"] = row["_id"].clone();
        row["_cardType"] = json!(1);
        row["_leaderSkillID"] = json!(4);
        row["_liveSkillID"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_liveScoreRankGroup"] = json!(1));
    replace_table(
        &mut synth,
        "MasterEvent",
        json!([{"_id":EVENT,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}]),
    );
    let effect = |id: i64, constraint: i64, card: (&str, i64), value: i64| {
        let mut row = json!({"_id":id,"_eventId":EVENT,"_eventBonusType":0,"_resourceTypeConstraint":constraint,
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
    replace_table(
        &mut synth,
        "MasterLiveScoreRank",
        json!([
            {"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0},
            {"_id":2,"_group":1,"_liveScoreRank":3,"_requiredScore":middle},
            {"_id":3,"_group":1,"_liveScoreRank":4,"_requiredScore":high}
        ]),
    );
    for (table, group, values) in
        [("MasterLiveEventPoint", 1, [10, 30, 1000]), ("MasterLiveChallengePoint", 0, [2, 5, 50])]
    {
        replace_table(
            &mut synth,
            table,
            json!([
                {"_id":1,"_group":group,"_scoreRank":2,"_value":values[0]},
                {"_id":2,"_group":group,"_scoreRank":3,"_value":values[1]},
                {"_id":3,"_group":group,"_scoreRank":4,"_value":values[2]}
            ]),
        );
    }
    synth
}

fn input() -> ContextInput {
    serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[EVENT],"capturedJstTicks":50},
        "resultClock":{"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100},
        "eventPayoff":{
            "consumedCount":0,
            "localEvents":[{"eventId":EVENT,"points":123,"challengePoints":37,"added":[]}],
            "eventWindows":[{"eventId":EVENT,"startJstTicks":90,"endJstTicks":110}]
        }
    }))
    .unwrap()
}

fn request(context: &ResolvedContext, k: usize) -> SearchRequest {
    let (mut chart, mut judgement_types) = short_chart(&mut Rng::new(7203), 8, false);
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

fn run(synth: &Synth, metric: &Metric, strategy: Strategy, k: usize) -> RecommendationOutcome {
    let master = synth.master();
    let owned = roster(&mut Rng::new(7202), &master);
    let input = input();
    let context = input.resolve(&master, Scenario::Free(10), Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    let limits = Limits { time_limit_ms: None, max_candidates: None, cache_entries: 32 };
    let out = solve_physical(
        &pool,
        &request(&context, k),
        metric,
        input.event_payoff.as_ref(),
        &limits,
        &strategy,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(out.completion, Completion::Complete);
    assert_eq!(out.optimality, Optimality::Proven);
    out
}

/// Branch-and-bound against exhaustive play: (rankings, shortfalls, handed over).
fn compare(synth: &Synth, metric: Metric) -> (usize, usize, bool) {
    let bounded = run(synth, &metric, Strategy::BranchAndBound, 5);
    let exhaustive = run(synth, &metric, Strategy::Exhaustive, 5);
    assert_eq!(bounded.results, exhaustive.results, "{metric:?}: full canonical Top-K including ties");
    let bounds = &bounded.telemetry.environment.bounds;
    assert!(bounds.compiled && bounds.fallback.is_none(), "{:?}", bounds.fallback);
    let setup = bounds.deck_payoff.as_ref().expect("deck payoff ranking");
    assert!(setup.score_cap.is_some(), "{:?}", setup.refusal);
    assert!(bounded.telemetry.joint.modules["deckPayoff"].checks > 0);
    (setup.rounds, setup.shortfalls, setup.handed_over)
}

#[test]
fn score_steps_rank_by_deck_and_keep_the_exhaustive_top_k() {
    // The score cap does not read the result ranks. Rank 3 starts just above the lowest order score among the five
    // most powerful decks, and at least five of the ten most powerful decks have every order at or above it; no deck
    // reaches rank 4.
    let provisional = fixture(1, 2);
    let points = Metric::ClientEventPoints { event_id: EVENT };
    let cap: i64 = run(&provisional, &points, Strategy::BranchAndBound, 5)
        .telemetry
        .environment
        .bounds
        .deck_payoff
        .as_ref()
        .and_then(|setup| setup.score_cap.as_ref())
        .expect("score cap")
        .parse()
        .unwrap();
    let scored = run(&provisional, &Metric::Score, Strategy::Exhaustive, 100);
    assert!(scored.results.iter().flat_map(|team| &team.order_outcomes).all(|&(_, score, _)| i64::from(score) <= cap));
    // Every order scores, so this target pays one for every deck and the Top-K is the canonical power order.
    let by_power = run(&provisional, &Metric::ScoreAtLeast { threshold: 1 }, Strategy::Exhaustive, 10);
    let lows: Vec<i32> = by_power
        .results
        .iter()
        .map(|team| team.order_outcomes.iter().map(|&(_, score, _)| score).min().unwrap())
        .collect();
    assert!(lows.iter().all(|&low| low >= 1));
    let middle = lows[..5].iter().min().unwrap() + 1;
    assert!(lows.iter().filter(|&&low| low >= middle).count() >= 5, "the fixture must pass five decks early: {lows:?}");
    let high = i32::try_from(cap + 1).unwrap();
    let synth = fixture(middle, high);

    // The most powerful decks bound the target at one; those with a lower order fall back and the ranking settles.
    let (rounds, shortfalls, handed_over) = compare(&synth, Metric::ScoreAtLeast { threshold: middle });
    assert!(rounds > 1 && shortfalls > 0 && !handed_over, "{rounds} {shortfalls} {handed_over}");
    for metric in [points, Metric::ClientChallengePoints { event_id: EVENT }] {
        compare(&synth, metric);
    }

    // No order reaches the target: every deck pays zero, and only the first K by power are played.
    let unreachable = Metric::ScoreAndLifeAtLeast { threshold: high, min_final_life: 1 };
    assert_eq!(compare(&synth, unreachable.clone()), (1, 0, false));
    let bounded = run(&synth, &unreachable, Strategy::BranchAndBound, 5);
    assert!(bounded.telemetry.leaves.simulations <= 120 * 5);
    assert!(bounded.results.iter().all(|team| team.expected_payoff.as_ref().unwrap().numerator == "0"));
}
