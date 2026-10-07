//! Control-state checks for admitted upper-bound preparation and complete all-order scoring.
use super::certified_search::*;
use super::expectation::{FiniteSeedContext, PhysicalDeck, context};
use super::gate_tests::common::{Rng, extend_table, replace_table, roster, set_column, synth_snaps};
use super::{GekisouObjective, Objective, PlayInput, SeedSet, uniform};
use ournotes_sim::live::full::{LuckDpCache, LuckRushDecline, LuckRushPreparation, LuckSkills, luck_skills};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::live::score::LiveScoreSettings;
use ournotes_sim::live::skip::{Chart, ChartNote};
use ournotes_sim::master::Master;
use ournotes_sim::pool::Pool;
use ournotes_sim::scenario::{ContextInput, PowerSnapshotInput, Scenario};
use serde_json::json;
use std::cell::Cell;

fn input() -> (Master, LuckSkills, FiniteSeedContext) {
    input_with_order_skills(false)
}

fn input_with_order_skills(order_skills: bool) -> (Master, LuckSkills, FiniteSeedContext) {
    let mut source = synth_snaps(&mut Rng::new(141), 5, 0, &[]);
    set_column(&mut source, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    set_column(&mut source, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(2);
        row["_gekisouMission2"] = json!(3);
        row["_gekisouMission3"] = json!(1);
    });
    let live_effects = if order_skills {
        set_column(&mut source, "MasterMemberCard", &mut |row| {
            let id = row["_id"].as_i64().unwrap();
            row["_liveSkillID"] = json!(if id <= 2 { 9100 + id } else { 0 });
            row["_gekisouSkillID"] = json!(0);
        });
        json!([
            {"_id":9101,"_liveSkillID":9101,"_level":1,"_skillEffectType":2000,"_effectValue":3500,"_activationTimeSecond":0.24},
            {"_id":9102,"_liveSkillID":9102,"_level":1,"_skillEffectType":2000,"_effectValue":9000,"_activationTimeSecond":0.24}
        ])
    } else {
        json!([])
    };
    replace_table(&mut source, "MasterLiveSkillEffect", live_effects);
    extend_table(
        &mut source,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":if order_skills {"10"} else {"40"}}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":if order_skills {"10"} else {"20"}}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBasePoint",
        json!([{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":10}]),
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBonusLot",
        json!(
            (0..5)
                .flat_map(|kind| (0..4).map(move |result| json!({
                    "_id":kind*10+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1
                })))
                .collect::<Vec<_>>()
        ),
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouRankingScoreBonus",
        json!(
            (1..=3)
                .flat_map(|pattern| (1..=3).map(move |count| json!({
                    "_id":pattern*10+count,"_missionPattern":pattern,"_count":count,"_rank":1,"_scoreBonusPercent":10
                })))
                .collect::<Vec<_>>()
        ),
    );
    let master = source.master();
    let mut owned = roster(&mut Rng::new(142), &master);
    if order_skills {
        for member in &mut owned.members {
            member.live_skill_level = 1;
        }
    }
    let pool = Pool::new(&master, &owned).unwrap();
    // The stochastic order fixture uses two notes that each fill its short LUCK gauge. The first native
    // Critical/Miss result can affect the second note while the ordinary first-position skill remains active.
    let note_times = if order_skills { vec![100, 130] } else { vec![100] };
    let chart = Chart::from_notes(
        note_times
            .into_iter()
            .enumerate()
            .map(|(id, time_ms)| ChartNote { id: id as i32 + 1, time_ms, note_type: 1 })
            .collect(),
        if order_skills { vec![ournotes_sim::live::skip::SkillEvent { index: 0, time_ms: 60 }] } else { vec![] },
        &LiveScoreSettings::from_master(&master).unwrap(),
    )
    .unwrap();
    let stream = JudgementStream::theoretical_best(&chart);
    let judgement_types = vec![1; chart.notes.len()];
    let resolved = ContextInput {
        power_snapshot: PowerSnapshotInput { event_ids: vec![], captured_jst_ticks: None },
        result_clock: None,
        event_payoff: None,
    }
    .resolve(&master, Scenario::Mission(10), Some(1004), &[(50, 150)])
    .unwrap();
    let objective = Objective::LiveScore {
        score_id: 1004,
        chart,
        play: PlayInput::Stream { stream, judgement_types },
        event: false,
        exclude_snap_skills: false,
        gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: vec![(50, 150)] }),
    }
    .in_scenario(resolved);
    let physical = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
    let input = context(&pool, &physical, &objective).unwrap();
    let skills = luck_skills(&master).unwrap();
    (master, skills, input)
}

