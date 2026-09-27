//! Synthetic master tables and rosters for tests. Every number here is made up; the tables only share the column
//! layout the crate reads.

#![allow(dead_code)]

use ournotes_deck::cards::{OwnedMember, OwnedSnap, Player, Roster};
use ournotes_deck::live::skip::{Chart, ChartNote, SkillEvent};
use ournotes_deck::master::Master;
use serde_json::{Value, json};

/// xorshift64* with a fixed seed: reproducible without extra dependencies.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }
    pub fn chance(&mut self, p: f64) -> bool {
        ((self.next() >> 11) as f64 / ((1u64 << 53) as f64)) < p
    }
}

pub struct Synth {
    pub tables: Vec<(String, Value)>,
}

impl Synth {
    pub fn master(&self) -> Master {
        let texts: Vec<(String, String)> =
            self.tables.iter().map(|(n, v)| (n.clone(), json!({ "_allData": v }).to_string())).collect();
        Master::from_json_tables(|name| texts.iter().find(|(n, _)| n == name).map(|(_, t)| t.as_str())).unwrap()
    }
}

pub const CHARACTERS: i64 = 9;
pub const BANDS: i64 = 3;

/// A synthetic master with `members` member cards and `snaps` snaps.
pub fn synth(rng: &mut Rng, members: i64, snaps: i64) -> Synth {
    let mut t: Vec<(String, Value)> = Vec::new();
    let mut add = |n: &str, v: Value| t.push((n.to_string(), v));
    add(
        "MasterParameter",
        json!([
            {"_id": "music_type_base_bonus_rate", "_value": "400"},
            {"_id": "music_tag_base_bonus_rate", "_value": "300"},
            {"_id": "type_link_base_bonus_rate", "_value": "600"},
        ]),
    );
    add(
        "MasterCharacter",
        Value::Array((1..=CHARACTERS).map(|c| json!({"_id": c, "_bandID": (c - 1) % BANDS + 1})).collect()),
    );
    add(
        "MasterCharacterRank",
        Value::Array((1..=30).map(|r| json!({"_id": r, "_rank": r, "_bonus": (r - 1) * 7})).collect()),
    );
    add(
        "MasterCharacterTotalRank",
        Value::Array((0..20).map(|k| json!({"_id": k + 1, "_totalRank": 10 + 12 * k, "_bonus": 4 * k})).collect()),
    );
    let mut lv = Vec::new();
    for g in 1..=2 {
        for l in 1..=40 {
            let r = 4000 + (6000 * (l - 1)) / 39;
            lv.push(json!({"_id": g * 100 + l, "_group": g, "_level": l, "_exp": (l - 1) * (l + 3) * 13,
                           "_performanceRate": r, "_technicRate": r + g * 3, "_visualRate": r - g}));
        }
    }
    add("MasterMemberCardLevel", Value::Array(lv));
    add(
        "MasterMemberCardAwake",
        Value::Array(
            (1..=5)
                .map(|a| {
                    json!({"_id": a, "_group": 1, "_awakeCount": a, "_performanceRate": (a - 1) * 230,
            "_technicRate": (a - 1) * 210, "_visualRate": (a - 1) * 250})
                })
                .collect(),
        ),
    );
    add(
        "MasterMemberCardRank",
        Value::Array(
            (1..=5)
                .map(|r| {
                    json!({"_id": r, "_group": 1, "_rank": r, "_performanceRate": (r - 1) * 190,
            "_technicRate": (r - 1) * 220, "_visualRate": (r - 1) * 170, "_leaderSkillLevel": r,
            "_musicTypeBonusRate": (r - 1) * 350, "_musicTagBonusRate": (r - 1) * 450})
                })
                .collect(),
        ),
    );
    let mut slv = Vec::new();
    for l in 1..=30 {
        let r = 3900 + (6100 * (l - 1)) / 29;
        slv.push(json!({"_id": l, "_group": 1, "_level": l, "_exp": (l - 1) * 50, "_performanceRate": r,
                        "_technicRate": r, "_visualRate": r}));
    }
    add("MasterSupportCardLevel", Value::Array(slv));
    add(
        "MasterSupportCardRank",
        Value::Array(
            (1..=5)
                .map(|r| {
                    json!({"_id": r, "_group": 1, "_rank": r, "_limitLevel": 30,
            "_cardTypeLinkBonusRate": (r - 1) * 450})
                })
                .collect(),
        ),
    );
    // targets: 1..3 bands, 4..8 card types, 9 live-skill category 1, 10 character 1
    let mut targets = Vec::new();
    for b in 1..=BANDS {
        targets.push(json!({"_id": b, "_skillTargetType": 1, "_bandID": b}));
    }
    for ty in 1..=5 {
        targets.push(json!({"_id": 3 + ty, "_skillTargetType": 2, "_cardType": ty}));
    }
    targets.push(json!({"_id": 9, "_skillTargetType": 3, "_liveSkillCategories": [1]}));
    targets.push(json!({"_id": 10, "_skillTargetType": 3, "_characterID": 1}));
    targets.push(json!({"_id": 11, "_skillTargetType": 5, "_liveMusicType": 2}));
    targets.push(json!({"_id": 12, "_skillTargetType": 4, "_judgement": 5}));
    targets.push(json!({"_id": 13, "_skillTargetType": 4, "_judgement": 6}));
    add("MasterSkillTarget", Value::Array(targets));
    // conditions: 1 any member of band 1; 2 all members of type 1..5 (always true); 3 song type 2
    add(
        "MasterSkillCondition",
        json!([
            {"_id": 1, "_conditionType": 3000, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": [1]},
            {"_id": 2, "_conditionType": 3001, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": [4, 5, 6, 7, 8]},
            {"_id": 3, "_conditionType": 4012, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": [11]},
            {"_id": 4, "_conditionType": 3001, "_conditionValues": [], "_isPositive": true, "_conditionTargetIDs": [2]},
            {"_id": 5, "_conditionType": 2001, "_conditionValues": [650], "_isPositive": true, "_conditionTargetIDs": []},
            {"_id": 6, "_conditionType": 2001, "_conditionValues": [650], "_isPositive": false, "_conditionTargetIDs": []},
            {"_id": 7, "_conditionType": 2003, "_conditionValues": [400], "_isPositive": true, "_conditionTargetIDs": []},
        ]),
    );
    add(
        "MasterSkillConditionSet",
        json!([
            {"_id": 1, "_group": 1, "_conditionIds": [1]},
            {"_id": 2, "_group": 2, "_conditionIds": [2, 3]},
            {"_id": 3, "_group": 3, "_conditionIds": [4]},
            {"_id": 4, "_group": 4, "_conditionIds": [5]},
            {"_id": 5, "_group": 5, "_conditionIds": [6]},
            {"_id": 6, "_group": 5, "_conditionIds": [7]},
        ]),
    );
    add(
        "MasterSkillCumulativeCondition",
        json!([
            {"_id": 1, "_skillCumulativeConditionType": 3000, "_conditionTargetIDs": [1], "_maxCumulativeCount": 3},
            {"_id": 2, "_skillCumulativeConditionType": 3001, "_conditionTargetIDs": [4, 5], "_maxCumulativeCount": 0},
            {"_id": 3, "_skillCumulativeConditionType": 3002, "_conditionTargetIDs": [], "_maxCumulativeCount": 0},
            {"_id": 4, "_skillCumulativeConditionType": 3003, "_conditionTargetIDs": [], "_maxCumulativeCount": 2},
            {"_id": 5, "_skillCumulativeConditionType": 3004, "_conditionTargetIDs": [], "_maxCumulativeCount": 0},
            {"_id": 6, "_skillCumulativeConditionType": 3005, "_conditionTargetIDs": [], "_maxCumulativeCount": 4},
        ]),
    );
    // leader skills 1..6 simple, 7..12 conditional or cumulative
    let mut le = Vec::new();
    let mut id = 1;
    for skill in 1..=12i64 {
        for level in 1..=5i64 {
            let (group, targets, ty, value, cum) = match skill {
                1..=3 => (0, json!([skill]), 1000 + (skill % 4), 300 + 200 * level, 0),
                4 => (0, json!([]), 1000, 150 + 60 * level, 0),
                5 => (0, json!([4, 6]), 1002, 500 + 300 * level, 0),
                6 => (0, json!([9]), 1001, 700 + 200 * level, 0),
                7 => (1, json!([1, 2]), 1000, 200 + 100 * level, 0),
                8 => (2, json!([]), 1003, 400 + 90 * level, 0),
                9 => (0, json!([3]), 1500, 80 * level, 1),
                10 => (3, json!([]), 1501 + (level % 3), 60 * level, 1 + level % 6),
                11 => (0, json!([]), 1500, 50 * level, 5),
                _ => (0, json!([2, 7]), 1503, 70 * level, 6),
            };
            le.push(json!({"_id": id, "_leaderSkillID": skill, "_level": level, "_skillConditionGroup": group,
                           "_skillTargetIDs": targets, "_skillEffectType": ty, "_effectValue": value,
                           "_skillCumulativeConditionID": cum}));
            id += 1;
            if skill == 4 {
                le.push(json!({"_id": id, "_leaderSkillID": skill, "_level": level, "_skillConditionGroup": 0,
                               "_skillTargetIDs": [10], "_skillEffectType": 1003, "_effectValue": 100 * level,
                               "_skillCumulativeConditionID": 0}));
                id += 1;
            }
        }
    }
    add("MasterLeaderSkillEffect", Value::Array(le));
    let mut bi = Vec::new();
    for b in 1..=BANDS {
        for l in 1..=10 {
            bi.push(json!({"_id": b * 100 + l, "_bandItemId": 100 + b, "_level": l, "_skillTargetIDs": [b],
                           "_skillEffectType": 1000, "_effectValue": 13 * l}));
        }
    }
    bi.push(json!({"_id": 999, "_bandItemId": 200, "_level": 1, "_skillTargetIDs": [4, 10], "_skillEffectType": 1002, "_effectValue": 77}));
    add("MasterBandItemSkillEffect", Value::Array(bi));
    add(
        "MasterVipRankBonus",
        Value::Array(
            (2..=8).map(|r| json!({"_id": r, "_vipRank": r, "_vipBonusType": 7, "_value": 90 * (r - 1)})).collect(),
        ),
    );
    add(
        "MasterLiveSkill",
        json!([{"_id": 1, "_skillCategories": [1]}, {"_id": 2, "_skillCategories": [2]}, {"_id": 3, "_skillCategories": [1, 3]}]),
    );
    let mut lse = Vec::new();
    let mut id = 1;
    for skill in 1..=3i64 {
        for level in 1..=5i64 {
            let rows: Vec<Value> = match skill {
                1 => vec![json!({"_skillConditionGroup": 0, "_skillTargetIDs": [], "_skillEffectType": 2000,
                                 "_activationTimeSecond": 5.0, "_effectValue": 700 + 150 * level})],
                2 => vec![json!({"_skillConditionGroup": 0, "_skillTargetIDs": [12, 13], "_skillEffectType": 2004,
                                 "_activationTimeSecond": 4.5, "_effectValue": 1500 + 250 * level})],
                _ => vec![
                    json!({"_skillConditionGroup": 4, "_skillTargetIDs": [], "_skillEffectType": 2000,
                           "_activationTimeSecond": 6.0, "_effectValue": 1100 + 200 * level}),
                    json!({"_skillConditionGroup": 5, "_skillTargetIDs": [], "_skillEffectType": 2000,
                           "_activationTimeSecond": 6.0, "_effectValue": 500 + 100 * level}),
                ],
            };
            for mut r in rows {
                r["_id"] = json!(id);
                r["_liveSkillID"] = json!(skill);
                r["_level"] = json!(level);
                lse.push(r);
                id += 1;
            }
        }
    }
    add("MasterLiveSkillEffect", Value::Array(lse));
    add("MasterGekisouSkill", json!([{"_id": 1, "_skillCategories": [1], "_gekisouMissionType": 1}]));
    let mut mc = Vec::new();
    for i in 1..=members {
        let rarity = rng.range(2, 4);
        let top = 6000 + rarity * 1500;
        mc.push(
            json!({"_id": i, "_characterID": rng.range(1, CHARACTERS), "_rarity": rarity, "_cardType": rng.range(1, 5),
            "_bestMusicTagIDs": if rng.chance(0.5) { json!([rng.range(1, 3)]) } else { json!([]) },
            "_performancePowerMax": rng.range(top / 2, top), "_technicPowerMax": rng.range(top / 2, top),
            "_visualPowerMax": rng.range(top / 2, top), "_memberCardLevelGroup": rng.range(1, 2),
            "_memberCardAwakeGroup": 1, "_memberCardRankGroup": 1, "_leaderSkillID": rng.range(1, 12),
            "_liveSkillID": rng.range(1, 3), "_gekisouSkillID": 1}),
        );
    }
    add("MasterMemberCard", Value::Array(mc));
    let mut sc = Vec::new();
    for i in 1..=snaps {
        sc.push(json!({"_id": i, "_characterIDs": [rng.range(1, CHARACTERS)], "_rarity": rng.range(2, 4),
            "_cardType": rng.range(1, 5), "_performancePowerMax": rng.range(200, 900), "_technicPowerMax": rng.range(200, 900),
            "_visualPowerMax": rng.range(200, 900), "_supportCardLevelGroup": 1, "_supportCardRankGroup": 1}));
    }
    add("MasterSupportCard", Value::Array(sc));
    add(
        "MasterLiveMusic",
        json!([
            {"_id": 10, "_musicType": 1, "_bestMusicTagIDs": [1], "_easyID": 1001, "_normalID": 1002, "_hardID": 1003, "_expertID": 1004},
            {"_id": 20, "_musicType": 2, "_bestMusicTagIDs": [2, 3], "_easyID": 2001, "_normalID": 2002, "_hardID": 2003, "_expertID": 2004},
            {"_id": 30, "_musicType": 99, "_bestMusicTagIDs": [], "_easyID": 3001, "_normalID": 3002, "_hardID": 3003, "_expertID": 3004},
        ]),
    );
    add(
        "MasterLiveMusicScore",
        json!([
            {"_id": 1004, "_musicScoreLevel": 24}, {"_id": 2003, "_musicScoreLevel": 18}, {"_id": 3001, "_musicScoreLevel": 7},
        ]),
    );
    add(
        "MasterLiveNoteParameter",
        json!([
            {"_id": 1, "_noteOperateType": 1, "_scorePercent": 100}, {"_id": 2, "_noteOperateType": 20, "_scorePercent": 90},
            {"_id": 3, "_noteOperateType": 21, "_scorePercent": 15}, {"_id": 4, "_noteOperateType": 40, "_scorePercent": 110},
            {"_id": 5, "_noteOperateType": 120, "_scorePercent": 5},
        ]),
    );
    add(
        "MasterLiveJudgementParameter",
        json!([
            {"_id": 1, "_noteSimulateJudgement": 6, "_scorePercent": 190}, {"_id": 2, "_noteSimulateJudgement": 5, "_scorePercent": 100},
            {"_id": 3, "_noteSimulateJudgement": 4, "_scorePercent": 70}, {"_id": 4, "_noteSimulateJudgement": 3, "_scorePercent": 40},
            {"_id": 5, "_noteSimulateJudgement": 2, "_scorePercent": 0}, {"_id": 6, "_noteSimulateJudgement": 1, "_scorePercent": 0},
            {"_id": 7, "_noteSimulateJudgement": 8, "_scorePercent": 300},
        ]),
    );
    add(
        "MasterLiveComboScoreBonus",
        Value::Array(
            (1..=12)
                .map(|k| json!({"_id": k, "_comboBonusType": 0, "_requiredComboCount": 15 * k, "_bonusFactor": 0.02}))
                .collect(),
        ),
    );
    add(
        "MasterLiveSettings",
        json!([
            {"_id": 1, "_key": "note_score_adjustment_factor", "_value": "2.5"},
            {"_id": 2, "_key": "note_score_life_onus_factor", "_value": "0.25"},
            {"_id": 3, "_key": "assist_score_percent", "_value": "85"},
        ]),
    );
    Synth { tables: t }
}

/// A roster of every card of the synthetic master with random progress.
pub fn roster(rng: &mut Rng, master: &Master) -> Roster {
    let members = master
        .member_cards
        .iter()
        .map(|c| OwnedMember {
            id: c.id,
            level: Some(rng.range(1, 40)),
            exp: None,
            awake: rng.range(1, 5),
            rank: rng.range(1, 5),
            live_skill_level: rng.range(1, 5),
            gekisou_skill_level: 1,
        })
        .collect();
    let snaps = master
        .support_cards
        .iter()
        .map(|s| OwnedSnap { id: s.id, level: Some(rng.range(1, 30)), exp: None, rank: rng.range(1, 5) })
        .collect();
    let mut player = Player::default();
    for c in 1..=CHARACTERS {
        if rng.chance(0.8) {
            player.character_ranks.insert(c, rng.range(1, 30));
        }
    }
    for b in 1..=BANDS {
        if rng.chance(0.7) {
            player.band_items.insert(100 + b, rng.range(1, 10));
        }
    }
    if rng.chance(0.5) {
        player.band_items.insert(200, 1);
    }
    player.vip_rank = rng.range(1, 8);
    Roster { player, members, snaps }
}

/// A synthetic chart.
pub fn chart(rng: &mut Rng, notes: usize) -> Chart {
    let types = [1, 20, 21, 40, 120];
    let mut t = 1000;
    let notes: Vec<ChartNote> = (0..notes)
        .map(|i| {
            t += rng.range(0, 400) as i32;
            ChartNote { id: i as i32 + 1, time_ms: t, note_type: types[rng.below(5) as usize] }
        })
        .collect();
    let last = notes.last().map_or(0, |n| n.time_ms);
    let skill_events = (0..5).map(|k| SkillEvent { index: k, time_ms: 2000 + k * (last / 6).max(1) }).collect();
    Chart { converted_note_count: notes.len().max(1) as i32, last_timing_note_ms: last, notes, skill_events }
}

/// A synthetic play of a chart: random judgements, the combo reset on Bad and Miss, lives, and the life at each
/// skill event.
pub fn play(rng: &mut Rng, chart: &Chart) -> ournotes_deck::live::model::Play {
    use ournotes_deck::live::skill::NotePlay;
    let mut combo = 0;
    let mut life: i32 = 1000;
    let notes = chart
        .notes
        .iter()
        .map(|n| {
            let st = [1, 1, 1, 2, 2, 2, 3, 4, 5, 6][rng.below(10) as usize];
            combo = if st >= 5 { 0 } else { combo + 1 };
            if st >= 5 {
                life = (life - rng.range(20, 120) as i32).max(0);
            }
            NotePlay { note_id: n.id, time_ms: n.time_ms, note_type: n.note_type, score_type: st, life, combo }
        })
        .collect();
    let life_at_event = (0..5).map(|_| rng.range(300, 1000) as i32).collect();
    ournotes_deck::live::model::Play { notes, life_at_event, assist: rng.chance(0.2) }
}
