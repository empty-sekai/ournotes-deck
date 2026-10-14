//! The bounds of a domain under clusters of the lottery laws of its teams.

use super::*;
use ournotes_sim::num::FxHashMap;
use std::rc::Rc;

/// The most clusters a domain's laws split into (one bit each in a cluster set).
pub(crate) const MAX_CLUSTERS: usize = 64;

/// The kind index of a member and choice that takes no lottery kind.
pub(crate) const NO_KIND: u32 = u32::MAX;

/// The bounds of a domain compiled under clusters of the lottery laws of its multisets of lottery kinds
/// ([`JointBounds::compile_with_laws`]). The laws of every multiset lie in a cluster, whose bounds then cover every
/// team holding that multiset; each multiset belongs to one such cluster, and the traversal of a cluster's bounds
/// visits the teams of its multisets (see [`JointBounds::part_allows`]).
pub(crate) struct LawClusters {
    /// The bounds of each cluster.
    pub(crate) bounds: Vec<JointBounds>,
}

/// What places a prefix of a team among the clusters.
pub(crate) struct LawTables {
    /// The lottery kind index of each pool member with each choice (0 = no Snap, `j + 1` = domain Snap `j`), or
    /// `NO_KIND`; empty for a member outside the domain.
    kinds: Vec<Box<[u32]>>,
    /// The clusters (bits) of the multisets that contain each sorted multiset of one to four kind indices, by its
    /// [`pack`] key.
    prefixes: FxHashMap<u64, u64>,
    /// The cluster each multiset of five kinds belongs to, by its [`pack`] key.
    teams: FxHashMap<u64, u8>,
}

/// The cluster of the laws a traversal's bounds weigh: it visits the teams whose multiset of kinds belongs to it.
pub(crate) struct LawPart {
    tables: Rc<LawTables>,
    cluster: usize,
}

/// A sorted multiset of at most five kind indices below 4096 as one key; None when an index is larger.
pub(crate) fn pack(kinds: &[usize]) -> Option<u64> {
    kinds.iter().try_fold(kinds.len() as u64, |key, &k| (k < 1 << 12).then_some((key << 12) | k as u64))
}

impl LawTables {
    /// The kinds of each pool member by choice, the cluster sets of the sub-multisets and the cluster of each multiset
    /// of five kinds, as in the fields of the same names.
    pub(crate) fn new(kinds: Vec<Box<[u32]>>, prefixes: FxHashMap<u64, u64>, teams: FxHashMap<u64, u8>) -> LawTables {
        LawTables { kinds, prefixes, teams }
    }

    /// The kind of pool member `m` with `choice`, None when it takes none.
    fn kind(&self, m: usize, choice: usize) -> Option<usize> {
        self.kinds.get(m).and_then(|row| row.get(choice)).copied().filter(|&k| k != NO_KIND).map(|k| k as usize)
    }
}

/// The sorted kinds of a prefix of a traversal of the bounds of a cluster (see [`JointBounds::part_prefix`]); None
/// when one of its slots takes no kind.
pub(crate) struct PartPrefix(Option<([usize; 5], usize)>);

impl LawClusters {
    /// The clusters of the bounds `bounds` (at most [`MAX_CLUSTERS`]) with their tables.
    pub(crate) fn new(mut bounds: Vec<JointBounds>, tables: LawTables) -> LawClusters {
        assert!((1..=MAX_CLUSTERS).contains(&bounds.len()), "law clusters outside the cluster set width");
        let tables = Rc::new(tables);
        for (cluster, b) in bounds.iter_mut().enumerate() {
            b.part = Some(LawPart { tables: tables.clone(), cluster });
        }
        LawClusters { bounds }
    }
}

impl JointBounds {
    /// With bounds of a cluster of laws: the sorted kinds of the first `depth` search slots of `p` (`choices` by
    /// slot), which [`JointBounds::part_allows`] reads for each child of the prefix; None without a cluster.
    pub(crate) fn part_prefix(&self, p: &PhysicalDeck, depth: usize, choices: &[usize; 5]) -> Option<PartPrefix> {
        let part = self.part.as_ref()?;
        let mut kinds = [0usize; 5];
        for (n, &slot) in SLOTS[..depth].iter().enumerate() {
            let Some(kind) = part.tables.kind(p.members[slot], choices[slot]) else { return Some(PartPrefix(None)) };
            kinds[n] = kind;
        }
        kinds[..depth].sort_unstable();
        Some(PartPrefix(Some((kinds, depth))))
    }

    /// With bounds of a cluster of laws: whether the traversal visits the child of the prefix of `depth < 5` slots
    /// whose kinds are `prefix` that takes `m` with `choice` in the next slot: some multiset of the cluster contains
    /// its kinds, and a complete team's multiset belongs to the cluster. Always without a cluster; an error for a
    /// team whose kinds the census did not read.
    pub(crate) fn part_allows(
        &self,
        prefix: Option<&PartPrefix>,
        depth: usize,
        m: usize,
        choice: usize,
    ) -> Result<bool, Error> {
        let (Some(part), Some(prefix)) = (&self.part, prefix) else { return Ok(true) };
        let outside = || Error::Domain("a team outside the lottery census of its domain".into());
        let (Some((mut kinds, n)), Some(kind)) = (prefix.0, part.tables.kind(m, choice)) else {
            return if depth == 4 { Err(outside()) } else { Ok(true) };
        };
        // the child's kind into the sorted prefix
        let mut at = n;
        while at > 0 && kinds[at - 1] > kind {
            kinds[at] = kinds[at - 1];
            at -= 1;
        }
        kinds[at] = kind;
        let n = n + 1;
        let key = pack(&kinds[..n]);
        if n == 5 {
            let cluster = key.and_then(|key| part.tables.teams.get(&key)).ok_or_else(outside)?;
            return Ok(*cluster as usize == part.cluster);
        }
        Ok(key.is_none_or(|key| part.tables.prefixes.get(&key).is_some_and(|set| set >> part.cluster & 1 != 0)))
    }
}
