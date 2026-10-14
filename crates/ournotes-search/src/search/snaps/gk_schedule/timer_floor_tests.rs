use super::factors::{network_rank_factors, rerun_floors};
use super::*;
use ournotes_sim::live::full::PlayFrame;

fn setup() -> FullSetup {
    FullSetup {
        notes: vec![LiveNote { note_id: 1, time_ms: 80, note_operate_type: 1, judgement_type: 1 }],
        events: vec![(0, 20)],
        play: LivePlay {
            frames: [0, 40, 80, 120].into_iter().map(|time_ms| PlayFrame { time_ms, judged: Vec::new() }).collect(),
            base_seed: 0,
        },
        params: LiveParams {
            skill_target_music_type: 0,
            total_power: 0,
            music_level: 1,
            converted_note_count: 1,
            music_length_ms: 1000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        },
        gk: None,
    }
}

fn row() -> Row {
    Row {
        identity: RowIdentity { source: RowSource::Support, index: 0, id: 0 },
        trigger_type: 1,
        trigger: 0,
        condition: 0,
        release: 0,
        reset: 0,
        cumulative: 0,
        effect_type: 2000,
        value: 1000,
        act: 0.12,
        limit: 0,
        execute_limit: 0,
        targets: Vec::new(),
        max_value: 0,
        gk: false,
        gate: 0,
    }
}

#[test]
fn compact_timer_floor_retains_ordinary_clocks_and_music_length_clamp() {
    let mut s = setup();
    let mut r = row();
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 80]);
    r.effect_type = 15000;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 80]);
    s.params.music_length_ms = 50;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 50]);
}

#[test]
fn uncertified_timers_allow_reexecution_from_frame_zero() {
    let ordinary = setup();
    let ordinary_row = row();
    let mut long = ordinary.clone();
    long.play.frames[3].time_ms = (1 << 24) + 1;
    let mut negative_frame = ordinary.clone();
    negative_frame.play.frames[0].time_ms = -1;
    let mut old_note = ordinary.clone();
    old_note.notes[0].time_ms = -1;
    let mut distant_event = ordinary.clone();
    distant_event.events[0].1 = (1 << 24) + 1;
    let mut distant_range = ordinary.clone();
    distant_range.set_gekisou(
        GekisouSetup { fevers: vec![(0, (1 << 24) + 1)], missions: vec![MISSION_COMBO] },
        vec![0.04; 4],
        vec![0],
    );
    let mut shortening = ordinary_row.clone();
    shortening.effect_type = 15000;
    shortening.value = -1;
    let mut sustained = ordinary_row.clone();
    sustained.gk = true;
    sustained.trigger_type = 2;
    sustained.act = 0.08;
    let mut released = ordinary_row.clone();
    released.release = 1;
    released.act = 0.08;
    for (s, r) in [
        (&long, &ordinary_row),
        (&negative_frame, &ordinary_row),
        (&old_note, &ordinary_row),
        (&distant_event, &ordinary_row),
        (&distant_range, &ordinary_row),
        (&ordinary, &shortening),
        (&ordinary, &sustained),
        (&ordinary, &released),
    ] {
        assert_eq!(command_floor_times(s, [r].into_iter(), |_| false), [0; 4]);
    }
    sustained.act = 0.0;
    assert_eq!(command_floor_times(&ordinary, [&sustained].into_iter(), |_| false), [0, 0, 40, 80]);
}

#[test]
fn timers_covering_the_play_horizon_keep_compact_floors() {
    let s = setup();
    for (release, gk, trigger_type) in [(1, false, 1), (0, true, 2), (1, true, 2)] {
        let mut r = row();
        r.release = release;
        r.gk = gk;
        r.trigger_type = trigger_type;
        for act in [0.12f32, 0.12f32.next_up(), 1.0] {
            r.act = act;
            assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 80]);
        }
        for act in [0.12f32.next_down(), f32::MAX, f32::INFINITY] {
            r.act = act;
            assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0; 4]);
        }
        for act in [0.0, -1.0, f32::NEG_INFINITY, f32::NAN] {
            r.act = act;
            assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 80]);
        }
    }
}

