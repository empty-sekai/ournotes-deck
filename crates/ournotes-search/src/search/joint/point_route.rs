//! Result-route-specific reward tables for the joint event-point bound.
//!
//! This only selects the native rate and reward value at an already resolved rank. The caller still proves
//! nonnegative, nonwrapping bonus/products and builds prefix maxima and a concave majorant; a reward table need
//! not be monotone. Actual score outcomes are settled by the ordinary terminal-payoff path before averaging.

use super::unavailable;
use ournotes_sim::Error;
use ournotes_sim::event::{self, EventPointRequest, EventResultRoute};
use ournotes_sim::master::Master;

#[derive(Clone, Copy, Debug)]
enum Reward {
    Normal { group: i64, challenge_points: bool },
    ChallengeEvent { group: i64 },
}

/// Native table selection for one event. This is not the acquired Challenge-points currency itself.
#[derive(Clone, Copy, Debug)]
pub(super) struct PointRoute {
    reward: Reward,
    rate: i64,
}

impl PointRoute {
    /// `request` comes from `ResolvedContext::event_request`, which verifies the selected Challenge song's event
    /// and result clock. Normal routes retain the existing one-held-event bound domain. ChallengePlayed settles
    /// its selected event independently of the holding-event list, but still needs that event's local counter.
    pub(super) fn compile(
        master: &Master,
        request: &EventPointRequest,
        event_id: i64,
        challenge_points: bool,
    ) -> Result<Self, Error> {
        let ev = master.event(event_id).ok_or_else(|| unavailable("missing event"))?;
        if !request.local_events.iter().any(|local| local.event_id == event_id) {
            return Err(unavailable("PT bound requires the target local event"));
        }
        let (reward, rate) = match request.route {
            EventResultRoute::NormalPlayed => {
                if request.holding_event_ids != [event_id] {
                    return Err(unavailable("PT bound requires one held normal-played event"));
                }
                (
                    Reward::Normal { group: ev.live_event_point_group, challenge_points },
                    event::boost_bonus(master, i64::from(request.consumed_count))?[4],
                )
            }
            EventResultRoute::ChallengePlayed { event_id: selected } => {
                if selected != event_id {
                    return Err(unavailable("Challenge PT route differs from the target event"));
                }
                if challenge_points {
                    return Err(unavailable("Challenge Live spends Challenge points and earns none"));
                }
                // Native preview debits the local Challenge balance before this lookup, even if a later reward
                // row is missing. Balance never gates PT eligibility: leave the wrapping/clamped debit to preview.
                (
                    Reward::ChallengeEvent { group: ev.challenge_live_event_point_group },
                    event::challenge_point_bonus(master, i64::from(request.consumed_count))?[4],
                )
            }
            EventResultRoute::NormalSkip | EventResultRoute::ChallengeSkip { .. } => {
                return Err(unavailable("joint played PT bound does not cover a Skip result route"));
            }
        };
        Ok(Self { reward, rate })
    }

    /// Preserve the native i64 master value. The parent bound checks casts and every product before using it.
    pub(super) fn rate(&self) -> i64 {
        self.rate
    }