#[test]
fn best_order_cancellation_keeps_one_evaluated_order_and_all_pending_caps() {
    let (master, _, mut input) = input();
    input.gekisou = None;
    let mut calls = 0;
    let mut basis = [4, 2, 0, 3, 1];
    let caps = vec![i128::from(i32::MAX); uniform::ORDERS];
    let partial = evaluate_best_order_context(&master, None, &input, basis, &caps, None, None, || {
        calls += 1;
        calls >= 2
    })
    .unwrap()
    .unwrap();
    let witness = partial.best_order.unwrap();
    assert_eq!(witness.evaluated_orders, 1);
    assert_eq!(witness.order, [0, 1, 2, 3, 4]);
    assert!(!witness.optimal);
    assert_eq!(partial.score.upper(), f64::from(i32::MAX));
    assert_eq!(partial.exact_score, None);
    basis.sort_unstable();
    assert!(evaluate_best_order_context(&master, None, &input, basis, &caps, None, None, || true).unwrap().is_none());
}

#[test]
fn best_order_luck_expectation_encloses_independent_native_branch_means() {
    use super::expectation::ExactExpectation;
    use super::interval_topk::{compare_exact, exact_in_interval};
    use ournotes_sim::live::full::{LuckExactBudget, LuckExactSession};
    let (master, skills, input) = input_with_order_skills(true);
    let mut exact = LuckExactSession::new(
        &master,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
        0,
    )
    .unwrap();
    let mut work = LuckExactBudget::default();
    let mut oracle = Vec::new();
    let mut reachable = i32::MIN;
    for order in uniform::all_orders() {
        let performers = order.map(|slot| input.performers[slot].clone());
        let attempt = exact.law(&performers, &mut work, || false).unwrap();
        let law = attempt.law.expect("finite synthetic nominal branches");
        let denominator =
            law.atoms().iter().fold(1u128, |value, atom| value.checked_mul(atom.mass.denominator).unwrap());
        let numerator = law
            .atoms()
            .iter()
            .map(|atom| {
                reachable = reachable.max(atom.score);
                i128::from(atom.score)
                    * i128::try_from(atom.mass.numerator).unwrap()
                    * i128::try_from(denominator / atom.mass.denominator).unwrap()
            })
            .sum();
        oracle.push((order, ExactExpectation { numerator, denominator }));
    }
    assert!(oracle.iter().any(|row| !compare_exact(row.1, oracle[0].1).unwrap().is_eq()));
    oracle.sort_by(|a, b| compare_exact(b.1, a.1).unwrap().then(a.0.cmp(&b.0)));
    let (best_order, maximum) = oracle[0];
    assert!(
        compare_exact(maximum, ExactExpectation { numerator: i128::from(reachable), denominator: 1 }).unwrap().is_lt()
    );
    let mut canonical = input.clone();
    let (_, basis) = canonicalize_performers_with_basis(&mut canonical);
    for capacity in [0, 1 << 20] {
        let mut curves = LuckDpCache::new(capacity);
        let result = evaluate_best_order_context(
            &master,
            Some(&skills),
            &canonical,
            basis,
            &vec![i128::from(i32::MAX); uniform::ORDERS],
            None,
            Some(&mut curves),
            || false,
        )
        .unwrap()
        .unwrap();
        assert!(exact_in_interval(maximum, result.score).unwrap());
        for order in &result.orders {
            let value = oracle.iter().find(|row| row.0 == order.order).unwrap().1;
            assert!(exact_in_interval(value, order.mean).unwrap());
        }
        let witness = result.best_order.unwrap();
        let value = oracle.iter().find(|row| row.0 == witness.order).unwrap().1;
        assert!(exact_in_interval(value, witness.mean).unwrap());
        assert!(witness.evaluated_orders > 0);
        if witness.optimal {
            assert_eq!(witness.order, best_order);
        }
        if let Some(value) = result.exact_score {
            assert!(compare_exact(value, maximum).unwrap().is_eq());
        }
    }
}

