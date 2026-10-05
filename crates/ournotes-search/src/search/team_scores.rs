//! Reuse a complete lottery-free score law when only the leader changes and the exact power stays equal.
//!
//! The cache belongs to one Engine (one immutable pool, chart, play, clocks and setup). The live receives the
//! ordered Performers and total power, not the leader identity. Its 120 orders are therefore identical after
//! relabelling the slots of the same five member/Snap pairs. Keep the complete ordered score vector, so this
//! does not collapse team identities, ties or the order results. Payoffs are recomputed for the current physical
//! deck from its cached score and terminal life, preserving event bonus and score/life dependencies.

use super::expectation::{FiniteEvaluation, PhysicalDeck};
use super::telemetry::CacheUse;
use super::uniform::{ORDERS, all_orders, order_index};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    pairs: [(usize, Option<usize>); 5],
    power: i32,
}

impl Key {
    fn new(deck: &PhysicalDeck, power: i32) -> Self {
        let mut pairs = std::array::from_fn(|i| (deck.members[i], deck.snaps[i]));
        pairs.sort_unstable();
        Self { pairs, power }
    }

    /// Each physical slot's index in the sorted pair layout.
    fn slots(self, deck: &PhysicalDeck) -> [usize; 5] {
        std::array::from_fn(|i| {
            self.pairs.iter().position(|&pair| pair == (deck.members[i], deck.snaps[i])).expect("same five pairs")
        })
    }
}

/// At most 64 fixed arrays (60 KiB of score/life payload). Eviction changes work, never the search domain.
pub(super) struct TeamScores {
    rows: HashMap<Key, [(i32, i32); ORDERS]>,
    fifo: VecDeque<Key>,
    capacity: usize,
}

impl TeamScores {
    pub(super) fn new(cache_entries: usize) -> Self {
        Self { rows: HashMap::new(), fifo: VecDeque::new(), capacity: cache_entries.min(64) }
    }

    pub(super) fn get(&self, deck: &PhysicalDeck, power: i32, telemetry: &mut CacheUse) -> Option<Vec<(i32, i32)>> {
        if self.capacity == 0 {
            return None;
        }
        telemetry.lookups += 1;
        let key = Key::new(deck, power);
        let scores = self.rows.get(&key)?;
        telemetry.hits += 1;
        let slots = key.slots(deck);
        Some(all_orders().iter().map(|order| scores[order_index(&order.map(|slot| slots[slot]))]).collect())
    }

    pub(super) fn insert(
        &mut self,
        deck: &PhysicalDeck,
        power: i32,
        evaluation: &FiniteEvaluation,
        final_lives: &[i32; ORDERS],
        telemetry: &mut CacheUse,
    ) {
        if self.capacity == 0 || evaluation.outcomes.len() != ORDERS {
            return;
        }
        let key = Key::new(deck, power);
        let slots = key.slots(deck);
        let mut scores = [(0, 0); ORDERS];
        let mut seen = [false; ORDERS];
        for outcome in &evaluation.outcomes {
            if outcome.root_seed != 0 || outcome.weight != 1 {
                return;
            }
            let mut sorted = outcome.performance_order;
            sorted.sort_unstable();
            if sorted != [0, 1, 2, 3, 4] {
                return;
            }
            let index = order_index(&outcome.performance_order.map(|slot| slots[slot]));
            if seen[index] {
                return;
            }
            seen[index] = true;
            scores[index] = (outcome.final_score, final_lives[order_index(&outcome.performance_order)]);
        }
        if self.rows.contains_key(&key) {
            return;
        }
        if self.rows.len() == self.capacity {
            self.rows.remove(&self.fifo.pop_front().expect("nonempty full cache"));
            telemetry.evictions += 1;
        }
        self.rows.insert(key, scores);
        self.fifo.push_back(key);
        telemetry.peak_entries = telemetry.peak_entries.max(self.rows.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::expectation::{SeedOutcome, aggregate};

    fn score(members: [usize; 5]) -> i32 {
        members.into_iter().fold(0, |value, member| 10 * value + member as i32 + 1)
    }

    fn evaluation(deck: PhysicalDeck) -> FiniteEvaluation {
        aggregate(
            all_orders()
                .into_iter()
                .map(|performance_order| {
                    let final_score = score(performance_order.map(|slot| deck.members[slot]));
                    SeedOutcome {
                        root_seed: 0,
                        weight: 1,
                        performance_order,
                        final_score,
                        terminal_payoff: final_score.into(),
                    }
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn all_relabellings_keep_actual_performer_sequences_and_snap_pairing() {
        let origin = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [Some(7), None, Some(8), None, None] };
        let mut cache = TeamScores::new(64);
        let mut telemetry = CacheUse::default();
        cache.insert(&origin, 123, &evaluation(origin), &[100; ORDERS], &mut telemetry);
        for layout in all_orders() {
            let deck = PhysicalDeck {
                members: layout.map(|slot| origin.members[slot]),
                snaps: layout.map(|slot| origin.snaps[slot]),
            };
            let actual = cache.get(&deck, 123, &mut telemetry).unwrap();
            let expected: Vec<_> =
                all_orders().into_iter().map(|order| (score(order.map(|slot| deck.members[slot])), 100)).collect();
            assert_eq!(actual, expected);
        }
        assert_eq!(telemetry.hits, 120);
        assert!(cache.get(&origin, 124, &mut telemetry).is_none());
        let mut other_pairing = origin;
        other_pairing.snaps.swap(0, 2);
        assert!(cache.get(&other_pairing, 123, &mut telemetry).is_none());
    }

    #[test]
    fn zero_capacity_eviction_and_incomplete_laws_never_become_hits() {
        let deck = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
        let mut telemetry = CacheUse::default();
        let complete = evaluation(deck);
        let mut disabled = TeamScores::new(0);
        disabled.insert(&deck, 1, &complete, &[100; ORDERS], &mut telemetry);
        assert!(disabled.get(&deck, 1, &mut telemetry).is_none());
        let mut cache = TeamScores::new(1);
        let mut duplicate = complete.clone();
        duplicate.outcomes[119] = duplicate.outcomes[0].clone();
        cache.insert(&deck, 1, &duplicate, &[100; ORDERS], &mut telemetry);
        assert!(cache.get(&deck, 1, &mut telemetry).is_none());
        duplicate.outcomes.pop();
        cache.insert(&deck, 1, &duplicate, &[100; ORDERS], &mut telemetry);
        assert!(cache.get(&deck, 1, &mut telemetry).is_none());
        cache.insert(&deck, 1, &complete, &[100; ORDERS], &mut telemetry);
        cache.insert(&deck, 2, &complete, &[100; ORDERS], &mut telemetry);
        assert!(cache.get(&deck, 1, &mut telemetry).is_none());
        assert!(cache.get(&deck, 2, &mut telemetry).is_some());
        assert_eq!(telemetry.evictions, 1);
        assert_eq!(telemetry.peak_entries, 1);
    }
}
