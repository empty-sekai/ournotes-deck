//! A checked identity quotient for once-per-range Miss gauge writers.
//!
//! The original tape still propagates. Only its complete identity replaces each frame's Miss rows
//! by their vector of *separately native-rounded* integer increments. This vector is indexed by an
//! admitted superset of every possible gauge maximum. Equal vectors therefore give the same state
//! transition, including queued lots; adding raw percentages before rounding would not do so.
use super::*;

const DOMAIN: u64 = 0x6d697373646c7431;
const MAX_VIEW_BYTES: usize = 32 << 20;

/// Private evidence for the nonnegative, nonwrapping gauge domain of this original native tape.
/// Minimum conditioning may change only minimum actions. Every other transcript mutation must
/// repeat admission before retaining this capability.
#[derive(Debug)]
pub(super) struct MissGaugeDomain {
    maxima: Vec<i64>,
}

fn unsupported(message: &str) -> Error {
    Error::Unsupported(format!("LUCK Miss gauge quotient: {message}"))
}

fn capacity(message: &str) -> Error {
    Error::Capacity(format!("LUCK Miss gauge quotient: {message}"))
}

/// The exact operations in Dp::action, with a deliberately closed nonwrapping product domain.
/// This never computes floor(maximum * sum(percentages) / 10000).
fn delta(maximum: i64, value: i64) -> Option<i64> {
    let product = i128::from(maximum).checked_mul(i128::from(value))?;
    let product = i32::try_from(product).ok().filter(|&value| value >= 0)?;
    Some(i64::from(floor_to_i32(product as f32 / 10000f32)))
}

fn view_bound(transcript: &Transcript<ProbabilityMass>, dimensions: usize) -> Option<usize> {
    transcript
        .frames
        .len()
        .checked_mul(size_of::<Frame>().checked_add(dimensions.checked_add(2)?.checked_mul(size_of::<u64>())?)?)?
        .checked_add(transcript.actions.len().checked_mul(size_of::<Action<ProbabilityMass>>())?)
        .and_then(|bytes| bytes.checked_add(dimensions.checked_mul(size_of::<i64>())?))
}

fn reserved<T>(capacity: usize) -> Option<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(capacity).ok()?;
    Some(values)
}

impl MissGaugeDomain {
    pub(super) fn admit(transcript: &Transcript<ProbabilityMass>) -> Result<Self, Error> {
        if transcript.collect_moments {
            return Err(unsupported("additive-moment recordings retain their original operator"));
        }
        if let Some(error) = &transcript.failure {
            return Err(error.clone());
        }
        // Include native constructor defaults as well as every original template's initial/default/
        // Rush maxima. The inactive Chain::default has maximum zero, but the geometry check below
        // proves that no gauge action or hit can read that state before a native range start.
        let mut maxima = LuckScore::default().gauge_domain().to_vec();
        for template in &transcript.templates {
            maxima.extend(template.gauge_domain());
            if template.gauge < 0 || i64::from(template.gauge) > template.gauge_max {
                return Err(unsupported("an initial gauge is outside zero through its current maximum"));
            }
        }
        maxima.sort_unstable();
        maxima.dedup();
        if maxima.iter().any(|&maximum| !(1..=i64::from(i32::MAX)).contains(&maximum)) {
            return Err(unsupported("a native gauge maximum is outside the positive binary32/i32 domain"));
        }
        if view_bound(transcript, maxima.len()).is_none_or(|bytes| bytes > MAX_VIEW_BYTES) {
            return Err(capacity("canonical frame/action workspace exceeds 32 MiB"));
        }
        let domain = Self { maxima };
        domain.check_frames(transcript)?;
        Ok(domain)
    }