#[test]
fn nonlinear_order_evaluation_retains_the_existing_score_enclosures_and_refinement_maps() {
    let (master, skills, input) = input();
    let mut session = ournotes_sim::live::full::LuckScoreSession::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
    );
    let mut expected = Vec::new();
    for order in uniform::all_orders() {
        let performers = order.map(|slot| input.performers[slot].clone());
        let summary = session.summary(&performers, None, || false).unwrap().unwrap();
        let support = (summary.final_support.lower, summary.final_support.upper);
        expected.push(OrderScoreInterval {
            order,
            evaluated: true,
            mean: ournotes_sim::live::certified::F64Interval::new(summary.final_mean.lower, summary.final_mean.upper)
                .unwrap()
                .intersect(
                    ournotes_sim::live::certified::F64Interval::new(f64::from(support.0), f64::from(support.1))
                        .unwrap(),
                )
                .unwrap(),
            support,
            exact_mean: summary
                .exact_constant_score
                .map(|value| super::expectation::ExactExpectation { numerator: i128::from(value), denominator: 1 }),
            final_life: summary.exact_final_life.map(|life| (life, life)),
            tails: Default::default(),
            refined_payoff: None,
        });
    }
    let mid = expected[0].support.0.saturating_add((expected[0].support.1 - expected[0].support.0) / 2);
    for map in [
        PayoffMap::ScoreAtLeast { threshold: mid },
        PayoffMap::CappedScore { threshold: mid },
        PayoffMap::ScoreAndLifeAtLeast { threshold: mid, min_final_life: 1 },
    ] {
        let old = aggregate_orders(expected.clone(), &map).unwrap();
        let actual = evaluate_luck_context(&master, &skills, &input, &map, None, || false).unwrap().unwrap();
        assert_eq!(
            (actual.score, actual.payoff, actual.exact_score, actual.exact_payoff),
            (old.score, old.payoff, old.exact_score, old.exact_payoff)
        );
        for (a, b) in actual.orders.iter().zip(&expected) {
            assert_eq!((a.order, a.mean, a.support, a.final_life), (b.order, b.mean, b.support, b.final_life));
        }
    }
}

#[test]
fn all_preparations_and_scores_preserve_the_complete_canonical_aggregate() {
    let (master, skills, input) = input();
    let reference =
        evaluate_luck_context(&master, &skills, &input, &PayoffMap::Score, None, || false).unwrap().unwrap();
    for capacity in [0, 1 << 20] {
        let mut cache = LuckDpCache::new(capacity);
        let schedule: Vec<_> = (0..uniform::ORDERS).rev().collect();
        let (mut prepared, mut scored, mut finished) = (Vec::new(), Vec::new(), 0);
        let result = evaluate_luck_context_bounded(
            &master,
            &skills,
            &input,
            &PayoffMap::Score,
            Some(&mut cache),
            (&schedule, true, |event| {
                match event {
                    LuckContextEvent::UpperPrepared { index, preparation } => {
                        assert!(matches!(preparation, LuckRushPreparation::Ready(_)), "{preparation:?}");
                        prepared.push(index);
                    }
                    LuckContextEvent::Scored { index, .. } => scored.push(index),
                    LuckContextEvent::UpperFinished { .. } => finished += 1,
                    _ => {}
                }
                true
            }),
            || false,
        )
        .unwrap();
        let LuckContextOutcome::Full(actual) = result else { panic!("all orders must supply the aggregate") };
        assert_eq!(prepared, schedule);
        assert_eq!(scored, schedule);
        assert_eq!(finished, 1);
        assert_eq!(actual.score, reference.score);
        assert_eq!(actual.payoff, reference.payoff);
        assert_eq!(actual.exact_score, reference.exact_score);
        assert_eq!(actual.orders.len(), uniform::ORDERS);
        for (a, b) in actual.orders.iter().zip(&reference.orders) {
            assert_eq!((a.order, a.mean, a.support, a.final_life), (b.order, b.mean, b.support, b.final_life));
        }
    }
}

