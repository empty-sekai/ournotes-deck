//! The Gekisou factors of a candidate pool: the timer floors, the network rank factors and the luck crossings.

use super::*;

/// The Gekisou factors of the per-entry bound (entries in chart-time order) and what the execution count reads.
pub(crate) struct GkFactors {
    /// Gekisou combo factor bound `G_e` of every deck (from the largest combo bonuses of any five allowed members),
    /// luck factor bound `L_e`, and `R_e`: 1 plus the rank bonus percents (/ 100) of the completing ranges whose
    /// range score contains the entry.
    pub(crate) g: Vec<f64>,
    /// With a combo range: `carriers[n]` bounds the Gekisou combo factor of every deck with at most `n < 5` carrier
    /// slots (a slot whose member and Snap bring Gekisou combo bonus windows); each is at most `g` (empty: none).
    pub(crate) carriers: Vec<Vec<f64>>,
    /// With a combo range: `sums[n]` bounds the combo counts of the decks of `carriers[n]`, `n = 5` for every deck
    /// (`GkCombo::fill`'s sums, which `GkCombo::ramp_count` reads; empty: none).
    pub(crate) sums: Vec<Vec<f64>>,
    pub(crate) l: Vec<f64>,
    pub(crate) r: Vec<f64>,
    /// Per completing range with a rank bonus: (range end time, percent / 100, the entries of its range score).
    pub(crate) ranks: Vec<(i32, f64, Vec<u32>)>,
    /// Entries of a range score whose confirmation can precede the judgement of an earlier combo break.
    pub(crate) nobreak: Vec<bool>,
    /// Per play frame: the earliest time a command filed in the frame can land at (besides the previous frame's time
    /// and the chart times of the frame's notes).
    pub(crate) exec_lo: Vec<i32>,
    /// Per completing range `(start, time of its confirmation frame)`: the score frames after the start's up to the
    /// confirmation's are undone and executed once more.
    pub(crate) confirm: Vec<(i32, i32)>,
    /// With a combo range: what a candidate's own Gekisou combo factor reads (`g` then bounds it for every deck).
    pub(crate) combo: Option<GkCombo>,
    /// The entries a range score snapshot can read before their play frame's lottery. Network ranking snapshots a
    /// range at the last score query before its start, the second one of the play frame before the range starts, and
    /// at the second score query of the play frame its fever ends in, before that frame's lottery; each reads the
    /// notes of that query's own frame (chart times after the previous play frame's, up to its own) that way. Solo
    /// ranking snapshots after the frame's lottery and reads none.
    pub(crate) snapshot: Vec<bool>,
}

