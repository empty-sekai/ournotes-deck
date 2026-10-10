use super::{effect, master, member};
use ournotes_sim::event::*;
use ournotes_sim::master::Master;
use serde_json::json;

fn ranked_master() -> Master {
    master(json!({
        "MasterEvent": [{"_id":9,"_liveEventRewardGroup":37,"_challengeLiveEventRewardGroup":41}],
        "MasterLiveEventReward": [
            {"_id":101,"_group":5,"_eventGroup":37,"_scoreRank":2,"_resourceType":4,"_resourceId":88,"_resourceCount":7,"_probability":10000},
            {"_id":102,"_group":5,"_eventGroup":37,"_scoreRank":3,"_resourceType":4,"_resourceId":88,"_resourceCount":11,"_probability":10000},
            {"_id":103,"_group":5,"_eventGroup":37,"_scoreRank":4,"_resourceType":4,"_resourceId":88,"_resourceCount":9,"_probability":10000},
            {"_id":199,"_group":37,"_eventGroup":99,"_scoreRank":2,"_resourceType":4,"_resourceId":88,"_resourceCount":1000,"_probability":10000}
        ],
        "MasterChallengeLiveEventReward": [
            {"_id":201,"_group":5,"_eventGroup":41,"_scoreRank":2,"_resourceType":4,"_resourceId":88,"_resourceCount":14,"_probability":10000},
            {"_id":202,"_group":5,"_eventGroup":41,"_scoreRank":3,"_resourceType":4,"_resourceId":88,"_resourceCount":18,"_probability":10000}
        ],
        "MasterEventEffect": [
            effect(1, 9, EVENT_ITEM, 2, json!({}), [1100; 5]),
            effect(2, 9, EVENT_ITEM, 3, json!({}), [1400; 5]),
            effect(3, 9, EVENT_POINT, 2, json!({}), [9000; 5]),
            effect(4, 9, PARAMETER_ALL, 3, json!({}), [19000; 5])
        ],
        "MasterLiveMusicBoostBonus": [{"_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":3,"_eventPointRate":19}],
        "MasterChallengeMusicBoostBonus": [{"_id":1,"_consumedChallengePointCount":400,"_liveMusicRewardRate":2,"_eventPointRate":11}],
        "MasterLiveScoreRank": [
            {"_id":1,"_group":7,"_liveScoreRank":2,"_requiredScore":0},
            {"_id":2,"_group":7,"_liveScoreRank":3,"_requiredScore":1000},
            {"_id":3,"_group":7,"_liveScoreRank":4,"_requiredScore":2000}
        ]
    }))
}

#[test]
fn exact_grade_uses_event_group_and_does_not_accumulate_lower_rewards() {
    let master = ranked_master();
    for (score, id, amount) in
        [(0, 101, 7), (999, 101, 7), (1000, 102, 11), (1001, 102, 11), (1999, 102, 11), (2000, 103, 9)]
    {
        let rank = score_rank(&master, 7, score).unwrap();
        let selected = ranked_event_reward(&master, 9, &EventResultRoute::NormalPlayed, rank).unwrap();
        assert_eq!(selected.id, id);
        let result = preview_ranked_event_items(
            &master,
            &RankedEventItemRequest {
                route: EventResultRoute::NormalPlayed,
                consumed_count: 0,
                event_id: 9,
                score_rank: rank,
            },
            &[],
            Some(&[]),
        )
        .unwrap();
        assert_eq!(result.rewards.len(), 1);
        assert_eq!(result.rewards[0].amount, amount);
        assert!(result.missing_reward_ids.is_empty());
    }
}

#[test]
fn played_and_skip_routes_use_the_declared_grade_and_item_multiplier() {
    let master = ranked_master();
    let member = member(1, 1, Some(1), 1, &[], 1);
    let snap = EventSnap { id: 5, character_ids: vec![1], band_ids: vec![1], card_type: 1, rank: 1 };
    for (route, consumed_count, reward_id, amount) in [
        (EventResultRoute::NormalPlayed, 1, 101, 26),
        (EventResultRoute::NormalSkip, 1, 101, 26),
        (EventResultRoute::ChallengePlayed { event_id: 9 }, 400, 201, 35),
        (EventResultRoute::ChallengeSkip { event_id: 9 }, 400, 201, 35),
    ] {
        let result = preview_ranked_event_items(
            &master,
            &RankedEventItemRequest { route, consumed_count, event_id: 9, score_rank: 2 },
            &[Some(&member)],
            Some(&[Some(&snap)]),
        )
        .unwrap();
        assert_eq!(result.rewards[0].reward_id, reward_id);
        assert_eq!(result.rewards[0].amount, amount);
    }
}

#[test]
fn unsupported_probability_and_ambiguous_grades_are_explicit() {
    for probability in [-1, 0, 1, 9999, 10001] {
        let mut master = ranked_master();
        master.live_event_rewards[0].probability = probability;
        assert!(matches!(
            ranked_event_reward(&master, 9, &EventResultRoute::NormalPlayed, 2),
            Err(ournotes_sim::Error::Unsupported(_))
        ));
    }
    for (resource_id, group) in [(88, 5), (99, 5), (88, 6)] {
        let mut master = ranked_master();
        let mut duplicate = master.live_event_rewards[0].clone();
        duplicate.id = 301;
        duplicate.resource_id = resource_id;
        duplicate.group = group;
        master.live_event_rewards.push(duplicate);
        assert!(ranked_event_reward(&master, 9, &EventResultRoute::NormalPlayed, 2).is_err());
    }
    let master = ranked_master();
    for rank in [0, 7] {
        assert!(ranked_event_reward(&master, 9, &EventResultRoute::NormalPlayed, rank).is_err());
    }
    assert!(ranked_event_reward(&master, 9, &EventResultRoute::ChallengePlayed { event_id: 10 }, 2).is_err());
    assert!(ranked_event_reward(&master, 10, &EventResultRoute::NormalPlayed, 2).is_err());
    let mut master = master;
    master.events[0].live_event_reward_group = 0;
    assert!(ranked_event_reward(&master, 9, &EventResultRoute::NormalPlayed, 2).is_err());
}

#[test]
fn declared_reward_projection_needs_no_selected_ids_or_counter_balances() {
    let request: RankedEventItemRequest = serde_json::from_value(json!({
        "route": {"kind":"normalPlayed"}, "consumedCount":0, "eventId":9, "scoreRank":2
    }))
    .unwrap();
    let result = preview_ranked_event_items(&ranked_master(), &request, &[], Some(&[])).unwrap();
    assert_eq!(result.rewards[0].amount, 7);
}
