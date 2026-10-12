//! The joint evaluation of many skill orders against the evaluation of each order alone.
use super::*;
use crate::live::full::{JudgedNote, PlayFrame};
use serde_json::{Value, json};

type Formation = (Master, Vec<Performer>, Vec<LiveNote>, Vec<(i32, i32)>, LiveParams, GekisouSetup, LivePlay, Vec<f32>);

/// Five performers whose live skills raise the note score by different amounts at their own chart events, and a
/// Gekisou score-up of the first performer that holds while Rush runs; two LUCK ranges. `effects` adds live skill
/// effect rows `(live skill, effect type, value, condition group)` (a judgement conversion converts Perfect),
/// `conditions` condition rows `(id, type, value)`
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
        let targets: &[i64] = if effect == 12006 { &[57] } else { &[] };
        json!({"_id":id,"_liveSkillID":skill,"_level":1,"_skillEffectType":effect,"_effectValue":value,
            "_activationTimeSecond":0.3,"_skillConditionGroup":condition,"_skillTargetIDs":targets})
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
        "MasterSkillTarget":[{"_id":56,"_skillTargetType":5,"_gekisouMissionType":2},{"_id":57,"_skillTargetType":2,"_judgement":5}],
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

/// A score without its work counters and its life bounds.
fn comparable(score: &LuckExactScore) -> Value {
    let mut value = serde_json::to_value(score).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("profile");
    object.remove("peakPaths");
    object.remove("finalLife");
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

/// Asserts that each joint score is the score of its order evaluated alone, whose one terminal life lies within
/// the joint life bounds.
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
        assert_eq!(alone.final_life.lower, alone.final_life.upper, "{name} {order:?}");
        assert!(
            score.final_life.lower <= alone.final_life.lower && alone.final_life.upper <= score.final_life.upper,
            "{name} {order:?}: {:?} outside {:?}",
            alone.final_life,
            score.final_life
        );
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
            // Without a life skill every order ends at the initial life.
            let pinned = joint.scores.iter().all(|score| score.final_life.lower == score.final_life.upper);
            assert_eq!(pinned, *name == "score-ups", "{name}");
        }
    }
}

#[test]
fn given_laws_replace_the_law_recordings() {
    let formation = formation(&[], &[], &[]);
    let (master, deck, notes, events, params, setup, play, delta) = &formation;
    let skills = luck_skills(master).unwrap();
    let orders = permutations();
    let mut curves = LuckDpCache::new(usize::MAX);
    let recorded = joint(&formation, &orders, None, &mut curves);
    let live = lottery_live(&formation, &skills, *params, play);
    let laws = luck_order_laws(&live, deck, &orders, &mut curves, None).unwrap();
    let mut given = |laws: &[std::sync::Arc<LuckDpCertifiedResult>]| {
        luck_exact_orders_with_laws(
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
            &orders,
            laws,
            Some(&mut curves),
            true,
        )
    };
    let scores = given(&laws).unwrap();
    assert_eq!((scores.alone.as_deref(), scores.laws, scores.law_recordings), (None, recorded.laws, 0));
    for ((order, score), recorded) in orders.iter().zip(&scores.scores).zip(&recorded.scores) {
        assert_eq!(comparable(score), comparable(recorded), "{order:?}");
    }
    assert!(given(&laws[1..]).is_err());
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
fn a_law_serves_the_orders_that_keep_the_relative_order_of_inexact_gauge_filings() {
    // Two performers start their gauge-ups in the same phase. The gauge sums of 0.0625 and 0.125 are exact in either
    // filing order, so one law serves every order. Those of 0.05 and 0.025 with their removals are not shown to be,
    // so the orders that place the first performer before the second share a law, and the others another.
    let cases = [("exact gauge sums", 625, 1250, 1), ("inexact gauge sums", 500, 250, 2)];
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
fn a_law_serves_the_orders_that_keep_the_relative_order_of_different_actions() {
    // Performers guarantee a LUCK range's first result in the same phase. With the same guarantee the range-start
    // actions are the same in every order; with different ones their list follows the relative order of the
    // performers that hold them, one class of orders for each.
    let cases = [
        ("the same guarantees", vec![(1, 11005, 3), (2, 11005, 3)], 1),
        ("two different guarantees", vec![(1, 11005, 3), (2, 11005, 4)], 2),
        ("three different guarantees", vec![(1, 11005, 2), (2, 11005, 3), (3, 11005, 4)], 6),
    ];
    let orders = permutations();
    for (name, mechanisms, recordings) in cases {
        let formation = formation(&[], &[], &mechanisms);
        let mut curves = LuckDpCache::new(usize::MAX);
        let joint = joint(&formation, &orders, None, &mut curves);
        assert_eq!(joint.alone, None, "{name}");
        assert_eq!(joint.law_recordings, recordings, "{name}");
        assert_alone(name, &formation, &orders, None, &mut curves, &joint);
    }
}

/// The live of `formation` with the parameters `params` and the play `play`.
fn lottery_live<'a>(
    formation: &'a Formation,
    skills: &'a LuckSkills,
    params: LiveParams,
    play: &'a LivePlay,
) -> crate::live::full::LotteryLive<'a> {
    let (master, _, notes, events, _, setup, _, delta) = formation;
    crate::live::full::LotteryLive {
        master,
        skills,
        notes,
        events,
        params,
        setup,
        play,
        delta_times: delta,
        ranking: None,
    }
}

