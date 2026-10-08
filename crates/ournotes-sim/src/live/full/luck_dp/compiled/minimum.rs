//! A checked exact quotient of contiguous independent nominal minimum-guarantee actions.
use super::*;

const MAX_EXPONENT: u32 = 126;

/// A reduced nonnegative dyadic, bounded so all aligned integers fit positive i128.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dyadic {
    numerator: u128,
    exponent: u32,
}

impl Dyadic {
    const ZERO: Self = Self { numerator: 0, exponent: 0 };
    const ONE: Self = Self { numerator: 1, exponent: 0 };

    fn reduced(numerator: u128, exponent: u32) -> Option<Self> {
        if numerator == 0 {
            return Some(Self::ZERO);
        }
        let shift = numerator.trailing_zeros().min(exponent);
        let value = Self { numerator: numerator >> shift, exponent: exponent - shift };
        (value.exponent <= MAX_EXPONENT && value.numerator <= 1u128 << value.exponent).then_some(value)
    }

    fn point(mass: ProbabilityMass) -> Option<Self> {
        let interval = mass.interval();
        let value = interval.lower();
        if !interval.is_point() || !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return None;
        }
        if value == 0.0 {
            return Some(Self::ZERO);
        }
        let bits = value.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let (numerator, power) = if exponent == 0 {
            (u128::from(fraction), -1074)
        } else {
            (u128::from(fraction | (1u64 << 52)), exponent - 1023 - 52)
        };
        if power >= 0 {
            Self::reduced(numerator.checked_shl(power as u32)?, 0)
        } else {
            Self::reduced(numerator, (-power) as u32)
        }
    }

    fn complement(self) -> Self {
        Self::reduced((1u128 << self.exponent) - self.numerator, self.exponent).expect("bounded dyadic complement")
    }

    fn multiply(self, other: Self) -> Option<Self> {
        if self == Self::ZERO || other == Self::ZERO {
            return Some(Self::ZERO);
        }
        let exponent = self.exponent.checked_add(other.exponent)?;
        if exponent > MAX_EXPONENT {
            return None;
        }
        Self::reduced(self.numerator.checked_mul(other.numerator)?, exponent)
    }

    fn aligned(self, exponent: u32) -> u128 {
        debug_assert!(exponent >= self.exponent && exponent <= MAX_EXPONENT);
        self.numerator << (exponent - self.exponent)
    }

    /// (upper - lower) / upper; zero upper denotes a category dominated by a later sure guarantee.
    fn conditional(upper: Self, lower: Self) -> Option<ProbabilityMass> {
        if upper == Self::ZERO || upper == lower {
            return Some(ProbabilityMass::ZERO);
        }
        if lower == Self::ZERO {
            return Some(ProbabilityMass::ONE);
        }
        let exponent = upper.exponent.max(lower.exponent);
        let denominator = upper.aligned(exponent);
        let numerator = denominator.checked_sub(lower.aligned(exponent))?;
        let bounds = F64Interval::integer(i128::try_from(numerator).ok()?)
            .divide(F64Interval::integer(i128::try_from(denominator).ok()?))
            .ok()?;
        ProbabilityMass::from_bounds(bounds.lower().max(0.0), bounds.upper().min(1.0)).ok()
    }
}

/// Exact CDF at 0, 1, 2 (at 3 it is one), located in the rewritten action stream. Original source
/// multiplicity is deliberately absent: equal kernels may originate from different numbers of rows.
#[derive(Clone, Debug)]
pub(super) struct MinimumBlock {
    action_start: usize,
    cdf: [Dyadic; 3],
}

impl MinimumBlock {
    pub(super) fn push_words(&self, words: &mut Vec<u64>) {
        words.push(self.action_start as u64);
        for probability in self.cdf {
            words.extend([
                probability.numerator as u64,
                (probability.numerator >> 64) as u64,
                u64::from(probability.exponent),
            ]);
        }
    }
}

fn cdf(actions: &[Action<ProbabilityMass>]) -> Option<[Dyadic; 3]> {
    let mut cdf = [Dyadic::ONE; 3];
    for &action in actions {
        let Action::StartMinimum { result, chance } = action else { return None };
        if !(1..=3).contains(&result) {
            return None;
        }
        let failure = Dyadic::point(chance)?.complement();
        for probability in cdf.iter_mut().take(result as usize) {
            *probability = probability.multiply(failure)?;
        }
    }
    Some(cdf)
}

fn factor(cdf: [Dyadic; 3]) -> Option<Vec<Action<ProbabilityMass>>> {
    let mut actions = Vec::with_capacity(3);
    for result in 1..=3 {
        let upper = if result == 3 { Dyadic::ONE } else { cdf[result] };
        let chance = Dyadic::conditional(upper, cdf[result - 1])?;
        if chance != ProbabilityMass::ZERO {
            actions.push(Action::StartMinimum { result: result as i8, chance });
        }
    }
    Some(actions)
}

/// Keep every non-minimum action as an order barrier and never combine across original frames.
/// Non-singleton chances or checked-integer overflow retain that entire original run verbatim.
/// No extra original-model admission is removed; only the already recorded nominal operator changes.
pub(super) fn rewrite(transcript: &mut Transcript<ProbabilityMass>) -> Vec<MinimumBlock> {
    rewrite_actions(&mut transcript.frames, &mut transcript.actions)
}

