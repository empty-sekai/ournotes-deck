//! Public recommendation facade: parse -> build -> search. No game formula lives here.
use crate::clock::Instant;
use crate::types::{RecommendationOutcome, RecommendationRequest};
use ournotes_sim::{Error, cards::Roster, data::DeckData};

/// Recommend using an already loaded dataset and parsed roster/request.
/// The request deadline includes building the problem and running the solver.
pub fn recommend(
    data: &DeckData,
    roster: &Roster,
    request: &RecommendationRequest,
) -> Result<RecommendationOutcome, Error> {
    let start = Instant::now();
    let built = crate::handler::build_card_pool(data, roster, request)?;
    crate::search::dispatch::execute(&built, None, start, start.elapsed().as_secs_f64() * 1000.0)
}

/// JSON transport over the same typed entry point. Retain DeckData between calls.
pub fn recommend_json(data: &DeckData, roster_json: &str, request_json: &str) -> Result<String, Error> {
    let roster = Roster::from_json(roster_json)?;
    let request = serde_json::from_str(request_json).map_err(|e| Error::Input(format!("request: {e}")))?;
    let result = recommend(data, &roster, &request)?;
    serde_json::to_string(&result).map_err(|e| Error::Domain(format!("result JSON: {e}")))
}
