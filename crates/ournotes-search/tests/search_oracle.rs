//! The search against exhaustive enumeration on small synthetic pools.

#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, chart, play, roster, synth};
use ournotes_search::search::oracle::brute_force_best_order_diagnostic as brute_force;
use ournotes_search::search::{
    Completion, Constraints, Objective, PlayInput, SearchRequest, evaluate, search_best_order_diagnostic as search,
};
use ournotes_sim::pool::Pool;

fn cases() -> u64 {
    std::env::var("OURNOTES_DECK_ORACLE_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(12)
}

fn seed0() -> u64 {
    std::env::var("OURNOTES_DECK_ORACLE_SEED0").ok().and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn size() -> (i64, i64) {
    let m = std::env::var("OURNOTES_DECK_ORACLE_MEMBERS").ok().and_then(|s| s.parse().ok()).unwrap_or(8);
    let s = std::env::var("OURNOTES_DECK_ORACLE_SNAPS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    (m, s)
}

fn run(objective: Objective, constraints: Constraints, seed: u64) -> (usize, u64) {
    let mut rng = Rng::new(seed);
    let (nm, ns) = size();
    let s = synth(&mut rng, nm, ns);
    let master = s.master();
    let r = roster(&mut rng, &master);
    let pool = Pool::new(&master, &r).unwrap();
    let mut compared = 0;
    let all = SearchRequest {
        objective: objective.clone(),
        k: usize::MAX,
        constraints: constraints.clone(),
        time_limit: None,
    };
    let want = brute_force(&pool, &all);
    let evaluated = want.as_ref().map_or(0, |w| w.1);
    for k in [1usize, 3, 10, 1000] {
        let req = SearchRequest { objective: objective.clone(), k, constraints: constraints.clone(), time_limit: None };
        match (search(&pool, &req), &want) {
            (Ok(g), Ok((w, _))) => {
                assert_eq!(g.completion, Completion::Complete);
                let w = &w[..k.min(w.len())];
                assert_eq!(g.results, w, "seed {seed} k {k} {:?}", req.objective);
                for r in &g.results {
                    let deck = pool.deck(r.members, r.snaps, r.performance_order).unwrap();
                    let (p, score) = evaluate(&pool, &deck, &req.objective).unwrap();
                    assert_eq!((p, score), (r.power, r.score), "seed {seed}: evaluate disagrees");
                }
                compared += w.len();
            }
            (Err(a), Err(b)) => assert_eq!(&a, b),
            (g, w) => panic!("seed {seed}: search {g:?} vs oracle {w:?}"),
        }
    }
    (compared, evaluated)
}

fn constraint_variants(rng: &mut Rng, nm: i64, ns: i64) -> Vec<Constraints> {
    vec![
        Constraints::default(),
        Constraints { no_snaps: true, ..Default::default() },
        Constraints { leader: Some(rng.range(1, nm)), ..Default::default() },
        Constraints {
            include_members: vec![rng.range(1, nm)],
            exclude_snaps: vec![rng.range(1, ns)],
            ..Default::default()
        },
        Constraints { exclude_members: vec![rng.range(1, nm), rng.range(1, nm)], ..Default::default() },
    ]
}

#[test]
fn power_matches_oracle() {
    let (nm, ns) = size();
    let mut total = (0, 0);
    for seed in seed0()..seed0() + cases() {
        let mut rng = Rng::new(seed ^ 0xabc);
        for c in constraint_variants(&mut rng, nm, ns) {
            for objective in [
                Objective::Power { music_id: None, event: false },
                Objective::Power { music_id: Some(10 * rng.range(1, 3)), event: false },
            ] {
                let r = run(objective, c.clone(), seed);
                total.0 += r.0;
                total.1 += r.1;
            }
        }
    }
    eprintln!("power oracle: {} result rows compared, {} decks enumerated", total.0, total.1);
}

#[test]
fn skip_matches_oracle() {
    let mut total = (0, 0);
    for seed in seed0()..seed0() + cases() {
        let mut rng = Rng::new(seed ^ 0x5eed);
        let notes = 40 + rng.below(200) as usize;
        let ch = chart(&mut rng, notes);
        let score_id = [1004, 2003, 3001][rng.below(3) as usize];
        let r = run(Objective::SkipScore { score_id, chart: ch }, Constraints::default(), seed);
        total.0 += r.0;
        total.1 += r.1;
    }
    eprintln!("skip oracle: {} result rows compared, {} decks enumerated", total.0, total.1);
}

#[test]
fn infeasible_pool_is_complete_and_empty() {
    let mut rng = Rng::new(7);
    let s = synth(&mut rng, 3, 2);
    let master = s.master();
    let r = roster(&mut rng, &master);
    let pool = Pool::new(&master, &r).unwrap();
    let req = SearchRequest {
        objective: Objective::Power { music_id: None, event: false },
        k: 5,
        constraints: Constraints::default(),
        time_limit: None,
    };
    let out = search(&pool, &req).unwrap();
    assert_eq!(out.completion, Completion::Complete);
    assert!(out.results.is_empty());
}

#[test]
fn live_matches_oracle() {
    let mut total = (0, 0);
    for seed in seed0()..seed0() + cases() {
        let mut rng = Rng::new(seed ^ 0x11fe);
        let notes = 60 + rng.below(160) as usize;
        let ch = chart(&mut rng, notes);
        let pl = play(&mut rng, &ch);
        let score_id = [1004, 2003, 3001][rng.below(3) as usize];
        for c in [Constraints::default(), Constraints { no_snaps: true, ..Default::default() }] {
            let objective = Objective::LiveScore {
                score_id,
                chart: ch.clone(),
                play: PlayInput::Notes(pl.clone()),
                event: false,
                exclude_snap_skills: true,
                gekisou: None,
            };
            let r = run(objective, c, seed);
            total.0 += r.0;
            total.1 += r.1;
        }
    }
    eprintln!("live oracle: {} result rows compared, {} decks enumerated", total.0, total.1);
}

#[test]
fn live_model_matches_general_calculator() {
    use ournotes_sim::live::model::LiveModel;
    for seed in 0..cases() * 4 {
        let mut rng = Rng::new(seed ^ 0x7a7a);
        let s = synth(&mut rng, 4, 0);
        let master = s.master();
        let notes = 50 + rng.below(400) as usize;
        let ch = chart(&mut rng, notes);
        let pl = play(&mut rng, &ch);
        let model = LiveModel::new(&master, rng.range(5, 30) as i32, &ch, &pl).unwrap();
        let perf: Vec<(i64, i64)> = (0..5).map(|_| (rng.range(1, 3), rng.range(1, 5))).collect();
        let cmds = model.commands(&master, &perf).unwrap();
        let p = rng.range(0, 900_000) as i32;
        assert_eq!(model.score(p, &cmds), model.score_reference(p, &pl, &cmds).unwrap(), "seed {seed}");
    }
}

fn per_order_numeric_fixture(
    adjustment: &str,
) -> (ournotes_sim::master::Master, ournotes_sim::cards::Roster, SearchRequest) {
    use common::set_column;
    use ournotes_sim::live::model::Play;
    use ournotes_sim::live::score::PERFECT;
    use ournotes_sim::live::skill::NotePlay;
    use ournotes_sim::live::skip::{Chart, ChartNote};
    use serde_json::json;

    let mut rng = Rng::new(1);
    let mut data = synth(&mut rng, 5, 1);
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        row["_characterID"] = row["_id"].clone();
        row["_leaderSkillID"] = json!(0);
        for stat in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
            row[stat] = json!(10_000);
        }
    });
    set_column(&mut data, "MasterLiveSettings", &mut |row| {
        if row["_key"] == "note_score_adjustment_factor" {
            row["_value"] = json!(adjustment);
        }
    });
    let master = data.master();
    let mut owned = roster(&mut rng, &master);
    owned.player = Default::default();
    for member in &mut owned.members {
        member.level = Some(40);
        member.awake = 1;
        member.rank = 1;
    }
    let notes =
        vec![ChartNote { id: 1, time_ms: 1000, note_type: 1 }, ChartNote { id: 2, time_ms: 2000, note_type: 1 }];
    let play = Play {
        notes: notes
            .iter()
            .map(|note| NotePlay {
                note_id: note.id,
                time_ms: note.time_ms,
                note_type: note.note_type,
                score_type: PERFECT,
                life: 1000,
                combo: 0,
            })
            .collect(),
        life_at_event: vec![],
        assist: false,
    };
    let objective = Objective::LiveScore {
        score_id: 1004,
        chart: Chart { converted_note_count: 2, last_timing_note_ms: 2000, notes, skill_events: vec![] },
        play: PlayInput::Notes(play),
        event: false,
        exclude_snap_skills: true,
        gekisou: None,
    };
    let request = SearchRequest {
        objective,
        k: 1,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    };
    (master, owned, request)
}

