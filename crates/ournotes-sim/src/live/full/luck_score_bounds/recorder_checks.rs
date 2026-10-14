//! Checks that a reduced recorder's schedule is deterministic, and its probe rows in native order.

use super::*;

pub(crate) fn deterministic(checker: &Checker) -> bool {
    match checker {
        Checker::Fixed(_)
        | Checker::LifeAtLeast(_)
        | Checker::LifeAtMost(_)
        | Checker::LifeGreater(_)
        | Checker::LifeLess(_)
        | Checker::LifeChanged { .. }
        | Checker::LifePercent { .. }
        | Checker::LifeDelta { .. }
        | Checker::LiveComboMultiple { .. }
        | Checker::LiveComboAtLeast(_)
        | Checker::ElapsedTime { .. }
        | Checker::SameMemberLiveSkill(_)
        | Checker::NoteJudgementMatch { .. }
        | Checker::NoteJudgementCount { .. }
        | Checker::ComboAtLeast { .. }
        | Checker::GkInterval { .. }
        | Checker::JustAtLeast { .. }
        | Checker::GkJustEdge { .. }
        | Checker::SnapGekisouStart { .. }
        | Checker::GkLiveStart(_)
        | Checker::GkReachRank { .. }
        | Checker::RangeStart { .. }
        | Checker::RangePlaying { .. }
        | Checker::RangeComplete => true,
        Checker::And { items, .. } | Checker::Or(items) => items.iter().all(deterministic),
        Checker::Not(inner) => deterministic(inner),
        _ => false,
    }
}

/// Every current cumulative primitive reads only the fixed formation, frame clock, deterministic life and
/// converted-judgement/controller counts. Keep this exhaustive over variants: a future RNG/score-dependent
/// cumulative needs its own proof instead of inheriting this certificate.
pub(crate) fn deterministic_cumulative(cumulative: &conditions::Cumulative) -> bool {
    match cumulative {
        conditions::Cumulative::Fixed(_)
        | conditions::Cumulative::ElapsedTime { .. }
        | conditions::Cumulative::ComboPerN { .. }
        | conditions::Cumulative::JudgementPerN { .. }
        | conditions::Cumulative::LifePerN { .. } => true,
    }
}

fn fixed_predicate(checker: &Checker) -> bool {
    match checker {
        Checker::Fixed(_) => true,
        Checker::And { items, .. } | Checker::Or(items) => items.iter().all(fixed_predicate),
        Checker::Not(inner) => fixed_predicate(inner),
        _ => false,
    }
}

/// Retained ordinary effects change only score or deterministic life under a constant maximum. With no stochastic conditions on
/// those rows, note damage, frozen life, judgements, combos, gates and command values are independent of
/// lottery outcomes. The reduced DP separately compiles the lottery rows against that same life timeline.
/// Induction over native frame/phase order closes life, conversion limits, live-skill duration, ordinary combo,
/// Gekisou combo/Just counts, cumulative counters and their predicates. Their writers below read only these same
/// deterministic domains; no predicate reads score or a lottery result. Rank predicates read either solo rank 1
/// or the fixed external arrival timeline. Range states use fever events and the original binary32 stopwatches.
/// The separately admitted 11000..11005 chain and direct-7021 score rows write none of those domains. Extension
/// 15000 changes the native live-skill lifecycle in this same deterministic closure; no updater is bypassed.
/// Therefore every lottery path has the recorder's identical converted judgements, life, combo inputs, ordinary
/// command filings and effect values. The DP life recorder retains these writers and consumes the same converted
/// results. A sampled agreement is not the authority for exact_final_life or the ordinary score command history.
pub(crate) fn check_recorder(model: &LiveModel, skills: &LuckSkills) -> Result<(), Error> {
    check_recorder_mode(model, skills, false)
}

pub(crate) fn check_conditioned_recorder(model: &LiveModel, skills: &LuckSkills) -> Result<(), Error> {
    check_recorder_mode(model, skills, true)
}

