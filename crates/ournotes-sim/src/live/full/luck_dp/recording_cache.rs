//! Lossless storage of complete recording identities inside one immutable score session.
//!
//! The first admitted complete key supplies a byte dictionary. Every later key keeps its full length
//! and either its complete bytes or all differing byte runs. This is a representation change only:
//! lookup still compares the complete original key, with a hash used solely as a prefilter.

use crate::num::FxHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 1 << 20;

fn raw_hash(key: &[u8]) -> u64 {
    let mut hash = FxHasher::default();
    key.hash(&mut hash);
    hash.finish()
}

/// Each omitted position equals the dictionary at that same position. A changed length is retained
/// independently, so insertions, deletions, arbitrary bytes and signed-zero text need no special case.
enum Key {
    Raw(Box<[u8]>),
    Delta { len: u32, runs: Box<[u8]> },
}

fn differences<'a>(base: &'a [u8], key: &'a [u8]) -> impl Iterator<Item = Range<usize>> + 'a {
    let mut cursor = 0;
    std::iter::from_fn(move || {
        while cursor < key.len() && base.get(cursor) == Some(&key[cursor]) {
            cursor += 1;
        }
        if cursor == key.len() {
            return None;
        }
        let start = cursor;
        while cursor < key.len() && base.get(cursor) != Some(&key[cursor]) {
            cursor += 1;
        }
        Some(start..cursor)
    })
}

fn add_run_size(bytes: usize, run: usize) -> Option<usize> {
    bytes.checked_add(8)?.checked_add(run)
}

impl Key {
    fn encode(base: &[u8], key: Vec<u8>, capacity: usize) -> Option<Self> {
        if key.len() > capacity {
            return None;
        }
        let len = u32::try_from(key.len()).ok()?;
        let mut bytes = 0usize;
        for run in differences(base, &key) {
            bytes = add_run_size(bytes, run.len())?;
            if bytes >= key.len() {
                break;
            }
        }
        if bytes >= key.len() {
            return Some(Self::Raw(key.into_boxed_slice()));
        }
        let mut runs = Vec::new();
        runs.try_reserve_exact(bytes).ok()?;
        for run in differences(base, &key) {
            runs.extend_from_slice(&u32::try_from(run.start).ok()?.to_le_bytes());
            runs.extend_from_slice(&u32::try_from(run.len()).ok()?.to_le_bytes());
            runs.extend_from_slice(&key[run]);
        }
        debug_assert_eq!(runs.len(), bytes);
        Some(Self::Delta { len, runs: runs.into_boxed_slice() })
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Raw(bytes) => bytes.len(),
            Self::Delta { runs, .. } => runs.len(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Raw(bytes) => bytes.len(),
            Self::Delta { len, .. } => *len as usize,
        }
    }

    /// Visit an exact reconstruction without allocating it. Run headers are fixed-width little-endian
    /// integers. Strict range checks also make a malformed internal encoding fail closed.
    fn visit(&self, base: &[u8], mut accept: impl FnMut(&[u8]) -> bool) -> Option<()> {
        match self {
            Self::Raw(bytes) => accept(bytes).then_some(()),
            Self::Delta { len, runs } => {
                let len = *len as usize;
                let mut encoded = &runs[..];
                let mut cursor = 0usize;
                while !encoded.is_empty() {
                    let header = encoded.get(..8)?;
                    let start = u32::from_le_bytes(header[..4].try_into().ok()?) as usize;
                    let count = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
                    let end = start.checked_add(count)?;
                    if count == 0 || start < cursor || end > len {
                        return None;
                    }
                    encoded = &encoded[8..];
                    let changed = encoded.get(..count)?;
                    let unchanged = if cursor == start { &[] } else { base.get(cursor..start)? };
                    if !accept(unchanged) || !accept(changed) {
                        return None;
                    }
                    encoded = &encoded[count..];
                    cursor = end;
                }
                let unchanged = if cursor == len { &[] } else { base.get(cursor..len)? };
                accept(unchanged).then_some(())
            }
        }
    }

