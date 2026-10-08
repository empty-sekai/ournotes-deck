//! The live's random number streams: a port of the seeded `System.Random` (Knuth's subtractive generator) and the
//! four-stream wrapper the live draws from.

use crate::error::Error;
use crate::live::certified::ProbabilityMass;
use crate::num::{trunc_f64_to_i32, trunc_f64_to_i64};

const MBIG: i32 = i32::MAX;
const MSEED: i32 = 161_803_398;

/// Stream indexes.
pub const SKILL: usize = 0;
pub const LUCK: usize = 1;
pub const PRESENTATION: usize = 2;
pub const MEMBER_SHUFFLE: usize = 3;

/// Seeded `System.Random`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl NetRandom {
    pub fn new(seed: i32) -> NetRandom {
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = MSEED.wrapping_sub(subtraction);
        let mut sa = [0i32; 56];
        sa[55] = mj;
        let mut mk: i32 = 1;
        let mut ii = 0usize;
        for _ in 1..55 {
            ii += 21;
            if ii >= 55 {
                ii -= 55;
            }
            sa[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = sa[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let mut n = i + 30;
                if n >= 55 {
                    n -= 55;
                }
                sa[i] = sa[i].wrapping_sub(sa[1 + n]);
                if sa[i] < 0 {
                    sa[i] = sa[i].wrapping_add(MBIG);
                }
            }
        }
        NetRandom { seed_array: sa, inext: 0, inextp: 21 }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut a = self.inext + 1;
        let mut b = self.inextp + 1;
        if a >= 56 {
            a = 1;
        }
        if b >= 56 {
            b = 1;
        }
        let mut r = self.seed_array[a].wrapping_sub(self.seed_array[b]);
        if r == MBIG {
            r -= 1;
        }
        if r < 0 {
            r = r.wrapping_add(MBIG);
        }
        self.seed_array[a] = r;
        self.inext = a;
        self.inextp = b;
        r
    }

    fn sample(&mut self) -> f64 {
        self.internal_sample() as f64 * (1.0 / MBIG as f64)
    }

    /// `Next()`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        self.internal_sample()
    }

    /// `Next(max)`; a negative max is an error.
    pub fn next_max(&mut self, max_value: i32) -> Result<i32, Error> {
        if max_value < 0 {
            return Err(Error::Game("Random.Next: negative max".into()));
        }
        Ok(trunc_f64_to_i32(self.sample() * max_value as f64))
    }

    fn large_range_sample(&mut self) -> f64 {
        let mut result = self.internal_sample();
        if self.internal_sample() % 2 == 0 {
            result = -result;
        }
        let mut d = result as f64;
        d += (i32::MAX - 1) as f64;
        d /= 2.0 * i32::MAX as f64 - 1.0;
        d
    }

    /// `Next(min, max)`; `min > max` is an error.
    pub fn next_range(&mut self, min_value: i32, max_value: i32) -> Result<i32, Error> {
        if min_value > max_value {
            return Err(Error::Game("Random.Next: min > max".into()));
        }
        let range = max_value as i64 - min_value as i64;
        if range <= i32::MAX as i64 {
            return Ok(trunc_f64_to_i32(self.sample() * range as f64).wrapping_add(min_value));
        }
        Ok(trunc_f64_to_i64(self.large_range_sample() * range as f64).wrapping_add(min_value as i64) as i32)
    }

    /// `NextDouble()`.
    pub fn next_double(&mut self) -> f64 {
        self.sample()
    }
}

/// Four independent streams derived from one base seed: stream i uses `base ^ (i * 0x9E3779B9)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveRandom {
    pub base_seed: i32,
    streams: [NetRandom; 4],
    /// Values drawn since the streams were seeded.
    draws: u64,
    /// Optional independent nominal lottery and skill path. Native seeded execution never installs one.
    nominal: Option<NominalScript>,
    /// Independent semantic SKILL outcomes for a conditionally deterministic score recording.
    nominal_skill: Option<NominalSkillScript>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NominalSkillScript {
    prefix: Vec<bool>,
    cursor: usize,
    handled_draws: u64,
    branch_rate: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NominalOutcome {
    pub weight: u64,
    pub total: u64,
    pub value: i64,
}

/// A prefix of nontrivial semantic outcomes, not a script of PRNG seeds or raw integer values.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NominalScript {
    prefix: Vec<usize>,
    cursor: usize,
    handled_draws: u64,
    branch: Option<Vec<NominalOutcome>>,
}

