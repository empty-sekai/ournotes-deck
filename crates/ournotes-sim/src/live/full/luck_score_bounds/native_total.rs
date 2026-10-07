//! A complete terminal expectation enclosure from native note kernels and adjacent rank snapshots.
//!
//! Both endpoints enclose the expectation of the same completed native score. Each rank
//! uses the command prefix filed by its own end query, while all kernels share the complete history's roundoff
//! allowance and Combo hulls. Integer supports separately prove that sums and rank bonuses do not wrap.
//! A complete expectation interval supplies no probability law or automatic ranking certificate.

use super::rank_trace::{self, RankQuery};
use super::terminal_kernel::Kernel;
use super::terminal_prefix::TerminalNote;
use super::{BoundsEvent, BoundsTrace, F64Interval, I32Interval, LuckDpCertifiedResult, note_mass, rank_bonus_bounds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decline {
    Cancelled,
    Capacity,
    Plan,
    Readiness,
    Kernel,
    Support,
}

/// Enclosures of the same native random variable's expectation and integer support. The two endpoints of
/// `mean` include native floors, factor history, rank snapshots and the certified nominal joint probabilities.
#[derive(Clone, Copy, Debug)]
pub(super) struct ScoreEnclosure {
    pub(super) mean: F64Interval,
    pub(super) support: I32Interval,
}

impl ScoreEnclosure {
    fn new(mean: F64Interval, lower: i128, upper: i128) -> Result<Self, Decline> {
        let lower = i32::try_from(lower).map_err(|_| Decline::Support)?;
        let upper = i32::try_from(upper).map_err(|_| Decline::Support)?;
        let support = I32Interval::new(lower, upper).map_err(|_| Decline::Support)?;
        let mean = mean.intersect(support.as_real()).ok_or(Decline::Support)?;
        if lower < 0 || !mean.lower().is_finite() || !mean.upper().is_finite() {
            return Err(Decline::Support);
        }
        Ok(Self { mean, support })
    }
}

fn kernel_error(error: super::trace_drift::Decline) -> Decline {
    match error {
        super::trace_drift::Decline::Cancelled => Decline::Cancelled,
        super::trace_drift::Decline::Capacity => Decline::Capacity,
        _ => Decline::Kernel,
    }
}

pub(super) fn build(
    kernel: &Kernel<'_>,
    terminal_rows: &[[I32Interval; 4]],
    probability: &LuckDpCertifiedResult,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<ScoreEnclosure, Decline> {
    #[cfg(feature = "search-diagnostics")]
    let start = std::time::Instant::now();
    let result = build_inner(kernel, terminal_rows, probability, cancelled);
    #[cfg(feature = "search-diagnostics")]
    super::profile::record(super::LuckScoreProfile {
        native_score_builds: u64::from(result.is_ok()),
        native_score_plan_refusals: u64::from(matches!(&result, Err(Decline::Plan))),
        native_score_ready_refusals: u64::from(matches!(&result, Err(Decline::Readiness))),
        native_score_kernel_refusals: u64::from(matches!(&result, Err(Decline::Kernel))),
        native_score_support_refusals: u64::from(matches!(&result, Err(Decline::Support))),
        terminal_capacity_refusals: u64::from(matches!(&result, Err(Decline::Capacity))),
        native_score_ms: start.elapsed().as_secs_f64() * 1e3,
        ..Default::default()
    });
    result
}

fn build_inner(
    kernel: &Kernel<'_>,
    terminal_rows: &[[I32Interval; 4]],
    probability: &LuckDpCertifiedResult,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<ScoreEnclosure, Decline> {
    let plan = rank_trace::compile(kernel.trace, &mut *cancelled)
        .map_err(|error| match error {
            rank_trace::RankTraceDecline::Capacity => Decline::Capacity,
            _ => Decline::Plan,
        })?
        .ok_or(Decline::Cancelled)?;
    let mut total =
        weighted(kernel.trace, &kernel.ingredients.notes, terminal_rows, plan.terminal, probability, cancelled)?;
    for rank in plan.ranks {
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        if rank.percent < 0 {
            return Err(Decline::Support);
        }
        let notes = kernel
            .ingredients
            .query_notes(kernel.trace, rank.end.event, rank.start.to, rank.end.to, &mut *cancelled)
            .map_err(kernel_error)?;
        #[cfg(feature = "search-diagnostics")]
        super::profile::record(super::LuckScoreProfile {
            native_score_rank_windows: 1,
            native_score_rank_notes: notes.len() as u64,
            ..Default::default()
        });
        let rows = kernel.rows(&notes, &mut *cancelled).map_err(kernel_error)?;
        let range = weighted(kernel.trace, &notes, &rows, rank.end, probability, cancelled)?;
        let bonus = rank_enclosure(range, rank.percent)?;
        // Zero-coefficient overwritten ranks were never filed, but their numerical proof was still checked.
        // Pending ranks and all noncancelling earlier fixed scores were already refused by the plan compiler.
        total = add_fixed(total, bonus, rank.final_coefficient)?;
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    Ok(total)
}

/// Linearity of expectation requires no independence between notes, ranks or lottery draws. Readiness is
/// captured at this exact query; a later marker cannot justify an earlier query's chart-time joint masses.
fn weighted(
    trace: &BoundsTrace,
    notes: &[TerminalNote],
    rows: &[[I32Interval; 4]],
    query: RankQuery,
    probability: &LuckDpCertifiedResult,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<ScoreEnclosure, Decline> {
    if notes.len() != rows.len() {
        return Err(Decline::Kernel);
    }
    let mut mean = F64Interval::ZERO;
    let (mut lower, mut upper) = (0i128, 0i128);
    for (index, (note, row)) in notes.iter().zip(rows).enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        let Some(BoundsEvent::Note { frame, note: recorded, .. }) = trace.events.get(note.event) else {
            return Err(Decline::Kernel);
        };
        if note.event >= query.event
            || *frame > query.to as usize
            || recorded.time_ms != note.time_ms
            || row.iter().any(|support| support.lower() < 0)
        {
            return Err(Decline::Kernel);
        }
        if note.time_ms > query.probability_ready {
            return Err(Decline::Readiness);
        }
        lower = lower
            .checked_add(i128::from(row.iter().map(|support| support.lower()).min().ok_or(Decline::Kernel)?))
            .ok_or(Decline::Support)?;
        upper = upper
            .checked_add(i128::from(row.iter().map(|support| support.upper()).max().ok_or(Decline::Kernel)?))
            .ok_or(Decline::Support)?;
        if upper > i128::from(i32::MAX) {
            return Err(Decline::Support);
        }
        let index = probability.steps.partition_point(|(time, _)| *time <= note.time_ms);
        for (mass, &support) in note_mass(probability, index.checked_sub(1)).into_iter().zip(row) {
            mean = mean.add(mass.interval().multiply(support.as_real()));
        }
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    ScoreEnclosure::new(mean, lower, upper)
}

/// Native rank truncates `i128(range_score) * percent / 100`, then casts to i32. On this nonnegative,
/// nonwrapping domain the expected rounding correction lies between zero and the largest possible rank
/// remainder. The shared rank operator subtracts that correction before intersecting the independently
/// truncated integer support. Neither endpoint is obtained by flooring the expectation.
fn rank_enclosure(range: ScoreEnclosure, percent: i64) -> Result<ScoreEnclosure, Decline> {
    if percent < 0 || range.support.lower() < 0 {
        return Err(Decline::Support);
    }
    let (mean, support) = rank_bonus_bounds(range.mean, range.support, percent).map_err(|_| Decline::Support)?;
    ScoreEnclosure::new(mean, i128::from(support.lower()), i128::from(support.upper()))
}

fn add_fixed(total: ScoreEnclosure, bonus: ScoreEnclosure, coefficient: u8) -> Result<ScoreEnclosure, Decline> {
    if coefficient > 2 || total.support.lower() < 0 || bonus.support.lower() < 0 {
        return Err(Decline::Support);
    }
    let lower = i128::from(total.support.lower()) + i128::from(bonus.support.lower()) * i128::from(coefficient);
    let upper = i128::from(total.support.upper()) + i128::from(bonus.support.upper()) * i128::from(coefficient);
    let mean = total.mean.add(bonus.mean.scale_integer(i128::from(coefficient)));
    ScoreEnclosure::new(mean, lower, upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::full::gekisou::solo_rank_bonus;

    fn enclosure(mean: f64, lower: i32, upper: i32) -> ScoreEnclosure {
        ScoreEnclosure::new(F64Interval::point(mean).unwrap(), lower.into(), upper.into()).unwrap()
    }

    #[test]
    fn native_rank_expected_floor_is_bounded_without_flooring_the_mean() {
        let mut factors = [[0; 5]; 3];
        factors[0][0] = 75;
        let actual =
            (f64::from(solo_rank_bonus(0, 0, &factors).1) + f64::from(solo_rank_bonus(0, 2, &factors).1)) / 2.0;
        assert_eq!(actual, 0.5);
        let bound = rank_enclosure(enclosure(1.0, 0, 2), 75).unwrap();
        assert!(bound.mean.contains(actual) && bound.mean.upper() < 1.0);
        assert!(bound.mean.upper().floor() < actual, "flooring the expected product would underbound this native law");
        assert_eq!(bound.support, I32Interval::new(0, 1).unwrap());
    }

    #[test]
    fn native_rank_and_final_sum_refuse_possible_wrap_despite_small_means() {
        assert_eq!(rank_enclosure(enclosure(0.0, 0, i32::MAX), 101).unwrap_err(), Decline::Support);
        assert_eq!(add_fixed(enclosure(1.0, 0, i32::MAX - 1), enclosure(0.0, 0, 1), 2).unwrap_err(), Decline::Support,);
        let boundary = add_fixed(enclosure(1.0, 0, i32::MAX - 2), enclosure(0.0, 0, 1), 2).unwrap();
        assert_eq!(boundary.support.upper(), i32::MAX);
        assert!(boundary.mean.contains(1.0));
        assert_eq!(rank_enclosure(enclosure(1.0, 0, 1), -1).unwrap_err(), Decline::Support);
    }
}
