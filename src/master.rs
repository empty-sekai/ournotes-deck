//! Master data: the subset of the game's master tables this crate reads.
//!
//! Tables are read from the JSON form `{"_allData": [row, ...]}` (UTF-8 with or without a byte-order mark); the
//! deck data file ([`crate::data`]) supplies them. Unknown columns are ignored and missing columns read as zero /
//! empty. A missing table reads as empty; lookups that need a row report [`Error::Master`] or [`Error::Input`].

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use crate::error::Error;

fn null_vec<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

fn true_default() -> bool {
    true
}

/// A binary32 column: the number is parsed from its text straight to `f32` (correctly rounded, no detour through
/// `f64`); `1e999` / `-1e999` read as infinities.
fn f32_text<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
    let t = raw.get().trim();
    t.parse::<f32>().map_err(|_| serde::de::Error::custom(format!("expected a number, got {t}")))
}

macro_rules! row {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $f:ident : $t:ty = $json:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Default, Deserialize)]
        #[serde(default)]
        pub struct $name {
            $($(#[$fm])* #[serde(rename = $json)] pub $f: $t,)*
        }
    };
}

row!(
    /// `MasterMemberCard`.
    MemberCardRow {
        id: i64 = "_id",
        character_id: i64 = "_characterID",
        rarity: i64 = "_rarity",
        card_type: i64 = "_cardType",
        #[serde(deserialize_with = "null_vec")]
        best_music_tag_ids: Vec<i64> = "_bestMusicTagIDs",
        performance_power_max: i64 = "_performancePowerMax",
        technic_power_max: i64 = "_technicPowerMax",
        visual_power_max: i64 = "_visualPowerMax",
        level_group: i64 = "_memberCardLevelGroup",
        awake_group: i64 = "_memberCardAwakeGroup",
        rank_group: i64 = "_memberCardRankGroup",
        leader_skill_id: i64 = "_leaderSkillID",
        live_skill_id: i64 = "_liveSkillID",
        gekisou_skill_id: i64 = "_gekisouSkillID",
    }
);

row!(
    /// `MasterSupportCard` (snaps).
    SupportCardRow {
        id: i64 = "_id",
        #[serde(deserialize_with = "null_vec")]
        character_ids: Vec<i64> = "_characterIDs",
        rarity: i64 = "_rarity",
        card_type: i64 = "_cardType",
        performance_power_max: i64 = "_performancePowerMax",
        technic_power_max: i64 = "_technicPowerMax",
        visual_power_max: i64 = "_visualPowerMax",
        level_group: i64 = "_supportCardLevelGroup",
        rank_group: i64 = "_supportCardRankGroup",
        support_skill_id_01: i64 = "_supportSkillId01",
        support_skill_id_02: i64 = "_supportSkillId02",
        gekisou_support_skill_id_01: i64 = "_gekisouSupportSkillId01",
        gekisou_support_skill_id_02: i64 = "_gekisouSupportSkillId02",
    }
);

row!(
    /// `MasterMemberCardLevel` and `MasterSupportCardLevel`.
    LevelRow {
        id: i64 = "_id",
        group: i64 = "_group",
        level: i64 = "_level",
        exp: i64 = "_exp",
        performance_rate: i64 = "_performanceRate",
        technic_rate: i64 = "_technicRate",
        visual_rate: i64 = "_visualRate",
    }
);

row!(
    /// `MasterMemberCardAwake`.
    AwakeRow {
        id: i64 = "_id",
        group: i64 = "_group",
        awake_count: i64 = "_awakeCount",
        performance_rate: i64 = "_performanceRate",
        technic_rate: i64 = "_technicRate",
        visual_rate: i64 = "_visualRate",
    }
);

row!(
    /// `MasterMemberCardRank`.
    MemberRankRow {
        id: i64 = "_id",
        group: i64 = "_group",
        rank: i64 = "_rank",
        performance_rate: i64 = "_performanceRate",
        technic_rate: i64 = "_technicRate",
        visual_rate: i64 = "_visualRate",
        leader_skill_level: i64 = "_leaderSkillLevel",
        music_type_bonus_rate: i64 = "_musicTypeBonusRate",
        music_tag_bonus_rate: i64 = "_musicTagBonusRate",
    }
);