    fn matches(&self, base: &[u8], raw: &[u8]) -> bool {
        if self.len() != raw.len() {
            return false;
        }
        let mut cursor = 0usize;
        self.visit(base, |part| {
            let Some(end) = cursor.checked_add(part.len()) else { return false };
            let same = raw.get(cursor..end) == Some(part);
            cursor = end;
            same
        })
        .is_some()
            && cursor == raw.len()
    }

    #[cfg(test)]
    fn decode(&self, base: &[u8]) -> Option<Vec<u8>> {
        let mut raw = Vec::new();
        self.visit(base, |part| {
            raw.extend_from_slice(part);
            true
        })?;
        (raw.len() == self.len()).then_some(raw)
    }
}

struct Entry<V> {
    hash: u64,
    key: Key,
    value: Arc<V>,
}

/// Keys and the fixed entry buffer share the same byte allowance. Completed certificates are shared by
/// Arc without copying their contents. As with the recording cache's uncompressed keys, this key-storage
/// allowance does not include the shared probability objects themselves.
pub(super) struct Storage<V> {
    dictionary: Option<Box<[u8]>>,
    entries: VecDeque<Entry<V>>,
    content_bytes: usize,
}

impl<V> Default for Storage<V> {
    fn default() -> Self {
        Self { dictionary: None, entries: VecDeque::new(), content_bytes: 0 }
    }
}

impl<V> Storage<V> {
    pub(super) fn get(&self, raw: &[u8]) -> Option<&Arc<V>> {
        self.find(raw_hash(raw), raw)
    }

