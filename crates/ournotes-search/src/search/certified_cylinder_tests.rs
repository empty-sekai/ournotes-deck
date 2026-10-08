//! Full native laws independently check partial-path evidence and its all-label aggregation.
use super::*;
use crate::search::expectation::ExactExpectation;
use crate::search::interval_topk::{
    CandidateInterval, CanonicalTie, IntervalTopK, RemainingDomain, compare_exact, exact_in_interval,
};
use ournotes_sim::live::certified::F64Interval;
use ournotes_sim::live::full::{
    LuckCylinderChoice, LuckExactAtom, LuckExactBudget, LuckExactLaw, LuckExactMass, LuckExactSession,
    LuckTerminalCylinder,
};

fn session<'a>(master: &'a Master, input: &'a FiniteSeedContext) -> LuckExactSession<'a> {
    LuckExactSession::new(
        master,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        input.rank_confirmations.as_deref(),
        0,
    )
    .unwrap()
}

fn raw_rows(master: &Master, skills: &LuckSkills, input: &FiniteSeedContext) -> Vec<OrderScoreInterval> {
    let result = evaluate_luck_context(master, skills, input, &PayoffMap::Score, None, || false)
        .unwrap()
        .expect("complete raw native-domain certificates");
    assert_eq!(result.orders.iter().map(|row| row.order).collect::<Vec<_>>(), uniform::all_orders());
    assert!(result.orders.iter().all(|row| row.evaluated));
    result.orders
}

fn fraction(mass: LuckExactMass) -> ExactExpectation {
    ExactExpectation { numerator: i128::try_from(mass.numerator).unwrap(), denominator: mass.denominator }
}

fn divisor(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

// Independent reduced rational summation, without the production payoff or enclosure helpers.
fn sum(values: impl IntoIterator<Item = ExactExpectation>) -> ExactExpectation {
    let mut out = ExactExpectation { numerator: 0, denominator: 1 };
    for value in values {
        let d = out.denominator.checked_mul(value.denominator / divisor(out.denominator, value.denominator)).unwrap();
        let n = out.numerator.checked_mul(i128::try_from(d / out.denominator).unwrap()).unwrap()
            + value.numerator.checked_mul(i128::try_from(d / value.denominator).unwrap()).unwrap();
        let g = divisor(n.unsigned_abs(), d);
        out = ExactExpectation { numerator: n / i128::try_from(g).unwrap(), denominator: d / g };
    }
    out
}

fn payoff(atom: &LuckExactAtom, map: &PayoffMap) -> i128 {
    match *map {
        PayoffMap::ScoreAtLeast { threshold } => i128::from(atom.score >= threshold),
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life } => {
            i128::from(atom.score >= threshold && atom.final_life >= min_final_life)
        }
        PayoffMap::CappedScore { threshold } => i128::from(atom.score.min(threshold)),
        _ => unreachable!(),
    }
}

fn expectation(law: &LuckExactLaw, map: &PayoffMap) -> ExactExpectation {
    sum(law.atoms().iter().map(|atom| ExactExpectation {
        numerator: i128::try_from(atom.mass.numerator).unwrap().checked_mul(payoff(atom, map)).unwrap(),
        denominator: atom.mass.denominator,
    }))
}

fn raw_identity(row: &OrderScoreInterval) -> String {
    format!("{:?}", (row.order, row.evaluated, row.mean, row.support, row.exact_mean, row.final_life, &row.tails))
}

fn broad_row(row: &OrderScoreInterval) -> OrderScoreInterval {
    let mut row = row.clone();
    row.support = (i32::MIN, i32::MAX);
    row.mean = F64Interval::new(f64::from(i32::MIN), f64::from(i32::MAX)).unwrap();
    row.exact_mean = None;
    row.final_life = None;
    row.tails.clear();
    row.refined_payoff = None;
    row
}