impl LiveRandom {
    pub fn new(base_seed: i32) -> LiveRandom {
        LiveRandom {
            base_seed,
            streams: std::array::from_fn(|i| NetRandom::new(Self::derive_sub_seed(base_seed, i as i32))),
            draws: 0,
            nominal: None,
            nominal_skill: None,
        }
    }

    pub fn derive_sub_seed(base_seed: i32, index: i32) -> i32 {
        index.wrapping_mul(-0x61C8_8647) ^ base_seed
    }

    pub fn set_seed(&mut self, base_seed: i32) {
        *self = LiveRandom::new(base_seed);
    }

    /// The number of values drawn from any stream since the streams were seeded.
    pub fn draws(&self) -> u64 {
        self.draws
    }

    pub(crate) fn with_nominal_prefix(prefix: Vec<usize>) -> Self {
        let mut random = Self::new(0);
        random.nominal = Some(NominalScript { prefix, cursor: 0, handled_draws: 0, branch: None });
        random
    }

    pub(crate) fn with_nominal_skill_prefix() -> Self {
        let mut random = Self::new(0);
        random.nominal_skill =
            Some(NominalSkillScript { prefix: Vec::new(), cursor: 0, handled_draws: 0, branch_rate: None });
        random
    }

    pub(crate) fn extend_nominal_skill_prefix(&mut self, prefix: Vec<bool>) -> Result<(), Error> {
        let script = self.nominal_skill.as_mut().ok_or_else(|| Error::Domain("missing nominal skill script".into()))?;
        if script.branch_rate.is_some() || !prefix.starts_with(&script.prefix) {
            return Err(Error::Domain("nominal skill continuation must extend a settled checkpoint".into()));
        }
        script.prefix = prefix;
        Ok(())
    }

    pub(crate) fn nominal_skill_branch(&self) -> Option<ProbabilityMass> {
        self.nominal_skill
            .as_ref()?
            .branch_rate
            .map(|bits| ProbabilityMass::from_f32(f32::from_bits(bits)).expect("nontrivial native probability"))
    }

    pub(crate) fn nominal_skill_covers_draws(&self) -> bool {
        self.nominal_skill.as_ref().is_some_and(|script| script.handled_draws == self.draws)
    }

    pub(crate) fn nominal_skill_prefix_consumed(&self) -> bool {
        self.nominal_skill
            .as_ref()
            .is_some_and(|script| script.cursor == script.prefix.len() && script.branch_rate.is_none())
    }

    /// One nominal event at the original SKILL comparison, including deterministic rates. The event
    /// partition uses the exact binary32 rate, independently conditional on all previous checks.
    pub(crate) fn nominal_skill_probability(&mut self, rate: f32) -> Result<bool, Error> {
        let script = self.nominal_skill.as_mut().ok_or_else(|| Error::Domain("missing nominal skill script".into()))?;
        self.draws = self.draws.checked_add(1).ok_or_else(|| Error::Capacity("skill draw counter overflow".into()))?;
        script.handled_draws =
            script.handled_draws.checked_add(1).ok_or_else(|| Error::Capacity("skill draw counter overflow".into()))?;
        if rate.is_nan() || rate <= 0.0 {
            return Ok(false);
        }
        if rate >= 1.0 {
            return Ok(true);
        }
        let Some(&hit) = script.prefix.get(script.cursor) else {
            script.branch_rate = Some(rate.to_bits());
            return Err(Error::Unsupported("nominal skill recording requires another outcome branch".into()));
        };
        script.cursor += 1;
        Ok(hit)
    }

