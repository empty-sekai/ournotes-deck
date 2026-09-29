//! Event bonus and event-point functions on synthetic tables.

use ournotes_deck::event::*;
use ournotes_deck::master::Master;
use serde_json::json;

fn master(tables: serde_json::Value) -> Master {
    let texts: Vec<(String, String)> =
        tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({ "_allData": v }).to_string())).collect();
    Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, t)| t.as_str())).unwrap()
}

fn effect(
    id: i64,
    event: i64,
    kind: i64,
    resource: i64,
    keys: serde_json::Value,
    values: [i64; 5],
) -> serde_json::Value {
    let mut e = json!({"_id": id, "_eventId": event, "_eventBonusType": kind, "_resourceTypeConstraint": resource,
        "_characterId": 0, "_bandId": 0, "_cardType": 0, "_tagId": 0, "_memberCardId": 0, "_supportCardId": 0,
        "_rank1EffectValue": values[0], "_rank2EffectValue": values[1], "_rank3EffectValue": values[2],
        "_rank4EffectValue": values[3], "_rank5EffectValue": values[4]});
    for (k, v) in keys.as_object().unwrap() {
        e[k] = v.clone();
    }
    e
}

fn member(id: i64, character: i64, band: Option<i64>, ty: i64, tags: &[i64], rank: i64) -> EventMember {
    EventMember { id, character_id: character, band_id: band, card_type: ty, tags: tags.to_vec(), rank }
}

#[test]
fn member_and_snap_keys_are_asymmetric() {
    let m = master(json!({"MasterEventEffect": [
        effect(1, 1, EVENT_POINT, 2, json!({"_tagId": 3}), [100, 200, 300, 400, 500]),
        effect(2, 1, EVENT_POINT, 3, json!({"_tagId": 3}), [1000, 1000, 1000, 1000, 1000]),
        effect(3, 1, EVENT_POINT, 3, json!({"_bandId": 2}), [10, 20, 30, 40, 50]),
        effect(4, 1, EVENT_POINT, 2, json!({"_supportCardId": 9}), [7, 7, 7, 7, 7]),
    ]}));
    let ev = vec![event_effects(&m, 1)];
    let tagged = member(1, 1, Some(1), 1, &[3], 2);
    let snap = EventSnap { id: 9, character_ids: vec![4], band_ids: vec![2], card_type: 1, rank: 3 };
    // a member effect ignores the snap id key; a tagged effect never reaches a snap
    assert_eq!(total_effect_10000(&ev, Some(EventCard::Member(&tagged)), EVENT_POINT).unwrap(), 200 + 7);
    assert_eq!(total_effect_10000(&ev, Some(EventCard::Snap(&snap)), EVENT_POINT).unwrap(), 30);
    let members = [Some(&tagged), None, None, None, None];
    let snaps = [Some(&snap)];
    assert_eq!(event_point_bonus_10000(&ev, &members, Some(&snaps)).unwrap(), 237);
    assert!(matches!(event_point_bonus_10000(&ev, &members, None), Err(ournotes_deck::Error::Game(_))));
}

#[test]
fn band_key_needs_a_band_and_rank_must_exist() {
    let m =
        master(json!({"MasterEventEffect": [effect(1, 1, EVENT_POINT, 2, json!({"_bandId": 1}), [1, 2, 3, 4, 5])]}));
    let ev = vec![event_effects(&m, 1)];
    let no_band = member(1, 1, None, 1, &[], 1);
    assert!(total_effect_10000(&ev, Some(EventCard::Member(&no_band)), EVENT_POINT).is_err());
    let bad_rank = member(1, 1, Some(1), 1, &[], 6);
    assert!(total_effect_10000(&ev, Some(EventCard::Member(&bad_rank)), EVENT_POINT).is_err());
    let other_band = member(1, 1, Some(2), 1, &[], 6);
    assert_eq!(total_effect_10000(&ev, Some(EventCard::Member(&other_band)), EVENT_POINT).unwrap(), 0);
}

#[test]
fn points_wrap_like_the_client() {
    // 100 % bonus, rate 50 (ten boosts at 5 each), 2148 points: (20000 * 50 * 2148) exceeds 2^31 and wraps
    let wrapped = live_event_point(10000, 50, 2148);
    assert_eq!(wrapped, (20000i32.wrapping_mul(50).wrapping_mul(2148)) / 10000);
    assert!(wrapped < 0);
    assert_eq!(live_event_point(10000, 50, 2147), 214_700);
    assert_eq!(live_event_point(2550, 5, 1200), 7530);
    assert_eq!(challenge_live_event_point_count(2550, 5, 1200), 7530);
    assert_eq!(add_event_point_count(5, 0), 5);
    assert_eq!(as_percentage(2599), 25);
    assert_eq!(as_percentage(-1), -1);
}

