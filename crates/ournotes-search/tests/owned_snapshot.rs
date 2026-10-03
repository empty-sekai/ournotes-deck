#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;
#[cfg(feature = "native-fixtures")]
#[path = "../../ournotes-sim/tests/reference/mod.rs"]
mod reference;

use common::{Rng, extend_table, set_column, synth};
use ournotes_search::{
    owned_snapshot::{GoalDependencies as Goal, OwnedSnapshot},
    search::Objective,
};
use ournotes_sim::master::Master;
use serde_json::{Value, json};

fn fixture() -> (Master, Value) {
    let mut data = synth(&mut Rng::new(351), 6, 2);
    set_column(&mut data, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    extend_table(&mut data, "MasterMemberCardLevelLimit", (1..=5).flat_map(|rarity| (1..=5).map(move |awake| json!({"_id":rarity*10+awake,"_rarity":rarity,"_awakeCount":awake,"_limitLevel":10+awake*10}))).collect());
    let master = data.master();
    let ranks: Vec<_> = master.characters.iter().map(|row| json!({"id":row.id,"value":1})).collect();
    let snapshot = json!({
        "format":"ournotes.owned-snapshot/1","datasetId":"synthetic-351","revision":"rev-1",
        "ownedFacts":{"memberIds":[1,2,3,4,5,6],"snapIds":[1,2],"memberCoverage":"complete","snapCoverage":"complete"},
        "eligible":{
            "members":(1..=6).map(|id|json!({"id":id,"level":1,"awake":1,"rank":1})).collect::<Vec<_>>(),
            "snaps":[{"id":1,"level":1,"rank":1},{"id":2,"level":1,"rank":1}]
        },
        "player":{"characterRanks":{"coverage":"complete","values":ranks},"characterTotalRank":master.characters.len(),"vipRank":1,"bandItems":[],"memory":{"musicRanks":[],"unlockedMembers":[],"unlockedSnaps":[]},"eventIds":[]},
        "assumptions":[]
    });
    (master, snapshot)
}

fn parse(value: &Value) -> OwnedSnapshot {
    OwnedSnapshot::from_json(&value.to_string()).unwrap()
}
fn objective() -> Objective {
    Objective::Power { music_id: None, event: false }
}
fn power(snapshot: &OwnedSnapshot, master: &Master) -> i32 {
    let report = snapshot.resolve(master, "synthetic-351", Goal::Power);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    report.resolved.unwrap().evaluate_deck([1, 2, 3, 4, 5], [None; 5], &objective()).unwrap().0
}

#[test]
fn strict_nested_json_and_exact_integer_tokens() {
    let (_, value) = fixture();
    let raw = value.to_string();
    let duplicated = raw.replacen("\"revision\":\"rev-1\"", "\"revision\":\"rev-1\",\"revision\":\"rev-2\"", 1);
    assert!(OwnedSnapshot::from_json(&duplicated).is_err());
    let mut changed = value.clone();
    changed["eligible"]["members"][0]["typo"] = json!(1);
    assert!(OwnedSnapshot::from_json(&changed.to_string()).is_err());
    for invalid in [json!(1.5), json!("1"), json!({"value":1}), json!(u64::MAX)] {
        changed = value.clone();
        changed["eligible"]["members"][0]["id"] = invalid;
        assert!(OwnedSnapshot::from_json(&changed.to_string()).is_err());
    }
    changed = value;
    changed["ownedFacts"]["memberIds"][0] = json!(9_007_199_254_740_993i64);
    assert_eq!(parse(&changed).owned_facts.member_ids[0], 9_007_199_254_740_993);
}

#[test]
fn unknown_skills_remain_unknown_and_do_not_authorize_live() {
    let (master, value) = fixture();
    let snapshot = parse(&value);
    for goal in [Goal::Power, Goal::Skip] {
        let report = snapshot.resolve(&master, "synthetic-351", goal);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let resolved = report.resolved.unwrap();
        assert_eq!(resolved.snapshot().eligible.members[0].live_skill_level, None);
        // A Power capability cannot even be repurposed as Skip, or vice versa.
        if goal == Goal::Skip {
            assert!(resolved.evaluate_deck([1, 2, 3, 4, 5], [None; 5], &objective()).is_err());
        }
    }
    let normal = snapshot.resolve(&master, "synthetic-351", Goal::NormalLive);
    assert!(normal.resolved.is_none());
    assert!(normal.missing.iter().any(|i| i.path.ends_with(".liveSkillLevel")));
    assert!(!normal.missing.iter().any(|i| i.path.ends_with(".gekisouSkillLevel")));
    let gk = snapshot.resolve(&master, "synthetic-351", Goal::GekisouLive);
    assert!(gk.resolved.is_none());
    assert!(gk.missing.iter().any(|i| i.path.ends_with(".gekisouSkillLevel")));
}

#[test]
fn unknown_cultivation_and_global_facts_are_not_defaults_or_assumptions() {
    let (master, mut value) = fixture();
    value["eligible"]["members"][0]["level"] = Value::Null;
    value["eligible"]["members"][0]["awake"] = Value::Null;
    value["player"]["memory"] = Value::Null;
    value["player"]["vipRank"] = Value::Null;
    value["assumptions"] = json!([{"path":"player.vipRank","reason":"explicit annotation does not fill a value"}]);
    let snapshot = parse(&value);
    let report = snapshot.resolve(&master, "synthetic-351", Goal::Power);
    assert!(report.resolved.is_none());
    for path in ["eligible.members[1].levelOrExp", "eligible.members[1].awake", "player.memory", "player.vipRank"] {
        assert!(report.missing.iter().any(|i| i.path == path), "{path}: {:?}", report.missing);
    }
    assert!(snapshot.eligible.members[0].level.is_none());
}

#[test]
fn excluded_owned_member_still_changes_every_members_memory_bonus() {
    let (mut master, mut value) = fixture();
    master.memory_member_levels = vec![
        ournotes_sim::master::MemoryLevelRow {
            point: 1,
            performance: 100,
            technic: 100,
            visual: 100,
            ..Default::default()
        },
        ournotes_sim::master::MemoryLevelRow {
            point: 2,
            performance: 300,
            technic: 300,
            visual: 300,
            ..Default::default()
        },
    ];
    value["player"]["memory"]["unlockedMembers"] = json!([1, 6]);
    let before = power(&parse(&value), &master);
    value["eligible"]["members"].as_array_mut().unwrap().pop();
    let excluded = parse(&value);
    assert_eq!(before, power(&excluded, &master));
    assert!(excluded.owned_facts.member_ids.contains(&6));
    assert!(
        !excluded.resolve(&master, "synthetic-351", Goal::Power).resolved.unwrap().covers_all_declared_owned_cards()
    );
    value["ownedFacts"]["memberIds"] = json!([1, 2, 3, 4, 5]);
    value["player"]["memory"]["unlockedMembers"] = json!([1]);
    assert!(power(&parse(&value), &master) < before);
}

#[test]
fn independent_total_with_partial_local_ranks_does_not_invent_characters() {
    let (master, mut value) = fixture();
    let expected = power(&parse(&value), &master);
    value["player"]["characterRanks"]["coverage"] = json!("partial");
    value["player"]["characterRanks"]["values"].as_array_mut().unwrap().retain(|row| row["id"].as_i64().unwrap() <= 6);
    let snapshot = parse(&value);
    assert_eq!(power(&snapshot, &master), expected);
    let resolved = snapshot.resolve(&master, "synthetic-351", Goal::Power).resolved.unwrap();
    assert_eq!(resolved.snapshot().player.character_ranks.values.len(), 6);
    value["player"]["characterTotalRank"] = json!(6);
    let conflict = parse(&value).resolve(&master, "synthetic-351", Goal::Power);
    assert!(conflict.errors.iter().any(|i| i.code == "rank_conflict"));
    value["player"]["characterTotalRank"] = Value::Null;
    assert!(
        parse(&value)
            .resolve(&master, "synthetic-351", Goal::Power)
            .missing
            .iter()
            .any(|i| i.path == "player.characterTotalRank")
    );
    value["player"]["characterTotalRank"] = json!(i64::MAX);
    assert!(
        parse(&value).resolve(&master, "synthetic-351", Goal::Power).errors.iter().any(|i| i.code == "rank_conflict")
    );
}

#[test]
fn complete_rank_coverage_and_sum_conflicts_are_errors() {
    let (master, mut value) = fixture();
    value["player"]["characterTotalRank"] = json!(master.characters.len() + 1);
    assert!(
        parse(&value).resolve(&master, "synthetic-351", Goal::Power).errors.iter().any(|i| i.code == "rank_conflict")
    );
    value["player"]["characterRanks"]["values"].as_array_mut().unwrap().pop();
    assert!(
        parse(&value)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|i| i.code == "coverage_conflict")
    );
}

