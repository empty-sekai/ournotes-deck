//! Checks for the fixed-factor lifetime-start projection; all times and rows are synthetic.
use super::*;

fn row() -> Row {
    Row {
        identity: RowIdentity { source: RowSource::GekisouSupport, index: 0, id: 1 },
        trigger_type: 2,
        trigger: 1,
        condition: 0,
        release: 0,
        reset: 0,
        cumulative: 0,
        effect_type: 2000,
        value: 5000,
        act: 0.0,
        limit: 0,
        execute_limit: 0,
        targets: Vec::new(),
        max_value: 0,
        gk: true,
        gate: MISSION_LUCK,
    }
}

fn timing(groups: &[Vec<(i64, usize)>]) -> GkRowWin {
    GkRowWin {
        win: groups.iter().map(|_| (-1000, 1000, 1.0)).collect(),
        filed: groups.iter().map(|_| (-1000, 1000, 1.0)).collect(),
        executions: groups.iter().map(|s| (s.len() as f64).next_up()).collect(),
        conv: vec![(0, 100)],
        starts: groups.iter().flatten().map(|&(_, f)| f).collect(),
        win_starts: groups.iter().map(|s| Rc::new(RampStarts::new(s.clone()))).collect(),
        parts: groups.iter().map(|_| (-1000, 1000, None)).collect(),
    }
}

fn frames(open: &[bool]) -> GkFrames {
    GkFrames {
        times: (0..open.len()).map(|f| f as i32 * 10).collect(),
        gate: [vec![false; open.len()], open.to_vec(), vec![false; open.len()]],
        current: vec![None; open.len()],
        start: Vec::new(),
        complete: vec![false; open.len()],
        ranges: Vec::new(),
        states: vec![Vec::new(); open.len()],
        wlo: vec![0; open.len()],
        next_complete: vec![None; open.len() + 1],
        ent: Vec::new(),
        combo_triggers: None,
        combo_epochs: None,
    }
}

#[test]
fn sustained_start_cap_counts_observed_gate_frames_across_distant_ranges() {
    let mut open = vec![false; 10_001];
    for f in [0, 1, 2, 9998, 9999, 10_000] {
        open[f] = true;
    }
    let g = frames(&open);
    // This timing component contains six possible start frames. Closed-gate gaps cannot end and recycle
    // the current updater, so at most three of these starts execute.
    let w = timing(&[vec![(0, 0), (10, 1), (20, 2), (99_980, 9998), (99_990, 9999), (100_000, 10_000)]]);
    assert_eq!(factor_executions(&row(), &w, None), w.executions);
    assert_eq!(factor_executions(&row(), &w, Some(&g)), [3.0f64.next_up()]);
    // A separate mission's state updates are not observations of this row's gate.
    let mut other_gate = g;
    other_gate.gate[0].fill(true);
    other_gate.gate[2].fill(true);
    assert_eq!(factor_executions(&row(), &w, Some(&other_gate)), [3.0f64.next_up()]);
}

#[test]
fn sustained_start_cap_keeps_open_false_frames_between_sparse_possible_starts() {
    let g = frames(&[true, false, true, true, false, true, true]);
    // Open observations occur at 0, 2, 3, 5, 6. Starts at 0, 3, 6 are all possible when 2 and 5 observe false.
    let w = timing(&[vec![(0, 0), (30, 3), (60, 6)]]);
    assert_eq!(factor_executions(&row(), &w, Some(&g)), w.executions);
    let mut r = row();
    r.gate = MISSION_ALL;
    assert_eq!(factor_executions(&r, &w, Some(&g)), factor_executions(&r, &w, None));
    // Incomplete optional gate geometry retains the processing-frame certificate.
    assert_eq!(factor_executions(&row(), &w, Some(&frames(&[true; 3]))), factor_executions(&row(), &w, None));
}

#[test]
fn sustained_start_cap_counts_each_window_in_its_original_processing_interval() {
    let g = frames(&[true, false, true, true, false, true, true, true, false, true]);
    let w = timing(&[vec![(-20, 0), (-10, 2), (-30, 3)], vec![(40, 5), (40, 6), (30, 7), (20, 9)]]);
    assert_eq!(factor_executions(&row(), &w, Some(&g)), [2.0f64.next_up(), 2.0f64.next_up()]);
}

