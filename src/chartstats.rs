//! Chart statistics: what a chart contributes to the live score whatever the deck, measured on the whole-live
//! simulation ([`crate::live::full`]) in two scenarios: with Gekisou on (`seeds`), as a Gekisou live (Battle Live)
//! plays, and with Gekisou off (`off_seeds`), as a solo live (Free Live, Challenge Live) plays.
//!
//! With Gekisou on, the play is the theoretical best play of a live with Gekisou
//! ([`JudgementStream::theoretical_best_gekisou`]): every judged note at its exact time, Just inside the Just-count
//! ranges (where the game enables the Just judgement) and Perfect elsewhere, the frame delta times of the default
//! schedule. The Gekisou ranges are the chart's fevers with the song's missions ([`Scenario::Free`]); the player takes
//! rank 1 in every range, so every completed range adds its rank 1 bonus. Luck ranges draw lottery results (and luck
//! rushes, which raise the note score) from the play's random seed, so everything is given per seed of a seed set: one
//! seed when no range is a luck range (the play then draws nothing), else the first [`published_seeds`]. The seed set
//! is not the game's seed law (unknown), so a mean over it is not a native expectation.
//!
//! Other ranks follow without playing again. A range's rank bonus is `trunc(rangeScore * percent / 100)`, added as a
//! fixed score in the frame of the range's end; it changes no factor and no note score, and a range's score (its end
//! score minus its start score) holds an earlier range's bonus at both ends, so it does not depend on the ranks. At
//! ranks `r_i` the no-skill score is then exact ([`SeedStats::score_at_ranks`]) and every weight moves by the range
//! weights ([`SeedStats::weights_at_ranks`]). The ranks reach the skills only through the confirmed-rank condition
//! 7012: a kind whose condition groups have it gets no range weights, and a chart where a range's bonus falls inside
//! another range's score frames gets none at all.
//!
//! Each seed also plays the no-skill live on the Perfect play: the same play with every Just judged Perfect instead
//! (Just is enabled only inside the Just-count ranges, so nothing else changes), for a Just rate between the two.
//!
//! With Gekisou off, the play is the theoretical best play of a live without Gekisou
//! ([`JudgementStream::theoretical_best`]): every judged note Perfect at its exact time, seed [`OFF_SEED`]; there is
//! no Just, luck, Gekisou combo or rank bonus, and a chart with more than three fevers plays as any other. A kind
//! whose conditions read the Gekisou state cannot play there and has no weights.
//!
//! Numbers, per seed, at the measurement power [`POWER`]:
//! - `score`: the exact no-skill score, rank bonuses included;
//! - per score-up kind ([`Kind`]: a live skill effect row of type 2000, 2002, 2004 or 2005 with its duration,
//!   targets and conditions, value aside) and performance position `k`: `weights[kind][k]`, the exact score gained
//!   by a deck whose position-`k` member has one such effect at a factor of 1 (value 10000), divided by
//!   [`POWER`]. The skill runs through the simulation's own updaters, conditions, frames and appliers, so the
//!   weight carries every rule of the game: its execute and finish frames, the 40 ms score frames, the combo and
//!   Gekisou combo factors, Just scores, luck rushes and the rank bonuses of the ranges it overlaps;
//! - with Gekisou on, per range `i` also `range_weights[kind][k][i]`: the range score that effect gains, divided by
//!   [`POWER`].
//!
//! A deck of these kinds scores, up to the floors, `P * (score / power + sum_k factor_k * weights[kind_k][k])` with
//! `factor` the effect's factor as the applier converts it ([`kind_factor`]). [`SeedStats::check`] plays a random
//! deck of real master values at another power and bounds the deviation, [`SeedStats::rank_check`] the same deck at
//! random ranks and [`OffSeedStats::check`] a random deck with Gekisou off; a chart whose deviation exceeds a bound
//! fails. Effects of other types (cumulative score 2001 / 2003, life, judgement conversion, Gekisou skills, snap
//! skills) are not linear in the chart alone: a deck's score comes from the simulation itself.

use serde::Serialize;

mod aptitude;

pub use aptitude::{
    AptitudeHeader, AptitudeOptions, ChartAptitude, Condition, Cumulative, Effect, RangeDelta, RangeFactors, SeedRule,
    Shape, ShapeSkill, Variant, VariantCheck, aptitude_header, shapes,
};

