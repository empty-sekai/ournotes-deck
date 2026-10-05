//! Score-frame geometry, factor windows and frame execution counts.
use super::*;

/// Frame and entry times the windows read.
pub(super) struct Geo<'g> {
    /// Frame times, non-decreasing.
    pub(super) frames: &'g [i32],
    /// Entry chart times, non-decreasing.
    pub(super) times: &'g [i32],
    pub(super) exec: &'g Exec,
}

impl Geo<'_> {
    /// The number of play frames with a time in `[a, b)`.
    pub(super) fn frames_in(&self, a: i64, b: i64) -> f64 {
        let lo = self.frames.partition_point(|&x| (x as i64) < a);
        let hi = self.frames.partition_point(|&x| (x as i64) < b);
        hi.saturating_sub(lo) as f64
    }

    /// Entries with a chart time in `[a, b)`.
    pub(super) fn range(&self, a: i64, b: i64) -> (u32, u32) {
        let lo = self.times.partition_point(|&x| (x as i64) < a);
        let hi = self.times.partition_point(|&x| (x as i64) < b);
        (lo as u32, hi.max(lo) as u32)
    }

    /// Index of the first frame at or after `t`.
    pub(super) fn first_frame(&self, t: i32) -> Option<usize> {
        let i = self.frames.partition_point(|&x| x < t);
        (i < self.frames.len()).then_some(i)
    }

    /// The latest finish time of an effect started at `exec` in frame `i0` with activation `act` s and extensions
    /// adding at most `ext` ms (`unbounded`: no bound): its factor holds for the notes at chart times in
    /// `[exec, finish)`. The effect ends in the next frame (finish = that frame's time) when its duration
    /// `act * 1000 + extension` (binary32) does not exceed the elapsed time there, else at `exec + ceil(duration)` once
    /// a frame sees the elapsed time pass the duration; with no such frame it never ends (`i64::MAX`).
    pub(super) fn end(&self, exec: i32, i0: usize, act: f32, ext: f64, unbounded: bool) -> i64 {
        let n = self.frames.len();
        let next = if i0 + 1 < n { Some(self.frames[i0 + 1] as i64) } else { None };
        if act.is_nan() {
            return next.unwrap_or(i64::MAX);
        }
        if act <= 0.0 {
            if ext > 0.0 || unbounded {
                return i64::MAX;
            }
            return next.unwrap_or(i64::MAX);
        }
        if unbounded || ext >= (1 << 24) as f64 {
            return i64::MAX;
        }
        // the duration is non-decreasing in the extension (binary32 addition and ceil are monotone)
        let dur = act * 1000f32 + ext as f32;
        let last = self.frames[n - 1];
        match next {
            Some(t1) if dur < last.wrapping_sub(exec) as f32 => {
                (exec as i64 + ournotes_sim::num::ceil_to_i32(dur) as i64).max(t1)
            }
            _ => i64::MAX,
        }
    }
}

