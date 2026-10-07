//! Full-score identities are checked against original native constructors and complete nominal branches.
use super::*;
use crate::live::full::character_blind_uniform_score_identity;

fn input() -> RushCase {
    let mut input = four_bucket_case(800, 2, false);
    input.master.live_skills.push(crate::master::SkillRow { id: 903, ..Default::default() });
    input.master.gekisou_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 9900,
        skill_id: 901,
        level: 1,
        skill_trigger_type: SUSTAINED,
        skill_effect_type: 0,
        ..Default::default()
    });
    input.master.reindex().unwrap();
    input.deck.resize_with(5, Performer::default);
    for (slot, performer) in input.deck.iter_mut().enumerate() {
        performer.character_id = slot as i64 + 7;
    }
    input
}

fn key(input: &RushCase) -> Option<Vec<u8>> {
    character_blind_uniform_score_identity(&input.master, input.deck.as_slice().try_into().unwrap())
}

fn orders() -> Vec<[usize; 5]> {
    fn visit(prefix: &mut Vec<usize>, out: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            out.push(prefix.as_slice().try_into().unwrap());
            return;
        }
        for slot in 0..5 {
            if !prefix.contains(&slot) {
                prefix.push(slot);
                visit(prefix, out);
                prefix.pop();
            }
        }
    }
    let mut out = Vec::new();
    visit(&mut Vec::new(), &mut out);
    assert_eq!(out.len(), 120);
    out
}

/// This oracle observes complete native outcomes directly. In particular it does not classify the
/// command log as one Rush handle: the short chart can retain overlapping handles from adjacent ranges.
fn nominal_scores(input: &RushCase) -> Vec<(Fraction, i32, i32)> {
    let mut pending = vec![(Vec::new(), Fraction::ONE)];
    let mut outcomes = Vec::new();
    let mut visited = 0;
    while let Some((prefix, mass)) = pending.pop() {
        visited += 1;
        assert!(visited <= 4096 && prefix.len() <= 16, "the complete nominal fixture must remain bounded");
        let mut model = input.native();
        let result = model.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix.clone()));
        assert!(model.random.nominal_covers_draws());
        if let Some(branches) = model.random.nominal_branch() {
            assert!(result.is_err());
            let total = branches[0].total;
            assert_eq!(branches.iter().map(|branch| branch.weight).sum::<u64>(), total);
            for (index, branch) in branches.iter().enumerate() {
                assert_eq!(branch.total, total);
                let mut next = prefix.clone();
                next.push(index);
                pending.push((next, mass.times(u128::from(branch.weight), u128::from(total))));
            }
        } else {
            let score = result.unwrap();
            assert!(model.random.nominal_prefix_consumed());
            assert!(model.gk.as_ref().unwrap().ctrl.states.iter().all(|state| state.state == gekisou::S_FINISH));
            outcomes.push((mass, score, model.current_life()));
        }
    }
    assert_eq!(outcomes.iter().fold(Fraction::ZERO, |sum, outcome| sum.plus(outcome.0)), Fraction::ONE);
    outcomes
}

#[test]
fn score_cap_character_identity_preserves_all_native_order_labels_and_nominal_laws() {
    let original = input();
    let mut renamed = original.clone();
    for performer in &mut renamed.deck {
        performer.character_id += 1000;
    }
    assert_eq!(key(&original), key(&renamed));
    assert!(key(&original).unwrap().starts_with(b"uniform120/full-score-character-unread/v1/"));
    for order in orders() {
        let mut a = original.clone();
        let mut b = renamed.clone();
        a.deck = order.map(|slot| original.deck[slot].clone()).to_vec();
        b.deck = order.map(|slot| renamed.deck[slot].clone()).to_vec();
        assert_eq!(key(&a), key(&original));
        let mut native_a = a.native();
        let mut native_b = b.native();
        assert_eq!(
            crate::live::full::luck_exact::initialized_identity(&mut native_a).expect("small complete model"),
            crate::live::full::luck_exact::initialized_identity(&mut native_b).expect("small complete model"),
            "complete native initialization must agree at original label {order:?}"
        );
        let score_a = native_a.run_with_random(&a.play, &a.delta, LiveRandom::new(17)).unwrap();
        let score_b = native_b.run_with_random(&b.play, &b.delta, LiveRandom::new(17)).unwrap();
        assert_eq!((score_a, native_a.current_life()), (score_b, native_b.current_life()));
    }
    // Independently enumerate every nominal path at two physical positions and two exact powers.
    for (position, power) in [(0, 1001), (3, 12347)] {
        let mut a = original.clone();
        let mut b = renamed.clone();
        a.deck.swap(0, position);
        b.deck.swap(0, position);
        a.params.total_power = power;
        b.params.total_power = power;
        let a = nominal_scores(&a);
        let b = nominal_scores(&b);
        assert!(a.len() > 1);
        assert_eq!(a, b);
    }
}

