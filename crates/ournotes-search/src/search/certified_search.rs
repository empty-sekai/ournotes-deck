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
    /// True after complete conditional score expectation evaluation. False marks a proved pending-order bound.
    pub evaluated: bool,
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
    /// Each conditional law retains its expectation; aggregation maximizes only over performance orders.
    BestOrderExpectedScore,
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

impl PayoffMap {
    pub(super) fn terminal_payoff(&self) -> Option<ournotes_sim::live::full::LuckTerminalPayoff> {
        use ournotes_sim::live::full::LuckTerminalPayoff;
        Some(match *self {
            Self::ScoreAtLeast { threshold } => LuckTerminalPayoff::ScoreAtLeast { threshold },
            Self::CappedScore { threshold } => LuckTerminalPayoff::CappedScore { threshold },
            Self::ScoreAndLifeAtLeast { threshold, min_final_life } => {
                LuckTerminalPayoff::ScoreAndLifeAtLeast { threshold, min_final_life }
            }
            _ => return None,
        })
    }
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
    pub score: F64Interval,
    pub payoff: F64Interval,
    pub exact_score: Option<ExactExpectation>,
    pub exact_payoff: Option<ExactExpectation>,
    pub orders: Vec<OrderScoreInterval>,
    pub refinements: Vec<BoundaryRefinement>,
    pub best_order: Option<Box<BestOrderWitness>>,
}

#[derive(Clone, Debug)]
pub struct BestOrderWitness {
    pub order: [usize; 5],
    pub mean: F64Interval,
    pub exact_mean: Option<ExactExpectation>,
    pub optimal: bool,
    pub evaluated_orders: usize,
}

/// All cached and refined order labels use this same complete-performer basis. Paired support remains
/// inside each performer; request-local context, power and payoff identity are separately fixed by Engine.
pub(super) fn canonicalize_performers(input: &mut super::expectation::FiniteSeedContext) -> Vec<u8> {
    canonicalize_performers_with_basis(input).0
}

