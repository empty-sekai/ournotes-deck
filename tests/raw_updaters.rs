use ournotes_deck::live::raw::{
    AutoInput, FlickUnit, InputState, InputUnit, JudgementInfo, NoteState, TimingUnit, Vec2,
};
use ournotes_deck::live::raw_updaters::{RawUpdater, UpdaterContext, UpdaterJudgement};
use serde_json::Value;
fn context() -> UpdaterContext {
    UpdaterContext {
        chart_ms: 1000,
        judgement_type: 5,
        state: NoteState::Before,
        frame_diff_ms: -20,
        input_music_ms: 980,
        frame_music_ms: 980,
        before_ms: 100,
        after_ms: 100,
        perfect_after_ms: 30,
        lane_min: 2.,
        lane_max: 3.,
        lane_count: 24,
        area_offset: 0.,
        easy_flick: false,
    }
}
fn input(state: InputState) -> InputUnit {
    InputUnit { index: 4, state, lane: 2.5, time_ms: 99999 }
}
fn units() -> Vec<TimingUnit> {
    vec![
        TimingUnit::new(6, 10, 10),
        TimingUnit::new(5, 30, 30),
        TimingUnit::new(4, 60, 60),
        TimingUnit::new(1, 100, 100),
    ]
}
#[test]
fn flick_reserves_prior_time_without_finger_ownership() {
    let mut u = RawUpdater::new(40).unwrap();
    let mut c = context();
    assert!(
        u.prepare_candidate(
            input(InputState::Press),
            Some(FlickUnit { active: true, lane: 2.5, delta: Vec2 { x: -100., y: 0. } }),
            &c,
            AutoInput::default()
        )
        .is_none()
    );
    c.state = NoteState::Just;
    c.input_music_ms = 1005;
    let candidate = u.reserved_flick_candidate(&c, AutoInput::default()).unwrap();
    assert_eq!(candidate.time_ms, 980);
    let j = u.finish_candidate(candidate, &c, &units(), AutoInput::default()).unwrap();
    assert_eq!(j.result.origin_diff_ms, -20);
    assert!(!j.direction_mismatch);
    assert!(!j.is_easy_flick);
    c.state = NoteState::First;
    u.prepare_candidate(input(InputState::None), None, &c, AutoInput::default());
    assert!(!u.near_flick);
    assert_eq!(u.near_flick_time_ms, 980);
}
#[test]
fn trace_before_reserves_even_losing_candidate_and_exit_keeps_time() {
    for kind in [21, 60, 61, 62, 63, 104, 105, 120] {
        let mut u = RawUpdater::new(kind).unwrap();
        let mut c = context();
        assert!(u.prepare_candidate(input(InputState::Press), None, &c, AutoInput::default()).is_none());
        assert!(u.trace_reserved);
        c.state = NoteState::After;
        c.input_music_ms = 1017;
        let j = u.prepare_candidate(input(InputState::Press), None, &c, AutoInput::default()).unwrap();
        assert_eq!(j.time_ms, 1000);
        let j = u.prepare_candidate(input(InputState::Exit), None, &c, AutoInput::default()).unwrap();
        assert_eq!(j.time_ms, 1017);
    }
}
#[test]
fn easy_flick_invalidation_and_auto_bypass() {
    let mut u = RawUpdater::new(40).unwrap();
    let mut c = context();
    c.easy_flick = true;
    assert!(u.prepare_candidate(input(InputState::Press), None, &c, AutoInput::default()).is_none());
    assert!(u.easy_flicked);
    c.state = NoteState::After;
    c.input_music_ms = 1031;
    let d = u.prepare_candidate(input(InputState::Press), None, &c, AutoInput::default()).unwrap();
    assert_eq!(d.time_ms, 980);
    assert!(u.finish_candidate(d, &c, &units(), AutoInput::default()).unwrap().is_easy_flick);
    c.input_music_ms = 1000;
    u.prepare_candidate(input(InputState::None), None, &c, AutoInput::default());
    assert!(!u.easy_flicked);
    let d = u
        .prepare_candidate(input(InputState::None), None, &c, AutoInput { enabled: true, ..Default::default() })
        .unwrap();
    assert_eq!(d.time_ms, 1000);
}
#[test]
fn hold_release_and_special_last_and_force() {
    let mut u = RawUpdater::new(22).unwrap();
    let mut c = context();
    c.state = NoteState::After;
    c.frame_diff_ms = 20;
    c.input_music_ms = 1020;
    assert_eq!(u.prepare_candidate(input(InputState::Press), None, &c, AutoInput::default()).unwrap().time_ms, 1000);
    assert_eq!(u.prepare_candidate(input(InputState::Exit), None, &c, AutoInput::default()).unwrap().time_ms, 1020);
    for kind in [80, 82, 100, 103, 121, 122] {
        let u = RawUpdater::new(kind).unwrap();
        assert_eq!(u.last_judgement(kind, 1, 1150).unwrap().result.origin, 7);
    }
    for kind in [21, 63, 120] {
        let u = RawUpdater::new(kind).unwrap();
        assert_eq!(u.last_judgement(122, 21, 1150).unwrap().result.origin, 7);
        assert!(u.last_judgement(0, 21, 1150).is_err());
    }
    assert!(RawUpdater::new(123).is_err());
    let j = RawUpdater::forced_result(JudgementInfo {
        judgement_type: 0,
        judgement: 1,
        time_ms: 1000,
        timing: 6,
        diff_ms: i32::MAX,
    });
    assert_eq!(j.result.timing, 6);
}
#[test]
fn replay_original_arm64_vectors() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/raw_updaters_native.json");
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(path).expect("native updater fixture")).unwrap();
    assert!(cases.len() > 2000);
    for (index, case) in cases.iter().enumerate() {
        let mut u: RawUpdater = serde_json::from_value(case["before"].clone()).unwrap();
        let mut ctx = case["context"].clone();
        ctx["state"] = Value::from(
            ["Wait", "First", "Before", "Just", "After", "Last", "Done"][ctx["state"].as_u64().unwrap() as usize],
        );
        let c: UpdaterContext = serde_json::from_value(ctx).unwrap();
        let auto: AutoInput = if case["auto"].is_null() {
            AutoInput::default()
        } else {
            serde_json::from_value(case["auto"].clone()).unwrap()
        };
        let got = match case["action"].as_str().unwrap() {
            "candidate" | "reserved" => {
                let d = if case["action"] == "candidate" {
                    let mut inp = case["input"].clone();
                    inp["state"] =
                        Value::from(["None", "Enter", "Press", "Exit"][inp["state"].as_u64().unwrap() as usize]);
                    u.prepare_candidate(
                        serde_json::from_value(inp).unwrap(),
                        serde_json::from_value(case["flick"].clone()).unwrap(),
                        &c,
                        auto,
                    )
                } else {
                    u.reserved_flick_candidate(&c, auto)
                };
                assert_eq!(serde_json::to_value(d).unwrap(), case["candidate"], "candidate {index}");
                // Compare as structs: the fixture holds native f32 values widened to f64 (1.4989999532699585),
                // while serde_json writes an f32 in its shortest form (1.499).
                let after: RawUpdater = serde_json::from_value(case["after"].clone()).unwrap();
                assert_eq!(u, after, "cache {index}");
                d.and_then(|d| u.finish_candidate(d, &c, &units(), auto))
            }
            "last" => Some(
                u.last_judgement(
                    case["current_operate_type"].as_i64().unwrap() as i32,
                    c.judgement_type,
                    c.frame_music_ms,
                )
                .unwrap(),
            ),
            x => panic!("unknown action {x}"),
        };
        let expected: Option<UpdaterJudgement> = serde_json::from_value(case["result"].clone()).unwrap();
        assert_eq!(got, expected, "result {index}");
    }
}
