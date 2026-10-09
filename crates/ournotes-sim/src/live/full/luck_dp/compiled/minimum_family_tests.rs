use super::*;
use crate::chartstats::{LuckTableMinimumFamilySession, luck_neutral, luck_table_program, luck_table_program_virtual};
use crate::live::full::luck_dp::tests::fixture;

fn master() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let (mut master, notes, params, setup, play, deltas) = fixture(0, 60);
    master.skill_conditions.iter_mut().find(|condition| condition.id == 4011).unwrap().condition_values = vec![50];
    // A real constructor needs a neutral main for the separate physical score-probe validation.
    let mut neutral = master.gekisou_skills[0].clone();
    neutral.id = 4;
    master.gekisou_skills.push(neutral);
    let mut row = master.gekisou_skill_effects[0].clone();
    row.id = 400;
    row.skill_id = 4;
    row.skill_effect_type = 2000;
    master.gekisou_skill_effects.push(row);
    let mut minimum = master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 66).unwrap().clone();
    minimum.id = 166;
    minimum.level = 2;
    minimum.effect_value = 3;
    master.gekisou_support_skill_effects.push(minimum);
    master.reindex().unwrap();
    (master, notes, params, setup, play, deltas)
}

fn deck() -> Vec<Performer> {
    let mut deck = vec![Performer { gekisou_skill: Some((1, 1)), ..Default::default() }; 5];
    deck[0].gekisou_support_skills = vec![(66, 1)];
    deck
}

#[test]
fn minimum_family_reweights_native_levels_and_multiplicity_without_changing_conditioned_programs() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let recorder = MinimumFamilyRecorder::new(&master, &skills, &notes, params, &setup, &play, &deltas);
    let first = deck();
    let mut second = first.clone();
    second[0].gekisou_support_skills = vec![(66, 2)];
    second[1].gekisou_support_skills = vec![(66, 1)];
    let mut a = recorder.prepare(&first, None, false).unwrap();
    let a_admission = a.take_admission().expect("minimum-only source suffix");
    let mut b = recorder.prepare(&second, None, false).unwrap();
    let b_admission = b.take_admission().expect("minimum-only source suffix");
    assert_eq!(a_admission.key(), b_admission.key());
    let mut a = recorder.record(a).unwrap().start_minimum_basis(64).unwrap();
    let mut b = recorder.record(b).unwrap().start_minimum_basis(64).unwrap();
    assert!(a_admission.matches_basis(&a, 64).unwrap());
    assert!(b_admission.matches_basis(&b, 64).unwrap());
    assert_ne!(a.term_count(), b.term_count());
    b_admission.reweight(&mut a, 64).unwrap();
    assert_eq!(a.term_count(), b.term_count());
    for term in 0..a.term_count() {
        assert_eq!(a.term_weight(term).unwrap(), b.term_weight(term).unwrap());
        assert_eq!(a.term_choices(term).unwrap(), b.term_choices(term).unwrap());
        assert_eq!(a.term_identity_words(term).unwrap(), b.term_identity_words(term).unwrap());
    }
    let mut changed = second;
    changed[2].gekisou_skill = Some((2, 1));
    let changed = recorder.prepare(&changed, None, false).unwrap().take_admission().unwrap();
    assert_ne!(a_admission.key(), changed.key(), "native non-minimum controller must remain exact");
}

