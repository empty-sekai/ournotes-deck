//! Deck search: deterministic power/skip search and the exact search of played lives under the uniform member-order
//! target (`super::uniform`). Candidate proposals are heuristic; every returned value is evaluated exactly under the
//! declared model. Only exhaustion/proven pruning certifies K.

use super::expectation::{self, FiniteEvaluation, FiniteSeedContext, PhysicalDeck, SeedOutcome};
use super::telemetry::{self, Recorder, Telemetry, Traversal, slot};
use super::uniform::{self, ORDERS};
use super::{Completion, Objective, Pool, SearchRequest};
use crate::clock::Instant;
use crate::handler::{BuiltProblem, build_card_pool, reject_unsupported_lifecycle, validate};
use crate::types::*;
use ournotes_sim::replay::RankConfirmation;
use ournotes_sim::scenario::EventPayoffInput;
use ournotes_sim::{Error, cards::Roster, data::DeckData};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::time::Duration;

#[path = "composition.rs"]
mod composition;
#[path = "leaf.rs"]
mod leaf;
use leaf::Leaf;
#[path = "certified_engine.rs"]
pub(in crate::search) mod certified_engine;
use certified_engine::CertifiedState;
#[path = "deck_payoff_search.rs"]
mod deck_payoff_search;
#[path = "domain_laws.rs"]
mod domain_laws;
mod engine;
mod entry;
mod enumeration;
mod evaluation;
mod joint_search;
mod maximum;
#[path = "program_cache.rs"]
mod program_cache;
#[path = "progress.rs"]
mod progress;
#[cfg(test)]
mod ranking_tests;
#[path = "session.rs"]
mod session;
mod solve;
#[path = "team_power_search.rs"]
mod team_power_search;
#[path = "team_scores.rs"]
mod team_scores;
#[path = "warm.rs"]
mod warm;
use engine::{Engine, Offered};
use entry::{Entry, compare};
use enumeration::{ProposalRandom, members_rec, random_deck};
pub(crate) use evaluation::{arithmetic, payoff_of};
pub use evaluation::{evaluate_declared_context, score_summary};
use joint_search::{joint_rec, joint_regimes, joint_root_upper, node_upper, ordered_root};
pub(crate) use progress::ProgressHook;
pub use session::{
    GOAL_SPEC_VERSION, GoalSpec, SESSION_FORMAT, SearchSession, SessionBinding, SessionProgress, SessionStatus,
    StepBudget,
};
pub(crate) use solve::solve_physical_impl;
pub use solve::{solve_physical, solve_physical_with_aggregation};

pub(crate) fn session_start_clock() -> Instant {
    super::budget::now()
}
