//! Numerical building blocks for certified expectations.
//!
//! These operators enclose arithmetic, not the live's control flow. A caller must still establish that its
//! probability events, native command history, score snapshots and wrapping checks describe every admitted play.
//! Binary64 intervals enclose REAL arithmetic; binary32 intervals enclose the results of native RN32 operations.
//! Keeping these meanings separate is essential: flooring a mean is not the mean of the native note scores.

use crate::error::Error;
use crate::num::floor_to_i32;

fn invalid(message: &str) -> Error {
    Error::Domain(format!("certified arithmetic: {message}"))
}

/// An enclosure of finite real values. Infinite endpoints mean an unbounded enclosure, never a NaN value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct F64Interval {
    lower: f64,
    upper: f64,
}

#[allow(clippy::should_implement_trait)] // Named methods make the two different arithmetic models explicit.
impl F64Interval {
    pub const ZERO: Self = Self { lower: 0.0, upper: 0.0 };
    pub const ONE: Self = Self { lower: 1.0, upper: 1.0 };
    pub const WHOLE: Self = Self { lower: f64::NEG_INFINITY, upper: f64::INFINITY };

    pub fn new(lower: f64, upper: f64) -> Result<Self, Error> {
        if lower.is_nan() || upper.is_nan() || lower > upper || lower == f64::INFINITY || upper == f64::NEG_INFINITY {
            return Err(invalid("invalid real interval"));
        }
        Ok(Self { lower, upper })
    }

    /// A finite binary64 value interpreted as its exact real value.
    pub fn point(value: f64) -> Result<Self, Error> {
        if !value.is_finite() {
            return Err(invalid("a real point must be finite"));
        }
        Ok(Self { lower: value, upper: value })
    }

    /// The cast may round large integers; never treat the rounded cast as an exact integer.
    pub fn integer(value: i128) -> Self {
        let value_f64 = value as f64;
        if (-9_007_199_254_740_992..=9_007_199_254_740_992).contains(&value) {
            Self { lower: value_f64, upper: value_f64 }
        } else {
            Self { lower: value_f64.next_down(), upper: value_f64.next_up() }
        }
    }

    pub fn lower(self) -> f64 {
        self.lower
    }

    pub fn upper(self) -> f64 {
        self.upper
    }

    pub fn contains(self, value: f64) -> bool {
        !value.is_nan() && self.lower <= value && value <= self.upper
    }

    pub fn is_point(self) -> bool {
        self.lower == self.upper
    }

    pub fn hull(self, other: Self) -> Self {
        Self { lower: self.lower.min(other.lower), upper: self.upper.max(other.upper) }
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        Self::new(self.lower.max(other.lower), self.upper.min(other.upper)).ok()
    }

    pub fn negate(self) -> Self {
        Self { lower: -self.upper, upper: -self.lower }
    }

    pub fn add(self, other: Self) -> Self {
        if self == Self::ZERO {
            return other;
        }
        if other == Self::ZERO {
            return self;
        }
        Self { lower: (self.lower + other.lower).next_down(), upper: (self.upper + other.upper).next_up() }
    }

    pub fn subtract(self, other: Self) -> Self {
        if self.is_point() && self == other {
            return Self::ZERO;
        }
        self.add(other.negate())
    }

    pub fn multiply(self, other: Self) -> Self {
        if self == Self::ZERO || other == Self::ZERO {
            return Self::ZERO;
        }
        if self == Self::ONE {
            return other;
        }
        if other == Self::ONE {
            return self;
        }
        // Here infinities are bounds on finite real operands. A zero bound times an infinite bound is zero.
        let product = |a: f64, b: f64| if a == 0.0 || b == 0.0 { 0.0 } else { a * b };
        let corners = [
            product(self.lower, other.lower),
            product(self.lower, other.upper),
            product(self.upper, other.lower),
            product(self.upper, other.upper),
        ];
        Self {
            lower: corners.into_iter().fold(f64::INFINITY, f64::min).next_down(),
            upper: corners.into_iter().fold(f64::NEG_INFINITY, f64::max).next_up(),
        }
    }

    pub fn divide(self, other: Self) -> Result<Self, Error> {
        if other.contains(0.0) {
            return Err(invalid("real division interval contains zero"));
        }
        if other == Self::ONE {
            return Ok(self);
        }
        if self == Self::ZERO {
            return Ok(Self::ZERO);
        }
        let reciprocal = Self { lower: (1.0 / other.upper).next_down(), upper: (1.0 / other.lower).next_up() };
        Ok(self.multiply(reciprocal))
    }

