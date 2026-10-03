//! Explicitly synthetic UTF-8 transport corpus input; no game assets or native truth.
#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;
use common::{Rng, Synth, extend_table, replace_table, set_column, synth_snaps};
use ournotes_search::types::{Limits, Metric, SimulationInput, Strategy};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
const FIXTURE_SEED: u64 = 20_261_001;
const SCORE_ID: i64 = 1004;
const EVENT_ID: i64 = 7;

fn joint_request(mode: &str, gekisou: bool, metric: Value) -> ournotes_search::types::RecommendationRequest {
    serde_json::from_value(joint_request_json(mode, gekisou, metric)).unwrap()
}

fn joint_request_json(mode: &str, gekisou: bool, metric: Value) -> Value {
    json!({"format":"ournotes-deck.recommendation-request/1",
        "execution":{"kind":"live","scoreId":SCORE_ID,"gekisou":gekisou,"play":{"kind":"theoreticalBest"}},
        "scenario":{"kind":mode,"musicId":10},"context":context_document(false,false,false),
        "metric":metric,"k":12,"constraints":{"leader":3},
        "seedLaw":{"atoms":[[1,1],[-1,2],[1,3]],"provenance":"synthetic joint bound regression"},
        "strategy":{"kind":"branchAndBound"},"limits":{"timeLimitMs":null,"maxCandidates":null,"cacheEntries":0}
    })
}

