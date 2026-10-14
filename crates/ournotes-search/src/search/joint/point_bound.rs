//! The payoff of a performance order as a function of its score cap and the deck's event bonus.

use super::*;

#[derive(Clone)]
pub(super) struct PointBound {
    pub(super) member: Vec<i64>,
    pub(super) snap: Vec<i64>,
    pub(super) multiplier: i64,
    /// Prefix maximum reward multiplier at reachable score thresholds (solo rank rules only).
    pub(super) score_tiers: Option<Vec<(i64, i64)>>,
    /// The concave majorants of the step multiplier truncated at each tier: `hulls[k]` is that of `score_tiers[..=k]`
    /// (see `uniform::concave_majorant`), which equals the step multiplier's majorant on the scores below tier `k + 1`.
    pub(super) hulls: Option<Vec<Vec<(i64, i64)>>>,
    pub(super) target: Option<ScoreTarget>,
    /// The scores at which an order's payoff may step, when it is constant between them and zero below all of them.
    step_scores: Option<Vec<i64>>,
    /// A combined metric: the weight of the payoff the fields above bound, and the other terms as (weight, the largest
    /// event bonus of a team under the term, the term's bound). A term's payoff is nondecreasing in its event bonus,
    /// so every deck's payoff is at most the weighted sum of the first term at the deck's bonus and of the others at
    /// their largest bonuses.
    pub(super) weight: i128,
    pub(super) others: Vec<(i128, i64, PointBound)>,
    /// Whether the score caps this bound reads bound the expected score of an order over its lottery paths rather
    /// than each of its scores (see [`PointBound::weighed`]).
    pub(super) expected: bool,
}

#[derive(Clone, Copy)]
pub(super) enum ScoreTarget {
    AtLeast(i32),
    Capped(i32),
}

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
    use crate::search::deck_payoff::{card_bonus, point_product};
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
            crate::search::team_power::largest_team_bonus(members, &snaps)
        }
        None => 0,
    };
    point_product(value.unwrap_or(0).max(0), bonus, rate).ok()
}

impl PointBound {
    pub(super) fn score_target(members: usize, snaps: usize, target: ScoreTarget) -> Self {
        Self::plain(
            vec![0; members],
            vec![0; snaps],
            match target {
                ScoreTarget::AtLeast(_) => 1,
                ScoreTarget::Capped(t) => i64::from(t),
            },
            None,
            None,
            Some(target),
        )
    }

    /// The bound of one metric's payoff.
    pub(super) fn plain(
        member: Vec<i64>,
        snap: Vec<i64>,
        multiplier: i64,
        score_tiers: Option<Vec<(i64, i64)>>,
        hulls: Option<Vec<Vec<(i64, i64)>>>,
        target: Option<ScoreTarget>,
    ) -> Self {
        let step_scores = match (&score_tiers, target) {
            (Some(tiers), None) => Some(tiers.iter().map(|&(score, _)| score).collect()),
            _ => None,
        };
        Self {
            member,
            snap,
            multiplier,
            score_tiers,
            hulls,
            target,
            step_scores,
            weight: 1,
            others: Vec::new(),
            expected: false,
        }
    }

    /// This bound for score caps that bound the expected score of an order over its lottery paths, as the bounds
    /// compiled under lottery laws give. Every payoff it reads then is concave and nondecreasing in the score: a
    /// capped score is, and point tiers read the concave majorant of their whole step function, so its value at a cap
    /// of the expected score bounds the expected payoff (Jensen). None when a term is a score target, whose indicator
    /// has no such majorant below one.
    pub(super) fn weighed(&self) -> Option<Self> {
        if matches!(self.target, Some(ScoreTarget::AtLeast(_))) {
            return None;
        }
        let others = self
            .others
            .iter()
            .map(|(weight, bonus, part)| Some((*weight, *bonus, part.weighed()?)))
            .collect::<Option<_>>()?;
        Some(Self { step_scores: None, others, expected: true, ..self.clone() })
    }

