//! Live skills in a played live: the factor commands their effects add and the final score of a judgement stream.
//!
//! Covered: live skills whose effects are note score up (2000) or judgement-targeted note score up (2004), with
//! life conditions (2001, 2003). Support skills and Gekisou are not modelled; effect types or conditions outside
//! this set are reported as [`Error::Unsupported`].

use crate::error::Error;
use crate::live::score::{LiveScoreCalculator, ScoreFactorState, get_frame};
use crate::master::{LiveSkillEffectRow, Master};
use crate::num::{ceil_to_i32, floor_to_i32, floor_to_i32_f64};

/// Owner type of member (live) skills.
pub const OWNER_MEMBER: i32 = 1;
/// Owner type of snap (support) skills.
pub const OWNER_SNAP: i32 = 2;

/// One judged note of a play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePlay {
    pub note_id: i32,
    pub time_ms: i32,
    pub note_type: i32,
    /// Judgement score type (1 Just .. 6 Miss).
    pub score_type: i32,
    /// Life at the judgement.
    pub life: i32,
    /// Combo the score reads for this note.
    pub combo: i32,
}

/// A factor command: signed changes of the factor state at a music time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FactorCommand {
    pub time_ms: i32,
    pub owner_id: i32,
    pub note_mill: i32,
    pub combo_mill: i32,
    /// Note judgement (6 Just, 5 Perfect, 4 Great, 3 Good) of the judgement factor.
    pub judgement: i32,
    pub judge_mill: i32,
    pub band_total_power: i32,
    pub luck: i32,
}

/// Note factor in thousandths of a percent: `floor(factor * 100000f)` with the floor conversion's saturation.
pub fn note_factor_mill(factor: f32) -> i32 {
    floor_to_i32(factor * 100000f32)
}

/// Judgement factor in thousandths of a percent: `round((double)(factor * 100000f))` half to even, saturating.
pub fn judgement_factor_mill(factor: f32) -> i32 {
    let x = factor * 100000f32;
    if !x.is_finite() {
        return floor_to_i32(x);
    }
    floor_to_i32_f64((x as f64).round_ties_even())
}

/// Adds a command's changes to the state.
pub fn apply_factor(state: &mut ScoreFactorState, cmd: &FactorCommand) {
    state.band_total_power = state.band_total_power.wrapping_add(cmd.band_total_power);
    let div = |m: i32| if m != 0 { m as f32 / 100000f32 } else { 0f32 };
    let (fc, fn_, fj) = (div(cmd.combo_mill), div(cmd.note_mill), div(cmd.judge_mill));
    if fc != 0.0 {
        state.combo_score_up += fc;
    }
    if fn_ != 0.0 {
        state.note_score_up += fn_;
    }
    match cmd.judgement {
        6 => state.just += fj,
        5 => state.perfect += fj,
        4 => state.great += fj,
        3 => state.good += fj,
        _ => {}
    }
    state.added_luck_bonus = state.added_luck_bonus.wrapping_add(cmd.luck);
}

