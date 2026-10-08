//! Exact assembly of an admitted ordinary recording and a complete controller-output history.
//!
//! This uses the same IncrementalCalculator execution/undo loops as native playback. Potential markers
//! are not filings. Only actual timeline edges replace the weighted recorder's artificial Rush commands.
use super::super::super::scorecalc::IncrementalCalculator;
use super::super::timeline_support::{TimelineEdge, TimelineKind};
use super::*;
use crate::live::score::get_frame;
use crate::live::skill::FactorCommand;

mod prefix;
pub(super) use prefix::evaluate_support;
#[cfg(test)]
pub(super) use prefix::evaluate_support_with_limit;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAMES: usize = 8192;
const MAX_QUERIES: u64 = 128_000_000;
const MAX_FRAME_STEPS: u64 = 256_000_000;
const MAX_EVENTS: u64 = 512_000_000;

#[derive(Default)]
pub(super) struct Work {
    pub queries: u64,
    frame_steps: u64,
    events: u64,
    #[cfg(test)]
    pub checkpoint_peak_bytes: usize,
}

pub(super) struct Recipe {
    initial: IncrementalCalculator,
    events: Vec<BoundsEvent>,
    anchors: Vec<Option<(usize, u8)>>,
    note_counts: Vec<usize>,
    probes: Vec<EffectiveProbe>,
    music_length: i32,
    rush_percent: i32,
    frames: usize,
    queries: usize,
    clock: Vec<i32>,
}

fn refusal<T>() -> Checked<T> {
    declined(LuckScoreEquivalenceDecline::ScoreTrace)
}

impl Recipe {
    pub(super) fn admits_frames(frames: usize) -> bool {
        (1..=MAX_FRAMES).contains(&frames)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        initial: IncrementalCalculator,
        events: Vec<BoundsEvent>,
        frames: usize,
        queries: usize,
        probes: Vec<EffectiveProbe>,
        music_length: i32,
        rush_percent: i32,
        clock: &[i32],
    ) -> Checked<Self> {
        let estimated = frames
            .saturating_mul(512)
            .saturating_add(events.capacity().saturating_mul(size_of::<BoundsEvent>() + 64))
            .saturating_add(queries.saturating_mul(16))
            .saturating_add(clock.len().saturating_mul(4))
            .saturating_add(probes.capacity().saturating_mul(size_of::<EffectiveProbe>()));
        if estimated > MAX_BYTES || !Self::admits_frames(frames) || clock.is_empty() {
            return declined(LuckScoreEquivalenceDecline::Capacity);
        }
        // This also makes primary-query anchors unambiguous against the preceding frame's historical
        // rank queries. Repeated or reversed playback clocks remain eligible for the original route.
        if clock.windows(2).any(|pair| pair[0] >= pair[1]) {
            return refusal();
        }
        let mut anchors = vec![None; events.len()];
        let mut note_counts = vec![0usize; frames];
        let (mut frame, mut primary, mut observed_queries) = (0usize, 0usize, 0usize);
        for (index, event) in events.iter().enumerate() {
            match event {
                BoundsEvent::Note { frame, index, .. } => {
                    let Some(count) = note_counts.get_mut(*frame) else {
                        return refusal();
                    };
                    if *index != *count {
                        return refusal();
                    }
                    *count += 1;
                }
                BoundsEvent::Combo { frame, index, .. } => {
                    if note_counts.get(*frame).is_none_or(|count| *index >= *count) {
                        return refusal();
                    }
                }
                BoundsEvent::Factor { command, .. } => {
                    // Only the native Rush Handle owns -1. Reject any other payload before removing its
                    // weighted placeholder, including the zero-percent command which still invalidates.
                    if command.owner_id == -1
                        && (command.note_mill != 0
                            || command.combo_mill != 0
                            || command.judgement != 0
                            || command.judge_mill != 0
                            || command.band_total_power != 0)
                    {
                        return refusal();
                    }
                    if command.owner_id != -1 && command.luck != 0 {
                        return refusal();
                    }
                }
                BoundsEvent::Query { time_ms, .. } => {
                    if clock.get(frame) == Some(time_ms) {
                        if primary >= 2 {
                            return refusal();
                        }
                        anchors[index] = Some((frame, primary as u8));
                        primary += 1;
                    }
                    observed_queries += 1;
                }
                BoundsEvent::ProbabilityReady(time) => {
                    if clock.get(frame) != Some(time) || primary != 2 {
                        return refusal();
                    }
                    anchors[index] = Some((frame, 2));
                    frame += 1;
                    primary = 0;
                }
                BoundsEvent::Rank { start, end, .. } => {
                    if start.is_some_and(|q| q >= observed_queries) || end.is_none_or(|q| q >= observed_queries) {
                        return refusal();
                    }
                }
                BoundsEvent::Potential { .. } | BoundsEvent::Probe { .. } => {}
            }
        }
        if frame != clock.len() || observed_queries != queries {
            return refusal();
        }
        Ok(Self {
            initial,
            events,
            anchors,
            note_counts,
            probes,
            music_length,
            rush_percent,
            frames,
            queries,
            clock: clock.to_vec(),
        })
    }

    pub(super) fn has_probes(&self) -> bool {
        !self.probes.is_empty()
    }