    pub fn scale_integer(self, value: i128) -> Self {
        self.multiply(Self::integer(value))
    }

    pub fn compare(self, other: Self) -> CertifiedComparison {
        if self.lower > other.upper {
            CertifiedComparison::Greater
        } else if self.upper < other.lower {
            CertifiedComparison::Less
        } else if self.is_point() && other.is_point() && self.lower == other.lower {
            CertifiedComparison::Equal
        } else {
            CertifiedComparison::Unresolved
        }
    }
}

/// Only strict separation or two equal exact singleton enclosures establish a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertifiedComparison {
    Greater,
    Equal,
    Less,
    Unresolved,
}

/// An enclosure of one event's probability. Multiplication requires independent or conditional event masses;
/// merging requires mutually exclusive events. Those probabilistic premises belong to the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbabilityMass(F64Interval);

impl ProbabilityMass {
    pub const ZERO: Self = Self(F64Interval::ZERO);
    pub const ONE: Self = Self(F64Interval::ONE);

    pub fn from_ratio(weight: u64, total: u64) -> Result<Self, Error> {
        if total == 0 || weight > total {
            return Err(invalid("probability ratio must satisfy 0 <= weight <= positive total"));
        }
        if weight == 0 {
            return Ok(Self::ZERO);
        }
        if weight == total {
            return Ok(Self::ONE);
        }
        Ok(Self::clip(F64Interval::integer(i128::from(weight)).divide(F64Interval::integer(i128::from(total)))?))
    }

    /// A native binary32 probability is an exact dyadic rational, including its original rounding.
    pub fn from_f32(value: f32) -> Result<Self, Error> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(invalid("native probability must lie in [0, 1]"));
        }
        Ok(Self(F64Interval::point(f64::from(value))?))
    }

    /// Supply an already proved enclosure; this does not prove a nominal probability calculation.
    pub fn from_bounds(lower: f64, upper: f64) -> Result<Self, Error> {
        if lower < 0.0 || upper > 1.0 {
            return Err(invalid("probability enclosure must lie in [0, 1]"));
        }
        Ok(Self(F64Interval::new(lower, upper)?))
    }

    fn clip(value: F64Interval) -> Self {
        // Intersect a proved enclosure with the known probability range; never rescale or drop small masses.
        Self(F64Interval { lower: value.lower.clamp(0.0, 1.0), upper: value.upper.clamp(0.0, 1.0) })
    }

    pub fn interval(self) -> F64Interval {
        self.0
    }

    pub fn multiply(self, other: Self) -> Self {
        if self == Self::ZERO || other == Self::ZERO {
            return Self::ZERO;
        }
        if self == Self::ONE {
            return other;
        }
        if other == Self::ONE {
            return self;
        }
        // Probability endpoints are nonnegative: two products suffice in the hot DP transition path.
        Self(F64Interval {
            lower: (self.0.lower * other.0.lower).next_down().max(0.0),
            upper: (self.0.upper * other.0.upper).next_up().min(1.0),
        })
    }

    pub fn merge_disjoint(self, other: Self) -> Self {
        Self::clip(self.0.add(other.0))
    }

    pub fn complement(self) -> Self {
        if self == Self::ZERO {
            return Self::ONE;
        }
        if self == Self::ONE {
            return Self::ZERO;
        }
        Self::clip(F64Interval::ONE.subtract(self.0))
    }

    pub fn weighted_integer(self, value: i32) -> F64Interval {
        self.0.scale_integer(i128::from(value))
    }
}

/// An enclosure of possible native signed score integers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I32Interval {
    lower: i32,
    upper: i32,
}

impl I32Interval {
    pub const WHOLE: Self = Self { lower: i32::MIN, upper: i32::MAX };

    pub fn new(lower: i32, upper: i32) -> Result<Self, Error> {
        if lower > upper {
            return Err(invalid("invalid integer interval"));
        }
        Ok(Self { lower, upper })
    }

    pub fn point(value: i32) -> Self {
        Self { lower: value, upper: value }
    }

    pub fn lower(self) -> i32 {
        self.lower
    }

    pub fn upper(self) -> i32 {
        self.upper
    }

