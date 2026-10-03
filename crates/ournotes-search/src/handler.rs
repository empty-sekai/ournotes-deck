//! Build an immutable search problem from one dataset, roster and explicit goal.
//! No search, candidate truncation or gameplay simulation runs during construction.
use crate::search::expectation::{self, FiniteSeedLaw};
use crate::search::{GekisouObjective, Objective, PlayInput, SearchRequest, SeedSet};
use crate::types::*;
use ournotes_sim::pool::Pool;
use ournotes_sim::{
    Error,
    cards::{Roster, SongView},
    data::DeckData,
};
mod validation;
use crate::domain::CandidateDomain;
use ournotes_sim::live::model::{JudgementStream, JustRule};
use ournotes_sim::replay::RankConfirmation;
use ournotes_sim::scenario::{ContextInput, EventPayoffInput, PowerSnapshotInput};
use validation::{goal_description, validate_payoff, validate_play};
pub(crate) use validation::{reject_unsupported_lifecycle, validate};

/// The algorithm route selected by the objective, never by an arbitrary pool-size cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SolverRoute {
    CanonicalPowerSkip,
    PhysicalExhaustive,
    PhysicalBranchAndBound,
    PhysicalCandidate,
}

/// Frozen semantic inputs shared by search and fixed-deck evaluation.
pub struct SearchContext {
    pub(crate) request: SearchRequest,
    pub(crate) law: FiniteSeedLaw,
    pub(crate) context_input: ContextInput,
    pub(crate) player_goal: GoalDescription,
    pub(crate) resolved_context: serde_json::Value,
    pub(crate) spec: RecommendationRequest,
    pub(crate) route: SolverRoute,
    pub(crate) plan: ExecutionPlan,
    pub(crate) data: crate::search::telemetry::DataIdentity,
}
impl SearchContext {
    /// Exact request snapshot. Mutating the caller's original cannot change this problem.
    pub fn request(&self) -> &RecommendationRequest {
        &self.spec
    }
    pub fn route(&self) -> SolverRoute {
        self.route
    }
    pub fn seed_law(&self) -> &FiniteSeedLaw {
        &self.law
    }
    pub fn resolved_context(&self) -> &serde_json::Value {
        &self.resolved_context
    }
}

/// Built once, borrowed for any number of fresh searches/evaluations.
/// Its lifetime prevents the source dataset changing under a running search.
pub struct BuiltProblem<'m> {
    pub(crate) pool: Pool<'m>,
    pub(crate) context: SearchContext,
}
impl<'m> BuiltProblem<'m> {
    pub fn pool(&self) -> &Pool<'m> {
        &self.pool
    }
    pub fn context(&self) -> &SearchContext {
        &self.context
    }
    pub fn domain(&self) -> &CandidateDomain {
        &self.context.plan.domain
    }
}

