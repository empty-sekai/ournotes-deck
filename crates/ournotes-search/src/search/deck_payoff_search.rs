//! Canonical leader/member-Snap team Top-K by the deck payoff bound of `deck_payoff.rs`.
//!
//! Members branch in `SLOTS` order with ascending nonleader IDs, so each canonical member layout is visited once, and
//! a complete layout offers its exact frontier of Snap bindings. The ranking (payoff, then power, then canonical IDs)
//! needs no evaluation: the search keeps it alone and evaluates only the decks it ranks first, in rank order. A
//! deck-determined payoff must be reproduced exactly. A bounded payoff (score steps) may fall short: after that first
//! shortfall the joint traversal takes over, re-ranking the whole domain against the evaluated incumbents.
use super::*;
use crate::domain::CandidateDomain;
use crate::search::deck_payoff::DeckPayoffBounds;
use crate::search::team_power::SLOTS;

type Key = ([i64; 5], [Option<i64>; 5]);

/// A ranked team: payoff numerator over the engine's order mass (its bound until evaluated), power, canonical key.
struct Ranked {
    payoff: i128,
    power: i64,
    key: Key,
    deck: PhysicalDeck,
}

impl Ranked {
    fn ahead_of(&self, other: &Ranked) -> bool {
        (self.payoff, self.power) > (other.payoff, other.power)
            || ((self.payoff, self.power) == (other.payoff, other.power) && self.key < other.key)
    }
}

/// A complete power-ranked prefix settles Maximum only when every member attains the common cap.
/// If fewer than K legal teams exist, evaluating every returned team exhausts the domain instead.
pub(super) fn solve_upper_only(
    domain: &CandidateDomain,
    bounds: &DeckPayoffBounds,
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    let cap = bounds.upper_only().ok_or_else(|| Error::Domain("missing common terminal upper bound".into()))?;
    debug_assert!(e.live && e.aggregation == Aggregation::Maximum && e.certified.is_none());
    e.tel.joint.modules.entry("maximumPowerCap").or_default().checks += 1;
    let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    e.rec.frontier.clear();
    (e.rec.tracked, e.rec.bounded) = (true, true);
    e.note_open(Some(cap));
    let mut top = Vec::new();
    visit(0, &mut p, domain, bounds, 1, &mut top, e)?;
    if e.stop.is_some() {
        return stopped(e, Some(cap));
    }
    let exhausted = top.len() < e.request.k;
    for row in top {
        if !e.consider(row.deck)? {
            return stopped(e, Some(cap));
        }
        let Some(offered) = e.offered.take().or_else(|| e.recorded(&row.deck)) else {
            return Ok(false);
        };
        let Some((payoff, denominator)) = offered.payoff else { return Ok(false) };
        if denominator != 1 || i64::from(offered.power) != row.power || payoff > cap {
            return Err(Error::Domain("maximum evaluation disagrees with the power-ranked terminal bound".into()));
        }
        e.seeded.insert(row.deck);
        if !exhausted && payoff < cap {
            return Ok(false);
        }
    }
    e.tel.joint.modules.entry("maximumPowerCap").or_default().pruned += 1;
    Ok(true)
}

/// Ok(false) when the joint traversal is to take over (only with `joint_follows`).
///
/// One ranking, then the first `K` decks are evaluated in rank order. A deck that pays less than its bound makes the
/// ranking optimistic, so nothing below it can be settled without exact values: the joint traversal takes over and
/// re-ranks the whole domain against the incumbents already evaluated.
pub(super) fn solve(
    domain: &CandidateDomain,
    bounds: &DeckPayoffBounds,
    e: &mut Engine<'_, '_>,
    joint_follows: bool,
) -> Result<bool, Error> {
    let scale = if e.live && e.aggregation == Aggregation::Expected { ORDERS as i128 } else { 1 };
    let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
    let root = bounds.upper(e.pool, domain, &p, 0)?.map(|(payoff, _)| payoff * scale);
    e.rec.frontier.clear();
    (e.rec.tracked, e.rec.bounded) = (true, true);
    e.note_open(root);
    e.tel.environment.bounds.deck_payoff.get_or_insert_with(Default::default).rounds = 1;
    let mut top = Vec::new();
    visit(0, &mut p, domain, bounds, scale, &mut top, e)?;
    if e.stop.is_some() {
        return stopped(e, root);
    }
    for row in top {
        if !e.consider(row.deck)? {
            return stopped(e, root);
        }
        let Some(offered) = e.offered.take().or_else(|| e.recorded(&row.deck)) else {
            // This known deck is outside Top-K, but its optimistic rank may still precede an unseen winner. No
            // retained exact payoff means a bounded ranking cannot close on this row. The joint traversal retains
            // the incumbents and proves every remaining candidate against them; an exact payoff cannot move below
            // the K decks already retained.
            if bounds.bounded_only() {
                if joint_follows {
                    return hand_over(e);
                }
                return Err(Error::Domain("unsettled deck payoff without a joint traversal".into()));
            }
            continue;
        };
        let mismatch = || {
            Error::Domain(format!(
                "deck payoff bound predicted payoff {}/{scale} and power {} for {:?}, the evaluation differs",
                row.payoff, row.power, row.key
            ))
        };
        if i64::from(offered.power) != row.power {
            return Err(mismatch());
        }
        if let (Some(cap), Some(score)) = (bounds.score_cap(row.power), offered.score)
            && i128::from(score) > cap
        {
            return Err(Error::Domain(format!("score {score} above the score cap {cap} of {:?}", row.key)));
        }
        // The evaluated payoff fraction against the bound numerator over the aggregation's scale.
        let paid = offered.payoff.and_then(|(numerator, denominator)| {
            let denominator = i128::try_from(denominator).ok()?;
            Some((numerator.checked_mul(scale)?, row.payoff.checked_mul(denominator)?, denominator))
        });
        match paid {
            Some((paid, bound, _)) if paid == bound => {}
            Some((paid, bound, _)) if paid < bound && bounds.bounded_only() && e.certified.is_none() => {
                // The ranking was optimistic for this deck, so no later ranking can settle the Top-K without exact
                // values. The joint traversal re-ranks the whole domain against the incumbents evaluated so far.
                e.tel.environment.bounds.deck_payoff.get_or_insert_with(Default::default).shortfalls += 1;
                if joint_follows {
                    return hand_over(e);
                }
                return Err(Error::Domain("bounded deck payoff without a joint traversal".into()));
            }
            Some((paid, bound, _)) if paid > bound => return Err(mismatch()),
            // A certified payoff below its bound, or not settled: the joint traversal ranks it.
            _ if bounds.bounded_only() && joint_follows => return hand_over(e),
            _ => return Err(mismatch()),
        }
    }
    Ok(true)
}