#[test]
fn complete_rank_facts_derive_total_without_rewriting_unknown_input() {
    use ournotes_search::owned_snapshot::TotalRankOrigin;
    let (master, mut value) = fixture();
    let expected = power(&parse(&value), &master);
    value["player"]["characterTotalRank"] = Value::Null;
    let snapshot = parse(&value);
    let report = snapshot.resolve(&master, "synthetic-351", Goal::Power);
    assert!(report.errors.is_empty() && report.missing.is_empty());
    let resolved = report.resolved.unwrap();
    assert_eq!(
        resolved.character_total_rank(),
        (master.characters.len() as i64, TotalRankOrigin::DerivedCompleteRanks)
    );
    assert!(resolved.snapshot().player.character_total_rank.is_none());
    assert_eq!(power(&snapshot, &master), expected);
}

#[test]
fn cultivation_caps_boundaries_and_missing_cap_are_explicit() {
    let (mut master, mut value) = fixture();
    value["eligible"]["members"][0]["rank"] = json!(0);
    assert!(
        parse(&value).resolve(&master, "synthetic-351", Goal::Power).errors.iter().any(|i| i.code == "invalid_value")
    );
    value["eligible"]["members"][0]["rank"] = json!(1);
    let row = master.member_card(1).unwrap();
    let rarity = row.rarity;
    // The synthetic level table has levels 1..30; awake=1 cap is 20.
    value["eligible"]["members"][0]["level"] = json!(20);
    assert!(parse(&value).resolve(&master, "synthetic-351", Goal::Power).resolved.is_some());
    value["eligible"]["members"][0]["level"] = json!(21);
    assert!(parse(&value).resolve(&master, "synthetic-351", Goal::Power).errors.iter().any(|i| i.code == "level_cap"));
    master.member_card_level_limits.retain(|row| row.rarity != rarity || row.awake_count != 1);
    assert!(
        parse(&value)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|i| i.code == "unsupported_master")
    );
}

