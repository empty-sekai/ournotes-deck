//! Operation-count bounds for an already admitted, complete factor recording.
//!
//! This module proves roundoff allowances. It does not prove recorder admission, probability readiness,
//! note caps, rank arithmetic, a score law, or search completion. The adapter must preserve every original
//! query and every possible filing. No lottery probability is used to remove an operation.

const FIELDS: usize = 6;
const UNIT: f64 = 1.0 / 16_777_216.0;
const HALF_SUBNORMAL: f64 = f64::from_bits(873u64 << 52); // 2^-150.

/// The only trace inputs that affect operation counts. `Factor` deltas are the native binary32 values,
/// including their original signed command conversion. Every Probe can file one command per admitted row.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Note { frame: usize },
    Factor { frame: usize, deltas: [f32; FIELDS] },
    Potential { frame: usize },
    Probe { frame: usize },
    Query { to: i32 },
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decline {
    Cancelled,
    Incomplete,
    FrameIndex,
    CountOverflow,
    Nonfinite,
    Magnitude,
    Feedback,
    Capacity,
}

/// A range containing every actual native execution for this query. `from..=previous` contains every
/// undone frame, and `from..=to` every executed frame. Reversed ranges are empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueryRange {
    #[cfg(test)]
    pub(crate) previous: i32,
    pub(crate) to: i32,
    #[cfg(test)]
    pub(crate) from: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    commands: [u64; FIELDS],
    l1: [f64; FIELDS],
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FieldWork {
    /// Both the state and its FrameDiff execute at most this many nonzero additions.
    pub(crate) additions: u64,
    /// Undos of frames that could contain a nonzero command in this field.
    pub(crate) undo_subtractions: u64,
    /// Sum_f E_f C_f B_f, with B_f the final possible command L1 of frame f.
    pub(crate) diff_magnitude_weight: f64,
    /// Every possible lifetime command contributes once, regardless of how often it is replayed.
    pub(crate) lifetime_l1: f64,
    /// Maximum final possible command L1 of any one frame.
    pub(crate) maximum_frame_l1: f64,
}

#[derive(Debug)]
pub(crate) struct Certificate {
    pub(crate) fields: [FieldWork; FIELDS],
    pub(crate) queries: Box<[QueryRange]>,
    initial: [f32; FIELDS],
}

fn nonnegative(value: f64) -> Result<f64, Decline> {
    if value.is_finite() && value >= 0.0 { Ok(value) } else { Err(Decline::Nonfinite) }
}

fn plus(a: f64, b: f64) -> Result<f64, Decline> {
    nonnegative(a)?;
    nonnegative(b)?;
    if a == 0.0 {
        return Ok(b);
    }
    if b == 0.0 {
        return Ok(a);
    }
    nonnegative((a + b).next_up())
}

fn times(a: f64, b: f64) -> Result<f64, Decline> {
    nonnegative(a)?;
    nonnegative(b)?;
    if a == 0.0 || b == 0.0 {
        return Ok(0.0);
    }
    nonnegative((a * b).next_up())
}

