//! Exhausted LUCK frontiers over synthetic domains with distinct physical team identities.
//! These inputs declare independent nominal lottery tables.
use super::common::set_column;
use super::{data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Metric, RecommendationOutcome, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

/// Keep the residue transport case outside both earlier equality certificates without changing its law.
/// The substitute retains the same Rush score row under a distinct GK source and adds a fair, zero-valued
/// range-start gauge command. At range start the native gauge is zero; either Bernoulli outcome therefore
/// leaves exactly the same controller state. The strict pathwise certificate deliberately declines its
/// nontrivial action chance, while exact nominal refinement must recover the original rational mean.
pub(super) fn distinguish_neutral_residue_source(document: &mut serde_json::Value) {
    fn column(table: &serde_json::Value, name: &str) -> usize {
        table["columns"].as_array().unwrap().iter().position(|value| value == name).unwrap()
    }
    fn copy_row(table: &serde_json::Value, id: i64) -> serde_json::Value {
        let key = column(table, "_id");
        table["rows"].as_array().unwrap().iter().find(|row| row[key] == id).unwrap().clone()
    }
    fn append(table: &mut serde_json::Value, mut row: serde_json::Value, changes: &[(&str, serde_json::Value)]) {
        for (name, value) in changes {
            row[column(table, name)] = value.clone();
        }
        table["rows"].as_array_mut().unwrap().push(row);
    }
    let table = &mut document["master"]["MasterMemberCard"];
    let (id, skill) = (column(table, "_id"), column(table, "_gekisouSkillID"));
    let substitute = table["rows"].as_array_mut().unwrap().iter_mut().find(|row| row[id] == 6).unwrap();
    assert_eq!(substitute[skill], 103);
    substitute[skill] = json!(104);

    let table = &mut document["master"]["MasterGekisouSkill"];
    let row = copy_row(table, 103);
    append(table, row, &[("_id", json!(104))]);
    let table = &mut document["master"]["MasterGekisouSkillEffect"];
    let score = copy_row(table, 103);
    let gauge = copy_row(table, 102);
    append(table, score, &[("_id", json!(104)), ("_gekisouSkillID", json!(104))]);
    append(
        table,
        gauge,
        &[
            ("_id", json!(105)),
            ("_gekisouSkillID", json!(104)),
            ("_effectValue", json!(0)),
            ("_skillConditionGroup", json!(1904)),
        ],
    );
    let table = &mut document["master"]["MasterSkillCondition"];
    let row = copy_row(table, 904);
    append(table, row, &[("_id", json!(1904)), ("_conditionValues", json!([50]))]);
    let table = &mut document["master"]["MasterSkillConditionSet"];
    let row = copy_row(table, 904);
    append(table, row, &[("_id", json!(1904)), ("_group", json!(1904)), ("_conditionIds", json!([1904]))]);
}

fn inputs(threshold: i32, k: usize, cache_entries: usize) -> (DeckData, Roster, RecommendationRequest) {
    let mut synth = synthetic_master(5, 2, 5);
    set_column(&mut synth, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(2);
        row["_gekisouMission2"] = json!(3);
        row["_gekisouMission3"] = json!(1);
    });
    set_column(&mut synth, "MasterLiveSettings", &mut |row| {
        if matches!(row["_key"].as_str(), Some("gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush")) {
            row["_value"] = json!("10");
        }
    });
    let mut document = data_document(&synth, 5, 2, 5);
    document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    let mut request = joint_request("mission", true, json!({"kind":"scoreAtLeast","threshold":threshold}));
    request.k = k;
    request.strategy = Strategy::Exhaustive;
    request.limits.cache_entries = cache_entries;
    (data, roster, request)
}

