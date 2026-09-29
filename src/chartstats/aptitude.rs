//! The aptitude of a chart for Gekisou skills: what each Gekisou skill shape adds to the chart's score alone, measured
//! on the whole-live simulation.
//!
//! A shape is a member card's Gekisou skill at its highest level, or a snap's Gekisou support skill at the level of
//! the snap's highest rank, with its score-relevant effect parameters; skills with the same parameters are one shape
//! ([`shapes`]). A support skill's member target condition (5000, on its own member: the band condition) is measured
//! both ways, the member a target (`bandMatch` true) or not (false), so support skills that differ only in the band
//! are one shape.
//!
//! Each shape of the chart's missions is played alone on the chart's no-skill play (Gekisou on, rank 1, the
//! theoretical best play; [`super::SeedStats`]) and on its Perfect play: a member skill on one performer; a support
//! skill on one performer whose Gekisou skill is a synthetic one without effects (a support skill acts only with a
//! member Gekisou skill), of the support skill's mission. A Gekisou (support) skill acts only while a range of its
//! mission is concerned, so a shape of another mission adds nothing and is not measured. The increments (`with -
//! without` on the same seed) are exact per seed; a shape whose increments are the same on
//! [`DETERMINISTIC_TEST`] seeds is deterministic and is given on one seed, any other on seed batches
//! ([`BATCHES`]) until the standard error of its score increment is at most the larger of [`RELATIVE`] of the
//! increment and [`BASELINE`] of the no-skill score. The cross term, the change of the plain kind's weights, uses
//! the first `cross_seeds` of them.
//!
//! The increments of several shapes do not add up: the Gekisou combo factor saturates, the luck rush support skills
//! and the luck gauge skills multiply, and Just count additions reach the support skills triggered per Just count.

use serde::Serialize;

use super::{
    Checked, KIND_SKILL_BASE, Kind, Live, MAX_GEKISOU_FEVERS, POWER, RangeInfo, Rng, SeedStats, UNIT_VALUE, check_deck,
    kind_factor,
};
use crate::error::Error;
use crate::live::full::{GekisouRange, LiveModel, Performer};
use crate::live::score::get_frame;
use crate::live::seeds::published_seeds;
use crate::master::{GekisouSkillEffectRow, Master, SkillRow};

/// Seeds a shape's increments must agree on to be deterministic (the first published seeds).
pub const DETERMINISTIC_TEST: usize = 4;
/// Seed batches of a shape that is not deterministic: the first seeds of the published set.
pub const BATCHES: [usize; 6] = [32, 64, 128, 256, 512, 1024];
/// The standard error target relative to the score increment.
pub const RELATIVE: f64 = 0.01;
/// The standard error target relative to the no-skill score.
pub const BASELINE: f64 = 0.001;
/// Default most seeds of a shape.
pub const MAX_SEEDS: usize = 1024;
/// Default most seeds of the cross term.
pub const CROSS_SEEDS: usize = 64;
/// Skill condition type of a member target: a skill's condition on its own member (the band condition).
const CONDITION_MEMBER_TARGET: i64 = 5000;
/// Gekisou skill id of the synthetic host of mission `m`: `HOST_SKILL_BASE - m`.
const HOST_SKILL_BASE: i64 = -1000;
/// Start states of the generators of the check decks and ranks, each xored with the score id and the variant.
const APT_CHECK_SALT: u64 = 0x6170_745f_6368_6563;
const APT_RANK_SALT: u64 = 0x6170_745f_7261_6e6b;
/// How the shapes are measured, for the document.
pub const HOST: &str = "each shape alone on one performer, the other positions empty: a member skill as the \
    performer's Gekisou skill; a support skill paired with a synthetic Gekisou skill without effects (id -1000 - \
    mission, the support skill's mission) on the same performer, in every chart; a band condition (5000) measured \
    with the performer a target of it (the band, character, card type or tag of its first target) and with none \
    of those (bandMatch false)";
/// The model, for the document.
pub const MODEL: &str = "charts[].gekisouAptitude: every Gekisou skill shape of the chart's missions (gekisouAptitude.\
    shapes) played alone on the whole-live simulation, Gekisou on, rank 1, the theoretical best play (score) and \
    its Perfect play (scorePerfect); increments with minus without on the same seed as [mean, standard error]; a \
    shape of another mission adds nothing (its skills trigger only while a range of their mission is concerned); \
    deterministic shapes on one seed, the others on seed batches until the standard error is at most max(1% of the \
    increment, 0.1% of the no-skill score); tail = score - sum_j (rangeScore_j + rankBonus_j); at ranks r_j the \
    increment is tail + sum_j rangeScore_j * (1 + p_j(r_j) / 100) up to one point per range; weights and \
    rangeWeights: the change of the plain kind's weights (cross term) on the first crossSeeds seeds; one check per \
    variant at random ranks and a random plain deck; increments of several shapes do not add up";

