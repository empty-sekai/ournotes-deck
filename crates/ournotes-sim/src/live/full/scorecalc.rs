//! Incremental live score: commands filed in 40 ms frames, frames executed as the music time advances, and frames
//! undone and executed again when a command lands in a frame that was already executed.
//!
//! Undoing a frame subtracts its note scores and, per factor field, the float sum of the factor changes the frame
//! applied (one subtraction per field), so the float state after an undo can differ in the last bits from the state
//! before the frame ran; the score follows that state exactly.
//!
//! A fixed score (the Gekisou rank bonus) is filed at the next calculation under the frame of its time and added to
//! the score at once; when that frame is undone and executed again, it is also counted as rank bonus score.

use super::combo::ComboCounter;
use super::luck_score_bounds::{BoundsEvent, BoundsTrace, ProbeRow};
use super::score_program::{Kernel, Recorder, ScoreProgram, ValueId};
use crate::error::Error;
use crate::live::score::{GekisouComboInfo, LiveScoreCalculator, ScoreFactorState, get_frame};
use crate::live::skill::{FactorCommand, apply_factor};
use std::fmt;

/// Frames kept after the music length.
const EXTRA_FRAMES: i32 = 50;

/// The factor changes one frame applied.
#[derive(Clone, Copy, Debug, Default)]
struct FrameDiff {
    band_total_power: i32,
    combo: f32,
    note: f32,
    just: f32,
    perfect: f32,
    great: f32,
    good: f32,
    luck: i32,
}

fn mill(m: i32) -> f32 {
    if m != 0 { m as f32 / 100000f32 } else { 0f32 }
}

impl FrameDiff {
    fn is_bitwise_zero(&self) -> bool {
        let Self { band_total_power, combo, note, just, perfect, great, good, luck } = self;
        *band_total_power == 0
            && *luck == 0
            && [combo, note, just, perfect, great, good].iter().all(|value| value.to_bits() == 0)
    }

    fn add(&mut self, cmd: &FactorCommand) {
        self.band_total_power = self.band_total_power.wrapping_add(cmd.band_total_power);
        self.luck = self.luck.wrapping_add(cmd.luck);
        let (fc, fn_, fj) = (mill(cmd.combo_mill), mill(cmd.note_mill), mill(cmd.judge_mill));
        if fc != 0.0 {
            self.combo += fc;
        }
        if fn_ != 0.0 {
            self.note += fn_;
        }
        match cmd.judgement {
            6 => self.just += fj,
            5 => self.perfect += fj,
            4 => self.great += fj,
            3 => self.good += fj,
            _ => {}
        }
    }

    fn undo(&mut self, s: &mut ScoreFactorState) {
        s.band_total_power = s.band_total_power.wrapping_sub(self.band_total_power);
        s.just -= self.just;
        s.perfect -= self.perfect;
        s.combo_score_up -= self.combo;
        s.note_score_up -= self.note;
        s.added_luck_bonus = s.added_luck_bonus.wrapping_sub(self.luck);
        s.great -= self.great;
        s.good -= self.good;
        *self = FrameDiff::default();
    }
}

/// Diagnostics only: `(time, note id, last executed score, [Gekisou combo, combo, score-up factor])`.
#[cfg(feature = "search-diagnostics")]
pub type FiledNote = (i32, i32, i32, [f32; 3]);

/// A note score command: chart time, the life frozen at the judgement, note id, note type and score type.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NoteCommand {
    pub time_ms: i32,
    pub life: i32,
    pub note_id: i32,
    pub note_type: i32,
    pub score_type: i32,
    added: i32,
    /// Diagnostics only: the last execution's Gekisou combo factor, combo factor and score-up factor.
    #[cfg(feature = "search-diagnostics")]
    factors: [f32; 3],
    /// Tests only: the factor state of the last execution [combo, note, Just, Perfect, Great, Good].
    #[cfg(test)]
    state: [f32; 6],
}

impl NoteCommand {
    /// Every input the bounds replay reads from a filed note. The native accumulator and diagnostic fields
    /// describe a later execution; the independent replay never reads them.
    pub(super) fn bounds_identity(&self) -> [i32; 5] {
        let Self {
            time_ms,
            life,
            note_id,
            note_type,
            score_type,
            added: _,
            #[cfg(feature = "search-diagnostics")]
                factors: _,
            #[cfg(test)]
                state: _,
        } = self;
        [*time_ms, *life, *note_id, *note_type, *score_type]
    }

    pub(crate) fn new(time_ms: i32, life: i32, note_id: i32, note_type: i32, score_type: i32) -> NoteCommand {
        NoteCommand {
            time_ms,
            life,
            note_id,
            note_type,
            score_type,
            added: 0,
            #[cfg(feature = "search-diagnostics")]
            factors: [0.0; 3],
            #[cfg(test)]
            state: [0.0; 6],
        }
    }
}

#[derive(Clone)]
pub(crate) struct IncrementalCalculator {
    pub(super) minimum_score_up: Option<f32>,
    pub(super) bounds_trace: Option<BoundsTrace>,
    bounds_record_only: Option<BoundsRecordOnly>,
    program: Option<Recorder>,
    pub calc: LiveScoreCalculator,
    max_frame: i32,
    notes: Vec<Vec<NoteCommand>>,
    factors: Vec<Vec<FactorCommand>>,
    diffs: Vec<FrameDiff>,
    prev: i32,
    added: i32,
    pub score: i32,
    /// Filed fixed scores `(frame, score)`.
    fixed: Vec<(i32, i32)>,
    pending_fixed: Option<(i32, i32)>,
    /// Fixed scores counted again by frames executed after an undo.
    pub rank_bonus: i32,
    order_f: Vec<usize>,
    order_n: Vec<usize>,
    /// Search cutoff: frames below `settled_frame` were declared final by [`IncrementalCalculator::settle`]; their note
    /// and fixed scores sum to `settled_total`, of which `settled_fixed` are fixed scores.
    settled_frame: usize,
    settled_total: i64,
    settled_fixed: i64,
    /// Diagnostics only: undos of a frame already declared settled (a broken settledness argument).
    #[cfg(feature = "search-diagnostics")]
    pub settled_violations: u64,
}