#[test]
fn sustained_start_cap_uses_processing_frames_and_preserves_sparse_starts() {
    let groups = [
        (0..7).map(|f| (10 * f as i64, f)).collect(),
        vec![(0, 0), (20, 2), (40, 4)],
        // Filing order differs from processing order, including tied and negative times.
        vec![(-100, 40), (-900, 41), (-200, 42), (-900, 43)],
        vec![(0, u32::MAX as usize - 2), (0, u32::MAX as usize - 1), (0, u32::MAX as usize)],
    ];
    let w = timing(&groups);
    assert_eq!(
        factor_executions(&row(), &w, None),
        [4.0f64.next_up(), 3.0f64.next_up(), 2.0f64.next_up(), 2.0f64.next_up()]
    );
    // In T/F/T/F/T, all three possible start frames can execute. Halving their count would be unsafe.
    assert_eq!(factor_executions(&row(), &w, None)[1], w.executions[1]);
}

#[test]
fn sustained_start_cap_keeps_lifetime_counts_and_cached_domains_separate() {
    let w = Rc::new(timing(&[(0..25).map(|f| (f as i64, f)).collect()]));
    let original = (w.win.clone(), w.filed.clone(), w.executions.clone(), w.conv.clone(), w.parts.clone());
    let mut r = row();
    // Target multiplicity still belongs to command_count; this only counts updater starts.
    r.effect_type = 2004;
    r.targets = vec![6, 6, 5];
    // Dynamic conditions, counter resets and execution limits do not bypass the held current updater.
    r.condition = 7;
    r.reset = 8;
    r.execute_limit = 1;
    assert_eq!(factor_executions(&r, &w, None), [13.0f64.next_up()]);
    assert!(factor_executions(&r, &w, None)[0] > POOL);
    assert_eq!(original, (w.win.clone(), w.filed.clone(), w.executions.clone(), w.conv.clone(), w.parts.clone()));
    // A timing-cache entry can also be shared by an effect with an applier-controlled finish.
    r.effect_type = 11005;
    assert_eq!(factor_executions(&r, &w, None), w.executions);
}

#[test]
fn sustained_start_cap_retains_the_uncertified_lifecycles() {
    let w = timing(&[(0..6).map(|f| (f as i64, f)).collect()]);
    for act in [f32::NAN, f32::INFINITY, -1.0, 0.001, 2147483647f32] {
        let mut r = row();
        r.act = act;
        assert_eq!(factor_executions(&r, &w, None), w.executions);
    }
    let mut alternatives = Vec::new();
    for effect_type in [2001, 4004, 11005, 12004, 12006, 13005] {
        let mut r = row();
        r.effect_type = effect_type;
        alternatives.push(r);
    }
    let mut r = row();
    r.trigger_type = 1;
    alternatives.push(r);
    let mut r = row();
    r.release = 1;
    alternatives.push(r);
    let mut r = row();
    r.gk = false;
    alternatives.push(r);
    for r in alternatives {
        assert_eq!(factor_executions(&r, &w, None), w.executions);
    }
}

#[test]
fn sustained_start_cap_does_not_invent_missing_frame_evidence() {
    let mut w = timing(&[(0..6).map(|f| (f as i64, f)).collect()]);
    w.win_starts = vec![Rc::default()];
    assert_eq!(factor_executions(&row(), &w, None), w.executions);
    w.win_starts = vec![Rc::new(RampStarts { at: vec![0], lo: vec![9], hi: vec![2] })];
    assert_eq!(factor_executions(&row(), &w, None), w.executions);
    w.win_starts.clear();
    assert_eq!(factor_executions(&row(), &w, None), w.executions);
    w.executions = vec![f64::INFINITY];
    w.win_starts = vec![Rc::new(RampStarts::new(vec![(0, 0), (1, 1)]))];
    assert_eq!(factor_executions(&row(), &w, None), [f64::INFINITY]);
}