/// Options of the aptitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AptitudeOptions {
    /// Most seeds of a shape that is not deterministic.
    pub max_seeds: usize,
    /// Most seeds of the cross term.
    pub cross_seeds: usize,
}

impl Default for AptitudeOptions {
    fn default() -> AptitudeOptions {
        AptitudeOptions { max_seeds: MAX_SEEDS, cross_seeds: CROSS_SEEDS }
    }
}

/// A skill condition of a shape's effect.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    #[serde(rename = "type")]
    pub condition_type: i64,
    pub values: Vec<i64>,
    pub positive: bool,
    /// `None` for a member target condition (5000): the band condition, measured both ways.
    pub target_ids: Option<Vec<i64>>,
}

/// The cumulative condition of a shape's effect.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cumulative {
    #[serde(rename = "type")]
    pub cumulative_type: i64,
    pub values: Vec<i64>,
    pub target_ids: Vec<i64>,
    pub max_cumulative_count: i64,
}

/// An effect row of a shape; condition groups as their condition sets, each a list of conditions.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub effect_type: i64,
    pub trigger_type: i64,
    pub activation_time_second: f32,
    pub effect_value: i64,
    pub max_effect_value: i64,
    pub effect_limit_count: i64,
    pub effect_execute_limit_count: i64,
    pub skill_target_ids: Vec<i64>,
    pub trigger: Vec<Vec<Condition>>,
    pub condition: Vec<Vec<Condition>>,
    pub release: Vec<Vec<Condition>>,
    pub reset: Vec<Vec<Condition>>,
    pub cumulative: Option<Cumulative>,
}

/// A skill and level of a shape.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeSkill {
    pub id: i64,
    pub level: i64,
    /// The targets of its member target conditions (5000), ascending; `None` without one.
    pub member_target_ids: Option<Vec<i64>>,
    /// The bands of those targets, ascending; `None` without a member target condition.
    pub band_ids: Option<Vec<i64>>,
}

/// A Gekisou skill shape.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub id: usize,
    /// `"member"` (a member card's Gekisou skill) or `"support"` (a snap's Gekisou support skill).
    pub source: &'static str,
    pub mission: i64,
    /// Whether an effect has a member target condition (5000).
    pub band_condition: bool,
    pub effects: Vec<Effect>,
    pub skills: Vec<ShapeSkill>,
}

/// The seed rule of the aptitude, for the document.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedRule {
    pub deterministic_test: usize,
    pub batches: Vec<usize>,
    pub relative: f64,
    pub baseline: f64,
    pub cross_seeds: usize,
}

/// The document header of the aptitude (`gekisouAptitude`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AptitudeHeader {
    pub plain_kind: Option<usize>,
    pub host: &'static str,
    pub seed_rule: SeedRule,
    pub shapes: Vec<Shape>,
}

/// The factors of a range that shape the increments, from the chart's no-skill play.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeFactors {
    /// Judged notes in the range's score frames (after the Start frame to the End frame).
    pub judged_notes: i32,
    /// Of them, judged Just.
    pub just_notes: i32,
    /// Of them, judged Perfect in a Just-count range (a judgement type without a Just row); 0 elsewhere.
    pub perfect_notes: i32,
    /// Notes judged after the End frame to the Complete frame (the tail).
    pub tail_notes: i32,
    /// Notes judged before the Start frame (the combo entering the range).
    pub combo_at_start: i32,
    /// Lotteries drawn without skills (the sum of the lottery results) over the chart's seeds, `[mean, se]`.
    pub lotteries: [f64; 2],
}

/// Increments of a range, each `[mean, se]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeDelta {
    pub range_score: [f64; 2],
    pub rank_bonus: [f64; 2],
    pub range_score_perfect: [f64; 2],
    pub max_combo: [f64; 2],
    pub just_count: [f64; 2],
    pub luck_points: [f64; 2],
}

/// The check of a variant: the shape with a random plain deck at random ranks at the check power.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantCheck {
    pub seed: i32,
    pub ranks: Vec<i32>,
    pub deck: Vec<Option<(usize, i64)>>,
    pub exact: i32,
    pub predicted: f64,
    pub bound: f64,
}

