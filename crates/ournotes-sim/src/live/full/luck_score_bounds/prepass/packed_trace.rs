//! Lossless storage for completed terminal traces. Every query and filing remains at its original ordinal.
//!
//! BoundsEvent reserves enough space for its largest variant even for a two-integer Query. Retained recipes
//! keep these small events inline and move the remaining original variants into a payload table. Decoding
//! restores the complete BoundsTrace before the existing rank, prefix and native arithmetic consumers run;
//! this codec makes no claim that an event is numerically redundant or that two histories are equivalent.

use super::{BoundsEvent, BoundsTrace, ComboObserver, ProbeRow};
use crate::live::full::luck_score_bounds::trace_drift::Decline;
use std::borrow::Cow;
use std::mem::size_of;

#[derive(Clone, Copy, Debug)]
enum Event {
    Query { time_ms: i32, to: i32 },
    Ready(i32),
    Potential(u32),
    Probe { frame: u32, time_ms: i32 },
    Stored(u32),
}

/// A frame outside u32 is retained as the complete original variant, never narrowed or rejected as native
/// input. The explicit match also makes a future BoundsEvent variant require a storage decision here.
fn inline(event: &BoundsEvent) -> Option<Event> {
    match event {
        BoundsEvent::Query { time_ms, to } => Some(Event::Query { time_ms: *time_ms, to: *to }),
        BoundsEvent::ProbabilityReady(time) => Some(Event::Ready(*time)),
        BoundsEvent::Potential { frame } => u32::try_from(*frame).ok().map(Event::Potential),
        BoundsEvent::Probe { frame, time_ms } => {
            u32::try_from(*frame).ok().map(|frame| Event::Probe { frame, time_ms: *time_ms })
        }
        BoundsEvent::Note { .. }
        | BoundsEvent::Factor { .. }
        | BoundsEvent::Combo { .. }
        | BoundsEvent::Rank { .. } => None,
    }
}

/// The caller already drops recording-only ComboObserver scratch after its complete proof. Require that
/// exact default state instead of silently projecting any new or forgotten recorder state in the codec.
fn empty_observer(combo: &ComboObserver) -> bool {
    let ComboObserver { seen, consistent, judgements, windows, current, filed, stale } = combo;
    seen.capacity() == 0
        && *consistent == 0
        && *judgements == 0
        && windows.is_none()
        && current.capacity() == 0
        && filed.capacity() == 0
        && stale.capacity() == 0
}

fn add_product(bytes: usize, count: usize, width: usize) -> Result<usize, Decline> {
    bytes.checked_add(count.checked_mul(width).ok_or(Decline::Capacity)?).ok_or(Decline::Capacity)
}

/// Actual native-vector capacities, not their logical lengths. The observer must already be default.
fn trace_bytes(trace: &BoundsTrace) -> Result<usize, Decline> {
    let bytes = add_product(size_of::<BoundsTrace>(), trace.events.capacity(), size_of::<BoundsEvent>())?;
    let bytes = add_product(bytes, trace.probes.capacity(), size_of::<ProbeRow>())?;
    add_product(bytes, trace.probe_filings.as_ref().map_or(0, Vec::capacity), size_of::<usize>())
}

/// Small or unusually payload-heavy traces keep their original representation and need no decode workspace.
/// With several compact recipes the cache amortizes one workspace reservation; it never charges a workspace
/// per entry or assumes that the first compact entry alone has a smaller retained-plus-workspace total.
pub(super) enum StoredTerminalTrace {
    Original(BoundsTrace),
    Packed(PackedTerminalTrace),
}

