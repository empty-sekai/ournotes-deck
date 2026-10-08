use super::*;

fn key(position: usize) -> EntryKey {
    vec![
        (LuckSkillKey { source: LuckSource::Gekisou, id: 7, level: 1, matched: None }, position),
        (LuckSkillKey { source: LuckSource::GekisouSupport, id: 69, level: 3, matched: Some(true) }, position),
    ]
}

fn curve() -> ResponseCurve {
    ResponseCurve {
        steps: vec![
            ResponseStep { time_ms: i32::MIN, buckets: [[-0.0, 0.0], [0.1, 0.1f64.next_up()], [0.0, 0.0], [0.9, 1.0]] },
            ResponseStep { time_ms: -1, buckets: [[-0.0, 0.0], [0.1, 0.1f64.next_up()], [0.0, 0.0], [0.9, 1.0]] },
            ResponseStep { time_ms: i32::MAX, buckets: [[0.0, 0.0], [0.0, 0.0], [1.0, 1.0], [0.0, 0.0]] },
        ],
        probe_transitions: vec![1, 1, 2, 8, 8, 8, 4, 1],
        probes: vec![true, false, true, false, true, false, true, false, true],
        range_moments: vec![ResponseRangeMoments {
            luck_points: [100.0, 100.0f64.next_up()],
            lot_results: [[0.0, 1.0], [1.0, 2.0], [3.0, 3.0], [-0.0, 0.0]],
        }],
        peak_states: 1001,
        transitions: 9_001,
    }
}

fn table() -> ResponseTable {
    let value = Response::Success { curve: curve() };
    ResponseTable {
        context: ResponseContext {
            fingerprint: response_fingerprint(b"real descriptor"),
            algorithm_version: "test/1".into(),
        },
        entries: vec![
            ResponseEntry { key: key(3), response: value.clone() },
            ResponseEntry { key: key(0), response: value },
            ResponseEntry { key: vec![], response: Response::Unsupported { reason: "unsupported source".into() } },
        ],
    }
}

#[test]
fn luck_response_lossless_random_lookup_preserves_fields_bits_and_all_keys_with_shared_payloads() {
    let table = table();
    let limits = ResponseLimits::default();
    let bytes = table.encode(Quantization::Lossless, limits).unwrap();
    assert_eq!(&bytes[..MAGIC.len()], b"ONLRSP02");
    let archive = ResponseArchive::open(&bytes, limits).unwrap();
    assert_eq!(archive.context(), &table.context);
    assert_eq!(archive.keys().len(), 3);
    assert_eq!(archive.find(&key(0)).unwrap().blob, archive.find(&key(3)).unwrap().blob);
    for entry in &table.entries {
        let index = archive.key_index(&entry.key).unwrap();
        assert_eq!(archive.keys().nth(index), Some(&entry.key));
        let decoded = archive.lookup(&entry.key).unwrap().unwrap();
        assert!(decoded.allocated_bytes().unwrap() <= limits.max_decoded_bytes);
        assert_eq!(decoded, entry.response);
    }
    let Response::Success { curve: decoded } = archive.lookup(&key(0)).unwrap().unwrap() else { panic!() };
    for (before, after) in curve().steps.iter().zip(&decoded.steps) {
        assert_eq!(before.buckets.map(|pair| pair.map(f64::to_bits)), after.buckets.map(|pair| pair.map(f64::to_bits)));
    }
    let mut reordered = key(0);
    reordered.swap(0, 1);
    assert_eq!(archive.lookup(&reordered).unwrap(), None, "same-holder source order remains part of the key");
    assert_eq!(archive.lookup(&key(4)).unwrap(), None);
    let json = serde_json::to_vec(&table).unwrap();
    let restored = serde_json::from_slice::<ResponseTable>(&json).unwrap();
    assert_eq!(restored, table);
    for (before, after) in table.entries.iter().zip(&restored.entries) {
        if let (Response::Success { curve: before }, Response::Success { curve: after }) =
            (&before.response, &after.response)
        {
            for (before, after) in before.steps.iter().zip(&after.steps) {
                assert_eq!(
                    before.buckets.map(|pair| pair.map(f64::to_bits)),
                    after.buckets.map(|pair| pair.map(f64::to_bits)),
                    "JSON must preserve every binary64 endpoint, including signed zero and next-up values"
                );
            }
            for (before, after) in before.range_moments.iter().zip(&after.range_moments) {
                assert_eq!(before.luck_points.map(f64::to_bits), after.luck_points.map(f64::to_bits));
                assert_eq!(
                    before.lot_results.map(|pair| pair.map(f64::to_bits)),
                    after.lot_results.map(|pair| pair.map(f64::to_bits))
                );
            }
        }
    }
    for invalid in [
        r#"{"status":"success"}"#,
        r#"{"status":"unsupported","reason":"x","curve":null}"#,
        r#"{"status":"unsupported","reason":null}"#,
        r#"{"status":"unknown","reason":"x"}"#,
    ] {
        assert!(serde_json::from_str::<Response>(invalid).is_err());
    }
}

