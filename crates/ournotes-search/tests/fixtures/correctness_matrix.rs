//! Synthetic physical-domain and objective matrix for the shared fixed-deck model.

use super::common::set_column;
use super::{EVENT_ID, SCORE_ID, data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{
    auxiliary, engine, handler,
    search::Completion,
    types::{Execution, Metric, PlayPolicy, RecommendationRequest, RecommendedDeck, Strategy},
};
use ournotes_sim::{
    cards::Roster,
    data::DeckData,
    live::model::{JudgementStream, JustRule},
    pool::Pool,
    scenario::Scenario,
};
use serde_json::{Value, json};
use std::{cmp::Ordering, collections::BTreeSet};

type Key = ([i64; 5], [Option<i64>; 5]);

/// Enumerate member combinations, every eligible leader, and injective optional Snap bindings.
fn domain(pool: &Pool<'_>, request: &RecommendationRequest) -> Vec<Key> {
    let constraints = &request.constraints;
    let mut members: Vec<_> = pool
        .members
        .iter()
        .filter(|member| !constraints.exclude_members.contains(&member.id))
        .map(|member| (member.id, member.character_id))
        .collect();
    members.sort_unstable();
    let mut snaps: Vec<_> = pool
        .snaps
        .iter()
        .filter(|snap| !constraints.no_snaps && !constraints.exclude_snaps.contains(&snap.id))
        .map(|snap| snap.id)
        .collect();
    snaps.sort_unstable();

    fn combinations(rows: &[(i64, i64)], start: usize, chosen: &mut Vec<(i64, i64)>, out: &mut Vec<[i64; 5]>) {
        if chosen.len() == 5 {
            out.push(std::array::from_fn(|index| chosen[index].0));
            return;
        }
        for index in start..rows.len() {
            let member = rows[index];
            if chosen.iter().all(|row| row.1 != member.1) {
                chosen.push(member);
                combinations(rows, index + 1, chosen, out);
                chosen.pop();
            }
        }
    }

    fn bindings(members: [i64; 5], snaps: &[i64], row: &mut [Option<i64>; 5], slot: usize, out: &mut Vec<Key>) {
        if slot == 5 {
            out.push((members, *row));
            return;
        }
        row[slot] = None;
        bindings(members, snaps, row, slot + 1, out);
        for &snap in snaps {
            if row[..slot].iter().all(|&placed| placed != Some(snap)) {
                row[slot] = Some(snap);
                bindings(members, snaps, row, slot + 1, out);
            }
        }
        row[slot] = None;
    }

    let mut sets = Vec::new();
    combinations(&members, 0, &mut Vec::new(), &mut sets);
    let mut out = Vec::new();
    for set in sets {
        if constraints.include_members.iter().any(|id| !set.contains(id)) {
            continue;
        }
        for leader in set {
            if constraints.leader.is_some_and(|id| id != leader) {
                continue;
            }
            let others: Vec<_> = set.iter().copied().filter(|&id| id != leader).collect();
            let row = [others[0], others[1], leader, others[2], others[3]];
            bindings(row, &snaps, &mut [None; 5], 0, &mut out);
        }
    }
    assert_eq!(out.iter().collect::<BTreeSet<_>>().len(), out.len());
    out
}

fn key(row: &RecommendedDeck) -> Key {
    (row.members, row.snaps)
}

fn compare(a: &RecommendedDeck, b: &RecommendedDeck) -> Ordering {
    let a_payoff = a.expected_payoff.as_ref().expect("exact payoff in the deterministic matrix");
    let b_payoff = b.expected_payoff.as_ref().expect("exact payoff in the deterministic matrix");
    let left = a_payoff.numerator.parse::<i128>().unwrap() * b_payoff.denominator.parse::<i128>().unwrap();
    let right = b_payoff.numerator.parse::<i128>().unwrap() * a_payoff.denominator.parse::<i128>().unwrap();
    right.cmp(&left).then_with(|| b.power.cmp(&a.power)).then_with(|| key(a).cmp(&key(b)))
}

fn fixture(members: i64, snaps: i64, characters: i64, luck: bool) -> (DeckData, Roster) {
    let mut master = synthetic_master(members, snaps, characters);
    for table in ["MasterLiveMusic", "MasterArenaMusic"] {
        set_column(&mut master, table, &mut |row| {
            row["_gekisouMission1"] = json!(if luck { 2 } else { 1 });
            row["_gekisouMission2"] = json!(3);
            row["_gekisouMission3"] = json!(1);
        });
    }
    set_column(&mut master, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(5);
        row["_supportSkillId02"] = json!(6);
    });
    set_column(&mut master, "MasterLiveGekisouRankingScoreBonus", &mut |row| {
        row["_scoreBonusPercent"] = json!(match row["_count"].as_i64().unwrap() {
            1 => 250,
            2 => 125,
            _ => 0,
        });
    });
    set_column(&mut master, "MasterLiveSkillEffect", &mut |row| {
        row["_activationTimeSecond"] = json!(0.24);
    });
    if luck {
        set_column(&mut master, "MasterLiveSettings", &mut |row| {
            if matches!(row["_key"].as_str(), Some("gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush")) {
                row["_value"] = json!("10");
            }
        });
    }
    let mut document = data_document(&master, members, snaps, characters);
    document["charts"][0]["skillEvents"]["timeMs"] = json!([160, 500, 750, 1000, 1250]);
    if luck {
        document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    }
    let mut data = DeckData::from_json(&document.to_string()).unwrap();
    if luck {
        for effect in &mut data.master.leader_skill_effects {
            effect.effect_value = 0;
        }
    }
    let roster = Roster::from_json(&roster_document(members, snaps, characters).to_string()).unwrap();
    (data, roster)
}