#[test]
fn duplicate_identity_and_unowned_eligibility_are_not_merged() {
    let (master, mut value) = fixture();
    value["ownedFacts"]["memberIds"] = json!([1, 1, 2, 3, 4, 5]);
    let report = parse(&value).resolve(&master, "synthetic-351", Goal::Power);
    assert!(report.errors.iter().any(|i| i.code == "duplicate_id"));
    assert!(report.errors.iter().any(|i| i.code == "not_owned"));
    value["player"]["eventIds"] = json!([i64::MAX]);
    assert!(
        parse(&value)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|i| i.path == "player.eventIds" && i.code == "unknown_id")
    );
}

#[test]
fn pool_rules_reject_repeated_characters_snaps_and_unowned_deck_ids() {
    let (master, value) = fixture();
    let resolved = parse(&value).resolve(&master, "synthetic-351", Goal::Power).resolved.unwrap();
    assert!(resolved.evaluate_deck([1, 1, 3, 4, 5], [None; 5], &objective()).is_err());
    assert!(resolved.evaluate_deck([1, 2, 3, 4, 5], [Some(1), Some(1), None, None, None], &objective()).is_err());
    assert!(resolved.evaluate_deck([1, 2, 3, 4, 99], [None; 5], &objective()).is_err());
}

#[test]
fn explicit_total_preserves_legacy_player_behavior_when_absent() {
    let mut player = ournotes_sim::cards::Player::default();
    player.character_ranks.insert(1, 3);
    assert_eq!(player.character_total_rank(), 3);
    player.explicit_character_total_rank = Some(25);
    assert_eq!(player.character_total_rank(), 25);
    assert_eq!(player.character_ranks.len(), 1);
}

