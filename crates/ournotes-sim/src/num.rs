//! Numeric semantics shared by the power and score code.
//!
//! The game's arithmetic: 64-bit and 32-bit integer arithmetic wraps, integer division truncates toward zero, `float`
//! is IEEE binary32 with every operation rounded to binary32, and float-to-integer conversions saturate with the
//! special cases below. Rust's `f32` operations are the same IEEE operations (Rust never contracts `a * b + c` into a
//! fused multiply-add), so the only work here is the conversions.

/// Floor to `i32`: round toward minus infinity, saturating; NaN gives 0 and `+inf` gives `i32::MIN`.
#[inline]
pub fn floor_to_i32(x: f32) -> i32 {
    if x == f32::INFINITY {
        return i32::MIN;
    }
    // `as` saturates and maps NaN to 0.
    x.floor() as i32
}

/// Double-precision variant of [`floor_to_i32`].
#[inline]
pub fn floor_to_i32_f64(x: f64) -> i32 {
    if x == f64::INFINITY {
        return i32::MIN;
    }
    x.floor() as i32
}

/// Ceiling to `i32`, saturating, with `+inf` and `-inf` giving `i32::MIN` and NaN giving 0.
#[inline]
pub fn ceil_to_i32(x: f32) -> i32 {
    if x.is_infinite() {
        return i32::MIN;
    }
    x.ceil() as i32
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
}