    pub(super) fn value_at_rank(&self, master: &Master, rank: i64) -> Result<i64, Error> {
        match self.reward {
            Reward::Normal { group, challenge_points } => {
                // A normal native result reads both reward tables, regardless of which counter is the objective.
                // Keep either missing-row error; dropping the unused lookup would alter the accepted domain.
                let event_value = event::music_score_event_point(master, group, rank)
                    .ok_or_else(|| unavailable("missing reachable PT rank"))?;
                let challenge_value = event::music_score_challenge_point(master, rank)
                    .ok_or_else(|| unavailable("missing reachable challenge rank"))?;
                Ok(if challenge_points { challenge_value } else { event_value })
            }
            Reward::ChallengeEvent { group } => {
                // ChallengePlayed reports the absent row and awards no EP; it does not throw or read normal CP.
                Ok(event::challenge_live_event_point(master, group, rank).unwrap_or(0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event::LocalEvent;
    use serde_json::{Value, json};

    fn tables() -> Value {
        json!({
            "MasterEvent": [{"_id":7,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}],
            "MasterLiveScoreRank": [
                {"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0},
                {"_id":2,"_group":1,"_liveScoreRank":3,"_requiredScore":100},
                {"_id":3,"_group":1,"_liveScoreRank":4,"_requiredScore":200}
            ],
            "MasterLiveEventPoint": [
                {"_id":1,"_group":1,"_scoreRank":2,"_value":7},
                {"_id":2,"_group":1,"_scoreRank":3,"_value":33},
                {"_id":3,"_group":1,"_scoreRank":4,"_value":2}
            ],
            "MasterChallengeLiveEventPoint": [
                {"_id":1,"_group":2,"_scoreRank":2,"_value":20},
                {"_id":2,"_group":2,"_scoreRank":3,"_value":80},
                {"_id":3,"_group":2,"_scoreRank":4,"_value":5}
            ],
            "MasterLiveChallengePoint": [
                {"_id":1,"_scoreRank":2,"_value":3},
                {"_id":2,"_scoreRank":3,"_value":9},
                {"_id":3,"_scoreRank":4,"_value":1}
            ],
            "MasterLiveMusicBoostBonus": [{
                "_id":1,"_consumedLiveBoostCount":400,"_liveMusicRewardRate":6,"_playerExpRate":6,
                "_memberCardExpRate":6,"_friendshipExpRate":6,"_eventPointRate":6
            }],
            "MasterChallengeMusicBoostBonus": [
                {"_id":1,"_consumedChallengePointCount":400,"_eventPointRate":2},
                {"_id":2,"_consumedChallengePointCount":800,"_eventPointRate":4},
                {"_id":3,"_consumedChallengePointCount":1600,"_eventPointRate":8}
            ]
        })
    }

    fn build_master(tables: Value) -> Master {
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
            .unwrap()
    }

    fn request(route: EventResultRoute) -> EventPointRequest {
        EventPointRequest {
            route,
            holding_event_ids: vec![7],
            consumed_count: 400,
            local_events: vec![LocalEvent { event_id: 7, points: 100, challenge_points: 250, added: Vec::new() }],
        }
    }

    fn preview(master: &Master, request: &EventPointRequest, rank: i64) -> event::EventPointPreview {
        event::preview_client_event_points(master, request, &[None; 5], Some(&[None; 5]), rank).unwrap()
    }

    #[test]
    fn normal_route_keeps_its_rates_and_both_counter_tables() {
        let master = build_master(tables());
        let request = request(EventResultRoute::NormalPlayed);
        for challenge_points in [false, true] {
            let route = PointRoute::compile(&master, &request, 7, challenge_points).unwrap();
            assert_eq!(route.rate(), 6);
            for rank in [2, 3, 4] {
                let actual = preview(&master, &request, rank);
                let value = route.value_at_rank(&master, rank).unwrap() * route.rate();
                assert_eq!(
                    value,
                    i64::from(if challenge_points { actual.challenge_points_for(7) } else { actual.points_for(7) })
                );
            }
        }
        for missing in ["MasterLiveEventPoint", "MasterLiveChallengePoint"] {
            let mut data = tables();
            data.as_object_mut().unwrap().remove(missing);
            let master = build_master(data);
            for challenge_points in [false, true] {
                let route = PointRoute::compile(&master, &request, 7, challenge_points).unwrap();
                assert!(route.value_at_rank(&master, 2).is_err());
            }
        }
    }

    #[test]
    fn challenge_route_uses_only_its_selected_event_tables_and_consumption() {
        let mut data = tables();
        for unused in ["MasterLiveEventPoint", "MasterLiveChallengePoint", "MasterLiveMusicBoostBonus"] {
            data.as_object_mut().unwrap().remove(unused);
        }
        let master = build_master(data);
        let mut request = request(EventResultRoute::ChallengePlayed { event_id: 7 });
        request.holding_event_ids.clear();
        for (consumed, rate) in [(0, 1), (200, 1), (400, 2), (800, 4), (1600, 8)] {
            request.consumed_count = consumed;
            let route = PointRoute::compile(&master, &request, 7, false).unwrap();
            assert_eq!(route.rate(), rate);
            for rank in [2, 3, 4] {
                let actual = preview(&master, &request, rank);
                assert_eq!(route.value_at_rank(&master, rank).unwrap() * rate, i64::from(actual.points_for(7)));
                assert_eq!(actual.challenge_points_for(7), 0);
                assert_eq!(actual.local_events[0].challenge_points, event::debit_challenge_points(250, consumed));
            }
        }
        assert_eq!(request.local_events[0].challenge_points, 250, "bound compilation must not mutate counters");
    }

    #[test]
    fn challenge_missing_rank_awards_zero_but_native_preview_keeps_the_debit() {
        let master = build_master(tables());
        let request = request(EventResultRoute::ChallengePlayed { event_id: 7 });
        let route = PointRoute::compile(&master, &request, 7, false).unwrap();
        assert_eq!(route.value_at_rank(&master, 5).unwrap(), 0);
        let actual = preview(&master, &request, 5);
        assert_eq!(actual.points_for(7), 0);
        assert_eq!(actual.local_events[0].challenge_points, 0);
        assert_eq!(actual.missing_point_rows.len(), 1);
        assert!(actual.local_events[0].added.is_empty());
    }

    #[test]
    fn route_target_and_counter_prerequisites_are_not_silently_widened() {
        let master = build_master(tables());
        let mut challenge = request(EventResultRoute::ChallengePlayed { event_id: 7 });
        assert!(PointRoute::compile(&master, &challenge, 7, true).is_err());
        challenge.route = EventResultRoute::ChallengePlayed { event_id: 8 };
        assert!(PointRoute::compile(&master, &challenge, 7, false).is_err());
        challenge.route = EventResultRoute::ChallengePlayed { event_id: 7 };
        challenge.local_events.clear();
        assert!(PointRoute::compile(&master, &challenge, 7, false).is_err());
        for route in [EventResultRoute::NormalSkip, EventResultRoute::ChallengeSkip { event_id: 7 }] {
            assert!(PointRoute::compile(&master, &request(route), 7, false).is_err());
        }
        let mut normal = request(EventResultRoute::NormalPlayed);
        for held in [vec![], vec![8], vec![7, 8]] {
            normal.holding_event_ids = held;
            assert!(PointRoute::compile(&master, &normal, 7, false).is_err());
        }
    }

    #[test]
    fn route_selection_preserves_missing_boost_errors_and_raw_values_for_parent_wrap_checks() {
        let mut data = tables();
        data["MasterChallengeLiveEventPoint"][0]["_value"] = json!(-1);
        data["MasterChallengeLiveEventPoint"][1]["_value"] = json!(i64::from(i32::MAX) + 1);
        let master = build_master(data);
        let mut request = request(EventResultRoute::ChallengePlayed { event_id: 7 });
        let route = PointRoute::compile(&master, &request, 7, false).unwrap();
        assert_eq!(route.value_at_rank(&master, 2).unwrap(), -1);
        assert_eq!(route.value_at_rank(&master, 3).unwrap(), i64::from(i32::MAX) + 1);
        request.consumed_count = 201;
        assert!(PointRoute::compile(&master, &request, 7, false).is_err());
    }

    #[test]
    fn nonmonotone_challenge_rewards_use_prefix_max_and_hull_after_native_settlement() {
        use super::super::super::uniform::{concave_majorant, concave_value_ceil};
        let master = build_master(tables());
        let request = request(EventResultRoute::ChallengePlayed { event_id: 7 });
        let route = PointRoute::compile(&master, &request, 7, false).unwrap();
        let tiers: Vec<_> = [0, 100, 200]
            .into_iter()
            .map(|score| {
                let rank = event::score_rank(&master, 1, score).unwrap();
                (score, route.value_at_rank(&master, rank).unwrap() * route.rate())
            })
            .collect();
        assert_eq!(tiers, vec![(0, 40), (100, 160), (200, 10)]);
        let hull = concave_majorant(&tiers);
        let actual: Vec<_> = (0..=300)
            .map(|score| preview(&master, &request, event::score_rank(&master, 1, score).unwrap()).points_for(7))
            .collect();
        for cap in 0..=300 {
            let per_order =
                tiers.iter().filter(|&&(score, _)| score <= cap as i64).map(|&(_, value)| value).max().unwrap();
            assert!(actual[..=cap].iter().all(|&value| i64::from(value) <= per_order));
            assert!(i128::from(per_order) <= concave_value_ceil(&hull, cap as i128, 1, 1).unwrap());
        }
        // Mean of settled rewards differs from reward at mean score. Jensen applies only to the majorant.
        assert_eq!((actual[99] + actual[101]) / 2, 100);
        assert_eq!(actual[100], 160);
        for (a, b) in [(0usize, 200usize), (99, 101), (100, 300), (199, 201)] {
            let mean_score_cap = (a + b).div_ceil(2) as i128;
            let mean_payoff_cap = concave_value_ceil(&hull, mean_score_cap, 1, 1).unwrap();
            assert!(i128::from(actual[a]) + i128::from(actual[b]) <= 2 * mean_payoff_cap);
        }
    }
}
