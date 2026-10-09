//! Request-local admission for reweighting minimum-only sources without replaying the chart.
//!
//! Every query still constructs and admits its original native model. A family key can erase only
//! whole minimum-only updaters whose conditions are statically false in the native weighted recorder.
//! Those updaters never file an applier, consume random state, or change another updater. The original
//! ordered minimum Plan is retained separately for its independent nominal distribution. All remaining
//! initialized model fields and non-minimum actions compare exactly inside one immutable context.
//! Each retained row keeps its original native index. Interleaved minimum slots may vary only while
//! those indices remain equal; a suffix of erased minimum rows does not constrain row multiplicity.
use super::*;
use crate::live::full::build_context::BuildContext;

const INDEX_BYTES: usize = 1 << 20;

/// An immutable chart/master/frame scope. This private capability cannot be decoded or moved to a
/// different request. Its optional source index changes lookup work, never native admission.
pub(crate) struct MinimumFamilyRecorder<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    notes: &'a [LiveNote],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    deltas: &'a [f32],
    index: Option<BuildContext<'a>>,
    virtual_probes: Option<ValidatedVirtualProbes>,
}

pub(crate) struct MinimumProgramPreparation {
    prepared: PreparedRecording<ProbabilityMass>,
    virtual_observer: bool,
    admission: Option<MinimumFamilyAdmission>,
}

/// Complete owned equality bytes from one originally admitted initialized model. No public input can
/// construct this key or the associated probability actions. Hashes never authorize a family hit.
pub(crate) struct MinimumFamilyAdmission {
    key: Vec<u8>,
    actions: Vec<Action<ProbabilityMass>>,
}

impl MinimumFamilyAdmission {
    pub(crate) fn key(&self) -> &[u8] {
        &self.key
    }

    pub(crate) fn into_key(self) -> Vec<u8> {
        self.key
    }

    /// The first full recording must agree with the static Plan at every start before it establishes
    /// a reusable family. Later equal keys preserve that frame geometry and all non-minimum inputs.
    pub(crate) fn matches_basis(&self, basis: &CompiledLuckMinimumBasis, max_terms: usize) -> Result<bool, Error> {
        basis.matches_fixed_minimum_actions(&self.actions, max_terms)
    }

    pub(crate) fn reweight(&self, basis: &mut CompiledLuckMinimumBasis, max_terms: usize) -> Result<(), Error> {
        basis.reweight_fixed_minimum_actions(&self.actions, max_terms)
    }
}

impl MinimumProgramPreparation {
    pub(crate) fn take_admission(&mut self) -> Option<MinimumFamilyAdmission> {
        self.admission.take()
    }

    fn record(self, notes: &[LiveNote], play: &LivePlay, deltas: &[f32]) -> Result<CompiledLuckProgram, Error> {
        let mut transcript = record_prepared(self.prepared, notes, play, deltas, &mut || false)?
            .expect("uncancelled complete native recording");
        if let Some(error) = transcript.failure.take() {
            return Err(error);
        }
        Ok(CompiledLuckProgram {
            transcript,
            virtual_observer: self.virtual_observer,
            canonical_minimum: None,
            canonical_miss: None,
        })
    }
}

impl<'a> MinimumFamilyRecorder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        master: &'a Master,
        skills: &'a LuckSkills,
        notes: &'a [LiveNote],
        params: LiveParams,
        setup: &'a GekisouSetup,
        play: &'a LivePlay,
        deltas: &'a [f32],
    ) -> Self {
        Self {
            master,
            skills,
            notes,
            params,
            setup,
            play,
            deltas,
            index: BuildContext::try_bounded(master, INDEX_BYTES),
            virtual_probes: None,
        }
    }

    pub(crate) fn allocated_bytes(&self) -> Option<usize> {
        size_of::<Self>()
            .checked_add(
                self.index
                    .as_ref()
                    .map_or(Some(0), |index| index.bounded_bytes()?.checked_sub(size_of::<BuildContext<'_>>()))?,
            )?
            .checked_add(self.virtual_probes.as_ref().map_or(0, |probes| probes.0.capacity()))
    }

    pub(crate) fn discard_index(&mut self) {
        self.index = None;
    }

    pub(crate) fn discard_virtual_validation(&mut self) {
        self.virtual_probes = None;
    }

    pub(crate) fn validate_virtual<'b>(
        &mut self,
        validation: impl IntoIterator<Item = (&'b [Performer], &'b [Option<usize>])>,
    ) -> Result<(), Error> {
        if self.virtual_probes.is_none() {
            self.virtual_probes = Some(validate_virtual_probes(
                self.master,
                self.skills,
                self.notes,
                self.params,
                self.setup,
                self.play,
                self.deltas,
                validation,
            )?);
        }
        Ok(())
    }

    pub(crate) fn prepare(
        &self,
        deck: &[Performer],
        probes: Option<&[Option<usize>]>,
        virtual_observer: bool,
    ) -> Result<MinimumProgramPreparation, Error> {
        let mut prepared = prepare_recording_with_context::<ProbabilityMass>(
            self.master,
            self.skills,
            self.notes,
            &[],
            self.params,
            self.setup,
            self.play,
            self.deltas,
            deck,
            probes,
            None,
            false,
            self.index.as_ref(),
        )?;
        if virtual_observer {
            let validated = self
                .virtual_probes
                .as_ref()
                .ok_or_else(|| Error::Input("LUCK minimum family has no validated virtual observer".into()))?;
            prepared.plan.probes.clone_from(&validated.0);
        } else if let Some(probes) = probes
            && !prepared.plan.probes.iter().copied().eq(probes.iter().map(Option::is_some))
        {
            return Err(Error::Input("LUCK minimum family lost a designated native probe".into()));
        }
        let admission = admit(&mut prepared, virtual_observer);
        Ok(MinimumProgramPreparation { prepared, virtual_observer, admission })
    }

    pub(crate) fn record(&self, prepared: MinimumProgramPreparation) -> Result<CompiledLuckProgram, Error> {
        prepared.record(self.notes, self.play, self.deltas)
    }
}

