//! Retention pressure must not turn a repeatedly used complete proof into fresh native work.
use super::*;
use crate::search::{expectation, joint::reward_family_fixture};
use crate::types::{Metric, SimulationInput};
use ournotes_sim::live::full::luck_skills;

#[test]
fn family_cache_keeps_hot_families_and_reused_programs_under_cold_pressure() {
    let (master, owned, request) = reward_family_fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let members = [1, 2, 0, 3, 4];
    let physical = PhysicalDeck { members, snaps: [None; 5] };
    let input = expectation::context(&pool, &physical, &request.objective).unwrap();
    let skills = luck_skills(&master).unwrap();
    let context = LuckFamilyContext::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        || false,
    )
    .unwrap()
    .unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let mut cache = FamilyNodeCache::new(Some(&context), 8, 8 << 20, 6);
    assert!(cache.set_scope(bounds.family_reward_template().unwrap()));
    let mut family = cache.family(&pool, &domain, members, &mut || false).unwrap().unwrap();
    let required = bounds.required_family_profiles(&domain, &physical, &family.table, &[0]).unwrap();
    assert!(cache.complete_required(&mut family, &required, &mut curves, &mut || false).unwrap());
    // Every inserted prototype is constructed from a real complete native profile. Repeated cold donors
    // exercise retention without assuming equality of scores or manufacturing an opaque probability proof.
    let profile = required[0];
    let donor =
        context.prepare_budgeted_profile(&mut family.domain, profile, Some(&mut curves), || false).unwrap().unwrap();
    let mut prototypes = Vec::new();
    for _ in 0..24 {
        let key = context.budgeted_profile_program_key(&family.domain, profile, 1 << 20, || false).unwrap();
        prototypes
            .push(LuckFamilyProgram::from_budgeted_profile(&family.domain, key, &donor, 1 << 20, || false).unwrap());
    }
    let work = family.domain.work();
    cache.remember(members, Some(family));
    let orders = &crate::search::uniform::MEAN_ORDERS;
    let expected = cache.cached_leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut || false);
    assert!(matches!(expected, FamilyNodeOutcome::Upper(_)));
    let builds = cache.stats.profile_native_builds;
    let reservations = cache.stats.reserved_profile_frame_work;
    for program in prototypes {
        cache.remember_program(program);
        assert_eq!(cache.cached_leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut || false), expected);
        let retained = cache.entries.iter().find(|entry| entry.members == members).unwrap().state.as_ref().unwrap();
        assert_eq!(retained.domain.work(), work, "retention cannot forget a completed profile's work charge");
        assert_eq!(cache.stats.profile_native_builds, builds);
        assert_eq!(cache.stats.reserved_profile_frame_work, reservations);
        assert!(cache.entries.len() + cache.programs.len() <= 8);
        assert!(cache.bytes() <= 8 << 20);
    }
    assert!(cache.stats.profile_program_evictions > 0);
    assert_eq!(cache.stats.evictions, 0, "cold prototypes must not evict the repeatedly used complete family");
    assert!(cache.stats.profile_program_peak_entries <= 7);
    assert!(cache.stats.profile_program_peak_bytes <= 1 << 20);

    // Demonstrated full-label transport is more expensive to reproduce than a derived reward table.
    // A scan of new family identities must not displace that native proof before the program tier is full.
    let family = cache.entries.iter().find(|entry| entry.members == members).unwrap().state.as_ref().unwrap();
    let key = context.budgeted_profile_program_key(&family.domain, profile, 1 << 20, || false).unwrap();
    let index = cache.programs.iter().position(|entry| entry.program.matches(&key)).unwrap();
    assert!(cache.programs[index].program.transport_budgeted(&family.domain, &key, profile, || false).is_some());
    cache.touch_program(index);
    let protected = cache.programs.back().unwrap().last_used;
    for identity in 100..132 {
        cache.remember([identity; 5], None);
        assert!(cache.programs.iter().any(|entry| entry.last_used == protected && entry.transported));
        assert!(cache.entries.len() + cache.programs.len() <= 8);
        assert!(cache.bytes() <= 8 << 20);
    }
    let mut rebuilt = cache.family(&pool, &domain, members, &mut || false).unwrap().unwrap();
    assert!(rebuilt.table.profiles.iter().all(Option::is_none));
    assert!(cache.complete_required(&mut rebuilt, &required, &mut curves, &mut || false).unwrap());
    assert_eq!(
        cache.stats.profile_native_builds, builds,
        "derived-table rebuilding transports the complete native proof"
    );
    assert_eq!(rebuilt.domain.work().profile_attempts, 0);
    cache.remember(members, Some(rebuilt));
    assert_eq!(cache.cached_leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut || false), expected);
}