/// Private, one-way structural recording. A malformed note must still fail at the first query that would
/// execute it. Valid notes need no retained entry; their immutable percentage-map lookups were checked once.
#[derive(Clone, Debug, Default)]
struct BoundsRecordOnly {
    invalid_notes: Vec<(usize, usize)>,
    capacity_failed: bool,
    #[cfg(feature = "search-diagnostics")]
    diagnostic_started: bool,
}

/// A recorded pair is the ordinary combo factor before skill additions and the Gekisou combo factor.
/// The private exact replay supplies the snapshot belonging to this query, indexed by original filing order.
enum NoteInputs<'a> {
    Native { combo: &'a ComboCounter, gekisou: Option<&'a dyn GekisouComboInfo> },
    Recorded(&'a [Vec<Option<(f32, f32)>>]),
}

/// Empty frame storage has a lossless length representation. Nonempty entries keep their complete state.
struct FrameLists<'a, T>(&'a [Vec<T>]);

impl<T: fmt::Debug> fmt::Debug for FrameLists<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.iter().all(Vec::is_empty) {
            f.debug_tuple("EmptyFrameLists").field(&self.0.len()).finish()
        } else {
            fmt::Debug::fmt(self.0, f)
        }
    }
}

struct FrameDiffs<'a>(&'a [FrameDiff]);

impl fmt::Debug for FrameDiffs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.iter().all(FrameDiff::is_bitwise_zero) {
            f.debug_tuple("ZeroFrameDiffs").field(&self.0.len()).finish()
        } else {
            fmt::Debug::fmt(self.0, f)
        }
    }
}

impl fmt::Debug for IncrementalCalculator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Exhaustive destructuring keeps every field in the identity when the calculator changes.
        // Compression applies only to exact empty/bitwise-zero arrays, and always includes their lengths.
        let Self {
            minimum_score_up,
            bounds_trace,
            bounds_record_only,
            program,
            calc,
            max_frame,
            notes,
            factors,
            diffs,
            prev,
            added,
            score,
            fixed,
            pending_fixed,
            rank_bonus,
            order_f,
            order_n,
            settled_frame,
            settled_total,
            settled_fixed,
            #[cfg(feature = "search-diagnostics")]
            settled_violations,
        } = self;
        let mut view = f.debug_struct("IncrementalCalculator");
        view.field("minimum_score_up", minimum_score_up)
            .field("bounds_trace", bounds_trace)
            .field("bounds_record_only", bounds_record_only)
            .field("program", program)
            .field("calc", calc)
            .field("max_frame", max_frame)
            .field("notes", &FrameLists(notes))
            .field("factors", &FrameLists(factors))
            .field("diffs", &FrameDiffs(diffs))
            .field("prev", prev)
            .field("added", added)
            .field("score", score)
            .field("fixed", fixed)
            .field("pending_fixed", pending_fixed)
            .field("rank_bonus", rank_bonus)
            .field("order_f", order_f)
            .field("order_n", order_n)
            .field("settled_frame", settled_frame)
            .field("settled_total", settled_total)
            .field("settled_fixed", settled_fixed);
        #[cfg(feature = "search-diagnostics")]
        view.field("settled_violations", settled_violations);
        view.finish()
    }
}

impl IncrementalCalculator {
    pub(crate) fn new(calc: LiveScoreCalculator, music_length_ms: i32) -> IncrementalCalculator {
        let max_frame = get_frame(music_length_ms) + EXTRA_FRAMES;
        let n = max_frame as usize;
        IncrementalCalculator {
            minimum_score_up: None,
            bounds_trace: None,
            bounds_record_only: None,
            program: None,
            calc,
            max_frame,
            notes: vec![Vec::new(); n],
            factors: vec![Vec::new(); n],
            diffs: vec![FrameDiff::default(); n],
            prev: -1,
            added: -1,
            score: 0,
            fixed: Vec::new(),
            pending_fixed: None,
            rank_bonus: 0,
            order_f: Vec::new(),
            order_n: Vec::new(),
            settled_frame: 0,
            settled_total: 0,
            settled_fixed: 0,
            #[cfg(feature = "search-diagnostics")]
            settled_violations: 0,
        }
    }

    /// Search cutoff: declares the frames below `frame` final, as far as they were executed, and returns the score they
    /// hold `(note and fixed scores, fixed scores)`. The caller guarantees that no later command lands in or rewinds
    /// to such a frame; frames only ever join the settled prefix.
    pub(crate) fn settle(&mut self, frame: usize) -> (i64, i64) {
        let executed = if self.prev < 0 { 0 } else { self.prev as usize + 1 };
        // Later commands clamp to the last addressable frame, so that frame remains open.
        let mut to = frame.min(executed).min(self.notes.len().saturating_sub(1));
        // A pending fixed score files at its frame with the next calculation.
        if let Some((t, _)) = self.pending_fixed {
            to = to.min(get_frame(t).max(0) as usize);
        }
        while self.settled_frame < to {
            let f = self.settled_frame;
            let notes: i64 = self.notes[f].iter().map(|n| i64::from(n.added)).sum();
            let fixed = self.fixed_at(f as i32).map_or(0, i64::from);
            self.settled_total += notes + fixed;
            self.settled_fixed += fixed;
            self.settled_frame += 1;
        }
        (self.settled_total, self.settled_fixed)
    }