impl StoredTerminalTrace {
    pub(super) fn encode(
        trace: BoundsTrace,
        capacity: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, Decline> {
        // The caller's allowance covers this enum plus every allocation and the single native decode
        // workspace. PackedTerminalTrace's own ledger does not include the enum's larger inline storage.
        let packed_capacity =
            capacity.saturating_sub(size_of::<Self>().saturating_sub(size_of::<PackedTerminalTrace>()));
        let value = match PackedTerminalTrace::encode(&trace, packed_capacity, &mut cancelled) {
            Ok(packed)
                if packed.allocated_bytes() - size_of::<PackedTerminalTrace>()
                    < trace_bytes(&trace)? - size_of::<BoundsTrace>() =>
            {
                Self::Packed(packed)
            }
            Ok(_) | Err(Decline::Capacity) => Self::Original(trace),
            Err(error) => return Err(error),
        };
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        Ok(value)
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(match self {
            Self::Original(trace) => trace_bytes(trace).unwrap_or(usize::MAX).saturating_sub(size_of::<BoundsTrace>()),
            Self::Packed(trace) => trace.allocated_bytes() - size_of::<PackedTerminalTrace>(),
        })
    }

    pub(super) fn decode_workspace_bytes(&self) -> usize {
        match self {
            Self::Original(_) => 0,
            Self::Packed(trace) => trace.decode_workspace_bytes(),
        }
    }

    #[cfg(test)]
    pub(super) fn original_allocated_bytes(&self) -> usize {
        match self {
            Self::Original(_) => self.allocated_bytes(),
            Self::Packed(trace) => size_of::<Self>() + trace.decode_workspace_bytes() - size_of::<BoundsTrace>(),
        }
    }

    pub(super) fn decode(
        &self,
        capacity: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Cow<'_, BoundsTrace>, Decline> {
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        match self {
            Self::Original(trace) => Ok(Cow::Borrowed(trace)),
            Self::Packed(trace) => trace.decode(capacity, cancelled).map(Cow::Owned),
        }
    }
}

pub(super) struct PackedTerminalTrace {
    events: Vec<Event>,
    payloads: Vec<BoundsEvent>,
    queries: usize,
    frames: usize,
    probes: Vec<ProbeRow>,
    has_luck: bool,
    filing_gate: Option<Option<i64>>,
    probe_filings: Option<Vec<usize>>,
    /// The original native trace's actual capacity covers one decoding workspace. The decoder independently
    /// checks its actual allocated capacities before returning; the program cache reserves this amount once.
    decode_bytes: usize,
}

impl PackedTerminalTrace {
    /// Copy a completed trace without changing any event, reference, integer or floating-point bit.
    /// The original remains available if packing is unhelpful or an optional allocation is refused.
    pub(super) fn encode(
        trace: &BoundsTrace,
        capacity: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, Decline> {
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        if !empty_observer(&trace.combo) {
            return Err(Decline::Incomplete);
        }
        let decode_bytes = trace_bytes(trace)?;
        let mut payload_count = 0usize;
        for (ordinal, event) in trace.events.iter().enumerate() {
            if ordinal.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            if inline(event).is_none() {
                payload_count = payload_count.checked_add(1).ok_or(Decline::Capacity)?;
            }
        }
        // Stored indices are private storage references, never a restriction on native frame or query IDs.
        if payload_count > u32::MAX as usize {
            return Err(Decline::Capacity);
        }
        let mut peak = decode_bytes.checked_add(size_of::<Self>()).ok_or(Decline::Capacity)?;
        peak = add_product(peak, trace.events.len(), size_of::<Event>())?;
        peak = add_product(peak, payload_count, size_of::<BoundsEvent>())?;
        peak = add_product(peak, trace.probes.len(), size_of::<ProbeRow>())?;
        peak = add_product(peak, trace.probe_filings.as_ref().map_or(0, Vec::len), size_of::<usize>())?;
        if peak > capacity {
            return Err(Decline::Capacity);
        }
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        let mut events = Vec::new();
        events.try_reserve_exact(trace.events.len()).map_err(|_| Decline::Capacity)?;
        let mut payloads = Vec::new();
        payloads.try_reserve_exact(payload_count).map_err(|_| Decline::Capacity)?;
        let mut probes = Vec::new();
        probes.try_reserve_exact(trace.probes.len()).map_err(|_| Decline::Capacity)?;
        let mut probe_filings = trace.probe_filings.as_ref().map(|_| Vec::new());
        if let (Some(source), Some(target)) = (&trace.probe_filings, &mut probe_filings) {
            target.try_reserve_exact(source.len()).map_err(|_| Decline::Capacity)?;
        }
        let mut peak = decode_bytes.checked_add(size_of::<Self>()).ok_or(Decline::Capacity)?;
        peak = add_product(peak, events.capacity(), size_of::<Event>())?;
        peak = add_product(peak, payloads.capacity(), size_of::<BoundsEvent>())?;
        peak = add_product(peak, probes.capacity(), size_of::<ProbeRow>())?;
        peak = add_product(peak, probe_filings.as_ref().map_or(0, Vec::capacity), size_of::<usize>())?;
        if peak > capacity {
            return Err(Decline::Capacity);
        }
        for (ordinal, event) in trace.events.iter().enumerate() {
            if ordinal.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            let encoded = match inline(event) {
                Some(encoded) => encoded,
                None => {
                    let index = u32::try_from(payloads.len()).map_err(|_| Decline::Capacity)?;
                    payloads.push(event.clone());
                    Event::Stored(index)
                }
            };
            events.push(encoded);
        }
        for (index, &probe) in trace.probes.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            probes.push(probe);
        }
        if let (Some(source), Some(target)) = (&trace.probe_filings, &mut probe_filings) {
            for (index, &filing) in source.iter().enumerate() {
                if index.is_multiple_of(64) && cancelled() {
                    return Err(Decline::Cancelled);
                }
                target.push(filing);
            }
        }
        let value = Self {
            events,
            payloads,
            queries: trace.queries,
            frames: trace.frames,
            probes,
            has_luck: trace.has_luck,
            filing_gate: trace.filing_gate,
            probe_filings,
            decode_bytes,
        };
        // Admission includes both the compact allocations and the complete decoder workspace. The cache
        // adds its identity, ingredients, result, shared curves and other entries to this same byte ledger.
        if value.allocated_bytes().checked_add(decode_bytes).is_none_or(|bytes| bytes > capacity) {
            return Err(Decline::Capacity);
        }
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        Ok(value)
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        // encode checked these exact capacities and their sum before publishing the immutable value.
        size_of::<Self>()
            + self.events.capacity() * size_of::<Event>()
            + self.payloads.capacity() * size_of::<BoundsEvent>()
            + self.probes.capacity() * size_of::<ProbeRow>()
            + self.probe_filings.as_ref().map_or(0, |filings| filings.capacity() * size_of::<usize>())
    }