#[test]
fn timer_horizon_uses_native_binary32_duration_and_the_play_clock() {
    let mut s = setup();
    let mut r = row();
    r.release = 1;
    assert!((r.act as f64) * 1000.0 < 120.0);
    assert_eq!(r.act * 1000f32, 120.0);
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 80]);
    s.params.music_length_ms = 50;
    s.params.score_music_length_ms = Some(40);
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0, 0, 40, 50]);
    s.play.frames[3].time_ms = 121;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0; 4]);
    s.play.frames[0].time_ms = 20;
    s.events[0].1 = 0;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0; 4]);
}

#[test]
fn range_start_release_timers_keep_compact_floors_when_covering_the_next_frame() {
    let mut s = setup();
    s.set_gekisou(GekisouSetup { fevers: vec![(30, 100)], missions: vec![MISSION_COMBO] }, vec![0.04; 4], vec![0]);
    let mut r = row();
    r.gk = true;
    r.release = 1;
    // the range starting at 30 triggers in the frame at 40; the first timed finish is filed at 120
    r.act = 0.05;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0, 0, 40, 80]);
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| false), [0; 4]);
    r.trigger_type = 2;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0; 4]);
    r.trigger_type = 1;
    r.act = 0.049;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0; 4]);
    // a start on a frame time triggers in that frame
    s.set_gekisou(GekisouSetup { fevers: vec![(40, 100)], missions: vec![MISSION_COMBO] }, vec![0.04; 4], vec![0]);
    r.act = 0.04;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0, 0, 40, 80]);
    r.act = 0.039;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0; 4]);
    // without a frame after the trigger frame no timed finish is filed; every range is checked
    let fevers = vec![(30, 50), (90, 110)];
    s.set_gekisou(GekisouSetup { fevers, missions: vec![MISSION_COMBO; 2] }, vec![0.04; 4], vec![0]);
    r.act = 0.001;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0; 4]);
    r.act = 0.05;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0, 0, 40, 80]);
    s.set_gekisou(GekisouSetup { fevers: vec![(90, 110)], missions: vec![MISSION_COMBO] }, vec![0.04; 4], vec![0]);
    r.act = 0.001;
    assert_eq!(command_floor_times(&s, [&r].into_iter(), |_| true), [0, 0, 40, 80]);
}

#[test]
fn horizon_timer_certificate_retains_clock_and_extension_requirements() {
    let s = setup();
    let mut r = row();
    r.release = 1;
    r.act = 1.0;
    let mut extension = row();
    extension.effect_type = 15000;
    extension.value = 1000;
    assert_eq!(command_floor_times(&s, [&r, &extension].into_iter(), |_| false), [0, 0, 40, 80]);
    extension.value = -1;
    assert_eq!(command_floor_times(&s, [&r, &extension].into_iter(), |_| false), [0; 4]);
    let mut negative = s.clone();
    negative.play.frames[0].time_ms = -1;
    let mut decreasing = s.clone();
    decreasing.play.frames[2].time_ms = 121;
    let mut large = s.clone();
    large.play.frames[3].time_ms = (1 << 24) + 1;
    for setup in [&negative, &decreasing, &large] {
        assert_eq!(command_floor_times(setup, [&r].into_iter(), |_| false), [0; 4]);
    }
}

#[test]
fn execution_counts_and_network_snapshots_share_the_certified_floor() {
    let s = setup();
    let mut r = row();
    let compact = command_floor_times(&s, [&r].into_iter(), |_| false);
    r.effect_type = 15000;
    r.value = -1;
    let broad = command_floor_times(&s, [&r].into_iter(), |_| false);
    assert_eq!(Exec::new(&s, &[80], None, &compact).e, [4, 4, 4, 2]);
    assert_eq!(Exec::new(&s, &[80], None, &broad).e, [8, 6, 4, 2]);
    let (_, compact) = rerun_floors(&s, &[0], &[i32::MAX; 4], &[], &compact);
    let (_, broad) = rerun_floors(&s, &[0], &[i32::MAX; 4], &[], &broad);
    assert_eq!(compact, [0, 0, 1, 2]);
    assert_eq!(broad, [0; 4]);
    let snapshots = [(3, 0, Some(2), Some(3), 100)];
    let retained = network_rank_factors(&[0], &[0], &compact, &snapshots);
    let replayed = network_rank_factors(&[0], &[0], &broad, &snapshots);
    assert!(retained[0] < 1.000001);
    assert!(replayed[0] >= 2.0);
}
