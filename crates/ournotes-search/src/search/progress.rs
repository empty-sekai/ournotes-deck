//! Progress reports of a running physical-deck search.
//!
//! A report is the result the search would return if its time limit expired at that point: the exactly evaluated
//! Top-K so far and the telemetry so far. Reports are made at the search's own deadline checks and after Top-K
//! insertions, at most once per interval. Building a report only reads the search state (the telemetry is closed on
//! a copy), so the traversal, its node counts and its result are the same with or without a hook.
use super::Engine;
use crate::clock::Instant;
use crate::types::{RecommendationOutcome, Strategy};
use std::time::Duration;

/// A report sink and its rate. The outcome is passed by value so that each layer can complete it.
pub(crate) struct ProgressHook<'a> {
    pub(crate) interval: Duration,
    pub(crate) report: &'a mut dyn FnMut(RecommendationOutcome),
}

pub(super) struct Reporter<'a> {
    hook: ProgressHook<'a>,
    strategy: &'a Strategy,
    /// The search start, for the reports' elapsed time.
    start: Instant,
    /// The first clock value at which the next report may be made.
    due: Instant,
}

impl<'a> Reporter<'a> {
    /// Takes the hook's parts, so that the report sink's lifetime can shorten to the search's.
    pub(super) fn new(
        interval: Duration,
        report: &'a mut dyn FnMut(RecommendationOutcome),
        strategy: &'a Strategy,
        start: Instant,
    ) -> Self {
        let due = start.checked_add(interval).unwrap_or(start);
        Self { hook: ProgressHook { interval, report }, strategy, start, due }
    }
}

impl Engine<'_, '_> {
    /// Reports when the interval since the previous report (or the search start) has passed at `now`.
    pub(super) fn progress_at(&mut self, now: Instant) {
        let Some(reporter) = &self.progress else { return };
        if now < reporter.due {
            return;
        }
        let snapshot = self
            .report_outcome(reporter.strategy, reporter.start, now)
            .expect("every Top-K entry is an exactly evaluated deck with a positive score mass");
        let reporter = self.progress.as_mut().expect("reporter");
        (reporter.hook.report)(snapshot);
        let now = crate::search::budget::now();
        reporter.due = now.checked_add(reporter.hook.interval).unwrap_or(now);
    }

    /// A report check outside the deadline checks (after a Top-K insertion).
    pub(super) fn report_progress(&mut self) {
        if self.progress.is_some() {
            self.progress_at(crate::search::budget::now());
        }
    }
}
