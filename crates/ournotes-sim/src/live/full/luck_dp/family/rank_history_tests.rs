//! Fail-closed tests for the separate original-query geometry capability.
use super::*;
use crate::live::full::PlayFrame;
use crate::live::full::luck_score_bounds::ComboObserver;
use crate::live::full::scorecalc::NoteCommand;
use crate::live::skill::FactorCommand;

fn append_frame(trace: &mut BoundsTrace, time: i32, notes: &[i32], rank: Option<(usize, i32, i32)>) {
    for (index, &time_ms) in notes.iter().enumerate() {
        trace.events.push(BoundsEvent::Note {
            frame: get_frame(time_ms) as usize,
            index,
            note: NoteCommand::new(time_ms, 1000, 1, 1, 5),
        });
    }
    for _ in 0..2 {
        trace.events.push(BoundsEvent::Query { time_ms: time, to: get_frame(time) });
        trace.queries += 1;
    }
    trace.events.push(BoundsEvent::ProbabilityReady(time));
    if let Some((range, start_time, end_time)) = rank {
        let start = trace.queries;
        for time_ms in [start_time, end_time] {
            trace.events.push(BoundsEvent::Query { time_ms, to: get_frame(time_ms) });
            trace.queries += 1;
        }
        trace.events.push(BoundsEvent::Rank {
            range,
            time_ms: end_time,
            percent: 25,
            start: Some(start),
            end: Some(start + 1),
        });
    }
}

fn geometry() -> (BoundsTrace, LivePlay, GekisouSetup) {
    let mut trace = BoundsTrace {
        events: Vec::new(),
        queries: 0,
        frames: 8,
        probes: Vec::new(),
        combo: ComboObserver::default(),
        has_luck: true,
        filing_gate: Some(Some(M_LUCK)),
        probe_filings: None,
    };
    append_frame(&mut trace, 0, &[], None);
    append_frame(&mut trace, 40, &[], None);
    append_frame(&mut trace, 80, &[41, 79, 80, 80], Some((0, 0, 80)));
    append_frame(&mut trace, 100, &[], None);
    let play = LivePlay {
        frames: [0, 40, 80, 100].map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }).into(),
        base_seed: 0,
    };
    (trace, play, GekisouSetup { fevers: vec![(0, 80)], missions: vec![M_LUCK; 3] })
}

#[test]
fn rank_probe_history_geometry_keeps_closed_frames_repeated_times_and_original_queries() {
    let (trace, play, setup) = geometry();
    assert!(build(&trace, &play, &setup, 200, &mut || false).unwrap().is_some());
    // Both notes at exactly 80 and the earlier notes in closed native frames remain represented.
    assert_eq!(trace.events.iter().filter(|event| matches!(event, BoundsEvent::Note { .. })).count(), 4);
    assert_eq!(trace.queries, 2 * (play.frames.len() + 1));
    assert!(build(&trace, &play, &setup, 200, &mut || true).is_none());
    let mut polls = 0;
    assert!(
        build(&trace, &play, &setup, 200, &mut || {
            polls += 1;
            polls >= 5
        })
        .is_none()
    );
}

#[test]
fn rank_probe_history_geometry_refuses_any_missing_filing_or_readiness_in_the_whole_history() {
    let (original, play, setup) = geometry();
    for defect in ["missing ready", "between-query ready", "intervening command", "bad frame", "missing original frame"]
    {
        let mut trace = original.clone();
        match defect {
            "missing ready" => trace.events.retain(|event| !matches!(event, BoundsEvent::ProbabilityReady(80))),
            "between-query ready" => {
                let ready =
                    trace.events.iter().position(|event| matches!(event, BoundsEvent::ProbabilityReady(80))).unwrap();
                let event = trace.events.remove(ready);
                trace.events.insert(ready + 1, event);
            }
            "intervening command" => {
                let rank = trace.events.iter().position(|event| matches!(event, BoundsEvent::Rank { .. })).unwrap();
                trace.events.insert(
                    rank - 1,
                    BoundsEvent::Factor {
                        frame: 1,
                        command: FactorCommand { time_ms: 40, owner_id: 1, note_mill: 100, ..Default::default() },
                    },
                );
            }
            "bad frame" => {
                let BoundsEvent::Note { frame, .. } =
                    trace.events.iter_mut().find(|event| matches!(event, BoundsEvent::Note { .. })).unwrap()
                else {
                    unreachable!()
                };
                *frame += 1;
            }
            "missing original frame" => {
                trace.events.retain(|event| !matches!(event, BoundsEvent::ProbabilityReady(100)))
            }
            _ => unreachable!(),
        }
        assert!(build(&trace, &play, &setup, 200, &mut || false).unwrap().is_none(), "{defect}");
    }
    assert!(
        build(&original, &play, &setup, 79, &mut || false).unwrap().is_none(),
        "future clamped inverse can backfill history"
    );
    let mut repeated = play.clone();
    repeated.frames[3].time_ms = 80;
    assert!(build(&original, &repeated, &setup, 200, &mut || false).unwrap().is_none());
}

#[test]
fn rank_probe_history_geometry_refuses_a_later_note_in_the_same_native_ceil_frame() {
    let (mut trace, mut play, mut setup) = geometry();
    trace.events.clear();
    trace.queries = 0;
    append_frame(&mut trace, 0, &[], None);
    append_frame(&mut trace, 40, &[], None);
    append_frame(&mut trace, 70, &[41, 60], Some((0, 0, 60)));
    append_frame(&mut trace, 80, &[79], None);
    play.frames = [0, 40, 70, 80].map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }).into();
    setup.fevers[0] = (0, 60);
    assert_eq!(get_frame(60), get_frame(79), "native frame 2 is (40,80]");
    assert!(trace.events.iter().all(|event| match event {
        BoundsEvent::Note { frame, note, .. } => *frame == get_frame(note.time_ms) as usize,
        _ => true,
    }));
    assert_eq!(crate::live::full::luck_score_bounds::rank_history_structure_ready(&trace, &mut || false), Some(true));
    assert!(
        build(&trace, &play, &setup, 200, &mut || false).unwrap().is_none(),
        "the final-ready note was absent from the historical query"
    );
}

