//! Synthetic regressions for search input, arithmetic and completion contracts.

#[path = "../../../ournotes-sim/tests/common/mod.rs"]
pub(super) mod common;

use super::*;
use common::{Rng, Synth, extend_table, roster, set_column, synth};
use ournotes_sim::live::skip::ChartNote;
use serde_json::json;

fn distinct(n: i64, snaps: i64) -> Synth {
    let mut data = synth(&mut Rng::new(735), n, snaps);
    let mut character = 0;
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        character += 1;
        row["_characterID"] = json!(character);
    });
    data
}

fn request(k: usize) -> SearchRequest {
    SearchRequest {
        objective: Objective::Power { music_id: None, event: false },
        k,
        constraints: Constraints { no_snaps: true, ..Default::default() },
        time_limit: None,
    }
}

fn set_adjustment(data: &mut Synth, adjustment: &str) {
    set_column(data, "MasterLiveSettings", &mut |row| {
        if row["_key"] == "note_score_adjustment_factor" {
            row["_value"] = json!(adjustment);
        }
    });
}

fn one_note(master: &Master) -> Chart {
    Chart::from_notes(
        vec![ChartNote { id: 1, time_ms: 100, note_type: 1 }],
        vec![],
        &LiveScoreSettings::from_master(master).unwrap(),
    )
    .unwrap()
}

fn single_note_live_request(master: &Master) -> SearchRequest {
    let mut req = request(1);
    req.constraints.no_snaps = false;
    req.constraints.leader = Some(1);
    req.objective = Objective::LiveScore {
        score_id: 1004,
        chart: one_note(master),
        play: PlayInput::Notes(Play {
            notes: vec![ournotes_sim::live::skill::NotePlay {
                note_id: 1,
                time_ms: 100,
                note_type: 1,
                score_type: ournotes_sim::live::score::PERFECT,
                life: 1000,
                combo: 0,
            }],
            life_at_event: vec![],
            assist: false,
        }),
        event: false,
        exclude_snap_skills: true,
        gekisou: None,
    };
    req
}

#[test]
fn zero_budget_never_prepares_or_visits_a_deck() {
    let master = distinct(5, 0).master();
    let owned = roster(&mut Rng::new(739), &master);
    let pool = Pool::new(&master, &owned).unwrap();
    for objective in [request(1).objective, Objective::SkipScore { score_id: 1004, chart: one_note(&master) }] {
        let mut req = request(1);
        req.objective = objective;
        req.time_limit = Some(Duration::ZERO);
        for run in [search, search_best_order_diagnostic] {
            let result = run(&pool, &req).unwrap();
            assert_eq!(result.completion, Completion::TimedOut);
            assert!(result.results.is_empty());
            assert_eq!(result.stats, PowerStats::default());
            assert_eq!(result.verify_elapsed, Duration::ZERO);
        }
    }
}

#[test]
fn zero_k_is_an_input_error_in_feasible_and_infeasible_public_calls() {
    for members in [3, 5] {
        let master = distinct(members, 0).master();
        let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
        for limit in [None, Some(Duration::ZERO)] {
            let mut req = request(0);
            req.time_limit = limit;
            for run in [search, search_best_order_diagnostic] {
                assert!(matches!(run(&pool, &req), Err(Error::Input(_))));
            }
        }
    }
}

#[test]
fn huge_k_is_a_limit_without_eager_allocation() {
    let master = distinct(5, 0).master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let expected = search(&pool, &request(1)).unwrap();
    for k in [usize::MAX, usize::MAX - 1] {
        let actual = search(&pool, &request(k)).unwrap();
        assert_eq!(actual.completion, Completion::Complete);
        assert_eq!(actual.results, expected.results);
    }
}

#[test]
fn unrepresentable_deadline_is_an_input_error() {
    let master = distinct(5, 0).master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let mut req = request(1);
    req.time_limit = Some(Duration::MAX);
    assert!(matches!(search(&pool, &req), Err(Error::Input(_))));
}

#[test]
fn minimum_signed_leader_value_is_outside_the_proven_domain() {
    let mut data = distinct(5, 0);
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        row["_leaderSkillID"] = json!(4);
    });
    set_column(&mut data, "MasterLeaderSkillEffect", &mut |row| {
        if row["_leaderSkillID"] == 4 {
            row["_effectValue"] = json!(i64::MIN);
        }
    });
    let master = data.master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    assert!(matches!(search(&pool, &request(1)), Err(Error::Domain(_))));
}

