//! Synthetic conversion partitions and their exact full-domain ranking.

use super::common::{extend_table, replace_table, set_column};
use super::{data_document, joint_request_json, roster_document, synthetic_master};
use ournotes_search::{engine, search::Completion, types::Strategy};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::json;

#[test]
fn compiled_conversion_reach_preserves_every_snap_assignment() {
    for (judgement, targets, converters) in [(5, [4, 4], 0), (4, [4, 3], 1), (4, [4, 4], 2)] {
        let mut synth = synthetic_master(5, 2, 5);
        replace_table(&mut synth, "MasterLiveSkillEffect", json!([]));
        set_column(&mut synth, "MasterLiveMusic", &mut |row| {
            for mission in ["_gekisouMission1", "_gekisouMission2", "_gekisouMission3"] {
                row[mission] = json!(1);
            }
        });
        set_column(&mut synth, "MasterSupportCard", &mut |row| {
            row["_supportSkillId01"] = row["_id"].clone();
            row["_supportSkillId02"] = json!(0);
            row["_gekisouSupportSkillId01"] = json!(1);
            row["_gekisouSupportSkillId02"] = json!(0);
        });
        set_column(&mut synth, "MasterSupportCardRank", &mut |row| row["_gekisouSupportSkill01Level"] = json!(1));
        extend_table(
            &mut synth,
            "MasterSkillTarget",
            targets
                .into_iter()
                .enumerate()
                .map(|(i, judgement)| json!({"_id":9101+i,"_skillTargetType":4,"_judgement":judgement}))
                .collect(),
        );
        replace_table(
            &mut synth,
            "MasterSupportSkillEffect",
            json!(
                (1..=2)
                    .map(|id| json!({"_id":id,"_supportSkillID":id,"_level":3,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":53,"_skillTargetIDs":[9100+id],
                "_skillEffectType":12006,"_activationTimeSecond":5.0,"_effectValue":5}))
                    .collect::<Vec<_>>()
            ),
        );
        // The declared ranges use mission 1. These mission-3 rows stay present in every owned Snap.
        replace_table(&mut synth, "MasterGekisouSupportSkill", json!([{"_id":1,"_gekisouMissionType":3}]));
        replace_table(
            &mut synth,
            "MasterGekisouSupportSkillEffect",
            json!([{"_id":9001,"_gekisouSupportSkillID":1,"_level":1,"_skillTriggerType":1,
                "_skillTriggerConditionGroup":53,"_skillTargetIDs":[12],"_skillEffectType":12006,
                "_activationTimeSecond":5.0,"_effectValue":6}]),
        );
        let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
        let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
        let mut wire = joint_request_json("mission", true, json!({"kind":"score"}));
        wire["k"] = json!(31);
        wire["execution"]["play"] = json!({"kind":"stream","stream":{
            "frames":(0..=2400).step_by(20).collect::<Vec<_>>(),
            "judged":(1..=12).map(|i| json!([i*5,i,judgement,i*100])).collect::<Vec<_>>()}});
        let mut request: ournotes_search::types::RecommendationRequest = serde_json::from_value(wire).unwrap();
        let bounded = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(bounded.completion, Completion::Complete);
        assert!(bounded.telemetry.environment.bounds.compiled, "{:?}", bounded.telemetry.environment.bounds.fallback);
        assert_eq!(bounded.results.len(), 31, "five slots with two distinct optional Snaps");
        assert_eq!(bounded.telemetry.proof.parts, [1, 6, 21][converters]);
        assert_eq!(bounded.telemetry.proof.parts_done, bounded.telemetry.proof.parts);
        if converters > 0 {
            let conversion = bounded.telemetry.environment.bounds.conversion.as_ref().unwrap();
            assert_eq!(conversion.snaps, converters);
            assert_eq!(conversion.prepared_domains, [0, 2, 4][converters]);
            let mut limited = request.clone();
            limited.limits.max_candidates = Some(1);
            let stopped = engine::recommend(&data, &roster, &limited).unwrap();
            assert_eq!(stopped.completion, Completion::TimedOut);
            assert_eq!(stopped.telemetry.environment.bounds.conversion.as_ref().unwrap().prepared_domains, 0);
            let upper: i128 = stopped.telemetry.proof.upper_bound.as_ref().unwrap().parse().unwrap();
            let best: i128 = bounded.results[0].expected_payoff.as_ref().unwrap().numerator.parse().unwrap();
            assert!(upper >= best, "the prepared whole-domain envelope covers every conversion part");
        }

        #[cfg(feature = "search-diagnostics")]
        let built = ournotes_search::handler::build_card_pool(&data, &roster, &request).unwrap();
        request.strategy = Strategy::Exhaustive;
        let exhaustive = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(exhaustive.completion, Completion::Complete);
        assert_eq!(bounded.results, exhaustive.results);

        #[cfg(feature = "search-diagnostics")]
        {
            use ournotes_search::search::{ablate, diagnostics, set_bound_ablation};
            if converters == 2 {
                let mut limited = request.clone();
                limited.strategy = Strategy::BranchAndBound;
                limited.limits.max_candidates = Some(2);
                set_bound_ablation(ablate::NO_WARM_START | ablate::STATIC_ORDER);
                let stopped = engine::recommend(&data, &roster, &limited);
                set_bound_ablation(0);
                let stopped = stopped.unwrap();
                assert_eq!(stopped.completion, Completion::TimedOut);
                let prepared = stopped.telemetry.environment.bounds.conversion.as_ref().unwrap().prepared_domains;
                assert!(prepared > 0 && prepared < 4);
                assert!(stopped.telemetry.proof.parts_done > 0);
                assert!(stopped.telemetry.proof.parts_done < stopped.telemetry.proof.parts);
                let upper: i128 = stopped.telemetry.proof.global_upper_bound.as_ref().unwrap().parse().unwrap();
                let optimum: i128 = exhaustive.results[0].expected_payoff.as_ref().unwrap().numerator.parse().unwrap();
                assert!(upper >= optimum, "the whole-domain envelope covers every pending conversion group");
            }
            let mut scratch = diagnostics::PrefixAuditScratch::default();
            for deck in &exhaustive.results {
                let actual: i128 = deck.expected_payoff.as_ref().unwrap().numerator.parse().unwrap();
                for depth in 1..=5 {
                    let (cap, power) = diagnostics::prefix_upper(&built, deck.members, deck.snaps, depth, &mut scratch)
                        .unwrap()
                        .expect("compiled joint envelope");
                    assert!(cap >= actual, "prefix depth {depth}: {cap} < {actual}");
                    assert!(power >= i64::from(deck.power));
                }
                let order_caps = diagnostics::audit_order_caps(&built, deck.members, deck.snaps).unwrap();
                assert_eq!(order_caps["orders"], 120);
                assert_eq!(order_caps["violations"], 0, "{order_caps}");
            }
            assert!(scratch.checked_resource_prefixes > 0);
            assert!(scratch.checked_character_prefixes > 0);
        }
    }
}
