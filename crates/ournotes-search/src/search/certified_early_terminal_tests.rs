use super::*;
use crate::search::certified_search::evaluate_luck_context;
use crate::search::gate_tests::common::{Rng, extend_table, replace_table, roster, set_column, synth_snaps};
use crate::search::{Constraints, GekisouObjective, PlayInput, SeedSet};
use ournotes_sim::live::full::{LuckExactBudget, LuckExactSession, luck_skills};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::live::score::LiveScoreSettings;
use ournotes_sim::live::skip::{Chart, ChartNote};
use ournotes_sim::master::Master;
use ournotes_sim::scenario::{ContextInput, PowerSnapshotInput, Scenario};
use serde_json::json;

fn fixture(k: usize) -> (Master, Roster, SearchRequest) {
    let mut source = synth_snaps(&mut Rng::new(1721), 7, 0, &[]);
    let mut first = None;
    set_column(&mut source, "MasterMemberCard", &mut |row| {
        let id = row["_id"].as_i64().unwrap();
        row["_characterID"] = json!(id);
        row["_liveSkillID"] = json!(0);
        row["_gekisouSkillID"] = json!(0);
        if id == 1 {
            first = Some(row.clone());
        } else if id == 6 || id == 7 {
            *row = first.clone().unwrap();
            row["_id"] = json!(id);
            row["_characterID"] = json!(if id == 7 { 1 } else { 6 });
        }
    });
    set_column(&mut source, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(2);
        row["_gekisouMission2"] = json!(3);
        row["_gekisouMission3"] = json!(1);
    });
    replace_table(&mut source, "MasterLiveSkillEffect", json!([]));
    extend_table(
        &mut source,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"10"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"10"}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBasePoint",
        json!([
            {"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":10}
        ]),
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBonusLot",
        json!(
            (0..5)
                .flat_map(|kind| (0..4).map(move |result| json!({
                    "_id":kind*10+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1
                })))
                .collect::<Vec<_>>()
        ),
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouRankingScoreBonus",
        json!(
            (1..=3)
                .flat_map(|pattern| (1..=3).map(move |count| json!({
                    "_id":pattern*10+count,"_missionPattern":pattern,"_count":count,"_rank":1,"_scoreBonusPercent":10
                })))
                .collect::<Vec<_>>()
        ),
    );
    let master = source.master();
    let mut owned = roster(&mut Rng::new(1722), &master);
    let first = owned.members.iter().find(|member| member.id == 1).unwrap().clone();
    for id in [6, 7] {
        let mut alias = first.clone();
        alias.id = id;
        *owned.members.iter_mut().find(|member| member.id == id).unwrap() = alias;
    }
    let chart = Chart::from_notes(
        vec![ChartNote { id: 1, time_ms: 100, note_type: 1 }, ChartNote { id: 2, time_ms: 130, note_type: 1 }],
        vec![],
        &LiveScoreSettings::from_master(&master).unwrap(),
    )
    .unwrap();
    let resolved = ContextInput {
        power_snapshot: PowerSnapshotInput { event_ids: vec![], captured_jst_ticks: None },
        result_clock: None,
        event_payoff: None,
    }
    .resolve(&master, Scenario::Mission(10), Some(1004), &[(50, 150)])
    .unwrap();
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types: vec![1; 2] },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: vec![(50, 150)] }),
    }
    .in_scenario(resolved);
    (
        master,
        owned,
        SearchRequest {
            objective,
            k,
            constraints: Constraints { no_snaps: true, ..Default::default() },
            time_limit: None,
        },
    )
}

fn engine<'a, 'm>(
    pool: &'a Pool<'m>,
    request: &'a SearchRequest,
    metric: &'a Metric,
    limits: &'a Limits,
    simulation: &'a SimulationInput,
    early: bool,
) -> Engine<'a, 'm> {
    let now = crate::search::budget::now();
    let mut certified = CertifiedState::new(request.k, 0).unwrap();
    certified.early_terminal_enabled = early;
    // The long-chart retention policy must keep at most one row set even with a small native fixture.
    certified.retain_refinement = Some(false);
    Engine {
        pool,
        request,
        metric,
        event_input: None,
        simulation,
        limits,
        budget: crate::search::budget::SearchBudget::new(now, None).unwrap(),
        stop: None,
        tel: Telemetry::default(),
        rec: Recorder::new(now),
        correlated: false,
        resource: false,
        bound_scratch: Default::default(),
        bonus_scratch: Default::default(),
        order_steps: Default::default(),
        top: Vec::new(),
        certified: Some(certified),
        lottery_mode: LotteryMode::Certified,
        family: family_nodes::FamilyNodeCache::new(None, 0, 8 << 20, 31),
        lottery_free: None,
        seen: HashSet::new(),
        fifo: VecDeque::new(),
        team_scores: team_scores::TeamScores::new(0),
        programs: program_cache::ProgramCache::new(0),
        song: None,
        event: false,
        skip: None,
        live: true,
        orders: uniform::all_orders(),
        positions: uniform::order_positions().into_iter().map(|(p, _)| p).collect(),
        seeded: HashSet::new(),
        root_order: None,
        warm: None,
        progress: None,
        offered: None,
    }
}

