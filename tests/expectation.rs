//! Small synthetic exhaustive checks; fixed-root vectors independently executed in arm64.
mod common;
use common::{Rng, roster, short_chart, synth_snaps};
use ournotes_deck::live::model::JudgementStream;
use ournotes_deck::live::random::{LUCK, SKILL};
use ournotes_deck::search::expectation::*;
use ournotes_deck::search::{Completion, Constraints, Objective, PlayInput, Pool, SearchRequest};
use std::collections::BTreeSet;

#[test]
fn native_arm64_fixed_roots() {
    let data: serde_json::Value = serde_json::from_str(include_str!("fixtures/native-roots.json")).unwrap();
    for row in data["fixtures"].as_array().unwrap() {
        let seed = row["root_seed"].as_i64().unwrap() as i32;
        let (order, mut random) = native_member_order(seed).unwrap();
        assert_eq!(serde_json::json!(order), row["order"], "root {seed}");
        assert_eq!(random.draws(), 4);
        for (name, stream) in [("skill", SKILL), ("luck", LUCK)] {
            let draws: Vec<i32> = (0..4).map(|_| random.range(stream, 10000).unwrap()).collect();
            assert_eq!(serde_json::json!(draws), row[name], "root {seed} {name}");
        }
    }
    // Constructor abs on sub-seeds does NOT permit folding the signed root law.
    assert_ne!(native_member_order(1).unwrap().0, native_member_order(-1).unwrap().0);
}

#[test]
fn exact_payoff_not_reward_of_mean_and_checked_overflow() {
    let atom = |s, w, p| SeedOutcome {
        root_seed: s,
        weight: w,
        performance_order: [0, 1, 2, 3, 4],
        final_score: s,
        terminal_payoff: p,
    };
    let v = aggregate(vec![atom(0, 1, 0), atom(100, 3, 10)]).unwrap();
    assert_eq!(v.expected_score, ExactExpectation { numerator: 300, denominator: 4 });
    assert_eq!(v.expected_payoff, ExactExpectation { numerator: 30, denominator: 4 });
    // rank threshold 80: reward(E(score)=75)=0; E(reward)=7.5.
    assert_eq!(v.payoff_mass[&10], 3);
    assert!(aggregate(vec![atom(0, 2, i128::MAX)]).is_err());
    assert!(aggregate(vec![]).is_err());
    assert!(FiniteSeedLaw::new(vec![(1, 0)]).is_err());
    assert_eq!(FiniteSeedLaw::new(vec![(1, 1), (-1, 2), (1, 3)]).unwrap().total_weight(), 6);
}

#[test]
fn every_physical_slot_and_snap_injection_is_visited() {
    let mut rng = Rng::new(101);
    let mut s = synth_snaps(&mut rng, 5, 2, &[2000]);
    for (name, rows) in &mut s.tables {
        if name == "MasterMemberCard" {
            for (i, row) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                row["_characterID"] = serde_json::json!(i + 1);
            }
        }
    }
    let master = s.master();
    let r = roster(&mut rng, &master);
    let pool = Pool::new(&master, &r).unwrap();
    let mut unique = BTreeSet::new();
    let count = visit_physical_decks(&pool, &Constraints::default(), |d| {
        pool.check_deck(&d.as_deck()).unwrap();
        assert!(unique.insert(d));
        Ok(true)
    })
    .unwrap();
    // 5! member orders x (empty + one snap in either identity + two ordered placements).
    assert_eq!(count, 120 * (1 + 10 + 20));
    let c = Constraints { leader: Some(pool.members[0].id), no_snaps: true, ..Default::default() };
    assert_eq!(
        visit_physical_decks(&pool, &c, |d| {
            assert_eq!(d.members[2], 0);
            Ok(true)
        })
        .unwrap(),
        24
    );
}