/// A shape (with a band condition result) on a chart: its increments, each `[mean, se]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub shape: usize,
    pub band_match: Option<bool>,
    pub deterministic: bool,
    pub seeds: usize,
    pub se_target_met: bool,
    pub cross_seeds: usize,
    pub score: [f64; 2],
    pub score_perfect: [f64; 2],
    pub tail: [f64; 2],
    pub tail_perfect: [f64; 2],
    pub converted: [f64; 2],
    pub ranges: Vec<RangeDelta>,
    /// The change of the plain kind's weight per position; `None` without a plain kind.
    pub weights: Option<Vec<[f64; 2]>>,
    /// The change of its range weights per position and range; `None` without range weights or a plain kind.
    pub range_weights: Option<Vec<Vec<[f64; 2]>>>,
    pub check: VariantCheck,
}

/// A chart's aptitude for Gekisou skills.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartAptitude {
    pub factors: Vec<RangeFactors>,
    pub variants: Vec<Variant>,
}

/// The plain kind: effect type 2000 on the whole deck for 5 s without targets, conditions or limits (the page's
/// `plainKind`).
pub fn plain_kind(kinds: &[Kind]) -> Option<usize> {
    kinds
        .iter()
        .find(|k| {
            k.effect_type == 2000
                && k.skill_target_ids.is_empty()
                && k.skill_condition_group == 0
                && k.skill_release_condition_group == 0
                && k.effect_limit_count == 0
                && k.effect_execute_limit_count == 0
                && k.duration_ms == 5000
        })
        .map(|k| k.id)
}

/// The header of the aptitude.
pub fn aptitude_header(master: &Master, kinds: &[Kind], options: &AptitudeOptions) -> AptitudeHeader {
    let mut batches: Vec<usize> = BATCHES.iter().copied().filter(|&b| b <= options.max_seeds).collect();
    if batches.last().is_none_or(|&b| b < options.max_seeds) {
        batches.push(options.max_seeds);
    }
    AptitudeHeader {
        plain_kind: plain_kind(kinds),
        host: HOST,
        seed_rule: SeedRule {
            deterministic_test: DETERMINISTIC_TEST,
            batches,
            relative: RELATIVE,
            baseline: BASELINE,
            cross_seeds: options.cross_seeds,
        },
        shapes: shapes(master),
    }
}

/// A condition group as its condition sets (by id), each a list of conditions.
fn group(master: &Master, g: i64) -> Vec<Vec<Condition>> {
    if g == 0 {
        return Vec::new();
    }
    let mut sets: Vec<_> = master.skill_condition_sets.iter().filter(|s| s.group == g).collect();
    sets.sort_by_key(|s| s.id);
    sets.iter()
        .map(|s| {
            s.condition_ids
                .iter()
                .filter_map(|&c| master.skill_condition(c))
                .map(|c| Condition {
                    condition_type: c.condition_type,
                    values: c.condition_values.clone(),
                    positive: c.is_positive,
                    target_ids: (c.condition_type != CONDITION_MEMBER_TARGET).then(|| c.condition_target_ids.clone()),
                })
                .collect()
        })
        .collect()
}

fn effect(master: &Master, r: &GekisouSkillEffectRow) -> Effect {
    let cumulative = if r.skill_cumulative_condition_id != 0 {
        master.cumulative_condition(r.skill_cumulative_condition_id).map(|c| Cumulative {
            cumulative_type: c.condition_type,
            values: c.condition_values.clone(),
            target_ids: c.condition_target_ids.clone(),
            max_cumulative_count: c.max_cumulative_count,
        })
    } else {
        None
    };
    Effect {
        effect_type: r.skill_effect_type,
        trigger_type: r.skill_trigger_type,
        activation_time_second: r.activation_time_second,
        effect_value: r.effect_value,
        max_effect_value: r.max_effect_value,
        effect_limit_count: r.effect_limit_count,
        effect_execute_limit_count: r.effect_execute_limit_count,
        skill_target_ids: r.skill_target_ids.clone(),
        trigger: group(master, r.skill_trigger_condition_group),
        condition: group(master, r.skill_condition_group),
        release: group(master, r.skill_release_condition_group),
        reset: group(master, r.effect_execute_limit_reset_condition_group),
        cumulative,
    }
}

/// The groups of an effect row.
fn groups(r: &GekisouSkillEffectRow) -> [i64; 4] {
    [
        r.skill_trigger_condition_group,
        r.skill_condition_group,
        r.skill_release_condition_group,
        r.effect_execute_limit_reset_condition_group,
    ]
}

