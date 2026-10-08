use super::*;
use crate::live::full::scorecalc::NoteCommand;
use crate::live::skill::FactorCommand;

fn trace(events: Vec<BoundsEvent>) -> BoundsTrace {
    BoundsTrace {
        queries: events.iter().filter(|event| matches!(event, BoundsEvent::Query { .. })).count(),
        events,
        frames: 8192,
        probes: Vec::new(),
        combo: ComboObserver::default(),
        has_luck: true,
        filing_gate: Some(Some(2)),
        probe_filings: None,
    }
}

/// This is a storage oracle, not a recipe admission. Deliberately invalid arithmetic and snapshot inputs
/// must survive unchanged so the original downstream consumer can make its own refusal.
fn complete_fields() -> BoundsTrace {
    let mut events = vec![
        BoundsEvent::Note {
            frame: usize::MAX,
            index: usize::MAX - 1,
            note: NoteCommand::new(i32::MIN, -71, i32::MAX, -5, 6),
        },
        BoundsEvent::Factor {
            frame: 37,
            command: FactorCommand {
                time_ms: -39,
                owner_id: i32::MIN,
                note_mill: i32::MAX,
                combo_mill: -3,
                judgement: -7,
                judge_mill: 9,
                band_total_power: 11,
                luck: -13,
            },
        },
        BoundsEvent::Potential { frame: usize::MAX },
        BoundsEvent::Probe { frame: usize::MAX, time_ms: i32::MIN },
        BoundsEvent::Potential { frame: 0 },
        BoundsEvent::Potential { frame: u32::MAX as usize },
        BoundsEvent::Probe { frame: 77, time_ms: i32::MAX },
        BoundsEvent::Query { time_ms: -8, to: i32::MIN },
        BoundsEvent::Query { time_ms: -8, to: i32::MIN },
        BoundsEvent::ProbabilityReady(i32::MIN),
        BoundsEvent::ProbabilityReady(i32::MAX),
        BoundsEvent::Rank { range: usize::MAX, time_ms: -99, percent: i64::MIN, start: None, end: Some(usize::MAX) },
        BoundsEvent::Rank { range: 0, time_ms: i32::MAX, percent: i64::MAX, start: Some(8), end: None },
    ];
    for (index, bits) in
        [0, 0x8000_0000, 1, 0x8000_0001, 0x7f80_0000, 0xff80_0000, 0x7fc0_1234, 0xffc0_5678].into_iter().enumerate()
    {
        events.push(BoundsEvent::Combo {
            frame: index,
            index: usize::MAX - index,
            ordinary: f32::from_bits(bits),
            gekisou: f32::from_bits(bits ^ 0x8000_0000),
        });
    }
    let mut value = trace(events);
    value.frames = usize::MAX;
    value.queries = usize::MAX - 3;
    value.probes = vec![
        ProbeRow { owner: -4, value: f32::from_bits(0x8000_0000) },
        ProbeRow { owner: -4, value: f32::from_bits(0x7fc0_9876) },
        ProbeRow { owner: i32::MAX, value: -19.0 },
    ];
    value.probe_filings = Some(vec![usize::MAX, 1, 1, 0]);
    value
}

fn same_trace(expected: &BoundsTrace, actual: &BoundsTrace) {
    assert_eq!(
        (actual.frames, actual.queries, actual.has_luck),
        (expected.frames, expected.queries, expected.has_luck)
    );
    assert_eq!(actual.filing_gate, expected.filing_gate);
    assert_eq!(actual.probe_filings, expected.probe_filings);
    assert_eq!(
        actual.probes.iter().map(|probe| (probe.owner, probe.value.to_bits())).collect::<Vec<_>>(),
        expected.probes.iter().map(|probe| (probe.owner, probe.value.to_bits())).collect::<Vec<_>>()
    );
    assert_eq!(actual.events.len(), expected.events.len());
    for (ordinal, (actual, expected)) in actual.events.iter().zip(&expected.events).enumerate() {
        match (actual, expected) {
            (BoundsEvent::Note { frame: a, index: b, note: c }, BoundsEvent::Note { frame: x, index: y, note: z }) => {
                assert_eq!((a, b, c.bounds_identity()), (x, y, z.bounds_identity()), "event {ordinal}");
                // The retained payload is the original NoteCommand clone, including its private execution
                // accumulator and feature-dependent diagnostics. These filed notes have constructor zeros.
                assert_eq!(format!("{c:?}"), format!("{z:?}"), "event {ordinal}");
            }
            (BoundsEvent::Factor { frame: a, command: b }, BoundsEvent::Factor { frame: x, command: y }) => {
                assert_eq!((a, b), (x, y), "event {ordinal}");
            }
            (BoundsEvent::Potential { frame: a }, BoundsEvent::Potential { frame: x }) => {
                assert_eq!(a, x, "event {ordinal}");
            }
            (BoundsEvent::Probe { frame: a, time_ms: b }, BoundsEvent::Probe { frame: x, time_ms: y }) => {
                assert_eq!((a, b), (x, y), "event {ordinal}");
            }
            (BoundsEvent::Query { time_ms: a, to: b }, BoundsEvent::Query { time_ms: x, to: y }) => {
                assert_eq!((a, b), (x, y), "event {ordinal}");
            }
            (
                BoundsEvent::Combo { frame: a, index: b, ordinary: c, gekisou: d },
                BoundsEvent::Combo { frame: x, index: y, ordinary: z, gekisou: w },
            ) => {
                assert_eq!((a, b, c.to_bits(), d.to_bits()), (x, y, z.to_bits(), w.to_bits()), "event {ordinal}");
            }
            (BoundsEvent::ProbabilityReady(a), BoundsEvent::ProbabilityReady(x)) => {
                assert_eq!(a, x, "event {ordinal}");
            }
            (
                BoundsEvent::Rank { range: a, time_ms: b, percent: c, start: d, end: e },
                BoundsEvent::Rank { range: v, time_ms: w, percent: x, start: y, end: z },
            ) => {
                assert_eq!((a, b, c, d, e), (v, w, x, y, z), "event {ordinal}");
            }
            _ => panic!("event {ordinal} changed: {actual:?} != {expected:?}"),
        }
    }
    assert_eq!(format!("{:?}", actual.combo), format!("{:?}", expected.combo));
}

