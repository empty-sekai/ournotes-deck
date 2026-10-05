//! Ranking proofs from enclosures. An overlapping interval is a live candidate, never an estimated rank.
use super::expectation::ExactExpectation;
use ournotes_sim::{Error, live::certified::F64Interval};
use std::{cmp::Ordering, collections::BTreeMap, rc::Rc};

pub type CandidateId = u64;

/// The existing secondary ordering: larger power, then smaller complete canonical team key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalTie {
    pub power: i32,
    pub key: Vec<i64>,
}
impl CanonicalTie {
    fn better(&self, other: &Self) -> Ordering {
        self.power.cmp(&other.power).then_with(|| other.key.cmp(&self.key))
    }
}

/// Authority is request-local and uses the entire identity, not a hash or a sampled outcome.
#[derive(Clone, Debug)]
pub struct EqualityCertificate {
    scope: Rc<()>,
    identity: u64,
    power: i32,
}

#[derive(Clone, Debug)]
pub struct CandidateInterval {
    pub id: CandidateId,
    pub tie: CanonicalTie,
    pub score: F64Interval,
    pub payoff: F64Interval,
    /// Only mathematically exact values belong here. Neither an interval midpoint nor a rounded endpoint does.
    pub exact_score: Option<ExactExpectation>,
    pub exact_payoff: Option<ExactExpectation>,
    pub equality: Option<EqualityCertificate>,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum RemainingDomain {
    Exhausted,
    /// Upper bound on every unvisited candidate. None means no finite bound has been proved.
    Open {
        upper: Option<f64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefinementRequest {
    pub candidate: CandidateId,
    pub competitor: Option<CandidateId>,
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub struct RankingProof {
    /// This prefix has a proved ordering against every live and unseen competitor.
    pub ordered_prefix: Vec<CandidateId>,
    pub ambiguous: Vec<CandidateId>,
    pub complete: bool,
    pub refinement: Option<RefinementRequest>,
    pub pruned: u64,
}

pub struct IntervalTopK {
    k: usize,
    candidates: BTreeMap<CandidateId, CandidateInterval>,
    scope: Rc<()>,
    identities: BTreeMap<(Vec<u8>, i32, Vec<u8>), u64>,
    pruned: u64,
}

fn invalid(message: &str) -> Error {
    Error::Domain(format!("certified ranking: {message}"))
}

fn finite(value: F64Interval) -> bool {
    value.lower().is_finite() && value.upper().is_finite()
}

/// Exact comparison against a binary64 endpoint, using binary long division without a cross product.
fn compare_exact_real(value: ExactExpectation, endpoint: f64) -> Result<Ordering, Error> {
    if value.denominator == 0 || !endpoint.is_finite() {
        return Err(invalid("invalid exact comparison"));
    }
    let sign = value.numerator.signum();
    let other_sign = if endpoint > 0.0 {
        1
    } else if endpoint < 0.0 {
        -1
    } else {
        0
    };
    if sign != other_sign {
        return Ok(sign.cmp(&other_sign));
    }
    if sign == 0 {
        return Ok(Ordering::Equal);
    }
    let (n, d) = (value.numerator.unsigned_abs(), value.denominator);
    let double = |r: u128, d: u128| {
        if r >= d - r { (1u64, r - (d - r)) } else { (0u64, r + r) }
    };
    let (exponent, mut remainder, denominator) = if n >= d {
        let mut shift = n.ilog2() - d.ilog2();
        if n < d << shift {
            shift -= 1;
        }
        let scaled = d << shift;
        (shift as i32, n - scaled, scaled)
    } else {
        let (mut exponent, mut remainder) = (0i32, n);
        loop {
            exponent -= 1;
            let (bit, rest) = double(remainder, d);
            remainder = rest;
            if bit == 1 {
                break;
            }
        }
        (exponent, remainder, d)
    };
    let bits = endpoint.abs().to_bits();
    let raw_exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = (bits & ((1u64 << 52) - 1)) | if raw_exponent == 0 { 0 } else { 1u64 << 52 };
    let shift = if raw_exponent == 0 { -1074 } else { raw_exponent - 1023 - 52 };
    let highest = mantissa.ilog2();
    let mut ordering = exponent.cmp(&(shift + highest as i32));
    if ordering == Ordering::Equal {
        for bit in (0..highest).rev() {
            let (actual, rest) = double(remainder, denominator);
            remainder = rest;
            ordering = actual.cmp(&((mantissa >> bit) & 1));
            if ordering != Ordering::Equal {
                break;
            }
        }
        if ordering == Ordering::Equal && remainder != 0 {
            ordering = Ordering::Greater;
        }
    }
    Ok(if sign < 0 { ordering.reverse() } else { ordering })
}

pub fn exact_in_interval(value: ExactExpectation, interval: F64Interval) -> Result<bool, Error> {
    Ok(!compare_exact_real(value, interval.lower())?.is_lt() && !compare_exact_real(value, interval.upper())?.is_gt())
}

/// Compare signed fractions without overflowing a cross product, including i128::MIN/u128::MAX.
pub fn compare_exact(a: ExactExpectation, b: ExactExpectation) -> Result<Ordering, Error> {
    if a.denominator == 0 || b.denominator == 0 {
        return Err(invalid("zero exact denominator"));
    }
    if a.numerator.signum() != b.numerator.signum() {
        return Ok(a.numerator.signum().cmp(&b.numerator.signum()));
    }
    let (mut an, mut ad, mut bn, mut bd) =
        (a.numerator.unsigned_abs(), a.denominator, b.numerator.unsigned_abs(), b.denominator);
    let mut reverse = a.numerator < 0;
    loop {
        let q = (an / ad).cmp(&(bn / bd));
        if q != Ordering::Equal {
            return Ok(if reverse { q.reverse() } else { q });
        }
        let (ar, br) = (an % ad, bn % bd);
        if ar == 0 || br == 0 {
            let q = ar.cmp(&br);
            return Ok(if reverse { q.reverse() } else { q });
        }
        (an, ad, bn, bd) = (ad, ar, bd, br);
        reverse = !reverse;
    }
}

fn same_certificate(a: &CandidateInterval, b: &CandidateInterval) -> bool {
    match (&a.equality, &b.equality) {
        (Some(a), Some(b)) => Rc::ptr_eq(&a.scope, &b.scope) && a.identity == b.identity,
        _ => false,
    }
}

/// A proved bound of a true payoff: the exact value when known, otherwise an enclosure endpoint.
#[derive(Clone, Copy, Debug)]
enum PayoffBound {
    Exact(ExactExpectation),
    Real(f64),
}

fn compare_bound(a: PayoffBound, b: PayoffBound) -> Ordering {
    match (a, b) {
        (PayoffBound::Exact(x), PayoffBound::Exact(y)) => compare_exact(x, y),
        (PayoffBound::Exact(x), PayoffBound::Real(y)) => compare_exact_real(x, y),
        (PayoffBound::Real(x), PayoffBound::Exact(y)) => compare_exact_real(y, x).map(Ordering::reverse),
        (PayoffBound::Real(x), PayoffBound::Real(y)) => x.partial_cmp(&y).ok_or_else(|| invalid("NaN bound")),
    }
    .expect("validated finite bounds")
}

impl CandidateInterval {
    fn payoff_lower(&self) -> PayoffBound {
        self.exact_payoff.map_or(PayoffBound::Real(self.payoff.lower()), PayoffBound::Exact)
    }
    fn payoff_upper(&self) -> PayoffBound {
        self.exact_payoff.map_or(PayoffBound::Real(self.payoff.upper()), PayoffBound::Exact)
    }
}

/// Greater means a ranks before b: its payoff is proved larger, or proved at least as large while its canonical
/// tie is better. Bounds that only overlap prove nothing.
fn relation(a: &CandidateInterval, b: &CandidateInterval) -> Option<Ordering> {
    let tie = a.tie.better(&b.tie);
    if same_certificate(a, b) {
        return Some(tie);
    }
    let above = compare_bound(a.payoff_lower(), b.payoff_upper());
    let below = compare_bound(b.payoff_lower(), a.payoff_upper());
    if above == Ordering::Greater || (above == Ordering::Equal && tie == Ordering::Greater) {
        Some(Ordering::Greater)
    } else if below == Ordering::Greater || (below == Ordering::Equal && tie == Ordering::Less) {
        Some(Ordering::Less)
    } else {
        None
    }
}

impl IntervalTopK {
    pub fn new(k: usize) -> Result<Self, Error> {
        if k == 0 {
            return Err(invalid("positive K required"));
        }
        Ok(Self { k, candidates: BTreeMap::new(), scope: Rc::new(()), identities: BTreeMap::new(), pruned: 0 })
    }

    /// The caller proves that `complete_program_and_law` names the entire random program, scenario, parameters,
    /// order law and pairing, and `payoff_mapping` the complete terminal payoff mapping. No truncated digest,
    /// final score, bound shape or statistical agreement is a valid identity. Interning checks full byte equality.
    pub fn certify_equal_program(
        &mut self,
        complete_program_and_law: Vec<u8>,
        power: i32,
        payoff_mapping: Vec<u8>,
    ) -> EqualityCertificate {
        let next = self.identities.len() as u64;
        let identity = *self.identities.entry((complete_program_and_law, power, payoff_mapping)).or_insert(next);
        EqualityCertificate { scope: self.scope.clone(), identity, power }
    }

    fn validate(&self, candidate: &CandidateInterval) -> Result<(), Error> {
        if !finite(candidate.score) || !finite(candidate.payoff) {
            return Err(invalid("finite enclosures required"));
        }
        for exact in [candidate.exact_score, candidate.exact_payoff].into_iter().flatten() {
            if exact.denominator == 0 {
                return Err(invalid("zero exact denominator"));
            }
        }
        for (exact, interval) in [(candidate.exact_score, candidate.score), (candidate.exact_payoff, candidate.payoff)]
        {
            if let Some(exact) = exact
                && !exact_in_interval(exact, interval)?
            {
                return Err(invalid("exact value lies outside its enclosure"));
            }
        }
        if candidate.equality.as_ref().is_some_and(|c| !Rc::ptr_eq(&c.scope, &self.scope)) {
            return Err(invalid("equality certificate belongs to another request"));
        }
        if candidate.equality.as_ref().is_some_and(|c| c.power != candidate.tie.power) {
            return Err(invalid("equality certificate uses a different deck power"));
        }
        Ok(())
    }

    pub fn insert(&mut self, candidate: CandidateInterval) -> Result<(), Error> {
        self.validate(&candidate)?;
        if self.candidates.contains_key(&candidate.id) || self.candidates.values().any(|c| c.tie == candidate.tie) {
            return Err(invalid("duplicate candidate identity or canonical team"));
        }
        self.check_equal_class(&candidate)?;
        self.candidates.insert(candidate.id, candidate);
        self.tighten_equal_classes();
        self.prune();
        Ok(())
    }

    fn check_equal_class(&self, candidate: &CandidateInterval) -> Result<(), Error> {
        let (mut score, mut payoff) = (candidate.score, candidate.payoff);
        for other in self.candidates.values().filter(|c| c.id != candidate.id && same_certificate(c, candidate)) {
            score = score.intersect(other.score).ok_or_else(|| invalid("equal programs have disjoint score bounds"))?;
            payoff =
                payoff.intersect(other.payoff).ok_or_else(|| invalid("equal mappings have disjoint payoff bounds"))?;
            for (a, b) in [(candidate.exact_score, other.exact_score), (candidate.exact_payoff, other.exact_payoff)] {
                if let (Some(a), Some(b)) = (a, b)
                    && compare_exact(a, b)? != Ordering::Equal
                {
                    return Err(invalid("equal programs disagree on exact value"));
                }
            }
        }
        Ok(())
    }

    fn tighten_equal_classes(&mut self) {
        let mut classes =
            BTreeMap::<u64, (F64Interval, F64Interval, Option<ExactExpectation>, Option<ExactExpectation>)>::new();
        for c in self.candidates.values() {
            if let Some(certificate) = &c.equality {
                let v =
                    classes.entry(certificate.identity).or_insert((c.score, c.payoff, c.exact_score, c.exact_payoff));
                v.0 = v.0.intersect(c.score).expect("checked equal class");
                v.1 = v.1.intersect(c.payoff).expect("checked equal class");
                v.2 = v.2.or(c.exact_score);
                v.3 = v.3.or(c.exact_payoff);
            }
        }
        for c in self.candidates.values_mut() {
            if let Some(certificate) = &c.equality {
                let (score, payoff, exact_score, exact_payoff) = classes[&certificate.identity];
                (c.score, c.payoff, c.exact_score, c.exact_payoff) = (score, payoff, exact_score, exact_payoff);
            }
        }
    }

    /// Revisions reject stale refinement responses. A refinement must retain every previously proved restriction.
    pub fn refine(
        &mut self,
        id: CandidateId,
        expected_revision: u64,
        score: F64Interval,
        payoff: F64Interval,
        exact_score: Option<ExactExpectation>,
        exact_payoff: Option<ExactExpectation>,
    ) -> Result<(), Error> {
        let old = self.candidates.get(&id).ok_or_else(|| invalid("candidate no longer in frontier"))?;
        if old.revision != expected_revision {
            return Err(invalid("stale refinement revision"));
        }
        if old.score.intersect(score) != Some(score) || old.payoff.intersect(payoff) != Some(payoff) {
            return Err(invalid("refinement widens or contradicts its enclosure"));
        }
        for (previous, next) in [(old.exact_score, exact_score), (old.exact_payoff, exact_payoff)] {
            if let (Some(previous), Some(next)) = (previous, next)
                && compare_exact(previous, next)? != Ordering::Equal
            {
                return Err(invalid("refinement changes an exact value"));
            }
        }
        let mut next = old.clone();
        (next.score, next.payoff) = (score, payoff);
        next.exact_score = next.exact_score.or(exact_score);
        next.exact_payoff = next.exact_payoff.or(exact_payoff);
        next.revision = next.revision.checked_add(1).ok_or_else(|| invalid("revision overflow"))?;
        self.validate(&next)?;
        self.check_equal_class(&next)?;
        self.candidates.insert(id, next);
        self.tighten_equal_classes();
        self.prune();
        Ok(())
    }

    pub fn get(&self, id: CandidateId) -> Option<&CandidateInterval> {
        self.candidates.get(&id)
    }
    pub fn len(&self) -> usize {
        self.candidates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Node cutoff on the grid of payoff numerators over `denominator`. A team whose payoff is at most
    /// U/denominator ranks behind K candidates when U < threshold, and when U == threshold with a power below the
    /// returned power (i32::MIN when no K candidates prove that tie).
    pub fn grid_cutoff(&self, denominator: u128) -> Option<(i128, i32)> {
        if self.candidates.len() < self.k || denominator == 0 {
            return None;
        }
        let grid = |numerator| PayoffBound::Exact(ExactExpectation { numerator, denominator });
        let mut steps = Vec::with_capacity(self.candidates.len());
        for c in self.candidates.values() {
            // The least grid point at or above the proved lower bound. Every grid point below it is strictly below
            // the candidate's payoff, and the point itself is at most that payoff exactly when it equals the bound.
            let lower = c.payoff_lower();
            let approx = match lower {
                PayoffBound::Exact(x) => x.numerator as f64 / x.denominator as f64,
                PayoffBound::Real(v) => v,
            } * denominator as f64;
            let approx = approx.ceil();
            if !(approx.is_finite() && approx.abs() < 2f64.powi(100)) {
                return None;
            }
            let mut t = approx as i128;
            while compare_bound(grid(t), lower) == Ordering::Less {
                t += 1;
            }
            while compare_bound(grid(t - 1), lower) != Ordering::Less {
                t -= 1;
            }
            steps.push((t, compare_bound(grid(t), lower) == Ordering::Equal, c.tie.power));
        }
        steps.sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
        let threshold = steps[self.k - 1].0;
        // Candidates above the threshold beat any power; those at it beat only a smaller power.
        let above = steps.iter().take_while(|s| s.0 > threshold).count();
        let mut tied: Vec<_> = steps.iter().filter(|s| s.0 == threshold && s.1).map(|s| s.2).collect();
        tied.sort_unstable_by(|a, b| b.cmp(a));
        Some((threshold, tied.get(self.k - above - 1).copied().unwrap_or(i32::MIN)))
    }

    /// A candidate behind K proved-better candidates is outside every Top-K. Proved relations hold for the true
    /// payoffs, so the evidence against every other candidate survives the removal.
    fn prune(&mut self) {
        if self.candidates.len() <= self.k {
            return;
        }
        let dropped: Vec<_> = self
            .candidates
            .values()
            .filter(|c| {
                self.candidates
                    .values()
                    .filter(|d| d.id != c.id && relation(d, c) == Some(Ordering::Greater))
                    .take(self.k)
                    .count()
                    == self.k
            })
            .map(|c| c.id)
            .collect();
        for id in &dropped {
            self.candidates.remove(id);
        }
        self.pruned += dropped.len() as u64;
    }

    pub fn proof(&self, domain: RemainingDomain) -> Result<RankingProof, Error> {
        let unseen = match domain {
            RemainingDomain::Exhausted => None,
            RemainingDomain::Open { upper: Some(upper) } if upper.is_finite() => Some(upper),
            RemainingDomain::Open { upper: None } => Some(f64::INFINITY),
            _ => return Err(invalid("invalid unseen upper bound")),
        };
        let mut remaining: Vec<_> = self.candidates.values().collect();
        // This ordering schedules proof attempts only. It never becomes the returned ranking by itself.
        remaining.sort_by(|a, b| {
            b.payoff
                .upper()
                .total_cmp(&a.payoff.upper())
                .then_with(|| b.payoff.lower().total_cmp(&a.payoff.lower()))
                .then_with(|| b.tie.better(&a.tie))
        });
        let mut ordered_prefix = Vec::new();
        while ordered_prefix.len() < self.k && !remaining.is_empty() {
            let first = remaining[0];
            let winner = remaining.iter().position(|candidate| {
                (candidate.id == first.id || relation(candidate, first) == Some(Ordering::Greater))
                    && unseen.is_none_or(|upper| {
                        upper.is_finite()
                            && compare_bound(candidate.payoff_lower(), PayoffBound::Real(upper)) == Ordering::Greater
                    })
                    && remaining
                        .iter()
                        .all(|other| candidate.id == other.id || relation(candidate, other) == Some(Ordering::Greater))
            });
            let Some(winner) = winner else { break };
            ordered_prefix.push(remaining.remove(winner).id);
        }
        let complete =
            ordered_prefix.len() == self.k || (matches!(domain, RemainingDomain::Exhausted) && remaining.is_empty());
        let ambiguous = if complete { Vec::new() } else { remaining.iter().map(|c| c.id).collect() };
        let refinement = if complete {
            None
        } else {
            remaining.first().map(|c| RefinementRequest {
                candidate: c.id,
                revision: c.revision,
                competitor: remaining.iter().skip(1).find(|other| relation(c, other).is_none()).map(|other| other.id),
            })
        };
        Ok(RankingProof { ordered_prefix, ambiguous, complete, refinement, pruned: self.pruned })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(id: u64, lo: f64, hi: f64) -> CandidateInterval {
        CandidateInterval {
            id,
            tie: CanonicalTie { power: 100, key: vec![id as i64] },
            score: F64Interval::new(lo, hi).unwrap(),
            payoff: F64Interval::new(lo, hi).unwrap(),
            exact_score: None,
            exact_payoff: None,
            equality: None,
            revision: 0,
        }
    }
    #[test]
    fn overlap_touch_and_unseen_work_never_claim_complete() {
        let mut f = IntervalTopK::new(1).unwrap();
        f.insert(candidate(1, 10.0, 12.0)).unwrap();
        f.insert(candidate(2, 12.0, 13.0)).unwrap();
        let proof = f.proof(RemainingDomain::Exhausted).unwrap();
        assert!(!proof.complete && proof.ordered_prefix.is_empty());
        assert_eq!(proof.ambiguous.len(), 2);
        f.refine(2, 0, F64Interval::point(12.5).unwrap(), F64Interval::point(12.5).unwrap(), None, None).unwrap();
        assert_eq!(f.len(), 1);
        assert!(!f.proof(RemainingDomain::Open { upper: Some(12.5) }).unwrap().complete);
        assert!(f.proof(RemainingDomain::Open { upper: Some(12.49) }).unwrap().complete);
        assert_eq!(f.proof(RemainingDomain::Exhausted).unwrap().ordered_prefix, [2]);
    }
    #[test]
    fn entire_program_identity_proves_ties_without_fake_exact_values() {
        let mut f = IntervalTopK::new(2).unwrap();
        let certificate = f.certify_equal_program(vec![1, 2, 3], 100, vec![9]);
        for id in [3, 1, 2] {
            let mut c = candidate(id, 10.0, 11.0);
            c.equality = Some(certificate.clone());
            f.insert(c).unwrap();
        }
        let proof = f.proof(RemainingDomain::Exhausted).unwrap();
        assert!(proof.complete);
        assert_eq!(proof.ordered_prefix, [1, 2]);
        assert!(f.get(1).unwrap().exact_payoff.is_none());
        let mut other = IntervalTopK::new(1).unwrap();
        let mut c = candidate(5, 10.0, 11.0);
        c.equality = Some(certificate);
        assert!(other.insert(c).is_err());
    }
    #[test]
    fn boundary_refinement_and_stale_responses_preserve_topk() {
        let mut f = IntervalTopK::new(2).unwrap();
        for (id, lo, hi) in [(1, 30.0, 31.0), (2, 20.0, 22.0), (3, 21.0, 23.0), (4, 1.0, 2.0)] {
            f.insert(candidate(id, lo, hi)).unwrap();
        }
        let proof = f.proof(RemainingDomain::Exhausted).unwrap();
        assert_eq!(proof.ordered_prefix, [1]);
        assert!(!proof.complete);
        assert_eq!(proof.pruned, 1);
        let v = F64Interval::new(22.5, 22.75).unwrap();
        f.refine(3, 0, v, v, None, None).unwrap();
        assert!(f.refine(3, 0, v, v, None, None).is_err());
        assert!(f.refine(3, 1, F64Interval::new(22.0, 23.0).unwrap(), v, None, None).is_err());
        assert_eq!(f.proof(RemainingDomain::Exhausted).unwrap().ordered_prefix, [1, 3]);
    }
    fn exact(id: u64, power: i32, numerator: i128) -> CandidateInterval {
        // Aggregated exact means carry outward-rounded enclosures, never a singleton.
        let value = numerator as f64;
        let mut c = candidate(id, value.next_down(), value.next_up());
        c.tie.power = power;
        c.exact_payoff = Some(ExactExpectation { numerator, denominator: 1 });
        c
    }
    #[test]
    fn exact_power_ties_close_the_grid_cutoff_and_the_frontier() {
        let mut f = IntervalTopK::new(3).unwrap();
        for c in [exact(1, 10, 2), exact(2, 50, 1), exact(3, 70, 1), exact(4, 60, 1), candidate(5, 0.5, 0.9)] {
            f.insert(c).unwrap();
        }
        // Grid 120 per unit: one candidate above 120 and two tied at 120 with powers 70 and 60.
        assert_eq!(f.grid_cutoff(120), Some((120, 60)));
        assert!(f.get(2).is_none() && f.get(5).is_none());
        let proof = f.proof(RemainingDomain::Exhausted).unwrap();
        assert!(proof.complete);
        assert_eq!(proof.ordered_prefix, [1, 3, 4]);
        assert!(!f.proof(RemainingDomain::Open { upper: Some(1.0) }).unwrap().complete);
    }
    #[test]
    fn grid_ties_need_a_lower_bound_on_the_grid() {
        let mut f = IntervalTopK::new(1).unwrap();
        f.insert(candidate(1, 0.5, 0.6)).unwrap();
        assert_eq!(f.grid_cutoff(120), Some((60, 100)));
        let mut f = IntervalTopK::new(1).unwrap();
        f.insert(candidate(1, 0.1, 0.2)).unwrap();
        // 12/120 lies below the binary64 value 0.1, so only 13 and above bound this candidate strictly.
        assert_eq!(f.grid_cutoff(120), Some((13, i32::MIN)));
        let mut f = IntervalTopK::new(2).unwrap();
        f.insert(exact(1, 90, 1)).unwrap();
        f.insert(candidate(2, 1.0, 1.5)).unwrap();
        assert_eq!(f.grid_cutoff(120), Some((120, 90)));
        assert!(f.grid_cutoff(0).is_none());
    }
    #[test]
    fn exact_fraction_comparison_does_not_cross_multiply() {
        let q = |n, d| ExactExpectation { numerator: n, denominator: d };
        assert_eq!(compare_exact(q(i128::MAX, u128::MAX), q(i128::MAX - 1, u128::MAX)).unwrap(), Ordering::Greater);
        assert_eq!(compare_exact(q(i128::MIN, u128::MAX), q(-1, 2)).unwrap(), Ordering::Less);
        assert_eq!(compare_exact(q(1, 3), q(2, 6)).unwrap(), Ordering::Equal);
        assert!(compare_exact(q(1, 0), q(0, 1)).is_err());
        assert_eq!(compare_exact_real(q(1, 3), 1.0 / 3.0).unwrap(), Ordering::Greater);
        assert_eq!(compare_exact_real(q(1, u128::MAX), f64::from_bits(1)).unwrap(), Ordering::Greater);
        assert_eq!(compare_exact_real(q(i128::MIN, 1), i128::MIN as f64).unwrap(), Ordering::Equal);
        assert!(!exact_in_interval(q(1, 3), F64Interval::point(1.0 / 3.0).unwrap()).unwrap());
    }
}
