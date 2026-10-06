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
    let mut source = synth_snaps(&mut Rng::new(141), 5, 0, &[]);
    set_column(&mut source, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    set_column(&mut source, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(2);
        row["_gekisouMission2"] = json!(3);
        row["_gekisouMission3"] = json!(1);
    });
    replace_table(&mut source, "MasterLiveSkillEffect", json!([]));
    extend_table(
        &mut source,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"40"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"20"}),
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
    let owned = roster(&mut Rng::new(142), &master);
    let pool = Pool::new(&master, &owned).unwrap();
    let chart = Chart::from_notes(
        vec![ChartNote { id: 1, time_ms: 100, note_type: 1 }],
        vec![],
        &LiveScoreSettings::from_master(&master).unwrap(),
    )
    .unwrap();
    let stream = JudgementStream::theoretical_best(&chart);
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
        play: PlayInput::Stream { stream, judgement_types: vec![1] },
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
