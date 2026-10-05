//! The account input on a small made-up master: what it reads, what each goal needs, and how problems are reported.

use std::collections::{BTreeMap, BTreeSet};

use ournotes_sim::account::{AccountInput, Exclusions, Goal, Resolution, ResolvedAccount};
use ournotes_sim::cards::{OwnedMember, OwnedSnap, Player, Roster};
use ournotes_sim::data::DeckData;
use ournotes_sim::master::Master;
use ournotes_sim::memory::{MemoryState, current_music_bonus};
use ournotes_sim::pool::Pool;
use serde_json::{Value, json};

const DATASET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn member_card(id: i64, character: i64, rarity: i64, live: i64, gekisou: i64) -> Value {
    json!({"_id": id, "_characterID": character, "_rarity": rarity, "_cardType": 1, "_bestMusicTagIDs": [],
           "_performancePowerMax": 3000, "_technicPowerMax": 2800, "_visualPowerMax": 2600,
           "_memberCardLevelGroup": 1, "_memberCardAwakeGroup": 1, "_memberCardRankGroup": 1,
           "_liveSkillID": live, "_gekisouSkillID": gekisou})
}

fn tables() -> BTreeMap<&'static str, Value> {
    let mut t = BTreeMap::new();
    t.insert(
        "MasterParameter",
        json!([
            {"_id": "music_type_base_bonus_rate", "_value": "400"},
            {"_id": "music_tag_base_bonus_rate", "_value": "300"},
            {"_id": "type_link_base_bonus_rate", "_value": "600"},
        ]),
    );
    t.insert("MasterCharacter", json!([{"_id": 1, "_bandID": 1}, {"_id": 2, "_bandID": 1}, {"_id": 3, "_bandID": 2}]));
    // Listed out of rank order; ranks 1..5 need 0, 2, 5, 9, 14 experience.
    t.insert(
        "MasterCharacterRank",
        json!([
            {"_id": 3, "_rank": 3, "_exp": 5, "_bonus": 20}, {"_id": 1, "_rank": 1, "_exp": 0, "_bonus": 0},
            {"_id": 5, "_rank": 5, "_exp": 14, "_bonus": 40}, {"_id": 2, "_rank": 2, "_exp": 2, "_bonus": 10},
            {"_id": 4, "_rank": 4, "_exp": 9, "_bonus": 30},
        ]),
    );
    t.insert(
        "MasterCharacterTotalRank",
        json!([{"_id": 1, "_totalRank": 3, "_bonus": 5}, {"_id": 2, "_totalRank": 9, "_bonus": 12}]),
    );
    t.insert(
        "MasterMemberCard",
        json!([
            member_card(101, 1, 4, 11, 21),
            member_card(102, 2, 3, 0, 0),
            member_card(103, 3, 4, 12, 22),
            member_card(104, 1, 3, 0, 0),
        ]),
    );
    // Member cards and snaps both use level group 1, on different curves: 10 and 7 experience per level.
    t.insert(
        "MasterMemberCardLevel",
        Value::Array(
            (1..=10)
                .rev()
                .map(|l| {
                    let r = 4000 + 600 * (l - 1);
                    json!({"_id": l, "_group": 1, "_level": l, "_exp": (l - 1) * 10, "_performanceRate": r,
                           "_technicRate": r, "_visualRate": r})
                })
                .collect(),
        ),
    );
    t.insert(
        "MasterSupportCardLevel",
        Value::Array(
            (1..=10)
                .map(|l| {
                    let r = 3900 + 500 * (l - 1);
                    json!({"_id": l, "_group": 1, "_level": l, "_exp": (l - 1) * 7, "_performanceRate": r,
                           "_technicRate": r, "_visualRate": r})
                })
                .collect(),
        ),
    );
    let mut limits = Vec::new();
    for (a, cap) in [5, 7, 8, 9, 10].into_iter().enumerate() {
        let a = a as i64 + 1;
        limits.push(json!({"_id": a, "_rarity": 4, "_awakeCount": a, "_limitLevel": cap}));
        limits.push(json!({"_id": 10 + a, "_rarity": 3, "_awakeCount": a, "_limitLevel": 10}));
    }
    t.insert("MasterMemberCardLevelLimit", Value::Array(limits));
    t.insert(
        "MasterMemberCardAwake",
        Value::Array(
            (1..=5)
                .map(|a| {
                    json!({"_id": a, "_group": 1, "_awakeCount": a, "_performanceRate": (a - 1) * 200,
                           "_technicRate": (a - 1) * 210, "_visualRate": (a - 1) * 190})
                })
                .collect(),
        ),
    );
    t.insert(
        "MasterMemberCardRank",
        Value::Array(
            (1..=5)
                .map(|r| {
                    json!({"_id": r, "_group": 1, "_rank": r, "_performanceRate": (r - 1) * 150,
                           "_technicRate": (r - 1) * 160, "_visualRate": (r - 1) * 170, "_leaderSkillLevel": r})
                })
                .collect(),
        ),
    );
    t.insert(
        "MasterSupportCard",
        json!([
            {"_id": 201, "_characterIDs": [1], "_rarity": 3, "_cardType": 1, "_performancePowerMax": 900,
             "_technicPowerMax": 800, "_visualPowerMax": 700, "_supportCardLevelGroup": 1, "_supportCardRankGroup": 1},
            {"_id": 202, "_characterIDs": [3], "_rarity": 2, "_cardType": 1, "_performancePowerMax": 600,
             "_technicPowerMax": 500, "_visualPowerMax": 400, "_supportCardLevelGroup": 1, "_supportCardRankGroup": 1},
        ]),
    );
    t.insert(
        "MasterSupportCardRank",
        Value::Array(
            [4, 6, 8, 9, 10]
                .into_iter()
                .enumerate()
                .map(|(r, cap)| {
                    let r = r as i64 + 1;
                    json!({"_id": r, "_group": 1, "_rank": r, "_limitLevel": cap, "_cardTypeLinkBonusRate": r * 100})
                })
                .collect(),
        ),
    );
    t.insert("MasterLiveSkill", json!([{"_id": 11, "_skillCategories": [1]}, {"_id": 12, "_skillCategories": [1]}]));
    // Skill 11 has levels 1..4; skill 12 only levels 1 and 3.
    let live: Vec<Value> = [(11, 1), (11, 2), (11, 3), (11, 4), (12, 1), (12, 3)]
        .into_iter()
        .enumerate()
        .map(|(i, (s, l))| json!({"_id": i + 1, "_liveSkillID": s, "_level": l, "_skillEffectType": 1, "_effectValue": 100 * l}))
        .collect();
    t.insert("MasterLiveSkillEffect", Value::Array(live));
    t.insert("MasterGekisouSkill", json!([{"_id": 21}, {"_id": 22}]));
    // Skill 21 has levels 1..5; skill 22 levels 1..4.
    let gekisou: Vec<Value> = (1..=5)
        .map(|l| (21, l))
        .chain((1..=4).map(|l| (22, l)))
        .enumerate()
        .map(|(i, (s, l))| json!({"_id": i + 1, "_gekisouSkillID": s, "_level": l, "_skillEffectType": 1, "_effectValue": 50 * l}))
        .collect();
    t.insert("MasterGekisouSkillEffect", Value::Array(gekisou));
    t.insert("MasterVip", Value::Array((1..=5).map(|r| json!({"_id": r, "_vipRank": r})).collect()));
    t.insert("MasterSkillTarget", json!([{"_id": 1, "_skillTargetType": 1, "_bandID": 1}]));
    t.insert("MasterBandItem", json!([{"_id": 301, "_bandId": 1}, {"_id": 302, "_bandId": 2}]));
    let mut levels = Vec::new();
    let mut effects = Vec::new();
    for (i, (item, level)) in [301, 302].into_iter().flat_map(|i| (1..=3).map(move |l| (i, l))).enumerate() {
        levels.push(json!({"_id": i + 1, "_bandItemId": item, "_level": level}));
        effects.push(json!({"_id": i + 1, "_bandItemId": item, "_level": level, "_skillTargetIDs": [1],
                            "_skillEffectType": 1000, "_effectValue": 10 * level}));
    }
    t.insert("MasterBandItemLevel", Value::Array(levels));
    t.insert("MasterBandItemSkillEffect", Value::Array(effects));
    t.insert(
        "MasterMemoryMusicGroup",
        json!([{"_id": 401, "_skillTargetIds": [1]}, {"_id": 402, "_skillTargetIds": [1]}]),
    );
    t.insert(
        "MasterMemoryMusic",
        json!([{"_id": 501, "_groupId": 401}, {"_id": 502, "_groupId": 401}, {"_id": 503, "_groupId": 402}]),
    );
    t.insert(
        "MasterMemoryMusicBonus",
        json!([
            {"_id": 1, "_groupId": 401, "_scoreRank": 3, "_performance": 10},
            {"_id": 2, "_groupId": 401, "_scoreRank": 5, "_performance": 30},
            {"_id": 3, "_groupId": 402, "_scoreRank": 1, "_performance": 7},
        ]),
    );
    t.insert(
        "MasterMemoryMemberLevel",
        json!([{"_id": 1, "_point": 1, "_technic": 4}, {"_id": 2, "_point": 2, "_technic": 9}]),
    );
    t.insert("MasterMemorySupportLevel", json!([{"_id": 1, "_point": 1, "_visual": 5}]));
    t
}

