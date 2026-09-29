//! Gekisou windows: the frames of a play where Gekisou skills act, the simulation driven over those frames only, and
//! the note scores of a window recomputed from the factor commands and the Gekisou combo.
//!
//! A Gekisou (support) skill triggers only while a range of its mission is concerned (a state change of such a range
//! in the frame, or the range playing), and everything it changes ends by the range's Finish: its combo and Just
//! bonuses, cumulative rules and luck factors act on the range's own counts; its score factors end at the latest when
//! the range completes and the luck rush it raises ends at the Finish (`GekisouController.BeforeUpdate`). So a live
//! with Gekisou skills differs from the live without them only in the notes judged from a range's Start frame to its
//! Finish frame (the range's window), and the simulation can play just those frames: every random draw of the luck
//! lottery and of the probability condition 4011 happens inside them, in the same order.
//!
//! The window's note scores follow from the commands filed during the window: every frame is executed for the last
//! time after every command filed in it or earlier (a command filed in an executed frame undoes and executes again
//! from that frame, `IncrementalCalculator`), so a note's final score is the one of the time-ordered command list
//! (frames in order; in a frame factor commands by time and owner, a note after the commands of its time) with the
//! final Gekisou combo of its range (the combo is frozen once the range is past its end delay, and the range's notes
//! are scored again when it completes). The undo of a frame subtracts the float sum of the changes it applied, so the
//! game's float state can differ from the ordered sum in the last bits (see [`super::scorecalc`]); the recomputed
//! scores ignore that.

use serde::Serialize;

use super::gekisou::{S_COMPLETE, S_DELAY, S_END, S_FINISH, S_STANDBY, S_START};
use super::{LiveModel, LiveNote, LivePlay};
use crate::error::Error;
use crate::live::score::{GekisouComboInfo, LiveScoreCalculator, ScoreFactorState, get_frame, get_luck_factor_percent};
use crate::live::skill::{FactorCommand, apply_factor};
use crate::num::min_ignoring_nan;

/// Frame indices (into the play's frames) where a Gekisou range changed its state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeFrames {
    /// Wait -> Standby (`None` when the range never waited: it started in the first frame).
    pub standby: Option<usize>,
    /// The fever's start: Start (the next frame is Playing).
    pub start: usize,
    /// The fever's end: End.
    pub end: usize,
    /// The end delay (the longest judgement window after the end, counted with the frame delta times): Delay.
    pub delay: usize,
    /// 500 ms later: Complete (the range's scores are taken and its rank bonus added).
    pub complete: usize,
    /// The next frame: Finish (the luck rush ends).
    pub finish: usize,
}

/// A note of a window: the chart note, the combo the score reads for it (the live's combo at its time) and its score
/// type after the judgement conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowNote {
    pub note: LiveNote,
    pub combo: i32,
    pub score_type: i32,
}

/// A window note's score and its derivatives by the factors a live skill adds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEval {
    pub score: i32,
    /// The score per unit of note score factor (effect types 2000 / 2005, and 2004 when the score type is a target),
    /// before the floors: `base * judgement% * combo factor * luck% / notes`.
    pub unit: f64,
    /// The score per unit of combo score factor (2002): `base * judgement% * gekisou combo factor * score factor *
    /// luck% / notes`.
    pub unit_combo: f64,
}

impl LiveModel {
    /// Plays every frame of `play` (delta times `dt`) and records the frames where each Gekisou range changed state.
    pub fn record_range_frames(&mut self, play: &LivePlay, dt: &[f32]) -> Result<Vec<RangeFrames>, Error> {
        if dt.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        self.random.set_seed(play.base_seed);
        let n = self.gk.as_ref().map_or(0, |g| g.ctrl.ranges.len());
        let mut seen = vec![[None::<usize>; 9]; n];
        for (i, (f, &d)) in play.frames.iter().zip(dt).enumerate() {
            self.frame_timed(f.time_ms, &f.judged, d)?;
            let g = self.gk.as_ref().ok_or_else(|| Error::Input("range frames without Gekisou".into()))?;
            for &idx in &g.ctrl.state_updates {
                let s = g.ctrl.states[idx].state as usize;
                if let Some(slot) = seen[idx].get_mut(s) {
                    slot.get_or_insert(i);
                }
            }
        }
        seen.iter()
            .enumerate()
            .map(|(i, s)| {
                let need = |state: u8| {
                    s[state as usize]
                        .ok_or_else(|| Error::Game(format!("Gekisou range {i} never reached state {state}")))
                };
                Ok(RangeFrames {
                    standby: s[S_STANDBY as usize],
                    start: need(S_START)?,
                    end: need(S_END)?,
                    delay: need(S_DELAY)?,
                    complete: need(S_COMPLETE)?,
                    finish: need(S_FINISH)?,
                })
            })
            .collect()
    }

