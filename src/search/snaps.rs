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

use std::collections::HashMap;
use std::time::Instant;

use crate::cards::{MemberView, SnapView};
use crate::error::Error;
use crate::live::full::{LiveModel, LiveNote, LiveParams, LivePlay, Performer};
use crate::live::model::JudgementStream;
use crate::live::score::{
    COMBO, ComboTable, LiveScoreSettings, convert_score_type, get_frame, get_music_score_level_factor,
};
use crate::live::skill::{judgement_factor_mill, note_factor_mill};
use crate::live::skip::Chart;
use crate::master::{Master, SkillTargetRow};
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

/// A chart, a judgement stream and the live's numbers, prepared for simulating many decks.
#[derive(Clone, Debug)]
pub(crate) struct FullSetup {
    pub notes: Vec<LiveNote>,
    pub events: Vec<(i32, i32)>,
    pub play: LivePlay,
    pub params: LiveParams,
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
            total_power: 0,
            music_level,
            converted_note_count: chart.converted_note_count,
            music_length_ms: chart.last_timing_note_ms.wrapping_add(1000),
            score_music_length_ms: None,
            assist_factor,
        };
        Ok(FullSetup { notes, events, play: stream.to_live_play()?, params })
    }

    /// The simulated score of performers (in performance order) at a deck power.
    pub fn score(&self, master: &Master, performers: &[Performer], power: i32) -> Result<i32, Error> {
        let params = LiveParams { total_power: power, ..self.params };
        let mut lm = LiveModel::new(master, performers, &self.notes, &self.events, params)?;
        lm.run(&self.play)
    }
}

/// The performer of a member card paired with a snap.
#[allow(clippy::needless_update)]
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
        ..Default::default()
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

/// The member attributes member targets read.
#[derive(Clone, Copy, Debug)]
struct Attr {
    band_id: i64,
    character_id: i64,
    card_type: i64,
}

fn no_value() -> Error {
    Error::Master("skill condition without a value".into())
}

/// Member target match, as the simulation decides it when it builds a checker.
fn target_matches(tg: &SkillTargetRow, a: Attr) -> Result<bool, Error> {
    if tg.skill_target_type != 3 {
        return Err(Error::Unsupported(format!("member target of type {}", tg.skill_target_type)));
    }
    for (key, val) in [(tg.character_id, a.character_id), (tg.band_id, a.band_id), (tg.card_type, a.card_type)] {
        if key != 0 {
            return Ok(key == val);
        }
    }
    Ok(false)
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
}

impl Env<'_> {
    fn cond(&self, cid: i64, a: Attr) -> Result<Option<Out>, Error> {
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
                Out { yes: false, no: true, impure: false }
            }
            t @ (7005 | 7010 | 7013 | 7020 | 7021) => {
                return Err(Error::Unsupported(format!(
                    "condition type {t} reads the Gekisou state of a live without Gekisou"
                )));
            }
            t => return Err(Error::Unsupported(format!("skill condition type {t}"))),
        };
        Ok(Some(if c.is_positive { o } else { o.not() }))
    }

    /// A condition group: an OR over its sets, each an AND over its conditions (`None`: no checker).
    fn group(&self, gid: i64, a: Attr) -> Result<Option<Out>, Error> {
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
}

/// A live skill row of a member with its condition result and the row whose condition is its negation, if any.
#[derive(Clone, Debug)]
struct LiveRow {
    row: Row,
    out: Option<Out>,
    partner: Option<usize>,
}