/// Final score of a play when every command is known: frames in order; inside a frame factor commands by (time,
/// owner) and notes by (time, note id), a note before the next factor only when its time is earlier. Every command
/// is applied once; this is the game's result unless its recalculation replayed frames that already held factor
/// commands, which can change the float state in the last bit.
pub fn score_with_factors(
    calc: &mut LiveScoreCalculator,
    notes: &[NotePlay],
    commands: &[FactorCommand],
    max_frame: i32,
) -> Result<i32, Error> {
    #[derive(Clone, Copy)]
    enum Item<'a> {
        F(&'a FactorCommand),
        N(&'a NotePlay),
    }
    let frame = |t: i32| get_frame(t).min(max_frame.wrapping_sub(1));
    let mut items: Vec<((i32, i32, u8, i32), Item)> = Vec::with_capacity(notes.len() + commands.len());
    for c in commands {
        items.push(((frame(c.time_ms), c.time_ms, 0, c.owner_id), Item::F(c)));
    }
    for n in notes {
        items.push(((frame(n.time_ms), n.time_ms, 1, n.note_id), Item::N(n)));
    }
    items.sort_by_key(|x| x.0);
    let mut total = 0i32;
    for (_, it) in items {
        match it {
            Item::F(c) => apply_factor(&mut calc.state, c),
            Item::N(n) => {
                total =
                    total.wrapping_add(calc.note_score(n.combo, n.life, n.time_ms, n.note_type, n.score_type, None)?)
            }
        }
    }
    Ok(total)
}

fn condition_ok(master: &Master, cid: i64, life: i32) -> Result<bool, Error> {
    let c = master.skill_condition(cid).ok_or_else(|| Error::Master(format!("unknown condition {cid}")))?;
    let v0 =
        || c.condition_values.first().copied().ok_or_else(|| Error::Master(format!("condition {cid} has no value")));
    let ok = match c.condition_type {
        2001 => v0()? <= life as i64,
        2003 => life as i64 <= v0()?,
        t => return Err(Error::Unsupported(format!("live skill condition type {t}"))),
    };
    Ok(if c.is_positive { ok } else { !ok })
}

/// A condition group: an OR over its condition sets, each an AND over its conditions (both short-circuit).
pub fn condition_group_ok(master: &Master, group: i64, life: i32) -> Result<bool, Error> {
    if group == 0 {
        return Ok(true);
    }
    let mut any_set = false;
    for s in master.skill_condition_sets.iter().filter(|s| s.group == group) {
        any_set = true;
        let mut all = true;
        for &cid in &s.condition_ids {
            if !condition_ok(master, cid, life)? {
                all = false;
                break;
            }
        }
        if all {
            return Ok(true);
        }
    }
    if !any_set {
        return Err(Error::Master(format!("condition group {group} has no set")));
    }
    Ok(false)
}

/// Effect rows of a live skill at a level, in id order.
pub fn live_skill_rows(master: &Master, live_skill_id: i64, level: i64) -> Vec<&LiveSkillEffectRow> {
    let mut v: Vec<&LiveSkillEffectRow> =
        master.live_skill_effects.iter().filter(|r| r.live_skill_id == live_skill_id && r.level == level).collect();
    v.sort_by_key(|r| r.id);
    v
}

/// Factor commands of the live skills. `events` are the chart's skill events `(index, time)`; `performers[index]`
/// is the `(live skill id, level)` of the member at performance position `index`; `life_at_event[index]` is the life
/// in the frame where event `index` fires; `extension_ms[index]` extends that event's effects.
pub fn live_skill_commands(
    master: &Master,
    events: &[(i32, i32)],
    performers: &[(i64, i64)],
    life_at_event: &[i32],
    music_length_ms: i32,
    extension_ms: Option<&[f32]>,
) -> Result<Vec<FactorCommand>, Error> {
    let mut out = Vec::new();
    for &(index, t) in events {
        let k = usize::try_from(index).map_err(|_| Error::Input(format!("skill event index {index}")))?;
        let &(skill_id, level) =
            performers.get(k).ok_or_else(|| Error::Input(format!("no performer for skill event {index}")))?;
        let life = *life_at_event.get(k).ok_or_else(|| Error::Input(format!("no life for skill event {index}")))?;
        let ext = extension_ms.and_then(|e| e.get(k).copied()).unwrap_or(0.0);
        let owner = index.wrapping_mul(100).wrapping_add(OWNER_MEMBER);
        for r in live_skill_rows(master, skill_id, level) {
            if r.skill_release_condition_group != 0
                || r.effect_limit_count != 0
                || r.effect_execute_limit_count != 0
                || r.skill_cumulative_condition_id != 0
                || r.max_effect_value != 0
            {
                return Err(Error::Unsupported(format!(
                    "live skill effect {}: limit, release or cumulative condition",
                    r.id
                )));
            }
            if !condition_group_ok(master, r.skill_condition_group, life)? {
                continue;
            }
            let dur = (r.activation_time_second * 1000f32) + ext;
            let mut finish = t.wrapping_add(ceil_to_i32(dur));
            if music_length_ms > 0 && music_length_ms <= finish {
                finish = music_length_ms;
            }
            let factor = r.effect_value as f32 / 10000f32;
            match r.skill_effect_type {
                2000 => {
                    let m = note_factor_mill(factor);
                    out.push(FactorCommand { time_ms: t, owner_id: owner, note_mill: m, ..Default::default() });
                    out.push(FactorCommand {
                        time_ms: finish,
                        owner_id: owner,
                        note_mill: m.wrapping_neg(),
                        ..Default::default()
                    });
                }
                2004 => {
                    for &tid in &r.skill_target_ids {
                        let tg = master.skill_target(tid).filter(|tg| tg.skill_target_type == 4).ok_or_else(|| {
                            Error::Master(format!("live skill effect {}: target {tid} is not a judgement target", r.id))
                        })?;
                        let m = judgement_factor_mill(factor);
                        let j = tg.judgement as i32;
                        out.push(FactorCommand {
                            time_ms: t,
                            owner_id: owner,
                            judgement: j,
                            judge_mill: m,
                            ..Default::default()
                        });
                        out.push(FactorCommand {
                            time_ms: finish,
                            owner_id: owner,
                            judgement: j,
                            judge_mill: m.wrapping_neg(),
                            ..Default::default()
                        });
                    }
                }
                t => return Err(Error::Unsupported(format!("live skill effect type {t}"))),
            }
        }
    }
    Ok(out)
}