/// The member target conditions (5000) of effect rows: `(positive, target ids)`.
fn member_conditions(master: &Master, rows: &[&GekisouSkillEffectRow]) -> Vec<(bool, Vec<i64>)> {
    let mut out = Vec::new();
    for r in rows {
        for g in groups(r) {
            if g == 0 {
                continue;
            }
            for s in master.skill_condition_sets.iter().filter(|s| s.group == g) {
                for c in s.condition_ids.iter().filter_map(|&c| master.skill_condition(c)) {
                    if c.condition_type == CONDITION_MEMBER_TARGET {
                        out.push((c.is_positive, c.condition_target_ids.clone()));
                    }
                }
            }
        }
    }
    out
}

/// The effect rows of a skill at a level, by row id.
fn skill_rows<'m>(master: &'m Master, source: &str, id: i64, level: i64) -> Vec<&'m GekisouSkillEffectRow> {
    let table = if source == "member" { &master.gekisou_skill_effects } else { &master.gekisou_support_skill_effects };
    let mut rows: Vec<&GekisouSkillEffectRow> = table.iter().filter(|r| r.skill_id == id && r.level == level).collect();
    rows.sort_by_key(|r| r.id);
    rows
}

/// The Gekisou skill shapes of a master: member cards' Gekisou skills at their highest level and snaps' first Gekisou
/// support skills at the level of their highest rank, by skill id, grouped by source, mission and effects.
pub fn shapes(master: &Master) -> Vec<Shape> {
    let mut pairs: Vec<(&'static str, i64, i64)> = Vec::new();
    for c in &master.member_cards {
        let id = c.gekisou_skill_id;
        let level = master.gekisou_skill_effects.iter().filter(|r| id != 0 && r.skill_id == id).map(|r| r.level).max();
        if let Some(level) = level
            && !pairs.contains(&("member", id, level))
        {
            pairs.push(("member", id, level));
        }
    }
    for s in &master.support_cards {
        let id = s.gekisou_support_skill_id_01;
        let level = master
            .support_card_ranks
            .iter()
            .filter(|r| r.group == s.rank_group)
            .max_by_key(|r| r.rank)
            .map_or(0, |r| r.gekisou_support_skill_01_level);
        if id != 0 && level > 0 && !pairs.contains(&("support", id, level)) {
            pairs.push(("support", id, level));
        }
    }
    pairs.sort_by_key(|&(source, id, level)| (source != "member", id, level));
    let mut out: Vec<Shape> = Vec::new();
    for (source, id, level) in pairs {
        let row: Option<&SkillRow> =
            if source == "member" { master.gekisou_skill(id) } else { master.gekisou_support_skill(id) };
        let Some(row) = row else { continue };
        let rows = skill_rows(master, source, id, level);
        if rows.is_empty() {
            continue;
        }
        let effects: Vec<Effect> = rows.iter().map(|r| effect(master, r)).collect();
        let conditions = member_conditions(master, &rows);
        let band_condition = !conditions.is_empty();
        let mut targets: Vec<i64> = conditions.iter().flat_map(|c| c.1.iter().copied()).collect();
        targets.sort_unstable();
        targets.dedup();
        let mut bands: Vec<i64> =
            targets.iter().filter_map(|&t| master.skill_target(t)).map(|t| t.band_id).filter(|&b| b > 0).collect();
        bands.sort_unstable();
        bands.dedup();
        let skill = ShapeSkill {
            id,
            level,
            member_target_ids: band_condition.then(|| targets.clone()),
            band_ids: band_condition.then_some(bands),
        };
        let mission = row.gekisou_mission_type;
        match out.iter_mut().find(|s| s.source == source && s.mission == mission && s.effects == effects) {
            Some(s) => s.skills.push(skill),
            None => {
                out.push(Shape { id: out.len(), source, mission, band_condition, effects, skills: vec![skill] });
            }
        }
    }
    out
}

/// A master with the synthetic host Gekisou skills (missions 1 to 4, no effects).
fn with_hosts(master: &Master) -> Result<Master, Error> {
    let mut m = master.clone();
    for mission in 1..=4 {
        m.gekisou_skills.push(SkillRow {
            id: HOST_SKILL_BASE - mission,
            skill_categories: Vec::new(),
            gekisou_mission_type: mission,
        });
    }
    m.reindex()?;
    Ok(m)
}

/// Whether a performer is a target of member target conditions.
fn is_target(master: &Master, conditions: &[(bool, Vec<i64>)], p: &Performer) -> bool {
    conditions
        .iter()
        .any(|(_, targets)| targets.iter().filter_map(|&t| master.skill_target(t)).any(|t| p.matches_skill_target(t)))
}

/// The performer of a variant.
fn performer(master: &Master, shape: &Shape, band: Option<bool>) -> Result<Performer, Error> {
    let s = &shape.skills[0];
    let mut p = Performer { gekisou_mission_type: shape.mission, ..Default::default() };
    if shape.source == "member" {
        p.gekisou_skill = Some((s.id, s.level));
    } else {
        p.gekisou_skill = Some((HOST_SKILL_BASE - shape.mission, 1));
        p.gekisou_support_skills = vec![(s.id, s.level)];
    }
    if let Some(want) = band {
        let rows = skill_rows(master, shape.source, s.id, s.level);
        let conditions = member_conditions(master, &rows);
        if want {
            let t =
                conditions.iter().flat_map(|c| c.1.iter()).find_map(|&t| master.skill_target(t)).ok_or_else(|| {
                    Error::Master(format!("shape {}: a member target condition without targets", shape.id))
                })?;
            p.band_id = t.band_id;
            p.character_id = t.character_id;
            p.card_type = t.card_type;
            if t.tag_id > 0 {
                p.tag_ids = vec![t.tag_id];
            }
            p.live_skill_categories = t.live_skill_categories.clone();
            p.gekisou_skill_categories = t.gekisou_skill_categories.clone();
        }
        if is_target(master, &conditions, &p) != want {
            return Err(Error::Game(format!("shape {}: no performer with band condition {want}", shape.id)));
        }
    }
    Ok(p)
}

/// The numbers of one live.
#[derive(Clone, Debug)]
struct Played {
    score: i32,
    ranges: Vec<GekisouRange>,
    converted: u64,
}

/// A seed's no-skill numbers: the Just and the Perfect play, and the plain kind at each position.
#[derive(Clone, Debug)]
struct BaseRun {
    just: Played,
    perfect: Played,
    /// `(score, range scores)` with the plain kind at position k.
    cross: Option<Vec<(i32, Vec<i32>)>>,
}

/// A seed's increments.
#[derive(Clone, Debug, PartialEq)]
struct Sample {
    seed: i32,
    base_score: f64,
    values: Vec<f64>,
    /// `(weights per position, range weights per position and range)` changes.
    cross: Option<(Vec<f64>, Vec<Vec<f64>>)>,
}

/// The inputs of a chart's aptitude besides the live.
pub(super) struct Inputs<'a> {
    pub seeds: &'a [SeedStats],
    pub linear: bool,
    pub score_id: i64,
    pub judged: i32,
    pub options: &'a AptitudeOptions,
}

