use super::{data, data_with_tables};
use ournotes_sim::data::DeckData;
use ournotes_sim::replay::{
    PowerDomain, RankAnalysisRequest, RankAnalysisStatus, RankTarget, ReplayMode, ReplayRawRuntime, ReplayRequest,
    ReplaySession, RequiredPower,
};
use serde_json::{Value, json};

fn score_tables(rows: &mut Value) {
    rows["MasterSkillEffectSetting"] = json!([{"_id":1,"_skillEffectType":2000,"_phase":2}]);
    rows["MasterLiveSkillEffect"] = json!(
        (0..5)
            .map(|index| json!({
                "_id":10+index,"_liveSkillID":10+index,"_level":1,"_activationTimeSecond":5.0,
                "_skillEffectType":2000,"_effectValue":10000,"_skillTargetIDs":[]
            }))
            .collect::<Vec<_>>()
    );
    rows["MasterSkillCondition"] = json!([
        {"_id":1,"_conditionType":4010,"_conditionValues":[],"_isPositive":true},
        {"_id":2,"_conditionType":4011,"_conditionValues":[50],"_isPositive":true}
    ]);
    rows["MasterSkillConditionSet"] = json!([
        {"_id":1,"_group":1,"_conditionIds":[1]}, {"_id":2,"_group":2,"_conditionIds":[2]}
    ]);
    rows["MasterSupportSkillEffect"] = json!([
        {"_id":90,"_supportSkillID":90,"_level":1,"_skillTriggerType":1,"_activationTimeSecond":5.0,"_skillTriggerConditionGroup":1,
            "_skillEffectType":2000,"_effectValue":10000,"_skillTargetIDs":[]},
        {"_id":91,"_supportSkillID":91,"_level":1,"_skillTriggerType":1,"_activationTimeSecond":5.0,"_skillTriggerConditionGroup":1,
            "_skillConditionGroup":2,"_skillEffectType":2000,"_effectValue":10000,"_skillTargetIDs":[]}
    ]);
}

fn scored_data() -> DeckData {
    data_with_tables(score_tables)
}

fn scored_session() -> ReplaySession {
    ReplaySession::new(scored_data())
}

fn replay(session: &ReplaySession, power: i32) -> ReplayRequest {
    let mut replay = session.template(101, power, 30).unwrap();
    for (index, performer) in replay.performers.iter_mut().enumerate() {
        performer.live_skill = Some((10 + index as i64, 1));
        performer.character_id = index as i64 + 1;
    }
    replay.performers[0].support_skills = vec![(90, 1)];
    replay
}

fn request(replay: ReplayRequest, threshold: i32, domain: PowerDomain) -> RankAnalysisRequest {
    RankAnalysisRequest {
        format: "ournotes.replay-rank/1".into(),
        replay,
        target: RankTarget::Score { threshold },
        power_domain: domain,
    }
}

fn permutations() -> Vec<Vec<usize>> {
    fn visit(prefix: &mut Vec<usize>, result: &mut Vec<Vec<usize>>) {
        if prefix.len() == 5 {
            result.push(prefix.clone());
            return;
        }
        for member in 0..5 {
            if !prefix.contains(&member) {
                prefix.push(member);
                visit(prefix, result);
                prefix.pop();
            }
        }
    }
    let mut result = Vec::new();
    visit(&mut Vec::new(), &mut result);
    result
}

fn direct_scores(session: &ReplaySession, replay: &ReplayRequest, power: i32) -> Vec<i32> {
    let mut replay = replay.clone();
    replay.power = power;
    permutations()
        .into_iter()
        .map(|order| {
            replay.skill_order = order;
            session.run(&replay).unwrap().score
        })
        .collect()
}

