use super::*;
use crate::chartstats::{luck_neutral, luck_table_dp_certified, luck_table_program};

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
