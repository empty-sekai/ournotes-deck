//! Whole-live simulation, frame by frame, with live skills and snap (support) skills; Gekisou off.
//!
//! Each frame, in order: the chart's skill events whose time has come fire (each once); the frame's judged notes, in
//! stream order, go through judgement conversion, the combo counter (at their chart time), note damage and the
//! note score command (with the life at their chart time frozen into it); the score is brought to the frame time;
//! live skills triggered by the fired events restart; skills update in phase 1 then phase 2 (live skills by
//! ascending skill key, then condition skills in deck order), each phase followed by its appliers in list order; the
//! current life is synced and the score is brought to the frame time again.
//!
//! A command that lands in a 40 ms frame the score has already executed (a late judgement, a skill starting at its
//! trigger time) undoes the frames down to it and executes them again; the factor state after such a rewind is the
//! game's, which can differ in the last bits from applying every command once. The life keeps the game's frame
//! cache, which a command in an earlier frame does not fully invalidate.
//!
//! Modelled effect types: live and snap 2000 (note score up), 2004 (judgement score up), 3001 (life recovery with
//! over-heal), 3003 (damage guard), 12006 / 13005 (judgement conversion, with a conversion limit), 15000 (extension
//! of the member's running live skills). Condition types: 1030, 2001, 2003, 4010, 4011, 5000, 7000, 8000 (7005,
//! 7010, 7013, 7020, 7021 read the Gekisou state and fail when asked). Anything else is [`Error::Unsupported`].

mod combo;
mod conditions;
mod convert;
mod engine;
mod life;
mod scorecalc;

use std::collections::{HashMap, HashSet};

use conditions::{CheckCtx, Checker, Factory};
use convert::{Conversion, ConvertEffect};
use engine::{
    CondEffect, ConditionSkillUpdater, END_FRAME, EXECUTE_FRAME, EXECUTING, EffectState, FrameInput, ONE_SHOT, STAY,
    SUSTAINED, TriggerResult, UpdateCheckers, effect_update,
};
use life::LifeController;
use scorecalc::{IncrementalCalculator, NoteCommand};

use crate::error::Error;
use crate::live::random::LiveRandom;
use crate::live::score::{ComboTable, LiveScoreCalculator, LiveScoreSettings, ScoreFactorState, convert_score_type};
use crate::live::skill::{FactorCommand, OWNER_MEMBER, OWNER_SNAP, judgement_factor_mill, note_factor_mill};
use crate::master::Master;

/// Skill type digit of snap skills in their effect keys.
const SKILL_TYPE_SUPPORT: i64 = 3;
/// Skill update phases, in order.
const PHASES: [i64; 2] = [1, 2];

/// One performance position of the deck, in skill order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Performer {
    /// The member's live skill `(id, level)`.
    pub live_skill: Option<(i64, i64)>,
    /// The paired snap's support skills `(id, level)`.
    pub support_skills: Vec<(i64, i64)>,
    /// Member attributes read by member targets.
    pub band_id: i64,
    pub character_id: i64,
    pub card_type: i64,
}

/// A chart note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveNote {
    pub note_id: i32,
    pub time_ms: i32,
    pub note_operate_type: i32,
    /// Note judgement type (selects the judgement windows; types without a Just window are never converted to Just).
    pub judgement_type: i32,
}

/// A note judged in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgedNote {
    pub note_id: i32,
    /// The note judgement before conversion (1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just).
    pub judgement: i32,
    pub judgement_time_ms: i32,
}

/// One frame of a play: its music time and the notes judged in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayFrame {
    pub time_ms: i32,
    pub judged: Vec<JudgedNote>,
}

/// A play: the frame schedule with the judgements, and the live's random seed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LivePlay {
    pub frames: Vec<PlayFrame>,
    pub base_seed: i32,
}

/// The live's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveParams {
    pub total_power: i32,
    pub music_level: i32,
    pub converted_note_count: i32,
    /// Music length (caps effect finish times).
    pub music_length_ms: i32,
    /// Length the score's frame table covers; `None` or 0: the music length.
    pub score_music_length_ms: Option<i32>,
    /// Assist factor (1 without assist).
    pub assist_factor: f32,
}

