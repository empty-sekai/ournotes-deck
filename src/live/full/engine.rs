//! The skill effect state machine and the condition skill updater (snap, Gekisou and Gekisou support skills).
//!
//! An effect state moves Stay -> ExecuteFrame (started, with the trigger time) -> Executing -> EndFrame (with the
//! finish time) -> Stay. Appliers act on ExecuteFrame and EndFrame (some also on Executing). An effect with a
//! cumulative condition carries its count in the state while it runs.

use std::collections::{HashMap, VecDeque};

use super::conditions::{CheckCtx, Checker, Cumulative};
use super::gekisou::M_ALL;
use crate::error::Error;
use crate::num::ceil_to_i32;

pub(crate) const STAY: u8 = 0;
pub(crate) const EXECUTE_FRAME: u8 = 2;
pub(crate) const EXECUTING: u8 = 3;
pub(crate) const END_FRAME: u8 = 4;

/// Trigger types of condition skills.
pub(crate) const ONE_SHOT: i64 = 1;
pub(crate) const SUSTAINED: i64 = 2;

/// Effect updaters per effect.
const POOL: usize = 5;

/// One effect state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct EffectState {
    /// 0 Stay, 2 ExecuteFrame, 3 Executing, 4 EndFrame.
    pub state: u8,
    pub execute_ms: i32,
    pub finish_ms: i32,
    /// Duration added by extensions, in ms.
    pub extended_ms: f32,
    /// The cumulative condition's count (kept after the effect ends).
    pub cumulative_count: i64,
    /// The cumulative condition's unit and maximum count (0 without one).
    pub cumulative_unit: i64,
    pub cumulative_max: i64,
}

/// The frame values the updaters read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameInput {
    pub time_ms: i32,
    pub music_length_ms: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TriggerResult {
    pub is_trigger: bool,
    pub time_ms: i32,
}

const NO_TRIGGER: TriggerResult = TriggerResult { is_trigger: false, time_ms: 0 };

/// The checkers an effect update may ask.
pub(crate) struct UpdateCheckers<'c> {
    /// Start condition (asked in Stay on every update).
    pub condition: Option<&'c mut Checker>,
    /// Release condition (ends an executing effect).
    pub release: Option<&'c mut Checker>,
}

/// One update of an effect state.
pub(crate) fn effect_update(
    st: &mut EffectState,
    act: f32,
    inp: FrameInput,
    trigger: TriggerResult,
    checkers: UpdateCheckers,
    finish_frame: bool,
    ctx: &mut CheckCtx,
) -> Result<bool, Error> {
    match st.state {
        STAY => {
            let ok = match checkers.condition {
                None => true,
                Some(c) => c.check(ctx)?.0,
            };
            if ok && trigger.is_trigger {
                st.state = EXECUTE_FRAME;
                st.execute_ms = trigger.time_ms;
            }
            Ok(ok && trigger.is_trigger)
        }
        EXECUTE_FRAME => {
            let t = inp.time_ms;
            if finish_frame {
                st.state = END_FRAME;
                st.finish_ms = t;
            } else if checkers.release.is_some() {
                st.state = EXECUTING;
            } else {
                let dur = act * 1000f32 + st.extended_ms;
                let elapsed = t.wrapping_sub(st.execute_ms) as f32;
                if dur > elapsed {
                    st.state = EXECUTING;
                } else {
                    st.state = END_FRAME;
                    st.finish_ms = t;
                }
            }
            Ok(true)
        }
        EXECUTING => {
            let released = match checkers.release {
                None => false,
                Some(c) => c.check(ctx)?.0,
            };
            let dur = act * 1000f32 + st.extended_ms;
            let elapsed = inp.time_ms.wrapping_sub(st.execute_ms) as f32;
            let by_time = dur < elapsed && act > 0f32;
            if !(released || finish_frame || by_time) {
                return Ok(false);
            }
            st.state = END_FRAME;
            let mut finish =
                if !released && !finish_frame { st.execute_ms.wrapping_add(ceil_to_i32(dur)) } else { inp.time_ms };
            if inp.music_length_ms > 0 && inp.music_length_ms <= finish {
                finish = inp.music_length_ms;
            }
            st.finish_ms = finish;
            Ok(true)
        }
        END_FRAME => {
            st.state = STAY;
            Ok(true)
        }
        s => Err(Error::Game(format!("effect state {s} out of range"))),
    }
}

