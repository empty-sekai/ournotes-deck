//! Query structure for a completed, admitted native rank recording.
//!
//! This module proves only which stored prefixes cancel and how many copies of each filed rank bonus the
//! terminal query contains. It supplies no score value, probability law, numerical cap or completion state.
//! Its caller retains the full recorder/FINISH admission and separately checks probability readiness for
//! every note it includes, the native integer rank arithmetic, and all note/factor enclosures.

use super::{BoundsEvent, BoundsTrace};
use crate::live::score::get_frame;

/// A query's positions in the complete recording. Neither index is renumbered after projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RankQuery {
    /// Zero-based ordinal among every original Query; the native Rank.start/end fields use this index.
    pub(super) ordinal: usize,
    /// Zero-based ordinal among every original BoundsEvent.
    pub(super) event: usize,
    pub(super) time_ms: i32,
    pub(super) to: i32,
    /// Maximum ProbabilityReady seen before this exact Query, not the end-of-recording maximum.
    pub(super) probability_ready: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RankWindow {
    /// Unique rank identity in original filing order; a range number is not a bonus identity.
    pub(super) id: usize,
    pub(super) event: usize,
    pub(super) range: usize,
    pub(super) percent: i64,
    /// Native get_frame(time_ms), deliberately not clamped to the score-frame array.
    pub(super) frame: i32,
    pub(super) start: RankQuery,
    pub(super) end: RankQuery,
    /// Native terminal coefficient. An overwritten, never-filed pending rank has coefficient zero.
    pub(super) final_coefficient: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RankTracePlan {
    pub(super) ranks: Vec<RankWindow>,
    pub(super) terminal: RankQuery,
}

/// Structural refusals retain the caller's existing conservative path. Cancellation is a separate Ok(None).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RankTraceDecline {
    Capacity,
    FrameIndex,
    MissingSnapshot,
    UnrecordedSnapshot,
    NonadjacentSnapshots,
    ReversedSnapshots,
    InterveningFiling,
    ChangedFixedCoefficients,
    DuplicateFixedFrame,
    PendingRank,
    QueryCount,
    MissingTerminalQuery,
}

struct QueryRecord {
    query: RankQuery,
    /// Original event index of the most recent possible native filing, including Rank pending writes.
    last_filing: Option<usize>,
    /// Fixed bonus identities and their complete native coefficients at this query.
    fixed_coefficients: Vec<(usize, u8)>,
}

struct FixedRank {
    id: usize,
    frame: i32,
    offset: u8,
}

impl FixedRank {
    fn coefficient(&self, to: i32) -> u8 {
        self.offset + u8::from(self.frame <= to)
    }
}

fn reserve<T>(items: &mut Vec<T>, additional: usize) -> Result<(), RankTraceDecline> {
    items.try_reserve(additional).map_err(|_| RankTraceDecline::Capacity)
}

