//! Arithmetic ingredients for a terminal-note cap, before any probability capability is attached.
//!
//! Intended as a sibling of trace_drift in a private implementation. An admitted complete recorder must
//! provide this projection. The outer terminal capability still proves final-query readiness, the exact
//! note multiset, and (when used) the link from a joint bucket to direct-probe state.

#[cfg(feature = "search-diagnostics")]
use super::trace_drift::FieldWork;
use super::trace_drift::{self, Decline, Event};
use super::{BoundsEvent, BoundsTrace, replay};

const FIELDS: usize = 6;
type OrdinaryCommand = ((usize, i32, i32, usize), [f32; FIELDS]);

#[derive(Clone, Copy, Debug)]
pub(crate) enum TimedEvent {
    Note { frame: usize, note_id: i32, time_ms: i32 },
    Factor { frame: usize, time_ms: i32, owner: i32, deltas: [f32; FIELDS] },
    Potential { frame: usize },
    Probe { frame: usize, time_ms: i32 },
    Query { to: i32 },
    Other,
}

impl TimedEvent {
    fn counting(self) -> Event {
        match self {
            Self::Note { frame, .. } => Event::Note { frame },
            Self::Factor { frame, deltas, .. } => Event::Factor { frame, deltas },
            Self::Potential { frame } => Event::Potential { frame },
            Self::Probe { frame, .. } => Event::Probe { frame },
            Self::Query { to } => Event::Query { to },
            Self::Other => Event::Other,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Interval {
    lower: f64,
    upper: f64,
}

impl Interval {
    fn point(value: f32) -> Result<Self, Decline> {
        if !value.is_finite() {
            return Err(Decline::Nonfinite);
        }
        Ok(Self { lower: f64::from(value), upper: f64::from(value) })
    }

    fn add_exact(&mut self, value: f32) -> Result<(), Decline> {
        if !value.is_finite() {
            return Err(Decline::Nonfinite);
        }
        if value != 0.0 {
            self.lower = (self.lower + f64::from(value)).next_down();
            self.upper = (self.upper + f64::from(value)).next_up();
            if !self.lower.is_finite() || !self.upper.is_finite() {
                return Err(Decline::Nonfinite);
            }
        }
        Ok(())
    }
}

fn add_upper(a: f64, b: f64) -> Result<f64, Decline> {
    if !a.is_finite() || !b.is_finite() {
        return Err(Decline::Nonfinite);
    }
    let result = if b == 0.0 { a } else { (a + b).next_up() };
    result.is_finite().then_some(result).ok_or(Decline::Nonfinite)
}

fn multiply_upper(a: f64, b: f64) -> Result<f64, Decline> {
    if !a.is_finite() || !b.is_finite() || a < 0.0 || b < 0.0 {
        return Err(Decline::Nonfinite);
    }
    if a == 0.0 || b == 0.0 {
        return Ok(0.0);
    }
    let result = (a * b).next_up();
    result.is_finite().then_some(result).ok_or(Decline::Nonfinite)
}

#[derive(Clone, Debug)]
pub(crate) struct TerminalNote {
    pub(crate) time_ms: i32,
    /// Original BoundsEvent ordinal; the projection preserves every event and its order.
    pub(crate) event: usize,
    pub(crate) ordinary_lower: [f64; FIELDS],
    /// Ordinary native-delta prefix plus the full history drift, before any direct-probe ideal amplitude.
    /// Field order is combo, note, Just, Perfect, Great, Good.
    pub(crate) ordinary_upper: [f64; FIELDS],
}

#[derive(Debug)]
pub(crate) struct TerminalIngredients {
    /// Sorted by chart time, note ID, then original filing ordinal. Duplicate notes are retained.
    pub(crate) notes: Box<[TerminalNote]>,
    #[cfg(feature = "search-diagnostics")]
    pub(crate) work: [FieldWork; FIELDS],
    pub(crate) drift: [f64; FIELDS],
    #[cfg(feature = "search-diagnostics")]
    pub(crate) state_magnitude: [f64; FIELDS],
    pub(crate) probe_time_runs: usize,
    initial: [f32; FIELDS],
    /// Native execution order with original filing ordinals. Earlier rank queries may reuse the complete
    /// roundoff allowance, but their ideal prefixes must exclude every command not yet filed at that query.
    ordinary: Box<[OrdinaryCommand]>,
    probe_sum_upper: f64,
    unconditional_probe_upper: f64,
}

impl TerminalIngredients {
    /// Enclose every floating field at this note's retained terminal execution. The original query/filing
    /// certificate covers all histories; linking the direct probe class remains an outer admission.
    #[cfg(test)]
    pub(super) fn fields(&self, index: usize, linked: bool) -> Result<replay::Classes, Decline> {
        self.fields_for(self.notes.get(index).ok_or(Decline::Incomplete)?, linked)
    }

    pub(super) fn fields_for(&self, note: &TerminalNote, linked: bool) -> Result<replay::Classes, Decline> {
        use crate::live::certified::{F32Interval, F64Interval};
        let mut classes = [None; 2];
        for (class, out) in classes.iter_mut().enumerate() {
            let mut fields = [F32Interval::point(0.0).map_err(|_| Decline::Nonfinite)?; FIELDS];
            for (j, field) in fields.iter_mut().enumerate() {
                let (mut lower, mut upper) = (note.ordinary_lower[j], note.ordinary_upper[j]);
                if j == 1 {
                    let (probe_lower, probe_upper) = if linked {
                        // may_hold includes rows whose fixed condition might be false. Its positive
                        // amplitudes certify an upper sum, not a required active lower sum.
                        if class == 0 { (0.0, 0.0) } else { (0.0, self.probe_sum_upper) }
                    } else {
                        (-self.unconditional_probe_upper, self.unconditional_probe_upper)
                    };
                    if probe_lower != 0.0 {
                        lower = (lower + probe_lower).next_down();
                    }
                    upper = add_upper(upper, probe_upper)?;
                }
                if !lower.is_finite() || !upper.is_finite() {
                    return Err(Decline::Nonfinite);
                }
                *field = F32Interval::from_real(F64Interval::new(lower, upper).map_err(|_| Decline::Nonfinite)?);
                if !field.lower().is_finite() || !field.upper().is_finite() {
                    return Err(Decline::Nonfinite);
                }
            }
            *out = Some(fields);
        }
        Ok(classes)
    }

    /// Native ordinary prefixes of the notes newly executed between two adjacent, filing-free queries.
    /// The caller proves that query shape and supplies their actual score-frame endpoints. Commands and
    /// notes keep original event ordinals; later filings never enter an earlier ideal prefix. The complete
    /// trace's drift already encloses every earlier execution and every intermediate native field state.
    pub(super) fn query_notes(
        &self,
        trace: &BoundsTrace,
        query_event: usize,
        after: i32,
        through: i32,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Box<[TerminalNote]>, Decline> {
        if after < 0 || through < after || through as usize >= trace.frames {
            return Err(Decline::FrameIndex);
        }
        if !matches!(trace.events.get(query_event), Some(BoundsEvent::Query { to, .. }) if *to == through) {
            return Err(Decline::Incomplete);
        }
        let mut notes = Vec::new();
        for (ordinal, event) in trace.events.iter().take(query_event).enumerate() {
            if ordinal.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            if let BoundsEvent::Note { frame, note, .. } = event
                && *frame > after as usize
                && *frame <= through as usize
            {
                notes.try_reserve(1).map_err(|_| Decline::Capacity)?;
                notes.push((*frame, note.time_ms, note.note_id, ordinal));
            }
        }
        notes.sort_unstable();
        let mut prefix = [Interval::point(0.0)?; FIELDS];
        for (field, &initial) in prefix.iter_mut().zip(&self.initial) {
            *field = Interval::point(initial)?;
        }
        let mut next = 0;
        let mut out = Vec::new();
        out.try_reserve_exact(notes.len()).map_err(|_| Decline::Capacity)?;
        for (index, &(frame, time_ms, _, ordinal)) in notes.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            while let Some(&((f, time, _, filed), deltas)) = self.ordinary.get(next) {
                if (f, time) > (frame, time_ms) {
                    break;
                }
                if next.is_multiple_of(64) && cancelled() {
                    return Err(Decline::Cancelled);
                }
                if filed < query_event {
                    for (field, delta) in prefix.iter_mut().zip(deltas) {
                        field.add_exact(delta)?;
                    }
                }
                next += 1;
            }
            let mut ordinary_lower = [0.0; FIELDS];
            let mut ordinary_upper = [0.0; FIELDS];
            for j in 0..FIELDS {
                ordinary_lower[j] =
                    if self.drift[j] == 0.0 { prefix[j].lower } else { (prefix[j].lower - self.drift[j]).next_down() };
                ordinary_upper[j] = add_upper(prefix[j].upper, self.drift[j])?;
            }
            out.push(TerminalNote { time_ms, event: ordinal, ordinary_lower, ordinary_upper });
        }
        if cancelled() {
            return Err(Decline::Cancelled);
        }
        Ok(out.into_boxed_slice())
    }

    /// Valid without linking a terminal DP bucket to a probe class. The retained common untimed-row admission
    /// must still prove alternating exact inverse commands and their ordered opportunity mapping.
    pub(crate) fn unconditional_note_upper(&self, index: usize) -> Option<f64> {
        add_upper(self.notes.get(index)?.ordinary_upper[1], self.unconditional_probe_upper).ok()
    }

    /// Arithmetic for probe-off/probe-on classes. This method does NOT establish that a DP joint bucket has
    /// this probe state at this note. Only the outer private capability may attach that proven mapping;
    /// common LUCK gate, direct-row matching, command timing, and terminal readiness are separate obligations.
    pub(crate) fn linked_probe_note_upper(&self, index: usize) -> Option<[f64; 2]> {
        let off = self.notes.get(index)?.ordinary_upper[1];
        Some([off, add_upper(off, self.probe_sum_upper).ok()?])
    }
}

/// Complete ordinary prefixes for every terminal note. The magnitude proof for the variable direct-probe
/// part uses alternating inverse commands, not their outcome probabilities: partition all possible Probe
/// events into nondecreasing (score frame, chart time) runs. For each row, every score prefix within a run is
/// a contiguous prefix of alternating +/-v commands, whose magnitude is at most |v|. Sum over runs and rows.
/// Backdated finish clamps add runs instead of silently receiving a one-active-row magnitude assumption.
///
/// The ordinary magnitude uses a final lifetime L1, so it remains conservative through arbitrary late ordinary
/// filings and transient command states. A future exact ordinary-prefix maximum may tighten it independently.
/// This builder must only receive complete deterministic ordinary Factor events; power commands are rejected
/// by its adapter. It is not a public or independently constructible search proof capability.
pub(crate) fn build(
    frames: usize,
    initial: [f32; FIELDS],
    probe_values: &[f32],
    events: &[TimedEvent],
    mut cancelled: impl FnMut() -> bool,
) -> Result<TerminalIngredients, Decline> {
    let certificate = trace_drift::compile(
        frames,
        initial,
        probe_values,
        events.iter().copied().map(TimedEvent::counting),
        &mut cancelled,
    )?;
    #[cfg(feature = "search-diagnostics")]
    let work = certificate.fields;
    let to = certificate.queries.last().ok_or(Decline::Incomplete)?.to;
    let mut ordinary = Vec::<OrdinaryCommand>::new();
    let mut notes = Vec::<(usize, i32, i32, usize)>::new();
    let mut ordinary_l1 = [0.0; FIELDS];
    let mut last_probe = None;
    let mut probe_runs = 0usize;
    for (ordinal, event) in events.iter().copied().enumerate() {
        if ordinal % 64 == 0 && cancelled() {
            return Err(Decline::Cancelled);
        }
        match event {
            TimedEvent::Factor { frame, time_ms, owner, deltas } => {
                ordinary.try_reserve(1).map_err(|_| Decline::Capacity)?;
                ordinary.push(((frame, time_ms, owner, ordinal), deltas));
                for j in 0..FIELDS {
                    ordinary_l1[j] = add_upper(ordinary_l1[j], f64::from(deltas[j]).abs())?;
                }
            }
            TimedEvent::Note { frame, note_id, time_ms } if frame as i32 <= to => {
                notes.try_reserve(1).map_err(|_| Decline::Capacity)?;
                notes.push((frame, time_ms, note_id, ordinal));
            }
            TimedEvent::Probe { frame, time_ms } => {
                let key = (frame, time_ms);
                if last_probe.is_none_or(|previous| key < previous) {
                    probe_runs = probe_runs.checked_add(1).ok_or(Decline::CountOverflow)?;
                }
                last_probe = Some(key);
            }
            TimedEvent::Note { .. } | TimedEvent::Potential { .. } | TimedEvent::Query { .. } | TimedEvent::Other => {}
        }
    }
    let mut probe_sum = Interval::point(0.0)?;
    let mut probe_l1 = 0.0;
    for &value in probe_values {
        probe_sum.add_exact(value)?;
        probe_l1 = add_upper(probe_l1, f64::from(value).abs())?;
    }
    let run_count = if probe_runs == 0 { 0.0 } else { (probe_runs as f64).next_up() };
    let unconditional_probe_upper = multiply_upper(run_count, probe_l1)?;
    let mut state_magnitude = [0.0; FIELDS];
    for j in 0..FIELDS {
        state_magnitude[j] = add_upper(f64::from(initial[j]).abs(), ordinary_l1[j])?;
    }
    state_magnitude[1] = add_upper(state_magnitude[1], unconditional_probe_upper)?;

    // This optional exact-real bound retains every historical ordinary filed set and every internal
    // command prefix. The proved lifetime-L1 magnitude remains valid on allocation or numeric refusal.
    match super::ordinary_magnitude::bound(initial, events, 4 * 1024 * 1024, &mut cancelled) {
        Ok(mut ordinary_magnitude) => {
            if let Ok(note_magnitude) = add_upper(ordinary_magnitude[1], unconditional_probe_upper) {
                ordinary_magnitude[1] = note_magnitude;
                for (current, tighter) in state_magnitude.iter_mut().zip(ordinary_magnitude) {
                    *current = current.min(tighter);
                }
            }
        }
        Err(Decline::Cancelled) => return Err(Decline::Cancelled),
        Err(_) => {}
    }
    let drift = certificate.native_drift_with_state_bound(state_magnitude)?;

    ordinary.sort_unstable_by_key(|&(key, _)| key);
    notes.sort_unstable();
    let mut prefix = [Interval::point(0.0)?; FIELDS];
    for j in 0..FIELDS {
        prefix[j] = Interval::point(initial[j])?;
    }
    let mut next = 0;
    let mut terminal = Vec::new();
    terminal.try_reserve_exact(notes.len()).map_err(|_| Decline::Capacity)?;
    for (index, &(frame, time_ms, note_id, ordinal)) in notes.iter().enumerate() {
        if index % 64 == 0 && cancelled() {
            return Err(Decline::Cancelled);
        }
        while let Some(&((f, time, _, _), deltas)) = ordinary.get(next) {
            if (f, time) > (frame, time_ms) {
                break;
            }
            for j in 0..FIELDS {
                prefix[j].add_exact(deltas[j])?;
            }
            next += 1;
        }
        let mut ordinary_lower = [0.0; FIELDS];
        let mut ordinary_upper = [0.0; FIELDS];
        for j in 0..FIELDS {
            ordinary_lower[j] =
                if drift[j] == 0.0 { prefix[j].lower } else { (prefix[j].lower - drift[j]).next_down() };
            ordinary_upper[j] = add_upper(prefix[j].upper, drift[j])?;
        }
        terminal.push((
            (time_ms, note_id, ordinal),
            TerminalNote { time_ms, event: ordinal, ordinary_lower, ordinary_upper },
        ));
    }
    terminal.sort_unstable_by_key(|&(key, _)| key);
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    Ok(TerminalIngredients {
        notes: terminal.into_iter().map(|(_, note)| note).collect::<Vec<_>>().into_boxed_slice(),
        #[cfg(feature = "search-diagnostics")]
        work,
        drift,
        #[cfg(feature = "search-diagnostics")]
        state_magnitude,
        probe_time_runs: probe_runs,
        initial,
        ordinary: ordinary.into_boxed_slice(),
        probe_sum_upper: probe_sum.upper,
        unconditional_probe_upper,
    })
}

/// Private result attached only to an already admitted terminal probability capability.
pub(super) struct Prepared {
    pub(super) factors: Vec<[f64; 2]>,
    pub(super) ingredients: TerminalIngredients,
    pub(super) linked: bool,
    #[cfg(feature = "search-diagnostics")]
    pub(super) profile: super::LuckScoreProfile,
}

/// Preserve the exact complete trace projection. The initial fields are captured before recording. The outer
/// prepass proves deterministic recording, one untimed instance/inverse per row, terminal readiness and the
/// note-time multiset; this optional step does not run simulation or factor-history replay again.
pub(super) fn from_trace(
    trace: &BoundsTrace,
    initial: [f32; FIELDS],
    times: &[i32],
    probe_gate: Option<i64>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Prepared, Decline> {
    if trace.filing_gate.is_none() || initial[0] != 0.0 {
        return Err(Decline::Magnitude);
    }
    let mut events = Vec::new();
    events.try_reserve_exact(trace.events.len()).map_err(|_| Decline::Capacity)?;
    for (index, event) in trace.events.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        events.push(match event {
            BoundsEvent::Note { frame, note, .. } => {
                TimedEvent::Note { frame: *frame, note_id: note.note_id, time_ms: note.time_ms }
            }
            BoundsEvent::Factor { frame, command } => {
                // The FineView consumer uses a one-sided note factor with a nonnegative combo envelope.
                // Keep unrelated floating combo-score histories on the previous cap route.
                if command.band_total_power != 0 || command.combo_mill != 0 {
                    return Err(Decline::Magnitude);
                }
                TimedEvent::Factor {
                    frame: *frame,
                    time_ms: command.time_ms,
                    owner: command.owner_id,
                    deltas: replay::command_deltas(command),
                }
            }
            BoundsEvent::Potential { frame } => TimedEvent::Potential { frame: *frame },
            BoundsEvent::Probe { frame, time_ms } => TimedEvent::Probe { frame: *frame, time_ms: *time_ms },
            BoundsEvent::Query { to, .. } => TimedEvent::Query { to: *to },
            BoundsEvent::Combo { .. } | BoundsEvent::ProbabilityReady(_) | BoundsEvent::Rank { .. } => {
                TimedEvent::Other
            }
        });
    }
    let values: Vec<_> = trace.probes.iter().map(|row| row.value).collect();
    if values.iter().any(|&value| !value.is_finite() || value <= i32::MIN as f32 / 100000f32) {
        return Err(Decline::Magnitude);
    }
    let ingredients = build(trace.frames, initial, &values, &events, &mut *cancelled)?;
    if !ingredients.notes.iter().map(|note| note.time_ms).eq(times.iter().copied()) {
        return Err(Decline::Incomplete);
    }
    // One nondecreasing possible-filing run prevents a finish clamp from reordering a probe end before an
    // earlier note. Other cases retain the full unconditional probe magnitude in both DP score classes.
    let linked = probe_gate == Some(super::gekisou::M_LUCK)
        && ingredients.probe_time_runs <= 1
        && values.iter().all(|&value| value >= 0.0);
    let mut factors = Vec::new();
    factors.try_reserve_exact(times.len()).map_err(|_| Decline::Capacity)?;
    for index in 0..times.len() {
        if index.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        let upper = if linked {
            ingredients.linked_probe_note_upper(index)
        } else {
            ingredients.unconditional_note_upper(index).map(|upper| [upper; 2])
        }
        .ok_or(Decline::Nonfinite)?;
        factors.push(upper);
    }
    #[cfg(feature = "search-diagnostics")]
    let profile = super::LuckScoreProfile {
        terminal_factor_builds: 1,
        terminal_factor_additions: ingredients.work.iter().map(|field| field.additions).sum(),
        terminal_factor_undos: ingredients.work.iter().map(|field| field.undo_subtractions).sum(),
        terminal_factor_probe_runs: ingredients.probe_time_runs as u64,
        terminal_factor_maximum_state: ingredients.state_magnitude.into_iter().fold(0.0, f64::max),
        terminal_factor_maximum_drift: ingredients.drift.into_iter().fold(0.0, f64::max),
        ..Default::default()
    };
    Ok(Prepared {
        factors,
        ingredients,
        linked,
        #[cfg(feature = "search-diagnostics")]
        profile,
    })
}
