//! Conservative LUCK replay using the native controller, condition updater and effect appliers.
//!
//! Life/probability outcomes are nondeterministic; formation predicates are nondeterministic but constant per
//! checker instance. Every script branch must finish, otherwise there is no usable mask. Callers must separately
//! prove that every possible converted judgement remains in the raw judgement's LUCK control equivalence class.
//! Masks describe direct sole-positive-7021 triggers only: compound triggers can skip a sticky check.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// Union of possible direct-trigger flags, with an independent lifetime execution bound. Taking a union can
/// hide false gaps, so the component count of `flags` is NOT a bound on an individual execution's Rush starts.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[doc(hidden)]
pub struct RushMasks {
    pub flags: [Vec<bool>; 4],
    /// Maximum false-to-true transitions over completed branches, counting only gate-open observations.
    /// Gate-closed frames retain the prior observation, exactly as the condition updater's trigger cache does.
    pub max_runs: [u64; 4],
    /// The half-open chart-time spans `[added at, disabled at)`, sorted and disjoint, of every rush score bonus
    /// command of every branch; None when unknown. A score frame executes a factor command before its notes at the
    /// same or a later time, so a note reads a pair's bonus exactly in its span; the net bonus at a time is positive
    /// only inside the span of some pair (an inverted pair, disabled before its addition, only lowers it).
    pub spans: Option<Vec<(i32, i32)>>,
}

impl RushMasks {
    #[cfg(any(test, feature = "search-diagnostics"))]
    fn empty(frames: usize) -> Self {
        Self { flags: std::array::from_fn(|_| vec![false; frames]), max_runs: [0; 4], spans: Some(Vec::new()) }
    }

    /// Sorted disjoint half-open spans covering the nonempty `[added, disabled)` spans.
    fn merge_spans(spans: impl IntoIterator<Item = (i32, i32)>) -> Vec<(i32, i32)> {
        let mut all: Vec<(i32, i32)> = spans.into_iter().filter(|&(a, b)| a < b).collect();
        all.sort_unstable();
        let mut out: Vec<(i32, i32)> = Vec::with_capacity(all.len());
        for (a, b) in all {
            match out.last_mut() {
                Some(l) if a <= l.1 => l.1 = l.1.max(b),
                _ => out.push((a, b)),
            }
        }
        out
    }

    /// Whether a note at chart time `t` may read a positive rush bonus (always when the spans are unknown).
    #[doc(hidden)]
    pub fn rush_possible(&self, t: i32) -> bool {
        let Some(spans) = &self.spans else { return true };
        let i = spans.partition_point(|s| s.0 <= t);
        i > 0 && t < spans[i - 1].1
    }

