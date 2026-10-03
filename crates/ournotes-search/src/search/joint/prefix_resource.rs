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

pub(super) struct PrefixResourceTables {
    rows: Vec<TopCharacters>,
    choices: usize,
}
impl PrefixResourceTables {
    pub(super) fn compile(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        let choices = domain.snaps().len() + 1;
        // Bound optional storage and the matching solver's lexicographic i128 weights.
        if choices > 4097 {
            return None;
        }
        let cells = b.lead.len().checked_mul(15)?.checked_mul(choices)?;
        if cells > 250_000 {
            return None;
        }
        let mut rows = vec![TopCharacters::default(); cells];
        for profile in 0..b.lead.len() {
            for &m in domain.members() {
                for choice in 0..choices {
                    let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                    for position in 0..5 {
                        for (scale, &r) in b.correlation_scales.iter().enumerate() {
                            let q = quantized(power, b.gains[m][choice][position], r)?;
                            rows[((profile * 5 + position) * 3 + scale) * choices + choice]
                                .insert(pool.members[m].character_id, q);
                        }
                    }
                }
            }
        }
        Some(Self { rows, choices })
    }
    fn row(&self, profile: usize, position: usize, scale: usize, choice: usize) -> &TopCharacters {
        &self.rows[((profile * 5 + position) * 3 + scale) * self.choices + choice]
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
            let ns = self.choices - 1;
            let mut shifts = [0i64; 5];
            let mut weights: [Vec<i64>; 5] = std::array::from_fn(|_| vec![0; ns]);
            let mut allowed: [Vec<bool>; 5] = std::array::from_fn(|_| vec![false; ns]);
            for &slot in &SLOTS[depth..] {
                let pos = positions[slot];
                shifts[slot] = self.row(profile, pos, scale, 0).best(&occupied)?;
                for (j, &taken) in used.iter().enumerate() {
                    if !taken && let Some(q) = self.row(profile, pos, scale, j + 1).best(&occupied) {
                        allowed[slot][j] = true;
                        weights[slot][j] = q - shifts[slot];
                    }
                }
            }
            // Assigned slots are forced to dummy None columns. Remaining character
            // uniqueness and required-card restrictions are relaxed, Snap reuse is not.
            let (extra, _) = super::super::matching::constrained_assignment(
                weights.each_ref().map(|v| v.as_slice()),
                allowed.each_ref().map(|v| v.as_slice()),
                [true; 5],
            )?;
            let sum = fixed + shifts.iter().sum::<i64>() + extra;
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
    use super::TopCharacters;
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
