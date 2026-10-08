//! Uniform expected-score envelopes of an admitted fixed-member controller family.
//!
//! These coefficients are exclusions, never candidate values. The native family owns the complete writer-profile
//! and original-order cover and the common terminal-query mapping. Ordinary history, Rush magnitude and
//! conversion allowances retain their original complete-domain envelopes. A separate historical-query
//! capability may weight only the direct probe's ideal rank contribution. Integer probe-work evidence may
//! tighten the unweighted floating-point drift allowance for the actual five physical bindings.

use super::*;
use ournotes_sim::live::{
    certified::F64Interval,
    full::{LuckFamilyBindings, LuckFamilyOrderLaw, LuckFamilyProfile, LuckFamilyProfileDomain},
};

#[cfg(test)]
use ournotes_sim::live::full::{LuckControllerFamily, LuckFamilyDomain};

const MAX_TEMPLATE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CURVE_REWARDS: usize = 128;
const MAX_CURVE_REWARD_BYTES: usize = 1024 * 1024;

mod drift;
use drift::{DriftTemplate, PairWork};

#[derive(Clone, Copy)]
struct Term {
    lo: usize,
    hi: usize,
    note: f64,
    judge: [f64; 4],
    probe: bool,
}

struct Pair {
    terms: Vec<Term>,
    budget: f64,
    /// A contribution with a separately modelled ramp keeps its original complete upper coefficient.
    opaque: Option<f64>,
    work: Option<PairWork>,
}

/// Immutable reward-only inputs owned by one exact compiled domain. Physical choices keep their class mapping;
/// no actual candidate or probability law is replaced by a class representative.
pub(crate) struct ProfileRewardTemplate {
    times: Vec<i32>,
    /// Original terminal coefficient without/with the unconditional native Rush allowance.
    terminal: Vec<[f64; 2]>,
    history: Vec<f64>,
    z: Vec<f64>,
    jp: Vec<[f64; 5]>,
    class_of: Vec<Vec<usize>>,
    pairs: Vec<Vec<[Pair; 5]>>,
    /// The complete-domain absolute factor-drift coefficient; never weighted by a terminal probability.
    offset: f64,
    /// Independent unweighted command-history certificate. Unavailable data keeps `offset` unchanged.
    drift: Option<DriftTemplate>,
    /// Only the positive per-note arithmetic chain, from additive_joint_envelope. Legacy relative drift is
    /// never accepted by this template.
    eps: f64,
    global: f64,
    bytes: usize,
}

/// One completed envelope covers every admitted physical writer profile and all original orders. Binding a
/// node chooses its exact profile only after the whole table is complete; no profile or order is omitted.
#[cfg(test)]
pub(crate) struct FamilyRewardTable {
    pub(crate) members: [usize; 5],
    pub(crate) bindings: LuckFamilyBindings,
    pub(crate) profiles: Vec<FamilyProfileReward>,
    pub(crate) eps: f64,
    pub(crate) global: f64,
}

/// An admitted full physical domain with independently completed writer profiles. `None` means unknown,
/// never zero; a node may use this table only if every feasible binding has a completed profile.
pub(crate) struct FamilyProfileTable {
    pub(crate) members: [usize; 5],
    pub(crate) bindings: LuckFamilyBindings,
    pub(crate) profiles: Vec<Option<FamilyProfileReward>>,
    pub(crate) eps: f64,
    pub(crate) global: f64,
}

/// All coefficients share one fixed physical writer assignment throughout the complete uniform shuffle.
pub(crate) struct FamilyProfileReward {
    pub(crate) a0: f64,
    pub(crate) mean: [Vec<f64>; 5],
    /// The same complete 120-label coefficient sum before adding the global history-drift allowance.
    base_a0: f64,
    /// Maximum over all 120 original labels. Missing evidence in even one label keeps the old offset.
    pub(crate) max_probe_runs: Option<u64>,
    /// Original labels with a separate historical-query probe certificate; unknown labels kept the old bound.
    pub(crate) rank_probe_history_ready_orders: usize,
    /// Diagnostic upper-coefficient reduction for a unit probe covering every note, averaged over all 120 labels.
    /// This is neither an actual binding's saved score nor native work avoided.
    pub(crate) mean_unit_probe_history_reduction: f64,
}

/// Reward arithmetic for one immutable complete probability result, with every physical choice at all five
/// positions. The Arc identity authorizes `joint_at` and original-frame probe masks under this bind call's fixed
/// template, members, gate and lifecycle admission. It certifies no program/path equivalence; every original
/// profile/order still contributes its own scalar, integer work maximum and coverage label.
struct CurveRewards {
    a0: f64,
    base_a0: f64,
    probe_runs: Option<u64>,
    rank_probe_history_ready: bool,
    unit_probe_history_reduction: f64,
    gains: [Vec<[f64; 5]>; 5],
}

struct CurveRewardEntry<'a> {
    law: &'a LuckFamilyOrderLaw,
    reward: CurveRewards,
}

struct CurveRewardCache<'a> {
    entries: Vec<CurveRewardEntry<'a>>,
    limit: usize,
    byte_limit: usize,
    payload_bytes: usize,
}