row!(
    /// `MasterSupportCardRank`.
    SupportRankRow {
        id: i64 = "_id",
        group: i64 = "_group",
        rank: i64 = "_rank",
        limit_level: i64 = "_limitLevel",
        card_type_link_bonus_rate: i64 = "_cardTypeLinkBonusRate",
        support_skill_01_level: i64 = "_supportSkill01Level",
        support_skill_02_level: i64 = "_supportSkill02Level",
        gekisou_support_skill_01_level: i64 = "_gekisouSupportSkill01Level",
        gekisou_support_skill_02_level: i64 = "_gekisouSupportSkill02Level",
    }
);

row!(
    /// `MasterCharacter`.
    CharacterRow {
        id: i64 = "_id",
        band_id: i64 = "_bandID",
    }
);

row!(
    /// `MasterCharacterRank`.
    CharacterRankRow {
        id: i64 = "_id",
        rank: i64 = "_rank",
        bonus: i64 = "_bonus",
    }
);

row!(
    /// `MasterCharacterTotalRank`.
    CharacterTotalRankRow {
        id: i64 = "_id",
        total_rank: i64 = "_totalRank",
        bonus: i64 = "_bonus",
    }
);

row!(
    /// `MasterBandItemSkillEffect`.
    BandItemEffectRow {
        id: i64 = "_id",
        band_item_id: i64 = "_bandItemId",
        level: i64 = "_level",
        #[serde(deserialize_with = "null_vec")]
        skill_target_ids: Vec<i64> = "_skillTargetIDs",
        skill_effect_type: i64 = "_skillEffectType",
        effect_value: i64 = "_effectValue",
    }
);

row!(
    /// `MasterSkillTarget`.
    SkillTargetRow {
        id: i64 = "_id",
        skill_target_type: i64 = "_skillTargetType",
        character_id: i64 = "_characterID",
        band_id: i64 = "_bandID",
        card_type: i64 = "_cardType",
        tag_id: i64 = "_tagID",
        judgement: i64 = "_judgement",
        live_music_type: i64 = "_liveMusicType",
        gekisou_mission_type: i64 = "_gekisouMissionType",
        #[serde(deserialize_with = "null_vec")]
        live_skill_categories: Vec<i64> = "_liveSkillCategories",
        #[serde(deserialize_with = "null_vec")]
        gekisou_skill_categories: Vec<i64> = "_gekisouSkillCategories",
    }
);

row!(
    /// `MasterLeaderSkillEffect`.
    LeaderSkillEffectRow {
        id: i64 = "_id",
        leader_skill_id: i64 = "_leaderSkillID",
        level: i64 = "_level",
        skill_condition_group: i64 = "_skillConditionGroup",
        #[serde(deserialize_with = "null_vec")]
        skill_target_ids: Vec<i64> = "_skillTargetIDs",
        skill_effect_type: i64 = "_skillEffectType",
        effect_value: i64 = "_effectValue",
        skill_cumulative_condition_id: i64 = "_skillCumulativeConditionID",
    }
);

/// `MasterSkillCondition`.
#[derive(Clone, Debug, Deserialize)]
pub struct SkillConditionRow {
    #[serde(rename = "_id", default)]
    pub id: i64,
    #[serde(rename = "_conditionType", default)]
    pub condition_type: i64,
    #[serde(rename = "_conditionValues", default, deserialize_with = "null_vec")]
    pub condition_values: Vec<i64>,
    #[serde(rename = "_isPositive", default = "true_default")]
    pub is_positive: bool,
    #[serde(rename = "_conditionTargetIDs", default, deserialize_with = "null_vec")]
    pub condition_target_ids: Vec<i64>,
}

row!(
    /// `MasterSkillConditionSet`.
    SkillConditionSetRow {
        id: i64 = "_id",
        group: i64 = "_group",
        #[serde(deserialize_with = "null_vec")]
        condition_ids: Vec<i64> = "_conditionIds",
    }
);

row!(
    /// `MasterSkillCumulativeCondition`.
    CumulativeConditionRow {
        id: i64 = "_id",
        condition_type: i64 = "_skillCumulativeConditionType",
        #[serde(deserialize_with = "null_vec")]
        condition_values: Vec<i64> = "_conditionValues",
        #[serde(deserialize_with = "null_vec")]
        condition_target_ids: Vec<i64> = "_conditionTargetIDs",
        max_cumulative_count: i64 = "_maxCumulativeCount",
    }
);

row!(
    /// `MasterVipRankBonus`.
    VipRankBonusRow {
        id: i64 = "_id",
        vip_rank: i64 = "_vipRank",
        vip_bonus_type: i64 = "_vipBonusType",
        value: i64 = "_value",
    }
);

