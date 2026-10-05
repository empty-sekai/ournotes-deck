//! Deck-independent Gekisou range schedule, combo and factor envelopes.
use super::*;

/// An integer live setting.
pub(super) fn int_setting(master: &Master, key: &str) -> Result<i64, Error> {
    let v = master.live_setting(key).ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
    v.trim().parse::<i64>().map_err(|_| Error::Master(format!("MasterLiveSettings {key} is not an integer")))
}

/// The song's mission pattern (as the rank bonus table keys it): 0 when a mission is missing, 1 all the same, 2 all
/// different, 3 otherwise.
pub(super) fn mission_pattern(m: &[i64]) -> i64 {
    let (a, b, c) = (m.first().copied().unwrap_or(0), m.get(1).copied().unwrap_or(0), m.get(2).copied().unwrap_or(0));
    if a == 0 || b == 0 || c == 0 {
        return 0;
    }
    if a == b {
        return if a == c { 1 } else { 3 };
    }
    if b != c && a != c {
        return 2;
    }
    3
}

/// One Gekisou range of a live and its schedule.
#[derive(Clone, Copy, Debug)]
pub(super) struct RangeFacts {
    pub(super) start: i32,
    pub(super) end: i32,
    pub(super) mission: i64,
    /// Rank bonus percent (a solo player is rank 1).
    pub(super) pct: i64,
    /// The first play frame in which the range is in the state Start (its fever turned on), Complete (its rank bonus
    /// is confirmed there) and Finish.
    pub(super) f_start: Option<usize>,
    pub(super) f_complete: Option<usize>,
    pub(super) f_finish: Option<usize>,
}

/// The schedule of the Gekisou ranges: the state of every range after each play frame. The range state machine reads
/// only the frame times, the delta times and the fevers, so the schedule is the same for every deck; it is taken from
/// a run of the live without skills.
pub(super) struct Schedule {
    pub(super) states: Vec<Vec<u8>>,
    pub(super) ranges: Vec<RangeFacts>,
}

impl Schedule {
    pub(super) fn new(master: &Master, setup: &FullSetup, g: &GkPlay) -> Result<Schedule, Error> {
        let perf = vec![Performer::default(); 5];
        let params = LiveParams { total_power: 0, ..setup.params };
        let mut lm = if let Some(confirmations) = &g.confirmations {
            let mut lm = LiveModel::new_gekisou_external(master, &perf, &setup.notes, &setup.events, params, &g.setup)?;
            lm.set_rank_confirmation_timeline(confirmations)?;
            lm
        } else {
            LiveModel::new_gekisou(master, &perf, &setup.notes, &setup.events, params, &g.setup)?
        };
        let mut states = Vec::with_capacity(setup.play.frames.len());
        for (f, &dt) in setup.play.frames.iter().zip(&g.dt) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
            states.push(lm.gekisou_ranges().iter().map(|r| r.state).collect::<Vec<u8>>());
        }
        let pattern = mission_pattern(&g.setup.missions);
        let mut ranges = Vec::with_capacity(g.setup.fevers.len());
        for (i, &(start, end)) in g.setup.fevers.iter().enumerate() {
            let first = |s: u8| states.iter().position(|x: &Vec<u8>| x.get(i).is_some_and(|&v| v >= s));
            let mut pct = 0;
            for r in &master.gekisou_ranking_score_bonuses {
                if r.mission_pattern == pattern && r.count == i as i64 + 1 && r.rank == 1 {
                    pct = r.score_bonus_percent;
                }
            }
            if let Some(confirmations) = &g.confirmations {
                pct = confirmations.iter().find(|c| c.range == i).map_or(0, |c| c.percent);
            }
            let mission = *g.setup.missions.get(i).ok_or_else(|| Error::Input("fewer missions than fevers".into()))?;
            ranges.push(RangeFacts {
                start,
                end,
                mission,
                pct,
                f_start: first(RS_START),
                f_complete: first(RS_COMPLETE),
                f_finish: first(RS_FINISH),
            });
        }
        Ok(Schedule { states, ranges })
    }

    /// A play frame no later than the first one in which a deck of these rows can draw a random number: the first
    /// frame of a luck range in the state Start (its lottery), and for a row with a probability condition the first
    /// frame in which it can be asked (a Gekisou row gated by a mission only once some range has left Wait).
    pub(super) fn first_draw<'r>(&self, env: &Env, rows: impl Iterator<Item = &'r Row>) -> usize {
        let nf = self.states.len();
        let mut f0 =
            self.ranges.iter().filter(|r| r.mission == MISSION_LUCK).filter_map(|r| r.f_start).min().unwrap_or(nf);
        let awake = self.states.iter().position(|s| s.iter().any(|&x| x >= RS_STANDBY)).unwrap_or(nf);
        for r in rows {
            if [r.trigger, r.condition, r.release, r.reset].iter().any(|&g| env.draws(g)) {
                f0 = f0.min(if r.gk && r.gate != MISSION_ALL { awake } else { 0 });
            }
        }
        f0
    }
}

