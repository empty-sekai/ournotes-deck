use super::factory::{life_percent_threshold, target_matches};
use super::*;

fn with_life<T>(life: i32, f: impl FnOnce(&mut CheckCtx) -> T) -> T {
    let mut life = LifeController::new(life, Default::default(), 1000).unwrap();
    let mut random = LiveRandom::new(0);
    f(&mut CheckCtx {
        life: &mut life,
        random: &mut random,
        frame_time: 0,
        current_combo: 0,
        judged: &[],
        events: &[],
        gk: None,
        prev_confirmed_rank: None,
    })
}

#[test]
fn ordinary_combo_crossings_are_not_count_resettable() {
    with_life(1000, |ctx| {
        let mut c = Checker::LiveComboMultiple { threshold: 10, previous: 25 };
        assert!(!c.count_resettable());
        for (combo, hits) in [(29, 0), (35, 1), (35, 0), (7, 0), (25, 2)] {
            ctx.current_combo = combo;
            assert_eq!(c.check(ctx).unwrap(), (hits > 0, hits));
        }
        c.reset(ctx).unwrap();
        assert_eq!(c.check(ctx).unwrap(), (true, 2));
        let mut disabled = Checker::LiveComboAtLeast(0);
        assert_eq!(disabled.check(ctx).unwrap(), (false, 0));
    });
}

#[test]
fn factory_team_music_and_snap_targets_preserve_native_quantifiers() {
    use serde_json::json;
    let mut tables = json!({
        "MasterSkillTarget": [{"_id":1,"_skillTargetType":1,"_bandID":1,"_liveMusicType":99},
                              {"_id":2,"_skillTargetType":4,"_bandID":2,"_liveMusicType":1}],
        "MasterSkillCondition": [],
    });
    for (id, kind, targets) in [
        (1, 3000, vec![1]),
        (2, 3001, vec![1, 2]),
        (3, 3001, vec![1]),
        (4, 4012, vec![1]),
        (5, 4012, vec![2]),
        (6, 5020, vec![]),
    ] {
        tables["MasterSkillCondition"].as_array_mut().unwrap().push(json!({"_id":id,
            "_conditionType":kind,"_conditionValues":[],"_conditionTargetIDs":targets,"_isPositive":true}));
    }
    let text: Vec<(String, String)> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    let master =
        Master::from_json_tables(|name| text.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())).unwrap();
    let deck = [Performer { band_id: 1, ..Default::default() }, Performer { band_id: 2, ..Default::default() }];
    let f =
        Factory { master: &master, deck: &deck, initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 1 };
    with_life(1000, |ctx| {
        for (id, want) in [(1, true), (2, true), (3, false), (4, false), (5, true)] {
            assert_eq!(f.one(id, 0).unwrap().unwrap().check(ctx).unwrap(), (want, i64::from(want)));
        }
        ctx.events = &[(0, 10), (0, 20)];
        assert_eq!(f.one(6, 0).unwrap().unwrap().check(ctx).unwrap(), (true, 1));
        assert_eq!(f.one(6, 1).unwrap().unwrap().check(ctx).unwrap(), (false, 0));
        let empty = Factory { deck: &[], ..f };
        assert_eq!(empty.one(2, 0).unwrap().unwrap().check(ctx).unwrap(), (true, 1));
    });
}

#[test]
fn failed_not_preserves_other_and_counters_and_or_hit_count() {
    with_life(1000, |ctx| {
        let mut c = Checker::And {
            items: vec![
                Checker::Not(Box::new(Checker::Fixed(true))),
                Checker::ElapsedTime { period: 1000, elapsed: 700, previous: 0 },
            ],
            resettable: vec![true, true],
        };
        assert_eq!(c.check(ctx).unwrap(), (false, 0));
        if let Checker::And { items, .. } = c {
            assert!(matches!(items[1], Checker::ElapsedTime { elapsed: 700, .. }));
        }
        let mut c = Checker::Or(vec![Checker::Not(Box::new(Checker::Fixed(true))), Checker::Probability(2.0)]);
        assert_eq!(c.check(ctx).unwrap(), (true, 1));
        assert_eq!(c.override_time(), None);
    });
}

#[test]
fn a_combination_reports_no_override_time_of_its_items() {
    with_life(1000, |ctx| {
        ctx.judged = &[(1, 5, 300)];
        let judged = || Checker::NoteJudgementMatch { kind: 1000, targets: vec![5], override_ms: None };
        let mut lone = judged();
        assert_eq!(lone.check(ctx).unwrap(), (true, 1));
        assert_eq!(lone.override_time(), Some(300));
        let mut and = Checker::And { items: vec![judged(), Checker::Fixed(true)], resettable: vec![false, false] };
        assert_eq!(and.check(ctx).unwrap(), (true, 1));
        assert_eq!(and.override_time(), None);
        let mut or = Checker::Or(vec![judged(), Checker::Fixed(false)]);
        assert_eq!(or.check(ctx).unwrap(), (true, 1));
        assert_eq!(or.override_time(), None);
    });
}

