//! The same cooperative search clock on native hosts and in a browser Worker.
//!
//! Numeric simulation does not read this clock. It measures search budgets and
//! diagnostics; browser deadlines use `performance.now` through `web-time`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_time::Instant;
