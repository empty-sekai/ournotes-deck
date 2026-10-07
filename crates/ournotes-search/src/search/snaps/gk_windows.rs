//! Gekisou row timing windows, budgets and cumulative command churn.
use super::*;

/// An active row as the bounds read it. A cumulative note score up (2001) is bounded by its largest factor: the
/// effect value times the largest count of its cumulative condition, capped by its maximum effect value when that is
/// positive (the simulation multiplies in 128 bits and truncates to 32; the search requires the product to fit).
pub(super) fn active_row(env: &Env, r: &Row, can_start: bool, event_bound: bool) -> Result<ActiveRow, Error> {
    let mut value = r.value;
    if r.effect_type == 2001 {
        let cmax = if r.cumulative == 0 {
            0
        } else {
            let c = env
                .master
                .cumulative_condition(r.cumulative)
                .ok_or_else(|| Error::Master(format!("unknown cumulative condition {}", r.cumulative)))?;
            if c.condition_values.first().is_none_or(|&n| n < 1) {
                return Err(Error::Domain("cumulative condition with a unit below 1".into()));
            }
            if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count }
        };
        let prod = r.value as i128 * cmax as i128;
        if r.value < 0 || prod > i32::MAX as i128 {
            return Err(Error::Domain("cumulative note score up outside the modelled range".into()));
        }
        value = if r.max_value > 0 && (r.max_value as i128) < prod { r.max_value } else { prod as i64 };
    }
    let w = r.gk.then(|| gk_row(env, r));
    Ok(ActiveRow {
        effect_type: r.effect_type,
        value,
        act: r.act,
        event_bound,
        can_start,
        start_limit: factor_start_limit(env, r),
        count_win: factor_count_windows(env, r),
        targets: r.targets.clone(),
        churn: r.effect_type == 2001,
        churn_max: churn_max(env, r),
        gk_win: w
            .as_ref()
            .map(|x| if FILED_IN_ORDER.contains(&r.effect_type) { x.filed.clone() } else { x.win.clone() }),
        gk_starts: w.as_ref().map(|x| x.win_starts.clone()),
        gk_gate: w.as_ref().and_then(|x| combo_gate(env, r).map(|t| (t, x.parts.clone()))),
        gk_execs: w.as_ref().map(|x| factor_executions(r, x, env.gkf.as_deref())),
        gk_event_win: (r.gk && event_bound).then(|| std::array::from_fn(|k| gk_event_windows(env, r, k))),
        gk_conv: w.as_ref().map(|x| x.conv.clone()),
        budget: w.as_ref().and_then(|x| gk_budget(env, r, x)),
        cumulative_ramp: ramp::windows(env, r, event_bound),
        combo_ramp: combo_ramp(env, r),
        rush: rush::spec(env, r),
        rush_run_cap: rush::run_cap_eligible(env, r),
    })
}

/// A one-shot fixed factor starts at most once for each true trigger frame. Every alternative must contain a
/// positive judgement counter; its original target multiplicities and the complete conversion reach bound
/// the number of hits. Duration, condition failure, resets, execution limits and a full pool cannot add hits.
/// Sustained and cumulative lifecycles retain their independent command-count certificates.
fn factor_start_limit(env: &Env, r: &Row) -> Option<f64> {
    if r.gk || r.trigger_type != 1 || !matches!(r.effect_type, 2000 | 2004) {
        return None;
    }
    let hits = env.gkf.as_ref()?.count_trigger_hits(env, r.trigger, MISSION_ALL)?;
    (hits >= 0).then(|| (hits as f64).next_up())
}

/// Necessary start windows for an ordinary one-shot fixed factor with one positive 1030 trigger. With the same
/// target multiplicity under every reachable final judgement, this checker has a deterministic counter residue
/// and hit frames. It is checked before row conditions and pool/execute limits; the reset group resets only the
/// execution limit. None of those can move a hit to another frame. Composite and consecutive checkers retain the
/// generic envelope because short-circuiting and counter resets can move their hits.
///
/// Keep one window per hit frame, including when one frame has several hits or there are more hits than pool
/// instances. The lone checker's timestamp can be backdated to a counted note: `wlo` includes every such time,
/// while the current processing time bounds its latest end. The score-proof domain has nonnegative, ordered
/// clocks and no note judged before its chart time, so elapsed subtraction cannot wrap. Ordinary 15000 extensions
/// affect only the member's live-skill pool, not these conditional updaters.
fn factor_count_windows(env: &Env, r: &Row) -> Option<Vec<(i64, i64, f64)>> {
    if r.gk || r.trigger_type != 1 || r.trigger == 0 || r.release != 0 || !matches!(r.effect_type, 2000 | 2004) {
        return None;
    }
    let mut condition = None;
    for set in env.sets.get(&r.trigger)? {
        for &id in set.iter() {
            let c = env.master.skill_condition(id)?;
            if c.condition_type != 0 && condition.replace(c).is_some() {
                return None;
            }
        }
    }
    let c = condition?;
    let n = *c.condition_values.first()?;
    if !c.is_positive || c.condition_type != 1030 || !(1..=i32::MAX as i64).contains(&n) {
        return None;
    }
    let targets: Vec<_> = c
        .condition_target_ids
        .iter()
        .map(|&id| env.master.skill_target(id).map(|t| t.judgement))
        .collect::<Option<Vec<_>>>()?;
    let g = env.gkf.as_ref()?;
    if g.wlo.len() != g.times.len() || g.times.iter().any(|&t| t < 0) || !g.times.is_sorted() {
        return None;
    }
    let (mut count, mut previous) = (0u128, 0usize);
    let mut starts = Vec::new();
    for (i, &(frame, raw)) in g.ent.iter().enumerate() {
        if frame >= g.times.len() || frame < previous || !(0..8).contains(&raw) {
            return None;
        }
        previous = frame;
        let reach = env.reach_of(i, raw);
        let mut multiplicity = None;
        for j in 0..8 {
            if reach & (1u8 << j) == 0 {
                continue;
            }
            let m = targets.iter().filter(|&&target| target == j).count();
            if multiplicity.is_some_and(|old| old != m) {
                return None;
            }
            multiplicity = Some(m);
        }
        // With n <= i32::MAX the native counter resets before its signed increment can wrap. Duplicates may
        // produce several hits in one entry, but the updater starts only once for the frame's true result.
        let total = count + multiplicity? as u128;
        count = total % n as u128;
        if total >= n as u128 && starts.last().copied() != Some(frame) {
            starts.push(frame);
        }
    }
    starts
        .into_iter()
        .map(|frame| {
            let start = g.wlo[frame];
            (start <= g.times[frame] as i64).then(|| (start, frame_end(&g.times, g.times[frame], frame, r.act), 1.0))
        })
        .collect()
}

/// Lifetime starts for fixed Gekisou score factors. An untimed sustained updater keeps `current` while its
/// trigger is true, even when its condition fails. A false observed frame ends it; only a later frame can recycle
/// and start it again. Thus starts in a component's processing-frame interval `[lo, hi]` cannot be adjacent
/// among the frames where its mission gate is open. Closed gates leave the sustained updater and its trigger
/// cache untouched. Counting finished open-gate frames is conservative because they cannot start an effect.
///
/// Project after the timing cache: its key omits effect type, and other appliers can end an updater themselves.
/// Fixed 2000/2004 appliers only file start/end factor commands. Preserve the full generic timing domain for
/// other lifecycles, including cumulative factors and releases.
fn factor_executions(r: &Row, w: &GkRowWin, frames: Option<&GkFrames>) -> Vec<f64> {
    let mut executions = w.executions.clone();
    if !r.gk || r.trigger_type != 2 || r.act != 0.0 || r.release != 0 || !matches!(r.effect_type, 2000 | 2004) {
        return executions;
    }
    for (count, starts) in executions.iter_mut().zip(&w.win_starts) {
        if !count.is_finite() || *count < 0.0 {
            continue;
        }
        // These final prefix extrema are over actual start-frame indices, not filing-time order.
        let Some((&lo, &hi)) = starts.lo.last().zip(starts.hi.last()) else { continue };
        let Some(distance) = hi.checked_sub(lo) else { continue };
        let processing_frames = u64::from(distance) + 1;
        let observed_frames = frames
            .filter(|_| (1..=3).contains(&r.gate))
            .and_then(|frames| frames.gate[(r.gate - 1) as usize].get(lo as usize..=hi as usize))
            .map_or(processing_frames, |gate| gate.iter().filter(|&&open| open).count() as u64);
        let cap = observed_frames.div_ceil(2);
        *count = (*count).min((cap as f64).next_up());
    }
    executions
}

/// The threshold of a sustained Gekisou combo bonus (12000) whose trigger is one positive Gekisou combo count
/// condition (7005), when the whole-pool combo certificate admits the pool (no direct count additions, see
/// `combo_triggers`). Such a bonus starts only in a frame whose playing range already counts the threshold.
fn combo_gate(env: &Env, r: &Row) -> Option<i64> {
    if !r.gk || r.effect_type != 12000 || r.trigger_type != 2 {
        return None;
    }
    env.gkf.as_ref()?.combo_triggers.as_ref()?;
    let sets = env.sets.get(&r.trigger)?;
    if sets.len() != 1 || sets[0].len() != 1 {
        return None;
    }
    let c = env.master.skill_condition(sets[0][0])?;
    if c.condition_type != 7005 || !c.is_positive {
        return None;
    }
    c.condition_values.first().copied().filter(|&t| t > 0)
}