#[test]
fn upper_only_and_cancelled_preparations_never_supply_a_complete_evaluation() {
    let (master, skills, input) = input();
    let schedule: Vec<_> = (0..uniform::ORDERS).collect();
    // A caller may close its independently proved cap, cancel after a partial prepass, cancel inside
    // preparation, or stop after all 120 preparations. None of those actions completed an order score.
    for stop in ["exclude", "between", "inside", "after-all"] {
        let cancelled = Cell::new(false);
        let (mut prepared, mut scored, mut stopped, mut finished) = (0, 0, 0, 0);
        let mut cache = LuckDpCache::new(1 << 20);
        let result = evaluate_luck_context_bounded(
            &master,
            &skills,
            &input,
            &PayoffMap::Score,
            Some(&mut cache),
            (&schedule, true, |event| {
                match event {
                    LuckContextEvent::UpperAttempt if stop == "inside" => cancelled.set(true),
                    LuckContextEvent::UpperPrepared { preparation: LuckRushPreparation::Ready(_), .. } => {
                        prepared += 1;
                        if prepared == 2 {
                            if stop == "exclude" {
                                return false;
                            }
                            if stop == "between" {
                                cancelled.set(true);
                            }
                        }
                    }
                    LuckContextEvent::UpperPrepared { preparation: LuckRushPreparation::Stopped, .. } => {
                        stopped += 1;
                    }
                    LuckContextEvent::UpperPrepared { preparation, .. } => panic!("{preparation:?}"),
                    LuckContextEvent::UpperFinished { .. } => {
                        finished += 1;
                        if stop == "after-all" {
                            cancelled.set(true);
                        }
                    }
                    LuckContextEvent::Scored { .. } => scored += 1,
                    _ => {}
                }
                true
            }),
            || cancelled.get(),
        )
        .unwrap();
        if stop == "exclude" {
            assert!(matches!(result, LuckContextOutcome::UpperOnly));
        } else {
            assert!(matches!(result, LuckContextOutcome::Stopped));
        }
        assert_eq!(
            prepared,
            match stop {
                "inside" => 0,
                "after-all" => uniform::ORDERS,
                _ => 2,
            }
        );
        assert_eq!(stopped, usize::from(stop == "inside"));
        assert_eq!(scored, 0);
        assert_eq!(finished, 1);
        assert_eq!(cache.stats().program_compilations, 0, "preparation performs no factor-history replay");
    }
}

#[test]
fn refused_preparations_cannot_close_a_candidate_or_override_cancellation() {
    let (master, skills, mut input) = input();
    input.rank_confirmations = Some(Vec::new());
    let schedule: Vec<_> = (0..uniform::ORDERS).collect();
    let cancelled = Cell::new(false);
    let (mut declined, mut scored) = (0, 0);
    let result = evaluate_luck_context_bounded(
        &master,
        &skills,
        &input,
        &PayoffMap::Score,
        None,
        (&schedule, true, |event| {
            match event {
                LuckContextEvent::UpperPrepared { preparation, .. } => {
                    assert!(matches!(
                        preparation,
                        LuckRushPreparation::Unavailable { reason: LuckRushDecline::ExternalRanking, .. }
                    ));
                    declined += 1;
                    cancelled.set(true);
                    return false;
                }
                LuckContextEvent::Scored { .. } => scored += 1,
                _ => {}
            }
            true
        }),
        || cancelled.get(),
    )
    .unwrap();
    assert!(matches!(result, LuckContextOutcome::Stopped));
    assert_eq!((declined, scored), (1, 0));
}

#[test]
fn invalid_order_sets_and_nonlinear_prepasses_fail_before_work() {
    let (master, skills, input) = input();
    let all: Vec<_> = (0..uniform::ORDERS).collect();
    let mut duplicate = all.clone();
    duplicate[119] = 0;
    for (schedule, map) in [
        (all[..119].to_vec(), PayoffMap::Score),
        (duplicate, PayoffMap::Score),
        (all.clone(), PayoffMap::ScoreAtLeast { threshold: 100 }),
        (all, PayoffMap::CappedScore { threshold: 100 }),
    ] {
        let mut observed = 0;
        let result = evaluate_luck_context_bounded(
            &master,
            &skills,
            &input,
            &map,
            None,
            (&schedule, true, |_| {
                observed += 1;
                true
            }),
            || false,
        );
        assert!(result.is_err());
        assert_eq!(observed, 0);
    }
}

