//! Native-note uppers from a complete deterministic recording and certified factor prefixes.
//!
//! A note's conversion, frozen life and combo inputs are deterministic under the recorder admission. Its last
//! execution can be retained from an earlier query, so the combo enclosure includes every recorded observation.
//! No current-state snapshot, judgement-reach envelope or probability independence assumption replaces that
//! history. A separate rank-plan consumer may reuse these kernels for an optional full-score mean upper.

use super::terminal_prefix::{TerminalIngredients, TerminalNote};
use super::trace_drift::Decline;
use super::{BoundsEvent, BoundsTrace, LiveScoreCalculator, LuckDpCertifiedResult, note_bounds_at_power};
use crate::live::certified::F32Interval;
use crate::num::FxHashMap;

#[derive(Debug)]
pub(super) struct Prepared {
    pub(super) power: i32,
    /// Same-time notes share their componentwise maximum, preserving every possible caller ordering.
    pub(super) caps: Vec<[i32; 4]>,
    /// Complete terminal notes and every native rank bonus passed the independent integer-support checks.
    pub(super) mean_upper: Option<f64>,
}

#[derive(Clone, Copy)]
struct ComboInputs {
    ordinary: F32Interval,
    gekisou: F32Interval,
}

/// Shared inputs for terminal notes and earlier rank queries. The complete Combo hull is built once. Rank
/// prefixes contain only commands filed before their own query; their floating allowance still covers the
/// complete execution history, including every possible earlier retained execution.
pub(super) struct Kernel<'a> {
    calc: &'a LiveScoreCalculator,
    power: i32,
    rush_percent: i32,
    pub(super) trace: &'a BoundsTrace,
    pub(super) ingredients: &'a TerminalIngredients,
    linked: bool,
    combos: FxHashMap<(usize, usize), ComboInputs>,
    #[cfg(feature = "search-diagnostics")]
    observations: u64,
}