    pub(super) fn decode_workspace_bytes(&self) -> usize {
        self.decode_bytes
    }

    pub(super) fn decode(&self, capacity: usize, mut cancelled: impl FnMut() -> bool) -> Result<BoundsTrace, Decline> {
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        if self.decode_bytes > capacity {
            return Err(Decline::Capacity);
        }
        let mut trace = BoundsTrace {
            events: Vec::new(),
            queries: self.queries,
            frames: self.frames,
            probes: Vec::new(),
            combo: ComboObserver::default(),
            has_luck: self.has_luck,
            filing_gate: self.filing_gate,
            probe_filings: self.probe_filings.as_ref().map(|_| Vec::new()),
        };
        trace.events.try_reserve_exact(self.events.len()).map_err(|_| Decline::Capacity)?;
        if trace_bytes(&trace)? > capacity {
            return Err(Decline::Capacity);
        }
        trace.probes.try_reserve_exact(self.probes.len()).map_err(|_| Decline::Capacity)?;
        if trace_bytes(&trace)? > capacity {
            return Err(Decline::Capacity);
        }
        if let (Some(source), Some(target)) = (&self.probe_filings, &mut trace.probe_filings) {
            target.try_reserve_exact(source.len()).map_err(|_| Decline::Capacity)?;
        }
        if trace_bytes(&trace)? > capacity {
            return Err(Decline::Capacity);
        }
        let mut next_payload = 0usize;
        for (ordinal, &event) in self.events.iter().enumerate() {
            if ordinal.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            trace.events.push(match event {
                Event::Query { time_ms, to } => BoundsEvent::Query { time_ms, to },
                Event::Ready(time) => BoundsEvent::ProbabilityReady(time),
                Event::Potential(frame) => BoundsEvent::Potential { frame: frame as usize },
                Event::Probe { frame, time_ms } => BoundsEvent::Probe { frame: frame as usize, time_ms },
                Event::Stored(index) => {
                    if index as usize != next_payload {
                        return Err(Decline::Incomplete);
                    }
                    let event = self.payloads.get(next_payload).ok_or(Decline::Incomplete)?.clone();
                    next_payload += 1;
                    event
                }
            });
        }
        if next_payload != self.payloads.len() {
            return Err(Decline::Incomplete);
        }
        for (index, &probe) in self.probes.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            trace.probes.push(probe);
        }
        if let (Some(source), Some(target)) = (&self.probe_filings, &mut trace.probe_filings) {
            for (index, &filing) in source.iter().enumerate() {
                if index.is_multiple_of(64) && cancelled() {
                    return Err(Decline::Cancelled);
                }
                target.push(filing);
            }
        }
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        Ok(trace)
    }
}

#[cfg(test)]
mod tests;