#[test]
fn live_search_requires_nonnegative_score_multipliers() {
    let mut data = distinct(5, 1);
    set_adjustment(&mut data, "-2.5");
    let master = data.master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let req = single_note_live_request(&master);
    let (oracle, count) = oracle::brute_force_best_order_diagnostic(&pool, &req).unwrap();
    assert_eq!(count, 720);
    assert_eq!(oracle[0].snaps, [None; 5]);
    let actual = search_best_order_diagnostic(&pool, &req);
    assert!(matches!(actual, Err(Error::Domain(_))), "oracle={oracle:?}, actual={actual:?}");
}

#[test]
fn live_search_certifies_finite_intermediates_across_all_snap_powers() {
    let mut data = distinct(5, 1);
    set_column(&mut data, "MasterLiveMusicScore", &mut |row| {
        if row["_id"] == 1004 {
            row["_musicScoreLevel"] = json!(5);
        }
    });
    let master = data.master();
    let owned = roster(&mut Rng::new(739), &master);
    let pool = Pool::new(&master, &owned).unwrap();
    let req = single_note_live_request(&master);
    let (song, _, _) = objective_song(&pool, &req.objective).unwrap();
    let mut deck = Deck { members: [1, 2, 0, 3, 4], snaps: [None; 5], performance_order: [0, 1, 2, 3, 4] };
    let low = pool.deck_power(&deck, song.as_ref(), false).unwrap().power();
    let high = (0..5)
        .map(|slot| {
            deck.snaps = [None; 5];
            deck.snaps[slot] = Some(0);
            pool.deck_power(&deck, song.as_ref(), false).unwrap().power()
        })
        .max()
        .unwrap();
    assert!(high > low);
    let adjustment = f32::MAX / ((low as f32 + high as f32) / 2.0);
    set_adjustment(&mut data, &format!("{adjustment:e}"));
    let master = data.master();
    let pool = Pool::new(&master, &owned).unwrap();
    let req = single_note_live_request(&master);
    let (oracle, count) = oracle::brute_force_best_order_diagnostic(&pool, &req).unwrap();
    assert_eq!(count, 720);
    assert_eq!(oracle[0].score, Some(i32::MAX));
    assert!(oracle[0].power < high);
    assert!(matches!(search_best_order_diagnostic(&pool, &req), Err(Error::Domain(_))));
}

#[test]
fn power_search_certifies_every_feasible_deck_before_pruning() {
    let mut data = distinct(6, 0);
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        let id = row["_id"].as_i64().unwrap();
        row["_characterID"] = json!(id.min(5));
        row["_leaderSkillID"] = json!(if id == 1 { 1 } else { 0 });
        row["_cardType"] = json!(if id == 6 { 2 } else { 1 });
        let points = match id {
            5 => 101_000_000,
            6 => 100_000_000,
            _ => 1000,
        };
        for stat in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
            row[stat] = json!(points);
        }
    });
    set_column(&mut data, "MasterMemberCardLevel", &mut |row| {
        for stat in ["_performanceRate", "_technicRate", "_visualRate"] {
            row[stat] = json!(10_000);
        }
    });
    extend_table(&mut data, "MasterSkillTarget", vec![json!({"_id":14,"_skillTargetType":3,"_characterID":5})]);
    common::replace_table(
        &mut data,
        "MasterSkillCondition",
        json!([{"_id":1,"_conditionType":3000,"_conditionValues":[],"_isPositive":true,"_conditionTargetIDs":[5]}]),
    );
    common::replace_table(&mut data, "MasterSkillConditionSet", json!([{"_id":1,"_group":1,"_conditionIds":[1]}]));
    common::replace_table(
        &mut data,
        "MasterLeaderSkillEffect",
        json!([{
            "_id":1,"_leaderSkillID":1,"_level":1,"_skillConditionGroup":1,
            "_skillTargetIDs":[14],"_skillEffectType":1000,"_effectValue":-100_000,
            "_skillCumulativeConditionID":0
        }]),
    );
    let master = data.master();
    let mut owned = roster(&mut Rng::new(739), &master);
    owned.player = Default::default();
    for member in &mut owned.members {
        member.awake = 1;
        member.rank = 1;
    }
    let pool = Pool::new(&master, &owned).unwrap();
    let mut req = request(1);
    req.constraints.leader = Some(1);
    req.constraints.include_members = vec![1, 2, 3, 4];
    let (oracle, count) = oracle::brute_force(&pool, &req).unwrap();
    assert_eq!(count, 2);
    assert_eq!(oracle[0].members, [2, 3, 1, 4, 6]);
    assert_eq!(oracle[0].power, 1_594_979_296);
    let actual = search(&pool, &req);
    assert!(matches!(actual, Err(Error::Domain(_))), "oracle={oracle:?}, actual={actual:?}");
}

