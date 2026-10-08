//! Whole-domain admission and individually completed writer profiles are separate capabilities.
use super::*;

mod programs;
pub use programs::{LuckFamilyProgram, LuckFamilyProgramKey};

#[derive(Debug)]
pub(super) struct AdmittedFamilyInputs {
    pub(super) base: [Performer; SLOTS],
    pub(super) bindings: LuckFamilyBindings,
    pub(super) probe_runs_admitted: bool,
    pub(super) controller_inputs: input_reuse::ControllerInputAdmission,
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

/// Whole physical-pair admission with a cumulative allowance for requested native profiles. This capability
/// does not promise that every possible writer profile fits that allowance. Each returned profile still
/// contains every original order; an unrequested, refused or interrupted profile remains unknown.
#[derive(Debug)]
pub struct LuckFamilyProfileDomain {
    domain: LuckFamilyDomain,
    work: LuckFamilyProfileWork,
}

/// Conservatively reserved native work, including the common geometry pass. A native preparation reserves
/// all 120 orders before execution; refusal or cancellation does not refund that reservation. Complete-program
/// transport executes no native recording frames and leaves these counters unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LuckFamilyProfileWork {
    pub profile_attempts: u64,
    pub reserved_order_evaluations: usize,
    pub reserved_frame_work: u64,
    pub budget_refusals: u64,
}

/// Exactly one original writer-owner profile, with all 120 original performance-order labels completed.
/// This cannot be used as a complete `LuckControllerFamily` or as another admitted domain's profile.
#[derive(Debug)]
pub struct LuckFamilyProfile {
    identity: Arc<()>,
    mapping: Arc<DomainTerminalMapping>,
    profile: usize,
    orders: Vec<LuckFamilyOrderLaw>,
    probe_runs_admitted: bool,
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

impl LuckFamilyProfileDomain {
    pub fn profile_count(&self) -> usize {
        self.domain.profile_count()
    }

    pub fn profile_for(&self, resources: &[Option<usize>; SLOTS]) -> Option<usize> {
        self.domain.profile_for(resources)
    }

    pub fn bindings(&self) -> Option<LuckFamilyBindings> {
        self.domain.bindings()
    }

    pub fn note_times(&self) -> &[i32] {
        self.domain.note_times()
    }

    pub fn probe_gate(&self) -> Option<i64> {
        self.domain.probe_gate()
    }

    pub fn owns_profile(&self, profile: &LuckFamilyProfile) -> bool {
        self.domain.owns_profile(profile)
    }

    pub fn retained_bytes(&self) -> usize {
        self.domain.retained_bytes().saturating_sub(size_of::<LuckFamilyDomain>()).saturating_add(size_of::<Self>())
    }

    pub fn work(&self) -> LuckFamilyProfileWork {
        self.work
    }

    fn reserve_profile(&mut self) -> Result<(), LuckFamilyError> {
        let orders = self
            .work
            .reserved_order_evaluations
            .checked_add(ORDERS)
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "profile order-work overflow"))?;
        let frames = (ORDERS as u64)
            .checked_mul(self.domain.mapping.frame_work)
            .and_then(|frames| frames.checked_add(self.work.reserved_frame_work))
            .ok_or_else(|| fail(LuckFamilyDecline::Capacity, "profile frame-work overflow"))?;
        if orders > self.domain.limits.max_order_evaluations || frames > self.domain.limits.max_frame_work {
            self.work.budget_refusals = self.work.budget_refusals.saturating_add(1);
            return Err(fail(LuckFamilyDecline::Budget, "requested profile exceeds remaining native work"));
        }
        self.work.profile_attempts = self.work.profile_attempts.saturating_add(1);
        self.work.reserved_order_evaluations = orders;
        self.work.reserved_frame_work = frames;
        Ok(())
    }
}

impl LuckFamilyProfile {
    pub fn profile(&self) -> usize {
        self.profile
    }

    pub fn orders(&self) -> &[LuckFamilyOrderLaw] {
        &self.orders
    }

    /// Maximum direct positive-7021 starts along a relaxed two-state path for this original order.
    /// This is an integer work bound, not a probability law or a score-factor amplitude. Every native path
    /// is represented; joining edges from incompatible controller states can only increase the maximum.
    /// The full physical domain must have fixed-true holders in one legal phase. The original frame cover,
    /// initial false state, closed final lifecycle and inactive music-clamped tail are checked separately.
    /// Missing evidence keeps the original command budget; it does not invalidate the completed law.
    pub fn order_probe_run_bound(&self, order: usize) -> Option<u64> {
        if !self.probe_runs_admitted || self.orders.len() != ORDERS {
            return None;
        }
        probe_runs::maximum(
            &self.orders.get(order)?.probability.probe_transitions,
            usize::try_from(self.mapping.frame_work).ok()?,
            self.mapping.first_late_frame,
        )
    }