fn rewrite_actions(frames: &mut [Frame], actions: &mut Vec<Action<ProbabilityMass>>) -> Vec<MinimumBlock> {
    let original = std::mem::take(actions);
    let mut rewritten = Vec::with_capacity(original.len());
    let mut blocks = Vec::new();
    let mut begin = 0;
    for frame in frames {
        let end = frame.actions;
        let mut index = begin;
        while index < end {
            if !matches!(original[index], Action::StartMinimum { .. }) {
                rewritten.push(original[index]);
                index += 1;
                continue;
            }
            let mut next = index + 1;
            while next < end && matches!(original[next], Action::StartMinimum { .. }) {
                next += 1;
            }
            let converted = cdf(&original[index..next]).and_then(|cdf| factor(cdf).map(|actions| (cdf, actions)));
            if let Some((cdf, canonical)) = converted {
                if !canonical.is_empty() {
                    blocks.push(MinimumBlock { action_start: rewritten.len(), cdf });
                    rewritten.extend(canonical);
                }
            } else {
                rewritten.extend_from_slice(&original[index..next]);
            }
            index = next;
        }
        begin = end;
        frame.actions = rewritten.len();
    }
    debug_assert_eq!(begin, original.len());
    *actions = rewritten;
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimum(result: i8, probability: f32) -> Action<ProbabilityMass> {
        Action::StartMinimum { result, chance: ProbabilityMass::from_f32(probability).unwrap() }
    }

    fn frame(actions: usize) -> Frame {
        Frame {
            time_ms: 0,
            repeat: 1,
            start: None,
            complete: false,
            finish: false,
            gate: true,
            current_luck: true,
            target: 0,
            notes: 0,
            actions,
            pending: 0,
        }
    }

    #[test]
    fn canonical_minimum_exact_real_probability_witness_and_full_branch_enumeration() {
        for result in [1, 2] {
            let pair = [minimum(result, 0.60), minimum(result, 0.50)];
            let single = [minimum(result, 0.80)];
            assert_eq!(cdf(&pair), cdf(&single));
            let failed = cdf(&pair).unwrap()[0];
            assert_eq!(failed, Dyadic { numerator: 3_355_443, exponent: 24 });
            assert_eq!(
                factor(cdf(&pair).unwrap()).unwrap().iter().map(action_words).collect::<Vec<_>>(),
                factor(cdf(&single).unwrap()).unwrap().iter().map(action_words).collect::<Vec<_>>()
            );
        }
        // Independent integer enumeration of every Bernoulli assignment, including an already-held
        // minimum. The canonical CDF must describe each resulting max state, not just its mean.
        let input = [minimum(1, 0.25), minimum(3, 0.50), minimum(2, 0.75)];
        let law = cdf(&input).unwrap();
        for initial in 0..=3 {
            let mut weights = [0u128; 4];
            for mask in 0..8 {
                let mut state = initial;
                let mut weight = 1;
                for (i, (result, numerator)) in [(1, 1u128), (3, 2), (2, 3)].into_iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        state = state.max(result);
                        weight *= numerator;
                    } else {
                        weight *= 4 - numerator;
                    }
                }
                weights[state] += weight;
            }
            for (end, probability) in law.iter().enumerate() {
                let expected: u128 = weights[..=end].iter().sum();
                let actual = if end < initial { Dyadic::ZERO } else { *probability };
                assert_eq!(actual, Dyadic::reduced(expected, 6).unwrap());
            }
        }
        // A sure higher guarantee makes F=0 denominators harmless and removes dominated outcomes.
        let only = factor(cdf(&[minimum(1, 0.5), minimum(3, 1.0)]).unwrap()).unwrap();
        assert_eq!(only.iter().map(action_words).collect::<Vec<_>>(), vec![action_words(&minimum(3, 1.0))]);
    }

    #[test]
    fn canonical_minimum_rational_conversion_is_outward_for_large_exact_integers() {
        let one_third =
            Dyadic::conditional(Dyadic::reduced(3, 2).unwrap(), Dyadic::reduced(1, 1).unwrap()).unwrap().interval();
        assert!(one_third.lower() < 1.0 / 3.0 && one_third.upper() > 1.0 / 3.0);
        let slightly_above_half = Dyadic::reduced((1u128 << 100) + 1, 101).unwrap();
        let result = Dyadic::conditional(Dyadic::ONE, slightly_above_half).unwrap().interval();
        assert!(result.lower() < 0.5 && result.upper() >= 0.5);
        assert_eq!(Dyadic::conditional(Dyadic::ZERO, Dyadic::ZERO), Some(ProbabilityMass::ZERO));
        assert_eq!(Dyadic::conditional(Dyadic::ONE, Dyadic::ZERO), Some(ProbabilityMass::ONE));
    }

    #[test]
    fn canonical_minimum_keeps_barriers_frame_offsets_and_original_fallback() {
        let mut actions = vec![
            minimum(2, 0.6),
            minimum(2, 0.5),
            Action::StartGauge { value: 1, chance: ProbabilityMass::ONE },
            minimum(2, 0.8),
            minimum(2, 0.8),
        ];
        let mut frames = vec![frame(4), frame(5)];
        let blocks = rewrite_actions(&mut frames, &mut actions);
        assert_eq!(frames.iter().map(|frame| frame.actions).collect::<Vec<_>>(), [3, 4]);
        assert_eq!(blocks.iter().map(|block| block.action_start).collect::<Vec<_>>(), [0, 2, 3]);
        assert!(matches!(actions[1], Action::StartGauge { .. }));
        for mut actions in [
            vec![
                minimum(2, 0.5),
                Action::StartMinimum { result: 2, chance: ProbabilityMass::from_bounds(0.4, 0.6).unwrap() },
            ],
            vec![minimum(2, 0.01); 5],
        ] {
            let original: Vec<_> = actions.iter().map(action_words).collect();
            let mut frames = [frame(actions.len())];
            assert!(rewrite_actions(&mut frames, &mut actions).is_empty());
            assert_eq!(actions.iter().map(action_words).collect::<Vec<_>>(), original);
            assert_eq!(frames[0].actions, original.len());
        }
    }
}
