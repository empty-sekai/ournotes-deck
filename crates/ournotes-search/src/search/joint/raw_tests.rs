use super::*;
use crate::search::{Objective, expectation, uniform};
use crate::types::{Metric, SimulationInput};
use ournotes_sim::live::random::LiveRandom;
use ournotes_sim::master::SkillTargetRow;

fn fixture(conversion: bool) -> (ournotes_sim::master::Master, ournotes_sim::cards::Roster, SearchRequest) {
    let (mut master, mut owned, mut request) = reward_family_fixture();
    // Keep the actual timed live rows, ordinary extension and timed support row, but make this oracle
    // lottery-free. Six physical members and one distinct resource give twelve legal complete decks.
    request.objective = request.objective.inner().clone();
    let Objective::LiveScore { gekisou, .. } = &mut request.objective else { unreachable!() };
    *gekisou = None;
    owned.snaps.retain(|snap| snap.id == 2);
    if conversion {
        master.skill_targets.push(SkillTargetRow {
            id: 9961,
            skill_target_type: 4,
            judgement: 5,
            ..Default::default()
        });
        let mut converter = master.live_skill_effects[0].clone();
        converter.id = 9962;
        converter.live_skill_id = 9963;
        converter.skill_effect_type = 12006;
        converter.skill_target_ids = vec![9961];
        converter.effect_value = 4;
        converter.activation_time_second = 1.0;
        master.live_skill_effects.push(converter);
        let mut skill = master.live_skills[0].clone();
        skill.id = 9963;
        master.live_skills.push(skill);
        // The converter belongs only to the optional sixth member, never to the chosen prefix.
        master.member_cards.iter_mut().find(|member| member.id == 6).unwrap().live_skill_id = 9963;
        master.reindex().unwrap();
    }
    (master, owned, request)
}

fn descendants(pool: &Pool, domain: &CandidateDomain, bounds: &JointBounds) -> Vec<PhysicalDeck> {
    let mut rows = Vec::new();
    for last in [4, 5] {
        for resource_owner in 0..=5 {
            let mut physical = PhysicalDeck { members: [1, 2, 0, 3, last], snaps: [None; 5] };
            if resource_owner < 5 {
                physical.snaps[resource_owner] = Some(domain.snaps()[0]);
            }
            domain.check_fixed(pool, &physical).unwrap();
            // Only put the independent resource enumeration in the existing traversal's coordinate system.
            // The native order oracle below still visits every original permutation independently.
            let mut nonleaders: Vec<_> =
                SLOTS[1..].iter().map(|&slot| (physical.members[slot], physical.snaps[slot])).collect();
            nonleaders.sort_by_key(|&(member, snap)| {
                bounds.choices.iter().position(|&pair| pair == (member, usize::from(snap.is_some()))).unwrap()
            });
            for (&slot, (member, snap)) in SLOTS[1..].iter().zip(nonleaders) {
                physical.members[slot] = member;
                physical.snaps[slot] = snap;
            }
            rows.push(physical);
        }
    }
    assert_eq!(rows.len(), 12);
    rows
}

fn start_of(bounds: &JointBounds, domain: &CandidateDomain, physical: &PhysicalDeck, depth: usize) -> usize {
    let slot = SLOTS[depth - 1];
    let choices = JointBounds::prefix_choices(domain, physical, depth);
    bounds.choices.iter().position(|&pair| pair == (physical.members[slot], choices[slot])).unwrap() + 1
}

