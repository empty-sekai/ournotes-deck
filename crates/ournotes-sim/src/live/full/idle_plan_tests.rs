//! Differential tests against the unchanged updater path, including private pools.
use super::*;
use crate::live::full::idle_plan::{take_idle_plan_stats, with_idle_plan_disabled};
use crate::live::full::life::LifeController;
use crate::live::random::LiveRandom;

fn effect(id: i64, phase: i64, trigger: Checker) -> CondEffect {
    CondEffect {
        effect_id: id,
        trigger_type: ONE_SHOT,
        act: 0.025,
        phase,
        trigger: Some(trigger),
        condition: Some(Checker::Probability(1.0)),
        execute_limit: 0,
        reset: None,
        row: id as usize,
        cumulative: Some(Cumulative::ElapsedTime { period: 10, elapsed: 0, previous: 0, count: 0, max: 1000 }),
    }
}

fn model() -> ConditionSkillUpdater {
    let mut never = effect(30, 2, Checker::Fixed(false));
    // A false trigger must not evaluate even an invalid condition.
    never.condition = Some(Checker::LifeAtLeast(None));
    ConditionSkillUpdater::new(
        vec![effect(20, 2, Checker::SameMemberLiveSkill(0)), effect(10, 1, Checker::SameMemberLiveSkill(0)), never],
        |_| Ok(Some(Checker::Probability(0.0))),
        None,
    )
    .unwrap()
}

fn same_state(actual: &ConditionSkillUpdater, reference: &ConditionSkillUpdater) {
    assert_eq!(actual.executing, reference.executing);
    assert_eq!(actual.stacks, reference.stacks);
    assert_eq!(actual.execute_count, reference.execute_count);
    assert_eq!(actual.trigger_checked, reference.trigger_checked);
    assert_eq!(format!("{:?}", actual.cache), format!("{:?}", reference.cache));
    assert_eq!(format!("{:?}", actual.effects), format!("{:?}", reference.effects));
    assert_eq!(format!("{:?}", actual.updaters), format!("{:?}", reference.updaters));
    assert_eq!(format!("{:?}", actual.sustained), format!("{:?}", reference.sustained));
}

fn context<'a>(
    life: &'a mut LifeController,
    random: &'a mut LiveRandom,
    time: i32,
    events: &'a [(i32, i32)],
) -> CheckCtx<'a> {
    CheckCtx {
        life,
        random,
        frame_time: time,
        judged: &[],
        events,
        gk: None,
        prev_confirmed_rank: None,
        current_combo: 0,
    }
}

#[test]
fn event_sleep_preserves_phase_order_rng_cumulative_and_recycling() {
    take_idle_plan_stats();
    let mut actual = model();
    let mut reference = with_idle_plan_disabled(model);
    assert!(actual.idle_plan.is_some());
    assert!(reference.idle_plan.is_none());
    let mut life_a = LifeController::new(1000, HashMap::new(), 10_000).unwrap();
    let mut life_b = LifeController::new(1000, HashMap::new(), 10_000).unwrap();
    let mut random_a = LiveRandom::new(-17);
    let mut random_b = LiveRandom::new(-17);
    let mut executions = 0;
    for frame in 0..80 {
        let t = frame * 10;
        // More than one full pool's sequential executions, plus overlapping ones.
        let events = if frame % 8 == 2 || frame % 8 == 3 { vec![(0, t - 3)] } else { vec![(1, t)] };
        let inp = FrameInput { time_ms: t, music_length_ms: 1000, is_live_finished: frame >= 75 };
        actual.begin_frame();
        reference.begin_frame();
        for phase in [1, 2] {
            let a = actual.update(phase, inp, &mut context(&mut life_a, &mut random_a, t, &events)).unwrap();
            let b = reference.update(phase, inp, &mut context(&mut life_b, &mut random_b, t, &events)).unwrap();
            assert_eq!(a, b, "frame {frame} phase {phase}");
            executions += a.iter().filter(|&&u| actual.updaters[u].state.state == EXECUTE_FRAME).count();
            same_state(&actual, &reference);
            assert_eq!(random_a.draws(), random_b.draws());
        }
    }
    let stats = take_idle_plan_stats();
    assert!(stats.slept_frames > 0 && stats.slept_calls > stats.slept_frames);
    assert!(executions > 10);
    assert_eq!(stats.enabled_eligible_updaters, 1);
    assert_eq!(stats.eligible_updaters, 2);
}

#[test]
fn trigger_cached_in_first_phase_must_still_activate_second_phase() {
    let mut updater =
        ConditionSkillUpdater::new(vec![effect(1, 2, Checker::SameMemberLiveSkill(0))], |_| Ok(None), None).unwrap();
    let mut life = LifeController::new(1000, HashMap::new(), 10_000).unwrap();
    let mut random = LiveRandom::new(1);
    let mut ctx = CheckCtx {
        life: &mut life,
        random: &mut random,
        frame_time: 100,
        judged: &[],
        events: &[(0, 97)],
        gk: None,
        prev_confirmed_rank: None,
        current_combo: 0,
    };
    let inp = FrameInput { time_ms: 100, music_length_ms: 1000, is_live_finished: false };
    updater.begin_frame();
    assert!(updater.update(1, inp, &mut ctx).unwrap().is_empty());
    // Even changing the supplied events between calls cannot erase the cached hit.
    ctx.events = &[];
    let changed = updater.update(2, inp, &mut ctx).unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(updater.updaters[changed[0]].state.execute_ms, 100);
}

#[test]
fn composite_reset_and_sustained_observers_are_not_admitted() {
    let mut reset = effect(1, 2, Checker::Fixed(false));
    reset.reset = Some(Checker::Probability(0.5));
    assert!(IdleTriggerPlan::compile(&[reset]).is_none());
    let mut sustained = effect(1, 2, Checker::Fixed(false));
    sustained.trigger_type = SUSTAINED;
    assert!(IdleTriggerPlan::compile(&[sustained]).is_none());
    let composite =
        Checker::And { items: vec![Checker::Probability(0.5), Checker::Fixed(false)], resettable: vec![false, false] };
    assert!(IdleTriggerPlan::compile(&[effect(1, 2, composite)]).is_none());
    assert!(IdleTriggerPlan::compile(&[effect(1, 2, Checker::Not(Box::new(Checker::Fixed(true))))]).is_none());
}

#[test]
fn idle_path_keeps_gate_errors_and_live_finished_cache_semantics() {
    let build =
        || ConditionSkillUpdater::new(vec![effect(1, 2, Checker::Fixed(false))], |_| Ok(None), Some(1)).unwrap();
    for finished in [false, true] {
        let mut actual = build();
        let mut reference = with_idle_plan_disabled(build);
        let mut life = LifeController::new(1000, HashMap::new(), 10_000).unwrap();
        let mut random = LiveRandom::new(1);
        let mut ctx = CheckCtx {
            life: &mut life,
            random: &mut random,
            frame_time: 0,
            judged: &[],
            events: &[],
            gk: None,
            prev_confirmed_rank: None,
            current_combo: 0,
        };
        let inp = FrameInput { time_ms: 0, music_length_ms: 1000, is_live_finished: finished };
        let a = actual.update(1, inp, &mut ctx);
        let b = reference.update(1, inp, &mut ctx);
        assert_eq!(a.is_ok(), finished);
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        same_state(&actual, &reference);
    }
}