fn recorded_predicate(checker: &Checker, conditioned: bool) -> bool {
    match checker {
        Checker::ConditionedProbability(_) => conditioned,
        Checker::And { items, .. } | Checker::Or(items) => items.iter().all(|c| recorded_predicate(c, conditioned)),
        Checker::Not(inner) => recorded_predicate(inner, conditioned),
        _ => deterministic(checker),
    }
}

fn check_recorder_mode(model: &LiveModel, skills: &LuckSkills, conditioned: bool) -> Result<(), Error> {
    luck_dp::check_model_state_identities(model)?;
    // AddCommand invalidates all four native cache fields, so extra reads preserve the command-log
    // semantics at a constant cap. UpdateLifeMax deliberately does not invalidate: a read before a
    // max change can retain an old-cap recovery. A single reference path cannot certify that domain
    // until its reader/cache history is included in the stochastic state. Current JP has no 3000 rows.
    if model.rows.iter().any(|row| row.effect_type == 3000) {
        return Err(refuse("dynamic life maximum requires a complete reader/cache-history certificate"));
    }
    let related: FxHashSet<_> = model.luck_score_rows(skills).iter().map(|row| row.row).collect();
    let check = |row: &EffectRow, cumulative: Option<&conditions::Cumulative>, checkers: &[&Option<Checker>]| {
        if !matches!(row.effect_type, 2000..=2005 | 3000..=3004 | 4004 | 12000 | 12002..=12004 | 12006
            | 13000 | 13002..=13005 | 15000)
            || cumulative.is_some_and(|c| !deterministic_cumulative(c))
            || checkers.iter().any(|checker| checker.as_ref().is_some_and(|c| !recorded_predicate(c, conditioned)))
        {
            return Err(refuse(&format!("ordinary row {} has no proved deterministic score/life schedule", row.id)));
        }
        Ok(())
    };
    for skill in &model.live {
        for effect in &skill.effects {
            check(&model.rows[effect.row], effect.cumulative.as_ref(), &[&effect.condition, &effect.release])?;
        }
    }
    // The probe rows switch together: one trigger, one gate, fixed conditions (the replay's two classes).
    let mut probe_gate = None;
    for skill in &model.cond {
        for (effect_index, effect) in skill.updater.effects().iter().enumerate() {
            if related.contains(&effect.row) {
                if *probe_gate.get_or_insert(skill.updater.gate_mission()) != skill.updater.gate_mission() {
                    return Err(refuse("score probes need one common Gekisou gate"));
                }
                if !matches!(effect.trigger, Some(Checker::LuckRushPlaying(_)))
                    || effect.trigger_type != SUSTAINED
                    || effect.act != 0.0
                    || effect.cumulative.is_some()
                    || effect.execute_limit != 0
                    || effect.reset.is_some()
                    || effect.condition.as_ref().is_some_and(|checker| !fixed_predicate(checker))
                    || skill
                        .updater
                        .updaters
                        .iter()
                        .any(|updater| updater.effect == effect_index && updater.release.is_some())
                {
                    return Err(refuse(
                        "score probes need a common untimed direct 7021 predicate with fixed conditions",
                    ));
                }
                continue;
            }
            if is_luck_chain(model.rows[effect.row].effect_type) {
                continue;
            }
            check(
                &model.rows[effect.row],
                effect.cumulative.as_ref(),
                &[&effect.trigger, &effect.condition, &effect.reset],
            )?;
            for updater in skill.updater.updaters.iter().filter(|updater| updater.effect == effect_index) {
                if updater.release.as_ref().is_some_and(|checker| !recorded_predicate(checker, conditioned)) {
                    return Err(refuse("an ordinary release reads an unproved state"));
                }
            }
        }
    }
    Ok(())
}