row!(
    /// `MasterParameter` (string key and value).
    ParameterRow {
        id: String = "_id",
        value: String = "_value",
    }
);

row!(
    /// `MasterLiveSkill` and `MasterGekisouSkill` (the columns used for target matching).
    SkillRow {
        id: i64 = "_id",
        #[serde(deserialize_with = "null_vec")]
        skill_categories: Vec<i64> = "_skillCategories",
        gekisou_mission_type: i64 = "_gekisouMissionType",
    }
);

row!(
    /// `MasterLiveSkillEffect`.
    LiveSkillEffectRow {
        id: i64 = "_id",
        live_skill_id: i64 = "_liveSkillID",
        level: i64 = "_level",
        skill_condition_group: i64 = "_skillConditionGroup",
        skill_release_condition_group: i64 = "_skillReleaseConditionGroup",
        #[serde(deserialize_with = "null_vec")]
        skill_target_ids: Vec<i64> = "_skillTargetIDs",
        skill_effect_type: i64 = "_skillEffectType",
        #[serde(deserialize_with = "f32_text")]
        activation_time_second: f32 = "_activationTimeSecond",
        effect_value: i64 = "_effectValue",
        max_effect_value: i64 = "_maxEffectValue",
        effect_limit_count: i64 = "_effectLimitCount",
        skill_cumulative_condition_id: i64 = "_skillCumulativeConditionID",
        effect_execute_limit_count: i64 = "_effectExecuteLimitCount",
        effect_execute_limit_reset_condition_group: i64 = "_effectExecuteLimitResetConditionGroup",
    }
);

row!(
    /// `MasterSupportSkillEffect` (snap skills).
    SupportSkillEffectRow {
        id: i64 = "_id",
        support_skill_id: i64 = "_supportSkillID",
        level: i64 = "_level",
        skill_trigger_type: i64 = "_skillTriggerType",
        skill_trigger_condition_group: i64 = "_skillTriggerConditionGroup",
        skill_condition_group: i64 = "_skillConditionGroup",
        skill_release_condition_group: i64 = "_skillReleaseConditionGroup",
        #[serde(deserialize_with = "null_vec")]
        skill_target_ids: Vec<i64> = "_skillTargetIDs",
        skill_effect_type: i64 = "_skillEffectType",
        #[serde(deserialize_with = "f32_text")]
        activation_time_second: f32 = "_activationTimeSecond",
        effect_value: i64 = "_effectValue",
        max_effect_value: i64 = "_maxEffectValue",
        effect_limit_count: i64 = "_effectLimitCount",
        skill_cumulative_condition_id: i64 = "_skillCumulativeConditionID",
        effect_execute_limit_count: i64 = "_effectExecuteLimitCount",
        effect_execute_limit_reset_condition_group: i64 = "_effectExecuteLimitResetConditionGroup",
    }
);

row!(
    /// `MasterSkillEffectSetting` (the update phase of each effect type).
    SkillEffectSettingRow {
        id: i64 = "_id",
        skill_effect_type: i64 = "_skillEffectType",
        phase: i64 = "_phase",
    }
);

row!(
    /// `MasterLiveJudgementTiming` (judgement windows per note judgement type).
    LiveJudgementTimingRow {
        id: i64 = "_id",
        assist_level: i64 = "_assistLevel",
        judgement_priority: i64 = "_judgementPriority",
        note_judgement_type: i64 = "_noteJudgementType",
        note_simulate_judgement: i64 = "_noteSimulateJudgement",
        before_ms: i64 = "_beforeMs",
        after_ms: i64 = "_afterMs",
    }
);

row!(
    /// `MasterLiveMusic` (the columns the power and score code read).
    LiveMusicRow {
        id: i64 = "_id",
        music_type: i64 = "_musicType",
        #[serde(deserialize_with = "null_vec")]
        best_music_tag_ids: Vec<i64> = "_bestMusicTagIDs",
        live_score_rank_group: i64 = "_liveScoreRankGroup",
        easy_id: i64 = "_easyID",
        normal_id: i64 = "_normalID",
        hard_id: i64 = "_hardID",
        expert_id: i64 = "_expertID",
    }
);

row!(
    /// `MasterLiveMusicScore`.
    LiveMusicScoreRow {
        id: i64 = "_id",
        music_score_text_file_name: String = "_musicScoreTextFileName",
        music_score_level: i64 = "_musicScoreLevel",
        full_combo_count: i64 = "_fullComboCount",
    }
);

