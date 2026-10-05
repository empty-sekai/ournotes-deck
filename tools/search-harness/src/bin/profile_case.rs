//! One native request followed by the calling thread's LUCK certificate phase totals (diagnostic builds only).
use ournotes_search::engine::recommend_snapshot;
use ournotes_sim::{data::DeckData, live::full::take_luck_score_profile};
use std::{env, fs, time::Instant};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("profile_case DATA SNAPSHOT REQUEST OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let snapshot = fs::read_to_string(&args[1])?;
    let request = fs::read_to_string(&args[2])?;
    take_luck_score_profile();
    let started = Instant::now();
    let answer = recommend_snapshot(&data, &snapshot, &request, None);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
    let profile = take_luck_score_profile();
    fs::write(&args[3], serde_json::to_vec(&answer)?)?;
    println!("{}", serde_json::json!({"elapsedMs":elapsed_ms,"luckProfile":profile}));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