#[test]
fn ranks_and_boosts() {
    let m = master(json!({
        "MasterLiveScoreRank": [
            {"_id": 1, "_group": 1, "_liveScoreRank": 1, "_requiredScore": 0, "_battleLiveRequiredScore": 0},
            {"_id": 2, "_group": 1, "_liveScoreRank": 3, "_requiredScore": 5000, "_battleLiveRequiredScore": 9000},
            {"_id": 3, "_group": 1, "_liveScoreRank": 2, "_requiredScore": 1000, "_battleLiveRequiredScore": 2000},
        ],
        "MasterLiveMusicBoostBonus": [
            {"_id": 1, "_consumedLiveBoostCount": 1, "_liveMusicRewardRate": 2, "_playerExpRate": 3, "_memberCardExpRate": 4,
             "_friendshipExpRate": 5, "_eventPointRate": 6},
        ],
    }));
    assert_eq!(score_rank(&m, 1, 4999).unwrap(), RANK_D);
    assert_eq!(score_rank(&m, 1, 5000).unwrap(), RANK_C);
    assert!(score_rank(&m, 2, 10).is_err());
    // battle: threshold trunc(sqrt(5 / n) * base * n); E counts as D
    assert_eq!(battle_required_score(2000, 5), 10000);
    assert_eq!(battle_score_rank(&m, 1, 10, 5), RANK_D);
    assert_eq!(battle_required_score(100, 0), i32::MAX);
    assert_eq!(boost_bonus(&m, 0).unwrap(), [1; 5]);
    assert_eq!(boost_bonus(&m, 1).unwrap(), [2, 3, 4, 5, 6]);
    assert!(boost_bonus(&m, 2).is_err());
    assert_eq!(challenge_point_bonus(&m, 200).unwrap(), [1; 5]);
    assert!(is_challenge_live_boost(201));
    assert_eq!(parse_rank("C").unwrap(), RANK_C);
}

#[test]
fn held_event_clock_paths_and_boundaries() {
    let w = EventWindow { event_id: 1, start_jst_ticks: 100, end_jst_ticks: Some(200) };
    assert!(!w.is_holding_at(99));
    assert!(w.is_holding_at(100));
    assert!(w.is_holding_at(199));
    assert!(!w.is_holding_at(200));
    assert!(EventWindow { end_jst_ticks: None, ..w }.is_holding_at(i64::MAX));
    let played = EventResultClock::Played { live_start_jst_ticks: Some(199), server_now_jst_ticks: 201 };
    assert_eq!(holding_event_ids(&[w], played), vec![1]);
    assert!(holding_event_ids(&[w], EventResultClock::Skip { server_now_jst_ticks: 201 }).is_empty());
    let missing_start = EventResultClock::Played { live_start_jst_ticks: None, server_now_jst_ticks: 201 };
    assert!(holding_event_ids(&[w], missing_start).is_empty());
}

#[test]
fn challenge_debit_is_wrapping_clamped_and_precedes_failure() {
    assert_eq!(debit_challenge_points(250, 200), 50);
    assert_eq!(debit_challenge_points(100, 200), 0);
    assert_eq!(debit_challenge_points(i32::MIN, 1), i32::MAX);
    let m = master(json!({"MasterEvent": [{"_id": 1, "_challengeLiveEventPointGroup": 1}]}));
    let mut local = LocalEvent { event_id: 1, challenge_points: 500, ..LocalEvent::default() };
    assert!(consume_challenge_point_and_event(&m, &mut local, &[], Some(&[]), RANK_D, 201).is_err());
    assert_eq!(local.challenge_points, 299);
    assert_eq!(consume_challenge_point_and_event(&m, &mut local, &[], Some(&[]), RANK_D, 200).unwrap(), None);
    assert_eq!(local.challenge_points, 99);
}