row!(
    /// `MasterLiveNoteParameter`.
    NoteParameterRow {
        id: i64 = "_id",
        note_operate_type: i64 = "_noteOperateType",
        score_percent: i64 = "_scorePercent",
    }
);

row!(
    /// `MasterLiveJudgementParameter`.
    JudgementParameterRow {
        id: i64 = "_id",
        note_simulate_judgement: i64 = "_noteSimulateJudgement",
        score_percent: i64 = "_scorePercent",
        damage: i64 = "_damage",
    }
);

row!(
    /// `MasterLiveComboScoreBonus`.
    ComboScoreBonusRow {
        id: i64 = "_id",
        combo_bonus_type: i64 = "_comboBonusType",
        required_combo_count: i64 = "_requiredComboCount",
        #[serde(deserialize_with = "f32_text")]
        bonus_factor: f32 = "_bonusFactor",
    }
);

row!(
    /// `MasterLiveSettings` (string key and value).
    LiveSettingRow {
        id: i64 = "_id",
        key: String = "_key",
        value: String = "_value",
    }
);

row!(
    /// `MasterMemoryMusicGroup`.
    MemoryMusicGroupRow {
        id: i64 = "_id",
        #[serde(deserialize_with = "null_vec")]
        skill_target_ids: Vec<i64> = "_skillTargetIds",
    }
);

row!(
    /// `MasterMemoryMusic`.
    MemoryMusicRow {
        id: i64 = "_id",
        group_id: i64 = "_groupId",
    }
);

row!(
    /// `MasterMemoryMusicBonus`.
    MemoryMusicBonusRow {
        id: i64 = "_id",
        group_id: i64 = "_groupId",
        score_rank: i64 = "_scoreRank",
        performance: i64 = "_performance",
        technic: i64 = "_technic",
        visual: i64 = "_visual",
    }
);

row!(
    /// `MasterMemoryMemberLevel` and `MasterMemorySupportLevel`.
    MemoryLevelRow {
        id: i64 = "_id",
        point: i64 = "_point",
        performance: i64 = "_performance",
        technic: i64 = "_technic",
        visual: i64 = "_visual",
    }
);

row!(
    /// `MasterEventEffect`.
    EventEffectRow {
        id: i64 = "_id",
        event_id: i64 = "_eventId",
        event_bonus_type: i64 = "_eventBonusType",
        resource_type_constraint: i64 = "_resourceTypeConstraint",
        character_id: i64 = "_characterId",
        band_id: i64 = "_bandId",
        card_type: i64 = "_cardType",
        tag_id: i64 = "_tagId",
        member_card_id: i64 = "_memberCardId",
        support_card_id: i64 = "_supportCardId",
        rank1_effect_value: i64 = "_rank1EffectValue",
        rank2_effect_value: i64 = "_rank2EffectValue",
        rank3_effect_value: i64 = "_rank3EffectValue",
        rank4_effect_value: i64 = "_rank4EffectValue",
        rank5_effect_value: i64 = "_rank5EffectValue",
    }
);

row!(
    /// `MasterBand`.
    BandRow {
        id: i64 = "_id",
    }
);

row!(
    /// `MasterEvent` (the columns the event points read).
    EventRow {
        id: i64 = "_id",
        event_type: i64 = "_eventType",
        live_event_point_group: i64 = "_liveEventPointGroup",
        challenge_live_event_point_group: i64 = "_challengeLiveEventPointGroup",
    }
);

row!(
    /// `MasterLiveEventPoint`, `MasterChallengeLiveEventPoint` and `MasterLiveChallengePoint`.
    EventPointRow {
        id: i64 = "_id",
        group: i64 = "_group",
        score_rank: i64 = "_scoreRank",
        value: i64 = "_value",
    }
);

row!(
    /// `MasterLiveMusicBoostBonus` and `MasterChallengeMusicBoostBonus`.
    BoostBonusRow {
        id: i64 = "_id",
        #[serde(alias = "_consumedChallengePointCount")]
        consumed_count: i64 = "_consumedLiveBoostCount",
        live_music_reward_rate: i64 = "_liveMusicRewardRate",
        player_exp_rate: i64 = "_playerExpRate",
        member_card_exp_rate: i64 = "_memberCardExpRate",
        friendship_exp_rate: i64 = "_friendshipExpRate",
        event_point_rate: i64 = "_eventPointRate",
    }
);

