//! Account requests keep the parsing/resolution origin when entering the real solver.
//! A private earlier Instant represents time already consumed; these tests never sleep.
use super::{Answer, AnswerResult, Status, recommend, recommend_started};
use crate::{
    clock::Instant,
    search::test_common::{Rng, every_table, replace_table, set_column, synth},
};
use ournotes_sim::data::DeckData;
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

fn fixture() -> (DeckData, Value) {
    let mut tables = synth(&mut Rng::new(20261005), 6, 0);
    set_column(&mut tables, "MasterMemberCard", &mut |row| {
        row["_characterID"] = row["_id"].clone();
        row["_rarity"] = json!(1);
        row["_leaderSkillID"] = json!(0);
    });
    set_column(&mut tables, "MasterCharacterRank", &mut |row| {
        row["_exp"] = json!((row["_rank"].as_i64().unwrap() - 1) * 100);
    });
    replace_table(
        &mut tables,
        "MasterMemberCardLevelLimit",
        json!([{"_id":1,"_rarity":1,"_awakeCount":1,"_limitLevel":40}]),
    );
    let mut master = serde_json::Map::new();
    for (name, rows) in &tables.tables {
        let rows = rows.as_array().unwrap();
        let columns: BTreeSet<_> = rows.iter().flat_map(|row| row.as_object().unwrap().keys().cloned()).collect();
        let columns: Vec<_> = columns.into_iter().collect();
        let rows: Vec<Vec<Value>> =
            rows.iter().map(|row| columns.iter().map(|column| row[column].clone()).collect()).collect();
        master.insert(name.clone(), json!({"columns":columns,"rows":rows}));
    }
    every_table(&mut master);
    let data = DeckData::from_json(
        &json!({"format":"nnnotes.deck-data/1","provenance":{"region":"jp","synthetic":true},
            "master":master,"charts":[]})
        .to_string(),
    )
    .unwrap();
    let account = json!({
        "format":"ournotes.account/1","datasetId":data.sha256,"server":"jp","revision":"request-budget-fixture",
        "coverage":{
            "_player._memberCards":"complete","_player._supportCards":"complete","_player._characters":"complete",
            "_player._bandItems":"complete","_player._memory._musicGroups":"complete",
            "_player._memory._members":"complete","_player._memory._supports":"complete"
        },
        "assumptions":[],"declared":{"_vip":{"_rank":1}},
        "account":{"_player":{
            "_memberCards":(1..=6).map(|id| json!({"_masterId":id,"_exp":0,"_awakeCount":1,"_rank":1})).collect::<Vec<_>>(),
            "_supportCards":[],"_characters":[],"_bandItems":[],
            "_memory":{"_musicGroups":[],"_members":[],"_supports":[]}
        }}
    });
    (data, account)
}

fn request(limit: Option<u64>) -> Value {
    json!({"format":"ournotes-deck.recommendation-request/2","goal":{"kind":"power"},
        "k":3,"constraints":{"leader":3},"limits":{"timeLimitMs":limit}})
}

fn successful(answer: Answer) -> AnswerResult {
    assert_eq!(answer.status, Status::Ok, "{:?}; {:?}", answer.errors, answer.missing);
    assert!(answer.is_final);
    answer.result.unwrap()
}

fn earlier(seconds: u64) -> Instant {
    Instant::now().checked_sub(Duration::from_secs(seconds)).expect("test clock can represent earlier request work")
}

fn assert_no_search(result: &AnswerResult) {
    assert!(!result.optimality.proven);
    assert!(result.teams.is_empty());
    assert_eq!(result.telemetry["environment"]["timeLimitMs"], 0);
    assert_eq!(result.telemetry["leaves"]["visited"], 0);
    assert_eq!(result.telemetry["leaves"]["evaluated"], 0);
}

#[test]
fn account_elapsed_preparation_exhausts_the_original_budget() {
    let (data, account) = fixture();
    let result = successful(recommend_started(
        &data,
        &account.to_string(),
        &request(Some(60_000)).to_string(),
        None,
        earlier(120),
    ));
    assert_no_search(&result);
    assert!(result.elapsed_ms >= 120_000.0);
}

#[test]
fn account_partial_preparation_keeps_only_the_remaining_budget() {
    let (data, account) = fixture();
    let result = successful(recommend_started(
        &data,
        &account.to_string(),
        &request(Some(120_000)).to_string(),
        None,
        earlier(60),
    ));
    let remaining = result.telemetry["environment"]["timeLimitMs"].as_u64().unwrap();
    assert!(remaining <= 60_000, "a new full budget was granted: {remaining}");
    assert!(result.optimality.proven, "tiny Power fixture should finish with its remaining budget");
    assert_eq!(result.teams.len(), 3);
    assert!(result.elapsed_ms >= 60_000.0);
}

#[test]
fn account_zero_budget_uses_the_same_validated_timeout_path() {
    let (data, account) = fixture();
    let result = successful(recommend(&data, &account.to_string(), &request(Some(0)).to_string(), None));
    assert_no_search(&result);
}

#[test]
fn account_unlimited_budget_does_not_expire_from_an_earlier_origin() {
    let (data, account) = fixture();
    let request = request(None).to_string();
    let account = account.to_string();
    let normal = successful(recommend(&data, &account, &request, None));
    let earlier = successful(recommend_started(&data, &account, &request, None, earlier(120)));
    assert!(normal.optimality.proven && earlier.optimality.proven);
    assert_eq!(serde_json::to_value(normal.teams).unwrap(), serde_json::to_value(earlier.teams).unwrap());
    assert!(earlier.telemetry["environment"]["timeLimitMs"].is_null());
}

#[test]
fn account_expired_budget_does_not_hide_parse_resolution_or_domain_errors() {
    let (data, account) = fixture();
    let request = request(Some(0));
    let origin = earlier(120);
    let invalid = recommend_started(&data, &account.to_string(), "{", None, origin);
    assert_eq!(invalid.status, Status::Invalid);
    assert!(invalid.errors.iter().any(|issue| issue.path == "request" && issue.code == "parse"));
    assert!(invalid.result.is_none());

    let mut missing = account.clone();
    missing["declared"] = json!({});
    let incomplete = recommend_started(&data, &missing.to_string(), &request.to_string(), None, origin);
    assert_eq!(incomplete.status, Status::Incomplete, "{:?}", incomplete.errors);
    assert!(!incomplete.missing.is_empty());
    assert!(incomplete.result.is_none());

    // This is checked by the built candidate domain, after account resolution has succeeded.
    let mut invalid = request;
    invalid["constraints"]["leader"] = json!(9999);
    let answer = recommend_started(&data, &account.to_string(), &invalid.to_string(), None, origin);
    assert_eq!(answer.status, Status::Invalid, "{:?}", answer.errors);
    assert!(answer.errors.iter().any(|issue| issue.code == "input" && issue.message.contains("9999")));
    assert!(answer.result.is_none());
}
