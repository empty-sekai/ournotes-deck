//! The goals of a request and how far the solver supports each goal and metric.

use super::*;

/// How far the solver supports a goal and metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Computed, and the search proves the teams are the best (when it completes within its time limit).
    Proven,
    /// Computed; the teams are exactly evaluated, but the search does not guarantee a proof.
    Unproven,
    Unsupported,
}

impl Support {
    pub(super) fn name(self) -> &'static str {
        match self {
            Support::Proven => "proven",
            Support::Unproven => "unproven",
            Support::Unsupported => "unsupported",
        }
    }
}

/// The goals of a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalKind {
    /// Battle live (Gekisou on, a rank in every Gekisou range).
    BattleLive,
    /// Mission live (Gekisou on, solo rank rule).
    MissionLive,
    /// Arena live (Gekisou on; missions and attribute from the arena row).
    ArenaLive,
    /// Free live (Gekisou off).
    FreeLive,
    /// Challenge live (Gekisou off; attribute and event parameters from the challenge row).
    ChallengeLive,
    Skip,
    Power,
}

impl GoalKind {
    pub(super) const ALL: [GoalKind; 7] = [
        GoalKind::BattleLive,
        GoalKind::MissionLive,
        GoalKind::ArenaLive,
        GoalKind::FreeLive,
        GoalKind::ChallengeLive,
        GoalKind::Skip,
        GoalKind::Power,
    ];

    pub fn name(self) -> &'static str {
        match self {
            GoalKind::BattleLive => "battleLive",
            GoalKind::MissionLive => "missionLive",
            GoalKind::ArenaLive => "arenaLive",
            GoalKind::FreeLive => "freeLive",
            GoalKind::ChallengeLive => "challengeLive",
            GoalKind::Skip => "skip",
            GoalKind::Power => "power",
        }
    }

    pub(super) fn live(self) -> bool {
        !matches!(self, GoalKind::Skip | GoalKind::Power)
    }

    pub(super) fn gekisou(self) -> bool {
        matches!(self, GoalKind::BattleLive | GoalKind::MissionLive | GoalKind::ArenaLive)
    }

    /// The goal fields of this kind.
    pub(super) fn fields(self) -> &'static [&'static str] {
        match self {
            GoalKind::BattleLive => &["musicId", "difficulty", "rank", "accuracy", "play"],
            GoalKind::MissionLive | GoalKind::FreeLive => &["musicId", "difficulty", "accuracy", "play"],
            GoalKind::ArenaLive => &["arenaMusicId", "difficulty", "rank", "accuracy", "play"],
            GoalKind::ChallengeLive => &["challengeMusicId", "difficulty", "accuracy", "play"],
            GoalKind::Skip => &["musicId", "challengeMusicId", "difficulty"],
            GoalKind::Power => &["musicId", "eventParameter"],
        }
    }

    /// The metric kinds of this goal; the power goal ranks by the deck power (`power`, no metric in the request).
    pub(super) fn metrics(self) -> &'static [&'static str] {
        match self {
            GoalKind::Power => &["power"],
            GoalKind::Skip => &["score", "scoreAtLeast", "cappedScore", "eventPoints", "challengePoints", "eventItems"],
            GoalKind::ChallengeLive => {
                &["score", "scoreAtLeast", "cappedScore", "scoreAndLife", "eventPoints", "eventItems"]
            }
            _ => &[
                "score",
                "scoreAtLeast",
                "cappedScore",
                "scoreAndLife",
                "eventPoints",
                "challengePoints",
                "eventItems",
            ],
        }
    }

    /// Whether the deck power of this goal reads the held events (`eventIds`): `true`, `false`, or
    /// `"eventParameter"` (when the power goal asks for event parameters).
    pub(super) fn reads_event_ids(self) -> Value {
        match self {
            GoalKind::ChallengeLive => json!(true),
            GoalKind::Skip => json!("challengeMusicId"),
            GoalKind::Power => json!("eventParameter"),
            _ => json!(false),
        }
    }
}

/// How far this solver supports a goal and one of its metric kinds.
pub fn support(kind: GoalKind, metric: &str) -> Support {
    match (kind, metric) {
        (GoalKind::Power, "power") => Support::Proven,
        (
            GoalKind::BattleLive | GoalKind::ArenaLive | GoalKind::FreeLive | GoalKind::MissionLive | GoalKind::Skip,
            "challengePoints",
        ) => Support::Proven,
        (
            GoalKind::BattleLive
            | GoalKind::ArenaLive
            | GoalKind::FreeLive
            | GoalKind::MissionLive
            | GoalKind::ChallengeLive
            | GoalKind::Skip,
            "score" | "scoreAtLeast" | "cappedScore" | "eventPoints",
        ) => Support::Proven,
        (
            GoalKind::BattleLive
            | GoalKind::ArenaLive
            | GoalKind::FreeLive
            | GoalKind::MissionLive
            | GoalKind::ChallengeLive
            | GoalKind::Skip,
            "eventItems",
        ) => Support::Proven,
        (
            GoalKind::BattleLive
            | GoalKind::ArenaLive
            | GoalKind::FreeLive
            | GoalKind::MissionLive
            | GoalKind::ChallengeLive,
            "scoreAndLife",
        ) => Support::Proven,
        _ => Support::Unsupported,
    }
}
