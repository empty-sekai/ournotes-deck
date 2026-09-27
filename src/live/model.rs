//! Plays of a live: the per-note play of the per-order model, and the judgement stream of the whole-live simulation.
//!
//! [`LiveModel`] scores a stated play with live skills only. Model: Gekisou off and snap (support) skills excluded;
//! the play gives every judged note (judgement, life at the judgement, the combo the score reads) and the life in the
//! frame where each skill event fires. The result is the score with every factor command applied once in frame
//! order. The game recalculates frames when a command lands in a frame it already scored; when such a frame already
//! held factor commands, its float state can differ from this result in the last bit.
//!
//! [`JudgementStream`] is the input of the whole-live simulation ([`crate::live::full`]): the frames of a play and
//! the notes judged in each; life, combo and skills then follow from the simulation.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::live::full::{JudgedNote, LivePlay, PlayFrame};
use crate::live::score::{
    COMBO, ComboTable, LiveScoreCalculator, LiveScoreSettings, PERFECT, get_frame, get_luck_factor_percent,
};
use crate::live::skill::{FactorCommand, NotePlay, apply_factor, live_skill_commands};
use crate::live::skip::{Chart, is_judgement_note};
use crate::master::Master;
use crate::num::{floor_to_i32, min_ignoring_nan, trunc_to_i32};

/// A stated play.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Play {
    /// Every judged note.
    pub notes: Vec<NotePlay>,
    /// Life in the frame where skill event `index` fires.
    pub life_at_event: Vec<i32>,
    /// Assist mode (scales every note by the assist percentage).
    #[serde(default)]
    pub assist: bool,
}

/// `NoteSimulateJudgement` of a Perfect.
const SIMULATE_PERFECT: i64 = 5;

impl Play {
    /// The default play ("theoretical best") of a live with Gekisou off: every judged note is judged Perfect (the
    /// Just judgement is only enabled inside Gekisou Just-count ranges), so the play is a full combo. The life follows
    /// the life controller from `life_base` with the Perfect damage of `MasterLiveJudgementParameter`; the combo a
    /// note's score reads is the number of judged notes at earlier chart times.
    pub fn theoretical_best(master: &Master, chart: &Chart) -> Result<Play, Error> {
        let base = master
            .live_settings
            .iter()
            .find(|r| r.key == "life_base")
            .ok_or_else(|| Error::Master("MasterLiveSettings life_base missing".into()))?;
        let base: i32 = base.value.trim().parse().map_err(|_| Error::Master("life_base is not an integer".into()))?;
        let damage = master
            .judgement_parameters
            .iter()
            .find(|r| r.note_simulate_judgement == SIMULATE_PERFECT)
            .ok_or_else(|| Error::Master("MasterLiveJudgementParameter has no Perfect row".into()))?
            .damage;
        if damage != 0 {
            // the life at a skill event would depend on the frame schedule
            return Err(Error::Unsupported(format!("a Perfect judgement costs {damage} life")));
        }
        let mut judged: Vec<_> = chart.notes.iter().filter(|n| is_judgement_note(n.note_type)).collect();
        judged.sort_by_key(|n| (n.time_ms, n.id));
        let mut notes = Vec::with_capacity(judged.len());
        let (mut before, mut i) = (0i32, 0usize);
        while i < judged.len() {
            let t = judged[i].time_ms;
            let j = i + judged[i..].iter().take_while(|n| n.time_ms == t).count();
            for n in &judged[i..j] {
                notes.push(NotePlay {
                    note_id: n.id,
                    time_ms: n.time_ms,
                    note_type: n.note_type,
                    score_type: PERFECT,
                    life: base,
                    combo: before,
                });
            }
            before = before.wrapping_add((j - i) as i32);
            i = j;
        }
        let events = chart.skill_events.iter().map(|e| e.index.max(0) as usize + 1).max().unwrap_or(0);
        Ok(Play { notes, life_at_event: vec![base; events], assist: false })
    }
}

/// Frame rate of [`JudgementStream::theoretical_best`].
pub const THEORETICAL_FPS: i64 = 60;
/// How long [`JudgementStream::theoretical_best`] keeps playing after the last judged note and the last skill event.
pub const THEORETICAL_TAIL_MS: i32 = 2000;

/// A judgement stream: the frames of a play, the notes judged in each frame, the live's random seed and the assist
/// flag. It is the play of the whole-live simulation; life, combo and skills follow from the simulation.
///
/// JSON (`camelCase`):
///
/// ```json
/// {"frames": [0, 16, 33, 50], "judged": [[2, 1, 5, 1030], [3, 2, 4, 1047]], "baseSeed": 0, "assist": false}
/// ```
///
/// `frames` holds the music time of each frame in ms, in play order (non-decreasing). Each `judged` row is
/// `[frame, noteId, judgement, judgementTimeMs]`: the note is judged in frame `frames[frame]` with the note judgement
/// before skill conversion (`NoteSimulateJudgement`: 1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just) at the given
/// judgement time; the rows of one frame are judged in the order they appear. `baseSeed` (default 0) seeds the
/// live's random streams; `assist` (default false) scales every note by the assist percentage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgementStream {
    pub frames: Vec<i32>,
    #[serde(default)]
    pub judged: Vec<[i32; 4]>,
    #[serde(default)]
    pub base_seed: i32,
    #[serde(default)]
    pub assist: bool,
}