/// What a Gekisou combo factor reads, entries in chart-time order. The factor of a note is `min(table(c), 1) + 1`,
/// `c` the combo count of the first combo range containing its chart time as the range's recount left it at the last
/// of the range's judged notes with a chart time up to the end of the score frame before the note's (the range's
/// history is searched by chart time, which finds that note when the range's notes are judged in chart-time order).
/// Each judgement adds at most `floor(1 + the running combo bonuses)` to the count, so `c` is at most the sum of that
/// over the range's entries up to that time; the table lookup returns the value of some threshold at most `c`.
#[derive(Clone, Debug)]
pub(super) struct GkCombo {
    /// Per entry: its combo range (index into the ranges) and the number of that range's entries (chart order) with
    /// a chart time up to the end of the previous score frame.
    pub(super) range: Vec<Option<u32>>,
    pub(super) upto: Vec<u32>,
    /// Per range: its entries in chart order (empty for a range that is not a combo range), and whether its entries
    /// are judged in chart-time order (else its notes take `gmax`).
    pub(super) entries: Vec<Vec<u32>>,
    pub(super) ordered: Vec<bool>,
    /// Table thresholds in ascending order, each with the largest factor at a count from 0 up to it.
    pub(super) tab: Vec<(i64, f64)>,
    pub(super) gmax: f64,
    /// Entries whose combo-count reading may still come from the previously playing range: the first chart time
    /// of their range and everything within 100 ms of its start. Combo ramps keep their flat factor there.
    pub(super) fresh: Vec<bool>,
}

impl GkCombo {
    /// The largest factor at a count up to `c`.
    pub(super) fn factor(&self, c: f64) -> f64 {
        let i = self.tab.partition_point(|x| (x.0 as f64) <= c);
        if i == 0 { 1.0 } else { self.tab[i - 1].1 }
    }

    /// The factor bound of every entry, from the largest running combo bonus at the chart time of each entry of a
    /// combo range (`bonus(range, position in its entries, count bound of the earlier entries, entries at its chart
    /// time from it on)`); `sums` is scratch.
    pub(super) fn fill(
        &self,
        times: &[i32],
        mut bonus: impl FnMut(usize, usize, f64, usize) -> f64,
        out: &mut Vec<f64>,
        sums: &mut Vec<f64>,
    ) {
        let ne = self.range.len();
        out.clear();
        out.resize(ne, 1.0);
        sums.clear();
        let mut at = Vec::with_capacity(self.entries.len());
        for (ri, list) in self.entries.iter().enumerate() {
            at.push(sums.len());
            if !self.ordered[ri] {
                continue;
            }
            let mut acc = 0f64;
            sums.push(0.0);
            for q in 0..list.len() {
                let t = times[list[q] as usize];
                let group = list[q..].iter().take_while(|&&e| times[e as usize] == t).count();
                acc += (1.0 + bonus(ri, q, acc, group).max(0.0)).floor();
                sums.push(acc);
            }
        }
        for e in 0..ne {
            if let Some(ri) = self.range[e] {
                let ri = ri as usize;
                out[e] = if self.ordered[ri] { self.factor(sums[at[ri] + self.upto[e] as usize]) } else { self.gmax };
            }
        }
    }

    /// After `fill`: per entry, an upper bound on its playing range's combo count over the range's entries with a
    /// chart time up to its own (all bonuses included), or NaN where no ramp may be read.
    pub(super) fn inclusive_counts(&self, times: &[i32], sums: &[f64], out: &mut Vec<f64>) {
        out.clear();
        out.resize(self.range.len(), f64::NAN);
        let mut at = 0usize;
        for (ri, list) in self.entries.iter().enumerate() {
            if !self.ordered[ri] {
                continue;
            }
            let mut q = 0usize;
            while q < list.len() {
                let t = times[list[q] as usize];
                let mut g = q;
                while g + 1 < list.len() && times[list[g + 1] as usize] <= t {
                    g += 1;
                }
                for &e in &list[q..=g] {
                    if !self.fresh[e as usize] {
                        out[e as usize] = sums[at + g + 1];
                    }
                }
                q = g + 1;
            }
            at += list.len() + 1;
        }
    }
}

/// The Gekisou combo factor bound of every deck whose members bring combo bonus windows in at most `n` slots (its
/// carriers; `member_cb`: each allowed member's distinct window lists over its classes). The running bonus of such a
/// deck at a time is at most the sum of the `n` largest members' there (each the largest over its lists), and the
/// count it builds, which opens the gates, is at most the one this bonus builds. `n = 5` covers every deck.
pub(super) fn carrier_factors(
    gc: &GkCombo,
    times: &[i32],
    member_cb: &[Vec<Vec<ComboBonusRow>>],
    n: usize,
    out: &mut Vec<f64>,
) {
    keyed_factors(gc, times, member_cb, &[], n, out);
}

/// `carrier_factors` of the decks with carriers of the window lists `placed` and at most `r` other carriers: the
/// running bonus of such a deck at a time is at most `placed`'s there plus the `r` largest members'.
pub(super) fn keyed_factors(
    gc: &GkCombo,
    times: &[i32],
    member_cb: &[Vec<Vec<ComboBonusRow>>],
    placed: &[&[ComboBonusRow]],
    r: usize,
    out: &mut Vec<f64>,
) {
    let mut sums = Vec::new();
    gc.fill(
        times,
        |ri, q, acc, group| {
            let t = times[gc.entries[ri][q] as usize] as i64;
            let running = |l: &[ComboBonusRow], open: &dyn Fn(Option<(i64, u32)>) -> bool| {
                l.iter().filter(|w| w.0 <= t && t <= w.1 && open(w.3)).map(|w| w.2).sum::<f64>()
            };
            let top = |open: &dyn Fn(Option<(i64, u32)>) -> bool| {
                let mut vals: Vec<f64> =
                    member_cb.iter().map(|lists| lists.iter().map(|l| running(l, open)).fold(0f64, f64::max)).collect();
                vals.sort_by(|a, b| b.total_cmp(a));
                placed.iter().map(|l| running(l, open)).sum::<f64>() + vals.iter().take(r).sum::<f64>()
            };
            // the step reads every window open; a gate opened by it only adds bonus
            let step = (1.0 + top(&|_| true).max(0.0)).floor();
            top(&|gate| gate_open(gate, ri, acc, group, step))
        },
        out,
        &mut sums,
    );
}

