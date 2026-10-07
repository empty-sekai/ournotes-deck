//! Request-shared reuse of complete reduced recordings.
//!
//! An owned scope contains every external recording input and the complete remaining initialized reduced
//! model state. Only the reduced interpreter's initial score power is normalized: it never calculates score
//! and supplies literal zero to both controller phases. An optional life interpreter keeps its complete
//! initialized state. Its initial power can be projected out only after the existing exhaustive recorder
//! dependency certificate proves that no life/judgement input reads score; otherwise power remains exact.
//! A scope mismatch is a miss, and a different scope replaces the retained table only after completion.

use super::*;
use crate::num::FxHasher;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

const MAX_BYTES: usize = 1 << 20;
const ARC_HEADER_BYTES: usize = 2 * size_of::<usize>();
const LIFE_KEY_PREFIX: &[u8] = b"compiled-life-recording\0\x02";

pub(super) struct Scope {
    bytes: Box<[u8]>,
    hash: u64,
}

impl Scope {
    pub(super) fn bytes(&self) -> usize {
        ARC_HEADER_BYTES.saturating_add(size_of::<Self>()).saturating_add(self.bytes.len())
    }

    pub(super) fn same(a: &Arc<Self>, b: &Arc<Self>) -> bool {
        Arc::ptr_eq(a, b) || (a.hash == b.hash && a.bytes == b.bytes)
    }
}

/// A RecordingCache belongs to one immutable LuckScoreSession. A failed optional scope construction is
/// remembered at its byte allowance; changing that allowance permits a new attempt.
/// This memo relies on the current reduced constructor: all deck-dependent initialized fields are in the
/// original raw key. A future deck-dependent residual field must extend that key or disable the memo;
/// inclusion in a freshly formatted Debug identity alone does not prove order independence of a cached scope.
pub(super) struct ScopeMemo {
    capacity: usize,
    value: Option<Arc<Scope>>,
}

#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    pub notes: &'a [LiveNote],
    pub events: &'a [(i32, i32)],
    pub params: LiveParams,
    pub setup: &'a GekisouSetup,
    pub play: &'a LivePlay,
    pub deltas: &'a [f32],
    pub ranking: Option<&'a [crate::replay::RankConfirmation]>,
}

struct Bytes {
    value: Vec<u8>,
    limit: usize,
}

impl Bytes {
    fn append(&mut self, value: &[u8]) -> Option<()> {
        let length = self.value.len().checked_add(value.len())?;
        if length > self.limit {
            return None;
        }
        self.value.try_reserve_exact(value.len()).ok()?;
        if self.value.capacity() > self.limit {
            return None;
        }
        self.value.extend_from_slice(value);
        Some(())
    }

    fn len(&mut self, value: usize) -> Option<()> {
        self.append(&u64::try_from(value).ok()?.to_le_bytes())
    }

    fn blob(&mut self, value: &[u8]) -> Option<()> {
        self.len(value.len())?;
        self.append(value)
    }
}

/// Preserve all initialized fields not already in the original recording key or the exact chart scope.
/// The complete model Debug view automatically retains future fields; its bounded identity also sorts
/// randomized score lookup tables and declines NaN/opaque views. Every removed field is restored before return.
fn residual_identity(model: &mut LiveModel) -> Option<String> {
    let power = std::mem::replace(&mut model.score.calc.state.band_total_power, 0);
    let notes = std::mem::take(&mut model.notes);
    let events = std::mem::take(&mut model.events);
    let rows = std::mem::take(&mut model.rows);
    let cond = std::mem::take(&mut model.cond);
    let appliers = std::mem::take(&mut model.gk_appliers);
    let gk = model.gk.take();
    let value = super::super::luck_exact::initialized_identity(model);
    model.gk = gk;
    model.gk_appliers = appliers;
    model.cond = cond;
    model.rows = rows;
    model.events = events;
    model.notes = notes;
    model.score.calc.state.band_total_power = power;
    value
}