#[test]
fn scored_order_stops_never_export_or_reuse_a_partial_candidate() {
    let (master, skills, input) = input();
    let reference =
        evaluate_luck_context(&master, &skills, &input, &PayoffMap::Score, None, || false).unwrap().unwrap();
    let schedule: Vec<_> = (0..uniform::ORDERS).rev().collect();
    let labels = uniform::all_orders();
    for prepare_upper in [false, true] {
        for stop_after in [1, uniform::ORDERS - 1, uniform::ORDERS] {
            for cancel in [false, true] {
                let stopped = Cell::new(false);
                let (mut prepared, mut scored, mut finished) = (Vec::new(), Vec::new(), 0);
                let mut cache = LuckDpCache::new(1 << 20);
                let outcome = evaluate_luck_context_bounded(
                    &master,
                    &skills,
                    &input,
                    &PayoffMap::Score,
                    Some(&mut cache),
                    (&schedule, prepare_upper, |event| {
                        match event {
                            LuckContextEvent::UpperPrepared { index, preparation } => {
                                assert!(matches!(preparation, LuckRushPreparation::Ready(_)), "{preparation:?}");
                                prepared.push(index);
                            }
                            LuckContextEvent::UpperFinished { .. } => finished += 1,
                            LuckContextEvent::Scored { index, order } => {
                                assert_eq!(order.order, labels[index]);
                                scored.push(index);
                                if scored.len() == stop_after {
                                    if cancel {
                                        stopped.set(true);
                                    } else {
                                        // Exercise the caller's certified-exclusion response after this
                                        // complete order, including the final order in the schedule.
                                        return false;
                                    }
                                }
                            }
                            LuckContextEvent::UpperAttempt => {}
                        }
                        true
                    }),
                    || stopped.get(),
                )
                .unwrap();
                assert_eq!(scored, schedule[..stop_after]);
                assert_eq!(prepared, if prepare_upper { schedule.clone() } else { Vec::new() });
                assert_eq!(finished, usize::from(prepare_upper));
                // This is the actual boundary consumed by the leaf's complete-candidate cache:
                // only Full carries a CertifiedEvaluation. Even 120 completed order notifications
                // cannot export one after the caller excludes or cancels the evaluation.
                match (cancel, outcome) {
                    (false, LuckContextOutcome::UpperOnly) | (true, LuckContextOutcome::Stopped) => {}
                    (_, LuckContextOutcome::Full(_)) => panic!(
                        "a stopped candidate exported a complete evaluation: prepass={prepare_upper}, orders={stop_after}, cancel={cancel}"
                    ),
                    _ => panic!("cancellation and certified exclusion must retain distinct outcomes"),
                }

                // Complete per-order programs and probability curves may remain reusable. Retain
                // this real cache and resume through the ordinary evaluator; it must still report
                // every canonical order and build the complete aggregate from those witnesses.
                let mut resumed_orders = Vec::new();
                let resumed = evaluate_luck_context_bounded(
                    &master,
                    &skills,
                    &input,
                    &PayoffMap::Score,
                    Some(&mut cache),
                    (&schedule, prepare_upper, |event| {
                        if let LuckContextEvent::Scored { index, order } = event {
                            assert_eq!(order.order, labels[index]);
                            resumed_orders.push(index);
                        }
                        true
                    }),
                    || false,
                )
                .unwrap();
                let LuckContextOutcome::Full(actual) = resumed else {
                    panic!("the uninterrupted continuation must complete all 120 orders")
                };
                assert_eq!(resumed_orders, schedule);
                assert_eq!(actual.orders.len(), uniform::ORDERS);
                assert_eq!(actual.score, reference.score);
                assert_eq!(actual.payoff, reference.payoff);
                assert_eq!(actual.exact_score, reference.exact_score);
                assert_eq!(actual.exact_payoff, reference.exact_payoff);
                for (actual, expected) in actual.orders.iter().zip(&reference.orders) {
                    assert_eq!(
                        (actual.order, actual.mean, actual.support, actual.exact_mean, actual.final_life),
                        (expected.order, expected.mean, expected.support, expected.exact_mean, expected.final_life)
                    );
                }
            }
        }
    }
}
