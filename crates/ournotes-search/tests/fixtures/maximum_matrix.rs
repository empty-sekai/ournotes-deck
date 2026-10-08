//! Maximum terminal payoffs over independently enumerated physical decks and deterministic orders.
use super::common::{Rng, set_column};
use super::{
    EVENT_ID, SCORE_ID, context_document, correctness_matrix, data_document, roster_document, synthetic_master,
};
use ournotes_search::{
    auxiliary, engine, handler,
    search::Completion,
    types::{Aggregation, Execution, Optimality, OrderResult, RecommendationRequest, RecommendedDeck, Scene, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, pool::Pool};
use serde_json::{Value, json};

type Key = ([i64; 5], [Option<i64>; 5]);

#[derive(Debug)]
struct TerminalMaximum {
    key: Key,
    power: i32,
    payoff: i128,
    score: i32,
    best_order: Option<OrderResult>,
}

fn inputs(seed: u64, members: i64, snaps: i64, characters: i64, effects: usize) -> (DeckData, Roster) {
    let mut synth = synthetic_master(members, snaps, characters);
    let mut rng = Rng::new(seed);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        for column in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
            row[column] = json!(rng.range(4500, 12500));
        }
    });
    set_column(&mut synth, "MasterLiveSkillEffect", &mut |row| {
        row["_activationTimeSecond"] = json!(0.14 + rng.below(5) as f64 * 0.13);
        if effects == 1 {
            row["_effectValue"] = json!(-row["_effectValue"].as_i64().unwrap() / 2);
        }
    });
    set_column(&mut synth, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(if effects == 2 { 5 } else { 3 });
        row["_supportSkillId02"] = json!(if effects == 2 { 6 } else { 1 });
    });
    set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |row| {
        row["_scoreBonusPercent"] = json!(match row["_count"].as_i64().unwrap() {
            1 => 250,
            2 => 125,
            _ => 0,
        });
    });
    // Nonmonotone reward tiers require maximizing terminal payoff, not final score alone.
    for table in ["MasterLiveEventPoint", "MasterChallengeLiveEventPoint", "MasterLiveChallengePoint"] {
        set_column(&mut synth, table, &mut |row| {
            row["_value"] = json!([300, 900, 20, 80][row["_scoreRank"].as_u64().unwrap() as usize - 2]);
        });
    }
    let mut document = data_document(&synth, members, snaps, characters);
    document["charts"][0]["skillEvents"]["timeMs"] = json!([160, 320, 610, 850, 1040]);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(members, snaps, characters).to_string()).unwrap();
    (data, roster)
}

fn request(data: &DeckData, scene: &str, metric: Value) -> RecommendationRequest {
    let skipped = matches!(scene, "skip" | "challenge-skip");
    let challenge = matches!(scene, "challenge" | "challenge-skip");
    let mut request =
        correctness_matrix::request(data, if skipped || challenge { "free" } else { scene }, !skipped, metric);
    if challenge {
        request.scenario = Some(Scene::Challenge { music_id: 70 });
    }
    if skipped {
        request.execution = Execution::Skip { score_id: SCORE_ID };
        request.context = serde_json::from_value(context_document(true, false, false)).unwrap();
    }
    request.aggregation = Aggregation::Maximum;
    request
}

fn oracle(data: &DeckData, roster: &Roster, request: &RecommendationRequest, count: usize) -> Vec<TerminalMaximum> {
    let pool = Pool::new(&data.master, roster).unwrap();
    let keys = correctness_matrix::domain(&pool, request);
    assert_eq!(keys.len(), count);
    let mut expected = request.clone();
    expected.aggregation = Aggregation::Expected;
    expected.strategy = Strategy::Exhaustive;
    let built = handler::build_card_pool(data, roster, &expected).unwrap();
    let mut rows = Vec::new();
    for (members, snaps) in keys {
        let result = auxiliary::evaluate_built(&built, members, snaps).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.results.len(), 1);
        let row = &result.results[0];
        let (payoff, score) = if matches!(request.execution, Execution::Skip { .. }) {
            let value = row.expected_payoff.as_ref().unwrap();
            let numerator = value.numerator.parse::<i128>().unwrap();
            let denominator = value.denominator.parse::<i128>().unwrap();
            assert_eq!(numerator % denominator, 0);
            (numerator / denominator, row.maximum_score.unwrap())
        } else {
            assert_eq!(row.order_outcomes.len(), 120);
            let payoff = row.order_outcomes.iter().map(|value| value.2).max().unwrap();
            let score = row.order_outcomes.iter().map(|value| value.1).max().unwrap();
            (payoff, score)
        };
        rows.push(TerminalMaximum {
            key: (members, snaps),
            power: row.power,
            payoff,
            score,
            best_order: row.best_order.clone(),
        });
    }
    rows.sort_by(|a, b| b.payoff.cmp(&a.payoff).then(b.power.cmp(&a.power)).then(a.key.cmp(&b.key)));
    rows
}