    /// The first frame not yet declared settled.
    pub(crate) fn settled_frame(&self) -> usize {
        self.settled_frame
    }

    fn file(&mut self, t: i32) -> usize {
        let mut f = get_frame(t);
        if self.max_frame <= f {
            f = self.max_frame - 1;
        }
        self.added = if self.added < 0 { f } else { self.added.min(f) };
        f as usize
    }

    pub(crate) fn add_note(&mut self, cmd: NoteCommand) {
        let f = self.file(cmd.time_ms);
        if let Some(recording) = &mut self.bounds_record_only
            && (!self.calc.note_factor_percent.contains_key(&cmd.note_type)
                || !self.calc.judgement_score_factor_percent.contains_key(&cmd.score_type))
        {
            if recording.invalid_notes.try_reserve(1).is_err() {
                recording.capacity_failed = true;
            } else {
                recording.invalid_notes.push((f, self.notes[f].len()));
            }
        }
        if let Some(trace) = &mut self.bounds_trace {
            trace.events.push(BoundsEvent::Note { frame: f, index: self.notes[f].len(), note: cmd });
            trace.combo.filed(f);
        }
        self.notes[f].push(cmd);
        if let Some(program) = &mut self.program {
            program.add_note(f);
        }
    }

    pub(crate) fn add_factor(&mut self, cmd: FactorCommand) {
        let f = self.file(cmd.time_ms);
        if let Some(trace) = &mut self.bounds_trace {
            trace.events.push(BoundsEvent::Factor { frame: f, command: cmd });
        }
        self.factors[f].push(cmd);
    }

    pub(super) fn begin_bounds(&mut self, probes: Vec<ProbeRow>, has_luck: bool) {
        self.bounds_trace = Some(BoundsTrace {
            events: Vec::new(),
            queries: 0,
            frames: self.notes.len(),
            probes,
            combo: Default::default(),
            has_luck,
            filing_gate: None,
            probe_filings: None,
        });
    }

    /// Call only after the recorder's deterministic dependency closure and common untimed-probe gate are proved.
    pub(super) fn certify_bounds_filings(&mut self, gate: Option<i64>) {
        if let Some(trace) = &mut self.bounds_trace {
            trace.filing_gate = Some(gate);
            // Existing boundaries have no retroactive gate proof and stay possible.
            trace.probe_filings = Some(
                trace
                    .events
                    .iter()
                    .enumerate()
                    .filter_map(|(index, event)| matches!(event, BoundsEvent::Probe { .. }).then_some(index))
                    .collect(),
            );
        }
    }

    /// Only the private fresh solo-LUCK bounds model may call this after its deterministic admission.
    /// Native score, field, program and settled-prefix observations cannot consume this model afterwards.
    pub(super) fn try_enable_bounds_record_only(&mut self) -> bool {
        let expected = ScoreFactorState::new(self.calc.state.band_total_power);
        let actual = &self.calc.state;
        let initial_fields = [
            actual.combo_score_up.to_bits() == expected.combo_score_up.to_bits(),
            actual.note_score_up.to_bits() == expected.note_score_up.to_bits(),
            actual.just.to_bits() == expected.just.to_bits(),
            actual.perfect.to_bits() == expected.perfect.to_bits(),
            actual.great.to_bits() == expected.great.to_bits(),
            actual.good.to_bits() == expected.good.to_bits(),
        ];
        if self.bounds_record_only.is_some()
            || self.program.is_some()
            || self.minimum_score_up.is_some()
            || self.prev != -1
            || self.added != -1
            || self.score != 0
            || self.rank_bonus != 0
            || self.pending_fixed.is_some()
            || !self.fixed.is_empty()
            || self.settled_frame != 0
            || self.settled_total != 0
            || self.settled_fixed != 0
            || !self.order_f.is_empty()
            || !self.order_n.is_empty()
            || self.notes.iter().any(|notes| !notes.is_empty())
            || self.factors.iter().any(|factors| !factors.is_empty())
            || self.diffs.iter().any(|diff| !diff.is_bitwise_zero())
            || initial_fields.contains(&false)
            || actual.added_luck_bonus != 0
            || actual.gekisou_rank_bonus_score != 0
            || self.calc.converted_note_count <= 0
            || ![
                self.calc.score_adjustment_factor,
                self.calc.music_difficulty_factor,
                self.calc.life_onus_factor,
                self.calc.event_bonus_factor,
                self.calc.assist_factor,
            ]
            .iter()
            .all(|value| value.is_finite())
            || self.calc.luck_weight.as_ref().is_none_or(|weights| !weights.steps.is_empty())
            || !bounds_combo_table_is_valid(self.calc.combo_table.as_ref())
            || self.bounds_trace.as_ref().is_none_or(|trace| {
                !trace.has_luck || trace.filing_gate.is_none() || trace.queries != 0 || !trace.events.is_empty()
            })
        {
            return false;
        }
        self.bounds_record_only = Some(BoundsRecordOnly::default());
        true
    }