fn deck(engine: &Engine<'_, '_>, card: usize) -> PhysicalDeck {
    uniform::canonical(engine.pool, &PhysicalDeck { members: [card, 1, 2, 3, 4], snaps: [None; 5] })
}

fn constant_payoff_ranking(engine: &Engine<'_, '_>, cards: &[usize]) -> Vec<PhysicalDeck> {
    let mut candidates: Vec<_> = cards
        .iter()
        .map(|&card| {
            let physical = deck(engine, card);
            let power =
                expectation::context(engine.pool, &physical, &engine.request.objective).unwrap().params.total_power;
            let members = physical.members.map(|index| engine.pool.members[index].id);
            (physical, power, members)
        })
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    candidates.into_iter().take(engine.request.k).map(|candidate| candidate.0).collect()
}

fn offer_native(engine: &mut Engine<'_, '_>, physical: PhysicalDeck) {
    let mut input = expectation::context(engine.pool, &physical, &engine.request.objective).unwrap();
    let power = input.params.total_power;
    let program = canonicalize_performers(&mut input);
    let skills = luck_skills(engine.pool.master).unwrap();
    let mut rows = evaluate_luck_context(engine.pool.master, &skills, &input, &PayoffMap::Score, None, || false)
        .unwrap()
        .unwrap()
        .orders;
    assert_eq!(rows.iter().map(|row| row.order).collect::<Vec<_>>(), uniform::all_orders());
    if matches!(engine.metric, Metric::ScoreAtLeast { threshold: 0 }) {
        assert!(rows.iter().all(|row| row.support.0 >= 0), "the independent full native support proves payoff one");
    }
    // Forget precision of independently complete native summaries. The resulting valid enclosures
    // cannot settle the mapping alone; a later complete summary or mapped provider may restore it.
    for row in &mut rows {
        assert!(row.evaluated);
        row.mean = F64Interval::new(f64::from(i32::MIN), f64::from(i32::MAX)).unwrap();
        row.support = (i32::MIN, i32::MAX);
        row.exact_mean = None;
        row.final_life = None;
    }
    let Metric::ScoreAtLeast { threshold } = *engine.metric else { panic!("probability fixture") };
    let map = PayoffMap::ScoreAtLeast { threshold };
    let evaluation = aggregate_orders(rows, &map).unwrap();
    assert!(evaluation.exact_payoff.is_none());
    engine.tel.leaves.visited += 1;
    engine.offer_certified(physical, power, evaluation, program, map).unwrap();
}

fn setup_limits() -> Limits {
    Limits { cache_entries: 0, max_candidates: None, time_limit_ms: None }
}

