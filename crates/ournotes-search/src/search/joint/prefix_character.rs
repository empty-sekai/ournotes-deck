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

#[derive(Clone, Copy)]
struct CharacterEdge {
    character: usize,
    value: i64,
}

#[derive(Clone, Copy, Default)]
struct ResidualRow([Option<CharacterEdge>; 4]);
impl ResidualRow {
    fn insert(&mut self, character: usize, value: i64, count: usize) {
        let row = &mut self.0[..count];
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
/// remain exact in i64. Enumerating their product visits at most 4^4 bindings.
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
    let mut best = None;
    visit(rows, &mut [0; 4], 0, 0, &mut best);
    best
}

pub(super) struct PrefixCharacterTables {
    rows: Vec<TopSnaps>,
    characters: Vec<i64>,
}
impl PrefixCharacterTables {
    pub(super) fn compile(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        let mut characters: Vec<_> = domain.members().iter().map(|&m| pool.members[m].character_id).collect();
        characters.sort_unstable();
        characters.dedup();
        // Optional storage. Retain the existing capacity gate even though the
        // residual value solver no longer needs canonical matcher's i128 weights.
        if characters.len() > 4096 {
            return None;
        }
        let cells = b.lead.len().checked_mul(15)?.checked_mul(characters.len())?;
        if cells > 250_000 {
            return None;
        }
        let mut rows = vec![TopSnaps::default(); cells];
        for profile in 0..b.lead.len() {
            for &m in domain.members() {
                let character = characters.binary_search(&pool.members[m].character_id).ok()?;
                for choice in 0..=domain.snaps().len() {
                    let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                    for position in 0..5 {
                        for (scale, &r) in b.correlation_scales.iter().enumerate() {
                            let q = super::prefix_resource::quantized(power, b.gains[m][choice][position], r)?;
                            rows[((profile * 5 + position) * 3 + scale) * characters.len() + character]
                                .insert(choice, q);
                        }
                    }
                }
            }
        }
        Some(Self { rows, characters })
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
            let mut rows = [ResidualRow::default(); 4];
            for (row, &slot) in SLOTS[depth..].iter().enumerate() {
                let offset = ((profile * 5 + positions[slot]) * 3 + scale) * self.characters.len();
                for &character in &columns {
                    if let Some(q) = self.rows[offset + character].best(&chosen) {
                        rows[row].insert(character, q, count);
                    }
                }
            }
            // Every future position takes one distinct character. Future Snap
            // reuse and required-card constraints are relaxed; prefix resources
            // and the native position of every assigned slot remain fixed.
            let extra = residual_value(&rows[..count])?;
            let upper = add_up((fixed + extra) as f64, (r * keyed.map_or(b.a0, |k| k.a0)).next_up());
            let cap =
                (((upper * upper).next_up() / (4.0 * r)).next_up() * (1.0 + b.eps).next_up()).next_up().ceil() as i128;
            best = best.min(cap);
        }
        (best < i128::MAX).then_some(best)
    }
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
