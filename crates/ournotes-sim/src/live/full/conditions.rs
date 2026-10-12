//! Skill condition checkers and the factory that builds them from the condition tables.
//!
//! A checker answers `(hit, hit count)` and may carry an override time (the trigger time of a condition that counts
//! notes is the chart time of the note that completed the count). Condition groups are an OR over their condition
//! sets, each set an AND over its conditions; both stop at the first decisive item.
//!
//! The Gekisou conditions (7005, 7010, 7013, 7020, 7021) read the Gekisou controller and fail when asked in a live
//! without Gekisou; the lottery result condition (7000) never hits there.

use super::Performer;
use super::gekisou::{Controller, M_ALL, M_LUCK, S_COMPLETE, S_FINISH, S_PLAYING, S_START};
use super::life::LifeController;
use crate::error::Error;
use crate::live::random::{LiveRandom, SKILL};
use crate::master::{Master, SkillTargetRow};
use crate::num::{ceil_to_i32, floor_to_i32};

mod check;
mod cumulative;
mod factory;
pub(crate) use cumulative::Cumulative;
pub(crate) use factory::Factory;
use factory::no_value;

/// The Gekisou state the checkers read in a frame.
#[derive(Clone, Copy)]
pub(crate) struct GkView<'a> {
    pub ctrl: &'a Controller,
    /// The previous frame's lottery results.
    pub prev_lots: &'a [i64],
    /// The previous frame's time when it drew lottery results, else 0.
    pub prev_lot_ms: i32,
}

/// What a checker may read or advance while it is asked.
pub(crate) struct CheckCtx<'a> {
    /// Previous-frame rank confirmation, independent of Gekisou-controller presence.
    pub prev_confirmed_rank: Option<i32>,
    pub life: &'a mut LifeController,
    pub random: &'a mut LiveRandom,
    /// Music time of the frame.
    pub frame_time: i32,
    pub current_combo: i32,
    /// The frame's judged notes: `(note id, converted judgement, chart time)`.
    pub judged: &'a [(i32, i32, i32)],
    /// The frame's fired skill events: `(event index, event time)`.
    pub events: &'a [(i32, i32)],
    /// The Gekisou state (`None` without Gekisou).
    pub gk: Option<GkView<'a>>,
}

impl CheckCtx<'_> {
    fn ctrl(&self, t: i64) -> Result<&Controller, Error> {
        self.gk.map(|g| g.ctrl).ok_or_else(|| {
            Error::Unsupported(format!("condition type {t} reads the Gekisou state of a live without Gekisou"))
        })
    }
}

/// Target type of Gekisou mission targets.
const TARGET_MISSION: i64 = 5;
/// The judgement value of targets without a judgement.
const NO_JUDGEMENT: i64 = -1;

