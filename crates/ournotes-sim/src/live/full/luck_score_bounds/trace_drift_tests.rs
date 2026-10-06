use super::*;

fn initial() -> [f32; FIELDS] {
    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]
}

fn delta(value: f32) -> [f32; FIELDS] {
    [0.0, value, 0.0, 0.0, 0.0, 0.0]
}

/// Independent scalar version of native calculate: retain the actual filed commands, reset each FrameDiff
/// after undo, and execute every actual command again. Its exact reference does not use the count formula.
struct Native {
    commands: Vec<Vec<f32>>,
    diff: Vec<f32>,
    exact_diff: Vec<f64>,
    state: f32,
    initial: f64,
    // Keep the exact shift separate from the initial value so a subnormal command is not swallowed by 1.0.
    exact_state: f64,
    previous: i32,
    added: Option<i32>,
    additions: u64,
    undos: u64,
    maximum_error: f64,
}

impl Native {
    fn new(frames: usize) -> Self {
        Self::with_initial(frames, 1.0)
    }

    fn with_initial(frames: usize, initial: f32) -> Self {
        Self {
            commands: vec![Vec::new(); frames],
            diff: vec![0.0; frames],
            exact_diff: vec![0.0; frames],
            state: initial,
            initial: f64::from(initial),
            exact_state: 0.0,
            previous: -1,
            added: None,
            additions: 0,
            undos: 0,
            maximum_error: 0.0,
        }
    }

    fn file(&mut self, frame: usize, value: Option<f32>) {
        let f = frame as i32;
        self.added = Some(self.added.map_or(f, |old| old.min(f)));
        if let Some(value) = value {
            self.commands[frame].push(value);
        }
    }

    fn observe(&mut self) {
        self.maximum_error = self.maximum_error.max(((f64::from(self.state) - self.initial) - self.exact_state).abs());
    }

    fn query(&mut self, to: i32) -> i32 {
        let u = self.added.map_or(to, |a| to.min(a - 1));
        let start = if u < self.previous {
            for f in (u + 1..=self.previous).rev() {
                let f = f as usize;
                self.state -= self.diff[f];
                self.exact_state -= self.exact_diff[f];
                self.diff[f] = 0.0;
                self.exact_diff[f] = 0.0;
                self.undos += u64::from(!self.commands[f].is_empty());
                self.observe();
            }
            u + 1
        } else {
            self.previous + 1
        };
        for f in start..=to {
            let f = f as usize;
            for i in 0..self.commands[f].len() {
                let command = self.commands[f][i];
                self.state += command;
                self.exact_state += f64::from(command);
                self.diff[f] += command;
                self.exact_diff[f] += f64::from(command);
                self.additions += 1;
                self.observe();
            }
        }
        self.previous = to;
        self.added = None;
        start
    }
}

#[test]
fn every_optional_filing_subset_is_covered_with_backwards_and_future_queries() {
    let probe = 0.333_333_34f32;
    let events = [
        Event::Factor { frame: 0, deltas: delta(0.271_828_18) },
        Event::Note { frame: 1 },
        Event::Query { to: 1 },
        Event::Probe { frame: 0 },
        Event::Query { to: 2 },
        Event::Factor { frame: 1, deltas: delta(-0.125_125) },
        Event::Query { to: 1 },
        Event::Probe { frame: 2 },
        Event::Query { to: 1 },
        Event::Probe { frame: 0 },
        Event::Potential { frame: 1 },
        Event::Query { to: 0 },
        Event::Query { to: 2 },
        Event::Other,
    ];
    let certificate = compile(3, initial(), &[probe], events, || false).unwrap();
    let error = certificate.universal_native_drift().unwrap()[1];
    for mask in 0..16u32 {
        let mut native = Native::new(3);
        let (mut optional, mut query) = (0u32, 0usize);
        for event in events {
            match event {
                Event::Factor { frame, deltas } => native.file(frame, Some(deltas[1])),
                Event::Note { frame } => native.file(frame, None),
                Event::Probe { frame } | Event::Potential { frame } => {
                    if mask & (1 << optional) != 0 {
                        let command = matches!(event, Event::Probe { .. }).then_some(if optional % 2 == 0 {
                            probe
                        } else {
                            -probe
                        });
                        native.file(frame, command);
                    }
                    optional += 1;
                }
                Event::Query { to } => {
                    let actual = native.query(to);
                    let possible = certificate.queries[query];
                    assert!(possible.from <= actual, "mask {mask}, query {query}");
                    query += 1;
                }
                Event::Other => {}
            }
        }
        assert!(native.additions <= certificate.fields[1].additions);
        assert!(native.undos <= certificate.fields[1].undo_subtractions);
        assert!(native.maximum_error <= error, "mask {mask}: {} > {error}", native.maximum_error);
    }
}