fn master_of(tables: &BTreeMap<&'static str, Value>) -> Master {
    let texts: BTreeMap<&str, String> =
        tables.iter().map(|(name, rows)| (*name, json!({ "_allData": rows }).to_string())).collect();
    Master::from_json_tables(|name| texts.get(name).map(String::as_str)).unwrap()
}

fn master() -> Master {
    master_of(&tables())
}

/// Deck data of `region` named `DATASET`.
fn data_of(master: Master, region: &str) -> DeckData {
    DeckData { provenance: json!({"region": region}), sha256: Some(DATASET.into()), master, charts: Vec::new() }
}

fn data() -> DeckData {
    data_of(master(), "jp")
}

fn complete() -> Value {
    json!({
        "_player._memberCards": "complete", "_player._supportCards": "complete", "_player._characters": "complete",
        "_player._bandItems": "complete", "_player._memory._musicGroups": "complete",
        "_player._memory._members": "complete", "_player._memory._supports": "complete",
    })
}

/// A whole account, every list complete.
fn base() -> Value {
    json!({
        "format": "ournotes.account/1",
        "datasetId": DATASET,
        "server": "jp",
        "revision": "draft-1",
        "coverage": complete(),
        "assumptions": [],
        "declared": {"_vip": {"_rank": 3}},
        "account": {
            "_player": {
                "_memberCards": [
                    {"_masterId": 101, "_exp": 20, "_awakeCount": 2, "_rank": 2, "_liveSkillLevel": 3, "_performanceSkillLevel": 4},
                    {"_masterId": 102, "_exp": 0, "_awakeCount": 1, "_rank": 1, "_liveSkillLevel": 1, "_performanceSkillLevel": 1},
                    {"_masterId": 103, "_exp": 35, "_awakeCount": 5, "_rank": 5, "_liveSkillLevel": 3, "_performanceSkillLevel": 3},
                ],
                "_supportCards": [
                    {"_masterId": 201, "_exp": 21, "_rank": 3, "_duplicateCount": 2},
                    {"_masterId": 202, "_exp": 0, "_rank": 1, "_duplicateCount": 0},
                ],
                "_characters": [{"_masterId": 1, "_exp": 5}, {"_masterId": 2, "_exp": 14}],
                "_bandItems": [{"_masterId": 301, "_level": 2}, {"_masterId": 302, "_level": 0}],
                "_memory": {
                    "_musicGroups": [{"_id": 401, "_musics": [
                        {"_id": 501, "_unlockedScoreRank": 6}, {"_id": 502, "_unlockedScoreRank": 4},
                    ]}],
                    "_members": [{"_id": 101, "_unlocked": true}, {"_id": 102, "_unlocked": false},
                                 {"_id": 103, "_unlocked": false}, {"_id": 104, "_unlocked": true}],
                    "_supports": [{"_id": 201, "_unlocked": true}, {"_id": 202, "_unlocked": false}],
                },
            }
        }
    })
}