#[test]
fn early_terminal_native_small_domain_matches_final_only_canonical_results_and_all_labels() {
    let (master, owned, request) = fixture(2);
    let pool = Pool::new(&master, &owned).unwrap();
    let limits = setup_limits();
    let simulation = SimulationInput::default();
    let metric = Metric::ScoreAtLeast { threshold: 0 };
    let mut answers = Vec::new();
    for early in [false, true] {
        let mut engine = engine(&pool, &request, &metric, &limits, &simulation, early);
        let expected = constant_payoff_ranking(&engine, &[6, 5, 0]);
        for card in [6, 5, 0] {
            let physical = deck(&engine, card);
            offer_native(&mut engine, physical);
            let state = engine.certified.as_ref().unwrap();
            assert!(!state.domain_exhausted);
            assert!(!state.proof(false, None).unwrap().complete, "the unseen domain remains open");
            assert!(state.entries.values().filter(|entry| entry.refinement.is_some()).count() <= 1);
            assert_eq!(engine.tel.leaves.simulations, 0, "the offered native rows require no rematerialization");
            if engine.tel.leaves.visited < request.k as u64 {
                assert_eq!(engine.tel.lottery_refinement.terminal_attempted_orders, 0);
            }
            for retained in state.entries.values().filter_map(|entry| entry.refinement.as_ref()) {
                assert_eq!(retained.evaluation.orders.len(), ORDERS);
                assert!(retained.evaluation.orders.iter().all(|row| row.evaluated));
            }
        }
        engine.refine_certified_frontier().unwrap();
        let state = engine.certified.as_ref().unwrap();
        let proof = state.proof(false, None).unwrap();
        assert!(proof.complete && state.domain_exhausted, "early={early}, proof={proof:?}");
        let actual = proof.ordered_prefix.iter().map(|id| state.entries[id].physical).collect::<Vec<_>>();
        assert_eq!(actual, expected, "native constant payoff ranks by power and complete canonical team");
        answers.push(actual);
        assert!(state.exact_work.is_some());
    }
    assert_eq!(answers[0], answers[1]);
}

#[test]
fn early_terminal_cache_zero_late_better_alias_keeps_valid_canonical_proof() {
    let (master, owned, request) = fixture(1);
    let pool = Pool::new(&master, &owned).unwrap();
    let limits = setup_limits();
    let simulation = SimulationInput::default();
    let metric = Metric::ScoreAtLeast { threshold: 0 };
    let mut engine = engine(&pool, &request, &metric, &limits, &simulation, true);
    let first = deck(&engine, 6);
    let later = deck(&engine, 0);
    let expected = constant_payoff_ranking(&engine, &[6, 5, 0]);
    let mut a = expectation::context(&pool, &first, &request.objective).unwrap();
    let mut b = expectation::context(&pool, &later, &request.objective).unwrap();
    assert_eq!(canonicalize_performers(&mut a), canonicalize_performers(&mut b));
    assert_eq!(a.params.total_power, b.params.total_power);
    for (index, card) in [6, 5, 0].into_iter().enumerate() {
        let physical = deck(&engine, card);
        let completed_before = engine.tel.lottery_refinement.terminal_completed_orders;
        offer_native(&mut engine, physical);
        assert!(engine.tel.lottery_refinement.terminal_completed_orders - completed_before <= ORDERS as u64);
        assert!(engine.tel.lottery_refinement.terminal_attempted_orders <= (index as u64 + 1) * ORDERS as u64);
        let state = engine.certified.as_ref().unwrap();
        assert_eq!(state.entries.len(), 1);
        assert!(!state.proof(false, None).unwrap().complete);
        let offered = engine.offered.unwrap();
        assert_eq!(
            offered.power,
            expectation::context(&pool, &physical, &request.objective).unwrap().params.total_power
        );
        if let Some(payoff) = offered.payoff {
            assert_eq!(payoff, (1, 1));
        } else {
            assert!(!state.contains(&physical), "an unevaluated offered payoff is allowed only after proved exclusion");
        }
        assert!(state.frontier.get(*state.entries.keys().next().unwrap()).unwrap().exact_payoff.is_some());
    }
    assert!(!engine.certified.as_ref().unwrap().entries.contains_key(&1));
    let completed = engine.tel.lottery_refinement.terminal_completed_orders;
    engine.refine_certified_frontier().unwrap();
    assert_eq!(engine.tel.lottery_refinement.terminal_completed_orders, completed);
    let state = engine.certified.as_ref().unwrap();
    let proof = state.proof(false, None).unwrap();
    assert!(proof.complete);
    assert_eq!(proof.ordered_prefix.iter().map(|id| state.entries[id].physical).collect::<Vec<_>>(), expected);
}

