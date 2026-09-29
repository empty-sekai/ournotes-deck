//! Replays vectors produced by executing the original ARM64 functions: LiveAssistLevelAdjuster.Update
//! 0x55c1364 and AssistLiveExecutor.OnConvertedJudgementFunc 0x55c1ea4 / ResolveAssistJudgement 0x55c2370.
use ournotes_deck::live::raw::{NoteResult, TimingUnit};
use ournotes_deck::live::raw_assist::{AssistExecutor, AssistLevelAdjuster};
use ournotes_deck::live::raw_windows::TimingSet;
use serde_json::Value;
use std::collections::BTreeMap;

fn data() -> Value {
    let path = std::env::var("ASSIST_NATIVE_FIXTURE")
        .unwrap_or_else(|_| format!("{}/tests/fixtures/raw_assist_native.json", env!("CARGO_MANIFEST_DIR")));
    serde_json::from_slice(&std::fs::read(&path).expect("run native_assist.py to install the native fixture first"))
        .unwrap()
}
fn int(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}

#[test]
fn level_adjuster_matches_native_update() {
    let data = data();
    let cases = data["level_cases"].as_array().unwrap();
    assert!(cases.len() >= 900);
    for (index, case) in cases.iter().enumerate() {
        let s = &case["start"];
        let mut a = AssistLevelAdjuster {
            level: int(&s["level"]),
            point: int(&s["point"]),
            perfect_continue: int(&s["perfect_continue"]),
            gauge_max: int(&s["gauge_max"]),
            level_max: int(&s["level_max"]),
            level_down_count: int(&s["level_down_count"]),
            judgement_points: s["judgement_points"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.parse().unwrap(), int(v)))
                .collect::<BTreeMap<i32, i32>>(),
            fixed_level: s["fixed_level"].as_i64().map(|v| v as i32),
        };
        for step in case["steps"].as_array().unwrap() {
            let notes: Vec<(i32, i32)> =
                step["notes"].as_array().unwrap().iter().map(|n| (int(&n[0]), int(&n[1]))).collect();
            let got = a.update(&notes).unwrap();
            let want = step["state"].as_array().unwrap();
            assert_eq!(got, (int(&want[0]), int(&want[1]), int(&want[2])), "level case {index}");
            assert_eq!((a.level, a.point, a.perfect_continue), got, "level case {index}");
        }
    }
}

#[test]
fn converted_judgement_hook_matches_native() {
    let data = data();
    let cases = data["convert_cases"].as_array().unwrap();
    assert!(cases.len() > 10000);
    for (index, case) in cases.iter().enumerate() {
        let levels: Vec<Vec<TimingSet>> = data["level_specs"][case["variant"].as_str().unwrap()]
            .as_array()
            .unwrap()
            .iter()
            .map(|level| {
                data["convert_judgement_types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|jt| TimingSet {
                        judgement_type: int(jt),
                        units: level
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|u| TimingUnit::new(int(&u[0]), int(&u[1]), int(&u[2])))
                            .collect(),
                    })
                    .collect()
            })
            .collect();
        let adjuster = AssistLevelAdjuster {
            level: 0,
            point: 0,
            perfect_continue: 0,
            gauge_max: 1,
            level_max: 1,
            level_down_count: 1,
            judgement_points: BTreeMap::new(),
            fixed_level: None,
        };
        let mut x = AssistExecutor::new(adjuster, vec![], 1., 3.);
        x.current_level = int(&case["current_level"]);
        let result = NoteResult {
            origin: int(&case["judgement"]),
            judgement: int(&case["judgement"]),
            judgement_type: int(&case["judgement_type"]),
            timing: 0,
            time_ms: 0,
            origin_diff_ms: int(&case["diff_ms"]),
            diff_ms: int(&case["diff_ms"]),
        };
        let got = x.convert(1, &result, case["easy"].as_bool().unwrap(), &levels);
        let want = &case["result"];
        if want["error"].as_bool() == Some(true) {
            assert!(got.is_err(), "convert case {index}: native threw, model returned {got:?}");
        } else {
            assert_eq!(got, Ok(int(&want["grade"])), "convert case {index}");
            assert_eq!(x.assisted_notes.contains(&1), want["assisted"].as_bool().unwrap(), "convert case {index}");
        }
    }
}
