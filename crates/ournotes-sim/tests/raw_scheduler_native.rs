//! Replays reference vectors of the client's note scheduler.
#![cfg(feature = "native-fixtures")]
mod reference;
use ournotes_sim::live::{raw::*, raw_input::ScreenFrame, raw_scheduler::*};
use serde_json::Value;
use std::collections::BTreeMap;
#[test]
fn native_generic_replays_and_line_decisions() {
    let data: Value = reference::json("raw_scheduler_native.json");
    let int = |v: &Value, k: &str| v[k].as_i64().unwrap() as i32;
    for seq in data["sequences"].as_array().unwrap() {
        let n = ScheduledNote {
            id: 1,
            operate_type: 1,
            chart_ms: 1000,
            judgement_type: 0,
            lane_min: 0.,
            lane_max: 0.,
            lane_count: 1,
            cached_before_ms: 100,
            cached_after_ms: 100,
        };
        let t = SchedulerTiming {
            base: vec![TimingUnit::new(6, 10, 10), TimingUnit::new(5, 30, 30), TimingUnit::new(1, 100, 100)],
            effective_before_ms: 100,
            effective_after_ms: int(seq, "after"),
        };
        let settings = SchedulerSettings {
            before_playing_ms: 1000,
            input_timing_ms: int(seq, "input_offset"),
            simulator_lane_count: 1,
            area_offset: 0.,
            music_end_ms: 5000,
            input_capacity: 1,
        };
        let mut scheduler = RawScheduler::new(vec![n], vec![], settings, BTreeMap::from([(0, t)])).unwrap();
        for step in seq["steps"].as_array().unwrap() {
            let now = int(step, "now");
            let frame = ScreenFrame {
                real_time_ms: now + 4000,
                music_time_ms: now,
                units: vec![(
                    InputUnit { index: 0, state: InputState::None, lane: -1., time_ms: now + 4000 },
                    FlickUnit { active: false, lane: -1., delta: Vec2::default() },
                )],
                finger_indices: vec![],
            };
            let out = scheduler.step(&frame, FrameOptions::default()).unwrap();
            assert_eq!(out.notes[0].state as i32, int(step, "state"), "{step}");
            assert_eq!(out.notes[0].frame_diff_ms, int(step, "diff"), "{step}");
            assert_eq!(out.notes[0].progress, step["progress"].as_f64().unwrap() as f32, "{step}");
            assert_eq!(!out.near_note_ids.is_empty(), step["near"].as_bool().unwrap(), "{step}");
            assert_eq!(!out.changed_note_ids.is_empty(), step["changed"].as_bool().unwrap(), "{step}");
            assert_eq!(!out.judgements.is_empty(), step["judged"].as_bool().unwrap(), "{step}");
        }
    }
    for case in data["line_cases"].as_array().unwrap() {
        let input = InputUnit {
            index: 0,
            state: match int(case, "input_state") {
                0 => InputState::None,
                1 => InputState::Enter,
                2 => InputState::Press,
                3 => InputState::Exit,
                _ => panic!(),
            },
            lane: case["lane"].as_f64().unwrap() as f32,
            time_ms: 0,
        };
        let recent = if case["recent_miss"].as_bool().unwrap() { vec![1] } else { vec![] };
        let got = line_judgement_decision(
            int(case, "now"),
            1000,
            int(case, "start_grade"),
            case["auto"].as_bool().unwrap(),
            &recent,
            &[(input, FlickUnit { active: false, lane: 0., delta: Vec2::default() })],
            0,
            1,
        )
        .unwrap_or((false, true));
        assert_eq!(got, (case["result"][0].as_bool().unwrap(), case["result"][1].as_bool().unwrap()), "{case}");
    }
    for case in data["priority_cases"].as_array().unwrap() {
        assert_eq!(
            priority_note(Some((0, int(case, "first"))), (1, int(case, "second"))).0,
            int(case, "winner") as usize
        );
    }
    let units = vec![
        TimingUnit::new(6, 10, 10),
        TimingUnit::new(5, 30, 40),
        TimingUnit::new(4, 60, 70),
        TimingUnit::new(1, 100, 100),
    ];
    for case in data["clamp_cases"].as_array().unwrap() {
        assert_eq!(clamp_auto_timing(int(case, "diff"), &units), int(case, "result"));
    }
}