#[derive(Clone, Debug)]
pub(crate) enum Checker {
    /// Nondeterministic input used only by the conservative LUCK replay. Static formation predicates retain
    /// their first answer across resets; life/probability predicates may answer independently at each check.
    Scripted {
        script: super::luck::SharedScript,
        sticky: bool,
        memo: Option<bool>,
        true_hits: i64,
        /// A life comparison `(condition type 2000..=2003, value)`, answered without a branch where the script's
        /// proven life range decides it.
        life: Option<(i64, i64)>,
    },
    LiveComboMultiple {
        threshold: i32,
        previous: i32,
    },
    LiveComboAtLeast(i32),
    ElapsedTime {
        period: i32,
        elapsed: i32,
        previous: i32,
    },
    SnapGekisouStart {
        previous: i32,
    },
    And {
        items: Vec<Checker>,
        resettable: Vec<bool>,
    },
    Or(Vec<Checker>),
    Not(Box<Checker>),
    /// Life at the frame time `>= value` (`None`: the condition has no value).
    LifeAtLeast(Option<i64>),
    /// Life at the frame time `<= value`.
    LifeAtMost(Option<i64>),
    LifeGreater(Option<i64>),
    LifeLess(Option<i64>),
    /// A change since the last check; this checker returns a zero hit count even when it hits.
    LifeChanged {
        previous: i32,
    },
    /// A downward crossing of the threshold captured at initialization.
    LifePercent {
        threshold: i32,
        previous: i32,
    },
    /// Positive changes in one direction, accumulated with a remainder between checks.
    LifeDelta {
        threshold: i32,
        increase: bool,
        count: i32,
        previous: i32,
    },
    /// A chart skill event of this performer fired in the frame.
    SameMemberLiveSkill(i32),
    /// One draw of the skill random stream below the rate.
    Probability(f32),
    /// A score-only probability evaluated by an explicit nominal event script. Ordinary seeded lives
    /// never construct this checker; each conditional recording retains the complete updater state.
    ConditionedProbability(f32),
    /// A fixed answer (member targets are decided when the checker is built; score rank up never fires).
    Fixed(bool),
    /// Every `n` judged notes whose judgement is a target; the trigger time is the chart time of the note that
    /// completed the count (when its id is not negative).
    NoteJudgementMatch {
        kind: i64,
        targets: Vec<i64>,
        override_ms: Option<i32>,
    },
    NoteJudgementCount {
        n: i64,
        consecutive: bool,
        targets: Vec<i64>,
        count: i32,
        override_ms: Option<i32>,
    },
    /// The previous frame drew this lottery result (hit count: how often); trigger time: that frame's time.
    LuckLotResult {
        target: i64,
        override_ms: Option<i32>,
    },
    /// The playing range's Gekisou combo is at least the threshold; trigger time: its last combo judgement.
    ComboAtLeast {
        threshold: i64,
        override_ms: Option<i32>,
    },
    /// 7001/7002/7004: crossings of positive multiples of a controller counter.
    GkInterval {
        kind: i64,
        n: i32,
        previous: i32,
        override_ms: Option<i32>,
    },
    /// 7003: the inflated Just count meets a positive threshold.
    JustAtLeast {
        threshold: i32,
        override_ms: Option<i32>,
    },
    /// 7006/7007: one rising edge, rearmed by falling below or changing range.
    GkJustEdge {
        raw: bool,
        threshold: i32,
        previous: i32,
        last_range: i32,
        override_ms: Option<i32>,
    },
    GkLiveStart(bool),
    GkReachRank {
        threshold: i32,
        triggered: bool,
    },
    /// A range of a target mission (none or 4: any) started this frame, once per range until it finishes.
    RangeStart {
        missions: Vec<i64>,
        triggered: Vec<usize>,
        override_ms: Option<i32>,
    },
    /// A range completed this frame.
    RangeComplete,
    /// A range of a target mission is active (started, not yet complete).
    RangePlaying {
        missions: Vec<i64>,
        active: Vec<(usize, i64)>,
        override_ms: Option<i32>,
    },
    /// The playing luck range is in a rush.
    LuckRushPlaying(bool),
}

fn mission_ok(missions: &[i64], mission: i64) -> bool {
    missions.is_empty() || missions.contains(&M_ALL) || missions.contains(&mission)
}

/// Native interval checkers do not reset on a range gap or an index change.
fn gk_interval(previous: &mut i32, n: i32, current: Option<i32>) -> i64 {
    if n <= 0 {
        return 0;
    }
    let Some(current) = current else { return 0 };
    if current <= 0 {
        *previous = 0;
        return 0;
    }
    let old = *previous;
    *previous = current;
    if current <= old {
        return 0;
    }
    i64::from((current / n).wrapping_sub(old / n).max(0))
}

fn gk_just_edge(previous: &mut i32, last_range: &mut i32, threshold: i32, current: Option<(i32, i32)>) -> bool {
    if threshold <= 0 {
        return false;
    }
    let Some((range, count)) = current else {
        *previous = 0;
        *last_range = -1;
        return false;
    };
    if range != *last_range {
        *previous = 0;
        *last_range = range;
    }
    let hit = *previous < threshold && count >= threshold;
    *previous = count;
    hit
}

fn gk_once(triggered: &mut bool, eligible: bool) -> bool {
    let hit = !*triggered && eligible;
    *triggered |= hit;
    hit
}

impl Checker {
    /// Whether this checker or one it combines satisfies `f`.
    pub(crate) fn any(&self, f: &dyn Fn(&Checker) -> bool) -> bool {
        f(self)
            || match self {
                Checker::And { items, .. } | Checker::Or(items) => items.iter().any(|c| c.any(f)),
                Checker::Not(inner) => inner.any(f),
                _ => false,
            }
    }

    /// Replaces each life comparison this checker combines by its answer when every life in `low..=high` gives the
    /// same one. A comparison is monotone in the life, so the answers at the two ends decide it.
    pub(crate) fn fold_life(&mut self, (low, high): (i32, i32)) {
        let same = |value: i64, test: fn(i64, i64) -> bool| {
            let answer = test(i64::from(low), value);
            (answer == test(i64::from(high), value)).then_some(answer)
        };
        let answer = match self {
            Checker::LifeAtLeast(Some(value)) => same(*value, |life, value| life >= value),
            Checker::LifeAtMost(Some(value)) => same(*value, |life, value| life <= value),
            Checker::LifeGreater(Some(value)) => same(*value, |life, value| life > value),
            Checker::LifeLess(Some(value)) => same(*value, |life, value| life < value),
            Checker::And { items, .. } | Checker::Or(items) => {
                items.iter_mut().for_each(|item| item.fold_life((low, high)));
                None
            }
            Checker::Not(inner) => {
                inner.fold_life((low, high));
                None
            }
            _ => None,
        };
        if let Some(answer) = answer {
            *self = Checker::Fixed(answer);
        }
    }

