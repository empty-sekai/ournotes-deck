//! Portable, indexed lottery-response data, without a native probability or score certificate.
//!
//! A fingerprint is supplied by the producer; this codec cannot establish that it names the correct native
//! inputs or that generation completed. Successful decoding proves only structural validity. Quantization
//! encloses the stored probability endpoints, not a mean score, and never authorizes a cache admission.
//!
//! The `ONLRSP02` binary format pairs each bucket's endpoints within a probability run. Lossless runs
//! store the lower binary64 bits XOR the previous run's lower bits (initially zero), then the upper bits
//! XOR the current lower bits. Quantized runs store the lower grid word and the nonnegative interval
//! width. Time deltas, row RLE, masks, moments and indexed whole-payload sharing are unchanged. Earlier
//! format identifiers are rejected; JSON inspection types do not depend on the binary format version.

use crate::live::full::{LuckDpCertifiedResult, LuckSkillKey, LuckSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::mem::size_of;
use std::ops::Range;

const MAGIC: &[u8; 8] = b"ONLRSP02";

/// Lowercase SHA-256 of caller-supplied descriptor bytes. Hashing supplies identity, not semantic authority.
pub fn response_fingerprint(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(out, "{byte:02x}").expect("writing to a String cannot fail");
    }
    out
}