/// The effect row data the appliers read.
#[derive(Clone, Debug)]
struct EffectRow {
    id: i64,
    effect_type: i64,
    effect_value: i64,
    effect_limit_count: i64,
    /// Judgements of the effect's targets, or the first target id that is not in the master.
    targets: Result<Vec<i64>, i64>,
}

impl EffectRow {
    fn targets(&self) -> Result<&[i64], Error> {
        self.targets.as_deref().map_err(|&id| Error::Master(format!("unknown skill target {id}")))
    }
}

/// Identity of an effect state for the appliers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StateKey {
    Live { row_id: i64, member: usize },
    Cond { effect_id: i64, index: usize },
}

#[derive(Clone, Debug)]
struct LiveEffect {
    row: usize,
    act: f32,
    phase: i64,
    state: EffectState,
    condition: Option<Checker>,
}

#[derive(Clone, Debug)]
struct LiveSkill {
    key: i64,
    member: usize,
    effects: Vec<LiveEffect>,
}

#[derive(Clone, Copy, Debug)]
enum Listed {
    Live { skill: usize, effect: usize },
    Cond { updater: usize, u: usize },
}

/// A live in progress.
#[derive(Clone, Debug)]
pub struct LiveModel {
    notes: HashMap<i32, LiveNote>,
    events: Vec<(i32, i32)>,
    fired: Vec<bool>,
    music_length_ms: i32,
    random: LiveRandom,
    life: LifeController,
    combo: combo::ComboCounter,
    score: IncrementalCalculator,
    conversion: Conversion,
    rows: Vec<EffectRow>,
    live: Vec<LiveSkill>,
    cond: Vec<(usize, ConditionSkillUpdater)>,
    guards: HashMap<StateKey, i32>,
    frame_events: Vec<(i32, i32)>,
    judged: Vec<(i32, i32, i32)>,
    trace: Vec<(i32, i32)>,
}

fn effect_row(master: &Master, id: i64, effect_type: i64, value: i64, limit: i64, target_ids: &[i64]) -> EffectRow {
    let targets = target_ids.iter().map(|&t| master.skill_target(t).map(|r| r.judgement).ok_or(t)).collect();
    EffectRow { id, effect_type, effect_value: value, effect_limit_count: limit, targets }
}

