//! Payoffs and score summaries of evaluated decks, and the evaluation of a declared context.

use super::*;

/// The payoff of one outcome of a deck under the metric.
#[allow(clippy::too_many_arguments)]
pub(crate) fn payoff_of(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    p: &PhysicalDeck,
    score: i32,
    power: i32,
    final_life: Option<i32>,
) -> Result<i128, Error> {
    match *metric {
        Metric::Power => Ok(power as i128),
        Metric::Score => Ok(score as i128),
        Metric::ScoreAtLeast { threshold } => Ok(i128::from(score >= threshold)),
        Metric::CappedScore { threshold } => Ok(score.min(threshold) as i128),
        Metric::ScoreAndLifeAtLeast { threshold, min_final_life } => Ok(i128::from(
            score >= threshold
                && final_life.ok_or_else(|| Error::Input("terminal life requires played Live".into()))?
                    >= min_final_life,
        )),
        Metric::ClientEventPoints { event_id } => Ok(request
            .objective
            .context()
            .expect("validated context")
            .preview_event_points(pool, &p.as_deck(), event_input.expect("validated event input"), event_id, score)?
            .points_for(event_id) as i128),
        Metric::ClientChallengePoints { event_id } => Ok(request
            .objective
            .context()
            .expect("validated context")
            .preview_event_points(pool, &p.as_deck(), event_input.expect("validated event input"), event_id, score)?
            .challenge_points_for(event_id) as i128),
        Metric::RankedEventItems { event_id, resource_type, resource_id } => {
            let items = request.objective.context().expect("validated context").preview_event_items(
                pool,
                &p.as_deck(),
                event_input.expect("validated event input"),
                event_id,
                score,
            )?;
            ournotes_sim::scenario::item_payoff(&items, event_id, resource_type, resource_id)
        }
        Metric::Combined { ref levels } => {
            // The folded payoff (`levels.rs`): from the last level up, each level's weighted sum times its
            // multiplier. The levels after the first stay nonnegative and within their caps.
            let mut folded = 0i128;
            for (at, level) in levels.iter().enumerate().rev() {
                let own = level.terms.iter().try_fold(0i128, |sum, term| {
                    payoff_of(pool, request, &term.metric, event_input, p, score, power, final_life)?
                        .checked_mul(i128::from(term.weight))
                        .and_then(|value| sum.checked_add(value))
                        .ok_or_else(arithmetic)
                })?;
                if levels.len() > 1 && level.scale.multiplier == 0 {
                    return Err(Error::Input("the levels of a combined metric are folded by the search".into()));
                }
                folded = own
                    .checked_mul(level.scale.multiplier.max(1))
                    .and_then(|value| value.checked_add(folded))
                    .ok_or_else(arithmetic)?;
                if at > 0 && (own < 0 || folded > level.scale.cap) {
                    return Err(Error::Unsupported(
                        "a later level of the combined metric pays outside its nonnegative payoff range".into(),
                    ));
                }
            }
            Ok(folded)
        }
    }
}

pub(crate) fn arithmetic() -> Error {
    Error::Domain("finite-law checked exact arithmetic overflow".into())
}

/// A score distribution summary preserves integer masses and does not use f64 ranks.
/// Quantile p is the smallest score with cumulative mass >= ceil(p * total mass).
pub fn score_summary(mass: &BTreeMap<i32, u128>, target: Option<i32>) -> Result<ScoreSummary, Error> {
    if mass.is_empty() || mass.values().any(|&w| w == 0) {
        return Err(Error::Input("score summary requires positive nonempty masses".into()));
    }
    let total = mass.values().try_fold(0u128, |a, &w| a.checked_add(w).ok_or_else(arithmetic))?;
    let quantile = |tenth: u128| -> Result<i32, Error> {
        // Dividing first keeps valid u128 totals safe, including u128::MAX.
        let rank = (total / 10)
            .checked_mul(tenth)
            .and_then(|n| n.checked_add(((total % 10) * tenth).div_ceil(10)))
            .ok_or_else(arithmetic)?;
        let mut cumulative = 0u128;
        for (&score, &weight) in mass {
            cumulative = cumulative.checked_add(weight).ok_or_else(arithmetic)?;
            if cumulative >= rank {
                return Ok(score);
            }
        }
        Err(Error::Domain("score quantile exceeds mass".into()))
    };
    let (probability_at_least, expected_shortfall) = if let Some(target) = target {
        let mut success = 0u128;
        let mut shortfall = 0i128;
        for (&score, &weight) in mass {
            if score >= target {
                success = success.checked_add(weight).ok_or_else(arithmetic)?;
            } else {
                shortfall = shortfall
                    .checked_add(
                        (target as i128 - score as i128)
                            .checked_mul(i128::try_from(weight).map_err(|_| arithmetic())?)
                            .ok_or_else(arithmetic)?,
                    )
                    .ok_or_else(arithmetic)?;
            }
        }
        (
            Some(Fraction { numerator: success.to_string(), denominator: total.to_string() }),
            Some(Fraction { numerator: shortfall.to_string(), denominator: total.to_string() }),
        )
    } else {
        (None, None)
    };
    Ok(ScoreSummary {
        minimum: *mass.first_key_value().expect("nonempty").0,
        maximum: *mass.last_key_value().expect("nonempty").0,
        p10: quantile(1)?,
        p50: quantile(5)?,
        p90: quantile(9)?,
        target_score: target,
        probability_at_least,
        expected_shortfall,
    })
}

