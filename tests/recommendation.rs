//! Production solver against the unpruned physical-deck oracle, including exact tie order
//! and termination boundaries. Synthetic inputs are independent of OCR/reviewed player data.
mod common;
use common::{Rng, roster, short_chart, synth_snaps};
use ournotes_deck::search::{
    expectation::{self, ExactExpectation, FiniteSeedLaw},
    recommendation::*,
};
use ournotes_deck::{
    Error,
    live::model::JudgementStream,
    search::{Completion, Constraints, Objective, PlayInput, Pool, SearchRequest},
};

fn unlimited() -> Limits {
    Limits { time_limit_ms: None, max_candidates: None, cache_entries: 17 }
}
fn fixture(snaps: i64) -> (ournotes_deck::master::Master, ournotes_deck::cards::Roster, Objective) {
    let mut rng = Rng::new(20261001);
    let mut s = synth_snaps(&mut rng, 5, snaps, &[1, 3, 6, 10, 11]);
    for (name, rows) in &mut s.tables {
        if name == "MasterMemberCard" {
            for (i, r) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                r["_characterID"] = serde_json::json!(i + 1)
            }
        }
    }
    let master = s.master();
    let roster = roster(&mut rng, &master);
    let (chart, jt) = short_chart(&mut rng, 8, true);
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types: jt },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    };
    (master, roster, objective)
}
fn request(objective: Objective, k: usize) -> SearchRequest {
    SearchRequest {
        objective,
        k,
        constraints: Constraints { leader: Some(1), no_snaps: true, ..Default::default() },
        time_limit: None,
    }
}
fn assert_oracle(pool: &Pool, request: &SearchRequest, law: &FiniteSeedLaw) {
    let reference = expectation::oracle(pool, request, law).unwrap();
    let out = solve_physical(
        pool,
        request,
        law,
        &Metric::Score,
        None,
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(out.completion, Completion::Complete);
    assert_eq!(out.optimality, Optimality::Proven);
    assert_eq!(out.exit_reason, ExitReason::Exhausted);
    assert_eq!(out.results.len(), reference.results.len());
    for (a, b) in out.results.iter().zip(&reference.results) {
        assert_eq!((a.members, a.snaps, a.power), (b.members, b.snaps, b.power));
        assert_eq!(a.expected_score, Some(b.evaluation.expected_score.into()));
        assert_eq!(a.expected_payoff, Fraction::from(b.evaluation.expected_payoff));
        for (o, r) in a.atoms.iter().zip(&b.evaluation.outcomes) {
            assert_eq!(
                (o.root_seed, o.performance_order, o.score, o.payoff.clone()),
                (r.root_seed, r.performance_order, r.final_score, r.terminal_payoff.to_string())
            );
            assert_eq!(o.weight, r.weight.to_string());
        }
    }
    assert!(out.stats.peak_retained_decks <= request.k);
    assert!(out.stats.peak_cache_entries <= 17);
}

#[test]
fn exact_physical_topk_matches_oracle_with_signed_duplicate_mass_and_snap_identity() {
    let (m, r, o) = fixture(2);
    let pool = Pool::new(&m, &r).unwrap();
    let law = FiniteSeedLaw::new(vec![(1, 1), (-1, 3), (1, 2), (i32::MIN, 1), (i32::MAX, 1)]).unwrap();
    for k in [1, 3, 24] {
        assert_oracle(&pool, &request(o.clone(), k), &law);
    }
    let mut q = request(o, 9);
    q.constraints.no_snaps = false;
    assert_oracle(&pool, &q, &law);
}

#[test]
fn budget_at_space_size_is_complete_but_partial_space_never_claims_rank() {
    let (m, r, o) = fixture(0);
    let pool = Pool::new(&m, &r).unwrap();
    let q = request(o, 3);
    let law = FiniteSeedLaw::new(vec![(42, 1)]).unwrap();
    for cap in [0, 1, 23, 24, 25] {
        let mut limits = unlimited();
        limits.max_candidates = Some(cap);
        let out = solve_physical(
            &pool,
            &q,
            &law,
            &Metric::Score,
            None,
            &limits,
            &Strategy::Exhaustive,
            None,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(out.stats.evaluated, cap.min(24));
        assert!(out.results.len() <= 3);
        if cap >= 24 {
            assert_eq!(
                (out.completion, out.exit_reason, out.optimality),
                (Completion::Complete, ExitReason::Exhausted, Optimality::Proven)
            )
        } else {
            assert_eq!(
                (out.completion, out.exit_reason, out.optimality),
                (Completion::TimedOut, ExitReason::CandidateLimit, Optimality::Unproven)
            );
        }
    }
    let mut zero = unlimited();
    zero.time_limit_ms = Some(0);
    let out =
        solve_physical(&pool, &q, &law, &Metric::Score, None, &zero, &Strategy::Exhaustive, None, &Default::default())
            .unwrap();
    assert!(out.results.is_empty());
    assert_eq!(out.exit_reason, ExitReason::TimeLimit);
}

#[test]
fn candidate_values_are_exact_but_search_always_labels_heuristic() {
    let (m, r, o) = fixture(2);
    let pool = Pool::new(&m, &r).unwrap();
    let mut q = request(o, 5);
    q.constraints.no_snaps = false;
    let law = FiniteSeedLaw::new(vec![(1, u64::MAX), (-1, u64::MAX)]).unwrap();
    let strategy = Strategy::Candidate { power_seeds: 0, proposals: 90, proposal_seed: 987 };
    let a = solve_physical(&pool, &q, &law, &Metric::Score, None, &unlimited(), &strategy, None, &Default::default())
        .unwrap();
    let b = solve_physical(&pool, &q, &law, &Metric::Score, None, &unlimited(), &strategy, None, &Default::default())
        .unwrap();
    assert_eq!(a.results, b.results);
    assert_eq!(
        (a.completion, a.optimality, a.exit_reason),
        (Completion::TimedOut, Optimality::Heuristic, ExitReason::ProposalLimit)
    );
    assert!(a.stats.cache_hits > 0);
    assert!(a.stats.cache_evictions > 0);
    for result in &a.results {
        let d = pool.deck(result.members, result.snaps, [0, 1, 2, 3, 4]).unwrap();
        let p = expectation::PhysicalDeck { members: d.members, snaps: d.snaps };
        let expected =
            expectation::evaluate_finite(&pool, &p, &q.objective, &law, &(), |_, o, _| Ok(o.final_score as i128))
                .unwrap();
        assert_eq!(result.expected_payoff, Fraction::from(expected.expected_payoff));
        assert!(result.expected_payoff.denominator.parse::<u128>().unwrap() > u64::MAX as u128);
    }
}

#[test]
fn probability_metric_and_safe_atom_bound_match_unpruned_payoff_oracle() {
    let (m, r, o) = fixture(0);
    let pool = Pool::new(&m, &r).unwrap();
    let q = request(o, 7);
    let law = FiniteSeedLaw::new(vec![(1, 1), (-1, 3), (42, 2)]).unwrap();
    let scored = expectation::oracle(&pool, &q, &law).unwrap();
    let threshold = scored.results[0].evaluation.outcomes.iter().map(|o| o.final_score).max().unwrap();
    let oracle = expectation::oracle_with_payoff_factory(
        &pool,
        &q,
        &law,
        || Ok(()),
        |_, o, _| Ok(i128::from(o.final_score >= threshold)),
    )
    .unwrap();
    let out = solve_physical(
        &pool,
        &q,
        &law,
        &Metric::ScoreAtLeast { threshold },
        None,
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &Default::default(),
    )
    .unwrap();
    for (a, b) in out.results.iter().zip(oracle.results) {
        assert_eq!((a.members, a.snaps, a.power), (b.members, b.snaps, b.power));
        assert_eq!(a.expected_payoff, Fraction::from(b.evaluation.expected_payoff));
    }
    assert_eq!(out.completion, Completion::Complete);
}

#[test]
fn errors_and_no_feasible_decks_are_not_inferred_defaults() {
    let (m, r, o) = fixture(0);
    let pool = Pool::new(&m, &r).unwrap();
    let law = FiniteSeedLaw::new(vec![(1, 1)]).unwrap();
    for k in [0, MAX_K + 1, usize::MAX] {
        assert!(matches!(
            solve_physical(
                &pool,
                &request(o.clone(), k),
                &law,
                &Metric::Score,
                None,
                &unlimited(),
                &Strategy::Exhaustive,
                None,
                &Default::default()
            ),
            Err(Error::Input(_))
        ));
    }
    let mut q = request(o.clone(), 1);
    q.constraints.include_members = vec![1, 2, 3, 4, 5];
    let out = solve_physical(
        &pool,
        &q,
        &law,
        &Metric::Score,
        None,
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(out.stats.evaluated, 24);
    if let Objective::LiveScore { play: PlayInput::Stream { stream, .. }, .. } = &mut q.objective {
        stream.judged.pop();
    }
    assert!(matches!(
        solve_physical(
            &pool,
            &q,
            &law,
            &Metric::Score,
            None,
            &unlimited(),
            &Strategy::Exhaustive,
            None,
            &Default::default()
        ),
        Err(Error::Input(_))
    ));
    let f = Fraction::from(ExactExpectation { numerator: i128::MAX, denominator: u128::MAX });
    let json = serde_json::to_value(f).unwrap();
    assert!(json["numerator"].is_string() && json["denominator"].is_string());
}

#[test]
fn explicit_finite_roots_own_legacy_stream_seed() {
    let (m, r, mut o) = fixture(0);
    let pool = Pool::new(&m, &r).unwrap();
    let law = FiniteSeedLaw::new(vec![(1, 1), (-1, 1)]).unwrap();
    let a = solve_physical(
        &pool,
        &request(o.clone(), 2),
        &law,
        &Metric::Score,
        None,
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &Default::default(),
    )
    .unwrap();
    if let Objective::LiveScore { play: PlayInput::Stream { stream, .. }, .. } = &mut o {
        stream.base_seed = 12345;
    }
    let b = solve_physical(
        &pool,
        &request(o, 2),
        &law,
        &Metric::Score,
        None,
        &unlimited(),
        &Strategy::Exhaustive,
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(a.results, b.results);
}