/// Score-factor windows of the performer at one position: its live rows (with their condition results) and its
/// snap class's active rows; also the number of factor commands and the total factor that can be active at once.
#[allow(clippy::type_complexity)]
pub(super) fn windows(
    geo: &Geo,
    position: usize,
    live: &[LiveRow],
    snap: &[ActiveRow],
    events: &[i32],
) -> (Vec<Window>, f64, [f64; 4], f64, Vec<(i64, i64, f64)>, Vec<ComboRamp>, Vec<rush::RushRef>, f64, f64) {
    let nk = events.len() as f64;
    let mut ext = 0f64;
    let mut unbounded = false;
    for x in live {
        if x.row.effect_type == 15000 && x.out.is_none_or(|o| o.yes) {
            ext += (x.row.value.max(0) as f64) * nk;
        }
    }
    for r in snap {
        if r.effect_type == 15000 && r.can_start {
            if r.event_bound {
                ext += (r.value.max(0) as f64) * nk;
            } else {
                unbounded = true;
            }
        }
    }
    let factors = |t: i64, value: i64, targets: &[i64]| -> (f64, [f64; 4], f64) {
        let mut j = [0f64; 4];
        let mut note = 0f64;
        let mut cmds = 0f64;
        if t == 2000 || t == 2001 {
            note = note_factor_mill(value as f32 / 10000f32).max(0) as f64 / 1e5;
            cmds = 1.0;
        } else if t == 2004 {
            let f = judgement_factor_mill(value as f32 / 10000f32).max(0) as f64 / 1e5;
            for &x in targets {
                if (3..=6).contains(&x) {
                    j[(x - 3) as usize] += f;
                }
                cmds += 1.0;
            }
        }
        (note, j, cmds)
    };
    let mut out = Vec::new();
    let mut cmds = 0f64;
    let mut cmds_plain = 0f64;
    let mut ops = 0f64;
    let mut ops_plain = 0f64;
    let mut spans: Vec<(i64, i64, f64)> = Vec::new();
    // factor that can act on one note: the note factor plus the judgement factor of one judgement
    let mut fac = [0f64; 4];
    let add = |fac: &mut [f64; 4], note: f64, judge: [f64; 4], mult: f64| {
        for j in 0..4 {
            fac[j] += (note + judge[j]) * mult;
        }
    };
    let mut ramps: Vec<ComboRamp> = Vec::new();
    let mut rush_rows = Vec::new();
    let mut push = |a: i64, b: i64, note: f64, judge: [f64; 4], ramp: u32, rush: u32| {
        let (lo, hi) = geo.range(a, b);
        if hi > lo && (note != 0.0 || judge.iter().any(|&x| x != 0.0)) {
            out.push(Window { lo, hi, note, judge, ramp, rush });
        }
    };
    let can = |x: &LiveRow| matches!(x.row.effect_type, 2000 | 2004) && x.out.is_none_or(|o| o.yes);
    for (i, x) in live.iter().enumerate() {
        let r = &x.row;
        if !can(x) {
            continue;
        }
        let (mut note, mut judge, mut c) = factors(r.effect_type, r.value, &r.targets);
        // at most one of the pair starts at an event; with a second event of the position the first factor stays,
        // so the pair is merged only when the position has one event
        if let Some(p) = x.partner.filter(|_| events.len() <= 1) {
            if p < i && can(&live[p]) {
                continue;
            }
            if can(&live[p]) {
                let (n2, j2, c2) = factors(live[p].row.effect_type, live[p].row.value, &live[p].row.targets);
                note = note.max(n2);
                for q in 0..4 {
                    judge[q] = judge[q].max(j2[q]);
                }
                c = c.max(c2);
            }
        }
        for &ev in events {
            let Some(i0) = geo.first_frame(ev) else { continue };
            // a second event of the position restarts the effect without removing the first factor
            let end = if events.len() >= 2 { i64::MAX } else { geo.end(ev, i0, r.act, ext, unbounded) };
            push(ev as i64, end, note, judge, 0, 0);
            cmds += 2.0 * c;
            cmds_plain += 2.0 * c;
            add(&mut fac, note, judge, 1.0);
            let op = c * (geo.exec.over(ev as i64, ev as i64) + geo.exec.over(ev as i64, end));
            ops += op;
            ops_plain += op;
            spans.push((ev as i64, end, note + judge.iter().copied().fold(0f64, f64::max)));
        }
    }
    for r in snap {
        if !matches!(r.effect_type, 2000 | 2001 | 2004) || !r.can_start {
            continue;
        }
        let (note, judge, c) = factors(r.effect_type, r.value, &r.targets);
        if let Some(ramp) = &r.cumulative_ramp {
            for &(a, b, value) in ramp.iter() {
                let (note, judge, _) = factors(r.effect_type, value, &r.targets);
                push(a, b, note, judge, 0, 0);
            }
        }
        if let Some(ws) = r.gk_event_win.as_ref().map(|w| &w[position]).or(r.gk_win.as_ref()) {
            let rush = match &r.rush {
                Some(spec) => {
                    rush_rows.push(rush::RushRef {
                        spec: spec.clone(),
                        note,
                        judge,
                        run_cap: r.rush_run_cap,
                        ops: 0.0,
                        ops_per_run: 0.0,
                        cmds: 0.0,
                        cmds_per_run: 0.0,
                        max_runs: 0.0,
                    });
                    rush_rows.len() as u32
                }
                None => 0,
            };
            for (wi, &(a, b, mult)) in ws.iter().enumerate() {
                if r.cumulative_ramp.is_none() {
                    // A combo-count ramp keeps its flat window for every other bound; the candidate cap may read
                    // the ramp per entry instead.
                    let ramp = match &r.combo_ramp {
                        Some((unit, max_count, table)) if judge.iter().all(|&x| x == 0.0) => {
                            // an own-event window starts at its event frame's time
                            let starts = match (&r.gk_event_win, &r.gk_starts) {
                                (Some(_), _) | (None, None) => {
                                    Rc::new(RampStarts::new(vec![(a, geo.frames.partition_point(|&x| (x as i64) < a))]))
                                }
                                (None, Some(s)) => s[wi].clone(),
                            };
                            ramps.push(ComboRamp {
                                unit: *unit,
                                max_count: *max_count,
                                table: table.clone(),
                                mult,
                                starts,
                            });
                            ramps.len() as u32
                        }
                        _ => 0,
                    };
                    push(a, b, note * mult, judge.map(|x| x * mult), ramp, rush);
                }
                // Pool size bounds simultaneous factors, not lifetime starts: an
                // updater can be returned and reused many times in this span.
                let starts = if r.gk_event_win.is_some() {
                    1.0
                } else {
                    r.gk_execs.as_ref().and_then(|v| v.get(wi)).copied().unwrap_or(f64::INFINITY)
                };
                let count = command_count(
                    starts,
                    mult,
                    geo.frames_in(a, b),
                    r.churn.then_some(r.churn_max.unwrap_or(f64::INFINITY)),
                    c,
                );
                cmds = (cmds + count).next_up();
                add(&mut fac, note, judge, mult);
                let e = geo.exec.over(a, b);
                let op = product_up(count, e);
                ops = (ops + op).next_up();
                if rush == 0 {
                    ops_plain = (ops_plain + op).next_up();
                    cmds_plain = (cmds_plain + count).next_up();
                } else {
                    let row = &mut rush_rows[rush as usize - 1];
                    row.ops = (row.ops + op).next_up();
                    row.ops_per_run = (row.ops_per_run + product_up(2.0 * c, e)).next_up();
                    row.cmds = (row.cmds + count).next_up();
                    row.cmds_per_run = (row.cmds_per_run + 2.0 * c).next_up();
                    row.max_runs = (row.max_runs + starts).next_up();
                }
                spans.push((a, b, (note + judge.iter().copied().fold(0f64, f64::max)) * mult));
            }
        } else if r.event_bound && !r.churn {
            for &ev in events {
                let Some(i0) = geo.first_frame(ev) else { continue };
                let exec = geo.frames[i0];
                let end = geo.end(exec, i0, r.act, 0.0, false);
                push(exec as i64, end, note, judge, 0, 0);
                cmds += 2.0 * c;
                cmds_plain += 2.0 * c;
                add(&mut fac, note, judge, 1.0);
                let op = c * (geo.exec.over(exec as i64, exec as i64) + geo.exec.over(exec as i64, end));
                ops += op;
                ops_plain += op;
                spans.push((exec as i64, end, note + judge.iter().copied().fold(0f64, f64::max)));
            }
        } else {
            // a cumulative note score up replaces its factor (two commands) in any frame while it runs
            let churn = if r.churn { POOL + 1.0 } else { 1.0 };
            let judge5 = judge.map(|x| x * POOL);
            push(i64::MIN, i64::MAX, note * POOL, judge5, 0, 0);
            let count = 2.0 * c * geo.frames.len() as f64 * churn;
            cmds += count;
            cmds_plain += count;
            add(&mut fac, note, judge, POOL);
            let op = 2.0 * c * geo.frames.len() as f64 * geo.exec.max as f64 * churn;
            ops += op;
            ops_plain += op;
            spans.push((i64::MIN, i64::MAX, (note + judge.iter().copied().fold(0f64, f64::max)) * POOL));
        }
    }
    (out, cmds, fac, ops, spans, ramps, rush_rows, ops_plain, cmds_plain)
}

