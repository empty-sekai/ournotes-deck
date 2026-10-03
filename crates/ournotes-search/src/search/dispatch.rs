//! Solver routing for a built problem. Fixed evaluation uses the same physical leaf path.
use super::physical::{score_summary, solve_physical_impl};
use super::telemetry::{Phase, Telemetry, Traversal};
use super::{
    Completion,
    expectation::{ExactExpectation, PhysicalDeck},
};
use crate::clock::Instant;
use crate::types::*;
use ournotes_sim::Error;
use std::{collections::BTreeMap, time::Duration};

/// Search a frozen problem. Each call gets fresh budget/frontier/Top-K state.
/// The deadline starts here; engine::recommend additionally includes construction time.
pub fn recommend_built(built: &crate::handler::BuiltProblem<'_>) -> Result<RecommendationOutcome, Error> {
    execute(built, None, Instant::now(), 0.0)
}

pub(crate) fn execute(
    built: &crate::handler::BuiltProblem<'_>,
    fixed: Option<([i64; 5], [Option<i64>; 5])>,
    start: Instant,
    build_ms: f64,
) -> Result<RecommendationOutcome, Error> {
    let pool = &built.pool;
    let ctx = &built.context;
    let r = &ctx.spec;
    let request = &ctx.request;
    let law = &ctx.law;
    let context_input = &ctx.context_input;
    let mut limits = r.limits.clone();
    if let Some(ms) = limits.time_limit_ms {
        limits.time_limit_ms = Some(ms.saturating_sub(start.elapsed().as_millis().min(u64::MAX as u128) as u64));
    }
    let mut out = if let Some((members, snaps)) = fixed {
        let deck = pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
        let physical = PhysicalDeck { members: deck.members, snaps: deck.snaps };
        solve_physical_impl(
            pool,
            request,
            law,
            &r.metric,
            context_input.event_payoff.as_ref(),
            &limits,
            &Strategy::Exhaustive,
            r.network_confirmations.as_deref(),
            &r.simulation,
            Some(physical),
            Some(&ctx.plan),
            start,
        )?
    } else if ctx.route == crate::handler::SolverRoute::CanonicalPowerSkip {
        // Skip score is f(power) with f monotone in the checked domain. Both
        // g(s)=1[s>=target] and g(s)=min(s,target) are monotone. Ranking by
        // (g(f(power)), power) therefore has EXACTLY the same order as power:
        // distinct powers break every score plateau, and equal power has equal
        // payoff and the same canonical identity keys. The existing skip
        // search's (f(power), power) order is consequently the same order too.
        // This reduction must not be applied to event bonuses or played skills.
        let mut exact = request.clone();
        exact.time_limit = limits.time_limit_ms.map(Duration::from_millis);
        let search_start = Instant::now();
        let s = super::search(pool, &exact)?;
        let complete = s.completion == Completion::Complete;
        let payoffs: Vec<i128> = s
            .results
            .iter()
            .map(|d| match r.metric {
                Metric::ScoreAtLeast { threshold } => i128::from(d.score.expect("skip score") >= threshold),
                Metric::CappedScore { threshold } => d.score.expect("skip score").min(threshold) as i128,
                _ => d.score.unwrap_or(d.power) as i128,
            })
            .collect();
        let mut telemetry = Telemetry::default();
        let env = &mut telemetry.environment;
        env.traversal = Traversal::Canonical;
        env.k = request.k;
        env.time_limit_ms = limits.time_limit_ms;
        env.max_candidates = limits.max_candidates;
        env.cache_entries = limits.cache_entries;
        telemetry.nodes = s.stats.nodes;
        telemetry.leaves.visited = s.stats.leaves;
        telemetry.leaves.evaluated = s.stats.matchings;
        telemetry.leaves.peak_retained = s.results.len();
        let search_ms = (s.elapsed - s.verify_elapsed).as_secs_f64() * 1000.0;
        let offset = search_start.saturating_duration_since(start).as_secs_f64() * 1000.0;
        let searched = Phase { nodes: s.stats.nodes, candidates: s.stats.leaves, ..phase("search", offset, search_ms) };
        telemetry.phases = vec![searched, phase("verify", offset + search_ms, s.verify_elapsed.as_secs_f64() * 1000.0)];
        let proof = &mut telemetry.proof;
        (proof.complete, proof.parts, proof.parts_done) = (complete, 1, u64::from(complete));
        proof.fraction = complete.then_some(1.0);
        proof.best = payoffs.first().map(i128::to_string);
        proof.kth = (payoffs.len() == request.k).then(|| payoffs[request.k - 1].to_string());
        RecommendationOutcome {
            format: RESULT_FORMAT,
            completion: s.completion,
            optimality: if complete { Optimality::Proven } else { Optimality::Unproven },
            exit_reason: if complete { ExitReason::Exhausted } else { ExitReason::TimeLimit },
            result_identity: "canonicalMemberSet",
            metric: r.metric.clone(),
            player_goal: None,
            strategy: r.strategy.clone(),
            probability_law: serde_json::json!({"kind":"deterministic"}),
            proof_scope: "exact canonical member-set TopK under existing proven nonnegative nonoverflow domain; maxCandidates applies to physical search, not legacy branch search",
            resolved_context: serde_json::Value::Null,
            results: s
                .results
                .into_iter()
                .zip(payoffs)
                .map(|(d, payoff)| {
                    let summary = d
                        .score
                        .map(|score| score_summary(&BTreeMap::from([(score, 1)]), r.metric.target()))
                        .transpose()?;
                    Ok(RecommendedDeck {
                        members: d.members,
                        snaps: d.snaps,
                        power: d.power,
                        expected_score: d
                            .score
                            .map(|n| ExactExpectation { numerator: n as i128, denominator: 1 }.into()),
                        expected_payoff: ExactExpectation { numerator: payoff, denominator: 1 }.into(),
                        score_summary: summary,
                        atoms: Vec::new(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
            telemetry,
            elapsed_ms: s.elapsed.as_secs_f64() * 1000.0,
        }
    } else {
        solve_physical_impl(
            pool,
            request,
            law,
            &r.metric,
            context_input.event_payoff.as_ref(),
            &limits,
            &r.strategy,
            r.network_confirmations.as_deref(),
            &r.simulation,
            None,
            Some(&ctx.plan),
            start,
        )?
    };
    if let Some(l) = &r.seed_law {
        out.probability_law["provenance"] = serde_json::Value::String(l.provenance.clone());
    }
    if fixed.is_some() {
        out.proof_scope = "evaluation of the requested physical deck under declared inputs only; no deck-search optimality, account completeness or latest-native certification is implied";
        if matches!(r.execution, Execution::Power { .. }) {
            for deck in &mut out.results {
                deck.expected_score = None;
                deck.score_summary = None;
                deck.atoms.clear();
            }
        }
    }
    out.player_goal = Some(ctx.player_goal.clone());
    out.resolved_context = ctx.resolved_context.clone();
    let env = &mut out.telemetry.environment;
    env.route = Some(ctx.route);
    env.data = Some(ctx.data.clone());
    if build_ms > 0.0 {
        // This call built the problem: its construction precedes the search phases.
        let built_phases = if matches!(r.strategy, Strategy::BranchAndBound) {
            let at = ctx.plan.bound_compile_started.saturating_duration_since(start).as_secs_f64() * 1000.0;
            vec![phase("prepare", 0.0, at), phase("boundCompile", at, ctx.plan.bound_compile_ms)]
        } else {
            vec![phase("prepare", 0.0, build_ms)]
        };
        out.telemetry.phases.splice(0..0, built_phases);
    }
    out.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(out)
}

fn phase(name: &'static str, start_ms: f64, wall_ms: f64) -> Phase {
    Phase { name, label: None, start_ms, wall_ms, nodes: 0, candidates: 0, simulations: 0 }
}
