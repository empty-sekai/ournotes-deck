//! A one-entry arithmetic memo must preserve the original note evaluator and every refusal.
use super::*;
use crate::live::full::luck_score_bounds::terminal_prefix::{self, TimedEvent};
use crate::live::score::LiveScoreSettings;

struct Fixture {
    calc: LiveScoreCalculator,
    trace: BoundsTrace,
    ingredients: TerminalIngredients,
}

impl Fixture {
    fn new(count: usize) -> Self {
        let settings = LiveScoreSettings {
            score_adjustment_factor: 1.125,
            life_onus_factor: 0.25,
            note_factor_percent: [(1, 100), (2, 70)].into_iter().collect(),
            judgement_score_factor_percent: [(1, 230), (2, 100), (3, 80), (4, 50), (5, 0)].into_iter().collect(),
        };
        let calc = LiveScoreCalculator::new(17_003, 20, 173, &settings, 1.0625, 0.9375, None);
        let mut events = Vec::new();
        let mut timed = Vec::new();
        for index in 0..count {
            let frame = index / 2 + 1;
            let time_ms = frame as i32 * 40;
            events.push(BoundsEvent::Note {
                frame,
                index: index % 2,
                note: NoteCommand::new(time_ms, 100, index as i32, 1, 2),
            });
            timed.push(TimedEvent::Note { frame, note_id: index as i32, time_ms });
            events.push(BoundsEvent::Combo { frame, index: index % 2, ordinary: 1.125, gekisou: 1.0625 });
            timed.push(TimedEvent::Other);
        }
        let to = count / 2 + 1;
        events.push(BoundsEvent::Probe { frame: 1, time_ms: 40 });
        timed.push(TimedEvent::Probe { frame: 1, time_ms: 40 });
        events.push(BoundsEvent::Query { time_ms: to as i32 * 40, to: to as i32 });
        timed.push(TimedEvent::Query { to: to as i32 });
        let ingredients =
            terminal_prefix::build(to + 1, [0.125, 1.0, 0.0625, 0.125, 0.25, 0.5], &[0.25], &timed, || false).unwrap();
        let trace = BoundsTrace {
            events,
            queries: 1,
            frames: to + 1,
            probes: Vec::new(),
            combo: Default::default(),
            has_luck: true,
            filing_gate: Some(None),
            probe_filings: None,
        };
        Self { calc, trace, ingredients }
    }

    fn kernel(&self, linked: bool) -> Kernel<'_> {
        Kernel::new(&self.calc, 17_003, 35, &self.trace, &self.ingredients, linked, || false).unwrap()
    }

    fn note_mut(&mut self, index: usize) -> &mut NoteCommand {
        let BoundsEvent::Note { note, .. } = &mut self.trace.events[index * 2] else { unreachable!() };
        note
    }
}

fn compare(kernel: &Kernel<'_>, notes: &[TerminalNote]) -> (Result<Vec<[I32Interval; 4]>, Decline>, usize) {
    let mut hits = 0;
    let actual = kernel.rows_inner::<true>(notes, || false, &mut || hits += 1);
    let original = kernel.rows_inner::<false>(notes, || false, &mut || panic!("disabled memo hit"));
    // I32 intervals compare both exact endpoints; no tolerance or probability folding enters this oracle.
    assert_eq!(actual, original);
    (actual, hits)
}

