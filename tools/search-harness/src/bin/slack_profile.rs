//! Pair a fixed deck's per-entry fine-cap terms with its actual per-note scores for every declared root.
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
    if args.len() != 5 {
        return Err("slack_profile DATA ROSTER REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let mut values = Vec::new();
    for d in decks {
        let profiles = diagnostics::slack_profile(&built, d.members, d.snaps)?;
        values.push(serde_json::json!({"members":d.members,"snaps":d.snaps,"profiles":profiles}));
    }
    fs::write(
        &args[4],
        serde_json::to_vec(&serde_json::json!({
            "scope":"Fine-cap terms beside one simulation per root; attribution only, the terms are not caps.",
            "decks":values
        }))?,
    )?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
