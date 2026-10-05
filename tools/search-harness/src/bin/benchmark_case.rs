//! One native benchmark request, with an explicit post-dataset-load timing boundary.
use ournotes_search::engine::{Progress, recommend_snapshot};
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
    let setup = Instant::now();
    let data = DeckData::from_path(&args[0])?;
    let snapshot = fs::read_to_string(&args[1])?;
    let request = fs::read_to_string(&args[2])?;
    emit(serde_json::json!({"type":"ready","setupMs":setup.elapsed().as_secs_f64()*1000.,
        "datasetId":data.sha256,"arch":std::env::consts::ARCH}))?;
    let started = Instant::now();
    let mut report = |r: &ournotes_search::types::RecommendationOutcome| {
        let _ = emit(serde_json::json!({"type":"progress","atMs":started.elapsed().as_secs_f64()*1000.,
            "completion":r.completion,"optimality":r.optimality,"teams":r.results.len(),
            "ranksCertified":r.results.iter().filter(|t| t.rank_certified==Some(true)).count()}));
    };
    let answer = recommend_snapshot(
        &data,
        &snapshot,
        &request,
        Some(Progress { interval: Duration::from_millis(250), report: &mut report }),
    );
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