impl GkFactors {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        master: &Master,
        setup: &FullSetup,
        sc: &Schedule,
        entries: &[(usize, LiveNote, i32)],
        order: &[usize],
        frames: &[i32],
        reached: &[Vec<i32>],
        entry_reach: &[u8],
        overrides: (bool, bool),
        current: &[Option<usize>],
        member_cb: &[Vec<Vec<ComboBonusRow>>],
        command_floors: &[i32],
    ) -> Result<GkFactors, Error> {
        let ne = order.len();
        let times: Vec<i32> = order.iter().map(|&i| entries[i].1.time_ms).collect();
        let ranges = &sc.ranges;
        // Gekisou combo: min(table, 1) + 1 at the range's combo, inside a combo range
        let table = ComboTable::from_master(master)?;
        let mut gmax = 1f32;
        if let Some(Some(cu)) = table.cumulatives.as_ref().and_then(|c| c.get(GEKISOU_COMBO as usize)) {
            for &c in cu {
                let f = ournotes_sim::num::min_ignoring_nan(c, 1f32) + 1f32;
                if f.is_nan() || f < 0.0 {
                    return Err(Error::Domain("Gekisou combo bonus table is not non-negative".into()));
                }
                gmax = gmax.max(f);
            }
        }
        let gmax = if ablated(ablate::GEKISOU_COMBO) { 1.0 } else { gmax as f64 };
        let combo: Vec<(i32, i32)> =
            ranges.iter().filter(|r| r.mission == MISSION_COMBO).map(|r| (r.start, r.end)).collect();
        let mut gv: Vec<f64> =
            times.iter().map(|&t| if combo.iter().any(|&(a, b)| a <= t && t <= b) { gmax } else { 1.0 }).collect();
        let mut carriers: Vec<Vec<f64>> = Vec::new();
        let mut sums: Vec<Vec<f64>> = Vec::new();
        let gcombo = if combo.is_empty() || ablated(ablate::GEKISOU_COMBO) {
            None
        } else {
            let mut tab: Vec<(i64, f64)> = Vec::new();
            if let (Some(Some(th)), Some(Some(cu))) = (
                table.thresholds.as_ref().and_then(|x| x.get(GEKISOU_COMBO as usize)),
                table.cumulatives.as_ref().and_then(|x| x.get(GEKISOU_COMBO as usize)),
            ) {
                for (&t, &c) in th.iter().zip(cu) {
                    tab.push((t as i64, (ournotes_sim::num::min_ignoring_nan(c, 1f32) + 1f32) as f64));
                }
            }
            tab.sort_by_key(|a| a.0);
            let mut m = 1f64;
            for x in tab.iter_mut() {
                m = m.max(x.1);
                x.1 = m;
            }
            let nr = ranges.len();
            let range: Vec<Option<u32>> = times
                .iter()
                .map(|&t| {
                    (0..nr).find(|&r| ranges[r].mission == MISSION_COMBO && ranges[r].start <= t && t <= ranges[r].end)
                })
                .map(|r| r.map(|r| r as u32))
                .collect();
            // A range's history holds the notes judged 1 to 6 (not WAIT 0 or PASS 7), and only judgements 3 to 6 add
            // to its count: the lists hold the entries that can reach 3 to 6, and the order is checked on those that
            // can reach 1 to 6 (an empty reach mask: any judgement).
            let can = |i: usize, judgements: u8| entry_reach[i] == 0 || entry_reach[i] & judgements != 0;
            let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nr];
            let mut ordered = vec![false; nr];
            for (r, rf) in ranges.iter().enumerate() {
                if rf.mission != MISSION_COMBO {
                    continue;
                }
                let inr = |t: i32| rf.start <= t && t <= rf.end;
                lists[r] = (0..ne).filter(|&q| inr(times[q]) && can(order[q], 0x78)).map(|q| q as u32).collect();
                let proc: Vec<i32> = (0..entries.len())
                    .filter(|&i| inr(entries[i].1.time_ms) && can(i, 0x7e))
                    .map(|i| entries[i].1.time_ms)
                    .collect();
                ordered[r] = proc.windows(2).all(|w| w[0] <= w[1]);
            }
            let upto: Vec<u32> = (0..ne)
                .map(|e| match range[e] {
                    None => 0,
                    Some(r) => {
                        let prev_end = get_frame(times[e]).wrapping_mul(40).wrapping_sub(40);
                        lists[r as usize].partition_point(|&q| times[q as usize] <= prev_end) as u32
                    }
                })
                .collect();
            let judged: Vec<usize> = order.iter().map(|&i| entries[i].0).collect();
            let (frame, read, run) = GkCombo::reads(&lists, &ordered, &times, &judged, frames, current);
            let gc = GkCombo { range, upto, entries: lists, ordered, tab, gmax, frame, read, run };
            let mut every = Vec::new();
            carrier_factors(&gc, &times, member_cb, 5, &mut gv, &mut every);
            for n in 0..5 {
                let (mut v, mut c) = (Vec::new(), Vec::new());
                carrier_factors(&gc, &times, member_cb, n, &mut v, &mut c);
                carriers.push(v);
                sums.push(c);
            }
            sums.push(every);
            Some(gc)
        };

        // luck: the rush bonus of the luck ranges
        let rush = int_setting(master, "gekisou_luck_rush_score_bonus_percent")?;
        if !(0..=1 << 24).contains(&rush) {
            return Err(Error::Domain("luck rush bonus outside the modelled range".into()));
        }
        let mut lv = vec![1.0; ne];
        let luck: Vec<&RangeFacts> =
            ranges.iter().filter(|r| r.mission == MISSION_LUCK && r.f_start.is_some()).collect();
        if !luck.is_empty() {
            let cross = luck_crossings(frames, entries, sc);
            if rush.saturating_mul(cross) > 100 {
                return Err(Error::Domain("the luck bonus can turn negative on this stream".into()));
            }
            let span = |r: &RangeFacts| (r.f_start.unwrap_or(usize::MAX), r.f_finish.unwrap_or(usize::MAX));
            let leak = luck.iter().enumerate().any(|(i, a)| {
                luck[i + 1..].iter().any(|b| {
                    let (x, y) = (span(a), span(b));
                    x.0 <= y.1 && y.0 <= x.1
                })
            });
            let count = if leak { 1 << 20 } else { 1 + cross };
            let f = 100i64.saturating_add(rush.saturating_mul(count)).min(200) as f64 / 100.0;
            if !ablated(ablate::LUCK) {
                for (p, &t) in times.iter().enumerate() {
                    let inside = if leak {
                        luck.iter().any(|r| r.start <= t)
                    } else {
                        luck.iter().any(|r| {
                            let fin = r.f_finish.map_or(i64::MAX, |x| frames[x] as i64);
                            r.start <= t && (t as i64) < fin
                        })
                    };
                    if inside {
                        lv[p] = f;
                    }
                }
            }
        }

        // rank bonus: `trunc(range score * pct / 100)`, the range score being the sum of the scores filed in the
        // score frames after the start's up to the end's, when the range completes
        let p = &setup.params;
        let length = match p.score_music_length_ms {
            Some(l) if l != 0 => l,
            _ => p.music_length_ms,
        };
        let max_frame = get_frame(length).wrapping_add(50);
        let sframe = |t: i32| {
            let f = get_frame(t);
            if max_frame <= f { max_frame.wrapping_sub(1) } else { f }
        };
        let mut rv = vec![1.0; ne];
        let mut nobreak = vec![false; ne];
        let mut confirm = Vec::new();
        let mut ranks = Vec::new();
        let external = setup.gk.as_ref().and_then(|g| g.confirmations.as_deref());
        // Network snapshots and, per confirmed range, (play frame of its application, range start).
        let mut network = None;
        if let Some(confirmations) = external {
            // Network range differences use two actual controller snapshots. A note judged before the range can
            // change between them when a calculation in between re-executes its score frame, and a difference
            // (hence a fixed bonus) can be negative. Bound both signs of every bonus in application order. No
            // settled-prefix subtraction is valid for these retained snapshots. A note's cap must cover its value
            // at any snapshot, including before a later combo break.
            let mut snapshots = Vec::new();
            let mut reexec = Vec::new();
            for c in confirmations {
                let r = ranges.get(c.range).ok_or_else(|| Error::Input("network range outside schedule".into()))?;
                if c.percent < 0 {
                    return Err(Error::Domain("negative rank bonus percent".into()));
                }
                let Some(complete) = r.f_complete else { continue };
                let applied = complete.max(c.frame);
                if applied >= frames.len() {
                    continue;
                }
                let end = sc.states.iter().position(|s| s[c.range] >= RS_END);
                snapshots.push((applied, c.range, r.f_start, end, c.percent));
                confirm.push((r.start, frames[applied]));
                reexec.push((applied, r.start));
            }
            snapshots.sort_by_key(|&(applied, range, ..)| (applied, range));
            network = Some((snapshots, reexec));
            nobreak.fill(true);
        } else {
            for (ri, r) in ranges.iter().enumerate() {
                let Some(c) = r.f_complete else { continue };
                if r.pct < 0 {
                    return Err(Error::Domain("negative rank bonus percent".into()));
                }
                let (fs, fe) = (sframe(r.start), sframe(r.end));
                for (rj, q) in ranges.iter().enumerate() {
                    let x = sframe(q.end);
                    if rj != ri && q.f_complete.is_some() && fs < x && x <= fe {
                        return Err(Error::Unsupported("a rank bonus filed inside another range's score".into()));
                    }
                }
                confirm.push((r.start, frames[c]));
                let inw: Vec<usize> = (0..ne).filter(|&q| fs < sframe(times[q]) && sframe(times[q]) <= fe).collect();
                let Some(maxt) = inw.iter().map(|&q| times[q]).max() else { continue };
                // every command that ends a factor before one of these entries is filed by the confirmation frame
                if frames[c] <= maxt {
                    return Err(Error::Unsupported(
                        "a Gekisou range completes before the play reaches its notes".into(),
                    ));
                }
                let late = entries.iter().any(|e| e.0 > c && e.1.time_ms < maxt);
                if !ablated(ablate::RANK_BONUS) {
                    ranks.push((r.end, r.pct as f64 / 100.0, inw.iter().map(|&q| q as u32).collect()));
                }
                for &q in &inw {
                    if !ablated(ablate::RANK_BONUS) {
                        rv[q] += r.pct as f64 / 100.0;
                    }
                    if late {
                        nobreak[q] = true;
                    }
                }
            }
        }

        // Where the score commands filed in a frame can land, besides the previous frame's time and the chart times of
        // the frame's notes: the rush commands filed after the previous frame's second score update, at the chart
        // times of that frame's luck notes (and of the pending ones, not before the range's start); and the trigger
        // time of a Gekisou score row: the start of a range that turned Playing (after the frame before last) with a
        // range-playing trigger, the playing range's last combo judgement with a combo trigger.
        let (by_playing, by_combo) = overrides;
        let nf = frames.len();
        let luck_on = ranges.iter().any(|r| r.mission == MISSION_LUCK);
        let in_luck = |t: i32| ranges.iter().any(|r| r.mission == MISSION_LUCK && r.start <= t && t <= r.end);
        let mut by_frame: Vec<Vec<usize>> = vec![Vec::new(); nf];
        for (i, e) in entries.iter().enumerate() {
            by_frame[e.0].push(i);
        }
        // per range: the smallest chart time from the last judgement that is certainly a combo judgement on
        let mut lc: Vec<Option<i32>> = vec![None; ranges.len()];
        let mut exec_lo = vec![i32::MAX; nf];
        for f in 0..nf {
            let mut lo = i32::MAX;
            if f >= 1 && luck_on {
                for &i in &by_frame[f - 1] {
                    let t = entries[i].1.time_ms;
                    if in_luck(t) {
                        lo = lo.min(t);
                    }
                }
                for r in ranges.iter().filter(|r| r.mission == MISSION_LUCK && r.f_start == Some(f - 1)) {
                    lo = lo.min(r.start);
                }
            }
            if by_playing {
                lo = lo.min(if f >= 2 { frames[f - 2] } else { i32::MIN });
            }
            if by_combo && let Some(t) = current[f].and_then(|r| lc[r]) {
                lo = lo.min(t);
            }
            exec_lo[f] = lo;
            for &i in &by_frame[f] {
                let t = entries[i].1.time_ms;
                let certain = reached[i].iter().all(|j| (3..=6).contains(j));
                for (ri, r) in ranges.iter().enumerate() {
                    if r.start <= t && t <= r.end {
                        lc[ri] = Some(if certain { t } else { lc[ri].map_or(t, |x| x.min(t)) });
                    }
                }
            }
        }
        if let Some((snapshots, reexec)) = network.filter(|_| !ablated(ablate::RANK_BONUS)) {
            let judged: Vec<usize> = order.iter().map(|&i| entries[i].0).collect();
            let (score_frames, floors) = rerun_floors(setup, &times, &exec_lo, &reexec, command_floors);
            rv = network_rank_factors(&judged, &score_frames, &floors, &snapshots);
        }
        let mut snapshot = vec![false; ne];
        if external.is_some() {
            // Every range's snapshot frames, whether or not a rank bonus reads them.
            let first = |r: usize, state: u8| sc.states.iter().position(|s| s.get(r).is_some_and(|&v| v >= state));
            let mut read: Vec<usize> = Vec::new();
            for r in 0..ranges.len() {
                read.extend(first(r, RS_START).and_then(|f| f.checked_sub(1)));
                read.extend(first(r, RS_END));
            }
            for (q, &i) in order.iter().enumerate() {
                let own = frames.partition_point(|&t| t < times[q]);
                snapshot[q] = read.contains(&own) || read.contains(&entries[i].0);
            }
        }
        Ok(GkFactors { g: gv, carriers, sums, l: lv, r: rv, ranks, nobreak, exec_lo, confirm, combo: gcombo, snapshot })
    }
}

