//! Pre-construction identities inside one immutable, fully admitted family preparation.
//!
//! The native constructor reads member attributes through Factory's target predicates, then retains the
//! compiled checkers rather than the Performer. The closed source/condition proof excludes cumulative and
//! unknown consumers. Its whole-deck union of referenced target fields permits erasing only unread attributes
//! in this private key. All source identities, levels, order and every potentially read attribute remain exact;
//! the original performers still construct every uncached native model and all 120 labels remain present.
use super::*;
use std::fmt::Write;

// Compiled recording keys start with their Debug row list; this binary prefix cannot alias that namespace.
const PREFIX: &[u8] = b"\0family-complete-projected-input\0\x02";
const CONTROLLER_PREFIX: &[u8] = b"\0family-admitted-controller-input\0\x01";

/// Created only after every original physical pair and the complete labelled work guards have passed.
/// This permits a controller-only key; it is neither a completed profile nor a probability certificate.
#[derive(Debug)]
pub(super) struct ControllerInputAdmission {
    _complete_pairs: (),
}

impl ControllerInputAdmission {
    pub(super) fn after_complete_pairs() -> Self {
        Self { _complete_pairs: () }
    }
}

pub(super) struct InputKeys {
    physical: [Performer; SLOTS],
    prefix: &'static [u8],
}

impl InputKeys {
    pub(super) fn new(master: &Master, physical: &[Performer; SLOTS]) -> Option<Self> {
        Self::build(master, physical, false)
    }

    /// The family uses the empty writer catalogue, whose native `related` predicate keeps only chain rows.
    /// Three closed bonus types are removed before Factory constructs their conditions/cumulative counters.
    /// Conversion rows still refuse this proof, and retained writers keep the original closed read analysis.
    /// Thus no omitted bonus can enter a LIFE interpreter: no conversion is held and no writer reads LIFE.
    /// Sources, levels and ordered support vectors remain exact, including sources with no retained rows.
    pub(super) fn admitted_controller(
        master: &Master,
        physical: &[Performer; SLOTS],
        writers: &LuckSkills,
        _admitted: &ControllerInputAdmission,
    ) -> Option<Self> {
        if !writers.chain.is_empty() || !writers.shapes.is_empty() || !writers.rows.is_empty() {
            return None;
        }
        Self::new(master, physical).or_else(|| Self::build(master, physical, true))
    }

    fn build(master: &Master, physical: &[Performer; SLOTS], controller: bool) -> Option<Self> {
        let mut reads = AttributeReads::default();
        for performer in physical {
            if performer.live_skill.is_some() || !performer.support_skills.is_empty() {
                return None;
            }
            if let Some((id, level)) = performer.gekisou_skill {
                master.gekisou_skill(id)?;
                if !admit_rows(
                    master,
                    master.gekisou_skill_effects.iter().filter(|row| row.skill_id == id && row.level == level),
                    &mut reads,
                    controller,
                ) {
                    return None;
                }
            }
            for &(id, level) in &performer.gekisou_support_skills {
                master.gekisou_support_skill(id)?;
                if !admit_rows(
                    master,
                    master.gekisou_support_skill_effects.iter().filter(|row| row.skill_id == id && row.level == level),
                    &mut reads,
                    controller,
                ) {
                    return None;
                }
            }
        }
        let mut physical = physical.clone();
        for performer in &mut physical {
            reads.erase_unread(performer);
        }
        Some(Self { physical, prefix: if controller { CONTROLLER_PREFIX } else { PREFIX } })
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
        out.append(self.prefix).ok()?;
        // Performer derives Eq and Debug from integer/optional/vector fields. Keep its complete normalized
        // ordered image, including empty sources and the exact vectors of every potentially read attribute.
        write!(&mut out, "{ordered:?}").ok()?;
        Some(out.bytes)
    }
}

