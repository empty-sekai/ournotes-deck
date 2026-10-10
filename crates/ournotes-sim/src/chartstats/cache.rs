//! Persistent results of individual chart measurement programs. Keys describe compiled programs, not chart
//! documents or master versions. Floating-point interval endpoints are stored by their original bit patterns.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

use crate::Error;
use crate::live::certified::{F64Interval, ProbabilityMass};
use crate::live::full::{
    GekisouRange, IntegerBounds, LuckDpCertifiedResult, LuckRangeMoments, LuckRangeScoreBounds, LuckScoreExpectation,
    RealBounds,
};

const SCHEMA: &str = "ournotes-deck.chart-program-cache/1";
const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

/// Work performed through a chart program cache. `computed` counts successful computations, `writes` completed
/// atomic replacements, and `bytes` the bytes written. Failed computations are never stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartStatsCacheStats {
    pub requests: u64,
    pub hits: u64,
    pub computed: u64,
    pub writes: u64,
    pub invalid: u64,
    pub bytes: u64,
}

#[derive(Default)]
struct Counters {
    requests: AtomicU64,
    hits: AtomicU64,
    computed: AtomicU64,
    writes: AtomicU64,
    invalid: AtomicU64,
    bytes: AtomicU64,
}

/// A thread-safe, content-addressed cache of chart measurement programs. Each successful expectation,
/// deterministic run or lottery propagation is committed independently, so later failures preserve that work.
/// Input validation, current headers, kind/shape assembly and check-deck generation remain outside the cache.
pub struct ChartStatsCache {
    directory: PathBuf,
    counters: Counters,
    locks: [Mutex<()>; 192],
}

