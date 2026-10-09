use super::super::super::luck_score_bounds::BoundsEvent;
use super::super::tests::fixture;
use super::*;

#[test]
fn support_chance_endpoints_complements_and_interior_products_are_exact() {
    for value in [0.0, -0.0, f32::from_bits(1), 0.5, f32::from_bits(1f32.to_bits() - 1), 1.0] {
        let value = Support::from_f32(value).unwrap();
        assert_eq!(value.complement().complement(), value);
        assert_eq!(value.multiply(Support::One), value);
        assert_eq!(value.multiply(Support::Zero), Support::Zero);
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(chance::<Support>(&Checker::Probability(value), 0).is_none());
    }
    let partial = Checker::Probability(f32::from_bits(1));
    let both = Checker::And { items: vec![partial.clone(); 1024], resettable: vec![false; 1024] };
    assert_eq!(chance::<Support>(&both, 0), Some(Support::Partial));
    assert_eq!(chance::<f64>(&both, 0), Some(0.0));
    assert_eq!(chance::<Support>(&Checker::Not(Box::new(both)), 0), Some(Support::Partial));
    assert_eq!(chance::<Support>(&Checker::Or(vec![Checker::Fixed(false), partial]), 0), Some(Support::Partial));
    assert_eq!(
        chance::<Support>(&Checker::Or(vec![Checker::Probability(0.5), Checker::Fixed(true)]), 0),
        Some(Support::One)
    );
    assert_eq!(
        chance::<Support>(
            &Checker::And { items: vec![Checker::Probability(0.5), Checker::Fixed(false)], resettable: vec![false; 2] },
            0
        ),
        Some(Support::Zero)
    );
}

#[test]
fn support_integer_weights_keep_tiny_positive_and_merge_equal_outcomes() {
    let weights = positive_weights(vec![(0, u64::MAX, 0), (1, u64::MAX, 1), (u64::MAX - 1, u64::MAX, 1)]).unwrap();
    assert_eq!(weights.len(), 1);
    assert_eq!(weights[0].1, 1);
    assert!(weights[0].0.possible());
    assert_eq!(positive_weights(vec![(1, 1, 3)]).unwrap(), vec![(Support::One, 3)]);
    assert!(positive_weights(vec![(1, 0, 3)]).is_err());
    assert!(positive_weights(vec![(2, 1, 3)]).is_err());
    let mut path = Support::Partial;
    for _ in 0..10000 {
        path = path.multiply(Support::Partial);
    }
    assert!(path.possible() && path.complement().possible());
}

fn compare_support(support: &LuckDpSupportResult, certified: &LuckDpCertifiedResult) {
    assert_eq!(support.probes, certified.probes);
    assert_eq!(support.probe_transitions, certified.probe_transitions);
    assert_eq!(support.rush_filings, certified.rush_filings);
    assert_eq!(support.peak_states, certified.peak_states);
    assert_eq!(support.transitions, certified.transitions);
    let mut expected = Vec::new();
    for (time, weights) in &certified.steps {
        let positive = weights.map(|p| p.interval().upper() > 0.0);
        if expected.last().is_none_or(|last: &(i32, [bool; 4])| last.1 != positive) {
            expected.push((*time, positive));
        }
    }
    assert_eq!(support.steps, expected);
}