    /// Timeline edges never reorder recorded ordinary sources. No calculation happens between skill
    /// phases; the admission makes every same-owner ordinary filing precede its probe. Other owners are
    /// ordered by the original native stable (time, owner) comparator, inside the shared execute loop.
    pub(super) fn evaluate(
        &self,
        path: &[TimelineEdge],
        work: &mut Work,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Checked<i32> {
        poll(cancelled)?;
        let mut calc = self.initial.clone();
        let mut combos: Vec<Vec<Option<(f32, f32)>>> = self.note_counts.iter().map(|&n| vec![None; n]).collect();
        let mut snapshots = Vec::<i32>::with_capacity(self.queries);
        let (mut edge, mut rush, mut probe) = (0usize, false, false);
        let (mut previous, mut added) = (-1i32, -1i32);
        let file = |time: i32, added: &mut i32| -> Checked<()> {
            let frame = get_frame(time).min(self.frames as i32 - 1);
            if frame < 0 {
                return refusal();
            }
            *added = if *added < 0 { frame } else { (*added).min(frame) };
            Ok(())
        };
        for (index, event) in self.events.iter().enumerate() {
            if index.is_multiple_of(64) {
                poll(cancelled)?;
            }
            if work.events >= MAX_EVENTS {
                return declined(LuckScoreEquivalenceDecline::WorkBudget);
            }
            work.events += 1;
            if let Some((frame, anchor)) = self.anchors[index] {
                while let Some(next) = path.get(edge) {
                    if next.frame < frame {
                        return refusal();
                    }
                    if next.frame != frame
                        || match anchor {
                            0 => next.stage != 0,
                            1 => next.stage != 1,
                            _ => next.stage < 2,
                        }
                    {
                        break;
                    }
                    if next.chart_time > self.clock[frame] {
                        return refusal();
                    }
                    match next.kind {
                        TimelineKind::Rush => {
                            if !matches!(next.stage, 0 | 2 | 3) || next.on == rush {
                                return refusal();
                            }
                            rush = next.on;
                            file(next.chart_time, &mut added)?;
                            calc.add_factor(FactorCommand {
                                time_ms: next.chart_time,
                                owner_id: -1,
                                luck: if next.on { self.rush_percent } else { self.rush_percent.wrapping_neg() },
                                ..Default::default()
                            });
                        }
                        TimelineKind::Probe => {
                            if next.stage != 1
                                || next.on == probe
                                || next.chart_time != self.clock[frame]
                                || self.probes.is_empty()
                            {
                                return refusal();
                            }
                            probe = next.on;
                            // Untimed sustained_step changes ExecuteFrame to Executing before a false
                            // predicate forces END_FRAME. Native effect_update clamps that end timestamp.
                            let time = if !next.on && self.music_length > 0 {
                                next.chart_time.min(self.music_length)
                            } else {
                                next.chart_time
                            };
                            for row in &self.probes {
                                file(time, &mut added)?;
                                calc.add_factor(FactorCommand {
                                    time_ms: time,
                                    owner_id: row.owner,
                                    note_mill: if next.on { row.mill } else { row.mill.wrapping_neg() },
                                    ..Default::default()
                                });
                            }
                        }
                    }
                    edge += 1;
                }
            }
            match event {
                BoundsEvent::Note { note, .. } => {
                    file(note.time_ms, &mut added)?;
                    calc.add_note(*note);
                }
                BoundsEvent::Factor { command, .. } if command.owner_id != -1 => {
                    file(command.time_ms, &mut added)?;
                    calc.add_factor(*command);
                }
                BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                    combos[*frame][*index] = Some((*ordinary, *gekisou));
                }
                BoundsEvent::Query { time_ms, to } => {
                    let target = get_frame(*time_ms).max(0).min(self.frames as i32 - 1);
                    if target != *to {
                        return refusal();
                    }
                    let undo_to = if added < 0 { target } else { target.min(added - 1) };
                    let start = if undo_to < previous { undo_to + 1 } else { previous + 1 };
                    let visits = (previous - undo_to).max(0) as u64 + (target - start + 1).max(0) as u64;
                    if work.queries >= MAX_QUERIES || work.frame_steps.saturating_add(visits) > MAX_FRAME_STEPS {
                        return declined(LuckScoreEquivalenceDecline::WorkBudget);
                    }
                    work.queries += 1;
                    work.frame_steps += visits;
                    snapshots.push(native(
                        calc.calculate_recorded(*time_ms, &combos),
                        LuckScoreEquivalenceDecline::ScoreTrace,
                    )?);
                    previous = target;
                    added = -1;
                }
                BoundsEvent::Rank { time_ms, percent, start, end, .. } => {
                    let begin = start.map_or(0, |q| snapshots[q]);
                    let end = snapshots[end.ok_or(Failure::Decline(LuckScoreEquivalenceDecline::ScoreTrace))?];
                    let bonus = ((i128::from(end.wrapping_sub(begin)) * i128::from(*percent)) / 100) as i32;
                    calc.add_fixed(*time_ms, bonus);
                }
                BoundsEvent::Factor { .. }
                | BoundsEvent::Potential { .. }
                | BoundsEvent::Probe { .. }
                | BoundsEvent::ProbabilityReady(_) => {}
            }
        }
        poll(cancelled)?;
        if edge != path.len() || rush || probe || snapshots.len() != self.queries {
            return refusal();
        }
        Ok(calc.score)
    }
}