fn request(data: &DeckData, scene: &str, stream: bool, metric: Value) -> RecommendationRequest {
    let gekisou = scene != "free";
    let mut wire = joint_request_json(scene, gekisou, metric);
    if scene == "arena" {
        wire["scenario"]["musicId"] = json!(80);
    }
    if matches!(scene, "battle" | "arena") {
        wire["networkConfirmations"] = json!([
            {"frame":0,"range":0,"rank":1,"percent":250},
            {"frame":49,"range":1,"rank":1,"percent":125},
            {"frame":71,"range":2,"rank":1,"percent":0}
        ]);
        wire["context"]["eventPayoff"]["multiplayerScorePolicy"] =
            json!({"kind":"fixedOthersAverage","players":3,"score":150000});
    }
    let mut request: RecommendationRequest = serde_json::from_value(wire).unwrap();
    if stream {
        let chart = data.chart(SCORE_ID).unwrap();
        let mut played = if gekisou {
            let chart_data = data.data_chart(SCORE_ID).unwrap();
            let scenario = match scene {
                "battle" => Scenario::Battle(10),
                "arena" => Scenario::Arena(80),
                _ => Scenario::Mission(10),
            };
            let setup = scenario.resolve(&data.master).unwrap().gekisou_setup(&chart_data.fevers);
            let rule = JustRule::new(&data.master, &setup).unwrap();
            JudgementStream::theoretical_best_gekisou(&chart, &chart_data.judgement_types, &rule).unwrap()
        } else {
            JudgementStream::theoretical_best(&chart)
        };
        for (index, note) in played.judged.iter_mut().enumerate() {
            note[2] = [1, 2, 3, 4, 5, 6][index % 6].min(note[2]);
        }
        request.execution =
            Execution::Live { score_id: SCORE_ID, gekisou, play: PlayPolicy::Stream { stream: played } };
    }
    request
}

fn exact_oracle(
    data: &DeckData,
    roster: &Roster,
    request: &RecommendationRequest,
    candidates: usize,
) -> Vec<RecommendedDeck> {
    let pool = Pool::new(&data.master, roster).unwrap();
    let keys = domain(&pool, request);
    assert_eq!(keys.len(), candidates);
    let mut fixed_request = request.clone();
    fixed_request.strategy = Strategy::Exhaustive;
    let built = handler::build_card_pool(data, roster, &fixed_request).unwrap();
    let mut rows = Vec::with_capacity(keys.len());
    for (members, snaps) in keys {
        let result = auxiliary::evaluate_built(&built, members, snaps).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.results.len(), 1);
        let row = result.results.into_iter().next().unwrap();
        assert_eq!(key(&row), (members, snaps));
        assert_eq!(row.order_outcomes.len(), 120);
        assert_eq!(row.order_outcomes.iter().map(|order| order.0).collect::<BTreeSet<_>>().len(), 120);
        let payoff = row.expected_payoff.as_ref().unwrap();
        let numerator = row.order_outcomes.iter().map(|order| order.2).sum::<i128>();
        assert_eq!(payoff.denominator, "120");
        assert_eq!(payoff.numerator, numerator.to_string());
        for &(_, score, actual) in &row.order_outcomes {
            let expected = match request.metric {
                Metric::Score => Some(i128::from(score)),
                Metric::ScoreAtLeast { threshold } => Some(i128::from(score >= threshold)),
                Metric::CappedScore { threshold } => Some(i128::from(score.min(threshold))),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!(actual, expected);
            }
        }
        rows.push(row);
    }
    rows.sort_by(compare);
    rows
}