#[test]
fn terminal_kernel_memo_matches_original_rows_across_identity_life_categories_and_fields() {
    for linked in [false, true] {
        let mut fixture = Fixture::new(18);
        // Distinct same-time identities, and later chart times, share arithmetic without sharing metadata.
        fixture.note_mut(1).life = 1;
        fixture.note_mut(2).life = 999;
        fixture.note_mut(3).life = 0;
        fixture.note_mut(4).life = -7;
        fixture.note_mut(5).score_type = 1;
        fixture.note_mut(6).score_type = 3;
        fixture.note_mut(7).score_type = 4;
        fixture.note_mut(8).score_type = 5;
        fixture.note_mut(9).note_type = 2;
        fixture.note_mut(10).note_type = 2;
        // Vary each consumed ordinary field endpoint; every later pair still uses its original occurrence.
        for (index, field) in [(11, 0), (12, 1), (13, 2), (14, 3), (15, 4), (16, 5)] {
            let note = &mut fixture.ingredients.notes[index];
            // Without linking, note-up includes +/- the complete probe amplitude. Keep its
            // lower endpoint positive after that subtraction so this is a successful fixture.
            note.ordinary_lower[field] = if field == 1 { 0.5 } else { 0.03125 };
            note.ordinary_upper[field] = if field == 1 { 1.5 } else { 0.5625 };
        }
        fixture.note_mut(13).score_type = 1;
        fixture.note_mut(15).score_type = 3;
        fixture.note_mut(16).score_type = 4;
        let kernel = fixture.kernel(linked);
        let (rows, hits) = compare(&kernel, &fixture.ingredients.notes);
        let rows = rows.unwrap();
        assert!(hits >= 4, "three positive LIFE notes, two nonpositive LIFE notes and the repeated type must hit");
        assert_eq!(rows[0], rows[1]);
        assert_eq!(rows[1], rows[2]);
        assert_eq!(rows[3], rows[4]);
        assert_ne!(rows[2], rows[3]);
        assert_eq!(rows[9], rows[10]);
        assert_ne!(rows[0], rows[9]);
        assert_eq!(kernel.rows(&fixture.ingredients.notes, || false).unwrap(), rows);
    }
}

#[test]
fn terminal_kernel_memo_keeps_every_recorded_combo_hull() {
    let mut fixture = Fixture::new(6);
    for index in 2..6 {
        let BoundsEvent::Combo { ordinary, gekisou, .. } = &mut fixture.trace.events[index * 2 + 1] else {
            unreachable!()
        };
        if index < 4 {
            *ordinary = 1.25;
        } else {
            *gekisou = 1.5;
        }
    }
    // Repeated observations widen both equal notes' hulls; last-observation-only keys would be invalid.
    for index in 2..4 {
        fixture.trace.events.push(BoundsEvent::Combo {
            frame: index / 2 + 1,
            index: index % 2,
            ordinary: 1.375,
            gekisou: 1.0625,
        });
    }
    for linked in [false, true] {
        let kernel = fixture.kernel(linked);
        let (rows, hits) = compare(&kernel, &fixture.ingredients.notes);
        let rows = rows.unwrap();
        assert_eq!(hits, 3);
        assert_ne!(rows[0], rows[2]);
        assert_ne!(rows[2], rows[4]);
    }
}

#[test]
fn terminal_kernel_memo_key_keeps_both_classes_all_endpoint_bits_and_presence() {
    let note = NoteCommand::new(20, 100, 1, 1, 2);
    let interval = F32Interval::new(1.0, 2.0).unwrap();
    let factors = [Some([interval; 2]); 2];
    let key = row_key(&note, &factors);
    for class in 0..2 {
        let mut absent = factors;
        absent[class] = None;
        assert_ne!(row_key(&note, &absent), key);
        for field in 0..2 {
            for upper in [false, true] {
                let mut changed = factors;
                changed[class].as_mut().unwrap()[field] = if upper {
                    F32Interval::new(1.0, 2.0_f32.next_up()).unwrap()
                } else {
                    F32Interval::new(1.0_f32.next_down(), 2.0).unwrap()
                };
                assert_ne!(row_key(&note, &changed), key);
            }
        }
    }
    let positive_zero = [Some([F32Interval::point(0.0).unwrap(); 2]); 2];
    let mut negative_zero = positive_zero;
    negative_zero[1].as_mut().unwrap()[1] = F32Interval::point(-0.0).unwrap();
    assert_ne!(row_key(&note, &positive_zero), row_key(&note, &negative_zero));
    let mut other = note;
    other.note_id = 99;
    other.time_ms = 900;
    other.life = 1;
    assert_eq!(row_key(&other, &factors), key);
    other.life = 0;
    assert_ne!(row_key(&other, &factors), key);
    other = note;
    other.note_type += 1;
    assert_ne!(row_key(&other, &factors), key);
    other = note;
    other.score_type += 1;
    assert_ne!(row_key(&other, &factors), key);
}

