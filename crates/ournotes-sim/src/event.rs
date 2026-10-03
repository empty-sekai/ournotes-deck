//! Event bonuses, score ranks and event points, as the game client computes them.
//!
//! The awarded event points are decided by the game server; the functions here reproduce the client's own
//! computation (the value it shows until it syncs with the server). No event has run on the current master data, so
//! these functions are exercised with synthetic tables.

use serde::{Deserialize, Serialize};

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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

/// A master event window normalized to JST DateTime ticks (100ns since 0001-01-01, no Kind bits).
/// Unset end dates (null, empty, literal "null") become None in the input adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventWindow {
    pub event_id: i64,
    pub start_jst_ticks: i64,
    pub end_jst_ticks: Option<i64>,
}

impl EventWindow {
    pub fn is_holding_at(&self, jst_ticks: i64) -> bool {
        self.start_jst_ticks <= jst_ticks && self.end_jst_ticks.is_none_or(|end| jst_ticks < end)
    }
}

/// Reward clock, distinct from the power event snapshot. The adapter converts every value to JST ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum EventResultClock {
    /// Missing saved start falls back to the server-adjusted clock, not the host wall clock.
    Played {
        live_start_jst_ticks: Option<i64>,
        server_now_jst_ticks: i64,
    },
    Skip {
        server_now_jst_ticks: i64,
    },
}

impl EventResultClock {
    pub fn jst_ticks(self) -> i64 {
        match self {
            Self::Played { live_start_jst_ticks, server_now_jst_ticks } => {
                live_start_jst_ticks.unwrap_or(server_now_jst_ticks)
            }
            Self::Skip { server_now_jst_ticks } => server_now_jst_ticks,
        }
    }
}

/// Preserves supplied order. Models the predicate, not mutable cache order or backwards-clock cache behavior.
pub fn holding_event_ids(windows: &[EventWindow], clock: EventResultClock) -> Vec<i64> {
    let now = clock.jst_ticks();
    windows.iter().filter(|w| w.is_holding_at(now)).map(|w| w.event_id).collect()
}

/// Client debit: wrapping i32 subtraction, then clamp negatives to zero. Not a server eligibility check.
pub fn debit_challenge_points(current: i32, consumption: i32) -> i32 {
    current.wrapping_sub(consumption).max(0)
}

/// Challenge debit followed by its played-event points. Debit survives a later lookup/calculation error.
/// Items, EXP, claims and server synchronization are outside this function.
pub fn consume_challenge_point_and_event(
    master: &Master,
    local: &mut LocalEvent,
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
    score_rank: i64,
    consumption: i32,
) -> Result<Option<i32>, Error> {
    local.challenge_points = debit_challenge_points(local.challenge_points, consumption);
    consume_challenge_point_event(master, local, members, snaps, score_rank, i64::from(consumption))
}

/// Skip event selection, after debit and multiplier selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipEventMode {
    Normal,
    Challenge { event_id: i64 },
}

/// Skip logs a missing point row and continues, rather than inventing a row or failing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissingSkipEventPoint {
    pub event_id: i64,
    pub score_rank: i64,
    pub is_challenge: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkipEventOutcome {
    pub points: Vec<(i64, i32)>,
    pub missing_point_rows: Vec<MissingSkipEventPoint>,
}

/// Selected rank and already-resolved boost for skip event counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkipEventParams {
    pub score_rank: i64,
    pub event_point_rate: i32,
    pub mode: SkipEventMode,
}