#[test]
fn snap_skills_come_from_the_actual_rank_row() {
    let (mut master, mut value) = fixture();
    let group = master.support_card(1).unwrap().rank_group;
    let row = master.support_card_ranks.iter_mut().find(|r| r.group == group && r.rank == 2).unwrap();
    row.support_skill_01_level = 3;
    row.support_skill_02_level = 4;
    row.gekisou_support_skill_01_level = 5;
    row.gekisou_support_skill_02_level = 6;
    let card = master.support_cards.iter_mut().find(|r| r.id == 1).unwrap();
    card.support_skill_id_01 = 101;
    card.support_skill_id_02 = 102;
    card.gekisou_support_skill_id_01 = 201;
    card.gekisou_support_skill_id_02 = 202;
    value["eligible"]["snaps"][0]["rank"] = json!(2);
    let resolved = parse(&value).resolve(&master, "synthetic-351", Goal::Power).resolved.unwrap();
    let derived = resolved.snap_skill_derivation(1).unwrap();
    assert_eq!(derived.normal, vec![(101, 3), (102, 4)]);
    assert_eq!(derived.gekisou, vec![(201, 5), (202, 6)]);
    assert!(resolved.snap_skill_derivation(999).is_err());
    // No manually supplied skill override is part of the snapshot schema.
    value["eligible"]["snaps"][0]["supportSkillLevel"] = json!(5);
    assert!(OwnedSnapshot::from_json(&value.to_string()).is_err());
}

fn band_fixture() -> (Master, Value) {
    use ournotes_sim::master::{BandItemEffectRow, BandItemLevelRow, BandItemRow};
    let (mut master, mut snapshot) = fixture();
    master.band_items = (101..=103).map(|id| BandItemRow { id, band_id: id - 100, ..Default::default() }).collect();
    master.band_item_levels = (101..=103)
        .flat_map(|id| {
            (1..=30).map(move |level| BandItemLevelRow {
                id: id * 1000 + level,
                band_item_id: id,
                level,
                player_rank: level,
            })
        })
        .collect();
    master.band_item_effects.retain(|row| row.band_item_id != 101);
    master.band_item_effects.extend((1..=50).map(|level| BandItemEffectRow {
        id: 10_000 + level,
        band_item_id: 101,
        level,
        skill_target_ids: vec![1],
        skill_effect_type: 1000,
        effect_value: level * 13,
    }));
    master.reindex().unwrap();
    snapshot["player"]["bandItems"] = Value::Null;
    snapshot["player"]["bandItemFacts"] = json!({"coverage":"complete","values":[
        {"id":101,"owned":false,"level":null},
        {"id":102,"owned":false,"level":null},
        {"id":103,"owned":false,"level":null}
    ]});
    (master, snapshot)
}