/// The note factor of a Gekisou cumulative note score up (2001) that counts the playing range's combo (7001), at
/// each unit count until its value stops growing: `min(value * min(count, max), cap)` with the native product
/// range, through the same mill quantization as the flat window. None outside that domain or above 4096 steps.
fn combo_ramp(env: &Env, r: &Row) -> Option<(i64, i64, Rc<Vec<f64>>)> {
    // The frame-read ramp requires a running updater to refresh its cumulative count each frame.
    // Untimed sustained effects retain the count from their first activation, including after the
    // playing range changes. Keep their flat peak window until a separate held-factor bound exists.
    if !r.gk || r.trigger_type != 1 || r.effect_type != 2001 || r.value <= 0 || r.cumulative == 0 {
        return None;
    }
    let c = env.master.cumulative_condition(r.cumulative)?;
    if c.condition_type != 7001 {
        return None;
    }
    let unit = *c.condition_values.first()?;
    if unit < 1 {
        return None;
    }
    let max_count = if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count };
    let mut table = Vec::new();
    for k in 0..=4096i64 {
        let count = k.min(max_count);
        let product = r.value as i128 * count as i128;
        if product > i32::MAX as i128 {
            return None;
        }
        let mut value = product as i64;
        let capped = r.max_value > 0 && r.max_value <= value;
        if capped {
            value = r.max_value;
        }
        table.push(note_factor_mill(value as f32 / 10000f32).max(0) as f64 / 1e5);
        if capped || count == max_count {
            return Some((unit, max_count, Rc::new(table)));
        }
    }
    None
}

/// Per play frame, what the gates and the triggers of Gekisou rows read, from the ranges' schedule.
#[derive(Debug)]
pub(super) struct GkFrames {
    pub(super) times: Vec<i32>,
    /// Whether each mission's gate (1..=3) is open: a range of the mission changes state, or is the playing range.
    pub(super) gate: [Vec<bool>; 3],
    /// The playing range of each frame, if any.
    pub(super) current: Vec<Option<usize>>,
    /// The frames in which some range turns Start (per range) and in which some range turns Complete.
    pub(super) start: Vec<Option<usize>>,
    pub(super) complete: Vec<bool>,
    pub(super) ranges: Vec<RangeFacts>,
    pub(super) states: Vec<Vec<u8>>,
    /// The earliest trigger time of a row started in each frame: the frame before last (a range start), the chart
    /// time of a note judged in the frame (a judgement count), the start of a range in play (its combo).
    pub(super) wlo: Vec<i64>,
    /// The first frame from each frame on in which some range turns Complete.
    pub(super) next_complete: Vec<Option<usize>>,
    /// Frame index and raw judgement of each stream entry, in processing order.
    pub(super) ent: Vec<(usize, i32)>,
    /// Optional whole-pool certificate of count-threshold trigger frames and
    /// direct7005 override timestamps; never reads this frame's new judgements.
    pub(super) combo_triggers: Option<combo_triggers::ComboTriggers>,
}

impl GkFrames {
    pub(super) fn new(sc: &Schedule, frames: &[i32], entries: &[(usize, LiveNote, i32)]) -> GkFrames {
        let nf = frames.len();
        let nr = sc.ranges.len();
        let st = |f: usize, r: usize| sc.states[f][r];
        let prev = |f: usize, r: usize| if f == 0 { 1 } else { sc.states[f - 1][r] };
        let mut order: Vec<usize> = (0..nr).filter(|&r| sc.ranges[r].f_start.is_some()).collect();
        order.sort_by_key(|&r| (sc.ranges[r].f_start, r));
        let mut current = vec![None; nf];
        let mut gate: [Vec<bool>; 3] = std::array::from_fn(|_| vec![false; nf]);
        let mut complete = vec![false; nf];
        for f in 0..nf {
            let mut c = None;
            for &r in &order {
                if sc.ranges[r].f_start.is_some_and(|s| s < f) && matches!(prev(f, r), RS_START | RS_PLAYING | RS_END) {
                    c = Some(r);
                }
            }
            for r in 0..nr {
                if st(f, r) == RS_START && prev(f, r) < RS_START {
                    c = Some(r);
                }
            }
            current[f] = c;
            for r in 0..nr {
                if st(f, r) != prev(f, r) {
                    if let Some(g) = gate.get_mut((sc.ranges[r].mission - 1) as usize) {
                        g[f] = true;
                    }
                    if st(f, r) == RS_COMPLETE {
                        complete[f] = true;
                    }
                }
            }
            if let Some(g) = c.and_then(|r| gate.get_mut((sc.ranges[r].mission - 1) as usize)) {
                g[f] = true;
            }
        }
        let mut wlo = vec![i64::MIN; nf];
        for f in 0..nf {
            if f < 2 {
                continue;
            }
            let mut lo = frames[f - 2] as i64;
            for (r, rf) in sc.ranges.iter().enumerate() {
                if (RS_START..=RS_END).contains(&st(f, r)) {
                    lo = lo.min(rf.start as i64);
                }
            }
            wlo[f] = lo;
        }
        for e in entries {
            wlo[e.0] = wlo[e.0].min(e.1.time_ms as i64);
        }
        let mut next_complete = vec![None; nf + 3];
        for f in (0..nf).rev() {
            next_complete[f] = if complete[f] { Some(f) } else { next_complete[f + 1] };
        }
        GkFrames {
            times: frames.to_vec(),
            gate,
            current,
            start: sc.ranges.iter().map(|r| r.f_start).collect(),
            complete,
            ranges: sc.ranges.clone(),
            states: sc.states.clone(),
            wlo,
            next_complete,
            ent: entries.iter().map(|e| (e.0, e.2)).collect(),
            combo_triggers: None,
        }
    }

    pub(super) fn gate_open(&self, gate: i64, f: usize) -> bool {
        match gate {
            MISSION_ALL => true,
            1..=3 => self.gate[(gate - 1) as usize][f],
            _ => false,
        }
    }

    /// The frames in which a positive condition can hold (`None`: any frame).
    pub(super) fn cond_frames(&self, env: &Env, cid: i64, gate: i64) -> Option<Vec<bool>> {
        let c = env.master.skill_condition(cid)?;
        if !c.is_positive {
            return None;
        }
        let nf = self.times.len();
        let missions = || -> Vec<i64> {
            c.condition_target_ids
                .iter()
                .filter_map(|&t| env.master.skill_target(t))
                .filter(|t| t.skill_target_type == 5 && t.gekisou_mission_type != 0)
                .map(|t| t.gekisou_mission_type)
                .collect()
        };
        let matches = |ms: &[i64], m: i64| ms.is_empty() || ms.contains(&MISSION_ALL) || ms.contains(&m);
        let luck: Vec<&RangeFacts> = self.ranges.iter().filter(|r| r.mission == MISSION_LUCK).collect();
        let mut out = vec![false; nf];
        match c.condition_type {
            1000 | 1010 | 1020 => {
                // Judgement-match/count checkers can return true only while consuming
                // a judgement in this frame. Allow every raw entry regardless of target
                // or possible conversion, but never invent a count trigger in an empty
                // completion/finish frame that could leak a long conversion past a range.
                for &(frame, _) in &self.ent {
                    out[frame] = true;
                }
            }
            1030 | 1040 => {
                let threshold = *c.condition_values.first()?;
                if threshold < 1 || self.ent.iter().any(|&(_, j)| !(0..8).contains(&j)) {
                    return None;
                }
                let targets: Vec<_> = c
                    .condition_target_ids
                    .iter()
                    .map(|&id| env.master.skill_target(id).map(|t| t.judgement))
                    .collect::<Option<Vec<_>>>()?;
                let mut per_frame = vec![0i64; nf];
                for (i, &(f, raw)) in self.ent.iter().enumerate() {
                    if !self.gate_open(gate, f) {
                        continue;
                    }
                    // Duplicate judgement targets can increment the native counter more than
                    // once per note. Maximize over this note's reachable final judgement.
                    let reach = env.reach_of(i, raw);
                    let hits = (0..8)
                        .filter(|&j| reach & (1u8 << j) != 0)
                        .map(|j| targets.iter().filter(|&&t| t == j).count() as i64)
                        .max()
                        .unwrap_or(0);
                    per_frame[f] = per_frame[f].saturating_add(hits);
                }
                let mut possible_count = 0i64;
                for f in 0..nf {
                    possible_count = possible_count.saturating_add(per_frame[f]);
                    // Resets, consecutiveness, AND short-circuiting and prior firings can
                    // only consume counts. Dropping all of them gives a necessary condition.
                    out[f] = per_frame[f] > 0 && possible_count >= threshold;
                }
            }
            7010 => {
                let ms = missions();
                for (r, rf) in self.ranges.iter().enumerate() {
                    if let (Some(f), true) = (self.start[r], matches(&ms, rf.mission)) {
                        out[f] = true;
                    }
                }
            }
            // Rank is latched into the next frame and may arrive after Complete. Dropping its one-shot latch
            // and packet timing only widens the trigger envelope; never substitute RangeComplete (7013).
            7012 => out.fill(c.condition_values.first().copied().unwrap_or(1) >= 1),
            7013 => out.clone_from(&self.complete),
            7005 => {
                let threshold = *c.condition_values.first()?;
                for f in 0..nf {
                    out[f] = threshold > 0
                        && self.current[f].is_some()
                        && self.combo_triggers.as_ref().and_then(|c| c.possible(threshold, f)).unwrap_or(true);
                }
            }
            7000 => {
                for f in 1..nf {
                    out[f] = self.ranges.iter().enumerate().any(|(r, rf)| {
                        rf.mission == MISSION_LUCK && (RS_START..=RS_END).contains(&self.states[f - 1][r])
                    });
                }
            }
            7020 => {
                // the checker keeps a range until it sees it complete: from the first start of a matching range on
                let ms = missions();
                let first = self.ranges.iter().zip(&self.start).filter(|(r, _)| matches(&ms, r.mission));
                if let Some(f0) = first.filter_map(|(_, s)| *s).min() {
                    out[f0..].fill(true);
                }
            }
            7021 => {
                // the rush flag is set only while a luck range plays and cleared when this checker sees a luck range
                // complete, which a luck (or any) gate always does
                for r in &luck {
                    let Some(f0) = r.f_start else { continue };
                    let f1 = if gate == MISSION_LUCK || gate == MISSION_ALL {
                        r.f_complete.map_or(nf, |x| x + 1)
                    } else {
                        nf
                    };
                    out[f0..f1.min(nf)].fill(true);
                }
            }
            _ => return None,
        }
        Some(out)
    }

