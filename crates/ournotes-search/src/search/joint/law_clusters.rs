//! The bounds of a domain under clusters of the lottery laws of its teams.

use super::*;
use crate::search::certified_search::LuckLaw;
use std::sync::Arc;

/// The most clusters a domain's laws split into (one bit each in a cluster set).
pub(crate) const MAX_CLUSTERS: usize = 64;

/// The kind index of a member and choice that takes no lottery kind.
pub(crate) const NO_KIND: u32 = u32::MAX;

/// The bounds of a domain compiled under clusters of the lottery laws of its multisets of lottery kinds
/// ([`JointBounds::compile_with_laws`]). The laws of every multiset lie in one cluster, whose bounds then cover every
/// team holding that multiset: the completions of a prefix are below the K-th when the bounds of every cluster that
/// holds a multiset containing the prefix's kinds prove them below.
pub(crate) struct LawClusters {
    /// The bounds of each cluster.
    pub(crate) bounds: Vec<JointBounds>,
    /// The lottery kind index of each pool member with each choice (0 = no Snap, `j + 1` = domain Snap `j`), or
    /// `NO_KIND`; empty for a member outside the domain.
    kinds: Vec<Box<[u32]>>,
    /// The clusters (bits) whose law sets hold each law, by its address.
    holding: HashMap<usize, u64>,
    /// The clusters (bits) of the multisets that contain each sorted multiset of one to four kind indices, by its
    /// [`pack`] key.
    prefixes: HashMap<u64, u64>,
}

/// A sorted multiset of at most four kind indices below 4096 as one key; None when an index is larger.
pub(crate) fn pack(kinds: &[usize]) -> Option<u64> {
    kinds.iter().try_fold(kinds.len() as u64, |key, &k| (k < 1 << 12).then_some((key << 12) | k as u64))
}

impl LawClusters {
    /// The clusters of the bounds `bounds` (at most [`MAX_CLUSTERS`]), with the kinds, law sets and prefix sets of
    /// the fields of the same names.
    pub(crate) fn new(
        bounds: Vec<JointBounds>,
        kinds: Vec<Box<[u32]>>,
        holding: HashMap<usize, u64>,
        prefixes: HashMap<u64, u64>,
    ) -> LawClusters {
        assert!((1..=MAX_CLUSTERS).contains(&bounds.len()), "law clusters outside the cluster set width");
        LawClusters { bounds, kinds, holding, prefixes }
    }

    fn every(&self) -> u64 {
        u64::MAX >> (MAX_CLUSTERS - self.bounds.len())
    }

    /// The clusters (bits) whose bounds cover the completions of prefix `p` of `depth < 5` slots (`choices` by slot):
    /// those holding a multiset of kinds that contains the kinds of its slots; every cluster when a slot takes no
    /// kind or no such multiset was read.
    pub(crate) fn completions(&self, p: &PhysicalDeck, depth: usize, choices: &[usize; 5]) -> u64 {
        let n = depth.min(4);
        let mut kinds = [0usize; 4];
        for (k, &slot) in SLOTS[..n].iter().enumerate() {
            match self.kinds.get(p.members[slot]).and_then(|row| row.get(choices[slot])) {
                Some(&kind) if kind != NO_KIND => kinds[k] = kind as usize,
                _ => return self.every(),
            }
        }
        let prefix = &mut kinds[..n];
        prefix.sort_unstable();
        pack(prefix).and_then(|key| self.prefixes.get(&key)).copied().unwrap_or_else(|| self.every())
    }

    /// The clusters (bits) whose law sets hold every law of `laws` (none for no laws).
    pub(crate) fn holding(&self, laws: &[LuckLaw]) -> u64 {
        if laws.is_empty() {
            return 0;
        }
        let of = |law: &LuckLaw| self.holding.get(&(Arc::as_ptr(law) as usize)).copied().unwrap_or(0);
        laws.iter().fold(self.every(), |clusters, law| clusters & of(law))
    }

    /// Renumbers the clusters: cluster `order[i]` becomes cluster `i`.
    pub(crate) fn reorder(&mut self, order: &[usize]) {
        let mut to = vec![0; order.len()];
        for (i, &old) in order.iter().enumerate() {
            to[old] = i;
        }
        let mut bounds: Vec<Option<JointBounds>> = self.bounds.drain(..).map(Some).collect();
        self.bounds = order.iter().map(|&old| bounds[old].take().expect("a permutation")).collect();
        let renumber = |set: &mut u64| {
            let old = *set;
            *set = (0..to.len()).filter(|&c| old >> c & 1 != 0).fold(0, |new, c| new | 1 << to[c]);
        };
        self.holding.values_mut().for_each(renumber);
        self.prefixes.values_mut().for_each(renumber);
    }
}
