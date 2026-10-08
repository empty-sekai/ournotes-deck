//! Thread-local diagnostic phase totals; absent from ordinary/production WASM builds.
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckScoreProfile {
    pub evaluations: u64,
    pub terminal_evaluations: u64,
    pub terminal_replay_fallbacks: u64,
    pub terminal_cancellations: u64,
    pub terminal_capacity_refusals: u64,
    pub model_setup_ms: f64,
    pub curve_dp_ms: f64,
    pub recorder_run_ms: f64,
    pub recorder_trace_only_runs: u64,
    pub recorder_trace_only_queries: u64,
    pub recorder_trace_only_active_queries: u64,
    /// Complete native score recordings that also produced the nominal probability transcript.
    pub fused_recordings: u64,
    /// Original frames observed in that shared pass, including work before an error or cancellation.
    pub fused_frames: u64,
    /// Optional shared-pass admissions declined before retaining a probability transcript.
    pub fused_refusals: u64,
    pub bound_replay_ms: f64,
    pub factor_replay_ms: f64,
    pub combo_history_ms: f64,
    pub note_bounds_ms: f64,
    pub rank_bounds_ms: f64,
    pub rank_residue_attempts: u64,
    pub rank_residue_windows: u64,
    pub rank_residue_unresolved_windows: u64,
    pub rank_residue_peak_states: usize,
    /// Transitions in completed residue batches, including their controller admission suffixes.
    pub rank_residue_transitions: u64,
    pub rank_residue_ms: f64,
    pub program_key_ms: f64,
    pub program_lookup_ms: f64,
    pub program_recorded_key_ms: f64,
    pub program_recorded_lookup_ms: f64,
    pub program_run_ms: f64,
    pub factor_queries: u64,
    pub factor_quiet_queries: u64,
    pub factor_execute_frames: u64,
    pub factor_empty_frames: u64,
    pub factor_undo_frames: u64,
    pub paired_paths: u64,
    pub peak_paired_paths: usize,
    pub note_enclosures: u64,
    pub terminal_factor_builds: u64,
    pub terminal_factor_refusals: u64,
    pub terminal_factor_ms: f64,
    pub terminal_factor_additions: u64,
    pub terminal_factor_undos: u64,
    pub terminal_factor_probe_runs: u64,
    pub terminal_factor_maximum_state: f64,
    pub terminal_factor_maximum_drift: f64,
    pub terminal_kernel_builds: u64,
    pub terminal_kernel_refusals: u64,
    pub terminal_kernel_ms: f64,
    pub terminal_kernel_notes: u64,
    pub terminal_kernel_combo_observations: u64,
    pub native_score_builds: u64,
    pub native_score_plan_refusals: u64,
    pub native_score_ready_refusals: u64,
    pub native_score_kernel_refusals: u64,
    pub native_score_support_refusals: u64,
    pub native_score_ms: f64,
    pub native_score_rank_windows: u64,
    pub native_score_rank_notes: u64,
}

thread_local! {
    static PROFILE: RefCell<LuckScoreProfile> = RefCell::new(LuckScoreProfile::default());
}