#[test]
fn terminal_cylinder_search_both_selectors_enclose_full_native_laws_for_all_120_labels() {
    let (master, skills, input) = input_with_order_skills(true);
    let rows = raw_rows(&master, &skills, &input);
    let mut native = session(&master, &input);
    let mut work = LuckExactBudget::default();
    let laws: Vec<_> = rows
        .iter()
        .map(|row| {
            let performers = row.order.map(|slot| input.performers[slot].clone());
            native.law(&performers, &mut work, || false).unwrap().law.expect("complete native oracle")
        })
        .collect();
    let witness = laws
        .iter()
        .find(|law| law.atoms().iter().any(|atom| atom.score != law.atoms()[0].score))
        .expect("a threshold inside actual nonconstant native support");
    let lo = witness.atoms().iter().map(|atom| atom.score).min().unwrap();
    let hi = witness.atoms().iter().map(|atom| atom.score).max().unwrap();
    let threshold = lo + (hi - lo + 1) / 2;
    let life = witness.atoms()[0].final_life;
    let maps = [
        PayoffMap::ScoreAtLeast { threshold },
        PayoffMap::CappedScore { threshold },
        PayoffMap::CappedScore { threshold: -3 },
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: life },
        PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life: life.checked_add(1).unwrap() },
    ];
    let mut refined: Vec<_> = maps.iter().map(|_| rows.clone()).collect();
    let mut selections = [0; 2];
    for (index, (raw, law)) in rows.iter().zip(&laws).enumerate() {
        let performers = raw.order.map(|slot| input.performers[slot].clone());
        for (selector, choice) in [LuckCylinderChoice::First, LuckCylinderChoice::Last].into_iter().enumerate() {
            let attempt = native.terminal_cylinder(&performers, choice, &mut work, || false).unwrap();
            assert!(attempt.decline.is_none());
            assert_eq!(attempt.stats.terminal_paths, 1);
            let cylinder = attempt.cylinder.expect("one fully terminated positive-mass path");
            let atom = cylinder.atom();
            assert!(atom.mass.numerator > 0 && atom.mass.numerator <= atom.mass.denominator);
            let full_atom = law
                .atoms()
                .iter()
                .find(|other| (other.score, other.final_life) == (atom.score, atom.final_life))
                .expect("the independently enumerated native law contains this terminal");
            assert!(!compare_exact(fraction(atom.mass), fraction(full_atom.mass)).unwrap().is_gt());
            selections[selector] += 1;
            for (map, rows) in maps.iter().zip(&mut refined) {
                let row = &mut rows[index];
                let before = raw_identity(row);
                refine_order_with_terminal_cylinder(row, map, &cylinder).unwrap();
                assert_eq!(raw_identity(row), before, "cylinders change only mapped payoff evidence");
                let expected = expectation(law, map);
                if let Some(payoff) = &row.refined_payoff {
                    assert!(exact_in_interval(expected, payoff.bounds).unwrap());
                    if let Some(exact) = payoff.exact {
                        // The complete raw support may already prove a constant map (e.g. cap -3).
                        assert!(compare_exact(exact, expected).unwrap().is_eq());
                    }
                }
                let installed = format!("{:?}", row.refined_payoff);
                assert!(!refine_order_with_terminal_cylinder(row, map, &cylinder).unwrap());
                assert_eq!(format!("{:?}", row.refined_payoff), installed, "repeated mass is not accumulated");
                // Exact-law fallback still works after every partial certificate and preserves it.
                let mut completed = row.clone();
                assert!(refine_order_with_exact_law(&mut completed, map, law).unwrap());
                let before = raw_identity(&completed);
                let exact = completed.refined_payoff.as_ref().unwrap().exact.unwrap();
                assert!(compare_exact(exact, expected).unwrap().is_eq());
                refine_order_with_terminal_cylinder(&mut completed, map, &cylinder).unwrap();
                assert_eq!(raw_identity(&completed), before);
                assert_eq!(completed.refined_payoff.as_ref().unwrap().exact, Some(exact));
            }
        }
    }
    assert_eq!(selections, [uniform::ORDERS; 2]);
    for (map, rows) in maps.iter().zip(refined) {
        let actual = aggregate_orders(rows, map).unwrap();
        let total = sum(laws.iter().map(|law| expectation(law, map)));
        let expected = ExactExpectation {
            numerator: total.numerator,
            denominator: total.denominator.checked_mul(uniform::ORDERS as u128).unwrap(),
        };
        assert!(exact_in_interval(expected, actual.payoff).unwrap());
        assert_eq!(actual.orders.len(), uniform::ORDERS);
        assert_eq!(actual.orders.iter().map(|row| row.order).collect::<Vec<_>>(), uniform::all_orders());
    }
}

#[test]
fn terminal_cylinder_search_rejects_pending_and_support_contradictions_atomically() {
    let (master, skills, input) = input_with_order_skills(true);
    let rows = raw_rows(&master, &skills, &input);
    let native = session(&master, &input);
    let order = rows[0].order;
    let performers = order.map(|slot| input.performers[slot].clone());
    let cylinder = native
        .terminal_cylinder(&performers, LuckCylinderChoice::Last, &mut LuckExactBudget::default(), || false)
        .unwrap()
        .cylinder
        .unwrap();
    let atom = cylinder.atom();
    let map = PayoffMap::ScoreAtLeast { threshold: atom.score.checked_add(1).unwrap() };
    for kind in 0..5 {
        let mut row = broad_row(&rows[0]);
        let map = match kind {
            0 => {
                row.evaluated = false;
                map.clone()
            }
            1 => {
                row.support = (atom.score + 1, atom.score + 1);
                row.mean = F64Interval::integer(i128::from(atom.score) + 1);
                map.clone()
            }
            2 => {
                row.final_life = Some((atom.final_life + 1, atom.final_life + 1));
                map.clone()
            }
            3 => PayoffMap::Score,
            4 => {
                // A false already-installed exact value is a contradiction, not permission
                // to silently replace it by a compatible partial cylinder's bound.
                row.refine_payoff(PayoffRefinement {
                    map: map.clone(),
                    bounds: F64Interval::ONE,
                    exact: Some(ExactExpectation { numerator: 1, denominator: 1 }),
                })
                .unwrap();
                map.clone()
            }
            _ => unreachable!(),
        };
        row.validate().unwrap();
        let before = format!("{row:?}");
        assert!(refine_order_with_terminal_cylinder(&mut row, &map, &cylinder).is_err(), "case {kind}");
        assert_eq!(format!("{row:?}"), before, "failed evidence is atomic, case {kind}");
    }
}

