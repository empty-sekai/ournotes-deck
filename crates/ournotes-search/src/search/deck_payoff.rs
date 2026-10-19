//! Payoffs the deck alone determines: the Skip score, Skip event points, challenge points and ranked items at the
//! configured result rank. Each is a nondecreasing function of the deck's power or of one additive deck sum, the
//! event bonus of its members and Snaps, so the canonical team Top-K needs no Live play: payoff, power, then
//! canonical member and Snap IDs rank every team exactly. The Skip score is a closed form of the power alone; its
//! monotonicity over the compiled power interval is proved by [`TeamPowerBounds`] before a ranking reads it.
//!
//! A played Live's event points, challenge points, ranked items and score targets step with the local score instead
//! (a declared multiplayer room ranks its total, which is nondecreasing in the local score). Every order of a deck
//! scores at most its power times the joint envelope's global coefficient, so the step of that cap bounds the payoff
//! by the deck's bonus and power alone ([`ScoreSteps`]); the search then ranks by this bound and evaluates the decks
//! it ranks first, handing over to the joint traversal as soon as one pays less.
//!
//! Members branch in `SLOTS` order; a member prefix bounds its completions by the best remaining bonuses and the
//! power bound of [`TeamPowerBounds`], which assigns the remaining slots' Snap terms where that is tighter. A complete
//! member layout has one Snap-independent power constant, and its Snap bindings are ranked exactly by
//! [`DeckPayoffBounds::frontier`].
//!
//! A common terminal upper bound can also order the domain by power and canonical identity. Its proposals require
//! terminal evaluation; the upper-only mode does not assign that bound as a deck's actual payoff.
use super::{
    Objective, Pool, SearchRequest,
    bonus::{card_bonus, challenge_product, largest_team_bonus, nonnegative_i32, point_product, sum_rows},
    expectation::PhysicalDeck,
    team_power::{SLOTS, TeamPowerBounds, candidates},
};
use crate::search::bonus::unavailable;
use crate::search::curve::{ScoreSteps, TermSteps};
use crate::{domain::CandidateDomain, types::Metric};
use ournotes_sim::{
    Error,
    event::{self, EventCard, EventResultRoute},
    live::skip::SkipEvaluator,
    scenario::EventPayoffInput,
};
use std::collections::HashMap;

/// The payoff of a deck as a function of its event bonus (10000 = 100 %) and power, nondecreasing in both on the
/// compiled domain.
#[derive(Clone, Debug)]
enum Curve {
    /// Event points at a fixed result rank, `reward * (bonus + 10000) * rate / 10000`.
    Points { reward: i64, rate: i64 },
    /// One exact-grade reward. Its native products are checked even for a different target resource.
    Items { count: i64, rate: i64, counted: bool },
    /// No card changes the payoff.
    Constant(i128),
    /// Every deck pays at most this value; attainment requires terminal evaluation.
    UpperOnly(i128),
    /// A bound of each order of a played Live: the step its score cap reaches.
    Steps(ScoreSteps),
    /// The Skip score at the deck's power: a closed form of the power over the request's own chart. Its
    /// monotonicity over the compiled power interval is certified by [`TeamPowerBounds`] before the bound is used.
    Skip,
}

impl Curve {
    fn value(&self, bonus: i64, power: i64, skip: Option<&SkipEvaluator>) -> Result<i128, Error> {
        match self {
            Self::Points { reward, rate } => point_product(*reward, bonus, *rate),
            Self::Items { count, rate, counted } => {
                let amount = point_product(*count, bonus, *rate)?;
                Ok(if *counted { amount } else { 0 })
            }
            Self::Constant(value) | Self::UpperOnly(value) => Ok(*value),
            Self::Steps(steps) => steps.value(bonus, power),
            Self::Skip => {
                let skip = skip.ok_or_else(|| unavailable("Skip score monotonicity is not certified"))?;
                let power = i32::try_from(power)
                    .ok()
                    .filter(|power| *power >= 0)
                    .ok_or_else(|| unavailable("Skip power outside the certified interval"))?;
                Ok(i128::from(skip.score(power).0))
            }
        }
    }

    /// The largest payoff gain of 10000 units of bonus; it only orders the members a search visits.
    fn slope(&self) -> f64 {
        match self {
            Self::Points { reward, rate } => *reward as f64 * *rate as f64 / 10000.0,
            Self::Items { count, rate, counted } => {
                f64::from(u8::from(*counted)) * *count as f64 * *rate as f64 / 10000.0
            }
            Self::Constant(_) | Self::UpperOnly(_) | Self::Skip => 0.0,
            Self::Steps(steps) => steps.largest_step(),
        }
    }
}

