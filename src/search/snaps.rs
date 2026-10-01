//! Live-score objective with snap skills: the whole-live simulation (Gekisou off) scores every deck, and the leaf
//! searches snap placements and performance orders together.
//!
//! The argument is in `docs/search.md` ("Live score with snap skills"); the names below follow it. In short:
//! - every snap is classified per member card: its support skills either cannot change the simulation for that
//!   member under this play and pool (the empty class), or they fall in a class of snaps whose effect rows are
//!   interchangeable in the simulation; within one class assignment the score depends on the deck power alone and is
//!   non-decreasing in it, so the best snaps of a class assignment come from a constrained max-weight matching;
//! - an upper bound of the score is linear in the power: `P * (A0 + sum over positions of G)`, where `A0` is the
//!   no-skill value per unit of power and `G` the largest value per unit of power the performer at one position can
//!   add (its live skill, extended by its snap, and its snap's own score effects), each with a margin `eps` for the
//!   float arithmetic of the simulation (rounding and the drift of re-executed frames);
//! - candidates (order, class assignment) are enumerated below the bound and simulated in order of a per-note bound
//!   that reads the candidate's own conversions and, when it guards nowhere and recovers life only at its skill
//!   events, the notes whose filed damage empties the life; a candidate is dropped only when a bound is strictly
//!   below the best exact score or the Top-K threshold.

use crate::clock::Instant;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::cards::{MemberView, SnapView};
use crate::error::Error;
use crate::live::full::{GekisouSetup, LiveModel, LiveNote, LiveParams, LivePlay, Performer};
use crate::live::model::JudgementStream;
use crate::live::score::{
    COMBO, ComboTable, GEKISOU_COMBO, LiveScoreSettings, convert_score_type, get_frame, get_music_score_level_factor,
};
use crate::live::skill::{judgement_factor_mill, note_factor_mill};
use crate::live::skip::Chart;
use crate::master::{GekisouSkillEffectRow, Master, SkillTargetRow};
use crate::search::matching::constrained_assignment;
use crate::search::pool::{Deck, Pool};
use crate::search::power::PowerStats;
use crate::search::tables::Tables;
use crate::search::topk::NO_SNAP;

/// Relative error bound of the per-note float chain (as for the per-order model).
const CHAIN_EPS: f64 = 2e-6;
/// Concurrent executions of one snap effect.
const POOL: f64 = 5.0;
/// Pending candidates of one leaf before the best of them is simulated to raise the cutoff.
const FLUSH: usize = 64;

/// Validation switches of the live objective with Gekisou on. The first seven bits each replace one part of the
/// bound or of the seed loop by an inadmissible variant, so that the search must disagree with exhaustive
/// enumeration or report bound violations; `NO_EARLY_STOP` and `NO_PREFIX` switch off an exact shortcut.
pub mod ablate {
    /// No rank bonus factor.
    pub const RANK_BONUS: u32 = 1;
    /// No luck factor.
    pub const LUCK: u32 = 2;
    /// No Gekisou combo factor.
    pub const GEKISOU_COMBO: u32 = 4;
    /// The early stop also drops a candidate whose remaining bound equals the cutoff.
    pub const EARLY_STOP_EQUAL: u32 = 8;
    /// The per-seed bound replaced by the largest seed score simulated so far in the leaf.
    pub const OBSERVED_MAX: u32 = 16;
    /// Gekisou support rows left out of the snap class key.
    pub const CLASS_KEY: u32 = 32;
    /// The shared prefix of the seed runs cloned one frame after its first possible draw.
    pub const PREFIX_LATE: u32 = 64;
    /// Every candidate simulated on every seed.
    pub const NO_EARLY_STOP: u32 = 128;
    /// Every seed run from the first frame.
    pub const NO_PREFIX: u32 = 256;
}

thread_local! {
    static ABLATION: Cell<u32> = const { Cell::new(0) };
}

/// Sets the validation switches ([`ablate`]) of the searches run on the calling thread; 0 (the default) runs the
/// exact search with every shortcut. For validation only.
#[doc(hidden)]
pub fn set_bound_ablation(bits: u32) {
    ABLATION.with(|a| a.set(bits));
}

fn ablated(bit: u32) -> bool {
    ABLATION.with(|a| a.get() & bit != 0)
}

// TEMPORARY (measurement only, removed before release): `G6_OFF` bits switch a tightening off (1 conversion
// budgets, 2 Gekisou combo by count, 4 caches); `G6_TRACE` prints diagnostics.
fn g6_off(bit: u32) -> bool {
    static OFF: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("G6_OFF").ok().and_then(|x| x.parse().ok()).unwrap_or(0)) & bit != 0
}
fn g6_trace() -> bool {
    static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::env::var_os("G6_TRACE").is_some())
}

/// A chart, a judgement stream and the live's numbers, prepared for simulating many decks.
#[derive(Clone, Debug)]
pub(crate) struct FullSetup {
    pub notes: Vec<LiveNote>,
    pub events: Vec<(i32, i32)>,
    pub play: LivePlay,
    pub params: LiveParams,
    /// With Gekisou on: the Gekisou setup, the frame delta times and the seed set.
    pub gk: Option<GkPlay>,
}

/// The Gekisou part of a live objective.
#[derive(Clone, Debug)]
pub(crate) struct GkPlay {
    pub setup: GekisouSetup,
    /// Delta time of each play frame, in seconds.
    pub dt: Vec<f32>,
    pub seeds: Vec<i32>,
}

impl FullSetup {
    pub fn new(
        master: &Master,
        music_level: i32,
        chart: &Chart,
        stream: &JudgementStream,
        judgement_types: &[i32],
    ) -> Result<FullSetup, Error> {
        if judgement_types.len() != chart.notes.len() {
            return Err(Error::Input(format!(
                "{} note judgement types for {} chart notes",
                judgement_types.len(),
                chart.notes.len()
            )));
        }
        let notes = chart
            .notes
            .iter()
            .zip(judgement_types)
            .map(|(n, &jt)| LiveNote {
                note_id: n.id,
                time_ms: n.time_ms,
                note_operate_type: n.note_type,
                judgement_type: jt,
            })
            .collect();
        let events = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
        let assist_factor = if stream.assist {
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
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 0,
            music_level,
            converted_note_count: chart.converted_note_count,
            music_length_ms: chart.last_timing_note_ms.wrapping_add(1000),
            score_music_length_ms: None,
            assist_factor,
        };
        Ok(FullSetup { notes, events, play: stream.to_live_play()?, params, gk: None })
    }

    /// Plays the live with Gekisou on, on these frame delta times and seeds.
    pub fn set_gekisou(&mut self, setup: GekisouSetup, dt: Vec<f32>, seeds: Vec<i32>) {
        self.gk = Some(GkPlay { setup, dt, seeds });
    }

    /// The simulated score of performers (in performance order) at a deck power, Gekisou off.
    pub fn score(&self, master: &Master, performers: &[Performer], power: i32) -> Result<i32, Error> {
        let params = LiveParams { total_power: power, ..self.params };
        let mut lm = LiveModel::new(master, performers, &self.notes, &self.events, params)?;
        lm.run(&self.play)
    }

    /// With Gekisou on: a live of performers at a deck power, before its first frame.
    pub fn gekisou_model(&self, master: &Master, performers: &[Performer], power: i32) -> Result<LiveModel, Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        let params = LiveParams { total_power: power, ..self.params };
        LiveModel::new_gekisou(master, performers, &self.notes, &self.events, params, &g.setup)
    }

    /// With Gekisou on: the score of performers on every seed, in order, each an independent run of the whole play
    /// with its base seed set to the seed ([`LiveModel::run_timed`]).
    pub fn seed_scores(&self, master: &Master, performers: &[Performer], power: i32) -> Result<Vec<i32>, Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        let mut play = self.play.clone();
        let mut out = Vec::with_capacity(g.seeds.len());
        for &seed in &g.seeds {
            let mut lm = self.gekisou_model(master, performers, power)?;
            play.base_seed = seed;
            out.push(lm.run_timed(&play, &g.dt)?);
        }
        Ok(out)
    }

    /// Plays frames `from..` of the play with Gekisou on; returns the final score.
    fn play_from(&self, lm: &mut LiveModel, from: usize) -> Result<i32, Error> {
        let g = self.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        for (f, &dt) in self.play.frames[from..].iter().zip(&g.dt[from..]) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
        }
        Ok(lm.score())
    }
}

/// The performer of a member card paired with a snap (the Gekisou fields are read only by a live with Gekisou on).
pub(crate) fn performer(m: &MemberView, s: Option<&SnapView>) -> Result<Performer, Error> {
    Ok(Performer {
        live_skill: Some((m.live_skill_id, m.live_skill_level)),
        support_skills: match s {
            None => Vec::new(),
            Some(s) => s.support_skills()?,
        },
        band_id: m.band_id,
        character_id: m.character_id,
        card_type: m.card_type,
        tag_ids: m.best_music_tag_ids.clone(),
        live_skill_categories: m.live_skill_categories.clone().unwrap_or_default(),
        gekisou_skill_categories: m.gekisou_skill_categories.clone().unwrap_or_default(),
        gekisou_mission_type: m.gekisou_mission_type.unwrap_or(0),
        gekisou_skill: (m.gekisou_skill_id != 0).then_some((m.gekisou_skill_id, m.gekisou_skill_level)),
        gekisou_support_skills: match s {
            None => Vec::new(),
            Some(s) => s.gekisou_support_skills()?,
        },
    })
}

/// The performers of a deck, in performance order.
pub(crate) fn deck_performers(pool: &Pool, deck: &Deck) -> Result<Vec<Performer>, Error> {
    deck.performance_order
        .iter()
        .map(|&slot| performer(&pool.members[deck.members[slot]], deck.snaps[slot].map(|s| &pool.snaps[s])))
        .collect()
}

/// Possible results of a condition (or group) for one performer, and whether asking it can change anything else
/// (a draw from the random stream, a life query when life is not rigid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Out {
    yes: bool,
    no: bool,
    impure: bool,
}

impl Out {
    fn not(self) -> Out {
        Out { yes: self.no, no: self.yes, impure: self.impure }
    }
    fn decided_true(self) -> bool {
        self.yes && !self.no && !self.impure
    }
}

/// Keep the full member view: tags, categories and mission can all affect a target.
type Attr<'a> = &'a MemberView;

fn no_value() -> Error {
    Error::Master("skill condition without a value".into())
}

/// Use the same complete live predicate as the frame simulator.
fn target_matches(tg: &SkillTargetRow, a: Attr<'_>) -> Result<bool, Error> {
    Ok(performer(a, None)?.matches_skill_target(tg))
}

/// The judgement a convert effect converts to (-1: none).
fn convert_to(effect_type: i64, value: i64) -> i32 {
    if effect_type == 13005 {
        return 6;
    }
    let v = value as i32;
    if (v.wrapping_sub(1) as u32) > 5 { -1 } else { v }
}

/// Whether a sustained effect has an activation time (the simulation rejects that).
fn has_activation_time(act: f32) -> bool {
    if act.is_nan() || act <= 0f32 {
        return false;
    }
    let big = 2147483647f32;
    let m = act.abs().max(big);
    let tol = (m * 1e-6f32).max(f32::from_bits(1) * 8f32);
    tol <= (big - act).abs()
}

/// The facts of the chart, the play and the allowed cards that the classification reads.
struct Env<'a> {
    master: &'a Master,
    /// Condition sets of each group, in table order.
    sets: HashMap<i64, Vec<&'a [i64]>>,
    /// Every life the live can reach lies in `[life_lo, life_hi]`.
    life_lo: i64,
    life_hi: i64,
    /// Every life condition any allowed card can ask is decided on `[life_lo, life_hi]`, and the note score's life
    /// factor is fixed: then life values cannot change the score.
    life_rigid: bool,
    /// Raw judgements of the stream.
    raw: Vec<i32>,
    /// With Gekisou on: the missions of the ranges, whether some range completes in the play, and whether some
    /// reachable judgement is a Miss or a Bad.
    gk: Option<GkEnv>,
    /// With Gekisou on: the per-frame facts the windows of Gekisou rows read.
    gkf: Option<GkFrames>,
    /// The windows of Gekisou rows by (trigger, trigger type, gate, activation time bits, release).
    gk_cache: RefCell<HashMap<GkWindowKey, Rc<GkRowWin>>>,
    /// The conversion budgets of Gekisou rows by the fields `gk_budget` reads.
    budget_cache: RefCell<HashMap<BudgetKey, Option<f64>>>,
}

type GkWindowKey = (i64, i64, i64, u32, i64);
type SimulationMemberKey = (i64, i64, i64, i64, i64, i64, i64, usize);
type SeededDeckKey = ([(u32, u32); 5], i64);
type ComboBonusRow = (i64, i64, f64);

/// The fields of a row that its conversion budget reads.
type BudgetKey = (GkWindowKey, (i64, i64, i64, i64, i64), Vec<i64>);

/// The Gekisou facts the classification reads.
#[derive(Clone, Debug)]
struct GkEnv {
    missions: Vec<i64>,
    completes: bool,
    breaks: bool,
}

impl GkEnv {
    fn has(&self, mission: i64) -> bool {
        self.missions.contains(&mission)
    }

    /// Whether a mission target list (none or All: any) matches some range.
    fn any_of(&self, targets: &[i64]) -> bool {
        if targets.is_empty() || targets.contains(&MISSION_ALL) {
            return !self.missions.is_empty();
        }
        targets.iter().any(|m| self.has(*m))
    }
}

/// Gekisou missions.
const MISSION_COMBO: i64 = 1;
const MISSION_LUCK: i64 = 2;
const MISSION_ALL: i64 = 4;
/// Gekisou range states (as `live::full` reports them).
const RS_STANDBY: u8 = 2;
const RS_START: u8 = 3;
const RS_PLAYING: u8 = 4;
const RS_END: u8 = 5;
const RS_COMPLETE: u8 = 7;
const RS_FINISH: u8 = 8;

impl Env<'_> {
    fn cond(&self, cid: i64, a: Attr<'_>) -> Result<Option<Out>, Error> {
        let m = self.master;
        let c = m.skill_condition(cid).ok_or_else(|| Error::Master(format!("unknown skill condition {cid}")))?;
        let v0 = c.condition_values.first().copied();
        let target = |i: usize| {
            m.skill_target(c.condition_target_ids[i])
                .ok_or_else(|| Error::Master(format!("unknown skill target {}", c.condition_target_ids[i])))
        };
        let (lo, hi) = (self.life_lo, self.life_hi);
        let o = match c.condition_type {
            0 => return Ok(None),
            2001 => {
                let v = v0.ok_or_else(no_value)?;
                Out { yes: hi >= v, no: lo < v, impure: !self.life_rigid }
            }
            2003 => {
                let v = v0.ok_or_else(no_value)?;
                Out { yes: lo <= v, no: hi > v, impure: !self.life_rigid }
            }
            4010 => Out { yes: true, no: true, impure: false },
            4011 => {
                let v = v0.ok_or_else(no_value)?;
                Out { yes: v as f32 / 100f32 > 0f32, no: true, impure: true }
            }
            5000 => {
                let mut fixed = false;
                for i in 0..c.condition_target_ids.len() {
                    if target_matches(target(i)?, a)? {
                        fixed = true;
                        break;
                    }
                }
                Out { yes: fixed, no: !fixed, impure: false }
            }
            8000 => Out { yes: false, no: true, impure: false },
            1030 => {
                v0.ok_or_else(no_value)?;
                for i in 0..c.condition_target_ids.len() {
                    target(i)?;
                }
                Out { yes: true, no: true, impure: false }
            }
            7000 => {
                v0.ok_or_else(no_value)?;
                let yes = self.gk.as_ref().is_some_and(|g| g.has(MISSION_LUCK));
                Out { yes, no: true, impure: false }
            }
            t @ (7005 | 7010 | 7013 | 7020 | 7021) => {
                let Some(g) = &self.gk else {
                    return Err(Error::Unsupported(format!(
                        "condition type {t} reads the Gekisou state of a live without Gekisou"
                    )));
                };
                let yes = match t {
                    7005 => v0.ok_or_else(no_value)? > 0 && !g.missions.is_empty(),
                    7010 | 7020 => {
                        let mut ms = Vec::new();
                        for i in 0..c.condition_target_ids.len() {
                            let tg = target(i)?;
                            if tg.skill_target_type == 5 && tg.gekisou_mission_type != 0 {
                                ms.push(tg.gekisou_mission_type);
                            }
                        }
                        g.any_of(&ms)
                    }
                    7013 => g.completes,
                    _ => g.has(MISSION_LUCK),
                };
                Out { yes, no: true, impure: false }
            }
            t => return Err(Error::Unsupported(format!("skill condition type {t}"))),
        };
        Ok(Some(if c.is_positive { o } else { o.not() }))
    }

    /// A condition group: an OR over its sets, each an AND over its conditions (`None`: no checker).
    fn group(&self, gid: i64, a: Attr<'_>) -> Result<Option<Out>, Error> {
        if gid == 0 {
            return Ok(None);
        }
        let mut any: Option<Out> = None;
        for s in self.sets.get(&gid).map_or(&[][..], |v| &v[..]) {
            let mut and: Option<Out> = None;
            for &cid in s.iter() {
                if let Some(o) = self.cond(cid, a)? {
                    and = Some(match and {
                        None => o,
                        Some(x) => Out { yes: x.yes && o.yes, no: x.no || o.no, impure: x.impure || o.impure },
                    });
                }
            }
            if let Some(o) = and {
                any = Some(match any {
                    None => o,
                    Some(x) => Out { yes: x.yes || o.yes, no: x.no && o.no, impure: x.impure || o.impure },
                });
            }
        }
        Ok(any)
    }

    /// Whether a group is exactly one positive "same member's live skill fired" condition (its hits are the frames
    /// where the chart fires a skill event of the performer's position).
    fn event_only(&self, gid: i64) -> bool {
        let mut items = Vec::new();
        for s in self.sets.get(&gid).map_or(&[][..], |v| &v[..]) {
            let set: Vec<i64> = s
                .iter()
                .copied()
                .filter(|&c| self.master.skill_condition(c).is_none_or(|r| r.condition_type != 0))
                .collect();
            if !set.is_empty() {
                items.push(set);
            }
        }
        if items.len() != 1 || items[0].len() != 1 {
            return false;
        }
        self.master.skill_condition(items[0][0]).is_some_and(|c| c.condition_type == 4010 && c.is_positive)
    }

    /// Whether two condition groups are one condition each, the same except that one is negated: checked in the
    /// same frame and phase, at most one of them holds.
    fn negations(&self, g1: i64, g2: i64) -> bool {
        let single = |g: i64| -> Option<i64> {
            let sets = self.sets.get(&g)?;
            let mut ids = sets
                .iter()
                .flat_map(|s| s.iter().copied())
                .filter(|&c| self.master.skill_condition(c).is_none_or(|r| r.condition_type != 0));
            let first = ids.next()?;
            (ids.next().is_none() && sets.iter().filter(|s| !s.is_empty()).count() == 1).then_some(first)
        };
        let (Some(a), Some(b)) = (single(g1), single(g2)) else { return false };
        let (Some(a), Some(b)) = (self.master.skill_condition(a), self.master.skill_condition(b)) else { return false };
        a.condition_type == b.condition_type
            && a.condition_values == b.condition_values
            && a.condition_target_ids == b.condition_target_ids
            && a.is_positive != b.is_positive
            && !matches!(a.condition_type, 1030 | 4011)
    }

    /// Validates a cumulative condition the way the simulation does; true when counting it fails.
    fn cumulative_fails(&self, cid: i64) -> Result<bool, Error> {
        if cid == 0 {
            return Ok(false);
        }
        let m = self.master;
        let c =
            m.cumulative_condition(cid).ok_or_else(|| Error::Master(format!("unknown cumulative condition {cid}")))?;
        match c.condition_type {
            7001 => Ok(false),
            1000 => {
                for &i in &c.condition_target_ids {
                    m.skill_target(i).ok_or_else(|| Error::Master(format!("unknown skill target {i}")))?;
                }
                Ok(c.condition_values.is_empty())
            }
            t => Err(Error::Unsupported(format!("cumulative condition type {t}"))),
        }
    }

    /// Whether asking a condition group can draw a random number (a probability condition in one of its sets).
    fn draws(&self, gid: i64) -> bool {
        gid != 0
            && self.sets.get(&gid).is_some_and(|v| {
                v.iter()
                    .flat_map(|s| s.iter())
                    .any(|&c| self.master.skill_condition(c).is_some_and(|r| r.condition_type == 4011))
            })
    }

    fn targets(&self, ids: &[i64]) -> Result<Vec<i64>, Error> {
        ids.iter()
            .map(|&t| {
                self.master
                    .skill_target(t)
                    .map(|r| r.judgement)
                    .ok_or_else(|| Error::Master(format!("unknown skill target {t}")))
            })
            .collect()
    }
}

/// An effect row of a live skill or a snap skill, as the classification and the bounds read it.
#[derive(Clone, Debug)]
struct Row {
    trigger_type: i64,
    trigger: i64,
    condition: i64,
    release: i64,
    reset: i64,
    cumulative: i64,
    effect_type: i64,
    value: i64,
    act: f32,
    limit: i64,
    execute_limit: i64,
    targets: Vec<i64>,
    /// `_maxEffectValue`.
    max_value: i64,
    /// A Gekisou or Gekisou support row, and the mission gating its triggers (0: none).
    gk: bool,
    gate: i64,
}

/// The rows of a support skill at a level, by id.
fn support_rows(env: &Env, id: i64, level: i64) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> =
        env.master.support_skill_effects.iter().filter(|r| r.support_skill_id == id && r.level == level).collect();
    rows.sort_by_key(|r| r.id);
    rows.iter()
        .map(|r| {
            let targets = if matches!(r.skill_effect_type, 2004 | 12006 | 13005) {
                env.targets(&r.skill_target_ids)?
            } else {
                Vec::new()
            };
            Ok(Row {
                trigger_type: r.skill_trigger_type,
                trigger: r.skill_trigger_condition_group,
                condition: r.skill_condition_group,
                release: r.skill_release_condition_group,
                reset: r.effect_execute_limit_reset_condition_group,
                cumulative: r.skill_cumulative_condition_id,
                effect_type: r.skill_effect_type,
                value: r.effect_value,
                act: r.activation_time_second,
                limit: r.effect_limit_count,
                execute_limit: r.effect_execute_limit_count,
                targets,
                max_value: r.max_effect_value,
                gk: false,
                gate: 0,
            })
        })
        .collect()
}

/// The rows of a Gekisou or Gekisou support skill at a level, by id, gated by `gate`.
fn gekisou_rows(env: &Env, table: &[GekisouSkillEffectRow], id: i64, level: i64, gate: i64) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> = table.iter().filter(|r| r.skill_id == id && r.level == level).collect();
    rows.sort_by_key(|r| r.id);
    rows.iter()
        .map(|r| {
            let targets = if matches!(r.skill_effect_type, 2004 | 12006 | 13005) {
                env.targets(&r.skill_target_ids)?
            } else {
                Vec::new()
            };
            Ok(Row {
                trigger_type: r.skill_trigger_type,
                trigger: r.skill_trigger_condition_group,
                condition: r.skill_condition_group,
                release: r.skill_release_condition_group,
                reset: r.effect_execute_limit_reset_condition_group,
                cumulative: r.skill_cumulative_condition_id,
                effect_type: r.skill_effect_type,
                value: r.effect_value,
                act: r.activation_time_second,
                limit: r.effect_limit_count,
                execute_limit: r.effect_execute_limit_count,
                targets,
                max_value: r.max_effect_value,
                gk: true,
                gate,
            })
        })
        .collect()
}

