//! Synthetic full-domain Challenge EP bound regressions. These are model/oracle checks, not client parity.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, Synth, extend_table, replace_table, roster, set_column, short_chart, synth_snaps};
use ournotes_search::search::{Completion, Constraints, Objective, PlayInput, SearchRequest, solve_physical};
use ournotes_search::types::{Limits, Metric, Optimality, RecommendationOutcome, SimulationInput, Strategy};
use ournotes_sim::cards::Roster;
use ournotes_sim::event;
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::master::Master;
use ournotes_sim::scenario::{ContextInput, ResolvedContext, Scenario};
use serde_json::json;
use std::collections::HashSet;

const EVENT: i64 = 7;
const CHALLENGE: i64 = 70;

fn fixture() -> Synth {
    let mut synth = synth_snaps(&mut Rng::new(7001), 5, 1, &[3]);
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
        json!([
            {"_id":CHALLENGE,"_eventId":EVENT,"_liveMusicId":10,"_musicType":1}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterEvent",
        json!([
            {"_id":EVENT,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterEventEffect",
        json!([
            {"_id":1,"_eventId":EVENT,"_eventBonusType":0,"_resourceTypeConstraint":2,
             "_rank1EffectValue":750,"_rank2EffectValue":750,"_rank3EffectValue":750,
             "_rank4EffectValue":750,"_rank5EffectValue":750},
            {"_id":2,"_eventId":EVENT,"_eventBonusType":0,"_resourceTypeConstraint":3,
             "_rank1EffectValue":500,"_rank2EffectValue":500,"_rank3EffectValue":500,
             "_rank4EffectValue":500,"_rank5EffectValue":500}
        ]),
    );
    set_thresholds(&mut synth, 100, 200);
    for (table, group, values) in [
        ("MasterLiveEventPoint", 1, [13, 47, 3]),
        ("MasterChallengeLiveEventPoint", 2, [20, 90, 5]),
        ("MasterLiveChallengePoint", 0, [2, 11, 1]),
    ] {
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
    // The same consumption key intentionally maps to different native rate tables.
    replace_table(
        &mut synth,
        "MasterLiveMusicBoostBonus",
        json!([
            {"_id":1,"_consumedLiveBoostCount":201,"_eventPointRate":7}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterChallengeMusicBoostBonus",
        json!([
            {"_id":1,"_consumedChallengePointCount":201,"_eventPointRate":3}
        ]),
    );
    synth
}

fn set_thresholds(synth: &mut Synth, middle: i32, high: i32) {
    replace_table(
        synth,
        "MasterLiveScoreRank",
        json!([
            {"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0},
            {"_id":2,"_group":1,"_liveScoreRank":3,"_requiredScore":middle},
            {"_id":3,"_group":1,"_liveScoreRank":4,"_requiredScore":high}
        ]),
    );
}

fn owned(master: &Master) -> Roster {
    let mut result = roster(&mut Rng::new(7002), master);
    for (i, member) in result.members.iter_mut().enumerate() {
        member.live_skill_level = i as i64 + 1;
    }
    result
}

fn input(scenario: Scenario) -> ContextInput {
    serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[EVENT],"capturedJstTicks":50},
        "resultClock":{"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100},
        "eventPayoff":{
            "consumedCount":201,
            "localEvents":[{"eventId":EVENT,"points":123,"challengePoints":37,"added":[]}],
            // Challenge settlement must not accidentally inherit NormalPlayed's one-held-event restriction.
            "eventWindows":if matches!(scenario, Scenario::Challenge(_)) { json!([]) } else {
                json!([{"eventId":EVENT,"startJstTicks":90,"endJstTicks":110}])
            }
        }
    }))
    .unwrap()
}

fn request(context: &ResolvedContext, k: usize) -> SearchRequest {
    let (mut chart, mut judgement_types) = short_chart(&mut Rng::new(7003), 8, false);
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
        constraints: Constraints { leader: Some(3), ..Default::default() },
        time_limit: None,
    }
}

fn unlimited() -> Limits {
    Limits { time_limit_ms: None, max_candidates: None, cache_entries: 32 }
}

/// Calibrate reward steps from all six legal layouts, so the deliberately nonmonotone tiers are actually reached.
/// This only creates the fixture; no order/deck is removed from either search tested below.
fn calibrated_fixture() -> Synth {
    let mut synth = fixture();
    let master = synth.master();
    let owned = owned(&master);
    let input = input(Scenario::Challenge(CHALLENGE));
    let context = input.resolve(&master, Scenario::Challenge(CHALLENGE), Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    let all = solve_physical(
        &pool,
        &request(&context, 6),
        &Metric::Score,
        None,
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(all.completion, Completion::Complete);
    // Fixed leader, all five distinct characters, and either no Snap or the sole Snap in one of five slots.
    assert_eq!(all.results.len(), 6);
    let mut scores: Vec<_> =
        all.results.iter().flat_map(|team| team.order_outcomes.iter().map(|&(_, score, _)| score)).collect();
    scores.sort_unstable();
    scores.dedup();
    assert!(scores.len() >= 6, "fixture must exercise multiple native score outcomes");
    let (middle, high) = (scores[scores.len() / 3], scores[2 * scores.len() / 3]);
    assert!(0 < middle && middle < high && high < *scores.last().unwrap());
    set_thresholds(&mut synth, middle, high);
    synth
}

fn compare_full_k5(synth: &Synth, scenario: Scenario, metric: Metric, expect_bound: bool) -> RecommendationOutcome {
    let master = synth.master();
    let owned = owned(&master);
    let input = input(scenario);
    let event_input = input.event_payoff.as_ref().unwrap();
    let initial_counters = event_input.local_events.clone();
    let context = input.resolve(&master, scenario, Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    assert_eq!(pool.members.len(), 5);
    assert_eq!(pool.members.iter().map(|member| member.character_id).collect::<HashSet<_>>().len(), 5);
    assert_eq!(pool.snaps.len(), 1);
    let request = request(&context, 5);
    let bounded = solve_physical(
        &pool,
        &request,
        &metric,
        Some(event_input),
        &unlimited(),
        &Strategy::BranchAndBound,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(bounded.completion, Completion::Complete);
    assert_eq!(bounded.optimality, Optimality::Proven);
    assert_eq!(bounded.results.len(), 5);
    assert_eq!(bounded.telemetry.environment.bounds.compiled, expect_bound);
    if expect_bound {
        assert!(
            bounded.telemetry.environment.bounds.fallback.is_none(),
            "{:?}",
            bounded.telemetry.environment.bounds.fallback
        );
    } else {
        let reason = bounded.telemetry.environment.bounds.fallback.as_deref().expect("explicit fallback reason");
        assert!(reason.contains("wrapping"), "unexpected fallback: {reason}");
    }
    let exhaustive = solve_physical(
        &pool,
        &request,
        &metric,
        Some(event_input),
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(exhaustive.optimality, Optimality::Proven);
    assert_eq!(exhaustive.results.len(), 5);
    assert_eq!(bounded.results, exhaustive.results, "full canonical Top-K including ties and Snap layouts");
    assert!(bounded.results.iter().any(|team| team.snaps.iter().any(Option::is_some)));
    assert_eq!(event::challenge_point_bonus(&master, 201).unwrap()[4], 3);
    assert_eq!(event::boost_bonus(&master, 201).unwrap()[4], 7);

    for team in &bounded.results {
        assert_eq!(team.members[2], 3);
        assert!(team.members.iter().enumerate().filter(|&(i, _)| i != 2).map(|(_, id)| id).is_sorted());
        assert_eq!(team.order_outcomes.len(), 120);
        assert_eq!(team.order_outcomes.iter().map(|&(order, _, _)| order).collect::<HashSet<_>>().len(), 120);
        let mut payoff_sum = 0i128;
        let mut score_sum = 0i128;
        for &(order, score, payoff) in &team.order_outcomes {
            let mut sorted = order;
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2, 3, 4]);
            let deck = pool.deck(team.members, team.snaps, order).unwrap();
            let native = context.preview_event_points(&pool, &deck, event_input, EVENT, score).unwrap();
            let expected = match &metric {
                Metric::ClientEventPoints { .. } => native.points_for(EVENT),
                Metric::ClientChallengePoints { .. } => native.challenge_points_for(EVENT),
                _ => panic!("this oracle checks only the two point counters"),
            };
            assert_eq!(
                payoff,
                i128::from(expected),
                "settle each native score BEFORE averaging: {scenario:?}, {order:?}"
            );
            if matches!(scenario, Scenario::Challenge(_)) {
                assert_eq!(
                    native.local_events[0].challenge_points, 0,
                    "insufficient initial balance still follows native clamped debit"
                );
                assert_eq!(native.challenge_points_for(EVENT), 0);
            } else if matches!(metric, Metric::ClientChallengePoints { .. }) {
                let rank = event::score_rank(&master, 1, i64::from(score)).unwrap();
                assert_eq!(
                    i64::from(expected),
                    7 * event::music_score_challenge_point(&master, rank).unwrap(),
                    "ordinary Challenge currency must not receive the deck EP bonus"
                );
            }
            payoff_sum += payoff;
            score_sum += i128::from(score);
        }
        let payoff = team.expected_payoff.as_ref().expect("deterministic fixture");
        assert_eq!(payoff.denominator.parse::<i128>().unwrap(), 120);
        assert_eq!(payoff.numerator.parse::<i128>().unwrap(), payoff_sum);
        let score = team.expected_score.as_ref().unwrap();
        assert_eq!(score.denominator.parse::<i128>().unwrap(), 120);
        assert_eq!(score.numerator.parse::<i128>().unwrap(), score_sum);
    }
    assert_eq!(event_input.local_events, initial_counters, "preview/oracle calls must retain input counters");
    bounded
}

#[test]
fn challenge_nonmonotone_and_missing_reward_bounds_preserve_full_canonical_k5() {
    let mut synth = calibrated_fixture();
    let result =
        compare_full_k5(&synth, Scenario::Challenge(CHALLENGE), Metric::ClientEventPoints { event_id: EVENT }, true);
    assert!(result.results.iter().all(|team| team.order_outcomes.iter().any(|&(_, _, payoff)| payoff > 0)));

    // Normal EP and newly earned Challenge currency keep their distinct tables, rate, and bonus semantics.
    for metric in [Metric::ClientEventPoints { event_id: EVENT }, Metric::ClientChallengePoints { event_id: EVENT }] {
        compare_full_k5(&synth, Scenario::Free(10), metric, true);
    }

    replace_table(&mut synth, "MasterChallengeLiveEventPoint", json!([]));
    let missing =
        compare_full_k5(&synth, Scenario::Challenge(CHALLENGE), Metric::ClientEventPoints { event_id: EVENT }, true);
    assert!(missing.results.iter().all(|team| team.order_outcomes.iter().all(|&(_, _, payoff)| payoff == 0)));
    // Equal-payoff teams must still retain all power/physical-ID tie breakers rather than stopping after one team.
    assert_eq!(missing.results.len(), 5);
}

#[test]
fn challenge_wrapping_reward_falls_back_and_matches_full_exhaustive_without_claiming_a_bound() {
    let mut synth = fixture();
    set_column(&mut synth, "MasterChallengeLiveEventPoint", &mut |row| row["_value"] = json!(100_000));
    // 3 * 100000 * (10000 + five member bonuses + a Snap bonus) exceeds signed i32.
    let result =
        compare_full_k5(&synth, Scenario::Challenge(CHALLENGE), Metric::ClientEventPoints { event_id: EVENT }, false);
    assert!(!result.telemetry.environment.bounds.compiled);
    assert!(result.telemetry.environment.bounds.fallback.is_some());
}

#[test]
fn challenge_bonus_domain_is_the_largest_team_bonus() {
    let mut synth = calibrated_fixture();
    extend_table(
        &mut synth,
        "MasterEventEffect",
        vec![json!({"_id":3,"_eventId":EVENT,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":1,
             "_rank1EffectValue":100_000,"_rank2EffectValue":100_000,"_rank3EffectValue":100_000,
             "_rank4EffectValue":100_000,"_rank5EffectValue":100_000})],
    );
    set_column(&mut synth, "MasterChallengeLiveEventPoint", &mut |row| {
        if row["_scoreRank"] == 3 {
            row["_value"] = json!(3000);
        }
    });
    // The largest team bonus is 4 * 750 + 100750 + 500 = 104250, and 3 * 3000 * (104250 + 10000) fits in i32.
    let result =
        compare_full_k5(&synth, Scenario::Challenge(CHALLENGE), Metric::ClientEventPoints { event_id: EVENT }, true);
    assert!(result.results.iter().any(|team| team.order_outcomes.iter().any(|&(_, _, payoff)| payoff > 9000)));
}
