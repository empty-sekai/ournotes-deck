//! The fixed-point stat triple used by every power computation.
//!
//! A [`CardPower`] holds performance, technique and visual as 64-bit "BP" values: one stat point is 10 000 BP, and a
//! percentage uses 10 000 BP = 100 %. Arithmetic wraps at 64 bits.

use crate::num::{floor_to_i32, floor_to_i32_f64};

/// BP per stat point.
pub const BP_UNIT: i64 = 10_000;

/// Performance, technique and visual in BP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CardPower {
    pub performance: i64,
    pub technique: i64,
    pub visual: i64,
}

impl CardPower {
    pub const EMPTY: CardPower = CardPower { performance: 0, technique: 0, visual: 0 };

    /// Raw BP values.
    #[inline]
    pub const fn bp(performance: i64, technique: i64, visual: i64) -> Self {
        CardPower { performance, technique, visual }
    }

    /// The same raw BP value on all three stats.
    #[inline]
    pub const fn bp_single(v: i64) -> Self {
        CardPower { performance: v, technique: v, visual: v }
    }

    /// Whole stat points (`x * 10000` BP each).
    #[inline]
    pub fn points(performance: i64, technique: i64, visual: i64) -> Self {
        CardPower {
            performance: performance.wrapping_mul(BP_UNIT),
            technique: technique.wrapping_mul(BP_UNIT),
            visual: visual.wrapping_mul(BP_UNIT),
        }
    }

    /// The same number of whole points on all three stats.
    #[inline]
    pub fn points_single(v: i64) -> Self {
        let x = v.wrapping_mul(BP_UNIT);
        CardPower { performance: x, technique: x, visual: x }
    }

    /// Component-wise wrapping addition.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, o: CardPower) -> CardPower {
        CardPower {
            performance: self.performance.wrapping_add(o.performance),
            technique: self.technique.wrapping_add(o.technique),
            visual: self.visual.wrapping_add(o.visual),
        }
    }

    /// Component-wise wrapping subtraction.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, o: CardPower) -> CardPower {
        CardPower {
            performance: self.performance.wrapping_sub(o.performance),
            technique: self.technique.wrapping_sub(o.technique),
            visual: self.visual.wrapping_sub(o.visual),
        }
    }

    /// Adds `b` whole points to each stat.
    #[inline]
    pub fn add_points(self, b: i64) -> CardPower {
        self.add(CardPower::points_single(b))
    }

    /// Component-wise `(a * b) / 10000` with a wrapping 64-bit product and truncating division: a stat in BP times a
    /// percentage in BP gives BP.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, o: CardPower) -> CardPower {
        CardPower {
            performance: o.performance.wrapping_mul(self.performance) / BP_UNIT,
            technique: o.technique.wrapping_mul(self.technique) / BP_UNIT,
            visual: o.visual.wrapping_mul(self.visual) / BP_UNIT,
        }
    }

    /// Rounds each stat down to whole points through binary32: `floor((float)bp / 10000f) * 10000`.
    #[inline]
    pub fn to_floor(self) -> CardPower {
        #[inline]
        fn one(bp: i64) -> i64 {
            (floor_to_i32(bp as f32 / BP_UNIT as f32) as i64).wrapping_mul(BP_UNIT)
        }
        CardPower { performance: one(self.performance), technique: one(self.technique), visual: one(self.visual) }
    }

    /// Whole performance points (`floor((double)bp / 10000.0)`).
    #[inline]
    pub fn performance_points(self) -> i32 {
        floor_to_i32_f64(self.performance as f64 / 10000.0)
    }

    /// Whole technique points.
    #[inline]
    pub fn technique_points(self) -> i32 {
        floor_to_i32_f64(self.technique as f64 / 10000.0)
    }

    /// Whole visual points.
    #[inline]
    pub fn visual_points(self) -> i32 {
        floor_to_i32_f64(self.visual as f64 / 10000.0)
    }

    /// The displayed total: technique + performance + visual points (wrapping 32-bit sum).
    #[inline]
    pub fn total(self) -> i32 {
        self.technique_points().wrapping_add(self.performance_points()).wrapping_add(self.visual_points())
    }

    /// Sum of the three BP values.
    #[inline]
    pub fn bp_total(self) -> i64 {
        self.performance.wrapping_add(self.technique).wrapping_add(self.visual)
    }

    /// The three components as an array (performance, technique, visual).
    #[inline]
    pub fn to_array(self) -> [i64; 3] {
        [self.performance, self.technique, self.visual]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_floor_goes_through_binary32() {
        // 99 999 999 BP rounds to 1.0e8 in binary32 before the division.
        assert_eq!(CardPower::bp_single(99_999_999).to_floor(), CardPower::bp_single(100_000_000));
        assert_eq!(CardPower::bp_single(-12_345).to_floor(), CardPower::bp_single(-20_000));
    }

    #[test]
    fn mul_truncates_toward_zero() {
        let a = CardPower::bp(-15_001, 15_001, 1);
        let b = CardPower::bp_single(5_000);
        assert_eq!(a.mul(b), CardPower::bp(-7_500, 7_500, 0));
    }

    #[test]
    fn total_sums_floors() {
        let p = CardPower::bp(10_000, 25_000, -1);
        assert_eq!((p.performance_points(), p.technique_points(), p.visual_points()), (1, 2, -1));
        assert_eq!(p.total(), 2);
    }
}
