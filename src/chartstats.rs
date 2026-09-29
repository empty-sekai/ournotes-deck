//! Chart statistics: what a chart contributes to the live score whatever the deck, measured on the whole-live
//! simulation ([`crate::live::full`]) with Gekisou on, as the game plays every live.
//!
//! The play is the theoretical best play of a live with Gekisou ([`JudgementStream::theoretical_best_gekisou`]): every
//! judged note at its exact time, Just inside the Just-count ranges (where the game enables the Just judgement) and
//! Perfect elsewhere, the frame delta times of the default schedule. The Gekisou ranges are the chart's fevers with the
//! song's missions ([`Scenario::Free`]); a solo player takes rank 1, so every completed range adds its rank bonus.
//! Luck ranges draw lottery results (and luck rushes, which raise the note score) from the play's random seed, so
//! everything is given per seed of a seed set: one seed when no range is a luck range (the play then draws nothing),
//! else the first [`published_seeds`]. The seed set is not the game's seed law (unknown), so a mean over it is not a
//! native expectation.
//!
//! Numbers, per seed, at the measurement power [`POWER`]:
//! - `score`: the exact no-skill score, rank bonuses included;
//! - per score-up kind ([`Kind`]: a live skill effect row of type 2000, 2002, 2004 or 2005 with its duration,
//!   targets and conditions, value aside) and performance position `k`: `weights[kind][k]`, the exact score gained
//!   by a deck whose position-`k` member has one such effect at a factor of 1 (value 10000), divided by
//!   [`POWER`]. The skill runs through the simulation's own updaters, conditions, frames and appliers, so the
//!   weight carries every rule of the game: its execute and finish frames, the 40 ms score frames, the combo and
//!   Gekisou combo factors, Just scores, luck rushes and the rank bonuses of the ranges it overlaps.
//!
//! A deck of these kinds scores, up to the floors, `P * (score / POWER + sum_k factor_k * weights[kind_k][k])` with
//! `factor` the effect's factor as the applier converts it ([`kind_factor`]). [`SeedStats::check`] plays a random
//! deck of real master values at another power and bounds the deviation; a chart whose deviation exceeds the bound
//! fails. Effects of other types (cumulative score 2001 / 2003, life, judgement conversion, Gekisou skills, snap
//! skills) are not linear in the chart alone: a deck's score comes from the simulation itself.

use serde::Serialize;

use crate::data::{DataChart, DeckData};
use crate::error::Error;
use crate::live::full::{self, GekisouSetup, LiveNote, LiveParams, LivePlay, Performer};
use crate::live::model::{JudgementStream, JustRule};
use crate::live::score::{ComboTable, LiveScoreSettings};
use crate::live::seeds::published_seeds;
use crate::live::skill::{judgement_factor_mill, note_factor_mill};
use crate::live::skip::{Chart, SkipEvaluator, judgement_note_total_count};
use crate::master::{LiveSkillEffectRow, Master};
use crate::num::ceil_to_i32;
use crate::scenario::Scenario;

/// Output format name.
pub const FORMAT: &str = "ournotes-deck.chart-stats/2";
/// Deck power of the measurements (a power range of real decks).
pub const POWER: i32 = 300_000;
/// Deck power of the check deck.
pub const CHECK_POWER: i32 = 1_000_003;
/// Default number of seeds when a chart has a luck range.
pub const GEKISOU_SEEDS: usize = 8;
/// The effect value of factor 1 (`value / 10000`).
pub const UNIT_VALUE: i64 = 10000;
/// The most fevers a live can play: the game keeps three Gekisou ranges (`LiveMusicScore.GetGekisouRanges`
/// 0x55d0694) and fails when a fourth fever starts (`GekisouController.BeforeUpdate` 0x55d4838, index out of range
/// at 0x55d4e30).
pub const MAX_GEKISOU_FEVERS: usize = 3;
/// The score-up effect types whose score is linear in the effect's factor.
pub const KIND_TYPES: [i64; 4] = [2000, 2002, 2004, 2005];
/// The difficulty of a score id among its song's four.
pub const DIFFICULTIES: [&str; 4] = ["easy", "normal", "hard", "expert"];

/// Gekisou mission of the luck ranges.
const MISSION_LUCK: i64 = 2;
/// `NoteSimulateJudgement` of a Just.
const SIMULATE_JUST: i32 = 6;
/// Live skill id of the measurement skill of kind `i`: `KIND_SKILL_BASE - i`, below every real id.
const KIND_SKILL_BASE: i64 = -1_000_000;
/// Live skill id of the check deck's skill at position `k`: `CHECK_SKILL_BASE - k`.
const CHECK_SKILL_BASE: i64 = -2_000_000;

