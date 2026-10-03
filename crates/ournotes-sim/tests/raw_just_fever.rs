//! Gekisou's wrapper must toggle Just before raw note grading and percent-window appliers.
use ournotes_sim::live::full::{GekisouSetup, LiveModel, LiveNote, LiveParams, RawJudgementRuntime};
use ournotes_sim::live::raw::TimingUnit;
use ournotes_sim::live::raw_windows::TimingSet;
use ournotes_sim::master::Master;
use serde_json::{Value, json};

fn model(mission: i64, force: bool) -> LiveModel {
    let data: Value = serde_json::from_str(include_str!("fixtures/raw_bridge_master.json")).unwrap();
    let tables: Vec<_> =
        data.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    let master = Master::from_json_tables(|n| tables.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str())).unwrap();
    let note = LiveNote { note_id: 1, time_ms: 150, note_operate_type: 1, judgement_type: 5 };
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 200000,
        music_level: 25,
        converted_note_count: 1,
        music_length_ms: 1000,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers: vec![(100, 200)], missions: vec![mission; 3] };
    let mut model = LiveModel::new_gekisou(&master, &[], &[note], &[], params, &setup).unwrap();
    let mut just = TimingUnit::new(6, 2, 2);
    just.enabled = force;
    let mut runtime = RawJudgementRuntime::new(
        vec![TimingSet { judgement_type: 5, units: vec![just, TimingUnit::new(1, 0, 130)] }],
        0,
        0,
    );
    runtime.force_enable_just_judgement = force;
    model.enable_raw_runtime(runtime).unwrap();
    model
}

#[test]
fn just_fever_enables_before_raw_notes_and_percent_miss_cap_then_disables_at_end() {
    let mut model = model(3, false);
    model.begin_raw_frame(99, 0.016).unwrap();
    assert!(!model.raw_runtime().unwrap().windows.timings[0].units[0].enabled);
    model.finish_raw_frame().unwrap();
    model.begin_raw_frame(100, 0.016).unwrap();
    assert!(model.raw_runtime().unwrap().windows.timings[0].units[0].enabled);
    let windows = &mut model.raw_runtime_mut().unwrap().windows;
    let handles = windows.percent(&[6], 1.0);
    // Actual native type 5 / 12: Miss early bound 0 caps the Just early increment to zero.
    assert_eq!((windows.timings[0].units[0].before_ms, windows.timings[0].units[0].after_ms), (2, 4));
    windows.disable_percent(&handles);
    model.finish_raw_frame().unwrap();
    model.begin_raw_frame(200, 0.016).unwrap();
    assert!(!model.raw_runtime().unwrap().windows.timings[0].units[0].enabled);
    model.finish_raw_frame().unwrap();
}

#[test]
fn just_end_restores_force_setting_and_combo_ranges_do_not_change_it() {
    for mission in [1, 3] {
        let mut model = model(mission, true);
        model.frame_raw_timed(100, &[], 0.016).unwrap();
        model.begin_raw_frame(200, 0.016).unwrap();
        assert!(model.raw_runtime().unwrap().windows.timings[0].units[0].enabled);
        model.finish_raw_frame().unwrap();
    }
    let mut combo = model(1, false);
    for time in [100, 200] {
        combo.begin_raw_frame(time, 0.016).unwrap();
        assert!(!combo.raw_runtime().unwrap().windows.timings[0].units[0].enabled);
        combo.finish_raw_frame().unwrap();
    }
}
