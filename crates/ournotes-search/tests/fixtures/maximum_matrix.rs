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
        for scene in ["free", "challenge", "skip", "challenge-skip"] {
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
    assert_eq!(cases, 48);
    eprintln!("maximum scene matrix: {cases} cases, 288 physical candidates");
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
            let mut request = request(&data, "free", json!({"kind":"score"}));
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