#[test]
fn score_cap_character_identity_keeps_other_performer_fields_and_ordered_sources() {
    let original = input();
    let expected = key(&original).unwrap();
    for change in 0..9 {
        let mut changed = original.clone();
        let performer = &mut changed.deck[0];
        match change {
            0 => performer.band_id += 1,
            1 => performer.card_type += 1,
            2 => performer.tag_ids.push(7),
            3 => performer.live_skill_categories.push(8),
            4 => performer.gekisou_skill_categories.push(9),
            5 => performer.gekisou_mission_type += 1,
            6 => performer.live_skill = Some((903, 2)),
            7 => performer.gekisou_support_skills.clear(),
            8 => performer.gekisou_skill = None,
            _ => unreachable!(),
        }
        assert_ne!(key(&changed).as_deref(), Some(expected.as_slice()), "mutation {change}");
    }
    let mut changed = original.clone();
    let mut source = changed.master.gekisou_support_skills.iter().find(|row| row.id == 902).unwrap().clone();
    source.id = 9902;
    changed.master.gekisou_support_skills.push(source);
    let mut row = changed.master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 902).unwrap().clone();
    row.id = 9902;
    row.skill_id = 9902;
    changed.master.gekisou_support_skill_effects.push(row);
    changed.master.reindex().unwrap();
    changed.deck[0].gekisou_support_skills.push((9902, 1));
    let ordered = key(&changed).unwrap();
    changed.deck[0].gekisou_support_skills.reverse();
    assert_ne!(key(&changed).unwrap(), ordered, "source vectors are not a commutative set");
}

#[test]
fn score_cap_character_readers_and_unknown_source_closures_decline() {
    let original = input();
    for change in 0..10 {
        let mut changed = original.clone();
        changed.master.skill_targets.push(crate::master::SkillTargetRow {
            id: 9901,
            character_id: 7,
            ..Default::default()
        });
        changed.master.skill_conditions.push(crate::master::SkillConditionRow {
            id: 9901,
            condition_type: 5000,
            condition_values: Vec::new(),
            condition_target_ids: vec![9901],
            is_positive: true,
        });
        changed.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id: 9901,
            group: 9901,
            condition_ids: vec![9901],
        });
        match change {
            0 => {
                changed
                    .master
                    .live_skill_effects
                    .iter_mut()
                    .find(|row| row.live_skill_id == 903)
                    .unwrap()
                    .skill_condition_group = 9901
            }
            1 => {
                changed
                    .master
                    .gekisou_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 901)
                    .unwrap()
                    .skill_release_condition_group = 9901
            }
            2 => {
                changed
                    .master
                    .gekisou_support_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 902)
                    .unwrap()
                    .effect_execute_limit_reset_condition_group = 9901
            }
            3 => {
                changed
                    .master
                    .gekisou_support_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 902)
                    .unwrap()
                    .skill_effect_type = 99999
            }
            4 => {
                changed
                    .master
                    .gekisou_support_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 902)
                    .unwrap()
                    .skill_trigger_condition_group = 99999
            }
            5 => {
                changed
                    .master
                    .gekisou_support_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 902)
                    .unwrap()
                    .skill_target_ids = vec![99999]
            }
            6 => {
                changed
                    .master
                    .gekisou_support_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 902)
                    .unwrap()
                    .skill_cumulative_condition_id = 99999
            }
            7 => changed.deck[0].support_skills.push((99999, 1)),
            8 => {
                changed.master.skill_conditions.last_mut().unwrap().condition_type = 99999;
                changed
                    .master
                    .live_skill_effects
                    .iter_mut()
                    .find(|row| row.live_skill_id == 903)
                    .unwrap()
                    .skill_condition_group = 9901;
            }
            9 => {
                changed
                    .master
                    .gekisou_support_skill_effects
                    .iter_mut()
                    .find(|row| row.skill_id == 902)
                    .unwrap()
                    .skill_trigger_type = 99999
            }
            _ => unreachable!(),
        }
        changed.master.reindex().unwrap();
        assert!(key(&changed).is_none(), "unproved selected reader {change}");
        if change == 0 {
            let mut renamed = changed.clone();
            renamed.deck[0].character_id += 1000;
            assert_ne!(
                changed.native().run_with_random(&changed.play, &changed.delta, LiveRandom::new(17)).unwrap(),
                renamed.native().run_with_random(&renamed.play, &renamed.delta, LiveRandom::new(17)).unwrap(),
                "the rejected character reader must actually change native scoring"
            );
        }
    }
    let mut unused = original.clone();
    unused.master.skill_targets.push(crate::master::SkillTargetRow { id: 9901, character_id: 7, ..Default::default() });
    unused.master.reindex().unwrap();
    assert_eq!(key(&unused), key(&original), "an unselected master target reads no held performer");
}

