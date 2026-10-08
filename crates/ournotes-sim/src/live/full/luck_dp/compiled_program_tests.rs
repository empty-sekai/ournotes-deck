use super::*;
use crate::chartstats::{
    luck_neutral, luck_table_dp_certified, luck_table_program, luck_table_program_virtual, luck_table_validate,
    luck_table_validate_virtual,
};

fn same_response(a: &LuckDpCertifiedResult, b: &LuckDpCertifiedResult) {
    assert_eq!(a.steps, b.steps);
    assert_eq!(a.probes, b.probes);
    assert_eq!(a.probe_transitions, b.probe_transitions);
    assert_eq!(a.peak_states, b.peak_states);
    assert_eq!(a.transitions, b.transitions);
    assert_eq!(a.range_moments.len(), b.range_moments.len());
    for (a, b) in a.range_moments.iter().zip(&b.range_moments) {
        assert_eq!(a.luck_points, b.luck_points);
        assert_eq!(a.lot_results, b.lot_results);
    }
}

#[test]
fn compiled_luck_program_matches_original_and_replays_without_recording() {
    for result in [0, 3] {
        let (mut master, notes, params, setup, play, delta) = fixture(result, 60);
        if result == 0 {
            // A Critical minimum guarantee needs a Critical item in every native lottery table.
            // Retain the Miss branch as well, so both original and compiled paths exercise branching.
            let critical: Vec<_> = master
                .gekisou_luck_bonus_lots
                .iter()
                .cloned()
                .map(|mut row| {
                    row.id += 100;
                    row.lot_result = 3;
                    row
                })
                .collect();
            master.gekisou_luck_bonus_lots.extend(critical);
        }
        master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
        let skills = luck_skills(&master).unwrap();
        for skill in [1, 2, 3] {
            let deck = [Performer {
                gekisou_skill: Some((skill, 1)),
                gekisou_support_skills: vec![(31, 1), (66, 1)],
                ..Default::default()
            }];
            let reference = luck_rush_dp_certified_with_moments(
                &master,
                &skills,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                &deck,
                None,
                None,
            )
            .unwrap();
            let compiled = compile_luck_program(
                &master,
                &skills,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                &deck,
                None,
                None,
                true,
            )
            .unwrap();
            let identity = compiled.identity_words().unwrap();
            assert!(!identity.is_empty());
            assert!(compiled.allocated_bytes().unwrap() > std::mem::size_of::<CompiledLuckProgram>());
            let _ = take_luck_record_profile();
            same_response(&reference, &compiled.certified().unwrap());
            same_response(&reference, &compiled.certified().unwrap());
            assert_eq!(compiled.identity_words().unwrap(), identity);
            assert_eq!(take_luck_record_profile().calls, 0, "propagation must not prepare or replay a native model");
        }
    }
}

#[test]
fn compiled_luck_program_identity_keeps_lottery_actions_probes_and_declared_judgements() {
    let (master, notes, params, setup, play, delta) = fixture(3, 60);
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let compile = |master: &Master, play: &LivePlay, probes: Option<&[Option<usize>]>| {
        compile_luck_program(master, &skills, &notes, &[], params, &setup, play, &delta, &deck, probes, None, false)
            .unwrap()
    };
    let original = compile(&master, &play, None);
    let key = original.identity_words().unwrap();
    assert_eq!(compile(&master, &play, None).identity_words().unwrap(), key);
    assert_ne!(compile(&master, &play, Some(&[None])).identity_words().unwrap(), key);
    let mut lottery = master.clone();
    lottery.gekisou_luck_bonus_lots[0].weight += 1;
    assert_ne!(compile(&lottery, &play, None).identity_words().unwrap(), key);
    let mut action = master.clone();
    action.gekisou_skill_effects.iter_mut().find(|row| row.skill_id == 1).unwrap().effect_value += 10000;
    assert_ne!(compile(&action, &play, None).identity_words().unwrap(), key);
    let mut judged = play.clone();
    judged.frames.iter_mut().find(|frame| !frame.judged.is_empty()).unwrap().judged[0].judgement = 0;
    assert_ne!(compile(&master, &judged, None).identity_words().unwrap(), key);
    let expected = original.certified().unwrap();
    drop(master);
    same_response(&expected, &original.certified().unwrap());
}

#[test]
fn compiled_luck_program_rejects_late_recording_failure_before_identity_publication() {
    let (master, notes, params, setup, mut play, delta) = fixture(3, 60);
    let skills = luck_skills(&master).unwrap();
    // The preceding valid frames must not turn a failed final recording into a reusable program.
    play.frames.last_mut().unwrap().judged.push(JudgedNote { note_id: -1, judgement: 5, judgement_time_ms: 2000 });
    let error = compile_luck_program(
        &master,
        &skills,
        &notes,
        &[],
        params,
        &setup,
        &play,
        &delta,
        &[Performer::default()],
        None,
        None,
        false,
    )
    .unwrap_err();
    assert!(matches!(error, Error::Input(_)));
}