#[test]
fn complete_order_distribution_preserves_physical_snap_pairing_and_equal_skill_mass() {
    let session = scored_session();
    for power in [1, 127, 200_000, 1_090_877] {
        let replay = replay(&session, power);
        let scores = direct_scores(&session, &replay, power);
        let threshold = *scores.iter().max().unwrap();
        if power >= 200_000 {
            assert!(scores.iter().any(|&score| score < threshold));
        }
        let mut job = session.start_rank_analysis(request(replay.clone(), threshold, PowerDomain::default())).unwrap();
        assert_eq!(job.status().completed_orders, 0);
        for completed in 1..120 {
            let progress = job.advance(1).unwrap();
            assert_eq!(progress.completed_orders, completed);
            assert_eq!(progress.status, RankAnalysisStatus::Running);
            assert!(progress.result.is_none());
        }
        let progress = job.advance(1).unwrap();
        assert_eq!(progress.status, RankAnalysisStatus::Complete);
        let result = progress.result.as_ref().unwrap();
        assert_eq!(result.order_scores, scores);
        assert_eq!(result.score_sum, scores.iter().map(|&score| i64::from(score)).sum::<i64>());
        assert_eq!(result.order_count, 120);
        assert_eq!(result.target_hit_count, scores.iter().filter(|&&score| score >= threshold).count());
        if power >= 200_000 {
            assert_eq!(result.target_hit_count, 24);
        }
        let finished = job.status_json().unwrap();
        assert_eq!(job.advance_json(120).unwrap(), finished);
        assert_eq!(replay.performers[0].support_skills, [(90, 1)]);
    }
}

#[test]
fn required_power_matches_exhaustive_exact_mean_search_and_reports_its_boundary() {
    let session = scored_session();
    let replay = replay(&session, 24);
    let domain = PowerDomain { min: 1, max: 48 };
    let sums: Vec<i64> = (domain.min..=domain.max)
        .map(|power| direct_scores(&session, &replay, power).into_iter().map(i64::from).sum())
        .collect();
    let threshold = ((sums[23] + 119) / 120) as i32;
    let expected = sums.iter().position(|&sum| sum >= i64::from(threshold) * 120).unwrap();
    let mut job = session.start_rank_analysis(request(replay, threshold, domain)).unwrap();
    let result = job.advance(120).unwrap().result.as_ref().unwrap();
    match result.need {
        RequiredPower::Exact { power, score_sum, previous_score_sum } => {
            assert_eq!(power, domain.min + expected as i32);
            assert_eq!(score_sum, sums[expected]);
            assert_eq!(previous_score_sum, expected.checked_sub(1).map(|index| sums[index]));
            assert!(score_sum >= i64::from(threshold) * 120);
            assert!(previous_score_sum.is_none_or(|value| value < i64::from(threshold) * 120));
        }
        ref other => panic!("expected certified boundary, got {other:?}"),
    }
}

#[test]
fn zero_threshold_and_unreachable_threshold_are_distinct_complete_results() {
    let session = scored_session();
    for (threshold, hits) in [(0, 120), (i32::MAX, 0)] {
        let mut job = session
            .start_rank_analysis(request(replay(&session, 200), threshold, PowerDomain { min: 1, max: 200 }))
            .unwrap();
        let result = job.advance(120).unwrap().result.as_ref().unwrap();
        assert_eq!(result.target_hit_count, hits);
        if threshold == 0 {
            assert!(matches!(result.need, RequiredPower::Exact { power: 1, previous_score_sum: None, .. }));
        } else {
            assert!(matches!(result.need, RequiredPower::OutsideDomain));
        }
    }
}

