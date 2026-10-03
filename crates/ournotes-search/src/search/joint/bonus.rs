//! Score envelopes conditioned on an exact event-bonus sum.
//! Residual characters are distinct. Snap reuse and remaining positions are relaxed.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default)]
struct Row {
    power: i64,
    gain: f64,
    weighted: [f64; 3],
}
impl Row {
    fn merge(&mut self, other: Self) {
        self.power = self.power.max(other.power);
        self.gain = self.gain.max(other.gain);
        for i in 0..3 {
            self.weighted[i] = self.weighted[i].max(other.weighted[i]);
        }
    }
    fn plus(self, other: Self) -> Self {
        Self {
            power: self.power + other.power,
            gain: add_up(self.gain, other.gain),
            weighted: std::array::from_fn(|i| add_up(self.weighted[i], other.weighted[i])),
        }
    }
}
#[derive(Hash, PartialEq, Eq)]
struct Key {
    profile: usize,
    excluded: Vec<i64>,
    positions: u8,
}

/// Scratch belongs to one compiled JointBounds/domain. Capacity limits disable an
/// optional bound; they never remove a DP state and then claim the result is a cap.
#[derive(Default)]
pub(crate) struct BonusScratch {
    rows: HashMap<Key, Option<Vec<(i64, Row)>>>,
    cells: usize,
    /// Telemetry: row states built, lookups reaching a stored state (including the re-entry after each build) and
    /// lookups refused by the capacity limits.
    built: u64,
    found: u64,
    refused: u64,
}
const MAX_CELLS: usize = 65_536;
const MAX_STATES: usize = 2_048;

impl BonusScratch {
    /// Row-cache use and the lookups refused at capacity.
    pub(crate) fn cache_use(&self) -> (crate::search::telemetry::CacheUse, u64) {
        let hits = self.found - self.built;
        let usage = crate::search::telemetry::CacheUse {
            lookups: hits + self.built + self.refused,
            hits,
            evictions: 0,
            peak_entries: self.rows.len(),
        };
        (usage, self.refused)
    }
}