#[test]
fn default_exact_search_has_no_hidden_candidate_prefix_limit() {
    use ournotes_search::{
        engine,
        search::Completion,
        types::{RecommendationRequest, Strategy},
    };
    let synth = synthetic_master(5, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    let request: RecommendationRequest = serde_json::from_value(json!({
        "format":"ournotes-deck.recommendation-request/1",
        "execution":{"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}},
        "scenario":{"kind":"free","musicId":10},"metric":{"kind":"cappedScore","threshold":2_000_000_000},
        "seedLaw":{"atoms":[[1,1]],"provenance":"synthetic default-budget regression"},
        "k":1,"limits":{"timeLimitMs":null,"cacheEntries":0}
    }))
    .unwrap();
    let outcome = engine::recommend(&data, &roster, &request).unwrap();
    assert!(matches!(outcome.strategy, Strategy::BranchAndBound));
    assert_eq!(outcome.completion, Completion::Complete);
    assert_eq!(outcome.telemetry.leaves.visited, 3720);
}

#[test]
fn joint_search_matches_exhaustive_full_topk_and_preserves_resource_constraints() {
    use ournotes_search::{engine, search::Completion, types::Strategy};
    let synth = synthetic_master(6, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let mut pruned = 0;
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for metric in [json!({"kind":"score"}), json!({"kind":"clientEventPoints","eventId":EVENT_ID})] {
            for constrained in [false, true] {
                let mut req = joint_request(mode, gekisou, metric.clone());
                if constrained {
                    req.constraints.include_members = vec![6];
                    req.constraints.exclude_snaps = vec![1];
                    req.seed_law.as_mut().unwrap().atoms =
                        vec![(0, 1), (i32::MIN, 2), (i32::MAX, 3), (-47, 4), (47, 5), (0, 7)];
                }
                let bounded = engine::recommend(&data, &roster, &req).unwrap();
                assert_eq!(bounded.completion, Completion::Complete);
                assert!(
                    bounded.telemetry.environment.bounds.fallback.is_none(),
                    "{} {:?}: {:?}",
                    mode,
                    metric,
                    bounded.telemetry.environment.bounds.fallback
                );
                let tel = &bounded.telemetry;
                pruned += tel.joint.branch.pruned.iter().sum::<u64>()
                    + tel.composition.composition.pruned
                    + tel.composition.layout.pruned;
                req.strategy = Strategy::Exhaustive;
                let baseline = engine::recommend(&data, &roster, &req).unwrap();
                assert_eq!(bounded.results, baseline.results, "{mode} {metric:?} constrained={constrained}");
                assert!(
                    bounded.telemetry.leaves.simulations <= baseline.telemetry.leaves.simulations,
                    "{mode} {metric:?} constrained={constrained}: bounded={} oracle={}",
                    bounded.telemetry.leaves.simulations,
                    baseline.telemetry.leaves.simulations
                );
            }
        }
    }
    assert!(pruned > 0, "joint search must actually prune branches");
}

/// Every stop reports a true upper bound of what it leaves: each deck of the full Top-K that the stopped search
/// did not keep pays at most `max(upperBound, stopped K-th)`. Candidate limits stop the search deterministically at
/// every depth of the joint and composition traversals.
#[test]
fn proof_upper_bound_covers_every_deck_a_stop_leaves() {
    use ournotes_search::{
        engine,
        search::{Completion, telemetry::Traversal},
    };
    let synth = synthetic_master(6, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let value = |d: &ournotes_search::types::RecommendedDeck| d.expected_payoff.numerator.parse::<i128>().unwrap();
    let mut traversals = BTreeSet::new();
    let mut stops = 0;
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for metric in [json!({"kind":"score"}), json!({"kind":"clientEventPoints","eventId":EVENT_ID})] {
            for constrained in [false, true] {
                let mut req = joint_request(mode, gekisou, metric.clone());
                if constrained {
                    req.constraints.include_members = vec![6];
                    req.seed_law.as_mut().unwrap().atoms = vec![(0, 1), (i32::MIN, 2), (47, 5)];
                }
                let full = engine::recommend(&data, &roster, &req).unwrap();
                assert_eq!(full.completion, Completion::Complete);
                let proof = &full.telemetry.proof;
                assert!(proof.complete && proof.fraction == Some(1.0) && proof.upper_bound.is_none());
                assert_eq!(proof.best.as_deref(), Some(full.results[0].expected_payoff.numerator.as_str()));
                traversals.insert(format!("{:?}", full.telemetry.environment.traversal));
                let visited = full.telemetry.leaves.visited;
                let mut limits: Vec<u64> = (1..=8).chain((1..=24).map(|i| visited * i / 25)).collect();
                limits.sort_unstable();
                limits.dedup();
                for limit in limits.into_iter().filter(|&n| n > 0 && n < visited) {
                    req.limits.max_candidates = Some(limit);
                    let stopped = engine::recommend(&data, &roster, &req).unwrap();
                    assert_eq!(stopped.completion, Completion::TimedOut, "{mode} {metric} limit {limit}");
                    let proof = &stopped.telemetry.proof;
                    assert!(!proof.complete);
                    assert!(matches!(
                        stopped.telemetry.environment.traversal,
                        Traversal::Joint | Traversal::Composition
                    ));
                    let fraction = proof.fraction.expect("tracked traversal");
                    assert!((0.0..1.0).contains(&fraction), "fraction {fraction}");
                    let upper = proof.upper_bound.as_ref().map(|v| v.parse::<i128>().unwrap());
                    let kth = (stopped.results.len() == req.k).then(|| value(stopped.results.last().unwrap()));
                    for deck in &full.results {
                        if stopped.results.iter().any(|r| r.members == deck.members && r.snaps == deck.snaps) {
                            continue;
                        }
                        let cover = upper.max(kth);
                        assert!(
                            cover.is_some_and(|c| value(deck) <= c),
                            "{mode} {metric} constrained={constrained} limit {limit}: {} above {upper:?}/{kth:?}",
                            value(deck)
                        );
                    }
                    if let Some(gap) = proof.best_gap {
                        assert!(gap >= 0.0);
                    }
                    stops += 1;
                }
            }
        }
    }
    assert_eq!(traversals.len(), 2, "{traversals:?}");
    assert!(stops > 50, "{stops}");
}

#[test]
fn warm_start_and_visit_order_leave_the_canonical_topk_unchanged() {
    use ournotes_search::{
        engine,
        search::{Completion, ablate, set_bound_ablation},
    };
    let synth = synthetic_master(6, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let mut seeded = 0;
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for metric in [json!({"kind":"score"}), json!({"kind":"clientEventPoints","eventId":EVENT_ID})] {
            for (k, constrained) in [(12, false), (12, true), (1, false), (3, true)] {
                let mut req = joint_request(mode, gekisou, metric.clone());
                req.k = k;
                if constrained {
                    req.constraints.include_members = vec![6];
                    req.constraints.exclude_snaps = vec![1];
                    req.seed_law.as_mut().unwrap().atoms = vec![(0, 1), (i32::MIN, 2), (47, 5), (-47, 4)];
                }
                req.strategy = Strategy::Exhaustive;
                let oracle = engine::recommend(&data, &roster, &req).unwrap();
                req.strategy = Strategy::BranchAndBound;
                for bits in
                    [0, ablate::NO_WARM_START, ablate::STATIC_ORDER, ablate::NO_WARM_START | ablate::STATIC_ORDER]
                {
                    set_bound_ablation(bits);
                    let out = engine::recommend(&data, &roster, &req).unwrap();
                    set_bound_ablation(0);
                    let case = format!("{mode} {metric:?} k={k} constrained={constrained} ablation={bits}");
                    assert_eq!(out.completion, Completion::Complete, "{case}");
                    assert_eq!(out.results, oracle.results, "{case}");
                    assert!(out.telemetry.leaves.simulations <= oracle.telemetry.leaves.simulations, "{case}");
                    let warm = &out.telemetry.incumbents.warm_start;
                    if bits & ablate::NO_WARM_START == 0 {
                        seeded += warm.evaluations;
                    } else {
                        assert_eq!(warm.evaluations + warm.polish_evaluations, 0, "{case}");
                    }
                }
            }
        }
    }
    assert!(seeded > 0, "the warm start must evaluate decks");
}

#[test]
fn joint_pt_tiers_preserve_nonmonotone_rewards_and_full_canonical_topk() {
    use ournotes_search::{engine, search::Completion};
    let mut synth = synthetic_master(6, 2, 5);
    // A lower grade can pay more. Bounding by the reward at the maximum reachable
    // score instead of the maximum over ALL reachable tiers would lose solutions.
    set_column(&mut synth, "MasterLiveEventPoint", &mut |r| {
        r["_value"] = json!(match r["_scoreRank"].as_i64().unwrap() {
            2 => 300,
            3 => 900,
            4 => 20,
            _ => 80,
        });
    });
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    for (mode, gk) in [("free", false), ("mission", true)] {
        let mut req = joint_request(mode, gk, json!({"kind":"clientEventPoints","eventId":EVENT_ID}));
        let bounded = engine::recommend(&data, &roster, &req).unwrap();
        assert_eq!(bounded.completion, Completion::Complete);
        assert!(bounded.telemetry.environment.bounds.fallback.is_none());
        req.strategy = Strategy::Exhaustive;
        let oracle = engine::recommend(&data, &roster, &req).unwrap();
        assert_eq!(bounded.results, oracle.results);
    }
}

#[test]
fn composition_frontier_recovers_every_layout_when_k_exceeds_binding_count() {
    use ournotes_search::{engine, search::Completion, types::Strategy};
    let synth = synthetic_master(6, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let mut request = joint_request("free", false, json!({"kind":"clientEventPoints","eventId":EVENT_ID}));
    request.k = 64;
    request.constraints.no_snaps = true;
    let actual = engine::recommend(&data, &roster, &request).unwrap();
    request.strategy = Strategy::Exhaustive;
    let oracle = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(actual.completion, Completion::Complete);
    assert_eq!(actual.results, oracle.results);
    assert_eq!(actual.results.len(), 48);
    assert!(actual.telemetry.composition.power_frontier_closed > 0);
}

#[test]
fn pt_incumbent_regime_removes_only_strictly_inferior_members() {
    use ournotes_search::{engine, search::Completion};
    for (equal, wide_k) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut synth = synthetic_master(6, 2, 5);
        replace_table(
            &mut synth,
            "MasterLiveScoreRank",
            json!([{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0,"_battleLiveRequiredScore":0}]),
        );
        replace_table(&mut synth,"MasterEventEffect",Value::Array((1..=if equal {6} else {5}).map(|id|json!({
            "_id":id,"_eventId":EVENT_ID,"_resourceTypeConstraint":2,"_memberCardId":id,"_eventBonusType":0,
            "_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,"_rank4EffectValue":1000,"_rank5EffectValue":1000
        })).collect()));
        let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
        let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
        let mut request = joint_request("mission", true, json!({"kind":"clientEventPoints","eventId":EVENT_ID}));
        if wide_k {
            request.k = 64;
            request.constraints.no_snaps = true;
        }
        let actual = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(actual.completion, Completion::Complete);
        assert_eq!(
            actual.telemetry.environment.bounds.pt_regime.as_ref().map_or(0, |r| r.members_removed),
            usize::from(!equal && !wide_k),
            "telemetry={:?}; result={:?}",
            actual.telemetry,
            actual.results.first().map(|r| &r.expected_payoff)
        );
        assert!(actual.telemetry.environment.bounds.pt_regime.as_ref().is_none_or(|r| r.fallback.is_none()));
        request.strategy = Strategy::Exhaustive;
        let oracle = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(actual.results, oracle.results, "equal-bonus alternative={equal}");
    }
}

#[test]
fn joint_search_falls_back_for_wrapping_pt_and_explicit_clocks() {
    use ournotes_search::{engine, search::Completion, types::Strategy};
    let mut synth = synthetic_master(5, 1, 5);
    set_column(&mut synth, "MasterLiveEventPoint", &mut |row| row["_value"] = json!(1_000_000_000));
    let data = DeckData::from_json(&data_document(&synth, 5, 1, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 1, 5).to_string()).unwrap();
    let mut req = joint_request("free", false, json!({"kind":"clientEventPoints","eventId":EVENT_ID}));
    let bounded = engine::recommend(&data, &roster, &req).unwrap();
    assert!(bounded.telemetry.environment.bounds.fallback.as_ref().unwrap().contains("wrapping"));
    req.strategy = Strategy::Exhaustive;
    assert_eq!(bounded.results, engine::recommend(&data, &roster, &req).unwrap().results);
    let mut req = joint_request("free", false, json!({"kind":"score"}));
    req.simulation.music_length_ms = Some(20_000);
    let bounded = engine::recommend(&data, &roster, &req).unwrap();
    assert!(bounded.telemetry.environment.bounds.fallback.as_ref().unwrap().contains("clocks"));
    assert_eq!(bounded.completion, Completion::Complete);
    req.strategy = Strategy::Exhaustive;
    assert_eq!(bounded.results, engine::recommend(&data, &roster, &req).unwrap().results);
    req.strategy = Strategy::BranchAndBound;
    req.limits.time_limit_ms = Some(0);
    let stopped = engine::recommend(&data, &roster, &req).unwrap();
    assert_eq!(stopped.completion, Completion::TimedOut);
    assert!(stopped.results.is_empty());
}

#[test]
fn built_problem_reuses_frozen_inputs_across_live_snap_and_gekisou_score_pt() {
    use ournotes_search::{auxiliary, engine, handler, search};
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for metric in [json!({"kind":"score"}), json!({"kind":"clientEventPoints","eventId":EVENT_ID})] {
            let synth = synthetic_master(5, 2, 5);
            let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
            let roster_json = roster_document(5, 2, 5).to_string();
            let mut roster = Roster::from_json(&roster_json).unwrap();
            let request_json = json!({"format":"ournotes-deck.recommendation-request/1",
                "execution":{"kind":"live","scoreId":SCORE_ID,"gekisou":gekisou,"play":{"kind":"theoreticalBest"}},
                "scenario":{"kind":mode,"musicId":10}, "context":context_document(false,false,false),
                "metric":metric,"k":3,"constraints":{"leader":3},
                "seedLaw":{"atoms":[[1,1],[-1,2],[1,3]],"provenance":"synthetic joint law"},
                "strategy":{"kind":"exhaustive"},"limits":{"cacheEntries":0}})
            .to_string();
            let mut request: ournotes_search::types::RecommendationRequest =
                serde_json::from_str(&request_json).unwrap();
            let built = handler::build_card_pool(&data, &roster, &request).unwrap();
            assert_eq!(built.context().route(), handler::SolverRoute::PhysicalExhaustive);
            assert_eq!(built.domain().members().len(), 5);
            assert_eq!(built.domain().snaps().len(), 2);
            assert!(built.domain().is_feasible());
            let direct = engine::recommend(&data, &roster, &request).unwrap();
            let first = search::recommend_built(&built).unwrap();
            assert_eq!(first.results, direct.results);
            assert_eq!(first.completion, search::Completion::Complete);
            assert!(first.telemetry.phases.iter().all(|p| p.name != "prepare"));
            assert_eq!(first.telemetry.leaves.visited, 744);
            let json_result: Value =
                serde_json::from_str(&engine::recommend_json(&data, &roster_json, &request_json).unwrap()).unwrap();
            assert_eq!(json_result["results"], serde_json::to_value(&first.results).unwrap());
            // The caller can reuse/change its transport buffers, never the compiled problem.
            roster.members.clear();
            request.constraints.no_snaps = true;
            request.k = 1;
            let again = search::recommend_built(&built).unwrap();
            assert_eq!(again.results, first.results);
            assert_eq!(again.telemetry.leaves.simulations, first.telemetry.leaves.simulations);
            assert_eq!(built.context().request().k, 3);
            for row in &first.results {
                let evaluated = auxiliary::evaluate_built(&built, row.members, row.snaps).unwrap();
                assert_eq!(evaluated.results, [row.clone()]);
                assert_eq!(evaluated.optimality, ournotes_search::types::Optimality::NotApplicable);
                assert_eq!(evaluated.telemetry.leaves.visited, 1);
            }
            assert!(auxiliary::evaluate_built(&built, [1, 2, 3, 4, 5], [Some(1), Some(1), None, None, None]).is_err());
            assert!(auxiliary::evaluate_built(&built, [3, 2, 1, 4, 5], [None; 5]).is_err());
        }
    }
}

#[test]
fn built_problem_validates_before_zero_budget_and_preserves_solver_routes() {
    use ournotes_search::{engine, handler, search};
    let synth = synthetic_master(5, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    for execution in
        [json!({"kind":"power","musicId":10,"eventParameter":false}), json!({"kind":"skip","scoreId":SCORE_ID})]
    {
        let metric = if execution["kind"] == "power" { json!({"kind":"power"}) } else { json!({"kind":"score"}) };
        let request = fixed_request(execution, metric);
        let built = handler::build_card_pool(&data, &roster, &request).unwrap();
        assert_eq!(built.context().route(), handler::SolverRoute::CanonicalPowerSkip);
        let via_built = search::recommend_built(&built).unwrap();
        assert_eq!(via_built.result_identity, "canonicalMemberSet");
        assert_eq!(via_built.results, engine::recommend(&data, &roster, &request).unwrap().results);
    }
    let mut request = fixed_request(
        json!({"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}}),
        json!({"kind":"score"}),
    );
    request.limits.time_limit_ms = Some(0);
    let built = handler::build_card_pool(&data, &roster, &request).unwrap();
    let empty = search::recommend_built(&built).unwrap();
    assert_eq!(empty.completion, search::Completion::TimedOut);
    assert!(empty.results.is_empty());
    request.constraints.include_members = vec![999];
    assert!(handler::build_card_pool(&data, &roster, &request).is_err());
    request.constraints.include_members.clear();
    request.execution = ournotes_search::types::Execution::Live {
        score_id: SCORE_ID,
        gekisou: false,
        play: ournotes_search::types::PlayPolicy::Stream {
            stream: ournotes_sim::live::model::JudgementStream {
                frames: vec![],
                judged: vec![],
                base_seed: 0,
                assist: false,
                delta_times: None,
            },
        },
    };
    assert!(handler::build_card_pool(&data, &roster, &request).is_err());
}

fn synthetic_master(members: i64, snaps: i64, characters: i64) -> Synth {
    let mut s = synth_snaps(&mut Rng::new(FIXTURE_SEED), members, snaps, &[1, 3, 6, 10, 11]);
    replace_table(
        &mut s,
        "MasterCharacter",
        Value::Array((1..=characters).map(|id| json!({"_id":id,"_bandID":(id-1)%3+1})).collect()),
    );
    set_column(&mut s, "MasterMemberCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_characterID"] = json!((id - 1) % characters + 1);
        r["_cardType"] = json!((id - 1) % 5 + 1);
        r["_liveSkillID"] = json!((id - 1) % 3 + 1);
        r["_leaderSkillID"] = json!(4);
    });
    set_column(&mut s, "MasterSupportCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_characterIDs"] = json!([(id - 1) % characters + 1]);
        r["_supportSkillId01"] = json!(if id % 2 == 1 { 3 } else { 10 });
        r["_supportSkillId02"] = json!(if id % 2 == 1 { 1 } else { 6 });
    });
    replace_table(
        &mut s,
        "MasterLiveMusic",
        json!([{
            "_id":10,"_musicType":1,"_bestMusicTagIDs":[1],"_expertID":SCORE_ID,
            "_liveScoreRankGroup":1,"_gekisouMission1":1,"_gekisouMission2":2,"_gekisouMission3":3
        }]),
    );
    replace_table(
        &mut s,
        "MasterLiveMusicScore",
        json!([{
            "_id":SCORE_ID,"_musicScoreLevel":24,"_fullComboCount":12
        }]),
    );
    replace_table(
        &mut s,
        "MasterChallengeMusic",
        json!([
            {"_id":70,"_eventId":EVENT_ID,"_liveMusicId":10,"_musicType":4,"_bestMusicTagIDs":[2]},
            {"_id":71,"_eventId":EVENT_ID,"_liveMusicId":10,"_musicType":0,"_bestMusicTagIDs":[]}
        ]),
    );
    replace_table(
        &mut s,
        "MasterArenaMusic",
        json!([{
            "_id":80,"_liveMusicId":10,"_liveMusicType":5,
            "_gekisouMission1":3,"_gekisouMission2":2,"_gekisouMission3":1
        }]),
    );
    replace_table(
        &mut s,
        "MasterLiveJudgementTiming",
        json!([
            {"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_beforeMs":40,"_afterMs":40},
            {"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_beforeMs":80,"_afterMs":80},
            {"_id":3,"_noteJudgementType":2,"_noteSimulateJudgement":5,"_beforeMs":80,"_afterMs":80}
        ]),
    );
    extend_table(
        &mut s,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"40"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"20"}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    extend_table(
        &mut s,
        "MasterLiveComboScoreBonus",
        (1..=4).map(|i| json!({"_id":100+i,"_comboBonusType":1,"_requiredComboCount":i,"_bonusFactor":0.02})).collect(),
    );
    replace_table(
        &mut s,
        "MasterLiveGekisouRankingScoreBonus",
        Value::Array(
            (1..=3)
                .flat_map(|pattern| {
                    (1..=3).map(move |count| {
                        json!({"_id":pattern*10+count,"_missionPattern":pattern,
            "_count":count,"_rank":1,"_scoreBonusPercent":10})
                    })
                })
                .collect(),
        ),
    );
    replace_table(&mut s, "MasterLiveGekisouLuckBasePoint", Value::Array((3..=6).map(|judgement| {
        json!({"_id":judgement,"_noteCategory":0,"_noteSimulateJudgement":judgement,"_weight":1,"_basePoint":10})
    }).collect()));
    replace_table(
        &mut s,
        "MasterLiveGekisouLuckBonusLot",
        Value::Array(
            (0..5)
                .flat_map(|kind| {
                    (0..4).map(move |result| {
                        json!({"_id":kind*10+result+1,"_chanceLotType":kind,
            "_lotResult":result,"_weight":([5,4,2,1][result as usize])})
                    })
                })
                .collect(),
        ),
    );
    // Own skill-event trigger, with a real probability checker in the synthetic effect table.
    replace_table(
        &mut s,
        "MasterGekisouSkillEffect",
        json!([{
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":53,"_skillConditionGroup":66,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.4,"_effectValue":900,
            "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
        }]),
    );
    replace_table(
        &mut s,
        "MasterEvent",
        json!([{
            "_id":EVENT_ID,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2
        }]),
    );
    replace_table(
        &mut s,
        "MasterEventEffect",
        Value::Array(
            (0..=2)
                .map(|kind| {
                    json!({"_id":kind+1,"_eventId":EVENT_ID,"_eventBonusType":kind,"_resourceTypeConstraint":2,
            "_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,
            "_rank4EffectValue":1000,"_rank5EffectValue":1000})
                })
                .collect(),
        ),
    );
    replace_table(&mut s, "MasterLiveScoreRank", Value::Array([0,300_000,450_000,600_000].iter().enumerate().map(|(i, score)| {
        json!({"_id":i+1,"_group":1,"_liveScoreRank":i+2,"_requiredScore":score,"_battleLiveRequiredScore":score})
    }).collect()));
    for (table, group, base) in [
        ("MasterLiveEventPoint", 1, 100),
        ("MasterChallengeLiveEventPoint", 2, 300),
        ("MasterLiveChallengePoint", 1, 5),
    ] {
        replace_table(
            &mut s,
            table,
            Value::Array(
                (2..=5)
                    .map(|rank| json!({"_id":rank,"_group":group,"_scoreRank":rank,"_value":base+(rank-2)*base/3}))
                    .collect(),
            ),
        );
    }
    replace_table(
        &mut s,
        "MasterLiveMusicBoostBonus",
        json!([{
            "_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":2,"_playerExpRate":2,
            "_memberCardExpRate":2,"_friendshipExpRate":2,"_eventPointRate":2
        }]),
    );
    replace_table(
        &mut s,
        "MasterChallengeMusicBoostBonus",
        json!([{
            "_id":1,"_consumedChallengePointCount":201,"_liveMusicRewardRate":2,"_playerExpRate":2,
            "_memberCardExpRate":2,"_friendshipExpRate":2,"_eventPointRate":2
        }]),
    );
    for (table, count) in [("MasterLiveEventReward", 3), ("MasterChallengeLiveEventReward", 4)] {
        replace_table(
            &mut s,
            table,
            json!([{
                "_id":5,"_group":1,"_eventGroup":1,"_scoreRank":2,"_resourceType":11,
                "_resourceId":9,"_resourceCount":count,"_probability":999
            }]),
        );
    }
    replace_table(
        &mut s,
        "MasterEventAchievementReward",
        json!([{
            "_id":1,"_eventId":EVENT_ID,"_eventPoint":100,"_rewardIds":[5]
        }]),
    );
    s
}

