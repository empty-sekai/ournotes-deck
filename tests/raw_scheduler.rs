use ournotes_deck::live::{raw::*, raw_input::ScreenFrame, raw_scheduler::*};
use std::collections::BTreeMap;
fn note(id: i32, op: i32, time: i32) -> ScheduledNote {
    ScheduledNote {
        id,
        operate_type: op,
        chart_ms: time,
        judgement_type: 0,
        lane_min: 0.,
        lane_max: 0.,
        lane_count: 1,
        cached_before_ms: 100,
        cached_after_ms: 100,
    }
}
fn frame(time: i32, state: InputState) -> ScreenFrame {
    ScreenFrame {
        real_time_ms: time + 4000,
        music_time_ms: time,
        units: vec![
            (
                InputUnit { index: 0, state, lane: 0., time_ms: time + 4000 },
                FlickUnit { active: false, lane: 0., delta: Vec2::default() }
            );
            8
        ],
        finger_indices: vec![],
    }
}
fn scheduler(notes: Vec<ScheduledNote>, lines: Vec<ScheduledLine>) -> RawScheduler {
    let t = SchedulerTiming {
        base: vec![
            TimingUnit::new(6, 10, 10),
            TimingUnit::new(5, 30, 30),
            TimingUnit::new(4, 60, 60),
            TimingUnit::new(1, 100, 100),
        ],
        effective_before_ms: 100,
        effective_after_ms: 100,
    };
    RawScheduler::new(
        notes,
        lines,
        SchedulerSettings {
            before_playing_ms: 1000,
            input_timing_ms: 0,
            simulator_lane_count: 1,
            area_offset: 0.,
            music_end_ms: 5000,
            input_capacity: 8,
        },
        BTreeMap::from([(0, t)]),
    )
    .unwrap()
}
fn line() -> ScheduledLine {
    ScheduledLine {
        id: 10,
        start_note_id: 1,
        start_ms: 1000,
        end_ms: 2000,
        lane_min: 0,
        lane_max: 0,
        note_ids: vec![1, 2],
    }
}
#[test]
fn frame_states_are_single_dispatch_and_last_is_next_frame() {
    let mut s = scheduler(vec![note(1, 1, 1000)], vec![]);
    let mut got = vec![];
    for t in [0, 1, 2, 999, 1000, 1000, 1100, 1101, 1102] {
        let o = s.step(&frame(t, InputState::None), FrameOptions::default()).unwrap();
        got.push(o.notes[0].state);
        if t == 1101 {
            assert!(o.judgements.is_empty());
        }
        if t == 1102 {
            assert_eq!(
                (o.judgements[0].judgement.result.timing, o.judgements[0].judgement.result.diff_ms),
                (5, i32::MAX)
            );
        }
    }
    assert_eq!(
        got,
        vec![
            NoteState::Wait,
            NoteState::First,
            NoteState::Before,
            NoteState::Before,
            NoteState::Just,
            NoteState::After,
            NoteState::After,
            NoteState::Last,
            NoteState::Done
        ]
    );
}
#[test]
fn wait_late_does_not_write_offset_and_judgement_progress_gate_matters() {
    let mut s = scheduler(vec![note(1, 1, 1000)], vec![]);
    s.timings.get_mut(&0).unwrap().effective_after_ms = 20;
    let o = s.step(&frame(1099, InputState::None), FrameOptions::default()).unwrap();
    assert_eq!(o.notes[0].state, NoteState::After);
    let mut s = scheduler(vec![note(1, 1, 1000)], vec![]);
    let o = s.step(&frame(1300, InputState::None), FrameOptions::default()).unwrap();
    assert_eq!(o.notes[0].state, NoteState::Last);
    assert_eq!(o.notes[0].frame_diff_ms, 0);
    assert!(o.judgements.is_empty());
}
#[test]
fn raw_screen_competition_is_deduplicated_stable_and_preserves_origin() {
    let mut s = scheduler(vec![note(1, 1, 990), note(2, 1, 1010), note(3, 22, 1000)], vec![]);
    let o = s
        .step_with(&frame(1000, InputState::Enter), FrameOptions::default(), |e| {
            let mut r = e.judgement.result;
            r.judgement = 4;
            r.diff_ms = 77;
            Ok(r)
        })
        .unwrap();
    assert_eq!(o.near_note_ids, vec![1, 2, 3]);
    assert_eq!(o.judged_note_ids, vec![1]);
    assert_eq!(o.judgements[0].judgement.result.judgement, 6);
    assert_eq!(o.notes[0].result.unwrap().judgement, 4);
    assert_eq!(o.notes[0].result.unwrap().origin, 6);
}
#[test]
fn minimum_rejects_winner_without_fallback_and_last_bypasses_it() {
    let mut s = scheduler(vec![note(1, 1, 1015), note(2, 1, 1020)], vec![]);
    let options = FrameOptions { auto: AutoInput { minimum: 6, ..Default::default() }, ..Default::default() };
    assert!(s.step(&frame(1000, InputState::Enter), options).unwrap().judgements.is_empty());
    for t in [1001, 1020, 1021, 1200] {
        s.step(&frame(t, InputState::None), options).unwrap();
    }
    assert_eq!(s.step(&frame(1201, InputState::None), options).unwrap().judgements.len(), 2);
}
#[test]
fn all_factory_types_auto_and_timeout_are_routed_without_double_conversion() {
    let types = [1, 20, 21, 22, 40, 41, 42, 60, 61, 62, 63, 80, 82, 100, 101, 102, 103, 104, 105, 120, 121, 122];
    for op in types {
        let mut s = scheduler(vec![note(1, op, 1000)], vec![]);
        let opts = FrameOptions {
            auto: AutoInput { enabled: true, use_timing: false, judgement: 5, minimum: -1 },
            ..Default::default()
        };
        let o = s.step(&frame(1000, InputState::None), opts).unwrap();
        if matches!(op, 80 | 82 | 121 | 122) {
            assert!(o.judgements.is_empty());
            s.step(&frame(1001, InputState::None), opts).unwrap();
            s.step(&frame(1200, InputState::None), opts).unwrap();
            assert_eq!(
                s.step(&frame(1201, InputState::None), opts).unwrap().judgements[0].judgement.result.judgement,
                7
            );
        } else {
            assert_eq!(o.judgements.len(), 1, "op={op}");
            assert_eq!(o.judgements[0].judgement.result.diff_ms, i32::MAX);
        }
    }
}
#[test]
fn line_feedback_uses_converted_grade_and_hold_overrides_miss() {
    let mut s = scheduler(vec![note(1, 20, 1000), note(2, 1, 1200)], vec![line()]);
    let mut f = frame(1000, InputState::Enter);
    f.units[0].0.lane = 0.4;
    for (u, _) in &mut f.units[1..] {
        u.lane = -1.;
    }
    let o = s
        .step_with(&f, FrameOptions::default(), |e| {
            let mut r = e.judgement.result;
            r.judgement = 1;
            Ok(r)
        })
        .unwrap();
    assert!(!o.lines[0].enabled);
    assert!(o.lines[0].missed);
    let o = s.step(&frame(1001, InputState::Press), FrameOptions::default()).unwrap();
    assert!(o.lines[0].enabled);
    assert!(!o.lines[0].missed);
    let o = s.step(&frame(1200, InputState::Exit), FrameOptions::default()).unwrap();
    assert!(o.lines[0].enabled, "no decision preserves previous enable");
}
#[test]
fn line_end_force_and_retained_post_end_frame() {
    let mut l = line();
    l.note_ids = vec![1];
    l.end_ms = 1000;
    let mut s = scheduler(vec![note(1, 100, 1000)], vec![l]);
    let o = s.step(&frame(1001, InputState::None), FrameOptions::default()).unwrap();
    let r = o.judgements[0].judgement.result;
    assert_eq!((r.judgement_type, r.judgement, r.timing, r.diff_ms), (0, 7, 6, i32::MAX));
    assert!(o.all_notes_done);
    let o = s.step(&frame(7001, InputState::Enter), FrameOptions::default()).unwrap();
    assert!(o.beyond_end);
    assert_eq!(o.judgements.len(), 1);
}
#[test]
fn auto_override_is_clamped_and_cached_and_flick_reservation_survives_frames() {
    let mut s = scheduler(vec![note(1, 1, 1000)], vec![]);
    let options = FrameOptions {
        auto: AutoInput { enabled: true, ..Default::default() },
        auto_timing_override_ms: Some(500),
        ..Default::default()
    };
    assert!(s.step(&frame(1000, InputState::None), options).unwrap().judgements.is_empty());
    let options = FrameOptions { auto_timing_override_ms: Some(-500), ..options };
    let o = s.step(&frame(1060, InputState::None), options).unwrap();
    assert_eq!(o.judgements[0].judgement.result.origin_diff_ms, 60);
    let mut s = scheduler(vec![note(1, 40, 1000)], vec![]);
    s.step(&frame(900, InputState::None), FrameOptions::default()).unwrap();
    let mut f = frame(950, InputState::Press);
    f.units[0].1.active = true;
    f.units[0].1.delta = Vec2 { x: 20., y: 0. };
    assert!(s.step(&f, FrameOptions::default()).unwrap().judgements.is_empty());
    let o = s.step(&frame(1000, InputState::None), FrameOptions::default()).unwrap();
    assert_eq!(o.judgements[0].judgement.result.diff_ms, -50);
}