#[test]
fn original_query_ordinals_and_duplicate_probe_opportunities_are_retained() {
    let certificate = compile(
        2,
        initial(),
        &[0.1],
        [
            Event::Query { to: 1 },
            Event::Probe { frame: 0 },
            Event::Probe { frame: 0 },
            Event::Query { to: 1 },
            Event::Query { to: 0 },
            Event::Query { to: 1 },
        ],
        || false,
    )
    .unwrap();
    assert_eq!(
        certificate.queries.as_ref(),
        &[
            QueryRange { previous: -1, to: 1, from: 0 },
            QueryRange { previous: 1, to: 1, from: 0 },
            QueryRange { previous: 1, to: 0, from: 1 },
            QueryRange { previous: 0, to: 1, from: 1 },
        ]
    );
    assert_eq!(certificate.fields[1].additions, 4);
    assert!(certificate.fields[1].lifetime_l1 >= 2.0 * f64::from(0.1f32));
}

#[test]
fn mill_reference_allowance_includes_rounding_sensitive_integer_conversion() {
    for mill in [1, -1, (1 << 24) - 1, (1 << 24) + 1, i32::MAX, i32::MIN] {
        let value = mill as f32 / 100_000f32;
        let events = [Event::Factor { frame: 0, deltas: delta(value) }, Event::Query { to: 0 }];
        let certificate = compile(1, initial(), &[], events, || false).unwrap();
        let reference = f64::from(mill) / 100_000f64;
        let state_bound = (1.0 + reference.abs()).next_up();
        let margin = certificate.mill_reference_drift([state_bound; FIELDS]).unwrap()[1];
        let actual = f64::from(1.0f32 + value);
        assert!((actual - (1.0 + reference)).abs() <= margin, "mill {mill}");
    }
}

#[test]
fn every_history_add_and_undo_error_is_covered_with_signed_pulses() {
    for field in 0..FIELDS {
        for value in [0.1f32, f32::from_bits(1), 16_777_216f32, -0.375f32] {
            let mut events = Vec::new();
            let mut start = initial();
            if field != 1 {
                start[field] = -0.0;
            }
            let mut native = Native::with_initial(2, start[field]);
            for step in 0..32 {
                let sign = if step % 2 == 0 { value } else { -value };
                let frame = step % 2;
                let mut deltas = [0.0; FIELDS];
                deltas[field] = sign;
                events.push(Event::Factor { frame, deltas });
                events.push(Event::Query { to: 1 });
                native.file(frame, Some(sign));
                native.query(1);
            }
            let certificate = compile(2, start, &[], events, || false).unwrap();
            assert!(native.maximum_error <= certificate.universal_native_drift().unwrap()[field]);
            if field == 1 && value == f32::from_bits(1) {
                assert!(native.maximum_error > 0.0, "the exact-shift reference must retain subnormal commands");
            }
        }
    }
}

#[test]
fn zero_numeric_updates_have_zero_drift_without_identity_claims() {
    let mut state = initial();
    state[0] = -0.0;
    let certificate = compile(
        1,
        state,
        &[-0.0],
        [Event::Factor { frame: 0, deltas: [-0.0; FIELDS] }, Event::Probe { frame: 0 }, Event::Query { to: 0 }],
        || false,
    )
    .unwrap();
    assert_eq!(certificate.universal_native_drift().unwrap(), [0.0; FIELDS]);
    assert!(certificate.fields.iter().all(|field| field.additions == 0 && field.undo_subtractions == 0));
}

#[test]
fn cancellation_incomplete_indices_feedback_and_overflow_decline() {
    for stop_at in 1..=4 {
        let mut checks = 0;
        let error = compile(1, initial(), &[], [Event::Query { to: 0 }], || {
            checks += 1;
            checks == stop_at
        })
        .unwrap_err();
        assert_eq!(error, Decline::Cancelled);
    }
    assert_eq!(compile(1, initial(), &[], [], || false).unwrap_err(), Decline::Incomplete);
    assert_eq!(compile(1, initial(), &[], [Event::Query { to: 1 }], || false).unwrap_err(), Decline::FrameIndex);
    assert_eq!(
        compile(1, initial(), &[], [Event::Query { to: 0 }, Event::Potential { frame: 0 },], || false).unwrap_err(),
        Decline::Incomplete
    );
    let mut certificate = compile(1, initial(), &[], [Event::Query { to: 0 }], || false).unwrap();
    certificate.fields[1].additions = 1 << 24;
    assert_eq!(certificate.universal_native_drift().unwrap_err(), Decline::Feedback);
    certificate.fields[1].additions = u64::MAX;
    assert_eq!(certificate.universal_native_drift().unwrap_err(), Decline::CountOverflow);
    let overflow = compile(
        1,
        initial(),
        &[],
        [Event::Factor { frame: 0, deltas: delta(f32::MAX) }, Event::Query { to: 0 }],
        || false,
    )
    .unwrap();
    assert_eq!(overflow.universal_native_drift().unwrap_err(), Decline::Nonfinite);
}