#[test]
fn elapsed_time_catches_up_one_period_and_reset_rebases() {
    with_life(1000, |ctx| {
        let mut c = Checker::ElapsedTime { period: 1000, elapsed: 0, previous: 100 };
        ctx.frame_time = 3600;
        assert_eq!(c.check(ctx).unwrap(), (true, 1));
        assert_eq!(c.check(ctx).unwrap(), (true, 1));
        assert_eq!(c.check(ctx).unwrap(), (true, 1));
        assert_eq!(c.check(ctx).unwrap(), (false, 0));
        ctx.frame_time = 4000;
        c.reset_count(ctx).unwrap();
        ctx.frame_time = 4500;
        assert_eq!(c.check(ctx).unwrap(), (false, 0));
        ctx.frame_time = 5000;
        assert_eq!(c.check(ctx).unwrap(), (true, 1));
    });
}

#[test]
fn member_targets_are_ordered_alternatives_not_first_nonzero() {
    let p = Performer {
        band_id: 1,
        character_id: 2,
        card_type: 3,
        tag_ids: vec![7],
        live_skill_categories: vec![0, 10],
        gekisou_skill_categories: vec![20],
        gekisou_mission_type: 2,
        ..Default::default()
    };
    let t = SkillTargetRow { skill_target_type: 4, character_id: 999, band_id: 1, ..Default::default() };
    assert!(target_matches(&t, &p));
    for t in [
        SkillTargetRow { band_id: 999, card_type: 3, ..Default::default() },
        SkillTargetRow { band_id: 999, character_id: 2, ..Default::default() },
        SkillTargetRow { tag_id: 7, ..Default::default() },
        SkillTargetRow { tag_id: 999, live_skill_categories: vec![10], ..Default::default() },
        SkillTargetRow { tag_id: 999, gekisou_skill_categories: vec![20], ..Default::default() },
        SkillTargetRow { gekisou_mission_type: 2, ..Default::default() },
    ] {
        assert!(target_matches(&t, &p));
    }
    assert!(!target_matches(&SkillTargetRow::default(), &p));
    assert!(!target_matches(&SkillTargetRow { live_skill_categories: vec![0], ..Default::default() }, &p));
    assert!(!target_matches(&SkillTargetRow { gekisou_mission_type: 4, ..Default::default() }, &p));
    assert!(!target_matches(
        &SkillTargetRow { band_id: -1, ..Default::default() },
        &Performer { band_id: -1, ..Default::default() }
    ));
}

#[test]
fn judgement_matches_count_notes_not_duplicate_targets() {
    with_life(1000, |ctx| {
        ctx.judged = &[(1, 5, 100), (2, 4, 200), (3, 5, 300)];
        for (kind, expected) in [(1000, 2), (1010, 2), (1020, 3)] {
            let mut c = Checker::NoteJudgementMatch { kind, targets: vec![5, 5], override_ms: None };
            assert_eq!(c.check(ctx).unwrap(), (true, expected));
            assert_eq!(c.override_time(), Some(300));
        }
    });
}

#[test]
fn judgement_comparisons_preserve_special_value_asymmetry() {
    with_life(1000, |ctx| {
        ctx.judged = &[(1, 7, 100)];
        let mut less = Checker::NoteJudgementMatch { kind: 1020, targets: vec![5], override_ms: None };
        assert_eq!(less.check(ctx).unwrap(), (false, 0));
        let mut greater = Checker::NoteJudgementMatch { kind: 1010, targets: vec![5], override_ms: None };
        assert!(greater.check(ctx).is_err());
        ctx.judged = &[(1, 5, 100), (-1, 5, 200)];
        assert_eq!(greater.check(ctx).unwrap(), (true, 2));
        assert_eq!(greater.override_time(), None);
    });
}

#[test]
fn counted_judgement_reset_only_clears_the_count() {
    with_life(1000, |ctx| {
        let mut c =
            Checker::NoteJudgementCount { n: 2, consecutive: true, targets: vec![5], count: 1, override_ms: Some(100) };
        c.reset(ctx).unwrap();
        assert_eq!(c.override_time(), Some(100));
        ctx.judged = &[(2, 5, 200)];
        assert_eq!(c.check(ctx).unwrap(), (false, 0));
        assert_eq!(c.override_time(), None);
    });
}

