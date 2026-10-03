//! Diagnostic control and counters for the deterministic condition-skill idle plan.
//!
//! The switch is thread-local and captured when an updater is built. Existing models
//! keep their execution plan; wrapping a whole fixed evaluation/search builds a clean
//! reference run without changing player input or another worker's policy.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdlePlanStats {
    pub updaters_built: u64,
    pub eligible_updaters: u64,
    pub enabled_eligible_updaters: u64,
    pub update_calls: u64,
    pub slept_calls: u64,
    pub slept_frames: u64,
}

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(true) };
    static STATS: Cell<IdlePlanStats> = const { Cell::new(IdlePlanStats {
        updaters_built: 0, eligible_updaters: 0, enabled_eligible_updaters: 0,
        update_calls: 0, slept_calls: 0, slept_frames: 0,
    }) };
}

pub(super) fn enabled() -> bool {
    ENABLED.with(Cell::get)
}

/// Build reference models without the idle plan on this thread. Nested calls and
/// unwinding restore the previous policy. This does not reset diagnostic counters.
pub fn with_idle_plan_disabled<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ENABLED.with(|enabled| enabled.set(self.0));
        }
    }
    let _restore = Restore(ENABLED.with(|enabled| enabled.replace(false)));
    f()
}

/// Return and clear this thread's counters. No persistent application cache is reset.
pub fn take_idle_plan_stats() -> IdlePlanStats {
    STATS.with(|stats| stats.replace(IdlePlanStats::default()))
}

pub(super) fn built(eligible: bool, enabled: bool) {
    STATS.with(|stats| {
        let mut value = stats.get();
        value.updaters_built += 1;
        value.eligible_updaters += u64::from(eligible);
        value.enabled_eligible_updaters += u64::from(eligible && enabled);
        stats.set(value);
    });
}

pub(super) fn update(slept: bool, first: bool) {
    STATS.with(|stats| {
        let mut value = stats.get();
        value.update_calls += 1;
        value.slept_calls += u64::from(slept);
        value.slept_frames += u64::from(first);
        stats.set(value);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_scope_restores_nested_and_unwinding_policy() {
        assert!(enabled());
        with_idle_plan_disabled(|| {
            assert!(!enabled());
            with_idle_plan_disabled(|| assert!(!enabled()));
            assert!(!enabled());
            let failed = std::panic::catch_unwind(|| with_idle_plan_disabled(|| panic!("probe")));
            assert!(failed.is_err());
            assert!(!enabled());
        });
        assert!(enabled());
        assert!(std::panic::catch_unwind(|| with_idle_plan_disabled(|| panic!("probe"))).is_err());
        assert!(enabled());
    }
}