#[test]
fn packed_terminal_trace_roundtrips_every_field_bit_and_original_event_ordinal() {
    let mut original = complete_fields();
    for has_luck in [false, true] {
        for gate in [None, Some(None), Some(Some(i64::MIN)), Some(Some(2))] {
            for filings in [None, Some(Vec::new()), Some(vec![usize::MAX, 1, 1, 0])] {
                original.has_luck = has_luck;
                original.filing_gate = gate;
                original.probe_filings = filings;
                let packed = PackedTerminalTrace::encode(&original, usize::MAX, || false).unwrap();
                let restored = packed.decode(packed.decode_workspace_bytes(), || false).unwrap();
                same_trace(&original, &restored);
                assert!(trace_bytes(&restored).unwrap() <= packed.decode_workspace_bytes());
            }
        }
    }
}

fn clock_trace() -> BoundsTrace {
    let mut events = Vec::new();
    for frame in 0..512usize {
        events.push(BoundsEvent::Potential { frame });
        events.push(BoundsEvent::Probe { frame, time_ms: frame as i32 * 40 - 1 });
        events.push(BoundsEvent::Query { time_ms: frame as i32 * 40, to: frame as i32 });
        if frame.is_multiple_of(8) {
            events.push(BoundsEvent::Note {
                frame,
                index: 0,
                note: NoteCommand::new(frame as i32 * 40, 987, frame as i32, 1, 5),
            });
            events.push(BoundsEvent::Combo { frame, index: 0, ordinary: 1.125, gekisou: 1.25 });
        }
        events.push(BoundsEvent::ProbabilityReady(frame as i32 * 40));
        events.push(BoundsEvent::Query { time_ms: frame as i32 * 40, to: frame as i32 });
    }
    trace(events)
}

#[test]
fn packed_terminal_trace_reduces_clock_storage_without_deleting_any_query_or_filing() {
    let original = clock_trace();
    let original_bytes = trace_bytes(&original).unwrap();
    let packed = PackedTerminalTrace::encode(&original, 32 << 20, || false).unwrap();
    assert!(packed.allocated_bytes() * 3 < original_bytes);
    assert_eq!(packed.events.len(), original.events.len());
    assert_eq!(packed.queries, 1024);
    same_trace(&original, &packed.decode(packed.decode_workspace_bytes(), || false).unwrap());
    // One workspace is shared by all resident recipes. Even after charging it, retaining a full labelled
    // 120-order batch costs less than half the corresponding original trace allocations in this clock case.
    assert!(120 * packed.allocated_bytes() + packed.decode_workspace_bytes() < 60 * original_bytes);
    let stored = StoredTerminalTrace::encode(original, 32 << 20, || false).unwrap();
    assert!(matches!(stored, StoredTerminalTrace::Packed(_)));
}