/// A score-up kind: the fields of a live skill effect row that shape its score, the value aside.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kind {
    pub id: usize,
    pub effect_type: i64,
    pub activation_time_second: f32,
    /// `ceil(activationTimeSecond * 1000f)`, for display.
    pub duration_ms: i32,
    pub skill_target_ids: Vec<i64>,
    pub skill_condition_group: i64,
    pub skill_release_condition_group: i64,
    pub effect_limit_count: i64,
    pub effect_execute_limit_count: i64,
    pub effect_execute_limit_reset_condition_group: i64,
    /// Master rows of this kind.
    pub rows: usize,
    /// Distinct values of those rows, ascending.
    pub values: Vec<i64>,
}

impl Kind {
    fn key(r: &LiveSkillEffectRow) -> (i64, u32, Vec<i64>, i64, i64, i64, i64, i64) {
        (
            r.skill_effect_type,
            r.activation_time_second.to_bits(),
            r.skill_target_ids.clone(),
            r.skill_condition_group,
            r.skill_release_condition_group,
            r.effect_limit_count,
            r.effect_execute_limit_count,
            r.effect_execute_limit_reset_condition_group,
        )
    }

    /// A live skill effect row of this kind at a value.
    fn row(&self, id: i64, live_skill_id: i64, value: i64) -> LiveSkillEffectRow {
        LiveSkillEffectRow {
            id,
            live_skill_id,
            level: 1,
            skill_condition_group: self.skill_condition_group,
            skill_release_condition_group: self.skill_release_condition_group,
            skill_target_ids: self.skill_target_ids.clone(),
            skill_effect_type: self.effect_type,
            activation_time_second: self.activation_time_second,
            effect_value: value,
            max_effect_value: 0,
            effect_limit_count: self.effect_limit_count,
            skill_cumulative_condition_id: 0,
            effect_execute_limit_count: self.effect_execute_limit_count,
            effect_execute_limit_reset_condition_group: self.effect_execute_limit_reset_condition_group,
        }
    }
}

/// The score-up kinds of a master: its live skill effect rows of [`KIND_TYPES`] without a cumulative condition,
/// grouped by [`Kind`], in order of first appearance by row id.
pub fn kinds(master: &Master) -> Vec<Kind> {
    let mut rows: Vec<&LiveSkillEffectRow> = master
        .live_skill_effects
        .iter()
        .filter(|r| KIND_TYPES.contains(&r.skill_effect_type) && r.skill_cumulative_condition_id == 0)
        .collect();
    rows.sort_by_key(|r| r.id);
    let mut out: Vec<Kind> = Vec::new();
    let mut keys = Vec::new();
    for r in rows {
        let key = Kind::key(r);
        let i = match keys.iter().position(|k| *k == key) {
            Some(i) => i,
            None => {
                keys.push(key);
                out.push(Kind {
                    id: out.len(),
                    effect_type: r.skill_effect_type,
                    activation_time_second: r.activation_time_second,
                    duration_ms: ceil_to_i32(r.activation_time_second * 1000f32),
                    skill_target_ids: r.skill_target_ids.clone(),
                    skill_condition_group: r.skill_condition_group,
                    skill_release_condition_group: r.skill_release_condition_group,
                    effect_limit_count: r.effect_limit_count,
                    effect_execute_limit_count: r.effect_execute_limit_count,
                    effect_execute_limit_reset_condition_group: r.effect_execute_limit_reset_condition_group,
                    rows: 0,
                    values: Vec::new(),
                });
                out.len() - 1
            }
        };
        out[i].rows += 1;
        if !out[i].values.contains(&r.effect_value) {
            out[i].values.push(r.effect_value);
        }
    }
    for k in &mut out {
        k.values.sort_unstable();
    }
    out
}

/// The factor of an effect of a kind at a value, as its applier converts it: 2000 `floor(value / 10000f * 1e5)`,
/// 2005 `floor(value / -10000f * 1e5)`, 2002 / 2004 the same quotient rounded half to even; divided by 1e5.
pub fn kind_factor(effect_type: i64, value: i64) -> f64 {
    let mill = match effect_type {
        2000 => note_factor_mill(value as f32 / 10000f32),
        2005 => note_factor_mill(value as f32 / -10000f32),
        _ => judgement_factor_mill(value as f32 / 10000f32),
    };
    mill as f64 / 100000.0
}

/// One Gekisou range of the chart.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeInfo {
    pub index: usize,
    /// 1 combo, 2 luck, 3 Just count.
    pub mission: i64,
    pub start_ms: i32,
    pub end_ms: i32,
    /// The solo (rank 1) rank bonus percentage of the song's mission pattern.
    pub rank_bonus_percent: i64,
}