fn counter_master() -> Master {
    master(json!({
        "MasterEvent": [
            {"_id": 1, "_liveEventPointGroup": 1, "_challengeLiveEventPointGroup": 2},
            {"_id": 2, "_liveEventPointGroup": 1, "_challengeLiveEventPointGroup": 2},
            {"_id": 3, "_liveEventPointGroup": 99, "_challengeLiveEventPointGroup": 99}
        ],
        "MasterLiveEventPoint": [{"_id": 1, "_group": 1, "_scoreRank": 2, "_value": 10}],
        "MasterChallengeLiveEventPoint": [{"_id": 1, "_group": 2, "_scoreRank": 2, "_value": 20}],
        "MasterLiveChallengePoint": [{"_id": 1, "_scoreRank": 2, "_value": 3}]
    }))
}

#[test]
fn played_missing_local_event_is_error_but_skip_creates_it() {
    let m = counter_master();
    assert!(consume_live_boost_events(&m, &[1], &mut [], &[], Some(&[]), RANK_D, 0).is_err());
    let mut local = Vec::new();
    let out = apply_skip_event_counters(
        &m,
        &[1],
        &mut local,
        &[],
        Some(&[]),
        SkipEventParams { score_rank: RANK_D, event_point_rate: 2, mode: SkipEventMode::Normal },
    )
    .unwrap();
    assert_eq!(out.points, vec![(1, 20)]);
    assert_eq!(local[0].challenge_points, 6);
}

#[test]
fn skip_missing_point_row_logs_without_challenge_points_and_continues() {
    let m = counter_master();
    let mut local = Vec::new();
    let out = apply_skip_event_counters(
        &m,
        &[3, 1],
        &mut local,
        &[],
        Some(&[]),
        SkipEventParams { score_rank: RANK_D, event_point_rate: 1, mode: SkipEventMode::Normal },
    )
    .unwrap();
    assert_eq!(out.points, vec![(1, 10)]);
    assert_eq!(
        out.missing_point_rows,
        vec![MissingSkipEventPoint { event_id: 3, score_rank: RANK_D, is_challenge: false }]
    );
    assert_eq!(local[0].event_id, 3);
    assert_eq!(local[0].points, 0);
    assert_eq!(local[0].challenge_points, 0);
    assert_eq!(local[1].challenge_points, 3);
}

#[test]
fn challenge_skip_only_awards_matching_held_event_and_no_challenge_points() {
    let m = counter_master();
    let mut local = Vec::new();
    let params =
        SkipEventParams { score_rank: RANK_D, event_point_rate: 3, mode: SkipEventMode::Challenge { event_id: 2 } };
    let out = apply_skip_event_counters(&m, &[1, 2], &mut local, &[], Some(&[]), params).unwrap();
    assert_eq!(out.points, vec![(2, 60)]);
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].challenge_points, 0);
    let absent = apply_skip_event_counters(&m, &[1], &mut local, &[], Some(&[]), params).unwrap();
    assert!(absent.points.is_empty());
}

#[test]
fn event_preview_has_no_cross_sample_mutations() {
    let m = counter_master();
    let request = EventPointRequest {
        route: EventResultRoute::ChallengePlayed { event_id: 1 },
        holding_event_ids: vec![1],
        consumed_count: 200,
        local_events: vec![LocalEvent { event_id: 1, challenge_points: 300, ..LocalEvent::default() }],
    };
    let a = preview_client_event_points(&m, &request, &[], Some(&[]), RANK_D).unwrap();
    let b = preview_client_event_points(&m, &request, &[], Some(&[]), RANK_D).unwrap();
    assert_eq!(a.points, b.points);
    assert_eq!(a.points_for(1), 20);
    assert_eq!(a.local_events[0].challenge_points, 100);
    assert_eq!(request.local_events[0].challenge_points, 300);
}

#[test]
fn challenge_skip_preview_debits_even_when_event_ended() {
    let m = counter_master();
    let request = EventPointRequest {
        route: EventResultRoute::ChallengeSkip { event_id: 1 },
        holding_event_ids: vec![],
        consumed_count: 200,
        local_events: vec![LocalEvent { event_id: 1, challenge_points: 300, ..LocalEvent::default() }],
    };
    let out = preview_client_event_points(&m, &request, &[], Some(&[]), RANK_D).unwrap();
    assert_eq!(out.points_for(1), 0);
    assert_eq!(out.local_events[0].challenge_points, 100);
    assert!(out.missing_point_rows.is_empty());
}

#[test]
fn event_request_strict_serde_roundtrip() {
    let text = json!({"route":{"kind":"normalSkip"},"holdingEventIds":[1],"consumedCount":0,"localEvents":[]});
    let request: EventPointRequest = serde_json::from_value(text.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), text);
    let mut bad = text;
    bad["serverPoints"] = json!(123);
    assert!(serde_json::from_value::<EventPointRequest>(bad).is_err());
}

