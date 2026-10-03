//! Synthetic end-to-end raw result -> converter -> executor window callback -> skill Update.
use ournotes_sim::live::full::{
    GekisouSetup, LiveModel, LiveNote, LiveParams, Performer, RawJudgedNote, RawJudgementRuntime,
};
use ournotes_sim::live::raw::{NoteResult, TimingUnit};
use ournotes_sim::live::raw_windows::TimingSet;
use ournotes_sim::master::Master;
use serde_json::{Value, json};
fn model(skills: &[i64]) -> LiveModel {
    build(skills, false)
}
/// 13001 and 13005 are Gekisou appliers: they act only with Gekisou on. The
/// fever lies after the chart, so the Gekisou range never starts.
fn model_gekisou(skills: &[i64]) -> LiveModel {
    build(skills, true)
}
fn build(skills: &[i64], gekisou: bool) -> LiveModel {
    build_with_note_type(skills, gekisou, 1)
}
fn build_with_note_type(skills: &[i64], gekisou: bool, note_operate_type: i32) -> LiveModel {
    let data: Value = serde_json::from_str(include_str!("fixtures/raw_bridge_master.json")).unwrap();
    let tables: Vec<_> =
        data.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    let master = Master::from_json_tables(|n| tables.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str())).unwrap();
    let notes = [LiveNote { note_id: 1, time_ms: 1000, note_operate_type, judgement_type: 1 }];
    let deck: Vec<_> = skills.iter().map(|&id| Performer { live_skill: Some((id, 1)), ..Default::default() }).collect();
    let events: Vec<_> = skills.iter().enumerate().map(|(i, _)| (i as i32, 0)).collect();
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 200000,
        music_level: 25,
        converted_note_count: 1,
        music_length_ms: 12000,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let mut model = if gekisou {
        let setup = GekisouSetup { fevers: vec![(20000, 21000)], missions: vec![1, 2, 3] };
        LiveModel::new_gekisou(&master, &deck, &notes, &events, params, &setup).unwrap()
    } else {
        LiveModel::new(&master, &deck, &notes, &events, params).unwrap()
    };
    let timing = TimingSet {
        judgement_type: 1,
        units: vec![
            TimingUnit::new(6, 10, 10),
            TimingUnit::new(5, 30, 30),
            TimingUnit::new(4, 60, 60),
            TimingUnit::new(1, 100, 100),
        ],
    };
    model.enable_raw_runtime(RawJudgementRuntime::new(vec![timing], 50, 10)).unwrap();
    model
}
fn result(j: i32, diff: i32) -> RawJudgedNote {
    RawJudgedNote {
        note_id: 1,
        result: NoteResult {
            origin: j,
            judgement: j,
            judgement_type: 1,
            timing: 2,
            time_ms: 1020,
            origin_diff_ms: diff,
            diff_ms: diff,
        },
        direction_mismatch: false,
        is_easy_flick: false,
    }
}
fn before(m: &LiveModel, j: i32) -> i32 {
    m.raw_runtime().unwrap().windows.timings[0].units.iter().find(|u| u.judgement == j).unwrap().before()
}
#[test]
fn milliseconds_limit_runs_after_ft_not_in_submit() {
    let mut m = model(&[1]);
    m.frame_raw_timed(0, &[], 0.).unwrap();
    assert_eq!(before(&m, 6), 15);
    m.begin_raw_frame(1020, 0.016).unwrap();
    let r = m.submit_raw_judgement(result(5, 20)).unwrap();
    assert_eq!(r.judgement, 5);
    assert_eq!(before(&m, 6), 15);
    m.finish_raw_frame().unwrap();
    assert_eq!(before(&m, 6), 10);
    assert!(m.score() > 0);
}
#[test]
fn unscored_pass_still_consumes_executor_window_callback() {
    let mut m = build_with_note_type(&[1], false, 122);
    m.frame_raw_timed(0, &[], 0.).unwrap();
    assert_eq!(before(&m, 6), 15);
    m.begin_raw_frame(1020, 0.016).unwrap();
    let note = m.submit_raw_judgement(result(7, 20)).unwrap();
    assert_eq!((note.origin, note.judgement), (7, 7));
    assert_eq!(before(&m, 6), 15);
    m.finish_raw_frame().unwrap();
    // Skipping AddNoteScore must not filter the event before executor window-limit consumption.
    assert_eq!(before(&m, 6), 10);
    assert_eq!(m.score(), 0);
    assert_eq!(m.current_life(), 1000);
}
#[test]
fn just_limit_uses_converted_judgement_and_diff() {
    let mut m = model_gekisou(&[2, 4]);
    m.frame_raw_timed(0, &[], 0.).unwrap();
    assert_eq!(before(&m, 6), 35);
    m.begin_raw_frame(1020, 0.016).unwrap();
    let r = m.submit_raw_judgement(result(5, 20)).unwrap();
    assert_eq!((r.origin, r.judgement, r.origin_diff_ms, r.diff_ms), (5, 6, 20, 20));
    assert_eq!(m.raw_runtime().unwrap().just.remaining(), 1);
    m.finish_raw_frame().unwrap();
    assert_eq!(m.raw_runtime().unwrap().just.remaining(), 0);
    assert_eq!(before(&m, 6), 10);
    assert_eq!(m.frame_judgements(), &[(1, 6, 1000)]);
}
#[test]
fn diff_converter_precedes_window_limit_callback() {
    let mut m = model_gekisou(&[2, 4]);
    m.raw_runtime_mut().unwrap().diff_converter = |j, d| if j == 6 { 0 } else { d };
    m.frame_raw_timed(0, &[], 0.).unwrap();
    let r = m.frame_raw_timed(1020, &[result(5, 20)], 0.016).unwrap()[0];
    assert_eq!((r.origin_diff_ms, r.diff_ms), (20, 0));
    assert_eq!(m.raw_runtime().unwrap().just.remaining(), 1);
    assert_eq!(before(&m, 6), 35);
}
#[test]
fn just_window_and_just_conversion_do_nothing_without_gekisou() {
    let mut m = model(&[2, 4]);
    m.frame_raw_timed(0, &[], 0.).unwrap();
    assert_eq!(before(&m, 6), 10);
    let r = m.frame_raw_timed(1020, &[result(5, 20)], 0.016).unwrap()[0];
    assert_eq!((r.origin, r.judgement), (5, 5));
    assert_eq!(m.raw_runtime().unwrap().just.remaining(), 0);
    assert_eq!(before(&m, 6), 10);
}
#[test]
fn percent_effect_applies_static_targets_and_restores() {
    let mut m = model(&[3]);
    m.frame_raw_timed(0, &[], 0.).unwrap();
    assert_eq!((before(&m, 6), before(&m, 5), before(&m, 4), before(&m, 1)), (15, 45, 90, 100));
    m.frame_raw_timed(11000, &[], 0.016).unwrap();
    assert_eq!((before(&m, 6), before(&m, 5), before(&m, 4), before(&m, 1)), (10, 30, 60, 100));
}
#[test]
fn judged_stream_is_rejected_once_raw_runtime_is_enabled() {
    let mut m = model(&[1]);
    let judged = [ournotes_sim::live::full::JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 1020 }];
    assert!(m.frame(1020, &judged).is_err());
}
fn units(values: [(i32, i32); 4]) -> TimingSet {
    TimingSet { judgement_type: 1, units: values.iter().map(|&(j, ms)| TimingUnit::new(j, ms, ms)).collect() }
}
fn replica_before(m: &LiveModel, level: usize, j: i32) -> i32 {
    let set = &m.raw_runtime().unwrap().windows.replicas[level][0];
    set.units.iter().find(|u| u.judgement == j).unwrap().before()
}
#[test]
fn assist_replicas_take_additive_windows_and_convert_after_skill_converters() {
    use ournotes_sim::live::raw_assist::{AssistExecutor, AssistLevelAdjuster};
    use std::collections::BTreeMap;
    let mut m = model(&[1, 3]);
    let adjuster = AssistLevelAdjuster {
        level: 0,
        point: 0,
        perfect_continue: 0,
        gauge_max: 10,
        level_max: 1,
        level_down_count: 2,
        judgement_points: BTreeMap::from([(1, 10), (2, 5), (3, 2), (4, 1)]),
        fixed_level: Some(1),
    };
    let levels =
        vec![vec![units([(6, 10), (5, 30), (4, 60), (1, 100)])], vec![units([(6, 20), (5, 50), (4, 80), (1, 100)])]];
    m.raw_runtime_mut().unwrap().enable_assist(AssistExecutor::new(adjuster, vec![], 1., 3.), levels).unwrap();
    m.frame_raw_timed(0, &[], 0.).unwrap();
    // 4000 (+5 ms) reaches every Assist level; 4004 (+50% of base) only the main controller.
    assert_eq!((before(&m, 6), replica_before(&m, 0, 6), replica_before(&m, 1, 6)), (20, 15, 25));
    assert_eq!((before(&m, 5), replica_before(&m, 0, 5), replica_before(&m, 1, 5)), (50, 35, 55));
    m.begin_raw_frame(1020, 0.016).unwrap();
    let r = m.submit_raw_judgement(result(4, 40)).unwrap();
    assert_eq!((r.origin, r.judgement, r.origin_diff_ms, r.diff_ms), (4, 5, 40, 40));
    assert!(m.raw_runtime().unwrap().assist.as_ref().unwrap().assisted_notes.contains(&1));
    m.finish_raw_frame().unwrap();
    // The 4000 limit (1) is consumed by the executor after FT and removed from every level.
    assert_eq!((before(&m, 6), replica_before(&m, 0, 6), replica_before(&m, 1, 6)), (15, 10, 20));
}
#[test]
fn scheduler_results_feed_the_live_model_through_raw_frames() {
    use ournotes_sim::live::raw::{FlickUnit, InputState, InputUnit, Vec2};
    use ournotes_sim::live::raw_input::ScreenFrame;
    use ournotes_sim::live::raw_scheduler::{
        FrameOptions, RawScheduler, ScheduledNote, SchedulerSettings, SchedulerTiming,
    };
    use std::collections::BTreeMap;
    // No window skills: the scheduler's windows stay the static client data given here.
    let mut m = model(&[]);
    let base = vec![
        TimingUnit::new(6, 10, 10),
        TimingUnit::new(5, 30, 30),
        TimingUnit::new(4, 60, 60),
        TimingUnit::new(1, 100, 100),
    ];
    let note = ScheduledNote {
        id: 1,
        operate_type: 1,
        chart_ms: 1000,
        judgement_type: 1,
        lane_min: 0.,
        lane_max: 0.,
        lane_count: 1,
        cached_before_ms: 100,
        cached_after_ms: 100,
    };
    let settings = SchedulerSettings {
        before_playing_ms: 1000,
        input_timing_ms: 0,
        simulator_lane_count: 1,
        area_offset: 0.,
        music_end_ms: 5000,
        input_capacity: 1,
    };
    let timing = SchedulerTiming { base, effective_before_ms: 100, effective_after_ms: 100 };
    let mut s = RawScheduler::new(vec![note], vec![], settings, BTreeMap::from([(1, timing)])).unwrap();
    let mut judged = Vec::new();
    for t in (0..=1100).step_by(20) {
        let state = if t == 1020 { InputState::Enter } else { InputState::None };
        let frame = ScreenFrame {
            real_time_ms: t + 4000,
            music_time_ms: t,
            units: vec![(
                InputUnit { index: 0, state, lane: 0., time_ms: t + 4000 },
                FlickUnit { active: false, lane: 0., delta: Vec2::default() },
            )],
            finger_indices: vec![],
        };
        m.begin_raw_frame(t, 0.02).unwrap();
        let mut failure = None;
        s.step_with(&frame, FrameOptions::default(), |e| {
            let note = RawJudgedNote {
                note_id: e.note_id,
                result: e.judgement.result,
                direction_mismatch: e.judgement.direction_mismatch,
                is_easy_flick: e.judgement.is_easy_flick,
            };
            m.submit_raw_judgement(note).map_err(|err| {
                failure = Some(err);
                "live model rejected the raw result"
            })
        })
        .unwrap();
        assert!(failure.is_none());
        m.finish_raw_frame().unwrap();
        if let Some(r) = s.note_states()[0].result {
            judged.push((t, r));
        }
    }
    let (t, r) = judged[0];
    assert_eq!((t, r.origin, r.judgement, r.origin_diff_ms), (1020, 5, 5, 20));
    assert!(m.score() > 0);
}
