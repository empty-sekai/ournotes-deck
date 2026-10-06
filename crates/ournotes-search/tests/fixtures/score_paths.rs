//! Synthetic score-path inputs with independent growth, chart length, and LUCK controls.

use super::common::{extend_table, replace_table, set_column};
use super::{EVENT_ID, SCORE_ID, data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub(super) fn inputs(
    members: i64,
    snaps: i64,
    characters: i64,
    skewed: bool,
    long: bool,
    luck: bool,
) -> (Value, Value) {
    inputs_with_tiers(members, snaps, characters, skewed, long, luck, false)
}

fn inputs_with_tiers(
    members: i64,
    snaps: i64,
    characters: i64,
    skewed: bool,
    long: bool,
    luck: bool,
    tiered: bool,
) -> (Value, Value) {
    let mut master = synthetic_master(members, snaps, characters);
    extend_table(
        &mut master,
        "MasterMemberCardLevelLimit",
        (1..=5)
            .flat_map(|rarity| {
                (1..=5).map(
                    move |awake| json!({"_id":rarity*10+awake,"_rarity":rarity,"_awakeCount":awake,"_limitLevel":40}),
                )
            })
            .collect(),
    );
    replace_table(
        &mut master,
        "MasterBandItem",
        json!((1..=3).map(|band| json!({"_id":100+band,"_bandID":band})).collect::<Vec<_>>()),
    );
    replace_table(
        &mut master,
        "MasterBandItemLevel",
        json!(
            (1..=3)
                .flat_map(|band| (1..=10).map(
                    move |level| json!({"_id":band*100+level,"_bandItemId":100+band,"_level":level,"_playerRank":level})
                ))
                .collect::<Vec<_>>()
        ),
    );
    set_column(&mut master, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(if luck { 2 } else { 1 });
        row["_gekisouMission2"] = json!(3);
        row["_gekisouMission3"] = json!(1);
    });
    extend_table(
        &mut master,
        "MasterLiveGekisouRankingScoreBonus",
        (1..=3)
            .flat_map(|pattern| {
                (1..=3).flat_map(move |count| {
                    (2..=5).map(move |rank| {
                        json!({"_id":100+pattern*100+count*10+rank,"_missionPattern":pattern,
                            "_count":count,"_rank":rank,"_scoreBonusPercent":([10,7,4,2,1][rank as usize-1])})
                    })
                })
            })
            .collect(),
    );
    set_column(&mut master, "MasterLiveSkillEffect", &mut |row| {
        let skill = row["_liveSkillID"].as_i64().unwrap();
        row["_effectValue"] = json!(4000 + 2000 * skill + 500 * row["_level"].as_i64().unwrap());
        row["_activationTimeSecond"] = json!(if long { 6.0 } else { 1.2 });
    });
    set_column(&mut master, "MasterGekisouSkillEffect", &mut |row| {
        row["_effectValue"] = json!(8000);
        row["_activationTimeSecond"] = json!(if long { 4.0 } else { 0.9 });
    });
    set_column(&mut master, "MasterLiveSettings", &mut |row| {
        if matches!(row["_key"].as_str(), Some("gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush")) {
            row["_value"] = json!("20");
        }
    });
    if tiered {
        let thresholds = if skewed {
            [0, 300_000, 450_000, 600_000]
        } else if long {
            [0, 600_000, 650_000, 700_000]
        } else {
            [0, 450_000, 550_000, 650_000]
        };
        set_column(&mut master, "MasterLiveScoreRank", &mut |row| {
            let index = row["_liveScoreRank"].as_u64().unwrap() as usize - 2;
            row["_requiredScore"] = json!(thresholds[index]);
            row["_battleLiveRequiredScore"] = json!(thresholds[index]);
        });
        extend_table(
            &mut master,
            "MasterEventEffect",
            (1..=members)
                .map(|id| {
                    let value = 100 * (1 + ((id + 1) / 2) % 5);
                    json!({"_id":1000+id,"_eventId":EVENT_ID,"_eventBonusType":0,
                        "_resourceTypeConstraint":2,"_memberCardId":id,
                        "_rank1EffectValue":value,"_rank2EffectValue":value,"_rank3EffectValue":value,
                        "_rank4EffectValue":value,"_rank5EffectValue":value})
                })
                .collect(),
        );
    }
    let n = if long { 256 } else { 12 };
    set_column(&mut master, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(n));
    let mut document = data_document(&master, members, snaps, characters);
    document["provenance"]["source"] = json!("crates/ournotes-search/tests/fixtures/score_paths.rs");
    document["charts"][0]["asset"]["key"] = json!(format!("SYNTHETIC-score-path-{n}"));
    document["charts"][0]["notes"] = json!({
        "id":(1..=n).collect::<Vec<_>>(), "op":vec![1;n as usize],
        "judgementType":vec![1;n as usize], "timeMs":(1..=n).map(|i|i*if long {100} else {300}).collect::<Vec<_>>()
    });
    if long {
        document["charts"][0]["skillEvents"] = json!({"timeMs":[0,5000,10000,15000,20000]});
        document["charts"][0]["fevers"] = json!({"startMs":[150,9800,18400],"endMs":[400,17000,25000]});
    } else {
        document["charts"][0]["skillEvents"] = json!({"timeMs":[0,600,1200,1800,2400]});
        document["charts"][0]["fevers"] = json!({"startMs":[150,1600,2800],"endMs":[700,2000,3200]});
    }
    let mut roster = roster_document(members, snaps, characters);
    roster["player"]["vipRank"] = json!(1);
    for id in 1..=characters {
        roster["player"]["characterRanks"][id.to_string()] = json!(if skewed && (id - 1) % 3 == 0 { 30 } else { 1 });
    }
    if skewed {
        roster["player"]["bandItems"] = json!({"101":10});
    }
    for row in roster["members"].as_array_mut().unwrap() {
        row["level"] = json!(if skewed { 20 } else { 1 });
        row["awake"] = json!(1);
        row["liveSkillLevel"] = json!(5);
    }
    for row in roster["snaps"].as_array_mut().unwrap() {
        row["level"] = json!(if skewed { 15 } else { 1 });
    }
    (document, roster)
}