#[test]
fn an_unproved_power_domain_keeps_current_power_order_statistics() {
    let mut data = data_with_tables(|rows| {
        rows["MasterLiveSettings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["_key"] == "note_score_adjustment_factor")
            .unwrap()["_value"] = json!("1000000");
    });
    data.charts[0].skill_event_ms.clear();
    let session = ReplaySession::new(data);
    let replay = session.template(101, 1, 30).unwrap();
    let expected = session.run(&replay).unwrap().score;
    let mut job = session.start_rank_analysis(request(replay, expected.max(0), PowerDomain::default())).unwrap();
    let result = job.advance(120).unwrap().result.as_ref().unwrap();
    assert!(matches!(result.need, RequiredPower::Unproven { .. }));
    assert_eq!(result.order_scores, vec![expected; 120]);
    assert_eq!(result.target_hit_count, usize::from(expected >= 0) * 120);
}

#[test]
fn free_live_ignores_static_luck_missions_but_random_skills_remain_unsupported() {
    let session = scored_session();
    assert!(session.describe_chart(101).unwrap().missions.contains(&2));
    let mut valid = session.start_rank_analysis(request(replay(&session, 100), 0, PowerDomain::default())).unwrap();
    assert_eq!(valid.advance(120).unwrap().status, RankAnalysisStatus::Complete);
    let mut random = replay(&session, 100);
    random.performers[0].support_skills = vec![(91, 1)];
    let mut job = session.start_rank_analysis(request(random, 0, PowerDomain::default())).unwrap();
    let progress = job.advance(120).unwrap();
    assert_eq!(progress.status, RankAnalysisStatus::Unsupported);
    assert_eq!(progress.completed_orders, 0);
    assert!(progress.result.is_none());
    assert_eq!(progress.code, Some("unsupported-domain"));
}

#[test]
fn active_luck_ranges_are_unsupported_before_any_partial_statistics_are_published() {
    let mut data = data();
    data.master.live_musics[0].gekisou_mission_1 = 2;
    data.charts[0].fevers = vec![(500, 1500)];
    let session = ReplaySession::new(data);
    let mut replay = session.template(101, 100, 30).unwrap();
    replay.mode = ReplayMode::SoloGekisou;
    let mut job = session.start_rank_analysis(request(replay, 0, PowerDomain::default())).unwrap();
    let progress = job.advance(120).unwrap();
    assert_eq!(progress.status, RankAnalysisStatus::Unsupported);
    assert_eq!(progress.completed_orders, 0);
    assert!(progress.reason.as_ref().unwrap().contains("LUCK"));
    assert!(progress.result.is_none());
}

#[test]
fn fixed_solo_rank_settlement_matches_all_original_replay_orders() {
    let mut data = scored_data();
    data.master.live_musics[0].gekisou_mission_2 = 3;
    data.master.live_musics[0].gekisou_mission_3 = 1;
    data.charts[0].fevers = vec![(500, 1010), (1015, 1050), (1060, 1150)];
    for count in 1..=3 {
        for rank in 1..=5 {
            data.master.gekisou_ranking_score_bonuses.push(
                serde_json::from_value(json!({
                    "_id":count*10+rank,"_missionPattern":2,"_count":count,"_rank":rank,"_scoreBonusPercent":100-rank*10
                }))
                .unwrap(),
            );
        }
    }
    let session = ReplaySession::new(data);
    for ranks in [[1, 1, 1], [5, 3, 2]] {
        for power in [127, 200_000] {
            let mut replay = replay(&session, power);
            replay.mode = ReplayMode::FixedSoloGekisou { ranks };
            let scores = direct_scores(&session, &replay, power);
            let threshold = *scores.iter().max().unwrap();
            let mut job = session.start_rank_analysis(request(replay, threshold, PowerDomain::default())).unwrap();
            let progress = job.advance(120).unwrap();
            assert_eq!(progress.status, RankAnalysisStatus::Complete, "{:?}", progress.reason);
            let result = progress.result.as_ref().unwrap();
            assert_eq!(result.order_scores, scores);
            assert_eq!(result.target_hit_count, scores.iter().filter(|&&score| score >= threshold).count());
        }
    }
}

#[test]
fn raw_runtime_has_an_explicit_terminal_status_and_no_order_statistics() {
    let session = scored_session();
    let mut replay = replay(&session, 100);
    replay.raw_runtime = Some(ReplayRawRuntime {
        just_base_expansion_ms: 50,
        original_just_before_ms: 10,
        force_enable_just_judgement: false,
    });
    let mut job = session.start_rank_analysis(request(replay, 0, PowerDomain::default())).unwrap();
    assert_eq!(job.status().status, RankAnalysisStatus::Unsupported);
    assert_eq!(job.status().code, Some("raw-runtime"));
    assert!(job.status().result.is_none());
    let status = job.status_json().unwrap();
    assert_eq!(job.advance_json(1).unwrap(), status);
}

#[test]
#[ignore = "exports synthetic native references for the WASM transport test"]
fn export_rank_analysis_corpus() {
    let mut rows = Value::Null;
    let data = data_with_tables(|tables| {
        score_tables(tables);
        rows = tables.clone();
    });
    let master: serde_json::Map<String, Value> = ournotes_sim::master::TABLES
        .iter()
        .map(|&name| {
            let objects = rows.get(name).and_then(Value::as_array).cloned().unwrap_or_default();
            let columns: std::collections::BTreeSet<_> =
                objects.iter().flat_map(|row| row.as_object().unwrap().keys().cloned()).collect();
            let values: Vec<Vec<Value>> = objects
                .iter()
                .map(|row| columns.iter().map(|key| row.get(key).cloned().unwrap_or(Value::Null)).collect())
                .collect();
            (name.into(), json!({"columns": columns, "rows": values}))
        })
        .collect();
    let charts: Vec<_> = data
        .charts
        .iter()
        .map(|chart| {
            json!({
                "scoreId": chart.score_id,
                "asset": {"key": chart.asset_key, "sha256": chart.asset_sha256},
                "notes": {
                    "id": chart.notes.iter().map(|note| note.id).collect::<Vec<_>>(),
                    "op": chart.notes.iter().map(|note| note.note_type).collect::<Vec<_>>(),
                    "timeMs": chart.notes.iter().map(|note| note.time_ms).collect::<Vec<_>>(),
                    "judgementType": chart.judgement_types,
                },
                "skillEvents": {"timeMs": chart.skill_event_ms},
                "fevers": {
                    "startMs": chart.fevers.iter().map(|range| range.0).collect::<Vec<_>>(),
                    "endMs": chart.fevers.iter().map(|range| range.1).collect::<Vec<_>>(),
                },
            })
        })
        .collect();
    let wire =
        json!({"format": "nnnotes.deck-data/1", "provenance": data.provenance, "master": master, "charts": charts});
    let session = ReplaySession::from_json(&wire.to_string()).unwrap();
    let mut requests = Vec::new();
    for power in [1, 127, 200_000, 1_090_877] {
        let replay = replay(&session, power);
        let threshold = *direct_scores(&session, &replay, power).iter().max().unwrap();
        requests.push(request(replay.clone(), threshold, PowerDomain::default()));
        requests.push(request(replay, 0, PowerDomain { min: 1, max: 48 }));
    }
    requests.push(request(replay(&session, 100), i32::MAX, PowerDomain { min: 1, max: 48 }));
    let mut random = replay(&session, 100);
    random.performers[0].support_skills = vec![(91, 1)];
    requests.push(request(random, 0, PowerDomain::default()));
    let cases: Vec<_> = requests
        .into_iter()
        .map(|request| {
            let mut job = session.start_rank_analysis(request.clone()).unwrap();
            let expected = job.advance(120).unwrap();
            json!({"request": request, "expected": expected})
        })
        .collect();
    let output =
        std::env::var("OURNOTES_REPLAY_RANK_CORPUS").expect("set OURNOTES_REPLAY_RANK_CORPUS to an output file");
    std::fs::write(output, serde_json::to_vec(&json!({"data": wire, "cases": cases})).unwrap()).unwrap();
}

#[test]
fn rank_analysis_validates_inputs_and_step_sizes() {
    let session = scored_session();
    let base = request(replay(&session, 100), 0, PowerDomain::default());
    for domain in
        [PowerDomain { min: 0, max: 1 }, PowerDomain { min: 3, max: 2 }, PowerDomain { min: 1, max: 20_000_001 }]
    {
        assert!(session.start_rank_analysis(RankAnalysisRequest { power_domain: domain, ..base.clone() }).is_err());
    }
    let mut bad = base.clone();
    bad.replay.complete = false;
    assert!(session.start_rank_analysis(bad).is_err());
    let mut bad = base.clone();
    bad.replay.performers.pop();
    assert!(session.start_rank_analysis(bad).is_err());
    let mut bad = base.clone();
    bad.target = RankTarget::Score { threshold: -1 };
    assert!(session.start_rank_analysis(bad).is_err());
    let mut job = session.start_rank_analysis_json(&serde_json::to_string(&base).unwrap()).unwrap();
    assert!(job.advance(0).is_err());
    assert!(job.advance(121).is_err());
    assert_eq!(job.status().completed_orders, 0);
    let status: Value = serde_json::from_str(&job.advance_json(7).unwrap()).unwrap();
    assert_eq!(status["completedOrders"], 7);
    assert!(status["result"].is_null());
}