    /// Extend the selected outcomes while preserving the checkpoint's consumed draws and cursor.
    pub(crate) fn extend_nominal_prefix(&mut self, prefix: Vec<usize>) -> Result<(), Error> {
        let script = self.nominal.as_mut().ok_or_else(|| Error::Domain("missing nominal script".into()))?;
        if script.branch.is_some() || !prefix.starts_with(&script.prefix) {
            return Err(Error::Domain("nominal continuation must extend a settled checkpoint".into()));
        }
        script.prefix = prefix;
        Ok(())
    }

    pub(crate) fn is_nominal(&self) -> bool {
        self.nominal.is_some()
    }

    /// Every raw draw must have passed through an admitted semantic lottery or skill-probability draw.
    /// Unmodelled raw random calls still decline the nominal backend, even if their observed value is unused.
    pub(crate) fn nominal_covers_draws(&self) -> bool {
        self.nominal.as_ref().is_some_and(|script| script.handled_draws == self.draws)
    }

    pub(crate) fn nominal_branch(&self) -> Option<&[NominalOutcome]> {
        self.nominal.as_ref()?.branch.as_deref()
    }

    pub(crate) fn nominal_prefix_consumed(&self) -> bool {
        self.nominal.as_ref().is_some_and(|script| script.cursor == script.prefix.len() && script.branch.is_none())
    }

    /// Draw using exact nominal integer masses. Called at the native draw site, including one-outcome
    /// tables, so the native draw counter still observes one call. Duplicate results are coalesced only
    /// after the caller has established the native table's buff/minimum redistribution and modulus.
    pub(crate) fn nominal_lottery(&mut self, weights: Vec<(u64, u64, i64)>) -> Result<i64, Error> {
        let invalid = |message: &str| Error::Unsupported(format!("nominal LUCK replay: {message}"));
        let script = self.nominal.as_mut().ok_or_else(|| invalid("no path script"))?;
        self.draws = self.draws.checked_add(1).ok_or_else(|| invalid("draw counter overflow"))?;
        script.handled_draws = script.handled_draws.checked_add(1).ok_or_else(|| invalid("draw counter overflow"))?;
        let total = weights.first().map(|v| v.1).filter(|&v| v > 0).ok_or_else(|| invalid("empty lottery"))?;
        let mut sum = 0u64;
        let mut outcomes = Vec::<NominalOutcome>::new();
        for (weight, denominator, value) in weights {
            if denominator != total {
                return Err(invalid("inconsistent lottery modulus"));
            }
            sum = sum.checked_add(weight).ok_or_else(|| invalid("lottery mass overflow"))?;
            if weight == 0 {
                continue;
            }
            if let Some(outcome) = outcomes.iter_mut().find(|outcome| outcome.value == value) {
                outcome.weight = outcome.weight.checked_add(weight).ok_or_else(|| invalid("lottery mass overflow"))?;
            } else {
                outcomes.push(NominalOutcome { weight, total, value });
            }
        }
        if sum != total || outcomes.is_empty() {
            return Err(invalid("lottery does not partition probability one"));
        }
        if outcomes.len() == 1 {
            return Ok(outcomes[0].value);
        }
        let Some(&choice) = script.prefix.get(script.cursor) else {
            script.branch = Some(outcomes);
            return Err(invalid("another outcome branch is required"));
        };
        script.cursor += 1;
        outcomes.get(choice).map(|outcome| outcome.value).ok_or_else(|| invalid("path outcome is out of range"))
    }