/// A complete mapped terminal fold can resolve this frontier before the complete RNG-law fallback runs.
/// Require installed certificates and actual provider work, keeping their different authorities/counters
/// separate. Cache hits can install multiple labelled orders from one completed native computation.
fn assert_completed_payoff_refinement(result: &RecommendationOutcome) {
    let counters = &result.telemetry.lottery_refinement;
    assert!(
        counters.terminal_installed_orders > 0 || counters.installed_orders > 0,
        "a complete payoff provider must add information beyond the moment bounds: {counters:#?}"
    );
    assert_eq!(counters.terminal_completed_orders, counters.terminal_installed_orders);
    assert_eq!(
        counters.terminal_attempted_orders,
        counters.terminal_completed_orders + counters.terminal_declined_orders
    );
    assert_eq!(counters.completed_orders, counters.installed_orders);
    assert_eq!(counters.attempted_orders, counters.completed_orders + counters.declined_orders);
    assert_eq!(counters.arithmetic_declines, 0);
    if counters.terminal_installed_orders > 0 {
        assert!(counters.terminal_timeline_paths > 0);
        assert!(counters.terminal_timeline_transitions > 0);
        assert!(counters.terminal_score_fold_queries > 0, "the mapped payoff must use native score folds");
    }
    if counters.installed_orders > 0 {
        assert!(counters.terminal_paths > 0, "complete RNG laws retain their independent native terminal evidence");
        assert!(counters.replay_runs > 0);
    } else if counters.attempted_orders == 0 {
        assert_eq!(counters.terminal_paths, 0, "a mapped payoff enclosure is not a complete native RNG law");
    }
    for deck in &result.results {
        let probability = deck.payoff_interval.as_ref().unwrap();
        assert!(probability.lower_f64() >= 0.0 && probability.upper_f64() <= 1.0);
        // Ranking completion does not fill in an exact expectation or per-order score from a midpoint.
        if deck.expected_payoff.is_none() {
            assert!(probability.lower_f64() < probability.upper_f64());
        }
        assert!(deck.order_outcomes.is_empty(), "nominal lottery payoffs are not deterministic order outcomes");
        assert!(deck.score_summary.is_none(), "a mapped payoff proof does not publish a raw score distribution");
    }
}

/// Independent complete native lottery laws, with all 120 original performer labels. This oracle does
/// not use the terminal payoff enclosure or infer a probability from a score mean.
fn native_threshold_probability(
    data: &DeckData,
    roster: &Roster,
    request: &RecommendationRequest,
    winner: &ournotes_search::types::RecommendedDeck,
) -> (i128, i128) {
    use ournotes_search::{
        search::{GekisouObjective, Objective, PlayInput, SeedSet, expectation, uniform},
        types::{Execution, PlayPolicy, Scene},
    };
    use ournotes_sim::live::{
        full::{LuckExactBudget, LuckExactSession},
        model::{JudgementStream, JustRule},
    };
    let Execution::Live { score_id, gekisou: true, play } = &request.execution else {
        panic!("this oracle requires the declared live LUCK fixture");
    };
    let Metric::ScoreAtLeast { threshold } = request.metric else {
        panic!("this oracle requires the threshold payoff");
    };
    let chart = data.chart(*score_id).unwrap();
    let chart_data = data.data_chart(*score_id).unwrap();
    let Some(Scene::Mission { music_id }) = request.scenario.as_ref() else {
        panic!("this oracle requires the declared Mission fixture");
    };
    let context = request
        .context
        .as_ref()
        .unwrap()
        .resolve(
            &data.master,
            ournotes_sim::scenario::Scenario::Mission(*music_id),
            Some(*score_id),
            &chart_data.fevers,
        )
        .unwrap();
    let stream = match play {
        PlayPolicy::Stream { stream } => stream.clone(),
        PlayPolicy::TheoreticalBest => JudgementStream::theoretical_best_gekisou(
            &chart,
            &chart_data.judgement_types,
            &JustRule::new(&data.master, &context.gekisou).unwrap(),
        )
        .unwrap(),
        PlayPolicy::Accuracy(_) => panic!("the fixture does not use an accuracy policy"),
    };
    let objective = Objective::LiveScore {
        score_id: *score_id,
        chart,
        play: PlayInput::Stream { stream, judgement_types: chart_data.judgement_types.clone() },
        event: false,
        exclude_snap_skills: false,
        gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: chart_data.fevers.clone() }),
    }
    .in_scenario(context.clone());
    let pool = context.pool(&data.master, roster).unwrap();
    let deck = pool.deck(winner.members, winner.snaps, [0, 1, 2, 3, 4]).unwrap();
    let physical = expectation::PhysicalDeck { members: deck.members, snaps: deck.snaps };
    let input = expectation::context(&pool, &physical, &objective).unwrap();
    let mut session = LuckExactSession::new(
        &data.master,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        None,
        0,
    )
    .unwrap();
    fn add(a: (i128, i128), b: (i128, i128)) -> (i128, i128) {
        fn gcd(mut a: i128, mut b: i128) -> i128 {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        }
        let common = gcd(a.1, b.1);
        let numerator =
            a.0.checked_mul(b.1 / common).unwrap().checked_add(b.0.checked_mul(a.1 / common).unwrap()).unwrap();
        let denominator = a.1.checked_mul(b.1 / common).unwrap();
        let reduction = gcd(numerator.abs(), denominator);
        (numerator / reduction, denominator / reduction)
    }
    let orders = uniform::all_orders();
    assert_eq!(orders.len(), 120);
    let mut probability = (0, 1);
    for order in orders {
        let performers = order.map(|slot| input.performers[slot].clone());
        let attempt = session.law(&performers, &mut LuckExactBudget::default(), || false).unwrap();
        let law = attempt.law.expect("the complete finite native lottery tree must finish");
        let mut mass = (0, 1);
        for atom in law.atoms() {
            let p = (i128::try_from(atom.mass.numerator).unwrap(), i128::try_from(atom.mass.denominator).unwrap());
            mass = add(mass, p);
            if atom.score >= threshold {
                probability = add(probability, (p.0, p.1.checked_mul(120).unwrap()));
            }
        }
        assert_eq!(mass, (1, 1), "each original label has its complete nominal mass");
    }
    assert!(probability.0 > 0 && probability.0 < probability.1, "the witness has a nonconstant payoff");
    probability
}