    pub fn contains(self, value: i32) -> bool {
        self.lower <= value && value <= self.upper
    }

    pub fn as_real(self) -> F64Interval {
        F64Interval { lower: f64::from(self.lower), upper: f64::from(self.upper) }
    }

    fn wrapping_range(lower: i64, upper: i64) -> Self {
        // A signed modular interval is monotone between wrap boundaries, otherwise its hull is the full range.
        let bucket = |value: i64| (value - i64::from(i32::MIN)).div_euclid(1i64 << 32);
        if bucket(lower) == bucket(upper) { Self { lower: lower as i32, upper: upper as i32 } } else { Self::WHOLE }
    }

    pub fn wrapping_add(self, other: Self) -> Self {
        Self::wrapping_range(
            i64::from(self.lower) + i64::from(other.lower),
            i64::from(self.upper) + i64::from(other.upper),
        )
    }

    pub fn wrapping_subtract(self, other: Self) -> Self {
        Self::wrapping_range(
            i64::from(self.lower) - i64::from(other.upper),
            i64::from(self.upper) - i64::from(other.lower),
        )
    }
}

/// An enclosure of actual binary32 values. Operations use native RN32 at the interval endpoints, without an
/// additional outward ulp. An operation that might produce NaN is rejected, because a numeric hull cannot retain
/// its later native NaN-to-integer behavior. Infinite values from overflow are retained and handled by native_floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct F32Interval {
    lower: f32,
    upper: f32,
}

#[allow(clippy::should_implement_trait)] // These named native operations are fallible when NaN is possible.
impl F32Interval {
    pub fn new(lower: f32, upper: f32) -> Result<Self, Error> {
        if lower.is_nan() || upper.is_nan() || lower > upper {
            return Err(invalid("invalid native float interval"));
        }
        Ok(Self { lower, upper })
    }

    pub fn point(value: f32) -> Result<Self, Error> {
        Self::new(value, value)
    }

    /// Enclose all binary32 values contained in an established real enclosure. The extra boundary float is safe.
    pub fn from_real(value: F64Interval) -> Self {
        let mut lower = value.lower as f32;
        let mut upper = value.upper as f32;
        if f64::from(lower) > value.lower {
            lower = lower.next_down();
        }
        if f64::from(upper) < value.upper {
            upper = upper.next_up();
        }
        Self { lower, upper }
    }

    pub fn lower(self) -> f32 {
        self.lower
    }

    pub fn upper(self) -> f32 {
        self.upper
    }

    pub fn contains(self, value: f32) -> bool {
        !value.is_nan() && self.lower <= value && value <= self.upper
    }

    pub fn hull(self, other: Self) -> Self {
        Self { lower: self.lower.min(other.lower), upper: self.upper.max(other.upper) }
    }

    fn corners(self, other: Self, operation: impl Fn(f32, f32) -> f32) -> Result<Self, Error> {
        let mut lower = f32::INFINITY;
        let mut upper = f32::NEG_INFINITY;
        for a in [self.lower, self.upper] {
            for b in [other.lower, other.upper] {
                let value = operation(a, b);
                if value.is_nan() {
                    return Err(invalid("native operation may produce NaN"));
                }
                lower = lower.min(value);
                upper = upper.max(value);
            }
        }
        Self::new(lower, upper)
    }

    pub fn add(self, other: Self) -> Result<Self, Error> {
        self.corners(other, |a, b| a + b)
    }

    pub fn subtract(self, other: Self) -> Result<Self, Error> {
        self.corners(other, |a, b| a - b)
    }

    pub fn multiply(self, other: Self) -> Result<Self, Error> {
        // An interior zero times an infinite endpoint is also NaN, even when all four corners are nonzero.
        if (self.contains(0.0) && (other.lower.is_infinite() || other.upper.is_infinite()))
            || (other.contains(0.0) && (self.lower.is_infinite() || self.upper.is_infinite()))
        {
            return Err(invalid("native multiplication may produce NaN"));
        }
        self.corners(other, |a, b| a * b)
    }

    pub fn divide(self, other: Self) -> Result<Self, Error> {
        if other.contains(0.0) {
            return Err(invalid("native division interval contains zero"));
        }
        self.corners(other, |a, b| a / b)
    }

