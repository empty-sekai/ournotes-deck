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