    fn find(&self, hash: u64, raw: &[u8]) -> Option<&Arc<V>> {
        let base = self.dictionary.as_deref()?;
        self.entries.iter().find(|entry| entry.hash == hash && entry.key.matches(base, raw)).map(|entry| &entry.value)
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &Arc<V>> {
        self.entries.iter().map(|entry| &entry.value)
    }

    fn bytes(&self) -> usize {
        self.content_bytes.saturating_add(self.entries.capacity().saturating_mul(size_of::<Entry<V>>()))
    }

    pub(super) fn retained(&self) -> (usize, usize) {
        (self.entries.len(), self.bytes())
    }

    fn evict(&mut self) -> bool {
        let Some(entry) = self.entries.pop_front() else { return false };
        self.content_bytes -= entry.key.bytes();
        true
    }

    pub(super) fn limit(&mut self, capacity: usize) {
        let capacity = capacity.min(MAX_BYTES);
        while self.bytes() > capacity {
            if !self.evict() || self.entries.is_empty() {
                *self = Self::default();
            }
        }
    }

    /// Called only at the existing completed-recording insertion points. Refusal to retain a key has no
    /// effect on the computed result, and a duplicate leaves both its certificate and byte count intact.
    pub(super) fn insert(&mut self, raw: Vec<u8>, value: Arc<V>, capacity: usize) -> bool {
        let capacity = capacity.min(MAX_BYTES);
        self.limit(capacity);
        if capacity == 0 || raw.len() > capacity {
            return false;
        }
        let Ok(len) = u32::try_from(raw.len()) else { return false };
        let hash = raw_hash(&raw);
        if self.find(hash, &raw).is_some() {
            return true;
        }
        if self.dictionary.is_none() {
            let mut entries = VecDeque::new();
            if entries.try_reserve_exact(MAX_ENTRIES).is_err() {
                return false;
            }
            let Some(buffer_bytes) = entries.capacity().checked_mul(size_of::<Entry<V>>()) else { return false };
            if raw.len().checked_add(buffer_bytes).is_none_or(|bytes| bytes > capacity) {
                return false;
            }
            self.content_bytes = raw.len();
            self.dictionary = Some(raw.into_boxed_slice());
            entries.push_back(Entry { hash, key: Key::Delta { len, runs: Box::default() }, value });
            self.entries = entries;
            return true;
        }
        let base = self.dictionary.as_deref().expect("an occupied recording cache retains its dictionary");
        let Some(key) = Key::encode(base, raw, capacity) else { return false };
        let bytes = key.bytes();
        let buffer_bytes = self.entries.capacity().saturating_mul(size_of::<Entry<V>>());
        if base.len().checked_add(buffer_bytes).and_then(|n| n.checked_add(bytes)).is_none_or(|n| n > capacity) {
            return false;
        }
        while self.entries.len() >= MAX_ENTRIES || self.bytes().checked_add(bytes).is_none_or(|n| n > capacity) {
            // The independently retained dictionary survives even when its original entry is evicted.
            if !self.evict() {
                return false;
            }
        }
        let Some(content_bytes) = self.content_bytes.checked_add(bytes) else { return false };
        self.content_bytes = content_bytes;
        self.entries.push_back(Entry { hash, key, value });
        debug_assert!(self.entries.len() <= MAX_ENTRIES && self.bytes() <= capacity);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(max_len: usize) -> Vec<Vec<u8>> {
        let mut out = vec![Vec::new()];
        for _ in 0..max_len {
            let previous =
                out.iter().filter(|word| word.len() == out.last().unwrap().len()).cloned().collect::<Vec<_>>();
            for word in previous {
                for byte in [0, b'/', 255] {
                    let mut next = word.clone();
                    next.push(byte);
                    out.push(next);
                }
            }
        }
        out
    }

    #[test]
    fn encoding_reconstructs_every_short_binary_key_and_length_change() {
        let values = words(4);
        for base in &values {
            for raw in &values {
                let key = Key::encode(base, raw.clone(), MAX_BYTES).unwrap();
                assert_eq!(key.decode(base).as_ref(), Some(raw));
                assert!(key.matches(base, raw));
                let mut different = raw.clone();
                if let Some(byte) = different.first_mut() {
                    *byte ^= 1;
                } else {
                    different.push(0);
                }
                assert!(!key.matches(base, &different));
            }
        }
    }

    #[test]
    fn sparse_runs_preserve_insertions_deletions_suffixes_and_signed_zero() {
        let base = b"[state: +0.0, other: -0.0, repeated bytes repeated bytes repeated bytes]".to_vec();
        let mut cases = vec![base.clone(), base[..17].to_vec(), Vec::new()];
        for at in 0..=base.len() {
            let mut inserted = base.clone();
            inserted.splice(at..at, [0, 255, b'/', b']']);
            cases.push(inserted);
            if at < base.len() {
                let mut removed = base.clone();
                removed.remove(at);
                cases.push(removed);
            }
        }
        let mut zero = base.clone();
        zero[8] = b'-';
        cases.push(zero);
        for raw in cases {
            let key = Key::encode(&base, raw.clone(), MAX_BYTES).unwrap();
            assert_eq!(key.decode(&base), Some(raw.clone()));
            assert!(key.matches(&base, &raw));
        }
        assert!(matches!(Key::encode(&base, base.clone(), MAX_BYTES), Some(Key::Delta { .. })));
        assert!(matches!(Key::encode(&base, vec![255; base.len()], MAX_BYTES), Some(Key::Raw(_))));
    }

    #[test]
    fn invalid_delta_ranges_and_size_overflow_cannot_match() {
        let base = b"abcdefghijklmnop";
        let run = |start: u32, count: u32, bytes: &[u8]| {
            let mut out = Vec::new();
            out.extend_from_slice(&start.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(bytes);
            out
        };
        for encoded in [vec![1], run(0, 0, &[]), run(15, 2, b"xy"), run(3, 4, b"x"), {
            let mut out = run(2, 3, b"xyz");
            out.extend(run(3, 1, b"q"));
            out
        }] {
            let key = Key::Delta { len: base.len() as u32, runs: encoded.into_boxed_slice() };
            assert!(key.decode(base).is_none());
            assert!(!key.matches(base, base));
        }
        let missing_tail = Key::Delta { len: 20, runs: Box::default() };
        assert!(missing_tail.decode(base).is_none());
        assert!(add_run_size(usize::MAX, 1).is_none());
        assert!(add_run_size(1, usize::MAX).is_none());
        assert!(Key::encode(base, base.to_vec(), base.len() - 1).is_none());
    }

    fn family_key(id: u8) -> Vec<u8> {
        let mut key = vec![b'x'; 24_000];
        for at in [31, 5_100, 14_009, 23_001] {
            key[at] = id;
        }
        key
    }

    #[test]
    fn complete_order_family_fits_without_changing_raw_key_equality() {
        let mut cache = Storage::default();
        let values: Vec<_> = (0..120).map(Arc::new).collect();
        for (id, value) in values.iter().enumerate() {
            cache.insert(family_key(id as u8), Arc::clone(value), MAX_BYTES);
            assert!(cache.bytes() <= MAX_BYTES);
        }
        assert_eq!(cache.entries.len(), 120);
        assert!(120 * family_key(0).len() > MAX_BYTES);
        for (id, value) in values.iter().enumerate() {
            assert!(Arc::ptr_eq(cache.get(&family_key(id as u8)).unwrap(), value));
        }
        let before = cache.bytes();
        cache.insert(family_key(19), Arc::new(999), MAX_BYTES);
        assert_eq!(cache.bytes(), before);
        assert!(Arc::ptr_eq(cache.get(&family_key(19)).unwrap(), &values[19]));
        // A digest collision cannot make a different full key a hit.
        let missing = family_key(200);
        cache.entries[0].hash = raw_hash(&missing);
        assert!(cache.get(&missing).is_none());
    }

    #[test]
    fn fifo_eviction_keeps_dictionary_after_its_original_entry_is_gone() {
        let mut cache = Storage::default();
        for id in 0..128u8 {
            cache.insert(family_key(id), Arc::new(id), MAX_BYTES);
        }
        let dictionary = cache.dictionary.as_deref().unwrap().as_ptr();
        cache.insert(family_key(128), Arc::new(128), MAX_BYTES);
        assert_eq!(cache.entries.len(), 128);
        assert!(cache.get(&family_key(0)).is_none());
        assert_eq!(cache.dictionary.as_deref().unwrap().as_ptr(), dictionary);
        assert_eq!(cache.dictionary.as_deref(), Some(family_key(0).as_slice()));
        for id in 1..=128u8 {
            assert_eq!(cache.get(&family_key(id)).map(|v| **v), Some(id));
        }
    }

    #[test]
    fn byte_limits_release_buffers_and_refuse_oversized_keys() {
        let mut cache = Storage::default();
        cache.insert(family_key(0), Arc::new(0), MAX_BYTES);
        let base_bytes = cache.bytes();
        let one_delta = Key::encode(&family_key(0), family_key(1), MAX_BYTES).unwrap().bytes();
        let capacity = base_bytes + 2 * one_delta;
        for id in 1..=8u8 {
            cache.insert(family_key(id), Arc::new(id), capacity);
            assert!(cache.bytes() <= capacity);
        }
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.get(&family_key(7)).map(|v| **v), Some(7));
        assert_eq!(cache.get(&family_key(8)).map(|v| **v), Some(8));
        let held = cache.bytes();
        cache.insert(vec![0; capacity + 1], Arc::new(99), capacity);
        assert_eq!(cache.bytes(), held);
        cache.limit(base_bytes + one_delta);
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.bytes() <= base_bytes + one_delta);
        cache.limit(0);
        assert_eq!(cache.bytes(), 0);
        assert_eq!(cache.entries.capacity(), 0);
        assert!(cache.dictionary.is_none());
        cache.insert(family_key(1), Arc::new(1), 0);
        assert_eq!(cache.bytes(), 0);
        cache.insert(vec![1], Arc::new(1), 1);
        assert_eq!(cache.bytes(), 0, "entry buffer is charged to the same allowance");
    }
}
