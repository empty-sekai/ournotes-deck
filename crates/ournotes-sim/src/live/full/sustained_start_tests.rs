//! Native checks for the spacing of untimed sustained fixed-factor starts.
use super::*;
use crate::live::full::conditions::GkView;
use crate::live::full::gekisou::{Controller, M_LUCK, S_COMPLETE, S_FINISH, S_PLAYING, S_START};
use crate::live::full::life::LifeController;
use crate::live::random::LiveRandom;
use crate::master::Master;

fn effect(phase: i64, trigger: Checker, condition: Option<Checker>) -> CondEffect {
    CondEffect {
        effect_id: phase,
        trigger_type: SUSTAINED,
        act: 0.0,
        phase,
        trigger: Some(trigger),
        condition,
        execute_limit: 1,
        reset: None,
        row: 0,
        cumulative: None,
    }
}

fn step(
    updater: &mut ConditionSkillUpdater,
    time: i32,
    finished: bool,
    judged: &[(i32, i32, i32)],
    events: &[(i32, i32)],
    controller: Option<&Controller>,
) -> Vec<(usize, EffectState)> {
    let mut life = LifeController::new(1000, FxHashMap::default(), 10_000).unwrap();
    let mut random = LiveRandom::new(-9);
    let mut ctx = CheckCtx {
        life: &mut life,
        random: &mut random,
        frame_time: time,
        current_combo: 0,
        judged,
        events,
        gk: controller.map(|ctrl| GkView { ctrl, prev_lots: &[], prev_lot_ms: 0 }),
        prev_confirmed_rank: None,
    };
    let input = FrameInput { time_ms: time, music_length_ms: 10_000, is_live_finished: finished };
    updater.begin_frame();
    let mut states = Vec::new();
    for phase in [1, 2] {
        for u in updater.update(phase, input, &mut ctx).unwrap() {
            states.push((u, updater.updaters[u].state));
        }
    }
    states
}

#[test]
fn sustained_start_spacing_covers_all_short_trigger_and_condition_traces() {
    // Distinct frames can share a time, and a judgement can backdate its trigger before a negative frame time.
    let times = [-100, -60, -60, 10, 150, 151];
    for phase in [1, 2] {
        for trigger_mask in 0u32..1 << times.len() {
            for condition_mask in 0u32..1 << times.len() {
                let trigger = Checker::NoteJudgementCount {
                    n: 1,
                    consecutive: false,
                    targets: vec![5],
                    count: 0,
                    override_ms: None,
                };
                let mut row = effect(phase, trigger, Some(Checker::SameMemberLiveSkill(1)));
                row.reset = Some(Checker::Fixed(true));
                let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), None).unwrap();
                updater.execute_count.insert(phase, 99);
                let mut starts = Vec::new();
                for (frame, &time) in times.iter().enumerate() {
                    let judged = [(frame as i32, 5, time - 17)];
                    let events = [(1, time)];
                    let judged = if trigger_mask & (1 << frame) != 0 { judged.as_slice() } else { &[] };
                    let events = if condition_mask & (1 << frame) != 0 { events.as_slice() } else { &[] };
                    let changed = step(&mut updater, time, false, judged, events, None);
                    let fresh: Vec<_> = changed.iter().filter(|(_, state)| state.state == EXECUTE_FRAME).collect();
                    assert!(fresh.len() <= 1);
                    if let Some((_, state)) = fresh.first() {
                        assert!(!judged.is_empty() && !events.is_empty());
                        assert_eq!(state.execute_ms, time - 17);
                        starts.push(frame);
                    }
                }
                assert!(starts.windows(2).all(|pair| pair[1] > pair[0] + 1));
            }
        }
    }
}

#[test]
fn sustained_start_spacing_retains_reuse_beyond_one_updater_pool() {
    let row = effect(2, Checker::SameMemberLiveSkill(0), Some(Checker::Fixed(true)));
    let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), None).unwrap();
    // The one-shot counter limit is not a sustained lifetime-start limit, even without a reset checker.
    updater.execute_count.insert(2, 99);
    let mut starts = Vec::new();
    let mut instances = Vec::new();
    for frame in 0..25 {
        let time = frame * 10;
        let events = [(0, time)];
        let events = if frame % 2 == 0 { events.as_slice() } else { &[] };
        for (u, state) in step(&mut updater, time, false, &[], events, None) {
            if state.state == EXECUTE_FRAME {
                starts.push(frame);
                instances.push(u);
            }
        }
    }
    assert_eq!(starts, (0..25).step_by(2).collect::<Vec<_>>());
    assert_eq!(instances, (0..13).map(|i| i % POOL).collect::<Vec<_>>());
    assert!(starts.len() > POOL);
    assert_eq!(updater.execute_count.get(&2), Some(&99));
}

