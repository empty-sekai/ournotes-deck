//! The live objective with snap skills against exhaustive enumeration on small synthetic pools: every member set,
//! leader, snap placement and performance order simulated.

mod common;

use common::{Rng, random_stream, roster, short_chart, synth_snaps};
use ournotes_deck::Error;
use ournotes_deck::live::model::{JudgementStream, Play};
use ournotes_deck::search::oracle::brute_force;
use ournotes_deck::search::{Completion, Constraints, Objective, PlayInput, Pool, SearchRequest, evaluate, search};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn cases() -> u64 {
    env("OURNOTES_DECK_SNAPS_CASES", 3)
}

fn seed0() -> u64 {
    env("OURNOTES_DECK_SNAPS_SEED0", 0)
}

fn size() -> (i64, i64) {
    (env("OURNOTES_DECK_SNAPS_MEMBERS", 6) as i64, env("OURNOTES_DECK_SNAPS_SNAPS", 2) as i64)
}

fn notes() -> u64 {
    env("OURNOTES_DECK_SNAPS_NOTES", 24)
}

/// Compares the search with the oracle for K = 1, 3, 10, 1000 and re-evaluates every result; returns (rows
/// compared, decks enumerated).
fn check(pool: &Pool, objective: &Objective, constraints: &Constraints, label: &str) -> (usize, u64) {
    let all = SearchRequest {
        objective: objective.clone(),
        k: usize::MAX,
        constraints: constraints.clone(),
        time_limit: None,
    };
    let (want, evaluated) = brute_force(pool, &all).unwrap_or_else(|e| panic!("{label}: oracle failed: {e}"));
    let mut compared = 0;
    for k in [1usize, 3, 10, 1000] {
        let req = SearchRequest { objective: objective.clone(), k, constraints: constraints.clone(), time_limit: None };
        let got = search(pool, &req).unwrap_or_else(|e| panic!("{label}: search failed: {e}"));
        assert_eq!(got.completion, Completion::Complete);
        let w = &want[..k.min(want.len())];
        assert_eq!(got.results, w, "{label} k {k} {constraints:?}");
        for r in &got.results {
            let deck = pool.deck(r.members, r.snaps, r.performance_order).unwrap();
            assert_eq!(evaluate(pool, &deck, objective).unwrap(), (r.power, r.score), "{label}: evaluate disagrees");
        }
        compared += w.len();
    }
    (compared, evaluated)
}

fn variants(rng: &mut Rng, nm: i64, ns: i64) -> Vec<Constraints> {
    let all = vec![
        Constraints::default(),
        Constraints { leader: Some(rng.range(1, nm)), ..Default::default() },
        Constraints {
            include_members: vec![rng.range(1, nm)],
            exclude_snaps: vec![rng.range(1, ns)],
            ..Default::default()
        },
        Constraints { exclude_members: vec![rng.range(1, nm)], ..Default::default() },
        Constraints { no_snaps: true, ..Default::default() },
    ];
    let n = env("OURNOTES_DECK_SNAPS_VARIANTS", 2) as usize;
    all.into_iter().take(n.max(1)).collect()
}

