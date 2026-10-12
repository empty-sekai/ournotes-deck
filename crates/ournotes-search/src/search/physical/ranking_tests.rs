use super::*;

fn entry(payoff: i128, power: i32, id: i64) -> Entry {
    let expectation = expectation::ExactExpectation { numerator: payoff, denominator: 120 };
    Entry {
        physical: PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] },
        members: [id, 20, 30, 40, 50],
        snaps: [None; 5],
        power,
        evaluation: FiniteEvaluation {
            expected_score: expectation,
            expected_payoff: expectation,
            score_mass: BTreeMap::new(),
            payoff_mass: BTreeMap::new(),
            outcomes: Vec::new(),
        },
        terms: None,
    }
}

#[test]
fn a_higher_payoff_precedes_power_and_ids() {
    assert_eq!(compare(&entry(1201, 1, 2), &entry(1200, 1_000_000, 1)), Ordering::Less);
}

#[test]
fn equal_payoffs_use_power_then_canonical_ids() {
    let mut rows = [entry(1200, 100, 2), entry(1200, 101, 3), entry(1200, 100, 1)];
    rows.sort_by(compare);
    assert_eq!(rows.map(|row| row.members[0]), [3, 1, 2]);
}