use crate::data::{DataChart, DeckData};
use crate::error::Error;
use crate::live::full::{self, GekisouSetup, LiveNote, LiveParams, LivePlay, Performer};
use crate::live::model::{JudgementStream, JustRule};
use crate::live::score::{ComboTable, LiveScoreSettings, get_frame};
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
/// `NoteSimulateJudgement` of a Just and of a Perfect.
const SIMULATE_JUST: i32 = 6;
const SIMULATE_PERFECT: i32 = 5;
/// Live skill id of the measurement skill of kind `i`: `KIND_SKILL_BASE - i`, below every real id.
const KIND_SKILL_BASE: i64 = -1_000_000;
/// Live skill id of the check deck's skill at position `k`: `CHECK_SKILL_BASE - k`.
const CHECK_SKILL_BASE: i64 = -2_000_000;
/// The seed of the Gekisou off play.
pub const OFF_SEED: i32 = 0;
/// The ranks of a range: 1..=RANKS.
pub const RANKS: usize = 5;
/// Skill condition type of the previous frame's confirmed rank.
const CONDITION_CONFIRMED_RANK: i64 = 7012;
/// Start states of the generators of the check decks (Gekisou on, Gekisou off) and of the rank checks' ranks, each
/// xored with the score id.
const CHECK_SALT: u64 = 0x9e37_79b9_7f4a_7c15;
const OFF_CHECK_SALT: u64 = 0x6f66_665f_6368_6563;
const RANK_CHECK_SALT: u64 = 0x7261_6e6b_5f63_6865;

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

/// Whether a condition group has a condition of a type.
fn group_has(master: &Master, group: i64, condition_type: i64) -> bool {
    group != 0
        && master.skill_condition_sets.iter().filter(|s| s.group == group).any(|s| {
            s.condition_ids
                .iter()
                .any(|&c| master.skill_condition(c).is_some_and(|c| c.condition_type == condition_type))
        })
}

impl Kind {
    /// Whether the kind's condition, release or reset group reads the previous frame's confirmed rank (condition
    /// 7012).
    pub fn reads_rank(&self, master: &Master) -> bool {
        [
            self.skill_condition_group,
            self.skill_release_condition_group,
            self.effect_execute_limit_reset_condition_group,
        ]
        .iter()
        .any(|&g| group_has(master, g, CONDITION_CONFIRMED_RANK))
    }
}

/// Whether a range's rank bonus can fall inside another range's score: the bonus of range `i` is a fixed score in
/// the 40 ms frame of its end, and range `j`'s score is the score at the frame of its end minus the score at the
/// frame of its start, so the bonus is in it when `frame(start_j) < frame(end_i) <= frame(end_j)`.
fn bonus_inside_a_range(fevers: &[(i32, i32)]) -> bool {
    fevers.iter().enumerate().any(|(i, &(_, end_i))| {
        let f = get_frame(end_i);
        fevers
            .iter()
            .enumerate()
            .any(|(j, &(start_j, end_j))| i != j && get_frame(start_j) < f && f <= get_frame(end_j))
    })
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
    /// The rank 1 bonus percentage of the song's mission pattern (`rank_bonus_percents[0]`).
    pub rank_bonus_percent: i64,
    /// The rank bonus percentages of ranks 1..=5 of the song's mission pattern.
    pub rank_bonus_percents: [i64; RANKS],
}

impl RangeInfo {
    /// The rank bonus percentage at a rank (1..=5).
    pub fn percent(&self, rank: i32) -> Result<i64, Error> {
        usize::try_from(rank.wrapping_sub(1))
            .ok()
            .and_then(|r| self.rank_bonus_percents.get(r).copied())
            .ok_or_else(|| Error::Input(format!("rank {rank} is not in 1..={RANKS}")))
    }
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
    /// Luck points gained (`TotalBonusPoint`, the luck mission's rank figure).
    pub luck_points: i32,
    /// Lottery results drawn: Miss, Hit, Super Hit, Critical.
    pub lot_results: [i32; 4],
    /// The range score on the Perfect play (every Just judged Perfect).
    pub range_score_perfect: i32,
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

/// The check deck of a seed played at random ranks.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankCheck {
    /// The rank of each range (1..=5).
    pub ranks: Vec<i32>,
    pub exact: i32,
    pub predicted: f64,
    pub bound: f64,
}

/// One seed's measurements at [`POWER`], Gekisou on.
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
    /// The exact no-skill score on the Perfect play (every Just judged Perfect), its rank 1 bonuses included.
    pub score_perfect: i32,
    /// `range_weights[kind][position][range]`: range score gained per unit of deck power and of the effect's
    /// factor. `None` when a range's bonus can fall inside another range's score, a kind's `None` when its
    /// conditions read the confirmed rank: the ranks do not follow linearly there.
    pub range_weights: Option<Vec<Option<Vec<Vec<f64>>>>>,
    /// The check deck at random ranks; `None` without ranges, without range weights or when the check deck has a
    /// kind without them.
    pub rank_check: Option<RankCheck>,
}