#[test]
fn reserved_band_effect_levels_do_not_authorize_cultivation() {
    let (master, mut snapshot) = band_fixture();
    for level in [1, 30, 31, 50, 0, -1, 51] {
        snapshot["player"]["bandItemFacts"]["values"][0]["owned"] = json!(true);
        snapshot["player"]["bandItemFacts"]["values"][0]["level"] = json!(level);
        let parsed = parse(&snapshot);
        let result = parsed.resolve(&master, "synthetic-351", Goal::Power);
        assert_eq!(result.resolved.is_some(), matches!(level, 1 | 30), "{level}: {:?}", result.errors);
        if !matches!(level, 1 | 30) {
            assert!(result.errors.iter().any(|error| error.code == "invalid_level"));
        }
        assert_eq!(parsed.player.band_item_facts.unwrap().values[0].level, Some(level));
        if matches!(level, 31 | 50) {
            assert!(master.band_item_effects.iter().any(|row| row.band_item_id == 101 && row.level == level));
        }
    }
    snapshot["player"].as_object_mut().unwrap().remove("bandItemFacts");
    for level in [1, 30, 31, 50] {
        snapshot["player"]["bandItems"] = json!([{"id":101,"value":level}]);
        assert_eq!(
            parse(&snapshot).resolve(&master, "synthetic-351", Goal::Skip).resolved.is_some(),
            matches!(level, 1 | 30)
        );
    }
}

#[test]
fn explicit_not_owned_unknown_owned_and_missing_level_remain_distinct() {
    let (master, mut snapshot) = band_fixture();
    let no_items = power(&parse(&snapshot), &master);
    snapshot["player"]["bandItemFacts"]["values"][0]["owned"] = json!(true);
    snapshot["player"]["bandItemFacts"]["values"][0]["level"] = json!(30);
    assert!(power(&parse(&snapshot), &master) > no_items);
    snapshot["player"]["bandItemFacts"]["values"][0]["level"] = Value::Null;
    let report = parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power);
    assert!(report.missing.iter().any(|issue| issue.path == "player.bandItemFacts.values[101].level"));
    assert!(report.resolved.is_none());
    snapshot["player"]["bandItemFacts"]["values"][0]["owned"] = Value::Null;
    snapshot["player"]["bandItemFacts"]["values"][0]["level"] = json!(1);
    let parsed = parse(&snapshot);
    assert!(
        parsed
            .resolve(&master, "synthetic-351", Goal::Power)
            .missing
            .iter()
            .any(|issue| issue.path.ends_with("[101].owned"))
    );
    assert_eq!(parsed.player.band_item_facts.unwrap().values[0].owned, None);
    snapshot["player"]["bandItemFacts"]["values"][0]["owned"] = json!(false);
    for invalid in [0, 1, 30] {
        snapshot["player"]["bandItemFacts"]["values"][0]["level"] = json!(invalid);
        assert!(
            parse(&snapshot)
                .resolve(&master, "synthetic-351", Goal::Power)
                .errors
                .iter()
                .any(|issue| issue.code == "cultivation_conflict")
        );
    }
}

#[test]
fn partial_item_saves_preserve_unknowns_and_complete_does_not_fill_omitted_ids() {
    let (master, mut snapshot) = band_fixture();
    snapshot["player"]["bandItemFacts"]["coverage"] = json!("partial");
    snapshot["player"]["bandItemFacts"]["values"].as_array_mut().unwrap().pop();
    let parsed = parse(&snapshot);
    let report = parsed.resolve(&master, "synthetic-351", Goal::Power);
    assert!(report.resolved.is_none());
    assert!(report.missing.iter().any(|issue| issue.path == "player.bandItemFacts.values[103].owned"));
    assert_eq!(parsed.player.band_item_facts.unwrap().values.len(), 2);
    snapshot["player"]["bandItemFacts"]["coverage"] = json!("complete");
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "coverage_conflict")
    );
}