/// Compile only adjacent stored-prefix differences with no intervening possible native filing. Combo
/// observations and ProbabilityReady events do not file score commands, so their original positions remain
/// available to the numerical consumer without invalidating the stored-prefix cancellation.
///
/// Each Query consumes the most recent pending Rank exactly once. Its permanent filing offset is one when
/// its native frame lies after that Query's prefix, and zero otherwise. Every later Query contains this
/// offset plus its ordinary in-prefix copy. Comparing complete (identity, coefficient) snapshots proves that
/// the earlier fixed bonuses cancel in a rank difference without assuming that their values are independent.
///
/// Missing/forward snapshot references, unsupported prefix shapes and incomplete recordings decline. No raw
/// DP result can establish this recording's admission, and no later Ready event can repair an earlier Query.
pub(super) fn compile(
    trace: &BoundsTrace,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<RankTracePlan>, RankTraceDecline> {
    if cancelled() {
        return Ok(None);
    }
    let frames = i32::try_from(trace.frames).map_err(|_| RankTraceDecline::FrameIndex)?;
    if frames <= 0 {
        return Err(RankTraceDecline::FrameIndex);
    }
    let mut queries = Vec::<QueryRecord>::new();
    reserve(&mut queries, trace.queries.min(trace.events.len()))?;
    let mut ranks = Vec::<RankWindow>::new();
    let mut fixed = Vec::<FixedRank>::new();
    let mut pending = None::<usize>;
    let mut probability_ready = i32::MIN;
    let mut last_filing = None;
    for (event_index, event) in trace.events.iter().enumerate() {
        if event_index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        match event {
            BoundsEvent::Note { .. }
            | BoundsEvent::Factor { .. }
            | BoundsEvent::Potential { .. }
            | BoundsEvent::Probe { .. } => {
                last_filing = Some(event_index);
            }
            BoundsEvent::ProbabilityReady(time) => {
                probability_ready = probability_ready.max(*time);
            }
            BoundsEvent::Combo { .. } => {}
            BoundsEvent::Query { time_ms, to } => {
                if queries.len() >= trace.queries {
                    return Err(RankTraceDecline::QueryCount);
                }
                if *to < 0 || *to >= frames {
                    return Err(RankTraceDecline::FrameIndex);
                }
                if let Some(id) = pending.take() {
                    let frame = ranks[id].frame;
                    // The completed native recorder already rejects a second fixed score in one frame.
                    // Preserve that condition instead of silently merging distinct bonus identities.
                    if fixed.iter().any(|entry| entry.frame == frame) {
                        return Err(RankTraceDecline::DuplicateFixedFrame);
                    }
                    reserve(&mut fixed, 1)?;
                    fixed.push(FixedRank { id, frame, offset: u8::from(frame > *to) });
                }
                let mut fixed_coefficients = Vec::new();
                reserve(&mut fixed_coefficients, fixed.len())?;
                for (index, entry) in fixed.iter().enumerate() {
                    if index.is_multiple_of(64) && cancelled() {
                        return Ok(None);
                    }
                    fixed_coefficients.push((entry.id, entry.coefficient(*to)));
                }
                reserve(&mut queries, 1)?;
                queries.push(QueryRecord {
                    query: RankQuery {
                        ordinal: queries.len(),
                        event: event_index,
                        time_ms: *time_ms,
                        to: *to,
                        probability_ready,
                    },
                    last_filing,
                    fixed_coefficients,
                });
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                let (Some(start), Some(end)) = (*start, *end) else {
                    return Err(RankTraceDecline::MissingSnapshot);
                };
                let a = queries.get(start).ok_or(RankTraceDecline::UnrecordedSnapshot)?;
                let b = queries.get(end).ok_or(RankTraceDecline::UnrecordedSnapshot)?;
                if start.checked_add(1) != Some(end) {
                    return Err(RankTraceDecline::NonadjacentSnapshots);
                }
                if a.query.to > b.query.to {
                    return Err(RankTraceDecline::ReversedSnapshots);
                }
                if a.last_filing != b.last_filing {
                    return Err(RankTraceDecline::InterveningFiling);
                }
                if a.fixed_coefficients != b.fixed_coefficients {
                    return Err(RankTraceDecline::ChangedFixedCoefficients);
                }
                let id = ranks.len();
                reserve(&mut ranks, 1)?;
                ranks.push(RankWindow {
                    id,
                    event: event_index,
                    range: *range,
                    percent: *percent,
                    frame: get_frame(*time_ms),
                    start: a.query,
                    end: b.query,
                    final_coefficient: 0,
                });
                // Native add_fixed keeps only the last pending write before the next calculation.
                // An overwritten rank remains represented with coefficient zero; it was never filed.
                pending = Some(id);
                last_filing = Some(event_index);
            }
        }
    }
    if cancelled() {
        return Ok(None);
    }
    if pending.is_some() {
        return Err(RankTraceDecline::PendingRank);
    }
    if queries.len() != trace.queries {
        return Err(RankTraceDecline::QueryCount);
    }
    let terminal = queries.last().ok_or(RankTraceDecline::MissingTerminalQuery)?.query;
    for (index, entry) in fixed.into_iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        ranks[entry.id].final_coefficient = entry.coefficient(terminal.to);
    }
    if cancelled() {
        return Ok(None);
    }
    Ok(Some(RankTracePlan { ranks, terminal }))
}

#[cfg(test)]
mod tests {
    use super::super::{ComboObserver, NoteCommand};
    use super::*;
    use crate::live::skill::FactorCommand;

    fn query(to: i32) -> BoundsEvent {
        BoundsEvent::Query { time_ms: to * 40, to }
    }

    fn rank(range: usize, time_ms: i32, start: usize, end: usize) -> BoundsEvent {
        BoundsEvent::Rank { range, time_ms, percent: 333, start: Some(start), end: Some(end) }
    }

    fn trace(events: Vec<BoundsEvent>) -> BoundsTrace {
        BoundsTrace {
            queries: events.iter().filter(|event| matches!(event, BoundsEvent::Query { .. })).count(),
            events,
            frames: 32,
            probes: Vec::new(),
            combo: ComboObserver::default(),
            has_luck: true,
            filing_gate: Some(None),
        }
    }

    fn compile_plan(events: Vec<BoundsEvent>) -> RankTracePlan {
        compile(&trace(events), || false).unwrap().unwrap()
    }