impl SeedStats {
    fn check_ranks(&self, ranges: &[RangeInfo], ranks: &[i32]) -> Result<(), Error> {
        if ranks.len() != ranges.len() || ranges.len() != self.ranges.len() {
            return Err(Error::Input(format!("{} ranks for {} ranges", ranks.len(), self.ranges.len())));
        }
        Ok(())
    }

    /// The exact no-skill score at [`POWER`] when range `i` takes rank `ranks[i]` (1..=5):
    /// `score - sum_i rankBonus_i + sum_i trunc(rangeScore_i * percent_i(ranks[i]) / 100)`. It holds when
    /// `range_weights` is not `None`.
    pub fn score_at_ranks(&self, ranges: &[RangeInfo], ranks: &[i32]) -> Result<i32, Error> {
        self.check_ranks(ranges, ranks)?;
        let mut score = self.score as i64;
        for ((r, info), &rank) in self.ranges.iter().zip(ranges).zip(ranks) {
            score += r.range_score as i64 * info.percent(rank)? / 100 - r.rank_bonus as i64;
        }
        Ok(score as i32)
    }

    /// `weights[kind][position]` when range `i` takes rank `ranks[i]` (1..=5): `weights[kind][k] + sum_i
    /// (percent_i(ranks[i]) - percent_i(1)) / 100 * range_weights[kind][k][i]`; `None` without range weights, a
    /// kind's `None` without its own.
    pub fn weights_at_ranks(
        &self,
        ranges: &[RangeInfo],
        ranks: &[i32],
    ) -> Result<Option<Vec<Option<Vec<f64>>>>, Error> {
        self.check_ranks(ranges, ranks)?;
        let Some(rw) = &self.range_weights else { return Ok(None) };
        let mut d = Vec::with_capacity(ranges.len());
        for (info, &rank) in ranges.iter().zip(ranks) {
            d.push((info.percent(rank)? - info.percent(1)?) as f64 / 100.0);
        }
        Ok(Some(
            self.weights
                .iter()
                .zip(rw)
                .map(|(w, rw)| {
                    rw.as_ref().map(|rw| {
                        w.iter()
                            .zip(rw)
                            .map(|(&w, r)| w + r.iter().zip(&d).map(|(&r, &d)| d * r).sum::<f64>())
                            .collect()
                    })
                })
                .collect(),
        ))
    }
}

/// The measurements at [`POWER`] of a seed with Gekisou off.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OffSeedStats {
    pub seed: i32,
    /// The exact no-skill score.
    pub score: i32,
    /// `weights[kind][position]`: score gained per unit of deck power and of the effect's factor; a kind's `None`
    /// when its conditions read the Gekisou state, which a live without Gekisou does not have.
    pub weights: Vec<Option<Vec<f64>>>,
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
    /// Measurements per seed with Gekisou on; empty when the game cannot play the chart with Gekisou
    /// ([`ChartStats::unplayable`]).
    pub seeds: Vec<SeedStats>,
    /// Measurements with Gekisou off, one seed ([`OFF_SEED`]); every chart has them.
    pub off_seeds: Vec<OffSeedStats>,
    /// Why the game cannot play the chart with Gekisou (Gekisou off plays it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unplayable: Option<String>,
    /// The chart's aptitude for Gekisou skills: what each Gekisou (support) skill shape adds alone ([`ChartAptitude`]);
    /// `None` when the chart is unplayable with Gekisou, has no Gekisou range, the master has no Gekisou skill or the
    /// statistics leave the aptitude out.
    pub gekisou_aptitude: Option<ChartAptitude>,
}

/// What the chart statistics measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The size of the seed set of a chart with a luck range.
    pub seeds: usize,
    /// The aptitude for Gekisou skills; `None`: left out.
    pub aptitude: Option<AptitudeOptions>,
}

impl Options {
    fn validate(&self) -> Result<(), Error> {
        if self.seeds == 0 {
            return Err(Error::Input("an empty seed set".into()));
        }
        if self.aptitude.is_some_and(|a| a.max_seeds < 2 || a.cross_seeds == 0) {
            return Err(Error::Input("aptitude needs at least two maximum seeds and one cross seed".into()));
        }
        Ok(())
    }
}