/// A range on one seed's no-skill play.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeResult {
    /// Score gained inside the range (the score at its end minus the score at its start).
    pub range_score: i32,
    pub rank_bonus: i32,
    pub max_combo: i32,
    pub just_count: i32,
    /// Lottery results drawn: Miss, Hit, Super Hit, Critical.
    pub lot_results: [i32; 4],
}

/// The check deck of a seed: a random deck of real master values at [`CHECK_POWER`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// `(kind, value)` of each position, `None` for no skill.
    pub deck: Vec<Option<(usize, i64)>>,
    pub exact: i32,
    pub predicted: f64,
    pub bound: f64,
}

/// One seed's measurements at [`POWER`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedStats {
    pub seed: i32,
    /// The exact no-skill score, rank bonuses included.
    pub score: i32,
    pub ranges: Vec<RangeResult>,
    /// `weights[kind][position]`: score gained per unit of deck power and of the effect's factor.
    pub weights: Vec<Vec<f64>>,
    pub check: Check,
}

/// The statistics of one chart.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartStats {
    pub score_id: i64,
    pub music_id: i64,
    pub difficulty: &'static str,
    pub level: i32,
    pub judged_notes: i32,
    pub converted_note_count: i32,
    /// Time of the last timing note.
    pub last_note_ms: i32,
    /// The live's music length on the score path: the last timing note + 1000 ms.
    pub music_length_ms: i32,
    /// Score per unit of deck power of a skipped live (every note Great, combo 0, no skills; the real-valued sum of
    /// the skip score's per-note chain).
    pub skip: f64,
    /// Skill events `(position, time ms)` in chart order.
    pub events: Vec<(i32, i32)>,
    /// Performance positions the events fire (the largest event position + 1).
    pub positions: usize,
    /// The song's three missions.
    pub missions: [i64; 3],
    pub ranges: Vec<RangeInfo>,
    /// Notes judged Just on the play.
    pub just_notes: i32,
    /// Measurements per seed; empty when the game cannot play the chart ([`ChartStats::unplayable`]).
    pub seeds: Vec<SeedStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unplayable: Option<String>,
}

/// The song and difficulty of a score id.
pub fn song_of_score(master: &Master, score_id: i64) -> Result<(i64, &'static str), Error> {
    for m in &master.live_musics {
        let ids = [m.easy_id, m.normal_id, m.hard_id, m.expert_id];
        if let Some(d) = ids.iter().position(|&x| x == score_id) {
            return Ok((m.id, DIFFICULTIES[d]));
        }
    }
    Err(Error::Input(format!("no live music has chart {score_id}")))
}

/// A small deterministic generator for the check decks (xorshift64*).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % n.max(1)
    }
}

/// The simulation inputs of a chart's play.
struct Live<'m> {
    master: &'m Master,
    /// The master with one measurement skill per kind at [`UNIT_VALUE`].
    measure: Master,
    notes: Vec<LiveNote>,
    events: Vec<(i32, i32)>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    dt: Vec<f32>,
    positions: usize,
}