#[test]
fn compiled_luck_table_program_keeps_complete_probe_batches_and_exact_holder_validation() {
    let (mut master, notes, params, setup, play, delta) = fixture(3, 60);
    // A neutral member only enables its support; its ordinary row is excluded by the original LUCK compiler.
    let mut neutral_skill = master.gekisou_skills[0].clone();
    neutral_skill.id = 4;
    master.gekisou_skills.push(neutral_skill);
    let mut neutral_row = master.gekisou_skill_effects[0].clone();
    neutral_row.id = 400;
    neutral_row.skill_id = 4;
    neutral_row.skill_effect_type = 2000;
    master.gekisou_skill_effects.push(neutral_row);
    // The same direct predicate in a different source catalogue gives a second shape. Four writer
    // holders leave one free position, so the original helper must use two ordered probe batches.
    let mut probe_skill = master.gekisou_skills[0].clone();
    probe_skill.id = 5;
    master.gekisou_skills.push(probe_skill);
    let mut probe_row = master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 31).unwrap().clone();
    probe_row.id = 500;
    probe_row.skill_id = 5;
    master.gekisou_skill_effects.push(probe_row);
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    assert_eq!(neutral, Some((4, 1)));
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    assert_eq!(skills.shapes.len(), 2);
    let entries = vec![(writer, 0), (writer, 1), (writer, 2), (writer, 3)];
    let original =
        luck_table_dp_certified(&master, &skills, neutral, &notes, params, &setup, &play, &delta, &entries).unwrap();
    let compiled =
        luck_table_program(&master, &skills, neutral, &notes, params, &setup, &play, &delta, &entries).unwrap();
    let identity = compiled.identity().unwrap();
    assert!(identity.source_version.ends_with(crate::SOURCE_SHA256));
    assert_eq!(identity.batches.len(), 2);
    assert_ne!(identity.batches[0], identity.batches[1], "shape-index mapping remains in each batch identity");
    assert!(identity.allocated_bytes().unwrap() > std::mem::size_of_val(&identity));
    assert!(compiled.allocated_bytes().unwrap() > std::mem::size_of_val(&compiled));
    same_response(&original, &compiled.certified().unwrap());
    let all_holders: Vec<_> = (0..5).map(|slot| (writer, slot)).collect();
    assert!(matches!(
        luck_table_program(&master, &skills, neutral, &notes, params, &setup, &play, &delta, &all_holders),
        Err(Error::Input(_))
    ));
    let doubled_main = [(writer, 0), (writer, 0)];
    assert!(matches!(
        luck_table_program(&master, &skills, neutral, &notes, params, &setup, &play, &delta, &doubled_main),
        Err(Error::Input(_))
    ));
}

fn five_writer_inputs() -> (Vec<(LuckSkillKey, usize)>, Vec<Performer>) {
    let mut entries = Vec::new();
    let mut deck = Vec::new();
    for (position, (main, supports)) in
        [(1, vec![66]), (2, vec![96]), (3, vec![]), (2, vec![66]), (1, vec![96])].into_iter().enumerate()
    {
        entries.push((LuckSkillKey { source: LuckSource::Gekisou, id: main, level: 1, matched: None }, position));
        for &id in &supports {
            entries.push((LuckSkillKey { source: LuckSource::GekisouSupport, id, level: 1, matched: None }, position));
        }
        deck.push(Performer {
            gekisou_skill: Some((main, 1)),
            gekisou_support_skills: supports.into_iter().map(|id| (id, 1)).collect(),
            ..Default::default()
        });
    }
    (entries, deck)
}

fn add_probe_neutral(master: &mut Master) {
    let mut neutral_skill = master.gekisou_skills[0].clone();
    neutral_skill.id = 4;
    master.gekisou_skills.push(neutral_skill);
    let mut neutral_row = master.gekisou_skill_effects[0].clone();
    neutral_row.id = 400;
    neutral_row.skill_id = 4;
    neutral_row.skill_effect_type = 2000;
    master.gekisou_skill_effects.push(neutral_row);
    master.reindex().unwrap();
}

