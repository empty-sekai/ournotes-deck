//! Public synthetic integration cases for the shared browser/native replay entry point.
use ournotes_sim::data::{DataChart, DeckData};
use ournotes_sim::live::skip::ChartNote;
use ournotes_sim::master::Master;
use ournotes_sim::replay::{RankConfirmation, ReplayMode, ReplayRawResult, ReplayRawRuntime, ReplaySession};
use serde_json::{Value, json};

fn data() -> DeckData {
    let mut rows: Value = serde_json::from_str(include_str!("fixtures/raw_bridge_master.json")).unwrap();
    rows["MasterLiveMusic"] =
        json!([{"_id":1,"_easyID":101,"_musicType":1,"_gekisouMission1":1,"_gekisouMission2":2,"_gekisouMission3":3}]);
    rows["MasterLiveMusicScore"] = json!([{"_id":101,"_musicScoreLevel":25,"_fullComboCount":3}]);
    let tables: Vec<_> =
        rows.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
    let master = Master::from_json_tables(|n| tables.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str())).unwrap();
    DeckData {
        provenance: json!({"replay":{"musicLengthsMs":{"101":2500}}}),
        sha256: None,
        master,
        charts: vec![DataChart {
            score_id: 101,
            asset_key: "synthetic".into(),
            asset_sha256: "0".repeat(64),
            notes: vec![
                ChartNote { id: 30, time_ms: 1000, note_type: 1 },
                ChartNote { id: 10, time_ms: 1000, note_type: 1 },
                ChartNote { id: 20, time_ms: 1020, note_type: 1 },
                ChartNote { id: 99, time_ms: 1100, note_type: 122 },
            ],
            judgement_types: vec![1; 4],
            skill_event_ms: vec![0],
            fevers: vec![],
        }],
    }
}

#[test]
fn template_preserves_enumeration_and_independent_audio_score_lengths() {
    let session = ReplaySession::new(data());
    let d = session.describe_chart(101).unwrap();
    assert_eq!(d.music_length_ms, Some(2500));
    assert_eq!(d.score_music_length_ms, 2100);
    assert_eq!(d.notes.iter().map(|n| n.note_id).collect::<Vec<_>>(), [30, 10, 20, 99]);
    assert_eq!(d.notes[3].default_judgement, 7);
    for fps in [30, 60, 120] {
        let r = session.template(101, 200000, fps).unwrap();
        assert_eq!(r.score_music_length_ms, Some(2100));
        assert_eq!(r.frames[1].delta_seconds.to_bits(), (1.0 / fps as f32).to_bits());
        let f = r.frames.iter().find(|f| f.judgements.iter().any(|j| j.note_id == 30)).unwrap();
        assert_eq!(f.judgements.iter().take(2).map(|j| j.note_id).collect::<Vec<_>>(), [30, 10]);
        assert!(r.frames.last().unwrap().time_ms >= 4500);
        assert_eq!(r.frames.iter().flat_map(|f| &f.judgements).count(), 4);
    }
    assert!(session.template(101, 200000, 59).is_err());
    let mut missing = data();
    missing.provenance = json!({});
    assert!(ReplaySession::new(missing).template(101, 200000, 60).is_err());
}

#[test]
fn complete_mixed_play_counts_actual_results_and_unscored_pass() {
    let session = ReplaySession::new(data());
    let mut r = session.template(101, 200000, 60).unwrap();
    r.trace = true;
    for j in r.frames.iter_mut().flat_map(|f| &mut f.judgements) {
        j.judgement = match j.note_id {
            30 => 4,
            10 => 2,
            20 => 6,
            _ => 7,
        };
    }
    let result = session.run(&r).unwrap();
    assert_eq!(
        (result.judgements.great, result.judgements.bad, result.judgements.just, result.judgements.pass),
        (1, 1, 1, 1)
    );
    assert_eq!(result.life, 900);
    assert!(result.score > 0);
    assert_eq!(result.frame_score, result.score);
    assert_eq!(result.frames.len(), r.frames.len());
    assert_eq!(result.frames.iter().flat_map(|f| f.converted_judgements.iter()).count(), 4);
    let again = session.run_json(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&again).unwrap()["score"], result.score);
}

#[test]
fn invalid_inputs_are_rejected_without_filling_missing_results() {
    let session = ReplaySession::new(data());
    let base = session.template(101, 200000, 60).unwrap();
    let mut r = base.clone();
    r.score_id = 999;
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.score_music_length_ms = None;
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.performers[0].live_skill = Some((999, 1));
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.skill_order[1] = 0;
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.frames[1].time_ms = 0;
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.frames[1].delta_seconds = f32::NAN;
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.frames.iter_mut().flat_map(|f| &mut f.judgements).next().unwrap().note_id = 999;
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    let j = r.frames.iter().flat_map(|f| &f.judgements).next().unwrap().clone();
    r.frames.last_mut().unwrap().judgements.push(j);
    assert!(session.run(&r).is_err());
    let mut r = base.clone();
    r.frames.iter_mut().find(|f| !f.judgements.is_empty()).unwrap().judgements.remove(0);
    assert!(session.run(&r).is_err());
    r.complete = false;
    assert!(session.run(&r).is_ok());
    let mut json = serde_json::to_value(&base).unwrap();
    json["typo"] = json!(1);
    assert!(session.run_json(&json.to_string()).is_err());
}