pub(super) fn request(metric: Value, timed: bool) -> Value {
    let mut request = joint_request_json("mission", true, metric);
    request["constraints"] = json!({});
    request["k"] = json!(5);
    request["limits"] = json!({"timeLimitMs":if timed { Some(60_000) } else { None },
        "maxCandidates":null,"cacheEntries":64});
    request
}

pub(super) fn rank_skewed(request: &mut Value, long: bool) {
    request["scenario"]["kind"] = json!("battle");
    request["networkConfirmations"] = json!([
        {"frame":if long {30} else {50},"range":0,"rank":1,"percent":10},
        {"frame":if long {1027} else {128},"range":1,"rank":3,"percent":4},
        {"frame":if long {1507} else {200},"range":2,"rank":5,"percent":1}
    ]);
    request["context"]["eventPayoff"]["multiplayerScorePolicy"] =
        json!({"kind":"fixedOthersAverage","players":3,"score":150000});
}

fn write(out: &Path, name: &str, value: &Value) {
    let mut text = serde_json::to_vec_pretty(value).unwrap();
    text.push(b'\n');
    fs::write(out.join(name), text).unwrap();
}

fn snapshot(data: &DeckData, roster: &Value) -> Value {
    json!({
        "format":"ournotes.owned-snapshot/1","datasetId":data.sha256,"revision":"synthetic-score-paths-1",
        "ownedFacts":{"memberIds":roster["player"]["ownedMemberCardIds"],
            "snapIds":roster["player"]["ownedSupportCardIds"],"memberCoverage":"complete","snapCoverage":"complete"},
        "eligible":{"members":roster["members"],"snaps":roster["snaps"]},
        "player":{"characterRanks":{"coverage":"complete","values":roster["player"]["characterRanks"]
            .as_object().unwrap().iter().map(|(id,value)|json!({"id":id.parse::<i64>().unwrap(),"value":value}))
            .collect::<Vec<_>>()},"characterTotalRank":null,"vipRank":roster["player"]["vipRank"],
            "bandItems":roster["player"]["bandItems"].as_object().unwrap().iter()
                .map(|(id,level)|json!({"id":id.parse::<i64>().unwrap(),"value":level})).collect::<Vec<_>>(),
            "memory":{"musicRanks":[],"unlockedMembers":[],"unlockedSnaps":[]},"eventIds":[EVENT_ID]},
        "assumptions":[]
    })
}

#[test]
fn nominal_luck_score_paths_complete_fixed_decks() {
    use ournotes_search::{auxiliary::evaluate_fixed, search::Completion, types::RecommendationRequest};
    for skewed in [false, true] {
        for long in [false, true] {
            let (document, roster) = inputs(5, 3, 5, skewed, long, true);
            let data = DeckData::from_json(&document.to_string()).unwrap();
            let roster = Roster::from_json(&roster.to_string()).unwrap();
            let mut wire = request(json!({"kind":"score"}), false);
            if skewed {
                rank_skewed(&mut wire, long);
            }
            let request: RecommendationRequest = serde_json::from_value(wire).unwrap();
            let outcome =
                evaluate_fixed(&data, &roster, &request, [1, 2, 3, 4, 5], [Some(1), Some(2), None, None, Some(3)])
                    .unwrap();
            assert_eq!(outcome.completion, Completion::Complete, "skewed={skewed} long={long}");
            assert_eq!(outcome.results.len(), 1);
            assert_eq!(outcome.probability_law["lottery"], "certifiedNativeLotteryIntervals");
            assert!(outcome.results[0].score_interval.is_some());
        }
    }
}