impl<'a> CurveRewardCache<'a> {
    fn new(limit: usize, byte_limit: usize) -> Self {
        let limit = limit.min(MAX_CURVE_REWARDS);
        let byte_limit = byte_limit.min(MAX_CURVE_REWARD_BYTES);
        let mut entries = Vec::new();
        let affordable =
            byte_limit.saturating_sub(std::mem::size_of::<Self>()) / std::mem::size_of::<CurveRewardEntry<'a>>();
        let limit = limit.min(affordable);
        let limit = if entries.try_reserve_exact(limit).is_ok() { limit } else { 0 };
        let mut out = Self { entries, limit, byte_limit, payload_bytes: 0 };
        if out.bytes().is_none_or(|bytes| bytes > byte_limit) {
            out.entries = Vec::new();
            out.limit = 0;
        }
        out
    }

    fn bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>().checked_add(vector_bytes(&self.entries)?)?.checked_add(self.payload_bytes)
    }

    fn can_store(&self, counts: [usize; 5]) -> bool {
        self.entries.len() < self.limit
            && counts
                .into_iter()
                .try_fold(0usize, |sum, count| sum.checked_add(count))
                .and_then(|count| count.checked_mul(std::mem::size_of::<[f64; 5]>()))
                .and_then(|payload| self.bytes()?.checked_add(payload))
                .is_some_and(|bytes| bytes <= self.byte_limit)
    }

    fn get(&self, law: &LuckFamilyOrderLaw) -> Option<&CurveRewards> {
        self.entries.iter().find(|entry| entry.law.shares_joint_curve(law)).map(|entry| &entry.reward)
    }

    fn insert(&mut self, law: &'a LuckFamilyOrderLaw, reward: CurveRewards) {
        let payload = reward.gains.iter().try_fold(0usize, |bytes, row| bytes.checked_add(vector_bytes(row)?));
        let Some(payload) = payload else { return };
        if self.entries.len() >= self.limit
            || self.bytes().and_then(|bytes| bytes.checked_add(payload)).is_none_or(|bytes| bytes > self.byte_limit)
        {
            return;
        }
        // The entry vector was reserved up front. Refusing this optional retention changes only recomputation.
        self.payload_bytes += payload;
        self.entries.push(CurveRewardEntry { law, reward });
    }
}

/// Outward coefficient sums under one fixed physical writer profile. The complete native capability already
/// certifies the labels; this local cover also prevents a missing or repeated label from changing the divisor.
struct ProfileSums {
    seen: [u64; 2],
    a0: F64Interval,
    base_a0: F64Interval,
    max_probe_runs: Option<u64>,
    rank_probe_history_ready_orders: usize,
    unit_probe_history_reduction: F64Interval,
    gains: [Vec<F64Interval>; 5],
}

impl ProfileSums {
    fn new(counts: [usize; 5]) -> Option<Self> {
        let mut gains: [Vec<F64Interval>; 5] = std::array::from_fn(|_| Vec::new());
        for (row, count) in gains.iter_mut().zip(counts) {
            row.try_reserve_exact(count).ok()?;
            row.resize(count, F64Interval::ZERO);
        }
        Some(Self {
            seen: [0; 2],
            a0: F64Interval::ZERO,
            base_a0: F64Interval::ZERO,
            max_probe_runs: None,
            rank_probe_history_ready_orders: 0,
            unit_probe_history_reduction: F64Interval::ZERO,
            gains,
        })
    }

    fn record(&mut self, positions: &[usize; 5]) -> Option<()> {
        let mut mask = 0u8;
        for &position in positions {
            if position >= 5 || mask & (1 << position) != 0 {
                return None;
            }
            mask |= 1 << position;
        }
        let index = crate::search::uniform::order_index(positions);
        let bit = 1u64 << (index % 64);
        if self.seen[index / 64] & bit != 0 {
            return None;
        }
        self.seen[index / 64] |= bit;
        Some(())
    }

    fn complete(&self) -> bool {
        self.seen == [u64::MAX, (1u64 << (crate::search::uniform::ORDERS - 64)) - 1]
    }

    fn add_curve(&mut self, positions: &[usize; 5], reward: &CurveRewards) -> Option<()> {
        self.a0 = self.a0.add(point(reward.a0)?);
        self.base_a0 = self.base_a0.add(point(reward.base_a0)?);
        self.max_probe_runs = self.max_probe_runs.zip(reward.probe_runs).map(|(old, runs)| old.max(runs));
        self.rank_probe_history_ready_orders += usize::from(reward.rank_probe_history_ready);
        self.unit_probe_history_reduction =
            self.unit_probe_history_reduction.add(point(reward.unit_probe_history_reduction)?);
        for (slot, sums) in self.gains.iter_mut().enumerate() {
            if sums.len() != reward.gains[slot].len() {
                return None;
            }
            for (sum, values) in sums.iter_mut().zip(&reward.gains[slot]) {
                *sum = sum.add(point(*values.get(positions[slot])?)?);
            }
        }
        Some(())
    }

    fn finish(self) -> Option<FamilyProfileReward> {
        if !self.complete() {
            return None;
        }
        let mut mean: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
        for (row, sums) in mean.iter_mut().zip(self.gains) {
            row.try_reserve_exact(sums.len()).ok()?;
            for sum in sums {
                row.push(uniform_upper(sum)?);
            }
        }
        Some(FamilyProfileReward {
            a0: uniform_upper(self.a0)?,
            mean,
            base_a0: uniform_upper(self.base_a0)?,
            max_probe_runs: self.max_probe_runs,
            rank_probe_history_ready_orders: self.rank_probe_history_ready_orders,
            mean_unit_probe_history_reduction: uniform_upper(self.unit_probe_history_reduction)?,
        })
    }
}

