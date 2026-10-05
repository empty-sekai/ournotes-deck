//! Native all-path score enclosures for physical decks; results remain bounded diagnostics.
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
    let mut args: Vec<_> = env::args().skip(1).collect();
    let compact = args.first().is_some_and(|arg| arg == "--compact");
    if compact {
        args.remove(0);
    }
    if args.len() != 5 {
        return Err("luck_score_bounds [--compact] DATA ROSTER REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    if decks.is_empty() {
        return Err("no deck".into());
    }
    let mut output = Vec::with_capacity(decks.len());
    for (index, deck) in decks.iter().enumerate() {
        let mut result = if compact {
            diagnostics::luck_score_summary_replay(&built, deck.members, deck.snaps)?
        } else {
            diagnostics::luck_score_bounds_replay(&built, deck.members, deck.snaps)?
        };
        result["deckIndex"] = index.into();
        output.push(result);
    }
    fs::write(
        &args[4],
        serde_json::to_vec(&serde_json::json!({
            "format":"ournotes-deck.luck-score-bounds/1","decks":output
        }))?,
    )?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
