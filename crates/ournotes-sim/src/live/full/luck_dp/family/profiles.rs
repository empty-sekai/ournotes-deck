//! Whole-domain admission and individually completed writer profiles are separate capabilities.
use super::*;

mod programs;
pub use programs::{LuckFamilyProgram, LuckFamilyProgramKey};

#[derive(Debug)]
pub(super) struct AdmittedFamilyInputs {
    pub(super) base: [Performer; SLOTS],
    pub(super) bindings: LuckFamilyBindings,
}

/// Every supplied physical pair has passed the same admission and complete-cover work guards as `prepare`.
/// No probability law has been computed by this capability. The full physical binding map remains present.
#[derive(Debug)]
pub struct LuckFamilyDomain {
    identity: Arc<()>,
    mapping: Arc<DomainTerminalMapping>,
    admitted: AdmittedFamilyInputs,
    choices: [Vec<LuckFamilyChoice>; SLOTS],
    limits: LuckFamilyLimits,
    bytes: usize,
}

/// Exactly one original writer-owner profile, with all 120 original performance-order labels completed.
/// This cannot be used as a complete `LuckControllerFamily` or as another admitted domain's profile.
#[derive(Debug)]
pub struct LuckFamilyProfile {
    identity: Arc<()>,
    mapping: Arc<DomainTerminalMapping>,
    profile: usize,
    orders: Vec<LuckFamilyOrderLaw>,
    bytes: usize,
}

impl LuckFamilyDomain {
    pub fn profile_count(&self) -> usize {
        self.admitted.bindings.profile_count()
    }

    pub fn profile_for(&self, resources: &[Option<usize>; SLOTS]) -> Option<usize> {
        self.admitted.bindings.profile_for(resources)
    }

    pub fn bindings(&self) -> Option<LuckFamilyBindings> {
        self.admitted.bindings.copy()
    }

    pub fn note_times(&self) -> &[i32] {
        &self.mapping.times
    }

    pub fn probe_gate(&self) -> Option<i64> {
        Some(M_LUCK)
    }

    pub fn owns_profile(&self, profile: &LuckFamilyProfile) -> bool {
        Arc::ptr_eq(&self.identity, &profile.identity)
            && Arc::ptr_eq(&self.mapping, &profile.mapping)
            && profile.profile < self.profile_count()
    }

    /// Owned input containers, their full performer vectors, and the retained common mapping.
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    fn allocated_bytes(&self) -> Option<usize> {
        let mut bytes = size_of::<Self>()
            .checked_add(2 * size_of::<usize>())?
            .checked_add(mapping_bytes(&self.mapping)?)?
            .checked_add(self.admitted.bindings.retained_bytes().checked_sub(size_of::<LuckFamilyBindings>())?)?;
        for performer in &self.admitted.base {
            bytes = bytes.checked_add(performer_payload(performer)?)?;
        }
        for choices in &self.choices {
            bytes = bytes.checked_add(choices.capacity().checked_mul(size_of::<LuckFamilyChoice>())?)?;
            for choice in choices {
                bytes = bytes.checked_add(performer_payload(&choice.performer)?)?;
            }
        }
        Some(bytes)
    }
}

impl LuckFamilyProfile {
    pub fn profile(&self) -> usize {
        self.profile
    }

    pub fn orders(&self) -> &[LuckFamilyOrderLaw] {
        &self.orders
    }

    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
}

impl LuckFamilyBindings {
    pub(super) fn copy(&self) -> Option<Self> {
        fn copied<T: Copy>(source: &[T]) -> Option<Vec<T>> {
            let mut values = Vec::new();
            values.try_reserve_exact(source.len()).ok()?;
            values.extend_from_slice(source);
            Some(values)
        }
        let mut allowed = std::array::from_fn(|_| Vec::new());
        for (target, source) in allowed.iter_mut().zip(&self.allowed) {
            *target = copied(source)?;
        }
        let bindings = Self { allowed, writers: copied(&self.writers)?, profiles: copied(&self.profiles)? };
        (bindings.retained_bytes() != usize::MAX).then_some(bindings)
    }
}

fn mapping_bytes(mapping: &DomainTerminalMapping) -> Option<usize> {
    (2 * size_of::<usize>() + size_of::<DomainTerminalMapping>())
        .checked_add(mapping.times.capacity().checked_mul(size_of::<i32>())?)?
        .checked_add(mapping.inputs.capacity().checked_mul(size_of::<InputNote>())?)
}

fn performer_payload(p: &Performer) -> Option<usize> {
    p.support_skills
        .capacity()
        .checked_add(p.gekisou_support_skills.capacity())?
        .checked_mul(size_of::<(i64, i64)>())?
        .checked_add(
            p.tag_ids
                .capacity()
                .checked_add(p.live_skill_categories.capacity())?
                .checked_add(p.gekisou_skill_categories.capacity())?
                .checked_mul(size_of::<i64>())?,
        )
}

