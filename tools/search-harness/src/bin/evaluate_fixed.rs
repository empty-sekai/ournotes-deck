//! Evaluate externally proposed physical decks without running a candidate search.
use ournotes_search::{auxiliary, types::RecommendationRequest};
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
    if args.len() != 5 {
        return Err("evaluate_fixed DATA ROSTER REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let values = decks
        .into_iter()
        .map(|d| auxiliary::evaluate_fixed(&data, &roster, &request, d.members, d.snaps))
        .collect::<Result<Vec<_>, _>>()?;
    fs::write(&args[4], serde_json::to_vec_pretty(&values)?)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
