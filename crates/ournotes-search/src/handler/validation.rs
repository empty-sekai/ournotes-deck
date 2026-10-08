//! Semantic and capacity validation performed before any solver runs.
use super::*;
use ournotes_sim::live::skip::is_judgement_note;
use ournotes_sim::scenario::Scenario;
use std::collections::HashSet;

pub(crate) fn validate(k: usize, limits: &Limits, strategy: &Strategy) -> Result<(), Error> {
    if !(1..=MAX_K).contains(&k) {
        return Err(Error::Input(format!("k must be in 1..={MAX_K}")));
    }
    if limits.cache_entries > 100_000 {
        return Err(Error::Capacity("cacheEntries exceeds 100000".into()));
    }
    if let Strategy::Candidate { power_seeds, proposals, .. } = strategy
        && (*power_seeds > 128 || *proposals > 10_000_000)
    {
        return Err(Error::Capacity("candidate powerSeeds<=128 and proposals<=10000000 required".into()));
    }
    Ok(())
}

pub(crate) fn validate_payoff(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    input: Option<&EventPayoffInput>,
) -> Result<(), Error> {
    if metric.target().is_some_and(|threshold| threshold <= 0) {
        return Err(Error::Input("score target must be positive".into()));
    }
    if let Metric::ScoreAndLifeAtLeast { min_final_life, .. } = metric {
        if *min_final_life <= 0 {
            return Err(Error::Input("minFinalLife must be positive".into()));
        }
        if !matches!(request.objective.inner(), Objective::LiveScore { .. }) {
            return Err(Error::Input("scoreAndLifeAtLeast requires played Live".into()));
        }
    }
    if let Some(id) = metric.event() {
        let ctx = request
            .objective
            .context()
            .ok_or_else(|| Error::Input("event metrics require resolved scenario".into()))?;
        let input = input.ok_or_else(|| Error::Input("event metrics require context.eventPayoff".into()))?;
        ctx.event_request(pool.master, input, id)?;
        if matches!(metric, Metric::ClientChallengePoints { .. }) && matches!(ctx.scenario, Scenario::Challenge(_)) {
            return Err(Error::Input(
                "challenge-point earnings require an ordinary played/skip result; challenge Live spends points".into(),
            ));
        }
        if matches!(metric, Metric::ConditionalClientEventItems { .. }) && input.selected_rewards.is_none() {
            return Err(Error::Unsupported(
                "UnknownServerAuthority: selectedRewards are required for conditional items".into(),
            ));
        }
    }
    match (request.objective.inner(), metric) {
        (Objective::Power { .. }, Metric::Power)
        | (
            Objective::SkipScore { .. } | Objective::LiveScore { .. },
            Metric::Score
            | Metric::ScoreAtLeast { .. }
            | Metric::CappedScore { .. }
            | Metric::ScoreAndLifeAtLeast { .. }
            | Metric::ClientEventPoints { .. }
            | Metric::ClientChallengePoints { .. }
            | Metric::ConditionalClientEventItems { .. },
        ) => Ok(()),
        _ => Err(Error::Input("metric does not match execution".into())),
    }
}