#[test]
fn minimum_family_interleaved_rows_preserve_native_indices_and_complete_original_models() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let recorder = MinimumFamilyRecorder::new(&master, &skills, &notes, params, &setup, &play, &deltas);
    let mut interleaved = deck();
    interleaved[0].gekisou_support_skills.push((96, 1));
    let mut prepared = recorder.prepare(&interleaved, None, false).unwrap();
    let original = super::super::super::super::luck_exact::initialized_identity(&mut prepared.prepared.model).unwrap();
    let first = prepared.take_admission().expect("inert minimum slot before an exact Miss row");
    let repeated = admit(&mut prepared.prepared, false).unwrap();
    assert_eq!(first.key(), repeated.key());
    assert_eq!(
        original,
        super::super::super::super::luck_exact::initialized_identity(&mut prepared.prepared.model).unwrap()
    );
    let mut changed_level = interleaved.clone();
    changed_level[0].gekisou_support_skills[0].1 = 2;
    let mut changed = recorder.prepare(&changed_level, None, false).unwrap();
    let second = changed.take_admission().unwrap();
    assert_eq!(first.key(), second.key(), "minimum levels preserve the retained native row coordinates");
    let mut reused = recorder.record(prepared).unwrap().start_minimum_basis(64).unwrap();
    let mut fresh = recorder.record(changed).unwrap().start_minimum_basis(64).unwrap();
    second.reweight(&mut reused, 64).unwrap();
    for term in 0..fresh.term_count() {
        assert_eq!(reused.term_identity_words(term).unwrap(), fresh.term_identity_words(term).unwrap());
        assert_eq!(reused.term_weight(term).unwrap(), fresh.term_weight(term).unwrap());
    }
    let mut moved = interleaved.clone();
    moved[0].gekisou_support_skills.swap(0, 1);
    let moved = recorder.prepare(&moved, None, false).unwrap().take_admission().unwrap();
    assert_ne!(first.key(), moved.key(), "retained Miss row coordinates remain part of admission");
    let mut inserted = interleaved;
    inserted[0].gekisou_support_skills.pop();
    inserted[1].gekisou_support_skills = vec![(66, 2), (96, 1)];
    let inserted = recorder.prepare(&inserted, None, false).unwrap().take_admission().unwrap();
    assert_ne!(first.key(), inserted.key(), "additional minimum rows cannot shift a retained Miss row silently");
}

#[test]
fn minimum_family_rejects_mixed_updaters() {
    let (master, notes, params, setup, play, deltas) = master();
    let mut mixed = master.clone();
    let mut extra = mixed.gekisou_skill_effects.iter().find(|row| row.skill_id == 1).unwrap().clone();
    extra.id = 65; // Non-minimum row precedes 66, so the whole-updater guard is exercised separately.
    extra.skill_id = 66;
    mixed.gekisou_support_skill_effects.push(extra);
    mixed.reindex().unwrap();
    let mixed_skills = luck_skills(&mixed).unwrap();
    let recorder = MinimumFamilyRecorder::new(&mixed, &mixed_skills, &notes, params, &setup, &play, &deltas);
    let mut prepared = recorder.prepare(&deck(), None, false).unwrap();
    assert!(prepared.take_admission().is_none());
    assert!(recorder.record(prepared).is_ok());
}

