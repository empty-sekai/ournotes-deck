//! Independent historical-query readiness for Rush and a separately admitted direct-probe command class.
//!
//! An empty-reward recording supplies only the native clock and solo rank-query topology. It does NOT
//! supply an actual binding's ordinary command prefix or prove that its score probes have filed. That
//! second obligation uses the full physical domain's untimed sustained 7021 / fixed predicate / common
//! phase admission and the explicit complete-clock filing opportunities checked below.
use super::*;
use crate::live::full::luck_score_bounds::{BoundsEvent, BoundsTrace};
use crate::live::score::get_frame;

/// Private probe geometry witness; only `build_both` can construct it. No trace or query cache is retained.
#[derive(Debug)]
pub(super) struct Geometry {
    _complete: (),
}

/// A separate Rush witness. It does not require a direct probe, its phase or its music-clamped lifetime.
/// Only a complete admitted profile can attach a controller law to this empty-recording geometry.
#[derive(Debug)]
pub(super) struct RushGeometry {
    _complete: (),
}

#[derive(Clone, Copy)]
struct Ready {
    time: i32,
    event: usize,
    frame: usize,
}

#[derive(Clone, Copy)]
struct Query {
    ordinal: usize,
    event: usize,
    time: i32,
    to: i32,
    ready: Option<Ready>,
    last_filing: Option<usize>,
}

/// `None` is cancellation; `Some(None)` keeps the old unconditional historical bound. A failure in any
/// historical note/query refuses this entire optional geometry, rather than dropping that note or rank.
///
/// Native `gekisou_after` performs the two adjacent solo queries after ProbabilityReady, with no skill or
/// controller update between them. Whole-family admission excludes score feedback, raw-runtime callbacks
/// and range-machine writers; ordinary rows cannot change this clock/query topology. All recorded notes
/// in the queried score-frame difference must already have filed, even when their chart time is later than
/// the end query's timestamp within its closed 40-ms frame.
///
/// The actual score probe is admitted separately: every holder is untimed, sustained, direct positive 7021,
/// has a fixed predicate, no release/reset/limit, one common legal skill phase, and a paired native inverse.
/// Its only possible filing timestamps are each ORIGINAL frame's `t` and an end's `min(t, music_length)`.
/// At a historical query the current frame's skill phases have already executed. Every subsequent such
/// timestamp is proved strictly later than every included historical note. Thus no later probe start,
/// removal or music-clamped inverse can change that note's ideal signed probe prefix. The completed
/// controller law may be attached to this probe prefix; merely observing an empty trace is never enough.
///
/// Ordinary timed/removal commands can still backfill old times. Their historical window/magnitude bounds
/// remain untouched, as does the unweighted full native floating-point drift.
#[cfg(test)]
pub(super) fn build(
    trace: &BoundsTrace,
    play: &LivePlay,
    setup: &GekisouSetup,
    music_length: i32,
    cancelled: &mut impl FnMut() -> bool,
) -> Option<Option<Geometry>> {
    build_both(trace, play, setup, music_length, cancelled).map(|(probe, _)| probe)
}

