//! Independent canonical Skip PT oracle: enumerate the entire physical domain
//! before deduplicating teams, then use the public native evaluator/payoff.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, Synth, roster, short_chart, synth};
use ournotes_search::search::expectation::{PhysicalDeck, visit_physical_decks};
use ournotes_search::search::{self, Completion, Constraints, Objective, SearchRequest};
use ournotes_search::types::{
    ExitReason, Fraction, Limits, Metric, Optimality, RecommendedDeck, ScoreSummary, SimulationInput, Strategy,
};
use ournotes_sim::{
    pool::Pool,
    scenario::{ContextInput, EventPayoffInput, ResolvedContext, Scenario},
};
use serde_json::json;
use std::collections::HashSet;

fn fixture() -> Synth {
    let mut data = synth(&mut Rng::new(735), 6, 2);
    for (name, rows) in &mut data.tables {
        if name == "MasterParameter" {
            rows.as_array_mut().unwrap().retain(|row| row["_id"] != "live_skip_result_score_rank");
            rows.as_array_mut()
                .unwrap()
                .push(json!({"_id":"live_skip_result_score_rank","_type":"String","_value":"D"}));
        }
        if name == "MasterMemberCard" {
            for (i, row) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                row["_characterID"] = json!(i + 1);
                row["_leaderSkillID"] = json!(4);
                if row["_id"] == 6 {
                    for field in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                        row[field] = json!(100);
                    }
                }
            }
        }
        if name == "MasterLiveMusic" {
            for row in rows.as_array_mut().unwrap() {
                row["_liveScoreRankGroup"] = json!(1);
            }
        }
    }
    data.tables.extend([
        ("MasterChallengeMusic".into(), json!([{"_id":70,"_eventId":7,"_liveMusicId":10}])),
        ("MasterEvent".into(), json!([{"_id":7,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}])),
        ("MasterEventEffect".into(), json!([
            {"_id":1,"_eventId":7,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":6,
             "_rank1EffectValue":5000,"_rank2EffectValue":5000,"_rank3EffectValue":5000,"_rank4EffectValue":5000,"_rank5EffectValue":5000},
            {"_id":2,"_eventId":7,"_eventBonusType":0,"_resourceTypeConstraint":3,"_supportCardId":1,
             "_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,"_rank4EffectValue":1000,"_rank5EffectValue":1000}
        ])),
        ("MasterLiveScoreRank".into(), json!([{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0}])),
        ("MasterLiveEventPoint".into(), json!([{"_id":1,"_group":1,"_scoreRank":2,"_value":100}])),
        ("MasterChallengeLiveEventPoint".into(), json!([{"_id":1,"_group":2,"_scoreRank":2,"_value":300}])),
        ("MasterLiveChallengePoint".into(), json!([
            {"_id":1,"_scoreRank":2,"_value":5},{"_id":2,"_scoreRank":3,"_value":5},{"_id":3,"_scoreRank":4,"_value":5}
        ])),
    ]);
    data
}

/// Hand-calculated fixture payoff. The three explicit configuration cases are
/// intentionally independent of the production enum parser and score ranks.
fn manual_points(pool: &Pool, context: &ResolvedContext, members: [i64; 5], snaps: [Option<i64>; 5], cp: bool) -> i32 {
    let rank = match pool.master.parameter("live_skip_result_score_rank").expect("fixture configuration") {
        "D" => 2,
        "C" => 3,
        "c" => 0,
        other => panic!("unaccounted fixture config {other}"),
    };
    let challenge = matches!(context.scenario, Scenario::Challenge(_));
    let ep = if challenge {
        pool.master.challenge_live_event_points.iter().find(|r| r.group == 2 && r.score_rank == rank).map(|r| r.value)
    } else {
        pool.master.live_event_points.iter().find(|r| r.group == 1 && r.score_rank == rank).map(|r| r.value)
    };
    let Some(ep) = ep else {
        return 0;
    };
    if cp {
        if challenge {
            return 0;
        }
        // consumedCount=0 has rate=1. CP is independent of every EP bonus.
        return pool.master.live_challenge_points.iter().find(|r| r.score_rank == rank).expect("fixture CP row").value
            as i32;
    }
    let mut bonus = 0i32;
    for effect in &pool.master.event_effects {
        assert_eq!(effect.event_bonus_type, 0);
        assert_eq!(
            [
                effect.rank2_effect_value,
                effect.rank3_effect_value,
                effect.rank4_effect_value,
                effect.rank5_effect_value
            ],
            [effect.rank1_effect_value; 4]
        );
        let included = match effect.resource_type_constraint {
            2 => members.contains(&effect.member_card_id),
            3 => snaps.contains(&Some(effect.support_card_id)),
            _ => panic!("fixture contains an unaccounted effect"),
        };
        if included {
            bonus = bonus.wrapping_add(effect.rank1_effect_value as i32);
        }
    }
    (ep as i32).wrapping_mul(bonus.wrapping_add(10000)) / 10000
}

