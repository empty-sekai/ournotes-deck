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
    pub factor_replay_ms: f64,
    pub combo_history_ms: f64,
    pub note_bounds_ms: f64,
    pub rank_bounds_ms: f64,
    pub program_run_ms: f64,
    pub factor_queries: u64,
    pub factor_quiet_queries: u64,
    pub factor_execute_frames: u64,
    pub factor_empty_frames: u64,
    pub factor_undo_frames: u64,
    pub paired_paths: u64,
    pub peak_paired_paths: usize,
    pub note_enclosures: u64,
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
        total.factor_replay_ms += value.factor_replay_ms;
        total.combo_history_ms += value.combo_history_ms;
        total.note_bounds_ms += value.note_bounds_ms;
        total.rank_bounds_ms += value.rank_bounds_ms;
        total.program_run_ms += value.program_run_ms;
        total.factor_queries += value.factor_queries;
        total.factor_quiet_queries += value.factor_quiet_queries;
        total.factor_execute_frames += value.factor_execute_frames;
        total.factor_empty_frames += value.factor_empty_frames;
        total.factor_undo_frames += value.factor_undo_frames;
        total.paired_paths += value.paired_paths;
        total.peak_paired_paths = total.peak_paired_paths.max(value.peak_paired_paths);
        total.note_enclosures += value.note_enclosures;
    });
}

/// Return and reset the calling thread's completed certificate timings. These counters are diagnostic
/// measurements only; they neither change a score bound nor certify a request's completion.
pub fn take_luck_score_profile() -> LuckScoreProfile {
    PROFILE.with(|profile| std::mem::take(&mut *profile.borrow_mut()))
}