    /// The most hits of a trigger group each of whose sets holds a positive judgement count condition (1030, 1040),
    /// `None` otherwise. A counter hits each time it has counted `n` target judgements and then counts from 0, so a
    /// set hits at most the judgements its counter can count over `n`. It counts only the judgements of frames in
    /// which the gate lets the triggers be checked, each at most as often as a reachable final judgement is listed
    /// among its targets; resets, consecutiveness and AND short-circuiting only drop counts.
    pub(super) fn count_trigger_hits(&self, env: &Env, gid: i64, gate: i64) -> Option<i64> {
        let sets = env.sets.get(&gid)?;
        if self.ent.iter().any(|&(_, j)| !(0..8).contains(&j)) {
            return None;
        }
        // A lone set led by a range-playing condition restarts its counters whenever that condition fails.
        let resets = if sets.len() == 1 { self.playing_resets(env, sets[0], gate) } else { None };
        let mut total = 0i64;
        for s in sets {
            let mut set_hits: Option<i64> = None;
            for &cid in s.iter() {
                let c = env.master.skill_condition(cid)?;
                if !c.is_positive || !matches!(c.condition_type, 1030 | 1040) {
                    continue;
                }
                let n = *c.condition_values.first()?;
                if n < 1 {
                    continue;
                }
                let targets: Vec<_> = c
                    .condition_target_ids
                    .iter()
                    .map(|&id| env.master.skill_target(id).map(|t| t.judgement))
                    .collect::<Option<Vec<_>>>()?;
                let mut per_frame = vec![0i64; self.times.len()];
                for (i, &(f, raw)) in self.ent.iter().enumerate() {
                    let reach = env.reach_of(i, raw);
                    let most = (0..8)
                        .filter(|&j| reach & (1u8 << j) != 0)
                        .map(|j| targets.iter().filter(|&&t| t == j).count() as i64)
                        .max()
                        .unwrap_or(0);
                    per_frame[f] = per_frame[f].saturating_add(most);
                }
                // the counts of the frames in which the triggers are checked, split where the counter restarts
                let (mut hits, mut count) = (0i64, 0i64);
                for (f, &c) in per_frame.iter().enumerate() {
                    if resets.as_ref().is_some_and(|r| r[f]) {
                        hits = hits.saturating_add(count / n);
                        count = 0;
                    } else if self.gate_open(gate, f) {
                        count = count.saturating_add(c);
                    }
                }
                hits = hits.saturating_add(count / n);
                set_hits = Some(set_hits.map_or(hits, |h| h.min(hits)));
            }
            total = total.saturating_add(set_hits?);
        }
        Some(total)
    }

    /// The frames in which a set whose first condition is a positive range-playing condition (7020) restarts its
    /// count conditions (`None` for another set): the triggers are checked (the gate is open) and that condition
    /// fails, so the AND resets its count conditions before they read the frame's judgements. The checker holds a
    /// range from a checked frame in which it sees the range start or play until a checked frame in which it sees
    /// the range complete or finish; as the first condition it sees every checked frame. So it fails in a checked
    /// frame when every range of its missions has not started yet or completed or finished in a checked frame by
    /// then.
    fn playing_resets(&self, env: &Env, set: &[i64], gate: i64) -> Option<Vec<bool>> {
        let c = env.master.skill_condition(*set.first()?)?;
        if c.condition_type != 7020 || !c.is_positive {
            return None;
        }
        let ms: Vec<i64> = c
            .condition_target_ids
            .iter()
            .filter_map(|&t| env.master.skill_target(t))
            .filter(|t| t.skill_target_type == 5 && t.gekisou_mission_type != 0)
            .map(|t| t.gekisou_mission_type)
            .collect();
        let any_mission = ms.is_empty() || ms.contains(&MISSION_ALL);
        let nf = self.times.len();
        let first = |r: usize, s: u8| (0..nf).find(|&f| self.states[f].get(r).is_some_and(|&v| v >= s));
        let mut held = Vec::new();
        for (r, rf) in self.ranges.iter().enumerate() {
            if !any_mission && !ms.contains(&rf.mission) {
                continue;
            }
            let Some(start) = first(r, RS_START) else { continue };
            let released = [first(r, RS_COMPLETE), first(r, RS_FINISH)]
                .into_iter()
                .flatten()
                .find(|&f| self.gate_open(gate, f))
                .unwrap_or(usize::MAX);
            held.push((start, released));
        }
        Some((0..nf).map(|f| self.gate_open(gate, f) && held.iter().all(|&(s, r)| f < s || r <= f)).collect())
    }

    /// The frames in which a condition group can hold (`None`: any frame): the union over its sets of the
    /// intersection over their conditions.
    pub(super) fn group_frames(&self, env: &Env, gid: i64, gate: i64) -> Option<Vec<bool>> {
        let sets = env.sets.get(&gid)?;
        let mut any: Option<Vec<bool>> = Some(vec![false; self.times.len()]);
        for s in sets {
            let mut and: Option<Vec<bool>> = None;
            for &cid in s.iter() {
                if let Some(v) = self.cond_frames(env, cid, gate) {
                    and = Some(match and {
                        None => v,
                        Some(a) => a.iter().zip(&v).map(|(x, y)| *x && *y).collect(),
                    });
                }
            }
            match (and, any.as_mut()) {
                (None, _) => return None,
                (Some(a), Some(u)) => {
                    for (x, y) in u.iter_mut().zip(&a) {
                        *x |= *y;
                    }
                }
                (Some(_), None) => {}
            }
        }
        any
    }

    /// The frames in which a Gekisou row can start (its gate open and its trigger possible), and the frames in which
    /// its trigger is possible.
    pub(super) fn starts(&self, env: &Env, r: &Row) -> (Vec<usize>, Vec<bool>) {
        let nf = self.times.len();
        let p = self.group_frames(env, r.trigger, r.gate).unwrap_or_else(|| vec![true; nf]);
        let s = (0..nf).filter(|&f| p[f] && self.gate_open(r.gate, f)).collect();
        (s, p)
    }

    /// The first frame from `f` on in which some range turns Complete.
    pub(super) fn completion_from(&self, f: usize) -> Option<usize> {
        self.next_complete.get(f).copied().flatten()
    }

    /// `end_of` for each start frame `s` (frames where the trigger is possible: `p`).
    pub(super) fn ends(&self, env: &Env, r: &Row, s: &[usize], p: &[bool]) -> Vec<(i64, i64)> {
        let nf = self.times.len();
        // a sustained effect ends in the first open-gate frame after its start whose trigger fails
        let fail: Vec<Option<usize>> = if r.trigger_type == 2 {
            let mut v = vec![None; nf];
            let mut next = None;
            for x in (0..nf).rev() {
                v[x] = next;
                if self.gate_open(r.gate, x) && !p[x] {
                    next = Some(x);
                }
            }
            v
        } else {
            Vec::new()
        };
        s.iter().map(|&f| self.end_of(env, r, f, p, fail.get(f).copied())).collect()
    }

    /// Whether the row's release group is exactly one positive range-complete condition.
    pub(super) fn released_on_complete(env: &Env, r: &Row) -> bool {
        let Some(sets) = env.sets.get(&r.release) else { return false };
        let ids: Vec<i64> = sets
            .iter()
            .flat_map(|s| s.iter().copied())
            .filter(|&c| env.master.skill_condition(c).is_none_or(|x| x.condition_type != 0))
            .collect();
        ids.len() == 1
            && sets.iter().filter(|s| !s.is_empty()).count() == 1
            && env.master.skill_condition(ids[0]).is_some_and(|c| c.condition_type == 7013 && c.is_positive)
    }

    /// For an execution started in frame `f`: the latest time its factor can end (exclusive; `i64::MAX`: never)
    /// and the index of the last frame whose judgements its conversion can see.
    pub(super) fn end_of(&self, env: &Env, r: &Row, f: usize, p: &[bool], fail: Option<Option<usize>>) -> (i64, i64) {
        let nf = self.times.len();
        let at = |x: Option<usize>| x.map_or((i64::MAX, i64::MAX), |i| (self.times[i] as i64, i as i64));
        if r.trigger_type == 2 {
            // a sustained effect ends in the first open-gate frame whose trigger fails
            let x = match fail {
                Some(x) => x,
                None => (f + 1..nf).find(|&x| self.gate_open(r.gate, x) && !p[x]),
            };
            return at(x);
        }
        // Release checkers skip the first elapsed-time check, so the timed end is first tested two frames after the
        // start; no end is bounded when the first post-start update is the final play frame.
        let timed = if r.release == 0 {
            (frame_end(&self.times, self.times[f], f, r.act), register_end(&self.times, f, r.act))
        } else {
            (release_frame_end(&self.times, self.times[f], f, r.act), release_register_end(&self.times, f, r.act))
        };
        if r.release != 0 && Self::released_on_complete(env, r) {
            // the release is asked from the second frame after the start on
            let rel = at(self.completion_from(f + 2));
            return (timed.0.min(rel.0), timed.1.min(rel.1));
        }
        timed
    }
}