fn input() -> ContextInput {
    serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[7],"capturedJstTicks":50},
        "resultClock":{"execution":"skip","serverNowJstTicks":99},
        "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":7,"points":0,"challengePoints":30,"added":[]}],
            "eventWindows":[{"eventId":7,"startJstTicks":90,"endJstTicks":100}]}
    }))
    .unwrap()
}

fn request(master: &ournotes_sim::master::Master, context: ResolvedContext, k: usize) -> SearchRequest {
    let chart = short_chart(&mut Rng::new(3), 4, false).0;
    // This is the same public chart ID used by the synthetic master.
    assert!(master.live_music_score(1004).is_some());
    SearchRequest {
        objective: Objective::SkipScore { score_id: 1004, chart }.in_scenario(context),
        k,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    }
}

/// This independently sorts only nonleader member/Snap pairs by PUBLIC card ID.
/// It does not call the production canonicalization or truncate physical Top-K.
fn oracle(
    pool: &Pool,
    request: &SearchRequest,
    context: &ResolvedContext,
    input: &EventPayoffInput,
    challenge_points: bool,
) -> Vec<RecommendedDeck> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let visits = visit_physical_decks(pool, &request.constraints, |physical| {
        let mut pairs: Vec<_> =
            [0, 1, 3, 4].into_iter().map(|slot| (physical.members[slot], physical.snaps[slot])).collect();
        pairs.sort_by_key(|&(member, _)| pool.members[member].id);
        let mut deck = PhysicalDeck { members: physical.members, snaps: physical.snaps };
        for (slot, (member, snap)) in [0, 1, 3, 4].into_iter().zip(pairs) {
            deck.members[slot] = member;
            deck.snaps[slot] = snap;
        }
        let members = deck.members.map(|m| pool.members[m].id);
        let snaps = deck.snaps.map(|s| s.map(|s| pool.snaps[s].id));
        if !seen.insert((members, snaps)) {
            return Ok(true);
        }
        let (power, score) = search::evaluate(pool, &deck.as_deck(), &request.objective)?;
        let score = score.expect("Skip score");
        let preview = context.preview_event_points(pool, &deck.as_deck(), input, 7, score)?;
        let pt = if challenge_points { preview.challenge_points_for(7) } else { preview.points_for(7) };
        assert_eq!(
            pt,
            manual_points(pool, context, members, snaps, challenge_points),
            "configured-rank payoff must match hand calculation, independently of final score"
        );
        rows.push(RecommendedDeck {
            members,
            snaps,
            power,
            expected_score: Some(Fraction { numerator: score.to_string(), denominator: "1".into() }),
            expected_payoff: Some(Fraction { numerator: pt.to_string(), denominator: "1".into() }),
            event_rewards: None,
            score_interval: None,
            payoff_interval: None,
            rank_certified: None,
            score_summary: Some(ScoreSummary {
                minimum: score,
                maximum: score,
                p10: score,
                p50: score,
                p90: score,
                target_score: None,
                probability_at_least: None,
                expected_shortfall: None,
            }),
            best_order: None,
            order_outcomes: Vec::new(),
        });
        Ok(true)
    })
    .unwrap();
    assert_eq!(visits, 3720, "complete fixed-leader physical domain, including None and distinct Snap injections");
    assert_eq!(rows.len(), 155, "five member sets times 31 optional two-Snap injections");
    assert_eq!(rows.iter().filter(|r| r.snaps == [None; 5]).count(), 5);
    rows.sort_by(|a, b| {
        b.expected_payoff
            .as_ref()
            .expect("exact oracle payoff")
            .numerator
            .parse::<i128>()
            .unwrap()
            .cmp(&a.expected_payoff.as_ref().expect("exact oracle payoff").numerator.parse::<i128>().unwrap())
            .then(b.power.cmp(&a.power))
            .then(a.members.cmp(&b.members))
            .then(a.snaps.cmp(&b.snaps))
    });
    rows
}