    /// The point payoff of an order of a deck with this event bonus at a score cap without a score target: the step
    /// multiplier at the cap, or, for expected-score caps, the concave majorant of the whole step function there.
    fn tier_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        if !self.expected {
            return ((bonus + 10000) * self.multiplier_at(score_cap) / 10000) as i128;
        }
        match (&self.hulls, &self.score_tiers) {
            (Some(hulls), Some(_)) => crate::search::uniform::concave_value_ceil(
                hulls.last().expect("a hull per tier"),
                score_cap,
                i128::from(bonus + 10000),
                10000,
            )
            .expect("PT values in the nonwrapping domain"),
            // A step function without hulls is bounded by its largest step.
            (None, Some(_)) => ((bonus + 10000) * self.multiplier_at(i128::MAX) / 10000) as i128,
            _ => ((bonus + 10000) * self.multiplier / 10000) as i128,
        }
    }

    /// The bound of a combined metric's payoff from its terms' weights and bounds. The first term whose payoff
    /// reads an event bonus keeps its member and Snap bonuses; every other term reads the largest bonus of a team.
    pub(super) fn combined(
        pool: &Pool,
        domain: &CandidateDomain,
        mut parts: Vec<(i128, PointBound)>,
    ) -> Result<Self, Error> {
        if parts
            .iter()
            .any(|(_, part)| part.multiplier < 0 || part.score_tiers.iter().flatten().any(|&(_, value)| value < 0))
        {
            return Err(unavailable("combined payoff bounds require nonnegative term rewards"));
        }
        let reads_bonus = |part: &PointBound| {
            part.target.is_none()
                && (domain.members().iter().any(|&m| part.member[m] != 0) || part.snap.iter().any(|&b| b != 0))
        };
        let (weight, mut bound) = match parts.iter().position(|(_, part)| reads_bonus(part)) {
            Some(at) => parts.remove(at),
            None => (1, Self::plain(vec![0; pool.members.len()], vec![0; domain.snaps().len()], 0, None, None, None)),
        };
        let others: Vec<_> = parts
            .into_iter()
            .map(|(weight, part)| {
                let bonus = crate::search::team_power::largest_team_bonus(
                    domain.members().iter().map(|&m| (pool.members[m].character_id, part.member[m])),
                    &part.snap,
                );
                (weight, bonus, part)
            })
            .collect();
        // The whole payoff steps when every term steps or pays nothing at any score.
        let steps = |part: &PointBound| match (&part.step_scores, part.target) {
            (Some(scores), _) => Some(scores.clone()),
            (None, Some(ScoreTarget::AtLeast(threshold))) => Some(vec![i64::from(threshold)]),
            (None, None) if part.multiplier == 0 => Some(Vec::new()),
            _ => None,
        };
        bound.step_scores = steps(&bound).and_then(|own| {
            others.iter().try_fold(own, |mut all, (_, _, part)| {
                all.extend(steps(part)?);
                Some(all)
            })
        });
        if let Some(scores) = &mut bound.step_scores {
            scores.sort_unstable();
            scores.dedup();
        }
        bound.weight = weight;
        bound.others = others;
        Ok(bound)
    }

    /// Whether the bound covers the weighted terms of a combined metric.
    pub(super) fn is_combined(&self) -> bool {
        self.weight != 1 || !self.others.is_empty()
    }

    /// The largest payoff of an order of a deck with this event bonus.
    pub(super) fn maximum(&self, bonus: i64) -> i128 {
        let own = match self.target {
            Some(_) => i128::from(self.multiplier),
            None => ((bonus + 10000) * self.multiplier / 10000) as i128,
        };
        self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, bonus, part)| {
            sum.saturating_add(weight.saturating_mul(part.maximum(*bonus)))
        })
    }
    /// The event bonus of a member and choice.
    pub(super) fn pair_bonus(&self, m: usize, choice: usize) -> i64 {
        self.member[m] + if choice == 0 { 0 } else { self.snap[choice - 1] }
    }

    /// At least the sum over performance orders (one `factors` entry each) of the largest PT of one order over
    /// (power, bonus) pairs whose score caps are `ceil(power * factor)`. With score tiers that largest PT is a step
    /// function of the factor: a pair reaches a tier from some factor on, and then pays at least the tier's PT. The
    /// cap reaches `score` only when the rounded product exceeds `score - 1`, which needs the factor above
    /// `(score - 1) / power` less a relative rounding error, so each step starts no later than the threshold taken a
    /// few ulps low (`steps` holds the steps between the smallest and the largest factor). Without tiers, or with a
    /// score target, each order reads the pairs.
    pub(super) fn order_payoff_sum(
        &self,
        pairs: &[(i64, i64)],
        factors: &[f64],
        steps: &mut Vec<(f64, i128)>,
    ) -> Option<i128> {
        let best = |factor: f64| {
            pairs.iter().map(|&(power, bonus)| self.order_payoff(bonus, ((power as f64) * factor).ceil() as i128)).max()
        };
        let Some(scores) = &self.step_scores else {
            return factors.iter().try_fold(0i128, |total, &factor| total.checked_add(best(factor)?));
        };
        let (low, high) = factors.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &f| (l.min(f), h.max(f)));
        // A cap below every tier pays nothing.
        let mut base = 0i128;
        steps.clear();
        for &(power, bonus) in pairs {
            for &score in scores {
                let from = if score <= 0 {
                    f64::NEG_INFINITY
                } else if power > 0 {
                    (score - 1) as f64 / power as f64 * (1.0 - 4.0 * f64::EPSILON)
                } else {
                    continue;
                };
                let paid = self.order_payoff(bonus, i128::from(score));
                if from <= low {
                    base = base.max(paid);
                } else if from <= high {
                    steps.push((from, paid));
                }
            }
        }
        steps.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut run = base;
        for step in steps.iter_mut() {
            run = run.max(step.1);
            step.1 = run;
        }
        factors.iter().try_fold(0i128, |total, &factor| {
            let at = steps.partition_point(|&(from, _)| from <= factor);
            total.checked_add(if at == 0 { base } else { steps[at - 1].1 })
        })
    }

    /// The payoff of one performance order of a deck with this event bonus whose score is at most `score_cap`.
    pub(super) fn order_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        let own = self.own_order_payoff(bonus, score_cap);
        if !self.is_combined() {
            return own;
        }
        self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, bonus, part)| {
            sum.saturating_add(weight.saturating_mul(part.order_payoff(*bonus, score_cap)))
        })
    }

    /// [`PointBound::order_payoff`] of the first term alone.
    fn own_order_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        if let Some(target) = self.target {
            return match target {
                ScoreTarget::AtLeast(threshold) => i128::from(score_cap >= i128::from(threshold)),
                ScoreTarget::Capped(threshold) => score_cap.min(i128::from(threshold)),
            };
        }
        self.tier_payoff(bonus, score_cap)
    }

    /// A bound of the mean PT over the performance orders of a deck with this event bonus whose mean score is at most
    /// `score_cap` and whose score in every order is at most `max_cap`. The PT of an order is
    /// `(bonus + 10000) * m(S) / 10000` with `m` the step multiplier, which is not concave. Every score is at most
    /// `max_cap`, where `m` equals the step of the tiers at or below `max_cap`; the concave majorant `M` of that
    /// truncated step gives `E[m(S)] <= E[M(S)] <= M(E[S]) <= M(score_cap)` (Jensen, `M` non-decreasing), and the
    /// rounding goes up. The PT of every order is also at most the PT at `max_cap`. Without score tiers the multiplier
    /// is constant. A combined payoff sums the bounds of its terms, each a bound of the term's mean. For
    /// expected-score caps ([`PointBound::weighed`]) `max_cap` bounds an expectation too, and the whole step's majorant
    /// applies.
    pub(super) fn mean_payoff(&self, bonus: i64, score_cap: i128, max_cap: i128) -> i128 {
        let own = self.own_mean_payoff(bonus, score_cap, max_cap);
        if !self.is_combined() {
            return own;
        }
        self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, bonus, part)| {
            sum.saturating_add(weight.saturating_mul(part.mean_payoff(*bonus, score_cap, max_cap)))
        })
    }

    /// [`PointBound::mean_payoff`] of the first term alone.
    fn own_mean_payoff(&self, bonus: i64, score_cap: i128, max_cap: i128) -> i128 {
        if let Some(target) = self.target {
            return match target {
                // A bound on E[S] alone cannot rule out S >= threshold, especially for signed network scores; the
                // per-order cap can.
                ScoreTarget::AtLeast(threshold) => i128::from(max_cap >= i128::from(threshold)),
                // min(S,t) is nondecreasing and concave on the entire signed score domain.
                ScoreTarget::Capped(threshold) => score_cap.min(i128::from(threshold)),
            };
        }
        if self.expected {
            return self.tier_payoff(bonus, score_cap);
        }
        match (&self.hulls, &self.score_tiers) {
            (Some(hulls), Some(tiers)) => {
                let k = tiers.partition_point(|&(score, _)| i128::from(score) <= max_cap).max(1) - 1;
                crate::search::uniform::concave_value_ceil(&hulls[k], score_cap, i128::from(bonus + 10000), 10000)
                    .expect("PT values in the nonwrapping domain")
                    .min(self.own_order_payoff(bonus, max_cap))
            }
            _ => ((bonus + 10000) * self.multiplier / 10000) as i128,
        }
    }

    /// At least the payoff of an order of a deck with this event bonus whose score is at most `score_cap`: the first
    /// term's multiplier at the cap, without its score target, plus the other terms.
    pub(crate) fn cutoff_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        let own = self.tier_payoff(bonus, score_cap);
        if !self.is_combined() {
            return own;
        }
        self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, bonus, part)| {
            sum.saturating_add(weight.saturating_mul(part.order_payoff(*bonus, score_cap)))
        })
    }

    pub(super) fn multiplier_at(&self, score_cap: i128) -> i64 {
        self.score_tiers.as_ref().map_or(self.multiplier, |tiers| {
            tiers
                .iter()
                .take_while(|(score, _)| i128::from(*score) <= score_cap)
                .map(|(_, reward)| *reward)
                .max()
                .unwrap_or(0)
        })
    }

    /// Rank-dependent item rewards share the score envelope, but use EventItem effects and the item rate.
    /// Every rank cell is audited before resource filtering, including cells below a later, smaller reward.
    pub(super) fn compile_items(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        input: &EventPayoffInput,
        target: (i64, i64, i64),
        support: (i32, i32),
    ) -> Result<Self, Error> {
        use crate::search::deck_payoff::{card_bonus, point_product};
        use ournotes_sim::event::{self, EventCard, EventResultRoute};

        let (event_id, resource_type, resource_id) = target;
        let context = request.objective.context().ok_or_else(|| unavailable("items need resolved context"))?;
        let q = context.event_request(pool.master, input, event_id)?;
        if !matches!(q.route, EventResultRoute::NormalPlayed | EventResultRoute::ChallengePlayed { .. }) {
            return Err(unavailable("played item bounds require a played result route"));
        }
        let cuts = crate::search::certified_payoff::rank_cuts(pool, context, input, support)?;
        let mut ranks = Vec::with_capacity(cuts.len().saturating_sub(1));
        for cell in cuts.windows(2) {
            let lower = i32::try_from(cell[0]).map_err(|_| unavailable("item rank boundary outside Int32"))?;
            let upper = i32::try_from(cell[1] - 1).map_err(|_| unavailable("item rank boundary outside Int32"))?;
            let rank = context.event_score_rank(pool.master, input, &q.route, lower)?;
            if context.event_score_rank(pool.master, input, &q.route, upper)? != rank {
                return Err(unavailable("item rank changes inside a rank-constant partition"));
            }
            ranks.push((i64::from(lower), rank));
        }
        if !q.holding_event_ids.contains(&event_id) {
            return Ok(Self::plain(
                vec![0; pool.members.len()],
                vec![0; domain.snaps().len()],
                0,
                Some(vec![(0, 0)]),
                None,
                None,
            ));
        }
        let effects = event::event_effects(pool.master, event_id);
        let mut member = vec![0; pool.members.len()];
        for &m in domain.members() {
            let card = ournotes_sim::bonus::event_member(pool.master, &pool.members[m]);
            member[m] = card_bonus(&effects, EventCard::Member(&card), event::EVENT_ITEM)?;
        }
        let snap = domain
            .snaps()
            .iter()
            .map(|&s| {
                card_bonus(
                    &effects,
                    EventCard::Snap(&ournotes_sim::bonus::event_snap(&pool.snaps[s])),
                    event::EVENT_ITEM,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let maximum_bonus = crate::search::team_power::largest_team_bonus(
            domain.members().iter().map(|&m| (pool.members[m].character_id, member[m])),
            &snap,
        );
        let rate = if matches!(q.route, EventResultRoute::ChallengePlayed { .. }) {
            event::challenge_point_bonus(pool.master, i64::from(q.consumed_count))?[0]
        } else {
            event::boost_bonus(pool.master, i64::from(q.consumed_count))?[0]
        };
        // This checks the cast, bonus addition and both native products, even when the item rate is zero.
        point_product(0, maximum_bonus, rate)?;
        let mut tiers = Vec::with_capacity(ranks.len());
        let mut multiplier = 0;
        for (lower, rank) in ranks {
            let row = event::ranked_event_reward(pool.master, event_id, &q.route, rank)?;
            point_product(row.resource_count, maximum_bonus, rate)?;
            let value = if row.resource_type == resource_type && row.resource_id == resource_id {
                row.resource_count * rate
            } else {
                0
            };
            multiplier = multiplier.max(value);
            tiers.push((lower, value));
        }
        // Concave score majorants require nonnegative scores. Signed domains retain the largest audited reward.
        let score_tiers = (support.0 >= 0).then_some(tiers);
        let hulls = score_tiers
            .as_deref()
            .map(|tiers| (1..=tiers.len()).map(|k| crate::search::uniform::concave_majorant(&tiers[..k])).collect());
        Ok(Self::plain(member, snap, multiplier, score_tiers, hulls, None))
    }

    pub(super) fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        input: &EventPayoffInput,
        event_id: i64,
        challenge_points: bool,
        nonnegative_scores: bool,
    ) -> Result<Self, Error> {
        use ournotes_sim::event::{self, EventCard};
        let context = request.objective.context().ok_or_else(|| unavailable("PT needs resolved context"))?;
        let q = context.event_request(pool.master, input, event_id)?;
        let route = point_route::PointRoute::compile(pool.master, &q, event_id, challenge_points)?;
        let nonnegative_scores = nonnegative_scores
            || negative_scores_unpaid(pool, context, input, |rank| route.value_at_rank(pool.master, rank).is_ok());
        if !nonnegative_scores && route.value_at_rank(pool.master, event::RANK_NONE).is_err() {
            return Err(unavailable("signed score domain requires a defined NONE payoff"));
        }
        let effects = [event::event_effects(pool.master, event_id)];
        let mut member = vec![0; pool.members.len()];
        for &m in domain.members() {
            if challenge_points {
                continue;
            }
            member[m] = event::total_effect_10000(
                &effects,
                Some(EventCard::Member(&ournotes_sim::bonus::event_member(pool.master, &pool.members[m]))),
                event::EVENT_POINT,
            )? as i64;
        }
        let snap = domain
            .snaps()
            .iter()
            .map(|&s| {
                if challenge_points {
                    return Ok(0);
                }
                event::total_effect_10000(
                    &effects,
                    Some(EventCard::Snap(&ournotes_sim::bonus::event_snap(&pool.snaps[s]))),
                    event::EVENT_POINT,
                )
                .map(i64::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if member.iter().chain(&snap).any(|&v| v < 0) {
            return Err(unavailable("negative PT bonus outside relaxation domain"));
        }
        let group = pool
            .master
            .live_music(context.resolved.live_music_id)
            .ok_or_else(|| unavailable("missing music"))?
            .live_score_rank_group;
        let multiplayer = matches!(
            context.scenario,
            ournotes_sim::scenario::Scenario::Battle(_) | ournotes_sim::scenario::Scenario::Arena(_)
        );
        let room = if multiplayer { input.multiplayer_score_policy.as_ref() } else { None };
        // Include score zero and every local-score preimage of a native result-rank threshold. Room totals must
        // be calculated for each outcome; a rank at mean score is not the mean of the per-order PT payoff.
        let mut scores = vec![0];
        scores.extend(
            event::rank_rows_of_group(pool.master, group)
                .iter()
                .map(|r| match room {
                    Some(policy) => {
                        let threshold =
                            event::battle_required_score(r.battle_live_required_score, policy.players()) as i64;
                        match *policy {
                            ournotes_sim::scenario::MultiplayerScorePolicy::SameScore { players } => {
                                (threshold.max(0) + players - 1) / players
                            }
                            ournotes_sim::scenario::MultiplayerScorePolicy::FixedOthersAverage { players, score } => {
                                (threshold - (players - 1) * score as i64).max(0)
                            }
                        }
                    }
                    None => r.required_score,
                })
                .filter(|&v| (0..=i32::MAX as i64).contains(&v)),
        );
        scores.sort_unstable();
        scores.dedup();
        let mut values = Vec::new();
        for &score in &scores {
            let rank = match room {
                Some(policy) => {
                    let total = match *policy {
                        ournotes_sim::scenario::MultiplayerScorePolicy::SameScore { players } => score * players,
                        ournotes_sim::scenario::MultiplayerScorePolicy::FixedOthersAverage { players, score: peer } => {
                            score + (players - 1) * peer as i64
                        }
                    };
                    event::battle_score_rank(pool.master, group, total, policy.players())
                }
                None => event::score_rank(pool.master, group, score)?,
            };
            values.push(route.value_at_rank(pool.master, rank)?);
        }
        if multiplayer || !nonnegative_scores {
            values.extend(signed_network_reward_values(pool.master, &route, group));
        }
        let rate = route.rate();
        if !(0..=i32::MAX as i64).contains(&rate) || values.iter().any(|v| !(0..=i32::MAX as i64).contains(v)) {
            return Err(unavailable("PT rate/value outside nonwrapping domain"));
        }
        let maximum_bonus = crate::search::team_power::largest_team_bonus(
            domain.members().iter().map(|&m| (pool.members[m].character_id, member[m])),
            &snap,
        );
        let multiplier = rate
            .checked_mul(values.iter().copied().max().unwrap_or(0))
            .ok_or_else(|| unavailable("PT product overflow"))?;
        let base = maximum_bonus + 10000;
        if (challenge_points && multiplier > i32::MAX as i64)
            || (!challenge_points
                && (base > i32::MAX as i64
                    || base.checked_mul(rate).is_none_or(|v| v > i32::MAX as i64)
                    || base.checked_mul(multiplier).is_none_or(|v| v > i32::MAX as i64)))
        {
            return Err(unavailable("PT intermediate wrapping requires exhaustive fallback"));
        }
        // Score-tier hulls require a certified nonnegative score domain and, for
        // multiplayer, a declared nonnegative room-score mapping. A signed score
        // domain uses the maximum defined reward over the native rank domain.
        let signed = context.rank_confirmations.iter().flatten().any(|c| c.percent < 0);
        let score_tiers: Option<Vec<(i64, i64)>> = (nonnegative_scores
            && (!multiplayer || (room.is_some() && !signed)))
            .then(|| scores.into_iter().zip(values).map(|(s, v)| (s, v * rate)).collect());
        let hulls = score_tiers
            .as_deref()
            .map(|tiers| (1..=tiers.len()).map(|k| crate::search::uniform::concave_majorant(&tiers[..k])).collect());
        Ok(Self::plain(member, snap, multiplier, score_tiers, hulls, None))
    }
}

/// Whether no negative local score has a native result: the local score -1 has no rank (a solo score below every
/// threshold is a game error), or its room total takes NONE and `paid` holds for no NONE reward. Ranks walk the
/// thresholds a score or total reaches, so every smaller score takes no higher rank, and NONE is the least. Settlement
/// then reports a Game error for every negative score, and every team with a result scores at least zero in every
/// play, whatever its factor margin. False when the request maps no local score to a total.
pub(super) fn negative_scores_unpaid(
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

pub(super) fn signed_network_reward_values(
    master: &ournotes_sim::master::Master,
    route: &point_route::PointRoute,
    group: i64,
) -> Vec<i64> {
    use ournotes_sim::event;
    let mut ranks = vec![event::RANK_NONE];
    ranks.extend(
        event::rank_rows_of_group(master, group)
            .iter()
            .map(|row| if row.live_score_rank == event::RANK_E { event::RANK_D } else { row.live_score_rank }),
    );
    ranks.sort_unstable();
    ranks.dedup();
    // A missing native reward row has no terminal payoff to bound: exact settlement still reports its Game error.
    // Defined NONE rows must be included even though the first ordinary threshold is at score zero.
    ranks.into_iter().filter_map(|rank| route.value_at_rank(master, rank).ok()).collect()
}