impl JudgementStream {
    /// The default play ("theoretical best") of a live with Gekisou off, at [`THEORETICAL_FPS`]: frame `k` is at
    /// `floor(k * 1000 / 60)` ms for `k = 0, 1, ...` while that time is at most `T + 2000`, with `T` the latest time of
    /// a judged note or a skill event (0 when earlier); every judged note is judged Perfect in the first frame whose
    /// time reaches its chart time, with the chart time as its judgement time, in the order (chart time, note id)
    /// within a frame. Gekisou off never enables the Just judgement, so Perfect is the highest; the random seed is 0.
    pub fn theoretical_best(chart: &Chart) -> JudgementStream {
        let mut judged: Vec<_> = chart.notes.iter().filter(|n| is_judgement_note(n.note_type)).collect();
        judged.sort_by_key(|n| (n.time_ms, n.id));
        let last =
            judged.iter().map(|n| n.time_ms).chain(chart.skill_events.iter().map(|e| e.time_ms)).fold(0i32, i32::max);
        let end = last as i64 + THEORETICAL_TAIL_MS as i64;
        let mut frames = Vec::new();
        let mut k = 0i64;
        loop {
            let t = k * 1000 / THEORETICAL_FPS;
            if t > end {
                break;
            }
            frames.push(t as i32);
            k += 1;
        }
        let rows = judged
            .iter()
            .map(|n| {
                let f = frames.partition_point(|&t| t < n.time_ms);
                [f as i32, n.id, SIMULATE_PERFECT as i32, n.time_ms]
            })
            .collect();
        JudgementStream { frames, judged: rows, base_seed: 0, assist: false }
    }

    /// Checks the stream (frame indexes in range, non-decreasing frame times, judgements 1..=6) and builds the play
    /// the simulation reads.
    pub fn to_live_play(&self) -> Result<LivePlay, Error> {
        if self.frames.windows(2).any(|w| w[1] < w[0]) {
            return Err(Error::Input("judgement stream: frame times decrease".into()));
        }
        let mut frames: Vec<PlayFrame> =
            self.frames.iter().map(|&t| PlayFrame { time_ms: t, judged: Vec::new() }).collect();
        for r in &self.judged {
            let [f, note_id, judgement, judgement_time_ms] = *r;
            let frame = usize::try_from(f)
                .ok()
                .and_then(|i| frames.get_mut(i))
                .ok_or_else(|| Error::Input(format!("judgement stream: frame {f} out of range")))?;
            if !(1..=6).contains(&judgement) {
                return Err(Error::Input(format!("judgement stream: judgement {judgement} outside 1..=6")));
            }
            frame.judged.push(JudgedNote { note_id, judgement, judgement_time_ms });
        }
        Ok(LivePlay { frames, base_seed: self.base_seed })
    }
}

#[derive(Clone, Copy, Debug)]
struct PreNote {
    key: (i32, i32, i32),
    time_ms: i32,
    score_type: i32,
    note_pct: f32,
    judge_pct: f32,
    /// `min(table, 1) + 1` at the note's combo.
    combo_base: f32,
    life: f32,
}

/// A chart, a play and the score settings, prepared for evaluating many decks.
#[derive(Clone, Debug)]
pub struct LiveModel {
    pub music_score_level: i32,
    pub converted_note_count: i32,
    pub max_frame: i32,
    pub music_length_ms: i32,
    pub assist_factor: f32,
    pub events: Vec<(i32, i32)>,
    pub life_at_event: Vec<i32>,
    settings: LiveScoreSettings,
    combo: ComboTable,
    notes: Vec<PreNote>,
    difficulty: f32,
}

