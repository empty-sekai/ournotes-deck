//! Event bonuses, score ranks and event points, as the game client computes them.
//!
//! The awarded event points are decided by the game server; the functions here reproduce the client's own
//! computation (the value it shows until it syncs with the server). No event has run on the current master data, so
//! these functions are exercised with synthetic tables.

use crate::error::Error;
use crate::master::{EventEffectRow, Master};
use crate::num::floor_to_i32;

/// Event bonus kinds.
pub const EVENT_POINT: i64 = 0;
pub const EVENT_ITEM: i64 = 1;
pub const PARAMETER_ALL: i64 = 2;

const RESOURCE_MEMBER_CARD: i64 = 2;
const RESOURCE_SUPPORT_CARD: i64 = 3;

/// Score ranks.
pub const RANK_NONE: i64 = 0;
pub const RANK_E: i64 = 1;
pub const RANK_D: i64 = 2;
pub const RANK_C: i64 = 3;
pub const RANK_B: i64 = 4;
pub const RANK_A: i64 = 5;
pub const RANK_S: i64 = 6;
pub const RANK_SS: i64 = 7;

/// The rank named in master parameters ("E" .. "SS").
pub fn parse_rank(name: &str) -> Result<i64, Error> {
    Ok(match name.trim() {
        "None" => RANK_NONE,
        "E" => RANK_E,
        "D" => RANK_D,
        "C" => RANK_C,
        "B" => RANK_B,
        "A" => RANK_A,
        "S" => RANK_S,
        "SS" => RANK_SS,
        other => return Err(Error::Master(format!("unknown score rank {other:?}"))),
    })
}

/// What the event bonus reads from a member card. `band_id == None`: the card's band has no master row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventMember {
    pub id: i64,
    pub character_id: i64,
    pub band_id: Option<i64>,
    pub card_type: i64,
    pub tags: Vec<i64>,
    pub rank: i64,
}

/// What the event bonus reads from a snap: the bands of its characters, in character order without repeats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSnap {
    pub id: i64,
    pub character_ids: Vec<i64>,
    pub band_ids: Vec<i64>,
    pub card_type: i64,
    pub rank: i64,
}

