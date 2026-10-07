//! Complete physical two-writer covers checked against independent native nominal branches.
use super::*;

const EMPTY_MEMBER: i64 = 9600;
const SECOND_WRITER: i64 = 9601;
const MISS_TRIGGER: i64 = 9602;

fn two_writers(miss_gauge: bool) -> FamilyFixture {
    let mut fixture = FamilyFixture::new();
    let master = &mut fixture.input.master;
    master.gekisou_skills.push(crate::master::SkillRow {
        id: EMPTY_MEMBER,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    // Every physical owner has a native GK source, so attaching either Snap can execute its writer.
    for (slot, choices) in fixture.choices.iter_mut().enumerate() {
        for (_, performer) in choices {
            performer.gekisou_skill.get_or_insert((EMPTY_MEMBER, 1));
        }
        fixture.input.deck[slot].gekisou_skill.get_or_insert((EMPTY_MEMBER, 1));
    }
    let second = if miss_gauge {
        master.skill_conditions.push(crate::master::SkillConditionRow {
            id: MISS_TRIGGER,
            condition_type: 7000,
            condition_values: vec![0],
            condition_target_ids: Vec::new(),
            is_positive: true,
        });
        master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id: MISS_TRIGGER,
            group: MISS_TRIGGER,
            condition_ids: vec![MISS_TRIGGER],
        });
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id: SECOND_WRITER,
            gekisou_mission_type: 2,
            ..Default::default()
        });
        master.gekisou_support_skill_effects.push(
            serde_json::from_value(json!({
                "_id":SECOND_WRITER,"_gekisouSupportSkillID":SECOND_WRITER,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":MISS_TRIGGER,
                "_skillReleaseConditionGroup":FAMILY_FINISH,"_skillEffectType":11003,"_effectValue":10000,
                "_effectExecuteLimitCount":1,"_effectExecuteLimitResetConditionGroup":FAMILY_FINISH
            }))
            .unwrap(),
        );
        if let Some(setting) = master.skill_effect_settings.iter_mut().find(|row| row.skill_effect_type == 11003) {
            setting.phase = 1;
        } else {
            master.skill_effect_settings.push(
                serde_json::from_value(json!({"_id":SECOND_WRITER,"_skillEffectType":11003,"_phase":1})).unwrap(),
            );
        }
        SECOND_WRITER
    } else {
        // Two physical resources selecting the very same source must still occupy separate profile slots.
        FAMILY_GUARANTEE
    };
    for choices in &mut fixture.choices {
        choices[2].1.gekisou_support_skills.push((second, 1));
    }
    master.reindex().unwrap();
    fixture
}

fn check_native_cover(fixture: &FamilyFixture, family: &LuckControllerFamily) {
    assert_eq!(family.profile_count(), 31);
    assert_eq!(family.orders().len(), 31 * 120);
    let bindings = family.bindings().expect("bounded curve-free binding metadata");
    assert_eq!(bindings.profile_count(), 31);
    assert!(bindings.retained_bytes() < family.retained_bytes());
    let original_orders = physical_orders();
    let mut all_profiles = std::collections::BTreeSet::new();
    let mut checked = 0;
    for (resources, physical) in physical_bindings(fixture) {
        let profile = family.profile_for(&resources).expect("every legal physical assignment is covered");
        assert_eq!(bindings.profile_for(&resources), Some(profile));
        assert!(all_profiles.insert(profile), "two-writer physical assignments cannot share a profile label");
        let labels: std::collections::BTreeSet<_> =
            family.orders().iter().filter(|law| law.profile == profile).map(|law| law.positions).collect();
        assert_eq!(labels.len(), 120);
        for order in &original_orders {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            assert!(labels.contains(&positions));
        }
        for order in [[0, 1, 2, 3, 4], [4, 3, 2, 1, 0], [2, 4, 1, 0, 3]] {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let law = family.orders().iter().find(|law| law.profile == profile && law.positions == positions).unwrap();
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let oracle = family_native_oracle(&fixture.input, &deck);
            for (&time, masses) in &oracle.joint {
                for (mass, enclosure) in masses.iter().zip(law.joint_at(time)) {
                    assert_probability_contains(*mass, enclosure);
                }
            }
            checked += 1;
        }
    }
    assert_eq!(all_profiles.len(), 31);
    assert_eq!(checked, 31 * 3);
    assert!(family.profile_for(&[Some(0), Some(0), None, None, None]).is_none());
    assert!(family.profile_for(&[Some(1), Some(1), None, None, None]).is_none());
    assert!(family.profile_for(&[Some(2), None, None, None, None]).is_none());
    assert!(bindings.profile_for(&[Some(0), Some(0), None, None, None]).is_none());
    assert!(bindings.profile_for(&[Some(2), None, None, None, None]).is_none());
}

#[test]
fn two_physical_writers_with_identical_sources_keep_every_native_owner_and_order() {
    let fixture = two_writers(false);
    let family = prepare_family(&fixture, 8 * 1024 * 1024);
    check_native_cover(&fixture, &family);
}

#[test]
fn two_distinct_writer_mechanisms_keep_native_phases_and_zero_cache_coverage() {
    let fixture = two_writers(true);
    let mut both = fixture.input.deck.clone();
    both[0] = fixture.choices[0][1].1.clone();
    both[1] = fixture.choices[1][2].1.clone();
    let mut without_first = both.clone();
    without_first[0].gekisou_support_skills.clear();
    let mut without_second = both.clone();
    without_second[1].gekisou_support_skills.clear();
    let together = family_native_oracle(&fixture.input, &both);
    assert_ne!(together.joint, family_native_oracle(&fixture.input, &without_first).joint);
    assert_ne!(together.joint, family_native_oracle(&fixture.input, &without_second).joint);
    let family = prepare_family(&fixture, 0);
    check_native_cover(&fixture, &family);
}

#[test]
fn two_writer_family_budgets_and_cancellation_never_return_a_partial_cover() {
    let fixture = two_writers(true);
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let context = LuckFamilyContext::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .unwrap()
    .unwrap();
    let choices = family_choices(&fixture);
    for limit in 0..3 {
        let mut limits = family_limits();
        match limit {
            0 => limits.max_profiles = 30,
            1 => limits.max_order_evaluations = 31 * 120 - 1,
            2 => limits.max_frame_work = (31 * 120 + 1) * input.play.frames.len() as u64 - 1,
            _ => unreachable!(),
        }
        let error = context.prepare(&choices, None, limits, || false).unwrap_err();
        assert_eq!(error.reason, LuckFamilyDecline::Budget);
    }
    let polls = std::cell::Cell::new(0);
    let mut curves = LuckDpCache::new(8 * 1024 * 1024);
    let cancelled = context
        .prepare(&choices, Some(&mut curves), family_limits(), || {
            polls.set(polls.get() + 1);
            polls.get() >= 2000
        })
        .unwrap();
    assert!(cancelled.is_none());
    assert!(polls.get() >= 2000);
    let complete = context.prepare(&choices, Some(&mut curves), family_limits(), || false).unwrap().unwrap();
    assert_eq!(complete.profile_count(), 31);
    assert_eq!(complete.orders().len(), 31 * 120);
}