fn range_score(r: &GekisouRange) -> i32 {
    r.end_score.wrapping_sub(r.start_score)
}

/// The measurement of a chart.
struct Measure<'a, 'm> {
    live: &'a Live<'m>,
    master: Master,
    measure: Master,
    plain: Option<usize>,
    unit: f64,
    ranges: usize,
    base: std::collections::HashMap<i32, BaseRun>,
}

impl Measure<'_, '_> {
    fn play(
        &self,
        perfect: bool,
        master: &Master,
        p: Option<&Performer>,
        skills: &[Option<i64>],
        seed: i32,
    ) -> Result<Played, Error> {
        let g = self.live.gekisou.as_ref().ok_or_else(|| Error::Input("aptitude without Gekisou".into()))?;
        let play = if perfect { &g.perfect } else { &self.live.play };
        let formation: Vec<Performer> = p.into_iter().cloned().collect();
        let (score, ranges, converted) = self.live.run_counted(play, master, &formation, skills, POWER, seed, None)?;
        Ok(Played { score, ranges, converted })
    }

    /// The no-skill numbers of a seed, with the plain kind's runs when `cross`.
    fn base(&mut self, seed: i32, cross: bool) -> Result<&BaseRun, Error> {
        let none = vec![None; self.live.positions];
        if !self.base.contains_key(&seed) {
            let just = self.play(false, &self.master, None, &none, seed)?;
            let perfect = self.play(true, &self.master, None, &none, seed)?;
            self.base.insert(seed, BaseRun { just, perfect, cross: None });
        }
        if cross && self.plain.is_some() && self.base[&seed].cross.is_none() {
            let c = self.cross_runs(None, seed)?;
            self.base.get_mut(&seed).expect("base").cross = Some(c);
        }
        Ok(&self.base[&seed])
    }

    /// `(score, range scores)` with the plain kind at each position.
    fn cross_runs(&self, p: Option<&Performer>, seed: i32) -> Result<Vec<(i32, Vec<i32>)>, Error> {
        let Some(plain) = self.plain else { return Ok(Vec::new()) };
        let mut out = Vec::with_capacity(self.live.positions);
        for k in 0..self.live.positions {
            let mut skills = vec![None; self.live.positions];
            skills[k] = Some(KIND_SKILL_BASE - plain as i64);
            let r = self.play(false, &self.measure, p, &skills, seed)?;
            out.push((r.score, r.ranges.iter().map(range_score).collect()));
        }
        Ok(out)
    }

