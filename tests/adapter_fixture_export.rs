//! Explicitly synthetic UTF-8 transport corpus input; no game assets or native truth.
mod common;
use common::{Rng, Synth, extend_table, replace_table, set_column, synth_snaps};
use ournotes_deck::{cards::Roster, data::DeckData};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
const FIXTURE_SEED: u64 = 20_261_001;
const SCORE_ID: i64 = 1004;
const EVENT_ID: i64 = 7;

fn synthetic_master(members: i64, snaps: i64, characters: i64) -> Synth {
    let mut s = synth_snaps(&mut Rng::new(FIXTURE_SEED), members, snaps, &[1, 3, 6, 10, 11]);
    replace_table(
        &mut s,
        "MasterCharacter",
        Value::Array((1..=characters).map(|id| json!({"_id":id,"_bandID":(id-1)%3+1})).collect()),
    );
    set_column(&mut s, "MasterMemberCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_characterID"] = json!((id - 1) % characters + 1);
        r["_cardType"] = json!((id - 1) % 5 + 1);
        r["_liveSkillID"] = json!((id - 1) % 3 + 1);
        r["_leaderSkillID"] = json!(4);
    });
    set_column(&mut s, "MasterSupportCard", &mut |r| {
        let id = r["_id"].as_i64().unwrap();
        r["_characterIDs"] = json!([(id - 1) % characters + 1]);
        r["_supportSkillId01"] = json!(if id % 2 == 1 { 3 } else { 10 });
        r["_supportSkillId02"] = json!(if id % 2 == 1 { 1 } else { 6 });
    });
    replace_table(
        &mut s,
        "MasterLiveMusic",
        json!([{
            "_id":10,"_musicType":1,"_bestMusicTagIDs":[1],"_expertID":SCORE_ID,
            "_liveScoreRankGroup":1,"_gekisouMission1":1,"_gekisouMission2":2,"_gekisouMission3":3
        }]),
    );
    replace_table(
        &mut s,
        "MasterLiveMusicScore",
        json!([{
            "_id":SCORE_ID,"_musicScoreLevel":24,"_fullComboCount":12
        }]),
    );
    replace_table(
        &mut s,
        "MasterChallengeMusic",
        json!([
            {"_id":70,"_eventId":EVENT_ID,"_liveMusicId":10,"_musicType":4,"_bestMusicTagIDs":[2]},
            {"_id":71,"_eventId":EVENT_ID,"_liveMusicId":10,"_musicType":0,"_bestMusicTagIDs":[]}
        ]),
    );
    replace_table(
        &mut s,
        "MasterArenaMusic",
        json!([{
            "_id":80,"_liveMusicId":10,"_liveMusicType":5,
            "_gekisouMission1":3,"_gekisouMission2":2,"_gekisouMission3":1
        }]),
    );
    replace_table(
        &mut s,
        "MasterLiveJudgementTiming",
        json!([
            {"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_beforeMs":40,"_afterMs":40},
            {"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_beforeMs":80,"_afterMs":80},
            {"_id":3,"_noteJudgementType":2,"_noteSimulateJudgement":5,"_beforeMs":80,"_afterMs":80}
        ]),
    );
    extend_table(
        &mut s,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"40"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"20"}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    extend_table(
        &mut s,
        "MasterLiveComboScoreBonus",
        (1..=4).map(|i| json!({"_id":100+i,"_comboBonusType":1,"_requiredComboCount":i,"_bonusFactor":0.02})).collect(),
    );
    replace_table(
        &mut s,
        "MasterLiveGekisouRankingScoreBonus",
        Value::Array(
            (1..=3)
                .flat_map(|pattern| {
                    (1..=3).map(move |count| {
                        json!({"_id":pattern*10+count,"_missionPattern":pattern,
            "_count":count,"_rank":1,"_scoreBonusPercent":10})
                    })
                })
                .collect(),
        ),
    );
    replace_table(&mut s, "MasterLiveGekisouLuckBasePoint", Value::Array((3..=6).map(|judgement| {
        json!({"_id":judgement,"_noteCategory":0,"_noteSimulateJudgement":judgement,"_weight":1,"_basePoint":10})
    }).collect()));
    replace_table(
        &mut s,
        "MasterLiveGekisouLuckBonusLot",
        Value::Array(
            (0..5)
                .flat_map(|kind| {
                    (0..4).map(move |result| {
                        json!({"_id":kind*10+result+1,"_chanceLotType":kind,
            "_lotResult":result,"_weight":([5,4,2,1][result as usize])})
                    })
                })
                .collect(),
        ),
    );
    // Own skill-event trigger, with a real probability checker in the synthetic effect table.
    replace_table(
        &mut s,
        "MasterGekisouSkillEffect",
        json!([{
            "_id":1,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":1,
            "_skillTriggerConditionGroup":53,"_skillConditionGroup":66,"_skillReleaseConditionGroup":0,
            "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.4,"_effectValue":900,
            "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
            "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
        }]),
    );
    replace_table(
        &mut s,
        "MasterEvent",
        json!([{
            "_id":EVENT_ID,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2
        }]),
    );
    replace_table(
        &mut s,
        "MasterEventEffect",
        Value::Array(
            (0..=2)
                .map(|kind| {
                    json!({"_id":kind+1,"_eventId":EVENT_ID,"_eventBonusType":kind,"_resourceTypeConstraint":2,
            "_rank1EffectValue":1000,"_rank2EffectValue":1000,"_rank3EffectValue":1000,
            "_rank4EffectValue":1000,"_rank5EffectValue":1000})
                })
                .collect(),
        ),
    );
    replace_table(&mut s, "MasterLiveScoreRank", Value::Array([0,300_000,450_000,600_000].iter().enumerate().map(|(i, score)| {
        json!({"_id":i+1,"_group":1,"_liveScoreRank":i+2,"_requiredScore":score,"_battleLiveRequiredScore":score})
    }).collect()));
    for (table, group, base) in [
        ("MasterLiveEventPoint", 1, 100),
        ("MasterChallengeLiveEventPoint", 2, 300),
        ("MasterLiveChallengePoint", 1, 5),
    ] {
        replace_table(
            &mut s,
            table,
            Value::Array(
                (2..=5)
                    .map(|rank| json!({"_id":rank,"_group":group,"_scoreRank":rank,"_value":base+(rank-2)*base/3}))
                    .collect(),
            ),
        );
    }
    replace_table(
        &mut s,
        "MasterLiveMusicBoostBonus",
        json!([{
            "_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":2,"_playerExpRate":2,
            "_memberCardExpRate":2,"_friendshipExpRate":2,"_eventPointRate":2
        }]),
    );
    replace_table(
        &mut s,
        "MasterChallengeMusicBoostBonus",
        json!([{
            "_id":1,"_consumedChallengePointCount":201,"_liveMusicRewardRate":2,"_playerExpRate":2,
            "_memberCardExpRate":2,"_friendshipExpRate":2,"_eventPointRate":2
        }]),
    );
    for (table, count) in [("MasterLiveEventReward", 3), ("MasterChallengeLiveEventReward", 4)] {
        replace_table(
            &mut s,
            table,
            json!([{
                "_id":5,"_group":1,"_eventGroup":1,"_scoreRank":2,"_resourceType":11,
                "_resourceId":9,"_resourceCount":count,"_probability":999
            }]),
        );
    }
    replace_table(
        &mut s,
        "MasterEventAchievementReward",
        json!([{
            "_id":1,"_eventId":EVENT_ID,"_eventPoint":100,"_rewardIds":[5]
        }]),
    );
    s
}

