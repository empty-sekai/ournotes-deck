//! Maximum-score relaxation for ascending nonleader member IDs.
use super::*;

const MAX_BYTES: usize = 16 * 1024 * 1024;

struct Suffix {
    after: i64,
    characters: Vec<i64>,
    tables: relax_tables::RelaxTables,
}

/// Each suffix retains every member whose ID is greater than its exact boundary.
/// Tables relax remaining choices while retaining character and Snap exclusions.
pub(crate) struct MaximumIdPrefixes {
    suffixes: Vec<Suffix>,
    #[cfg(feature = "search-diagnostics")]
    estimated_bytes: usize,
}

impl MaximumIdPrefixes {
    /// Conservative storage bound, including the fixed-size top-five rows and vector headers.
    fn storage(members: usize, characters: usize, profiles: usize, snaps: usize) -> Option<usize> {
        let per = 512usize
            .checked_add(64usize.checked_mul(profiles)?)?
            .checked_add(112usize.checked_mul(profiles)?.checked_mul(characters)?)?
            .checked_add(704usize.checked_mul(characters)?)?
            .checked_add(640usize.checked_mul(snaps)?)?
            .checked_add(16usize.checked_mul(members)?)?;
        members.checked_mul(per)
    }

    pub(super) fn compile(
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        mut stop: impl FnMut() -> bool,
    ) -> Option<Self> {
        if !b.maximum_carrier_score || b.points.is_some() || b.rules.is_some() {
            return None;
        }
        let characters = domain.members().iter().map(|&m| pool.members[m].character_id).collect::<HashSet<_>>().len();
        let estimated_bytes = Self::storage(domain.members().len(), characters, b.lead.len(), domain.snaps().len())?;
        if estimated_bytes > MAX_BYTES {
            return None;
        }
        let mut suffixes = Vec::with_capacity(domain.members().len());
        for &member in domain.members() {
            if stop() {
                return None;
            }
            let after = pool.members[member].id;
            let mut characters: Vec<_> = domain
                .members()
                .iter()
                .filter(|&&m| pool.members[m].id > after)
                .map(|&m| pool.members[m].character_id)
                .collect();
            characters.sort_unstable();
            characters.dedup();
            let tables = relax_tables::RelaxTables::compile_where(
                b,
                pool,
                domain,
                &|m, _| pool.members[m].id > after,
                &|m, choice| b.gains[m][choice],
            )?;
            suffixes.push(Suffix { after, characters, tables });
        }
        suffixes.sort_unstable_by_key(|suffix| suffix.after);
        Some(Self {
            suffixes,
            #[cfg(feature = "search-diagnostics")]
            estimated_bytes,
        })
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn table_count(&self) -> usize {
        self.suffixes.len()
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    /// Every completion considered here has the declared prefix and all remaining IDs above `after`.
    /// The leader imposes no ID restriction on the first nonleader, so depth one has no suffix cap.
    pub(super) fn upper(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        after: Option<i64>,
    ) -> Option<(i128, i64)> {
        if !(2..5).contains(&depth) {
            return None;
        }
        let after = after?;
        if pool.members[p.members[SLOTS[depth - 1]]].id != after {
            return None;
        }
        let index = self.suffixes.binary_search_by_key(&after, |suffix| suffix.after).ok()?;
        let suffix = &self.suffixes[index];
        let selected = &SLOTS[..depth];
        let free = 5 - depth;
        let available = suffix
            .characters
            .iter()
            .filter(|&&character| !selected.iter().any(|&slot| pool.members[p.members[slot]].character_id == character))
            .count();
        if available < free {
            return Some((i128::MIN, 0));
        }
        // Only the suffix table's own character indexes are used with its taken mask.
        // Forced slot rules are relaxed to every choice; no foreign table reads this mask.
        let (taken_characters, taken_snaps) = suffix.tables.taken(pool, domain, p, depth);
        let profile = b.profile[p.members[2]];
        let (mut power, mut gain) = (0i64, 0.0);
        for &slot in selected {
            let member = p.members[slot];
            let choice =
                p.snaps[slot].map(|snap| domain.snaps().iter().position(|&s| s == snap).expect("compiled Snap"));
            power += b.a[member] + b.lead[profile][member] + choice.map_or(0, |j| b.w[member][j]);
            gain = add_up(gain, b.gains[member][choice.map_or(0, |j| j + 1)][slot]);
        }
        let (free_power, free_gain, _) =
            suffix.tables.free_part(b, profile, &SLOTS[depth..], &[0, 1, 2, 3, 4], &taken_characters, &taken_snaps);
        power += free_power;
        gain = add_up(gain, free_gain);
        // Pool-wide spread and placement columns still enclose every remaining order.
        let (spread, placement) = b.order_gain_bounds(domain, p, depth, free, None);
        gain = add_up(gain, spread).min(order_gain_bound(&placement, &b.column, 0));
        let cap = ((power as f64) * add_up(b.a0, gain).min(b.global) * (1.0 + b.eps)).ceil() as i128;
        Some((cap, power))
    }
}

impl JointBounds {
    pub(crate) fn maximum_id_prefixes(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        stop: impl FnMut() -> bool,
    ) -> Option<MaximumIdPrefixes> {
        MaximumIdPrefixes::compile(self, pool, domain, stop)
    }

    pub(crate) fn maximum_id_upper(
        &self,
        tables: &MaximumIdPrefixes,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        after: Option<i64>,
    ) -> Option<(i128, i64)> {
        tables.upper(self, pool, domain, p, depth, after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_id_prefix_storage_admits_small_domains_and_checks_capacity() {
        assert!(MaximumIdPrefixes::storage(44, 44, 44, 42).unwrap() < MAX_BYTES);
        assert!(MaximumIdPrefixes::storage(1000, 1000, 1000, 1000).unwrap() > MAX_BYTES);
        assert!(MaximumIdPrefixes::storage(usize::MAX, 5, 5, 5).is_none());
    }
}