/// The score frame of each chart time and, per play frame, the first score frame its calculations can re-execute, as
/// [`crate::search::snaps::score_windows::Exec`] reads them: the certified timer floor, the chart time of each judged note,
/// `exec_lo`, and the frame after the start of a range whose rank bonus applies in that play frame.
pub(super) fn rerun_floors(
    setup: &FullSetup,
    times: &[i32],
    exec_lo: &[i32],
    reexec: &[(usize, i32)],
    command_floors: &[i32],
) -> (Vec<i32>, Vec<i32>) {
    let score_frames = ScoreFrames::new(&setup.params);
    let clamp = |t: i32| score_frames.at(t as i64);
    let notes: std::collections::HashMap<i32, i32> = setup.notes.iter().map(|n| (n.note_id, n.time_ms)).collect();
    let mut floors = Vec::with_capacity(setup.play.frames.len());
    for (i, f) in setup.play.frames.iter().enumerate() {
        let mut lo = command_floors.get(i).copied().map(clamp).unwrap_or(0);
        for j in &f.judged {
            if let Some(&t) = notes.get(&j.note_id) {
                lo = lo.min(clamp(t));
            }
        }
        match exec_lo.get(i) {
            Some(&x) => lo = lo.min(clamp(x)),
            None => lo = 0,
        }
        for &(at, start) in reexec {
            if at == i {
                lo = lo.min(clamp(start).saturating_add(1));
            }
        }
        floors.push(lo);
    }
    (times.iter().map(|&t| clamp(t)).collect(), floors)
}

