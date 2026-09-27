//! Per-note score, combo bonus table, frames and the score settings.
//!
//! Every float operation is binary32 in the grouping the game uses; float multiplication is commutative but not
//! associative, so the parenthesisation below is part of the result.

use std::collections::HashMap;

use crate::error::Error;
use crate::master::Master;
use crate::num::{ceil_to_i32, floor_to_i32, min_ignoring_nan, trunc_to_i32};

/// Judgement score types.
pub const JUST: i32 = 1;
pub const PERFECT: i32 = 2;
pub const GREAT: i32 = 3;
pub const GOOD: i32 = 4;
pub const BAD: i32 = 5;
pub const MISS: i32 = 6;

/// Combo bonus types.
pub const COMBO: i32 = 0;
pub const GEKISOU_COMBO: i32 = 1;
/// Number of combo bonus types.
pub const COMBO_BONUS_TYPE_COUNT: usize = 2;
/// Note type of combo-only notes (not scored by the skip).
pub const NOTE_TYPE_COMBO: i32 = 120;

/// The 0.005 step of the level factor.
const LEVEL_STEP: f32 = f32::from_bits(0x3ba3_d70a);

/// Frame of a music time: 0 below 0 ms, else `ceil(ms / 40f)`; frame k covers (40(k-1), 40k] ms.
#[inline]
pub fn get_frame(ms: i32) -> i32 {
    if ms < 0 {
        return 0;
    }
    ceil_to_i32(ms as f32 / 40f32)
}

/// Luck percentage: `min(added + 100, 200)` with a wrapping add.
#[inline]
pub fn get_luck_factor_percent(added_luck_bonus: i32) -> i32 {
    let v = added_luck_bonus.wrapping_add(100);
    if v < 200 { v } else { 200 }
}

/// Level factor: `(float)(level - 5) * 0.005f + 1f`.
#[inline]
pub fn get_music_score_level_factor(music_score_level: i32) -> f32 {
    music_score_level.wrapping_sub(5) as f32 * LEVEL_STEP + 1f32
}

/// Combo factor from the table value and the skill factor: `(min(table, 1) + 1) + skill`.
#[inline]
pub fn get_combo_bonus_factor(_combo: i32, combo_score_up_skill_factor: f32, combo_bonus_master_factor: f32) -> f32 {
    (min_ignoring_nan(combo_bonus_master_factor, 1f32) + 1f32) + combo_score_up_skill_factor
}

/// Judgement conversion from the note judgement (-1 None, 0 Wait, 1 Miss .. 6 Just, 7 Pass) to the score type.
pub fn convert_score_type(note_simulate_judgement: i64) -> Result<i32, Error> {
    const TABLE: [i32; 9] = [0, 0, 6, 5, 4, 3, 2, 1, 0];
    let i = note_simulate_judgement + 1;
    if !(0..9).contains(&i) {
        return Err(Error::Game(format!("judgement {note_simulate_judgement} out of range")));
    }
    Ok(TABLE[i as usize])
}

/// Combo thresholds and cumulative factors per combo bonus type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComboTable {
    /// `thresholds[type]`: required combo counts in ascending order (`None`: absent).
    pub thresholds: Option<Vec<Option<Vec<i32>>>>,
    /// `cumulatives[type]`: running binary32 sums of the bonus factors.
    pub cumulatives: Option<Vec<Option<Vec<f32>>>>,
}

impl ComboTable {
    /// Rows grouped by type and sorted by required count; the cumulative value is the running binary32 sum.
    /// Equal counts inside one type make the order undefined and are rejected.
    pub fn build(rows: impl IntoIterator<Item = (i64, i64, f32)>) -> Result<ComboTable, Error> {
        let mut groups: Vec<(i64, Vec<(i64, f32)>)> = Vec::new();
        for (t, count, factor) in rows {
            match groups.iter_mut().find(|g| g.0 == t) {
                Some(g) => g.1.push((count, factor)),
                None => groups.push((t, vec![(count, factor)])),
            }
        }
        let mut th: Vec<Option<Vec<i32>>> = vec![None; COMBO_BONUS_TYPE_COUNT];
        let mut cu: Vec<Option<Vec<f32>>> = vec![None; COMBO_BONUS_TYPE_COUNT];
        for (t, mut g) in groups {
            let mut counts: Vec<i64> = g.iter().map(|x| x.0).collect();
            counts.sort_unstable();
            if counts.windows(2).any(|w| w[0] == w[1]) {
                return Err(Error::Master(format!("combo bonus type {t}: equal required combo counts")));
            }
            g.sort_by_key(|x| x.0);
            let mut acc = 0f32;
            let mut cum = Vec::with_capacity(g.len());
            for &(_, f) in &g {
                acc += f;
                cum.push(acc);
            }
            if !(0..COMBO_BONUS_TYPE_COUNT as i64).contains(&t) {
                return Err(Error::Master(format!("combo bonus type {t} out of range")));
            }
            th[t as usize] = Some(g.iter().map(|x| x.0 as i32).collect());
            cu[t as usize] = Some(cum);
        }
        Ok(ComboTable { thresholds: Some(th), cumulatives: Some(cu) })
    }