    #[test]
    fn rank_plan_keeps_original_event_and_query_ordinals_and_prior_readiness() {
        let plan = compile_plan(vec![
            BoundsEvent::Note { frame: 2, index: 0, note: NoteCommand::new(80, 1000, 1, 1, 3) },
            BoundsEvent::ProbabilityReady(80),
            query(3),
            BoundsEvent::Combo { frame: 2, index: 0, ordinary: 1.0, gekisou: 1.0 },
            BoundsEvent::ProbabilityReady(320),
            query(8),
            rank(4, 360, 0, 1),
            BoundsEvent::ProbabilityReady(10000),
            query(10),
        ]);
        let rank = plan.ranks[0];
        assert_eq!((rank.id, rank.event, rank.range, rank.percent, rank.frame), (0, 6, 4, 333, 9));
        assert_eq!((rank.start.ordinal, rank.start.event, rank.start.to, rank.start.probability_ready), (0, 2, 3, 80));
        assert_eq!((rank.end.ordinal, rank.end.event, rank.end.to, rank.end.probability_ready), (1, 5, 8, 320));
        assert_eq!(rank.final_coefficient, 1);
        assert_eq!((plan.terminal.ordinal, plan.terminal.event, plan.terminal.probability_ready), (2, 8, 10000));
    }

    #[test]
    fn rank_plan_never_uses_a_later_ready_for_a_query() {
        let plan = compile_plan(vec![
            BoundsEvent::ProbabilityReady(10),
            query(1),
            BoundsEvent::ProbabilityReady(5),
            query(2),
            rank(0, 120, 0, 1),
            query(4),
            BoundsEvent::ProbabilityReady(i32::MAX),
        ]);
        assert_eq!(plan.ranks[0].start.probability_ready, 10);
        assert_eq!(plan.ranks[0].end.probability_ready, 10);
        assert_eq!(plan.terminal.probability_ready, 10);
        let empty = compile_plan(vec![query(0), BoundsEvent::ProbabilityReady(100)]);
        assert!(empty.ranks.is_empty());
        assert_eq!(empty.terminal.probability_ready, i32::MIN);
    }

    #[test]
    fn rank_plan_requires_recorded_adjacent_snapshot_ids() {
        let prefix = vec![query(0), query(1), query(2)];
        for (start, end, expected) in [
            (None, Some(1), RankTraceDecline::MissingSnapshot),
            (Some(0), None, RankTraceDecline::MissingSnapshot),
            (Some(0), Some(3), RankTraceDecline::UnrecordedSnapshot),
            (Some(usize::MAX), Some(1), RankTraceDecline::UnrecordedSnapshot),
            (Some(0), Some(2), RankTraceDecline::NonadjacentSnapshots),
            (Some(1), Some(1), RankTraceDecline::NonadjacentSnapshots),
            (Some(1), Some(0), RankTraceDecline::NonadjacentSnapshots),
        ] {
            let mut events = prefix.clone();
            events.push(BoundsEvent::Rank { range: 0, time_ms: 120, percent: 250, start, end });
            events.push(query(4));
            assert_eq!(compile(&trace(events), || false), Err(expected));
        }
    }

    #[test]
    fn rank_plan_requires_nonnegative_ordered_score_prefixes() {
        for to in [-2, -1, 32] {
            assert_eq!(compile(&trace(vec![query(to)]), || false), Err(RankTraceDecline::FrameIndex));
        }
        let reversed = trace(vec![query(3), query(2), rank(0, 160, 0, 1), query(5)]);
        assert_eq!(compile(&reversed, || false), Err(RankTraceDecline::ReversedSnapshots));
        let equal = compile_plan(vec![query(4), query(4), rank(0, 200, 0, 1), query(6)]);
        assert_eq!((equal.ranks[0].start.to, equal.ranks[0].end.to), (4, 4));
    }

    #[test]
    fn rank_plan_rejects_every_possible_filing_between_snapshots() {
        let filings = [
            BoundsEvent::Note { frame: 1, index: 0, note: NoteCommand::new(40, 1000, 1, 1, 3) },
            BoundsEvent::Factor { frame: 1, command: FactorCommand::default() },
            BoundsEvent::Potential { frame: 1 },
            BoundsEvent::Probe { frame: 1, time_ms: 40 },
        ];
        for filing in filings {
            let trace = trace(vec![query(0), filing, query(1), rank(0, 80, 0, 1), query(3)]);
            assert_eq!(compile(&trace, || false), Err(RankTraceDecline::InterveningFiling));
        }
        let with_rank = trace(vec![query(0), query(1), rank(0, 80, 0, 1), query(3), rank(1, 160, 1, 2), query(5)]);
        assert_eq!(compile(&with_rank, || false), Err(RankTraceDecline::InterveningFiling));
    }

