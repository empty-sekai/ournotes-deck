//! Cold leaves may prepare their own complete profile without certifying another unknown binding.
use super::*;
use crate::search::{expectation, joint::reward_family_fixture};
use crate::types::{Metric, SimulationInput};
use ournotes_sim::live::full::luck_skills;

#[test]
fn family_leaf_prepares_only_the_actual_profile_and_matches_the_complete_table() {
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
    let mut full_cache = FamilyNodeCache::new(Some(&context), 16, 8 << 20, 6);
    assert!(full_cache.set_scope(bounds.family_reward_template().unwrap()));
    let mut full = full_cache.family(&pool, &domain, physical.members, &mut || false).unwrap().unwrap();
    let all: Vec<_> = (0..full.table.profiles.len()).collect();
    assert_eq!(all.len(), 6);
    assert!(full_cache.complete_required(&mut full, &all, &mut curves, &mut || false).unwrap());
    assert!(full.table.profiles.iter().all(Option::is_some));
    let expected = |p: &PhysicalDeck| {
        let choice = p.snaps[SLOTS[4]].map_or(0, |snap| domain.snaps().iter().position(|&id| id == snap).unwrap() + 1);
        FamilyNodeOutcome::Upper(
            bounds.family_profiles_upper_measured(&domain, p, &full.table, &[choice]).unwrap().upper,
        )
    };

    let mut cache = FamilyNodeCache::new(Some(&context), 16, 8 << 20, 6);
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || false),
        expected(&physical)
    );
    assert_eq!((cache.stats.leaf_checks, cache.stats.bounded_leaves, cache.stats.leaf_preparation_attempts), (1, 1, 1));
    assert_eq!((cache.stats.profiles, cache.stats.order_laws), (1, 120));
    let state = cache.entries.iter().find(|entry| entry.members == physical.members).unwrap().state.as_ref().unwrap();
    assert_eq!(state.table.profiles.iter().filter(|value| value.is_some()).count(), 1);
    let work = state.domain.work();
    let mut writer = physical;
    writer.snaps[0] = Some(domain.snaps()[0]);
    assert_eq!(
        cache.cached_leaf_upper(&pool, &domain, &bounds, &writer, orders, &mut || false),
        FamilyNodeOutcome::Unavailable
    );
    assert_eq!(cache.stats.profiles, 1, "a different unrequested writer owner has no certificate");
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || false),
        expected(&physical)
    );
    let state = cache.entries.iter().find(|entry| entry.members == physical.members).unwrap().state.as_ref().unwrap();
    assert_eq!(state.domain.work(), work, "a completed hit reserves no new native work");
    assert_eq!(cache.stats.leaf_preparation_attempts, 1);
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &writer, orders, &mut curves, &mut || false),
        expected(&writer)
    );
    assert_eq!((cache.stats.profiles, cache.stats.order_laws), (2, 240));
    assert_eq!(cache.stats.leaf_preparation_attempts, 2);

    let mut reordered = writer;
    reordered.members.swap(0, 3);
    reordered.snaps.swap(0, 3);
    let builds = cache.stats.profile_native_builds;
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &reordered, orders, &mut curves, &mut || false),
        expected(&reordered)
    );
    assert_eq!(cache.stats.profile_native_builds, builds, "canonical owner remapping reuses the same complete table");
    assert_eq!((cache.stats.leaf_checks, cache.stats.bounded_leaves, cache.stats.leaf_preparation_attempts), (5, 4, 2));
    let stats = serde_json::to_value(cache.stats()).unwrap();
    assert_eq!(stats["leafPreparationAttempts"], 2);
    assert!(cache.stats.peak_entries <= 16 && cache.stats.peak_bytes <= 8 << 20);
}

