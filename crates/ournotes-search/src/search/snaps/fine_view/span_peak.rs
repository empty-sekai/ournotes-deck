//! Original-order binary64 folds over exactly the span-start samples used by candidate drift bounds.
//!
//! Endpoints update membership only. Each sampled sum starts at zero and visits active spans in their
//! original contributor/row order, including zero and signed values. Sample maxima retain their original order
//! too, including duplicate samples and signed-zero ties. No rounded sum is updated by subtraction.
use super::*;

const MIN_SWEEP_SPANS: usize = 16;
// Bound optional scratch storage independently of the candidate domain; larger inputs retain the direct scan.
const MAX_SWEEP_SPANS: usize = 32_768;
const SAMPLE: u8 = 0;
const START: u8 = 1;
const END: u8 = 2;

#[derive(Clone, Copy)]
struct Endpoint {
    time: i64,
    span: u32,
    kind: u8,
}

#[cfg(test)]
#[derive(Default)]
struct Work {
    endpoints: usize,
    samples: usize,
    folded_spans: usize,
    scanned_pairs: usize,
}

#[derive(Default)]
pub(in super::super) struct Scratch {
    events: Vec<Endpoint>,
    values: Vec<f64>,
    samples: Vec<f64>,
    active: Vec<u64>,
    #[cfg(test)]
    work: Work,
}

impl Scratch {
    pub(super) fn peaks(&mut self, parts: [&Contrib; 5], frames: ScoreFrames) -> (f64, f64) {
        self.peaks_with_limit(parts, frames, MAX_SWEEP_SPANS)
    }

    fn peaks_with_limit(&mut self, parts: [&Contrib; 5], frames: ScoreFrames, limit: usize) -> (f64, f64) {
        #[cfg(test)]
        {
            self.work = Work::default();
        }
        let count = parts.iter().map(|part| part.spans.len()).sum::<usize>();
        if !(MIN_SWEEP_SPANS..=limit.min(MAX_SWEEP_SPANS)).contains(&count) || !self.reserve(count) {
            #[cfg(test)]
            {
                self.work.scanned_pairs = count.saturating_mul(count).saturating_mul(2);
            }
            return scan(parts, frames);
        }
        self.values.clear();
        self.values.extend(parts.into_iter().flat_map(|part| part.spans.iter().map(|span| span.2)));
        let time = self.sweep(parts, None);
        let frame = self.sweep(parts, Some(frames));
        (time, frame)
    }

    fn reserve(&mut self, count: usize) -> bool {
        self.events.clear();
        self.values.clear();
        self.samples.clear();
        self.active.clear();
        self.events.try_reserve_exact(count * 3).is_ok()
            && self.values.try_reserve_exact(count).is_ok()
            && self.samples.try_reserve_exact(count).is_ok()
            && self.active.try_reserve_exact(count.div_ceil(64)).is_ok()
    }

    fn sweep(&mut self, parts: [&Contrib; 5], frames: Option<ScoreFrames>) -> f64 {
        self.events.clear();
        self.samples.clear();
        self.samples.resize(self.values.len(), 0.0);
        self.active.clear();
        self.active.resize(self.values.len().div_ceil(64), 0);
        for (span, &(start, end, _)) in parts.iter().flat_map(|part| &part.spans).enumerate() {
            let sample = frames.map_or(start, |frames| i64::from(frames.at(start)));
            self.events.push(Endpoint { time: sample, span: span as u32, kind: SAMPLE });
            let interval = match frames {
                // ScoreFrames is closed, including zero-length spans and clamped music boundaries.
                Some(frames) => frames.closed(start, end).map(|(lo, hi)| (lo, hi + 1)),
                None => (start < end).then_some((start, end)),
            };
            if let Some((lo, hi)) = interval {
                self.events.push(Endpoint { time: lo, span: span as u32, kind: START });
                self.events.push(Endpoint { time: hi, span: span as u32, kind: END });
            }
        }
        self.events.sort_unstable_by_key(|event| event.time);
        #[cfg(test)]
        {
            self.work.endpoints += self.events.len();
        }
        let mut cursor = 0;
        while cursor < self.events.len() {
            let begin = cursor;
            let time = self.events[cursor].time;
            let mut sampled = false;
            while cursor < self.events.len() && self.events[cursor].time == time {
                let event = self.events[cursor];
                match event.kind {
                    SAMPLE => sampled = true,
                    START => self.active[event.span as usize / 64] |= 1u64 << (event.span % 64),
                    END => self.active[event.span as usize / 64] &= !(1u64 << (event.span % 64)),
                    _ => unreachable!("private span endpoint kind"),
                }
                cursor += 1;
            }
            if sampled {
                #[cfg(test)]
                {
                    self.work.samples += 1;
                }
                let mut sum = 0.0f64;
                // Time sorting cannot reorder these rounded additions: bit indexes are original span ordinals.
                for (word, &active) in self.active.iter().enumerate() {
                    let mut active = active;
                    while active != 0 {
                        let index = word * 64 + active.trailing_zeros() as usize;
                        sum = (sum + self.values[index]).next_up();
                        active &= active - 1;
                        #[cfg(test)]
                        {
                            self.work.folded_spans += 1;
                        }
                    }
                }
                for event in &self.events[begin..cursor] {
                    if event.kind == SAMPLE {
                        self.samples[event.span as usize] = sum;
                    }
                }
            }
        }
        self.samples.iter().fold(0.0f64, |peak, &sample| peak.max(sample))
    }
}

/// Small inputs and optional scratch-capacity refusals retain the direct candidate scan.
fn scan(parts: [&Contrib; 5], frames: ScoreFrames) -> (f64, f64) {
    let (mut peak, mut near_peak) = (0.0f64, 0.0f64);
    for part in parts {
        for &(time, _, _) in &part.spans {
            let frame = frames.at(time);
            let (mut at, mut near) = (0.0f64, 0.0f64);
            for other in parts {
                for &(start, end, value) in &other.spans {
                    if start <= time && time < end {
                        at = (at + value).next_up();
                    }
                    if frames.meets(start, end, frame) {
                        near = (near + value).next_up();
                    }
                }
            }
            peak = peak.max(at);
            near_peak = near_peak.max(near);
        }
    }
    (peak, near_peak)
}

#[cfg(test)]
mod tests;
