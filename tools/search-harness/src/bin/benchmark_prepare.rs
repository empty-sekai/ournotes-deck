//! Prepare benchmark play streams and rank confirmations through the native model.
//! This does not search, choose teams, modify master tables or claim a performance result.
use ournotes_sim::{
    data::DeckData,
    live::{
        full::gekisou_rank_factors,
        model::{JudgementStream, JustRule},
    },
    scenario::Scenario,
};
use serde::Deserialize;
use serde_json::json;
use std::{env, fs};

// JudgementStream transports NoteSimulateJudgement, whose Miss=1 (not score::MISS=6).
const SIMULATE_MISS: i32 = 1;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Spec {
    key: String,
    score_id: i64,
    scene: String,
    music_id: i64,
    #[serde(default)]
    miss_every: usize,
    #[serde(default)]
    seed: i32,
    #[serde(default)]
    include_stream: bool,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("benchmark_prepare DATA SPECS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let specs: Vec<Spec> = serde_json::from_slice(&fs::read(&args[1])?)?;
    let mut rows = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    for s in specs {
        if !keys.insert(s.key.clone()) {
            return Err("duplicate preparation key".into());
        }
        let scene = match s.scene.as_str() {
            "free" => Scenario::Free(s.music_id),
            "mission" => Scenario::Mission(s.music_id),
            "battle" => Scenario::Battle(s.music_id),
            "challenge" => Scenario::Challenge(s.music_id),
            "arena" => Scenario::Arena(s.music_id),
            _ => return Err("unknown scene".into()),
        };
        let resolved = scene.resolve(&data.master)?;
        let chart = data.chart(s.score_id)?;
        let dc = data.data_chart(s.score_id).ok_or("missing chart")?;
        let context = ournotes_sim::scenario::ResolvedContext::resolve(
            &data.master,
            scene,
            Some(s.score_id),
            &dc.fevers,
            Vec::new(),
        )?;
        let gekisou = matches!(s.scene.as_str(), "mission" | "battle" | "arena");
        let mut stream = if gekisou {
            JudgementStream::theoretical_best_gekisou(
                &chart,
                &dc.judgement_types,
                &JustRule::new(&data.master, &context.gekisou)?,
            )?
        } else {
            JudgementStream::theoretical_best(&chart)
        };
        stream.base_seed = s.seed;
        let mut misses = 0;
        if s.miss_every > 0 {
            for (i, row) in stream.judged.iter_mut().enumerate() {
                if (i + 1) % s.miss_every == 0 {
                    row[2] = SIMULATE_MISS;
                    misses += 1;
                }
            }
        }
        let confirmations = if matches!(s.scene.as_str(), "battle" | "arena") {
            if dc.fevers.len() > 3 {
                return Err("native network ranking has at most three ranges".into());
            }
            let factors = gekisou_rank_factors(&data.master, &resolved.gekisou_missions)?;
            Some(
                (0..dc.fevers.len())
                    .map(|range| ournotes_sim::replay::RankConfirmation {
                        frame: 0,
                        range,
                        rank: 1,
                        percent: factors[range][0],
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        rows.push(json!({"key":s.key, "scoreId":s.score_id, "scene":s.scene,
            "musicId":s.music_id, "judgedCount":stream.judged.len(), "frameCount":stream.frames.len(),
            "missCount":misses, "seed":s.seed,
            "stream":if s.include_stream || s.miss_every > 0 || s.seed != 0 { Some(stream) } else { None },
            "rankConfirmations":confirmations}));
    }
    fs::write(
        &args[2],
        serde_json::to_vec(&json!({"format":"ournotes.benchmark-preparation/1",
        "datasetId":data.sha256, "entries":rows}))?,
    )?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
