//! Canonical Top-K with one result per member set.

/// Snap id stand-in for "no snap" in the canonical order (after every snap).
pub(crate) const NO_SNAP: i64 = i64::MAX;

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
    /// Snap ids in slot order (`NO_SNAP` for an empty slot).
    pub snap_ids: [i64; 5],
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
        TopK { k, items: Vec::with_capacity(k + 1) }
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
