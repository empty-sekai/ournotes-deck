//! Production aggregation of certified random-score laws over all 120 performer orders.
//! Native step payoffs consume tail probabilities, never a native payoff evaluated at the mean score.
use super::{
    expectation::ExactExpectation,
    interval_topk::{compare_exact, compare_exact_real, exact_in_interval},
    uniform,
};
use ournotes_sim::{Error, live::certified::F64Interval};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct TailProbability {
    pub bounds: F64Interval,
    pub exact: Option<ExactExpectation>,
}

#[derive(Clone, Debug)]
pub struct OrderScoreInterval {
    pub order: [usize; 5],
    pub mean: F64Interval,
    pub support: (i32, i32),
    pub exact_mean: Option<ExactExpectation>,
    pub final_life: Option<(i32, i32)>,
    /// Certified P(score >= threshold). Missing thresholds get valid support/first-moment bounds.
    pub tails: BTreeMap<i32, TailProbability>,
    /// A provider may refine a truncated moment or joint score/life event without expanding every score atom.
    pub refined_payoff: Option<PayoffRefinement>,
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
pub struct PayoffRefinement {
    pub map: PayoffMap,
    pub bounds: F64Interval,
    pub exact: Option<ExactExpectation>,
}

#[derive(Clone, Debug)]
pub struct BoundaryRefinement {
    pub order_index: usize,
    pub thresholds: Vec<i32>,
    pub joint_life: bool,
    /// Capped-score refinement needs E[min(S,T)], not merely P(S>=T).
    pub truncated_at: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct CertifiedEvaluation {
    /// The aggregate of the 120 order scores under the evaluation's [`Reduction`].
    pub score: F64Interval,
    pub payoff: F64Interval,
    pub exact_score: Option<ExactExpectation>,
    pub exact_payoff: Option<ExactExpectation>,
    pub orders: Vec<OrderScoreInterval>,
    pub refinements: Vec<BoundaryRefinement>,
    /// Per order of `orders`: whether it can still decide the aggregate. Every order does under
    /// [`Reduction::Mean`]; under [`Reduction::Maximum`] an order whose payoff upper bound lies strictly below the
    /// payoff of another order cannot be the maximum.
    pub contending: Vec<bool>,
    /// The order of `orders` proved to attain the maximum: the first whose exact payoff is the maximum, or the only
    /// order that can still attain it. Only [`Reduction::Maximum`] has one.
    pub best_order: Option<[usize; 5]>,
}

/// How the payoffs of the 120 performance orders become the payoff of a team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reduction {
    /// The mean payoff over the 120 orders.
    Mean,
    /// The payoff of the order whose expected payoff is the greatest.
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
        if self.exact_mean.is_some_and(|v| v.denominator == 0) || self.final_life.is_some_and(|(lo, hi)| lo > hi) {
            return Err(invalid("invalid exact mean or life support"));
        }
        if let Some(exact) = self.exact_mean
            && !exact_in_interval(exact, self.mean)?
        {
            return Err(invalid("exact mean lies outside its enclosure"));
        }
        for (&threshold, tail) in &self.tails {
            let prior = self.moment_tail(threshold)?;
            if tail.bounds.lower() < 0.0 || tail.bounds.upper() > 1.0 || prior.intersect(tail.bounds).is_none() {
                return Err(invalid("tail contradicts probability/support/moment bounds"));
            }
            if let Some(exact) = tail.exact {
                if !exact_in_interval(exact, tail.bounds)? {
                    return Err(invalid("exact tail lies outside its enclosure"));
                }
                if compare_exact(exact, fraction(0))?.is_lt() || compare_exact(exact, fraction(1))?.is_gt() {
                    return Err(invalid("exact tail outside [0,1]"));
                }
            }
        }
        let mut previous = 1.0f64;
        for tail in self.tails.values() {
            if tail.bounds.lower() > previous {
                return Err(invalid("nonmonotonic score tail bounds"));
            }
            previous = previous.min(tail.bounds.upper());
        }
        Ok(())
    }