fn uniform_upper(sum: F64Interval) -> Option<f64> {
    finite_nonnegative(sum.divide(F64Interval::integer(crate::search::uniform::ORDERS as i128)).ok()?.upper())
}

fn finite_nonnegative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn point(value: f64) -> Option<F64Interval> {
    F64Interval::point(finite_nonnegative(value)?).ok()
}

fn reserve<T>(length: usize) -> Option<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).ok()?;
    Some(values)
}

fn copy_slice<T: Copy>(source: &[T]) -> Option<Vec<T>> {
    let mut out = reserve(source.len())?;
    out.extend_from_slice(source);
    Some(out)
}

fn vector_bytes<T>(values: &Vec<T>) -> Option<usize> {
    values.capacity().checked_mul(std::mem::size_of::<T>())
}

fn pair_reward(
    row: &Pair,
    normal: &[Vec<F64Interval>; 5],
    probe: &[F64Interval],
    probe_enabled: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Option<f64> {
    if let Some(gain) = row.opaque {
        return finite_nonnegative(gain);
    }
    let mut gain = point(row.budget)?;
    for (index, term) in row.terms.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return None;
        }
        let note = if probe_enabled && term.probe { probe } else { &normal[0] };
        gain = gain.add(note[term.hi].subtract(note[term.lo]).multiply(point(term.note)?));
        for j in 0..4 {
            if term.judge[j] != 0.0 {
                gain =
                    gain.add(normal[j + 1][term.hi].subtract(normal[j + 1][term.lo]).multiply(point(term.judge[j])?));
            }
        }
    }
    finite_nonnegative(gain.upper().max(0.0))
}