fn resolve_with(doc: &Value, goal: Goal, exclusions: &Exclusions) -> Resolution {
    AccountInput::from_json(&doc.to_string()).unwrap().resolve(&data(), goal, exclusions)
}

fn resolve(doc: &Value, goal: Goal) -> Resolution {
    resolve_with(doc, goal, &Exclusions::default())
}

fn ok(resolution: Resolution) -> ResolvedAccount {
    assert!(
        resolution.errors.is_empty() && resolution.missing.is_empty(),
        "errors {:?}, missing {:?}",
        resolution.errors,
        resolution.missing
    );
    resolution.resolved.unwrap()
}

/// `(path, code)` of every error.
fn errors(resolution: &Resolution) -> Vec<(String, String)> {
    assert!(resolution.resolved.is_none());
    resolution.errors.iter().map(|e| (e.path.clone(), e.code.clone())).collect()
}

fn error(path: &str, code: &str) -> Vec<(String, String)> {
    vec![(path.to_string(), code.to_string())]
}

fn missing(resolution: &Resolution) -> Vec<String> {
    assert!(resolution.resolved.is_none() && resolution.errors.is_empty(), "{:?}", resolution.errors);
    resolution.missing.iter().map(|m| m.path.clone()).collect()
}

fn member(doc: &mut Value, i: usize) -> &mut Value {
    &mut doc["account"]["_player"]["_memberCards"][i]
}

fn level_of(resolved: &ResolvedAccount, id: i64) -> Option<i64> {
    let roster = resolved.roster();
    roster
        .members
        .iter()
        .find(|m| m.id == id)
        .and_then(|m| m.level)
        .or_else(|| roster.snaps.iter().find(|s| s.id == id).and_then(|s| s.level))
}

#[test]
fn a_whole_save_resolves_and_only_read_fields_are_kept() {
    let mut doc = base();
    let player = doc["account"]["_player"].as_object_mut().unwrap();
    player.insert("_name".into(), json!("Hidden Player Name"));
    player.insert("_profileId".into(), json!("98765432109876543"));
    player.insert("_friendProfile".into(), json!({"_name": "Hidden Friend", "_accountid": 1234567890123456789u64}));
    player.insert("_items".into(), json!([{"_masterItemId": 1, "_amount": 5}]));
    player.insert(
        "_characterFriendships".into(),
        json!([{"_pair": {"_masterCharacterIdA": 1, "_masterCharacterIdB": 2}, "_exp": 3}]),
    );
    player.insert("_events".into(), json!([{"_masterId": 7, "_eventPointCount": 100, "_rankingCount": 2}]));
    player.insert("_topHighScoreRatings".into(), json!([{"_mstLiveMusicId": 1, "_highScoreRating": 3}]));
    player.insert("_optionDataBytes".into(), json!([1, 2, 3]));
    player.insert("_exp".into(), json!(12345));
    // The save's own VIP field is not read: the rank comes from `declared`.
    player.insert("_vip".into(), json!({"_rank": 99}));
    doc["account"]["_settings"] = json!({"_volume": 0.5});
    // A 19-digit identity and one beyond 64 bits, as written by the game, in skipped fields.
    let text = doc.to_string().replacen(
        "\"_exp\":12345",
        "\"_accountid\":1234567890123456789,\"_world_room_id\":123456789012345678901234567890",
        1,
    );
    let input = AccountInput::from_json(&text).unwrap();
    let resolution = input.resolve(&data(), Goal::Power, &Exclusions::default());
    let resolved = ok(resolution.clone());
    for shown in [
        format!("{input:?}"),
        format!("{resolution:?}"),
        resolved.scope().to_string(),
        format!("{:?}", resolved.roster()),
    ] {
        for secret in ["Hidden", "1234567890123456789", "98765432109876543", "123456789012345678901234567890"] {
            assert!(!shown.contains(secret), "{secret} kept");
        }
    }
    assert_eq!(resolved.roster().members.len(), 3);
    assert_eq!(resolved.roster().snaps.len(), 2);
}

#[test]
fn long_and_int_fields_are_read_exactly() {
    let mut doc = base();
    member(&mut doc, 0)["_masterId"] = json!("101");
    doc["account"]["_player"]["_characters"][0]["_masterId"] = json!("1");
    assert_eq!(level_of(&ok(resolve(&doc, Goal::Power)), 101), Some(3));

    for (value, ok_value) in [
        ("101.0", false),
        ("1e2", false),
        ("\"0101\"", false),
        ("\"+101\"", false),
        ("9223372036854775808", false),
        ("\"101\"", true),
    ] {
        let text = base().to_string().replacen("\"_masterId\":101", &format!("\"_masterId\":{value}"), 1);
        let resolution = AccountInput::from_json(&text).unwrap().resolve(&data(), Goal::Power, &Exclusions::default());
        if ok_value {
            ok(resolution);
        } else {
            assert_eq!(errors(&resolution), error("_player._memberCards[0]._masterId", "invalid_type"), "{value}");
        }
    }
    for value in ["\"20\"", "20.0", "2147483648", "true"] {
        let text = base().to_string().replacen("\"_exp\":20", &format!("\"_exp\":{value}"), 1);
        let resolution = AccountInput::from_json(&text).unwrap().resolve(&data(), Goal::Power, &Exclusions::default());
        assert_eq!(errors(&resolution), error("_player._memberCards[0]._exp", "invalid_type"), "{value}");
    }
    let mut doc = base();
    doc["account"]["_player"]["_memory"]["_members"][0]["_unlocked"] = json!(1);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._memory._members[0]._unlocked", "invalid_type"));
}