#[test]
fn packed_terminal_trace_capacity_refusals_preserve_the_original_recipe_format() {
    let original = clock_trace();
    assert!(matches!(PackedTerminalTrace::encode(&original, 0, || false), Err(Decline::Capacity)));
    let packed = PackedTerminalTrace::encode(&original, usize::MAX, || false).unwrap();
    let bytes = packed.decode_workspace_bytes();
    for capacity in [0, bytes - 1] {
        assert!(matches!(packed.decode(capacity, || false), Err(Decline::Capacity)));
    }
    same_trace(&original, &packed.decode(bytes, || false).unwrap());
    // A tiny optional codec budget does not disable the old power-independent recipe representation.
    let stored = StoredTerminalTrace::encode(original.clone(), 1, || false).unwrap();
    assert!(matches!(stored, StoredTerminalTrace::Original(_)));
    assert_eq!(stored.decode_workspace_bytes(), 0);
    assert!(matches!(stored.decode(0, || false).unwrap(), Cow::Borrowed(_)));
    same_trace(&original, &stored.decode(0, || false).unwrap());

    // Full payload variants offer no compaction benefit when no padded allocation was left by recording.
    let mut original =
        trace(vec![BoundsEvent::Rank { range: 3, time_ms: -80, percent: -17, start: Some(0), end: Some(1) }]);
    original.events.shrink_to_fit();
    let stored = StoredTerminalTrace::encode(original.clone(), usize::MAX, || false).unwrap();
    assert!(matches!(stored, StoredTerminalTrace::Original(_)));
    same_trace(&original, &stored.decode(0, || false).unwrap());
}

#[test]
fn packed_terminal_trace_format_boundary_charges_the_storage_enum_and_complete_workspace() {
    let original = clock_trace();
    let packed = StoredTerminalTrace::encode(clock_trace(), usize::MAX, || false).unwrap();
    assert!(matches!(packed, StoredTerminalTrace::Packed(_)));
    let total = packed.allocated_bytes() + packed.decode_workspace_bytes();
    let fits = StoredTerminalTrace::encode(clock_trace(), total, || false).unwrap();
    assert!(matches!(fits, StoredTerminalTrace::Packed(_)));
    assert_eq!(fits.allocated_bytes() + fits.decode_workspace_bytes(), total);
    same_trace(&original, &fits.decode(fits.decode_workspace_bytes(), || false).unwrap());
    let raw = StoredTerminalTrace::encode(clock_trace(), total - 1, || false).unwrap();
    assert!(matches!(raw, StoredTerminalTrace::Original(_)));
    assert_eq!(raw.decode_workspace_bytes(), 0);
    same_trace(&original, &raw.decode(0, || false).unwrap());
}

#[test]
fn packed_terminal_trace_rejects_nondefault_recording_scratch_instead_of_silently_dropping_it() {
    for field in 0..8 {
        let mut original = complete_fields();
        match field {
            0 => original.combo.seen.push(vec![Some((0x8000_0000, 0x7fc0_1234))]),
            1 => original.combo.consistent = 1,
            2 => original.combo.judgements = 1,
            3 => original.combo.windows = Some(Vec::new()),
            4 => original.combo.current.push((-1, 2, 3)),
            5 => original.combo.filed.push(7),
            6 => original.combo.stale.push((0, 1)),
            7 => original.combo.seen.reserve_exact(1),
            _ => unreachable!(),
        }
        assert!(matches!(PackedTerminalTrace::encode(&original, usize::MAX, || false), Err(Decline::Incomplete)));
    }
}

#[test]
fn packed_terminal_trace_cancels_at_every_codec_poll_and_never_publishes_partial_output() {
    let original = clock_trace();
    let mut checks = 0;
    let packed = PackedTerminalTrace::encode(&original, usize::MAX, || {
        checks += 1;
        false
    })
    .unwrap();
    assert!(checks > 4);
    for stop in 1..=checks {
        let mut seen = 0;
        assert!(matches!(
            PackedTerminalTrace::encode(&original, usize::MAX, || {
                seen += 1;
                seen >= stop
            }),
            Err(Decline::Cancelled)
        ));
    }
    let mut checks = 0;
    packed
        .decode(packed.decode_workspace_bytes(), || {
            checks += 1;
            false
        })
        .unwrap();
    assert!(checks > 4);
    for stop in 1..=checks {
        let mut seen = 0;
        assert!(matches!(
            packed.decode(packed.decode_workspace_bytes(), || {
                seen += 1;
                seen >= stop
            }),
            Err(Decline::Cancelled)
        ));
    }
    same_trace(&original, &packed.decode(packed.decode_workspace_bytes(), || false).unwrap());
    assert!(matches!(StoredTerminalTrace::encode(original.clone(), 0, || true), Err(Decline::Cancelled)));
    let stored = StoredTerminalTrace::encode(original, 0, || false).unwrap();
    assert!(matches!(stored.decode(0, || true), Err(Decline::Cancelled)));
}

#[test]
fn packed_terminal_trace_checks_payload_order_and_completeness_before_returning() {
    let original = complete_fields();
    for change in 0..3 {
        let mut packed = PackedTerminalTrace::encode(&original, usize::MAX, || false).unwrap();
        match change {
            0 => packed.events[0] = Event::Stored(u32::MAX),
            1 => packed.payloads.clear(),
            2 => packed.events[0] = Event::Query { time_ms: 0, to: 0 },
            _ => unreachable!(),
        }
        assert!(matches!(packed.decode(packed.decode_workspace_bytes(), || false), Err(Decline::Incomplete)));
    }
}
