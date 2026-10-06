//! Canonical Top-K with one result per member set.

/// Canonical Snap order: real card IDs ascending, then empty slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SnapKey {
    Card(i64),
    Empty,
}

impl From<Option<i64>> for SnapKey {
    fn from(id: Option<i64>) -> Self {
        match id {
            Some(id) => Self::Card(id),
            None => Self::Empty,
        }
    }
}

/// A legal, exactly evaluated deck.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The objective (power, skip score or live score).
    pub value: i64,
    /// Deck power (equal to `value` for the power objective).
    pub power: i64,
    /// The result identity: the five member card ids, ascending.
    pub ids: [i64; 5],
    pub leader_id: i64,
    /// Canonical Snap keys in slot order.
    pub snap_ids: [SnapKey; 5],
    /// Performance order: `order[k]` is the slot at position k.
    pub order: [usize; 5],
    /// Pool indexes, slot order.
    pub members: [usize; 5],
    pub snaps: [Option<usize>; 5],
}

impl Entry {
    /// Canonical key, better first: objective desc, power desc, member ids asc, leader id asc, snap ids asc,
    /// performance order asc.
    fn better_than(&self, o: &Entry) -> bool {
        (o.value, o.power, self.ids, self.leader_id, self.snap_ids, self.order)
            < (self.value, self.power, o.ids, o.leader_id, o.snap_ids, o.order)
    }
}

/// The identity of a result (one function so another identity rule can be added in one place).
pub(crate) fn identity(e: &Entry) -> [i64; 5] {
    e.ids
}

#[derive(Clone, Debug)]
pub(crate) struct TopK {
    k: usize,
    items: Vec<Entry>,
}

impl TopK {
    pub fn new(k: usize) -> TopK {
        // K is a result limit, not a promise that the pool contains K sets.
        // Allocate as candidates arrive; usize::MAX also means "all results".
        TopK { k, items: Vec::new() }
    }

    /// The K-th objective value once K results are held: a branch whose bound is below it cannot enter.
    pub fn threshold(&self) -> i64 {
        if self.items.len() < self.k { i64::MIN } else { self.items[self.k - 1].value }
    }

    pub fn insert(&mut self, e: Entry) {
        if self.k == 0 {
            return;
        }
        if let Some(pos) = self.items.iter().position(|x| identity(x) == identity(&e)) {
            if !e.better_than(&self.items[pos]) {
                return;
            }
            self.items.remove(pos);
        }
        let pos = self.items.iter().position(|x| e.better_than(x)).unwrap_or(self.items.len());
        if pos >= self.k {
            return;
        }
        self.items.insert(pos, e);
        self.items.truncate(self.k);
    }

    pub fn into_vec(self) -> Vec<Entry> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, SnapKey, TopK};

    #[test]
    fn real_snap_ids_precede_empty_slots_before_performance_order() {
        let empty = Entry {
            value: 1,
            power: 1,
            ids: [1, 2, 3, 4, 5],
            leader_id: 1,
            snap_ids: [SnapKey::Empty; 5],
            order: [0, 1, 2, 3, 4],
            members: [1, 2, 0, 3, 4],
            snaps: [None; 5],
        };
        for id in [i64::MIN, -1, 0, i64::MAX] {
            let mut paired = empty.clone();
            paired.snap_ids[0] = SnapKey::from(Some(id));
            paired.snaps[0] = Some(0);
            paired.order = [4, 3, 2, 1, 0];
            for candidates in [[empty.clone(), paired.clone()], [paired.clone(), empty.clone()]] {
                let mut top = TopK::new(1);
                for candidate in candidates {
                    top.insert(candidate);
                }
                assert_eq!(top.into_vec(), vec![paired.clone()]);
            }
        }
    }
}