    /// Native skill comparison, or its independent nominal Bernoulli law when a nominal path is installed.
    /// The nominal mass is the exact finite, clamped stored binary32 rate, matching the controller DP's
    /// declared model. This does not assert that seeded random floats have a continuous uniform distribution.
    pub(crate) fn skill_probability(&mut self, rate: f32) -> Result<bool, Error> {
        if !self.is_nominal() {
            return Ok(self.value(SKILL) < rate);
        }
        let invalid = || Error::Unsupported("nominal skill probability has no exact bounded integer weights".into());
        if !rate.is_finite() {
            return Err(invalid());
        }
        let rate = rate.clamp(0.0, 1.0);
        if rate == 0.0 || rate == 1.0 {
            // A deterministic semantic draw consumes one native draw but no branch-prefix choice.
            return self.nominal_lottery(vec![(1, 1, i64::from(rate == 1.0))]).map(|value| value != 0);
        }
        let bits = rate.to_bits();
        let exponent = (bits >> 23) & 0xff;
        let mut numerator = u64::from(bits & 0x7fffff);
        let mut shift = if exponent == 0 {
            149
        } else {
            numerator |= 1 << 23;
            150 - exponent
        };
        let common = numerator.trailing_zeros().min(shift);
        numerator >>= common;
        shift -= common;
        let denominator = 1u64.checked_shl(shift).ok_or_else(invalid)?;
        let failure = denominator.checked_sub(numerator).ok_or_else(invalid)?;
        self.nominal_lottery(vec![(failure, denominator, 0), (numerator, denominator, 1)]).map(|value| value != 0)
    }

    /// `Range(type, max)`.
    pub fn range(&mut self, stream: usize, max_value: i32) -> Result<i32, Error> {
        self.draws += 1;
        self.streams[stream].next_max(max_value)
    }

    /// `Range(type, min, max)`.
    pub fn range2(&mut self, stream: usize, min_value: i32, max_value: i32) -> Result<i32, Error> {
        self.draws += 1;
        self.streams[stream].next_range(min_value, max_value)
    }

    /// `Value(type)`: `(float)NextDouble()`.
    pub fn value(&mut self, stream: usize) -> f32 {
        self.draws += 1;
        self.streams[stream].next_double() as f32
    }

    /// `NextInt(type)`: `Next(int.MinValue, int.MaxValue)`.
    pub fn next_int(&mut self, stream: usize) -> i32 {
        self.draws += 1;
        self.streams[stream].next_range(i32::MIN, i32::MAX).expect("valid range")
    }
}

#[cfg(test)]
mod nominal_tests {
    use super::*;

    #[test]
    fn skill_probability_keeps_seeded_values_and_all_stream_states() {
        for seed in [i32::MIN, -971, -1, 0, 1, 529, i32::MAX] {
            let mut actual = LiveRandom::new(seed);
            let mut native = actual.clone();
            for _ in 0..32 {
                for rate in [-1.0, -0.0, 0.0, 0.01, 0.3, 0.5, 1.0, 2.0, f32::NAN, f32::INFINITY] {
                    let expected = native.value(SKILL) < rate;
                    assert_eq!(actual.skill_probability(rate).unwrap(), expected);
                    assert_eq!(actual, native, "every stream and draw counter must remain identical");
                }
            }
        }
    }

    #[test]
    fn nominal_skill_masses_are_exact_binary32_rates() {
        for (rate, numerator, denominator) in [
            (0.5, 1u64, 2u64),
            (0.25, 1, 4),
            (0.75, 3, 4),
            (0.3, 5_033_165, 16_777_216),
            (0.01, 5_368_709, 536_870_912),
            (f32::from_bits(64 << 23), 1, 1u64 << 63),
        ] {
            let mut pending = LiveRandom::with_nominal_prefix(Vec::new());
            assert!(pending.skill_probability(rate).is_err());
            assert_eq!(
                pending.nominal_branch().unwrap(),
                [
                    NominalOutcome { weight: denominator - numerator, total: denominator, value: 0 },
                    NominalOutcome { weight: numerator, total: denominator, value: 1 },
                ]
            );
            assert_eq!(pending.draws(), 1);
            assert!(pending.nominal_covers_draws());
            for (choice, expected) in [(0, false), (1, true)] {
                let mut branch = LiveRandom::with_nominal_prefix(vec![choice]);
                assert_eq!(branch.skill_probability(rate).unwrap(), expected);
                assert!(branch.nominal_prefix_consumed() && branch.nominal_covers_draws());
            }
        }
    }