/// The bonus type, effect event and payoff curve of a request's deck-determined payoff, or of the per-order payoff
/// bound of a played Live with these score `steps`.
fn curve(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    input: Option<&EventPayoffInput>,
    steps: Option<ScoreSteps>,
) -> Result<(i64, Option<i64>, Curve), Error> {
    if let Some(steps) = steps {
        return match *metric {
            Metric::ClientEventPoints { event_id } => Ok((event::EVENT_POINT, Some(event_id), Curve::Steps(steps))),
            Metric::RankedEventItems { event_id, .. } => {
                let context = request.objective.context().ok_or_else(|| unavailable("missing resolved context"))?;
                let input = input.ok_or_else(|| unavailable("missing event payoff input"))?;
                let q = context.event_request(pool.master, input, event_id)?;
                let effect = q.holding_event_ids.contains(&event_id).then_some(event_id);
                Ok((event::EVENT_ITEM, effect, Curve::Steps(steps)))
            }
            Metric::ClientChallengePoints { .. } | Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. } => {
                Ok((event::EVENT_POINT, None, Curve::Steps(steps)))
            }
            _ => Err(unavailable("score steps of a payoff that does not step with the score")),
        };
    }
    // A skipped Live has one score: a closed form of the deck's power over the request's chart. It reads no event
    // bonus and needs no event payoff input.
    if matches!(request.objective.inner(), Objective::SkipScore { .. }) && matches!(*metric, Metric::Score) {
        return Ok((event::EVENT_POINT, None, Curve::Skip));
    }
    let input = input.ok_or_else(|| unavailable("missing event payoff input"))?;
    let master = pool.master;
    let context = request.objective.context().ok_or_else(|| unavailable("missing resolved context"))?;
    let skip = matches!(request.objective.inner(), Objective::SkipScore { .. });
    match *metric {
        Metric::ClientEventPoints { event_id } | Metric::ClientChallengePoints { event_id } if skip => {
            let challenge_points = matches!(metric, Metric::ClientChallengePoints { .. });
            let q = context.event_request(master, input, event_id)?;
            let challenge = match q.route {
                EventResultRoute::NormalSkip => false,
                EventResultRoute::ChallengeSkip { event_id: target } if target == event_id => true,
                _ => return Err(unavailable("requires NormalSkip or matching ChallengeSkip")),
            };
            // Native Skip uses Enum.TryParse's result even on failure (NONE = 0).
            // Its EXP parse-success behavior does not change the EP/CP rank.
            let rank = event::skip_result_rank(master)?.rank;
            let rate = if challenge {
                event::challenge_point_bonus(master, i64::from(q.consumed_count))?[4]
            } else {
                event::boost_bonus(master, i64::from(q.consumed_count))?[4]
            };
            nonnegative_i32(rate)?;
            let ev = master.event(event_id).ok_or_else(|| unavailable("missing event"))?;
            let active = q.holding_event_ids.contains(&event_id);
            // Missing rows contribute no PT on Skip (the evaluated deck retains diagnostics).
            let reward = if !active || (challenge_points && challenge) {
                0
            } else if challenge_points {
                // Normal Skip continues before its CP update if the EP row is
                // absent, even when a CP row exists for this rank.
                if event::music_score_event_point(master, ev.live_event_point_group, rank).is_some() {
                    event::music_score_challenge_point(master, rank)
                        .ok_or_else(|| unavailable("missing reachable challenge-point row"))?
                } else {
                    0
                }
            } else if challenge {
                event::challenge_live_event_point(master, ev.challenge_live_event_point_group, rank).unwrap_or(0)
            } else {
                event::music_score_event_point(master, ev.live_event_point_group, rank).unwrap_or(0)
            };
            Ok(if challenge_points {
                (event::EVENT_POINT, None, Curve::Constant(challenge_product(reward, rate)?))
            } else {
                (event::EVENT_POINT, Some(event_id), Curve::Points { reward, rate })
            })
        }
        Metric::RankedEventItems { event_id, resource_type, resource_id } if skip => {
            let q = context.event_request(master, input, event_id)?;
            let challenge = match q.route {
                EventResultRoute::NormalSkip => false,
                EventResultRoute::ChallengeSkip { event_id: selected } if selected == event_id => true,
                _ => return Err(unavailable("ranked Skip items require a matching Skip route")),
            };
            let rank = event::skip_result_rank(master)?.rank;
            if !q.holding_event_ids.contains(&event_id) {
                return Ok((event::EVENT_ITEM, None, Curve::Constant(0)));
            }
            let rate = if challenge {
                event::challenge_point_bonus(master, i64::from(q.consumed_count))?[0]
            } else {
                event::boost_bonus(master, i64::from(q.consumed_count))?[0]
            };
            let row = event::ranked_event_reward(master, event_id, &q.route, rank)?;
            let counted = row.resource_type == resource_type && row.resource_id == resource_id;
            Ok((event::EVENT_ITEM, Some(event_id), Curve::Items { count: row.resource_count, rate, counted }))
        }
        _ => Err(unavailable("requires a fixed-rank Skip payoff or played score steps")),
    }
}

/// The most distinct event bonuses the terms of one payoff read.
const KINDS: usize = 8;

/// An event bonus one or more terms read.
struct Kind {
    member: Vec<i64>,
    /// Per Snap column (`domain.snaps()` order).
    snap: Vec<i64>,
    /// The sum of the five largest Snap bonuses.
    snap_best: i64,
    /// The largest bonus of a legal team.
    maximum: i64,
}

