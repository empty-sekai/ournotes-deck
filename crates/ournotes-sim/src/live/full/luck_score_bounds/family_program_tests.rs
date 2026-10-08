//! Complete-profile reuse is an exact ordered-input isomorphism, with fresh target capabilities and labels.
use super::*;
use crate::live::full::LuckFamilyProgram;

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

#[test]
fn family_program_three_cycle_preserves_every_target_label_and_native_joint_law() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let source_choices = family_choices(&fixture);
    // A non-self-inverse mapping catches using source->target where target->source was required.
    let target_to_source = [1, 2, 0, 3, 4];
    let mut target_choices = target_to_source.map(|slot| source_choices[slot].clone());
    for (owner, choices) in target_choices.iter_mut().enumerate() {
        for choice in choices {
            // These attributes have no consumers in any projected source of this full domain. Native target
            // replay still receives each changed field; only its proved input identity may omit the difference.
            choice.performer.band_id = owner as i64 + 11;
            choice.performer.card_type = owner as i64 - 7;
            choice.performer.tag_ids = vec![owner as i64 + 31];
            choice.performer.live_skill_categories = vec![0, owner as i64 + 41];
            choice.performer.gekisou_skill_categories = vec![owner as i64 + 51];
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
    let fresh = context.prepare_profile(&target, target_id, Some(&mut curves), || false).unwrap().unwrap();
    assert!(target.owns_profile(&transported));
    assert!(!source.owns_profile(&transported));
    assert_eq!(transported.profile(), target_id);
    assert_eq!(transported.orders().len(), 120);
    let physical: Vec<_> = target_choices
        .iter()
        .zip(resources)
        .map(|(choices, resource)| choices.iter().find(|choice| choice.resource == resource).unwrap().performer.clone())
        .collect();
    let mut labels = std::collections::BTreeSet::new();
    for ((order, law), expected) in physical_orders().iter().zip(transported.orders()).zip(fresh.orders()) {
        let mut positions = [0; 5];
        for (position, &slot) in order.iter().enumerate() {
            positions[slot] = position;
        }
        // Check sequence, not merely the set: downstream coefficient additions retain native label order.
        assert_eq!((law.profile, law.positions), (target_id, positions));
        assert_eq!(law.positions, expected.positions);
        assert!(labels.insert(law.positions));
        let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
        let native = family_native_oracle(&fixture.input, &deck);
        for (&time, masses) in &native.joint {
            for (left, right) in law.joint_at(time).into_iter().zip(expected.joint_at(time)) {
                assert_eq!(left.interval().lower().to_bits(), right.interval().lower().to_bits());
                assert_eq!(left.interval().upper().to_bits(), right.interval().upper().to_bits());
            }
            for (mass, enclosed) in masses.iter().zip(law.joint_at(time)) {
                assert_probability_contains(*mass, enclosed);
            }
        }
    }
    // The empty projected members include repeated descriptors, but their physical labels remain distinct.
    assert_eq!(labels.len(), 120);
    assert!(program.retained_bytes() <= 1 << 20);
    assert_eq!(LuckFamilyProgram::retained_collection_bytes(std::iter::once(&program)), Some(program.retained_bytes()));
}

#[test]
fn family_program_provenance_context_capacity_and_cancellation_never_publish_partial_transport() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let choices = family_choices(&fixture);
    let source = context.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    let target = context.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    let index = source.profile_for(&[None; 5]).unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let donor = context.prepare_profile(&source, index, Some(&mut curves), || false).unwrap().unwrap();
    let target_law = context.prepare_profile(&target, index, Some(&mut curves), || false).unwrap().unwrap();
    let key = || context.profile_program_key(&source, index, 1 << 20, || false).unwrap();
    assert!(LuckFamilyProgram::from_profile(&target, key(), &target_law, 1 << 20, || false).is_none());
    assert!(LuckFamilyProgram::from_profile(&source, key(), &target_law, 1 << 20, || false).is_none());
    let wrong_profile = context.profile_program_key(&source, index + 1, 1 << 20, || false).unwrap();
    assert!(LuckFamilyProgram::from_profile(&source, wrong_profile, &donor, 1 << 20, || false).is_none());
    assert!(LuckFamilyProgram::from_profile(&source, key(), &donor, 1, || false).is_none());
    let mut polls = 0;
    assert!(
        LuckFamilyProgram::from_profile(&source, key(), &donor, 1 << 20, || {
            polls += 1;
            polls >= 30
        })
        .is_none()
    );
    assert_eq!(polls, 30);
    let program = LuckFamilyProgram::from_profile(&source, key(), &donor, 1 << 20, || false).unwrap();
    let target_key = context.profile_program_key(&target, index, 1 << 20, || false).unwrap();
    assert!(program.matches(&target_key));
    assert!(program.transport(&target, &key(), index, || false).is_none());
    assert!(program.transport(&target, &target_key, index + 1, || false).is_none());
    polls = 0;
    assert!(
        program
            .transport(&target, &target_key, index, || {
                polls += 1;
                polls >= 30
            })
            .is_none()
    );
    assert_eq!(polls, 30);
    assert_eq!(program.transport(&target, &target_key, index, || false).unwrap().orders().len(), 120);
    assert!(context.profile_program_key(&source, index, 0, || false).is_none());
    assert!(context.profile_program_key(&source, index, 1, || false).is_none());
    assert!(context.profile_program_key(&source, index, 1 << 20, || true).is_none());
    let mut key_polls = 0;
    assert!(
        context
            .profile_program_key(&source, index, 1 << 20, || {
                key_polls += 1;
                key_polls >= 2
            })
            .is_none()
    );
    let other = self::context(&fixture, &skills);
    let foreign = other.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    let foreign_key = other.profile_program_key(&foreign, index, 1 << 20, || false).unwrap();
    assert!(!program.matches(&foreign_key));
    assert!(program.transport(&foreign, &foreign_key, index, || false).is_none());
    assert!(other.profile_program_key(&source, index, 1 << 20, || false).is_none());
    // An unread tag is covered by the exact input proof. A changed native source still cannot be matched
    // merely because a particular marginal or terminal value happens to agree.
    let mut changed = choices.clone();
    for choice in &mut changed[0] {
        choice.performer.tag_ids.push(7_000_001);
    }
    let changed = context.admit_domain(&changed, family_limits(), || false).unwrap().unwrap();
    let changed_key = context.profile_program_key(&changed, index, 1 << 20, || false).unwrap();
    assert!(program.matches(&changed_key));
    let mut changed = choices.clone();
    for choice in &mut changed[0] {
        choice.performer.gekisou_skill = Some((FAMILY_SCORE, 1));
    }
    let changed = context.admit_domain(&changed, family_limits(), || false).unwrap().unwrap();
    let changed_key = context.profile_program_key(&changed, index, 1 << 20, || false).unwrap();
    assert!(!program.matches(&changed_key));
}

#[test]
fn family_program_character_reader_falls_back_to_complete_original_profile() {
    let mut fixture = FamilyFixture::new();
    fixture.input.master.skill_targets.push(
        serde_json::from_value(json!({
            "_id":9891,"_characterID":1
        }))
        .unwrap(),
    );
    fixture.input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 9891,
        condition_type: 5000,
        condition_values: Vec::new(),
        condition_target_ids: vec![9891],
        is_positive: true,
    });
    fixture.input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 9891,
        group: 9891,
        condition_ids: vec![9891],
    });
    fixture
        .input
        .master
        .gekisou_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == FAMILY_SPEED)
        .unwrap()
        .skill_condition_group = 9891;
    fixture.input.master.reindex().unwrap();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
    let index = domain.profile_for(&[None; 5]).unwrap();
    assert!(context.profile_program_key(&domain, index, 1 << 20, || false).is_none());
    let original = context.prepare_profile(&domain, index, None, || false).unwrap().unwrap();
    assert_eq!(original.orders().len(), 120);
    assert!(domain.owns_profile(&original));
}
