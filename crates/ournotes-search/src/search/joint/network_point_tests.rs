use super::order_steps::pareto_pairs;
use super::point_bound::signed_network_reward_values;
use super::*;
use ournotes_sim::{event, master::Master};
use serde_json::json;

#[test]
fn score_target_caps_preserve_signed_means_and_per_order_cutoffs() {
    for threshold in [1, 10, 100] {
        let probability = PointBound::score_target(0, 0, ScoreTarget::AtLeast(threshold));
        let capped = PointBound::score_target(0, 0, ScoreTarget::Capped(threshold));
        for a in [-1000i128, -10, 0, 1, 10, 100, 1000] {
            assert_eq!(probability.order_payoff(0, a), i128::from(a >= threshold as i128));
            assert_eq!(capped.order_payoff(0, a), a.min(threshold as i128));
            for b in [-1000i128, -10, 0, 1, 10, 100, 1000] {
                let mean_ceiling = (a + b).div_euclid(2) + i128::from((a + b).rem_euclid(2) != 0);
                assert!(
                    2 * probability.mean_payoff(0, mean_ceiling, a.max(b))
                        >= i128::from(a >= threshold as i128) + i128::from(b >= threshold as i128)
                );
                assert!(
                    2 * capped.mean_payoff(0, mean_ceiling, a.max(b))
                        >= a.min(threshold as i128) + b.min(threshold as i128)
                );
            }
        }
    }
}

#[test]
fn truncated_majorants_bound_the_mean_step_payoff_below_the_order_cap() {
    let tiers = vec![(0, 15), (2_104_068, 25), (5_025_129, 35), (11_281_724, 50), (15_052_045, 75), (20_496_573, 100)];
    let hulls = (1..=tiers.len()).map(|k| super::super::uniform::concave_majorant(&tiers[..k])).collect();
    let bound = PointBound::plain(Vec::new(), Vec::new(), 100, Some(tiers), Some(hulls), None);
    // An xorshift sequence of order scores; each set's mean and maximum bound its mean step payoff.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..2000 {
        let centre = (next() % 22_000_000) as i128;
        let width = (next() % 3_000_000) as i128;
        let scores: Vec<i128> = (0..120).map(|_| (centre + (next() % (width as u64 + 1)) as i128).max(0)).collect();
        let bonus = (next() % 20_000) as i64;
        let max = *scores.iter().max().unwrap();
        let sum: i128 = scores.iter().sum();
        let mean_ceiling = (sum + 119) / 120;
        let actual: i128 = scores.iter().map(|&s| bound.order_payoff(bonus, s)).sum();
        assert!(actual <= 120 * bound.mean_payoff(bonus, mean_ceiling, max), "{centre} {width}");
        // the truncation is never looser than the full majorant
        assert!(bound.mean_payoff(bonus, mean_ceiling, max) <= bound.mean_payoff(bonus, mean_ceiling, i128::MAX));
    }
    // Every order below the 75 tier: the bound is the 50 step, not the chord to the 100 tier.
    assert_eq!(bound.mean_payoff(8500, 12_105_470, 12_818_652), 92);
    assert_eq!(bound.mean_payoff(8500, 12_105_470, i128::MAX), 122);
}

#[test]
fn payoff_steps_sum_the_best_order_payoffs_and_never_fall_below_at_tier_edges() {
    let tiers = vec![(0, 15), (935_655, 25), (2_076_590, 35), (4_741_282, 50), (6_360_627, 75), (8_551_127, 100)];
    let bound = PointBound::plain(Vec::new(), Vec::new(), 100, Some(tiers.clone()), None, None);
    let exact = |pairs: &[(i64, i64)], factors: &[f64]| -> i128 {
        factors
            .iter()
            .map(|&f| pairs.iter().map(|&(p, b)| bound.order_payoff(b, ((p as f64) * f).ceil() as i128)).max().unwrap())
            .sum()
    };
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut steps = Vec::new();
    for _ in 0..2000 {
        let pairs: Vec<(i64, i64)> =
            (0..1 + next() % 6).map(|_| ((next() % 400_000) as i64, (next() % 30_000) as i64)).collect();
        let factors: Vec<f64> = (0..120).map(|_| 5.0 + (next() % 1_000_000) as f64 / 25_000.0).collect();
        // Random factors never land within a few ulps of a tier edge.
        assert_eq!(bound.order_payoff_sum(&pairs, &factors, &mut steps), Some(exact(&pairs, &factors)));
    }
    // Factors around each edge: the steps start no later than the caps reach the tier.
    for &(score, _) in &tiers[1..] {
        for power in [1, 3, 997, 123_457, 399_989] {
            let edge = (score - 1) as f64 / power as f64;
            let mut factors = vec![edge];
            for _ in 0..4 {
                factors.push(factors.last().unwrap().next_up());
                factors.insert(0, factors[0].next_down());
            }
            let pairs = [(power, 8500)];
            for f in &factors {
                let one = std::slice::from_ref(f);
                assert!(bound.order_payoff_sum(&pairs, one, &mut steps).unwrap() >= exact(&pairs, one));
            }
        }
    }
}