#[test]
fn native_infinity_conversion_is_preserved_but_not_proven_monotone() {
    let mut data = distinct(5, 0);
    set_adjustment(&mut data, "3e38");
    let master = data.master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let mut req = request(1);
    req.objective = Objective::SkipScore { score_id: 1004, chart: one_note(&master) };
    let model = match &req.objective {
        Objective::SkipScore { score_id, chart } => SkipModel::new(&master, *score_id, chart).unwrap(),
        _ => unreachable!(),
    };
    // The native positive-infinity conversion produces i32::MIN.
    assert_eq!(model.score_reference(94_037).unwrap(), i32::MIN);
    assert_eq!(model.fast.score(94_037), (i32::MIN, i32::MIN as i64));
    assert!(matches!(search(&pool, &req), Err(Error::Domain(_))));
}

#[test]
fn full_domain_guard_rejects_the_two_deck_rank_reversal() {
    let mut data = distinct(5, 0);
    for (name, rows) in &mut data.tables {
        if name == "MasterMemberCard" {
            let rows = rows.as_array_mut().unwrap();
            let mut high = rows[4].clone();
            high["_id"] = json!(6);
            for stat in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                high[stat] = json!(high[stat].as_i64().unwrap() * 50);
            }
            rows.push(high);
        }
    }
    set_adjustment(&mut data, "3e33");
    let master = data.master();
    let mut owned = roster(&mut Rng::new(739), &master);
    owned.members[5] = owned.members[4].clone();
    owned.members[5].id = 6;
    let pool = Pool::new(&master, &owned).unwrap();
    let mut req = request(2);
    req.objective = Objective::SkipScore { score_id: 1004, chart: one_note(&master) };
    req.constraints.leader = Some(1);
    req.constraints.include_members = vec![1, 2, 3, 4];
    let (oracle, count) = oracle::brute_force(&pool, &req).unwrap();
    assert_eq!(count, 2);
    assert_eq!((oracle[0].members, oracle[0].power, oracle[0].score), ([2, 3, 1, 4, 5], 94_307, Some(i32::MAX)));
    assert_eq!((oracle[1].members, oracle[1].power, oracle[1].score), ([2, 3, 1, 4, 6], 947_750, Some(i32::MIN)));
    req.k = 1;
    assert!(matches!(search(&pool, &req), Err(Error::Domain(_))));
}

#[test]
fn preparation_expiry_discards_partial_tables() {
    let master = distinct(7, 2).master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let mut req = request(5);
    req.time_limit = Some(Duration::from_millis(10));
    let out = budget::test_clock::with_expiry("table-member", 2, || search(&pool, &req)).unwrap();
    assert_eq!(out.completion, Completion::TimedOut);
    assert!(out.results.is_empty());
    assert_eq!(out.stats, PowerStats::default());
}

fn verify_expiry(stage: &'static str, hit: usize) {
    let master = distinct(7, 0).master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let mut req = request(5);
    let (oracle, _) = oracle::brute_force(&pool, &req).unwrap();
    req.time_limit = Some(Duration::from_millis(10));
    let out = budget::test_clock::with_expiry(stage, hit, || search(&pool, &req)).unwrap();
    assert_eq!(out.completion, Completion::TimedOut);
    assert_eq!(out.results, oracle[..1]);
    let result = &out.results[0];
    let deck = pool.deck(result.members, result.snaps, result.performance_order).unwrap();
    assert_eq!(evaluate(&pool, &deck, &req.objective).unwrap(), (result.power, result.score));
}

#[test]
fn expiry_before_second_regular_verification_returns_only_verified_prefix() {
    verify_expiry("verify-candidate", 2);
}

#[test]
fn completed_atomic_verification_may_overrun_but_never_starts_next() {
    verify_expiry("verify-finished", 1);
}