    #[test]
    fn rank_plan_preserves_native_filing_offsets_and_unclamped_frames() {
        let plan = compile_plan(vec![
            query(0),
            query(1),
            rank(0, 200, 0, 1),
            query(2),
            query(4),
            rank(1, 440, 2, 3),
            query(8),
        ]);
        // Rank zero was added immediately before its native frame first executed: both copies survive.
        assert_eq!((plan.ranks[0].frame, plan.ranks[0].final_coefficient), (5, 2));
        // Rank one's frame remains after the final prefix; its permanent filing copy still survives.
        assert_eq!((plan.ranks[1].frame, plan.ranks[1].final_coefficient), (11, 1));
        let outside = compile_plan(vec![query(0), query(1), rank(0, i32::MAX, 0, 1), query(31)]);
        assert_eq!(outside.ranks[0].frame, get_frame(i32::MAX));
        assert!(outside.ranks[0].frame > 31);
        assert_eq!(outside.ranks[0].final_coefficient, 1);
    }

    #[test]
    fn rank_plan_rejects_a_shared_bonus_whose_snapshot_coefficient_changed() {
        let trace =
            trace(vec![query(0), query(1), rank(0, 200, 0, 1), query(2), query(8), rank(1, 440, 2, 3), query(12)]);
        assert_eq!(compile(&trace, || false), Err(RankTraceDecline::ChangedFixedCoefficients));
    }

    #[test]
    fn rank_plan_preserves_zero_coefficients_and_pending_last_write_wins() {
        let undone = compile_plan(vec![query(0), query(1), rank(0, 200, 0, 1), query(8), query(3)]);
        assert_eq!(undone.ranks[0].final_coefficient, 0);
        let overwritten = compile_plan(vec![query(0), query(1), rank(7, 80, 0, 1), rank(7, 120, 0, 1), query(8)]);
        assert_eq!(overwritten.ranks.len(), 2);
        assert_eq!((overwritten.ranks[0].id, overwritten.ranks[0].final_coefficient), (0, 0));
        assert_eq!((overwritten.ranks[1].id, overwritten.ranks[1].final_coefficient), (1, 1));
        assert_ne!(overwritten.ranks[0].event, overwritten.ranks[1].event);
    }

    #[test]
    fn rank_plan_rejects_duplicate_native_fixed_frames() {
        let trace =
            trace(vec![query(0), query(1), rank(0, 120, 0, 1), query(4), query(5), rank(1, 120, 2, 3), query(6)]);
        assert_eq!(compile(&trace, || false), Err(RankTraceDecline::DuplicateFixedFrame));
    }

    #[test]
    fn rank_plan_requires_a_complete_matching_terminal_query() {
        assert_eq!(compile(&trace(Vec::new()), || false), Err(RankTraceDecline::MissingTerminalQuery));
        let pending = trace(vec![query(0), query(1), rank(0, 80, 0, 1)]);
        assert_eq!(compile(&pending, || false), Err(RankTraceDecline::PendingRank));
        let mut mismatch = trace(vec![query(0)]);
        mismatch.queries = 2;
        assert_eq!(compile(&mismatch, || false), Err(RankTraceDecline::QueryCount));
        let mut no_frames = trace(vec![query(0)]);
        no_frames.frames = 0;
        assert_eq!(compile(&no_frames, || false), Err(RankTraceDecline::FrameIndex));
    }

    #[test]
    fn rank_plan_cancellation_never_returns_a_partial_certificate() {
        let empty = trace(Vec::new());
        assert_eq!(compile(&empty, || true), Ok(None));
        let mut events = vec![BoundsEvent::ProbabilityReady(0); 256];
        events.extend([query(0), query(1), rank(0, 80, 0, 1), query(3)]);
        let mut polls = 0;
        assert_eq!(
            compile(&trace(events), || {
                polls += 1;
                polls == 3
            }),
            Ok(None)
        );
        assert_eq!(polls, 3);
        let mut polls = 0;
        assert_eq!(
            compile(&trace(vec![query(0)]), || {
                polls += 1;
                polls == 3
            }),
            Ok(None)
        );
    }

    #[test]
    fn rank_plan_capacity_failure_declines_without_an_allocation_attempt() {
        let mut bytes = Vec::<u8>::new();
        assert_eq!(reserve(&mut bytes, usize::MAX), Err(RankTraceDecline::Capacity));
        assert!(bytes.is_empty());
    }
}