impl Default for Options {
    fn default() -> Options {
        Options { seeds: GEKISOU_SEEDS, aptitude: Some(AptitudeOptions::default()) }
    }
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

/// Gekisou of a play: the setup, the play's frame delta times and its Perfect play (every Just judged Perfect).
struct Gekisou {
    setup: GekisouSetup,
    dt: Vec<f32>,
    perfect: LivePlay,
}

/// The simulation inputs of a chart's play.
struct Live<'m> {
    master: &'m Master,
    /// The master with one measurement skill per kind at [`UNIT_VALUE`].
    measure: &'m Master,
    notes: &'m [LiveNote],
    events: &'m [(i32, i32)],
    params: LiveParams,
    /// `None`: Gekisou off.
    gekisou: Option<Gekisou>,
    play: LivePlay,
    positions: usize,
}

/// A check deck: `(kind, value)` per position, and its effect rows `(live skill id, kind, value)`.
type CheckDeck<'k> = (Vec<Option<(usize, i64)>>, Vec<(i64, &'k Kind, i64)>);

/// A random check deck of real master values of the kinds `usable` (indices into `kinds`): each position has no
/// skill with probability 1/4, else a kind and one of its values.
fn check_deck<'k>(kinds: &'k [Kind], usable: &[usize], positions: usize, rng: &mut Rng) -> CheckDeck<'k> {
    let mut deck = vec![None; positions];
    let mut rows = Vec::new();
    if !usable.is_empty() {
        for (k, slot) in deck.iter_mut().enumerate() {
            if rng.below(4) == 0 {
                continue;
            }
            let ki = usable[rng.below(usable.len())];
            let value = kinds[ki].values[rng.below(kinds[ki].values.len())];
            *slot = Some((ki, value));
            rows.push((CHECK_SKILL_BASE - k as i64, &kinds[ki], value));
        }
    }
    (deck, rows)
}

/// The live skill of each position of a check deck.
fn check_skills(deck: &[Option<(usize, i64)>]) -> Vec<Option<i64>> {
    deck.iter().enumerate().map(|(k, d)| d.map(|_| CHECK_SKILL_BASE - k as i64)).collect()
}

/// A check: the exact score of a check deck, the prediction and the bound.
struct Checked {
    exact: i32,
    predicted: f64,
    bound: f64,
}

impl Checked {
    fn within(self, what: impl FnOnce() -> String) -> Result<Checked, Error> {
        if !self.predicted.is_finite()
            || !self.bound.is_finite()
            || self.bound < 0.0
            || (self.exact as f64 - self.predicted).abs() > self.bound
        {
            return Err(Error::Domain(format!(
                "{}: the check deck scores {}, the chart statistics predict {:.1} (bound {:.1})",
                what(),
                self.exact,
                self.predicted,
                self.bound
            )));
        }
        Ok(self)
    }
}