/// The two optional witnesses share the original query/note/fixed-bonus proof, but fail independently.
/// For Rush, every possible filing AFTER the original start query must lie in a strictly later native score
/// frame than each included note. Potential covers after-frame pending notes, judged notes and pending lots,
/// plus every before-frame COMPLETE -> FINISH opportunity, including non-LUCK ranges' shared-handle inverses.
/// Weighted actual Luck factors are retained as extra conservative filings, including START commands.
/// Complete family admission rejects overlapping active LUCK
/// ranges, range-clock/score feedback and changed LUCK judgement classes, so this filing superset applies to
/// every legal physical binding, not just the empty deck. The completed profile is still required to use it.
///
/// Earlier filings, including a note and multiple Rush switches in the same frame before ProbabilityReady,
/// are retained. No future native command can change their exact signed integer Rush prefix at the note;
/// hence both adjacent historical queries use its complete chart-time Rush class. This certifies only ideal
/// nonnegative coefficients: ordinary history, integer rank and floating-point error keep their old bounds.
pub(super) fn build_both(
    trace: &BoundsTrace,
    play: &LivePlay,
    setup: &GekisouSetup,
    music_length: i32,
    cancelled: &mut impl FnMut() -> bool,
) -> Option<(Option<Geometry>, Option<RushGeometry>)> {
    if cancelled() {
        return None;
    }
    // Adjacent queries can still consume a pending fixed bonus or cross its native frame. Reuse the
    // complete fixed-(identity, coefficient) cancellation proof instead of inferring it from no commands.
    if !super::super::super::luck_score_bounds::rank_history_structure_ready(trace, cancelled)? {
        return Some((None, None));
    }
    let Some(last) = i32::try_from(trace.frames).ok().and_then(|frames| frames.checked_sub(1)) else {
        return Some((None, None));
    };
    if last < 0 || play.frames.is_empty() || play.frames.windows(2).any(|pair| pair[0].time_ms >= pair[1].time_ms) {
        return Some((None, None));
    }
    let mut probe_ready = music_length > 0;
    let mut rush_ready = trace.has_luck && trace.filing_gate == Some(Some(M_LUCK));
    let (mut next_frame, mut query_count, mut rank_count) = (0usize, 0usize, 0usize);
    let (mut ready, mut previous, mut latest) = (None::<Ready>, None::<Query>, None::<Query>);
    let mut last_filing = None;
    let mut pending_rank = false;
    for (event_index, event) in trace.events.iter().enumerate() {
        if event_index.is_multiple_of(64) && cancelled() {
            return None;
        }
        match event {
            BoundsEvent::Note { .. }
            | BoundsEvent::Factor { .. }
            | BoundsEvent::Potential { .. }
            | BoundsEvent::Probe { .. } => last_filing = Some(event_index),
            BoundsEvent::Combo { .. } => {}
            BoundsEvent::ProbabilityReady(time) => {
                if play.frames.get(next_frame).is_none_or(|frame| frame.time_ms != *time) {
                    return Some((None, None));
                }
                ready = Some(Ready { time: *time, event: event_index, frame: next_frame });
                next_frame += 1;
            }
            BoundsEvent::Query { time_ms, to } => {
                if *to < 0 || *to > last || *to != get_frame(*time_ms).min(last) {
                    return Some((None, None));
                }
                previous = latest;
                latest = Some(Query {
                    ordinal: query_count,
                    event: event_index,
                    time: *time_ms,
                    to: *to,
                    ready,
                    last_filing,
                });
                query_count += 1;
                pending_rank = false;
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                rank_count += 1;
                let (Some(a), Some(b), Some(current)) = (previous, latest, ready) else {
                    return Some((None, None));
                };
                let Some(&(range_start, range_end)) = setup.fevers.get(*range) else {
                    return Some((None, None));
                };
                if *start != Some(a.ordinal)
                    || *end != Some(b.ordinal)
                    || a.ordinal.checked_add(1) != Some(b.ordinal)
                    || a.to > b.to
                    || (a.time, b.time, *time_ms) != (range_start, range_end, range_end)
                    || *percent < 0
                    || a.last_filing != b.last_filing
                    || a.ready.is_none_or(|value| value.event != current.event)
                    || b.ready.is_none_or(|value| value.event != current.event)
                    || a.event <= current.event
                    || b.event <= current.event
                    || a.time > current.time
                    || b.time > current.time
                {
                    return Some((None, None));
                }
                // This is a superset of ACTUAL future probe filings, not the empty recorder's observed
                // commands. Keep the music clamp even when the empty deck has no holder that could use it.
                let earliest_future = play.frames.get(current.frame + 1).map(|frame| frame.time_ms.min(music_length));
                // Use the original query event, not the note's filing event: this frame's controller
                // updates are already reflected by Ready and may legitimately target that same note.
                let mut future_rush_frame = None::<usize>;
                if rush_ready {
                    for (ordinal, event) in trace.events.iter().enumerate().skip(a.event + 1) {
                        if ordinal.is_multiple_of(64) && cancelled() {
                            return None;
                        }
                        let frame = match event {
                            BoundsEvent::Potential { frame } => Some(*frame),
                            BoundsEvent::Factor { frame, command } if command.luck != 0 => {
                                if i32::try_from(*frame).ok() != Some(get_frame(command.time_ms).min(last)) {
                                    rush_ready = false;
                                }
                                Some(*frame)
                            }
                            _ => None,
                        };
                        if let Some(frame) = frame {
                            if frame >= trace.frames {
                                rush_ready = false;
                            }
                            future_rush_frame = Some(future_rush_frame.map_or(frame, |old| old.min(frame)));
                        }
                    }
                }
                for (ordinal, event) in trace.events.iter().enumerate() {
                    if ordinal.is_multiple_of(64) && cancelled() {
                        return None;
                    }
                    let BoundsEvent::Note { frame, note, .. } = event else { continue };
                    let Ok(frame) = i32::try_from(*frame) else { return Some((None, None)) };
                    if frame != get_frame(note.time_ms).min(last) {
                        return Some((None, None));
                    }
                    if a.to < frame && frame <= b.to {
                        if ordinal >= a.event || note.time_ms > current.time {
                            return Some((None, None));
                        }
                        if music_length <= note.time_ms || earliest_future.is_some_and(|time| time <= note.time_ms) {
                            probe_ready = false;
                        }
                        if future_rush_frame.is_some_and(|future| future <= frame as usize) {
                            rush_ready = false;
                        }
                    }
                }
                last_filing = Some(event_index);
                pending_rank = true;
            }
        }
    }
    if cancelled() {
        return None;
    }
    let expected_queries = play.frames.len().checked_add(rank_count).and_then(|count| count.checked_mul(2));
    if next_frame != play.frames.len()
        || query_count != trace.queries
        || Some(query_count) != expected_queries
        || latest.is_none()
        || pending_rank
    {
        return Some((None, None));
    }
    Some((probe_ready.then_some(Geometry { _complete: () }), rush_ready.then_some(RushGeometry { _complete: () })))
}

#[cfg(test)]
#[path = "rank_history_tests.rs"]
mod tests;