fn verify_row(actual: &RecommendedDeck, expected: &TerminalMaximum) {
    assert_eq!((actual.members, actual.snaps), expected.key);
    assert_eq!(actual.power, expected.power);
    let value = actual.objective_value.as_ref().unwrap();
    assert_eq!(value.denominator, "1");
    assert_eq!(value.numerator.parse::<i128>().unwrap(), expected.payoff);
    assert!(actual.expected_payoff.is_none() && actual.expected_score.is_none());
    assert_eq!(actual.maximum_score, Some(expected.score));
    assert_eq!(actual.best_order, expected.best_order);
}

fn verify(data: &DeckData, roster: &Roster, request: &RecommendationRequest, count: usize, name: &str) {
    let reference = oracle(data, roster, request, count);
    for (strategy, cache, k) in [
        (Strategy::Exhaustive, 0, 5),
        (Strategy::BranchAndBound, 0, 1),
        (Strategy::BranchAndBound, 32, 3),
        (Strategy::BranchAndBound, 32, 5),
    ] {
        let mut current = request.clone();
        current.strategy = strategy;
        current.k = k;
        current.limits.cache_entries = cache;
        let result = engine::recommend(data, roster, &current).unwrap();
        assert_eq!(result.completion, Completion::Complete, "{name}: k={k} cache={cache}");
        assert_eq!(result.optimality, Optimality::Proven, "{name}");
        assert_eq!(result.results.len(), reference.len().min(k), "{name}");
        for (actual, expected) in result.results.iter().zip(&reference) {
            verify_row(actual, expected);
        }
    }
}

