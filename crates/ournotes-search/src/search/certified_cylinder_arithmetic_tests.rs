//! A cylinder bounds the unexplored event by pathwise support, never by its prior mean.
use super::*;
use ournotes_sim::live::full::LuckExactMass;

fn mass(numerator: u128, denominator: u128) -> LuckExactMass {
    LuckExactMass { numerator, denominator }
}

#[test]
fn terminal_cylinder_arithmetic_keeps_unseen_payoff_support_separate_from_prior_expectation() {
    // One failed path has mass 1/10; all other paths can succeed. The true expectation 9/10
    // is inside the prior interval. Multiplying that prior's upper .95 by .9 would be false.
    let prior = F64Interval::new(0.85, 0.95).unwrap();
    let cylinder = cylinder_enclosure(mass(1, 10), 0, 0, 1).unwrap();
    let refined = prior.intersect(cylinder).unwrap();
    assert!(exact_in_interval(ExactExpectation { numerator: 9, denominator: 10 }, refined).unwrap());
    assert_eq!(refined.lower(), prior.lower());
    assert!(refined.upper() > 0.855 && refined.upper() < 0.91);
    // The same event can be returned twice, including by different deterministic selectors.
    assert_eq!(refined.intersect(cylinder), Some(refined));
    assert!(exact_in_interval(ExactExpectation { numerator: 9, denominator: 10 }, refined).unwrap());
}

#[test]
fn terminal_cylinder_arithmetic_covers_signed_capped_payoff_endpoints() {
    // For Y=min(S,-3), a cylinder Y=-7 of mass 1/4 and full support [-10,-3]
    // gives [-37/4,-4]. The remaining mass can attain either endpoint independently.
    let bounds = cylinder_enclosure(mass(1, 4), -7, -10, -3).unwrap();
    assert!(exact_in_interval(ExactExpectation { numerator: -37, denominator: 4 }, bounds).unwrap());
    assert!(exact_in_interval(fraction(-4), bounds).unwrap());
    assert!(bounds.lower() > -9.26 && bounds.upper() < -3.99);
    let certain = cylinder_enclosure(mass(1, 1), -7, -10, -3).unwrap();
    assert!(exact_in_interval(fraction(-7), certain).unwrap());
    assert!(certain.lower() > -7.01 && certain.upper() < -6.99);
}

#[test]
fn terminal_cylinder_arithmetic_tiny_positive_mass_cannot_force_a_binary64_gap() {
    let denominator = 1u128 << 100;
    let bounds = cylinder_enclosure(mass(1, denominator), 0, 0, 1).unwrap();
    let prior = F64Interval::new(0.0, 1.0).unwrap();
    let refined = prior.intersect(bounds).unwrap();
    assert_eq!(refined.upper(), 1.0, "no representable strict ceiling gap is proved");
    assert!(
        exact_in_interval(
            ExactExpectation { numerator: i128::try_from(denominator - 1).unwrap(), denominator },
            refined,
        )
        .unwrap()
    );
}

#[test]
fn terminal_cylinder_arithmetic_refuses_invalid_mass_support_and_checked_overflow() {
    for probability in [mass(0, 1), mass(1, 0), mass(2, 1), mass(u128::MAX, u128::MAX)] {
        assert!(cylinder_enclosure(probability, 0, 0, 1).is_none());
    }
    for (value, lower, upper) in [(2, 0, 1), (-1, 0, 1), (0, 1, -1)] {
        assert!(cylinder_enclosure(mass(1, 2), value, lower, upper).is_none());
    }
    assert!(cylinder_enclosure(mass(1, 2), i128::MAX, i128::MIN, i128::MAX).is_none());
    // The reduced rational remains outside the signed endpoint converter's capacity.
    assert!(cylinder_enclosure(mass(1, u128::MAX), 0, 0, 1).is_none());
    assert!(cylinder_enclosure(mass(i128::MAX as u128, i128::MAX as u128), 2, 0, 2).is_none());
}
