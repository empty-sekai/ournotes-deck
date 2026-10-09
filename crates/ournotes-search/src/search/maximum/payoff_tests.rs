use super::*;
use crate::search::certified_search::PayoffStep;

fn primitive_payoff(metric: &Metric, score: i32, life: i32) -> i128 {
    match *metric {
        Metric::Score => score.into(),
        Metric::ScoreAtLeast { threshold } => i128::from(score >= threshold),
        Metric::CappedScore { threshold } => score.min(threshold).into(),
        Metric::ScoreAndLifeAtLeast { threshold, min_final_life } => {
            i128::from(score >= threshold && life >= min_final_life)
        }
        _ => unreachable!(),
    }
}

#[test]
fn primitive_certificates_match_every_nonempty_finite_score_life_support() {
    let universe: Vec<_> = [-1, 0, 1, 2].into_iter().flat_map(|score| [0, 5].map(|life| (score, life))).collect();
    let mut accepted = 0;
    let mut refused = 0;
    for mask in 1..1usize << universe.len() {
        let support: Vec<_> = universe
            .iter()
            .enumerate()
            .filter_map(|(index, &outcome)| ((mask >> index) & 1 != 0).then_some(outcome))
            .collect();
        let maximum = support.iter().map(|&(score, _)| score).max().unwrap();
        let constant_life = support.iter().all(|&(_, life)| life == support[0].1).then_some(support[0].1);
        let mut metrics = vec![Metric::Score];
        for threshold in [-2, 0, 1, 2, 3] {
            metrics.push(Metric::ScoreAtLeast { threshold });
            metrics.push(Metric::CappedScore { threshold });
            metrics.push(Metric::ScoreAndLifeAtLeast { threshold, min_final_life: 3 });
        }
        for metric in metrics {
            let expected =
                support.iter().map(|&(score, life)| (primitive_payoff(&metric, score, life), score)).max().unwrap();
            for &(score, life) in support.iter().filter(|&&(score, _)| score == maximum) {
                for certificate in [None, constant_life] {
                    if primitive_witness_suffices(&metric, maximum, &[life], certificate).unwrap() {
                        assert_eq!((primitive_payoff(&metric, score, life), score), expected);
                        accepted += 1;
                    } else {
                        refused += 1;
                    }
                }
            }
        }
    }
    assert!(accepted > 1000 && refused > 0);
}

#[test]
fn native_certificates_match_exhaustive_signed_step_tables_and_score_supports() {
    let mut nonmonotone_accepted = 0;
    let mut nonmonotone_refused = 0;
    for encoded in 0..81 {
        let values: [i128; 4] = std::array::from_fn(|index| [-3, 0, 7][(encoded / 3usize.pow(index as u32)) % 3]);
        let map = PayoffMap::NativeSteps(
            values
                .iter()
                .enumerate()
                .map(|(score, &value)| PayoffStep { lower: score as i32, upper: score as i32, value })
                .collect(),
        );
        for mask in 1..16 {
            let support: Vec<_> = (0..4).filter(|score| (mask >> score) & 1 != 0).collect();
            let lower = *support.first().unwrap();
            let maximum = *support.last().unwrap();
            let expected = support.iter().map(|&score| (values[score as usize], score)).max().unwrap();
            let accepted = native_witness_suffices(&map, lower, maximum);
            if accepted {
                assert_eq!((values[maximum as usize], maximum), expected);
            }
            if values.windows(2).any(|pair| pair[0] > pair[1]) {
                nonmonotone_accepted += usize::from(accepted);
                nonmonotone_refused += usize::from(!accepted);
            }
        }
    }
    assert!(nonmonotone_accepted > 0 && nonmonotone_refused > 0);
}

#[test]
fn life_witnesses_preserve_joint_success_and_highest_score_ties() {
    let metric = Metric::ScoreAndLifeAtLeast { threshold: 5, min_final_life: 100 };
    assert_eq!(primitive_witness_suffices(&metric, 10, &[50], None), Some(false));
    // A second path at the same maximum score can succeed, even when the first path fails.
    assert_eq!(primitive_witness_suffices(&metric, 10, &[50, 100], None), Some(true));
    assert_eq!(primitive_witness_suffices(&metric, 4, &[50], None), Some(true));
    assert_eq!(primitive_witness_suffices(&metric, 10, &[50], Some(50)), Some(true));
    let support = [(10, 50), (9, 100)];
    assert_eq!(
        support.into_iter().map(|(score, life)| (primitive_payoff(&metric, score, life), score)).max(),
        Some((1, 9))
    );
}

#[test]
fn step_enclosures_do_not_fill_missing_scores_or_use_only_matching_endpoints() {
    let steps = |entries: &[(i32, i32, i128)]| {
        PayoffMap::NativeSteps(
            entries.iter().map(|&(lower, upper, value)| PayoffStep { lower, upper, value }).collect(),
        )
    };
    assert!(!native_witness_suffices(&steps(&[(0, 0, 5), (2, 2, 5)]), 0, 2));
    assert!(!native_witness_suffices(&steps(&[(0, 0, 5), (1, 1, 9), (2, 2, 5)]), 0, 2));
    assert!(native_witness_suffices(&steps(&[(0, 0, 5), (1, 1, -9), (2, 2, 5)]), 0, 2));
    assert!(!native_witness_suffices(&steps(&[(0, 1, 5)]), 0, 2));
    assert!(native_witness_suffices(&steps(&[(i32::MIN, i32::MAX, -1)]), i32::MIN, i32::MAX));
}