fn columns_and_rows(s: &Synth) -> Value {
    let master: BTreeMap<_, _> = s
        .tables
        .iter()
        .map(|(name, rows)| {
            let objects = rows.as_array().expect("synthetic table is an array");
            let columns: Vec<_> = objects
                .iter()
                .flat_map(|r| r.as_object().unwrap().keys().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let data_rows: Vec<Vec<Value>> = objects
                .iter()
                .map(|r| columns.iter().map(|key| r.get(key).cloned().unwrap_or(Value::Null)).collect())
                .collect();
            (name.clone(), json!({"columns":columns,"rows":data_rows}))
        })
        .collect();
    json!(master)
}

fn data_document(s: &Synth, members: i64, snaps: i64, characters: i64) -> Value {
    json!({
        "format":"nnnotes.deck-data/1",
        "provenance":{"synthetic":true,"fixtureSeed":FIXTURE_SEED,"region":"synthetic",
            "masterVersion":"synthetic-recommend-fixture-20261001","clientVersion":"synthetic-inputs",
            "source":"tests/recommend_fixture_export.rs + tests/common::synth_snaps",
            "members":members,"snaps":snaps,"characters":characters,"isOCRTruth":false,
            "assetSha256Policy":"64 zero placeholder; no game asset or real chart is claimed"},
        "master":columns_and_rows(s),
        "charts":[{"scoreId":SCORE_ID,"asset":{"key":"SYNTHETIC-short-chart-1004",
            "sha256":"0000000000000000000000000000000000000000000000000000000000000000"},
            "notes":{"id":(1..=12).collect::<Vec<_>>(),"op":vec![1;12],
                "judgementType":vec![1;12],"timeMs":(1..=12).map(|i|i*100).collect::<Vec<_>>()},
            "skillEvents":{"timeMs":[0,250,500,750,1000]},
            "fevers":{"startMs":[150,450,850],"endMs":[400,800,1150]}}]
    })
}

fn roster_document(members: i64, snaps: i64, characters: i64) -> Value {
    let ranks: BTreeMap<_, _> = (1..=characters).map(|id| (id.to_string(), 10)).collect();
    json!({
        "provenance":{"synthetic":true,"isOCRTruth":false,"source":"manually fixed synthetic progress"},
        "player":{"characterRanks":ranks,"bandItems":{},"vipRank":3,"events":[EVENT_ID],
            "memory":null,"ownedMemberCardIds":(1..=members).collect::<Vec<_>>(),
            "ownedSupportCardIds":(1..=snaps).collect::<Vec<_>>()},
        "members":(1..=members).map(|id| json!({"id":id,"level":40,"exp":null,"awake":2,
            "rank":3,"liveSkillLevel":4,"gekisouSkillLevel":1})).collect::<Vec<_>>(),
        "snaps":(1..=snaps).map(|id| json!({"id":id,"level":30,"exp":null,"rank":3})).collect::<Vec<_>>()
    })
}

fn context_document(skip: bool, multiplayer: bool, expired: bool) -> Value {
    let mut value = json!({
        "powerSnapshot":{"eventIds":[EVENT_ID],"capturedJstTicks":50},
        "resultClock":if skip {json!({"execution":"skip","serverNowJstTicks":if expired {200} else {150}})}
            else {json!({"execution":"played","savedStartJstTicks":150,"serverNowJstTicks":201})},
        "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":EVENT_ID,"points":0,
            "challengePoints":500,"added":[]}],"eventWindows":[{"eventId":EVENT_ID,
            "startJstTicks":100,"endJstTicks":200}],"selectedRewards":[{"eventId":EVENT_ID,"rewardId":5}]}
    });
    if multiplayer {
        value["eventPayoff"]["multiplayerResultPanel"] = json!({"localPlayerIndex":1,
            "localDisconnected":false,"otherPlayers":[{"finalScore":100000,"disconnected":false},
                {"finalScore":50000,"disconnected":true}]});
    }
    value
}