    /// The master's combo table.
    pub fn from_master(master: &Master) -> Result<ComboTable, Error> {
        ComboTable::build(
            master.combo_score_bonuses.iter().map(|r| (r.combo_bonus_type, r.required_combo_count, r.bonus_factor)),
        )
    }

    /// The cumulative factor at the last threshold `<= combo`; 0 below the first threshold, for a negative type, a
    /// type beyond the table or an empty threshold list.
    pub fn get_cumulative_factor(&self, type_: i32, combo: i32) -> Result<f32, Error> {
        if type_ < 0 {
            return Ok(0.0);
        }
        let null = || Error::Game("combo table: null array".into());
        let thresholds = self.thresholds.as_ref().ok_or_else(null)?;
        if type_ as usize >= thresholds.len() {
            return Ok(0.0);
        }
        let cumulatives = self.cumulatives.as_ref().ok_or_else(null)?;
        if type_ as usize >= cumulatives.len() {
            return Err(Error::Game("combo table: index out of range".into()));
        }
        let Some(th) = thresholds[type_ as usize].as_ref() else { return Ok(0.0) };
        if th.is_empty() || th[0] > combo {
            return Ok(0.0);
        }
        let (mut lo, mut hi) = (0usize, th.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if th[mid] > combo {
                hi = mid - 1;
            } else {
                lo = mid;
            }
        }
        let cum = cumulatives[type_ as usize].as_ref().ok_or_else(null)?;
        cum.get(lo).copied().ok_or_else(|| Error::Game("combo table: index out of range".into()))
    }
}

/// The score settings: two factors and the note / judgement percentage tables.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveScoreSettings {
    pub score_adjustment_factor: f32,
    pub life_onus_factor: f32,
    /// Note type -> score percent.
    pub note_factor_percent: HashMap<i32, i32>,
    /// Judgement score type -> score percent.
    pub judgement_score_factor_percent: HashMap<i32, i32>,
}

fn setting_f32(settings: &HashMap<&str, &str>, key: &str) -> Result<f32, Error> {
    let v = settings.get(key).ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
    v.trim().parse::<f32>().map_err(|_| Error::Master(format!("MasterLiveSettings {key} = {v:?} is not a number")))
}

impl LiveScoreSettings {
    /// Judgement rows except Excellent (8), keyed by the converted score type; note rows keyed by note type; the
    /// two factors from the live settings. A duplicate key is a master error.
    pub fn from_master(master: &Master) -> Result<LiveScoreSettings, Error> {
        let mut judge = HashMap::new();
        for r in &master.judgement_parameters {
            if r.note_simulate_judgement == 8 {
                continue;
            }
            let k = convert_score_type(r.note_simulate_judgement)?;
            if judge.insert(k, r.score_percent as i32).is_some() {
                return Err(Error::Master("duplicate judgement parameter".into()));
            }
        }
        let mut note = HashMap::new();
        for r in &master.note_parameters {
            if note.insert(r.note_operate_type as i32, r.score_percent as i32).is_some() {
                return Err(Error::Master("duplicate note parameter".into()));
            }
        }
        let settings: HashMap<&str, &str> =
            master.live_settings.iter().map(|r| (r.key.as_str(), r.value.as_str())).collect();
        Ok(LiveScoreSettings {
            score_adjustment_factor: setting_f32(&settings, "note_score_adjustment_factor")?,
            life_onus_factor: setting_f32(&settings, "note_score_life_onus_factor")?,
            note_factor_percent: note,
            judgement_score_factor_percent: judge,
        })
    }

    /// Note types the skip scores: every type of the note table except combo notes.
    pub fn valid_note_types(&self) -> Vec<i32> {
        let mut v: Vec<i32> = self.note_factor_percent.keys().copied().filter(|&t| t != NOTE_TYPE_COMBO).collect();
        v.sort_unstable();
        v
    }
}

/// The running factor state of a live.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreFactorState {
    pub band_total_power: i32,
    pub combo_score_up: f32,
    pub note_score_up: f32,
    pub just: f32,
    pub perfect: f32,
    pub great: f32,
    pub good: f32,
    pub added_luck_bonus: i32,
    pub gekisou_rank_bonus_score: i32,
}

impl ScoreFactorState {
    pub fn new(band_total_power: i32) -> ScoreFactorState {
        ScoreFactorState {
            band_total_power,
            combo_score_up: 0.0,
            note_score_up: 1.0,
            just: 0.0,
            perfect: 0.0,
            great: 0.0,
            good: 0.0,
            added_luck_bonus: 0,
            gekisou_rank_bonus_score: 0,
        }
    }

    /// The judgement factor of a score type (0 for Bad, Miss and anything else).
    pub fn judgement_factor(&self, score_type: i32) -> f32 {
        match score_type {
            JUST => self.just,
            PERFECT => self.perfect,
            GREAT => self.great,
            GOOD => self.good,
            _ => 0.0,
        }
    }
}

