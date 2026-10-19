//! Payoffs and score summaries of evaluated decks, and the evaluation of a declared context.

use super::*;
use crate::search::reward::arithmetic;

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
