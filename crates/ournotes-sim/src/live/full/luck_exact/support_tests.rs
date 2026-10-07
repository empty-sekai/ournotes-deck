use super::{
    super::{JudgedNote, PlayFrame},
    tests::fixture,
    *,
};
use std::collections::BTreeSet;

/// Every discovered prefix starts from a fresh native model. This deliberately shares no checkpoints,
/// caches, witness strategy, score bounds or terminal accumulation with the support evaluator.
fn root_replay_oracle(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta: &[f32],
) -> BTreeSet<(i32, i32)> {
    let mut pending = vec![Vec::<usize>::new()];
    let mut terminals = BTreeSet::new();
    let mut runs = 0;
    while let Some(prefix) = pending.pop() {
        runs += 1;
        assert!(runs <= 32_768, "compact oracle path count");
        let mut model = LiveModel::new_gekisou(master, deck, notes, &[], params, setup).unwrap();
        let run = model.run_with_random(play, delta, LiveRandom::with_support_prefix(prefix.clone()));
        assert!(model.random.nominal_covers_draws());
        if let Some(branch) = model.random.nominal_branch() {
            assert!(run.is_err());
            assert!(prefix.len() <= 16, "compact oracle depth");
            for choice in 0..branch.len() {
                assert!(branch[choice].weight > 0);
                let mut next = prefix.clone();
                next.push(choice);
                pending.push(next);
            }
        } else {
            run.unwrap();
            assert!(model.random.nominal_prefix_consumed());
            terminals.insert((model.score(), model.current_life()));
        }
    }
    terminals
}