#[test]
fn new_manual_capability_and_legacy_nonempty_items_reject_effect_only_data() {
    let (mut master, mut snapshot) = band_fixture();
    master.band_items.clear();
    master.band_item_levels.clear();
    master.reindex().unwrap();
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "unsupported_master")
    );
    snapshot["player"].as_object_mut().unwrap().remove("bandItemFacts");
    snapshot["player"]["bandItems"] = json!([{"id":101,"value":1}]);
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "unsupported_master")
    );
    snapshot["player"]["bandItems"] = json!([]);
    assert!(parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power).resolved.is_some());
}

#[test]
fn item_fact_wire_is_strict_and_i64_ids_never_need_js_number() {
    let (mut master, mut snapshot) = band_fixture();
    let raw = snapshot.to_string();
    assert!(OwnedSnapshot::from_json(&raw.replacen("\"owned\":false", "\"owned\":false,\"owned\":true", 1)).is_err());
    for field in ["typo", "catalog"] {
        let mut invalid = snapshot.clone();
        invalid["player"]["bandItemFacts"]["values"][0][field] = json!(1);
        assert!(OwnedSnapshot::from_json(&invalid.to_string()).is_err());
    }
    for invalid in [json!("101"), json!(101.0), json!(u64::MAX)] {
        let mut changed = snapshot.clone();
        changed["player"]["bandItemFacts"]["values"][0]["id"] = invalid;
        assert!(OwnedSnapshot::from_json(&changed.to_string()).is_err());
    }
    let id = 9_007_199_254_740_993i64;
    master.band_items[0].id = id;
    for level in &mut master.band_item_levels {
        if level.band_item_id == 101 {
            level.band_item_id = id;
        }
    }
    for effect in &mut master.band_item_effects {
        if effect.band_item_id == 101 {
            effect.band_item_id = id;
        }
    }
    master.reindex().unwrap();
    snapshot["player"]["bandItemFacts"]["values"][0]["id"] = json!(id);
    assert_eq!(parse(&snapshot).player.band_item_facts.as_ref().unwrap().values[0].id, id);
    assert!(parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power).resolved.is_some());
}

#[test]
fn conflicting_inputs_duplicate_ids_and_unknown_catalog_ids_cannot_resolve() {
    let (master, mut snapshot) = band_fixture();
    snapshot["player"]["bandItems"] = json!([]);
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "input_conflict")
    );
    snapshot["player"]["bandItems"] = Value::Null;
    snapshot["player"]["bandItemFacts"]["values"][1]["id"] = json!(101);
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "duplicate_id")
    );
    snapshot["player"]["bandItemFacts"]["values"][1]["id"] = json!(999_999);
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "unknown_id")
    );
}

#[test]
fn explicit_ownership_coverage_and_not_owned_ids_survive_zero_budget_shared_evaluation() {
    use ournotes_search::types::RecommendationRequest;
    use ournotes_sim::data::DeckData;
    let (master, mut value) = band_fixture();
    value["player"]["bandItemFacts"]["coverage"] = json!("partial");
    let data = DeckData { master, charts: vec![], provenance: json!({"scope":"synthetic"}), sha256: None };
    let snapshot = parse(&value);
    let report = snapshot.resolve_data(&data, "synthetic-351", Goal::Power);
    assert!(report.errors.is_empty() && report.missing.is_empty());
    let request: RecommendationRequest = serde_json::from_value(json!({
        "format":"ournotes-deck.recommendation-request/1","execution":{"kind":"power"},"metric":{"kind":"power"},
        "limits":{"timeLimitMs":0}
    }))
    .unwrap();
    let result = report.resolved.unwrap().recommend(&data, &request).unwrap();
    let input = &result.resolved_context["ownedSnapshot"]["bandItemInput"];
    assert_eq!(input["kind"], "explicitOwnership");
    assert_eq!(input["coverage"], "partial");
    assert_eq!(input["notOwnedIds"], json!([101, 102, 103]));
    assert!(snapshot.player.band_items.is_none());
}