/// Exactly the native weighted-recorder evaluation for this closed, immutable subset. Do not use the
/// nominal chance here: even Probability(1) is false in weighted mode. Fixed-true minimum appliers,
/// LIFE predicates and unknown checker variants keep their original complete recording path.
fn weighted_condition(checker: &Checker) -> Option<bool> {
    match checker {
        Checker::Fixed(value) => Some(*value),
        Checker::Probability(value) if value.is_finite() => Some(false),
        Checker::Not(inner) => weighted_condition(inner).map(|value| !value),
        Checker::And { items, .. } => {
            items.iter().try_fold(true, |value, item| weighted_condition(item).map(|next| value && next))
        }
        Checker::Or(items) => {
            items.iter().try_fold(false, |value, item| weighted_condition(item).map(|next| value || next))
        }
        _ => None,
    }
}

fn admit(prepared: &mut PreparedRecording<ProbabilityMass>, virtual_observer: bool) -> Option<MinimumFamilyAdmission> {
    if prepared.collect_moments || prepared.life.is_some() || prepared.life_deck.is_some() {
        return None;
    }
    let model = &mut prepared.model;
    // The reduced native constructor owns every lottery row in a condition updater. Preserve that
    // boundary explicitly: no live effect may refer to a row whose metadata this key omits.
    if model.live.iter().flat_map(|skill| &skill.effects).any(|effect| model.rows[effect.row].effect_type == 11005) {
        return None;
    }
    let mut erased = vec![false; model.rows.len()];
    for skill in &model.cond {
        let effects = skill.updater.effects();
        let minimum = effects.iter().filter(|effect| model.rows[effect.row].effect_type == 11005).count();
        if minimum == 0 {
            continue;
        }
        if minimum != effects.len()
            || effects.iter().any(|effect| {
                effect.condition.as_ref().and_then(weighted_condition) != Some(false)
                    || effect.condition.as_ref().is_some_and(reads_life)
            })
        {
            return None;
        }
        for effect in effects {
            erased[effect.row] = true;
        }
    }
    if model.rows.iter().zip(&erased).any(|(row, &erased)| (row.effect_type == 11005) != erased) {
        return None;
    }
    let mut actions = Vec::new();
    let mut other_actions = Vec::new();
    for (phase, action, checker) in &prepared.plan.actions {
        if checker.as_ref().is_some_and(reads_life) {
            return None;
        }
        match *action {
            Action::StartMinimum { .. } => actions.push(*action),
            _ => other_actions.push((*phase, action, checker)),
        }
    }
    // Keep the complete retained row metadata with its original index in a separate identity view.
    // Erased slots never change a retained row's coordinates. An erased trailing suffix contributes
    // no coordinates, so its count can vary. The remaining model still includes every live/condition
    // reference and other current/future initialized field automatically. No projected model executes.
    let rows = std::mem::take(&mut model.rows);
    let conditions = std::mem::take(&mut model.cond);
    let notes = std::mem::take(&mut model.notes);
    let events = std::mem::take(&mut model.events);
    model.cond.extend(
        conditions.iter().filter(|skill| skill.updater.effects().iter().all(|effect| !erased[effect.row])).cloned(),
    );
    let state = super::super::super::luck_exact::initialized_identity(model);
    let indexed_rows = rows.iter().enumerate().filter(|&(index, _)| !erased[index]).collect::<Vec<_>>();
    let row_state = super::super::super::luck_exact::state_identity(&indexed_rows);
    // Every failure below leaves the original admitted model ready for complete native recording.
    model.events = events;
    model.notes = notes;
    model.cond = conditions;
    model.rows = rows;
    let state = state?;
    let row_state = row_state?;
    let plan =
        super::super::super::luck_exact::state_identity(&(&prepared.plan.probes, other_actions, virtual_observer))?;
    let mut key = Vec::new();
    key.extend_from_slice(b"\0conditional-minimum-initialized-family\0\x02");
    key.extend_from_slice(&(state.len() as u64).to_le_bytes());
    key.extend_from_slice(state.as_bytes());
    key.extend_from_slice(&(row_state.len() as u64).to_le_bytes());
    key.extend_from_slice(row_state.as_bytes());
    key.extend_from_slice(plan.as_bytes());
    Some(MinimumFamilyAdmission { key, actions })
}

#[cfg(test)]
#[path = "minimum_family_tests.rs"]
mod tests;
