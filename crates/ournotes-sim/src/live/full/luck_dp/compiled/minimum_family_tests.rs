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
fn minimum_family_rejects_mixed_updaters_and_nonminimum_rows_after_a_minimum() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let recorder = MinimumFamilyRecorder::new(&master, &skills, &notes, params, &setup, &play, &deltas);
    let mut interleaved = deck();
    interleaved[0].gekisou_support_skills.push((96, 1));
    let mut prepared = recorder.prepare(&interleaved, None, false).unwrap();
    assert!(prepared.take_admission().is_none());
    assert!(recorder.record(prepared).is_ok(), "optional family refusal preserves full native fallback");

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
