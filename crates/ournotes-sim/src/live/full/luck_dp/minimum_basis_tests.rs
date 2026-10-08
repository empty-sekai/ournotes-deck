use super::*;
use crate::chartstats::{luck_neutral, luck_table_program};

fn branching_master(master: &mut Master) {
    let originals = master.gekisou_luck_bonus_lots.clone();
    for result in 1..=3 {
        master.gekisou_luck_bonus_lots.extend(originals.iter().cloned().map(|mut row| {
            row.id += result * 100;
            row.lot_result = result;
            row
        }));
    }
    master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
}

fn probability_condition(master: &mut Master, probability: i64) -> i64 {
    let id = 4000 + probability;
    let mut condition = master.skill_conditions.iter().find(|row| row.id == 4011).unwrap().clone();
    condition.id = id;
    condition.condition_values = vec![probability];
    master.skill_conditions.push(condition);
    let mut set = master.skill_condition_sets.iter().find(|row| row.group == 4011).unwrap().clone();
    set.id = id;
    set.group = id;
    set.condition_ids = vec![id];
    master.skill_condition_sets.push(set);
    id
}

fn compatible_laws(a: &LuckDpCertifiedResult, b: &LuckDpCertifiedResult) {
    assert_eq!(a.probes, b.probes);
    assert_eq!(a.probe_transitions, b.probe_transitions);
    assert!(a.range_moments.is_empty() && b.range_moments.is_empty());
    for time in a.steps.iter().chain(&b.steps).map(|step| step.0) {
        let left = a.steps[a.steps.partition_point(|step| step.0 <= time) - 1].1;
        let right = b.steps[b.steps.partition_point(|step| step.0 <= time) - 1].1;
        for (left, right) in left.into_iter().zip(right) {
            assert!(left.interval().intersect(right.interval()).is_some(), "time={time}, {left:?}, {right:?}");
        }
    }
}

#[test]
fn compiled_luck_minimum_basis_three_ranges_and_interleaved_miss_match_original_law() {
    let (mut master, original_notes, mut params, _, _, _) = fixture(0, 60);
    branching_master(&mut master);
    let condition = probability_condition(&mut master, 25);
    let guarantee = master.gekisou_support_skill_effects.iter_mut().find(|row| row.skill_id == 66).unwrap();
    guarantee.effect_value = 3; // minimum Super Hit, with native binary32 probability 0.5.
    let mut other = guarantee.clone();
    other.id = 67;
    other.skill_id = 67;
    other.effect_value = 2;
    other.skill_condition_group = condition;
    master.gekisou_support_skill_effects.push(other);
    let mut skill = master.gekisou_support_skills.iter().find(|row| row.id == 66).unwrap().clone();
    skill.id = 67;
    master.gekisou_support_skills.push(skill);
    master.reindex().unwrap();
    let mut notes = Vec::new();
    for offset in [0, 1000, 2000] {
        for note in &original_notes {
            let mut note = *note;
            note.time_ms += offset;
            note.note_id = notes.len() as i32;
            notes.push(note);
        }
    }
    params.music_length_ms = 3000;
    params.converted_note_count = notes.len() as i32;
    let setup = GekisouSetup { fevers: vec![(100, 300), (1100, 1300), (2100, 2300)], missions: vec![2, 2, 2] };
    let mut frames: Vec<_> = (0..=30).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
    for note in &notes {
        frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
            note_id: note.note_id,
            judgement: 5,
            judgement_time_ms: note.time_ms,
        });
    }
    let delta = vec![0.1; frames.len()];
    let play = LivePlay { frames, base_seed: 0 };
    let skills = luck_skills(&master).unwrap();
    // StartGauge and a native timed speed stay in the tape. A MissGauge row separates the two
    // support minimum actions in original holder order, so this is also a commutation regression.
    let deck = [
        Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(66, 1), (96, 1)], ..Default::default() },
        Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(67, 1), (31, 1)], ..Default::default() },
    ];
    let compile = || {
        compile_luck_program(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None, false)
            .unwrap()
    };
    let original = compile();
    let reference = original.certified().unwrap();
    assert!(matches!(compile().start_minimum_basis(26), Err(Error::Capacity(_))));
    let mut basis = original.start_minimum_basis(64).unwrap();
    assert_eq!(basis.start_count(), 3);
    assert_eq!(basis.minimum_action_count(), 6);
    assert_eq!(basis.term_count(), 27);
    assert_eq!(basis.operator_contract(), "whole-live-start-minimum-basis/1");
    assert!(basis.allocated_bytes().unwrap() > std::mem::size_of_val(&basis));
    let _ = take_luck_record_profile();
    let terms: Vec<_> = (0..basis.term_count()).map(|index| basis.certified_term(index).unwrap()).collect();
    let mixed = basis.reconstruct(&terms.iter().collect::<Vec<_>>()).unwrap();
    assert_eq!(take_luck_record_profile().calls, 0, "conditioning must never repeat native admission/recording");
    compatible_laws(&reference, &mixed);
    assert_eq!(mixed.transitions, terms.iter().map(|term| term.curve().transitions).sum::<u64>());
    assert_eq!(mixed.peak_states, terms.iter().map(|term| term.curve().peak_states).max().unwrap());
    assert!(basis.term_weight(27).is_err());
    assert!(basis.term_identity_words(27).is_err());
    assert!(basis.reconstruct(&terms[..26].iter().collect::<Vec<_>>()).is_err());
    let mut swapped = terms.iter().collect::<Vec<_>>();
    swapped.swap(0, 1);
    assert!(matches!(basis.reconstruct(&swapped), Err(Error::Input(_))));
}