#[test]
fn family_node_cutoff_keeps_partial_covers_unavailable_and_preserves_strict_and_power_ties() {
    let (master, owned, request) = reward_family_fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let physical = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [None; 5] };
    let input = expectation::context(&pool, &physical, &request.objective).unwrap();
    let skills = luck_skills(&master).unwrap();
    let context = LuckFamilyContext::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        || false,
    )
    .unwrap()
    .unwrap();
    let orders = &crate::search::uniform::MEAN_ORDERS;
    let mut curves = LuckDpCache::new(8 << 20);
    let mut reference = FamilyNodeCache::new(Some(&context), 16, 8 << 20, 6);
    let complete =
        reference.upper_at_depth_four(&pool, &domain, &bounds, &physical, 0, orders, None, &mut curves, &mut || false);
    let FamilyNodeOutcome::Upper(complete_upper) = complete else { panic!("{complete:?}") };
    assert!(complete_upper > 0);
    assert_eq!(reference.stats.admitted_families, 2, "both legal last members belong to the node");

    let mut partial = FamilyNodeCache::new(Some(&context), 16, 8 << 20, 6);
    assert_eq!(
        partial.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            orders,
            Some(FamilyPruneCutoff { score: 0, power_allows_equal: false }),
            &mut curves,
            &mut || false,
        ),
        FamilyNodeOutcome::Unavailable,
        "an abandoned cover must not expose its partial maximum as a whole-node upper"
    );
    assert_eq!(partial.stats.admitted_families, 1, "the second complete native admission is unnecessary here");
    assert_eq!(partial.stats.non_pruning_exits, 1);
    assert_eq!(partial.stats.bounded_nodes, 0);
    assert_eq!(partial.stats.order_laws, partial.stats.profiles * 120);
    assert!(matches!(
        partial.cached_leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut || false),
        FamilyNodeOutcome::Upper(_)
    ));
    assert_eq!(
        partial.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            orders,
            Some(FamilyPruneCutoff { score: 0, power_allows_equal: true }),
            &mut curves,
            &mut || false,
        ),
        FamilyNodeOutcome::Unavailable,
        "an upper strictly above the score cutoff remains unusable even with a power certificate"
    );
    assert_eq!(partial.stats.admitted_families, 1);
    let mut unprepared = physical;
    unprepared.members[SLOTS[4]] = 5;
    assert_eq!(
        partial.cached_leaf_upper(&pool, &domain, &bounds, &unprepared, orders, &mut || false),
        FamilyNodeOutcome::Unavailable,
        "the omitted member family has no implicit cached leaf certificate"
    );
    // A later useful query completes the missing family. Previously completed labels and work remain valid.
    assert_eq!(
        partial.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            orders,
            Some(FamilyPruneCutoff { score: complete_upper + 1, power_allows_equal: false }),
            &mut curves,
            &mut || false,
        ),
        complete
    );
    assert_eq!(partial.stats.admitted_families, 2);
    assert_eq!(partial.stats.profiles, reference.stats.profiles);
    assert_eq!(partial.stats.order_laws, reference.stats.order_laws);
    assert_eq!(partial.stats.bounded_nodes, 1);
    let builds = partial.stats.profile_native_builds;
    let reservations = partial.stats.reserved_profile_frame_work;

    for (power_allows_equal, expected) in [(false, FamilyNodeOutcome::Unavailable), (true, complete)] {
        assert_eq!(
            partial.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &physical,
                0,
                orders,
                Some(FamilyPruneCutoff { score: complete_upper, power_allows_equal }),
                &mut curves,
                &mut || false,
            ),
            expected,
            "only the caller's existing strict power certificate permits an equal-score prune"
        );
    }
    assert_eq!(partial.stats.profile_native_builds, builds);
    assert_eq!(partial.stats.reserved_profile_frame_work, reservations);
    assert!(partial.stats.peak_entries <= 16 && partial.stats.peak_bytes <= 8 << 20);
    let mut polls = 0;
    assert_eq!(
        partial.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            orders,
            Some(FamilyPruneCutoff { score: 0, power_allows_equal: false }),
            &mut curves,
            &mut || {
                polls += 1;
                false
            },
        ),
        FamilyNodeOutcome::Unavailable
    );
    let exits = partial.stats.non_pruning_exits;
    let mut cancelled_polls = 0;
    assert_eq!(
        partial.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            orders,
            Some(FamilyPruneCutoff { score: 0, power_allows_equal: false }),
            &mut curves,
            &mut || {
                cancelled_polls += 1;
                cancelled_polls == polls
            },
        ),
        FamilyNodeOutcome::Stopped,
        "cancellation after complete-family work has priority over the optional-work exit"
    );
    assert_eq!(partial.stats.non_pruning_exits, exits);
    assert_eq!(
        partial.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            orders,
            Some(FamilyPruneCutoff { score: i128::MIN, power_allows_equal: false }),
            &mut curves,
            &mut || true,
        ),
        FamilyNodeOutcome::Stopped,
        "cancellation cannot be converted into an optional-cap refusal"
    );
}