/// The Gekisou effects whose commands the controller applies in filing order (`Controller::recalculate`): combo
/// bonus, combo protection and Just bonus.
const FILED_IN_ORDER: [i64; 3] = [12000, 12004, 13000];

/// What the bounds read of a Gekisou row's timing: its factor windows in chart time, each `(start, end, concurrent
/// executions)`, the play-frame index ranges `(a, b]` whose judgements its conversion can see, and the frames in
/// which it can start.
pub(super) struct GkRowWin {
    pub(super) win: Vec<(i64, i64, f64)>,
    /// `win` ending at the time of the frame that files each end, for the effects in `FILED_IN_ORDER`. The
    /// controller applies their commands in filing order and stops at the first one later than a judgement, so an
    /// end filed after a later command another member filed in the same frame holds until that command's time, at
    /// most the frame's time: every command carries a time at most the time of the frame filing it.
    pub(super) filed: Vec<(i64, i64, f64)>,
    /// Lifetime activation count per window, distinct from maximum concurrency.
    pub(super) executions: Vec<f64>,
    pub(super) conv: Vec<(i64, i64)>,
    pub(super) starts: Vec<usize>,
    /// The executions each window of `win` (and `filed`) covers.
    pub(super) win_starts: Vec<Rc<RampStarts>>,
    /// A sustained row (one execution at a time): the union of its start frames' factor spans, each component with
    /// the playing range of all its start frames when they agree (empty for other rows).
    pub(super) parts: Vec<SpanPart>,
}

/// The timing of a Gekisou row (it reads only the trigger, the trigger type, the gate, the activation time and the
/// release), computed once per search. Every frame in which the row can start gives one factor window from the
/// earliest trigger time there to its latest end; with many such frames, one window over all of them with at most
/// five executions (the updaters of one effect). A sustained effect runs one execution at a time inside the span of
/// its start frame: one window per component of the spans' union. Registered in a frame in which it can start, a
/// conversion converts the notes judged in the next frames up to the frame that processes its end.
pub(super) fn gk_row(env: &Env, r: &Row) -> Rc<GkRowWin> {
    let key = (r.trigger, r.trigger_type, r.gate, r.act.to_bits(), r.release);
    if let Some(w) = env.gk_cache.borrow().get(&key) {
        return w.clone();
    }
    let w = Rc::new(gk_row_timing(env, r));
    env.gk_cache.borrow_mut().insert(key, w.clone());
    w
}

pub(super) fn gk_row_timing(env: &Env, r: &Row) -> GkRowWin {
    let Some(g) = &env.gkf else {
        return GkRowWin {
            win: vec![(i64::MIN, i64::MAX, POOL)],
            filed: vec![(i64::MIN, i64::MAX, POOL)],
            executions: vec![f64::INFINITY],
            conv: vec![(i64::MIN, i64::MAX)],
            starts: Vec::new(),
            // without Gekisou frames no combo count is read
            win_starts: vec![Rc::default()],
            parts: Vec::new(),
        };
    };
    let (s, p) = g.starts(env, r);
    let ends = g.ends(env, r, &s, &p);
    // the frame ranges are only asked whether some range contains a frame: keep their union
    let mut all: Vec<(i64, i64)> = s.iter().zip(&ends).map(|(&f, e)| (f as i64, e.1)).collect();
    all.sort_unstable();
    let mut conv: Vec<(i64, i64)> = Vec::with_capacity(all.len());
    for (a, b) in all {
        match conv.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => conv.push((a, b)),
        }
    }
    let framed = frame_timed(env, r.trigger);
    let spans: Vec<(i64, i64)> = s
        .iter()
        .zip(&ends)
        .map(|(&f, e)| {
            if framed {
                return (g.times[f] as i64, e.0);
            }
            let lower = g.combo_triggers.as_ref().and_then(|c| c.trigger_time(env, r.trigger, f));
            (lower.map_or(g.wlo[f], |t| g.wlo[f].max(t)), e.0)
        })
        .collect();
    // the start frames each window covers: one each for a few one-shot starts; for a sustained row (one execution at
    // a time, inside the span of its start frame) those of each component of the spans' union; else all of them
    let groups: Vec<Vec<usize>> = if spans.is_empty() {
        Vec::new()
    } else if r.trigger_type == 1 && spans.len() <= 8 {
        (0..spans.len()).map(|i| vec![i]).collect()
    } else if r.trigger_type == 2 {
        components(&spans)
    } else {
        vec![(0..spans.len()).collect()]
    };
    let mult = if r.trigger_type == 2 || (r.trigger_type == 1 && spans.len() <= 8) {
        1.0
    } else {
        POOL.min(spans.len() as f64)
    };
    let windows = |spans: &[(i64, i64)]| -> Vec<(i64, i64, f64)> {
        groups
            .iter()
            .map(|c| {
                let a = c.iter().map(|&i| spans[i].0).min().unwrap_or(i64::MIN);
                let b = c.iter().map(|&i| spans[i].1).max().unwrap_or(i64::MAX);
                (a, b, mult)
            })
            .collect()
    };
    let win = windows(&spans);
    let win_starts: Vec<Rc<RampStarts>> =
        groups.iter().map(|c| Rc::new(RampStarts::new(c.iter().map(|&i| (spans[i].0, s[i])).collect()))).collect();
    // the frame that processes an end (`end_of`) files it
    let filed_spans: Vec<(i64, i64)> = spans
        .iter()
        .zip(&ends)
        .map(|(&(a, b), e)| {
            let filed = frame_time(&g.times, e.1);
            debug_assert!(filed >= b, "an end is filed no earlier than its time");
            (a, filed)
        })
        .collect();
    let filed = windows(&filed_spans);
    let executions = if r.trigger_type == 1 && spans.len() <= 8 {
        vec![1.0; spans.len()]
    } else {
        groups.iter().map(|c| (c.len() as f64).next_up()).collect()
    };
    // every execution of a sustained row runs inside the span of its start frame, so inside one component of their
    // union, and starts in one of that component's start frames
    let parts = if r.trigger_type == 2 {
        groups
            .iter()
            .zip(&win)
            .map(|(c, &(a, b, _))| {
                let current = g.current[s[c[0]]];
                (a, b, current.filter(|_| c.iter().all(|&i| g.current[s[i]] == current)))
            })
            .collect()
    } else {
        Vec::new()
    };
    GkRowWin { win, filed, executions, conv, starts: s, win_starts, parts }
}

/// The spans (by index) of each component of their union, by start.
fn components(spans: &[(i64, i64)]) -> Vec<Vec<usize>> {
    let mut by: Vec<usize> = (0..spans.len()).collect();
    by.sort_unstable_by_key(|&i| spans[i]);
    let mut out: Vec<(i64, Vec<usize>)> = Vec::new();
    for i in by {
        let (a, b) = spans[i];
        match out.last_mut() {
            Some(l) if a <= l.0 => {
                l.0 = l.0.max(b);
                l.1.push(i);
            }
            _ => out.push((b, vec![i])),
        }
    }
    out.into_iter().map(|x| x.1).collect()
}

/// Whether a trigger group triggers at the time of the frame that checks it. The simulation drops the conditions of
/// type 0, combines the rest of a set with an AND and two or more non-empty sets with an OR; neither combination
/// reports a trigger time of its own, so only a group of one set with one remaining condition can carry an earlier
/// one.
pub(super) fn frame_timed(env: &Env, group: i64) -> bool {
    let Some(sets) = env.sets.get(&group) else { return false };
    let (mut nonempty, mut widest) = (0usize, 0usize);
    for s in sets {
        let mut n = 0usize;
        for &cid in s.iter() {
            match env.master.skill_condition(cid) {
                None => return false,
                Some(c) if c.condition_type == 0 => {}
                Some(_) => n += 1,
            }
        }
        if n > 0 {
            nonempty += 1;
            widest = widest.max(n);
        }
    }
    nonempty >= 2 || widest >= 2
}

/// The 4010 checker has no time override and can fire only on this performer's event frames.
/// Intersect that known trigger with the mission gate; condition success, release and updater
/// capacity remain optimistic. Unlike the generic pool envelope, do not allow five copies
/// to remain active throughout an entire mission range.
pub(super) fn gk_event_windows(env: &Env, r: &Row, position: usize) -> Vec<(i64, i64, f64)> {
    let Some(g) = &env.gkf else {
        return vec![(i64::MIN, i64::MAX, POOL)];
    };
    let mut possible = vec![false; g.times.len()];
    for &(k, time) in env.events {
        if k == position as i32 {
            let f = g.times.partition_point(|&t| t < time);
            if f < possible.len() {
                possible[f] = true;
            }
        }
    }
    let starts: Vec<_> = (0..possible.len()).filter(|&f| possible[f] && g.gate_open(r.gate, f)).collect();
    let ends = g.ends(env, r, &starts, &possible);
    starts.iter().zip(ends).map(|(&f, end)| (g.times[f] as i64, end.0, 1.0)).collect()
}

