//! Empty-source projection is a controller identity, never a full score or raw model identity.
use super::*;

fn fixture() -> (Master, [Performer; SLOTS], [Performer; SLOTS]) {
    let (mut master, mut original) = super::tests::fixture();
    for id in 20..=27 {
        master.gekisou_skills.push(crate::master::SkillRow {
            id,
            gekisou_mission_type: (id - 20) % 4 + 1,
            ..Default::default()
        });
        for level in 1..=2 {
            master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
                id: 1000 + id * 10 + level,
                skill_id: id,
                level,
                skill_effect_type: [12000, 13000, 13002][(id % 3) as usize],
                skill_trigger_type: ONE_SHOT,
                skill_trigger_condition_group: 1,
                skill_release_condition_group: 3,
                effect_value: 100,
                activation_time_second: 0.2,
                ..Default::default()
            });
        }
    }
    for (id, effect) in [(30, 12000), (31, 13000), (32, 13002)] {
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id,
            gekisou_mission_type: 2,
            ..Default::default()
        });
        master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id: 2000 + id,
            skill_id: id,
            level: 1,
            skill_effect_type: effect,
            skill_trigger_type: ONE_SHOT,
            skill_trigger_condition_group: 1,
            skill_release_condition_group: 3,
            effect_value: 100,
            activation_time_second: 0.2,
            ..Default::default()
        });
    }
    original[0].gekisou_skill = Some((20, 1));
    original[0].gekisou_support_skills = vec![(30, 1), (2, 1), (31, 1), (3, 1), (32, 1)];
    for (slot, performer) in original.iter_mut().enumerate().skip(2) {
        performer.gekisou_skill = Some((slot as i64 + 19, 1));
    }
    let mut renamed = original.clone();
    renamed[0].gekisou_skill = Some((27, 2));
    renamed[0].gekisou_support_skills = vec![(2, 1), (3, 1)];
    renamed[2].gekisou_skill = None;
    renamed[3].gekisou_skill = Some((24, 2));
    renamed[4].gekisou_skill = Some((25, 2));
    master.reindex().unwrap();
    (master, original, renamed)
}

fn keys(master: &Master, physical: &[Performer; SLOTS]) -> Option<InputKeys> {
    InputKeys::admitted_controller(
        master,
        physical,
        &LuckSkills::default(),
        &ControllerInputAdmission::after_complete_pairs(),
    )
}

/// Both constructors and the entire frame recording consume original inputs. Empty native updaters are
/// deliberately retained in `model`; the existing compiled plan key independently discards only those
/// updaters, and the transcript includes every frame/query/action boundary used by the family controller.
fn native(master: &Master, deck: &[Performer; SLOTS]) -> (String, Vec<u8>, Vec<u64>) {
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
    // Full construction must also accept each original source, including its otherwise discarded rows.
    LiveModel::new_gekisou(master, deck, &notes, &[], params, &setup).unwrap();
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
fn family_controller_empty_sources_preserve_original_native_all_120_labels() {
    let (master, original, renamed) = fixture();
    let unchanged = original.clone();
    let left = keys(&master, &original).unwrap();
    let right = keys(&master, &renamed).unwrap();
    assert_eq!(original, unchanged, "only the private key is normalized");
    assert!(InputKeys::new(&master, &original).is_none(), "ordinary source proof remains unchanged");
    assert_eq!(left.canonical(1 << 20), right.canonical(1 << 20));
    let mut order = [0, 1, 2, 3, 4];
    let mut labels = std::collections::BTreeSet::new();
    for label in 0..120 {
        assert!(labels.insert(order));
        assert_eq!(left.key(&order, 1 << 20), right.key(&order, 1 << 20), "label {label}");
        let before = native(&master, &order.map(|slot| original[slot].clone()));
        let after = native(&master, &order.map(|slot| renamed[slot].clone()));
        assert_ne!(before.0, after.0, "empty updater identities were actually different");
        assert_eq!(before.1, after.1, "complete compiled controller, label {label}");
        assert_eq!(before.2, after.2, "complete native frame transcript, label {label}");
        assert_eq!(next_order(&mut order), label != 119);
    }
    assert_eq!(labels.len(), 120);
}

#[test]
fn family_controller_empty_main_preserves_support_enable_gate_and_canonical_bijection() {
    let (master, _, enabled) = fixture();
    let mut disabled = enabled.clone();
    disabled[0].gekisou_skill = None;
    let enabled_keys = keys(&master, &enabled).unwrap();
    let disabled_keys = keys(&master, &disabled).unwrap();
    assert_eq!(enabled_keys.physical, disabled_keys.physical, "the separate gate is indispensable");
    assert_ne!(enabled_keys.key(&[0, 1, 2, 3, 4], 1 << 20), disabled_keys.key(&[0, 1, 2, 3, 4], 1 << 20));
    assert_ne!(enabled_keys.canonical(1 << 20).unwrap().0, disabled_keys.canonical(1 << 20).unwrap().0);
    assert_ne!(native(&master, &enabled).1, native(&master, &disabled).1, "main presence enables native support rows");

    // Two otherwise identical descriptors must sort by their support-enable gate as well. The returned
    // inverse transports every distinct physical label, including equal-descriptor multiplicities.
    let mut mixed = enabled.clone();
    mixed[2] = disabled[0].clone();
    let permutation = [2, 4, 0, 3, 1];
    let reordered = permutation.map(|slot| mixed[slot].clone());
    let left = keys(&master, &mixed).unwrap();
    let right = keys(&master, &reordered).unwrap();
    let (left_key, left_inverse) = left.canonical(1 << 20).unwrap();
    let (right_key, right_inverse) = right.canonical(1 << 20).unwrap();
    assert_eq!(left_key, right_key);
    let mut right_physical = [0; SLOTS];
    for (physical, &canonical) in right_inverse.iter().enumerate() {
        right_physical[canonical] = physical;
    }
    let mut order = [0, 1, 2, 3, 4];
    let mut transported = std::collections::BTreeSet::new();
    for label in 0..120 {
        let canonical = order.map(|slot| left_inverse[slot]);
        let target = canonical.map(|slot| right_physical[slot]);
        assert!(transported.insert(target));
        assert_eq!(left.key(&order, 1 << 20), right.key(&target, 1 << 20));
        assert_eq!(next_order(&mut order), label != 119);
    }
    assert_eq!(transported.len(), 120);
}

#[test]
fn family_controller_empty_sources_share_one_namespace_but_keep_writer_order_and_reads() {
    let (master, original, _) = fixture();
    let mut absent = original.clone();
    for performer in &mut absent {
        performer.gekisou_skill = None;
        performer.gekisou_support_skills.clear();
    }
    let mut bonus = absent.clone();
    bonus[0].gekisou_skill = Some((20, 1));
    bonus[0].gekisou_support_skills = vec![(30, 1)];
    assert!(InputKeys::new(&master, &absent).is_some());
    assert!(InputKeys::new(&master, &bonus).is_none());
    let key = |deck: &[Performer; SLOTS]| keys(&master, deck).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20);
    assert_eq!(key(&absent), key(&bonus), "both paths use the admitted controller namespace");
    assert_eq!(native(&master, &absent).1, native(&master, &bonus).1);
    assert_eq!(native(&master, &absent).2, native(&master, &bonus).2);
    let base = key(&original);
    let mut reversed = original.clone();
    reversed[0].gekisou_support_skills.reverse();
    assert_ne!(base, key(&reversed), "empty sources cannot erase retained relative order");
    assert_ne!(native(&master, &original).1, native(&master, &reversed).1);
    let mut changed_attribute = original.clone();
    changed_attribute[0].band_id = 2;
    assert_ne!(base, key(&changed_attribute), "an empty main still owns a member-target support predicate");
    assert_ne!(native(&master, &original).1, native(&master, &changed_attribute).1);
}