    fn check_frames(&self, transcript: &Transcript<ProbabilityMass>) -> Result<(), Error> {
        let maximum = *self.maxima.last().ok_or_else(|| unsupported("missing gauge maxima"))?;
        let mut active = None;
        let (mut actions_from, mut notes_from, mut hits_from, mut pending_from) = (0, 0, 0, 0);
        for frame in &transcript.frames {
            if frame.repeat == 0 {
                return Err(unsupported("an empty native frame run"));
            }
            if let Some(range) = frame.start {
                if active.is_some()
                    || frame.repeat != 1
                    || transcript.templates.get(range).is_none()
                    || !transcript.luck.get(range).copied().unwrap_or(false)
                {
                    return Err(unsupported("a start does not install one original native Luck template"));
                }
                active = Some(range);
            }
            let actions = transcript
                .actions
                .get(actions_from..frame.actions)
                .ok_or_else(|| unsupported("invalid native frame action offsets"))?;
            if !actions.is_empty() && (!frame.gate || frame.target < 0 || active != Some(frame.target as usize)) {
                return Err(unsupported("a gauge action can observe the inactive or another range's chain"));
            }
            // All admitted actions run before notes/pending draws, without changing gauge_max.
            // Sum both kinds of possible gauge additions: a Miss operator may cross a probabilistic
            // StartGauge only when even their all-enabled sum stays inside the nonwrapping domain.
            for &gauge_maximum in &self.maxima {
                let mut sum = 0i64;
                for &action in actions {
                    let value = match action {
                        Action::StartGauge { value, .. } | Action::MissGauge { value } => value,
                        Action::StartMinimum { .. } => continue,
                        Action::CriticalPoints { .. } | Action::StartPoints { .. } => {
                            return Err(unsupported("an additive-point action was retained"));
                        }
                    };
                    let value = delta(gauge_maximum, value)
                        .ok_or_else(|| unsupported("a native percent product is negative or wraps i32"))?;
                    sum = sum.checked_add(value).ok_or_else(|| capacity("frame gauge sum overflow"))?;
                }
                if maximum.checked_add(sum).is_none_or(|sum| sum > i64::from(i32::MAX)) {
                    return Err(unsupported("the all-enabled frame gauge sum can wrap native addition"));
                }
            }
            let notes = transcript
                .notes
                .get(notes_from..frame.notes)
                .ok_or_else(|| unsupported("invalid native frame note offsets"))?;
            if frame.repeat != 1 && !notes.is_empty() {
                return Err(unsupported("a repeated frame contains native notes"));
            }
            for note in notes {
                let hits = transcript
                    .hits
                    .get(hits_from..note.hits)
                    .ok_or_else(|| unsupported("invalid native note hit offsets"))?;
                for hit in hits {
                    if active != Some(hit.range) {
                        return Err(unsupported("a hit can observe the inactive or another range's chain"));
                    }
                    if matches!(note.judgement, 0 | 7) {
                        continue;
                    }
                    for (weight, _, point) in
                        transcript.machine.base_point_probability_weights(note.note_type, note.judgement)?
                    {
                        if weight == 0 {
                            continue;
                        }
                        let native = (hit.speed + 1f32) * point as f32;
                        if !native.is_finite() || native < 0.0 {
                            return Err(unsupported("a native note gauge increment is negative or nonfinite"));
                        }
                        let value = i64::from(floor_to_i32(native));
                        if maximum.checked_add(value).is_none_or(|sum| sum > i64::from(i32::MAX)) {
                            return Err(unsupported("a native note gauge increment can wrap addition"));
                        }
                    }
                }
                hits_from = note.hits;
            }
            let pending = transcript
                .pending
                .get(pending_from..frame.pending)
                .ok_or_else(|| unsupported("invalid native pending offsets"))?;
            if pending.iter().any(|&(range, _)| active != Some(range)) {
                return Err(unsupported("a pending draw can observe the inactive or another range's chain"));
            }
            if frame.finish {
                if frame.repeat != 1 || active.is_none() {
                    return Err(unsupported("a finish does not close one active native Luck chain"));
                }
                active = None;
            }
            actions_from = frame.actions;
            notes_from = frame.notes;
            pending_from = frame.pending;
        }
        if actions_from != transcript.actions.len()
            || notes_from != transcript.notes.len()
            || hits_from != transcript.hits.len()
            || pending_from != transcript.pending.len()
        {
            return Err(unsupported("an original action, note, hit or pending draw is unframed"));
        }
        Ok(())
    }