#[test]
fn maximum_scene_metric_matrix_reduces_independent_order_terminals() {
    let mut cases = 0;
    for (seed, effects) in [(173, 0), (991, 2)] {
        let (data, roster) = inputs(seed, 5, 1, 5, effects);
        for scene in ["free", "mission", "battle", "arena", "challenge", "skip", "challenge-skip"] {
            let mut metrics = vec![
                json!({"kind":"score"}),
                json!({"kind":"scoreAtLeast","threshold":450000}),
                json!({"kind":"cappedScore","threshold":450000}),
                json!({"kind":"clientEventPoints","eventId":EVENT_ID}),
                json!({"kind":"conditionalClientEventItems","eventId":EVENT_ID,"resourceType":11,"resourceId":9}),
            ];
            if !scene.starts_with("challenge") {
                metrics.push(json!({"kind":"clientChallengePoints","eventId":EVENT_ID}));
            }
            if !matches!(scene, "skip" | "challenge-skip") {
                metrics.push(json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":800}));
            }
            for metric in metrics {
                let request = request(&data, scene, metric);
                verify(&data, &roster, &request, 6, &format!("seed={seed} {scene} {:?}", request.metric));
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 90);
    eprintln!("maximum scene matrix: {cases} cases, 540 physical candidates");
}

#[test]
fn maximum_constraints_and_signed_effects_preserve_canonical_topk() {
    let cases = [
        ("required-and-excluded", json!({"leader":3,"includeMembers":[1,2],"excludeMembers":[7]}), 93),
        ("excluded-snap", json!({"leader":3,"includeMembers":[1,2],"excludeMembers":[7],"excludeSnaps":[2]}), 18),
        ("five-members", json!({"leader":3,"excludeMembers":[6,7]}), 31),
        ("all-leaders", json!({"noSnaps":true}), 55),
        ("same-character", json!({"includeMembers":[1,7],"noSnaps":true}), 0),
    ];
    for (seed, effects) in [(67, 0), (2027, 1)] {
        let (data, roster) = inputs(seed, 7, 2, 6, effects);
        for (name, constraints, count) in &cases {
            let mut request = request(&data, "mission", json!({"kind":"score"}));
            request.constraints = serde_json::from_value(constraints.clone()).unwrap();
            verify(&data, &roster, &request, *count, &format!("seed={seed} {name}"));
        }
    }
    let (mut data, roster) = inputs(173, 5, 0, 5, 0);
    for effect in &mut data.master.leader_skill_effects {
        effect.effect_value = 0;
    }
    let mut tied = request(&data, "free", json!({"kind":"scoreAtLeast","threshold":1}));
    tied.constraints.leader = None;
    let rows = oracle(&data, &roster, &tied, 5);
    assert!(rows.windows(2).all(|pair| pair[0].payoff == pair[1].payoff && pair[0].power == pair[1].power));
    verify(&data, &roster, &tied, 5, "canonical-leader-ties");
    eprintln!("maximum constraint matrix: 11 cases, 399 physical candidates");
}

#[test]
fn maximum_live_probability_matches_both_deterministic_skill_worlds() {
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_liveSkillID"] = json!(if row["_id"] == 1 { 1 } else { 2 });
    });
    set_column(&mut synth, "MasterLiveSkillEffect", &mut |row| {
        row["_activationTimeSecond"] = json!(0.23);
        if row["_liveSkillID"] == 1 {
            row["_skillConditionGroup"] = json!(66);
        }
    });
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    for probability in [0, 1, 50, 100] {
        set_column(&mut synth, "MasterSkillCondition", &mut |row| {
            if row["_id"] == 76 {
                row["_conditionValues"] = json!([probability]);
            }
        });
        let data = DeckData::from_json(&data_document(&synth, 5, 0, 5).to_string()).unwrap();
        for metric in [
            json!({"kind":"score"}),
            json!({"kind":"cappedScore","threshold":300000}),
            json!({"kind":"scoreAtLeast","threshold":300000}),
        ] {
            let request = request(&data, "free", metric);
            let mut worlds = Vec::new();
            for active in [false, true] {
                if (active && probability == 0) || (!active && probability == 100) {
                    continue;
                }
                let mut deterministic = DeckData::from_json(&data_document(&synth, 5, 0, 5).to_string()).unwrap();
                for effect in &mut deterministic.master.live_skill_effects {
                    if effect.live_skill_id == 1 {
                        effect.skill_condition_group = 0;
                        if !active {
                            effect.effect_value = 0;
                        }
                    }
                }
                deterministic.master.reindex().unwrap();
                worlds.push(oracle(&deterministic, &roster, &request, 1).remove(0));
            }
            let reference = TerminalMaximum {
                key: worlds[0].key,
                power: worlds[0].power,
                payoff: worlds.iter().map(|world| world.payoff).max().unwrap(),
                score: worlds.iter().map(|world| world.score).max().unwrap(),
                best_order: worlds.iter().filter_map(|world| world.best_order.clone()).max_by(|a, b| {
                    a.payoff
                        .parse::<i128>()
                        .unwrap()
                        .cmp(&b.payoff.parse::<i128>().unwrap())
                        .then(a.score.cmp(&b.score))
                        .then_with(|| b.performance_order.cmp(&a.performance_order))
                }),
            };
            for cache in [0, 32] {
                let mut current = request.clone();
                current.limits.cache_entries = cache;
                let result = engine::recommend(&data, &roster, &current).unwrap();
                assert_eq!(result.completion, Completion::Complete, "rate={probability} cache={cache}");
                assert_eq!(result.results.len(), 1);
                verify_row(&result.results[0], &reference);
            }
        }
    }
}

#[test]
fn maximum_fine_caps_preserve_zero_score_canonical_ties() {
    use ournotes_search::types::PlayPolicy;
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterLeaderSkillEffect", &mut |row| row["_effectValue"] = json!(0));
    for table in ["MasterLiveSkillEffect", "MasterGekisouSkillEffect"] {
        set_column(&mut synth, table, &mut |row| {
            row["_skillEffectType"] = json!(2000);
            row["_effectValue"] = json!(0);
            row["_skillConditionGroup"] = json!(0);
        });
    }
    let data = DeckData::from_json(&data_document(&synth, 5, 0, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let mut current = request(&data, "mission", json!({"kind":"score"}));
    current.constraints.leader = None;
    let Execution::Live { play: PlayPolicy::Stream { stream }, .. } = &mut current.execution else {
        panic!("explicit judgement stream");
    };
    let miss = 1;
    assert_eq!(
        ournotes_sim::live::score::convert_score_type(i64::from(miss)).unwrap(),
        ournotes_sim::live::score::MISS
    );
    for note in &mut stream.judged {
        note[2] = miss;
    }
    let reference = oracle(&data, &roster, &current, 5);
    assert!(reference.iter().all(|row| row.score == 0 && row.payoff == 0));
    assert!(reference.windows(2).all(|pair| pair[0].power == pair[1].power && pair[0].key < pair[1].key));
    current.k = 3;
    current.strategy = Strategy::BranchAndBound;
    current.limits.cache_entries = 0;
    let result = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.telemetry.leaves.fine_orders, 240);
    assert_eq!(result.telemetry.leaves.fine_pruned, 0);
    assert_eq!(result.results.len(), 3);
    for (actual, expected) in result.results.iter().zip(&reference) {
        verify_row(actual, expected);
        assert_eq!(actual.best_order.as_ref().unwrap().performance_order, [0, 1, 2, 3, 4]);
    }
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(result.results, exhaustive.results);
}

#[test]
fn maximum_fine_caps_match_all_terminal_orders_at_goal_boundaries() {
    for (seed, effects) in [(173, 0), (2027, 1)] {
        let (data, roster) = inputs(seed, 5, 1, 5, effects);
        if effects == 1 {
            assert!(data.master.live_skill_effects.iter().any(|row| row.effect_value < 0));
        }
        let score = request(&data, "mission", json!({"kind":"score"}));
        let scores = oracle(&data, &roster, &score, 6);
        let boundary = scores[2].score;
        assert!(boundary > 1 && boundary < i32::MAX);
        let mut metrics = vec![json!({"kind":"score"})];
        for threshold in [boundary - 1, boundary, boundary + 1] {
            metrics.extend([
                json!({"kind":"scoreAtLeast","threshold":threshold}),
                json!({"kind":"cappedScore","threshold":threshold}),
                json!({"kind":"scoreAndLifeAtLeast","threshold":threshold,"minFinalLife":800}),
            ]);
        }
        for metric in metrics {
            let mut current = request(&data, "mission", metric);
            let reference = oracle(&data, &roster, &current, 6);
            current.k = 3;
            current.strategy = Strategy::Exhaustive;
            let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(exhaustive.completion, Completion::Complete);
            assert_eq!(exhaustive.results.len(), 3);
            for (actual, expected) in exhaustive.results.iter().zip(&reference) {
                verify_row(actual, expected);
            }
            current.strategy = Strategy::BranchAndBound;
            for cache in [0, 32] {
                current.limits.cache_entries = cache;
                let actual = engine::recommend(&data, &roster, &current).unwrap();
                assert_eq!(actual.completion, Completion::Complete);
                assert_eq!(actual.optimality, Optimality::Proven);
                assert_eq!(actual.results, exhaustive.results, "seed={seed} metric={:?} cache={cache}", current.metric);
            }
        }
    }
}

#[test]
fn maximum_fine_admission_checks_probability_behind_a_false_predicate() {
    use super::common::extend_table;
    let mut synth = synthetic_master(5, 1, 5);
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        vec![json!({"_id":76001,"_conditionType":5000,"_conditionValues":[],
                    "_conditionTargetIDs":[],"_isPositive":true})],
    );
    extend_table(
        &mut synth,
        "MasterSkillConditionSet",
        vec![json!({"_id":76001,"_group":76001,"_conditionIds":[76001,76]})],
    );
    set_column(&mut synth, "MasterLiveSkillEffect", &mut |row| {
        row["_skillConditionGroup"] = json!(76001);
    });
    let data = DeckData::from_json(&data_document(&synth, 5, 1, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 1, 5).to_string()).unwrap();
    let mut current = request(&data, "mission", json!({"kind":"score"}));
    let reference = oracle(&data, &roster, &current, 6);
    current.k = 3;
    current.strategy = Strategy::BranchAndBound;
    let result = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.telemetry.leaves.fine_orders, 0);
    assert_eq!(result.results.len(), 3);
    for (actual, expected) in result.results.iter().zip(&reference) {
        verify_row(actual, expected);
    }
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(result.results, exhaustive.results);
}

