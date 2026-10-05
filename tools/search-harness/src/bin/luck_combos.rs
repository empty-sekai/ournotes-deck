//! LUCK table combinations on a request's chart: random sets of luck chain skills sampled together beside their
//! single entries.
use ournotes_search::{handler, search::diagnostics, types::RecommendationRequest};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde::Deserialize;
use std::{env, fs};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deck {
    members: [i64; 5],
    snaps: [Option<i64>; 5],
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 9 {
        return Err("luck_combos DATA ROSTER REQUEST DECKS RUNS COMBOS MAX_K SEED OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let d = decks.first().ok_or("no deck")?;
    let v = diagnostics::luck_combos(
        &built,
        d.members,
        d.snaps,
        args[4].parse()?,
        args[5].parse()?,
        args[6].parse()?,
        args[7].parse()?,
    )?;
    fs::write(&args[8], serde_json::to_vec(&v)?)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