    /// The only fallible note-score operations omitted by structural recording are the immutable percentage
    /// lookups. Combo lookups remain in the unchanged query observer, with a structurally valid fresh table.
    fn validate_record_only_notes(&self, from: i32, to: i32) -> Result<(), Error> {
        let recording = self.bounds_record_only.as_ref().expect("private record-only mode");
        if self.program.is_some() || self.minimum_score_up.is_some() || self.settled_frame != 0 {
            return Err(Error::Unsupported("bounds-only recording cannot serve a numeric observer".into()));
        }
        if recording.capacity_failed {
            return Err(Error::Capacity("bounds-only note validation capacity".into()));
        }
        let first = recording
            .invalid_notes
            .iter()
            .copied()
            .filter(|&(frame, _)| from <= frame as i32 && frame as i32 <= to)
            .min_by_key(|&(frame, index)| {
                let note = &self.notes[frame][index];
                (frame, note.time_ms, note.note_id, index)
            });
        if let Some((frame, index)) = first {
            let note = &self.notes[frame][index];
            if !self.calc.note_factor_percent.contains_key(&note.note_type) {
                return Err(Error::Game(format!("note type {} has no score percent", note.note_type)));
            }
            if !self.calc.judgement_score_factor_percent.contains_key(&note.score_type) {
                return Err(Error::Game(format!("score type {} has no score percent", note.score_type)));
            }
        }
        Ok(())
    }

    pub(super) fn bounds_potential_rush(&mut self, time_ms: i32) {
        if self.bounds_trace.as_ref().is_none_or(|trace| !trace.has_luck) {
            return;
        }
        let frame = get_frame(time_ms).min(self.max_frame - 1) as usize;
        if let Some(trace) = &mut self.bounds_trace {
            trace.events.push(BoundsEvent::Potential { frame });
        }
    }

    pub(super) fn bounds_potential_skills(&mut self, time_ms: i32, possible: bool) {
        if self.bounds_trace.is_none() {
            return;
        }
        let frame = get_frame(time_ms).min(self.max_frame - 1) as usize;
        if let Some(trace) = &mut self.bounds_trace
            && !trace.probes.is_empty()
        {
            if possible && let Some(filings) = &mut trace.probe_filings {
                filings.push(trace.events.len());
            }
            trace.events.push(BoundsEvent::Probe { frame, time_ms });
        }
    }

    pub(super) fn bounds_last_query(&self) -> Option<usize> {
        self.bounds_trace.as_ref().and_then(|trace| trace.queries.checked_sub(1))
    }

    pub(super) fn bounds_probability_ready(&mut self, time_ms: i32) {
        if let Some(trace) = &mut self.bounds_trace {
            trace.events.push(BoundsEvent::ProbabilityReady(time_ms));
        }
    }

    pub(super) fn bounds_rank(
        &mut self,
        range: usize,
        time_ms: i32,
        percent: i64,
        start: Option<usize>,
        end: Option<usize>,
    ) {
        if let Some(trace) = &mut self.bounds_trace {
            trace.events.push(BoundsEvent::Rank { range, time_ms, percent, start, end });
        }
    }

    /// Sets the fixed score filed by the next calculation (only the last one set counts).
    pub(crate) fn add_fixed(&mut self, time_ms: i32, score: i32) {
        self.pending_fixed = Some((time_ms, score));
        if let Some(program) = &mut self.program {
            program.pending_literal(score);
        }
    }

    pub(crate) fn begin_program(&mut self) -> Result<(), Error> {
        if self.program.is_some()
            || self.prev != -1
            || self.added != -1
            || self.pending_fixed.is_some()
            || !self.fixed.is_empty()
            || self.notes.iter().any(|v| !v.is_empty())
            || self.factors.iter().any(|v| !v.is_empty())
        {
            return Err(Error::Input("score program requires a fresh score calculator".into()));
        }
        self.program = Some(Recorder::new(self.calc.state.band_total_power, self.notes.len()));
        Ok(())
    }

    pub(crate) fn finish_program(&mut self) -> Result<ScoreProgram, Error> {
        self.program.take().ok_or_else(|| Error::Input("score program was not started".into()))?.finish(self.score)
    }

    pub(crate) fn export_program(&self) -> Result<Option<std::sync::Arc<ScoreProgram>>, Error> {
        self.program.as_ref().map(|program| program.export(self.score)).transpose()
    }

    pub(crate) fn program_snapshot(&self) -> Option<ValueId> {
        self.program.as_ref().map(Recorder::snapshot)
    }

    pub(crate) fn record_rank_bonus(
        &mut self,
        start: Option<ValueId>,
        end: Option<ValueId>,
        pct: i64,
    ) -> Result<(), Error> {
        if let Some(program) = &mut self.program {
            program.pending_rank(start, end, pct)?;
        }
        Ok(())
    }