    /// Merge independent alternatives, including distinct roots with the same performance order. Reject shape
    /// mismatches before mutation; lifetime counts take their maximum, never the union's component count.
    #[doc(hidden)]
    pub fn union_with(&mut self, other: &Self) -> bool {
        if self.flags.iter().zip(&other.flags).any(|(a, b)| a.len() != b.len()) {
            return false;
        }
        for gate in 0..4 {
            for (out, &flag) in self.flags[gate].iter_mut().zip(&other.flags[gate]) {
                *out |= flag;
            }
            self.max_runs[gate] = self.max_runs[gate].max(other.max_runs[gate]);
        }
        self.spans = match (self.spans.take(), &other.spans) {
            (Some(a), Some(b)) => Some(Self::merge_spans(a.into_iter().chain(b.iter().copied()))),
            _ => None,
        };
        true
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RunCounter {
    previous: bool,
    starts: u64,
}

impl RunCounter {
    fn observe(&mut self, open: bool, flag: bool) -> Result<(), Error> {
        if open {
            if flag && !self.previous {
                self.starts = self
                    .starts
                    .checked_add(1)
                    .ok_or_else(|| Error::Capacity("LUCK replay run count overflow".into()))?;
            }
            self.previous = flag;
        }
        Ok(())
    }
}
pub(super) type SharedScript = Rc<RefCell<Script>>;

#[derive(Clone, Debug, Default)]
pub(super) struct Script {
    prefix: Vec<bool>,
    cursor: usize,
    overflow: bool,
    /// Frame time of every answer of the current run, the last one included when it overflows.
    asked: Vec<i32>,
    /// `[lowest, highest]` life of every live the caller admits, at every time (see [`rush_frames`]).
    life: Option<(i64, i64)>,
}

impl Script {
    /// The value of life comparison `ty` with `v` (as `LifeGreater`, `LifeAtLeast`, `LifeLess`, `LifeAtMost`) when
    /// every life in the proven range gives the same one.
    pub(super) fn life_decides(&self, ty: i64, v: i64) -> Option<bool> {
        let (lo, hi) = self.life?;
        debug_assert!(lo <= hi, "empty proven life range");
        let (yes, no) = match ty {
            2000 => (v < lo, hi <= v),
            2001 => (v <= lo, hi < v),
            2002 => (hi < v, v <= lo),
            2003 => (hi <= v, v < lo),
            _ => return None,
        };
        if yes {
            Some(true)
        } else if no {
            Some(false)
        } else {
            None
        }
    }

    pub(super) fn answer(&mut self, t: i32) -> Result<bool, Error> {
        self.asked.push(t);
        let Some(&answer) = self.prefix.get(self.cursor) else {
            self.overflow = true;
            return Err(Error::Unsupported("LUCK replay script needs another branch".into()));
        };
        self.cursor += 1;
        Ok(answer)
    }
}

/// Why the conservative replay supplies no masks. Every case keeps the caller's wide envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "search-diagnostics"), allow(dead_code))]
#[doc(hidden)]
pub enum RushDecline {
    /// No run budget, or not one delta time per declared frame.
    Input,
    /// A retained LUCK effect on an ordinary skill, or a cumulative condition on a Gekisou one.
    Signature,
    /// The reduced model refuses an effect or condition.
    Build(String),
    /// The scripted branches exceed the run budget: the frame times of the answers of the last explored branch.
    Runs(Vec<i32>),
    /// Some branch failed in a native pool or range path.
    Branch(String),
    /// Branch masks of different shapes.
    Shape,
}

#[cfg(feature = "search-diagnostics")]
impl RushDecline {
    #[doc(hidden)]
    pub fn label(&self) -> &'static str {
        match self {
            RushDecline::Input => "input",
            RushDecline::Signature => "signature",
            RushDecline::Build(_) => "build",
            RushDecline::Runs(_) => "runs",
            RushDecline::Branch(_) => "branch",
            RushDecline::Shape => "shape",
        }
    }
}

pub(super) fn retained(effect: i64) -> bool {
    matches!(effect, 11000 | 11001 | 11003 | 11005)
}

/// The controller skips Wait/Pass completely; Miss still attempts to consume pending lots. Bad consumes a base
/// RNG draw but adds zero gauge. Perfect and Just share the same base-point table and controller path.
#[doc(hidden)]
pub fn luck_judgement_class(judgement: i32) -> Option<u8> {
    match judgement {
        0 | 7 => Some(0),
        -1 | 1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 | 6 => Some(5),
        _ => None,
    }
}

/// Request-local identity of retained LUCK skills. Preserve physical/performance slot order and support order;
/// binary32 gauge-factor accumulation and native shared state are not assumed commutative.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[doc(hidden)]
pub struct LuckSignature {
    pub member: Option<(i64, i64)>,
    pub support: Vec<(i64, i64)>,
    pub has_gk: bool,
}

#[doc(hidden)]
pub fn luck_signature(master: &Master, performer: &Performer) -> Result<Option<LuckSignature>, Error> {
    // The reduced interpreter omits ordinary skill pools. Refuse to omit a LUCK effect from either source.
    if performer.live_skill.is_some_and(|(id, lv)| {
        master
            .live_skill_effects
            .iter()
            .any(|r| r.live_skill_id == id && r.level == lv && retained(r.skill_effect_type))
    }) || performer.support_skills.iter().any(|&(id, lv)| {
        master
            .support_skill_effects
            .iter()
            .any(|r| r.support_skill_id == id && r.level == lv && retained(r.skill_effect_type))
    }) {
        return Ok(None);
    }
    let mut signature = LuckSignature { member: None, support: Vec::new(), has_gk: performer.gekisou_skill.is_some() };
    if let Some((id, lv)) = performer.gekisou_skill {
        master.gekisou_skill(id).ok_or_else(|| Error::Master(format!("unknown Gekisou skill {id}")))?;
        let rows: Vec<_> = master
            .gekisou_skill_effects
            .iter()
            .filter(|r| r.skill_id == id && r.level == lv && retained(r.skill_effect_type))
            .collect();
        if rows.iter().any(|r| r.skill_cumulative_condition_id != 0) {
            return Ok(None);
        }
        if !rows.is_empty() {
            signature.member = Some((id, lv));
        }
        for &(id, lv) in &performer.gekisou_support_skills {
            master
                .gekisou_support_skill(id)
                .ok_or_else(|| Error::Master(format!("unknown Gekisou support skill {id}")))?;
            let rows: Vec<_> = master
                .gekisou_support_skill_effects
                .iter()
                .filter(|r| r.skill_id == id && r.level == lv && retained(r.skill_effect_type))
                .collect();
            if rows.iter().any(|r| r.skill_cumulative_condition_id != 0) {
                return Ok(None);
            }
            if !rows.is_empty() {
                signature.support.push((id, lv));
            }
        }
    }
    Ok(Some(signature))
}

#[derive(Clone, Debug)]
pub(super) struct RushProbes {
    probes: [Checker; 4],
    pub masks: [Vec<bool>; 4],
    runs: [RunCounter; 4],
}

impl RushProbes {
    fn new() -> Self {
        Self {
            probes: std::array::from_fn(|_| Checker::LuckRushPlaying(false)),
            masks: std::array::from_fn(|_| Vec::new()),
            runs: [RunCounter::default(); 4],
        }
    }

