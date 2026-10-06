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

#[derive(Clone, Debug)]
pub(crate) struct IncrementalCalculator {
    pub(super) bounds_trace: Option<BoundsTrace>,
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

impl IncrementalCalculator {
    pub(crate) fn new(calc: LiveScoreCalculator, music_length_ms: i32) -> IncrementalCalculator {
        let max_frame = get_frame(music_length_ms) + EXTRA_FRAMES;
        let n = max_frame as usize;
        IncrementalCalculator {
            bounds_trace: None,
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
        });
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

    pub(super) fn bounds_potential_skills(&mut self, time_ms: i32) {
        if self.bounds_trace.is_none() {
            return;
        }
        let frame = get_frame(time_ms).min(self.max_frame - 1) as usize;
        if let Some(trace) = &mut self.bounds_trace
            && !trace.probes.is_empty()
        {
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

    /// Tests only: the number of score frames, and every note of the executed frames with its last execution's
    /// factor state.
    #[cfg(test)]
    pub(crate) fn executed_states(&self) -> (usize, Vec<(i32, [f32; 6])>) {
        let executed = (self.prev + 1).max(0) as usize;
        let notes = self.notes.iter().take(executed).flatten().map(|n| (n.note_id, n.state)).collect();
        (self.notes.len(), notes)
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
        let g = get_frame(t);
        let mut to = if g < 0 { 0 } else { g };
        if self.max_frame <= g {
            to = self.max_frame - 1;
        }
        let u = if self.added < 0 { to } else { to.min(self.added - 1) };
        let start = if u < self.prev {
            for f in (u + 1..=self.prev).rev() {
                self.undo(f as usize);
            }
            u + 1
        } else {
            self.prev + 1
        };
        for f in start..=to {
            self.execute(f as usize, combo, gekisou)?;
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

    fn execute(&mut self, f: usize, combo: &ComboCounter, gekisou: Option<&dyn GekisouComboInfo>) -> Result<(), Error> {
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
                let c = combo.timing_combo(n.time_ms)?;
                let s = self.calc.note_score(c, n.life, n.time_ms, n.note_type, n.score_type, gekisou)?;
                if let Some(program) = &mut self.program {
                    let kernel = Kernel::capture(
                        &self.calc,
                        c,
                        n.life,
                        n.time_ms,
                        n.note_type,
                        n.score_type,
                        gekisou,
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
                        self.calc.gekisou_combo_bonus_factor(gekisou, n.time_ms)?,
                        self.calc.state.combo_score_up + (crate::num::min_ignoring_nan(cum, 1f32) + 1f32),
                        self.calc.state.note_score_up + self.calc.state.judgement_factor(n.score_type),
                    ];
                }
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