#[test]
fn early_terminal_materialized_constant_installs_its_complete_aggregate_without_provider_work() {
    let (master, owned, request) = fixture(1);
    let pool = Pool::new(&master, &owned).unwrap();
    let limits = setup_limits();
    let simulation = SimulationInput::default();
    let metric = Metric::ScoreAtLeast { threshold: 0 };
    let mut engine = engine(&pool, &request, &metric, &limits, &simulation, false);
    let physical = deck(&engine, 6);
    offer_native(&mut engine, physical);
    assert!(engine.certified.as_ref().unwrap().frontier.get(1).unwrap().exact_payoff.is_none());
    engine.certified.as_mut().unwrap().entries.get_mut(&1).unwrap().refinement = None;
    assert!(engine.materialize_certified_refinement(1, false).unwrap());
    let state = engine.certified.as_ref().unwrap();
    let retained = state.entries[&1].refinement.as_ref().unwrap();
    assert_eq!(retained.evaluation.orders.len(), ORDERS);
    assert!(retained.evaluation.refinements.is_empty());
    assert_eq!(
        state.frontier.get(1).unwrap().exact_payoff,
        Some(expectation::ExactExpectation { numerator: 1, denominator: 1 })
    );
    assert!(!state.domain_exhausted);
    assert!(!state.proof(false, None).unwrap().complete);
    assert_eq!(engine.tel.lottery_refinement.terminal_attempted_orders, 0);
    engine.refine_certified_frontier().unwrap();
    assert!(engine.certified.as_ref().unwrap().proof(false, None).unwrap().complete);
    assert_eq!(engine.tel.lottery_refinement.terminal_attempted_orders, 0);
}

#[test]
fn early_terminal_uses_one_request_budget_and_exhaustion_keeps_traversal_available() {
    let (master, owned, request) = fixture(1);
    let pool = Pool::new(&master, &owned).unwrap();
    let limits = setup_limits();
    let simulation = SimulationInput::default();
    let metric = Metric::ScoreAtLeast { threshold: 0 };
    let mut engine = engine(&pool, &request, &metric, &limits, &simulation, true);
    engine.certified.as_mut().unwrap().exact_work.as_mut().unwrap().remaining_runs = 1;
    let physical = deck(&engine, 6);
    offer_native(&mut engine, physical);
    assert!(engine.stop.is_none());
    assert_eq!(engine.certified.as_ref().unwrap().exact_work.as_ref().unwrap().remaining_runs, 0);
    let attempted = engine.tel.lottery_refinement.terminal_attempted_orders;
    let next = deck(&engine, 5);
    offer_native(&mut engine, next);
    assert_eq!(engine.tel.leaves.visited, 2);
    assert_eq!(engine.tel.lottery_refinement.terminal_attempted_orders, attempted);
    assert!(engine.stop.is_none());
    engine.refine_certified_frontier().unwrap();
    assert_eq!(engine.certified.as_ref().unwrap().exact_work.as_ref().unwrap().remaining_runs, 0);
    let before = engine.certified.as_ref().unwrap().exact_work.as_ref().unwrap().remaining_frames;
    let result: Result<(), Error> = engine.with_exact_work(|_, work| {
        work.remaining_frames -= 17;
        Err(Error::Domain("test provider error".into()))
    });
    assert!(result.is_err());
    assert_eq!(engine.certified.as_ref().unwrap().exact_work.as_ref().unwrap().remaining_frames, before - 17);
    assert!(before < LuckExactBudget::default().remaining_frames);
}

