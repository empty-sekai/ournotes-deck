//! Skill rows, their static conditions and the classification environment.
use super::*;

/// Possible results of a condition (or group) for one performer, and whether asking it can change anything else
/// (a draw from the random stream, a life query when life is not rigid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Out {
    pub(super) yes: bool,
    pub(super) no: bool,
    pub(super) impure: bool,
}

impl Out {
    pub(super) fn not(self) -> Out {
        Out { yes: self.no, no: self.yes, impure: self.impure }
    }
    pub(super) fn decided_true(self) -> bool {
        self.yes && !self.no && !self.impure
    }
}

/// Keep the full member view: tags, categories and mission can all affect a target.
pub(super) type Attr<'a> = &'a MemberView;

pub(super) fn no_value() -> Error {
    Error::Master("skill condition without a value".into())
}

/// Use the same complete live predicate as the frame simulator.
pub(super) fn target_matches(tg: &SkillTargetRow, a: Attr<'_>) -> Result<bool, Error> {
    Ok(performer(a, None)?.matches_skill_target(tg))
}

/// The judgement a convert effect converts to (-1: none).
pub(super) fn convert_to(effect_type: i64, value: i64) -> i32 {
    if effect_type == 13005 {
        return 6;
    }
    let v = value as i32;
    if (v.wrapping_sub(1) as u32) > 5 { -1 } else { v }
}

/// Whether a sustained effect has an activation time (the simulation rejects that).
pub(super) fn has_activation_time(act: f32) -> bool {
    if act.is_nan() || act <= 0f32 {
        return false;
    }
    let big = 2147483647f32;
    let m = act.abs().max(big);
    let tol = (m * 1e-6f32).max(f32::from_bits(1) * 8f32);
    tol <= (big - act).abs()
}

/// The facts of the chart, the play and the allowed cards that the classification reads.
pub(super) struct Env<'a> {
    pub(super) master: &'a Master,
    pub(super) events: &'a [(i32, i32)],
    /// Condition sets of each group, in table order.
    pub(super) sets: HashMap<i64, Vec<&'a [i64]>>,
    /// Every life the live can reach lies in `[life_lo, life_hi]`.
    pub(super) life_lo: i64,
    pub(super) life_hi: i64,
    /// Every life condition any allowed card can ask is decided on `[life_lo, life_hi]`, and the note score's life
    /// factor is fixed: then life values cannot change the score.
    pub(super) life_rigid: bool,
    /// Raw judgements of the stream.
    pub(super) raw: Vec<i32>,
    /// Whole-pool judgement closure, ignoring timing/conditions, for necessary count triggers.
    pub(super) count_reach: [u8; 8],
    /// Per stream entry (processing order), the judgements it can end as under the allowed cards' conversions and
    /// their registration windows, closed transitively. Empty until those windows are known; then `count_reach`.
    pub(super) entry_reach: Vec<u8>,
    /// With Gekisou on: the missions of the ranges, whether some range completes in the play, and whether some
    /// reachable judgement is a Miss or a Bad.
    pub(super) gk: Option<GkEnv>,
    /// With Gekisou on: the per-frame facts the windows of Gekisou rows read.
    pub(super) gkf: Option<Rc<GkFrames>>,
    pub(super) rush_cache: RefCell<HashMap<GkWindowKey, Option<Rc<rush::RushSpec>>>>,
    /// The windows of Gekisou rows by (trigger, trigger type, gate, activation time bits, release).
    pub(super) gk_cache: RefCell<HashMap<GkWindowKey, Rc<GkRowWin>>>,
    /// The conversion budgets of Gekisou rows by the fields `gk_budget` reads.
    pub(super) budget_cache: RefCell<HashMap<BudgetKey, Option<f64>>>,
    pub(super) ramp_cache: RefCell<HashMap<ramp::RampKey, Option<ramp::RampWindows>>>,
}
/// The Gekisou facts the classification reads.
#[derive(Clone, Debug)]
pub(super) struct GkEnv {
    pub(super) missions: Vec<i64>,
    pub(super) completes: bool,
    pub(super) breaks: bool,
}

impl GkEnv {
    pub(super) fn has(&self, mission: i64) -> bool {
        self.missions.contains(&mission)
    }

