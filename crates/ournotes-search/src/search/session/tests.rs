//! Independent tiny-domain traversal/ranking checks. Point values reuse the
//! shared evaluator; no native-model equivalence is claimed by these tests.

use super::*;
use crate::auxiliary::SongTarget;
use crate::owned_snapshot::{GoalDependencies, OwnedSnapshot, ResolvedOwnedSnapshot};
use crate::search::Constraints;
use crate::search::budget::test_clock;
use crate::search::gate_tests::common;
use common::{Rng, extend_table, set_column, synth};
use ournotes_sim::data::DataChart;
use ournotes_sim::live::skip::ChartNote;
use ournotes_sim::scenario::Scenario;
use ournotes_sim::scenario::{ContextInput, PowerSnapshotInput};
use serde_json::json;

fn fixture(members: i64, snaps: i64) -> (DeckData, OwnedSnapshot) {
    let mut source = synth(&mut Rng::new(351), members, snaps);
    set_column(&mut source, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    extend_table(&mut source, "MasterMemberCardLevelLimit", (1..=5).flat_map(|rarity| (1..=5).map(move |awake| json!({"_id":rarity*10+awake,"_rarity":rarity,"_awakeCount":awake,"_limitLevel":10+awake*10}))).collect());
    let master = source.master();
    let ranks: Vec<_> = master.characters.iter().map(|row| json!({"id":row.id,"value":1})).collect();
    let raw = json!({
        "format":"ournotes.owned-snapshot/1","datasetId":"synthetic-351","revision":"rev-1",
        "ownedFacts":{"memberIds":(1..=members).collect::<Vec<_>>(),"snapIds":(1..=snaps).collect::<Vec<_>>(),"memberCoverage":"complete","snapCoverage":"complete"},
        "eligible":{
            "members":(1..=members).map(|id|json!({"id":id,"level":1,"awake":1,"rank":1})).collect::<Vec<_>>(),
            "snaps":(1..=snaps).map(|id|json!({"id":id,"level":1,"rank":1})).collect::<Vec<_>>()
        },
        "player":{"characterRanks":{"coverage":"complete","values":ranks},"characterTotalRank":master.characters.len(),"vipRank":1,"bandItems":[],"memory":{"musicRanks":[],"unlockedMembers":[],"unlockedSnaps":[]},"eventIds":[]},
        "assumptions":[]
    });
    let data = DeckData {
        master,
        provenance: json!({"region":"synthetic","model":"current-core only"}),
        sha256: None,
        charts: vec![DataChart {
            score_id: 1004,
            asset_key: "tiny".into(),
            asset_sha256: "synthetic".into(),
            notes: vec![ChartNote { id: 1, time_ms: 1000, note_type: 1 }],
            judgement_types: vec![1],
            skill_event_ms: vec![],
            fevers: vec![],
        }],
    };
    (data, OwnedSnapshot::from_json(&raw.to_string()).unwrap())
}
fn binding() -> SessionBinding {
    SessionBinding {
        job_id: "job-1".into(),
        input_revision: "rev-1".into(),
        dataset_id: "synthetic-351".into(),
        objective_hash: "caller-verified-test-objective".into(),
    }
}
fn request(metric: Metric) -> RecommendationRequest {
    let power = matches!(metric, Metric::Power);
    RecommendationRequest {
        format: REQUEST_FORMAT.into(),
        execution: if power {
            Execution::Power { music_id: None, event_parameter: false }
        } else {
            Execution::Skip { score_id: 1004 }
        },
        scenario: Some(Scene::Free { music_id: 10 }),
        context: None,
        metric,
        aggregation: Aggregation::Expected,
        goal: None,
        constraints: Constraints::default(),
        k: 7,
        strategy: Strategy::Exhaustive,
        limits: Limits { time_limit_ms: None, max_candidates: None, cache_entries: 13 },
        network_confirmations: None,
        simulation: SimulationInput::default(),
        initial_decks: Vec::new(),
    }
}
fn resolve<'m>(
    data: &'m DeckData,
    snapshot: &OwnedSnapshot,
    request: &RecommendationRequest,
) -> ResolvedOwnedSnapshot<'m> {
    snapshot
        .resolve_data(
            data,
            "synthetic-351",
            if matches!(request.execution, Execution::Power { .. }) {
                GoalDependencies::Power
            } else {
                GoalDependencies::Skip
            },
        )
        .resolved
        .unwrap()
}
fn slice(work: u64) -> StepBudget {
    StepBudget { max_work_units: work, time_slice_ms: None }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OracleRow {
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    payoff: i128,
    score: Option<i32>,
}
fn row(deck: &RecommendedDeck) -> OracleRow {
    assert_eq!(deck.expected_payoff.as_ref().expect("exact deterministic fixture").denominator, "1");
    OracleRow {
        members: deck.members,
        snaps: deck.snaps,
        power: deck.power,
        payoff: deck.expected_payoff.as_ref().expect("exact deterministic fixture").numerator.parse().unwrap(),
        score: deck.expected_score.as_ref().map(|score| {
            assert_eq!(score.denominator, "1");
            score.numerator.parse().unwrap()
        }),
    }
}

// Flat cartesian decoding and independent full-deck filtering, unlike the
// production DFS. It uses no production comparator/Top-K/cache/matching/prune.
fn oracle(data: &DeckData, resolved: &ResolvedOwnedSnapshot, request: &RecommendationRequest) -> Vec<OracleRow> {
    let members: Vec<_> = resolved.snapshot().eligible.members.iter().map(|card| card.id).collect();
    let mut snaps = vec![None];
    if !request.constraints.no_snaps {
        snaps.extend(resolved.snapshot().eligible.snaps.iter().map(|card| Some(card.id)));
    }
    let context = ContextInput {
        power_snapshot: PowerSnapshotInput { event_ids: vec![], captured_jst_ticks: None },
        result_clock: None,
        event_payoff: None,
    }
    .resolve(
        &data.master,
        Scenario::Free(10),
        if matches!(request.execution, Execution::Skip { .. }) { Some(1004) } else { None },
        &[],
    )
    .unwrap();
    let objective = match request.execution {
        Execution::Power { .. } => Objective::Power { music_id: None, event: false },
        _ => Objective::SkipScore { score_id: 1004, chart: data.chart(1004).unwrap() },
    }
    .in_scenario(context);
    let decode = |mut index: usize, radix: usize| {
        let mut result = [0; 5];
        for digit in &mut result {
            *digit = index % radix;
            index /= radix;
        }
        result
    };
    let mut rows = Vec::new();
    for member_code in 0..members.len().pow(5) {
        let m = decode(member_code, members.len()).map(|digit| members[digit]);
        let mut characters: Vec<_> = m.iter().map(|&id| data.master.member_card(id).unwrap().character_id).collect();
        characters.sort_unstable();
        if characters.windows(2).any(|pair| pair[0] == pair[1])
            || request.constraints.leader.is_some_and(|id| m[2] != id)
            || request.constraints.include_members.iter().any(|id| !m.contains(id))
            || request.constraints.exclude_members.iter().any(|id| m.contains(id))
        {
            continue;
        }
        for snap_code in 0..snaps.len().pow(5) {
            let s = decode(snap_code, snaps.len()).map(|digit| snaps[digit]);
            let mut ids: Vec<_> = s.iter().flatten().copied().collect();
            ids.sort_unstable();
            if ids.windows(2).any(|pair| pair[0] == pair[1])
                || request.constraints.exclude_snaps.iter().any(|id| ids.contains(id))
            {
                continue;
            }
            let (power, score) = resolved.evaluate_deck(m, s, &objective).unwrap();
            let payoff = match request.metric {
                Metric::Power => power as i128,
                Metric::Score => score.unwrap() as i128,
                Metric::ScoreAtLeast { threshold } => i128::from(score.unwrap() >= threshold),
                Metric::CappedScore { threshold } => score.unwrap().min(threshold) as i128,
                _ => unreachable!(),
            };
            rows.push(OracleRow { members: m, snaps: s, power, payoff, score });
        }
    }
    rows.sort_by(|a, b| {
        b.payoff.cmp(&a.payoff).then(b.power.cmp(&a.power)).then(a.members.cmp(&b.members)).then(a.snaps.cmp(&b.snaps))
    });
    rows
}
fn finish(session: &mut SearchSession, work: u64) -> SessionProgress {
    for _ in 0..1_000_000 {
        let result = session.step(&binding(), slice(work)).unwrap();
        assert!(result.last_step_work_units <= work);
        assert!(result.telemetry.leaves.evaluated <= result.telemetry.leaves.visited);
        if result.status != SessionStatus::Running {
            return result;
        }
    }
    panic!("session lost termination");
}

#[test]
fn maximum_session_keeps_deterministic_model_while_yielding_and_after_exhaustion() {
    let (data, snapshot) = fixture(5, 0);
    for metric in [Metric::Power, Metric::Score] {
        let mut request = request(metric);
        request.aggregation = Aggregation::Maximum;
        let resolved = resolve(&data, &snapshot, &request);
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let initial = session.progress(&binding()).unwrap();
        assert_eq!(initial.status, SessionStatus::Running);
        assert_eq!(initial.maximum_model, Some(MaximumModel::for_execution(false)));
        assert_eq!(
            serde_json::to_value(&initial).unwrap()["maximumModel"],
            json!({
                "kind":"deterministic", "performanceOrders":1,
                "rootSeedRealizability":"notApplicable", "bestOrderCertificate":"notApplicable"
            })
        );
        let completed = finish(&mut session, u64::MAX);
        assert_eq!(completed.status, SessionStatus::Exhausted);
        assert_eq!(completed.optimality, Optimality::Proven);
        assert_eq!(completed.maximum_model, initial.maximum_model);
    }
}

#[test]
fn independent_oracle_matches_four_goals_and_every_step_boundary() {
    let (data, snapshot) = fixture(5, 2);
    for metric in [
        Metric::Power,
        Metric::Score,
        Metric::ScoreAtLeast { threshold: i32::MAX },
        Metric::CappedScore { threshold: 1 },
    ] {
        let request = request(metric);
        let resolved = resolve(&data, &snapshot, &request);
        let rows = oracle(&data, &resolved, &request);
        assert_eq!(rows.len(), 120 * 31);
        let mut unbounded = resolved.start_search_session(&data, &request, binding()).unwrap();
        let exact = finish(&mut unbounded, u64::MAX);
        assert_eq!(exact.status, SessionStatus::Exhausted);
        assert_eq!(exact.completion, Some(Completion::Complete));
        assert_eq!(exact.optimality, Optimality::Proven);
        assert_eq!(exact.results.iter().map(row).collect::<Vec<_>>(), rows[..request.k]);
        assert_eq!(exact.goal_spec.strategy, "exhaustive");
        assert_eq!(exact.goal_spec.result_identity, "physicalDeck");
        assert_eq!(exact.telemetry.leaves.evaluated, rows.len() as u64);
        for work in [1, 7, 113] {
            let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
            let result = finish(&mut session, work);
            assert_eq!(result.results, exact.results);
            assert_eq!(result.telemetry.leaves.evaluated, exact.telemetry.leaves.evaluated);
            assert_eq!(result.telemetry.nodes, exact.telemetry.nodes);
            assert!(result.telemetry.caches.candidates.peak_entries <= 13);
            assert_eq!(result.telemetry.leaves.peak_retained, request.k);
        }
        if matches!(request.metric, Metric::Power) {
            assert!(exact.results.iter().all(|deck| deck.expected_score.is_none()
                && deck.score_summary.is_none()
                && deck.best_order.is_none()));
        }
    }
}

#[test]
fn cancelled_frontier_resumes_without_revisiting_or_resetting_budgets() {
    let (data, snapshot) = fixture(6, 1);
    let mut request = request(Metric::Power);
    request.constraints.leader = Some(3);
    request.constraints.include_members = vec![6];
    request.constraints.exclude_members = vec![1];
    let resolved = resolve(&data, &snapshot, &request);
    let rows = oracle(&data, &resolved, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    for _ in 0..7 {
        session.step(&binding(), slice(73)).unwrap();
        let paused = session.cancel(&binding()).unwrap();
        assert_eq!(paused.status, SessionStatus::Cancelled);
        assert_eq!(paused.completion, None);
        assert_eq!(paused.optimality, Optimality::Unproven);
        let ignored_step = session.step(&binding(), slice(1000)).unwrap();
        assert_eq!(ignored_step.telemetry.nodes, paused.telemetry.nodes);
        assert_eq!(ignored_step.results, paused.results);
        session.resume(&binding()).unwrap();
    }
    let exact = finish(&mut session, 71);
    assert_eq!(exact.results.iter().map(row).collect::<Vec<_>>(), rows[..request.k]);
    assert_eq!(exact.telemetry.leaves.evaluated, rows.len() as u64);
    assert_eq!(exact.resolved_context["ownedSnapshot"]["revision"], "rev-1");
    assert_eq!(resolved.snapshot().eligible.members[0].live_skill_level, None);
}

#[test]
fn input_clones_and_public_id_ties_are_independent_of_import_order() {
    let (data, mut snapshot) = fixture(5, 2);
    let original = request(Metric::Power);
    let mut request = original.clone();
    let resolved = resolve(&data, &snapshot, &request);
    let mut first = resolved.start_search_session(&data, &request, binding()).unwrap();
    request.k = 1;
    request.constraints.leader = Some(1);
    snapshot.eligible.members.reverse();
    snapshot.eligible.snaps.reverse();
    let reversed = resolve(&data, &snapshot, &original);
    let mut second = reversed.start_search_session(&data, &original, binding()).unwrap();
    let a = finish(&mut first, 37);
    let b = finish(&mut second, 39);
    assert_eq!(a.results, b.results);
    assert_eq!(a.results.len(), 7);
}

#[test]
fn every_changed_binding_discards_complete_and_in_progress_results_permanently() {
    let (data, snapshot) = fixture(5, 0);
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    for field in 0..4 {
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        if field == 0 {
            finish(&mut session, u64::MAX);
        } else {
            while session.step(&binding(), slice(1)).unwrap().results.is_empty() {}
        }
        let mut changed = binding();
        match field {
            0 => changed.job_id.push('x'),
            1 => changed.input_revision.push('x'),
            2 => changed.dataset_id.push('x'),
            _ => changed.objective_hash.push('x'),
        }
        let stale = session.progress(&changed).unwrap();
        assert_eq!(stale.status, SessionStatus::Stale);
        assert!(stale.results.is_empty());
        assert_eq!(stale.completion, None);
        assert!(session.resume(&binding()).is_err());
        assert_eq!(session.step(&binding(), slice(u64::MAX)).unwrap().status, SessionStatus::Stale);
    }
}

#[test]
fn zero_step_and_zero_total_budgets_never_claim_complete_or_visit_candidates() {
    let (data, snapshot) = fixture(5, 0);
    let mut request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    for step in [slice(0), StepBudget { max_work_units: u64::MAX, time_slice_ms: Some(0) }] {
        let result = session.step(&binding(), step).unwrap();
        assert_eq!(result.status, SessionStatus::Running);
        assert_eq!(result.telemetry.nodes, 0);
        assert_eq!(result.completion, None);
    }
    for clock in [true, false] {
        request.limits.time_limit_ms = clock.then_some(0);
        request.limits.max_candidates = (!clock).then_some(0);
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let result = session.step(&binding(), slice(u64::MAX)).unwrap();
        assert_eq!(result.status, if clock { SessionStatus::TimeLimit } else { SessionStatus::CandidateLimit });
        assert_eq!(result.telemetry.nodes, 0);
        assert_eq!(result.telemetry.leaves.evaluated, 0);
        assert_eq!(result.completion, None);
        assert!(session.resume(&binding()).is_err());
    }
}

#[test]
fn candidate_budget_is_shared_and_exact_cardinality_boundary_is_sound() {
    let (data, snapshot) = fixture(5, 0);
    let mut request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    for cap in [1, 119, 120, 121] {
        request.limits.max_candidates = Some(cap);
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let result = finish(&mut session, 17);
        assert_eq!(result.telemetry.leaves.evaluated, cap.min(120));
        assert_eq!(result.telemetry.leaves.visited, cap.min(120));
        assert_eq!(result.status, if cap < 120 { SessionStatus::CandidateLimit } else { SessionStatus::Exhausted });
        assert_eq!(result.completion, (cap >= 120).then_some(Completion::Complete));
    }
}

#[test]
fn fixed_monotonic_deadline_includes_preparation_and_cancelled_wait() {
    let (data, snapshot) = fixture(5, 0);
    let mut request = request(Metric::Power);
    request.limits.time_limit_ms = Some(10);
    let resolved = resolve(&data, &snapshot, &request);
    test_clock::with_expiry("session_prepare", 1, || {
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let result = session.step(&binding(), slice(u64::MAX)).unwrap();
        assert_eq!(result.status, SessionStatus::TimeLimit);
        assert_eq!(result.telemetry.nodes, 0);
    });
    test_clock::with_expiry("cancel_wait", 1, || {
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        session.cancel(&binding()).unwrap();
        test_clock::stage("cancel_wait");
        assert!(session.resume(&binding()).is_err());
        let result = session.progress(&binding()).unwrap();
        assert_eq!(result.status, SessionStatus::TimeLimit);
        assert_eq!(result.completion, None);
    });
}

#[test]
fn active_slice_yields_but_does_not_reset_the_frontier_or_total_deadline() {
    let (data, snapshot) = fixture(5, 0);
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    test_clock::with_expiry("session_work", 1, || {
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let first = session.step(&binding(), StepBudget { max_work_units: u64::MAX, time_slice_ms: Some(10) }).unwrap();
        assert_eq!(first.status, SessionStatus::Running);
        assert_eq!(first.last_step_work_units, 1);
        assert_eq!(first.telemetry.leaves.evaluated, 0);
        let final_result = finish(&mut session, 97);
        assert_eq!(final_result.status, SessionStatus::Exhausted);
        assert_eq!(final_result.telemetry.leaves.evaluated, 120);
    });
}

#[test]
fn deadline_after_atomic_candidate_preserves_only_the_complete_candidate() {
    let (data, snapshot) = fixture(5, 0);
    let mut request = request(Metric::Power);
    request.limits.time_limit_ms = Some(10);
    let resolved = resolve(&data, &snapshot, &request);
    test_clock::with_expiry("session_candidate", 1, || {
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let result = session.step(&binding(), slice(u64::MAX)).unwrap();
        assert_eq!(result.status, SessionStatus::TimeLimit);
        assert_eq!(result.telemetry.leaves.evaluated, 1);
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.telemetry.leaves.partial, 0);
        assert_eq!(result.completion, None);
        assert!(session.resume(&binding()).is_err());
    });
}

#[test]
fn no_step_or_zero_budget_bypasses_capability_and_input_validation() {
    let (data, snapshot) = fixture(5, 0);
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let foreign = data.clone();
    assert!(resolved.start_search_session(&foreign, &request, binding()).is_err());
    let master_only = snapshot.resolve(&data.master, "synthetic-351", GoalDependencies::Power).resolved.unwrap();
    assert!(master_only.start_search_session(&data, &request, binding()).is_err());
    let mut invalid = binding();
    invalid.input_revision.push('x');
    assert!(resolved.start_search_session(&data, &request, invalid).is_err());
    let mut invalid = request.clone();
    invalid.execution = Execution::Live { score_id: 1004, gekisou: false, play: PlayPolicy::TheoreticalBest };
    assert!(resolved.start_search_session(&data, &invalid, binding()).is_err());
    for change in 0..4 {
        let mut invalid = request.clone();
        invalid.limits.time_limit_ms = Some(0);
        match change {
            0 => invalid.k = 0,
            1 => invalid.constraints.leader = Some(999),
            2 => {
                invalid.initial_decks =
                    vec![DeckInput { members: [1, 2, 3, 4, 5], snaps: [None; 5] }; MAX_INITIAL_DECKS + 1]
            }
            _ => invalid.strategy = Strategy::Candidate { power_seeds: 1, proposals: 1, proposal_seed: 1 },
        }
        assert!(resolved.start_search_session(&data, &invalid, binding()).is_err());
    }
}

#[test]
fn empty_or_infeasible_domain_requires_a_traversal_and_retains_snapshot_scope() {
    let (data, snapshot) = fixture(4, 0);
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    let first = session.progress(&binding()).unwrap();
    assert_eq!(first.completion, None);
    let exhausted = session.step(&binding(), slice(1)).unwrap();
    assert_eq!(exhausted.status, SessionStatus::Exhausted);
    assert!(exhausted.results.is_empty());
    assert_eq!(exhausted.resolved_context["ownedSnapshot"]["datasetId"], "synthetic-351");
}

#[test]
fn every_single_work_step_contains_only_complete_oracle_values_in_prefix_top_k() {
    use std::collections::BTreeSet;
    let (data, snapshot) = fixture(5, 0);
    let request = request(Metric::Score);
    let resolved = resolve(&data, &snapshot, &request);
    let rows = oracle(&data, &resolved, &request);
    let mut visited = BTreeSet::new();
    let mut evaluated = 0;
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    loop {
        let progress = session.step(&binding(), slice(1)).unwrap();
        assert!(progress.telemetry.leaves.evaluated - evaluated <= 1);
        if progress.telemetry.leaves.evaluated > evaluated {
            let p = session.cursor.physical;
            let members = p.members.map(|index| session.prepared.pool.members[index].id);
            let snaps = p.snaps.map(|index| index.map(|index| session.prepared.pool.snaps[index].id));
            assert!(visited.insert((members, snaps)), "duplicate candidate after stepping");
        }
        evaluated = progress.telemetry.leaves.evaluated;
        let expected: Vec<_> = rows
            .iter()
            .filter(|entry| visited.contains(&(entry.members, entry.snaps)))
            .take(request.k)
            .cloned()
            .collect();
        assert_eq!(progress.results.iter().map(row).collect::<Vec<_>>(), expected);
        assert_eq!(progress.telemetry.leaves.partial, 0);
        if progress.status != SessionStatus::Running {
            assert_eq!(progress.status, SessionStatus::Exhausted);
            assert_eq!(visited.len(), rows.len());
            break;
        }
    }
}

#[test]
fn inert_snap_ties_follow_explicit_none_first_public_id_order() {
    let (mut data, snapshot) = fixture(5, 2);
    for snap in &mut data.master.support_cards {
        snap.performance_power_max = 0;
        snap.technic_power_max = 0;
        snap.visual_power_max = 0;
        snap.card_type = 99; // No type-link bonus; all members have types 1..=5.
    }
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let expected = oracle(&data, &resolved, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    let result = finish(&mut session, 19);
    assert_eq!(result.results.iter().map(row).collect::<Vec<_>>(), expected[..request.k]);
    assert_eq!(result.results[0].snaps, [None; 5]);
    assert_eq!(result.results[1].snaps, [None, None, None, None, Some(1)]);
    assert_eq!(result.results[2].snaps, [None, None, None, None, Some(2)]);
    assert_eq!(result.results[0].members, result.results[1].members);
    assert_eq!(result.results[0].power, result.results[1].power);
}

#[test]
fn missing_point_model_dependency_fails_instead_of_exhausting_or_dropping_cards() {
    let (mut data, snapshot) = fixture(5, 0);
    for effect in &mut data.master.leader_skill_effects {
        effect.skill_condition_group = 0;
        effect.skill_target_ids = vec![999_999];
    }
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    assert!(session.step(&binding(), slice(u64::MAX)).is_err());
    let failed = session.progress(&binding()).unwrap();
    assert_eq!(failed.status, SessionStatus::Failed);
    assert_eq!(failed.completion, None);
    assert!(failed.results.is_empty());
    assert_eq!(failed.telemetry.leaves.evaluated, 0);
    assert!(session.resume(&binding()).is_err());
}

#[test]
fn alternate_cards_of_one_character_and_conflicting_required_cards_match_legal_oracle() {
    let (mut data, snapshot) = fixture(6, 0);
    data.master.member_cards.iter_mut().find(|card| card.id == 6).unwrap().character_id = 1;
    let mut request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let expected = oracle(&data, &resolved, &request);
    assert_eq!(expected.len(), 240);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    let result = finish(&mut session, 41);
    assert_eq!(result.results.iter().map(row).collect::<Vec<_>>(), expected[..request.k]);
    assert_eq!(result.telemetry.leaves.evaluated, expected.len() as u64);
    request.constraints.include_members = vec![1, 6];
    assert!(oracle(&data, &resolved, &request).is_empty());
    let mut infeasible = resolved.start_search_session(&data, &request, binding()).unwrap();
    let result = finish(&mut infeasible, 1);
    assert_eq!(result.status, SessionStatus::Exhausted);
    assert!(result.results.is_empty());
    assert_eq!(result.telemetry.leaves.evaluated, 0);
}

fn manual_items_fixture() -> (DeckData, OwnedSnapshot) {
    use crate::owned_snapshot::{BandItemFact, BandItemFacts, Coverage};
    use ournotes_sim::master::{BandItemEffectRow, BandItemLevelRow, BandItemRow};
    let (mut data, mut snapshot) = fixture(5, 0);
    data.master.band_items =
        (101..=102).map(|id| BandItemRow { id, band_id: id - 100, ..Default::default() }).collect();
    data.master.band_item_levels = (101..=102)
        .flat_map(|id| {
            (1..=30).map(move |level| BandItemLevelRow {
                id: id * 1000 + level,
                band_item_id: id,
                level,
                player_rank: level,
            })
        })
        .collect();
    data.master.band_item_effects.push(BandItemEffectRow {
        id: 999_999,
        band_item_id: 101,
        level: 31,
        skill_target_ids: vec![1],
        skill_effect_type: 1000,
        effect_value: 1000,
    });
    data.master.reindex().unwrap();
    snapshot.player.band_items = None;
    snapshot.player.band_item_facts = Some(BandItemFacts {
        coverage: Coverage::Complete,
        values: vec![
            BandItemFact { id: 101, owned: Some(true), level: Some(10) },
            BandItemFact { id: 102, owned: Some(false), level: None },
        ],
    });
    let mut other = data.charts[0].clone();
    other.score_id = 2003;
    other.asset_key = "second-synthetic-song".into();
    data.charts.push(other);
    (data, snapshot)
}

#[test]
fn manual_player_bonuses_are_shared_by_session_fixed_deck_and_each_ranked_song() {
    let (data, snapshot) = manual_items_fixture();
    for metric in [Metric::Power, Metric::Score] {
        let request = request(metric);
        let resolved = resolve(&data, &snapshot, &request);
        let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
        let result = finish(&mut session, 29);
        assert_eq!(result.status, SessionStatus::Exhausted);
        assert_eq!(result.resolved_context["ownedSnapshot"]["bandItemInput"]["coverage"], "complete");
        let evidence = &result.resolved_context["ownedSnapshot"]["playerBonusEvidence"];
        assert_eq!(evidence["status"], "conditionalCurrentCoreProjection");
        assert_eq!(evidence["memoryEffects"]["status"], "unmodeledLatestNative");
        assert_eq!(evidence["eventEffects"]["status"], "unmodeledLatestNative");
        let chosen = &result.results[0];
        let fixed = resolved.evaluate_fixed(&data, &request, chosen.members, chosen.snaps).unwrap();
        assert_eq!(fixed.results[0], *chosen);
        let mut no_items = snapshot.clone();
        let row = &mut no_items.player.band_item_facts.as_mut().unwrap().values[0];
        row.owned = Some(false);
        row.level = None;
        let baseline =
            resolve(&data, &no_items, &request).evaluate_fixed(&data, &request, chosen.members, chosen.snaps).unwrap();
        assert!(chosen.power > baseline.results[0].power);
        if matches!(request.metric, Metric::Score) {
            let targets = [
                SongTarget { score_id: 1004, scenario: Scene::Free { music_id: 10 } },
                SongTarget { score_id: 2003, scenario: Scene::Free { music_id: 20 } },
            ];
            let ranked = resolved.rank_fixed_songs(&data, &request, chosen.members, chosen.snaps, &targets).unwrap();
            assert_eq!(ranked.completion, Completion::Complete);
            for song in &ranked.results {
                let target = targets.iter().find(|target| target.score_id == song.score_id).unwrap();
                let mut song_request = request.clone();
                song_request.execution = Execution::Skip { score_id: target.score_id };
                song_request.scenario = Some(target.scenario.clone());
                let fixed = resolved.evaluate_fixed(&data, &song_request, chosen.members, chosen.snaps).unwrap();
                assert_eq!(song.evaluation.results, fixed.results);
                let mut song_session = resolved.start_search_session(&data, &song_request, binding()).unwrap();
                let searched = finish(&mut song_session, 31);
                let searched_fixed = resolved
                    .evaluate_fixed(&data, &song_request, searched.results[0].members, searched.results[0].snaps)
                    .unwrap();
                assert_eq!(searched.results[0], searched_fixed.results[0]);
                assert_eq!(
                    song.evaluation.resolved_context["ownedSnapshot"]["bandItemInput"]["notOwnedIds"],
                    json!([102])
                );
            }
        }
    }
}

#[test]
fn incomplete_or_reserved_manual_facts_cannot_reach_zero_budget_session_entry() {
    use crate::owned_snapshot::Coverage;
    let (data, mut snapshot) = manual_items_fixture();
    snapshot.player.band_item_facts.as_mut().unwrap().coverage = Coverage::Partial;
    snapshot.player.band_item_facts.as_mut().unwrap().values.pop();
    let partial = snapshot.resolve_data(&data, "synthetic-351", GoalDependencies::Power);
    assert!(partial.resolved.is_none());
    assert!(partial.missing.iter().any(|issue| issue.path == "player.bandItemFacts.values[102].owned"));
    let (_, mut snapshot) = manual_items_fixture();
    snapshot.player.band_item_facts.as_mut().unwrap().values[0].level = Some(31);
    let invalid = snapshot.resolve_data(&data, "synthetic-351", GoalDependencies::Power);
    assert!(invalid.resolved.is_none());
    assert!(invalid.errors.iter().any(|issue| issue.code == "invalid_level"));
    let (_, mut snapshot) = manual_items_fixture();
    snapshot.player.band_item_facts.as_mut().unwrap().values[0].level = None;
    let unknown = snapshot.resolve_data(&data, "synthetic-351", GoalDependencies::Skip);
    assert!(unknown.resolved.is_none());
    assert!(unknown.missing.iter().any(|issue| issue.path == "player.bandItemFacts.values[101].level"));
    let mut request = request(Metric::Power);
    request.limits.time_limit_ms = Some(0);
    let (_, lawful) = manual_items_fixture();
    let resolved = resolve(&data, &lawful, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    assert_eq!(session.step(&binding(), slice(1)).unwrap().status, SessionStatus::TimeLimit);
    // No resolved capability exists for any rejected input, so a zero deadline
    // cannot suppress the missing list or create an input with default bonuses.
    assert!(partial.resolved.is_none() && invalid.resolved.is_none() && unknown.resolved.is_none());
}

#[test]
fn manual_bonus_changes_do_not_mutate_in_flight_session_and_stale_results_clear() {
    let (data, mut snapshot) = manual_items_fixture();
    let request = request(Metric::Power);
    let resolved = resolve(&data, &snapshot, &request);
    let mut session = resolved.start_search_session(&data, &request, binding()).unwrap();
    snapshot.player.band_item_facts.as_mut().unwrap().values[0].level = Some(1);
    snapshot.revision = "rev-2".into();
    let old = finish(&mut session, 23);
    let chosen = &old.results[0];
    assert_eq!(
        chosen,
        resolved.evaluate_fixed(&data, &request, chosen.members, chosen.snaps).unwrap().results.first().unwrap()
    );
    let changed =
        resolve(&data, &snapshot, &request).evaluate_fixed(&data, &request, chosen.members, chosen.snaps).unwrap();
    assert!(chosen.power > changed.results[0].power);
    let mut new_binding = binding();
    new_binding.input_revision = "rev-2".into();
    let late = session.progress(&new_binding).unwrap();
    assert_eq!(late.status, SessionStatus::Stale);
    assert!(late.results.is_empty());
    assert!(session.resume(&binding()).is_err());
}