fn assert_joint_laws(a: &LuckDpCertifiedResult, b: &LuckDpCertifiedResult) {
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
fn minimum_family_canonical_miss_reuses_interleaved_levels_with_fresh_identity_and_dp_agreement() {
    let (mut master, notes, params, setup, play, deltas) = master();
    // Both positive and failed lots exercise the Miss writer while the conditional minimum varies.
    let originals = master.gekisou_luck_bonus_lots.clone();
    for result in 1..=3 {
        master.gekisou_luck_bonus_lots.extend(originals.iter().cloned().map(|mut row| {
            row.id += result * 100;
            row.lot_result = result;
            row
        }));
    }
    let miss = master.gekisou_support_skill_effects.iter_mut().find(|row| row.skill_id == 96).unwrap();
    miss.effect_value = 500;
    let mut second_miss = miss.clone();
    second_miss.id = 196;
    second_miss.level = 2;
    second_miss.effect_value = 1000;
    master.gekisou_support_skill_effects.push(second_miss);
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    let miss = LuckSkillKey { source: LuckSource::GekisouSupport, id: 96, level: 1, matched: None };
    let mut entries: Vec<_> = (0..5).map(|position| (writer, position)).collect();
    entries.extend([(minimum, 0), (miss, 0), (minimum, 1), (miss, 1)]);
    let compile = |entries: &[(LuckSkillKey, usize)]| {
        luck_table_program_virtual(&master, &skills, neutral, &notes, params, &setup, &play, &deltas, entries).unwrap()
    };
    let mut session = LuckTableMinimumFamilySession::new(
        &master,
        &skills,
        neutral,
        &notes,
        params,
        &setup,
        &play,
        &deltas,
        64,
        32 << 20,
        32 << 20,
    )
    .unwrap()
    .with_canonical_miss_gauge()
    .unwrap();
    for levels in [[1, 1], [2, 1], [1, 2], [2, 2], [1, 1]] {
        entries[5].0.level = levels[0];
        entries[7].0.level = levels[1];
        let original = compile(&entries);
        let reference = original.certified().unwrap();
        let mut fresh = original.canonicalize_miss_gauge().unwrap().start_minimum_basis(64).unwrap();
        let (family, reused) = session.basis(&entries).unwrap();
        assert_eq!(family, Some(0));
        assert_eq!(reused.operator_contract(), "whole-live-start-minimum-basis+canonical-miss-gauge-deltas/1");
        assert_eq!(reused.term_count(), fresh.term_count());
        let mut responses = Vec::new();
        for term in 0..fresh.term_count() {
            assert_eq!(reused.term_identity(term).unwrap(), fresh.term_identity(term).unwrap());
            assert_eq!(reused.term_choices(term).unwrap(), fresh.term_choices(term).unwrap());
            assert_eq!(reused.term_weight(term).unwrap(), fresh.term_weight(term).unwrap());
            responses.push(reused.certified_term(term).unwrap());
        }
        let responses = responses.iter().collect::<Vec<_>>();
        assert_joint_laws(&reference, &reused.reconstruct(&responses).unwrap());
        assert_joint_laws(&reference, &fresh.reconstruct(&responses).unwrap());
    }
    assert_eq!((session.stats().native_recordings, session.stats().family_hits), (1, 4));
    entries[6].0.level = 2;
    assert_eq!(session.basis(&entries).unwrap().0, Some(1), "changed original Miss controls form a new family");
    entries[0].0.id = 2;
    assert_eq!(session.basis(&entries).unwrap().0, Some(2), "changed nonminimum main actions remain exact");
    assert!(session.with_canonical_miss_gauge().is_err(), "identity mode cannot change an established family id");
}

#[test]
fn minimum_family_canonical_miss_keeps_its_identity_mode_and_bounded_fallback() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    let miss = LuckSkillKey { source: LuckSource::GekisouSupport, id: 96, level: 1, matched: None };
    let mut entries: Vec<_> = (0..5).map(|position| (writer, position)).collect();
    entries.extend([(minimum, 0), (miss, 0)]);
    let session = |capacity, program_capacity| {
        LuckTableMinimumFamilySession::new(
            &master,
            &skills,
            neutral,
            &notes,
            params,
            &setup,
            &play,
            &deltas,
            64,
            program_capacity,
            capacity,
        )
        .unwrap()
    };
    let mut original = session(32 << 20, 32 << 20);
    let original = original.basis(&entries).unwrap().1.term_identity(0).unwrap();
    let mut shared = session(32 << 20, 32 << 20).with_canonical_miss_gauge().unwrap();
    let empty_bytes = shared.allocated_bytes().unwrap();
    let mut fresh =
        luck_table_program_virtual(&master, &skills, neutral, &notes, params, &setup, &play, &deltas, &entries)
            .unwrap()
            .canonicalize_miss_gauge()
            .unwrap()
            .start_minimum_basis(64)
            .unwrap();
    let expected: Vec<_> = (0..fresh.term_count()).map(|term| fresh.term_identity(term).unwrap()).collect();
    let (_, shared) = shared.basis(&entries).unwrap();
    assert_ne!(original, shared.term_identity(0).unwrap());
    for (term, identity) in expected.iter().enumerate() {
        assert_eq!(identity, &shared.term_identity(term).unwrap());
    }
    let mut bounded = session(empty_bytes, 32 << 20).with_canonical_miss_gauge().unwrap();
    for _ in 0..2 {
        let (family, basis) = bounded.basis(&entries).unwrap();
        assert_eq!(family, None);
        assert_eq!(basis.operator_contract(), "whole-live-start-minimum-basis+canonical-miss-gauge-deltas/1");
        for (term, identity) in expected.iter().enumerate() {
            assert_eq!(identity, &basis.term_identity(term).unwrap());
            assert_eq!(fresh.term_weight(term).unwrap(), basis.term_weight(term).unwrap());
        }
        assert!(bounded.allocated_bytes().unwrap() <= empty_bytes);
    }
    assert_eq!((bounded.stats().native_recordings, bounded.stats().family_fallbacks), (2, 2));
    assert_eq!(bounded.stats().retained_families, 0);
    let mut no_program = session(32 << 20, 0).with_canonical_miss_gauge().unwrap();
    assert!(matches!(no_program.basis(&entries), Err(Error::Capacity(_))));
    assert_eq!(no_program.stats().retained_families, 0);
}