fn columns_and_rows(s: &Synth) -> Value {
    let mut master: serde_json::Map<_, _> = s
        .tables
        .iter()
        .map(|(name, rows)| {
            let objects = rows.as_array().expect("synthetic table is an array");
            let columns: Vec<_> = objects
                .iter()
                .flat_map(|r| r.as_object().unwrap().keys().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let data_rows: Vec<Vec<Value>> = objects
                .iter()
                .map(|r| columns.iter().map(|key| r.get(key).cloned().unwrap_or(Value::Null)).collect())
                .collect();
            (name.clone(), json!({"columns":columns,"rows":data_rows}))
        })
        .collect();
    common::every_table(&mut master);
    json!(master)
}

fn data_document(s: &Synth, members: i64, snaps: i64, characters: i64) -> Value {
    json!({
        "format":"nnnotes.deck-data/1",
        "provenance":{"synthetic":true,"fixtureSeed":FIXTURE_SEED,"region":"synthetic",
            "masterVersion":"synthetic-recommend-fixture-20261001","clientVersion":"synthetic-inputs",
            "source":"tests/recommend_fixture_export.rs + tests/common::synth_snaps",
            "members":members,"snaps":snaps,"characters":characters,"isOCRTruth":false,
            "assetSha256Policy":"64 zero placeholder; no game asset or real chart is claimed"},
        "master":columns_and_rows(s),
        "charts":[{"scoreId":SCORE_ID,"asset":{"key":"SYNTHETIC-short-chart-1004",
            "sha256":"0000000000000000000000000000000000000000000000000000000000000000"},
            "notes":{"id":(1..=12).collect::<Vec<_>>(),"op":vec![1;12],
                "judgementType":vec![1;12],"timeMs":(1..=12).map(|i|i*100).collect::<Vec<_>>()},
            "skillEvents":{"timeMs":[0,250,500,750,1000]},
            "fevers":{"startMs":[150,450,850],"endMs":[400,800,1150]}}]
    })
}

fn roster_document(members: i64, snaps: i64, characters: i64) -> Value {
    let ranks: BTreeMap<_, _> = (1..=characters).map(|id| (id.to_string(), 10)).collect();
    json!({
        "provenance":{"synthetic":true,"isOCRTruth":false,"source":"manually fixed synthetic progress"},
        "player":{"characterRanks":ranks,"bandItems":{},"vipRank":3,"events":[EVENT_ID],
            "memory":null,"ownedMemberCardIds":(1..=members).collect::<Vec<_>>(),
            "ownedSupportCardIds":(1..=snaps).collect::<Vec<_>>()},
        "members":(1..=members).map(|id| json!({"id":id,"level":40,"exp":null,"awake":2,
            "rank":3,"liveSkillLevel":4,"gekisouSkillLevel":1})).collect::<Vec<_>>(),
        "snaps":(1..=snaps).map(|id| json!({"id":id,"level":30,"exp":null,"rank":3})).collect::<Vec<_>>()
    })
}

fn context_document(skip: bool, multiplayer: bool, expired: bool) -> Value {
    let mut value = json!({
        "powerSnapshot":{"eventIds":[EVENT_ID],"capturedJstTicks":50},
        "resultClock":if skip {json!({"execution":"skip","serverNowJstTicks":if expired {200} else {150}})}
            else {json!({"execution":"played","savedStartJstTicks":150,"serverNowJstTicks":201})},
        "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":EVENT_ID,"points":0,
            "challengePoints":500,"added":[]}],"eventWindows":[{"eventId":EVENT_ID,
            "startJstTicks":100,"endJstTicks":200}],"selectedRewards":[{"eventId":EVENT_ID,"rewardId":5}]}
    });
    if multiplayer {
        value["eventPayoff"]["multiplayerResultPanel"] = json!({"localPlayerIndex":1,
            "localDisconnected":false,"otherPlayers":[{"finalScore":100000,"disconnected":false},
                {"finalScore":50000,"disconnected":true}]});
    }
    value
}