/// The rows of a live skill at a level, by id, checked the way the simulation checks them.
fn live_rows(env: &Env, id: i64, level: i64) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> =
        env.master.live_skill_effects.iter().filter(|r| r.live_skill_id == id && r.level == level).collect();
    rows.sort_by_key(|r| r.id);
    rows.iter()
        .map(|r| {
            if r.skill_release_condition_group != 0 || r.skill_cumulative_condition_id != 0 || r.effect_limit_count != 0
            {
                return Err(Error::Unsupported(format!(
                    "live skill effect {}: release condition, cumulative condition or effect limit",
                    r.id
                )));
            }
            if !matches!(r.skill_effect_type, 2000 | 2004 | 3001 | 3003 | 12006 | 13005 | 15000) {
                return Err(Error::Unsupported(format!("skill effect type {}", r.skill_effect_type)));
            }
            let targets = if matches!(r.skill_effect_type, 2004 | 12006 | 13005) {
                env.targets(&r.skill_target_ids)?
            } else {
                Vec::new()
            };
            Ok(Row {
                trigger_type: 0,
                trigger: 0,
                condition: r.skill_condition_group,
                release: 0,
                reset: 0,
                cumulative: 0,
                effect_type: r.skill_effect_type,
                value: r.effect_value,
                act: r.activation_time_second,
                limit: 0,
                execute_limit: 0,
                targets,
                max_value: r.max_effect_value,
                gk: false,
                gate: 0,
            })
        })
        .collect()
}

/// Effect of a snap row for one member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Never starts, and checking it changes nothing.
    Never,
    /// May start, but cannot change the score under this play and pool.
    Inert,
    /// May change the score.
    Active,
}

/// The part of a row that identifies it in the simulation, with a decided pure condition replaced by "none".
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RowSig {
    trigger_type: i64,
    trigger: i64,
    condition: i64,
    release: i64,
    reset: i64,
    cumulative: i64,
    effect_type: i64,
    value: i64,
    act: u32,
    limit: i64,
    execute_limit: i64,
    targets: Vec<i64>,
    max_value: i64,
}

/// An active snap row as the bounds read it.
#[derive(Clone, Debug)]
struct ActiveRow {
    effect_type: i64,
    value: i64,
    act: f32,
    /// The trigger is the performer's own skill event (starts in the frames where it fires).
    event_bound: bool,
    /// Trigger and condition can both hold.
    can_start: bool,
    targets: Vec<i64>,
    /// A cumulative note score up (2001) whose factor changes while it runs (`value` is its largest value), and the
    /// most changes of one execution (`None`: any frame).
    churn: bool,
    churn_max: Option<f64>,
    /// A Gekisou row: its factor windows in chart time `(start, end, concurrent executions)` and the play-frame
    /// index ranges `(a, b]` whose judgements its conversion can see.
    gk_win: Option<Vec<(i64, i64, f64)>>,
    gk_conv: Option<Vec<(i64, i64)>>,
    /// A Gekisou conversion row whose conversions in the play are fewer than the entries it can see: at most this
    /// many (see `gk_budget`).
    budget: Option<f64>,
}

/// A live skill row of a member with its condition result and the row whose condition is its negation, if any.
#[derive(Clone, Debug)]
struct LiveRow {
    row: Row,
    out: Option<Out>,
    partner: Option<usize>,
}

/// Effect types a Gekisou or Gekisou support row may have.
const GK_TYPES: [i64; 18] = [
    2000, 2001, 2004, 3001, 3003, 4004, 11000, 11001, 11002, 11003, 11005, 12000, 12004, 12006, 13000, 13002, 13005,
    15000,
];

/// Classification of one row for a member.
fn support_status(env: &Env, r: &Row, a: Attr<'_>) -> Result<(Status, bool, bool), Error> {
    if r.trigger_type != 1 && r.trigger_type != 2 {
        return Err(Error::Unsupported(format!("skill trigger type {}", r.trigger_type)));
    }
    if r.trigger_type == 2 && has_activation_time(r.act) {
        return Err(Error::Unsupported("sustained effect with an activation time".into()));
    }
    if r.gk {
        // a Gekisou row checks its triggers only while a range of its mission is concerned: with no such range it
        // never checks anything
        let g = env.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou row in a live without Gekisou".into()))?;
        if r.gate != MISSION_ALL && !g.has(r.gate) {
            return Ok((Status::Never, false, false));
        }
    }
    let trig = env.group(r.trigger, a)?;
    let cond = env.group(r.condition, a)?;
    let reset = env.group(r.reset, a)?;
    let release = env.group(r.release, a)?;
    let fails = env.cumulative_fails(r.cumulative)?;
    let can_start = trig.is_some_and(|t| t.yes) && cond.is_none_or(|c| c.yes);
    let event_bound = env.event_only(r.trigger);
    let modelled = if r.gk {
        GK_TYPES.contains(&r.effect_type)
    } else {
        matches!(r.effect_type, 2000 | 2004 | 3001 | 3003 | 12006 | 13005 | 15000)
    };
    if !modelled && can_start {
        return Err(Error::Unsupported(format!("skill effect type {}", r.effect_type)));
    }
    if trig.is_some_and(|t| t.impure) || reset.is_some_and(|t| t.impure) {
        return Ok((Status::Active, can_start, event_bound));
    }
    if !trig.is_some_and(|t| t.yes) {
        return Ok((Status::Never, false, event_bound));
    }
    if cond.is_some_and(|c| c.impure) {
        return Ok((Status::Active, can_start, event_bound));
    }
    if !can_start {
        return Ok((Status::Never, false, event_bound));
    }
    if release.is_some_and(|t| t.impure) || fails {
        return Ok((Status::Active, true, event_bound));
    }
    let luck = env.gk.as_ref().is_some_and(|g| g.has(MISSION_LUCK));
    let ranges = env.gk.as_ref().is_some_and(|g| !g.missions.is_empty());
    let st = match r.effect_type {
        2000 | 2001 | 2004 | 15000 => Status::Active,
        // the Just counts read by nothing that reaches the score
        4004 | 13000 | 13002 => Status::Inert,
        // lottery weights, gauge and points: consumed only in luck ranges
        11000 | 11001 | 11002 | 11003 | 11005 => {
            if luck {
                Status::Active
            } else {
                Status::Inert
            }
        }
        12000 => {
            if ranges {
                Status::Active
            } else {
                Status::Inert
            }
        }
        // combo protection only acts on a Miss or a Bad
        12004 => {
            if env.gk.as_ref().is_some_and(|g| g.breaks) {
                Status::Active
            } else {
                Status::Inert
            }
        }
        3001 | 3003 => {
            if env.life_rigid {
                Status::Inert
            } else {
                Status::Active
            }
        }
        _ => {
            let to = convert_to(r.effect_type, r.value);
            if to != -1 && env.raw.iter().any(|&j| j != to && r.targets.contains(&(j as i64))) {
                Status::Active
            } else {
                Status::Inert
            }
        }
    };
    Ok((st, true, event_bound))
}

/// A class of snaps for one member: the snaps whose active rows are the same in the simulation.
#[derive(Clone, Debug)]
struct Class {
    /// Allowed-snap indexes (into `Tables::snaps`).
    snaps: Vec<usize>,
    rows: Vec<ActiveRow>,
}

/// Per-entry coefficients of the bound, entries sorted by chart time.
#[derive(Clone, Debug, Default)]
struct Coef {
    times: Vec<i32>,
    /// Power-free part of the float chain before the floor: `adj * level factor * note% * combo / divisor`.
    k: Vec<f64>,
    /// Largest judgement percent over the reachable judgements.
    max_jp: Vec<f64>,
    /// Judgement percent of Good, Great, Perfect, Just when reachable, else 0.
    jp: Vec<[f64; 4]>,
    /// The reachable judgements (bit `j` for judgement `j`) these read: without the conversions of rows with a
    /// conversion budget.
    vmask: Vec<u8>,
    /// Factor after the floor (assist times the largest life factor).
    z: Vec<f64>,
    /// Prefix sums of `z * k * max_jp` and of `z * k * jp[j]`.
    pc: Vec<f64>,
    pj: [Vec<f64>; 4],
    /// The same with the life-zero factor (assist times the life-zero factor) in place of `z`.
    pcd: Vec<f64>,
    pjd: [Vec<f64>; 4],
}

/// What the per-entry bound of one candidate reads, entries in chart-time order. A candidate's entries reach only
/// the judgements its own performers' conversions can give them (at its positions' events), so its combo breaks and
/// judgement percents are those of that reach, never above the pool-wide ones of `Coef`.
#[derive(Clone, Debug)]
struct Fine {
    /// Raw judgement of each entry.
    raw: Vec<u8>,
    /// Index of the first entry at the same chart time.
    group: Vec<u32>,
    /// `adj * level factor * note%` of each entry.
    pre: Vec<f64>,
    cnc: f64,
    /// Largest combo factor at any combo up to `c`, for every combo the pool-wide coefficients read (a candidate's
    /// combo counts are never larger).
    combo_max: Vec<f64>,
    /// By judgement mask (bit `j` for judgement `j`): the largest judgement percent, the percents of Good, Great,
    /// Perfect, Just when present (else 0), and whether no judgement above Bad is present (the entry breaks the
    /// combo whatever it ends as).
    mjp: Vec<f64>,
    jp4: Vec<[f64; 4]>,
    breaks: Vec<bool>,
    /// Conversion source of each member and class (0: none).
    src: Vec<Vec<u32>>,
    /// The judgements a source adds to each entry when its performer is at position `k`: `extra[source][k][entry]`,
    /// a mask; `extra_v` without the conversions of rows with a budget (the judgement percents read it, the combo
    /// breaks read `extra`).
    extra: Vec<[Vec<u8>; 5]>,
    extra_v: Vec<[Vec<u8>; 5]>,
    /// The rows of each source with a conversion budget: (judgement converted to, most conversions, the entries it
    /// can convert).
    budget: Vec<Vec<(u8, f64, Vec<u32>)>>,
    /// With Gekisou on and a combo range: what the candidate's Gekisou combo factor reads (see `GkCombo`).
    gcombo: Option<GkCombo>,
    /// Entries whose life is 0 in every play of a candidate that neither recovers life nor guards: the damage
    /// already filed when the entry reads its life empties it. `z_dead` is their factor after the floor.
    dead: Vec<bool>,
    /// With Gekisou on (empty otherwise): the factor of each entry's floored bound for the rank bonuses whose range
    /// score contains it, and whether its combo is bounded from the first entry on (a rank bonus that reads it
    /// before a combo break is judged).
    rank: Vec<f64>,
    nobreak: Vec<bool>,
    z_dead: f64,
    /// The rows of each member and class that can raise the life.
    life: Vec<Vec<LifeKind>>,
    /// For candidates that recover life at their skill events: the base life; the slots of the life fold in time
    /// order (a run of life frames that the frame cache can fold twice, or one chart time outside such runs) with
    /// the last time each covers and its damage (the smallest damage of the reachable judgements of its entries);
    /// for each position, the slot of each of its skill events' recoveries and how many times the fold can apply
    /// it; for each entry, the smallest chart time of the entries judged after it.
    base: i64,
    slot_end: Vec<i64>,
    slot_dmg: Vec<i64>,
    ev_slot: [Vec<(usize, i64)>; 5],
    until: Vec<i64>,
    /// The smallest `until` from each entry on, and the first entry from which every entry is in `dead`.
    until_min: Vec<i64>,
    dead_from: usize,
}

/// The rows of one performer that can raise the life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifeKind {
    None,
    /// Only life recovery rows triggered by the performer's own skill event, recovering this much in total there.
    Recovery(i64),
    /// Anything else (a guard, a recovery with another trigger, a live skill row).
    Other,
}

/// What the per-entry bound of one candidate knows about its life.
#[derive(Clone, Copy, Debug)]
enum CandLife {
    /// Nothing raises the life: `Fine::dead` applies.
    NoRise,
    /// Only recoveries at skill events: an entry reads life 0 when its chart time is at least this time and the entries
    /// at chart times up to it are judged no later than the entry (see `Fine::zero_from`).
    ZeroFrom(i64),
    /// No life bound.
    Unknown,
}

impl Fine {
    /// The last time of the first slot at which the life is 0 in the slot-ordered fold of every damage (smallest
    /// damage per entry) and of the recoveries `rec[k]` at position `k`'s skill events, each as many times as the
    /// frame cache can apply it (`i64::MAX`: never). The commands of one slot are folded in an order that is not
    /// known, so a slot with both damage and recovery gives `clamp(life + recovery - damage, 0, 2 * base)`, at least
    /// what any order gives.
    fn zero_from(&self, rec: [i64; 5]) -> i64 {
        let mut ups: Vec<(usize, i64)> = Vec::new();
        for (k, slots) in self.ev_slot.iter().enumerate() {
            if rec[k] > 0 {
                ups.extend(slots.iter().map(|&(slot, times)| (slot, rec[k].saturating_mul(times))));
            }
        }
        ups.sort_unstable();
        let cap = 2 * self.base;
        let mut life = self.base;
        let mut j = 0usize;
        for (slot, (&end, &dmg)) in self.slot_end.iter().zip(&self.slot_dmg).enumerate() {
            let mut up = 0i64;
            while j < ups.len() && ups[j].0 == slot {
                up = up.saturating_add(ups[j].1);
                j += 1;
            }
            life = if up == 0 {
                (life - dmg).max(0)
            } else if dmg == 0 {
                life.saturating_add(up).min(cap)
            } else {
                (life.saturating_add(up) - dmg).clamp(0, cap)
            };
            if life <= 0 {
                return end;
            }
        }
        i64::MAX
    }
}

/// The conversions of one performer: its live skill's (with their target judgement and targets) and its snap
/// class's (also with activation time bits and whether the trigger is its own skill event).
type ConvSource = (Vec<(i32, Vec<i64>)>, Vec<SnapConv>);

/// A snap class's conversion: judgement converted to, targets, activation time bits, whether the trigger is the
/// performer's own skill event, the frame ranges of a Gekisou row, and its conversion budget (bits).
type SnapConv = (i32, Vec<i64>, u32, bool, Option<Vec<(i64, i64)>>, Option<u64>);

/// One window of score factors of a performer: entries `lo..hi` (time order), note factor and judgement factors.
#[derive(Clone, Copy, Debug)]
struct Window {
    lo: u32,
    hi: u32,
    note: f64,
    judge: [f64; 4],
}

/// The bound data of one (member, class, position).
#[derive(Clone, Debug, Default)]
struct Contrib {
    windows: Vec<Window>,
    gain: f64,
    judge: bool,
    /// Score-frame executions that apply this performer's factor commands (see `cand_eps`).
    ops: f64,
    /// Every factor window in chart time (with or without notes): start, end, largest factor on one note.
    spans: Vec<(i64, i64, f64)>,
    /// The part of `gain` for the conversions of rows with a conversion budget (see `SnapLive::new`).
    budget: f64,
    /// The class's Gekisou combo bonus windows (`combo_windows`).
    cb: Vec<(i64, i64, f64)>,
}

/// A judgement conversion some allowed card can register: the judgement it converts to, its target judgements, and
/// the play-frame index ranges `(a, b]` of the frames whose judgements it can see.
type Conv = (i32, Vec<i64>, Vec<(i64, i64)>);

/// A snap assignment and its weight.
type Assignment = (i64, [Option<usize>; 5]);

/// One candidate of a leaf.
#[derive(Clone, Debug)]
struct Cand {
    bound: i64,
    power: i64,
    snaps: [Option<usize>; 5],
    snap_ids: [i64; 5],
    order: [usize; 5],
    classes: [usize; 5],
}

/// The best representative of a member set.
#[derive(Clone, Debug)]
pub(crate) struct LeafBest {
    pub score: i64,
    pub power: i64,
    /// Pool snap index of each slot.
    pub snaps: [Option<usize>; 5],
    pub order: [usize; 5],
}

/// The live objective with snap skills, prepared for one search.
pub(crate) struct SnapLive<'a> {
    master: &'a Master,
    setup: &'a FullSetup,
    /// Classes of each pool member (class 0: no active row; empty for members that are not allowed).
    classes: Vec<Vec<Class>>,
    /// Class of each allowed snap for each member.
    class_of: Vec<Vec<u16>>,
    /// `contrib[m][c][k]`.
    contrib: Vec<Vec<[Contrib; 5]>>,
    /// Identity of each member for the simulation (equal ids: interchangeable performers with equal snap classes).
    sim_id: Vec<u32>,
    /// Pool-wide id of each member's classes (equal ids: equal class keys).
    class_gid: Vec<Vec<u32>>,
    coef: Coef,
    fine: Fine,
    a0: f64,
    global: f64,
    /// Whether some entry can read life 0 at a factor below the one of `Coef::z` (the class search then bounds life).
    life_bound: bool,
    /// With `life_bound`, for each member and position (`m * 5 + k`): the prefix sums, with `Coef::z` and with the
    /// life-zero factor, of the largest per-entry gain over the member's classes without other life-raising rows
    /// (empty when it has none).
    split: Vec<(Vec<f64>, Vec<f64>)>,
    /// With `life_bound`: the largest conversion budget term (`Contrib::budget`) of each member's classes without
    /// other life-raising rows, by `m * 5 + k`.
    split_budget: Vec<f64>,
    /// Relative margin of the bounds.
    eps: f64,
    /// The part of `eps` for the Gekisou factors of the per-note chain (0 with Gekisou off).
    chain_extra: f64,
    /// Seeds per deck (1 with Gekisou off): the value of a deck is the sum of its seed scores, and every bound of
    /// one seed's score is multiplied by `n` before it is compared with the cutoff.
    n: i64,
    /// With Gekisou on: a play frame no later than the first frame in which any deck can draw a random number.
    prefix_frame: usize,
}

/// A snap's class key for one member: per skill with active rows, in the performer's order, (skill kind: 3 snap, 5
/// Gekisou support; its mission gate; its active rows).
type ClassKey = Vec<(i64, i64, Vec<RowSig>)>;

/// Relative error added to the per-note chain margin with Gekisou on (the Gekisou combo and luck factors: their
/// binary32 computation and the two extra multiplications, with margin).
const GK_CHAIN_EPS: f64 = 1.0 / (1u64 << 21) as f64;

/// `SnapLive::split`: for each member and position, the largest gain of each entry over the member's classes without
/// other life-raising rows (the value of its windows containing the entry, before the factor after the floor), summed
/// with `Coef::z` and with the life-zero factor.
fn split_envelopes(contrib: &[Vec<[Contrib; 5]>], fine: &Fine, coef: &Coef) -> Vec<(Vec<f64>, Vec<f64>)> {
    let ne = coef.times.len();
    let mut out = vec![(Vec::new(), Vec::new()); contrib.len() * 5];
    let mut v = vec![0f64; ne];
    let mut best = vec![0f64; ne];
    for (m, per) in contrib.iter().enumerate() {
        for k in 0..5 {
            let mut any = false;
            best.fill(0.0);
            for (c, arr) in per.iter().enumerate() {
                if fine.life[m][c] == LifeKind::Other {
                    continue;
                }
                any = true;
                let ws = &arr[k].windows;
                for w in ws {
                    for e in w.lo as usize..w.hi as usize {
                        let mut x = w.note * coef.k[e] * coef.max_jp[e];
                        for j in 0..4 {
                            if w.judge[j] != 0.0 {
                                x += w.judge[j] * coef.k[e] * coef.jp[e][j];
                            }
                        }
                        v[e] += x;
                    }
                }
                for w in ws {
                    for e in w.lo as usize..w.hi as usize {
                        best[e] = best[e].max(v[e]);
                    }
                }
                for w in ws {
                    v[w.lo as usize..w.hi as usize].fill(0.0);
                }
            }
            if !any {
                continue;
            }
            let (mut pz, mut pd) = (vec![0f64; ne + 1], vec![0f64; ne + 1]);
            for e in 0..ne {
                pz[e + 1] = pz[e] + coef.z[e] * best[e];
                pd[e + 1] = pd[e] + fine.z_dead * best[e];
            }
            out[m * 5 + k] = (pz, pd);
        }
    }
    out
}

/// The sum of the `n` largest values (all of them when fewer); reorders `v`.
fn top_sum(v: &mut [f64], n: f64) -> f64 {
    let n = if n >= v.len() as f64 { v.len() } else { n.max(0.0) as usize };
    if n == 0 {
        return 0.0;
    }
    if n < v.len() {
        v.select_nth_unstable_by(n - 1, |a, b| b.total_cmp(a));
    }
    v[..n].iter().sum()
}

/// The largest `sum_i g[i][k_i]` over the assignments of the five slots to distinct positions.
fn best_assignment(g: &[[f64; 5]; 5]) -> f64 {
    let mut best = [f64::MIN; 32];
    best[0] = 0.0;
    for mask in 0usize..31 {
        if best[mask] == f64::MIN {
            continue;
        }
        let i = mask.count_ones() as usize;
        for k in 0..5 {
            if mask & (1 << k) == 0 {
                let v = best[mask] + g[i][k];
                let next = mask | (1 << k);
                if v > best[next] {
                    best[next] = v;
                }
            }
        }
    }
    best[31]
}

fn ub(power: i64, a: f64, eps: f64) -> i64 {
    let v = (power.max(0) as f64) * a * (1.0 + eps);
    if v >= i64::MAX as f64 { i64::MAX } else { v.ceil() as i64 }
}

impl<'a> SnapLive<'a> {
    /// Prepares the objective for the allowed members and snaps (`t.snaps`); rejects cards the simulation cannot run
    /// and inputs outside the domain of the bounds.
    pub fn new(
        pool: &Pool<'a>,
        t: &Tables,
        allowed_members: &[bool],
        setup: &'a FullSetup,
    ) -> Result<SnapLive<'a>, Error> {
        let t_new = Instant::now();
        let master: &'a Master = pool.master;
        let settings = LiveScoreSettings::from_master(master)?;
        let combo = ComboTable::from_master(master)?;
        let setting = |key: &str| -> Result<i64, Error> {
            let r = master
                .live_settings
                .iter()
                .find(|r| r.key == key)
                .ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
            r.value
                .trim()
                .parse::<i64>()
                .map_err(|_| Error::Master(format!("MasterLiveSettings {key} is not an integer")))
        };
        let base = setting("life_base")?;
        setting("life_denger")?;
        if base <= 0 || base > (1 << 29) {
            return Err(Error::Domain(format!("life_base {base} outside the modelled range")));
        }
        let mut damage: HashMap<i64, i64> = HashMap::new();
        for r in &master.judgement_parameters {
            if damage.insert(r.note_simulate_judgement, r.damage).is_some() {
                return Err(Error::Master("duplicate judgement parameter".into()));
            }
            if r.damage < 0 || r.damage > i32::MAX as i64 {
                return Err(Error::Domain("judgement damage outside the modelled range".into()));
            }
        }
        let notes: HashMap<i32, LiveNote> = setup.notes.iter().map(|n| (n.note_id, *n)).collect();
        // stream entries: (frame index, note, raw judgement)
        let mut entries = Vec::new();
        for (fi, f) in setup.play.frames.iter().enumerate() {
            for j in &f.judged {
                let n = *notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
                entries.push((fi, n, j.judgement));
            }
        }
        let mut raw: Vec<i32> = entries.iter().map(|e| e.2).collect();
        raw.sort_unstable();
        raw.dedup();

