//! Complete-domain controller keys for native-filtered Combo/Just programs.
use super::*;
use crate::live::full::LuckFamilyProgram;

const FILTERED_CONDITION: i64 = 9700;
const FILTERED_CUMULATIVE: i64 = 9701;
const FILTERED_SOURCE: i64 = 9702;

fn context<'a>(fixture: &'a FamilyFixture, skills: &'a LuckSkills) -> LuckFamilyContext<'a> {
    let input = &fixture.input;
    LuckFamilyContext::new(
        &input.master,
        skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .unwrap()
    .unwrap()
}

fn filtered_fixture() -> FamilyFixture {
    let mut fixture = FamilyFixture::new();
    let master = &mut fixture.input.master;
    master.skill_targets.push(serde_json::from_value(json!({"_id":9700,"_cardType":2})).unwrap());
    for (id, kind, values, targets) in [(FILTERED_CONDITION, 5000, vec![], vec![9700]), (9701, 7005, vec![0], vec![])] {
        master.skill_conditions.push(crate::master::SkillConditionRow {
            id,
            condition_type: kind,
            condition_values: values,
            condition_target_ids: targets,
            is_positive: true,
        });
    }
    master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: FILTERED_CONDITION,
        group: FILTERED_CONDITION,
        condition_ids: vec![FILTERED_CONDITION, FAMILY_LIFE],
    });
    master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 9701,
        group: FILTERED_CONDITION,
        condition_ids: vec![9701],
    });
    master.cumulative_conditions.push(
        serde_json::from_value(json!({
            "_id":FILTERED_CUMULATIVE,"_skillCumulativeConditionType":1000,"_conditionValues":[2],
            "_conditionTargetIDs":[9251],"_maxCumulativeCount":4
        }))
        .unwrap(),
    );
    master.gekisou_skills.push(crate::master::SkillRow {
        id: FILTERED_SOURCE,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    for (id, source, effect) in
        [(9710, FAMILY_SPEED, 12000), (9711, FAMILY_SCORE, 13000), (9712, FILTERED_SOURCE, 13002)]
    {
        master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id,
            skill_id: source,
            level: 1,
            skill_trigger_type: ONE_SHOT,
            skill_trigger_condition_group: FAMILY_START,
            skill_condition_group: FILTERED_CONDITION,
            skill_release_condition_group: FAMILY_FINISH,
            skill_effect_type: effect,
            skill_cumulative_condition_id: if effect == 13002 { FILTERED_CUMULATIVE } else { 0 },
            effect_value: 1,
            activation_time_second: 0.2,
            ..Default::default()
        });
        master.skill_effect_settings.push(
            serde_json::from_value(json!({
                "_id":effect,"_skillEffectType":effect,"_phase":2
            }))
            .unwrap(),
        );
    }
    for (owner, choices) in fixture.choices.iter_mut().enumerate() {
        for (_, performer) in choices {
            performer.card_type = 2;
            if owner == 2 {
                performer.gekisou_skill = Some((FILTERED_SOURCE, 1));
            }
        }
    }
    master.reindex().unwrap();
    fixture
}

#[test]
fn family_controller_program_filtered_rows_preserve_all_120_actual_native_joint_laws() {
    let fixture = filtered_fixture();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let source_choices = family_choices(&fixture);
    let mut target_choices = [1, 2, 0, 3, 4].map(|slot| source_choices[slot].clone());
    let unmodified_choices = target_choices.clone();
    for (owner, choices) in target_choices.iter_mut().enumerate() {
        for choice in choices {
            // The filtered row's actual formation predicate changes from true to false. Its LIFE/combo
            // readers and JudgementPerN cumulative remain in the full native source, with original IDs.
            choice.performer.card_type = 30 + owner as i64;
            choice.performer.band_id = 40 + owner as i64;
            choice.performer.character_id = 50 + owner as i64;
            choice.performer.tag_ids = vec![60, owner as i64, 60];
            choice.performer.live_skill_categories = vec![70, 0];
            choice.performer.gekisou_skill_categories = vec![80];
        }
    }
    let source = context.admit_domain(&source_choices, family_limits(), || false).unwrap().unwrap();
    let target = context.admit_domain(&target_choices, family_limits(), || false).unwrap().unwrap();
    let source_id = source.profile_for(&[Some(0), None, None, None, None]).unwrap();
    let resources = [None, None, Some(0), None, None];
    let target_id = target.profile_for(&resources).unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let donor = context.prepare_profile(&source, source_id, Some(&mut curves), || false).unwrap().unwrap();
    let key = context.profile_program_key(&source, source_id, 1 << 20, || false).unwrap();
    let program = LuckFamilyProgram::from_profile(&source, key, &donor, 1 << 20, || false).unwrap();
    let target_key = context.profile_program_key(&target, target_id, 1 << 20, || false).unwrap();
    assert!(program.matches(&target_key));
    let transported = program.transport(&target, &target_key, target_id, || false).unwrap();
    let fresh = context.prepare_profile(&target, target_id, None, || false).unwrap().unwrap();
    let physical: Vec<_> = target_choices
        .iter()
        .zip(resources)
        .map(|(choices, resource)| choices.iter().find(|choice| choice.resource == resource).unwrap().performer.clone())
        .collect();
    let unmodified: Vec<_> = unmodified_choices
        .iter()
        .zip(resources)
        .map(|(choices, resource)| choices.iter().find(|choice| choice.resource == resource).unwrap().performer.clone())
        .collect();
    let full_image = |deck: &[Performer]| {
        let input = &fixture.input;
        let mut model =
            LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        crate::live::full::luck_exact::initialized_identity(&mut model).unwrap()
    };
    assert_ne!(full_image(&physical), full_image(&unmodified), "the full native predicates really changed");
    let mut labels = std::collections::BTreeSet::new();
    for ((order, law), rebuilt) in physical_orders().iter().zip(transported.orders()).zip(fresh.orders()) {
        let mut positions = [0; 5];
        for (position, &slot) in order.iter().enumerate() {
            positions[slot] = position;
        }
        assert_eq!(law.positions, positions);
        assert_eq!(law.positions, rebuilt.positions);
        assert!(labels.insert(positions));
        let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
        let native = family_native_oracle(&fixture.input, &deck);
        if labels.len() == 1 {
            let original: Vec<_> = order.iter().map(|&slot| unmodified[slot].clone()).collect();
            assert_eq!(native.joint, family_native_oracle(&fixture.input, &original).joint);
        }
        for (&time, masses) in &native.joint {
            for (left, right) in law.joint_at(time).into_iter().zip(rebuilt.joint_at(time)) {
                assert_eq!(left.interval().lower().to_bits(), right.interval().lower().to_bits());
                assert_eq!(left.interval().upper().to_bits(), right.interval().upper().to_bits());
            }
            for (mass, enclosed) in masses.iter().zip(law.joint_at(time)) {
                assert_probability_contains(*mass, enclosed);
            }
        }
    }
    assert_eq!(labels.len(), 120);
    assert!(target.owns_profile(&transported) && !source.owns_profile(&transported));
    assert!(program.retained_bytes() <= 1 << 20);
    let mut polls = 0;
    assert!(
        program
            .transport(&target, &target_key, target_id, || {
                polls += 1;
                polls == 30
            })
            .is_none()
    );
    assert!(context.prepare_profile(&target, target_id, None, || true).unwrap().is_none());
    assert!(context.profile_program_key(&target, target_id, 0, || false).is_none());
    assert!(context.profile_program_key(&target, target_id, 1 << 20, || true).is_none());
}