    /// The original tape is not rewritten. Every non-Miss action retains its relative position;
    /// one placeholder per nonempty Miss group identifies the still-present once/reset behavior.
    /// Its zero value is an encoding marker, never an instruction used for propagation.
    pub(super) fn identity_words(&self, transcript: &Transcript<ProbabilityMass>) -> Option<Vec<u64>> {
        if view_bound(transcript, self.maxima.len())? > MAX_VIEW_BYTES {
            return None;
        }
        let mut frames = reserved(transcript.frames.len())?;
        frames.extend_from_slice(&transcript.frames);
        let mut actions = reserved(transcript.actions.len())?;
        let mut blocks = reserved(frames.len().checked_mul(self.maxima.len().checked_add(2)?)?)?;
        let mut increments = reserved(self.maxima.len())?;
        increments.resize(self.maxima.len(), 0i64);
        let workspace = frames
            .capacity()
            .checked_mul(size_of::<Frame>())?
            .checked_add(actions.capacity().checked_mul(size_of::<Action<ProbabilityMass>>())?)?
            .checked_add(blocks.capacity().checked_mul(size_of::<u64>())?)?
            .checked_add(increments.capacity().checked_mul(size_of::<i64>())?)?;
        if workspace > MAX_VIEW_BYTES {
            return None;
        }
        let mut begin = 0;
        let mut count = 0;
        for (frame_index, frame) in frames.iter_mut().enumerate() {
            let end = frame.actions;
            let original = transcript.actions.get(begin..end)?;
            increments.fill(0);
            let mut miss = false;
            for &action in original {
                if let Action::MissGauge { value } = action {
                    miss = true;
                    for (&maximum, increment) in self.maxima.iter().zip(&mut increments) {
                        *increment = increment.checked_add(delta(maximum, value)?)?;
                    }
                } else {
                    actions.push(action);
                }
            }
            if miss {
                // Keep this marker even when every separately rounded increment is zero. The
                // original miss_rows flag and subsequent miss_used commit remain exact inputs.
                blocks.extend([frame_index as u64, actions.len() as u64]);
                blocks.extend(increments.iter().map(|&value| value as u64));
                actions.push(Action::MissGauge { value: 0 });
                count += 1;
            }
            frame.actions = actions.len();
            begin = end;
        }
        if begin != transcript.actions.len() {
            return None;
        }
        let mut words = transcript.key_with_frames_actions(&frames, &actions)?;
        words.extend([DOMAIN, self.maxima.len() as u64]);
        words.extend(self.maxima.iter().map(|&maximum| maximum as u64));
        words.push(count);
        words.extend(blocks);
        Some(words)
    }

    pub(super) fn allocated_bytes(&self) -> Option<usize> {
        size_of::<Self>().checked_add(self.maxima.capacity().checked_mul(size_of::<i64>())?)
    }
}

/// Transport a marker owned by another admitted operator, such as a conditioned minimum slot, into
/// the canonical identity's action offsets. A Miss input slot maps to its shared group marker.
pub(super) fn action_slot(transcript: &Transcript<ProbabilityMass>, slot: usize) -> Option<usize> {
    transcript.actions.get(slot)?;
    let (mut begin, mut canonical) = (0usize, 0usize);
    for frame in &transcript.frames {
        let original = transcript.actions.get(begin..frame.actions)?;
        let retained = original.iter().filter(|action| !matches!(action, Action::MissGauge { .. })).count();
        if slot < frame.actions {
            return if matches!(transcript.actions[slot], Action::MissGauge { .. }) {
                canonical.checked_add(retained)
            } else {
                canonical.checked_add(
                    transcript.actions[begin..slot]
                        .iter()
                        .filter(|action| !matches!(action, Action::MissGauge { .. }))
                        .count(),
                )
            };
        }
        canonical = canonical.checked_add(retained.checked_add(usize::from(retained != original.len()))?)?;
        begin = frame.actions;
    }
    None
}

#[cfg(test)]
mod tests;
