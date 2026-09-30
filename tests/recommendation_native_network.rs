//! Optional offline captured ARM64 regression, using the SAME evaluator as production.
//! NATIVE_LIVE_DATA / NATIVE_LIVE_CAPTURE point to authorized immutable local artifacts.
#[path = "recommend_fixture_export.rs"]
#[allow(dead_code)]
mod fixtures;
use ournotes_deck::{
    data::DeckData,
    live::{
        full::{
            GekisouSetup, JudgedNote, LiveNote, LiveParams, LivePlay, NetworkGekisouResult, Performer, PlayFrame,
            calculate_network_gekisou_ranking, gekisou_rank_factors,
        },
        model::JudgementStream,
    },
    replay::RankConfirmation,
    search::{
        Objective, PlayInput, Pool,
        expectation::{self, PhysicalDeck},
        recommendation::{SimulationInput, evaluate_declared_context},
    },
};
use serde_json::{Value, json};

#[test]
#[ignore = "requires authorized local original ARM64 capture/data artifacts"]
fn production_evaluator_matches_native_network_batch_score_trace() {
    let path = std::env::var("NATIVE_LIVE_DATA").unwrap();
    let capture_path = std::env::var("NATIVE_LIVE_CAPTURE").unwrap();
    let data = DeckData::from_path(&path).unwrap();
    let capture: Value = serde_json::from_slice(&std::fs::read(&capture_path).unwrap()).unwrap();
    // Bootstrap a legal context identity, then feed explicitly captured numerical inputs.
    // No claim that this is an OCR roster or whole roster-to-power validation is made.
    let (small, r) = fixtures::small_fixture();
    let pool = Pool::new(&small.master, &r).unwrap();
    let chart = small.chart(1004).unwrap();
    let objective = Objective::LiveScore {
        score_id: 1004,
        play: PlayInput::Stream {
            stream: JudgementStream::theoretical_best(&chart),
            judgement_types: small.data_chart(1004).unwrap().judgement_types.clone(),
        },
        chart,
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    };
    let physical = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
    let mut input = expectation::context(&pool, &physical, &objective).unwrap();
    let score_id = capture["chartId"].as_i64().unwrap();
    let dc = data.data_chart(score_id).unwrap();
    let chart = data.chart(score_id).unwrap();
    input.performers = std::array::from_fn(|_| Performer::default());
    input.notes = chart
        .notes
        .iter()
        .zip(&dc.judgement_types)
        .map(|(n, &jt)| LiveNote {
            note_id: n.id,
            time_ms: n.time_ms,
            note_operate_type: n.note_type,
            judgement_type: jt,
        })
        .collect();
    input.events = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
    input.params = LiveParams {
        skill_target_music_type: 1,
        total_power: capture["totalPower"].as_i64().unwrap() as i32,
        music_level: data.master.live_music_score(score_id).unwrap().music_score_level as i32,
        converted_note_count: chart.converted_note_count,
        music_length_ms: chart.last_timing_note_ms + 1000,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup {
        fevers: capture["rangeInfo"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["startMs"].as_i64().unwrap() as i32, r["endMs"].as_i64().unwrap() as i32))
            .collect(),
        missions: capture["rangeInfo"].as_array().unwrap().iter().map(|r| r["mission"].as_i64().unwrap()).collect(),
    };
    input.gekisou = Some(setup.clone());
    let judgements = capture["judgements"].as_array().unwrap();
    input.play = LivePlay {
        base_seed: 0,
        frames: capture["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| PlayFrame {
                time_ms: f["timeMs"].as_i64().unwrap() as i32,
                judged: judgements
                    .iter()
                    .filter(|j| j["frame"] == f["frame"])
                    .map(|j| JudgedNote {
                        note_id: j["noteId"].as_i64().unwrap() as i32,
                        judgement: j["origin"].as_i64().unwrap() as i32,
                        judgement_time_ms: j["timeMs"].as_i64().unwrap() as i32,
                    })
                    .collect(),
            })
            .collect(),
    };
    input.delta_times = vec![1.0 / 60.0; input.play.frames.len()];
    let factors = gekisou_rank_factors(&data.master, &setup.missions.clone().try_into().unwrap()).unwrap();
    let confirmations: Vec<_> = capture["networkMessages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let range = m["range"].as_u64().unwrap() as usize;
            let packets: Vec<_> = m["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    Some(NetworkGekisouResult {
                        range_index: r["range_index"].as_i64().unwrap() as i32,
                        combo: r["combo"].as_i64().unwrap() as i32,
                        luck_total_point: r["luck_total_point"].as_i64().unwrap() as i32,
                        just_count: r["just_count"].as_i64().unwrap() as i32,
                        perfect_count: r["perfect_count"].as_i64().unwrap() as i32,
                        score: r["score"].as_i64().unwrap() as i32,
                        has_no_input: r["has_no_input"].as_bool().unwrap(),
                    })
                })
                .collect();
            let ranking = calculate_network_gekisou_ranking(&setup.missions, 5, &packets).unwrap();
            let rank = ranking.player_rank(2);
            RankConfirmation {
                frame: m["frame"].as_u64().unwrap() as usize,
                range,
                rank,
                percent: factors[range][rank as usize - 1],
            }
        })
        .collect();
    let (terminal, applied) = evaluate_declared_context(
        &data.master,
        &physical,
        &input,
        capture["seed"].as_i64().unwrap() as i32,
        Some(&confirmations),
        &SimulationInput::default(),
    )
    .unwrap();
    let frames = capture["frames"].as_array().unwrap();
    assert_eq!(terminal.model.trace().len(), frames.len());
    let mut differences = Vec::new();
    for (i, ((t, s), f)) in terminal.model.trace().iter().zip(frames).enumerate() {
        if *t != f["timeMs"].as_i64().unwrap() as i32 || *s != f["settledScore"].as_i64().unwrap() as i32 {
            differences
                .push(json!({"frame":i,"time":t,"productionSettledScore":s,"nativeSettledScore":f["settledScore"]}));
        }
    }
    let native_applied: Vec<_> = frames
        .iter()
        .filter(|f| f["rankConfirmed"].as_i64().unwrap() != 0)
        .enumerate()
        .map(|(range, f)| (range, f["frame"].as_u64().unwrap() as usize))
        .collect();
    assert_eq!(applied, native_applied);
    assert_eq!(terminal.final_score, frames.last().unwrap()["settledScore"].as_i64().unwrap() as i32);
    let report = json!({"scope":"production recommendation numerical evaluator against independently captured ARM64 component trace; explicit no-skill performers/power/packets, not end-to-end OCR or server authority","data":path,"capture":capture_path,"chartId":score_id,"frames":frames.len(),"comparedFields":"per-frame time and settledScore; actual confirmation frame sequence; terminal score","differences":differences,"applied":applied,"finalScore":terminal.final_score,"nativeFinalScore":frames.last().unwrap()["settledScore"],"nativeCaptureDrawsExcludeMemberInitialization":true,"productionMemberShuffleDraws":4});
    if let Ok(out) = std::env::var("NATIVE_PRODUCTION_REPORT") {
        std::fs::write(out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    assert!(
        differences.is_empty(),
        "production settled score differs from original ARM64 capture: {}",
        differences.len()
    );
}