        let members: Vec<usize> = (0..pool.members.len()).filter(|&m| allowed_members[m]).collect();
        fn attr(m: &MemberView) -> Attr<'_> {
            m
        }
        let mut sets: HashMap<i64, Vec<&[i64]>> = HashMap::new();
        for s in &master.skill_condition_sets {
            sets.entry(s.group).or_default().push(&s.condition_ids);
        }
        let mut env = Env {
            master,
            sets,
            life_lo: 0,
            life_hi: 2 * base,
            life_rigid: false,
            raw,
            gk: None,
            gkf: None,
            gk_cache: RefCell::new(HashMap::new()),
            budget_cache: RefCell::new(HashMap::new()),
        };
        // every row an allowed card can bring
        let mut live: HashMap<(i64, i64), Vec<Row>> = HashMap::new();
        for &m in &members {
            let v = &pool.members[m];
            let k = (v.live_skill_id, v.live_skill_level);
            if let std::collections::hash_map::Entry::Vacant(e) = live.entry(k) {
                e.insert(live_rows(&env, k.0, k.1)?);
            }
        }
        let mut snap_rows: Vec<Vec<Vec<Row>>> = Vec::with_capacity(t.snaps.len());
        for &s in &t.snaps {
            let mut per = Vec::new();
            for (id, lv) in pool.snaps[s].support_skills()? {
                per.push(support_rows(&env, id, lv)?);
            }
            snap_rows.push(per);
        }
        // with Gekisou on: the Gekisou support skills of each snap (they run only for a member with a Gekisou skill)
        // and the Gekisou skill of each member, each gated by its mission
        let gk_on = setup.gk.is_some();
        let mut snap_gk_rows: Vec<Vec<Vec<Row>>> = vec![Vec::new(); t.snaps.len()];
        let mut member_gk: HashMap<(i64, i64), Vec<Row>> = HashMap::new();
        if gk_on {
            for (j, &s) in t.snaps.iter().enumerate() {
                for (id, lv) in pool.snaps[s].gekisou_support_skills()? {
                    let row = master
                        .gekisou_support_skill(id)
                        .ok_or_else(|| Error::Master(format!("unknown Gekisou support skill {id}")))?;
                    let table = &master.gekisou_support_skill_effects;
                    snap_gk_rows[j].push(gekisou_rows(&env, table, id, lv, row.gekisou_mission_type)?);
                }
            }
            for &m in &members {
                let v = &pool.members[m];
                let k = (v.gekisou_skill_id, v.gekisou_skill_level);
                if k.0 == 0 || member_gk.contains_key(&k) {
                    continue;
                }
                let row =
                    master.gekisou_skill(k.0).ok_or_else(|| Error::Master(format!("unknown Gekisou skill {}", k.0)))?;
                if !(1..=3).contains(&row.gekisou_mission_type) {
                    return Err(Error::Unsupported(format!("Gekisou mission type {}", row.gekisou_mission_type)));
                }
                let table = &master.gekisou_skill_effects;
                member_gk.insert(k, gekisou_rows(&env, table, k.0, k.1, row.gekisou_mission_type)?);
            }
        }
        let all_rows = || {
            live.values()
                .flatten()
                .chain(snap_rows.iter().flatten().flatten())
                .chain(snap_gk_rows.iter().flatten().flatten())
                .chain(member_gk.values().flatten())
        };
        // life: the judgements each stream entry can reach (conversions of any allowed card registered when the
        // entry is judged), damage and recovery
        let frames: Vec<i32> = setup.play.frames.iter().map(|f| f.time_ms).collect();
        // with Gekisou on: the ranges' schedule, which does not depend on the deck
        let sched = match &setup.gk {
            None => None,
            Some(g) => Some(Schedule::new(master, setup, g)?),
        };
        if let Some(sc) = &sched {
            env.gkf = Some(GkFrames::new(sc, &frames, &entries));
        }
        let fire: Vec<usize> = setup
            .events
            .iter()
            .filter(|e| (0..5).contains(&e.0))
            .filter_map(|e| {
                let i = frames.partition_point(|&x| x < e.1);
                (i < frames.len()).then_some(i)
            })
            .collect();
        let mut convs: Vec<(Conv, bool)> = Vec::new();
        let whole = vec![(i64::MIN, i64::MAX)];
        for r in live.values().flatten().filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            convs.push(((convert_to(r.effect_type, r.value), r.targets.clone(), whole.clone()), false));
        }
        let cond_rows = snap_rows.iter().flatten().flatten();
        let cond_rows = cond_rows.chain(snap_gk_rows.iter().flatten().flatten()).chain(member_gk.values().flatten());
        for r in cond_rows.filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            let (w, budget) = if r.gk {
                let g = gk_row(&env, r);
                (g.conv.clone(), gk_budget(&env, r, &g).is_some())
            } else if env.event_only(r.trigger) {
                // registered in the start frame, it converts the notes judged in the next frames up to the frame that
                // processes its end
                (fire.iter().map(|&i0| (i0 as i64, register_end(&frames, i0, r.act))).collect(), false)
            } else {
                (whole.clone(), false)
            };
            convs.push(((convert_to(r.effect_type, r.value), r.targets.clone(), w), budget));
        }
        convs.retain(|c| c.0.0 != -1);
        // `all`: every conversion; else without the rows with a conversion budget (their score is bounded apart)
        let reach = |j: i32, t: i64, all: bool| -> Vec<i32> {
            let mut v = vec![j];
            for ((to, targets, w), budget) in &convs {
                if (all || !budget)
                    && *to != j
                    && targets.contains(&(j as i64))
                    && !v.contains(to)
                    && w.iter().any(|&(a, b)| a < t && t <= b)
                {
                    v.push(*to);
                }
            }
            v
        };
        let reached: Vec<Vec<i32>> = entries.iter().map(|&(fi, _, j)| reach(j, fi as i64, true)).collect();
        // the judgements the score bounds read outside the conversion budgets
        let reached_v: Vec<Vec<i32>> = entries.iter().map(|&(fi, _, j)| reach(j, fi as i64, false)).collect();
        if let Some(sc) = &sched {
            env.gk = Some(GkEnv {
                missions: sc.ranges.iter().map(|r| r.mission).collect(),
                completes: sc.ranges.iter().any(|r| r.f_complete.is_some()),
                breaks: reached.iter().any(|r| r.iter().any(|&j| j == 1 || j == 2)),
            });
        }
        let mut damaging = false;
        for (e, r) in entries.iter().zip(&reached) {
            if !damage.contains_key(&(e.2 as i64)) {
                return Err(Error::Game(format!("judgement {} has no damage entry", e.2)));
            }
            if r.iter().any(|&x| damage.get(&(x as i64)).is_none_or(|&d| d > 0)) {
                damaging = true;
            }
        }
        // Without recovery and guard every life the simulation computes for a note is at most the base minus the
        // damage of the entries already filed at that point (judged in earlier frames or earlier in the same frame,
        // itself included) with chart times up to the note's: damage only lowers life, a life query folds every filed
        // command up to its time at least once (the frame cache can fold some twice), and the floor is 0. The damage
        // of an entry is at least the smallest damage of its reachable judgements.
        let dmin: Vec<i64> = reached
            .iter()
            .map(|r| r.iter().map(|&x| damage.get(&(x as i64)).copied().unwrap_or(0).max(0)).min().unwrap_or(0))
            .collect();
        let mut tr: Vec<i32> = entries.iter().map(|e| e.1.time_ms).collect();
        tr.sort_unstable();
        tr.dedup();
        let mut bit = vec![0i64; tr.len() + 1];
        let mut dead_stream = Vec::with_capacity(entries.len());
        for (e, &d) in entries.iter().zip(&dmin) {
            let r = tr.partition_point(|&x| x < e.1.time_ms) + 1;
            let mut i = r;
            while i <= tr.len() {
                bit[i] = bit[i].saturating_add(d);
                i += i & i.wrapping_neg();
            }
            let (mut sum, mut i) = (0i64, r);
            while i > 0 {
                sum = sum.saturating_add(bit[i]);
                i -= i & i.wrapping_neg();
            }
            dead_stream.push(base.saturating_sub(sum) <= 0);
        }
        // the simulation recovers by the value as a 32-bit integer; below 2^30 the sum with a life of at most twice
        // the base (at most 2^30) cannot wrap
        if all_rows().any(|r| r.effect_type == 3001 && (r.value as i32) >= 1 << 30) {
            return Err(Error::Domain("life recovery outside the modelled range".into()));
        }
        let recovery = all_rows().any(|r| r.effect_type == 3001 && (r.value as i32) > 0);
        env.life_lo = if damaging { 0 } else { base };
        env.life_hi = if recovery { 2 * base } else { base };
        let onus = settings.life_onus_factor;
        // life rigidity: every life condition any allowed card can ask is decided on [lo, hi]
        let mut groups: Vec<i64> = all_rows().flat_map(|r| [r.trigger, r.condition, r.release, r.reset]).collect();
        groups.sort_unstable();
        groups.dedup();
        let (lo, hi) = (env.life_lo, env.life_hi);
        let mut rigid = env.life_lo > 0 || onus == 1.0;
        for s in master.skill_condition_sets.iter().filter(|s| s.group != 0 && groups.binary_search(&s.group).is_ok()) {
            for &cid in &s.condition_ids {
                let Some(c) = master.skill_condition(cid) else { continue };
                let Some(&v) = c.condition_values.first() else { continue };
                let decided = match c.condition_type {
                    2001 => hi < v || lo >= v,
                    2003 => lo > v || hi <= v,
                    _ => true,
                };
                rigid &= decided;
            }
        }
        env.life_rigid = rigid;

        // monotonicity preconditions
        let positive = |x: f32| x.is_finite() && x > 0.0;
        let level = setup.params.music_level;
        let adj = settings.score_adjustment_factor;
        let mdf = get_music_score_level_factor(level);
        let cnc = setup.params.converted_note_count;
        let assist = setup.params.assist_factor;
        if !(positive(adj) && positive(mdf) && cnc > 0 && assist.is_finite() && assist >= 0.0)
            || onus.is_nan()
            || onus < 0.0
        {
            return Err(Error::Domain("live score settings are not all positive".into()));
        }
        if all_rows().any(|r| matches!(r.effect_type, 2000 | 2004) && r.value < 0) {
            return Err(Error::Domain("negative score factor".into()));
        }

        // classes
        let n = pool.members.len();
        let mut classes: Vec<Vec<Class>> = vec![Vec::new(); n];
        let mut class_of: Vec<Vec<u16>> = vec![Vec::new(); n];
        let mut member_live: Vec<Vec<LiveRow>> = vec![Vec::new(); n];
        let mut gids: HashMap<ClassKey, u32> = HashMap::new();
        let mut class_gid: Vec<Vec<u32>> = vec![Vec::new(); n];
        for &m in &members {
            let v = &pool.members[m];
            let a = attr(v);
            for r in &live[&(v.live_skill_id, v.live_skill_level)] {
                let o = env.group(r.condition, a)?;
                member_live[m].push(LiveRow { row: r.clone(), out: o, partner: None });
            }
            // rows whose conditions negate each other start at most one of the two at an event
            let lr = &mut member_live[m];
            for i in 0..lr.len() {
                for j in i + 1..lr.len() {
                    if lr[i].partner.is_none()
                        && lr[j].partner.is_none()
                        && lr[i].row.effect_type == lr[j].row.effect_type
                        && lr[i].row.act.to_bits() == lr[j].row.act.to_bits()
                        && env.negations(lr[i].row.condition, lr[j].row.condition)
                    {
                        lr[i].partner = Some(j);
                        lr[j].partner = Some(i);
                    }
                }
            }
            // the member's own Gekisou skill: the same for every snap, so it joins every class's rows (not its key)
            let mut own = Vec::new();
            if let Some(rs) = member_gk.get(&(v.gekisou_skill_id, v.gekisou_skill_level)) {
                for r in rs {
                    let (st, can_start, event_bound) = support_status(&env, r, a)?;
                    if st == Status::Active {
                        own.push(active_row(&env, r, can_start, event_bound)?);
                    }
                }
            }
            let has_gk = gk_on && v.gekisou_skill_id != 0;
            let mut keys: HashMap<ClassKey, usize> = HashMap::new();
            let mut cl = vec![Class { snaps: Vec::new(), rows: own.clone() }];
            keys.insert(Vec::new(), 0);
            let next = gids.len() as u32;
            let mut cg = vec![*gids.entry(Vec::new()).or_insert(next)];
            let mut of = Vec::with_capacity(t.snaps.len());
            for (j, per) in snap_rows.iter().enumerate() {
                let mut key: ClassKey = Vec::new();
                let mut rows = Vec::new();
                let gk_skills = if has_gk { &snap_gk_rows[j][..] } else { &[][..] };
                for (kind, skill) in per.iter().map(|x| (3, x)).chain(gk_skills.iter().map(|x| (5, x))) {
                    let mut sk = Vec::new();
                    for r in skill {
                        let (st, can_start, event_bound) = support_status(&env, r, a)?;
                        if st != Status::Active {
                            continue;
                        }
                        let cond = env.group(r.condition, a)?;
                        sk.push(RowSig {
                            trigger_type: r.trigger_type,
                            trigger: r.trigger,
                            condition: if cond.is_none_or(|c| c.decided_true()) { 0 } else { r.condition },
                            release: r.release,
                            reset: r.reset,
                            cumulative: r.cumulative,
                            effect_type: r.effect_type,
                            value: r.value,
                            act: r.act.to_bits(),
                            limit: r.limit,
                            execute_limit: r.execute_limit,
                            targets: r.targets.clone(),
                            max_value: if r.gk { r.max_value } else { 0 },
                        });
                        rows.push(active_row(&env, r, can_start, event_bound)?);
                    }
                    if !(sk.is_empty() || kind == 5 && ablated(ablate::CLASS_KEY)) {
                        key.push((kind, skill.first().map_or(0, |r| r.gate), sk));
                    }
                }
                let c = match keys.get(&key) {
                    Some(&c) => c,
                    None => {
                        rows.extend(own.iter().cloned());
                        cl.push(Class { snaps: Vec::new(), rows });
                        let next = gids.len() as u32;
                        cg.push(*gids.entry(key.clone()).or_insert(next));
                        keys.insert(key, cl.len() - 1);
                        cl.len() - 1
                    }
                };
                if cl.len() > u16::MAX as usize {
                    return Err(Error::Capacity("too many snap classes".into()));
                }
                cl[c].snaps.push(j);
                of.push(c as u16);
            }
            classes[m] = cl;
            class_of[m] = of;
            class_gid[m] = cg;
        }
        // Do not merge different members while live targets can also read tags and categories.
        // Member identity is a conservative key; completeness takes precedence over this reduction.
        let mut ids: HashMap<SimulationMemberKey, u32> = HashMap::new();
        let mut sim_id = vec![u32::MAX; n];
        for &m in &members {
            let v = &pool.members[m];
            let (gs, gl) = if gk_on { (v.gekisou_skill_id, v.gekisou_skill_level) } else { (0, 0) };
            let key = (v.live_skill_id, v.live_skill_level, v.band_id, v.card_type, v.character_id, gs, gl, m);
            let next = ids.len() as u32;
            sim_id[m] = *ids.entry(key).or_insert(next);
        }

        // per-entry coefficients, entries in chart-time order
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|&i| (entries[i].1.time_ms, i));
        let times_all: Vec<i32> = order.iter().map(|&i| entries[i].1.time_ms).collect();
        // times of the entries that break the combo whatever the conversions (Miss or Bad, nothing to reach)
        let breakers: Vec<i32> =
            order.iter().filter(|&&i| reached[i].iter().all(|&j| j < 3)).map(|&i| entries[i].1.time_ms).collect();
        let mut combo_max: Vec<f64> = Vec::new();
        let mut best_combo = 0f64;
        let life_f = if env.life_lo > 0 { 1.0 } else { (onus as f64).max(1.0) };
        let pool_life_up = all_rows().any(|r| matches!(r.effect_type, 3001 | 3003));
        let mut coef = Coef::default();
        let adj64 = adj as f64;
        let mdf64 = mdf as f64;
        let mut pre: Vec<f64> = Vec::with_capacity(order.len());
        // with Gekisou on: the Gekisou combo, luck and rank bonus factors of each entry (chart-time order)
        let gkf = match (&sched, &setup.gk) {
            (Some(sc), Some(_)) => {
                // the trigger kinds of the Gekisou score rows whose trigger time can precede the frame's
                let kinds = |ty: i64| {
                    all_rows().filter(|r| r.gk && matches!(r.effect_type, 2000 | 2001 | 2004)).any(|r| {
                        env.sets.get(&r.trigger).is_some_and(|v| {
                            v.iter()
                                .flat_map(|s| s.iter())
                                .any(|&c| master.skill_condition(c).is_some_and(|x| x.condition_type == ty))
                        })
                    })
                };
                let overrides = (kinds(7020), kinds(7005));
                // each allowed member's Gekisou combo bonus windows, one list per distinct list over its classes
                let member_cb: Vec<Vec<Vec<(i64, i64, f64)>>> = members
                    .iter()
                    .map(|&m| {
                        let mut lists: Vec<Vec<(i64, i64, f64)>> = Vec::new();
                        for c in &classes[m] {
                            let l = combo_windows(&c.rows);
                            if !l.is_empty() && !lists.contains(&l) {
                                lists.push(l);
                            }
                        }
                        lists
                    })
                    .collect();
                let current = &env.gkf.as_ref().expect("Gekisou frames").current;
                Some(GkFactors::new(
                    master, setup, sc, &entries, &order, &frames, &reached, overrides, current, &member_cb,
                )?)
            }
            _ => None,
        };
        for (pos, &i) in order.iter().enumerate() {
            let (_, n, _) = entries[i];
            // the combo a note reads counts the entries at earlier chart times since the last one that breaks it
            // (with Gekisou on, the combo a rank bonus reads can miss breaks judged later: counted from the start)
            let b = breakers.partition_point(|&x| x < n.time_ms);
            let from = if b == 0 || gkf.as_ref().is_some_and(|g| g.nobreak[pos]) {
                0
            } else {
                times_all.partition_point(|&x| x < breakers[b - 1])
            };
            let before = times_all.partition_point(|&x| x < n.time_ms) - from;
            while combo_max.len() <= before {
                let c = combo_max.len() as i32;
                let cum = combo.get_cumulative_factor(COMBO, c)?;
                let f = crate::num::min_ignoring_nan(cum, 1f32) + 1f32;
                if f.is_nan() || f < 1.0 {
                    return Err(Error::Domain("combo bonus table is not non-negative".into()));
                }
                best_combo = best_combo.max(f as f64);
                combo_max.push(best_combo);
            }
            let note_pct = *settings
                .note_factor_percent
                .get(&n.note_operate_type)
                .ok_or_else(|| Error::Game(format!("note type {} has no score percent", n.note_operate_type)))?;
            if note_pct < 0 {
                return Err(Error::Domain("negative note score percent".into()));
            }
            let mut jp = [0f64; 4];
            let mut max_jp = 0f64;
            let mut vmask = 0u8;
            for &x in &reached_v[i] {
                vmask |= 1 << x;
                let st = convert_score_type(x as i64)?;
                let p = *settings
                    .judgement_score_factor_percent
                    .get(&st)
                    .ok_or_else(|| Error::Game(format!("score type {st} has no score percent")))?;
                if p < 0 {
                    return Err(Error::Domain("negative judgement score percent".into()));
                }
                let p = p as f64 / 100.0;
                max_jp = max_jp.max(p);
                if (3..=6).contains(&x) {
                    jp[(x - 3) as usize] = p;
                }
            }
            let mut pre_e = adj64 * mdf64 * (note_pct as f64 / 100.0);
            let mut rank = 1.0;
            let mut gpool = 1.0;
            if let Some(g) = &gkf {
                if g.combo.is_some() {
                    // the candidate bound reads its own Gekisou combo factor
                    pre_e *= g.l[pos];
                    gpool = g.g[pos];
                } else {
                    pre_e *= g.g[pos] * g.l[pos];
                }
                rank = g.r[pos];
            }
            pre.push(pre_e);
            let k = pre_e * gpool * combo_max[before] / cnc as f64 * rank;
            coef.times.push(n.time_ms);
            coef.k.push(k);
            coef.max_jp.push(max_jp);
            coef.jp.push(jp);
            coef.vmask.push(vmask);
            // with no life recovery or guard among the allowed cards, the life-zero factor where life is certainly 0
            coef.z.push(if pool_life_up || !dead_stream[i] {
                assist as f64 * life_f
            } else {
                assist as f64 * onus as f64
            });
        }
        let ne = coef.times.len();
        coef.pc = vec![0f64; ne + 1];
        coef.pj = [vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1]];
        coef.pcd = vec![0f64; ne + 1];
        coef.pjd = [vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1]];
        let z_dead = assist as f64 * onus as f64;
        for e in 0..ne {
            coef.pc[e + 1] = coef.pc[e] + coef.z[e] * coef.k[e] * coef.max_jp[e];
            coef.pcd[e + 1] = coef.pcd[e] + z_dead * coef.k[e] * coef.max_jp[e];
            for j in 0..4 {
                coef.pj[j][e + 1] = coef.pj[j][e] + coef.z[e] * coef.k[e] * coef.jp[e][j];
                coef.pjd[j][e + 1] = coef.pjd[j][e] + z_dead * coef.k[e] * coef.jp[e][j];
            }
        }
        let a0 = coef.pc[ne];
        let mut jp_of = [f64::INFINITY; 7];
        for (x, slot) in jp_of.iter_mut().enumerate().skip(1) {
            if let Ok(st) = convert_score_type(x as i64)
                && let Some(&p) = settings.judgement_score_factor_percent.get(&st)
                && p >= 0
            {
                *slot = p as f64 / 100.0;
            }
        }
        let mut fine = Fine {
            raw: order.iter().map(|&i| entries[i].2 as u8).collect(),
            group: Vec::with_capacity(ne),
            pre,
            cnc: cnc as f64,
            combo_max: combo_max.clone(),
            mjp: vec![0f64; 128],
            jp4: vec![[0f64; 4]; 128],
            breaks: vec![false; 128],
            src: vec![Vec::new(); n],
            extra: vec![Default::default()],
            extra_v: vec![Default::default()],
            budget: vec![Vec::new()],
            gcombo: gkf.as_ref().and_then(|g| g.combo.clone()),
            dead: order.iter().map(|&i| dead_stream[i]).collect(),
            rank: gkf.as_ref().map(|g| g.r.clone()).unwrap_or_default(),
            nobreak: gkf.as_ref().map(|g| g.nobreak.clone()).unwrap_or_default(),
            z_dead: assist as f64 * onus as f64,
            life: vec![Vec::new(); n],
            base,
            slot_end: Vec::new(),
            slot_dmg: Vec::new(),
            ev_slot: Default::default(),
            until: Vec::new(),
            until_min: Vec::new(),
            dead_from: 0,
        };
        for e in 0..ne {
            let g = if e > 0 && coef.times[e] == coef.times[e - 1] { fine.group[e - 1] } else { e as u32 };
            fine.group.push(g);
        }
        for mask in 0..128usize {
            for (x, &p) in jp_of.iter().enumerate().skip(1) {
                if (mask >> x) & 1 == 1 {
                    fine.mjp[mask] = fine.mjp[mask].max(p);
                    if x >= 3 {
                        fine.jp4[mask][x - 3] = p;
                    }
                }
            }
            fine.breaks[mask] = (mask & 0b111_1000) == 0;
        }
        // conversion sources: the conversions of each (member, class), and what each adds at each position
        let mut fire_k: [Vec<usize>; 5] = Default::default();
        for &(idx, time) in &setup.events {
            if (0..5).contains(&idx) {
                let i = frames.partition_point(|&x| x < time);
                if i < frames.len() {
                    fire_k[idx as usize].push(i);
                }
            }
        }
        // life bound of recovering candidates
        {
            let mut after = vec![i64::MAX; entries.len()];
            for i in (0..entries.len().saturating_sub(1)).rev() {
                after[i] = after[i + 1].min(entries[i + 1].1.time_ms as i64);
            }
            fine.until = order.iter().map(|&i| after[i]).collect();
            fine.until_min = fine.until.clone();
            for e in (0..fine.until_min.len().saturating_sub(1)).rev() {
                fine.until_min[e] = fine.until_min[e].min(fine.until_min[e + 1]);
            }
            fine.dead_from = fine.dead.len();
            while fine.dead_from > 0 && fine.dead[fine.dead_from - 1] {
                fine.dead_from -= 1;
            }
            // Life frames and the frame cache (see `docs/search.md`): a life query at life frame `q` leaves the cache
            // complete up to at most `q - 1`; a command filed at a life frame `f` up to the cache folds the frames from
            // `f` to the cache once more at the next query. Queries happen at each play frame's time and at each
            // judged entry's chart time; commands are the entries' damage (filed before that entry's query) and the
            // skill events' recoveries (at the time of the frame where the event fires, after that frame's entries).
            let lmax = get_frame(setup.params.music_length_ms).wrapping_add(2);
            let lf = |ms: i32| {
                let f = get_frame(ms);
                if lmax <= f { lmax.wrapping_sub(1) } else { f }
            };
            let pf: Vec<i32> = frames.iter().map(|&t| lf(t)).collect();
            // the windows `[f, q - 1]` that a command can fold again: `f` its life frame, `q` the largest life frame
            // queried before it is filed
            let mut windows: Vec<(i32, i32)> = Vec::new();
            let mut mq_note = i32::MIN;
            for e in &entries {
                let q = if e.0 == 0 { i32::MIN } else { pf[e.0 - 1] }.max(mq_note);
                let f = lf(e.1.time_ms);
                if f < q {
                    windows.push((f, q - 1));
                }
                mq_note = mq_note.max(f);
            }
            let mut note_upto = vec![i32::MIN; frames.len()];
            let (mut m, mut q) = (i32::MIN, 0usize);
            for (i, slot) in note_upto.iter_mut().enumerate() {
                while q < entries.len() && entries[q].0 <= i {
                    m = m.max(lf(entries[q].1.time_ms));
                    q += 1;
                }
                *slot = m;
            }
            for &i1 in fire_k.iter().flatten() {
                let q = pf[i1].max(note_upto[i1]);
                if pf[i1] < q {
                    windows.push((pf[i1], q - 1));
                }
            }
            windows.sort_unstable();
            let mut runs: Vec<(i32, i32)> = Vec::new();
            for &(a, b) in &windows {
                match runs.last_mut() {
                    Some(r) if a <= r.1 => r.1 = r.1.max(b),
                    _ => runs.push((a, b)),
                }
            }
            // slot key: (first life frame, time); a run is one slot, a time outside the runs is one slot
            let key = |ms: i32| -> (i32, i64) {
                let f = lf(ms);
                let r = runs.partition_point(|x| x.1 < f);
                match runs.get(r) {
                    Some(&(a, _)) if a <= f => (a, i64::MIN),
                    _ => (f, ms as i64),
                }
            };
            let end_of = |k: (i32, i64)| -> i64 {
                if k.1 != i64::MIN {
                    return k.1;
                }
                let b = runs[runs.partition_point(|x| x.0 < k.0)].1;
                if b >= lmax.wrapping_sub(1) { i64::MAX } else { 40 * b as i64 }
            };
            let mut keys: Vec<(i32, i64)> = entries.iter().map(|e| key(e.1.time_ms)).collect();
            keys.extend(fire_k.iter().flatten().map(|&i| key(frames[i])));
            keys.sort_unstable();
            keys.dedup();
            fine.slot_end = keys.iter().map(|&k| end_of(k)).collect();
            fine.slot_dmg = vec![0i64; keys.len()];
            for (e, &d) in entries.iter().zip(&dmin) {
                let slot = keys.binary_search(&key(e.1.time_ms)).expect("slot of an entry");
                fine.slot_dmg[slot] = fine.slot_dmg[slot].saturating_add(d);
            }
            for k in 0..5 {
                fine.ev_slot[k] = fire_k[k]
                    .iter()
                    .map(|&i| {
                        let slot = keys.binary_search(&key(frames[i])).expect("slot of a recovery");
                        let times = 1 + windows.iter().filter(|w| w.0 <= pf[i] && pf[i] <= w.1).count() as i64;
                        (slot, times)
                    })
                    .collect();
            }
        }
        let ent_frame: Vec<i64> = order.iter().map(|&i| entries[i].0 as i64).collect();
        let mut sources: HashMap<ConvSource, u32> = HashMap::new();
        for &m in &members {
            let live_conv: Vec<(i32, Vec<i64>)> = member_live[m]
                .iter()
                .filter(|x| matches!(x.row.effect_type, 12006 | 13005) && x.out.is_none_or(|o| o.yes))
                .map(|x| (convert_to(x.row.effect_type, x.row.value), x.row.targets.clone()))
                .filter(|c| c.0 != -1)
                .collect();
            let live_life =
                member_live[m].iter().any(|x| matches!(x.row.effect_type, 3001 | 3003) && x.out.is_none_or(|o| o.yes));
            fine.life[m] = classes[m]
                .iter()
                .map(|c| {
                    let (mut up, mut other) = (0i64, live_life);
                    for r in c.rows.iter().filter(|r| r.can_start) {
                        match r.effect_type {
                            3001 if r.event_bound => up += (r.value as i32).max(0) as i64,
                            3001 | 3003 => other = true,
                            _ => {}
                        }
                    }
                    if other {
                        LifeKind::Other
                    } else if up > 0 {
                        LifeKind::Recovery(up)
                    } else {
                        LifeKind::None
                    }
                })
                .collect();
            let mut per = Vec::with_capacity(classes[m].len());
            for c in &classes[m] {
                let snap_conv: Vec<SnapConv> = c
                    .rows
                    .iter()
                    .filter(|r| matches!(r.effect_type, 12006 | 13005) && r.can_start)
                    .map(|r| {
                        let to = convert_to(r.effect_type, r.value);
                        let b = r.budget.map(f64::to_bits);
                        (to, r.targets.clone(), r.act.to_bits(), r.event_bound, r.gk_conv.clone(), b)
                    })
                    .filter(|c| c.0 != -1)
                    .collect();
                if live_conv.is_empty() && snap_conv.is_empty() {
                    per.push(0);
                    continue;
                }
                let key = (live_conv.clone(), snap_conv);
                if let Some(&id) = sources.get(&key) {
                    per.push(id);
                    continue;
                }
                let id = fine.extra.len() as u32;
                let mut ex: [Vec<u8>; 5] = Default::default();
                let mut ev: [Vec<u8>; 5] = Default::default();
                let mut bud: Vec<(u8, f64, Vec<u32>)> =
                    key.1.iter().filter_map(|c| c.5.map(|b| (c.0 as u8, f64::from_bits(b), Vec::new()))).collect();
                for k in 0..5 {
                    let (mut out, mut out_v) = (Vec::with_capacity(ne), Vec::with_capacity(ne));
                    for e in 0..ne {
                        let (j, fi) = (fine.raw[e] as i32, ent_frame[e]);
                        let (mut mask, mut mask_v) = (0u8, 0u8);
                        for (to, tg) in &key.0 {
                            if *to != j && tg.contains(&(j as i64)) {
                                mask |= 1 << to;
                                mask_v |= 1 << to;
                            }
                        }
                        let mut b = 0usize;
                        for (to, tg, act, eb, gw, budget) in &key.1 {
                            let seen = match gw {
                                Some(w) => w.iter().any(|&(a, b)| a < fi && fi <= b),
                                None => {
                                    !eb || fire_k[k].iter().any(|&i0| {
                                        (i0 as i64) < fi && fi <= register_end(&frames, i0, f32::from_bits(*act))
                                    })
                                }
                            };
                            let hit = *to != j && tg.contains(&(j as i64)) && seen;
                            if hit {
                                mask |= 1 << to;
                            }
                            if budget.is_some() {
                                // a budget row's frames do not depend on the position
                                if hit && k == 0 {
                                    bud[b].2.push(e as u32);
                                }
                                b += 1;
                            } else if hit {
                                mask_v |= 1 << to;
                            }
                        }
                        out.push(mask);
                        out_v.push(mask_v);
                    }
                    ex[k] = out;
                    ev[k] = out_v;
                }
                fine.extra.push(ex);
                fine.extra_v.push(ev);
                fine.budget.push(bud);
                sources.insert(key, id);
                per.push(id);
            }
            fine.src[m] = per;
        }

        // events of each position
        let mut ev_by_k: [Vec<i32>; 5] = Default::default();
        for &(idx, time) in &setup.events {
            if (0..5).contains(&idx) {
                ev_by_k[idx as usize].push(time);
            }
        }
        let exec = Exec::new(setup, &coef.times, gkf.as_ref());
        let geo = Geo { frames: &frames, times: &coef.times, exec: &exec };
        let mut contrib: Vec<Vec<[Contrib; 5]>> = vec![Vec::new(); n];
        // command and factor totals for the drift margin: per position, the largest over members and classes
        let mut cmd_k = [0f64; 5];
        let mut fac_k = [0f64; 5];
        for &m in &members {
            let mut per = Vec::with_capacity(classes[m].len());
            for c in &classes[m] {
                let mut arr: [Contrib; 5] = Default::default();
                for k in 0..5 {
                    let (w, cmds, fac, ops, spans) = windows(&geo, &member_live[m], &c.rows, &ev_by_k[k]);
                    let fac = fac.iter().copied().fold(0f64, f64::max);
                    let mut g = 0f64;
                    let mut judge = false;
                    for x in &w {
                        let (lo, hi) = (x.lo as usize, x.hi as usize);
                        g += x.note * (coef.pc[hi] - coef.pc[lo]);
                        for j in 0..4 {
                            if x.judge[j] != 0.0 {
                                judge = true;
                                g += x.judge[j] * (coef.pj[j][hi] - coef.pj[j][lo]);
                            }
                        }
                    }
                    cmd_k[k] = cmd_k[k].max(cmds);
                    fac_k[k] = fac_k[k].max(fac);
                    let cb = if fine.gcombo.is_some() { combo_windows(&c.rows) } else { Vec::new() };
                    arr[k] = Contrib { windows: w, gain: g, judge, ops, spans, budget: 0.0, cb };
                }
                per.push(arr);
            }
            contrib[m] = per;
        }
        // Conversion budgets in the linear bound: a conversion of entry `e` to `to` (not in the entry's reach of
        // `Coef`) adds at most `z k ((jp(to) - max_jp)^+ (1 + NF_e) + jp(to) JF_e)`, `NF_e` and `JF_e` the largest note
        // and `to` judgement factors any allowed performer adds at `e`, summed over the positions. A row adds at most
        // its budget's largest such terms over the entries it can convert.
        if fine.budget.iter().any(|b| !b.is_empty()) {
            let mut nfx = vec![0f64; ne];
            let mut jfx = [vec![0f64; ne], vec![0f64; ne], vec![0f64; ne], vec![0f64; ne]];
            let mut d = vec![[0f64; 5]; ne + 1];
            for k in 0..5 {
                let (mut nk, mut jk) =
                    (vec![0f64; ne], [vec![0f64; ne], vec![0f64; ne], vec![0f64; ne], vec![0f64; ne]]);
                for &m in &members {
                    for c in &contrib[m] {
                        let ws = &c[k].windows;
                        if ws.is_empty() {
                            continue;
                        }
                        for w in ws {
                            let (lo, hi) = (w.lo as usize, w.hi as usize);
                            d[lo][0] += w.note;
                            d[hi][0] -= w.note;
                            for j in 0..4 {
                                d[lo][j + 1] += w.judge[j];
                                d[hi][j + 1] -= w.judge[j];
                            }
                        }
                        let lo = ws.iter().map(|w| w.lo as usize).min().unwrap_or(0);
                        let hi = ws.iter().map(|w| w.hi as usize).max().unwrap_or(0);
                        let mut acc = [0f64; 5];
                        for e in lo..hi {
                            for (a, x) in acc.iter_mut().zip(&d[e]) {
                                *a += x;
                            }
                            nk[e] = nk[e].max(acc[0]);
                            for j in 0..4 {
                                jk[j][e] = jk[j][e].max(acc[j + 1]);
                            }
                        }
                        for x in d[lo..=hi].iter_mut() {
                            *x = [0f64; 5];
                        }
                    }
                }
                for e in 0..ne {
                    nfx[e] += nk[e].max(0.0);
                    for j in 0..4 {
                        jfx[j][e] += jk[j][e].max(0.0);
                    }
                }
            }
            let mut terms: Vec<f64> = Vec::new();
            let per_src: Vec<f64> = fine
                .budget
                .iter()
                .map(|rows| {
                    let mut sum = 0f64;
                    for (to, n, elig) in rows {
                        let to = *to as usize;
                        let jt = jp_of[to];
                        terms.clear();
                        for &e in elig {
                            let e = e as usize;
                            if coef.vmask[e] & (1 << to) != 0 {
                                continue;
                            }
                            let jf = if to >= 3 { jfx[to - 3][e] } else { 0.0 };
                            let add = (jt - coef.max_jp[e]).max(0.0) * (1.0 + nfx[e]) + jt * jf;
                            terms.push(coef.z[e] * coef.k[e] * add);
                        }
                        sum += top_sum(&mut terms, *n);
                    }
                    sum
                })
                .collect();
            for &m in &members {
                for (c, arr) in contrib[m].iter_mut().enumerate() {
                    let b = per_src[fine.src[m][c] as usize];
                    for x in arr.iter_mut() {
                        x.budget = b;
                        x.gain += b;
                    }
                }
            }
        }
        let mut global = a0;
        for k in 0..5 {
            let mut best = 0f64;
            for &m in &members {
                for c in &contrib[m] {
                    best = best.max(c[k].gain);
                }
            }
            global += best;
        }
        // drift of the factor state: every float operation on it rounds by at most 2^-24 of a value below
        // 1 + (every factor that can be active); a frame is executed at most `e_max` times up to the last note, each
        // execution applying its commands (two roundings each: state and frame diff) and each undo one more
        let e_max = exec.max as f64;
        let n_cmd: f64 = cmd_k.iter().sum();
        let f_tot: f64 = fac_k.iter().sum();
        let ops = 3.0 * e_max * n_cmd + n_cmd;
        let drift = ops * 2f64.powi(-24) * (1.0 + f_tot) * 1.01;
        // plus the rounding of the factor after the floor (assist, life)
        let mut eps = drift + 2f64.powi(-22) + CHAIN_EPS + 2f64.powi(-19);
        // with Gekisou on, the chain also multiplies by the Gekisou combo and luck factors, each computed in binary32
        let chain_extra = if gk_on { GK_CHAIN_EPS } else { 0.0 };
        if gk_on {
            eps += chain_extra;
        }
        // the class search bounds life only when some entry can read life 0 (when the fold without recoveries never
        // reaches 0, no fold with recoveries does) at a factor below `Coef::z`
        let life_bound = env.life_lo <= 0
            && (fine.dead_from < coef.times.len() || fine.zero_from([0; 5]) != i64::MAX)
            && (0..coef.times.len()).any(|e| fine.z_dead < coef.z[e]);
        let split = if life_bound { split_envelopes(&contrib, &fine, &coef) } else { Vec::new() };
        let split_budget = if life_bound {
            (0..n * 5)
                .map(|mk| {
                    let (m, k) = (mk / 5, mk % 5);
                    contrib[m]
                        .iter()
                        .enumerate()
                        .filter(|(c, _)| fine.life[m][*c] != LifeKind::Other)
                        .map(|(_, a)| a[k].budget)
                        .fold(0f64, f64::max)
                })
                .collect()
        } else {
            Vec::new()
        };
        let first_draw = match &sched {
            None => 0,
            Some(sc) => sc.first_draw(&env, all_rows()),
        };
        let sl = SnapLive {
            master,
            setup,
            classes,
            class_of,
            contrib,
            sim_id,
            class_gid,
            coef,
            fine,
            a0,
            global,
            life_bound,
            split,
            split_budget,
            eps,
            chain_extra,
            n: setup.gk.as_ref().map_or(1, |g| g.seeds.len() as i64),
            prefix_frame: first_draw,
        };
        // the score must stay far from the 32-bit range for the per-note sum to be monotone in power
        let mut u: Vec<i64> = members
            .iter()
            .map(|&m| t.a[m] + t.lead.iter().map(|row| row[m]).max().unwrap_or(0).max(0) + t.wmax[m])
            .collect();
        u.sort_unstable_by(|x, y| y.cmp(x));
        let p_max: i64 = u.iter().take(5).sum();
        if g6_trace() {
            let nb: usize = sl.fine.budget.iter().map(|b| b.len()).sum();
            let nbs = sl.fine.budget.iter().filter(|b| !b.is_empty()).count();
            let gc_on = sl.fine.gcombo.is_some();
            eprintln!(
                "G6 setup {:.1} ms budget rows {} sources {} gcombo {} cache {} ne {} nf {}",
                t_new.elapsed().as_secs_f64() * 1e3,
                nb,
                nbs,
                gc_on,
                env.gk_cache.borrow().len(),
                sl.coef.times.len(),
                frames.len()
            );
            let ne = sl.coef.times.len().max(1) as f64;
            let gsum: Vec<f64> = (0..5)
                .map(|k| {
                    members.iter().flat_map(|&m| sl.contrib[m].iter().map(move |c| c[k].gain)).fold(0f64, f64::max)
                })
                .collect();
            eprintln!(
                "G6 a0 {:.4e} global {:.4e} eps {:.4e} gains {:?} p_max {} bound {} exec_max {} kmean {:.4e}",
                sl.a0,
                sl.global,
                sl.eps,
                gsum,
                p_max,
                ub(p_max, sl.global, sl.eps),
                exec.max,
                sl.coef.k.iter().sum::<f64>() / ne
            );
        }
        if ub(p_max, sl.global, sl.eps) >= i32::MAX as i64 / 2 {
            return Err(Error::Domain("live score bound exceeds the 32-bit range".into()));
        }
        Ok(sl)
    }

    /// An upper bound of the live score of any deck with power at most `power`.
    pub fn score_bound(&self, power: i64) -> i64 {
        ub(power, self.global, self.eps).saturating_mul(self.n)
    }

    /// The number of snap classes of each allowed member `(card id, classes)`.
    pub fn class_counts(&self, pool: &Pool) -> Vec<(i64, u32)> {
        self.classes
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_empty())
            .map(|(m, c)| (pool.members[m].id, c.len() as u32))
            .collect()
    }

    /// The largest gain of member `m` at each position over its classes (`G(k, m, c)`; 0 for members that are not
    /// allowed).
    pub fn position_gains(&self, m: usize) -> [f64; 5] {
        let mut g = [0f64; 5];
        for c in &self.contrib[m] {
            for (k, x) in g.iter_mut().enumerate() {
                *x = x.max(c[k].gain);
            }
        }
        g
    }

    /// An upper bound of the live score of any deck with power at most `power` whose positions' gains sum to at
    /// most `gain`.
    pub fn gain_bound(&self, power: i64, gain: f64) -> i64 {
        ub(power, (self.a0 + gain).min(self.global), self.eps).saturating_mul(self.n)
    }

    /// The first entry (chart-time order) from which every entry reads life 0 under `life` (the number of entries
    /// when there is none).
    fn dead_start(&self, life: CandLife) -> usize {
        let f = &self.fine;
        match life {
            CandLife::NoRise => f.dead_from,
            CandLife::ZeroFrom(t0) => {
                let a = self.coef.times.partition_point(|&t| (t as i64) < t0);
                a.max(f.until_min.partition_point(|&u| u <= t0))
            }
            CandLife::Unknown => self.coef.times.len(),
        }
    }

    /// `A0` plus the gains of `parts`, with the entries from `start` on at the life-zero factor.
    fn life_sum(&self, parts: [&Contrib; 5], start: usize) -> f64 {
        self.a0_from(start) + parts.iter().map(|p| self.gain_from(p, start)).sum::<f64>()
    }

    /// At least the gain of every class of member `m` without other life-raising rows at position `k`, with the
    /// entries from `start` on at the life-zero factor (`plain`: at least the gain of every such class).
    fn split_gain(&self, m: usize, k: usize, start: usize, plain: f64) -> f64 {
        match self.split.get(m * 5 + k) {
            Some((pz, pd)) if !pz.is_empty() => {
                let ne = self.coef.times.len();
                let s = start.min(ne);
                plain.min(pz[s] + (pd[ne] - pd[s]) + self.split_budget.get(m * 5 + k).copied().unwrap_or(0.0))
            }
            _ => plain,
        }
    }

    /// `A0` with the entries from `start` on at the life-zero factor.
    fn a0_from(&self, start: usize) -> f64 {
        let c = &self.coef;
        let ne = c.times.len();
        c.pc[start.min(ne)] + (c.pcd[ne] - c.pcd[start.min(ne)])
    }

    /// The gain of one part with the entries from `start` on at the life-zero factor (at most its `gain`).
    fn gain_from(&self, part: &Contrib, start: usize) -> f64 {
        let c = &self.coef;
        let seg = |p: &[f64], d: &[f64], lo: usize, hi: usize| -> f64 {
            (p[hi.min(start)] - p[lo.min(start)]) + (d[hi.max(start)] - d[lo.max(start)])
        };
        let mut total = part.budget;
        for w in &part.windows {
            let (lo, hi) = (w.lo as usize, w.hi as usize);
            total += w.note * seg(&c.pc, &c.pcd, lo, hi);
            for j in 0..4 {
                if w.judge[j] != 0.0 {
                    total += w.judge[j] * seg(&c.pj[j], &c.pjd[j], lo, hi);
                }
            }
        }
        total
    }

    /// The margin of one candidate: as `eps`, with the drift from this candidate's own commands. Each execution of
    /// a score frame holding one of its commands rounds the state once when applying it (the state is below
    /// `1 + peak`, the largest factor total active at one time), once in the frame difference (below the factors of
    /// the windows that meet one 40 ms frame) and once in the undo; `ops` counts the executions; each factor's
    /// binary32 representation adds one more rounding.
    fn cand_eps(&self, parts: [&Contrib; 5]) -> f64 {
        let mut peak = 0f64;
        let mut peak_frame = 0f64;
        let mut n = 0f64;
        for p in parts {
            for &(a, _, _) in &p.spans {
                let (mut at, mut near) = (0f64, 0f64);
                for q in parts {
                    for &(b0, b1, g) in &q.spans {
                        if b0 <= a && a < b1 {
                            at += g;
                        }
                        if b0 <= a.saturating_add(40) && a.saturating_sub(40) <= b1 {
                            near += g;
                        }
                    }
                }
                peak = peak.max(at);
                peak_frame = peak_frame.max(near);
                n += 2.0;
            }
        }
        let e: f64 = parts.iter().map(|p| p.ops).sum();
        let drift = (e * (2.0 * (1.0 + peak) + peak_frame) + n * (1.0 + peak)) * 2f64.powi(-24) * 1.01;
        drift + 2f64.powi(-22) + CHAIN_EPS + 2f64.powi(-19) + self.chain_extra
    }

    /// Per-note bound of one candidate (conversion source `src[k]` at position `k`): the sum over the stream of each
    /// note's floored bound, with the judgements and combo breaks of the candidate's own conversions, and the
    /// life-zero factor where `life` shows that the life is 0.
    fn fine_bound(
        &self,
        power: i64,
        parts: [&Contrib; 5],
        src: [u32; 5],
        life: CandLife,
        scratch: &mut Scratch,
    ) -> i64 {
        let ne = self.coef.times.len();
        let judge = parts.iter().any(|p| p.judge);
        scratch.note.clear();
        scratch.note.resize(ne + 1, 0.0);
        if judge {
            for v in scratch.judge.iter_mut() {
                v.clear();
                v.resize(ne + 1, 0.0);
            }
        }
        for p in parts {
            for w in &p.windows {
                scratch.note[w.lo as usize] += w.note;
                scratch.note[w.hi as usize] -= w.note;
                if judge {
                    for j in 0..4 {
                        scratch.judge[j][w.lo as usize] += w.judge[j];
                        scratch.judge[j][w.hi as usize] -= w.judge[j];
                    }
                }
            }
        }
        let p = power.max(0) as f64;
        let eps = self.cand_eps(parts);
        let mut acc = 0f64;
        let mut accj = [0f64; 4];
        let mut total = 0i64;
        let mut ranked = 0f64;
        let c = &self.coef;
        let f = &self.fine;
        let extra: [Option<&[u8]>; 5] =
            std::array::from_fn(|k| (src[k] != 0).then(|| &f.extra[src[k] as usize][k][..]));
        let extra_v: [Option<&[u8]>; 5] =
            std::array::from_fn(|k| (src[k] != 0).then(|| &f.extra_v[src[k] as usize][k][..]));
        // the candidate's own Gekisou combo factors, from its combo bonus windows
        let mut gk_g = std::mem::take(&mut scratch.gk_g);
        if let Some(gc) = &f.gcombo {
            let mut ev = std::mem::take(&mut scratch.ev);
            let all: Vec<(i64, i64, f64)> = parts.iter().flat_map(|q| q.cb.iter().copied()).collect();
            bonus_at(&all, &mut ev);
            let (mut ri0, mut i, mut cur) = (usize::MAX, 0usize, 0f64);
            gc.fill(
                |ri, q| {
                    if ri != ri0 {
                        (ri0, i, cur) = (ri, 0, 0.0);
                    }
                    let t = c.times[gc.entries[ri][q] as usize] as i64;
                    while i < ev.len() && ev[i].0 <= t {
                        cur += ev[i].1;
                        i += 1;
                    }
                    cur
                },
                &mut gk_g,
                &mut scratch.sums,
            );
            scratch.ev = ev;
        }
        // the candidate's rows with a conversion budget
        scratch.brow.clear();
        for &sid in src.iter().filter(|&&x| x != 0) {
            for r in 0..f.budget[sid as usize].len() {
                scratch.brow.push((sid, r, 0));
            }
        }
        while scratch.bterms.len() < scratch.brow.len() {
            scratch.bterms.push(Vec::new());
        }
        for t in scratch.bterms.iter_mut() {
            t.clear();
        }
        // the combo is counted from the first entry at the time of the last entry that breaks it
        let (mut from, mut broke, mut group) = (0usize, None::<usize>, usize::MAX);
        for e in 0..ne {
            let gs = f.group[e] as usize;
            if gs != group {
                if let Some(b) = broke {
                    from = b;
                }
                group = gs;
            }
            let (mut mask, mut vmask) = (1u8 << f.raw[e], 1u8 << f.raw[e]);
            for x in extra.iter().flatten() {
                mask |= x[e];
            }
            for x in extra_v.iter().flatten() {
                vmask |= x[e];
            }
            if f.breaks[mask as usize] {
                broke = Some(gs);
            }
            let since = if f.nobreak.get(e).copied().unwrap_or(false) { gs } else { gs - from };
            let combo = f.combo_max.get(since).copied().unwrap_or(f64::INFINITY);
            let k = if f.gcombo.is_some() { f.pre[e] * gk_g[e] * combo / f.cnc } else { f.pre[e] * combo / f.cnc };
            acc += scratch.note[e];
            if judge {
                for j in 0..4 {
                    accj[j] += scratch.judge[j][e];
                }
            }
            let dead = match life {
                CandLife::NoRise => f.dead[e],
                CandLife::ZeroFrom(t0) => t0 <= c.times[e] as i64 && t0 < f.until[e],
                CandLife::Unknown => false,
            };
            let ze = if dead { f.z_dead } else { c.z[e] };
            let zval = |m: usize| {
                let mut v = f.mjp[m] * (1.0 + acc.max(0.0));
                if judge {
                    for j in 0..4 {
                        v += f.jp4[m][j] * accj[j].max(0.0);
                    }
                }
                let x = p * k * v * (1.0 + eps);
                let y = x.floor();
                if ze == 1.0 { y } else { (ze * y * (1.0 + 2f64.powi(-20))).floor() }
            };
            let z = zval(vmask as usize);
            let rk = if f.rank.is_empty() { 1.0 } else { f.rank[e] };
            if f.rank.is_empty() {
                total = total.saturating_add(z as i64);
            } else {
                // a rank bonus adds its percent of the range score, the sum of these entries' scores
                ranked += z * rk;
            }
            // what converting this entry would add, for each budget row that can see it
            for (bi, (sid, r, next)) in scratch.brow.iter_mut().enumerate() {
                let (to, _, elig) = &f.budget[*sid as usize][*r];
                let to = *to;
                if *next < elig.len() && elig[*next] as usize == e {
                    *next += 1;
                    if vmask & (1 << to) == 0 {
                        let d = (zval((vmask | (1 << to)) as usize) - z) * rk;
                        if d > 0.0 {
                            scratch.bterms[bi].push(d);
                        }
                    }
                }
            }
        }
        // each budget row converts at most its budget of these entries
        let mut conv = 0f64;
        for (bi, &(sid, r, _)) in scratch.brow.iter().enumerate() {
            conv += top_sum(&mut scratch.bterms[bi], f.budget[sid as usize][r].1);
        }
        scratch.gk_g = gk_g;
        if !f.rank.is_empty() {
            let v = ((ranked + conv) * (1.0 + 1e-12)).ceil();
            total = if v >= i64::MAX as f64 { i64::MAX } else { v as i64 };
        } else if conv > 0.0 {
            total = total.saturating_add(conv.min(i64::MAX as f64) as i64);
        }
        total
    }

    /// The best representative (score, power, snaps, order) of the member set `members` (slot order, leader at slot
    /// 2) whose member-only power is `fixed`; `None` when no deck of the set reaches `threshold`. The second value
    /// is true when the deadline passed (the result is then the best deck found so far).
    #[allow(clippy::too_many_arguments)]
    pub fn best<'m>(
        &self,
        pool: &Pool<'m>,
        t: &Tables<'m>,
        members: [usize; 5],
        fixed: i64,
        threshold: i64,
        deadline: Option<Instant>,
        stats: &mut PowerStats,
    ) -> Result<(Option<LeafBest>, bool), Error> {
        let cls: [&Vec<Class>; 5] = members.map(|m| &self.classes[m]);
        let wb: Vec<Vec<i64>> = (0..5)
            .map(|i| {
                cls[i]
                    .iter()
                    .enumerate()
                    .map(|(c, class)| {
                        let best = class.snaps.iter().map(|&j| t.w[members[i]][j]).max();
                        if c == 0 { best.unwrap_or(0).max(0) } else { best.unwrap_or(i64::MIN / 4) }
                    })
                    .collect()
            })
            .collect();
        let wbmax: [i64; 5] = std::array::from_fn(|i| wb[i].iter().copied().max().unwrap_or(0));
        let gmax: [[f64; 5]; 5] = std::array::from_fn(|i| {
            std::array::from_fn(|k| self.contrib[members[i]].iter().map(|c| c[k].gain).fold(0f64, f64::max))
        });
        let s1max = fixed + wbmax.iter().sum::<i64>();
        let eps = self.eps;
        // every order with its bound, best first (ties keep the lexicographic order)
        let mut orders: Vec<(i64, [usize; 5])> = Vec::with_capacity(120);
        let mut o = [0usize, 1, 2, 3, 4];
        loop {
            let s2 = self.a0 + (0..5).map(|k| gmax[o[k]][k]).sum::<f64>();
            orders.push((ub(s1max, s2, eps), o));
            if !crate::search::live::next_permutation(&mut o) {
                break;
            }
        }
        orders.sort_by_key(|x| std::cmp::Reverse(x.0));
        if orders[0].0.saturating_mul(self.n) < threshold {
            return Ok((None, false));
        }
        let mut lf = Leaf {
            sl: self,
            pool,
            t,
            members,
            fixed,
            wb,
            wbmax,
            threshold,
            best: None,
            pending: Vec::new(),
            matched: HashMap::new(),
            scratch: Scratch::default(),
            simulated: HashMap::new(),
            zero_from: HashMap::new(),
            deadline,
            timed_out: false,
            sims: 0,
            nodes: 0,
            wbn: [i64::MIN / 4; 5],
            wbo: [i64::MIN / 4; 5],
            gn: [[0f64; 5]; 5],
            go: [[0f64; 5]; 5],
            recn: [0; 5],
            has_o: [false; 5],
            n: self.n,
            seeded: HashMap::new(),
            count: LeafCounts::default(),
            diag: Vec::new(),
        };
        for i in 0..5 {
            let m = members[i];
            for c in 0..cls[i].len() {
                let w = lf.wb[i][c];
                if w <= i64::MIN / 8 {
                    continue;
                }
                let g: [f64; 5] = std::array::from_fn(|k| self.contrib[m][c][k].gain);
                match self.fine.life[m][c] {
                    LifeKind::Other => {
                        lf.has_o[i] = true;
                        lf.wbo[i] = lf.wbo[i].max(w);
                        for k in 0..5 {
                            lf.go[i][k] = lf.go[i][k].max(g[k]);
                        }
                    }
                    kind => {
                        lf.wbn[i] = lf.wbn[i].max(w);
                        for k in 0..5 {
                            lf.gn[i][k] = lf.gn[i][k].max(g[k]);
                        }
                        if let LifeKind::Recovery(r) = kind {
                            lf.recn[i] = lf.recn[i].max(r);
                        }
                    }
                }
            }
        }
        if self.life_bound && !lf.leaf_open(&gmax) {
            return Ok((None, false));
        }
        // a first candidate: the best-bound order with each slot's class of largest linearised gain
        {
            let (_, o0) = orders[0];
            let mut pos = [0usize; 5];
            for (k, &s) in o0.iter().enumerate() {
                pos[s] = k;
            }
            let s2max = self.a0 + (0..5).map(|i| gmax[i][pos[i]]).sum::<f64>();
            let mut cs = [0usize; 5];
            for i in 0..5 {
                let mut bestv = f64::MIN;
                for c in 0..cls[i].len() {
                    let g = self.contrib[members[i]][c][pos[i]].gain;
                    let v = lf.wb[i][c] as f64 * s2max + g * s1max as f64;
                    if v > bestv {
                        bestv = v;
                        cs[i] = c;
                    }
                }
            }
            if lf.matching(cs).is_none() {
                cs = [0; 5];
            }
            lf.consider(o0, cs, pos)?;
            lf.flush(true)?;
        }
        for &(bound, o) in &orders {
            if lf.timed_out || lf.sc(bound) < lf.cutoff() {
                break;
            }
            let mut pos = [0usize; 5];
            for (k, &s) in o.iter().enumerate() {
                pos[s] = k;
            }
            let rest2: [f64; 6] = {
                let mut r = [0f64; 6];
                for i in (0..5).rev() {
                    r[i] = r[i + 1] + gmax[i][pos[i]];
                }
                r
            };
            let rest1: [i64; 6] = {
                let mut r = [0i64; 6];
                for i in (0..5).rev() {
                    r[i] = r[i + 1] + lf.wbmax[i];
                }
                r
            };
            let mut rest = Rests { w: rest1, g: rest2, wn: [0; 6], gn: [0f64; 6] };
            for i in (0..5).rev() {
                rest.wn[i] = rest.wn[i + 1].saturating_add(lf.wbn[i]);
                rest.gn[i] = rest.gn[i + 1] + lf.gn[i][pos[i]];
            }
            let mut cs = [0usize; 5];
            if self.life_bound && !lf.order_open(pos, &rest) {
                continue;
            }
            lf.dfs(o, pos, 0, fixed, self.a0, &rest, false, &mut cs)?;
            lf.flush(false)?;
        }
        lf.flush(true)?;
        if g6_trace() && !lf.diag.is_empty() {
            let best = lf.best.as_ref().map_or(0, |b| b.1);
            let n = lf.diag.len();
            let fine_cut = lf.diag.iter().filter(|d| lf.sc(d.0) < best).count();
            let lin_cut = lf.diag.iter().filter(|d| lf.sc(d.1) < best).count();
            let q = |mut v: Vec<f64>, p: f64| {
                v.sort_by(|a, b| a.total_cmp(b));
                v[((v.len() - 1) as f64 * p).round() as usize]
            };
            let fb: Vec<f64> = lf.diag.iter().map(|d| d.0 as f64 * self.n as f64 / best.max(1) as f64).collect();
            let lb: Vec<f64> = lf.diag.iter().map(|d| d.1 as f64 * self.n as f64 / best.max(1) as f64).collect();
            let sb: Vec<f64> = lf.diag.iter().map(|d| d.2 as f64 * self.n as f64 / best.max(1) as f64).collect();
            let fs: Vec<f64> = lf.diag.iter().map(|d| d.0 as f64 / d.2.max(1) as f64).collect();
            eprintln!(
                "G6 leaf sims {} best {} fine_cut {} lin_cut {} fine/best {:.3} {:.3} {:.3} lin/best {:.3} {:.3} \
                 score/best {:.3} {:.3} {:.3} fine/score {:.3} {:.3} {:.3} choices {} cands {} timed_out {}",
                n,
                best,
                fine_cut,
                lin_cut,
                q(fb.clone(), 0.1),
                q(fb.clone(), 0.5),
                q(fb, 0.9),
                q(lb.clone(), 0.1),
                q(lb, 0.5),
                q(sb.clone(), 0.1),
                q(sb.clone(), 0.5),
                q(sb, 0.9),
                q(fs.clone(), 0.1),
                q(fs.clone(), 0.5),
                q(fs, 0.9),
                lf.count.class_choices,
                lf.count.candidates,
                lf.timed_out
            );
        }
        stats.orders += lf.sims;
        let c = lf.count;
        stats.seed_sims += c.seed_sims;
        stats.early_stops += c.early_stops;
        stats.seeds_saved += c.seeds_saved;
        stats.prefix_frames_saved += c.prefix_frames_saved;
        stats.bound_violations += c.violations;
        stats.class_choices += c.class_choices;
        stats.candidates += c.candidates;
        let timed_out = lf.timed_out;
        Ok((lf.best.map(|(c, score)| LeafBest { score, power: c.power, snaps: c.snaps, order: c.order }), timed_out))
    }
}