    fn moment_tail(&self, threshold: i32) -> Result<F64Interval, Error> {
        let (lo, hi) = self.support;
        if threshold <= lo {
            return Ok(F64Interval::ONE);
        }
        if threshold > hi {
            return Ok(F64Interval::ZERO);
        }
        // E[S] >= L + P(S>=T)*(T-L), and E[S] <= (T-1) + P(S>=T)*(U-(T-1)).
        let low = self
            .mean
            .subtract(F64Interval::integer(threshold as i128 - 1))
            .divide(F64Interval::integer(hi as i128 - threshold as i128 + 1))?;
        let high = self
            .mean
            .subtract(F64Interval::integer(lo as i128))
            .divide(F64Interval::integer(threshold as i128 - lo as i128))?;
        F64Interval::new(low.lower().clamp(0.0, 1.0), high.upper().clamp(0.0, 1.0))
    }

    pub fn tail(&self, threshold: i32) -> Result<TailProbability, Error> {
        let moment = self.moment_tail(threshold)?;
        match self.tails.get(&threshold) {
            Some(tail) => Ok(TailProbability {
                bounds: moment.intersect(tail.bounds).ok_or_else(|| invalid("tail contradicts moment bound"))?,
                exact: tail.exact,
            }),
            None => Ok(TailProbability {
                bounds: moment,
                exact: if threshold <= self.support.0 {
                    Some(fraction(1))
                } else if threshold > self.support.1 {
                    Some(fraction(0))
                } else {
                    None
                },
            }),
        }
    }

    /// Install a certified boundary refinement. The provider must prove it for the same full random score law.
    pub fn refine_tail(&mut self, threshold: i32, refined: TailProbability) -> Result<(), Error> {
        let old = self.tail(threshold)?;
        if old.bounds.intersect(refined.bounds) != Some(refined.bounds) {
            return Err(invalid("tail refinement widens bounds"));
        }
        if let (Some(a), Some(b)) = (old.exact, refined.exact)
            && compare_exact(a, b)? != std::cmp::Ordering::Equal
        {
            return Err(invalid("tail refinement changes an exact value"));
        }
        let mut next = self.clone();
        next.tails.insert(threshold, TailProbability { bounds: refined.bounds, exact: old.exact.or(refined.exact) });
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn refine_payoff(&mut self, refined: PayoffRefinement) -> Result<(), Error> {
        if let Some(exact) = refined.exact
            && !exact_in_interval(exact, refined.bounds)?
        {
            return Err(invalid("exact payoff lies outside its enclosure"));
        }
        let prior = order_payoff(self, &refined.map)?;
        if prior.bounds.intersect(refined.bounds) != Some(refined.bounds)
            || refined.exact.is_some_and(|v| v.denominator == 0)
        {
            return Err(invalid("payoff refinement widens its bounds or has invalid exact metadata"));
        }
        if let (Some(a), Some(b)) = (prior.exact, refined.exact)
            && compare_exact(a, b)? != std::cmp::Ordering::Equal
        {
            return Err(invalid("payoff refinement changes an exact value"));
        }
        self.refined_payoff = Some(refined);
        Ok(())
    }
}

fn exact_enclosure(value: ExactExpectation) -> Option<F64Interval> {
    let denominator = i128::try_from(value.denominator).ok()?;
    F64Interval::integer(value.numerator).divide(F64Interval::integer(denominator)).ok()
}

/// Consume only the complete, independently nominal joint law produced by the full native replay.
/// Checked arithmetic exhaustion leaves the existing order untouched. An actual contradiction between
/// two certificates is an error; neither clipping an exact value nor averaging partial mass is permitted.
pub(super) fn refine_order_with_exact_law(
    order: &mut OrderScoreInterval,
    map: &PayoffMap,
    law: &ournotes_sim::live::full::LuckExactLaw,
) -> Result<bool, Error> {
    let prior_payoff = order_payoff(order, map)?;
    let (mut score, mut payoff) = (fraction(0), fraction(0));
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
        let value = match map {
            PayoffMap::Score => atom.score as i128,
            PayoffMap::ScoreAtLeast { threshold } => i128::from(atom.score >= *threshold),
            PayoffMap::CappedScore { threshold } => atom.score.min(*threshold) as i128,
            PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life } => {
                i128::from(atom.score >= *threshold && atom.final_life >= *min_final_life)
            }
            PayoffMap::NativeSteps(steps) => {
                steps
                    .iter()
                    .find(|step| step.lower <= atom.score && atom.score <= step.upper)
                    .ok_or_else(|| invalid("native payoff map does not cover an exact score"))?
                    .value
            }
        };
        let Some(next_score) = scale_exact(mass, atom.score as i128).and_then(|v| add_exact(score, v)) else {
            return Ok(false);
        };
        let Some(next_payoff) = scale_exact(mass, value).and_then(|v| add_exact(payoff, v)) else {
            return Ok(false);
        };
        (score, payoff) = (next_score, next_payoff);
        support = (support.0.min(atom.score), support.1.max(atom.score));
        life = (life.0.min(atom.final_life), life.1.max(atom.final_life));
    }
    if support.0 > support.1 {
        return Err(invalid("empty exact nominal law"));
    }
    let (Some(score_bounds), Some(payoff_bounds)) = (exact_enclosure(score), exact_enclosure(payoff)) else {
        return Ok(false);
    };
    if !exact_in_interval(score, order.mean)? || !exact_in_interval(payoff, prior_payoff.bounds)? {
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
    // The tighter exact support and mean may also tighten the old payoff enclosure. Intersect every
    // independently certified restriction before installation, retaining the exact rational itself.
    let tightened_prior = order_payoff(&next, map)?;
    let bounds = prior_payoff
        .bounds
        .intersect(tightened_prior.bounds)
        .and_then(|b| b.intersect(payoff_bounds))
        .ok_or_else(|| invalid("inconsistent exact payoff enclosure"))?;
    next.refine_payoff(PayoffRefinement { map: map.clone(), bounds, exact: Some(payoff) })?;
    next.validate()?;
    *order = next;
    Ok(true)
}

