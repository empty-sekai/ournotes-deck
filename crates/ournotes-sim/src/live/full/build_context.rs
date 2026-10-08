//! Source-row indexes for repeated construction under one immutable master borrow.
//!
//! Entries retain every row in table order. They do not compile checkers, validate data, or retain mutable
//! runtime state. The ordinary constructor determines validation order and still creates fresh pools and
//! counters. A filtered or edited master receives its own context; no index is attached to `Master`.
use super::*;
use crate::master::{LiveSkillEffectRow, SkillConditionSetRow};

type SkillKey = (i64, i64);

pub(super) struct BuildContext<'a> {
    master: &'a Master,
    live: RowIndex<SkillKey>,
    support: RowIndex<SkillKey>,
    gekisou: RowIndex<SkillKey>,
    gekisou_support: RowIndex<SkillKey>,
    conditions: RowIndex<i64>,
}

enum RowIndex<K> {
    Hashed(FxHashMap<K, Vec<usize>>),
    // The bounded route has no opaque hash-table allocation or per-key Vec.
    // Original ordinal breaks key ties, preserving exact table order.
    Sorted(Vec<(K, usize)>),
}

enum Positions<'a, K> {
    Hashed(std::slice::Iter<'a, usize>),
    Sorted(std::slice::Iter<'a, (K, usize)>),
}

impl<K> Iterator for Positions<'_, K> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Hashed(rows) => rows.next().copied(),
            Self::Sorted(rows) => rows.next().map(|(_, ordinal)| *ordinal),
        }
    }
}

impl<K: Ord + std::hash::Hash> RowIndex<K> {
    fn positions(&self, key: &K) -> Positions<'_, K> {
        match self {
            Self::Hashed(index) => Positions::Hashed(index.get(key).map_or(&[][..], Vec::as_slice).iter()),
            Self::Sorted(index) => {
                let lo = index.partition_point(|(row, _)| row < key);
                let hi = index.partition_point(|(row, _)| row <= key);
                Positions::Sorted(index[lo..hi].iter())
            }
        }
    }

    fn bounded_payload_bytes(&self) -> Option<usize> {
        match self {
            Self::Hashed(_) => None,
            Self::Sorted(index) => index.capacity().checked_mul(std::mem::size_of::<(K, usize)>()),
        }
    }
}

fn index<T, K: Eq + std::hash::Hash>(rows: &[T], key: impl Fn(&T) -> K) -> RowIndex<K> {
    let mut out = FxHashMap::<K, Vec<usize>>::default();
    for (position, row) in rows.iter().enumerate() {
        out.entry(key(row)).or_default().push(position);
    }
    RowIndex::Hashed(out)
}

fn bounded_index<T, K: Ord>(rows: &[T], key: impl Fn(&T) -> K, remaining: &mut usize) -> Option<RowIndex<K>> {
    let requested = rows.len().checked_mul(std::mem::size_of::<(K, usize)>())?;
    if requested > *remaining {
        return None;
    }
    let mut index = Vec::new();
    index.try_reserve_exact(rows.len()).ok()?;
    let allocated = index.capacity().checked_mul(std::mem::size_of::<(K, usize)>())?;
    *remaining = remaining.checked_sub(allocated)?;
    index.extend(rows.iter().enumerate().map(|(position, row)| (key(row), position)));
    // This in-place sort uses no heap scratch. Complete (key, ordinal) order
    // also preserves duplicate source rows and condition-set order exactly.
    index.sort_unstable();
    Some(RowIndex::Sorted(index))
}

impl<'a> BuildContext<'a> {
    pub(super) fn new(master: &'a Master) -> Self {
        Self {
            master,
            live: index(&master.live_skill_effects, |row| (row.live_skill_id, row.level)),
            support: index(&master.support_skill_effects, |row| (row.support_skill_id, row.level)),
            gekisou: index(&master.gekisou_skill_effects, |row| (row.skill_id, row.level)),
            gekisou_support: index(&master.gekisou_support_skill_effects, |row| (row.skill_id, row.level)),
            conditions: index(&master.skill_condition_sets, |row| row.group),
        }
    }

    /// Optional temporary index under a caller's existing byte allowance.
    /// Refusal leaves ordinary native construction available. No data is
    /// validated, filtered or compiled before the original constructor reads it.
    pub(super) fn try_bounded(master: &'a Master, byte_limit: usize) -> Option<Self> {
        let mut remaining = byte_limit.checked_sub(std::mem::size_of::<Self>())?;
        // Check the complete minimum before allocating any partial index.
        let source_rows = master
            .live_skill_effects
            .len()
            .checked_add(master.support_skill_effects.len())?
            .checked_add(master.gekisou_skill_effects.len())?
            .checked_add(master.gekisou_support_skill_effects.len())?;
        let needed = source_rows
            .checked_mul(std::mem::size_of::<(SkillKey, usize)>())?
            .checked_add(master.skill_condition_sets.len().checked_mul(std::mem::size_of::<(i64, usize)>())?)?;
        if needed > remaining {
            return None;
        }
        let result = Self {
            master,
            live: bounded_index(&master.live_skill_effects, |row| (row.live_skill_id, row.level), &mut remaining)?,
            support: bounded_index(
                &master.support_skill_effects,
                |row| (row.support_skill_id, row.level),
                &mut remaining,
            )?,
            gekisou: bounded_index(&master.gekisou_skill_effects, |row| (row.skill_id, row.level), &mut remaining)?,
            gekisou_support: bounded_index(
                &master.gekisou_support_skill_effects,
                |row| (row.skill_id, row.level),
                &mut remaining,
            )?,
            conditions: bounded_index(&master.skill_condition_sets, |row| row.group, &mut remaining)?,
        };
        (result.bounded_bytes()? <= byte_limit).then_some(result)
    }

    /// Actual retained capacities of the bounded variant, including inline index storage.
    pub(super) fn bounded_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>()
            .checked_add(self.live.bounded_payload_bytes()?)?
            .checked_add(self.support.bounded_payload_bytes()?)?
            .checked_add(self.gekisou.bounded_payload_bytes()?)?
            .checked_add(self.gekisou_support.bounded_payload_bytes()?)?
            .checked_add(self.conditions.bounded_payload_bytes()?)
    }

    pub(super) fn master(&self) -> &'a Master {
        self.master
    }

    pub(super) fn live_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a LiveSkillEffectRow> + '_ {
        self.live.positions(&key).map(|i| &self.master.live_skill_effects[i])
    }

    pub(super) fn support_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a SupportSkillEffectRow> + '_ {
        self.support.positions(&key).map(|i| &self.master.support_skill_effects[i])
    }

    pub(super) fn gekisou_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a GekisouSkillEffectRow> + '_ {
        self.gekisou.positions(&key).map(|i| &self.master.gekisou_skill_effects[i])
    }

    pub(super) fn gekisou_support_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a GekisouSkillEffectRow> + '_ {
        self.gekisou_support.positions(&key).map(|i| &self.master.gekisou_support_skill_effects[i])
    }

    pub(super) fn condition_sets(&self, group: i64) -> impl Iterator<Item = &'a SkillConditionSetRow> + '_ {
        self.conditions.positions(&group).map(|i| &self.master.skill_condition_sets[i])
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bounded_tests;
