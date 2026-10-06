//! Unique-resource correlated bounds before the remaining members are selected.
use super::*;

#[derive(Clone, Copy)]
struct Edge {
    character: i64,
    value: i64,
}

/// At most four characters are occupied at an unfinished prefix. Five distinct
/// maxima therefore retain the exact best unoccupied character for this edge.
#[derive(Clone, Default)]
struct TopCharacters([Option<Edge>; 5]);
impl TopCharacters {
    fn insert(&mut self, character: i64, value: i64) {
        // The entries stay sorted by value, so an edge no better than the last one changes nothing: the character's
        // own entry, if any, is at least as good, and otherwise the last entry would stay.
        if self.0[4].is_some_and(|e| e.value >= value) {
            return;
        }
        let at = self.0.iter().position(|e| e.is_some_and(|e| e.character == character)).unwrap_or(4);
        if self.0[at].is_some_and(|e| e.value >= value) {
            return;
        }
        self.0[at] = Some(Edge { character, value });
        self.0.sort_unstable_by(|a, b| b.map(|e| e.value).cmp(&a.map(|e| e.value)));
    }
    fn best(&self, occupied: &[i64]) -> Option<i64> {
        self.0.iter().flatten().find(|e| !occupied.contains(&e.character)).map(|e| e.value)
    }
}

/// Identical free-slot rows: `base` is the repeatable None choice, and each value belongs to one distinct
/// available Snap. An optimum uses at most `count` positive increments over `base`, the largest ones available.
fn shared_value(base: i64, count: usize, values: impl Iterator<Item = i64>) -> Option<i64> {
    if count == 0 {
        return Some(0);
    }
    let mut increments = super::relax_tables::Largest::new(count, 0i64);
    for value in values {
        if value > base {
            increments.push(value.checked_sub(base)?);
        }
    }
    let baseline = base.checked_mul(i64::try_from(count).ok()?)?;
    increments.values().iter().try_fold(baseline, |sum, &increment| sum.checked_add(increment))
}