impl BoostBonusRow {
    /// `(live music reward, player exp, member exp, friendship exp, event point)`.
    pub fn rates(&self) -> [i64; 5] {
        [
            self.live_music_reward_rate,
            self.player_exp_rate,
            self.member_card_exp_rate,
            self.friendship_exp_rate,
            self.event_point_rate,
        ]
    }
}

row!(
    /// `MasterLiveScoreRank`.
    LiveScoreRankRow {
        id: i64 = "_id",
        group: i64 = "_group",
        live_score_rank: i64 = "_liveScoreRank",
        required_score: i64 = "_requiredScore",
        battle_live_required_score: i64 = "_battleLiveRequiredScore",
    }
);

#[derive(Deserialize)]
struct TableFile<T> {
    #[serde(rename = "_allData")]
    all_data: Vec<T>,
}

/// The master tables, in table order, with the id indexes the lookups need.
#[derive(Clone, Debug, Default)]
pub struct Master {
    pub member_cards: Vec<MemberCardRow>,
    pub support_cards: Vec<SupportCardRow>,
    pub member_card_levels: Vec<LevelRow>,
    pub member_card_awakes: Vec<AwakeRow>,
    pub member_card_ranks: Vec<MemberRankRow>,
    pub support_card_levels: Vec<LevelRow>,
    pub support_card_ranks: Vec<SupportRankRow>,
    pub characters: Vec<CharacterRow>,
    pub character_ranks: Vec<CharacterRankRow>,
    pub character_total_ranks: Vec<CharacterTotalRankRow>,
    pub band_item_effects: Vec<BandItemEffectRow>,
    pub skill_targets: Vec<SkillTargetRow>,
    pub leader_skill_effects: Vec<LeaderSkillEffectRow>,
    pub skill_conditions: Vec<SkillConditionRow>,
    pub skill_condition_sets: Vec<SkillConditionSetRow>,
    pub cumulative_conditions: Vec<CumulativeConditionRow>,
    pub vip_rank_bonuses: Vec<VipRankBonusRow>,
    pub parameters: Vec<ParameterRow>,
    pub live_skills: Vec<SkillRow>,
    pub gekisou_skills: Vec<SkillRow>,
    pub live_skill_effects: Vec<LiveSkillEffectRow>,
    pub live_musics: Vec<LiveMusicRow>,
    pub live_music_scores: Vec<LiveMusicScoreRow>,
    pub note_parameters: Vec<NoteParameterRow>,
    pub judgement_parameters: Vec<JudgementParameterRow>,
    pub combo_score_bonuses: Vec<ComboScoreBonusRow>,
    pub live_settings: Vec<LiveSettingRow>,
    pub memory_music_groups: Vec<MemoryMusicGroupRow>,
    pub memory_musics: Vec<MemoryMusicRow>,
    pub memory_music_bonuses: Vec<MemoryMusicBonusRow>,
    pub memory_member_levels: Vec<MemoryLevelRow>,
    pub memory_support_levels: Vec<MemoryLevelRow>,
    pub event_effects: Vec<EventEffectRow>,
    pub bands: Vec<BandRow>,
    pub events: Vec<EventRow>,
    pub live_event_points: Vec<EventPointRow>,
    pub challenge_live_event_points: Vec<EventPointRow>,
    pub live_challenge_points: Vec<EventPointRow>,
    pub live_music_boost_bonuses: Vec<BoostBonusRow>,
    pub challenge_music_boost_bonuses: Vec<BoostBonusRow>,
    pub live_score_ranks: Vec<LiveScoreRankRow>,
    pub support_skill_effects: Vec<SupportSkillEffectRow>,
    pub skill_effect_settings: Vec<SkillEffectSettingRow>,
    pub live_judgement_timings: Vec<LiveJudgementTimingRow>,
    index: Index,
}

#[derive(Clone, Debug, Default)]
struct Index {
    member_card: HashMap<i64, usize>,
    support_card: HashMap<i64, usize>,
    character: HashMap<i64, usize>,
    skill_target: HashMap<i64, usize>,
    skill_condition: HashMap<i64, usize>,
    cumulative_condition: HashMap<i64, usize>,
    live_skill: HashMap<i64, usize>,
    gekisou_skill: HashMap<i64, usize>,
    live_music: HashMap<i64, usize>,
    live_music_score: HashMap<i64, usize>,
    band: HashMap<i64, usize>,
    event: HashMap<i64, usize>,
}

