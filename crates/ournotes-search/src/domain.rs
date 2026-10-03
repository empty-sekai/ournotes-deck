//! Frozen legal candidate domain, separate from search traversal and ranking.
//! Pool indexes preserve the source roster. No quality truncation or standalone
//! Snap dominance is performed: every unique resource remains available.
use crate::search::{Constraints, expectation::PhysicalDeck};
use ournotes_sim::Error;
use ournotes_sim::pool::Pool;
use std::collections::HashSet;

/// Immutable candidate indexes and hard constraints for one built problem.
#[derive(Clone, Debug)]
pub struct CandidateDomain {
    members: Vec<usize>,
    snaps: Vec<usize>,
    required: Vec<usize>,
    leader: Option<usize>,
    allowed_members: Vec<bool>,
    feasible: bool,
}
impl CandidateDomain {
    pub(crate) fn build(pool: &Pool, constraints: &Constraints) -> Result<Self, Error> {
        let (allowed, snaps) = crate::search::resolve_allowed(pool, constraints)?;
        let mut members: Vec<_> = (0..pool.members.len()).filter(|&i| allowed.members[i]).collect();
        members.sort_by_key(|&i| pool.members[i].id);
        let mut required = allowed.required;
        if let Some(leader) = allowed.leader
            && !required.contains(&leader)
        {
            required.push(leader);
        }
        let feasible = required.len() <= 5
            && required.iter().map(|&i| pool.members[i].character_id).collect::<HashSet<_>>().len() == required.len()
            && members.iter().map(|&i| pool.members[i].character_id).collect::<HashSet<_>>().len() >= 5;
        Ok(Self { members, snaps, required, leader: allowed.leader, allowed_members: allowed.members, feasible })
    }
    /// Internal proof-restricted view; the caller must prove discarded members cannot
    /// tie or beat an already evaluated incumbent in the ORIGINAL domain.
    pub(crate) fn retain_proven_members(&self, pool: &Pool, keep: &HashSet<usize>) -> Self {
        let mut d = self.clone();
        d.members.retain(|m| keep.contains(m));
        for (m, allowed) in d.allowed_members.iter_mut().enumerate() {
            *allowed &= keep.contains(&m);
        }
        d.feasible &= d.required.iter().all(|m| keep.contains(m))
            && d.members.iter().map(|&m| pool.members[m].character_id).collect::<HashSet<_>>().len() >= 5;
        d
    }

    /// Internal partition view keeping only the Snaps `keep` accepts; the caller must also search the complement.
    pub(crate) fn retain_snaps(&self, keep: impl Fn(usize) -> bool) -> Self {
        let mut d = self.clone();
        d.snaps.retain(|&s| keep(s));
        d
    }

    /// Public-ID sorted indexes into BuiltProblem::pool().members.
    pub fn members(&self) -> &[usize] {
        &self.members
    }
    /// Public-ID sorted indexes; None is always an additional pairing choice.
    pub fn snaps(&self) -> &[usize] {
        &self.snaps
    }
    pub fn required(&self) -> &[usize] {
        &self.required
    }
    pub fn leader(&self) -> Option<usize> {
        self.leader
    }
    pub fn is_feasible(&self) -> bool {
        self.feasible
    }
    pub(crate) fn check_fixed(&self, pool: &Pool, p: &PhysicalDeck) -> Result<(), Error> {
        pool.check_deck(&p.as_deck())?;
        if p.members.iter().any(|&m| !self.allowed_members[m])
            || self.required.iter().any(|m| !p.members.contains(m))
            || self.leader.is_some_and(|leader| p.members[2] != leader)
            || p.snaps.iter().flatten().any(|snap| !self.snaps.contains(snap))
        {
            return Err(Error::Input("fixed deck violates the declared constraints".into()));
        }
        Ok(())
    }
}
