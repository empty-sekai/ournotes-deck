//! The effect, skill and score-command state of a Live model.

use super::*;

/// The effect row data the appliers read.
#[derive(Clone, Debug)]
pub(super) struct EffectRow {
    pub(super) id: i64,
    pub(super) effect_type: i64,
    pub(super) effect_value: i64,
    pub(super) max_effect_value: i64,
    pub(super) effect_limit_count: i64,
    /// Judgements of the effect's targets, or the first target id that is not in the master.
    pub(super) targets: Result<Vec<i64>, i64>,
    pub(super) applier: applier_plan::ApplierPlan,
}

impl EffectRow {
    pub(super) fn targets(&self) -> Result<&[i64], Error> {
        self.targets.as_deref().map_err(|&id| Error::Master(format!("unknown skill target {id}")))
    }
}

/// Identity of an effect state for the appliers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum StateKey {
    Live { row_id: i64, member: usize, index: usize },
    Cond { effect_id: i64, index: usize },
}

#[derive(Clone, Debug)]
pub(super) struct LiveEffect {
    pub(super) row: usize,
    pub(super) act: f32,
    pub(super) phase: i64,
    pub(super) state: EffectState,
    pub(super) condition: Option<Checker>,
    pub(super) release: Option<Checker>,
    pub(super) cumulative: Option<Cumulative>,
}

impl LiveEffect {
    pub(super) fn update(
        &mut self,
        inp: FrameInput,
        trigger: TriggerResult,
        ctx: &mut CheckCtx,
    ) -> Result<bool, Error> {
        let before = self.state.state;
        let checkers = UpdateCheckers { condition: self.condition.as_mut(), release: self.release.as_mut() };
        let changed = effect_update(&mut self.state, self.act, inp, trigger, checkers, false, ctx)?;
        if let Some(c) = self.cumulative.as_mut() {
            let after = self.state.state;
            if (before == STAY && after == EXECUTE_FRAME)
                || before == EXECUTE_FRAME
                || (before == EXECUTING && after == EXECUTING)
            {
                self.state.cumulative_count = c.update_count(ctx)?;
            } else if before == END_FRAME {
                c.reset();
            }
        }
        Ok(changed)
    }
}

#[derive(Clone, Debug)]
pub(super) struct LiveSkill {
    pub(super) key: i64,
    pub(super) member: usize,
    pub(super) index: usize,
    pub(super) parent_state: u8,
    pub(super) trigger: TriggerResult,
    pub(super) effects: Vec<LiveEffect>,
}

#[derive(Clone, Debug)]
pub(super) struct LivePool {
    pub(super) key: i64,
    pub(super) member: usize,
    pub(super) available: VecDeque<usize>,
}

pub(super) fn aggregate_live_state(effects: &[LiveEffect]) -> u8 {
    for state in [EXECUTING, EXECUTE_FRAME, END_FRAME] {
        if effects.iter().any(|e| e.state.state == state) {
            return state;
        }
    }
    STAY
}

/// A condition skill of one member: snap (3), Gekisou (4) or Gekisou support (5).
#[derive(Clone, Debug)]
pub(super) struct CondSkill {
    pub(super) member: usize,
    pub(super) skill_type: i64,
    /// The list a Gekisou skill joins (its mission group); 0 for the other skill types.
    pub(super) group: i64,
    pub(super) updater: ConditionSkillUpdater,
}

