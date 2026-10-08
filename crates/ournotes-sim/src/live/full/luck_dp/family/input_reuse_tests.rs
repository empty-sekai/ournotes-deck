use super::*;
use serde_json::json;

pub(super) fn fixture() -> (Master, [Performer; SLOTS]) {
    let tables = json!({
        "MasterLiveSettings":[
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}
        ],
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":60}],
        "MasterLiveGekisouLuckBonusLot":(0..5).flat_map(|kind| [0,3].map(move |result| json!({
            "_id":kind*10+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1
        }))).collect::<Vec<_>>(),
        "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2}],
        "MasterGekisouSupportSkill":[{"_id":2,"_gekisouMissionType":2},{"_id":3,"_gekisouMissionType":2}],
        "MasterGekisouSkillEffect":[{
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillEffectType":11001,
            "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillConditionGroup":2,
            "_effectValue":5000,"_activationTimeSecond":0.2
        }],
        "MasterGekisouSupportSkillEffect":[
            {"_id":2,"_gekisouSupportSkillID":2,"_level":1,"_skillEffectType":11005,"_skillConditionGroup":2,
             "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillReleaseConditionGroup":3,
             "_effectValue":3,"_effectLimitCount":1},
            {"_id":3,"_gekisouSupportSkillID":3,"_level":1,"_skillEffectType":11003,"_skillConditionGroup":2,
             "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillReleaseConditionGroup":3,"_effectValue":2500}
        ],
        "MasterSkillConditionSet":[
            {"_id":1,"_group":1,"_conditionIds":[1]},
            {"_id":2,"_group":2,"_conditionIds":[2]},
            {"_id":3,"_group":3,"_conditionIds":[3]}
        ],
        "MasterSkillCondition":[
            {"_id":1,"_conditionType":7010,"_conditionTargetIDs":[1],"_isPositive":true},
            {"_id":2,"_conditionType":5000,"_conditionTargetIDs":[2],"_isPositive":true},
            {"_id":3,"_conditionType":7013,"_isPositive":true}
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
        card_type: slot as i64 + 2,
        tag_ids: vec![slot as i64 + 10],
        live_skill_categories: vec![0, slot as i64 + 20],
        gekisou_skill_categories: vec![slot as i64 + 30],
        gekisou_mission_type: 2,
        gekisou_skill: Some((1, 1)),
        gekisou_support_skills: if slot == 0 { vec![(2, 1), (3, 1)] } else { Vec::new() },
        ..Default::default()
    });
    (master, physical)
}

#[test]
fn family_input_identity_erases_only_unread_attributes_and_keeps_sources() {
    let (master, physical) = fixture();
    let order = [0, 1, 2, 3, 4];
    let key = InputKeys::new(&master, &physical).unwrap().key(&order, 1 << 20).unwrap();
    let mut renamed = physical.clone();
    for performer in &mut renamed {
        performer.character_id += 1000;
        performer.card_type = -91;
        performer.tag_ids.reverse();
        performer.tag_ids.push(77);
        performer.live_skill_categories = vec![13, 0, -11];
        performer.gekisou_skill_categories.clear();
    }
    assert_eq!(Some(key.clone()), InputKeys::new(&master, &renamed).unwrap().key(&order, 1 << 20));
    for change in 0..5 {
        let mut changed = physical.clone();
        match change {
            0 => changed[0].band_id += 1,
            1 => changed[0].gekisou_mission_type += 1,
            2 => changed[0].gekisou_skill = None,
            3 => changed[0].gekisou_support_skills.reverse(),
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

#[derive(Debug, PartialEq, Eq)]
struct NativeWitness {
    complete_model: String,
    recorder_model: String,
    plan: Vec<u8>,
    transcript: Vec<u64>,
}

/// Independent native construction and recording from the original performers. No InputKeys or family
/// lookup is used by this oracle; the plan image includes exact action/chance bits and every probe label.
fn native_witness(master: &Master, physical: &[Performer; SLOTS]) -> NativeWitness {
    let notes: Vec<_> = [100, 110, 200, 300, 400, 700]
        .into_iter()
        .enumerate()
        .map(|(id, time_ms)| LiveNote { note_id: id as i32, time_ms, note_operate_type: 1, judgement_type: 1 })
        .collect();
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 1000,
        music_level: 20,
        converted_note_count: notes.len() as i32,
        music_length_ms: 2000,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers: vec![(100, 500)], missions: vec![2, 2, 2] };
    let mut frames: Vec<_> = (0..=20).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
    for note in &notes {
        frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
            note_id: note.note_id,
            judgement: 5,
            judgement_time_ms: note.time_ms,
        });
    }
    let play = LivePlay { frames, base_seed: 0 };
    let deltas = vec![0.1; play.frames.len()];
    let mut native = LiveModel::new_gekisou(master, physical, &notes, &[], params, &setup).unwrap();
    let complete_model = crate::live::full::luck_exact::initialized_identity(&mut native).unwrap();
    let skills = luck_skills(master).unwrap();
    let mut prepared = prepare_recording::<ProbabilityMass>(
        master,
        &skills,
        &notes,
        &[],
        params,
        &setup,
        &play,
        &deltas,
        physical,
        None,
        None,
        false,
    )
    .unwrap();
    assert!(prepared.life.is_none() && prepared.life_deck.is_none());
    let recorder_model = crate::live::full::luck_exact::initialized_identity(&mut prepared.model).unwrap();
    let plan = RecordingCache::key(&prepared);
    let recorded = record_prepared(prepared, &notes, &play, &deltas, &mut || false).unwrap().unwrap();
    let transcript = recorded.key().expect("the entire native frame transcript completed");
    NativeWitness { complete_model, recorder_model, plan, transcript }
}