    /// Whether a mission target list (none or All: any) matches some range.
    pub(super) fn any_of(&self, targets: &[i64]) -> bool {
        if targets.is_empty() || targets.contains(&MISSION_ALL) {
            return !self.missions.is_empty();
        }
        targets.iter().any(|m| self.has(*m))
    }
}
impl Env<'_> {
    /// The judgements entry `i` (processing order, raw judgement `raw` in 0..8) can end as.
    pub(super) fn reach_of(&self, i: usize, raw: i32) -> u8 {
        self.entry_reach.get(i).copied().unwrap_or(self.count_reach[raw as usize])
    }
    pub(super) fn cond(&self, cid: i64, a: Attr<'_>) -> Result<Option<Out>, Error> {
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
            t @ (7005 | 7010 | 7012 | 7013 | 7020 | 7021) => {
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
                    7012 => g.completes && v0.unwrap_or(1) >= 1,
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
    pub(super) fn group(&self, gid: i64, a: Attr<'_>) -> Result<Option<Out>, Error> {
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
    pub(super) fn event_only(&self, gid: i64) -> bool {
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

    /// Whether a trigger group has a set that keeps a condition and every such set requires a positive Gekisou
    /// range-start condition: it can hold only in the first play frame at or after a range's start time, and it
    /// reports that start time or the frame time.
    pub(super) fn range_start_only(&self, gid: i64) -> bool {
        let Some(sets) = self.sets.get(&gid) else { return false };
        let kept = |c: i64| self.master.skill_condition(c).is_none_or(|r| r.condition_type != 0);
        let starts = |c: i64| self.master.skill_condition(c).is_some_and(|r| r.condition_type == 7010 && r.is_positive);
        let mut any = false;
        for s in sets.iter().filter(|s| s.iter().any(|&c| kept(c))) {
            if !s.iter().any(|&c| starts(c)) {
                return false;
            }
            any = true;
        }
        any
    }

    /// Whether two condition groups are one condition each, the same except that one is negated: checked in the
    /// same frame and phase, at most one of them holds.
    pub(super) fn negations(&self, g1: i64, g2: i64) -> bool {
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
    pub(super) fn cumulative_fails(&self, cid: i64) -> Result<bool, Error> {
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
    pub(super) fn draws(&self, gid: i64) -> bool {
        gid != 0
            && self.sets.get(&gid).is_some_and(|v| {
                v.iter()
                    .flat_map(|s| s.iter())
                    .any(|&c| self.master.skill_condition(c).is_some_and(|r| r.condition_type == 4011))
            })
    }

    pub(super) fn targets(&self, ids: &[i64]) -> Result<Vec<i64>, Error> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RowSource {
    Live,
    Support,
    Gekisou,
    GekisouSupport,
}

/// The actual source row, independent of how many allowed cards reference it.
#[derive(Clone, Copy, Debug)]
pub(super) struct RowIdentity {
    pub(super) source: RowSource,
    pub(super) index: usize,
    pub(super) id: i64,
}

/// An effect row of a live skill or a snap skill, as the classification and the bounds read it.
#[derive(Clone, Debug)]
pub(super) struct Row {
    pub(super) identity: RowIdentity,
    pub(super) trigger_type: i64,
    pub(super) trigger: i64,
    pub(super) condition: i64,
    pub(super) release: i64,
    pub(super) reset: i64,
    pub(super) cumulative: i64,
    pub(super) effect_type: i64,
    pub(super) value: i64,
    pub(super) act: f32,
    pub(super) limit: i64,
    pub(super) execute_limit: i64,
    pub(super) targets: Vec<i64>,
    /// `_maxEffectValue`.
    pub(super) max_value: i64,
    /// A Gekisou or Gekisou support row, and the mission gating its triggers (0: none).
    pub(super) gk: bool,
    pub(super) gate: i64,
}

/// Every condition key `100 * id + 10 * kind + position`, for kinds 3..=5
/// and positions 0..=4, preserves the raw row order without integer wrapping.
fn check_condition_key(id: i64) -> Result<(), Error> {
    if id.checked_mul(100).and_then(|base| base.checked_add(54)).is_none() {
        return Err(Error::Domain("condition effect key outside the certified integer range".into()));
    }
    Ok(())
}

/// One Snap's skills of a kind have no separate native key namespace per skill
/// slot. Every effect identity must occur only once in that performer's program.
pub(super) fn check_snap_program_identities<'r>(rows: impl Iterator<Item = &'r Row>) -> Result<(), Error> {
    let mut identities = std::collections::HashSet::new();
    for row in rows {
        if !identities.insert((row.identity.source, row.identity.id)) {
            return Err(Error::Domain("a Snap skill program repeats a native effect identity".into()));
        }
    }
    Ok(())
}

/// Row IDs identify native effect states within a source, and conversion target
/// caches across sources. Repeated references to the same source row are valid.
pub(super) fn check_row_identities<'r>(rows: impl Iterator<Item = &'r Row>) -> Result<(), Error> {
    let mut sources = HashMap::new();
    let mut conversions = HashMap::<i64, &'r [i64]>::new();
    for row in rows {
        let RowIdentity { source, index, id } = row.identity;
        if let Some(previous) = sources.insert((source, id), index)
            && previous != index
        {
            return Err(Error::Domain("distinct skill effect rows share a native identity".into()));
        }
        if matches!(row.effect_type, 12006 | 13005)
            && let Some(previous) = conversions.insert(id, &row.targets)
            && previous != row.targets.as_slice()
        {
            return Err(Error::Domain("conversion effect ID has inconsistent native targets".into()));
        }
    }
    Ok(())
}

/// The rows of a support skill at a level, by id.
pub(super) fn support_rows(env: &Env, id: i64, level: i64) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> = env
        .master
        .support_skill_effects
        .iter()
        .enumerate()
        .filter(|(_, r)| r.support_skill_id == id && r.level == level)
        .collect();
    rows.sort_by_key(|(_, r)| r.id);
    rows.iter()
        .map(|&(index, r)| {
            check_condition_key(r.id)?;
            let targets = if matches!(r.skill_effect_type, 2004 | 12006 | 13005) {
                env.targets(&r.skill_target_ids)?
            } else {
                Vec::new()
            };
            Ok(Row {
                identity: RowIdentity { source: RowSource::Support, index, id: r.id },
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
pub(super) fn gekisou_rows(
    env: &Env,
    table: &[GekisouSkillEffectRow],
    source: RowSource,
    id: i64,
    level: i64,
    gate: i64,
) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> = table.iter().enumerate().filter(|(_, r)| r.skill_id == id && r.level == level).collect();
    rows.sort_by_key(|(_, r)| r.id);
    rows.iter()
        .map(|&(index, r)| {
            check_condition_key(r.id)?;
            let targets = if matches!(r.skill_effect_type, 2004 | 12006 | 13005) {
                env.targets(&r.skill_target_ids)?
            } else {
                Vec::new()
            };
            Ok(Row {
                identity: RowIdentity { source, index, id: r.id },
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
pub(super) fn live_rows(env: &Env, id: i64, level: i64) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> = env
        .master
        .live_skill_effects
        .iter()
        .enumerate()
        .filter(|(_, r)| r.live_skill_id == id && r.level == level)
        .collect();
    rows.sort_by_key(|(_, r)| r.id);
    rows.iter()
        .map(|&(index, r)| {
            if r.skill_release_condition_group != 0 || r.skill_cumulative_condition_id != 0 || r.effect_limit_count != 0
            {
                return Err(Error::Unsupported(format!(
                    "live skill effect {}: release condition, cumulative condition or effect limit",
                    r.id
                )));
            }
            if !matches!(r.skill_effect_type, 2000 | 2004 | 3001 | 3003 | 3004 | 12006 | 13005 | 15000) {
                return Err(Error::Unsupported(format!("skill effect type {}", r.skill_effect_type)));
            }
            let targets = if matches!(r.skill_effect_type, 2004 | 12006 | 13005) {
                env.targets(&r.skill_target_ids)?
            } else {
                Vec::new()
            };
            Ok(Row {
                identity: RowIdentity { source: RowSource::Live, index, id: r.id },
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
pub(super) enum Status {
    /// Never starts, and checking it changes nothing.
    Never,
    /// May start, but cannot change the score under this play and pool.
    Inert,
    /// May change the score.
    Active,
}

/// The part of a row that identifies it in the simulation, with a decided pure condition replaced by "none".
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct RowSig {
    pub(super) trigger_type: i64,
    pub(super) trigger: i64,
    pub(super) condition: i64,
    pub(super) release: i64,
    pub(super) reset: i64,
    pub(super) cumulative: i64,
    pub(super) effect_type: i64,
    pub(super) value: i64,
    pub(super) act: u32,
    pub(super) limit: i64,
    pub(super) execute_limit: i64,
    pub(super) targets: Vec<i64>,
    pub(super) max_value: i64,
}

/// An active snap row as the bounds read it.
#[derive(Clone, Debug)]
pub(super) struct ActiveRow {
    pub(super) effect_type: i64,
    pub(super) value: i64,
    pub(super) act: f32,
    /// The trigger is the performer's own skill event (starts in the frames where it fires).
    pub(super) event_bound: bool,
    /// Trigger and condition can both hold.
    pub(super) can_start: bool,
    /// Whole-play starts of an ordinary one-shot fixed score factor, from necessary positive judgement
    /// counters in every trigger alternative. Conditions, resets and updater availability only remove starts.
    pub(super) start_limit: Option<f64>,
    /// One window per hit frame of a lone ordinary judgement counter whose original-target multiplicity is
    /// invariant across every reachable final grade. Each frame starts at most one pool instance.
    pub(super) count_win: Option<Vec<(i64, i64, f64)>>,
    pub(super) targets: Vec<i64>,
    /// A cumulative note score up (2001) whose factor changes while it runs (`value` is its largest value), and the
    /// most changes of one execution (`None`: any frame).
    pub(super) churn: bool,
    pub(super) churn_max: Option<f64>,
    /// A Gekisou row: its factor windows in chart time `(start, end, concurrent executions)` and the play-frame
    /// index ranges `(a, b]` whose judgements its conversion can see.
    pub(super) gk_win: Option<Vec<(i64, i64, f64)>>,
    /// The executions each window of `gk_win` covers.
    pub(super) gk_starts: Option<Vec<Rc<RampStarts>>>,
    pub(super) gk_execs: Option<Vec<f64>>,
    /// A sustained combo bonus started by one Gekisou combo count condition: its threshold and its span components
    /// `(start, end, playing range of every start frame)` (see `combo_gate`).
    pub(super) gk_gate: Option<(i64, Vec<SpanPart>)>,
    /// Exact own-event trigger timing, still relaxing conditions and execution limits.
    pub(super) gk_event_win: Option<[Vec<(i64, i64, f64)>; 5]>,
    pub(super) gk_conv: Option<Vec<(i64, i64)>>,
    /// A Gekisou conversion row whose conversions in the play are fewer than the entries it can see: at most this
    /// many (see `gk_budget`).
    pub(super) budget: Option<f64>,
    /// Per-execution judgement-count caps, with initial backdating preserved.
    pub(super) cumulative_ramp: Option<ramp::RampWindows>,
    /// A cumulative note score up counting the playing range's combo: unit, count cap and note factor table.
    pub(super) combo_ramp: Option<(i64, i64, Rc<Vec<f64>>)>,
    pub(super) rush: Option<Rc<rush::RushSpec>>,
    pub(super) rush_run_cap: bool,
}

/// A live skill row of a member with its condition result and the row whose condition is its negation, if any.
#[derive(Clone, Debug)]
pub(super) struct LiveRow {
    pub(super) row: Row,
    pub(super) out: Option<Out>,
    pub(super) partner: Option<usize>,
}

/// Effect types a Gekisou or Gekisou support row may have.
pub(super) const GK_TYPES: [i64; 19] = [
    2000, 2001, 2004, 3001, 3003, 3004, 4004, 11000, 11001, 11002, 11003, 11005, 12000, 12004, 12006, 13000, 13002,
    13005, 15000,
];

/// Classification of one row for a member.
pub(super) fn support_status(env: &Env, r: &Row, a: Attr<'_>) -> Result<(Status, bool, bool), Error> {
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
    // A release checker skips the first elapsed-time check and can retain a zero-duration effect. The compact
    // own-event window requires the ordinary timer transition; other rows keep the full-stream envelope.
    let event_bound = r.release == 0 && env.event_only(r.trigger);
    let modelled = if r.gk {
        GK_TYPES.contains(&r.effect_type)
    } else {
        matches!(r.effect_type, 2000 | 2004 | 3001 | 3003 | 3004 | 12006 | 13005 | 15000)
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
        3001 | 3003 | 3004 => {
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