#[test]
fn support_complete_curves_match_positive_certified_classes_and_cached_certificates() {
    for weights in [[1, 0, 0, 0], [0, 0, 0, 1], [1, 0, 0, 1], [1, 2, 3, 4]] {
        let (mut master, notes, params, setup, play, delta) = fixture(3, 60);
        master.gekisou_luck_bonus_lots = (0..5)
            .flat_map(|kind| {
                weights.into_iter().enumerate().map(move |(result, weight)| crate::master::LuckBonusLotRow {
                    id: kind * 4 + result as i64 + 1,
                    chance_lot_type: kind,
                    lot_result: result as i64,
                    weight,
                })
            })
            .collect();
        master.reindex().unwrap();
        let skills = luck_skills(&master).unwrap();
        for skill in [1, 2, 3] {
            let deck = [Performer {
                gekisou_skill: Some((skill, 1)),
                gekisou_support_skills: vec![(31, 1), (96, 1)],
                ..Default::default()
            }];
            let support = luck_rush_dp_support_with_ranking(
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
            let certified =
                certified_mode(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None, false)
                    .unwrap();
            compare_support(&support, &certified);
            let mut cache = LuckDpCache::new(1 << 20);
            for _ in 0..2 {
                let cached = cache
                    .certified(&master, &skills, &notes, &[], params, &setup, &play, &delta, &deck, None, None)
                    .unwrap();
                compare_support(&support, &cached);
            }
            assert_eq!(
                support,
                luck_rush_dp_support_with_ranking(
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
                    None
                )
                .unwrap()
            );
        }
    }
}

#[test]
fn support_original_probe_masks_equal_complete_native_positive_path_union() {
    let (mut master, mut notes, mut params, setup, mut play, delta) = fixture(3, 140);
    notes.truncate(2);
    params.converted_note_count = 2;
    for frame in &mut play.frames {
        frame.judged.retain(|n| n.note_id < 2);
    }
    master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            [0, 3].into_iter().map(move |result| crate::master::LuckBonusLotRow {
                id: kind * 2 + result / 3 + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: 1,
            })
        })
        .collect();
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let support = luck_rush_dp_support_with_ranking(
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
    let mut masks = vec![0u8; play.frames.len()];
    let mut prefixes = vec![Vec::new()];
    let mut terminals = 0;
    while let Some(prefix) = prefixes.pop() {
        let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        native.set_random(LiveRandom::with_support_prefix(prefix.clone()));
        native.score.begin_bounds(Vec::new(), true);
        let mut applied = 0i32;
        let mut path = Vec::new();
        let mut failed = None;
        for (frame, &dt) in play.frames.iter().zip(&delta) {
            let before = applied != 0;
            if let Err(error) = native.frame_timed(frame.time_ms, &frame.judged, dt) {
                failed = Some(error);
                break;
            }
            for event in native.score.bounds_trace.as_mut().unwrap().events.drain(..) {
                if let BoundsEvent::Factor { command, .. } = event {
                    applied += command.note_mill;
                }
            }
            path.push(1 << (2 * usize::from(before) + usize::from(applied != 0)));
        }
        assert!(native.random.nominal_covers_draws());
        if let Some(branch) = native.random.nominal_branch() {
            assert!(failed.is_some());
            for choice in 0..branch.len() {
                let mut next = prefix.clone();
                next.push(choice);
                prefixes.push(next);
            }
        } else {
            assert!(failed.is_none());
            assert!(native.random.nominal_prefix_consumed());
            terminals += 1;
            for (mask, edge) in masks.iter_mut().zip(path) {
                *mask |= edge;
            }
        }
    }
    assert!(terminals > 1);
    assert_eq!(masks, support.probe_transitions);
}

#[test]
fn support_cancellation_and_input_refusal_do_not_return_partial_curves() {
    let (master, notes, params, setup, play, delta) = fixture(3, 60);
    let skills = luck_skills(&master).unwrap();
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let cancelled = luck_rush_dp_support_with_ranking_cancellable(
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
        &mut || true,
    )
    .unwrap();
    assert!(cancelled.is_none());
    let cancelled_invalid = luck_rush_dp_support_with_ranking_cancellable(
        &master,
        &skills,
        &notes,
        &[],
        params,
        &setup,
        &play,
        &delta[..delta.len() - 1],
        &deck,
        None,
        None,
        &mut || true,
    )
    .unwrap();
    assert!(cancelled_invalid.is_none());
    let mut cache = LuckDpCache::new(0);
    let certified_cancelled_invalid = cache
        .certified_cancellable(
            &master,
            &skills,
            &notes,
            &[],
            params,
            &setup,
            &play,
            &delta[..delta.len() - 1],
            &deck,
            None,
            None,
            None,
            &mut || true,
        )
        .unwrap();
    assert!(certified_cancelled_invalid.is_none());
    let refused = luck_rush_dp_support_with_ranking(
        &master,
        &skills,
        &notes,
        &[],
        params,
        &setup,
        &play,
        &delta[..delta.len() - 1],
        &deck,
        None,
        None,
    )
    .unwrap_err();
    let expected = certified_mode(
        &master,
        &skills,
        &notes,
        &[],
        params,
        &setup,
        &play,
        &delta[..delta.len() - 1],
        &deck,
        None,
        None,
        false,
    )
    .unwrap_err();
    assert_eq!(refused, expected);
    let prepared = prepare_recording::<Support>(
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
        false,
    )
    .unwrap();
    let transcript = record_prepared(prepared, &notes, &play, &delta, &mut || false).unwrap().unwrap();
    assert!(propagate_cancellable(&transcript, &mut || true).unwrap().is_none());
}

#[test]
fn support_point_only_rows_preserve_native_probe_and_rush_class_projection() {
    let (mut master, notes, params, setup, play, delta) = fixture(3, 140);
    let deck = [Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(31, 1)], ..Default::default() }];
    let skills = luck_skills(&master).unwrap();
    let plain = luck_rush_dp_support_with_ranking(
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
    let mut original = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    original.run_timed(&play, &delta).unwrap();
    let mut point = master.gekisou_skill_effects[0].clone();
    point.id = 991;
    point.skill_effect_type = 11002;
    point.activation_time_second = 0.0;
    point.effect_value = 77;
    master.gekisou_skill_effects.push(point);
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let extended = luck_rush_dp_support_with_ranking(
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
    assert_eq!(plain, extended);
    let mut native = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    native.run_timed(&play, &delta).unwrap();
    assert!(native.gekisou_ranges()[0].luck_points >= original.gekisou_ranges()[0].luck_points + 77);
    assert_eq!(native.score(), original.score());
    assert_eq!(native.current_life(), original.current_life());
}