/// Fixed-deck evaluator over the shared core, exposed for independent checks.
/// Context identity must match. Network arrivals use controller frame snapshots and explicit aggregate ranks.
/// Explicit finished lifecycle remains unsupported.
/// No wall clock is used by this unbounded numeric evaluator.
pub fn evaluate_declared_context(
    master: &ournotes_sim::master::Master,
    physical: &PhysicalDeck,
    input: &FiniteSeedContext,
    root_seed: i32,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
) -> Result<(expectation::ConditionalOutcome, Vec<(usize, usize)>), Error> {
    reject_unsupported_lifecycle(network, simulation.live_finished_from_frame)?;
    if input.physical() != *physical {
        return Err(Error::Input("physical deck differs from declared context".into()));
    }
    let mut input = input.clone();
    if let Some(v) = simulation.music_length_ms {
        if v <= 0 {
            return Err(Error::Input("musicLengthMs must be positive".into()));
        }
        input.params.music_length_ms = v;
    }
    if let Some(v) = simulation.score_music_length_ms {
        if v <= 0 {
            return Err(Error::Input("scoreMusicLengthMs must be positive".into()));
        }
        input.params.score_music_length_ms = Some(v);
    }
    if input.delta_times.len() != input.play.frames.len() {
        return Err(Error::Input("one deltaTime per declared frame required".into()));
    }
    if simulation.live_finished_from_frame.is_some_and(|v| v >= input.play.frames.len()) {
        return Err(Error::Input("lifecycle frame outside declared play".into()));
    }
    if let Some(cs) = network {
        let g = input.gekisou.as_ref().ok_or_else(|| Error::Input("network confirmations require Gekisou".into()))?;
        if g.fevers.len() > 3 {
            return Err(Error::Input("at most three native Gekisou ranges supported".into()));
        }
        let missions: [i64; 3] =
            g.missions.clone().try_into().map_err(|_| Error::Input("three native missions required".into()))?;
        let factors = ournotes_sim::live::full::gekisou_rank_factors(master, &missions)?;
        let mut ranges = HashSet::new();
        for c in cs {
            if c.frame >= input.play.frames.len()
                || c.range >= g.fevers.len().min(3)
                || !(1..=5).contains(&c.rank)
                || !ranges.insert(c.range)
                || c.percent != factors[c.range][c.rank as usize - 1]
            {
                return Err(Error::Input(
                    "invalid aggregate network packet arrival/range/group rank/percentage".into(),
                ));
            }
        }
        if ranges.len() != g.fevers.len() {
            return Err(Error::Input("one aggregate packet per fever required".into()));
        }
    }
    let outcome = simulate_current(&input, master, root_seed, network, simulation.live_finished_from_frame, || false)?
        .ok_or_else(|| Error::Domain("unbounded declared evaluator cancelled unexpectedly".into()))?;
    Ok((outcome.terminal, outcome.network_applications))
}

struct CurrentDeclaredOutcome {
    terminal: expectation::ConditionalOutcome,
    network_applications: Vec<(usize, usize)>,
}

fn simulate_current<F: FnMut() -> bool>(
    input: &FiniteSeedContext,
    master: &ournotes_sim::master::Master,
    root_seed: i32,
    confirmations: Option<&[RankConfirmation]>,
    finished_from_frame: Option<usize>,
    mut cancelled: F,
) -> Result<Option<CurrentDeclaredOutcome>, Error> {
    if finished_from_frame.is_some() {
        return Err(Error::Unsupported("finished lifecycle is unsupported".into()));
    }
    if cancelled() {
        return Ok(None);
    }
    let mut input = input.clone();
    if let Some(confirmations) = confirmations {
        input.rank_confirmations = Some(confirmations.to_vec());
    }
    let terminal = input.simulate(master, root_seed)?;
    if cancelled() {
        return Ok(None);
    }
    let network_applications = terminal.model.rank_confirmation_applications().to_vec();
    Ok(Some(CurrentDeclaredOutcome { terminal, network_applications }))
}