pub(super) struct PrefixResourceTables {
    /// Position-mean gains give every position the same rows, indexed by profile, scale and choice.
    rows: Vec<TopCharacters>,
    choices: usize,
}
impl PrefixResourceTables {
    /// Empty rows for the domain, None past the storage gate; `prefix_character::compile_prefix_tables` fills them.
    pub(super) fn empty(b: &JointBounds, domain: &CandidateDomain) -> Option<Self> {
        let choices = domain.snaps().len() + 1;
        // Bounded optional storage for the domain choices.
        if choices > 4097 {
            return None;
        }
        let cells = b.lead.len().checked_mul(3)?.checked_mul(choices)?;
        if cells > 250_000 {
            return None;
        }
        Some(Self { rows: vec![TopCharacters::default(); cells], choices })
    }
    /// Offers the quantized edge `q` of a member of `character` with `choice` at (profile, scale).
    #[inline]
    pub(super) fn insert(&mut self, profile: usize, scale: usize, choice: usize, character: i64, q: i64) {
        self.rows[(profile * 3 + scale) * self.choices + choice].insert(character, q);
    }
    fn row(&self, profile: usize, scale: usize, choice: usize) -> &TopCharacters {
        &self.rows[(profile * 3 + scale) * self.choices + choice]
    }
    fn upper(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
    ) -> Option<i128> {
        if !(1..5).contains(&depth) {
            return None;
        }
        let profile = b.profile[p.members[2]];
        let occupied: Vec<_> = SLOTS[..depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let mut used = vec![false; self.choices - 1];
        let mut chosen = [0usize; 5];
        for &slot in &SLOTS[..depth] {
            if let Some(snap) = p.snaps[slot] {
                let j = domain.snaps().iter().position(|&s| s == snap).expect("compiled Snap");
                used[j] = true;
                chosen[slot] = j + 1;
            }
        }
        let mut best = i128::MAX;
        for (scale, &r) in b.correlation_scales.iter().enumerate() {
            let mut fixed = 0i64;
            for &slot in &SLOTS[..depth] {
                let m = p.members[slot];
                let choice = chosen[slot];
                let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                fixed += quantized(power, b.gains[m][choice][positions[slot]], r)?;
            }
            let base = self.row(profile, scale, 0).best(&occupied)?;
            let values = used
                .iter()
                .enumerate()
                .filter_map(|(j, &taken)| (!taken).then(|| self.row(profile, scale, j + 1).best(&occupied)).flatten());
            let extra = shared_value(base, 5 - depth, values)?;
            let sum = fixed + extra;
            let upper = add_up(sum as f64, (r * b.a0).next_up());
            let cap =
                (((upper * upper).next_up() / (4.0 * r)).next_up() * (1.0 + b.eps).next_up()).next_up().ceil() as i128;
            best = best.min(cap);
        }
        (best < i128::MAX).then_some(best)
    }
}
pub(super) fn quantized(power: i64, gain: f64, r: f64) -> Option<i64> {
    let q = add_up(power as f64, (r * gain).next_up()).ceil();
    (power >= 0 && gain >= 0.0 && q.is_finite() && (0.0..=1e15).contains(&q)).then_some(q as i64)
}

impl JointBounds {
    pub(crate) fn resource_prefix_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
    ) -> Option<i128> {
        self.prefix_resource.as_ref()?.upper(self, pool, domain, p, depth, positions)
    }
    pub(crate) fn resource_expected_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
    ) -> Option<i128> {
        let mut total = 0i128;
        for (positions, mass) in orders {
            total = total.checked_add(
                self.resource_prefix_upper(pool, domain, p, depth, positions)?
                    .checked_mul(i128::try_from(*mass).ok()?)?,
            )?;
        }
        Some(total)
    }
    pub(crate) fn resource_worthwhile(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        orders: &[([usize; 5], u128)],
        correlated_enabled: bool,
    ) -> Result<bool, Error> {
        if self.prefix_resource.is_none() {
            return Ok(false);
        }
        let mut probes = 0;
        for &(m, choice) in &self.choices {
            if domain.leader().is_some_and(|l| l != m) {
                continue;
            }
            let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
            p.members[2] = m;
            p.snaps[2] = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
            let mut preceding = self.expected_upper(pool, domain, &p, 1, orders)?.0;
            if correlated_enabled {
                preceding = preceding.min(self.correlated_expected_upper(pool, domain, &p, 1, orders)?);
            }
            // Measure additional tightening beyond the stages the search will
            // already run. This selects optional work only; oracle audits still
            // check the resource cap independently of this preparation probe.
            if self
                .resource_expected_upper(pool, domain, &p, 1, orders)
                .is_some_and(|cap| (cap as f64) < preceding as f64 * 0.97)
            {
                return Ok(true);
            }
            probes += 1;
            if probes == 16 {
                break;
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{TopCharacters, shared_value};

    fn exhaustive_shared(base: i64, values: &[Option<i64>], count: usize, used: usize) -> i64 {
        if count == 0 {
            return 0;
        }
        let mut best = base + exhaustive_shared(base, values, count - 1, used);
        for (choice, &value) in values.iter().enumerate() {
            if let Some(value) = value
                && used & (1 << choice) == 0
            {
                best = best.max(value + exhaustive_shared(base, values, count - 1, used | (1 << choice)));
            }
        }
        best
    }

    #[test]
    fn shared_rows_match_exhaustive_assignments_with_optional_choices() {
        for base in [-13i64, 0, 13, 999_999_999_999_997] {
            for choices in 0..=5 {
                for encoding in 0..4usize.pow(choices as u32) {
                    let mut digits = encoding;
                    let values: Vec<_> = (0..choices)
                        .map(|_| {
                            let digit = digits % 4;
                            digits /= 4;
                            match digit {
                                0 => None,
                                1 => Some(base.saturating_sub(1)),
                                2 => Some(base),
                                _ => Some(base + 2),
                            }
                        })
                        .collect();
                    for count in 0..=4 {
                        assert_eq!(
                            shared_value(base, count, values.iter().copied().flatten()),
                            Some(exhaustive_shared(base, &values, count, 0)),
                            "base={base} count={count} values={values:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shared_rows_keep_checked_integer_sums() {
        assert_eq!(shared_value(i64::MAX, 2, std::iter::empty()), None);
        assert_eq!(shared_value(i64::MIN, 1, std::iter::once(i64::MAX)), None);
        assert_eq!(shared_value(0, 2, [i64::MAX, 1].into_iter()), None);
    }

    #[test]
    fn five_distinct_maxima_cover_every_four_occupied_characters() {
        for seed in 0..17 {
            let mut top = TopCharacters::default();
            let mut all = Vec::new();
            for i in 0..63 {
                let c = (i % 9) as i64;
                let v = ((i * 17 + seed * 23) % 101) as i64;
                top.insert(c, v);
                all.push((c, v));
            }
            for mask in 0..512usize {
                if mask.count_ones() > 4 {
                    continue;
                }
                let used: Vec<_> = (0..9).filter(|c| mask & (1 << c) != 0).map(i64::from).collect();
                let expected = all.iter().filter(|(c, _)| !used.contains(c)).map(|(_, v)| *v).max();
                assert_eq!(top.best(&used), expected, "seed={seed} mask={mask}");
            }
        }
    }
}
