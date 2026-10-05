//! Browser Worker transport for account recommendations; no scoring copy.
use ournotes_search::{
    engine::{Answer, AnswerProgress, Progress, recommend_account, recommend_snapshot},
    types::RecommendationOutcome,
};
use ournotes_sim::data::DeckData;
use std::time::Duration;
use wasm_bindgen::prelude::*;

const PROGRESS_INTERVAL_MS: u32 = 250;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(resultJson: string) => void")]
    pub type ProgressCallback;
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call(this: &ProgressCallback, receiver: &JsValue, result_json: &str) -> Result<JsValue, JsValue>;
}

/// One immutable deck data generation reused across recommendations.
#[wasm_bindgen]
pub struct DeckSolver {
    data: DeckData,
}

/// Identifies only the legacy input envelope, without parsing or exposing account fields.
fn legacy_snapshot(input: &str) -> bool {
    #[derive(serde::Deserialize)]
    struct Format {
        format: String,
    }
    serde_json::from_str::<Format>(input).is_ok_and(|v| v.format == "ournotes.owned-snapshot/1")
}

#[wasm_bindgen]
impl DeckSolver {
    /// Original UTF-8 deck-data bytes (`Uint8Array`), or original text for legacy callers.
    /// Invalid UTF-8 is rejected; no replacement decoding or integer parsing changes the dataset hash.
    #[wasm_bindgen(constructor)]
    pub fn new(deck_data: JsValue) -> Result<DeckSolver, JsError> {
        let text = if let Some(text) = deck_data.as_string() {
            text
        } else if let Some(bytes) = deck_data.dyn_ref::<js_sys::Uint8Array>() {
            String::from_utf8(bytes.to_vec()).map_err(|_| JsError::new("deck data must be valid UTF-8"))?
        } else {
            return Err(JsError::new("deck data must be a Uint8Array or original JSON text"));
        };
        DeckData::from_json(&text).map(|data| Self { data }).map_err(|error| JsError::new(&error.to_string()))
    }

    /// SHA-256 of the original deck-data bytes, including any UTF-8 BOM.
    #[wasm_bindgen(getter, js_name = datasetId)]
    pub fn dataset_id(&self) -> String {
        self.data.sha256.clone().expect("deck data read from text")
    }

    /// Formats, supported goals/metrics and explicit limitations of this build, as JSON text.
    pub fn capabilities(&self) -> String {
        serde_json::to_string(&ournotes_search::engine::capabilities()).expect("capabilities JSON")
    }

    /// `ournotes.account/1` + `ournotes-deck.recommendation-request/2` -> account-recommendation/1.
    /// Every progress callback receives a whole answer with `final:false`; terminate the Worker to cancel.
    /// Legacy owned-snapshot inputs retain their former request and result formats for existing harness callers.
    pub fn recommend(
        &self,
        account_json: &str,
        request_json: &str,
        on_progress: Option<ProgressCallback>,
        progress_interval_ms: Option<u32>,
    ) -> String {
        let interval = Duration::from_millis(progress_interval_ms.unwrap_or(PROGRESS_INTERVAL_MS).into());
        if legacy_snapshot(account_json) {
            let mut report = |out: &RecommendationOutcome| {
                if let Some(callback) = &on_progress {
                    let _ = callback.call(&JsValue::NULL, &serde_json::to_string(out).expect("result JSON"));
                }
            };
            let progress = on_progress.is_some().then_some(Progress { interval, report: &mut report });
            serde_json::to_string(&recommend_snapshot(&self.data, account_json, request_json, progress))
                .expect("answer JSON")
        } else {
            let mut report = |answer: &Answer| {
                if let Some(callback) = &on_progress {
                    let _ = callback.call(&JsValue::NULL, &serde_json::to_string(answer).expect("answer JSON"));
                }
            };
            let progress = on_progress.is_some().then_some(AnswerProgress { interval, report: &mut report });
            serde_json::to_string(&recommend_account(&self.data, account_json, request_json, progress))
                .expect("answer JSON")
        }
    }
}