fn assert_native_probability_enclosed(winner: &ournotes_search::types::RecommendedDeck, native: (i128, i128)) {
    let parse = |value: &ournotes_search::types::Fraction| {
        (value.numerator.parse::<i128>().unwrap(), value.denominator.parse::<i128>().unwrap())
    };
    let compare = |a: (i128, i128), b: (i128, i128)| a.0.checked_mul(b.1).unwrap().cmp(&b.0.checked_mul(a.1).unwrap());
    let interval = winner.payoff_interval.as_ref().unwrap();
    assert!(compare(parse(&interval.lower), native).is_le(), "{interval:?} must contain native {native:?}");
    assert!(compare(parse(&interval.upper), native).is_ge(), "{interval:?} must contain native {native:?}");
    if let Some(exact) = &winner.expected_payoff {
        assert!(compare(parse(exact), native).is_eq());
    }
}

#[test]
fn probability_range_settles_the_k1_and_k12_certain_event_witnesses() {
    let powers = [127480, 125551, 125292, 125100, 124371, 124190, 124054, 123871, 123296, 122261, 122183, 122125];
    for (k, cache) in [(1, 0), (12, 64)] {
        let (data, roster, request) = inputs(545_749, k, cache);
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.telemetry.leaves.visited, 31);
        assert_eq!(result.results.iter().map(|deck| deck.power).collect::<Vec<_>>(), powers[..k]);
        assert_eq!(
            result.telemetry.lottery_refinement.attempted_orders, 0,
            "the closed probability range already proves these power ties"
        );
        for deck in &result.results {
            assert_eq!(deck.rank_certified, Some(true));
            let payoff = deck.expected_payoff.as_ref().expect("the threshold is reached on every path");
            assert_eq!(payoff.numerator, payoff.denominator);
            let interval = deck.payoff_interval.as_ref().unwrap();
            assert!(interval.lower_f64() >= 0.0);
            assert_eq!(interval.upper_f64(), 1.0);
        }
    }
}

