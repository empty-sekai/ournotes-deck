//! Compact tables of the carrier split, one value per pair (the position means). Entries are kept in fixed arrays and
//! each table is ordered by an upper bound of what a query can read from an entry, so a top-`k` query stops as soon as
//! no later entry can enter; its result equals the scan over every entry.
use super::relax_tables::Largest;

/// The larger of two values (the first on ties).
fn larger<T: PartialOrd>(a: T, b: T) -> T {
    if b > a { b } else { a }
}

/// Up to five (value, key) entries with distinct keys, best first: value descending, then key ascending.
#[derive(Clone, Copy)]
pub(super) struct Five<T> {
    values: [T; 5],
    keys: [u16; 5],
    len: u8,
}

impl<T: Copy + PartialOrd> Five<T> {
    /// The five best entries of `best`, as a full sort by value then key would order them.
    fn from_best(zero: T, best: impl Iterator<Item = (T, u16)>) -> Self {
        let mut top = Five { values: [zero; 5], keys: [0; 5], len: 0 };
        let before = |a: (T, u16), b: (T, u16)| match a.0.partial_cmp(&b.0).expect("finite split value") {
            std::cmp::Ordering::Equal => a.1 < b.1,
            order => order.is_gt(),
        };
        for e in best {
            let len = top.len as usize;
            if len == 5 && !before(e, (top.values[4], top.keys[4])) {
                continue;
            }
            let mut i = len.min(4);
            top.len = (len + 1).min(5) as u8;
            while i > 0 && before(e, (top.values[i - 1], top.keys[i - 1])) {
                top.values[i] = top.values[i - 1];
                top.keys[i] = top.keys[i - 1];
                i -= 1;
            }
            top.values[i] = e.0;
            top.keys[i] = e.1;
        }
        top
    }

    /// The best value whose key is free, if any.
    fn first(&self, taken: impl Fn(u16) -> bool) -> Option<T> {
        (0..self.len as usize).find(|&i| !taken(self.keys[i])).map(|i| self.values[i])
    }

    /// The best value, if any.
    fn top(&self) -> Option<T> {
        (self.len > 0).then(|| self.values[0])
    }
}

/// A character with its pairs: the best value without a Snap (if any) and the best values per Snap (keys: domain Snap
/// index), with `upper` at least every value a query reads from it.
#[derive(Clone, Copy)]
struct Row<T> {
    upper: T,
    character: u16,
    base: Option<T>,
    snaps: Five<T>,
}

impl<T: Copy + PartialOrd> Row<T> {
    /// The best value with no Snap or a Snap outside `taken_snaps`.
    fn best(&self, taken_snaps: &[bool]) -> Option<T> {
        match (self.base, self.snaps.first(|j| taken_snaps[j as usize])) {
            (Some(a), Some(b)) => Some(larger(a, b)),
            (a, b) => a.or(b),
        }
    }
}

/// Orders rows by `upper`, descending.
fn by_upper<T: PartialOrd>(a: &T, b: &T) -> std::cmp::Ordering {
    b.partial_cmp(a).expect("finite split value")
}

/// One carrier list's pairs by character.
pub(super) struct ByCharacter<T> {
    rows: Vec<Row<T>>,
}

impl<T: Copy + PartialOrd> ByCharacter<T> {
    /// The rows of the pairs (character, choice, value); choice 0 is no Snap, choice `c` the domain Snap `c - 1`.
    pub(super) fn compile(mut pairs: Vec<(u16, usize, T)>, zero: T) -> Self {
        pairs.sort_by_key(|&(character, choice, _)| (character, choice));
        let mut rows = Vec::new();
        let mut i = 0;
        while i < pairs.len() {
            let character = pairs[i].0;
            let mut base = None;
            let mut snaps: Vec<(T, u16)> = Vec::new();
            while i < pairs.len() && pairs[i].0 == character {
                let (_, choice, v) = pairs[i];
                if choice == 0 {
                    base = Some(base.map_or(v, |x| larger(x, v)));
                } else {
                    match snaps.last_mut() {
                        Some(last) if last.1 as usize == choice - 1 => last.0 = larger(last.0, v),
                        _ => snaps.push((v, (choice - 1) as u16)),
                    }
                }
                i += 1;
            }
            let snaps = Five::from_best(zero, snaps.into_iter());
            let upper = match (base, snaps.top()) {
                (Some(a), Some(b)) => larger(a, b),
                (a, b) => a.or(b).expect("a character row has a pair"),
            };
            rows.push(Row { upper, character, base, snaps });
        }
        rows.sort_by(|a, b| by_upper(&a.upper, &b.upper));
        ByCharacter { rows }
    }

    /// The `k` largest best values of characters outside `taken` with no Snap or a Snap outside `taken_snaps`,
    /// summed in descending order with `add` from `zero`; None with fewer such characters.
    pub(super) fn top(
        &self,
        taken: &[bool],
        taken_snaps: &[bool],
        k: usize,
        zero: T,
        add: impl Fn(T, T) -> T,
    ) -> Option<T> {
        if k == 0 {
            return Some(zero);
        }
        let mut best = Largest::new(k, zero);
        for row in &self.rows {
            if !best.admits(row.upper) {
                break;
            }
            if taken[row.character as usize] {
                continue;
            }
            if let Some(v) = row.best(taken_snaps) {
                best.push(v);
            }
        }
        (best.values().len() == k).then(|| best.values().iter().fold(zero, |sum, &v| add(sum, v)))
    }

