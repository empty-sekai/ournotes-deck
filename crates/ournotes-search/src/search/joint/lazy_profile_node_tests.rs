//! Every required binding is checked against independent native descendants, including unknown profiles.
use super::*;
use ournotes_sim::live::full::LuckFamilyDecline;

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
    assert!(cap <= bounds.family_mask_upper(&domain, &physical, &complete_table, &[0, 1, 2]).unwrap());

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

#[test]
fn profile_probe_runs_tighten_actual_bindings_while_covering_every_native_descendant() {
    let (master, owned, request) = fixture();
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
    let scope = bounds.family_reward_template().unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let oracles = native_candidates(&pool, &request, &domain);
    let mut tightened = 0;
    let mut checked = 0;
    let mut by_profile = BTreeMap::<(usize, usize), std::collections::BTreeSet<u64>>::new();
    for last in [4, 5] {
        let members = [1, 2, 0, 3, last];
        let choices = family_choices(&pool, &domain, members);
        let limits = LuckFamilyLimits { max_pair_models: 64, max_frame_work: 1_000_000, ..Default::default() };
        let admitted = context.admit_domain(&choices, limits, || false).unwrap().unwrap();
        let full = context.prepare(&choices, Some(&mut curves), limits, || false).unwrap().unwrap();
        let old = scope.bind(members, &full, &mut || false).unwrap();
        let mut table = scope.start_profiles(members, &admitted).unwrap();
        for id in 0..admitted.profile_count() {
            let profile = context.prepare_profile(&admitted, id, Some(&mut curves), || false).unwrap().unwrap();
            let reward = scope.bind_profile(members, &admitted, &profile, &mut || false).unwrap();
            assert!(reward.max_probe_runs.is_some());
            table.profiles[id] = Some(reward);
        }
        for oracle in oracles.iter().filter(|oracle| oracle.physical.members == members) {
            let physical = &oracle.physical;
            let last_choice = [last_choice(&domain, physical)];
            let result = bounds.family_profiles_upper_measured(&domain, physical, &table, &last_choice).unwrap();
            let old_cap = bounds.family_mask_upper(&domain, physical, &old, &last_choice).unwrap();
            assert!(result.upper <= old_cap);
            assert!(oracle.sum_of_order_means.at_most_integer(result.upper), "{:?}", physical);
            assert_eq!(result.binding_drift_checks, 1);
            assert!(result.maximum_binding_offset_reduction >= 0.0);
            tightened += usize::from(result.upper < old_cap);
            let id = admitted.profile_for(&physical.snaps).unwrap();
            by_profile.entry((last, id)).or_default().insert(result.maximum_binding_offset_reduction.to_bits());

            // Losing optional transition evidence restores exactly the old budget, including its a0/rounding.
            let run_bound = table.profiles[id].as_mut().unwrap().max_probe_runs.take();
            assert_eq!(bounds.family_profiles_upper(&domain, physical, &table, &last_choice), Some(old_cap));
            table.profiles[id].as_mut().unwrap().max_probe_runs = run_bound;
            checked += oracle.order_means.len();
        }
        let mut absent = PhysicalDeck { members, snaps: [None; 5] };
        absent.snaps[SLOTS[0]] = Some(domain.snaps()[0]);
        let needed = admitted.profile_for(&absent.snaps).unwrap();
        let completed = table.profiles[needed].take();
        assert!(bounds.family_profiles_upper(&domain, &absent, &table, &[0]).is_none());
        table.profiles[needed] = completed;
        assert!(bounds.family_profiles_upper(&domain, &absent, &table, &[0]).is_some());
    }
    assert_eq!(checked, 62 * 120);
    assert!(tightened > 0);
    assert!(by_profile.values().any(|values| values.len() > 1), "ordinary bindings keep distinct work budgets");
}