fn product_up(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 { 0.0 } else { (a * b).next_up() }
}

/// Start/end commands for every possible lifetime activation, plus cumulative
/// replacements. At most `concurrency` updaters replace a factor in one frame,
/// so replacement work is capped by both activation count and processing frames.
fn command_count(starts: f64, concurrency: f64, frames: f64, churn: Option<f64>, fields: f64) -> f64 {
    let replacements = churn.map_or(0.0, |steps| product_up(starts, steps).min(product_up(concurrency, frames)));
    product_up(2.0 * fields, (starts + replacements).next_up())
}

#[cfg(test)]
mod lifetime_command_tests {
    use super::*;
    use ournotes_sim::live::score::ScoreFactorState;
    use ournotes_sim::live::skill::{FactorCommand, apply_factor};

    #[test]
    fn reused_pool_slots_do_not_bound_total_starts() {
        let plain = command_count(100.0, 5.0, 1000.0, None, 1.0);
        assert!((200.0..201.0).contains(&plain));
        let cumulative = command_count(1000.0, 5.0, 100.0, Some(f64::INFINITY), 1.0);
        assert!((3000.0..3001.0).contains(&cumulative));
        assert!(plain > 2.0 * POOL);
    }

    #[test]
    fn repeated_factor_pulses_and_frame_replay_fit_lifetime_drift_margin() {
        let (pulses, repeats) = (256usize, 3usize);
        let commands = command_count(pulses as f64, 1.0, (pulses * 2) as f64, None, 2.0);
        let executions = commands * repeats as f64;
        let norm = 6.0;
        let amplification = float_margin::amplification(3.0 * executions + 2.0 * commands, 2f64.powi(-24)).unwrap();
        let error = (3.0 * executions + 2.0 * commands) * 2f64.powi(-24) * norm * amplification;
        let mut state = ScoreFactorState::new(1000);
        let mut actual_commands = 0usize;
        for pulse in 0..pulses {
            let (a, b) = [(129999, 3000), (33333, 270000), (141421, 173205)][pulse % 3];
            for (values, ideal) in [([a, b], 1.0 + (a + b) as f64 / 100000.0), ([-b, -a], 1.0)] {
                let mut difference = 0f32;
                for replay in 0..repeats {
                    if replay != 0 {
                        state.note_score_up -= difference;
                        difference = 0.0;
                    }
                    for mill in values {
                        apply_factor(&mut state, &FactorCommand { note_mill: mill, ..Default::default() });
                        difference += mill as f32 / 100000f32;
                        actual_commands += 1;
                    }
                    assert!((state.note_score_up as f64 - ideal).abs() <= error);
                }
            }
        }
        assert!(executions >= actual_commands as f64);
    }
}