// Independent canonical member-set/leader/Snap enumeration (not all physical
// permutations, which do not affect Power), with no production constraints,
// matching, class reduction, pruning or bound helper used for actual evaluation.
fn each_deck(pool: &Pool, mut visit: impl FnMut(Deck)) {
    fn snaps(slot: usize, pool: &Pool, deck: &mut Deck, visit: &mut impl FnMut(Deck)) {
        if slot == 5 {
            visit(*deck);
            return;
        }
        deck.snaps[slot] = None;
        snaps(slot + 1, pool, deck, visit);
        for snap in 0..pool.snaps.len() {
            if deck.snaps[..slot].contains(&Some(snap)) {
                continue;
            }
            deck.snaps[slot] = Some(snap);
            snaps(slot + 1, pool, deck, visit);
        }
        deck.snaps[slot] = None;
    }
    fn members(start: usize, selected: &mut Vec<usize>, pool: &Pool, visit: &mut impl FnMut(Deck)) {
        if selected.len() == 5 {
            for &leader in selected.iter() {
                let other: Vec<_> = selected.iter().copied().filter(|&m| m != leader).collect();
                let mut deck = Deck {
                    members: [other[0], other[1], leader, other[2], other[3]],
                    snaps: [None; 5],
                    performance_order: [0, 1, 2, 3, 4],
                };
                snaps(0, pool, &mut deck, visit);
            }
            return;
        }
        for member in start..pool.members.len() {
            if selected.iter().any(|&m| pool.members[m].character_id == pool.members[member].character_id) {
                continue;
            }
            selected.push(member);
            members(member + 1, selected, pool, visit);
            selected.pop();
        }
    }
    members(0, &mut Vec::new(), pool, &mut visit);
}

fn signed_profiles(mode: usize) -> Synth {
    let mut data = distinct(8, 2);
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        row["_leaderSkillID"] = json!(4);
        row["_cardType"] = json!(if mode == 0 { 2 } else { 1 + row["_id"].as_i64().unwrap() % 5 });
    });
    if mode < 2 {
        set_column(&mut data, "MasterCharacter", &mut |row| {
            row["_bandID"] = json!(if mode == 0 { 2 } else { 1 });
        });
    }
    set_column(&mut data, "MasterLeaderSkillEffect", &mut |row| {
        if row["_leaderSkillID"] == 4 && row["_skillEffectType"] == 1000 {
            row["_effectValue"] = json!(2000);
        }
    });
    let mut effects = Vec::new();
    let mut id = 1000;
    for level in 1..=5 {
        for (group, target, kind, value, cumulative) in
            [(1, vec![], 1000, -37, 0), (2, vec![], 1001, 43, 0), (3, vec![3], 1002, -53, 0)]
        {
            effects.push(json!({"_id":id,"_leaderSkillID":4,"_level":level,"_skillConditionGroup":group,"_skillTargetIDs":target,"_skillEffectType":kind,"_effectValue":value,"_skillCumulativeConditionID":cumulative}));
            id += 1;
        }
        for cumulative in 1..=6 {
            for value in [-19, 23] {
                effects.push(json!({"_id":id,"_leaderSkillID":4,"_level":level,"_skillConditionGroup":0,"_skillTargetIDs":[],"_skillEffectType":1500 + cumulative % 4,"_effectValue":value,"_skillCumulativeConditionID":cumulative}));
                id += 1;
            }
        }
    }
    extend_table(&mut data, "MasterLeaderSkillEffect", effects);
    data
}

#[test]
fn signed_conditional_count_bounds_cover_every_regular_deck() {
    let mut checked = 0;
    for mode in 0..3 {
        let master = signed_profiles(mode).master();
        let pool = Pool::new(&master, &roster(&mut Rng::new(739 + mode as u64), &master)).unwrap();
        let song = pool.song(20).unwrap();
        let snaps: Vec<_> = (0..pool.snaps.len()).collect();
        let budget = SearchBudget::new(Instant::now(), None).unwrap();
        let table = Tables::new(&pool, Some(song.clone()), false, &snaps, budget).unwrap().unwrap();
        let (allowed, _) = resolve_allowed(&pool, &Constraints::default()).unwrap();
        let mut search = PowerSearch::new(&pool, &table, &allowed, 1, budget, LiveMode::None);
        let upper = search.power_upper_bound().unwrap();
        assert!(table.prove_nonnegative_power(&pool, &search.prepared_leaders(), &allowed, budget).unwrap());
        each_deck(&pool, |deck| {
            let actual = pool.deck_power(&deck, Some(&song), false).unwrap().power() as i64;
            let (lower, local_upper) = table.deck_power_bounds(&pool, deck.members).unwrap();
            assert!(
                lower <= actual && actual <= local_upper && actual <= upper,
                "mode {mode} {deck:?}: {lower} <= {actual} <= {local_upper} <= global {upper}"
            );
            checked += 1;
        });
    }
    assert_eq!(checked, 26_040);
    eprintln!("independent signed-bound oracle: {checked} regular-path legal decks");
}