    pub(super) fn bytes(&self) -> usize {
        self.rows.len() * std::mem::size_of::<Row<T>>()
    }
}

/// The table relaxation of the slots that take no carrier, for one value per pair: every slot takes a distinct
/// character outside the prefix, each at most its best pair with no Snap or a free Snap, and their sum is also at
/// most the best member-only values of distinct characters plus the best Snap increments of distinct free Snaps. A
/// member with an admitted pair keeps its member-only value as the base of its Snap increments.
pub(super) struct Plain<T> {
    /// Characters with an admitted pair by their member-only value (`base` of the compile), descending.
    base: Vec<(T, u16)>,
    /// The same characters with their best pairs, by `upper` descending.
    best: Vec<Row<T>>,
    /// Snaps by the largest increment of any character (at least `zero`), descending, with the increments by
    /// character.
    increments: Vec<(T, u16, Five<T>)>,
    /// The number of domain Snaps.
    snaps: usize,
}

/// The parts of the table relaxation for some slots: the best pairs, the member-only values and the Snap increments,
/// each the largest of distinct characters or Snaps.
pub(super) struct PlainParts<T> {
    pub(super) best: Largest<T>,
    pub(super) base: Largest<T>,
    pub(super) increments: Largest<T>,
}

impl<T: Copy + PartialOrd> Plain<T> {
    /// The tables of the pairs `admitted` admits among `members` (pool member, character index), with
    /// `pair(m, choice)` the value of a pair and `bottom` below every value. A character's member-only value is
    /// `floor(pair(m, 0))` (the larger of it and zero for gains), an increment `increment(m, j)` for domain Snap `j`.
    /// None when a value is not finite (`finite`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compile(
        members: &[(usize, u16)],
        characters: usize,
        snaps: usize,
        admitted: &dyn Fn(usize, usize) -> bool,
        pair: &dyn Fn(usize, usize) -> T,
        increment: &dyn Fn(usize, usize, T, T) -> T,
        floor: &dyn Fn(T) -> T,
        finite: &dyn Fn(T) -> bool,
        zero: T,
        bottom: T,
    ) -> Option<Self> {
        let mut base = vec![None::<T>; characters];
        // [character * snaps + j]: best pair value; [j * characters + character]: best increment
        let mut by_snap = vec![bottom; characters * snaps];
        let mut increments = vec![bottom; characters * snaps];
        for &(m, character) in members {
            let c = character as usize;
            let own: Vec<usize> = (1..=snaps).filter(|&choice| admitted(m, choice)).collect();
            if own.is_empty() && !admitted(m, 0) {
                continue;
            }
            let b = pair(m, 0);
            if !finite(b) {
                return None;
            }
            base[c] = Some(base[c].map_or(b, |x| larger(x, b)));
            for choice in own {
                let (j, v) = (choice - 1, pair(m, choice));
                if !finite(v) {
                    return None;
                }
                by_snap[c * snaps + j] = larger(by_snap[c * snaps + j], v);
                increments[j * characters + c] = larger(increments[j * characters + c], increment(m, j, v, b));
            }
        }
        let mut base_rows = Vec::new();
        let mut best = Vec::new();
        for (c, b) in base.iter().enumerate() {
            let Some(b) = b.map(floor) else { continue };
            let character = c as u16;
            base_rows.push((b, character));
            let row = &by_snap[c * snaps..(c + 1) * snaps];
            let five = Five::from_best(zero, row.iter().copied().zip(0..));
            let upper = five.top().map_or(b, |v| larger(b, v));
            best.push(Row { upper, character, base: Some(b), snaps: five });
        }
        base_rows.sort_by(|a, b| by_upper(&a.0, &b.0));
        best.sort_by(|a, b| by_upper(&a.upper, &b.upper));
        let mut snap_rows: Vec<(T, u16, Five<T>)> = (0..snaps)
            .map(|j| {
                let row = &increments[j * characters..(j + 1) * characters];
                let five = Five::from_best(zero, row.iter().copied().zip(0..));
                (five.top().map_or(zero, |v| larger(zero, v)), j as u16, five)
            })
            .collect();
        snap_rows.sort_by(|a, b| by_upper(&a.0, &b.0));
        Some(Plain { base: base_rows, best, increments: snap_rows, snaps })
    }

    /// The parts for `take` slots (at least one) with the prefix's characters `taken` and Snaps `taken_snaps`.
    pub(super) fn parts(&self, take: usize, taken: &[bool], taken_snaps: &[bool], zero: T) -> PlainParts<T> {
        let mut best = Largest::new(take, zero);
        for row in &self.best {
            if !best.admits(row.upper) {
                break;
            }
            if !taken[row.character as usize] {
                let v = row.snaps.first(|j| taken_snaps[j as usize]);
                let b = row.base.expect("a plain row has its base");
                best.push(v.map_or(b, |v| larger(b, v)));
            }
        }
        let mut base = Largest::new(take, zero);
        for &(v, c) in &self.base {
            if !base.admits(v) {
                break;
            }
            if !taken[c as usize] {
                base.push(v);
            }
        }
        // a taken Snap adds nothing; every increment is at least zero
        let mut increments = Largest::new(take, zero);
        for (upper, j, five) in &self.increments {
            if !increments.admits(*upper) {
                break;
            }
            if !taken_snaps[*j as usize] {
                increments.push(five.first(|c| taken[c as usize]).map_or(zero, |v| larger(zero, v)));
            }
        }
        while increments.values().len() < take.min(self.snaps) {
            increments.push(zero);
        }
        PlainParts { best, base, increments }
    }

    pub(super) fn bytes(&self) -> usize {
        self.base.len() * std::mem::size_of::<(T, u16)>()
            + self.best.len() * std::mem::size_of::<Row<T>>()
            + self.increments.len() * std::mem::size_of::<(T, u16, Five<T>)>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_keeps_the_sorted_prefix() {
        let values = [3.0, 7.0, 7.0, 1.0, 9.0, 2.0, 7.0, 5.0];
        let five = Five::from_best(0.0, values.iter().copied().zip(0..));
        let mut sorted: Vec<(f64, u16)> = values.iter().copied().zip(0..).collect();
        sorted.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
        for (i, &(v, k)) in sorted.iter().take(5).enumerate() {
            assert_eq!((five.values[i], five.keys[i]), (v, k));
        }
        assert_eq!(five.first(|k| k == 4 || k == 1), Some(7.0));
    }

    #[test]
    fn early_stops_match_full_scans() {
        // characters 0..6, Snaps 0..5, values from a fixed pattern
        let (characters, snaps) = (6usize, 5usize);
        let members: Vec<(usize, u16)> = (0..9).map(|m| (m, (m % characters) as u16)).collect();
        let value = |m: usize, choice: usize| ((m * 37 + choice * 11) % 23) as f64 + 0.25 * m as f64;
        let admitted = |m: usize, choice: usize| !(m + choice).is_multiple_of(4);
        let plain = Plain::compile(
            &members,
            characters,
            snaps,
            &admitted,
            &value,
            &|_, _, v, b| (v - b).next_up(),
            &|b: f64| b.max(0.0),
            &|v: f64| v.is_finite(),
            0.0,
            f64::NEG_INFINITY,
        )
        .unwrap();
        for mask in 0..(1u32 << (characters + snaps)) {
            // a prefix fixes at most four characters and four Snaps
            if (mask & ((1 << characters) - 1)).count_ones() > 4 || (mask >> characters).count_ones() > 4 {
                continue;
            }
            let taken: Vec<bool> = (0..characters).map(|c| mask >> c & 1 == 1).collect();
            let taken_snaps: Vec<bool> = (0..snaps).map(|j| mask >> (characters + j) & 1 == 1).collect();
            for take in 1..=3 {
                let parts = plain.parts(take, &taken, &taken_snaps, 0.0);
                // the full scans
                let mut best = Largest::new(take, 0.0);
                let mut base = Largest::new(take, 0.0);
                for c in 0..characters {
                    if taken[c] {
                        continue;
                    }
                    let own: Vec<usize> = members.iter().filter(|x| x.1 as usize == c).map(|x| x.0).collect();
                    let with: Vec<usize> =
                        own.iter().copied().filter(|&m| (0..=snaps).any(|choice| admitted(m, choice))).collect();
                    if with.is_empty() {
                        continue;
                    }
                    let b = with.iter().map(|&m| value(m, 0)).fold(f64::NEG_INFINITY, f64::max).max(0.0);
                    let s = with
                        .iter()
                        .flat_map(|&m| (1..=snaps).filter(move |&ch| admitted(m, ch)).map(move |ch| (m, ch)))
                        .filter(|&(_, ch)| !taken_snaps[ch - 1])
                        .map(|(m, ch)| value(m, ch))
                        .fold(f64::NEG_INFINITY, f64::max);
                    best.push(b.max(s));
                    base.push(b);
                }
                let mut increments = Largest::new(take, 0.0);
                for j in 0..snaps {
                    let mut g = 0.0f64;
                    if !taken_snaps[j] {
                        for &(m, c) in &members {
                            let with = (0..=snaps).any(|choice| admitted(m, choice));
                            if with && !taken[c as usize] && admitted(m, j + 1) {
                                g = g.max((value(m, j + 1) - value(m, 0)).next_up());
                            }
                        }
                    }
                    increments.push(g);
                }
                assert_eq!(parts.best.values(), best.values(), "mask {mask} take {take}");
                assert_eq!(parts.base.values(), base.values(), "mask {mask} take {take}");
                assert_eq!(parts.increments.values(), increments.values(), "mask {mask} take {take}");
            }
        }
    }
}
