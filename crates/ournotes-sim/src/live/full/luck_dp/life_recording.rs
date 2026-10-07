//! Complete deterministic life/judgement transcripts, independent of the reduced lottery machine.
//!
//! Admission and the initialized life-model identity come from the existing native life recorder; initial
//! power is projected only with the score recorder's exhaustive dependency certificate.
//! Every frame's two phase-life observations and every converted judgement are retained losslessly. Equal
//! consecutive phase values share one run; unchanged judgements are reconstructed from the exact input scope.
//! Only a completed successful reduced recording/propagation can publish its accompanying life transcript.

use super::*;
use std::mem::size_of;
use std::sync::Arc;

const MAX_BYTES: usize = 1 << 20;
const ARC_HEADER_BYTES: usize = 2 * size_of::<usize>();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Phase {
    frame: usize,
    values: [i32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Conversion {
    frame: usize,
    note: usize,
    judgement: i32,
}

pub(super) struct Tape {
    frames: usize,
    phases: Vec<Phase>,
    conversions: Vec<Conversion>,
}

impl Tape {
    fn allocated_bytes(&self) -> usize {
        ARC_HEADER_BYTES
            + size_of::<Self>()
            + self.phases.capacity() * size_of::<Phase>()
            + self.conversions.capacity() * size_of::<Conversion>()
    }

    pub(super) fn reader(&self) -> Reader<'_> {
        Reader { tape: self, frame: 0, phase: 0, conversion: 0 }
    }
}

pub(super) struct Reader<'a> {
    tape: &'a Tape,
    frame: usize,
    phase: usize,
    conversion: usize,
}

impl Reader<'_> {
    pub(super) fn next(&mut self, judged: &mut [gekisou::GkNote]) -> Result<[i32; 2], Error> {
        if self.frame >= self.tape.frames {
            return Err(Error::Input("life transcript frame exceeds its complete input".into()));
        }
        while self.tape.phases.get(self.phase + 1).is_some_and(|phase| phase.frame <= self.frame) {
            self.phase += 1;
        }
        let phase = self
            .tape
            .phases
            .get(self.phase)
            .filter(|phase| phase.frame <= self.frame)
            .ok_or_else(|| Error::Input("life transcript has no phase observation".into()))?;
        while let Some(conversion) =
            self.tape.conversions.get(self.conversion).filter(|value| value.frame == self.frame)
        {
            let note = judged
                .get_mut(conversion.note)
                .ok_or_else(|| Error::Input("life transcript judgement exceeds its complete input".into()))?;
            note.3 = conversion.judgement;
            self.conversion += 1;
        }
        self.frame += 1;
        Ok(phase.values)
    }
}

pub(super) struct Builder {
    tape: Tape,
    capacity: usize,
    valid: bool,
}

impl Builder {
    pub(super) fn new(capacity: usize) -> Self {
        Self { tape: Tape { frames: 0, phases: Vec::new(), conversions: Vec::new() }, capacity, valid: capacity > 0 }
    }

    fn reserve<T>(values: &mut Vec<T>, available: usize) -> bool {
        if values.len() < values.capacity() {
            return true;
        }
        let Some(required) = values.len().checked_add(1) else {
            return false;
        };
        let Some(limit) = available.checked_div(size_of::<T>()) else { return false };
        if required > limit {
            return false;
        }
        let capacity = values.capacity().saturating_mul(2).max(8).min(limit);
        if values.try_reserve_exact(capacity - values.len()).is_err() {
            return false;
        }
        values.capacity().saturating_mul(size_of::<T>()) <= available
    }