#[test]
fn early_terminal_cache_zero_completed_nonconstant_stage_keeps_exact_law_available_without_repeating() {
    let (master, owned, request) = fixture(2);
    let pool = Pool::new(&master, &owned).unwrap();
    let limits = setup_limits();
    let simulation = SimulationInput::default();
    let physical = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
    let input = expectation::context(&pool, &physical, &request.objective).unwrap();
    let mut native = LuckExactSession::new(
        &master,
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
    let law = native.law(&input.performers, &mut LuckExactBudget::default(), || false).unwrap().law.unwrap();
    let low = law.atoms().iter().map(|atom| atom.score).min().unwrap();
    let high = law.atoms().iter().map(|atom| atom.score).max().unwrap();
    assert!(low < high, "use an actual nonconstant native payoff");
    let metric = Metric::ScoreAtLeast { threshold: low + (high - low + 1) / 2 };
    let mut engine = engine(&pool, &request, &metric, &limits, &simulation, true);
    for card in [6, 5] {
        let physical = deck(&engine, card);
        offer_native(&mut engine, physical);
    }
    assert_eq!(engine.tel.lottery_refinement.terminal_completed_orders, ORDERS as u64);
    let state = engine.certified.as_ref().unwrap();
    assert_eq!(state.entries[&2].terminal_stage, TerminalStage::Attempted);
    assert!(state.frontier.get(2).unwrap().exact_payoff.is_none());
    let remaining = state.exact_work.as_ref().unwrap().remaining_runs;
    let proved = state.frontier.get(2).unwrap().clone();
    // Dropping detailed order rows must not drop an already installed aggregate. Reconstruction can
    // tighten it further, but must intersect with every previously installed restriction.
    engine.certified.as_mut().unwrap().entries.get_mut(&2).unwrap().refinement = None;
    assert!(engine.materialize_certified_refinement(2, false).unwrap());
    let restored = engine.certified.as_ref().unwrap().frontier.get(2).unwrap();
    assert_eq!(proved.score.intersect(restored.score), Some(restored.score));
    assert_eq!(proved.payoff.intersect(restored.payoff), Some(restored.payoff));
    if let Some(exact) = proved.exact_payoff {
        assert_eq!(restored.exact_payoff, Some(exact));
    }
    // The original native-law provider remains valid for a mapped stage's order, independently of
    // whether the frontier still needs to schedule it once another completed proof separates the teams.
    let retained = engine.certified.as_ref().unwrap().entries[&2].refinement.as_ref().unwrap();
    let mut row = retained.evaluation.orders[0].clone();
    let map = retained.map.clone();
    let physical = engine.certified.as_ref().unwrap().entries[&2].physical;
    let mut input = expectation::context(&pool, &physical, &request.objective).unwrap();
    canonicalize_performers(&mut input);
    let performers = row.order.map(|slot| input.performers[slot].clone());
    let mut native = LuckExactSession::new(
        &master,
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
    let law = native.law(&performers, &mut LuckExactBudget::default(), || false).unwrap().law.unwrap();
    assert!(refine_order_with_exact_law(&mut row, &map, &law).unwrap());
    assert!(row.refined_payoff.as_ref().unwrap().exact.is_some());
    let completed = engine.tel.lottery_refinement.terminal_completed_orders;
    engine
        .with_exact_work(|engine, work| engine.refine_certified_terminal_payoff(2, TerminalPhase::AfterTraversal, work))
        .unwrap();
    assert_eq!(engine.tel.lottery_refinement.terminal_completed_orders, completed);
    engine.refine_certified_frontier().unwrap();
    // Only the other candidate's pending stage may add work; it may stop as soon as the frontier is proved.
    assert!(engine.tel.lottery_refinement.terminal_completed_orders <= completed + ORDERS as u64);
    let state = engine.certified.as_ref().unwrap();
    assert!(state.proof(false, None).unwrap().complete);
    assert!(state.exact_work.as_ref().unwrap().remaining_runs <= remaining);
}

#[test]
fn early_terminal_cancellation_at_first_119th_and_120th_orders_keeps_only_complete_evidence() {
    let (master, owned, request) = fixture(1);
    let pool = Pool::new(&master, &owned).unwrap();
    let limits = setup_limits();
    let simulation = SimulationInput::default();
    let metric = Metric::ScoreAtLeast { threshold: 0 };
    for stop in [1, 119, 120] {
        crate::search::budget::test_clock::with_expiry("terminal-payoff-order", stop, || {
            let mut engine = engine(&pool, &request, &metric, &limits, &simulation, true);
            engine.budget = crate::search::budget::SearchBudget::new(
                crate::search::budget::now(),
                Some(Duration::from_millis(500)),
            )
            .unwrap();
            let physical = deck(&engine, 6);
            offer_native(&mut engine, physical);
            assert_eq!(engine.stop, Some(ExitReason::TimeLimit));
            assert_eq!(engine.tel.lottery_refinement.terminal_completed_orders, stop as u64 - 1);
            let state = engine.certified.as_ref().unwrap();
            assert!(state.exact_work.is_some());
            assert!(!state.domain_exhausted);
            assert!(!state.proof(false, None).unwrap().complete);
            let retained = state.entries[&1].refinement.as_ref().unwrap();
            assert_eq!(retained.evaluation.orders.len(), ORDERS);
            assert_eq!(retained.evaluation.orders.iter().filter(|row| row.refined_payoff.is_some()).count(), stop - 1);
            assert!(state.frontier.get(1).unwrap().exact_payoff.is_none());
            // A later retry cannot obtain fresh private fold counters for the interrupted stage.
            engine.stop = None;
            engine.budget = crate::search::budget::SearchBudget::new(crate::search::budget::now(), None).unwrap();
            engine
                .with_exact_work(|engine, work| {
                    engine.refine_certified_terminal_payoff(1, TerminalPhase::DuringSearch, work)
                })
                .unwrap();
            assert_eq!(engine.tel.lottery_refinement.terminal_completed_orders, stop as u64 - 1);
        });
    }
}