#[test]
fn complete_payoff_refinement_settles_the_remaining_threshold_witness_with_and_without_a_cache() {
    let mut uncached = None;
    let mut native_probability = None;
    for cache_entries in [0, 64] {
        let (data, roster, request) = inputs(610_000, 1, cache_entries);
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.optimality, ournotes_search::types::Optimality::Proven);
        assert_eq!(result.telemetry.leaves.visited, 31);
        assert_eq!(result.results.len(), 1);
        let winner = &result.results[0];
        assert_eq!(winner.members, [1, 2, 3, 4, 5]);
        assert_eq!(winner.snaps, [Some(1), Some(2), None, None, None]);
        assert_eq!(winner.power, 127_480);
        assert_eq!(winner.rank_certified, Some(true));
        assert_completed_payoff_refinement(&result);
        let counters = &result.telemetry.lottery_refinement;
        assert_eq!(counters.declined_orders, 0);
        if cache_entries == 0 && counters.installed_orders > 0 {
            assert!(
                counters.terminal_paths > counters.completed_orders,
                "the uncached exact-law fallback remains stochastic"
            );
        }
        if counters.installed_orders == 0 {
            assert!(
                winner.expected_score.is_none(),
                "mapped payoff folds cannot make this stochastic raw score mean exact"
            );
        }
        let probability = winner.payoff_interval.as_ref().unwrap();
        // Completion can follow from tighter competing upper bounds without raising this winner's lower
        // endpoint. Require the published enclosure to contain the independent complete native payoff.
        let native =
            *native_probability.get_or_insert_with(|| native_threshold_probability(&data, &roster, &request, winner));
        assert_native_probability_enclosed(winner, native);
        if let Some((lower, upper)) = uncached {
            assert!(probability.lower_f64() <= upper && lower <= probability.upper_f64());
        } else {
            uncached = Some((probability.lower_f64(), probability.upper_f64()));
        }
    }
}

#[test]
fn a_long_frame_schedule_loads_boundary_orders_and_certifies_the_threshold_ranking() {
    use ournotes_search::types::{Execution, PlayPolicy};
    use ournotes_sim::live::model::JudgementStream;

    let mut native_probability = None;
    for cache_entries in [0, 64] {
        let (data, roster, mut request) = inputs(610_000, 1, cache_entries);
        let mut stream = JudgementStream::theoretical_best(&data.chart(super::SCORE_ID).unwrap());
        let first = stream.frames.len() as i32;
        stream.frames.extend((first..=600).map(|frame| frame * 1000 / 60));
        request.execution =
            Execution::Live { score_id: super::SCORE_ID, gekisou: true, play: PlayPolicy::Stream { stream } };
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(
            result.completion,
            Completion::Complete,
            "{:#?}; results: {:#?}",
            result.telemetry.lottery_refinement,
            result.results
        );
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.optimality, ournotes_search::types::Optimality::Proven);
        assert_eq!(result.telemetry.leaves.visited, 31);
        let winner = &result.results[0];
        assert_eq!(winner.members, [1, 2, 3, 4, 5]);
        assert_eq!(winner.snaps, [Some(1), Some(2), None, None, None]);
        assert_eq!(winner.power, 127_480);
        assert_eq!(winner.rank_certified, Some(true));
        assert_completed_payoff_refinement(&result);
        assert_eq!(result.telemetry.lottery_refinement.declined_orders, 0);
        let native =
            *native_probability.get_or_insert_with(|| native_threshold_probability(&data, &roster, &request, winner));
        assert_native_probability_enclosed(winner, native);
    }
}

