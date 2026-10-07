//! A cover is over labelled profile/order identities, not just a number of completed computations.
use super::{FamilyCoverage, LuckFamilyDecline, writer_profiles};

#[test]
fn two_writer_profiles_equal_the_complete_physical_assignment_projection() {
    let allowed: [Vec<Option<usize>>; 5] = std::array::from_fn(|_| vec![None, Some(7), Some(11), Some(19)]);
    let profiles = writer_profiles(&[7, 11], &allowed).unwrap();
    let actual: std::collections::BTreeSet<_> = profiles.iter().copied().collect();
    assert_eq!(profiles.len(), 31);
    assert_eq!(actual.len(), 31, "equal source programs never merge physical resource labels");
    let mut expected = std::collections::BTreeSet::new();
    for code in 0..4usize.pow(5) {
        let mut digits = code;
        let resources: [Option<usize>; 5] = std::array::from_fn(|_| {
            let resource = [None, Some(7), Some(11), Some(19)][digits % 4];
            digits /= 4;
            resource
        });
        if resources.iter().flatten().any(|resource| resources.iter().filter(|r| **r == Some(*resource)).count() > 1) {
            continue;
        }
        expected.insert(resources.map(|resource| resource.filter(|&resource| resource == 7 || resource == 11)));
    }
    assert_eq!(actual, expected);
    let restricted = [vec![None, Some(7)], vec![None], vec![None, Some(11)], vec![None], vec![None]];
    assert_eq!(writer_profiles(&[7, 11], &restricted).unwrap().len(), 4);
    assert_eq!(writer_profiles(&[7, 11, 19], &allowed).unwrap_err().reason, LuckFamilyDecline::WriterProfiles);
}

#[test]
fn two_writer_coverage_retains_every_original_order_in_every_owner_vector() {
    let mut coverage = FamilyCoverage::new(31).unwrap();
    for profile in 0..31 {
        for order in 0..120 {
            if (profile, order) != (30, 119) {
                assert!(coverage.mark(profile, order));
            }
        }
    }
    assert!(!coverage.complete());
    assert!(!coverage.mark(0, 119));
    assert!(!coverage.complete(), "a duplicate cannot replace the missing dual-writer order");
    assert!(coverage.mark(30, 119));
    assert!(coverage.complete());
}

#[test]
fn one_missing_label_cannot_be_replaced_by_duplicate_completed_work() {
    // The word boundary is deliberately included, while the assertions only use the public 120-order contract.
    for missing in [(0, 0), (1, 63), (3, 64), (5, 119)] {
        let mut coverage = FamilyCoverage::new(6).unwrap();
        let mut submitted = 0;
        for profile in 0..6 {
            for order in 0..120 {
                if (profile, order) == missing {
                    continue;
                }
                assert!(coverage.mark(profile, order));
                submitted += 1;
            }
        }
        assert_eq!(submitted, 719);
        assert!(!coverage.complete());
        let duplicate = if missing == (0, 0) { (0, 1) } else { (0, 0) };
        assert!(!coverage.mark(duplicate.0, duplicate.1));
        submitted += 1;
        assert_eq!(submitted, 720, "the raw computation count alone looks complete");
        assert!(!coverage.complete(), "a duplicate never proves the missing original order");
        assert!(coverage.mark(missing.0, missing.1));
        assert!(coverage.complete());
    }
}

#[test]
fn a_whole_missing_writer_owner_profile_never_yields_a_complete_family() {
    let mut coverage = FamilyCoverage::new(6).unwrap();
    for profile in [0, 1, 2, 3, 5] {
        for order in 0..120 {
            assert!(coverage.mark(profile, order));
        }
    }
    // Repeat an existing profile to reach the same number of submissions as a complete product.
    for order in 0..120 {
        assert!(!coverage.mark(2, order));
    }
    assert!(!coverage.complete());
    for order in (0..120).rev() {
        assert!(coverage.mark(4, order));
    }
    assert!(coverage.complete(), "arrival order does not replace the original labelled cover");
}

#[test]
fn empty_or_out_of_domain_coverage_is_never_ready() {
    let mut empty = FamilyCoverage::new(0).unwrap();
    assert!(!empty.complete());
    assert!(!empty.mark(0, 0));
    let mut coverage = FamilyCoverage::new(1).unwrap();
    for (profile, order) in [(1, 0), (0, 120), (usize::MAX, 0), (0, usize::MAX)] {
        assert!(!coverage.mark(profile, order));
        assert!(!coverage.complete());
    }
    for order in 0..120 {
        assert!(coverage.mark(0, order));
    }
    assert!(coverage.complete());
    assert!(!coverage.mark(0, 120));
    assert!(coverage.complete(), "an invalid extra label cannot mutate a previously completed cover");
}