#[test]
fn consecutive_judgements_reset_for_each_nonmatching_target() {
    with_life(1000, |ctx| {
        ctx.judged = &[(1, 5, 100), (2, 5, 200)];
        for (targets, expected) in [(vec![5], 1), (vec![4, 5], 0), (vec![5, 5], 2)] {
            let mut c = Checker::NoteJudgementCount { n: 2, consecutive: true, targets, count: 0, override_ms: None };
            assert_eq!(c.check(ctx).unwrap(), (expected > 0, expected));
        }
    });
}

#[test]
fn strict_life_comparisons_exclude_the_boundary() {
    let mut greater = Checker::LifeGreater(Some(1000));
    let mut less = Checker::LifeLess(Some(1000));
    assert_eq!(with_life(1000, |c| greater.check(c)).unwrap(), (false, 0));
    assert_eq!(with_life(1001, |c| greater.check(c)).unwrap(), (true, 1));
    assert_eq!(with_life(1000, |c| less.check(c)).unwrap(), (false, 0));
    assert_eq!(with_life(999, |c| less.check(c)).unwrap(), (true, 1));
}

#[test]
fn life_change_reports_zero_hit_count_and_keeps_history_on_reset() {
    let mut changed = Checker::LifeChanged { previous: 1000 };
    assert_eq!(with_life(900, |c| changed.check(c)).unwrap(), (true, 0));
    with_life(800, |c| changed.reset(c)).unwrap();
    assert_eq!(with_life(900, |c| changed.check(c)).unwrap(), (false, 0));
}

#[test]
fn life_percentage_only_reports_downward_crossings() {
    assert_eq!(life_percent_threshold(3, 999), 300);
    assert_eq!(life_percent_threshold(3, 1), 1);
    assert_eq!(life_percent_threshold(-3, 999), -299);
    let mut c = Checker::LifePercent { threshold: 300, previous: 0 };
    for (life, want) in [(200, false), (1000, false), (300, true), (200, false), (301, false), (299, true)] {
        assert_eq!(with_life(life, |ctx| c.check(ctx)).unwrap(), (want, i64::from(want)));
    }
}

#[test]
fn life_delta_keeps_remainders_and_resets_to_current_life() {
    let mut c = Checker::LifeDelta { threshold: 100, increase: false, count: 0, previous: 1000 };
    for (life, hits) in [(750, 2), (800, 0), (650, 2)] {
        assert_eq!(with_life(life, |ctx| c.check(ctx)).unwrap(), (hits > 0, hits));
    }
    with_life(500, |ctx| c.reset(ctx)).unwrap();
    assert_eq!(with_life(450, |ctx| c.check(ctx)).unwrap(), (false, 0));
    assert_eq!(with_life(400, |ctx| c.check(ctx)).unwrap(), (true, 1));
}

#[test]
fn failed_and_resets_life_delta_to_the_current_life() {
    let mut c = Checker::And {
        items: vec![
            Checker::LifeDelta { threshold: 100, increase: true, count: 50, previous: 1000 },
            Checker::Fixed(false),
        ],
        resettable: vec![true, false],
    };
    // The counting check fails first, so the remainder survives.
    assert_eq!(with_life(1040, |ctx| c.check(ctx)).unwrap(), (false, 0));
    // It now hits, but the next non-counting condition fails and resets both count and baseline.
    assert_eq!(with_life(1100, |ctx| c.check(ctx)).unwrap(), (false, 0));
    if let Checker::And { items, .. } = &mut c {
        assert_eq!(with_life(1150, |ctx| items[0].check(ctx)).unwrap(), (false, 0));
        assert_eq!(with_life(1200, |ctx| items[0].check(ctx)).unwrap(), (true, 1));
    }
}
fn cumulative_master(kind: i64, values: &[i64], max: i64) -> Master {
    let rows = serde_json::json!({"_allData": [{"_id": 1, "_skillCumulativeConditionType": kind,
        "_conditionValues": values, "_conditionTargetIDs": [1, 1], "_maxCumulativeCount": max}]})
    .to_string();
    let targets = serde_json::json!({"_allData": [{"_id": 1, "_characterID": 7,
        "_judgement": 5}]})
    .to_string();
    Master::from_json_tables(|name| match name {
        "MasterSkillCumulativeCondition" => Some(rows.as_str()),
        "MasterSkillTarget" => Some(targets.as_str()),
        _ => None,
    })
    .unwrap()
}

