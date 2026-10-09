//! One native benchmark request, with an explicit post-dataset-load timing boundary.
use ournotes_search::engine::{Answer, AnswerProgress, Progress, recommend_account, recommend_snapshot};
use ournotes_sim::data::DeckData;
use std::{
    env, fs,
    io::{self, Write},
    time::{Duration, Instant},
};

fn emit(value: serde_json::Value) -> io::Result<()> {
    println!("{value}");
    io::stdout().flush()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("benchmark_case DATA SNAPSHOT REQUEST OUTPUT".into());
    }
    // Diagnostics: OURNOTES_CENSUS=<payoff numerator> counts the teams the caps leave open at that threshold instead
    // of simulating them (no warm start); OURNOTES_ABLATE=<bits> sets the validation switches.
    let census = env::var("OURNOTES_CENSUS").ok().map(|v| v.parse::<i128>()).transpose()?;
    let mut ablation = env::var("OURNOTES_ABLATE").ok().map(|v| v.parse::<u32>()).transpose()?.unwrap_or(0);
    if census.is_some() {
        ablation |= ournotes_search::search::ablate::NO_WARM_START;
    }
    ournotes_search::search::set_census(census);
    ournotes_search::search::set_bound_ablation(ablation);
    let setup = Instant::now();
    let data = DeckData::from_path(&args[0])?;
    let snapshot = fs::read_to_string(&args[1])?;
    let request = fs::read_to_string(&args[2])?;
    emit(serde_json::json!({"type":"ready","setupMs":setup.elapsed().as_secs_f64()*1000.,
        "datasetId":data.sha256,"arch":std::env::consts::ARCH}))?;
    let started = Instant::now();
    let interval_ms = env::var("OURNOTES_PROGRESS_MS").ok().map(|v| v.parse::<u64>()).transpose()?.unwrap_or(250);
    let interval = Duration::from_millis(interval_ms);
    let mut report = |r: &ournotes_search::types::RecommendationOutcome| {
        let _ = emit(serde_json::json!({"type":"progress","atMs":started.elapsed().as_secs_f64()*1000.,
            "completion":r.completion,"optimality":r.optimality,"teams":r.results.len(),
            "ranksCertified":r.results.iter().filter(|t| t.rank_certified==Some(true)).count()}));
    };
    let account = serde_json::from_str::<serde_json::Value>(&snapshot)?["format"] == "ournotes.account/1";
    let answer = if account {
        let mut report = |answer: &Answer| {
            if let Ok(value) = serde_json::to_value(answer) {
                let _ = emit(serde_json::json!({"type":"progress","atMs":started.elapsed().as_secs_f64()*1000.,
                    "optimality":value["result"]["optimality"], "status":value["status"],
                    "teams":value["result"]["teams"].as_array().map_or(0, Vec::len)}));
            }
        };
        serde_json::to_value(recommend_account(&data, &snapshot, &request, Some(AnswerProgress { interval, report: &mut report })))?
    } else {
        serde_json::to_value(recommend_snapshot(&data, &snapshot, &request, Some(Progress { interval, report: &mut report })))?
    };
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
    fs::write(&args[3], serde_json::to_vec(&answer)?)?;
    emit(serde_json::json!({"type":"done","elapsedMs":elapsed_ms}))?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
