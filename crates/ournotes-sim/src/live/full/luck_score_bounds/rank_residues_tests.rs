//! Rank bonus expectations from residue laws against the complete tree of nominal lottery outcomes, and the power
//! comparison of note scores against the nominal laws at both powers.
use super::exact::{Observers, PowerTwin, exact_score};
use super::exact_tests::{Case, cases, nominal};
use super::*;
use crate::replay::RankConfirmation;

fn plain(case: &Case, ranking: Option<&[RankConfirmation]>) -> LuckExactScore {
    let (master, deck, notes, params, setup, play, delta) = case;
    let skills = luck_skills(master).unwrap();
    luck_exact_score(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking, None, false).unwrap()
}

fn refined(case: &Case, ranking: Option<&[RankConfirmation]>) -> LuckExactScore {
    let (master, deck, notes, params, setup, play, delta) = case;
    let skills = luck_skills(master).unwrap();
    luck_exact_score_refined(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking).unwrap()
}

fn width(bounds: RealBounds) -> f64 {
    bounds.upper - bounds.lower
}

/// Percentages 250, 370 and 333 truncate modulo 2, 10 and 100. With every note scoring one integer per lottery class,
/// the residue law of every range score gives the bonus expectation, so the final mean holds the nominal expectation
/// within outward rounding, inside the enclosure without residue laws.
#[test]
fn residue_laws_give_the_rank_bonus_expectations_of_the_complete_nominal_law() {
    let mut narrowed = 0;
    for percent in [250, 370, 333] {
        for (name, mut case) in cases() {
            for row in &mut case.0.gekisou_ranking_score_bonuses {
                row.score_bonus_percent = percent;
            }
            let confirmations: Vec<_> =
                (0..case.4.fevers.len()).map(|range| RankConfirmation { frame: 0, range, rank: 1, percent }).collect();
            for ranking in [None, Some(confirmations.as_slice())] {
                let (expected, _) = nominal(name, &case, ranking);
                let (before, score) = (plain(&case, ranking), refined(&case, ranking));
                let slack = 1e-9 * (1.0 + expected.abs());
                let label = format!("{name} {percent}% network={}", ranking.is_some());
                assert!(
                    score.final_mean.lower - slack <= expected && expected <= score.final_mean.upper + slack,
                    "{label}: nominal {expected} outside {:?}",
                    score.final_mean
                );
                assert!(
                    before.final_mean.lower <= score.final_mean.lower
                        && score.final_mean.upper <= before.final_mean.upper,
                    "{label}: {:?} leaves {:?}",
                    score.final_mean,
                    before.final_mean
                );
                assert!(width(score.final_mean) <= slack, "{label}: width {}", width(score.final_mean));
                for (range, old) in score.ranges.iter().zip(&before.ranges) {
                    assert!(width(range.bonus_mean) <= width(old.bonus_mean), "{label}: {range:?} {old:?}");
                    narrowed += usize::from(width(range.bonus_mean) < width(old.bonus_mean));
                }
            }
        }
    }
    assert!(narrowed > 0);
}

/// A formation whose note values each score one integer per lottery class, the same at another power, has the same
/// complete nominal law at that power. Small power steps leave the scores of some cases and large ones move them.
#[test]
fn equal_note_scores_at_two_powers_give_the_same_nominal_law() {
    let (mut equal, mut differ) = (0, 0);
    for (name, mut case) in cases() {
        // Scores of a few dozen points, so a small power step leaves most of their floors.
        case.3.converted_note_count *= 30;
        let (master, deck, notes, params, setup, play, delta) = &case;
        let skills = luck_skills(master).unwrap();
        let live = LotteryLive {
            master,
            skills: &skills,
            notes,
            events: &[],
            params: *params,
            setup,
            play,
            delta_times: delta,
            ranking: None,
        };
        let (_, scores) = nominal(name, &case, None);
        for step in [1, 2, 3, 7, 500, 1000] {
            let power = params.total_power + step;
            let mut twin = PowerTwin::new(power);
            let evaluation =
                exact_score(&live, deck, None, None, false, Observers { reads: None, twin: Some(&mut twin) });
            // The comparison ends the evaluation at the first note value that may score differently.
            assert_eq!(evaluation.is_ok(), twin.equal(), "{name} power {power}");
            if twin.equal() {
                let mut other = case.clone();
                other.3.total_power = power;
                assert_eq!(scores, nominal(name, &other, None).1, "{name} power {power}");
                equal += 1;
            } else {
                differ += 1;
            }
        }
    }
    assert!(equal > 0 && differ > 0, "{equal} {differ}");
}