    /// Plays only the frames `frames` of `play` (indices, ascending) with their delta times and the random seed `seed` (the
    /// play's own seed is not read); returns the judged notes
    /// of those frames in order, `(note id, judgement after the conversion)`.
    pub fn run_frames(
        &mut self,
        play: &LivePlay,
        seed: i32,
        dt: &[f32],
        frames: &[usize],
    ) -> Result<Vec<(i32, i32)>, Error> {
        if dt.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        if frames.windows(2).any(|w| w[1] <= w[0]) {
            return Err(Error::Input("window frames must ascend".into()));
        }
        self.random.set_seed(seed);
        let mut judged = Vec::new();
        for &i in frames {
            let f = play.frames.get(i).ok_or_else(|| Error::Input(format!("window frame {i} out of range")))?;
            self.frame_timed(f.time_ms, &f.judged, dt[i])?;
            judged.extend(self.judged.iter().map(|&(id, j, _)| (id, j)));
        }
        Ok(judged)
    }

    /// The Gekisou combo the score reads at a chart time (`None` outside a combo range or without Gekisou).
    pub fn gekisou_combo_at(&self, time_ms: i32) -> Option<i32> {
        self.gk.as_ref().and_then(|g| g.ctrl.gekisou_combo(time_ms))
    }

    /// Every factor command filed so far, frame by frame, in filing order within a frame.
    pub fn factor_commands(&self) -> Vec<FactorCommand> {
        self.score.factor_commands().copied().collect()
    }

    /// The score calculator with the live's initial factor state (only the band total power).
    pub fn initial_calculator(&self, total_power: i32) -> LiveScoreCalculator {
        let mut c = self.score.calc.clone();
        c.state = ScoreFactorState::new(total_power);
        c
    }

    /// The last frame (exclusive) of the score's frame table.
    pub fn score_max_frame(&self) -> i32 {
        self.score.max_frame()
    }
}

/// The scores of a window's notes (in their order) from the factor commands filed in the window (in filing order):
/// frames in order; in a frame the commands by (time, owner), stably, each before the notes of its time and later,
/// the notes by (time, note id). `gekisou` gives the Gekisou combo; the life is full.
pub fn window_note_scores(
    calc: &LiveScoreCalculator,
    max_frame: i32,
    notes: &[WindowNote],
    commands: &[FactorCommand],
    gekisou: &dyn GekisouComboInfo,
) -> Result<Vec<NoteEval>, Error> {
    let frame = |t: i32| get_frame(t).min(max_frame.wrapping_sub(1));
    // (frame, time, 0 command / 1 note, owner or note id, index)
    let mut items: Vec<(i32, i32, u8, i32, usize)> = Vec::with_capacity(notes.len() + commands.len());
    for (i, c) in commands.iter().enumerate() {
        items.push((frame(c.time_ms), c.time_ms, 0, c.owner_id, i));
    }
    for (i, n) in notes.iter().enumerate() {
        items.push((frame(n.note.time_ms), n.note.time_ms, 1, n.note.note_id, i));
    }
    items.sort_by_key(|x| (x.0, x.1, x.2, x.3));
    let mut calc = calc.clone();
    let mut out = vec![NoteEval { score: 0, unit: 0.0, unit_combo: 0.0 }; notes.len()];
    for (_, _, kind, _, i) in items {
        if kind == 0 {
            apply_factor(&mut calc.state, &commands[i]);
            continue;
        }
        let n = &notes[i];
        let score =
            calc.note_score(n.combo, 1000, n.note.time_ms, n.note.note_operate_type, n.score_type, Some(gekisou))?;
        let (unit, unit_combo) = note_units(&calc, n, gekisou)?;
        out[i] = NoteEval { score, unit, unit_combo };
    }
    Ok(out)
}

/// The score of a note per unit of note score factor and of combo score factor, before the floors.
fn note_units(calc: &LiveScoreCalculator, n: &WindowNote, gekisou: &dyn GekisouComboInfo) -> Result<(f64, f64), Error> {
    let np = *calc
        .note_factor_percent
        .get(&n.note.note_operate_type)
        .ok_or_else(|| Error::Game(format!("note type {} has no score percent", n.note.note_operate_type)))?;
    let jp = *calc
        .judgement_score_factor_percent
        .get(&n.score_type)
        .ok_or_else(|| Error::Game(format!("score type {} has no score percent", n.score_type)))?;
    let t = (calc.score_adjustment_factor * calc.state.band_total_power as f32) * calc.music_difficulty_factor;
    let a = (np as f32 / 100f32) * t;
    let b = (jp as f32 / 100f32) * a;
    let gk = calc.gekisou_combo_bonus_factor(Some(gekisou), n.note.time_ms)?;
    let cum = match &calc.combo_table {
        None => 0f32,
        Some(tb) => tb.get_cumulative_factor(crate::live::score::COMBO, n.combo)?,
    };
    let combo_factor = gk * (calc.state.combo_score_up + (min_ignoring_nan(cum, 1f32) + 1f32));
    let score_up = calc.state.note_score_up + calc.state.judgement_factor(n.score_type);
    let luck = get_luck_factor_percent(calc.state.added_luck_bonus) as f64 / 100.0;
    let per = f64::from(b) * luck / f64::from(calc.converted_note_count)
        * f64::from(calc.event_bonus_factor)
        * f64::from(calc.assist_factor);
    Ok((per * f64::from(combo_factor), per * f64::from(gk) * f64::from(score_up)))
}

impl LiveModel {
    /// `(note id, chart time, last score)` of every note scored so far.
    pub fn note_results(&self) -> Vec<(i32, i32, i32)> {
        self.score.note_results()
    }
}