/// How many times each 40 ms score frame up to the last judged note can be executed (its first run and the re-runs
/// after a command lands in it or before it), from the frame schedule and the notes judged in each play frame: in a
/// play frame, both recalculations re-execute score frames only from the earliest frame a command filed in that play
/// frame can land in (the previous play frame's frame for skill commands, the chart time of each judged note) up to
/// the current one. Commands that land after the last note cannot change a note's score and count as none.
pub(super) struct Exec {
    pub(super) e: Vec<u32>,
    pub(super) max: u32,
    pub(super) max_frame: i32,
    /// `sparse[j][i]`: the largest of `e[i..i + 2^j]`.
    pub(super) sparse: Vec<Vec<u32>>,
}

impl Exec {
    pub(super) fn new(setup: &FullSetup, times: &[i32], gk: Option<&GkFactors>) -> Exec {
        let ml = setup.params.music_length_ms;
        let max_frame = get_frame(ml).wrapping_add(50).max(1);
        let Some(&last_note) = times.last() else {
            return Exec { e: Vec::new(), max: 2, max_frame, sparse: Vec::new() };
        };
        let clamp = |t: i32| {
            let g = get_frame(t);
            if g >= max_frame { max_frame - 1 } else { g.max(0) }
        };
        let g_last = clamp(last_note);
        let notes: HashMap<i32, i32> = setup.notes.iter().map(|n| (n.note_id, n.time_ms)).collect();
        let mut diff = vec![0i64; g_last as usize + 2];
        let mut prev_to = 0;
        for (i, f) in setup.play.frames.iter().enumerate() {
            let to = clamp(f.time_ms);
            let mut lo = if i == 0 { 0 } else { prev_to };
            for j in &f.judged {
                if let Some(&t) = notes.get(&j.note_id) {
                    lo = lo.min(clamp(t));
                }
            }
            if let Some(x) = gk.and_then(|g| g.exec_lo.get(i)) {
                lo = lo.min(clamp(*x));
            }
            let hi = to.min(g_last);
            if lo <= hi {
                diff[lo as usize] += 2;
                diff[hi as usize + 1] -= 2;
            }
            prev_to = to;
        }
        // confirming a rank bonus undoes the score frames after the range start and executes them again
        for &(a, b) in gk.map_or(&[][..], |g| &g.confirm[..]) {
            let (a, b) = (clamp(a) + 1, clamp(b).min(g_last));
            if a <= b {
                diff[a as usize] += 1;
                diff[b as usize + 1] -= 1;
            }
        }
        let mut acc = 0i64;
        let mut e = Vec::with_capacity(g_last as usize + 1);
        for d in diff.iter().take(g_last as usize + 1) {
            acc += d;
            e.push(acc.max(1) as u32);
        }
        let max = e.iter().copied().max().unwrap_or(1).max(2);
        let mut sparse = vec![e.clone()];
        let mut w = 1usize;
        while 2 * w <= e.len() {
            let prev = &sparse[sparse.len() - 1];
            let next: Vec<u32> = (0..=e.len() - 2 * w).map(|i| prev[i].max(prev[i + w])).collect();
            sparse.push(next);
            w *= 2;
        }
        Exec { e, max, max_frame, sparse }
    }

