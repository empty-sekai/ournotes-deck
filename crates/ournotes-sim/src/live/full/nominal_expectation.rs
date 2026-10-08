//! Complete semantic SKILL event trees for score-only chart measurements.
//!
//! Score predicates independent of the lottery are evaluated at their original native check sites.
//! Each completed conditional recording uses the same certified LUCK law, then contributes its
//! entire score and rank snapshots with the event path's outward probability mass.

use std::{rc::Rc, sync::Arc};

use super::{
    GekisouSetup, LiveModel, LiveNote, LiveParams, LivePlay, LuckDpCertifiedResult, LuckRangeScoreBounds,
    LuckScoreExpectation, LuckSkills, Performer, RealBounds, conditions::Checker, gekisou, luck_score_bounds, setting,
};
use crate::{
    Error,
    live::{
        certified::{F64Interval, ProbabilityMass},
        random::LiveRandom,
        score::LiveScoreCalculator,
    },
    master::Master,
    replay::RankConfirmation,
};

const CHECKPOINT_FRAMES: usize = 256;
const MAX_BRANCH_DEPTH: usize = 24;
const MAX_REPLAY_RUNS: u64 = 32_768;
const MAX_REPLAY_FRAMES: u64 = 8_000_000;

fn unsupported(message: &str) -> Error {
    Error::Unsupported(format!("nominal score measurement: {message}"))
}

fn capacity() -> Error {
    Error::Capacity("nominal score measurement requires a larger complete event tree".into())
}

/// A comparison is conditioned only after its other leaves have the existing deterministic-domain
/// proof. The checker topology, count-reset rules, cached answers and short-circuit order are retained.
pub(super) fn condition_checker(checker: &mut Option<Checker>) -> Result<bool, Error> {
    let Some(checker) = checker else { return Ok(false) };
    if !checker.any(&|c| matches!(c, Checker::Probability(_))) {
        return Ok(false);
    }
    fn convert(checker: &mut Checker) -> Result<(), Error> {
        match checker {
            Checker::Probability(rate) => *checker = Checker::ConditionedProbability(*rate),
            Checker::And { items, .. } | Checker::Or(items) => {
                for item in items {
                    convert(item)?;
                }
            }
            Checker::Not(inner) => convert(inner)?,
            checker if luck_score_bounds::deterministic(checker) => {}
            _ => return Err(unsupported("a score probability predicate reads a lottery-dependent state")),
        }
        Ok(())
    }
    convert(checker)?;
    Ok(true)
}

pub(crate) fn has_nominal_score_probabilities(master: &Master, deck: &[Performer], skills: &LuckSkills) -> bool {
    let probability = |group| {
        group != 0
            && master.skill_condition_sets.iter().filter(|s| s.group == group).any(|s| {
                s.condition_ids.iter().any(|&id| master.skill_condition(id).is_some_and(|c| c.condition_type == 4011))
            })
    };
    let groups = |groups: [i64; 4]| groups.into_iter().any(probability);
    let ordinary_score = |effect| matches!(effect, 2000..=2005);
    deck.iter().any(|p| {
        p.live_skill.is_some_and(|(id, level)| {
            master.live_skill_effects.iter().any(|r| {
                r.live_skill_id == id
                    && r.level == level
                    && ordinary_score(r.skill_effect_type)
                    && groups([0, r.skill_condition_group, r.skill_release_condition_group, 0])
            })
        }) || p.support_skills.iter().any(|&(id, level)| {
            master.support_skill_effects.iter().any(|r| {
                r.support_skill_id == id
                    && r.level == level
                    && ordinary_score(r.skill_effect_type)
                    && groups([
                        r.skill_trigger_condition_group,
                        r.skill_condition_group,
                        r.skill_release_condition_group,
                        if r.effect_execute_limit_count > 0 { r.effect_execute_limit_reset_condition_group } else { 0 },
                    ])
            })
        }) || p.gekisou_skill.is_some_and(|(id, level)| {
            master.gekisou_skill_effects.iter().any(|r| {
                r.skill_id == id
                    && r.level == level
                    && ordinary_score(r.skill_effect_type)
                    && !skills.rows.contains_key(&(super::LuckSource::Gekisou, r.id))
                    && groups([
                        r.skill_trigger_condition_group,
                        r.skill_condition_group,
                        r.skill_release_condition_group,
                        if r.effect_execute_limit_count > 0 { r.effect_execute_limit_reset_condition_group } else { 0 },
                    ])
            })
        }) || (p.gekisou_skill.is_some()
            && p.gekisou_support_skills.iter().any(|&(id, level)| {
                master.gekisou_support_skill_effects.iter().any(|r| {
                    r.skill_id == id
                        && r.level == level
                        && ordinary_score(r.skill_effect_type)
                        && !skills.rows.contains_key(&(super::LuckSource::GekisouSupport, r.id))
                        && groups([
                            r.skill_trigger_condition_group,
                            r.skill_condition_group,
                            r.skill_release_condition_group,
                            if r.effect_execute_limit_count > 0 {
                                r.effect_execute_limit_reset_condition_group
                            } else {
                                0
                            },
                        ])
                })
            }))
    })
}