/// The life interpreter is a deterministic state machine admitted by `life_recorder`. Equal complete
/// initialized states and the exact shared input stream therefore produce the same life/judgement history.
/// The score recorder's exhaustive dependency certificate additionally permits initial power projection:
/// score values and rank bonuses may change, but no admitted life/conversion/lifecycle reader consumes them.
/// Solo ranks are fixed; external rank arrivals remain exact inputs. Without this certificate power stays.
/// Notes/events have complete scope encodings; every other field, owner and mutable checker remains here.
pub(super) fn life_key(prepared: &mut PreparedRecording<ProbabilityMass>, skills: &LuckSkills) -> Option<Vec<u8>> {
    let life = prepared.life.as_mut()?;
    let power_independent = super::super::luck_score_bounds::check_recorder(life, skills).is_ok();
    let power = power_independent.then(|| std::mem::replace(&mut life.score.calc.state.band_total_power, 0));
    let notes = std::mem::take(&mut life.notes);
    let events = std::mem::take(&mut life.events);
    let identity = super::super::luck_exact::initialized_identity(life);
    life.notes = notes;
    life.events = events;
    if let Some(power) = power {
        life.score.calc.state.band_total_power = power;
    }
    let identity = identity?;
    // Performer input has already been fully compiled. The complete life model above identifies its
    // effects; retaining an unconsumed performer field would unnecessarily split equal native states.
    let life_deck = prepared.life_deck.take();
    let reduced = RecordingCache::key(prepared);
    prepared.life_deck = life_deck;
    let bytes = LIFE_KEY_PREFIX
        .len()
        .checked_add(8)?
        .checked_add(reduced.len())?
        .checked_add(1)?
        .checked_add(identity.len())?;
    if bytes > MAX_BYTES {
        return None;
    }
    let mut key = Vec::new();
    key.try_reserve_exact(bytes).ok()?;
    if key.capacity() > MAX_BYTES {
        return None;
    }
    key.extend_from_slice(LIFE_KEY_PREFIX);
    key.extend_from_slice(&(reduced.len() as u64).to_le_bytes());
    key.extend_from_slice(&reduced);
    // The proof mode is part of equality, so a declined power certificate never aliases an admitted one.
    key.push(u8::from(power_independent));
    key.extend_from_slice(identity.as_bytes());
    Some(key)
}

/// The independent life interpreter's identity and power-proof mode, without the reduced lottery rows.
pub(super) fn life_identity(key: &[u8]) -> Option<&[u8]> {
    let body = key.strip_prefix(LIFE_KEY_PREFIX)?;
    let length = usize::try_from(u64::from_le_bytes(body.get(..8)?.try_into().ok()?)).ok()?;
    body.get(8usize.checked_add(length)?..)
}