    /// Diagnostics only: every filed note's last executed score and factors, and the filed fixed scores.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn filed_scores(&self) -> (Vec<FiledNote>, Vec<(i32, i32)>) {
        let notes = self.notes.iter().flatten().map(|n| (n.time_ms, n.note_id, n.added, n.factors)).collect();
        (notes, self.fixed.clone())
    }

    /// All original factor filings, including zero commands and both halves of replacements.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn filed_factor_commands(&self) -> impl Iterator<Item = &FactorCommand> {
        self.factors.iter().flatten()
    }

    /// Tests only: the number of score frames, and every note of the executed frames with its last execution's
    /// factor state.
    #[cfg(test)]
    pub(crate) fn executed_states(&self) -> (usize, Vec<(i32, [f32; 6])>) {
        let executed = (self.prev + 1).max(0) as usize;
        let notes = self.notes.iter().take(executed).flatten().map(|n| (n.note_id, n.state)).collect();
        (self.notes.len(), notes)
    }

    /// Tests only: each addressable note's stored integer score from its last actual execution.
    #[cfg(test)]
    pub(crate) fn executed_note_scores(&self) -> Vec<(i32, i32)> {
        let executed = (self.prev + 1).max(0) as usize;
        self.notes.iter().take(executed).flatten().map(|note| (note.note_id, note.added)).collect()
    }

    fn fixed_at(&self, f: i32) -> Option<i32> {
        self.fixed.iter().find(|x| x.0 == f).map(|x| x.1)
    }

    /// Brings the score to the frame of `t`, undoing first down to the earliest frame that received a command.
    pub(crate) fn calculate(
        &mut self,
        t: i32,
        combo: &ComboCounter,
        gekisou: Option<&dyn GekisouComboInfo>,
    ) -> Result<i32, Error> {
        self.calculate_with_inputs(t, NoteInputs::Native { combo, gekisou })
    }

    /// Execute an admitted exact command tape using this query's recorded combo inputs. The caller must
    /// preserve every native filing and query, and supply all changed combo observations before the query.
    /// This is only a numeric execution primitive: arbitrary recorded inputs carry no admission certificate.
    /// Program/trace/record-only observers cannot consume this path because they require native combo state.
    pub(super) fn calculate_recorded(&mut self, t: i32, combos: &[Vec<Option<(f32, f32)>>]) -> Result<i32, Error> {
        if self.bounds_trace.is_some()
            || self.bounds_record_only.is_some()
            || self.program.is_some()
            || self.minimum_score_up.is_some()
            || self.settled_frame != 0
            || self.settled_total != 0
            || self.settled_fixed != 0
            || self.calc.luck_weight.is_some()
        {
            return Err(Error::Unsupported("recorded score query requires an unobserved unweighted calculator".into()));
        }
        self.calculate_with_inputs(t, NoteInputs::Recorded(combos))
    }

    /// Retained storage for a private, unobserved recorded replay. Vector capacities include the complete
    /// native frame histories. The immutable integer-map estimate includes bucket/control-table allowance;
    /// this is a scratch-budget estimate, not a process RSS measurement.
    pub(super) fn recorded_storage_bytes(&self) -> Option<usize> {
        fn vector<T>(values: &Vec<T>) -> usize {
            values.capacity().saturating_mul(std::mem::size_of::<T>())
        }
        fn nested<T>(values: &Option<Vec<Option<Vec<T>>>>) -> usize {
            values.as_ref().map_or(0, |rows| {
                rows.iter().flatten().fold(vector(rows), |bytes, row| bytes.saturating_add(vector(row)))
            })
        }
        if self.bounds_trace.is_some()
            || self.bounds_record_only.is_some()
            || self.program.is_some()
            || self.minimum_score_up.is_some()
            || self.calc.luck_weight.is_some()
        {
            return None;
        }
        let mut bytes = std::mem::size_of::<Self>()
            .saturating_add(vector(&self.notes))
            .saturating_add(vector(&self.factors))
            .saturating_add(vector(&self.diffs))
            .saturating_add(vector(&self.fixed))
            .saturating_add(vector(&self.order_f))
            .saturating_add(vector(&self.order_n));
        for row in &self.notes {
            bytes = bytes.saturating_add(vector(row));
        }
        for row in &self.factors {
            bytes = bytes.saturating_add(vector(row));
        }
        bytes = bytes.saturating_add(
            self.calc
                .note_factor_percent
                .capacity()
                .saturating_add(self.calc.judgement_score_factor_percent.capacity())
                .saturating_mul(64)
                .saturating_add(1024),
        );
        if let Some(table) = &self.calc.combo_table {
            bytes = bytes.saturating_add(nested(&table.thresholds)).saturating_add(nested(&table.cumulatives));
        }
        Some(bytes)
    }

    fn calculate_with_inputs(&mut self, t: i32, inputs: NoteInputs<'_>) -> Result<i32, Error> {
        let g = get_frame(t);
        let mut to = if g < 0 { 0 } else { g };
        if self.max_frame <= g {
            to = self.max_frame - 1;
        }
        let u = if self.added < 0 { to } else { to.min(self.added - 1) };
        let start = if u < self.prev {
            if self.bounds_record_only.is_none() {
                for f in (u + 1..=self.prev).rev() {
                    self.undo(f as usize);
                }
            }
            u + 1
        } else {
            self.prev + 1
        };
        if let Some(_recording) = &mut self.bounds_record_only {
            #[cfg(feature = "search-diagnostics")]
            {
                let first = !std::mem::replace(&mut _recording.diagnostic_started, true);
                super::luck_score_bounds::record_trace_only_query(first, u < self.prev || start <= to);
            }
            self.validate_record_only_notes(start, to)?;
        } else {
            for f in start..=to {
                self.execute(f as usize, &inputs)?;
            }
        }
        if let Some((ft, fs)) = self.pending_fixed.take() {
            let ff = get_frame(ft);
            if self.fixed_at(ff).is_some() {
                return Err(Error::Game("two fixed scores in one frame".into()));
            }
            self.fixed.push((ff, fs));
            self.score = self.score.wrapping_add(fs);
            if let Some(program) = &mut self.program {
                program.file_fixed(ff)?;
            }
        }
        self.prev = to;
        self.added = -1;
        if let Some(trace) = &mut self.bounds_trace {
            let NoteInputs::Native { combo, gekisou } = inputs else {
                unreachable!("recorded queries reject native trace observers before execution")
            };
            // Observe every currently addressable note, including notes this recorder did not reexecute.
            // The support evaluator uses these values on every possible native rewind path. Notes outside the
            // stale frames have unchanged combo inputs, so observing them again would emit nothing.
            let frames = ((to + 1).max(0) as usize).min(self.notes.len());
            let last = self.max_frame - 1;
            let stale = trace.combo.begin(frames, combo, gekisou, |t| get_frame(t).min(last).max(0) as usize);
            for frame in stale.iter().flat_map(|&(from, until)| from..until) {
                for (index, note) in self.notes[frame].iter().enumerate() {
                    let (ordinary, gk) = combo_inputs(&self.calc, combo, gekisou, note.time_ms)?;
                    if trace.combo.changed(frame, index, (ordinary.to_bits(), gk.to_bits())) {
                        trace.events.push(BoundsEvent::Combo { frame, index, ordinary, gekisou: gk });
                    }
                }
            }
            trace.combo.end(stale);
            #[cfg(test)]
            for (frame, notes) in self.notes.iter().enumerate().take(frames) {
                for (index, note) in notes.iter().enumerate() {
                    let (ordinary, gk) = combo_inputs(&self.calc, combo, gekisou, note.time_ms)?;
                    assert_eq!(trace.combo.seen(frame, index), Some((ordinary.to_bits(), gk.to_bits())));
                }
            }
            trace.events.push(BoundsEvent::Query { time_ms: t, to });
            trace.queries += 1;
        }
        Ok(self.score)
    }

    fn undo(&mut self, f: usize) {
        #[cfg(feature = "search-diagnostics")]
        if f < self.settled_frame {
            self.settled_violations += 1;
        }
        if let Some(program) = &mut self.program {
            program.undo(f);
        }
        for n in &self.notes[f] {
            self.score = self.score.wrapping_sub(n.added);
        }
        if let Some(v) = self.fixed_at(f as i32) {
            self.score = self.score.wrapping_sub(v);
        }
        self.diffs[f].undo(&mut self.calc.state);
    }

    fn execute(&mut self, f: usize, inputs: &NoteInputs<'_>) -> Result<(), Error> {
        let (fl, nl) = (&self.factors[f], &mut self.notes[f]);
        self.order_f.clear();
        self.order_f.extend(0..fl.len());
        self.order_f.sort_by_key(|&i| (fl[i].time_ms, fl[i].owner_id));
        self.order_n.clear();
        self.order_n.extend(0..nl.len());
        self.order_n.sort_by_key(|&i| (nl[i].time_ms, nl[i].note_id));
        let (mut a, mut b) = (0usize, 0usize);
        while a < self.order_f.len() || b < self.order_n.len() {
            if a < self.order_f.len()
                && (b >= self.order_n.len() || nl[self.order_n[b]].time_ms >= fl[self.order_f[a]].time_ms)
            {
                let c = &fl[self.order_f[a]];
                apply_factor(&mut self.calc.state, c);
                self.diffs[f].add(c);
                a += 1;
            } else {
                let n = &mut nl[self.order_n[b]];
                let s = match inputs {
                    NoteInputs::Native { combo, gekisou } => {
                        let c = combo.timing_combo(n.time_ms)?;
                        let (s, score_up) = self.calc.note_score_and_score_up(
                            c,
                            n.life,
                            n.time_ms,
                            n.note_type,
                            n.score_type,
                            *gekisou,
                        )?;
                        observe_score_up(&mut self.minimum_score_up, score_up);
                        if let Some(program) = &mut self.program {
                            let kernel = Kernel::capture(
                                &self.calc,
                                c,
                                n.life,
                                n.time_ms,
                                n.note_type,
                                n.score_type,
                                *gekisou,
                                program.origin_power(),
                            )?;
                            program.execute_note(f, self.order_n[b], kernel);
                        }
                        #[cfg(feature = "search-diagnostics")]
                        {
                            let cum = match &self.calc.combo_table {
                                Some(table) => table.get_cumulative_factor(crate::live::score::COMBO, c)?,
                                None => 0.0,
                            };
                            n.factors = [
                                self.calc.gekisou_combo_bonus_factor(*gekisou, n.time_ms)?,
                                self.calc.state.combo_score_up + (crate::num::min_ignoring_nan(cum, 1f32) + 1f32),
                                self.calc.state.note_score_up + self.calc.state.judgement_factor(n.score_type),
                            ];
                        }
                        s
                    }
                    NoteInputs::Recorded(combos) => {
                        let (ordinary, gekisou) =
                            combos.get(f).and_then(|row| row.get(self.order_n[b])).copied().flatten().ok_or_else(
                                || Error::Unsupported("recorded score query is missing a note's combo inputs".into()),
                            )?;
                        let combo = gekisou * (self.calc.state.combo_score_up + ordinary);
                        let (score_up, luck) = self.calc.score_up_and_luck(n.score_type, n.time_ms);
                        let s = self.calc.note_score_core(n.life, n.note_type, n.score_type, combo, score_up, luck)?;
                        observe_score_up(&mut self.minimum_score_up, score_up);
                        #[cfg(feature = "search-diagnostics")]
                        {
                            n.factors = [gekisou, self.calc.state.combo_score_up + ordinary, score_up];
                        }
                        s
                    }
                };
                #[cfg(test)]
                {
                    let st = &self.calc.state;
                    n.state = [st.combo_score_up, st.note_score_up, st.just, st.perfect, st.great, st.good];
                }
                n.added = s;
                self.score = self.score.wrapping_add(s);
                b += 1;
            }
        }
        if let Some(v) = self.fixed_at(f as i32) {
            if let Some(program) = &mut self.program {
                program.execute_fixed(f as i32);
            }
            self.score = self.score.wrapping_add(v);
            self.rank_bonus = self.rank_bonus.wrapping_add(v);
        }
        Ok(())
    }
}