#[test]
#[ignore = "export synthetic score-path matrix to OURNOTES_SCORE_PATH_OUT"]
fn export_score_path_matrix() {
    let directory = std::env::var_os("OURNOTES_SCORE_PATH_OUT").expect("OURNOTES_SCORE_PATH_OUT");
    export_matrix(Path::new(&directory), false);
}

#[test]
#[ignore = "export synthetic payoff-tier matrix to OURNOTES_SCORE_PATH_TIER_OUT"]
fn export_score_path_tier_matrix() {
    let directory = std::env::var_os("OURNOTES_SCORE_PATH_TIER_OUT").expect("OURNOTES_SCORE_PATH_TIER_OUT");
    export_matrix(Path::new(&directory), true);
}

fn export_matrix(out: &Path, tiered: bool) {
    fs::create_dir_all(out).unwrap();
    let mut pressure = Vec::new();
    let mut oracle = Vec::new();
    for (size, members, snaps, characters, candidates) in [("bounded", 6, 1, 5, 60), ("pressure", 8, 3, 6, 13600)] {
        for skewed in [false, true] {
            for long in [false, true] {
                for luck in [false, true] {
                    let family = if skewed { "rank-skewed" } else { "low-power-strong-skill" };
                    let length = if long { "long" } else { "short" };
                    let law = if luck { "luck" } else { "no-luck" };
                    let prefix = if tiered { "tiers-" } else { "" };
                    let id = format!("{prefix}{size}-{family}-{length}-{law}");
                    let (document, roster) = inputs_with_tiers(members, snaps, characters, skewed, long, luck, tiered);
                    let data_name = format!("{id}-data.json");
                    let roster_name = format!("{id}-roster.json");
                    let snapshot_name = format!("{id}-snapshot.json");
                    write(out, &data_name, &document);
                    write(out, &roster_name, &roster);
                    let data = DeckData::from_path(out.join(&data_name)).unwrap();
                    let frames =
                        ournotes_sim::live::model::JudgementStream::theoretical_best(&data.chart(SCORE_ID).unwrap())
                            .frames
                            .len();
                    Roster::from_json(&roster.to_string()).unwrap();
                    let snapshot = snapshot(&data, &roster);
                    let resolved = ournotes_search::owned_snapshot::OwnedSnapshot::from_json(&snapshot.to_string())
                        .unwrap()
                        .resolve(
                            &data.master,
                            data.sha256.as_deref().unwrap(),
                            ournotes_search::owned_snapshot::GoalDependencies::GekisouLive,
                        );
                    assert!(resolved.resolved.is_some(), "{id}: {:?} {:?}", resolved.missing, resolved.errors);
                    write(out, &snapshot_name, &snapshot);
                    for (objective, metric) in [
                        ("score", json!({"kind":"score"})),
                        ("pt", json!({"kind":"clientEventPoints","eventId":EVENT_ID})),
                    ] {
                        if size == "pressure" && objective != "pt" {
                            continue;
                        }
                        let name = format!("{id}-{objective}");
                        let request_name = format!("{name}-request.json");
                        let mut request = request(metric, size == "pressure");
                        if skewed {
                            rank_skewed(&mut request, long);
                        }
                        write(out, &request_name, &request);
                        if size == "bounded" && !luck {
                            let case_name = format!("{name}.json");
                            write(
                                out,
                                &case_name,
                                &json!({"id":name,"data":data_name,"roster":roster_name,
                                "request":request_name,"oracleMaxCandidates":candidates,"experiments":[
                                    {"name":"exhaustive","patch":{"strategy":{"kind":"exhaustive"}},"repeats":1},
                                    {"name":"joint-bnb","patch":{"strategy":{"kind":"branchAndBound"}},"repeats":2}]}),
                            );
                            oracle.push(case_name);
                        }
                        pressure.push(json!({"name":name,"data":data_name,"roster":roster_name,
                            "snapshot":snapshot_name,"request":request_name,"physicalCandidates":candidates,
                            "notes":if long {256} else {12},"frames":frames,"luck":luck,
                            "boundedOracle":size=="bounded" && !luck}));
                    }
                }
            }
        }
    }
    write(
        out,
        "matrix.json",
        &json!({"format":"ournotes-deck.score-path-matrix/1","scope":"synthetic current-model",
        "scoreId":SCORE_ID,"cases":pressure}),
    );
    write(
        out,
        "suite.json",
        &json!({"format":"ournotes-deck.search-harness-suite/1", "scope":"synthetic current-model",
        "cases":oracle}),
    );
}
