//! Largest payoff each term of a metric can pay, used to fold a combined metric and to bound its terms.

use super::{Pool, SearchRequest};
use crate::domain::CandidateDomain;
use crate::search::bonus::{card_bonus, point_product};
use crate::types::Metric;
use ournotes_sim::Error;
use ournotes_sim::scenario::EventPayoffInput;

/// At least the payoff of one terminal result of any team of the domain under a term of a combined metric: one for
/// a score target met or missed, the threshold of a capped score, the largest row of an event reward table, else
/// the largest Int32.
pub(crate) fn term_cap(
    pool: &Pool,
    request: &SearchRequest,
    domain: &CandidateDomain,
    metric: &Metric,
    input: Option<&EventPayoffInput>,
) -> i128 {
    match metric {
        Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. } => 1,
        Metric::CappedScore { threshold } => i128::from(*threshold),
        _ => event_cap(pool, request, domain, metric, input).unwrap_or(i128::from(i32::MAX)),
    }
}

/// The largest row of an event metric's reward table on the request's result route, played or skipped, at the
/// route's rate and the largest event bonus of a team of the domain. None for another metric and for a product
/// outside Int32.
fn event_cap(
    pool: &Pool,
    request: &SearchRequest,
    domain: &CandidateDomain,
    metric: &Metric,
    input: Option<&EventPayoffInput>,
) -> Option<i128> {
    use ournotes_sim::event::{self, EventCard, EventResultRoute};

    let event_id = metric.event()?;
    let master = pool.master;
    let q = request.objective.context()?.event_request(master, input?, event_id).ok()?;
    let ev = master.event(event_id)?;
    let challenge =
        matches!(q.route, EventResultRoute::ChallengePlayed { .. } | EventResultRoute::ChallengeSkip { .. });
    let consumed = i64::from(q.consumed_count);
    let rates =
        if challenge { event::challenge_point_bonus(master, consumed) } else { event::boost_bonus(master, consumed) }
            .ok()?;
    let (value, rate, effect) = match *metric {
        Metric::ClientEventPoints { .. } => {
            let (rows, group) = if challenge {
                (&master.challenge_live_event_points, ev.challenge_live_event_point_group)
            } else {
                (&master.live_event_points, ev.live_event_point_group)
            };
            (
                rows.iter().filter(|row| row.group == group).map(|row| row.value).max(),
                rates[4],
                Some(event::EVENT_POINT),
            )
        }
        Metric::ClientChallengePoints { .. } => {
            (master.live_challenge_points.iter().map(|row| row.value).max(), rates[4], None)
        }
        Metric::RankedEventItems { resource_type, resource_id, .. } => {
            let (rows, group) = if challenge {
                (&master.challenge_live_event_rewards, ev.challenge_live_event_reward_group)
            } else {
                (&master.live_event_rewards, ev.live_event_reward_group)
            };
            let counts = rows
                .iter()
                .filter(|row| {
                    row.event_group == group && row.resource_type == resource_type && row.resource_id == resource_id
                })
                .map(|row| row.resource_count);
            (counts.max(), rates[0], Some(event::EVENT_ITEM))
        }
        _ => return None,
    };
    let bonus = match effect {
        Some(kind) => {
            let effects = event::event_effects(master, event_id);
            let members = domain
                .members()
                .iter()
                .map(|&m| {
                    let card = ournotes_sim::bonus::event_member(master, &pool.members[m]);
                    Ok((pool.members[m].character_id, card_bonus(&effects, EventCard::Member(&card), kind)?))
                })
                .collect::<Result<Vec<_>, Error>>()
                .ok()?;
            let snaps = domain
                .snaps()
                .iter()
                .map(|&s| card_bonus(&effects, EventCard::Snap(&ournotes_sim::bonus::event_snap(&pool.snaps[s])), kind))
                .collect::<Result<Vec<_>, Error>>()
                .ok()?;
            crate::search::bonus::largest_team_bonus(members, &snaps)
        }
        None => 0,
    };
    point_product(value.unwrap_or(0).max(0), bonus, rate).ok()
}

/// Whether no negative local score has a native result: the local score -1 has no rank (a solo score below every
/// threshold is a game error), or its room total takes NONE and `paid` holds for no NONE reward. Ranks walk the
/// thresholds a score or total reaches, so every smaller score takes no higher rank, and NONE is the least. Settlement
/// then reports a Game error for every negative score, and every team with a result scores at least zero in every
/// play, whatever its factor margin. False when the request maps no local score to a total.
pub(crate) fn negative_scores_unpaid(
    pool: &Pool,
    context: &ournotes_sim::scenario::ResolvedContext,
    input: &EventPayoffInput,
    paid: impl Fn(i64) -> bool,
) -> bool {
    use ournotes_sim::event;
    use ournotes_sim::scenario::Scenario;
    let Some(music) = pool.master.live_music(context.resolved.live_music_id) else { return false };
    let group = music.live_score_rank_group;
    if !matches!(context.scenario, Scenario::Battle(_) | Scenario::Arena(_)) {
        return event::score_rank(pool.master, group, -1).is_err();
    }
    input.multiplayer_score_policy.as_ref().and_then(|policy| policy.total_and_count(-1).ok()).is_some_and(
        |(total, players)| {
            event::battle_score_rank(pool.master, group, i64::from(total), players) == event::RANK_NONE
                && !paid(event::RANK_NONE)
        },
    )
}