pub(crate) struct ExecutionPlan {
    pub(crate) joint: Option<crate::search::joint::JointBounds>,
    /// When the bound compile began, and its duration (zero without branch-and-bound).
    pub(crate) bound_compile_started: crate::clock::Instant,
    pub(crate) bound_compile_ms: f64,
    pub(crate) bound_fallback: Option<String>,
    pub(crate) domain: CandidateDomain,
    pub(crate) song: Option<SongView>,
    pub(crate) event: bool,
    pub(crate) skip: Option<ournotes_sim::live::skip::SkipEvaluator>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_execution(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
    strategy: &Strategy,
) -> Result<ExecutionPlan, Error> {
    reject_unsupported_lifecycle(network, simulation.live_finished_from_frame)?;
    let (song, event, skip) = crate::search::objective_song(pool, &request.objective)?;
    validate_payoff(pool, request, metric, event_input)?;
    if matches!(request.objective.inner(), Objective::LiveScore { .. }) {
        validate_play(pool, request, network, simulation)?;
    } else if network.is_some()
        || simulation.music_length_ms.is_some()
        || simulation.score_music_length_ms.is_some()
        || simulation.live_finished_from_frame.is_some()
    {
        return Err(Error::Input("simulation/network inputs apply only to played live".into()));
    }
    let domain = CandidateDomain::build(pool, &request.constraints)?;
    let started = crate::clock::Instant::now();
    let (joint, bound_fallback) = if matches!(strategy, Strategy::BranchAndBound) {
        match crate::search::joint::JointBounds::compile(pool, request, &domain, metric, event_input, simulation) {
            Ok(bound) => (Some(bound), None),
            Err(reason) => (None, Some(reason.to_string())),
        }
    } else {
        (None, None)
    };
    let bound_compile_ms =
        if matches!(strategy, Strategy::BranchAndBound) { started.elapsed().as_secs_f64() * 1000.0 } else { 0.0 };
    Ok(ExecutionPlan {
        joint,
        bound_compile_started: started,
        bound_compile_ms,
        bound_fallback,
        domain,
        song,
        event,
        skip: skip.map(|s| s.fast),
    })
}

/// Resolve cultivation, scenario, full Snap skill execution, objective and legal domain.
pub fn build_card_pool<'m>(
    data: &'m DeckData,
    roster: &Roster,
    r: &RecommendationRequest,
) -> Result<BuiltProblem<'m>, Error> {
    // Explicitly unavailable in the unchanged current numerical core.
    reject_unsupported_lifecycle(r.network_confirmations.as_deref(), r.simulation.live_finished_from_frame)?;

    if r.format != REQUEST_FORMAT {
        return Err(Error::Input(format!("unsupported recommendation format {}", r.format)));
    }
    let player_goal = goal_description(r)?;
    let score_id = match r.execution {
        Execution::Power { .. } => None,
        Execution::Skip { score_id } | Execution::Live { score_id, .. } => Some(score_id),
    };
    if score_id.is_some() && r.scenario.is_none() {
        return Err(Error::Input("skip/live require an explicit scenario".into()));
    }
    let context_input = r.context.clone().unwrap_or(ContextInput {
        power_snapshot: PowerSnapshotInput { event_ids: roster.player.events.clone(), captured_jst_ticks: None },
        result_clock: None,
        event_payoff: None,
    });
    let fevers = score_id.and_then(|i| data.data_chart(i)).map(|c| c.fevers.as_slice()).unwrap_or(&[]);
    let context =
        r.scenario.as_ref().map(|s| context_input.resolve(&data.master, s.scenario(), score_id, fevers)).transpose()?;
    let pool = match &context {
        Some(c) => c.pool(&data.master, roster)?,
        None => {
            let mut frozen = roster.clone();
            frozen.player.events = context_input.power_snapshot.event_ids.clone();
            Pool::new(&data.master, &frozen)?
        }
    };
    let objective = match &r.execution {
        Execution::Power { music_id, event_parameter } => {
            if context.is_some() && *event_parameter {
                return Err(Error::Input("explicit scenario determines event parameters".into()));
            }
            Objective::Power { music_id: *music_id, event: *event_parameter }
        }
        Execution::Skip { score_id } => Objective::SkipScore { score_id: *score_id, chart: data.chart(*score_id)? },
        Execution::Live { score_id, gekisou, play } => {
            let chart = data.chart(*score_id)?;
            let dc = data.data_chart(*score_id).ok_or_else(|| Error::Input("chart is absent".into()))?;
            let ctx = context.as_ref().expect("explicit live scene");
            let stream = match play {
                PlayPolicy::Stream { stream } => stream.clone(),
                PlayPolicy::TheoreticalBest if *gekisou => JudgementStream::theoretical_best_gekisou(
                    &chart,
                    &dc.judgement_types,
                    &JustRule::new(&data.master, &ctx.gekisou)?,
                )?,
                PlayPolicy::TheoreticalBest => JudgementStream::theoretical_best(&chart),
            };
            Objective::LiveScore {
                score_id: *score_id,
                chart,
                play: PlayInput::Stream { stream, judgement_types: dc.judgement_types.clone() },
                event: false,
                exclude_snap_skills: false,
                gekisou: gekisou.then(|| GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: dc.fevers.clone() }),
            }
        }
    };
    let objective = match &context {
        Some(c) => objective.in_scenario(c.clone()),
        None => objective,
    };
    let objective = expectation::normalized_objective(&objective);
    let request = SearchRequest { objective, k: r.k, constraints: r.constraints.clone(), time_limit: None };
    let law = match (&r.execution, &r.seed_law) {
        (Execution::Live { .. }, Some(l)) => {
            if l.provenance.trim().is_empty() {
                return Err(Error::Input("seedLaw.provenance must declare its assumption/source".into()));
            }
            FiniteSeedLaw::new(l.atoms.clone())?
        }
        (Execution::Live { .. }, None) => {
            return Err(Error::Input(
                "live requires explicit positive-mass seedLaw; native TickCount population law is unknown".into(),
            ));
        }
        (_, Some(_)) => return Err(Error::Input("seedLaw applies only to played live".into())),
        _ => FiniteSeedLaw::new(vec![(0, 1)])?,
    };
    validate(r.k, &law, &r.limits, &r.strategy)?;
    validate_payoff(&pool, &request, &r.metric, context_input.event_payoff.as_ref())?;
    let resolved_context = serde_json::json!({"dataProvenance":data.provenance,"scenario":context.as_ref().map(|c|format!("{:?}",c.scenario)),"baseLiveMusicId":context.as_ref().map(|c|c.resolved.live_music_id),"scoreId":score_id,"calcEventParameter":context.as_ref().map(|c|c.resolved.calc_event_parameter),"skillTargetMusicType":context.as_ref().map(|c|c.resolved.skill_target_music_type),"gekisouMissions":context.as_ref().map(|c|c.resolved.gekisou_missions),"context":context_input,"simulation":r.simulation,"networkConfirmations":r.network_confirmations,"playPolicy":match &r.execution {Execution::Live{play:PlayPolicy::TheoreticalBest,..}=>"explicit theoretical AP/Just scenario",Execution::Live{..}=>"declared judgement stream",_=>"deterministic"},"eligibility":"unlock/progression eligibility not inferred","nativeEvidence":"1.0.1-25 AArch64; selected regional master is explicit data, online patch/version parity not inferred"});
    let plan = compile_execution(
        &pool,
        &request,
        &r.metric,
        context_input.event_payoff.as_ref(),
        r.network_confirmations.as_deref(),
        &r.simulation,
        &r.strategy,
    )?;
    let route = match (&r.execution, &r.metric) {
        (Execution::Power { .. }, Metric::Power)
        | (Execution::Skip { .. }, Metric::Score | Metric::ScoreAtLeast { .. } | Metric::CappedScore { .. }) => {
            SolverRoute::CanonicalPowerSkip
        }
        _ if matches!(r.strategy, Strategy::Exhaustive) => SolverRoute::PhysicalExhaustive,
        _ if matches!(r.strategy, Strategy::BranchAndBound) => SolverRoute::PhysicalBranchAndBound,
        _ => SolverRoute::PhysicalCandidate,
    };
    if route == SolverRoute::CanonicalPowerSkip && matches!(r.strategy, Strategy::Candidate { .. }) {
        return Err(Error::Input("power/skip score and monotone score targets use exact canonical search; candidate strategy applies to physical metrics".into()));
    }
    let context = SearchContext {
        request,
        law,
        context_input,
        player_goal,
        resolved_context,
        spec: r.clone(),
        route,
        plan,
        data: crate::search::telemetry::DataIdentity::of(data),
    };
    Ok(BuiltProblem { pool, context })
}
