//! The default joint traversal must use a family exclusion and still prove the native canonical Top-K.
use super::*;
use crate::types::{ExitReason, Fraction, FractionInterval, Limits, Optimality, RecommendationOutcome, Strategy};

fn end_to_end_fixture() -> (Master, Roster, SearchRequest) {
    let (mut master, mut owned, request) = fixture();
    // This separate synthetic case leaves a gap between an all-Rush support cap and a Rush-weighted mean.
    // The sixth card's moderate power advantage is enough to create an incumbent, without making every low
    // family disappear through a much earlier power bound. It does not alter the main descendant-cap fixture.
    let sixth = master.member_cards.iter_mut().find(|row| row.id == 6).unwrap();
    sixth.performance_power_max = 4_000;
    sixth.technic_power_max = 4_000;
    sixth.visual_power_max = 4_000;
    master.live_settings.iter_mut().find(|row| row.key == "gekisou_luck_rush_score_bonus_percent").unwrap().value =
        "1000".into();
    for row in &mut master.gekisou_ranking_score_bonuses {
        row.score_bonus_percent = 0;
    }
    master.reindex().unwrap();
    // Neither member nor Snap pool indexes coincide with the public ID order. Actual performance orders still
    // contain the complete initialized performers; these arrays only change the roster's transport order.
    owned.members.rotate_left(2);
    owned.snaps.reverse();
    (master, owned, request)
}

fn public_id_descendants(pool: &Pool, domain: &CandidateDomain) -> Vec<PhysicalDeck> {
    let member = |id| pool.members.iter().position(|row| row.id == id).unwrap();
    let snap = |id| pool.snaps.iter().position(|row| row.id == id).unwrap();
    let resources = [snap(1), snap(2)];
    fn bind(
        pool: &Pool,
        domain: &CandidateDomain,
        resources: &[usize; 2],
        deck: &mut PhysicalDeck,
        slot: usize,
        out: &mut Vec<PhysicalDeck>,
    ) {
        if slot == 5 {
            domain.check_fixed(pool, deck).unwrap();
            out.push(*deck);
            return;
        }
        deck.snaps[slot] = None;
        bind(pool, domain, resources, deck, slot + 1, out);
        for &resource in resources {
            if deck.snaps[..slot].contains(&Some(resource)) {
                continue;
            }
            deck.snaps[slot] = Some(resource);
            bind(pool, domain, resources, deck, slot + 1, out);
        }
    }
    let mut out = Vec::new();
    for last in [5, 6] {
        let members = [2, 3, 1, 4, last].map(member);
        bind(pool, domain, &resources, &mut PhysicalDeck { members, snaps: [None; 5] }, 0, &mut out);
    }
    assert_eq!(out.len(), 62);
    assert_eq!(out.iter().copied().collect::<std::collections::BTreeSet<_>>().len(), 62);
    out
}

struct NativeRank {
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    power: i32,
    mean: Rational,
    nonconstant: bool,
}

fn compare_rational(left: Rational, right: Rational) -> std::cmp::Ordering {
    left.0.checked_mul(right.1).unwrap().cmp(&right.0.checked_mul(left.1).unwrap())
}

fn native_public_ranking(pool: &Pool, request: &SearchRequest, domain: &CandidateDomain) -> Vec<NativeRank> {
    let orders = independent_orders();
    let mut ranking = Vec::new();
    let mut checked = 0usize;
    for physical in public_id_descendants(pool, domain) {
        let input = expectation::context(pool, &physical, &request.objective).unwrap();
        let mut session = LuckExactSession::new(
            pool.master,
            &input.notes,
            &input.events,
            input.params,
            input.gekisou.as_ref().unwrap(),
            &input.play,
            &input.delta_times,
            None,
            0,
        )
        .unwrap();
        let mut sum = Rational::ZERO;
        let mut nonconstant = false;
        for order in &orders {
            let performers = order.map(|slot| input.performers[slot].clone());
            let attempt = session.law(&performers, &mut LuckExactBudget::default(), || false).unwrap();
            let law = attempt.law.unwrap_or_else(|| {
                panic!(
                    "complete E2E native oracle declined {:?}; stats={:?}; members={:?}; snaps={:?}; order={order:?}",
                    attempt.decline, attempt.stats, physical.members, physical.snaps
                )
            });
            nonconstant |= law.atoms().windows(2).any(|pair| pair[0].score != pair[1].score);
            let mut mass = Rational::ZERO;
            for atom in law.atoms() {
                let numerator = i128::try_from(atom.mass.numerator).unwrap();
                let denominator = i128::try_from(atom.mass.denominator).unwrap();
                mass = mass.add(Rational::new(numerator, denominator));
                sum = sum.add(Rational::new(numerator.checked_mul(i128::from(atom.score)).unwrap(), denominator));
            }
            assert_eq!(mass, Rational::ONE);
            checked += 1;
        }
        ranking.push(NativeRank {
            members: physical.members.map(|member| pool.members[member].id),
            snaps: physical.snaps.map(|snap| snap.map(|snap| pool.snaps[snap].id)),
            power: input.params.total_power,
            mean: Rational::new(sum.0, sum.1.checked_mul(120).unwrap()),
            nonconstant,
        });
    }
    assert_eq!(checked, 62 * 120);
    // This is the declared canonical ranking, independently of the search's frontier and its cached upper.
    ranking.sort_by(|a, b| {
        compare_rational(b.mean, a.mean)
            .then_with(|| b.power.cmp(&a.power))
            .then_with(|| a.members.cmp(&b.members))
            .then_with(|| a.snaps.cmp(&b.snaps))
    });
    assert!(ranking.windows(2).any(|pair| pair[0].mean != pair[1].mean), "candidate means must not be constant");
    assert!(ranking.iter().take(request.k).all(|rank| rank.nonconstant), "all K retained laws remain nonconstant");
    ranking
}

