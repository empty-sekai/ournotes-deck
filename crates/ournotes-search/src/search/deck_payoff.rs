//! Payoffs the deck alone determines: Skip event points and challenge points at the configured result rank, and the
//! selected item rewards of any route. Each is a nondecreasing function of one additive deck sum, the event bonus of
//! its members and Snaps, so the canonical team Top-K needs no Live play: the payoff, then power, then the canonical
//! member and Snap IDs rank every team exactly.
//!
//! A played Live's event points, challenge points and score targets step with the local score of each order instead
//! (a declared multiplayer room ranks its total, which is nondecreasing in the local score). Every order of a deck
//! scores at most its power times the joint envelope's global coefficient, so the step of that cap bounds the payoff
//! by the deck's bonus and power alone ([`ScoreSteps`]); the search then ranks by this bound and evaluates the decks
//! it ranks first, re-ranking those that pay less.
//!
//! Members branch in `SLOTS` order; a member prefix bounds its completions by the best remaining bonuses and the
//! power bound of [`TeamPowerBounds`]. A complete member layout has one Snap-independent power constant, and its Snap
//! bindings are ranked exactly by [`DeckPayoffBounds::frontier`].
//!
//! A common terminal upper bound can also order the domain by power and canonical identity. Its proposals require
//! terminal evaluation; the upper-only mode does not assign that bound as a deck's actual payoff.
use super::{
    Objective, Pool, SearchRequest,
    expectation::PhysicalDeck,
    team_power::{SLOTS, TeamPowerBounds, candidates, largest_team_bonus, sum_rows},
};
use crate::{domain::CandidateDomain, types::Metric};
use ournotes_sim::{
    Error,
    event::{self, EventCard, EventResultRoute},
    master::EventEffectRow,
    scenario::EventPayoffInput,
};
use std::collections::{HashMap, HashSet};

fn unavailable(message: &str) -> Error {
    Error::Unsupported(format!("deck payoff bound unavailable: {message}"))
}

fn nonnegative_i32(value: i64) -> Result<i64, Error> {
    if (0..=i64::from(i32::MAX)).contains(&value) {
        Ok(value)
    } else {
        Err(unavailable("negative or wrapping native Int32 value"))
    }
}

/// Audit the native order `value * (bonus + 10000) * rate / 10000`.
/// The first product must fit even when the final rate is zero.
fn point_product(value: i64, bonus: i64, rate: i64) -> Result<i128, Error> {
    nonnegative_i32(value)?;
    nonnegative_i32(bonus)?;
    nonnegative_i32(rate)?;
    let base = nonnegative_i32(bonus.checked_add(10000).ok_or_else(|| unavailable("bonus overflow"))?)?;
    let first = nonnegative_i32(value.checked_mul(base).ok_or_else(|| unavailable("first product overflow"))?)?;
    let last = nonnegative_i32(first.checked_mul(rate).ok_or_else(|| unavailable("second product overflow"))?)?;
    Ok(i128::from(last / 10000))
}

/// Challenge-point earnings have no event bonus or 10000-scale intermediate.
fn challenge_product(value: i64, rate: i64) -> Result<i128, Error> {
    nonnegative_i32(value)?;
    nonnegative_i32(rate)?;
    let product = value.checked_mul(rate).ok_or_else(|| unavailable("CP product overflow"))?;
    Ok(i128::from(nonnegative_i32(product)?))
}

/// Reproduce the native card's sum of `bonus_type` effects only after proving every cast/addition safe.
fn card_bonus(effects: &[&EventEffectRow], card: EventCard<'_>, bonus_type: i64) -> Result<i64, Error> {
    let mut sum = 0i64;
    for effect in effects.iter().filter(|e| e.event_bonus_type == bonus_type) {
        let (hit, rank) = match card {
            EventCard::Member(m) => (event::is_member_target(effect, m)?, m.rank),
            EventCard::Snap(s) => (event::is_snap_target(effect, s), s.rank),
        };
        if hit {
            let value = nonnegative_i32(event::rank_effect_value(effect, rank)?)?;
            sum = nonnegative_i32(sum + value)?;
        }
    }
    Ok(sum)
}