impl ChartStatsCache {
    /// Open a cache directory. This schema has no migration from whole-chart statistics caches. Model source
    /// identities occupy separate namespaces; identical compiled programs can be shared between master versions.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let directory = path.as_ref().join("chart-program-v1").join(crate::SOURCE_SHA256);
        fs::create_dir_all(&directory).map_err(|e| cache_error(&directory, e))?;
        Ok(Self { directory, counters: Counters::default(), locks: std::array::from_fn(|_| Mutex::new(())) })
    }

    pub fn snapshot(&self) -> ChartStatsCacheStats {
        let c = &self.counters;
        ChartStatsCacheStats {
            requests: c.requests.load(Ordering::Relaxed),
            hits: c.hits.load(Ordering::Relaxed),
            computed: c.computed.load(Ordering::Relaxed),
            writes: c.writes.load(Ordering::Relaxed),
            invalid: c.invalid.load(Ordering::Relaxed),
            bytes: c.bytes.load(Ordering::Relaxed),
        }
    }

    pub(super) fn expectation(
        &self,
        identity: Option<&[u8]>,
        ranges: usize,
        compute: impl FnOnce() -> Result<LuckScoreExpectation, Error>,
    ) -> Result<LuckScoreExpectation, Error> {
        self.evaluate("expectation", identity, |v| v.ranges.len() == ranges, compute)
    }

    pub(super) fn counted(
        &self,
        identity: Option<&[u8]>,
        ranges: usize,
        compute: impl FnOnce() -> Result<Counted, Error>,
    ) -> Result<Counted, Error> {
        self.evaluate("run", identity, |v| v.1.len() == ranges, compute)
    }

    pub(crate) fn curve(
        &self,
        identity: Option<&[u8]>,
        frames: usize,
        probes: usize,
        ranges: usize,
        compute: impl FnOnce() -> Result<LuckDpCertifiedResult, Error>,
    ) -> Result<LuckDpCertifiedResult, Error> {
        self.evaluate(
            "curve",
            identity,
            |v| {
                v.probe_transitions.len() == frames
                    && v.rush_transitions.len() == frames
                    && v.probes.len() == probes
                    && v.range_moments.len() == ranges
            },
            compute,
        )
    }

    fn evaluate<T: Record>(
        &self,
        kind: &str,
        identity: Option<&[u8]>,
        valid: impl Fn(&T) -> bool,
        compute: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        let Some(identity) = identity else {
            let result = compute()?;
            self.counters.computed.fetch_add(1, Ordering::Relaxed);
            return Ok(result);
        };
        let mut hash = Sha256::new();
        for part in
            [SCHEMA.as_bytes(), crate::SOURCE_SHA256.as_bytes(), super::FORMAT.as_bytes(), kind.as_bytes(), identity]
        {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        let hash = hash.finalize();
        let key = hex(&hash);
        let directory = self.directory.join(kind).join(&key[..2]);
        let path = directory.join(format!("{key}.json"));
        // Different threads asking for the same program share its first result. Independent processes may
        // duplicate computation, but use distinct temporary files and publish only complete records.
        // An expectation may request a curve while its own lock is held; separate banks keep those nested
        // operations independent even when their digest prefixes happen to agree.
        let bank = match kind {
            "expectation" => 0,
            "run" => 1,
            "curve" => 2,
            _ => unreachable!(),
        };
        let _lock = self.locks[bank * 64 + usize::from(hash[0]) % 64].lock().unwrap_or_else(|e| e.into_inner());
        if let Some(found) = self.read::<T>(&path, kind, &key) {
            if valid(&found) {
                self.counters.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(found);
            }
            self.counters.invalid.fetch_add(1, Ordering::Relaxed);
        }
        let result = compute()?;
        self.counters.computed.fetch_add(1, Ordering::Relaxed);
        if !valid(&result) {
            return Err(Error::Game("chart measurement returned an inconsistent cache record".into()));
        }
        let payload = serde_json::to_string(&result.stored()).map_err(|e| cache_error(&path, e))?;
        let envelope = Envelope {
            schema: SCHEMA.to_owned(),
            source_sha256: crate::SOURCE_SHA256.to_owned(),
            kind: kind.to_owned(),
            key,
            checksum: hex(&Sha256::digest(payload.as_bytes())),
            payload: RawValue::from_string(payload).map_err(|e| cache_error(&path, e))?,
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|e| cache_error(&path, e))?;
        // Oversized programs remain valid measurements; bound the memory needed to read a cache record.
        if bytes.len() as u64 <= MAX_RECORD_BYTES {
            fs::create_dir_all(&directory).map_err(|e| cache_error(&directory, e))?;
            write_atomic(&path, &bytes)?;
            self.counters.writes.fetch_add(1, Ordering::Relaxed);
            self.counters.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        Ok(result)
    }

    fn read<T: Record>(&self, path: &Path, kind: &str, key: &str) -> Option<T> {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => {
                self.counters.invalid.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };
        let read = || {
            if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
                return None;
            }
            let bytes = fs::read(path).ok()?;
            let envelope: Envelope = serde_json::from_slice(&bytes).ok()?;
            if envelope.schema != SCHEMA
                || envelope.source_sha256 != crate::SOURCE_SHA256
                || envelope.kind != kind
                || envelope.key != key
                || envelope.checksum != hex(&Sha256::digest(envelope.payload.get().as_bytes()))
            {
                return None;
            }
            T::restore(serde_json::from_str(envelope.payload.get()).ok()?)
        };
        let result = read();
        if result.is_none() {
            self.counters.invalid.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    schema: String,
    source_sha256: String,
    kind: String,
    key: String,
    checksum: String,
    payload: Box<RawValue>,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("writing a string");
    }
    out
}

fn cache_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::Input(format!("chart statistics cache {}: {error}", path.display()))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    write_atomic_with(path, bytes, || TEMPORARY_ID.fetch_add(1, Ordering::Relaxed))
}

fn write_atomic_with(path: &Path, bytes: &[u8], mut next: impl FnMut() -> u64) -> Result<(), Error> {
    let (temporary, mut file) = loop {
        let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), next()));
        match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(file) => break (temporary, file),
            // A process may have died before rename, and a later process may receive the same id. Its
            // unfinished file is neither a cache hit nor owned by this writer; try another unique name.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(cache_error(path, e)),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| cache_error(path, e))
}

