//! The joint expectation of note scores and lottery classes against the complete tree of nominal lottery outcomes.
use super::exact_tests::{Case, cases, fixture_with_probes, nominal};
use super::*;
use crate::replay::RankConfirmation;

fn width(bounds: RealBounds) -> f64 {
    bounds.upper - bounds.lower
}

/// In solo and network lives, the joint evaluation of every fixture holds the nominal expectation, inside the
/// enclosure of the exact evaluation, and its note mean is no wider.
#[test]
fn joint_expectations_contain_the_complete_nominal_law() {
    for (name, case) in cases() {
        let confirmations: Vec<_> =
            (0..case.4.fevers.len()).map(|range| RankConfirmation { frame: 0, range, rank: 1, percent: 250 }).collect();
        for ranking in [None, Some(confirmations.as_slice())] {
            let (master, deck, notes, params, setup, play, delta) = &case;
            let skills = luck_skills(master).unwrap();
            let before =
                luck_exact_score(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking, None, false)
                    .unwrap();
            let score =
                luck_joint_score(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking, &mut || {
                    false
                })
                .unwrap()
                .expect("an uncancelled joint evaluation");
            let (expected, _) = nominal(name, &case, ranking);
            let slack = 1e-9 * (1.0 + expected.abs());
            let label = format!("{name} network={}", ranking.is_some());
            assert!(
                score.final_mean.lower - slack <= expected && expected <= score.final_mean.upper + slack,
                "{label}: nominal {expected} outside {:?}",
                score.final_mean
            );
            assert!(
                before.final_mean.lower <= score.final_mean.lower && score.final_mean.upper <= before.final_mean.upper,
                "{label}: {:?} leaves {:?}",
                score.final_mean,
                before.final_mean
            );
            assert!(width(score.final_note_mean) <= width(before.final_note_mean), "{label}");
            assert_eq!(score.ranges.len(), before.ranges.len(), "{label}");
            for (range, old) in score.ranges.iter().zip(&before.ranges) {
                assert!(old.mean.lower <= range.mean.lower && range.mean.upper <= old.mean.upper, "{label}: {range:?}");
            }
        }
    }
}

/// Fixtures whose score queries execute frames again along different lottery paths: two or three probes with large
/// score-ups and a high power leave the binary32 factor states of one lottery class a few units in the last place
/// apart, so some notes of the exact evaluation take two integers in one class.
fn drifting_cases() -> Vec<(&'static str, Case)> {
    let drifting = |probes: &[i64], step, spread, times: &[i32]| {
        let mut case = fixture_with_probes(probes, step, spread, times, vec![(100, 400)]);
        case.3.total_power = 900_000;
        case
    };
    vec![
        ("two probes", drifting(&[8000, 12345], 10, 20, &[100, 140, 180, 220])),
        ("two probes, odd frames", drifting(&[12345, 8000], 7, 23, &[100, 150, 200, 250])),
        ("three probes, fine frames", drifting(&[8000, 12345, 4321], 5, 10, &[120, 160, 200, 240])),
    ]
}

/// The exact evaluation of every solo fixture leaves notes wide. In solo and network lives, the joint evaluation still
/// holds the nominal expectation, inside the enclosure of the exact evaluation, and narrows it where notes are wide.
#[test]
fn joint_expectations_narrow_notes_whose_factor_histories_drift() {
    for (name, case) in drifting_cases() {
        let confirmations: Vec<_> =
            (0..case.4.fevers.len()).map(|range| RankConfirmation { frame: 0, range, rank: 1, percent: 250 }).collect();
        for ranking in [None, Some(confirmations.as_slice())] {
            let (master, deck, notes, params, setup, play, delta) = &case;
            let skills = luck_skills(master).unwrap();
            let before =
                luck_exact_score(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking, None, false)
                    .unwrap();
            let score =
                luck_joint_score(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking, &mut || {
                    false
                })
                .unwrap()
                .expect("an uncancelled joint evaluation");
            let (expected, _) = nominal(name, &case, ranking);
            let slack = 1e-9 * (1.0 + expected.abs());
            let label = format!("{name} network={}", ranking.is_some());
            assert!(ranking.is_some() || before.wide_notes > 0, "{label}: {before:?}");
            assert!(
                score.final_mean.lower - slack <= expected && expected <= score.final_mean.upper + slack,
                "{label}: nominal {expected} outside {:?}",
                score.final_mean
            );
            assert!(
                before.final_mean.lower <= score.final_mean.lower && score.final_mean.upper <= before.final_mean.upper,
                "{label}: {:?} leaves {:?}",
                score.final_mean,
                before.final_mean
            );
            assert!(
                before.wide_notes == 0 || width(score.final_mean) < width(before.final_mean),
                "{label}: {:?} against {:?}",
                score.final_mean,
                before.final_mean
            );
        }
    }
}