/// A card of either kind.
#[derive(Clone, Copy, Debug)]
pub enum EventCard<'a> {
    Member(&'a EventMember),
    Snap(&'a EventSnap),
}

/// The effect value for a card rank (ranks 1..5; any other rank is a game error).
pub fn rank_effect_value(e: &EventEffectRow, rank: i64) -> Result<i64, Error> {
    Ok(match rank {
        1 => e.rank1_effect_value,
        2 => e.rank2_effect_value,
        3 => e.rank3_effect_value,
        4 => e.rank4_effect_value,
        5 => e.rank5_effect_value,
        _ => return Err(Error::Game(format!("event effect value for rank {rank}"))),
    })
}

/// Whether an effect applies to a member card: member-card effects whose every set key matches (character, band,
/// card type, tag, member card id). A band key on a card without a band row is a game error.
pub fn is_member_target(e: &EventEffectRow, card: &EventMember) -> Result<bool, Error> {
    if e.resource_type_constraint != RESOURCE_MEMBER_CARD {
        return Ok(false);
    }
    if e.character_id > 0 && e.character_id != card.character_id {
        return Ok(false);
    }
    if e.band_id > 0 {
        match card.band_id {
            None => return Err(Error::Game("member card without a band".into())),
            Some(b) if b != e.band_id => return Ok(false),
            _ => {}
        }
    }
    if e.card_type != 0 && e.card_type != card.card_type {
        return Ok(false);
    }
    if e.tag_id > 0 && !card.tags.contains(&e.tag_id) {
        return Ok(false);
    }
    Ok(e.member_card_id < 1 || e.member_card_id == card.id)
}

/// Whether an effect applies to a snap: snap effects whose set keys match (one of its characters, one of its
/// bands, card type, snap id); an effect with a tag never applies to a snap.
pub fn is_snap_target(e: &EventEffectRow, card: &EventSnap) -> bool {
    if e.resource_type_constraint != RESOURCE_SUPPORT_CARD {
        return false;
    }
    if e.character_id > 0 && !card.character_ids.contains(&e.character_id) {
        return false;
    }
    if e.band_id > 0 && !card.band_ids.contains(&e.band_id) {
        return false;
    }
    if e.card_type != 0 && e.card_type != card.card_type {
        return false;
    }
    if e.tag_id >= 1 {
        return false;
    }
    e.support_card_id < 1 || e.support_card_id == card.id
}

/// An event's effect rows (master order).
pub fn event_effects(master: &Master, event_id: i64) -> Vec<&EventEffectRow> {
    master.event_effects.iter().filter(|e| e.event_id == event_id).collect()
}

/// The 10000-scale bonus of one card from one event's effects of `bonus_type` (32-bit wrapping sum). `None`: 0.
pub fn effect_10000(effects: &[&EventEffectRow], card: Option<EventCard>, bonus_type: i64) -> Result<i32, Error> {
    let Some(card) = card else { return Ok(0) };
    let mut total = 0i32;
    for e in effects {
        if e.event_bonus_type != bonus_type {
            continue;
        }
        let (hit, rank) = match card {
            EventCard::Member(m) => (is_member_target(e, m)?, m.rank),
            EventCard::Snap(s) => (is_snap_target(e, s), s.rank),
        };
        if hit {
            total = total.wrapping_add(rank_effect_value(e, rank)? as i32);
        }
    }
    Ok(total)
}

/// Sum over events.
pub fn total_effect_10000(
    events: &[Vec<&EventEffectRow>],
    card: Option<EventCard>,
    bonus_type: i64,
) -> Result<i32, Error> {
    let mut total = 0i32;
    for effects in events {
        total = total.wrapping_add(effect_10000(effects, card, bonus_type)?);
    }
    Ok(total)
}

/// Deck total: the members, then the snaps. A missing snap list is a game error (raised after the members).
pub fn total_effect_10000_deck(
    events: &[Vec<&EventEffectRow>],
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
    bonus_type: i64,
) -> Result<i32, Error> {
    let mut total = 0i32;
    for m in members {
        total = total.wrapping_add(total_effect_10000(events, m.map(EventCard::Member), bonus_type)?);
    }
    let snaps = snaps.ok_or_else(|| Error::Game("snap list missing".into()))?;
    for s in snaps {
        total = total.wrapping_add(total_effect_10000(events, s.map(EventCard::Snap), bonus_type)?);
    }
    Ok(total)
}

/// The event-point bonus of a deck (kind 0), 10000 = 100 %.
pub fn event_point_bonus_10000(
    events: &[Vec<&EventEffectRow>],
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
) -> Result<i32, Error> {
    total_effect_10000_deck(events, members, snaps, EVENT_POINT)
}

/// The displayed percentage: `floor((float)x / 100f)`.
pub fn as_percentage(value_10000: i32) -> i32 {
    floor_to_i32(value_10000 as f32 / 100f32)
}

/// Boost rates `(live music reward, player exp, member exp, friendship exp, event point)`: all 1 below one boost,
/// else the boost row of that count (a missing row is a master error).
pub fn boost_bonus(master: &Master, consumed: i64) -> Result<[i64; 5], Error> {
    if consumed < 1 {
        return Ok([1; 5]);
    }
    master
        .live_music_boost_bonuses
        .iter()
        .find(|r| r.consumed_count == consumed)
        .map(|r| r.rates())
        .ok_or_else(|| Error::Master(format!("no boost row for {consumed}")))
}

/// Challenge boost rates: all 1 up to 200 challenge points, else the row of that count.
pub fn challenge_point_bonus(master: &Master, consumed: i64) -> Result<[i64; 5], Error> {
    if consumed < 201 {
        return Ok([1; 5]);
    }
    master
        .challenge_music_boost_bonuses
        .iter()
        .find(|r| r.consumed_count == consumed)
        .map(|r| r.rates())
        .ok_or_else(|| Error::Master(format!("no challenge boost row for {consumed}")))
}

pub fn is_challenge_live_boost(consumed: i64) -> bool {
    consumed > 200
}

/// Score-rank rows of a group in required-score order (stable).
pub fn rank_rows_of_group(master: &Master, group: i64) -> Vec<&crate::master::LiveScoreRankRow> {
    let mut v: Vec<_> = master.live_score_ranks.iter().filter(|r| r.group == group).collect();
    v.sort_by_key(|r| r.required_score);
    v
}

/// The score rank of a score: the last row (in required-score order) whose required score is reached. None
/// reached is a game error.
pub fn score_rank(master: &Master, group: i64, score: i64) -> Result<i64, Error> {
    let mut hit = None;
    for r in rank_rows_of_group(master, group) {
        if r.required_score <= score {
            hit = Some(r.live_score_rank);
        }
    }
    hit.ok_or_else(|| Error::Game(format!("no score rank for group {group} score {score}")))
}

/// Battle threshold for `players` connected players: `trunc(sqrt(5 / n) * base * n)` in double, saturating.
pub fn battle_required_score(base: i64, players: i64) -> i32 {
    if players < 1 {
        return i32::MAX;
    }
    let d = ((5.0 / players as f64).sqrt() * base as f64) * players as f64;
    if d == f64::INFINITY {
        return i32::MIN;
    }
    d as i32
}

/// Battle rank of a total score: walk the group's rows in order while the threshold is reached; E counts as D.
pub fn battle_score_rank(master: &Master, group: i64, total_score: i64, players: i64) -> i64 {
    let mut rank = RANK_NONE;
    for r in rank_rows_of_group(master, group) {
        if total_score < battle_required_score(r.battle_live_required_score, players) as i64 {
            break;
        }
        rank = r.live_score_rank;
    }
    if rank == RANK_E { RANK_D } else { rank }
}

/// Number of players that count for the battle threshold (not disconnected).
pub fn rank_target_player_count(disconnected: &[bool]) -> i64 {
    disconnected.iter().filter(|d| !**d).count() as i64
}

/// The rank shown during a live: the first of SS, S, A, B, C, D whose threshold (missing: 0) is reached, else D.
pub fn live_rank_table_rank(master: &Master, group: i64, score: i64) -> i64 {
    let rows = rank_rows_of_group(master, group);
    let threshold = |rank: i64| {
        let mut v = None;
        for r in &rows {
            if r.live_score_rank == rank {
                v = Some(r.required_score);
            }
        }
        v.unwrap_or(0)
    };
    for r in [RANK_SS, RANK_S, RANK_A, RANK_B, RANK_C, RANK_D] {
        if threshold(r) <= score {
            return r;
        }
    }
    RANK_D
}

/// Points of a score rank for an event: the first live event point row of the event's group with that rank.
pub fn music_score_event_point(master: &Master, group: i64, rank: i64) -> Option<i64> {
    master.live_event_points.iter().find(|r| r.group == group && r.score_rank == rank).map(|r| r.value)
}

/// Points of a score rank in a challenge live (the event's challenge group).
pub fn challenge_live_event_point(master: &Master, group: i64, rank: i64) -> Option<i64> {
    master.challenge_live_event_points.iter().find(|r| r.group == group && r.score_rank == rank).map(|r| r.value)
}

/// Challenge points of a score rank: the first row with that rank in the whole table.
pub fn music_score_challenge_point(master: &Master, rank: i64) -> Option<i64> {
    master.live_challenge_points.iter().find(|r| r.score_rank == rank).map(|r| r.value)
}

/// Event points of one live for one event: `((bonus + 10000) * rate * value) / 10000` with 32-bit wrapping
/// products and truncating division.
pub fn live_event_point(bonus_10000: i32, event_point_rate: i64, value: i64) -> i32 {
    bonus_10000.wrapping_add(10000).wrapping_mul(event_point_rate as i32).wrapping_mul(value as i32) / 10000
}

/// Event points of a challenge live: `(value * rate * (bonus + 10000)) / 10000` (32-bit).
pub fn challenge_live_event_point_count(bonus_10000: i32, event_point_rate: i64, value: i64) -> i32 {
    (value as i32).wrapping_mul(event_point_rate as i32).wrapping_mul(bonus_10000.wrapping_add(10000)) / 10000
}

/// Event item amount: `(count * (bonus + 10000) * rate) / 10000` (32-bit).
pub fn event_item_amount(resource_count: i64, item_bonus_10000: i32, reward_rate: i64) -> i32 {
    (resource_count as i32).wrapping_mul(item_bonus_10000.wrapping_add(10000)).wrapping_mul(reward_rate as i32) / 10000
}

/// Adds points to an event counter (0 adds nothing).
pub fn add_event_point_count(current: i32, point: i32) -> i32 {
    if point == 0 { current } else { current.wrapping_add(point) }
}

/// The player's counters of one event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalEvent {
    pub event_id: i64,
    pub points: i32,
    pub challenge_points: i32,
    /// Every point amount added, in order.
    pub added: Vec<i32>,
}