trait Record: Sized {
    type Stored: Serialize + DeserializeOwned;
    fn stored(&self) -> Self::Stored;
    fn restore(stored: Self::Stored) -> Option<Self>;
}

// DTOs contain integers only: neither JSON float parsing nor output rounding may change an interval endpoint.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Interval([u64; 2]);

impl Interval {
    fn of(value: RealBounds) -> Self {
        Self([value.lower.to_bits(), value.upper.to_bits()])
    }

    fn bounds(self) -> Option<RealBounds> {
        let [lower, upper] = self.0.map(f64::from_bits);
        F64Interval::new(lower, upper).ok().map(RealBounds::from)
    }

    fn interval(self) -> Option<F64Interval> {
        let [lower, upper] = self.0.map(f64::from_bits);
        F64Interval::new(lower, upper).ok()
    }
}

fn integers([lower, upper]: [i32; 2]) -> Option<IntegerBounds> {
    (lower <= upper).then_some(IntegerBounds { lower, upper })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedRangeRecord {
    range: usize,
    start_query: Option<usize>,
    end_query: usize,
    percent: i64,
    mean: Interval,
    support: [i32; 2],
    bonus_mean: Interval,
    bonus_support: [i32; 2],
    luck_points_mean: Option<Interval>,
    lot_results_mean: Option<[Interval; 4]>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectationRecord {
    final_mean: Interval,
    final_support: [i32; 2],
    ranges: Vec<ExpectedRangeRecord>,
}

impl Record for LuckScoreExpectation {
    type Stored = ExpectationRecord;

    fn stored(&self) -> Self::Stored {
        let Self { final_mean, final_support, ranges } = self;
        ExpectationRecord {
            final_mean: Interval::of(*final_mean),
            final_support: [final_support.lower, final_support.upper],
            ranges: ranges
                .iter()
                .map(|r| {
                    let LuckRangeScoreBounds {
                        range,
                        start_query,
                        end_query,
                        percent,
                        mean,
                        support,
                        bonus_mean,
                        bonus_support,
                        luck_points_mean,
                        lot_results_mean,
                    } = r;
                    ExpectedRangeRecord {
                        range: *range,
                        start_query: *start_query,
                        end_query: *end_query,
                        percent: *percent,
                        mean: Interval::of(*mean),
                        support: [support.lower, support.upper],
                        bonus_mean: Interval::of(*bonus_mean),
                        bonus_support: [bonus_support.lower, bonus_support.upper],
                        luck_points_mean: luck_points_mean.map(Interval::of),
                        lot_results_mean: lot_results_mean.map(|values| values.map(Interval::of)),
                    }
                })
                .collect(),
        }
    }

    fn restore(stored: Self::Stored) -> Option<Self> {
        let mut ranges = Vec::with_capacity(stored.ranges.len());
        for (index, r) in stored.ranges.into_iter().enumerate() {
            if r.range != index {
                return None;
            }
            ranges.push(LuckRangeScoreBounds {
                range: r.range,
                start_query: r.start_query,
                end_query: r.end_query,
                percent: r.percent,
                mean: r.mean.bounds()?,
                support: integers(r.support)?,
                bonus_mean: r.bonus_mean.bounds()?,
                bonus_support: integers(r.bonus_support)?,
                luck_points_mean: Some(r.luck_points_mean?.bounds()?),
                lot_results_mean: Some(
                    r.lot_results_mean?
                        .into_iter()
                        .map(Interval::bounds)
                        .collect::<Option<Vec<_>>>()?
                        .try_into()
                        .ok()?,
                ),
            });
        }
        Some(Self { final_mean: stored.final_mean.bounds()?, final_support: integers(stored.final_support)?, ranges })
    }
}

type Counted = (i32, Vec<GekisouRange>, u64);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeRecord {
    mission: i64,
    state: u8,
    combo: i32,
    max_combo: i32,
    just_count: i32,
    start_score: i32,
    end_score: i32,
    luck_points: i32,
    luck_gauge: i32,
    rush_combo: i32,
    lot_results: [i32; 4],
    rank_bonus: Option<i32>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CountedRecord {
    score: i32,
    ranges: Vec<RangeRecord>,
    converted: u64,
}

impl Record for Counted {
    type Stored = CountedRecord;

    fn stored(&self) -> Self::Stored {
        CountedRecord {
            score: self.0,
            converted: self.2,
            ranges: self
                .1
                .iter()
                .map(|r| {
                    let GekisouRange {
                        mission,
                        state,
                        combo,
                        max_combo,
                        just_count,
                        start_score,
                        end_score,
                        luck_points,
                        luck_gauge,
                        rush_combo,
                        lot_results,
                        rank_bonus,
                    } = *r;
                    RangeRecord {
                        mission,
                        state,
                        combo,
                        max_combo,
                        just_count,
                        start_score,
                        end_score,
                        luck_points,
                        luck_gauge,
                        rush_combo,
                        lot_results,
                        rank_bonus,
                    }
                })
                .collect(),
        }
    }

    fn restore(stored: Self::Stored) -> Option<Self> {
        let ranges = stored
            .ranges
            .into_iter()
            .map(|r| {
                (1..=8).contains(&r.state).then_some(GekisouRange {
                    mission: r.mission,
                    state: r.state,
                    combo: r.combo,
                    max_combo: r.max_combo,
                    just_count: r.just_count,
                    start_score: r.start_score,
                    end_score: r.end_score,
                    luck_points: r.luck_points,
                    luck_gauge: r.luck_gauge,
                    rush_combo: r.rush_combo,
                    lot_results: r.lot_results,
                    rank_bonus: r.rank_bonus,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some((stored.score, ranges, stored.converted))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveRecord {
    probe_transitions: Vec<u8>,
    rush_transitions: Vec<u16>,
    steps: Vec<(i32, [Interval; 4])>,
    frame_queries: Vec<(i32, [[Interval; 4]; 3])>,
    probes: Vec<bool>,
    range_moments: Vec<(Interval, [Interval; 4])>,
    peak_states: usize,
    transitions: u64,
}

impl Record for LuckDpCertifiedResult {
    type Stored = CurveRecord;

    fn stored(&self) -> Self::Stored {
        let Self {
            probe_transitions,
            rush_transitions,
            steps,
            frame_queries,
            probes,
            range_moments,
            peak_states,
            transitions,
        } = self;
        let interval = |mass: [ProbabilityMass; 4]| mass.map(|m| Interval::of(m.interval().into()));
        CurveRecord {
            probe_transitions: probe_transitions.clone(),
            rush_transitions: rush_transitions.clone(),
            steps: steps.iter().map(|&(time, mass)| (time, interval(mass))).collect(),
            frame_queries: frame_queries.iter().map(|&(time, masses)| (time, masses.map(interval))).collect(),
            probes: probes.clone(),
            range_moments: range_moments
                .iter()
                .map(|m| (Interval::of(m.luck_points.into()), m.lot_results.map(|v| Interval::of(v.into()))))
                .collect(),
            peak_states: *peak_states,
            transitions: *transitions,
        }
    }

    fn restore(stored: Self::Stored) -> Option<Self> {
        if stored.probe_transitions.iter().any(|&v| v > 15) || stored.rush_transitions.iter().any(|&v| v >> 10 != 0) {
            return None;
        }
        let joint = |masses: [Interval; 4]| -> Option<[ProbabilityMass; 4]> {
            masses
                .into_iter()
                .map(|m| {
                    let bounds = m.bounds()?;
                    ProbabilityMass::from_bounds(bounds.lower, bounds.upper).ok()
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()
        };
        let steps =
            stored.steps.into_iter().map(|(time, masses)| Some((time, joint(masses)?))).collect::<Option<Vec<_>>>()?;
        let frame_queries = stored
            .frame_queries
            .into_iter()
            .map(|(time, [inside, after, before])| Some((time, [joint(inside)?, joint(after)?, joint(before)?])))
            .collect::<Option<Vec<_>>>()?;
        let range_moments = stored
            .range_moments
            .into_iter()
            .map(|(points, lots)| {
                Some(LuckRangeMoments {
                    luck_points: points.interval()?,
                    lot_results: lots
                        .into_iter()
                        .map(Interval::interval)
                        .collect::<Option<Vec<_>>>()?
                        .try_into()
                        .ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            probe_transitions: stored.probe_transitions,
            rush_transitions: stored.rush_transitions,
            steps,
            frame_queries,
            probes: stored.probes,
            range_moments,
            peak_states: stored.peak_states,
            transitions: stored.transitions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            Self(std::env::temp_dir().join(format!(
                "ournotes-chart-program-cache-{}-{stamp}-{}",
                std::process::id(),
                TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn record_path(cache: &ChartStatsCache, kind: &str) -> PathBuf {
        let shard = fs::read_dir(cache.directory.join(kind)).unwrap().next().unwrap().unwrap().path();
        fs::read_dir(shard).unwrap().next().unwrap().unwrap().path()
    }

    #[test]
    fn expectation_and_probability_endpoints_round_trip_every_bit() {
        let expected = LuckScoreExpectation {
            final_mean: RealBounds { lower: -0.0, upper: f64::from_bits(1) },
            final_support: IntegerBounds { lower: 0, upper: 1 },
            ranges: vec![LuckRangeScoreBounds {
                range: 0,
                start_query: Some(2),
                end_query: 5,
                percent: 100,
                mean: RealBounds { lower: 1.0, upper: 1.0f64.next_up() },
                support: IntegerBounds { lower: 1, upper: 2 },
                bonus_mean: RealBounds { lower: -1.0f64.next_up(), upper: 1.0 },
                bonus_support: IntegerBounds { lower: -2, upper: 1 },
                luck_points_mean: Some(RealBounds { lower: 0.0, upper: f64::from_bits(7) }),
                lot_results_mean: Some([RealBounds { lower: 0.25f64.next_down(), upper: 0.25f64.next_up() }; 4]),
            }],
        };
        let bytes = serde_json::to_vec(&expected.stored()).unwrap();
        let restored = LuckScoreExpectation::restore(serde_json::from_slice(&bytes).unwrap()).unwrap();
        assert_eq!(serde_json::to_vec(&restored.stored()).unwrap(), bytes);
        assert_eq!(restored.final_mean.lower.to_bits(), (-0.0f64).to_bits());

        let curve = LuckDpCertifiedResult {
            probe_transitions: vec![1, 3, 8],
            rush_transitions: vec![65, 0x302, 0x1aa],
            steps: vec![(12, [ProbabilityMass::from_bounds(0.25f64.next_down(), 0.25f64.next_up()).unwrap(); 4])],
            frame_queries: vec![(
                12,
                [[ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO]; 3],
            )],
            probes: vec![false, true],
            range_moments: vec![LuckRangeMoments {
                luck_points: F64Interval::new(1.0, 1.0f64.next_up()).unwrap(),
                lot_results: [F64Interval::new(-0.0, f64::from_bits(1)).unwrap(); 4],
            }],
            peak_states: 17,
            transitions: 100,
        };
        let bytes = serde_json::to_vec(&curve.stored()).unwrap();
        let restored = LuckDpCertifiedResult::restore(serde_json::from_slice(&bytes).unwrap()).unwrap();
        assert_eq!(serde_json::to_vec(&restored.stored()).unwrap(), bytes);
    }

    #[test]
    fn a_new_instance_reuses_completed_programs_and_preserves_failures() {
        let directory = Directory::new();
        let cache = ChartStatsCache::new(&directory.0).unwrap();
        cache.counted(Some(b"first"), 0, || Ok((12, Vec::new(), 3))).unwrap();
        let failure = Error::Domain("incomplete measurement".into());
        assert_eq!(cache.counted(Some(b"second"), 0, || Err(failure.clone())).unwrap_err(), failure);
        assert_eq!(cache.snapshot().writes, 1);
        let cache = ChartStatsCache::new(&directory.0).unwrap();
        assert_eq!(cache.counted(Some(b"first"), 0, || panic!("a completed run must be reused")).unwrap().0, 12);
        cache.counted(Some(b"second"), 0, || Ok((34, Vec::new(), 4))).unwrap();
        assert_eq!(cache.snapshot().hits, 1);
        assert_eq!(cache.snapshot().computed, 1);
        assert_eq!(cache.snapshot().writes, 1);
    }

    #[test]
    fn damaged_checksum_or_payload_is_recomputed_and_repaired() {
        let directory = Directory::new();
        let cache = ChartStatsCache::new(&directory.0).unwrap();
        cache.counted(Some(b"one"), 0, || Ok((12, Vec::new(), 3))).unwrap();
        let path = record_path(&cache, "run");
        let original = fs::read(&path).unwrap();
        for bad in [b"{truncated".to_vec(), b"[]".to_vec()] {
            fs::write(&path, bad).unwrap();
            assert_eq!(cache.counted(Some(b"one"), 0, || Ok((12, Vec::new(), 3))).unwrap().0, 12);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        let mut envelope: Envelope = serde_json::from_slice(&original).unwrap();
        envelope.payload = RawValue::from_string("{}".into()).unwrap();
        // A parseable payload is still invalid, both with a stale checksum and with a matching checksum.
        for repair_checksum in [false, true] {
            if repair_checksum {
                envelope.checksum = hex(&Sha256::digest(envelope.payload.get().as_bytes()));
            }
            fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
            cache.counted(Some(b"one"), 0, || Ok((12, Vec::new(), 3))).unwrap();
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        assert_eq!(cache.snapshot().invalid, 4);
        assert_eq!(cache.snapshot().computed, 5);
    }

    #[test]
    fn concurrent_requests_commit_one_complete_record() {
        let directory = Directory::new();
        let cache = ChartStatsCache::new(&directory.0).unwrap();
        let computed = AtomicU64::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let result = cache
                        .counted(Some(b"shared"), 0, || {
                            computed.fetch_add(1, Ordering::Relaxed);
                            Ok((123, Vec::new(), 9))
                        })
                        .unwrap();
                    assert_eq!(result.0, 123);
                });
            }
        });
        assert_eq!(computed.load(Ordering::Relaxed), 1);
        assert_eq!(cache.snapshot().hits, 7);
        assert_eq!(cache.snapshot().writes, 1);
        let _: Envelope = serde_json::from_slice(&fs::read(record_path(&cache, "run")).unwrap()).unwrap();
    }

    #[test]
    fn nested_curve_storage_does_not_hold_the_expectation_lock_bank() {
        let directory = Directory::new();
        let cache = ChartStatsCache::new(&directory.0).unwrap();
        let empty_curve = || {
            Ok(LuckDpCertifiedResult {
                probe_transitions: Vec::new(),
                rush_transitions: Vec::new(),
                steps: Vec::new(),
                frame_queries: Vec::new(),
                probes: Vec::new(),
                range_moments: Vec::new(),
                peak_states: 1,
                transitions: 0,
            })
        };
        let result = cache
            .expectation(Some(b"expectation"), 0, || {
                cache.curve(Some(b"curve"), 0, 0, 0, empty_curve)?;
                Ok(LuckScoreExpectation {
                    final_mean: RealBounds { lower: 1.0, upper: 1.0 },
                    final_support: IntegerBounds { lower: 1, upper: 1 },
                    ranges: Vec::new(),
                })
            })
            .unwrap();
        assert_eq!(result.final_mean.lower, 1.0);
        assert_eq!(cache.snapshot().writes, 2);
    }

    #[test]
    fn an_interrupted_writer_with_a_reused_process_id_does_not_block_the_next_commit() {
        let directory = Directory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.0.join("record.json");
        let stale = path.with_extension(format!("{}.7.tmp", std::process::id()));
        fs::write(&stale, b"unfinished predecessor").unwrap();
        let mut ids = [7, 8].into_iter();
        write_atomic_with(&path, b"complete replacement", || ids.next().unwrap()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"complete replacement");
        assert_eq!(fs::read(&stale).unwrap(), b"unfinished predecessor");
        assert!(!path.with_extension(format!("{}.8.tmp", std::process::id())).exists());
    }
}
