//! The page recommendation: an account (`ournotes.account/1`) and a request (`ournotes-deck.recommendation-request/2`)
//! in, one answer (`ournotes-deck.account-recommendation/1`) out. The request names a goal (a scene of the game, or
//! the deck power) and what to maximize; the answer ranks teams (a leader and four other members, each with a Snap)
//! by their value and reports every input problem with its JSON path. See `docs/recommendation.md`.
//!
//! A played live is valued by the mean over its 120 equally likely performance orders. Each goal and metric is
//! supported with a proof, supported without a guaranteed proof, or not supported yet ([`capabilities`]).

use crate::clock::Instant;
use crate::search::Constraints;
use crate::search::physical::ProgressHook;
use crate::types::{
    Execution, Fraction, FractionInterval, Limits, MAX_K, Metric, Optimality, PlayPolicy, RecommendationOutcome,
    RecommendationRequest, RecommendedDeck, Scene, SimulationInput, Strategy,
};
use ournotes_sim::Error;
use ournotes_sim::account::{AccountInput, Exclusions, Issue};
use ournotes_sim::data::DeckData;
use ournotes_sim::event::{EventWindow, LocalEvent, ServerEventReward};
use ournotes_sim::live::model::{Accuracy, JudgementStream, JustRule};
use ournotes_sim::scenario::{
    ContextInput, EventPayoffInput, MultiplayerRankInput, MultiplayerResultPanelInput, MultiplayerScorePolicy,
    PowerSnapshotInput, ResultClockInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

mod play;

pub const REQUEST_FORMAT: &str = "ournotes-deck.recommendation-request/2";
pub const ANSWER_FORMAT: &str = "ournotes-deck.account-recommendation/1";
/// Teams returned when the request does not say.
pub const DEFAULT_K: usize = 5;
/// The performance orders of a played live: every order of the five members, equally likely.
pub const ORDERS: usize = 120;
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
    fn name(self) -> &'static str {
        match self {
            Support::Proven => "proven",
            Support::Unproven => "unproven",
            Support::Unsupported => "unsupported",
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Request

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestWire {
    format: String,
    goal: GoalWire,
    #[serde(default)]
    metric: Option<MetricWire>,
    #[serde(default)]
    event_ids: Option<Vec<i64>>,
    #[serde(default)]
    event_context: Option<EventContextWire>,
    #[serde(default)]
    room: Option<RoomWire>,
    #[serde(default)]
    constraints: Option<ConstraintsWire>,
    #[serde(default)]
    k: Option<i64>,
    #[serde(default)]
    limits: Option<LimitsWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoalWire {
    kind: String,
    #[serde(default)]
    music_id: Option<i64>,
    #[serde(default)]
    arena_music_id: Option<i64>,
    #[serde(default)]
    challenge_music_id: Option<i64>,
    #[serde(default)]
    difficulty: Option<String>,
    #[serde(default)]
    rank: Option<i64>,
    #[serde(default)]
    accuracy: Option<AccuracyWire>,
    /// Explicit complete play. Parsed below so missing/invalid stream errors retain goal paths.
    #[serde(default)]
    play: Option<Value>,
    #[serde(default)]
    event_parameter: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccuracyWire {
    #[serde(default)]
    great_fraction: Option<f64>,
    #[serde(default)]
    just_fraction: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricWire {
    kind: String,
    #[serde(default)]
    threshold: Option<i32>,
    #[serde(default)]
    min_final_life: Option<i32>,
    #[serde(default)]
    event_id: Option<i64>,
    #[serde(default)]
    resource_type: Option<i64>,
    #[serde(default)]
    resource_id: Option<i64>,
    #[serde(default)]
    consumption: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventContextWire {
    /// Project one result's EP/CP increment. Synthetic counters are never account balances or inventory.
    #[serde(default)]
    reward_projection: bool,
    #[serde(default)]
    result_clock: Option<ResultClockWire>,
    #[serde(default)]
    local_events: Vec<LocalEvent>,
    #[serde(default)]
    event_windows: Option<Vec<EventWindow>>,
    #[serde(default)]
    selected_rewards: Option<Vec<ServerEventReward>>,
    #[serde(default)]
    multiplayer_ranks: Option<Vec<MultiplayerRankInput>>,
    #[serde(default)]
    multiplayer_result_panel: Option<MultiplayerResultPanelInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultClockWire {
    #[serde(rename = "kind", alias = "execution")]
    execution: String,
    #[serde(default, rename = "liveStartJstTicks", alias = "savedStartJstTicks")]
    saved_start_jst_ticks: Option<i64>,
    server_now_jst_ticks: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoomWire {
    players: u8,
    #[serde(default)]
    others_average_score: Option<i32>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct ConstraintsWire {
    leader: Option<i64>,
    include_members: Vec<i64>,
    exclude_members: Vec<i64>,
    exclude_snaps: Vec<i64>,
    no_snaps: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LimitsWire {
    #[serde(default)]
    time_limit_ms: Option<u64>,
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
    const ALL: [GoalKind; 7] = [
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

    fn live(self) -> bool {
        !matches!(self, GoalKind::Skip | GoalKind::Power)
    }

    fn gekisou(self) -> bool {
        matches!(self, GoalKind::BattleLive | GoalKind::MissionLive | GoalKind::ArenaLive)
    }

    /// The goal fields of this kind.
    fn fields(self) -> &'static [&'static str] {
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
    fn metrics(self) -> &'static [&'static str] {
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
    fn reads_event_ids(self) -> Value {
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

/// A request checked and mapped to the search's request.
struct Parsed {
    kind: GoalKind,
    goal: Value,
    metric: Option<Value>,
    exclusions: Exclusions,
    k: usize,
    search: RecommendationRequest,
}

struct Issues(Vec<Issue>);

impl Issues {
    fn add(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.0.push(Issue { path: path.into(), code: code.into(), message: message.into() });
    }
}

/// A fraction field of `accuracy`: in `[0, 1]`.
fn fraction(issues: &mut Issues, path: &str, value: f64) -> Option<f64> {
    if (0.0..=1.0).contains(&value) {
        Some(value)
    } else {
        issues.add(path, "input", format!("{value} is not in [0, 1]"));
        None
    }
}

/// The score id of a difficulty of a base song.
fn score_id(data: &DeckData, issues: &mut Issues, music_id: i64, difficulty: Option<&str>) -> Option<i64> {
    let row = data.master.live_music(music_id)?;
    let Some(difficulty) = difficulty else {
        issues.add("goal.difficulty", "input", "the goal needs a difficulty");
        return None;
    };
    let id = match difficulty {
        "easy" => row.easy_id,
        "normal" => row.normal_id,
        "hard" => row.hard_id,
        "expert" => row.expert_id,
        other => {
            issues.add("goal.difficulty", "input", format!("{other} is not easy, normal, hard or expert"));
            return None;
        }
    };
    if id == 0 {
        issues.add("goal.difficulty", "input", format!("live music {music_id} has no {difficulty} chart"));
        return None;
    }
    if data.data_chart(id).is_none() {
        issues.add("goal.difficulty", "input", format!("the deck data has no chart {id}"));
        return None;
    }
    Some(id)
}

/// Lexicographic index of a performance order among the 120 orders.
pub fn order_index(order: &[usize; 5]) -> usize {
    let mut index = 0;
    for i in 0..5 {
        index = index * (5 - i) + (i + 1..5).filter(|&j| order[j] < order[i]).count();
    }
    index
}

/// The performance order of a lexicographic index (inverse of [`order_index`]).
pub fn order_of_index(mut index: usize) -> [usize; 5] {
    let mut left = vec![0, 1, 2, 3, 4];
    let mut order = [0; 5];
    for (i, o) in order.iter_mut().enumerate() {
        let block = (1..5 - i).product::<usize>();
        *o = left.remove(index / block);
        index %= block;
    }
    order
}

/// The fields of a metric kind; `None` for an unknown kind.
fn metric_fields(kind: &str) -> Option<&'static [&'static str]> {
    let fields: &'static [&'static str] = match kind {
        "score" => &[],
        "scoreAtLeast" | "cappedScore" => &["threshold"],
        "scoreAndLife" => &["threshold", "minFinalLife"],
        "eventPoints" | "challengePoints" => &["eventId", "consumption"],
        "eventItems" => &["eventId", "resourceType", "resourceId", "consumption"],
        _ => return None,
    };
    Some(fields)
}

/// Validate the declared play before account resolution/search, retaining its public input path.
fn complete_stream(
    data: &DeckData,
    kind: GoalKind,
    scene_id: i64,
    score_id: i64,
    stream: &JudgementStream,
) -> Result<(), Error> {
    use ournotes_sim::{live::skip::is_judgement_note, scenario::Scenario};
    let chart = data.chart(score_id)?;
    let dc = data.data_chart(score_id).ok_or_else(|| Error::Input("chart is absent".into()))?;
    stream.to_live_play()?;
    stream.delta_times()?;
    if stream.frames.is_empty() {
        return Err(Error::Input("complete play requires nonempty frames".into()));
    }
    let expected: BTreeSet<_> = chart.notes.iter().filter(|n| is_judgement_note(n.note_type)).map(|n| n.id).collect();
    let judged: BTreeSet<_> = stream.judged.iter().map(|r| r[1]).collect();
    if expected != judged || judged.len() != stream.judged.len() {
        return Err(Error::Input("complete play must include every judged chart note exactly once, including explicit Miss results, with no unknown notes".into()));
    }
    let mut last =
        chart.notes.iter().map(|n| n.time_ms).chain(chart.skill_events.iter().map(|e| e.time_ms)).max().unwrap_or(0);
    if kind.gekisou() {
        let scene = match kind {
            GoalKind::BattleLive => Scenario::Battle(scene_id),
            GoalKind::ArenaLive => Scenario::Arena(scene_id),
            _ => Scenario::Mission(scene_id),
        };
        let setup = scene.resolve(&data.master)?.gekisou_setup(&dc.fevers);
        stream.check_just(&chart, &dc.judgement_types, &JustRule::new(&data.master, &setup)?)?;
        last = last.max(dc.fevers.iter().map(|p| p.1).max().unwrap_or(0));
    } else if stream.judged.iter().any(|r| r[2] == 6) {
        return Err(Error::Input("Gekisou off does not enable raw Just judgements".into()));
    }
    if stream.frames.last().copied().unwrap_or(-1) < last {
        return Err(Error::Input("complete clock ends before chart notes, skill events or Gekisou ranges".into()));
    }
    Ok(())
}

fn parse_request(data: &DeckData, w: RequestWire, issues: &mut Issues) -> Option<Parsed> {
    let master = &data.master;
    if w.format != REQUEST_FORMAT {
        issues.add("format", "unsupported_format", format!("expected {REQUEST_FORMAT}"));
    }
    let g = &w.goal;
    let Some(kind) = GoalKind::ALL.into_iter().find(|k| k.name() == g.kind) else {
        let names: Vec<&str> = GoalKind::ALL.iter().map(|k| k.name()).collect();
        issues.add("goal.kind", "input", format!("{} is not one of {}", g.kind, names.join(", ")));
        return None;
    };
    for (name, present) in [
        ("musicId", g.music_id.is_some()),
        ("arenaMusicId", g.arena_music_id.is_some()),
        ("challengeMusicId", g.challenge_music_id.is_some()),
        ("difficulty", g.difficulty.is_some()),
        ("rank", g.rank.is_some()),
        ("accuracy", g.accuracy.is_some()),
        ("play", g.play.is_some()),
        ("eventParameter", g.event_parameter.is_some()),
    ] {
        if present && !kind.fields().contains(&name) {
            issues.add(format!("goal.{name}"), "input", format!("{name} does not apply to {}", kind.name()));
        }
    }
    let challenge_skip = kind == GoalKind::Skip && g.challenge_music_id.is_some();
    if kind == GoalKind::Skip && g.music_id.is_some() && g.challenge_music_id.is_some() {
        issues.add("goal.challengeMusicId", "input", "skip musicId and challengeMusicId are mutually exclusive");
    }

    // The base song, and the scene id of arena and challenge lives.
    let id_of = |issues: &mut Issues, path: &str, id: Option<i64>, known: bool, what: &str| match id {
        None => {
            issues.add(path, "input", "the goal needs this field");
            None
        }
        Some(id) if !known => {
            issues.add(path, "input", format!("{what} {id} is not in the deck data"));
            None
        }
        Some(id) => Some(id),
    };
    let music = |id: Option<i64>| id.is_some_and(|id| master.live_music(id).is_some());
    let (scene_id, base) = match kind {
        GoalKind::ArenaLive => {
            let row = g.arena_music_id.and_then(|id| master.arena_music(id));
            let id = id_of(issues, "goal.arenaMusicId", g.arena_music_id, row.is_some(), "arena music");
            (id, row.map(|r| r.live_music_id))
        }
        GoalKind::ChallengeLive | GoalKind::Skip if kind == GoalKind::ChallengeLive || challenge_skip => {
            let row = g.challenge_music_id.and_then(|id| master.challenge_music(id));
            let id = id_of(issues, "goal.challengeMusicId", g.challenge_music_id, row.is_some(), "challenge music");
            (id, row.map(|r| r.live_music_id))
        }
        GoalKind::Power => match g.music_id {
            Some(id) if !music(Some(id)) => {
                issues.add("goal.musicId", "input", format!("live music {id} is not in the deck data"));
                (None, None)
            }
            id => (id, id),
        },
        _ => {
            let id = id_of(issues, "goal.musicId", g.music_id, music(g.music_id), "live music");
            (id, id)
        }
    };
    let score = match kind {
        GoalKind::Power => None,
        _ => base.and_then(|b| score_id(data, issues, b, g.difficulty.as_deref())),
    };

    // Play accuracy and rank.
    let mut explicit_play = None;
    let mut play_echo = None;
    if let Some(raw) = &g.play {
        if g.accuracy.is_some() {
            issues.add("goal.accuracy", "input", "accuracy and explicit play are mutually exclusive");
        }
        if let Some((policy, echo)) = play::parse(data, kind, scene_id, score, raw, issues) {
            explicit_play = Some(policy);
            play_echo = Some(echo);
        }
    }
    let mut accuracy = None;
    if kind.live() && g.play.is_none() {
        let given = g.accuracy.as_ref();
        let great = given.and_then(|a| a.great_fraction).unwrap_or(0.0);
        let just = given.and_then(|a| a.just_fraction).unwrap_or(if kind.gekisou() { 1.0 } else { 0.0 });
        let great = fraction(issues, "goal.accuracy.greatFraction", great);
        let just = fraction(issues, "goal.accuracy.justFraction", just);
        if !kind.gekisou() && just.is_some_and(|j| j != 0.0) {
            issues.add(
                "goal.accuracy.justFraction",
                "input",
                format!("{} has no Just judgement (Gekisou off): justFraction must be 0", kind.name()),
            );
        }
        if let (Some(great_fraction), Some(just_fraction)) = (great, just) {
            accuracy = Some(Accuracy { great_fraction, just_fraction });
        }
    }
    let rank = matches!(kind, GoalKind::BattleLive | GoalKind::ArenaLive).then(|| g.rank.unwrap_or(1));
    if let Some(r) = rank
        && r != 1
    {
        issues.add("goal.rank", "unsupported", format!("rank {r}: only rank 1 on completion is currently offered"));
    }

    if let Some(room) = &w.room {
        if !matches!(kind, GoalKind::BattleLive | GoalKind::ArenaLive) {
            issues.add("room", "input", "room applies only to battleLive or arenaLive");
        }
        if !(1..=5).contains(&room.players) {
            issues.add("room.players", "input", "room players must be in 1..=5");
        }
        if room.others_average_score.is_some_and(|score| score < 0) {
            issues.add("room.othersAverageScore", "input", "other players average score must be nonnegative");
        }
    }

    // Metric and event inputs.
    let metric_wire = w.metric.as_ref();
    let mut metric = None;
    let mut consumption = None;
    match (kind, metric_wire) {
        (GoalKind::Power, Some(_)) => issues.add("metric", "input", "the power goal takes no metric"),
        (GoalKind::Power, None) => metric = Some(Metric::Power),
        (_, None) => metric = Some(Metric::Score),
        (_, Some(m)) => match metric_fields(&m.kind) {
            None => issues.add("metric.kind", "input", format!("{} is not a metric", m.kind)),
            Some(_) if !kind.metrics().contains(&m.kind.as_str()) => {
                issues.add("metric.kind", "input", format!("{} does not apply to {}", m.kind, kind.name()))
            }
            Some(fields) => {
                let mut complete = true;
                for (name, present) in [
                    ("threshold", m.threshold.is_some()),
                    ("minFinalLife", m.min_final_life.is_some()),
                    ("eventId", m.event_id.is_some()),
                    ("resourceType", m.resource_type.is_some()),
                    ("resourceId", m.resource_id.is_some()),
                    ("consumption", m.consumption.is_some()),
                ] {
                    match (present, fields.contains(&name)) {
                        (true, false) => issues.add(
                            format!("metric.{name}"),
                            "input",
                            format!("{name} does not apply to {}", m.kind),
                        ),
                        (false, true) => {
                            complete = false;
                            issues.add(format!("metric.{name}"), "input", format!("{} needs {name}", m.kind));
                        }
                        _ => {}
                    }
                }
                if complete {
                    let t = m.threshold.unwrap_or_default();
                    let e = m.event_id.unwrap_or_default();
                    metric = Some(match m.kind.as_str() {
                        "score" => Metric::Score,
                        "scoreAtLeast" => Metric::ScoreAtLeast { threshold: t },
                        "cappedScore" => Metric::CappedScore { threshold: t },
                        "scoreAndLife" => Metric::ScoreAndLifeAtLeast {
                            threshold: t,
                            min_final_life: m.min_final_life.unwrap_or_default(),
                        },
                        "eventPoints" => Metric::ClientEventPoints { event_id: e },
                        "challengePoints" => Metric::ClientChallengePoints { event_id: e },
                        _ => Metric::ConditionalClientEventItems {
                            event_id: e,
                            resource_type: m.resource_type.unwrap_or_default(),
                            resource_id: m.resource_id.unwrap_or_default(),
                        },
                    });
                    consumption = m.consumption;
                }
            }
        },
    }
    let metric_kind = match kind {
        GoalKind::Power => "power",
        _ => metric_wire.map_or("score", |m| m.kind.as_str()),
    };
    if metric_kind == "scoreAndLife" && explicit_play.is_none() {
        issues.add("goal.play", "input", "scoreAndLife requires an explicit complete play (pattern or stream); accuracy or an omitted play cannot describe life risk");
    }
    if challenge_skip && metric_kind == "challengePoints" {
        issues.add(
            "metric.kind",
            "input",
            "challenge skip spends challenge points and cannot optimize challenge-point earnings",
        );
    }
    if kind.metrics().contains(&metric_kind) && support(kind, metric_kind) == Support::Unsupported {
        if kind.metrics().iter().all(|&m| support(kind, m) == Support::Unsupported) {
            issues.add("goal.kind", "unsupported", format!("{} is not supported yet", kind.name()));
        } else {
            issues.add("metric.kind", "unsupported", format!("{metric_kind} is not supported for {} yet", kind.name()));
        }
    }
    let event_metric = metric.as_ref().is_some_and(|m| m.event().is_some());
    let event_ids = w.event_ids.clone().unwrap_or_default();
    let mut seen = BTreeSet::new();
    for (i, id) in event_ids.iter().enumerate() {
        if master.event(*id).is_none() {
            issues.add(format!("eventIds[{i}]"), "input", format!("event {id} is not in the deck data"));
        } else if !seen.insert(*id) {
            issues.add(format!("eventIds[{i}]"), "input", format!("event {id} is listed twice"));
        }
    }
    let mut result_clock = None;
    let mut event_payoff = None;
    match (&w.event_context, event_metric) {
        (None, true) => issues.add("eventContext", "input", "event metrics need eventContext"),
        (Some(_), false) => issues.add("eventContext", "input", "eventContext applies only to event metrics"),
        (None, false) => {}
        (Some(c), true) => {
            if matches!(metric.as_ref(), Some(Metric::ConditionalClientEventItems { .. }))
                && c.selected_rewards.is_none()
            {
                issues.add(
                    "eventContext.selectedRewards",
                    "input",
                    "eventItems requires an explicit server-selected reward list; omission does not mean zero rewards",
                );
            }
            if c.reward_projection && !c.local_events.is_empty() {
                issues.add(
                    "eventContext.localEvents",
                    "input",
                    "rewardProjection and explicit localEvents are mutually exclusive",
                );
            }
            if c.reward_projection && matches!(metric.as_ref(), Some(Metric::ConditionalClientEventItems { .. })) {
                issues.add("eventContext.rewardProjection", "input", "rewardProjection applies to eventPoints and challengePoints; conditional items require their explicit server reward context");
            }
            if let Some(clock) = &c.result_clock {
                result_clock = match (clock.execution.as_str(), clock.saved_start_jst_ticks) {
                    ("played", saved) => Some(ResultClockInput::Played {
                        saved_start_jst_ticks: saved,
                        server_now_jst_ticks: clock.server_now_jst_ticks,
                    }),
                    ("skip", None) => Some(ResultClockInput::Skip { server_now_jst_ticks: clock.server_now_jst_ticks }),
                    ("skip", Some(_)) => {
                        issues.add("eventContext.resultClock.liveStartJstTicks", "input", "a skip has no live start");
                        None
                    }
                    (other, _) => {
                        issues.add("eventContext.resultClock.kind", "input", format!("{other} is not played or skip"));
                        None
                    }
                };
            }
            event_payoff = Some(EventPayoffInput {
                consumed_count: consumption.unwrap_or_default(),
                // Native per-result EP is computed before AddEventPointCount; CP increments modulo Int32.
                // Neither payoff reads the previous balance. Real balance/debit eligibility and cumulative
                // achievements are outside this explicit projection, so these counters never escape the solver.
                local_events: if c.reward_projection {
                    master
                        .events
                        .iter()
                        .map(|event| LocalEvent { event_id: event.id, ..LocalEvent::default() })
                        .collect()
                } else {
                    c.local_events.clone()
                },
                event_window_adapter: c.event_windows.is_none().then(|| "canonical-master-no-offset".to_string()),
                event_windows: c.event_windows.clone(),
                selected_rewards: c.selected_rewards.clone(),
                multiplayer_ranks: c.multiplayer_ranks.clone(),
                multiplayer_result_panel: c.multiplayer_result_panel.clone(),
                multiplayer_score_policy: w.room.as_ref().map(|room| match room.others_average_score {
                    Some(score) => {
                        MultiplayerScorePolicy::FixedOthersAverage { players: i64::from(room.players), score }
                    }
                    None => MultiplayerScorePolicy::SameScore { players: i64::from(room.players) },
                }),
            });
        }
    }

    // Constraints, k and limits.
    let c = w.constraints.unwrap_or_default();
    if let Some(leader) = c.leader
        && c.exclude_members.contains(&leader)
    {
        issues.add("constraints.leader", "input", format!("member card {leader} is also excluded"));
    }
    for (i, id) in c.include_members.iter().enumerate() {
        if c.exclude_members.contains(id) {
            issues.add(
                format!("constraints.includeMembers[{i}]"),
                "input",
                format!("member card {id} is also excluded"),
            );
        }
    }
    let k = w.k.unwrap_or(DEFAULT_K as i64);
    if !(1..=MAX_K as i64).contains(&k) {
        issues.add("k", "input", format!("k must be in 1..={MAX_K}"));
    }
    let time_limit_ms = w.limits.and_then(|l| l.time_limit_ms);

    let (Some(metric), Some(scene_id)) = (metric, scene_id.or(if kind == GoalKind::Power { Some(0) } else { None }))
    else {
        return None;
    };
    if kind.live() && accuracy.is_none() && explicit_play.is_none() {
        return None;
    }
    if kind != GoalKind::Power && score.is_none() {
        return None;
    }
    let base_id = base.unwrap_or_default();
    let play = explicit_play.unwrap_or_else(|| match accuracy {
        Some(a) if a == Accuracy { great_fraction: 0.0, just_fraction: if kind.gekisou() { 1.0 } else { 0.0 } } => {
            PlayPolicy::TheoreticalBest
        }
        Some(a) => PlayPolicy::Accuracy(a),
        None => PlayPolicy::TheoreticalBest,
    });
    let (execution, scenario) = match kind {
        GoalKind::Power => {
            (Execution::Power { music_id: base, event_parameter: g.event_parameter.unwrap_or(false) }, None)
        }
        GoalKind::Skip => (
            Execution::Skip { score_id: score.expect("checked") },
            Some(if challenge_skip {
                Scene::Challenge { music_id: scene_id }
            } else {
                Scene::Free { music_id: base_id }
            }),
        ),
        _ => {
            let scene = match kind {
                GoalKind::BattleLive => Scene::Battle { music_id: scene_id },
                GoalKind::MissionLive => Scene::Mission { music_id: scene_id },
                GoalKind::ArenaLive => Scene::Arena { music_id: scene_id },
                GoalKind::FreeLive => Scene::Free { music_id: scene_id },
                _ => Scene::Challenge { music_id: scene_id },
            };
            (Execution::Live { score_id: score.expect("checked"), gekisou: kind.gekisou(), play }, Some(scene))
        }
    };

    let mut goal = json!({"kind": kind.name()});
    match kind {
        GoalKind::Power => {
            goal["musicId"] = json!(base);
            goal["eventParameter"] = json!(g.event_parameter.unwrap_or(false));
        }
        _ => {
            goal["musicId"] = json!(base_id);
            match kind {
                GoalKind::ArenaLive => goal["arenaMusicId"] = json!(scene_id),
                GoalKind::ChallengeLive => goal["challengeMusicId"] = json!(scene_id),
                GoalKind::Skip if challenge_skip => goal["challengeMusicId"] = json!(scene_id),
                _ => {}
            }
            goal["difficulty"] = json!(g.difficulty);
            goal["scoreId"] = json!(score);
            if let Some(r) = rank {
                goal["rank"] = json!(r);
            }
            if let Some(a) = accuracy {
                goal["accuracy"] = json!({"greatFraction": a.great_fraction, "justFraction": a.just_fraction});
            }
            if let Some(echo) = play_echo {
                goal["play"] = echo;
            }
        }
    }
    let metric_echo = (kind != GoalKind::Power).then(|| {
        let mut echo = json!({"kind": metric_wire.map_or("score", |m| m.kind.as_str())});
        if let Some(m) = metric_wire {
            for (name, value) in [
                ("threshold", m.threshold.map(i64::from)),
                ("minFinalLife", m.min_final_life.map(i64::from)),
                ("eventId", m.event_id),
                ("resourceType", m.resource_type),
                ("resourceId", m.resource_id),
                ("consumption", m.consumption.map(i64::from)),
            ] {
                if let Some(v) = value {
                    echo[name] = json!(v);
                }
            }
        }
        if w.event_context.as_ref().is_some_and(|c| c.reward_projection) {
            echo["rewardProjection"] = json!(true);
        }
        echo
    });

    let k = k.clamp(1, MAX_K as i64) as usize;
    let network_confirmations = if let Some(rank) = rank {
        let scene = scenario.as_ref()?.scenario().resolve(master).ok()?;
        let factors = match ournotes_sim::live::full::gekisou_rank_factors(master, &scene.gekisou_missions) {
            Ok(factors) => factors,
            Err(error) => {
                issues.add("goal.rank", "master", error.to_string());
                return None;
            }
        };
        let chart = data.data_chart(score?)?;
        if chart.fevers.len() > 3 {
            issues.add("goal", "game", "native network ranking has at most three Gekisou ranges");
            return None;
        }
        goal["rankConfirmation"] = json!("onCompletion");
        Some(
            (0..chart.fevers.len())
                .map(|range| ournotes_sim::replay::RankConfirmation {
                    frame: 0,
                    range,
                    rank: rank.clamp(1, 5) as i32,
                    percent: factors[range][rank.clamp(1, 5) as usize - 1],
                })
                .collect(),
        )
    } else {
        None
    };
    let search = RecommendationRequest {
        format: crate::types::REQUEST_FORMAT.into(),
        execution,
        scenario,
        context: Some(ContextInput {
            power_snapshot: PowerSnapshotInput { event_ids, captured_jst_ticks: None },
            result_clock,
            event_payoff,
        }),
        metric,
        goal: None,
        constraints: Constraints {
            leader: c.leader,
            include_members: c.include_members,
            exclude_members: Vec::new(),
            exclude_snaps: Vec::new(),
            no_snaps: c.no_snaps,
        },
        k,
        strategy: Strategy::default(),
        limits: Limits { time_limit_ms, max_candidates: None, cache_entries: Limits::default().cache_entries },
        network_confirmations,
        simulation: SimulationInput::default(),
        initial_decks: Vec::new(),
    };
    Some(Parsed {
        kind,
        goal,
        metric: metric_echo,
        exclusions: Exclusions { members: c.exclude_members, snaps: c.exclude_snaps },
        k,
        search,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Answer

/// How far a recommendation got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// `result` holds the recommendation (progress reports too).
    Ok,
    /// The account lacks facts the goal reads (`missing`); nothing else is wrong.
    Incomplete,
    /// The account or the request is invalid (`errors`, possibly with `missing`).
    Invalid,
    /// The inputs are valid but the computation is unsupported or failed (`errors`).
    Failed,
}

/// The answer to one recommendation, final or a progress report.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub format: &'static str,
    /// The lowercase hex SHA-256 of the deck data text, which an account's `datasetId` must name.
    pub dataset_id: Option<String>,
    /// False for progress reports.
    #[serde(rename = "final")]
    pub is_final: bool,
    pub status: Status,
    pub missing: Vec<Issue>,
    pub errors: Vec<Issue>,
    pub result: Option<AnswerResult>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    /// The goal as computed, with its defaults and the chart's score id.
    pub goal: Value,
    /// The metric as computed (null for the power goal).
    pub metric: Option<Value>,
    /// `search` in progress reports, `done` in the final answer.
    pub phase: &'static str,
    pub elapsed_ms: f64,
    pub optimality: OptimalityReport,
    /// Best first.
    pub teams: Vec<Team>,
    /// The account's scope.
    pub account: Value,
    /// Diagnostics of the search, with `memory`.
    pub telemetry: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimalityReport {
    /// Whether the teams are proven to be the best `k`.
    pub proven: bool,
    /// The best team's value (rounded down; a probability for probability metrics).
    pub lower_bound: Option<Value>,
    /// A value no team can exceed (rounded up; a probability for probability metrics), when known.
    pub upper_bound: Option<Value>,
    pub best_gap: Option<f64>,
    pub kth_gap: Option<f64>,
    /// Share of the search tree decided, for a progress bar.
    pub fraction: Option<f64>,
}

/// A member card and its Snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Pair {
    pub member: i64,
    pub snap: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub rank: usize,
    pub leader: Pair,
    /// The other four, in ascending member card id order; where they sit does not change the value.
    pub others: [Pair; 4],
    pub power: i32,
    /// Null for the power goal.
    pub value: Option<TeamValue>,
    /// A played live's order results; null for skip, power and progress reports.
    pub orders: Option<Orders>,
    /// The canonical layout: the leader in slot 2, the others in slots 0, 1, 3, 4.
    pub layout: Layout,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank_certified: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamValue {
    /// The expected score, rounded down.
    pub score: i64,
    pub exact: Option<Fraction>,
    /// Bounds of the expected score when it is only known within them.
    pub interval: Option<Interval>,
    /// The expected metric value when the metric is not the score.
    pub payoff: Option<PayoffValue>,
}

pub type Interval = FractionInterval;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayoffValue {
    pub score: f64,
    pub exact: Option<Fraction>,
    pub interval: Option<Interval>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Orders {
    pub count: usize,
    pub min: OrderPoint,
    /// The 60th smallest score (lower median), a real order.
    pub median: OrderPoint,
    pub max: OrderPoint,
    /// The order with the highest metric value (then score).
    pub best: BestOrder,
    /// The score of each order, by lexicographic order index over the layout's slots.
    pub values: Vec<i64>,
    /// The metric value of each order when the metric is not the score.
    pub payoff_values: Option<Vec<i64>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderPoint {
    pub score: i64,
    /// Member card ids in performance order.
    pub order: [i64; 5],
}

#[derive(Clone, Debug, Serialize)]
pub struct BestOrder {
    pub score: i64,
    pub payoff: Option<i64>,
    pub order: [i64; 5],
}

#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
}

/// Progress reports of [`crate::engine::recommend_account`]: each is a complete answer (`final: false`).
pub struct AnswerProgress<'a> {
    pub interval: Duration,
    pub report: &'a mut dyn FnMut(&Answer),
}

fn exact(f: &Fraction) -> Result<(i128, u128), Error> {
    let bad = || Error::Domain("result fraction".into());
    let numerator = f.numerator.parse().map_err(|_| bad())?;
    let denominator = f.denominator.parse().map_err(|_| bad())?;
    if denominator == 0 {
        return Err(bad());
    }
    Ok((numerator, denominator))
}

/// Exact rounding for a positive denominator, including denominators above i128::MAX.
fn floor_div(n: i128, d: u128) -> i128 {
    match i128::try_from(d) {
        Ok(d) => n.div_euclid(d),
        Err(_) => {
            if n < 0 {
                -1
            } else {
                0
            }
        }
    }
}

fn ceil_div(n: i128, d: u128) -> i128 {
    floor_div(n, d) + i128::from(!n.unsigned_abs().is_multiple_of(d))
}

/// Whether a metric's value is a probability.
fn probability(metric: &Metric) -> bool {
    matches!(metric, Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. })
}

fn bound(metric: &Metric, n: i128, d: u128, up: bool) -> Value {
    if probability(metric) {
        use ournotes_sim::live::certified::F64Interval;
        if n == 0 {
            return json!(0.0);
        }
        if n >= 0 && n as u128 == d {
            return json!(1.0);
        }
        let denominator = match i128::try_from(d) {
            Ok(d) => F64Interval::integer(d),
            // Both integer parts fit i128, and the interval operations preserve the full unsigned value.
            Err(_) => {
                F64Interval::integer((d >> 1) as i128).scale_integer(2).add(F64Interval::integer((d & 1) as i128))
            }
        };
        let value = F64Interval::integer(n).divide(denominator).expect("positive result denominator");
        json!(if up { value.upper() } else { value.lower() })
    } else if up {
        json!(ceil_div(n, d))
    } else {
        json!(floor_div(n, d))
    }
}

/// A team from a searched deck: its key, layout and the map from the deck's slots to the layout's.
fn canonical(deck: &RecommendedDeck) -> (Pair, [Pair; 4], [usize; 5]) {
    let pair = |s: usize| Pair { member: deck.members[s], snap: deck.snaps[s] };
    let mut others: Vec<(Pair, usize)> = [0, 1, 3, 4].iter().map(|&s| (pair(s), s)).collect();
    others.sort();
    let mut slot = [2; 5];
    for (i, &(_, s)) in others.iter().enumerate() {
        slot[s] = [0, 1, 3, 4][i];
    }
    (pair(2), [others[0].0, others[1].0, others[2].0, others[3].0], slot)
}

fn orders(deck: &RecommendedDeck, slot: &[usize; 5], layout: &Layout, score_metric: bool) -> Result<Orders, Error> {
    let bad = || Error::Domain("a played live result lacks a performance order".into());
    let mut values = [None; ORDERS];
    for &(order, score, payoff) in &deck.order_outcomes {
        let order = order.map(|s| slot[s]);
        let payoff = i64::try_from(payoff).map_err(|_| bad())?;
        let at = &mut values[order_index(&order)];
        if at.is_some() {
            return Err(bad());
        }
        *at = Some((i64::from(score), payoff));
    }
    let values: Vec<(i64, i64)> = values.iter().map(|v| v.ok_or_else(bad)).collect::<Result<_, _>>()?;
    let members = |i: usize| order_of_index(i).map(|s| layout.members[s]);
    let point = |i: usize| OrderPoint { score: values[i].0, order: members(i) };
    let mut by_score: Vec<usize> = (0..ORDERS).collect();
    by_score.sort_by_key(|&i| (values[i].0, i));
    let max = (0..ORDERS).max_by_key(|&i| (values[i].0, std::cmp::Reverse(i))).expect("orders");
    let best = (0..ORDERS).max_by_key(|&i| (values[i].1, values[i].0, std::cmp::Reverse(i))).expect("orders");
    Ok(Orders {
        count: ORDERS,
        min: point(by_score[0]),
        median: point(by_score[ORDERS / 2 - 1]),
        max: point(max),
        best: BestOrder {
            score: values[best].0,
            payoff: (!score_metric).then_some(values[best].1),
            order: members(best),
        },
        values: values.iter().map(|v| v.0).collect(),
        payoff_values: (!score_metric).then(|| values.iter().map(|v| v.1).collect()),
    })
}

/// The teams of a search outcome in their canonical layout, best first, at most `k`.
fn teams(outcome: &RecommendationOutcome, parsed: &Parsed, with_orders: bool) -> Result<Vec<Team>, Error> {
    let metric = &parsed.search.metric;
    let score_metric = matches!(metric, Metric::Score);
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for deck in &outcome.results {
        let (leader, others, slot) = canonical(deck);
        if !seen.insert((leader, others)) {
            continue;
        }
        let mut layout = Layout { members: [0; 5], snaps: [None; 5] };
        for s in 0..5 {
            layout.members[slot[s]] = deck.members[s];
            layout.snaps[slot[s]] = deck.snaps[s];
        }
        let value = if parsed.kind == GoalKind::Power {
            None
        } else {
            let score = match (&deck.expected_score, &deck.score_interval) {
                (Some(score), _) => {
                    let (n, d) = exact(score)?;
                    Some(i64::try_from(floor_div(n, d)).map_err(|_| Error::Domain("result score exceeds i64".into()))?)
                }
                (None, Some(interval)) => Some(interval.lower_f64().floor() as i64),
                (None, None) => None,
            };
            let payoff = if score_metric {
                None
            } else {
                let value = match (&deck.expected_payoff, &deck.payoff_interval) {
                    (Some(payoff), _) => {
                        let (n, d) = exact(payoff)?;
                        Some(n as f64 / d as f64)
                    }
                    (None, Some(interval)) => Some(interval.lower_f64()),
                    (None, None) => None,
                };
                value.map(|score| PayoffValue {
                    score,
                    exact: deck.expected_payoff.clone(),
                    interval: deck.payoff_interval.clone(),
                })
            };
            score.map(|score| TeamValue {
                score,
                exact: deck.expected_score.clone(),
                interval: deck.score_interval.clone(),
                payoff,
            })
        };
        let orders = if with_orders && parsed.kind.live() && deck.order_outcomes.len() == ORDERS {
            Some(orders(deck, &slot, &layout, score_metric)?)
        } else {
            None
        };
        rows.push(Team {
            rank: 0,
            leader,
            others,
            power: deck.power,
            value,
            orders,
            layout,
            rank_certified: deck.rank_certified,
        });
    }
    // Preserve the search ordering and its proof. Changing the tie objective here would not recover
    // teams already pruned by the solver.
    rows.truncate(parsed.k);
    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(i, mut team)| {
            team.rank = i + 1;
            team
        })
        .collect())
}

/// The memory of the solver: on WebAssembly the linear memory, which never shrinks, so its size is also the peak.
fn memory() -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = core::arch::wasm32::memory_size(0) as u64 * 65536;
        json!({"currentBytes": bytes, "peakBytes": bytes})
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        json!({"currentBytes": null, "peakBytes": null})
    }
}

fn result_of(
    outcome: &RecommendationOutcome,
    parsed: &Parsed,
    scope: &Value,
    start: Instant,
    is_final: bool,
) -> Result<AnswerResult, Error> {
    let teams = teams(outcome, parsed, is_final)?;
    let metric = &parsed.search.metric;
    let live = parsed.kind.live();
    let proof = &outcome.telemetry.proof;
    let proven = outcome.optimality == Optimality::Proven;
    let best_deck = outcome.results.first();
    let best = best_deck.and_then(|d| d.expected_payoff.as_ref()).map(exact).transpose()?;
    let lower_bound = best.map(|(n, d)| bound(metric, n, d, false)).or_else(|| {
        best_deck
            .and_then(|d| d.payoff_interval.as_ref())
            .map(|v| if probability(metric) { json!(v.lower_f64()) } else { json!(v.lower_f64().floor() as i64) })
    });
    let upper_bound = if proven {
        best.map(|(n, d)| bound(metric, n, d, true)).or_else(|| {
            best_deck
                .and_then(|d| d.payoff_interval.as_ref())
                .map(|v| if probability(metric) { json!(v.upper_f64()) } else { json!(v.upper_f64().ceil() as i64) })
        })
    } else {
        // Payoff numerators of the proof are over the target's denominator.
        // upper_bound covers only unexplored branches. It cannot replace a missing global bound: retained
        // interval candidates can exceed it, and a displayed Top-K does not certify the rest of that frontier.
        let target = outcome.telemetry.environment.target.as_ref().and_then(|t| t.denominator.parse::<u128>().ok());
        let denominator = target.unwrap_or(if live { ORDERS as u128 } else { 1 });
        proof
            .global_upper_bound
            .as_deref()
            .and_then(|u| u.parse::<i128>().ok())
            .map(|n| bound(metric, n, denominator, true))
    };
    let mut telemetry = outcome.telemetry.clone();
    if !is_final {
        telemetry.incumbents.timeline.clear();
    }
    let mut telemetry = serde_json::to_value(&telemetry).map_err(|e| Error::Domain(format!("telemetry: {e}")))?;
    telemetry["memory"] = memory();
    let mut account = scope.clone();
    account["goal"] = json!(parsed.kind.name());
    Ok(AnswerResult {
        goal: parsed.goal.clone(),
        metric: parsed.metric.clone(),
        phase: if is_final { "done" } else { "search" },
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        optimality: OptimalityReport {
            proven,
            lower_bound,
            upper_bound,
            best_gap: if proven { Some(0.0) } else { proof.best_gap },
            kth_gap: if proven { Some(0.0) } else { proof.kth_gap },
            fraction: proof.fraction,
        },
        teams,
        account,
        telemetry,
    })
}

fn issue(path: &str, error: Error) -> Issue {
    let (code, message) = match error {
        Error::Master(m) => ("master", m),
        Error::Input(m) => ("input", m),
        Error::Game(m) => ("game", m),
        Error::Unsupported(m) => ("unsupported", m),
        Error::Domain(m) => ("domain", m),
        Error::Capacity(m) => ("capacity", m),
    };
    Issue { path: path.into(), code: code.into(), message }
}

/// The status of an answer with these issues: computation-side errors (`unsupported`, `master`, `game`, `domain`,
/// `capacity`) alone fail it, any other error makes it invalid, missing facts alone make it incomplete.
fn status_of(missing: &[Issue], errors: &[Issue]) -> Status {
    let computation = |i: &Issue| matches!(i.code.as_str(), "unsupported" | "master" | "game" | "domain" | "capacity");
    if errors.is_empty() {
        if missing.is_empty() { Status::Ok } else { Status::Incomplete }
    } else if errors.iter().all(computation) {
        Status::Failed
    } else {
        Status::Invalid
    }
}

/// See [`crate::engine::recommend_account`].
pub(crate) fn recommend(
    data: &DeckData,
    account_json: &str,
    request_json: &str,
    progress: Option<AnswerProgress<'_>>,
) -> Answer {
    recommend_started(data, account_json, request_json, progress, Instant::now())
}

/// Keep one origin through parsing, resolution, problem construction and search. A supplied origin is private to
/// this module; tests use it to represent elapsed request work without sleeping or exposing a clock override.
fn recommend_started(
    data: &DeckData,
    account_json: &str,
    request_json: &str,
    progress: Option<AnswerProgress<'_>>,
    start: Instant,
) -> Answer {
    let mut answer = Answer {
        format: ANSWER_FORMAT,
        dataset_id: data.sha256.clone(),
        is_final: true,
        status: Status::Invalid,
        missing: Vec::new(),
        errors: Vec::new(),
        result: None,
    };
    let mut issues = Issues(Vec::new());
    let wire = serde_json::from_str::<RequestWire>(request_json)
        .map_err(|e| issues.add("request", "parse", e.to_string()))
        .ok();
    let account = AccountInput::from_json(account_json)
        .map_err(|e| issues.add("account", "parse", e.to_string().trim_start_matches("input: ").to_string()))
        .ok();
    let parsed = wire.and_then(|w| parse_request(data, w, &mut issues));
    // The account is resolved for the goal's facts whenever the goal is known.
    let resolved = match (&parsed, &account) {
        (Some(p), Some(account)) => {
            let resolution = account.resolve(data, crate::account::goal_of(&p.search.execution), &p.exclusions);
            answer.missing = resolution.missing;
            issues.0.extend(resolution.errors);
            resolution.resolved
        }
        _ => None,
    };
    answer.errors = issues.0;
    answer.status = status_of(&answer.missing, &answer.errors);
    let (Some(parsed), Some(resolved), Status::Ok) = (parsed, resolved, answer.status) else { return answer };

    let scope = resolved.scope();
    let dataset_id = answer.dataset_id.clone();
    let mut forward;
    let hook = match progress {
        Some(AnswerProgress { interval, report }) => {
            let parsed = &parsed;
            let scope = &scope;
            forward = move |out: RecommendationOutcome| {
                if let Ok(result) = result_of(&out, parsed, scope, start, false) {
                    report(&Answer {
                        format: ANSWER_FORMAT,
                        dataset_id: dataset_id.clone(),
                        is_final: false,
                        status: Status::Ok,
                        missing: Vec::new(),
                        errors: Vec::new(),
                        result: Some(result),
                    });
                }
            };
            Some(ProgressHook { interval, report: &mut forward })
        }
        None => None,
    };
    let outcome = crate::engine::recommend_hooked_started(data, resolved.roster(), &parsed.search, hook, start)
        .and_then(|outcome| result_of(&outcome, &parsed, &scope, start, true));
    match outcome {
        Ok(result) => answer.result = Some(result),
        Err(error) => {
            answer.errors.push(issue("request", error));
            answer.status = status_of(&answer.missing, &answer.errors);
        }
    }
    answer
}

/// What this solver computes, for a page to enable its controls. See `docs/recommendation.md`.
pub fn capabilities() -> Value {
    let goals: Vec<GoalKind> = GoalKind::ALL
        .into_iter()
        .filter(|k| k.metrics().iter().any(|m| support(*k, m) != Support::Unsupported))
        .collect();
    let mut metrics = serde_json::Map::new();
    let mut event_ids = serde_json::Map::new();
    for kind in &goals {
        metrics.insert(
            kind.name().into(),
            json!(kind.metrics().iter().filter(|m| support(*kind, m) != Support::Unsupported).collect::<Vec<_>>()),
        );
        event_ids.insert(kind.name().into(), kind.reads_event_ids());
    }
    let support_by_goal: std::collections::BTreeMap<_, _> = GoalKind::ALL
        .iter()
        .map(|kind| {
            let metrics: std::collections::BTreeMap<_, _> =
                kind.metrics().iter().map(|metric| (*metric, support(*kind, metric).name())).collect();
            (kind.name(), metrics)
        })
        .collect();
    json!({
        "requestFormat": REQUEST_FORMAT,
        "answerFormat": ANSWER_FORMAT,
        "accountFormat": ournotes_sim::account::FORMAT,
        "goals": goals.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "metrics": metrics,
        "accuracy": {"great": true, "just": true},
        "play": {"completeStream":true,"field":"goal.play","mutuallyExclusiveWith":"goal.accuracy",
            "judgements":"NoteSimulateJudgement: 1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just"},
        "patternPlay": {"field":"goal.play","kind":"pattern",
            "required":["greatFraction","justFraction","missEvery"],
            "goals":goals.iter().filter(|k| k.live()).map(|k| k.name()).collect::<Vec<_>>(),
            "fractions":{"minimum":0,"maximum":1,"justRequiresGekisou":true},
            "missEvery":{"minimum":0,"integer":true,"zero":"no Miss",
                "positive":"every Nth judged note, in native stream order (chart time, note id)"},
            "accuracyBeforeMisses":true,"completeStream":true,"survivalProbability":false},
        "scoreAndLife": {"requiresCompleteJudgementStream":true,"survivalProbability":false},
        "skip": {"musicIdOrChallengeMusicId":true,"mutuallyExclusive":true,
            "challengeMetrics":["score","scoreAtLeast","cappedScore","eventPoints","eventItems"]},
        "ranks": [1],
        "rankConfirmation": "onCompletion",
        "rankAssumption": "player-declared rank 1 for each completed Battle/Arena range; native network frame snapshots",
        "eventIds": event_ids,
        "luckMissions": true,
        "provenGoals": goals.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "support": support_by_goal,
        "memoryBudgetBytes": null,
        "lottery": "certified expectations; only proved rank separation or equality certifies TopK",
        "accuracyLaw": "deterministic evenly spread Greats; Just share of remaining eligible notes",
        "tieBreak": ["expectedPayoff", "power", "canonicalTeamKey"],
        "eventItemsRequireSelectedRewards": true,
        "eventRewardProjection": {
            "field": "eventContext.rewardProjection", "metrics": ["eventPoints", "challengePoints"],
            "balanceValidation": false, "cumulativeRewards": false,
        },
        "eventMusicRanking": {
            "goal": "challengeLive", "metric": "score", "key": "challengeMusicId",
            "record": "maximum solo score across difficulties",
            "optimization": "expected solo score for the selected challenge song and difficulty",
            "serverRankPrediction": false,
        },
    })
}

#[cfg(test)]
#[path = "../tests/fixtures/request_budget.rs"]
mod request_budget;

#[cfg(test)]
#[path = "../tests/fixtures/account_bounds.rs"]
mod account_bounds;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fraction_rounding_covers_signed_and_unsigned_limits() {
        let half_unsigned = 1u128 << 127;
        for (n, d, lower, upper) in [
            (7, 3, 2, 3),
            (-7, 3, -3, -2),
            (i128::MIN, 1, i128::MIN, i128::MIN),
            (i128::MAX, 1, i128::MAX, i128::MAX),
            (i128::MIN, i128::MAX as u128, -2, -1),
            (i128::MAX, i128::MAX as u128, 1, 1),
            (i128::MIN, half_unsigned, -1, -1),
            (i128::MAX, half_unsigned, 0, 1),
            (1, half_unsigned, 0, 1),
            (-1, half_unsigned, -1, 0),
            (i128::MIN, u128::MAX, -1, 0),
            (i128::MAX, u128::MAX, 0, 1),
            (0, u128::MAX, 0, 0),
        ] {
            assert_eq!(floor_div(n, d), lower, "floor of {n}/{d}");
            assert_eq!(ceil_div(n, d), upper, "ceiling of {n}/{d}");
        }
        assert_eq!(
            exact(&Fraction { numerator: i128::MIN.to_string(), denominator: u128::MAX.to_string() }).unwrap(),
            (i128::MIN, u128::MAX)
        );
        assert!(exact(&Fraction { numerator: "1".into(), denominator: "0".into() }).is_err());
    }

    #[test]
    fn integer_result_bounds_preserve_exact_rounding_at_fraction_limits() {
        let denominator = 1u128 << 127;
        for (numerator, lower, upper) in [(1, 0, 1), (-1, -1, 0)] {
            assert_eq!(bound(&Metric::Score, numerator, denominator, false), json!(lower));
            assert_eq!(bound(&Metric::Score, numerator, denominator, true), json!(upper));
        }
        assert_eq!(bound(&Metric::Score, i128::MIN, 1, true), json!(i128::MIN));
        assert_eq!(bound(&Metric::Score, i128::MAX, 1, false), json!(i128::MAX));
    }

    #[test]
    fn probability_result_bounds_enclose_exact_rationals() {
        use crate::search::{expectation::ExactExpectation, interval_topk::exact_in_interval};
        use ournotes_sim::live::certified::F64Interval;
        for metric in
            [Metric::ScoreAtLeast { threshold: 1 }, Metric::ScoreAndLifeAtLeast { threshold: 1, min_final_life: 1 }]
        {
            for (n, d) in [
                (1, 3),
                (1, 10),
                (1, 2),
                (0, 1),
                (1, 1),
                (0, u128::MAX),
                (i128::MAX, i128::MAX as u128),
                (1, 1u128 << 127),
                (1, u128::MAX),
                (i128::MAX, 1u128 << 127),
                (i128::MAX, u128::MAX),
                (i128::MAX - 1, u128::MAX),
            ] {
                let lower = bound(&metric, n, d, false).as_f64().unwrap();
                let upper = bound(&metric, n, d, true).as_f64().unwrap();
                assert!(
                    exact_in_interval(
                        ExactExpectation { numerator: n, denominator: d },
                        F64Interval::new(lower, upper).unwrap()
                    )
                    .unwrap(),
                    "{n}/{d} outside [{lower}, {upper}]"
                );
                if n == 0 || n as u128 == d {
                    assert_eq!(lower, upper);
                }
            }
        }
        let metric = Metric::ScoreAtLeast { threshold: 1 };
        assert!(bound(&metric, 1, 3, true).as_f64().unwrap() > 1.0 / 3.0);
        assert!(bound(&metric, 1, 10, false).as_f64().unwrap() < 0.1);
    }

    #[test]
    fn uniform_lottery_average_keeps_an_unsigned_result_denominator() {
        use crate::search::{
            certified_search::{OrderScoreInterval, PayoffMap, aggregate_orders},
            expectation::ExactExpectation,
            uniform,
        };
        use ournotes_sim::live::certified::F64Interval;
        let orders = uniform::all_orders()
            .into_iter()
            .enumerate()
            .map(|(index, order)| OrderScoreInterval {
                order,
                mean: F64Interval::point(if index == 0 { 2f64.powi(-121) } else { 0.0 }).unwrap(),
                support: (0, 1),
                exact_mean: Some(ExactExpectation {
                    numerator: i128::from(index == 0),
                    denominator: if index == 0 { 1u128 << 121 } else { 1 },
                }),
                final_life: None,
                tails: Default::default(),
                refined_payoff: None,
            })
            .collect();
        let evaluation = aggregate_orders(orders, &PayoffMap::Score).unwrap();
        let fraction = evaluation.exact_score.unwrap();
        assert_eq!(fraction.numerator, 1);
        assert_eq!(fraction.denominator, 120u128 << 121);
        assert!(fraction.denominator > i128::MAX as u128);
        assert_eq!(bound(&Metric::Score, fraction.numerator, fraction.denominator, false), json!(0));
        assert_eq!(bound(&Metric::Score, fraction.numerator, fraction.denominator, true), json!(1));
    }

    #[test]
    fn certified_team_keeps_rational_bounds_without_fabricating_exact_values_or_orders() {
        let search: RecommendationRequest = serde_json::from_value(json!({
            "format":crate::types::REQUEST_FORMAT,"execution":{"kind":"power"},"metric":{"kind":"score"}
        }))
        .unwrap();
        let parsed = Parsed {
            kind: GoalKind::MissionLive,
            goal: Value::Null,
            metric: None,
            exclusions: Exclusions::default(),
            k: 5,
            search,
        };
        let outcome = RecommendationOutcome {
            format: crate::types::RESULT_FORMAT,
            completion: crate::search::Completion::Complete,
            optimality: Optimality::Proven,
            exit_reason: crate::types::ExitReason::Exhausted,
            result_identity: "team",
            metric: Metric::Score,
            player_goal: None,
            strategy: Strategy::Exhaustive,
            probability_law: Value::Null,
            proof_scope: "fixture",
            resolved_context: Value::Null,
            telemetry: Default::default(),
            elapsed_ms: 0.0,
            results: vec![RecommendedDeck {
                members: [1, 2, 3, 4, 5],
                snaps: [None; 5],
                power: 100,
                expected_score: None,
                expected_payoff: None,
                score_interval: Some(FractionInterval::from_f64(150.5, 150.75).unwrap()),
                payoff_interval: Some(FractionInterval::from_f64(150.5, 150.75).unwrap()),
                rank_certified: Some(true),
                score_summary: None,
                best_order: None,
                order_outcomes: vec![],
            }],
        };
        let rows = teams(&outcome, &parsed, true).unwrap();
        let wire = serde_json::to_value(&rows[0]).unwrap();
        assert!(wire["value"]["exact"].is_null());
        assert!(wire["orders"].is_null());
        assert_eq!(wire["rankCertified"], true);
        assert_eq!(wire["value"]["interval"]["lower"]["numerator"], "301");
        assert_eq!(wire["value"]["interval"]["lower"]["denominator"], "2");
        assert_eq!(wire["value"]["score"], 150);
    }

    #[test]
    fn every_order_index_round_trips() {
        let mut seen = BTreeSet::new();
        for i in 0..ORDERS {
            let order = order_of_index(i);
            assert_eq!(order_index(&order), i);
            assert!(seen.insert(order));
        }
    }

    #[test]
    fn challenge_points_and_song_ranking_have_distinct_native_payoffs() {
        let value = capabilities();
        for kind in
            [GoalKind::FreeLive, GoalKind::MissionLive, GoalKind::BattleLive, GoalKind::ArenaLive, GoalKind::Skip]
        {
            assert!(kind.metrics().contains(&"challengePoints"));
            assert_eq!(support(kind, "challengePoints"), Support::Proven);
        }
        assert!(!GoalKind::ChallengeLive.metrics().contains(&"challengePoints"));
        assert_eq!(metric_fields("challengePoints"), Some(["eventId", "consumption"].as_slice()));
        assert_eq!(value["eventMusicRanking"]["goal"], "challengeLive");
        assert_eq!(value["eventMusicRanking"]["metric"], "score");
        assert_eq!(value["eventMusicRanking"]["serverRankPrediction"], false);
    }

    #[test]
    fn frontend_result_clock_and_room_are_accepted_without_rounding_ticks() {
        let raw = r#"{"format":"ournotes-deck.recommendation-request/2","goal":{"kind":"battleLive","musicId":10,"difficulty":"expert","rank":1},"metric":{"kind":"eventPoints","eventId":7,"consumption":1},"room":{"players":5,"othersAverageScore":null},"eventContext":{"resultClock":{"kind":"played","liveStartJstTicks":null,"serverNowJstTicks":639268416000000001}},"limits":{"timeLimitMs":null}}"#;
        let parsed: RequestWire = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.event_context.unwrap().result_clock.unwrap().server_now_jst_ticks, 639268416000000001);
        assert_eq!(parsed.room.unwrap().players, 5);
    }

    #[test]
    fn capabilities_report_missing_domains_without_calling_them_complete() {
        let value = capabilities();
        assert_eq!(value["support"]["battleLive"]["score"], "proven");
        assert_eq!(value["support"]["arenaLive"]["eventPoints"], "proven");
        assert_eq!(value["rankConfirmation"], "onCompletion");
        assert_eq!(value["support"]["freeLive"]["score"], "proven");
        assert_eq!(value["luckMissions"], true);
        assert_eq!(value["requestFormat"], REQUEST_FORMAT);
        assert_eq!(value["support"]["freeLive"]["scoreAndLife"], "proven");
        assert_eq!(value["scoreAndLife"]["requiresCompleteJudgementStream"], true);
        assert_eq!(value["skip"]["musicIdOrChallengeMusicId"], true);
        assert_eq!(value["eventIds"]["skip"], "challengeMusicId");
    }
}
