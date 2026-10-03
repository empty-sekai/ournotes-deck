//! Audit the simulation cutoff (settled prefixes and caps) against complete simulations.
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
        return Err("cutoff_audit DATA ROSTER REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let decks: Vec<_> = decks.into_iter().map(|d| (d.members, d.snaps)).collect();
    let report = diagnostics::audit_cutoff(&built, &decks)?;
    // Preserve counterexamples before exiting unsuccessfully. A completed audit
    // with unsupported roots is explicit partial coverage, not a passed audit.
    fs::write(&args[4], serde_json::to_vec_pretty(&report)?)?;
    if report["violations"].as_u64() != Some(0) {
        return Err("Cutoff audit found violations; inspect the saved report".into());
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