/// The root cap covers every unvisited branch and every ranked team not yet evaluated.
fn stopped(e: &mut Engine<'_, '_>, root: Option<i128>) -> Result<bool, Error> {
    let started = e.bound_start();
    e.fold_unexplored(root, started);
    Ok(true)
}

fn hand_over(e: &mut Engine<'_, '_>) -> Result<bool, Error> {
    e.tel.environment.bounds.deck_payoff.get_or_insert_with(Default::default).handed_over = true;
    Ok(false)
}

fn visit(
    depth: usize,
    p: &mut PhysicalDeck,
    domain: &CandidateDomain,
    bounds: &DeckPayoffBounds,
    scale: i128,
    top: &mut Vec<Ranked>,
    e: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    e.tel.nodes += 1;
    if e.expired() {
        return Ok(false);
    }
    let upper = bounds.upper(e.pool, domain, p, depth)?;
    let k = e.request.k;
    let count = e.tel.joint.modules.entry("deckPayoff").or_default();
    count.checks += 1;
    let Some((payoff, power)) = upper else {
        count.pruned += 1;
        return Ok(true);
    };
    let payoff = payoff * scale;
    if top.len() == k {
        let kth = &top[k - 1];
        // A payoff bound that only ties the K-th's leaves the power to decide. Assigning the Snap terms of the
        // remaining slots, rather than bounding each column alone, can then rule out every completion that could
        // reach it, and the tie falls to the canonical key.
        let power = if payoff == kth.payoff {
            bounds.power.assigned_upper(e.pool, domain, p, depth).map_or(power, |assigned| power.min(assigned))
        } else {
            power
        };
        // Equal payoff and power keep every completion whose canonical key may still come first.
        if (payoff, power) < (kth.payoff, kth.power)
            || ((payoff, power) == (kth.payoff, kth.power)
                && bounds.power.least_key(e.pool, domain, p, depth).is_some_and(|key| key > kth.key))
        {
            count.pruned += 1;
            return Ok(true);
        }
    }
    if depth == 5 {
        let bare = PhysicalDeck { members: p.members, snaps: [None; 5] };
        let base = e.pool.deck_power(&bare.as_deck(), e.song, e.event)?.power();
        for (bound, power, deck) in bounds.frontier(domain, p, i64::from(base), k)? {
            let payoff = bound * scale;
            let key = (deck.members.map(|m| e.pool.members[m].id), deck.snaps.map(|s| s.map(|s| e.pool.snaps[s].id)));
            let row = Ranked { payoff, power, key, deck };
            let at = top.partition_point(|t| t.ahead_of(&row));
            if at < k {
                top.insert(at, row);
                top.truncate(k);
            }
        }
        return Ok(true);
    }
    let slot = SLOTS[depth];
    for (index, &member) in bounds.members.iter().enumerate() {
        if index % 32 == 31 && e.expired() {
            return Ok(false);
        }
        if slot == 2 && domain.leader().is_some_and(|m| m != member) {
            continue;
        }
        if depth > 1 && e.pool.members[member].id <= e.pool.members[p.members[SLOTS[depth - 1]]].id {
            continue;
        }
        let c = e.pool.members[member].character_id;
        if SLOTS[..depth].iter().any(|&s| e.pool.members[p.members[s]].character_id == c)
            || domain.required().iter().any(|&m| m != member && e.pool.members[m].character_id == c)
        {
            continue;
        }
        p.members[slot] = member;
        e.rec.frontier.set(depth, index, bounds.members.len());
        if !visit(depth + 1, p, domain, bounds, scale, top, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}