/// Whether a sustained effect has an activation time: `act > 0` and not approximately `int.MaxValue`.
fn has_activation_time(act: f32) -> bool {
    if act.is_nan() || act <= 0f32 {
        return false;
    }
    let big = 2147483647f32;
    let m = act.abs().max(big);
    let tol = (m * 1e-6f32).max(f32::from_bits(1) * 8f32);
    tol <= (big - act).abs()
}

/// One effect of a condition skill.
#[derive(Clone, Debug)]
pub(crate) struct CondEffect {
    pub effect_id: i64,
    pub trigger_type: i64,
    pub act: f32,
    pub phase: i64,
    pub trigger: Option<Checker>,
    pub condition: Option<Checker>,
    pub execute_limit: i64,
    pub reset: Option<Checker>,
    /// Index of the effect's row data in the model.
    pub row: usize,
    /// The cumulative condition (each effect updater counts its own).
    pub cumulative: Option<Cumulative>,
}

/// An effect updater of a condition skill: its own state, release checker and cumulative counter.
#[derive(Clone, Debug)]
pub(crate) struct EffectUpdater {
    pub effect: usize,
    pub index: usize,
    pub state: EffectState,
    release: Option<Checker>,
    phase: i64,
    cumulative: Option<Cumulative>,
}

impl EffectUpdater {
    fn update(
        &mut self,
        act: f32,
        inp: FrameInput,
        trigger: TriggerResult,
        finish_frame: bool,
        ctx: &mut CheckCtx,
    ) -> Result<bool, Error> {
        let s0 = self.state.state;
        let checkers = UpdateCheckers { condition: None, release: self.release.as_mut() };
        let r = effect_update(&mut self.state, act, inp, trigger, checkers, finish_frame, ctx)?;
        if let Some(c) = self.cumulative.as_mut() {
            let s1 = self.state.state;
            if (s0 == STAY && s1 == EXECUTE_FRAME) || s0 == EXECUTE_FRAME || (s0 == EXECUTING && s1 == EXECUTING) {
                self.state.cumulative_count = c.update_count(ctx)?;
            } else if s0 == END_FRAME {
                c.reset();
            }
        }
        Ok(r)
    }
}

/// The sustained updater of one effect (activation time 0): one effect updater runs while the trigger hits.
#[derive(Clone, Debug)]
struct Sustained {
    queue: VecDeque<usize>,
    current: Option<usize>,
    enabled: bool,
}

/// The updater of one condition skill.
#[derive(Clone, Debug)]
pub(crate) struct ConditionSkillUpdater {
    pub effects: Vec<CondEffect>,
    sorted: Vec<usize>,
    pub updaters: Vec<EffectUpdater>,
    stacks: Vec<Vec<usize>>,
    sustained: Vec<Option<Sustained>>,
    executing: Vec<usize>,
    execute_count: HashMap<i64, i64>,
    trigger_checked: bool,
    cache: Vec<Option<TriggerResult>>,
    /// Gekisou (support) skills trigger only while a range of this mission is concerned (4: always).
    gate: Option<i64>,
}