#[derive(Default)]
struct Scratch {
    note: Vec<f64>,
    judge: [Vec<f64>; 4],
    /// Gekisou combo factors, bonus events and sums of one candidate.
    gk_g: Vec<f64>,
    ev: Vec<(i64, f64)>,
    sums: Vec<f64>,
    /// Budget rows of one candidate (source, row, next eligible entry) and their conversion gains.
    brow: Vec<(u32, usize, usize)>,
    bterms: Vec<Vec<f64>>,
}

/// The state of one leaf search.
struct Leaf<'s, 'a, 'm> {
    sl: &'s SnapLive<'a>,
    pool: &'s Pool<'m>,
    t: &'s Tables<'m>,
    members: [usize; 5],
    fixed: i64,
    wb: Vec<Vec<i64>>,
    wbmax: [i64; 5],
    threshold: i64,
    /// Best candidate simulated so far and its score.
    best: Option<(Cand, i64)>,
    pending: Vec<Cand>,
    /// Constrained matching of each class assignment.
    matched: HashMap<[usize; 5], Option<Assignment>>,
    scratch: Scratch,
    /// Scores simulated in this leaf by performer identities and power.
    simulated: HashMap<([(u32, u32); 5], i64), i64>,
    /// `Fine::zero_from` of each recovery vector met in this leaf.
    zero_from: HashMap<[i64; 5], i64>,
    deadline: Option<Instant>,
    timed_out: bool,
    sims: u64,
    /// Nodes of the class search (the deadline is checked every 1024).
    nodes: u64,
    /// Per slot, over the classes whose life-raising rows are only recoveries at skill events (`n`) and over the
    /// others (`o`): the largest snap weight, the largest gain at each position, the largest recovery (`n` only),
    /// and whether there is an other class.
    wbn: [i64; 5],
    wbo: [i64; 5],
    gn: [[f64; 5]; 5],
    go: [[f64; 5]; 5],
    recn: [i64; 5],
    has_o: [bool; 5],
    /// Seeds per deck (`SnapLive::n`).
    n: i64,
    /// With Gekisou on: the seed scores simulated so far (a prefix of the seed set) by performer identities and power.
    seeded: HashMap<SeededDeckKey, Vec<i32>>,
    count: LeafCounts,
    /// TEMPORARY diagnostics: (fine bound, linear bound, first seed score) of each simulated candidate.
    diag: Vec<(i64, i64, i64)>,
}