    pub(super) fn observe(&mut self, phase: [i32; 2], original: &[JudgedNote], judged: &[gekisou::GkNote]) {
        if !self.valid {
            return;
        }
        if original.len() != judged.len() {
            self.valid = false;
            return;
        }
        let base = ARC_HEADER_BYTES + size_of::<Tape>();
        if self.tape.phases.last().is_none_or(|previous| previous.values != phase) {
            let available =
                self.capacity.saturating_sub(base + self.tape.conversions.capacity() * size_of::<Conversion>());
            if !Self::reserve(&mut self.tape.phases, available) {
                self.valid = false;
                return;
            }
            self.tape.phases.push(Phase { frame: self.tape.frames, values: phase });
        }
        for (note, (original, judged)) in original.iter().zip(judged).enumerate() {
            if original.judgement == judged.3 {
                continue;
            }
            let available = self.capacity.saturating_sub(base + self.tape.phases.capacity() * size_of::<Phase>());
            if !Self::reserve(&mut self.tape.conversions, available) {
                self.valid = false;
                return;
            }
            self.tape.conversions.push(Conversion { frame: self.tape.frames, note, judgement: judged.3 });
        }
        self.tape.frames += 1;
    }

    pub(super) fn finish(self, frames: usize) -> Option<Arc<Tape>> {
        (self.valid && self.tape.frames == frames && self.tape.allocated_bytes() <= self.capacity)
            .then(|| Arc::new(self.tape))
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Stats {
    pub lookups: u64,
    pub hits: u64,
    pub declines: u64,
    pub peak_entries: usize,
    pub peak_bytes: usize,
}

#[derive(Default)]
pub(super) struct Cache {
    scope: Option<Arc<shared_recording::Scope>>,
    storage: recording_cache::Storage<Tape>,
    capacity: usize,
    pub stats: Stats,
}

impl Cache {
    fn value_bytes(&self) -> usize {
        let mut seen = [std::ptr::null(); 128];
        let mut count = 0;
        let mut bytes = 0usize;
        for value in self.storage.values() {
            let pointer = Arc::as_ptr(value);
            if !seen[..count].contains(&pointer) {
                let Some(place) = seen.get_mut(count) else { return usize::MAX };
                *place = pointer;
                count += 1;
                bytes = bytes.saturating_add(value.allocated_bytes());
            }
        }
        bytes
    }

    fn bytes(&self) -> usize {
        self.scope
            .as_ref()
            .map_or(0, |scope| scope.bytes())
            .saturating_add(self.storage.retained().1)
            .saturating_add(self.value_bytes())
    }

    fn clear(&mut self) {
        self.scope = None;
        self.storage = Default::default();
    }

    pub(super) fn limit(&mut self, capacity: usize) {
        let capacity = capacity.min(MAX_BYTES);
        if self.capacity == capacity {
            return;
        }
        self.capacity = capacity;
        if self.bytes() > self.capacity {
            self.clear();
        }
    }

    pub(super) fn get(&mut self, scope: &Arc<shared_recording::Scope>, identity: &[u8]) -> Option<Arc<Tape>> {
        self.stats.lookups += 1;
        let value = self
            .scope
            .as_ref()
            .filter(|old| shared_recording::Scope::same(old, scope))
            .and_then(|_| self.storage.get(identity))
            .cloned();
        self.stats.hits += u64::from(value.is_some());
        value
    }

    pub(super) fn insert(&mut self, scope: Arc<shared_recording::Scope>, identity: Vec<u8>, tape: Arc<Tape>) {
        if self.scope.as_ref().is_some_and(|old| shared_recording::Scope::same(old, &scope))
            && self.storage.get(&identity).is_some()
        {
            return;
        }
        let available = self.capacity.saturating_sub(scope.bytes());
        let tape_bytes = tape.allocated_bytes();
        if tape_bytes >= available || identity.len() >= available - tape_bytes {
            self.stats.declines += 1;
            return;
        }
        if !self.scope.as_ref().is_some_and(|old| shared_recording::Scope::same(old, &scope)) {
            self.clear();
            self.scope = Some(scope);
        }
        let mut values = self.value_bytes().saturating_add(tape_bytes);
        if values >= available {
            self.storage = Default::default();
            values = tape_bytes;
        }
        if !self.storage.insert(identity, tape, available - values) {
            self.stats.declines += 1;
        }
        debug_assert!(self.bytes() <= self.capacity);
        self.stats.peak_entries = self.stats.peak_entries.max(self.storage.retained().0);
        self.stats.peak_bytes = self.stats.peak_bytes.max(self.bytes());
    }

    #[cfg(test)]
    pub(super) fn retained(&self) -> (usize, usize) {
        (self.storage.retained().0, self.bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_frames_reconstruct_every_phase_and_conversion() {
        let mut builder = Builder::new(MAX_BYTES);
        let mut expected = Vec::new();
        for frame in 0..100usize {
            let phase = match frame / 20 {
                0 => [1000, 1000],
                1 => [400, 700],
                2 => [700, 700],
                3 => [i32::MIN, i32::MAX],
                _ => [0, 0],
            };
            let original: Vec<_> = (0..frame % 4)
                .map(|note| JudgedNote { note_id: note as i32, judgement: 5, judgement_time_ms: frame as i32 })
                .collect();
            let judged: Vec<_> = original
                .iter()
                .enumerate()
                .map(|(note, source)| (source.note_id, 1, frame as i32, if (frame + note) % 3 == 0 { 6 } else { 5 }))
                .collect();
            builder.observe(phase, &original, &judged);
            expected.push((phase, original, judged));
        }
        let tape = builder.finish(100).unwrap();
        assert_eq!(tape.phases.len(), 5);
        assert!(tape.allocated_bytes() <= MAX_BYTES);
        let mut reader = tape.reader();
        for (phase, original, expected) in expected {
            let mut judged: Vec<_> =
                original.iter().map(|source| (source.note_id, 1, source.judgement_time_ms, source.judgement)).collect();
            assert_eq!(reader.next(&mut judged).unwrap(), phase);
            assert_eq!(judged, expected);
        }
        assert!(reader.next(&mut []).is_err());
    }

    #[test]
    fn transcript_capacity_and_incomplete_frames_decline_optional_reuse() {
        for capacity in [0, 1, size_of::<Tape>(), MAX_BYTES] {
            let mut builder = Builder::new(capacity);
            builder.observe([1000, 1000], &[], &[]);
            let complete = builder.finish(1);
            assert_eq!(complete.is_some(), capacity == MAX_BYTES);
        }
        let mut builder = Builder::new(MAX_BYTES);
        builder.observe([1000, 1000], &[], &[]);
        assert!(builder.finish(2).is_none());
        let mut builder = Builder::new(MAX_BYTES);
        builder.observe([1000, 1000], &[JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 0 }], &[]);
        assert!(builder.finish(1).is_none());
    }

    #[test]
    fn changing_frames_and_conversions_use_bounded_amortized_storage() {
        let mut builder = Builder::new(MAX_BYTES);
        let mut previous = (0, 0);
        let mut growths = 0;
        for frame in 0..8192i32 {
            let original = [JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: frame }];
            builder.observe([frame, -frame], &original, &[(1, 1, frame, 6)]);
            assert!(builder.valid && builder.tape.allocated_bytes() <= MAX_BYTES);
            let capacities = (builder.tape.phases.capacity(), builder.tape.conversions.capacity());
            growths += usize::from(capacities != previous);
            previous = capacities;
        }
        let tape = builder.finish(8192).unwrap();
        assert_eq!((tape.phases.len(), tape.conversions.len()), (8192, 8192));
        assert!(growths <= 32, "a long changing history must not reallocate at every observation");
        let mut reader = tape.reader();
        for frame in 0..8192i32 {
            let mut judged = [(1, 1, frame, 5)];
            assert_eq!(reader.next(&mut judged).unwrap(), [frame, -frame]);
            assert_eq!(judged[0].3, 6);
        }
    }
}
