//! An independently supplied complete-law proof joins existing classes, never physical candidates.
use super::*;

fn candidate(id: u64, equality: EqualityCertificate, lower: f64, upper: f64) -> CandidateInterval {
    CandidateInterval {
        id,
        tie: CanonicalTie { power: equality.power, key: vec![id as i64] },
        score: F64Interval::new(lower, upper).unwrap(),
        payoff: F64Interval::new(lower, upper).unwrap(),
        exact_score: None,
        exact_payoff: None,
        equality: Some(equality),
        revision: 0,
    }
}

#[test]
fn complete_equality_merge_keeps_canonical_physical_ties_and_does_not_close_unseen_work() {
    let mut frontier = IntervalTopK::new(3).unwrap();
    let a = frontier.certify_equal_program(b"complete-A".to_vec(), 100, b"Score".to_vec());
    let b = frontier.certify_equal_program(b"complete-B".to_vec(), 100, b"Score".to_vec());
    for id in [5, 2, 6, 1, 4, 3] {
        let (certificate, lower, upper) = if id % 2 == 1 { (a.clone(), 10.0, 12.0) } else { (b.clone(), 11.0, 13.0) };
        frontier.insert(candidate(id, certificate, lower, upper)).unwrap();
    }
    assert_eq!(frontier.len(), 6);
    assert!(!frontier.proof(RemainingDomain::Exhausted).unwrap().complete);
    frontier.merge_equal_classes(5, 6).unwrap();
    assert_eq!(frontier.candidates.keys().copied().collect::<Vec<_>>(), [1, 2, 3]);
    for candidate in frontier.candidates.values() {
        assert_eq!(candidate.tie.key, [candidate.id as i64]);
        assert_eq!(candidate.score, F64Interval::new(11.0, 12.0).unwrap());
        assert_eq!(candidate.payoff, candidate.score);
        assert!(candidate.exact_score.is_none() && candidate.exact_payoff.is_none());
        assert_eq!(candidate.revision, 1);
    }
    for domain in [RemainingDomain::Open { upper: None }, RemainingDomain::Open { upper: Some(12.0) }] {
        assert!(!frontier.proof(domain).unwrap().complete);
    }
    let proof = frontier.proof(RemainingDomain::Exhausted).unwrap();
    assert!(proof.complete);
    assert_eq!(proof.ordered_prefix, [1, 2, 3]);
    assert_eq!(proof.pruned, 3);
    assert!(
        frontier
            .refine(1, 0, F64Interval::new(11.0, 12.0).unwrap(), F64Interval::new(11.0, 12.0).unwrap(), None, None)
            .is_err()
    );

    // Future interning of either complete program retains the joined class, with its real physical key.
    let b = frontier.certify_equal_program(b"complete-B".to_vec(), 100, b"Score".to_vec());
    frontier.insert(candidate(0, b, 11.0, 12.0)).unwrap();
    assert_eq!(frontier.proof(RemainingDomain::Exhausted).unwrap().ordered_prefix, [0, 1, 2]);
}

#[test]
fn incompatible_equality_merges_are_atomic() {
    for cause in 0..6 {
        let mut frontier = IntervalTopK::new(3).unwrap();
        let a = frontier.certify_equal_program(b"A".to_vec(), 100, b"Score".to_vec());
        let b = frontier.certify_equal_program(
            b"B".to_vec(),
            if cause == 0 { 101 } else { 100 },
            if cause == 1 { b"Other".to_vec() } else { b"Score".to_vec() },
        );
        let mut a = candidate(1, a, 10.0, 13.0);
        let (lower, upper) = match cause {
            2 => (14.0, 15.0),
            4 => (11.5, 12.5),
            _ => (10.0, 13.0),
        };
        let mut b = candidate(2, b, lower, upper);
        if matches!(cause, 3 | 4) {
            a.exact_score = Some(ExactExpectation { numerator: 11, denominator: 1 });
            a.exact_payoff = a.exact_score;
        }
        if cause == 3 {
            b.exact_score = Some(ExactExpectation { numerator: 12, denominator: 1 });
            b.exact_payoff = b.exact_score;
        }
        if cause == 5 {
            b.revision = u64::MAX;
        }
        frontier.insert(a).unwrap();
        frontier.insert(b).unwrap();
        let before = format!("{:?}", frontier.candidates);
        let identities = frontier.identities.clone();
        let pruned = frontier.pruned;
        assert!(frontier.merge_equal_classes(1, 2).is_err(), "conflict {cause}");
        assert_eq!(format!("{:?}", frontier.candidates), before, "no partial class mutation on conflict {cause}");
        assert_eq!(frontier.identities, identities);
        assert_eq!(frontier.pruned, pruned);
    }
}

#[test]
fn already_equal_classes_do_not_repeat_a_refinement() {
    let mut frontier = IntervalTopK::new(2).unwrap();
    let equality = frontier.certify_equal_program(vec![1], 100, b"Score".to_vec());
    frontier.insert(candidate(1, equality.clone(), 10.0, 12.0)).unwrap();
    frontier.insert(candidate(2, equality, 10.0, 12.0)).unwrap();
    frontier.merge_equal_classes(1, 2).unwrap();
    assert_eq!(frontier.get(1).unwrap().revision, 0);
    assert_eq!(frontier.get(2).unwrap().revision, 0);
    assert_eq!(frontier.proof(RemainingDomain::Exhausted).unwrap().ordered_prefix, [1, 2]);
}

#[test]
fn redirected_certificates_cannot_bypass_payoff_scope_in_a_later_merge() {
    let mut frontier = IntervalTopK::new(10).unwrap();
    let a = frontier.certify_equal_program(vec![1], 100, b"Score".to_vec());
    let b = frontier.certify_equal_program(vec![2], 100, b"Score".to_vec());
    let c = frontier.certify_equal_program(vec![3], 100, b"Other".to_vec());
    let d = frontier.certify_equal_program(vec![4], 100, b"Other".to_vec());
    for (id, certificate) in [(1, a), (2, b.clone()), (3, c), (4, d.clone())] {
        frontier.insert(candidate(id, certificate, 10.0, 12.0)).unwrap();
    }
    frontier.merge_equal_classes(1, 2).unwrap();
    frontier.merge_equal_classes(3, 4).unwrap();
    // Earlier issued proofs remain valid but lose the newly established equivalence information.
    // Their old IDs no longer have an interned mapping; two missing mappings are not equal mappings.
    frontier.insert(candidate(5, b, 10.0, 12.0)).unwrap();
    frontier.insert(candidate(6, d, 10.0, 12.0)).unwrap();
    let before = format!("{:?}", frontier.candidates);
    assert!(frontier.merge_equal_classes(5, 6).is_err());
    assert_eq!(format!("{:?}", frontier.candidates), before);
}