#[test]
fn family_input_unread_attributes_preserve_native_model_plan_transcript_and_all_120_labels() {
    fn enumerate(depth: usize, used: u8, order: &mut [usize; SLOTS], orders: &mut Vec<[usize; SLOTS]>) {
        if depth == SLOTS {
            orders.push(*order);
            return;
        }
        for slot in 0..SLOTS {
            if used & (1 << slot) == 0 {
                order[depth] = slot;
                enumerate(depth + 1, used | (1 << slot), order, orders);
            }
        }
    }
    let (master, physical) = fixture();
    let original = physical.clone();
    let keys = InputKeys::new(&master, &physical).unwrap();
    assert_eq!(physical, original, "normalization must not modify native inputs");
    let mut orders = Vec::new();
    enumerate(0, 0, &mut [0; SLOTS], &mut orders);
    let mut labels = std::collections::BTreeSet::new();
    let mut native_by_key = std::collections::BTreeMap::new();
    for order in orders {
        assert!(labels.insert(order));
        let deck = order.map(|slot| physical[slot].clone());
        let actual = native_witness(&master, &deck);
        let key = keys.key(&order, 1 << 20).unwrap();
        if let Some(expected) = native_by_key.get(&key) {
            assert_eq!(&actual, expected, "original label {order:?}");
        } else {
            native_by_key.insert(key, actual);
        }
    }
    assert_eq!(labels.len(), 120);
    // Four physically distinct cards differ in unread attributes. All 24 permutations of those cards have
    // identical complete native recordings; the five possible positions of the support owner remain distinct.
    assert_eq!(native_by_key.len(), 5);
}

#[test]
fn family_input_each_read_attribute_keeps_a_native_counterexample_separate() {
    for field in 0..6 {
        let (mut master, mut physical) = fixture();
        for performer in &mut physical {
            performer.band_id = 0;
            performer.card_type = 0;
            performer.tag_ids.clear();
            performer.live_skill_categories.clear();
            performer.gekisou_skill_categories.clear();
            performer.gekisou_mission_type = 0;
        }
        // The native member predicate ignores this discriminator. Negative nonzero card/category/mission
        // values still read the corresponding performer field, while band and tag selectors require > 0.
        let target = master.skill_targets.iter_mut().find(|target| target.id == 2).unwrap();
        *target = crate::master::SkillTargetRow { id: 2, skill_target_type: 99, ..Default::default() };
        let mut changed = physical.clone();
        match field {
            0 => {
                target.band_id = 17;
                changed[3].band_id = 17;
            }
            1 => {
                target.card_type = -7;
                changed[3].card_type = -7;
            }
            2 => {
                target.tag_id = 23;
                changed[3].tag_ids.push(23);
            }
            3 => {
                target.live_skill_categories = vec![0, -11];
                changed[3].live_skill_categories.push(-11);
            }
            4 => {
                target.gekisou_skill_categories = vec![0, -13];
                changed[3].gekisou_skill_categories.push(-13);
            }
            _ => {
                target.gekisou_mission_type = -2;
                changed[3].gekisou_mission_type = -2;
            }
        }
        master.reindex().unwrap();
        let key = |deck: &[Performer; SLOTS]| InputKeys::new(&master, deck).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20);
        assert_ne!(key(&physical), key(&changed), "target field {field}");
        let before = native_witness(&master, &physical);
        let after = native_witness(&master, &changed);
        assert_ne!(before.complete_model, after.complete_model, "native reads field {field}");
        assert_ne!(before.recorder_model, after.recorder_model, "recorder reads field {field}");
    }
}