fn item_master() -> Master {
    master(json!({
        "MasterEvent":[{"_id":1},{"_id":2}],
        "MasterEventEffect":[
            effect(1,1,EVENT_ITEM,2,json!({}),[2500;5]),
            effect(2,2,EVENT_ITEM,2,json!({}),[20000;5])
        ],
        "MasterLiveEventReward":[{"_id":1,"_resourceType":4,"_resourceId":88,"_resourceCount":7,"_probability":1}],
        "MasterChallengeLiveEventReward":[{"_id":1,"_resourceType":4,"_resourceId":88,"_resourceCount":7,"_probability":999}],
        "MasterLiveMusicBoostBonus":[{"_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":2,"_eventPointRate":99}]
    }))
}

#[test]
fn selected_item_rewards_use_reward_rate_and_preserve_server_selection() {
    let m = item_master();
    let card = member(1, 1, Some(1), 1, &[], 1);
    let mut request = EventItemRequest {
        route: EventResultRoute::NormalPlayed,
        consumed_count: 1,
        local_event_ids: vec![1],
        selected_rewards: Some(vec![ServerEventReward { event_id: 1, reward_id: 1 }]),
    };
    let out = preview_client_event_items(&m, &request, &[Some(&card)], Some(&[])).unwrap();
    assert_eq!(out.rewards[0].amount, 17);
    assert_eq!(out.rewards[0].resource_id, 88);
    request.local_event_ids.clear();
    assert_eq!(preview_client_event_items(&m, &request, &[Some(&card)], Some(&[])).unwrap().rewards[0].amount, 14);
    request.route = EventResultRoute::NormalSkip;
    assert!(preview_client_event_items(&m, &request, &[Some(&card)], Some(&[])).is_err());
}

#[test]
fn challenge_played_item_bonus_uses_selected_event_not_reward_event() {
    let m = item_master();
    let card = member(1, 1, Some(1), 1, &[], 1);
    let mut request = EventItemRequest {
        route: EventResultRoute::ChallengePlayed { event_id: 1 },
        consumed_count: 200,
        local_event_ids: vec![1, 2],
        selected_rewards: Some(vec![ServerEventReward { event_id: 2, reward_id: 1 }]),
    };
    assert_eq!(preview_client_event_items(&m, &request, &[Some(&card)], Some(&[])).unwrap().rewards[0].amount, 8);
    request.route = EventResultRoute::ChallengeSkip { event_id: 1 };
    assert_eq!(preview_client_event_items(&m, &request, &[Some(&card)], Some(&[])).unwrap().rewards[0].amount, 21);
}

#[test]
fn item_unknown_selection_and_missing_rows_are_not_zero_predictions() {
    let m = item_master();
    let mut request = EventItemRequest {
        route: EventResultRoute::NormalPlayed,
        consumed_count: 0,
        local_event_ids: vec![],
        selected_rewards: None,
    };
    assert!(matches!(
        preview_client_event_items(&m, &request, &[], Some(&[])),
        Err(ournotes_deck::Error::Unsupported(_))
    ));
    request.selected_rewards = Some(vec![]);
    assert!(preview_client_event_items(&m, &request, &[], Some(&[])).unwrap().rewards.is_empty());
    request.selected_rewards = Some(vec![ServerEventReward { event_id: 1, reward_id: 999 }]);
    assert!(preview_client_event_items(&m, &request, &[], Some(&[])).is_err());
    for route in [
        EventResultRoute::NormalSkip,
        EventResultRoute::ChallengePlayed { event_id: 1 },
        EventResultRoute::ChallengeSkip { event_id: 1 },
    ] {
        request.route = route;
        let out = preview_client_event_items(&m, &request, &[], Some(&[])).unwrap();
        assert_eq!(out.missing_reward_ids, vec![999]);
        assert!(out.rewards.is_empty());
    }
}

#[test]
fn canonical_jst_date_has_explicit_domain_and_tick_precision() {
    assert_eq!(parse_master_jst_canonical("0001-01-01").unwrap(), 0);
    assert_eq!(parse_master_jst_canonical("1970-01-01").unwrap(), 621355968000000000);
    assert_eq!(parse_master_jst_canonical("9999-12-31T23:59:59.9999999").unwrap(), 3155378975999999999);
    let x = parse_master_jst_canonical("2024-02-29 00:00:00.0000001").unwrap();
    assert_eq!(x - parse_master_jst_canonical("2024-02-28").unwrap(), 864000000001);
    for text in ["2023-02-29", "2024-01-01T24:00:00", "2024-00-01", "0000-01-01"] {
        assert!(parse_master_jst_canonical(text).is_err(), "{text}");
    }
    for text in ["2024-01-01T00:00:00Z", "2024-01-01T00:00:00+09:00", "1/1/2024", " 2024-01-01", ""] {
        assert!(matches!(parse_master_jst_canonical(text), Err(ournotes_deck::Error::Unsupported(_))), "{text}");
    }
}

