//! Deck power, skip score and live score of BanG Dream! Our Notes: the model the deck search and the chart
//! statistics run on.
//!
//! The arithmetic reproduces the game's own integer and binary32 float semantics, including its rounding and
//! floor conversions, so a value computed here is the value the game computes for the same inputs.
//!
//! Layout:
//! - [`power`], [`calc`], [`bonus`], [`memory`], [`cards`], [`deck`]: card stats, slot power and deck power;
//! - [`pool`]: a roster resolved against the master, and a deck by pool indexes;
//! - [`live`]: per-note score, skip score, live skills, the whole-live simulation and the random streams;
//! - [`event`]: event bonuses, score ranks and the client's event-point computation;
//! - [`scenario`]: the music and event inputs each live mode selects;
//! - [`master`]: the master tables the crate reads (supplied by the user, not bundled);
//! - [`data`]: the deck data file (`nnnotes.deck-data/1`) with the master tables and every chart;
//! - [`chartstats`]: per-chart score coefficients that hold for every deck, for chart rankings;
//! - [`replay`]: one live replayed from explicit judgements, the model behind the replay WASM.
//!
//! The exact Top-K deck search is the crate `ournotes-search`, built on this one.

pub mod account;
pub mod bonus;
pub mod calc;
pub mod cards;
pub mod chartstats;
pub mod data;
pub mod deck;
pub mod error;
pub mod event;
pub mod live;
pub mod master;
pub mod memory;
pub mod num;
pub mod pool;
pub mod power;
pub mod replay;
pub mod scenario;

pub use error::Error;

/// The SHA-256 of this crate's sources: `Cargo.toml`, `build.rs` and every file under `src/`, each as its path and
/// contents. Builds of sources with the same value run the same model, so a result computed by one of them (chart
/// statistics, a replay, a live score) holds for the others; a cache of such results can key them by this value
/// instead of by the release or commit.
pub const SOURCE_SHA256: &str = env!("OURNOTES_SIM_SOURCE_SHA256");

#[cfg(test)]
mod source_tests {
    #[test]
    fn the_source_digest_is_a_sha256() {
        assert!(
            super::SOURCE_SHA256.len() == 64
                && super::SOURCE_SHA256.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
    }
}