#[test]
fn read_objects_reject_repeated_keys_and_skipped_objects_do_not() {
    let text = base().to_string();
    for repeated in [
        text.replacen("\"_exp\":20", "\"_exp\":20,\"_exp\":20", 1),
        text.replacen("\"revision\":", "\"revision\":\"x\",\"revision\":", 1),
        text.replacen("\"_player\":{", "\"_player\":{\"_characters\":[],", 1),
    ] {
        let error = AccountInput::from_json(&repeated).unwrap_err().to_string();
        assert!(error.contains("duplicate field"), "{error}");
    }
    let skipped = text.replacen(
        "\"_player\":{",
        "\"_player\":{\"_name\":\"a\",\"_name\":\"b\",\"_friendProfile\":{\"_k\":1,\"_k\":2},",
        1,
    );
    ok(AccountInput::from_json(&skipped).unwrap().resolve(&data(), Goal::Power, &Exclusions::default()));
}

#[test]
fn null_and_absent_values_are_unknown_and_lists_are_empty() {
    let mut doc = base();
    member(&mut doc, 0)["_awakeCount"] = Value::Null;
    assert_eq!(missing(&resolve(&doc, Goal::Power)), ["_player._memberCards[0]._awakeCount"]);
    member(&mut doc, 0).as_object_mut().unwrap().remove("_awakeCount");
    assert_eq!(missing(&resolve(&doc, Goal::Power)), ["_player._memberCards[0]._awakeCount"]);
    // A null list is an empty one: with complete coverage the player owns no snap.
    let mut doc = base();
    doc["account"]["_player"]["_supportCards"] = Value::Null;
    doc["account"]["_player"]["_memory"]["_supports"] = Value::Null;
    let resolved = ok(resolve(&doc, Goal::Power));
    assert!(resolved.roster().snaps.is_empty());
    assert_eq!(resolved.roster().player.owned_support_card_ids, Some(BTreeSet::new()));
    // An identity cannot be unknown.
    let mut doc = base();
    member(&mut doc, 1)["_masterId"] = Value::Null;
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._memberCards[1]._masterId", "invalid_value"));
}

#[test]
fn item_messages_name_the_item() {
    let message = |resolution: Resolution| {
        let issue = resolution.errors.first().or(resolution.missing.first()).unwrap();
        issue.message.clone()
    };
    let mut doc = base();
    member(&mut doc, 2)["_awakeCount"] = Value::Null;
    assert!(message(resolve(&doc, Goal::Power)).starts_with("member card _masterId 103: "));
    let mut doc = base();
    doc["account"]["_player"]["_supportCards"][1]["_rank"] = json!(9);
    assert!(message(resolve(&doc, Goal::Power)).starts_with("snap _masterId 202: "));
    let mut doc = base();
    doc["account"]["_player"]["_characters"][1]["_exp"] = json!(-1);
    assert!(message(resolve(&doc, Goal::Power)).starts_with("character _masterId 2: "));
    let mut doc = base();
    doc["account"]["_player"]["_bandItems"][0]["_level"] = json!(4);
    assert!(message(resolve(&doc, Goal::Power)).starts_with("band item _masterId 301: "));
    let mut doc = base();
    doc["account"]["_player"]["_memory"]["_musicGroups"][0]["_musics"][1]["_unlockedScoreRank"] = json!(8);
    assert!(message(resolve(&doc, Goal::Power)).starts_with("memory music _id 502: "));
    let mut doc = base();
    doc["account"]["_player"]["_memory"]["_supports"][0]["_unlocked"] = json!(1);
    assert!(message(resolve(&doc, Goal::Power)).starts_with("memory snap _id 201: "));
}

#[test]
fn the_envelope_is_checked() {
    let parse = |doc: &Value| AccountInput::from_json(&doc.to_string());
    let mut doc = base();
    doc["coverage"].as_object_mut().unwrap().remove("_player._bandItems");
    assert!(parse(&doc).is_err());
    let mut doc = base();
    doc["coverage"]["_player._items"] = json!("complete");
    assert!(parse(&doc).is_err());
    let mut doc = base();
    doc["coverage"]["_player._characters"] = json!("full");
    assert!(parse(&doc).is_err());
    let mut doc = base();
    doc["owner"] = json!("someone");
    assert!(parse(&doc).is_err());
    let mut doc = base();
    doc["account"] = json!({});
    assert!(parse(&doc).is_err());
    let mut doc = base();
    doc["account"]["_player"] = Value::Null;
    assert!(parse(&doc).is_err());

    let mut doc = base();
    doc["format"] = json!("ournotes.account/2");
    doc["datasetId"] = json!("another");
    doc["revision"] = json!("");
    doc["assumptions"] = json!([{"path": "declared._vip._rank", "reason": "entered by the user"}, {"path": "", "reason": "x"}, {"path": "_player._characters", "reason": " "}]);
    assert_eq!(
        errors(&resolve(&doc, Goal::Power)),
        [
            ("format", "unsupported_format"),
            ("datasetId", "dataset_mismatch"),
            ("revision", "missing_identity"),
            ("assumptions[1]", "invalid_assumption"),
            ("assumptions[2]", "invalid_assumption"),
        ]
        .map(|(p, c)| (p.to_string(), c.to_string()))
    );
}

#[test]
fn experience_maps_to_level_through_each_card_kinds_table() {
    // Member cards: 10 experience per level; snaps: 7, in the same level group.
    for (exp, level) in [(0, 1), (9, 1), (10, 2), (19, 2), (20, 3), (60, 7)] {
        let mut doc = base();
        member(&mut doc, 0)["_exp"] = json!(exp);
        assert_eq!(level_of(&ok(resolve(&doc, Goal::Power)), 101), Some(level), "member exp {exp}");
    }
    for (exp, level) in [(0, 1), (6, 1), (7, 2), (20, 3), (21, 4), (55, 8)] {
        let mut doc = base();
        doc["account"]["_player"]["_supportCards"][0]["_exp"] = json!(exp);
        doc["account"]["_player"]["_supportCards"][0]["_rank"] = json!(3);
        assert_eq!(level_of(&ok(resolve(&doc, Goal::Power)), 201), Some(level), "snap exp {exp}");
    }
    let mut doc = base();
    member(&mut doc, 0)["_exp"] = json!(-1);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._memberCards[0]._exp", "invalid_value"));
}