/// The table names [`Master`] reads.
pub const TABLES: &[&str] = &[
    "MasterMemberCard",
    "MasterSupportCard",
    "MasterMemberCardLevel",
    "MasterMemberCardAwake",
    "MasterMemberCardRank",
    "MasterSupportCardLevel",
    "MasterSupportCardRank",
    "MasterCharacter",
    "MasterCharacterRank",
    "MasterCharacterTotalRank",
    "MasterBandItemSkillEffect",
    "MasterSkillTarget",
    "MasterLeaderSkillEffect",
    "MasterSkillCondition",
    "MasterSkillConditionSet",
    "MasterSkillCumulativeCondition",
    "MasterVipRankBonus",
    "MasterParameter",
    "MasterLiveSkill",
    "MasterGekisouSkill",
    "MasterLiveSkillEffect",
    "MasterLiveMusic",
    "MasterLiveMusicScore",
    "MasterLiveNoteParameter",
    "MasterLiveJudgementParameter",
    "MasterLiveComboScoreBonus",
    "MasterLiveSettings",
    "MasterMemoryMusicGroup",
    "MasterMemoryMusic",
    "MasterMemoryMusicBonus",
    "MasterMemoryMemberLevel",
    "MasterMemorySupportLevel",
    "MasterEventEffect",
    "MasterBand",
    "MasterEvent",
    "MasterLiveEventPoint",
    "MasterChallengeLiveEventPoint",
    "MasterLiveChallengePoint",
    "MasterLiveMusicBoostBonus",
    "MasterChallengeMusicBoostBonus",
    "MasterLiveScoreRank",
    "MasterSupportSkillEffect",
    "MasterSkillEffectSetting",
    "MasterLiveJudgementTiming",
];

fn parse_table<T: DeserializeOwned>(name: &str, text: Option<&str>) -> Result<Vec<T>, Error> {
    let Some(text) = text else { return Ok(Vec::new()) };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: TableFile<T> = serde_json::from_str(text).map_err(|e| Error::Master(format!("{name}: {e}")))?;
    Ok(file.all_data)
}

fn index_by<T>(name: &str, rows: &[T], id: impl Fn(&T) -> i64) -> Result<HashMap<i64, usize>, Error> {
    let mut m = HashMap::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        if m.insert(id(r), i).is_some() {
            return Err(Error::Master(format!("{name}: duplicate _id {}", id(r))));
        }
    }
    Ok(m)
}

