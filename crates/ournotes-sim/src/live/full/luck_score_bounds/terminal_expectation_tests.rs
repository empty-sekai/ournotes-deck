//! Complete native terminal expectations checked against independently enumerated nominal branches.
use super::*;

fn summary(
    input: &RushCase,
    terminal: bool,
    cancelled: impl FnMut() -> bool,
) -> Result<Option<LuckScoreSummary>, Error> {
    let skills = luck_skills(&input.master)?;
    let mut session = LuckScoreSession::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        input.ranking.as_deref(),
    );
    if terminal {
        session.summary_or_terminal(&input.deck, None, cancelled)
    } else {
        session.summary(&input.deck, None, cancelled)
    }
}

fn bits(value: &LuckScoreSummary) -> (u64, u64, i32, i32, Option<i32>, Option<i32>) {
    (
        value.final_mean.lower.to_bits(),
        value.final_mean.upper.to_bits(),
        value.final_support.lower,
        value.final_support.upper,
        value.exact_constant_score,
        value.exact_final_life,
    )
}

#[test]
fn terminal_summary_encloses_the_same_native_mean_with_and_without_curve_storage() {
    let mut input = four_bucket_case(2400, 2, false);
    input.params.total_power = 997;
    input.deck[1].live_skill = Some((903, 1));
    input.events = vec![(0, 110), (1, 110), (0, 2510)];
    let mut down = input.master.live_skill_effects[0].clone();
    down.id = 907;
    down.skill_effect_type = 2005;
    down.effect_value = 1234;
    input.master.live_skill_effects.push(down);
    input.master.reindex().unwrap();
    let branches = assert_native_total_mean(&input);
    assert!(branches.iter().any(|branch| branch.retained_notes_at_last_query));
    assert!(branches.iter().all(|branch| branch.same_owner_ordinary_ties));
    let capability = input.ready(None);
    let direct = capability.terminal_summary(i64::from(input.params.total_power)).unwrap();
    let selected = summary(&input, true, || false).unwrap().unwrap();
    assert_eq!(bits(&direct), bits(&selected));
    let replay = summary(&input, false, || false).unwrap().unwrap();
    assert!(direct.final_mean.lower <= replay.final_mean.upper && replay.final_mean.lower <= direct.final_mean.upper);
    assert!(direct.exact_constant_score.is_none(), "a completed interval is not an exact rational expectation");
}

#[test]
fn terminal_probe_lower_requires_complete_true_fixed_predicates() {
    use super::super::super::prepass::{fixed_truth, required_probe_lower};
    let input = four_bucket_case(2400, 2, false);
    let mut model = input.native();
    let skills = luck_skills(&input.master).unwrap();
    let rows = model.luck_score_rows(&skills);
    assert_eq!(rows.iter().map(|row| row.row).collect::<FxHashSet<_>>().len(), rows.len());
    assert!(rows.iter().map(|row| row.owner).collect::<FxHashSet<_>>().len() < rows.len());
    let full = required_probe_lower(&model, &rows, &mut || false).unwrap();
    assert!(full > 0.0);
    let false_tree = Checker::Not(Box::new(Checker::And {
        items: vec![Checker::Fixed(true), Checker::Fixed(true)],
        resettable: vec![false, false],
    }));
    assert!(false_tree.may_hold());
    assert_eq!(fixed_truth(&false_tree), Some(false));
    for skill in &mut model.cond {
        let mut effects = skill.updater.effects().to_vec();
        for effect in &mut effects {
            if rows.iter().any(|row| row.row == effect.row) {
                effect.condition = Some(false_tree.clone());
            }
        }
        skill.updater = ConditionSkillUpdater::new(effects, |_| Ok(None), skill.updater.gate_mission()).unwrap();
    }
    assert_eq!(required_probe_lower(&model, &rows, &mut || false), Ok(0.0));
    assert_eq!(fixed_truth(&Checker::LifeAtLeast(Some(0))), None);
    assert_eq!(
        required_probe_lower(&model, &rows, &mut || true),
        Err(super::super::super::trace_drift::Decline::Cancelled)
    );
    let mut duplicate = rows.clone();
    duplicate.push(rows[0]);
    assert_eq!(
        required_probe_lower(&model, &duplicate, &mut || false),
        Err(super::super::super::trace_drift::Decline::Incomplete)
    );
}

#[test]
fn terminal_summary_falls_back_to_the_complete_factor_evaluator_on_unproved_combo_history() {
    let mut input = four_bucket_case(2400, 2, false);
    input.master.live_skill_effects[0].skill_effect_type = 2002;
    input.master.reindex().unwrap();
    let capability = input.ready(None);
    assert!(capability.terminal_summary(i64::from(input.params.total_power)).is_none());
    let replay = summary(&input, false, || false).unwrap().unwrap();
    let selected = summary(&input, true, || false).unwrap().unwrap();
    assert_eq!(bits(&selected), bits(&replay));
}

#[test]
fn terminal_summary_cancellation_never_returns_or_caches_a_partial_expectation() {
    let input = four_bucket_case(2400, 2, false);
    let mut checks = 0;
    let reference = summary(&input, true, || {
        checks += 1;
        false
    })
    .unwrap()
    .unwrap();
    assert!(checks > 4);
    for stop in [1, checks / 2, checks] {
        let mut seen = 0;
        assert!(
            summary(&input, true, || {
                seen += 1;
                seen >= stop
            })
            .unwrap()
            .is_none()
        );
        assert_eq!(bits(&summary(&input, true, || false).unwrap().unwrap()), bits(&reference));
    }
}

