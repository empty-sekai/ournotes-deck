//! Thread-local diagnostic phase totals; absent from ordinary/production WASM builds.
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreProfile {
    pub evaluations: u64,
    pub model_setup_ms: f64,
    pub curve_dp_ms: f64,
    pub recorder_run_ms: f64,
    pub bound_replay_ms: f64,
}

thread_local! {
    static PROFILE: RefCell<LuckScoreProfile> = RefCell::new(LuckScoreProfile::default());
}

pub(super) fn record(value: LuckScoreProfile) {
    PROFILE.with(|profile| {
        let mut total = profile.borrow_mut();
        total.evaluations += value.evaluations;
        total.model_setup_ms += value.model_setup_ms;
        total.curve_dp_ms += value.curve_dp_ms;
        total.recorder_run_ms += value.recorder_run_ms;
        total.bound_replay_ms += value.bound_replay_ms;
    });
}

/// Return and reset the calling thread's completed certificate timings. These counters are diagnostic
/// measurements only; they neither change a score bound nor certify a request's completion.
pub fn take_luck_score_profile() -> LuckScoreProfile {
    PROFILE.with(|profile| std::mem::take(&mut *profile.borrow_mut()))
}