/// Reports whether a Gekisou combo is running at a music time, and its count.
pub trait GekisouComboInfo {
    fn gekisou_combo(&self, time_ms: i32) -> Option<i32>;
}

/// The per-note score calculator.
#[derive(Clone, Debug)]
pub struct LiveScoreCalculator {
    pub score_adjustment_factor: f32,
    pub music_difficulty_factor: f32,
    pub converted_note_count: i32,
    pub life_onus_factor: f32,
    pub event_bonus_factor: f32,
    pub assist_factor: f32,
    pub note_factor_percent: HashMap<i32, i32>,
    pub judgement_score_factor_percent: HashMap<i32, i32>,
    pub state: ScoreFactorState,
    pub combo_table: Option<ComboTable>,
}

impl LiveScoreCalculator {
    /// A calculator with the initial state (band total power = the deck power, note factor 1, the rest 0).
    pub fn new(
        total_power: i32,
        music_score_level: i32,
        converted_note_count: i32,
        settings: &LiveScoreSettings,
        event_bonus_factor: f32,
        assist_factor: f32,
        combo_table: Option<ComboTable>,
    ) -> LiveScoreCalculator {
        LiveScoreCalculator {
            score_adjustment_factor: settings.score_adjustment_factor,
            music_difficulty_factor: get_music_score_level_factor(music_score_level),
            converted_note_count,
            life_onus_factor: settings.life_onus_factor,
            event_bonus_factor,
            assist_factor,
            note_factor_percent: settings.note_factor_percent.clone(),
            judgement_score_factor_percent: settings.judgement_score_factor_percent.clone(),
            state: ScoreFactorState::new(total_power),
            combo_table,
        }
    }

    fn table(&self, type_: i32, combo: i32) -> Result<f32, Error> {
        match &self.combo_table {
            None => Ok(0.0),
            Some(t) => t.get_cumulative_factor(type_, combo),
        }
    }

    /// The Gekisou combo factor: 1 without a running Gekisou combo, else `min(table, 1) + 1`.
    pub fn gekisou_combo_bonus_factor(&self, info: Option<&dyn GekisouComboInfo>, time_ms: i32) -> Result<f32, Error> {
        let Some(info) = info else { return Ok(1.0) };
        let Some(c) = info.gekisou_combo(time_ms) else { return Ok(1.0) };
        Ok(min_ignoring_nan(self.table(GEKISOU_COMBO, c)?, 1f32) + 1f32)
    }

    /// Score of one note from the current state.
    pub fn note_score(
        &self,
        current_combo: i32,
        current_life: i32,
        time_ms: i32,
        note_operate_type: i32,
        score_type: i32,
        gekisou: Option<&dyn GekisouComboInfo>,
    ) -> Result<i32, Error> {
        let cum = self.table(COMBO, current_combo)?;
        let combo_up = self.state.combo_score_up;
        let gk = self.gekisou_combo_bonus_factor(gekisou, time_ms)?;
        let score_up = self.state.note_score_up + self.state.judgement_factor(score_type);
        let luck = get_luck_factor_percent(self.state.added_luck_bonus);
        let combo_factor = gk * (combo_up + (min_ignoring_nan(cum, 1f32) + 1f32));
        self.note_score_core(current_life, note_operate_type, score_type, combo_factor, score_up, luck)
    }

    /// The note score from explicit combo, score-up and luck factors.
    pub fn note_score_core(
        &self,
        current_life: i32,
        note_operate_type: i32,
        score_type: i32,
        combo_bonus_factor: f32,
        score_up_factor: f32,
        luck_score_factor_percent: i32,
    ) -> Result<i32, Error> {
        let note_pct = *self
            .note_factor_percent
            .get(&note_operate_type)
            .ok_or_else(|| Error::Game(format!("note type {note_operate_type} has no score percent")))?;
        let judge_pct = *self
            .judgement_score_factor_percent
            .get(&score_type)
            .ok_or_else(|| Error::Game(format!("score type {score_type} has no score percent")))?;
        let life = if current_life > 0 { 1f32 } else { self.life_onus_factor };
        let t = (self.score_adjustment_factor * self.state.band_total_power as f32) * self.music_difficulty_factor;
        let a = (note_pct as f32 / 100f32) * t;
        let b = (judge_pct as f32 / 100f32) * a;
        let c = (b * combo_bonus_factor) * score_up_factor;
        let d = (luck_score_factor_percent as f32 / 100f32) * c;
        let x = d / self.converted_note_count as f32;
        let y = floor_as_float(x);
        let z = self.assist_factor * (life * (y * self.event_bonus_factor));
        Ok(floor_to_i32(z))
    }
}

/// Floor, truncated through `i32` and back to float, with `+inf` giving `-2^31`.
#[inline]
fn floor_as_float(x: f32) -> f32 {
    let f = x.floor();
    if f == f32::INFINITY {
        return -2147483648f32;
    }
    trunc_to_i32(f) as f32
}
