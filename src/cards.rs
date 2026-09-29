//! The user's cards and player state, and the resolved card views the calculations read.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::calc::{
    SlotMember, SlotSupport, level_rates, member_awake_bonus, member_level_power, member_rank_bonus,
    support_level_percent,
};
use crate::error::Error;
use crate::master::{LiveMusicRow, Master};
use crate::memory::{MemoryState, memory_power_bonus};
use crate::power::CardPower;

/// Player-wide state that changes card power.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Player {
    /// Character id -> character rank. A character missing here has rank 1.
    pub character_ranks: BTreeMap<i64, i64>,
    /// Band item id -> level.
    pub band_items: BTreeMap<i64, i64>,
    pub vip_rank: i64,
    /// Ids of the events being held (they add event bonuses when event parameters are requested).
    pub events: Vec<i64>,
    pub memory: Option<MemoryState>,
    /// Member card ids the player owns, for the memory unlock count (`None`: every card counts as owned).
    pub owned_member_card_ids: Option<BTreeSet<i64>>,
    /// Snap ids the player owns, for the memory unlock count (`None`: every snap counts as owned).
    pub owned_support_card_ids: Option<BTreeSet<i64>>,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            character_ranks: BTreeMap::new(),
            band_items: BTreeMap::new(),
            vip_rank: 1,
            events: Vec::new(),
            memory: None,
            owned_member_card_ids: None,
            owned_support_card_ids: None,
        }
    }
}

impl Player {
    /// The player's rank of a character (1 when absent).
    pub fn character_rank(&self, character_id: i64) -> i64 {
        self.character_ranks.get(&character_id).copied().unwrap_or(1)
    }

    /// Sum of every character rank the player has.
    pub fn character_total_rank(&self) -> i64 {
        self.character_ranks.values().fold(0i64, |a, &b| a.wrapping_add(b))
    }
}

/// A member card the player owns.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedMember {
    pub id: i64,
    /// The level; when absent the level is derived from `exp`.
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(default)]
    pub exp: Option<i64>,
    #[serde(default = "one")]
    pub awake: i64,
    #[serde(default = "one")]
    pub rank: i64,
    #[serde(default = "one")]
    pub live_skill_level: i64,
    #[serde(default = "one")]
    pub gekisou_skill_level: i64,
}

/// A snap the player owns.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedSnap {
    pub id: i64,
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(default)]
    pub exp: Option<i64>,
    #[serde(default = "one")]
    pub rank: i64,
}

fn one() -> i64 {
    1
}

/// The user's box: player state, member cards and snaps.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Roster {
    pub player: Player,
    pub members: Vec<OwnedMember>,
    pub snaps: Vec<OwnedSnap>,
}

impl Roster {
    pub fn from_json(text: &str) -> Result<Roster, Error> {
        serde_json::from_str(text).map_err(|e| Error::Input(format!("roster: {e}")))
    }
}

/// A member card resolved against the master and the player.
#[derive(Clone, Debug)]
pub struct MemberView {
    pub id: i64,
    pub character_id: i64,
    pub band_id: i64,
    pub card_type: i64,
    pub rarity: i64,
    pub level: i64,
    pub awake: i64,
    pub rank: i64,
    pub rank_group: i64,
    pub best_music_tag_ids: Vec<i64>,
    pub leader_skill_id: i64,
    /// From the card's rank row.
    pub leader_skill_level: i64,
    pub live_skill_id: i64,
    pub live_skill_level: i64,
    /// Categories of the live skill (`None` when the skill has no master row).
    pub live_skill_categories: Option<Vec<i64>>,
    pub gekisou_skill_id: i64,
    pub gekisou_skill_level: i64,
    pub gekisou_skill_categories: Option<Vec<i64>>,
    pub gekisou_mission_type: Option<i64>,
    /// Level, awake, rank and memory terms.
    pub power: CardPower,
    pub character_rank: i64,
    pub character_total_rank: i64,
}

