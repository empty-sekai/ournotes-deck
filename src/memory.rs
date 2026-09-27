//! Memory bonus: flat stat points from memory music groups and the memory member / snap levels.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::bonus::is_target_member;
use crate::cards::{MemberView, Player};
use crate::error::Error;
use crate::master::{Master, MemoryLevelRow, MemoryMusicBonusRow};
use crate::power::CardPower;

/// The player's memory progress.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoryState {
    /// Memory music id -> unlocked score rank.
    pub music_ranks: BTreeMap<i64, i64>,
    /// Member card ids whose memory is unlocked.
    pub unlocked_members: BTreeSet<i64>,
    /// Snap ids whose memory is unlocked.
    pub unlocked_supports: BTreeSet<i64>,
}

fn power(p: i64, t: i64, v: i64) -> CardPower {
    CardPower::points(p, t, v)
}

fn group_reaches(master: &Master, group_id: i64, state: &MemoryState, row: &MemoryMusicBonusRow) -> bool {
    master
        .memory_musics
        .iter()
        .filter(|m| m.group_id == group_id)
        .all(|m| row.score_rank <= state.music_ranks.get(&m.id).copied().unwrap_or(0))
}

/// The current bonus row of a memory music group: the last row (master order) that every music of the group
/// reaches.
pub fn current_music_bonus<'m>(
    master: &'m Master,
    group_id: i64,
    state: &MemoryState,
) -> Option<&'m MemoryMusicBonusRow> {
    master
        .memory_music_bonuses
        .iter()
        .filter(|r| r.group_id == group_id)
        .rfind(|r| group_reaches(master, group_id, state, r))
}

fn unlocked_count(unlocked: &BTreeSet<i64>, owned: Option<&BTreeSet<i64>>) -> i64 {
    unlocked.iter().filter(|i| owned.is_none_or(|o| o.contains(i))).count() as i64
}

fn current_level(rows: &[MemoryLevelRow], count: i64) -> Option<&MemoryLevelRow> {
    rows.iter().rfind(|r| r.point <= count)
}

/// The memory member level bonus (goes to every member card).
pub fn member_current_bonus(master: &Master, player: &Player) -> CardPower {
    let empty = MemoryState::default();
    let state = player.memory.as_ref().unwrap_or(&empty);
    let n = unlocked_count(&state.unlocked_members, player.owned_member_card_ids.as_ref());
    current_level(&master.memory_member_levels, n)
        .map_or(CardPower::EMPTY, |r| power(r.performance, r.technic, r.visual))
}

/// The memory snap level bonus (also goes to every member card).
pub fn support_current_bonus(master: &Master, player: &Player) -> CardPower {
    let empty = MemoryState::default();
    let state = player.memory.as_ref().unwrap_or(&empty);
    let n = unlocked_count(&state.unlocked_supports, player.owned_support_card_ids.as_ref());
    current_level(&master.memory_support_levels, n)
        .map_or(CardPower::EMPTY, |r| power(r.performance, r.technic, r.visual))
}

/// The memory bonus of a member card: each music group with a current bonus adds it once when any of its targets
/// selects the card (an empty target list selects nothing), then the member and snap level bonuses.
pub fn memory_power_bonus(master: &Master, player: &Player, card: &MemberView) -> Result<CardPower, Error> {
    let empty = MemoryState::default();
    let state = player.memory.as_ref().unwrap_or(&empty);
    let mut acc = CardPower::EMPTY;
    for g in &master.memory_music_groups {
        let Some(cur) = current_music_bonus(master, g.id, state) else { continue };
        let mut hit = false;
        for &tid in &g.skill_target_ids {
            let t = master
                .skill_target(tid)
                .ok_or_else(|| Error::Master(format!("memory group {}: unknown skill target {tid}", g.id)))?;
            hit = hit || is_target_member(card, t);
        }
        if hit {
            acc = acc.add(power(cur.performance, cur.technic, cur.visual));
        }
    }
    Ok(acc.add(member_current_bonus(master, player)).add(support_current_bonus(master, player)))
}
