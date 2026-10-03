//! Errors.

use std::fmt;

/// Every failure the crate reports. Nothing is approximated silently: an input the game would reject, a feature
/// that is not modelled and a pool the search cannot represent are all explicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Master data is malformed or lacks a row or parameter the computation needs.
    Master(String),
    /// The roster, deck, chart or play is invalid (unknown card, duplicate id, missing level row, ...).
    Input(String),
    /// The game itself would fail on this input (for example a card rank without a rank row, or a note type that
    /// the score tables do not list).
    Game(String),
    /// The request needs a part of the game that this version does not model.
    Unsupported(String),
    /// The search's bounds are only proven for non-negative, non-overflowing values; the inputs leave that domain.
    Domain(String),
    /// The pool is larger than the search's index width.
    Capacity(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Master(s) => write!(f, "master data: {s}"),
            Error::Input(s) => write!(f, "input: {s}"),
            Error::Game(s) => write!(f, "rejected by the game's rules: {s}"),
            Error::Unsupported(s) => write!(f, "not supported: {s}"),
            Error::Domain(s) => write!(f, "outside the search domain: {s}"),
            Error::Capacity(s) => write!(f, "capacity: {s}"),
        }
    }
}

impl std::error::Error for Error {}