pub(crate) fn goal_description(r: &RecommendationRequest) -> Result<GoalDescription, Error> {
    let inferred = match (&r.execution, &r.metric) {
        (Execution::Power { .. }, _) => PlayerGoal::Power,
        (
            _,
            Metric::ClientEventPoints { .. }
            | Metric::ClientChallengePoints { .. }
            | Metric::ConditionalClientEventItems { .. },
        ) => PlayerGoal::EventFarming,
        (_, Metric::ScoreAtLeast { .. } | Metric::CappedScore { .. } | Metric::ScoreAndLifeAtLeast { .. }) => {
            PlayerGoal::StableTarget
        }
        (Execution::Skip { .. }, _) => PlayerGoal::SkipFarming,
        (Execution::Live { gekisou: true, .. }, _) => PlayerGoal::GekisouScore,
        _ => PlayerGoal::DailyHighScore,
    };
    let goal = r.goal.unwrap_or(inferred);
    let valid = match goal {
        PlayerGoal::Power => matches!((&r.execution, &r.metric), (Execution::Power { .. }, Metric::Power)),
        PlayerGoal::DailyHighScore => matches!((&r.execution, &r.metric), (Execution::Live { .. }, Metric::Score)),
        PlayerGoal::StableTarget => r.metric.target().is_some(),
        PlayerGoal::EventFarming => r.metric.event().is_some(),
        PlayerGoal::SkipFarming => matches!(r.execution, Execution::Skip { .. }),
        PlayerGoal::GekisouScore => {
            matches!(r.execution, Execution::Live { gekisou: true, .. })
                && matches!(r.metric, Metric::Score | Metric::CappedScore { .. } | Metric::ScoreAtLeast { .. })
        }
    };
    if !valid {
        return Err(Error::Input("player goal does not match the executable action and payoff metric".into()));
    }
    if matches!(r.metric, Metric::ScoreAndLifeAtLeast { .. })
        && !matches!(&r.execution, Execution::Live { play: PlayPolicy::Stream { .. }, .. })
    {
        return Err(Error::Input(
            "scoreAndLifeAtLeast requires an explicit complete judgement stream; theoreticalBest cannot model practical life risk".into(),
        ));
    }
    let title = match goal {
        PlayerGoal::DailyHighScore => "日常冲分",
        PlayerGoal::StableTarget => "稳定达到目标",
        PlayerGoal::EventFarming => "活动收益",
        PlayerGoal::SkipFarming => "跳过刷取",
        PlayerGoal::GekisouScore => "撃奏冲分",
        PlayerGoal::Power => "综合力",
    };
    let payoff_meaning = match r.metric {
        Metric::Power => "maximize current-progression deck power",
        Metric::Score => "maximize the expected final score over the random performance order under the declared play",
        Metric::ScoreAtLeast { .. } => {
            "maximize the probability of reaching the score target over the random performance order"
        }
        Metric::CappedScore { .. } => "maximize E(min(final score, target)); score above target adds no utility",
        Metric::ScoreAndLifeAtLeast { .. } => {
            "maximize probability of score >= target AND terminal life >= minFinalLife; not native clear/failure probability"
        }
        Metric::ClientEventPoints { .. } => "maximize expected client event-point preview per declared consumption",
        Metric::ClientChallengePoints { .. } => {
            "maximize expected newly earned client challenge points per declared Live Boost consumption; no deck event-point bonus"
        }
        Metric::ConditionalClientEventItems { .. } => {
            "maximize expected selected resource quantity conditional on explicitly supplied server rewards"
        }
    };
    let payoff_meaning = if r.aggregation == Aggregation::Maximum {
        match r.metric {
            Metric::Power => "maximize current-progression deck power",
            Metric::Score => {
                "maximize reachable final score over performance orders and positive-probability random outcomes"
            }
            Metric::ScoreAtLeast { .. } => "maximize the reachable score-target indicator",
            Metric::CappedScore { .. } => "maximize reachable final score capped at the declared target",
            Metric::ScoreAndLifeAtLeast { .. } => {
                "maximize the reachable joint score and terminal-life target indicator"
            }
            Metric::ClientEventPoints { .. } => {
                "maximize reachable client event-point preview per declared consumption"
            }
            Metric::ClientChallengePoints { .. } => {
                "maximize reachable newly earned client challenge points per declared consumption"
            }
            Metric::ConditionalClientEventItems { .. } => {
                "maximize reachable selected resource quantity under supplied server rewards"
            }
        }
    } else {
        payoff_meaning
    };
    let mut assumptions = vec!["supplied roster progression and player bonuses; no upgrades or costs inferred"];
    if let Execution::Live { play, .. } = &r.execution {
        assumptions.push(match play {
            PlayPolicy::TheoreticalBest => "chosen theoretical AP/Just play; not predicted player performance",
            PlayPolicy::Accuracy(_) => "chosen theoretical play with declared Great/Just shares spread evenly; not predicted player performance",
            PlayPolicy::Stream { .. } => {
                "complete declared judgement stream; touch timing and human error distribution not inferred"
            }
        });
        if r.aggregation == Aggregation::Maximum {
            assumptions.push("maximize over all performance orders with each Snap paired to its member, and independent native draw support; the stream base seed is not a constraint and whole-sequence root-seed realizability is not established");
        } else {
            assumptions.push("the five members perform in a uniformly random order; paired snaps follow their members");
            assumptions.push(
                "native lottery probability law; certified intervals remain explicit until sufficient to prove ranking",
            );
        }
    }
    if r.metric.event().is_some() {
        assumptions
            .push("client counters under explicit clocks, consumption and peer inputs; no server award authority");
    }
    if r.network_confirmations.is_some() {
        assumptions.push("conditional immutable peer rank arrival timeline with native network score snapshots; frame-zero arrivals apply on range completion, not opponent placement prediction");
    }
    if matches!(r.metric, Metric::ScoreAndLifeAtLeast { .. }) {
        assumptions
            .push("terminal life does not prove survival throughout the live or the failure/continue/quit route");
    }
    Ok(GoalDescription { kind: goal, title, payoff_meaning, assumptions })
}
pub(crate) fn validate_play(
    pool: &Pool,
    request: &SearchRequest,
    network: Option<&[RankConfirmation]>,
    sim: &SimulationInput,
) -> Result<(), Error> {
    let Objective::LiveScore {
        chart,
        play: PlayInput::Stream { stream, judgement_types },
        exclude_snap_skills: false,
        gekisou,
        ..
    } = request.objective.inner()
    else {
        return Err(Error::Input("production expectation requires whole-live Stream including snap skills".into()));
    };
    let setup = crate::search::full_setup(pool, &request.objective)?
        .ok_or_else(|| Error::Input("whole-live setup missing".into()))?;
    if stream.frames.is_empty() {
        return Err(Error::Input("complete play requires nonempty frames".into()));
    }
    let expected: HashSet<_> = chart.notes.iter().filter(|n| is_judgement_note(n.note_type)).map(|n| n.id).collect();
    let judged: HashSet<_> = stream.judged.iter().map(|r| r[1]).collect();
    if !expected.is_subset(&judged) || judged.len() != stream.judged.len() {
        return Err(Error::Input(
            "complete play must include every judged chart note exactly once, including explicit Miss results".into(),
        ));
    }
    let last =
        chart.notes.iter().map(|n| n.time_ms).chain(chart.skill_events.iter().map(|s| s.time_ms)).max().unwrap_or(0);
    if stream.frames.last().copied().unwrap_or(-1) < last {
        return Err(Error::Input("complete clock ends before chart notes/events".into()));
    }
    if sim.music_length_ms.is_some_and(|v| v <= 0)
        || sim.score_music_length_ms.is_some_and(|v| v <= 0)
        || sim.live_finished_from_frame.is_some_and(|v| v >= stream.frames.len())
    {
        return Err(Error::Input("invalid explicit simulation lengths/lifecycle frame".into()));
    }
    if let Some(ctx) = request.objective.context() {
        if matches!(ctx.scenario, Scenario::Mission(_) | Scenario::Battle(_) | Scenario::Arena(_)) && gekisou.is_none()
        {
            return Err(Error::Game(
                "native Mission/Battle/Arena force Gekisou on (1.0.1-25 GetIsGekisouEnabled)".into(),
            ));
        }
        if matches!(ctx.scenario, Scenario::Battle(_) | Scenario::Arena(_)) && network.is_none() {
            return Err(Error::Unsupported(
                "Battle/Arena Gekisou require explicit networkConfirmations; Solo rank-1 substitution is invalid"
                    .into(),
            ));
        }
        if !matches!(ctx.scenario, Scenario::Battle(_) | Scenario::Arena(_)) && network.is_some() {
            return Err(Error::Input("external confirmations are only the declared Battle/Arena adapter".into()));
        }
    } else if network.is_some() {
        return Err(Error::Input("network confirmations require explicit Battle/Arena scenario".into()));
    }
    if let Some(cs) = network {
        let g = setup.gk.as_ref().ok_or_else(|| Error::Input("confirmations require Gekisou".into()))?;
        if g.setup.fevers.len() > 3 {
            return Err(Error::Game("native network ranking has at most three Gekisou ranges".into()));
        }
        let missions: [i64; 3] =
            g.setup.missions.clone().try_into().map_err(|_| Error::Input("three missions required".into()))?;
        let factors = ournotes_sim::live::full::gekisou_rank_factors(pool.master, &missions)?;
        let mut seen = HashSet::new();
        for c in cs {
            if c.frame >= stream.frames.len()
                || c.range >= g.setup.fevers.len()
                || !(1..=5).contains(&c.rank)
                || !seen.insert(c.range)
                || c.percent != factors[c.range][c.rank as usize - 1]
            {
                return Err(Error::Input(
                    "network confirmation requires unique known range, valid frame/rank, and matching master percent"
                        .into(),
                ));
            }
        }
        if seen.len() != g.setup.fevers.len() {
            return Err(Error::Input("complete network play requires one confirmation per fever range".into()));
        }
    }
    // Keep this check explicit even if all constraints eliminate decks.
    if judgement_types.len() != chart.notes.len() {
        return Err(Error::Input("chart judgement type count differs".into()));
    }
    Ok(())
}

// Cancellation occurs at complete performance-order boundaries;
// synchronous Worker termination is the hard cancellation mechanism.
pub(crate) fn reject_unsupported_lifecycle(
    _network: Option<&[RankConfirmation]>,
    finished: Option<usize>,
) -> Result<(), Error> {
    if finished.is_some() {
        return Err(Error::Unsupported(
            "explicit liveFinishedFromFrame requires separately verified lifecycle semantics".into(),
        ));
    }
    Ok(())
}