/// Work counters of one leaf (see [`PowerStats`](crate::search::PowerStats)).
#[derive(Clone, Copy, Debug, Default)]
struct LeafCounts {
    seed_sims: u64,
    early_stops: u64,
    seeds_saved: u64,
    prefix_frames_saved: u64,
    violations: u64,
    class_choices: u64,
    candidates: u64,
    /// The largest seed score simulated in the leaf (read only by an ablation).
    observed_max: i64,
}

/// The part of the class search's life bound that depends on the chosen classes, by the recovery of a node's slot:
/// (recovery, start of the entries at the life-zero factor, rest of the bound).
struct LifeMemo {
    n: usize,
    at: [(i64, usize, f64); 8],
}

/// Remaining sums of the class search of one order, from each slot on: largest snap weights and gains over all
/// classes (`w`, `g`) and over the classes without other life-raising rows (`wn`, `gn`).
struct Rests {
    w: [i64; 6],
    g: [f64; 6],
    wn: [i64; 6],
    gn: [f64; 6],
}

impl Leaf<'_, '_, '_> {
    fn cutoff(&self) -> i64 {
        match &self.best {
            None => self.threshold,
            Some((_, s)) => self.threshold.max(*s),
        }
    }

    /// A per-seed bound scaled to the value of a deck (the sum over the seeds).
    fn sc(&self, bound: i64) -> i64 {
        bound.saturating_mul(self.n)
    }

    /// Whether a candidate with this bound, power and identity could still beat the best simulated one.
    fn could_beat(&self, c: &Cand) -> bool {
        self.reaches(c, self.sc(c.bound), false)
    }

    /// Whether a candidate whose value is at most `v` could still enter the Top-K and beat the leaf's best deck
    /// (`stop`: the early stop of the seed loop, where an ablation also drops equal values).
    fn reaches(&self, c: &Cand, v: i64, stop: bool) -> bool {
        let strict = stop && ablated(ablate::EARLY_STOP_EQUAL);
        if v < self.threshold || (strict && v == self.threshold) {
            return false;
        }
        match &self.best {
            None => true,
            Some((b, s)) => {
                v > *s || (v == *s && !strict && (c.power, b.snap_ids, b.order) > (b.power, c.snap_ids, c.order))
            }
        }
    }

    fn matching(&mut self, cs: [usize; 5]) -> Option<Assignment> {
        if let Some(x) = self.matched.get(&cs) {
            return *x;
        }
        let t = self.t;
        let sl = self.sl;
        let masks: Vec<Vec<bool>> =
            (0..5).map(|i| sl.class_of[self.members[i]].iter().map(|&c| c as usize == cs[i]).collect()).collect();
        let none_ok = cs.map(|c| c == 0);
        let m = self.members;
        let r = constrained_assignment(
            [&t.w[m[0]][..], &t.w[m[1]][..], &t.w[m[2]][..], &t.w[m[3]][..], &t.w[m[4]][..]],
            [&masks[0][..], &masks[1][..], &masks[2][..], &masks[3][..], &masks[4][..]],
            none_ok,
        );
        self.matched.insert(cs, r);
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn dfs(
        &mut self,
        o: [usize; 5],
        pos: [usize; 5],
        i: usize,
        s1: i64,
        s2: f64,
        rest: &Rests,
        other: bool,
        cs: &mut [usize; 5],
    ) -> Result<(), Error> {
        self.nodes += 1;
        if self.nodes.is_multiple_of(1024)
            && let Some(d) = self.deadline
            && Instant::now() >= d
        {
            self.timed_out = true;
        }
        if self.timed_out {
            return Ok(());
        }
        if i == 5 {
            self.count.class_choices += 1;
            return self.consider(o, *cs, pos);
        }
        let m = self.members[i];
        let ncl = self.sl.classes[m].len();
        let life_on = !other && self.sl.life_bound;
        let mut memo = LifeMemo { n: 0, at: [(0, 0, 0.0); 8] };
        for c in 0..ncl {
            let w = self.wb[i][c];
            if w <= i64::MIN / 8 {
                continue;
            }
            let g = self.sl.contrib[m][c][pos[i]].gain;
            let (n1, n2) = (s1 + w, s2 + g);
            if self.sc(ub(n1 + rest.w[i + 1], n2 + rest.g[i + 1], self.sl.eps)) < self.cutoff() {
                continue;
            }
            cs[i] = c;
            let kind = self.sl.fine.life[m][c];
            if life_on && kind != LifeKind::Other && !self.class_open(pos, i, n1, n2, rest, cs, &mut memo) {
                continue;
            }
            self.dfs(o, pos, i + 1, n1, n2, rest, other || kind == LifeKind::Other, cs)?;
        }
        Ok(())
    }

    /// The start of the entries at the life-zero factor, and the rest of the class search's life bound there (`A0`
    /// and the gains of the chosen slots before `upto - 1` split at the start, plus a bound of the split gains of the
    /// remaining slots' classes without other life-raising rows), for the chosen classes `cs[..upto]` and the largest
    /// recovery of each remaining slot. The rest is 0 when no entry is at the life-zero factor.
    fn life_rest(&mut self, pos: [usize; 5], upto: usize, cs: &[usize; 5]) -> (usize, f64) {
        let sl = self.sl;
        let mut rec = [0i64; 5];
        for j in 0..5 {
            rec[pos[j]] = if j < upto {
                match sl.fine.life[self.members[j]][cs[j]] {
                    LifeKind::Recovery(r) => r,
                    _ => 0,
                }
            } else {
                self.recn[j]
            };
        }
        // the fold is non-decreasing in each recovery, so the largest ones give the latest `t0`
        let life = if rec == [0; 5] {
            CandLife::NoRise
        } else {
            let f = &sl.fine;
            CandLife::ZeroFrom(*self.zero_from.entry(rec).or_insert_with(|| f.zero_from(rec)))
        };
        let start = sl.dead_start(life);
        if start >= sl.coef.times.len() {
            return (start, 0.0);
        }
        let mut a = sl.a0_from(start);
        for j in upto..5 {
            a += sl.split_gain(self.members[j], pos[j], start, self.gn[j][pos[j]]);
        }
        for j in 0..upto.saturating_sub(1) {
            a += sl.gain_from(&sl.contrib[self.members[j]][cs[j]][pos[j]], start);
        }
        (start, a)
    }

    /// Whether the completions of the chosen classes `cs[..upto]` (power up to `n1`, gains `n2`) with another
    /// life-raising class at some remaining slot can reach the cutoff (bounded without life).
    fn other_open(&self, pos: [usize; 5], upto: usize, n1: i64, n2: f64, rest: &Rests) -> bool {
        let cutoff = self.cutoff();
        (upto..5).any(|j| {
            self.has_o[j] && {
                let p = n1 + rest.w[upto] - self.wbmax[j] + self.wbo[j];
                let (gm, go) = (rest.g[j] - rest.g[j + 1], self.go[j][pos[j]]);
                self.sc(ub(p, n2 + rest.g[upto] - gm + go, self.sl.eps)) >= cutoff
            }
        })
    }

    /// Whether the leaf can reach the cutoff under the class search's life bound, before any order: its candidates with
    /// another life-raising class at some slot, bounded without life, and the others with the largest recovery of
    /// any slot at every position; gains by the best assignment of slots to positions (`gmax`: the largest gain of
    /// each slot at each position).
    fn leaf_open(&mut self, gmax: &[[f64; 5]; 5]) -> bool {
        let sl = self.sl;
        let cutoff = self.cutoff();
        let wsum: i64 = self.wbmax.iter().sum();
        for j in 0..5 {
            if self.has_o[j] {
                let mut g = *gmax;
                g[j] = self.go[j];
                let p = self.fixed + wsum - self.wbmax[j] + self.wbo[j];
                if self.sc(ub(p, sl.a0 + best_assignment(&g), sl.eps)) >= cutoff {
                    return true;
                }
            }
        }
        let r = self.recn.iter().copied().max().unwrap_or(0);
        let life = if r <= 0 {
            CandLife::NoRise
        } else {
            let (f, rec) = (&sl.fine, [r; 5]);
            CandLife::ZeroFrom(*self.zero_from.entry(rec).or_insert_with(|| f.zero_from(rec)))
        };
        let start = sl.dead_start(life);
        let power = self.wbn.iter().fold(self.fixed, |a, &w| a.saturating_add(w));
        if start >= sl.coef.times.len() {
            return self.sc(ub(power, sl.a0 + best_assignment(&self.gn), sl.eps)) >= cutoff;
        }
        let g: [[f64; 5]; 5] =
            std::array::from_fn(|j| std::array::from_fn(|k| sl.split_gain(self.members[j], k, start, self.gn[j][k])));
        self.sc(ub(power, sl.a0_from(start) + best_assignment(&g), sl.eps)) >= cutoff
    }

    /// Whether an order can still reach the cutoff under the class search's life bound, before any class is chosen.
    fn order_open(&mut self, pos: [usize; 5], rest: &Rests) -> bool {
        let (fixed, a0) = (self.fixed, self.sl.a0);
        if self.other_open(pos, 0, fixed, a0, rest) {
            return true;
        }
        let (start, a) = self.life_rest(pos, 0, &[0; 5]);
        let a = if start >= self.sl.coef.times.len() { a0 + rest.gn[0] } else { a };
        self.sc(ub(fixed.saturating_add(rest.wn[0]), a, self.sl.eps)) >= self.cutoff()
    }

    /// Whether a node of the class search whose slots up to `i` have classes `cs[..=i]` without other life-raising
    /// rows (power up to `n1`, gains `n2`) can still reach the cutoff: its completions with such classes only, whose
    /// recoveries are at most the largest of each slot, read life 0 from the start their fold gives (`A0` and the
    /// chosen gains split there, the remaining gains unchanged); its completions with another class at some slot are
    /// bounded without life. `memo` keeps the start and the rest of the bound by the recovery of slot `i`.
    #[allow(clippy::too_many_arguments)]
    fn class_open(
        &mut self,
        pos: [usize; 5],
        i: usize,
        n1: i64,
        n2: f64,
        rest: &Rests,
        cs: &[usize; 5],
        memo: &mut LifeMemo,
    ) -> bool {
        if self.other_open(pos, i + 1, n1, n2, rest) {
            return true;
        }
        let sl = self.sl;
        let r = match sl.fine.life[self.members[i]][cs[i]] {
            LifeKind::Recovery(r) => r,
            _ => 0,
        };
        let (start, a) = match memo.at[..memo.n].iter().find(|x| x.0 == r) {
            Some(&(_, start, a)) => (start, a),
            None => {
                let (start, a) = self.life_rest(pos, i + 1, cs);
                if memo.n < memo.at.len() {
                    memo.at[memo.n] = (r, start, a);
                    memo.n += 1;
                }
                (start, a)
            }
        };
        let power = n1.saturating_add(rest.wn[i + 1]);
        let cutoff = self.cutoff();
        if start >= sl.coef.times.len() {
            return self.sc(ub(power, n2 + rest.gn[i + 1], sl.eps)) >= cutoff;
        }
        // the split gain is at most the plain one
        let part = &sl.contrib[self.members[i]][cs[i]][pos[i]];
        if self.sc(ub(power, a + part.gain, sl.eps)) < cutoff {
            return false;
        }
        self.sc(ub(power, a + sl.gain_from(part, start), sl.eps)) >= cutoff
    }

    /// Bounds one (order, class assignment) and queues it when it can still win.
    fn consider(&mut self, o: [usize; 5], cs: [usize; 5], pos: [usize; 5]) -> Result<(), Error> {
        let Some((w, snaps_j)) = self.matching(cs) else { return Ok(()) };
        let power = self.fixed + w;
        let sl = self.sl;
        let parts: [&Contrib; 5] = std::array::from_fn(|i| &sl.contrib[self.members[i]][cs[i]][pos[i]]);
        let s2 = sl.a0 + parts.iter().map(|p| p.gain).sum::<f64>();
        if self.sc(ub(power, s2, sl.eps)) < self.cutoff() {
            return Ok(());
        }
        let life = self.cand_life(o, cs);
        let start = sl.dead_start(life);
        if start < sl.coef.times.len() && self.sc(ub(power, sl.life_sum(parts, start), sl.eps)) < self.cutoff() {
            return Ok(());
        }
        let snaps = snaps_j.map(|x| x.map(|j| self.t.snaps[j]));
        let snap_ids = snaps.map(|x| x.map_or(NO_SNAP, |i| self.pool.snaps[i].id));
        let mut scratch = std::mem::take(&mut self.scratch);
        let src: [u32; 5] = std::array::from_fn(|k| sl.fine.src[self.members[o[k]]][cs[o[k]]]);
        let bound = sl.fine_bound(power, parts, src, life, &mut scratch);
        self.scratch = scratch;
        let c = Cand { bound, power, snaps, snap_ids, order: o, classes: cs };
        if !self.could_beat(&c) {
            return Ok(());
        }
        self.count.candidates += 1;
        self.pending.push(c);
        if self.pending.len() >= FLUSH {
            self.flush(false)?;
        }
        Ok(())
    }

    /// The life bound of the candidate with order `o` and classes `cs`: from the recovery of the performer at each
    /// position, when every life-raising row is a recovery at the performer's own skill events.
    fn cand_life(&mut self, o: [usize; 5], cs: [usize; 5]) -> CandLife {
        let f = &self.sl.fine;
        let mut rec = [0i64; 5];
        for k in 0..5 {
            match f.life[self.members[o[k]]][cs[o[k]]] {
                LifeKind::None => {}
                LifeKind::Recovery(r) => rec[k] = r,
                LifeKind::Other => return CandLife::Unknown,
            }
        }
        if rec == [0; 5] {
            return CandLife::NoRise;
        }
        CandLife::ZeroFrom(*self.zero_from.entry(rec).or_insert_with(|| f.zero_from(rec)))
    }

    /// Simulates pending candidates in order of their bound: all of them that can still win when `all`, else the
    /// first one (to raise the cutoff).
    fn flush(&mut self, all: bool) -> Result<(), Error> {
        self.pending
            .sort_by(|a, b| (b.bound, b.power, a.snap_ids, a.order).cmp(&(a.bound, a.power, b.snap_ids, b.order)));
        let pending = std::mem::take(&mut self.pending);
        let mut rest = Vec::new();
        let mut done = 0usize;
        for c in pending {
            if !self.could_beat(&c) {
                continue;
            }
            if self.timed_out || (!all && done >= 1) {
                rest.push(c);
                continue;
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.timed_out = true;
                rest.push(c);
                continue;
            }
            let Some(score) = self.evaluate(&c)? else {
                done += 1;
                continue;
            };
            done += 1;
            let better = match &self.best {
                None => true,
                Some((b, s)) => (score, c.power, b.snap_ids, b.order) > (*s, b.power, c.snap_ids, c.order),
            };
            if better {
                self.best = Some((c, score));
            }
        }
        self.pending = rest;
        Ok(())
    }

    /// The value of a candidate: its simulated score, or with Gekisou on the sum of its seed scores. With Gekisou on
    /// the seeds run in order and the candidate is dropped (`None`) as soon as its partial sum plus the per-seed bound
    /// for each remaining seed cannot reach the cutoff, or when the deadline passes; a value is returned only when
    /// every seed has been simulated.
    fn evaluate(&mut self, c: &Cand) -> Result<Option<i64>, Error> {
        let sl = self.sl;
        let Some(g) = sl.setup.gk.as_ref() else { return Ok(Some(self.simulate(c)? as i64)) };
        let key: [(u32, u32); 5] = std::array::from_fn(|k| {
            let slot = c.order[k];
            let m = self.members[slot];
            (sl.sim_id[m], sl.class_gid[m][c.classes[slot]])
        });
        let mut done = self.seeded.remove(&(key, c.power)).unwrap_or_default();
        let n = g.seeds.len();
        let mut sum: i64 = done.iter().map(|&x| x as i64).sum();
        let mut runner: Option<SeedRunner> = None;
        let mut out = None;
        loop {
            let j = done.len();
            if j == n {
                out = Some(sum);
                break;
            }
            let per = if ablated(ablate::OBSERVED_MAX) { self.count.observed_max } else { c.bound };
            let most = sum.saturating_add(per.saturating_mul((n - j) as i64));
            if j > 0 && !ablated(ablate::NO_EARLY_STOP) && !self.reaches(c, most, true) {
                self.count.early_stops += 1;
                self.count.seeds_saved += (n - j) as u64;
                break;
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.timed_out = true;
                break;
            }
            if runner.is_none() {
                if done.is_empty() {
                    self.sims += 1;
                }
                runner = Some(SeedRunner::new(self, c)?);
            }
            let r = runner.as_mut().expect("runner");
            let x = r.run(sl, g.seeds[j], &mut self.count)?;
            self.count.seed_sims += 1;
            if g6_trace() && self.count.seed_sims <= 12 {
                eprintln!(
                    "G6 cand bound {} seed score {} ratio {:.3} power {}",
                    c.bound,
                    x,
                    c.bound as f64 / x.max(1) as f64,
                    c.power
                );
            }
            if x as i64 > c.bound {
                self.count.violations += 1;
            }
            if g6_trace() && j == 0 {
                let mut pos = [0usize; 5];
                for (k, &slot) in c.order.iter().enumerate() {
                    pos[slot] = k;
                }
                let s2 = sl.a0 + (0..5).map(|i| sl.contrib[self.members[i]][c.classes[i]][pos[i]].gain).sum::<f64>();
                self.diag.push((c.bound, ub(c.power, s2, sl.eps), x as i64));
            }
            self.count.observed_max = self.count.observed_max.max(x as i64);
            done.push(x);
            sum += x as i64;
        }
        self.seeded.insert((key, c.power), done);
        Ok(out)
    }

    fn simulate(&mut self, c: &Cand) -> Result<i32, Error> {
        // performers with equal identities and snap classes give the same simulation
        let sl = self.sl;
        let key: [(u32, u32); 5] = std::array::from_fn(|k| {
            let slot = c.order[k];
            let m = self.members[slot];
            (sl.sim_id[m], sl.class_gid[m][c.classes[slot]])
        });
        if let Some(&s) = self.simulated.get(&(key, c.power)) {
            return Ok(s as i32);
        }
        let s = self.simulate_deck(c)?;
        self.simulated.insert((key, c.power), s as i64);
        Ok(s)
    }

    fn simulate_deck(&mut self, c: &Cand) -> Result<i32, Error> {
        self.sims += 1;
        let (perf, power) = self.performers(c)?;
        self.sl.setup.score(self.sl.master, &perf, power)
    }

    /// The performers of a candidate, in performance order, and its power.
    fn performers(&self, c: &Cand) -> Result<(Vec<Performer>, i32), Error> {
        let pool = self.pool;
        let perf = c
            .order
            .iter()
            .map(|&slot| performer(&pool.members[self.members[slot]], c.snaps[slot].map(|s| &pool.snaps[s])))
            .collect::<Result<Vec<_>, _>>()?;
        let power = i32::try_from(c.power).map_err(|_| Error::Domain("deck power exceeds the 32-bit range".into()))?;
        Ok((perf, power))
    }
}