/// Fresh master-built tables have matching arrays. Preserve the ordinary numeric path for malformed custom
/// tables; otherwise every immutable COMBO/GEKISOU_COMBO lookup is total for every integer count.
fn bounds_combo_table_is_valid(table: Option<&crate::live::score::ComboTable>) -> bool {
    let Some(table) = table else { return true };
    let (Some(thresholds), Some(cumulatives)) = (&table.thresholds, &table.cumulatives) else {
        return false;
    };
    thresholds.len() == cumulatives.len()
        && thresholds.iter().zip(cumulatives).all(|(thresholds, cumulatives)| {
            thresholds.as_ref().is_none_or(|thresholds| {
                thresholds.is_empty()
                    || cumulatives.as_ref().is_some_and(|cumulatives| cumulatives.len() == thresholds.len())
            })
        })
}

fn observe_score_up(minimum: &mut Option<f32>, score_up: f32) {
    if let Some(value) = minimum
        && !value.is_nan()
        && score_up.partial_cmp(value) != Some(std::cmp::Ordering::Greater)
    {
        *value = score_up;
    }
}

/// The (ordinary, Gekisou) combo factors the score reads for a note at `time_ms`.
fn combo_inputs(
    calc: &LiveScoreCalculator,
    combo: &ComboCounter,
    gekisou: Option<&dyn GekisouComboInfo>,
    time_ms: i32,
) -> Result<(f32, f32), Error> {
    let count = combo.timing_combo(time_ms)?;
    let ordinary = calc
        .combo_table
        .as_ref()
        .map_or(Ok(0.0), |table| table.get_cumulative_factor(crate::live::score::COMBO, count))?
        .min(1.0)
        + 1.0;
    let gk = calc.gekisou_combo_bonus_factor(gekisou, time_ms)?;
    if !ordinary.is_finite() || !gk.is_finite() {
        return Err(Error::Unsupported("score bounds encountered nonfinite combo inputs".into()));
    }
    Ok((ordinary, gk))
}

