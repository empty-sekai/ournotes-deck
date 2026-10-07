//! Native payoff requests retain an explicit refinement outcome when score support is unresolved.
use super::common::{replace_table, set_column};
use super::{EVENT_ID, data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Metric, Optimality, RecommendationRequest},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

#[test]
fn unresolved_score_domain_stops_before_native_room_payoff_partition() {
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| row["_liveSkillID"] = json!(1));
    set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_gekisouMission1"] = json!(2));
    set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(64));
    set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |row| row["_scoreBonusPercent"] = json!(250));
    set_column(&mut synth, "MasterLiveSettings", &mut |row| {
        if matches!(row["_key"].as_str(), Some("gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush")) {
            row["_value"] = json!("1");
        }
    });
    replace_table(&mut synth, "MasterGekisouSkillEffect", json!([]));
    replace_table(&mut synth, "MasterGekisouSupportSkillEffect", json!([]));
    replace_table(
        &mut synth,
        "MasterLiveSkillEffect",
        json!([{
            "_id":1,"_liveSkillID":1,"_level":4,"_skillEffectType":3000,"_effectValue":100,
            "_activationTimeSecond":0.5
        }]),
    );
    replace_table(
        &mut synth,
        "MasterLiveGekisouLuckBasePoint",
        json!([{
            "_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":1
        }]),
    );
    replace_table(
        &mut synth,
        "MasterLiveGekisouLuckBonusLot",
        json!(
            (0..5)
                .flat_map(|kind| [0, 3].into_iter().enumerate().map(move |(index, result)| json!({
                    "_id":kind*2+index+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1
                })))
                .collect::<Vec<_>>()
        ),
    );
    let mut document = data_document(&synth, 5, 0, 5);
    document["charts"][0]["notes"] = json!({"id":(1..=64).collect::<Vec<_>>(),
        "op":vec![1;64],"judgementType":vec![1;64],"timeMs":(300..364).collect::<Vec<_>>()});
    document["charts"][0]["skillEvents"]["timeMs"] = json!([0, 100, 200, 300, 400]);
    document["charts"][0]["fevers"] = json!({"startMs":[100],"endMs":[800]});
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    for metric in [
        Metric::ClientEventPoints { event_id: EVENT_ID },
        Metric::ClientChallengePoints { event_id: EVENT_ID },
        Metric::ConditionalClientEventItems { event_id: EVENT_ID, resource_type: 1, resource_id: 1 },
    ] {
        for cache in [0, 64] {
            let mut wire = joint_request_json("battle", true, serde_json::to_value(metric.clone()).unwrap());
            wire["k"] = json!(1);
            wire["strategy"] = json!({"kind":"exhaustive"});
            wire["limits"]["cacheEntries"] = json!(cache);
            wire["context"]["eventPayoff"]["multiplayerScorePolicy"] = json!({"kind":"sameScore","players":3});
            wire["networkConfirmations"] = json!([{"frame":0,"range":0,"rank":1,"percent":250}]);
            wire["execution"]["play"] = json!({"kind":"stream","stream":{
                "frames":(0..=20).map(|i|i*100).collect::<Vec<_>>(),"deltaTimes":vec![0.1;21],
                "judged":(1..=64).map(|id|[4,id,5,299+id]).collect::<Vec<_>>()
            }});
            let request: RecommendationRequest = serde_json::from_value(wire).unwrap();
            let result = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(result.completion, Completion::RefinementRequired);
            assert_eq!(result.optimality, Optimality::Unproven);
            assert_eq!(result.telemetry.leaves.partial, 1);
            assert_eq!(result.telemetry.leaves.evaluated, 0);
            assert!(result.results.is_empty());
        }
    }
}
