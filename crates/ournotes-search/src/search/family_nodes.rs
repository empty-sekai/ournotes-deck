//! A fixed-member controller family bounds the remaining Snap choices of a joint depth-four node.
//!
//! Family preparation enumerates writer profiles and original performance orders, not reward-only Snap
//! assignments. The same completed coefficient table is reused by every paired prefix of that member family.

use crate::domain::CandidateDomain;
use crate::search::{
    expectation::PhysicalDeck,
    joint::{FamilyBoundResult, JointBounds, SLOTS},
    snaps::{FamilyProfileTable, ProfileRewardTemplate},
};
use ournotes_sim::{
    live::full::{
        LuckDpCache, LuckFamilyChoice, LuckFamilyContext, LuckFamilyDecline, LuckFamilyLimits, LuckFamilyProfileDomain,
        LuckFamilyProgram,
    },
    pool::Pool,
};
use std::{
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FamilyNodeOutcome {
    Upper(i128),
    Unavailable,
    Stopped,
}

#[cfg(test)]
#[path = "family_cache_tests.rs"]
mod cache_tests;

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FamilyRefusals {
    context: u64,
    terminal_mapping: u64,
    pair_domain: u64,
    recorder_admission: u64,
    life_feedback: u64,
    judgement_feedback: u64,
    writer_profiles: u64,
    probability_domain: u64,
    budget: u64,
    capacity: u64,
    incomplete_coverage: u64,
}

impl FamilyRefusals {
    fn record(&mut self, reason: LuckFamilyDecline) {
        let counter = match reason {
            LuckFamilyDecline::Context => &mut self.context,
            LuckFamilyDecline::TerminalMapping => &mut self.terminal_mapping,
            LuckFamilyDecline::PairDomain => &mut self.pair_domain,
            LuckFamilyDecline::RecorderAdmission => &mut self.recorder_admission,
            LuckFamilyDecline::LifeFeedback => &mut self.life_feedback,
            LuckFamilyDecline::JudgementFeedback => &mut self.judgement_feedback,
            LuckFamilyDecline::WriterProfiles => &mut self.writer_profiles,
            LuckFamilyDecline::ProbabilityDomain => &mut self.probability_domain,
            LuckFamilyDecline::Budget => &mut self.budget,
            LuckFamilyDecline::Capacity => &mut self.capacity,
            LuckFamilyDecline::IncompleteCoverage => &mut self.incomplete_coverage,
        };
        *counter += 1;
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FamilyNodeStats {
    pub(crate) context_checks: u64,
    pub(crate) context_refusals: FamilyRefusals,
    pub(crate) context_stopped: u64,
    pub(crate) context_ms: f64,
    pub(crate) checks: u64,
    pub(crate) bounded_nodes: u64,
    /// Singleton physical bindings checked against already retained complete profile coefficients.
    pub(crate) leaf_checks: u64,
    pub(crate) bounded_leaves: u64,
    pub(crate) family_lookups: u64,
    pub(crate) family_hits: u64,
    pub(crate) refused_hits: u64,
    /// Member families with at least one completed 120-label profile (not necessarily every profile).
    pub(crate) prepared_families: u64,
    pub(crate) admitted_families: u64,
    pub(crate) profile_lookups: u64,
    pub(crate) profile_hits: u64,
    pub(crate) profile_program_lookups: u64,
    pub(crate) profile_program_hits: u64,
    pub(crate) profile_program_builds: u64,
    pub(crate) profile_program_declines: u64,
    pub(crate) profile_program_evictions: u64,
    pub(crate) profile_program_peak_entries: usize,
    pub(crate) profile_program_peak_bytes: usize,
    pub(crate) profile_native_builds: u64,
    /// Conservative native work reservations; complete-program transport runs no native recording frames.
    pub(crate) reserved_profile_order_work: u64,
    pub(crate) reserved_profile_frame_work: u64,
    pub(crate) profile_budget_refusals: u64,
    pub(crate) probe_run_profiles: u64,
    pub(crate) probe_run_unavailable: u64,
    pub(crate) maximum_probe_runs: u64,
    /// Complete-mask binding checks, including repeated coefficient-table hits.
    pub(crate) binding_drift_checks: u64,
    pub(crate) binding_drift_tightened: u64,
    /// Coefficient reductions per unit of power, not time or unique saved work.
    pub(crate) binding_offset_reduction_sum: f64,
    pub(crate) maximum_binding_offset_reduction: f64,
    pub(crate) preparation_refusals: u64,
    pub(crate) preparation_declines: FamilyRefusals,
    /// Actual wall time spent in native family preparation, including refused or cancelled attempts.
    pub(crate) preparation_ms: f64,
    pub(crate) envelope_ms: f64,
    pub(crate) envelope_refusals: u64,
    pub(crate) capacity_declines: u64,
    pub(crate) stopped: u64,
    /// Complete profile/order certificates materialized, not actual DP propagations or scored orders.
    pub(crate) order_laws: u64,
    pub(crate) profiles: u64,
    pub(crate) evictions: u64,
    /// Cache container, one retained reward template and complete coefficient-table allocations; not RSS.
    pub(crate) peak_entries: usize,
    pub(crate) peak_bytes: usize,
}

impl FamilyNodeStats {
    fn record_bound(&mut self, bound: &FamilyBoundResult) {
        self.binding_drift_checks += bound.binding_drift_checks;
        self.binding_drift_tightened += bound.binding_drift_tightened;
        self.binding_offset_reduction_sum += bound.binding_offset_reduction_sum;
        self.maximum_binding_offset_reduction =
            self.maximum_binding_offset_reduction.max(bound.maximum_binding_offset_reduction);
    }
}

struct Entry {
    members: [usize; 5],
    state: Option<Box<FamilyState>>,
    bytes: usize,
    last_used: u64,
}

struct ProgramEntry {
    program: LuckFamilyProgram,
    last_used: u64,
    transported: bool,
}

struct FamilyState {
    domain: LuckFamilyProfileDomain,
    table: FamilyProfileTable,
    refused: Vec<bool>,
}

impl FamilyState {
    fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>()
            .checked_add(self.domain.retained_bytes().checked_sub(std::mem::size_of::<LuckFamilyProfileDomain>())?)?
            .checked_add(self.table.retained_bytes()?.checked_sub(std::mem::size_of::<FamilyProfileTable>())?)?
            .checked_add(self.refused.capacity().checked_mul(std::mem::size_of::<bool>())?)
    }
}

/// The context is immutably borrowed for the entire cache lifetime. The exact compiled reward template is held
/// by Rc, so changing a conversion partition or other compiled scope cannot authorize old member-family tables.
#[derive(Default)]
pub(crate) struct FamilyNodeCache<'a> {
    context: Option<&'a LuckFamilyContext<'a>>,
    scope: Option<Rc<ProfileRewardTemplate>>,
    entries: VecDeque<Entry>,
    programs: VecDeque<ProgramEntry>,
    use_sequence: u64,
    program_bytes: usize,
    limit: usize,
    byte_limit: usize,
    profile_limit: usize,
    order_work_limit: usize,
    frame_work_limit: u64,
    payload_bytes: usize,
    stats: FamilyNodeStats,
}

impl<'a> FamilyNodeCache<'a> {
    pub(crate) fn new(
        context: Option<&'a LuckFamilyContext<'a>>,
        entries: usize,
        bytes: usize,
        profile_budget: usize,
    ) -> Self {
        Self {
            context,
            limit: entries.min(64),
            byte_limit: bytes,
            profile_limit: profile_budget.min(31),
            order_work_limit: profile_budget.min(31) * crate::search::uniform::ORDERS,
            frame_work_limit: 16_000_000,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn with_profile_work_limits(mut self, orders: usize, frames: u64) -> Self {
        self.order_work_limit = self.order_work_limit.min(orders);
        self.frame_work_limit = self.frame_work_limit.min(frames);
        self
    }

    pub(crate) fn stats(&self) -> &FamilyNodeStats {
        &self.stats
    }

    pub(crate) fn enabled(&self) -> bool {
        self.context.is_some() && self.limit > 0 && self.byte_limit > 0 && self.profile_limit > 0
    }

    pub(crate) fn record_context_attempt(&mut self, refusal: Option<LuckFamilyDecline>, stopped: bool, ms: f64) {
        self.stats.context_checks += 1;
        self.stats.context_ms += ms;
        self.stats.context_stopped += u64::from(stopped);
        if let Some(reason) = refusal {
            self.stats.context_refusals.record(reason);
        }
    }

    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.entries.capacity().saturating_mul(std::mem::size_of::<Entry>()))
            .saturating_add(self.scope.as_ref().map_or(0, |scope| scope.retained_bytes()))
            .saturating_add(self.payload_bytes)
            .saturating_add(self.program_bytes)
    }

    fn set_scope(&mut self, scope: Rc<ProfileRewardTemplate>) -> bool {
        if self.scope.as_ref().is_some_and(|old| Rc::ptr_eq(old, &scope)) {
            return true;
        }
        self.entries.clear();
        self.payload_bytes = 0;
        self.programs = VecDeque::new();
        self.program_bytes = 0;
        self.scope = Some(scope);
        if self.bytes() > self.byte_limit {
            self.scope = None;
            self.entries.shrink_to_fit();
            self.stats.capacity_declines += 1;
            return false;
        }
        true
    }

    fn remember(&mut self, members: [usize; 5], state: Option<Box<FamilyState>>) {
        let bytes = match state.as_ref().map(|t| t.retained_bytes()) {
            Some(Some(bytes)) => bytes,
            Some(None) => return,
            None => 0,
        };
        if self.entries.try_reserve(1).is_err() {
            self.stats.capacity_declines += 1;
            return;
        }
        self.payload_bytes = self.payload_bytes.saturating_add(bytes);
        let last_used = self.next_use();
        self.entries.push_back(Entry { members, state, bytes, last_used });
        self.trim();
        self.report_retained();
    }

    fn next_use(&mut self) -> u64 {
        self.use_sequence = self.use_sequence.saturating_add(1);
        self.use_sequence
    }

    fn touch_family(&mut self, index: usize) {
        if let Some(mut entry) = self.entries.remove(index) {
            entry.last_used = self.next_use();
            self.entries.push_back(entry);
        }
    }

    fn touch_program(&mut self, index: usize) {
        if let Some(mut entry) = self.programs.remove(index) {
            entry.last_used = self.next_use();
            entry.transported = true;
            self.programs.push_back(entry);
        }
    }

    fn trim(&mut self) {
        self.trim_to(self.limit);
    }

    fn trim_to(&mut self, entries: usize) {
        while self.entries.len().saturating_add(self.programs.len()) > entries || self.bytes() > self.byte_limit {
            // A program that has actually transported all 120 labels avoids native recording when a derived
            // family table is rebuilt. Protect that demonstrated reuse from scans of derived tables. Cold
            // programs and families still compete by access age; the unchanged independent program limits
            // evict the oldest program when its own tier is full, so new native programs remain admissible.
            let cold_program = self.programs.iter().position(|program| !program.transported);
            let program_to_evict = match (self.entries.front(), cold_program) {
                (None, _) => (!self.programs.is_empty()).then_some(0),
                (Some(family), Some(index)) if self.programs[index].last_used <= family.last_used => Some(index),
                _ => None,
            };
            if let Some(index) = program_to_evict {
                self.programs.remove(index);
                self.stats.profile_program_evictions += 1;
                self.recount_programs();
            } else if let Some(old) = self.entries.pop_front() {
                self.payload_bytes -= old.bytes;
                self.stats.evictions += 1;
            } else {
                break;
            }
        }
        if self.entries.is_empty() && self.bytes() > self.byte_limit {
            self.entries.shrink_to_fit();
        }
        if self.programs.is_empty() && self.bytes() > self.byte_limit {
            self.programs.shrink_to_fit();
            self.recount_programs();
        }
    }

    fn recount_programs(&mut self) {
        self.program_bytes =
            LuckFamilyProgram::retained_collection_bytes(self.programs.iter().map(|entry| &entry.program))
                .and_then(|bytes| {
                    bytes.checked_sub(self.programs.len().checked_mul(std::mem::size_of::<LuckFamilyProgram>())?)
                })
                .and_then(|bytes| {
                    bytes.checked_add(self.programs.capacity().checked_mul(std::mem::size_of::<ProgramEntry>())?)
                })
                .unwrap_or(usize::MAX);
    }

    fn report_retained(&mut self) {
        self.stats.peak_entries = self.stats.peak_entries.max(self.entries.len() + self.programs.len());
        self.stats.peak_bytes = self.stats.peak_bytes.max(self.bytes());
        self.stats.profile_program_peak_entries = self.stats.profile_program_peak_entries.max(self.programs.len());
        self.stats.profile_program_peak_bytes = self.stats.profile_program_peak_bytes.max(self.program_bytes);
    }

    fn remember_program(&mut self, program: LuckFamilyProgram) {
        let limit = self.limit.saturating_sub(1).min(63);
        let bytes = self.byte_limit.min(1 << 20);
        if limit == 0 || program.retained_bytes() > bytes || self.programs.try_reserve(1).is_err() {
            self.stats.profile_program_declines += 1;
            return;
        }
        let last_used = self.next_use();
        self.programs.push_back(ProgramEntry { program, last_used, transported: false });
        self.recount_programs();
        while self.programs.len() > limit || self.program_bytes > bytes {
            if self.programs.pop_front().is_none() {
                break;
            }
            self.stats.profile_program_evictions += 1;
            self.recount_programs();
        }
        if self.programs.is_empty() && self.program_bytes > bytes {
            self.programs.shrink_to_fit();
            self.recount_programs();
        }
        // Reserve one original entry for the family currently taken out for preparation. Prototypes and
        // admitted-domain states otherwise share exactly the same original entry and byte allowance.
        self.trim_to(self.limit.saturating_sub(1));
        self.report_retained();
    }

    fn family(
        &mut self,
        pool: &Pool<'_>,
        domain: &CandidateDomain,
        members: [usize; 5],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<Box<FamilyState>>, ()> {
        if cancelled() {
            return Err(());
        }
        self.stats.family_lookups += 1;
        if let Some(index) = self.entries.iter().position(|entry| entry.members == members) {
            if cancelled() {
                return Err(());
            }
            if self.entries[index].state.is_none() {
                self.stats.refused_hits += 1;
                self.touch_family(index);
                return Ok(None);
            }
            self.stats.family_hits += 1;
            let entry = self.entries.remove(index).expect("matched family entry");
            self.payload_bytes -= entry.bytes;
            return Ok(entry.state);
        }
        let Some(context) = self.context else { return Ok(None) };
        let mut choices: [Vec<LuckFamilyChoice>; 5] = std::array::from_fn(|_| Vec::new());
        for slot in 0..5 {
            if cancelled() {
                return Err(());
            }
            if choices[slot].try_reserve_exact(domain.snaps().len() + 1).is_err() {
                self.stats.capacity_declines += 1;
                return Ok(None);
            }
            for snap in std::iter::once(None).chain(domain.snaps().iter().copied().map(Some)) {
                let performer =
                    match crate::search::snaps::performer(&pool.members[members[slot]], snap.map(|s| &pool.snaps[s])) {
                        Ok(performer) => performer,
                        Err(_) => {
                            if cancelled() {
                                return Err(());
                            }
                            self.stats.preparation_refusals += 1;
                            self.stats.preparation_declines.record(LuckFamilyDecline::PairDomain);
                            self.remember(members, None);
                            return Ok(None);
                        }
                    };
                choices[slot].push(LuckFamilyChoice { resource: snap, performer });
            }
        }
        let limits = LuckFamilyLimits {
            max_pair_models: 4096,
            max_profiles: self.profile_limit,
            max_order_evaluations: self.order_work_limit,
            max_frame_work: self.frame_work_limit,
            max_retained_bytes: 32 * 1024 * 1024,
        };
        let started = crate::clock::Instant::now();
        let prepared = context.admit_profile_domain(&choices, limits, &mut *cancelled);
        self.stats.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
        let family = match prepared {
            Ok(Some(family)) => family,
            Ok(None) => return Err(()),
            Err(refusal) => {
                if cancelled() {
                    return Err(());
                }
                self.stats.preparation_refusals += 1;
                self.stats.preparation_declines.record(refusal.reason);
                self.remember(members, None);
                return Ok(None);
            }
        };
        let Some(table) = self.scope.as_ref().and_then(|scope| scope.start_budgeted_profiles(members, &family)) else {
            self.stats.envelope_refusals += 1;
            self.remember(members, None);
            return Ok(None);
        };
        let mut refused = Vec::new();
        if refused.try_reserve_exact(family.profile_count()).is_err() {
            self.stats.capacity_declines += 1;
            return Ok(None);
        }
        refused.resize(family.profile_count(), false);
        if cancelled() {
            return Err(());
        }
        self.stats.admitted_families += 1;
        self.stats.reserved_profile_frame_work += family.work().reserved_frame_work;
        Ok(Some(Box::new(FamilyState { domain: family, table, refused })))
    }

    fn complete_required(
        &mut self,
        state: &mut FamilyState,
        required: &[usize],
        curves: &mut LuckDpCache,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<bool, ()> {
        let Some(context) = self.context else { return Ok(false) };
        for &profile in required {
            if cancelled() {
                return Err(());
            }
            self.stats.profile_lookups += 1;
            if state.table.profiles.get(profile).is_some_and(Option::is_some) {
                self.stats.profile_hits += 1;
                continue;
            }
            if state.refused.get(profile).copied() != Some(false) {
                return Ok(false);
            }
            let started = crate::clock::Instant::now();
            let capacity = self.byte_limit.min(1 << 20);
            let key = if self.limit > 1 {
                context.budgeted_profile_program_key(&state.domain, profile, capacity, &mut *cancelled)
            } else {
                None
            };
            if cancelled() {
                self.stats.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
                return Err(());
            }
            let transported = key.as_ref().and_then(|key| {
                self.stats.profile_program_lookups += 1;
                let index = self.programs.iter().position(|entry| entry.program.matches(key))?;
                let law =
                    self.programs[index].program.transport_budgeted(&state.domain, key, profile, &mut *cancelled)?;
                self.touch_program(index);
                Some(law)
            });
            if cancelled() {
                self.stats.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
                return Err(());
            }
            let law = if let Some(law) = transported {
                self.stats.profile_program_hits += 1;
                law
            } else {
                if key.is_none() {
                    self.stats.profile_program_declines += 1;
                }
                let before = state.domain.work();
                let prepared =
                    context.prepare_budgeted_profile(&mut state.domain, profile, Some(&mut *curves), &mut *cancelled);
                let after = state.domain.work();
                self.stats.reserved_profile_order_work +=
                    (after.reserved_order_evaluations - before.reserved_order_evaluations) as u64;
                self.stats.reserved_profile_frame_work += after.reserved_frame_work - before.reserved_frame_work;
                self.stats.profile_budget_refusals += after.budget_refusals - before.budget_refusals;
                let law = match prepared {
                    Ok(Some(law)) => law,
                    Ok(None) => {
                        self.stats.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
                        return Err(());
                    }
                    Err(refusal) => {
                        self.stats.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
                        if cancelled() {
                            return Err(());
                        }
                        self.stats.preparation_refusals += 1;
                        self.stats.preparation_declines.record(refusal.reason);
                        state.refused[profile] = true;
                        return Ok(false);
                    }
                };
                self.stats.profile_native_builds += 1;
                if let Some(key) = key {
                    if let Some(program) =
                        LuckFamilyProgram::from_budgeted_profile(&state.domain, key, &law, capacity, &mut *cancelled)
                    {
                        self.stats.profile_program_builds += 1;
                        self.remember_program(program);
                    } else if !cancelled() {
                        self.stats.profile_program_declines += 1;
                    }
                }
                law
            };
            self.stats.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
            if cancelled() {
                return Err(());
            }
            let started = crate::clock::Instant::now();
            let reward = self
                .scope
                .as_ref()
                .and_then(|scope| scope.bind_budgeted_profile(state.table.members, &state.domain, &law, cancelled));
            self.stats.envelope_ms += started.elapsed().as_secs_f64() * 1000.0;
            if cancelled() {
                return Err(());
            }
            let Some(reward) = reward else {
                self.stats.envelope_refusals += 1;
                state.refused[profile] = true;
                return Ok(false);
            };
            // Publish only the complete 120-label reward. Unrequested and stopped profiles remain unknown.
            if state.table.profiles.iter().all(Option::is_none) {
                self.stats.prepared_families += 1;
            }
            self.stats.order_laws += law.orders().len() as u64;
            self.stats.profiles += 1;
            if let Some(runs) = reward.max_probe_runs {
                self.stats.probe_run_profiles += 1;
                self.stats.maximum_probe_runs = self.stats.maximum_probe_runs.max(runs);
            } else {
                self.stats.probe_run_unavailable += 1;
            }
            state.table.profiles[profile] = Some(reward);
        }
        if cancelled() {
            return Err(());
        }
        Ok(true)
    }

    /// Reuse an already completed profile for one exact physical leaf. The parent may remain open because
    /// a different last-slot binding has a high cap; that does not prevent excluding this cheaper child.
    /// This lookup performs no new recording, profile preparation, cache insertion or work reservation.
    pub(crate) fn cached_leaf_upper(
        &mut self,
        pool: &Pool<'_>,
        domain: &CandidateDomain,
        bounds: &JointBounds,
        physical: &PhysicalDeck,
        orders: &[([usize; 5], u128)],
        cancelled: &mut impl FnMut() -> bool,
    ) -> FamilyNodeOutcome {
        if cancelled() {
            self.stats.stopped += 1;
            return FamilyNodeOutcome::Stopped;
        }
        if !self.enabled() || orders != crate::search::uniform::MEAN_ORDERS.as_slice() {
            return FamilyNodeOutcome::Unavailable;
        }
        self.stats.leaf_checks += 1;
        let Some(scope) = bounds.family_reward_template() else { return FamilyNodeOutcome::Unavailable };
        if !self.scope.as_ref().is_some_and(|old| Rc::ptr_eq(old, &scope))
            || domain.check_fixed(pool, physical).is_err()
        {
            return FamilyNodeOutcome::Unavailable;
        }
        let mut members = physical.members;
        let mut others = [members[0], members[1], members[3], members[4]];
        others.sort_unstable();
        for (slot, member) in [0, 1, 3, 4].into_iter().zip(others) {
            members[slot] = member;
        }
        let Some(index) = self.entries.iter().position(|entry| entry.members == members) else {
            return FamilyNodeOutcome::Unavailable;
        };
        let Some(state) = self.entries[index].state.as_ref() else { return FamilyNodeOutcome::Unavailable };
        let choice = match physical.snaps[SLOTS[4]] {
            None => 0,
            Some(snap) => match domain.snaps().iter().position(|&value| value == snap) {
                Some(index) => index + 1,
                None => return FamilyNodeOutcome::Unavailable,
            },
        };
        let cap = bounds.family_profiles_upper_measured(domain, physical, &state.table, &[choice]);
        if cancelled() {
            self.stats.stopped += 1;
            return FamilyNodeOutcome::Stopped;
        }
        match cap {
            Some(cap) => {
                self.touch_family(index);
                self.stats.record_bound(&cap);
                self.stats.bounded_leaves += 1;
                FamilyNodeOutcome::Upper(cap.upper)
            }
            None => FamilyNodeOutcome::Unavailable,
        }
    }

    /// A complete upper for the joint depth-four subtree. One missing/refused member family prevents this
    /// optional node certificate. Original traversal and its old bound then continue unchanged.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upper_at_depth_four(
        &mut self,
        pool: &Pool<'_>,
        domain: &CandidateDomain,
        bounds: &JointBounds,
        physical: &PhysicalDeck,
        start: usize,
        orders: &[([usize; 5], u128)],
        curves: &mut LuckDpCache,
        cancelled: &mut impl FnMut() -> bool,
    ) -> FamilyNodeOutcome {
        if cancelled() {
            self.stats.stopped += 1;
            return FamilyNodeOutcome::Stopped;
        }
        if !self.enabled()
            || orders != crate::search::uniform::MEAN_ORDERS.as_slice()
            || start > bounds.choices.len()
            || domain.snaps().len() > 4096
        {
            return FamilyNodeOutcome::Unavailable;
        }
        let Some(scope) = bounds.family_reward_template() else { return FamilyNodeOutcome::Unavailable };
        if !self.set_scope(scope) {
            return FamilyNodeOutcome::Unavailable;
        }
        self.stats.checks += 1;
        let missing: Vec<_> = domain
            .required()
            .iter()
            .copied()
            .filter(|m| !SLOTS[..4].iter().any(|&slot| physical.members[slot] == *m))
            .collect();
        if missing.len() > 1 {
            self.stats.bounded_nodes += 1;
            return FamilyNodeOutcome::Upper(0);
        }
        let slot = SLOTS[4];
        let mut remaining: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, &(member, choice)) in bounds.choices[start..].iter().enumerate() {
            if i.is_multiple_of(64) && cancelled() {
                self.stats.stopped += 1;
                return FamilyNodeOutcome::Stopped;
            }
            let character = pool.members[member].character_id;
            if SLOTS[..4].iter().any(|&s| pool.members[physical.members[s]].character_id == character)
                || missing.first().is_some_and(|&required| required != member)
                || domain.required().iter().any(|&r| r != member && pool.members[r].character_id == character)
                || !bounds.allows(slot, choice)
            {
                continue;
            }
            let snap = if choice == 0 { None } else { domain.snaps().get(choice - 1).copied() };
            if choice > 0 && snap.is_none() {
                return FamilyNodeOutcome::Unavailable;
            }
            if snap.is_some() && SLOTS[..4].iter().any(|&s| physical.snaps[s] == snap) {
                continue;
            }
            let choices = remaining.entry(member).or_default();
            if !choices.contains(&choice) {
                choices.push(choice);
            }
        }
        let mut upper = 0i128;
        for (member, choices) in remaining {
            let mut complete = *physical;
            complete.members[slot] = member;
            let mut members = complete.members;
            let mut others = [members[0], members[1], members[3], members[4]];
            others.sort_unstable();
            for (slot, member) in [0, 1, 3, 4].into_iter().zip(others) {
                members[slot] = member;
            }
            let mut state = match self.family(pool, domain, members, cancelled) {
                Ok(Some(state)) => state,
                Ok(None) => return FamilyNodeOutcome::Unavailable,
                Err(()) => {
                    self.stats.stopped += 1;
                    return FamilyNodeOutcome::Stopped;
                }
            };
            let required = bounds.required_family_profiles(domain, &complete, &state.table, &choices);
            let result = match required {
                Some(required) => self.complete_required(&mut state, &required, curves, cancelled),
                None => Ok(false),
            };
            let cap = if matches!(result, Ok(true)) {
                bounds.family_profiles_upper_measured(domain, &complete, &state.table, &choices)
            } else {
                None
            };
            self.remember(members, Some(state));
            if result.is_err() {
                self.stats.stopped += 1;
                return FamilyNodeOutcome::Stopped;
            }
            let Some(cap) = cap else { return FamilyNodeOutcome::Unavailable };
            self.stats.record_bound(&cap);
            upper = upper.max(cap.upper);
        }
        if cancelled() {
            self.stats.stopped += 1;
            return FamilyNodeOutcome::Stopped;
        }
        self.stats.bounded_nodes += 1;
        FamilyNodeOutcome::Upper(upper)
    }
}
