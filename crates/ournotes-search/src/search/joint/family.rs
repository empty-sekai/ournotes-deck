//! Fixed-member masks applied only to completed native writer-profile reward envelopes.

use super::*;
#[cfg(test)]
use crate::search::snaps::FamilyRewardTable;
use crate::search::snaps::{FamilyProfileTable, ProfileRewardTemplate};
use ournotes_sim::live::{certified::F64Interval, full::LuckFamilyBindings};
use std::rc::Rc;

struct FamilyAssignment {
    profile: usize,
    choices: [usize; 5],
    power: i64,
}

/// Work performed for this complete mask. Offset reductions are score coefficients per unit of power,
/// counted once per actual binding check, including repeated cache hits; they are not unique saved work.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FamilyBoundResult {
    pub(crate) upper: i128,
    pub(crate) binding_drift_checks: u64,
    pub(crate) binding_drift_tightened: u64,
    pub(crate) binding_offset_reduction_sum: f64,
    pub(crate) maximum_binding_offset_reduction: f64,
}

impl JointBounds {
    pub(crate) fn family_reward_template(&self) -> Option<Rc<ProfileRewardTemplate>> {
        self.family_rewards.clone()
    }

    pub(crate) fn family_template_diagnostics(&self) -> Option<super::super::telemetry::FamilyTemplateSetup> {
        self.family_template.clone()
    }

    /// Complete-family compatibility entry. Both paths enumerate precisely the same feasible bindings.
    #[cfg(test)]
    pub(crate) fn family_mask_upper(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        table: &FamilyRewardTable,
        last_choices: &[usize],
    ) -> Option<i128> {
        if table.bindings.profile_count() != table.profiles.len()
            || table.profiles.is_empty()
            || table.profiles.iter().any(|profile| profile.mean.iter().any(|row| row.len() != domain.snaps().len() + 1))
        {
            return None;
        }
        let assignments = self.family_assignments(domain, p, table.members, &table.bindings, last_choices)?;
        family_assignment_upper(&assignments, |i| table.profiles.get(i), table.eps, table.global, None)
            .map(|result| result.upper)
    }

    /// All required profile IDs are derived from the very same physical-binding enumeration as the upper.
    /// Unknown profiles are not omitted; the preparation caller must complete each returned ID.
    pub(crate) fn required_family_profiles(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        table: &FamilyProfileTable,
        last_choices: &[usize],
    ) -> Option<Vec<usize>> {
        if table.bindings.profile_count() != table.profiles.len() || table.profiles.is_empty() {
            return None;
        }
        let assignments = self.family_assignments(domain, p, table.members, &table.bindings, last_choices)?;
        let mut required = Vec::new();
        required.try_reserve_exact(table.profiles.len()).ok()?;
        for assignment in assignments {
            if !required.contains(&assignment.profile) {
                required.push(assignment.profile);
            }
        }
        Some(required)
    }

    /// Every feasible final choice must have its own completed 120-label profile. A missing entry refuses
    /// this entire mask, so the traversal continues under its existing whole-domain upper.
    #[cfg(test)]
    pub(crate) fn family_profiles_upper(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        table: &FamilyProfileTable,
        last_choices: &[usize],
    ) -> Option<i128> {
        self.family_profiles_upper_measured(domain, p, table, last_choices).map(|result| result.upper)
    }

    pub(crate) fn family_profiles_upper_measured(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        table: &FamilyProfileTable,
        last_choices: &[usize],
    ) -> Option<FamilyBoundResult> {
        if table.bindings.profile_count() != table.profiles.len()
            || table.profiles.is_empty()
            || table
                .profiles
                .iter()
                .flatten()
                .any(|profile| profile.mean.iter().any(|row| row.len() != domain.snaps().len() + 1))
        {
            return None;
        }
        let assignments = self.family_assignments(domain, p, table.members, &table.bindings, last_choices)?;
        family_assignment_upper(
            &assignments,
            |i| table.profiles.get(i)?.as_ref(),
            table.eps,
            table.global,
            self.family_rewards.as_deref().map(|template| (template, table.members)),
        )
    }