impl JointBounds {
    fn bonus_rows(&self, pool: &Pool, domain: &CandidateDomain, key: &Key) -> Option<Vec<(i64, Row)>> {
        let pt = self.points.as_ref()?;
        let mut groups = BTreeMap::<i64, BTreeMap<i64, Row>>::new();
        for &m in domain.members() {
            let character = pool.members[m].character_id;
            if key.excluded.binary_search(&character).is_ok() {
                continue;
            }
            let group = groups.entry(character).or_default();
            for choice in 0..=domain.snaps().len() {
                let power = self.a[m] + self.lead[key.profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
                let gain = (0..5)
                    .filter(|&pos| key.positions & (1 << pos) != 0)
                    .map(|pos| self.gains[m][choice][pos])
                    .fold(0.0, f64::max);
                let row = Row {
                    power,
                    gain,
                    weighted: self.correlation_scales.map(|r| add_up(power as f64, (r * gain).next_up())),
                };
                let bonus = pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] };
                group.entry(bonus).or_default().merge(row);
            }
        }
        let count = key.positions.count_ones() as usize;
        let mut dp: Vec<BTreeMap<i64, Row>> = (0..=count).map(|_| BTreeMap::new()).collect();
        dp[0].insert(0, Row::default());
        for group in groups.values() {
            for n in (1..=count).rev() {
                let (before, after) = dp.split_at_mut(n);
                if before[n - 1].len().saturating_mul(group.len()) > 1_000_000 {
                    return None;
                }
                for (&b, &v) in &before[n - 1] {
                    for (&extra, &r) in group {
                        after[0].entry(b + extra).or_default().merge(v.plus(r));
                    }
                }
                if after[0].len() > MAX_STATES {
                    return None;
                }
            }
        }
        Some(dp.pop()?.into_iter().collect())
    }

    #[allow(clippy::too_many_arguments)]
    fn bonus_caps(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
        scratch: &mut BonusScratch,
    ) -> Option<Vec<(i64, i128, i64)>> {
        let pt = self.points.as_ref()?;
        if !(1..5).contains(&depth) || pt.score_tiers.is_none() {
            return None;
        }
        let profile = self.profile[p.members[2]];
        let mut excluded = Vec::new();
        let mut fixed = Row::default();
        let mut bonus = 0;
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            excluded.push(pool.members[m].character_id);
            let choice =
                p.snaps[slot].map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
            let power = self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
            let gain = self.gains[m][choice][positions[slot]];
            fixed = fixed.plus(Row {
                power,
                gain,
                weighted: self.correlation_scales.map(|r| add_up(power as f64, (r * gain).next_up())),
            });
            bonus += pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] };
        }
        excluded.sort_unstable();
        let key = Key {
            profile,
            excluded,
            positions: SLOTS[depth..].iter().fold(0, |mask, &slot| mask | (1 << positions[slot])),
        };
        if !scratch.rows.contains_key(&key) {
            if scratch.cells >= MAX_CELLS || scratch.rows.len() >= MAX_STATES {
                scratch.refused += 1;
                return None;
            }
            let rows = self.bonus_rows(pool, domain, &key);
            scratch.cells += rows.as_ref().map_or(0, Vec::len);
            scratch.built += 1;
            scratch.rows.insert(key, rows);
            // The key has moved; re-enter using the now populated cache.
            return self.bonus_caps(pool, domain, p, depth, positions, scratch);
        }
        scratch.found += 1;
        let rows = scratch.rows.get(&key)?.as_ref()?;
        let mut caps = Vec::with_capacity(rows.len());
        for &(extra, residual) in rows {
            let row = fixed.plus(residual);
            let mut score =
                ((row.power as f64) * add_up(self.a0, row.gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
            for (i, &r) in self.correlation_scales.iter().enumerate() {
                let w = add_up(row.weighted[i], (r * self.a0).next_up());
                let cap =
                    (((w * w).next_up() / (4.0 * r)).next_up() * (1.0 + self.eps).next_up()).next_up().ceil() as i128;
                score = score.min(cap);
            }
            caps.push((bonus + extra, ((10000 + bonus + extra) * pt.multiplier_at(score) / 10000) as i128, row.power));
        }
        Some(caps)
    }

    #[cfg(feature = "search-diagnostics")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bonus_upper_at(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
        scratch: &mut BonusScratch,
    ) -> Option<i128> {
        self.bonus_caps(pool, domain, p, depth, positions, scratch)?.into_iter().map(|(_, v, _)| v).max()
    }

    #[cfg(feature = "search-diagnostics")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bonus_expected_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        scratch: &mut BonusScratch,
    ) -> Result<Option<i128>, Error> {
        Ok(self.bonus_expected_upper_with_power(pool, domain, p, depth, orders, scratch)?.map(|(cap, _)| cap))
    }

    /// The bonus cap, and a power cap for the completions that can reach it: every completion with an exact event
    /// bonus has at most that bonus cell's expected payoff and residual power, so a completion whose payoff reaches
    /// the cap lies in a cell whose total is the cap. A tie with the Top-K cutoff then needs that power.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bonus_expected_upper_with_power(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        scratch: &mut BonusScratch,
    ) -> Result<Option<(i128, i64)>, Error> {
        // Physical event bonus is shared by every root. Sum root caps within the
        // same bonus cell before maximizing, rather than choosing a new bonus per root.
        let mut totals = BTreeMap::<i64, (i128, i64)>::new();
        for (positions, weight) in orders {
            let Some(caps) = self.bonus_caps(pool, domain, p, depth, positions, scratch) else {
                return Ok(None);
            };
            for (bonus, cap, power) in caps {
                let (total, cell_power) = totals.entry(bonus).or_insert((0, i64::MIN));
                *cell_power = (*cell_power).max(power);
                *total = total
                    .checked_add(
                        cap.checked_mul(i128::try_from(*weight).map_err(|_| unavailable("bonus bound mass overflow"))?)
                            .ok_or_else(|| unavailable("bonus bound product overflow"))?,
                    )
                    .ok_or_else(|| unavailable("bonus bound sum overflow"))?;
            }
        }
        let Some(cap) = totals.values().map(|v| v.0).max() else { return Ok(None) };
        let power = totals.values().filter(|v| v.0 == cap).map(|v| v.1).max().expect("a cell reaches the cap");
        Ok(Some((cap, power)))
    }
}