#[test]
fn score_cap_character_identity_checks_ordinary_support_and_formation_cumulative_readers() {
    let mut ordinary = input();
    // A missing trigger is native NO_TRIGGER, so the source would otherwise never execute and the
    // character-sensitive condition/cumulative below would be an inert reader. Negating Fixed(false)
    // gives both ordinary sources a deterministic, character-independent trigger on every frame.
    ordinary.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 9912,
        condition_type: 8000,
        is_positive: false,
        condition_values: Vec::new(),
        condition_target_ids: Vec::new(),
    });
    ordinary.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 9912,
        group: 9912,
        condition_ids: vec![9912],
    });
    for id in [9910, 9911] {
        ordinary.master.support_skill_effects.push(crate::master::SupportSkillEffectRow {
            id,
            support_skill_id: id,
            level: 1,
            skill_trigger_type: SUSTAINED,
            skill_trigger_condition_group: 9912,
            skill_effect_type: 2000,
            effect_value: 40000,
            ..Default::default()
        });
        ordinary.deck[0].support_skills.push((id, 1));
    }
    ordinary.master.reindex().unwrap();
    let admitted = key(&ordinary).expect("complete character-independent ordinary support sources");
    let mut renamed = ordinary.clone();
    for performer in &mut renamed.deck {
        performer.character_id += 1000;
    }
    assert_eq!(key(&renamed).unwrap(), admitted);
    let mut native_a = ordinary.native();
    let mut native_b = renamed.native();
    assert_eq!(
        crate::live::full::luck_exact::initialized_identity(&mut native_a).unwrap(),
        crate::live::full::luck_exact::initialized_identity(&mut native_b).unwrap()
    );
    assert_eq!(
        native_a.run_with_random(&ordinary.play, &ordinary.delta, LiveRandom::new(17)).unwrap(),
        native_b.run_with_random(&renamed.play, &renamed.delta, LiveRandom::new(17)).unwrap()
    );
    let mut reordered = ordinary.clone();
    reordered.deck[0].support_skills.reverse();
    assert_ne!(key(&reordered).unwrap(), admitted, "ordinary source ordering remains in the identity");

    for reader in 0..3 {
        let mut changed = ordinary.clone();
        changed.master.skill_targets.push(crate::master::SkillTargetRow {
            id: 9910,
            character_id: 7,
            ..Default::default()
        });
        if reader == 0 {
            changed.master.skill_conditions.push(crate::master::SkillConditionRow {
                id: 9910,
                condition_type: 5000,
                condition_values: Vec::new(),
                condition_target_ids: vec![9910],
                is_positive: true,
            });
            changed.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
                id: 9910,
                group: 9910,
                condition_ids: vec![9910],
            });
            changed
                .master
                .support_skill_effects
                .iter_mut()
                .find(|row| row.support_skill_id == 9910)
                .unwrap()
                .skill_condition_group = 9910;
        } else {
            changed.master.cumulative_conditions.push(crate::master::CumulativeConditionRow {
                id: 9910,
                condition_type: 2999 + reader,
                condition_values: Vec::new(),
                condition_target_ids: vec![9910],
                max_cumulative_count: 0,
            });
            let row = changed.master.support_skill_effects.iter_mut().find(|row| row.support_skill_id == 9910).unwrap();
            row.skill_effect_type = 2001;
            row.skill_cumulative_condition_id = 9910;
            // 3001 excludes the owner; select another physical member to expose that read.
            if reader == 2 {
                changed.master.skill_targets.last_mut().unwrap().character_id = 8;
            }
        }
        changed.master.reindex().unwrap();
        assert!(key(&changed).is_none(), "selected ordinary character reader {reader}");
        let mut renamed = changed.clone();
        for performer in &mut renamed.deck {
            performer.character_id += 1000;
        }
        assert_ne!(
            changed.native().run_with_random(&changed.play, &changed.delta, LiveRandom::new(17)).unwrap(),
            renamed.native().run_with_random(&renamed.play, &renamed.delta, LiveRandom::new(17)).unwrap(),
            "rejected ordinary reader {reader} must change native scoring"
        );
    }
}
