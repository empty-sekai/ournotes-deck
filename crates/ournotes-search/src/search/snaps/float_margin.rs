//! Explicit operation-count certificates for native drift and envelope arithmetic.

/// Bound rounding-error feedback instead of assuming a fixed one-percent reserve
/// covers arbitrary command counts. An unavailable certificate disables the cap.
pub(super) fn amplification(weight: f64, unit: f64) -> Option<f64> {
    if !weight.is_finite() || weight < 0.0 {
        return None;
    }
    let alpha = (weight * unit).next_up();
    if alpha >= 1.0 {
        return None;
    }
    let value = (1.0 / (1.0 - alpha).next_down()).next_up().max(1.01);
    value.is_finite().then_some(value)
}

pub(super) fn with_chain(drift: f64, extra: f64) -> Option<f64> {
    if !drift.is_finite() || drift < 0.0 {
        return None;
    }
    let mut factor = (1.0 + drift).next_up();
    for allowance in [2f64.powi(-22), super::CHAIN_EPS, 2f64.powi(-19), extra] {
        factor = (factor * (1.0 + allowance).next_up()).next_up();
    }
    let margin = (factor - 1.0).next_up();
    margin.is_finite().then_some(margin)
}

#[derive(Default)]
pub(super) struct WindowRoundoff {
    windows: u64,
    endpoint_l1: f64,
    invalid: bool,
}

impl WindowRoundoff {
    pub(super) fn add(&mut self, note: f64, judge: [f64; 4], mult: f64) {
        let Some(windows) = self.windows.checked_add(1) else {
            self.invalid = true;
            return;
        };
        self.windows = windows;
        for factor in std::iter::once(note).chain(judge) {
            if !factor.is_finite() || !mult.is_finite() || factor < 0.0 || mult < 0.0 {
                self.invalid = true;
                return;
            }
            let term = ((factor * mult).next_up() * 2.0).next_up();
            self.endpoint_l1 = (self.endpoint_l1 + term).next_up();
        }
    }

    /// At most2W endpoint writes, followed by at most2W nonzero prefix additions,
    /// and one factor multiplication on each input path. gamma_(4W+1)*sum|input|
    /// bounds the combined absolute error across note and all judgement fields.
    /// Adding zero to a finite binary64 accumulator is exact. The endpoint norm
    /// and gamma arithmetic themselves round outward.
    pub(super) fn absolute_error(&self) -> Option<f64> {
        if self.invalid || !self.endpoint_l1.is_finite() {
            return None;
        }
        if self.windows == 0 {
            return Some(0.0);
        }
        let n = (((self.windows as f64).next_up() * 4.0).next_up() + 1.0).next_up();
        let alpha = (n * 2f64.powi(-53)).next_up();
        if alpha >= 1.0 {
            return None;
        }
        let gamma = (alpha / (1.0 - alpha).next_down()).next_up();
        let error = (gamma * self.endpoint_l1).next_up();
        error.is_finite().then_some(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_window_roundoff_covers_cancellation_across_many_pulses() {
        // Exact reference in integer mill units; mixing large and small pulses
        // exercises endpoint cancellation and the later prefix accumulation.
        let mut diff = vec![0.0f64; 8193];
        let mut exact = vec![0i64; diff.len()];
        let mut bound = WindowRoundoff::default();
        for i in 0..4096 {
            let lo = i;
            let hi = i + 4096;
            let mill = if i % 2 == 0 { 1_000_000_000 } else { 1 };
            let factor = mill as f64 / 100000.0;
            bound.add(factor, [0.0; 4], 1.0);
            diff[lo] += factor;
            diff[hi] -= factor;
            exact[lo] += mill;
            exact[hi] -= mill;
        }
        let error = bound.absolute_error().unwrap();
        let (mut actual, mut reference) = (0.0, 0i64);
        for (delta, integer_delta) in diff.into_iter().zip(exact) {
            actual += delta;
            reference += integer_delta;
            assert!((actual - reference as f64 / 100000.0).abs() <= error);
        }
    }

    #[test]
    fn large_native_operation_counts_get_a_larger_amplification_or_no_cap() {
        assert_eq!(amplification(1000.0, 2f64.powi(-24)), Some(1.01));
        assert!(amplification(1_000_000.0, 2f64.powi(-24)).unwrap() > 1.06);
        assert!(amplification(2f64.powi(24), 2f64.powi(-24)).is_none());
        let mut invalid = WindowRoundoff::default();
        invalid.add(f64::INFINITY, [0.0; 4], 1.0);
        assert!(invalid.absolute_error().is_none());
    }

    #[test]
    fn integer_precision_and_saturated_mills_charge_cast_and_division() {
        use ournotes_sim::live::score::ScoreFactorState;
        use ournotes_sim::live::skill::{FactorCommand, apply_factor};
        let u = 2f64.powi(-24);
        for mill in [(1 << 24) - 1, (1 << 24) + 1, i32::MAX] {
            let exact_factor = mill as f64 / 100000.0;
            let reconstructed = (mill as f32 / 100000f32) as f64;
            let representation = 2.0 * u / (1.0 - 2.0 * u) * exact_factor;
            assert!((reconstructed - exact_factor).abs() <= representation);
            let mut state = ScoreFactorState::new(1000);
            apply_factor(&mut state, &FactorCommand { note_mill: mill, ..Default::default() });
            let weight = 3.0 + 2.0; // One executed command, two representation operations.
            let drift = weight * u * (1.0 + exact_factor) * amplification(weight, u).unwrap();
            assert!((state.note_score_up as f64 - (1.0 + exact_factor)).abs() <= drift);
        }
        assert_ne!(i32::MAX as f32 as f64, i32::MAX as f64);
    }
}