/// The running combo bonus of a list of bonus windows at each time of a non-decreasing sequence (`windows`: `(start,
/// end, bonus)`, closed; `ev` is scratch).
/// Whether a combo bonus window with `gate` counts at an entry of combo range `ri`: always without a gate or when it
/// names another range, else once the range's combo can reach the threshold by the entry's chart time: `acc` bounds
/// the count of the earlier entries and each of the `group` entries at that time adds at most `step`.
///
/// Its start needs the threshold counted from judgements processed before that frame; judged in chart order, those
/// are at chart times up to the trigger time (the last combo judgement, or the frame time), and the bonus reaches
/// the judgements from the trigger time on.
pub(super) fn gate_open(gate: Option<(i64, u32)>, ri: usize, acc: f64, group: usize, step: f64) -> bool {
    match gate {
        None => true,
        Some((threshold, range)) => range as usize != ri || acc + group as f64 * step >= threshold as f64,
    }
}

pub(super) fn bonus_at(windows: &[(i64, i64, f64)], ev: &mut Vec<(i64, f64)>) {
    ev.clear();
    for &(a, b, v) in windows {
        ev.push((a, v));
        ev.push((b.saturating_add(1), -v));
    }
    ev.sort_by_key(|x| x.0);
}

/// The Gekisou factors of the per-entry bound (entries in chart-time order) and what the execution count reads.
pub(super) struct GkFactors {
    /// Gekisou combo factor bound `G_e` of every deck (from the largest combo bonuses of any five allowed members),
    /// luck factor bound `L_e`, and `R_e`: 1 plus the rank bonus percents (/ 100) of the completing ranges whose
    /// range score contains the entry.
    pub(super) g: Vec<f64>,
    /// With a combo range: `carriers[n]` bounds the Gekisou combo factor of every deck with at most `n < 5` carrier
    /// slots (a slot whose member and Snap bring Gekisou combo bonus windows); each is at most `g` (empty: none).
    pub(super) carriers: Vec<Vec<f64>>,
    pub(super) l: Vec<f64>,
    pub(super) r: Vec<f64>,
    /// Per completing range with a rank bonus: (range end time, percent / 100, the entries of its range score).
    pub(super) ranks: Vec<(i32, f64, Vec<u32>)>,
    /// Entries of a range score whose confirmation can precede the judgement of an earlier combo break.
    pub(super) nobreak: Vec<bool>,
    /// Per play frame: the earliest time a command filed in the frame can land at (besides the previous frame's time
    /// and the chart times of the frame's notes).
    pub(super) exec_lo: Vec<i32>,
    /// Per completing range `(start, time of its confirmation frame)`: the score frames after the start's up to the
    /// confirmation's are undone and executed once more.
    pub(super) confirm: Vec<(i32, i32)>,
    /// With a combo range: what a candidate's own Gekisou combo factor reads (`g` then bounds it for every deck).
    pub(super) combo: Option<GkCombo>,
}

impl GkFactors {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        master: &Master,
        setup: &FullSetup,
        sc: &Schedule,
        entries: &[(usize, LiveNote, i32)],
        order: &[usize],
        frames: &[i32],
        reached: &[Vec<i32>],
        overrides: (bool, bool),
        current: &[Option<usize>],
        member_cb: &[Vec<Vec<ComboBonusRow>>],
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
            let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nr];
            let mut ordered = vec![false; nr];
            for (r, rf) in ranges.iter().enumerate() {
                if rf.mission != MISSION_COMBO {
                    continue;
                }
                let inr = |t: i32| rf.start <= t && t <= rf.end;
                lists[r] = (0..ne).filter(|&q| inr(times[q])).map(|q| q as u32).collect();
                let proc: Vec<i32> = entries.iter().map(|e| e.1.time_ms).filter(|&t| inr(t)).collect();
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
            let fresh: Vec<bool> = (0..ne)
                .map(|e| match range[e] {
                    None => true,
                    Some(r) => {
                        let first = lists[r as usize].first().map_or(i32::MAX, |&q| times[q as usize]);
                        times[e] <= first.max(ranges[r as usize].start.saturating_add(100))
                    }
                })
                .collect();
            let gc = GkCombo { range, upto, entries: lists, ordered, tab, gmax, fresh };
            carrier_factors(&gc, &times, member_cb, 5, &mut gv);
            for n in 0..5 {
                let mut v = Vec::new();
                carrier_factors(&gc, &times, member_cb, n, &mut v);
                carriers.push(v);
            }
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
            let (score_frames, floors) = rerun_floors(setup, &times, &exec_lo, &reexec);
            rv = network_rank_factors(&judged, &score_frames, &floors, &snapshots);
        }
        Ok(GkFactors { g: gv, carriers, l: lv, r: rv, ranks, nobreak, exec_lo, confirm, combo: gcombo })
    }
}

