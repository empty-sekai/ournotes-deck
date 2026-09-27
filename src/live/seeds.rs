//! The published seed set of the live objective with Gekisou on.
//!
//! A live draws from four random streams derived from one base seed ([`crate::live::random::LiveRandom`]); only the
//! skill stream (seeded with the base seed `b`) and the luck stream (seeded with `b ^ 0x9E3779B9`) reach the score,
//! and each stream only depends on the absolute value of its seed. The seed set is a fixed sequence: candidate `k`
//! (`k = 0, 1, ...`) is the low 32 bits of output number `k + 1` of SplitMix64 started at [`GEKISOU_SEED_ORIGIN`];
//! a candidate is kept when its pair of effective stream seeds differs from those of every seed kept before it. The
//! published set of size `n` is the first `n` seeds kept from candidate 0 on, so a smaller set is a prefix of a
//! larger one.

use std::collections::HashSet;

/// Start state of the SplitMix64 sequence (the ASCII bytes of `gekisou1`).
pub const GEKISOU_SEED_ORIGIN: u64 = 0x6765_6B69_736F_7531;

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
/// The luck stream's seed is the base seed xor this value.
const LUCK_XOR: i32 = 0x9E37_79B9_u32 as i32;

/// Candidate `index`: the low 32 bits of SplitMix64 output number `index + 1` from [`GEKISOU_SEED_ORIGIN`].
pub fn seed_candidate(index: u64) -> i32 {
    mix(GEKISOU_SEED_ORIGIN.wrapping_add(GAMMA.wrapping_mul(index.wrapping_add(1)))) as u32 as i32
}

/// The SplitMix64 output function of a state.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The effective seeds `(skill, luck)` of a base seed: the absolute values of `b` and `b ^ 0x9E3779B9`, with
/// `i32::MIN` taken as `i32::MAX`.
pub fn seed_key(seed: i32) -> (i32, i32) {
    fn eff(x: i32) -> i32 {
        if x == i32::MIN { i32::MAX } else { x.abs() }
    }
    (eff(seed), eff(seed ^ LUCK_XOR))
}

/// The first `n` candidates from index `start` on whose keys ([`seed_key`]) differ from those kept before them.
pub fn seeds_from(start: u64, n: usize) -> Vec<i32> {
    let mut keys = HashSet::with_capacity(n);
    let mut out = Vec::with_capacity(n);
    let mut index = start;
    while out.len() < n {
        let s = seed_candidate(index);
        if keys.insert(seed_key(s)) {
            out.push(s);
        }
        index = index.wrapping_add(1);
    }
    out
}

/// The published seed set of size `n` (the first `n` seeds kept from candidate 0 on).
pub fn published_seeds(n: usize) -> Vec<i32> {
    seeds_from(0, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_reference_outputs() {
        // SplitMix64 started at 0.
        let out: Vec<u64> = (1..=3u64).map(|k| mix(GAMMA.wrapping_mul(k))).collect();
        assert_eq!(out, [0xE220_A839_7B1D_CDAF, 0x6E78_9E6A_A1B9_65F4, 0x06C4_5D18_8009_454F]);
    }
}
