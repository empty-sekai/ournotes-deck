use super::*;
use crate::search::expectation::{self, FiniteSeedContext, PhysicalDeck};
use crate::search::gate_tests::common::{Rng, roster, set_column, short_chart, synth};
use crate::search::interval_topk::{CandidateInterval, CanonicalTie, IntervalTopK, RemainingDomain};
use crate::search::{Objective, PlayInput};
use crate::types::Metric;
use ournotes_sim::live::full::{
    GekisouSetup, JudgedNote, LiveModel, LiveNote, LiveParams, LivePlay, LuckExactBudget, Performer, PlayFrame,
    luck_skills,
};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::{master::Master, pool::Pool};
use serde_json::json;

fn fixture() -> (Master, FiniteSeedContext) {
    let mut seeded = synth(&mut Rng::new(8511), 5, 0);
    set_column(&mut seeded, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    let seed_master = seeded.master();
    let owned = roster(&mut Rng::new(8512), &seed_master);
    let pool = Pool::new(&seed_master, &owned).unwrap();
    let (chart, judgement_types) = short_chart(&mut Rng::new(8513), 1, false);
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    };
    let mut input =
        expectation::context(&pool, &PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] }, &objective).unwrap();
    let lots: Vec<_> = (0..5)
        .flat_map(|kind| {
            [0,3].into_iter().enumerate().map(move |(index,result)| {
        json!({"_id":kind*2+index+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1})
    })
        })
        .collect();
    let tables = json!({
        "MasterLiveSettings":[
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":140}],
        "MasterLiveGekisouLuckBonusLot":lots,
        "MasterLiveSkillEffect":[{"_id":1,"_liveSkillID":1,"_level":1,"_skillEffectType":3000,"_effectValue":100}]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
            .unwrap();
    input.performers = std::array::from_fn(|_| Performer { live_skill: Some((1, 1)), ..Default::default() });
    input.notes = vec![LiveNote { note_id: 1, time_ms: 300, note_operate_type: 1, judgement_type: 1 }];
    input.events.clear();
    input.params = LiveParams {
        skill_target_music_type: 0,
        total_power: 1000,
        music_level: 20,
        converted_note_count: 1,
        music_length_ms: 2000,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    input.gekisou = Some(GekisouSetup { fevers: vec![(100, 500)], missions: vec![2, 2, 2] });
    input.play = LivePlay {
        base_seed: 0,
        frames: (0..=20)
            .map(|i| PlayFrame {
                time_ms: i * 100,
                judged: if i == 3 {
                    vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 300 }]
                } else {
                    Vec::new()
                },
            })
            .collect(),
    };
    input.delta_times = vec![0.1; input.play.frames.len()];
    (master, input)
}

fn endpoints(master: &Master, input: &FiniteSeedContext) -> [(i32, i32); 2] {
    [0, 3].map(|result| {
        let mut deterministic = master.clone();
        deterministic.gekisou_luck_bonus_lots.retain(|row| row.lot_result == result);
        deterministic.reindex().unwrap();
        let mut model = LiveModel::new_gekisou(
            &deterministic,
            &input.performers,
            &input.notes,
            &input.events,
            input.params,
            input.gekisou.as_ref().unwrap(),
        )
        .unwrap();
        let score = model.run_timed(&input.play, &input.delta_times).unwrap();
        (score, model.current_life())
    })
}

#[test]
fn declined_score_certificate_uses_complete_primitive_laws() {
    let (master, input) = fixture();
    let outcomes = endpoints(&master, &input);
    assert_ne!(outcomes[0].0, outcomes[1].0);
    let threshold = (outcomes[0].0 + outcomes[1].0) / 2;
    let skills = luck_skills(&master).unwrap();
    for map in [
        PayoffMap::Score,
        PayoffMap::ScoreAtLeast { threshold },
        PayoffMap::CappedScore { threshold },
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: 1000 },
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: 1001 },
    ] {
        let result = evaluate_luck_context(&master, &skills, &input, &map, None, || false).unwrap().unwrap();
        let expected = outcomes
            .iter()
            .map(|&(score, life)| match &map {
                PayoffMap::Score => i128::from(score),
                PayoffMap::ScoreAtLeast { threshold } => i128::from(score >= *threshold),
                PayoffMap::CappedScore { threshold } => i128::from(score.min(*threshold)),
                PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life } => {
                    i128::from(score >= *threshold && life >= *min_final_life)
                }
                _ => unreachable!(),
            })
            .sum::<i128>();
        assert_eq!(
            compare_exact(result.exact_payoff.unwrap(), ExactExpectation { numerator: expected, denominator: 2 })
                .unwrap(),
            std::cmp::Ordering::Equal
        );
        assert!(result.orders.iter().all(|order| order.exact_mean.is_some() && order.final_life.is_some()));
        assert!(!result.needs_native_payoff_support(&Metric::ClientEventPoints { event_id: 1 }));
    }
}