#[test]
fn terminal_kernel_memo_never_hides_later_structure_inputs_or_magnitude_refusal() {
    for kind in 0..9 {
        let mut fixture = Fixture::new(4);
        let expected = match kind {
            0 => {
                fixture.ingredients.notes[2].event = 1; // Combo is not a Note, after one successful hit.
                Decline::Incomplete
            }
            1 => {
                fixture.ingredients.notes[2].time_ms += 1;
                Decline::Incomplete
            }
            2 => {
                fixture.trace.events[5] = BoundsEvent::ProbabilityReady(0); // Missing this note's Combo.
                Decline::Incomplete
            }
            3 => {
                fixture.ingredients.notes[2].ordinary_upper[1] = f64::INFINITY;
                Decline::Nonfinite
            }
            4 => {
                fixture.ingredients.notes[2].ordinary_lower[1] = 3.0;
                fixture.ingredients.notes[2].ordinary_upper[1] = 2.0;
                Decline::Nonfinite
            }
            5 => {
                fixture.note_mut(2).note_type = 99;
                Decline::Nonfinite
            }
            6 => {
                fixture.note_mut(2).score_type = 99;
                Decline::Nonfinite
            }
            7 => {
                fixture.ingredients.notes[2].ordinary_lower[1] = -10.0;
                fixture.ingredients.notes[2].ordinary_upper[1] = -9.0;
                Decline::Magnitude
            }
            8 => {
                fixture.calc.assist_factor = 1.0;
                fixture.ingredients.notes[2].ordinary_lower[1] = 1e20;
                fixture.ingredients.notes[2].ordinary_upper[1] = 2e20;
                Decline::Magnitude
            }
            _ => unreachable!(),
        };
        for linked in [false, true] {
            let kernel = fixture.kernel(linked);
            let (result, hits) = compare(&kernel, &fixture.ingredients.notes);
            assert_eq!(result, Err(expected), "case {kind}, linked {linked}");
            assert_eq!(hits, 1, "only the preceding complete duplicate may be reused");
        }
    }
}

#[test]
fn terminal_kernel_memo_keeps_unlinked_probe_lower_support_refusal() {
    let mut fixture = Fixture::new(4);
    fixture.ingredients.notes[2].ordinary_lower[1] = 0.03125;
    fixture.ingredients.notes[2].ordinary_upper[1] = 0.5625;
    // Unlinked note-up permits subtraction of the 1/4 probe amplitude. Together with
    // Perfect's 1/8 contribution, its lower score is negative; linked classes are positive.
    let (unlinked, hits) = compare(&fixture.kernel(false), &fixture.ingredients.notes);
    assert_eq!(unlinked, Err(Decline::Magnitude));
    assert_eq!(hits, 1);
    let (linked, hits) = compare(&fixture.kernel(true), &fixture.ingredients.notes);
    assert_eq!(linked.unwrap().len(), 4);
    assert_eq!(hits, 1);
}

#[test]
fn terminal_kernel_memo_preserves_initial_periodic_and_final_cancellation() {
    let fixture = Fixture::new(129);
    let kernel = fixture.kernel(true);
    let (complete, hits) = compare(&kernel, &fixture.ingredients.notes);
    assert_eq!(complete.unwrap().len(), 129);
    assert_eq!(hits, 128);
    // Existing polling: before notes 0, 64, 128 and after the complete loop. Hits never skip these checks.
    for stop in 1..=4 {
        let mut polls = 0;
        let mut memo_hits = 0;
        let actual = kernel.rows_inner::<true>(
            &fixture.ingredients.notes,
            || {
                polls += 1;
                polls == stop
            },
            &mut || memo_hits += 1,
        );
        assert_eq!(actual, Err(Decline::Cancelled));
        assert_eq!(polls, stop);
        assert_eq!(memo_hits, [0, 63, 127, 128][stop - 1]);
        let mut polls = 0;
        let original = kernel.rows_inner::<false>(
            &fixture.ingredients.notes,
            || {
                polls += 1;
                polls == stop
            },
            &mut || panic!("disabled memo hit"),
        );
        assert_eq!(actual, original);
        assert_eq!(polls, stop);
    }
    // An abandoned invocation never leaves a previous result in the immutable Kernel.
    assert_eq!(compare(&kernel, &fixture.ingredients.notes).1, 128);
}