/// One weighted term of the payoff: the curve of the bonus `kinds[kind]` and the power.
struct Term {
    weight: i128,
    kind: usize,
    curve: Curve,
}

/// Owned arrays only. The payoff unit is one deck payoff; the engine scales it to its order mass. The payoff is the
/// weighted sum of its terms (one term of weight one for a single metric), each nondecreasing in its own event bonus
/// and in the power.
pub(crate) struct DeckPayoffBounds {
    pub(crate) power: TeamPowerBounds,
    /// Visit order of members: the bonus the payoff gains most from, then the power order.
    pub(crate) members: Vec<usize>,
    kinds: Vec<Kind>,
    terms: Vec<Term>,
    /// The Skip score of the request's own chart, present exactly when a [`Curve::Skip`] term reads it.
    skip: Option<SkipEvaluator>,
}

impl DeckPayoffBounds {
    /// A common terminal upper bound ranks proposals by exact power and canonical identity.
    /// This mode certifies no deck's payoff until the caller evaluates it.
    pub(crate) fn compile_upper_only(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
    ) -> Result<Self, Error> {
        let cap = match *metric {
            Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. } => 1,
            Metric::CappedScore { threshold } => i128::from(threshold),
            _ => return Err(unavailable("requires a capped terminal utility")),
        };
        let power = TeamPowerBounds::tables(pool, request, domain)?;
        let members = power.members.clone();
        let kind =
            Kind { member: vec![0; pool.members.len()], snap: vec![0; domain.snaps().len()], snap_best: 0, maximum: 0 };
        Ok(Self {
            power,
            members,
            kinds: vec![kind],
            terms: vec![Term { weight: 1, kind: 0, curve: Curve::UpperOnly(cap) }],
            skip: None,
        })
    }

    /// The common terminal bound of an upper-only payoff.
    pub(crate) fn upper_only(&self) -> Option<i128> {
        match self.terms.as_slice() {
            [Term { curve: Curve::UpperOnly(cap), .. }] => Some(*cap),
            _ => None,
        }
    }

    /// With score `steps` (a played Live's single metric), the payoff is only bounded: an evaluated deck may fall
    /// below it. A combined metric of deck-determined payoffs sums the curves of its terms with their folded weights.
    pub(crate) fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
        input: Option<&EventPayoffInput>,
        steps: Option<ScoreSteps>,
    ) -> Result<Self, Error> {
        let terms: Vec<(i128, &Metric)> = match metric.folded_terms() {
            Some(_) if steps.is_some() => return Err(unavailable("score steps of one metric given for several terms")),
            Some(terms) => terms.collect(),
            None => vec![(1, metric)],
        };
        let mut steps = steps;
        let mut parts = Vec::with_capacity(terms.len());
        for (weight, term) in terms {
            let (bonus_type, effect_event, curve) = curve(pool, request, term, input, steps.take())?;
            let effects = effect_event.map(|id| event::event_effects(pool.master, id)).unwrap_or_default();
            let mut member = vec![0; pool.members.len()];
            for &m in domain.members() {
                let card = ournotes_sim::bonus::event_member(pool.master, &pool.members[m]);
                member[m] = card_bonus(&effects, EventCard::Member(&card), bonus_type)?;
            }
            let snap = domain
                .snaps()
                .iter()
                .map(|&s| {
                    card_bonus(&effects, EventCard::Snap(&ournotes_sim::bonus::event_snap(&pool.snaps[s])), bonus_type)
                })
                .collect::<Result<Vec<_>, _>>()?;
            parts.push((weight, member, snap, curve));
        }
        Self::assemble(pool, request, domain, parts)
    }

    /// The bound of a played Live's combined metric from the score steps of every term.
    pub(crate) fn stepped(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        terms: Vec<TermSteps<'_>>,
    ) -> Result<Self, Error> {
        let parts = terms
            .into_iter()
            .map(|(weight, member, snap, steps)| (weight, member.to_vec(), snap.to_vec(), Curve::Steps(steps)))
            .collect();
        Self::assemble(pool, request, domain, parts)
    }

    /// Terms as (weight, bonus per member, bonus per Snap column, curve). Terms with equal bonuses share one kind.
    fn assemble(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        parts: Vec<(i128, Vec<i64>, Vec<i64>, Curve)>,
    ) -> Result<Self, Error> {
        // The power tables certify nonnegative, nonwrapping powers for every legal deck, and for a Skip objective
        // they also prove the Skip score nondecreasing over the whole compiled power interval and hold its evaluator.
        // They protect native evaluation; without score steps power only breaks payoff ties.
        let power = TeamPowerBounds::tables(pool, request, domain)?;
        // Without that proof no Skip term is ranked by deck, and the request keeps its other route.
        let skip = if parts.iter().any(|part| matches!(part.3, Curve::Skip)) {
            Some(power.monotone_skip().ok_or_else(|| unavailable("Skip score monotonicity is not certified"))?)
        } else {
            None
        };
        let mut kinds = Vec::<Kind>::new();
        let mut terms = Vec::with_capacity(parts.len());
        // How much the payoff gains from each kind's bonus, at least.
        let mut gains = Vec::<f64>::new();
        for (weight, member, snap, curve) in parts {
            let kind = match kinds.iter().position(|kind| kind.member == member && kind.snap == snap) {
                Some(kind) => kind,
                None => {
                    let maximum = nonnegative_i32(largest_team_bonus(
                        domain.members().iter().map(|&m| (pool.members[m].character_id, member[m])),
                        &snap,
                    ))?;
                    // Prove bonus + 10000 and every product safe up to the largest bonus, independently of a zero
                    // factor.
                    point_product(0, maximum, 0)?;
                    let mut best = snap.clone();
                    best.sort_unstable_by(|a, b| b.cmp(a));
                    let snap_best = best.iter().take(5).sum();
                    kinds.push(Kind { member, snap, snap_best, maximum });
                    gains.push(0.0);
                    kinds.len() - 1
                }
            };
            gains[kind] += weight as f64 * curve.slope();
            terms.push(Term { weight, kind, curve });
        }
        if kinds.len() > KINDS {
            return Err(unavailable("too many distinct event bonuses"));
        }
        let mut members = power.members.clone();
        let bound = Self { power, members: Vec::new(), kinds, terms, skip };
        let root = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        let maxima: Vec<i64> = bound.kinds.iter().map(|kind| kind.maximum).collect();
        bound
            .payoff(&maxima, bound.power.upper(pool, domain, &root, 0).ok_or_else(|| unavailable("no legal team"))?)?;
        let rank = |m: usize| {
            bound.kinds.iter().zip(&gains).map(|(kind, gain)| gain.max(1.0) * kind.member[m] as f64).sum::<f64>()
        };
        // Stable: equal bonuses keep the power order.
        members.sort_by(|&a, &b| rank(b).total_cmp(&rank(a)));
        Ok(Self { members, ..bound })
    }

    /// The payoff of a deck with these event bonuses (one per kind) and this power.
    fn payoff(&self, bonus: &[i64], power: i64) -> Result<i128, Error> {
        let mut sum = 0i128;
        for term in &self.terms {
            sum = term
                .curve
                .value(bonus[term.kind], power, self.skip.as_ref())?
                .checked_mul(term.weight)
                .and_then(|value| sum.checked_add(value))
                .ok_or_else(|| unavailable("weighted payoff overflow"))?;
        }
        Ok(sum)
    }

    /// Bound every completion of the members in `SLOTS[..depth]` (no Snap is placed yet): (payoff, power), or None
    /// when the prefix has no legal completion. Each kind's bonus is bounded on its own. The power is the smaller of
    /// the column relaxation and, where it applies, an assignment of the remaining slots' Snap terms; both bound
    /// every completion, so the payoff curve read at it stays a bound.
    pub(crate) fn upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> Result<Option<(i128, i64)>, Error> {
        let Some((available, required)) = candidates(pool, domain, p, depth) else {
            return Ok(None);
        };
        let power = self.power.refined_upper(pool, domain, p, depth, &available, &required);
        let mut bonus = [0i64; KINDS];
        let mut rows = HashMap::<i64, i64>::new();
        for (kind, bonus) in self.kinds.iter().zip(&mut bonus) {
            rows.clear();
            for &m in &available {
                rows.entry(pool.members[m].character_id)
                    .and_modify(|v| *v = (*v).max(kind.member[m]))
                    .or_insert(kind.member[m]);
            }
            *bonus = SLOTS[..depth].iter().map(|&s| kind.member[p.members[s]]).sum::<i64>()
                + sum_rows(&rows, &required, 5 - depth)
                + kind.snap_best;
            if *bonus > kind.maximum {
                return Err(unavailable("prefix escaped the compiled proof domain"));
            }
        }
        Ok(Some((self.payoff(&bonus, power)?, power)))
    }

    /// Whether an evaluated deck may pay less than its score-step or common terminal bound.
    pub(crate) fn bounded_only(&self) -> bool {
        self.terms.iter().any(|term| matches!(term.curve, Curve::Steps(_) | Curve::UpperOnly(_)))
    }

    /// With score steps, the score cap of every order of a deck with power at most `power`. The terms of one payoff
    /// share the cap.
    pub(crate) fn score_cap(&self, power: i64) -> Option<i128> {
        self.terms.iter().find_map(|term| match &term.curve {
            Curve::Steps(steps) => Some(steps.score_cap(power)),
            _ => None,
        })
    }

    /// An upper bound of the power of every legal team.
    pub(crate) fn power_cap(&self, pool: &Pool, domain: &CandidateDomain) -> Option<i64> {
        self.power.upper(pool, domain, &PhysicalDeck { members: [0; 5], snaps: [None; 5] }, 0)
    }

    /// The first `k` Snap bindings of a complete member layout, ranked by curve value, then power, then Snap IDs (no
    /// Snap first), with their curve value and exact power. `base` is the layout's power without Snaps.
    ///
    /// Snaps are scanned once in public-ID order, so partial bindings of one slot mask extend identically. A binding
    /// is dropped once `k` bindings of its mask with at least its bonus of every kind beat its power and Snap IDs:
    /// every common extension keeps them ahead (the payoff is nondecreasing in each bonus and in the power), so no
    /// dropped binding is among the layout's first `k`.
    ///
    /// A [`Curve::Skip`] term reads no bonus: the payoff of a binding is its own power's score, so the rows are
    /// ranked by the payoff at `base + power`. With one bonus-only kind the `N == 1` shortcut still holds on power,
    /// since a higher power scores at least as much.
    pub(crate) fn frontier(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        base: i64,
        k: usize,
    ) -> Result<Vec<(i128, i64, PhysicalDeck)>, Error> {
        match self.kinds.len() {
            1 => self.frontier_of::<1>(domain, p, base, k),
            2 => self.frontier_of::<2>(domain, p, base, k),
            _ => self.frontier_of::<KINDS>(domain, p, base, k),
        }
    }

    /// [`DeckPayoffBounds::frontier`] with the bonuses of at most `N` kinds.
    fn frontier_of<const N: usize>(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        base: i64,
        k: usize,
    ) -> Result<Vec<(i128, i64, PhysicalDeck)>, Error> {
        type Partial<const N: usize> = ([i64; N], i64, [Option<usize>; 5]);
        let per_kind = |read: &dyn Fn(&Kind) -> i64| -> [i64; N] {
            std::array::from_fn(|kind| self.kinds.get(kind).map_or(0, read))
        };
        let w = p.members.map(|m| self.power.w[m].as_slice());
        let members = per_kind(&|kind| p.members.iter().map(|&m| kind.member[m]).sum());
        let mut states: Vec<Vec<Partial<N>>> = vec![Vec::new(); 32];
        states[0].push(([0; N], 0, [None; 5]));
        for snap in 0..domain.snaps().len() {
            let bonus = per_kind(&|kind| kind.snap[snap]);
            let mut next = states.clone();
            for (mask, rows) in states.iter().enumerate() {
                for slot in (0..5).filter(|slot| mask & (1 << slot) == 0) {
                    for &(sum, power, binding) in rows {
                        let mut binding = binding;
                        binding[slot] = Some(snap);
                        let sum = std::array::from_fn(|kind| sum[kind] + bonus[kind]);
                        next[mask | (1 << slot)].push((sum, power + w[slot][snap], binding));
                    }
                }
            }
            for rows in &mut next {
                if N == 1 {
                    // One bonus orders the rows: only the `k` best powers among the rows before one can beat it.
                    rows.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
                    let mut ahead: Vec<(i64, [Option<usize>; 5])> = Vec::with_capacity(k + 1);
                    rows.retain(|&(_, power, binding)| {
                        let before = |&(p, b): &(i64, [Option<usize>; 5])| p > power || (p == power && b < binding);
                        let at = ahead.partition_point(before);
                        if at >= k {
                            return false;
                        }
                        ahead.insert(at, (power, binding));
                        ahead.truncate(k);
                        true
                    });
                } else {
                    // By power and Snap IDs, every row that beats a row comes before it; a dropped row's `k` kept
                    // rows also beat whatever it beats.
                    rows.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
                    let mut kept: Vec<Partial<N>> = Vec::with_capacity(rows.len());
                    for &row in rows.iter() {
                        let covers = |other: &&Partial<N>| (0..N).all(|kind| other.0[kind] >= row.0[kind]);
                        if kept.iter().filter(covers).take(k).count() < k {
                            kept.push(row);
                        }
                    }
                    *rows = kept;
                }
            }
            states = next;
        }
        let mut out = Vec::new();
        for (sum, power, binding) in states.into_iter().flatten() {
            let bonus: [i64; N] = std::array::from_fn(|kind| members[kind] + sum[kind]);
            if self.kinds.iter().zip(&bonus).any(|(kind, &bonus)| bonus > kind.maximum) {
                return Err(unavailable("binding escaped the compiled proof domain"));
            }
            out.push((self.payoff(&bonus, base + power)?, base + power, binding));
        }
        out.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        out.truncate(k);
        Ok(out
            .into_iter()
            .map(|(payoff, power, binding)| {
                let snaps = binding.map(|s| s.map(|column| domain.snaps()[column]));
                (payoff, power, PhysicalDeck { members: p.members, snaps })
            })
            .collect())
    }
}