#[test]
fn ranked_bounds_cover_the_ranking_value_of_every_expected_score() {
    use crate::search::certified_search::REWARD_WEIGHT;
    let tiers = vec![(0, 15), (2_104_068, 25), (5_025_129, 35), (11_281_724, 50), (15_052_045, 75), (20_496_573, 100)];
    let hulls = (1..=tiers.len()).map(|k| super::super::uniform::concave_majorant(&tiers[..k])).collect();
    let points = PointBound::plain(Vec::new(), Vec::new(), 100, Some(tiers), Some(hulls), None);
    let plain = [
        points,
        PointBound::score_target(0, 0, ScoreTarget::AtLeast(12_000_000)),
        PointBound::score_target(0, 0, ScoreTarget::Capped(12_000_000)),
    ];
    let ranked: Vec<_> = plain.iter().map(PointBound::ranked).collect();
    // An order earns its reward at its expected score and ranks by it, then by that score.
    let rank = |plain: &PointBound, bonus: i64, mean: i128| plain.order_payoff(bonus, mean) * REWARD_WEIGHT + mean;
    let mut x = 0x5851_f42d_4c95_7f2du64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..2000 {
        // The expected scores of a team's orders and caps at least their mean and their maximum.
        let means: Vec<i128> = (0..1 + next() % 8).map(|_| (next() % 22_000_000) as i128).collect();
        let n = means.len() as i128;
        let mean_cap = (means.iter().sum::<i128>() + n - 1) / n + (next() % 1000) as i128;
        let max_cap = *means.iter().max().unwrap() + (next() % 1000) as i128;
        let bonus = (next() % 20_000) as i64;
        for (plain, ranked) in plain.iter().zip(&ranked) {
            let values: Vec<i128> = means.iter().map(|&m| rank(plain, bonus, m)).collect();
            assert!(values.iter().all(|&v| v <= ranked.order_payoff(bonus, max_cap)));
            assert!(values.iter().all(|&v| v <= ranked.maximum(bonus)));
            assert!(values.iter().sum::<i128>() <= n * ranked.mean_payoff(bonus, mean_cap, max_cap));
            if plain.target.is_none() {
                assert!(values.iter().all(|&v| v <= ranked.cutoff_payoff(bonus, max_cap)));
            }
        }
    }
    // Equal rewards rank by the expected score, and a larger reward ranks above any score.
    assert!(ranked[0].order_payoff(0, 11_300_000) > ranked[0].order_payoff(0, 11_290_000));
    assert!(ranked[0].order_payoff(0, 11_281_724) > ranked[0].order_payoff(0, 11_281_723) + i128::from(i32::MAX));
    assert_eq!(ranked[2].order_payoff(0, 13_000_000), 12_000_000 * REWARD_WEIGHT + 13_000_000);
}

#[test]
fn pareto_pairs_keep_exactly_the_undominated_pairs() {
    let pairs = vec![(5, 1), (3, 4), (5, 2), (1, 9), (3, 3), (2, 4), (1, 9), (0, 10)];
    assert_eq!(pareto_pairs(pairs), vec![(5, 2), (3, 4), (1, 9), (0, 10)]);
}

#[test]
fn negative_network_score_none_reward_is_in_ep_and_cp_caps() {
    let tables = json!({
        "MasterEvent":[{"_id":7,"_liveEventPointGroup":1}],
        "MasterLiveScoreRank":[{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0,"_battleLiveRequiredScore":0}],
        "MasterLiveEventPoint":[{"_id":1,"_group":1,"_scoreRank":0,"_value":100},{"_id":2,"_group":1,"_scoreRank":2,"_value":1}],
        "MasterLiveChallengePoint":[{"_id":1,"_scoreRank":0,"_value":100},{"_id":2,"_scoreRank":2,"_value":1}],
        "MasterLiveMusicBoostBonus":[{"_id":1,"_consumedLiveBoostCount":0,"_eventPointRate":1}]
    });
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
            .unwrap();
    let request = event::EventPointRequest {
        route: event::EventResultRoute::NormalPlayed,
        holding_event_ids: vec![7],
        consumed_count: 0,
        local_events: vec![event::LocalEvent { event_id: 7, points: 0, challenge_points: 0, added: Vec::new() }],
    };
    assert_eq!(event::battle_score_rank(&master, 1, -1, 1), event::RANK_NONE);
    assert_eq!(event::battle_score_rank(&master, 1, 0, 1), event::RANK_D);
    for cp in [false, true] {
        let route = point_route::PointRoute::compile(&master, &request, 7, cp).unwrap();
        let multiplier = signed_network_reward_values(&master, &route, 1).into_iter().max().unwrap() * route.rate();
        let bound = PointBound::plain(Vec::new(), Vec::new(), multiplier, None, None, None);
        assert_eq!(route.value_at_rank(&master, event::RANK_NONE).unwrap(), 100);
        assert_eq!(bound.order_payoff(0, 10), 100);
        assert_eq!(bound.mean_payoff(0, 10, 10), 100);
        assert_eq!(bound.mean_payoff(0, -1, -1), 100);
    }
}
