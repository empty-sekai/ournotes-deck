//! Native complete nominal branches independently validate optional profile command-work evidence.
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
fn profile_probe_runs_cover_all_native_starts_and_original_orders_in_both_skill_phases() {
    let mut checked = 0;
    let mut positive = 0;
    for grouped in [false, true] {
        let mut fixture = FamilyFixture::new();
        // The native 7021 checker may remain true through END/DELAY and closes only at COMPLETE/FINISH,
        // not at the fever endpoint. Keep the fixture's original music end and append an inactive tail.
        let music_end = fixture.input.params.music_length_ms;
        for time_ms in ((music_end + 20)..=(music_end + 200)).step_by(20) {
            fixture.input.play.frames.push(PlayFrame { time_ms, judged: Vec::new() });
            fixture.input.delta.push(0.02);
        }
        if grouped {
            for row in &mut fixture.input.master.skill_effect_settings {
                if row.skill_effect_type == 2000 {
                    row.phase = 1;
                }
            }
            fixture.input.master.reindex().unwrap();
            // Several judgements and ordinary skill events share an original frame. Only one observation
            // of the direct-probe checker exists at that frame's skill boundary.
            fixture.input.play.frames = (0..=(music_end + 200) / 100)
                .map(|index| {
                    let time_ms = index * 100;
                    PlayFrame {
                        time_ms,
                        judged: fixture
                            .input
                            .notes
                            .iter()
                            .filter(|note| time_ms - 100 < note.time_ms && note.time_ms <= time_ms)
                            .map(|note| JudgedNote {
                                note_id: note.note_id,
                                judgement: 5,
                                judgement_time_ms: note.time_ms,
                            })
                            .collect(),
                    }
                })
                .collect();
            fixture.input.delta = vec![0.1; fixture.input.play.frames.len()];
            fixture.input.events = vec![(0, 80), (0, 80), (1, 100), (2, 140), (3, 140), (4, 200)];
        }
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
        let mut representatives = BTreeMap::new();
        for (resources, physical) in physical_bindings(&fixture) {
            representatives.entry(domain.profile_for(&resources).unwrap()).or_insert(physical);
        }
        assert_eq!(representatives.len(), domain.profile_count());
        let mut curves = LuckDpCache::new(8 << 20);
        for (id, physical) in representatives {
            let profile = context.prepare_profile(&domain, id, Some(&mut curves), || false).unwrap().unwrap();
            for (index, order) in physical_orders().into_iter().enumerate() {
                let bound = profile
                    .order_probe_run_bound(index)
                    .unwrap_or_else(|| panic!("incomplete probe work: grouped={grouped} profile={id} order={order:?}"));
                let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
                let native = family_native_oracle(&fixture.input, &deck);
                assert!(native.maximum_probe_starts <= bound, "grouped={grouped} profile={id} order={order:?}");
                assert!(bound < fixture.input.play.frames.len() as u64);
                positive += usize::from(native.maximum_probe_starts > 0);
                checked += 1;
            }
            assert!(profile.order_probe_run_bound(120).is_none());
            let key = context.profile_program_key(&domain, id, 1 << 20, || false).unwrap();
            let program = LuckFamilyProgram::from_profile(&domain, key, &profile, 1 << 20, || false).unwrap();
            let key = context.profile_program_key(&domain, id, 1 << 20, || false).unwrap();
            let transported = program.transport(&domain, &key, id, || false).unwrap();
            for order in 0..120 {
                assert_eq!(transported.order_probe_run_bound(order), profile.order_probe_run_bound(order));
            }
        }
    }
    assert_eq!(checked, 2 * 6 * 120);
    assert!(positive > 0);
}

#[test]
fn profile_probe_runs_refuse_unproved_phase_and_late_lifetime_without_refusing_complete_laws() {
    for late in [false, true] {
        let mut fixture = FamilyFixture::new();
        if late {
            // Every chart note precedes music end, but some positive-mass probe lifetimes end after it.
            fixture.input.params.music_length_ms = 400;
        } else {
            for row in &mut fixture.input.master.skill_effect_settings {
                if row.skill_effect_type == 2000 {
                    row.phase = 0;
                }
            }
            fixture.input.master.reindex().unwrap();
        }
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
        let profile = context.prepare_profile(&domain, 0, None, || false).unwrap().unwrap();
        assert_eq!(profile.orders().len(), 120, "the original full law capability remains available");
        assert!(profile.orders().iter().enumerate().all(|(index, _)| profile.order_probe_run_bound(index).is_none()));
        assert!((0..120).all(|index| !profile.order_rank_probe_history_ready(index)));
    }
}

