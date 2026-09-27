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
}

impl LiveRandom {
    pub fn new(base_seed: i32) -> LiveRandom {
        LiveRandom {
            base_seed,
            streams: std::array::from_fn(|i| NetRandom::new(Self::derive_sub_seed(base_seed, i as i32))),
        }
    }

    pub fn derive_sub_seed(base_seed: i32, index: i32) -> i32 {
        index.wrapping_mul(-0x61C8_8647) ^ base_seed
    }

    pub fn set_seed(&mut self, base_seed: i32) {
        *self = LiveRandom::new(base_seed);
    }

    /// `Range(type, max)`.
    pub fn range(&mut self, stream: usize, max_value: i32) -> Result<i32, Error> {
        self.streams[stream].next_max(max_value)
    }

    /// `Range(type, min, max)`.
    pub fn range2(&mut self, stream: usize, min_value: i32, max_value: i32) -> Result<i32, Error> {
        self.streams[stream].next_range(min_value, max_value)
    }

    /// `Value(type)`: `(float)NextDouble()`.
    pub fn value(&mut self, stream: usize) -> f32 {
        self.streams[stream].next_double() as f32
    }

    /// `NextInt(type)`: `Next(int.MinValue, int.MaxValue)`.
    pub fn next_int(&mut self, stream: usize) -> i32 {
        self.streams[stream].next_range(i32::MIN, i32::MAX).expect("valid range")
    }
}
