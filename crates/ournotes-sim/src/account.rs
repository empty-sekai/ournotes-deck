//! The account input (`ournotes.account/1`): the player's save data in the game's own field names, with the
//! completeness of each list declared, resolved into the [`Roster`] one goal reads.
//!
//! The document is `{format, datasetId, server, revision, coverage, assumptions, declared, account}`:
//!
//! - `datasetId`: the deck data the account is resolved against, the lowercase hex SHA-256 of its text;
//! - `server`: `jp` for a save of the Japanese server, read with deck data exported for region `jp`; `intl` for a save
//!   of the international servers, read with deck data exported for region `tw` (its master holds every card of the
//!   international servers);
//! - `revision`: an opaque identifier of this input, for example a hash of the stored save;
//! - `declared`: values the save does not hold, given by the caller: `{"_vip": {"_rank": n}}`, the VIP rank;
//! - `account`: the save's root object, `{"_player": ...}`.
//!
//! Of `account` only `_player` is read, and of `_player` only:
//!
//! - `_memberCards[]`: `_masterId`, `_exp`, `_awakeCount`, `_rank`, `_liveSkillLevel` and `_performanceSkillLevel`
//!   (the Gekisou skill level);
//! - `_supportCards[]`: `_masterId`, `_exp`, `_rank`;
//! - `_characters[]`: `_masterId`, `_exp`;
//! - `_bandItems[]`: `_masterId`, `_level` (0: not built);
//! - `_memory`: `_musicGroups[]` (`_id`, and `_musics[]` with `_id`, `_unlockedScoreRank`), `_members[]` and
//!   `_supports[]` (`_id`, `_unlocked`).
//!
//! Every other field (identity, profile, settings, items, records) is skipped while parsing and never stored.
//!
//! Values: a missing or `null` number or flag is unknown, a missing or `null` list is empty. The `long` fields
//! (`_masterId`, `_id`) take an integer or a decimal string, the `int` fields an integer in the 32-bit range; any other
//! number is an error, never rounded. A read object with a key twice is rejected.
//!
//! Coverage: `complete` states that an item missing from the list is in its starting state (card not owned, character
//! at experience 0, band item at level 0, memory not unlocked, music group not had); `partial` states that it is
//! unknown. The facts a goal reads are listed by [`AccountInput::resolve`]; an unknown fact a goal reads is reported
//! as missing, never filled in.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

use crate::cards::{OwnedMember, OwnedSnap, Player, Roster};
use crate::data::DeckData;
use crate::error::Error;
use crate::master::{Master, MemberCardRow, SupportCardRow};
use crate::memory::MemoryState;
use crate::pool::Pool;

/// The format this reader reads.
pub const FORMAT: &str = "ournotes.account/1";

/// The highest Gekisou skill level of the game (levels run from 1).
pub const GEKISOU_SKILL_MAX_LEVEL: i64 = 5;

/// The unlocked score rank of a memory music: 0 (none) to 7 (SS).
const SCORE_RANKS: std::ops::RangeInclusive<i64> = 0..=7;

/// What a recommendation computes, which decides the facts it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Power,
    Skip,
    /// A live with the members' live skills.
    NormalLive,
    /// A live with live skills and Gekisou skills.
    GekisouLive,
}

impl Goal {
    /// Whether the goal plays the members' live skills (`_liveSkillLevel`).
    pub fn reads_live_skills(self) -> bool {
        matches!(self, Goal::NormalLive | Goal::GekisouLive)
    }

    /// Whether the goal plays the members' Gekisou skills (`_performanceSkillLevel`).
    pub fn reads_gekisou_skills(self) -> bool {
        self == Goal::GekisouLive
    }

    fn name(self) -> &'static str {
        match self {
            Goal::Power => "power",
            Goal::Skip => "skip",
            Goal::NormalLive => "normalLive",
            Goal::GekisouLive => "gekisouLive",
        }
    }
}

/// The game server a save comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Server {
    /// The Japanese server.
    Jp,
    /// The international servers.
    Intl,
}

impl Server {
    /// The region (`provenance.region`) of the deck data a save of this server is read with.
    pub fn data_region(self) -> &'static str {
        match self {
            Server::Jp => "jp",
            Server::Intl => "tw",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Coverage {
    Complete,
    Partial,
}

/// The coverage of each list the resolver reads; every key is required.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageDecl {
    #[serde(rename = "_player._memberCards")]
    pub member_cards: Coverage,
    #[serde(rename = "_player._supportCards")]
    pub support_cards: Coverage,
    #[serde(rename = "_player._characters")]
    pub characters: Coverage,
    #[serde(rename = "_player._bandItems")]
    pub band_items: Coverage,
    #[serde(rename = "_player._memory._musicGroups")]
    pub memory_music_groups: Coverage,
    #[serde(rename = "_player._memory._members")]
    pub memory_members: Coverage,
    #[serde(rename = "_player._memory._supports")]
    pub memory_supports: Coverage,
}

/// A note that a given value was entered or confirmed by the user rather than read from the game. It never fills an
/// unknown value.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub path: String,
    pub reason: String,
}