impl LiveModel {
    /// Builds a live: `deck` in skill order, the chart notes, the chart's skill events `(performer index, time)` in
    /// chart order.
    pub fn new(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
    ) -> Result<LiveModel, Error> {
        let settings = LiveScoreSettings::from_master(master)?;
        let table = ComboTable::from_master(master)?;
        let calc = LiveScoreCalculator::new(
            params.total_power,
            params.music_level,
            params.converted_note_count,
            &settings,
            1.0,
            params.assist_factor,
            Some(table),
        );
        let (base, damage) = life::life_settings(master)?;
        let life_length = notes.iter().map(|n| n.time_ms).max().map_or(1000, |t| t.wrapping_add(1000));
        let life = LifeController::new(base, damage, life_length)?;
        let score_length = match params.score_music_length_ms {
            Some(l) if l != 0 => l,
            _ => params.music_length_ms,
        };
        let have_just: HashSet<i64> = master
            .live_judgement_timings
            .iter()
            .filter(|r| r.note_simulate_judgement == 6)
            .map(|r| r.note_judgement_type)
            .collect();
        let no_just: HashSet<i64> = master
            .live_judgement_timings
            .iter()
            .map(|r| r.note_judgement_type)
            .filter(|t| !have_just.contains(t))
            .collect();
        let phase_of: HashMap<i64, i64> =
            master.skill_effect_settings.iter().map(|r| (r.skill_effect_type, r.phase)).collect();
        let phase = |t: i64| phase_of.get(&t).copied().unwrap_or(1);
        let factory = Factory { master, deck };

        let mut rows = Vec::new();
        let mut live = Vec::new();
        for (k, p) in deck.iter().enumerate() {
            let Some((sid, lv)) = p.live_skill else { continue };
            let mut rs: Vec<_> =
                master.live_skill_effects.iter().filter(|r| r.live_skill_id == sid && r.level == lv).collect();
            rs.sort_by_key(|r| r.id);
            let mut effects = Vec::with_capacity(rs.len());
            for r in rs {
                if r.skill_release_condition_group != 0
                    || r.skill_cumulative_condition_id != 0
                    || r.effect_limit_count != 0
                {
                    return Err(Error::Unsupported(format!(
                        "live skill effect {}: release condition, cumulative condition or effect limit",
                        r.id
                    )));
                }
                let condition = factory.group(r.skill_condition_group, k)?;
                rows.push(effect_row(
                    master,
                    r.id,
                    r.skill_effect_type,
                    r.effect_value,
                    r.effect_limit_count,
                    &r.skill_target_ids,
                ));
                effects.push(LiveEffect {
                    row: rows.len() - 1,
                    act: r.activation_time_second,
                    phase: phase(r.skill_effect_type),
                    state: EffectState::default(),
                    condition,
                });
            }
            let key = sid.wrapping_mul(1000).wrapping_add(100).wrapping_add(k as i64);
            live.push(LiveSkill { key, member: k, effects });
        }
        live.sort_by_key(|s| s.key);

        let mut cond = Vec::new();
        for (k, p) in deck.iter().enumerate() {
            for &(sid, lv) in &p.support_skills {
                let mut rs: Vec<_> = master
                    .support_skill_effects
                    .iter()
                    .filter(|r| r.support_skill_id == sid && r.level == lv)
                    .collect();
                rs.sort_by_key(|r| r.id);
                let mut effects = Vec::with_capacity(rs.len());
                let mut release_groups = Vec::with_capacity(rs.len());
                for r in rs {
                    let effect_id = r.id.wrapping_mul(100).wrapping_add(SKILL_TYPE_SUPPORT * 10).wrapping_add(k as i64);
                    let trigger = factory.group(r.skill_trigger_condition_group, k)?;
                    let condition = factory.group(r.skill_condition_group, k)?;
                    let reset = factory.group(r.effect_execute_limit_reset_condition_group, k)?;
                    let count_fails = if r.skill_trigger_type == ONE_SHOT || r.skill_trigger_type == SUSTAINED {
                        factory.cumulative(r.skill_cumulative_condition_id)?
                    } else {
                        false
                    };
                    rows.push(effect_row(
                        master,
                        r.id,
                        r.skill_effect_type,
                        r.effect_value,
                        r.effect_limit_count,
                        &r.skill_target_ids,
                    ));
                    effects.push(CondEffect {
                        effect_id,
                        trigger_type: r.skill_trigger_type,
                        act: r.activation_time_second,
                        phase: phase(r.skill_effect_type),
                        trigger,
                        condition,
                        execute_limit: r.effect_execute_limit_count,
                        reset,
                        row: rows.len() - 1,
                        count_fails,
                    });
                    release_groups.push(r.skill_release_condition_group);
                }
                let updater = ConditionSkillUpdater::new(effects, |e| factory.group(release_groups[e], k))?;
                cond.push((k, updater));
            }
        }

        let mut map = HashMap::with_capacity(notes.len());
        for n in notes {
            map.insert(n.note_id, *n);
        }
        Ok(LiveModel {
            notes: map,
            events: skill_events.to_vec(),
            fired: vec![false; skill_events.len()],
            music_length_ms: params.music_length_ms,
            random: LiveRandom::new(0),
            life,
            combo: combo::ComboCounter::new(notes.len()),
            score: IncrementalCalculator::new(calc, score_length),
            conversion: Conversion::new(no_just),
            rows,
            live,
            cond,
            guards: HashMap::new(),
            frame_events: Vec::new(),
            judged: Vec::new(),
            trace: Vec::new(),
        })
    }