impl Live<'_> {
    /// Plays the live with one live skill per position (`None`: no skill) on a master, at a power and seed; with
    /// `ranks`, range `i` takes the confirmed rank and bonus percentage `ranks[i]` (Gekisou on only).
    fn run(
        &self,
        master: &Master,
        skills: &[Option<i64>],
        power: i32,
        seed: i32,
        ranks: Option<&[(i32, i64)]>,
    ) -> Result<(i32, Vec<full::GekisouRange>), Error> {
        self.run_play(&self.play, master, skills, power, seed, ranks)
    }

    /// [`Live::run`] on another play of the same frames.
    fn run_play(
        &self,
        play: &LivePlay,
        master: &Master,
        skills: &[Option<i64>],
        power: i32,
        seed: i32,
        ranks: Option<&[(i32, i64)]>,
    ) -> Result<(i32, Vec<full::GekisouRange>), Error> {
        self.run_deck(play, master, &[], skills, power, seed, ranks)
    }

    /// [`Live::run_play`] with these performers (a Gekisou formation; empty: none), each position's live skill set.
    #[allow(clippy::too_many_arguments)]
    fn run_deck(
        &self,
        play: &LivePlay,
        master: &Master,
        formation: &[Performer],
        skills: &[Option<i64>],
        power: i32,
        seed: i32,
        ranks: Option<&[(i32, i64)]>,
    ) -> Result<(i32, Vec<full::GekisouRange>), Error> {
        let (score, ranges, _) = self.run_counted(play, master, formation, skills, power, seed, ranks)?;
        Ok((score, ranges))
    }

    /// [`Live::run_deck`], also the number of judgements converted by skills.
    #[allow(clippy::too_many_arguments)]
    fn run_counted(
        &self,
        play: &LivePlay,
        master: &Master,
        formation: &[Performer],
        skills: &[Option<i64>],
        power: i32,
        seed: i32,
        ranks: Option<&[(i32, i64)]>,
    ) -> Result<(i32, Vec<full::GekisouRange>, u64), Error> {
        let deck: Vec<Performer> = (0..skills.len().max(formation.len()))
            .map(|k| Performer {
                live_skill: skills.get(k).copied().flatten().map(|id| (id, 1)),
                ..formation.get(k).cloned().unwrap_or_default()
            })
            .collect();
        let params = LiveParams { total_power: power, ..self.params };
        let mut play = play.clone();
        play.base_seed = seed;
        let Some(g) = &self.gekisou else {
            if ranks.is_some() {
                return Err(Error::Input("ranks without Gekisou".into()));
            }
            let mut lm = full::LiveModel::new(master, &deck, self.notes, self.events, params)?;
            let score = lm.run(&play)?;
            return Ok((score, Vec::new(), lm.converted_judgements()));
        };
        let mut lm = match ranks {
            None => full::LiveModel::new_gekisou(master, &deck, self.notes, self.events, params, &g.setup)?,
            Some(ranks) => {
                let mut lm =
                    full::LiveModel::new_gekisou_external(master, &deck, self.notes, self.events, params, &g.setup)?;
                for (i, &(rank, percent)) in ranks.iter().enumerate() {
                    lm.queue_gekisou_rank_confirmation(i, rank, percent)?;
                }
                lm
            }
        };
        let score = lm.run_timed(&play, &g.dt)?;
        Ok((score, lm.gekisou_ranges(), lm.converted_judgements()))
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

    /// Plays a check deck (its skills on `master`) at [`CHECK_POWER`] and predicts its score, `CHECK_POWER * (base +
    /// sum_k factor_k * weight(kind_k, k))`. The bound: one point per judged note and per range bonus (`floors`
    /// counts them) in each run, where the measured weights carry the floors of two runs, scaled by the power ratio
    /// and the factors; `slack` more points per unit of factor at [`POWER`]; the binary32 chain a few ulps.
    #[allow(clippy::too_many_arguments)]
    fn check(
        &self,
        kinds: &[Kind],
        master: &Master,
        deck: &[Option<(usize, i64)>],
        seed: i32,
        ranks: Option<&[(i32, i64)]>,
        base: f64,
        weight: impl Fn(usize, usize) -> f64,
        floors: f64,
        slack: f64,
    ) -> Result<Checked, Error> {
        self.check_with(kinds, master, &[], deck, seed, ranks, base, weight, floors, slack)
    }

    /// [`Live::check`] with a Gekisou formation.
    #[allow(clippy::too_many_arguments)]
    fn check_with(
        &self,
        kinds: &[Kind],
        master: &Master,
        formation: &[Performer],
        deck: &[Option<(usize, i64)>],
        seed: i32,
        ranks: Option<&[(i32, i64)]>,
        base: f64,
        weight: impl Fn(usize, usize) -> f64,
        floors: f64,
        slack: f64,
    ) -> Result<Checked, Error> {
        let (exact, _) = self.run_deck(&self.play, master, formation, &check_skills(deck), CHECK_POWER, seed, ranks)?;
        let scale = CHECK_POWER as f64 / POWER as f64;
        let mut per_power = base;
        let mut gain = 0f64;
        for (k, d) in deck.iter().enumerate() {
            if let Some((ki, value)) = *d {
                let x = kind_factor(kinds[ki].effect_type, value);
                per_power += x * weight(ki, k);
                gain += x.abs();
            }
        }
        let predicted = CHECK_POWER as f64 * per_power;
        let floors = floors * (1.0 + scale * (1.0 + 2.0 * gain));
        let bound = floors + slack * scale * gain + 4e-6 * predicted.abs();
        Ok(Checked { exact, predicted, bound })
    }

    /// The measurements of one seed with Gekisou on; `infos` are the chart's ranges, `linear` whether the ranks
    /// follow linearly on the chart, `reads_rank[kind]` whether a kind reads the confirmed rank.
    #[allow(clippy::too_many_arguments)]
    fn seed_stats(
        &self,
        kinds: &[Kind],
        infos: &[RangeInfo],
        linear: bool,
        reads_rank: &[bool],
        seed: i32,
        rng: &mut Rng,
        rank_rng: &mut Rng,
        judged: i32,
    ) -> Result<SeedStats, Error> {
        let none = vec![None; self.positions];
        let (score, gk) = self.run(self.master, &none, POWER, seed, None)?;
        let perfect = &self.gekisou.as_ref().ok_or_else(|| Error::Input("seed stats without Gekisou".into()))?.perfect;
        let (score_perfect, gk_perfect) = self.run_play(perfect, self.master, &none, POWER, seed, None)?;
        if gk_perfect.len() != gk.len() {
            return Err(Error::Game("the Perfect play has other ranges".into()));
        }
        let ranges: Vec<RangeResult> = gk
            .iter()
            .zip(&gk_perfect)
            .map(|(r, p)| RangeResult {
                range_score: r.end_score.wrapping_sub(r.start_score),
                rank_bonus: r.rank_bonus.unwrap_or(0),
                max_combo: r.max_combo,
                just_count: r.just_count,
                luck_points: r.luck_points,
                lot_results: r.lot_results,
                range_score_perfect: p.end_score.wrapping_sub(p.start_score),
            })
            .collect();
        let mut weights = vec![vec![0f64; self.positions]; kinds.len()];
        let mut range_weights = vec![vec![vec![0f64; ranges.len()]; self.positions]; kinds.len()];
        for (ki, kind) in kinds.iter().enumerate() {
            let unit = kind_factor(kind.effect_type, UNIT_VALUE);
            for k in 0..self.positions {
                let mut skills = none.clone();
                skills[k] = Some(KIND_SKILL_BASE - ki as i64);
                let (s, gk) = self.run(self.measure, &skills, POWER, seed, None)?;
                weights[ki][k] = (s as f64 - score as f64) / (POWER as f64 * unit);
                for ((w, r), r0) in range_weights[ki][k].iter_mut().zip(&gk).zip(&ranges) {
                    let rs = r.end_score.wrapping_sub(r.start_score);
                    *w = (rs as f64 - r0.range_score as f64) / (POWER as f64 * unit);
                }
            }
        }
        let range_weights: Option<Vec<Option<Vec<Vec<f64>>>>> =
            linear.then(|| range_weights.into_iter().zip(reads_rank).map(|(w, &r)| (!r).then_some(w)).collect());

        // the check deck: a random kind and master value at each position, at another power
        let usable: Vec<usize> = (0..kinds.len()).collect();
        let (deck, rows) = check_deck(kinds, &usable, self.positions, rng);
        let master = Self::master_with(self.master, &rows);
        let floors = judged as f64 + MAX_GEKISOU_FEVERS as f64;
        let c = self
            .check(kinds, &master, &deck, seed, None, score as f64 / POWER as f64, |ki, k| weights[ki][k], floors, 0.0)?
            .within(|| format!("seed {seed}"))?;
        let check = Check { deck, exact: c.exact, predicted: c.predicted, bound: c.bound };
        let mut stats =
            SeedStats { seed, score, ranges, weights, check, score_perfect, range_weights, rank_check: None };

        // the same deck at random ranks, through the explicit rank confirmations
        let Some(rw) = &stats.range_weights else { return Ok(stats) };
        if stats.ranges.is_empty() || stats.check.deck.iter().flatten().any(|&(ki, _)| rw[ki].is_none()) {
            return Ok(stats);
        }
        let ranks: Vec<i32> = infos.iter().map(|_| 1 + rank_rng.below(RANKS) as i32).collect();
        let confirmations: Vec<(i32, i64)> =
            ranks.iter().zip(infos).map(|(&r, info)| Ok((r, info.percent(r)?))).collect::<Result<_, Error>>()?;
        let base = stats.score_at_ranks(infos, &ranks)? as f64 / POWER as f64;
        let w = stats.weights_at_ranks(infos, &ranks)?.ok_or_else(|| Error::Game("no range weights".into()))?;
        // each range moves a weight by the difference of two floored bonuses from its rank 1 value: under 2 points
        let slack = 2.0 * infos.len() as f64;
        let deck = stats.check.deck.clone();
        let weight = |ki: usize, k: usize| w[ki].as_ref().map_or(f64::NAN, |w| w[k]);
        let c = self
            .check(kinds, &master, &deck, seed, Some(&confirmations), base, weight, floors, slack)?
            .within(|| format!("seed {seed} at ranks {ranks:?}"))?;
        stats.rank_check = Some(RankCheck { ranks, exact: c.exact, predicted: c.predicted, bound: c.bound });
        Ok(stats)
    }

    /// The measurements of one seed with Gekisou off.
    fn off_seed_stats(&self, kinds: &[Kind], seed: i32, rng: &mut Rng, judged: i32) -> Result<OffSeedStats, Error> {
        let none = vec![None; self.positions];
        let (score, _) = self.run(self.master, &none, POWER, seed, None)?;
        let mut weights: Vec<Option<Vec<f64>>> = Vec::with_capacity(kinds.len());
        'kinds: for (ki, kind) in kinds.iter().enumerate() {
            let unit = kind_factor(kind.effect_type, UNIT_VALUE);
            let mut w = Vec::with_capacity(self.positions);
            for k in 0..self.positions {
                let mut skills = none.clone();
                skills[k] = Some(KIND_SKILL_BASE - ki as i64);
                match self.run(self.measure, &skills, POWER, seed, None) {
                    Ok((s, _)) => w.push((s as f64 - score as f64) / (POWER as f64 * unit)),
                    // a condition that reads the Gekisou state: the kind cannot play without Gekisou
                    Err(Error::Unsupported(_)) => {
                        weights.push(None);
                        continue 'kinds;
                    }
                    Err(e) => return Err(e),
                }
            }
            weights.push(Some(w));
        }
        let usable: Vec<usize> = (0..kinds.len()).filter(|&ki| weights[ki].is_some()).collect();
        let (deck, rows) = check_deck(kinds, &usable, self.positions, rng);
        let master = Self::master_with(self.master, &rows);
        let weight = |ki: usize, k: usize| weights[ki].as_ref().map_or(f64::NAN, |w| w[k]);
        let c = self
            .check(kinds, &master, &deck, seed, None, score as f64 / POWER as f64, weight, judged as f64, 0.0)?
            .within(|| format!("Gekisou off, seed {seed}"))?;
        Ok(OffSeedStats {
            seed,
            score,
            weights,
            check: Check { deck, exact: c.exact, predicted: c.predicted, bound: c.bound },
        })
    }
}