#[test]
fn long_stream_refinement_materializes_the_boundary_candidate() {
    let (data, roster, _) = inputs(610_000, 1, 0);
    let mut wire = super::joint_request_json("mission", true, json!({"kind":"scoreAtLeast","threshold":610_000}));
    wire["k"] = json!(1);
    wire["strategy"] = json!({"kind":"exhaustive"});
    wire["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":(0..=2000).step_by(20).collect::<Vec<_>>(),
        "judged":(1..=12).map(|id| json!([id*5,id,5,id*100])).collect::<Vec<_>>()
    }});
    for cache_entries in [0, 64] {
        wire["limits"]["cacheEntries"] = json!(cache_entries);
        wire["execution"]["play"]["stream"]["frames"] = json!((0..=2000).step_by(20).collect::<Vec<_>>());
        let short = engine::recommend(&data, &roster, &serde_json::from_value(wire.clone()).unwrap()).unwrap();
        assert_eq!(short.completion, Completion::Complete);
        assert_eq!(short.optimality, ournotes_search::types::Optimality::Proven);
        assert_eq!(short.telemetry.leaves.visited, 31);
        assert_eq!(short.results.len(), 1);
        assert_completed_payoff_refinement(&short);
        wire["execution"]["play"]["stream"]["frames"] = json!((0..=10400).step_by(20).collect::<Vec<_>>());
        let long = engine::recommend(&data, &roster, &serde_json::from_value(wire.clone()).unwrap()).unwrap();
        assert!(matches!(long.completion, Completion::Complete | Completion::RefinementRequired));
        assert_eq!(long.telemetry.leaves.visited, 31);
        assert_completed_payoff_refinement(&long);
        let counters = &long.telemetry.lottery_refinement;
        if long.completion == Completion::RefinementRequired {
            assert_eq!(long.optimality, ournotes_search::types::Optimality::Unproven);
            assert!(counters.declined_orders > 0 || counters.terminal_declined_orders > 0);
            assert!(counters.budget_exhausted);
            let budget = ournotes_sim::live::full::LuckExactBudget::default();
            assert!(counters.frames == budget.remaining_frames || counters.replay_runs == budget.remaining_runs);
        } else {
            assert_eq!(long.optimality, ournotes_search::types::Optimality::Proven);
            assert_eq!(long.results.len(), 1);
            assert!(long.results.iter().all(|row| row.rank_certified == Some(true)));
            assert_eq!(
                (long.results[0].members, long.results[0].snaps),
                (short.results[0].members, short.results[0].snaps)
            );
        }
        // The finite shared work allowance preserves the ambiguous frontier and the true winning candidate.
        let expected = &short.results[0];
        assert_eq!(expected.rank_certified, Some(true));
        let actual = long
            .results
            .iter()
            .find(|row| row.members == expected.members && row.snaps == expected.snaps)
            .expect("the certified short-schedule winner remains represented");
        assert_eq!(actual.power, expected.power);
        let a = actual.payoff_interval.as_ref().unwrap();
        let b = expected.payoff_interval.as_ref().unwrap();
        assert!(a.lower_f64() <= b.upper_f64() && b.lower_f64() <= a.upper_f64());
    }
}

#[test]
fn certified_seed_budget_preserves_the_complete_canonical_ranking() {
    for (k, cache) in [(1, 0), (3, 64), (31, 0)] {
        let (data, roster, _) = inputs(610_000, k, cache);
        let mut request = joint_request("mission", true, json!({"kind":"score"}));
        request.k = k;
        request.limits.cache_entries = cache;
        request.strategy = Strategy::Exhaustive;
        let reference = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(reference.completion, Completion::Complete);
        request.strategy = Strategy::BranchAndBound;
        let actual = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(actual.completion, Completion::Complete);
        assert_eq!(actual.results.len(), reference.results.len());
        for (a, b) in actual.results.iter().zip(&reference.results) {
            assert_eq!((a.members, a.snaps, a.power), (b.members, b.snaps, b.power));
            assert_eq!(a.rank_certified, Some(true));
            let x = a.payoff_interval.as_ref().unwrap();
            let y = b.payoff_interval.as_ref().unwrap();
            assert!(x.lower_f64() <= y.upper_f64() && y.lower_f64() <= x.upper_f64());
        }
        let proposals = actual.telemetry.incumbents.warm_start.evaluations;
        assert!(
            proposals > 0 && proposals <= k.min(3) as u64,
            "score k={k} proposals={proposals} bounds={:?}",
            actual.telemetry.environment.bounds
        );
        assert!(actual.telemetry.phases.iter().any(|phase| phase.name == "search"));
    }
}