/// Apply skip event counters in supplied held-event order; event_point_rate is tuple item 5, not item 1.
/// Skip creates missing local events. Missing point rows log and skip challenge points as well.
/// The caller owns time filtering, debit/EXP/item ordering, and reporting the returned diagnostics.
pub fn apply_skip_event_counters(
    master: &Master,
    holding_event_ids: &[i64],
    local: &mut Vec<LocalEvent>,
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
    params: SkipEventParams,
) -> Result<SkipEventOutcome, Error> {
    let SkipEventParams { score_rank, event_point_rate, mode } = params;
    let mut out = SkipEventOutcome::default();
    for &eid in holding_event_ids {
        if matches!(mode, SkipEventMode::Challenge { event_id } if event_id != eid) {
            continue;
        }
        let ev = master.event(eid).ok_or_else(|| Error::Master(format!("unknown event {eid}")))?;
        let li = match local.iter().position(|l| l.event_id == eid) {
            Some(i) => i,
            None => {
                local.push(LocalEvent { event_id: eid, ..LocalEvent::default() });
                local.len() - 1
            }
        };
        let is_challenge = matches!(mode, SkipEventMode::Challenge { .. });
        let value = if is_challenge {
            challenge_live_event_point(master, ev.challenge_live_event_point_group, score_rank)
        } else {
            music_score_event_point(master, ev.live_event_point_group, score_rank)
        };
        let Some(value) = value else {
            out.missing_point_rows.push(MissingSkipEventPoint { event_id: eid, score_rank, is_challenge });
            continue;
        };
        let bonus = event_point_bonus_10000(&[event_effects(master, eid)], members, snaps)?;
        let p = (value as i32).wrapping_mul(bonus.wrapping_add(10000)).wrapping_mul(event_point_rate) / 10000;
        let l = &mut local[li];
        l.added.push(p);
        l.points = add_event_point_count(l.points, p);
        out.points.push((eid, p));
        if !is_challenge {
            let ch = music_score_challenge_point(master, score_rank)
                .ok_or_else(|| Error::Game(format!("no challenge point row for rank {score_rank}")))?;
            l.challenge_points = l.challenge_points.wrapping_add((ch as i32).wrapping_mul(event_point_rate));
        }
    }
    Ok(out)
}

/// Distinct result pipelines. Battle and Arena use NormalPlayed, with their resolved total score rank.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum EventResultRoute {
    NormalPlayed,
    ChallengePlayed { event_id: i64 },
    NormalSkip,
    ChallengeSkip { event_id: i64 },
}

/// Frozen input to one counter preview. Power events have a separate snapshot.
/// consumed_count is the actual consumed boost count, or the selected challenge-point consumption value.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventPointRequest {
    pub route: EventResultRoute,
    pub holding_event_ids: Vec<i64>,
    pub consumed_count: i32,
    pub local_events: Vec<LocalEvent>,
}

/// Client counters only, never a claim about server-awarded inventory or claim acceptance.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventPointPreview {
    pub points: Vec<(i64, i32)>,
    pub local_events: Vec<LocalEvent>,
    pub missing_point_rows: Vec<MissingSkipEventPoint>,
}

impl EventPointPreview {
    pub fn points_for(&self, event_id: i64) -> i32 {
        self.points.iter().filter(|(id, _)| *id == event_id).fold(0i32, |sum, (_, p)| sum.wrapping_add(*p))
    }
}

/// Side-effect-free preview for a terminal-payoff closure. Every call clones the same initial local counters.
/// A missing row that the client only logs is retained in diagnostics; a fatal client path remains an error.
/// This covers event counters, not the intervening EXP, item, claim, and story-unlock side effects.
pub fn preview_client_event_points(
    master: &Master,
    request: &EventPointRequest,
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
    score_rank: i64,
) -> Result<EventPointPreview, Error> {
    let mut local = request.local_events.clone();
    let mut missing_point_rows = Vec::new();
    let consumed = request.consumed_count;
    let points = match request.route {
        EventResultRoute::NormalPlayed => consume_live_boost_events(
            master,
            &request.holding_event_ids,
            &mut local,
            members,
            snaps,
            score_rank,
            i64::from(consumed),
        )?,
        EventResultRoute::ChallengePlayed { event_id } => {
            let l = local
                .iter_mut()
                .find(|l| l.event_id == event_id)
                .ok_or_else(|| Error::Game(format!("no local event {event_id}")))?;
            match consume_challenge_point_and_event(master, l, members, snaps, score_rank, consumed)? {
                Some(p) => vec![(event_id, p)],
                None => {
                    missing_point_rows.push(MissingSkipEventPoint { event_id, score_rank, is_challenge: true });
                    Vec::new()
                }
            }
        }
        route @ (EventResultRoute::NormalSkip | EventResultRoute::ChallengeSkip { .. }) => {
            let (rate, mode) = if let EventResultRoute::ChallengeSkip { event_id } = route {
                let l = local
                    .iter_mut()
                    .find(|l| l.event_id == event_id)
                    .ok_or_else(|| Error::Game(format!("no local event {event_id}")))?;
                l.challenge_points = debit_challenge_points(l.challenge_points, consumed);
                (challenge_point_bonus(master, i64::from(consumed))?[4], SkipEventMode::Challenge { event_id })
            } else {
                (boost_bonus(master, i64::from(consumed))?[4], SkipEventMode::Normal)
            };
            let out = apply_skip_event_counters(
                master,
                &request.holding_event_ids,
                &mut local,
                members,
                snaps,
                SkipEventParams { score_rank, event_point_rate: rate as i32, mode },
            )?;
            missing_point_rows = out.missing_point_rows;
            out.points
        }
    };
    Ok(EventPointPreview { points, local_events: local, missing_point_rows })
}

