//! An optional whole-score mean upper from native note kernels and adjacent rank snapshots.
//!
//! This is only an exclusion upper. It supplies neither a candidate value nor a probability law. Each rank
//! uses the command prefix filed by its own end query, while all kernels share the complete history's roundoff
//! allowance and Combo hulls. Every integer sum and rank bonus must have nonnegative, nonwrapping support.

use super::rank_trace::{self, RankQuery};
use super::terminal_kernel::Kernel;
use super::terminal_prefix::TerminalNote;
use super::{BoundsEvent, BoundsTrace, F64Interval, LuckDpCertifiedResult, note_mass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decline {
    Cancelled,
    Plan,
    Readiness,
    Kernel,
    Support,
}

/// An upper on the expectation together with an independent nonnegative integer support ceiling.
/// `mean` is not a candidate score and is not a two-sided enclosure of its expectation.
#[derive(Clone, Copy, Debug)]
struct Upper {
    mean: f64,
    support: i32,
}

fn kernel_error(error: super::trace_drift::Decline) -> Decline {
    match error {
        super::trace_drift::Decline::Cancelled => Decline::Cancelled,
        _ => Decline::Kernel,
    }
}

pub(super) fn build(
    kernel: &Kernel<'_>,
    terminal_rows: &[[i32; 4]],
    probability: &LuckDpCertifiedResult,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<f64, Decline> {
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
        native_score_ms: start.elapsed().as_secs_f64() * 1e3,
        ..Default::default()
    });
    result
}

fn build_inner(
    kernel: &Kernel<'_>,
    terminal_rows: &[[i32; 4]],
    probability: &LuckDpCertifiedResult,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<f64, Decline> {
    let plan =
        rank_trace::compile(kernel.trace, &mut *cancelled).map_err(|_| Decline::Plan)?.ok_or(Decline::Cancelled)?;
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
        let bonus = rank_upper(range, rank.percent)?;
        // Zero-coefficient overwritten ranks were never filed, but their numerical proof was still checked.
        // Pending ranks and all noncancelling earlier fixed scores were already refused by the plan compiler.
        total = add_fixed(total, bonus, rank.final_coefficient)?;
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    Ok(total.mean)
}

/// Linearity of expectation requires no independence between notes, ranks or lottery draws. Readiness is
/// captured at this exact query; a later marker cannot justify an earlier query's chart-time joint masses.
fn weighted(
    trace: &BoundsTrace,
    notes: &[TerminalNote],
    rows: &[[i32; 4]],
    query: RankQuery,
    probability: &LuckDpCertifiedResult,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Upper, Decline> {
    if notes.len() != rows.len() {
        return Err(Decline::Kernel);
    }
    let mut mean = F64Interval::ZERO;
    let mut support = 0i128;
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
            || row.iter().any(|&cap| cap < 0)
        {
            return Err(Decline::Kernel);
        }
        if note.time_ms > query.probability_ready {
            return Err(Decline::Readiness);
        }
        support = support.checked_add(i128::from(*row.iter().max().ok_or(Decline::Kernel)?)).ok_or(Decline::Support)?;
        if support > i128::from(i32::MAX) {
            return Err(Decline::Support);
        }
        let index = probability.steps.partition_point(|(time, _)| *time <= note.time_ms);
        for (mass, &cap) in note_mass(probability, index.checked_sub(1)).into_iter().zip(row) {
            mean = mean.add(mass.interval().multiply(F64Interval::integer(i128::from(cap))));
        }
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    if !mean.upper().is_finite() || mean.upper() < 0.0 {
        return Err(Decline::Support);
    }
    let support = i32::try_from(support).map_err(|_| Decline::Support)?;
    Ok(Upper { mean: mean.upper().min(f64::from(support)), support })
}

/// Native rank truncates `i128(range_score) * percent / 100`, then casts to i32. On this nonnegative,
/// nonwrapping domain, the expected truncation is at most the untruncated expected product. Flooring the
/// expectation would be a different, unsound operation. Only the independently proved support is floored.
fn rank_upper(range: Upper, percent: i64) -> Result<Upper, Decline> {
    if percent < 0 || range.support < 0 || !range.mean.is_finite() || range.mean < 0.0 {
        return Err(Decline::Support);
    }
    let support = i128::from(range.support).checked_mul(i128::from(percent)).ok_or(Decline::Support)? / 100;
    let support = i32::try_from(support).map_err(|_| Decline::Support)?;
    let mean = F64Interval::point(range.mean)
        .map_err(|_| Decline::Support)?
        .scale_integer(i128::from(percent))
        .divide(F64Interval::integer(100))
        .map_err(|_| Decline::Support)?
        .upper();
    if !mean.is_finite() || mean < 0.0 {
        return Err(Decline::Support);
    }
    Ok(Upper { mean: mean.min(f64::from(support)), support })
}

fn add_fixed(total: Upper, bonus: Upper, coefficient: u8) -> Result<Upper, Decline> {
    if coefficient > 2 || total.support < 0 || bonus.support < 0 {
        return Err(Decline::Support);
    }
    let support = i128::from(total.support) + i128::from(bonus.support) * i128::from(coefficient);
    let support = i32::try_from(support).map_err(|_| Decline::Support)?;
    let mean = F64Interval::point(total.mean)
        .map_err(|_| Decline::Support)?
        .add(F64Interval::point(bonus.mean).map_err(|_| Decline::Support)?.scale_integer(i128::from(coefficient)))
        .upper();
    if !mean.is_finite() || mean < 0.0 {
        return Err(Decline::Support);
    }
    Ok(Upper { mean: mean.min(f64::from(support)), support })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::full::gekisou::solo_rank_bonus;

    #[test]
    fn native_rank_expected_floor_is_bounded_without_flooring_the_mean() {
        let mut factors = [[0; 5]; 3];
        factors[0][0] = 75;
        let actual =
            (f64::from(solo_rank_bonus(0, 0, &factors).1) + f64::from(solo_rank_bonus(0, 2, &factors).1)) / 2.0;
        assert_eq!(actual, 0.5);
        let bound = rank_upper(Upper { mean: 1.0, support: 2 }, 75).unwrap();
        assert!(bound.mean >= actual && bound.mean < 1.0);
        assert!(bound.mean.floor() < actual, "flooring the expected product would underbound this native law");
        assert_eq!(bound.support, 1);
    }

    #[test]
    fn native_rank_and_final_sum_refuse_possible_wrap_despite_small_means() {
        assert_eq!(rank_upper(Upper { mean: 0.0, support: i32::MAX }, 101).unwrap_err(), Decline::Support);
        assert_eq!(
            add_fixed(Upper { mean: 1.0, support: i32::MAX - 1 }, Upper { mean: 0.0, support: 1 }, 2).unwrap_err(),
            Decline::Support,
        );
        let boundary =
            add_fixed(Upper { mean: 1.0, support: i32::MAX - 2 }, Upper { mean: 0.0, support: 1 }, 2).unwrap();
        assert_eq!(boundary.support, i32::MAX);
        assert!(boundary.mean >= 1.0);
        assert_eq!(rank_upper(Upper { mean: 1.0, support: 1 }, -1).unwrap_err(), Decline::Support);
    }
}
