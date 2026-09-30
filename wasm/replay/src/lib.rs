//! A JSON transport only. Every calculation and default play template lives in ournotes-deck.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ReplaySession {
    inner: ournotes_deck::replay::ReplaySession,
}

fn js_error(error: ournotes_deck::Error) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
impl ReplaySession {
    #[wasm_bindgen(constructor)]
    pub fn new(deck_data_json: &str) -> Result<ReplaySession, JsValue> {
        Ok(Self { inner: ournotes_deck::replay::ReplaySession::from_json(deck_data_json).map_err(js_error)? })
    }
    pub fn run(&self, request_json: &str) -> Result<String, JsValue> {
        self.inner.run_json(request_json).map_err(js_error)
    }
    #[wasm_bindgen(js_name = describeChart)]
    pub fn describe_chart(&self, score_id: i32) -> Result<String, JsValue> {
        self.inner.describe_chart_json(i64::from(score_id)).map_err(js_error)
    }
    pub fn template(&self, score_id: i32, power: i32, fps: u32) -> Result<String, JsValue> {
        self.inner.template_json(i64::from(score_id), power, fps).map_err(js_error)
    }
}
