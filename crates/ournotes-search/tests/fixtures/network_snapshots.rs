//! Synthetic network snapshots taken before a timed score factor is backfilled.

use super::common::{replace_table, set_column};
use super::{data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{auxiliary, engine, search::Completion, types::Strategy};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

#[test]
fn network_snapshot_caps_cover_timed_factor_end_frames() {
    for (duration, snapshot_end) in [(0.24, 400), (0.23, 380)] {
        for (effect, targets) in [(2000, vec![]), (2004, vec![5])] {
            let mut synth = synthetic_master(6, 0, 6);
            set_column(&mut synth, "MasterMemberCard", &mut |row| {
                row["_liveSkillID"] = json!(if row["_id"] == 1 { 1 } else { 2 });
            });
            set_column(&mut synth, "MasterLiveMusic", &mut |row| {
                row["_gekisouMission1"] = json!(3);
                row["_gekisouMission2"] = json!(3);
                row["_gekisouMission3"] = json!(3);
            });
            set_column(&mut synth, "MasterLiveGekisouRankingScoreBonus", &mut |row| {
                row["_scoreBonusPercent"] = json!(250);
            });
            replace_table(&mut synth, "MasterGekisouSkillEffect", json!([]));
            replace_table(
                &mut synth,
                "MasterLiveSkillEffect",
                json!([{"_id":1,"_liveSkillID":1,"_level":4,"_skillConditionGroup":0,
                    "_skillTargetIDs":targets,"_skillEffectType":effect,
                    "_activationTimeSecond":duration,"_effectValue":10000}]),
            );
            let times = [100, 200, 300, 400, 420, 440, 800, 1200];
            set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(times.len()));
            let mut document = data_document(&synth, 6, 0, 6);
            document["charts"][0]["notes"] = json!({"id":(1..=times.len()).collect::<Vec<_>>(),
                "op":vec![1;times.len()],"judgementType":vec![1;times.len()],"timeMs":times});
            document["charts"][0]["skillEvents"]["timeMs"] = json!([160, 500, 750, 1000, 1250]);
            document["charts"][0]["fevers"] = json!({"startMs":[100,700,1100],
                "endMs":[snapshot_end,800,1200]});
            let data = DeckData::from_json(&document.to_string()).unwrap();
            let roster = Roster::from_json(&roster_document(6, 0, 6).to_string()).unwrap();
            let mut wire = joint_request_json("battle", true, json!({"kind":"score"}));
            let frames: Vec<_> = (0..=2400).step_by(20).collect();
            wire["execution"]["play"] = json!({"kind":"stream","stream":{
                "frames":frames,"judged":times.iter().enumerate().map(|(i,&time)| {
                    let judged = if time == 400 { snapshot_end } else { time };
                    json!([judged / 20, i + 1, 5, time])
                }).collect::<Vec<_>>()}});
            wire["networkConfirmations"] =
                json!((0..3).map(|range| json!({"frame":0,"range":range,"rank":1,"percent":250})).collect::<Vec<_>>());
            wire["k"] = json!(3);
            let mut request = serde_json::from_value::<ournotes_search::types::RecommendationRequest>(wire).unwrap();
            let fixed = auxiliary::evaluate_fixed(&data, &roster, &request, [1, 2, 3, 4, 5], [None; 5]).unwrap();
            assert_eq!(fixed.completion, Completion::Complete);
            assert_eq!(fixed.results[0].order_outcomes.len(), 120);
            #[cfg(feature = "search-diagnostics")]
            {
                use ournotes_search::{handler, search::diagnostics};
                let built = handler::build_card_pool(&data, &roster, &request).unwrap();
                let audit = diagnostics::audit_order_caps(&built, [1, 2, 3, 4, 5], [None; 5]).unwrap();
                assert_eq!(audit["orders"], 120);
                assert_eq!(audit["violations"], 0, "duration={duration} effect={effect}: {audit}");
            }
            let bounded = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(bounded.completion, Completion::Complete);
            assert!(bounded.telemetry.environment.bounds.compiled);
            request.strategy = Strategy::Exhaustive;
            let oracle = engine::recommend(&data, &roster, &request).unwrap();
            assert_eq!(oracle.completion, Completion::Complete);
            assert_eq!(bounded.results, oracle.results, "duration={duration} effect={effect}");
        }
    }
}