#[test]
fn experience_above_the_level_cap_is_an_error() {
    // Rarity 4 at awake count 1 caps at level 5 (experience 40); at 2, level 7 (60).
    let mut doc = base();
    member(&mut doc, 0)["_awakeCount"] = json!(1);
    member(&mut doc, 0)["_exp"] = json!(40);
    ok(resolve(&doc, Goal::Power));
    member(&mut doc, 0)["_exp"] = json!(50);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._memberCards[0]._exp", "level_cap"));
    member(&mut doc, 0)["_awakeCount"] = json!(2);
    member(&mut doc, 0)["_exp"] = json!(70);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._memberCards[0]._exp", "level_cap"));
    // Snap rank 1 caps at level 4 (experience 21).
    let mut doc = base();
    doc["account"]["_player"]["_supportCards"][0]["_rank"] = json!(1);
    ok(resolve(&doc, Goal::Power));
    doc["account"]["_player"]["_supportCards"][0]["_exp"] = json!(28);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._supportCards[0]._exp", "level_cap"));
}

#[test]
fn counts_start_at_one_and_stay_in_their_range() {
    let cases: [(usize, &str, Value, &str); 8] = [
        (0, "_awakeCount", json!(0), "_player._memberCards[0]._awakeCount"),
        (0, "_awakeCount", json!(6), "_player._memberCards[0]._awakeCount"),
        (0, "_rank", json!(0), "_player._memberCards[0]._rank"),
        (0, "_rank", json!(6), "_player._memberCards[0]._rank"),
        (0, "_liveSkillLevel", json!(0), "_player._memberCards[0]._liveSkillLevel"),
        (0, "_liveSkillLevel", json!(5), "_player._memberCards[0]._liveSkillLevel"),
        (0, "_performanceSkillLevel", json!(6), "_player._memberCards[0]._performanceSkillLevel"),
        (1, "_liveSkillLevel", json!(0), "_player._memberCards[1]._liveSkillLevel"),
    ];
    // Checked whatever the goal reads: a stated level is a fact of the save.
    for (card, field, value, path) in cases {
        let mut doc = base();
        member(&mut doc, card)[field] = value.clone();
        assert_eq!(errors(&resolve(&doc, Goal::Power)), error(path, "invalid_value"), "{field} = {value}");
    }
    // A card without a live skill may state any level from 1.
    let mut doc = base();
    member(&mut doc, 1)["_liveSkillLevel"] = json!(7);
    ok(resolve(&doc, Goal::NormalLive));
    let mut doc = base();
    doc["account"]["_player"]["_supportCards"][0]["_rank"] = json!(0);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._supportCards[0]._rank", "invalid_value"));
}

#[test]
fn characters_reach_ranks_by_experience_and_all_count_in_the_total() {
    for (exp, rank) in [(0, 1), (1, 1), (2, 2), (4, 2), (5, 3), (13, 4), (14, 5), (500, 5)] {
        let mut doc = base();
        doc["account"]["_player"]["_characters"][0]["_exp"] = json!(exp);
        assert_eq!(ok(resolve(&doc, Goal::Power)).roster().player.character_ranks[&1], rank, "exp {exp}");
    }
    // Character 3 is not listed: under complete coverage it is at experience 0, rank 1, and counts in the total.
    let resolved = ok(resolve(&base(), Goal::Power));
    let player = &resolved.roster().player;
    assert_eq!(player.character_ranks, BTreeMap::from([(1, 3), (2, 5), (3, 1)]));
    assert_eq!(player.character_total_rank(), 9);
    let m = master();
    let pool = Pool::new(&m, resolved.roster()).unwrap();
    let card = &pool.members[pool.member_index(103).unwrap()];
    assert_eq!((card.character_rank, card.character_total_rank), (1, 9));

    let mut doc = base();
    doc["coverage"]["_player._characters"] = json!("partial");
    let resolution = resolve(&doc, Goal::Power);
    assert_eq!(missing(&resolution), ["_player._characters"]);
    assert!(resolution.missing[0].message.contains("characters 3 "), "{}", resolution.missing[0].message);
    doc["account"]["_player"]["_characters"].as_array_mut().unwrap().push(json!({"_masterId": 3, "_exp": 0}));
    ok(resolve(&doc, Goal::Power));

    let mut doc = base();
    doc["account"]["_player"]["_characters"][1]["_masterId"] = json!(1);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._characters[1]._masterId", "duplicate_id"));
    let mut doc = base();
    doc["account"]["_player"]["_characters"][1]["_exp"] = Value::Null;
    assert_eq!(missing(&resolve(&doc, Goal::Power)), ["_player._characters[1]._exp"]);
}

#[test]
fn the_declared_vip_rank_is_required_and_must_be_in_the_deck_data() {
    let mut doc = base();
    doc.as_object_mut().unwrap().remove("declared");
    // The save's own VIP field is not read.
    doc["account"]["_player"]["_vip"] = json!({"_rank": 3});
    assert_eq!(missing(&resolve(&doc, Goal::Power)), ["declared._vip._rank"]);
    for declared in
        [Value::Null, json!({}), json!({"_vip": null}), json!({"_vip": {}}), json!({"_vip": {"_rank": null}})]
    {
        doc["declared"] = declared;
        assert_eq!(missing(&resolve(&doc, Goal::Power)), ["declared._vip._rank"]);
    }
    for rank in [0, 6] {
        doc["declared"] = json!({"_vip": {"_rank": rank}});
        assert_eq!(errors(&resolve(&doc, Goal::Power)), error("declared._vip._rank", "invalid_value"));
    }
    doc["declared"] = json!({"_vip": {"_rank": "3"}});
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("declared._vip._rank", "invalid_type"));
    doc["declared"] = json!({"_vip": {"_rank": 1}});
    assert_eq!(ok(resolve(&doc, Goal::Power)).roster().player.vip_rank, 1);
    for unknown in [json!({"_vip": {"_rank": 1}, "_level": 2}), json!({"_vip": {"_rank": 1, "_point": 5}})] {
        doc["declared"] = unknown;
        assert!(AccountInput::from_json(&doc.to_string()).is_err());
    }
}

