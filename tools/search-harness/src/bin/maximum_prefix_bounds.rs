//! Read maximum-score prefix bounds without simulating or searching any deck.
use ournotes_search::{
    handler,
    owned_snapshot::{GoalDependencies, OwnedSnapshot},
    search::diagnostics,
    types::RecommendationRequest,
};
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
        return Err("maximum_prefix_bounds DATA ROSTER|SNAPSHOT REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let text = fs::read_to_string(&args[1])?;
    let roster = match OwnedSnapshot::from_json(&text) {
        Ok(snapshot) => {
            let goal = GoalDependencies::of(&request.execution);
            let resolution = snapshot.resolve_data(&data, data.sha256.as_deref().unwrap_or_default(), goal);
            resolution.resolved.ok_or("snapshot is unresolved")?.diagnostic_projection().clone()
        }
        Err(_) => Roster::from_json(&text)?,
    };
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let mut rows = Vec::new();
    for deck in decks {
        rows.push(serde_json::json!({"members":deck.members,"snaps":deck.snaps,
            "bounds":diagnostics::maximum_prefix_bounds(&built,deck.members,deck.snaps)?}));
    }
    fs::write(&args[4], serde_json::to_vec_pretty(&serde_json::json!({"decks":rows}))?)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