/// The returned basis maps a canonical performer index to its original physical slot.
pub(super) fn canonicalize_performers_with_basis(
    input: &mut super::expectation::FiniteSeedContext,
) -> (Vec<u8>, [usize; 5]) {
    let mut performers: Vec<_> =
        input.performers.iter().enumerate().map(|(slot, p)| (format!("{p:?}"), p.clone(), slot)).collect();
    performers.sort_by(|a, b| a.0.cmp(&b.0));
    let keys: Vec<_> = performers.iter().map(|p| &p.0).collect();
    let program = format!("uniform120/full-performers/{keys:?}").into_bytes();
    let basis = std::array::from_fn(|i| performers[i].2);
    input.performers = performers.into_iter().map(|p| p.1).collect::<Vec<_>>().try_into().expect("five performers");
    (program, basis)
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
    /// Intersect an independently completed factor-history enclosure of this same physical order.
    /// Existing exact values and payoff evidence remain attached to the original nominal law.
    pub(super) fn refine_summary(&mut self, summary: ournotes_sim::live::full::LuckScoreSummary) -> Result<(), Error> {
        let refined = summary_order(self.order, summary)?;
        let mut next = self.clone();
        next.support = (self.support.0.max(refined.support.0), self.support.1.min(refined.support.1));
        if next.support.0 > next.support.1 {
            return Err(invalid("factor-history refinement contradicts score support"));
        }
        next.mean = self
            .mean
            .intersect(refined.mean)
            .and_then(|mean| {
                mean.intersect(F64Interval::new(f64::from(next.support.0), f64::from(next.support.1)).ok()?)
            })
            .ok_or_else(|| invalid("factor-history refinement contradicts score mean"))?;
        if let (Some(previous), Some(refined)) = (self.exact_mean, refined.exact_mean)
            && compare_exact(previous, refined)? != std::cmp::Ordering::Equal
        {
            return Err(invalid("factor-history refinement changes an exact mean"));
        }
        next.exact_mean = self.exact_mean.or(refined.exact_mean);
        next.final_life = match (self.final_life, refined.final_life) {
            (Some(a), Some(b)) => Some((a.0.max(b.0), a.1.min(b.1))),
            (a, b) => a.or(b),
        };
        next.evaluated = true;
        next.validate()?;
        *self = next;
        Ok(())
    }

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
        if !self.evaluated && self.exact_mean.is_some() {
            return Err(invalid("a pending order cannot declare an exact evaluated mean"));
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

    pub fn refine_payoff(&mut self, mut refined: PayoffRefinement) -> Result<(), Error> {
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
        refined.exact = prior.exact.or(refined.exact);
        self.refined_payoff = Some(refined);
        Ok(())
    }
}

/// A completed controller-output law supplies only a mapped expectation enclosure. Keep the raw score
/// evidence and exact metadata independent, and retain the original full mapping before installation.
pub(super) fn refine_order_with_terminal_payoff(
    order: &mut OrderScoreInterval,
    map: &PayoffMap,
    result: &ournotes_sim::live::full::LuckTerminalPayoffBounds,
) -> Result<(), Error> {
    if map.terminal_payoff() != Some(result.map()) {
        return Err(invalid("terminal payoff refinement changes the complete mapping"));
    }
    let bounds = order_payoff(order, map)?
        .bounds
        .intersect(result.bounds())
        .ok_or_else(|| invalid("complete terminal payoff contradicts its prior enclosure"))?;
    order.refine_payoff(PayoffRefinement {
        map: map.clone(),
        bounds,
        exact: result.exact_constant().map(|value| fraction(i128::from(value))),
    })
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
            PayoffMap::Score | PayoffMap::BestOrderExpectedScore => atom.score as i128,
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
    next.evaluated = true;
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
    let mut value = unrefined_order_payoff(order, map)?;
    if let Some(refined) = &order.refined_payoff
        && &refined.map == map
    {
        value.bounds = value
            .bounds
            .intersect(refined.bounds)
            .ok_or_else(|| invalid("mapped payoff refinement contradicts current score evidence"))?;
        if let (Some(prior), Some(next)) = (value.exact, refined.exact)
            && compare_exact(prior, next)? != std::cmp::Ordering::Equal
        {
            return Err(invalid("mapped payoff refinement changes an exact value"));
        }
        value.exact = value.exact.or(refined.exact);
    }
    if value.exact.is_some() {
        value.thresholds.clear();
        value.joint_life = false;
        value.truncated_at = None;
    } else {
        // An outward mapped enclosure is still eligible for stronger numerical/exact work. In
        // particular it must not disappear from the scheduler merely because the optional fold ran.
        match *map {
            PayoffMap::ScoreAtLeast { threshold } | PayoffMap::ScoreAndLifeAtLeast { threshold, .. } => {
                if value.thresholds.is_empty() {
                    value.thresholds.push(threshold);
                }
            }
            PayoffMap::CappedScore { threshold } => value.truncated_at = Some(threshold),
            _ => {}
        }
    }
    Ok(value)
}

fn unrefined_order_payoff(order: &OrderScoreInterval, map: &PayoffMap) -> Result<OrderPayoff, Error> {
    let plain =
        |bounds, exact| OrderPayoff { bounds, exact, thresholds: Vec::new(), joint_life: false, truncated_at: None };
    match map {
        PayoffMap::Score | PayoffMap::BestOrderExpectedScore => Ok(plain(order.mean, order.exact_mean)),
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

pub fn aggregate_orders(mut orders: Vec<OrderScoreInterval>, map: &PayoffMap) -> Result<CertifiedEvaluation, Error> {
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
    if matches!(map, PayoffMap::BestOrderExpectedScore) {
        return aggregate_best_order(orders);
    }
    if orders.iter().any(|order| !order.evaluated) {
        return Err(invalid("uniform aggregation requires a complete evaluation of every order"));
    }
    let (mut score, mut payoff) = (F64Interval::ZERO, F64Interval::ZERO);
    let (mut exact_score, mut exact_payoff) = (Some(fraction(0)), Some(fraction(0)));
    let mut refinements = Vec::new();
    for (index, order) in orders.iter().enumerate() {
        let value = order_payoff(order, map)?;
        score = score.add(order.mean);
        payoff = payoff.add(value.bounds);
        exact_score = exact_score.and_then(|a| order.exact_mean.and_then(|b| add_exact(a, b)));
        exact_payoff = exact_payoff.and_then(|a| value.exact.and_then(|b| add_exact(a, b)));
        if !value.thresholds.is_empty() || value.joint_life || value.truncated_at.is_some() {
            refinements.push(BoundaryRefinement {
                order_index: index,
                thresholds: value.thresholds,
                joint_life: value.joint_life,
                truncated_at: value.truncated_at,
            });
        }
    }
    let denominator = F64Interval::integer(uniform::ORDERS as i128);
    let mut payoff = payoff.divide(denominator)?;
    if matches!(map, PayoffMap::ScoreAtLeast { .. } | PayoffMap::ScoreAndLifeAtLeast { .. }) {
        // Outward summation/division can extend a certain event a few ulps above one. The objective's
        // semantic range is itself a proof and must remain available to ranking, including exact ties.
        payoff = payoff
            .intersect(F64Interval::new(0.0, 1.0)?)
            .ok_or_else(|| invalid("indicator expectation outside probability range"))?;
    }
    Ok(CertifiedEvaluation {
        score: score.divide(denominator)?,
        payoff,
        exact_score: exact_score.and_then(average_exact),
        exact_payoff: exact_payoff.and_then(average_exact),
        orders,
        refinements,
        best_order: None,
    })
}

fn expected_bound_cmp(
    a: Option<ExactExpectation>,
    a_endpoint: f64,
    b: Option<ExactExpectation>,
    b_endpoint: f64,
) -> Result<std::cmp::Ordering, Error> {
    match (a, b) {
        (Some(a), Some(b)) => compare_exact(a, b),
        (Some(a), None) => compare_exact_real(a, b_endpoint),
        (None, Some(b)) => compare_exact_real(b, a_endpoint).map(std::cmp::Ordering::reverse),
        (None, None) => a_endpoint.partial_cmp(&b_endpoint).ok_or_else(|| invalid("nonfinite expectation bound")),
    }
}

/// Every order has a complete-domain enclosure, including pending orders. The witness must have been
/// evaluated; midpoint estimates and upper-only preparations cannot supply it. A canonical tie is established
/// with exact fractions or certified endpoint separation, independently of the number of evaluated orders.
fn aggregate_best_order(orders: Vec<OrderScoreInterval>) -> Result<CertifiedEvaluation, Error> {
    let score = F64Interval::new(
        orders.iter().map(|order| order.mean.lower()).fold(f64::NEG_INFINITY, f64::max),
        orders.iter().map(|order| order.mean.upper()).fold(f64::NEG_INFINITY, f64::max),
    )?;
    let mut best: Option<&OrderScoreInterval> = None;
    for order in orders.iter().filter(|order| order.evaluated) {
        let replace = match best {
            None => true,
            Some(current) => {
                expected_bound_cmp(order.exact_mean, order.mean.lower(), current.exact_mean, current.mean.lower())?
                    .is_gt()
            }
        };
        if replace {
            best = Some(order);
        }
    }
    let mut exact_score = None;
    let best_order = best
        .map(|best| {
            let mut value_proven = true;
            let mut optimal = true;
            for other in orders.iter().filter(|order| order.order != best.order) {
                let ordering =
                    expected_bound_cmp(best.exact_mean, best.mean.lower(), other.exact_mean, other.mean.upper())?;
                value_proven &= !ordering.is_lt();
                optimal &= ordering.is_gt() || (ordering.is_eq() && best.order < other.order);
            }
            if value_proven {
                exact_score = best.exact_mean;
            }
            Ok::<_, Error>(Box::new(BestOrderWitness {
                order: best.order,
                mean: best.mean,
                exact_mean: best.exact_mean,
                optimal,
                evaluated_orders: orders.iter().filter(|order| order.evaluated).count(),
            }))
        })
        .transpose()?;
    Ok(CertifiedEvaluation {
        score,
        payoff: score,
        exact_score,
        exact_payoff: exact_score,
        orders,
        refinements: Vec::new(),
        best_order,
    })
}

/// A best-order candidate retains an evaluated witness even if its remaining order work stops. Every pending
/// row remains a proved whole-order enclosure, so neither the maximum nor its proof omits an unfinished order.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_best_order_context(
    master: &ournotes_sim::master::Master,
    skills: Option<&ournotes_sim::live::full::LuckSkills>,
    input: &super::expectation::FiniteSeedContext,
    basis: [usize; 5],
    caps: &[i128],
    cutoff: Option<(i128, i32, i32)>,
    mut curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<CertifiedEvaluation>, Error> {
    if caps.len() != uniform::ORDERS {
        return Err(invalid("best-order bounds require all 120 orders"));
    }
    let labels = uniform::all_orders();
    let mut orders = labels
        .iter()
        .zip(caps)
        .map(|(order, &cap)| {
            let upper = cap.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32;
            Ok(OrderScoreInterval {
                order: order.map(|slot| basis[slot]),
                evaluated: false,
                mean: F64Interval::new(f64::from(i32::MIN), f64::from(upper))?,
                support: (i32::MIN, i32::MAX),
                exact_mean: None,
                final_life: None,
                tails: BTreeMap::new(),
                refined_payoff: None,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let mut schedule: Vec<_> = (0..uniform::ORDERS).collect();
    schedule.sort_by(|&a, &b| caps[b].cmp(&caps[a]).then(orders[a].order.cmp(&orders[b].order)));
    let mut session = skills
        .map(|skills| {
            let setup = input.gekisou.as_ref().ok_or_else(|| invalid("LUCK requires Gekisou context"))?;
            Ok::<_, Error>(ournotes_sim::live::full::LuckScoreSession::new(
                master,
                skills,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
            ))
        })
        .transpose()?;
    let mut evaluated = false;
    let mut incumbent: Option<Box<BestOrderWitness>> = None;
    for index in schedule {
        if cancelled() {
            break;
        }
        if let Some(best) = &incumbent {
            let relation = expected_bound_cmp(best.exact_mean, best.mean.lower(), None, orders[index].mean.upper())?;
            if relation.is_gt() || (relation.is_eq() && best.order < orders[index].order) {
                continue;
            }
        }
        let canonical_order = labels[index];
        let value = if let Some(session) = &mut session {
            let performers = canonical_order.map(|slot| input.performers[slot].clone());
            let Some(summary) = session.summary_or_terminal(&performers, curves.as_deref_mut(), &mut cancelled)? else {
                break;
            };
            summary_order(orders[index].order, summary)?
        } else {
            let outcome = input.simulate_performance_order(master, canonical_order)?;
            if outcome.model.draws() != 0 {
                return Err(invalid("deterministic best-order evaluation consumed lottery draws"));
            }
            OrderScoreInterval {
                order: orders[index].order,
                evaluated: true,
                mean: F64Interval::integer(i128::from(outcome.final_score)),
                support: (outcome.final_score, outcome.final_score),
                exact_mean: Some(fraction(i128::from(outcome.final_score))),
                final_life: Some((outcome.model.current_life(), outcome.model.current_life())),
                tails: BTreeMap::new(),
                refined_payoff: None,
            }
        };
        if value.mean.upper() > orders[index].mean.upper() && value.mean.lower() > orders[index].mean.upper() {
            return Err(invalid("evaluated order contradicts its complete-domain upper bound"));
        }
        let mut value = value;
        value.mean = value.mean.intersect(orders[index].mean).ok_or_else(|| invalid("conflicting order bounds"))?;
        orders[index] = value;
        evaluated = true;
        let complete = aggregate_orders(orders.clone(), &PayoffMap::BestOrderExpectedScore)?;
        let cap = (complete.score.upper().ceil() as i128).saturating_mul(uniform::ORDERS as i128);
        if cutoff
            .is_some_and(|(threshold, power, kth_power)| cap < threshold || (cap == threshold && power < kth_power))
        {
            return Ok(Some(complete));
        }
        if complete.best_order.as_ref().is_some_and(|best| best.optimal) {
            return Ok(Some(complete));
        }
        incumbent = complete.best_order;
        if cancelled() {
            break;
        }
    }
    if evaluated { aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).map(Some) } else { Ok(None) }
}

fn summary_order(
    order: [usize; 5],
    summary: ournotes_sim::live::full::LuckScoreSummary,
) -> Result<OrderScoreInterval, Error> {
    let support = (summary.final_support.lower, summary.final_support.upper);
    let mean = F64Interval::new(summary.final_mean.lower, summary.final_mean.upper)?
        .intersect(F64Interval::new(support.0 as f64, support.1 as f64)?)
        .ok_or_else(|| invalid("LUCK mean and support disagree"))?;
    Ok(OrderScoreInterval {
        order,
        evaluated: true,
        mean,
        support,
        exact_mean: summary.exact_constant_score.map(|s| fraction(s as i128)),
        final_life: summary.exact_final_life.map(|life| (life, life)),
        tails: BTreeMap::new(),
        refined_payoff: None,
    })
}

/// The production scorer supplies one all-path law enclosure per order. Cancellation retains no partial value
/// masquerading as a 120-order mean; callers may retain the completed orders separately for the next refinement.
pub fn evaluate_orders(
    map: &PayoffMap,
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
    aggregate_orders(orders, map).map(Some)
}

/// The actual all-path LUCK scorer in a normal native or WASM build. Performers are permuted before constructing
/// the model; external rank arrivals, clock data and every other live parameter remain the declared context.
/// `curves` shares lottery curves across the orders and with other calls.
pub fn evaluate_luck_context(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    map: &PayoffMap,
    curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    cancelled: impl FnMut() -> bool,
) -> Result<Option<CertifiedEvaluation>, Error> {
    evaluate_luck_context_until(
        master,
        skills,
        input,
        map,
        curves,
        (&(0..uniform::ORDERS).collect::<Vec<_>>(), |_, _| true),
        cancelled,
    )
}

/// A caller may stop after a complete order enclosure when its remaining-order caps prove exclusion.
/// A stopped evaluation supplies no aggregate or cache entry.
pub(super) fn evaluate_luck_context_until(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    map: &PayoffMap,
    curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    control: (&[usize], impl FnMut(usize, &OrderScoreInterval) -> bool),
    cancelled: impl FnMut() -> bool,
) -> Result<Option<CertifiedEvaluation>, Error> {
    let (schedule, mut keep_going) = control;
    match evaluate_luck_context_bounded(
        master,
        skills,
        input,
        map,
        curves,
        (schedule, false, |event| match event {
            LuckContextEvent::Scored { index, order } => keep_going(index, order),
            _ => true,
        }),
        cancelled,
    )? {
        LuckContextOutcome::Full(score) => Ok(Some(score)),
        LuckContextOutcome::UpperOnly | LuckContextOutcome::Stopped => Ok(None),
    }
}

/// Notifications from a candidate's all-order bound preparation and complete order scoring.
/// Returning false may exclude the candidate only for a Ready preparation or a complete Scored order.
/// The caller must independently prove that its new cap and every unfinished order's retained cap exclude it.
pub(super) enum LuckContextEvent<'a> {
    UpperAttempt,
    UpperPrepared {
        index: usize,
        preparation: &'a ournotes_sim::live::full::LuckRushPreparation,
    },
    /// Includes preparation and the caller's bound work; a subset of the surrounding simulation activity.
    UpperFinished {
        elapsed_ms: f64,
    },
    Scored {
        index: usize,
        order: &'a OrderScoreInterval,
    },
}

/// UpperOnly supplies no candidate value, score law or cacheable complete evaluation.
/// Stopped preserves cancellation and deadline exhaustion independently of cap exclusion.
pub(super) enum LuckContextOutcome {
    Full(CertifiedEvaluation),
    UpperOnly,
    Stopped,
}

/// A Score caller with valid caps can prepare terminal Rush laws before factor-history replay. One session is
/// retained through both stages, so completed lottery recordings remain reusable by the subsequent full scorer.
/// Only a complete set of all 120 scored orders can reach Full, even when every prepass order was prepared.
pub(super) fn evaluate_luck_context_bounded(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    map: &PayoffMap,
    curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    control: (&[usize], bool, impl FnMut(LuckContextEvent<'_>) -> bool),
    cancelled: impl FnMut() -> bool,
) -> Result<LuckContextOutcome, Error> {
    evaluate_luck_context_bounded_policy(
        master,
        skills,
        input,
        map,
        curves,
        matches!(map, PayoffMap::Score | PayoffMap::BestOrderExpectedScore),
        control,
        cancelled,
    )
}

/// Select only the complete raw-score summary provider. Nonlinear maps still consume their original
/// support/first-moment bounds and subsequent mapped-payoff or complete-law refinements.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_luck_context_bounded_policy(
    master: &ournotes_sim::master::Master,
    skills: &ournotes_sim::live::full::LuckSkills,
    input: &super::expectation::FiniteSeedContext,
    map: &PayoffMap,
    mut curves: Option<&mut ournotes_sim::live::full::LuckDpCache>,
    complete_terminal: bool,
    control: (&[usize], bool, impl FnMut(LuckContextEvent<'_>) -> bool),
    mut cancelled: impl FnMut() -> bool,
) -> Result<LuckContextOutcome, Error> {
    use ournotes_sim::live::full::LuckRushPreparation;
    let (schedule, prepare_upper, mut observe) = control;
    let mut seen = [false; uniform::ORDERS];
    if schedule.len() != uniform::ORDERS
        || schedule.iter().any(|&index| index >= uniform::ORDERS || std::mem::replace(&mut seen[index], true))
    {
        return Err(invalid("LUCK order schedule must contain all 120 distinct order indices"));
    }
    if prepare_upper && !matches!(map, PayoffMap::Score) {
        return Err(invalid("terminal Rush upper bounds require the Score objective"));
    }
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
    let labels = uniform::all_orders();
    let mut prepared_scores = vec![None; uniform::ORDERS];
    if prepare_upper {
        let started = crate::clock::Instant::now();
        let finish = |observe: &mut dyn FnMut(LuckContextEvent<'_>) -> bool| {
            observe(LuckContextEvent::UpperFinished { elapsed_ms: started.elapsed().as_secs_f64() * 1e3 });
        };
        for &index in schedule {
            if cancelled() {
                finish(&mut observe);
                return Ok(LuckContextOutcome::Stopped);
            }
            observe(LuckContextEvent::UpperAttempt);
            let performers = labels[index].map(|slot| input.performers[slot].clone());
            let preparation = session.rush_cap_preparation(&performers, curves.as_deref_mut(), &mut cancelled);
            let keep_going = observe(LuckContextEvent::UpperPrepared { index, preparation: &preparation });
            match preparation {
                LuckRushPreparation::Stopped => {
                    finish(&mut observe);
                    return Ok(LuckContextOutcome::Stopped);
                }
                LuckRushPreparation::Ready(_) if !keep_going => {
                    finish(&mut observe);
                    return Ok(LuckContextOutcome::UpperOnly);
                }
                LuckRushPreparation::Ready(terminal) => {
                    if complete_terminal {
                        prepared_scores[index] = terminal.terminal_summary(i64::from(input.params.total_power));
                    }
                }
                LuckRushPreparation::Unavailable { .. } => {}
            }
        }
        finish(&mut observe);
    }
    let mut orders = Vec::with_capacity(uniform::ORDERS);
    for &index in schedule {
        let order = labels[index];
        if cancelled() {
            return Ok(LuckContextOutcome::Stopped);
        }
        let performers = order.map(|slot| input.performers[slot].clone());
        let summary = match prepared_scores[index].take() {
            Some(summary) => Some(summary),
            None if prepare_upper || !complete_terminal => {
                session.summary(&performers, curves.as_deref_mut(), &mut cancelled)?
            }
            None => session.summary_or_terminal(&performers, curves.as_deref_mut(), &mut cancelled)?,
        };
        let Some(summary) = summary else {
            return Ok(LuckContextOutcome::Stopped);
        };
        let value = summary_order(order, summary)?;
        if !observe(LuckContextEvent::Scored { index, order: &value }) {
            return Ok(LuckContextOutcome::UpperOnly);
        }
        orders.push(value);
        if cancelled() {
            return Ok(LuckContextOutcome::Stopped);
        }
    }
    aggregate_orders(orders, map).map(LuckContextOutcome::Full)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn laws(mean: f64, support: (i32, i32)) -> Vec<OrderScoreInterval> {
        uniform::all_orders()
            .into_iter()
            .map(|order| OrderScoreInterval {
                order,
                evaluated: true,
                mean: F64Interval::point(mean).unwrap(),
                support,
                exact_mean: Some(fraction(mean as i128)),
                final_life: None,
                tails: BTreeMap::new(),
                refined_payoff: None,
            })
            .collect()
    }

    fn summary(lower: f64, upper: f64, support: (i32, i32), life: i32) -> ournotes_sim::live::full::LuckScoreSummary {
        ournotes_sim::live::full::LuckScoreSummary {
            final_mean: F64Interval::new(lower, upper).unwrap().into(),
            final_support: ournotes_sim::live::certified::I32Interval::new(support.0, support.1).unwrap().into(),
            exact_constant_score: (support.0 == support.1).then_some(support.0),
            exact_final_life: Some(life),
            probability_peak_states: 1,
            probability_transitions: 0,
        }
    }

    #[test]
    fn summary_refinement_intersects_score_and_preserves_existing_law_evidence() {
        let mut order = laws(50.0, (0, 100)).remove(0);
        order.mean = F64Interval::new(45.0, 55.0).unwrap();
        order.final_life = Some((900, 1100));
        order.refine_tail(40, TailProbability { bounds: F64Interval::ONE, exact: Some(fraction(1)) }).unwrap();
        order
            .refine_payoff(PayoffRefinement { map: PayoffMap::Score, bounds: order.mean, exact: Some(fraction(50)) })
            .unwrap();
        order.refine_summary(summary(49.5, 50.5, (48, 52), 1000)).unwrap();
        assert_eq!(order.mean, F64Interval::new(49.5, 50.5).unwrap());
        assert_eq!(order.support, (48, 52));
        assert_eq!(order.final_life, Some((1000, 1000)));
        assert_eq!(order.exact_mean, Some(fraction(50)));
        assert_eq!(order.tails[&40].exact, Some(fraction(1)));
        assert_eq!(order.refined_payoff.unwrap().exact, Some(fraction(50)));
    }

    #[test]
    fn summary_refinement_rejects_contradictions_without_losing_previous_evidence() {
        let mut order = laws(50.0, (0, 100)).remove(0);
        order.mean = F64Interval::new(40.0, 60.0).unwrap();
        order.final_life = Some((900, 1100));
        let previous = format!("{order:?}");
        for fresh in [
            summary(30.0, 39.0, (0, 100), 1000),
            summary(101.0, 110.0, (101, 110), 1000),
            summary(51.0, 51.0, (51, 51), 1000),
            summary(49.0, 51.0, (0, 100), 1200),
        ] {
            assert!(order.refine_summary(fresh).is_err());
            assert_eq!(format!("{order:?}"), previous);
        }
    }

    #[test]
    fn summary_refinement_preserves_best_order_labels_after_a_nontrivial_basis_mapping() {
        let basis = [4, 2, 0, 3, 1];
        let mut orders = laws(0.0, (0, 100));
        for order in &mut orders {
            order.order = order.order.map(|slot| basis[slot]);
            order.evaluated = false;
            order.mean = F64Interval::new(0.0, 60.0).unwrap();
            order.exact_mean = None;
        }
        let expected = orders[7].order;
        orders[7].mean = F64Interval::new(0.0, 100.0).unwrap();
        orders[7].refine_summary(summary(75.0, 76.0, (70, 80), 1000)).unwrap();
        let result = aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).unwrap();
        let witness = result.best_order.unwrap();
        assert_eq!(witness.order, expected);
        assert!(witness.optimal);
        assert_eq!(witness.evaluated_orders, 1);
        assert!(result.exact_score.is_none());
    }

    #[test]
    fn best_order_maximizes_conditional_means_and_proves_exact_fraction_ties() {
        let mut orders = laws(0.0, (0, 1_000_000));
        for order in &mut orders {
            order.mean = F64Interval::new(0.0, 1.0).unwrap();
            order.exact_mean = Some(ExactExpectation { numerator: 1, denominator: 3 });
        }
        for index in [7, 40] {
            orders[index].exact_mean = Some(ExactExpectation { numerator: 2, denominator: 3 });
        }
        let expected = orders[7].order;
        let uniform = aggregate_orders(orders.clone(), &PayoffMap::Score).unwrap();
        let best = aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).unwrap();
        assert_eq!(best.exact_score, Some(ExactExpectation { numerator: 2, denominator: 3 }));
        assert!(compare_exact(best.exact_score.unwrap(), uniform.exact_score.unwrap()).unwrap().is_gt());
        let witness = best.best_order.unwrap();
        assert_eq!(witness.order, expected);
        assert!(witness.optimal);
        assert_eq!(witness.evaluated_orders, 120);
        assert!(best.score.contains(2.0 / 3.0));
        assert!(best.score.upper() < 1_000_000.0);
    }

    #[test]
    fn best_order_pending_bounds_preserve_the_maximum_and_canonical_tie() {
        let mut orders = laws(0.0, (0, 100));
        for order in &mut orders {
            order.evaluated = false;
            order.mean = F64Interval::new(0.0, 50.0).unwrap();
            order.exact_mean = None;
        }
        orders[7].evaluated = true;
        orders[7].mean = F64Interval::point(50.0).unwrap();
        orders[7].exact_mean = Some(fraction(50));
        let partial = aggregate_orders(orders.clone(), &PayoffMap::BestOrderExpectedScore).unwrap();
        assert_eq!(partial.exact_score, Some(fraction(50)));
        let witness = partial.best_order.unwrap();
        assert_eq!(witness.order, orders[7].order);
        assert!(!witness.optimal, "an earlier unevaluated order can tie");
        assert_eq!(witness.evaluated_orders, 1);
        assert!(aggregate_orders(orders.clone(), &PayoffMap::Score).is_err());
        orders[0].evaluated = true;
        orders[0].mean = F64Interval::point(50.0).unwrap();
        orders[0].exact_mean = Some(fraction(50));
        let tied = aggregate_orders(orders.clone(), &PayoffMap::BestOrderExpectedScore).unwrap();
        assert!(tied.best_order.as_ref().unwrap().optimal);
        assert_eq!(tied.best_order.as_ref().unwrap().evaluated_orders, 2);
        orders[80].mean = F64Interval::new(0.0, 80.0).unwrap();
        let open = aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).unwrap();
        assert_eq!(open.exact_score, None);
        assert_eq!((open.score.lower(), open.score.upper()), (50.0, 80.0));
        assert!(!open.best_order.unwrap().optimal);
    }

    #[test]
    fn best_order_completion_count_does_not_prove_overlapping_expectations() {
        let mut orders = laws(10.0, (0, 100));
        for order in &mut orders {
            order.exact_mean = None;
            order.mean = F64Interval::new(10.0, 11.0).unwrap();
        }
        orders[90].mean = F64Interval::new(10.5, 12.0).unwrap();
        let best = aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).unwrap();
        assert_eq!(best.exact_score, None);
        let witness = best.best_order.unwrap();
        assert_eq!(witness.evaluated_orders, 120);
        assert!(!witness.optimal);
        assert_eq!(uniform::order_index(&witness.order), 90);
    }

    #[test]
    fn pending_orders_cannot_supply_exact_evaluated_metadata() {
        let mut orders = laws(10.0, (0, 100));
        orders[0].evaluated = false;
        assert!(aggregate_orders(orders.clone(), &PayoffMap::BestOrderExpectedScore).is_err());
        orders[0].evaluated = true;
        orders[0].exact_mean = Some(ExactExpectation { numerator: 10, denominator: 0 });
        assert!(aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).is_err());
    }

    #[test]
    fn best_order_signed_zero_bounds_keep_the_mathematical_tie() {
        let mut orders = laws(0.0, (0, 0));
        for order in &mut orders {
            order.exact_mean = None;
        }
        orders[0].mean = F64Interval::point(-0.0).unwrap();
        let result = aggregate_orders(orders, &PayoffMap::BestOrderExpectedScore).unwrap();
        let witness = result.best_order.unwrap();
        assert_eq!(witness.order, [0, 1, 2, 3, 4]);
        assert!(witness.optimal);
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
                let evaluation = aggregate_orders(orders, &map).unwrap();
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
        let initial = aggregate_orders(orders.clone(), &map).unwrap();
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
        let refined = aggregate_orders(orders, &map).unwrap();
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
        let v = aggregate_orders(orders, &PayoffMap::NativeSteps(vec![PayoffStep { lower: 0, upper: 99, value: 73 }]))
            .unwrap();
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
                assert!(aggregate_orders(orders.clone(), &map).unwrap().payoff.contains(actual));
                let capped = (a.min(3) + b.min(3)) as f64 / 2.0;
                assert!(
                    aggregate_orders(orders, &PayoffMap::CappedScore { threshold: 3 }).unwrap().payoff.contains(capped)
                );
            }
        }
    }
    #[test]
    fn partial_duplicate_orders_and_unproved_life_never_become_exact() {
        let mut orders = laws(50.0, (0, 100));
        assert!(aggregate_orders(orders[..119].to_vec(), &PayoffMap::Score).is_err());
        orders[119].order = orders[0].order;
        assert!(aggregate_orders(orders, &PayoffMap::Score).is_err());
        let value =
            aggregate_orders(laws(50.0, (0, 100)), &PayoffMap::ScoreAndLifeAtLeast { threshold: 1, min_final_life: 1 })
                .unwrap();
        assert!(value.exact_payoff.is_none());
        assert!(value.refinements.iter().all(|r| r.joint_life));
    }

    #[test]
    fn outward_mapped_refinements_keep_exact_fallback_and_independent_score_evidence() {
        for (map, lo, hi, exact) in [
            (PayoffMap::ScoreAtLeast { threshold: 50 }, 0.4, 0.6, ExactExpectation { numerator: 1, denominator: 2 }),
            (PayoffMap::CappedScore { threshold: 50 }, 25.0, 30.0, fraction(25)),
            (
                PayoffMap::ScoreAndLifeAtLeast { threshold: 50, min_final_life: 1 },
                0.4,
                0.6,
                ExactExpectation { numerator: 1, denominator: 2 },
            ),
        ] {
            let mut orders = laws(50.0, (0, 100));
            for order in &mut orders {
                order.exact_mean = None;
                order.final_life = Some((1, 1));
                order
                    .refine_payoff(PayoffRefinement {
                        map: map.clone(),
                        bounds: F64Interval::new(lo, hi).unwrap(),
                        exact: None,
                    })
                    .unwrap();
                assert_eq!(order.mean, F64Interval::point(50.0).unwrap());
                assert_eq!(order.support, (0, 100));
                assert!(order.exact_mean.is_none());
            }
            let partial = aggregate_orders(orders.clone(), &map).unwrap();
            assert_eq!(partial.refinements.len(), 120, "outward mapping evidence must not hide exact work");
            assert!(partial.exact_payoff.is_none() && partial.exact_score.is_none());
            let expected = exact.numerator as f64 / exact.denominator as f64;
            for order in &mut orders {
                order
                    .refine_payoff(PayoffRefinement {
                        map: map.clone(),
                        bounds: F64Interval::point(expected).unwrap(),
                        exact: Some(exact),
                    })
                    .unwrap();
            }
            let complete = aggregate_orders(orders, &map).unwrap();
            assert_eq!(complete.exact_payoff, Some(exact));
            assert!(complete.refinements.is_empty());
            assert!(complete.exact_score.is_none(), "a mapped proof cannot manufacture an exact raw score");
        }
    }
}
