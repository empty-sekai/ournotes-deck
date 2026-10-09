//! Bounded session-local reuse of an admitted non-minimum native recorder.
use super::*;
use std::mem::size_of;

const MAX_FAMILIES: usize = 256;

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckTableMinimumFamilyStats {
    pub family_admission_calls: usize,
    pub native_recordings: usize,
    pub native_batch_recordings: usize,
    pub family_hits: usize,
    pub family_misses: usize,
    pub family_fallbacks: usize,
    pub retained_families: usize,
    pub family_cache_bytes: usize,
    pub family_key_peak_bytes: usize,
    pub original_program_peak_bytes: usize,
}

struct Family {
    key: Vec<Vec<u8>>,
    basis: CompiledLuckTableMinimumBasis,
}

impl Family {
    fn allocated_bytes(&self) -> Option<usize> {
        size_of::<Self>()
            .checked_add(self.key.capacity().checked_mul(size_of::<Vec<u8>>())?)?
            .checked_add(self.key.iter().try_fold(0usize, |bytes, key| bytes.checked_add(key.capacity()))?)?
            .checked_add(self.basis.allocated_bytes()?.checked_sub(size_of::<CompiledLuckTableMinimumBasis>())?)
    }
}

/// An immutable master/chart/play/setup scope for optional minimum-family reuse. Every requested
/// original holder and model still passes native admission. A hit needs complete equality of privately
/// constructed initialized-state keys, and can change only the independently admitted minimum law.
/// Unsupported projections use a fresh full recording. External table data cannot construct this scope.
pub struct LuckTableMinimumFamilySession<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    neutral: Option<(i64, i64)>,
    recorder: full::MinimumFamilyRecorder<'a>,
    max_terms: usize,
    capacity: usize,
    program_capacity: usize,
    families: Vec<Family>,
    current: Option<CompiledLuckTableMinimumBasis>,
    stats: LuckTableMinimumFamilyStats,
}