#[test]
fn compiled_luck_virtual_five_writers_match_independent_native_held_probe() {
    for phase in [1, 2] {
        let (mut master, notes, params, setup, play, delta) = fixture(0, 60);
        add_probe_neutral(&mut master);
        let critical: Vec<_> = master
            .gekisou_luck_bonus_lots
            .iter()
            .cloned()
            .map(|mut row| {
                row.id += 100;
                row.lot_result = 3;
                row.weight = 2;
                row
            })
            .collect();
        master.gekisou_luck_bonus_lots.extend(critical);
        master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
        master
            .skill_effect_settings
            .push(serde_json::from_value(json!({"_id":1,"_skillEffectType":2000,"_phase":phase})).unwrap());
        let skills = luck_skills(&master).unwrap();
        let neutral = luck_neutral(&master, &skills);
        let (entries, mut native_deck) = five_writer_inputs();
        // Every original holder and support writer remains. An extra real support probe fits on the
        // third holder, independently exercising the original native admission and frame recorder.
        native_deck[2].gekisou_support_skills.push((31, 1));
        assert!(native_deck.iter().all(|p| p.gekisou_support_skills.len() <= 2));
        assert!(luck_table_validate(&master, &skills, neutral, &entries).is_err());
        luck_table_validate_virtual(&master, &skills, neutral, &entries).unwrap();
        let native =
            luck_rush_dp_certified(&master, &skills, &notes, params, &setup, &play, &delta, &native_deck, None)
                .unwrap();
        let held = compile_luck_program(
            &master,
            &skills,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            &native_deck,
            None,
            None,
            false,
        )
        .unwrap();
        let virtual_program =
            luck_table_program_virtual(&master, &skills, neutral, &notes, params, &setup, &play, &delta, &entries)
                .unwrap();
        assert_eq!(held.observer_contract(), "native-held-direct-7021/1");
        assert_eq!(virtual_program.observer_contract(), "validated-virtual-direct-7021/1");
        let held_words = held.identity_words().unwrap();
        let virtual_identity = virtual_program.identity().unwrap();
        assert_eq!(virtual_identity.batches.len(), 1);
        assert_eq!(held_words.last(), Some(&0));
        assert_eq!(virtual_identity.batches[0].last(), Some(&1));
        assert_ne!(held_words, virtual_identity.batches[0]);
        let _ = take_luck_record_profile();
        let actual = virtual_program.certified().unwrap();
        same_response(&native, &actual);
        same_response(&native, &held.certified().unwrap());
        assert_eq!(take_luck_record_profile().calls, 0);
        assert_eq!(actual.probes, vec![true]);
        assert_eq!(actual.probe_transitions.len(), play.frames.len());
        assert!(
            actual.steps.iter().any(|(_, buckets)| buckets
                .iter()
                .any(|p| { p.interval().lower() > 0.0 && p.interval().upper() < 1.0 })),
            "oracle must exercise a nontrivial lottery distribution"
        );
    }
}

#[test]
fn compiled_luck_virtual_refuses_unsupported_observers_and_illegal_holders() {
    let (mut master, notes, params, setup, play, delta) = fixture(3, 60);
    add_probe_neutral(&mut master);
    let (entries, _) = five_writer_inputs();
    for variant in 0..5 {
        let mut invalid = master.clone();
        let probe = invalid.gekisou_support_skill_effects.iter_mut().find(|row| row.skill_id == 31).unwrap();
        match variant {
            0 => probe.activation_time_second = 0.1,
            1 => probe.skill_release_condition_group = 7013,
            2 => probe.effect_execute_limit_count = 1,
            3 => {
                invalid
                    .skill_effect_settings
                    .push(serde_json::from_value(json!({"_id":1,"_skillEffectType":2000,"_phase":3})).unwrap());
            }
            4 => {
                probe.skill_condition_group = 4011;
                invalid.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
            }
            _ => unreachable!(),
        }
        let skills = luck_skills(&invalid).unwrap();
        let result = luck_table_program_virtual(
            &invalid,
            &skills,
            luck_neutral(&invalid, &skills),
            &notes,
            params,
            &setup,
            &play,
            &delta,
            &entries,
        );
        assert!(matches!(result, Err(Error::Unsupported(_))), "variant {variant}: {result:?}");
    }
    // Passing separate probe validation does not waive the original writer admission.
    let mut bad_writer = master.clone();
    bad_writer.gekisou_skill_effects.iter_mut().find(|row| row.skill_id == 2).unwrap().skill_effect_type = 11004;
    let bad_skills = luck_skills(&bad_writer).unwrap();
    assert!(matches!(
        luck_table_program_virtual(
            &bad_writer,
            &bad_skills,
            luck_neutral(&bad_writer, &bad_skills),
            &notes,
            params,
            &setup,
            &play,
            &delta,
            &entries,
        ),
        Err(Error::Unsupported(_))
    ));
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let mut four = entries.clone();
    four.retain(|(_, position)| *position != 4);
    let mut duplicate_main = entries.clone();
    duplicate_main.push(entries[0]);
    let mut outside = entries.clone();
    outside[0].1 = 5;
    let mut three_supports = entries.clone();
    three_supports.extend([entries[1], entries[1]]);
    for invalid in [four, duplicate_main, outside, three_supports] {
        assert!(matches!(luck_table_validate_virtual(&master, &skills, neutral, &invalid), Err(Error::Input(_))));
    }
    // Validation coverage cannot be invented by simply toggling output flags.
    let (_, native_deck) = five_writer_inputs();
    assert!(matches!(
        compile_luck_program_virtual(
            &master,
            &skills,
            &notes,
            params,
            &setup,
            &play,
            &delta,
            &native_deck,
            std::iter::empty(),
        ),
        Err(Error::Input(_))
    ));
}
