//! Joint residues of caller-certified additive rewards on the native lottery transition graph.
//!
//! The caller proves the nonnegative integer score decomposition and each chart-time group's four rewards.
//! This module supplies only their joint nominal law. It neither infers independence from marginal curves nor
//! turns an unknown reward into a random choice: an unresolved path stays in one absorbing side state.

use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(in super::super) struct Request {
    pub rewards: BTreeMap<i32, [Option<u8>; 4]>,
    pub modulus: u8,
}

#[derive(Debug)]
pub(in super::super) struct Law {
    /// Disjoint path masses whose complete reward sum has this known residue.
    pub residues: Vec<ProbabilityMass>,
    /// Paths that encountered at least one reward without a singleton proof.
    pub unresolved: ProbabilityMass,
}

#[derive(Debug)]
pub(in super::super) struct Output {
    /// Request order is preserved. A malformed, unconsumed or capacity-refused window has no law.
    pub laws: Vec<Option<Law>>,
    pub peak_states: usize,
    pub transitions: u64,
}

/// Record the supplied native context once, then consume each independent reward window in sequence.
/// `Ok(None)` is cancellation. Unsupported/Capacity errors are optional refusals; other native errors keep
/// their original meaning. A successful law covers every path, including its unresolved part, after the last
/// requested observation. The complete transcript still passes the original propagation admission checks.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn probabilities(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    requests: &[Request],
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<Output>, Error> {
    if cancelled() {
        return Ok(None);
    }
    let prepared = prepare_recording::<ProbabilityMass>(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        deck,
        None,
        ranking,
    )?;
    if cancelled() {
        return Ok(None);
    }
    let Some(transcript) = record_prepared(prepared, notes, play, delta_times, &mut cancelled)? else {
        return Ok(None);
    };
    if cancelled() {
        return Ok(None);
    }
    if let Some(failure) = &transcript.failure {
        return Err(failure.clone());
    }
    let mut output = Output { laws: Vec::new(), peak_states: 0, transitions: 0 };
    output
        .laws
        .try_reserve_exact(requests.len())
        .map_err(|_| Error::Capacity("rank residue result allocation".into()))?;
    for request in requests {
        if cancelled() {
            return Ok(None);
        }
        if request.modulus < 2 || request.rewards.values().flatten().flatten().any(|&reward| reward >= request.modulus)
        {
            output.laws.push(None);
            continue;
        }
        let mut observer = Residues::new(request)?;
        let mut work = LuckDpCacheStats::default();
        let result = propagate_observed_cancellable(&transcript, &mut cancelled, Some(&mut work), &mut observer);
        output.peak_states = output.peak_states.max(work.peak_states);
        output.transitions = output.transitions.saturating_add(work.transitions);
        match result {
            Ok(Some(_)) => output.laws.push(observer.law),
            Ok(None) => return Ok(None),
            Err(Error::Unsupported(_) | Error::Capacity(_)) => output.laws.push(None),
            Err(error) => return Err(error),
        }
    }
    if cancelled() {
        return Ok(None);
    }
    Ok(Some(output))
}

struct Residues<'a> {
    request: &'a Request,
    consumed: usize,
    last_consumed: Option<i32>,
    invalid: bool,
    law: Option<Law>,
}

impl<'a> Residues<'a> {
    fn new(request: &'a Request) -> Result<Self, Error> {
        let law = if request.rewards.is_empty() {
            let mut law = empty_law(request.modulus)?;
            law.residues[0] = ProbabilityMass::ONE;
            Some(law)
        } else {
            None
        };
        Ok(Self { request, consumed: 0, last_consumed: None, invalid: false, law })
    }
}

impl NoteObserver<ProbabilityMass> for Residues<'_> {
    const COLLECT_CURVE: bool = false;

    fn observe(
        &mut self,
        time: i32,
        at_frame: bool,
        dp: &mut Dp<'_, ProbabilityMass>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<bool, Error> {
        let Some(rewards) = self.request.rewards.get(&time) else { return Ok(true) };
        if self.invalid {
            return Ok(true);
        }
        if self.last_consumed.is_some_and(|last| time <= last) {
            // Even an accidental duplicate after the saved last observation invalidates that law.
            self.invalid = true;
            self.law = None;
            return remap(
                dp,
                |mut state| {
                    state.residue = 0;
                    state
                },
                cancelled,
            );
        }
        self.last_consumed = Some(time);
        self.consumed += 1;
        let modulus = self.request.modulus;
        if !remap(
            dp,
            |mut state| {
                if state.residue != modulus {
                    let rush = if at_frame { state.rush } else { state.query_rush };
                    let score = if at_frame { state.score } else { state.score_before };
                    let class = 2 * usize::from(rush) + usize::from(score);
                    state.residue = rewards[class].map_or(modulus, |reward| {
                        ((u16::from(state.residue) + u16::from(reward)) % u16::from(modulus)) as u8
                    });
                }
                state
            },
            cancelled,
        )? {
            return Ok(false);
        }
        if self.consumed == self.request.rewards.len() {
            let mut law = empty_law(modulus)?;
            for (index, (state, &mass)) in dp.dist.iter().enumerate() {
                if index.is_multiple_of(64) && cancelled() {
                    return Ok(false);
                }
                if state.residue == modulus {
                    law.unresolved = law.unresolved.merge_disjoint(mass);
                } else {
                    law.residues[usize::from(state.residue)] =
                        law.residues[usize::from(state.residue)].merge_disjoint(mass);
                }
            }
            self.law = Some(law);
            // The saved law never undergoes future rounding. Residue has no controller feedback, so its
            // labels can now be forgotten while the remaining transcript still completes admission.
            return remap(
                dp,
                |mut state| {
                    state.residue = 0;
                    state
                },
                cancelled,
            );
        }
        Ok(true)
    }
}

fn empty_law(modulus: u8) -> Result<Law, Error> {
    let mut residues = Vec::new();
    residues
        .try_reserve_exact(usize::from(modulus))
        .map_err(|_| Error::Capacity("rank residue bin allocation".into()))?;
    residues.resize(usize::from(modulus), ProbabilityMass::ZERO);
    Ok(Law { residues, unresolved: ProbabilityMass::ZERO })
}

/// A cancellable version of the ordinary deterministic state map. Unknown rewards only relabel a state;
/// they never supply chance weights. All physical transitions still use ProbabilityMass unchanged.
fn remap(
    dp: &mut Dp<'_, ProbabilityMass>,
    mut transform: impl FnMut(State) -> State,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<bool, Error> {
    let mut out = std::mem::take(&mut dp.spare);
    let mut previous = std::mem::take(&mut dp.dist);
    for (index, (state, mass)) in previous.drain().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(false);
        }
        dp.push(&mut out, transform(state), mass)?;
    }
    dp.spare = previous;
    dp.replace(out)?;
    Ok(true)
}