impl LiveModel {
    pub fn new(master: &Master, music_score_level: i32, chart: &Chart, play: &Play) -> Result<LiveModel, Error> {
        let settings = LiveScoreSettings::from_master(master)?;
        let combo = ComboTable::from_master(master)?;
        let assist_factor = if play.assist {
            let v = master
                .live_settings
                .iter()
                .find(|r| r.key == "assist_score_percent")
                .ok_or_else(|| Error::Master("MasterLiveSettings assist_score_percent missing".into()))?;
            let p: f32 =
                v.value.trim().parse().map_err(|_| Error::Master("assist_score_percent is not a number".into()))?;
            p / 100f32
        } else {
            1.0
        };
        // The live's music length is the last timing note + 1000 ms, as on the skip path.
        let music_length_ms = chart.last_timing_note_ms.wrapping_add(1000);
        let max_frame = get_frame(music_length_ms).wrapping_add(50);
        let frame = |t: i32| get_frame(t).min(max_frame.wrapping_sub(1));
        let mut notes = Vec::with_capacity(play.notes.len());
        for n in &play.notes {
            let note_pct = *settings
                .note_factor_percent
                .get(&n.note_type)
                .ok_or_else(|| Error::Game(format!("note type {} has no score percent", n.note_type)))?;
            let judge_pct = *settings
                .judgement_score_factor_percent
                .get(&n.score_type)
                .ok_or_else(|| Error::Game(format!("score type {} has no score percent", n.score_type)))?;
            let cum = combo.get_cumulative_factor(COMBO, n.combo)?;
            notes.push(PreNote {
                key: (frame(n.time_ms), n.time_ms, n.note_id),
                time_ms: n.time_ms,
                score_type: n.score_type,
                note_pct: note_pct as f32 / 100f32,
                judge_pct: judge_pct as f32 / 100f32,
                combo_base: min_ignoring_nan(cum, 1f32) + 1f32,
                life: if n.life > 0 { 1f32 } else { settings.life_onus_factor },
            });
        }
        notes.sort_by_key(|n| n.key);
        let events = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
        Ok(LiveModel {
            music_score_level,
            converted_note_count: chart.converted_note_count,
            max_frame,
            music_length_ms,
            assist_factor,
            events,
            life_at_event: play.life_at_event.clone(),
            difficulty: crate::live::score::get_music_score_level_factor(music_score_level),
            settings,
            combo,
            notes,
        })
    }

    /// Factor commands of the live skills for `performers[k]` = `(live skill id, level)` of performance position k.
    pub fn commands(&self, master: &Master, performers: &[(i64, i64)]) -> Result<Vec<FactorCommand>, Error> {
        live_skill_commands(master, &self.events, performers, &self.life_at_event, self.music_length_ms, None)
    }

    /// The score for a deck power and a command list.
    pub fn score(&self, total_power: i32, commands: &[FactorCommand]) -> i32 {
        let frame = |t: i32| get_frame(t).min(self.max_frame.wrapping_sub(1));
        let mut cmds: Vec<((i32, i32, i32), &FactorCommand)> =
            commands.iter().map(|c| ((frame(c.time_ms), c.time_ms, c.owner_id), c)).collect();
        cmds.sort_by_key(|x| x.0);
        let mut state = crate::live::score::ScoreFactorState::new(total_power);
        let adj = self.settings.score_adjustment_factor;
        let cnc = self.converted_note_count as f32;
        let mut ci = 0;
        let mut total = 0i32;
        for n in &self.notes {
            // a factor goes first unless the note's (frame, time) is earlier
            while ci < cmds.len() && (cmds[ci].0.0, cmds[ci].0.1) <= (n.key.0, n.key.1) {
                apply_factor(&mut state, cmds[ci].1);
                ci += 1;
            }
            let score_up = state.note_score_up + state.judgement_factor(n.score_type);
            let combo_factor = 1f32 * (state.combo_score_up + n.combo_base);
            let luck = get_luck_factor_percent(state.added_luck_bonus);
            let t = (adj * state.band_total_power as f32) * self.difficulty;
            let a = n.note_pct * t;
            let b = n.judge_pct * a;
            let c = (b * combo_factor) * score_up;
            let d = (luck as f32 / 100f32) * c;
            let x = d / cnc;
            let fl = x.floor();
            let y = if fl == f32::INFINITY { -2147483648f32 } else { trunc_to_i32(fl) as f32 };
            let z = self.assist_factor * (n.life * (y * 1f32));
            total = total.wrapping_add(floor_to_i32(z));
            let _ = n.time_ms;
        }
        total
    }

    /// The same score through the general calculator (a paired implementation for tests).
    pub fn score_reference(&self, total_power: i32, play: &Play, commands: &[FactorCommand]) -> Result<i32, Error> {
        let mut calc = LiveScoreCalculator::new(
            total_power,
            self.music_score_level,
            self.converted_note_count,
            &self.settings,
            1.0,
            self.assist_factor,
            Some(self.combo.clone()),
        );
        crate::live::skill::score_with_factors(&mut calc, &play.notes, commands, self.max_frame)
    }

    /// Per-note upper bound data: for each note (processing order) its time, judgement and the real-valued score
    /// per unit of power and per unit of score-up factor.
    pub(crate) fn note_coefficients(&self) -> Vec<(i32, i32, f64)> {
        let adj = self.settings.score_adjustment_factor as f64;
        let cnc = self.converted_note_count as f64;
        self.notes
            .iter()
            .map(|n| {
                let k = self.assist_factor as f64
                    * n.life as f64
                    * n.judge_pct as f64
                    * n.note_pct as f64
                    * adj
                    * self.difficulty as f64
                    * n.combo_base as f64
                    / cnc;
                (n.time_ms, n.score_type, k)
            })
            .collect()
    }
}