impl MemberView {
    pub fn resolve(master: &Master, player: &Player, owned: &OwnedMember) -> Result<MemberView, Error> {
        let row =
            master.member_card(owned.id).ok_or_else(|| Error::Input(format!("unknown member card {}", owned.id)))?;
        let level_row = match owned.level {
            Some(level) => master.member_level(row.level_group, level),
            None => master.member_level_by_exp(row.level_group, owned.exp.unwrap_or(0)),
        }
        .ok_or_else(|| Error::Input(format!("no level row for member card {}", owned.id)))?;
        let awake_row = master
            .member_awake(row.awake_group, owned.awake)
            .ok_or_else(|| Error::Input(format!("no awake row ({}, {})", row.awake_group, owned.awake)))?;
        let rank_row = master
            .member_rank(row.rank_group, owned.rank)
            .ok_or_else(|| Error::Input(format!("no rank row ({}, {})", row.rank_group, owned.rank)))?;
        let character = master
            .character(row.character_id)
            .ok_or_else(|| Error::Input(format!("unknown character {}", row.character_id)))?;
        let max = [row.performance_power_max, row.technic_power_max, row.visual_power_max];
        let own = member_level_power(level_rates(level_row), max)
            .add(member_awake_bonus([awake_row.performance_rate, awake_row.technic_rate, awake_row.visual_rate], max))
            .add(member_rank_bonus([rank_row.performance_rate, rank_row.technic_rate, rank_row.visual_rate], max));
        let live = master.live_skill(row.live_skill_id);
        let gekisou = master.gekisou_skill(row.gekisou_skill_id);
        let mut v = MemberView {
            id: owned.id,
            character_id: row.character_id,
            band_id: character.band_id,
            card_type: row.card_type,
            rarity: row.rarity,
            level: level_row.level,
            awake: owned.awake,
            rank: owned.rank,
            rank_group: row.rank_group,
            best_music_tag_ids: row.best_music_tag_ids.clone(),
            leader_skill_id: row.leader_skill_id,
            leader_skill_level: rank_row.leader_skill_level,
            live_skill_id: row.live_skill_id,
            live_skill_level: owned.live_skill_level,
            live_skill_categories: live.map(|s| s.skill_categories.clone()),
            gekisou_skill_id: row.gekisou_skill_id,
            gekisou_skill_level: owned.gekisou_skill_level,
            gekisou_skill_categories: gekisou.map(|s| s.skill_categories.clone()),
            gekisou_mission_type: gekisou.map(|s| s.gekisou_mission_type),
            power: own,
            character_rank: player.character_rank(row.character_id),
            character_total_rank: player.character_total_rank(),
        };
        v.power = own.add(memory_power_bonus(master, player, &v)?);
        Ok(v)
    }

    pub fn has_tag(&self, tag: i64) -> bool {
        self.best_music_tag_ids.contains(&tag)
    }

    /// The slot view of this card.
    pub fn slot(&self) -> SlotMember<'_> {
        SlotMember {
            power: self.power,
            character_rank: self.character_rank,
            character_total_rank: self.character_total_rank,
            card_type: self.card_type,
            music_type: self.card_type,
            best_music_tag_ids: Some(&self.best_music_tag_ids),
            rank_group: self.rank_group,
            rank: self.rank,
        }
    }
}

/// A snap resolved against the master.
#[derive(Clone, Debug)]
pub struct SnapView {
    pub id: i64,
    pub card_type: i64,
    pub rarity: i64,
    pub level: i64,
    pub rank: i64,
    pub rank_group: i64,
    pub character_ids: Vec<i64>,
    /// Band of each character (`None` for a character without a master row).
    pub character_band_ids: Vec<Option<i64>>,
    pub power_bonus_percent: CardPower,
    pub support_skill_ids: [i64; 2],
    /// Levels of the two support skills at the snap's rank (`None` when the rank has no row).
    pub support_skill_levels: Option<[i64; 2]>,
    pub gekisou_support_skill_ids: [i64; 2],
    /// Levels of the two Gekisou support skills at the snap's rank (`None` when the rank has no row).
    pub gekisou_support_skill_levels: Option<[i64; 2]>,
}

impl SnapView {
    pub fn resolve(master: &Master, owned: &OwnedSnap) -> Result<SnapView, Error> {
        let row = master.support_card(owned.id).ok_or_else(|| Error::Input(format!("unknown snap {}", owned.id)))?;
        let level_row = match owned.level {
            Some(level) => master.support_level(row.level_group, level),
            None => master.support_level_by_exp(row.level_group, owned.exp.unwrap_or(0)),
        }
        .ok_or_else(|| Error::Input(format!("no level row for snap {}", owned.id)))?;
        let max = [row.performance_power_max, row.technic_power_max, row.visual_power_max];
        let rank_row = master.support_card_ranks.iter().find(|r| r.group == row.rank_group && r.rank == owned.rank);
        Ok(SnapView {
            id: owned.id,
            card_type: row.card_type,
            rarity: row.rarity,
            level: level_row.level,
            rank: owned.rank,
            rank_group: row.rank_group,
            character_ids: row.character_ids.clone(),
            character_band_ids: row.character_ids.iter().map(|&c| master.character(c).map(|r| r.band_id)).collect(),
            power_bonus_percent: support_level_percent(level_rates(level_row), max),
            support_skill_ids: [row.support_skill_id_01, row.support_skill_id_02],
            support_skill_levels: rank_row.map(|r| [r.support_skill_01_level, r.support_skill_02_level]),
            gekisou_support_skill_ids: [row.gekisou_support_skill_id_01, row.gekisou_support_skill_id_02],
            gekisou_support_skill_levels: rank_row
                .map(|r| [r.gekisou_support_skill_01_level, r.gekisou_support_skill_02_level]),
        })
    }