impl Live<'_> {
    /// Plays the live with one live skill per position (`None`: no skill) on a master, at a power and seed.
    fn run(
        &self,
        master: &Master,
        skills: &[Option<i64>],
        power: i32,
        seed: i32,
    ) -> Result<(i32, Vec<full::GekisouRange>), Error> {
        let deck: Vec<Performer> =
            skills.iter().map(|s| Performer { live_skill: s.map(|id| (id, 1)), ..Default::default() }).collect();
        let params = LiveParams { total_power: power, ..self.params };
        let mut lm = full::LiveModel::new_gekisou(master, &deck, &self.notes, &self.events, params, &self.setup)?;
        let mut play = self.play.clone();
        play.base_seed = seed;
        let score = lm.run_timed(&play, &self.dt)?;
        Ok((score, lm.gekisou_ranges()))
    }

    /// A master with these skills added, `(live skill id, kind, value)`, one effect row each.
    fn master_with(master: &Master, skills: &[(i64, &Kind, i64)]) -> Master {
        let mut m = master.clone();
        let next = m.live_skill_effects.iter().map(|r| r.id).max().unwrap_or(0);
        for (i, &(id, kind, value)) in skills.iter().enumerate() {
            m.live_skill_effects.push(kind.row(next + 1 + i as i64, id, value));
        }
        m
    }

    fn seed_stats(&self, kinds: &[Kind], seed: i32, rng: &mut Rng, judged: i32) -> Result<SeedStats, Error> {
        let none = vec![None; self.positions];
        let (score, gk) = self.run(self.master, &none, POWER, seed)?;
        let ranges = gk
            .iter()
            .map(|r| RangeResult {
                range_score: r.end_score.wrapping_sub(r.start_score),
                rank_bonus: r.rank_bonus.unwrap_or(0),
                max_combo: r.max_combo,
                just_count: r.just_count,
                lot_results: r.lot_results,
            })
            .collect();
        let mut weights = vec![vec![0f64; self.positions]; kinds.len()];
        for (ki, kind) in kinds.iter().enumerate() {
            let unit = kind_factor(kind.effect_type, UNIT_VALUE);
            for k in 0..self.positions {
                let mut skills = none.clone();
                skills[k] = Some(KIND_SKILL_BASE - ki as i64);
                let (s, _) = self.run(&self.measure, &skills, POWER, seed)?;
                weights[ki][k] = (s as f64 - score as f64) / (POWER as f64 * unit);
            }
        }

        // the check deck: a random kind and master value at each position, at another power
        let mut deck = vec![None; self.positions];
        let mut rows = Vec::new();
        if !kinds.is_empty() {
            for (k, slot) in deck.iter_mut().enumerate() {
                if rng.below(4) == 0 {
                    continue;
                }
                let ki = rng.below(kinds.len());
                let value = kinds[ki].values[rng.below(kinds[ki].values.len())];
                *slot = Some((ki, value));
                rows.push((CHECK_SKILL_BASE - k as i64, &kinds[ki], value));
            }
        }
        let master = Self::master_with(self.master, &rows);
        let skills: Vec<Option<i64>> =
            deck.iter().enumerate().map(|(k, d)| d.map(|_| CHECK_SKILL_BASE - k as i64)).collect();
        let (exact, _) = self.run(&master, &skills, CHECK_POWER, seed)?;
        let scale = CHECK_POWER as f64 / POWER as f64;
        let mut per_power = score as f64 / POWER as f64;
        let mut gain = 0f64;
        for (k, d) in deck.iter().enumerate() {
            if let Some((ki, value)) = *d {
                let x = kind_factor(kinds[ki].effect_type, value);
                per_power += x * weights[ki][k];
                gain += x.abs();
            }
        }
        let predicted = CHECK_POWER as f64 * per_power;
        // floors: one point per judged note and per range bonus in each run; the measured weights carry the
        // floors of two runs, scaled by the power ratio and the factors; the binary32 chain a few ulps
        let floors = (judged as f64 + MAX_GEKISOU_FEVERS as f64) * (1.0 + scale * (1.0 + 2.0 * gain));
        let bound = floors + 4e-6 * predicted.abs();
        if (exact as f64 - predicted).abs() > bound {
            return Err(Error::Domain(format!(
                "seed {seed}: the check deck scores {exact}, the chart statistics predict {predicted:.1} (bound {bound:.1})"
            )));
        }
        Ok(SeedStats { seed, score, ranges, weights, check: Check { deck, exact, predicted, bound } })
    }
}