    fn family_assignments(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        members: [usize; 5],
        bindings: &LuckFamilyBindings,
        last_choices: &[usize],
    ) -> Option<Vec<FamilyAssignment>> {
        if self.best_order || self.points.is_some() || last_choices.is_empty() || members[2] != p.members[2] {
            return None;
        }
        let ns = domain.snaps().len();
        if ns > 4096 {
            return None;
        }
        let mut choices = [0usize; 5];
        let mut resources = [None; 5];
        let mut seen = [false; 5];
        for &slot in &SLOTS[..4] {
            let family_slot = members.iter().position(|&m| m == p.members[slot])?;
            if seen[family_slot] {
                return None;
            }
            seen[family_slot] = true;
            if let Some(snap) = p.snaps[slot]
                && resources.contains(&Some(snap))
            {
                return None;
            }
            resources[family_slot] = p.snaps[slot];
            choices[family_slot] = match p.snaps[slot] {
                None => 0,
                Some(s) => domain.snaps().iter().position(|&x| x == s)?.checked_add(1)?,
            };
        }
        let last_slot = members.iter().position(|&m| m == p.members[SLOTS[4]])?;
        if seen[last_slot] {
            return None;
        }
        let weights = members.map(|m| self.w.get(m).map(Vec::as_slice));
        let weights: [&[i64]; 5] = [weights[0]?, weights[1]?, weights[2]?, weights[3]?, weights[4]?];
        if weights.iter().any(|row| row.len() != ns || row.iter().any(|&w| w.unsigned_abs() > i32::MAX as u64)) {
            return None;
        }
        let leader = *self.profile.get(p.members[2])?;
        let base = members
            .iter()
            .try_fold(0i64, |sum, &m| sum.checked_add(self.a.get(m)?.checked_add(*self.lead.get(leader)?.get(m)?)?))?;
        let mut assignments = Vec::new();
        assignments.try_reserve_exact(last_choices.len()).ok()?;
        for &choice in last_choices {
            if choice > ns {
                return None;
            }
            let resource = if choice == 0 { None } else { Some(*domain.snaps().get(choice - 1)?) };
            if let Some(snap) = resource
                && resources.iter().enumerate().any(|(slot, &used)| slot != last_slot && used == Some(snap))
            {
                continue;
            }
            choices[last_slot] = choice;
            resources[last_slot] = resource;
            let profile = bindings.profile_for(&resources)?;
            let mut power = base;
            for slot in 0..5 {
                if choices[slot] != 0 {
                    power = power.checked_add(*weights[slot].get(choices[slot] - 1)?)?;
                }
            }
            if power < 0 {
                return None;
            }
            assignments.push(FamilyAssignment { profile, choices, power });
        }
        (!assignments.is_empty()).then_some(assignments)
    }
}

fn family_assignment_upper<'a>(
    assignments: &[FamilyAssignment],
    profile: impl Fn(usize) -> Option<&'a crate::search::snaps::FamilyProfileReward>,
    eps: f64,
    global: f64,
    drift: Option<(&ProfileRewardTemplate, [usize; 5])>,
) -> Option<FamilyBoundResult> {
    let mut upper = None;
    let mut result = FamilyBoundResult::default();
    for assignment in assignments {
        let profile = profile(assignment.profile)?;
        let mut a0 = profile.a0;
        if let Some((template, members)) = drift {
            result.binding_drift_checks += 1;
            if let Some(tightened) = template.binding_drift_a0(members, profile, &assignment.choices) {
                a0 = a0.min(tightened);
                let reduction = profile.a0 - a0;
                result.binding_drift_tightened += u64::from(reduction > 0.0);
                result.binding_offset_reduction_sum += reduction;
                result.maximum_binding_offset_reduction = result.maximum_binding_offset_reduction.max(reduction);
            }
        }
        let mut gain = F64Interval::point(a0).ok()?;
        for slot in 0..5 {
            gain = gain.add(F64Interval::point(*profile.mean[slot].get(assignment.choices[slot])?).ok()?);
        }
        let direct = F64Interval::integer(i128::from(assignment.power))
            .multiply(F64Interval::point(gain.upper().min(global)).ok()?)
            .multiply(F64Interval::ONE.add(F64Interval::point(eps).ok()?))
            .upper();
        if !direct.is_finite() || direct < 0.0 {
            return None;
        }
        let cap = (direct.ceil() as i128).checked_mul(super::super::uniform::ORDERS as i128)?;
        upper = Some(upper.map_or(cap, |old: i128| old.max(cap)));
    }
    result.upper = upper?;
    Some(result)
}

#[cfg(test)]
#[path = "family_node_tests.rs"]
pub(crate) mod tests;
