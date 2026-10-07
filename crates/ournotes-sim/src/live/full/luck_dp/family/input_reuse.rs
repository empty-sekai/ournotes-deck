//! Pre-construction identities inside one immutable, fully admitted family preparation.
//!
//! This is intentionally narrower than general recorder admission. The native constructor reads a
//! Performer's character only through Factory's member-target predicates, then retains the compiled
//! checkers rather than the Performer. Known character-blind sources therefore have exactly the same
//! initialized model and Plan after erasing character_id. Every other input field and source order stays.
use super::*;
use std::fmt::Write;

// Compiled recording keys start with their Debug row list; this binary prefix cannot alias that namespace.
const PREFIX: &[u8] = b"\0family-complete-projected-input\0\x01";

pub(super) struct InputKeys {
    physical: [Performer; SLOTS],
}

impl InputKeys {
    pub(super) fn new(master: &Master, physical: &[Performer; SLOTS]) -> Option<Self> {
        for performer in physical {
            if performer.live_skill.is_some() || !performer.support_skills.is_empty() {
                return None;
            }
            if let Some((id, level)) = performer.gekisou_skill {
                master.gekisou_skill(id)?;
                for row in master.gekisou_skill_effects.iter().filter(|row| row.skill_id == id && row.level == level) {
                    if !character_blind_row(master, row) {
                        return None;
                    }
                }
            }
            for &(id, level) in &performer.gekisou_support_skills {
                master.gekisou_support_skill(id)?;
                for row in
                    master.gekisou_support_skill_effects.iter().filter(|row| row.skill_id == id && row.level == level)
                {
                    if !character_blind_row(master, row) {
                        return None;
                    }
                }
            }
        }
        let mut physical = physical.clone();
        for performer in &mut physical {
            performer.character_id = 0;
        }
        Some(Self { physical })
    }

    /// Sort the complete normalized descriptors, with multiplicities. The inverse is a fixed slot
    /// bijection; it is used only to transport labels, never to reorder an actual native replay.
    pub(super) fn canonical(&self, capacity: usize) -> Option<(Vec<u8>, [usize; SLOTS])> {
        let mut order = [0, 1, 2, 3, 4];
        order.sort_by(|&a, &b| self.physical[a].cmp(&self.physical[b]));
        let bytes = self.key(&order, capacity)?;
        let mut inverse = [0; SLOTS];
        for (canonical, &physical) in order.iter().enumerate() {
            inverse[physical] = canonical;
        }
        Some((bytes, inverse))
    }

    pub(super) fn key(&self, order: &[usize; SLOTS], capacity: usize) -> Option<Vec<u8>> {
        let mut ordered = [None; SLOTS];
        let mut seen = 0u8;
        for (position, &slot) in order.iter().enumerate() {
            let performer = self.physical.get(slot)?;
            if seen & (1 << slot) != 0 {
                return None;
            }
            seen |= 1 << slot;
            ordered[position] = Some(performer);
        }
        let mut out = KeyBytes { bytes: Vec::new(), capacity };
        out.bytes.try_reserve_exact(capacity.min(4096)).ok()?;
        if out.bytes.capacity() > capacity {
            return None;
        }
        out.append(PREFIX).ok()?;
        // Performer derives Eq and Debug from integer/optional/vector fields. Keep its complete ordered
        // image, including empty sources and all member attributes except the one proved unread above.
        write!(&mut out, "{ordered:?}").ok()?;
        Some(out.bytes)
    }
}

fn character_blind_row(master: &Master, row: &crate::master::GekisouSkillEffectRow) -> bool {
    // Include full selected rows, not merely the writer catalogue: otherwise an omitted conversion could
    // construct a separate LIFE model. Unknown/cumulative/conversion programs keep the original route.
    if !matches!(row.skill_effect_type, 2000 | 2005 | 11001 | 11002 | 11003 | 11005)
        || row.skill_cumulative_condition_id != 0
    {
        return false;
    }
    [
        row.skill_trigger_condition_group,
        row.skill_condition_group,
        row.skill_release_condition_group,
        row.effect_execute_limit_reset_condition_group,
    ]
    .into_iter()
    .all(|group| character_blind_group(master, group))
}

fn character_blind_group(master: &Master, group: i64) -> bool {
    if group == 0 {
        return true;
    }
    master.skill_condition_sets.iter().filter(|set| set.group == group).all(|set| {
        set.condition_ids.iter().all(|&id| {
            let Some(condition) = master.skill_condition(id) else { return false };
            // A closed subset of Factory::one. Only 3000/3001/5000 inspect member targets, and those
            // targets cannot use character equality here. The other kinds read music, range or chance.
            if !matches!(
                condition.condition_type,
                0 | 3000 | 3001 | 4011 | 4012 | 5000 | 7000 | 7010 | 7013 | 7020 | 7021 | 8000
            ) {
                return false;
            }
            condition
                .condition_target_ids
                .iter()
                .all(|&id| master.skill_target(id).is_some_and(|target| target.character_id <= 0))
        })
    })
}

struct KeyBytes {
    bytes: Vec<u8>,
    capacity: usize,
}

impl KeyBytes {
    fn append(&mut self, bytes: &[u8]) -> std::fmt::Result {
        let length = self.bytes.len().checked_add(bytes.len()).ok_or(std::fmt::Error)?;
        if length > self.capacity {
            return Err(std::fmt::Error);
        }
        if length > self.bytes.capacity() {
            self.bytes.try_reserve_exact(length - self.bytes.len()).map_err(|_| std::fmt::Error)?;
            if self.bytes.capacity() > self.capacity {
                return Err(std::fmt::Error);
            }
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

impl std::fmt::Write for KeyBytes {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.append(value.as_bytes())
    }
}

#[cfg(test)]
#[path = "input_reuse_tests.rs"]
mod tests;
