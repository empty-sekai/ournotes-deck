//! Static applier dispatch states. Updaters, checks, clocks and counters still run on every frame.
//!
//! The omitted calls are identities of `LiveModel::apply`, established by its effect-type branches, not
//! by an observed quiet frame. In particular, conversion limits, cumulative factors and raw windows keep
//! the ordinary path. Unknown target errors also keep their original frame of observation.
use super::engine::{END_FRAME, EXECUTE_FRAME};

#[derive(Clone, Copy, Debug)]
pub(super) struct ApplierPlan(u8);

impl ApplierPlan {
    const ALL: Self = Self(u8::MAX);

    pub(super) fn compile(effect_type: i64, valid_targets: bool) -> Self {
        #[cfg(any(test, feature = "search-diagnostics"))]
        if !control::enabled() {
            return Self::ALL;
        }
        let start = 1 << EXECUTE_FRAME;
        let edges = start | (1 << END_FRAME);
        match effect_type {
            // These branches read their value only on ExecuteFrame; no target lookup or limit observer.
            3001 | 3002 | 15000 | 11002..=11004 | 12002 | 12003 | 13003 | 13004 => Self(start),
            // No operation, allocation, checker or effect-state write occurs in Executing.
            2000 | 2002 | 2005 | 3000 | 3003 | 3004 | 11000 | 11001 | 12000 | 13000 | 13002 => Self(edges),
            // The original branch validates targets even in Executing. Invalid targets must still fail there.
            2004 if valid_targets => Self(edges),
            // In particular: 2001/2003, 11005, 12004, 12006/13005, 4000..4004/13001.
            _ => Self::ALL,
        }
    }

    #[inline]
    pub(super) fn observes(self, state: u8) -> bool {
        state >= 8 || (self.0 & (1 << state)) != 0
    }
}

#[cfg(any(test, feature = "search-diagnostics"))]
mod control {
    use std::cell::Cell;
    thread_local! {
        static ENABLED: Cell<bool> = const { Cell::new(true) };
    }
    pub(super) fn enabled() -> bool {
        ENABLED.with(Cell::get)
    }
    /// Construct reference models with every original applier dispatch enabled. Existing models keep their
    /// immutable plan. Nested scopes and unwinding restore the previous thread-local policy.
    #[cfg(feature = "search-diagnostics")]
    pub fn with_applier_plan_disabled<T>(f: impl FnOnce() -> T) -> T {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                ENABLED.with(|enabled| enabled.set(self.0));
            }
        }
        let _restore = Restore(ENABLED.with(|enabled| enabled.replace(false)));
        f()
    }
}

#[cfg(feature = "search-diagnostics")]
pub use control::with_applier_plan_disabled;
