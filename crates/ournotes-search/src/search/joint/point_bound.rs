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
    /// Of a [`PointBound::ranked`] bound: the scores at which the reward of an order at a fixed event bonus may change
    /// with its score cap (see [`PointBound::reward_breaks`]); None when the reward is not piecewise constant.
    reward_breaks: Option<Vec<i64>>,
    /// A combined metric: the weight of the payoff the fields above bound, and the other terms as (weight, the largest
    /// event bonus of a team under the term, the term's bound). A term's payoff is nondecreasing in its event bonus,
    /// so every deck's payoff is at most the weighted sum of the first term at the deck's bonus and of the others at
    /// their largest bonuses.
    pub(super) weight: i128,
    pub(super) others: Vec<(i128, i64, PointBound)>,
    /// Whether the payoff this bound gives is the ranking value of an order of a certified LUCK live
    /// ([`PointBound::ranked`]).
    pub(super) ranked: bool,
}

#[derive(Clone, Copy)]
pub(super) enum ScoreTarget {
    AtLeast(i32),
    Capped(i32),
}

/// The most (power, bonus) pairs [`PointBound::ranked_payoff_sum`] reads at once; more read each order's payoffs.
const RANKED_PAIRS: usize = 32;
/// The score caps [`PointBound::ranked_payoff_sum`] compares by reward first lie within this distance of zero, so two
/// of them differ by less than [`crate::search::certified_search::REWARD_WEIGHT`].
const RANKED_CAPS: f64 = (1u64 << 38) as f64;
/// The rewards [`PointBound::ranked_payoff_sum`] reads lie within this distance of zero, so no ranking value or sum
/// of 120 of them saturates.
const RANKED_REWARDS: i128 = 1 << 80;

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
            reward_breaks: None,
            weight: 1,
            others: Vec::new(),
            ranked: false,
        }
    }

    /// The score thresholds of the tiers of this bound and of its other terms.
    pub(super) fn tier_scores(&self, scores: &mut Vec<i64>) {
        scores.extend(self.score_tiers.iter().flatten().map(|&(score, _)| score));
        for (_, _, part) in &self.others {
            part.tier_scores(scores);
        }
    }

    /// Drops the tiers above `cap`, which no order's expected score exceeds: the multiplier at any score, the concave
    /// majorants and the largest multiplier read only the tiers left. The first tier stays.
    pub(super) fn cap_scores(&mut self, cap: i64) {
        if let Some(tiers) = &mut self.score_tiers {
            let keep = tiers.partition_point(|&(score, _)| score <= cap).max(1);
            tiers.truncate(keep);
            self.multiplier = tiers.iter().map(|&(_, multiplier)| multiplier).max().unwrap_or(0);
            if let Some(hulls) = &mut self.hulls {
                hulls.truncate(keep);
            }
        }
        for (_, _, part) in &mut self.others {
            part.cap_scores(cap);
        }
    }

    /// This bound for the ranking values of the orders of a certified LUCK live
    /// ([`crate::search::certified_search::REWARD_WEIGHT`]): an order earns its reward at the integer part of its
    /// expected score and ranks by `reward * REWARD_WEIGHT + expected score`. The score caps
    /// then bound expected scores (every lottery path's score bounds its expectation too), so the reward at a cap
    /// bounds the reward of each order as a deterministic score would, and the cap bounds the expected score. The
    /// ranking value steps nowhere, so each order reads its own cap.
    pub(super) fn ranked(&self) -> Self {
        Self { step_scores: None, reward_breaks: self.reward_breaks(), ranked: true, ..self.clone() }
    }

    /// The scores at which the reward of an order at a fixed event bonus may change with its score cap: the reward
    /// is constant from each one up to the next, below the first and from the last on. A score target steps at its
    /// threshold, a tiered payoff at its tiers' scores and a term without tiers nowhere. None with a capped score
    /// term, whose reward grows with the cap. Dropping tiers ([`PointBound::cap_scores`]) only removes changes, so
    /// the breaks of the bound before it still hold.
    fn reward_breaks(&self) -> Option<Vec<i64>> {
        let mut breaks = match self.target {
            Some(ScoreTarget::AtLeast(threshold)) => vec![i64::from(threshold)],
            Some(ScoreTarget::Capped(_)) => return None,
            None => self.score_tiers.iter().flatten().map(|&(score, _)| score).collect(),
        };
        for (_, _, part) in &self.others {
            breaks.extend(part.reward_breaks()?);
        }
        breaks.sort_unstable();
        breaks.dedup();
        Some(breaks)
    }

    /// The ranking value of an order with reward `reward` whose (expected) score is at most `score_cap`: the reward
    /// itself unless the bound is [`PointBound::ranked`].
    fn rank(&self, reward: i128, score_cap: i128) -> i128 {
        if !self.ranked {
            return reward;
        }
        reward.saturating_mul(crate::search::certified_search::REWARD_WEIGHT).saturating_add(score_cap)
    }

    /// The point payoff of an order of a deck with this event bonus at a score cap without a score target: the step
    /// multiplier at the cap.
    fn tier_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        ((bonus + 10000) * self.multiplier_at(score_cap) / 10000) as i128
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
                let bonus = crate::search::bonus::largest_team_bonus(
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

    /// The largest payoff of an order of a deck with this event bonus, the other terms at the event bonuses `others`
    /// (see [`PointBound::mean_payoff_with`]).
    pub(super) fn maximum_with(&self, bonus: i64, others: &[i64]) -> i128 {
        self.rank(self.maximum_reward(bonus, others), i128::from(i32::MAX))
    }

    /// The largest reward of an order of a deck with this event bonus, the other terms at `others`.
    fn maximum_reward(&self, bonus: i64, others: &[i64]) -> i128 {
        let own = match self.target {
            Some(_) => i128::from(self.multiplier),
            None => ((bonus + 10000) * self.multiplier / 10000) as i128,
        };
        self.others.iter().enumerate().fold(self.weight.saturating_mul(own), |sum, (i, (weight, largest, part))| {
            let bonus = others.get(i).copied().unwrap_or(*largest);
            sum.saturating_add(weight.saturating_mul(part.maximum_reward(bonus, &[])))
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
    /// score target, each order reads the pairs. A ranked bound reads each pair's rewards over its range of caps once
    /// ([`PointBound::ranked_payoff_sum`]).
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
            if self.ranked
                && let Some(breaks) = &self.reward_breaks
                && let Some(sum) = self.ranked_payoff_sum(breaks, pairs, factors, steps)
            {
                return Some(sum);
            }
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

    /// [`PointBound::order_payoff_sum`] of a ranked bound whose rewards change only at `breaks`, or None when it
    /// reads each order's payoffs instead. The caps of a pair over the factors lie between its caps at the smallest
    /// and the largest factor, `ceil(power * factor)` being monotone in the factor; across that range its reward
    /// changes only at the breaks inside it, so its segments (from the lower cap and from each such break on) hold
    /// its rewards, read once. The ranking value `reward * REWARD_WEIGHT + cap` of an order is then largest at the
    /// pair with the largest reward, and among those at the largest cap, as long as two caps differ by less than
    /// the weight ([`RANKED_CAPS`]) and nothing saturates ([`RANKED_REWARDS`]); `ceil` keeps the order of the
    /// products, so the pairs compare by the rounded product and only the best one's is converted. A pair whose
    /// largest reward is below another's smallest never ranks first. Without pairs, with a factor that is not a
    /// number, with more pairs than [`RANKED_PAIRS`] or outside those limits each order reads its payoffs. `segments`
    /// is scratch space.
    fn ranked_payoff_sum(
        &self,
        breaks: &[i64],
        pairs: &[(i64, i64)],
        factors: &[f64],
        segments: &mut Vec<(f64, i128)>,
    ) -> Option<i128> {
        if pairs.is_empty() || pairs.len() > RANKED_PAIRS {
            return None;
        }
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for &factor in factors {
            if factor.is_nan() {
                return None;
            }
            (low, high) = (low.min(factor), high.max(factor));
        }
        // Per pair: its power, its segments as (the first rounded product, the reward) in `segments[start..end]` and
        // its largest reward.
        let mut read = [(0.0, 0, 0, 0); RANKED_PAIRS];
        let mut floor = i128::MIN;
        segments.clear();
        for (slot, &(power, bonus)) in read.iter_mut().zip(pairs) {
            let power = power as f64;
            let (a, b) = ((power * low).ceil(), (power * high).ceil());
            if !(a.abs() < RANKED_CAPS && b.abs() < RANKED_CAPS) {
                return None;
            }
            let (lo, hi) = (a.min(b) as i64, a.max(b) as i64);
            let start = segments.len();
            segments.push((f64::NEG_INFINITY, self.order_reward(bonus, i128::from(lo))));
            let next = breaks.partition_point(|&score| score <= lo);
            for &score in breaks[next..].iter().take_while(|&&score| score <= hi) {
                segments.push((score as f64, self.order_reward(bonus, i128::from(score))));
            }
            let rewards = segments[start..].iter().map(|&(_, reward)| reward);
            let (least, most) = rewards.fold((i128::MAX, i128::MIN), |(l, m), r| (l.min(r), m.max(r)));
            if least <= -RANKED_REWARDS || most >= RANKED_REWARDS {
                return None;
            }
            floor = floor.max(least);
            *slot = (power, start, segments.len(), most);
        }
        // The pairs that may rank first.
        let mut count = 0;
        for at in 0..pairs.len() {
            if read[at].3 >= floor {
                read[count] = read[at];
                count += 1;
            }
        }
        let read = &read[..count];
        let mut total = 0i128;
        for &factor in factors {
            let (mut reward, mut product) = (i128::MIN, f64::NEG_INFINITY);
            for &(power, start, end, _) in read {
                let x = (power * factor).ceil();
                let mut at = start;
                while at + 1 < end && segments[at + 1].0 <= x {
                    at += 1;
                }
                let r = segments[at].1;
                if r > reward || (r == reward && x > product) {
                    (reward, product) = (r, x);
                }
            }
            total += reward * crate::search::certified_search::REWARD_WEIGHT + i128::from(product as i64);
        }
        Some(total)
    }

    /// The payoff of one performance order of a deck with this event bonus whose score is at most `score_cap`.
    pub(super) fn order_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        self.rank(self.order_reward(bonus, score_cap), score_cap)
    }

    /// The reward of one performance order of a deck with this event bonus whose score is at most `score_cap`.
    fn order_reward(&self, bonus: i64, score_cap: i128) -> i128 {
        let own = self.own_order_payoff(bonus, score_cap);
        if !self.is_combined() {
            return own;
        }
        self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, bonus, part)| {
            sum.saturating_add(weight.saturating_mul(part.order_reward(*bonus, score_cap)))
        })
    }

    /// The payoff of one performance order of a complete team, whose member and choice pairs are `pairs`, whose score
    /// is at most `score_cap`: [`PointBound::order_payoff`] with every term at the team's own event bonus under that
    /// term, in place of the largest bonus of a team.
    pub(super) fn team_order_payoff(&self, pairs: &[(usize, usize); 5], score_cap: i128) -> i128 {
        self.rank(self.team_order_reward(pairs, score_cap), score_cap)
    }

    /// [`PointBound::team_order_payoff`] of the rewards.
    fn team_order_reward(&self, pairs: &[(usize, usize); 5], score_cap: i128) -> i128 {
        let bonus = pairs.iter().map(|&(m, choice)| self.pair_bonus(m, choice)).sum();
        let own = self.own_order_payoff(bonus, score_cap);
        if !self.is_combined() {
            return own;
        }
        self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, _, part)| {
            sum.saturating_add(weight.saturating_mul(part.team_order_reward(pairs, score_cap)))
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
    /// is constant. A combined payoff sums the bounds of its terms, each a bound of the term's mean. A
    /// [`PointBound::ranked`] bound reads expected scores as scores and adds the mean expected score's cap, so it
    /// bounds the mean ranking value.
    #[cfg(any(test, feature = "search-diagnostics"))]
    pub(super) fn mean_payoff(&self, bonus: i64, score_cap: i128, max_cap: i128) -> i128 {
        self.rank(self.mean_reward(bonus, score_cap, max_cap), score_cap)
    }

    /// [`PointBound::mean_payoff`] of the rewards.
    fn mean_reward(&self, bonus: i64, score_cap: i128, max_cap: i128) -> i128 {
        self.mean_reward_with(bonus, &[], score_cap, max_cap)
    }

    /// [`PointBound::mean_payoff`] with the other terms of a combined metric at the event bonuses `others`, one per
    /// term in their order, each at least the bonus under the term of every deck it bounds; a term past the end of
    /// `others` reads its largest bonus of a team.
    pub(super) fn mean_payoff_with(&self, bonus: i64, others: &[i64], score_cap: i128, max_cap: i128) -> i128 {
        self.rank(self.mean_reward_with(bonus, others, score_cap, max_cap), score_cap)
    }

    /// [`PointBound::mean_payoff_with`] of the rewards.
    fn mean_reward_with(&self, bonus: i64, others: &[i64], score_cap: i128, max_cap: i128) -> i128 {
        let own = self.own_mean_payoff(bonus, score_cap, max_cap);
        if !self.is_combined() {
            return own;
        }
        self.others.iter().enumerate().fold(self.weight.saturating_mul(own), |sum, (i, (weight, largest, part))| {
            let bonus = others.get(i).copied().unwrap_or(*largest);
            sum.saturating_add(weight.saturating_mul(part.mean_reward(bonus, score_cap, max_cap)))
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
        let reward = if !self.is_combined() {
            own
        } else {
            self.others.iter().fold(self.weight.saturating_mul(own), |sum, (weight, bonus, part)| {
                sum.saturating_add(weight.saturating_mul(part.order_reward(*bonus, score_cap)))
            })
        };
        self.rank(reward, score_cap)
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
        use crate::search::bonus::{card_bonus, point_product};
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
        let maximum_bonus = crate::search::bonus::largest_team_bonus(
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
            || crate::search::caps::negative_scores_unpaid(pool, context, input, |rank| {
                route.value_at_rank(pool.master, rank).is_ok()
            });
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
        let maximum_bonus = crate::search::bonus::largest_team_bonus(
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