#[test]
fn the_server_must_match_the_deck_data_region() {
    let parse = |doc: &Value| AccountInput::from_json(&doc.to_string()).unwrap();
    let mut doc = base();
    let ex = Exclusions::default();
    assert_eq!(ok(parse(&doc).resolve(&data_of(master(), "jp"), Goal::Power, &ex)).server().data_region(), "jp");
    doc["server"] = json!("intl");
    // International saves are read with deck data exported for region tw.
    assert_eq!(ok(parse(&doc).resolve(&data_of(master(), "tw"), Goal::Power, &ex)).scope()["server"], "intl");
    for (server, region) in [("intl", "jp"), ("jp", "tw"), ("jp", "embedded"), ("intl", "en"), ("intl", "kr")] {
        doc["server"] = json!(server);
        assert_eq!(
            errors(&parse(&doc).resolve(&data_of(master(), region), Goal::Power, &ex)),
            error("server", "server_mismatch"),
            "{server} with {region}"
        );
    }
    let mut no_region = data();
    no_region.provenance = json!({});
    doc["server"] = json!("jp");
    assert_eq!(errors(&parse(&doc).resolve(&no_region, Goal::Power, &ex)), error("server", "server_mismatch"));
    // Deck data built in memory has no identity to name.
    let mut unnamed = data();
    unnamed.sha256 = None;
    assert_eq!(errors(&parse(&doc).resolve(&unnamed, Goal::Power, &ex)), error("datasetId", "dataset_mismatch"));
    // The server is part of the envelope: required, and one of jp and intl.
    for server in [json!("tw"), json!("JP"), Value::Null] {
        doc["server"] = server;
        assert!(AccountInput::from_json(&doc.to_string()).is_err());
    }
    doc.as_object_mut().unwrap().remove("server");
    assert!(AccountInput::from_json(&doc.to_string()).is_err());
}

#[test]
fn band_items_at_level_zero_are_not_built() {
    let resolved = ok(resolve(&base(), Goal::Power));
    assert_eq!(resolved.roster().player.band_items, BTreeMap::from([(301, 2)]));
    let items = |doc: &Value| ok(resolve(doc, Goal::Power)).roster().player.band_items.clone();
    // Complete coverage: an unlisted item is at level 0.
    let mut doc = base();
    doc["account"]["_player"]["_bandItems"].as_array_mut().unwrap().remove(1);
    assert_eq!(items(&doc), BTreeMap::from([(301, 2)]));
    doc["coverage"]["_player._bandItems"] = json!("partial");
    let resolution = resolve(&doc, Goal::Power);
    assert_eq!(missing(&resolution), ["_player._bandItems"]);
    assert!(resolution.missing[0].message.contains("302"));
    for (level, code) in [(json!(4), "invalid_value"), (json!(-1), "invalid_value"), (json!(1.5), "invalid_type")] {
        let mut doc = base();
        doc["account"]["_player"]["_bandItems"][0]["_level"] = level;
        assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._bandItems[0]._level", code));
    }
    let mut doc = base();
    doc["account"]["_player"]["_bandItems"][0]["_level"] = Value::Null;
    assert_eq!(missing(&resolve(&doc, Goal::Power)), ["_player._bandItems[0]._level"]);
}

#[test]
fn memory_counts_owned_unlocked_cards_and_the_musics_listed() {
    let m = master();
    let resolved = ok(resolve(&base(), Goal::Power));
    let memory = resolved.roster().player.memory.clone().unwrap();
    assert_eq!(memory.music_groups, Some(BTreeMap::from([(401, vec![(501, 6), (502, 4)])])));
    // Card 104 is unlocked but not owned (the card list is complete): it does not count.
    assert_eq!(memory.unlocked_members, BTreeSet::from([101, 104]));
    assert_eq!(resolved.scope()["playerBonusEvidence"]["memoryUnlockedMemberCards"], 1);
    assert_eq!(current_music_bonus(&m, 401, &memory).unwrap().id, 1);
    assert!(current_music_bonus(&m, 402, &memory).is_none());

    let groups = |doc: &Value| ok(resolve(doc, Goal::Power)).roster().player.memory.clone().unwrap();
    let mut doc = base();
    doc["account"]["_player"]["_memory"]["_musicGroups"][0]["_musics"][1]["_unlockedScoreRank"] = json!(5);
    doc["account"]["_player"]["_memory"]["_musicGroups"]
        .as_array_mut()
        .unwrap()
        .push(json!({"_id": 402, "_musics": [{"_id": 503, "_unlockedScoreRank": 1}]}));
    let memory = groups(&doc);
    assert_eq!(current_music_bonus(&m, 401, &memory).unwrap().id, 2);
    assert_eq!(current_music_bonus(&m, 402, &memory).unwrap().id, 3);
    // Only the musics listed in the group count.
    let mut doc = base();
    doc["account"]["_player"]["_memory"]["_musicGroups"][0]["_musics"].as_array_mut().unwrap().remove(1);
    assert_eq!(current_music_bonus(&m, 401, &groups(&doc)).unwrap().id, 2);

    let group = "_player._memory._musicGroups[0]._musics";
    for (edit, path, code) in [
        (json!([]), group.to_string(), "invalid_value"),
        (
            json!([{"_id": 501, "_unlockedScoreRank": 6}, {"_id": 503, "_unlockedScoreRank": 6}]),
            format!("{group}[1]._id"),
            "invalid_value",
        ),
        (json!([{"_id": 501, "_unlockedScoreRank": 8}]), format!("{group}[0]._unlockedScoreRank"), "invalid_value"),
        (json!([{"_id": 501, "_unlockedScoreRank": -1}]), format!("{group}[0]._unlockedScoreRank"), "invalid_value"),
        (
            json!([{"_id": 501, "_unlockedScoreRank": 1}, {"_id": 501, "_unlockedScoreRank": 2}]),
            format!("{group}[1]._id"),
            "duplicate_id",
        ),
        (json!([{"_id": 599, "_unlockedScoreRank": 1}]), format!("{group}[0]._id"), "unknown_id"),
    ] {
        let mut doc = base();
        doc["account"]["_player"]["_memory"]["_musicGroups"][0]["_musics"] = edit;
        assert_eq!(errors(&resolve(&doc, Goal::Power)), error(&path, code));
    }

    let mut doc = base();
    doc["coverage"]["_player._memory._musicGroups"] = json!("partial");
    let resolution = resolve(&doc, Goal::Power);
    assert_eq!(missing(&resolution), ["_player._memory._musicGroups"]);
    assert!(resolution.missing[0].message.contains("402"));

    // Partial memory list: owned cards it does not list are unknown.
    let mut doc = base();
    doc["coverage"]["_player._memory._members"] = json!("partial");
    doc["account"]["_player"]["_memory"]["_members"] = json!([{"_id": 101, "_unlocked": true}]);
    let resolution = resolve(&doc, Goal::Power);
    assert_eq!(missing(&resolution), ["_player._memory._members", "_player._memory._members"]);
    assert!(resolution.missing[0].message.contains("102") && resolution.missing[1].message.contains("103"));
    // Partial card list: an unlocked card it does not list may be owned.
    let mut doc = base();
    doc["coverage"]["_player._memberCards"] = json!("partial");
    let resolution = resolve(&doc, Goal::Power);
    assert_eq!(missing(&resolution), ["_player._memberCards"]);
    assert!(resolution.missing[0].message.contains("104"));
    doc["account"]["_player"]["_memory"]["_members"][3]["_unlocked"] = json!(false);
    ok(resolve(&doc, Goal::Power));
    doc["coverage"]["_player._memory._members"] = json!("partial");
    assert_eq!(missing(&resolve(&doc, Goal::Power)), ["_player._memory._members"]);
    // Without memory level rows the counts change nothing.
    let mut t = tables();
    t.insert("MasterMemoryMemberLevel", json!([]));
    let resolution = AccountInput::from_json(&doc.to_string()).unwrap().resolve(
        &data_of(master_of(&t), "jp"),
        Goal::Power,
        &Exclusions::default(),
    );
    ok(resolution);
}

