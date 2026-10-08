//! Whole-domain monotone epochs of the controller count read by cumulative 7001.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct ComboEpochs {
    /// All current-range changes and all possibly breaking entries, plus the initial epoch.
    pub(super) epochs: usize,
    /// Nonnegative native combo before its i32 addition can wrap.
    pub(super) max_combo: i32,
}

impl ComboEpochs {
    pub(super) fn churn(&self, env: &Env, row: &Row, stable_steps: f64) -> Option<f64> {
        let c = env.master.cumulative_condition(row.cumulative)?;
        let n = *c.condition_values.first()?;
        if c.condition_type != 7001 || n <= 0 || row.value <= 0 {
            return None;
        }
        // The native division is monotone on nonnegative integers, including its binary32
        // rounding. Check its largest product before the applier's cast, which precedes min(cap).
        let count = i64::from(ournotes_sim::num::floor_to_i32(self.max_combo as f32 / n as f32));
        let count = count.min(if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count });
        if count < 0 || i128::from(row.value) * i128::from(count) > i128::from(i32::MAX) {
            return None;
        }
        if !stable_steps.is_finite() || !(1.0..=4097.0).contains(&stable_steps) || stable_steps.fract() != 0.0 {
            return None;
        }
        let changes = u64::try_from(self.epochs).ok()?.checked_mul(stable_steps as u64)?;
        (changes <= 1 << 53).then_some((changes as f64).next_up())
    }
}

/// Compile after the complete per-entry conversion closure. A refusal only disables this
/// optional work bound. It neither changes the candidate domain nor substitutes a raw judgement
/// for a reachable converted one.
pub(super) fn compile<'r>(
    env: &Env,
    setup: &FullSetup,
    entries: &[(usize, LiveNote, i32)],
    rows: impl Iterator<Item = &'r Row>,
    max_bonus: i64,
) -> Option<ComboEpochs> {
    let g = env.gkf.as_deref()?;
    setup.gk.as_ref()?;
    let frames = &setup.play.frames;
    if !(0..1 << 24).contains(&max_bonus)
        || g.times.len() != frames.len()
        || g.current.len() != frames.len()
        || g.ent.len() != entries.len()
        || env.entry_reach.len() != entries.len()
        || frames.iter().zip(&g.times).any(|(frame, &time)| frame.time_ms != time || !(0..=1 << 24).contains(&time))
        || g.times.windows(2).any(|times| times[0] >= times[1])
        || g.current.iter().flatten().any(|&r| r >= g.ranges.len())
        || g.ranges.iter().any(|r| r.start < 0 || r.start > r.end || r.end > 1 << 24)
    {
        return None;
    }
    let mut counts = vec![0usize; g.ranges.len()];
    let mut last = vec![None; g.ranges.len()];
    let mut newest = vec![None; frames.len()];
    let mut breaks = 0usize;
    let mut previous_frame = 0;
    for (i, &(frame, note, raw)) in entries.iter().enumerate() {
        let mask = env.entry_reach[i];
        if frame >= frames.len()
            || frame < previous_frame
            || g.ent[i] != (frame, raw)
            || !(0..8).contains(&raw)
            || mask & (1 << raw) == 0
            || note.time_ms < 0
            || note.time_ms > frames[frame].time_ms
        {
            return None;
        }
        previous_frame = frame;
        let mut relevant = false;
        for (r, range) in g.ranges.iter().enumerate() {
            if range.start <= note.time_ms && note.time_ms <= range.end {
                // frame_timed converts with judgement_time_ms, but gekisou_after passes the
                // original LiveNote time into controller history. History stays in append order.
                if last[r].is_some_and(|time| time > note.time_ms) {
                    return None;
                }
                last[r] = Some(note.time_ms);
                counts[r] = counts[r].checked_add(1)?;
                relevant = true;
            }
        }
        if relevant {
            newest[frame] = Some(newest[frame].map_or(note.time_ms, |time: i32| time.max(note.time_ms)));
            if mask & 0b0000_0110 != 0 {
                breaks = breaks.checked_add(1)?;
            }
        }
    }
    let max_combo = i64::try_from(counts.into_iter().max().unwrap_or(0)).ok()?.checked_mul(max_bonus + 1)?;
    let max_combo = i32::try_from(max_combo).ok()?;
    let mut previous = None;
    let old_history: Vec<Option<i32>> = newest
        .into_iter()
        .map(|time| {
            let old = previous;
            if let Some(time) = time {
                previous = Some(previous.map_or(time, |old: i32| old.max(time)));
            }
            old
        })
        .collect();
    // EXECUTING finishes clamp to music length, even when the originating start is later.
    // No such inverse may change a counted history entry, including at the same millisecond.
    if setup.params.music_length_ms > 0 && previous.is_some_and(|time| time >= setup.params.music_length_ms) {
        return None;
    }
    let mut handle_rows = 0usize;
    for row in rows {
        if matches!(row.effect_type, 12000 | 12004 | 13000 | 13002) {
            handle_rows = handle_rows.checked_add(1)?;
        }
        match row.effect_type {
            12000 | 12004 => inverse_is_forward(env, setup, row, &old_history)?,
            // These effects cannot write controller combo or protection. Converters have already
            // contributed their full transitive reach, and Just writers only change the Just count.
            2000 | 2001 | 2004 | 3001 | 3003 | 3004 | 4004 | 11000 | 11001 | 11002 | 11003 | 11004 | 11005 | 12006
            | 13000 | 13002 | 13003 | 13004 | 13005 | 15000 => {}
            // In particular, fixed combo additions 12002/12003 need a different integer/history proof.
            _ => return None,
        }
    }
    // The four handle-allocating controller appliers each have at most five instances
    // per effect and performer. Relax every domain row onto every performer. This
    // prevents handle-ID reuse from changing protection/bonus ownership after wrapping.
    let handles = frames.len().checked_mul(handle_rows)?.checked_mul(5 * 5)?;
    if handles > i32::MAX as usize {
        return None;
    }
    let changes = g.current.windows(2).filter(|pair| pair[0] != pair[1]).count();
    let epochs = 1usize.checked_add(changes)?.checked_add(breaks)?;
    // Between these cuts, earlier history cannot acquire a negative combo/protection command.
    // New nonnegative bonuses/protection cannot reduce a prefix; appending a nonbreaking entry
    // cannot reduce its nonnegative integer count. Every current-range change is a separate cut,
    // including entering/leaving None and changes between overlapping ranges. A frozen range is
    // constant. This counts all missions, because 7001 reads whichever range is current.
    Some(ComboEpochs { epochs, max_combo })
}