/// Asserts that two laws of the same formation have the same possible transitions and overlapping masses.
fn assert_laws_overlap(a: &LuckDpCertifiedResult, b: &LuckDpCertifiedResult) {
    assert_eq!(
        (&a.probe_transitions, &a.rush_transitions, &a.probes),
        (&b.probe_transitions, &b.rush_transitions, &b.probes)
    );
    assert_eq!(a.frame_queries.len(), b.frame_queries.len());
    for ((time, x), (other, y)) in a.frame_queries.iter().zip(b.frame_queries.iter()) {
        assert_eq!(time, other);
        for (x, y) in x.iter().flatten().zip(y.iter().flatten()) {
            let (x, y) = (x.interval(), y.interval());
            assert!(x.lower() <= y.upper() && y.lower() <= x.upper(), "{time}: {x:?} {y:?}");
        }
    }
}

#[test]
fn lottery_kinds_compose_one_law_for_every_order_of_their_performers() {
    // Two performers guarantee a LUCK range's first result with different guarantees. Their recordings form two
    // classes of orders, whose laws the one composed law overlaps, and the scores under either agree. Live skills and
    // the total power leave the kinds alone, and contributions here do not depend on the position, so a formation that
    // changes them or moves a guarantee to another performer takes the composed law of the first. A live skill that may
    // convert a judgement of the play takes no kind, and another play drops the kinds.
    let first = formation(&[(16, 12006, 4, 0)], &[], &[(1, 11005, 3), (2, 11005, 4)]);
    let (master, deck, notes, events, params, setup, play, delta) = &first;
    let skills = luck_skills(master).unwrap();
    let orders = permutations();
    let mut kinds = LotteryKinds::new(master, usize::MAX);
    let mut curves = LuckDpCache::new(usize::MAX);
    let mut laws = |deck: &[Performer], params: LiveParams, play: &LivePlay, kinds: Option<&mut LotteryKinds<'_>>| {
        let live = lottery_live(&first, &skills, params, play);
        luck_order_laws(&live, deck, &orders, &mut curves, kinds)
    };
    let same = |laws: &[std::sync::Arc<LuckDpCertifiedResult>], law: &std::sync::Arc<LuckDpCertifiedResult>| {
        laws.iter().all(|other| std::sync::Arc::ptr_eq(other, law))
    };
    let composed = laws(deck, *params, play, Some(&mut kinds)).unwrap();
    assert!(same(&composed, &composed[0]));
    let native = laws(deck, *params, play, None).unwrap();
    let mut distinct = Vec::<&std::sync::Arc<LuckDpCertifiedResult>>::new();
    for law in &native {
        if !distinct.iter().any(|other| std::sync::Arc::ptr_eq(other, law)) {
            distinct.push(law);
        }
    }
    assert_eq!(distinct.len(), 2);
    distinct.iter().for_each(|law| assert_laws_overlap(&composed[0], law));
    let scores = |laws: &[std::sync::Arc<LuckDpCertifiedResult>]| {
        luck_exact_orders_with_laws(
            master, &skills, deck, notes, events, *params, setup, play, delta, None, &orders, laws, None, false,
        )
        .unwrap()
        .scores
    };
    for (a, b) in scores(&composed).iter().zip(&scores(&native)) {
        assert!(a.final_mean.lower <= b.final_mean.upper && b.final_mean.lower <= a.final_mean.upper);
    }
    let stats = kinds.stats();
    assert_eq!((stats.teams, stats.composed, stats.law_lookups, stats.law_hits), (1, 1, 1, 0));

    let mut other = deck.clone();
    other.swap(0, 4);
    (other[0].gekisou_skill, other[4].gekisou_skill) = (other[4].gekisou_skill, other[0].gekisou_skill);
    other[3].live_skill = None;
    let other_params = LiveParams { total_power: 5000, ..*params };
    assert!(same(&laws(&other, other_params, play, Some(&mut kinds)).unwrap(), &composed[0]));
    let mut moved = deck.clone();
    (moved[2].gekisou_skill, moved[3].gekisou_skill) = (None, moved[2].gekisou_skill);
    assert!(same(&laws(&moved, *params, play, Some(&mut kinds)).unwrap(), &composed[0]));
    let stats = kinds.stats();
    assert_eq!((stats.teams, stats.composed, stats.law_lookups, stats.law_hits, stats.lives), (3, 1, 3, 2, 1));

    let mut converting = deck.clone();
    converting[4].live_skill = Some((16, 1));
    // Its recording reads the converted judgement, which the play gives no damage.
    let fallback = laws(&converting, *params, play, Some(&mut kinds)).unwrap_err();
    assert_eq!(fallback.to_string(), laws(&converting, *params, play, None).unwrap_err().to_string());
    assert_eq!(kinds.stats().unkinded, 1);

    let other_play = LivePlay { base_seed: 1, ..play.clone() };
    let replayed = laws(deck, *params, &other_play, Some(&mut kinds)).unwrap();
    assert_eq!(format!("{:?}", replayed[0]), format!("{:?}", composed[0]));
    assert_eq!((kinds.stats().lives, kinds.stats().composed), (2, 2));
}