#[cfg(feature = "search-diagnostics")]
fn maximum_carrier_inputs(negative: bool) -> (DeckData, Roster) {
    use super::common::{extend_table, replace_table};
    let mut synth = synthetic_master(6, 1, 6);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        row["_gekisouSkillID"] = json!(if matches!(row["_id"].as_i64().unwrap(), 1 | 4) { 1 } else { 2 });
    });
    replace_table(
        &mut synth,
        "MasterGekisouSkill",
        json!([
            {"_id":1,"_skillCategories":[1],"_gekisouMissionType":1},
            {"_id":2,"_skillCategories":[1],"_gekisouMissionType":1}
        ]),
    );
    replace_table(
        &mut synth,
        "MasterGekisouSupportSkill",
        json!([{"_id":1,"_skillCategories":[1],"_gekisouMissionType":1}]),
    );
    set_column(&mut synth, "MasterSupportCard", &mut |row| {
        row["_gekisouSupportSkillId01"] = json!(1);
        row["_gekisouSupportSkillId02"] = json!(0);
    });
    set_column(&mut synth, "MasterSupportCardRank", &mut |row| {
        row["_gekisouSupportSkill01Level"] = json!(1);
        row["_gekisouSupportSkill02Level"] = json!(0);
    });
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        vec![json!({"_id":9601,"_conditionType":8000,"_conditionValues":[],
                    "_conditionTargetIDs":[],"_isPositive":false})],
    );
    extend_table(&mut synth, "MasterSkillConditionSet", vec![json!({"_id":9601,"_group":9601,"_conditionIds":[9601]})]);
    extend_table(&mut synth, "MasterSkillEffectSetting", vec![json!({"_id":9601,"_skillEffectType":12000,"_phase":2})]);
    let combo_row = |id: i64, field: &str, value: i64| {
        json!({"_id":id,field:1,"_level":1,"_skillTriggerType":2,
            "_skillTriggerConditionGroup":9601,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":12000,"_activationTimeSecond":0.0,
            "_effectValue":value,"_maxEffectValue":0,"_effectLimitCount":0,
            "_skillCumulativeConditionID":0,"_effectExecuteLimitCount":0,
            "_effectExecuteLimitResetConditionGroup":0})
    };
    replace_table(&mut synth, "MasterGekisouSkillEffect", json!([combo_row(9601, "_gekisouSkillID", 2)]));
    replace_table(&mut synth, "MasterGekisouSupportSkillEffect", json!([combo_row(9602, "_gekisouSupportSkillID", 3)]));
    replace_table(
        &mut synth,
        "MasterLiveComboScoreBonus",
        json!([
            {"_id":1,"_comboBonusType":0,"_requiredComboCount":1,"_bonusFactor":0.0},
            {"_id":2,"_comboBonusType":1,"_requiredComboCount":1,"_bonusFactor":0.0},
            {"_id":3,"_comboBonusType":1,"_requiredComboCount":4,"_bonusFactor":0.2},
            {"_id":4,"_comboBonusType":1,"_requiredComboCount":7,"_bonusFactor":0.3},
            {"_id":5,"_comboBonusType":1,"_requiredComboCount":10,"_bonusFactor":0.4},
            {"_id":6,"_comboBonusType":1,"_requiredComboCount":20,"_bonusFactor":0.7}
        ]),
    );
    set_column(&mut synth, "MasterLiveSkillEffect", &mut |row| {
        row["_activationTimeSecond"] = json!(0.24 + row["_liveSkillID"].as_i64().unwrap() as f64 * 0.1);
        if negative && row["_liveSkillID"] == 2 {
            row["_effectValue"] = json!(-row["_effectValue"].as_i64().unwrap() / 2);
        }
    });
    set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |row| {
        row["_scoreBonusPercent"] = json!(match row["_count"].as_i64().unwrap() {
            1 => 250,
            2 => 125,
            _ => 0,
        });
    });
    let mut document = data_document(&synth, 6, 1, 6);
    document["charts"][0]["skillEvents"]["timeMs"] = json!([160, 320, 610, 850, 1040]);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 1, 6).to_string()).unwrap();
    (data, roster)
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn maximum_carrier_prefixes_bound_all_terminal_completions() {
    use ournotes_search::search::diagnostics;
    use std::collections::{BTreeMap, BTreeSet};
    let mut totals = (0usize, 0usize, 0usize);
    let mut deck_identities = BTreeSet::new();
    let mut prefix_identities = BTreeSet::new();
    for (scene, negative, constrained) in [("mission", false, false), ("battle", false, false), ("arena", false, true)]
    {
        let (data, roster) = maximum_carrier_inputs(negative);
        assert_eq!(data.master.live_skill_effects.iter().any(|row| row.effect_value < 0), negative);
        let mut current = request(&data, scene, json!({"kind":"score"}));
        if constrained {
            current.constraints.include_members = vec![1];
            current.constraints.exclude_members = vec![6];
        }
        let count = if constrained { 6 } else { 30 };
        let reference = oracle(&data, &roster, &current, count);
        current.strategy = Strategy::BranchAndBound;
        let built = handler::build_card_pool(&data, &roster, &current).unwrap();
        let mut prefixes = BTreeMap::new();
        let mut carrier_counts = BTreeSet::new();
        let mut tightened = 0;
        for row in &reference {
            deck_identities.insert(row.key);
            for depth in 1..=5 {
                let bound = diagnostics::maximum_prefix_upper(&built, row.key.0, row.key.1, depth).unwrap().unwrap();
                assert!(bound.carrier_upper.is_some() && bound.keyed, "{scene} depth={depth}: {bound:?}");
                assert!(bound.upper <= bound.pool_upper);
                assert!(bound.upper >= i128::from(row.score), "{scene} depth={depth}: {bound:?} vs {row:?}");
                assert!(bound.carrier_upper.unwrap() >= i128::from(row.score));
                assert!(bound.power >= i64::from(row.power));
                let prefix: Vec<_> =
                    [2, 0, 1, 3, 4][..depth].iter().map(|&slot| (row.key.0[slot], row.key.1[slot])).collect();
                prefix_identities.insert(prefix.clone());
                let entry = prefixes.entry(prefix).or_insert((bound, 0usize, i32::MIN));
                assert_eq!(entry.0, bound, "unplaced identities must not affect a prefix bound");
                entry.1 += 1;
                entry.2 = entry.2.max(row.score);
                carrier_counts.insert(bound.carriers_placed);
                tightened += usize::from(bound.upper < bound.pool_upper);
            }
        }
        assert!(prefixes.values().any(|(_, completions, _)| *completions > 1));
        assert!(carrier_counts.contains(&0) && carrier_counts.iter().any(|&count| count >= 2));
        for (bound, _, best) in prefixes.values() {
            assert!(bound.upper >= i128::from(*best));
        }
        totals.0 += reference.len() * 120;
        totals.1 += prefixes.len();
        totals.2 += tightened;
        for cache in [0, 32] {
            current.k = 3;
            current.limits.cache_entries = cache;
            let result = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(result.completion, Completion::Complete);
            assert_eq!(result.optimality, Optimality::Proven);
            assert_eq!(result.results.len(), 3);
            for (actual, expected) in result.results.iter().zip(&reference) {
                verify_row(actual, expected);
            }
        }
    }
    assert_eq!(totals.0, 7920);
    assert_eq!(totals.1, 160);
    assert_eq!(deck_identities.len(), 30);
    assert_eq!(prefix_identities.len(), 70);
    assert!(totals.2 > 0, "carrier information must tighten at least one prefix");
    eprintln!(
        "maximum carrier audit: {} terminal order cases, {} context-prefix cases, {} physical deck identities, {} physical prefix identities, {} tightened witnesses",
        totals.0,
        totals.1,
        deck_identities.len(),
        prefix_identities.len(),
        totals.2
    );
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn maximum_carrier_prefix_admission_preserves_other_objectives_and_refusals() {
    use ournotes_search::search::diagnostics;
    let (mut data, roster) = maximum_carrier_inputs(false);
    let members = [1, 2, 3, 4, 5];
    let snaps = [None; 5];
    for (scene, metric) in [
        ("free", json!({"kind":"score"})),
        ("mission", json!({"kind":"scoreAtLeast","threshold":1})),
        ("mission", json!({"kind":"cappedScore","threshold":1})),
    ] {
        let current = request(&data, scene, metric);
        let built = handler::build_card_pool(&data, &roster, &current).unwrap();
        let cap = diagnostics::maximum_prefix_upper(&built, members, snaps, 1).unwrap().unwrap();
        assert!(cap.carrier_upper.is_none() && !cap.keyed);
        assert_eq!(cap.upper, cap.pool_upper);
        assert!(diagnostics::maximum_prefix_upper(&built, members, snaps, 0).is_err());
        assert!(diagnostics::maximum_prefix_upper(&built, members, snaps, 6).is_err());
    }
    data.master.live_musics.iter_mut().find(|row| row.id == 10).unwrap().gekisou_mission_1 = 2;
    let current = request(&data, "mission", json!({"kind":"score"}));
    let built = handler::build_card_pool(&data, &roster, &current).unwrap();
    if let Some(cap) = diagnostics::maximum_prefix_upper(&built, members, snaps, 2).unwrap() {
        assert!(cap.carrier_upper.is_none() && !cap.keyed);
        assert_eq!(cap.upper, cap.pool_upper);
    }
    drop(built);
    data.master.live_musics.iter_mut().find(|row| row.id == 10).unwrap().gekisou_mission_1 = 1;
    data.master.gekisou_skill_effects[0].id = i64::MAX;
    let current = request(&data, "mission", json!({"kind":"score"}));
    let built = handler::build_card_pool(&data, &roster, &current).unwrap();
    assert!(diagnostics::maximum_prefix_upper(&built, members, snaps, 2).unwrap().is_none());
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn maximum_carrier_signed_factors_keep_complete_terminal_search() {
    use ournotes_search::search::diagnostics;
    let (data, roster) = maximum_carrier_inputs(true);
    assert!(data.master.live_skill_effects.iter().any(|row| row.effect_value < 0));
    let mut current = request(&data, "mission", json!({"kind":"score"}));
    current.constraints.exclude_members = vec![6];
    let reference = oracle(&data, &roster, &current, 6);
    current.strategy = Strategy::BranchAndBound;
    let built = handler::build_card_pool(&data, &roster, &current).unwrap();
    for row in &reference {
        for depth in 1..=5 {
            assert!(diagnostics::maximum_prefix_upper(&built, row.key.0, row.key.1, depth).unwrap().is_none());
        }
    }
    current.k = 3;
    let result = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.optimality, Optimality::Proven);
    assert_eq!(result.results.len(), 3);
    for (actual, expected) in result.results.iter().zip(&reference) {
        verify_row(actual, expected);
    }
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn maximum_id_prefixes_enclose_all_accuracy_completions_and_canonical_top_k() {
    use ournotes_search::search::diagnostics;
    use std::collections::BTreeMap;
    let mut terminal_cases = 0usize;
    let mut context_prefixes = 0usize;
    let mut tightened = 0usize;
    for (members, snaps, leader, duplicate, count) in [(6, 1, 6, false, 30), (6, 1, 3, true, 12), (5, 2, 5, false, 31)]
    {
        let (mut data, roster) = inputs(7301, members, snaps, members, 0);
        for music in &mut data.master.live_musics {
            music.gekisou_mission_1 = 3;
            music.gekisou_mission_2 = 3;
            music.gekisou_mission_3 = 3;
        }
        for member in &mut data.master.member_cards {
            member.performance_power_max = 20_000 - member.id * 1000;
            member.technic_power_max = 20_000 - member.id * 1000;
            member.visual_power_max = 20_000 - member.id * 1000;
            if duplicate && member.id == 6 {
                member.character_id = 1;
            }
        }
        let mut current = request(&data, "mission", json!({"kind":"score"}));
        current.constraints.leader = Some(leader);
        if duplicate {
            current.constraints.include_members = vec![4];
        }
        let reference = oracle(&data, &roster, &current, count);
        current.strategy = Strategy::BranchAndBound;
        let built = handler::build_card_pool(&data, &roster, &current).unwrap();
        let mut prefixes = BTreeMap::new();
        for row in &reference {
            let audit = diagnostics::maximum_prefix_bounds(&built, row.key.0, row.key.1).unwrap();
            assert_eq!(audit["carrierLevels"], 0);
            assert_eq!(audit["idSuffixTables"].as_u64(), Some(members as u64));
            assert!(audit["idSuffixEstimatedBytes"].as_u64().unwrap() <= 16 * 1024 * 1024);
            for bound in audit["prefixes"].as_array().unwrap() {
                let depth = bound["depth"].as_u64().unwrap() as usize;
                let cap = bound["upper"].as_str().unwrap().parse::<i128>().unwrap();
                let pool = bound["poolUpper"].as_str().unwrap().parse::<i128>().unwrap();
                assert!(cap >= i128::from(row.score), "depth={depth} bound={bound} terminal={row:?}");
                assert!(bound["power"].as_i64().unwrap() >= i64::from(row.power));
                assert!(bound["carrierUpper"].is_null());
                if (2..5).contains(&depth) {
                    let id = bound["idUpper"].as_str().unwrap().parse::<i128>().unwrap();
                    assert!(id >= i128::from(row.score));
                    assert!(id <= pool, "an ID suffix only narrows the admitted completion set");
                    tightened += usize::from(id < pool);
                } else {
                    assert!(bound["idUpper"].is_null());
                }
                if depth == 1 {
                    assert!(bound["after"].is_null(), "the leader does not restrict nonleader IDs");
                }
                let key: Vec<_> =
                    [2, 0, 1, 3, 4][..depth].iter().map(|&slot| (row.key.0[slot], row.key.1[slot])).collect();
                let value = (cap, bound["power"].as_i64().unwrap());
                let entry = prefixes.entry(key).or_insert((value, 0usize));
                assert_eq!(entry.0, value, "unplaced identities must not affect the prefix cap");
                entry.1 += 1;
            }
        }
        assert!(prefixes.values().any(|(_, completions)| *completions > 1));
        terminal_cases += reference.len() * 120;
        context_prefixes += prefixes.len();
        for cache in [0, 32] {
            current.k = 3;
            current.limits.cache_entries = cache;
            let result = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(result.completion, Completion::Complete);
            assert_eq!(result.optimality, Optimality::Proven);
            assert_eq!(result.results.len(), 3);
            for (actual, expected) in result.results.iter().zip(&reference) {
                verify_row(actual, expected);
            }
        }
    }
    assert_eq!(terminal_cases, 8760);
    assert!(tightened > 0);
    eprintln!(
        "ID-prefix audit: {terminal_cases} terminal order cases, {context_prefixes} context-prefix cases, {tightened} tightened witnesses"
    );
}

#[test]
fn maximum_dense_stream_caps_match_every_terminal_order() {
    use ournotes_search::types::{Metric, PlayPolicy};
    let mut terminals = 0;
    for effects in [0, 1] {
        let (data, roster) = inputs(673, 5, 1, 5, effects);
        let mut current = request(&data, "mission", json!({"kind":"score"}));
        let Execution::Live { play: PlayPolicy::Stream { stream }, .. } = &mut current.execution else {
            panic!("explicit judgement stream");
        };
        let original = stream.frames.clone();
        let first = *original.first().unwrap();
        let last = *original.last().unwrap();
        stream.frames = (first..=last).collect();
        stream.delta_times = Some(vec![0.001; stream.frames.len()]);
        for judged in &mut stream.judged {
            judged[0] = original[judged[0] as usize] - first;
        }
        assert!(stream.frames.len() > 1000);
        let reference = oracle(&data, &roster, &current, 6);
        terminals += 6 * 120;
        for cache in [0, 32] {
            current.strategy = Strategy::BranchAndBound;
            current.k = 3;
            current.limits.cache_entries = cache;
            let result = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(result.completion, Completion::Complete);
            assert_eq!(result.optimality, Optimality::Proven);
            for (actual, expected) in result.results.iter().zip(&reference) {
                verify_row(actual, expected);
            }
            assert_eq!(result.results.len(), 3);
        }
        current.metric = Metric::CappedScore { threshold: reference[2].score };
        let reference = oracle(&data, &roster, &current, 6);
        terminals += 6 * 120;
        for cache in [0, 32] {
            current.limits.cache_entries = cache;
            let result = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(result.completion, Completion::Complete);
            assert_eq!(result.optimality, Optimality::Proven);
            for (actual, expected) in result.results.iter().zip(&reference) {
                verify_row(actual, expected);
            }
            assert_eq!(result.results.len(), 3);
        }
    }
    assert_eq!(terminals, 2880);
}