fn wire_rational(value: &Fraction) -> Rational {
    Rational::new(value.numerator.parse().unwrap(), value.denominator.parse().unwrap())
}

fn encloses(interval: &FractionInterval, mean: Rational) {
    assert!(!compare_rational(wire_rational(&interval.lower), mean).is_gt());
    assert!(!compare_rational(mean, wire_rational(&interval.upper)).is_gt());
}

fn assert_complete_native_top_k(actual: &RecommendationOutcome, oracle: &[NativeRank], k: usize) {
    assert_eq!(actual.completion, crate::search::Completion::Complete, "{:?}", actual.telemetry.lottery_refinement);
    assert_eq!(actual.exit_reason, ExitReason::Exhausted);
    assert_eq!(actual.optimality, Optimality::Proven);
    assert!(actual.telemetry.proof.complete);
    assert_eq!(actual.result_identity, "team");
    assert_eq!(
        actual.probability_law,
        json!({
            "kind":"uniformMemberOrder", "orders":120, "lottery":"certifiedNativeLotteryIntervals"
        })
    );
    assert_eq!(actual.results.len(), k);
    for (team, expected) in actual.results.iter().zip(oracle) {
        assert_eq!((team.members, team.snaps, team.power), (expected.members, expected.snaps, expected.power));
        assert_eq!(team.rank_certified, Some(true));
        encloses(team.score_interval.as_ref().expect("complete certified score enclosure"), expected.mean);
        encloses(team.payoff_interval.as_ref().expect("Score has the same expected payoff"), expected.mean);
        for exact in [&team.expected_score, &team.expected_payoff].into_iter().flatten() {
            assert_eq!(wire_rational(exact), expected.mean);
        }
    }
}

#[test]
fn default_family_pruning_proves_the_native_canonical_top_k_with_shuffled_roster() {
    let (master, owned, request) = end_to_end_fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    assert_eq!(pool.members.iter().map(|member| member.id).collect::<Vec<_>>(), [3, 4, 5, 6, 1, 2]);
    assert_eq!(pool.snaps.iter().map(|snap| snap.id).collect::<Vec<_>>(), [2, 1]);
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let oracle = native_public_ranking(&pool, &request, &domain);
    let mut answers = Vec::new();
    for cache_entries in [64, 0] {
        let actual = crate::search::physical::solve_physical(
            &pool,
            &request,
            &Metric::Score,
            None,
            &Limits { time_limit_ms: None, max_candidates: None, cache_entries },
            &Strategy::BranchAndBound,
            None,
            &SimulationInput::default(),
        )
        .unwrap();
        assert_complete_native_top_k(&actual, &oracle, request.k);
        let module = actual.telemetry.joint.modules.get("luckFamily");
        if cache_entries != 0 {
            assert!(actual.telemetry.joint.luck_family.prepared_families > 0);
            assert!(
                module.is_some_and(|module| module.pruned > 0),
                "the default traversal must actually prune through the probability family: {}",
                serde_json::to_string(&actual.telemetry.joint).unwrap()
            );
        } else {
            assert_eq!(actual.telemetry.joint.luck_family.context_checks, 0);
            assert_eq!(actual.telemetry.joint.luck_family.prepared_families, 0);
            assert!(module.is_none_or(|module| module.checks == 0 && module.pruned == 0));
        }
        answers.push(actual);
    }
    let identities = |answer: &RecommendationOutcome| {
        answer.results.iter().map(|team| (team.members, team.snaps, team.power)).collect::<Vec<_>>()
    };
    assert_eq!(identities(&answers[0]), identities(&answers[1]));
    assert_eq!(answers[0].probability_law, answers[1].probability_law);
    assert_eq!(answers[0].result_identity, answers[1].result_identity);
    // A candidate budget still terminates the ordinary proof. A completed upper table is not a candidate law,
    // never fills Top-K, and cannot change CandidateLimit into Exhausted.
    let limited = crate::search::physical::solve_physical(
        &pool,
        &request,
        &Metric::Score,
        None,
        &Limits { time_limit_ms: None, max_candidates: Some(1), cache_entries: 64 },
        &Strategy::BranchAndBound,
        None,
        &SimulationInput::default(),
    )
    .unwrap();
    assert_eq!(limited.completion, crate::search::Completion::TimedOut);
    assert_eq!(limited.exit_reason, ExitReason::CandidateLimit);
    assert_eq!(limited.optimality, Optimality::Unproven);
    assert!(!limited.telemetry.proof.complete);
    assert!(limited.results.len() < request.k);
}