/// A server-selected event reward row. Neither its selection nor its probability is inferred by this crate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerEventReward {
    pub event_id: i64,
    pub reward_id: i64,
}

/// Conditional item preview, after the event-counter stage. None means unknown selection, not no drops.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventItemRequest {
    pub route: EventResultRoute,
    pub consumed_count: i32,
    pub local_event_ids: Vec<i64>,
    pub selected_rewards: Option<Vec<ServerEventReward>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientEventItem {
    pub event_id: i64,
    pub reward_id: i64,
    pub resource_type: i64,
    pub resource_id: i64,
    pub amount: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventItemPreview {
    pub rewards: Vec<ClientEventItem>,
    /// Skip logs missing rows; played Challenge skips them. Played normal returns an error instead.
    pub missing_reward_ids: Vec<i64>,
}

/// Transform selected event reward IDs exactly as the selected client path does. Normal receiveRewards are
/// already supplied separately and must not be multiplied again. No inventory is mutated and no drops rolled.
pub fn preview_client_event_items(
    master: &Master,
    request: &EventItemRequest,
    members: &[Option<&EventMember>],
    snaps: Option<&[Option<&EventSnap>]>,
) -> Result<EventItemPreview, Error> {
    let choices = request
        .selected_rewards
        .as_ref()
        .ok_or_else(|| Error::Unsupported("server-selected event reward IDs are unknown".into()))?;
    let challenge =
        matches!(request.route, EventResultRoute::ChallengePlayed { .. } | EventResultRoute::ChallengeSkip { .. });
    let rate = if challenge {
        challenge_point_bonus(master, i64::from(request.consumed_count))?[0]
    } else {
        boost_bonus(master, i64::from(request.consumed_count))?[0]
    };
    let rows = if challenge { &master.challenge_live_event_rewards } else { &master.live_event_rewards };
    let mut out = EventItemPreview { rewards: Vec::new(), missing_reward_ids: Vec::new() };
    for choice in choices {
        let bonus_for = || -> Result<i32, Error> {
            let has_local = request.local_event_ids.contains(&choice.event_id);
            let bonus = if !has_local {
                if matches!(request.route, EventResultRoute::NormalSkip | EventResultRoute::ChallengeSkip { .. }) {
                    return Err(Error::Game(format!("no local event {} for skip item reward", choice.event_id)));
                }
                0
            } else {
                // Played Challenge uses the selected challenge event list even when the reward names another event.
                let effect_event_id = match request.route {
                    EventResultRoute::ChallengePlayed { event_id } => event_id,
                    _ => choice.event_id,
                };
                master
                    .event(effect_event_id)
                    .ok_or_else(|| Error::Master(format!("unknown event {effect_event_id}")))?;
                total_effect_10000_deck(&[event_effects(master, effect_event_id)], members, snaps, EVENT_ITEM)?
            };
            Ok(bonus)
        };
        let early_bonus =
            if matches!(request.route, EventResultRoute::NormalPlayed) { Some(bonus_for()?) } else { None };
        let Some(row) = rows.iter().find(|r| r.id == choice.reward_id) else {
            if matches!(request.route, EventResultRoute::NormalPlayed) {
                return Err(Error::Master(format!("unknown normal event reward {}", choice.reward_id)));
            }
            out.missing_reward_ids.push(choice.reward_id);
            continue;
        };
        let bonus = match early_bonus {
            Some(value) => value,
            None => bonus_for()?,
        };
        out.rewards.push(ClientEventItem {
            event_id: choice.event_id,
            reward_id: choice.reward_id,
            resource_type: row.resource_type,
            resource_id: row.resource_id,
            amount: event_item_amount(row.resource_count, bonus, rate),
        });
    }
    Ok(out)
}

const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;

fn days_before_year(year: i64) -> i64 {
    let y = year - 1;
    y * 365 + y / 4 - y / 100 + y / 400
}

/// Supported master-date adapter: yyyy-MM-dd, or yyyy-MM-dd[T or space]HH:mm:ss with optional 1..7
/// fractional digits. No timezone suffix, locale formats, trimming, or DateTime.Kind coercion is assumed.
/// The client accepts broader InvariantCulture DateTime.Parse inputs; unsupported forms are explicit errors.
pub fn parse_master_jst_canonical(text: &str) -> Result<i64, Error> {
    let b = text.as_bytes();
    let supported = b.is_ascii()
        && (b.len() == 10 || b.len() == 19 || (21..=27).contains(&b.len()))
        && b.get(4) == Some(&b'-')
        && b.get(7) == Some(&b'-')
        && (b.len() == 10 || (matches!(b[10], b'T' | b' ') && b[13] == b':' && b[16] == b':'))
        && (b.len() <= 19 || b[19] == b'.');
    if !supported {
        return Err(Error::Unsupported(format!("master date is outside canonical no-offset format: {text:?}")));
    }
    let digits = |range: std::ops::Range<usize>| -> Result<i64, Error> {
        let mut n = 0i64;
        for &digit in &b[range] {
            if !digit.is_ascii_digit() {
                return Err(Error::Unsupported(format!("non-decimal canonical date field: {text:?}")));
            }
            n = n * 10 + i64::from(digit - b'0');
        }
        Ok(n)
    };
    let year = digits(0..4)?;
    let month = digits(5..7)?;
    let day = digits(8..10)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_lengths = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > month_lengths[(month - 1).clamp(0, 11) as usize]
    {
        return Err(Error::Input(format!("invalid Gregorian master date: {text:?}")));
    }
    let (hour, minute, second) =
        if b.len() == 10 { (0, 0, 0) } else { (digits(11..13)?, digits(14..16)?, digits(17..19)?) };
    if hour > 23 || minute > 59 || second > 59 {
        return Err(Error::Input(format!("invalid canonical time: {text:?}")));
    }
    let fraction = if b.len() > 19 { digits(20..b.len())? * 10i64.pow((27 - b.len()) as u32) } else { 0 };
    let days = days_before_year(year) + month_lengths[..(month - 1) as usize].iter().sum::<i64>() + day - 1;
    Ok(days * TICKS_PER_DAY + (hour * 3600 + minute * 60 + second) * TICKS_PER_SECOND + fraction)
}

/// Unset start uses DateTime.MinValue, whereas unset end means no end. Whitespace is not unset.
pub fn event_window_canonical(row: &crate::master::EventRow) -> Result<EventWindow, Error> {
    let is_unset = |s: Option<&str>| matches!(s, None | Some("") | Some("null"));
    let start_jst_ticks = if is_unset(row.start_at.as_deref()) {
        0
    } else {
        parse_master_jst_canonical(row.start_at.as_deref().expect("checked present"))?
    };
    let end_jst_ticks = if is_unset(row.end_at.as_deref()) {
        None
    } else {
        Some(parse_master_jst_canonical(row.end_at.as_deref().expect("checked present"))?)
    };
    Ok(EventWindow { event_id: row.id, start_jst_ticks, end_jst_ticks })
}

/// Convert modern UTC ticks to JST (+09:00), with an explicit 1970..2100 supported UTC year domain.
/// This is not a replacement for historical/future system TimeZoneInfo or arbitrary DateTime.Kind behavior.
pub fn utc_to_jst_ticks_modern(utc_ticks: i64) -> Result<i64, Error> {
    let start = days_before_year(1970) * TICKS_PER_DAY;
    let end = days_before_year(2101) * TICKS_PER_DAY;
    if !(start..end).contains(&utc_ticks) {
        return Err(Error::Unsupported(
            "UTC-to-JST adapter supports UTC years 1970..2100 only; supply normalized JST ticks otherwise".into(),
        ));
    }
    Ok(utc_ticks + 9 * 3600 * TICKS_PER_SECOND)
}

impl EventResultClock {
    pub fn played_from_utc_ticks(live_start: Option<i64>, server_now: i64) -> Result<Self, Error> {
        Ok(Self::Played {
            live_start_jst_ticks: live_start.map(utc_to_jst_ticks_modern).transpose()?,
            server_now_jst_ticks: utc_to_jst_ticks_modern(server_now)?,
        })
    }

    pub fn skip_from_utc_ticks(server_now: i64) -> Result<Self, Error> {
        Ok(Self::Skip { server_now_jst_ticks: utc_to_jst_ticks_modern(server_now)? })
    }
}

/// A newly crossed one-time reward threshold. A decrease or wrapped-negative new total reaches no positive row.
pub fn achievement_reward_reached(required: i32, old_points: i32, new_points: i32) -> bool {
    old_points < required && required <= new_points
}

/// Low-level client loop counter, including synthetic signed/zero-step cases. The normal master getter rejects
/// step <= 0 before calling this leaf; the leaf itself returns zero at step zero and uses truncating signed division.
pub fn loop_reward_count(points: i32, start: i32, step: i32) -> i32 {
    if points < start.wrapping_add(step) || step == 0 { 0 } else { points.wrapping_sub(start).wrapping_div(step) }
}

pub fn reached_loop_reward_count(old_points: i32, new_points: i32, start: i32, step: i32) -> i32 {
    loop_reward_count(new_points, start, step).wrapping_sub(loop_reward_count(old_points, start, step))
}

/// A compact representation of an append to the unviewed-reward UI queue, not an inventory grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientAchievementNotice {
    pub is_loop: bool,
    pub row_id: i64,
    pub reward_ids: Vec<i64>,
    /// The client appends the same loop row this many times, after the one-time rows.
    pub count: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventAchievementPreview {
    pub new_points: i32,
    pub notices: Vec<ClientAchievementNotice>,
    pub duplicate_loop_row_count: usize,
    pub invalid_loop_reward_ids: Vec<i64>,
}

/// Counter and unviewed achievement-queue changes from AddEventPointCount. The client later also updates story
/// unlock state; this function does not claim a reward, mutate story state, or predict server acceptance.
pub fn preview_event_achievement_update(
    master: &Master,
    event_id: i64,
    old_points: i32,
    added_points: i32,
) -> Result<EventAchievementPreview, Error> {
    let new_points = add_event_point_count(old_points, added_points);
    let mut out = EventAchievementPreview {
        new_points,
        notices: Vec::new(),
        duplicate_loop_row_count: 0,
        invalid_loop_reward_ids: Vec::new(),
    };
    if added_points == 0 {
        return Ok(out);
    }
    master.event(event_id).ok_or_else(|| Error::Master(format!("unknown event {event_id}")))?;
    let mut rows: Vec<_> = master.event_achievement_rewards.iter().filter(|r| r.event_id == event_id).collect();
    rows.sort_by_key(|r| r.event_point);
    for row in rows {
        if achievement_reward_reached(row.event_point as i32, old_points, new_points) {
            out.notices.push(ClientAchievementNotice {
                is_loop: false,
                row_id: row.id,
                reward_ids: row.reward_ids.clone(),
                count: 1,
            });
        }
    }
    let loops: Vec<_> = master.event_achievement_loop_rewards.iter().filter(|r| r.event_id == event_id).collect();
    if loops.len() > 1 {
        out.duplicate_loop_row_count = loops.len();
    }
    if let Some(row) = loops.first() {
        let step = row.loop_event_point as i32;
        if step < 1 {
            out.invalid_loop_reward_ids.push(row.id);
        } else {
            let count = reached_loop_reward_count(old_points, new_points, row.loop_start_event_point as i32, step);
            if count > 0 {
                out.notices.push(ClientAchievementNotice {
                    is_loop: true,
                    row_id: row.id,
                    reward_ids: row.reward_ids.clone(),
                    count,
                });
            }
        }
    }
    Ok(out)
}