    /// Whether the checker can hold given the fixed answers it combines (formation predicates); every other checker
    /// is taken as able to hold.
    pub(crate) fn may_hold(&self) -> bool {
        match self {
            Checker::Fixed(b) => *b,
            Checker::And { items, .. } => items.iter().all(Checker::may_hold),
            Checker::Or(items) => items.iter().any(Checker::may_hold),
            Checker::Not(inner) => !matches!(**inner, Checker::Fixed(true)),
            _ => true,
        }
    }

    /// Whether an AND resets this item's count when another item fails.
    fn count_resettable(&self) -> bool {
        matches!(
            self,
            Checker::NoteJudgementCount { .. }
                | Checker::LifeDelta { .. }
                | Checker::ElapsedTime { .. }
                | Checker::Not(_)
        )
    }

    /// The trigger time override of the last check, if any.
    pub(crate) fn override_time(&self) -> Option<i32> {
        match self {
            Checker::NoteJudgementMatch { override_ms, .. }
            | Checker::NoteJudgementCount { override_ms, .. }
            | Checker::LuckLotResult { override_ms, .. }
            | Checker::ComboAtLeast { override_ms, .. }
            | Checker::GkInterval { override_ms, .. }
            | Checker::JustAtLeast { override_ms, .. }
            | Checker::GkJustEdge { override_ms, .. }
            | Checker::RangeStart { override_ms, .. }
            | Checker::RangePlaying { override_ms, .. } => *override_ms,
            Checker::Not(inner) => inner.override_time(),
            _ => None,
        }
    }

    /// Moves the checker to other performance positions: a check of position `k` becomes a check of `map[k]`.
    pub(crate) fn move_positions(&mut self, map: &[usize]) {
        match self {
            Checker::SameMemberLiveSkill(k) => {
                if let Some(&to) = usize::try_from(*k).ok().and_then(|i| map.get(i)) {
                    *k = to as i32;
                }
            }
            Checker::And { items, .. } | Checker::Or(items) => items.iter_mut().for_each(|c| c.move_positions(map)),
            Checker::Not(inner) => inner.move_positions(map),
            _ => {}
        }
    }

    pub(crate) fn reset(&mut self, ctx: &mut CheckCtx) -> Result<(), Error> {
        match self {
            Checker::LiveComboMultiple { previous, .. } => *previous = 0,
            Checker::ElapsedTime { elapsed, previous, .. } => {
                *elapsed = 0;
                *previous = ctx.frame_time;
            }
            Checker::SnapGekisouStart { previous } => *previous = -1,
            Checker::And { items, .. } | Checker::Or(items) => {
                for item in items {
                    item.reset(ctx)?;
                }
            }
            Checker::Not(inner) => inner.reset(ctx)?,
            Checker::LifeDelta { count, previous, .. } => {
                *count = 0;
                *previous = ctx.life.get_life_at_ms(ctx.frame_time)?;
            }
            Checker::NoteJudgementMatch { override_ms, .. } => *override_ms = None,
            Checker::NoteJudgementCount { count, .. } => *count = 0,
            Checker::LuckLotResult { override_ms, .. } | Checker::ComboAtLeast { override_ms, .. } => {
                *override_ms = None;
            }
            Checker::GkInterval { previous, override_ms, .. } => {
                *previous = 0;
                *override_ms = None;
            }
            Checker::JustAtLeast { override_ms, .. } => *override_ms = None,
            Checker::GkJustEdge { previous, last_range, override_ms, .. } => {
                *previous = 0;
                *last_range = -1;
                *override_ms = None;
            }
            Checker::GkLiveStart(triggered) | Checker::GkReachRank { triggered, .. } => *triggered = false,
            Checker::RangeStart { triggered, override_ms, .. } => {
                triggered.clear();
                *override_ms = None;
            }
            Checker::RangePlaying { active, override_ms, .. } => {
                active.clear();
                *override_ms = None;
            }
            Checker::LuckRushPlaying(rush) => *rush = false,
            _ => {}
        }
        Ok(())
    }

    fn reset_count(&mut self, ctx: &mut CheckCtx) -> Result<(), Error> {
        match self {
            Checker::NoteJudgementCount { .. } | Checker::LifeDelta { .. } | Checker::ElapsedTime { .. } => {
                self.reset(ctx)?
            }
            Checker::Not(inner) if inner.count_resettable() => inner.reset_count(ctx)?,
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod gekisou_condition_tests;
#[cfg(test)]
mod gekisou_factory_tests;
#[cfg(test)]
mod tests;