impl Master {
    /// Builds the master from table texts; `get(name)` returns the JSON of a table or `None` when it is absent.
    pub fn from_json_tables<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Result<Master, Error> {
        let mut m = Master {
            member_cards: parse_table("MasterMemberCard", get("MasterMemberCard"))?,
            support_cards: parse_table("MasterSupportCard", get("MasterSupportCard"))?,
            member_card_levels: parse_table("MasterMemberCardLevel", get("MasterMemberCardLevel"))?,
            member_card_awakes: parse_table("MasterMemberCardAwake", get("MasterMemberCardAwake"))?,
            member_card_ranks: parse_table("MasterMemberCardRank", get("MasterMemberCardRank"))?,
            support_card_levels: parse_table("MasterSupportCardLevel", get("MasterSupportCardLevel"))?,
            support_card_ranks: parse_table("MasterSupportCardRank", get("MasterSupportCardRank"))?,
            characters: parse_table("MasterCharacter", get("MasterCharacter"))?,
            character_ranks: parse_table("MasterCharacterRank", get("MasterCharacterRank"))?,
            character_total_ranks: parse_table("MasterCharacterTotalRank", get("MasterCharacterTotalRank"))?,
            band_item_effects: parse_table("MasterBandItemSkillEffect", get("MasterBandItemSkillEffect"))?,
            skill_targets: parse_table("MasterSkillTarget", get("MasterSkillTarget"))?,
            leader_skill_effects: parse_table("MasterLeaderSkillEffect", get("MasterLeaderSkillEffect"))?,
            skill_conditions: parse_table("MasterSkillCondition", get("MasterSkillCondition"))?,
            skill_condition_sets: parse_table("MasterSkillConditionSet", get("MasterSkillConditionSet"))?,
            cumulative_conditions: parse_table(
                "MasterSkillCumulativeCondition",
                get("MasterSkillCumulativeCondition"),
            )?,
            vip_rank_bonuses: parse_table("MasterVipRankBonus", get("MasterVipRankBonus"))?,
            parameters: parse_table("MasterParameter", get("MasterParameter"))?,
            live_skills: parse_table("MasterLiveSkill", get("MasterLiveSkill"))?,
            gekisou_skills: parse_table("MasterGekisouSkill", get("MasterGekisouSkill"))?,
            live_skill_effects: parse_table("MasterLiveSkillEffect", get("MasterLiveSkillEffect"))?,
            live_musics: parse_table("MasterLiveMusic", get("MasterLiveMusic"))?,
            live_music_scores: parse_table("MasterLiveMusicScore", get("MasterLiveMusicScore"))?,
            note_parameters: parse_table("MasterLiveNoteParameter", get("MasterLiveNoteParameter"))?,
            judgement_parameters: parse_table("MasterLiveJudgementParameter", get("MasterLiveJudgementParameter"))?,
            combo_score_bonuses: parse_table("MasterLiveComboScoreBonus", get("MasterLiveComboScoreBonus"))?,
            live_settings: parse_table("MasterLiveSettings", get("MasterLiveSettings"))?,
            memory_music_groups: parse_table("MasterMemoryMusicGroup", get("MasterMemoryMusicGroup"))?,
            memory_musics: parse_table("MasterMemoryMusic", get("MasterMemoryMusic"))?,
            memory_music_bonuses: parse_table("MasterMemoryMusicBonus", get("MasterMemoryMusicBonus"))?,
            memory_member_levels: parse_table("MasterMemoryMemberLevel", get("MasterMemoryMemberLevel"))?,
            memory_support_levels: parse_table("MasterMemorySupportLevel", get("MasterMemorySupportLevel"))?,
            event_effects: parse_table("MasterEventEffect", get("MasterEventEffect"))?,
            bands: parse_table("MasterBand", get("MasterBand"))?,
            events: parse_table("MasterEvent", get("MasterEvent"))?,
            live_event_points: parse_table("MasterLiveEventPoint", get("MasterLiveEventPoint"))?,
            challenge_live_event_points: parse_table(
                "MasterChallengeLiveEventPoint",
                get("MasterChallengeLiveEventPoint"),
            )?,
            live_challenge_points: parse_table("MasterLiveChallengePoint", get("MasterLiveChallengePoint"))?,
            live_music_boost_bonuses: parse_table("MasterLiveMusicBoostBonus", get("MasterLiveMusicBoostBonus"))?,
            challenge_music_boost_bonuses: parse_table(
                "MasterChallengeMusicBoostBonus",
                get("MasterChallengeMusicBoostBonus"),
            )?,
            live_score_ranks: parse_table("MasterLiveScoreRank", get("MasterLiveScoreRank"))?,
            support_skill_effects: parse_table("MasterSupportSkillEffect", get("MasterSupportSkillEffect"))?,
            skill_effect_settings: parse_table("MasterSkillEffectSetting", get("MasterSkillEffectSetting"))?,
            live_judgement_timings: parse_table("MasterLiveJudgementTiming", get("MasterLiveJudgementTiming"))?,
            index: Index::default(),
        };
        m.reindex()?;
        Ok(m)
    }

    /// Rebuilds the id indexes after the tables were edited in place.
    pub fn reindex(&mut self) -> Result<(), Error> {
        self.index = Index {
            member_card: index_by("MasterMemberCard", &self.member_cards, |r| r.id)?,
            support_card: index_by("MasterSupportCard", &self.support_cards, |r| r.id)?,
            character: index_by("MasterCharacter", &self.characters, |r| r.id)?,
            skill_target: index_by("MasterSkillTarget", &self.skill_targets, |r| r.id)?,
            skill_condition: index_by("MasterSkillCondition", &self.skill_conditions, |r| r.id)?,
            cumulative_condition: index_by("MasterSkillCumulativeCondition", &self.cumulative_conditions, |r| r.id)?,
            live_skill: index_by("MasterLiveSkill", &self.live_skills, |r| r.id)?,
            gekisou_skill: index_by("MasterGekisouSkill", &self.gekisou_skills, |r| r.id)?,
            live_music: index_by("MasterLiveMusic", &self.live_musics, |r| r.id)?,
            live_music_score: index_by("MasterLiveMusicScore", &self.live_music_scores, |r| r.id)?,
            band: index_by("MasterBand", &self.bands, |r| r.id)?,
            event: index_by("MasterEvent", &self.events, |r| r.id)?,
        };
        Ok(())
    }

