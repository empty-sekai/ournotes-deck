//! Controller-only identities retain the old ordinary-input refusal boundary.
use super::*;
use serde_json::json;

fn fixture() -> (Master, [Performer; SLOTS]) {
    let (mut master, physical) = super::tests::fixture();
    master.skill_targets.push(serde_json::from_value(json!({"_id":20,"_cardType":2})).unwrap());
    master.skill_targets.push(serde_json::from_value(json!({"_id":21,"_judgement":5})).unwrap());
    for (id, kind, values, targets) in
        [(20, 5000, vec![], vec![20]), (21, 2001, vec![900], vec![]), (22, 7005, vec![0], vec![])]
    {
        master.skill_conditions.push(crate::master::SkillConditionRow {
            id,
            condition_type: kind,
            condition_values: values,
            condition_target_ids: targets,
            is_positive: true,
        });
    }
    master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 20,
        group: 20,
        condition_ids: vec![20, 21],
    });
    master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 21,
        group: 20,
        condition_ids: vec![22],
    });
    master.cumulative_conditions.push(
        serde_json::from_value(json!({
            "_id":20,"_skillCumulativeConditionType":1000,"_conditionValues":[2],
            "_conditionTargetIDs":[21],"_maxCumulativeCount":4
        }))
        .unwrap(),
    );
    for (id, effect) in [(20, 12000), (21, 13000), (22, 13002)] {
        master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id,
            skill_id: 1,
            level: 1,
            skill_effect_type: effect,
            skill_trigger_type: ONE_SHOT,
            skill_trigger_condition_group: 1,
            skill_condition_group: 20,
            skill_release_condition_group: 3,
            skill_cumulative_condition_id: if effect == 13002 { 20 } else { 0 },
            effect_value: 1,
            activation_time_second: 0.2,
            ..Default::default()
        });
    }
    master.reindex().unwrap();
    (master, physical)
}

fn controller_keys(master: &Master, physical: &[Performer; SLOTS]) -> Option<InputKeys> {
    // Unit checks exercise the closed read proof. Public capability tests separately obtain this token only
    // through complete original-pair admission, including rejection and cancellation before publication.
    InputKeys::admitted_controller(
        master,
        physical,
        &LuckSkills::default(),
        &ControllerInputAdmission::after_complete_pairs(),
    )
}

/// Actual reduced native model, plan and complete frame transcript, using the original unnormalized deck.
fn native_controller(master: &Master, deck: &[Performer; SLOTS]) -> (String, Vec<u8>, Vec<u64>) {
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
    let mut prepared = prepare_recording::<ProbabilityMass>(
        master,
        &LuckSkills::default(),
        &notes,
        &[],
        params,
        &setup,
        &play,
        &deltas,
        deck,
        None,
        None,
        false,
    )
    .unwrap();
    assert!(prepared.life.is_none() && prepared.life_deck.is_none());
    assert!(prepared.model.rows.iter().all(|row| is_luck_chain(row.effect_type)));
    let model = crate::live::full::luck_exact::initialized_identity(&mut prepared.model).unwrap();
    let plan = RecordingCache::key(&prepared);
    let transcript = record_prepared(prepared, &notes, &play, &deltas, &mut || false).unwrap().unwrap().key().unwrap();
    (model, plan, transcript)
}

#[test]
fn family_controller_input_filtered_counters_and_attributes_preserve_native_all_120_labels() {
    let (master, physical) = fixture();
    assert!(InputKeys::new(&master, &physical).is_none(), "ordinary input proof keeps its original boundary");
    let keys = controller_keys(&master, &physical).unwrap();
    let mut changed = physical.clone();
    for (slot, performer) in changed.iter_mut().enumerate() {
        performer.character_id += 100;
        performer.card_type = 200 + slot as i64;
        performer.tag_ids = vec![700, slot as i64, 700];
        performer.live_skill_categories.reverse();
        performer.gekisou_skill_categories.clear();
    }
    let changed_keys = controller_keys(&master, &changed).unwrap();
    let mut order = [0, 1, 2, 3, 4];
    let mut labels = std::collections::BTreeSet::new();
    for label in 0..120 {
        assert!(labels.insert(order));
        assert_eq!(keys.key(&order, 1 << 20), changed_keys.key(&order, 1 << 20));
        let original = order.map(|slot| physical[slot].clone());
        let renamed = order.map(|slot| changed[slot].clone());
        assert_eq!(native_controller(&master, &original), native_controller(&master, &renamed), "label {label}");
        assert_eq!(next_order(&mut order), label != 119);
    }
    assert_eq!(labels.len(), 120);
    assert!(keys.key(&[0, 1, 2, 3, 4], 1 << 20).unwrap().starts_with(CONTROLLER_PREFIX));
}

#[test]
fn family_controller_input_keeps_writer_reads_sources_levels_counters_and_order_exact() {
    let (mut master, physical) = fixture();
    let mut level = master.gekisou_skill_effects[0].clone();
    level.id = 40;
    level.level = 2;
    master.gekisou_skill_effects.push(level);
    let mut source = master.gekisou_support_skills[0].clone();
    source.id = 4;
    master.gekisou_support_skills.push(source);
    let mut effect = master.gekisou_support_skill_effects[0].clone();
    effect.id = 41;
    effect.skill_id = 4;
    master.gekisou_support_skill_effects.push(effect);
    master.reindex().unwrap();
    let key = |deck: &[Performer; SLOTS]| controller_keys(&master, deck).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20);
    let original = key(&physical);
    for change in 0..5 {
        let mut changed = physical.clone();
        match change {
            0 => changed[0].band_id = 2,
            1 => changed[0].gekisou_skill = Some((1, 2)),
            2 => changed[0].gekisou_support_skills[0] = (4, 1),
            3 => changed[0].gekisou_support_skills.reverse(),
            _ => changed.swap(0, 1),
        }
        assert_ne!(original, key(&changed), "change {change}");
        if change == 0 {
            assert_ne!(native_controller(&master, &physical), native_controller(&master, &changed));
        }
    }
    // Unlike the ignored cumulative bonus, a retained writer's cumulative counter is never waived.
    master.gekisou_skill_effects[0].skill_cumulative_condition_id = 20;
    master.reindex().unwrap();
    assert!(controller_keys(&master, &physical).is_none());
}

#[test]
fn family_controller_input_refuses_converters_unknown_rows_and_nonempty_catalogues() {
    for effect in [12006, 13005, 99999] {
        let (mut master, physical) = fixture();
        master.gekisou_skill_effects.last_mut().unwrap().skill_effect_type = effect;
        master.reindex().unwrap();
        assert!(controller_keys(&master, &physical).is_none(), "effect {effect}");
    }
    let (master, physical) = fixture();
    let mut writers = LuckSkills::default();
    writers.rows.insert((LuckSource::Gekisou, 20), 0);
    assert!(InputKeys::admitted_controller(
        &master, &physical, &writers, &ControllerInputAdmission::after_complete_pairs(),
    ).is_none());
    let keys = controller_keys(&master, &physical).unwrap();
    let order = [0, 1, 2, 3, 4];
    let bytes = keys.key(&order, 1 << 20).unwrap();
    assert!(keys.key(&order, 0).is_none());
    assert!(keys.key(&order, bytes.len() - 1).is_none());
    assert_eq!(keys.key(&order, bytes.len()), Some(bytes));
}
