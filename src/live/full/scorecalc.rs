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

/// A note score command: chart time, the life frozen at the judgement, note id, note type and score type.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NoteCommand {
    pub time_ms: i32,
    pub life: i32,
    pub note_id: i32,
    pub note_type: i32,
    pub score_type: i32,
    added: i32,
}

impl NoteCommand {
    pub(crate) fn new(time_ms: i32, life: i32, note_id: i32, note_type: i32, score_type: i32) -> NoteCommand {
        NoteCommand { time_ms, life, note_id, note_type, score_type, added: 0 }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IncrementalCalculator {
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
}

impl IncrementalCalculator {
    pub(crate) fn new(calc: LiveScoreCalculator, music_length_ms: i32) -> IncrementalCalculator {
        let max_frame = get_frame(music_length_ms) + EXTRA_FRAMES;
        let n = max_frame as usize;
        IncrementalCalculator {
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
        }
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
        self.notes[f].push(cmd);
    }

    pub(crate) fn add_factor(&mut self, cmd: FactorCommand) {
        let f = self.file(cmd.time_ms);
        self.factors[f].push(cmd);
    }

    /// Sets the fixed score filed by the next calculation (only the last one set counts).
    pub(crate) fn add_fixed(&mut self, time_ms: i32, score: i32) {
        self.pending_fixed = Some((time_ms, score));
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
        }
        self.prev = to;
        self.added = -1;
        Ok(self.score)
    }

    fn undo(&mut self, f: usize) {
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
                n.added = s;
                self.score = self.score.wrapping_add(s);
                b += 1;
            }
        }
        if let Some(v) = self.fixed_at(f as i32) {
            self.score = self.score.wrapping_add(v);
            self.rank_bonus = self.rank_bonus.wrapping_add(v);
        }
        Ok(())
    }
}