fn verify_searches(
    data: &DeckData,
    roster: &Roster,
    request: &RecommendationRequest,
    oracle: &[RecommendedDeck],
    name: &str,
) {
    let mut current = request.clone();
    current.strategy = Strategy::Exhaustive;
    current.k = 100;
    let all = engine::recommend(data, roster, &current).unwrap();
    assert_eq!(all.completion, Completion::Complete, "{name}");
    assert_eq!(all.results, oracle, "{name}: exhaustive physical domain");
    current.strategy = Strategy::BranchAndBound;
    for cache in [0, 64] {
        current.limits.cache_entries = cache;
        for k in [1, 3, 5, 100] {
            current.k = k;
            let result = engine::recommend(data, roster, &current).unwrap();
            assert_eq!(result.completion, Completion::Complete, "{name}: k={k} cache={cache}");
            assert_eq!(result.results, oracle[..oracle.len().min(k)], "{name}: k={k} cache={cache}");
            if !oracle.is_empty() {
                assert!(
                    result.telemetry.environment.bounds.compiled,
                    "{name}: {:?}",
                    result.telemetry.environment.bounds.fallback
                );
            }
        }
    }
    #[cfg(feature = "search-diagnostics")]
    {
        use ournotes_search::search::diagnostics;
        let built = handler::build_card_pool(data, roster, &current).unwrap();
        let mut scratch = diagnostics::PrefixAuditScratch::default();
        for row in oracle {
            let numerator = row.expected_payoff.as_ref().unwrap().numerator.parse::<i128>().unwrap();
            for depth in 1..=5 {
                let (payoff, power) = diagnostics::prefix_upper(&built, row.members, row.snaps, depth, &mut scratch)
                    .unwrap()
                    .expect("joint prefix cap for the deterministic matrix");
                assert!(payoff >= numerator && power >= i64::from(row.power), "{name}: depth={depth}");
            }
            for team in [false, true] {
                for depth in usize::from(!team)..=5 {
                    let (payoff, power) = diagnostics::split_prefix_upper(&built, row.members, row.snaps, depth, team)
                        .unwrap()
                        .expect("composition and team cap for the deterministic matrix");
                    assert!(payoff >= numerator && power >= i64::from(row.power), "{name}: team={team} depth={depth}");
                }
            }
            let orders = diagnostics::audit_order_caps(&built, row.members, row.snaps).unwrap();
            assert_eq!(orders["orders"], 120, "{name}");
            assert_eq!(orders["violations"], 0, "{name}: {orders}");
        }
    }
}

