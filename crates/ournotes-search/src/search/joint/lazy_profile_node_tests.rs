//! Every required binding is checked against independent native descendants, including unknown profiles.
use super::*;

#[test]
fn lazy_profile_node_unknown_profiles_refuse_the_whole_mask_and_completed_subsets_enclose_native_descendants() {
    let (master, owned, request) = fixture();
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
    let limits = LuckFamilyLimits {
        max_pair_models: 64,
        max_profiles: 6,
        max_order_evaluations: 720,
        max_frame_work: 1_000_000,
        max_retained_bytes: 32 << 20,
    };
    let choices = family_choices(&pool, &domain, members);
    let admitted = context.admit_domain(&choices, limits, || false).unwrap().unwrap();
    let scope = bounds.family_reward_template().unwrap();
    let mut table = scope.start_profiles(members, &admitted).unwrap();
    let wanted = bounds.required_family_profiles(&domain, &physical, &table, &[0, 1, 2]).unwrap();
    assert_eq!(wanted.len(), 2, "a fixed empty prefix needs absence and only the last-slot writer");
    assert_eq!(table.profiles.len(), 6);
    assert!(table.profiles.iter().all(Option::is_none));
    assert!(bounds.family_profiles_upper(&domain, &physical, &table, &[0, 1, 2]).is_none());
    let mut curves = LuckDpCache::new(8 << 20);
    let absent = admitted.profile_for(&[None; 5]).unwrap();
    let law = context.prepare_profile(&admitted, absent, Some(&mut curves), || false).unwrap().unwrap();
    let foreign = context.admit_domain(&choices, limits, || false).unwrap().unwrap();
    assert!(scope.bind_profile(members, &foreign, &law, &mut || false).is_none());
    table.profiles[absent] = scope.bind_profile(members, &admitted, &law, &mut || false);
    assert!(bounds.family_profiles_upper(&domain, &physical, &table, &[0]).is_some());
    assert!(
        bounds.family_profiles_upper(&domain, &physical, &table, &[0, 1, 2]).is_none(),
        "a valid absent-writer bound cannot conceal the uncomputed writer alternative"
    );
    for &profile in &wanted {
        if table.profiles[profile].is_none() {
            let law = context.prepare_profile(&admitted, profile, Some(&mut curves), || false).unwrap().unwrap();
            table.profiles[profile] = scope.bind_profile(members, &admitted, &law, &mut || false);
        }
    }
    assert_eq!(table.profiles.iter().filter(|profile| profile.is_some()).count(), 2);
    let cap = bounds.family_profiles_upper(&domain, &physical, &table, &[0, 1, 2]).unwrap();
    assert_eq!(
        Some(cap),
        [0, 1, 2]
            .iter()
            .filter_map(|choice| {
                bounds.family_profiles_upper(&domain, &physical, &table, std::slice::from_ref(choice))
            })
            .max()
    );
    assert!(bounds.family_profiles_upper(&domain, &physical, &table, &[0, usize::MAX]).is_none());
    let oracles = native_candidates(&pool, &request, &domain);
    let mut checked = 0;
    for oracle in
        oracles.iter().filter(|oracle| oracle.physical.members == members && same_prefix(&physical, &oracle.physical))
    {
        assert!(oracle.sum_of_order_means.at_most_integer(cap));
        checked += 1;
    }
    assert_eq!(checked, 3);
    let mut different_owner = physical;
    different_owner.snaps[2] = Some(domain.snaps()[0]);
    assert!(
        bounds.family_profiles_upper(&domain, &different_owner, &table, &[0]).is_none(),
        "moving the same physical writer to an unrequested owner cannot reuse another profile"
    );
    let full = context.prepare(&choices, Some(&mut curves), limits, || false).unwrap().unwrap();
    let complete_table = scope.bind(members, &full, &mut || false).unwrap();
    assert_eq!(bounds.family_mask_upper(&domain, &physical, &complete_table, &[0, 1, 2]), Some(cap));

    // Exercise the real depth-four path: both possible last members need just two of their six profiles.
    let mut cache = FamilyNodeCache::new(Some(&context), 16, 8 << 20, 6);
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
    let FamilyNodeOutcome::Upper(node_cap) = outcome else { panic!("{outcome:?}") };
    assert_eq!(cache.stats().admitted_families, 2);
    assert_eq!(cache.stats().prepared_families, 2);
    assert_eq!(cache.stats().profiles, 4);
    assert_eq!(cache.stats().order_laws, 4 * 120);
    assert!(cache.stats().peak_entries <= 16 && cache.stats().peak_bytes <= 8 << 20);
    for oracle in oracles.iter().filter(|oracle| same_prefix(&physical, &oracle.physical)) {
        assert!(oracle.sum_of_order_means.at_most_integer(node_cap));
    }
    assert_eq!(
        cache.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &crate::search::uniform::MEAN_ORDERS,
            &mut curves,
            &mut || false
        ),
        outcome
    );
    assert_eq!(cache.stats().profiles, 4);
    assert!(cache.stats().profile_hits >= 4);
}
