use super::tests::fixture;
use super::*;
use serde_json::json;

type OwnerOrderFixture = (Master, Vec<Performer>, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>);

/// One owner's ordinary elapsed-time row lies between two sustained direct probes in effect-key order.
fn owner_order_fixture() -> OwnerOrderFixture {
    let (mut master, _, mut params, _, _, _) = fixture();
    master.gekisou_luck_base_points[0].base_point = 140;
    master.gekisou_skills.push(serde_json::from_value(json!({"_id": 1, "_gekisouMissionType": 2})).unwrap());
    for (id, kind, values) in [(7021, 7021, vec![]), (4000, 4000, vec![1])] {
        master.skill_conditions.push(
            serde_json::from_value(json!({
                "_id": id, "_conditionType": kind, "_conditionValues": values,
                "_conditionTargetIDs": [], "_isPositive": true,
            }))
            .unwrap(),
        );
        master
            .skill_condition_sets
            .push(serde_json::from_value(json!({"_id": id, "_group": id, "_conditionIds": [id]})).unwrap());
    }
    master
        .skill_effect_settings
        .push(serde_json::from_value(json!({"_id": 1, "_skillEffectType": 2000, "_phase": 1})).unwrap());
    for (id, trigger, value) in [(1, 7021, 1), (2, 4000, 3000), (3, 7021, 1)] {
        master.gekisou_skill_effects.push(
            serde_json::from_value(json!({
                "_id": id, "_gekisouSkillID": 1, "_level": 1, "_skillTriggerType": 2,
                "_skillTriggerConditionGroup": trigger, "_skillConditionGroup": 0,
                "_skillReleaseConditionGroup": 0, "_skillTargetIDs": [], "_skillEffectType": 2000,
                "_activationTimeSecond": 0.0, "_effectValue": value, "_maxEffectValue": 0,
                "_effectLimitCount": 1, "_skillCumulativeConditionID": 0,
                "_effectExecuteLimitCount": 0, "_effectExecuteLimitResetConditionGroup": 0,
            }))
            .unwrap(),
        );
    }
    master.reindex().unwrap();
    let notes: Vec<_> = [900, 1000]
        .into_iter()
        .enumerate()
        .map(|(id, time_ms)| LiveNote { note_id: id as i32, note_operate_type: 1, judgement_type: 1, time_ms })
        .collect();
    params.music_length_ms = 4000;
    params.total_power = 1894;
    params.converted_note_count = 2;
    let setup = GekisouSetup { fevers: vec![(100, 2000)], missions: vec![2, 2, 2] };
    let frames = (0..=40)
        .map(|i| PlayFrame {
            time_ms: i * 100,
            judged: notes
                .iter()
                .filter(|note| note.time_ms == i * 100)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    (
        master,
        vec![Performer { gekisou_skill: Some((1, 1)), ..Default::default() }],
        notes,
        params,
        setup,
        LivePlay { frames, base_seed: 0 },
        vec![0.1; 41],
    )
}

#[test]
fn native_same_owner_filing_interleaves_ordinary_commands_and_direct_probes() {
    let (master, deck, notes, params, setup, play, delta) = owner_order_fixture();
    let skills = luck_skills(&master).unwrap();
    let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    check_recorder(&native, &skills).unwrap();
    let curve = luck_rush_dp_certified_with_ranking(
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
    assert!(curve.probes.iter().any(|&held| held));
    assert!(
        curve
            .steps
            .iter()
            .any(|(_, masses)| { masses[1].interval().lower() > 0.0 || masses[3].interval().lower() > 0.0 })
    );
    let probes = probes_in_native_order(&native, &skills).unwrap();
    assert_eq!(probes.len(), 2);
    assert_eq!(probes[0].owner, probes[1].owner);
    native.score.begin_bounds(probes, true);
    for (frame, &dt) in play.frames.iter().zip(&delta).take(11) {
        native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
    }
    let filed: Vec<_> = native
        .score
        .bounds_trace
        .as_ref()
        .unwrap()
        .events
        .iter()
        .filter_map(|event| match event {
            BoundsEvent::Factor { command, .. } if command.time_ms == 1000 && command.note_mill != 0 => {
                Some((command.owner_id, command.note_mill))
            }
            _ => None,
        })
        .collect();
    assert_eq!(filed, [(1, 10), (1, 30000), (1, 10)]);
    assert_eq!(native.factor_state().note_score_up.to_bits(), 0x3fa6_6cf5);
    let probes_first = ((1.0f32 + 0.0001f32) + 0.0001f32) + 0.3f32;
    let command_first = ((1.0f32 + 0.3f32) + 0.0001f32) + 0.0001f32;
    assert_eq!(probes_first.to_bits(), 0x3fa6_6cf4);
    assert_eq!(command_first.to_bits(), 0x3fa6_6cf4);
    assert!(native.factor_state().note_score_up > probes_first.max(command_first));
}

#[test]
fn score_support_encloses_a_native_same_owner_interleaved_query() {
    let (master, deck, notes, params, setup, play, delta) = owner_order_fixture();
    let bounds = luck_score_bounds(&master, &deck, &notes, &[], params, &setup, &play, &delta).unwrap();
    let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    native.score.begin_bounds(Vec::new(), true);
    for (frame, &dt) in play.frames.iter().zip(&delta).take(11) {
        native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
    }
    let query = native.score.bounds_last_query().unwrap();
    let support = bounds.queries[query].support;
    assert_eq!(query, 21);
    assert_eq!(native.score(), 7727);
    assert!(
        support.lower <= native.score() && native.score() <= support.upper,
        "native={} support={support:?} query={query} note_factor_bits={:08x}",
        native.score(),
        native.factor_state().note_score_up.to_bits(),
    );
}

#[test]
fn a_capacity_refusal_does_not_cache_a_partial_score_certificate() {
    let (mut master, deck, notes, params, setup, play, delta) = owner_order_fixture();
    let probe = master.gekisou_skill_effects[0].clone();
    let ordinary = master.gekisou_skill_effects[1].clone();
    master.gekisou_skill_effects.clear();
    for id in 1..=20 {
        let mut row = if id % 2 == 1 { probe.clone() } else { ordinary.clone() };
        row.id = id;
        master.gekisou_skill_effects.push(row);
    }
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let mut curves = LuckDpCache::new(1 << 20);
    let mut session = LuckScoreSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    for _ in 0..2 {
        assert!(matches!(session.summary(&deck, Some(&mut curves), || false), Err(Error::Capacity(_))));
    }
    let stats = curves.stats();
    assert!(stats.hits + stats.recording_hits > 0);
}

#[test]
fn probe_order_follows_native_phases_and_wrapping_signed_updater_keys() {
    for second_phase in [1, 2] {
        let (mut master, deck, notes, params, setup, play, delta) = owner_order_fixture();
        let row = master.gekisou_skill_effects.iter_mut().find(|row| row.id == 3).unwrap();
        row.id = i64::MAX / 100 + 1;
        row.effect_value = 3;
        if second_phase == 2 {
            row.skill_effect_type = 2005;
        }
        master
            .skill_effect_settings
            .push(serde_json::from_value(json!({"_id": 2, "_skillEffectType": 2005, "_phase": second_phase})).unwrap());
        master.reindex().unwrap();
        let skills = luck_skills(&master).unwrap();
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        check_recorder(&native, &skills).unwrap();
        let probes = probes_in_native_order(&native, &skills).unwrap();
        assert_eq!(probes.len(), 2);
        let expected: Vec<_> = probes.iter().map(|row| row.value.to_bits()).collect();
        native.score.begin_bounds(probes, true);
        for (frame, &dt) in play.frames.iter().zip(&delta).take(11) {
            native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        }
        let actual: Vec<_> = native
            .score
            .bounds_trace
            .as_ref()
            .unwrap()
            .events
            .iter()
            .filter_map(|event| match event {
                BoundsEvent::Factor { command, .. }
                    if command.time_ms == 1000 && command.note_mill != 0 && command.note_mill != 30000 =>
                {
                    Some((command.note_mill as f32 / 100000f32).to_bits())
                }
                _ => None,
            })
            .collect();
        assert_eq!(actual, expected, "phase={second_phase}");
    }
}
