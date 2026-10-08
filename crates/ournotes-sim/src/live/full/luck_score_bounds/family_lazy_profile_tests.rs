//! Lazy requests retain whole-domain admission and independently prove all labels of each requested profile.
use super::*;
use crate::live::full::{LuckFamilyDomain, LuckFamilyProgram};

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
fn lazy_profile_native_laws_keep_original_labels_and_full_physical_mapping() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let choices = family_choices(&fixture);
    let domain = context.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    assert_eq!(domain.profile_count(), 6);
    let full = context.prepare(&choices, Some(&mut curves), family_limits(), || false).unwrap().unwrap();
    let mut representatives = std::collections::BTreeMap::new();
    for (resources, deck) in physical_bindings(&fixture) {
        let profile = domain.profile_for(&resources).expect("all physical bindings remain admitted");
        assert_eq!(Some(profile), full.profile_for(&resources));
        representatives.entry(profile).or_insert(deck);
    }
    assert_eq!(representatives.len(), 6);
    let mut checked = 0;
    for (&index, physical) in representatives.iter().rev() {
        // Out-of-order profile requests must retain their original full-domain IDs.
        let profile = context.prepare_profile(&domain, index, Some(&mut curves), || false).unwrap().unwrap();
        assert!(domain.owns_profile(&profile));
        assert_eq!(profile.profile(), index);
        assert_eq!(profile.orders().len(), 120);
        assert!(profile.retained_bytes() <= family_limits().max_retained_bytes);
        let mut labels = std::collections::BTreeSet::new();
        for order in physical_orders() {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let law = profile.orders().iter().find(|law| law.positions == positions).unwrap();
            assert_eq!(law.profile, index);
            assert!(labels.insert(law.positions));
            let eager = full.orders().iter().find(|law| law.profile == index && law.positions == positions).unwrap();
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let oracle = family_native_oracle(&fixture.input, &deck);
            for (&time, masses) in &oracle.joint {
                assert_eq!(law.joint_at(time), eager.joint_at(time));
                for (mass, enclosed) in masses.iter().zip(law.joint_at(time)) {
                    assert_probability_contains(*mass, enclosed);
                }
            }
            checked += 1;
        }
        assert_eq!(labels.len(), 120);
    }
    assert_eq!(checked, 6 * 120);
    assert!(domain.retained_bytes() <= family_limits().max_retained_bytes);
    assert!(domain.profile_for(&[Some(0), Some(0), None, None, None]).is_none());
}

#[test]
fn lazy_profile_scope_cancellation_and_full_cover_budgets_never_publish_a_partial_profile() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let first = context(&fixture, &skills);
    let second = context(&fixture, &skills);
    let choices = family_choices(&fixture);
    let domain = first.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    assert_eq!(
        second.prepare_profile(&domain, 0, Some(&mut curves), || false).unwrap_err().reason,
        LuckFamilyDecline::Context
    );
    assert_eq!(
        first.prepare_profile(&domain, domain.profile_count(), Some(&mut curves), || false).unwrap_err().reason,
        LuckFamilyDecline::IncompleteCoverage
    );
    let mut calls = 0;
    assert!(
        first
            .prepare_profile(&domain, 0, Some(&mut curves), || {
                calls += 1;
                calls >= 50
            })
            .unwrap()
            .is_none()
    );
    assert!(calls >= 50 && curves.stats().family_input_lookups > 0);
    let complete = first.prepare_profile(&domain, 0, Some(&mut curves), || false).unwrap().unwrap();
    assert_eq!(complete.orders().len(), 120);
    let another = first.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    assert!(!another.owns_profile(&complete), "equal choices do not forge an admitted-domain capability");
    assert!(first.prepare_profile(&domain, 0, Some(&mut curves), || true).unwrap().is_none());
    for kind in 0..5 {
        let mut limits = family_limits();
        match kind {
            0 => limits.max_pair_models = 1,
            1 => limits.max_profiles = 5,
            2 => limits.max_order_evaluations = 719,
            3 => limits.max_frame_work = 1,
            4 => limits.max_retained_bytes = 1,
            _ => unreachable!(),
        }
        let error = first.admit_domain(&choices, limits, || false).unwrap_err();
        assert_eq!(error.reason, if kind == 4 { LuckFamilyDecline::Capacity } else { LuckFamilyDecline::Budget });
    }
}