impl ConditionSkillUpdater {
    /// `release(e)` builds a fresh release checker for an updater of effect `e`; `gate` is the mission of a Gekisou
    /// (support) skill.
    pub(crate) fn new(
        effects: Vec<CondEffect>,
        mut release: impl FnMut(usize) -> Result<Option<Checker>, Error>,
        gate: Option<i64>,
    ) -> Result<ConditionSkillUpdater, Error> {
        let mut ids: Vec<i64> = effects.iter().map(|e| e.effect_id).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(Error::Master("condition skill with a duplicate effect id".into()));
        }
        let mut sorted: Vec<usize> = (0..effects.len()).collect();
        sorted.sort_by_key(|&e| effects[e].effect_id);
        let mut updaters = Vec::new();
        let mut stacks = vec![Vec::new(); effects.len()];
        let mut sustained = vec![None; effects.len()];
        for (e, ef) in effects.iter().enumerate() {
            if ef.trigger_type == SUSTAINED && has_activation_time(ef.act) {
                return Err(Error::Unsupported("sustained effect with an activation time".into()));
            }
            if ef.trigger_type != ONE_SHOT && ef.trigger_type != SUSTAINED {
                continue;
            }
            let mut pool = Vec::with_capacity(POOL);
            for index in 0..POOL {
                pool.push(updaters.len());
                let mut state = EffectState::default();
                if let Some(c) = &ef.cumulative {
                    state.cumulative_unit = c.unit();
                    state.cumulative_max = c.max();
                }
                updaters.push(EffectUpdater {
                    effect: e,
                    index,
                    state,
                    release: release(e)?,
                    phase: ef.phase,
                    cumulative: ef.cumulative.clone(),
                });
            }
            if ef.trigger_type == ONE_SHOT {
                stacks[e] = pool;
            } else {
                sustained[e] = Some(Sustained { queue: pool.into(), current: None, enabled: false });
            }
        }
        let n = effects.len();
        Ok(ConditionSkillUpdater {
            effects,
            sorted,
            updaters,
            stacks,
            sustained,
            executing: Vec::new(),
            execute_count: HashMap::new(),
            trigger_checked: false,
            cache: vec![None; n],
            gate,
        })
    }

    /// Whether the Gekisou gate lets the triggers be checked this frame.
    fn gate_open(&self, ctx: &CheckCtx) -> Result<bool, Error> {
        let Some(mission) = self.gate else { return Ok(true) };
        if mission == M_ALL {
            return Ok(true);
        }
        let c = ctx.gk.ok_or_else(|| Error::Unsupported("Gekisou skill in a live without Gekisou".into()))?.ctrl;
        if c.state_updates.iter().any(|&i| c.ranges[i].mission == mission) {
            return Ok(true);
        }
        let i = c.current_playing_index;
        Ok(i >= 0 && c.ranges[i as usize].mission == mission)
    }

    pub(crate) fn begin_frame(&mut self) {
        self.trigger_checked = false;
    }

    fn update_one(
        &mut self,
        u: usize,
        inp: FrameInput,
        trigger: TriggerResult,
        finish_frame: bool,
        ctx: &mut CheckCtx,
    ) -> Result<bool, Error> {
        let act = self.effects[self.updaters[u].effect].act;
        self.updaters[u].update(act, inp, trigger, finish_frame, ctx)
    }

    /// Updates the skill for one phase; returns the effect updaters whose states the appliers see, in order.
    pub(crate) fn update(&mut self, phase: i64, inp: FrameInput, ctx: &mut CheckCtx) -> Result<Vec<usize>, Error> {
        let mut updated = Vec::new();
        let mut done = Vec::new();
        for i in 0..self.executing.len() {
            let u = self.executing[i];
            if self.updaters[u].phase == phase {
                self.update_one(u, inp, NO_TRIGGER, false, ctx)?;
                if self.updaters[u].state.state == STAY {
                    done.push(u);
                } else {
                    updated.push(u);
                }
            }
        }
        if !self.trigger_checked {
            self.trigger_checked = true;
            self.cache.iter_mut().for_each(|c| *c = None);
            if !self.gate_open(ctx)? {
                return self.finish_update(updated, done);
            }
            for ef in self.effects.iter_mut() {
                if let Some(r) = ef.reset.as_mut() {
                    if r.check(ctx)?.0 {
                        self.execute_count.remove(&ef.effect_id);
                    }
                }
            }
            for &e in &self.sorted {
                let tr = match self.effects[e].trigger.as_mut() {
                    None => NO_TRIGGER,
                    Some(c) => {
                        let (hit, _) = c.check(ctx)?;
                        TriggerResult { is_trigger: hit, time_ms: c.override_time().unwrap_or(inp.time_ms) }
                    }
                };
                self.cache[e] = Some(tr);
            }
        }
        for si in 0..self.sorted.len() {
            let e = self.sorted[si];
            let Some(tr) = self.cache[e] else { continue };
            let trigger_type = self.effects[e].trigger_type;
            if trigger_type == SUSTAINED {
                if self.effects[e].phase == phase {
                    if let Some(u) = self.update_sustained(e, inp, tr, ctx)? {
                        updated.push(u);
                    }
                }
                continue;
            }
            if trigger_type != ONE_SHOT {
                return Err(Error::Unsupported(format!("skill trigger type {trigger_type}")));
            }
            if !tr.is_trigger {
                continue;
            }
            let Some(&top) = self.stacks[e].last() else { continue };
            if self.updaters[top].phase != phase {
                continue;
            }
            let ef = &mut self.effects[e];
            if ef.execute_limit > 0 {
                if let Some(&n) = self.execute_count.get(&ef.effect_id) {
                    if ef.execute_limit <= n {
                        continue;
                    }
                }
            }
            if let Some(c) = ef.condition.as_mut() {
                if !c.check(ctx)?.0 {
                    continue;
                }
            }
            let (limited, eid) = (ef.execute_limit > 0, ef.effect_id);
            let u = self.stacks[e].pop().expect("non-empty stack");
            self.update_one(u, inp, TriggerResult { is_trigger: true, time_ms: tr.time_ms }, false, ctx)?;
            self.executing.push(u);
            updated.push(u);
            if limited {
                *self.execute_count.entry(eid).or_insert(0) += 1;
            }
        }
        self.finish_update(updated, done)
    }

    /// Puts the one-shot updaters that went back to Stay back on their stacks.
    fn finish_update(&mut self, updated: Vec<usize>, done: Vec<usize>) -> Result<Vec<usize>, Error> {
        for u in done {
            let e = self.updaters[u].effect;
            self.stacks[e].push(u);
            if let Some(p) = self.executing.iter().position(|&x| x == u) {
                self.executing.remove(p);
            }
        }
        Ok(updated)
    }

    fn update_sustained(
        &mut self,
        e: usize,
        inp: FrameInput,
        tr: TriggerResult,
        ctx: &mut CheckCtx,
    ) -> Result<Option<usize>, Error> {
        let execute_ms = if tr.is_trigger { tr.time_ms } else { inp.time_ms };
        let mut s = self.sustained[e].take().expect("sustained updater");
        let r = self.sustained_step(&mut s, e, execute_ms, inp, tr, ctx);
        self.sustained[e] = Some(s);
        r
    }

    fn sustained_step(
        &mut self,
        s: &mut Sustained,
        e: usize,
        execute_ms: i32,
        inp: FrameInput,
        tr: TriggerResult,
        ctx: &mut CheckCtx,
    ) -> Result<Option<usize>, Error> {
        let mut cur = s.current;
        if let Some(c) = cur {
            if self.updaters[c].state.state == END_FRAME {
                self.update_one(c, inp, NO_TRIGGER, false, ctx)?;
                s.queue.push_back(c);
                s.current = None;
                cur = None;
            }
        }
        if let Some(c) = cur {
            if self.updaters[c].state.state == EXECUTE_FRAME {
                self.updaters[c].state.state = EXECUTING;
            }
        }
        if tr.is_trigger {
            if !s.enabled {
                if let Some(c) = self.effects[e].condition.as_mut() {
                    c.reset();
                }
                s.enabled = true;
            }
            if let Some(c) = self.effects[e].condition.as_mut() {
                if !c.check(ctx)?.0 {
                    return Ok(None);
                }
            }
            if cur.is_none() {
                let c = s.queue.pop_front().ok_or_else(|| Error::Game("sustained effect queue is empty".into()))?;
                s.current = Some(c);
                if self.update_one(c, inp, TriggerResult { is_trigger: true, time_ms: execute_ms }, false, ctx)? {
                    return Ok(Some(c));
                }
            }
        } else {
            if let Some(c) = cur {
                self.update_one(c, inp, NO_TRIGGER, true, ctx)?;
                s.enabled = false;
                return Ok(Some(c));
            }
            s.enabled = false;
        }
        Ok(None)
    }
}