/// `matches_skill_target` ignores the target discriminator and ORs all active selectors. A field is retained
/// if any referenced target can read it, even when another selector happens to match the current performers.
/// The mask spans every source and owner because Factory's 3000/3001 predicates inspect the whole deck.
#[derive(Default)]
struct AttributeReads {
    band: bool,
    card_type: bool,
    tags: bool,
    live_categories: bool,
    gekisou_categories: bool,
    mission: bool,
}

impl AttributeReads {
    fn include(&mut self, target: &crate::master::SkillTargetRow) -> bool {
        // Preserve the original character-reader refusal boundary.
        if target.character_id > 0 {
            return false;
        }
        self.band |= target.band_id > 0;
        self.card_type |= target.card_type != 0;
        self.tags |= target.tag_id > 0;
        self.live_categories |= target.live_skill_categories.iter().any(|&value| value != 0);
        self.gekisou_categories |= target.gekisou_skill_categories.iter().any(|&value| value != 0);
        self.mission |= target.gekisou_mission_type != 0;
        true
    }

    fn erase_unread(&self, performer: &mut Performer) {
        performer.character_id = 0;
        if !self.band {
            performer.band_id = 0;
        }
        if !self.card_type {
            performer.card_type = 0;
        }
        if !self.tags {
            performer.tag_ids.clear();
        }
        if !self.live_categories {
            performer.live_skill_categories.clear();
        }
        if !self.gekisou_categories {
            performer.gekisou_skill_categories.clear();
        }
        if !self.mission {
            performer.gekisou_mission_type = 0;
        }
    }
}

fn admit_rows<'a>(
    master: &Master,
    rows: impl Iterator<Item = &'a crate::master::GekisouSkillEffectRow>,
    reads: &mut AttributeReads,
    controller: bool,
) -> bool {
    let mut selected = false;
    for row in rows {
        selected = true;
        // These rows are absent from the actual family controller before checker/cumulative construction.
        // Whole-pair admission has already constructed the original sources. Do not reinterpret their
        // predicates, erase their source identities, or extend this branch to converters/unknown effects.
        if controller && matches!(row.skill_effect_type, 12000 | 13000 | 13002) {
            if row.skill_target_ids.iter().any(|&id| master.skill_target(id).is_none()) {
                return false;
            }
            continue;
        }
        if controller && matches!(row.skill_effect_type, 12006 | 13005) {
            return false;
        }
        if !admit_row(master, row, reads) {
            return false;
        }
    }
    // A missing selected level keeps the ordinary recording path, rather than authorizing an empty proof.
    selected
}

fn admit_row(master: &Master, row: &crate::master::GekisouSkillEffectRow, reads: &mut AttributeReads) -> bool {
    // Include full selected rows, not merely the writer catalogue: otherwise an omitted conversion could
    // construct a separate LIFE model. Unknown/cumulative/conversion programs keep the original route.
    if !matches!(row.skill_effect_type, 2000 | 2005 | 11001 | 11002 | 11003 | 11005)
        || row.skill_cumulative_condition_id != 0
        || row.skill_target_ids.iter().any(|&id| master.skill_target(id).is_none())
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
    .all(|group| admit_group(master, group, reads))
}

fn admit_group(master: &Master, group: i64, reads: &mut AttributeReads) -> bool {
    if group == 0 {
        return true;
    }
    let mut selected = false;
    for set in master.skill_condition_sets.iter().filter(|set| set.group == group) {
        selected = true;
        for &id in &set.condition_ids {
            let Some(condition) = master.skill_condition(id) else { return false };
            // A closed subset of Factory::one. Only 3000/3001/5000 inspect member targets. All referenced
            // target fields are retained conservatively even for the music/range/chance conditions.
            if !matches!(
                condition.condition_type,
                0 | 3000 | 3001 | 4011 | 4012 | 5000 | 7000 | 7010 | 7013 | 7020 | 7021 | 8000
            ) {
                return false;
            }
            for &id in &condition.condition_target_ids {
                let Some(target) = master.skill_target(id) else { return false };
                if !reads.include(target) {
                    return false;
                }
            }
        }
    }
    selected
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

#[cfg(test)]
#[path = "controller_input_tests.rs"]
mod controller_tests;
