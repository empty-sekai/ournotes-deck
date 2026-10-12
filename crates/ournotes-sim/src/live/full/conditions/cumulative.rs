//! Cumulative conditions: the counts that scale an effect.

use super::*;

/// A cumulative condition: the count an effect state carries while it runs (read by Gekisou effects).
#[derive(Clone, Debug)]
pub(crate) enum Cumulative {
    Fixed(i64),
    ElapsedTime {
        period: i32,
        elapsed: i32,
        previous: i32,
        count: i32,
        max: i64,
    },
    /// `floor(Gekisou combo of the playing range / n)`, 0 outside a range (and without Gekisou).
    ComboPerN {
        n: i64,
        values_empty: bool,
        max: i64,
        just: bool,
    },
    /// `floor(judged target notes counted while the effect runs / n)`.
    JudgementPerN {
        n: i64,
        values_empty: bool,
        targets: Vec<i64>,
        max: i64,
        count: i32,
        kind: i64,
    },
    LifePerN {
        init: i64,
        n: i64,
        values_empty: bool,
        max: i64,
        limit: bool,
    },
}

impl Cumulative {
    pub(crate) fn init_count(&self) -> i64 {
        match self {
            Self::LifePerN { init, .. } | Self::Fixed(init) => *init,
            _ => 0,
        }
    }

    pub(crate) fn unit(&self) -> i64 {
        match self {
            Cumulative::ComboPerN { n, .. } | Cumulative::JudgementPerN { n, .. } => *n,
            Cumulative::LifePerN { .. } | Cumulative::Fixed(_) | Cumulative::ElapsedTime { .. } => 0,
        }
    }

    pub(crate) fn max(&self) -> i64 {
        match self {
            Cumulative::ComboPerN { max, .. } | Cumulative::JudgementPerN { max, .. } => *max,
            Cumulative::LifePerN { .. } | Cumulative::Fixed(_) | Cumulative::ElapsedTime { .. } => 0,
        }
    }

    pub(crate) fn update_count(&mut self, ctx: &mut CheckCtx) -> Result<i64, Error> {
        match self {
            Cumulative::Fixed(value) => Ok(*value),
            Cumulative::ElapsedTime { period, elapsed, previous, count, max } => {
                *elapsed = elapsed.wrapping_add(ctx.frame_time.wrapping_sub(*previous));
                *previous = ctx.frame_time;
                if *elapsed >= *period {
                    *elapsed = elapsed.wrapping_sub(*period);
                    *count = count.wrapping_add(1);
                }
                Ok((*count as i64).min(*max))
            }
            Cumulative::ComboPerN { n, values_empty, max, just } => {
                let Some(g) = ctx.gk else { return Ok(0) };
                let idx = g.ctrl.current_playing_index;
                let mut v = 0i64;
                if idx >= 0 {
                    if *values_empty {
                        return Err(no_value());
                    }
                    let state = &g.ctrl.states[idx as usize];
                    let count = if *just { state.just } else { state.combo };
                    v = floor_to_i32(count as f32 / *n as f32) as i64;
                }
                Ok(if v >= *max { *max } else { v })
            }
            Cumulative::LifePerN { n, values_empty, max, limit, .. } => {
                if *values_empty {
                    return Err(no_value());
                }
                let life = if *limit { ctx.life.max_life() } else { ctx.life.get_life_at_ms(ctx.frame_time)? };
                Ok((floor_to_i32(life as f32 / *n as f32) as i64).min(*max))
            }
            Cumulative::JudgementPerN { n, values_empty, targets, max, count, kind } => {
                for &(_, j, _) in ctx.judged {
                    if targets.iter().any(|&target| match *kind {
                        1001 => j as i64 >= target,
                        1002 => j as i64 <= target,
                        _ => j as i64 == target,
                    }) {
                        *count = count.wrapping_add(1);
                    }
                }
                if *values_empty {
                    return Err(no_value());
                }
                Ok((floor_to_i32(*count as f32 / *n as f32) as i64).min(*max))
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Cumulative::JudgementPerN { count, .. } => *count = 0,
            Cumulative::ElapsedTime { elapsed, count, .. } => {
                *elapsed = 0;
                *count = 0;
            }
            _ => {}
        }
    }
}