fn count_as_upper(count: u64) -> f64 {
    if count == 0 { 0.0 } else { (count as f64).next_up() }
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, Decline> {
    let mut out = Vec::new();
    out.try_reserve_exact(count).map_err(|_| Decline::Capacity)?;
    out.resize(count, value);
    Ok(out)
}

fn add_range(diff: &mut [i64], from: i32, to: i32) -> Result<(), Decline> {
    if from > to {
        return Ok(());
    }
    let from = usize::try_from(from).map_err(|_| Decline::FrameIndex)?;
    let until = usize::try_from(to).map_err(|_| Decline::FrameIndex)?.checked_add(1).ok_or(Decline::FrameIndex)?;
    if until >= diff.len() {
        return Err(Decline::FrameIndex);
    }
    diff[from] = diff[from].checked_add(1).ok_or(Decline::CountOverflow)?;
    diff[until] = diff[until].checked_sub(1).ok_or(Decline::CountOverflow)?;
    Ok(())
}

fn add_command(frame: &mut Frame, deltas: [f32; FIELDS]) -> Result<(), Decline> {
    for (field, delta) in deltas.into_iter().enumerate() {
        if !delta.is_finite() {
            return Err(Decline::Nonfinite);
        }
        // Adding numeric zero cannot create nonzero roundoff. Signed zero is deliberately irrelevant to this
        // real-valued error certificate; this is not an identity key or a bitwise state-merging authority.
        if delta != 0.0 {
            frame.commands[field] = frame.commands[field].checked_add(1).ok_or(Decline::CountOverflow)?;
            frame.l1[field] = plus(frame.l1[field], f64::from(delta).abs())?;
        }
    }
    Ok(())
}

/// Compile exact integer range counts and outward real magnitude sums. Temporary storage is O(F), and the
/// retained query ranges are O(Q). Each event is visited once; no factor classes or replay paths are built.
///
/// Preconditions supplied by the caller: ordinary command/note recording is identical across all admitted
/// lottery paths; every optional command filing is represented by a Potential/Probe at the correct score frame;
/// direct untimed probe rows each file at most one signed command at one represented opportunity; calculator
/// state starts fresh. Preserve duplicate Probe events: counting them again is conservative.
pub(crate) fn compile(
    frames: usize,
    initial: [f32; FIELDS],
    probe_values: &[f32],
    events: impl IntoIterator<Item = Event>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Certificate, Decline> {
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    if frames == 0 || frames > i32::MAX as usize {
        return Err(Decline::FrameIndex);
    }
    if initial.iter().chain(probe_values).any(|x| !x.is_finite()) {
        return Err(Decline::Nonfinite);
    }
    let mut frame = filled(frames, Frame::default())?;
    let mut executions = filled(frames + 1, 0i64)?;
    let mut undos = filled(frames + 1, 0i64)?;
    let mut queries = Vec::new();
    let (mut previous, mut added) = (-1i32, None::<i32>);
    for (index, event) in events.into_iter().enumerate() {
        if index % 64 == 0 && cancelled() {
            return Err(Decline::Cancelled);
        }
        let filing = match event {
            Event::Note { frame } | Event::Potential { frame } => Some(frame),
            Event::Factor { frame: f, deltas } => {
                add_command(frame.get_mut(f).ok_or(Decline::FrameIndex)?, deltas)?;
                Some(f)
            }
            Event::Probe { frame: f } => {
                let destination = frame.get_mut(f).ok_or(Decline::FrameIndex)?;
                for &value in probe_values {
                    let mut deltas = [0.0; FIELDS];
                    deltas[1] = value;
                    add_command(destination, deltas)?;
                }
                Some(f)
            }
            Event::Query { to } => {
                if to < 0 || to as usize >= frames {
                    return Err(Decline::FrameIndex);
                }
                let u = added.map_or(to, |a| to.min(a - 1));
                let from = previous.min(u) + 1;
                add_range(&mut undos, from, previous)?;
                add_range(&mut executions, from, to)?;
                queries.try_reserve(1).map_err(|_| Decline::Capacity)?;
                queries.push(QueryRange {
                    #[cfg(test)]
                    previous,
                    to,
                    #[cfg(test)]
                    from,
                });
                previous = to;
                added = None;
                None
            }
            Event::Other => None,
        };
        if let Some(f) = filing {
            if f >= frames {
                return Err(Decline::FrameIndex);
            }
            let f = f as i32;
            added = Some(added.map_or(f, |old| old.min(f)));
        }
    }
    if queries.is_empty() || added.is_some() {
        return Err(Decline::Incomplete);
    }
    let mut fields = [FieldWork::default(); FIELDS];
    let (mut execute_count, mut undo_count) = (0i64, 0i64);
    for f in 0..frames {
        if f % 64 == 0 && cancelled() {
            return Err(Decline::Cancelled);
        }
        execute_count = execute_count.checked_add(executions[f]).ok_or(Decline::CountOverflow)?;
        undo_count = undo_count.checked_add(undos[f]).ok_or(Decline::CountOverflow)?;
        let e = u64::try_from(execute_count).map_err(|_| Decline::CountOverflow)?;
        let u = u64::try_from(undo_count).map_err(|_| Decline::CountOverflow)?;
        for (field, out) in fields.iter_mut().enumerate() {
            let c = frame[f].commands[field];
            let ec = e.checked_mul(c).ok_or(Decline::CountOverflow)?;
            out.additions = out.additions.checked_add(ec).ok_or(Decline::CountOverflow)?;
            if c > 0 {
                out.undo_subtractions = out.undo_subtractions.checked_add(u).ok_or(Decline::CountOverflow)?;
            }
            out.diff_magnitude_weight =
                plus(out.diff_magnitude_weight, times(count_as_upper(ec), frame[f].l1[field])?)?;
            out.lifetime_l1 = plus(out.lifetime_l1, frame[f].l1[field])?;
            out.maximum_frame_l1 = out.maximum_frame_l1.max(frame[f].l1[field]);
        }
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    Ok(Certificate { fields, queries: queries.into_boxed_slice(), initial })
}

impl Certificate {
    /// Universal, potentially loose allowance against the exact-real sum of native binary32 deltas. It needs
    /// no signed-pair cancellation, positive-factor assumption or probability-class magnitude restriction.
    #[cfg(test)]
    pub(crate) fn universal_native_drift(&self) -> Result<[f64; FIELDS], Decline> {
        let mut state = [0.0; FIELDS];
        for (j, magnitude) in state.iter_mut().enumerate() {
            *magnitude = plus(f64::from(self.initial[j]).abs(), self.fields[j].lifetime_l1)?;
        }
        self.native_drift_with_state_bound(state)
    }

    /// Use an independently proved bound on the absolute EXACT-REAL factor state at every native state add
    /// and undo, across all intermediate command positions and histories. A note-only/window-at-query maximum
    /// is insufficient. The bound is for native binary32 deltas treated as exact real numbers.
    ///
    /// Each FrameDiff error is consumed by at most one undo before that diff is reset. The state error is thus
    /// a sum of distinct local rounding errors with coefficients in {-1, 0, 1}; no local error is multiplied by
    /// the number of future undos. For A additions, U relevant undos, N=2A+U and W the L1 of local errors,
    ///
    /// W <= unit * ((A+U)*M + sum_f E_f*C_f*B_f + N*W) + N*2^-150.
    ///
    /// The finite bound below closes this inequality. Every operation input is then strictly below f32::MAX,
    /// proving that a first overflow could not have occurred. Unprovable feedback or overflow declines.
    pub(crate) fn native_drift_with_state_bound(&self, state: [f64; FIELDS]) -> Result<[f64; FIELDS], Decline> {
        let mut result = [0.0; FIELDS];
        for (j, out) in result.iter_mut().enumerate() {
            let work = self.fields[j];
            let m = nonnegative(state[j])?;
            if m < f64::from(self.initial[j]).abs() {
                return Err(Decline::Magnitude);
            }
            let au = work.additions.checked_add(work.undo_subtractions).ok_or(Decline::CountOverflow)?;
            let n = work
                .additions
                .checked_mul(2)
                .and_then(|x| x.checked_add(work.undo_subtractions))
                .ok_or(Decline::CountOverflow)?;
            if n == 0 {
                continue;
            }
            let alpha = times(count_as_upper(n), UNIT)?;
            if alpha >= 1.0 {
                return Err(Decline::Feedback);
            }
            let denominator = (1.0 - alpha).next_down();
            if denominator <= 0.0 {
                return Err(Decline::Feedback);
            }
            let magnitude = plus(times(count_as_upper(au), m)?, work.diff_magnitude_weight)?;
            let numerator = plus(times(UNIT, magnitude)?, times(count_as_upper(n), HALF_SUBNORMAL)?)?;
            let drift = nonnegative((numerator / denominator).next_up())?;
            if plus(m.max(work.maximum_frame_l1), drift)? >= f64::from(f32::MAX) {
                return Err(Decline::Nonfinite);
            }
            *out = drift;
        }
        Ok(result)
    }

    /// Allowance against a mill/100000 exact-real reference, suitable for a caller whose window magnitudes
    /// use that reference. Every source delta MUST have arisen from a signed integer mill converted by the
    /// native `mill as f32 / 100000f32`; the initial fields must have the same exact reference values.
    /// `ideal_state` must independently enclose every ideal intermediate state, not just scored notes.
    ///
    /// Two correctly rounded conversions give |native-reference| <= gamma_2*|reference|. Consequently it is
    /// <= [2u/(1-4u)]*|native|. The lifetime L1 charges this representation once per possible filed command;
    /// it is not charged once per reexecution. No probe pairing is assumed to cancel representation errors.
    #[cfg(test)]
    pub(crate) fn mill_reference_drift(&self, ideal_state: [f64; FIELDS]) -> Result<[f64; FIELDS], Decline> {
        let denominator = (1.0 - times(4.0, UNIT)?).next_down();
        let rho = nonnegative((times(2.0, UNIT)? / denominator).next_up())?;
        let mut representation = [0.0; FIELDS];
        let mut native_state = [0.0; FIELDS];
        for j in 0..FIELDS {
            representation[j] = times(rho, self.fields[j].lifetime_l1)?;
            native_state[j] = plus(ideal_state[j], representation[j])?;
        }
        let mut result = self.native_drift_with_state_bound(native_state)?;
        for j in 0..FIELDS {
            result[j] = plus(result[j], representation[j])?;
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "trace_drift_tests.rs"]
mod tests;