#[test]
fn rank_probe_history_geometry_reuses_fixed_identity_cancellation_and_refuses_partial_queries() {
    let (mut trace, mut play, mut setup) = geometry();
    append_frame(&mut trace, 120, &[120], Some((1, 40, 120)));
    append_frame(&mut trace, 160, &[], None);
    play.frames.extend([120, 160].map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }));
    setup.fevers.push((40, 120));
    assert_eq!(crate::live::full::luck_score_bounds::rank_history_structure_ready(&trace, &mut || false), Some(false));
    assert!(
        build(&trace, &play, &setup, 200, &mut || false).unwrap().is_none(),
        "previous fixed bonus crosses the second signed query difference"
    );

    let (mut trace, play, setup) = geometry();
    let BoundsEvent::Rank { start, .. } =
        trace.events.iter_mut().find(|event| matches!(event, BoundsEvent::Rank { .. })).unwrap()
    else {
        unreachable!()
    };
    *start = Some(0);
    assert!(build(&trace, &play, &setup, 200, &mut || false).unwrap().is_none());
}

#[test]
fn rank_rush_history_geometry_keeps_current_frame_filings_and_is_independent_of_probe_clamping() {
    let (mut trace, play, setup) = geometry();
    let ready = trace.events.iter().position(|event| matches!(event, BoundsEvent::ProbabilityReady(80))).unwrap();
    trace.events.insert(ready, BoundsEvent::Potential { frame: get_frame(80) as usize });
    trace.events.insert(
        ready,
        BoundsEvent::Factor {
            frame: get_frame(80) as usize,
            command: FactorCommand { time_ms: 80, owner_id: -1, luck: 47, ..Default::default() },
        },
    );
    let (probe, rush) = build_both(&trace, &play, &setup, 200, &mut || false).unwrap();
    assert!(probe.is_some() && rush.is_some(), "same-frame commands before Ready are already historical");
    let (probe, rush) = build_both(&trace, &play, &setup, 79, &mut || false).unwrap();
    assert!(probe.is_none(), "the future direct probe inverse could be music-clamped");
    assert!(rush.is_some(), "Rush FINISH uses its original frame time, not the probe's music clamp");
    assert!(build_both(&trace, &play, &setup, 200, &mut || true).is_none());
    let mut polls = 0;
    assert!(
        build_both(&trace, &play, &setup, 200, &mut || {
            polls += 1;
            polls >= 5
        })
        .is_none(),
        "an interrupted geometry check publishes neither witness"
    );
}

#[test]
fn rank_rush_history_geometry_refuses_future_potential_and_before_frame_inverse_at_closed_frame() {
    let (original, play, setup) = geometry();
    for time in [79, 80, 81] {
        for factor in [false, true] {
            let mut trace = original.clone();
            let at =
                trace.events.iter().position(|event| matches!(event, BoundsEvent::Query { time_ms: 100, .. })).unwrap();
            let frame = get_frame(time) as usize;
            let event = if factor {
                // Actual Luck commands remain conservative evidence even without companion metadata.
                BoundsEvent::Factor {
                    frame,
                    command: FactorCommand { time_ms: time, owner_id: -1, luck: -47, ..Default::default() },
                }
            } else {
                BoundsEvent::Potential { frame }
            };
            trace.events.insert(at, event);
            let (probe, rush) = build_both(&trace, &play, &setup, 200, &mut || false).unwrap();
            assert!(probe.is_some(), "the Rush-only defect does not erase probe geometry");
            assert_eq!(rush.is_some(), time == 81, "factor={factor} time={time}");
        }
    }
    let mut unproved = original.clone();
    unproved.filing_gate = None;
    let (probe, rush) = build_both(&unproved, &play, &setup, 200, &mut || false).unwrap();
    assert!(probe.is_some() && rush.is_none(), "unclassified recorder potentials cannot prove the superset");
}

#[test]
fn rank_rush_history_geometry_refuses_late_notes_changed_fixed_coefficients_and_partial_queries() {
    let (mut trace, mut play, mut setup) = geometry();
    trace.events.clear();
    trace.queries = 0;
    append_frame(&mut trace, 0, &[], None);
    append_frame(&mut trace, 40, &[], None);
    append_frame(&mut trace, 70, &[41, 60], Some((0, 0, 60)));
    append_frame(&mut trace, 80, &[79], None);
    play.frames = [0, 40, 70, 80].map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }).into();
    setup.fevers[0] = (0, 60);
    assert_eq!(get_frame(60), get_frame(79));
    assert!(build_both(&trace, &play, &setup, 200, &mut || false).unwrap().1.is_none());

    let (mut trace, mut play, mut setup) = geometry();
    append_frame(&mut trace, 120, &[120], Some((1, 40, 120)));
    append_frame(&mut trace, 160, &[], None);
    play.frames.extend([120, 160].map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }));
    setup.fevers.push((40, 120));
    assert!(build_both(&trace, &play, &setup, 200, &mut || false).unwrap().1.is_none());

    let (mut trace, play, setup) = geometry();
    trace.events.pop(); // The final frame no longer has its original ProbabilityReady anchor.
    assert!(build_both(&trace, &play, &setup, 200, &mut || false).unwrap().1.is_none());
}
