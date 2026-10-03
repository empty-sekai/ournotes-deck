//! The exhaustive skip event-point oracle: every physical deck is visited, since score and power pruning is
//! invalid for event-point payoffs.

use ournotes_sim::error::Error;
use ournotes_sim::scenario::{EventPayoffInput, item_payoff};

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedSkipEventDeck {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
    pub power: i32,
    pub score: i32,
    pub event_points: i32,
    pub terminal_payoff: i128,
    pub conditional_items: Option<ournotes_sim::event::EventItemPreview>,
    pub preview: ournotes_sim::event::EventPointPreview,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipEventSearchOutcome {
    pub completion: crate::search::Completion,
    pub evaluated: u64,
    pub results: Vec<RankedSkipEventDeck>,
}

/// Exhaustive physical-deck oracle: score/power pruning is invalid for event-point payoffs.
pub fn search_skip_event_points(
    pool: &ournotes_sim::pool::Pool,
    request: &crate::search::SearchRequest,
    input: &EventPayoffInput,
    event_id: i64,
) -> Result<SkipEventSearchOutcome, Error> {
    search_skip_event_payoff(pool, request, input, event_id, None)
}

/// With an item target, rank by the explicitly conditional resource quantity instead of event points.
pub fn search_skip_event_payoff(
    pool: &ournotes_sim::pool::Pool,
    request: &crate::search::SearchRequest,
    input: &EventPayoffInput,
    event_id: i64,
    item_target: Option<(i64, i64)>,
) -> Result<SkipEventSearchOutcome, Error> {
    use crate::search::{Completion, Objective};
    if item_target.is_some() && input.selected_rewards.is_none() {
        return Err(Error::Unsupported(
            "UnknownServerAuthority: selectedRewards are required for a conditional item objective".into(),
        ));
    }
    if !matches!(request.objective.inner(), Objective::SkipScore { .. }) {
        return Err(Error::Input("skip event oracle requires a SkipScore objective".into()));
    }
    let context = request
        .objective
        .context()
        .ok_or_else(|| Error::Input("skip event oracle requires a resolved scenario".into()))?;
    if !matches!(context.result_clock, Some(ournotes_sim::event::EventResultClock::Skip { .. })) {
        return Err(Error::Input("skip event oracle requires a skip resultClock".into()));
    }
    context.event_request(pool.master, input, event_id)?;
    context.validate_pool(pool)?;
    let start = crate::clock::Instant::now();
    let mut out = SkipEventSearchOutcome { completion: Completion::Complete, evaluated: 0, results: Vec::new() };
    crate::search::expectation::visit_physical_decks(pool, &request.constraints, |physical| {
        if request.time_limit.is_some_and(|t| start.elapsed() >= t) {
            out.completion = Completion::TimedOut;
            return Ok(false);
        }
        let (power, score) = crate::search::evaluate(pool, &physical.as_deck(), &request.objective)?;
        let score = score.ok_or_else(|| Error::Input("missing skip score".into()))?;
        let preview = context.preview_event_points(pool, &physical.as_deck(), input, event_id, score)?;
        let conditional_items = item_target
            .map(|_| context.preview_event_items(pool, &physical.as_deck(), input, event_id, score))
            .transpose()?;
        let terminal_payoff = match (item_target, &conditional_items) {
            (Some((ty, id)), Some(items)) => item_payoff(items, event_id, ty, id)?,
            _ => i128::from(preview.points_for(event_id)),
        };
        out.evaluated += 1;
        out.results.push(RankedSkipEventDeck {
            members: physical.members.map(|i| pool.members[i].id),
            snaps: physical.snaps.map(|s| s.map(|i| pool.snaps[i].id)),
            power,
            score,
            event_points: preview.points_for(event_id),
            terminal_payoff,
            conditional_items,
            preview,
        });
        Ok(true)
    })?;
    out.results.sort_by(|a, b| {
        b.terminal_payoff
            .cmp(&a.terminal_payoff)
            .then(b.power.cmp(&a.power))
            .then(a.members.cmp(&b.members))
            .then(a.snaps.cmp(&b.snaps))
    });
    out.results.truncate(request.k);
    Ok(out)
}