#[test]
fn master_event_unset_start_and_end_are_distinct() {
    let m = master(json!({"MasterEvent":[{"_id":1,"_startAt":null,"_endAt":"null"},
        {"_id":2,"_startAt":"2024-01-01","_endAt":"2024-01-02"}]}));
    let empty = event_window_canonical(m.event(1).unwrap()).unwrap();
    assert_eq!(empty.start_jst_ticks, 0);
    assert_eq!(empty.end_jst_ticks, None);
    let w = event_window_canonical(m.event(2).unwrap()).unwrap();
    assert!(w.is_holding_at(w.start_jst_ticks));
    assert!(!w.is_holding_at(w.end_jst_ticks.unwrap()));
}

#[test]
fn modern_utc_adapters_do_not_conflate_start_and_settlement() {
    let start = parse_master_jst_canonical("2026-09-29T14:59:59.9999999").unwrap();
    let now = parse_master_jst_canonical("2026-09-29T15:00:00").unwrap();
    assert_eq!(utc_to_jst_ticks_modern(now).unwrap(), parse_master_jst_canonical("2026-09-30").unwrap());
    let played = EventResultClock::played_from_utc_ticks(Some(start), now).unwrap();
    let skip = EventResultClock::skip_from_utc_ticks(now).unwrap();
    assert_eq!(skip.jst_ticks() - played.jst_ticks(), 1);
    assert_eq!(EventResultClock::played_from_utc_ticks(None, now).unwrap().jst_ticks(), skip.jst_ticks());
    assert!(utc_to_jst_ticks_modern(0).is_err());
}

#[test]
fn achievement_queue_is_sorted_then_looped_without_claiming_inventory() {
    let m = master(json!({"MasterEvent":[{"_id":1}],"MasterEventAchievementReward":[
        {"_id":1,"_eventId":1,"_eventPoint":200,"_rewardIds":[90]},
        {"_id":2,"_eventId":1,"_eventPoint":100,"_rewardIds":[91]},
        {"_id":3,"_eventId":1,"_eventPoint":200,"_rewardIds":[92]}],
        "MasterEventAchievementLoopReward":[{"_id":4,"_eventId":1,"_loopStartEventPoint":200,"_loopEventPoint":50,"_rewardIds":[93]},
            {"_id":5,"_eventId":1,"_loopStartEventPoint":0,"_loopEventPoint":1,"_rewardIds":[94]}]}));
    let out = preview_event_achievement_update(&m, 1, 99, 251).unwrap();
    assert_eq!(out.new_points, 350);
    assert_eq!(
        out.notices.iter().map(|n| (n.row_id, n.count)).collect::<Vec<_>>(),
        vec![(2, 1), (1, 1), (3, 1), (4, 3)]
    );
    assert_eq!(out.duplicate_loop_row_count, 2);
    assert!(preview_event_achievement_update(&m, 1, 350, -300).unwrap().notices.is_empty());
    assert!(preview_event_achievement_update(&m, 999, 0, 0).unwrap().notices.is_empty());
}

#[test]
fn invalid_loop_step_is_logged_without_using_later_matching_row() {
    let m = master(json!({"MasterEvent":[{"_id":1}],"MasterEventAchievementLoopReward":[
        {"_id":4,"_eventId":1,"_loopStartEventPoint":0,"_loopEventPoint":0},
        {"_id":5,"_eventId":1,"_loopStartEventPoint":0,"_loopEventPoint":1}]}));
    let out = preview_event_achievement_update(&m, 1, 0, 100).unwrap();
    assert!(out.notices.is_empty());
    assert_eq!(out.invalid_loop_reward_ids, vec![4]);
    assert_eq!(loop_reward_count(100, 0, 0), 0);
    assert_eq!(loop_reward_count(i32::MIN, 0, -1), 0);
    assert!(!achievement_reward_reached(10, 10, 20));
    assert!(achievement_reward_reached(10, 9, 10));
}
