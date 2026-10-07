//! Uniform expected-score envelopes of an admitted fixed-member controller family.
//!
//! These coefficients are exclusions, never candidate values. The native family owns the complete writer-profile
//! and original-order cover and the common terminal-query mapping. All ordinary history, rank, conversion-budget
//! and floating-point allowances remain those of the original complete-domain envelope.

use super::*;
use ournotes_sim::live::{certified::F64Interval, full::LuckControllerFamily};

const MAX_TEMPLATE_BYTES: usize = 8 * 1024 * 1024;

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
    /// Only the positive per-note arithmetic chain, from additive_joint_envelope. Legacy relative drift is
    /// never accepted by this template.
    eps: f64,
    global: f64,
    bytes: usize,
}

/// One completed envelope covers the five members with every Snap choice of the template and every original
/// performance order. Each component averages all original orders within a fixed physical writer profile,
/// then takes its maximum over complete profiles. A profile never changes with the performance order.
pub(crate) struct FamilyRewardTable {
    pub(crate) members: [usize; 5],
    pub(crate) a0: f64,
    pub(crate) mean: [Vec<f64>; 5],
    pub(crate) eps: f64,
    pub(crate) global: f64,
}

/// Outward coefficient sums under one fixed physical writer profile. The complete native capability already
/// certifies the labels; this local cover also prevents a missing or repeated label from changing the divisor.
struct ProfileSums {
    seen: [u64; 2],
    a0: F64Interval,
    gains: [Vec<F64Interval>; 5],
}

