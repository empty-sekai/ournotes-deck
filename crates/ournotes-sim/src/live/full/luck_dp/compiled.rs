//! Owned native recording for exact program identification and later propagation.
use super::*;
use std::mem::size_of;

mod minimum;
mod minimum_basis;
mod minimum_family;
mod miss;
pub use minimum_basis::{CompiledLuckMinimumBasis, LuckMinimumTermResponse};
pub(crate) use minimum_family::{MinimumFamilyAdmission, MinimumFamilyRecorder};

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
    canonical_miss: Option<miss::MissGaugeDomain>,
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
        let mut words = match &self.canonical_miss {
            Some(domain) => domain.identity_words(&self.transcript)?,
            None => self.transcript.key()?,
        };
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
    /// entirely unchanged. An already selected Miss identity quotient also remains unchanged; mixed
    /// quotient requests are refused by the table diagnostics. The old interval endpoints need not
    /// equal the new outward enclosure.
    pub fn canonicalize_start_minimum(mut self) -> Self {
        if self.canonical_minimum.is_none() && self.canonical_miss.is_none() && !self.transcript.collect_moments {
            self.canonical_minimum = Some(minimum::rewrite(&mut self.transcript));
        }
        self
    }

    pub fn operator_contract(&self) -> &'static str {
        if self.canonical_miss.is_some() {
            "canonical-miss-gauge-deltas/1"
        } else if self.canonical_minimum.is_some() {
            "canonical-start-minimum-cdf/1"
        } else {
            "native-ordered-actions/1"
        }
    }

    /// Opt in to an exact identity quotient of separately native-rounded once-per-range Miss gauge
    /// additions. The original native tape still propagates, and the returned curves remain nominal
    /// controller probabilities. Unknown, negative or wrapping gauge domains fail explicitly. The
    /// start-minimum CDF quotient is a separate choice; conditional minimum bases remain compatible.
    pub fn canonicalize_miss_gauge(mut self) -> Result<Self, Error> {
        if self.canonical_minimum.is_some() {
            return Err(Error::Unsupported(
                "LUCK Miss gauge quotient cannot combine with the minimum CDF quotient".into(),
            ));
        }
        if self.canonical_miss.is_none() {
            self.canonical_miss = Some(miss::MissGaugeDomain::admit(&self.transcript)?);
        }
        Ok(self)
    }

    /// Transport a private conditioned-operator marker into the complete identity's action view.
    pub(super) fn identity_action_slot(&self, slot: usize) -> Option<usize> {
        self.transcript.actions.get(slot)?;
        if self.canonical_miss.is_some() { miss::action_slot(&self.transcript, slot) } else { Some(slot) }
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
        if let Some(domain) = &self.canonical_miss {
            bytes = bytes.checked_add(domain.allocated_bytes()?.checked_sub(size_of::<miss::MissGaugeDomain>())?)?;
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
    Ok(CompiledLuckProgram { transcript, virtual_observer: false, canonical_minimum: None, canonical_miss: None })
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
fn validate_virtual_probes<'a>(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    validation: impl IntoIterator<Item = (&'a [Performer], &'a [Option<usize>])>,
) -> Result<ValidatedVirtualProbes, Error> {
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
    Ok(ValidatedVirtualProbes(covered))
}

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
    let validated = validate_virtual_probes(master, skills, notes, params, setup, play, delta_times, validation)?;
    // Record all original writers with the original admission and error path. No actual performer is
    // displaced, no score row is inserted into this recording, and no controller action is bypassed.
    let mut program =
        compile_luck_program(master, skills, notes, &[], params, setup, play, delta_times, deck, None, None, false)?;
    program.transcript.probes = validated.0;
    program.virtual_observer = true;
    Ok(program)
}