/// The most judgements a one-shot Gekisou conversion row can convert in the play, when that is below the number of
/// entries it can see (`None` otherwise). One execution converts at most `limit` judgements and then unregisters;
/// the effect executes at most once per frame, only in frames in which it can start, and with an execute limit at
/// most that many times between two frames in which its reset can hold (the reset is asked, with the gate open,
/// before the frame's triggers).
pub(super) fn gk_budget(env: &Env, r: &Row, w: &GkRowWin) -> Option<f64> {
    if !matches!(r.effect_type, 12006 | 13005) || r.trigger_type != 1 || r.limit <= 0 {
        return None;
    }
    let key = (
        (r.trigger, r.trigger_type, r.gate, r.act.to_bits(), r.release),
        (r.effect_type, r.value, r.limit, r.execute_limit, r.reset),
        r.targets.clone(),
    );
    if let Some(&b) = env.budget_cache.borrow().get(&key) {
        return b;
    }
    let b = gk_budget_uncached(env, r, w);
    env.budget_cache.borrow_mut().insert(key, b);
    b
}

pub(super) fn gk_budget_uncached(env: &Env, r: &Row, w: &GkRowWin) -> Option<f64> {
    let g = env.gkf.as_ref()?;
    let mut execs = w.starts.len() as f64;
    if r.execute_limit > 0 {
        let periods = if r.reset == 0 {
            usize::from(!w.starts.is_empty())
        } else {
            match g.group_frames(env, r.reset, r.gate) {
                None => w.starts.len(),
                Some(rs) => {
                    let (mut n, mut period, mut last, mut si) = (0usize, 0usize, None, 0usize);
                    for (f, &can) in rs.iter().enumerate() {
                        if can && g.gate_open(r.gate, f) {
                            period += 1;
                        }
                        while si < w.starts.len() && w.starts[si] == f {
                            if last != Some(period) {
                                n += 1;
                                last = Some(period);
                            }
                            si += 1;
                        }
                    }
                    n
                }
            }
        };
        execs = execs.min(r.execute_limit as f64 * periods as f64);
    }
    if let Some(hits) = g.count_trigger_hits(env, r.trigger, r.gate) {
        execs = execs.min(hits as f64);
    }
    let n = r.limit as f64 * execs;
    let to = convert_to(r.effect_type, r.value);
    let seen = g
        .ent
        .iter()
        .filter(|&&(fi, j)| {
            j != to && r.targets.contains(&(j as i64)) && w.conv.iter().any(|&(a, b)| a < fi as i64 && fi as i64 <= b)
        })
        .count();
    (to != -1 && n < seen as f64).then_some(n)
}

/// The Gekisou combo bonus windows of a class's rows: `(start, end, bonus)` in chart time, each the combo count a
/// judgement adds while it runs (closed intervals: a bonus filed at a time applies to the judgements from that time
/// on and until its end is filed).
pub(super) fn combo_windows(rows: &[ActiveRow]) -> Vec<ComboBonusRow> {
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.effect_type == 12000 && r.can_start && r.value > 0) {
        // a sustained bonus runs once at a time inside the components of its spans; one started in a component
        // whose start frames all play range `ri` needs that range's combo at its threshold
        if let Some((threshold, parts)) = &r.gk_gate {
            for &(a, b, range) in parts {
                out.push((a, b, r.value as f64, range.map(|ri| (*threshold, ri as u32))));
            }
            continue;
        }
        for &(a, b, mult) in r.gk_win.as_deref().unwrap_or(&[(i64::MIN, i64::MAX, POOL)]) {
            out.push((a, b, r.value as f64 * mult, None));
        }
    }
    out
}

/// The latest finish of a one-shot effect started in frame `i0` with a trigger time at most `exec` (see
/// `Geo::end`, without extensions): `i64::MAX` when no frame is late enough to end it.
pub(super) fn frame_end(frames: &[i32], exec: i32, i0: usize, act: f32) -> i64 {
    let n = frames.len();
    let next = if i0 + 1 < n { Some(frames[i0 + 1] as i64) } else { None };
    if act.is_nan() || act <= 0.0 {
        return next.unwrap_or(i64::MAX);
    }
    let dur = act * 1000f32;
    let last = frames[n - 1];
    match next {
        Some(t1) if dur < last.wrapping_sub(exec) as f32 => {
            (exec as i64 + ournotes_sim::num::ceil_to_i32(dur) as i64).max(t1)
        }
        _ => i64::MAX,
    }
}

/// `register_end` for a one-shot effect with a release checker started in frame `i0`: its first post-start update
/// skips the elapsed-time check, so the strict timed-end test first runs in frame `i0 + 2`, and the end is processed
/// in that frame at the latest when the test already holds there. `i64::MAX` when no frame is late enough, or when
/// the activation is not positive (only the release can end it).
pub(super) fn release_register_end(frames: &[i32], i0: usize, act: f32) -> i64 {
    if act.is_nan() || act <= 0.0 || i0 + 2 >= frames.len() {
        return i64::MAX;
    }
    match register_end(frames, i0, act) {
        i64::MAX => i64::MAX,
        end => end.max(i0 as i64 + 2),
    }
}

/// `frame_end` for a one-shot effect with a release checker started in frame `i0` with a trigger time at most
/// `exec`: it ends no later than the frame `release_register_end` reports. A timed finish there files `exec + ceil(d)`;
/// a release in that frame or before files the current frame time.
pub(super) fn release_frame_end(frames: &[i32], exec: i32, i0: usize, act: f32) -> i64 {
    match release_register_end(frames, i0, act) {
        i64::MAX => i64::MAX,
        end => (exec as i64 + ournotes_sim::num::ceil_to_i32(act * 1000f32) as i64).max(frames[end as usize] as i64),
    }
}

/// The time of play frame `frame`, `i64::MAX` past the play (as `end_of` reports an end no frame processes).
pub(super) fn frame_time(frames: &[i32], frame: i64) -> i64 {
    usize::try_from(frame).ok().and_then(|i| frames.get(i)).map_or(i64::MAX, |&t| t as i64)
}

/// The most factor changes of one execution of a cumulative note score up: its value is `min(effect value *
/// count, max)`, so once the count reaches `ceil(max / value)` it stops changing. A judgement count only grows while
/// the effect runs; a combo count grows within a range unless a Miss or a Bad is reachable, and restarts with each
/// range.
pub(super) fn churn_max(env: &Env, r: &Row) -> Option<f64> {
    if r.effect_type != 2001 || r.value <= 0 || r.max_value <= 0 {
        return None;
    }
    let steps = stable_cumulative_churn(r.value, r.max_value)?;
    let c = env.master.cumulative_condition(r.cumulative)?;
    match c.condition_type {
        1000 => Some(steps),
        7001 => {
            let g = env.gk.as_ref()?;
            if g.breaks { None } else { Some(steps * (g.missions.len() as f64 + 1.0)) }
        }
        _ => None,
    }
}

/// A constant cumulative count stops emitting replacements only when its stored
/// quantized factor compares equal to the requested factor. For example, 45/10000f
/// becomes a 449-mill command; its reconstructed factor fails the native
/// `approximately` check, so every later executing frame emits two more commands.
/// Exact binary32 reconstruction is a sufficient certificate for that check.
/// The work cap disables this optional count bound, not the candidate or skill.
pub(super) fn stable_cumulative_churn(step: i64, ceiling: i64) -> Option<f64> {
    if step <= 0 || ceiling <= 0 {
        return None;
    }
    let changes = (ceiling as i128 + step as i128 - 1) / step as i128;
    if changes > 4096 {
        return None;
    }
    for count in 1..=changes {
        let value = (step as i128 * count).min(ceiling as i128);
        if value > i32::MAX as i128 {
            return None;
        }
        let factor = value as f32 / 10000f32;
        let stored = note_factor_mill(factor) as f32 / 100000f32;
        if factor.to_bits() != stored.to_bits() {
            return None;
        }
    }
    Some(changes as f64 + 1.0)
}

#[cfg(test)]
mod filed_end_tests {
    use super::*;

    /// The case of `tests/gekisou.rs` (`a_combo_bonus_end_waits_behind_a_later_command_of_the_same_frame`): 16 ms
    /// frames, a one-shot combo bonus triggered at 2000 for 1 s. Its end command carries 3000 and is filed at 3008,
    /// where a bonus another member ends at 3005 and files first holds it for a judgement at 3002.
    #[test]
    fn a_timed_end_is_filed_in_the_first_frame_past_its_duration() {
        let frames: Vec<i32> = (0..400).map(|k| 16 * k).collect();
        let i0 = frames.iter().position(|&t| t == 2000).unwrap();
        assert_eq!(frame_end(&frames, 2000, i0, 1.0), 3000);
        assert_eq!(frame_time(&frames, register_end(&frames, i0, 1.0)), 3008);
        // an executing effect whose duration ends on a frame time ends in the next frame
        assert_eq!(frame_time(&frames, register_end(&frames, i0, 0.032)), 2048);
        // no frame past the duration: never filed
        assert_eq!(frame_time(&frames, register_end(&frames, i0, 10.0)), i64::MAX);
        assert_eq!(frame_end(&frames, 2000, i0, 10.0), i64::MAX);
    }

