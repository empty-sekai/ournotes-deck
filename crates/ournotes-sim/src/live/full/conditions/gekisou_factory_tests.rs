use super::*;

fn master(kind: i64, values: &[i32]) -> Master {
    let rows = serde_json::json!({"_allData": [{"_id": 1, "_conditionType": kind,
        "_conditionValues": values, "_conditionTargetIDs": [], "_isPositive": true}]})
    .to_string();
    Master::from_json_tables(|name| (name == "MasterSkillCondition").then_some(rows.as_str())).unwrap()
}

fn ask(checker: &mut Checker, ctrl: Option<&Controller>, rank: Option<i32>) -> ((bool, i64), Option<i32>) {
    let mut life = LifeController::new(1000, Default::default(), 1000).unwrap();
    let mut random = LiveRandom::new(0);
    let mut ctx = CheckCtx {
        life: &mut life,
        random: &mut random,
        frame_time: 999,
        current_combo: 0,
        judged: &[],
        events: &[],
        prev_confirmed_rank: rank,
        gk: ctrl.map(|ctrl| GkView { ctrl, prev_lots: &[], prev_lot_ms: 0 }),
    };
    (checker.check(&mut ctx).unwrap(), checker.override_time())
}

#[test]
fn factory_intervals_read_distinct_controller_counters_and_last_times() {
    for (kind, expected, time) in [(7001, 4, 120), (7002, 3, 110), (7004, 1, 110)] {
        let m = master(kind, &[3]);
        let mut c =
            Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                .one(1, 0)
                .unwrap()
                .unwrap();
        let mut ctrl = Controller::new(vec![(0, 200, 1)], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
        ctrl.current_playing_index = 0;
        ctrl.states[0].combo = 12;
        ctrl.states[0].just = 9;
        ctrl.states[0].raw_just = 3;
        ctrl.states[0].last_combo_ms = 120;
        ctrl.states[0].last_just_ms = 110;
        assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, expected), Some(time)));
        assert_eq!(ask(&mut c, Some(&ctrl), None), ((false, 0), None));
        assert_eq!(ask(&mut c, None, None), ((false, 0), None));
    }
}

#[test]
fn factory_edges_distinguish_raw_and_inflated_and_rearm_on_gap() {
    for (kind, first) in [(7006, true), (7007, false)] {
        let m = master(kind, &[5]);
        let mut c =
            Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
                .one(1, 0)
                .unwrap()
                .unwrap();
        let mut ctrl = Controller::new(vec![(0, 200, 1)], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
        ctrl.current_playing_index = 0;
        ctrl.states[0].just = 10;
        ctrl.states[0].raw_just = 4;
        ctrl.states[0].last_just_ms = 123;
        assert_eq!(ask(&mut c, Some(&ctrl), None).0, (first, i64::from(first)));
        ctrl.states[0].raw_just = 5;
        assert_eq!(ask(&mut c, Some(&ctrl), None).0, (!first, i64::from(!first)));
        assert_eq!(ask(&mut c, None, None).0, (false, 0));
        assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, 1), Some(123)));
    }
}

#[test]
fn factory_start_and_rank_use_presence_and_actual_confirmed_rank() {
    let m = master(7011, &[]);
    let ctrl = Controller::new(vec![], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
    let mut start =
        Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
            .one(1, 0)
            .unwrap()
            .unwrap();
    assert_eq!(ask(&mut start, None, None).0, (false, 0));
    assert_eq!(ask(&mut start, Some(&ctrl), None).0, (true, 1));
    assert_eq!(ask(&mut start, Some(&ctrl), None).0, (false, 0));
    let m = master(7012, &[]); // native empty list defaults to first place
    let mut rank =
        Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
            .one(1, 0)
            .unwrap()
            .unwrap();
    for r in [None, Some(0), Some(-1), Some(2)] {
        assert_eq!(ask(&mut rank, Some(&ctrl), r).0, (false, 0));
    }
    assert_eq!(ask(&mut rank, None, Some(1)).0, (true, 1));
    assert_eq!(ask(&mut rank, Some(&ctrl), Some(1)).0, (false, 0));
}

#[test]
fn factory_requires_interval_value_and_just_level_is_not_an_edge() {
    let m = master(7001, &[]);
    assert!(
        Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
            .one(1, 0)
            .is_err()
    );
    let m = master(7003, &[5]);
    let mut c = Factory { master: &m, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 }
        .one(1, 0)
        .unwrap()
        .unwrap();
    let mut ctrl = Controller::new(vec![(0, 200, 1)], std::iter::empty(), &m, 100, 100, 0, 0).unwrap();
    ctrl.current_playing_index = 0;
    ctrl.states[0].just = 5;
    assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, 1), None));
    assert_eq!(ask(&mut c, Some(&ctrl), None), ((true, 1), None));
}