#[test]
fn lazy_profile_unrequested_bad_pairs_still_refuse_the_whole_domain() {
    for kind in 0..3 {
        let mut fixture = FamilyFixture::new();
        match kind {
            0 => fixture.life_reading_writer(),
            1 => fixture.conversion(4, 5),
            2 => {
                for choices in &mut fixture.choices {
                    choices[2].1.gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
                    choices.push((Some(2), choices[1].1.clone()));
                }
            }
            _ => unreachable!(),
        }
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let prepared: Result<Option<LuckFamilyDomain>, _> =
            context.admit_domain(&family_choices(&fixture), family_limits(), || false);
        assert_eq!(
            prepared.unwrap_err().reason,
            [LuckFamilyDecline::LifeFeedback, LuckFamilyDecline::JudgementFeedback, LuckFamilyDecline::WriterProfiles]
                [kind]
        );
    }
}

#[test]
fn budgeted_profiles_keep_full_pair_admission_and_charge_each_complete_native_cover() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let choices = family_choices(&fixture);
    let frames = fixture.input.play.frames.len() as u64;
    let mut limits = family_limits();
    limits.max_order_evaluations = 240;
    limits.max_frame_work = frames * 241;
    assert_eq!(context.admit_domain(&choices, limits, || false).unwrap_err().reason, LuckFamilyDecline::Budget);
    assert_eq!(context.prepare(&choices, None, limits, || false).unwrap_err().reason, LuckFamilyDecline::Budget);
    let mut domain = context.admit_profile_domain(&choices, limits, || false).unwrap().unwrap();
    let strict = context.admit_domain(&choices, family_limits(), || false).unwrap().unwrap();
    assert_eq!(domain.profile_count(), 6);
    assert_eq!(domain.work().reserved_frame_work, frames);
    assert_eq!(domain.work().reserved_order_evaluations, 0);
    assert!(domain.retained_bytes() <= limits.max_retained_bytes);
    for (resources, _) in physical_bindings(&fixture) {
        assert_eq!(domain.profile_for(&resources), strict.profile_for(&resources));
    }
    let mut curves = LuckDpCache::new(8 << 20);
    for profile in [5, 0] {
        let completed =
            context.prepare_budgeted_profile(&mut domain, profile, Some(&mut curves), || false).unwrap().unwrap();
        let reference = context.prepare_profile(&strict, profile, Some(&mut curves), || false).unwrap().unwrap();
        assert!(domain.owns_profile(&completed));
        assert!(!domain.owns_profile(&reference));
        assert_eq!(completed.orders().len(), 120);
        assert_eq!(completed.profile(), profile);
        for (actual, expected) in completed.orders().iter().zip(reference.orders()) {
            assert_eq!(actual.positions, expected.positions);
            assert_eq!(actual.profile, expected.profile);
            for &time in domain.note_times() {
                assert_eq!(actual.joint_at(time), expected.joint_at(time));
            }
        }
        let key = context.budgeted_profile_program_key(&domain, profile, 8 << 20, || false).unwrap();
        let program = LuckFamilyProgram::from_budgeted_profile(&domain, key, &completed, 8 << 20, || false).unwrap();
        let key = context.budgeted_profile_program_key(&domain, profile, 8 << 20, || false).unwrap();
        let before = domain.work();
        let transported = program.transport_budgeted(&domain, &key, profile, || false).unwrap();
        assert_eq!(domain.work(), before);
        assert!(domain.owns_profile(&transported));
        assert_eq!(transported.orders().len(), 120);
        assert!(program.transport_budgeted(&domain, &key, (profile + 1) % 6, || false).is_none());
    }
    let work = domain.work();
    assert_eq!(work.profile_attempts, 2);
    assert_eq!(work.reserved_order_evaluations, 240);
    assert_eq!(work.reserved_frame_work, limits.max_frame_work);
    assert_eq!(
        context.prepare_budgeted_profile(&mut domain, 1, Some(&mut curves), || false).unwrap_err().reason,
        LuckFamilyDecline::Budget
    );
    assert_eq!(domain.work().profile_attempts, 2);
    assert_eq!(domain.work().reserved_order_evaluations, 240);
    assert_eq!(domain.work().reserved_frame_work, limits.max_frame_work);
    assert_eq!(domain.work().budget_refusals, 1);
}