#[test]
fn deterministic_scene_objective_matrix_matches_the_independent_domain() {
    let (data, roster) = fixture(5, 1, 5, false);
    let mut cases = 0;
    for scene in ["free", "mission", "battle", "arena"] {
        for stream in [false, true] {
            let mut metrics = vec![
                json!({"kind":"score"}),
                json!({"kind":"scoreAtLeast","threshold":450000}),
                json!({"kind":"cappedScore","threshold":450000}),
                json!({"kind":"clientEventPoints","eventId":EVENT_ID}),
            ];
            if stream {
                metrics.push(json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":800}));
            }
            for metric in metrics {
                let request = request(&data, scene, stream, metric);
                let oracle = exact_oracle(&data, &roster, &request, 6);
                let name = format!("{scene} stream={stream} {:?}", request.metric);
                verify_searches(&data, &roster, &request, &oracle, &name);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 36);
    eprintln!("scene/objective matrix: {cases} cases, 216 physical candidates, 25920 oracle order outcomes");
}

#[test]
fn constraints_and_ties_preserve_the_independent_canonical_topk() {
    let (mut data, roster) = fixture(6, 2, 5, false);
    for effect in &mut data.master.leader_skill_effects {
        effect.effect_value = 0;
    }
    let base = request(&data, "mission", false, json!({"kind":"scoreAtLeast","threshold":1}));
    let cases = [
        ("optional-snaps", json!({"leader":3}), 62),
        ("required-member", json!({"leader":3,"includeMembers":[1]}), 31),
        ("excluded-member", json!({"leader":3,"excludeMembers":[6]}), 31),
        ("excluded-snap", json!({"leader":3,"excludeSnaps":[2]}), 12),
        ("all-leaders", json!({"noSnaps":true}), 10),
        ("same-character-requirements", json!({"leader":3,"includeMembers":[1,6]}), 0),
        ("alternative-leader", json!({"leader":6,"noSnaps":true}), 1),
        ("canonical-leader-ties", json!({"includeMembers":[1],"excludeMembers":[6],"noSnaps":true}), 5),
    ];
    let mut candidates = 0;
    for (name, constraints, expected) in cases {
        let mut request = base.clone();
        request.constraints = serde_json::from_value(constraints).unwrap();
        let oracle = exact_oracle(&data, &roster, &request, expected);
        if name == "canonical-leader-ties" {
            assert!(oracle.windows(2).all(|rows| rows[0].power == rows[1].power && key(&rows[0]) < key(&rows[1])));
        }
        verify_searches(&data, &roster, &request, &oracle, name);
        candidates += expected;
    }
    assert_eq!(candidates, 152);
    eprintln!("constraint matrix: 8 cases, {candidates} physical candidates, 18240 oracle order outcomes");
}

#[test]
fn nominal_luck_objectives_certify_equivalent_leader_programs() {
    let (data, roster) = fixture(5, 0, 5, true);
    let pool = Pool::new(&data.master, &roster).unwrap();
    let mut cases = 0;
    for stream in [false, true] {
        let mut metrics = vec![
            (json!({"kind":"score"}), None),
            (json!({"kind":"scoreAtLeast","threshold":1}), Some(1)),
            (json!({"kind":"scoreAtLeast","threshold":2147483647}), Some(0)),
            (json!({"kind":"cappedScore","threshold":1}), Some(1)),
            (json!({"kind":"clientEventPoints","eventId":EVENT_ID}), None),
        ];
        if stream {
            metrics.push((json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":1000}), Some(0)));
        }
        for (metric, certain) in metrics {
            let mut current = request(&data, "mission", stream, metric);
            current.constraints.leader = None;
            current.constraints.no_snaps = true;
            let mut keys = domain(&pool, &current);
            keys.sort_unstable();
            assert_eq!(keys.len(), 5);
            let mut fixed_request = current.clone();
            fixed_request.strategy = Strategy::Exhaustive;
            let built = handler::build_card_pool(&data, &roster, &fixed_request).unwrap();
            let mut fixed = Vec::new();
            for &(members, snaps) in &keys {
                let evaluated = auxiliary::evaluate_built(&built, members, snaps).unwrap();
                assert_eq!(evaluated.completion, Completion::Complete);
                assert_eq!(evaluated.results.len(), 1);
                let row = evaluated.results.into_iter().next().unwrap();
                assert_eq!(key(&row), (members, snaps));
                assert!(row.score_interval.is_some() && row.payoff_interval.is_some());
                fixed.push(row);
            }
            // Equal leader factors and the same performer multiset give the same law over all 120 orders.
            assert!(fixed.windows(2).all(|rows| rows[0].power == rows[1].power));
            for strategy in [Strategy::Exhaustive, Strategy::BranchAndBound] {
                current.strategy = strategy;
                for cache in [0, 64] {
                    current.limits.cache_entries = cache;
                    for k in [1, 3, 5, 100] {
                        current.k = k;
                        let result = engine::recommend(&data, &roster, &current).unwrap();
                        assert_eq!(
                            result.completion,
                            Completion::Complete,
                            "stream={stream} {:?} k={k} cache={cache}",
                            current.metric
                        );
                        assert_eq!(result.results.iter().map(key).collect::<Vec<_>>(), keys[..keys.len().min(k)]);
                        for row in &result.results {
                            assert_eq!(row.rank_certified, Some(true));
                            assert!(row.score_interval.is_some() && row.payoff_interval.is_some());
                            if let Some(value) = certain {
                                let payoff = row.expected_payoff.as_ref().expect("certified constant payoff");
                                assert_eq!(
                                    payoff.numerator.parse::<i128>().unwrap(),
                                    value * payoff.denominator.parse::<i128>().unwrap()
                                );
                            }
                        }
                    }
                }
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 11);
    eprintln!(
        "nominal LUCK matrix: {cases} cases, 55 fixed physical candidates, 5 equivalent leader programs per case"
    );
}
