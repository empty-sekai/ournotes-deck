use super::*;

#[test]
fn terminal_cylinder_choices_match_independent_weighted_native_paths_and_do_not_cache_a_law() {
    let (mut master, mut notes, mut params, setup, mut play, delta) = fixture();
    for row in &mut master.gekisou_luck_bonus_lots {
        row.weight = if row.lot_result == 3 { 3 } else { 1 };
    }
    notes.push(LiveNote { note_id: 2, ..notes[0] });
    play.frames[1].judged.push(JudgedNote { note_id: 2, judgement: 5, judgement_time_ms: 100 });
    params.converted_note_count = 2;
    let mut session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 64).unwrap();
    let mut work = LuckExactBudget::default();
    for (choice, index, numerator) in [(LuckCylinderChoice::First, 0, 27), (LuckCylinderChoice::Last, 1, 1)] {
        let before = work.clone();
        let result = session.terminal_cylinder(&[], choice, &mut work, || false).unwrap();
        assert_eq!(result.decline, None);
        assert_eq!(result.stats.terminal_paths, 1);
        assert_eq!(result.stats.replay_runs, before.remaining_runs - work.remaining_runs);
        assert_eq!(result.stats.frames, before.remaining_frames - work.remaining_frames);
        let atom = result.cylinder.unwrap().atom();
        assert_eq!(atom.mass, LuckExactMass::reduced(numerator, 64).unwrap());
        let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
        native.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(vec![index; 3])).unwrap();
        assert!(native.random.nominal_prefix_consumed() && native.random.nominal_covers_draws());
        assert_eq!((atom.score, atom.final_life), (native.score(), native.current_life()));
        assert!(session.laws.is_empty());
        assert_eq!(session.cached_bytes, 0);
    }
    let result = session.law(&[], &mut work, || false).unwrap();
    assert!(result.stats.replay_runs > 0 && result.stats.terminal_paths > 1);
    assert!(result.law.is_some());
    let retained = (session.laws.len(), session.cached_bytes);
    let mut empty = LuckExactBudget { remaining_runs: 0, remaining_frames: 0 };
    assert!(session.law(&[], &mut empty, || false).unwrap().law.is_some());
    let refused = session.first_terminal(&[], &mut empty, || false).unwrap();
    assert_eq!(refused.decline, Some(LuckExactDecline::WorkBudget));
    assert!(refused.cylinder.is_none());
    assert_eq!((session.laws.len(), session.cached_bytes), retained);
}

#[test]
fn terminal_cylinder_preserves_singleton_draws_and_unit_mass_without_inventing_a_full_law() {
    let (mut master, notes, params, setup, play, delta) = fixture();
    master.gekisou_luck_bonus_lots.retain(|row| row.lot_result == 0);
    let session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
    for choice in [LuckCylinderChoice::First, LuckCylinderChoice::Last] {
        let result = session.terminal_cylinder(&[], choice, &mut LuckExactBudget::default(), || false).unwrap();
        let atom = result.cylinder.unwrap().atom();
        assert_eq!(atom.mass, LuckExactMass::ONE);
        assert_eq!(result.stats.replay_runs, 1);
        assert_eq!(result.stats.frames, play.frames.len() as u64);
        let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
        native.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(Vec::new())).unwrap();
        assert!(native.draws() > 0, "deterministic semantic outcomes still consume the original draws");
        assert!(native.random.nominal_prefix_consumed() && native.random.nominal_covers_draws());
        assert_eq!((atom.score, atom.final_life), (native.score(), native.current_life()));
        assert!(session.laws.is_empty());
    }
}