    pub(super) fn observe(&mut self, model: &mut LiveModel) -> Result<(), Error> {
        let gk =
            model.gk.as_ref().map(|g| GkView { ctrl: &g.ctrl, prev_lots: &g.prev_lots, prev_lot_ms: g.prev_lot_ms });
        let mut ctx = CheckCtx {
            life: &mut model.life,
            random: &mut model.random,
            frame_time: model.frame_time,
            current_combo: model.current_combo,
            judged: &[],
            events: &[],
            gk,
            prev_confirmed_rank: model.frame_rank_confirmation,
        };
        for (index, probe) in self.probes.iter_mut().enumerate() {
            let open = engine::mission_gate_open(Some(index as i64 + 1), &ctx)?;
            let flag = open && probe.check(&mut ctx)?.0;
            self.runs[index].observe(open, flag)?;
            self.masks[index].push(flag);
        }
        Ok(())
    }
}

impl LiveModel {
    /// LUCK-only frame. The range state machine, trigger phase order, effect pools and applier state are native.
    /// Retained effects do not read score/life/combo and cannot change Rush before the final controller update.
    fn luck_frame(&mut self, t: i32, judged: &[GkNote], dt: f32, probes: &mut RushProbes) -> Result<(), Error> {
        self.frame_time = t;
        let gk = self.gk.as_mut().ok_or_else(|| Error::Unsupported("LUCK replay requires Gekisou".into()))?;
        gk.fever.update(t, &mut gk.fever_updates);
        let mut handle = Handle { sc: &mut self.scorectl, score: &mut self.score };
        let mut env = Env { random: &mut self.random, handle: &mut handle };
        gk.ctrl.before_update(dt, t, &gk.fever_updates, 0, &mut env)?;
        probes.observe(self)?;
        let input = FrameInput { time_ms: t, music_length_ms: self.music_length_ms, is_live_finished: false };
        for condition in &mut self.cond {
            condition.updater.begin_frame();
        }
        for phase in PHASES {
            let gk =
                self.gk.as_ref().map(|g| GkView { ctrl: &g.ctrl, prev_lots: &g.prev_lots, prev_lot_ms: g.prev_lot_ms });
            let mut ctx = CheckCtx {
                life: &mut self.life,
                random: &mut self.random,
                frame_time: t,
                current_combo: 0,
                judged: &[],
                events: &[],
                gk,
                prev_confirmed_rank: None,
            };
            let mut listed = Vec::new();
            for (updater, condition) in self.cond.iter_mut().enumerate() {
                for u in condition.updater.update(phase, input, &mut ctx)? {
                    if condition.updater.updaters[u].state.state != STAY {
                        listed.push(Listed::Cond { updater, u });
                    }
                }
            }
            for item in listed {
                self.apply(item)?;
            }
        }
        let gk = self.gk.as_mut().expect("Gekisou checked above");
        let mut handle = Handle { sc: &mut self.scorectl, score: &mut self.score };
        let mut env = Env { random: &mut self.random, handle: &mut handle };
        gk.ctrl.update(t, judged, &gk.fever_updates, 0, &mut env)?;
        gk.prev_lots.clone_from(&gk.ctrl.lot_results);
        gk.prev_lot_ms = if gk.prev_lots.is_empty() { 0 } else { t };
        Ok(())
    }

