//! The physical slots of a team in search order: the leader first, then the four other members.

/// Leader first, then the other physical slots. Performance order is never a decision.
pub(crate) const SLOTS: [usize; 5] = [2, 0, 1, 3, 4];