/// The statistics of one chart for these kinds; `seeds` is the size of the seed set when a range is a luck range; the
/// aptitude for Gekisou skills with the default [`AptitudeOptions`].
pub fn chart_stats(master: &Master, chart: &DataChart, kinds: &[Kind], seeds: usize) -> Result<ChartStats, Error> {
    chart_stats_with(master, chart, kinds, &Options { seeds, ..Options::default() })
}

/// The statistics of one chart for these kinds with these options.
pub fn chart_stats_with(
    master: &Master,
    chart: &DataChart,
    kinds: &[Kind],
    options: &Options,
) -> Result<ChartStats, Error> {
    options.validate()?;
    let seeds = options.seeds;
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
        .map(|(i, &(start_ms, end_ms))| {
            let percents = factors.get(i).copied().unwrap_or([0; RANKS]);
            RangeInfo {
                index: i,
                mission: resolved.gekisou_missions[i.min(2)],
                start_ms,
                end_ms,
                rank_bonus_percent: percents[0],
                rank_bonus_percents: percents,
            }
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
        off_seeds: Vec::new(),
        unplayable: None,
        gekisou_aptitude: None,
    };
    let notes: Vec<LiveNote> = c
        .notes
        .iter()
        .zip(&chart.judgement_types)
        .map(|(n, &jt)| LiveNote {
            note_id: n.id,
            time_ms: n.time_ms,
            note_operate_type: n.note_type,
            judgement_type: jt,
        })
        .collect();
    let unit: Vec<(i64, &Kind, i64)> =
        kinds.iter().enumerate().map(|(i, k)| (KIND_SKILL_BASE - i as i64, k, UNIT_VALUE)).collect();
    let measure = Live::master_with(master, &unit);
    let params = LiveParams {
        skill_target_music_type: resolved.skill_target_music_type,
        total_power: POWER,
        music_level: level,
        converted_note_count: c.converted_note_count,
        music_length_ms,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };

    // Gekisou off: every chart, whatever its fevers
    let off = Live {
        master,
        measure: &measure,
        notes: &notes,
        events: &events,
        params,
        gekisou: None,
        play: JudgementStream::theoretical_best(&c).to_live_play()?,
        positions,
    };
    let mut off_rng = Rng(OFF_CHECK_SALT ^ chart.score_id as u64);
    out.off_seeds.push(off.off_seed_stats(kinds, OFF_SEED, &mut off_rng, judged)?);

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
    let mut perfect = stream.clone();
    for r in &mut perfect.judged {
        if r[2] == SIMULATE_JUST {
            r[2] = SIMULATE_PERFECT;
        }
    }
    let live = Live {
        master,
        measure: &measure,
        notes: &notes,
        events: &events,
        params,
        gekisou: Some(Gekisou { setup, dt: stream.delta_times()?, perfect: perfect.to_live_play()? }),
        play: stream.to_live_play()?,
        positions,
    };
    let setup = &live.gekisou.as_ref().expect("Gekisou on").setup;
    let luck = setup.missions.iter().take(setup.fevers.len()).any(|&m| m == MISSION_LUCK);
    let seed_list = if luck { published_seeds(seeds.max(1)) } else { vec![0] };
    let linear = !bonus_inside_a_range(&chart.fevers);
    let reads_rank: Vec<bool> = kinds.iter().map(|k| k.reads_rank(master)).collect();
    let mut rng = Rng(CHECK_SALT ^ chart.score_id as u64);
    let mut rank_rng = Rng(RANK_CHECK_SALT ^ chart.score_id as u64);
    for seed in seed_list {
        let s = live.seed_stats(kinds, &out.ranges, linear, &reads_rank, seed, &mut rng, &mut rank_rng, judged)?;
        out.seeds.push(s);
    }

    // the aptitude for Gekisou skills
    if let Some(a) = &options.aptitude {
        let shapes = shapes(master);
        if !shapes.is_empty() && !out.ranges.is_empty() {
            let inputs = aptitude::Inputs { seeds: &out.seeds, linear, score_id: chart.score_id, judged, options: a };
            out.gekisou_aptitude = Some(aptitude::chart_aptitude(&live, kinds, &out.ranges, &shapes, &inputs)?);
        }
    }
    Ok(out)
}

/// The statistics of every chart of a deck data file (score id order) as the `ournotes-deck.chart-stats/2` document;
/// `seeds` is the size of the seed set of charts with a luck range (default [`GEKISOU_SEEDS`]).
pub fn document(data: &DeckData, seeds: Option<usize>) -> Result<serde_json::Value, Error> {
    document_with(data, &Options { seeds: seeds.unwrap_or(GEKISOU_SEEDS), ..Options::default() })
}

/// [`document`] with these options.
pub fn document_with(data: &DeckData, options: &Options) -> Result<serde_json::Value, Error> {
    options.validate()?;
    let kinds = kinds(&data.master);
    let mut charts = Vec::with_capacity(data.charts.len());
    for c in &data.charts {
        charts.push(chart_stats_with(&data.master, c, &kinds, options).map_err(|e| match e {
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
            "ranks": "Gekisou on at rank r_i (1..5) in range i: score becomes score - sum_i rankBonus_i + sum_i \
                      trunc(rangeScore_i * rankBonusPercents_i[r_i - 1] / 100) (exact) and weights[kind][k] becomes \
                      weights[kind][k] + sum_i (rankBonusPercents_i[r_i - 1] - rankBonusPercents_i[0]) / 100 * \
                      rangeWeights[kind][k][i]; rankCheck plays the check deck at random ranks",
            "perfect": "scorePerfect and rangeScorePerfect: the no-skill score (rank 1 bonuses included) and range \
                        scores on the same play with every Just judged Perfect",
            "off": "offSeeds: Gekisou off (no Just, luck, Gekisou combo or rank bonus), theoretical best play (every \
                    note Perfect at its time), seed 0, every chart; the same formula on its score and weights, a \
                    kind whose conditions read the Gekisou state has null weights; checked",
            "gekisouAptitude": aptitude::MODEL,
        },
        "gekisouAptitude": options.aptitude.as_ref().map(|a| aptitude_header(&data.master, &kinds, a)),
        "kinds": kinds,
        "charts": charts,
    }))
}

#[cfg(test)]
mod tests {
    use super::bonus_inside_a_range;

    #[test]
    fn a_bonus_inside_another_range_is_found() {
        // apart, and a start in the frame after the end: no bonus inside another range
        assert!(!bonus_inside_a_range(&[(8000, 16000), (24000, 32000), (42000, 50000)]));
        assert!(!bonus_inside_a_range(&[(8000, 16000), (16001, 24000)]));
        // a start in the frame of the end: the bonus is at both ends of the later range
        assert!(!bonus_inside_a_range(&[(8000, 16000), (15990, 24000)]));
        // overlapping ranges: the first bonus falls inside the second range
        assert!(bonus_inside_a_range(&[(8000, 16000), (15000, 24000)]));
        assert!(bonus_inside_a_range(&[(15000, 24000), (8000, 16000)]));
    }
}
