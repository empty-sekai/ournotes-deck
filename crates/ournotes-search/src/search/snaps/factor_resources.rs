//! Complete-domain factor-command limits from distinct physical resources.
//!
//! Every legal deck uses five distinct characters and each physical Snap at most once. Independently relax
//! each resource to its largest complete member/class contribution at each of the five performance positions.
//! A 32-mask assignment then bounds each command/error quantity over all original orders. The two resource
//! relaxations need not choose the same deck; their componentwise minimum is still a universal upper bound.

use std::collections::BTreeMap;

/// Lifetime commands, command executions, total factor norm, and score-frame factor norm.
type Counts = [f64; 4];
type Positions = [Counts; 5];
const MAX_RESOURCES: usize = 4096;

#[derive(Clone, Copy)]
pub(super) struct ResourceLimits {
    pub(super) characters: Counts,
    pub(super) snaps: Counts,
}

pub(super) struct FactorResources {
    valid: bool,
    characters: BTreeMap<i64, Positions>,
    /// Physical indexes in Tables::snaps, never one entry per equivalence class.
    snaps: Vec<Positions>,
    none: Positions,
}

impl FactorResources {
    /// Capacity is only an optional-bound budget; declining leaves the original full-domain envelope intact.
    pub(super) fn new(snaps: usize) -> Option<Self> {
        if snaps > MAX_RESOURCES {
            return None;
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(snaps).ok()?;
        rows.resize(snaps, [[0.0; 4]; 5]);
        Some(Self { valid: true, characters: BTreeMap::new(), snaps: rows, none: [[0.0; 4]; 5] })
    }

    pub(super) fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Record an existing windows() certificate, before its member/class identity is forgotten.
    pub(super) fn observe(&mut self, character: i64, none: bool, snaps: &[usize], position: usize, q: Counts) {
        if !self.valid {
            return;
        }
        if position >= 5
            || q.iter().any(|v| !v.is_finite() || *v < 0.0)
            || snaps.iter().any(|&s| s >= self.snaps.len())
            || (self.characters.len() >= MAX_RESOURCES && !self.characters.contains_key(&character))
        {
            self.valid = false;
            return;
        }
        let row = self.characters.entry(character).or_insert([[0.0; 4]; 5]);
        for (target, value) in row[position].iter_mut().zip(q) {
            *target = target.max(value);
        }
        if none {
            for (target, value) in self.none[position].iter_mut().zip(q) {
                *target = target.max(value);
            }
        }
        for &snap in snaps {
            for (target, value) in self.snaps[snap][position].iter_mut().zip(q) {
                *target = target.max(value);
            }
        }
    }

    pub(super) fn finish(self) -> Option<ResourceLimits> {
        if !self.valid {
            return None;
        }
        let characters = distinct_positions(self.characters.values())?;
        // None is freely repeatable. Five separate dummy resources cover every number of empty pairings.
        let snaps = distinct_positions(self.snaps.iter().chain(std::iter::repeat_n(&self.none, 5)))?;
        Some(ResourceLimits { characters, snaps })
    }
}

/// A resource may be skipped or placed once. Each component is maximized independently; no ranking or
/// candidate value is computed. Reading an unchanged previous table prevents reusing the current resource.
fn distinct_positions<'a>(rows: impl Iterator<Item = &'a Positions>) -> Option<Counts> {
    let mut values = [[f64::NEG_INFINITY; 4]; 32];
    values[0] = [0.0; 4];
    for row in rows {
        if row.iter().flatten().any(|v| !v.is_finite() || *v < 0.0) {
            return None;
        }
        let previous = values;
        for mask in 0..31 {
            if !previous[mask][0].is_finite() {
                continue;
            }
            for (position, weights) in row.iter().enumerate() {
                if mask & (1 << position) != 0 {
                    continue;
                }
                let next = &mut values[mask | (1 << position)];
                for (component, &weight) in weights.iter().enumerate() {
                    let upper = (previous[mask][component] + weight).next_up();
                    if !upper.is_finite() {
                        return None;
                    }
                    next[component] = next[component].max(upper);
                }
            }
        }
    }
    values[31].iter().all(|v| v.is_finite() && *v >= 0.0).then_some(values[31])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(member: usize, class: usize, position: usize) -> Counts {
        let base = (1 + (member * 11 + class * 7 + position * 3) % 13) as f64;
        let rare = if member == 0 && class == 1 { 100.0 } else { 0.0 };
        [base + rare, base * 8.0 + rare * 9.0, base / 8.0 + rare, base / 16.0 + rare]
    }

    #[test]
    fn resource_caps_cover_all_legal_members_pairings_and_positions() {
        let chars = [0, 0, 1, 2, 3, 4, 5];
        let mut resources = FactorResources::new(3).unwrap();
        let mut positional = [[0.0f64; 4]; 5];
        for (member, &character) in chars.iter().enumerate() {
            // Two physical Snaps deliberately share class 1. Class 0 includes the None choice.
            for (class, snaps) in [&[][..], &[0, 1][..], &[2][..]].into_iter().enumerate() {
                for (position, maximum) in positional.iter_mut().enumerate() {
                    let q = counts(member, class, position);
                    resources.observe(character, class == 0, snaps, position, q);
                    for (v, q) in maximum.iter_mut().zip(q) {
                        *v = v.max(q);
                    }
                }
            }
        }
        let limits = resources.finish().unwrap();
        let mut exact = [0.0f64; 4];
        let mut visited = 0;
        fn enumerate(
            position: usize,
            occupied: u8,
            used_snaps: u8,
            sum: Counts,
            chars: &[i64],
            exact: &mut Counts,
            visited: &mut usize,
        ) {
            if position == 5 {
                *visited += 1;
                for (best, q) in exact.iter_mut().zip(sum) {
                    *best = best.max(q);
                }
                return;
            }
            for (member, &character) in chars.iter().enumerate() {
                if occupied & (1 << character) != 0 {
                    continue;
                }
                for snap in [None, Some(0), Some(1), Some(2)] {
                    if snap.is_some_and(|s| used_snaps & (1 << s) != 0) {
                        continue;
                    }
                    let class = snap.map_or(0, |s| if s < 2 { 1 } else { 2 });
                    let q = counts(member, class, position);
                    // These small dyadic inputs sum exactly in binary64; this oracle does not use the DP.
                    let sum = std::array::from_fn(|i| sum[i] + q[i]);
                    enumerate(
                        position + 1,
                        occupied | (1 << character),
                        used_snaps | snap.map_or(0, |s| 1 << s),
                        sum,
                        chars,
                        exact,
                        visited,
                    );
                }
            }
        }
        enumerate(0, 0, 0, [0.0; 4], &chars, &mut exact, &mut visited);
        assert_eq!(visited, 179_520);
        for component in 0..4 {
            let original = positional.iter().fold(0.0f64, |sum, row| (sum + row[component]).next_up());
            assert!(limits.characters[component] >= exact[component]);
            assert!(limits.snaps[component] >= exact[component]);
            assert!(limits.characters[component].min(limits.snaps[component]) < original);
        }
    }

    #[test]
    fn physical_snap_duplicates_and_repeatable_none_remain_available() {
        let mut resources = FactorResources::new(2).unwrap();
        for character in 0..5 {
            for position in 0..5 {
                resources.observe(character, true, &[], position, [1.0; 4]);
                resources.observe(character, false, &[0, 1], position, [10.0; 4]);
            }
        }
        let limits = resources.finish().unwrap();
        for component in 0..4 {
            assert!(limits.characters[component] >= 50.0);
            assert!((23.0..23.0 + 1e-12).contains(&limits.snaps[component]));
        }
    }

    #[test]
    fn invalid_or_unavailable_certificate_keeps_optional_fallback() {
        assert!(FactorResources::new(MAX_RESOURCES + 1).is_none());
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            let mut resources = FactorResources::new(0).unwrap();
            for character in 0..5 {
                for position in 0..5 {
                    resources.observe(character, true, &[], position, [1.0; 4]);
                }
            }
            resources.observe(0, true, &[], 0, [1.0, invalid, 1.0, 1.0]);
            assert!(resources.finish().is_none());
        }
        let one = [[1.0; 4]; 5];
        assert!(distinct_positions(std::iter::once(&one)).is_none());
        let huge = [[f64::MAX; 4]; 5];
        assert!(distinct_positions(std::iter::repeat_n(&huge, 5)).is_none());
    }
}
