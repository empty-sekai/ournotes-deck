//! Full-domain diagnostic schedule runs. This is not an exhaustive oracle.
use ournotes_search::{search::diagnostics, types::RecommendationRequest};
use ournotes_sim::{cards::Roster, data::DeckData};
use std::{env, fs};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("search_variant SCHEDULE DATA ROSTER REQUEST OUTPUT".into());
    }
    let schedule = serde_json::from_value::<diagnostics::Schedule>(serde_json::Value::String(args[0].clone()))?;
    let data = DeckData::from_path(&args[1])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[2])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let outcome = diagnostics::recommend_experiment(&data, &roster, &request, schedule)?;
    let built = ournotes_search::handler::build_card_pool(&data, &roster, &request)?;
    let mut explanations = Vec::new();
    for deck in &outcome.results {
        if let Some(best) = &deck.best_order {
            explanations.push(serde_json::json!({"members":deck.members,"snaps":deck.snaps,
                "order":best.performance_order,"score":best.score,
                "bound":diagnostics::describe_bound(&built,deck.members,deck.snaps,best.performance_order)?}));
        }
    }
    fs::write(
        &args[4],
        serde_json::to_vec_pretty(&serde_json::json!({"schedule":schedule,
        "evidenceScope":"diagnostic search under the shared scorer; no exhaustive oracle", "outcome":outcome,
        "boundExplanations":explanations}))?,
    )?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