#[test]
fn reachable_maximum_matches_fresh_root_oracle_for_probability_feedback_and_signed_effects() {
    let mut compared = 0;
    let mut needs_fallback = 0;
    let mut variable_life = 0;
    for (effect, value) in [(2000, 5000), (2000, -5000), (3001, 100), (3002, 100), (12006, 3)] {
        for rate in [0, 1, 50, 100] {
            for ranges in [1, 2] {
                let (mut master, original_notes, mut params, mut setup, mut play, _) = fixture();
                master.skill_conditions.iter_mut().find(|row| row.id == 2).unwrap().condition_values = vec![rate];
                let row = master.support_skill_effects.iter_mut().find(|row| row.id == 1).unwrap();
                row.skill_effect_type = effect;
                row.effect_value = value;
                row.effect_execute_limit_count = 1;
                row.skill_target_ids = if effect == 12006 { vec![2] } else { Vec::new() };
                if effect == 12006 {
                    let mut target = master.skill_targets[0].clone();
                    target.id = 2;
                    target.skill_target_type = 4;
                    target.character_id = 0;
                    target.judgement = 5;
                    master.skill_targets.push(target);
                    let mut judgement = master.judgement_parameters[0].clone();
                    judgement.id = 2;
                    judgement.note_simulate_judgement = 3;
                    judgement.score_percent = 50;
                    judgement.damage = 50;
                    master.judgement_parameters.push(judgement);
                    let mut base = master.gekisou_luck_base_points[0].clone();
                    base.id = 2;
                    base.note_simulate_judgement = 3;
                    master.gekisou_luck_base_points.push(base);
                }
                master.reindex().unwrap();
                let notes: Vec<_> = (0..ranges)
                    .flat_map(|range| [100, 300].map(move |time| range * 4000 + time))
                    .enumerate()
                    .map(|(index, time)| LiveNote { note_id: index as i32 + 1, time_ms: time, ..original_notes[0] })
                    .collect();
                setup.fevers = (0..ranges).map(|range| (range * 4000, range * 4000 + 350)).collect();
                params.converted_note_count = notes.len() as i32;
                params.music_length_ms = ranges * 4000;
                play.frames = (0..ranges * 40)
                    .map(|index| PlayFrame {
                        time_ms: index * 100,
                        judged: notes
                            .iter()
                            .filter(|note| note.time_ms == index * 100)
                            .map(|note| JudgedNote {
                                note_id: note.note_id,
                                judgement: 5,
                                judgement_time_ms: note.time_ms,
                            })
                            .collect(),
                    })
                    .collect();
                let delta = vec![0.1; play.frames.len()];
                let deck = [Performer { support_skills: vec![(1, 1)], ..Default::default() }];
                let expected = root_replay_oracle(&master, &deck, &notes, params, &setup, &play, &delta);
                let maximum = expected.iter().map(|&(score, _)| score).max().unwrap();
                variable_life += usize::from(expected.iter().map(|&(_, life)| life).collect::<BTreeSet<_>>().len() > 1);
                let mut session =
                    LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
                let full = session.support(&deck, &mut LuckExactBudget::default(), None, || false).unwrap();
                assert_eq!(full.decline, None, "effect={effect}, rate={rate}, ranges={ranges}");
                assert!(!full.attained_ceiling);
                assert_eq!(full.outcomes.into_iter().collect::<BTreeSet<_>>(), expected);
                let peak = session.support(&deck, &mut LuckExactBudget::default(), Some(maximum), || false).unwrap();
                assert_eq!(peak.decline, None, "effect={effect}, rate={rate}, ranges={ranges}");
                assert!(peak.attained_ceiling);
                assert_eq!(peak.outcomes.iter().map(|&(score, _)| score).max(), Some(maximum));
                assert!(peak.outcomes.iter().all(|outcome| expected.contains(outcome)));
                needs_fallback += usize::from(peak.stats.replay_runs > 1);
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 40);
    assert!(needs_fallback > 0, "locally greatest draws need not maximize the final score");
    assert!(variable_life > 0, "the matrix includes lottery-dependent terminal life");
}

#[test]
fn loose_ceiling_returns_complete_support_and_interrupted_witnesses_publish_nothing() {
    let (master, mut notes, mut params, setup, mut play, delta) = fixture();
    notes.push(LiveNote { note_id: 2, ..notes[0] });
    params.converted_note_count = 2;
    play.frames[1].judged.push(JudgedNote { note_id: 2, judgement: 5, judgement_time_ms: 100 });
    let expected = root_replay_oracle(&master, &[], &notes, params, &setup, &play, &delta);
    let maximum = expected.iter().map(|&(score, _)| score).max().unwrap();
    let mut session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 0).unwrap();
    let complete = session.support(&[], &mut LuckExactBudget::default(), Some(maximum + 1), || false).unwrap();
    assert_eq!(complete.decline, None);
    assert!(!complete.attained_ceiling);
    assert_eq!(complete.outcomes.into_iter().collect::<BTreeSet<_>>(), expected);
    let incomplete = session
        .support(
            &[],
            &mut LuckExactBudget { remaining_runs: 1, remaining_frames: play.frames.len() as u64 },
            Some(maximum + 1),
            || false,
        )
        .unwrap();
    assert_eq!(incomplete.decline, Some(LuckExactDecline::WorkBudget));
    assert!(incomplete.outcomes.is_empty());
    assert!(!incomplete.attained_ceiling);
    let mut checks = 0;
    let cancelled = session
        .support(&[], &mut LuckExactBudget::default(), Some(maximum), || {
            checks += 1;
            checks == 8
        })
        .unwrap();
    assert_eq!(cancelled.decline, Some(LuckExactDecline::Cancelled));
    assert!(cancelled.stats.frames > 0 && cancelled.stats.frames < play.frames.len() as u64);
    assert!(cancelled.outcomes.is_empty());
    assert!(!cancelled.attained_ceiling);
}

#[test]
fn cached_maximum_keeps_its_certificate_without_claiming_complete_support() {
    let (master, notes, params, setup, play, delta) = fixture();
    let expected = root_replay_oracle(&master, &[], &notes, params, &setup, &play, &delta);
    let maximum = expected.iter().map(|&(score, _)| score).max().unwrap();
    let mut session = LuckExactSession::new(&master, &notes, &[], params, &setup, &play, &delta, None, 2).unwrap();
    let first = session.support(&[], &mut LuckExactBudget::default(), Some(maximum), || false).unwrap();
    assert!(first.attained_ceiling);
    let mut empty = LuckExactBudget { remaining_runs: 0, remaining_frames: 0 };
    let larger = session.support(&[], &mut empty, Some(maximum + 1), || false).unwrap();
    assert!(larger.attained_ceiling);
    assert_eq!(larger.outcomes, first.outcomes);
    assert_eq!(larger.stats, LuckExactStats::default());
    assert!(session.support(&[], &mut empty, Some(maximum - 1), || false).is_err());
    assert_eq!(session.support(&[], &mut empty, None, || false).unwrap().decline, Some(LuckExactDecline::WorkBudget));
    assert_eq!(
        session.support(&[], &mut empty, Some(maximum), || true).unwrap().decline,
        Some(LuckExactDecline::Cancelled)
    );
}
