//! The payoff of one terminal outcome of a deck, and the arithmetic that must fit for it to be exact.
use super::expectation::PhysicalDeck;
use super::{Pool, SearchRequest};
use crate::types::Metric;
use ournotes_sim::Error;
use ournotes_sim::scenario::EventPayoffInput;

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
        Metric::Score
        | Metric::ScoreAtLeast { .. }
        | Metric::CappedScore { .. }
        | Metric::ScoreAndLifeAtLeast { .. } => score_payoff(metric, score, final_life),
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

/// The payoff of a score, capped score or score target at one terminal score.
pub(crate) fn score_payoff(metric: &Metric, score: i32, final_life: Option<i32>) -> Result<i128, Error> {
    match *metric {
        Metric::Score => Ok(score as i128),
        Metric::ScoreAtLeast { threshold } => Ok(i128::from(score >= threshold)),
        Metric::CappedScore { threshold } => Ok(score.min(threshold) as i128),
        Metric::ScoreAndLifeAtLeast { threshold, min_final_life } => Ok(i128::from(
            score >= threshold
                && final_life.ok_or_else(|| Error::Input("terminal life requires played Live".into()))?
                    >= min_final_life,
        )),
        _ => Err(Error::Unsupported("not a score payoff".into())),
    }
}

/// The points a step multiplier pays at one event bonus, `(bonus + 10000) * multiplier / 10000`, in the same order
/// a played Live pays it. Callers certify that the native intermediates stay inside the supported range.
pub(crate) fn points_payoff(bonus: i64, multiplier: i64) -> i128 {
    i128::from(multiplier) * i128::from(bonus + 10000) / 10000
}

pub(crate) fn arithmetic() -> Error {
    Error::Domain("finite-law checked exact arithmetic overflow".into())
}