/// Build once per immutable session. Public direct curve calls have no session memo and own a fresh scope.
/// None declines optional reuse. Cancellation is separate so callers cannot turn it into an uncached success.
fn scope(
    prepared: &mut PreparedRecording<ProbabilityMass>,
    context: Context<'_>,
    capacity: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Arc<Scope>>, ()> {
    if cancelled() {
        return Err(());
    }
    let Some(identity) = residual_identity(&mut prepared.model) else { return Ok(None) };
    if cancelled() {
        return Err(());
    }
    // Every loop below has fixed-width elements. Reserve one conservative encoded size so scope construction
    // does not repeatedly reallocate a growing chart buffer. Overflow or an oversized context declines reuse.
    let limit = capacity.min(MAX_BYTES).saturating_sub(ARC_HEADER_BYTES + size_of::<Scope>());
    let estimate = (|| {
        let mut size = identity.len().checked_add(256)?;
        for (count, width) in [
            (context.notes.len(), 16usize),
            (context.events.len(), 8),
            (context.setup.fevers.len(), 8),
            (context.setup.missions.len(), 8),
            (context.play.frames.len(), 12),
            (context.deltas.len(), 4),
            (context.ranking.map_or(0, |values| values.len()), 28),
        ] {
            size = size.checked_add(count.checked_mul(width)?)?;
        }
        if size > limit {
            return None;
        }
        for frame in &context.play.frames {
            size = size.checked_add(frame.judged.len().checked_mul(12)?)?;
        }
        (size <= limit).then_some(size)
    })();
    let Some(estimate) = estimate else { return Ok(None) };
    let mut value = Vec::new();
    if value.try_reserve_exact(estimate).is_err() || value.capacity() > limit {
        return Ok(None);
    }
    let mut out = Bytes { value, limit };
    let mut stopped = false;
    let built = (|| {
        out.append(b"complete-recording-scope\0\x02")?;
        out.blob(identity.as_bytes())?;
        let Context { notes, events, params, setup, play, deltas, ranking } = context;
        let LiveParams {
            skill_target_music_type,
            total_power: _,
            music_level,
            converted_note_count,
            music_length_ms,
            score_music_length_ms,
            assist_factor,
        } = params;
        out.append(&skill_target_music_type.to_le_bytes())?;
        out.append(&music_level.to_le_bytes())?;
        out.append(&converted_note_count.to_le_bytes())?;
        out.append(&music_length_ms.to_le_bytes())?;
        match score_music_length_ms {
            None => out.append(&[0])?,
            Some(value) => {
                out.append(&[1])?;
                out.append(&value.to_le_bytes())?;
            }
        }
        out.append(&assist_factor.to_bits().to_le_bytes())?;
        out.len(notes.len())?;
        for (index, LiveNote { note_id, time_ms, note_operate_type, judgement_type }) in notes.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                stopped = true;
                return None;
            }
            out.append(&note_id.to_le_bytes())?;
            out.append(&time_ms.to_le_bytes())?;
            out.append(&note_operate_type.to_le_bytes())?;
            out.append(&judgement_type.to_le_bytes())?;
        }
        out.len(events.len())?;
        for (index, (kind, time)) in events.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                stopped = true;
                return None;
            }
            out.append(&kind.to_le_bytes())?;
            out.append(&time.to_le_bytes())?;
        }
        let GekisouSetup { fevers, missions } = setup;
        out.len(fevers.len())?;
        for (begin, end) in fevers {
            out.append(&begin.to_le_bytes())?;
            out.append(&end.to_le_bytes())?;
        }
        out.len(missions.len())?;
        for mission in missions {
            out.append(&mission.to_le_bytes())?;
        }
        let LivePlay { frames, base_seed } = play;
        out.append(&base_seed.to_le_bytes())?;
        out.len(frames.len())?;
        for (index, PlayFrame { time_ms, judged }) in frames.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                stopped = true;
                return None;
            }
            out.append(&time_ms.to_le_bytes())?;
            out.len(judged.len())?;
            for (index, JudgedNote { note_id, judgement, judgement_time_ms }) in judged.iter().enumerate() {
                if index.is_multiple_of(64) && cancelled() {
                    stopped = true;
                    return None;
                }
                out.append(&note_id.to_le_bytes())?;
                out.append(&judgement.to_le_bytes())?;
                out.append(&judgement_time_ms.to_le_bytes())?;
            }
        }
        out.len(deltas.len())?;
        for (index, delta) in deltas.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                stopped = true;
                return None;
            }
            out.append(&delta.to_bits().to_le_bytes())?;
        }
        match ranking {
            None => out.append(&[0])?,
            Some(ranking) => {
                out.append(&[1])?;
                out.len(ranking.len())?;
                for crate::replay::RankConfirmation { range, frame, rank, percent } in ranking {
                    out.len(*range)?;
                    out.len(*frame)?;
                    out.append(&rank.to_le_bytes())?;
                    out.append(&percent.to_le_bytes())?;
                }
            }
        }
        Some(())
    })();
    if stopped || cancelled() {
        return Err(());
    }
    if built.is_none() {
        return Ok(None);
    }
    let mut hash = FxHasher::default();
    out.value.hash(&mut hash);
    Ok(Some(Arc::new(Scope { bytes: out.value.into_boxed_slice(), hash: hash.finish() })))
}

#[derive(Clone, Copy, Default)]
pub(super) struct Stats {
    pub lookups: u64,
    pub hits: u64,
    pub scope_builds: u64,
    pub scope_bytes: u64,
    pub scope_declines: u64,
    pub scope_ms: f64,
    pub key_declines: u64,
    pub capacity_declines: u64,
    pub peak_entries: usize,
    pub peak_bytes: usize,
}

/// Its own allowance is separate from the unchanged session-local recording table. It includes the retained
/// owned scope, exact raw-key representation, entry-buffer capacity and each distinct retained curve allocation.
/// Arc sharing never duplicates a curve's Vec storage. Allocator metadata and caller-owned inputs are not RSS.
#[derive(Default)]
pub(super) struct SharedRecordings {
    scope: Option<Arc<Scope>>,
    storage: recording_cache::Storage<LuckDpCertifiedResult>,
    capacity: usize,
    pub stats: Stats,
}

fn curve_bytes(curve: &LuckDpCertifiedResult) -> usize {
    ARC_HEADER_BYTES
        .saturating_add(size_of::<LuckDpCertifiedResult>())
        .saturating_add(curve.steps.capacity().saturating_mul(size_of::<(i32, [ProbabilityMass; 4])>()))
        .saturating_add(curve.probes.capacity().saturating_mul(size_of::<bool>()))
}

impl SharedRecordings {
    fn clear(&mut self) {
        self.scope = None;
        self.storage = Default::default();
    }