/// The statistics of one chart for these kinds; `seeds` is the size of the seed set when a range is a luck range.
pub fn chart_stats(master: &Master, chart: &DataChart, kinds: &[Kind], seeds: usize) -> Result<ChartStats, Error> {
    let settings = LiveScoreSettings::from_master(master)?;
    let c: Chart = chart.chart(&settings)?;
    let row = master
        .live_music_score(chart.score_id)
        .ok_or_else(|| Error::Input(format!("chart {}: no MasterLiveMusicScore row", chart.score_id)))?;
    let level = row.music_score_level as i32;
    let (music_id, difficulty) = song_of_score(master, chart.score_id)?;
    if chart.judgement_types.len() != c.notes.len() {
        return Err(Error::Input(format!("chart {}: judgement types do not match the notes", chart.score_id)));
    }
    let judged = judgement_note_total_count(&c.notes);
    let skip = SkipEvaluator::new(
        level,
        &c,
        &settings,
        &settings.valid_note_types(),
        Some(&ComboTable::from_master(master)?),
    )?
    .coefficient_sum();
    let resolved = Scenario::Free(music_id).resolve(master)?;
    let setup = resolved.gekisou_setup(&chart.fevers);
    let factors = full::gekisou_rank_factors(master, &resolved.gekisou_missions)?;
    let ranges: Vec<RangeInfo> = chart
        .fevers
        .iter()
        .take(MAX_GEKISOU_FEVERS)
        .enumerate()
        .map(|(i, &(start_ms, end_ms))| RangeInfo {
            index: i,
            mission: resolved.gekisou_missions[i.min(2)],
            start_ms,
            end_ms,
            rank_bonus_percent: factors.get(i).map_or(0, |f| f[0]),
        })
        .collect();
    let events: Vec<(i32, i32)> = c.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
    let positions = events.iter().map(|e| e.0.max(0) as usize + 1).max().unwrap_or(0);
    let music_length_ms = c.last_timing_note_ms.wrapping_add(1000);
    let mut out = ChartStats {
        score_id: chart.score_id,
        music_id,
        difficulty,
        level,
        judged_notes: judged,
        converted_note_count: c.converted_note_count,
        last_note_ms: c.last_timing_note_ms,
        music_length_ms,
        skip,
        events: events.clone(),
        positions,
        missions: resolved.gekisou_missions,
        ranges,
        just_notes: 0,
        seeds: Vec::new(),
        unplayable: None,
    };
    if chart.fevers.len() > MAX_GEKISOU_FEVERS {
        out.unplayable = Some(format!(
            "{} fevers: the game fails when the fourth fever starts (GekisouController.BeforeUpdate 0x55d4838)",
            chart.fevers.len()
        ));
        return Ok(out);
    }
    let rule = JustRule::new(master, &setup)?;
    let stream = JudgementStream::theoretical_best_gekisou(&c, &chart.judgement_types, &rule)?;
    out.just_notes = stream.judged.iter().filter(|r| r[2] == SIMULATE_JUST).count() as i32;
    let unit: Vec<(i64, &Kind, i64)> =
        kinds.iter().enumerate().map(|(i, k)| (KIND_SKILL_BASE - i as i64, k, UNIT_VALUE)).collect();
    let live = Live {
        master,
        measure: Live::master_with(master, &unit),
        notes: c
            .notes
            .iter()
            .zip(&chart.judgement_types)
            .map(|(n, &jt)| LiveNote {
                note_id: n.id,
                time_ms: n.time_ms,
                note_operate_type: n.note_type,
                judgement_type: jt,
            })
            .collect(),
        events,
        params: LiveParams {
            skill_target_music_type: resolved.skill_target_music_type,
            total_power: POWER,
            music_level: level,
            converted_note_count: c.converted_note_count,
            music_length_ms,
            score_music_length_ms: None,
            assist_factor: 1.0,
        },
        dt: stream.delta_times()?,
        play: stream.to_live_play()?,
        setup,
        positions,
    };
    let luck = live.setup.missions.iter().take(live.setup.fevers.len()).any(|&m| m == MISSION_LUCK);
    let seed_list = if luck { published_seeds(seeds.max(1)) } else { vec![0] };
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ chart.score_id as u64);
    for seed in seed_list {
        out.seeds.push(live.seed_stats(kinds, seed, &mut rng, judged)?);
    }
    Ok(out)
}

/// The statistics of every chart of a deck data file (score id order) as the `ournotes-deck.chart-stats/2` document;
/// `seeds` is the size of the seed set of charts with a luck range (default [`GEKISOU_SEEDS`]).
pub fn document(data: &DeckData, seeds: Option<usize>) -> Result<serde_json::Value, Error> {
    let seeds = seeds.unwrap_or(GEKISOU_SEEDS);
    if seeds == 0 {
        return Err(Error::Input("an empty seed set".into()));
    }
    let kinds = kinds(&data.master);
    let mut charts = Vec::with_capacity(data.charts.len());
    for c in &data.charts {
        charts.push(chart_stats(&data.master, c, &kinds, seeds).map_err(|e| match e {
            Error::Domain(m) => Error::Domain(format!("chart {}: {m}", c.score_id)),
            e => e,
        })?);
    }
    let source = serde_json::json!({
        "format": crate::data::FORMAT,
        "region": data.provenance.get("region"),
        "master": data.provenance.get("master").map(|m| serde_json::json!({
            "source": m.get("source"), "version": m.get("version")
        })),
        "exporter": data.provenance.get("exporter"),
    });
    Ok(serde_json::json!({
        "format": FORMAT,
        "source": source,
        "model": {
            "engine": "whole-live simulation (live::full), Gekisou on, solo rank 1",
            "play": "theoretical best: exact note times, Just inside Just-count ranges, Perfect elsewhere",
            "score": "P * (score / power + sum_k factor_k * weights[kind_k][k]) up to the floors; checked per seed",
            "power": POWER,
            "checkPower": CHECK_POWER,
            "unitValue": UNIT_VALUE,
            "seeds": "one seed without a luck range, else the first published seeds; not a native expectation",
        },
        "kinds": kinds,
        "charts": charts,
    }))
}