#[test]
fn cumulative_factory_formation_counts_slots_and_never_divides_by_n() {
    let deck: Vec<_> = [(1, 1), (1, 1), (2, 2), (2, 0), (3, 99)]
        .into_iter()
        .map(|(band_id, card_type)| Performer { band_id, card_type, character_id: 7, ..Default::default() })
        .collect();
    for (kind, expected) in [(3000, 5), (3001, 4), (3002, 2), (3003, 2), (3004, 3), (3005, 4)] {
        for values in [vec![], vec![99], vec![0], vec![-1]] {
            for cap in [-1, 0, 1, 3, 99] {
                let master = cumulative_master(kind, &values, cap);
                let f = Factory {
                    master: &master,
                    deck: &deck,
                    initial_life: 1000,
                    initial_time_ms: 0,
                    skill_target_music_type: 0,
                };
                let mut c = f.cumulative(1, 1).unwrap().unwrap();
                let want = if cap > 0 { expected.min(cap) } else { expected };
                assert_eq!((c.init_count(), c.unit(), c.max()), (want, 0, 0));
                assert_eq!(with_life(1000, |ctx| c.update_count(ctx)).unwrap(), want);
                c.reset();
                assert_eq!(with_life(250, |ctx| c.update_count(ctx)).unwrap(), want);
            }
        }
    }
    for kind in 3000..=3005 {
        let master = cumulative_master(kind, &[], 0);
        let f = Factory {
            master: &master,
            deck: &deck,
            initial_life: 1000,
            initial_time_ms: 0,
            skill_target_music_type: 0,
        };
        assert_eq!(f.cumulative(1, 99).is_err(), kind == 3002 || kind == 3003);
        let f =
            Factory { master: &master, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 };
        assert_eq!(f.cumulative(1, 0).is_err(), kind == 3002 || kind == 3003);
    }
}

#[test]
fn cumulative_factory_elapsed_backlog_reset_and_wrapping() {
    let master = cumulative_master(6000, &[1], 2);
    let f =
        Factory { master: &master, deck: &[], initial_life: 1000, initial_time_ms: 100, skill_target_music_type: 0 };
    let mut c = f.cumulative(1, 99).unwrap().unwrap();
    assert_eq!((c.init_count(), c.unit(), c.max()), (0, 0, 0));
    with_life(1000, |ctx| {
        for (now, want) in [(3600, 1), (3600, 2), (3600, 2)] {
            ctx.frame_time = now;
            assert_eq!(c.update_count(ctx).unwrap(), want);
        }
        c.reset();
        ctx.frame_time = 4000;
        assert_eq!(c.update_count(ctx).unwrap(), 0);
        ctx.frame_time = 4600;
        assert_eq!(c.update_count(ctx).unwrap(), 1);
        ctx.frame_time = 3500;
        assert_eq!(c.update_count(ctx).unwrap(), 1);
    });
    for n in [0, -1, i32::MAX as i64] {
        let master = cumulative_master(6000, &[n], 0);
        let f = Factory {
            master: &master,
            deck: &[],
            initial_life: 0,
            initial_time_ms: i32::MAX,
            skill_target_music_type: 0,
        };
        let mut c = f.cumulative(1, 0).unwrap().unwrap();
        with_life(0, |ctx| {
            ctx.frame_time = i32::MIN;
            assert_eq!(c.update_count(ctx).unwrap(), 1);
        });
    }
}

#[test]
fn cumulative_factory_life_initial_cap_and_signed_judgements() {
    for kind in [2000, 2001] {
        let master = cumulative_master(kind, &[100], 3);
        let f =
            Factory { master: &master, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 };
        let mut c = f.cumulative(1, 0).unwrap().unwrap();
        assert_eq!(c.init_count(), if kind == 2000 { 10 } else { 0 });
        assert_eq!((c.unit(), c.max()), (0, 0));
        assert_eq!(with_life(1000, |ctx| c.update_count(ctx)).unwrap(), 3);
        c.reset();
        assert_eq!(c.init_count(), if kind == 2000 { 10 } else { 0 });
    }
    for (kind, want) in [(1000, 1), (1001, 3), (1002, 3)] {
        let master = cumulative_master(kind, &[1], 0);
        let f =
            Factory { master: &master, deck: &[], initial_life: 1000, initial_time_ms: 0, skill_target_music_type: 0 };
        let mut c = f.cumulative(1, 0).unwrap().unwrap();
        with_life(1000, |ctx| {
            ctx.judged = &[(1, -1, 0), (2, 0, 0), (3, 5, 0), (4, 6, 0), (5, 7, 0)];
            assert_eq!(c.update_count(ctx).unwrap(), want);
            assert_eq!(c.update_count(ctx).unwrap(), want * 2);
            c.reset();
            assert_eq!(c.update_count(ctx).unwrap(), want);
        });
    }
    let master = cumulative_master(0, &[], 0);
    let f = Factory { master: &master, deck: &[], initial_life: 0, initial_time_ms: 0, skill_target_music_type: 0 };
    assert!(f.cumulative(0, 0).unwrap().is_none());
    assert!(f.cumulative(1, 0).is_err());
}
