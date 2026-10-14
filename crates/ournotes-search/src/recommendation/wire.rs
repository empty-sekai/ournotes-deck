//! The wire format of a recommendation request.

use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RequestWire {
    pub(super) format: String,
    pub(super) goal: GoalWire,
    #[serde(default)]
    pub(super) aggregation: Option<String>,
    #[serde(default)]
    pub(super) metric: Option<MetricWire>,
    #[serde(default)]
    pub(super) event_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub(super) event_context: Option<EventContextWire>,
    #[serde(default)]
    pub(super) room: Option<RoomWire>,
    #[serde(default)]
    pub(super) constraints: Option<ConstraintsWire>,
    #[serde(default)]
    pub(super) k: Option<i64>,
    #[serde(default)]
    pub(super) limits: Option<LimitsWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GoalWire {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) music_id: Option<i64>,
    #[serde(default)]
    pub(super) arena_music_id: Option<i64>,
    #[serde(default)]
    pub(super) challenge_music_id: Option<i64>,
    #[serde(default)]
    pub(super) difficulty: Option<String>,
    #[serde(default)]
    pub(super) rank: Option<i64>,
    #[serde(default)]
    pub(super) accuracy: Option<AccuracyWire>,
    /// Explicit complete play. Parsed below so missing/invalid stream errors retain goal paths.
    #[serde(default)]
    pub(super) play: Option<Value>,
    #[serde(default)]
    pub(super) event_parameter: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AccuracyWire {
    #[serde(default)]
    pub(super) great_fraction: Option<f64>,
    #[serde(default)]
    pub(super) just_fraction: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MetricWire {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) secondary_priority: Option<String>,
    #[serde(default)]
    pub(super) threshold: Option<i32>,
    #[serde(default)]
    pub(super) min_final_life: Option<i32>,
    #[serde(default)]
    pub(super) event_id: Option<i64>,
    #[serde(default)]
    pub(super) resource_type: Option<i64>,
    #[serde(default)]
    pub(super) resource_id: Option<i64>,
    #[serde(default)]
    pub(super) consumption: Option<i32>,
    /// The levels of a `combined` metric, each a list of weighted terms.
    #[serde(default)]
    pub(super) levels: Option<Vec<Vec<MetricTermWire>>>,
}

/// One term of a `combined` metric: a metric kind with its fields and a weight. The request's
/// `metric.consumption` covers every event term.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MetricTermWire {
    pub(super) kind: String,
    #[serde(default)]
    pub(super) weight: Option<i64>,
    #[serde(default)]
    pub(super) threshold: Option<i32>,
    #[serde(default)]
    pub(super) min_final_life: Option<i32>,
    #[serde(default)]
    pub(super) event_id: Option<i64>,
    #[serde(default)]
    pub(super) resource_type: Option<i64>,
    #[serde(default)]
    pub(super) resource_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EventContextWire {
    /// Project one result's event rewards. Synthetic counters are never account balances or inventory.
    #[serde(default)]
    pub(super) reward_projection: bool,
    #[serde(default)]
    pub(super) result_clock: Option<ResultClockWire>,
    #[serde(default)]
    pub(super) local_events: Vec<LocalEvent>,
    #[serde(default)]
    pub(super) event_windows: Option<Vec<EventWindow>>,
    #[serde(default)]
    pub(super) multiplayer_ranks: Option<Vec<MultiplayerRankInput>>,
    #[serde(default)]
    pub(super) multiplayer_result_panel: Option<MultiplayerResultPanelInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResultClockWire {
    #[serde(rename = "kind", alias = "execution")]
    pub(super) execution: String,
    #[serde(default, rename = "liveStartJstTicks", alias = "savedStartJstTicks")]
    pub(super) saved_start_jst_ticks: Option<i64>,
    pub(super) server_now_jst_ticks: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RoomWire {
    pub(super) players: u8,
    #[serde(default)]
    pub(super) others_average_score: Option<i32>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub(super) struct ConstraintsWire {
    pub(super) leader: Option<i64>,
    pub(super) include_members: Vec<i64>,
    pub(super) exclude_members: Vec<i64>,
    pub(super) exclude_snaps: Vec<i64>,
    pub(super) no_snaps: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LimitsWire {
    #[serde(default)]
    pub(super) time_limit_ms: Option<u64>,
}