#[test]
fn finite_oracle_matches_independent_loop_and_resets_local_per_root() {
    let mut rng = Rng::new(202);
    let mut s = synth_snaps(&mut rng, 5, 1, &[2000]);
    for (name, rows) in &mut s.tables {
        if name == "MasterMemberCard" {
            for (i, row) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                row["_characterID"] = serde_json::json!(i + 1);
            }
        }
    }
    let master = s.master();
    let r = roster(&mut rng, &master);
    let pool = Pool::new(&master, &r).unwrap();
    let (chart, judgement_types) = short_chart(&mut rng, 6, false);
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    };
    let law = FiniteSeedLaw::new(vec![(1, 1), (-1, 3), (1, 2)]).unwrap();
    let request = SearchRequest {
        objective: objective.clone(),
        k: usize::MAX,
        constraints: Constraints { leader: Some(pool.members[0].id), no_snaps: true, ..Default::default() },
        time_limit: None,
    };
    let result = oracle(&pool, &request, &law).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.evaluated, 24);
    assert_eq!(result.results.len(), 24);
    assert!(
        result
            .results
            .windows(2)
            .all(|w| w[0].evaluation.expected_payoff.numerator >= w[1].evaluation.expected_payoff.numerator)
    );
    for row in &result.results {
        let mut expected = 0i128;
        for &(seed, weight) in law.atoms() {
            let out = evaluate_seed(&pool, &row.physical, &objective, seed).unwrap();
            expected += out.final_score as i128 * weight as i128;
        }
        assert_eq!(row.evaluation.expected_score.numerator, expected);
        assert_eq!(row.evaluation.expected_score.denominator, 6);
    }
    let d = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [Some(0), None, None, None, None] };
    let initial = vec![7];
    let mut calls = 0;
    let got = evaluate_finite(&pool, &d, &objective, &law, &initial, |physical, out, local| {
        assert_eq!(physical.snaps[0], Some(0));
        assert_eq!(*local, vec![7]);
        local.push(99);
        calls += 1;
        Ok(if out.final_score >= 1000 { 100 } else { 0 })
    })
    .unwrap();
    assert_eq!(calls, 3);
    assert_eq!(initial, vec![7]);
    assert_eq!(got.outcomes[0].final_score, got.outcomes[2].final_score);
    assert_eq!(got.outcomes[0].performance_order, got.outcomes[2].performance_order);
    let input = context(&pool, &d, &objective).unwrap();
    // A deliberately order-sensitive live: only physical slot 4 has a skill and only
    // performance position 0 triggers. Native roots 1/-1 select slots 4/1 respectively.
    let mut sensitive = input.clone();
    for p in &mut sensitive.performers {
        p.live_skill = None;
        p.support_skills.clear();
    }
    sensitive.performers[4].live_skill = Some((1, 1));
    sensitive.events = vec![(0, 0)];
    let high = sensitive.simulate(&master, 1).unwrap().final_score;
    let low = sensitive.simulate(&master, -1).unwrap().final_score;
    assert!(high > low, "fixture must distinguish native orders");
    let sensitive_law = FiniteSeedLaw::new(vec![(1, 1), (-1, 3)]).unwrap();
    let scored = evaluate_context(&master, &d, &sensitive, &sensitive_law, &(), |_, o, _| {
        Ok(if o.final_score == high { 100 } else { 0 })
    })
    .unwrap();
    assert_eq!(scored.expected_score.numerator, high as i128 + 3 * low as i128);
    assert_eq!(scored.expected_payoff, ExactExpectation { numerator: 100, denominator: 4 });
    assert_eq!(scored.score_mass.len(), 2);
    let assumption = evaluate_independent_uniform_order_assumption(
        &master,
        &d,
        &input,
        &FiniteSeedLaw::new(vec![(1, 1)]).unwrap(),
        &(),
        |_, o, _| Ok((o.performance_order[0] == 4) as i128),
    )
    .unwrap();
    assert_eq!(assumption.evaluation.expected_payoff, ExactExpectation { numerator: 24, denominator: 120 });
    let conditional =
        evaluate_context(&master, &d, &input, &FiniteSeedLaw::new(vec![(1, 1)]).unwrap(), &(), |_, o, _| {
            Ok((o.performance_order[0] == 4) as i128)
        })
        .unwrap();
    assert_eq!(conditional.expected_payoff.numerator, 1);
    if let Ok(path) = std::env::var("EXPECTATION_EXPORT") {
        let performers: Vec<_> = input
            .performers
            .iter()
            .map(|p| {
                serde_json::json!({
            "live_skill":p.live_skill, "support_skills":p.support_skills, "band_id":p.band_id,
            "character_id":p.character_id, "card_type":p.card_type, "gekisou_skill":p.gekisou_skill,
            "gekisou_support_skills":p.gekisou_support_skills, "tag_ids":p.tag_ids,
            "live_skill_categories":p.live_skill_categories, "gekisou_skill_categories":p.gekisou_skill_categories,
            "gekisou_mission_type":p.gekisou_mission_type })
            })
            .collect();
        let notes: Vec<_> =
            input.notes.iter().map(|n| [n.note_id, n.time_ms, n.note_operate_type, n.judgement_type]).collect();
        let frames: Vec<_> = input.play.frames.iter().zip(&input.delta_times).map(|(f,dt)| serde_json::json!({
            "t":f.time_ms,"dt":dt,"judged":f.judged.iter().map(|j| [j.note_id,j.judgement,j.judgement_time_ms]).collect::<Vec<_>>() })).collect();
        let output = serde_json::json!({ "tables":s.tables,"performers":performers,"notes":notes,
            "events":input.events,"frames":frames,"power":input.params.total_power,"level":input.params.music_level,
            "count":input.params.converted_note_count,"length":input.params.music_length_ms,"expected":got });
        std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }
    let (order, random) = native_member_order(1).unwrap();
    let perf = order.map(|i| input.performers[i].clone());
    let mut manual =
        ournotes_deck::live::full::LiveModel::new(&master, &perf, &input.notes, &input.events, input.params).unwrap();
    let score = manual.run_with_random(&input.play, &input.delta_times, random).unwrap();
    assert_eq!(score, got.outcomes[0].final_score);
    assert!(
        evaluate_finite(&pool, &d, &objective, &law, &(), |_, _, _| Err(ournotes_deck::Error::Input(
            "missing local event".into()
        )))
        .is_err()
    );
}