struct OrderPayoff {
    bounds: F64Interval,
    exact: Option<ExactExpectation>,
    thresholds: Vec<i32>,
    joint_life: bool,
    truncated_at: Option<i32>,
}

fn order_payoff(order: &OrderScoreInterval, map: &PayoffMap) -> Result<OrderPayoff, Error> {
    let plain =
        |bounds, exact| OrderPayoff { bounds, exact, thresholds: Vec::new(), joint_life: false, truncated_at: None };
    if let Some(refined) = &order.refined_payoff
        && &refined.map == map
    {
        return Ok(plain(refined.bounds, refined.exact));
    }
    match map {
        PayoffMap::Score => Ok(plain(order.mean, order.exact_mean)),
        PayoffMap::ScoreAtLeast { threshold } | PayoffMap::ScoreAndLifeAtLeast { threshold, .. } => {
            let tail = order.tail(*threshold)?;
            let mut out = plain(tail.bounds, tail.exact);
            if tail.exact.is_none() {
                out.thresholds.push(*threshold);
            }
            if let PayoffMap::ScoreAndLifeAtLeast { min_final_life, .. } = map {
                match order.final_life {
                    Some((_, hi)) if hi < *min_final_life => return Ok(plain(F64Interval::ZERO, Some(fraction(0)))),
                    Some((lo, _)) if lo >= *min_final_life => {}
                    _ if tail.bounds.upper() == 0.0 => return Ok(plain(F64Interval::ZERO, Some(fraction(0)))),
                    _ => {
                        out.bounds = F64Interval::new(0.0, tail.bounds.upper())?;
                        out.exact = None;
                        out.joint_life = true;
                    }
                }
            }
            Ok(out)
        }
        PayoffMap::CappedScore { threshold } => {
            let (lo, hi) = order.support;
            if hi <= *threshold {
                return Ok(plain(order.mean, order.exact_mean));
            }
            if lo >= *threshold {
                return Ok(plain(F64Interval::integer(*threshold as i128), Some(fraction(*threshold as i128))));
            }
            // A concave min(S,T) lies above its support chord and below min(E[S],T).
            let chord = order
                .mean
                .subtract(F64Interval::integer(lo as i128))
                .scale_integer(*threshold as i128 - lo as i128)
                .divide(F64Interval::integer(hi as i128 - lo as i128))?
                .add(F64Interval::integer(lo as i128));
            Ok(OrderPayoff {
                bounds: F64Interval::new(chord.lower().max(lo as f64), order.mean.upper().min(*threshold as f64))?,
                exact: None,
                thresholds: Vec::new(),
                joint_life: false,
                truncated_at: Some(*threshold),
            })
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
            let reachable: Vec<_> =
                steps.iter().filter(|s| s.upper >= order.support.0 && s.lower <= order.support.1).collect();
            let minimum = reachable.iter().map(|s| s.value).min().unwrap();
            let maximum = reachable.iter().map(|s| s.value).max().unwrap();
            if minimum == maximum {
                return Ok(plain(F64Interval::integer(minimum), Some(fraction(minimum))));
            }
            let mut out = plain(F64Interval::integer(reachable[0].value), Some(fraction(reachable[0].value)));
            for pair in reachable.windows(2) {
                let delta = pair[1]
                    .value
                    .checked_sub(pair[0].value)
                    .ok_or_else(|| invalid("native payoff difference overflow"))?;
                if delta == 0 {
                    continue;
                }
                let threshold = pair[1].lower;
                let tail = order.tail(threshold)?;
                out.bounds = out.bounds.add(tail.bounds.scale_integer(delta));
                out.exact = out
                    .exact
                    .and_then(|v| tail.exact.and_then(|p| scale_exact(p, delta)).and_then(|p| add_exact(v, p)));
                if tail.exact.is_none() {
                    out.thresholds.push(threshold);
                }
            }
            out.bounds = out
                .bounds
                .intersect(F64Interval::new(
                    F64Interval::integer(minimum).lower(),
                    F64Interval::integer(maximum).upper(),
                )?)
                .ok_or_else(|| invalid("payoff moment bounds contradict support"))?;
            Ok(out)
        }
    }
}