#[test]
fn raw_result_bridge_requires_metadata_and_runs_window_callbacks() {
    let session = ReplaySession::new(data());
    let mut r = session.template(101, 200000, 60).unwrap();
    r.performers[0].live_skill = Some((1, 1)); // 4000, one-result callback limit.
    assert!(session.run(&r).is_err()); // A judged-only stream cannot silently ignore its windows.
    r.raw_runtime = Some(ReplayRawRuntime {
        just_base_expansion_ms: 50,
        original_just_before_ms: 10,
        force_enable_just_judgement: false,
    });
    assert!(session.run(&r).is_err());
    for j in r.frames.iter_mut().flat_map(|f| &mut f.judgements) {
        j.raw_result = Some(ReplayRawResult {
            origin: j.judgement,
            timing: 2,
            origin_diff_ms: 0,
            diff_ms: 0,
            direction_mismatch: false,
            is_easy_flick: false,
        });
    }
    let result = session.run(&r).unwrap();
    assert_eq!(result.input_kind, "givenRawResultsBeforeSkillConversion");
    assert_eq!((result.judgements.perfect, result.judgements.pass), (3, 1));
    assert_eq!(result.life, 1000);
    assert!(result.score > 0);
}

#[test]
fn three_missions_settle_with_distinct_solo_and_external_contracts() {
    let mut d = data();
    d.master.live_musics[0].gekisou_mission_2 = 3;
    d.master.live_musics[0].gekisou_mission_3 = 2;
    for count in 1..=3 {
        for rank in 1..=5 {
            d.master.gekisou_ranking_score_bonuses.push(
                serde_json::from_value(json!({
                    "_id":count*10+rank,"_missionPattern":2,"_count":count,"_rank":rank,"_scoreBonusPercent":100-rank*10
                }))
                .unwrap(),
            );
        }
    }
    d.master.gekisou_luck_base_points.push(
        serde_json::from_value(
            json!({"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":150}),
        )
        .unwrap(),
    );
    for chance in 0..5 {
        d.master.gekisou_luck_bonus_lots.push(
            serde_json::from_value(json!({"_id":chance+1,"_chanceLotType":chance,"_lotResult":3,"_weight":1})).unwrap(),
        );
    }
    d.charts[0].notes[2].time_ms = 1200;
    d.charts[0].notes.insert(3, ChartNote { id: 40, time_ms: 1400, note_type: 1 });
    d.charts[0].judgement_types.push(1);
    d.charts[0].fevers = vec![(500, 1100), (1150, 1300), (1350, 1500)];
    let session = ReplaySession::new(d);
    let mut r = session.template(101, 200000, 60).unwrap();
    for j in r.frames.iter_mut().flat_map(|f| &mut f.judgements) {
        if j.note_id == 20 {
            j.judgement = 6;
        }
    }
    r.mode = ReplayMode::SoloGekisou;
    let solo = session.run(&r).unwrap();
    assert_eq!(solo.ranges.len(), 3);
    assert!(solo.ranges.iter().all(|r| r.state == 8 && r.rank_bonus.is_some()));
    assert!(solo.ranges[0].max_combo > 0);
    assert_eq!(solo.ranges[1].just_count, 1);
    assert!(solo.ranges[2].luck_points > 0);
    assert_eq!(solo.bonus_events.iter().map(|e| e.rank).collect::<Vec<_>>(), [1, 1, 1]);
    r.mode = ReplayMode::FixedSoloGekisou { ranks: [1, 1, 1] };
    let fixed = session.run(&r).unwrap();
    assert_eq!((fixed.score, fixed.frame_score), (solo.score, solo.frame_score));
    r.mode = ReplayMode::FixedSoloGekisou { ranks: [5, 3, 2] };
    let fixed = session.run(&r).unwrap();
    assert_eq!(fixed.bonus_events.iter().map(|e| e.rank).collect::<Vec<_>>(), [5, 3, 2]);
    let frame = r.frames.len() - 1;
    r.mode = ReplayMode::ExternalGekisou {
        confirmations: vec![
            RankConfirmation { frame, range: 0, rank: 5, percent: 50 },
            RankConfirmation { frame, range: 1, rank: 3, percent: 70 },
            RankConfirmation { frame, range: 2, rank: 2, percent: 80 },
        ],
    };
    let external = session.run(&r).unwrap();
    assert_eq!(external.bonus_events.iter().map(|e| e.rank).collect::<Vec<_>>(), [5, 3, 2]);
    assert!(matches!(external.mode, ReplayMode::ExternalGekisou { .. }));
    r.mode = ReplayMode::ExternalGekisou { confirmations: vec![] };
    assert!(session.run(&r).is_err()); // complete cannot hide unsettled external ranks.
}