#[test]
fn luck_response_quantization_encloses_original_endpoints_and_keeps_masks_moments_and_exact_extremes() {
    let mut original = curve();
    original.steps.clear();
    let mut values =
        vec![0.0, f64::from_bits(1), 0.1, 0.5f64.next_down(), 0.5, 0.5f64.next_up(), 1.0f64.next_down(), 1.0];
    // Independent deterministic binary64 points, including values near grid boundaries.
    for numerator in 1..256 {
        values.push(f64::from(numerator) / 257.0);
    }
    for (index, value) in values.into_iter().enumerate() {
        original.steps.push(ResponseStep { time_ms: index as i32, buckets: [[value, value]; 4] });
    }
    let limits = ResponseLimits::default();
    for mode in [Quantization::U16, Quantization::U24, Quantization::U32] {
        let table = ResponseTable {
            context: table().context,
            entries: vec![ResponseEntry { key: key(0), response: Response::Success { curve: original.clone() } }],
        };
        let bytes = table.encode(mode, limits).unwrap();
        let archive = ResponseArchive::open(&bytes, limits).unwrap();
        let Response::Success { curve: decoded } = archive.lookup(&key(0)).unwrap().unwrap() else { panic!() };
        assert_eq!(decoded.probe_transitions, original.probe_transitions);
        assert_eq!(decoded.probes, original.probes);
        assert_eq!(decoded.range_moments, original.range_moments);
        assert_eq!((decoded.peak_states, decoded.transitions), (original.peak_states, original.transitions));
        let grid = (1u64 << mode.tag()) as f64;
        for (before, after) in original.steps.iter().zip(&decoded.steps) {
            assert_eq!(before.time_ms, after.time_ms);
            for (input, output) in before.buckets.iter().zip(after.buckets) {
                assert!(output[0] <= input[0] && output[1] >= input[1]);
                assert!(input[0] - output[0] <= 1.0 / grid && output[1] - input[1] <= 1.0 / grid);
                if input[0] == 0.0 || input[0] == 1.0 {
                    assert_eq!(output, *input);
                }
            }
        }
    }
}

#[test]
fn luck_response_empty_tables_empty_curves_duplicates_and_limits_are_explicit() {
    let limits = ResponseLimits::default();
    let empty = ResponseTable { context: table().context, entries: vec![] };
    let bytes = empty.encode(Quantization::Lossless, limits).unwrap();
    assert_eq!(ResponseArchive::open(&bytes, limits).unwrap().keys().len(), 0);
    let mut duplicate = table();
    duplicate.entries.push(duplicate.entries[0].clone());
    assert!(duplicate.encode(Quantization::Lossless, limits).is_err());
    let empty_curve = ResponseCurve {
        steps: vec![],
        probe_transitions: vec![],
        probes: vec![],
        range_moments: vec![],
        peak_states: 0,
        transitions: 0,
    };
    let single = ResponseTable {
        context: empty.context,
        entries: vec![ResponseEntry { key: vec![], response: Response::Success { curve: empty_curve } }],
    };
    let bytes = single.encode(Quantization::Lossless, limits).unwrap();
    let archive = ResponseArchive::open(&bytes, limits).unwrap();
    assert_eq!(archive.lookup(&vec![]).unwrap().unwrap(), single.entries[0].response);
    assert!(ResponseArchive::open(&bytes, ResponseLimits { max_archive_bytes: bytes.len() - 1, ..limits }).is_err());
    assert!(
        ResponseArchive::open(&bytes, ResponseLimits { max_index_bytes: archive.index_bytes() - 1, ..limits }).is_err()
    );
    let small = ResponseArchive::open(&bytes, ResponseLimits { max_decoded_bytes: 0, ..limits }).unwrap();
    assert!(small.lookup(&vec![]).is_err());
    assert!(table().encode(Quantization::Lossless, ResponseLimits { max_steps: 2, ..limits }).is_err());
}