#[test]
fn family_input_read_mask_unions_all_owners_and_all_four_condition_groups() {
    for group_slot in 0..4 {
        let (mut master, physical) = fixture();
        master.skill_targets.push(crate::master::SkillTargetRow { id: 9, tag_id: 77, ..Default::default() });
        master.skill_conditions.push(crate::master::SkillConditionRow {
            id: 9,
            condition_type: 3000,
            condition_values: Vec::new(),
            condition_target_ids: vec![9],
            is_positive: true,
        });
        master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id: 9,
            group: 9,
            condition_ids: vec![9],
        });
        let row = &mut master.gekisou_support_skill_effects[1];
        match group_slot {
            0 => row.skill_trigger_condition_group = 9,
            1 => row.skill_condition_group = 9,
            2 => row.skill_release_condition_group = 9,
            _ => row.effect_execute_limit_reset_condition_group = 9,
        }
        master.reindex().unwrap();
        let mut changed = physical.clone();
        // The only reader is a support row of owner zero, but it reads another physical owner's tags.
        changed[4].tag_ids.push(77);
        let key = |deck: &[Performer; SLOTS]| InputKeys::new(&master, deck).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20);
        assert_ne!(key(&physical), key(&changed), "condition group slot {group_slot}");
    }
    for kind in [3000, 3001] {
        let (mut master, physical) = fixture();
        master.skill_conditions.iter_mut().find(|condition| condition.id == 2).unwrap().condition_type = kind;
        master.reindex().unwrap();
        let mut changed = physical.clone();
        if kind == 3000 {
            changed.iter_mut().for_each(|performer| performer.band_id = 0);
        } else {
            changed[4].band_id = 0;
        }
        let key = |deck: &[Performer; SLOTS]| InputKeys::new(&master, deck).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20);
        assert_ne!(key(&physical), key(&changed));
        // These whole-deck predicates are compiled natively even though the narrower public lottery-skill
        // classifier does not admit them beside a writer. Compare Factory directly without widening that gate.
        let fixed = |deck: &[Performer; SLOTS]| {
            let factory = conditions::Factory {
                master: &master,
                deck,
                initial_life: 1000,
                initial_time_ms: 0,
                skill_target_music_type: 0,
            };
            format!("{:?}", factory.group(2, 0).unwrap())
        };
        assert_ne!(fixed(&physical), fixed(&changed));
    }
}

#[test]
fn family_input_zero_and_negative_inactive_target_fields_authorize_erasure() {
    let mut reads = AttributeReads::default();
    assert!(reads.include(&crate::master::SkillTargetRow {
        band_id: -1,
        character_id: -2,
        tag_id: -3,
        live_skill_categories: vec![0, 0],
        gekisou_skill_categories: vec![0],
        ..Default::default()
    }));
    let (_, mut physical) = fixture();
    for performer in &mut physical {
        let sources = (performer.gekisou_skill, performer.gekisou_support_skills.clone());
        reads.erase_unread(performer);
        assert_eq!(
            *performer,
            Performer { gekisou_skill: sources.0, gekisou_support_skills: sources.1, ..Default::default() }
        );
    }
}

#[test]
fn family_input_does_not_erase_other_selectors_when_one_target_disjunct_already_matches() {
    let (mut master, physical) = fixture();
    // Every member already matches band 1. Tags still have a native consumer and must be retained in full,
    // including their order and multiplicity; present predicate truth is not an input-identity certificate.
    master.skill_targets.iter_mut().find(|target| target.id == 2).unwrap().tag_id = 77;
    master.reindex().unwrap();
    let mut changed = physical.clone();
    changed[4].tag_ids.extend([77, 77]);
    let key = |deck: &[Performer; SLOTS]| InputKeys::new(&master, deck).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20);
    assert_ne!(key(&physical), key(&changed));
    assert_eq!(native_witness(&master, &physical), native_witness(&master, &changed));
    let mut reordered = changed.clone();
    reordered[4].tag_ids.reverse();
    assert_ne!(key(&changed), key(&reordered));
}

#[test]
fn family_input_proof_checks_release_reset_omitted_rows_and_unknown_consumers() {
    for change in 0..15 {
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
            8 => {
                let mut row = master.gekisou_skill_effects[0].clone();
                row.id = 99;
                row.skill_effect_type = 2000; // omitted by the writer catalogue, still part of the proof
                row.skill_cumulative_condition_id = 7;
                master.gekisou_skill_effects.push(row);
            }
            9 => master.gekisou_skill_effects[0].level += 1,
            10 => master.gekisou_support_skill_effects[0].level += 1,
            11 => master.skill_targets.retain(|row| row.id != 2),
            12 => master.skill_conditions.retain(|row| row.id != 2),
            13 => master.skill_condition_sets.retain(|row| row.group != 2),
            _ => master.gekisou_skill_effects[0].skill_target_ids.push(99999),
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
