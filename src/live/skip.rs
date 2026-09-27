//! Skip score: the score of a skipped live, a closed form of the deck power and the chart.

use crate::error::Error;
use crate::live::score::{ComboTable, GREAT, LiveScoreCalculator, LiveScoreSettings, get_frame};
use crate::num::ceil_to_i32;

/// A note of a chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartNote {
    pub id: i32,
    pub time_ms: i32,
    /// Note operate type.
    pub note_type: i32,
}

/// A skill event of a chart: event `index` fires the live skill of the member at that performance position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillEvent {
    pub index: i32,
    pub time_ms: i32,
}

/// The chart values the scores read. `notes` are in the chart's enumeration order. [`Chart::from_notes`] computes
/// the two counts the way the game does; the fields are public for callers that state them directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// Note count after the chart's conversions (the per-note divisor).
    pub converted_note_count: i32,
    /// Time of the last timing note.
    pub last_timing_note_ms: i32,
    pub notes: Vec<ChartNote>,
    pub skill_events: Vec<SkillEvent>,
}

impl Chart {
    /// A chart from its notes (in enumeration order) and skill events, with the two counts computed the way the game
    /// computes them from the notes and the note score table. A chart without notes is rejected.
    pub fn from_notes(
        notes: Vec<ChartNote>,
        skill_events: Vec<SkillEvent>,
        settings: &LiveScoreSettings,
    ) -> Result<Chart, Error> {
        let last = last_timing_note_ms(&notes).ok_or_else(|| Error::Input("chart has no notes".into()))?;
        Ok(Chart {
            converted_note_count: converted_note_count(&notes, settings),
            last_timing_note_ms: last,
            notes,
            skill_events,
        })
    }
}

/// Whether notes of this operate type are judged (and count for a full combo).
pub fn is_judgement_note(note_type: i32) -> bool {
    !matches!(note_type, 0 | 80 | 82 | 100 | 103 | 121 | 122 | 123)
}

/// The number of judged notes (the full-combo count of the chart).
pub fn judgement_note_total_count(notes: &[ChartNote]) -> i32 {
    notes.iter().filter(|n| is_judgement_note(n.note_type)).count() as i32
}

/// The per-note divisor: the ceiling of (sum of the notes' score percents) / 100, the sum over every note in 32-bit
/// arithmetic and a type without a score percent adding nothing.
pub fn converted_note_count(notes: &[ChartNote], settings: &LiveScoreSettings) -> i32 {
    let sum = notes
        .iter()
        .filter_map(|n| settings.note_factor_percent.get(&n.note_type))
        .fold(0i32, |s, &p| s.wrapping_add(p));
    ceil_to_i32(sum as f32 / 100f32)
}

/// Time of the notes at the chart's last position (`None` for a chart without notes).
pub fn last_timing_note_ms(notes: &[ChartNote]) -> Option<i32> {
    notes.iter().map(|n| n.time_ms).max()
}

/// Skip score: every note of a valid type is scored once as Great with life 100, combo 0, no skills; notes are
/// counted up to the frame of the last valid note in enumeration order.
pub fn skip_score(
    total_power: i32,
    music_score_level: i32,
    chart: &Chart,
    settings: &LiveScoreSettings,
    valid_note_types: &[i32],
    combo_table: Option<&ComboTable>,
) -> Result<i32, Error> {
    Ok(skip_sum(total_power, music_score_level, chart, settings, valid_note_types, combo_table)?.0)
}

/// The skip score's note sum without the 32-bit wrap (for range checks).
pub fn skip_score_wide(
    total_power: i32,
    music_score_level: i32,
    chart: &Chart,
    settings: &LiveScoreSettings,
    valid_note_types: &[i32],
    combo_table: Option<&ComboTable>,
) -> Result<i64, Error> {
    Ok(skip_sum(total_power, music_score_level, chart, settings, valid_note_types, combo_table)?.1)
}

fn skip_sum(
    total_power: i32,
    music_score_level: i32,
    chart: &Chart,
    settings: &LiveScoreSettings,
    valid_note_types: &[i32],
    combo_table: Option<&ComboTable>,
) -> Result<(i32, i64), Error> {
    let calc = LiveScoreCalculator::new(
        total_power,
        music_score_level,
        chart.converted_note_count,
        settings,
        1.0,
        1.0,
        combo_table.cloned(),
    );
    let max_frame = get_frame(chart.last_timing_note_ms.wrapping_add(1000)).wrapping_add(50);
    let mut cmds: Vec<(i32, &ChartNote)> = Vec::with_capacity(chart.notes.len());
    let mut t_last = 0;
    for n in &chart.notes {
        if valid_note_types.contains(&n.note_type) {
            t_last = n.time_ms;
            let mut frame = get_frame(n.time_ms);
            if frame >= max_frame {
                frame = max_frame.wrapping_sub(1);
            }
            cmds.push((frame, n));
        }
    }
    let mut to_frame = get_frame(t_last).max(0);
    if to_frame >= max_frame {
        to_frame = max_frame.wrapping_sub(1);
    }
    let mut total = 0i32;
    let mut wide = 0i64;
    for (frame, n) in cmds {
        if frame <= to_frame {
            let x = calc.note_score(0, 100, n.time_ms, n.note_type, GREAT, None)?;
            total = total.wrapping_add(x);
            wide += x as i64;
        }
    }
    Ok((total, wide))
}