/// One step of a played Live's per-order payoff, from its lowest score on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// Event or challenge points `multiplier * (bonus + 10000) / 10000`, the multiplier being the rate times the
    /// largest reward value at or below the step's score (challenge points read no card bonus).
    Points(i64),
    /// A score target: reached (1) or not (0).
    Target(bool),
}

/// The per-order payoff of a played Live steps with the order's score. Every order of a deck with power `p`
/// scores at most `ceil(p * global * (1 + eps))` (the global coefficient of the joint envelope with its margin, see
/// `JointBounds::score_steps`), so the step of that cap bounds each of its orders.
#[derive(Clone, Debug)]
pub(crate) struct ScoreSteps {
    global: f64,
    eps: f64,
    /// (lowest score, step) by ascending score, each step at least the previous one.
    steps: Vec<(i128, Step)>,
}

impl ScoreSteps {
    pub(crate) fn new(global: f64, eps: f64, steps: Vec<(i128, Step)>) -> Self {
        Self { global, eps, steps }
    }

    /// The score cap of every order of a deck with power at most `power`.
    pub(crate) fn score_cap(&self, power: i64) -> i128 {
        ((power as f64) * self.global * (1.0 + self.eps)).ceil() as i128
    }

    fn value(&self, bonus: i64, power: i64) -> Result<i128, Error> {
        let cap = self.score_cap(power);
        let at = self.steps.partition_point(|&(score, _)| score <= cap);
        // The joint bound's compilation proved the native products nonwrapping up to the largest bonus.
        match at.checked_sub(1).map(|i| self.steps[i].1) {
            None => Err(unavailable("score cap below the lowest step")),
            Some(Step::Points(multiplier)) => Ok(i128::from(multiplier) * i128::from(bonus + 10000) / 10000),
            Some(Step::Target(reached)) => Ok(i128::from(reached)),
        }
    }
}

/// The payoff of a deck as a function of its event bonus (10000 = 100 %) and power, nondecreasing in both on the
/// compiled domain.
#[derive(Clone, Debug)]
enum Curve {
    /// Event points at a fixed result rank, `reward * (bonus + 10000) * rate / 10000`.
    Points { reward: i64, rate: i64 },
    /// The selected rewards of the metric's resource, each `count * (bonus + 10000) * rate / 10000`.
    Items { counts: Vec<i64>, rate: i64 },
    /// No card changes the payoff.
    Constant(i128),
    /// Every deck pays at most this value; attainment requires terminal evaluation.
    UpperOnly(i128),
    /// A bound of each order of a played Live: the step its score cap reaches.
    Steps(ScoreSteps),
}

impl Curve {
    fn value(&self, bonus: i64, power: i64) -> Result<i128, Error> {
        match self {
            Self::Points { reward, rate } => point_product(*reward, bonus, *rate),
            Self::Items { counts, rate } => {
                counts.iter().try_fold(0i128, |sum, &count| Ok(sum + point_product(count, bonus, *rate)?))
            }
            Self::Constant(value) | Self::UpperOnly(value) => Ok(*value),
            Self::Steps(steps) => steps.value(bonus, power),
        }
    }
}

