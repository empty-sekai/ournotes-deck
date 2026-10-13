//! Dual of the Snap-resource relaxation: retain character and position capacity.
use super::*;

#[derive(Clone, Copy)]
struct Edge {
    choice: usize,
    value: i64,
}

/// An unfinished prefix occupies at most four Snaps. Five distinct choices keep
/// the exact best available edge; choice zero (None) is never occupied.
#[derive(Clone, Default)]
struct TopSnaps([Option<Edge>; 5]);
impl TopSnaps {
    fn insert(&mut self, choice: usize, value: i64) {
        // Sorted by value: an edge no better than the last entry changes nothing (see `TopCharacters::insert`).
        if self.0[4].is_some_and(|e| e.value >= value) {
            return;
        }
        let at = self.0.iter().position(|e| e.is_some_and(|e| e.choice == choice)).unwrap_or(4);
        if self.0[at].is_some_and(|e| e.value >= value) {
            return;
        }
        self.0[at] = Some(Edge { choice, value });
        self.0.sort_unstable_by(|a, b| b.map(|e| e.value).cmp(&a.map(|e| e.value)));
    }
    fn best(&self, occupied: &[usize]) -> Option<i64> {
        self.0.iter().flatten().find(|e| e.choice == 0 || !occupied.contains(&e.choice)).map(|e| e.value)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CharacterEdge {
    character: usize,
    value: i64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct ResidualRow([Option<CharacterEdge>; 4]);
impl ResidualRow {
    fn insert(&mut self, character: usize, value: i64, count: usize) {
        let row = &mut self.0[..count];
        // Sorted by value: an edge no better than the last entry changes nothing (see `TopCharacters::insert`).
        if row[count - 1].is_some_and(|e| e.value >= value) {
            return;
        }
        let at = row.iter().position(|e| e.is_some_and(|e| e.character == character)).unwrap_or(count - 1);
        if row[at].is_some_and(|e| e.value >= value) {
            return;
        }
        row[at] = Some(CharacterEdge { character, value });
        row.sort_unstable_by(|a, b| b.map(|e| e.value).cmp(&a.map(|e| e.value)));
    }
}

/// Exact value for k mandatory rows, retaining only each row's best k characters.
/// If an assignment uses an edge outside that row's top k, at most k-1 of those
/// k characters are occupied by other rows, so an unused edge can replace it
/// without reducing the value. Repeating this produces an optimum using only
/// retained edges. Each character is distinct within a row; ties are harmless
/// because no canonical binding is returned or used to certify physical Top-K.
/// The caller supplies k<=4 rows and quantized edges in 0..=1e15, so all sums
/// remain exact in i64. Equal rows use their largest k distinct characters directly; other rows enumerate at
/// most 4^4 bindings.
fn residual_value(rows: &[ResidualRow]) -> Option<i64> {
    fn visit(rows: &[ResidualRow], chosen: &mut [usize; 4], depth: usize, value: i64, best: &mut Option<i64>) {
        if depth == rows.len() {
            *best = Some(best.map_or(value, |old| old.max(value)));
            return;
        }
        for edge in rows[depth].0.iter().flatten() {
            if chosen[..depth].contains(&edge.character) {
                continue;
            }
            chosen[depth] = edge.character;
            visit(rows, chosen, depth + 1, value + edge.value, best);
        }
    }
    debug_assert!(rows.len() <= 4);
    if let Some(first) = rows.first()
        && rows.iter().all(|row| row == first)
    {
        return first.0[..rows.len()].iter().try_fold(0i64, |sum, edge| sum.checked_add(edge.map(|e| e.value)?));
    }
    let mut best = None;
    visit(rows, &mut [0; 4], 0, 0, &mut best);
    best
}

pub(super) struct PrefixCharacterTables {
    /// Position-mean gains give every position the same rows, indexed by profile, scale and character.
    rows: Vec<TopSnaps>,
    characters: Vec<i64>,
    /// By profile and scale, the largest quantized edge of the pairs of the choice order from each offset on
    /// (`suffix`, one more offset than pairs) and of each of its blocks of [`PAIR_BLOCK`] pairs (`blocks`); empty
    /// past their storage gate.
    suffix: Vec<i64>,
    blocks: Vec<i64>,
}
impl PrefixCharacterTables {
    /// Empty rows for the domain, None past the storage gate; [`compile_prefix_tables`] fills them.
    fn empty(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        let mut characters: Vec<_> = domain.members().iter().map(|&m| pool.members[m].character_id).collect();
        characters.sort_unstable();
        characters.dedup();
        // Bounded optional storage for the domain characters.
        if characters.len() > 4096 {
            return None;
        }
        let cells = b.lead.len().checked_mul(3)?.checked_mul(characters.len())?;
        if cells > 250_000 {
            return None;
        }
        Some(Self { rows: vec![TopSnaps::default(); cells], characters, suffix: Vec::new(), blocks: Vec::new() })
    }

    /// The suffix and block maxima of the quantized edges along the choice order of `b` (see the fields).
    fn compile_maxima(&mut self, b: &JointBounds) -> Option<()> {
        let n = b.choices.len();
        let nb = n.div_ceil(PAIR_BLOCK);
        let sets = b.lead.len().checked_mul(3)?;
        if sets.checked_mul(n + 1 + nb)? > 2_000_000 {
            return Some(());
        }
        let (mut suffix, mut blocks) = (vec![0i64; sets * (n + 1)], vec![0i64; sets * nb]);
        for profile in 0..b.lead.len() {
            for (scale, &r) in b.correlation_scales.iter().enumerate() {
                let set = profile * 3 + scale;
                for i in (0..n).rev() {
                    let (m, choice) = b.choices[i];
                    let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                    let q = super::prefix_resource::quantized(power, b.gains[m][choice][0], r)?;
                    suffix[set * (n + 1) + i] = suffix[set * (n + 1) + i + 1].max(q);
                    let block = &mut blocks[set * nb + i / PAIR_BLOCK];
                    *block = (*block).max(q);
                }
            }
        }
        (self.suffix, self.blocks) = (suffix, blocks);
        Some(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn upper(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
        keyed: Option<&super::Keyed>,
    ) -> Option<i128> {
        if !(1..5).contains(&depth) {
            return None;
        }
        let profile = b.profile[p.members[2]];
        let occupied: Vec<_> = SLOTS[..depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let columns: Vec<_> =
            self.characters.iter().enumerate().filter(|(_, c)| !occupied.contains(c)).map(|(j, _)| j).collect();
        let mut chosen = [0usize; 5];
        for &slot in &SLOTS[..depth] {
            chosen[slot] =
                p.snaps[slot].map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
        }
        let mut best = i128::MAX;
        for (scale, &r) in b.correlation_scales.iter().enumerate() {
            let mut fixed = 0i64;
            for &slot in &SLOTS[..depth] {
                let m = p.members[slot];
                let choice = chosen[slot];
                let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                let gain = keyed.map_or(b.gains[m][choice][positions[slot]], |k| k.placed[slot][positions[slot]]);
                fixed += super::prefix_resource::quantized(power, gain, r)?;
            }
            let count = 5 - depth;
            let mut shared = ResidualRow::default();
            let offset = (profile * 3 + scale) * self.characters.len();
            for &character in &columns {
                if let Some(q) = self.rows[offset + character].best(&chosen) {
                    shared.insert(character, q, count);
                }
            }
            // Every future position takes one distinct character. Future Snap
            // reuse and required-card constraints are relaxed; prefix resources
            // and the native position of every assigned slot remain fixed.
            let extra = residual_value(&[shared; 4][..count])?;
            let upper = add_up((fixed + extra) as f64, (r * keyed.map_or(b.a0, |k| k.a0)).next_up());
            let cap =
                (((upper * upper).next_up() / (4.0 * r)).next_up() * (1.0 + b.eps).next_up()).next_up().ceil() as i128;
            best = best.min(cap);
        }
        (best < i128::MAX).then_some(best)
    }

    /// What [`JointBounds::character_pair_upper`] reads of a prefix of one to four slots: per correlation scale,
    /// the quantized edges of its slots, the best edges of the characters left (the first `5 - depth`), and `r * A0`.
    #[allow(clippy::too_many_arguments)]
    fn pairs(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
        keyed: Option<&super::Keyed>,
    ) -> Option<[(i64, ResidualRow, f64); 3]> {
        if !(1..5).contains(&depth) {
            return None;
        }
        let profile = b.profile[p.members[2]];
        let occupied: Vec<_> = SLOTS[..depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let mut chosen = [0usize; 5];
        for &slot in &SLOTS[..depth] {
            chosen[slot] =
                p.snaps[slot].map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
        }
        let mut out = [(0i64, ResidualRow::default(), 0f64); 3];
        for (scale, &r) in b.correlation_scales.iter().enumerate() {
            let mut fixed = 0i64;
            for &slot in &SLOTS[..depth] {
                let m = p.members[slot];
                let choice = chosen[slot];
                let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                let gain = keyed.map_or(b.gains[m][choice][positions[slot]], |k| k.placed[slot][positions[slot]]);
                fixed += super::prefix_resource::quantized(power, gain, r)?;
            }
            let mut shared = ResidualRow::default();
            let offset = (profile * 3 + scale) * self.characters.len();
            for (character, id) in self.characters.iter().enumerate() {
                if occupied.contains(id) {
                    continue;
                }
                if let Some(q) = self.rows[offset + character].best(&chosen) {
                    shared.insert(character, q, 5 - depth);
                }
            }
            out[scale] = (fixed, shared, (r * keyed.map_or(b.a0, |k| k.a0)).next_up());
        }
        Some(out)
    }
}

/// The correlated bounds of the children of one prefix (see [`JointBounds::character_pairs`]).
pub(crate) struct CharacterPairs {
    scales: [(i64, ResidualRow, f64); 3],
    /// Per scale, the sum of the best edges of the characters left that the slots after the next one take (None
    /// when too few are left).
    rest: [Option<i64>; 3],
    depth: usize,
    profile: usize,
    /// The total mass of the performance orders.
    weight: i128,
}

impl JointBounds {
    /// With the character prefix tables: what bounds each child of the prefix `p` of one to four slots in the way
    /// [`JointBounds::correlated_expected_upper_keyed`] bounds the prefix itself (see
    /// [`JointBounds::character_pair_upper`]). A child of four slots is a complete team, whose bound has no
    /// characters left: `P * (A0 + G)` is at most `(P + r * (A0 + G))^2 / (4 * r)` for a team as for a prefix.
    pub(crate) fn character_pairs(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        keyed: Option<&super::Keyed>,
    ) -> Option<CharacterPairs> {
        let t = self.prefix_character.as_ref()?;
        let (positions, _) = orders.first()?;
        let weight = orders.iter().try_fold(0i128, |sum, &(_, w)| sum.checked_add(i128::try_from(w).ok()?))?;
        let scales = t.pairs(self, pool, domain, p, depth, positions, keyed)?;
        let rest = scales.each_ref().map(|(_, shared, _)| {
            let edges = shared.0.iter().flatten().take(4 - depth);
            let (n, sum) = edges.fold((0, Some(0i64)), |(n, sum), e| (n + 1, sum.and_then(|s| s.checked_add(e.value))));
            sum.filter(|_| n == 4 - depth)
        });
        Some(CharacterPairs { scales, rest, depth, profile: self.profile[p.members[2]], weight })
    }

    /// At least [`JointBounds::character_pair_upper`] of every child whose pair lies at or after `offset` in the
    /// choice order (`blocks` false), or in block `offset` of [`PAIR_BLOCK`] pairs (`blocks` true): the largest edge
    /// of those pairs with the best edges of every character left, the child's included. None without the maxima.
    pub(crate) fn character_set_upper(&self, pairs: &CharacterPairs, offset: usize, blocks: bool) -> Option<i128> {
        let t = self.prefix_character.as_ref()?;
        let n = self.choices.len();
        let (table, width) = if blocks { (&t.blocks, n.div_ceil(PAIR_BLOCK)) } else { (&t.suffix, n + 1) };
        if table.is_empty() {
            return None;
        }
        let mut best = i128::MAX;
        for (scale, (((fixed, _, a0r), rest), &r)) in
            pairs.scales.iter().zip(&pairs.rest).zip(&self.correlation_scales).enumerate()
        {
            let q = *table.get((pairs.profile * 3 + scale) * width + offset)?;
            let upper = add_up(fixed.checked_add(q)?.checked_add((*rest)?)? as f64, *a0r);
            best = best.min(self.correlated_cap(upper, r));
        }
        best.checked_mul(pairs.weight)
    }

    /// The correlated score cap of the sum `upper` of a team's edges and `r * A0` at scale `r`.
    fn correlated_cap(&self, upper: f64, r: f64) -> i128 {
        (((upper * upper).next_up() / (4.0 * r)).next_up() * (1.0 + self.eps).next_up()).next_up().ceil() as i128
    }

    /// At least the correlated bound, summed over the orders, of every completion of the child of the prefix of
    /// `pairs` that takes `m` with `choice` in the next slot: the prefix's bound with that slot fixed, whose rows
    /// are the best edges of the characters left but the child's (the child's Snap stays available to them). The
    /// gains of every position agree in the character tables, so one row serves every order. None when the child
    /// cannot be completed in the tables or an edge leaves their range.
    pub(crate) fn character_pair_upper(
        &self,
        pairs: &CharacterPairs,
        pool: &Pool,
        m: usize,
        choice: usize,
    ) -> Option<i128> {
        let t = self.prefix_character.as_ref()?;
        let column = t.characters.binary_search(&pool.members[m].character_id).ok()?;
        let power = self.a[m] + self.lead[pairs.profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
        let left = 4 - pairs.depth;
        let mut best = i128::MAX;
        for ((fixed, shared, a0r), &r) in pairs.scales.iter().zip(&self.correlation_scales) {
            let q = super::prefix_resource::quantized(power, self.gains[m][choice][0], r)?;
            let mut extra = 0i64;
            let mut taken = 0;
            for edge in shared.0.iter().flatten().filter(|e| e.character != column).take(left) {
                extra = extra.checked_add(edge.value)?;
                taken += 1;
            }
            if taken < left {
                return None;
            }
            let upper = add_up(fixed.checked_add(q)?.checked_add(extra)? as f64, *a0r);
            best = best.min(self.correlated_cap(upper, r));
        }
        best.checked_mul(pairs.weight)
    }
}

/// Both prefix tables, each None past its own storage gate, in one pass. Position-mean gains give every position
/// the same quantized edge of each (profile, member, choice, scale), so all positions share one column. Every
/// resource row receives its edges by member and every character row by member then choice. Compilation checks
/// exact bit equality across positions and the quantized range of every edge.
pub(super) fn compile_prefix_tables(
    b: &JointBounds,
    pool: &Pool,
    domain: &CandidateDomain,
) -> (Option<super::prefix_resource::PrefixResourceTables>, Option<PrefixCharacterTables>) {
    if b.gains.iter().flatten().any(|g| g.iter().any(|v| v.to_bits() != g[0].to_bits())) {
        return (None, None);
    }
    let mut resource = super::prefix_resource::PrefixResourceTables::empty(b, domain);
    let mut character = PrefixCharacterTables::empty(b, pool, domain);
    if resource.is_none() && character.is_none() {
        return (None, None);
    }
    for profile in 0..b.lead.len() {
        for &m in domain.members() {
            let id = pool.members[m].character_id;
            let column = character.as_ref().map(|t| t.characters.binary_search(&id).expect("character of the domain"));
            for choice in 0..=domain.snaps().len() {
                let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                for (scale, &r) in b.correlation_scales.iter().enumerate() {
                    let Some(q) = super::prefix_resource::quantized(power, b.gains[m][choice][0], r) else {
                        return (None, None);
                    };
                    if let Some(t) = resource.as_mut() {
                        t.insert(profile, scale, choice, id, q);
                    }
                    if let (Some(t), Some(column)) = (character.as_mut(), column) {
                        t.rows[(profile * 3 + scale) * t.characters.len() + column].insert(choice, q);
                    }
                }
            }
        }
    }
    if let Some(t) = character.as_mut()
        && t.compile_maxima(b).is_none()
    {
        character = None;
    }
    (resource, character)
}

impl JointBounds {
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn character_prefix_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
    ) -> Option<i128> {
        self.prefix_character.as_ref()?.upper(self, pool, domain, p, depth, positions, None)
    }
}

#[cfg(test)]
mod tests {
    use super::{ResidualRow, TopSnaps, residual_value};

    // Enumerate the original, untruncated edge lists independently of the
    // top-k representation and solver, including infeasible assignments.
    fn brute_full(rows: &[Vec<(usize, i64)>]) -> Option<i64> {
        fn visit(rows: &[Vec<(usize, i64)>], used: &mut Vec<usize>, total: i64) -> Option<i64> {
            let Some((row, rest)) = rows.split_first() else {
                return Some(total);
            };
            let mut best = None;
            for &(character, value) in row {
                if used.contains(&character) {
                    continue;
                }
                used.push(character);
                if let Some(value) = visit(rest, used, total + value) {
                    best = Some(best.map_or(value, |old: i64| old.max(value)));
                }
                used.pop();
            }
            best
        }
        visit(rows, &mut Vec::new(), 0)
    }

    fn retained_value(rows: &[Vec<(usize, i64)>]) -> Option<i64> {
        let mut retained = [ResidualRow::default(); 4];
        for (row, edges) in rows.iter().enumerate() {
            for &(character, value) in edges {
                retained[row].insert(character, value, rows.len());
            }
        }
        residual_value(&retained[..rows.len()])
    }

    #[test]
    fn shared_character_rows_match_full_assignment_enumeration() {
        for count in 0..=4 {
            for columns in 0..=7 {
                for seed in 0..17 {
                    let edges: Vec<_> = (0..columns)
                        .filter(|&c| (c + seed) % 4 != 0)
                        .flat_map(|c| {
                            let value = 1_000_000_000_000_000 - ((c * 13 + seed * 7) % 19) as i64;
                            [(c, value), (c, value - 1)]
                        })
                        .collect();
                    let rows = vec![edges; count];
                    assert_eq!(retained_value(&rows), brute_full(&rows), "{rows:?}");
                }
            }
        }
    }

    #[test]
    fn residual_matches_every_small_matrix_with_missing_zero_and_tied_edges() {
        for count in 0..=3 {
            for columns in 0..=3 {
                for encoding in 0..3usize.pow((count * columns) as u32) {
                    let mut digits = encoding;
                    let rows: Vec<Vec<_>> = (0..count)
                        .map(|_| {
                            (0..columns)
                                .filter_map(|character| {
                                    let digit = digits % 3;
                                    digits /= 3;
                                    match digit {
                                        0 => None,
                                        _ => Some((character, (digit - 1) as i64)),
                                    }
                                })
                                .collect()
                        })
                        .collect();
                    assert_eq!(retained_value(&rows), brute_full(&rows), "{rows:?}");
                }
            }
        }
    }

    #[test]
    fn residual_four_rows_match_full_edges_with_large_values_and_duplicate_characters() {
        for columns in 0..=8 {
            for seed in 0..67 {
                let rows: Vec<Vec<_>> = (0..4)
                    .map(|row| {
                        (0..columns)
                            .rev()
                            .filter(|&character| (character * 7 + row * 11 + seed) % 5 != 0)
                            .flat_map(|character| {
                                let value = 1_000_000_000_000_000 - ((row * 13 + character * 7 + seed * 3) % 11) as i64;
                                // A repeated character must keep its best edge,
                                // never consume two places in the retained row.
                                [(character, value), (character, value - (seed % 3) as i64)]
                            })
                            .collect()
                    })
                    .collect();
                assert_eq!(retained_value(&rows), brute_full(&rows), "{rows:?}");
            }
        }
        let missing = vec![vec![(0, 1)], vec![(1, 1)], vec![(2, 1)], vec![]];
        assert_eq!(retained_value(&missing), None);
        let conflict = vec![vec![(0, 1_000_000_000_000_000)]; 4];
        assert_eq!(retained_value(&conflict), None);
        let ties = vec![(0..8).map(|c| (c, 1_000_000_000_000_000)).collect(); 4];
        assert_eq!(retained_value(&ties), Some(4_000_000_000_000_000));
    }

    #[test]
    fn retained_choices_cover_every_four_occupied_snaps_and_none_is_reusable() {
        for seed in 0..17 {
            let mut top = TopSnaps::default();
            let mut all = Vec::new();
            for i in 0..63 {
                let choice = i % 9;
                let value = ((i * 17 + seed * 23) % 101) as i64;
                top.insert(choice, value);
                all.push((choice, value));
            }
            for mask in 0..256usize {
                if mask.count_ones() > 4 {
                    continue;
                }
                let mut occupied: Vec<_> = (1..9).filter(|c| mask & (1 << (c - 1)) != 0).collect();
                occupied.push(0); // Empty prefix slots must never remove None.
                let expected = all.iter().filter(|(c, _)| *c == 0 || !occupied.contains(c)).map(|(_, v)| *v).max();
                assert_eq!(top.best(&occupied), expected, "seed={seed} mask={mask}");
            }
        }
    }
}