/// A network rank bonus: (application frame, range, range start frame, range end frame, percent).
type NetworkSnapshot = (usize, usize, Option<usize>, Option<usize>, i64);

/// Positive/negative fixed-bonus envelopes per note. A snapshot has nonnegative raw note scores plus previously
/// applied signed bonuses. For P = pct * (end - start), discard the nonnegative raw start, add positive bonuses
/// visible at end and negative magnitudes visible at start; the negative envelope is symmetric. Ranks apply after
/// the frame's score calculations. The start snapshot is from the preceding calculation, so including every
/// application strictly before its frame is conservative. End sees only applications strictly before its frame.
/// Since raw start is a subset of raw end, induction gives negative <= positive. Thus 1 + sum(positive) also bounds
/// the absolute value of all intermediate snapshots, used by the existing nonwrapping score-domain check.
///
/// Two exact cancellations tighten this. A note judged before the start whose score frame lies below every frame
/// the calculations from the start's play frame to the end's re-execute (`floors`) holds one value in both
/// snapshots. A rank bonus files with the first calculation after its application and stays a constant, so one
/// applied at least two play frames before the start is in both snapshots.
pub(super) fn network_rank_factors(
    judgement_frames: &[usize],
    score_frames: &[i32],
    floors: &[i32],
    snapshots: &[NetworkSnapshot],
) -> Vec<f64> {
    let ne = judgement_frames.len();
    let mut positive: Vec<Vec<f64>> = Vec::new();
    let mut negative: Vec<Vec<f64>> = Vec::new();
    let mut out = vec![1.0; ne];
    for (i, &(_, _, start, end, pct)) in snapshots.iter().enumerate() {
        let scale = (pct as f64 / 100.0).next_up();
        let between = start.zip(end).filter(|(s, e)| s <= e);
        // The lowest score frame re-executed between the snapshots (none when a play frame is unknown).
        let floor = between.map_or(i32::MIN, |(s, e)| {
            (s..=e).map(|k| floors.get(k).copied().unwrap_or(i32::MIN)).min().unwrap_or(i32::MIN)
        });
        let mut p = vec![0.0; ne];
        let mut n = vec![0.0; ne];
        for (e, &f) in judgement_frames.iter().enumerate() {
            let held = start.is_some_and(|start| f < start) && score_frames[e] < floor;
            let mut hi = if !held && end.is_some_and(|end| f <= end) { 1.0f64 } else { 0.0 };
            let mut lo = if !held && start.is_some_and(|start| f < start) { 1.0f64 } else { 0.0 };
            for (j, &(applied, ..)) in snapshots[..i].iter().enumerate() {
                if between.is_some_and(|(s, _)| applied + 2 <= s) {
                    continue;
                }
                if end.is_some_and(|end| applied < end) {
                    hi = (hi + positive[j][e]).next_up();
                    lo = (lo + negative[j][e]).next_up();
                }
                if start.is_some_and(|start| applied < start) {
                    hi = (hi + negative[j][e]).next_up();
                    lo = (lo + positive[j][e]).next_up();
                }
            }
            p[e] = (hi * scale).next_up();
            n[e] = (lo * scale).next_up();
            out[e] = (out[e] + p[e]).next_up();
        }
        positive.push(p);
        negative.push(n);
    }
    out
}