/// Both complete arithmetic envelopes remain available. The explicit minimum preserves the original cap
/// even if outward prefix subtraction happens to be a few ulps wider after a negligible probability change.
fn pair_reward_with_history(
    row: &Pair,
    normal: &[Vec<F64Interval>; 5],
    probe: &[F64Interval],
    original_probe: Option<&[F64Interval]>,
    probe_enabled: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Option<f64> {
    let gain = pair_reward(row, normal, probe, probe_enabled, cancelled)?;
    if probe_enabled && row.opaque.is_none() && row.terms.iter().any(|term| term.probe) {
        if let Some(original) = original_probe {
            return Some(gain.min(pair_reward(row, normal, original, true, cancelled)?));
        }
    }
    Some(gain)
}

impl ProfileRewardTemplate {
    pub(crate) fn compile(live: &SnapLive<'_>) -> Result<Rc<Self>, crate::search::telemetry::FamilyTemplateRefusal> {
        use crate::search::telemetry::FamilyTemplateRefusal as Refusal;
        // additive_joint_envelope charges delta * B using the original complete-domain command count and
        // all-Rush/rank/judgement sensitivity. Its third result is only the relative arithmetic chain. Requiring
        // this representation prevents shrinking an absolute history error by multiplying a smaller mean gain.
        let (with_offset, global, eps) = live.joint_additive.ok_or(Refusal::AdditiveEnvelope)?;
        if !live.terminal_caps_admitted {
            return Err(Refusal::TerminalCaps);
        }
        if !live.fine.rush_eligible {
            return Err(Refusal::RushClass);
        }
        if live.fine.network_ranking {
            return Err(Refusal::NetworkRanking);
        }
        let n = live.coef.times.len();
        if n == 0 || live.coef.family_terminal.len() != n || live.fine.rank.len() != n {
            return Err(Refusal::CoefficientShape);
        }
        let offset = point(with_offset)
            .ok_or(Refusal::CoefficientDomain)?
            .subtract(point(live.a0).ok_or(Refusal::CoefficientDomain)?)
            .upper();
        finite_nonnegative(offset).ok_or(Refusal::CoefficientDomain)?;
        let minimum_bytes = std::mem::size_of::<Self>()
            .checked_add(2 * std::mem::size_of::<usize>())
            .ok_or(Refusal::CapacityArithmetic)?
            .checked_add(
                n.checked_mul(std::mem::size_of::<i32>() + 9 * std::mem::size_of::<f64>())
                    .ok_or(Refusal::CapacityArithmetic)?,
            )
            .ok_or(Refusal::CapacityArithmetic)?;
        if minimum_bytes > MAX_TEMPLATE_BYTES {
            return Err(Refusal::Capacity);
        }
        let mut history = reserve(n).ok_or(Refusal::Allocation)?;
        let mut jp = reserve(n).ok_or(Refusal::Allocation)?;
        for e in 0..n {
            let [plain, rush] = live.coef.family_terminal[e];
            if plain > rush {
                return Err(Refusal::TerminalOrder);
            }
            if live.fine.rank[e] < 1.0 {
                return Err(Refusal::RankDomain);
            }
            point(plain).ok_or(Refusal::CoefficientDomain)?;
            let extra = point(live.fine.rank[e]).ok_or(Refusal::CoefficientDomain)?.subtract(F64Interval::ONE);
            history.push(
                finite_nonnegative(point(rush).ok_or(Refusal::CoefficientDomain)?.multiply(extra).upper())
                    .ok_or(Refusal::CoefficientDomain)?,
            );
            point(live.coef.z[e]).ok_or(Refusal::CoefficientDomain)?;
            let mut row = [live.coef.max_jp[e]; 5];
            row[1..].copy_from_slice(&live.coef.jp[e]);
            for value in row {
                point(value).ok_or(Refusal::CoefficientDomain)?;
            }
            jp.push(row);
        }
        let mut pairs = reserve(live.contrib.len()).ok_or(Refusal::Allocation)?;
        let mut estimate = minimum_bytes;
        for classes in &live.contrib {
            let mut rows = reserve(classes.len()).ok_or(Refusal::Allocation)?;
            for positions in classes {
                let mut built = reserve(5).ok_or(Refusal::Allocation)?;
                for contribution in positions {
                    let opaque = contribution.windows.iter().any(|w| w.ramp != 0).then_some(contribution.gain);
                    if let Some(gain) = opaque {
                        point(gain).ok_or(Refusal::CoefficientDomain)?;
                    }
                    let mut terms = reserve(if opaque.is_some() { 0 } else { contribution.windows.len() })
                        .ok_or(Refusal::Allocation)?;
                    if opaque.is_none() {
                        for w in &contribution.windows {
                            if w.lo > w.hi || w.hi as usize > n {
                                return Err(Refusal::WindowRange);
                            }
                            point(w.note).ok_or(Refusal::CoefficientDomain)?;
                            for value in w.judge {
                                point(value).ok_or(Refusal::CoefficientDomain)?;
                            }
                            let probe = w
                                .rush
                                .checked_sub(1)
                                .and_then(|i| contribution.rush.get(i as usize))
                                .is_some_and(|r| r.terminal_probe(Some(MISSION_LUCK)));
                            if probe && w.judge != [0.0; 4] {
                                return Err(Refusal::ProbeJudgement);
                            }
                            terms.push(Term {
                                lo: w.lo as usize,
                                hi: w.hi as usize,
                                note: w.note,
                                judge: w.judge,
                                probe,
                            });
                        }
                    }
                    point(contribution.budget).ok_or(Refusal::CoefficientDomain)?;
                    estimate = estimate
                        .checked_add(std::mem::size_of::<Pair>())
                        .ok_or(Refusal::CapacityArithmetic)?
                        .checked_add(
                            terms
                                .capacity()
                                .checked_mul(std::mem::size_of::<Term>())
                                .ok_or(Refusal::CapacityArithmetic)?,
                        )
                        .ok_or(Refusal::CapacityArithmetic)?;
                    if estimate > MAX_TEMPLATE_BYTES {
                        return Err(Refusal::Capacity);
                    }
                    built.push(Pair { terms, budget: contribution.budget, opaque, work: PairWork::new(contribution) });
                }
                rows.push(built.try_into().map_err(|_| Refusal::PositionShape)?);
            }
            estimate =
                estimate.checked_add(std::mem::size_of::<Vec<[Pair; 5]>>()).ok_or(Refusal::CapacityArithmetic)?;
            pairs.push(rows);
        }
        let mut class_of = reserve(live.class_of.len()).ok_or(Refusal::Allocation)?;
        for classes in &live.class_of {
            let mut row =
                reserve(classes.len().checked_add(1).ok_or(Refusal::CapacityArithmetic)?).ok_or(Refusal::Allocation)?;
            row.push(0);
            row.extend(classes.iter().map(|&class| usize::from(class)));
            estimate = estimate
                .checked_add(std::mem::size_of::<Vec<usize>>())
                .ok_or(Refusal::CapacityArithmetic)?
                .checked_add(
                    row.capacity().checked_mul(std::mem::size_of::<usize>()).ok_or(Refusal::CapacityArithmetic)?,
                )
                .ok_or(Refusal::CapacityArithmetic)?;
            class_of.push(row);
        }
        if estimate > MAX_TEMPLATE_BYTES {
            return Err(Refusal::Capacity);
        }
        let mut template = Self {
            times: copy_slice(&live.coef.times).ok_or(Refusal::Allocation)?,
            terminal: copy_slice(&live.coef.family_terminal).ok_or(Refusal::Allocation)?,
            history,
            z: copy_slice(&live.coef.z).ok_or(Refusal::Allocation)?,
            jp,
            class_of,
            pairs,
            offset,
            drift: DriftTemplate::new(live),
            eps,
            global,
            bytes: 0,
        };
        template.bytes = template.allocation_bytes().ok_or(Refusal::CapacityArithmetic)?;
        if template.bytes <= MAX_TEMPLATE_BYTES { Ok(Rc::new(template)) } else { Err(Refusal::Capacity) }
    }

    fn allocation_bytes(&self) -> Option<usize> {
        // Include the shared allocation header and every actual retained Vec capacity, rather than its length.
        let mut bytes = std::mem::size_of::<Self>().checked_add(2 * std::mem::size_of::<usize>())?;
        for amount in [
            vector_bytes(&self.times)?,
            vector_bytes(&self.terminal)?,
            vector_bytes(&self.history)?,
            vector_bytes(&self.z)?,
            vector_bytes(&self.jp)?,
            vector_bytes(&self.class_of)?,
            vector_bytes(&self.pairs)?,
        ] {
            bytes = bytes.checked_add(amount)?;
        }
        for row in &self.class_of {
            bytes = bytes.checked_add(vector_bytes(row)?)?;
        }
        for classes in &self.pairs {
            bytes = bytes.checked_add(vector_bytes(classes)?)?;
            for positions in classes {
                for pair in positions {
                    bytes = bytes.checked_add(vector_bytes(&pair.terms)?)?;
                }
            }
        }
        Some(bytes)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.bytes
    }

    /// None is cancellation or an unavailable optional numerical envelope; the caller distinguishes cancellation
    /// using its same cancellation predicate. A partial table is never returned or inserted in a cache.
    #[cfg(test)]
    pub(crate) fn bind(
        &self,
        members: [usize; 5],
        family: &LuckControllerFamily,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<FamilyRewardTable> {
        self.bind_with_cache(members, family, MAX_CURVE_REWARDS, MAX_CURVE_REWARD_BYTES, cancelled)
    }

    #[cfg(test)]
    fn bind_with_cache(
        &self,
        members: [usize; 5],
        family: &LuckControllerFamily,
        cache_entries: usize,
        cache_bytes: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<FamilyRewardTable> {
        let profile_count = family.profile_count();
        if family.note_times() != self.times
            || !(1..=31).contains(&profile_count)
            || family.orders().len() != profile_count.checked_mul(crate::search::uniform::ORDERS)?
        {
            return None;
        }
        let mut profile_ids = reserve(profile_count)?;
        profile_ids.extend(0..profile_count);
        let completed = self.bind_laws(
            members,
            &profile_ids,
            family.orders(),
            family.probe_gate() == Some(MISSION_LUCK),
            None,
            cache_entries,
            cache_bytes,
            cancelled,
        )?;
        let bindings = family.bindings()?;
        if bindings.profile_count() != completed.len() || cancelled() {
            return None;
        }
        Some(FamilyRewardTable { members, bindings, profiles: completed, eps: self.eps, global: self.global })
    }

    #[cfg(test)]
    pub(crate) fn start_profiles(&self, members: [usize; 5], domain: &LuckFamilyDomain) -> Option<FamilyProfileTable> {
        self.start_profile_table(members, domain.note_times(), domain.bindings()?)
    }

    pub(crate) fn start_budgeted_profiles(
        &self,
        members: [usize; 5],
        domain: &LuckFamilyProfileDomain,
    ) -> Option<FamilyProfileTable> {
        self.start_profile_table(members, domain.note_times(), domain.bindings()?)
    }

    fn start_profile_table(
        &self,
        members: [usize; 5],
        times: &[i32],
        bindings: LuckFamilyBindings,
    ) -> Option<FamilyProfileTable> {
        let count = bindings.profile_count();
        if times != self.times || !(1..=31).contains(&count) {
            return None;
        }
        let mut profiles = reserve(count)?;
        profiles.resize_with(count, || None);
        Some(FamilyProfileTable { members, bindings, profiles, eps: self.eps, global: self.global })
    }

    #[cfg(test)]
    pub(crate) fn bind_profile(
        &self,
        members: [usize; 5],
        domain: &LuckFamilyDomain,
        profile: &LuckFamilyProfile,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<FamilyProfileReward> {
        if !domain.owns_profile(profile)
            || domain.note_times() != self.times
            || profile.orders().len() != crate::search::uniform::ORDERS
        {
            return None;
        }
        self.bind_complete_profile(members, profile, domain.probe_gate(), cancelled)
    }

    pub(crate) fn bind_budgeted_profile(
        &self,
        members: [usize; 5],
        domain: &LuckFamilyProfileDomain,
        profile: &LuckFamilyProfile,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<FamilyProfileReward> {
        if !domain.owns_profile(profile)
            || domain.note_times() != self.times
            || profile.orders().len() != crate::search::uniform::ORDERS
        {
            return None;
        }
        self.bind_complete_profile(members, profile, domain.probe_gate(), cancelled)
    }

    fn bind_complete_profile(
        &self,
        members: [usize; 5],
        profile: &LuckFamilyProfile,
        probe_gate: Option<i64>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<FamilyProfileReward> {
        let mut bound = self.bind_laws(
            members,
            &[profile.profile()],
            profile.orders(),
            probe_gate == Some(MISSION_LUCK),
            Some(profile),
            MAX_CURVE_REWARDS,
            MAX_CURVE_REWARD_BYTES,
            cancelled,
        )?;
        (bound.len() == 1 && !cancelled()).then(|| bound.remove(0))
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_laws(
        &self,
        members: [usize; 5],
        profile_ids: &[usize],
        laws: &[LuckFamilyOrderLaw],
        probe_enabled: bool,
        probe_profile: Option<&LuckFamilyProfile>,
        cache_entries: usize,
        cache_bytes: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<Vec<FamilyProfileReward>> {
        let mut counts = [0usize; 5];
        for slot in 0..5 {
            counts[slot] = self.class_of.get(members[slot])?.len();
        }
        let mut profiles = reserve(profile_ids.len())?;
        for _ in profile_ids {
            let mut sums = ProfileSums::new(counts)?;
            sums.max_probe_runs = probe_profile.map(|_| 0);
            profiles.push(sums);
        }
        let n = self.times.len();
        let mut normal: [Vec<F64Interval>; 5] = std::array::from_fn(|_| Vec::new());
        let mut probe = reserve(n + 1)?;
        probe.resize(n + 1, F64Interval::ZERO);
        let mut original_probe = reserve(n + 1)?;
        original_probe.resize(n + 1, F64Interval::ZERO);
        for prefix in &mut normal {
            prefix.try_reserve_exact(n + 1).ok()?;
            prefix.resize(n + 1, F64Interval::ZERO);
        }
        let mut rewards = CurveRewardCache::new(cache_entries, cache_bytes);
        for (law_index, law) in laws.iter().enumerate() {
            if cancelled() {
                return None;
            }
            let sums = profiles.get_mut(profile_ids.iter().position(|&id| id == law.profile)?)?;
            sums.record(&law.positions)?;
            if let Some(reward) = rewards.get(law) {
                sums.add_curve(&law.positions, reward)?;
                continue;
            }
            for row in &mut normal {
                row[0] = F64Interval::ZERO;
            }
            probe[0] = F64Interval::ZERO;
            original_probe[0] = F64Interval::ZERO;
            let probe_certificates = probe_profile.and_then(|profile| profile.order_probe_certificates(law_index));
            let probe_runs = probe_certificates.map(|(runs, _)| runs);
            let rank_probe_history_ready = probe_enabled && probe_certificates.is_some_and(|(_, ready)| ready);
            for e in 0..n {
                if e.is_multiple_of(64) && cancelled() {
                    return None;
                }
                let mut weighted = F64Interval::ZERO;
                let mut weighted_probe = F64Interval::ZERO;
                let mut probe_mass = F64Interval::ZERO;
                for (bucket, mass) in law.joint_at(self.times[e]).into_iter().enumerate() {
                    let term = mass.interval().multiply(point(self.terminal[e][usize::from(bucket & 2 != 0)])?);
                    weighted = weighted.add(term);
                    if bucket & 1 != 0 {
                        weighted_probe = weighted_probe.add(term);
                        probe_mass = probe_mass.add(mass.interval());
                    }
                }
                let k = point(self.history[e])?.add(weighted).multiply(point(self.z[e])?);
                let history = point(self.history[e])?;
                // Only a historical QUERY/FILING capability permits this probability factor. The Rush
                // multiplier inside `history` stays unconditional; ordinary history and the complete
                // native error offset remain independent of this terminal/direct-probe probability.
                let probe_history = if rank_probe_history_ready {
                    history.multiply(probe_mass).intersect(F64Interval::new(0.0, self.history[e]).ok()?)?
                } else {
                    history
                };
                let kp = probe_history.add(weighted_probe).multiply(point(self.z[e])?);
                let original_kp = history.add(weighted_probe).multiply(point(self.z[e])?);
                for (j, prefix) in normal.iter_mut().enumerate() {
                    prefix[e + 1] = prefix[e].add(k.multiply(point(self.jp[e][j])?));
                }
                probe[e + 1] = probe[e].add(kp.multiply(point(self.jp[e][0])?));
                original_probe[e + 1] = original_probe[e].add(original_kp.multiply(point(self.jp[e][0])?));
            }
            let base_a0 = finite_nonnegative(normal[0][n].upper())?;
            let a0 = finite_nonnegative(normal[0][n].add(point(self.offset)?).upper())?;
            let unit_probe_history_reduction = if rank_probe_history_ready {
                // Compare the two actually used upper endpoints, not overlapping interval widths: an
                // unchanged envelope must not register a positive diagnostic discount from roundoff alone.
                finite_nonnegative((original_probe[n].upper() - probe[n].upper()).max(0.0))?
            } else {
                0.0
            };
            let original = rank_probe_history_ready.then_some(original_probe.as_slice());
            if rewards.can_store(counts) {
                let prepared = self.curve_rewards(
                    members,
                    a0,
                    base_a0,
                    probe_runs,
                    rank_probe_history_ready,
                    unit_probe_history_reduction,
                    &normal,
                    &probe,
                    original,
                    probe_enabled,
                    cancelled,
                );
                if cancelled() {
                    return None;
                }
                if let Some(reward) = prepared {
                    sums.add_curve(&law.positions, &reward)?;
                    rewards.insert(law, reward);
                    continue;
                }
                // Optional all-position work may refuse an allocation or an unused coefficient. The current
                // label still has its original direct evaluation; no cache refusal can remove a family law.
            }
            sums.a0 = sums.a0.add(point(a0)?);
            sums.base_a0 = sums.base_a0.add(point(base_a0)?);
            sums.max_probe_runs = sums.max_probe_runs.zip(probe_runs).map(|(old, runs)| old.max(runs));
            sums.rank_probe_history_ready_orders += usize::from(rank_probe_history_ready);
            sums.unit_probe_history_reduction =
                sums.unit_probe_history_reduction.add(point(unit_probe_history_reduction)?);
            for slot in 0..5 {
                let member = members[slot];
                let position = law.positions[slot];
                for (choice, &class) in self.class_of[member].iter().enumerate() {
                    let row = self.pairs.get(member)?.get(class)?.get(position)?;
                    let gain = pair_reward_with_history(row, &normal, &probe, original, probe_enabled, cancelled)?;
                    sums.gains[slot][choice] = sums.gains[slot][choice].add(point(gain)?);
                }
            }
        }
        if profiles.iter().any(|profile| !profile.complete()) || cancelled() {
            return None;
        }
        // Average each complete original-order product before a physical binding selects its controller law.
        // Retaining the profile preserves the dependence shared by its base and all five reward coefficients.
        // The native binding metadata validates physical resource identity, owner and uniqueness without
        // retaining any curve payload. No numerical similarity between laws can select a different profile.
        let mut completed = reserve(profile_ids.len())?;
        for profile in profiles {
            if cancelled() {
                return None;
            }
            completed.push(profile.finish()?);
        }
        Some(completed)
    }

    #[allow(clippy::too_many_arguments)]
    fn curve_rewards(
        &self,
        members: [usize; 5],
        a0: f64,
        base_a0: f64,
        probe_runs: Option<u64>,
        rank_probe_history_ready: bool,
        unit_probe_history_reduction: f64,
        normal: &[Vec<F64Interval>; 5],
        probe: &[F64Interval],
        original_probe: Option<&[F64Interval]>,
        probe_enabled: bool,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<CurveRewards> {
        let mut gains: [Vec<[f64; 5]>; 5] = std::array::from_fn(|_| Vec::new());
        for (slot, &member) in members.iter().enumerate() {
            let classes = self.class_of.get(member)?;
            gains[slot].try_reserve_exact(classes.len()).ok()?;
            for &class in classes {
                if cancelled() {
                    return None;
                }
                let rows = self.pairs.get(member)?.get(class)?;
                let mut values = [0.0; 5];
                for (value, row) in values.iter_mut().zip(rows) {
                    *value = pair_reward_with_history(row, normal, probe, original_probe, probe_enabled, cancelled)?;
                }
                gains[slot].push(values);
            }
        }
        Some(CurveRewards { a0, base_a0, probe_runs, rank_probe_history_ready, unit_probe_history_reduction, gains })
    }
}

impl FamilyProfileTable {
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let mut bytes = std::mem::size_of::<Self>()
            .checked_add(self.bindings.retained_bytes().checked_sub(std::mem::size_of::<LuckFamilyBindings>())?)?
            .checked_add(vector_bytes(&self.profiles)?)?;
        for profile in self.profiles.iter().flatten() {
            for row in &profile.mean {
                bytes = bytes.checked_add(vector_bytes(row)?)?;
            }
        }
        Some(bytes)
    }
}

#[cfg(test)]
mod profile_mean_tests {
    use super::*;

    fn table_bits(table: &FamilyRewardTable) -> Vec<u64> {
        let mut bits = vec![table.eps.to_bits(), table.global.to_bits()];
        for profile in &table.profiles {
            bits.push(profile.a0.to_bits());
            bits.extend(profile.mean.iter().flatten().map(|value| value.to_bits()));
        }
        bits
    }

    #[test]
    fn curve_reward_reuse_preserves_real_family_labels_and_zero_capacity_arithmetic() {
        use crate::search::expectation;
        use crate::search::joint::{JointBounds, reward_family_choices, reward_family_fixture};
        use crate::types::{Metric, SimulationInput};
        use ournotes_sim::live::full::{LuckDpCache, LuckFamilyContext, LuckFamilyLimits, luck_skills};

        let (master, owned, request) = reward_family_fixture();
        let pool = Pool::new(&master, &owned).unwrap();
        let domain = crate::domain::CandidateDomain::build(&pool, &request.constraints).unwrap();
        let bounds =
            JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
        let members = [1, 2, 0, 3, 4];
        let physical = expectation::PhysicalDeck { members, snaps: [None; 5] };
        let input = expectation::context(&pool, &physical, &request.objective).unwrap();
        let skills = luck_skills(&master).unwrap();
        let context = LuckFamilyContext::new(
            &master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            input.gekisou.as_ref().unwrap(),
            &input.play,
            &input.delta_times,
            || false,
        )
        .unwrap()
        .unwrap();
        let mut curves = LuckDpCache::new(8 * 1024 * 1024);
        let family = context
            .prepare(
                &reward_family_choices(&pool, &domain, members),
                Some(&mut curves),
                LuckFamilyLimits {
                    max_pair_models: 64,
                    max_profiles: 6,
                    max_order_evaluations: 720,
                    max_frame_work: 1_000_000,
                    max_retained_bytes: 32 * 1024 * 1024,
                },
                || false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(family.orders().len(), 120 * family.profile_count());
        let first = &family.orders()[0];
        let shared = family
            .orders()
            .iter()
            .find(|law| law.positions != first.positions && law.shares_joint_curve(first))
            .expect("distinct labelled permutations share this exact retained native curve");
        let different = family
            .orders()
            .iter()
            .find(|law| family.note_times().iter().any(|&time| law.joint_at(time) != first.joint_at(time)))
            .expect("the native writer fixture has different joint probabilities");
        assert!(!different.shares_joint_curve(first));

        let mut cache = CurveRewardCache::new(MAX_CURVE_REWARDS, MAX_CURVE_REWARD_BYTES);
        assert!(cache.can_store([0; 5]));
        cache.insert(
            first,
            CurveRewards {
                a0: 123.0,
                base_a0: 100.0,
                probe_runs: None,
                rank_probe_history_ready: false,
                unit_probe_history_reduction: 0.0,
                gains: std::array::from_fn(|_| Vec::new()),
            },
        );
        assert_eq!(cache.get(shared).unwrap().a0, 123.0);
        assert!(cache.get(different).is_none());
        assert!(cache.bytes().unwrap() <= MAX_CURVE_REWARD_BYTES);
        assert!(!CurveRewardCache::new(0, MAX_CURVE_REWARD_BYTES).can_store([0; 5]));
        assert!(!CurveRewardCache::new(MAX_CURVE_REWARDS, 0).can_store([0; 5]));

        let template = bounds.family_reward_template().unwrap();
        let cached = template.bind(members, &family, &mut || false).unwrap();
        assert_eq!(cached.profiles.len(), family.profile_count());
        for (entries, bytes) in [(0, MAX_CURVE_REWARD_BYTES), (MAX_CURVE_REWARDS, 0), (1, 256)] {
            let direct = template.bind_with_cache(members, &family, entries, bytes, &mut || false).unwrap();
            assert_eq!(table_bits(&cached), table_bits(&direct), "capacity changes only reward recomputation");
        }
        let mut checks = 0;
        assert!(
            template
                .bind(members, &family, &mut || {
                    checks += 1;
                    checks >= 80
                })
                .is_none(),
            "cancellation cannot publish a partially accumulated label set"
        );
        assert_eq!(table_bits(&cached), table_bits(&template.bind(members, &family, &mut || false).unwrap()));
    }

    #[test]
    fn complete_profiles_preserve_distinct_correlated_order_means() {
        let orders = crate::search::uniform::all_orders();
        let mut profiles = [ProfileSums::new([1; 5]).unwrap(), ProfileSums::new([1; 5]).unwrap()];
        let mut exact_base = [0u64; 2];
        let mut exact_gain = [0u64; 2];
        let mut old_position = [[0.0f64; 5]; 2];
        for order in &orders {
            let positions = crate::search::uniform::positions_of(order);
            for profile in 0..2 {
                let base = if profile == 0 { 10 + 5 * u64::from(positions[2] == 0) } else { 12 };
                let gain = if profile == 0 {
                    120 * u64::from(positions[0] < positions[1])
                } else {
                    120 * u64::from(positions[0] > positions[1])
                };
                let sums = &mut profiles[profile];
                sums.record(&positions).unwrap();
                sums.a0 = sums.a0.add(F64Interval::integer(base.into()));
                sums.gains[0][0] = sums.gains[0][0].add(F64Interval::integer(gain.into()));
                exact_base[profile] += base;
                exact_gain[profile] += gain;
                old_position[profile][positions[0]] = old_position[profile][positions[0]].max(gain as f64);
            }
        }
        assert_eq!(exact_base, [1320, 1440]);
        assert_eq!(exact_gain, [7200, 7200]);
        let mut completed = Vec::new();
        for (profile, sums) in profiles.into_iter().enumerate() {
            assert!(sums.complete());
            let result = sums.finish().unwrap();
            assert!(result.a0 >= exact_base[profile] as f64 / 120.0);
            assert!(result.mean[0][0] >= exact_gain[profile] as f64 / 120.0);
            completed.push(result);
        }
        // The selected physical profile keeps its own base; taking componentwise profile maxima would lose
        // this distinction even though each profile has already covered the whole 120-label shuffle.
        assert!((11.0..11.000001).contains(&completed[0].a0));
        assert!((12.0..12.000001).contains(&completed[1].a0));
        // Both individual laws are nonconstant and complementary. Moving the maximum inside the order mean
        // would choose a different physical writer profile on each shuffle and produce 120 instead of 60.
        let old = std::array::from_fn(|position| old_position[0][position].max(old_position[1][position]));
        assert!(crate::search::uniform::mean_up(&old) > 119.0);
        for profile in completed {
            assert!((60.0..60.000001).contains(&profile.mean[0][0]));
            assert!(profile.mean[0][0] < crate::search::uniform::mean_up(&old));
        }
    }

    #[test]
    fn mean_divisor_requires_each_original_order_once_per_profile() {
        let orders = crate::search::uniform::all_orders();
        let mut partial = ProfileSums::new([0; 5]).unwrap();
        for order in orders.iter().take(119) {
            partial.record(&crate::search::uniform::positions_of(order)).unwrap();
        }
        assert!(!partial.complete());
        assert!(partial.record(&crate::search::uniform::positions_of(&orders[0])).is_none());
        assert!(!partial.complete());
        partial.record(&crate::search::uniform::positions_of(&orders[119])).unwrap();
        assert!(partial.complete());
        assert!(partial.record(&[0, 1, 1, 3, 4]).is_none());
        assert!(partial.record(&[0, 1, 2, 3, 5]).is_none());
        let untouched_profile = ProfileSums::new([0; 5]).unwrap();
        assert!(!untouched_profile.complete());
        assert!(untouched_profile.finish().is_none());
        let mut missing_last = ProfileSums::new([0; 5]).unwrap();
        for order in orders.iter().take(119) {
            missing_last.record(&crate::search::uniform::positions_of(order)).unwrap();
        }
        assert!(missing_last.finish().is_none());
        assert!(partial.finish().is_some());
        assert!(uniform_upper(F64Interval::WHOLE).is_none());
    }
}