impl<'a> LuckTableMinimumFamilySession<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        master: &'a Master,
        skills: &'a LuckSkills,
        neutral: Option<(i64, i64)>,
        notes: &'a [LiveNote],
        params: LiveParams,
        setup: &'a GekisouSetup,
        play: &'a LivePlay,
        deltas: &'a [f32],
        max_terms: usize,
        program_capacity: usize,
        capacity: usize,
    ) -> Result<Self, Error> {
        let mut session = Self {
            master,
            skills,
            neutral,
            recorder: full::MinimumFamilyRecorder::new(master, skills, notes, params, setup, play, deltas),
            max_terms: max_terms.min(64),
            capacity: capacity.min(256 << 20),
            program_capacity: program_capacity.min(256 << 20),
            // Stable preallocated slots avoid retained index growth after a cache admission.
            families: Vec::with_capacity(MAX_FAMILIES),
            current: None,
            stats: LuckTableMinimumFamilyStats::default(),
        };
        if session.allocated_bytes().is_none_or(|bytes| bytes > session.capacity) {
            session.recorder.discard_index();
        }
        if session.allocated_bytes().is_none_or(|bytes| bytes > session.capacity) {
            return Err(Error::Capacity("LUCK minimum family context exceeds its allowance".into()));
        }
        Ok(session)
    }

    pub fn stats(&self) -> LuckTableMinimumFamilyStats {
        LuckTableMinimumFamilyStats {
            retained_families: self.families.len(),
            family_cache_bytes: self.allocated_bytes().unwrap_or(usize::MAX),
            ..self.stats
        }
    }

    /// Family ids are stable only inside this exact mutable session. They authorize reuse of an
    /// already compared family/choices identity, never a cross-request or serialized cache hit.
    pub fn basis(
        &mut self,
        entries: &[(LuckSkillKey, usize)],
    ) -> Result<(Option<usize>, &mut CompiledLuckTableMinimumBasis), Error> {
        self.current = None;
        self.stats.family_admission_calls += 1;
        let virtual_observer = (0..DECK).all(|position| entries.iter().any(|(_, held)| *held == position));
        let mut prepared = Vec::new();
        if virtual_observer {
            let (deck, validation) = virtual_probe_inputs(self.master, self.skills, self.neutral, entries)?;
            self.recorder
                .validate_virtual(validation.iter().map(|batch| (batch.deck.as_slice(), batch.probes.as_slice())))?;
            prepared.push(self.recorder.prepare(&deck, None, true)?);
        } else {
            for batch in luck_probe_batches(self.master, self.skills, self.neutral, entries)? {
                prepared.push(self.recorder.prepare(&batch.deck, Some(&batch.probes), false)?);
            }
        }
        // New virtual coverage is an optional retained validation cache. The prepared original
        // batches already own its admitted flags; dropping this memo preserves their authority.
        if self.allocated_bytes().is_none_or(|bytes| bytes > self.capacity) {
            self.recorder.discard_index();
        }
        if self.allocated_bytes().is_none_or(|bytes| bytes > self.capacity) {
            self.recorder.discard_virtual_validation();
        }
        let admissions = prepared.iter_mut().map(|prepared| prepared.take_admission()).collect::<Option<Vec<_>>>();
        if let Some(admissions) = &admissions {
            let key_bytes = admissions
                .iter()
                .try_fold(0usize, |bytes, item| bytes.checked_add(item.key().len()))
                .ok_or_else(|| Error::Capacity("LUCK minimum family key size overflow".into()))?;
            self.stats.family_key_peak_bytes = self.stats.family_key_peak_bytes.max(key_bytes);
            if let Some(index) = self.families.iter().position(|family| {
                family.key.len() == admissions.len()
                    && family.key.iter().zip(admissions).all(|(key, admitted)| key.as_slice() == admitted.key())
            }) {
                let basis = &mut self.families[index].basis;
                for (batch, admission) in basis.batches.iter_mut().zip(admissions) {
                    admission.reweight(batch, self.max_terms)?;
                }
                Self::same_support(basis)?;
                self.stats.family_hits += 1;
                return Ok((Some(index), basis));
            }
            self.stats.family_misses += 1;
        }

        // The first member of each family and every refused optional projection runs the complete
        // original native frame path. A partially recorded program never enters the family cache.
        self.stats.native_recordings += 1;
        let mut batches = Vec::with_capacity(prepared.len());
        for prepared in prepared {
            self.stats.native_batch_recordings += 1;
            batches.push(self.recorder.record(prepared)?);
        }
        let program = CompiledLuckTableProgram { batches };
        let original_bytes = program
            .allocated_bytes()
            .ok_or_else(|| Error::Capacity("LUCK minimum family original program allocation".into()))?;
        self.stats.original_program_peak_bytes = self.stats.original_program_peak_bytes.max(original_bytes);
        if original_bytes > self.program_capacity {
            return Err(Error::Capacity("LUCK minimum family original program exceeds its allowance".into()));
        }
        let mut basis = program.start_minimum_basis(self.max_terms)?;
        self.check_program_bytes(&basis)?;
        if let Some(admissions) = admissions {
            let mut matches = true;
            for (batch, admission) in basis.batches.iter().zip(&admissions) {
                matches &= admission.matches_basis(batch, self.max_terms)?;
            }
            if matches {
                for (batch, admission) in basis.batches.iter_mut().zip(&admissions) {
                    admission.reweight(batch, self.max_terms)?;
                }
                self.check_program_bytes(&basis)?;
                let family =
                    Family { key: admissions.into_iter().map(full::MinimumFamilyAdmission::into_key).collect(), basis };
                let bytes =
                    family.allocated_bytes().ok_or_else(|| Error::Capacity("LUCK minimum family allocation".into()))?;
                let existing = self
                    .allocated_bytes()
                    .ok_or_else(|| Error::Capacity("LUCK minimum family cache allocation".into()))?;
                if self.families.len() < MAX_FAMILIES
                    && existing.checked_add(bytes).is_some_and(|bytes| bytes <= self.capacity)
                {
                    let index = self.families.len();
                    self.families.push(family);
                    debug_assert!(self.allocated_bytes().is_some_and(|bytes| bytes <= self.capacity));
                    return Ok((Some(index), &mut self.families[index].basis));
                } else {
                    self.current = Some(family.basis);
                }
            } else {
                self.current = Some(basis);
            }
        } else {
            self.current = Some(basis);
        }
        self.stats.family_fallbacks += 1;
        Ok((None, self.current.as_mut().expect("complete standalone basis")))
    }

    fn same_support(basis: &CompiledLuckTableMinimumBasis) -> Result<(), Error> {
        let first =
            basis.batches.first().ok_or_else(|| Error::Input("LUCK minimum family has no native batch".into()))?;
        for batch in &basis.batches[1..] {
            if batch.start_count() != first.start_count() || batch.term_count() != first.term_count() {
                return Err(Error::Unsupported("LUCK minimum family probe batches disagree on support".into()));
            }
            for index in 0..first.term_count() {
                if batch.term_choices(index)? != first.term_choices(index)?
                    || batch.term_weight(index)? != first.term_weight(index)?
                {
                    return Err(Error::Unsupported("LUCK minimum family probe batches disagree on weights".into()));
                }
            }
        }
        Ok(())
    }

    fn check_program_bytes(&self, basis: &CompiledLuckTableMinimumBasis) -> Result<(), Error> {
        if basis.allocated_bytes().is_none_or(|bytes| bytes > self.program_capacity) {
            return Err(Error::Capacity("LUCK minimum family basis exceeds its compiled program allowance".into()));
        }
        Ok(())
    }

    /// Actual retained recorder/index/key/basis capacities. The standalone fallback basis is charged
    /// by the separate current-program allowance; original inputs, output JSON and DP work are separate.
    pub fn allocated_bytes(&self) -> Option<usize> {
        let mut bytes = size_of::<Self>()
            .checked_add(self.recorder.allocated_bytes()?.checked_sub(size_of::<full::MinimumFamilyRecorder<'_>>())?)?
            .checked_add(self.families.capacity().checked_mul(size_of::<Family>())?)?;
        for family in &self.families {
            bytes = bytes.checked_add(family.allocated_bytes()?.checked_sub(size_of::<Family>())?)?;
        }
        Some(bytes)
    }
}