    fn value_bytes(&self) -> usize {
        // Storage admits at most 128 entries. This stack list counts shared allocations once without
        // retaining another set of Arc handles or allocating on a lookup.
        let mut seen = [std::ptr::null(); 128];
        let mut count = 0usize;
        let mut bytes = 0usize;
        for value in self.storage.values() {
            let pointer = Arc::as_ptr(value);
            if !seen[..count].contains(&pointer) {
                let Some(place) = seen.get_mut(count) else { return usize::MAX };
                *place = pointer;
                count += 1;
                bytes = bytes.saturating_add(curve_bytes(value));
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

    pub(super) fn limit(&mut self, capacity: usize) {
        let capacity = capacity.min(MAX_BYTES);
        if self.capacity == capacity {
            return;
        }
        self.capacity = capacity;
        // Scope changes and capacity changes are rare. Dropping optional reuse preserves the exact result.
        if self.bytes() > capacity {
            self.clear();
        }
    }

    pub(super) fn prepare_scope(
        &mut self,
        memo: Option<&mut Option<ScopeMemo>>,
        prepared: &mut PreparedRecording<ProbabilityMass>,
        context: Context<'_>,
        raw: &[u8],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<Arc<Scope>>, ()> {
        if cancelled() {
            return Err(());
        }
        if self.capacity == 0 {
            return Ok(None);
        }
        if raw.len() > self.capacity
            || raw.windows(3).any(|part| part == b"NaN")
            || raw.windows(10).any(|part| part == b"<borrowed>")
        {
            self.stats.key_declines += 1;
            return Ok(None);
        }
        if let Some(value) = memo.as_ref().and_then(|memo| memo.as_ref())
            && value.capacity == self.capacity
        {
            return Ok(value.value.clone());
        }
        self.stats.scope_builds += 1;
        #[cfg(feature = "search-diagnostics")]
        let started = std::time::Instant::now();
        let result = scope(prepared, context, self.capacity, cancelled);
        #[cfg(feature = "search-diagnostics")]
        {
            self.stats.scope_ms += started.elapsed().as_secs_f64() * 1e3;
        }
        let value = result?;
        self.stats.scope_declines += u64::from(value.is_none());
        if let Some(scope) = &value {
            self.stats.scope_bytes = self.stats.scope_bytes.saturating_add(scope.bytes.len() as u64);
        }
        if let Some(memo) = memo {
            *memo = Some(ScopeMemo { capacity: self.capacity, value: value.clone() });
        }
        Ok(value)
    }

    pub(super) fn get(
        &mut self,
        scope: &mut Arc<Scope>,
        raw: &[u8],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<Arc<LuckDpCertifiedResult>>, ()> {
        if cancelled() {
            return Err(());
        }
        self.stats.lookups += 1;
        let value = self.scope.as_ref().filter(|retained| Scope::same(retained, scope)).and_then(|retained| {
            // Full owned-byte equality authorizes this identity shortcut for the session's later orders.
            *scope = Arc::clone(retained);
            self.storage.get(raw).cloned()
        });
        if cancelled() {
            return Err(());
        }
        self.stats.hits += u64::from(value.is_some());
        Ok(value)
    }

    pub(super) fn remember_scope(memo: Option<&mut Option<ScopeMemo>>, scope: &Arc<Scope>) {
        if let Some(Some(memo)) = memo {
            memo.value = Some(Arc::clone(scope));
        }
    }

    pub(super) fn insert(&mut self, scope: Arc<Scope>, raw: Vec<u8>, value: Arc<LuckDpCertifiedResult>) {
        if self.scope.as_ref().is_some_and(|old| Scope::same(old, &scope)) && self.storage.get(&raw).is_some() {
            return;
        }
        let Some(available) = self.capacity.checked_sub(scope.bytes()) else {
            self.stats.capacity_declines += 1;
            return;
        };
        let new_bytes = curve_bytes(&value);
        if new_bytes >= available {
            self.stats.capacity_declines += 1;
            return;
        }
        if !self.scope.as_ref().is_some_and(|old| Scope::same(old, &scope)) {
            // Caller reaches this method only for a complete recording and completed certified propagation.
            self.clear();
            self.scope = Some(scope);
        }
        let duplicate_value = self.storage.values().any(|old| Arc::ptr_eq(old, &value));
        let mut values = self.value_bytes().saturating_add(if duplicate_value { 0 } else { new_bytes });
        if values >= available {
            self.storage = Default::default();
            values = new_bytes;
        }
        if !self.storage.insert(raw, value, available - values) {
            self.stats.capacity_declines += 1;
        }
        let bytes = self.bytes();
        debug_assert!(bytes <= self.capacity);
        self.stats.peak_entries = self.stats.peak_entries.max(self.storage.retained().0);
        self.stats.peak_bytes = self.stats.peak_bytes.max(bytes);
    }

    #[cfg(test)]
    pub(super) fn retained(&self) -> (usize, usize) {
        (self.storage.retained().0, self.bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_scope(bytes: &[u8], hash: u64) -> Arc<Scope> {
        Arc::new(Scope { bytes: bytes.to_vec().into_boxed_slice(), hash })
    }

    fn curve() -> Arc<LuckDpCertifiedResult> {
        Arc::new(LuckDpCertifiedResult {
            steps: vec![(
                40,
                [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO],
            )],
            probes: vec![true, false],
            peak_states: 1,
            transitions: 7,
        })
    }

    #[test]
    fn shared_recording_scope_collision_requires_full_owned_bytes() {
        let mut cache = SharedRecordings::default();
        cache.limit(MAX_BYTES);
        let first = test_scope(b"chart-a", 1);
        let value = curve();
        cache.insert(first.clone(), b"raw-a".to_vec(), value.clone());
        let mut collision = test_scope(b"chart-b", 1);
        assert!(cache.get(&mut collision, b"raw-a", &mut || false).unwrap().is_none());
        let mut equal = test_scope(b"chart-a", 1);
        let found = cache.get(&mut equal, b"raw-a", &mut || false).unwrap().unwrap();
        assert!(Arc::ptr_eq(&found, &value));
        assert!(Arc::ptr_eq(&equal, &first), "exact comparison interns the owned scope");
        cache.insert(collision.clone(), b"raw-a".to_vec(), curve());
        assert!(cache.get(&mut equal, b"raw-a", &mut || false).unwrap().is_none());
        assert!(cache.get(&mut collision, b"raw-a", &mut || false).unwrap().is_some());
    }

    #[test]
    fn shared_recording_storage_counts_scope_and_distinct_curve_allocations() {
        let mut cache = SharedRecordings::default();
        cache.limit(MAX_BYTES);
        let scope = test_scope(b"owned-chart", 1);
        let first = curve();
        for index in 0u32..130 {
            cache.insert(scope.clone(), index.to_le_bytes().to_vec(), first.clone());
        }
        assert_eq!(cache.retained().0, 128);
        assert_eq!(cache.value_bytes(), curve_bytes(&first));
        assert_eq!(cache.retained().1, scope.bytes() + cache.storage.retained().1 + curve_bytes(&first));
        assert!(cache.retained().1 <= MAX_BYTES);
        assert!(cache.storage.get(&0u32.to_le_bytes()).is_none());
        let before = cache.retained();
        let second = curve();
        cache.insert(scope.clone(), 129u32.to_le_bytes().to_vec(), second.clone());
        assert_eq!(cache.retained(), before, "duplicate insertion retains its original allocation and accounting");
        assert!(Arc::ptr_eq(cache.storage.get(&129u32.to_le_bytes()).unwrap(), &first));
        cache.insert(scope, b"new-certificate".to_vec(), second.clone());
        assert_eq!(cache.value_bytes(), curve_bytes(&first) + curve_bytes(&second));
        assert!(cache.stats.peak_bytes <= MAX_BYTES);
        cache.limit(0);
        assert_eq!(cache.retained(), (0, 0));
    }

    #[test]
    fn shared_recording_capacity_and_cancelled_hits_fail_closed() {
        let mut cache = SharedRecordings::default();
        cache.limit(MAX_BYTES);
        let mut scope = test_scope(b"owned-chart", 1);
        cache.insert(scope.clone(), b"key".to_vec(), curve());
        let before = cache.stats;
        let mut checks = 0;
        assert!(
            cache
                .get(&mut scope, b"key", &mut || {
                    checks += 1;
                    checks == 2
                })
                .is_err()
        );
        assert_eq!(cache.stats.hits, before.hits);
        assert!(cache.get(&mut scope, b"key", &mut || false).unwrap().is_some());
        cache.limit(64);
        assert_eq!(cache.retained(), (0, 0));
        let declined = cache.stats.capacity_declines;
        cache.insert(scope, b"key".to_vec(), curve());
        assert_eq!(cache.stats.capacity_declines, declined + 1);
        assert_eq!(cache.retained(), (0, 0));
    }
}