#[test]
fn a_model_whose_members_move_has_the_shape_of_each_order() {
    // Two performers hold score probes, so the probe rows of an order follow both of their positions.
    let mut formation = formation(&[], &[], &[(1, 11001, 625)]);
    formation.1[3].gekisou_skill = Some((1, 1));
    let (master, deck, notes, events, params, setup, _, _) = &formation;
    let skills = luck_skills(master).unwrap();
    let model = |deck: &[Performer]| super::exact::exact_model(master, deck, notes, events, *params, setup, None);
    let shape = |model: &mut LiveModel| {
        let (probes, phase) = super::exact_orders::order_shape(model, &skills).unwrap();
        (probes.iter().map(|row| (row.owner, row.value.to_bits())).collect::<Vec<_>>(), phase)
    };
    let mut moved = model(deck).unwrap();
    let mut assign: Vec<usize> = (0..5).collect();
    let mut rows = std::collections::BTreeSet::new();
    for order in permutations() {
        moved.move_to(&mut assign, &order).unwrap();
        let ordered: Vec<_> = order.iter().map(|&member| deck[member].clone()).collect();
        let built = shape(&mut model(&ordered).unwrap());
        assert_eq!(shape(&mut moved), built, "{order:?}");
        assert_eq!(built.0.len(), 2, "{order:?}");
        rows.insert(built.0);
    }
    assert!(rows.len() > 1, "the probe rows must follow the positions");
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