/// The event part of a normal live's result: for each held event (in order) the event points of the rank, then the
/// challenge points. Returns the points added per event.
pub fn consume_live_boost_events(
    master: &Master,
    holding_event_ids: &[i64],
    local: &mut [LocalEvent],
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
    score_rank: i64,
    consumed: i64,
) -> Result<Vec<(i64, i32)>, Error> {
    let rate = boost_bonus(master, consumed)?[4];
    let mut out = Vec::new();
    for &eid in holding_event_ids {
        let li =
            local.iter().position(|l| l.event_id == eid).ok_or_else(|| Error::Game(format!("no local event {eid}")))?;
        let ev = master.event(eid).ok_or_else(|| Error::Master(format!("unknown event {eid}")))?;
        let row = music_score_event_point(master, ev.live_event_point_group, score_rank);
        let bonus = event_point_bonus_10000(&[event_effects(master, eid)], members, snaps)?;
        let value = row.ok_or_else(|| Error::Game(format!("no event point row for event {eid} rank {score_rank}")))?;
        let p = live_event_point(bonus, rate, value);
        let l = &mut local[li];
        l.added.push(p);
        l.points = add_event_point_count(l.points, p);
        out.push((eid, p));
        let ch = music_score_challenge_point(master, score_rank)
            .ok_or_else(|| Error::Game(format!("no challenge point row for rank {score_rank}")))?;
        l.challenge_points = l.challenge_points.wrapping_add((ch as i32).wrapping_mul(rate as i32));
    }
    Ok(out)
}

/// The event-point part of a challenge live's result; `None` when the event has no row for the rank.
pub fn consume_challenge_point_event(
    master: &Master,
    local: &mut LocalEvent,
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
    score_rank: i64,
    consumption: i64,
) -> Result<Option<i32>, Error> {
    let rate = challenge_point_bonus(master, consumption)?[4];
    let ev = master.event(local.event_id).ok_or_else(|| Error::Master(format!("unknown event {}", local.event_id)))?;
    let Some(value) = challenge_live_event_point(master, ev.challenge_live_event_point_group, score_rank) else {
        return Ok(None);
    };
    let bonus = event_point_bonus_10000(&[event_effects(master, local.event_id)], members, snaps)?;
    let p = challenge_live_event_point_count(bonus, rate, value);
    local.added.push(p);
    local.points = add_event_point_count(local.points, p);
    Ok(Some(p))
}