    /// Plays every frame of `play`; returns the final score.
    pub fn run(&mut self, play: &LivePlay) -> Result<i32, Error> {
        self.random.set_seed(play.base_seed);
        for f in &play.frames {
            self.frame(f.time_ms, &f.judged)?;
        }
        Ok(self.score.score)
    }

    /// The current score.
    pub fn score(&self) -> i32 {
        self.score.score
    }

    /// `(frame time, score after the frame)` of every frame played.
    pub fn trace(&self) -> &[(i32, i32)] {
        &self.trace
    }

    /// The score factor state.
    pub fn factor_state(&self) -> &ScoreFactorState {
        &self.score.calc.state
    }

    /// The life stored at the end of the last frame.
    pub fn current_life(&self) -> i32 {
        self.life.current_life
    }

    /// Number of judgements converted by skills so far.
    pub fn converted_judgements(&self) -> u64 {
        self.conversion.converted
    }

    /// Plays one frame at music time `t`.
    pub fn frame(&mut self, t: i32, judged: &[JudgedNote]) -> Result<(), Error> {
        self.frame_events.clear();
        for (i, &(index, ev_t)) in self.events.iter().enumerate() {
            if !self.fired[i] && ev_t <= t {
                self.fired[i] = true;
                self.frame_events.push((index, ev_t));
            }
        }
        // judgement conversion and the combo counter, note by note
        self.judged.clear();
        let mut results = Vec::with_capacity(judged.len());
        for j in judged {
            let n = *self.notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
            let conv = self.conversion.convert(j.judgement, n.judgement_type, j.judgement_time_ms)?;
            self.combo.add_judgement(n.time_ms, conv)?;
            results.push((n, conv));
            self.judged.push((n.note_id, conv, n.time_ms));
        }
        // note damage, frozen life and the note score commands
        for &(n, conv) in &results {
            self.life.add_note_damage(n.time_ms, conv)?;
            let life = self.life.get_life_at_ms(n.time_ms)?;
            let score_type = convert_score_type(conv as i64)?;
            self.score.add_note(NoteCommand::new(n.time_ms, life, n.note_id, n.note_operate_type, score_type));
        }
        self.score.calculate(t, &self.combo)?;
        // skills
        let inp = FrameInput { time_ms: t, music_length_ms: self.music_length_ms };
        let mut started: Vec<(i64, i32)> = Vec::new();
        for s in self.live.iter_mut() {
            if let Some(&(_, ev_t)) = self.frame_events.iter().find(|&&(index, _)| index as i64 == s.member as i64) {
                started.push((s.key, ev_t));
                for e in s.effects.iter_mut() {
                    e.state.state = STAY;
                }
            }
        }
        for (_, u) in self.cond.iter_mut() {
            u.begin_frame();
        }
        let mut listed = Vec::new();
        for ph in PHASES {
            listed.clear();
            let mut ctx = CheckCtx {
                life: &mut self.life,
                random: &mut self.random,
                frame_time: t,
                judged: &self.judged,
                events: &self.frame_events,
            };
            for (si, s) in self.live.iter_mut().enumerate() {
                let start = started.iter().find(|x| x.0 == s.key).map(|x| x.1);
                for (ei, e) in s.effects.iter_mut().enumerate() {
                    if e.phase != ph {
                        continue;
                    }
                    let trigger = match start {
                        Some(time_ms) => TriggerResult { is_trigger: true, time_ms },
                        None => TriggerResult::default(),
                    };
                    if e.state.state != STAY || start.is_some() {
                        let checkers = UpdateCheckers { condition: e.condition.as_mut(), release: None };
                        effect_update(&mut e.state, e.act, inp, trigger, checkers, false, &mut ctx)?;
                    }
                    listed.push(Listed::Live { skill: si, effect: ei });
                }
            }
            for (ui, (_, u)) in self.cond.iter_mut().enumerate() {
                for x in u.update(ph, inp, &mut ctx)? {
                    listed.push(Listed::Cond { updater: ui, u: x });
                }
            }
            for &item in &listed {
                self.apply(item)?;
            }
        }
        self.life.sync_current_life(t)?;
        self.score.calculate(t, &self.combo)?;
        self.trace.push((t, self.score.score));
        Ok(())
    }