/// Work priority for one uniform-order enclosure; ranking is certified separately by the frontier.
pub(super) fn refinement_uncertainty(order: &OrderScoreInterval, map: &PayoffMap) -> Result<f64, Error> {
    let bounds = order_payoff(order, map)?.bounds;
    Ok(bounds.upper() - bounds.lower())
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

/// Aggregates the 120 order enclosures into one certificate under `reduction`. Its refinements are the boundary work of
/// the orders that can still decide the aggregate.
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
    let (score, mut payoff, exact_score, exact_payoff);
    match reduction {
        Reduction::Mean => {
            let (mut sum_score, mut sum_payoff) = (F64Interval::ZERO, F64Interval::ZERO);
            let (mut sum_exact_score, mut sum_exact_payoff) = (Some(fraction(0)), Some(fraction(0)));
            for (order, value) in orders.iter().zip(&values) {
                sum_score = sum_score.add(order.mean);
                sum_payoff = sum_payoff.add(value.bounds);
                sum_exact_score = sum_exact_score.and_then(|a| order.exact_mean.and_then(|b| add_exact(a, b)));
                sum_exact_payoff = sum_exact_payoff.and_then(|a| value.exact.and_then(|b| add_exact(a, b)));
            }
            let denominator = F64Interval::integer(uniform::ORDERS as i128);
            score = sum_score.divide(denominator)?;
            payoff = sum_payoff.divide(denominator)?;
            exact_score = sum_exact_score.and_then(average_exact);
            exact_payoff = sum_exact_payoff.and_then(average_exact);
        }
        Reduction::Maximum => {
            let payoffs: Vec<_> = values.iter().map(|v| (v.bounds, v.exact)).collect();
            let means: Vec<_> = orders.iter().map(|o| (o.mean, o.exact_mean)).collect();
            let (mut best_score, mut best_payoff) =
                (maximum_enclosure(means.iter().map(|m| m.0))?, maximum_enclosure(payoffs.iter().map(|p| p.0))?);
            exact_score = exact_maximum(&means)?;
            exact_payoff = exact_maximum(&payoffs)?;
            // The maximum is the exact value: it lies in every enclosure of it.
            if let Some(enclosure) = exact_score.and_then(exact_enclosure) {
                best_score =
                    best_score.intersect(enclosure).ok_or_else(|| invalid("exact maximum outside its score"))?;
            }
            if let Some(exact) = exact_payoff {
                if let Some(enclosure) = exact_enclosure(exact) {
                    best_payoff =
                        best_payoff.intersect(enclosure).ok_or_else(|| invalid("exact maximum outside its payoff"))?;
                }
                for (order, value) in orders.iter().zip(&values) {
                    if let Some(own) = value.exact
                        && compare_exact(own, exact)?.is_eq()
                    {
                        best_order = Some(order.order);
                        break;
                    }
                }
            }
            score = best_score;
            payoff = best_payoff;
            // The maximum is at least every lower bound and at least every exact value, so an order whose upper bound
            // lies strictly below either cannot be it.
            let mut floor = f64::NEG_INFINITY;
            let mut greatest_exact: Option<ExactExpectation> = None;
            for (bounds, exact) in &payoffs {
                floor = floor.max(bounds.lower());
                if let Some(exact) = *exact
                    && greatest_exact.map_or(Ok(true), |g| compare_exact(exact, g).map(|o| o.is_gt()))?
                {
                    greatest_exact = Some(exact);
                }
            }
            for (flag, (bounds, _)) in contending.iter_mut().zip(&payoffs) {
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
    let mut refinements = Vec::new();
    for (index, value) in values.into_iter().enumerate() {
        if contending[index] && (!value.thresholds.is_empty() || value.joint_life || value.truncated_at.is_some()) {
            refinements.push(BoundaryRefinement {
                order_index: index,
                thresholds: value.thresholds,
                joint_life: value.joint_life,
                truncated_at: value.truncated_at,
            });
        }
    }
    if matches!(map, PayoffMap::ScoreAtLeast { .. } | PayoffMap::ScoreAndLifeAtLeast { .. }) {
        // Outward summation/division can extend a certain event a few ulps above one. The objective's
        // semantic range is itself a proof and must remain available to ranking, including exact ties.
        payoff = payoff
            .intersect(F64Interval::new(0.0, 1.0)?)
            .ok_or_else(|| invalid("indicator expectation outside probability range"))?;
    }
    Ok(CertifiedEvaluation { score, payoff, exact_score, exact_payoff, orders, refinements, contending, best_order })
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
                tails: BTreeMap::new(),
                refined_payoff: None,
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
            tails: BTreeMap::new(),
            refined_payoff: None,
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
                tails: BTreeMap::new(),
                refined_payoff: None,
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
                assert!(evaluation.payoff.lower() >= 0.0 && evaluation.payoff.upper() <= 1.0);
                if threshold == 0 {
                    assert_eq!(evaluation.exact_payoff, Some(fraction(1)));
                    assert_eq!(evaluation.payoff.upper(), 1.0);
                }
            }
        }
    }
    #[test]
    fn random_native_payoff_uses_tail_mass_instead_of_payoff_at_mean() {
        // Equiprobable scores 0 and 100: grade-at-mean pays 100, while the actual mean reward is 50.
        let map = PayoffMap::NativeSteps(vec![
            PayoffStep { lower: 0, upper: 49, value: 0 },
            PayoffStep { lower: 50, upper: 100, value: 100 },
        ]);
        let mut orders = laws(50.0, (0, 100));
        let initial = aggregate_orders(orders.clone(), &map, Reduction::Mean).unwrap();
        assert!(initial.payoff.contains(50.0));
        assert!(initial.exact_payoff.is_none());
        assert_eq!(initial.refinements.len(), 120);
        for order in &mut orders {
            order
                .refine_tail(
                    50,
                    TailProbability {
                        bounds: F64Interval::point(0.5).unwrap(),
                        exact: Some(ExactExpectation { numerator: 1, denominator: 2 }),
                    },
                )
                .unwrap();
        }
        let refined = aggregate_orders(orders, &map, Reduction::Mean).unwrap();
        assert_eq!(refined.exact_payoff, Some(fraction(50)));
        assert!(refined.refinements.is_empty());
        assert!(refined.payoff.contains(50.0));
        assert!(refined.payoff.upper() < initial.payoff.upper());
    }
    #[test]
    fn one_grade_is_exact_even_when_score_expectation_is_not() {
        let mut orders = laws(17.0, (10, 20));
        for order in &mut orders {
            order.exact_mean = None;
        }
        let map = PayoffMap::NativeSteps(vec![PayoffStep { lower: 0, upper: 99, value: 73 }]);
        let v = aggregate_orders(orders, &map, Reduction::Mean).unwrap();
        assert!(v.exact_score.is_none());
        assert_eq!(v.exact_payoff, Some(fraction(73)));
    }
    #[test]
    fn nonmonotonic_native_steps_and_capped_score_keep_valid_moment_bounds() {
        let map = PayoffMap::NativeSteps(vec![
            PayoffStep { lower: -10, upper: -1, value: 70 },
            PayoffStep { lower: 0, upper: 4, value: 10 },
            PayoffStep { lower: 5, upper: 10, value: 30 },
        ]);
        for a in -10..=10 {
            for b in a..=10 {
                let mut orders = laws((a + b) as f64 / 2.0, (a, b));
                for order in &mut orders {
                    order.exact_mean = None;
                }
                let reward = |s| {
                    if s < 0 {
                        70.0
                    } else if s < 5 {
                        10.0
                    } else {
                        30.0
                    }
                };
                let actual = (reward(a) + reward(b)) / 2.0;
                assert!(aggregate_orders(orders.clone(), &map, Reduction::Mean).unwrap().payoff.contains(actual));
                let capped = (a.min(3) + b.min(3)) as f64 / 2.0;
                assert!(
                    aggregate_orders(orders, &PayoffMap::CappedScore { threshold: 3 }, Reduction::Mean)
                        .unwrap()
                        .payoff
                        .contains(capped)
                );
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
        assert!(value.exact_payoff.is_none());
        assert!(value.refinements.iter().all(|r| r.joint_life));
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
                    tails: BTreeMap::new(),
                    refined_payoff: None,
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
    fn orders_below_the_maximum_leave_the_refinements() {
        let map = PayoffMap::ScoreAtLeast { threshold: 50 };
        // Mean 90 gives a tail of at least 0.8 and mean 10 a tail of at most 0.2.
        let orders = || ranged(|i| if i == 3 { (90.0, 90.0, None) } else { (10.0, 10.0, None) });
        let value = maximum(orders(), &map);
        assert_eq!(value.contending.iter().filter(|&&c| c).count(), 1);
        assert!(value.contending[3]);
        // The only order that can attain the maximum is proved to attain it, its value still an interval.
        assert_eq!(value.best_order, Some(uniform::all_orders()[3]));
        assert!(value.exact_payoff.is_none());
        assert_eq!(value.refinements.iter().map(|r| r.order_index).collect::<Vec<_>>(), [3]);
        assert!(value.payoff.lower() >= 0.8 && value.payoff.upper() <= 1.0);
        let mean = aggregate_orders(orders(), &map, Reduction::Mean).unwrap();
        assert!(mean.contending.iter().all(|&c| c));
        assert_eq!(mean.refinements.len(), 120);
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