    /// The support skills `(id, level)` the snap brings to a live, in order (skill id 0 is no skill).
    pub fn support_skills(&self) -> Result<Vec<(i64, i64)>, Error> {
        let ids = self.support_skill_ids;
        if ids.iter().all(|&id| id == 0) {
            return Ok(Vec::new());
        }
        let levels = self
            .support_skill_levels
            .ok_or_else(|| Error::Input(format!("snap {}: no rank row for its support skill levels", self.id)))?;
        Ok(ids.iter().zip(levels).filter(|(id, _)| **id != 0).map(|(&id, lv)| (id, lv)).collect())
    }

    /// The Gekisou support skills `(id, level)` the snap brings to a live with Gekisou, in order (skill id 0 is no
    /// skill). The live only runs them for a member with a Gekisou skill.
    pub fn gekisou_support_skills(&self) -> Result<Vec<(i64, i64)>, Error> {
        let ids = self.gekisou_support_skill_ids;
        if ids.iter().all(|&id| id == 0) {
            return Ok(Vec::new());
        }
        let levels = self.gekisou_support_skill_levels.ok_or_else(|| {
            Error::Input(format!("snap {}: no rank row for its Gekisou support skill levels", self.id))
        })?;
        Ok(ids.iter().zip(levels).filter(|(id, _)| **id != 0).map(|(&id, lv)| (id, lv)).collect())
    }

    /// The slot view of this snap.
    pub fn slot(&self) -> SlotSupport {
        SlotSupport {
            power_bonus_percent: self.power_bonus_percent,
            card_type: self.card_type,
            rank_group: self.rank_group,
            rank: self.rank,
        }
    }
}

/// The song values the power reads.
#[derive(Clone, Debug)]
pub struct SongView {
    pub id: i64,
    pub music_type: i64,
    pub best_music_tag_ids: Option<Vec<i64>>,
    pub type_bonus_rate: i64,
    pub tag_bonus_rate: i64,
}

impl SongView {
    /// A regular live song (its extra type and tag rates are 0).
    pub fn from_row(row: &LiveMusicRow) -> SongView {
        SongView {
            id: row.id,
            music_type: row.music_type,
            best_music_tag_ids: Some(row.best_music_tag_ids.clone()),
            type_bonus_rate: 0,
            tag_bonus_rate: 0,
        }
    }

    /// Challenge music uses its own nonzero type, or the base song's type, and the base song's tags.
    pub fn from_challenge_row(master: &Master, row: &crate::master::ChallengeMusicRow) -> Result<SongView, Error> {
        let base = master
            .live_music(row.live_music_id)
            .ok_or_else(|| Error::Master(format!("unknown live music {}", row.live_music_id)))?;
        let mut music = Self::from_row(base);
        if row.music_type != 0 {
            music.music_type = row.music_type;
        }
        Ok(music)
    }

    /// Arena music's parameter view. Its type is used literally (including zero), unlike challenge music.
    /// This is not the base-song parameter view used by the multiplayer live setup.
    pub fn from_arena_row(master: &Master, row: &crate::master::ArenaMusicRow) -> Result<SongView, Error> {
        let base = master
            .live_music(row.live_music_id)
            .ok_or_else(|| Error::Master(format!("unknown live music {}", row.live_music_id)))?;
        let mut music = Self::from_row(base);
        music.music_type = row.live_music_type;
        music.type_bonus_rate = row.type_bonus_rate;
        music.tag_bonus_rate = row.best_music_tag_bonus_rate;
        Ok(music)
    }

    pub fn slot(&self) -> crate::calc::SlotMusic<'_> {
        crate::calc::SlotMusic {
            music_type: self.music_type,
            best_music_tag_ids: self.best_music_tag_ids.as_deref(),
            type_bonus_rate: self.type_bonus_rate,
            tag_bonus_rate: self.tag_bonus_rate,
        }
    }
}