#[test]
fn per_order_search_rejects_negative_and_wrapping_power_rank_reversals() {
    use ournotes_sim::error::Error;
    for (adjustment, best_score, higher_power_score) in
        [("-1", -166_564, -168_244), ("12800", 2_132_001_792, -2_141_451_008)]
    {
        let (master, owned, request) = per_order_numeric_fixture(adjustment);
        let pool = Pool::new(&master, &owned).unwrap();
        let (expected, evaluated) = brute_force(&pool, &request).unwrap();
        assert_eq!(evaluated, 720);
        assert_eq!(expected[0].snaps, [None; 5]);
        assert_eq!((expected[0].power, expected[0].score), (152_112, Some(best_score)));
        let higher = pool.deck([2, 3, 1, 4, 5], [Some(1), None, None, None, None], [0, 1, 2, 3, 4]).unwrap();
        assert_eq!(evaluate(&pool, &higher, &request.objective).unwrap(), (153_647, Some(higher_power_score)));
        assert!(higher_power_score < best_score);
        assert!(matches!(search(&pool, &request), Err(Error::Domain(_))));
    }
}

#[test]
fn per_order_search_requires_a_finite_normal_score_chain() {
    use ournotes_sim::error::Error;
    for adjustment in ["NaN", "inf", "3e38", "1e-45"] {
        let (master, owned, request) = per_order_numeric_fixture(adjustment);
        let pool = Pool::new(&master, &owned).unwrap();
        assert!(matches!(search(&pool, &request), Err(Error::Domain(_))), "adjustment {adjustment}");
    }
    let (master, owned, mut request) = per_order_numeric_fixture("2.5");
    let pool = Pool::new(&master, &owned).unwrap();
    let (expected, _) = brute_force(&pool, &request).unwrap();
    let actual = search(&pool, &request).unwrap();
    assert_eq!(actual.completion, Completion::Complete);
    assert_eq!(actual.results, expected);
    if let Objective::LiveScore { chart, .. } = &mut request.objective {
        chart.converted_note_count = 0;
    }
    assert!(matches!(search(&pool, &request), Err(Error::Domain(_))));
}

#[test]
fn per_order_search_requires_nonnegative_forward_factor_windows() {
    use ournotes_sim::error::Error;
    use ournotes_sim::live::skip::SkillEvent;
    for (effect_value, duration) in [(-20_000, 5.0), (1_000, -1.0)] {
        let (mut master, owned, mut request) = per_order_numeric_fixture("2.5");
        for row in &mut master.live_skill_effects {
            row.skill_condition_group = 0;
            row.effect_value = effect_value;
            row.activation_time_second = duration;
        }
        if let Objective::LiveScore { chart, play: PlayInput::Notes(play), .. } = &mut request.objective {
            chart.skill_events.push(SkillEvent { index: 0, time_ms: 500 });
            play.life_at_event = vec![1000; 5];
        }
        let pool = Pool::new(&master, &owned).unwrap();
        assert!(matches!(search(&pool, &request), Err(Error::Domain(_))));
    }
}