    #[test]
    fn nominal_deterministic_skill_calls_still_consume_a_draw() {
        let mut random = LiveRandom::with_nominal_prefix(Vec::new());
        for (index, (rate, expected)) in
            [(-3.0, false), (-0.0, false), (0.0, false), (1.0, true), (2.0, true)].into_iter().enumerate()
        {
            assert_eq!(random.skill_probability(rate).unwrap(), expected);
            assert_eq!(random.draws(), index as u64 + 1);
            assert!(random.nominal_prefix_consumed() && random.nominal_covers_draws());
        }
    }

    #[test]
    fn unsupported_nominal_skill_rates_never_supply_a_branch_or_hide_raw_draws() {
        for rate in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MIN_POSITIVE, f32::from_bits(1)] {
            let mut random = LiveRandom::with_nominal_prefix(Vec::new());
            assert!(matches!(random.skill_probability(rate), Err(Error::Unsupported(_))));
            assert!(random.nominal_branch().is_none());
            assert_eq!(random.draws(), 0);
        }
        let mut random = LiveRandom::with_nominal_prefix(vec![1]);
        random.value(SKILL);
        assert!(random.skill_probability(0.5).unwrap());
        assert!(!random.nominal_covers_draws());
    }

    #[test]
    fn skill_and_lottery_prefixes_share_complete_checkpoint_consumption() {
        let mut random = LiveRandom::with_nominal_prefix(vec![1]);
        assert!(random.skill_probability(0.5).unwrap());
        let checkpoint = random.clone();
        random.extend_nominal_prefix(vec![1, 0, 1]).unwrap();
        assert_eq!(random.nominal_lottery(vec![(1, 3, 10), (2, 3, 20)]).unwrap(), 10);
        assert!(random.skill_probability(0.25).unwrap());
        assert!(random.nominal_prefix_consumed() && random.nominal_covers_draws());
        assert_eq!(random.draws(), 3);
        assert_eq!(checkpoint.draws(), 1);
        assert!(checkpoint.nominal_prefix_consumed());
    }

    #[test]
    fn independent_skill_and_lottery_branches_keep_their_joint_mass() {
        let mut total = 0;
        for first in 0..2 {
            for second in 0..2 {
                for lottery in 0..2 {
                    let mut random = LiveRandom::with_nominal_prefix(vec![first, second, lottery]);
                    assert_eq!(random.skill_probability(0.5).unwrap(), first == 1);
                    assert_eq!(random.skill_probability(0.25).unwrap(), second == 1);
                    assert_eq!(
                        random.nominal_lottery(vec![(1, 3, 10), (2, 3, 20)]).unwrap(),
                        if lottery == 0 { 10 } else { 20 }
                    );
                    // Independent manual masses: 1/2, (3 or 1)/4, and (1 or 2)/3.
                    total += [3, 1][second] * [1, 2][lottery];
                    assert_eq!(random.draws(), 3);
                    assert!(random.nominal_prefix_consumed() && random.nominal_covers_draws());
                }
            }
        }
        assert_eq!(total, 24);
    }

    #[test]
    fn skill_events_keep_boundary_draws_and_checkpoint_prefixes() {
        let mut random = LiveRandom::with_nominal_skill_prefix();
        random.extend_nominal_skill_prefix(vec![false]).unwrap();
        assert!(!random.nominal_skill_probability(-0.5).unwrap());
        assert!(!random.nominal_skill_probability(0.0).unwrap());
        assert!(random.nominal_skill_probability(1.0).unwrap());
        assert!(random.nominal_skill_probability(2.0).unwrap());
        assert!(!random.nominal_skill_probability(0.01).unwrap());
        assert!(random.nominal_skill_prefix_consumed());
        assert_eq!(random.draws(), 5);
        let checkpoint = random.clone();
        random.extend_nominal_skill_prefix(vec![false, true]).unwrap();
        assert!(random.nominal_skill_probability(0.25).unwrap());
        assert!(random.nominal_skill_prefix_consumed() && random.nominal_skill_covers_draws());
        assert_eq!(checkpoint.draws(), 5);
        assert!(random.extend_nominal_skill_prefix(vec![true]).is_err());
        assert!(random.nominal_skill_probability(0.75).is_err());
        assert_eq!(random.nominal_skill_branch().unwrap(), ProbabilityMass::from_f32(0.75).unwrap());
        assert!(random.nominal_skill_covers_draws());
        assert!(random.extend_nominal_skill_prefix(vec![false, true, false]).is_err());
    }

    #[test]
    fn skill_events_retain_small_positive_masses_and_refuse_unhandled_draws() {
        let mut random = LiveRandom::with_nominal_skill_prefix();
        let tiny = f32::from_bits(1);
        assert!(random.nominal_skill_probability(tiny).is_err());
        let hit = random.nominal_skill_branch().unwrap();
        assert!(hit.interval().upper() > 0.0);
        assert_eq!(hit.interval().lower(), f64::from(tiny));
        assert!(hit.merge_disjoint(hit.complement()).interval().contains(1.0));
        assert!(random.nominal_skill_covers_draws());
        let mut unhandled = LiveRandom::with_nominal_skill_prefix();
        unhandled.value(SKILL);
        assert!(unhandled.nominal_skill_probability(0.5).is_err());
        assert!(!unhandled.nominal_skill_covers_draws());
        assert!(LiveRandom::new(0).nominal_skill_probability(0.5).is_err());
    }

    #[test]
    fn checkpoint_extension_preserves_consumption_and_unhandled_draws() {
        let table = || vec![(1, 2, 10), (1, 2, 20)];
        let mut random = LiveRandom::with_nominal_prefix(vec![0]);
        assert_eq!(random.nominal_lottery(table()).unwrap(), 10);
        let checkpoint = random.clone();
        random.extend_nominal_prefix(vec![0, 1]).unwrap();
        assert_eq!(random.nominal_lottery(table()).unwrap(), 20);
        assert_eq!(random.draws(), 2);
        assert!(random.nominal_prefix_consumed() && random.nominal_covers_draws());
        assert_eq!(checkpoint.draws(), 1);
        assert!(checkpoint.nominal_prefix_consumed());
        assert!(random.extend_nominal_prefix(vec![1, 1]).is_err());
        assert!(random.nominal_lottery(table()).is_err());
        assert!(random.extend_nominal_prefix(vec![0, 1, 0]).is_err());
        let mut unhandled = checkpoint;
        unhandled.value(SKILL);
        unhandled.extend_nominal_prefix(vec![0, 1]).unwrap();
        assert!(!unhandled.nominal_covers_draws());
    }

    #[test]
    fn semantic_draws_preserve_mass_and_count_even_for_deterministic_tables() {
        let mut random = LiveRandom::with_nominal_prefix(vec![1]);
        assert_eq!(random.nominal_lottery(vec![(2, 5, 7), (3, 5, 7)]).unwrap(), 7);
        assert_eq!(random.nominal_lottery(vec![(1, 3, 0), (2, 3, 3)]).unwrap(), 3);
        assert!(random.nominal_prefix_consumed());
        assert!(random.nominal_covers_draws());
        assert_eq!(random.draws(), 2);
        assert!(random.nominal_lottery(vec![(1, 3, 0), (2, 3, 3)]).is_err());
        assert_eq!(random.nominal_branch().unwrap().iter().map(|v| v.weight).sum::<u64>(), 3);
        assert!(random.nominal_covers_draws(), "the pending native draw is counted on both sides");
        assert_eq!(random.draws(), 3);
    }

    #[test]
    fn an_unhandled_random_source_or_incomplete_mass_never_becomes_a_nominal_law() {
        let mut random = LiveRandom::with_nominal_prefix(Vec::new());
        random.value(SKILL);
        assert!(!random.nominal_covers_draws());
        assert!(random.nominal_lottery(vec![(1, 3, 0), (2, 3, 3)]).is_err());
        assert!(random.nominal_branch().is_some());
        assert!(!random.nominal_covers_draws(), "check before expanding a pending branch too");
        let mut invalid = LiveRandom::with_nominal_prefix(Vec::new());
        assert!(invalid.nominal_lottery(vec![(1, 3, 0), (1, 3, 3)]).is_err());
        assert!(invalid.nominal_branch().is_none(), "missing probability is not normalized away");
    }
}