/// A number or flag as written; `None` when missing or `null`.
pub type Field = Option<Box<RawValue>>;

fn null_vec<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// The account input document.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountInput {
    pub format: String,
    /// The deck data the account is resolved against: the lowercase hex SHA-256 of its text.
    pub dataset_id: String,
    pub server: Server,
    /// An opaque identifier of this input, chosen by the client.
    pub revision: String,
    pub coverage: CoverageDecl,
    pub assumptions: Vec<Assumption>,
    /// Values the save does not hold; missing or `null`: none given.
    #[serde(default)]
    pub declared: Option<Declared>,
    pub account: SaveRoot,
}

/// Values the caller gives because the save does not hold them.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    #[serde(rename = "_vip", default)]
    pub vip: Option<DeclaredVip>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredVip {
    /// The VIP rank (`int`).
    #[serde(rename = "_rank", default)]
    pub rank: Field,
}

/// The save's root object.
#[derive(Clone, Debug, Deserialize)]
pub struct SaveRoot {
    #[serde(rename = "_player")]
    pub player: PlayerSave,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PlayerSave {
    #[serde(rename = "_memberCards", deserialize_with = "null_vec")]
    pub member_cards: Vec<MemberCardSave>,
    #[serde(rename = "_supportCards", deserialize_with = "null_vec")]
    pub support_cards: Vec<SupportCardSave>,
    #[serde(rename = "_characters", deserialize_with = "null_vec")]
    pub characters: Vec<CharacterSave>,
    #[serde(rename = "_bandItems", deserialize_with = "null_vec")]
    pub band_items: Vec<BandItemSave>,
    #[serde(rename = "_memory")]
    pub memory: Option<MemorySave>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemberCardSave {
    #[serde(rename = "_masterId")]
    pub master_id: Field,
    #[serde(rename = "_exp")]
    pub exp: Field,
    #[serde(rename = "_awakeCount")]
    pub awake_count: Field,
    #[serde(rename = "_rank")]
    pub rank: Field,
    #[serde(rename = "_liveSkillLevel")]
    pub live_skill_level: Field,
    #[serde(rename = "_performanceSkillLevel")]
    pub performance_skill_level: Field,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct SupportCardSave {
    #[serde(rename = "_masterId")]
    pub master_id: Field,
    #[serde(rename = "_exp")]
    pub exp: Field,
    #[serde(rename = "_rank")]
    pub rank: Field,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CharacterSave {
    #[serde(rename = "_masterId")]
    pub master_id: Field,
    #[serde(rename = "_exp")]
    pub exp: Field,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BandItemSave {
    #[serde(rename = "_masterId")]
    pub master_id: Field,
    #[serde(rename = "_level")]
    pub level: Field,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemorySave {
    #[serde(rename = "_musicGroups", deserialize_with = "null_vec")]
    pub music_groups: Vec<MemoryMusicGroupSave>,
    #[serde(rename = "_members", deserialize_with = "null_vec")]
    pub members: Vec<MemoryCardSave>,
    #[serde(rename = "_supports", deserialize_with = "null_vec")]
    pub supports: Vec<MemoryCardSave>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemoryMusicGroupSave {
    #[serde(rename = "_id")]
    pub id: Field,
    #[serde(rename = "_musics", deserialize_with = "null_vec")]
    pub musics: Vec<MemoryMusicSave>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemoryMusicSave {
    #[serde(rename = "_id")]
    pub id: Field,
    #[serde(rename = "_unlockedScoreRank")]
    pub unlocked_score_rank: Field,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemoryCardSave {
    #[serde(rename = "_id")]
    pub id: Field,
    #[serde(rename = "_unlocked")]
    pub unlocked: Field,
}

/// One problem of an input: `path` names the field (`_player._memberCards[3]._exp`, a list for an item it does not
/// list, or an envelope field).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub path: String,
    pub code: String,
    pub message: String,
}

/// Request-side exclusions: owned cards the recommendation must not use. An excluded card needs only its identity; it
/// stays owned (memory counts it) but is left out of the roster.
#[derive(Clone, Debug, Default)]
pub struct Exclusions {
    pub members: Vec<i64>,
    pub snaps: Vec<i64>,
}

/// The outcome of [`AccountInput::resolve`]: `resolved` is set exactly when `missing` and `errors` are empty.
#[derive(Clone, Debug, Default)]
pub struct Resolution {
    /// Facts the goal reads that the input leaves unknown.
    pub missing: Vec<Issue>,
    /// Values that are malformed, out of range, unknown to the deck data or in conflict.
    pub errors: Vec<Issue>,
    pub resolved: Option<ResolvedAccount>,
}

/// An account resolved for one goal and one deck data.
#[derive(Clone, Debug)]
pub struct ResolvedAccount {
    goal: Goal,
    dataset_id: String,
    server: Server,
    revision: String,
    coverage: CoverageDecl,
    assumptions: Vec<Assumption>,
    roster: Roster,
}

impl ResolvedAccount {
    pub fn goal(&self) -> Goal {
        self.goal
    }

    pub fn dataset_id(&self) -> &str {
        &self.dataset_id
    }

    pub fn server(&self) -> Server {
        self.server
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn coverage(&self) -> &CoverageDecl {
        &self.coverage
    }

    pub fn assumptions(&self) -> &[Assumption] {
        &self.assumptions
    }

    /// The roster the goal reads: the owned cards that are not excluded, and the player state. The skill levels the
    /// goal does not read are 0 (unavailable), so the roster serves only the goal it was resolved for.
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// Whether both card lists are complete, so the roster holds every card the player owns but the excluded ones.
    pub fn covers_all_owned_cards(&self) -> bool {
        self.coverage.member_cards == Coverage::Complete && self.coverage.support_cards == Coverage::Complete
    }

    /// The input's scope, for answers: identity, coverage, assumptions, card counts and the player values derived
    /// from the account. It holds nothing from the save beyond these.
    pub fn scope(&self) -> serde_json::Value {
        let player = &self.roster.player;
        let memory = player.memory.as_ref();
        let unlocked = |ids: Option<&BTreeSet<i64>>, owned: Option<&BTreeSet<i64>>| {
            ids.map_or(0, |ids| ids.iter().filter(|id| owned.is_some_and(|owned| owned.contains(id))).count())
        };
        serde_json::json!({
            "datasetId": self.dataset_id,
            "server": self.server,
            "revision": self.revision,
            "goal": self.goal.name(),
            "coverage": self.coverage,
            "assumptions": self.assumptions,
            "cards": {
                "ownedMemberCards": player.owned_member_card_ids.as_ref().map_or(0, BTreeSet::len),
                "ownedSupportCards": player.owned_support_card_ids.as_ref().map_or(0, BTreeSet::len),
                "candidateMemberCards": self.roster.members.len(),
                "candidateSupportCards": self.roster.snaps.len(),
                "coversAllOwnedCards": self.covers_all_owned_cards(),
            },
            "playerBonusEvidence": {
                "vipRank": player.vip_rank,
                "characterTotalRank": player.character_total_rank(),
                "builtBandItems": player.band_items.len(),
                "memoryMusicGroups": memory.map_or(0, |m| m.music_groups.as_ref().map_or(0, BTreeMap::len)),
                "memoryUnlockedMemberCards":
                    unlocked(memory.map(|m| &m.unlocked_members), player.owned_member_card_ids.as_ref()),
                "memoryUnlockedSupportCards":
                    unlocked(memory.map(|m| &m.unlocked_supports), player.owned_support_card_ids.as_ref()),
            },
        })
    }
}

/// A value read from a field.
enum Fact<T> {
    Unknown,
    Known(T),
    /// Malformed; the error is already reported.
    Invalid,
}

/// An integer token: an optional minus sign and digits without leading zeros.
fn integer(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let canonical =
        digits == "0" || (!digits.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit()));
    if canonical { text.parse().ok() } else { None }
}

impl Resolution {
    fn error(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.errors.push(Issue { path: path.into(), code: code.into(), message: message.into() });
    }

    fn missing(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.missing.push(Issue { path: path.into(), code: "missing".into(), message: message.into() });
    }

    /// The issue counts so far, for [`Self::label_since`].
    fn mark(&self) -> (usize, usize) {
        (self.errors.len(), self.missing.len())
    }

    /// Prefixes the messages of the issues reported since `mark` with the item they concern.
    fn label_since(&mut self, mark: (usize, usize), item: &str) {
        let errors = self.errors[mark.0..].iter_mut();
        for issue in errors.chain(self.missing[mark.1..].iter_mut()) {
            issue.message = format!("{item}: {}", issue.message);
        }
    }

    /// A `long` field: an integer or a decimal string.
    fn long(&mut self, path: &str, field: &Field) -> Fact<i64> {
        let Some(raw) = field else { return Fact::Unknown };
        let text = raw.get().trim();
        let value = if text.starts_with('"') {
            serde_json::from_str::<String>(text).ok().and_then(|s| integer(&s))
        } else {
            integer(text)
        };
        match value {
            Some(v) => Fact::Known(v),
            None => {
                self.error(
                    path,
                    "invalid_type",
                    format!("expected a 64-bit integer or its decimal string, got {text}"),
                );
                Fact::Invalid
            }
        }
    }

    /// An `int` field: an integer in the 32-bit range.
    fn int(&mut self, path: &str, field: &Field) -> Fact<i64> {
        let Some(raw) = field else { return Fact::Unknown };
        let text = raw.get().trim();
        match integer(text).filter(|v| i32::try_from(*v).is_ok()) {
            Some(v) => Fact::Known(v),
            None => {
                self.error(path, "invalid_type", format!("expected a 32-bit integer, got {text}"));
                Fact::Invalid
            }
        }
    }

    /// A `bool` field.
    fn flag(&mut self, path: &str, field: &Field) -> Fact<bool> {
        let Some(raw) = field else { return Fact::Unknown };
        match raw.get().trim() {
            "true" => Fact::Known(true),
            "false" => Fact::Known(false),
            text => {
                self.error(path, "invalid_type", format!("expected true or false, got {text}"));
                Fact::Invalid
            }
        }
    }

    /// A fact the goal reads: an unknown one is reported missing.
    fn required<T>(&mut self, path: &str, fact: Fact<T>) -> Option<T> {
        match fact {
            Fact::Known(v) => Some(v),
            Fact::Unknown => {
                self.missing(path, "the selected goal reads this value");
                None
            }
            Fact::Invalid => None,
        }
    }

    fn required_int(&mut self, path: &str, field: &Field) -> Option<i64> {
        let fact = self.int(path, field);
        self.required(path, fact)
    }

    /// The identities of a list (`key` of each item), each known to the deck data and listed once. Returns the item
    /// index and identity of every item whose identity is valid.
    fn identities<T>(
        &mut self,
        list: &str,
        items: &[T],
        key: &str,
        id_of: impl Fn(&T) -> &Field,
        known: impl Fn(i64) -> bool,
        what: &str,
    ) -> Vec<(usize, i64)> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let path = format!("{list}[{i}].{key}");
            let id = match self.long(&path, id_of(item)) {
                Fact::Known(id) => id,
                Fact::Unknown => {
                    self.error(&path, "invalid_value", "an identity cannot be missing or null");
                    continue;
                }
                Fact::Invalid => continue,
            };
            if !known(id) {
                self.error(&path, "unknown_id", format!("{what} {id} is not in the deck data"));
            } else if !seen.insert(id) {
                self.error(&path, "duplicate_id", format!("{what} {id} is listed twice"));
            } else {
                out.push((i, id));
            }
        }
        out
    }

    /// A count that starts at 1 (`_awakeCount`, `_rank`, skill levels).
    fn at_least_one(&mut self, path: &str, value: Option<i64>) -> Option<i64> {
        match value {
            Some(v) if v < 1 => {
                self.error(path, "invalid_value", format!("{v} is below 1, where this value starts"));
                None
            }
            v => v,
        }
    }

    fn experience(&mut self, path: &str, field: &Field) -> Option<i64> {
        match self.required_int(path, field) {
            Some(v) if v < 0 => {
                self.error(path, "invalid_value", format!("experience {v} is negative"));
                None
            }
            v => v,
        }
    }

    fn member(
        &mut self,
        master: &Master,
        goal: Goal,
        path: &str,
        id: i64,
        card: &MemberCardSave,
        row: &MemberCardRow,
    ) -> Option<OwnedMember> {
        let exp_path = format!("{path}._exp");
        let awake_path = format!("{path}._awakeCount");
        let exp = self.experience(&exp_path, &card.exp);
        let awake = self.required_int(&awake_path, &card.awake_count);
        let awake = self.at_least_one(&awake_path, awake).filter(|&a| {
            let found = master.member_awake(row.awake_group, a).is_some();
            if !found {
                self.error(&awake_path, "invalid_value", format!("no awake row of group {} at {a}", row.awake_group));
            }
            found
        });
        let rank_path = format!("{path}._rank");
        let rank = self.required_int(&rank_path, &card.rank);
        let rank = self.at_least_one(&rank_path, rank).filter(|&r| {
            let found = master.member_rank(row.rank_group, r).is_some();
            if !found {
                self.error(&rank_path, "invalid_value", format!("no rank row of group {} at {r}", row.rank_group));
            }
            found
        });
        let level = exp.and_then(|exp| {
            let level = master.member_level_by_exp(row.level_group, exp).map(|r| r.level);
            if level.is_none() {
                self.error(
                    &exp_path,
                    "master_row_missing",
                    format!("no level row of group {} reached", row.level_group),
                );
            }
            level
        });
        if let (Some(exp), Some(level), Some(awake)) = (exp, level, awake) {
            match master.member_card_level_limits.iter().find(|c| c.rarity == row.rarity && c.awake_count == awake) {
                None => self.error(
                    &awake_path,
                    "master_row_missing",
                    format!("no level cap row for rarity {} at awake count {awake}", row.rarity),
                ),
                Some(cap) if level > cap.limit_level => self.error(
                    &exp_path,
                    "level_cap",
                    format!(
                        "experience {exp} reaches level {level}, above the cap {} at awake count {awake}",
                        cap.limit_level
                    ),
                ),
                Some(_) => {}
            }
        }
        let live = self.skill_level(
            &format!("{path}._liveSkillLevel"),
            &card.live_skill_level,
            row.live_skill_id,
            goal.reads_live_skills(),
            master.live_skill_max_level(row.live_skill_id),
            |level| master.live_skill_effects.iter().any(|r| r.live_skill_id == row.live_skill_id && r.level == level),
        );
        let gekisou = self.skill_level(
            &format!("{path}._performanceSkillLevel"),
            &card.performance_skill_level,
            row.gekisou_skill_id,
            goal.reads_gekisou_skills(),
            Some(GEKISOU_SKILL_MAX_LEVEL),
            |level| master.gekisou_skill_effects.iter().any(|r| r.skill_id == row.gekisou_skill_id && r.level == level),
        );
        let (Some(exp), Some(level), Some(awake), Some(rank), Some(live), Some(gekisou)) =
            (exp, level, awake, rank, live, gekisou)
        else {
            return None;
        };
        Some(OwnedMember {
            id,
            level: Some(level),
            exp: Some(exp),
            awake,
            rank,
            live_skill_level: live,
            gekisou_skill_level: gekisou,
        })
    }

    /// A skill level: required when the goal reads it and the card has the skill, else 0 (unavailable). A given level
    /// is checked against the skill's range either way; a level the goal reads needs its effect row.
    fn skill_level(
        &mut self,
        path: &str,
        field: &Field,
        skill_id: i64,
        read: bool,
        max: Option<i64>,
        has_effect: impl Fn(i64) -> bool,
    ) -> Option<i64> {
        let read = read && skill_id != 0;
        let fact = self.int(path, field);
        let value = if read {
            self.required(path, fact)?
        } else {
            match fact {
                Fact::Known(v) => v,
                Fact::Unknown => return Some(0),
                Fact::Invalid => return None,
            }
        };
        let value = self.at_least_one(path, Some(value))?;
        if skill_id != 0
            && let Some(max) = max
            && value > max
        {
            self.error(path, "invalid_value", format!("level {value} is above the skill's highest level {max}"));
            return None;
        }
        if !read {
            return Some(0);
        }
        if !has_effect(value) {
            self.error(path, "master_row_missing", format!("no effect row of skill {skill_id} at level {value}"));
            return None;
        }
        Some(value)
    }

    fn snap(
        &mut self,
        master: &Master,
        path: &str,
        id: i64,
        card: &SupportCardSave,
        row: &SupportCardRow,
    ) -> Option<OwnedSnap> {
        let exp_path = format!("{path}._exp");
        let rank_path = format!("{path}._rank");
        let exp = self.experience(&exp_path, &card.exp);
        let rank = self.required_int(&rank_path, &card.rank);
        let rank = self.at_least_one(&rank_path, rank);
        let rank_row = rank.and_then(|r| {
            let found = master.support_card_ranks.iter().find(|x| x.group == row.rank_group && x.rank == r);
            if found.is_none() {
                self.error(&rank_path, "invalid_value", format!("no rank row of group {} at {r}", row.rank_group));
            }
            found
        });
        let level = exp.and_then(|exp| {
            let level = master.support_level_by_exp(row.level_group, exp).map(|r| r.level);
            if level.is_none() {
                self.error(
                    &exp_path,
                    "master_row_missing",
                    format!("no level row of group {} reached", row.level_group),
                );
            }
            level
        });
        let (Some(exp), Some(level), Some(rank_row)) = (exp, level, rank_row) else { return None };
        if level > rank_row.limit_level {
            self.error(
                &exp_path,
                "level_cap",
                format!(
                    "experience {exp} reaches level {level}, above the cap {} at rank {}",
                    rank_row.limit_level, rank_row.rank
                ),
            );
            return None;
        }
        Some(OwnedSnap { id, level: Some(level), exp: Some(exp), rank: rank_row.rank })
    }
}

/// Lists ids for a message, at most ten.
fn id_list(ids: &[i64]) -> String {
    let shown: Vec<String> = ids.iter().take(10).map(i64::to_string).collect();
    if ids.len() > 10 { format!("{} and {} more", shown.join(", "), ids.len() - 10) } else { shown.join(", ") }
}

impl AccountInput {
    /// Parses the document. Structural problems (not JSON, a list or object of the wrong kind, a missing envelope
    /// field or coverage key, a read key given twice) fail here; field values are checked by [`Self::resolve`].
    pub fn from_json(text: &str) -> Result<AccountInput, Error> {
        serde_json::from_str(text).map_err(|e| Error::Input(format!("account: {e}")))
    }

    /// Resolves the account against `data` for `goal`. The account must name the data (`datasetId`, the SHA-256 of
    /// the text it was read from) and suit its region (`server`).
    ///
    /// Every goal reads the VIP rank, the characters, the band items, the memory and, for each owned card that is not
    /// excluded, its experience, awake count and rank (member cards) or experience and rank (snaps). Normal and
    /// Gekisou Live also read `_liveSkillLevel` of the cards with a live skill; Gekisou Live also reads
    /// `_performanceSkillLevel` of the cards with a Gekisou skill.
    pub fn resolve(&self, data: &DeckData, goal: Goal, exclusions: &Exclusions) -> Resolution {
        let master = &data.master;
        let mut out = Resolution::default();
        if master.character_ranks.iter().any(|row| row.exp.is_none()) {
            out.error("datasetId", "unsupported_master", "MasterCharacterRank._exp is required for account input");
        }
        if self.format != FORMAT {
            out.error("format", "unsupported_format", format!("expected {FORMAT}"));
        }
        if data.sha256.as_deref() != Some(self.dataset_id.as_str()) {
            out.error("datasetId", "dataset_mismatch", "the account names another deck data");
        }
        let region = data.provenance.get("region").and_then(serde_json::Value::as_str);
        if region != Some(self.server.data_region()) {
            out.error(
                "server",
                "server_mismatch",
                format!(
                    "a save of this server is read with deck data of region {}, not {}",
                    self.server.data_region(),
                    region.unwrap_or("(none)")
                ),
            );
        }
        if self.revision.is_empty() {
            out.error("revision", "missing_identity", "revision must not be empty");
        }
        for (i, a) in self.assumptions.iter().enumerate() {
            if a.path.is_empty() || a.reason.trim().is_empty() {
                out.error(format!("assumptions[{i}]"), "invalid_assumption", "an assumption needs a path and a reason");
            }
        }
        let player = &self.account.player;
        let coverage = &self.coverage;

        // Cards: ownership, exclusions, then the cultivation of each candidate.
        let member_items = out.identities(
            "_player._memberCards",
            &player.member_cards,
            "_masterId",
            |c| &c.master_id,
            |id| master.member_card(id).is_some(),
            "member card",
        );
        let snap_items = out.identities(
            "_player._supportCards",
            &player.support_cards,
            "_masterId",
            |c| &c.master_id,
            |id| master.support_card(id).is_some(),
            "snap",
        );
        let owned_members: BTreeSet<i64> = member_items.iter().map(|&(_, id)| id).collect();
        let owned_snaps: BTreeSet<i64> = snap_items.iter().map(|&(_, id)| id).collect();
        for (list, ids, owned, what) in [
            ("constraints.excludeMembers", &exclusions.members, &owned_members, "member card"),
            ("constraints.excludeSnaps", &exclusions.snaps, &owned_snaps, "snap"),
        ] {
            for (i, id) in ids.iter().enumerate() {
                if !owned.contains(id) {
                    out.error(format!("{list}[{i}]"), "not_owned", format!("{what} {id} is not an owned card"));
                }
            }
        }
        let mut members = Vec::new();
        for &(i, id) in &member_items {
            if exclusions.members.contains(&id) {
                continue;
            }
            let row = master.member_card(id).expect("identity checked");
            let card = &player.member_cards[i];
            let mark = out.mark();
            if let Some(m) = out.member(master, goal, &format!("_player._memberCards[{i}]"), id, card, row) {
                members.push(m);
            }
            out.label_since(mark, &format!("member card _masterId {id}"));
        }
        let mut snaps = Vec::new();
        for &(i, id) in &snap_items {
            if exclusions.snaps.contains(&id) {
                continue;
            }
            let row = master.support_card(id).expect("identity checked");
            let card = &player.support_cards[i];
            let mark = out.mark();
            if let Some(s) = out.snap(master, &format!("_player._supportCards[{i}]"), id, card, row) {
                snaps.push(s);
            }
            out.label_since(mark, &format!("snap _masterId {id}"));
        }

        // Characters: the rank each reaches with its experience; under complete coverage an unlisted character is at
        // experience 0, so every character counts in the total rank.
        let mut character_ranks = BTreeMap::new();
        let character_items = out.identities(
            "_player._characters",
            &player.characters,
            "_masterId",
            |c| &c.master_id,
            |id| master.character(id).is_some(),
            "character",
        );
        let listed: BTreeSet<i64> = character_items.iter().map(|&(_, id)| id).collect();
        for &(i, id) in &character_items {
            let path = format!("_player._characters[{i}]._exp");
            let mark = out.mark();
            if let Some(exp) = out.experience(&path, &player.characters[i].exp) {
                match master.character_rank_by_exp(exp) {
                    Some(row) => {
                        character_ranks.insert(id, row.rank);
                    }
                    None => out.error(&path, "master_row_missing", format!("no character rank row reached at {exp}")),
                }
            }
            out.label_since(mark, &format!("character _masterId {id}"));
        }
        let unlisted: Vec<i64> = master.characters.iter().map(|c| c.id).filter(|id| !listed.contains(id)).collect();
        if !unlisted.is_empty() {
            match coverage.characters {
                Coverage::Complete => match master.character_rank_by_exp(0) {
                    Some(row) => character_ranks.extend(unlisted.iter().map(|&id| (id, row.rank))),
                    None => {
                        out.error("_player._characters", "master_row_missing", "no character rank row at experience 0")
                    }
                },
                Coverage::Partial => out.missing(
                    "_player._characters",
                    format!("characters {} are not listed and the list is partial", id_list(&unlisted)),
                ),
            }
        }

        // VIP rank: not part of the save, declared by the caller.
        let vip = match self.declared.as_ref().and_then(|d| d.vip.as_ref()) {
            None => Fact::Unknown,
            Some(vip) => out.int("declared._vip._rank", &vip.rank),
        };
        let vip = out.required("declared._vip._rank", vip);
        let vip = out.at_least_one("declared._vip._rank", vip).filter(|&rank| {
            let found = master.vip_rank(rank).is_some();
            if !found {
                out.error("declared._vip._rank", "invalid_value", format!("VIP rank {rank} is not in the deck data"));
            }
            found
        });

        // Band items: level 0 is not built and has no effect.
        let mut band_items = BTreeMap::new();
        let item_list = out.identities(
            "_player._bandItems",
            &player.band_items,
            "_masterId",
            |b| &b.master_id,
            |id| master.band_item(id).is_some(),
            "band item",
        );
        for &(i, id) in &item_list {
            let path = format!("_player._bandItems[{i}]._level");
            let mark = out.mark();
            match out.required_int(&path, &player.band_items[i].level) {
                Some(level) if level < 0 => out.error(&path, "invalid_value", format!("level {level} is negative")),
                Some(0) | None => {}
                Some(level) if master.band_item_level(id, level).is_none() => {
                    out.error(&path, "invalid_value", format!("no level {level}"))
                }
                Some(level) => {
                    band_items.insert(id, level);
                }
            }
            out.label_since(mark, &format!("band item _masterId {id}"));
        }
        if coverage.band_items == Coverage::Partial {
            let listed: BTreeSet<i64> = item_list.iter().map(|&(_, id)| id).collect();
            for row in master.band_items.iter().filter(|row| !listed.contains(&row.id)) {
                out.missing(
                    "_player._bandItems",
                    format!("band item {} is not listed and the list is partial", row.id),
                );
            }
        }

        let memory = resolve_memory(master, player, coverage, &owned_members, &owned_snaps, &mut out);

        if out.errors.is_empty() && out.missing.is_empty() {
            let player = Player {
                explicit_character_total_rank: None,
                character_ranks,
                band_items,
                vip_rank: vip.expect("checked"),
                events: Vec::new(),
                memory: Some(memory),
                owned_member_card_ids: Some(owned_members),
                owned_support_card_ids: Some(owned_snaps),
            };
            let roster = Roster { player, members, snaps };
            match Pool::new(master, &roster) {
                Ok(_) => {
                    out.resolved = Some(ResolvedAccount {
                        goal,
                        dataset_id: self.dataset_id.clone(),
                        server: self.server,
                        revision: self.revision.clone(),
                        coverage: self.coverage.clone(),
                        assumptions: self.assumptions.clone(),
                        roster,
                    })
                }
                Err(e) => out.error("_player", "core_resolution", e.to_string()),
            }
        }
        out
    }
}

/// The memory: each music group with its musics, and the unlocked member cards and snaps. Unlock counts only cover
/// owned cards, so an unlocked card whose ownership is unknown, or an owned card whose unlock is unknown, is missing
/// when the deck data has memory level rows.
fn resolve_memory(
    master: &Master,
    player: &PlayerSave,
    coverage: &CoverageDecl,
    owned_members: &BTreeSet<i64>,
    owned_snaps: &BTreeSet<i64>,
    out: &mut Resolution,
) -> MemoryState {
    let empty = MemorySave::default();
    let memory = player.memory.as_ref().unwrap_or(&empty);
    let mut state = MemoryState { music_groups: Some(BTreeMap::new()), ..MemoryState::default() };

    let groups = out.identities(
        "_player._memory._musicGroups",
        &memory.music_groups,
        "_id",
        |g| &g.id,
        |id| master.memory_music_groups.iter().any(|g| g.id == id),
        "memory music group",
    );
    for &(gi, group_id) in &groups {
        let path = format!("_player._memory._musicGroups[{gi}]._musics");
        let group = &memory.music_groups[gi];
        if group.musics.is_empty() {
            out.error(&path, "invalid_value", format!("memory music group {group_id} lists no music"));
            continue;
        }
        let musics = out.identities(
            &path,
            &group.musics,
            "_id",
            |m| &m.id,
            |id| master.memory_musics.iter().any(|m| m.id == id),
            "memory music",
        );
        let mut list = Vec::new();
        for &(mi, music_id) in &musics {
            let music_path = format!("{path}[{mi}]");
            let mark = out.mark();
            let row_group = master.memory_musics.iter().find(|m| m.id == music_id).map_or(0, |m| m.group_id);
            if row_group != group_id {
                out.error(
                    format!("{music_path}._id"),
                    "invalid_value",
                    format!("memory music {music_id} belongs to group {row_group}, not {group_id}"),
                );
            }
            let rank_path = format!("{music_path}._unlockedScoreRank");
            match out.required_int(&rank_path, &group.musics[mi].unlocked_score_rank) {
                Some(rank) if !SCORE_RANKS.contains(&rank) => {
                    out.error(&rank_path, "invalid_value", format!("score rank {rank} is not in 0 (none) to 7 (SS)"))
                }
                Some(rank) => list.push((music_id, rank)),
                None => {}
            }
            out.label_since(mark, &format!("memory music _id {music_id}"));
        }
        state.music_groups.as_mut().expect("account groups").insert(group_id, list);
    }
    if coverage.memory_music_groups == Coverage::Partial {
        let listed: BTreeSet<i64> = groups.iter().map(|&(_, id)| id).collect();
        for g in master.memory_music_groups.iter().filter(|g| !listed.contains(&g.id)) {
            if master.memory_music_bonuses.iter().any(|r| r.group_id == g.id) {
                out.missing(
                    "_player._memory._musicGroups",
                    format!("memory music group {} is not listed and the list is partial", g.id),
                );
            }
        }
    }

    type Known = fn(&Master, i64) -> bool;
    for (list, cards, card_list, card_coverage, memory_coverage, owned, has_levels, unlocked, what, known) in [
        (
            "_player._memory._members",
            &memory.members,
            "_player._memberCards",
            coverage.member_cards,
            coverage.memory_members,
            owned_members,
            !master.memory_member_levels.is_empty(),
            &mut state.unlocked_members,
            "member card",
            (|m: &Master, id: i64| m.member_card(id).is_some()) as Known,
        ),
        (
            "_player._memory._supports",
            &memory.supports,
            "_player._supportCards",
            coverage.support_cards,
            coverage.memory_supports,
            owned_snaps,
            !master.memory_support_levels.is_empty(),
            &mut state.unlocked_supports,
            "snap",
            (|m: &Master, id: i64| m.support_card(id).is_some()) as Known,
        ),
    ] {
        let items = out.identities(list, cards, "_id", |c| &c.id, |id| known(master, id), what);
        for &(k, id) in &items {
            let mark = out.mark();
            let fact = out.flag(&format!("{list}[{k}]._unlocked"), &cards[k].unlocked);
            if out.required(&format!("{list}[{k}]._unlocked"), fact) == Some(true) {
                unlocked.insert(id);
            }
            out.label_since(mark, &format!("memory {what} _id {id}"));
        }
        if !has_levels {
            continue;
        }
        if card_coverage == Coverage::Partial {
            for id in unlocked.iter().filter(|id| !owned.contains(id)) {
                out.missing(
                    card_list,
                    format!("{what} {id} has an unlocked memory but is not listed, and the list is partial"),
                );
            }
        }
        if memory_coverage == Coverage::Partial {
            let listed: BTreeSet<i64> = items.iter().map(|&(_, id)| id).collect();
            for id in owned.iter().filter(|id| !listed.contains(id)) {
                out.missing(list, format!("owned {what} {id} is not listed and the list is partial"));
            }
            if card_coverage == Coverage::Partial {
                out.missing(list, format!("both {list} and {card_list} are partial: an unlisted card may count"));
            }
        }
    }
    state
}