fn check(data: &Synth, scenario: Scenario, check_bonus_winner: bool, challenge_points: bool) {
    let master = data.master();
    let mut owned = roster(&mut Rng::new(739), &master);
    owned.members.reverse(); // canonical ordering must not use pool indexes.
    owned.snaps.reverse();
    let inputs = input();
    let context = inputs.resolve(&master, scenario, Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    let payoff = inputs.event_payoff.as_ref().unwrap();
    let expected = oracle(&pool, &request(&master, context.clone(), 31), &context, payoff, challenge_points);
    let metric = if challenge_points {
        Metric::ClientChallengePoints { event_id: 7 }
    } else {
        Metric::ClientEventPoints { event_id: 7 }
    };
    if check_bonus_winner {
        assert!(expected[0].members.contains(&6), "weak member earns more PT through its event bonus");
        assert!(expected[0].power < expected.iter().map(|r| r.power).max().unwrap());
    }
    for k in [5, 31] {
        for strategy in [Strategy::BranchAndBound, Strategy::Exhaustive] {
            let out = search::solve_physical(
                &pool,
                &request(&master, context.clone(), k),
                &metric,
                Some(payoff),
                &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 0 },
                &strategy,
                None,
                &SimulationInput::default(),
            )
            .unwrap();
            assert_eq!(out.completion, Completion::Complete);
            assert_eq!(out.optimality, Optimality::Proven);
            assert_eq!(out.results.len(), k, "K is counted after canonicalization inside search");
            assert_eq!(out.results, expected[..k]);
            assert_eq!(out.result_identity, "team");
            assert_eq!(out.results.iter().map(|r| (r.members, r.snaps)).collect::<HashSet<_>>().len(), k);
            if matches!(strategy, Strategy::BranchAndBound) {
                assert!(out.telemetry.environment.bounds.compiled);
                assert!(out.telemetry.environment.bounds.fallback.is_none());
                assert!(out.telemetry.joint.modules["deckPayoff"].checks > 0);
                if check_bonus_winner {
                    assert!(out.telemetry.joint.modules["deckPayoff"].pruned > 0);
                }
            }
        }
    }
    let limited = search::solve_physical(
        &pool,
        &request(&master, context.clone(), 5),
        &metric,
        Some(payoff),
        &Limits { time_limit_ms: None, max_candidates: Some(1), cache_entries: 0 },
        &Strategy::BranchAndBound,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(limited.optimality, Optimality::Unproven);
    assert_eq!(limited.exit_reason, ExitReason::CandidateLimit);
    assert_ne!(limited.completion, Completion::Complete);
    assert_eq!(limited.result_identity, "team");
    assert_eq!(limited.results.len(), 1);
    assert!(expected.contains(&limited.results[0]));
}

#[test]
fn normal_and_challenge_skip_preserve_canonical_k5_and_k31() {
    let data = fixture();
    check(&data, Scenario::Free(10), true, false);
    check(&data, Scenario::Challenge(70), true, false);
    check(&data, Scenario::Free(10), false, true);
}

#[test]
fn configured_c_reward_stays_fixed_when_native_scores_cross_rank_thresholds() {
    for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
        let mut data = fixture();
        // Native scores occupy three rank intervals with nonmonotone rewards.
        // Configured C still controls every Skip EP/CP payoff in all intervals.
        let scores = {
            let master = data.master();
            let owned = roster(&mut Rng::new(739), &master);
            let inputs = input();
            let context = inputs.resolve(&master, scenario, Some(1004), &[]).unwrap();
            let pool = context.pool(&master, &owned).unwrap();
            let rows = oracle(
                &pool,
                &request(&master, context.clone(), 31),
                &context,
                inputs.event_payoff.as_ref().unwrap(),
                false,
            );
            let mut scores: Vec<i64> =
                rows.iter().map(|r| r.expected_score.as_ref().unwrap().numerator.parse().unwrap()).collect();
            scores.sort_unstable();
            scores.dedup();
            scores
        };
        assert!(scores.len() > 5);
        let middle = scores[scores.len() / 3];
        let high = scores[2 * scores.len() / 3];
        assert!(scores[0] < middle && middle < high && high < *scores.last().unwrap());
        for (name, rows) in &mut data.tables {
            if name == "MasterParameter" {
                for row in rows.as_array_mut().unwrap() {
                    if row["_id"] == "live_skip_result_score_rank" {
                        row["_value"] = json!("C");
                    }
                }
            }
            if name == "MasterLiveScoreRank" {
                *rows = json!([
                    {"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0},
                    {"_id":2,"_group":1,"_liveScoreRank":3,"_requiredScore":middle},
                    {"_id":3,"_group":1,"_liveScoreRank":4,"_requiredScore":high}
                ]);
            }
            if name == "MasterLiveEventPoint" || name == "MasterChallengeLiveEventPoint" {
                let group = if name == "MasterLiveEventPoint" { 1 } else { 2 };
                *rows = json!([
                    {"_id":1,"_group":group,"_scoreRank":2,"_value":100},
                    {"_id":2,"_group":group,"_scoreRank":3,"_value":1000},
                    {"_id":3,"_group":group,"_scoreRank":4,"_value":1}
                ]);
            }
            if name == "MasterLiveChallengePoint" {
                *rows = json!([
                    {"_id":1,"_scoreRank":2,"_value":5},
                    {"_id":2,"_scoreRank":3,"_value":50},
                    {"_id":3,"_scoreRank":4,"_value":1}
                ]);
            }
        }
        check(&data, scenario, false, false);
        if matches!(scenario, Scenario::Free(_)) {
            check(&data, scenario, false, true);
        }
    }
}

#[test]
fn missing_skip_reward_rows_keep_zero_payoff_and_all_canonical_ties() {
    let mut data = fixture();
    for (name, rows) in &mut data.tables {
        if name == "MasterLiveEventPoint" || name == "MasterChallengeLiveEventPoint" {
            *rows = json!([]);
        }
    }
    check(&data, Scenario::Free(10), false, false);
    check(&data, Scenario::Challenge(70), false, false);
    check(&data, Scenario::Free(10), false, true);
}

#[test]
fn cp_does_not_use_ep_bonus_or_the_ep_10000_intermediate_guard() {
    let mut data = fixture();
    for (name, rows) in &mut data.tables {
        if name == "MasterLiveChallengePoint" {
            for row in rows.as_array_mut().unwrap() {
                row["_value"] = json!(500_000);
            }
        }
        if name == "MasterEventEffect" {
            for row in rows.as_array_mut().unwrap() {
                for key in [
                    "_rank1EffectValue",
                    "_rank2EffectValue",
                    "_rank3EffectValue",
                    "_rank4EffectValue",
                    "_rank5EffectValue",
                ] {
                    row[key] = json!(100_000_000);
                }
            }
        }
    }
    // Native EP wraps in this fixture, but CP is independently 500000 * 1.
    // A CP compiler that reused EP's bonus/10000 checks would fall back here.
    check(&data, Scenario::Free(10), false, true);
}

#[test]
fn invalid_lowercase_configuration_uses_none_reward_not_the_score_rank() {
    let mut data = fixture();
    for (name, rows) in &mut data.tables {
        if name == "MasterParameter" {
            for row in rows.as_array_mut().unwrap() {
                if row["_id"] == "live_skip_result_score_rank" {
                    row["_value"] = json!("c");
                }
            }
        }
        if name == "MasterLiveEventPoint" {
            *rows = json!([{"_id":1,"_group":1,"_scoreRank":0,"_value":777}]);
        }
        if name == "MasterLiveChallengePoint" {
            *rows = json!([{"_id":1,"_scoreRank":0,"_value":77}]);
        }
    }
    // Score still maps to D, but case-sensitive Enum.TryParse("c") fails and
    // EP/CP use its NONE(0) result: 777 times bonus for EP, exactly 77 for CP.
    check(&data, Scenario::Free(10), false, false);
    check(&data, Scenario::Free(10), false, true);
}

#[test]
fn missing_skip_rank_configuration_is_an_error() {
    let mut data = fixture();
    for (name, rows) in &mut data.tables {
        if name == "MasterParameter" {
            rows.as_array_mut().unwrap().retain(|row| row["_id"] != "live_skip_result_score_rank");
        }
    }
    let master = data.master();
    assert!(ournotes_sim::event::skip_result_rank(&master).is_err());
    let owned = roster(&mut Rng::new(739), &master);
    let inputs = input();
    let context = inputs.resolve(&master, Scenario::Free(10), Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    for metric in [Metric::ClientEventPoints { event_id: 7 }, Metric::ClientChallengePoints { event_id: 7 }] {
        let out = search::solve_physical(
            &pool,
            &request(&master, context.clone(), 5),
            &metric,
            inputs.event_payoff.as_ref(),
            &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 0 },
            &Strategy::BranchAndBound,
            None,
            &SimulationInput::default(),
        );
        assert!(out.is_err(), "missing configuration cannot silently become score-ranked PT");
    }
}

#[test]
fn skip_rank_requires_string_parameter_type_before_enum_parsing() {
    for parameter_type in [None, Some("Int32")] {
        let mut data = fixture();
        for (name, rows) in &mut data.tables {
            if name == "MasterParameter" {
                for row in rows.as_array_mut().unwrap() {
                    if row["_id"] == "live_skip_result_score_rank" {
                        if let Some(value) = parameter_type {
                            row["_type"] = json!(value);
                        } else {
                            row.as_object_mut().unwrap().remove("_type");
                        }
                    }
                }
            }
        }
        assert!(
            ournotes_sim::event::skip_result_rank(&data.master()).is_err(),
            "missing/wrong parameter type is not Enum.TryParse failure"
        );
    }
}

#[test]
fn five_fixed_pairs_have_exactly_five_teams_even_when_all_values_tie() {
    let master = fixture().master();
    let mut owned = roster(&mut Rng::new(739), &master);
    // Equal leader skill levels make all five leader alternatives numerically
    // identical, while their canonical team identities must remain different.
    for member in &mut owned.members {
        member.rank = 1;
    }
    owned.members.reverse();
    let inputs = input();
    for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
        let context = inputs.resolve(&master, scenario, Some(1004), &[]).unwrap();
        let pool = context.pool(&master, &owned).unwrap();
        let mut req = request(&master, context.clone(), 31);
        req.constraints = Constraints { include_members: vec![1, 2, 3, 4, 5], no_snaps: true, ..Default::default() };
        let mut canonical = HashSet::new();
        let visited = visit_physical_decks(&pool, &req.constraints, |physical| {
            let leader = pool.members[physical.members[2]].id;
            let mut rest: Vec<_> = [0, 1, 3, 4].into_iter().map(|s| pool.members[physical.members[s]].id).collect();
            rest.sort_unstable();
            canonical.insert([rest[0], rest[1], leader, rest[2], rest[3]]);
            Ok(true)
        })
        .unwrap();
        assert_eq!(visited, 120);
        assert_eq!(canonical.len(), 5);
        let mut canonical: Vec<_> = canonical.into_iter().collect();
        canonical.sort_unstable();
        let mut metrics = vec![Metric::ClientEventPoints { event_id: 7 }];
        if matches!(scenario, Scenario::Free(_)) {
            metrics.push(Metric::ClientChallengePoints { event_id: 7 });
        }
        for metric in metrics {
            for k in [5, 31] {
                req.k = k;
                for strategy in [Strategy::BranchAndBound, Strategy::Exhaustive] {
                    let out = search::solve_physical(
                        &pool,
                        &req,
                        &metric,
                        inputs.event_payoff.as_ref(),
                        &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 0 },
                        &strategy,
                        None,
                        &SimulationInput::default(),
                    )
                    .unwrap();
                    assert_eq!(out.completion, Completion::Complete);
                    assert_eq!(
                        out.results.len(),
                        5,
                        "five bound pairs have five leader choices, not 120 layouts or one merged team"
                    );
                    assert_eq!(out.results.iter().map(|r| r.members).collect::<Vec<_>>(), canonical);
                    assert_eq!(
                        out.results.iter().map(|r| r.members[2]).collect::<HashSet<_>>(),
                        HashSet::from([1, 2, 3, 4, 5])
                    );
                    assert_eq!(out.results.iter().map(|r| r.power).collect::<HashSet<_>>().len(), 1);
                    assert_eq!(
                        out.results
                            .iter()
                            .map(|r| r.expected_payoff.as_ref().expect("exact payoff").numerator.clone())
                            .collect::<HashSet<_>>()
                            .len(),
                        1
                    );
                    for row in &out.results {
                        assert_eq!(row.snaps, [None; 5]);
                        assert_eq!(row.expected_payoff.as_ref().unwrap().denominator, "1");
                        assert_eq!(
                            row.expected_payoff.as_ref().unwrap().numerator,
                            manual_points(
                                &pool,
                                &context,
                                row.members,
                                row.snaps,
                                matches!(metric, Metric::ClientChallengePoints { .. })
                            )
                            .to_string()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn challenge_skip_cp_remains_outside_the_public_earnings_goal() {
    let master = fixture().master();
    let owned = roster(&mut Rng::new(739), &master);
    let inputs = input();
    let context = inputs.resolve(&master, Scenario::Challenge(70), Some(1004), &[]).unwrap();
    let pool = context.pool(&master, &owned).unwrap();
    let error = search::solve_physical(
        &pool,
        &request(&master, context.clone(), 5),
        &Metric::ClientChallengePoints { event_id: 7 },
        inputs.event_payoff.as_ref(),
        &Limits { time_limit_ms: None, max_candidates: None, cache_entries: 0 },
        &Strategy::BranchAndBound,
        None,
        &SimulationInput::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("challenge-point earnings require"));
}