#[test]
fn budgeted_profile_nodes_cover_every_legal_suffix_with_only_required_native_profiles() {
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
    let frames = input.play.frames.len() as u64;
    let limits = LuckFamilyLimits {
        max_pair_models: 64,
        max_profiles: 6,
        max_order_evaluations: 240,
        max_frame_work: frames * 241,
        max_retained_bytes: 32 << 20,
    };
    let choices = family_choices(&pool, &domain, members);
    assert_eq!(context.admit_domain(&choices, limits, || false).unwrap_err().reason, LuckFamilyDecline::Budget);
    let mut admitted = context.admit_profile_domain(&choices, limits, || false).unwrap().unwrap();
    let scope = bounds.family_reward_template().unwrap();
    let mut table = scope.start_budgeted_profiles(members, &admitted).unwrap();
    let required = bounds.required_family_profiles(&domain, &physical, &table, &[0, 1, 2]).unwrap();
    assert_eq!(required.len(), 2);
    assert_eq!(table.profiles.len(), 6);
    let mut curves = LuckDpCache::new(8 << 20);
    for (index, &profile) in required.iter().enumerate() {
        let law =
            context.prepare_budgeted_profile(&mut admitted, profile, Some(&mut curves), || false).unwrap().unwrap();
        table.profiles[profile] = scope.bind_budgeted_profile(members, &admitted, &law, &mut || false);
        if index + 1 < required.len() {
            assert!(bounds.family_profiles_upper(&domain, &physical, &table, &[0, 1, 2]).is_none());
        }
    }
    assert_eq!(table.profiles.iter().flatten().count(), 2);
    let cap = bounds.family_profiles_upper(&domain, &physical, &table, &[0, 1, 2]).unwrap();
    let oracles = native_candidates(&pool, &request, &domain);
    let mut checked = 0;
    for oracle in
        oracles.iter().filter(|oracle| oracle.physical.members == members && same_prefix(&physical, &oracle.physical))
    {
        assert!(oracle.sum_of_order_means.at_most_integer(cap));
        checked += 1;
    }
    assert_eq!(checked, 3);

    // One cache entry disables complete-program retention, making the native budget boundary observable.
    let mut cache = FamilyNodeCache::new(Some(&context), 1, 8 << 20, 6).with_profile_work_limits(240, frames * 241);
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
    assert_eq!(cache.stats().profiles, 4);
    assert_eq!(cache.stats().order_laws, 480);
    assert_eq!(cache.stats().reserved_profile_order_work, 480);
    assert_eq!(cache.stats().reserved_profile_frame_work, frames * 482);
    assert_eq!(cache.stats().profile_budget_refusals, 0);
    for oracle in oracles.iter().filter(|oracle| same_prefix(&physical, &oracle.physical)) {
        assert!(oracle.sum_of_order_means.at_most_integer(node_cap));
    }
    let mut limited = FamilyNodeCache::new(Some(&context), 1, 8 << 20, 6).with_profile_work_limits(120, frames * 121);
    assert_eq!(
        limited.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &crate::search::uniform::MEAN_ORDERS,
            &mut curves,
            &mut || false,
        ),
        FamilyNodeOutcome::Unavailable
    );
    assert_eq!(limited.stats().profiles, 1);
    assert_eq!(limited.stats().order_laws, 120);
    assert_eq!(limited.stats().reserved_profile_order_work, 120);
    assert_eq!(limited.stats().reserved_profile_frame_work, frames * 121);
    assert_eq!(limited.stats().profile_budget_refusals, 1);
    assert_eq!(limited.stats().bounded_nodes, 0);
    assert_eq!(
        limited.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &crate::search::uniform::MEAN_ORDERS,
            &mut curves,
            &mut || false,
        ),
        FamilyNodeOutcome::Unavailable
    );
    assert_eq!(limited.stats().reserved_profile_order_work, 120);
    assert_eq!(limited.stats().reserved_profile_frame_work, frames * 121);
}