    /// The native conversion is monotone EXCEPT at positive infinity, which maps to i32::MIN.
    pub fn native_floor(self) -> I32Interval {
        if self.lower == f32::INFINITY {
            return I32Interval::point(i32::MIN);
        }
        if self.upper == f32::INFINITY {
            return I32Interval::WHOLE;
        }
        I32Interval { lower: floor_to_i32(self.lower), upper: floor_to_i32(self.upper) }
    }

    /// The first note-score floor: native floor -> saturated i32 -> RN32 conversion back to float.
    pub fn native_floor_as_float(self) -> Self {
        let integers = self.native_floor();
        Self { lower: integers.lower as f32, upper: integers.upper as f32 }
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Exact remainder information for native truncation of `score * percent / 100` before the final i32 cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankRemainder {
    percent: i64,
    modulus: u8,
    multiplier: u64,
}

impl RankRemainder {
    pub fn new(percent: i64) -> Self {
        let divisor = gcd(percent.unsigned_abs(), 100);
        Self { percent, modulus: (100 / divisor) as u8, multiplier: percent.unsigned_abs() / divisor }
    }

    /// At most 100 residue classes; negative scores additionally require their sign for truncation toward zero.
    pub fn modulus(self) -> u8 {
        self.modulus
    }

    pub fn residue(self, score: i32) -> (bool, u8) {
        (score < 0, (score.unsigned_abs() % u32::from(self.modulus)) as u8)
    }

    /// The correction C in `trunc(score * percent / 100) = score * percent / 100 - C`.
    pub fn correction(self, negative_score: bool, residue: u8) -> Result<F64Interval, Error> {
        if residue >= self.modulus {
            return Err(invalid("rank remainder is outside its modulus"));
        }
        // Reduce first, so even i64::MIN percentages do not overflow the multiplication.
        let numerator = (self.multiplier % u64::from(self.modulus)) * u64::from(residue) % u64::from(self.modulus);
        let value = ProbabilityMass::from_ratio(numerator, u64::from(self.modulus))?.interval();
        Ok(if negative_score ^ (self.percent < 0) { value.negate() } else { value })
    }
}

fn rank_inputs(mean: F64Interval, support: I32Interval, percent: i64) -> Result<F64Interval, Error> {
    for score in [support.lower, support.upper] {
        let result = i128::from(score) * i128::from(percent) / 100;
        if i32::try_from(result).is_err() {
            return Err(invalid("rank interval may wrap its final i32 cast"));
        }
    }
    mean.intersect(support.as_real()).ok_or_else(|| invalid("rank mean and score support are disjoint"))
}

/// Bound the expected native rank bonus using the integer support and the mean of its ACTUAL range snapshot.
/// Reject wrapping; do not substitute a final note window for that snapshot. This also covers negative inputs:
/// the native integer division truncates toward zero, so its correction changes sign with score * percent.
pub fn rank_mean_bounds(mean: F64Interval, support: I32Interval, percent: i64) -> Result<F64Interval, Error> {
    let mean = rank_inputs(mean, support, percent)?;
    if percent == 0 {
        return Ok(F64Interval::ZERO);
    }
    let linear = mean.scale_integer(i128::from(percent)).divide(F64Interval::integer(100))?;
    let modulus = RankRemainder::new(percent).modulus();
    let gap = ProbabilityMass::from_ratio(u64::from(modulus - 1), u64::from(modulus))?.interval().upper();
    let a = i128::from(support.lower) * i128::from(percent);
    let b = i128::from(support.upper) * i128::from(percent);
    let correction =
        F64Interval { lower: if a.min(b) < 0 { -gap } else { 0.0 }, upper: if a.max(b) > 0 { gap } else { 0.0 } };
    Ok(linear.subtract(correction))
}

/// One event in a complete, mutually exclusive distribution of signed range-score residues.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RankResidueMass {
    pub negative_score: bool,
    pub residue: u8,
    pub mass: ProbabilityMass,
}

/// Refine the rank mean with the actual residue distribution. The caller proves that bins partition the same
/// range score used by `mean`; the mass-sum check only catches an inconsistent enclosure, not that semantic proof.
pub fn rank_mean_with_residues(
    mean: F64Interval,
    support: I32Interval,
    percent: i64,
    residues: &[RankResidueMass],
) -> Result<F64Interval, Error> {
    let mean = rank_inputs(mean, support, percent)?;
    let remainder = RankRemainder::new(percent);
    let mut mass = F64Interval::ZERO;
    let mut correction = F64Interval::ZERO;
    for bin in residues {
        mass = mass.add(bin.mass.interval());
        correction =
            correction.add(bin.mass.interval().multiply(remainder.correction(bin.negative_score, bin.residue)?));
    }
    if !mass.contains(1.0) {
        return Err(invalid("rank residue masses do not enclose total mass one"));
    }
    if percent == 0 {
        return Ok(F64Interval::ZERO);
    }
    Ok(mean.scale_integer(i128::from(percent)).divide(F64Interval::integer(100))?.subtract(correction))
}

/// Refine a native rank bonus when only part of its residue distribution is known. The signed residue bins and
/// `unresolved` must partition the same actual range score used by `mean`. Unresolved histories contribute their
/// probability mass times the full possible truncation correction; an interval reward is never split into
/// invented probability branches. With no unresolved mass this is [`rank_mean_with_residues`].
pub fn rank_mean_with_partial_residues(
    mean: F64Interval,
    support: I32Interval,
    percent: i64,
    residues: &[RankResidueMass],
    unresolved: ProbabilityMass,
) -> Result<F64Interval, Error> {
    if unresolved == ProbabilityMass::ZERO {
        return rank_mean_with_residues(mean, support, percent, residues);
    }
    let mean = rank_inputs(mean, support, percent)?;
    let remainder = RankRemainder::new(percent);
    let gap = ProbabilityMass::from_ratio(u64::from(remainder.modulus() - 1), u64::from(remainder.modulus()))?
        .interval()
        .upper();
    let a = i128::from(support.lower()) * i128::from(percent);
    let b = i128::from(support.upper()) * i128::from(percent);
    let possible =
        F64Interval { lower: if a.min(b) < 0 { -gap } else { 0.0 }, upper: if a.max(b) > 0 { gap } else { 0.0 } };
    let mut mass = unresolved.interval();
    let mut correction = mass.multiply(possible);
    for bin in residues {
        mass = mass.add(bin.mass.interval());
        correction =
            correction.add(bin.mass.interval().multiply(remainder.correction(bin.negative_score, bin.residue)?));
    }
    if !mass.contains(1.0) {
        return Err(invalid("partial rank residue masses do not enclose total mass one"));
    }
    if percent == 0 {
        return Ok(F64Interval::ZERO);
    }
    Ok(mean.scale_integer(i128::from(percent)).divide(F64Interval::integer(100))?.subtract(correction))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_rank_residues_enclose_every_unresolved_subset_of_signed_integer_laws() {
        let scores = [-32i32, -12, 0, 7, 13, 29];
        let weights = [1u64, 2, 3, 4, 5, 6];
        let total: u64 = weights.iter().sum();
        let numerator: i128 =
            scores.iter().zip(weights).map(|(&score, weight)| i128::from(score) * i128::from(weight)).sum();
        let mean = F64Interval::integer(numerator).divide(F64Interval::integer(i128::from(total))).unwrap();
        let support = I32Interval::new(-32, 29).unwrap();
        for percent in [-250, -25, 0, 10, 25, 333] {
            let remainder = RankRemainder::new(percent);
            let exact: i128 = scores
                .iter()
                .zip(weights)
                .map(|(&score, weight)| (i128::from(score) * i128::from(percent) / 100) * i128::from(weight))
                .sum();
            for known in 0..1u8 << scores.len() {
                let mut bins = Vec::new();
                let mut unresolved = 0;
                for (index, (&score, weight)) in scores.iter().zip(weights).enumerate() {
                    if known & (1 << index) == 0 {
                        unresolved += weight;
                    } else {
                        let (negative_score, residue) = remainder.residue(score);
                        bins.push(RankResidueMass {
                            negative_score,
                            residue,
                            mass: ProbabilityMass::from_ratio(weight, total).unwrap(),
                        });
                    }
                }
                let result = rank_mean_with_partial_residues(
                    mean,
                    support,
                    percent,
                    &bins,
                    ProbabilityMass::from_ratio(unresolved, total).unwrap(),
                )
                .unwrap();
                contains_fraction(result, exact, i128::from(total));
                if unresolved == 0 {
                    assert_eq!(result, rank_mean_with_residues(mean, support, percent, &bins).unwrap());
                }
            }
        }
        assert!(
            rank_mean_with_partial_residues(mean, support, 10, &[], ProbabilityMass::from_ratio(1, 2).unwrap(),)
                .is_err()
        );
    }

    // Exact dyadic conversion for the modest finite values in the rational tests. Comparisons are integer-only.
    fn fraction(value: f64) -> (i128, i128) {
        if value == 0.0 {
            return (0, 1);
        }
        assert!(value.is_finite());
        let bits = value.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023 - 52;
        let significand = i128::from((bits & ((1u64 << 52) - 1)) | (1u64 << 52));
        let sign = if value < 0.0 { -1 } else { 1 };
        if exponent >= 0 { (sign * (significand << exponent), 1) } else { (sign * significand, 1i128 << -exponent) }
    }

    fn contains_fraction(interval: F64Interval, numerator: i128, denominator: i128) {
        assert!(denominator > 0);
        if interval.lower != f64::NEG_INFINITY {
            let (n, d) = fraction(interval.lower);
            assert!(n * denominator <= numerator * d, "{interval:?} misses {numerator}/{denominator}");
        }
        if interval.upper != f64::INFINITY {
            let (n, d) = fraction(interval.upper);
            assert!(numerator * d <= n * denominator, "{interval:?} misses {numerator}/{denominator}");
        }
    }

    #[test]
    fn rational_probability_operations_enclose_small_exact_laws() {
        for total in 1..=19u64 {
            for weight in 0..=total {
                let p = ProbabilityMass::from_ratio(weight, total).unwrap();
                contains_fraction(p.interval(), i128::from(weight), i128::from(total));
                contains_fraction(p.complement().interval(), i128::from(total - weight), i128::from(total));
                for other in 0..=total - weight {
                    let q = ProbabilityMass::from_ratio(other, total).unwrap();
                    contains_fraction(p.merge_disjoint(q).interval(), i128::from(weight + other), i128::from(total));
                    contains_fraction(p.multiply(q).interval(), i128::from(weight * other), i128::from(total * total));
                }
                for score in [-109, -1, 0, 1, 317] {
                    contains_fraction(
                        p.weighted_integer(score),
                        i128::from(weight) * i128::from(score),
                        i128::from(total),
                    );
                }
            }
        }
        assert_eq!(ProbabilityMass::ZERO.multiply(ProbabilityMass::ONE), ProbabilityMass::ZERO);
        assert_eq!(ProbabilityMass::ONE.multiply(ProbabilityMass::ONE), ProbabilityMass::ONE);
        assert_eq!(ProbabilityMass::ZERO.complement(), ProbabilityMass::ONE);
        assert_eq!(ProbabilityMass::ONE.complement(), ProbabilityMass::ZERO);
    }

    #[test]
    fn signed_real_arithmetic_encloses_exact_rationals() {
        for a in -12..=12i128 {
            for b in -12..=12i128 {
                let x = F64Interval::integer(a).divide(F64Interval::integer(7)).unwrap();
                let y = F64Interval::integer(b).divide(F64Interval::integer(3)).unwrap();
                contains_fraction(x.add(y), 3 * a + 7 * b, 21);
                contains_fraction(x.subtract(y), 3 * a - 7 * b, 21);
                contains_fraction(x.multiply(y), a * b, 21);
                if b != 0 {
                    let sign = b.signum();
                    contains_fraction(x.divide(y).unwrap(), 3 * a * sign, 7 * b * sign);
                }
            }
        }
    }

    #[test]
    fn constructors_overflow_and_ambiguous_comparisons_are_explicit() {
        assert!(F64Interval::point(f64::INFINITY).is_err());
        assert!(F64Interval::new(f64::NAN, 1.0).is_err());
        assert!(F64Interval::new(2.0, 1.0).is_err());
        assert!(ProbabilityMass::from_ratio(0, 0).is_err());
        assert!(ProbabilityMass::from_ratio(2, 1).is_err());
        assert!(ProbabilityMass::from_f32(-0.1).is_err());
        assert!(ProbabilityMass::from_f32(f32::NAN).is_err());
        let large = F64Interval::point(f64::MAX).unwrap().multiply(F64Interval::integer(2));
        assert_eq!(large.lower(), f64::MAX);
        assert_eq!(large.upper(), f64::INFINITY);
        let negative = large.negate();
        assert_eq!(negative.lower(), f64::NEG_INFINITY);
        assert_eq!(negative.upper(), -f64::MAX);
        assert_eq!(large.add(negative), F64Interval::WHOLE);
        assert_eq!(F64Interval::WHOLE.multiply(F64Interval::ZERO), F64Interval::ZERO);
        assert!(F64Interval::ONE.divide(F64Interval::new(-1.0, 1.0).unwrap()).is_err());
        let a = F64Interval::new(1.0, 2.0).unwrap();
        assert_eq!(a.compare(F64Interval::new(2.0, 3.0).unwrap()), CertifiedComparison::Unresolved);
        assert_eq!(a.compare(a), CertifiedComparison::Unresolved);
        assert_eq!(F64Interval::ONE.compare(F64Interval::ONE), CertifiedComparison::Equal);
        assert_eq!(a.compare(F64Interval::ZERO), CertifiedComparison::Greater);
        assert_eq!(F64Interval::ZERO.compare(a), CertifiedComparison::Less);
    }

    #[test]
    fn large_probability_inputs_and_subnormal_mass_are_retained() {
        let p = ProbabilityMass::from_ratio(u64::MAX - 1, u64::MAX).unwrap();
        assert!(p.interval().lower() < 1.0);
        assert_eq!(p.interval().upper(), 1.0);
        assert!(p.complement().interval().upper() > 0.0);
        let tiny = ProbabilityMass::from_f32(f32::from_bits(1)).unwrap();
        assert!(tiny.multiply(tiny).interval().lower() > 0.0);
        let mut fading = tiny;
        for _ in 0..10 {
            fading = fading.multiply(tiny);
        }
        assert_eq!(fading.interval().lower(), 0.0);
        assert!(fading.interval().upper() > 0.0);
        assert!(F64Interval::integer(i128::MIN).contains(i128::MIN as f64));
        assert!(F64Interval::integer(i128::MAX).contains(i128::MAX as f64));
    }

    #[test]
    fn native_operations_enclose_an_independent_small_float_enumeration() {
        let values: Vec<f32> = (-24..=24).map(|i| i as f32 / 8.0).collect();
        for (i, &lower) in values.iter().enumerate().step_by(4) {
            for &upper in values[i..].iter().step_by(4) {
                let a = F32Interval::new(lower, upper).unwrap();
                for b in [F32Interval::new(-2.0, -0.25).unwrap(), F32Interval::new(0.25, 2.0).unwrap()] {
                    let (add, sub, mul, div) =
                        (a.add(b).unwrap(), a.subtract(b).unwrap(), a.multiply(b).unwrap(), a.divide(b).unwrap());
                    for &x in values.iter().filter(|&&x| a.contains(x)) {
                        for &y in values.iter().filter(|&&y| b.contains(y)) {
                            assert!(add.contains(x + y));
                            assert!(sub.contains(x - y));
                            assert!(mul.contains(x * y));
                            assert!(div.contains(x / y));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn native_floor_infinities_saturation_and_nan_are_not_monotone_shortcuts() {
        for value in [f32::NEG_INFINITY, -3.0e9, -1.2, -0.0, 0.0, 1.2, 3.0e9, f32::MAX, f32::INFINITY] {
            let interval = F32Interval::point(value).unwrap();
            // Independent native specification, not the optimized num::floor_to_i32 helper.
            let expected = if value == f32::INFINITY { i32::MIN } else { value.floor() as i32 };
            assert_eq!(interval.native_floor(), I32Interval::point(expected));
            assert!(interval.native_floor_as_float().contains(expected as f32));
        }
        assert_eq!(F32Interval::new(f32::MAX, f32::INFINITY).unwrap().native_floor(), I32Interval::WHOLE);
        let overflow = F32Interval::point(f32::MAX).unwrap().multiply(F32Interval::point(2.0).unwrap()).unwrap();
        assert_eq!(overflow.native_floor(), I32Interval::point(i32::MIN));
        assert!(F32Interval::new(-1.0, 1.0).unwrap().multiply(overflow).is_err());
        assert!(overflow.subtract(overflow).is_err());
        assert!(F32Interval::point(0.0).unwrap().divide(F32Interval::point(0.0).unwrap()).is_err());
        assert!(F32Interval::point(f32::NAN).is_err());
    }

    #[test]
    fn adjacent_float_rounding_and_both_native_floors_are_enclosed() {
        for center in [f32::from_bits(1), 0.5, 1.0, 16_777_216.0, 2_147_483_648.0] {
            let values = [center.next_down(), center, center.next_up()];
            let inputs = F32Interval::new(values[0], values[2]).unwrap();
            let multipliers = [0.999_999_94f32, 1.0, 1.000_000_1];
            let factors = F32Interval::new(multipliers[0], multipliers[2]).unwrap();
            let result =
                inputs.multiply(factors).unwrap().native_floor_as_float().multiply(factors).unwrap().native_floor();
            for x in values {
                for a in multipliers {
                    for b in multipliers {
                        let first = (x * a).floor() as i32;
                        let second = ((first as f32) * b).floor() as i32;
                        assert!(result.contains(second));
                    }
                }
            }
        }
    }

    #[test]
    fn two_floors_must_be_inside_the_expectation() {
        let outcomes = [0.9f32, 1.1];
        let event = F32Interval::point(1.5).unwrap();
        let mut correct = F64Interval::ZERO;
        for value in outcomes {
            let score =
                F32Interval::point(value).unwrap().native_floor_as_float().multiply(event).unwrap().native_floor();
            assert_eq!(score.lower(), score.upper());
            correct = correct.add(ProbabilityMass::from_ratio(1, 2).unwrap().weighted_integer(score.lower()));
        }
        contains_fraction(correct, 1, 2);
        let incorrect = (((outcomes[0] + outcomes[1]) / 2.0).floor() * 1.5).floor() as i32;
        assert_eq!(incorrect, 1);
        assert!(correct.upper() < f64::from(incorrect));
    }

    #[test]
    fn integer_wrapping_encloses_every_small_cross_boundary_pair() {
        let intervals = [
            I32Interval::new(i32::MIN, i32::MIN + 4).unwrap(),
            I32Interval::new(-2, 2).unwrap(),
            I32Interval::new(i32::MAX - 4, i32::MAX).unwrap(),
        ];
        for a in intervals {
            for b in intervals {
                for x in a.lower..=a.upper {
                    for y in b.lower..=b.upper {
                        assert!(a.wrapping_add(b).contains(x.wrapping_add(y)));
                        assert!(a.wrapping_subtract(b).contains(x.wrapping_sub(y)));
                    }
                }
            }
        }
    }

    #[test]
    fn rank_bounds_and_residue_refinement_match_exact_signed_laws() {
        for percent in [-199, -100, -77, -1, 0, 1, 25, 77, 100, 199] {
            let remainder = RankRemainder::new(percent);
            for scores in [[-17, -3, 0, 21], [-101, -7, -4, -1], [1, 7, 11, 100]] {
                let weights = [1, 2, 3, 4];
                let mut mean = F64Interval::ZERO;
                let mut exact_bonus = 0i128;
                let mut bins = Vec::new();
                for (&score, weight) in scores.iter().zip(weights) {
                    let mass = ProbabilityMass::from_ratio(weight, 10).unwrap();
                    mean = mean.add(mass.weighted_integer(score));
                    exact_bonus += i128::from(weight) * (i128::from(score) * i128::from(percent) / 100);
                    let (negative_score, residue) = remainder.residue(score);
                    bins.push(RankResidueMass { negative_score, residue, mass });
                }
                let support = I32Interval::new(*scores.iter().min().unwrap(), *scores.iter().max().unwrap()).unwrap();
                let coarse = rank_mean_bounds(mean, support, percent).unwrap();
                let refined = rank_mean_with_residues(mean, support, percent, &bins).unwrap();
                contains_fraction(coarse, exact_bonus, 10);
                contains_fraction(refined, exact_bonus, 10);
                assert!(refined.upper() - refined.lower() < 1.0e-9);
            }
        }
        assert!(rank_mean_bounds(F64Interval::integer(i32::MAX.into()), I32Interval::point(i32::MAX), 101).is_err());
        assert!(rank_mean_bounds(F64Interval::ONE, I32Interval::point(0), 100).is_err());
        assert!(rank_mean_with_residues(F64Interval::ZERO, I32Interval::point(0), 100, &[]).is_err());
        assert!(RankRemainder::new(25).correction(false, 4).is_err());
        // i64::MIN is accepted by the remainder arithmetic without overflowing abs() or its modular product.
        let extreme = RankRemainder::new(i64::MIN);
        assert!(extreme.correction(true, extreme.modulus() - 1).is_ok());
    }
}
