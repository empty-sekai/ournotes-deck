//! The bounds of a domain under clusters of the lottery laws of its teams.

use super::*;
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
    tables: Rc<LawTables>,
}

/// What places a prefix of a team among the clusters.
pub(crate) struct LawTables {
    /// The lottery kind index of each pool member with each choice (0 = no Snap, `j + 1` = domain Snap `j`), or
    /// `NO_KIND`; empty for a member outside the domain.
    kinds: Vec<Box<[u32]>>,
    /// The clusters (bits) of the multisets that contain each sorted multiset of one to four kind indices, by its
    /// [`pack`] key.
    prefixes: HashMap<u64, u64>,
    /// The cluster each multiset of five kinds belongs to, by its [`pack`] key.
    teams: HashMap<u64, u8>,
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
    pub(crate) fn new(kinds: Vec<Box<[u32]>>, prefixes: HashMap<u64, u64>, teams: HashMap<u64, u8>) -> LawTables {
        LawTables { kinds, prefixes, teams }
    }

    /// The sorted kinds of the first `depth` search slots of `p` (choice of each slot by slot), and of `extra` when
    /// given; None when one takes no kind.
    fn prefix(
        &self,
        p: &PhysicalDeck,
        depth: usize,
        choices: &[usize; 5],
        extra: Option<(usize, usize)>,
    ) -> Option<([usize; 5], usize)> {
        let kind = |m: usize, choice: usize| {
            self.kinds.get(m).and_then(|row| row.get(choice)).copied().filter(|&k| k != NO_KIND).map(|k| k as usize)
        };
        let mut kinds = [0usize; 5];
        let mut n = 0;
        for &slot in &SLOTS[..depth] {
            kinds[n] = kind(p.members[slot], choices[slot])?;
            n += 1;
        }
        if let Some((m, choice)) = extra {
            kinds[n] = kind(m, choice)?;
            n += 1;
        }
        kinds[..n].sort_unstable();
        Some((kinds, n))
    }
}

impl LawClusters {
    /// The clusters of the bounds `bounds` (at most [`MAX_CLUSTERS`]) with their tables.
    pub(crate) fn new(mut bounds: Vec<JointBounds>, tables: LawTables) -> LawClusters {
        assert!((1..=MAX_CLUSTERS).contains(&bounds.len()), "law clusters outside the cluster set width");
        let tables = Rc::new(tables);
        for (cluster, b) in bounds.iter_mut().enumerate() {
            b.part = Some(LawPart { tables: tables.clone(), cluster });
        }
        LawClusters { bounds, tables }
    }

    /// Renumbers the clusters: cluster `order[i]` becomes cluster `i`.
    pub(crate) fn reorder(&mut self, order: &[usize]) {
        let mut to = vec![0; order.len()];
        for (i, &old) in order.iter().enumerate() {
            to[old] = i;
        }
        let mut bounds: Vec<Option<JointBounds>> = self.bounds.drain(..).map(Some).collect();
        self.bounds = order.iter().map(|&old| bounds[old].take().expect("a permutation")).collect();
        self.bounds.iter_mut().for_each(|b| b.part = None);
        let tables = Rc::get_mut(&mut self.tables).expect("law tables held by their clusters alone");
        for set in tables.prefixes.values_mut() {
            let old = *set;
            *set = (0..to.len()).filter(|&c| old >> c & 1 != 0).fold(0, |new, c| new | 1 << to[c]);
        }
        tables.teams.values_mut().for_each(|c| *c = to[*c as usize] as u8);
        for (cluster, b) in self.bounds.iter_mut().enumerate() {
            b.part = Some(LawPart { tables: self.tables.clone(), cluster });
        }
    }
}

impl JointBounds {
    /// With bounds of a cluster of laws: whether the traversal visits the child of prefix `p` of `depth < 5` slots
    /// (`choices` by slot) that takes `m` with `choice` in the next slot: some multiset of the cluster contains its
    /// kinds, and a complete team's multiset belongs to the cluster. Always without a cluster; an error for a team
    /// whose kinds the census did not read.
    pub(crate) fn part_allows(
        &self,
        p: &PhysicalDeck,
        depth: usize,
        choices: &[usize; 5],
        m: usize,
        choice: usize,
    ) -> Result<bool, Error> {
        let Some(part) = &self.part else { return Ok(true) };
        let outside = || Error::Domain("a team outside the lottery census of its domain".into());
        let Some((kinds, n)) = part.tables.prefix(p, depth, choices, Some((m, choice))) else {
            return if depth == 4 { Err(outside()) } else { Ok(true) };
        };
        let key = pack(&kinds[..n]);
        if n == 5 {
            let cluster = key.and_then(|key| part.tables.teams.get(&key)).ok_or_else(outside)?;
            return Ok(*cluster as usize == part.cluster);
        }
        Ok(key.is_none_or(|key| part.tables.prefixes.get(&key).is_some_and(|set| set >> part.cluster & 1 != 0)))
    }
}