/// The largest number of luck rush commands that can be undone before they apply: the controller files a rush start
/// and its end in processing order, and an end filed at an earlier time than its start lowers the luck bonus between
/// the two. The ends and starts pair up in disjoint stretches of the processing order, so at a time `x` at most as
/// many are inverted as there are consecutive potential luck events `(p, q)` in processing order with `q <= x < p`.
/// Potential luck events, per frame: the frame time when a range turns Finish, the chart times of pending luck
/// notes, the chart times of the frame's luck notes, and the frame time of a playing luck range's pending lots.
fn luck_crossings(frames: &[i32], entries: &[(usize, LiveNote, i32)], sc: &Schedule) -> i64 {
    let nr = sc.ranges.len();
    let nf = frames.len();
    let mut judged: Vec<Vec<i32>> = vec![Vec::new(); nf];
    for e in entries {
        judged[e.0].push(e.1.time_ms);
    }
    let mut seq: Vec<i64> = Vec::new();
    let mut pending: Vec<Vec<i32>> = vec![Vec::new(); nr];
    let mut prev = vec![0u8; nr];
    for (f, &t) in frames.iter().enumerate() {
        let st = &sc.states[f];
        if (0..nr).any(|r| st[r] == RS_FINISH && prev[r] != RS_FINISH) {
            seq.push(t as i64);
        }
        for r in 0..nr {
            if st[r] > RS_STANDBY && !pending[r].is_empty() {
                for tn in std::mem::take(&mut pending[r]) {
                    if (RS_START..=RS_END).contains(&st[r]) {
                        seq.push(tn as i64);
                    }
                }
            }
        }
        for (r, rf) in sc.ranges.iter().enumerate() {
            if rf.mission != MISSION_LUCK {
                continue;
            }
            for &tn in &judged[f] {
                if rf.start <= tn && tn <= rf.end {
                    if st[r] < RS_START {
                        pending[r].push(tn);
                    } else if st[r] <= RS_END {
                        seq.push(tn as i64);
                    }
                }
            }
        }
        for (r, rf) in sc.ranges.iter().enumerate() {
            if rf.mission == MISSION_LUCK && st[r] == RS_PLAYING {
                seq.push(t as i64);
            }
        }
        prev.clone_from(st);
    }
    // every descent (p, q) covers the times [q, p); the largest overlap
    let mut ev: Vec<(i64, i64)> = Vec::new();
    for w in seq.windows(2) {
        if w[1] < w[0] {
            ev.push((w[1], 1));
            ev.push((w[0], -1));
        }
    }
    ev.sort_unstable();
    let (mut cur, mut best) = (0i64, 0i64);
    for (_, d) in ev {
        cur += d;
        best = best.max(cur);
    }
    best
}
