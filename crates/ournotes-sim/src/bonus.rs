//! Per-slot player bonuses: band items, the leader skill, events and VIP.

use std::collections::HashMap;

use crate::cards::{MemberView, Player, SnapView, SongView};
use crate::error::Error;
use crate::event::{EventCard, EventMember, EventSnap, PARAMETER_ALL, event_effects, total_effect_10000};
use crate::master::{BandItemEffectRow, Master, SkillTargetRow};
use crate::power::CardPower;

/// VIP bonus type of the deck power bonus.
pub const VIP_DECK_TOTAL_POWER: i64 = 7;

// --- band items -----------------------------------------------------------------------------------

/// Band item effects filed by character, band and card type.
#[derive(Clone, Debug, Default)]
pub struct BandItemMaps<'m> {
    by_character: HashMap<i64, Vec<&'m BandItemEffectRow>>,
    by_band: HashMap<i64, Vec<&'m BandItemEffectRow>>,
    by_type: HashMap<i64, Vec<&'m BandItemEffectRow>>,
}

impl<'m> BandItemMaps<'m> {
    /// For every owned band item, the effect row of its level; each target of the row files it under its band,
    /// character and card type keys independently.
    pub fn build(master: &'m Master, player: &Player) -> Result<BandItemMaps<'m>, Error> {
        let mut maps = BandItemMaps::default();
        for (&item, &level) in &player.band_items {
            let Some(row) = master.band_item_effects.iter().find(|r| r.band_item_id == item && r.level == level) else {
                continue;
            };
            for &tid in &row.skill_target_ids {
                let t = master.skill_target(tid).ok_or_else(|| Error::Master(format!("unknown skill target {tid}")))?;
                if t.band_id > 0 {
                    maps.by_band.entry(t.band_id).or_default().push(row);
                }
                if t.character_id > 0 {
                    maps.by_character.entry(t.character_id).or_default().push(row);
                }
                if t.card_type != 0 {
                    maps.by_type.entry(t.card_type).or_default().push(row);
                }
            }
        }
        Ok(maps)
    }

    /// The band item percentage of a member: effects under its character, its band and its card type; type 1000
    /// adds to all stats, 1001 technique, 1002 visual, 1003 performance (32-bit accumulators).
    pub fn bonus(&self, card: &MemberView) -> CardPower {
        let mut acc = [0i32; 3];
        let empty = Vec::new();
        for rows in [
            self.by_character.get(&card.character_id).unwrap_or(&empty),
            self.by_band.get(&card.band_id).unwrap_or(&empty),
            self.by_type.get(&card.card_type).unwrap_or(&empty),
        ] {
            for r in rows {
                let v = r.effect_value as i32;
                match r.skill_effect_type {
                    1000 => {
                        acc[0] = acc[0].wrapping_add(v);
                        acc[1] = acc[1].wrapping_add(v);
                        acc[2] = acc[2].wrapping_add(v);
                    }
                    1001 => acc[1] = acc[1].wrapping_add(v),
                    1002 => acc[2] = acc[2].wrapping_add(v),
                    1003 => acc[0] = acc[0].wrapping_add(v),
                    _ => {}
                }
            }
        }
        CardPower::bp(acc[0] as i64, acc[1] as i64, acc[2] as i64)
    }
}

// --- target matching ------------------------------------------------------------------------------

fn category_match(a: &[i64], b: Option<&Vec<i64>>) -> bool {
    match b {
        Some(b) => !a.is_empty() && !b.is_empty() && a.iter().any(|x| b.contains(x)),
        None => false,
    }
}

/// Whether a skill target selects a member card: the first set key that matches, in the order band, card type,
/// character, tag, live skill categories, Gekisou skill categories, Gekisou mission type.
pub fn is_target_member(card: &MemberView, t: &SkillTargetRow) -> bool {
    if t.band_id > 0 && t.band_id == card.band_id {
        return true;
    }
    if t.card_type != 0 && t.card_type == card.card_type {
        return true;
    }
    if t.character_id > 0 && t.character_id == card.character_id {
        return true;
    }
    if t.tag_id > 0 && card.has_tag(t.tag_id) {
        return true;
    }
    if category_match(&t.live_skill_categories, card.live_skill_categories.as_ref()) {
        return true;
    }
    if category_match(&t.gekisou_skill_categories, card.gekisou_skill_categories.as_ref()) {
        return true;
    }
    if t.gekisou_mission_type != 0 && card.gekisou_mission_type == Some(t.gekisou_mission_type) {
        return true;
    }
    false
}

fn matches_any_target(card: &MemberView, targets: Option<&[&SkillTargetRow]>) -> bool {
    targets.is_some_and(|ts| ts.iter().any(|t| is_target_member(card, t)))
}

/// Target rows of an id list (`None` for an empty list); an unknown id is a master error.
pub(crate) fn targets<'m>(master: &'m Master, ids: &[i64]) -> Result<Option<Vec<&'m SkillTargetRow>>, Error> {
    if ids.is_empty() {
        return Ok(None);
    }
    ids.iter()
        .map(|&i| master.skill_target(i).ok_or_else(|| Error::Master(format!("unknown skill target {i}"))))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