fn candidate(id: u64, power: i32, evaluation: &CertifiedEvaluation) -> CandidateInterval {
    CandidateInterval {
        id,
        tie: CanonicalTie { power, key: vec![id as i64] },
        score: evaluation.score,
        payoff: evaluation.payoff,
        exact_score: evaluation.exact_score,
        exact_payoff: evaluation.exact_payoff,
        equality: None,
        revision: 0,
    }
}

fn install(frontier: &mut IntervalTopK, id: u64, result: &CertifiedEvaluation) {
    let old = frontier.get(id).unwrap();
    frontier
        .refine(
            id,
            old.revision,
            old.score.intersect(result.score).unwrap(),
            old.payoff.intersect(result.payoff).unwrap(),
            result.exact_score,
            result.exact_payoff,
        )
        .unwrap();
}

#[test]
fn terminal_cylinder_search_frontier_uses_all_labels_and_never_closes_an_open_domain() {
    let (master, skills, input) = input_with_order_skills(true);
    let raw = raw_rows(&master, &skills, &input);
    let native = session(&master, &input);
    let performers = raw[0].order.map(|slot| input.performers[slot].clone());
    let mut work = LuckExactBudget::default();
    let cylinders: Vec<LuckTerminalCylinder> = [LuckCylinderChoice::First, LuckCylinderChoice::Last]
        .into_iter()
        .map(|choice| native.terminal_cylinder(&performers, choice, &mut work, || false).unwrap().cylinder.unwrap())
        .collect();
    let success = cylinders.iter().max_by_key(|c| c.atom().score).unwrap();
    let failure = cylinders.iter().min_by_key(|c| c.atom().score).unwrap();
    assert!(failure.atom().score < success.atom().score, "select a genuinely splitting native threshold");
    let map = PayoffMap::ScoreAtLeast { threshold: success.atom().score };
    let mut rows: Vec<_> = raw.iter().map(broad_row).collect();
    let initial = aggregate_orders(rows.clone(), &map).unwrap();
    let mut frontier = IntervalTopK::new(1).unwrap();
    // The stronger canonical tie belongs to the unresolved contender, so a ceiling incumbent
    // cannot remove it while its upper bound remains one.
    frontier.insert(candidate(1, 20, &initial)).unwrap();
    let mut incumbent = candidate(2, 10, &initial);
    incumbent.payoff = F64Interval::ONE;
    incumbent.exact_payoff = Some(ExactExpectation { numerator: 1, denominator: 1 });
    frontier.insert(incumbent).unwrap();
    assert!(refine_order_with_terminal_cylinder(&mut rows[0], &map, success).unwrap());
    let after_success = aggregate_orders(rows.clone(), &map).unwrap();
    assert_eq!(after_success.payoff.upper(), 1.0);
    assert!(after_success.exact_payoff.is_none());
    install(&mut frontier, 1, &after_success);
    assert!(frontier.get(1).is_some(), "one success path cannot discard a canonically better contender");
    assert!(!frontier.proof(RemainingDomain::Exhausted).unwrap().complete);
    assert!(refine_order_with_terminal_cylinder(&mut rows[0], &map, failure).unwrap());
    let after_failure = aggregate_orders(rows.clone(), &map).unwrap();
    assert!(after_failure.payoff.upper() < 1.0);
    let p = failure.atom().mass;
    let denominator = p.denominator.checked_mul(uniform::ORDERS as u128).unwrap();
    let all_label_ceiling =
        ExactExpectation { numerator: i128::try_from(denominator - p.numerator).unwrap(), denominator };
    assert!(exact_in_interval(all_label_ceiling, after_failure.payoff).unwrap());
    assert!(aggregate_orders(rows[..uniform::ORDERS - 1].to_vec(), &map).is_err());
    rows[1].order = rows[0].order;
    assert!(aggregate_orders(rows, &map).is_err(), "a duplicate label cannot stand in for missing mass");
    install(&mut frontier, 1, &after_failure);
    assert!(frontier.get(1).is_none(), "only the complete 120-label bound authorizes exclusion");
    assert_eq!(frontier.proof(RemainingDomain::Exhausted).unwrap().ordered_prefix, vec![2]);
    assert!(!frontier.proof(RemainingDomain::Open { upper: Some(1.0) }).unwrap().complete);
    assert!(!frontier.proof(RemainingDomain::Open { upper: None }).unwrap().complete);
    // A later, truly ceiling-valued canonical winner is still admissible after the early exclusion.
    let mut later = candidate(3, 30, &initial);
    later.payoff = F64Interval::ONE;
    later.exact_payoff = Some(ExactExpectation { numerator: 1, denominator: 1 });
    frontier.insert(later).unwrap();
    assert_eq!(frontier.proof(RemainingDomain::Exhausted).unwrap().ordered_prefix, vec![3]);
}