#[test]
fn terminal_constant_support_is_required_for_an_exact_score() {
    let mut input = four_bucket_case(2400, 2, false);
    input.params.total_power = 0;
    let result = summary(&input, true, || false).unwrap().unwrap();
    assert_eq!((result.final_support.lower, result.final_support.upper), (0, 0));
    assert_eq!((result.final_mean.lower, result.final_mean.upper), (0.0, 0.0));
    assert_eq!(result.exact_constant_score, Some(0));
    assert!(result.exact_final_life.is_some());
}

#[test]
fn terminal_mean_keeps_same_owner_rows_across_mixed_mission_finishes() {
    let mut input = four_bucket_case(2400, 1, false);
    input.setup.missions = vec![1, 2, 3];
    for row in &mut input.master.gekisou_ranking_score_bonuses {
        row.mission_pattern = gekisou::mission_pattern(1, 2, 3);
    }
    input.master.reindex().unwrap();
    let branches = assert_native_total_mean(&input);
    assert!(branches.iter().any(|branch| branch.probe_commands > 0));
    let value = summary(&input, true, || false).unwrap().unwrap();
    assert!(value.final_mean.lower > 0.0);
}

#[test]
fn terminal_mean_keeps_signed_probe_end_commands_at_the_music_boundary() {
    let mut input = four_bucket_case(2400, 1, false);
    input.params.music_length_ms = input.notes.iter().map(|note| note.time_ms).max().unwrap() + 1;
    // Multiple distinct rows still share each Snap owner. Their small amplitudes keep every all-path
    // support nonnegative while a backdated finish adds a second possible probe-time run.
    for row in &mut input.master.gekisou_support_skill_effects {
        row.effect_value = 250;
    }
    input.master.reindex().unwrap();
    let branches = assert_native_total_mean(&input);
    assert!(branches.iter().any(|branch| branch.clamped_probe_end));
}

#[test]
fn terminal_mean_preserves_refusal_when_a_foreign_finish_interrupts_an_overlapping_luck_range() {
    let mut input = four_bucket_case(2400, 1, false);
    input.setup.fevers = vec![(100, 160), (200, 6000), (2500, 2560)];
    input.setup.missions = vec![1, 2, 3];
    input.notes = [210, 220, 3000, 3100]
        .into_iter()
        .enumerate()
        .map(|(id, time_ms)| LiveNote { note_id: id as i32, note_operate_type: 1, judgement_type: 1, time_ms })
        .collect();
    input.params.music_length_ms = 8200;
    input.play.frames = (0..=83).map(|frame| PlayFrame { time_ms: frame * 100, judged: Vec::new() }).collect();
    input.delta = vec![0.1; input.play.frames.len()];
    rebuild_rush_frames(&mut input, |_| 5);
    for row in &mut input.master.gekisou_ranking_score_bonuses {
        row.mission_pattern = gekisou::mission_pattern(1, 2, 3);
    }
    input.master.reindex().unwrap();
    let branches = native_branches(&input);
    assert!(branches.len() > 1 && branches.iter().any(|branch| branch.probe_commands > 0));
    let mean = branches
        .iter()
        .fold(Fraction::ZERO, |mean, branch| mean.plus(branch.mass.times(branch.total_score as u128, 1)));
    let terminal = match input.prepare(None, || false) {
        LuckRushPreparation::Ready(capability) => capability.terminal_summary(i64::from(input.params.total_power)),
        LuckRushPreparation::Unavailable { .. } => None,
        LuckRushPreparation::Stopped => panic!("uncancelled native preparation"),
    };
    let selected = summary(&input, true, || false);
    match selected {
        Ok(Some(value)) => {
            assert!(mean.at_least(value.final_mean.lower) && mean.at_most(value.final_mean.upper));
            for branch in &branches {
                assert!(
                    value.final_support.lower <= branch.total_score && branch.total_score <= value.final_support.upper
                );
            }
        }
        Err(Error::Unsupported(_)) => assert!(terminal.is_none()),
        outcome => panic!("a complete native domain retains a summary or an explicit admission refusal: {outcome:?}"),
    }
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn terminal_summary_diagnostics_distinguish_acceptance_fallback_and_cancellation() {
    let mut input = four_bucket_case(2400, 2, false);
    take_luck_score_profile();
    assert!(summary(&input, true, || false).unwrap().is_some());
    let complete = take_luck_score_profile();
    assert_eq!(complete.terminal_evaluations, 1);
    assert_eq!(complete.terminal_replay_fallbacks, 0);
    assert_eq!(complete.factor_queries, 0);
    assert!(complete.recorder_trace_only_queries > 0);

    input.master.live_skill_effects[0].skill_effect_type = 2002;
    input.master.reindex().unwrap();
    assert!(summary(&input, true, || false).unwrap().is_some());
    let fallback = take_luck_score_profile();
    assert_eq!(fallback.terminal_evaluations, 0);
    assert_eq!(fallback.terminal_replay_fallbacks, 1);
    assert!(fallback.factor_queries > 0);

    assert!(summary(&input, true, || true).unwrap().is_none());
    let cancelled = take_luck_score_profile();
    assert_eq!(cancelled.terminal_cancellations, 1);
    assert_eq!(cancelled.terminal_evaluations, 0);
    assert_eq!(cancelled.terminal_replay_fallbacks, 0);
}
