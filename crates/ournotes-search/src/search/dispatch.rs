//! Solver routing for a built problem. Fixed evaluation uses the same physical leaf path.
use super::expectation::PhysicalDeck;
use super::physical::{ProgressHook, solve_physical_impl};
use super::telemetry::Phase;
use crate::clock::Instant;
use crate::types::*;
use ournotes_sim::Error;

/// Search a frozen problem. Each call gets fresh budget/frontier/Top-K state.
/// The deadline starts here; engine::recommend additionally includes construction time.
pub fn recommend_built(built: &crate::handler::BuiltProblem<'_>) -> Result<RecommendationOutcome, Error> {
    execute(built, None, Instant::now(), 0.0, None)
}

/// `progress` receives formal search reports, completed like the final result. Fixed evaluation makes no reports.
pub(crate) fn execute(
    built: &crate::handler::BuiltProblem<'_>,
    fixed: Option<([i64; 5], [Option<i64>; 5])>,
    start: Instant,
    build_ms: f64,
    progress: Option<ProgressHook<'_>>,
) -> Result<RecommendationOutcome, Error> {
    let pool = &built.pool;
    let ctx = &built.context;
    let r = &ctx.spec;
    let request = &ctx.request;
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
            &r.metric,
            context_input.event_payoff.as_ref(),
            &limits,
            &Strategy::Exhaustive,
            r.network_confirmations.as_deref(),
            &r.simulation,
            Some(physical),
            &[],
            Some(&ctx.plan),
            start,
            None,
        )?
    } else {
        let initial = r
            .initial_decks
            .iter()
            .map(|d| {
                pool.deck(d.members, d.snaps, [0, 1, 2, 3, 4])
                    .map(|d| PhysicalDeck { members: d.members, snaps: d.snaps })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut forward;
        let progress = match progress {
            Some(ProgressHook { interval, report }) => {
                forward = move |mut out: RecommendationOutcome| {
                    finish(&mut out, built, false, start, build_ms);
                    report(out)
                };
                Some(ProgressHook { interval, report: &mut forward })
            }
            None => None,
        };
        solve_physical_impl(
            pool,
            request,
            &r.metric,
            context_input.event_payoff.as_ref(),
            &limits,
            &r.strategy,
            r.network_confirmations.as_deref(),
            &r.simulation,
            None,
            &initial,
            Some(&ctx.plan),
            start,
            progress,
        )?
    };
    finish(&mut out, built, fixed.is_some(), start, build_ms);
    Ok(out)
}

/// The request-level fields of a solver outcome, for the final result and for each progress report.
fn finish(
    out: &mut RecommendationOutcome,
    built: &crate::handler::BuiltProblem<'_>,
    fixed: bool,
    start: Instant,
    build_ms: f64,
) {
    let ctx = &built.context;
    let r = &ctx.spec;
    if fixed {
        out.proof_scope = "evaluation of the requested physical deck under declared inputs only; no deck-search optimality, account completeness or latest-native certification is implied";
        if matches!(r.execution, Execution::Power { .. }) {
            for deck in &mut out.results {
                deck.expected_score = None;
                deck.score_summary = None;
                deck.best_order = None;
            }
        }
    } else if super::team_power::applies(&ctx.request.objective, &r.metric) {
        out.proof_scope = "canonical leader/member-Snap team TopK under the declared deterministic inputs; distinct leaders and Snap bindings remain distinct results, and only Complete certifies their ranks";
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
}

fn phase(name: &'static str, start_ms: f64, wall_ms: f64) -> Phase {
    Phase { name, label: None, start_ms, wall_ms, nodes: 0, candidates: 0, simulations: 0 }
}