fn condition_score_model(model: &mut LiveModel, skills: &LuckSkills) -> Result<(), Error> {
    let related: Vec<_> = model.luck_score_rows(skills).iter().map(|r| r.row).collect();
    let score_rows: Vec<_> = model
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| matches!(row.effect_type, 2000..=2005) && !related.contains(&index))
        .collect();
    let mut found = false;
    for skill in &mut model.live {
        let mut conditional = false;
        for effect in &mut skill.effects {
            if score_rows[effect.row] {
                conditional |= condition_checker(&mut effect.condition)?;
                conditional |= condition_checker(&mut effect.release)?;
            }
        }
        if conditional && skill.effects.iter().any(|effect| !score_rows[effect.row]) {
            return Err(unsupported("a probabilistic live-skill pool contains a non-score effect"));
        }
        found |= conditional;
    }
    for skill in &mut model.cond {
        found |= skill.updater.condition_score_probabilities(&score_rows)?;
    }
    if !found {
        return Err(Error::Domain("a nominal score recording has no probability predicate".into()));
    }
    luck_score_bounds::check_conditioned_recorder(model, skills)
}

fn real(value: RealBounds) -> F64Interval {
    F64Interval::new(value.lower, value.upper).expect("ordered score enclosure")
}

fn add_real(sum: &mut RealBounds, value: RealBounds, mass: ProbabilityMass) {
    *sum = real(*sum).add(real(value).multiply(mass.interval())).into();
}

fn weighted_range(mut range: LuckRangeScoreBounds, mass: ProbabilityMass) -> LuckRangeScoreBounds {
    range.mean = real(range.mean).multiply(mass.interval()).into();
    range.bonus_mean = real(range.bonus_mean).multiply(mass.interval()).into();
    range.luck_points_mean = range.luck_points_mean.map(|v| real(v).multiply(mass.interval()).into());
    range.lot_results_mean = range.lot_results_mean.map(|v| v.map(|v| real(v).multiply(mass.interval()).into()));
    range
}