/// Bind the shared direct-probe class to the native effect lifetime. Fixed-false rows have no holder;
/// every retained row needs the same legal phase and an extra condition that is proved true.
pub(crate) fn bind_probe_phase(model: &LiveModel, skills: &LuckSkills) -> bool {
    let related: FxHashSet<_> =
        model.luck_score_rows(skills).iter().filter(|row| row.may_hold).map(|row| row.row).collect();
    let mut phase = None;
    model.cond.iter().flat_map(|skill| skill.updater.effects()).filter(|effect| related.contains(&effect.row)).all(
        |effect| {
            matches!(effect.phase, 1 | 2)
                && *phase.get_or_insert(effect.phase) == effect.phase
                && effect.condition.as_ref().is_none_or(|condition| luck_dp::fixed_condition(condition) == Some(true))
        },
    )
}

/// A probe that is off and stays off at every skill boundary after music end files no clamped inverse.
/// Certify that tail against the complete native frame clock and its normal/clamped observer schedule.
pub(super) fn check_probe_music_boundary(
    frame_times: &[i32],
    curve: &LuckDpCertifiedResult,
    music_length_ms: i32,
    phase_bound: bool,
    trace: &BoundsTrace,
) -> Result<(), Error> {
    check_probe_filings(
        frame_times,
        curve,
        music_length_ms,
        phase_bound,
        !trace.probes.is_empty(),
        trace.frames,
        trace.events.iter(),
    )
}

/// [`check_probe_music_boundary`] of a schedule with `frames` score frames and these events.
pub(super) fn check_probe_filings<'a>(
    frame_times: &[i32],
    curve: &LuckDpCertifiedResult,
    music_length_ms: i32,
    phase_bound: bool,
    has_probes: bool,
    frames: usize,
    events: impl Iterator<Item = &'a BoundsEvent>,
) -> Result<(), Error> {
    if music_length_ms <= 0 || !has_probes || !frame_times.iter().any(|&time| time > music_length_ms) {
        return Ok(());
    }
    let unsupported = || refuse("score probe lifetimes beyond music end need a certified inactive tail");
    if !phase_bound
        || curve.probe_transitions.len() != frame_times.len()
        || curve.probe_transitions.iter().any(|&mask| !(1..=15).contains(&mask))
        || frame_times.windows(2).any(|times| times[0] >= times[1])
        || frame_times
            .iter()
            .zip(&curve.probe_transitions)
            .any(|(&time, &mask)| time > music_length_ms && mask != 0b0001)
    {
        return Err(unsupported());
    }
    let last = i32::try_from(frames).ok().filter(|&frames| frames > 0).ok_or_else(unsupported)? - 1;
    let mut events = events.filter_map(|event| match event {
        BoundsEvent::Probe { frame, time_ms } => Some((*frame, *time_ms)),
        _ => None,
    });
    for &time in frame_times {
        let score_frame = get_frame(time).min(last) as usize;
        if events.next() != Some((score_frame, time)) {
            return Err(unsupported());
        }
        if music_length_ms > 0 && time > music_length_ms {
            let score_frame = get_frame(music_length_ms).min(last) as usize;
            if events.next() != Some((score_frame, music_length_ms)) {
                return Err(unsupported());
            }
        }
    }
    if events.next().is_some() {
        return Err(unsupported());
    }
    Ok(())
}

/// Probe filing order follows native phases, condition-skill order and each updater's signed effect key. The
/// replay's stable owner sort retains this order for probes with equal owners.
pub(crate) fn probes_in_native_order(model: &LiveModel, skills: &LuckSkills) -> Result<Vec<ProbeRow>, Error> {
    let mut rows: FxHashMap<_, _> = model
        .luck_score_rows(skills)
        .into_iter()
        .filter(|row| row.may_hold)
        .map(|row| (row.row, ProbeRow { owner: row.owner, value: row.value }))
        .collect();
    let mut probes = Vec::with_capacity(rows.len());
    for phase in PHASES {
        for skill in &model.cond {
            let mut effects: Vec<_> = skill.updater.effects().iter().filter(|effect| effect.phase == phase).collect();
            effects.sort_by_key(|effect| effect.effect_id);
            for effect in effects {
                if let Some(row) = rows.remove(&effect.row) {
                    probes.push(row);
                }
            }
        }
    }
    if !rows.is_empty() {
        return Err(refuse("a score probe has no supported native phase order"));
    }
    Ok(probes)
}