/// The skip score prepared for many deck powers: the scored notes and their constants are fixed by the chart, so
/// each evaluation is the per-note float chain alone. Equal to [`skip_score`] for every power.
#[derive(Clone, Debug)]
pub struct SkipEvaluator {
    /// Per scored note: (note percent / 100, combo factor).
    notes: Vec<(f32, f32)>,
    judge: f32,
    adj: f32,
    difficulty: f32,
    cnc: f32,
}

impl SkipEvaluator {
    pub fn new(
        music_score_level: i32,
        chart: &Chart,
        settings: &LiveScoreSettings,
        valid_note_types: &[i32],
        combo_table: Option<&ComboTable>,
    ) -> Result<SkipEvaluator, Error> {
        let max_frame = get_frame(chart.last_timing_note_ms.wrapping_add(1000)).wrapping_add(50);
        let mut scored = Vec::new();
        let mut t_last = 0;
        for n in &chart.notes {
            if valid_note_types.contains(&n.note_type) {
                t_last = n.time_ms;
                let mut frame = get_frame(n.time_ms);
                if frame >= max_frame {
                    frame = max_frame.wrapping_sub(1);
                }
                scored.push((frame, n.note_type));
            }
        }
        let mut to_frame = get_frame(t_last).max(0);
        if to_frame >= max_frame {
            to_frame = max_frame.wrapping_sub(1);
        }
        let judge = *settings
            .judgement_score_factor_percent
            .get(&GREAT)
            .ok_or_else(|| Error::Game("score type Great has no score percent".into()))?;
        let cum = match combo_table {
            None => 0.0,
            Some(t) => t.get_cumulative_factor(crate::live::score::COMBO, 0)?,
        };
        let combo = 1f32 * (0f32 + (crate::num::min_ignoring_nan(cum, 1f32) + 1f32));
        let mut notes = Vec::new();
        for (frame, ty) in scored {
            if frame <= to_frame {
                let pct = *settings
                    .note_factor_percent
                    .get(&ty)
                    .ok_or_else(|| Error::Game(format!("note type {ty} has no score percent")))?;
                notes.push((pct as f32 / 100f32, combo));
            }
        }
        Ok(SkipEvaluator {
            notes,
            judge: judge as f32 / 100f32,
            adj: settings.score_adjustment_factor,
            difficulty: crate::live::score::get_music_score_level_factor(music_score_level),
            cnc: chart.converted_note_count as f32,
        })
    }

    /// Skip score and its note sum without the 32-bit wrap.
    pub fn score(&self, total_power: i32) -> (i32, i64) {
        let t = (self.adj * total_power as f32) * self.difficulty;
        let (mut total, mut wide) = (0i32, 0i64);
        for &(npf, combo) in &self.notes {
            let a = npf * t;
            let b = self.judge * a;
            let c = (b * combo) * (1f32 + 0f32);
            // luck 100 %: (100f / 100f) = 1f
            let d = 1f32 * c;
            let x = d / self.cnc;
            let fl = x.floor();
            let y = if fl == f32::INFINITY { -2147483648f32 } else { crate::num::trunc_to_i32(fl) as f32 };
            let s = crate::num::floor_to_i32(1f32 * (1f32 * (y * 1f32)));
            total = total.wrapping_add(s);
            wide += s as i64;
        }
        (total, wide)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn note(id: i32, time_ms: i32, note_type: i32) -> ChartNote {
        ChartNote { id, time_ms, note_type }
    }

    fn settings(pct: &[(i32, i32)]) -> LiveScoreSettings {
        LiveScoreSettings {
            score_adjustment_factor: 1.0,
            life_onus_factor: 0.5,
            note_factor_percent: pct.iter().copied().collect::<HashMap<_, _>>(),
            judgement_score_factor_percent: HashMap::new(),
        }
    }

    #[test]
    fn counts_follow_the_notes() {
        let s = settings(&[(1, 100), (120, 10)]);
        // 2 x 100 + 3 x 10 = 230 -> ceil(2.3) = 3; type 122 has no score percent and adds nothing
        let notes = vec![note(1, 500, 1), note(2, 900, 120), note(3, 900, 120), note(4, 700, 1), note(5, 950, 120)];
        let mut with_hidden = notes.clone();
        with_hidden.push(note(6, 1200, 122));
        let c = Chart::from_notes(with_hidden.clone(), vec![], &s).unwrap();
        assert_eq!(c.converted_note_count, 3);
        assert_eq!(c.last_timing_note_ms, 1200);
        assert_eq!(judgement_note_total_count(&with_hidden), 5);
        // a multiple of 100 stays exact
        assert_eq!(converted_note_count(&notes[..1], &s), 1);
        assert_eq!(converted_note_count(&[], &s), 0);
        assert!(Chart::from_notes(vec![], vec![], &s).is_err());
    }

    #[test]
    fn judgement_note_types() {
        for t in [0, 80, 82, 100, 103, 121, 122, 123] {
            assert!(!is_judgement_note(t));
        }
        for t in [1, 20, 21, 22, 40, 41, 42, 60, 61, 62, 63, 101, 102, 104, 105, 120] {
            assert!(is_judgement_note(t));
        }
    }
}
