//! Full native family admission remains ahead of the optional projected-input lookup.
use super::*;

fn context<'a>(input: &'a RushCase, skills: &'a LuckSkills) -> LuckFamilyContext<'a> {
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
fn family_input_hits_keep_all_labels_exact_curve_bits_and_zero_capacity_behavior() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture.input, &skills);
    let choices = family_choices(&fixture);
    let mut cached = LuckDpCache::new(8 * 1024 * 1024);
    let family = context.prepare(&choices, Some(&mut cached), family_limits(), || false).unwrap().unwrap();
    let stats = cached.stats();
    assert!(stats.family_input_lookups > 0 && stats.family_input_hits > 0);
    assert!(stats.recording_peak_entries <= 128 && stats.recording_peak_bytes <= 1 << 20);
    let mut off = LuckDpCache::new(0);
    let control = context.prepare(&choices, Some(&mut off), family_limits(), || false).unwrap().unwrap();
    assert_eq!(off.stats().family_input_lookups, 0);
    assert_eq!(off.stats().family_input_hits, 0);
    assert_eq!(family.orders().len(), family.profile_count() * 120);
    assert_eq!(family.orders().len(), control.orders().len());
    for (left, right) in family.orders().iter().zip(control.orders()) {
        assert_eq!((left.profile, left.positions), (right.profile, right.positions));
        for &time in family.note_times() {
            assert_eq!(left.joint_at(time), right.joint_at(time));
        }
    }
    for (resources, _) in physical_bindings(&fixture) {
        assert_eq!(family.profile_for(&resources), control.profile_for(&resources));
    }
    // The API exposes allocation identity only: equal values from independent cache-off runs need not share it.
    assert!(family.orders()[0].shares_joint_curve(&family.orders()[0]));
    assert!(!family.orders()[0].shares_joint_curve(&control.orders()[0]));
    assert!(
        family
            .orders()
            .iter()
            .enumerate()
            .any(|(i, law)| family.orders()[..i].iter().any(|old| old.shares_joint_curve(law)))
    );
}

#[test]
fn family_input_cache_cannot_bypass_pair_admission_or_turn_cancellation_into_a_family() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture.input, &skills);
    let choices = family_choices(&fixture);
    let mut curves = LuckDpCache::new(8 * 1024 * 1024);
    context.prepare(&choices, Some(&mut curves), family_limits(), || false).unwrap().unwrap();
    let before = curves.stats().family_input_lookups;
    let mut bad = choices.clone();
    let duplicate = bad[0][0].clone();
    bad[0].push(duplicate);
    assert_eq!(
        context.prepare(&bad, Some(&mut curves), family_limits(), || false).unwrap_err().reason,
        LuckFamilyDecline::PairDomain
    );
    assert_eq!(curves.stats().family_input_lookups, before);
    let mut calls = 0;
    let stopped = context
        .prepare(&choices, Some(&mut curves), family_limits(), || {
            calls += 1;
            calls > 180
        })
        .unwrap();
    assert!(stopped.is_none());
    assert!(curves.stats().family_input_lookups > before, "cancellation occurs during labelled evaluation");
}

#[test]
fn family_input_character_reader_declines_erasure_without_refusing_the_supported_family() {
    let mut fixture = FamilyFixture::new();
    // Every profile has this member source, so no profile can use the character-blind input proof.
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
    let context = context(&fixture.input, &skills);
    let mut curves = LuckDpCache::new(8 * 1024 * 1024);
    let family =
        context.prepare(&family_choices(&fixture), Some(&mut curves), family_limits(), || false).unwrap().unwrap();
    assert_eq!(family.orders().len(), family.profile_count() * 120);
    assert_eq!(curves.stats().family_input_lookups, 0);
    assert_eq!(curves.stats().family_input_hits, 0);
}
