//! Lift complete raw-judgement packets to a uniform-order physical-prefix bound.
//!
//! This is an optional proof over the ENTIRE allowed suffix. In particular a conversion-capable pair is
//! never omitted from the maximum: one unavailable position of any possible pair refuses the whole bound.
//! No native recorder, probability approximation, retained table or new search/cache budget is involved.
use super::*;
use crate::search::snaps::RawNodePacket;

impl JointBounds {
    /// `None` means cancelled; `Some(None)` leaves the existing node proof unchanged. Successful caps retain
    /// the complete old cap as an independent minimum. The caller supplies its already proved power maximum.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn raw_node_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        physical: &PhysicalDeck,
        depth: usize,
        start: usize,
        power: i64,
        previous: i128,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<Option<i128>> {
        if cancelled() {
            return None;
        }
        if !(3..=4).contains(&depth) || self.best_order || self.points.is_some() || self.min_final_life.is_some() {
            return Some(None);
        }
        let (Some(fine), Some(suffix)) = (&self.fine, self.choices.get(start..)) else {
            return Some(None);
        };
        let choices = Self::prefix_choices(domain, physical, depth);
        let mut packet = RawNodePacket::default();
        for &slot in &SLOTS[..depth] {
            let Some(pair) = fine.raw_node_packet(physical.members[slot], choices[slot]) else {
                return Some(None);
            };
            packet = packet.add(pair);
        }
        for &slot in &SLOTS[depth..] {
            let mut maximum = None::<RawNodePacket>;
            for (offset, &(member, choice)) in suffix.iter().enumerate() {
                if offset.is_multiple_of(64) && cancelled() {
                    return None;
                }
                let character = pool.members[member].character_id;
                if !self.allows(slot, choice)
                    || SLOTS[..depth]
                        .iter()
                        .any(|&fixed| pool.members[physical.members[fixed]].character_id == character)
                    || domain
                        .required()
                        .iter()
                        .any(|&required| required != member && pool.members[required].character_id == character)
                    || (choice != 0 && SLOTS[..depth].iter().any(|&fixed| choices[fixed] == choice))
                {
                    continue;
                }
                // Remaining slots may repeat a resource and need not respect each other's ascending index.
                // This superset includes every legal completion, and eligibility must cover the whole superset.
                let Some(pair) = fine.raw_node_packet(member, choice) else {
                    return Some(None);
                };
                maximum = Some(maximum.map_or(pair, |old| old.maximum(pair)));
            }
            let Some(maximum) = maximum else { return Some(None) };
            packet = packet.add(maximum);
        }
        if cancelled() {
            return None;
        }
        Some(fine.raw_node_upper(power, packet).map(|cap| cap.min(previous)))
    }
}

#[cfg(test)]
#[path = "raw_tests.rs"]
mod tests;