#[test]
fn partial_work_retains_unproved_full_score_support() {
    let (master, input) = fixture();
    let skills = luck_skills(&master).unwrap();
    let mut work = LuckExactBudget { remaining_runs: 0, remaining_frames: 0 };
    let result =
        evaluate_luck_context_with_budget(&master, &skills, &input, &PayoffMap::Score, None, &mut work, || false)
            .unwrap()
            .unwrap();
    assert_eq!(result.orders.len(), 120);
    assert!(result.exact_score.is_none() && result.exact_payoff.is_none());
    assert!(result.orders.iter().all(|order| order.support == (i32::MIN, i32::MAX)
        && order.exact_mean.is_none()
        && order.final_life.is_none()));
    let mut frontier = IntervalTopK::new(1).unwrap();
    for id in 0..2 {
        frontier
            .insert(CandidateInterval {
                id,
                tie: CanonicalTie { power: 100, key: vec![id as i64] },
                score: result.score,
                payoff: result.payoff,
                exact_score: None,
                exact_payoff: None,
                equality: None,
                revision: 0,
            })
            .unwrap();
    }
    assert!(!frontier.proof(RemainingDomain::Exhausted).unwrap().complete);
    for metric in [
        Metric::Score,
        Metric::ScoreAtLeast { threshold: 1 },
        Metric::CappedScore { threshold: 1 },
        Metric::ScoreAndLifeAtLeast { threshold: 1, min_final_life: 1 },
    ] {
        assert!(!result.needs_native_payoff_support(&metric));
    }
    for metric in [
        Metric::Power,
        Metric::ClientEventPoints { event_id: 1 },
        Metric::ClientChallengePoints { event_id: 1 },
        Metric::ConditionalClientEventItems { event_id: 1, resource_type: 1, resource_id: 1 },
    ] {
        assert!(result.needs_native_payoff_support(&metric));
    }
}

#[test]
fn fallback_orders_share_one_finite_work_allowance() {
    let (mut master, input) = fixture();
    master.gekisou_luck_bonus_lots.retain(|row| row.lot_result == 3);
    master.reindex().unwrap();
    let mut work = LuckExactBudget { remaining_runs: 1, remaining_frames: input.play.frames.len() as u64 };
    let result = evaluate_luck_context_with_budget(
        &master,
        &luck_skills(&master).unwrap(),
        &input,
        &PayoffMap::Score,
        None,
        &mut work,
        || false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.orders.iter().filter(|order| order.exact_mean.is_some()).count(), 1);
    assert_eq!(work.remaining_frames, 0);
    assert!(result.exact_score.is_none() && result.exact_payoff.is_none());
}

#[test]
fn fallback_preserves_cancellation_and_input_errors() {
    let (master, mut input) = fixture();
    let skills = luck_skills(&master).unwrap();
    assert!(evaluate_luck_context(&master, &skills, &input, &PayoffMap::Score, None, || true).unwrap().is_none());
    let mut work = LuckExactBudget::default();
    let initial_frames = work.remaining_frames;
    let mut checks = 0;
    let cancelled =
        evaluate_luck_context_with_budget(&master, &skills, &input, &PayoffMap::Score, None, &mut work, || {
            checks += 1;
            checks > 10
        })
        .unwrap();
    assert!(cancelled.is_none());
    assert!(work.remaining_frames < initial_frames);
    input.play.frames[3].judged[0].note_id = 999;
    assert!(matches!(
        evaluate_luck_context(&master, &skills, &input, &PayoffMap::Score, None, || false),
        Err(Error::Input(_))
    ));
    input.gekisou = None;
    assert!(matches!(
        evaluate_luck_context(&master, &skills, &input, &PayoffMap::Score, None, || false),
        Err(Error::Domain(_))
    ));
}

