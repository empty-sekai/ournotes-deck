fn same_history(actual: &Replay, expected: &Replay) {
    assert_eq!(class_bits(actual.state), class_bits(expected.state));
    assert_eq!(actual.prev, expected.prev);
    assert_eq!(actual.fresh, expected.fresh);
    assert_eq!(actual.mandatory, expected.mandatory);
    assert_eq!(actual.potential, expected.potential);
    assert_eq!(actual.notes.len(), expected.notes.len());
    for (actual, expected) in actual.notes.iter().zip(&expected.notes) {
        assert_eq!((actual.time_ms, actual.note_id), (expected.time_ms, expected.note_id));
        assert_eq!(class_bits(actual.executed), class_bits(expected.executed));
    }
    for (actual, expected) in actual.frames.iter().zip(&expected.frames) {
        assert_eq!(actual.diff.map(|value| value.map(class_bits)), expected.diff.map(|value| value.map(class_bits)));
        assert_eq!(actual.undo.map(class_bits), expected.undo.map(class_bits));
    }
}

#[test]
fn empty_frame_reuse_matches_full_arithmetic_for_each_class_and_field() {
    let probe_rows = [ProbeRow { owner: 1, value: 0.125 }];
    for rows in [&[][..], &probe_rows[..]] {
        for mask in 0..4 {
            for all in [false, true] {
                for field in 0..FIELDS {
                    for (lower, upper) in
                        [(-0.0, -0.0), (0.0, 0.0), (-0.0, 0.0), (0.0, -0.0), (-1.25, 2.5), (-f32::MAX, f32::MAX)]
                    {
                        let mut fields = initial_state();
                        fields[field] = F32Interval::new(lower, upper).unwrap();
                        let state = std::array::from_fn(|class| ((mask >> class) & 1 != 0).then_some(fields));
                        let mut actual = Replay::new(3, rows);
                        let mut expected = Replay::new(3, rows);
                        for replay in [&mut actual, &mut expected] {
                            replay.file_note(1, 40, 1).unwrap();
                            replay.execute_full(1, [Some(initial_state()), None], true).unwrap();
                        }
                        let result = actual.execute(1, state, all).unwrap();
                        let reference = expected.execute_full(1, state, all).unwrap();
                        assert_eq!(class_bits(result), class_bits(reference));
                        same_history(&actual, &expected);
                    }
                }
            }
        }
    }
}

#[test]
fn empty_frames_reject_nonfinite_fields_before_recording_history() {
    for class in 0..2 {
        for field in 0..FIELDS {
            for (lower, upper) in [
                (f32::NEG_INFINITY, f32::NEG_INFINITY),
                (f32::INFINITY, f32::INFINITY),
                (f32::NEG_INFINITY, 1.0),
                (1.0, f32::INFINITY),
            ] {
                for all in [false, true] {
                    let mut fields = initial_state();
                    fields[field] = F32Interval::new(lower, upper).unwrap();
                    let mut state = [None, None];
                    state[class] = Some(fields);
                    let mut actual = Replay::new(3, &[]);
                    let mut expected = Replay::new(3, &[]);
                    for replay in [&mut actual, &mut expected] {
                        replay.file_note(1, 40, 1).unwrap();
                        replay.execute_full(1, [Some(initial_state()), None], true).unwrap();
                    }
                    let before_diff = actual.frames[1].diff.unwrap().map(class_bits);
                    let before_undo = class_bits(actual.frames[1].undo.unwrap());
                    let before_note = class_bits(actual.notes[0].executed);
                    let result = actual.execute(1, state, all).unwrap_err();
                    let reference = expected.execute_full(1, state, all).unwrap_err();
                    assert!(matches!(result, Error::Unsupported(_)));
                    assert_eq!(result.to_string(), reference.to_string());
                    for replay in [&actual, &expected] {
                        assert_eq!(replay.frames[1].diff.unwrap().map(class_bits), before_diff);
                        assert_eq!(class_bits(replay.frames[1].undo.unwrap()), before_undo);
                        assert_eq!(class_bits(replay.notes[0].executed), before_note);
                    }
                }
            }
        }
    }
}