/// The seed runs of one candidate. Before the first frame in which a draw can happen, nothing in the live depends
/// on the seed, so the state there is the same for every seed: the first run keeps a copy of it (after checking
/// that nothing has been drawn), and every later run starts from that copy with its own seed.
struct SeedRunner {
    fresh: LiveModel,
    prefix: Option<LiveModel>,
}

impl SeedRunner {
    fn new(leaf: &Leaf, c: &Cand) -> Result<SeedRunner, Error> {
        let (perf, power) = leaf.performers(c)?;
        Ok(SeedRunner { fresh: leaf.sl.setup.gekisou_model(leaf.sl.master, &perf, power)?, prefix: None })
    }

    fn run(&mut self, sl: &SnapLive, seed: i32, count: &mut LeafCounts) -> Result<i32, Error> {
        let setup = sl.setup;
        let late = ablated(ablate::PREFIX_LATE);
        let f0 =
            if ablated(ablate::NO_PREFIX) { 0 } else { (sl.prefix_frame + late as usize).min(setup.play.frames.len()) };
        if let Some(p) = &self.prefix {
            let mut lm = p.clone();
            lm.set_seed(seed);
            count.prefix_frames_saved += f0 as u64;
            return setup.play_from(&mut lm, f0);
        }
        let mut lm = self.fresh.clone();
        lm.set_seed(seed);
        if f0 == 0 {
            return setup.play_from(&mut lm, 0);
        }
        let g = setup.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        for (f, &dt) in setup.play.frames[..f0].iter().zip(&g.dt[..f0]) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
        }
        if lm.draws() != 0 && !late {
            return Err(Error::Game("a random draw before the first frame that can draw".into()));
        }
        self.prefix = Some(lm.clone());
        setup.play_from(&mut lm, f0)
    }
}

/// An active row as the bounds read it. A cumulative note score up (2001) is bounded by its largest factor: the
/// effect value times the largest count of its cumulative condition, capped by its maximum effect value when that is
/// positive (the simulation multiplies in 128 bits and truncates to 32; the search requires the product to fit).
fn active_row(env: &Env, r: &Row, can_start: bool, event_bound: bool) -> Result<ActiveRow, Error> {
    let mut value = r.value;
    if r.effect_type == 2001 {
        let cmax = if r.cumulative == 0 {
            0
        } else {
            let c = env
                .master
                .cumulative_condition(r.cumulative)
                .ok_or_else(|| Error::Master(format!("unknown cumulative condition {}", r.cumulative)))?;
            if c.condition_values.first().is_none_or(|&n| n < 1) {
                return Err(Error::Domain("cumulative condition with a unit below 1".into()));
            }
            if c.max_cumulative_count < 1 { i32::MAX as i64 } else { c.max_cumulative_count }
        };
        let prod = r.value as i128 * cmax as i128;
        if r.value < 0 || prod > i32::MAX as i128 {
            return Err(Error::Domain("cumulative note score up outside the modelled range".into()));
        }
        value = if r.max_value > 0 && (r.max_value as i128) < prod { r.max_value } else { prod as i64 };
    }
    let w = r.gk.then(|| gk_row(env, r));
    Ok(ActiveRow {
        effect_type: r.effect_type,
        value,
        act: r.act,
        event_bound,
        can_start,
        targets: r.targets.clone(),
        churn: r.effect_type == 2001,
        churn_max: churn_max(env, r),
        gk_win: w.as_ref().map(|x| x.win.clone()),
        gk_conv: w.as_ref().map(|x| x.conv.clone()),
        budget: w.as_ref().and_then(|x| gk_budget(env, r, x)),
    })
}

/// Per play frame, what the gates and the triggers of Gekisou rows read, from the ranges' schedule.
struct GkFrames {
    times: Vec<i32>,
    /// Whether each mission's gate (1..=3) is open: a range of the mission changes state, or is the playing range.
    gate: [Vec<bool>; 3],
    /// The playing range of each frame, if any.
    current: Vec<Option<usize>>,
    /// The frames in which some range turns Start (per range) and in which some range turns Complete.
    start: Vec<Option<usize>>,
    complete: Vec<bool>,
    ranges: Vec<RangeFacts>,
    states: Vec<Vec<u8>>,
    /// The earliest trigger time of a row started in each frame: the frame before last (a range start), the chart
    /// time of a note judged in the frame (a judgement count), the start of a range in play (its combo).
    wlo: Vec<i64>,
    /// The first frame from each frame on in which some range turns Complete.
    next_complete: Vec<Option<usize>>,
    /// Frame index and raw judgement of each stream entry, in processing order.
    ent: Vec<(usize, i32)>,
}

impl GkFrames {
    fn new(sc: &Schedule, frames: &[i32], entries: &[(usize, LiveNote, i32)]) -> GkFrames {
        let nf = frames.len();
        let nr = sc.ranges.len();
        let st = |f: usize, r: usize| sc.states[f][r];
        let prev = |f: usize, r: usize| if f == 0 { 1 } else { sc.states[f - 1][r] };
        let mut order: Vec<usize> = (0..nr).filter(|&r| sc.ranges[r].f_start.is_some()).collect();
        order.sort_by_key(|&r| (sc.ranges[r].f_start, r));
        let mut current = vec![None; nf];
        let mut gate: [Vec<bool>; 3] = std::array::from_fn(|_| vec![false; nf]);
        let mut complete = vec![false; nf];
        for f in 0..nf {
            let mut c = None;
            for &r in &order {
                if sc.ranges[r].f_start.is_some_and(|s| s < f) && matches!(prev(f, r), RS_START | RS_PLAYING | RS_END) {
                    c = Some(r);
                }
            }
            for r in 0..nr {
                if st(f, r) == RS_START && prev(f, r) < RS_START {
                    c = Some(r);
                }
            }
            current[f] = c;
            for r in 0..nr {
                if st(f, r) != prev(f, r) {
                    if let Some(g) = gate.get_mut((sc.ranges[r].mission - 1) as usize) {
                        g[f] = true;
                    }
                    if st(f, r) == RS_COMPLETE {
                        complete[f] = true;
                    }
                }
            }
            if let Some(g) = c.and_then(|r| gate.get_mut((sc.ranges[r].mission - 1) as usize)) {
                g[f] = true;
            }
        }
        let mut wlo = vec![i64::MIN; nf];
        for f in 0..nf {
            if f < 2 {
                continue;
            }
            let mut lo = frames[f - 2] as i64;
            for (r, rf) in sc.ranges.iter().enumerate() {
                if (RS_START..=RS_END).contains(&st(f, r)) {
                    lo = lo.min(rf.start as i64);
                }
            }
            wlo[f] = lo;
        }
        for e in entries {
            wlo[e.0] = wlo[e.0].min(e.1.time_ms as i64);
        }
        let mut next_complete = vec![None; nf + 3];
        for f in (0..nf).rev() {
            next_complete[f] = if complete[f] { Some(f) } else { next_complete[f + 1] };
        }
        GkFrames {
            times: frames.to_vec(),
            gate,
            current,
            start: sc.ranges.iter().map(|r| r.f_start).collect(),
            complete,
            ranges: sc.ranges.clone(),
            states: sc.states.clone(),
            wlo,
            next_complete,
            ent: entries.iter().map(|e| (e.0, e.2)).collect(),
        }
    }

    fn gate_open(&self, gate: i64, f: usize) -> bool {
        match gate {
            MISSION_ALL => true,
            1..=3 => self.gate[(gate - 1) as usize][f],
            _ => false,
        }
    }

    /// The frames in which a positive condition can hold (`None`: any frame).
    fn cond_frames(&self, env: &Env, cid: i64, gate: i64) -> Option<Vec<bool>> {
        let c = env.master.skill_condition(cid)?;
        if !c.is_positive {
            return None;
        }
        let nf = self.times.len();
        let missions = || -> Vec<i64> {
            c.condition_target_ids
                .iter()
                .filter_map(|&t| env.master.skill_target(t))
                .filter(|t| t.skill_target_type == 5 && t.gekisou_mission_type != 0)
                .map(|t| t.gekisou_mission_type)
                .collect()
        };
        let matches = |ms: &[i64], m: i64| ms.is_empty() || ms.contains(&MISSION_ALL) || ms.contains(&m);
        let luck: Vec<&RangeFacts> = self.ranges.iter().filter(|r| r.mission == MISSION_LUCK).collect();
        let mut out = vec![false; nf];
        match c.condition_type {
            7010 => {
                let ms = missions();
                for (r, rf) in self.ranges.iter().enumerate() {
                    if let (Some(f), true) = (self.start[r], matches(&ms, rf.mission)) {
                        out[f] = true;
                    }
                }
            }
            7013 => out.clone_from(&self.complete),
            7005 => {
                for f in 0..nf {
                    out[f] = self.current[f].is_some();
                }
            }
            7000 => {
                for f in 1..nf {
                    out[f] = self.ranges.iter().enumerate().any(|(r, rf)| {
                        rf.mission == MISSION_LUCK && (RS_START..=RS_END).contains(&self.states[f - 1][r])
                    });
                }
            }
            7020 => {
                // the checker keeps a range until it sees it complete: from the first start of a matching range on
                let ms = missions();
                let first = self.ranges.iter().zip(&self.start).filter(|(r, _)| matches(&ms, r.mission));
                if let Some(f0) = first.filter_map(|(_, s)| *s).min() {
                    out[f0..].fill(true);
                }
            }
            7021 => {
                // the rush flag is set only while a luck range plays and cleared when this checker sees a luck range
                // complete, which a luck (or any) gate always does
                for r in &luck {
                    let Some(f0) = r.f_start else { continue };
                    let f1 = if gate == MISSION_LUCK || gate == MISSION_ALL {
                        r.f_complete.map_or(nf, |x| x + 1)
                    } else {
                        nf
                    };
                    out[f0..f1.min(nf)].fill(true);
                }
            }
            _ => return None,
        }
        Some(out)
    }

    /// The frames in which a condition group can hold (`None`: any frame): the union over its sets of the
    /// intersection over their conditions.
    fn group_frames(&self, env: &Env, gid: i64, gate: i64) -> Option<Vec<bool>> {
        let sets = env.sets.get(&gid)?;
        let mut any: Option<Vec<bool>> = Some(vec![false; self.times.len()]);
        for s in sets {
            let mut and: Option<Vec<bool>> = None;
            for &cid in s.iter() {
                if let Some(v) = self.cond_frames(env, cid, gate) {
                    and = Some(match and {
                        None => v,
                        Some(a) => a.iter().zip(&v).map(|(x, y)| *x && *y).collect(),
                    });
                }
            }
            match (and, any.as_mut()) {
                (None, _) => return None,
                (Some(a), Some(u)) => {
                    for (x, y) in u.iter_mut().zip(&a) {
                        *x |= *y;
                    }
                }
                (Some(_), None) => {}
            }
        }
        any
    }

    /// The frames in which a Gekisou row can start (its gate open and its trigger possible), and the frames in which
    /// its trigger is possible.
    fn starts(&self, env: &Env, r: &Row) -> (Vec<usize>, Vec<bool>) {
        let nf = self.times.len();
        let p = self.group_frames(env, r.trigger, r.gate).unwrap_or_else(|| vec![true; nf]);
        let s = (0..nf).filter(|&f| p[f] && self.gate_open(r.gate, f)).collect();
        (s, p)
    }

