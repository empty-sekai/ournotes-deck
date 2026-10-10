#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;
use common::{Rng, Synth, roster, short_chart, synth_snaps};
use ournotes_search::search::expectation::{self, FiniteSeedLaw, PhysicalDeck};
use ournotes_search::search::{self, Constraints, Objective, PlayInput, SearchRequest};
use ournotes_search::skip_event::search_skip_event_points;
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::pool::{Deck, Pool};
use ournotes_sim::scenario::*;
use serde_json::json;

fn fixture() -> Synth {
    let mut s = synth_snaps(&mut Rng::new(70), 5, 0, &[]);
    for (name, rows) in &mut s.tables {
        if name == "MasterMemberCard" {
            for (i, r) in rows.as_array_mut().unwrap().iter_mut().enumerate() {
                r["_characterID"] = json!(i + 1);
                r["_cardType"] = json!(4);
                r["_leaderSkillID"] = json!(4);
            }
        }
        if name == "MasterLiveMusic" {
            for r in rows.as_array_mut().unwrap() {
                r["_liveScoreRankGroup"] = json!(1);
            }
        }
    }
    s.tables.extend([
        ("MasterChallengeMusic".into(),json!([{"_id":70,"_eventId":7,"_liveMusicId":10,"_musicType":4},{"_id":71,"_eventId":7,"_liveMusicId":10,"_musicType":0}])),
        ("MasterArenaMusic".into(),json!([{"_id":80,"_liveMusicId":10,"_liveMusicType":5,"_gekisouMission1":3,"_gekisouMission2":2,"_gekisouMission3":1}])),
        ("MasterEvent".into(),json!([{"_id":7,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2,"_liveEventRewardGroup":37,"_challengeLiveEventRewardGroup":41}])),
        ("MasterEventEffect".into(),json!([{"_id":1,"_eventId":7,"_eventBonusType":2,"_resourceTypeConstraint":2,"_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,"_rank4EffectValue":1000,"_rank5EffectValue":1000}])),
        ("MasterLiveScoreRank".into(),json!([{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0}])),
        ("MasterLiveEventPoint".into(),json!([{"_id":1,"_group":1,"_scoreRank":2,"_value":100}])),
        ("MasterChallengeLiveEventPoint".into(),json!([{"_id":1,"_group":2,"_scoreRank":2,"_value":300}])),
        ("MasterLiveChallengePoint".into(),json!([{"_id":1,"_scoreRank":2,"_value":5}])),
    ]);
    s.tables.push((
        "MasterLiveEventReward".into(),
        json!([{"_id":5,"_eventGroup":37,"_scoreRank":2,"_resourceCount":3,"_resourceType":11,"_resourceId":9,"_probability":10000}]),
    ));
    s.tables.push((
        "MasterChallengeLiveEventReward".into(),
        json!([{"_id":5,"_eventGroup":41,"_scoreRank":2,"_resourceCount":4,"_resourceType":11,"_resourceId":9,"_probability":10000}]),
    ));
    s
}
fn input(skip: bool) -> ContextInput {
    serde_json::from_value(json!({"powerSnapshot":{"eventIds":[7],"capturedJstTicks":50},
        "resultClock":if skip {json!({"execution":"skip","serverNowJstTicks":100})} else {json!({"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100})},
        "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":7,"points":0,"challengePoints":30,"added":[]}],
            "eventWindows":[{"eventId":7,"startJstTicks":90,"endJstTicks":100}]}})).unwrap()
}
fn request(objective: Objective) -> SearchRequest {
    SearchRequest {
        objective,
        k: 2,
        constraints: Constraints { no_snaps: true, ..Default::default() },
        time_limit: None,
    }
}

#[test]
fn challenge_skip_uses_resolved_power_in_evaluate_search_and_oracle() {
    let s = fixture();
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let (chart, _) = short_chart(&mut Rng::new(3), 5, false);
    let context = input(true).resolve(&m, Scenario::Challenge(70), Some(1004), &[]).unwrap();
    let pool = context.pool(&m, &r).unwrap();
    let deck = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    let objective = Objective::SkipScore { score_id: 1004, chart: chart.clone() }.in_scenario(context.clone());
    let expected = pool.deck_power(&deck, Some(&context.resolved.power_music), true).unwrap().power();
    assert_eq!(search::evaluate(&pool, &deck, &objective).unwrap().0, expected);
    let free = input(true).resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    assert!(
        expected
            > search::evaluate(&pool, &deck, &Objective::SkipScore { score_id: 1004, chart }.in_scenario(free))
                .unwrap()
                .0
    );
    let req = request(objective);
    let out = search::search(&pool, &req).unwrap();
    let brute = search::oracle::brute_force(&pool, &req).unwrap();
    assert_eq!(out.results, brute.0);
    assert!(!out.results.is_empty());
}

#[test]
fn context_rejects_wrong_chart_bad_range_and_changed_precomputed_snapshot() {
    let s = fixture();
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    assert!(input(true).resolve(&m, Scenario::Challenge(70), Some(2003), &[]).is_err());
    assert!(input(true).resolve(&m, Scenario::Free(10), Some(1004), &[(20, 10)]).is_err());
    let ctx = input(true).resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    let mut pool = Pool::new(&m, &r).unwrap();
    pool.player.events = vec![7];
    assert!(ctx.validate_pool(&pool).is_err());
    assert!(
        serde_json::from_value::<ContextInput>(
            json!({"powerSnapshot":{"eventIds":[],"capturedJstTicks":"2026-09-29T00:00:00Z"}})
        )
        .is_err()
    );
}

#[test]
fn three_clocks_are_independent_and_windows_are_half_open() {
    let s = fixture();
    let m = s.master();
    let i = input(false);
    let ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    assert_eq!(ctx.power_snapshot_jst_ticks, Some(50));
    assert_eq!(ctx.result_clock.unwrap().jst_ticks(), 99);
    assert_eq!(ctx.event_request(&m, i.event_payoff.as_ref().unwrap(), 7).unwrap().holding_event_ids, vec![7]);
    let i = input(true);
    let ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    assert!(ctx.event_request(&m, i.event_payoff.as_ref().unwrap(), 7).unwrap().holding_event_ids.is_empty());
    let clock = ResultClockInput::Played { saved_start_jst_ticks: None, server_now_jst_ticks: 101 };
    assert_eq!(clock.resolve().unwrap().jst_ticks(), 101);
}

#[test]
fn finite_expectation_uses_context_and_no_ordinary_best_order_entry() {
    let s = fixture();
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let (chart, jt) = short_chart(&mut Rng::new(3), 4, false);
    let ctx = input(false).resolve(&m, Scenario::Challenge(71), Some(1004), &[]).unwrap();
    assert_eq!(ctx.resolved.power_music.music_type, 1);
    assert_eq!(ctx.resolved.skill_target_music_type, 0);
    let pool = ctx.pool(&m, &r).unwrap();
    let o = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types: jt },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    }
    .in_scenario(ctx);
    let p = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
    let c = expectation::context(&pool, &p, &o).unwrap();
    assert_eq!(c.params.skill_target_music_type, 0);
    let law = FiniteSeedLaw::new(vec![(42, 1), (-42, 2)]).unwrap();
    let e = expectation::evaluate_finite(&pool, &p, &o, &law, &0, |_, terminal, state| {
        assert_eq!(*state, 0);
        *state = 1;
        Ok(terminal.final_score as i128)
    })
    .unwrap();
    assert_eq!(e.expected_score, e.expected_payoff);
    assert_eq!(e.expected_score.denominator, 3);
    let req = request(o);
    assert!(search::search(&pool, &req).is_err());
    assert!(search::oracle::brute_force(&pool, &req).is_err());
    assert!(!expectation::oracle(&pool, &req, &law).unwrap().results.is_empty());
}

#[test]
fn event_payoff_uses_result_clock_and_preserves_initial_counters() {
    let s = fixture();
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let i = input(false);
    let ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    let pool = ctx.pool(&m, &r).unwrap();
    let deck = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    let event_input = i.event_payoff.as_ref().unwrap();
    let a = ctx.preview_event_points(&pool, &deck, event_input, 7, 100).unwrap();
    let b = ctx.preview_event_points(&pool, &deck, event_input, 7, 100).unwrap();
    assert_eq!(a.points, b.points);
    assert!(a.points_for(7) > 0);
    assert_eq!(event_input.local_events[0].points, 0);
    let i = input(true);
    let skip_ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    let chart = short_chart(&mut Rng::new(3), 4, false).0;
    let req = request(Objective::SkipScore { score_id: 1004, chart }.in_scenario(skip_ctx));
    let out = search_skip_event_points(&pool, &req, i.event_payoff.as_ref().unwrap(), 7).unwrap();
    assert_eq!(out.evaluated, 120);
    assert_eq!(out.results[0].event_points, Some(0));
    let arena = i.resolve(&m, Scenario::Arena(80), Some(1004), &[]).unwrap();
    assert!(arena.event_request(&m, i.event_payoff.as_ref().unwrap(), 7).is_err());
}

#[test]
fn cli_selects_special_song_and_native_expectation_end_to_end() {
    let mut s = fixture();
    for (name, rows) in &mut s.tables {
        if name == "MasterLiveMusicScore" {
            for row in rows.as_array_mut().unwrap() {
                if row["_id"] == 1004 {
                    row["_fullComboCount"] = json!(2);
                }
            }
        }
    }
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let dir = std::env::temp_dir().join(format!("scenario-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut master = serde_json::Map::new();
    for (name, values) in &s.tables {
        let mut columns = std::collections::BTreeSet::new();
        for row in values.as_array().unwrap() {
            columns.extend(row.as_object().unwrap().keys().cloned());
        }
        let columns: Vec<_> = columns.into_iter().collect();
        let rows: Vec<Vec<_>> = values
            .as_array()
            .unwrap()
            .iter()
            .map(|r| columns.iter().map(|k| r.get(k).cloned().unwrap_or(serde_json::Value::Null)).collect())
            .collect();
        master.insert(name.clone(), json!({"columns":columns,"rows":rows}));
    }
    common::every_table(&mut master);
    let data = json!({"format":"nnnotes.deck-data/1","master":master,"charts":[{"scoreId":1004,"asset":{"key":"synthetic","sha256":"synthetic-not-real"},
        "notes":{"id":[1,2],"op":[1,1],"judgementType":[1,1],"timeMs":[100,200]},"skillEvents":{"timeMs":[0,40,80,120,160]},"fevers":{"startMs":[],"endMs":[]}}]});
    let members:Vec<_>=r.members.iter().map(|m|json!({"id":m.id,"level":m.level,"awake":m.awake,"rank":m.rank,"liveSkillLevel":m.live_skill_level,"gekisouSkillLevel":1})).collect();
    let datafile = dir.join("data.json");
    let rosterfile = dir.join("roster.json");
    let lawfile = dir.join("law.json");
    std::fs::write(&datafile, data.to_string()).unwrap();
    std::fs::write(&rosterfile, json!({"members":members,"snaps":[]}).to_string()).unwrap();
    std::fs::write(&lawfile, "[[42,1],[-42,1]]").unwrap();
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_ournotes-deck"))
            .args(extra)
            .arg("--data")
            .arg(&datafile)
            .arg("--roster")
            .arg(&rosterfile)
            .output()
            .unwrap()
    };
    let o = run(&["skip", "--scenario", "challenge", "--scenario-music", "70", "--score", "1004", "-k", "1"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(out["resolvedContext"]["powerMusic"]["musicType"], 4);
    assert_eq!(out["resolvedContext"]["calcEventParameter"], true);
    let o = run(&[
        "live",
        "--scenario",
        "arena",
        "--scenario-music",
        "80",
        "--score",
        "1004",
        "--expectation",
        "finite",
        "--seed-law",
        lawfile.to_str().unwrap(),
        "-k",
        "1",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(out["search"]["evaluated"], 120);
    assert_eq!(out["resolvedContext"]["gekisouMissions"], json!([3, 2, 1]));
    assert_eq!(out["resolvedContext"]["powerMusic"]["musicType"], 1);
    assert!(!run(&["live", "--score", "1004"]).status.success());
    assert!(!run(&["skip", "--scenario", "arena", "--scenario-music", "80", "--score", "1004"]).status.success());
    let contextfile = dir.join("context.json");
    std::fs::write(&contextfile, serde_json::to_string(&input(false)).unwrap()).unwrap();
    let o = run(&[
        "live",
        "--scenario",
        "free",
        "--scenario-music",
        "10",
        "--score",
        "1004",
        "--expectation",
        "finite",
        "--seed-law",
        lawfile.to_str().unwrap(),
        "--objective",
        "client-event-points",
        "--event-id",
        "7",
        "--context",
        contextfile.to_str().unwrap(),
        "-k",
        "1",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(out["search"]["results"][0]["evaluation"]["expected_payoff"]["numerator"], 200);
    assert_eq!(out["clientCounterPreviews"][0][0]["points"], json!([[7, 100]]));
    let ranked = input(false);
    std::fs::write(&contextfile, serde_json::to_string(&ranked).unwrap()).unwrap();
    let o = run(&[
        "live",
        "--scenario",
        "free",
        "--scenario-music",
        "10",
        "--score",
        "1004",
        "--expectation",
        "finite",
        "--seed-law",
        lawfile.to_str().unwrap(),
        "--objective",
        "ranked-event-items",
        "--event-id",
        "7",
        "--resource-type",
        "11",
        "--resource-id",
        "9",
        "--context",
        contextfile.to_str().unwrap(),
        "-k",
        "1",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(out["search"]["results"][0]["evaluation"]["expected_payoff"]["numerator"], 6);
    assert_eq!(out["rankedItemPreviews"][0][0]["rewards"][0]["amount"], 3);
    std::fs::write(&lawfile, "[[42,18446744073709551615],[-42,18446744073709551615]]").unwrap();
    let o = run(&[
        "live",
        "--score",
        "1004",
        "--expectation",
        "finite",
        "--seed-law",
        lawfile.to_str().unwrap(),
        "-k",
        "1",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        out["search"]["results"][0]["evaluation"]["expected_score"]["denominator"].to_string(),
        "36893488147419103230"
    );
    let skip_items = input(true);
    std::fs::write(&contextfile, serde_json::to_string(&skip_items).unwrap()).unwrap();
    let o = run(&[
        "skip",
        "--score",
        "1004",
        "--objective",
        "ranked-event-items",
        "--event-id",
        "7",
        "--resource-type",
        "11",
        "--resource-id",
        "9",
        "--context",
        contextfile.to_str().unwrap(),
        "-k",
        "1",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(out["search"]["results"][0]["terminalPayoff"], 0);
    assert!(out["search"]["results"][0]["eventPoints"].is_null());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn arena_resolved_missions_reach_default_just_rule_and_full_setup() {
    use ournotes_search::search::{GekisouObjective, SeedSet};
    use ournotes_sim::live::model::JustRule;
    let s = fixture();
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let (chart, jt) = short_chart(&mut Rng::new(3), 4, false);
    let fevers = vec![(0, 500)];
    let ctx = input(false).resolve(&m, Scenario::Arena(80), Some(1004), &fevers).unwrap();
    let rule = JustRule::new(&m, &ctx.gekisou).unwrap();
    assert!(!rule.windows(&[0, 100, 500, 600]).is_empty());
    let stream = JudgementStream::theoretical_best_gekisou(&chart, &jt, &rule).unwrap();
    let pool = ctx.pool(&m, &r).unwrap();
    let o = Objective::LiveScore {
        score_id: 1004,
        chart,
        play: PlayInput::Stream { stream, judgement_types: jt },
        event: false,
        exclude_snap_skills: false,
        gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers }),
    }
    .in_scenario(ctx);
    let resolved =
        expectation::context(&pool, &PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] }, &o).unwrap();
    assert_eq!(resolved.gekisou.unwrap().missions, vec![3, 2, 1]);
    assert_eq!(resolved.params.skill_target_music_type, 1);
}

#[test]
fn event_payoff_is_applied_before_expectation_and_type2_changes_only_power() {
    let mut s = fixture();
    for (name, rows) in &mut s.tables {
        match name.as_str() {
            "MasterLiveScoreRank" => {
                rows.as_array_mut().unwrap().push(json!({"_id":2,"_group":1,"_liveScoreRank":3,"_requiredScore":100}))
            }
            "MasterLiveEventPoint" => {
                rows.as_array_mut().unwrap().push(json!({"_id":2,"_group":1,"_scoreRank":3,"_value":1000}))
            }
            "MasterLiveChallengePoint" => rows.as_array_mut().unwrap().push(json!({"_id":2,"_scoreRank":3,"_value":5})),
            _ => {}
        }
    }
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let i = input(false);
    let ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    let pool = ctx.pool(&m, &r).unwrap();
    let deck = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    let input = i.event_payoff.unwrap();
    let points = |score| ctx.preview_event_points(&pool, &deck, &input, 7, score).unwrap().points_for(7);
    let aggregate = expectation::aggregate(
        [99, 101]
            .into_iter()
            .map(|score| expectation::SeedOutcome {
                root_seed: score,
                weight: 1,
                performance_order: [0, 1, 2, 3, 4],
                final_score: score,
                terminal_payoff: points(score) as i128,
            })
            .collect(),
    )
    .unwrap();
    assert_eq!(aggregate.expected_score.numerator, 200);
    assert_eq!(aggregate.expected_payoff.numerator, 1100);
    assert_ne!(aggregate.expected_payoff.numerator / 2, points(100) as i128);
    assert_eq!(aggregate.payoff_mass.len(), 2);
}

#[test]
fn multiplayer_result_panel_checks_sum_and_counts_disconnections_separately() {
    let panel = MultiplayerResultPanelInput {
        local_player_index: 0,
        local_disconnected: false,
        other_players: vec![
            PeerResultInput { final_score: 200, disconnected: true },
            PeerResultInput { final_score: 300, disconnected: false },
        ],
    };
    assert_eq!(panel.total_and_count(100).unwrap(), (600, 2));
    assert!(panel.total_and_count(i32::MAX).is_err());
}

#[test]
fn ranked_items_project_without_balances_and_master_dates_need_named_adapter() {
    let s = fixture();
    let mut m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let mut i = input(false);
    let ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    let pool = ctx.pool(&m, &r).unwrap();
    let deck = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    let input = i.event_payoff.as_mut().unwrap();
    input.local_events.clear();
    let items = ctx.preview_event_items(&pool, &deck, input, 7, 100).unwrap();
    assert_eq!(item_payoff(&items, 7, 11, 9).unwrap(), 3);
    input.event_windows = None;
    assert!(input.resolve_event_windows(&m).is_err());
    input.event_window_adapter = Some("canonical-master-no-offset".into());
    assert_eq!(input.resolve_event_windows(&m).unwrap()[0].start_jst_ticks, 0);
    drop(pool);
    m.events[0].start_at = Some("2026-09-29T00:00:00Z".into());
    assert!(input.resolve_event_windows(&m).is_err());
    m.events[0].start_at = Some("2026-09-29 00:00:00".into());
    assert!(input.resolve_event_windows(&m).is_ok());
}

#[test]
fn event_effect_types_other_than_two_never_change_scenario_power() {
    let s = fixture();
    let m = s.master();
    let roster = roster(&mut Rng::new(2), &m);
    let measure = |m: &ournotes_sim::master::Master| {
        let ctx = input(true).resolve(m, Scenario::Challenge(70), Some(1004), &[]).unwrap();
        let pool = ctx.pool(m, &roster).unwrap();
        let d = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
        search::evaluate(&pool, &d, &Objective::Power { music_id: None, event: false }.in_scenario(ctx)).unwrap().0
    };
    let with_type2 = measure(&m);
    let mut no_effects = m.clone();
    no_effects.event_effects.clear();
    let base = measure(&no_effects);
    assert!(with_type2 > base);
    for kind in [0, 1, 3, 4, 5] {
        let mut alternate = m.clone();
        alternate.event_effects[0].event_bonus_type = kind;
        assert_eq!(measure(&alternate), base);
    }
}

#[test]
fn exact_finite_mass_json_preserves_values_larger_than_u64() {
    let atom = expectation::SeedOutcome {
        root_seed: 0,
        weight: u64::MAX,
        performance_order: [0, 1, 2, 3, 4],
        final_score: 1,
        terminal_payoff: 1,
    };
    let value = expectation::aggregate(vec![atom.clone(), atom]).unwrap();
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["expected_payoff"]["denominator"].to_string(), "36893488147419103230");
    assert_eq!(json["expected_payoff"]["numerator"].to_string(), "36893488147419103230");
}

#[test]
fn exact_payoff_json_supports_signed_i128_edges() {
    for payoff in [i128::MIN, i128::MAX] {
        let atom = expectation::SeedOutcome {
            root_seed: 0,
            weight: 1,
            performance_order: [0, 1, 2, 3, 4],
            final_score: 0,
            terminal_payoff: payoff,
        };
        let value = serde_json::to_value(expectation::aggregate(vec![atom]).unwrap()).unwrap();
        assert_eq!(value["expected_payoff"]["numerator"].to_string(), payoff.to_string());
    }
}

#[test]
fn event_oracle_can_choose_lower_power_deck_with_higher_event_bonus() {
    let mut s = fixture();
    for (name, rows) in &mut s.tables {
        if name == "MasterMemberCard" {
            let mut weak = rows[0].clone();
            weak["_id"] = json!(6);
            for key in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                weak[key] = json!(100);
            }
            rows.as_array_mut().unwrap().push(weak);
        }
        if name == "MasterEventEffect" {
            rows.as_array_mut().unwrap().push(json!({"_id":2,"_eventId":7,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":6,
                "_rank1EffectValue":5000,"_rank2EffectValue":5000,"_rank3EffectValue":5000,"_rank4EffectValue":5000,"_rank5EffectValue":5000}));
        }
    }
    let m = s.master();
    let r = roster(&mut Rng::new(2), &m);
    let mut i = input(true);
    i.result_clock = Some(ResultClockInput::Skip { server_now_jst_ticks: 99 });
    let ctx = i.resolve(&m, Scenario::Free(10), Some(1004), &[]).unwrap();
    let pool = ctx.pool(&m, &r).unwrap();
    let chart = short_chart(&mut Rng::new(3), 4, false).0;
    let points = search_skip_event_points(
        &pool,
        &request(Objective::SkipScore { score_id: 1004, chart }.in_scenario(ctx.clone())),
        i.event_payoff.as_ref().unwrap(),
        7,
    )
    .unwrap();
    let power =
        search::search(&pool, &request(Objective::Power { music_id: None, event: false }.in_scenario(ctx))).unwrap();
    assert!(points.results[0].members.contains(&6));
    assert!(!points.results[0].members.contains(&1));
    assert_eq!(points.results[0].event_points, Some(150));
    assert!(points.results[0].power < power.results[0].power);
}

#[test]
fn ranked_items_use_each_terminal_grade_before_expectation_without_point_rows() {
    let mut master = fixture().master();
    let mut rank = master.live_score_ranks[0].clone();
    rank.id = 2;
    rank.live_score_rank = 3;
    rank.required_score = 1000;
    rank.battle_live_required_score = 1000;
    master.live_score_ranks.push(rank);
    for (rows, count) in [(&mut master.live_event_rewards, 11), (&mut master.challenge_live_event_rewards, 17)] {
        let mut higher = rows[0].clone();
        higher.id = 6;
        higher.score_rank = 3;
        higher.resource_count = count;
        rows.push(higher);
    }
    master.live_event_points.clear();
    master.challenge_live_event_points.clear();
    master.live_challenge_points.clear();
    let roster = roster(&mut Rng::new(2), &master);
    let deck = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    for (scene, low, high) in [(Scenario::Free(10), 3, 11), (Scenario::Challenge(70), 4, 17)] {
        let mut request = input(false);
        request.event_payoff.as_mut().unwrap().local_events.clear();
        let context = request.resolve(&master, scene, Some(1004), &[]).unwrap();
        let pool = context.pool(&master, &roster).unwrap();
        let payoff = |score| {
            let preview =
                context.preview_event_items(&pool, &deck, request.event_payoff.as_ref().unwrap(), 7, score).unwrap();
            assert_eq!(preview.rewards.len(), 1);
            item_payoff(&preview, 7, 11, 9).unwrap()
        };
        assert_eq!(payoff(999), low);
        assert_eq!(payoff(1000), high);
        assert_eq!(payoff(1001), high);
        assert_ne!(payoff(999) + payoff(1001), 2 * payoff(1000));
    }
    for scene in [Scenario::Battle(10), Scenario::Arena(80)] {
        let mut request = input(false);
        let input = request.event_payoff.as_mut().unwrap();
        input.local_events.clear();
        input.multiplayer_score_policy = Some(MultiplayerScorePolicy::SameScore { players: 3 });
        let context = request.resolve(&master, scene, Some(1004), &[]).unwrap();
        let pool = context.pool(&master, &roster).unwrap();
        for (score, expected) in [(1290, 3), (1291, 11)] {
            let preview =
                context.preview_event_items(&pool, &deck, request.event_payoff.as_ref().unwrap(), 7, score).unwrap();
            assert_eq!(item_payoff(&preview, 7, 11, 9).unwrap(), expected);
        }
    }
}

#[test]
fn ranked_skip_items_use_fixed_grade_and_result_time() {
    let mut master = fixture().master();
    master.parameters.iter_mut().find(|row| row.id == "live_skip_result_score_rank").unwrap().value = "D".into();
    let roster = roster(&mut Rng::new(2), &master);
    let deck = Deck { members: [0, 1, 2, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    for (scene, count) in [(Scenario::Free(10), 3), (Scenario::Challenge(70), 4)] {
        for (now, expected) in [(99, count), (100, 0)] {
            let mut request = input(true);
            request.result_clock = Some(ResultClockInput::Skip { server_now_jst_ticks: now });
            request.event_payoff.as_mut().unwrap().local_events.clear();
            let context = request.resolve(&master, scene, Some(1004), &[]).unwrap();
            let pool = context.pool(&master, &roster).unwrap();
            for score in [0, 999_999] {
                let preview = context
                    .preview_event_items(&pool, &deck, request.event_payoff.as_ref().unwrap(), 7, score)
                    .unwrap();
                assert_eq!(item_payoff(&preview, 7, 11, 9).unwrap(), expected);
            }
        }
    }
}