impl LuckFamilyContext<'_> {
    /// Admit the complete allowed pair domain, without calculating any writer profile. The original complete
    /// 31-profile/120-order budget checks remain in force even if the caller later asks for only one profile.
    pub fn admit_domain(
        &self,
        choices: &[Vec<LuckFamilyChoice>; SLOTS],
        limits: LuckFamilyLimits,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckFamilyDomain>, LuckFamilyError> {
        let Some(admitted) = self.admit_inputs(choices, limits, &mut cancelled)? else {
            return Ok(None);
        };
        let mut copied = std::array::from_fn(|_| Vec::new());
        for (target, source) in copied.iter_mut().zip(choices) {
            if cancelled() {
                return Ok(None);
            }
            *target = reserve(source.len())?;
            target.extend_from_slice(source);
        }
        let mut domain = LuckFamilyDomain {
            identity: Arc::new(()),
            mapping: Arc::clone(&self.mapping),
            admitted,
            choices: copied,
            limits,
            bytes: 0,
        };
        domain.bytes =
            domain.allocated_bytes().ok_or_else(|| fail(LuckFamilyDecline::Capacity, "domain payload overflow"))?;
        if domain.bytes > limits.max_retained_bytes {
            return Err(fail(LuckFamilyDecline::Capacity, "admitted domain exceeds the byte budget"));
        }
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(domain))
    }

    /// Complete one exact admitted writer-owner profile. Cancellation, refusal or missing labels never publish
    /// a partial profile. Another context's domain cannot authorize replay against these immutable inputs.
    pub fn prepare_profile(
        &self,
        domain: &LuckFamilyDomain,
        profile: usize,
        curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckFamilyProfile>, LuckFamilyError> {
        if cancelled() {
            return Ok(None);
        }
        if !Arc::ptr_eq(&self.mapping, &domain.mapping) {
            return Err(fail(LuckFamilyDecline::Context, "profile domain belongs to another immutable context"));
        }
        let resources =
            domain.admitted.bindings.profiles.get(profile).ok_or_else(|| {
                fail(LuckFamilyDecline::IncompleteCoverage, "writer profile outside the admitted domain")
            })?;
        let mut physical = domain.admitted.base.clone();
        for (owner, resource) in resources.iter().copied().enumerate() {
            if let Some(resource) = resource {
                physical[owner] = domain.choices[owner]
                    .iter()
                    .find(|choice| choice.resource == Some(resource))
                    .expect("admitted writer profile uses an allowed resource")
                    .performer
                    .clone();
            }
        }
        let physical = physical.map(|performer| projected(self.master, &performer));
        let mut temporary = LuckDpCache::new(0);
        let curves = curves.unwrap_or(&mut temporary);
        let recording_capacity = curves.recording_capacity();
        let mut recordings = RecordingCache::default();
        recordings.limit(recording_capacity);
        let input_keys =
            (recording_capacity > 0).then(|| input_reuse::InputKeys::new(self.master, &physical)).flatten();
        let mut completed = LuckFamilyProfile {
            identity: Arc::clone(&domain.identity),
            mapping: Arc::clone(&self.mapping),
            profile,
            orders: reserve(ORDERS)?,
            bytes: 0,
        };
        completed.bytes = size_of::<LuckFamilyProfile>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(mapping_bytes(&self.mapping)?))
            .and_then(|bytes| {
                bytes.checked_add(completed.orders.capacity().checked_mul(size_of::<LuckFamilyOrderLaw>())?)
            })
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "profile container overflow"))?;
        if completed.bytes > domain.limits.max_retained_bytes {
            return Err(fail(LuckFamilyDecline::Capacity, "profile containers exceed the byte budget"));
        }
        let mut coverage = FamilyCoverage::new(1)?;
        let mut order = [0, 1, 2, 3, 4];
        for ordinal in 0..ORDERS {
            if cancelled() {
                return Ok(None);
            }
            let input_key = input_keys.as_ref().and_then(|keys| keys.key(&order, recording_capacity));
            let cached = input_key.as_deref().and_then(|key| {
                curves.stats.family_input_lookups += 1;
                recordings.get(key).cloned()
            });
            if cancelled() {
                return Ok(None);
            }
            let probability = if let Some(probability) = cached {
                curves.stats.family_input_hits += 1;
                Some(probability)
            } else {
                let deck = order.map(|slot| physical[slot].clone());
                curves
                    .certified_cancellable(
                        self.master,
                        &self.writer_skills,
                        self.notes,
                        self.events,
                        self.params,
                        self.setup,
                        self.play,
                        self.deltas,
                        &deck,
                        None,
                        None,
                        Some(&mut recordings),
                        &mut cancelled,
                    )
                    .map_err(|e| source_error(LuckFamilyDecline::ProbabilityDomain, e))?
            };
            let Some(probability) = probability else { return Ok(None) };
            if cancelled() {
                return Ok(None);
            }
            if let Some(key) = input_key {
                recordings.insert(key, probability.clone(), recording_capacity);
                recordings.report(&mut curves.stats);
            }
            let mut positions = [0; SLOTS];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            if !coverage.mark(0, ordinal) {
                return Err(fail(LuckFamilyDecline::IncompleteCoverage, "duplicate profile order"));
            }
            if !completed.orders.iter().any(|old| Arc::ptr_eq(&old.probability, &probability)) {
                completed.bytes = completed
                    .bytes
                    .checked_add(curve_bytes(&probability)?)
                    .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "profile curve payload overflow"))?;
            }
            completed.orders.push(LuckFamilyOrderLaw { positions, profile, probability });
            if completed.bytes > domain.limits.max_retained_bytes {
                return Err(fail(LuckFamilyDecline::Capacity, "complete profile exceeds the byte budget"));
            }
            if next_order(&mut order) != (ordinal + 1 < ORDERS) {
                return Err(fail(LuckFamilyDecline::IncompleteCoverage, "profile order enumeration is incomplete"));
            }
        }
        if cancelled() {
            return Ok(None);
        }
        if !coverage.complete() || completed.orders.len() != ORDERS {
            return Err(fail(LuckFamilyDecline::IncompleteCoverage, "incomplete original profile-order cover"));
        }
        Ok(Some(completed))
    }
}