/// Classification of one row for a member.
fn support_status(env: &Env, r: &Row, a: Attr) -> Result<(Status, bool, bool), Error> {
    if r.trigger_type != 1 && r.trigger_type != 2 {
        return Err(Error::Unsupported(format!("skill trigger type {}", r.trigger_type)));
    }
    if r.trigger_type == 2 && has_activation_time(r.act) {
        return Err(Error::Unsupported("sustained effect with an activation time".into()));
    }
    let trig = env.group(r.trigger, a)?;
    let cond = env.group(r.condition, a)?;
    let reset = env.group(r.reset, a)?;
    let release = env.group(r.release, a)?;
    let fails = env.cumulative_fails(r.cumulative)?;
    let can_start = trig.is_some_and(|t| t.yes) && cond.is_none_or(|c| c.yes);
    let event_bound = env.event_only(r.trigger);
    if !matches!(r.effect_type, 2000 | 2004 | 3001 | 3003 | 12006 | 13005 | 15000) && can_start {
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
    let st = match r.effect_type {
        2000 | 2004 | 15000 => Status::Active,
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
    /// a mask.
    extra: Vec<[Vec<u8>; 5]>,
    /// Entries whose life is 0 in every play of a candidate that neither recovers life nor guards: the damage
    /// already filed when the entry reads its life empties it. `z_dead` is their factor after the floor.
    dead: Vec<bool>,
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
type ConvSource = (Vec<(i32, Vec<i64>)>, Vec<(i32, Vec<i64>, u32, bool)>);

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
    /// Relative margin of the bounds.
    eps: f64,
}

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
        let attr = |m: &MemberView| Attr { band_id: m.band_id, character_id: m.character_id, card_type: m.card_type };
        let mut sets: HashMap<i64, Vec<&[i64]>> = HashMap::new();
        for s in &master.skill_condition_sets {
            sets.entry(s.group).or_default().push(&s.condition_ids);
        }
        let mut env = Env { master, sets, life_lo: 0, life_hi: 2 * base, life_rigid: false, raw };
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
        let all_rows = || live.values().flatten().chain(snap_rows.iter().flatten().flatten());
        // life: the judgements each stream entry can reach (conversions of any allowed card registered when the
        // entry is judged), damage and recovery
        let frames: Vec<i32> = setup.play.frames.iter().map(|f| f.time_ms).collect();
        let fire: Vec<usize> = setup
            .events
            .iter()
            .filter(|e| (0..5).contains(&e.0))
            .filter_map(|e| {
                let i = frames.partition_point(|&x| x < e.1);
                (i < frames.len()).then_some(i)
            })
            .collect();
        let mut convs: Vec<Conv> = Vec::new();
        let whole = vec![(i64::MIN, i64::MAX)];
        for r in live.values().flatten().filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            convs.push((convert_to(r.effect_type, r.value), r.targets.clone(), whole.clone()));
        }
        for r in snap_rows.iter().flatten().flatten().filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            let w = if env.event_only(r.trigger) {
                // registered in the start frame, it converts the notes judged in the next frames up to the frame that
                // processes its end
                fire.iter().map(|&i0| (i0 as i64, register_end(&frames, i0, r.act))).collect()
            } else {
                whole.clone()
            };
            convs.push((convert_to(r.effect_type, r.value), r.targets.clone(), w));
        }
        convs.retain(|c| c.0 != -1);
        let reach = |j: i32, t: i64| -> Vec<i32> {
            let mut v = vec![j];
            for (to, targets, w) in &convs {
                if *to != j
                    && targets.contains(&(j as i64))
                    && !v.contains(to)
                    && w.iter().any(|&(a, b)| a < t && t <= b)
                {
                    v.push(*to);
                }
            }
            v
        };
        let reached: Vec<Vec<i32>> = entries.iter().map(|&(fi, _, j)| reach(j, fi as i64)).collect();
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
        if !positive(adj)
            || !positive(mdf)
            || cnc <= 0
            || !(assist.is_finite() && assist >= 0.0)
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
        let mut gids: HashMap<Vec<Vec<RowSig>>, u32> = HashMap::new();
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
            let mut keys: HashMap<Vec<Vec<RowSig>>, usize> = HashMap::new();
            let mut cl = vec![Class { snaps: Vec::new(), rows: Vec::new() }];
            keys.insert(Vec::new(), 0);
            let next = gids.len() as u32;
            let mut cg = vec![*gids.entry(Vec::new()).or_insert(next)];
            let mut of = Vec::with_capacity(t.snaps.len());
            for (j, per) in snap_rows.iter().enumerate() {
                let mut key: Vec<Vec<RowSig>> = Vec::new();
                let mut rows = Vec::new();
                for skill in per {
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
                        });
                        rows.push(ActiveRow {
                            effect_type: r.effect_type,
                            value: r.value,
                            act: r.act,
                            event_bound,
                            can_start,
                            targets: r.targets.clone(),
                        });
                    }
                    if !sk.is_empty() {
                        key.push(sk);
                    }
                }
                let c = match keys.get(&key) {
                    Some(&c) => c,
                    None => {
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
        // members that differ only in attributes no member target reads are interchangeable
        let char_read = master.skill_targets.iter().any(|t| t.skill_target_type == 3 && t.character_id != 0);
        let mut ids: HashMap<(i64, i64, i64, i64, i64), u32> = HashMap::new();
        let mut sim_id = vec![u32::MAX; n];
        for &m in &members {
            let v = &pool.members[m];
            let key = (
                v.live_skill_id,
                v.live_skill_level,
                v.band_id,
                v.card_type,
                if char_read { v.character_id } else { 0 },
            );
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
        for &i in &order {
            let (_, n, _) = entries[i];
            // the combo a note reads counts the entries at earlier chart times since the last one that breaks it
            let b = breakers.partition_point(|&x| x < n.time_ms);
            let from = if b == 0 { 0 } else { times_all.partition_point(|&x| x < breakers[b - 1]) };
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
            for &x in &reached[i] {
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
            let pre_e = adj64 * mdf64 * (note_pct as f64 / 100.0);
            pre.push(pre_e);
            let k = pre_e * combo_max[before] / cnc as f64;
            coef.times.push(n.time_ms);
            coef.k.push(k);
            coef.max_jp.push(max_jp);
            coef.jp.push(jp);
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
            if let Ok(st) = convert_score_type(x as i64) {
                if let Some(&p) = settings.judgement_score_factor_percent.get(&st) {
                    if p >= 0 {
                        *slot = p as f64 / 100.0;
                    }
                }
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
            dead: order.iter().map(|&i| dead_stream[i]).collect(),
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
                let snap_conv: Vec<(i32, Vec<i64>, u32, bool)> = c
                    .rows
                    .iter()
                    .filter(|r| matches!(r.effect_type, 12006 | 13005) && r.can_start)
                    .map(|r| (convert_to(r.effect_type, r.value), r.targets.clone(), r.act.to_bits(), r.event_bound))
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
                for (k, out) in ex.iter_mut().enumerate() {
                    *out = (0..ne)
                        .map(|e| {
                            let (j, fi) = (fine.raw[e] as i32, ent_frame[e]);
                            let mut mask = 0u8;
                            for (to, tg) in &key.0 {
                                if *to != j && tg.contains(&(j as i64)) {
                                    mask |= 1 << to;
                                }
                            }
                            for (to, tg, act, eb) in &key.1 {
                                let seen = !eb
                                    || fire_k[k].iter().any(|&i0| {
                                        (i0 as i64) < fi && fi <= register_end(&frames, i0, f32::from_bits(*act))
                                    });
                                if *to != j && tg.contains(&(j as i64)) && seen {
                                    mask |= 1 << to;
                                }
                            }
                            mask
                        })
                        .collect();
                }
                fine.extra.push(ex);
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
        let exec = Exec::new(setup, &coef.times);
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
                    arr[k] = Contrib { windows: w, gain: g, judge, ops, spans };
                }
                per.push(arr);
            }
            contrib[m] = per;
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
        let eps = drift + 2f64.powi(-22) + CHAIN_EPS + 2f64.powi(-19);
        // the class search bounds life only when some entry can read life 0 (when the fold without recoveries never
        // reaches 0, no fold with recoveries does) at a factor below `Coef::z`
        let life_bound = env.life_lo <= 0
            && (fine.dead_from < coef.times.len() || fine.zero_from([0; 5]) != i64::MAX)
            && (0..coef.times.len()).any(|e| fine.z_dead < coef.z[e]);
        let split = if life_bound { split_envelopes(&contrib, &fine, &coef) } else { Vec::new() };
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
            eps,
        };
        // the score must stay far from the 32-bit range for the per-note sum to be monotone in power
        let mut u: Vec<i64> = members
            .iter()
            .map(|&m| t.a[m] + t.lead.iter().map(|row| row[m]).max().unwrap_or(0).max(0) + t.wmax[m])
            .collect();
        u.sort_unstable_by(|x, y| y.cmp(x));
        let p_max: i64 = u.iter().take(5).sum();
        if sl.score_bound(p_max) >= i32::MAX as i64 / 2 {
            return Err(Error::Domain("live score bound exceeds the 32-bit range".into()));
        }
        Ok(sl)
    }

    /// An upper bound of the live score of any deck with power at most `power`.
    pub fn score_bound(&self, power: i64) -> i64 {
        ub(power, self.global, self.eps)
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
        ub(power, (self.a0 + gain).min(self.global), self.eps)
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
                plain.min(pz[s] + (pd[ne] - pd[s]))
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
        let mut total = 0f64;
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
        drift + 2f64.powi(-22) + CHAIN_EPS + 2f64.powi(-19)
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
        let c = &self.coef;
        let f = &self.fine;
        let extra: [Option<&[u8]>; 5] =
            std::array::from_fn(|k| (src[k] != 0).then(|| &f.extra[src[k] as usize][k][..]));
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
            let mut mask = 1u8 << f.raw[e];
            for x in extra.iter().flatten() {
                mask |= x[e];
            }
            let mask = mask as usize;
            if f.breaks[mask] {
                broke = Some(gs);
            }
            let combo = f.combo_max.get(gs - from).copied().unwrap_or(f64::INFINITY);
            let k = f.pre[e] * combo / f.cnc;
            acc += scratch.note[e];
            let mut v = f.mjp[mask] * (1.0 + acc.max(0.0));
            if judge {
                for j in 0..4 {
                    accj[j] += scratch.judge[j][e];
                    v += f.jp4[mask][j] * accj[j].max(0.0);
                }
            }
            let x = p * k * v * (1.0 + eps);
            let y = x.floor();
            let dead = match life {
                CandLife::NoRise => f.dead[e],
                CandLife::ZeroFrom(t0) => t0 <= c.times[e] as i64 && t0 < f.until[e],
                CandLife::Unknown => false,
            };
            let ze = if dead { f.z_dead } else { c.z[e] };
            let z = if ze == 1.0 { y } else { (ze * y * (1.0 + 2f64.powi(-20))).floor() };
            total = total.saturating_add(z as i64);
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
        if orders[0].0 < threshold {
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
            if lf.timed_out || bound < lf.cutoff() {
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
        stats.orders += lf.sims;
        let timed_out = lf.timed_out;
        Ok((lf.best.map(|(c, score)| LeafBest { score, power: c.power, snaps: c.snaps, order: c.order }), timed_out))
    }
}

#[derive(Default)]
struct Scratch {
    note: Vec<f64>,
    judge: [Vec<f64>; 4],
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

    /// Whether a candidate with this bound, power and identity could still beat the best simulated one.
    fn could_beat(&self, c: &Cand) -> bool {
        if c.bound < self.threshold {
            return false;
        }
        match &self.best {
            None => true,
            Some((b, s)) => {
                c.bound > *s || (c.bound == *s && (c.power, b.snap_ids, b.order) > (b.power, c.snap_ids, c.order))
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
        if self.nodes % 1024 == 0 {
            if let Some(d) = self.deadline {
                if Instant::now() >= d {
                    self.timed_out = true;
                }
            }
        }
        if self.timed_out {
            return Ok(());
        }
        if i == 5 {
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
            if ub(n1 + rest.w[i + 1], n2 + rest.g[i + 1], self.sl.eps) < self.cutoff() {
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
                ub(p, n2 + rest.g[upto] - gm + go, self.sl.eps) >= cutoff
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
                if ub(p, sl.a0 + best_assignment(&g), sl.eps) >= cutoff {
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
            return ub(power, sl.a0 + best_assignment(&self.gn), sl.eps) >= cutoff;
        }
        let g: [[f64; 5]; 5] =
            std::array::from_fn(|j| std::array::from_fn(|k| sl.split_gain(self.members[j], k, start, self.gn[j][k])));
        ub(power, sl.a0_from(start) + best_assignment(&g), sl.eps) >= cutoff
    }

    /// Whether an order can still reach the cutoff under the class search's life bound, before any class is chosen.
    fn order_open(&mut self, pos: [usize; 5], rest: &Rests) -> bool {
        let (fixed, a0) = (self.fixed, self.sl.a0);
        if self.other_open(pos, 0, fixed, a0, rest) {
            return true;
        }
        let (start, a) = self.life_rest(pos, 0, &[0; 5]);
        let a = if start >= self.sl.coef.times.len() { a0 + rest.gn[0] } else { a };
        ub(fixed.saturating_add(rest.wn[0]), a, self.sl.eps) >= self.cutoff()
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
            return ub(power, n2 + rest.gn[i + 1], sl.eps) >= cutoff;
        }
        // the split gain is at most the plain one
        let part = &sl.contrib[self.members[i]][cs[i]][pos[i]];
        if ub(power, a + part.gain, sl.eps) < cutoff {
            return false;
        }
        ub(power, a + sl.gain_from(part, start), sl.eps) >= cutoff
    }

    /// Bounds one (order, class assignment) and queues it when it can still win.
    fn consider(&mut self, o: [usize; 5], cs: [usize; 5], pos: [usize; 5]) -> Result<(), Error> {
        let Some((w, snaps_j)) = self.matching(cs) else { return Ok(()) };
        let power = self.fixed + w;
        let sl = self.sl;
        let parts: [&Contrib; 5] = std::array::from_fn(|i| &sl.contrib[self.members[i]][cs[i]][pos[i]]);
        let s2 = sl.a0 + parts.iter().map(|p| p.gain).sum::<f64>();
        if ub(power, s2, sl.eps) < self.cutoff() {
            return Ok(());
        }
        let life = self.cand_life(o, cs);
        let start = sl.dead_start(life);
        if start < sl.coef.times.len() && ub(power, sl.life_sum(parts, start), sl.eps) < self.cutoff() {
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
            if let Some(d) = self.deadline {
                if Instant::now() >= d {
                    self.timed_out = true;
                    rest.push(c);
                    continue;
                }
            }
            let score = self.simulate(&c)? as i64;
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
        let pool = self.pool;
        let perf = c
            .order
            .iter()
            .map(|&slot| performer(&pool.members[self.members[slot]], c.snaps[slot].map(|s| &pool.snaps[s])))
            .collect::<Result<Vec<_>, _>>()?;
        let power = i32::try_from(c.power).map_err(|_| Error::Domain("deck power exceeds the 32-bit range".into()))?;
        self.sl.setup.score(self.sl.master, &perf, power)
    }
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
        if t == 2000 {
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
        if !matches!(r.effect_type, 2000 | 2004) || !r.can_start {
            continue;
        }
        let (note, judge, c) = factors(r.effect_type, r.value, &r.targets);
        if r.event_bound {
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
            let judge5 = judge.map(|x| x * POOL);
            push(i64::MIN, i64::MAX, note * POOL, judge5);
            cmds += 2.0 * c * geo.frames.len() as f64;
            add(&mut fac, note, judge, POOL);
            ops += 2.0 * c * geo.frames.len() as f64 * geo.exec.max as f64;
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
}

impl Exec {
    fn new(setup: &FullSetup, times: &[i32]) -> Exec {
        let ml = setup.params.music_length_ms;
        let max_frame = get_frame(ml).wrapping_add(50).max(1);
        let Some(&last_note) = times.last() else { return Exec { e: Vec::new(), max: 2, max_frame } };
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
            let hi = to.min(g_last);
            if lo <= hi {
                diff[lo as usize] += 2;
                diff[hi as usize + 1] -= 2;
            }
            prev_to = to;
        }
        let mut acc = 0i64;
        let mut e = Vec::with_capacity(g_last as usize + 1);
        for d in diff.iter().take(g_last as usize + 1) {
            acc += d;
            e.push(acc.max(1) as u32);
        }
        let max = e.iter().copied().max().unwrap_or(1).max(2);
        Exec { e, max, max_frame }
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
        self.e[ga..=gb.min(self.e.len() - 1)].iter().copied().max().unwrap_or(0) as f64
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