#[test]
fn sustained_start_spacing_survives_luck_completion_gate_gaps_and_finished_frames() {
    let mut controller = Controller::new(
        vec![(-100, 500, M_LUCK), (600, 1000, M_LUCK)],
        std::iter::empty(),
        &Master::default(),
        100,
        100,
        0,
        0,
    )
    .unwrap();
    let rows = [1, 2]
        .map(|phase| {
            let condition = Checker::NoteJudgementCount {
                n: 2,
                consecutive: false,
                targets: vec![5],
                count: 19,
                override_ms: None,
            };
            let mut row = effect(phase, Checker::LuckRushPlaying(false), Some(condition));
            row.reset = Some(Checker::Fixed(true));
            row
        })
        .into();
    let mut updater = ConditionSkillUpdater::new(rows, |_| Ok(None), Some(M_LUCK)).unwrap();
    let mut starts: [Vec<usize>; 2] = std::array::from_fn(|_| Vec::new());
    let mut ends: [Vec<usize>; 2] = std::array::from_fn(|_| Vec::new());
    for frame in 0..14 {
        let range = usize::from(frame >= 7);
        let closed = matches!(frame, 1 | 4 | 10 | 13);
        let finished = frame == 9;
        controller.current_playing_index = if closed { -1 } else { range as i32 };
        controller.state_updates.clear();
        controller.states[range].state = match frame {
            6 => S_COMPLETE,
            7 => S_START,
            12 => S_FINISH,
            _ => S_PLAYING,
        };
        controller.states[range].luck.rush_combo = i32::from(!matches!(frame, 4 | 9 | 10));
        if matches!(frame, 6 | 7 | 12) {
            controller.state_updates.push(range);
        }
        let time = frame as i32 * 80 - 100;
        let judged = [(frame as i32, 5, time - 3)];
        for (u, state) in step(&mut updater, time, finished, &judged, &[], Some(&controller)) {
            let effect = updater.updaters[u].effect;
            if state.state == EXECUTE_FRAME {
                starts[effect].push(frame);
            } else if state.state == END_FRAME {
                ends[effect].push(frame);
            }
        }
    }
    assert_eq!(starts, [vec![2, 8], vec![2, 8]]);
    assert_eq!(ends, [vec![6, 12], vec![6, 12]]);
}

#[test]
fn sustained_start_spacing_holds_in_the_open_gate_observation_sequence() {
    let mut controller =
        Controller::new(vec![(-100, 1000, M_LUCK)], std::iter::empty(), &Master::default(), 100, 100, 0, 0).unwrap();
    controller.states[0].state = S_PLAYING;
    // Every length-five trace of: closed gate, open/false trigger, open/false condition,
    // open/true trigger and condition, and an open gate on a finished frame.
    let times = [-100, -60, -60, 10, 150];
    for phase in [1, 2] {
        for mut trace in 0..5usize.pow(times.len() as u32) {
            let mut row = effect(phase, Checker::LuckRushPlaying(false), Some(Checker::SameMemberLiveSkill(1)));
            row.reset = Some(Checker::Fixed(true));
            let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), Some(M_LUCK)).unwrap();
            let mut observed = 0usize;
            let mut starts = Vec::new();
            for &time in &times {
                let symbol = trace % 5;
                trace /= 5;
                let open = symbol != 0;
                observed += usize::from(open);
                controller.current_playing_index = if open { 0 } else { -1 };
                controller.state_updates.clear();
                controller.states[0].luck.rush_combo = i32::from(symbol != 1);
                let events = [(1, time)];
                let events = if symbol == 2 { &[] } else { events.as_slice() };
                for (_, state) in step(&mut updater, time, symbol == 4, &[], events, Some(&controller)) {
                    if state.state == EXECUTE_FRAME {
                        assert_eq!(symbol, 3);
                        starts.push(observed);
                    }
                }
            }
            assert!(starts.windows(2).all(|pair| pair[1] > pair[0] + 1));
            assert!(starts.len() <= observed.div_ceil(2));
        }
    }
}

#[test]
fn one_shot_factor_starts_do_not_exceed_counted_judgement_hits() {
    let times = [i32::MIN + 40, -100, -60, -60, 0, 40, i32::MAX - 40, i32::MAX];
    for phase in [1, 2] {
        for targets in [vec![5], vec![5, 5], vec![5, 6], vec![6, 6, 5]] {
            for n in [1, 2, 3, 4, 9] {
                for consecutive in [false, true] {
                    for mask in 0..64usize {
                        let trigger = Checker::NoteJudgementCount {
                            n,
                            consecutive,
                            targets: targets.clone(),
                            count: 0,
                            override_ms: None,
                        };
                        let mut row = effect(phase, trigger, Some(Checker::SameMemberLiveSkill(1)));
                        row.trigger_type = ONE_SHOT;
                        row.act = 0.05;
                        row.reset = Some(Checker::Fixed(true));
                        let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), None).unwrap();
                        let mut hits = 0usize;
                        let mut starts = 0usize;
                        for (frame, &time) in times.iter().enumerate() {
                            // Each of six declared entries may have either of two converted judgements.
                            // Duplicate targets count separately; consecutive resets only discard counts.
                            let judgement = if mask & (1 << frame) == 0 { 5 } else { 6 };
                            let judged = [(frame as i32 - 1, judgement, time.wrapping_sub(17))];
                            let judged = if frame < 6 { judged.as_slice() } else { &[] };
                            hits += judged
                                .iter()
                                .map(|&(_, j, _)| targets.iter().filter(|&&target| target == i64::from(j)).count())
                                .sum::<usize>();
                            let events = [(1, time)];
                            let events = if frame % 3 == 1 { &[] } else { events.as_slice() };
                            starts += step(&mut updater, time, frame == 7, judged, events, None)
                                .iter()
                                .filter(|(_, state)| state.state == EXECUTE_FRAME)
                                .count();
                        }
                        assert!(starts <= hits / n as usize);
                    }
                }
            }
        }
    }
}