#[test]
#[ignore = "export manual synthetic input for explicit CLI/WASM checks"]
fn export_adapter_inputs() {
    let directory = std::env::var_os("BDON_FIXTURE_OUT").expect("BDON_FIXTURE_OUT");
    let out = Path::new(&directory);
    fs::create_dir_all(out).unwrap();
    let synth = synthetic_master(7, 3, 7);
    let document = data_document(&synth, 7, 3, 7);
    let roster = roster_document(7, 3, 7);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    Roster::from_json(&roster.to_string()).unwrap();
    let stream = ournotes_sim::live::model::JudgementStream::theoretical_best(&data.chart(SCORE_ID).unwrap());
    for (name, value) in [
        ("DeckData.json", document),
        ("roster.json", roster),
        ("play-ordinary.json", serde_json::to_value(stream).unwrap()),
        ("context-played.json", context_document(false, false, false)),
        ("context-skip.json", context_document(true, false, false)),
    ] {
        let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
        bytes.push(b'\n');
        fs::write(out.join(name), bytes).unwrap();
    }
}

#[test]
#[ignore = "export synthetic Live/Gekisou search-harness corpus"]
fn export_search_harness_inputs() {
    let directory = std::env::var_os("BDON_HARNESS_OUT").expect("BDON_HARNESS_OUT");
    let out = Path::new(&directory);
    fs::create_dir_all(out).unwrap();
    // Same-character alternatives plus two distinct support programs. All optional
    // Snap bindings and every physical member order remain in the declared domain.
    let stress = std::env::var_os("BDON_HARNESS_STRESS").is_some();
    let (members, snaps, characters) = if stress { (8, 3, 6) } else { (6, 2, 5) };
    let synth = synthetic_master(members, snaps, characters);
    let data = data_document(&synth, members, snaps, characters);
    let roster = roster_document(members, snaps, characters);
    for (name, value) in [("DeckData.json", data), ("roster.json", roster)] {
        fs::write(out.join(name), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    let mut cases = Vec::new();
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for (objective, metric) in
            [("score", json!({"kind":"score"})), ("pt", json!({"kind":"clientEventPoints","eventId":EVENT_ID}))]
        {
            if stress && (mode != "free" || objective != "pt") {
                continue;
            }
            let id = format!("{mode}-{objective}");
            let request = json!({"format":"ournotes-deck.recommendation-request/1",
                "execution":{"kind":"live","scoreId":SCORE_ID,"gekisou":gekisou,
                    "play":{"kind":"theoreticalBest"}},
                "scenario":{"kind":mode,"musicId":10},
                "context":context_document(false,false,false),"metric":metric,"k":3,
                "seedLaw":{"atoms":[[1,1],[-1,2],[1,3]],
                    "provenance":"synthetic signed roots and duplicate integer masses; not a population law"},
                "strategy":{"kind":"exhaustive"},
                "limits":{"timeLimitMs":null,"maxCandidates":null,"cacheEntries":0}});
            let request_name = format!("{id}-request.json");
            fs::write(out.join(&request_name), serde_json::to_vec_pretty(&request).unwrap()).unwrap();
            let mut case = json!({"id":id,"data":"DeckData.json","roster":"roster.json",
                "request":request_name,"oracleMaxCandidates":if stress {500000} else {10000},
                "dominance":[{"kind":"member","from":1,"to":6},
                    {"kind":"snap","from":1,"to":2}],
                "experiments":[
                    {"name":"exhaustive","patch":{},"repeats":2},
                    {"name":"joint-bnb","patch":{"strategy":{"kind":"branchAndBound"}},"repeats":2},
                    {"name":"candidate-128","patch":{"strategy":{"kind":"candidate",
                        "powerSeeds":2,"proposals":128,"proposalSeed":8419}},"repeats":2},
                    {"name":"remove-snap-1-unproved","patch":{"constraints":{"excludeSnaps":[1]}},"repeats":1}]});
            if stress {
                case["dominance"] = json!([]);
                case["experiments"] =
                    json!([{"name":"joint-bnb","patch":{"strategy":{"kind":"branchAndBound"}},"repeats":3}]);
            }
            let name = format!("{id}.json");
            fs::write(out.join(&name), serde_json::to_vec_pretty(&case).unwrap()).unwrap();
            cases.push(name);
        }
    }
    fs::write(
        out.join("suite.json"),
        serde_json::to_vec_pretty(&json!({
        "format":"ournotes-deck.search-harness-suite/1","scope":"synthetic current-model",
        "cases":cases}))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn unsupported_lifecycle_rejected_before_low_level_feasibility_or_validation() {
    use ournotes_search::search::{Constraints, Objective, SearchRequest, expectation::FiniteSeedLaw, solve_physical};
    use ournotes_sim::{Error, pool::Pool};
    let synth = synthetic_master(7, 3, 7);
    let data = DeckData::from_json(&data_document(&synth, 7, 3, 7).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(7, 3, 7).to_string()).unwrap();
    let pool = Pool::new(&data.master, &roster).unwrap();
    let request = SearchRequest {
        objective: Objective::Power { music_id: Some(10), event: false },
        k: 1,
        constraints: Constraints { include_members: vec![999], ..Default::default() },
        time_limit: None,
    };
    let law = FiniteSeedLaw::new(vec![(1, 1)]).unwrap();
    for (network, finished) in [(Some(&[][..]), None), (None, Some(0))] {
        let result = solve_physical(
            &pool,
            &request,
            &law,
            &Metric::Power,
            None,
            &Limits::default(),
            &Strategy::Exhaustive,
            network,
            &SimulationInput { live_finished_from_frame: finished, ..Default::default() },
        );
        assert!(matches!(result, Err(Error::Unsupported(_))));
    }
}

fn fixed_request(execution: Value, metric: Value) -> ournotes_search::types::RecommendationRequest {
    let live = execution["kind"] == "live";
    let mut value = json!({
        "format":"ournotes-deck.recommendation-request/1", "execution":execution,
        "scenario":{"kind":"free","musicId":10}, "metric":metric, "k":100,
        "constraints":{"leader":3,"noSnaps":true},
        "limits":{"timeLimitMs":null,"maxCandidates":null,"cacheEntries":128}
    });
    if live {
        value["seedLaw"] = json!({"atoms":[[3,2],[7,1]],"provenance":"synthetic finite-law test"});
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn fixed_deck_matches_search_without_changing_slots_or_payoff() {
    use ournotes_search::{auxiliary::evaluate_fixed, engine::recommend, types::Optimality};
    let s = synthetic_master(5, 0, 5);
    let data = DeckData::from_json(&data_document(&s, 5, 0, 5).to_string()).unwrap();
    let owned = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    for execution in [
        json!({"kind":"power","musicId":10,"eventParameter":false}),
        json!({"kind":"skip","scoreId":SCORE_ID}),
        json!({"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}}),
    ] {
        let power = execution["kind"] == "power";
        let live = execution["kind"] == "live";
        let r = fixed_request(execution, json!({"kind":if power {"power"} else {"score"}}));
        let search = recommend(&data, &owned, &r).unwrap();
        let best = &search.results[0];
        let fixed = evaluate_fixed(&data, &owned, &r, best.members, best.snaps).unwrap();
        assert_eq!(fixed.optimality, Optimality::NotApplicable);
        assert_eq!(fixed.result_identity, "fixedPhysicalDeck");
        assert_eq!(fixed.telemetry.leaves.evaluated, 1);
        let actual = &fixed.results[0];
        assert_eq!(actual.members, best.members);
        assert_eq!(actual.snaps, best.snaps);
        assert_eq!(actual.power, best.power);
        assert_eq!(actual.expected_score, best.expected_score);
        assert_eq!(actual.expected_payoff, best.expected_payoff);
        assert_eq!(actual.score_summary, best.score_summary);
        if live {
            assert_eq!(actual.atoms, best.atoms);
        }
    }
}

#[test]
fn fixed_deck_rejects_constraints_and_never_substitutes_another_deck() {
    use ournotes_search::{auxiliary::evaluate_fixed, search::Completion};
    use ournotes_sim::Error;
    let s = synthetic_master(6, 2, 6);
    let data = DeckData::from_json(&data_document(&s, 6, 2, 6).to_string()).unwrap();
    let owned = Roster::from_json(&roster_document(6, 2, 6).to_string()).unwrap();
    let r = fixed_request(json!({"kind":"power","musicId":10,"eventParameter":false}), json!({"kind":"power"}));
    for (members, snaps) in [
        ([3, 2, 1, 4, 5], [None; 5]), // declared leader is in the wrong physical slot
        ([1, 2, 3, 4, 5], [Some(1), None, None, None, None]), // noSnaps
        ([1, 2, 3, 4, 4], [None; 5]), // duplicated character
    ] {
        assert!(matches!(evaluate_fixed(&data, &owned, &r, members, snaps), Err(Error::Input(_))));
    }
    let mut excluded = r.clone();
    excluded.constraints.exclude_members = vec![1];
    assert!(matches!(evaluate_fixed(&data, &owned, &excluded, [1, 2, 3, 4, 5], [None; 5]), Err(Error::Input(_))));
    let mut required = r.clone();
    required.constraints.include_members = vec![6];
    assert!(matches!(evaluate_fixed(&data, &owned, &required, [1, 2, 3, 4, 5], [None; 5]), Err(Error::Input(_))));
    let mut zero = r;
    zero.limits.time_limit_ms = Some(0);
    let out = evaluate_fixed(&data, &owned, &zero, [1, 2, 3, 4, 5], [None; 5]).unwrap();
    assert_eq!(out.completion, Completion::TimedOut);
    assert!(out.results.is_empty());
    assert_eq!(out.telemetry.leaves.evaluated, 0);
}

#[test]
fn song_ranking_resolves_each_song_and_shares_the_total_candidate_budget() {
    use ournotes_search::{
        auxiliary::{SongTarget, evaluate_fixed, rank_fixed_songs},
        search::Completion,
        types::{Execution, Scene},
    };
    let mut s = synthetic_master(5, 0, 5);
    set_column(&mut s, "MasterMemberCard", &mut |row| {
        row["_cardType"] = json!(1);
    });
    extend_table(
        &mut s,
        "MasterLiveMusic",
        vec![json!({
            "_id":11,"_musicType":2,"_bestMusicTagIDs":[],"_expertID":2004,
            "_liveScoreRankGroup":1,"_gekisouMission1":1,"_gekisouMission2":2,"_gekisouMission3":3
        })],
    );
    extend_table(&mut s, "MasterLiveMusicScore", vec![json!({"_id":2004,"_musicScoreLevel":24,"_fullComboCount":12})]);
    let mut doc = data_document(&s, 5, 0, 5);
    let mut second = doc["charts"][0].clone();
    second["scoreId"] = json!(2004);
    doc["charts"].as_array_mut().unwrap().push(second);
    let data = DeckData::from_json(&doc.to_string()).unwrap();
    let owned = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let r = fixed_request(json!({"kind":"skip","scoreId":SCORE_ID}), json!({"kind":"score"}));
    let targets = [
        SongTarget { score_id: SCORE_ID, scenario: Scene::Free { music_id: 10 } },
        SongTarget { score_id: 2004, scenario: Scene::Free { music_id: 11 } },
    ];
    let ranked = rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &targets).unwrap();
    assert_eq!(ranked.completion, Completion::Complete);
    assert!(ranked.remaining_score_ids.is_empty());
    assert_eq!(ranked.results.len(), 2);
    for target in &targets {
        let mut one = r.clone();
        one.scenario = Some(target.scenario.clone());
        one.execution = Execution::Skip { score_id: target.score_id };
        let expected = evaluate_fixed(&data, &owned, &one, [1, 2, 3, 4, 5], [None; 5]).unwrap();
        let actual = ranked.results.iter().find(|row| row.score_id == target.score_id).unwrap();
        assert_eq!(actual.evaluation.results, expected.results);
    }
    assert_ne!(ranked.results[0].evaluation.results[0].power, ranked.results[1].evaluation.results[0].power);
    assert_eq!(ranked.results.iter().map(|row| row.score_id).collect::<Vec<_>>(), [SCORE_ID, 2004]);
    let reverse =
        rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &[targets[1].clone(), targets[0].clone()])
            .unwrap();
    assert_eq!(reverse.results.iter().map(|row| row.score_id).collect::<Vec<_>>(), [SCORE_ID, 2004]);
    let mut limited = r.clone();
    limited.limits.max_candidates = Some(1);
    let out = rank_fixed_songs(&data, &owned, &limited, [1, 2, 3, 4, 5], [None; 5], &targets).unwrap();
    assert_eq!(out.completion, Completion::TimedOut);
    assert_eq!(out.results.len(), 1);
    assert_eq!(out.remaining_score_ids, [2004]);
    limited.limits.time_limit_ms = Some(0);
    let out = rank_fixed_songs(&data, &owned, &limited, [1, 2, 3, 4, 5], [None; 5], &targets).unwrap();
    assert!(out.results.is_empty());
    assert_eq!(out.remaining_score_ids, [SCORE_ID, 2004]);
    assert!(
        rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &[targets[0].clone(), targets[0].clone()])
            .is_err()
    );

    let live = fixed_request(
        json!({"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}}),
        json!({"kind":"score"}),
    );
    let ranked = rank_fixed_songs(&data, &owned, &live, [1, 2, 3, 4, 5], [None; 5], &targets).unwrap();
    for target in &targets {
        let mut one = live.clone();
        one.scenario = Some(target.scenario.clone());
        one.execution = Execution::Live {
            score_id: target.score_id,
            gekisou: false,
            play: ournotes_search::types::PlayPolicy::TheoreticalBest,
        };
        let fixed = evaluate_fixed(&data, &owned, &one, [1, 2, 3, 4, 5], [None; 5]).unwrap();
        let row = ranked.results.iter().find(|row| row.score_id == target.score_id).unwrap();
        assert_eq!(row.evaluation.results, fixed.results);
        assert_eq!(row.evaluation.results[0].expected_payoff.denominator, "3");
    }
}

#[test]
fn song_ranking_validates_zero_budget_inputs_and_caps_all_retained_atoms() {
    use ournotes_search::{
        auxiliary::{SongTarget, rank_fixed_songs},
        types::Scene,
    };
    use ournotes_sim::Error;
    let s = synthetic_master(5, 0, 5);
    let data = DeckData::from_json(&data_document(&s, 5, 0, 5).to_string()).unwrap();
    let owned = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let targets = [SongTarget { score_id: SCORE_ID, scenario: Scene::Free { music_id: 10 } }];
    let mut r = fixed_request(json!({"kind":"power","musicId":10,"eventParameter":false}), json!({"kind":"power"}));
    r.limits.time_limit_ms = Some(0);
    assert!(matches!(rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &targets), Err(Error::Input(_))));
    let mut r = fixed_request(
        json!({"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}}),
        json!({"kind":"score"}),
    );
    r.limits.time_limit_ms = Some(0);
    r.seed_law = None;
    assert!(matches!(rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &targets), Err(Error::Input(_))));
    let mut r = fixed_request(json!({"kind":"skip","scoreId":SCORE_ID}), json!({"kind":"score"}));
    r.limits.time_limit_ms = Some(0);
    assert!(matches!(rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 4], [None; 5], &targets), Err(Error::Input(_))));
    r.format = "wrong-format".into();
    assert!(matches!(rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &targets), Err(Error::Input(_))));
    let mut r = fixed_request(
        json!({"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}}),
        json!({"kind":"score"}),
    );
    r.seed_law.as_mut().unwrap().atoms = (0..4096).map(|root| (root, 1)).collect();
    let many: Vec<_> =
        (0..17).map(|i| SongTarget { score_id: SCORE_ID + i, scenario: Scene::Free { music_id: 10 } }).collect();
    assert!(matches!(rank_fixed_songs(&data, &owned, &r, [1, 2, 3, 4, 5], [None; 5], &many), Err(Error::Capacity(_))));
}

#[test]
fn resolved_owned_facts_use_all_three_shared_entrypoints_without_leaking_defaults() {
    use ournotes_search::{
        auxiliary::{SongTarget, evaluate_fixed},
        owned_snapshot::{GoalDependencies, OwnedSnapshot},
        types::Scene,
    };
    use ournotes_sim::Error;
    let mut s = synthetic_master(6, 0, 6);
    extend_table(
        &mut s,
        "MasterMemberCardLevelLimit",
        (1..=5)
            .flat_map(|rarity| {
                (1..=5).map(
                    move |awake| json!({"_id":rarity*10+awake,"_rarity":rarity,"_awakeCount":awake,"_limitLevel":100}),
                )
            })
            .collect(),
    );
    let document = data_document(&s, 6, 0, 6);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let mut member_facts = roster_document(6, 0, 6)["members"].as_array().unwrap().clone();
    member_facts.retain(|row| row["id"] != 6);
    for row in &mut member_facts {
        row.as_object_mut().unwrap().remove("liveSkillLevel");
        row.as_object_mut().unwrap().remove("gekisouSkillLevel");
    }
    let value = json!({
        "format":"ournotes.owned-snapshot/1","datasetId":"synthetic-shared","revision":"owned-r1",
        "ownedFacts":{"memberIds":[1,2,3,4,5,6],"snapIds":[],"memberCoverage":"complete","snapCoverage":"complete"},
        "eligible":{"members":member_facts,"snaps":[]},
        "player":{"characterRanks":{"coverage":"complete","values":(1..=6).map(|id|json!({"id":id,"value":10})).collect::<Vec<_>>()},
            "characterTotalRank":null,"vipRank":3,"bandItems":[],"memory":{"musicRanks":[],"unlockedMembers":[],"unlockedSnaps":[]},"eventIds":[EVENT_ID]},
        "assumptions":[{"path":"player.vipRank","reason":"synthetic fixture"}]
    });
    let snapshot = OwnedSnapshot::from_json(&value.to_string()).unwrap();
    let mut reference = Roster::from_json(&roster_document(6, 0, 6).to_string()).unwrap();
    reference.members.retain(|member| member.id != 6);
    for (goal, execution, metric) in [
        (GoalDependencies::Power, json!({"kind":"power","musicId":10,"eventParameter":false}), json!({"kind":"power"})),
        (GoalDependencies::Skip, json!({"kind":"skip","scoreId":SCORE_ID}), json!({"kind":"score"})),
    ] {
        let resolution = snapshot.resolve_data(&data, "synthetic-shared", goal);
        assert!(resolution.missing.is_empty(), "{:?}", resolution.missing);
        assert!(resolution.errors.is_empty(), "{:?}", resolution.errors);
        let resolved = resolution.resolved.unwrap();
        let r = fixed_request(execution, metric);
        let out = resolved.evaluate_fixed(&data, &r, [1, 2, 3, 4, 5], [None; 5]).unwrap();
        let expected = evaluate_fixed(&data, &reference, &r, [1, 2, 3, 4, 5], [None; 5]).unwrap();
        assert_eq!(out.results, expected.results);
        let scope = &out.resolved_context["ownedSnapshot"];
        assert_eq!(scope["revision"], "owned-r1");
        assert_eq!(scope["eligibleCoversDeclaredOwned"], false);
        assert_eq!(scope["totalRankOrigin"], "derivedCompleteRanks");
        assert_eq!(scope["assumptions"][0]["reason"], "synthetic fixture");
        assert!(resolved.snapshot().eligible.members[0].live_skill_level.is_none());
        assert!(resolved.snapshot().player.character_total_rank.is_none());
        assert!(resolved.snapshot().owned_facts.member_ids.contains(&6));
        let searched = resolved.recommend(&data, &r).unwrap();
        assert_eq!(searched.resolved_context["ownedSnapshot"], *scope);
        assert!(searched.results.iter().all(|result| !result.members.contains(&6)));
        assert!(resolved.evaluate_fixed(&data, &r, [1, 2, 3, 4, 6], [None; 5]).is_err());
        let foreign = DeckData::from_json(&document.to_string()).unwrap();
        assert!(matches!(resolved.evaluate_fixed(&foreign, &r, [1, 2, 3, 4, 5], [None; 5]), Err(Error::Input(_))));
        let mut changed_events = r.clone();
        changed_events.context = Some(serde_json::from_value(json!({"powerSnapshot":{"eventIds":[]}})).unwrap());
        assert!(matches!(
            resolved.evaluate_fixed(&data, &changed_events, [1, 2, 3, 4, 5], [None; 5]),
            Err(Error::Input(_))
        ));
        let live = fixed_request(
            json!({"kind":"live","scoreId":SCORE_ID,"gekisou":false,"play":{"kind":"theoreticalBest"}}),
            json!({"kind":"score"}),
        );
        assert!(matches!(resolved.evaluate_fixed(&data, &live, [1, 2, 3, 4, 5], [None; 5]), Err(Error::Input(_))));
        if goal == GoalDependencies::Skip {
            let targets = [SongTarget { score_id: SCORE_ID, scenario: Scene::Free { music_id: 10 } }];
            let ranked = resolved.rank_fixed_songs(&data, &r, [1, 2, 3, 4, 5], [None; 5], &targets).unwrap();
            assert_eq!(ranked.results[0].evaluation.results, out.results);
            assert_eq!(ranked.owned_snapshot_scope.as_ref().unwrap(), scope);
            let mut zero = r;
            zero.limits.time_limit_ms = Some(0);
            let empty = resolved.rank_fixed_songs(&data, &zero, [1, 2, 3, 4, 5], [None; 5], &targets).unwrap();
            assert!(empty.results.is_empty());
            assert_eq!(empty.owned_snapshot_scope.unwrap()["revision"], "owned-r1");
        }
    }
}

/// The owned snapshot stating exactly the facts of `roster_document`.
fn snapshot_document(dataset_id: &str, members: i64, snaps: i64, characters: i64) -> Value {
    let roster = roster_document(members, snaps, characters);
    json!({
        "format":"ournotes.owned-snapshot/1","datasetId":dataset_id,"revision":"r1",
        "ownedFacts":{"memberIds":(1..=members).collect::<Vec<_>>(),"snapIds":(1..=snaps).collect::<Vec<_>>(),
            "memberCoverage":"complete","snapCoverage":"complete"},
        "eligible":{"members":roster["members"],"snaps":roster["snaps"]},
        "player":{"characterRanks":{"coverage":"complete",
                "values":(1..=characters).map(|id|json!({"id":id,"value":10})).collect::<Vec<_>>()},
            "characterTotalRank":null,"vipRank":3,"bandItems":[],
            "memory":{"musicRanks":[],"unlockedMembers":[],"unlockedSnaps":[]},"eventIds":[EVENT_ID]},
        "assumptions":[]
    })
}

#[test]
fn snapshot_recommendation_matches_the_same_roster_and_locates_every_input_problem() {
    use ournotes_search::engine::{SnapshotStatus, recommend, recommend_snapshot};
    let mut s = synthetic_master(6, 2, 5);
    extend_table(
        &mut s,
        "MasterMemberCardLevelLimit",
        (1..=5)
            .flat_map(|rarity| {
                (1..=5).map(
                    move |awake| json!({"_id":rarity*10+awake,"_rarity":rarity,"_awakeCount":awake,"_limitLevel":100}),
                )
            })
            .collect(),
    );
    let data = DeckData::from_json(&data_document(&s, 6, 2, 5).to_string()).unwrap();
    let id = data.sha256.clone().unwrap();
    assert!(id.len() == 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let snapshot = snapshot_document(&id, 6, 2, 5);
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for metric in [json!({"kind":"score"}), json!({"kind":"clientEventPoints","eventId":EVENT_ID})] {
            let request = joint_request_json(mode, gekisou, metric.clone()).to_string();
            let answer = recommend_snapshot(&data, &snapshot.to_string(), &request, None);
            assert_eq!(
                answer.status,
                SnapshotStatus::Ok,
                "{mode} {metric:?}: {:?} {:?}",
                answer.missing,
                answer.errors
            );
            assert_eq!(answer.dataset_id.as_deref(), Some(id.as_str()));
            let result = answer.result.unwrap();
            let expected = recommend(&data, &roster, &joint_request(mode, gekisou, metric.clone())).unwrap();
            assert_eq!(result.results, expected.results, "{mode} {metric:?}");
            assert_eq!(result.resolved_context["ownedSnapshot"]["revision"], "r1");
        }
    }

    let gekisou = joint_request_json("mission", true, json!({"kind":"score"})).to_string();
    let normal = joint_request_json("free", false, json!({"kind":"score"})).to_string();
    let mut unknown = snapshot.clone();
    unknown["eligible"]["members"][3]["gekisouSkillLevel"] = Value::Null;
    let answer = recommend_snapshot(&data, &unknown.to_string(), &gekisou, None);
    assert_eq!(answer.status, SnapshotStatus::Incomplete);
    assert!(answer.result.is_none() && answer.errors.is_empty());
    let missing: Vec<_> = answer.missing.iter().map(|i| (i.path.as_str(), i.code.as_str())).collect();
    assert_eq!(missing, [("eligible.members[4].gekisouSkillLevel", "missing")]);
    let wire = serde_json::to_value(&answer).unwrap();
    assert_eq!(wire["format"], "ournotes-deck.snapshot-recommendation/1");
    assert_eq!(wire["status"], "incomplete");
    assert_eq!(wire["result"], Value::Null);
    // Normal Live does not read Gekisou levels.
    assert_eq!(recommend_snapshot(&data, &unknown.to_string(), &normal, None).status, SnapshotStatus::Ok);

    let mut foreign = snapshot.clone();
    foreign["datasetId"] = json!("0".repeat(64));
    let answer = recommend_snapshot(&data, &foreign.to_string(), &normal, None);
    assert_eq!(answer.status, SnapshotStatus::Invalid);
    assert!(answer.errors.iter().any(|i| i.path == "datasetId" && i.code == "dataset_mismatch"));

    let answer = recommend_snapshot(&data, "{", "{}", None);
    assert_eq!(answer.status, SnapshotStatus::Invalid);
    let errors: Vec<_> = answer.errors.iter().map(|i| (i.path.as_str(), i.code.as_str())).collect();
    assert_eq!(errors, [("request", "parse"), ("snapshot", "parse")]);

    let mut no_law = joint_request_json("free", false, json!({"kind":"score"}));
    no_law.as_object_mut().unwrap().remove("seedLaw");
    let answer = recommend_snapshot(&data, &snapshot.to_string(), &no_law.to_string(), None);
    assert_eq!(answer.status, SnapshotStatus::Invalid);
    let errors: Vec<_> = answer.errors.iter().map(|i| (i.path.as_str(), i.code.as_str())).collect();
    assert_eq!(errors, [("request", "input")]);
}

/// Telemetry without its millisecond fields, which are the only ones allowed to differ between two runs.
fn untimed(outcome: &ournotes_search::types::RecommendationOutcome) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|key, _| !key.ends_with("Ms"));
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut telemetry = serde_json::to_value(&outcome.telemetry).unwrap();
    strip(&mut telemetry);
    telemetry
}

#[test]
fn progress_reports_exact_decks_and_leave_the_search_unchanged() {
    use ournotes_search::{
        auxiliary::evaluate_fixed,
        engine::{Progress, recommend, recommend_with_progress},
        search::Completion,
        types::RecommendationOutcome,
    };
    use std::time::Duration;
    let synth = synthetic_master(6, 2, 5);
    let data = DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap();
    let numerator =
        |out: &RecommendationOutcome| out.results.first().map(|d| d.expected_payoff.numerator.parse::<i128>().unwrap());
    for (mode, gekisou) in [("free", false), ("mission", true)] {
        for metric in [json!({"kind":"score"}), json!({"kind":"clientEventPoints","eventId":EVENT_ID})] {
            let case = format!("{mode} {metric:?}");
            let request = joint_request(mode, gekisou, metric);
            let plain = recommend(&data, &roster, &request).unwrap();
            let mut reports = Vec::new();
            let mut report = |out: &RecommendationOutcome| reports.push(out.clone());
            let progress = Progress { interval: Duration::ZERO, report: &mut report };
            let hooked = recommend_with_progress(&data, &roster, &request, progress).unwrap();
            assert_eq!(hooked.completion, Completion::Complete, "{case}");
            assert_eq!(hooked.results, plain.results, "{case}");
            assert_eq!(untimed(&hooked), untimed(&plain), "{case}");
            assert!(!reports.is_empty(), "{case}");
            assert_eq!(reports.last().unwrap().results, hooked.results, "{case}");
            let mut decks = BTreeMap::new();
            for (before, after) in reports.iter().zip(&reports[1..]) {
                assert!(before.telemetry.nodes <= after.telemetry.nodes, "{case}");
                assert!(numerator(before) <= numerator(after), "{case}");
            }
            for r in &reports {
                assert_eq!(r.completion, Completion::TimedOut, "{case}");
                assert_eq!(r.resolved_context, plain.resolved_context, "{case}");
                assert!(r.telemetry.nodes <= hooked.telemetry.nodes, "{case}");
                assert!(!r.telemetry.proof.complete && r.telemetry.proof.upper_bound.is_none(), "{case}");
                for d in &r.results {
                    decks.insert((d.members, d.snaps), d.expected_payoff.clone());
                }
            }
            // Every reported deck is exactly evaluated.
            for ((members, snaps), payoff) in decks {
                let fixed = evaluate_fixed(&data, &roster, &request, members, snaps).unwrap();
                assert_eq!(fixed.results[0].expected_payoff, payoff, "{case} {members:?} {snaps:?}");
            }
        }
    }
    // The canonical power search makes no reports.
    let power = fixed_request(json!({"kind":"power","musicId":10,"eventParameter":false}), json!({"kind":"power"}));
    let mut count = 0;
    let mut report = |_: &RecommendationOutcome| count += 1;
    let progress = Progress { interval: Duration::ZERO, report: &mut report };
    let out = recommend_with_progress(&data, &roster, &power, progress).unwrap();
    assert_eq!(out.results, recommend(&data, &roster, &power).unwrap().results);
    assert_eq!(count, 0);
}