#[test]
fn raw_node_packets_cover_every_native_descendant_and_all_original_order_caps() {
    let (master, owned, request) = fixture(false);
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let rows = descendants(&pool, &domain, &bounds);
    let orders = uniform::all_orders();
    let mut oracles = Vec::new();
    for physical in &rows {
        let input = expectation::context(&pool, physical, &request.objective).unwrap();
        let power = i64::from(input.params.total_power);
        let live = input.into_ordered();
        let (mut native, mut raw) = (0i128, 0i128);
        for order in &orders {
            let model = live.simulate(&master, order, LiveRandom::new(0)).unwrap();
            assert_eq!(model.draws(), 0);
            let upper = bounds.raw_upper(&domain, physical, power, &uniform::positions_of(order)).unwrap();
            assert!(i128::from(model.score()) <= upper);
            native += i128::from(model.score());
            raw += upper;
        }
        oracles.push((power, native, raw));
    }
    let mut checks = 0;
    let mut tightened = 0;
    for depth in [3, 4] {
        for physical in &rows {
            let start = start_of(&bounds, &domain, physical, depth);
            let matching: Vec<_> = rows
                .iter()
                .enumerate()
                .filter(|(_, other)| {
                    SLOTS[..depth].iter().all(|&slot| {
                        physical.members[slot] == other.members[slot] && physical.snaps[slot] == other.snaps[slot]
                    })
                })
                .collect();
            let (previous, power) =
                bounds.expected_upper(&pool, &domain, physical, depth, &uniform::MEAN_ORDERS).unwrap();
            let cap = bounds
                .raw_node_upper(&pool, &domain, physical, depth, start, power, previous, &mut || false)
                .unwrap()
                .expect("all suffix packets have fixed judgements");
            let uncut = bounds
                .raw_node_upper(&pool, &domain, physical, depth, start, power, i128::MAX, &mut || false)
                .unwrap()
                .unwrap();
            assert!(cap <= previous && cap <= uncut);
            tightened += usize::from(cap < previous);
            for (index, _) in matching {
                let (native_power, native, raw) = oracles[index];
                assert!(power >= native_power);
                assert!(cap >= native, "depth {depth}: {cap} < native {native}");
                assert!(uncut >= raw, "position means and deterministic work must cover all 120 raw caps");
                checks += 1;
            }
        }
    }
    assert!(checks >= 24);
    assert!(tightened > 0, "a valid node packet must also supply a useful stricter envelope in this fixture");
}

#[test]
fn raw_node_conversion_in_one_allowed_suffix_refuses_the_entire_optional_bound() {
    let (master, owned, request) = fixture(true);
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let mut bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    // A legal complete suffix with the sixth-member converter follows the fifth member. Keeping the
    // converter in the tail must defeat the capability even though another complete descendant is admitted.
    bounds.choices.sort_unstable();
    let physical = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [None; 5] };
    let start = start_of(&bounds, &domain, &physical, 4);
    assert!(bounds.fine.as_ref().unwrap().raw_node_packet(4, 0).is_some());
    assert!(bounds.fine.as_ref().unwrap().raw_node_packet(5, 0).is_none());
    assert_eq!(
        bounds.raw_node_upper(&pool, &domain, &physical, 4, start, 100_000, i128::MAX, &mut || false),
        Some(None)
    );
    let mut converter = physical;
    converter.members[4] = 5;
    domain.check_fixed(&pool, &converter).unwrap();
    assert!(
        uniform::all_orders()
            .iter()
            .any(|order| { bounds.raw_upper(&domain, &converter, 100_000, &uniform::positions_of(order)).is_none() })
    );
}

#[test]
fn raw_node_cancellation_overflow_unsupported_objectives_and_strict_ties_keep_old_proof() {
    let (master, owned, request) = fixture(false);
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let mut bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let physical = descendants(&pool, &domain, &bounds)[0];
    let start = start_of(&bounds, &domain, &physical, 4);
    assert_eq!(bounds.raw_node_upper(&pool, &domain, &physical, 4, start, 100_000, i128::MAX, &mut || true), None);
    let mut calls = 0;
    assert_eq!(
        bounds.raw_node_upper(&pool, &domain, &physical, 4, start, 100_000, i128::MAX, &mut || {
            calls += 1;
            calls == 3 // cancellation after packet collection, before a certificate can be published
        }),
        None
    );
    assert_eq!(
        bounds.raw_node_upper(&pool, &domain, &physical, 4, start, i64::MAX, i128::MAX, &mut || false),
        Some(None)
    );
    let cap = bounds.raw_node_upper(&pool, &domain, &physical, 4, start, 100_000, 100, &mut || false).unwrap().unwrap();
    assert_eq!(cap, 100, "a stronger complete previous envelope is retained");
    for cutoff in [cap - 1, cap, cap + 1] {
        for lower_power in [false, true] {
            let excluded = cap < cutoff || (cap == cutoff && lower_power);
            assert_eq!(excluded, cutoff > cap || (cutoff == cap && lower_power));
        }
    }
    bounds.best_order = true;
    assert_eq!(
        bounds.raw_node_upper(&pool, &domain, &physical, 4, start, 100_000, i128::MAX, &mut || false),
        Some(None)
    );
}
