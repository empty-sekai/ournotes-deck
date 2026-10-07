use super::*;
use serde_json::json;

fn fixture() -> (Master, [Performer; SLOTS]) {
    let tables = json!({
        "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2}],
        "MasterGekisouSupportSkill":[{"_id":2,"_gekisouMissionType":2},{"_id":3,"_gekisouMissionType":2}],
        "MasterGekisouSkillEffect":[{
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillEffectType":11001,
            "_skillTriggerConditionGroup":1,"_skillConditionGroup":2
        }],
        "MasterGekisouSupportSkillEffect":[
            {"_id":2,"_gekisouSupportSkillID":2,"_level":1,"_skillEffectType":11005,"_skillConditionGroup":2},
            {"_id":3,"_gekisouSupportSkillID":3,"_level":1,"_skillEffectType":11003,"_skillConditionGroup":2}
        ],
        "MasterSkillConditionSet":[{"_id":1,"_group":1,"_conditionIds":[1]},{"_id":2,"_group":2,"_conditionIds":[2]}],
        "MasterSkillCondition":[
            {"_id":1,"_conditionType":7010,"_conditionTargetIDs":[1],"_isPositive":true},
            {"_id":2,"_conditionType":5000,"_conditionTargetIDs":[2],"_isPositive":true}
        ],
        "MasterSkillTarget":[{"_id":1,"_skillTargetType":5,"_gekisouMissionType":2},{"_id":2,"_bandID":1}]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(n, _)| n == name).map(|(_, text)| text.as_str())).unwrap();
    let physical = std::array::from_fn(|slot| Performer {
        character_id: slot as i64 + 1,
        band_id: 1,
        gekisou_skill: Some((1, 1)),
        gekisou_support_skills: if slot == 0 { vec![(2, 1), (3, 1)] } else { Vec::new() },
        ..Default::default()
    });
    (master, physical)
}

#[test]
fn family_input_identity_erases_only_proved_unread_character_and_keeps_every_other_field() {
    let (master, physical) = fixture();
    let order = [0, 1, 2, 3, 4];
    let key = InputKeys::new(&master, &physical).unwrap().key(&order, 1 << 20).unwrap();
    let mut renamed = physical.clone();
    for performer in &mut renamed {
        performer.character_id += 1000;
    }
    assert_eq!(Some(key.clone()), InputKeys::new(&master, &renamed).unwrap().key(&order, 1 << 20));
    for change in 0..9 {
        let mut changed = physical.clone();
        match change {
            0 => changed[0].band_id += 1,
            1 => changed[0].card_type += 1,
            2 => changed[0].tag_ids.push(7),
            3 => changed[0].live_skill_categories.push(8),
            4 => changed[0].gekisou_skill_categories.push(9),
            5 => changed[0].gekisou_mission_type += 1,
            6 => changed[0].gekisou_skill = Some((1, 2)),
            7 => changed[0].gekisou_support_skills.reverse(),
            _ => changed.swap(0, 1),
        }
        assert_ne!(
            Some(key.clone()),
            InputKeys::new(&master, &changed).unwrap().key(&order, 1 << 20),
            "change {change}"
        );
    }
    let keys = InputKeys::new(&master, &physical).unwrap();
    assert_ne!(Some(key.clone()), keys.key(&[1, 0, 2, 3, 4], 1 << 20));
    for capacity in [0, PREFIX.len(), key.len() - 1] {
        assert!(keys.key(&order, capacity).is_none());
    }
    assert_eq!(keys.key(&order, key.len()), Some(key));
    assert!(keys.key(&[0, 0, 2, 3, 4], 1 << 20).is_none());
    assert!(keys.key(&[0, 1, 2, 3, 5], 1 << 20).is_none());
}

#[test]
fn family_input_proof_checks_release_reset_omitted_rows_and_unknown_consumers() {
    for change in 0..9 {
        let (mut master, physical) = fixture();
        match change {
            0 => master.skill_targets.iter_mut().find(|row| row.id == 2).unwrap().character_id = 1,
            1 => master.skill_conditions.iter_mut().find(|row| row.id == 2).unwrap().condition_type = 99999,
            2 => master.gekisou_skill_effects[0].skill_cumulative_condition_id = 7,
            3 => master.gekisou_skill_effects[0].skill_effect_type = 12006,
            4 => master.gekisou_skill_effects[0].skill_effect_type = 13005,
            5 => master.gekisou_skill_effects[0].skill_effect_type = 99999,
            6 | 7 => {
                master.skill_conditions.iter_mut().find(|row| row.id == 2).unwrap().condition_type = 2001;
                master.gekisou_skill_effects[0].skill_condition_group = 0;
                for row in &mut master.gekisou_support_skill_effects {
                    row.skill_condition_group = 0;
                }
                if change == 6 {
                    master.gekisou_skill_effects[0].skill_release_condition_group = 2;
                } else {
                    master.gekisou_skill_effects[0].effect_execute_limit_reset_condition_group = 2;
                }
            }
            _ => {
                let mut row = master.gekisou_skill_effects[0].clone();
                row.id = 99;
                row.skill_effect_type = 2000; // omitted by the writer catalogue, still part of the proof
                row.skill_cumulative_condition_id = 7;
                master.gekisou_skill_effects.push(row);
            }
        }
        master.reindex().unwrap();
        assert!(InputKeys::new(&master, &physical).is_none(), "change {change}");
    }
    let (master, mut physical) = fixture();
    physical[0].live_skill = Some((9, 1));
    assert!(InputKeys::new(&master, &physical).is_none());
    physical[0].live_skill = None;
    physical[0].support_skills.push((9, 1));
    assert!(InputKeys::new(&master, &physical).is_none());
}

#[test]
fn family_input_and_compiled_keys_share_the_original_storage_capacity() {
    let (master, physical) = fixture();
    let keys = InputKeys::new(&master, &physical).unwrap();
    let raw = keys.key(&[0, 1, 2, 3, 4], 1 << 20).unwrap();
    let mut storage = recording_cache::Storage::<u8>::default();
    for index in 0u32..256 {
        let mut input = raw.clone();
        input.extend_from_slice(&index.to_le_bytes());
        storage.insert(input, Arc::new(1), 1 << 20);
        storage.insert(format!("compiled-recording-{index}").into_bytes(), Arc::new(2), 1 << 20);
    }
    let (entries, bytes) = storage.retained();
    assert!(entries <= 128 && bytes <= 1 << 20);
    storage.limit(0);
    assert_eq!(storage.retained(), (0, 0));
}