#[test]
fn minimum_family_canonical_miss_rejects_unadmitted_gauge_domains_before_retaining_a_family() {
    let (mut master, notes, params, setup, play, deltas) = master();
    master.gekisou_support_skill_effects.iter_mut().find(|row| row.skill_id == 96).unwrap().effect_value = -1;
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    let miss = LuckSkillKey { source: LuckSource::GekisouSupport, id: 96, level: 1, matched: None };
    let mut entries: Vec<_> = (0..5).map(|position| (writer, position)).collect();
    entries.extend([(minimum, 0), (miss, 0)]);
    assert!(matches!(
        luck_table_program_virtual(&master, &skills, neutral, &notes, params, &setup, &play, &deltas, &entries)
            .unwrap()
            .canonicalize_miss_gauge(),
        Err(Error::Unsupported(_))
    ));
    let mut session = LuckTableMinimumFamilySession::new(
        &master,
        &skills,
        neutral,
        &notes,
        params,
        &setup,
        &play,
        &deltas,
        64,
        32 << 20,
        32 << 20,
    )
    .unwrap()
    .with_canonical_miss_gauge()
    .unwrap();
    for _ in 0..2 {
        assert!(matches!(session.basis(&entries), Err(Error::Unsupported(_))));
    }
    assert_eq!(session.stats().retained_families, 0);
    assert_eq!((session.stats().native_recordings, session.stats().family_hits), (2, 0));
}

#[test]
fn minimum_family_keeps_original_admission_and_refuses_executing_or_life_dependent_minima() {
    let (mut master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let recorder = MinimumFamilyRecorder::new(&master, &skills, &notes, params, &setup, &play, &deltas);
    let mut duplicates = deck();
    duplicates[0].gekisou_support_skills.push((66, 1));
    assert!(
        recorder.prepare(&duplicates, None, false).is_err(),
        "minimum projection cannot hide native state collisions"
    );
    let mut invalid = deck();
    invalid[0].gekisou_support_skills = vec![(999_999, 1)];
    assert!(recorder.prepare(&invalid, None, false).is_err());
    let mut prepared = recorder.prepare(&deck(), None, false).unwrap();
    let (_, _, condition) = prepared
        .prepared
        .plan
        .actions
        .iter_mut()
        .find(|(_, action, _)| matches!(action, Action::StartMinimum { .. }))
        .unwrap();
    *condition = Some(Checker::LifeAtLeast(Some(100)));
    assert!(admit(&mut prepared.prepared, false).is_none());
    drop(recorder);

    master
        .gekisou_support_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == 66 && row.level == 1)
        .unwrap()
        .skill_condition_group = 0;
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let recorder = MinimumFamilyRecorder::new(&master, &skills, &notes, params, &setup, &play, &deltas);
    let mut prepared = recorder.prepare(&deck(), None, false).unwrap();
    assert!(prepared.take_admission().is_none(), "a fixed-true minimum applier is not erased");
    assert!(recorder.record(prepared).is_ok());
}

#[test]
fn minimum_family_table_session_reuses_one_recording_and_matches_fresh_native_term_identities() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    let mut first: Vec<_> = (0..5).map(|position| (writer, position)).collect();
    first.push((minimum, 0));
    let mut second = first.clone();
    second.push((LuckSkillKey { level: 2, ..minimum }, 1));
    let mut session = LuckTableMinimumFamilySession::new(
        &master,
        &skills,
        neutral,
        &notes,
        params,
        &setup,
        &play,
        &deltas,
        64,
        32 << 20,
        32 << 20,
    )
    .unwrap();
    assert_eq!(session.basis(&first).unwrap().0, Some(0));
    let (family, reused) = session.basis(&second).unwrap();
    assert_eq!(family, Some(0));
    let mut fresh =
        luck_table_program_virtual(&master, &skills, neutral, &notes, params, &setup, &play, &deltas, &second)
            .unwrap()
            .start_minimum_basis(64)
            .unwrap();
    assert_eq!(reused.term_count(), fresh.term_count());
    for term in 0..fresh.term_count() {
        assert_eq!(reused.term_identity(term).unwrap(), fresh.term_identity(term).unwrap());
        assert_eq!(reused.term_weight(term).unwrap(), fresh.term_weight(term).unwrap());
    }
    let stats = session.stats();
    assert_eq!(
        (stats.family_admission_calls, stats.native_recordings, stats.family_hits, stats.family_misses),
        (2, 1, 1, 1)
    );
    assert_eq!(stats.family_fallbacks, 0);
    assert_eq!(stats.retained_families, 1);
    assert!(stats.family_cache_bytes <= 32 << 20);
    let invalid = [first.as_slice(), &[(minimum, 0)]].concat();
    assert!(session.basis(&invalid).is_err(), "a previously cached family cannot bypass native state admission");
    assert_eq!(session.stats().native_recordings, 1);
}