#[test]
fn budgeted_profile_cancellation_keeps_its_charge_and_cannot_publish_an_incomplete_cover() {
    let fixture = FamilyFixture::new();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let first = context(&fixture, &skills);
    let second = context(&fixture, &skills);
    let choices = family_choices(&fixture);
    let frames = fixture.input.play.frames.len() as u64;
    let mut limits = family_limits();
    limits.max_order_evaluations = 240;
    limits.max_frame_work = frames * 241;
    let mut domain = first.admit_profile_domain(&choices, limits, || false).unwrap().unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let before = domain.work();
    assert!(first.prepare_budgeted_profile(&mut domain, 0, Some(&mut curves), || true).unwrap().is_none());
    assert_eq!(domain.work(), before);
    assert_eq!(
        second.prepare_budgeted_profile(&mut domain, 0, Some(&mut curves), || false).unwrap_err().reason,
        LuckFamilyDecline::Context
    );
    assert_eq!(
        first.prepare_budgeted_profile(&mut domain, 6, Some(&mut curves), || false).unwrap_err().reason,
        LuckFamilyDecline::IncompleteCoverage
    );
    assert_eq!(domain.work(), before);
    let mut calls = 0;
    assert!(
        first
            .prepare_budgeted_profile(&mut domain, 0, Some(&mut curves), || {
                calls += 1;
                calls >= 120
            })
            .unwrap()
            .is_none()
    );
    assert!(calls >= 120 && curves.stats().family_input_lookups > 0);
    assert_eq!(domain.work().profile_attempts, 1);
    assert_eq!(domain.work().reserved_order_evaluations, 120);
    assert_eq!(domain.work().reserved_frame_work, frames * 121);
    let complete = first.prepare_budgeted_profile(&mut domain, 0, Some(&mut curves), || false).unwrap().unwrap();
    assert!(domain.owns_profile(&complete));
    assert_eq!(complete.orders().len(), 120);
    assert_eq!(domain.work().reserved_frame_work, limits.max_frame_work);
    assert_eq!(
        first.prepare_budgeted_profile(&mut domain, 1, Some(&mut curves), || false).unwrap_err().reason,
        LuckFamilyDecline::Budget
    );
    for kind in 0..3 {
        let mut limited = limits;
        match kind {
            0 => limited.max_order_evaluations = 119,
            1 => limited.max_frame_work = frames * 121 - 1,
            2 => limited.max_profiles = 5,
            _ => unreachable!(),
        }
        assert_eq!(
            first.admit_profile_domain(&choices, limited, || false).unwrap_err().reason,
            LuckFamilyDecline::Budget
        );
    }
}

#[test]
fn budgeted_profiles_reject_unrequested_semantic_failures_across_the_full_pair_domain() {
    for kind in 0..3 {
        let mut fixture = FamilyFixture::new();
        match kind {
            0 => fixture.life_reading_writer(),
            1 => fixture.conversion(4, 5),
            2 => {
                for choices in &mut fixture.choices {
                    choices[2].1.gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
                    choices.push((Some(2), choices[1].1.clone()));
                }
            }
            _ => unreachable!(),
        }
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let mut limits = family_limits();
        limits.max_order_evaluations = 120;
        limits.max_frame_work = fixture.input.play.frames.len() as u64 * 121;
        let error = context.admit_profile_domain(&family_choices(&fixture), limits, || false).unwrap_err();
        assert_eq!(
            error.reason,
            [LuckFamilyDecline::LifeFeedback, LuckFamilyDecline::JudgementFeedback, LuckFamilyDecline::WriterProfiles]
                [kind]
        );
    }
}
