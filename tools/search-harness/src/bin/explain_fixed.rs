//! Explain a fixed deck's admissible caps and diagnostic-only zero-margin value.
use ournotes_search::{
    auxiliary, handler,
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
        return Err("explain_fixed DATA ROSTER|SNAPSHOT REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let roster = roster_of(&data, &fs::read_to_string(&args[1])?, &request)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let mut values = Vec::new();
    for d in decks {
        let value = auxiliary::evaluate_built(&built, d.members, d.snaps)?;
        let mut explanations = Vec::new();
        for order in diagnostics::audit_orders() {
            explanations.push(serde_json::json!({"order":order,
                "bound":diagnostics::describe_bound(&built, d.members, d.snaps, order)?}));
        }
        values.push(serde_json::json!({"outcome":value,"boundExplanations":explanations}));
    }
    fs::write(
        &args[4],
        serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Fixed-deck scorer and bound diagnostics; zero-margin values are not admissible caps and never authorize pruning.",
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

/// A roster file as is, or the goal-scoped projection of an owned snapshot.
fn roster_of(
    data: &DeckData,
    text: &str,
    request: &RecommendationRequest,
) -> Result<Roster, Box<dyn std::error::Error>> {
    let Ok(snapshot) = OwnedSnapshot::from_json(text) else {
        return Ok(Roster::from_json(text)?);
    };
    let goal = GoalDependencies::of(&request.execution);
    let resolution = snapshot.resolve_data(data, data.sha256.as_deref().unwrap_or_default(), goal);
    let resolved = resolution
        .resolved
        .ok_or_else(|| format!("snapshot unresolved: {:?} {:?}", resolution.missing, resolution.errors))?;
    Ok(resolved.diagnostic_projection().clone())
}