    /// Full-model diagnostic observation for direct singleton 7021 triggers, using the same gate helper as the
    /// actual condition updater. Probes do not draw random values or update gameplay state.
    #[cfg(feature = "search-diagnostics")]
    pub fn run_with_rush_masks(
        &mut self,
        play: &LivePlay,
        delta_times: &[f32],
        random: LiveRandom,
    ) -> Result<(i32, [Vec<bool>; 4]), Error> {
        if self.program_has_started || self.rush_probes.is_some() {
            return Err(Error::Input("Rush probes require a fresh model".into()));
        }
        if self.gk.is_none() {
            return Err(Error::Unsupported("Rush probes require Gekisou".into()));
        }
        self.rush_probes = Some(RushProbes::new());
        let result = self.run_with_random(play, delta_times, random);
        let probes = self.rush_probes.take().expect("Rush probes installed");
        result.map(|score| (score, probes.masks))
    }
}

/// Conservative union of direct-singleton-7021 flags over every admitted life/probability/formation outcome.
/// `deck` is in PERFORMANCE order. The caller must prove raw-vs-converted LUCK judgement class equivalence
/// for every potentially consumed LUCK note. This routine does not establish that precondition itself.
/// `life`, when given, must contain every life the complete live can read at any time; a life comparison it
/// decides then takes that one answer instead of both.
#[cfg(any(test, feature = "search-diagnostics"))]
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn rush_frames(
    master: &Master,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    seed: i32,
    max_runs: usize,
    life: Option<(i64, i64)>,
) -> Result<Option<RushMasks>, Error> {
    Ok(rush_frames_why(master, notes, params, setup, play, delta_times, deck, seed, max_runs, life)?.ok())
}

/// [`rush_frames`], with the reason when it supplies no masks.
#[cfg(any(test, feature = "search-diagnostics"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn rush_frames_why(
    master: &Master,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    seed: i32,
    max_runs: usize,
    life: Option<(i64, i64)>,
) -> Result<Result<RushMasks, RushDecline>, Error> {
    let branches = rush_branches_why(master, notes, params, setup, play, delta_times, deck, seed, max_runs, life)?;
    Ok(branches.and_then(|branches| {
        let mut union = RushMasks::empty(play.frames.len());
        for masks in &branches {
            if !union.union_with(masks) {
                return Err(RushDecline::Shape);
            }
        }
        Ok(union)
    }))
}