#[test]
fn family_leaf_budget_refusal_stays_recorded_and_disabled_caches_do_no_work() {
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
    let mut curves = LuckDpCache::new(0);
    for (entries, bytes, profiles) in [(0, 8 << 20, 6), (16, 0, 6), (16, 8 << 20, 0), (16, 1, 6)] {
        let mut cache = FamilyNodeCache::new(Some(&context), entries, bytes, profiles);
        assert_eq!(
            cache.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || false),
            FamilyNodeOutcome::Unavailable
        );
        assert_eq!(
            (cache.stats.leaf_preparation_attempts, cache.stats.profile_native_builds, cache.stats.order_laws),
            (0, 0, 0)
        );
    }
    // Keep the original six-profile binding domain, but permit only one native 120-label computation.
    let mut cache = FamilyNodeCache::new(Some(&context), 1, 8 << 20, 6).with_profile_work_limits(120, u64::MAX);
    assert!(matches!(
        cache.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || false),
        FamilyNodeOutcome::Upper(_)
    ));
    let mut writer = physical;
    writer.snaps[2] = Some(domain.snaps()[0]);
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &writer, orders, &mut curves, &mut || false),
        FamilyNodeOutcome::Unavailable
    );
    let state = cache.entries.front().unwrap().state.as_ref().unwrap();
    let work = state.domain.work();
    assert_eq!((work.profile_attempts, work.reserved_order_evaluations, work.budget_refusals), (1, 120, 1));
    let profile = bounds.required_family_profiles(&domain, &writer, &state.table, &[0]).unwrap()[0];
    assert!(state.refused[profile] && state.table.profiles[profile].is_none());
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &writer, orders, &mut curves, &mut || false),
        FamilyNodeOutcome::Unavailable
    );
    assert_eq!(cache.entries.front().unwrap().state.as_ref().unwrap().domain.work(), work);
    assert_eq!(cache.stats.profile_native_builds, 1);
    assert_eq!(cache.stats.order_laws, 120);
    assert!(matches!(
        cache.cached_leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut || false),
        FamilyNodeOutcome::Upper(_)
    ));

    let attempts = cache.stats.leaf_preparation_attempts;
    let mut illegal = physical;
    illegal.members[0] = illegal.members[1];
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &illegal, orders, &mut curves, &mut || false),
        FamilyNodeOutcome::Unavailable
    );
    assert_eq!(
        cache.leaf_upper(&pool, &domain, &bounds, &physical, &orders[..119], &mut curves, &mut || false),
        FamilyNodeOutcome::Unavailable
    );
    assert_eq!(cache.stats.leaf_preparation_attempts, attempts);
}

#[test]
fn family_leaf_cancellation_keeps_reserved_work_and_only_completed_profile_truth() {
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
    let fresh = || FamilyNodeCache::new(Some(&context), 1, 8 << 20, 6).with_profile_work_limits(120, u64::MAX);
    let mut complete = fresh();
    let mut curves = LuckDpCache::new(0);
    let mut polls = 0;
    let expected = complete.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || {
        polls += 1;
        false
    });
    assert!(matches!(expected, FamilyNodeOutcome::Upper(_)));
    let mut saw_charged_partial = false;
    for stop in [1, polls / 2, polls - 1, polls] {
        let mut cache = fresh();
        let mut curves = LuckDpCache::new(0);
        let mut called = 0;
        assert_eq!(
            cache.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || {
                called += 1;
                called >= stop
            }),
            FamilyNodeOutcome::Stopped,
            "poll {stop}"
        );
        assert_eq!(cache.stats.bounded_leaves, 0, "cancellation publishes no leaf upper");
        let Some(state) = cache.entries.front().and_then(|entry| entry.state.as_ref()) else {
            assert_eq!(cache.stats.reserved_profile_order_work, 0);
            continue;
        };
        let work = state.domain.work();
        let completed = state.table.profiles.iter().any(Option::is_some);
        assert_eq!(work.reserved_order_evaluations as u64, cache.stats.reserved_profile_order_work);
        assert_eq!(work.reserved_frame_work, cache.stats.reserved_profile_frame_work);
        let cached = cache.cached_leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut || false);
        assert_eq!(cached, if completed { expected } else { FamilyNodeOutcome::Unavailable });
        if work.profile_attempts > 0 && !completed {
            saw_charged_partial = true;
            assert_eq!(work.reserved_order_evaluations, 120);
            assert_eq!(
                cache.leaf_upper(&pool, &domain, &bounds, &physical, orders, &mut curves, &mut || false),
                FamilyNodeOutcome::Unavailable
            );
            let retained = cache.entries.front().unwrap().state.as_ref().unwrap().domain.work();
            assert_eq!(retained.profile_attempts, work.profile_attempts);
            assert_eq!(retained.reserved_order_evaluations, work.reserved_order_evaluations);
            assert_eq!(retained.reserved_frame_work, work.reserved_frame_work);
            assert_eq!(retained.budget_refusals, work.budget_refusals + 1);
        }
        if stop == polls {
            assert!(completed, "the final publication poll retains the already completed profile");
        }
    }
    assert!(saw_charged_partial, "exercise cancellation inside an already reserved profile");
}