    /// A seed's increments of a performer: score, score on the Perfect play, tail, Perfect tail, conversions, then
    /// per range the range score, rank bonus, Perfect range score, max combo, Just count and luck points.
    fn sample(&mut self, p: &Performer, seed: i32, cross: bool) -> Result<Sample, Error> {
        let none = vec![None; self.live.positions];
        let just = self.play(false, &self.master, Some(p), &none, seed)?;
        let perfect = self.play(true, &self.master, Some(p), &none, seed)?;
        let c = if cross && self.plain.is_some() { Some(self.cross_runs(Some(p), seed)?) } else { None };
        let ranges = self.ranges;
        let unit = self.unit;
        let b = self.base(seed, cross)?.clone();
        if just.ranges.len() != ranges || perfect.ranges.len() != ranges {
            return Err(Error::Game("the shape's plays have other ranges".into()));
        }
        let tail = |x: &Played, y: &Played| -> f64 {
            let mut t = (x.score as i64 - y.score as i64) as f64;
            for (a, b) in x.ranges.iter().zip(&y.ranges) {
                t -= (range_score(a) as i64 - range_score(b) as i64) as f64;
                t -= (a.rank_bonus.unwrap_or(0) as i64 - b.rank_bonus.unwrap_or(0) as i64) as f64;
            }
            t
        };
        let mut values = vec![
            (just.score as i64 - b.just.score as i64) as f64,
            (perfect.score as i64 - b.perfect.score as i64) as f64,
            tail(&just, &b.just),
            tail(&perfect, &b.perfect),
            just.converted as f64 - b.just.converted as f64,
        ];
        for j in 0..ranges {
            let (x, y) = (&just.ranges[j], &b.just.ranges[j]);
            let (xp, yp) = (&perfect.ranges[j], &b.perfect.ranges[j]);
            values.push((range_score(x) as i64 - range_score(y) as i64) as f64);
            values.push((x.rank_bonus.unwrap_or(0) as i64 - y.rank_bonus.unwrap_or(0) as i64) as f64);
            values.push((range_score(xp) as i64 - range_score(yp) as i64) as f64);
            values.push((x.max_combo - y.max_combo) as f64);
            values.push((x.just_count - y.just_count) as f64);
            values.push((x.luck_points - y.luck_points) as f64);
        }
        let cross = match (c, &b.cross) {
            (Some(c), Some(bc)) => {
                let mut w = Vec::with_capacity(c.len());
                let mut rw = Vec::with_capacity(c.len());
                for ((s, rs), (s0, rs0)) in c.iter().zip(bc) {
                    let with = (*s as f64 - just.score as f64) / (POWER as f64 * unit);
                    let without = (*s0 as f64 - b.just.score as f64) / (POWER as f64 * unit);
                    w.push(with - without);
                    let per_range = (0..ranges)
                        .map(|j| {
                            let with = (rs[j] as f64 - range_score(&just.ranges[j]) as f64) / (POWER as f64 * unit);
                            let without =
                                (rs0[j] as f64 - range_score(&b.just.ranges[j]) as f64) / (POWER as f64 * unit);
                            with - without
                        })
                        .collect();
                    rw.push(per_range);
                }
                Some((w, rw))
            }
            _ => None,
        };
        Ok(Sample { seed, base_score: b.just.score as f64, values, cross })
    }
}

/// `[mean, standard error]` of values.
fn mean_se(x: impl Iterator<Item = f64> + Clone) -> [f64; 2] {
    let n = x.clone().count();
    if n == 0 {
        return [0.0, 0.0];
    }
    let mean = x.clone().sum::<f64>() / n as f64;
    if n == 1 {
        return [mean, 0.0];
    }
    let var = x.map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1) as f64;
    [mean, (var / n as f64).sqrt()]
}

/// Rounded for the output: points to 1e-3.
fn points(x: [f64; 2]) -> [f64; 2] {
    [(x[0] * 1000.0).round() / 1000.0, (x[1] * 1000.0).round() / 1000.0]
}