    /// A release checker skips the elapsed-time check in the frame after the start: a duration that has already
    /// passed there is first tested two frames after the start, and a release in that frame files its time.
    #[test]
    fn a_release_timer_ends_from_the_second_frame_after_its_start() {
        let frames: Vec<i32> = (0..400).map(|k| 16 * k).collect();
        let i0 = frames.iter().position(|&t| t == 2000).unwrap();
        assert_eq!(frame_end(&frames, 2000, i0, 0.004), 2016);
        assert_eq!(release_frame_end(&frames, 2000, i0, 0.004), 2032);
        assert_eq!(frame_time(&frames, release_register_end(&frames, i0, 0.004)), 2032);
        assert_eq!(release_frame_end(&frames, 2000, i0, 1.0), 3008);
        assert_eq!(frame_time(&frames, release_register_end(&frames, i0, 1.0)), 3008);
        let n = frames.len();
        assert_eq!(release_frame_end(&frames, frames[n - 3], n - 3, 0.004), frames[n - 1] as i64);
        for i in [n - 2, n - 1] {
            assert_eq!(release_frame_end(&frames, frames[i], i, 0.004), i64::MAX);
            assert_eq!(release_register_end(&frames, i, 0.004), i64::MAX);
        }
        for act in [0.0, -1.0, f32::NAN, 10.0, f32::INFINITY] {
            assert_eq!(release_frame_end(&frames, 2000, i0, act), i64::MAX);
            assert_eq!(release_register_end(&frames, i0, act), i64::MAX);
        }
    }

    #[test]
    fn sustained_spans_split_where_their_union_has_a_gap() {
        let spans = [(30, 40), (0, 10), (25, 26), (5, 20), (40, 41)];
        assert_eq!(components(&spans), [vec![1, 3], vec![2], vec![0, 4]]);
        assert!(components(&[]).is_empty());
    }
}

#[cfg(test)]
mod cumulative_churn_tests {
    use super::*;

    #[test]
    fn unstable_quantization_cannot_use_a_count_only_command_bound() {
        let requested = 45f32 / 10000f32;
        let mill = note_factor_mill(requested);
        let stored = mill as f32 / 100000f32;
        assert_eq!(mill, 449);
        // Even after the count has reached its ceiling, apply_gekisou will
        // replace this same factor again on every EXECUTING frame.
        assert!((stored - requested).abs() > 1e-6f32 * stored.abs().max(requested.abs()));
        assert_eq!(stable_cumulative_churn(45, 45), None);
        assert_eq!(stable_cumulative_churn(100, 3000), Some(31.0));
    }

    #[test]
    fn stability_includes_the_clamped_last_value_and_has_bounded_work() {
        assert_eq!(stable_cumulative_churn(100, 45), None);
        assert_eq!(stable_cumulative_churn(100, 750), Some(9.0));
        assert_eq!(stable_cumulative_churn(1, i64::MAX), None);
    }
}

#[cfg(test)]
mod count_hit_tests {
    use super::*;

    fn ordinary_factor(trigger: i64) -> Row {
        Row {
            identity: RowIdentity { source: RowSource::Support, index: 0, id: 1 },
            trigger_type: 1,
            trigger,
            condition: 0,
            release: 0,
            reset: 0,
            cumulative: 0,
            effect_type: 2000,
            value: 5000,
            act: 1.0,
            limit: 0,
            execute_limit: 0,
            targets: Vec::new(),
            max_value: 0,
            gk: false,
            gate: MISSION_ALL,
        }
    }

    fn count_frames() -> GkFrames {
        let frames: Vec<_> = (0..1000).map(|f| f * 40).collect();
        let entries: Vec<_> = (0..12)
            .map(|i| {
                let note = LiveNote { note_id: i, time_ms: i * 1000, note_operate_type: 1, judgement_type: 1 };
                (i as usize * 25, note, if i % 2 == 0 { 5 } else { 4 })
            })
            .collect();
        GkFrames::new(&Schedule { states: vec![Vec::new(); frames.len()], ranges: Vec::new() }, &frames, &entries)
    }

