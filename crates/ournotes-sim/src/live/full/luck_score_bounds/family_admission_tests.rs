//! Native witnesses for the optional family admission's cross-source and clock boundaries.
use super::*;
use crate::live::full::LuckFamilyError;

const ALIAS_ROW: i64 = 9701;
const ALIAS_GK: i64 = 9702;
const ALIAS_SUPPORT: i64 = 9703;
const NEVER_PLAYING: i64 = 9704;

fn alias_fixture() -> FamilyFixture {
    let mut fixture = FamilyFixture::new();
    // The independent nominal oracle below branches only at the native lottery sites. This witness needs no
    // additional random condition: the guarantee is deterministic while the lottery outcomes remain random.
    fixture
        .input
        .master
        .gekisou_support_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == FAMILY_GUARANTEE)
        .unwrap()
        .skill_condition_group = 0;
    if !fixture.input.master.judgement_parameters.iter().any(|row| row.note_simulate_judgement == 1) {
        fixture.input.master.judgement_parameters.push(
            serde_json::from_value(json!({
                "_id":9701,"_noteSimulateJudgement":1,"_scorePercent":0,"_damage":100
            }))
            .unwrap(),
        );
    }
    fixture.input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: ALIAS_GK,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    fixture.input.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: ALIAS_ROW,
        skill_id: ALIAS_GK,
        level: 1,
        skill_trigger_type: ONE_SHOT,
        skill_trigger_condition_group: FAMILY_START,
        skill_effect_type: 12006,
        effect_value: 6,
        activation_time_second: 0.5,
        skill_target_ids: vec![9251], // Perfect -> Just: the same LUCK controller class.
        ..Default::default()
    });
    fixture.input.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":ALIAS_ROW,"_supportSkillID":ALIAS_SUPPORT,"_level":1,
            "_skillTriggerType":1,"_skillTriggerConditionGroup":FAMILY_LIVE,
            "_skillEffectType":12006,"_effectValue":1,"_activationTimeSecond":0.5,
            "_skillTargetIDs":[9252]
        }))
        .unwrap(),
    ); // Great -> Miss: the declared stream has no Great notes.
    for choices in &mut fixture.choices {
        choices[1].1.gekisou_support_skills.push((ALIAS_GK, 1));
        choices[2].1.support_skills.push((ALIAS_SUPPORT, 1));
    }
    fixture.input.master.reindex().unwrap();
    fixture
}

/// Enumerate the original full interpreter's complete nominal tree and retain its actually consumed grades.
/// This does not call the controller DP, conversion graph, family preparation or score-bound replay.
fn native_grade_law(input: &RushCase, deck: &[Performer]) -> BTreeMap<Vec<(i32, i32, i32)>, Fraction> {
    let mut todo = vec![(Vec::new(), Fraction::ONE)];
    let mut law = BTreeMap::<Vec<(i32, i32, i32)>, Fraction>::new();
    let mut visited = 0;
    while let Some((prefix, mass)) = todo.pop() {
        visited += 1;
        assert!(visited <= 4096 && prefix.len() <= 16, "the complete synthetic nominal tree must finish");
        let mut native =
            LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        native.random = LiveRandom::with_nominal_prefix(prefix.clone());
        let mut grades = Vec::new();
        let mut result = Ok(());
        for (frame, &delta) in input.play.frames.iter().zip(&input.delta) {
            if let Err(error) = native.frame_timed(frame.time_ms, &frame.judged, delta) {
                result = Err(error);
                break;
            }
            grades.extend_from_slice(native.frame_judgements());
        }
        assert!(native.random.nominal_covers_draws());
        if let Some(outcomes) = native.random.nominal_branch() {
            assert!(result.is_err());
            let total = outcomes[0].total;
            assert_eq!(outcomes.iter().map(|outcome| outcome.weight).sum::<u64>(), total);
            for (choice, outcome) in outcomes.iter().enumerate() {
                assert_eq!(outcome.total, total);
                let mut next = prefix.clone();
                next.push(choice);
                todo.push((next, mass.times(u128::from(outcome.weight), u128::from(total))));
            }
            continue;
        }
        result.unwrap();
        assert!(native.random.nominal_prefix_consumed());
        assert_eq!(grades.len(), input.notes.len());
        assert!(native.gk.as_ref().unwrap().ctrl.states.iter().all(|state| state.state == gekisou::S_FINISH));
        law.entry(grades).and_modify(|old| *old = old.plus(mass)).or_insert(mass);
    }
    assert_eq!(law.values().copied().fold(Fraction::ZERO, Fraction::plus), Fraction::ONE);
    law
}