fn run(stream_kind: u64, kinds: &[i64], tag: u64) {
    let (nm, ns) = size();
    let mut total = (0usize, 0u64);
    for seed in seed0()..seed0() + cases() {
        let mut rng = Rng::new(seed ^ tag);
        let s = synth_snaps(&mut rng, nm, ns, kinds);
        let master = s.master();
        let r = roster(&mut rng, &master);
        let pool = Pool::new(&master, &r).unwrap();
        let n = notes() as usize / 2 + rng.below(notes()) as usize;
        let repeat = rng.chance(0.2);
        let (chart, judgement_types) = short_chart(&mut rng, n, repeat);
        let stream = match stream_kind {
            0 => JudgementStream::theoretical_best(&chart),
            _ => {
                let fps = [30, 60][rng.below(2) as usize];
                let mut s = random_stream(&mut rng, &chart, fps);
                if stream_kind == 2 {
                    // half of the notes missed: the life runs out
                    for r in s.judged.iter_mut() {
                        if rng.chance(0.5) {
                            r[2] = 1;
                        }
                    }
                }
                if stream_kind == 3 {
                    // many misses, and notes judged up to 3 frames early or up to 5 frames late
                    let last = s.frames.len() as i64 - 1;
                    for r in s.judged.iter_mut() {
                        if rng.chance(0.35) {
                            r[2] = [1, 2][rng.below(2) as usize];
                        }
                        if rng.chance(0.3) {
                            r[0] = (r[0] as i64 + rng.range(-3, 5)).clamp(0, last) as i32;
                        }
                    }
                    s.judged.sort_by_key(|r| r[0]);
                }
                s
            }
        };
        let score_id = [1004, 2003, 3001][rng.below(3) as usize];
        let objective = Objective::LiveScore {
            score_id,
            chart,
            play: PlayInput::Stream { stream, judgement_types },
            event: false,
            exclude_snap_skills: false,
        };
        let t0 = std::time::Instant::now();
        let mut case = (0usize, 0u64);
        for c in variants(&mut rng, nm, ns) {
            let x = check(&pool, &objective, &c, &format!("seed {seed} stream {stream_kind}"));
            case.0 += x.0;
            case.1 += x.1;
        }
        eprintln!(
            "seed {seed}: {} rows compared, {} deck-orders simulated, {:.1} s",
            case.0,
            case.1,
            t0.elapsed().as_secs_f64()
        );
        total.0 += case.0;
        total.1 += case.1;
    }
    eprintln!(
        "snaps oracle (stream {stream_kind}, kinds {kinds:?}): {} rows compared, {} deck-orders simulated",
        total.0, total.1
    );
}

#[test]
fn theoretical_best_matches_oracle() {
    run(0, &[], 0x5a95);
}

#[test]
fn random_stream_matches_oracle() {
    run(1, &[], 0x77e1);
}

#[test]
fn extensions_and_score_ups_match_oracle() {
    run(0, &[1, 2, 3, 4, 11], 0x0e47);
}

#[test]
fn life_and_conversions_match_oracle() {
    run(1, &[5, 6, 8, 1], 0x11fe);
}

/// Streams that empty the life, with snaps that recover life or guard and with snaps that do neither.
#[test]
fn life_zero_matches_oracle() {
    run(2, &[], 0x11f0);
    run(2, &[1, 3, 4, 6, 9, 11], 0x11f1);
}

/// Life recovery at skill events with the life running out, under early and late judgements (the life frame cache
/// folds some frames twice), with and without guards.
#[test]
fn recovery_with_early_and_late_judgements_matches_oracle() {
    run(3, &[5, 1, 3, 11], 0x2ec0);
    run(3, &[5, 8, 6, 4], 0x9d17);
}

#[test]
fn counted_and_random_triggers_match_oracle() {
    run(1, &[9, 10, 7, 2], 0xc0de);
}

/// The snap skills change the best decks: the oracle's best score differs from that of the same pool whose snaps
/// have no support skills.
#[test]
fn snap_skills_change_the_ranking() {
    let mut differ = 0;
    for seed in 0..6u64 {
        let mut rng = Rng::new(seed ^ 0xd1ff);
        let s = synth_snaps(&mut rng, 6, 2, &[1, 2, 3, 11]);
        let master = s.master();
        let r = roster(&mut rng, &master);
        let (chart, judgement_types) = short_chart(&mut rng, 20, false);
        let stream = JudgementStream::theoretical_best(&chart);
        let objective = Objective::LiveScore {
            score_id: 1004,
            chart,
            play: PlayInput::Stream { stream, judgement_types },
            event: false,
            exclude_snap_skills: false,
        };
        let mut bare = s.master();
        for c in bare.support_cards.iter_mut() {
            c.support_skill_id_01 = 0;
            c.support_skill_id_02 = 0;
        }
        bare.reindex().unwrap();
        let req = SearchRequest { objective, k: 1, constraints: Constraints::default(), time_limit: None };
        let a = search(&Pool::new(&master, &r).unwrap(), &req).unwrap();
        let b = search(&Pool::new(&bare, &r).unwrap(), &req).unwrap();
        if a.results.first().map(|x| x.score) != b.results.first().map(|x| x.score) {
            differ += 1;
        }
    }
    assert!(differ > 0, "snap skills never changed the best score");
}