#[test]
fn family_controller_empty_sources_require_complete_rows_and_preserve_mixed_source_identity() {
    let (master, original, _) = fixture();
    for source in [None, Some((99999, 1)), Some((20, 99999))] {
        let mut bad = original.clone();
        match source {
            None => bad[0].gekisou_support_skills.push((99999, 1)),
            some => bad[0].gekisou_skill = some,
        }
        assert!(keys(&master, &bad).is_none(), "missing source/selected level {source:?}");
    }
    let mut missing_support_level = original.clone();
    missing_support_level[0].gekisou_support_skills.push((30, 99999));
    assert!(keys(&master, &missing_support_level).is_none());
    for effect in [12006, 13005, 99999] {
        let mut bad = master.clone();
        bad.gekisou_skill_effects
            .iter_mut()
            .find(|row| row.skill_id == 20 && row.level == 1)
            .unwrap()
            .skill_effect_type = effect;
        bad.reindex().unwrap();
        assert!(keys(&bad, &original).is_none(), "effect {effect} cannot authorize empty-source deletion");
    }
    let mut bad_target = master.clone();
    bad_target
        .gekisou_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == 20 && row.level == 1)
        .unwrap()
        .skill_target_ids = vec![99999];
    bad_target.reindex().unwrap();
    assert!(keys(&bad_target, &original).is_none());

    let mut mixed = master.clone();
    for level in 1..=2 {
        let mut writer = mixed.gekisou_skill_effects[0].clone();
        writer.id = 4000 + level;
        writer.skill_id = 20;
        writer.level = level;
        mixed.gekisou_skill_effects.push(writer);
    }
    mixed.reindex().unwrap();
    let base = keys(&mixed, &original).unwrap();
    assert_eq!(base.physical[0].gekisou_skill, Some((20, 1)), "one writer prevents deleting the entire source");
    let mut changed = original.clone();
    changed[0].gekisou_skill = Some((20, 2));
    assert_ne!(base.key(&[0, 1, 2, 3, 4], 1 << 20), keys(&mixed, &changed).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20));
    changed[0].gekisou_skill = Some((21, 1));
    assert_ne!(base.key(&[0, 1, 2, 3, 4], 1 << 20), keys(&mixed, &changed).unwrap().key(&[0, 1, 2, 3, 4], 1 << 20));
    let order = [0, 1, 2, 3, 4];
    let bytes = base.key(&order, 1 << 20).unwrap();
    assert!(base.key(&order, bytes.len() - 1).is_none());
    assert_eq!(base.key(&order, bytes.len()), Some(bytes));
    assert!(base.key(&[0, 0, 2, 3, 4], 1 << 20).is_none());
    assert!(base.key(&[0, 1, 2, 3, 5], 1 << 20).is_none());
}