#[test]
fn each_goal_reads_its_skill_levels() {
    let mut unknown = base();
    for i in 0..3 {
        let card = member(&mut unknown, i).as_object_mut().unwrap();
        card.remove("_liveSkillLevel");
        card.insert("_performanceSkillLevel".into(), Value::Null);
    }
    for goal in [Goal::Power, Goal::Skip] {
        let resolved = ok(resolve(&unknown, goal));
        assert!(resolved.roster().members.iter().all(|m| m.live_skill_level == 0 && m.gekisou_skill_level == 0));
    }
    // Card 102 has neither skill, so no goal reads its levels.
    assert_eq!(
        missing(&resolve(&unknown, Goal::NormalLive)),
        ["_player._memberCards[0]._liveSkillLevel", "_player._memberCards[2]._liveSkillLevel"]
    );
    assert_eq!(
        missing(&resolve(&unknown, Goal::GekisouLive)),
        [
            "_player._memberCards[0]._liveSkillLevel",
            "_player._memberCards[0]._performanceSkillLevel",
            "_player._memberCards[2]._liveSkillLevel",
            "_player._memberCards[2]._performanceSkillLevel",
        ]
    );
    let levels = |goal| {
        ok(resolve(&base(), goal))
            .roster()
            .members
            .iter()
            .map(|m| (m.id, m.live_skill_level, m.gekisou_skill_level))
            .collect::<Vec<_>>()
    };
    assert_eq!(levels(Goal::Power), [(101, 0, 0), (102, 0, 0), (103, 0, 0)]);
    assert_eq!(levels(Goal::NormalLive), [(101, 3, 0), (102, 0, 0), (103, 3, 0)]);
    assert_eq!(levels(Goal::GekisouLive), [(101, 3, 4), (102, 0, 0), (103, 3, 3)]);
    assert_eq!(ok(resolve(&base(), Goal::GekisouLive)).goal(), Goal::GekisouLive);

    // A level the goal reads needs its effect row: skill 12 has no level 2, skill 22 no level 5.
    let mut doc = base();
    member(&mut doc, 2)["_liveSkillLevel"] = json!(2);
    member(&mut doc, 2)["_performanceSkillLevel"] = json!(5);
    ok(resolve(&doc, Goal::Power));
    assert_eq!(
        errors(&resolve(&doc, Goal::NormalLive)),
        error("_player._memberCards[2]._liveSkillLevel", "master_row_missing")
    );
    member(&mut doc, 2)["_liveSkillLevel"] = json!(3);
    ok(resolve(&doc, Goal::NormalLive));
    assert_eq!(
        errors(&resolve(&doc, Goal::GekisouLive)),
        error("_player._memberCards[2]._performanceSkillLevel", "master_row_missing")
    );
}

#[test]
fn excluded_cards_need_only_their_identity() {
    let mut doc = base();
    let card = member(&mut doc, 2).as_object_mut().unwrap();
    for field in ["_exp", "_awakeCount", "_rank", "_liveSkillLevel", "_performanceSkillLevel"] {
        card.insert(field.into(), Value::Null);
    }
    doc["account"]["_player"]["_memory"]["_members"][2]["_unlocked"] = json!(true);
    doc["account"]["_player"]["_supportCards"][1]["_rank"] = Value::Null;
    assert_eq!(missing(&resolve(&doc, Goal::GekisouLive)).len(), 6);
    let exclusions = Exclusions { members: vec![103], snaps: vec![202] };
    let resolved = ok(resolve_with(&doc, Goal::GekisouLive, &exclusions));
    let roster = resolved.roster();
    assert_eq!(roster.members.iter().map(|m| m.id).collect::<Vec<_>>(), [101, 102]);
    assert_eq!(roster.snaps.iter().map(|s| s.id).collect::<Vec<_>>(), [201]);
    // Still owned: its unlocked memory counts.
    assert_eq!(roster.player.owned_member_card_ids, Some(BTreeSet::from([101, 102, 103])));
    assert_eq!(resolved.scope()["playerBonusEvidence"]["memoryUnlockedMemberCards"], 2);
    assert_eq!(resolved.scope()["cards"]["candidateMemberCards"], 2);

    let exclusions = Exclusions { members: vec![101, 104], snaps: vec![203] };
    assert_eq!(
        errors(&resolve_with(&base(), Goal::Power, &exclusions)),
        [("constraints.excludeMembers[1]", "not_owned"), ("constraints.excludeSnaps[0]", "not_owned")]
            .map(|(p, c)| (p.to_string(), c.to_string()))
    );
}

