//! Browser transport for the recommendation adapter; no scoring copy.
//!
//! A session owns one DeckData generation. Roster and request inputs cross the
//! boundary as original JSON text, preserving large integers and decimal tokens.
use ournotes_search::{engine::recommend, types::RecommendationRequest};
use ournotes_sim::{cards::Roster, data::DeckData};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct RecommendationSession {
    data: DeckData,
    roster: Option<Roster>,
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
impl RecommendationSession {
    /// Parse and retain exactly one caller-provided data generation.
    #[wasm_bindgen(constructor)]
    pub fn new(deck_data_json: &str) -> Result<RecommendationSession, JsValue> {
        Ok(Self { data: DeckData::from_json(deck_data_json).map_err(js_error)?, roster: None })
    }

    /// A failed parse preserves the previously configured roster.
    /// The production core's progression defaults and validation remain authoritative.
    #[wasm_bindgen(js_name = setRoster)]
    pub fn set_roster(&mut self, roster_json: &str) -> Result<(), JsValue> {
        let roster = Roster::from_json(roster_json).map_err(js_error)?;
        self.roster = Some(roster);
        Ok(())
    }

    /// Execute the same production Rust function as the native CLI.
    /// Synchronous within the calling Worker: deadlines are cooperative, and a
    /// queued JS message cannot interrupt this call. Terminating the Worker drops
    /// its state; no unfinished candidate is presented as a finished result.
    pub fn recommend(&self, request_json: &str) -> Result<String, JsValue> {
        let roster = self.roster.as_ref().ok_or_else(|| JsValue::from_str("setRoster is required before recommend"))?;
        let request: RecommendationRequest = serde_json::from_str(request_json).map_err(js_error)?;
        let result = recommend(&self.data, roster, &request).map_err(js_error)?;
        serde_json::to_string(&result).map_err(js_error)
    }
}
