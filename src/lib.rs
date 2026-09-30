//! Deck power, skip score and live score of BanG Dream! Our Notes, and an exact Top-K deck search.
//!
//! The arithmetic reproduces the game's own integer and binary32 float semantics, including its rounding and
//! floor conversions, so a value computed here is the value the game computes for the same inputs.
//!
//! Layout:
//! - [`power`], [`calc`], [`bonus`], [`memory`], [`cards`], [`deck`]: card stats, slot power and deck power;
//! - [`live`]: per-note score, skip score, live skills and the random streams;
//! - [`event`]: event bonuses, score ranks and the client's event-point computation;
//! - [`master`]: the master tables the crate reads (supplied by the user, not bundled);
//! - [`data`]: the deck data file (`nnnotes.deck-data/1`) with the master tables and every chart;
//! - [`search`]: the exact Top-K deck search and its brute-force oracle;
//! - [`chartstats`]: per-chart score coefficients that hold for every deck, for chart rankings.

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
pub mod power;
pub mod replay;
pub mod scenario;
pub mod search;

pub use error::Error;
