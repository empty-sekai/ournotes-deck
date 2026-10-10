//! A JSON transport only. Every calculation and default play template lives in ournotes-sim.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ReplaySession {
    inner: ournotes_sim::replay::ReplaySession,
}

fn js_error(error: ournotes_sim::Error) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
impl ReplaySession {
    #[wasm_bindgen(constructor)]
    pub fn new(deck_data_json: &str) -> Result<ReplaySession, JsValue> {
        Ok(Self { inner: ournotes_sim::replay::ReplaySession::from_json(deck_data_json).map_err(js_error)? })
    }
    #[wasm_bindgen(js_name = startRankAnalysis)]
    pub fn start_rank_analysis(&self, request_json: &str) -> Result<ReplayRankAnalysisJob, JsValue> {
        Ok(ReplayRankAnalysisJob { inner: self.inner.start_rank_analysis_json(request_json).map_err(js_error)? })
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

#[wasm_bindgen]
pub struct ReplayRankAnalysisJob {
    inner: ournotes_sim::replay::RankAnalysisJob,
}

#[wasm_bindgen]
impl ReplayRankAnalysisJob {
    pub fn advance(&mut self, max_orders: u32) -> Result<String, JsValue> {
        self.inner.advance_json(max_orders as usize).map_err(js_error)
    }

    pub fn status(&self) -> Result<String, JsValue> {
        self.inner.status_json().map_err(js_error)
    }
}