#[test]
fn lone_count_hit_frames_survive_conditions_limits_and_original_target_multiplicity() {
    let times: [i32; 12] = [0, 40, 40, 80, 120, 160, 200, 240, 280, 320, 360, 400];
    let counts = [1usize, 3, 2, 0, 1, 2, 1, 0, 2, 1, 0, 0];
    for phase in [1, 2] {
        for copies in [1, 2, 4] {
            let targets: Vec<_> = (0..copies).flat_map(|_| [5, 6]).collect();
            for n in [1, 2, 3, 5, 9, i32::MAX as i64] {
                for grade_mask in 0..4usize {
                    for reset in [false, true] {
                        let trigger = Checker::NoteJudgementCount {
                            n,
                            consecutive: false,
                            targets: targets.clone(),
                            count: 0,
                            override_ms: None,
                        };
                        let mut row = effect(phase, trigger, Some(Checker::SameMemberLiveSkill(1)));
                        row.trigger_type = ONE_SHOT;
                        row.act = 0.05;
                        row.execute_limit = 2;
                        row.reset = reset.then_some(Checker::Fixed(true));
                        let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), None).unwrap();
                        let (mut entries, mut count) = (0usize, 0i64);
                        for (frame, &time) in times.iter().enumerate() {
                            let before = count;
                            let mut override_time = None;
                            let judged: Vec<_> = (0..counts[frame])
                                .map(|_| {
                                    let id = if entries % 3 == 0 { -1 } else { 7 };
                                    let grade = if grade_mask & (1 << (entries % 2)) == 0 { 5 } else { 6 };
                                    let chart_time = time.saturating_sub((entries % 4) as i32 * 17);
                                    let old = count;
                                    count += copies;
                                    if old / n != count / n {
                                        override_time = Some(if id < 0 { time } else { chart_time });
                                    }
                                    entries += 1;
                                    (id, grade, chart_time)
                                })
                                .collect();
                            let events = [(1, time)];
                            let events = if frame % 3 == 1 { &[] } else { events.as_slice() };
                            let fresh: Vec<_> = step(&mut updater, time, false, &judged, events, None)
                                .into_iter()
                                .filter(|(_, state)| state.state == EXECUTE_FRAME)
                                .collect();
                            assert!(fresh.len() <= 1);
                            for (_, state) in fresh {
                                assert!(before / n < count / n);
                                assert!(!events.is_empty());
                                assert_eq!(state.execute_ms, override_time.unwrap());
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn recycled_one_shot_instances_can_overlap_beyond_the_pool_at_backdated_times() {
    let trigger =
        Checker::NoteJudgementCount { n: 1, consecutive: false, targets: vec![5], count: 0, override_ms: None };
    let mut row = effect(2, trigger, None);
    row.trigger_type = ONE_SHOT;
    row.act = 0.05;
    row.execute_limit = 0;
    let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), None).unwrap();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for frame in 0..65 {
        let time = frame * 40;
        // Twelve uses of the same chart note, each long after the previous instance has ended and recycled.
        let judged = [(7, 5, 0)];
        let judged = if frame < 60 && frame % 5 == 0 { judged.as_slice() } else { &[] };
        for (u, state) in step(&mut updater, time, false, judged, &[], None) {
            if state.state == EXECUTE_FRAME {
                starts.push((u, state.execute_ms));
            } else if state.state == END_FRAME {
                ends.push(state.finish_ms);
            }
        }
    }
    assert_eq!(starts.len(), 12);
    assert_eq!(ends.len(), 12);
    assert!(starts.iter().all(|&(_, time)| time == 0));
    assert!(ends.iter().all(|&time| time > 0));
    assert!(starts.windows(2).any(|pair| pair[0].0 == pair[1].0));
    assert!(starts.len() > POOL);
}

#[test]
fn timed_sustained_can_start_in_adjacent_frames_and_requires_the_guard() {
    let mut row = effect(2, Checker::Fixed(true), Some(Checker::Fixed(true)));
    row.act = 1.0;
    let mut updater = ConditionSkillUpdater::new(vec![row], |_| Ok(None), None).unwrap();
    let mut starts = Vec::new();
    for frame in 0..3 {
        for (_, state) in step(&mut updater, frame * 10, false, &[], &[], None) {
            if state.state == EXECUTE_FRAME {
                starts.push(frame);
            }
        }
    }
    assert_eq!(starts, [0, 1, 2]);
}
