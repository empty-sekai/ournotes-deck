//! The luck gauge and lot count of a luck range.

use super::*;

/// The luck state of one range.
#[derive(Clone, Debug)]
pub(crate) struct LuckScore {
    pub total_bonus_point: i32,
    pub gauge: i32,
    pub lot_count: i32,
    pub rush_combo: i32,
    /// Lottery results counted: Miss, Hit, Super Hit, Critical.
    pub results: [i32; 4],
    pub(crate) next: i64,
    pub(crate) gauge_max: i64,
    gauge_max_default: i64,
    gauge_max_rush: i64,
}

impl Default for LuckScore {
    fn default() -> LuckScore {
        LuckScore {
            total_bonus_point: 0,
            gauge: 0,
            lot_count: 0,
            rush_combo: 0,
            results: [0; 4],
            next: INVALID,
            gauge_max: 100,
            gauge_max_default: 100,
            gauge_max_rush: 50,
        }
    }
}

impl LuckScore {
    /// Every field as words: equal words mean equal states.
    pub(crate) fn push_words(&self, out: &mut Vec<u64>) {
        let Self {
            total_bonus_point,
            gauge,
            lot_count,
            rush_combo,
            results,
            next,
            gauge_max,
            gauge_max_default,
            gauge_max_rush,
        } = self;
        for value in [*total_bonus_point, *gauge, *lot_count, *rush_combo].into_iter().chain(*results) {
            out.push(u64::from(value as u32));
        }
        out.extend([*next as u64, *gauge_max as u64, *gauge_max_default as u64, *gauge_max_rush as u64]);
    }

    pub(crate) fn add_gauge(&mut self, v: i32) -> Result<(), Error> {
        let g = self.gauge.wrapping_add(v) as i64;
        let m = self.gauge_max;
        self.gauge = if m <= g {
            if m <= 0 {
                return Err(game("luck gauge maximum below 1"));
            }
            self.lot_count = self.lot_count.wrapping_add((g / m) as i32);
            (g % m) as i32
        } else {
            g as i32
        };
        Ok(())
    }

    fn change_max(&mut self, m: i64) {
        if m < 1 || self.gauge_max == m {
            return;
        }
        self.gauge_max = m;
        let g = self.gauge as i64;
        if g <= m {
            return;
        }
        self.lot_count = self.lot_count.wrapping_add((g / m) as i32);
        self.gauge = (g % m) as i32;
    }

    pub(super) fn initialize(&mut self, gauge_max: i64, rush_max: i64) {
        if gauge_max > 0 && rush_max > 0 {
            self.gauge_max_default = gauge_max;
            self.gauge_max_rush = rush_max;
            self.change_max(gauge_max);
        }
    }

    pub(crate) fn current_lot_type(&self) -> usize {
        let rc = self.rush_combo as u32;
        if rc < 4 { LOT_TYPE_BY_RUSH[rc as usize] } else { CHANCE_LOW }
    }

    pub(crate) fn add_score(&mut self, r: i64) -> Result<(), Error> {
        let i = r.wrapping_sub(1);
        if (i as u32) < 3 {
            let p = usize::try_from(i).ok().and_then(|i| BONUS_POINT_BY_RESULT.get(i));
            let p = *p.ok_or_else(|| game("lottery result out of range"))?;
            self.total_bonus_point = self.total_bonus_point.wrapping_add(p);
        }
        if (MISS_R..=CRITICAL).contains(&r) {
            let c = &mut self.results[r as usize];
            *c = c.wrapping_add(1);
        }
        if (r as u32) < 3 {
            self.rush_combo = 0;
            self.change_max(self.gauge_max_default);
        } else if r == CRITICAL {
            self.rush_combo = self.rush_combo.wrapping_add(1);
            self.change_max(self.gauge_max_rush);
        }
        Ok(())
    }
}