#[cfg(feature = "native-fixtures")]
#[test]
fn captured_jp_catalog_level_membership_ends_at_30_while_effects_continue_to_50() {
    let captured = reference::json("jp-band-item-catalog.json");
    let texts: Vec<_> = captured["tables"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, record)| (name.clone(), record["table"].to_string()))
        .collect();
    let mut master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
            .unwrap();
    assert_eq!(master.band_items.len(), 25);
    assert_eq!(master.band_item_levels.len(), 750);
    assert_eq!(master.band_item_effects.len(), 1250);
    assert_eq!(master.band_item(101).unwrap().band_id, 1);
    for item in &master.band_items {
        assert!(master.band_item_level(item.id, 1).is_some());
        assert!(master.band_item_level(item.id, 30).is_some());
        assert!(master.band_item_level(item.id, 31).is_none());
        assert!(master.band_item_level(item.id, 50).is_none());
        assert!(master.band_item_effects.iter().any(|row| row.band_item_id == item.id && row.level == 50));
    }
    assert!(ournotes_sim::master::TABLES.contains(&"MasterBandItem"));
    assert!(ournotes_sim::master::TABLES.contains(&"MasterBandItemLevel"));
    let mut duplicate = master.band_item_levels[0].clone();
    duplicate.id += 99_999_999;
    master.band_item_levels.push(duplicate);
    assert!(master.reindex().is_err());
}

#[test]
fn exact_level_membership_and_effect_dependency_are_both_required() {
    let (mut master, mut snapshot) = band_fixture();
    snapshot["player"]["bandItemFacts"]["values"][0]["owned"] = json!(true);
    snapshot["player"]["bandItemFacts"]["values"][0]["level"] = json!(2);
    master.band_item_levels.retain(|row| row.band_item_id != 101 || row.level != 2);
    master.reindex().unwrap();
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "invalid_level")
    );
    snapshot["player"]["bandItemFacts"]["values"][0]["level"] = json!(1);
    master.band_item_effects.retain(|row| row.band_item_id != 101 || row.level != 1);
    assert!(
        parse(&snapshot)
            .resolve(&master, "synthetic-351", Goal::Power)
            .errors
            .iter()
            .any(|issue| issue.code == "master_row_missing")
    );
}

#[cfg(feature = "native-fixtures")]
fn captured_rank_catalog() -> Master {
    let captured = reference::json("jp-player-rank-catalog.json");
    let tables: Vec<_> = captured["tables"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, record)| (name.clone(), record["table"].to_string()))
        .collect();
    Master::from_json_tables(|name| tables.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str())).unwrap()
}

/// Made-up player-rank tables with the shape the resolver checks: VIP ranks 1..=21 with bonus rows only from
/// rank 2, character ranks 1..=50, and total-rank thresholds up to 5000.
fn rank_catalog() -> Master {
    let tables = [
        ("MasterVip", (1..=21).map(|r| json!({"_id": r, "_vipRank": r})).collect::<Vec<_>>()),
        (
            "MasterVipRankBonus",
            (2..=21)
                .flat_map(|r| {
                    [7, 9].map(|t| json!({"_id": r * 10 + t, "_vipRank": r, "_vipBonusType": t, "_value": r}))
                })
                .collect(),
        ),
        ("MasterCharacterRank", (1..=50).map(|r| json!({"_id": r, "_rank": r, "_bonus": 3 * (r - 1)})).collect()),
        (
            "MasterCharacterTotalRank",
            [25, 100, 1000, 5000]
                .iter()
                .enumerate()
                .map(|(i, t)| json!({"_id": i + 1, "_totalRank": t, "_bonus": 10 * i}))
                .collect(),
        ),
    ];
    let texts: Vec<_> = tables.iter().map(|(name, rows)| (*name, json!({ "_allData": rows }).to_string())).collect();
    Master::from_json_tables(|name| texts.iter().find(|(key, _)| *key == name).map(|(_, text)| text.as_str())).unwrap()
}

