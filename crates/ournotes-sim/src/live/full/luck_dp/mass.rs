//! Probability masses of the lottery-state propagation: binary64 and certified intervals.

use super::*;

/// One transition graph, with either fast nominal masses or outward-certified masses. In particular,
/// certified arithmetic never turns a small positive upper bound into a pruned branch.
pub(super) trait Mass: Copy {
    type Weights: PartialEq;
    const ZERO: Self;
    const ONE: Self;

    fn from_f32(value: f32) -> Option<Self>;
    fn multiply(self, other: Self) -> Self;
    fn merge(self, other: Self) -> Self;
    fn complement(self) -> Self;
    fn possible(self) -> bool;
    /// Merge into each of `out` the mass at its place in `masses`, if possible, times `probability`.
    fn accumulate(out: &mut [Self], masses: &[Self], probability: Self);
    /// The merged masses of `masses` that are possible; None without one.
    fn total(masses: &[Self]) -> Option<Self>;
    /// The value's bit pattern; equal patterns mean equal values.
    fn bits(self) -> [u64; 2];
    fn interval(self) -> F64Interval;
    fn bonus(machine: &LotteryMachine, kind: usize, buff: i32, minimum: i8) -> Result<Vec<(Self, i64)>, Error>;
    fn base(machine: &LotteryMachine, note_type: i32, judgement: i32) -> Result<Vec<(Self, i64)>, Error>;
    /// The weights of a distribution, given the states' rests with their total masses.
    fn weights<'s>(totals: impl Iterator<Item = (&'s State, Self)>, probes: &[bool], phase: Phase) -> Self::Weights;
}

impl Mass for f64 {
    type Weights = Vec<f32>;
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;

    fn from_f32(value: f32) -> Option<Self> {
        Some(f64::from(value))
    }

    fn multiply(self, other: Self) -> Self {
        self * other
    }

    fn merge(self, other: Self) -> Self {
        self + other
    }

    fn complement(self) -> Self {
        1.0 - self
    }

    fn possible(self) -> bool {
        self > 0.0
    }

    fn accumulate(out: &mut [Self], masses: &[Self], probability: Self) {
        for (out, &mass) in out.iter_mut().zip(masses) {
            if mass > 0.0 {
                *out += mass * probability;
            }
        }
    }

    fn total(masses: &[Self]) -> Option<Self> {
        let mut held = masses.iter().filter(|&&mass| mass > 0.0).peekable();
        held.peek()?;
        Some(held.sum())
    }

    fn bits(self) -> [u64; 2] {
        [self.to_bits(), 0]
    }

    fn interval(self) -> F64Interval {
        F64Interval::point(self).expect("finite nominal mass")
    }

    fn bonus(machine: &LotteryMachine, kind: usize, buff: i32, minimum: i8) -> Result<Vec<(Self, i64)>, Error> {
        machine.bonus_probabilities(kind, buff, i64::from(minimum))
    }

    fn base(machine: &LotteryMachine, note_type: i32, judgement: i32) -> Result<Vec<(Self, i64)>, Error> {
        machine.base_point_probabilities(note_type, judgement)
    }

    fn weights<'s>(totals: impl Iterator<Item = (&'s State, Self)>, probes: &[bool], phase: Phase) -> Self::Weights {
        let mut values = [0f64; 3];
        for (state, probability) in totals {
            let (rush, score) = phase.reads(state);
            if rush {
                values[0] += probability;
            }
            if score {
                values[1] += probability;
                if rush {
                    values[2] += probability;
                }
            }
        }
        let values = values.map(|value| value.clamp(0.0, 1.0) as f32);
        let mut weights = Vec::with_capacity(1 + 2 * probes.len());
        weights.push(values[0]);
        for &enabled in probes {
            weights.extend_from_slice(if enabled { &values[1..] } else { &[0.0, 0.0] });
        }
        weights
    }
}

fn certified_weights(weights: Vec<(u64, u64, i64)>) -> Result<Vec<(ProbabilityMass, i64)>, Error> {
    let mut out: Vec<(ProbabilityMass, i64)> = Vec::new();
    for (weight, total, result) in weights {
        let probability = ProbabilityMass::from_ratio(weight, total)?;
        if let Some(existing) = out.iter_mut().find(|item| item.1 == result) {
            existing.0 = existing.0.merge_disjoint(probability);
        } else {
            out.push((probability, result));
        }
    }
    Ok(out)
}

impl Mass for ProbabilityMass {
    type Weights = [ProbabilityMass; 4];
    const ZERO: Self = ProbabilityMass::ZERO;
    const ONE: Self = ProbabilityMass::ONE;

    fn from_f32(value: f32) -> Option<Self> {
        ProbabilityMass::from_f32(value).ok()
    }

    fn multiply(self, other: Self) -> Self {
        ProbabilityMass::multiply(self, other)
    }

    fn merge(self, other: Self) -> Self {
        self.merge_disjoint(other)
    }

    fn complement(self) -> Self {
        ProbabilityMass::complement(self)
    }

    fn possible(self) -> bool {
        self.interval().upper() > 0.0
    }

    fn accumulate(out: &mut [Self], masses: &[Self], probability: Self) {
        ProbabilityMass::accumulate(out, masses, probability);
    }

    fn total(masses: &[Self]) -> Option<Self> {
        ProbabilityMass::total(masses)
    }

    fn bits(self) -> [u64; 2] {
        [self.interval().lower().to_bits(), self.interval().upper().to_bits()]
    }

    fn interval(self) -> F64Interval {
        ProbabilityMass::interval(self)
    }

    fn bonus(machine: &LotteryMachine, kind: usize, buff: i32, minimum: i8) -> Result<Vec<(Self, i64)>, Error> {
        certified_weights(machine.bonus_probability_weights(kind, buff, i64::from(minimum))?)
    }

    fn base(machine: &LotteryMachine, note_type: i32, judgement: i32) -> Result<Vec<(Self, i64)>, Error> {
        certified_weights(machine.base_point_probability_weights(note_type, judgement)?)
    }

    fn weights<'s>(totals: impl Iterator<Item = (&'s State, Self)>, _probes: &[bool], phase: Phase) -> Self::Weights {
        let mut joint = [Self::ZERO; 4];
        for (state, probability) in totals {
            let (rush, score) = phase.reads(state);
            let index = 2 * usize::from(rush) + usize::from(score);
            joint[index] = joint[index].merge_disjoint(probability);
        }
        joint
    }
}

/// The native nominal table coalesces identical results and rejects values outside 0..=3. Store its
/// existing order, including the original probability values, without allocating on each state draw.
#[derive(Clone, Copy)]
pub(super) struct Draws<M: Mass> {
    pub(super) outcomes: [(M, i64); 4],
    pub(super) len: usize,
}

impl<M: Mass> Draws<M> {
    pub(super) fn one(result: i64) -> Self {
        Self { outcomes: [(M::ONE, result), (M::ZERO, 0), (M::ZERO, 0), (M::ZERO, 0)], len: 1 }
    }

    pub(super) fn of(outcomes: &[(M, i64)]) -> Result<Self, Error> {
        if outcomes.len() > 4 {
            return Err(Error::Capacity("LUCK DP exceeds four distinct lottery results".into()));
        }
        let mut draws = Self { outcomes: [(M::ZERO, 0); 4], len: outcomes.len() };
        draws.outcomes[..draws.len].copy_from_slice(outcomes);
        Ok(draws)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (M, i64)> + '_ {
        self.outcomes[..self.len].iter().copied()
    }
}