/// The score frame of each chart time and, per play frame, the first score frame its calculations can re-execute, as
/// [`super::score_windows::Exec`] reads them: the previous play frame's frame, the chart time of each judged note,
/// `exec_lo`, and the frame after the start of a range whose rank bonus applies in that play frame.
fn rerun_floors(setup: &FullSetup, times: &[i32], exec_lo: &[i32], reexec: &[(usize, i32)]) -> (Vec<i32>, Vec<i32>) {
    let max_frame = get_frame(setup.params.music_length_ms).wrapping_add(50).max(1);
    let clamp = |t: i32| {
        let g = get_frame(t);
        if g >= max_frame { max_frame - 1 } else { g.max(0) }
    };
    let notes: std::collections::HashMap<i32, i32> = setup.notes.iter().map(|n| (n.note_id, n.time_ms)).collect();
    let mut floors = Vec::with_capacity(setup.play.frames.len());
    let mut prev_to = 0;
    for (i, f) in setup.play.frames.iter().enumerate() {
        let mut lo = if i == 0 { 0 } else { prev_to };
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
        prev_to = clamp(f.time_ms);
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
fn network_rank_factors(
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
pub(super) fn luck_crossings(frames: &[i32], entries: &[(usize, LiveNote, i32)], sc: &Schedule) -> i64 {
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

// Linear envelopes by Gekisou combo carrier count.
//
// A slot is a carrier when its member and class bring Gekisou combo bonus windows (`Contrib::cb`). The pool-wide
// Gekisou combo factor `G_e` of `Coef::k` reads the five largest members' bonuses, but a deck with at most `n`
// carriers counts at most the `n` largest (`GkFactors::carriers`). Its score is then bounded by the linear envelope
// rebuilt with that factor: every coefficient, window and conversion budget is the pool-wide one except the factor,
// which is at most `G_e`, so each level's `A0`, gains and `global` are at most the pool-wide ones.

/// The linear envelope of decks with at most `n < 5` carriers: `A0`, `global` and the gain of every member, class
/// and position (`gains[m][c][k]`, empty for members that are not allowed).
#[derive(Clone, Debug)]
pub(super) struct CarrierLevel {
    pub(super) a0: f64,
    pub(super) global: f64,
    pub(super) gains: Vec<Vec<[f64; 5]>>,
    /// The level's coefficient of each entry (`Coef::k` with the level's combo factor).
    pub(super) k: Vec<f64>,
}

/// `terms[e] = (pre, combo, rank)` of each entry, so that the pool-wide coefficient is
/// `pre * G_e * combo / cnc * rank`; `factors[n]` the combo factor bound of decks with at most `n` carriers. A level
/// whose factor equals the next one's (or the pool-wide one) is `None`: the next level covers it unchanged.
pub(super) fn carrier_level_envelopes(
    coef: &Coef,
    terms: &[(f64, f64, f64)],
    cnc: f64,
    pool_factor: &[f64],
    factors: &[Vec<f64>],
    members: &[usize],
    contrib: &[Vec<[Contrib; 5]>],
) -> Vec<Option<CarrierLevel>> {
    let ne = coef.times.len();
    debug_assert!(terms.len() == ne && pool_factor.len() == ne && factors.iter().all(|g| g.len() == ne));
    let mut out: Vec<Option<CarrierLevel>> = vec![None; factors.len()];
    for n in 0..factors.len() {
        let g = &factors[n];
        let next = factors.get(n + 1).map_or(pool_factor, |v| &v[..]);
        if g == next {
            continue;
        }
        let (ks, pc, pj) = factor_sums(coef, terms, cnc, |e| g[e]);
        let a0 = pc[ne];
        let mut best = [0f64; 5];
        let mut gains: Vec<Vec<[f64; 5]>> = vec![Vec::new(); contrib.len()];
        for &m in members {
            gains[m] = contrib[m]
                .iter()
                .map(|arr| {
                    std::array::from_fn(|k| {
                        let v = window_gain(&arr[k].windows, arr[k].budget, &pc, &pj);
                        best[k] = best[k].max(v);
                        v
                    })
                })
                .collect();
        }
        let global = best.iter().fold(a0, |sum, &g| sum + g);
        out[n] = Some(CarrierLevel { a0, global, gains, k: ks });
    }
    out
}

/// The coefficients of the combo factor bound `g` (`terms` as in `carrier_level_envelopes`) and their prefix sums
/// `(k, pc, pj)` as in `Coef`, by the operations of the pool-wide coefficients.
fn factor_sums(
    coef: &Coef,
    terms: &[(f64, f64, f64)],
    cnc: f64,
    g: impl Fn(usize) -> f64,
) -> (Vec<f64>, Vec<f64>, [Vec<f64>; 4]) {
    let ne = terms.len();
    let mut pc = vec![0f64; ne + 1];
    let mut pj = [vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1]];
    let mut ks = Vec::with_capacity(ne);
    for (e, &(pre, combo, rank)) in terms.iter().enumerate() {
        // the same operations as the pool-wide coefficient, with a factor at most its `G_e`
        let k = pre * g(e) * combo / cnc * rank;
        ks.push(k);
        pc[e + 1] = pc[e] + coef.z[e] * k * coef.max_jp[e];
        for j in 0..4 {
            pj[j][e + 1] = pj[j][e] + coef.z[e] * k * coef.jp[e][j];
        }
    }
    (ks, pc, pj)
}

/// The gain of a member, class and position (its windows and conversion budget) under the prefix sums of some
/// coefficients, by the operations of the pool-wide gains. The conversion budget read the pool-wide coefficients,
/// never smaller.
fn window_gain(windows: &[Window], budget: f64, pc: &[f64], pj: &[Vec<f64>; 4]) -> f64 {
    let mut v = 0f64;
    for x in windows {
        let (lo, hi) = (x.lo as usize, x.hi as usize);
        v += x.note * (pc[hi] - pc[lo]);
        for j in 0..4 {
            if x.judge[j] != 0.0 {
                v += x.judge[j] * (pj[j][hi] - pj[j][lo]);
            }
        }
    }
    v + budget
}

/// What the Gekisou combo carriers a search prefix placed tell the linear envelope of its completions. With carriers
/// of the window lists `S` placed and `r` slots to fill, a completion's combo factor is at most `keyed_factors(S, r)`
/// and at most the count level `|S| + r`'s; the envelope rebuilt with the smaller of the two (as
/// `carrier_level_envelopes`) bounds its score. The bounds read its `A0` and the gains of the placed slots; the slots to
/// fill keep the count level's gains, which are at least these (every coefficient is).
pub(crate) struct CarrierKeys {
    gc: GkCombo,
    member_cb: Vec<Vec<Vec<ComboBonusRow>>>,
    /// The distinct carrier window lists; the list of every pool member and choice (0 = None, `j + 1` = Snap `j`).
    lists: Vec<Vec<ComboBonusRow>>,
    list_of: Vec<Vec<Option<u16>>>,
    /// The class of every pool member and choice, and the windows and conversion budget of every class by position.
    class_of: Vec<Vec<usize>>,
    windows: Vec<Vec<ClassWindows>>,
    /// Times, `z`, `max_jp` and `jp` of the entries, with `terms` and `cnc` as in `carrier_level_envelopes`.
    coef: Coef,
    terms: Vec<(f64, f64, f64)>,
    cnc: f64,
    /// The combo factor bound of decks with at most `n` carriers, `n` in 0..=5 (5: every deck).
    levels: Vec<Vec<f64>>,
    /// With the additive drift envelope, what its offset reads; the factor commands and their largest factor of every
    /// pool member and choice at any position.
    additive: Option<KeyedDrift>,
    commands: Vec<Vec<(f64, f64)>>,
    /// The envelopes built so far by sorted placed lists and slots to fill (at most `KEYED_CACHE`).
    cache: RefCell<HashMap<KeyedKey, Rc<KeyedEnvelope>>>,
}

/// The windows and conversion budget of a member's class at each position.
type ClassWindows = [(Vec<Window>, f64); 5];

/// A keyed envelope's key: the sorted placed lists (zero-padded), their count and the slots to fill.
type KeyedKey = ([u16; 5], usize, usize);

/// The most keyed envelopes kept at a time; a search visits few placed carrier lists below each prefix, so dropping
/// them all when full rebuilds few.
const KEYED_CACHE: usize = 256;

/// What the additive drift offset of a keyed envelope reads (see `SnapLive::joint_additive`): the pool-wide rounding
/// count, the largest judgement factor, the chain allowance, the frame executions and the per-position command and
/// factor maxima in descending order.
pub(super) struct KeyedDrift {
    pub(super) roundings: f64,
    pub(super) judgement_max: f64,
    pub(super) chain_extra: f64,
    pub(super) e_max: f64,
    pub(super) cmd_top: [f64; 5],
    pub(super) fac_top: [f64; 5],
}

/// The envelope of the completions of one placed carrier lists and count of slots to fill (see `CarrierKeys`): `A0`
/// without the drift offset and the factor error sensitivity of its coefficients.
pub(crate) struct KeyedEnvelope {
    a0: f64,
    sensitivity: f64,
    pc: Vec<f64>,
    pj: [Vec<f64>; 4],
    gains: RefCell<HashMap<(usize, usize), [f64; 5]>>,
}

impl CarrierKeys {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        gc: &GkCombo,
        coef: &Coef,
        terms: Vec<(f64, f64, f64)>,
        cnc: f64,
        levels: Vec<Vec<f64>>,
        members: &[usize],
        class_of: &[Vec<u16>],
        contrib: &[Vec<[Contrib; 5]>],
        additive: Option<KeyedDrift>,
    ) -> CarrierKeys {
        let mut lists: Vec<Vec<ComboBonusRow>> = Vec::new();
        let mut list_of = vec![Vec::new(); contrib.len()];
        let mut choice_class = vec![Vec::new(); contrib.len()];
        let mut windows = vec![Vec::new(); contrib.len()];
        let mut commands = vec![Vec::new(); contrib.len()];
        let mut member_cb = Vec::with_capacity(members.len());
        for &m in members {
            let classes: Vec<usize> = std::iter::once(0).chain(class_of[m].iter().map(|&c| c as usize)).collect();
            let mut own: Vec<Vec<ComboBonusRow>> = Vec::new();
            list_of[m] = classes
                .iter()
                .map(|&c| {
                    let cb = &contrib[m][c][0].cb;
                    if cb.is_empty() {
                        return None;
                    }
                    if !own.contains(cb) {
                        own.push(cb.clone());
                    }
                    let id = lists.iter().position(|l| l == cb).unwrap_or_else(|| {
                        lists.push(cb.clone());
                        lists.len() - 1
                    });
                    Some(u16::try_from(id).expect("at most 65536 distinct carrier lists"))
                })
                .collect();
            member_cb.push(own);
            commands[m] = classes
                .iter()
                .map(|&c| {
                    let arr = &contrib[m][c];
                    (arr.iter().map(|x| x.cmds).fold(0f64, f64::max), arr.iter().map(|x| x.fac).fold(0f64, f64::max))
                })
                .collect();
            choice_class[m] = classes;
            windows[m] = contrib[m].iter().map(|arr| arr.each_ref().map(|x| (x.windows.clone(), x.budget))).collect();
        }
        CarrierKeys {
            gc: gc.clone(),
            member_cb,
            lists,
            list_of,
            class_of: choice_class,
            windows,
            coef: Coef {
                times: coef.times.clone(),
                z: coef.z.clone(),
                max_jp: coef.max_jp.clone(),
                jp: coef.jp.clone(),
                ..Default::default()
            },
            terms,
            cnc,
            levels,
            additive,
            commands,
            cache: RefCell::default(),
        }
    }

    /// The window list of a pool member and choice when it is a carrier.
    pub(crate) fn list(&self, m: usize, choice: usize) -> Option<u16> {
        self.list_of[m][choice]
    }

    /// The envelope of the completions of a prefix with carriers of the lists `placed` and `r` slots to fill.
    pub(crate) fn envelope(&self, placed: &[u16], r: usize) -> Rc<KeyedEnvelope> {
        let mut ids = placed.to_vec();
        ids.sort_unstable();
        let mut key: KeyedKey = ([0u16; 5], ids.len(), r);
        key.0[..ids.len()].copy_from_slice(&ids);
        if let Some(e) = self.cache.borrow().get(&key) {
            return e.clone();
        }
        let lists: Vec<&[ComboBonusRow]> = ids.iter().map(|&i| &self.lists[i as usize][..]).collect();
        let mut g = Vec::new();
        keyed_factors(&self.gc, &self.coef.times, &self.member_cb, &lists, r, &mut g);
        let level = &self.levels[(ids.len() + r).min(5)];
        let (ks, pc, pj) = factor_sums(&self.coef, &self.terms, self.cnc, |e| g[e].min(level[e]));
        let a0 = pc[ks.len()];
        // the completions read coefficients at most these, so their factor error sensitivity is at most theirs
        let sensitivity = self.additive.as_ref().map_or(0.0, |d| {
            let coef = Coef { k: ks, z: self.coef.z.clone(), ..Default::default() };
            factor_error_sensitivity(&coef, d.judgement_max).expect("keyed sensitivity at most the pool-wide one")
        });
        let e = Rc::new(KeyedEnvelope { a0, sensitivity, pc, pj, gains: RefCell::default() });
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= KEYED_CACHE {
            cache.clear();
        }
        cache.insert(key, e.clone());
        e
    }

    /// `A0` of an envelope for completions of the placed performers `placed` (pool member, choice) with `free` slots to
    /// fill. With the additive drift envelope, its offset reads the drift of these performers' commands and of the
    /// most any performers file in the other slots (`factor_drift`), at most the pool-wide one.
    pub(crate) fn a0(&self, env: &KeyedEnvelope, placed: impl Iterator<Item = (usize, usize)>, free: usize) -> f64 {
        self.a0_of(env, self.commands_of(placed, free))
    }

    /// The factor commands and the factor total of the placed performers `placed` (pool member, choice) and of the
    /// most any performers file in `free` other slots.
    pub(crate) fn commands_of(&self, placed: impl Iterator<Item = (usize, usize)>, free: usize) -> (f64, f64) {
        let (mut n_cmd, mut f_tot) = (0f64, 0f64);
        for (m, choice) in placed {
            (n_cmd, f_tot) = self.with_performer((n_cmd, f_tot), m, choice);
        }
        if let Some(d) = &self.additive {
            for k in 0..free {
                n_cmd = (n_cmd + d.cmd_top[k]).next_up();
                f_tot = (f_tot + d.fac_top[k]).next_up();
            }
        }
        (n_cmd, f_tot)
    }

    /// `commands_of` with one more performer.
    pub(crate) fn with_performer(&self, (n_cmd, f_tot): (f64, f64), m: usize, choice: usize) -> (f64, f64) {
        let (cmds, fac) = self.commands[m][choice];
        ((n_cmd + cmds).next_up(), (f_tot + fac).next_up())
    }

    /// `A0` of an envelope for completions whose performers file at most `n_cmd` factor commands with factors summing
    /// to at most `f_tot` (`commands_of`).
    pub(crate) fn a0_of(&self, env: &KeyedEnvelope, (n_cmd, f_tot): (f64, f64)) -> f64 {
        let Some(d) = &self.additive else { return env.a0 };
        let drift = factor_drift(d.e_max, n_cmd, f_tot).expect("prefix drift at most the pool-wide one");
        let delta = float_margin::with_chain(drift, d.chain_extra).expect("finite prefix drift");
        additive_joint_envelope(env.a0, env.a0, delta, d.roundings, env.sensitivity, d.chain_extra)
            .expect("prefix envelope at most the pool-wide one")
            .0
    }

    /// The gains of a placed pool member and choice under an envelope, by position.
    pub(crate) fn gains(&self, env: &KeyedEnvelope, m: usize, choice: usize) -> [f64; 5] {
        if let Some(g) = env.gains.borrow().get(&(m, choice)) {
            return *g;
        }
        let class = self.class_of[m][choice];
        let g = self.windows[m][class].each_ref().map(|(w, budget)| window_gain(w, *budget, &env.pc, &env.pj));
        env.gains.borrow_mut().insert((m, choice), g);
        g
    }
}

/// Whether member `m`'s class `c` brings Gekisou combo bonus windows (the windows do not depend on the position).
pub(super) fn is_carrier_class(contrib: &[Vec<[Contrib; 5]>], m: usize, c: usize) -> bool {
    contrib[m].get(c).is_some_and(|arr| !arr[0].cb.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_snapshot_envelope_covers_negative_retroactive_range_scores() {
        // Snapshot raw values can vary independently inside the per-note cap. Include negative early bonuses
        // read by later start snapshots, late packets, two ranks applied together, and notes judged after an end.
        let frames = [0, 2, 4, 7];
        let snapshots = [(3, 0, Some(1), Some(2), 150), (6, 1, Some(4), Some(5), 70), (6, 2, Some(2), Some(4), 30)];
        let caps = [31.0, 67.0, 43.0, 101.0];
        let factors = network_rank_factors(&frames, &[0; 4], &[i32::MIN; 8], &snapshots);
        let bound: f64 = factors.iter().zip(caps).map(|(r, v)| r * v).sum();
        for bits in 0..4096u64 {
            let mut bonus = Vec::new();
            for (i, &(_, _, start, end, pct)) in snapshots.iter().enumerate() {
                let raw = |at: Option<usize>, start: bool, shift: usize| -> f64 {
                    frames
                        .iter()
                        .zip(caps)
                        .enumerate()
                        .filter(|(e, (frame, _))| {
                            at.is_some_and(|at| if start { **frame < at } else { **frame <= at })
                                && (bits >> ((shift + e) % 12)) & 1 != 0
                        })
                        .map(|(_, (_, cap))| cap)
                        .sum()
                };
                let mut s0 = raw(start, true, i * 4);
                let mut s1 = raw(end, false, i * 4 + 2);
                for (j, &(applied, ..)) in snapshots[..i].iter().enumerate() {
                    if start.is_some_and(|start| applied < start) {
                        s0 += bonus[j];
                    }
                    if end.is_some_and(|end| applied < end) {
                        s1 += bonus[j];
                    }
                }
                bonus.push(((s1 - s0) * pct as f64 / 100.0).trunc());
            }
            let score = caps.iter().sum::<f64>() + bonus.iter().sum::<f64>();
            assert!(score <= bound, "{bits}: {score} > {bound}");
        }
        assert!(factors[3] < 1.000001, "notes after every end receive no rank contribution");
    }

    #[test]
    fn network_snapshot_envelope_cancels_held_notes_and_filed_bonuses() {
        // A note changes only in its judgement frame and in play frames whose calculations re-execute its score
        // frame. A bonus files with the calculation after its application, or (a stronger adversary) at once.
        let judged = [0, 1, 3, 5, 8];
        let score_frames = [0, 2, 4, 6, 9];
        let floors = [0, 0, 0, 3, 3, 3, 5, 5, 5, 8];
        let snapshots = [(2, 0, Some(1), Some(2), 150), (6, 1, Some(5), Some(6), 80), (7, 2, Some(6), Some(7), 40)];
        let caps = [31.0, 67.0, 43.0, 101.0, 59.0];
        let factors = network_rank_factors(&judged, &score_frames, &floors, &snapshots);
        let bound: f64 = factors.iter().zip(caps).map(|(r, v)| r * v).sum();
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            // value[e][k]: the note's value after the calculations of play frame k
            let mut value = vec![vec![0.0f64; floors.len()]; caps.len()];
            for (e, row) in value.iter_mut().enumerate() {
                let mut v = 0.0;
                for (k, slot) in row.iter_mut().enumerate() {
                    let can = k == judged[e] || (k > judged[e] && floors[k] <= score_frames[e]);
                    if can {
                        v = caps[e] * (next() % 5) as f64 / 4.0;
                    }
                    *slot = if k >= judged[e] { v } else { 0.0 };
                }
            }
            let at_once: Vec<bool> = snapshots.iter().map(|_| next() & 1 != 0).collect();
            let mut bonus: Vec<f64> = Vec::new();
            for (i, &(_, _, start, end, pct)) in snapshots.iter().enumerate() {
                let (s, e) = (start.unwrap(), end.unwrap());
                // The start snapshot opens play frame `s`, after the applications of frame `s - 1`; the end
                // snapshot precedes the applications of frame `e`.
                let total = |k: usize, filed: &dyn Fn(usize) -> bool| -> f64 {
                    value.iter().map(|row| row[k]).sum::<f64>()
                        + (0..i).filter(|&j| filed(j)).map(|j| bonus[j]).sum::<f64>()
                };
                let s0 = total(s - 1, &|j| s - 1 > snapshots[j].0 || (at_once[j] && s - 1 == snapshots[j].0));
                let s1 = total(e, &|j| e > snapshots[j].0);
                bonus.push(((s1 - s0) * pct as f64 / 100.0).trunc());
            }
            let last = floors.len() - 1;
            let score = value.iter().map(|row| row[last]).sum::<f64>() + bonus.iter().sum::<f64>();
            assert!(score <= bound, "{score} > {bound}");
        }
        // The first note moves only within the first range; the first bonus is in both snapshots of the later ones.
        assert!(factors[0] < 2.51, "{factors:?}");
        assert!(factors[4] < 1.000001);
    }

    #[test]
    fn gated_bonus_opens_once_the_time_group_can_reach_the_threshold() {
        // one ordered combo range of four entries, two of them at the same chart time
        let times = [10, 20, 20, 30];
        let gc = GkCombo {
            range: vec![Some(0); 4],
            upto: vec![0; 4],
            entries: vec![vec![0, 1, 2, 3]],
            ordered: vec![true],
            tab: Vec::new(),
            gmax: 1.0,
            fresh: vec![false; 4],
        };
        // a +5 bonus gated at a count of 10 in range 0
        let w: ComboBonusRow = (0, 100, 5.0, Some((10, 0)));
        let (mut out, mut sums) = (Vec::new(), Vec::new());
        gc.fill(
            &times,
            |ri, _, acc, group| {
                let step = (1.0 + w.2).floor();
                if gate_open(w.3, ri, acc, group, step) { w.2 } else { 0.0 }
            },
            &mut out,
            &mut sums,
        );
        // closed for the first entry (0 + 6 < 10); the pair at 20 can reach it (1 + 2 * 6), so both count the bonus
        assert_eq!(sums, vec![0.0, 1.0, 7.0, 13.0, 19.0]);
        // a gate naming another range never closes
        assert!(gate_open(Some((1000, 1)), 0, 0.0, 1, 1.0));
        assert!(gate_open(None, 0, 0.0, 1, 1.0));
    }

    #[test]
    fn carrier_factors_cover_every_deck_with_that_many_carriers() {
        // two ordered combo ranges, entries at repeated chart times, a table rising to 1.3
        let times: Vec<i32> = (0..40).map(|i| 100 + 40 * (i / 2) + if i >= 20 { 1000 } else { 0 }).collect();
        let gc = GkCombo {
            range: (0..40).map(|i| Some(u32::from(i >= 20))).collect(),
            upto: (0..40).map(|i| (i % 20) as u32 / 2 * 2).collect(),
            entries: vec![(0..20).collect(), (20..40).collect()],
            ordered: vec![true, true],
            tab: vec![(5, 1.05), (20, 1.1), (60, 1.2), (150, 1.3)],
            gmax: 1.3,
            fresh: vec![false; 40],
        };
        // per member, its window lists: plain and gated bonuses of different lengths
        let mut seed = 7u64;
        let mut next = move |m: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        let members: Vec<Vec<Vec<ComboBonusRow>>> = (0..7)
            .map(|_| {
                (0..1 + next(2))
                    .map(|_| {
                        (0..1 + next(2))
                            .map(|_| {
                                let a = 100 + 40 * next(25) as i64;
                                let gate = (next(2) == 0).then(|| (1 + next(12) as i64, next(2) as u32));
                                (a, a + 40 * next(12) as i64, (1 + next(5)) as f64, gate)
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        let bound: Vec<Vec<f64>> = (0..=5)
            .map(|n| {
                let mut v = Vec::new();
                carrier_factors(&gc, &times, &members, n, &mut v);
                v
            })
            .collect();
        for n in 0..5 {
            assert!(bound[n].iter().zip(&bound[n + 1]).all(|(a, b)| a <= b), "{n}");
        }
        // every deck: a subset of the members, each with one of its lists, through the candidate fill
        for mask in 0usize..1 << members.len() {
            if mask.count_ones() > 5 {
                continue;
            }
            let picks: Vec<usize> = (0..members.len()).filter(|&m| mask & (1 << m) != 0).collect();
            for choice in 0..1usize << picks.len() {
                let windows: Vec<ComboBonusRow> = picks
                    .iter()
                    .enumerate()
                    .flat_map(|(i, &m)| members[m][((choice >> i) & 1).min(members[m].len() - 1)].iter().copied())
                    .collect();
                let (mut own, mut sums) = (Vec::new(), Vec::new());
                gc.fill(
                    &times,
                    |ri, q, acc, group| {
                        let t = times[gc.entries[ri][q] as usize] as i64;
                        let running = || windows.iter().filter(|w| w.0 <= t && t <= w.1);
                        let step = (1.0 + running().map(|w| w.2).sum::<f64>()).floor();
                        running().filter(|w| gate_open(w.3, ri, acc, group, step)).map(|w| w.2).sum()
                    },
                    &mut own,
                    &mut sums,
                );
                let level = &bound[picks.len()];
                assert!(own.iter().zip(level).all(|(a, b)| a <= b), "{picks:?} {choice}");
                // any of its carriers placed with their lists, the others among the slots to fill
                for placed in 0usize..1 << picks.len() {
                    let lists: Vec<&[ComboBonusRow]> = picks
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| placed & (1 << i) != 0)
                        .map(|(i, &m)| &members[m][((choice >> i) & 1).min(members[m].len() - 1)][..])
                        .collect();
                    let mut keyed = Vec::new();
                    keyed_factors(&gc, &times, &members, &lists, picks.len() - lists.len(), &mut keyed);
                    assert!(own.iter().zip(&keyed).all(|(a, b)| a <= b), "{picks:?} {choice} {placed}");
                }
            }
        }
        // fewer carriers really count less here
        assert!(bound[1].iter().sum::<f64>() < bound[5].iter().sum::<f64>());
    }
}