impl CondSkill {
    /// The position in the updater list: snap skills by member, Gekisou skills by mission group then member,
    /// Gekisou support skills by member; one member's skills of a type keep their order.
    pub(super) fn list_key(&self) -> (i64, i64, usize) {
        (self.skill_type, self.group, self.member)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Listed {
    Live { skill: usize, effect: usize },
    Cond { updater: usize, u: usize },
}

/// Per-frame buffers kept by the live so that a frame allocates nothing; their contents never outlive the frame.
#[derive(Clone, Debug, Default)]
pub(super) struct FrameScratch {
    pub(super) results: Vec<(LiveNote, i32)>,
    pub(super) listed: Vec<Listed>,
    pub(super) updated: Vec<usize>,
    pub(super) gk_judged: Vec<GkNote>,
}

/// Score factor handles: note / combo score up by id and the Gekisou luck (rush) bonus.
#[derive(Clone, Debug, Default)]
pub(super) struct ScoreCtl {
    counter: i32,
    cmds: FxHashMap<i32, FactorCommand>,
}

impl ScoreCtl {
    fn put(&mut self, score: &mut IncrementalCalculator, cmd: FactorCommand) -> i32 {
        self.counter = self.counter.wrapping_add(1);
        self.cmds.insert(self.counter, cmd);
        score.add_factor(cmd);
        self.counter
    }

    fn take(&mut self, id: i32) -> Result<FactorCommand, Error> {
        self.cmds.remove(&id).ok_or_else(|| Error::Game(format!("score factor {id} not found")))
    }

    pub(super) fn add_note_score_up(
        &mut self,
        score: &mut IncrementalCalculator,
        owner: i32,
        t: i32,
        factor: f32,
    ) -> i32 {
        let cmd =
            FactorCommand { time_ms: t, owner_id: owner, note_mill: note_factor_mill(factor), ..Default::default() };
        self.put(score, cmd)
    }

    pub(super) fn disable_note_score_up(
        &mut self,
        score: &mut IncrementalCalculator,
        t: i32,
        id: i32,
    ) -> Result<(), Error> {
        let c = self.take(id)?;
        score.add_factor(FactorCommand {
            time_ms: t,
            owner_id: c.owner_id,
            note_mill: c.note_mill.wrapping_neg(),
            ..Default::default()
        });
        Ok(())
    }

    pub(super) fn add_combo_bonus(
        &mut self,
        score: &mut IncrementalCalculator,
        owner: i32,
        t: i32,
        factor: f32,
    ) -> i32 {
        let cmd = FactorCommand {
            time_ms: t,
            owner_id: owner,
            combo_mill: judgement_factor_mill(factor),
            ..Default::default()
        };
        self.put(score, cmd)
    }

    pub(super) fn disable_combo_bonus(
        &mut self,
        score: &mut IncrementalCalculator,
        t: i32,
        id: i32,
    ) -> Result<(), Error> {
        let c = self.take(id)?;
        score.add_factor(FactorCommand {
            time_ms: t,
            owner_id: c.owner_id,
            combo_mill: c.combo_mill.wrapping_neg(),
            ..Default::default()
        });
        Ok(())
    }

    pub(super) fn combo_bonus_factor(&self, id: i32) -> f32 {
        match self.cmds.get(&id) {
            Some(c) if c.combo_mill != 0 => c.combo_mill as f32 / 100000f32,
            _ => 0f32,
        }
    }

    pub(super) fn score_up_factor(&self, id: i32) -> f32 {
        match self.cmds.get(&id) {
            Some(c) if c.note_mill != 0 => c.note_mill as f32 / 100000f32,
            _ => 0f32,
        }
    }
}

/// The luck handle of the Gekisou controller: rush bonus commands through the score controller.
pub(super) struct Handle<'a> {
    pub(super) sc: &'a mut ScoreCtl,
    pub(super) score: &'a mut IncrementalCalculator,
}

impl LuckHandle for Handle<'_> {
    fn add(&mut self, t: i32, percent: i32) -> i32 {
        self.sc.put(self.score, FactorCommand { time_ms: t, owner_id: -1, luck: percent, ..Default::default() })
    }

    fn disable(&mut self, t: i32, id: i32) -> Result<(), Error> {
        let c = self.sc.take(id)?;
        self.score.add_factor(FactorCommand {
            time_ms: t,
            owner_id: -1,
            luck: c.luck.wrapping_neg(),
            ..Default::default()
        });
        Ok(())
    }
}

/// The Gekisou part of a live.
#[derive(Clone, Debug)]
pub(super) struct GekisouLive {
    pub(super) ctrl: Controller,
    pub(super) fever: FeverUpdater,
    pub(super) factors: [[i64; 5]; 3],
    pub(super) external_ranking: bool,
    pub(super) solo_score_queries: bool,
    pub(super) completed_scores: Vec<bool>,
    pub(super) pending_ranks: Vec<Option<(i32, i64)>>,
    pub(super) fever_updates: Vec<(usize, u8)>,
    pub(super) prev_lots: Vec<i64>,
    pub(super) prev_lot_ms: i32,
    /// `(range, rank, bonus, percent)` of each confirmed range.
    pub(super) rank_bonus: Vec<(usize, i32, i32, i64)>,
    pub(super) rank_applications: Vec<(usize, usize)>,
    pub(super) program_rank_snapshots: Vec<(Option<score_program::ValueId>, Option<score_program::ValueId>)>,
    pub(super) rank_snapshot_queries: Vec<(Option<usize>, Option<usize>)>,
}

/// Per-state bookkeeping of the Gekisou appliers.
#[derive(Clone, Debug, Default)]
pub(super) struct GkAppliers {
    pub(super) ids: FxHashMap<StateKey, i32>,
    pub(super) exhausted: FxHashSet<StateKey>,
    pub(super) limit_finished: FxHashMap<StateKey, i32>,
}

impl GkAppliers {
    pub(super) fn add(&mut self, key: StateKey, id: i32) -> Result<(), Error> {
        if self.ids.contains_key(&key) {
            return Err(Error::Game("effect state registered twice".into()));
        }
        self.ids.insert(key, id);
        Ok(())
    }

    pub(super) fn pop(&mut self, key: StateKey) -> Result<i32, Error> {
        self.ids.remove(&key).ok_or_else(|| Error::Game("effect state not registered".into()))
    }

    /// Ends the state at the time its limit ran out, when that was recorded.
    pub(super) fn limit(&mut self, key: StateKey, st: &mut EffectState) {
        if let Some(t) = self.limit_finished.remove(&key) {
            st.state = END_FRAME;
            st.finish_ms = t;
        }
    }
}