#[test]
fn supported_negative_effect_profiles_retain_canonical_top_k() {
    let master = signed_profiles(2).master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let mut req = request(usize::MAX);
    req.constraints.no_snaps = false;
    req.objective = Objective::SkipScore { score_id: 2003, chart: common::chart(&mut Rng::new(53), 25) };
    let (oracle, enumerated) = oracle::brute_force(&pool, &req).unwrap();
    for k in [1, 5, 30, usize::MAX] {
        req.k = k;
        let out = search(&pool, &req).unwrap();
        assert_eq!(out.completion, Completion::Complete);
        assert_eq!(out.results, oracle[..oracle.len().min(k)]);
    }
    eprintln!("signed-effect canonical Top-K oracle: {enumerated} decks, {} member sets, K=1/5/30/all", oracle.len());
}

#[test]
fn genuinely_unproven_signed_domain_returns_domain_error() {
    let mut data = distinct(5, 0);
    set_column(&mut data, "MasterMemberCard", &mut |row| {
        row["_leaderSkillID"] = json!(4);
    });
    extend_table(&mut data, "MasterLeaderSkillEffect", (1..=5).map(|level| json!({"_id":1000+level,"_leaderSkillID":4,"_level":level,"_skillConditionGroup":1,"_skillTargetIDs":[],"_skillEffectType":1000,"_effectValue":-100_000,"_skillCumulativeConditionID":0})).collect());
    let master = data.master();
    let pool = Pool::new(&master, &roster(&mut Rng::new(739), &master)).unwrap();
    let mut req = request(1);
    req.objective = Objective::SkipScore { score_id: 1004, chart: one_note(&master) };
    assert!(matches!(search(&pool, &req), Err(Error::Domain(_))));
}

#[test]
fn constrained_lower_proof_matches_independent_canonical_enumeration() {
    let mut comparisons = 0;
    let mut accepted = 0;
    let mut rejected = 0;
    for magnitude in [0, 2_000, 12_000, 50_000] {
        let mut data = signed_profiles(2);
        set_column(&mut data, "MasterMemberCard", &mut |row| {
            if row["_id"] == 6 {
                row["_characterID"] = json!(5);
            }
        });
        extend_table(
            &mut data,
            "MasterLeaderSkillEffect",
            (1..=5)
                .map(|level| {
                    json!({
                        "_id":9000+level,"_leaderSkillID":4,"_level":level,
                        "_skillConditionGroup":1,"_skillTargetIDs":[],"_skillEffectType":1000,
                        "_effectValue":-magnitude,"_skillCumulativeConditionID":0
                    })
                })
                .collect(),
        );
        let master = data.master();
        let mut owned = roster(&mut Rng::new(739), &master);
        owned.snaps.clear();
        let pool = Pool::new(&master, &owned).unwrap();
        for required in [vec![1, 2, 3, 4, 5], vec![2, 3], vec![5], vec![]] {
            let constraints = Constraints {
                leader: Some(1),
                include_members: required.clone(),
                no_snaps: true,
                ..Default::default()
            };
            let (allowed, snaps) = resolve_allowed(&pool, &constraints).unwrap();
            let budget = SearchBudget::new(Instant::now(), None).unwrap();
            let table = Tables::new(&pool, None, false, &snaps, budget).unwrap().unwrap();
            let mut solver = PowerSearch::new(&pool, &table, &allowed, 1, budget, LiveMode::None);
            let upper = solver.power_upper_bound().unwrap();
            let mut minimum = i64::MAX;
            let mut count = 0;
            // Filter by IDs directly, without resolve_allowed's membership flags.
            // Enumerate component lower sums to independently audit the new
            // required/fixed/distinct-character minimization, including picks=0.
            each_deck(&pool, |deck| {
                let ids = deck.members.map(|m| pool.members[m].id);
                if ids[2] != 1 || !required.iter().all(|id| ids.contains(id)) {
                    return;
                }
                let (lower, _) = table.deck_power_bounds(&pool, deck.members).unwrap();
                minimum = minimum.min(lower);
                let actual = pool.deck_power(&deck, None, false).unwrap().power() as i64;
                assert!(lower <= actual && actual <= upper);
                count += 1;
            });
            assert!(count > 0);
            let proof = table.prove_nonnegative_power(&pool, &solver.prepared_leaders(), &allowed, budget);
            if minimum >= 0 {
                assert!(proof.unwrap());
                accepted += 1;
            } else {
                assert!(matches!(proof, Err(Error::Domain(_))));
                rejected += 1;
            }
            comparisons += 1;
        }
    }
    assert_eq!(comparisons, 16);
    assert!(accepted > 0 && rejected > 0);
    eprintln!("independent constrained lower oracle: {comparisons} cases, {accepted} accepted, {rejected} rejected");
}
