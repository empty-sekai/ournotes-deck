//! Numeric semantics shared by the power and score code.
//!
//! The game's arithmetic: 64-bit and 32-bit integer arithmetic wraps, integer division truncates toward zero, `float`
//! is IEEE binary32 with every operation rounded to binary32, and float-to-integer conversions saturate with the
//! special cases below. Rust's `f32` operations are the same IEEE operations (Rust never contracts `a * b + c` into a
//! fused multiply-add), so the only work here is the conversions.

/// Floor to `i32`: round toward minus infinity, saturating; NaN gives 0 and `+inf` gives `i32::MIN`.
///
/// Equal to `x.floor() as i32` for every other input, without the `floorf` library call that baseline x86-64 makes
/// for `f32::floor`: `as` truncates toward zero (saturating, NaN to 0), and the truncation is one too high exactly when
/// it lies above `x`. Below `2^24` the truncation converts back exactly; from there on every float is an integer, so
/// the truncation equals `x` unless it saturated. A saturated `i32::MIN` is already the floor.
#[inline]
pub fn floor_to_i32(x: f32) -> i32 {
    if x == f32::INFINITY {
        return i32::MIN;
    }
    let t = x as i32;
    if t != i32::MIN && t as f32 > x { t - 1 } else { t }
}

/// Double-precision variant of [`floor_to_i32`] (every `i32` converts to `f64` exactly).
#[inline]
pub fn floor_to_i32_f64(x: f64) -> i32 {
    if x == f64::INFINITY {
        return i32::MIN;
    }
    let t = x as i32;
    if t != i32::MIN && t as f64 > x { t - 1 } else { t }
}

/// Ceiling to `i32`, saturating, with `+inf` and `-inf` giving `i32::MIN` and NaN giving 0.
///
/// Equal to `x.ceil() as i32` for finite inputs, by the argument of [`floor_to_i32`] mirrored: the truncation is one
/// too low exactly when it lies below `x`, and a saturated `i32::MAX` is already the ceiling.
#[inline]
pub fn ceil_to_i32(x: f32) -> i32 {
    if x.is_infinite() {
        return i32::MIN;
    }
    let t = x as i32;
    if t != i32::MAX && (t as f32) < x { t + 1 } else { t }
}

/// Truncate toward zero to `i32`, saturating, NaN gives 0.
#[inline]
pub fn trunc_to_i32(x: f32) -> i32 {
    x as i32
}

/// Truncate a double toward zero to `i32`, saturating, NaN gives 0.
#[inline]
pub fn trunc_f64_to_i32(x: f64) -> i32 {
    x as i32
}

/// Truncate a double toward zero to `i64`, saturating, NaN gives 0.
#[inline]
pub fn trunc_f64_to_i64(x: f64) -> i64 {
    x as i64
}