/// Event IDs of the local counters after a result's event stage: a normal Skip adds the held events it counts; every
/// other route only updates existing counters (or fails for every deck alike).
fn result_local_events(route: EventResultRoute, local: &[event::LocalEvent], holding: &[i64]) -> HashSet<i64> {
    let mut ids: HashSet<_> = local.iter().map(|l| l.event_id).collect();
    if route == EventResultRoute::NormalSkip {
        ids.extend(holding);
    }
    ids
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
            Metric::ClientChallengePoints { .. } | Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. } => {
                Ok((event::EVENT_POINT, None, Curve::Steps(steps)))
            }
            _ => Err(unavailable("score steps of a payoff that does not step with the score")),
        };
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
        Metric::ConditionalClientEventItems { event_id, resource_type, resource_id }
            if skip || matches!(request.objective.inner(), Objective::LiveScore { .. }) =>
        {
            let q = context.event_request(master, input, event_id)?;
            let choices = input.selected_rewards.as_ref().ok_or_else(|| unavailable("unknown selected rewards"))?;
            let challenge =
                matches!(q.route, EventResultRoute::ChallengePlayed { .. } | EventResultRoute::ChallengeSkip { .. });
            let rate = if challenge {
                event::challenge_point_bonus(master, i64::from(q.consumed_count))?[0]
            } else {
                event::boost_bonus(master, i64::from(q.consumed_count))?[0]
            };
            nonnegative_i32(rate)?;
            let rows = if challenge { &master.challenge_live_event_rewards } else { &master.live_event_rewards };
            let local = result_local_events(q.route, &q.local_events, &q.holding_event_ids);
            // Each settled reward: its effect event (None: no local counter, no bonus) and, when it counts toward
            // the metric, its resource count.
            let mut settled: Vec<(Option<i64>, Option<i64>)> = Vec::new();
            for choice in choices {
                let row = rows.iter().find(|r| r.id == choice.reward_id);
                if row.is_none() && q.route != EventResultRoute::NormalPlayed {
                    continue;
                }
                let effect = if local.contains(&choice.event_id) {
                    // Played Challenge uses the selected challenge event even when the reward names another event.
                    let id = match q.route {
                        EventResultRoute::ChallengePlayed { event_id } => event_id,
                        _ => choice.event_id,
                    };
                    master.event(id).ok_or_else(|| unavailable("unknown reward effect event"))?;
                    Some(id)
                } else if matches!(q.route, EventResultRoute::NormalSkip | EventResultRoute::ChallengeSkip { .. }) {
                    return Err(unavailable("a selected Skip reward has no local event"));
                } else {
                    None
                };
                let row = row.ok_or_else(|| unavailable("unknown normal event reward"))?;
                let counted =
                    choice.event_id == event_id && row.resource_type == resource_type && row.resource_id == resource_id;
                settled.push((effect, counted.then_some(row.resource_count)));
            }
            // Every settled reward reads its effect event's bonus of each card; prove all of them native-safe.
            let read: HashSet<i64> = settled.iter().filter_map(|&(effect, _)| effect).collect();
            for id in read {
                let effects = event::event_effects(master, id);
                for m in &pool.members {
                    let card = ournotes_sim::bonus::event_member(master, m);
                    card_bonus(&effects, EventCard::Member(&card), event::EVENT_ITEM)?;
                }
                for s in &pool.snaps {
                    card_bonus(&effects, EventCard::Snap(&ournotes_sim::bonus::event_snap(s)), event::EVENT_ITEM)?;
                }
            }
            let counted: Vec<_> = settled.iter().filter_map(|&(effect, count)| count.map(|c| (effect, c))).collect();
            let target = counted.first().and_then(|&(effect, _)| effect);
            if counted.iter().any(|&(effect, _)| effect != target) {
                return Err(unavailable("counted rewards read different effect events"));
            }
            let counts = counted.into_iter().map(|(_, c)| nonnegative_i32(c)).collect::<Result<Vec<_>, _>>()?;
            let items = Curve::Items { counts, rate };
            Ok(match target {
                Some(_) => (event::EVENT_ITEM, target, items),
                None => (event::EVENT_ITEM, None, Curve::Constant(items.value(0, 0)?)),
            })
        }
        _ => Err(unavailable("requires a Skip point counter or a selected item reward")),
    }
}