    /// The direct untimed LUCK probe's ideal command prefix at every historical rank note agrees with
    /// its chart-time class in this complete original order. This certifies only that probe contribution:
    /// ordinary factors, Rush magnitude, native roundoff and integer rank arithmetic keep their own bounds.
    /// Geometry without complete physical phase/lifecycle admission or an entire closed order returns false.
    pub fn order_rank_probe_history_ready(&self, order: usize) -> bool {
        self.order_probe_certificates(order).is_some_and(|(_, ready)| ready)
    }

    /// Every historical rank note has the same ideal signed integer Rush prefix at its two adjacent
    /// original queries and at the terminal query. Its Rush class may therefore use this original order's
    /// complete `joint_at(note_time)` law. Whole-domain pair admission and every original order have already
    /// completed; geometry from an empty recorder alone cannot authorize the substitution.
    ///
    /// This is independent of direct-probe phase/lifetime admission. Conditioning a historical probe term
    /// jointly on Rush additionally requires `order_rank_probe_history_ready`. Ordinary command windows,
    /// native arithmetic error and integer rank allowances retain their separate complete bounds.
    pub fn order_rank_rush_history_ready(&self, order: usize) -> bool {
        self.orders.len() == ORDERS && self.orders.get(order).is_some() && self.mapping.rank_rush_history.is_some()
    }

    /// The two independent optional probe certificates, with one scan of the complete original clock.
    /// The first value is a relaxed maximum start count; the second additionally requires every historical
    /// rank query's ideal probe prefix to match its chart-time class. Neither value is a probability law.
    pub fn order_probe_certificates(&self, order: usize) -> Option<(u64, bool)> {
        self.order_probe_run_bound(order).map(|runs| (runs, self.mapping.rank_probe_history.is_some()))
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
        let Some(admitted) = self.admit_inputs(choices, limits, CoverWork::Complete, &mut cancelled)? else {
            return Ok(None);
        };
        self.retain_domain(choices, limits, admitted, &mut cancelled)
    }

    /// Check every physical pair and retain the complete writer-owner map. The work allowance must hold one
    /// full native profile; later native preparations reserve their complete 120-order work cumulatively.
    /// Only a returned `LuckFamilyProfile` supplies probabilities for its own original profile and labels.
    pub fn admit_profile_domain(
        &self,
        choices: &[Vec<LuckFamilyChoice>; SLOTS],
        limits: LuckFamilyLimits,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckFamilyProfileDomain>, LuckFamilyError> {
        let Some(admitted) = self.admit_inputs(choices, limits, CoverWork::FirstProfile, &mut cancelled)? else {
            return Ok(None);
        };
        let Some(domain) = self.retain_domain(choices, limits, admitted, &mut cancelled)? else {
            return Ok(None);
        };
        let result = LuckFamilyProfileDomain {
            domain,
            work: LuckFamilyProfileWork { reserved_frame_work: self.mapping.frame_work, ..Default::default() },
        };
        if result.retained_bytes() > limits.max_retained_bytes {
            return Err(fail(LuckFamilyDecline::Capacity, "profile domain exceeds the byte budget"));
        }
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(result))
    }

    fn retain_domain(
        &self,
        choices: &[Vec<LuckFamilyChoice>; SLOTS],
        limits: LuckFamilyLimits,
        admitted: AdmittedFamilyInputs,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<LuckFamilyDomain>, LuckFamilyError> {
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

    /// Complete one requested profile after reserving all of its native order/frame work. A failed or cancelled
    /// attempt keeps its charge. The private complete-cover representation cannot escape this capability.
    pub fn prepare_budgeted_profile(
        &self,
        domain: &mut LuckFamilyProfileDomain,
        profile: usize,
        curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckFamilyProfile>, LuckFamilyError> {
        if cancelled() {
            return Ok(None);
        }
        if !Arc::ptr_eq(&self.mapping, &domain.domain.mapping) {
            return Err(fail(LuckFamilyDecline::Context, "profile domain belongs to another immutable context"));
        }
        if profile >= domain.profile_count() {
            return Err(fail(LuckFamilyDecline::IncompleteCoverage, "writer profile outside the admitted domain"));
        }
        domain.reserve_profile()?;
        self.prepare_profile(&domain.domain, profile, curves, cancelled)
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
        let input_keys = (recording_capacity > 0)
            .then(|| {
                input_reuse::InputKeys::admitted_controller(
                    self.master,
                    &physical,
                    &self.writer_skills,
                    &domain.admitted.controller_inputs,
                )
            })
            .flatten();
        let mut completed = LuckFamilyProfile {
            identity: Arc::clone(&domain.identity),
            mapping: Arc::clone(&self.mapping),
            profile,
            orders: reserve(ORDERS)?,
            probe_runs_admitted: domain.admitted.probe_runs_admitted,
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