fn refused_preparation(fixture: &FamilyFixture) -> LuckFamilyError {
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
    context.prepare(&family_choices(fixture), None, family_limits(), || false).unwrap_err()
}

#[test]
fn controller_family_rejects_cross_source_conversion_target_aliases_from_different_choices() {
    let fixture = alias_fixture();
    let empty: Vec<_> = fixture.choices.iter().map(|choices| choices[0].1.clone()).collect();
    let mut first = empty.clone();
    first[0] = fixture.choices[0][1].1.clone();
    let mut second = empty.clone();
    second[1] = fixture.choices[1][2].1.clone();
    for deck in [&first, &second] {
        for grades in native_grade_law(&fixture.input, deck).keys() {
            assert!(
                grades.iter().all(|&(_, grade, _)| luck_judgement_class(grade) == Some(5)),
                "each conversion source alone preserves every declared controller class"
            );
        }
    }
    let mut both = first;
    both[1] = second[1].clone();
    let native = native_grade_law(&fixture.input, &both);
    for grades in native.keys() {
        assert_eq!(
            grades.iter().map(|&(_, grade, _)| grade).collect::<Vec<_>>(),
            vec![6, 1, 1, 1],
            "the later support converter actually borrows the GK converter's cached Perfect target"
        );
        assert!(grades.iter().any(|&(_, grade, _)| luck_judgement_class(grade) != Some(5)));
    }
    // No single admission pair contains both Snaps. The consistency condition must span all choices, not
    // merely each initialized pair model, and the expanded owner/type IDs are deliberately different.
    let error = refused_preparation(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::JudgementFeedback);
    assert!(error.error.to_string().contains("aliased conversion rows"));
}

#[test]
fn controller_family_checks_conversion_aliases_before_omitting_late_or_impossible_rows() {
    let mut fixture = alias_fixture();
    fixture.input.master.skill_targets.push(
        serde_json::from_value(json!({
            "_id":NEVER_PLAYING,"_skillTargetType":5,"_gekisouMissionType":3
        }))
        .unwrap(),
    );
    fixture.input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: NEVER_PLAYING,
        condition_type: 7020,
        condition_values: Vec::new(),
        condition_target_ids: vec![NEVER_PLAYING],
        is_positive: true,
    });
    fixture.input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: NEVER_PLAYING,
        group: NEVER_PLAYING,
        condition_ids: vec![NEVER_PLAYING],
    });
    fixture
        .input
        .master
        .gekisou_support_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == ALIAS_GK)
        .unwrap()
        .skill_trigger_condition_group = NEVER_PLAYING;
    fixture.input.master.reindex().unwrap();
    let error = refused_preparation(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::JudgementFeedback);
    assert!(error.error.to_string().contains("aliased conversion rows"));
}

#[test]
fn controller_family_equal_raw_conversion_targets_remain_admissible() {
    let mut fixture = alias_fixture();
    let second = fixture
        .input
        .master
        .support_skill_effects
        .iter_mut()
        .find(|row| row.support_skill_id == ALIAS_SUPPORT)
        .unwrap();
    second.skill_target_ids = vec![9251];
    second.effect_value = 6;
    fixture.input.master.reindex().unwrap();
    let family = prepare_family(&fixture, 8 * 1024 * 1024);
    assert_eq!(family.orders().len(), family.profile_count() * 120);
    assert!(family.profile_for(&[Some(0), Some(1), None, None, None]).is_some());
}