    /// The first frame from `f` on in which some range turns Complete.
    fn completion_from(&self, f: usize) -> Option<usize> {
        if g6_off(4) {
            return (f..self.times.len()).find(|&x| self.complete[x]);
        }
        self.next_complete.get(f).copied().flatten()
    }

    /// `end_of` for each start frame `s` (frames where the trigger is possible: `p`).
    fn ends(&self, env: &Env, r: &Row, s: &[usize], p: &[bool]) -> Vec<(i64, i64)> {
        let nf = self.times.len();
        // a sustained effect ends in the first open-gate frame after its start whose trigger fails
        let fail: Vec<Option<usize>> = if r.trigger_type == 2 && !g6_off(4) {
            let mut v = vec![None; nf];
            let mut next = None;
            for x in (0..nf).rev() {
                v[x] = next;
                if self.gate_open(r.gate, x) && !p[x] {
                    next = Some(x);
                }
            }
            v
        } else {
            Vec::new()
        };
        s.iter().map(|&f| self.end_of(env, r, f, p, fail.get(f).copied())).collect()
    }

    /// Whether the row's release group is exactly one positive range-complete condition.
    fn released_on_complete(env: &Env, r: &Row) -> bool {
        let Some(sets) = env.sets.get(&r.release) else { return false };
        let ids: Vec<i64> = sets
            .iter()
            .flat_map(|s| s.iter().copied())
            .filter(|&c| env.master.skill_condition(c).is_none_or(|x| x.condition_type != 0))
            .collect();
        ids.len() == 1
            && sets.iter().filter(|s| !s.is_empty()).count() == 1
            && env.master.skill_condition(ids[0]).is_some_and(|c| c.condition_type == 7013 && c.is_positive)
    }

    /// For an execution started in frame `f`: the latest time its factor can end (exclusive; `i64::MAX`: never)
    /// and the index of the last frame whose judgements its conversion can see.
    fn end_of(&self, env: &Env, r: &Row, f: usize, p: &[bool], fail: Option<Option<usize>>) -> (i64, i64) {
        let nf = self.times.len();
        let at = |x: Option<usize>| x.map_or((i64::MAX, i64::MAX), |i| (self.times[i] as i64, i as i64));
        if r.trigger_type == 2 {
            // a sustained effect ends in the first open-gate frame whose trigger fails
            let x = match fail {
                Some(x) => x,
                None => (f + 1..nf).find(|&x| self.gate_open(r.gate, x) && !p[x]),
            };
            return at(x);
        }
        let timed = if r.act > 0.0 || r.act.is_nan() || r.release == 0 {
            (frame_end(&self.times, self.times[f], f, r.act), register_end(&self.times, f, r.act))
        } else {
            (i64::MAX, i64::MAX)
        };
        if r.release != 0 && Self::released_on_complete(env, r) {
            // the release is asked from the second frame after the start on
            let rel = at(self.completion_from(f + 2));
            return (timed.0.min(rel.0), timed.1.min(rel.1));
        }
        if r.release != 0 && r.act <= 0.0 {
            return (i64::MAX, i64::MAX);
        }
        timed
    }
}

/// What the bounds read of a Gekisou row's timing: its factor windows in chart time, each `(start, end, concurrent
/// executions)`, the play-frame index ranges `(a, b]` whose judgements its conversion can see, and the frames in
/// which it can start.
struct GkRowWin {
    win: Vec<(i64, i64, f64)>,
    conv: Vec<(i64, i64)>,
    starts: Vec<usize>,
}

/// The timing of a Gekisou row (it reads only the trigger, the trigger type, the gate, the activation time and the
/// release), computed once per search. Every frame in which the row can start gives one factor window from the
/// earliest trigger time there to its latest end; with many such frames, one window over all of them with at most
/// five executions (the updaters of one effect), or one for a sustained effect. Registered in a frame in which it
/// can start, a conversion converts the notes judged in the next frames up to the frame that processes its end.
fn gk_row(env: &Env, r: &Row) -> Rc<GkRowWin> {
    let key = (r.trigger, r.trigger_type, r.gate, r.act.to_bits(), r.release);
    if !g6_off(4)
        && let Some(w) = env.gk_cache.borrow().get(&key)
    {
        return w.clone();
    }
    let w = Rc::new(gk_row_timing(env, r));
    env.gk_cache.borrow_mut().insert(key, w.clone());
    w
}

fn gk_row_timing(env: &Env, r: &Row) -> GkRowWin {
    let Some(g) = &env.gkf else {
        return GkRowWin {
            win: vec![(i64::MIN, i64::MAX, POOL)],
            conv: vec![(i64::MIN, i64::MAX)],
            starts: Vec::new(),
        };
    };
    let (s, p) = g.starts(env, r);
    let ends = g.ends(env, r, &s, &p);
    // the frame ranges are only asked whether some range contains a frame: keep their union
    let mut all: Vec<(i64, i64)> = s.iter().zip(&ends).map(|(&f, e)| (f as i64, e.1)).collect();
    all.sort_unstable();
    let mut conv: Vec<(i64, i64)> = Vec::with_capacity(all.len());
    for (a, b) in all {
        match conv.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => conv.push((a, b)),
        }
    }
    let spans: Vec<(i64, i64)> = s.iter().zip(&ends).map(|(&f, e)| (g.wlo[f], e.0)).collect();
    let win = if spans.is_empty() {
        Vec::new()
    } else if r.trigger_type == 1 && spans.len() <= 8 {
        spans.iter().map(|&(a, b)| (a, b, 1.0)).collect()
    } else {
        let a = spans.iter().map(|x| x.0).min().unwrap_or(i64::MIN);
        let b = spans.iter().map(|x| x.1).max().unwrap_or(i64::MAX);
        let mult = if r.trigger_type == 2 { 1.0 } else { POOL.min(spans.len() as f64) };
        vec![(a, b, mult)]
    };
    GkRowWin { win, conv, starts: s }
}

/// The most judgements a one-shot Gekisou conversion row can convert in the play, when that is below the number of
/// entries it can see (`None` otherwise). One execution converts at most `limit` judgements and then unregisters;
/// the effect executes at most once per frame, only in frames in which it can start, and with an execute limit at
/// most that many times between two frames in which its reset can hold (the reset is asked, with the gate open,
/// before the frame's triggers).
fn gk_budget(env: &Env, r: &Row, w: &GkRowWin) -> Option<f64> {
    if g6_off(1) || !matches!(r.effect_type, 12006 | 13005) || r.trigger_type != 1 || r.limit <= 0 {
        return None;
    }
    let key = (
        (r.trigger, r.trigger_type, r.gate, r.act.to_bits(), r.release),
        (r.effect_type, r.value, r.limit, r.execute_limit, r.reset),
        r.targets.clone(),
    );
    if let Some(&b) = env.budget_cache.borrow().get(&key) {
        return b;
    }
    let b = gk_budget_uncached(env, r, w);
    env.budget_cache.borrow_mut().insert(key, b);
    b
}

fn gk_budget_uncached(env: &Env, r: &Row, w: &GkRowWin) -> Option<f64> {
    let g = env.gkf.as_ref()?;
    let mut execs = w.starts.len() as f64;
    if r.execute_limit > 0 {
        let periods = if r.reset == 0 {
            usize::from(!w.starts.is_empty())
        } else {
            match g.group_frames(env, r.reset, r.gate) {
                None => w.starts.len(),
                Some(rs) => {
                    let (mut n, mut period, mut last, mut si) = (0usize, 0usize, None, 0usize);
                    for (f, &can) in rs.iter().enumerate() {
                        if can && g.gate_open(r.gate, f) {
                            period += 1;
                        }
                        while si < w.starts.len() && w.starts[si] == f {
                            if last != Some(period) {
                                n += 1;
                                last = Some(period);
                            }
                            si += 1;
                        }
                    }
                    n
                }
            }
        };
        execs = execs.min(r.execute_limit as f64 * periods as f64);
    }
    let n = r.limit as f64 * execs;
    let to = convert_to(r.effect_type, r.value);
    let seen = g
        .ent
        .iter()
        .filter(|&&(fi, j)| {
            j != to && r.targets.contains(&(j as i64)) && w.conv.iter().any(|&(a, b)| a < fi as i64 && fi as i64 <= b)
        })
        .count();
    (to != -1 && n < seen as f64).then_some(n)
}

/// The Gekisou combo bonus windows of a class's rows: `(start, end, bonus)` in chart time, each the combo count a
/// judgement adds while it runs (closed intervals: a bonus filed at a time applies to the judgements from that time
/// on and until its end is filed).
fn combo_windows(rows: &[ActiveRow]) -> Vec<(i64, i64, f64)> {
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.effect_type == 12000 && r.can_start && r.value > 0) {
        for &(a, b, mult) in r.gk_win.as_deref().unwrap_or(&[(i64::MIN, i64::MAX, POOL)]) {
            out.push((a, b, r.value as f64 * mult));
        }
    }
    out
}

/// The latest finish of a one-shot effect started in frame `i0` with a trigger time at most `exec` (see
/// `Geo::end`, without extensions): `i64::MAX` when no frame is late enough to end it.
fn frame_end(frames: &[i32], exec: i32, i0: usize, act: f32) -> i64 {
    let n = frames.len();
    let next = if i0 + 1 < n { Some(frames[i0 + 1] as i64) } else { None };
    if act.is_nan() || act <= 0.0 {
        return next.unwrap_or(i64::MAX);
    }
    let dur = act * 1000f32;
    let last = frames[n - 1];
    match next {
        Some(t1) if dur < last.wrapping_sub(exec) as f32 => (exec as i64 + crate::num::ceil_to_i32(dur) as i64).max(t1),
        _ => i64::MAX,
    }
}

/// The most factor changes of one execution of a cumulative note score up: its value is `min(effect value *
/// count, max)`, so once the count reaches `ceil(max / value)` it stops changing. A judgement count only grows while
/// the effect runs; a combo count grows within a range unless a Miss or a Bad is reachable, and restarts with each
/// range.
fn churn_max(env: &Env, r: &Row) -> Option<f64> {
    if r.effect_type != 2001 || r.value <= 0 || r.max_value <= 0 {
        return None;
    }
    let steps = ((r.max_value + r.value - 1) / r.value) as f64 + 1.0;
    let c = env.master.cumulative_condition(r.cumulative)?;
    match c.condition_type {
        1000 => Some(steps),
        7001 => {
            let g = env.gk.as_ref()?;
            if g.breaks { None } else { Some(steps * (g.missions.len() as f64 + 1.0)) }
        }
        _ => None,
    }
}

/// An integer live setting.
fn int_setting(master: &Master, key: &str) -> Result<i64, Error> {
    let v = master.live_setting(key).ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
    v.trim().parse::<i64>().map_err(|_| Error::Master(format!("MasterLiveSettings {key} is not an integer")))
}

/// The song's mission pattern (as the rank bonus table keys it): 0 when a mission is missing, 1 all the same, 2 all
/// different, 3 otherwise.
fn mission_pattern(m: &[i64]) -> i64 {
    let (a, b, c) = (m.first().copied().unwrap_or(0), m.get(1).copied().unwrap_or(0), m.get(2).copied().unwrap_or(0));
    if a == 0 || b == 0 || c == 0 {
        return 0;
    }
    if a == b {
        return if a == c { 1 } else { 3 };
    }
    if b != c && a != c {
        return 2;
    }
    3
}

/// One Gekisou range of a live and its schedule.
#[derive(Clone, Copy, Debug)]
struct RangeFacts {
    start: i32,
    end: i32,
    mission: i64,
    /// Rank bonus percent (a solo player is rank 1).
    pct: i64,
    /// The first play frame in which the range is in the state Start (its fever turned on), Complete (its rank bonus
    /// is confirmed there) and Finish.
    f_start: Option<usize>,
    f_complete: Option<usize>,
    f_finish: Option<usize>,
}

/// The schedule of the Gekisou ranges: the state of every range after each play frame. The range state machine reads
/// only the frame times, the delta times and the fevers, so the schedule is the same for every deck; it is taken from
/// a run of the live without skills.
struct Schedule {
    states: Vec<Vec<u8>>,
    ranges: Vec<RangeFacts>,
}

impl Schedule {
    fn new(master: &Master, setup: &FullSetup, g: &GkPlay) -> Result<Schedule, Error> {
        let perf = vec![Performer::default(); 5];
        let params = LiveParams { total_power: 0, ..setup.params };
        let mut lm = LiveModel::new_gekisou(master, &perf, &setup.notes, &setup.events, params, &g.setup)?;
        let mut states = Vec::with_capacity(setup.play.frames.len());
        for (f, &dt) in setup.play.frames.iter().zip(&g.dt) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
            states.push(lm.gekisou_ranges().iter().map(|r| r.state).collect::<Vec<u8>>());
        }
        let pattern = mission_pattern(&g.setup.missions);
        let mut ranges = Vec::with_capacity(g.setup.fevers.len());
        for (i, &(start, end)) in g.setup.fevers.iter().enumerate() {
            let first = |s: u8| states.iter().position(|x: &Vec<u8>| x.get(i).is_some_and(|&v| v >= s));
            let mut pct = 0;
            for r in &master.gekisou_ranking_score_bonuses {
                if r.mission_pattern == pattern && r.count == i as i64 + 1 && r.rank == 1 {
                    pct = r.score_bonus_percent;
                }
            }
            let mission = *g.setup.missions.get(i).ok_or_else(|| Error::Input("fewer missions than fevers".into()))?;
            ranges.push(RangeFacts {
                start,
                end,
                mission,
                pct,
                f_start: first(RS_START),
                f_complete: first(RS_COMPLETE),
                f_finish: first(RS_FINISH),
            });
        }
        Ok(Schedule { states, ranges })
    }

    /// A play frame no later than the first one in which a deck of these rows can draw a random number: the first
    /// frame of a luck range in the state Start (its lottery), and for a row with a probability condition the first
    /// frame in which it can be asked (a Gekisou row gated by a mission only once some range has left Wait).
    fn first_draw<'r>(&self, env: &Env, rows: impl Iterator<Item = &'r Row>) -> usize {
        let nf = self.states.len();
        let mut f0 =
            self.ranges.iter().filter(|r| r.mission == MISSION_LUCK).filter_map(|r| r.f_start).min().unwrap_or(nf);
        let awake = self.states.iter().position(|s| s.iter().any(|&x| x >= RS_STANDBY)).unwrap_or(nf);
        for r in rows {
            if [r.trigger, r.condition, r.release, r.reset].iter().any(|&g| env.draws(g)) {
                f0 = f0.min(if r.gk && r.gate != MISSION_ALL { awake } else { 0 });
            }
        }
        f0
    }
}

/// What a Gekisou combo factor reads, entries in chart-time order. The factor of a note is `min(table(c), 1) + 1`,
/// `c` the combo count of the first combo range containing its chart time as the range's recount left it at the last
/// of the range's judged notes with a chart time up to the end of the score frame before the note's (the range's
/// history is searched by chart time, which finds that note when the range's notes are judged in chart-time order).
/// Each judgement adds at most `floor(1 + the running combo bonuses)` to the count, so `c` is at most the sum of that
/// over the range's entries up to that time; the table lookup returns the value of some threshold at most `c`.
#[derive(Clone, Debug)]
struct GkCombo {
    /// Per entry: its combo range (index into the ranges) and the number of that range's entries (chart order) with
    /// a chart time up to the end of the previous score frame.
    range: Vec<Option<u32>>,
    upto: Vec<u32>,
    /// Per range: its entries in chart order (empty for a range that is not a combo range), and whether its entries
    /// are judged in chart-time order (else its notes take `gmax`).
    entries: Vec<Vec<u32>>,
    ordered: Vec<bool>,
    /// Table thresholds in ascending order, each with the largest factor at a count from 0 up to it.
    tab: Vec<(i64, f64)>,
    gmax: f64,
}

impl GkCombo {
    /// The largest factor at a count up to `c`.
    fn factor(&self, c: f64) -> f64 {
        let i = self.tab.partition_point(|x| (x.0 as f64) <= c);
        if i == 0 { 1.0 } else { self.tab[i - 1].1 }
    }

    /// The factor bound of every entry, from the largest running combo bonus at the chart time of each entry of a
    /// combo range (`bonus(range, position in its entries)`); `sums` is scratch.
    fn fill(&self, mut bonus: impl FnMut(usize, usize) -> f64, out: &mut Vec<f64>, sums: &mut Vec<f64>) {
        let ne = self.range.len();
        out.clear();
        out.resize(ne, 1.0);
        sums.clear();
        let mut at = Vec::with_capacity(self.entries.len());
        for (ri, list) in self.entries.iter().enumerate() {
            at.push(sums.len());
            if !self.ordered[ri] {
                continue;
            }
            let mut acc = 0f64;
            sums.push(0.0);
            for q in 0..list.len() {
                acc += (1.0 + bonus(ri, q).max(0.0)).floor();
                sums.push(acc);
            }
        }
        for e in 0..ne {
            if let Some(ri) = self.range[e] {
                let ri = ri as usize;
                out[e] = if self.ordered[ri] { self.factor(sums[at[ri] + self.upto[e] as usize]) } else { self.gmax };
            }
        }
    }
}

/// The running combo bonus of a list of bonus windows at each time of a non-decreasing sequence (`windows`: `(start,
/// end, bonus)`, closed; `ev` is scratch).
fn bonus_at(windows: &[(i64, i64, f64)], ev: &mut Vec<(i64, f64)>) {
    ev.clear();
    for &(a, b, v) in windows {
        ev.push((a, v));
        ev.push((b.saturating_add(1), -v));
    }
    ev.sort_by_key(|x| x.0);
}

/// The Gekisou factors of the per-entry bound (entries in chart-time order) and what the execution count reads.
struct GkFactors {
    /// Gekisou combo factor bound `G_e` of every deck (from the largest combo bonuses of any five allowed members),
    /// luck factor bound `L_e`, and `R_e`: 1 plus the rank bonus percents (/ 100) of the completing ranges whose
    /// range score contains the entry.
    g: Vec<f64>,
    l: Vec<f64>,
    r: Vec<f64>,
    /// Entries of a range score whose confirmation can precede the judgement of an earlier combo break.
    nobreak: Vec<bool>,
    /// Per play frame: the earliest time a command filed in the frame can land at (besides the previous frame's time
    /// and the chart times of the frame's notes).
    exec_lo: Vec<i32>,
    /// Per completing range `(start, time of its confirmation frame)`: the score frames after the start's up to the
    /// confirmation's are undone and executed once more.
    confirm: Vec<(i32, i32)>,
    /// With a combo range: what a candidate's own Gekisou combo factor reads (`g` then bounds it for every deck).
    combo: Option<GkCombo>,
}

impl GkFactors {
    #[allow(clippy::too_many_arguments)]
    fn new(
        master: &Master,
        setup: &FullSetup,
        sc: &Schedule,
        entries: &[(usize, LiveNote, i32)],
        order: &[usize],
        frames: &[i32],
        reached: &[Vec<i32>],
        overrides: (bool, bool),
        current: &[Option<usize>],
        member_cb: &[Vec<Vec<ComboBonusRow>>],
    ) -> Result<GkFactors, Error> {
        let ne = order.len();
        let times: Vec<i32> = order.iter().map(|&i| entries[i].1.time_ms).collect();
        let ranges = &sc.ranges;
        // Gekisou combo: min(table, 1) + 1 at the range's combo, inside a combo range
        let table = ComboTable::from_master(master)?;
        let mut gmax = 1f32;
        if let Some(Some(cu)) = table.cumulatives.as_ref().and_then(|c| c.get(GEKISOU_COMBO as usize)) {
            for &c in cu {
                let f = crate::num::min_ignoring_nan(c, 1f32) + 1f32;
                if f.is_nan() || f < 0.0 {
                    return Err(Error::Domain("Gekisou combo bonus table is not non-negative".into()));
                }
                gmax = gmax.max(f);
            }
        }
        let gmax = if ablated(ablate::GEKISOU_COMBO) { 1.0 } else { gmax as f64 };
        let combo: Vec<(i32, i32)> =
            ranges.iter().filter(|r| r.mission == MISSION_COMBO).map(|r| (r.start, r.end)).collect();
        let mut gv: Vec<f64> =
            times.iter().map(|&t| if combo.iter().any(|&(a, b)| a <= t && t <= b) { gmax } else { 1.0 }).collect();
        let gcombo = if combo.is_empty() || ablated(ablate::GEKISOU_COMBO) || g6_off(2) {
            None
        } else {
            let mut tab: Vec<(i64, f64)> = Vec::new();
            if let (Some(Some(th)), Some(Some(cu))) = (
                table.thresholds.as_ref().and_then(|x| x.get(GEKISOU_COMBO as usize)),
                table.cumulatives.as_ref().and_then(|x| x.get(GEKISOU_COMBO as usize)),
            ) {
                for (&t, &c) in th.iter().zip(cu) {
                    tab.push((t as i64, (crate::num::min_ignoring_nan(c, 1f32) + 1f32) as f64));
                }
            }
            tab.sort_by_key(|a| a.0);
            let mut m = 1f64;
            for x in tab.iter_mut() {
                m = m.max(x.1);
                x.1 = m;
            }
            let nr = ranges.len();
            let range: Vec<Option<u32>> = times
                .iter()
                .map(|&t| {
                    (0..nr).find(|&r| ranges[r].mission == MISSION_COMBO && ranges[r].start <= t && t <= ranges[r].end)
                })
                .map(|r| r.map(|r| r as u32))
                .collect();
            let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nr];
            let mut ordered = vec![false; nr];
            for (r, rf) in ranges.iter().enumerate() {
                if rf.mission != MISSION_COMBO {
                    continue;
                }
                let inr = |t: i32| rf.start <= t && t <= rf.end;
                lists[r] = (0..ne).filter(|&q| inr(times[q])).map(|q| q as u32).collect();
                let proc: Vec<i32> = entries.iter().map(|e| e.1.time_ms).filter(|&t| inr(t)).collect();
                ordered[r] = proc.windows(2).all(|w| w[0] <= w[1]);
            }
            let upto: Vec<u32> = (0..ne)
                .map(|e| match range[e] {
                    None => 0,
                    Some(r) => {
                        let prev_end = get_frame(times[e]).wrapping_mul(40).wrapping_sub(40);
                        lists[r as usize].partition_point(|&q| times[q as usize] <= prev_end) as u32
                    }
                })
                .collect();
            let gc = GkCombo { range, upto, entries: lists, ordered, tab, gmax };
            // every deck: the running bonus at a time is at most the sum of the five largest members' there
            let mut sums = Vec::new();
            gc.fill(
                |ri, q| {
                    let t = times[gc.entries[ri][q] as usize] as i64;
                    let mut vals: Vec<f64> = member_cb
                        .iter()
                        .map(|lists| {
                            lists
                                .iter()
                                .map(|l| l.iter().filter(|w| w.0 <= t && t <= w.1).map(|w| w.2).sum::<f64>())
                                .fold(0f64, f64::max)
                        })
                        .collect();
                    vals.sort_by(|a, b| b.total_cmp(a));
                    vals.iter().take(5).sum()
                },
                &mut gv,
                &mut sums,
            );
            Some(gc)
        };