#[cfg(test)]
use super::gate_tests::common;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{Constraints, expectation::visit_physical_decks, uniform::canonical};
    use common::{Rng, Synth, replace_table, roster, set_column, short_chart, synth};
    use ournotes_sim::scenario::{ContextInput, Scenario};
    use serde_json::json;
    use std::collections::HashSet;

    /// Seven one-character members and three Snaps. Event 7 has point effects and item effects on different cards,
    /// and normal and Challenge item rewards of resource 4/88 next to another resource.
    fn fixture() -> Synth {
        let mut data = synth(&mut Rng::new(735), 7, 3);
        set_column(&mut data, "MasterMemberCard", &mut |row| {
            row["_characterID"] = row["_id"].clone();
            row["_leaderSkillID"] = json!(4);
        });
        set_column(&mut data, "MasterLiveMusic", &mut |row| row["_liveScoreRankGroup"] = json!(1));
        let effect = |id: i64, kind: i64, constraint: i64, card: (&str, i64), value: i64| {
            let mut row = json!({"_id":id,"_eventId":7,"_eventBonusType":kind,"_resourceTypeConstraint":constraint,
                "_rank1EffectValue":value,"_rank2EffectValue":value,"_rank3EffectValue":value,
                "_rank4EffectValue":value,"_rank5EffectValue":value});
            if card.1 > 0 {
                row[card.0] = json!(card.1);
            }
            row
        };
        replace_table(&mut data, "MasterChallengeMusic", json!([{"_id":70,"_eventId":7,"_liveMusicId":10}]));
        replace_table(
            &mut data,
            "MasterEvent",
            json!([{"_id":7,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2,
                "_liveEventRewardGroup":37,"_challengeLiveEventRewardGroup":38}]),
        );
        replace_table(
            &mut data,
            "MasterEventEffect",
            json!([
                effect(1, 0, 2, ("_memberCardId", 2), 1000),
                effect(2, 0, 3, ("_supportCardId", 2), 700),
                effect(3, 1, 2, ("_memberCardId", 3), 2500),
                effect(4, 1, 2, ("_memberCardId", 5), 4000),
                effect(5, 1, 3, ("_supportCardId", 0), 1500),
                effect(6, 1, 3, ("_supportCardId", 3), 3500),
            ]),
        );
        replace_table(
            &mut data,
            "MasterLiveScoreRank",
            json!([{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0}]),
        );
        replace_table(&mut data, "MasterLiveEventPoint", json!([{"_id":1,"_group":1,"_scoreRank":2,"_value":100}]));
        replace_table(
            &mut data,
            "MasterChallengeLiveEventPoint",
            json!([{"_id":1,"_group":2,"_scoreRank":2,"_value":300}]),
        );
        replace_table(&mut data, "MasterLiveChallengePoint", json!([{"_id":1,"_scoreRank":2,"_value":5}]));
        replace_table(
            &mut data,
            "MasterLiveEventReward",
            json!([
                {"_id":11,"_eventGroup":37,"_group":5,"_scoreRank":2,"_probability":10000,
                    "_resourceType":4,"_resourceId":88,"_resourceCount":10},
                {"_id":13,"_eventGroup":39,"_group":37,"_scoreRank":2,"_probability":10000,
                    "_resourceType":1,"_resourceId":5,"_resourceCount":100}
            ]),
        );
        replace_table(
            &mut data,
            "MasterChallengeLiveEventReward",
            json!([{"_id":11,"_eventGroup":38,"_group":5,"_scoreRank":2,"_probability":10000,
                "_resourceType":4,"_resourceId":88,"_resourceCount":2}]),
        );
        data
    }

    /// Every canonical team of a fixed-leader Skip domain: each member prefix bounds it, and its layout's frontier
    /// holds its exact native payoff and power, in the canonical rank order. A combined metric pays the folded
    /// weighted sum of its terms' native payoffs.
    fn check_exact(scenario: Scenario, metric: Metric) {
        let data = fixture();
        let master = data.master();
        let owned = roster(&mut Rng::new(739), &master);
        let input: ContextInput = serde_json::from_value(json!({
            "powerSnapshot":{"eventIds":[7],"capturedJstTicks":50},
            "resultClock":{"execution":"skip","serverNowJstTicks":99},
            "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":7,"points":0,"challengePoints":30,"added":[]}],
                "eventWindows":[{"eventId":7,"startJstTicks":90,"endJstTicks":100}]}
        }))
        .unwrap();
        let context = input.resolve(&master, scenario, Some(1004), &[]).unwrap();
        let pool = context.pool(&master, &owned).unwrap();
        let chart = short_chart(&mut Rng::new(3), 4, false).0;
        let request = SearchRequest {
            objective: Objective::SkipScore { score_id: 1004, chart }.in_scenario(context.clone()),
            k: 3,
            constraints: Constraints { leader: Some(1), ..Default::default() },
            time_limit: None,
        };
        let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
        let payoff = input.event_payoff.as_ref().unwrap();
        let folded = crate::search::levels::fold(&metric, |term| {
            crate::search::joint::term_cap(&pool, &request, &domain, term, Some(payoff))
        })
        .unwrap();
        let metric = folded.unwrap_or(metric);
        let bound = DeckPayoffBounds::compile(&pool, &request, &domain, &metric, Some(payoff), None).unwrap();
        // Point effects and item effects are two different bonuses of the fixture; challenge points and the Skip
        // score read no bonus at all and share the third, all-zero one.
        let read = |test: fn(&Metric) -> bool| usize::from(metric.any(test));
        let kinds = read(|term| matches!(term, Metric::ClientEventPoints { .. }))
            + read(|term| matches!(term, Metric::RankedEventItems { .. }))
            + usize::from(metric.any(|term| matches!(term, Metric::ClientChallengePoints { .. } | Metric::Score)));
        assert_eq!(bound.kinds.len(), kinds);
        let mut layouts = HashMap::new();
        let mut teams = 0;
        let mut payoffs = HashSet::new();
        visit_physical_decks(&pool, &request.constraints, |deck| {
            if canonical(&pool, &deck) != deck {
                return Ok(true);
            }
            teams += 1;
            let (power, score) = crate::search::evaluate(&pool, &deck.as_deck(), &request.objective)?;
            let score = score.expect("Skip score");
            let native = |term: &Metric| -> Result<i128, Error> {
                Ok(match *term {
                    Metric::Score => i128::from(score),
                    Metric::ClientEventPoints { event_id } => i128::from(
                        context
                            .preview_event_points(&pool, &deck.as_deck(), payoff, event_id, score)?
                            .points_for(event_id),
                    ),
                    Metric::ClientChallengePoints { event_id } => i128::from(
                        context
                            .preview_event_points(&pool, &deck.as_deck(), payoff, event_id, score)?
                            .challenge_points_for(event_id),
                    ),
                    Metric::RankedEventItems { event_id, resource_type, resource_id } => {
                        let items = context.preview_event_items(&pool, &deck.as_deck(), payoff, event_id, score)?;
                        ournotes_sim::scenario::item_payoff(&items, event_id, resource_type, resource_id)?
                    }
                    _ => unreachable!(),
                })
            };
            let exact = match metric.folded_terms() {
                Some(terms) => terms.map(|(weight, term)| Ok(weight * native(term)?)).sum::<Result<i128, Error>>()?,
                None => native(&metric)?,
            };
            payoffs.insert(exact);
            for depth in 0..=5 {
                let (upper, power_upper) = bound.upper(&pool, &domain, &deck, depth)?.expect("legal prefix");
                assert!(upper >= exact, "{metric:?} depth {depth}: {upper} < {exact}");
                assert!(power_upper >= i64::from(power));
            }
            let all = layouts.entry(deck.members).or_insert_with(|| {
                let bare = PhysicalDeck { members: deck.members, snaps: [None; 5] };
                let base = crate::search::evaluate(&pool, &bare.as_deck(), &request.objective).unwrap().0;
                bound.frontier(&domain, &deck, i64::from(base), 1000).unwrap()
            });
            assert!(all.contains(&(exact, i64::from(power), deck)), "{metric:?}: {deck:?} {exact} {power}");
            Ok(true)
        })
        .unwrap();
        // Six member sets around the fixed leader, each with every injection of up to three Snaps into five slots.
        assert_eq!(teams, 15 * 136);
        assert_eq!(layouts.len(), 15);
        for (members, all) in &layouts {
            assert_eq!(all.len(), 136);
            let key = |d: &PhysicalDeck| d.snaps.map(|s| s.map(|s| pool.snaps[s].id));
            assert!(
                all.is_sorted_by(|a, b| (a.0, a.1) > (b.0, b.1) || ((a.0, a.1) == (b.0, b.1) && key(&a.2) < key(&b.2)))
            );
            let p = PhysicalDeck { members: *members, snaps: [None; 5] };
            let base = all.iter().find(|row| row.2 == p).expect("the bare layout").1;
            assert_eq!(bound.frontier(&domain, &p, base, 4).unwrap(), all[..4]);
        }
        if !matches!(metric, Metric::ClientChallengePoints { .. }) {
            assert!(payoffs.len() > 2, "{metric:?}: the fixture must rank by payoff, not only by power");
        }
    }

    /// Both power bounds of a member prefix are at least the exact power of every completion beneath it, and the bound
    /// the search uses is the smaller of the two, so it never exceeds the column relaxation.
    #[test]
    fn assignment_power_bounds_every_completion_and_never_exceeds_the_relaxation() {
        let data = fixture();
        let master = data.master();
        let owned = roster(&mut Rng::new(739), &master);
        let input: ContextInput = serde_json::from_value(json!({
            "powerSnapshot":{"eventIds":[7],"capturedJstTicks":50},
            "resultClock":{"execution":"skip","serverNowJstTicks":99},
            "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":7,"points":0,"challengePoints":30,"added":[]}],
                "eventWindows":[{"eventId":7,"startJstTicks":90,"endJstTicks":100}]}
        }))
        .unwrap();
        let context = input.resolve(&master, Scenario::Free(10), Some(1004), &[]).unwrap();
        let pool = context.pool(&master, &owned).unwrap();
        let chart = short_chart(&mut Rng::new(3), 4, false).0;
        let request = SearchRequest {
            objective: Objective::SkipScore { score_id: 1004, chart }.in_scenario(context.clone()),
            k: 3,
            constraints: Constraints { leader: Some(1), ..Default::default() },
            time_limit: None,
        };
        let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
        let bound = DeckPayoffBounds::compile(
            &pool,
            &request,
            &domain,
            &Metric::ClientEventPoints { event_id: 7 },
            input.event_payoff.as_ref(),
            None,
        )
        .unwrap();
        // The best exact power over the completions of each member prefix. Only the canonical layout is a team the
        // search visits: its four non-leaders take slots 0, 1, 3, 4 in ascending ID order, so every prefix of it is a
        // prefix the member order and the ID floor of the search admit.
        let mut best = HashMap::<(usize, [usize; 5]), i64>::new();
        visit_physical_decks(&pool, &request.constraints, |deck| {
            if canonical(&pool, &deck) != deck {
                return Ok(true);
            }
            let power = i64::from(crate::search::evaluate(&pool, &deck.as_deck(), &request.objective)?.0);
            for depth in 0..=5 {
                let mut prefix = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
                for &slot in &SLOTS[..depth] {
                    prefix.members[slot] = deck.members[slot];
                }
                let held = best.entry((depth, prefix.members)).or_insert(i64::MIN);
                *held = (*held).max(power);
            }
            Ok(true)
        })
        .unwrap();
        assert!(best.len() > 5);
        for (&(depth, members), &power) in &best {
            let prefix = PhysicalDeck { members, snaps: [None; 5] };
            let (available, required) = candidates(&pool, &domain, &prefix, depth).expect("legal prefix");
            let relaxed = bound.power.upper(&pool, &domain, &prefix, depth).expect("legal prefix");
            assert!(relaxed >= power, "depth {depth}: the relaxation is below a completion");
            let refined = bound.power.refined_upper(&pool, &domain, &prefix, depth, &available, &required);
            assert!(refined >= power, "depth {depth}: the bound in use is below a completion");
            assert!(refined <= relaxed, "depth {depth}: the bound in use is above the relaxation");
            match bound.power.assigned_upper(&pool, &domain, &prefix, depth) {
                Some(assigned) => assert!(assigned >= power, "depth {depth}: the assignment is below a completion"),
                None => assert_eq!(depth, 5, "a member prefix still to fill has an assignment bound"),
            }
        }
    }

    fn combined(levels: &[&[(&Metric, u32)]]) -> Metric {
        use crate::types::{MetricLevel, MetricTerm};
        Metric::Combined {
            levels: levels
                .iter()
                .map(|terms| {
                    MetricLevel::new(
                        terms.iter().map(|&(metric, weight)| MetricTerm { metric: metric.clone(), weight }).collect(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn every_canonical_skip_team_is_bounded_and_ranked_exactly() {
        let items = Metric::RankedEventItems { event_id: 7, resource_type: 4, resource_id: 88 };
        for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
            check_exact(scenario, Metric::ClientEventPoints { event_id: 7 });
            check_exact(scenario, items.clone());
        }
        check_exact(Scenario::Free(10), Metric::ClientChallengePoints { event_id: 7 });
    }

    #[test]
    fn combined_skip_score_terms_are_bounded_and_ranked_exactly() {
        let points = Metric::ClientEventPoints { event_id: 7 };
        let items = Metric::RankedEventItems { event_id: 7, resource_type: 4, resource_id: 88 };
        for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
            check_exact(scenario, combined(&[&[(&Metric::Score, 1), (&points, 1)]]));
            check_exact(scenario, combined(&[&[(&Metric::Score, 1), (&points, 1000)]]));
            check_exact(scenario, combined(&[&[(&points, 1), (&Metric::Score, 1)]]));
            check_exact(scenario, combined(&[&[(&Metric::Score, 1)], &[(&points, 1)]]));
            check_exact(scenario, combined(&[&[(&points, 2)], &[(&Metric::Score, 1)], &[(&items, 1)]]));
        }
        check_exact(Scenario::Free(10), combined(&[&[(&Metric::Score, 3)]]));
    }

    #[test]
    fn combined_skip_payoffs_are_ranked_exactly_over_the_bonus_of_each_term() {
        let points = Metric::ClientEventPoints { event_id: 7 };
        let challenge = Metric::ClientChallengePoints { event_id: 7 };
        let items = Metric::RankedEventItems { event_id: 7, resource_type: 4, resource_id: 88 };
        for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
            check_exact(scenario, combined(&[&[(&points, 3), (&items, 40)]]));
            check_exact(scenario, combined(&[&[(&items, 1), (&points, 1_000_000_000)]]));
            check_exact(scenario, combined(&[&[(&items, 1)], &[(&points, 1)]]));
            check_exact(scenario, combined(&[&[(&points, 1)], &[(&items, 1)]]));
        }
        check_exact(Scenario::Free(10), combined(&[&[(&points, 2), (&challenge, 35), (&items, 7)]]));
        check_exact(Scenario::Free(10), combined(&[&[(&challenge, 1)], &[(&items, 5), (&points, 1)]]));
        check_exact(Scenario::Free(10), combined(&[&[(&items, 9)]]));
    }
}
