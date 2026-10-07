//! Fixed-member masks applied to a completed native controller-family reward envelope.

use super::*;
use crate::search::snaps::{FamilyRewardTable, ProfileRewardTemplate};
use ournotes_sim::live::certified::F64Interval;
use std::rc::Rc;

impl JointBounds {
    pub(crate) fn family_reward_template(&self) -> Option<Rc<ProfileRewardTemplate>> {
        self.family_rewards.clone()
    }

    /// All five members are fixed; the first four SLOTS carry actual Snap bindings, and the last slot takes a
    /// choice from `last_choices`. Unassigned tail values in p.snaps are never read. The original pair traversal's
    /// suffix and slot rules are represented by the caller's exact last-choice list.
    pub(crate) fn family_mask_upper(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        table: &FamilyRewardTable,
        last_choices: &[usize],
    ) -> Option<i128> {
        if self.points.is_some() || last_choices.is_empty() || table.members[2] != p.members[2] {
            return None;
        }
        let ns = domain.snaps().len();
        if ns > 4096 {
            return None;
        }
        let mut fixed = [None; 5];
        let mut seen = [false; 5];
        for &slot in &SLOTS[..4] {
            let family_slot = table.members.iter().position(|&m| m == p.members[slot])?;
            if seen[family_slot] {
                return None;
            }
            seen[family_slot] = true;
            fixed[family_slot] = Some(match p.snaps[slot] {
                None => 0,
                Some(s) => domain.snaps().iter().position(|&x| x == s)?.checked_add(1)?,
            });
        }
        let last_member = p.members[SLOTS[4]];
        let last_slot = table.members.iter().position(|&m| m == last_member)?;
        if seen[last_slot] || table.mean.iter().any(|row| row.len() != ns + 1) {
            return None;
        }
        let mut masks: [Vec<bool>; 5] = std::array::from_fn(|_| vec![false; ns]);
        let mut none = [false; 5];
        for slot in 0..5 {
            let choices = match &fixed[slot] {
                Some(choice) => std::slice::from_ref(choice),
                None => last_choices,
            };
            for &choice in choices {
                if choice > ns {
                    return None;
                }
                if choice == 0 {
                    none[slot] = true;
                } else {
                    masks[slot][choice - 1] = true;
                }
            }
        }
        let weights = table.members.map(|m| self.w.get(m).map(Vec::as_slice));
        let weights: [&[i64]; 5] = [weights[0]?, weights[1]?, weights[2]?, weights[3]?, weights[4]?];
        // This follows from JointBounds' nonnegative, nonwrapping slot-power gate; check it again before the
        // optional assignment's i128 tie weights and its five-term i64 sum.
        if weights.iter().any(|row| row.len() != ns || row.iter().any(|&w| w.unsigned_abs() > i32::MAX as u64)) {
            return None;
        }
        let (extra, _) =
            super::super::matching::constrained_assignment(weights, masks.each_ref().map(Vec::as_slice), none)?;
        let leader = *self.profile.get(p.members[2])?;
        let base = table
            .members
            .iter()
            .try_fold(0i64, |sum, &m| sum.checked_add(self.a.get(m)?.checked_add(*self.lead.get(leader)?.get(m)?)?))?;
        let power = base.checked_add(extra)?;
        if power < 0 {
            return None;
        }
        let mut gain = F64Interval::point(table.a0).ok()?;
        for slot in 0..5 {
            let mut best = if none[slot] { table.mean[slot][0] } else { 0.0 };
            for (snap, &allowed) in masks[slot].iter().enumerate() {
                if allowed {
                    best = best.max(table.mean[slot][snap + 1]);
                }
            }
            gain = gain.add(F64Interval::point(best).ok()?);
        }
        let coefficient = gain.upper().min(table.global);
        let direct = F64Interval::integer(i128::from(power))
            .multiply(F64Interval::point(coefficient).ok()?)
            .multiply(F64Interval::ONE.add(F64Interval::point(table.eps).ok()?))
            .upper();
        if !direct.is_finite() || direct < 0.0 {
            return None;
        }
        let mut cap = direct.ceil() as i128;
        // Retain the same physical Snap for its power and gain in each weighted assignment. These three
        // optional relaxations never replace a native score or require monotonicity of an actual candidate.
        let mut rows: [Vec<(i64, f64)>; 5] = std::array::from_fn(|_| Vec::new());
        for slot in 0..5 {
            let m = table.members[slot];
            let base = self.a.get(m)?.checked_add(*self.lead.get(leader)?.get(m)?)?;
            rows[slot].try_reserve_exact(ns + 1).ok()?;
            for choice in 0..=ns {
                let extra = if choice == 0 { 0 } else { *self.w.get(m)?.get(choice - 1)? };
                let pair_power = base.checked_add(extra)?;
                rows[slot].push((pair_power, table.mean[slot][choice]));
            }
        }
        let scale = (power as f64 / gain.upper().max(1e-100)).clamp(1e-90, 1e90);
        cap = cap.min(super::resource::product_upper(
            &rows,
            &masks,
            none,
            table.a0,
            table.eps,
            [scale * 0.5, scale, scale * 2.0],
        ));
        cap.checked_mul(super::super::uniform::ORDERS as i128)
    }
}

#[cfg(test)]
#[path = "family_node_tests.rs"]
mod tests;