    /// Executions of the worst score frame a command in chart-time range `[a, b]` can land in (0 past the last
    /// note).
    pub(super) fn over(&self, a: i64, b: i64) -> f64 {
        let clamp = |t: i64| {
            let t = t.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let g = get_frame(t);
            (if g >= self.max_frame { self.max_frame - 1 } else { g.max(0) }) as usize
        };
        let (ga, gb) = (clamp(a), clamp(b));
        if ga >= self.e.len() {
            return 0.0;
        }
        let hi = gb.min(self.e.len() - 1);
        if hi < ga {
            return self.e[ga..=hi].iter().copied().max().unwrap_or(0) as f64;
        }
        let j = (usize::BITS - 1 - (hi - ga + 1).leading_zeros()) as usize;
        self.sparse[j][ga].max(self.sparse[j][hi + 1 - (1 << j)]) as f64
    }
}

/// Index of the last frame whose judgements a convert function registered in frame `i0` (activation `act` s) can
/// still see: the frame that processes its end at the latest (unbounded when no frame is late enough). The windows
/// of convert functions are in frame indexes: they see the notes judged in frames `i0 + 1 ..= end`.
pub(super) fn register_end(frames: &[i32], i0: usize, act: f32) -> i64 {
    let next = (i0 + 1) as i64;
    if act.is_nan() || act <= 0.0 {
        return next;
    }
    let dur = act as f64 * 1000.0 * (1.0 + 2f64.powi(-22)) + 1.0;
    let limit = frames[i0] as f64 + dur;
    let i = frames.partition_point(|&t| t as f64 <= limit);
    if i >= frames.len() { i64::MAX } else { (i as i64).max(next) }
}

#[cfg(test)]
mod member_target_regression {
    use super::*;
    use ournotes_sim::cards::{OwnedMember, Player};

    #[test]
    fn member_master_attributes_reach_live_and_search_matchers() {
        let tables = [
            (
                "MasterMemberCard",
                r#"{"_allData":[{"_id":1,"_characterID":2,"_cardType":3,"_bestMusicTagIDs":[7],"_memberCardLevelGroup":1,"_memberCardAwakeGroup":1,"_memberCardRankGroup":1,"_liveSkillID":10,"_gekisouSkillID":20}]}"#,
            ),
            ("MasterCharacter", r#"{"_allData":[{"_id":2,"_bandID":1}]}"#),
            ("MasterMemberCardLevel", r#"{"_allData":[{"_id":1,"_group":1,"_level":1}]}"#),
            ("MasterMemberCardAwake", r#"{"_allData":[{"_id":1,"_group":1,"_awakeCount":0}]}"#),
            ("MasterMemberCardRank", r#"{"_allData":[{"_id":1,"_group":1,"_rank":1}]}"#),
            ("MasterLiveSkill", r#"{"_allData":[{"_id":10,"_skillCategories":[11]}]}"#),
            ("MasterGekisouSkill", r#"{"_allData":[{"_id":20,"_skillCategories":[21],"_gekisouMissionType":2}]}"#),
        ];
        let master = Master::from_json_tables(|name| tables.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)).unwrap();
        let owned: OwnedMember = serde_json::from_str(r#"{"id":1,"level":1,"awake":0,"rank":1}"#).unwrap();
        let m = MemberView::resolve(&master, &Player::default(), &owned).unwrap();
        let p = performer(&m, None).unwrap();
        assert_eq!(p.tag_ids, vec![7]);
        assert_eq!(p.live_skill_categories, vec![11]);
        assert_eq!(p.gekisou_skill_categories, vec![21]);
        assert_eq!(p.gekisou_mission_type, 2);
        for target in [
            SkillTargetRow { skill_target_type: 1, band_id: 1, character_id: 999, ..Default::default() },
            SkillTargetRow { tag_id: 7, ..Default::default() },
            SkillTargetRow { tag_id: 999, live_skill_categories: vec![11], ..Default::default() },
            SkillTargetRow { gekisou_skill_categories: vec![21], ..Default::default() },
            SkillTargetRow { gekisou_mission_type: 2, ..Default::default() },
        ] {
            assert!(p.matches_skill_target(&target));
            assert!(target_matches(&target, &m).unwrap());
        }
    }
}