#[test]
fn profile_probe_runs_accept_native_fixed_band_and_character_conditions_for_every_original_order() {
    let mut checked = 0;
    for (selector, target) in
        [("band", json!({"_id":9892,"_bandID":7})), ("character", json!({"_id":9892,"_characterID":2}))]
    {
        let mut fixture = FamilyFixture::new();
        for performer in &mut fixture.input.deck {
            performer.band_id = 7;
        }
        for (_, performer) in fixture.choices.iter_mut().flatten() {
            performer.band_id = 7;
        }
        fixture.input.master.skill_targets.push(serde_json::from_value(target).unwrap());
        fixture.input.master.skill_conditions.push(crate::master::SkillConditionRow {
            id: 9892,
            // The full native lottery catalogue admits holder formation 5000. Its targets include both
            // band and character selectors; 3000/3001 have a separate existing catalogue refusal below.
            condition_type: 5000,
            condition_values: Vec::new(),
            condition_target_ids: vec![9892],
            is_positive: true,
        });
        fixture.input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id: 9892,
            group: 9892,
            condition_ids: vec![9892],
        });
        fixture
            .input
            .master
            .gekisou_skill_effects
            .iter_mut()
            .find(|row| row.skill_id == FAMILY_SCORE)
            .unwrap()
            .skill_condition_group = 9892;
        fixture.input.master.reindex().unwrap();
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
        let absent = domain.profile_for(&[None; 5]).unwrap();
        let profile = context.prepare_profile(&domain, absent, None, || false).unwrap().unwrap();
        for (index, order) in physical_orders().into_iter().enumerate() {
            let bound = profile.order_probe_run_bound(index).expect("fixed native formation condition");
            let deck: Vec<_> = order.iter().map(|&slot| fixture.choices[slot][0].1.clone()).collect();
            let model = LiveModel::new_gekisou(
                &fixture.input.master,
                &deck,
                &fixture.input.notes,
                &fixture.input.events,
                fixture.input.params,
                &fixture.input.setup,
            )
            .unwrap();
            assert!(model.luck_score_rows(&skills).iter().any(|row| row.may_hold));
            let native = family_native_oracle(&fixture.input, &deck);
            assert!(native.maximum_probe_starts > 0);
            assert!(native.maximum_probe_starts <= bound, "selector={selector} order={order:?}");
            checked += 1;
        }
    }
    assert_eq!(checked, 2 * 120);
}

#[test]
fn profile_probe_runs_do_not_bypass_lottery_catalogue_formation_admission() {
    for kind in [3000, 3001] {
        let mut fixture = FamilyFixture::new();
        fixture.input.master.skill_targets.push(serde_json::from_value(json!({"_id":9892,"_bandID":7})).unwrap());
        fixture.input.master.skill_conditions.push(crate::master::SkillConditionRow {
            id: 9892,
            condition_type: kind,
            condition_values: Vec::new(),
            condition_target_ids: vec![9892],
            is_positive: true,
        });
        fixture.input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id: 9892,
            group: 9892,
            condition_ids: vec![9892],
        });
        fixture
            .input
            .master
            .gekisou_skill_effects
            .iter_mut()
            .find(|row| row.skill_id == FAMILY_SCORE)
            .unwrap()
            .skill_condition_group = 9892;
        fixture.input.master.reindex().unwrap();
        // Factory can resolve these predicates, but that alone does not grant the complete family law.
        LiveModel::new_gekisou(
            &fixture.input.master,
            &fixture.input.deck,
            &fixture.input.notes,
            &fixture.input.events,
            fixture.input.params,
            &fixture.input.setup,
        )
        .unwrap();
        let error = luck_skills(&fixture.input.master).unwrap_err();
        assert!(matches!(error, Error::Unsupported(_)));
        assert!(error.to_string().contains(&format!("condition type {kind}")));
    }
}

#[test]
fn profile_probe_runs_inspect_unrequested_reward_pairs_before_granting_common_phase() {
    let mut fixture = FamilyFixture::new();
    fixture.input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 9893,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    fixture.input.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 9893,
        skill_id: 9893,
        level: 1,
        skill_trigger_type: SUSTAINED,
        skill_trigger_condition_group: FAMILY_PROBE,
        skill_effect_type: 2005,
        effect_value: 100,
        ..Default::default()
    });
    fixture
        .input
        .master
        .skill_effect_settings
        .push(serde_json::from_value(json!({"_id":2005,"_skillEffectType":2005,"_phase":1})).unwrap());
    // This non-writer reward resource is absent from the requested profile and its representative deck.
    // Its holder would use phase 1 alongside the fixed member's phase-2 probe in a legal physical binding.
    for choices in &mut fixture.choices {
        choices[2].1.gekisou_support_skills.push((9893, 1));
    }
    fixture.input.master.reindex().unwrap();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
    let absent = domain.profile_for(&[None; 5]).unwrap();
    let profile = context.prepare_profile(&domain, absent, None, || false).unwrap().unwrap();
    assert_eq!(profile.orders().len(), 120);
    assert!((0..120).all(|order| profile.order_probe_run_bound(order).is_none()));
    assert!(
        (0..120).all(|order| !profile.order_rank_probe_history_ready(order)),
        "an unrequested physical reward pair still blocks historical probability weighting"
    );
}