// --- leader skill ---------------------------------------------------------------------------------

fn check_formation_condition(
    ctype: i64,
    members: &[&MemberView; 5],
    targets: Option<&[&SkillTargetRow]>,
    music: Option<&SongView>,
) -> bool {
    let Some(ts) = targets else { return true };
    match ctype {
        3000 => members.iter().any(|m| matches_any_target(m, Some(ts))),
        4012 => match music {
            None => false,
            Some(mu) => ts.iter().any(|t| t.live_music_type != 0 && t.live_music_type == mu.music_type),
        },
        3001 => members.iter().all(|m| matches_any_target(m, Some(ts))),
        _ => true,
    }
}

fn check_condition(
    master: &Master,
    group: i64,
    members: &[&MemberView; 5],
    music: Option<&SongView>,
) -> Result<bool, Error> {
    if group <= 0 {
        return Ok(true);
    }
    for cs in master.skill_condition_sets.iter().filter(|s| s.group == group) {
        for &cid in &cs.condition_ids {
            let c = master.skill_condition(cid).ok_or_else(|| Error::Master(format!("unknown condition {cid}")))?;
            if c.condition_type != 0 {
                let ts = targets(master, &c.condition_target_ids)?;
                if !check_formation_condition(c.condition_type, members, ts.as_deref(), music) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn evaluate_cumulative_count(
    master: &Master,
    cid: i64,
    members: &[&MemberView; 5],
    leader_index: usize,
) -> Result<i64, Error> {
    if cid < 1 {
        return Ok(1);
    }
    let Some(row) = master.cumulative_condition(cid) else { return Ok(1) };
    let ts = targets(master, &row.condition_target_ids)?;
    let ts = ts.as_deref();
    let leader = members[leader_index];
    let n = match row.condition_type {
        3000 => members.iter().filter(|m| matches_any_target(m, ts)).count() as i64,
        3001 => {
            members.iter().enumerate().filter(|&(i, m)| i != leader_index && matches_any_target(m, ts)).count() as i64
        }
        3002 => members.iter().filter(|m| m.band_id == leader.band_id).count() as i64,
        3003 => members.iter().filter(|m| m.band_id != leader.band_id).count() as i64,
        3004 => {
            let mut b: Vec<i64> = members.iter().map(|m| m.band_id).collect();
            b.sort_unstable();
            b.dedup();
            b.len() as i64
        }
        3005 => {
            let mut b: Vec<i64> = members.iter().map(|m| m.card_type).collect();
            b.sort_unstable();
            b.dedup();
            b.len() as i64
        }
        _ => return Ok(1),
    };
    let cap = row.max_cumulative_count;
    Ok(if cap < 1 { n } else { n.min(cap) })
}

/// Whether a leader skill effect type accumulates by a count.
pub fn is_cumulative_leader_effect(effect_type: i64) -> bool {
    (effect_type & !3) == 1500
}

fn accumulate(effect_type: i64, value: i64, acc: &mut [i64; 3]) {
    match effect_type {
        1000 | 1500 => {
            acc[0] = acc[0].wrapping_add(value);
            acc[1] = acc[1].wrapping_add(value);
            acc[2] = acc[2].wrapping_add(value);
        }
        1001 | 1501 => acc[1] = acc[1].wrapping_add(value),
        1002 | 1502 => acc[2] = acc[2].wrapping_add(value),
        1003 | 1503 => acc[0] = acc[0].wrapping_add(value),
        _ => {}
    }
}

/// Leader skill percentages of the five slots: the effects of the slot-2 member's leader skill at its level; an
/// effect applies when its condition group holds, with its value (times the cumulative count for cumulative
/// types) added to every slot whose member it targets (no targets: every slot).
pub fn leader_skill_bonuses(
    master: &Master,
    members: &[&MemberView; 5],
    music: Option<&SongView>,
) -> Result<[CardPower; 5], Error> {
    const LEADER: usize = 2;
    let leader = members[LEADER];
    let mut out = [[0i64; 3]; 5];
    for e in master
        .leader_skill_effects
        .iter()
        .filter(|e| e.leader_skill_id == leader.leader_skill_id && e.level == leader.leader_skill_level)
    {
        if !check_condition(master, e.skill_condition_group, members, music)? {
            continue;
        }
        let mut value = e.effect_value;
        if is_cumulative_leader_effect(e.skill_effect_type) {
            let n = evaluate_cumulative_count(master, e.skill_cumulative_condition_id, members, LEADER)?;
            value = (value as i32).wrapping_mul(n as i32) as i64;
        }
        let ts = targets(master, &e.skill_target_ids)?;
        for i in 0..5 {
            if ts.is_none() || matches_any_target(members[i], ts.as_deref()) {
                accumulate(e.skill_effect_type, value, &mut out[i]);
            }
        }
    }
    Ok(out.map(|a| CardPower::bp(a[0], a[1], a[2])))
}

/// The effects of one leader skill at one level, prepared for the search.
#[derive(Clone, Debug)]
pub struct LeaderProfile<'m> {
    pub leader_skill_id: i64,
    pub level: i64,
    /// Whether every effect is unconditional and non-cumulative, so that a member's percentage depends on that member
    /// alone.
    pub simple: bool,
    effects: Vec<(&'m crate::master::LeaderSkillEffectRow, Option<Vec<&'m SkillTargetRow>>)>,
}

impl<'m> LeaderProfile<'m> {
    pub fn new(master: &'m Master, leader_skill_id: i64, level: i64) -> Result<LeaderProfile<'m>, Error> {
        let mut effects = Vec::new();
        let mut simple = true;
        for e in master.leader_skill_effects.iter().filter(|e| e.leader_skill_id == leader_skill_id && e.level == level)
        {
            if e.skill_condition_group > 0 || is_cumulative_leader_effect(e.skill_effect_type) {
                simple = false;
            }
            effects.push((e, targets(master, &e.skill_target_ids)?));
        }
        Ok(LeaderProfile { leader_skill_id, level, simple, effects })
    }

    /// The exact percentage of a member when the profile is simple.
    pub fn simple_percent(&self, member: &MemberView) -> CardPower {
        debug_assert!(self.simple);
        let mut acc = [0i64; 3];
        for (e, ts) in &self.effects {
            if ts.is_none() || matches_any_target(member, ts.as_deref()) {
                accumulate(e.skill_effect_type, e.effect_value, &mut acc);
            }
        }
        CardPower::bp(acc[0], acc[1], acc[2])
    }

    /// A component-wise upper bound of the member's percentage over every deck: each effect that targets the member
    /// counted as active with its largest possible count (at most 5, or the cumulative cap), negative
    /// contributions dropped. Valid while every value times 5 fits in 32 bits.
    pub fn percent_bound(&self, master: &Master, member: &MemberView) -> CardPower {
        let mut acc = [0i64; 3];
        for (e, ts) in &self.effects {
            if !(ts.is_none() || matches_any_target(member, ts.as_deref())) {
                continue;
            }
            let mut v = e.effect_value;
            if is_cumulative_leader_effect(e.skill_effect_type) {
                let cap = master
                    .cumulative_condition(e.skill_cumulative_condition_id)
                    .map_or(5, |r| if r.max_cumulative_count >= 1 { r.max_cumulative_count.min(5) } else { 5 });
                v = v.max(v * cap);
            }
            accumulate(e.skill_effect_type, v.max(0), &mut acc);
        }
        CardPower::bp(acc[0], acc[1], acc[2])
    }

    /// Largest absolute effect value (for the search's overflow guard).
    pub fn max_abs_value(&self) -> i64 {
        // An unrepresentable magnitude must fail the positive domain guard too.
        self.effects.iter().map(|(e, _)| e.effect_value.saturating_abs()).max().unwrap_or(0)
    }

    /// Conservative component lower bound using the same target/accumulation
    /// primitives as evaluation. Positive conditional/count effects may be absent;
    /// negative ones use the largest count in the existing five-member proof.
    /// Unconditional noncumulative contributions are retained even when negative.
    #[doc(hidden)]
    pub fn percent_lower_bound(&self, master: &Master, member: &MemberView) -> Result<CardPower, Error> {
        if self.simple {
            return Ok(self.simple_percent(member));
        }
        let mut acc = [0i64; 3];
        for (effect, targets) in &self.effects {
            if !(targets.is_none() || matches_any_target(member, targets.as_deref())) {
                continue;
            }
            let cumulative = is_cumulative_leader_effect(effect.skill_effect_type);
            let value = if !cumulative && effect.skill_condition_group <= 0 {
                effect.effect_value
            } else if effect.effect_value >= 0 {
                0
            } else {
                let cap = if cumulative {
                    master.cumulative_condition(effect.skill_cumulative_condition_id).map_or(5, |row| {
                        if row.max_cumulative_count >= 1 { row.max_cumulative_count.min(5) } else { 5 }
                    })
                } else {
                    1
                };
                effect
                    .effect_value
                    .checked_mul(cap)
                    .ok_or_else(|| Error::Domain("leader lower bound multiplication overflow".into()))?
            };
            let mut contribution = [0i64; 3];
            accumulate(effect.skill_effect_type, value, &mut contribution);
            for i in 0..3 {
                acc[i] = acc[i]
                    .checked_add(contribution[i])
                    .ok_or_else(|| Error::Domain("leader lower bound addition overflow".into()))?;
            }
        }
        Ok(CardPower::bp(acc[0], acc[1], acc[2]))
    }
}

// --- events ---------------------------------------------------------------------------------------

/// The event view of a member card (its band is `None` when the band has no master row).
pub fn event_member(master: &Master, card: &MemberView) -> EventMember {
    EventMember {
        id: card.id,
        character_id: card.character_id,
        band_id: master.band(card.band_id).map(|b| b.id),
        card_type: card.card_type,
        tags: card.best_music_tag_ids.clone(),
        rank: card.rank,
    }
}

/// The event view of a snap.
pub fn event_snap(card: &SnapView) -> EventSnap {
    let mut bands = Vec::new();
    for b in card.character_band_ids.iter().flatten() {
        if !bands.contains(b) {
            bands.push(*b);
        }
    }
    EventSnap {
        id: card.id,
        character_ids: card.character_ids.clone(),
        band_ids: bands,
        card_type: card.card_type,
        rank: card.rank,
    }
}

/// The effect rows of every held event.
pub fn held_event_effects<'m>(master: &'m Master, player: &Player) -> Vec<Vec<&'m crate::master::EventEffectRow>> {
    player.events.iter().map(|&e| event_effects(master, e)).collect()
}

/// Event parameter bonus of a member card (a percentage on all stats).
pub fn member_event_bonus(master: &Master, player: &Player, card: &MemberView) -> Result<CardPower, Error> {
    let m = event_member(master, card);
    let v = total_effect_10000(&held_event_effects(master, player), Some(EventCard::Member(&m)), PARAMETER_ALL)?;
    Ok(CardPower::bp_single(v as i64))
}

/// Event parameter bonus of a snap.
pub fn snap_event_bonus(master: &Master, player: &Player, card: Option<&SnapView>) -> Result<CardPower, Error> {
    let Some(c) = card else { return Ok(CardPower::EMPTY) };
    let s = event_snap(c);
    let v = total_effect_10000(&held_event_effects(master, player), Some(EventCard::Snap(&s)), PARAMETER_ALL)?;
    Ok(CardPower::bp_single(v as i64))
}

/// VIP deck power bonus rate of the player's VIP rank.
pub fn vip_bonus(master: &Master, player: &Player) -> i64 {
    master.vip_bonus(VIP_DECK_TOTAL_POWER, player.vip_rank)
}
