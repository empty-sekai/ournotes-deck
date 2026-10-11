//! The joint evaluation of many skill orders against the evaluation of each order alone.
use super::*;
use crate::live::full::{JudgedNote, PlayFrame};
use serde_json::{Value, json};

type Formation = (Master, Vec<Performer>, Vec<LiveNote>, Vec<(i32, i32)>, LiveParams, GekisouSetup, LivePlay, Vec<f32>);

/// Five performers whose live skills raise the note score by different amounts at their own chart events, and a
/// Gekisou score-up of the first performer that holds while Rush runs; two LUCK ranges. `effects` adds live skill
/// effect rows `(live skill, effect type, value, condition group)`, `conditions` condition rows `(id, type, value)`
/// with a group of the same id, and `mechanisms` Gekisou LUCK skills `(performer, effect type, value)` of other
/// performers: gauge-ups (11001) that hold while a LUCK range plays, or result guarantees (11005) at a LUCK range
/// start.
fn formation(
    effects: &[(i64, i64, i64, i64)],
    conditions: &[(i64, i64, i64)],
    mechanisms: &[(i64, i64, i64)],
) -> Formation {
    let lots: Vec<_> = (0..5)
        .flat_map(|kind| {
            (0..4)
                .map(move |result| json!({"_id":kind*4+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1}))
        })
        .collect();
    let live = |id: i64, skill: i64, effect: i64, value: i64, condition: i64| {
        json!({"_id":id,"_liveSkillID":skill,"_level":1,"_skillEffectType":effect,"_effectValue":value,
            "_activationTimeSecond":0.3,"_skillConditionGroup":condition})
    };
    let mut live_effects: Vec<Value> = [1500, 2500, 4000, 6000, 9000]
        .iter()
        .zip(11..)
        .map(|(&value, skill)| live(skill, skill, 2000, value, 0))
        .collect();
    live_effects.extend(
        effects
            .iter()
            .zip(100..)
            .map(|(&(skill, effect, value, condition), id)| live(id, skill, effect, value, condition)),
    );
    let mut condition_rows = vec![
        json!({"_id":1,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}),
        json!({"_id":7010,"_conditionType":7010,"_conditionValues":[],"_conditionTargetIDs":[56],"_isPositive":true}),
        json!({"_id":7020,"_conditionType":7020,"_conditionValues":[],"_conditionTargetIDs":[56],"_isPositive":true}),
        json!({"_id":7013,"_conditionType":7013,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}),
    ];
    let mut sets = vec![
        json!({"_id":1,"_group":1,"_conditionIds":[1]}),
        json!({"_id":7010,"_group":7010,"_conditionIds":[7010]}),
        json!({"_id":7020,"_group":7020,"_conditionIds":[7020]}),
        json!({"_id":7013,"_group":7013,"_conditionIds":[7013]}),
    ];
    let gekisou = |id: i64, (trigger_type, trigger, release): (i64, i64, i64), effect: i64, value: i64, limit: i64| {
        json!({"_id":id,"_gekisouSkillID":id,"_level":1,"_skillTriggerType":trigger_type,
            "_skillTriggerConditionGroup":trigger,"_skillConditionGroup":0,"_skillReleaseConditionGroup":release,
            "_skillTargetIDs":[],"_skillEffectType":effect,"_activationTimeSecond":0.0,"_effectValue":value,
            "_maxEffectValue":0,"_effectLimitCount":limit,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0})
    };
    let mut gekisou_skills = vec![json!({"_id":1,"_gekisouMissionType":2})];
    let mut gekisou_effects = vec![gekisou(1, (2, 1, 0), 2000, 8000, 8)];
    for &(member, effect, value) in mechanisms {
        gekisou_skills.push(json!({"_id":20+member,"_gekisouMissionType":2}));
        let (trigger, limit) = if effect == 11005 { ((1, 7010, 7013), 1) } else { ((2, 7020, 7013), 8) };
        gekisou_effects.push(gekisou(20 + member, trigger, effect, value, limit));
    }
    for &(id, kind, value) in conditions {
        condition_rows.push(
            json!({"_id":id,"_conditionType":kind,"_conditionValues":[value],"_conditionTargetIDs":[],"_isPositive":true}),
        );
        sets.push(json!({"_id":id,"_group":id,"_conditionIds":[id]}));
    }
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
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":60}],
        "MasterLiveGekisouLuckBonusLot":lots,
        "MasterSkillTarget":[{"_id":56,"_skillTargetType":5,"_gekisouMissionType":2}],
        "MasterSkillCondition":condition_rows,
        "MasterSkillConditionSet":sets,
        "MasterSkillEffectSetting":[
            {"_id":1,"_skillEffectType":2000,"_phase":1},
            {"_id":2,"_skillEffectType":3002,"_phase":1},
            {"_id":3,"_skillEffectType":12000,"_phase":1}],
        "MasterLiveSkillEffect":live_effects,
        "MasterGekisouSkill":gekisou_skills,
        "MasterGekisouSkillEffect":gekisou_effects
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let mut master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()))
            .unwrap();
    master.gekisou_ranking_score_bonuses = (1..=3)
        .map(|count| crate::master::GekisouRankingBonusRow {
            id: count,
            mission_pattern: gekisou::mission_pattern(2, 2, 2),
            rank: 1,
            count,
            score_bonus_percent: 250,
        })
        .collect();
    let deck: Vec<_> = (0..5)
        .map(|member| Performer {
            live_skill: Some((11 + member, 1)),
            gekisou_skill: if member == 0 {
                Some((1, 1))
            } else {
                mechanisms.iter().any(|&(other, _, _)| other == member).then_some((20 + member, 1))
            },
            ..Default::default()
        })
        .collect();
    let events: Vec<_> =
        (0..5).flat_map(|position| [(position, 120 + 50 * position), (position, 2050 + 40 * position)]).collect();
    let note_times = (100..=400).step_by(25).chain((2000..=2300).step_by(25));
    let notes: Vec<_> = note_times
        .enumerate()
        .map(|(index, time_ms)| LiveNote {
            note_id: index as i32 + 1,
            note_operate_type: 1,
            judgement_type: 1,
            time_ms,
        })
        .collect();
    let fevers = vec![(100, 400), (2000, 2300)];
    let music_length_ms = 3300;
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 1000,
        music_level: 20,
        converted_note_count: notes.len() as i32,
        music_length_ms,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let step = 20;
    let frames: Vec<_> = (0..=music_length_ms / step)
        .map(|i| PlayFrame {
            time_ms: i * step,
            judged: notes
                .iter()
                .filter(|note| note.time_ms > (i - 1) * step && note.time_ms <= i * step)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let delta = vec![step as f32 / 1000.0; frames.len()];
    let setup = GekisouSetup { fevers, missions: vec![2, 2, 2] };
    (master, deck, notes, events, params, setup, LivePlay { frames, base_seed: 0 }, delta)
}

fn permutations() -> Vec<[usize; 5]> {
    let mut orders = Vec::new();
    for a in 0..5 {
        for b in (0..5).filter(|&b| b != a) {
            for c in (0..5).filter(|&c| c != a && c != b) {
                for d in (0..5).filter(|&d| d != a && d != b && d != c) {
                    orders.push([a, b, c, d, 10 - a - b - c - d]);
                }
            }
        }
    }
    orders
}

/// A score without its work counters.
fn comparable(score: &LuckExactScore) -> Value {
    let mut value = serde_json::to_value(score).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("profile");
    object.remove("peakPaths");
    value
}

fn joint(
    formation: &Formation,
    orders: &[[usize; 5]],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: &mut LuckDpCache,
) -> LuckExactOrders {
    let (master, deck, notes, events, params, setup, play, delta) = formation;
    let skills = luck_skills(master).unwrap();
    luck_exact_orders(
        master,
        &skills,
        deck,
        notes,
        events,
        *params,
        setup,
        play,
        delta,
        ranking,
        orders,
        Some(curves),
        true,
    )
    .unwrap()
}

/// Asserts that each joint score is the score of its order evaluated alone.
fn assert_alone(
    name: &str,
    formation: &Formation,
    orders: &[[usize; 5]],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: &mut LuckDpCache,
    joint: &LuckExactOrders,
) {
    let (master, deck, notes, events, params, setup, play, delta) = formation;
    let skills = luck_skills(master).unwrap();
    assert_eq!(joint.scores.len(), orders.len());
    for (order, score) in orders.iter().zip(&joint.scores) {
        let deck: Vec<_> = order.iter().map(|&member| deck[member].clone()).collect();
        let alone = luck_exact_score(
            master,
            &skills,
            &deck,
            notes,
            events,
            *params,
            setup,
            play,
            delta,
            ranking,
            Some(&mut *curves),
            true,
        )
        .unwrap();
        assert_eq!(comparable(score), comparable(&alone), "{name} {order:?}");
    }
}

fn confirmations(formation: &Formation) -> Vec<crate::replay::RankConfirmation> {
    (0..formation.5.fevers.len())
        .map(|range| crate::replay::RankConfirmation { frame: 0, range, rank: 1, percent: 250 })
        .collect()
}

#[test]
fn every_order_from_the_rotations_matches_the_order_evaluated_alone() {
    let cases = [
        ("score-ups", formation(&[], &[], &[])),
        // Safe skill damage keeps the life at 1 or more, so the predicate answers alike in every order.
        ("safe damage, life at least 1", formation(&[(12, 3002, 300, 0), (13, 2000, 5000, 50)], &[(50, 2001, 1)], &[])),
    ];
    let orders = permutations();
    for (name, formation) in &cases {
        let confirmations = confirmations(formation);
        for ranking in [None, Some(confirmations.as_slice())] {
            let mut curves = LuckDpCache::new(usize::MAX);
            let joint = joint(formation, &orders, ranking, &mut curves);
            assert_eq!(joint.alone, None, "{name}");
            assert_eq!((joint.laws, joint.law_recordings), (1, 1), "{name}");
            assert!(joint.replays < orders.len(), "{name}: {} replays", joint.replays);
            let means: std::collections::BTreeSet<_> =
                joint.scores.iter().map(|score| score.final_mean.lower.to_bits()).collect();
            assert!(means.len() > 1, "{name}: the order must change the score");
            assert_alone(name, formation, &orders, ranking, &mut curves, &joint);
        }
    }
}

#[test]
fn orders_outside_the_certificate_are_evaluated_alone() {
    let cases = [
        ("live-skill Gekisou bonus", formation(&[(12, 12000, 500, 0)], &[], &[]), "Gekisou state"),
        (
            "life threshold",
            formation(&[(12, 3002, 300, 0), (13, 2000, 5000, 50)], &[(50, 2001, 1000)], &[]),
            "life predicate",
        ),
    ];
    let orders = &permutations()[..8];
    for (name, formation, reason) in &cases {
        let mut curves = LuckDpCache::new(usize::MAX);
        let joint = joint(formation, orders, None, &mut curves);
        let alone = joint.alone.as_deref().unwrap_or_else(|| panic!("{name}: spliced"));
        assert!(alone.contains(reason), "{name}: {alone}");
        assert_alone(name, formation, orders, None, &mut curves, &joint);
    }
}

#[test]
fn one_law_serves_every_order_when_the_gauge_sums_are_exact() {
    // Two performers start their gauge-ups in the same phase. The gauge sums of 0.0625 and 0.125 are exact in either
    // filing order. Those of 0.05 and 0.025 with their removals are not shown to be, so the law of each order is
    // recorded on its own.
    let cases = [("exact gauge sums", 625, 1250, 1), ("inexact gauge sums", 500, 250, 121)];
    let orders = permutations();
    for (name, first, second, recordings) in cases {
        let formation = formation(&[], &[], &[(1, 11001, first), (2, 11001, second)]);
        let mut curves = LuckDpCache::new(usize::MAX);
        let joint = joint(&formation, &orders, None, &mut curves);
        assert_eq!(joint.alone, None, "{name}");
        assert_eq!(joint.law_recordings, recordings, "{name}");
        assert_alone(name, &formation, &orders, None, &mut curves, &joint);
    }
}

#[test]
fn one_law_serves_every_order_when_each_phase_has_the_same_actions() {
    // Two performers guarantee a LUCK range's first result in the same phase. With the same guarantee the
    // range-start actions are the same in every order; with different ones their order follows the performers.
    let cases = [("the same guarantees", 3, 3, 1), ("different guarantees", 3, 4, 121)];
    let orders = permutations();
    for (name, first, second, recordings) in cases {
        let formation = formation(&[], &[], &[(1, 11005, first), (2, 11005, second)]);
        let mut curves = LuckDpCache::new(usize::MAX);
        let joint = joint(&formation, &orders, None, &mut curves);
        assert_eq!(joint.alone, None, "{name}");
        assert_eq!(joint.law_recordings, recordings, "{name}");
        assert_alone(name, &formation, &orders, None, &mut curves, &joint);
    }
}

#[test]
fn orders_are_permutations_of_five_performers() {
    let formation = formation(&[], &[], &[]);
    let (master, deck, notes, events, params, setup, play, delta) = &formation;
    let skills = luck_skills(master).unwrap();
    let run = |deck: &[Performer], order: [usize; 5]| {
        luck_exact_orders(
            master,
            &skills,
            deck,
            notes,
            events,
            *params,
            setup,
            play,
            delta,
            None,
            &[order],
            None,
            false,
        )
    };
    assert!(matches!(run(deck, [0, 1, 2, 3, 3]), Err(Error::Input(_))));
    assert!(matches!(run(deck, [0, 1, 2, 3, 5]), Err(Error::Input(_))));
    assert!(matches!(run(&deck[..4], [0, 1, 2, 3, 4]), Err(Error::Input(_))));
}
