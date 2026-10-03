//! Browser Worker transport for owned-snapshot recommendations; no scoring copy.
//!
//! A solver owns one deck data generation. Snapshot and request cross the boundary as their original JSON text,
//! which preserves large integers and decimal tokens, and every answer returns as JSON text.
use ournotes_search::{
    engine::{Progress, recommend_snapshot},
    types::RecommendationOutcome,
};
use ournotes_sim::data::DeckData;
use std::time::Duration;
use wasm_bindgen::prelude::*;

/// Default milliseconds between two progress reports.
const PROGRESS_INTERVAL_MS: u32 = 250;

#[wasm_bindgen]
extern "C" {
    /// A JavaScript function that receives one progress report as JSON text.
    #[wasm_bindgen(typescript_type = "(resultJson: string) => void")]
    pub type ProgressCallback;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call(this: &ProgressCallback, receiver: &JsValue, result_json: &str) -> Result<JsValue, JsValue>;
}

#[wasm_bindgen]
pub struct DeckSolver {
    data: DeckData,
}

#[wasm_bindgen]
impl DeckSolver {
    /// Parse one deck data document (`nnnotes.deck-data/1`); throws an Error with the reason when it is invalid.
    #[wasm_bindgen(constructor)]
    pub fn new(deck_data_json: &str) -> Result<DeckSolver, JsError> {
        DeckData::from_json(deck_data_json).map(|data| Self { data }).map_err(|error| JsError::new(&error.to_string()))
    }

    /// Lowercase hexadecimal SHA-256 of the deck data text: the `datasetId` a snapshot must name.
    #[wasm_bindgen(getter, js_name = datasetId)]
    pub fn dataset_id(&self) -> String {
        self.data.sha256.clone().expect("deck data read from text")
    }

    /// Recommend for one owned snapshot and one request, both as their original JSON text. Returns the
    /// `ournotes-deck.snapshot-recommendation/1` answer as JSON text: input problems are listed in it, not thrown.
    /// `onProgress`, when given, receives progress reports (result JSON text) at most once per
    /// `progressIntervalMs` (default 250); exceptions it throws are ignored. The call is synchronous: run it in a
    /// dedicated Worker and terminate the Worker to cancel.
    pub fn recommend(
        &self,
        snapshot_json: &str,
        request_json: &str,
        on_progress: Option<ProgressCallback>,
        progress_interval_ms: Option<u32>,
    ) -> String {
        let mut report = |out: &RecommendationOutcome| {
            if let Some(callback) = &on_progress {
                let _ = callback.call(&JsValue::NULL, &serde_json::to_string(out).expect("result JSON"));
            }
        };
        let interval = Duration::from_millis(progress_interval_ms.unwrap_or(PROGRESS_INTERVAL_MS).into());
        let progress = on_progress.is_some().then_some(Progress { interval, report: &mut report });
        serde_json::to_string(&recommend_snapshot(&self.data, snapshot_json, request_json, progress))
            .expect("answer JSON")
    }
}
