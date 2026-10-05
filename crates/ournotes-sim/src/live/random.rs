//! The live's random number streams: a port of the seeded `System.Random` (Knuth's subtractive generator) and the
//! four-stream wrapper the live draws from.

use crate::error::Error;
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
    /// Optional independent nominal LUCK path. Native seeded execution never installs one.
    nominal: Option<NominalScript>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NominalOutcome {
    pub weight: u64,
    pub total: u64,
    pub value: i64,
}

/// A prefix of nontrivial semantic lottery outcomes, not a script of PRNG seeds or raw integer values.
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

    pub(crate) fn is_nominal(&self) -> bool {
        self.nominal.is_some()
    }

    /// Every raw draw must have passed through an admitted semantic LUCK draw. In particular a SKILL
    /// probability draw, even one whose answer happens to be deterministic, declines this narrow backend.
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