#[test]
fn family_controller_program_twenty_member_bonus_template_admits_keys_without_reducing_labels() {
    let mut fixture = filtered_fixture();
    // A public synthetic roster shape: 17 Combo/Just members and three actual controller writers.
    // IDs and values are local to this fixture; no roster or master download is needed by this test.
    let effects = [
        13000, 13000, 13000, 13000, 13000, 12000, 12000, 12000, 12000, 12000, 12000, 12000, 12000, 12000, 13002, 13002,
        13002, 11001, 11001, 11001,
    ];
    let mut members = Vec::new();
    for (member, effect) in effects.into_iter().enumerate() {
        let id = 9800 + member as i64;
        fixture.input.master.gekisou_skills.push(crate::master::SkillRow {
            id,
            gekisou_mission_type: 2,
            ..Default::default()
        });
        fixture.input.master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id,
            skill_id: id,
            level: 1,
            skill_trigger_type: ONE_SHOT,
            skill_trigger_condition_group: FAMILY_START,
            skill_effect_type: effect,
            skill_condition_group: if effect == 11001 { 0 } else { FILTERED_CONDITION },
            skill_cumulative_condition_id: if effect == 13002 { FILTERED_CUMULATIVE } else { 0 },
            activation_time_second: 0.2,
            effect_value: if effect == 11001 { 5000 } else { 1 },
            ..Default::default()
        });
        members.push(Performer { character_id: id, card_type: 2, gekisou_skill: Some((id, 1)), ..Default::default() });
    }
    fixture.input.master.reindex().unwrap();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    for group in 0..4 {
        let choices = std::array::from_fn(|slot| {
            vec![LuckFamilyChoice { resource: None, performer: members[group * 5 + slot].clone() }]
        });
        let mut limits = family_limits();
        limits.max_order_evaluations = 119;
        assert!(context.admit_domain(&choices, limits, || false).is_err());
        assert!(context.admit_domain(&choices, family_limits(), || true).unwrap().is_none());
        let domain = context.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
        let profile = domain.profile_for(&[None; 5]).unwrap();
        assert!(context.profile_program_key(&domain, profile, 1 << 20, || false).is_some(), "group {group}");
    }
}

#[test]
fn family_controller_program_conversion_keeps_full_admission_and_key_refusal_separate() {
    for to in [5, 4] {
        let mut fixture = filtered_fixture();
        fixture.input.master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id: 9790,
            skill_id: FILTERED_SOURCE,
            level: 1,
            skill_trigger_type: ONE_SHOT,
            skill_trigger_condition_group: FAMILY_START,
            skill_effect_type: 12006,
            skill_target_ids: vec![9251],
            effect_value: to,
            activation_time_second: 0.2,
            ..Default::default()
        });
        fixture.input.master.reindex().unwrap();
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let choices = family_choices(&fixture);
        let domain = context.admit_domain(&choices, family_limits(), || false);
        if to == 5 {
            // An identity conversion may pass complete family admission; it still gets no broader input key.
            let domain = domain.unwrap().unwrap();
            let profile = domain.profile_for(&[None; 5]).unwrap();
            assert!(context.profile_program_key(&domain, profile, 1 << 20, || false).is_none());
        } else {
            assert!(domain.is_err(), "a changed LUCK judgement still requires the original feedback proof");
        }
    }
}