/// Owned arrays only. The payoff unit is one deck payoff; the engine scales it to its order mass.
pub(crate) struct DeckPayoffBounds {
    pub(crate) power: TeamPowerBounds,
    /// Visit order of members: event bonus, then the power order.
    pub(crate) members: Vec<usize>,
    member_bonus: Vec<i64>,
    /// Per Snap column (`domain.snaps()` order).
    snap_bonus: Vec<i64>,
    /// The sum of the five largest Snap bonuses.
    snap_best: i64,
    maximum_bonus: i64,
    curve: Curve,
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
        Ok(Self {
            power,
            members,
            member_bonus: vec![0; pool.members.len()],
            snap_bonus: vec![0; domain.snaps().len()],
            snap_best: 0,
            maximum_bonus: 0,
            curve: Curve::UpperOnly(cap),
        })
    }

    pub(crate) fn upper_only(&self) -> Option<i128> {
        match self.curve {
            Curve::UpperOnly(cap) => Some(cap),
            _ => None,
        }
    }

    /// With score `steps` (a played Live), the payoff is only bounded: an evaluated deck may fall below it.
    pub(crate) fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
        input: Option<&EventPayoffInput>,
        steps: Option<ScoreSteps>,
    ) -> Result<Self, Error> {
        let (bonus_type, effect_event, curve) = curve(pool, request, metric, input, steps)?;
        // The power tables certify nonnegative, nonwrapping powers (and a monotone Skip score) for every legal
        // deck. They protect native evaluation; without score steps power only breaks payoff ties.
        let power = TeamPowerBounds::tables(pool, request, domain)?;
        let effects = effect_event.map(|id| event::event_effects(pool.master, id)).unwrap_or_default();
        let mut member_bonus = vec![0; pool.members.len()];
        for &m in domain.members() {
            let card = ournotes_sim::bonus::event_member(pool.master, &pool.members[m]);
            member_bonus[m] = card_bonus(&effects, EventCard::Member(&card), bonus_type)?;
        }
        let snap_bonus = domain
            .snaps()
            .iter()
            .map(|&s| {
                card_bonus(&effects, EventCard::Snap(&ournotes_sim::bonus::event_snap(&pool.snaps[s])), bonus_type)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let maximum_bonus = nonnegative_i32(largest_team_bonus(
            domain.members().iter().map(|&m| (pool.members[m].character_id, member_bonus[m])),
            &snap_bonus,
        ))?;
        // Prove bonus + 10000 and every product safe up to the largest bonus, independently of a zero factor.
        point_product(0, maximum_bonus, 0)?;
        let root = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        curve.value(maximum_bonus, power.upper(pool, domain, &root, 0).ok_or_else(|| unavailable("no legal team"))?)?;
        let mut best = snap_bonus.clone();
        best.sort_unstable_by(|a, b| b.cmp(a));
        let snap_best = best.iter().take(5).sum();
        let mut members = power.members.clone();
        // Stable: equal bonuses keep the power order.
        members.sort_by(|&a, &b| member_bonus[b].cmp(&member_bonus[a]));
        Ok(Self { power, members, member_bonus, snap_bonus, snap_best, maximum_bonus, curve })
    }

    /// Bound every completion of the members in `SLOTS[..depth]` (no Snap is placed yet): (payoff, power), or None
    /// when the prefix has no legal completion.
    pub(crate) fn upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> Result<Option<(i128, i64)>, Error> {
        let (Some(power), Some((available, required))) =
            (self.power.upper(pool, domain, p, depth), candidates(pool, domain, p, depth))
        else {
            return Ok(None);
        };
        let mut rows = HashMap::<i64, i64>::new();
        for &m in &available {
            rows.entry(pool.members[m].character_id)
                .and_modify(|v| *v = (*v).max(self.member_bonus[m]))
                .or_insert(self.member_bonus[m]);
        }
        let bonus = SLOTS[..depth].iter().map(|&s| self.member_bonus[p.members[s]]).sum::<i64>()
            + sum_rows(&rows, &required, 5 - depth)
            + self.snap_best;
        if bonus > self.maximum_bonus {
            return Err(unavailable("prefix escaped the compiled proof domain"));
        }
        Ok(Some((self.curve.value(bonus, power)?, power)))
    }

    /// Whether an evaluated deck may pay less than its score-step or common terminal bound.
    pub(crate) fn bounded_only(&self) -> bool {
        matches!(self.curve, Curve::Steps(_) | Curve::UpperOnly(_))
    }

    /// With score steps, the score cap of every order of a deck with power at most `power`.
    pub(crate) fn score_cap(&self, power: i64) -> Option<i128> {
        let Curve::Steps(steps) = &self.curve else { return None };
        Some(steps.score_cap(power))
    }

    /// An upper bound of the power of every legal team.
    pub(crate) fn power_cap(&self, pool: &Pool, domain: &CandidateDomain) -> Option<i64> {
        self.power.upper(pool, domain, &PhysicalDeck { members: [0; 5], snaps: [None; 5] }, 0)
    }

    /// The first `k` Snap bindings of a complete member layout, ranked by curve value, then power, then Snap IDs (no
    /// Snap first), with their curve value and exact power. `base` is the layout's power without Snaps.
    ///
    /// Snaps are scanned once in public-ID order, so partial bindings of one slot mask extend identically. A binding
    /// is dropped once `k` bindings of its mask with at least its bonus beat its power and Snap IDs: every common
    /// extension keeps them ahead (the payoff is nondecreasing in bonus and power), so no dropped binding is among the
    /// layout's first `k`.
    pub(crate) fn frontier(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        base: i64,
        k: usize,
    ) -> Result<Vec<(i128, i64, PhysicalDeck)>, Error> {
        type Partial = (i64, i64, [Option<usize>; 5]);
        let w = p.members.map(|m| self.power.w[m].as_slice());
        let members: i64 = p.members.iter().map(|&m| self.member_bonus[m]).sum();
        let mut states: Vec<Vec<Partial>> = vec![Vec::new(); 32];
        states[0].push((0, 0, [None; 5]));
        for (snap, &bonus) in self.snap_bonus.iter().enumerate() {
            let mut next = states.clone();
            for (mask, rows) in states.iter().enumerate() {
                for slot in (0..5).filter(|slot| mask & (1 << slot) == 0) {
                    for &(sum, power, binding) in rows {
                        let mut binding = binding;
                        binding[slot] = Some(snap);
                        next[mask | (1 << slot)].push((sum + bonus, power + w[slot][snap], binding));
                    }
                }
            }
            for rows in &mut next {
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
            }
            states = next;
        }
        let mut out = Vec::new();
        for (sum, power, binding) in states.into_iter().flatten() {
            let bonus = members + sum;
            if bonus > self.maximum_bonus {
                return Err(unavailable("binding escaped the compiled proof domain"));
            }
            out.push((self.curve.value(bonus, base + power)?, base + power, binding));
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

    #[test]
    fn native_intermediate_wrapping_is_refused_even_at_zero_rate() {
        assert!(point_product(i64::from(i32::MAX), 0, 0).is_err());
        assert!(point_product(1, i64::from(i32::MAX), 0).is_err());
        assert!(point_product(1, 0, i64::from(i32::MAX)).is_err());
        assert!(point_product(-1, 0, 1).is_err());
        assert_eq!(point_product(100, 20000, 0).unwrap(), 0);
        assert_eq!(challenge_product(i64::from(i32::MAX), 1).unwrap(), i128::from(i32::MAX));
        assert_eq!(challenge_product(i64::from(i32::MAX), 0).unwrap(), 0);
        assert!(challenge_product(i64::from(i32::MAX), 2).is_err());
    }

    #[test]
    fn effect_casts_and_each_addition_must_fit_before_native_wrapping() {
        let member =
            event::EventMember { id: 1, character_id: 1, band_id: Some(1), card_type: 1, tags: vec![], rank: 1 };
        let mut effect = EventEffectRow {
            event_bonus_type: event::EVENT_POINT,
            resource_type_constraint: 2,
            rank1_effect_value: i64::from(i32::MAX),
            ..Default::default()
        };
        let card = EventCard::Member(&member);
        assert_eq!(card_bonus(&[&effect], card, event::EVENT_POINT).unwrap(), i64::from(i32::MAX));
        assert_eq!(card_bonus(&[&effect], card, event::EVENT_ITEM).unwrap(), 0, "another bonus type is not read");
        assert!(card_bonus(&[&effect, &effect], card, event::EVENT_POINT).is_err());
        effect.rank1_effect_value = 1i64 << 32;
        assert!(card_bonus(&[&effect], card, event::EVENT_POINT).is_err());
        effect.rank1_effect_value = -1;
        assert!(card_bonus(&[&effect], card, event::EVENT_POINT).is_err());
    }

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
            json!([{"_id":7,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}]),
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
                {"_id":11,"_resourceType":4,"_resourceId":88,"_resourceCount":3},
                {"_id":12,"_resourceType":4,"_resourceId":88,"_resourceCount":7},
                {"_id":13,"_resourceType":1,"_resourceId":5,"_resourceCount":100}
            ]),
        );
        replace_table(
            &mut data,
            "MasterChallengeLiveEventReward",
            json!([{"_id":11,"_resourceType":4,"_resourceId":88,"_resourceCount":2}]),
        );
        data
    }

    /// Every canonical team of a fixed-leader Skip domain: each member prefix bounds it, and its layout's frontier
    /// holds its exact native payoff and power, in the canonical rank order.
    fn check_exact(scenario: Scenario, metric: Metric) {
        let data = fixture();
        let master = data.master();
        let owned = roster(&mut Rng::new(739), &master);
        let input: ContextInput = serde_json::from_value(json!({
            "powerSnapshot":{"eventIds":[7],"capturedJstTicks":50},
            "resultClock":{"execution":"skip","serverNowJstTicks":99},
            "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":7,"points":0,"challengePoints":30,"added":[]}],
                "eventWindows":[{"eventId":7,"startJstTicks":90,"endJstTicks":100}],
                "selectedRewards":[{"eventId":7,"rewardId":11},{"eventId":7,"rewardId":12},{"eventId":7,"rewardId":13}]}
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
        let bound = DeckPayoffBounds::compile(&pool, &request, &domain, &metric, Some(payoff), None).unwrap();
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
            let exact = match metric {
                Metric::ClientEventPoints { event_id } => i128::from(
                    context.preview_event_points(&pool, &deck.as_deck(), payoff, event_id, score)?.points_for(event_id),
                ),
                Metric::ClientChallengePoints { event_id } => i128::from(
                    context
                        .preview_event_points(&pool, &deck.as_deck(), payoff, event_id, score)?
                        .challenge_points_for(event_id),
                ),
                Metric::ConditionalClientEventItems { event_id, resource_type, resource_id } => {
                    let items = context.preview_event_items(&pool, &deck.as_deck(), payoff, event_id, score)?;
                    ournotes_sim::scenario::item_payoff(&items, event_id, resource_type, resource_id)?
                }
                _ => unreachable!(),
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

    #[test]
    fn every_canonical_skip_team_is_bounded_and_ranked_exactly() {
        let items = Metric::ConditionalClientEventItems { event_id: 7, resource_type: 4, resource_id: 88 };
        for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
            check_exact(scenario, Metric::ClientEventPoints { event_id: 7 });
            check_exact(scenario, items.clone());
        }
        check_exact(Scenario::Free(10), Metric::ClientChallengePoints { event_id: 7 });
    }
}