#[cfg(test)]
mod score_up_observation_tests {
    use super::{FrameDiff, FrameDiffs, FrameLists, observe_score_up};

    #[test]
    fn compact_frame_storage_identity_retains_lengths_and_signed_zero() {
        let empty: Vec<Vec<i32>> = vec![vec![]; 4000];
        assert_eq!(format!("{:?}", FrameLists(&empty)), "EmptyFrameLists(4000)");
        assert_ne!(format!("{:?}", FrameLists(&empty)), format!("{:?}", FrameLists(&empty[..3999])));
        assert_ne!(format!("{:?}", FrameLists(&[Vec::<i32>::new()])), format!("{:?}", FrameLists(&[vec![0]])));
        let mut diffs = vec![FrameDiff::default(); 4000];
        assert_eq!(format!("{:?}", FrameDiffs(&diffs)), "ZeroFrameDiffs(4000)");
        diffs[3999].note = -0.0;
        assert!(!diffs[3999].is_bitwise_zero());
        assert!(!format!("{:?}", FrameDiffs(&diffs)).starts_with("ZeroFrameDiffs"));
        diffs[3999] = FrameDiff { band_total_power: 1, ..Default::default() };
        assert!(!diffs[3999].is_bitwise_zero());
    }

    #[test]
    fn observation_covers_reexecutions_and_preserves_nan() {
        let mut minimum = None;
        observe_score_up(&mut minimum, 0.25);
        assert!(minimum.is_none());
        minimum = Some(f32::INFINITY);
        for score_up in [1.5, 0.75, 2.0, 0.5, 1.0] {
            observe_score_up(&mut minimum, score_up);
        }
        assert_eq!(minimum, Some(0.5));
        observe_score_up(&mut minimum, f32::NAN);
        observe_score_up(&mut minimum, 0.1);
        assert!(minimum.unwrap().is_nan());
    }
}

#[cfg(test)]
mod recorded_query_tests {
    use super::*;
    use crate::live::score::{COMBO, ComboTable, GEKISOU_COMBO};

    struct Gk(i32);
    impl GekisouComboInfo for Gk {
        fn gekisou_combo(&self, time_ms: i32) -> Option<i32> {
            (40..=220).contains(&time_ms).then_some(self.0)
        }
        fn combo_windows(&self, out: &mut Vec<(i32, i32, u64)>) {
            out.push((40, 220, self.0 as u64));
        }
    }

    fn calculator() -> IncrementalCalculator {
        let table = ComboTable::build([
            (i64::from(COMBO), 1, 0.037),
            (i64::from(COMBO), 3, 0.081),
            (i64::from(GEKISOU_COMBO), 1, 0.073),
            (i64::from(GEKISOU_COMBO), 3, 0.097),
        ])
        .unwrap();
        IncrementalCalculator::new(
            LiveScoreCalculator {
                score_adjustment_factor: 0.731,
                music_difficulty_factor: 1.035,
                converted_note_count: 7,
                life_onus_factor: 0.5,
                event_bonus_factor: 1.1,
                assist_factor: 0.93,
                note_factor_percent: [(100, 100)].into_iter().collect(),
                judgement_score_factor_percent: [(1, 103), (2, 100)].into_iter().collect(),
                state: ScoreFactorState::new(123457),
                combo_table: Some(table),
                luck_weight: None,
            },
            400,
        )
    }

    fn fields(state: &ScoreFactorState) -> [u32; 9] {
        [
            state.band_total_power as u32,
            state.combo_score_up.to_bits(),
            state.note_score_up.to_bits(),
            state.just.to_bits(),
            state.perfect.to_bits(),
            state.great.to_bits(),
            state.good.to_bits(),
            state.added_luck_bonus as u32,
            state.gekisou_rank_bonus_score as u32,
        ]
    }