#[test]
fn luck_response_rejects_truncation_corruption_aliasing_invalid_intervals_and_unbounded_lengths() {
    let limits = ResponseLimits::default();
    let bytes = table().encode(Quantization::Lossless, limits).unwrap();
    for end in 0..bytes.len() {
        assert!(ResponseArchive::open(&bytes[..end], limits).is_err(), "truncation at {end}");
    }
    let mut bad = bytes.clone();
    bad[0] ^= 1;
    assert!(ResponseArchive::open(&bad, limits).is_err());
    let mut old_format = bytes.clone();
    old_format[..MAGIC.len()].copy_from_slice(b"ONLRSP01");
    assert!(ResponseArchive::open(&old_format, limits).is_err());
    let archive = ResponseArchive::open(&bytes, limits).unwrap();
    let blob = archive.find(&key(0)).unwrap().blob.clone();
    let mut bad = bytes.clone();
    bad[blob.start] ^= 1;
    let damaged = ResponseArchive::open(&bad, limits).unwrap();
    assert!(damaged.lookup(&key(0)).is_err());
    assert!(damaged.lookup(&vec![]).is_ok(), "a lookup decodes only its own independent blob");
    let mut bad_table = table();
    if let Response::Success { curve } = &mut bad_table.entries[0].response {
        curve.steps[0].buckets[0] = [0.6, 0.5];
    }
    assert!(bad_table.encode(Quantization::Lossless, limits).is_err());
    // Mutating an offset cannot point a payload into the header, even when its length remains in bounds.
    let mut reader = Reader { bytes: &bytes, at: MAGIC.len() + 1 };
    for _ in 0..2 {
        let len = reader.count(limits.max_string_bytes).unwrap();
        reader.take(len).unwrap();
    }
    reader.var().unwrap();
    let count = reader.count(limits.max_key_items).unwrap();
    for _ in 0..count {
        reader.byte().unwrap();
        reader.signed().unwrap();
        reader.signed().unwrap();
        reader.take(2).unwrap();
    }
    let mut bad = bytes.clone();
    bad[reader.at..reader.at + 8].copy_from_slice(&0u64.to_le_bytes());
    assert!(ResponseArchive::open(&bad, limits).is_err());
    // A hostile count never reaches an allocation.
    let mut payload = Writer::new(100);
    payload.put(&[1]).unwrap();
    payload.var(0).unwrap();
    payload.var(0).unwrap();
    payload.var(u64::MAX).unwrap();
    assert!(decode_response(&payload.bytes, Quantization::Lossless, limits).is_err());
    assert!(Reader { bytes: &[255; 10], at: 0 }.var().is_err());
}

#[test]
fn luck_response_paired_endpoint_encoding_and_checked_quantized_widths() {
    let limits = ResponseLimits::default();
    let original = curve();
    let payload =
        encode_response(&Response::Success { curve: original.clone() }, Quantization::Lossless, limits).unwrap();
    let mut reader = Reader { bytes: &payload, at: 0 };
    assert_eq!(reader.byte().unwrap(), 1);
    assert_eq!(reader.var().unwrap(), original.peak_states as u64);
    assert_eq!(reader.var().unwrap(), original.transitions);
    assert_eq!(reader.var().unwrap(), original.steps.len() as u64);
    for _ in &original.steps {
        reader.signed().unwrap();
    }
    let mut previous_lower = [0u64; 4];
    for (step, run) in [(&original.steps[0], 2), (&original.steps[2], 1)] {
        assert_eq!(reader.var().unwrap(), run);
        for (bounds, old_lower) in step.buckets.iter().zip(&mut previous_lower) {
            let [lower, upper] = bounds.map(f64::to_bits);
            assert_eq!(reader.var().unwrap(), lower ^ *old_lower);
            assert_eq!(reader.var().unwrap(), upper ^ lower);
            *old_lower = lower;
        }
    }
    // A complete single-row payload distinguishes width arithmetic/grid rejection from truncation.
    let quantized_payload = |lower: u64, width: u64| {
        let mut out = Writer::new(256);
        out.put(&[1]).unwrap();
        out.var(0).unwrap(); // Peak states.
        out.var(0).unwrap(); // Transitions.
        out.var(1).unwrap(); // One step.
        out.signed(0).unwrap();
        out.var(1).unwrap(); // One-row run.
        out.var(lower).unwrap();
        out.var(width).unwrap();
        for _ in 0..3 {
            out.var(0).unwrap();
            out.var(0).unwrap();
        }
        for _ in 0..3 {
            out.var(0).unwrap(); // Empty masks, probes and moments.
        }
        out.bytes
    };
    for mode in [Quantization::U16, Quantization::U24, Quantization::U32] {
        let grid = 1u64 << mode.tag();
        for (lower, width) in [(0, grid), (grid - 1, 1), (grid, 0)] {
            let Response::Success { curve } = decode_response(&quantized_payload(lower, width), mode, limits).unwrap()
            else {
                panic!()
            };
            assert_eq!(curve.steps[0].buckets[0], [lower as f64 / grid as f64, (lower + width) as f64 / grid as f64]);
        }
        for (lower, width, error) in [
            (1, u64::MAX, "quantized interval width overflow"),
            (grid, 1, "quantized endpoint exceeds one"),
            (grid + 1, 0, "quantized endpoint exceeds one"),
        ] {
            assert_eq!(decode_response(&quantized_payload(lower, width), mode, limits).unwrap_err(), invalid(error));
        }
    }
}