#[test]
fn certified_score_caps_reuse_leaf_exclusion_proofs_across_equivalent_leaders() {
    let (mut data, roster, mut request) = inputs(610_000, 3, 64);
    for effect in &mut data.master.leader_skill_effects {
        effect.effect_value = 0;
    }
    request.constraints.leader = None;
    request.metric = Metric::Score;
    let reference = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(reference.completion, Completion::Complete);
    assert_eq!(reference.telemetry.leaves.visited, 155);
    assert_eq!(reference.results.len(), 3);
    let paired = |deck: &ournotes_search::types::RecommendedDeck| {
        let mut pairs: Vec<_> = deck.members.into_iter().zip(deck.snaps).collect();
        pairs.sort_unstable();
        pairs
    };
    let family = paired(&reference.results[0]);
    assert!(reference.results.iter().all(|deck| paired(deck) == family));
    let leaders: std::collections::BTreeSet<_> = reference.results.iter().map(|deck| deck.members[2]).collect();
    assert_eq!(leaders.len(), 3, "equal programs must retain distinct canonical leader identities");

    // Supply real incumbents, then every legal Snap binding for these three leaders through the public
    // initial-deck path. It applies the same leaf bounds before the whole-domain traversal, so an earlier
    // family-node proof cannot conceal the complete score-cap cache this test is meant to exercise.
    // The other two leaders remain in the searched domain; no candidate or time budget is changed.
    request.initial_decks = reference
        .results
        .iter()
        .map(|deck| ournotes_search::types::DeckInput { members: deck.members, snaps: deck.snaps })
        .collect();
    for leader in leaders {
        let others: Vec<_> = (1..=5).filter(|&member| member != leader).collect();
        let members = [others[0], others[1], leader, others[2], others[3]];
        let mut bindings = 0;
        for first in 0..=5 {
            for second in 0..=5 {
                // Slot 5 means the resource is absent. A present physical Snap has exactly one owner.
                if first < 5 && first == second {
                    continue;
                }
                let mut snaps = [None; 5];
                if first < 5 {
                    snaps[first] = Some(1);
                }
                if second < 5 {
                    snaps[second] = Some(2);
                }
                request.initial_decks.push(ournotes_search::types::DeckInput { members, snaps });
                bindings += 1;
            }
        }
        assert_eq!(bindings, 31);
    }
    assert_eq!(request.initial_decks.len(), 96);
    assert!(request.initial_decks.len() <= ournotes_search::types::MAX_INITIAL_DECKS);
    request.strategy = Strategy::BranchAndBound;
    let mut uncached = None;
    for cache_entries in [0, 64] {
        request.limits.cache_entries = cache_entries;
        let result = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(result.completion, Completion::Complete);
        assert_eq!(result.optimality, ournotes_search::types::Optimality::Proven);
        assert_eq!(result.results.len(), reference.results.len());
        for (actual, expected) in result.results.iter().zip(&reference.results) {
            assert_eq!(
                (actual.members, actual.snaps, actual.power),
                (expected.members, expected.snaps, expected.power)
            );
            assert_eq!(actual.rank_certified, Some(true));
            let a = actual.payoff_interval.as_ref().unwrap();
            let b = expected.payoff_interval.as_ref().unwrap();
            assert!(a.lower_f64() <= b.upper_f64() && b.lower_f64() <= a.upper_f64());
        }
        let caps = &result.telemetry.caches.luck_score_caps;
        if cache_entries == 0 {
            assert_eq!(caps.hits, 0);
            assert_eq!(caps.peak_entries, 0);
            uncached = Some(result.results.clone());
        } else {
            assert!(caps.hits > 0, "equivalent leaders must reuse completed upper-bound proofs: {caps:?}");
            assert!(caps.peak_entries > 0);
            assert!(caps.lookups > caps.hits, "a completed exclusion must be built before reuse");
            assert_eq!(
                result.results,
                *uncached.as_ref().unwrap(),
                "cache on/off preserves complete canonical results"
            );
        }
        assert!(result.telemetry.leaves.order_bound_pruned > caps.hits, "at least one fresh exclusion must finish");
    }
}
