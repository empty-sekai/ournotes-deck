//! Exhaustive and random enumeration of the decks of a domain.

use super::*;

/// Every deck of the domain, slot by slot. For played lives the non-leader slots 0, 1, 3, 4 take ascending candidate
/// indices (`start` is the first index left to the next one), one layout per team.
#[allow(clippy::too_many_arguments)]
pub(super) fn members_rec(
    slot: usize,
    start: usize,
    p: &mut PhysicalDeck,
    candidates: &[usize],
    required: &[usize],
    leader: Option<usize>,
    snaps: &[usize],
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if required.iter().filter(|r| !p.members[..slot].contains(r)).count() > 5 - slot {
        return Ok(true);
    }
    if slot == 5 {
        return snaps_rec(0, p, snaps, e);
    }
    let first = if e.team_identity() && slot != 2 { start } else { 0 };
    for (index, &m) in candidates.iter().enumerate().skip(first) {
        if slot == 2 && leader.is_some_and(|l| l != m) {
            continue;
        }
        if p.members[..slot].iter().any(|&i| e.pool.members[i].character_id == e.pool.members[m].character_id) {
            continue;
        }
        // Picking another card of a required character can never satisfy that requirement.
        if required.iter().any(|&r| r != m && e.pool.members[r].character_id == e.pool.members[m].character_id) {
            continue;
        }
        p.members[slot] = m;
        e.rec.frontier.set(slot, index, candidates.len());
        let next = if slot == 2 { start } else { index + 1 };
        if !members_rec(slot + 1, next, p, candidates, required, leader, snaps, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn snaps_rec(slot: usize, p: &mut PhysicalDeck, snaps: &[usize], e: &mut Engine<'_, '_>) -> Result<bool, Error> {
    e.tel.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    if slot == 5 {
        return e.consider(*p);
    }
    p.snaps[slot] = None;
    e.rec.frontier.set(5 + slot, 0, snaps.len() + 1);
    if !snaps_rec(slot + 1, p, snaps, e)? {
        return Ok(false);
    }
    for (index, &s) in snaps.iter().enumerate() {
        if p.snaps[..slot].contains(&Some(s)) {
            continue;
        }
        p.snaps[slot] = Some(s);
        e.rec.frontier.set(5 + slot, index + 1, snaps.len() + 1);
        if !snaps_rec(slot + 1, p, snaps, e)? {
            return Ok(false);
        }
    }
    p.snaps[slot] = None;
    Ok(true)
}

pub(super) struct ProposalRandom(pub(super) u64);

impl ProposalRandom {
    pub(super) fn index(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

pub(super) fn random_deck(
    pool: &Pool,
    candidates: &[usize],
    required: &[usize],
    leader: Option<usize>,
    snaps: &[usize],
    r: &mut ProposalRandom,
) -> PhysicalDeck {
    let mut selected = required.to_vec();
    let offset = r.index(candidates.len());
    if selected.len() < 5 {
        for i in 0..candidates.len() {
            let m = candidates[(i + offset) % candidates.len()];
            if selected.iter().any(|&a| pool.members[a].character_id == pool.members[m].character_id) {
                continue;
            }
            selected.push(m);
            if selected.len() == 5 {
                break;
            }
        }
    }
    // Required may already contain five cards.
    selected.truncate(5);
    for i in (1..5).rev() {
        let j = r.index(i + 1);
        selected.swap(i, j);
    }
    if let Some(l) = leader {
        let at = selected.iter().position(|&i| i == l).expect("leader is required");
        selected.swap(2, at);
    }
    let mut p = PhysicalDeck { members: selected.try_into().expect("feasible five characters"), snaps: [None; 5] };
    for i in 0..5 {
        let s = r.index(snaps.len() + 1);
        if s < snaps.len() && !p.snaps[..i].contains(&Some(snaps[s])) {
            p.snaps[i] = Some(snaps[s]);
        }
    }
    p
}