/// The distinct masks of the completed branches of the reduced replay. The complete live's script answers are
/// one enumerated path, so its own flags, runs and spans are those of one branch: a bound that holds for a live
/// covered by any one branch's masks, taken at its maximum over the branches, is a bound for the complete live.
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn rush_branches_why(
    master: &Master,
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    seed: i32,
    max_runs: usize,
    life: Option<(i64, i64)>,
) -> Result<Result<Vec<RushMasks>, RushDecline>, Error> {
    if max_runs == 0 || delta_times.len() != play.frames.len() {
        return Ok(Err(RushDecline::Input));
    }
    for performer in deck {
        if luck_signature(master, performer)?.is_none() {
            return Ok(Err(RushDecline::Signature));
        }
    }
    let script = Rc::new(RefCell::new(Script::default()));
    let reduced: Vec<_> = deck
        .iter()
        .cloned()
        .map(|mut p| {
            p.live_skill = None;
            p.support_skills.clear();
            p
        })
        .collect();
    let fresh = match LiveModel::build(master, &reduced, notes, &[], params, Some(setup), false, Some(script.clone())) {
        Ok(model) => model,
        Err(Error::Unsupported(why)) => return Ok(Err(RushDecline::Build(why))),
        Err(error) => return Err(error),
    };
    let note_map: HashMap<_, _> = notes.iter().map(|n| (n.note_id, n)).collect();
    let frames = play
        .frames
        .iter()
        .map(|frame| {
            frame
                .judged
                .iter()
                .map(|j| {
                    let note =
                        note_map.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
                    Ok((note.note_id, note.note_operate_type, note.time_ms, j.judgement))
                })
                .collect::<Result<Vec<GkNote>, Error>>()
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let mut branches: Vec<RushMasks> = Vec::new();
    let mut pending = vec![Vec::new()];
    let mut runs = 0;
    while let Some(prefix) = pending.pop() {
        if runs >= max_runs {
            return Ok(Err(RushDecline::Runs(std::mem::take(&mut script.borrow_mut().asked))));
        }
        runs += 1;
        *script.borrow_mut() = Script { prefix: prefix.clone(), cursor: 0, overflow: false, asked: Vec::new(), life };
        let mut model = fresh.clone();
        // The LUCK stream is independent of member shuffle and SKILL consumption.
        model.random = LiveRandom::new(seed);
        let mut probes = RushProbes::new();
        let result = play
            .frames
            .iter()
            .zip(&frames)
            .zip(delta_times)
            .try_for_each(|((frame, judged), &dt)| model.luck_frame(frame.time_ms, judged, dt, &mut probes));
        if script.borrow().overflow {
            let mut no = prefix.clone();
            no.push(false);
            let mut yes = prefix;
            yes.push(true);
            pending.push(yes);
            pending.push(no);
            continue;
        }
        match result {
            // An impossible nondeterministic branch may hit a native pool/range failure. It cannot be silently
            // dropped: the reduction does not prove infeasibility, so decline the whole mask conservatively.
            Err(error) => return Ok(Err(RushDecline::Branch(error.to_string()))),
            Ok(()) => {
                let completed = RushMasks {
                    flags: probes.masks,
                    max_runs: probes.runs.map(|r| r.starts),
                    spans: Some(RushMasks::merge_spans(model.rush_spans())),
                };
                if !branches.contains(&completed) {
                    branches.push(completed);
                }
            }
        }
    }
    Ok(Ok(branches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn row(id: i64, key: &str, skill: i64, effect: i64, value: i64, trigger: i64, condition: i64) -> Value {
        json!({"_id":id,key:skill,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":trigger,"_skillConditionGroup":condition,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":effect,"_activationTimeSecond":0.0,"_effectValue":value,
            "_maxEffectValue":0,"_effectLimitCount":1,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0})
    }

    fn master(extra_ordinary: bool, cumulative: bool) -> Master {
        let mut speed = row(1, "_gekisouSkillID", 1, 11001, 20_000, 7020, 0);
        speed["_skillTriggerType"] = json!(2);
        let mut gauge = row(2, "_gekisouSkillID", 2, 11003, 10_000, 7010, 2001);
        if cumulative {
            gauge["_skillCumulativeConditionID"] = json!(1);
        }
        let mut miss = row(2, "_gekisouSupportSkillID", 2, 11003, 1000, 7000, 5000);
        miss["_effectExecuteLimitCount"] = json!(1);
        let lots: Vec<_> = (0..5)
            .flat_map(|kind| {
                [0, 3].map(
                    move |result| json!({"_id":kind*4+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1}),
                )
            })
            .collect();
        let ordinary: Vec<_> =
            if extra_ordinary { vec![row(1, "_supportSkillID", 1, 11003, 1000, 7010, 0)] } else { vec![] };
        let tables = json!({
            "MasterLiveSettings":[
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"},
                {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
                {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
                {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
            "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
            "MasterLiveJudgementParameter":[
                {"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0},
                {"_id":2,"_noteSimulateJudgement":6,"_scorePercent":200,"_damage":0}],
            "MasterLiveJudgementTiming":[
                {"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":100},
                {"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":100}],
            "MasterLiveGekisouLuckBasePoint":[
                {"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":100}],
            "MasterLiveGekisouLuckBonusLot":lots,
            "MasterSkillTarget":[{"_id":1,"_bandID":1}],
            "MasterSkillCondition":[
                {"_id":7000,"_conditionType":7000,"_conditionValues":[0],"_conditionTargetIDs":[],"_isPositive":true},
                {"_id":7010,"_conditionType":7010,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true},
                {"_id":7020,"_conditionType":7020,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true},
                {"_id":2001,"_conditionType":2001,"_conditionValues":[700],"_conditionTargetIDs":[],"_isPositive":true},
                {"_id":4011,"_conditionType":4011,"_conditionValues":[50],"_conditionTargetIDs":[],"_isPositive":true},
                {"_id":5000,"_conditionType":5000,"_conditionValues":[],"_conditionTargetIDs":[1],"_isPositive":true},
                {"_id":4002,"_conditionType":4002,"_conditionValues":[1],"_conditionTargetIDs":[],"_isPositive":true}],
            "MasterSkillConditionSet":[
                {"_id":1,"_group":7000,"_conditionIds":[7000]},
                {"_id":2,"_group":7010,"_conditionIds":[7010]},
                {"_id":3,"_group":7020,"_conditionIds":[7020]},
                {"_id":4,"_group":2001,"_conditionIds":[2001,4011]},
                {"_id":5,"_group":5000,"_conditionIds":[5000]},
                {"_id":6,"_group":4011,"_conditionIds":[4011,5000]},
                {"_id":7,"_group":4002,"_conditionIds":[4002]}],
            "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2},{"_id":2,"_gekisouMissionType":2},
                {"_id":3,"_gekisouMissionType":2}],
            "MasterGekisouSkillEffect":[speed,gauge,row(3,"_gekisouSkillID",3,11003,1000,7010,4002)],
            "MasterGekisouSupportSkill":[{"_id":1,"_gekisouMissionType":2},{"_id":2,"_gekisouMissionType":2}],
            "MasterGekisouSupportSkillEffect":[row(1,"_gekisouSupportSkillID",1,11005,4,7010,4011),miss],
            "MasterSupportSkillEffect":ordinary
        });
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()))
            .unwrap()
    }

    fn fixture() -> (Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
        let notes: Vec<_> = (0..15)
            .map(|i| LiveNote { note_id: i, note_operate_type: 1, judgement_type: 1, time_ms: 60 + i * 60 })
            .collect();
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 1000,
            music_level: 20,
            converted_note_count: 15,
            music_length_ms: 2000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let setup = GekisouSetup { fevers: vec![(100, 900)], missions: vec![2, 2, 2] };
        let mut frames: Vec<_> = (0..=125).map(|i| PlayFrame { time_ms: i * 16, judged: Vec::new() }).collect();
        for note in &notes {
            frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
                note_id: note.note_id,
                judgement: 5,
                judgement_time_ms: note.time_ms,
            });
        }
        let delta = vec![0.016; frames.len()];
        (notes, params, setup, LivePlay { frames, base_seed: 0 }, delta)
    }

    #[test]
    fn rush_spans_merge_inverted_and_open_commands() {
        // an inverted pair adds no positive span; the disabling time itself reads no bonus
        let spans = RushMasks::merge_spans([(100, 200), (250, 180), (200, 220), (400, i32::MAX)]);
        assert_eq!(spans, vec![(100, 220), (400, i32::MAX)]);
        let m = RushMasks { flags: std::array::from_fn(|_| Vec::new()), max_runs: [0; 4], spans: Some(spans) };
        assert!(m.rush_possible(100) && m.rush_possible(219) && m.rush_possible(i32::MAX - 1));
        assert!(!m.rush_possible(99) && !m.rush_possible(220) && !m.rush_possible(250) && !m.rush_possible(399));
        let unknown = RushMasks { spans: None, ..m.clone() };
        assert!(unknown.rush_possible(0));
        let mut union = m.clone();
        assert!(union.union_with(&RushMasks { spans: Some(vec![(300, 350)]), ..m }));
        assert_eq!(union.spans, Some(vec![(100, 220), (300, 350), (400, i32::MAX)]));
    }

    #[test]
    fn judgement_classes_distinguish_skipped_notes_and_pending_lot_consumption() {
        assert_ne!(luck_judgement_class(0), luck_judgement_class(1));
        assert_eq!(luck_judgement_class(0), luck_judgement_class(7));
        assert_eq!(luck_judgement_class(-1), luck_judgement_class(1));
        assert_eq!(luck_judgement_class(5), luck_judgement_class(6));
        assert_ne!(luck_judgement_class(1), luck_judgement_class(2));
        assert_eq!(luck_judgement_class(8), None);
    }

    #[test]
    fn mask_union_preserves_branch_execution_maximum_when_false_gaps_disappear() {
        let branch = |flags: Vec<bool>| {
            let mut counter = RunCounter::default();
            for &flag in &flags {
                counter.observe(true, flag).unwrap();
            }
            RushMasks { flags: std::array::from_fn(|_| flags.clone()), max_runs: [counter.starts; 4], spans: None }
        };
        let alternating = branch(vec![true, false, true, false, true]);
        let continuous = branch(vec![true; 5]);
        assert_eq!(alternating.max_runs, [3; 4]);
        assert_eq!(continuous.max_runs, [1; 4]);
        let mut union = alternating.clone();
        assert!(union.union_with(&continuous));
        assert_eq!(union.flags, continuous.flags);
        assert_eq!(union.max_runs, [3; 4]);
        let mut reverse = continuous;
        assert!(reverse.union_with(&alternating));
        assert_eq!(union, reverse);
        let before = union.clone();
        assert!(!union.union_with(&RushMasks::empty(4)));
        assert_eq!(union, before);
    }

    #[test]
    fn run_counter_ignores_closed_gates_and_checks_overflow() {
        let mut counter = RunCounter::default();
        for (open, flag) in [(true, true), (false, false), (true, true), (true, false), (false, true), (true, true)] {
            counter.observe(open, flag).unwrap();
        }
        assert_eq!(counter.starts, 2);
        counter.previous = false;
        counter.starts = u64::MAX;
        assert!(matches!(counter.observe(true, true), Err(Error::Capacity(_))));
    }

    #[test]
    #[cfg(feature = "search-diagnostics")]
    fn actual_probes_reject_ordinary_live_without_panicking_or_leaving_recording_state() {
        let (notes, params, _, play, dt) = fixture();
        let master = master(false, false);
        let mut model = LiveModel::new(&master, &[Performer::default()], &notes, &[], params).unwrap();
        assert!(matches!(model.run_with_rush_masks(&play, &dt, LiveRandom::new(7)), Err(Error::Unsupported(_))));
        assert!(model.rush_probes.is_none());
        assert!(model.run_with_random(&play, &dt, LiveRandom::new(7)).is_ok());
    }

    #[test]
    fn replay_declines_unknown_control_ordinary_luck_cumulative_and_unfinished_branches() {
        let (notes, params, setup, play, dt) = fixture();
        let run = |master: &Master, performer: Performer, budget| {
            rush_frames(master, &notes, params, &setup, &play, &dt, &[performer], 7, budget, None).unwrap()
        };
        assert!(
            run(&master(false, false), Performer { gekisou_skill: Some((3, 1)), ..Default::default() }, 64).is_none()
        );
        assert!(
            run(&master(false, false), Performer { gekisou_skill: Some((2, 1)), ..Default::default() }, 1).is_none()
        );
        assert!(
            run(&master(false, true), Performer { gekisou_skill: Some((2, 1)), ..Default::default() }, 64).is_none()
        );
        assert!(
            run(&master(true, false), Performer { support_skills: vec![(1, 1)], ..Default::default() }, 64).is_none()
        );
    }

    #[test]
    fn proven_life_range_answers_life_conditions_without_branches() {
        let (notes, params, setup, play, dt) = fixture();
        let master = master(false, false);
        // gauge row: LifeAtLeast(700) and a probability, both scripted unless the life range decides the first
        let deck = [Performer { gekisou_skill: Some((2, 1)), ..Default::default() }];
        let run = |budget, life| {
            rush_frames_why(&master, &notes, params, &setup, &play, &dt, &deck, 7, budget, life).unwrap()
        };
        let least = |life| (1..=64).find(|&budget| run(budget, life).is_ok()).expect("some budget completes");
        let wide_runs = least(None);
        assert!(least(Some((700, 1400))) < wide_runs);
        assert!(least(Some((0, 699))) < least(Some((700, 1400))));
        assert_eq!(least(Some((600, 800))), wide_runs);
        assert!(matches!(run(wide_runs - 1, None), Err(RushDecline::Runs(_))));
        let wide = run(64, None).unwrap();
        let high = run(64, Some((700, 1400))).unwrap();
        let low = run(64, Some((0, 699))).unwrap();
        for gate in 0..4 {
            for (masks, name) in [(&high, "high"), (&low, "low")] {
                assert!(masks.flags[gate].iter().zip(&wide.flags[gate]).all(|(&a, &b)| !a || b), "{name} {gate}");
                assert!(masks.max_runs[gate] <= wide.max_runs[gate]);
            }
        }
        let script = Script { life: Some((700, 700)), ..Script::default() };
        assert_eq!(script.life_decides(2000, 699), Some(true));
        assert_eq!(script.life_decides(2000, 700), Some(false));
        assert_eq!(script.life_decides(2001, 700), Some(true));
        assert_eq!(script.life_decides(2001, 701), Some(false));
        assert_eq!(script.life_decides(2002, 701), Some(true));
        assert_eq!(script.life_decides(2002, 700), Some(false));
        assert_eq!(script.life_decides(2003, 700), Some(true));
        assert_eq!(script.life_decides(2003, 699), Some(false));
        assert_eq!(script.life_decides(2004, 700), None);
        let open = Script { life: Some((600, 800)), ..Script::default() };
        assert_eq!(open.life_decides(2001, 700), None);
    }

    #[test]
    fn deterministic_replay_is_identical_and_scripts_enclose_complete_engine_runs() {
        let master = master(false, false);
        let (notes, params, setup, play, dt) = fixture();
        let decks = [
            vec![Performer::default()],
            vec![Performer { gekisou_skill: Some((1, 1)), ..Default::default() }],
            vec![Performer {
                gekisou_skill: Some((2, 1)),
                band_id: 1,
                gekisou_support_skills: vec![(1, 1), (2, 1)],
                ..Default::default()
            }],
        ];
        for (case, deck) in decks.iter().enumerate() {
            for seed in [-1, 0, 1, 17] {
                let masks = rush_frames(&master, &notes, params, &setup, &play, &dt, deck, seed, 128, None)
                    .unwrap()
                    .expect("complete scripts");
                let mut model = LiveModel::new_gekisou(&master, deck, &notes, &[], params, &setup).unwrap();
                model.random = LiveRandom::new(seed);
                let mut actual = RushProbes::new();
                for (frame, &delta) in play.frames.iter().zip(&dt) {
                    // Probe the complete engine at the exact pre-skills boundary, without the diagnostic hook.
                    // This fixture has no live/ordinary skills or grade conversions; before_update has no
                    // grade-driven effect before the skills phase.
                    model.begin_frame_internal(frame.time_ms, delta).unwrap();
                    actual.observe(&mut model).unwrap();
                    let results: Vec<_> = frame
                        .judged
                        .iter()
                        .map(|j| {
                            let note = *model.notes.get(&j.note_id).unwrap();
                            let converted = model
                                .conversion
                                .convert(j.judgement, note.judgement_type, j.judgement_time_ms)
                                .unwrap();
                            model.combo.add_judgement(note.time_ms, converted).unwrap();
                            model.judged.push((note.note_id, converted, note.time_ms));
                            (note, converted)
                        })
                        .collect();
                    model.finish_frame_internal(frame.time_ms, results, &[]).unwrap();
                }
                for gate in 0..4 {
                    assert_eq!(masks.flags[gate].len(), play.frames.len());
                    if case < 2 {
                        assert_eq!(actual.masks[gate], masks.flags[gate], "case={case} seed={seed} gate={gate}");
                        assert_eq!(
                            actual.runs[gate].starts, masks.max_runs[gate],
                            "case={case} seed={seed} gate={gate}"
                        );
                    }
                    assert!(
                        actual.masks[gate].iter().zip(&masks.flags[gate]).all(|(&a, &b)| !a || b),
                        "case={case} seed={seed} gate={gate}"
                    );
                    assert!(actual.runs[gate].starts <= masks.max_runs[gate], "case={case} seed={seed} gate={gate}");
                }
                // The complete live follows one branch exactly: its flags and runs are that branch's, and its
                // spans lie inside the branch's. The branches' union is the union mask.
                let branches = rush_branches_why(&master, &notes, params, &setup, &play, &dt, deck, seed, 128, None)
                    .unwrap()
                    .expect("complete scripts");
                let spans = RushMasks::merge_spans(model.rush_spans());
                let inside = |b: &RushMasks| {
                    b.spans
                        .as_ref()
                        .is_some_and(|s| spans.iter().all(|&(a0, a1)| s.iter().any(|&(b0, b1)| b0 <= a0 && a1 <= b1)))
                };
                assert!(
                    branches.iter().any(|b| (0..4)
                        .all(|g| b.flags[g] == actual.masks[g] && b.max_runs[g] == actual.runs[g].starts)
                        && inside(b)),
                    "case={case} seed={seed}"
                );
                let mut union = RushMasks::empty(play.frames.len());
                for b in &branches {
                    assert!(union.union_with(b));
                }
                assert_eq!(union, masks, "case={case} seed={seed}");
            }
        }
    }
}