    fn master() -> Master {
        Master::from_json_tables(|name| match name {
            "MasterSkillTarget" => Some(
                r#"{"_allData":[{"_id":41,"_skillTargetType":4,"_judgement":5},
                {"_id":42,"_skillTargetType":4,"_judgement":4},
                {"_id":43,"_skillTargetType":4,"_judgement":-1},
                {"_id":57,"_skillTargetType":5,"_gekisouMissionType":1}]}"#,
            ),
            "MasterSkillCondition" => Some(
                r#"{"_allData":[
                {"_id":1,"_conditionType":1030,"_conditionValues":[2],"_isPositive":true,"_conditionTargetIDs":[41]},
                {"_id":2,"_conditionType":1030,"_conditionValues":[3],"_isPositive":true,"_conditionTargetIDs":[41,41]},
                {"_id":3,"_conditionType":5000,"_isPositive":true},
                {"_id":4,"_conditionType":1030,"_conditionValues":[2],"_isPositive":false,"_conditionTargetIDs":[41]},
                {"_id":5,"_conditionType":7020,"_isPositive":true,"_conditionTargetIDs":[57]},
                {"_id":6,"_conditionType":0,"_isPositive":true},
                {"_id":7,"_conditionType":1030,"_conditionValues":[3],"_isPositive":true,"_conditionTargetIDs":[41,41]},
                {"_id":8,"_conditionType":1030,"_conditionValues":[3],"_isPositive":true,"_conditionTargetIDs":[41,42]},
                {"_id":9,"_conditionType":1030,"_conditionValues":[3],"_isPositive":true,"_conditionTargetIDs":[41,41,42,42]},
                {"_id":10,"_conditionType":1040,"_conditionValues":[2],"_isPositive":true,"_conditionTargetIDs":[41]},
                {"_id":11,"_conditionType":1030,"_conditionValues":[0],"_isPositive":true,"_conditionTargetIDs":[41]},
                {"_id":12,"_conditionType":1030,"_conditionValues":[2147483648],"_isPositive":true,"_conditionTargetIDs":[41]},
                {"_id":13,"_conditionType":1030,"_conditionValues":[2147483647],"_isPositive":true,"_conditionTargetIDs":[41]},
                {"_id":14,"_conditionType":1030,"_conditionValues":[1],"_isPositive":true,"_conditionTargetIDs":[41]},
                {"_id":15,"_conditionType":1030,"_conditionValues":[3],"_isPositive":true,"_conditionTargetIDs":[41,43,42]}]}"#,
            ),
            "MasterSkillConditionSet" => Some(
                r#"{"_allData":[
                {"_id":1,"_group":1,"_conditionIds":[1,3]},
                {"_id":2,"_group":2,"_conditionIds":[1]},
                {"_id":3,"_group":2,"_conditionIds":[2]},
                {"_id":4,"_group":3,"_conditionIds":[1,2]},
                {"_id":5,"_group":4,"_conditionIds":[3]},
                {"_id":6,"_group":5,"_conditionIds":[4]},
                {"_id":7,"_group":6,"_conditionIds":[5,1]},
                {"_id":8,"_group":7,"_conditionIds":[1,5]},
                {"_id":9,"_group":8,"_conditionIds":[6,1]},
                {"_id":10,"_group":8,"_conditionIds":[6]},
                {"_id":11,"_group":10,"_conditionIds":[7]},
                {"_id":12,"_group":11,"_conditionIds":[8]},
                {"_id":13,"_group":12,"_conditionIds":[9]},
                {"_id":14,"_group":13,"_conditionIds":[10]},
                {"_id":15,"_group":14,"_conditionIds":[11]},
                {"_id":16,"_group":15,"_conditionIds":[12]},
                {"_id":17,"_group":16,"_conditionIds":[13]},
                {"_id":18,"_group":17,"_conditionIds":[14]},
                {"_id":19,"_group":18,"_conditionIds":[15]}]}"#,
            ),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn hits_are_the_countable_target_judgements_over_the_threshold() {
        let master = master();
        let frames = [0, 40, 80, 120, 160, 200];
        let schedule = Schedule {
            states: vec![
                vec![RS_START],
                vec![RS_PLAYING],
                vec![RS_PLAYING],
                vec![RS_PLAYING],
                vec![RS_PLAYING],
                vec![RS_PLAYING],
            ],
            ranges: vec![RangeFacts {
                start: 0,
                end: 300,
                mission: MISSION_COMBO,
                pct: 0,
                f_start: Some(0),
                f_complete: None,
                f_finish: None,
            }],
        };
        let note = |id: i32, t: i32| LiveNote { note_id: id, time_ms: t, note_operate_type: 1, judgement_type: 1 };
        // five Perfects and one Great
        let entries = [
            (1, note(0, 41), 5),
            (2, note(1, 81), 5),
            (2, note(2, 82), 4),
            (3, note(3, 121), 5),
            (4, note(4, 161), 5),
            (5, note(5, 201), 5),
        ];
        let g = GkFrames::new(&schedule, &frames, &entries);
        let mut env = env(&master);
        // one counter of two Perfects beside a fixed condition
        assert_eq!(g.count_trigger_hits(&env, 1, MISSION_ALL), Some(2));
        // the sets of a group add; a target listed twice counts a judgement twice
        assert_eq!(g.count_trigger_hits(&env, 2, MISSION_ALL), Some(2 + 10 / 3));
        // the counters of one set bound it each
        assert_eq!(g.count_trigger_hits(&env, 3, MISSION_ALL), Some(2));
        // without a counter in every set, or with a negated one, there is no bound
        assert_eq!(g.count_trigger_hits(&env, 4, MISSION_ALL), None);
        assert_eq!(g.count_trigger_hits(&env, 5, MISSION_ALL), None);
        assert_eq!(g.count_trigger_hits(&env, 9, MISSION_ALL), None);
        // a closed gate counts nothing
        assert_eq!(g.count_trigger_hits(&env, 1, 2), Some(0));
        // a judgement that can be converted to a target counts
        env.count_reach[4] |= 1 << 5;
        assert_eq!(g.count_trigger_hits(&env, 1, MISSION_ALL), Some(3));
    }

    #[test]
    fn ordinary_fixed_factor_starts_keep_counter_alternatives_and_conversion_reach() {
        let master = master();
        let mut env = env(&master);
        env.gkf = Some(Rc::new(count_frames()));
        assert_eq!(factor_start_limit(&env, &ordinary_factor(1)), Some(3.0f64.next_up()));
        // OR alternatives add their hit budgets, retaining duplicate targets in the second counter.
        assert_eq!(factor_start_limit(&env, &ordinary_factor(2)), Some(7.0f64.next_up()));
        assert_eq!(factor_start_limit(&env, &ordinary_factor(3)), Some(3.0f64.next_up()));
        for trigger in [4, 5, 9] {
            assert_eq!(factor_start_limit(&env, &ordinary_factor(trigger)), None);
        }
        env.entry_reach = vec![(1 << 4) | (1 << 5); 12];
        assert_eq!(factor_start_limit(&env, &ordinary_factor(1)), Some(6.0f64.next_up()));
        assert_eq!(factor_start_limit(&env, &ordinary_factor(2)), Some(14.0f64.next_up()));
        // The final judgement cannot become a counted target when no admitted conversion reaches it.
        env.entry_reach = vec![1 << 4; 12];
        assert!(factor_start_limit(&env, &ordinary_factor(1)).unwrap() < 1.0);
    }

    #[test]
    fn ordinary_counter_caps_tighten_commands_and_lifetime_factor_amplitudes() {
        use ournotes_sim::live::score::ScoreFactorState;
        use ournotes_sim::live::skill::{FactorCommand, apply_factor};

        let master = master();
        let mut env = env(&master);
        env.gkf = Some(Rc::new(count_frames()));
        let row = ordinary_factor(1);
        let active = active_row(&env, &row, true, false).unwrap();
        assert_eq!(active.start_limit, Some(3.0f64.next_up()));
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 1000,
            music_level: 1,
            converted_note_count: 1,
            music_length_ms: 40_000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let frames = &env.gkf.as_ref().unwrap().times;
        let play = LivePlay {
            frames: frames
                .iter()
                .map(|&time_ms| ournotes_sim::live::full::PlayFrame { time_ms, judged: Vec::new() })
                .collect(),
            base_seed: 0,
        };
        let setup = FullSetup { notes: Vec::new(), events: Vec::new(), play, params, gk: None };
        let exec = Exec::new(&setup, &[40], None, &[]);
        let geo = Geo { frames, times: &[40], exec: &exec, music_length_ms: 40_000, snapshot_frame_limit: None };
        let mut uncapped = active.clone();
        uncapped.start_limit = None;
        let (w, commands, norm, executions, spans, _, _, _, _) = windows(&geo, 0, &[], &[active], &[]);
        let (old_w, old_commands, old_norm, old_executions, old_spans, _, _, _, _) =
            windows(&geo, 0, &[], &[uncapped], &[]);
        assert!((6.0..7.0).contains(&commands));
        assert!(old_commands >= 2000.0);
        assert!(executions < old_executions);
        // Six Perfect judgements and a threshold of two allow at most three starts. Conditions can
        // suppress those starts, but cannot add more. The generic historical-factor envelope retains
        // every lifetime start, so the same certificate tightens its amplitudes as well as its commands.
        let perfects = env.gkf.as_ref().unwrap().ent.iter().filter(|&&(_, judgement)| judgement == 5).count();
        assert_eq!(perfects, 6);
        let starts = perfects / 2;
        assert_eq!(starts, 3);
        assert!(norm.iter().all(|n| (1.5..1.501).contains(n)));
        assert!(old_norm.iter().all(|n| (500.0..500.001).contains(n)));
        assert_eq!(spans.len(), old_spans.len());
        for (a, b) in spans.iter().zip(&old_spans) {
            assert_eq!((a.0, a.1), (b.0, b.1));
            assert!((1.5..1.501).contains(&a.2));
            assert!((500.0..500.001).contains(&b.2));
        }
        assert_eq!(w.len(), old_w.len());
        for (a, b) in w.iter().zip(&old_w) {
            assert_eq!((a.lo, a.hi, a.judge), (b.lo, b.hi, b.judge));
            assert!((1.5..1.501).contains(&a.note));
            assert!((500.0..500.001).contains(&b.note));
        }
        assert_eq!(w.len(), 1);
        // Independently exercise native factor arithmetic on the relaxed worst case: all three
        // historical starts precede their end commands. This must fit both the window and drift norm.
        let mut native = ScoreFactorState::new(1000);
        for sign in [1, -1] {
            for _ in 0..starts {
                apply_factor(&mut native, &FactorCommand { note_mill: sign * 50_000, ..Default::default() });
                let added = f64::from(native.note_score_up - 1.0);
                assert!(added <= w[0].note && norm.iter().all(|&bound| added <= bound));
            }
            assert_eq!(native.note_score_up, if sign == 1 { 2.5 } else { 1.0 });
        }
        assert!((2 * starts) as f64 <= commands);
        let mut row = row;
        row.effect_type = 2004;
        row.targets = vec![5, 5, 6];
        let active = active_row(&env, &row, true, false).unwrap();
        let (w, commands, norm, _, _, _, _, _, _) = windows(&geo, 0, &[], &[active], &[]);
        assert!((18.0..19.0).contains(&commands));
        assert_eq!(w.len(), 1);
        let mut native = ScoreFactorState::new(1000);
        for _ in 0..starts {
            for judgement in [5, 5, 6] {
                apply_factor(&mut native, &FactorCommand { judgement, judge_mill: 50_000, ..Default::default() });
            }
        }
        assert_eq!((native.good, native.great, native.perfect, native.just), (0.0, 0.0, 3.0, 1.5));
        for (j, factor) in [native.good, native.great, native.perfect, native.just].into_iter().enumerate() {
            assert!(f64::from(factor) <= w[0].judge[j] && f64::from(factor) <= norm[j]);
        }
        assert!((2 * starts * 3) as f64 <= commands);
    }

    #[test]
    fn ordinary_counter_caps_require_one_shot_fixed_factors_and_complete_frame_inputs() {
        let master = master();
        let mut env = env(&master);
        let mut row = ordinary_factor(1);
        assert_eq!(factor_start_limit(&env, &row), None);
        env.gkf = Some(Rc::new(count_frames()));
        for effect_type in [2001, 3001, 12006, 15000] {
            row.effect_type = effect_type;
            assert_eq!(factor_start_limit(&env, &row), None);
        }
        row.effect_type = 2004;
        row.targets = vec![6, 6, 5];
        row.release = 4;
        row.reset = 5;
        row.execute_limit = 1;
        assert_eq!(factor_start_limit(&env, &row), Some(3.0f64.next_up()));
        row.trigger_type = 2;
        assert_eq!(factor_start_limit(&env, &row), None);
        row.trigger_type = 1;
        row.gk = true;
        assert_eq!(factor_start_limit(&env, &row), None);
    }

    #[test]
    fn ordinary_count_windows_keep_lone_checker_semantics_and_all_reachable_grades() {
        let master = master();
        let mut env = env(&master);
        env.gkf = Some(Rc::new(count_frames()));
        // Type-0 checkers and the sets they empty do not wrap the native lone counter.
        assert_eq!(
            factor_count_windows(&env, &ordinary_factor(8)).unwrap(),
            [(1920, 3000, 1.0), (5920, 7000, 1.0), (9920, 11000, 1.0)]
        );
        let starts = |windows: Vec<(i64, i64, f64)>| windows.into_iter().map(|w| w.0).collect::<Vec<_>>();
        // Two target occurrences preserve the residue across entries; they do not mean two updater starts.
        assert_eq!(starts(factor_count_windows(&env, &ordinary_factor(10)).unwrap()), [1920, 3920, 7920, 9920]);
        let both = factor_count_windows(&env, &ordinary_factor(11)).unwrap();
        assert_eq!(starts(both.clone()), [1920, 4920, 7920, 10920]);
        env.entry_reach = vec![(1 << 4) | (1 << 5); 12];
        assert_eq!(factor_count_windows(&env, &ordinary_factor(11)), Some(both.clone()));
        assert_eq!(factor_count_windows(&env, &ordinary_factor(18)), Some(both));
        assert_eq!(
            starts(factor_count_windows(&env, &ordinary_factor(12)).unwrap()),
            [920, 1920, 3920, 4920, 6920, 7920, 9920, 10920]
        );
        // A legal conversion that changes target multiplicity can move the hit frames, so the refinement declines.
        assert_eq!(factor_count_windows(&env, &ordinary_factor(8)), None);
        env.entry_reach.clear();
        env.count_reach[4] |= 1 << 5;
        assert_eq!(factor_count_windows(&env, &ordinary_factor(8)), None);
        env.entry_reach = vec![0; 12];
        assert_eq!(factor_count_windows(&env, &ordinary_factor(11)), None);
    }

    #[test]
    fn ordinary_count_windows_decline_composites_consecutive_counters_and_incomplete_geometry() {
        let master = master();
        let mut env = env(&master);
        let mut row = ordinary_factor(8);
        assert_eq!(factor_count_windows(&env, &row), None);
        env.gkf = Some(Rc::new(count_frames()));
        for trigger in [0, 1, 2, 3, 4, 5, 9, 13, 14, 15] {
            assert_eq!(factor_count_windows(&env, &ordinary_factor(trigger)), None, "trigger {trigger}");
        }
        assert_eq!(factor_count_windows(&env, &ordinary_factor(16)), Some(Vec::new()));
        row.condition = 4;
        row.reset = 2;
        row.execute_limit = 1;
        assert!(factor_count_windows(&env, &row).is_some());
        row.release = 4;
        assert_eq!(factor_count_windows(&env, &row), None);
        row.release = 0;
        row.trigger_type = 2;
        assert_eq!(factor_count_windows(&env, &row), None);
        row.trigger_type = 1;
        row.gk = true;
        assert_eq!(factor_count_windows(&env, &row), None);
        row.gk = false;
        for effect_type in [2001, 3001, 12006, 15000] {
            row.effect_type = effect_type;
            assert_eq!(factor_count_windows(&env, &row), None);
        }
        row.effect_type = 2004;
        assert!(factor_count_windows(&env, &row).is_some());
        for bad in 0..5 {
            let mut g = count_frames();
            match bad {
                0 => {
                    g.times[0] = -1;
                }
                1 => {
                    g.times[5] = 0;
                }
                2 => {
                    g.wlo.pop();
                }
                3 => {
                    g.ent[0].0 = g.times.len();
                }
                _ => {
                    g.ent[1].1 = 8;
                }
            }
            env.gkf = Some(Rc::new(g));
            assert_eq!(factor_count_windows(&env, &row), None);
        }
    }

    #[test]
    fn ordinary_count_windows_keep_all_lifetime_starts_and_bound_chart_time_overlap() {
        let master = master();
        let mut env = env(&master);
        let frames: Vec<i32> = (0..220).map(|f| f * 40).collect();
        let entries: Vec<_> = (0..40)
            .map(|i| {
                (i as usize * 5, LiveNote { note_id: i, time_ms: i * 200, note_operate_type: 1, judgement_type: 1 }, 5)
            })
            .collect();
        let schedule = Schedule { states: vec![Vec::new(); frames.len()], ranges: Vec::new() };
        env.gkf = Some(Rc::new(GkFrames::new(&schedule, &frames, &entries)));
        let mut row = ordinary_factor(17);
        row.act = 0.3;
        let active = active_row(&env, &row, true, false).unwrap();
        assert_eq!(active.count_win.as_ref().unwrap().len(), 40);
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 1000,
            music_level: 1,
            converted_note_count: 40,
            music_length_ms: 10_000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let play = LivePlay {
            frames: frames
                .iter()
                .map(|&time_ms| ournotes_sim::live::full::PlayFrame { time_ms, judged: Vec::new() })
                .collect(),
            base_seed: 0,
        };
        let setup =
            FullSetup { notes: entries.iter().map(|e| e.1).collect(), events: Vec::new(), play, params, gk: None };
        let times: Vec<_> = entries.iter().map(|e| e.1.time_ms).collect();
        let exec = Exec::new(&setup, &times, None, &[]);
        let mut geo =
            Geo { frames: &frames, times: &times, exec: &exec, music_length_ms: 10_000, snapshot_frame_limit: None };
        let (ws, commands, norm, _, spans, _, _, _, _) = windows(&geo, 0, &[], std::slice::from_ref(&active), &[]);
        assert_eq!(spans.len(), 40);
        assert!((80.0..81.0).contains(&commands));
        // The closed native frame of a 300 ms end also contains the next 320 ms start: three
        // conservative windows meet there, although at most two contain an actual entry below.
        assert!(norm.iter().all(|n| (1.5..1.501).contains(n)));
        for i in 0..times.len() as u32 {
            let factor: f64 = ws.iter().filter(|w| w.lo <= i && i < w.hi).map(|w| w.note).sum();
            assert!(factor <= 1.0);
        }
        // A possible finish clamp before a late start retains the lifetime norm.
        geo.music_length_ms = 4000;
        let (_, _, norm, _, _, _, _, _, _) = windows(&geo, 0, &[], &[active], &[]);
        assert!(norm.iter().all(|n| (20.0..20.001).contains(n)));

        // Repeated late judgements can backdate more chart-time factors than the five physical updaters.
        let entries: Vec<_> = entries
            .into_iter()
            .map(|(f, mut note, j)| {
                note.time_ms = 0;
                (f, note, j)
            })
            .collect();
        env.gkf = Some(Rc::new(GkFrames::new(&schedule, &frames, &entries)));
        let active = active_row(&env, &row, true, false).unwrap();
        geo.music_length_ms = 10_000;
        let (_, _, norm, _, _, _, _, _, _) = windows(&geo, 0, &[], &[active], &[]);
        assert!(norm.iter().all(|n| (20.0..20.001).contains(n)));
    }

    #[test]
    fn ordinary_count_windows_merge_same_frame_hits_and_keep_backdating() {
        let master = master();
        let mut env = env(&master);
        let frames = [0, 40, 40, 80, 120, 2000];
        let note = |id, time_ms| LiveNote { note_id: id, time_ms, note_operate_type: 1, judgement_type: 1 };
        let entries =
            [(1, note(7, 1), 5), (1, note(-1, 0), 5), (1, note(7, 1), 5), (2, note(-2, 0), 5), (3, note(7, 1), 5)];
        env.gkf = Some(Rc::new(GkFrames::new(
            &Schedule { states: vec![Vec::new(); frames.len()], ranges: Vec::new() },
            &frames,
            &entries,
        )));
        let mut row = ordinary_factor(17);
        row.act = 0.0;
        assert_eq!(factor_count_windows(&env, &row).unwrap(), [(i64::MIN, 40, 1.0), (0, 80, 1.0), (1, 120, 1.0),]);
        env.entry_reach = vec![1 << 4; entries.len()];
        let active = active_row(&env, &row, true, false).unwrap();
        assert_eq!(active.count_win, Some(Vec::new()));
    }

    #[test]
    fn only_a_lone_condition_can_trigger_before_its_frame() {
        let master = master();
        let env = env(&master);
        // an AND of a counter and a fixed condition, and an OR of two sets, trigger at the frame time
        assert!(frame_timed(&env, 1));
        assert!(frame_timed(&env, 2));
        // one condition, also once the conditions of type 0 and the sets they empty are dropped, keeps its own time
        assert!(!frame_timed(&env, 4));
        assert!(!frame_timed(&env, 8));
        assert!(!frame_timed(&env, 10));
    }

    fn env(master: &Master) -> Env<'_> {
        let mut env = Env {
            master,
            events: &[],
            sets: HashMap::new(),
            life_lo: 0,
            life_hi: 1000,
            life_rigid: false,
            raw: vec![4, 5],
            count_reach: std::array::from_fn(|j| 1u8 << j),
            entry_reach: Vec::new(),
            gk: None,
            gkf: None,
            rush_cache: RefCell::new(HashMap::new()),
            gk_cache: RefCell::new(HashMap::new()),
            budget_cache: RefCell::new(HashMap::new()),
            ramp_cache: RefCell::new(HashMap::new()),
        };
        for s in &master.skill_condition_sets {
            env.sets.entry(s.group).or_default().push(&s.condition_ids);
        }
        env
    }

    #[test]
    fn a_leading_range_playing_condition_restarts_the_count_between_ranges() {
        let master = master();
        let frames = [0, 40, 80, 120, 160, 200];
        // range 0 starts in frame 0 and completes in frame 2; range 1 starts in frame 4
        let schedule = Schedule {
            states: vec![
                vec![RS_START, 0],
                vec![RS_PLAYING, 0],
                vec![RS_COMPLETE, 0],
                vec![RS_FINISH, 0],
                vec![RS_FINISH, RS_START],
                vec![RS_FINISH, RS_PLAYING],
            ],
            ranges: [(0, 90, 0, 2, 3), (150, 300, 4, 6, 7)]
                .map(|(start, end, s, c, f)| RangeFacts {
                    start,
                    end,
                    mission: MISSION_COMBO,
                    pct: 0,
                    f_start: Some(s),
                    f_complete: (c < 6).then_some(c),
                    f_finish: (f < 6).then_some(f),
                })
                .into(),
        };
        let note = |id: i32, t: i32| LiveNote { note_id: id, time_ms: t, note_operate_type: 1, judgement_type: 1 };
        // a Perfect judged in each of frames 1 to 5
        let entries: Vec<_> = (1..6).map(|f| (f, note(f as i32, 40 * f as i32 - 1), 5)).collect();
        let g = GkFrames::new(&schedule, &frames, &entries);
        let env = env(&master);
        // frames 1 to 5 are checked: five Perfects make two hits of two
        assert_eq!(g.count_trigger_hits(&env, 1, MISSION_COMBO), Some(2));
        // led by the range condition: one Perfect in range 0, the frames 2 and 3 restart, two Perfects in range 1
        assert_eq!(g.count_trigger_hits(&env, 6, MISSION_COMBO), Some(1));
        // the range condition after the counter does not restart it in the frames the counter fails
        assert_eq!(g.count_trigger_hits(&env, 7, MISSION_COMBO), Some(2));
    }
}

#[cfg(test)]
#[path = "sustained_start_tests.rs"]
mod sustained_start_tests;
