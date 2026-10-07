//! Program transport preserves target reward addition order and stays inside the original shared cache.
use super::*;
use ournotes_sim::live::full::LuckFamilyProgram;

#[test]
fn family_program_three_cycle_keeps_target_reward_coefficients_bit_identical() {
    let (master, owned, request) = fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let source_members = [1, 2, 0, 3, 4];
    let target_members = [2, 0, 1, 3, 4];
    let physical = PhysicalDeck { members: source_members, snaps: [None; 5] };
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
    let limits = LuckFamilyLimits {
        max_pair_models: 64,
        max_profiles: 6,
        max_order_evaluations: 720,
        max_frame_work: 1_000_000,
        max_retained_bytes: 32 << 20,
    };
    let source =
        context.admit_domain(&family_choices(&pool, &domain, source_members), limits, || false).unwrap().unwrap();
    let target =
        context.admit_domain(&family_choices(&pool, &domain, target_members), limits, || false).unwrap().unwrap();
    let writer = domain.snaps()[0];
    let source_id = source.profile_for(&[Some(writer), None, None, None, None]).unwrap();
    let target_id = target.profile_for(&[None, None, Some(writer), None, None]).unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let donor = context.prepare_profile(&source, source_id, Some(&mut curves), || false).unwrap().unwrap();
    let source_key = context.profile_program_key(&source, source_id, 1 << 20, || false).unwrap();
    let program = LuckFamilyProgram::from_profile(&source, source_key, &donor, 1 << 20, || false).unwrap();
    let target_key = context.profile_program_key(&target, target_id, 1 << 20, || false).unwrap();
    let transported = program.transport(&target, &target_key, target_id, || false).unwrap();
    let fresh = context.prepare_profile(&target, target_id, Some(&mut curves), || false).unwrap().unwrap();
    let scope = bounds.family_reward_template().unwrap();
    assert!(scope.bind_profile(target_members, &target, &donor, &mut || false).is_none());
    let transported = scope.bind_profile(target_members, &target, &transported, &mut || false).unwrap();
    let fresh = scope.bind_profile(target_members, &target, &fresh, &mut || false).unwrap();
    assert_eq!(transported.a0.to_bits(), fresh.a0.to_bits());
    for (left, right) in transported.mean.iter().zip(&fresh.mean) {
        assert_eq!(left.len(), right.len());
        for (left, right) in left.iter().zip(right) {
            assert_eq!(left.to_bits(), right.to_bits());
        }
    }
}

#[test]
fn family_program_node_reuse_respects_shared_capacity_and_zero_cache_fallback() {
    let (master, owned, request) = fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let first = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [None; 5] };
    let second = PhysicalDeck { members: [2, 0, 1, 3, 4], snaps: [None; 5] };
    let input = expectation::context(&pool, &first, &request.objective).unwrap();
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
    let mut expected = None;
    // One entry disables the optional program memo but keeps the exact original complete-profile path.
    for entries in [1, 2, 16, 1024] {
        let mut cache = FamilyNodeCache::new(Some(&context), entries, 8 << 20, 6);
        let mut outcomes = Vec::new();
        for physical in [first, second, first] {
            let outcome = cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &physical,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || false,
            );
            assert!(matches!(outcome, FamilyNodeOutcome::Upper(_)), "{outcome:?}");
            outcomes.push(outcome);
        }
        if let Some(expected) = &expected {
            assert_eq!(&outcomes, expected);
        } else {
            expected = Some(outcomes);
        }
        let stats = cache.stats();
        assert!(stats.peak_entries <= entries.min(64));
        assert!(stats.peak_bytes <= 8 << 20);
        assert!(stats.profile_program_peak_entries <= entries.min(64).saturating_sub(1).min(63));
        assert!(stats.profile_program_peak_bytes <= 1 << 20);
        if entries == 1 {
            assert_eq!(stats.profile_program_lookups, 0);
            assert_eq!(stats.profile_program_hits, 0);
        } else if entries >= 16 {
            assert!(stats.profile_program_hits > 0, "permuted member slots reuse a completed program");
            assert!(stats.profile_native_builds < stats.profiles);
            assert_eq!(stats.order_laws, stats.profiles * 120);
        }
        assert_eq!(
            cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &second,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || true,
            ),
            FamilyNodeOutcome::Stopped
        );
        assert_eq!(
            cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &second,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || false,
            ),
            expected.as_ref().unwrap()[1]
        );
    }
    for (entries, bytes) in [(0, 8 << 20), (16, 0), (16, 1)] {
        let mut cache = FamilyNodeCache::new(Some(&context), entries, bytes, 6);
        assert_eq!(
            cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &first,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || false,
            ),
            FamilyNodeOutcome::Unavailable
        );
        assert_eq!(cache.stats().profile_program_hits, 0);
        assert_eq!(cache.stats().profile_native_builds, 0);
        assert_eq!(cache.stats().order_laws, 0);
    }
}
