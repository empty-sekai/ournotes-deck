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