/// Every negative write is strictly later than all already-appended controller history.
/// 12004 ignores the sequence number, and 12000 subtraction does not advance it, so equality
/// is deliberately refused for both. We do not infer safety merely from a raw non-Miss stream.
fn inverse_is_forward(env: &Env, setup: &FullSetup, row: &Row, old_history: &[Option<i32>]) -> Option<()> {
    if !row.gk || row.value < 0 || !row.act.is_finite() || row.act < 0.0 {
        return None;
    }
    if row.effect_type == 12004 && row.limit >= i32::MAX as i64 {
        return None; // the native limited-protection tag is (limit as i32) + 1
    }
    // Untimed sustained rows end only on an observed false trigger, at the current frame (or
    // its music clamp). No timer, release or exhausted-protection callback can backdate them.
    // 12004 itself never inserts a limit_finished callback; only the 11005 applier does so.
    if row.trigger_type == 2 && row.act == 0.0 && row.release == 0 {
        return Some(());
    }
    if row.trigger_type != 1
        || !sole_positive(env, row.trigger, 7010)
        || row.release != 0 && !sole_positive(env, row.release, 7013)
    {
        return None;
    }
    let duration = row.act * 1000f32;
    if !duration.is_finite() || duration > (1 << 24) as f32 {
        return None;
    }
    let g = env.gkf.as_deref()?;
    for range in &g.ranges {
        let Some(start_frame) = range.f_start else { continue };
        let start = range.start;
        let Some(&first) = g.times.get(start_frame.checked_add(1)?) else { continue };
        if start < 0 || start > g.times[start_frame] {
            return None;
        }
        // The first no-release update can finish directly at the current time. Otherwise the
        // updater is EXECUTING; timers are checked each later frame, even with a closed gate.
        if row.release == 0 && duration <= (first - start) as f32 {
            continue;
        }
        if row.act == 0.0 {
            continue; // a release has a current-frame timestamp; no positive timer exists
        }
        let Some(finish_frame) = (start_frame + 2..g.times.len()).find(|&f| duration < (g.times[f] - start) as f32)
        else {
            continue;
        };
        let mut finish = start.checked_add(ournotes_sim::num::ceil_to_i32(duration))?;
        if setup.params.music_length_ms > 0 {
            finish = finish.min(setup.params.music_length_ms);
        }
        if old_history[finish_frame].is_some_and(|time| time >= finish) {
            return None;
        }
    }
    Some(())
}

fn sole_positive(env: &Env, group: i64, kind: i64) -> bool {
    let Some(sets) = env.sets.get(&group) else { return false };
    if sets.len() != 1 || sets[0].len() != 1 {
        return false;
    }
    env.master
        .skill_condition(sets[0][0])
        .is_some_and(|condition| condition.is_positive && condition.condition_type == kind)
}