fn factory_fixture() -> (ournotes_deck::master::Master, ournotes_deck::cards::Roster, Objective) {
    let mut rng = Rng::new(909);
    let mut s = synth_snaps(&mut rng, 6, 2, &[2000]);
    for (name, rows) in &mut s.tables {
        if name == "MasterMemberCard" {
            for (i, row) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                row["_characterID"] = serde_json::json!((i + 1).min(5));
            }
        }
    }
    let master = s.master();
    let roster = roster(&mut rng, &master);
    let (chart, judgement_types) = short_chart(&mut rng, 4, false);
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    };
    (master, roster, objective)
}

#[test]
fn factory_isolates_rc_state_and_rejects_mismatched_deck() {
    use std::{cell::Cell, rc::Rc};
    let (master, roster, objective) = factory_fixture();
    let pool = Pool::new(&master, &roster).unwrap();
    let deck = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
    let input = context(&pool, &deck, &objective).unwrap();
    let law = FiniteSeedLaw::new(vec![(1, 1), (1, 1)]).unwrap();
    let increment = |_: &PhysicalDeck, _: &ConditionalOutcome, local: &mut Rc<Cell<i128>>| {
        local.set(local.get() + 1);
        Ok(local.get())
    };
    // Deliberately violate the Clone convenience precondition: reproduce why Clone
    // is not an isolation mechanism, rather than promising to repair shared state.
    let shared = Rc::new(Cell::new(0i128));
    let bad = evaluate_context(&master, &deck, &input, &law, &shared, increment).unwrap();
    assert_eq!(bad.outcomes.iter().map(|o| o.terminal_payoff).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(shared.get(), 2);
    let initial = Rc::new(Cell::new(0i128));
    let good = evaluate_finite_with_factory(
        &pool,
        &deck,
        &objective,
        &law,
        || Ok(Rc::new(Cell::new(initial.get()))),
        increment,
    )
    .unwrap();
    assert_eq!(good.outcomes.iter().map(|o| o.terminal_payoff).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(initial.get(), 0);
    let assumption = evaluate_independent_uniform_order_assumption_with_factory(
        &master,
        &deck,
        &input,
        &law,
        || Ok(Rc::new(Cell::new(initial.get()))),
        increment,
    )
    .unwrap();
    assert_eq!(assumption.evaluation.outcomes.len(), 240);
    assert!(assumption.evaluation.outcomes.iter().all(|o| o.terminal_payoff == 1));
    assert_eq!(initial.get(), 0);
    let request = SearchRequest {
        objective,
        k: 1,
        constraints: Constraints {
            leader: Some(pool.members[0].id),
            no_snaps: true,
            exclude_members: vec![pool.members[5].id],
            ..Default::default()
        },
        time_limit: None,
    };
    let result =
        oracle_with_payoff_factory(&pool, &request, &law, || Ok(Rc::new(Cell::new(initial.get()))), increment).unwrap();
    assert_eq!(result.evaluated, 24);
    assert_eq!(result.results[0].evaluation.expected_payoff, ExactExpectation { numerator: 2, denominator: 2 });
    assert_eq!(initial.get(), 0);
    struct OwnedState(i128);
    let nc = evaluate_context_with_factory(
        &master,
        &deck,
        &input,
        &law,
        || Ok(OwnedState(0)),
        |_, _, local| {
            local.0 += 1;
            Ok(local.0)
        },
    )
    .unwrap();
    assert!(nc.outcomes.iter().all(|o| o.terminal_payoff == 1));
    let mut wrong = deck;
    wrong.members.swap(0, 1);
    assert_eq!(input.physical(), deck);
    let mut called = false;
    let mismatch = evaluate_context_with_factory(
        &master,
        &wrong,
        &input,
        &law,
        || {
            called = true;
            Ok(())
        },
        |_, _, _| Ok(0),
    );
    assert!(mismatch.is_err());
    assert!(!called);
    assert!(evaluate_context(&master, &wrong, &input, &law, &(), |_, _, _| Ok(0)).is_err());
    assert!(
        evaluate_independent_uniform_order_assumption_with_factory(
            &master,
            &wrong,
            &input,
            &law,
            || Ok(()),
            |_, _, _| Ok(0)
        )
        .is_err()
    );
    let failed = evaluate_context_with_factory(
        &master,
        &deck,
        &input,
        &law,
        || Err::<(), _>(ournotes_deck::Error::Input("factory failure".into())),
        |_, _, _| panic!("not called"),
    );
    assert!(failed.is_err());
}

#[test]
fn constrained_physical_enumeration_matches_independent_cartesian_reference() {
    let (master, roster, _) = factory_fixture();
    let pool = Pool::new(&master, &roster).unwrap();
    // Six cards, last two of the same character. Cartesian product rejection is
    // independent of production recursive member and snap injection.
    let id = |i: usize| pool.members[i].id;
    let constraints = [
        Constraints::default(),
        Constraints { include_members: vec![id(0), id(4)], exclude_members: vec![id(5)], ..Default::default() },
        Constraints {
            leader: Some(id(0)),
            exclude_members: vec![id(4)],
            exclude_snaps: vec![pool.snaps[1].id],
            ..Default::default()
        },
        Constraints { include_members: vec![id(4), id(5)], ..Default::default() },
        Constraints { exclude_members: vec![id(0), id(1)], no_snaps: true, ..Default::default() },
    ];
    for c in constraints {
        let mut want = BTreeSet::new();
        for encoded in 0..6usize.pow(5) {
            let mut code = encoded;
            let members = std::array::from_fn(|_| {
                let i = code % 6;
                code /= 6;
                i
            });
            let chars: BTreeSet<_> = members.map(|m| pool.members[m].character_id).into_iter().collect();
            if chars.len() != 5
                || members.iter().any(|m| c.exclude_members.contains(&id(*m)))
                || !c.include_members.iter().all(|r| members.iter().any(|m| id(*m) == *r))
                || c.leader.is_some_and(|leader| id(members[2]) != leader)
            {
                continue;
            }
            for encoded in 0..3usize.pow(5) {
                let mut code = encoded;
                let snaps = std::array::from_fn(|_| {
                    let i = code % 3;
                    code /= 3;
                    if i == 0 { None } else { Some(i - 1) }
                });
                let equipped: Vec<_> = snaps.iter().flatten().copied().collect();
                if equipped.iter().copied().collect::<BTreeSet<_>>().len() != equipped.len()
                    || (c.no_snaps && !equipped.is_empty())
                    || equipped.iter().any(|s| c.exclude_snaps.contains(&pool.snaps[*s].id))
                {
                    continue;
                }
                want.insert(PhysicalDeck { members, snaps });
            }
        }
        let mut got = BTreeSet::new();
        let count = visit_physical_decks(&pool, &c, |d| {
            assert!(got.insert(d));
            Ok(true)
        })
        .unwrap();
        assert_eq!(count, want.len() as u64);
        assert_eq!(got, want);
    }
    assert!(
        visit_physical_decks(
            &pool,
            &Constraints { include_members: vec![id(0)], exclude_members: vec![id(0)], ..Default::default() },
            |_| Ok(true)
        )
        .is_err()
    );
}

#[test]
fn exact_json_integer_boundaries() {
    // Default serde_json writer accepts 128-bit integers; Value does not. A second pass
    // with dependency arbitrary_precision and the env flag validates the CLI Value path.
    let precise = serde_json::to_value(u128::MAX).is_ok();
    if std::env::var_os("EXPECT_ARBITRARY_PRECISION").is_some() {
        assert!(precise);
    }
    for n in [i128::MIN, i128::MAX] {
        let value = ExactExpectation { numerator: n, denominator: u128::MAX };
        let text = serde_json::to_string(&value).unwrap();
        assert!(text.contains(&n.to_string()));
        assert!(text.contains(&u128::MAX.to_string()));
        let as_value = serde_json::to_value(value);
        if precise {
            let v = as_value.unwrap();
            assert_eq!(v["numerator"].to_string(), n.to_string());
            assert_eq!(v["denominator"].to_string(), u128::MAX.to_string());
        } else {
            assert!(as_value.is_err());
        }
    }
    let law = FiniteSeedLaw::new(vec![(0, u64::MAX), (1, u64::MAX)]).unwrap();
    let total = 2 * u64::MAX as u128;
    assert_eq!(law.total_weight(), total);
    let value = serde_json::to_value(&law);
    if precise {
        assert_eq!(value.unwrap()["total_weight"].to_string(), total.to_string());
    } else {
        assert!(value.is_err());
    }
    let outcomes = [0, 1].map(|seed| SeedOutcome {
        root_seed: seed,
        weight: u64::MAX,
        performance_order: [0, 1, 2, 3, 4],
        final_score: 1,
        terminal_payoff: 1,
    });
    let evaluation = aggregate(outcomes.to_vec()).unwrap();
    assert_eq!(evaluation.payoff_mass[&1], total);
    let value = serde_json::to_value(&evaluation);
    if precise {
        let value = value.unwrap();
        assert_eq!(value["payoff_mass"]["1"].to_string(), total.to_string());
        assert_eq!(value["score_mass"]["1"].to_string(), total.to_string());
    } else {
        assert!(value.is_err());
    }
}