pub(super) fn record(value: LuckScoreProfile) {
    PROFILE.with(|profile| {
        let mut total = profile.borrow_mut();
        total.evaluations += value.evaluations;
        total.terminal_evaluations += value.terminal_evaluations;
        total.terminal_replay_fallbacks += value.terminal_replay_fallbacks;
        total.terminal_cancellations += value.terminal_cancellations;
        total.terminal_capacity_refusals += value.terminal_capacity_refusals;
        total.model_setup_ms += value.model_setup_ms;
        total.curve_dp_ms += value.curve_dp_ms;
        total.recorder_run_ms += value.recorder_run_ms;
        total.recorder_trace_only_runs += value.recorder_trace_only_runs;
        total.recorder_trace_only_queries += value.recorder_trace_only_queries;
        total.recorder_trace_only_active_queries += value.recorder_trace_only_active_queries;
        total.fused_recordings += value.fused_recordings;
        total.fused_frames += value.fused_frames;
        total.fused_refusals += value.fused_refusals;
        total.bound_replay_ms += value.bound_replay_ms;
        total.factor_replay_ms += value.factor_replay_ms;
        total.combo_history_ms += value.combo_history_ms;
        total.note_bounds_ms += value.note_bounds_ms;
        total.rank_bounds_ms += value.rank_bounds_ms;
        total.rank_residue_attempts += value.rank_residue_attempts;
        total.rank_residue_windows += value.rank_residue_windows;
        total.rank_residue_unresolved_windows += value.rank_residue_unresolved_windows;
        total.rank_residue_peak_states = total.rank_residue_peak_states.max(value.rank_residue_peak_states);
        total.rank_residue_transitions += value.rank_residue_transitions;
        total.rank_residue_ms += value.rank_residue_ms;
        total.program_key_ms += value.program_key_ms;
        total.program_lookup_ms += value.program_lookup_ms;
        total.program_recorded_key_ms += value.program_recorded_key_ms;
        total.program_recorded_lookup_ms += value.program_recorded_lookup_ms;
        total.program_run_ms += value.program_run_ms;
        total.factor_queries += value.factor_queries;
        total.factor_quiet_queries += value.factor_quiet_queries;
        total.factor_execute_frames += value.factor_execute_frames;
        total.factor_empty_frames += value.factor_empty_frames;
        total.factor_undo_frames += value.factor_undo_frames;
        total.paired_paths += value.paired_paths;
        total.peak_paired_paths = total.peak_paired_paths.max(value.peak_paired_paths);
        total.note_enclosures += value.note_enclosures;
        total.terminal_factor_builds += value.terminal_factor_builds;
        total.terminal_factor_refusals += value.terminal_factor_refusals;
        total.terminal_factor_ms += value.terminal_factor_ms;
        total.terminal_factor_additions += value.terminal_factor_additions;
        total.terminal_factor_undos += value.terminal_factor_undos;
        total.terminal_factor_probe_runs += value.terminal_factor_probe_runs;
        total.terminal_factor_maximum_state =
            total.terminal_factor_maximum_state.max(value.terminal_factor_maximum_state);
        total.terminal_factor_maximum_drift =
            total.terminal_factor_maximum_drift.max(value.terminal_factor_maximum_drift);
        total.terminal_kernel_builds += value.terminal_kernel_builds;
        total.terminal_kernel_refusals += value.terminal_kernel_refusals;
        total.terminal_kernel_ms += value.terminal_kernel_ms;
        total.terminal_kernel_notes += value.terminal_kernel_notes;
        total.terminal_kernel_combo_observations += value.terminal_kernel_combo_observations;
        total.native_score_builds += value.native_score_builds;
        total.native_score_plan_refusals += value.native_score_plan_refusals;
        total.native_score_ready_refusals += value.native_score_ready_refusals;
        total.native_score_kernel_refusals += value.native_score_kernel_refusals;
        total.native_score_support_refusals += value.native_score_support_refusals;
        total.native_score_ms += value.native_score_ms;
        total.native_score_rank_windows += value.native_score_rank_windows;
        total.native_score_rank_notes += value.native_score_rank_notes;
    });
}

/// Count actual structural calculate entries immediately, including work before an error or cancellation.
/// Active means that the native undo or execution frame interval is nonempty; no frame or note is scanned.
pub(super) fn record_trace_only_query(first: bool, active: bool) {
    PROFILE.with(|profile| {
        let mut total = profile.borrow_mut();
        total.recorder_trace_only_runs += u64::from(first);
        total.recorder_trace_only_queries += 1;
        total.recorder_trace_only_active_queries += u64::from(active);
    });
}

/// Keep the full recorder's original timing boundary while retaining work on every early return.
pub(super) struct RecorderTimer(std::time::Instant);

impl RecorderTimer {
    pub(super) fn start() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Drop for RecorderTimer {
    fn drop(&mut self) {
        let elapsed = self.0.elapsed().as_secs_f64() * 1e3;
        PROFILE.with(|profile| profile.borrow_mut().recorder_run_ms += elapsed);
    }
}

/// Return and reset the calling thread's diagnostic certificate work and timings. Recorder work includes
/// partial attempts that stop or fail; these measurements neither change a score bound nor certify completion.
pub fn take_luck_score_profile() -> LuckScoreProfile {
    PROFILE.with(|profile| std::mem::take(&mut *profile.borrow_mut()))
}