    pub fn member_card(&self, id: i64) -> Option<&MemberCardRow> {
        self.index.member_card.get(&id).map(|&i| &self.member_cards[i])
    }

    pub fn support_card(&self, id: i64) -> Option<&SupportCardRow> {
        self.index.support_card.get(&id).map(|&i| &self.support_cards[i])
    }

    pub fn character(&self, id: i64) -> Option<&CharacterRow> {
        self.index.character.get(&id).map(|&i| &self.characters[i])
    }

    pub fn skill_target(&self, id: i64) -> Option<&SkillTargetRow> {
        self.index.skill_target.get(&id).map(|&i| &self.skill_targets[i])
    }

    pub fn skill_condition(&self, id: i64) -> Option<&SkillConditionRow> {
        self.index.skill_condition.get(&id).map(|&i| &self.skill_conditions[i])
    }

    pub fn cumulative_condition(&self, id: i64) -> Option<&CumulativeConditionRow> {
        self.index.cumulative_condition.get(&id).map(|&i| &self.cumulative_conditions[i])
    }

    pub fn live_skill(&self, id: i64) -> Option<&SkillRow> {
        self.index.live_skill.get(&id).map(|&i| &self.live_skills[i])
    }

    pub fn gekisou_skill(&self, id: i64) -> Option<&SkillRow> {
        self.index.gekisou_skill.get(&id).map(|&i| &self.gekisou_skills[i])
    }

    pub fn live_music(&self, id: i64) -> Option<&LiveMusicRow> {
        self.index.live_music.get(&id).map(|&i| &self.live_musics[i])
    }

    pub fn live_music_score(&self, id: i64) -> Option<&LiveMusicScoreRow> {
        self.index.live_music_score.get(&id).map(|&i| &self.live_music_scores[i])
    }

    pub fn band(&self, id: i64) -> Option<&BandRow> {
        self.index.band.get(&id).map(|&i| &self.bands[i])
    }

    pub fn event(&self, id: i64) -> Option<&EventRow> {
        self.index.event.get(&id).map(|&i| &self.events[i])
    }

    /// `MasterParameter` value by key.
    pub fn parameter(&self, key: &str) -> Option<&str> {
        self.parameters.iter().find(|r| r.id == key).map(|r| r.value.as_str())
    }

    /// The member level row of `group` reached with `exp`: the group's rows in level order, the last one whose
    /// `_exp <= exp` before the first that is not.
    pub fn member_level_by_exp(&self, group: i64, exp: i64) -> Option<&LevelRow> {
        level_by_exp(&self.member_card_levels, group, exp)
    }

    /// The snap level row of `group` reached with `exp` (same rule as members).
    pub fn support_level_by_exp(&self, group: i64, exp: i64) -> Option<&LevelRow> {
        level_by_exp(&self.support_card_levels, group, exp)
    }

    /// The first member level row with this group and level.
    pub fn member_level(&self, group: i64, level: i64) -> Option<&LevelRow> {
        self.member_card_levels.iter().find(|r| r.group == group && r.level == level)
    }

    /// The first snap level row with this group and level.
    pub fn support_level(&self, group: i64, level: i64) -> Option<&LevelRow> {
        self.support_card_levels.iter().find(|r| r.group == group && r.level == level)
    }

    /// The first awake row with this group and awake count.
    pub fn member_awake(&self, group: i64, awake_count: i64) -> Option<&AwakeRow> {
        self.member_card_awakes.iter().find(|r| r.group == group && r.awake_count == awake_count)
    }

    /// The first member rank row with this group and rank.
    pub fn member_rank(&self, group: i64, rank: i64) -> Option<&MemberRankRow> {
        self.member_card_ranks.iter().find(|r| r.group == group && r.rank == rank)
    }

    /// `MasterVipRankBonus` value of the first row with this rank and type, else 0.
    pub fn vip_bonus(&self, bonus_type: i64, rank: i64) -> i64 {
        self.vip_rank_bonuses
            .iter()
            .find(|r| r.vip_rank == rank && r.vip_bonus_type == bonus_type)
            .map_or(0, |r| r.value)
    }
}

fn level_by_exp(rows: &[LevelRow], group: i64, exp: i64) -> Option<&LevelRow> {
    let mut g: Vec<&LevelRow> = rows.iter().filter(|r| r.group == group).collect();
    g.sort_by_key(|r| r.level);
    let mut last = None;
    for r in g {
        if r.exp > exp {
            break;
        }
        last = Some(r);
    }
    last
}