fn probe_boundary_context(value: i64, early: bool) -> (Master, FiniteSeedContext) {
    let (mut master, mut input) = fixture();
    master.live_skill_effects.clear();
    master.gekisou_luck_bonus_lots.retain(|row| row.lot_result == 3);
    master.live_settings.iter_mut().find(|row| row.key == "gekisou_luck_rush_score_bonus_percent").unwrap().value =
        "0".into();
    master.skill_conditions.push(
        serde_json::from_value(json!({
            "_id":1,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true
        }))
        .unwrap(),
    );
    master.skill_condition_sets.push(
        serde_json::from_value(json!({
            "_id":1,"_group":1,"_conditionIds":[1]
        }))
        .unwrap(),
    );
    master.skill_effect_settings.push(
        serde_json::from_value(json!({
            "_id":1,"_skillEffectType":2000,"_phase":1
        }))
        .unwrap(),
    );
    master.gekisou_skills.push(serde_json::from_value(json!({"_id":1,"_gekisouMissionType":2})).unwrap());
    master.gekisou_skill_effects.push(
        serde_json::from_value(json!({
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":2,
            "_skillTriggerConditionGroup":1,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.0,"_effectValue":value,
            "_maxEffectValue":0,"_effectLimitCount":1,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
        }))
        .unwrap(),
    );
    master.reindex().unwrap();
    input.performers = std::array::from_fn(|_| Performer::default());
    input.performers[0].gekisou_skill = Some((1, 1));
    input.notes = vec![LiveNote { note_id: 1, time_ms: 1050, note_operate_type: 1, judgement_type: 1 }];
    if early {
        input.notes.insert(0, LiveNote { note_id: 0, time_ms: 500, note_operate_type: 1, judgement_type: 1 });
    }
    input.params.music_length_ms = 1000;
    input.params.converted_note_count = input.notes.len() as i32;
    input.gekisou = Some(GekisouSetup { fevers: vec![(100, 1300)], missions: vec![2, 2, 2] });
    input.play.frames = (0..=22)
        .map(|frame| PlayFrame {
            time_ms: frame * 100,
            judged: input
                .notes
                .iter()
                .filter(|note| frame * 100 - 100 < note.time_ms && note.time_ms <= frame * 100)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    input.delta_times = vec![0.1; input.play.frames.len()];
    (master, input)
}

#[test]
fn declined_probe_boundary_uses_native_order_means_and_attained_support() {
    use ournotes_sim::live::full::{LuckExactSession, LuckScoreSession};
    for early in [false, true] {
        for value in [-8000, 8000] {
            let (master, input) = probe_boundary_context(value, early);
            let skills = luck_skills(&master).unwrap();
            let setup = input.gekisou.as_ref().unwrap();
            let mut bounds = LuckScoreSession::new(
                &master,
                &skills,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                None,
            );
            assert!(matches!(bounds.summary(&input.performers, None, || false), Err(Error::Unsupported(_))));
            let mut native = Vec::new();
            let mut support = LuckExactSession::new(
                &master,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                None,
                0,
            )
            .unwrap();
            let mut work = LuckExactBudget::default();
            for order in uniform::all_orders() {
                let performers = order.map(|slot| input.performers[slot].clone());
                let mut model =
                    LiveModel::new_gekisou(&master, &performers, &input.notes, &input.events, input.params, setup)
                        .unwrap();
                let score = model.run_timed(&input.play, &input.delta_times).unwrap();
                let life = model.current_life();
                let reachable = support.support(&performers, &mut work, None, || false).unwrap();
                assert_eq!(reachable.decline, None);
                assert!(!reachable.attained_ceiling);
                assert_eq!(reachable.outcomes, [(score, life)]);
                assert_eq!(
                    score,
                    if early {
                        3224
                    } else if value < 0 {
                        5805
                    } else {
                        644
                    }
                );
                native.push((order, score, life));
            }
            assert_eq!(native.len(), 120);
            let threshold = native[0].1;
            for map in [
                PayoffMap::Score,
                PayoffMap::ScoreAtLeast { threshold },
                PayoffMap::ScoreAtLeast { threshold: threshold + 1 },
                PayoffMap::CappedScore { threshold: threshold - 1 },
                PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: 1000 },
                PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: 1001 },
            ] {
                let result = evaluate_luck_context(&master, &skills, &input, &map, None, || false).unwrap().unwrap();
                let score_sum = native.iter().map(|(_, score, _)| i128::from(*score)).sum();
                let payoff_sum = native
                    .iter()
                    .map(|&(_, score, life)| match &map {
                        PayoffMap::Score => i128::from(score),
                        PayoffMap::ScoreAtLeast { threshold } => i128::from(score >= *threshold),
                        PayoffMap::CappedScore { threshold } => i128::from(score.min(*threshold)),
                        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life } => {
                            i128::from(score >= *threshold && life >= *min_final_life)
                        }
                        _ => unreachable!(),
                    })
                    .sum();
                assert_eq!(
                    compare_exact(
                        result.exact_score.unwrap(),
                        ExactExpectation { numerator: score_sum, denominator: 120 }
                    )
                    .unwrap(),
                    std::cmp::Ordering::Equal
                );
                assert_eq!(
                    compare_exact(
                        result.exact_payoff.unwrap(),
                        ExactExpectation { numerator: payoff_sum, denominator: 120 }
                    )
                    .unwrap(),
                    std::cmp::Ordering::Equal
                );
                for order in &result.orders {
                    let &(_, score, life) = native.iter().find(|row| row.0 == order.order).unwrap();
                    assert_eq!(order.support, (score, score));
                    assert_eq!(order.final_life, Some((life, life)));
                    assert_eq!(
                        compare_exact(order.exact_mean.unwrap(), fraction(i128::from(score))).unwrap(),
                        std::cmp::Ordering::Equal
                    );
                }
                let maximum = native.iter().map(|row| row.1).max().unwrap();
                assert_eq!(result.orders.iter().map(|order| order.support.1).max(), Some(maximum));
            }
        }
    }
}
