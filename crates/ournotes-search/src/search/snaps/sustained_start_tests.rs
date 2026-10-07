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
    assert_eq!(factor_executions(&row(), &w), [4.0f64.next_up(), 3.0f64.next_up(), 2.0f64.next_up(), 2.0f64.next_up()]);
    // In T/F/T/F/T, all three possible start frames can execute. Halving their count would be unsafe.
    assert_eq!(factor_executions(&row(), &w)[1], w.executions[1]);
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
    assert_eq!(factor_executions(&r, &w), [13.0f64.next_up()]);
    assert!(factor_executions(&r, &w)[0] > POOL);
    assert_eq!(original, (w.win.clone(), w.filed.clone(), w.executions.clone(), w.conv.clone(), w.parts.clone()));
    // A timing-cache entry can also be shared by an effect with an applier-controlled finish.
    r.effect_type = 11005;
    assert_eq!(factor_executions(&r, &w), w.executions);
}

#[test]
fn sustained_start_cap_retains_the_uncertified_lifecycles() {
    let w = timing(&[(0..6).map(|f| (f as i64, f)).collect()]);
    for act in [f32::NAN, f32::INFINITY, -1.0, 0.001, 2147483647f32] {
        let mut r = row();
        r.act = act;
        assert_eq!(factor_executions(&r, &w), w.executions);
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
        assert_eq!(factor_executions(&r, &w), w.executions);
    }
}

#[test]
fn sustained_start_cap_does_not_invent_missing_frame_evidence() {
    let mut w = timing(&[(0..6).map(|f| (f as i64, f)).collect()]);
    w.win_starts = vec![Rc::default()];
    assert_eq!(factor_executions(&row(), &w), w.executions);
    w.win_starts = vec![Rc::new(RampStarts { at: vec![0], lo: vec![9], hi: vec![2] })];
    assert_eq!(factor_executions(&row(), &w), w.executions);
    w.win_starts.clear();
    assert_eq!(factor_executions(&row(), &w), w.executions);
    w.executions = vec![f64::INFINITY];
    w.win_starts = vec![Rc::new(RampStarts::new(vec![(0, 0), (1, 1)]))];
    assert_eq!(factor_executions(&row(), &w), [f64::INFINITY]);
}