impl<'a> Kernel<'a> {
    fn new(
        calc: &'a LiveScoreCalculator,
        power: i32,
        rush_percent: i32,
        trace: &'a BoundsTrace,
        ingredients: &'a TerminalIngredients,
        linked: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, Decline> {
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        if trace.filing_gate.is_none()
            || !trace.has_luck
            || power < 0
            || calc.converted_note_count <= 0
            || rush_percent < 0
            || 100i32.checked_add(rush_percent).is_none()
            || ![
                calc.score_adjustment_factor,
                calc.music_difficulty_factor,
                calc.assist_factor,
                calc.life_onus_factor,
                calc.event_bonus_factor,
            ]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
        {
            return Err(Decline::Magnitude);
        }
        let mut combos = FxHashMap::<(usize, usize), ComboInputs>::default();
        #[cfg(feature = "search-diagnostics")]
        let mut observations = 0u64;
        for (ordinal, event) in trace.events.iter().enumerate() {
            if ordinal.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            if let BoundsEvent::Combo { frame, index, ordinary, gekisou } = event {
                if !ordinary.is_finite() || !gekisou.is_finite() || *ordinary < 0.0 || *gekisou < 0.0 {
                    return Err(Decline::Magnitude);
                }
                let ordinary = F32Interval::point(*ordinary).map_err(|_| Decline::Nonfinite)?;
                let gekisou = F32Interval::point(*gekisou).map_err(|_| Decline::Nonfinite)?;
                if let Some(previous) = combos.get_mut(&(*frame, *index)) {
                    previous.ordinary = previous.ordinary.hull(ordinary);
                    previous.gekisou = previous.gekisou.hull(gekisou);
                } else {
                    combos.try_reserve(1).map_err(|_| Decline::Capacity)?;
                    combos.insert((*frame, *index), ComboInputs { ordinary, gekisou });
                }
                #[cfg(feature = "search-diagnostics")]
                {
                    observations = observations.checked_add(1).ok_or(Decline::CountOverflow)?;
                }
            }
        }
        Ok(Self {
            calc,
            power,
            rush_percent,
            trace,
            ingredients,
            linked,
            combos,
            #[cfg(feature = "search-diagnostics")]
            observations,
        })
    }

    /// Integer uppers for these exact note occurrences, without pooling notes that share chart times.
    /// The caller separately establishes which query's ready probability law may be attached to them.
    pub(super) fn rows(
        &self,
        notes: &[TerminalNote],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Vec<[i32; 4]>, Decline> {
        let mut caps = Vec::new();
        caps.try_reserve_exact(notes.len()).map_err(|_| Decline::Capacity)?;
        for (at, prefix) in notes.iter().enumerate() {
            if at.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            let Some(BoundsEvent::Note { frame, index, note }) = self.trace.events.get(prefix.event) else {
                return Err(Decline::Incomplete);
            };
            if note.time_ms != prefix.time_ms {
                return Err(Decline::Incomplete);
            }
            let combo = self.combos.get(&(*frame, *index)).ok_or(Decline::Incomplete)?;
            let fields = self.ingredients.fields_for(prefix, self.linked)?;
            let (bounds, support) = note_bounds_at_power(
                self.calc,
                self.power,
                note,
                &fields,
                combo.ordinary,
                combo.gekisou,
                self.rush_percent,
            )
            .map_err(|_| Decline::Nonfinite)?;
            // The optional positive comparison refuses signed scores and native overflow/saturation domains.
            // The previous factor/fine caps remain available on every such input.
            if support.lower() < 0 || support.upper() == i32::MAX {
                return Err(Decline::Magnitude);
            }
            let mut row = [0; 4];
            for (value, bucket) in row.iter_mut().zip(bounds.buckets) {
                *value = bucket.ok_or(Decline::Incomplete)?.upper;
            }
            if !self.linked && (row[0] != row[1] || row[2] != row[3]) {
                return Err(Decline::Incomplete);
            }
            if row[..3].iter().any(|value| *value > row[3]) {
                return Err(Decline::Magnitude);
            }
            caps.push(row);
        }
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        Ok(caps)
    }
}

/// The calculator's score constants remain unchanged by recording; only its explicitly supplied initial
/// power and the certified factor prefixes enter these native note kernels. Complete-score construction is
/// optional: a rank-plan, readiness or support refusal preserves every successful per-note certificate.
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    calc: &LiveScoreCalculator,
    power: i32,
    rush_percent: i32,
    trace: &BoundsTrace,
    ingredients: &TerminalIngredients,
    linked: bool,
    times: &[i32],
    probability: &LuckDpCertifiedResult,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Prepared, Decline> {
    if !ingredients.notes.iter().map(|note| note.time_ms).eq(times.iter().copied()) {
        return Err(Decline::Incomplete);
    }
    let kernel = Kernel::new(calc, power, rush_percent, trace, ingredients, linked, &mut cancelled)?;
    let mut caps = kernel.rows(&ingredients.notes, &mut cancelled)?;
    let mean_upper = match super::native_total::build(&kernel, &caps, probability, &mut cancelled) {
        Ok(upper) => Some(upper),
        Err(super::native_total::Decline::Cancelled) => return Err(Decline::Cancelled),
        Err(_) => None,
    };

    // Fine entries preserve the input occurrence order at tied times; native factor prefixes use note IDs.
    // The direct score sum above keeps the original occurrences. Only the public time-only per-note interface
    // pools each exact-time group so different native type, conversion and frozen life cannot be mismatched.
    let mut from = 0;
    while from < times.len() {
        if from.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        let until = from + times[from..].partition_point(|time| *time == times[from]);
        let mut maximum = [0; 4];
        for (index, row) in caps[from..until].iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            for (maximum, value) in maximum.iter_mut().zip(row) {
                *maximum = (*maximum).max(*value);
            }
        }
        caps[from..until].fill(maximum);
        from = until;
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    #[cfg(feature = "search-diagnostics")]
    super::profile::record(super::LuckScoreProfile {
        terminal_kernel_builds: 1,
        terminal_kernel_notes: caps.len() as u64,
        terminal_kernel_combo_observations: kernel.observations,
        ..Default::default()
    });
    Ok(Prepared { power, caps, mean_upper })
}
