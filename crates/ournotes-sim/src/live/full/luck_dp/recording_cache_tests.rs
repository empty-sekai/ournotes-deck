use super::*;

#[test]
fn stopped_or_refused_recordings_never_enter_the_delta_cache() {
    let (master, notes, params, setup, play, delta) = random_fixture();
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let mut complete_checks = 0usize;
    let mut curves = LuckDpCache::new(1 << 20);
    let mut recordings = RecordingCache::default();
    let expected = curves
        .certified_cancellable(
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
            Some(&mut recordings),
            &mut || {
                complete_checks += 1;
                false
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(recordings.storage.retained().0, 1);
    assert!(complete_checks >= 4);
    for stop_at in [1, 2, complete_checks / 2, complete_checks] {
        let mut curves = LuckDpCache::new(1 << 20);
        let mut recordings = RecordingCache::default();
        let mut checks = 0usize;
        let stopped = curves
            .certified_cancellable(
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
                Some(&mut recordings),
                &mut || {
                    checks += 1;
                    checks == stop_at
                },
            )
            .unwrap();
        assert!(stopped.is_none(), "cancellation check {stop_at}");
        assert_eq!(recordings.storage.retained(), (0, 0));
        let resumed = curves
            .certified_cancellable(
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
                Some(&mut recordings),
                &mut || false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(curve_words(&resumed), curve_words(&expected));
        assert_eq!(recordings.storage.retained().0, 1);
    }
    let hit_count = curves.stats().recording_hits;
    assert!(
        curves
            .certified_cancellable(
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
                Some(&mut recordings),
                &mut || true,
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(curves.stats().recording_hits, hit_count);
    let mut disabled = LuckDpCache::new(0);
    let independent = disabled
        .certified_cancellable(
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
            Some(&mut recordings),
            &mut || false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(curve_words(&independent), curve_words(&expected));
    assert_eq!(recordings.storage.retained(), (0, 0));
    assert_eq!(disabled.stats().recording_lookups, 0);

    let mut invalid_play = play.clone();
    invalid_play.frames.iter_mut().find(|frame| !frame.judged.is_empty()).unwrap().judged[0].judgement_time_ms += 1;
    let mut invalid_curves = LuckDpCache::new(1 << 20);
    let mut invalid_recordings = RecordingCache::default();
    assert!(
        invalid_curves
            .certified_cancellable(
                &master,
                &skills,
                &notes,
                &[],
                params,
                &setup,
                &invalid_play,
                &delta,
                &deck,
                None,
                None,
                Some(&mut invalid_recordings),
                &mut || false,
            )
            .is_err()
    );
    assert_eq!(invalid_recordings.storage.retained(), (0, 0));
}

#[test]
fn complete_native_recording_family_reuses_every_original_key() {
    let (mut master, notes, params, setup, play, delta) = random_fixture();
    let template = master.gekisou_skill_effects.iter().find(|row| row.id == 1).unwrap().clone();
    let performers: Vec<_> = (0..5i64)
        .map(|slot| {
            let id = 501 + slot;
            master.gekisou_skills.push(serde_json::from_value(json!({"_id":id,"_gekisouMissionType":2})).unwrap());
            for variant in 0..3i64 {
                let mut row = template.clone();
                row.id = 5_010 + slot * 10 + variant;
                row.skill_id = id;
                row.effect_value = 333 + slot * 111 + variant * 7;
                master.gekisou_skill_effects.push(row);
            }
            Performer {
                character_id: slot + 1,
                gekisou_skill: Some((id, 1)),
                gekisou_support_skills: if slot == 0 { vec![(31, 1)] } else { Vec::new() },
                ..Default::default()
            }
        })
        .collect();
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    fn extend(prefix: &mut Vec<usize>, used: u8, out: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            out.push(prefix.as_slice().try_into().unwrap());
        } else {
            for slot in 0..5 {
                if used & (1 << slot) == 0 {
                    prefix.push(slot);
                    extend(prefix, used | (1 << slot), out);
                    prefix.pop();
                }
            }
        }
    }
    let mut orders = Vec::new();
    extend(&mut Vec::new(), 0, &mut orders);
    let decks: Vec<Vec<_>> =
        orders.iter().map(|order| order.iter().map(|&slot| performers[slot].clone()).collect()).collect();
    let mut raw_keys = crate::num::FxHashSet::default();
    let mut expected = Vec::new();
    let mut curves = LuckDpCache::new(1 << 20);
    let mut recordings = RecordingCache::default();
    for deck in &decks {
        let prepared = prepare_recording::<ProbabilityMass>(
            &master,
            &skills,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            deck,
            None,
            None,
        )
        .unwrap();
        assert!(raw_keys.insert(RecordingCache::key(&prepared)), "the recording identities must be distinct");
        let direct = luck_rush_dp_certified_with_ranking(
            &master,
            &skills,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta,
            deck,
            None,
            None,
        )
        .unwrap();
        let cached = curves
            .certified_cancellable(
                &master,
                &skills,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                deck,
                None,
                None,
                Some(&mut recordings),
                &mut || false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(curve_words(&cached), curve_words(&direct));
        expected.push(curve_words(&direct));
    }
    assert_eq!(raw_keys.len(), 120);
    assert!(raw_keys.iter().map(Vec::len).sum::<usize>() > 1 << 20);
    assert_eq!(recordings.storage.retained().0, 120);
    assert!(recordings.storage.retained().1 <= 1 << 20);
    let before = curves.stats();
    for (deck, expected) in decks.iter().zip(expected) {
        let cached = curves
            .certified_cancellable(
                &master,
                &skills,
                &notes,
                &[],
                params,
                &setup,
                &play,
                &delta,
                deck,
                None,
                None,
                Some(&mut recordings),
                &mut || false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(curve_words(&cached), expected);
    }
    let after = curves.stats();
    assert_eq!(after.recording_hits - before.recording_hits, 120);
    assert_eq!(after.recording_peak_entries, 120);
    assert_eq!(after.recording_peak_bytes, recordings.storage.retained().1);
    assert_eq!(after.propagated_curves, before.propagated_curves);
    assert_eq!(after.transitions, before.transitions);
}