#[test]
fn identities_must_be_in_the_deck_data_and_listed_once() {
    let cases = [
        (vec!["_memberCards", "0", "_masterId"], "_player._memberCards[0]._masterId"),
        (vec!["_supportCards", "1", "_masterId"], "_player._supportCards[1]._masterId"),
        (vec!["_characters", "0", "_masterId"], "_player._characters[0]._masterId"),
        (vec!["_bandItems", "1", "_masterId"], "_player._bandItems[1]._masterId"),
        (vec!["_memory", "_musicGroups", "0", "_id"], "_player._memory._musicGroups[0]._id"),
        (vec!["_memory", "_members", "1", "_id"], "_player._memory._members[1]._id"),
        (vec!["_memory", "_supports", "0", "_id"], "_player._memory._supports[0]._id"),
    ];
    for (keys, path) in cases {
        let mut doc = base();
        let mut target = &mut doc["account"]["_player"];
        for key in keys {
            target = match key.parse::<usize>() {
                Ok(i) => &mut target[i],
                Err(_) => &mut target[key],
            };
        }
        *target = json!(999);
        assert_eq!(errors(&resolve(&doc, Goal::Power)), error(path, "unknown_id"));
    }
    let mut doc = base();
    member(&mut doc, 2)["_masterId"] = json!(101);
    assert_eq!(errors(&resolve(&doc, Goal::Power)), error("_player._memberCards[2]._masterId", "duplicate_id"));
}

#[test]
fn the_roster_equals_the_same_facts_built_directly() {
    let resolved = ok(resolve(&base(), Goal::Power));
    let expected = Roster {
        player: Player {
            explicit_character_total_rank: None,
            character_ranks: BTreeMap::from([(1, 3), (2, 5), (3, 1)]),
            band_items: BTreeMap::from([(301, 2)]),
            vip_rank: 3,
            events: Vec::new(),
            memory: Some(MemoryState {
                music_ranks: BTreeMap::new(),
                music_groups: Some(BTreeMap::from([(401, vec![(501, 6), (502, 4)])])),
                unlocked_members: BTreeSet::from([101, 104]),
                unlocked_supports: BTreeSet::from([201]),
            }),
            owned_member_card_ids: Some(BTreeSet::from([101, 102, 103])),
            owned_support_card_ids: Some(BTreeSet::from([201, 202])),
        },
        members: vec![
            OwnedMember {
                id: 101,
                level: Some(3),
                exp: Some(20),
                awake: 2,
                rank: 2,
                live_skill_level: 0,
                gekisou_skill_level: 0,
            },
            OwnedMember {
                id: 102,
                level: Some(1),
                exp: Some(0),
                awake: 1,
                rank: 1,
                live_skill_level: 0,
                gekisou_skill_level: 0,
            },
            OwnedMember {
                id: 103,
                level: Some(4),
                exp: Some(35),
                awake: 5,
                rank: 5,
                live_skill_level: 0,
                gekisou_skill_level: 0,
            },
        ],
        snaps: vec![
            OwnedSnap { id: 201, level: Some(4), exp: Some(21), rank: 3 },
            OwnedSnap { id: 202, level: Some(1), exp: Some(0), rank: 1 },
        ],
    };
    assert_eq!(format!("{:?}", resolved.roster()), format!("{expected:?}"));
    let m = master();
    let a = Pool::new(&m, resolved.roster()).unwrap();
    let b = Pool::new(&m, &expected).unwrap();
    assert_eq!(format!("{:?}", a.members), format!("{:?}", b.members));
    // The memory adds to every member: one owned unlocked member card (technic 4), one snap (visual 5), and group 401
    // (performance 10) to the band 1 cards.
    let power = |pool: &Pool<'_>, id| pool.members[pool.member_index(id).unwrap()].power;
    let mut no_memory = expected.clone();
    no_memory.player.memory = None;
    let c = Pool::new(&m, &no_memory).unwrap();
    assert_ne!(power(&a, 101), power(&c, 101));
}

#[test]
fn the_scope_reports_coverage_counts_and_derived_values() {
    let mut doc = base();
    doc["coverage"]["_player._supportCards"] = json!("partial");
    doc["assumptions"] = json!([{"path": "declared._vip._rank", "reason": "entered by the user"}]);
    let resolved = ok(resolve(&doc, Goal::NormalLive));
    assert!(!resolved.covers_all_owned_cards());
    assert_eq!(resolved.revision(), "draft-1");
    assert_eq!(resolved.dataset_id(), DATASET);
    let mut coverage = complete();
    coverage["_player._supportCards"] = json!("partial");
    assert_eq!(
        resolved.scope(),
        json!({
            "datasetId": DATASET,
            "server": "jp",
            "revision": "draft-1",
            "goal": "normalLive",
            "coverage": coverage,
            "assumptions": [{"path": "declared._vip._rank", "reason": "entered by the user"}],
            "cards": {"ownedMemberCards": 3, "ownedSupportCards": 2, "candidateMemberCards": 3,
                      "candidateSupportCards": 2, "coversAllOwnedCards": false},
            "playerBonusEvidence": {"vipRank": 3, "characterTotalRank": 9, "builtBandItems": 1, "memoryMusicGroups": 1,
                                    "memoryUnlockedMemberCards": 1, "memoryUnlockedSupportCards": 1},
        })
    );
}