#[test]
fn controller_family_rejects_negative_event_outside_its_admitted_clock_domain() {
    let mut fixture = FamilyFixture::new();
    fixture.input.events[0].1 = -100;
    let row = fixture.input.master.live_skill_effects.iter_mut().find(|row| row.live_skill_id == 9401).unwrap();
    row.skill_effect_type = 3001;
    row.effect_value = 100;
    fixture.input.master.reindex().unwrap();
    let mut native = LiveModel::new_gekisou(
        &fixture.input.master,
        &fixture.input.deck,
        &fixture.input.notes,
        &fixture.input.events,
        fixture.input.params,
        &fixture.input.setup,
    )
    .unwrap();
    let initial_life = native.life.current_life;
    native.frame_timed(0, &[], 0.02).unwrap();
    assert_eq!(crate::live::score::get_frame(-100), 0, "native negative times clamp to frame zero");
    assert!(
        native
            .live
            .iter()
            .filter(|skill| skill.member == 0)
            .flat_map(|skill| &skill.effects)
            .any(|effect| { native.rows[effect.row].effect_type == 3001 && effect.state.execute_ms == -100 })
    );
    assert!(native.life.current_life > initial_life, "the native negative-time recovery actually executes");
    // This is an additional nonnegative-clock admission condition, not an error in the original scorer.
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let error = LuckFamilyContext::new(
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
    .unwrap_err();
    assert_eq!(error.reason, LuckFamilyDecline::Context);
    assert!(error.error.to_string().contains("negative ordinary skill event"));
}

#[test]
fn controller_family_rejects_negative_extensions_in_any_snap_choice() {
    let mut fixture = FamilyFixture::new();
    fixture
        .input
        .master
        .support_skill_effects
        .iter_mut()
        .find(|row| row.support_skill_id == FAMILY_EXTEND)
        .unwrap()
        .effect_value = -500;
    fixture.input.master.reindex().unwrap();
    let error = refused_preparation(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::RecorderAdmission);
    assert!(error.error.to_string().contains("negative ordinary duration extension"));
}

#[test]
fn controller_family_and_shared_dp_use_the_actual_selected_writer_mission_gate() {
    const UNUSED_LUCK: i64 = 9791;
    let mut fixture = FamilyFixture::new();
    let selected = fixture
        .input
        .master
        .gekisou_support_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == FAMILY_GUARANTEE)
        .unwrap();
    selected.skill_condition_group = 0;
    let mut alias = selected.clone();
    alias.skill_id = UNUSED_LUCK;
    fixture.input.master.gekisou_support_skill_effects.insert(0, alias);
    fixture.input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: UNUSED_LUCK,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    fixture
        .input
        .master
        .gekisou_support_skills
        .iter_mut()
        .find(|row| row.id == FAMILY_GUARANTEE)
        .unwrap()
        .gekisou_mission_type = 1;
    fixture.input.master.reindex().unwrap();
    let empty: Vec<_> = fixture.choices.iter().map(|choices| choices[0].1.clone()).collect();
    let mut selected = empty.clone();
    selected[0] = fixture.choices[0][1].1.clone();
    let model = LiveModel::new_gekisou(
        &fixture.input.master,
        &selected,
        &fixture.input.notes,
        &fixture.input.events,
        fixture.input.params,
        &fixture.input.setup,
    )
    .unwrap();
    let actual = model
        .cond
        .iter()
        .find(|skill| skill.updater.effects().iter().any(|effect| model.rows[effect.row].id == FAMILY_GUARANTEE))
        .unwrap();
    assert_eq!(actual.updater.gate_mission(), Some(1));
    let first_raw =
        fixture.input.master.gekisou_support_skill_effects.iter().find(|row| row.id == FAMILY_GUARANTEE).unwrap();
    assert_eq!(first_raw.skill_id, UNUSED_LUCK, "the first raw-ID match is deliberately another source");
    let absent = family_native_oracle(&fixture.input, &empty);
    let closed = family_native_oracle(&fixture.input, &selected);
    assert!(closed.paths > 1, "the independent native lottery law is nonconstant");
    assert_eq!(closed.joint, absent.joint, "a COMBO gate never opens on this all-LUCK chart");
    assert_eq!(closed.scores, absent.scores);
    let error = refused_preparation(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::RecorderAdmission);
    assert!(error.error.to_string().contains("no LUCK mission gate"));
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let error = luck_rush_dp_certified_with_events(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        &selected,
        None,
    )
    .unwrap_err();
    assert!(matches!(error, Error::Unsupported(_)));
    assert!(error.to_string().contains("only Luck mission mechanisms"));

    // Changing the selected header itself opens the actual native gate and changes the complete law. The
    // corrected compiler accepts that header; it does not reject every harmless raw-ID alias indiscriminately.
    fixture
        .input
        .master
        .gekisou_support_skills
        .iter_mut()
        .find(|row| row.id == FAMILY_GUARANTEE)
        .unwrap()
        .gekisou_mission_type = 2;
    fixture.input.master.reindex().unwrap();
    let opened = family_native_oracle(&fixture.input, &selected);
    assert_ne!(opened.joint, closed.joint);
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let curve = luck_rush_dp_certified_with_events(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        &selected,
        None,
    )
    .unwrap();
    for (&time, native) in &opened.joint {
        let at = curve.steps.partition_point(|(step, _)| *step <= time);
        let certified = at.checked_sub(1).map_or(
            [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO],
            |index| curve.steps[index].1,
        );
        for (mass, bound) in native.iter().zip(certified) {
            assert_probability_contains(*mass, bound);
        }
    }
}
