use super::*;

fn frames(length: i32, score_length: Option<i32>) -> ScoreFrames {
    ScoreFrames::new(&LiveParams {
        skill_target_music_type: 0,
        total_power: 1,
        music_level: 1,
        converted_note_count: 1,
        music_length_ms: length,
        score_music_length_ms: score_length,
        assist_factor: 1.0,
    })
}

/// Independent candidate reference: sample each original span start and rescan all five original rows.
fn reference(parts: [&Contrib; 5], score_frames: ScoreFrames) -> (f64, f64) {
    let (mut peak, mut peak_frame) = (0.0f64, 0.0f64);
    for p in parts {
        for &(a, _, _) in &p.spans {
            let frame = score_frames.at(a);
            let (mut at, mut near) = (0.0f64, 0.0f64);
            for q in parts {
                for &(b0, b1, g) in &q.spans {
                    if b0 <= a && a < b1 {
                        at = (at + g).next_up();
                    }
                    if score_frames.meets(b0, b1, frame) {
                        near = (near + g).next_up();
                    }
                }
            }
            peak = peak.max(at);
            peak_frame = peak_frame.max(near);
        }
    }
    (peak, peak_frame)
}

fn assert_same(actual: (f64, f64), expected: (f64, f64)) {
    assert_eq!([actual.0.to_bits(), actual.1.to_bits()], [expected.0.to_bits(), expected.1.to_bits()]);
}

#[test]
fn fine_span_sweep_matches_every_original_part_order_at_native_frame_boundaries() {
    let times = [
        (0, 0),
        (-30, 20),
        (80, 80),
        (10, 50),
        (50, 90),
        (2000, 2020),
        (2040, 2080),
        (100, 40),
        (i64::MIN, i64::MAX),
        (i64::MAX, i64::MAX),
        (134_217_880, 134_217_928),
        (i64::MIN, i64::MIN),
    ];
    let values =
        [0.11, 0.15, 0.35, 2f64.powi(52), -2f64.powi(52), 0.0, -0.0, f64::from_bits(1), -f64::from_bits(1), -0.7];
    let parts: [Contrib; 5] = std::array::from_fn(|part| Contrib {
        spans: (0..times.len())
            .map(|index| {
                let (start, end) = times[(index * 5 + part) % times.len()];
                (start, end, values[(index + part * 3) % values.len()])
            })
            .collect(),
        ..Default::default()
    });
    let mut scratch = Scratch::default();
    for score_frames in [frames(0, None), frames(80, None), frames(500, Some(1600)), frames(134_217_928, None)] {
        let mut order = [0, 1, 2, 3, 4];
        let mut count = 0;
        loop {
            let parts = order.map(|slot| &parts[slot]);
            assert_same(scratch.peaks(parts, score_frames), reference(parts, score_frames));
            assert_eq!(scratch.work.scanned_pairs, 0);
            count += 1;
            let Some(i) = (0..4).rev().find(|&i| order[i] < order[i + 1]) else { break };
            let j = (i + 1..5).rev().find(|&j| order[i] < order[j]).unwrap();
            order.swap(i, j);
            order[i + 1..].reverse();
        }
        assert_eq!(count, 120);
    }
}

#[test]
fn fine_span_sweep_keeps_nonfinite_signed_and_duplicate_sample_arithmetic() {
    for special in [f64::INFINITY, f64::NEG_INFINITY, f64::from_bits(0x7ff8_0000_0000_0042), -0.0, -1.75] {
        let parts: [Contrib; 5] = std::array::from_fn(|part| Contrib {
            spans: vec![
                (80, 160, special),
                (0, 0, 0.0),
                (100, 120, 0.11 + part as f64),
                (120, 160, -0.15),
                (80, 160, 0.35),
            ],
            ..Default::default()
        });
        let parts = parts.each_ref();
        let score_frames = frames(90, Some(500));
        let mut scratch = Scratch::default();
        assert_same(scratch.peaks(parts, score_frames), reference(parts, score_frames));
        assert!(scratch.work.samples < parts.iter().map(|part| part.spans.len()).sum::<usize>() * 2);
    }
}

#[test]
fn fine_span_sweep_preserves_each_sample_even_when_another_sample_wins_the_peak() {
    let parts: [Contrib; 5] = std::array::from_fn(|part| Contrib {
        spans: vec![
            (160, 200, -f64::from_bits(1)),
            (80, 120, if part % 2 == 0 { 2f64.powi(52) } else { -2f64.powi(52) }),
            (0, 40, 0.11 + part as f64 / 100.0),
            (40, 80, -0.15),
            (120, 160, 0.0),
        ],
        ..Default::default()
    });
    let parts = parts.each_ref();
    let score_frames = frames(80, None);
    let mut expected_time = Vec::new();
    let mut expected_frame = Vec::new();
    for p in parts {
        for &(a, _, _) in &p.spans {
            let frame = score_frames.at(a);
            let (mut at, mut near) = (0.0f64, 0.0f64);
            for q in parts {
                for &(b0, b1, g) in &q.spans {
                    if b0 <= a && a < b1 {
                        at = (at + g).next_up();
                    }
                    if score_frames.meets(b0, b1, frame) {
                        near = (near + g).next_up();
                    }
                }
            }
            expected_time.push(at.to_bits());
            expected_frame.push(near.to_bits());
        }
    }
    let mut scratch = Scratch::default();
    assert_same(scratch.peaks(parts, score_frames), reference(parts, score_frames));
    scratch.sweep(parts, None);
    assert_eq!(scratch.samples.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), expected_time);
    scratch.sweep(parts, Some(score_frames));
    assert_eq!(scratch.samples.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), expected_frame);
}

#[test]
fn fine_span_sweep_uses_active_terms_instead_of_all_span_pairs() {
    let parts: [Contrib; 5] = std::array::from_fn(|part| Contrib {
        spans: (0..256)
            .rev()
            .map(|index| {
                let start = index * 200 + part as i64 * 5;
                (start, start + 40, 0.11 + part as f64 / 100.0)
            })
            .collect(),
        ..Default::default()
    });
    let parts = parts.each_ref();
    let score_frames = frames(60_000, None);
    let mut scratch = Scratch::default();
    assert_same(scratch.peaks(parts, score_frames), reference(parts, score_frames));
    let count = parts.iter().map(|part| part.spans.len()).sum::<usize>();
    let old_pairs = 2 * count * count;
    assert_eq!(scratch.work.scanned_pairs, 0);
    assert!(scratch.work.endpoints + scratch.work.folded_spans < old_pairs / 32);
    assert!(scratch.work.folded_spans >= count, "every active source retains its own outward addition");
}

#[test]
fn fine_span_sweep_small_and_capacity_fallbacks_preserve_values_and_reusable_scratch() {
    let score_frames = frames(500, None);
    let mut scratch = Scratch::default();
    for count in [64, 0, 1, 15, 16, 80] {
        let parts: [Contrib; 5] = std::array::from_fn(|part| Contrib {
            spans: if part == 2 {
                (0..count).map(|index| (index as i64 * 20, index as i64 * 20 + 40, 0.15)).collect()
            } else {
                Vec::new()
            },
            ..Default::default()
        });
        let parts = parts.each_ref();
        let expected = reference(parts, score_frames);
        assert_same(scratch.peaks(parts, score_frames), expected);
        assert_same(scratch.peaks_with_limit(parts, score_frames, 0), expected);
        assert_eq!(scratch.work.scanned_pairs, 2 * count * count);
    }
}