impl ProfileSums {
    fn new(counts: [usize; 5]) -> Option<Self> {
        let mut gains: [Vec<F64Interval>; 5] = std::array::from_fn(|_| Vec::new());
        for (row, count) in gains.iter_mut().zip(counts) {
            row.try_reserve_exact(count).ok()?;
            row.resize(count, F64Interval::ZERO);
        }
        Some(Self { seen: [0; 2], a0: F64Interval::ZERO, gains })
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

impl ProfileRewardTemplate {
    pub(crate) fn compile(live: &SnapLive<'_>) -> Option<Rc<Self>> {
        // additive_joint_envelope charges delta * B using the original complete-domain command count and
        // all-Rush/rank/judgement sensitivity. Its third result is only the relative arithmetic chain. Requiring
        // this representation prevents shrinking an absolute history error by multiplying a smaller mean gain.
        let (with_offset, global, eps) = live.joint_additive?;
        if !live.terminal_caps_admitted || !live.fine.rush_eligible || live.fine.network_ranking {
            return None;
        }
        let n = live.coef.times.len();
        if n == 0 || live.coef.family_terminal.len() != n || live.fine.rank.len() != n {
            return None;
        }
        let offset = point(with_offset)?.subtract(point(live.a0)?).upper();
        finite_nonnegative(offset)?;
        let minimum_bytes = std::mem::size_of::<Self>()
            .checked_add(2 * std::mem::size_of::<usize>())?
            .checked_add(n.checked_mul(std::mem::size_of::<i32>() + 9 * std::mem::size_of::<f64>())?)?;
        if minimum_bytes > MAX_TEMPLATE_BYTES {
            return None;
        }
        let mut history = reserve(n)?;
        let mut jp = reserve(n)?;
        for e in 0..n {
            let [plain, rush] = live.coef.family_terminal[e];
            if plain > rush || live.fine.rank[e] < 1.0 {
                return None;
            }
            point(plain)?;
            let extra = point(live.fine.rank[e])?.subtract(F64Interval::ONE);
            history.push(finite_nonnegative(point(rush)?.multiply(extra).upper())?);
            point(live.coef.z[e])?;
            let mut row = [live.coef.max_jp[e]; 5];
            row[1..].copy_from_slice(&live.coef.jp[e]);
            for value in row {
                point(value)?;
            }
            jp.push(row);
        }
        let mut pairs = reserve(live.contrib.len())?;
        let mut estimate = minimum_bytes;
        for classes in &live.contrib {
            let mut rows = reserve(classes.len())?;
            for positions in classes {
                let mut built = reserve(5)?;
                for contribution in positions {
                    let opaque = contribution.windows.iter().any(|w| w.ramp != 0).then_some(contribution.gain);
                    if let Some(gain) = opaque {
                        point(gain)?;
                    }
                    let mut terms = reserve(if opaque.is_some() { 0 } else { contribution.windows.len() })?;
                    if opaque.is_none() {
                        for w in &contribution.windows {
                            if w.lo > w.hi || w.hi as usize > n {
                                return None;
                            }
                            point(w.note)?;
                            for value in w.judge {
                                point(value)?;
                            }
                            let probe = w
                                .rush
                                .checked_sub(1)
                                .and_then(|i| contribution.rush.get(i as usize))
                                .is_some_and(|r| r.terminal_probe(Some(MISSION_LUCK)));
                            if probe && w.judge != [0.0; 4] {
                                return None;
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
                    point(contribution.budget)?;
                    estimate = estimate
                        .checked_add(std::mem::size_of::<Pair>())?
                        .checked_add(terms.capacity().checked_mul(std::mem::size_of::<Term>())?)?;
                    if estimate > MAX_TEMPLATE_BYTES {
                        return None;
                    }
                    built.push(Pair { terms, budget: contribution.budget, opaque });
                }
                rows.push(built.try_into().ok()?);
            }
            estimate = estimate.checked_add(std::mem::size_of::<Vec<[Pair; 5]>>())?;
            pairs.push(rows);
        }
        let mut class_of = reserve(live.class_of.len())?;
        for classes in &live.class_of {
            let mut row = reserve(classes.len().checked_add(1)?)?;
            row.push(0);
            row.extend(classes.iter().map(|&class| usize::from(class)));
            estimate = estimate
                .checked_add(std::mem::size_of::<Vec<usize>>())?
                .checked_add(row.capacity().checked_mul(std::mem::size_of::<usize>())?)?;
            class_of.push(row);
        }
        if estimate > MAX_TEMPLATE_BYTES {
            return None;
        }
        let mut template = Self {
            times: copy_slice(&live.coef.times)?,
            terminal: copy_slice(&live.coef.family_terminal)?,
            history,
            z: copy_slice(&live.coef.z)?,
            jp,
            class_of,
            pairs,
            offset,
            eps,
            global,
            bytes: 0,
        };
        template.bytes = template.allocation_bytes()?;
        (template.bytes <= MAX_TEMPLATE_BYTES).then(|| Rc::new(template))
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
    pub(crate) fn bind(
        &self,
        members: [usize; 5],
        family: &LuckControllerFamily,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<FamilyRewardTable> {
        let profile_count = family.profile_count();
        if family.note_times() != self.times
            || !(1..=6).contains(&profile_count)
            || family.orders().len() != profile_count.checked_mul(crate::search::uniform::ORDERS)?
        {
            return None;
        }
        let probe_enabled = family.probe_gate() == Some(MISSION_LUCK);
        let mut counts = [0usize; 5];
        for slot in 0..5 {
            counts[slot] = self.class_of.get(members[slot])?.len();
        }
        let mut profiles = reserve(profile_count)?;
        for _ in 0..profile_count {
            profiles.push(ProfileSums::new(counts)?);
        }
        let n = self.times.len();
        let mut normal: [Vec<F64Interval>; 5] = std::array::from_fn(|_| Vec::new());
        let mut probe = reserve(n + 1)?;
        probe.resize(n + 1, F64Interval::ZERO);
        for prefix in &mut normal {
            prefix.try_reserve_exact(n + 1).ok()?;
            prefix.resize(n + 1, F64Interval::ZERO);
        }
        for law in family.orders() {
            if cancelled() {
                return None;
            }
            let sums = profiles.get_mut(law.profile)?;
            sums.record(&law.positions)?;
            for row in &mut normal {
                row[0] = F64Interval::ZERO;
            }
            probe[0] = F64Interval::ZERO;
            for e in 0..n {
                if e.is_multiple_of(64) && cancelled() {
                    return None;
                }
                let mut weighted = F64Interval::ZERO;
                let mut weighted_probe = F64Interval::ZERO;
                for (bucket, mass) in law.joint_at(self.times[e]).into_iter().enumerate() {
                    let term = mass.interval().multiply(point(self.terminal[e][usize::from(bucket & 2 != 0)])?);
                    weighted = weighted.add(term);
                    if bucket & 1 != 0 {
                        weighted_probe = weighted_probe.add(term);
                    }
                }
                let k = point(self.history[e])?.add(weighted).multiply(point(self.z[e])?);
                let kp = point(self.history[e])?.add(weighted_probe).multiply(point(self.z[e])?);
                for (j, prefix) in normal.iter_mut().enumerate() {
                    prefix[e + 1] = prefix[e].add(k.multiply(point(self.jp[e][j])?));
                }
                probe[e + 1] = probe[e].add(kp.multiply(point(self.jp[e][0])?));
            }
            sums.a0 = sums.a0.add(point(finite_nonnegative(normal[0][n].add(point(self.offset)?).upper())?)?);
            for slot in 0..5 {
                let member = members[slot];
                let position = law.positions[slot];
                for (choice, &class) in self.class_of[member].iter().enumerate() {
                    let row = self.pairs.get(member)?.get(class)?.get(position)?;
                    let gain = if let Some(gain) = row.opaque {
                        gain
                    } else {
                        let mut gain = point(row.budget)?;
                        for term in &row.terms {
                            let note = if probe_enabled && term.probe { &probe } else { &normal[0] };
                            gain = gain.add(note[term.hi].subtract(note[term.lo]).multiply(point(term.note)?));
                            for j in 0..4 {
                                if term.judge[j] != 0.0 {
                                    gain = gain.add(
                                        normal[j + 1][term.hi]
                                            .subtract(normal[j + 1][term.lo])
                                            .multiply(point(term.judge[j])?),
                                    );
                                }
                            }
                        }
                        finite_nonnegative(gain.upper().max(0.0))?
                    };
                    sums.gains[slot][choice] = sums.gains[slot][choice].add(point(gain)?);
                }
            }
        }
        if profiles.iter().any(|profile| !profile.complete()) || cancelled() {
            return None;
        }
        let mut mean: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
        for slot in 0..5 {
            mean[slot].try_reserve_exact(counts[slot]).ok()?;
            mean[slot].resize(counts[slot], 0.0);
        }
        // A physical binding chooses its writer profile before the shuffle. Linearity therefore permits an
        // average over all 120 original orders inside that profile. Taking component maxima only after these
        // averages still covers every binding, without choosing a different profile for each order. Power and
        // the unconditional absolute drift/history allowances stay fixed throughout this Score-only bound.
        let mut a0 = 0.0f64;
        for profile in &profiles {
            if cancelled() {
                return None;
            }
            a0 = a0.max(uniform_upper(profile.a0)?);
            for slot in 0..5 {
                for choice in 0..counts[slot] {
                    mean[slot][choice] = mean[slot][choice].max(uniform_upper(profile.gains[slot][choice])?);
                }
            }
        }
        Some(FamilyRewardTable { members, a0, mean, eps: self.eps, global: self.global })
    }
}

impl FamilyRewardTable {
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.mean.iter().try_fold(std::mem::size_of::<Self>(), |bytes, row| {
            bytes.checked_add(row.capacity().checked_mul(std::mem::size_of::<f64>())?)
        })
    }
}

#[cfg(test)]
mod profile_mean_tests {
    use super::*;

    #[test]
    fn complete_profiles_average_correlated_orders_before_component_maxima() {
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
        let mut base = 0.0f64;
        let mut gain = 0.0f64;
        for (profile, sums) in profiles.iter().enumerate() {
            assert!(sums.complete());
            let b = uniform_upper(sums.a0).unwrap();
            let g = uniform_upper(sums.gains[0][0]).unwrap();
            assert!(b >= exact_base[profile] as f64 / 120.0);
            assert!(g >= exact_gain[profile] as f64 / 120.0);
            base = base.max(b);
            gain = gain.max(g);
        }
        // Both individual laws are nonconstant and complementary. Moving the maximum inside the order mean
        // would choose a different physical writer profile on each shuffle and produce 120 instead of 60.
        assert!((12.0..12.000001).contains(&base));
        assert!((60.0..60.000001).contains(&gain));
        let old = std::array::from_fn(|position| old_position[0][position].max(old_position[1][position]));
        assert!(crate::search::uniform::mean_up(&old) > 119.0);
        assert!(gain < crate::search::uniform::mean_up(&old));
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
        assert!(uniform_upper(F64Interval::WHOLE).is_none());
    }
}