        // luck: the rush bonus of the luck ranges
        let rush = int_setting(master, "gekisou_luck_rush_score_bonus_percent")?;
        if !(0..=1 << 24).contains(&rush) {
            return Err(Error::Domain("luck rush bonus outside the modelled range".into()));
        }
        let mut lv = vec![1.0; ne];
        let luck: Vec<&RangeFacts> =
            ranges.iter().filter(|r| r.mission == MISSION_LUCK && r.f_start.is_some()).collect();
        if !luck.is_empty() {
            let cross = luck_crossings(frames, entries, sc);
            if rush.saturating_mul(cross) > 100 {
                return Err(Error::Domain("the luck bonus can turn negative on this stream".into()));
            }
            let span = |r: &RangeFacts| (r.f_start.unwrap_or(usize::MAX), r.f_finish.unwrap_or(usize::MAX));
            let leak = luck.iter().enumerate().any(|(i, a)| {
                luck[i + 1..].iter().any(|b| {
                    let (x, y) = (span(a), span(b));
                    x.0 <= y.1 && y.0 <= x.1
                })
            });
            let count = if leak { 1 << 20 } else { 1 + cross };
            let f = 100i64.saturating_add(rush.saturating_mul(count)).min(200) as f64 / 100.0;
            if !ablated(ablate::LUCK) {
                for (p, &t) in times.iter().enumerate() {
                    let inside = if leak {
                        luck.iter().any(|r| r.start <= t)
                    } else {
                        luck.iter().any(|r| {
                            let fin = r.f_finish.map_or(i64::MAX, |x| frames[x] as i64);
                            r.start <= t && (t as i64) < fin
                        })
                    };
                    if inside {
                        lv[p] = f;
                    }
                }
            }
        }

        // rank bonus: `trunc(range score * pct / 100)`, the range score being the sum of the scores filed in the
        // score frames after the start's up to the end's, when the range completes
        let p = &setup.params;
        let length = match p.score_music_length_ms {
            Some(l) if l != 0 => l,
            _ => p.music_length_ms,
        };
        let max_frame = get_frame(length).wrapping_add(50);
        let sframe = |t: i32| {
            let f = get_frame(t);
            if max_frame <= f { max_frame.wrapping_sub(1) } else { f }
        };
        let mut rv = vec![1.0; ne];
        let mut nobreak = vec![false; ne];
        let mut confirm = Vec::new();
        for (ri, r) in ranges.iter().enumerate() {
            let Some(c) = r.f_complete else { continue };
            if r.pct < 0 {
                return Err(Error::Domain("negative rank bonus percent".into()));
            }
            let (fs, fe) = (sframe(r.start), sframe(r.end));
            for (rj, q) in ranges.iter().enumerate() {
                let x = sframe(q.end);
                if rj != ri && q.f_complete.is_some() && fs < x && x <= fe {
                    return Err(Error::Unsupported("a rank bonus filed inside another range's score".into()));
                }
            }
            confirm.push((r.start, frames[c]));
            let inw: Vec<usize> = (0..ne).filter(|&q| fs < sframe(times[q]) && sframe(times[q]) <= fe).collect();
            let Some(maxt) = inw.iter().map(|&q| times[q]).max() else { continue };
            // every command that ends a factor before one of these entries is filed by the confirmation frame
            if frames[c] <= maxt {
                return Err(Error::Unsupported("a Gekisou range completes before the play reaches its notes".into()));
            }
            let late = entries.iter().any(|e| e.0 > c && e.1.time_ms < maxt);
            for &q in &inw {
                if !ablated(ablate::RANK_BONUS) {
                    rv[q] += r.pct as f64 / 100.0;
                }
                if late {
                    nobreak[q] = true;
                }
            }
        }

        // Where the score commands filed in a frame can land, besides the previous frame's time and the chart times of
        // the frame's notes: the rush commands filed after the previous frame's second score update, at the chart
        // times of that frame's luck notes (and of the pending ones, not before the range's start); and the trigger
        // time of a Gekisou score row: the start of a range that turned Playing (after the frame before last) with a
        // range-playing trigger, the playing range's last combo judgement with a combo trigger.
        let (by_playing, by_combo) = overrides;
        let nf = frames.len();
        let luck_on = ranges.iter().any(|r| r.mission == MISSION_LUCK);
        let in_luck = |t: i32| ranges.iter().any(|r| r.mission == MISSION_LUCK && r.start <= t && t <= r.end);
        let mut by_frame: Vec<Vec<usize>> = vec![Vec::new(); nf];
        for (i, e) in entries.iter().enumerate() {
            by_frame[e.0].push(i);
        }
        // per range: the smallest chart time from the last judgement that is certainly a combo judgement on
        let mut lc: Vec<Option<i32>> = vec![None; ranges.len()];
        let mut exec_lo = vec![i32::MAX; nf];
        for f in 0..nf {
            let mut lo = i32::MAX;
            if f >= 1 && luck_on {
                for &i in &by_frame[f - 1] {
                    let t = entries[i].1.time_ms;
                    if in_luck(t) {
                        lo = lo.min(t);
                    }
                }
                for r in ranges.iter().filter(|r| r.mission == MISSION_LUCK && r.f_start == Some(f - 1)) {
                    lo = lo.min(r.start);
                }
            }
            if by_playing {
                lo = lo.min(if f >= 2 { frames[f - 2] } else { i32::MIN });
            }
            if by_combo && let Some(t) = current[f].and_then(|r| lc[r]) {
                lo = lo.min(t);
            }
            exec_lo[f] = lo;
            for &i in &by_frame[f] {
                let t = entries[i].1.time_ms;
                let certain = reached[i].iter().all(|j| (3..=6).contains(j));
                for (ri, r) in ranges.iter().enumerate() {
                    if r.start <= t && t <= r.end {
                        lc[ri] = Some(if certain { t } else { lc[ri].map_or(t, |x| x.min(t)) });
                    }
                }
            }
        }
        Ok(GkFactors { g: gv, l: lv, r: rv, nobreak, exec_lo, confirm, combo: gcombo })
    }
}

/// The largest number of luck rush commands that can be undone before they apply: the controller files a rush start
/// and its end in processing order, and an end filed at an earlier time than its start lowers the luck bonus between
/// the two. The ends and starts pair up in disjoint stretches of the processing order, so at a time `x` at most as
/// many are inverted as there are consecutive potential luck events `(p, q)` in processing order with `q <= x < p`.
/// Potential luck events, per frame: the frame time when a range turns Finish, the chart times of pending luck
/// notes, the chart times of the frame's luck notes, and the frame time of a playing luck range's pending lots.
fn luck_crossings(frames: &[i32], entries: &[(usize, LiveNote, i32)], sc: &Schedule) -> i64 {
    let nr = sc.ranges.len();
    let nf = frames.len();
    let mut judged: Vec<Vec<i32>> = vec![Vec::new(); nf];
    for e in entries {
        judged[e.0].push(e.1.time_ms);
    }
    let mut seq: Vec<i64> = Vec::new();
    let mut pending: Vec<Vec<i32>> = vec![Vec::new(); nr];
    let mut prev = vec![0u8; nr];
    for (f, &t) in frames.iter().enumerate() {
        let st = &sc.states[f];
        if (0..nr).any(|r| st[r] == RS_FINISH && prev[r] != RS_FINISH) {
            seq.push(t as i64);
        }
        for r in 0..nr {
            if st[r] > RS_STANDBY && !pending[r].is_empty() {
                for tn in std::mem::take(&mut pending[r]) {
                    if (RS_START..=RS_END).contains(&st[r]) {
                        seq.push(tn as i64);
                    }
                }
            }
        }
        for (r, rf) in sc.ranges.iter().enumerate() {
            if rf.mission != MISSION_LUCK {
                continue;
            }
            for &tn in &judged[f] {
                if rf.start <= tn && tn <= rf.end {
                    if st[r] < RS_START {
                        pending[r].push(tn);
                    } else if st[r] <= RS_END {
                        seq.push(tn as i64);
                    }
                }
            }
        }
        for (r, rf) in sc.ranges.iter().enumerate() {
            if rf.mission == MISSION_LUCK && st[r] == RS_PLAYING {
                seq.push(t as i64);
            }
        }
        prev.clone_from(st);
    }
    // every descent (p, q) covers the times [q, p); the largest overlap
    let mut ev: Vec<(i64, i64)> = Vec::new();
    for w in seq.windows(2) {
        if w[1] < w[0] {
            ev.push((w[1], 1));
            ev.push((w[0], -1));
        }
    }
    ev.sort_unstable();
    let (mut cur, mut best) = (0i64, 0i64);
    for (_, d) in ev {
        cur += d;
        best = best.max(cur);
    }
    best
}

/// Frame and entry times the windows read.
struct Geo<'g> {
    /// Frame times, non-decreasing.
    frames: &'g [i32],
    /// Entry chart times, non-decreasing.
    times: &'g [i32],
    exec: &'g Exec,
}

impl Geo<'_> {
    /// The number of play frames with a time in `[a, b)`.
    fn frames_in(&self, a: i64, b: i64) -> f64 {
        let lo = self.frames.partition_point(|&x| (x as i64) < a);
        let hi = self.frames.partition_point(|&x| (x as i64) < b);
        hi.saturating_sub(lo) as f64
    }

    /// Entries with a chart time in `[a, b)`.
    fn range(&self, a: i64, b: i64) -> (u32, u32) {
        let lo = self.times.partition_point(|&x| (x as i64) < a);
        let hi = self.times.partition_point(|&x| (x as i64) < b);
        (lo as u32, hi.max(lo) as u32)
    }

    /// Index of the first frame at or after `t`.
    fn first_frame(&self, t: i32) -> Option<usize> {
        let i = self.frames.partition_point(|&x| x < t);
        (i < self.frames.len()).then_some(i)
    }

    /// The latest finish time of an effect started at `exec` in frame `i0` with activation `act` s and extensions
    /// adding at most `ext` ms (`unbounded`: no bound): its factor holds for the notes at chart times in
    /// `[exec, finish)`. The effect ends in the next frame (finish = that frame's time) when its duration
    /// `act * 1000 + extension` (binary32) does not exceed the elapsed time there, else at `exec + ceil(duration)` once
    /// a frame sees the elapsed time pass the duration; with no such frame it never ends (`i64::MAX`).
    fn end(&self, exec: i32, i0: usize, act: f32, ext: f64, unbounded: bool) -> i64 {
        let n = self.frames.len();
        let next = if i0 + 1 < n { Some(self.frames[i0 + 1] as i64) } else { None };
        if act.is_nan() {
            return next.unwrap_or(i64::MAX);
        }
        if act <= 0.0 {
            if ext > 0.0 || unbounded {
                return i64::MAX;
            }
            return next.unwrap_or(i64::MAX);
        }
        if unbounded || ext >= (1 << 24) as f64 {
            return i64::MAX;
        }
        // the duration is non-decreasing in the extension (binary32 addition and ceil are monotone)
        let dur = act * 1000f32 + ext as f32;
        let last = self.frames[n - 1];
        match next {
            Some(t1) if dur < last.wrapping_sub(exec) as f32 => {
                (exec as i64 + crate::num::ceil_to_i32(dur) as i64).max(t1)
            }
            _ => i64::MAX,
        }
    }
}

/// Score-factor windows of the performer at one position: its live rows (with their condition results) and its
/// snap class's active rows; also the number of factor commands and the total factor that can be active at once.
#[allow(clippy::type_complexity)]
fn windows(
    geo: &Geo,
    live: &[LiveRow],
    snap: &[ActiveRow],
    events: &[i32],
) -> (Vec<Window>, f64, [f64; 4], f64, Vec<(i64, i64, f64)>) {
    let nk = events.len() as f64;
    let mut ext = 0f64;
    let mut unbounded = false;
    for x in live {
        if x.row.effect_type == 15000 && x.out.is_none_or(|o| o.yes) {
            ext += (x.row.value.max(0) as f64) * nk;
        }
    }
    for r in snap {
        if r.effect_type == 15000 && r.can_start {
            if r.event_bound {
                ext += (r.value.max(0) as f64) * nk;
            } else {
                unbounded = true;
            }
        }
    }
    let factors = |t: i64, value: i64, targets: &[i64]| -> (f64, [f64; 4], f64) {
        let mut j = [0f64; 4];
        let mut note = 0f64;
        let mut cmds = 0f64;
        if t == 2000 || t == 2001 {
            note = note_factor_mill(value as f32 / 10000f32).max(0) as f64 / 1e5;
            cmds = 1.0;
        } else if t == 2004 {
            let f = judgement_factor_mill(value as f32 / 10000f32).max(0) as f64 / 1e5;
            for &x in targets {
                if (3..=6).contains(&x) {
                    j[(x - 3) as usize] += f;
                }
                cmds += 1.0;
            }
        }
        (note, j, cmds)
    };
    let mut out = Vec::new();
    let mut cmds = 0f64;
    let mut ops = 0f64;
    let mut spans: Vec<(i64, i64, f64)> = Vec::new();
    // factor that can act on one note: the note factor plus the judgement factor of one judgement
    let mut fac = [0f64; 4];
    let add = |fac: &mut [f64; 4], note: f64, judge: [f64; 4], mult: f64| {
        for j in 0..4 {
            fac[j] += (note + judge[j]) * mult;
        }
    };
    let mut push = |a: i64, b: i64, note: f64, judge: [f64; 4]| {
        let (lo, hi) = geo.range(a, b);
        if hi > lo && (note != 0.0 || judge.iter().any(|&x| x != 0.0)) {
            out.push(Window { lo, hi, note, judge });
        }
    };
    let can = |x: &LiveRow| matches!(x.row.effect_type, 2000 | 2004) && x.out.is_none_or(|o| o.yes);
    for (i, x) in live.iter().enumerate() {
        let r = &x.row;
        if !can(x) {
            continue;
        }
        let (mut note, mut judge, mut c) = factors(r.effect_type, r.value, &r.targets);
        // at most one of the pair starts at an event; with a second event of the position the first factor stays,
        // so the pair is merged only when the position has one event
        if let Some(p) = x.partner.filter(|_| events.len() <= 1) {
            if p < i && can(&live[p]) {
                continue;
            }
            if can(&live[p]) {
                let (n2, j2, c2) = factors(live[p].row.effect_type, live[p].row.value, &live[p].row.targets);
                note = note.max(n2);
                for q in 0..4 {
                    judge[q] = judge[q].max(j2[q]);
                }
                c = c.max(c2);
            }
        }
        for &ev in events {
            let Some(i0) = geo.first_frame(ev) else { continue };
            // a second event of the position restarts the effect without removing the first factor
            let end = if events.len() >= 2 { i64::MAX } else { geo.end(ev, i0, r.act, ext, unbounded) };
            push(ev as i64, end, note, judge);
            cmds += 2.0 * c;
            add(&mut fac, note, judge, 1.0);
            ops += c * (geo.exec.over(ev as i64, ev as i64) + geo.exec.over(ev as i64, end));
            spans.push((ev as i64, end, note + judge.iter().copied().fold(0f64, f64::max)));
        }
    }
    for r in snap {
        if !matches!(r.effect_type, 2000 | 2001 | 2004) || !r.can_start {
            continue;
        }
        let (note, judge, c) = factors(r.effect_type, r.value, &r.targets);
        if let Some(ws) = &r.gk_win {
            for &(a, b, mult) in ws {
                push(a, b, note * mult, judge.map(|x| x * mult));
                // a start and an end per execution; a cumulative one replaces its factor in any frame
                let churn =
                    if r.churn { 2.0 * r.churn_max.unwrap_or(f64::INFINITY).min(geo.frames_in(a, b)) } else { 0.0 };
                cmds += (2.0 + churn) * c * mult;
                add(&mut fac, note, judge, mult);
                ops += (2.0 + churn) * c * mult * geo.exec.over(a, b);
                spans.push((a, b, (note + judge.iter().copied().fold(0f64, f64::max)) * mult));
            }
        } else if r.event_bound && !r.churn {
            for &ev in events {
                let Some(i0) = geo.first_frame(ev) else { continue };
                let exec = geo.frames[i0];
                let end = geo.end(exec, i0, r.act, 0.0, false);
                push(exec as i64, end, note, judge);
                cmds += 2.0 * c;
                add(&mut fac, note, judge, 1.0);
                ops += c * (geo.exec.over(exec as i64, exec as i64) + geo.exec.over(exec as i64, end));
                spans.push((exec as i64, end, note + judge.iter().copied().fold(0f64, f64::max)));
            }
        } else {
            // a cumulative note score up replaces its factor (two commands) in any frame while it runs
            let churn = if r.churn { POOL + 1.0 } else { 1.0 };
            let judge5 = judge.map(|x| x * POOL);
            push(i64::MIN, i64::MAX, note * POOL, judge5);
            cmds += 2.0 * c * geo.frames.len() as f64 * churn;
            add(&mut fac, note, judge, POOL);
            ops += 2.0 * c * geo.frames.len() as f64 * geo.exec.max as f64 * churn;
            spans.push((i64::MIN, i64::MAX, (note + judge.iter().copied().fold(0f64, f64::max)) * POOL));
        }
    }
    (out, cmds, fac, ops, spans)
}

/// How many times each 40 ms score frame up to the last judged note can be executed (its first run and the re-runs
/// after a command lands in it or before it), from the frame schedule and the notes judged in each play frame: in a
/// play frame, both recalculations re-execute score frames only from the earliest frame a command filed in that play
/// frame can land in (the previous play frame's frame for skill commands, the chart time of each judged note) up to
/// the current one. Commands that land after the last note cannot change a note's score and count as none.
struct Exec {
    e: Vec<u32>,
    max: u32,
    max_frame: i32,
    /// `sparse[j][i]`: the largest of `e[i..i + 2^j]`.
    sparse: Vec<Vec<u32>>,
}

impl Exec {
    fn new(setup: &FullSetup, times: &[i32], gk: Option<&GkFactors>) -> Exec {
        let ml = setup.params.music_length_ms;
        let max_frame = get_frame(ml).wrapping_add(50).max(1);
        let Some(&last_note) = times.last() else {
            return Exec { e: Vec::new(), max: 2, max_frame, sparse: Vec::new() };
        };
        let clamp = |t: i32| {
            let g = get_frame(t);
            if g >= max_frame { max_frame - 1 } else { g.max(0) }
        };
        let g_last = clamp(last_note);
        let notes: HashMap<i32, i32> = setup.notes.iter().map(|n| (n.note_id, n.time_ms)).collect();
        let mut diff = vec![0i64; g_last as usize + 2];
        let mut prev_to = 0;
        for (i, f) in setup.play.frames.iter().enumerate() {
            let to = clamp(f.time_ms);
            let mut lo = if i == 0 { 0 } else { prev_to };
            for j in &f.judged {
                if let Some(&t) = notes.get(&j.note_id) {
                    lo = lo.min(clamp(t));
                }
            }
            if let Some(x) = gk.and_then(|g| g.exec_lo.get(i)) {
                lo = lo.min(clamp(*x));
            }
            let hi = to.min(g_last);
            if lo <= hi {
                diff[lo as usize] += 2;
                diff[hi as usize + 1] -= 2;
            }
            prev_to = to;
        }
        // confirming a rank bonus undoes the score frames after the range start and executes them again
        for &(a, b) in gk.map_or(&[][..], |g| &g.confirm[..]) {
            let (a, b) = (clamp(a) + 1, clamp(b).min(g_last));
            if a <= b {
                diff[a as usize] += 1;
                diff[b as usize + 1] -= 1;
            }
        }
        let mut acc = 0i64;
        let mut e = Vec::with_capacity(g_last as usize + 1);
        for d in diff.iter().take(g_last as usize + 1) {
            acc += d;
            e.push(acc.max(1) as u32);
        }
        let max = e.iter().copied().max().unwrap_or(1).max(2);
        let mut sparse = vec![e.clone()];
        let mut w = 1usize;
        while 2 * w <= e.len() {
            let prev = &sparse[sparse.len() - 1];
            let next: Vec<u32> = (0..=e.len() - 2 * w).map(|i| prev[i].max(prev[i + w])).collect();
            sparse.push(next);
            w *= 2;
        }
        Exec { e, max, max_frame, sparse }
    }

    /// Executions of the worst score frame a command in chart-time range `[a, b]` can land in (0 past the last
    /// note).
    fn over(&self, a: i64, b: i64) -> f64 {
        let clamp = |t: i64| {
            let t = t.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let g = get_frame(t);
            (if g >= self.max_frame { self.max_frame - 1 } else { g.max(0) }) as usize
        };
        let (ga, gb) = (clamp(a), clamp(b));
        if ga >= self.e.len() {
            return 0.0;
        }
        let hi = gb.min(self.e.len() - 1);
        if g6_off(4) || hi < ga {
            return self.e[ga..=hi].iter().copied().max().unwrap_or(0) as f64;
        }
        let j = (usize::BITS - 1 - (hi - ga + 1).leading_zeros()) as usize;
        self.sparse[j][ga].max(self.sparse[j][hi + 1 - (1 << j)]) as f64
    }
}

/// Index of the last frame whose judgements a convert function registered in frame `i0` (activation `act` s) can
/// still see: the frame that processes its end at the latest (unbounded when no frame is late enough). The windows
/// of convert functions are in frame indexes: they see the notes judged in frames `i0 + 1 ..= end`.
fn register_end(frames: &[i32], i0: usize, act: f32) -> i64 {
    let next = (i0 + 1) as i64;
    if act.is_nan() || act <= 0.0 {
        return next;
    }
    let dur = act as f64 * 1000.0 * (1.0 + 2f64.powi(-22)) + 1.0;
    let limit = frames[i0] as f64 + dur;
    let i = frames.partition_point(|&t| t as f64 <= limit);
    if i >= frames.len() { i64::MAX } else { (i as i64).max(next) }
}

#[cfg(test)]
mod member_target_regression {
    use super::*;
    use crate::cards::{OwnedMember, Player};

    #[test]
    fn member_master_attributes_reach_live_and_search_matchers() {
        let tables = [
            (
                "MasterMemberCard",
                r#"{"_allData":[{"_id":1,"_characterID":2,"_cardType":3,"_bestMusicTagIDs":[7],"_memberCardLevelGroup":1,"_memberCardAwakeGroup":1,"_memberCardRankGroup":1,"_liveSkillID":10,"_gekisouSkillID":20}]}"#,
            ),
            ("MasterCharacter", r#"{"_allData":[{"_id":2,"_bandID":1}]}"#),
            ("MasterMemberCardLevel", r#"{"_allData":[{"_id":1,"_group":1,"_level":1}]}"#),
            ("MasterMemberCardAwake", r#"{"_allData":[{"_id":1,"_group":1,"_awakeCount":0}]}"#),
            ("MasterMemberCardRank", r#"{"_allData":[{"_id":1,"_group":1,"_rank":1}]}"#),
            ("MasterLiveSkill", r#"{"_allData":[{"_id":10,"_skillCategories":[11]}]}"#),
            ("MasterGekisouSkill", r#"{"_allData":[{"_id":20,"_skillCategories":[21],"_gekisouMissionType":2}]}"#),
        ];
        let master = Master::from_json_tables(|name| tables.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)).unwrap();
        let owned: OwnedMember = serde_json::from_str(r#"{"id":1,"level":1,"awake":0,"rank":1}"#).unwrap();
        let m = MemberView::resolve(&master, &Player::default(), &owned).unwrap();
        let p = performer(&m, None).unwrap();
        assert_eq!(p.tag_ids, vec![7]);
        assert_eq!(p.live_skill_categories, vec![11]);
        assert_eq!(p.gekisou_skill_categories, vec![21]);
        assert_eq!(p.gekisou_mission_type, 2);
        for target in [
            SkillTargetRow { skill_target_type: 1, band_id: 1, character_id: 999, ..Default::default() },
            SkillTargetRow { tag_id: 7, ..Default::default() },
            SkillTargetRow { tag_id: 999, live_skill_categories: vec![11], ..Default::default() },
            SkillTargetRow { gekisou_skill_categories: vec![21], ..Default::default() },
            SkillTargetRow { gekisou_mission_type: 2, ..Default::default() },
        ] {
            assert!(p.matches_skill_target(&target));
            assert!(target_matches(&target, &m).unwrap());
        }
    }
}
