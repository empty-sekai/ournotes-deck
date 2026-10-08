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
    live: FxHashMap<SkillKey, Vec<usize>>,
    support: FxHashMap<SkillKey, Vec<usize>>,
    gekisou: FxHashMap<SkillKey, Vec<usize>>,
    gekisou_support: FxHashMap<SkillKey, Vec<usize>>,
    conditions: FxHashMap<i64, Vec<usize>>,
}

fn index<T, K: Eq + std::hash::Hash>(rows: &[T], key: impl Fn(&T) -> K) -> FxHashMap<K, Vec<usize>> {
    let mut out = FxHashMap::<K, Vec<usize>>::default();
    for (position, row) in rows.iter().enumerate() {
        out.entry(key(row)).or_default().push(position);
    }
    out
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

    pub(super) fn master(&self) -> &'a Master {
        self.master
    }

    pub(super) fn live_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a LiveSkillEffectRow> + '_ {
        self.live.get(&key).into_iter().flatten().map(|&i| &self.master.live_skill_effects[i])
    }

    pub(super) fn support_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a SupportSkillEffectRow> + '_ {
        self.support.get(&key).into_iter().flatten().map(|&i| &self.master.support_skill_effects[i])
    }

    pub(super) fn gekisou_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a GekisouSkillEffectRow> + '_ {
        self.gekisou.get(&key).into_iter().flatten().map(|&i| &self.master.gekisou_skill_effects[i])
    }

    pub(super) fn gekisou_support_rows(&self, key: SkillKey) -> impl Iterator<Item = &'a GekisouSkillEffectRow> + '_ {
        self.gekisou_support.get(&key).into_iter().flatten().map(|&i| &self.master.gekisou_support_skill_effects[i])
    }

    pub(super) fn condition_sets(&self, group: i64) -> impl Iterator<Item = &'a SkillConditionSetRow> + '_ {
        self.conditions.get(&group).into_iter().flatten().map(|&i| &self.master.skill_condition_sets[i])
    }
}

#[cfg(test)]
mod tests;
