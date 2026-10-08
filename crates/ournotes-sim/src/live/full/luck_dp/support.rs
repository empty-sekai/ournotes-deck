//! Positive reachability through the admitted lottery-state transition graph.
use super::*;

/// Complete reachable lottery classes at chart and original skill-boundary times.
/// This contains no path probabilities and does not certify a reachable whole-score maximum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuckDpSupportResult {
    pub rush_filings: Vec<LuckRushFrameSupport>,
    pub probe_transitions: Vec<u8>,
    /// Reachable buckets in [neither, score only, Rush only, Rush and score] order.
    pub steps: Vec<(i32, [bool; 4])>,
    pub probes: Vec<bool>,
    pub peak_states: usize,
    pub transitions: u64,
}

/// Endpoint categories for independent chance products. A positive interior chance admits both
/// outcomes; complementing ordinary reachability booleans would incorrectly discard its miss branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Support {
    Zero,
    Partial,
    One,
}

fn positive_weights(weights: Vec<(u64, u64, i64)>) -> Result<Vec<(Support, i64)>, Error> {
    let mut out: Vec<(Support, i64)> = Vec::new();
    for (weight, total, result) in weights {
        if total == 0 || weight > total {
            return Err(Error::Input("invalid positive lottery weight".into()));
        }
        if weight == 0 {
            continue;
        }
        let support = if weight == total { Support::One } else { Support::Partial };
        if let Some(existing) = out.iter_mut().find(|item| item.1 == result) {
            existing.0 = existing.0.merge(support);
        } else {
            out.push((support, result));
        }
    }
    Ok(out)
}

impl Mass for Support {
    type Weights = [bool; 4];
    const ZERO: Self = Self::Zero;
    const ONE: Self = Self::One;
    fn from_f32(value: f32) -> Option<Self> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return None;
        }
        Some(if value == 0.0 {
            Self::Zero
        } else if value == 1.0 {
            Self::One
        } else {
            Self::Partial
        })
    }
    fn multiply(self, other: Self) -> Self {
        match (self, other) {
            (Self::Zero, _) | (_, Self::Zero) => Self::Zero,
            (Self::One, Self::One) => Self::One,
            _ => Self::Partial,
        }
    }
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Zero, other) => other,
            (this, Self::Zero) => this,
            _ => Self::Partial,
        }
    }
    fn complement(self) -> Self {
        match self {
            Self::Zero => Self::One,
            Self::One => Self::Zero,
            Self::Partial => Self::Partial,
        }
    }
    fn possible(self) -> bool {
        self != Self::Zero
    }
    fn bits(self) -> [u64; 2] {
        [
            match self {
                Self::Zero => 0,
                Self::Partial => 1,
                Self::One => 2,
            },
            0,
        ]
    }
    fn interval(self) -> F64Interval {
        unreachable!("reachability propagation does not accumulate probability moments")
    }
    fn bonus(machine: &LotteryMachine, kind: usize, buff: i32, minimum: i8) -> Result<Vec<(Self, i64)>, Error> {
        positive_weights(machine.bonus_probability_weights(kind, buff, i64::from(minimum))?)
    }
    fn base(machine: &LotteryMachine, note_type: i32, judgement: i32) -> Result<Vec<(Self, i64)>, Error> {
        positive_weights(machine.base_point_probability_weights(note_type, judgement)?)
    }
    fn weights(dist: &Distribution<Self>, _probes: &[bool], at_frame: bool) -> Self::Weights {
        let mut joint = [false; 4];
        for (state, support) in dist {
            if !support.possible() {
                continue;
            }
            let rush = if at_frame { state.rush } else { state.query_rush };
            let score = if at_frame { state.score } else { state.score_before };
            joint[2 * usize::from(rush) + usize::from(score)] = true;
        }
        joint
    }
    fn admission_endpoints(checker: Option<&Checker>) -> Option<(bool, bool)> {
        let value = match checker {
            Some(checker) => chance::<Self>(checker, 0)?,
            None => Self::One,
        };
        Some((value == Self::Zero, value == Self::One))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_support_with_ranking(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LuckDpSupportResult, Error> {
    Ok(luck_rush_dp_support_with_ranking_cancellable(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        ranking,
        &mut || false,
    )?
    .expect("complete support recording"))
}

#[allow(clippy::too_many_arguments)]
pub fn luck_rush_dp_support_with_ranking_cancellable(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<LuckDpSupportResult>, Error> {
    if cancelled() {
        return Ok(None);
    }
    let prepared = prepare_recording::<Support>(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        probes,
        ranking,
        false,
    )?;
    let Some(transcript) = record_prepared(prepared, notes, play, delta_times, cancelled)? else {
        return Ok(None);
    };
    if cancelled() {
        return Ok(None);
    }
    let Some(result) = propagate_cancellable(&transcript, cancelled)? else {
        return Ok(None);
    };
    debug_assert!(result.range_moments.is_empty());
    Ok(Some(LuckDpSupportResult {
        rush_filings: result.rush_filings,
        probe_transitions: result.probe_transitions,
        steps: result.steps,
        probes: result.probes,
        peak_states: result.peak_states,
        transitions: result.transitions,
    }))
}

#[cfg(test)]
mod tests;