#[test]
fn minimum_family_keeps_all_native_held_probe_batches_and_falls_back_at_its_cache_limit() {
    let (mut master, notes, params, setup, play, deltas) = master();
    let mut probe = master.gekisou_skills[0].clone();
    probe.id = 5;
    master.gekisou_skills.push(probe);
    let mut row = master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 31).unwrap().clone();
    row.id = 500;
    row.skill_id = 5;
    master.gekisou_skill_effects.push(row);
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    assert_eq!(skills.shapes.len(), 2);
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    // The free position is before the minimum support holders, preserving the admitted row suffix
    // in both probe batches. A single free position needs two independent original native batches.
    let mut first: Vec<_> = (1..5).map(|position| (writer, position)).collect();
    first.push((minimum, 1));
    let mut second = first.clone();
    second.push((LuckSkillKey { level: 2, ..minimum }, 2));
    let session = |capacity| {
        LuckTableMinimumFamilySession::new(
            &master,
            &skills,
            neutral,
            &notes,
            params,
            &setup,
            &play,
            &deltas,
            64,
            32 << 20,
            capacity,
        )
    };
    let mut shared = session(32 << 20).unwrap();
    let empty_bytes = shared.allocated_bytes().unwrap();
    assert_eq!(shared.basis(&first).unwrap().0, Some(0));
    let (family, reused) = shared.basis(&second).unwrap();
    assert_eq!(family, Some(0));
    let mut fresh = luck_table_program(&master, &skills, neutral, &notes, params, &setup, &play, &deltas, &second)
        .unwrap()
        .start_minimum_basis(64)
        .unwrap();
    let fresh_keys: Vec<_> = (0..fresh.term_count())
        .map(|term| {
            let identity = fresh.term_identity(term).unwrap();
            assert_eq!(identity.batches.len(), 2);
            assert_eq!(identity, reused.term_identity(term).unwrap());
            assert_eq!(fresh.term_weight(term).unwrap(), reused.term_weight(term).unwrap());
            identity
        })
        .collect();
    assert_eq!(shared.stats().native_recordings, 1);
    assert_eq!(shared.stats().native_batch_recordings, 2);
    let mut bounded = session(empty_bytes).unwrap();
    for _ in 0..2 {
        let (family, fallback) = bounded.basis(&second).unwrap();
        assert_eq!(family, None, "the empty scope allowance cannot retain this complete family");
        for (term, expected) in fresh_keys.iter().enumerate() {
            assert_eq!(&fallback.term_identity(term).unwrap(), expected);
            assert_eq!(fallback.term_weight(term).unwrap(), fresh.term_weight(term).unwrap());
        }
        assert!(bounded.allocated_bytes().unwrap() <= empty_bytes);
    }
    assert_eq!((bounded.stats().native_recordings, bounded.stats().family_fallbacks), (2, 2));
    assert_eq!(bounded.stats().retained_families, 0);
    assert!(session(0).is_err());
}

#[test]
fn minimum_family_never_retains_a_failed_original_recording_or_an_oversized_program() {
    let (master, notes, params, setup, mut play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let neutral = luck_neutral(&master, &skills);
    let writer = LuckSkillKey { source: LuckSource::Gekisou, id: 1, level: 1, matched: None };
    let minimum = LuckSkillKey { source: LuckSource::GekisouSupport, id: 66, level: 1, matched: None };
    let mut entries: Vec<_> = (0..5).map(|position| (writer, position)).collect();
    entries.push((minimum, 0));
    let mut bounded = LuckTableMinimumFamilySession::new(
        &master,
        &skills,
        neutral,
        &notes,
        params,
        &setup,
        &play,
        &deltas,
        64,
        0,
        32 << 20,
    )
    .unwrap();
    assert!(matches!(bounded.basis(&entries), Err(Error::Capacity(_))));
    assert_eq!(bounded.stats().retained_families, 0);
    drop(bounded);

    play.frames[1].time_ms = play.frames[0].time_ms;
    let mut broken = LuckTableMinimumFamilySession::new(
        &master,
        &skills,
        neutral,
        &notes,
        params,
        &setup,
        &play,
        &deltas,
        64,
        32 << 20,
        32 << 20,
    )
    .unwrap();
    assert!(broken.basis(&entries).is_err());
    assert!(broken.basis(&entries).is_err());
    assert_eq!(broken.stats().retained_families, 0);
    assert_eq!(broken.stats().native_recordings, 2);
}
