//! The native schedule a bounds recording observes: its events, trace and combo observer.

use super::*;

#[derive(Clone, Debug)]
pub(crate) enum BoundsEvent {
    Note {
        frame: usize,
        index: usize,
        note: NoteCommand,
    },
    Factor {
        frame: usize,
        command: FactorCommand,
    },
    /// A possible Rush filing in this frame: at a play frame's start, where only a range FINISH disables a Rush,
    /// or by the play frame's lotteries.
    Potential {
        frame: usize,
        start: bool,
    },
    /// A possible probe filing at this frame time: every probe row may switch on or off here.
    Probe {
        frame: usize,
        time_ms: i32,
    },
    Query {
        time_ms: i32,
        to: i32,
    },
    /// Exact deterministic combo inputs at a query; emitted only when this note's values change.
    Combo {
        frame: usize,
        index: usize,
        ordinary: f32,
        gekisou: f32,
    },
    /// All lottery-dependent commands at chart times <= this completed native frame have been filed.
    ProbabilityReady(i32),
    Rank {
        range: usize,
        time_ms: i32,
        percent: i64,
        start: Option<usize>,
        end: Option<usize>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct BoundsTrace {
    pub events: Vec<BoundsEvent>,
    pub queries: usize,
    pub frames: usize,
    pub probes: Vec<ProbeRow>,
    pub combo: ComboObserver,
    pub has_luck: bool,
}

/// The filed notes a score query must observe again: every note whose combo inputs may have changed since the
/// previous query. A note outside these frames still has the factors it was last observed with, so the full
/// rescan would emit nothing for it.
#[derive(Clone, Debug, Default)]
pub(crate) struct ComboObserver {
    /// Last observed (ordinary, Gekisou) combo factor bits of each filed note, by frame and index.
    pub(super) seen: Vec<Vec<Option<(u32, u32)>>>,
    /// Every note of the first `consistent` frames was observed at the inputs of the previous query.
    consistent: usize,
    /// Combo judgements recorded before the previous query.
    pub(super) judgements: usize,
    /// Gekisou combo ranges and versions at the previous query; None before the first query.
    pub(super) windows: Option<Vec<(i32, i32, u64)>>,
    pub(super) current: Vec<(i32, i32, u64)>,
    /// Frames that received a note since the previous query.
    pub(super) filed: Vec<usize>,
    stale: Vec<(usize, usize)>,
}

impl ComboObserver {
    pub(crate) fn filed(&mut self, frame: usize) {
        self.filed.push(frame);
    }

    /// Ascending disjoint half-open intervals of the first `frames` frames to observe at this query. `frame_of` is
    /// the monotone frame a note at a chart time is filed in. Return the intervals through [`Self::end`].
    #[inline(never)]
    pub(crate) fn begin(
        &mut self,
        frames: usize,
        combo: &super::combo::ComboCounter,
        gekisou: Option<&dyn crate::live::score::GekisouComboInfo>,
        frame_of: impl Fn(i32) -> usize,
    ) -> Vec<(usize, usize)> {
        let known = self.consistent.min(frames);
        let mut stale = std::mem::take(&mut self.stale);
        stale.clear();
        stale.push((known, frames));
        // A judgement at t changes the combo read only at times after t.
        let times = combo.judgement_times();
        match times.get(self.judgements..) {
            Some(new) => {
                if let Some(&first) = new.iter().min() {
                    stale.push((frame_of(first), known));
                }
            }
            None => stale.push((0, known)),
        }
        self.judgements = times.len();
        self.current.clear();
        if let Some(gekisou) = gekisou {
            gekisou.combo_windows(&mut self.current);
        }
        match &mut self.windows {
            Some(previous) if previous.len() == self.current.len() => {
                for (old, new) in previous.iter().zip(&self.current) {
                    if old != new {
                        stale.push((frame_of(old.0.min(new.0)), frame_of(old.1.max(new.1)) + 1));
                    }
                }
                std::mem::swap(previous, &mut self.current);
            }
            windows => {
                stale.push((0, known));
                *windows = Some(self.current.clone());
            }
        }
        stale.extend(self.filed.drain(..).map(|frame| (frame, frame + 1)));
        for interval in &mut stale {
            interval.1 = interval.1.min(frames);
        }
        stale.retain(|interval| interval.0 < interval.1);
        stale.sort_unstable();
        let mut merged = 0;
        for i in 0..stale.len() {
            if merged > 0 && stale[i].0 <= stale[merged - 1].1 {
                stale[merged - 1].1 = stale[merged - 1].1.max(stale[i].1);
            } else {
                stale[merged] = stale[i];
                merged += 1;
            }
        }
        stale.truncate(merged);
        self.consistent = frames;
        stale
    }

    pub(crate) fn end(&mut self, stale: Vec<(usize, usize)>) {
        self.stale = stale;
    }

    #[cfg(test)]
    pub(crate) fn seen(&self, frame: usize, index: usize) -> Option<(u32, u32)> {
        self.seen.get(frame).and_then(|row| row.get(index)).copied().flatten()
    }

    /// Records an observation; true when it differs from the note's previous one.
    #[inline(never)]
    pub(crate) fn changed(&mut self, frame: usize, index: usize, bits: (u32, u32)) -> bool {
        if self.seen.len() <= frame {
            self.seen.resize_with(frame + 1, Vec::new);
        }
        let row = &mut self.seen[frame];
        if row.len() <= index {
            row.resize(index + 1, None);
        }
        let changed = row[index] != Some(bits);
        row[index] = Some(bits);
        changed
    }
}