/// Minimum where a NaN operand is ignored and `-0 < +0`.
#[inline]
pub fn min_ignoring_nan(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return b;
    }
    if b.is_nan() {
        return a;
    }
    if a == b {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// The hasher factory of [`FxHashMap`] and [`FxHashSet`].
pub type FxBuildHasher = std::hash::BuildHasherDefault<FxHasher>;
/// A `HashMap` with [`FxHasher`]; build it with `default()` or `with_capacity_and_hasher`.
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
/// A `HashSet` with [`FxHasher`].
pub type FxHashSet<T> = std::collections::HashSet<T, FxBuildHasher>;

/// A fast deterministic hasher for the crate's internal maps, the multiply-and-rotate scheme of FxHash (rustc-hash
/// 2): each word is added to the state, which is then multiplied by a fixed odd constant; `finish` rotates the state
/// so that its well-mixed high bits select the bucket. The state is 64-bit on every target.
///
/// The keys are the crate's own ids, indexes and small tuples of them, so no protection against chosen keys is needed,
/// and std's SipHash with a per-process random key costs more than the lookups it serves. Iteration order stays
/// unspecified, as with std's randomly keyed default: such a map is only iterated where the order cannot matter (an
/// order-free fold, a set rebuilt from it, a collection sorted afterwards).
#[derive(Clone, Copy, Debug, Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    /// The multiplier of rustc-hash 2.
    const K: u64 = 0xf135_7aea_2e62_a9c5;

    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = self.hash.wrapping_add(word).wrapping_mul(Self::K);
    }
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for c in chunks {
            self.add(u64::from_le_bytes(*c));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.add(i as u64);
        self.add((i >> 64) as u64);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash.rotate_left(26)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_rules() {
        assert_eq!(floor_to_i32(-1.2345), -2);
        assert_eq!(floor_to_i32(f32::NAN), 0);
        assert_eq!(floor_to_i32(f32::INFINITY), i32::MIN);
        assert_eq!(floor_to_i32(f32::NEG_INFINITY), i32::MIN);
        assert_eq!(floor_to_i32(3.0e9), i32::MAX);
        assert_eq!(floor_to_i32(-3.0e9), i32::MIN);
        assert_eq!(floor_to_i32_f64(2147483647.5), i32::MAX);
    }

    #[test]
    fn ceil_rules() {
        assert_eq!(ceil_to_i32(0.1), 1);
        assert_eq!(ceil_to_i32(-0.9), 0);
        assert_eq!(ceil_to_i32(f32::INFINITY), i32::MIN);
        assert_eq!(ceil_to_i32(f32::NEG_INFINITY), i32::MIN);
        assert_eq!(ceil_to_i32(f32::NAN), 0);
    }

    #[test]
    fn min_ignoring_nan_rules() {
        assert_eq!(min_ignoring_nan(f32::NAN, 2.0), 2.0);
        assert_eq!(min_ignoring_nan(2.0, f32::NAN), 2.0);
        assert!(min_ignoring_nan(0.0, -0.0).is_sign_negative());
        assert!(min_ignoring_nan(-0.0, 0.0).is_sign_negative());
        assert_eq!(min_ignoring_nan(1.0, 0.5), 0.5);
    }

    #[test]
    fn fast_floor_and_ceil_match_the_library() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut floats: Vec<f32> = vec![
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.0,
            -1.0,
            1.5,
            -1.5,
            16777216.0,
            -8388607.5,
            2147483520.0,
            -2147483520.0,
            2147483648.0,
            -2147483648.0,
            -2147483904.0,
            3.0e9,
            -3.0e9,
            f32::MAX,
            f32::MIN,
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
            f32::from_bits(1),
            -f32::from_bits(1),
            f32::NAN,
            -f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        for k in -70_000i32..70_000 {
            for d in [-1e-3f32, 0.0, 1e-3, 0.5] {
                floats.push(k as f32 + d);
                floats.push(k as f32 * 65537.0 + d);
            }
        }
        floats.extend((0..400_000).map(|_| f32::from_bits(next() as u32)));
        for &x in &floats {
            let floor = if x == f32::INFINITY { i32::MIN } else { x.floor() as i32 };
            let ceil = if x.is_infinite() { i32::MIN } else { x.ceil() as i32 };
            assert_eq!(floor_to_i32(x), floor, "floor {x:e}");
            assert_eq!(ceil_to_i32(x), ceil, "ceil {x:e}");
            let d = f64::from(x) * 1.000000119;
            let floor_d = if d == f64::INFINITY { i32::MIN } else { d.floor() as i32 };
            assert_eq!(floor_to_i32_f64(d), floor_d, "floor f64 {d:e}");
        }
    }

    #[test]
    fn fx_hash_maps() {
        use std::hash::BuildHasher;
        let b = FxBuildHasher::default();
        assert_eq!(b.hash_one((3i64, 7usize)), b.hash_one((3i64, 7usize)));
        assert_ne!(b.hash_one(1i32), b.hash_one(2i32));
        assert_ne!(b.hash_one([1u8, 2, 3].as_slice()), b.hash_one([1u8, 2].as_slice()));
        let mut m = FxHashMap::default();
        for i in 0..10_000i32 {
            m.insert(i, i * 2);
        }
        assert!((0..10_000).all(|i| m[&i] == i * 2));
        let s: FxHashSet<Vec<u64>> = (0..100u64).map(|i| vec![i; (i % 13 + 1) as usize]).collect();
        assert_eq!(s.len(), 100);
    }
}