    fn state_mut(&mut self, item: Listed) -> &mut EffectState {
        match item {
            Listed::Live { skill, effect } => &mut self.live[skill].effects[effect].state,
            Listed::Cond { updater, u } => &mut self.cond[updater].1.updaters[u].state,
        }
    }

    fn apply(&mut self, item: Listed) -> Result<(), Error> {
        let (ri, k, owner_type, key) = match item {
            Listed::Live { skill, effect } => {
                let s = &self.live[skill];
                let ri = s.effects[effect].row;
                (ri, s.member, OWNER_MEMBER, StateKey::Live { row_id: self.rows[ri].id, member: s.member })
            }
            Listed::Cond { updater, u } => {
                let (k, up) = &self.cond[updater];
                let upd = &up.updaters[u];
                let ef = &up.effects[upd.effect];
                (ef.row, *k, OWNER_SNAP, StateKey::Cond { effect_id: ef.effect_id, index: upd.index })
            }
        };
        let st = *self.state_mut(item);
        let owner = (k as i32).wrapping_mul(100).wrapping_add(owner_type);
        let (effect_type, value) = (self.rows[ri].effect_type, self.rows[ri].effect_value);
        match effect_type {
            2000 => {
                let m = note_factor_mill(value as f32 / 10000f32);
                if st.state == EXECUTE_FRAME {
                    self.score.add_factor(FactorCommand {
                        time_ms: st.execute_ms,
                        owner_id: owner,
                        note_mill: m,
                        ..Default::default()
                    });
                } else if st.state == END_FRAME {
                    self.score.add_factor(FactorCommand {
                        time_ms: st.finish_ms,
                        owner_id: owner,
                        note_mill: m.wrapping_neg(),
                        ..Default::default()
                    });
                }
            }
            2004 => {
                let m = judgement_factor_mill(value as f32 / 10000f32);
                let targets = self.rows[ri].targets()?.to_vec();
                for j in targets {
                    let (time_ms, judge_mill) = match st.state {
                        EXECUTE_FRAME => (st.execute_ms, m),
                        END_FRAME => (st.finish_ms, m.wrapping_neg()),
                        _ => continue,
                    };
                    self.score.add_factor(FactorCommand {
                        time_ms,
                        owner_id: owner,
                        judgement: j as i32,
                        judge_mill,
                        ..Default::default()
                    });
                }
            }
            15000 => {
                if st.state == EXECUTE_FRAME {
                    self.extend(k, value as f32);
                }
            }
            3001 => {
                if st.state == EXECUTE_FRAME {
                    self.life.recovery(st.execute_ms, value, true)?;
                }
            }
            3003 => {
                if st.state == EXECUTE_FRAME {
                    let id = self.life.enable_guard(st.execute_ms)?;
                    self.guards.insert(key, id);
                } else if st.state == END_FRAME {
                    if let Some(id) = self.guards.remove(&key) {
                        self.life.disable_guard(st.finish_ms, id)?;
                    }
                }
            }
            12006 | 13005 => {
                let row = &self.rows[ri];
                let effect = ConvertEffect {
                    effect_type,
                    effect_value: value,
                    effect_limit_count: row.effect_limit_count,
                    targets: row.targets()?,
                    effect_id: row.id,
                };
                if let Some(t) = self.conversion.update(key, st.state, &effect)? {
                    let s = self.state_mut(item);
                    s.state = END_FRAME;
                    s.finish_ms = t;
                }
            }
            t => return Err(Error::Unsupported(format!("skill effect type {t}"))),
        }
        Ok(())
    }

    /// Extends every running live skill effect of the member.
    fn extend(&mut self, member: usize, ms: f32) {
        for s in self.live.iter_mut() {
            if s.member != member {
                continue;
            }
            for e in s.effects.iter_mut() {
                if e.state.state == EXECUTE_FRAME || e.state.state == EXECUTING {
                    e.state.extended_ms += ms;
                }
            }
        }
    }
}