/// The range factors of a chart's play.
fn factors(live: &Live<'_>, infos: &[RangeInfo], seeds: &[SeedStats]) -> Result<Vec<RangeFactors>, Error> {
    let g = live.gekisou.as_ref().ok_or_else(|| Error::Input("aptitude without Gekisou".into()))?;
    let mut lm = LiveModel::new_gekisou(live.master, &[], live.notes, &[], live.params, &g.setup)?;
    let frames = lm.record_range_frames(&live.play, &g.dt)?;
    if frames.len() != infos.len() {
        return Err(Error::Game("the play has other ranges".into()));
    }
    let time: std::collections::HashMap<i32, i32> = live.notes.iter().map(|n| (n.note_id, n.time_ms)).collect();
    let mut out = Vec::with_capacity(infos.len());
    for (j, (info, rf)) in infos.iter().zip(&frames).enumerate() {
        let (fs, fe) = (get_frame(info.start_ms), get_frame(info.end_ms));
        let (mut judged, mut just, mut perfect, mut tail, mut before) = (0, 0, 0, 0, 0);
        for (i, f) in live.play.frames.iter().enumerate() {
            for n in &f.judged {
                let t = *time.get(&n.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", n.note_id)))?;
                let tf = get_frame(t);
                if fs < tf && tf <= fe {
                    judged += 1;
                    if n.judgement == 6 {
                        just += 1;
                    } else if n.judgement == 5 && info.mission == 3 {
                        perfect += 1;
                    }
                }
                if rf.end < i && i <= rf.complete {
                    tail += 1;
                }
                if i < rf.start {
                    before += 1;
                }
            }
        }
        let lotteries = mean_se(seeds.iter().map(|s| s.ranges[j].lot_results.iter().sum::<i32>() as f64));
        out.push(RangeFactors {
            judged_notes: judged,
            just_notes: just,
            perfect_notes: perfect,
            tail_notes: tail,
            combo_at_start: before,
            lotteries,
        });
    }
    Ok(out)
}

/// The aptitude of a chart for Gekisou skills.
pub(super) fn chart_aptitude(
    live: &Live<'_>,
    kinds: &[Kind],
    infos: &[RangeInfo],
    shapes: &[Shape],
    inp: &Inputs<'_>,
) -> Result<ChartAptitude, Error> {
    let plain = plain_kind(kinds);
    let mut m = Measure {
        live,
        master: with_hosts(live.master)?,
        measure: with_hosts(live.measure)?,
        plain,
        unit: plain.map_or(1.0, |p| kind_factor(kinds[p].effect_type, UNIT_VALUE)),
        ranges: infos.len(),
        base: std::collections::HashMap::new(),
    };
    let missions: Vec<i64> = infos.iter().map(|r| r.mission).collect();
    let first_seed = inp.seeds.first().map_or(0, |s| s.seed);
    let test_seeds = published_seeds(DETERMINISTIC_TEST);
    let mut batches: Vec<usize> = BATCHES.iter().copied().filter(|&b| b <= inp.options.max_seeds).collect();
    if batches.last().is_none_or(|&b| b < inp.options.max_seeds) {
        batches.push(inp.options.max_seeds);
    }
    let mut variants = Vec::new();
    for shape in shapes.iter().filter(|s| s.mission == 4 || missions.contains(&s.mission)) {
        let bands: Vec<Option<bool>> = if shape.band_condition { vec![Some(true), Some(false)] } else { vec![None] };
        for band in bands {
            let p = performer(&m.master, shape, band)?;
            // deterministic when the increments agree on the test seeds
            let mut test = Vec::with_capacity(test_seeds.len());
            for &s in &test_seeds {
                test.push(m.sample(&p, s, false)?.values);
            }
            let deterministic = test.windows(2).all(|w| w[0] == w[1]);
            let mut samples: Vec<Sample> = Vec::new();
            let mut met = true;
            if deterministic {
                samples.push(m.sample(&p, first_seed, true)?);
            } else {
                met = false;
                for &n in &batches {
                    let seeds = published_seeds(n);
                    for (i, &s) in seeds.iter().enumerate().skip(samples.len()) {
                        samples.push(m.sample(&p, s, i < inp.options.cross_seeds)?);
                    }
                    let [mean, se] = mean_se(samples.iter().map(|s| s.values[0]));
                    let base = samples.iter().map(|s| s.base_score).sum::<f64>() / samples.len() as f64;
                    if se <= (RELATIVE * mean.abs()).max(BASELINE * base) {
                        met = true;
                        break;
                    }
                }
            }
            let crossed: Vec<&Sample> = samples.iter().filter(|s| s.cross.is_some()).collect();
            let at = |i: usize| points(mean_se(samples.iter().map(move |s| s.values[i])));
            let ranges = (0..infos.len())
                .map(|j| {
                    let o = 5 + 6 * j;
                    RangeDelta {
                        range_score: at(o),
                        rank_bonus: at(o + 1),
                        range_score_perfect: at(o + 2),
                        max_combo: at(o + 3),
                        just_count: at(o + 4),
                        luck_points: at(o + 5),
                    }
                })
                .collect();
            let weights = plain.map(|_| {
                (0..live.positions)
                    .map(|k| mean_se(crossed.iter().map(|s| s.cross.as_ref().expect("cross").0[k])))
                    .collect()
            });
            let range_weights = (plain.is_some() && inp.linear).then(|| {
                (0..live.positions)
                    .map(|k| {
                        (0..infos.len())
                            .map(|j| mean_se(crossed.iter().map(|s| s.cross.as_ref().expect("cross").1[k][j])))
                            .collect()
                    })
                    .collect()
            });
            let salt = inp.score_id as u64 ^ ((shape.id as u64) << 32) ^ (band.map_or(0, |b| 1 + b as u64) << 48);
            let check = check(&mut m, kinds, infos, inp, &p, &samples[0], salt)?;
            variants.push(Variant {
                shape: shape.id,
                band_match: band,
                deterministic,
                seeds: samples.len(),
                se_target_met: met,
                cross_seeds: crossed.len(),
                score: at(0),
                score_perfect: at(1),
                tail: at(2),
                tail_perfect: at(3),
                converted: at(4),
                ranges,
                weights,
                range_weights,
                check,
            });
        }
    }
    Ok(ChartAptitude { factors: factors(live, infos, inp.seeds)?, variants })
}

/// The check of a variant on its first seed: the shape with a random plain deck at random ranks (rank 1 where the
/// ranks do not follow linearly) at the check power, against the linear prediction from that seed's numbers.
fn check(
    m: &mut Measure<'_, '_>,
    kinds: &[Kind],
    infos: &[RangeInfo],
    inp: &Inputs<'_>,
    p: &Performer,
    s: &Sample,
    salt: u64,
) -> Result<VariantCheck, Error> {
    let live = m.live;
    let b = m.base(s.seed, true)?.clone();
    let mut rng = Rng(APT_CHECK_SALT ^ salt);
    let mut rank_rng = Rng(APT_RANK_SALT ^ salt);
    let ranks: Vec<i32> =
        infos.iter().map(|_| if inp.linear { 1 + rank_rng.below(super::RANKS) as i32 } else { 1 }).collect();
    let mut d = Vec::with_capacity(infos.len());
    for (info, &r) in infos.iter().zip(&ranks) {
        d.push(((info.percent(r)? - info.percent(1)?) as f64 / 100.0, info.percent(r)? as f64 / 100.0));
    }
    // the base at these ranks (exact) and the shape's increment by the linear rank formula
    let mut base = b.just.score as i64;
    for (r, info_r) in b.just.ranges.iter().zip(infos.iter().zip(&ranks)) {
        let (info, &rank) = info_r;
        base += range_score(r) as i64 * info.percent(rank)? / 100 - r.rank_bonus.unwrap_or(0) as i64;
    }
    let mut delta = s.values[2];
    for (j, &(_, pr)) in d.iter().enumerate() {
        delta += s.values[5 + 6 * j] * (1.0 + pr);
    }
    let usable: Vec<usize> = m.plain.into_iter().collect();
    let (deck, rows) = check_deck(kinds, &usable, live.positions, &mut rng);
    let master = Live::master_with(&m.master, &rows);
    let unit = m.unit;
    let weight = |_: usize, k: usize| -> f64 {
        let (Some(bc), Some((dw, drw))) = (&b.cross, &s.cross) else { return f64::NAN };
        let w0 = (bc[k].0 as f64 - b.just.score as f64) / (POWER as f64 * unit);
        let mut w = w0 + dw[k];
        for (j, &(dp, _)) in d.iter().enumerate() {
            let rw0 = (bc[k].1[j] as f64 - range_score(&b.just.ranges[j]) as f64) / (POWER as f64 * unit);
            w += dp * (rw0 + drw[k][j]);
        }
        w
    };
    let floors = inp.judged as f64 + MAX_GEKISOU_FEVERS as f64 + 2.0 * infos.len() as f64;
    let slack = 2.0 * infos.len() as f64;
    let confirmations: Vec<(i32, i64)> =
        ranks.iter().zip(infos).map(|(&r, info)| Ok((r, info.percent(r)?))).collect::<Result<_, Error>>()?;
    let external = inp.linear.then_some(confirmations.as_slice());
    let per_power = (base as f64 + delta) / POWER as f64;
    let c: Checked = live
        .check_with(kinds, &master, std::slice::from_ref(p), &deck, s.seed, external, per_power, weight, floors, slack)?
        .within(|| format!("Gekisou aptitude, seed {} at ranks {ranks:?}", s.seed))?;
    Ok(VariantCheck { seed: s.seed, ranks, deck, exact: c.exact, predicted: c.predicted, bound: c.bound })
}