#[cfg(feature = "native-fixtures")]
#[test]
fn captured_vip_and_character_rank_domains_are_independent_of_bonus_rows_and_thresholds() {
    let captured = captured_rank_catalog();
    assert_eq!(captured.vip_ranks.len(), 21);
    assert_eq!(captured.vip_rank_bonuses.len(), 125);
    assert_eq!(captured.vip_ranks.iter().map(|row| row.vip_rank).collect::<Vec<_>>(), (1..=21).collect::<Vec<_>>());
    assert_eq!(captured.vip_rank_bonuses.iter().map(|row| row.vip_rank).min(), Some(2));
    assert_eq!(captured.vip_rank_bonuses.iter().map(|row| row.vip_rank).max(), Some(21));
    assert!(captured.vip_rank(1).is_some());
    assert!(!captured.vip_rank_bonuses.iter().any(|row| row.vip_rank == 1));
    assert!(captured.vip_rank(0).is_none() && captured.vip_rank(22).is_none());
    assert_eq!(captured.character_ranks.iter().map(|row| row.rank).collect::<Vec<_>>(), (1..=50).collect::<Vec<_>>());
    assert_eq!(captured.character_total_ranks.len(), 57);
    assert_eq!(captured.character_total_ranks.iter().map(|row| row.total_rank).max(), Some(5000));
    assert!(ournotes_sim::master::TABLES.contains(&"MasterVip"));
}

#[test]
fn strict_vip_requires_represented_player_rank_even_if_a_bonus_row_exists() {
    let (mut master, mut snapshot) = fixture();
    let catalog = rank_catalog();
    master.vip_ranks = catalog.vip_ranks;
    master.vip_rank_bonuses = catalog.vip_rank_bonuses;
    master.reindex().unwrap();
    for rank in 1..=21 {
        snapshot["player"]["vipRank"] = json!(rank);
        let report = parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power);
        assert!(report.resolved.is_some(), "{rank}: {:?} {:?}", report.errors, report.missing);
    }
    master.vip_rank_bonuses.push(ournotes_sim::master::VipRankBonusRow {
        id: 999_999,
        vip_rank: 22,
        vip_bonus_type: 7,
        value: 99_999,
    });
    for rank in [-1, 0, 22] {
        snapshot["player"]["vipRank"] = json!(rank);
        let report = parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power);
        assert!(report.resolved.is_none());
        assert!(report.errors.iter().any(|issue| issue.path == "player.vipRank" && issue.code == "invalid_value"));
    }
    snapshot["player"]["vipRank"] = json!(1);
    master.vip_rank_bonuses.clear();
    assert!(parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power).resolved.is_some());
    master.vip_ranks.clear();
    let report = parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power);
    assert!(report.resolved.is_none());
    assert!(report.errors.iter().any(|issue| issue.path == "player.vipRank" && issue.code == "unsupported_master"));
}

#[test]
fn master_rejects_ambiguous_vip_ids_or_rank_and_character_domain_uses_full_table() {
    let mut catalog = rank_catalog();
    let mut duplicate = catalog.vip_ranks[0].clone();
    duplicate.id = 999_999;
    catalog.vip_ranks.push(duplicate);
    assert!(catalog.reindex().is_err());
    let mut catalog = rank_catalog();
    let mut duplicate = catalog.vip_ranks[0].clone();
    duplicate.vip_rank = 999_999;
    catalog.vip_ranks.push(duplicate);
    assert!(catalog.reindex().is_err());
    let (mut master, mut snapshot) = fixture();
    master.character_ranks = rank_catalog().character_ranks;
    for rank in [1, 50, 51, 5000] {
        snapshot["player"]["characterRanks"]["values"][0]["value"] = json!(rank);
        snapshot["player"]["characterTotalRank"] = json!(master.characters.len() as i64 - 1 + rank);
        let report = parse(&snapshot).resolve(&master, "synthetic-351", Goal::Power);
        assert_eq!(report.resolved.is_some(), rank <= 50, "{rank}: {:?} {:?}", report.errors, report.missing);
    }
}
