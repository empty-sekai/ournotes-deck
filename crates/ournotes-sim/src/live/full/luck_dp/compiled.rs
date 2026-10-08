//! Owned native recording for exact program identification and later propagation.
use super::*;
use std::mem::size_of;

/// An immutable, completely recorded native lottery program. Construction runs the original recorder
/// and every admission check, but performs no distribution propagation. Compilation can succeed even
/// when a later propagation reaches a state or capacity the DP refuses. No deserializer or public field
/// can turn external words into this capability; use its complete identity only to compare programs
/// compiled by the same source version. This is not a whole-score or finite-seed expectation proof.
pub struct CompiledLuckProgram {
    transcript: Transcript<ProbabilityMass>,
}

impl std::fmt::Debug for CompiledLuckProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledLuckProgram")
            .field("frames", &self.transcript.frames.len())
            .field("notes", &self.transcript.notes.len())
            .field("actions", &self.transcript.actions.len())
            .finish_non_exhaustive()
    }
}

impl CompiledLuckProgram {
    /// Complete ordered input words consumed by propagation, including original frame geometry,
    /// templates, lottery tables, action probabilities, binary32 factors and shape-index probe flags.
    /// Hashes may index these words, but only complete equality establishes a matching program.
    pub fn identity_words(&self) -> Option<Vec<u64>> {
        self.transcript.key()
    }

    /// Propagate this original recording directly, without another model construction or frame replay.
    /// Each call owns a fresh distribution; this immutable program retains no computed curve.
    pub fn certified(&self) -> Result<LuckDpCertifiedResult, Error> {
        let result = propagate(&self.transcript)?;
        Ok(LuckDpCertifiedResult {
            probe_transitions: result.probe_transitions,
            steps: result.steps,
            probes: result.probes,
            range_moments: result.range_moments,
            peak_states: result.peak_states,
            transitions: result.transitions,
        })
    }

    /// Shape-index flags validated against the designated original holders at compilation.
    pub fn probes(&self) -> &[bool] {
        &self.transcript.probes
    }

    /// Owned struct and vector capacities. This excludes a caller's separately allocated identity,
    /// returned response, temporary propagation distribution and allocator bookkeeping.
    pub fn allocated_bytes(&self) -> Option<usize> {
        let t = &self.transcript;
        let mut bytes = size_of::<Self>();
        for (capacity, width) in [
            (t.templates.capacity(), size_of::<LuckScore>()),
            (t.luck.capacity(), size_of::<bool>()),
            (t.probes.capacity(), size_of::<bool>()),
            (t.frames.capacity(), size_of::<Frame>()),
            (t.notes.capacity(), size_of::<Judged>()),
            (t.hits.capacity(), size_of::<Hit>()),
            (t.actions.capacity(), size_of::<Action<ProbabilityMass>>()),
            (t.pending.capacity(), size_of::<(usize, i32)>()),
        ] {
            bytes = bytes.checked_add(capacity.checked_mul(width)?)?;
        }
        bytes.checked_add(t.machine.owned_allocation_bytes()?)
    }
}

/// Record one exact controller program using the original full admission and native frame path.
/// A failure at any recorded frame returns an error, never a partial identity. The requested optional
/// additive moments are part of the program identity, just as in the original certified entry point.
#[allow(clippy::too_many_arguments)]
pub fn compile_luck_program(
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
    collect_moments: bool,
) -> Result<CompiledLuckProgram, Error> {
    let mut transcript = record::<ProbabilityMass>(
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
        collect_moments,
    )?;
    if let Some(error) = transcript.failure.take() {
        return Err(error);
    }
    Ok(CompiledLuckProgram { transcript })
}