#[test]
fn terminal_cylinder_keeps_complete_checkpoints_rank_arrivals_and_final_life() {
    let (master, mut notes, mut params, mut setup, mut play, _) = fixture();
    notes.push(LiveNote { note_id: 2, time_ms: 6100, ..notes[0] });
    params.converted_note_count = 2;
    params.music_length_ms = 9200;
    setup.fevers = vec![(0, 150), (6000, 6150)];
    play.frames = (0..=900)
        .map(|index| PlayFrame {
            time_ms: index * 10,
            judged: notes
                .iter()
                .filter(|note| note.time_ms == index * 10)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let delta = vec![0.01; play.frames.len()];
    let ranking = [
        RankConfirmation { frame: 0, range: 0, rank: 1, percent: 23 },
        RankConfirmation { frame: 600, range: 1, rank: 1, percent: 37 },
    ];
    let session =
        LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, Some(&ranking), 0).unwrap();
    for (choice, index) in [(LuckCylinderChoice::First, 0), (LuckCylinderChoice::Last, 1)] {
        let mut budget = LuckExactBudget::default();
        let before = budget.clone();
        let result = session.terminal_cylinder(&[], choice, &mut budget, || false).unwrap();
        let atom = result.cylinder.unwrap().atom();
        let mut native = LiveModel::new_gekisou_external(&master, &[], &notes, &[], params, &setup).unwrap();
        native.set_rank_confirmation_timeline(&ranking).unwrap();
        native.run_with_random(&play, &delta, LiveRandom::with_nominal_prefix(vec![index; 4])).unwrap();
        assert!(native.random.nominal_prefix_consumed() && native.random.nominal_covers_draws());
        assert_eq!((atom.score, atom.final_life), (native.score(), native.current_life()));
        assert_eq!(atom.mass, LuckExactMass::reduced(1, 16).unwrap());
        assert_eq!(native.rank_confirmation_applications().len(), 2);
        assert_eq!(result.stats.frames, before.remaining_frames - budget.remaining_frames);
        assert_eq!(result.stats.replay_runs, before.remaining_runs - budget.remaining_runs);
        assert!(result.stats.frames < result.stats.replay_runs * play.frames.len() as u64);
    }
}

#[test]
fn terminal_cylinder_budget_and_every_cancellation_boundary_publish_no_partial_atom() {
    let (master, notes, params, setup, play, delta) = fixture();
    let session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
    let mut polls = 0;
    let reference = session
        .terminal_cylinder(&[], LuckCylinderChoice::Last, &mut LuckExactBudget::default(), || {
            polls += 1;
            false
        })
        .unwrap();
    assert!(reference.cylinder.is_some());
    for stop in [1, polls / 2, polls - 1, polls] {
        let mut seen = 0;
        let mut budget = LuckExactBudget::default();
        let before = budget.clone();
        let result = session
            .terminal_cylinder(&[], LuckCylinderChoice::Last, &mut budget, || {
                seen += 1;
                seen == stop
            })
            .unwrap();
        assert_eq!(result.decline, Some(LuckExactDecline::Cancelled));
        assert!(result.cylinder.is_none());
        assert_eq!(result.stats.terminal_paths, 0);
        assert_eq!(result.stats.frames, before.remaining_frames - budget.remaining_frames);
        assert_eq!(result.stats.replay_runs, before.remaining_runs - budget.remaining_runs);
    }
    for mut budget in [
        LuckExactBudget { remaining_runs: 1, remaining_frames: 10_000 },
        LuckExactBudget { remaining_runs: 100, remaining_frames: reference.stats.frames - 1 },
    ] {
        let before = budget.clone();
        let result = session.terminal_cylinder(&[], LuckCylinderChoice::Last, &mut budget, || false).unwrap();
        assert_eq!(result.decline, Some(LuckExactDecline::WorkBudget));
        assert!(result.cylinder.is_none());
        assert_eq!(result.stats.terminal_paths, 0);
        assert_eq!(result.stats.frames, before.remaining_frames - budget.remaining_frames);
        assert_eq!(result.stats.replay_runs, before.remaining_runs - budget.remaining_runs);
    }
    let mut exact_budget =
        LuckExactBudget { remaining_runs: reference.stats.replay_runs, remaining_frames: reference.stats.frames };
    let complete = session.terminal_cylinder(&[], LuckCylinderChoice::Last, &mut exact_budget, || false).unwrap();
    assert_eq!(complete.cylinder.unwrap().atom(), reference.cylinder.unwrap().atom());
    assert!(exact_budget.exhausted());
    assert!(session.laws.is_empty());
}

#[test]
fn terminal_cylinder_keeps_depth_limits_and_rejects_unhandled_initial_randomness() {
    let (master, _, mut params, mut setup, mut play, _) = fixture();
    let notes: Vec<_> = (1..=40)
        .map(|index| LiveNote { note_id: index, note_operate_type: 1, judgement_type: 1, time_ms: index * 100 })
        .collect();
    params.converted_note_count = notes.len() as i32;
    params.music_length_ms = 7000;
    setup.fevers = vec![(0, 4500)];
    play.frames = (0..=650)
        .map(|index| PlayFrame {
            time_ms: index * 10,
            judged: notes
                .iter()
                .filter(|note| note.time_ms == index * 10)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let delta = vec![0.01; play.frames.len()];
    let session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
    for choice in [LuckCylinderChoice::First, LuckCylinderChoice::Last] {
        let result = session.terminal_cylinder(&[], choice, &mut LuckExactBudget::default(), || false).unwrap();
        assert_eq!(result.decline, Some(LuckExactDecline::BranchDepth));
        assert!(result.cylinder.is_none());
        assert_eq!(result.stats.terminal_paths, 0);
        assert!(result.stats.replay_runs <= MAX_BRANCH_DEPTH as u64 + 1);
    }
    let mut native = LiveModel::new_gekisou(&master, &[], &notes, &[], params, &setup).unwrap();
    native.random.next_int(crate::live::random::PRESENTATION);
    let result = terminal_cylinder_path(
        native,
        &play,
        &delta,
        LuckCylinderChoice::First,
        &mut LuckExactBudget::default(),
        || false,
    )
    .unwrap();
    assert_eq!(result.decline, Some(LuckExactDecline::UnhandledRandom));
    assert_eq!(result.stats, LuckExactStats::default());
    assert!(result.cylinder.is_none());
}

#[test]
fn terminal_cylinder_rational_overflow_never_rounds_a_positive_mass_into_a_certificate() {
    let (mut master, _, mut params, mut setup, mut play, delta) = fixture();
    for row in &mut master.gekisou_luck_bonus_lots {
        // Both native f32 weights are exact, while the odd denominator's sixth power exceeds u128.
        row.weight = if row.lot_result == 3 { 1 << 24 } else { 1 };
    }
    let notes: Vec<_> = (1..=6)
        .map(|index| LiveNote { note_id: index, note_operate_type: 1, judgement_type: 1, time_ms: index * 100 })
        .collect();
    params.converted_note_count = notes.len() as i32;
    setup.fevers = vec![(0, 800)];
    for frame in &mut play.frames {
        frame.judged = notes
            .iter()
            .filter(|note| note.time_ms == frame.time_ms)
            .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
            .collect();
    }
    let session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
    for choice in [LuckCylinderChoice::First, LuckCylinderChoice::Last] {
        let result = session.terminal_cylinder(&[], choice, &mut LuckExactBudget::default(), || false).unwrap();
        assert_eq!(result.decline, Some(LuckExactDecline::Arithmetic));
        assert!(result.cylinder.is_none());
        assert_eq!(result.stats.terminal_paths, 0);
    }
}

#[test]
fn terminal_cylinder_does_not_admit_unsupported_unvisited_siblings_as_a_complete_domain() {
    use crate::live::full::{conditions::Checker, engine::ConditionSkillUpdater};
    let (master, notes, params, setup, play, delta) = fixture();
    let deck = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
    let mut model = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
    fn replace_probability(checker: &mut Checker) {
        match checker {
            Checker::Probability(rate) => *rate = f32::from_bits(1),
            Checker::And { items, .. } | Checker::Or(items) => items.iter_mut().for_each(replace_probability),
            Checker::Not(inner) => replace_probability(inner),
            _ => {}
        }
    }
    for skill in &mut model.cond {
        let mut effects = skill.updater.effects().to_vec();
        for effect in &mut effects {
            if let Some(trigger) = &mut effect.trigger {
                replace_probability(trigger);
            }
        }
        skill.updater = ConditionSkillUpdater::new(effects, |_| Ok(None), skill.updater.gate_mission()).unwrap();
    }
    let first = terminal_cylinder_path(
        model.clone(),
        &play,
        &delta,
        LuckCylinderChoice::First,
        &mut LuckExactBudget::default(),
        || false,
    )
    .unwrap();
    assert!(first.cylinder.is_some(), "the Critical cylinder never evaluates the Miss-only skill rate");
    let last = terminal_cylinder_path(
        model.clone(),
        &play,
        &delta,
        LuckCylinderChoice::Last,
        &mut LuckExactBudget::default(),
        || false,
    )
    .unwrap();
    assert_eq!(last.decline, Some(LuckExactDecline::Unsupported));
    assert!(last.cylinder.is_none());
    let complete = enumerate_law(model, &play, &delta, &mut LuckExactBudget::default(), || false).unwrap();
    assert_eq!(complete.decline, Some(LuckExactDecline::Unsupported));
    assert!(complete.law.is_none(), "one successful cylinder must never become a whole-domain certificate");
}