/// Source order and multiplicity are significant, including sources on the same holder.
pub type EntryKey = Vec<(LuckSkillKey, usize)>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseContext {
    pub fingerprint: String,
    pub algorithm_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseStep {
    pub time_ms: i32,
    /// [neither, direct probe only, Rush only, both], each [lower, upper].
    pub buckets: [[f64; 2]; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseRangeMoments {
    pub luck_points: [f64; 2],
    /// Miss, Hit, Super Hit and Critical expected counts. These are not probabilities.
    pub lot_results: [[f64; 2]; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseCurve {
    pub steps: Vec<ResponseStep>,
    pub probe_transitions: Vec<u8>,
    pub probes: Vec<bool>,
    pub range_moments: Vec<ResponseRangeMoments>,
    pub peak_states: usize,
    pub transitions: u64,
}

impl ResponseCurve {
    /// Copy every exported field. This creates inspection data, not a transferable native proof.
    pub fn from_certified(result: &LuckDpCertifiedResult) -> Self {
        // New native payload fields require an explicit codec decision instead of silently disappearing.
        let LuckDpCertifiedResult { steps, probe_transitions, probes, range_moments, peak_states, transitions } =
            result;
        let bounds = |v: crate::live::certified::F64Interval| [v.lower(), v.upper()];
        Self {
            steps: steps
                .iter()
                .map(|(time_ms, values)| ResponseStep {
                    time_ms: *time_ms,
                    buckets: values.map(|mass| bounds(mass.interval())),
                })
                .collect(),
            probe_transitions: probe_transitions.clone(),
            probes: probes.clone(),
            range_moments: range_moments
                .iter()
                .map(|range| ResponseRangeMoments {
                    luck_points: bounds(range.luck_points),
                    lot_results: range.lot_results.map(bounds),
                })
                .collect(),
            peak_states: *peak_states,
            transitions: *transitions,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Response {
    Success {
        curve: ResponseCurve,
    },
    /// Producers must separately distinguish structural refusal from cancellation or exhausted work.
    Unsupported {
        reason: String,
    },
}

impl<'de> Deserialize<'de> for Response {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        // Internally tagged enums buffer through serde's generic Content representation, which cannot
        // retain serde_json's arbitrary-precision Number. Going through Value keeps the decimal number
        // intact; from_value uses Number's correctly rounded f64 conversion for every endpoint.
        let value = serde_json::Value::deserialize(deserializer)?;
        let serde_json::Value::Object(mut fields) = value else {
            return Err(D::Error::custom("response must be an object"));
        };
        let status = fields.remove("status").ok_or_else(|| D::Error::custom("missing response status"))?;
        let result = match status.as_str() {
            Some("success") => {
                let curve = fields.remove("curve").ok_or_else(|| D::Error::custom("missing response curve"))?;
                Self::Success { curve: serde_json::from_value(curve).map_err(D::Error::custom)? }
            }
            Some("unsupported") => {
                let reason = fields.remove("reason").ok_or_else(|| D::Error::custom("missing refusal reason"))?;
                let serde_json::Value::String(reason) = reason else {
                    return Err(D::Error::custom("refusal reason must be a string"));
                };
                Self::Unsupported { reason }
            }
            _ => return Err(D::Error::custom("unknown response status")),
        };
        if !fields.is_empty() {
            return Err(D::Error::custom("unexpected fields for response status"));
        }
        Ok(result)
    }
}

impl Response {
    /// Resident owned bytes after decoding, including actual vector/string capacities.
    pub fn allocated_bytes(&self) -> Option<usize> {
        let mut bytes = size_of::<Self>();
        match self {
            Self::Unsupported { reason } => add_bytes(&mut bytes, reason.capacity(), 1, usize::MAX).ok()?,
            Self::Success { curve } => {
                add_bytes(&mut bytes, curve.steps.capacity(), size_of::<ResponseStep>(), usize::MAX).ok()?;
                add_bytes(&mut bytes, curve.probe_transitions.capacity(), 1, usize::MAX).ok()?;
                add_bytes(&mut bytes, curve.probes.capacity(), size_of::<bool>(), usize::MAX).ok()?;
                add_bytes(&mut bytes, curve.range_moments.capacity(), size_of::<ResponseRangeMoments>(), usize::MAX)
                    .ok()?;
            }
        }
        Some(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseEntry {
    pub key: EntryKey,
    pub response: Response,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseTable {
    pub context: ResponseContext,
    pub entries: Vec<ResponseEntry>,
}

/// Quantized probabilities use exact binary grids 2^-bits, with an extra endpoint for exactly one.
/// Integer varints retain that endpoint without saturation. Moments always retain all binary64 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quantization {
    Lossless,
    U16,
    U24,
    U32,
}

impl Quantization {
    fn tag(self) -> u8 {
        match self {
            Self::Lossless => 0,
            Self::U16 => 16,
            Self::U24 => 24,
            Self::U32 => 32,
        }
    }

    fn read(tag: u8) -> Result<Self, ResponseError> {
        match tag {
            0 => Ok(Self::Lossless),
            16 => Ok(Self::U16),
            24 => Ok(Self::U24),
            32 => Ok(Self::U32),
            _ => Err(invalid("unknown probability encoding")),
        }
    }
}

/// Independent resource limits for an archive, its resident index, and one decoded response.
/// Limits do not reserve space in any native search cache. Callers must include these allocations in
/// their own budget; encoded input and decoded output can coexist during a lookup.
#[derive(Clone, Copy, Debug)]
pub struct ResponseLimits {
    pub max_archive_bytes: usize,
    pub max_index_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_entries: usize,
    pub max_key_items: usize,
    pub max_string_bytes: usize,
    pub max_steps: usize,
    pub max_frames: usize,
}

impl Default for ResponseLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 64 * 1024 * 1024,
            max_index_bytes: 8 * 1024 * 1024,
            max_decoded_bytes: 32 * 1024 * 1024,
            max_entries: 65_536,
            max_key_items: 15,
            max_string_bytes: 1024 * 1024,
            max_steps: 1_048_576,
            max_frames: 1_048_576,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseError(&'static str);

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ResponseError {}

fn invalid(message: &'static str) -> ResponseError {
    ResponseError(message)
}

fn add_bytes(total: &mut usize, count: usize, unit: usize, limit: usize) -> Result<(), ResponseError> {
    *total = total
        .checked_add(count.checked_mul(unit).ok_or_else(|| invalid("size overflow"))?)
        .filter(|bytes| *bytes <= limit)
        .ok_or_else(|| invalid("byte limit exceeded"))?;
    Ok(())
}

fn reserved<T>(count: usize, total: &mut usize, limit: usize) -> Result<Vec<T>, ResponseError> {
    // Check before allocation, then charge actual capacity rather than the requested count.
    let mut estimate = *total;
    add_bytes(&mut estimate, count, size_of::<T>(), limit)?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| invalid("allocation failed"))?;
    add_bytes(total, values.capacity(), size_of::<T>(), limit)?;
    Ok(values)
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Writer {
    fn new(limit: usize) -> Self {
        Self { bytes: Vec::new(), limit }
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), ResponseError> {
        let needed = self.bytes.len().checked_add(bytes.len()).ok_or_else(|| invalid("size overflow"))?;
        if needed > self.limit {
            return Err(invalid("encoded byte limit exceeded"));
        }
        if needed > self.bytes.capacity() {
            let capacity = needed.max(self.bytes.capacity().saturating_mul(2)).min(self.limit);
            self.bytes.try_reserve_exact(capacity - self.bytes.len()).map_err(|_| invalid("allocation failed"))?;
        }
        if self.bytes.capacity() > self.limit {
            return Err(invalid("encoded capacity exceeds byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn var(&mut self, mut value: u64) -> Result<(), ResponseError> {
        let mut out = [0u8; 10];
        let mut len = 0;
        loop {
            out[len] = (value & 127) as u8;
            value >>= 7;
            if value != 0 {
                out[len] |= 128;
            }
            len += 1;
            if value == 0 {
                return self.put(&out[..len]);
            }
        }
    }

    fn signed(&mut self, value: i64) -> Result<(), ResponseError> {
        self.var(((value as u64) << 1) ^ ((value >> 63) as u64))
    }

    fn string(&mut self, value: &str, limits: ResponseLimits) -> Result<(), ResponseError> {
        if value.len() > limits.max_string_bytes {
            return Err(invalid("string limit exceeded"));
        }
        self.var(value.len() as u64)?;
        self.put(value.as_bytes())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], ResponseError> {
        let end = self.at.checked_add(len).ok_or_else(|| invalid("size overflow"))?;
        let value = self.bytes.get(self.at..end).ok_or_else(|| invalid("truncated response archive"))?;
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, ResponseError> {
        Ok(self.take(1)?[0])
    }

    fn var(&mut self) -> Result<u64, ResponseError> {
        let mut value = 0u64;
        for index in 0..10 {
            let byte = self.byte()?;
            if index == 9 && byte > 1 {
                return Err(invalid("varint overflow"));
            }
            value |= u64::from(byte & 127) << (index * 7);
            if byte & 128 == 0 {
                if index != 0 && byte == 0 {
                    return Err(invalid("noncanonical varint"));
                }
                return Ok(value);
            }
        }
        Err(invalid("varint overflow"))
    }

    fn count(&mut self, maximum: usize) -> Result<usize, ResponseError> {
        usize::try_from(self.var()?)
            .ok()
            .filter(|value| *value <= maximum)
            .ok_or_else(|| invalid("count limit exceeded"))
    }

    fn signed(&mut self) -> Result<i64, ResponseError> {
        let value = self.var()?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }

    fn word(&mut self) -> Result<u64, ResponseError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| invalid("word size"))?))
    }

    fn string(&mut self, limits: ResponseLimits, total: &mut usize, limit: usize) -> Result<String, ResponseError> {
        let len = self.count(limits.max_string_bytes)?;
        let text = std::str::from_utf8(self.take(len)?).map_err(|_| invalid("invalid UTF-8"))?;
        let mut out = String::new();
        let mut estimate = *total;
        add_bytes(&mut estimate, len, 1, limit)?;
        out.try_reserve_exact(len).map_err(|_| invalid("allocation failed"))?;
        add_bytes(total, out.capacity(), 1, limit)?;
        out.push_str(text);
        Ok(out)
    }
}

fn validate_interval([lower, upper]: [f64; 2], probability: bool) -> Result<(), ResponseError> {
    if !lower.is_finite() || !upper.is_finite() || lower > upper || (probability && (lower < 0.0 || upper > 1.0)) {
        return Err(invalid("invalid response interval"));
    }
    Ok(())
}

fn bucket_words(buckets: &[[f64; 2]; 4], mode: Quantization) -> Result<[u64; 8], ResponseError> {
    let mut words = [0; 8];
    for (index, &bounds) in buckets.iter().enumerate() {
        validate_interval(bounds, true)?;
        if mode == Quantization::Lossless {
            words[index * 2] = bounds[0].to_bits();
            words[index * 2 + 1] = bounds[1].to_bits();
        } else {
            let grid = (1u64 << mode.tag()) as f64;
            words[index * 2] = (bounds[0] * grid).floor() as u64;
            words[index * 2 + 1] = (bounds[1] * grid).ceil() as u64;
        }
    }
    Ok(words)
}

fn encode_response(response: &Response, mode: Quantization, limits: ResponseLimits) -> Result<Vec<u8>, ResponseError> {
    let mut out = Writer::new(limits.max_archive_bytes);
    let curve = match response {
        Response::Unsupported { reason } => {
            out.put(&[0])?;
            out.string(reason, limits)?;
            return Ok(out.bytes);
        }
        Response::Success { curve } => curve,
    };
    if curve.steps.len() > limits.max_steps
        || curve.probe_transitions.len() > limits.max_frames
        || curve.probes.len() > limits.max_frames
        || curve.range_moments.len() > limits.max_frames
    {
        return Err(invalid("response count limit exceeded"));
    }
    out.put(&[1])?;
    out.var(curve.peak_states as u64)?;
    out.var(curve.transitions)?;
    out.var(curve.steps.len() as u64)?;
    let mut previous = 0i64;
    for (index, step) in curve.steps.iter().enumerate() {
        let time = i64::from(step.time_ms);
        if index != 0 && time <= previous {
            return Err(invalid("response times must be strictly increasing"));
        }
        out.signed(time - previous)?;
        previous = time;
    }
    let mut at = 0;
    let mut previous_lower = [0u64; 4];
    while at < curve.steps.len() {
        let words = bucket_words(&curve.steps[at].buckets, mode)?;
        let mut end = at + 1;
        while end < curve.steps.len() && bucket_words(&curve.steps[end].buckets, mode)? == words {
            end += 1;
        }
        out.var((end - at) as u64)?;
        for (pair, old_lower) in words.chunks_exact(2).zip(&mut previous_lower) {
            let (lower, upper) = (pair[0], pair[1]);
            if mode == Quantization::Lossless {
                out.var(lower ^ *old_lower)?;
                out.var(upper ^ lower)?;
                *old_lower = lower;
            } else {
                out.var(lower)?;
                out.var(upper.checked_sub(lower).ok_or_else(|| invalid("negative quantized interval width"))?)?;
            }
        }
        at = end;
    }
    out.var(curve.probe_transitions.len() as u64)?;
    at = 0;
    while at < curve.probe_transitions.len() {
        let mask = curve.probe_transitions[at];
        if !(1..=15).contains(&mask) {
            return Err(invalid("invalid probe transition mask"));
        }
        let mut end = at + 1;
        while end < curve.probe_transitions.len() && curve.probe_transitions[end] == mask {
            end += 1;
        }
        out.var((end - at) as u64)?;
        out.put(&[mask])?;
        at = end;
    }
    out.var(curve.probes.len() as u64)?;
    for chunk in curve.probes.chunks(8) {
        out.put(&[chunk.iter().enumerate().fold(0u8, |bits, (bit, held)| bits | (u8::from(*held) << bit))])?;
    }
    out.var(curve.range_moments.len() as u64)?;
    for range in &curve.range_moments {
        for bounds in std::iter::once(range.luck_points).chain(range.lot_results) {
            validate_interval(bounds, false)?;
            for value in bounds {
                out.put(&value.to_bits().to_le_bytes())?;
            }
        }
    }
    Ok(out.bytes)
}

#[derive(Debug)]
struct IndexEntry {
    key: EntryKey,
    blob: Range<usize>,
    digest: [u8; 32],
}

struct EncodedBlob {
    digest: [u8; 32],
    range: Range<usize>,
}

impl ResponseTable {
    /// Encode all keys. Only byte-identical complete payloads share storage; keys are never removed.
    pub fn encode(&self, mode: Quantization, limits: ResponseLimits) -> Result<Vec<u8>, ResponseError> {
        if self.entries.len() > limits.max_entries {
            return Err(invalid("entry limit exceeded"));
        }
        let mut index_bytes = 0;
        let mut sorted = reserved(self.entries.len(), &mut index_bytes, limits.max_index_bytes)?;
        sorted.extend(self.entries.iter());
        sorted.sort_unstable_by(|a, b| a.key.cmp(&b.key));
        if sorted.windows(2).any(|pair| pair[0].key == pair[1].key) {
            return Err(invalid("duplicate response key"));
        }
        let mut patches = reserved(self.entries.len(), &mut index_bytes, limits.max_index_bytes)?;
        let mut blobs = reserved::<EncodedBlob>(self.entries.len(), &mut index_bytes, limits.max_index_bytes)?;
        let mut out = Writer::new(limits.max_archive_bytes);
        out.put(MAGIC)?;
        out.put(&[mode.tag()])?;
        out.string(&self.context.fingerprint, limits)?;
        out.string(&self.context.algorithm_version, limits)?;
        out.var(self.entries.len() as u64)?;
        for entry in &self.entries {
            if entry.key.len() > limits.max_key_items {
                return Err(invalid("key length limit exceeded"));
            }
            out.var(entry.key.len() as u64)?;
            for (skill, position) in &entry.key {
                if *position >= 5 {
                    return Err(invalid("invalid holder position"));
                }
                out.put(&[match skill.source {
                    LuckSource::Gekisou => 0,
                    LuckSource::GekisouSupport => 1,
                }])?;
                out.signed(skill.id)?;
                out.signed(skill.level)?;
                out.put(&[
                    match skill.matched {
                        None => 0,
                        Some(false) => 1,
                        Some(true) => 2,
                    },
                    *position as u8,
                ])?;
            }
            patches.push(out.bytes.len());
            out.put(&[0; 48])?;
        }
        for (entry, patch) in self.entries.iter().zip(patches) {
            let payload = encode_response(&entry.response, mode, limits)?;
            let digest: [u8; 32] = Sha256::digest(&payload).into();
            let start = blobs.partition_point(|blob| blob.digest < digest);
            let same = blobs[start..]
                .iter()
                .take_while(|blob| blob.digest == digest)
                .find(|blob| &out.bytes[blob.range.clone()] == payload.as_slice());
            let range = if let Some(blob) = same {
                blob.range.clone()
            } else {
                let from = out.bytes.len();
                out.put(&payload)?;
                let range = from..out.bytes.len();
                blobs.insert(start, EncodedBlob { digest, range: range.clone() });
                range
            };
            out.bytes[patch..patch + 8].copy_from_slice(&(range.start as u64).to_le_bytes());
            out.bytes[patch + 8..patch + 16].copy_from_slice(&(range.len() as u64).to_le_bytes());
            out.bytes[patch + 16..patch + 48].copy_from_slice(&digest);
        }
        drop(sorted);
        drop(blobs);
        // Apply the same index and decoded-size guards to producer output.
        let archive = ResponseArchive::open(&out.bytes, limits)?;
        for entry in &archive.entries {
            archive.decode(entry)?;
        }
        Ok(out.bytes)
    }
}

/// An index borrowing the encoded bytes. Opening checks the index and exact, non-overlapping blob cover;
/// lookup validates and decodes one blob, never an entire table. Context identity remains the caller's duty.
#[derive(Debug)]
pub struct ResponseArchive<'a> {
    bytes: &'a [u8],
    context: ResponseContext,
    mode: Quantization,
    entries: Vec<IndexEntry>,
    limits: ResponseLimits,
    index_bytes: usize,
}

impl<'a> ResponseArchive<'a> {
    pub fn open(bytes: &'a [u8], limits: ResponseLimits) -> Result<Self, ResponseError> {
        if bytes.len() > limits.max_archive_bytes {
            return Err(invalid("archive byte limit exceeded"));
        }
        let mut input = Reader { bytes, at: 0 };
        if input.take(MAGIC.len())? != MAGIC {
            return Err(invalid("invalid response archive magic"));
        }
        let mode = Quantization::read(input.byte()?)?;
        let mut index_bytes = size_of::<Self>();
        let context = ResponseContext {
            fingerprint: input.string(limits, &mut index_bytes, limits.max_index_bytes)?,
            algorithm_version: input.string(limits, &mut index_bytes, limits.max_index_bytes)?,
        };
        let count = input.count(limits.max_entries)?;
        let mut entries = reserved::<IndexEntry>(count, &mut index_bytes, limits.max_index_bytes)?;
        for _ in 0..count {
            let count = input.count(limits.max_key_items)?;
            let mut key = reserved(count, &mut index_bytes, limits.max_index_bytes)?;
            for _ in 0..count {
                let source = match input.byte()? {
                    0 => LuckSource::Gekisou,
                    1 => LuckSource::GekisouSupport,
                    _ => return Err(invalid("invalid skill source")),
                };
                let id = input.signed()?;
                let level = input.signed()?;
                let matched = match input.byte()? {
                    0 => None,
                    1 => Some(false),
                    2 => Some(true),
                    _ => return Err(invalid("invalid formation flag")),
                };
                let position = usize::from(input.byte()?);
                if position >= 5 {
                    return Err(invalid("invalid holder position"));
                }
                key.push((LuckSkillKey { source, id, level, matched }, position));
            }
            let start = usize::try_from(input.word()?).map_err(|_| invalid("blob offset overflow"))?;
            let len = usize::try_from(input.word()?).map_err(|_| invalid("blob length overflow"))?;
            let end = start
                .checked_add(len)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| invalid("invalid blob range"))?;
            if len == 0 {
                return Err(invalid("empty response blob"));
            }
            let digest = input.take(32)?.try_into().map_err(|_| invalid("digest size"))?;
            entries.push(IndexEntry { key, blob: start..end, digest });
        }
        // Sorting the only resident vector also gives allocation-free binary-search lookup.
        entries.sort_unstable_by(|a, b| a.key.cmp(&b.key));
        if entries.windows(2).any(|pair| pair[0].key == pair[1].key) {
            return Err(invalid("duplicate response key"));
        }
        // Offset validation needs temporary storage, charged simultaneously against the index allowance.
        let mut scratch_bytes = index_bytes;
        let mut ranges = reserved(count, &mut scratch_bytes, limits.max_index_bytes)?;
        ranges.extend(entries.iter().map(|entry| (entry.blob.start, entry.blob.end)));
        ranges.sort_unstable();
        ranges.dedup();
        let mut end = input.at;
        for (start, next) in ranges {
            if start != end {
                return Err(invalid("overlapping, missing or unreferenced response bytes"));
            }
            end = next;
        }
        if end != bytes.len() {
            return Err(invalid("trailing response bytes"));
        }
        Ok(Self { bytes, context, mode, entries, limits, index_bytes })
    }

    pub fn context(&self) -> &ResponseContext {
        &self.context
    }

    pub fn quantization(&self) -> Quantization {
        self.mode
    }

    pub fn keys(&self) -> impl ExactSizeIterator<Item = &EntryKey> {
        self.entries.iter().map(|entry| &entry.key)
    }

    pub fn index_bytes(&self) -> usize {
        self.index_bytes
    }

    pub fn encoded_payload_bytes(&self, key: &EntryKey) -> Option<usize> {
        self.find(key).map(|entry| entry.blob.len())
    }

    /// Position in this archive's sorted key index, without decoding a response. The position is local
    /// to this archive and can index a caller's bounded usage bitmap.
    pub fn key_index(&self, key: &EntryKey) -> Option<usize> {
        self.entries.binary_search_by(|entry| entry.key.cmp(key)).ok()
    }

    fn find(&self, key: &EntryKey) -> Option<&IndexEntry> {
        self.key_index(key).map(|index| &self.entries[index])
    }

    pub fn lookup(&self, key: &EntryKey) -> Result<Option<Response>, ResponseError> {
        self.find(key).map(|entry| self.decode(entry)).transpose()
    }

    fn decode(&self, entry: &IndexEntry) -> Result<Response, ResponseError> {
        let bytes = &self.bytes[entry.blob.clone()];
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if digest != entry.digest {
            return Err(invalid("response payload checksum mismatch"));
        }
        decode_response(bytes, self.mode, self.limits)
    }
}

fn decode_response(bytes: &[u8], mode: Quantization, limits: ResponseLimits) -> Result<Response, ResponseError> {
    let mut input = Reader { bytes, at: 0 };
    let mut decoded = size_of::<Response>();
    let status = input.byte()?;
    if status == 0 {
        let reason = input.string(limits, &mut decoded, limits.max_decoded_bytes)?;
        if input.at != bytes.len() {
            return Err(invalid("trailing response payload"));
        }
        return Ok(Response::Unsupported { reason });
    }
    if status != 1 {
        return Err(invalid("unknown response status"));
    }
    let peak_states = input.count(usize::MAX)?;
    let transitions = input.var()?;
    let count = input.count(limits.max_steps)?;
    let mut steps = reserved::<ResponseStep>(count, &mut decoded, limits.max_decoded_bytes)?;
    let mut previous = 0i64;
    for index in 0..count {
        let time = previous.checked_add(input.signed()?).ok_or_else(|| invalid("time overflow"))?;
        if (index != 0 && time <= previous) || i32::try_from(time).is_err() {
            return Err(invalid("invalid response time"));
        }
        steps.push(ResponseStep { time_ms: time as i32, buckets: [[0.0; 2]; 4] });
        previous = time;
    }
    let mut at = 0;
    let mut previous_lower = [0u64; 4];
    while at < count {
        let run = input.count(count - at)?;
        if run == 0 {
            return Err(invalid("empty probability run"));
        }
        let mut buckets = [[0.0; 2]; 4];
        for (bounds, old_lower) in buckets.iter_mut().zip(&mut previous_lower) {
            let first = input.var()?;
            let second = input.var()?;
            *bounds = if mode == Quantization::Lossless {
                let lower = first ^ *old_lower;
                let upper = second ^ lower;
                *old_lower = lower;
                [f64::from_bits(lower), f64::from_bits(upper)]
            } else {
                let lower = first;
                let upper = lower.checked_add(second).ok_or_else(|| invalid("quantized interval width overflow"))?;
                let grid = 1u64 << mode.tag();
                if lower > grid || upper > grid {
                    return Err(invalid("quantized endpoint exceeds one"));
                }
                [lower as f64 / grid as f64, upper as f64 / grid as f64]
            };
        }
        for bounds in buckets {
            validate_interval(bounds, true)?;
        }
        for step in &mut steps[at..at + run] {
            step.buckets = buckets;
        }
        at += run;
    }
    let count = input.count(limits.max_frames)?;
    let mut probe_transitions = reserved::<u8>(count, &mut decoded, limits.max_decoded_bytes)?;
    while probe_transitions.len() < count {
        let run = input.count(count - probe_transitions.len())?;
        let mask = input.byte()?;
        if run == 0 || !(1..=15).contains(&mask) {
            return Err(invalid("invalid probe transition run"));
        }
        probe_transitions.resize(probe_transitions.len() + run, mask);
    }
    let count = input.count(limits.max_frames)?;
    let mut probes = reserved::<bool>(count, &mut decoded, limits.max_decoded_bytes)?;
    for index in (0..count).step_by(8) {
        let bits = input.byte()?;
        let len = (count - index).min(8);
        if len < 8 && bits >> len != 0 {
            return Err(invalid("nonzero probe padding"));
        }
        probes.extend((0..len).map(|bit| bits & (1 << bit) != 0));
    }
    let count = input.count(limits.max_frames)?;
    let mut range_moments = reserved::<ResponseRangeMoments>(count, &mut decoded, limits.max_decoded_bytes)?;
    for _ in 0..count {
        let mut values = [[0.0; 2]; 5];
        for value in &mut values {
            *value = [f64::from_bits(input.word()?), f64::from_bits(input.word()?)];
            validate_interval(*value, false)?;
        }
        range_moments.push(ResponseRangeMoments {
            luck_points: values[0],
            lot_results: [values[1], values[2], values[3], values[4]],
        });
    }
    if input.at != bytes.len() {
        return Err(invalid("trailing response payload"));
    }
    Ok(Response::Success {
        curve: ResponseCurve { steps, probe_transitions, probes, range_moments, peak_states, transitions },
    })
}

#[cfg(test)]
#[path = "luck_response_tests.rs"]
mod tests;
