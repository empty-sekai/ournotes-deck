use super::*;

#[test]
fn reachable_terminal_support_contains_the_complete_native_outcome_set() {
    let (mut master, mut notes, mut params, setup, mut play, delta) = tests::fixture();
    notes.truncate(2);
    params.converted_note_count = notes.len() as i32;
    for frame in &mut play.frames {
        frame.judged.retain(|note| notes.iter().any(|item| item.note_id == note.note_id));
    }
    master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            [0, 3].into_iter().enumerate().map(move |(index, result)| crate::master::LuckBonusLotRow {
                id: kind * 2 + index as i64 + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: 1,
            })
        })
        .collect();
    master.reindex().unwrap();
    let skills = luck_skills(&master).unwrap();
    let mut support = LuckMaximumSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    let bounds = support.support(&[], || false).unwrap().unwrap();
    let mut exact = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
    let outcomes = exact.support(&[], &mut LuckExactBudget::default(), None, || false).unwrap();
    assert!(outcomes.decline.is_none(), "complete native outcome oracle required");
    assert!(!outcomes.outcomes.is_empty());
    for (score, life) in outcomes.outcomes {
        assert!(bounds.final_support.lower <= score && score <= bounds.final_support.upper);
        if let Some(value) = bounds.exact_constant_score {
            assert_eq!(value, score);
        }
        if let Some(value) = bounds.exact_final_life {
            assert_eq!(value, life);
        }
    }
    let serialized = serde_json::to_value(&bounds).unwrap();
    assert!(serialized.get("finalSupport").is_some());
    assert!(serialized.get("reachabilityPeakStates").is_some());
}

#[test]
fn support_session_preserves_cancellation_and_frame_input_validation() {
    let (master, notes, params, setup, play, delta) = tests::fixture();
    let skills = luck_skills(&master).unwrap();
    let mut session = LuckMaximumSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    assert!(session.support(&[], || true).unwrap().is_none());
    let mut polls = 0;
    assert!(
        session
            .support(&[], || {
                polls += 1;
                polls >= 3
            })
            .unwrap()
            .is_none()
    );
    assert!(session.support(&[], || false).unwrap().is_some());
    let mut invalid =
        LuckMaximumSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta[..delta.len() - 1], None);
    assert!(matches!(invalid.support(&[], || false), Err(Error::Input(_))));
    let mut params = params;
    params.converted_note_count = 0;
    let mut invalid = LuckMaximumSession::new(&master, &skills, &notes, &[], params, &setup, &play, &delta, None);
    assert!(matches!(invalid.support(&[], || false), Err(Error::Unsupported(_))));
}

#[test]
fn support_only_snapshot_difference_retains_shared_integer_history_cancellation() {
    let queries = [
        QueryParts {
            note_mean: F64Interval::integer(700),
            note_support: I32Interval::point(700),
            fixed_coefficients: vec![1],
            to: 10,
            executed_from: 0,
            notes: Some(vec![(1, I32Interval::point(700), F64Interval::integer(700))]),
        },
        QueryParts {
            note_mean: F64Interval::integer(725),
            note_support: I32Interval::point(725),
            fixed_coefficients: vec![1],
            to: 12,
            executed_from: 11,
            notes: Some(vec![
                (1, I32Interval::point(700), F64Interval::integer(700)),
                (11, I32Interval::point(25), F64Interval::integer(25)),
            ]),
        },
    ];
    let fixed = [(1, 0, F64Interval::new(-100.0, 100.0).unwrap(), I32Interval::new(-100, 100).unwrap())];
    let (_, support) = snapshot_difference_mode::<false>(Some(0), 1, &queries, &fixed).unwrap();
    assert_eq!(support.lower(), 25);
    assert_eq!(support.upper(), 25);
}