#[test]
#[ignore = "export manual synthetic input for explicit CLI/WASM checks"]
fn export_adapter_inputs() {
    let directory = std::env::var_os("BDON_FIXTURE_OUT").expect("BDON_FIXTURE_OUT");
    let out = Path::new(&directory);
    fs::create_dir_all(out).unwrap();
    let synth = synthetic_master(7, 3, 7);
    let document = data_document(&synth, 7, 3, 7);
    let roster = roster_document(7, 3, 7);
    let data = DeckData::from_json(&document.to_string()).unwrap();
    Roster::from_json(&roster.to_string()).unwrap();
    let stream = ournotes_deck::live::model::JudgementStream::theoretical_best(&data.chart(SCORE_ID).unwrap());
    for (name, value) in [
        ("DeckData.json", document),
        ("roster.json", roster),
        ("play-ordinary.json", serde_json::to_value(stream).unwrap()),
        ("context-played.json", context_document(false, false, false)),
        ("context-skip.json", context_document(true, false, false)),
    ] {
        let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
        bytes.push(b'\n');
        fs::write(out.join(name), bytes).unwrap();
    }
}

#[test]
fn unsupported_lifecycle_rejected_before_low_level_feasibility_or_validation() {
    use ournotes_deck::{
        Error,
        search::{
            Constraints, Objective, Pool, SearchRequest,
            expectation::FiniteSeedLaw,
            recommendation::{Limits, Metric, SimulationInput, Strategy, solve_physical},
        },
    };
    let synth = synthetic_master(7, 3, 7);
    let data = DeckData::from_json(&data_document(&synth, 7, 3, 7).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(7, 3, 7).to_string()).unwrap();
    let pool = Pool::new(&data.master, &roster).unwrap();
    let request = SearchRequest {
        objective: Objective::Power { music_id: Some(10), event: false },
        k: 1,
        constraints: Constraints { include_members: vec![999], ..Default::default() },
        time_limit: None,
    };
    let law = FiniteSeedLaw::new(vec![(1, 1)]).unwrap();
    for (network, finished) in [(Some(&[][..]), None), (None, Some(0))] {
        let result = solve_physical(
            &pool,
            &request,
            &law,
            &Metric::Power,
            None,
            &Limits::default(),
            &Strategy::Exhaustive,
            network,
            &SimulationInput { live_finished_from_frame: finished, ..Default::default() },
        );
        assert!(matches!(result, Err(Error::Unsupported(_))));
    }
}