#[test]
fn play_kind_must_match_the_objective() {
    let mut rng = Rng::new(3);
    let s = synth_snaps(&mut rng, 6, 2, &[]);
    let master = s.master();
    let r = roster(&mut rng, &master);
    let pool = Pool::new(&master, &r).unwrap();
    let (chart, judgement_types) = short_chart(&mut rng, 10, false);
    let stream = JudgementStream::theoretical_best(&chart);
    let notes = Play { notes: Vec::new(), life_at_event: vec![1000; 5], assist: false };
    for (play, exclude) in [
        (PlayInput::Notes(notes), false),
        (PlayInput::Stream { stream: stream.clone(), judgement_types: judgement_types.clone() }, true),
        (PlayInput::Stream { stream, judgement_types: vec![1] }, false),
    ] {
        let objective = Objective::LiveScore {
            score_id: 1004,
            chart: chart.clone(),
            play,
            event: false,
            exclude_snap_skills: exclude,
        };
        let req = SearchRequest { objective, k: 3, constraints: Constraints::default(), time_limit: None };
        assert!(matches!(search(&pool, &req), Err(Error::Input(_))));
    }
}

#[test]
fn time_limit_returns_legal_decks() {
    let mut rng = Rng::new(11);
    let s = synth_snaps(&mut rng, 14, 6, &[]);
    let master = s.master();
    let r = roster(&mut rng, &master);
    let pool = Pool::new(&master, &r).unwrap();
    let (chart, judgement_types) = short_chart(&mut rng, 40, false);
    let stream = JudgementStream::theoretical_best(&chart);
    let objective = Objective::LiveScore {
        score_id: 1004,
        chart,
        play: PlayInput::Stream { stream, judgement_types },
        event: false,
        exclude_snap_skills: false,
    };
    let req = SearchRequest {
        objective: objective.clone(),
        k: 5,
        constraints: Constraints::default(),
        time_limit: Some(std::time::Duration::from_millis(0)),
    };
    let out = search(&pool, &req).unwrap();
    for r in &out.results {
        let deck = pool.deck(r.members, r.snaps, r.performance_order).unwrap();
        assert_eq!(evaluate(&pool, &deck, &objective).unwrap(), (r.power, r.score));
    }
}

#[test]
fn stream_json_round_trip() {
    let text = r#"{"frames": [0, 16, 33], "judged": [[1, 7, 5, 16], [2, 8, 4, 30]], "baseSeed": 9}"#;
    let s: JudgementStream = serde_json::from_str(text).unwrap();
    assert_eq!(s.frames, vec![0, 16, 33]);
    assert_eq!(s.judged, vec![[1, 7, 5, 16], [2, 8, 4, 30]]);
    assert_eq!((s.base_seed, s.assist), (9, false));
    let back: JudgementStream = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    let play = s.to_live_play().unwrap();
    assert_eq!(play.frames[1].judged.len(), 1);
    let bad = JudgementStream { frames: vec![5, 3], judged: vec![], base_seed: 0, assist: false, delta_times: None };
    assert!(matches!(bad.to_live_play(), Err(Error::Input(_))));
    let bad =
        JudgementStream { frames: vec![0], judged: vec![[1, 1, 5, 0]], base_seed: 0, assist: false, delta_times: None };
    assert!(matches!(bad.to_live_play(), Err(Error::Input(_))));
}
