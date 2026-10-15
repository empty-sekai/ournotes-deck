//! Production aggregation of certified random-score laws over all 120 performer orders.
//!
//! The reward of a performance order is its payoff at the order's expected score: the step that holds the expected
//! score, the indicator of a target the expected score meets, or the expected score capped. A team ranks by its
//! ranking value ([`REWARD_WEIGHT`]): by its reward first and then, for steps and targets, by its expected score.
use super::{
    expectation::ExactExpectation,
    interval_topk::{compare_exact, compare_exact_real, exact_in_interval},
    uniform,
};
use ournotes_sim::{Error, live::certified::F64Interval};

/// The weight of the reward in the ranking value of an order, `reward * REWARD_WEIGHT + expected score`, for reward
/// steps and score targets, whose rewards are integers. 2^39 exceeds 120 times the span of the Int32 scores, so a team
/// whose reward is larger by at least one 120th ranks first, and teams with equal rewards rank by their expected
/// scores. The ranking value of the score and of the capped score is the reward itself.
pub const REWARD_WEIGHT: i128 = 1 << 39;

#[derive(Clone, Debug)]
pub struct OrderScoreInterval {
    pub order: [usize; 5],
    pub mean: F64Interval,
    pub support: (i32, i32),
    pub exact_mean: Option<ExactExpectation>,
    pub final_life: Option<(i32, i32)>,
    /// The exact expected terminal life over the order's lottery paths, once its complete law is known.
    pub exact_life: Option<ExactExpectation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayoffStep {
    pub lower: i32,
    pub upper: i32,
    pub value: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayoffMap {
    Score,
    ScoreAtLeast {
        threshold: i32,
    },
    CappedScore {
        threshold: i32,
    },
    ScoreAndLifeAtLeast {
        threshold: i32,
        min_final_life: i32,
    },
    /// The adapter establishes the exact native payoff on each closed integer interval. Steps must be ordered,
    /// disjoint and cover every score in the candidate's support. EP, CP and item tables may be nonmonotonic.
    NativeSteps(Vec<PayoffStep>),
}

#[derive(Clone, Debug)]
pub struct CertifiedEvaluation {
    /// The aggregate of the 120 order scores under the evaluation's [`Reduction`].
    pub score: F64Interval,
    /// The aggregate of the orders' ranking values ([`REWARD_WEIGHT`]), which ranks the team.
    pub payoff: F64Interval,
    pub exact_score: Option<ExactExpectation>,
    pub exact_payoff: Option<ExactExpectation>,
    /// The aggregate of the orders' rewards.
    pub reward: F64Interval,
    pub exact_reward: Option<ExactExpectation>,
    pub orders: Vec<OrderScoreInterval>,
    /// Per order of `orders`: whether it can still decide the aggregate. Every order does under
    /// [`Reduction::Mean`]; under [`Reduction::Maximum`] an order whose ranking value's upper bound lies strictly
    /// below that of another order cannot be the maximum.
    pub contending: Vec<bool>,
    /// The order of `orders` proved to attain the maximum: the first whose exact ranking value is the maximum, or the
    /// only order that can still attain it. Only [`Reduction::Maximum`] has one.
    pub best_order: Option<[usize; 5]>,
}

/// How the rankings of the 120 performance orders become the ranking of a team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reduction {
    /// The mean over the 120 orders.
    Mean,
    /// The order whose ranking value is the greatest.
    Maximum,
}

/// All cached and refined order labels use this same complete-performer basis. Paired support remains
/// inside each performer; request-local context, power and payoff identity are separately fixed by Engine.
pub(super) fn canonicalize_performers(input: &mut super::expectation::FiniteSeedContext) -> Vec<u8> {
    let mut performers: Vec<_> = input.performers.iter().map(|p| (format!("{p:?}"), p.clone())).collect();
    performers.sort_by(|a, b| a.0.cmp(&b.0));
    let keys: Vec<_> = performers.iter().map(|p| &p.0).collect();
    let program = format!("uniform120/full-performers/{keys:?}").into_bytes();
    input.performers = performers.into_iter().map(|p| p.1).collect::<Vec<_>>().try_into().expect("five performers");
    program
}

fn invalid(message: &str) -> Error {
    Error::Domain(format!("certified evaluation: {message}"))
}
fn fraction(numerator: i128) -> ExactExpectation {
    ExactExpectation { numerator, denominator: 1 }
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Exact arithmetic is optional metadata: a representational overflow drops the metadata, never the enclosure.
fn add_exact(a: ExactExpectation, b: ExactExpectation) -> Option<ExactExpectation> {
    if a.denominator == 0 || b.denominator == 0 {
        return None;
    }
    let g = gcd(a.denominator, b.denominator);
    let (am, bm) = (b.denominator / g, a.denominator / g);
    let n = a
        .numerator
        .checked_mul(i128::try_from(am).ok()?)?
        .checked_add(b.numerator.checked_mul(i128::try_from(bm).ok()?)?)?;
    if n == 0 {
        return Some(fraction(0));
    }
    let d = a.denominator.checked_mul(am)?;
    let g = gcd(n.unsigned_abs(), d);
    Some(ExactExpectation { numerator: n.checked_div(i128::try_from(g).ok()?)?, denominator: d / g })
}
fn scale_exact(a: ExactExpectation, value: i128) -> Option<ExactExpectation> {
    let g = gcd(value.unsigned_abs(), a.denominator);
    let divisor = i128::try_from(g).ok()?;
    Some(ExactExpectation { numerator: a.numerator.checked_mul(value / divisor)?, denominator: a.denominator / g })
}
fn average_exact(a: ExactExpectation) -> Option<ExactExpectation> {
    let g = gcd(a.numerator.unsigned_abs(), uniform::ORDERS as u128);
    Some(ExactExpectation {
        numerator: a.numerator / g as i128,
        denominator: a.denominator.checked_mul(uniform::ORDERS as u128 / g)?,
    })
}

impl OrderScoreInterval {
    pub fn validate(&self) -> Result<(), Error> {
        let mut order = self.order;
        order.sort_unstable();
        if order != [0, 1, 2, 3, 4]
            || self.support.0 > self.support.1
            || !self.mean.lower().is_finite()
            || !self.mean.upper().is_finite()
            || self.mean.upper() < self.support.0 as f64
            || self.mean.lower() > self.support.1 as f64
        {
            return Err(invalid("invalid performance order or score enclosure"));
        }
        if self.exact_mean.is_some_and(|v| v.denominator == 0)
            || self.exact_life.is_some_and(|v| v.denominator == 0)
            || self.final_life.is_some_and(|(lo, hi)| lo > hi)
        {
            return Err(invalid("invalid exact mean or life support"));
        }
        if let Some(exact) = self.exact_mean
            && !exact_in_interval(exact, self.mean)?
        {
            return Err(invalid("exact mean lies outside its enclosure"));
        }
        Ok(())
    }
}

fn exact_enclosure(value: ExactExpectation) -> Option<F64Interval> {
    let denominator = i128::try_from(value.denominator).ok()?;
    F64Interval::integer(value.numerator).divide(F64Interval::integer(denominator)).ok()
}

/// Consume only the complete, independently nominal joint law produced by the full native replay: its exact mean,
/// support and terminal life. Checked arithmetic exhaustion leaves the existing order untouched. An actual
/// contradiction between two certificates is an error; neither clipping an exact value nor averaging partial mass is
/// permitted.
pub(super) fn refine_order_with_exact_law(
    order: &mut OrderScoreInterval,
    law: &ournotes_sim::live::full::LuckExactLaw,
) -> Result<bool, Error> {
    let (mut score, mut life_mean) = (fraction(0), fraction(0));
    let (mut support, mut life) = ((i32::MAX, i32::MIN), (i32::MAX, i32::MIN));
    for atom in law.atoms() {
        if atom.score < order.support.0
            || atom.score > order.support.1
            || order.final_life.is_some_and(|(lo, hi)| atom.final_life < lo || atom.final_life > hi)
        {
            return Err(invalid("complete nominal path contradicts score/life support"));
        }
        let Some(numerator) = i128::try_from(atom.mass.numerator).ok() else {
            return Ok(false);
        };
        let mass = ExactExpectation { numerator, denominator: atom.mass.denominator };
        let Some(next) = scale_exact(mass, atom.score as i128).and_then(|v| add_exact(score, v)) else {
            return Ok(false);
        };
        let Some(next_life) = scale_exact(mass, atom.final_life as i128).and_then(|v| add_exact(life_mean, v)) else {
            return Ok(false);
        };
        (score, life_mean) = (next, next_life);
        support = (support.0.min(atom.score), support.1.max(atom.score));
        life = (life.0.min(atom.final_life), life.1.max(atom.final_life));
    }
    if support.0 > support.1 {
        return Err(invalid("empty exact nominal law"));
    }
    let Some(score_bounds) = exact_enclosure(score) else {
        return Ok(false);
    };
    if !exact_in_interval(score, order.mean)? {
        return Err(invalid("complete nominal expectation contradicts prior enclosure"));
    }
    if let Some(previous) = order.exact_mean
        && compare_exact(previous, score)? != std::cmp::Ordering::Equal
    {
        return Err(invalid("complete nominal law changes an exact score mean"));
    }
    let mut next = order.clone();
    next.mean = order.mean.intersect(score_bounds).ok_or_else(|| invalid("inconsistent exact score enclosure"))?;
    next.exact_mean = Some(score);
    next.support = support;
    next.final_life = Some(life);
    next.exact_life = Some(life_mean);
    next.validate()?;
    *order = next;
    Ok(true)
}

/// The reward and ranking value ([`REWARD_WEIGHT`]) of one order.
struct OrderPayoff {
    rank: F64Interval,
    exact_rank: Option<ExactExpectation>,
    reward: F64Interval,
    exact_reward: Option<ExactExpectation>,
}

/// The reward of `order` at its expected score `m` under `map`, which every score of the order's support pays into:
/// the step that holds `floor(m)`, `m` itself, the indicator of `floor(m) >= threshold` (and of an expected terminal
/// life whose integer part reaches the minimum), or `min(m, threshold)`.
fn order_reward(order: &OrderScoreInterval, map: &PayoffMap) -> Result<(F64Interval, Option<ExactExpectation>), Error> {
    let exact = |value: i128| (F64Interval::integer(value), Some(fraction(value)));
    // The integer part of the expected score: exact with an exact mean, else that of each end of its enclosure.
    let floors = match order.exact_mean {
        Some(x) => {
            let floor = x.numerator.div_euclid(i128::try_from(x.denominator).map_err(|_| invalid("mean overflow"))?);
            (floor, floor)
        }
        None => (order.mean.lower().floor() as i128, order.mean.upper().floor() as i128),
    };
    let floors = (floors.0.max(i128::from(order.support.0)), floors.1.min(i128::from(order.support.1)));
    let indicator = |threshold: i32| {
        if floors.0 >= i128::from(threshold) {
            exact(1)
        } else if floors.1 < i128::from(threshold) {
            exact(0)
        } else {
            (F64Interval::new(0.0, 1.0).expect("unit interval"), None)
        }
    };
    match map {
        PayoffMap::Score => Ok((order.mean, order.exact_mean)),
        PayoffMap::ScoreAtLeast { threshold } => Ok(indicator(*threshold)),
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life } => {
            // Whether the integer part of the expected terminal life reaches the minimum: every life of the support
            // lies on one side of it, or the exact expectation decides.
            let minimum = i128::from(*min_final_life);
            let life = match (order.exact_life, order.final_life) {
                (Some(x), _) => Some(
                    x.numerator.div_euclid(i128::try_from(x.denominator).map_err(|_| invalid("life overflow"))?)
                        >= minimum,
                ),
                (None, Some((lo, _))) if i128::from(lo) >= minimum => Some(true),
                (None, Some((_, hi))) if i128::from(hi) < minimum => Some(false),
                _ => None,
            };
            Ok(match (life, indicator(*threshold)) {
                (Some(true), met) => met,
                (Some(false), _) => exact(0),
                (None, (_, Some(zero))) if zero.numerator == 0 => exact(0),
                (None, _) => (F64Interval::new(0.0, 1.0).expect("unit interval"), None),
            })
        }
        PayoffMap::CappedScore { threshold } => {
            let cap = f64::from(*threshold);
            if order.mean.lower() >= cap {
                return Ok(exact(i128::from(*threshold)));
            }
            Ok((
                F64Interval::new(order.mean.lower().min(cap), order.mean.upper().min(cap))?,
                order
                    .exact_mean
                    .map(|x| {
                        if compare_exact(x, fraction(i128::from(*threshold)))?.is_gt() {
                            Ok(fraction(i128::from(*threshold)))
                        } else {
                            Ok(x)
                        }
                    })
                    .transpose()?,
            ))
        }
        PayoffMap::NativeSteps(steps) => {
            if steps.is_empty()
                || steps.iter().any(|s| s.lower > s.upper)
                || steps.windows(2).any(|s| s[0].upper as i64 + 1 != s[1].lower as i64)
                || steps[0].lower > order.support.0
                || steps.last().unwrap().upper < order.support.1
            {
                return Err(invalid("native payoff steps must cover the entire score support without gaps"));
            }
            let values = steps
                .iter()
                .filter(|s| i128::from(s.upper) >= floors.0 && i128::from(s.lower) <= floors.1)
                .map(|s| s.value);
            let (minimum, maximum) = values.fold((i128::MAX, i128::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
            if minimum > maximum {
                return Err(invalid("native payoff steps miss the expected score"));
            }
            if minimum == maximum {
                return Ok(exact(minimum));
            }
            Ok((F64Interval::new(F64Interval::integer(minimum).lower(), F64Interval::integer(maximum).upper())?, None))
        }
    }
}

fn order_payoff(order: &OrderScoreInterval, map: &PayoffMap) -> Result<OrderPayoff, Error> {
    let (reward, exact_reward) = order_reward(order, map)?;
    if matches!(map, PayoffMap::Score | PayoffMap::CappedScore { .. }) {
        return Ok(OrderPayoff { rank: reward, exact_rank: exact_reward, reward, exact_reward });
    }
    let rank = reward.scale_integer(REWARD_WEIGHT).add(order.mean);
    let exact_rank = exact_reward
        .and_then(|r| scale_exact(r, REWARD_WEIGHT))
        .and_then(|r| order.exact_mean.and_then(|m| add_exact(r, m)));
    Ok(OrderPayoff { rank, exact_rank, reward, exact_reward })
}

/// Whether the ranking value of `order` under `map` is not yet exact, so its complete law may still decide a rank.
pub(super) fn order_rank_open(order: &OrderScoreInterval, map: &PayoffMap) -> Result<bool, Error> {
    Ok(order_payoff(order, map)?.exact_rank.is_none())
}

/// Work priority for one uniform-order enclosure; ranking is certified separately by the frontier.
pub(super) fn refinement_uncertainty(order: &OrderScoreInterval, map: &PayoffMap) -> Result<f64, Error> {
    let rank = order_payoff(order, map)?.rank;
    Ok(rank.upper() - rank.lower())
}

/// The greatest exact value among `values` when it is proved to be their maximum: every value without an exact one
/// has an upper bound at or below it. None otherwise.
fn exact_maximum(values: &[(F64Interval, Option<ExactExpectation>)]) -> Result<Option<ExactExpectation>, Error> {
    let mut best: Option<ExactExpectation> = None;
    for exact in values.iter().filter_map(|v| v.1) {
        if best.map_or(Ok(true), |b| compare_exact(exact, b).map(|o| o.is_gt()))? {
            best = Some(exact);
        }
    }
    let Some(best) = best else { return Ok(None) };
    for (bounds, exact) in values {
        if exact.is_none() && compare_exact_real(best, bounds.upper())?.is_lt() {
            return Ok(None);
        }
    }
    Ok(Some(best))
}

/// The interval from the greatest lower bound to the greatest upper bound: the enclosure of the maximum.
fn maximum_enclosure(values: impl Iterator<Item = F64Interval>) -> Result<F64Interval, Error> {
    let (mut lower, mut upper) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for value in values {
        (lower, upper) = (lower.max(value.lower()), upper.max(value.upper()));
    }
    F64Interval::new(lower, upper)
}

/// Aggregates the 120 order enclosures into one certificate under `reduction`.
pub fn aggregate_orders(
    mut orders: Vec<OrderScoreInterval>,
    map: &PayoffMap,
    reduction: Reduction,
) -> Result<CertifiedEvaluation, Error> {
    if orders.len() != uniform::ORDERS {
        return Err(invalid("all 120 performance orders required"));
    }
    for order in &orders {
        order.validate()?;
    }
    orders.sort_by_key(|order| uniform::order_index(&order.order));
    if orders.windows(2).any(|v| v[0].order == v[1].order) {
        return Err(invalid("duplicate performance order"));
    }
    let values = orders.iter().map(|order| order_payoff(order, map)).collect::<Result<Vec<_>, Error>>()?;
    let mut contending = vec![true; orders.len()];
    let mut best_order = None;
    let (score, payoff, exact_score, exact_payoff, mut reward, exact_reward);
    match reduction {
        Reduction::Mean => {
            let mean = |items: &mut dyn Iterator<Item = (F64Interval, Option<ExactExpectation>)>| {
                let (mut sum, mut exact) = (F64Interval::ZERO, Some(fraction(0)));
                for (bounds, value) in items {
                    sum = sum.add(bounds);
                    exact = exact.and_then(|a| value.and_then(|b| add_exact(a, b)));
                }
                Ok::<_, Error>((
                    sum.divide(F64Interval::integer(uniform::ORDERS as i128))?,
                    exact.and_then(average_exact),
                ))
            };
            (score, exact_score) = mean(&mut orders.iter().map(|o| (o.mean, o.exact_mean)))?;
            (payoff, exact_payoff) = mean(&mut values.iter().map(|v| (v.rank, v.exact_rank)))?;
            (reward, exact_reward) = mean(&mut values.iter().map(|v| (v.reward, v.exact_reward)))?;
        }
        Reduction::Maximum => {
            let ranks: Vec<_> = values.iter().map(|v| (v.rank, v.exact_rank)).collect();
            let means: Vec<_> = orders.iter().map(|o| (o.mean, o.exact_mean)).collect();
            let rewards: Vec<_> = values.iter().map(|v| (v.reward, v.exact_reward)).collect();
            let maximum = |values: &[(F64Interval, Option<ExactExpectation>)], label: &str| {
                let exact = exact_maximum(values)?;
                let mut bounds = maximum_enclosure(values.iter().map(|v| v.0))?;
                // The maximum is the exact value: it lies in every enclosure of it.
                if let Some(enclosure) = exact.and_then(exact_enclosure) {
                    bounds = bounds.intersect(enclosure).ok_or_else(|| invalid(label))?;
                }
                Ok::<_, Error>((bounds, exact))
            };
            (score, exact_score) = maximum(&means, "exact maximum outside its score")?;
            (payoff, exact_payoff) = maximum(&ranks, "exact maximum outside its ranking value")?;
            // The order of the greatest ranking value has the greatest reward.
            (reward, exact_reward) = maximum(&rewards, "exact maximum outside its reward")?;
            if let Some(exact) = exact_payoff {
                for (order, value) in orders.iter().zip(&values) {
                    if let Some(own) = value.exact_rank
                        && compare_exact(own, exact)?.is_eq()
                    {
                        best_order = Some(order.order);
                        break;
                    }
                }
            }
            // The maximum is at least every lower bound and at least every exact value, so an order whose upper bound
            // lies strictly below either cannot be it.
            let mut floor = f64::NEG_INFINITY;
            let mut greatest_exact: Option<ExactExpectation> = None;
            for (bounds, exact) in &ranks {
                floor = floor.max(bounds.lower());
                if let Some(exact) = *exact
                    && greatest_exact.map_or(Ok(true), |g| compare_exact(exact, g).map(|o| o.is_gt()))?
                {
                    greatest_exact = Some(exact);
                }
            }
            for (flag, (bounds, _)) in contending.iter_mut().zip(&ranks) {
                let below_exact = match greatest_exact {
                    Some(exact) => compare_exact_real(exact, bounds.upper())?.is_gt(),
                    None => false,
                };
                *flag = !(bounds.upper() < floor || below_exact);
            }
            // Every other order lies strictly below the maximum, so the only order that can attain it does.
            if best_order.is_none() {
                let mut open = contending.iter().enumerate().filter(|(_, flag)| **flag);
                if let (Some((index, _)), None) = (open.next(), open.next()) {
                    best_order = Some(orders[index].order);
                }
            }
        }
    }
    if matches!(map, PayoffMap::ScoreAtLeast { .. } | PayoffMap::ScoreAndLifeAtLeast { .. }) {
        // Outward summation/division can extend a certain event a few ulps above one. The indicator's semantic range
        // is itself a proof.
        reward = reward
            .intersect(F64Interval::new(0.0, 1.0)?)
            .ok_or_else(|| invalid("indicator expectation outside probability range"))?;
    }
    Ok(CertifiedEvaluation {
        score,
        payoff,
        exact_score,
        exact_payoff,
        reward,
        exact_reward,
        orders,
        contending,
        best_order,
    })
}

/// The production scorer supplies one all-path law enclosure per order. Cancellation retains no partial value
/// masquerading as a 120-order mean; callers may retain the completed orders separately for the next refinement.
pub fn evaluate_orders(
    map: &PayoffMap,
    reduction: Reduction,
    mut score: impl FnMut([usize; 5]) -> Result<OrderScoreInterval, Error>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<CertifiedEvaluation>, Error> {
    let mut orders = Vec::with_capacity(uniform::ORDERS);
    for order in uniform::all_orders() {
        if cancelled() {
            return Ok(None);
        }
        let value = score(order)?;
        if value.order != order {
            return Err(invalid("scorer changed the requested performance order"));
        }
        orders.push(value);
        if cancelled() {
            return Ok(None);
        }
    }
    aggregate_orders(orders, map, reduction).map(Some)
}

/// The certified lottery law of one performance order.
pub type LuckLaw = std::sync::Arc<ournotes_sim::live::full::LuckDpCertifiedResult>;

/// The certified lottery law of every performance order of the formation, in [`uniform::all_orders`] order and in
/// the declared context as [`evaluate_luck_exact`] reads it. `kinds` composes them from the lottery kinds of the
/// performers when it can.
pub fn luck_order_laws(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    curves: &mut ournotes_sim::live::full::LuckDpCache,
    kinds: Option<&mut ournotes_sim::live::full::LotteryKinds<'_>>,
) -> Result<Vec<LuckLaw>, Error> {
    let setup = input.gekisou.as_ref().ok_or_else(|| invalid("LUCK requires Gekisou context"))?;
    let live = ournotes_sim::live::full::LotteryLive {
        master,
        skills,
        notes: &input.notes,
        events: &input.events,
        params: input.params,
        setup,
        play: &input.play,
        delta_times: &input.delta_times,
        ranking: input.rank_confirmations.as_deref(),
    };
    ournotes_sim::live::full::luck_order_laws(&live, &input.performers, &uniform::all_orders(), curves, kinds)
}

/// The expected native score of every performance order from one joint evaluation of the formation
/// ([`ournotes_sim::live::full::luck_exact_orders`]), in the declared context as [`evaluate_luck_context`] reads it.
/// `laws` holds the formation's [`luck_order_laws`] when the caller has them. With the evaluation, how it went
/// ([`LuckExactReport`]). None when cancelled before the evaluation starts.
pub fn evaluate_luck_exact(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    map: &PayoffMap,
    laws: Option<&[LuckLaw]>,
    curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<(CertifiedEvaluation, LuckExactReport)>, Error> {
    let setup = input.gekisou.as_ref().ok_or_else(|| invalid("LUCK requires Gekisou context"))?;
    if cancelled() {
        return Ok(None);
    }
    let orders = uniform::all_orders();
    let (deck, notes, events, play) = (&input.performers, &input.notes, &input.events, &input.play);
    let (delta, ranking) = (&input.delta_times, input.rank_confirmations.as_deref());
    let joint = match laws {
        Some(laws) => ournotes_sim::live::full::luck_exact_orders_with_laws(
            master,
            skills,
            deck,
            notes,
            events,
            input.params,
            setup,
            play,
            delta,
            ranking,
            &orders,
            laws,
            curves,
            false,
        ),
        None => ournotes_sim::live::full::luck_exact_orders(
            master,
            skills,
            deck,
            notes,
            events,
            input.params,
            setup,
            play,
            delta,
            ranking,
            &orders,
            curves,
            false,
        ),
    }?;
    let relaxed = joint.scores.iter().any(|score| score.relaxed);
    let orders = orders
        .into_iter()
        .zip(joint.scores)
        .map(|(order, score)| {
            let support = (score.final_support.lower, score.final_support.upper);
            let mean = F64Interval::new(score.final_mean.lower, score.final_mean.upper)?
                .intersect(F64Interval::new(support.0 as f64, support.1 as f64)?)
                .ok_or_else(|| invalid("LUCK mean and support disagree"))?;
            Ok(OrderScoreInterval {
                order,
                mean,
                support,
                exact_mean: (support.0 == support.1).then(|| fraction(support.0 as i128)),
                final_life: Some((score.final_life.lower, score.final_life.upper)),
                exact_life: None,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let report = LuckExactReport { alone: joint.alone, replays: joint.replays, relaxed, profile: joint.profile };
    Ok(Some((aggregate_orders(orders, map, Reduction::Mean)?, report)))
}

/// How a joint evaluation of the orders of a formation went: why its orders were not spliced from the recordings of
/// the rotations (None when they were), the replays it ran, whether the lottery paths of some order exceeded their
/// exact budget and relaxed bounds of their histories gave its score, and its work.
pub struct LuckExactReport {
    pub alone: Option<String>,
    pub replays: usize,
    pub relaxed: bool,
    pub profile: ournotes_sim::live::full::LuckExactOrdersProfile,
}

/// The actual all-path LUCK scorer in a normal native or WASM build. Performers are permuted before constructing
/// the model; external rank arrivals, clock data and every other live parameter remain the declared context.
/// `curves` shares lottery curves across the orders and with other calls.
pub fn evaluate_luck_context(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    map: &PayoffMap,
    mut curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    cancelled: impl FnMut() -> bool,
) -> Result<Option<CertifiedEvaluation>, Error> {
    let setup = input.gekisou.as_ref().ok_or_else(|| invalid("LUCK requires Gekisou context"))?;
    let mut session = ournotes_sim::live::full::LuckScoreSession::new(
        master,
        skills,
        &input.notes,
        &input.events,
        input.params,
        setup,
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
    );
    let mut cancelled = cancelled;
    let mut orders = Vec::with_capacity(uniform::ORDERS);
    for order in uniform::all_orders() {
        if cancelled() {
            return Ok(None);
        }
        let performers = order.map(|slot| input.performers[slot].clone());
        let Some(summary) = session.summary(&performers, curves.as_deref_mut(), &mut cancelled)? else {
            return Ok(None);
        };
        let support = (summary.final_support.lower, summary.final_support.upper);
        let mean = F64Interval::new(summary.final_mean.lower, summary.final_mean.upper)?
            .intersect(F64Interval::new(support.0 as f64, support.1 as f64)?)
            .ok_or_else(|| invalid("LUCK mean and support disagree"))?;
        orders.push(OrderScoreInterval {
            order,
            mean,
            support,
            exact_mean: summary.exact_constant_score.map(|s| fraction(s as i128)),
            final_life: summary.exact_final_life.map(|life| (life, life)),
            exact_life: None,
        });
        if cancelled() {
            return Ok(None);
        }
    }
    aggregate_orders(orders, map, Reduction::Mean).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn laws(mean: f64, support: (i32, i32)) -> Vec<OrderScoreInterval> {
        uniform::all_orders()
            .into_iter()
            .map(|order| OrderScoreInterval {
                order,
                mean: F64Interval::point(mean).unwrap(),
                support,
                exact_mean: Some(fraction(mean as i128)),
                final_life: None,
                exact_life: None,
            })
            .collect()
    }

    #[test]
    fn aggregated_indicators_preserve_the_closed_probability_range() {
        for threshold in [0, 25, 50, 100, 101] {
            for map in
                [PayoffMap::ScoreAtLeast { threshold }, PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: 1 }]
            {
                let mut orders = laws(50.0, (0, 100));
                for order in &mut orders {
                    order.final_life = Some((1, 1));
                }
                let evaluation = aggregate_orders(orders, &map, Reduction::Mean).unwrap();
                assert!(evaluation.reward.lower() >= 0.0 && evaluation.reward.upper() <= 1.0);
                let met = i128::from(threshold <= 50);
                assert_eq!(evaluation.exact_reward, Some(fraction(met)));
                assert_eq!(evaluation.exact_payoff, Some(fraction(met * REWARD_WEIGHT + 50)));
            }
        }
    }
    #[test]
    fn native_steps_pay_the_step_of_the_expected_score() {
        let map = PayoffMap::NativeSteps(vec![
            PayoffStep { lower: 0, upper: 49, value: 0 },
            PayoffStep { lower: 50, upper: 100, value: 100 },
        ]);
        let at = aggregate_orders(laws(50.0, (0, 100)), &map, Reduction::Mean).unwrap();
        assert_eq!(at.exact_reward, Some(fraction(100)));
        assert_eq!(at.exact_payoff, Some(fraction(100 * REWARD_WEIGHT + 50)));
        let below = aggregate_orders(ranged(|_| (49.5, 49.5, None)), &map, Reduction::Mean).unwrap();
        assert_eq!(below.exact_reward, Some(fraction(0)));
        assert!(below.exact_payoff.is_none() && below.payoff.contains(49.5));
        // An expected score that may lie on either side of the step keeps both rewards.
        let open = aggregate_orders(ranged(|_| (49.5, 50.5, None)), &map, Reduction::Mean).unwrap();
        assert!(open.exact_reward.is_none() && open.reward.contains(0.0) && open.reward.contains(100.0));
    }
    #[test]
    fn equal_rewards_rank_by_the_expected_score() {
        let map = PayoffMap::NativeSteps(vec![
            PayoffStep { lower: 0, upper: 49, value: 0 },
            PayoffStep { lower: 50, upper: 100, value: 6 },
        ]);
        let value = |mean: f64| aggregate_orders(ranged(|_| (mean, mean, None)), &map, Reduction::Mean).unwrap();
        let (high, low, missed) = (value(90.0), value(60.0), value(49.0));
        assert!(high.payoff.lower() > low.payoff.upper());
        assert!(low.payoff.lower() > missed.payoff.upper());
        assert_eq!((high.exact_reward, low.exact_reward), (Some(fraction(6)), Some(fraction(6))));
        // A larger reward ranks first whatever the scores.
        let rich = PayoffMap::NativeSteps(vec![PayoffStep { lower: 0, upper: 100, value: 7 }]);
        let rich = aggregate_orders(ranged(|_| (0.0, 0.0, None)), &rich, Reduction::Mean).unwrap();
        let poor = PayoffMap::NativeSteps(vec![PayoffStep { lower: 0, upper: i32::MAX, value: 6 }]);
        let mut top = ranged(|_| (0.0, 0.0, None));
        for order in &mut top {
            (order.mean, order.support) = (F64Interval::point(f64::from(i32::MAX)).unwrap(), (0, i32::MAX));
        }
        let poor = aggregate_orders(top, &poor, Reduction::Mean).unwrap();
        assert!(rich.payoff.lower() > poor.payoff.upper());
    }
    #[test]
    fn one_grade_is_exact_even_when_score_expectation_is_not() {
        let mut orders = laws(17.0, (10, 20));
        for order in &mut orders {
            order.exact_mean = None;
        }
        let map = PayoffMap::NativeSteps(vec![PayoffStep { lower: 0, upper: 99, value: 73 }]);
        let v = aggregate_orders(orders, &map, Reduction::Mean).unwrap();
        assert!(v.exact_score.is_none() && v.exact_payoff.is_none());
        assert_eq!(v.exact_reward, Some(fraction(73)));
    }
    #[test]
    fn nonmonotonic_native_steps_and_capped_score_pay_at_the_expected_score() {
        let map = PayoffMap::NativeSteps(vec![
            PayoffStep { lower: -10, upper: -1, value: 70 },
            PayoffStep { lower: 0, upper: 4, value: 10 },
            PayoffStep { lower: 5, upper: 10, value: 30 },
        ]);
        let reward = |s: f64| {
            if s < 0.0 {
                70.0
            } else if s < 5.0 {
                10.0
            } else {
                30.0
            }
        };
        for a in -10..=10 {
            for b in a..=10 {
                let mean = (a + b) as f64 / 2.0;
                let mut orders = laws(mean, (a, b));
                for order in &mut orders {
                    order.exact_mean = None;
                }
                let steps = aggregate_orders(orders.clone(), &map, Reduction::Mean).unwrap();
                let expected = reward(mean.floor());
                assert_eq!(steps.exact_reward, Some(fraction(expected as i128)));
                assert!(steps.reward.contains(expected) && steps.reward.upper() - steps.reward.lower() < 1e-9);
                let capped = PayoffMap::CappedScore { threshold: 3 };
                let capped = aggregate_orders(orders, &capped, Reduction::Mean).unwrap();
                assert!(capped.reward.contains(mean.min(3.0)) && capped.payoff.contains(mean.min(3.0)));
            }
        }
    }
    #[test]
    fn partial_duplicate_orders_and_unproved_life_never_become_exact() {
        let mut orders = laws(50.0, (0, 100));
        assert!(aggregate_orders(orders[..119].to_vec(), &PayoffMap::Score, Reduction::Mean).is_err());
        orders[119].order = orders[0].order;
        assert!(aggregate_orders(orders, &PayoffMap::Score, Reduction::Mean).is_err());
        let map = PayoffMap::ScoreAndLifeAtLeast { threshold: 1, min_final_life: 1 };
        let value = aggregate_orders(laws(50.0, (0, 100)), &map, Reduction::Mean).unwrap();
        assert!(value.exact_payoff.is_none() && value.exact_reward.is_none());
        assert_eq!((value.reward.lower(), value.reward.upper()), (0.0, 1.0));
    }
    /// Orders whose mean lies in `[lower, upper]` (the support is 0..=100), exact when `exact` holds a value.
    fn ranged(value: impl Fn(usize) -> (f64, f64, Option<i128>)) -> Vec<OrderScoreInterval> {
        uniform::all_orders()
            .into_iter()
            .enumerate()
            .map(|(index, order)| {
                let (lower, upper, exact) = value(index);
                OrderScoreInterval {
                    order,
                    mean: F64Interval::new(lower, upper).unwrap(),
                    support: (0, 100),
                    exact_mean: exact.map(fraction),
                    final_life: None,
                    exact_life: None,
                }
            })
            .collect()
    }
    fn maximum(orders: Vec<OrderScoreInterval>, map: &PayoffMap) -> CertifiedEvaluation {
        aggregate_orders(orders, map, Reduction::Maximum).unwrap()
    }

    #[test]
    fn the_maximum_of_exact_orders_is_the_greatest_and_names_its_first_order() {
        let exact = |i: usize| ((i % 50) as f64, Some((i % 50) as i128));
        let orders = ranged(|i| (exact(i).0, exact(i).0, exact(i).1));
        let value = maximum(orders, &PayoffMap::Score);
        assert_eq!(value.exact_payoff, Some(fraction(49)));
        assert_eq!(value.exact_score, Some(fraction(49)));
        assert_eq!(value.best_order, Some(uniform::all_orders()[49]));
        assert!(value.payoff.contains(49.0) && value.payoff.lower() > 48.9 && value.payoff.upper() < 49.1);
        assert_eq!(value.contending.iter().filter(|&&c| c).count(), 2);
        assert!(value.contending[49] && value.contending[99]);
    }
    #[test]
    fn an_exact_order_that_no_inexact_order_can_exceed_is_the_exact_maximum() {
        let orders = ranged(|i| if i == 7 { (60.0, 60.0, Some(60)) } else { (10.0, 60.0, None) });
        let value = maximum(orders, &PayoffMap::Score);
        assert_eq!(value.exact_payoff, Some(fraction(60)));
        assert_eq!(value.exact_score, Some(fraction(60)));
        assert_eq!(value.best_order, Some(uniform::all_orders()[7]));
        assert!(value.payoff.contains(60.0));
        assert!(value.payoff.upper() < 60.1);
    }
    #[test]
    fn an_inexact_order_that_may_exceed_the_exact_value_leaves_the_maximum_open() {
        let orders = ranged(|i| match i {
            7 => (60.0, 60.0, Some(60)),
            9 => (10.0, 61.0, None),
            _ => (10.0, 60.0, None),
        });
        let value = maximum(orders, &PayoffMap::Score);
        assert!(value.exact_payoff.is_none() && value.exact_score.is_none() && value.best_order.is_none());
        assert_eq!((value.payoff.lower(), value.payoff.upper()), (60.0, 61.0));
        assert_eq!((value.score.lower(), value.score.upper()), (60.0, 61.0));
    }
    #[test]
    fn orders_below_the_maximum_stop_contending() {
        let map = PayoffMap::ScoreAtLeast { threshold: 50 };
        // Mean 90 meets the target and mean 10 misses it.
        let orders = || ranged(|i| if i == 3 { (90.0, 90.0, None) } else { (10.0, 10.0, None) });
        let value = maximum(orders(), &map);
        assert_eq!(value.contending.iter().filter(|&&c| c).count(), 1);
        assert!(value.contending[3]);
        // The only order that can attain the maximum is proved to attain it, its ranking value still an interval.
        assert_eq!(value.best_order, Some(uniform::all_orders()[3]));
        assert!(value.exact_payoff.is_none());
        assert_eq!(value.exact_reward, Some(fraction(1)));
        assert!(value.payoff.contains(REWARD_WEIGHT as f64 + 90.0));
        let mean = aggregate_orders(orders(), &map, Reduction::Mean).unwrap();
        assert!(mean.contending.iter().all(|&c| c));
        assert_eq!(mean.exact_reward, Some(ExactExpectation { numerator: 1, denominator: 120 }));
        assert!(mean.best_order.is_none());
    }
    #[test]
    fn an_exact_order_removes_the_orders_whose_upper_bound_lies_below_it() {
        // The inexact orders' enclosures overlap each other, and only the exact order bounds them from below.
        let orders = ranged(|i| match i {
            0 => (50.0, 50.0, Some(50)),
            1 => (50.0, 50.0, None),
            _ => (0.0, 49.5, None),
        });
        let value = maximum(orders, &PayoffMap::Score);
        assert!(value.contending[0] && value.contending[1]);
        assert!(value.contending[2..].iter().all(|&c| !c));
    }
    #[test]
    fn the_maximum_never_lies_below_the_mean() {
        for shift in [0.0, 5.0, 20.0] {
            let orders = || {
                ranged(|i| {
                    let lower = (i % 7) as f64 * 3.0 + shift;
                    (lower, lower + (i % 5) as f64, None)
                })
            };
            let (max, mean) = (
                maximum(orders(), &PayoffMap::Score),
                aggregate_orders(orders(), &PayoffMap::Score, Reduction::Mean).unwrap(),
            );
            assert!(max.payoff.lower() >= mean.payoff.lower() && max.payoff.upper() >= mean.payoff.upper());
            assert!(max.score.lower() >= mean.score.lower());
        }
    }
}
