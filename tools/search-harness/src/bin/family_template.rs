//! Inspect the compiled full-domain controller-family template without evaluating a candidate or order.
use ournotes_search::{
    handler,
    owned_snapshot::{GoalDependencies, OwnedSnapshot},
    search::diagnostics,
    types::RecommendationRequest,
};
use ournotes_sim::{cards::Roster, data::DeckData};
use std::{env, fs};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("family_template DATA ROSTER|SNAPSHOT REQUEST OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let input = fs::read_to_string(&args[1])?;
    let roster = if let Ok(snapshot) = OwnedSnapshot::from_json(&input) {
        let resolution = snapshot.resolve_data(
            &data,
            data.sha256.as_deref().unwrap_or_default(),
            GoalDependencies::of(&request.execution),
        );
        resolution
            .resolved
            .ok_or_else(|| format!("snapshot unresolved: {:?} {:?}", resolution.missing, resolution.errors))?
            .diagnostic_projection()
            .clone()
    } else {
        Roster::from_json(&input)?
    };
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let value = serde_json::json!({
        "format": "ournotes-deck.family-template-diagnostics/1",
        "scope": "Full-domain bound compilation only; no candidate, probability law or ranking certificate.",
        "members": built.domain().members().len(),
        "snaps": built.domain().snaps().len(),
        "k": request.k,
        "familyTemplate": diagnostics::family_template_diagnostics(&built),
    });
    fs::write(&args[3], serde_json::to_vec_pretty(&value)?)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
