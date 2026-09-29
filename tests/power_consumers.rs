//! Synthetic consumers plus fixtures produced by executing the original ARM64 helpers.
use ournotes_deck::{
    bonus::{BandItemMaps, LeaderProfile, is_target_member, leader_skill_bonuses, vip_bonus},
    cards::{MemberView, Player},
    master::Master,
    memory::{MemoryState, current_music_bonus, memory_power_bonus},
    power::CardPower,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn master(tables: Value) -> Master {
    let texts: BTreeMap<String, String> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    Master::from_json_tables(|name| texts.get(name).map(String::as_str)).unwrap()
}
fn card() -> MemberView {
    MemberView {
        id: 1,
        character_id: 3,
        band_id: 2,
        card_type: 1,
        rarity: 1,
        level: 1,
        awake: 0,
        rank: 1,
        rank_group: 1,
        best_music_tag_ids: vec![10, 11],
        leader_skill_id: 1,
        leader_skill_level: 1,
        live_skill_id: 1,
        live_skill_level: 1,
        live_skill_categories: Some(vec![4]),
        gekisou_skill_id: 1,
        gekisou_skill_level: 1,
        gekisou_skill_categories: Some(vec![5]),
        gekisou_mission_type: Some(7),
        power: CardPower::EMPTY,
        character_rank: 0,
        character_total_rank: 0,
    }
}
fn expected(v: &Value) -> CardPower {
    CardPower::bp(v[0].as_i64().unwrap(), v[1].as_i64().unwrap(), v[2].as_i64().unwrap())
}
#[test]
fn native_leader_effects_and_search_profile() {
    let vectors: Value = serde_json::from_str(include_str!("power_native.json")).unwrap();
    let c = card();
    for v in vectors["leader"].as_array().unwrap() {
        let n = v["count"].as_u64().unwrap() as usize;
        let mut cards = vec![c.clone(); 5];
        for (i, c) in cards.iter_mut().enumerate() {
            c.band_id = if i < n { 2 } else { 99 };
        }
        let m = master(
            json!({"MasterSkillTarget":[{"_id":1,"_bandID":2}],"MasterSkillCumulativeCondition":[{"_id":1,"_skillCumulativeConditionType":3000,"_conditionTargetIDs":[1]}],"MasterLeaderSkillEffect":[{"_id":1,"_leaderSkillID":1,"_level":1,"_skillEffectType":v["effect"],"_effectValue":v["value"],"_skillCumulativeConditionID":1,"_maxEffectValue":1}]}),
        );
        let refs = std::array::from_fn(|i| &cards[i]);
        assert_eq!(leader_skill_bonuses(&m, &refs, None).unwrap(), [expected(&v["power"]); 5], "{v}");
        let profile = LeaderProfile::new(&m, 1, 1).unwrap();
        if profile.simple {
            assert_eq!(profile.simple_percent(&c), expected(&v["power"]));
        }
    }
}
#[test]
fn native_item_reducer_and_independent_registration() {
    let vectors: Value = serde_json::from_str(include_str!("power_native.json")).unwrap();
    let c = card();
    for v in vectors["item"].as_array().unwrap() {
        let copies = v["copies"].as_u64().unwrap();
        // One multi-field target registers under three independent keys; repeated ids register again.
        let (target, ids) = if copies == 1 {
            (json!({"_id":1,"_bandID":2}), json!([1]))
        } else {
            (
                json!({"_id":1,"_bandID":2,"_characterID":3,"_cardType":1}),
                if copies == 3 { json!([1]) } else { json!([1, 1]) },
            )
        };
        let m = master(
            json!({"MasterSkillTarget":[target],"MasterBandItemSkillEffect":[{"_id":1,"_bandItemId":1,"_level":1,"_skillEffectType":v["effect"],"_effectValue":v["value"],"_skillTargetIDs":ids}]}),
        );
        let p = Player { band_items: BTreeMap::from([(1, 1)]), ..Default::default() };
        assert_eq!(BandItemMaps::build(&m, &p).unwrap().bonus(&c), expected(&v["power"]), "{v}");
    }
}
#[test]
fn native_both_matchers_reach_music_memory_and_leader() {
    let vectors: Value = serde_json::from_str(include_str!("power_native.json")).unwrap();
    let c = card();
    let keys = [
        "_bandID",
        "_cardType",
        "_characterID",
        "_tagID",
        "_liveSkillCategories",
        "_gekisouSkillCategories",
        "_gekisouMissionType",
    ];
    let hits = [json!(2), json!(1), json!(3), json!(10), json!([4]), json!([5]), json!(7)];
    let misses = [json!(99), json!(99), json!(99), json!(99), json!([99]), json!([99]), json!(99)];
    for v in vectors["matcher"].as_array().unwrap() {
        let mask = v["mask"].as_u64().unwrap();
        let mut t = json!({"_id":1});
        for i in 0..7 {
            t[keys[i]] = if mask & (1 << i) != 0 { hits[i].clone() } else { misses[i].clone() };
        }
        let m = master(
            json!({"MasterSkillTarget":[t],"MasterMemoryMusicGroup":[{"_id":1,"_skillTargetIds":[1,1]}],"MasterMemoryMusicBonus":[{"_id":1,"_groupId":1,"_scoreRank":9,"_performance":10,"_technic":20,"_visual":30}]}),
        );
        assert_eq!(is_target_member(&c, m.skill_target(1).unwrap()), v["leader"].as_bool().unwrap());
        assert_eq!(
            memory_power_bonus(&m, &Player::default(), &c).unwrap(),
            if v["memory"].as_bool().unwrap() { CardPower::points(10, 20, 30) } else { CardPower::EMPTY }
        );
    }
}
#[test]
fn vip_only_type_seven_exact_rank_and_first_row() {
    let rows: Vec<_> = (0..10)
        .map(|i| json!({"_id":i,"_vipRank":3,"_vipBonusType":i,"_value":100+i}))
        .chain([json!({"_id":11,"_vipRank":3,"_vipBonusType":7,"_value":999})])
        .collect();
    let m = master(json!({"MasterVipRankBonus":rows}));
    assert_eq!(vip_bonus(&m, &Player { vip_rank: 3, ..Default::default() }), 107);
    assert_eq!(vip_bonus(&m, &Player { vip_rank: 4, ..Default::default() }), 0);
    for ty in (0..10).filter(|&ty| ty != 7) {
        let m = master(json!({"MasterVipRankBonus":[{"_vipRank":3,"_vipBonusType":ty,"_value":999}]}));
        assert_eq!(vip_bonus(&m, &Player { vip_rank: 3, ..Default::default() }), 0);
    }
}
#[test]
fn memory_last_eligible_owned_intersection_and_empty_targets() {
    let m = master(
        json!({"MasterSkillTarget":[{"_id":1,"_bandID":2}],"MasterMemoryMusicGroup":[{"_id":1,"_skillTargetIds":[1,1]},{"_id":2,"_skillTargetIds":[]}],"MasterMemoryMusic":[{"_id":1,"_groupId":1},{"_id":2,"_groupId":1}],"MasterMemoryMusicBonus":[{"_id":1,"_groupId":1,"_scoreRank":5,"_performance":99},{"_id":2,"_groupId":1,"_scoreRank":2,"_performance":10,"_technic":20,"_visual":30},{"_id":3,"_groupId":2,"_scoreRank":0,"_performance":999}],"MasterMemoryMemberLevel":[{"_id":1,"_point":2,"_performance":999},{"_id":2,"_point":1,"_technic":4}],"MasterMemorySupportLevel":[{"_id":1,"_point":1,"_visual":5}]}),
    );
    let state = MemoryState {
        music_ranks: BTreeMap::from([(1, 5), (2, 5)]),
        unlocked_members: [1, 2].into(),
        unlocked_supports: [1, 2].into(),
    };
    assert_eq!(current_music_bonus(&m, 1, &state).unwrap().id, 2);
    let p = Player {
        memory: Some(state),
        owned_member_card_ids: Some([1].into()),
        owned_support_card_ids: Some([2].into()),
        ..Default::default()
    };
    assert_eq!(memory_power_bonus(&m, &p, &card()).unwrap(), CardPower::points(10, 24, 35));
    let mut p = p;
    p.memory.as_mut().unwrap().music_ranks.remove(&2);
    assert_eq!(memory_power_bonus(&m, &p, &card()).unwrap(), CardPower::points(0, 4, 5));
}