fn accumulate(
    sum: &mut Option<LuckScoreExpectation>,
    value: LuckScoreExpectation,
    mass: ProbabilityMass,
) -> Result<(), Error> {
    let Some(sum) = sum else {
        *sum = Some(LuckScoreExpectation {
            final_mean: real(value.final_mean).multiply(mass.interval()).into(),
            final_support: value.final_support,
            ranges: value.ranges.into_iter().map(|r| weighted_range(r, mass)).collect(),
        });
        return Ok(());
    };
    add_real(&mut sum.final_mean, value.final_mean, mass);
    sum.final_support.lower = sum.final_support.lower.min(value.final_support.lower);
    sum.final_support.upper = sum.final_support.upper.max(value.final_support.upper);
    if sum.ranges.len() != value.ranges.len() {
        return Err(Error::Domain("nominal score paths have different range counts".into()));
    }
    for (sum, value) in sum.ranges.iter_mut().zip(value.ranges) {
        if sum.range != value.range || sum.percent != value.percent {
            return Err(Error::Domain("nominal score paths have different rank scenarios".into()));
        }
        add_real(&mut sum.mean, value.mean, mass);
        add_real(&mut sum.bonus_mean, value.bonus_mean, mass);
        sum.support.lower = sum.support.lower.min(value.support.lower);
        sum.support.upper = sum.support.upper.max(value.support.upper);
        sum.bonus_support.lower = sum.bonus_support.lower.min(value.bonus_support.lower);
        sum.bonus_support.upper = sum.bonus_support.upper.max(value.bonus_support.upper);
        match (&mut sum.luck_points_mean, value.luck_points_mean) {
            (Some(sum), Some(value)) => add_real(sum, value, mass),
            (None, None) => {}
            _ => return Err(Error::Domain("nominal score paths have different range indicators".into())),
        }
        match (&mut sum.lot_results_mean, value.lot_results_mean) {
            (Some(sum), Some(value)) => {
                for (sum, value) in sum.iter_mut().zip(value) {
                    add_real(sum, value, mass);
                }
            }
            (None, None) => {}
            _ => return Err(Error::Domain("nominal score paths have different lottery indicators".into())),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn nominal_score_expectation_for_chart(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[RankConfirmation]>,
    probability: Arc<LuckDpCertifiedResult>,
) -> Result<LuckScoreExpectation, Error> {
    if delta_times.len() != play.frames.len() {
        return Err(Error::Input("one delta time per frame".into()));
    }
    let mut fresh = if let Some(ranking) = ranking {
        let mut model = LiveModel::new_gekisou_ranked(master, deck, notes, events, params, setup)?;
        model.set_rank_confirmation_timeline(ranking)?;
        model
    } else {
        LiveModel::new_gekisou(master, deck, notes, events, params, setup)?
    };
    if fresh.draws() != 0 {
        return Err(unsupported("model initialization consumed an unhandled draw"));
    }
    condition_score_model(&mut fresh, skills)?;
    let probes = luck_score_bounds::probes_in_native_order(&fresh, skills)?;
    if probes.iter().any(|row| !row.value.is_finite() || row.value <= i32::MIN as f32 / 100000f32) {
        return Err(unsupported("a direct score command cannot be safely paired with its signed inverse"));
    }
    let calc: LiveScoreCalculator = fresh.score.calc.clone();
    if calc.converted_note_count <= 0
        || ![
            calc.score_adjustment_factor,
            calc.music_difficulty_factor,
            calc.life_onus_factor,
            calc.event_bonus_factor,
            calc.assist_factor,
        ]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(unsupported("nonfinite score constants or nonpositive note count"));
    }
    let rush_percent = i32::try_from(setting(master, "gekisou_luck_rush_score_bonus_percent")?)
        .map_err(|_| unsupported("Rush percent exceeds i32"))?;
    if 100i32.checked_add(rush_percent).is_none() {
        return Err(unsupported("Rush factor may wrap"));
    }
    let has_luck = setup.missions.iter().take(setup.fevers.len()).any(|&m| m == gekisou::M_LUCK);
    fresh.set_luck_weights(skills, Vec::new())?;
    let probe_phase_bound = luck_score_bounds::bind_probe_phase(&fresh, skills);
    fresh.score.begin_bounds(probes, has_luck);
    fresh.set_random(LiveRandom::with_nominal_skill_prefix());
    let mut pending = vec![(Vec::<bool>::new(), ProbabilityMass::ONE, 0usize, Rc::new(fresh))];
    let mut runs = 0u64;
    let mut frames = 0u64;
    let mut total_mass = ProbabilityMass::ZERO;
    let mut result = None;
    while let Some((prefix, mass, mut checkpoint_frame, mut checkpoint)) = pending.pop() {
        if runs >= MAX_REPLAY_RUNS {
            return Err(capacity());
        }
        runs += 1;
        let mut model = (*checkpoint).clone();
        model.random.extend_nominal_skill_prefix(prefix.clone())?;
        let mut error = None;
        let start_frame = checkpoint_frame;
        for (index, (frame, &delta)) in play.frames.iter().zip(delta_times).enumerate().skip(start_frame) {
            if frames >= MAX_REPLAY_FRAMES {
                return Err(capacity());
            }
            if index - checkpoint_frame >= CHECKPOINT_FRAMES {
                checkpoint_frame = index;
                checkpoint = Rc::new(model.clone());
            }
            frames += 1;
            if let Err(e) = model.frame_timed(frame.time_ms, &frame.judged, delta) {
                error = Some(e);
                break;
            }
        }
        if !model.random.nominal_skill_covers_draws() {
            return Err(unsupported("a conditional score path consumed an unhandled draw"));
        }
        if let Some(hit_mass) = model.random.nominal_skill_branch() {
            if error.is_none() {
                return Err(Error::Domain("a nominal skill branch did not stop recording".into()));
            }
            if prefix.len() >= MAX_BRANCH_DEPTH || pending.len() + 2 > MAX_REPLAY_RUNS as usize {
                return Err(capacity());
            }
            for (hit, event_mass) in [(true, hit_mass), (false, hit_mass.complement())] {
                let mut next = prefix.clone();
                next.push(hit);
                pending.push((next, mass.multiply(event_mass), checkpoint_frame, Rc::clone(&checkpoint)));
            }
            continue;
        }
        if let Some(error) = error {
            return Err(error);
        }
        if !model.random.nominal_skill_prefix_consumed() {
            return Err(Error::Domain("a terminal nominal score path did not consume its outcome prefix".into()));
        }
        let mut bounds = luck_score_bounds::complete_bounds_recording(
            model,
            calc.clone(),
            rush_percent,
            probability.clone(),
            play,
            probe_phase_bound,
            setup.fevers.len(),
            true,
            false,
            has_luck,
            &mut || false,
        )?
        .expect("complete nominal score recording");
        bounds.ranges.sort_by_key(|r| r.range);
        if bounds.ranges.len() != setup.fevers.len() || bounds.ranges.iter().enumerate().any(|(i, r)| r.range != i) {
            return Err(unsupported("a Gekisou range has no rank enclosure"));
        }
        accumulate(
            &mut result,
            LuckScoreExpectation {
                final_mean: bounds.final_mean,
                final_support: bounds.final_support,
                ranges: bounds.ranges,
            },
            mass,
        )?;
        total_mass = total_mass.merge_disjoint(mass);
    }
    if !total_mass.interval().contains(1.0) {
        return Err(Error::Domain("the completed nominal score tree does not enclose probability one".into()));
    }
    result.ok_or_else(|| Error::Domain("the completed nominal score tree has no terminal path".into()))
}