fn two_probe_master(master: &mut Master) {
    let mut neutral = master.gekisou_skills[0].clone();
    neutral.id = 4;
    master.gekisou_skills.push(neutral);
    let mut row = master.gekisou_skill_effects[0].clone();
    row.id = 400;
    row.skill_id = 4;
    row.skill_effect_type = 2000;
    master.gekisou_skill_effects.push(row);
    let mut probe = master.gekisou_skills[0].clone();
    probe.id = 5;
    master.gekisou_skills.push(probe);
    let mut row = master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 31).unwrap().clone();
    row.id = 500;
    row.skill_id = 5;
    master.gekisou_skill_effects.push(row);
    master.reindex().unwrap();
}

#[test]
fn compiled_luck_minimum_basis_reuses_two_native_probe_batches_across_source_multiplicity() {
    let (mut master, notes, params, setup, play, delta) = fixture(0, 60);
    branching_master(&mut master);
    two_probe_master(&mut master);
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 2, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    let mut single: Vec<_> = (0..4).map(|position| (writer, position)).collect();
    single.push((minimum, 0));
    let mut doubled = single.clone();
    doubled.push((minimum, 1));
    let compile = |entries: &[(LuckSkillKey, usize)]| {
        luck_table_program(&master, &skills, neutral, &notes, params, &setup, &play, &delta, entries).unwrap()
    };
    let a = compile(&single);
    let b = compile(&doubled);
    assert_ne!(a.identity(), b.identity());
    let expected_a = a.certified().unwrap();
    let expected_b = b.certified().unwrap();
    let mut a = a.start_minimum_basis(64).unwrap();
    let mut b = b.start_minimum_basis(64).unwrap();
    assert_eq!((a.term_count(), b.term_count()), (2, 2));
    assert_ne!(a.term_weight(0).unwrap(), b.term_weight(0).unwrap());
    let _ = take_luck_record_profile();
    let mut responses = Vec::new();
    for index in 0..2 {
        let identity = a.term_identity(index).unwrap();
        assert_eq!(identity, b.term_identity(index).unwrap());
        assert_eq!(identity.batches.len(), 2);
        assert!(identity.source_version.ends_with(crate::SOURCE_SHA256));
        let response = a.certified_term(index).unwrap();
        assert_eq!(response.identity(), &identity);
        assert_eq!(response.curve().probes, vec![true, true]);
        assert!(response.allocated_bytes().unwrap() > std::mem::size_of_val(&response));
        responses.push(response);
    }
    let refs: Vec<_> = responses.iter().collect();
    compatible_laws(&expected_a, &a.reconstruct(&refs).unwrap());
    compatible_laws(&expected_b, &b.reconstruct(&refs).unwrap());
    assert_eq!(take_luck_record_profile().calls, 0);
    assert!(a.allocated_bytes().unwrap() > std::mem::size_of_val(&a));
    assert!(matches!(b.reconstruct(&[&responses[1], &responses[0]]), Err(Error::Input(_))));
}

#[test]
fn compiled_luck_minimum_basis_base_is_one_term_and_refuses_moments_or_other_quotient() {
    let (master, notes, params, setup, play, delta) = fixture(3, 60);
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let compile = |moments| {
        compile_luck_program(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None, moments)
            .unwrap()
    };
    assert!(matches!(compile(true).start_minimum_basis(64), Err(Error::Unsupported(_))));
    assert!(matches!(compile(false).canonicalize_start_minimum().start_minimum_basis(64), Err(Error::Unsupported(_))));
    assert!(matches!(compile(false).start_minimum_basis(0), Err(Error::Capacity(_))));
    let original = compile(false);
    let expected = original.certified().unwrap();
    let mut basis = original.start_minimum_basis(1).unwrap();
    assert_eq!(basis.term_count(), 1);
    assert_eq!(basis.minimum_action_count(), 0);
    assert_eq!(basis.term_choices(0).unwrap(), [0]);
    assert_eq!(basis.term_weight(0).unwrap(), ProbabilityMass::ONE);
    let response = basis.certified_term(0).unwrap();
    compatible_laws(&expected, &basis.reconstruct(&[&response]).unwrap());
}