    fn check_query(
        native: &mut IncrementalCalculator,
        recorded: &mut IncrementalCalculator,
        time: i32,
        combo: &ComboCounter,
        gk: &Gk,
    ) -> i32 {
        let inputs: Vec<_> = native
            .notes
            .iter()
            .map(|notes| {
                notes
                    .iter()
                    .map(|note| Some(combo_inputs(&native.calc, combo, Some(gk), note.time_ms).unwrap()))
                    .collect()
            })
            .collect();
        let expected = native.calculate(time, combo, Some(gk)).unwrap();
        assert_eq!(recorded.calculate_recorded(time, &inputs).unwrap(), expected);
        assert_eq!(fields(&native.calc.state), fields(&recorded.calc.state));
        assert_eq!(native.executed_note_scores(), recorded.executed_note_scores());
        assert_eq!(native.rank_bonus, recorded.rank_bonus);
        assert_eq!(native.fixed, recorded.fixed);
        assert_eq!(native.prev, recorded.prev);
        // Includes every native FrameDiff f32 field, even a drift that has not yet reached an integer floor.
        for (a, b) in native.diffs.iter().zip(&recorded.diffs) {
            assert_eq!((a.band_total_power, a.luck), (b.band_total_power, b.luck));
            assert_eq!(
                [a.combo, a.note, a.just, a.perfect, a.great, a.good].map(f32::to_bits),
                [b.combo, b.note, b.just, b.perfect, b.great, b.good].map(f32::to_bits)
            );
        }
        expected
    }

    #[test]
    fn recorded_queries_keep_signed_owner_filings_rewinds_changed_combo_and_rank_snapshots() {
        let mut native = calculator();
        let mut combo = ComboCounter::new(8);
        for time in [40, 80, 120] {
            combo.add_judgement(time, 5).unwrap();
        }
        // Deliberately file owner order backwards; equal owner/time commands retain their filing order.
        for (owner, note_mill) in [(402, -4500), (301, 11000), (102, 19000), (102, -1234)] {
            native.add_factor(FactorCommand { time_ms: 80, owner_id: owner, note_mill, ..Default::default() });
        }
        native.add_factor(FactorCommand { time_ms: 40, owner_id: -1, luck: 10, ..Default::default() });
        native.add_factor(FactorCommand {
            time_ms: 100,
            owner_id: 201,
            combo_mill: -7301,
            judgement: 6,
            judge_mill: 4317,
            ..Default::default()
        });
        for (id, time, life) in [(0, 80, 1000), (1, 120, 0), (2, 200, 700)] {
            native.add_note(NoteCommand::new(time, life, id, 100, 1));
        }
        let mut recorded = native.clone();
        let mut gk = Gk(1);
        check_query(&mut native, &mut recorded, 240, &combo, &gk);
        let before = combo_inputs(&native.calc, &combo, Some(&gk), 120).unwrap();
        combo.add_judgement(100, 1).unwrap();
        gk.0 = 4;
        let after = combo_inputs(&native.calc, &combo, Some(&gk), 120).unwrap();
        assert_ne!(before.0.to_bits(), after.0.to_bits());
        assert_ne!(before.1.to_bits(), after.1.to_bits());
        for calc in [&mut native, &mut recorded] {
            calc.add_factor(FactorCommand { time_ms: 40, owner_id: 102, note_mill: -11234, ..Default::default() });
            calc.add_note(NoteCommand::new(60, 300, 3, 100, 2));
        }
        check_query(&mut native, &mut recorded, 160, &combo, &gk);
        let start = check_query(&mut native, &mut recorded, 60, &combo, &gk);
        let end = check_query(&mut native, &mut recorded, 200, &combo, &gk);
        assert_ne!(start, end);
        let rank = (i128::from(end.wrapping_sub(start)) * -17 / 100) as i32;
        assert_ne!(rank, 0);
        for calc in [&mut native, &mut recorded] {
            calc.add_fixed(220, rank);
        }
        check_query(&mut native, &mut recorded, 240, &combo, &gk);
        assert_eq!(native.rank_bonus, 0, "pending rank is first added outside frame execution");
        for calc in [&mut native, &mut recorded] {
            calc.add_factor(FactorCommand { time_ms: 80, owner_id: 402, note_mill: 4500, ..Default::default() });
            calc.add_factor(FactorCommand { time_ms: 220, owner_id: -1, luck: -10, ..Default::default() });
        }
        check_query(&mut native, &mut recorded, 240, &combo, &gk);
        assert_eq!(native.rank_bonus, rank, "rewind now executes the filed rank's original frame");
        check_query(&mut native, &mut recorded, 80, &combo, &gk);
        check_query(&mut native, &mut recorded, 4000, &combo, &gk);
    }

    #[test]
    fn recorded_queries_refuse_missing_inputs_and_native_observers() {
        let mut missing = calculator();
        missing.add_note(NoteCommand::new(40, 1000, 0, 100, 1));
        assert!(matches!(missing.calculate_recorded(40, &[]), Err(Error::Unsupported(_))));
        let mut traced = calculator();
        traced.begin_bounds(Vec::new(), false);
        assert!(matches!(traced.calculate_recorded(0, &[]), Err(Error::Unsupported(_))));
        assert_eq!((traced.prev, traced.score), (-1, 0));
        let mut program = calculator();
        program.begin_program().unwrap();
        assert!(matches!(program.calculate_recorded(0, &[]), Err(Error::Unsupported(_))));
        assert_eq!((program.prev, program.score), (-1, 0));
        let mut minimum = calculator();
        minimum.minimum_score_up = Some(f32::INFINITY);
        assert!(matches!(minimum.calculate_recorded(0, &[]), Err(Error::Unsupported(_))));
        assert_eq!((minimum.prev, minimum.score), (-1, 0));
    }
}
