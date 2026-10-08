//! Owned native recording for exact program identification and later propagation.
use super::*;
use std::mem::size_of;

mod minimum;
mod minimum_basis;
pub use minimum_basis::{CompiledLuckMinimumBasis, LuckMinimumTermResponse};

/// An immutable, completely recorded native lottery program. Construction runs the original recorder
/// and every admission check, but performs no distribution propagation. Compilation can succeed even
/// when a later propagation reaches a state or capacity the DP refuses. No deserializer or public field
/// can turn external words into this capability; use its complete identity only to compare programs
/// compiled by the same source version. An explicit minimum-operator quotient may then replace its
/// nominal action tape. This is not a whole-score or finite-seed expectation proof.
pub struct CompiledLuckProgram {
    transcript: Transcript<ProbabilityMass>,
    virtual_observer: bool,
    canonical_minimum: Option<Vec<minimum::MinimumBlock>>,
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
    /// Complete ordered inputs consumed by propagation, including frame geometry, templates, lottery
    /// tables, action probabilities, binary32 factors and shape-index probe flags. Canonical mode also
    /// retains exact CDF words: coincident rounded probability bounds alone never authorize equivalence.
    /// Hashes may index these words, but only complete equality establishes a matching program.
    pub fn identity_words(&self) -> Option<Vec<u64>> {
        let mut words = self.transcript.key()?;
        // The observer contract is part of the identity even when the controller transcript and
        // shape coverage happen to be identical. External words still cannot construct a program.
        if let Some(blocks) = &self.canonical_minimum {
            words.extend([0x6d696e6364663031, blocks.len() as u64]);
            for block in blocks {
                block.push_words(&mut words);
            }
        }
        words.extend([0x6f62736572766531, u64::from(self.virtual_observer)]);
        Some(words)
    }

    /// Propagate this recorded program (or its explicit nominal quotient) without another native replay.
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

    /// Opt in to a quotient of contiguous independent nominal minimum-guarantee actions. Exact CDF
    /// identities authorize sharing this operator's probability law, not the finite-seed stream or
    /// whole-score paths. Original recording/admission is complete before this transformation.
    /// Unsupported chance arithmetic keeps its original action run; additive-moment programs remain
    /// entirely unchanged. The old interval endpoints need not equal the new outward enclosure.
    pub fn canonicalize_start_minimum(mut self) -> Self {
        if self.canonical_minimum.is_none() && !self.transcript.collect_moments {
            self.canonical_minimum = Some(minimum::rewrite(&mut self.transcript));
        }
        self
    }

    pub fn operator_contract(&self) -> &'static str {
        if self.canonical_minimum.is_some() { "canonical-start-minimum-cdf/1" } else { "native-ordered-actions/1" }
    }

    /// Describes how the direct 7021 observer's shape coverage was established.
    pub fn observer_contract(&self) -> &'static str {
        if self.virtual_observer { "validated-virtual-direct-7021/1" } else { "native-held-direct-7021/1" }
    }

    /// Shape-index coverage validated against actual holders or the explicit virtual observer contract.
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
        if let Some(blocks) = &self.canonical_minimum {
            bytes = bytes.checked_add(blocks.capacity().checked_mul(size_of::<minimum::MinimumBlock>())?)?;
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
    Ok(CompiledLuckProgram { transcript, virtual_observer: false, canonical_minimum: None })
}

/// Constructible only after every shape has passed the original native probe compiler. This
/// capability stays local to this call: it cannot be reused with another master or probe catalogue.
struct ValidatedVirtualProbes(Vec<bool>);

/// Explicit virtual observation leaves the actual writer deck and native recording unchanged. The
/// separate validation decks establish that every observed score shape is exactly an untimed,
/// sustained, fixed-true direct 7021 reader, with no controller writer or LIFE interpreter. The DP's
/// existing score/score_before bits already implement that reader at the native skill boundary;
/// neither actions nor note lotteries later in that frame alter its observed class.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_luck_program_virtual<'a>(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    validation: impl IntoIterator<Item = (&'a [Performer], &'a [Option<usize>])>,
) -> Result<CompiledLuckProgram, Error> {
    let mut covered = vec![false; skills.shapes.len()];
    for (probe_deck, designated) in validation {
        let prepared = prepare_recording::<ProbabilityMass>(
            master,
            skills,
            notes,
            &[],
            params,
            setup,
            play,
            delta_times,
            probe_deck,
            Some(designated),
            None,
            false,
        )?;
        if prepared.life.is_some() || prepared.model.rows.iter().any(|row| (11000..=11005).contains(&row.effect_type)) {
            return Err(Error::Unsupported("LUCK virtual observer validation contains a controller dependency".into()));
        }
        for (shape, (&held, &position)) in prepared.plan.probes.iter().zip(designated).enumerate() {
            if held != position.is_some() {
                return Err(Error::Input("LUCK virtual observer did not retain its designated probe".into()));
            }
            let Some(member) = position else { continue };
            if covered[shape] {
                return Err(Error::Input("LUCK virtual observer has duplicate shape coverage".into()));
            }
            // `may_hold` is deliberately permissive for general Checker trees. Require actual fixed
            // true here, including nested Not/And/Or, rather than interpreting possible as certain.
            let enabled = prepared.model.cond.iter().filter(|condition| condition.member == member).any(|condition| {
                let source = match condition.skill_type {
                    SKILL_TYPE_GEKISOU => LuckSource::Gekisou,
                    SKILL_TYPE_GEKISOU_SUPPORT => LuckSource::GekisouSupport,
                    _ => return false,
                };
                condition.updater.effects().iter().any(|effect| {
                    skills.rows.get(&(source, prepared.model.rows[effect.row].id)) == Some(&shape)
                        && effect.condition.as_ref().is_none_or(|checker| fixed_condition(checker) == Some(true))
                })
            });
            if !enabled {
                return Err(Error::Unsupported("LUCK virtual observer has no fixed-true score probe".into()));
            }
            covered[shape] = true;
        }
    }
    if covered.iter().any(|held| !held) {
        return Err(Error::Input("LUCK virtual observer is missing a score shape".into()));
    }
    let validated = ValidatedVirtualProbes(covered);
    // Record all original writers with the original admission and error path. No actual performer is
    // displaced, no score row is inserted into this recording, and no controller action is bypassed.
    let mut program =
        compile_luck_program(master, skills, notes, &[], params, setup, play, delta_times, deck, None, None, false)?;
    program.transcript.probes = validated.0;
    program.virtual_observer = true;
    Ok(program)
}
