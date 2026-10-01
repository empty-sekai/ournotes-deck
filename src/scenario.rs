//! The music and event-parameter inputs selected by each player-deck live mode.
//!
//! The song used for power and the source of Gekisou missions are separate inputs. Challenge music overrides
//! power but not missions; arena music overrides missions but multiplayer power still uses the base song.

use crate::cards::SongView;
use crate::error::Error;
use crate::live::full::GekisouSetup;
use crate::master::{LiveMusicRow, Master};

/// A live mode and its master music id. Arena and challenge ids belong to their own music tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Free(i64),
    Mission(i64),
    Battle(i64),
    Arena(i64),
    Challenge(i64),
}

/// Resolved inputs to deck power and the live simulator. These do not decide server-awarded rewards.
#[derive(Clone, Debug)]
pub struct ResolvedScenario {
    /// The base song (owns the chart ids and score rank group).
    pub live_music_id: i64,
    /// The parameter view used by both slot bonuses and leader-skill conditions.
    pub power_music: SongView,
    pub calc_event_parameter: bool,
    /// Skill condition 4012: Challenge uses its literal type, including zero; Arena uses base.
    pub skill_target_music_type: i64,
    /// The three resolved mission slots, before selecting the chart's fever ranges.
    pub gekisou_missions: [i64; 3],
}

fn music(master: &Master, id: i64) -> Result<&LiveMusicRow, Error> {
    master.live_music(id).ok_or_else(|| Error::Master(format!("unknown live music {id}")))
}

impl Scenario {
    pub fn resolve(self, master: &Master) -> Result<ResolvedScenario, Error> {
        let (base, challenge, arena) = match self {
            Self::Free(id) | Self::Mission(id) | Self::Battle(id) => (music(master, id)?, None, None),
            Self::Challenge(id) => {
                let row =
                    master.challenge_music(id).ok_or_else(|| Error::Master(format!("unknown challenge music {id}")))?;
                (music(master, row.live_music_id)?, Some(row), None)
            }
            Self::Arena(id) => {
                let row = master.arena_music(id).ok_or_else(|| Error::Master(format!("unknown arena music {id}")))?;
                (music(master, row.live_music_id)?, None, Some(row))
            }
        };
        let mut missions = [base.gekisou_mission_1, base.gekisou_mission_2, base.gekisou_mission_3];
        if let Some(row) = arena {
            for (base, over) in
                missions.iter_mut().zip([row.gekisou_mission_1, row.gekisou_mission_2, row.gekisou_mission_3])
            {
                if over != 0 {
                    *base = over;
                }
            }
        }
        let skill_target_music_type = challenge.map_or(base.music_type, |r| r.music_type);
        if !matches!(skill_target_music_type, 0..=5 | 99) {
            return Err(Error::Game(format!("ToSkillTargetType rejects music type {skill_target_music_type}")));
        }
        Ok(ResolvedScenario {
            live_music_id: base.id,
            power_music: match challenge {
                Some(row) => SongView::from_challenge_row(master, row)?,
                None => SongView::from_row(base),
            },
            calc_event_parameter: challenge.is_some(),
            skill_target_music_type,
            gekisou_missions: missions,
        })
    }
}

impl ResolvedScenario {
    pub fn gekisou_setup(&self, fevers: &[(i32, i32)]) -> GekisouSetup {
        GekisouSetup { fevers: fevers.to_vec(), missions: self.gekisou_missions.to_vec() }
    }
}

/// One resolution shared by power, chart evaluation, mission setup and search.
/// Construct a pool through `pool` so its precomputed bonuses use the same frozen event snapshot.
#[derive(Clone, Debug)]
pub struct ResolvedContext {
    pub scenario: Scenario,
    pub resolved: ResolvedScenario,
    pub score_id: Option<i64>,
    pub gekisou: GekisouSetup,
    pub power_event_ids: Vec<i64>,
    pub power_snapshot_jst_ticks: Option<i64>,
    pub result_clock: Option<crate::event::EventResultClock>,
}

impl ResolvedContext {
    pub fn resolve(
        master: &Master,
        scenario: Scenario,
        score_id: Option<i64>,
        fevers: &[(i32, i32)],
        power_event_ids: Vec<i64>,
    ) -> Result<Self, Error> {
        let resolved = scenario.resolve(master)?;
        let base = music(master, resolved.live_music_id)?;
        if let Some(id) = score_id {
            if id == 0 || ![base.easy_id, base.normal_id, base.hard_id, base.expert_id].contains(&id) {
                return Err(Error::Input(format!("chart {id} does not belong to resolved base song {}", base.id)));
            }
            master.live_music_score(id).ok_or_else(|| Error::Master(format!("missing chart row {id}")))?;
        }
        if fevers.iter().any(|&(start, end)| end < start) || fevers.windows(2).any(|w| w[1].0 < w[0].0) {
            return Err(Error::Input("fever ranges must have start <= end and be sorted by start".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for id in &power_event_ids {
            if !seen.insert(*id) {
                return Err(Error::Input(format!("duplicate power snapshot event {id}")));
            }
            master.event(*id).ok_or_else(|| Error::Master(format!("unknown power snapshot event {id}")))?;
        }
        let gekisou = resolved.gekisou_setup(fevers);
        Ok(Self {
            scenario,
            resolved,
            score_id,
            gekisou,
            power_event_ids,
            power_snapshot_jst_ticks: None,
            result_clock: None,
        })
    }

    pub fn pool<'m>(
        &self,
        master: &'m Master,
        roster: &crate::cards::Roster,
    ) -> Result<crate::search::Pool<'m>, Error> {
        let mut frozen = roster.clone();
        frozen.player.events = self.power_event_ids.clone();
        crate::search::Pool::new(master, &frozen)
    }

    pub fn validate_pool(&self, pool: &crate::search::Pool) -> Result<(), Error> {
        if pool.player.events != self.power_event_ids || pool.power_event_snapshot != self.power_event_ids {
            return Err(Error::Input(
                "power event snapshot differs from resolved scenario; rebuild the pool with context.pool".into(),
            ));
        }
        Ok(())
    }
}

/// JSON boundary takes normalized DateTime ticks, never implicitly parsed date strings.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextInput {
    pub power_snapshot: PowerSnapshotInput,
    pub result_clock: Option<ResultClockInput>,
    pub event_payoff: Option<EventPayoffInput>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PowerSnapshotInput {
    pub event_ids: Vec<i64>,
    pub captured_jst_ticks: Option<i64>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "execution", rename_all = "camelCase", deny_unknown_fields)]
pub enum ResultClockInput {
    Played {
        #[serde(rename = "savedStartJstTicks")]
        saved_start_jst_ticks: Option<i64>,
        #[serde(rename = "serverNowJstTicks")]
        server_now_jst_ticks: i64,
    },
    Skip {
        #[serde(rename = "serverNowJstTicks")]
        server_now_jst_ticks: i64,
    },
}

impl ResultClockInput {
    pub fn resolve(&self) -> Result<crate::event::EventResultClock, Error> {
        use crate::event::EventResultClock;
        Ok(match *self {
            Self::Played { saved_start_jst_ticks, server_now_jst_ticks } => {
                if let Some(t) = saved_start_jst_ticks {
                    check_ticks(t)?;
                }
                check_ticks(server_now_jst_ticks)?;
                EventResultClock::Played { live_start_jst_ticks: saved_start_jst_ticks, server_now_jst_ticks }
            }
            Self::Skip { server_now_jst_ticks } => {
                check_ticks(server_now_jst_ticks)?;
                EventResultClock::Skip { server_now_jst_ticks }
            }
        })
    }
}

fn check_ticks(ticks: i64) -> Result<(), Error> {
    if !(0..=3_155_378_975_999_999_999).contains(&ticks) {
        return Err(Error::Input("normalized DateTime ticks must be in [0,3155378975999999999]; date strings require an explicit InvariantCulture compatibility adapter".into()));
    }
    Ok(())
}

impl ContextInput {
    pub fn resolve(
        &self,
        master: &Master,
        scenario: Scenario,
        score_id: Option<i64>,
        fevers: &[(i32, i32)],
    ) -> Result<ResolvedContext, Error> {
        if let Some(t) = self.power_snapshot.captured_jst_ticks {
            check_ticks(t)?;
        }
        let mut context =
            ResolvedContext::resolve(master, scenario, score_id, fevers, self.power_snapshot.event_ids.clone())?;
        context.power_snapshot_jst_ticks = self.power_snapshot.captured_jst_ticks;
        context.result_clock = self.result_clock.as_ref().map(ResultClockInput::resolve).transpose()?;
        Ok(context)
    }
}

/// Explicit client-counter inputs, not server eligibility or inventory authority.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventPayoffInput {
    pub consumed_count: i32,
    pub local_events: Vec<crate::event::LocalEvent>,
    pub event_windows: Option<Vec<crate::event::EventWindow>>,
    pub event_window_adapter: Option<String>,
    pub selected_rewards: Option<Vec<crate::event::ServerEventReward>>,
    /// External total-score ranks keyed by terminal local score, not guessed solo ranks.
    pub multiplayer_ranks: Option<Vec<MultiplayerRankInput>>,
    /// Explicit result-panel adapter. This models 0x59f0128, not unverified network save semantics.
    pub multiplayer_result_panel: Option<MultiplayerResultPanelInput>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiplayerRankInput {
    pub local_final_score: i32,
    pub resolved_total_score_rank: i64,
}

/// Other players are explicit; local disconnection is independent of their flags.
/// Scores of disconnected participants remain in the checked Int32 total.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultiplayerResultPanelInput {
    /// Position of the local player in the original result list. Other players retain their list order.
    pub local_player_index: usize,
    pub local_disconnected: bool,
    pub other_players: Vec<PeerResultInput>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeerResultInput {
    pub final_score: i32,
    pub disconnected: bool,
}

impl MultiplayerResultPanelInput {
    pub fn total_and_count(&self, local_score: i32) -> Result<(i32, i64), Error> {
        if self.other_players.len() > 4 {
            return Err(Error::Input("multiplayer result panel allows at most four other players".into()));
        }
        if self.local_player_index > self.other_players.len() {
            return Err(Error::Input("localPlayerIndex is outside the original multiplayer result list".into()));
        }
        let mut total = 0i32;
        let mut count = i64::from(!self.local_disconnected);
        for position in 0..=self.other_players.len() {
            let score = if position == self.local_player_index {
                local_score
            } else {
                let p = &self.other_players[position - usize::from(position > self.local_player_index)];
                count += i64::from(!p.disconnected);
                p.final_score
            };
            total = total
                .checked_add(score)
                .ok_or_else(|| Error::Game("multiplayer result-panel checked Int32 score sum overflow".into()))?;
        }
        Ok((total, count))
    }
}

impl ResolvedContext {
    /// Select held events at the RESULT clock, never the power capture clock.
    pub fn event_request(
        &self,
        master: &Master,
        input: &EventPayoffInput,
        target_event_id: i64,
    ) -> Result<crate::event::EventPointRequest, Error> {
        use crate::event::{EventResultClock, EventResultRoute};
        master.event(target_event_id).ok_or_else(|| Error::Input(format!("unknown target event {target_event_id}")))?;
        if let Scenario::Challenge(id) = self.scenario {
            let selected = master
                .challenge_music(id)
                .ok_or_else(|| Error::Master(format!("unknown challenge music {id}")))?
                .event_id;
            if selected != target_event_id {
                return Err(Error::Input(format!(
                    "challenge music {id} belongs to event {selected}, not target event {target_event_id}"
                )));
            }
        }
        let clock = self.result_clock.ok_or_else(|| {
            Error::Input("event payoff requires a separate resultClock; power capture time is not a result time".into())
        })?;
        let mut seen = std::collections::HashSet::new();
        let windows = input.resolve_event_windows(master)?;
        for w in &windows {
            if !seen.insert(w.event_id) {
                return Err(Error::Input(format!("duplicate event window {}", w.event_id)));
            }
            master.event(w.event_id).ok_or_else(|| Error::Input(format!("unknown window event {}", w.event_id)))?;
            check_ticks(w.start_jst_ticks)?;
            if let Some(end) = w.end_jst_ticks {
                check_ticks(end)?;
            }
        }
        seen.clear();
        for l in &input.local_events {
            if !seen.insert(l.event_id) {
                return Err(Error::Input(format!("duplicate local event {}", l.event_id)));
            }
            master.event(l.event_id).ok_or_else(|| Error::Input(format!("unknown local event {}", l.event_id)))?;
        }
        let route = match (self.scenario, clock) {
            (Scenario::Challenge(_), EventResultClock::Played { .. }) => {
                EventResultRoute::ChallengePlayed { event_id: target_event_id }
            }
            (Scenario::Challenge(_), EventResultClock::Skip { .. }) => {
                EventResultRoute::ChallengeSkip { event_id: target_event_id }
            }
            (Scenario::Free(_), EventResultClock::Skip { .. }) => EventResultRoute::NormalSkip,
            (_, EventResultClock::Played { .. }) => EventResultRoute::NormalPlayed,
            _ => return Err(Error::Input("skip event route is verified only for Free and Challenge".into())),
        };
        if matches!(self.scenario, Scenario::Battle(_) | Scenario::Arena(_))
            && input.multiplayer_ranks.is_none()
            && input.multiplayer_result_panel.is_none()
        {
            return Err(Error::Input("Battle/Arena event payoff requires multiplayerRanks from an explicit total-score/peer adapter; solo rank and player-count defaults are not valid".into()));
        }
        if input.multiplayer_ranks.is_some() && input.multiplayer_result_panel.is_some() {
            return Err(Error::Input("choose multiplayerRanks OR multiplayerResultPanel, not both".into()));
        }
        if input.multiplayer_result_panel.as_ref().is_some_and(|p| p.other_players.len() > 4) {
            return Err(Error::Input("multiplayer result panel allows at most four other players".into()));
        }
        if let Some(ranks) = &input.multiplayer_ranks {
            let mut scores = std::collections::HashSet::new();
            for r in ranks {
                if !scores.insert(r.local_final_score) || !(0..=7).contains(&r.resolved_total_score_rank) {
                    return Err(Error::Input("multiplayerRanks requires unique terminal scores and ranks 0..7".into()));
                }
            }
        }
        Ok(crate::event::EventPointRequest {
            route,
            holding_event_ids: crate::event::holding_event_ids(&windows, clock),
            consumed_count: input.consumed_count,
            local_events: input.local_events.clone(),
        })
    }

    /// Call inside each random-law atom, not on E(score).
    pub fn preview_event_points(
        &self,
        pool: &crate::search::Pool,
        deck: &crate::search::Deck,
        input: &EventPayoffInput,
        target_event_id: i64,
        final_score: i32,
    ) -> Result<crate::event::EventPointPreview, Error> {
        self.validate_pool(pool)?;
        pool.check_deck(deck)?;
        let request = self.event_request(pool.master, input, target_event_id)?;
        let group = music(pool.master, self.resolved.live_music_id)?.live_score_rank_group;
        let rank = if matches!(self.scenario, Scenario::Battle(_) | Scenario::Arena(_)) {
            if let Some(panel) = &input.multiplayer_result_panel {
                let (total, count) = panel.total_and_count(final_score)?;
                crate::event::battle_score_rank(pool.master, group, i64::from(total), count)
            } else {
                input
                    .multiplayer_ranks
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|r| r.local_final_score == final_score))
                    .ok_or_else(|| {
                        Error::Input(format!(
                            "no explicit multiplayer total-score rank for terminal local score {final_score}"
                        ))
                    })?
                    .resolved_total_score_rank
            }
        } else {
            crate::event::score_rank(pool.master, group, i64::from(final_score))?
        };
        let members = deck.members.map(|i| crate::bonus::event_member(pool.master, &pool.members[i]));
        let snaps = deck.snaps.map(|i| i.map(|i| crate::bonus::event_snap(&pool.snaps[i])));
        crate::event::preview_client_event_points(
            pool.master,
            &request,
            &members.each_ref().map(Some),
            Some(&snaps.each_ref().map(|s| s.as_ref())),
            rank,
        )
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedSkipEventDeck {
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
    pub power: i32,
    pub score: i32,
    pub event_points: i32,
    pub terminal_payoff: i128,
    pub conditional_items: Option<crate::event::EventItemPreview>,
    pub preview: crate::event::EventPointPreview,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipEventSearchOutcome {
    pub completion: crate::search::Completion,
    pub evaluated: u64,
    pub results: Vec<RankedSkipEventDeck>,
}

/// Exhaustive physical-deck oracle: score/power pruning is invalid for event-point payoffs.
pub fn search_skip_event_points(
    pool: &crate::search::Pool,
    request: &crate::search::SearchRequest,
    input: &EventPayoffInput,
    event_id: i64,
) -> Result<SkipEventSearchOutcome, Error> {
    search_skip_event_payoff(pool, request, input, event_id, None)
}

/// With an item target, rank by the explicitly conditional resource quantity instead of event points.
pub fn search_skip_event_payoff(
    pool: &crate::search::Pool,
    request: &crate::search::SearchRequest,
    input: &EventPayoffInput,
    event_id: i64,
    item_target: Option<(i64, i64)>,
) -> Result<SkipEventSearchOutcome, Error> {
    use crate::search::{Completion, Objective};
    if item_target.is_some() && input.selected_rewards.is_none() {
        return Err(Error::Unsupported(
            "UnknownServerAuthority: selectedRewards are required for a conditional item objective".into(),
        ));
    }
    if !matches!(request.objective.inner(), Objective::SkipScore { .. }) {
        return Err(Error::Input("skip event oracle requires a SkipScore objective".into()));
    }
    let context = request
        .objective
        .context()
        .ok_or_else(|| Error::Input("skip event oracle requires a resolved scenario".into()))?;
    if !matches!(context.result_clock, Some(crate::event::EventResultClock::Skip { .. })) {
        return Err(Error::Input("skip event oracle requires a skip resultClock".into()));
    }
    context.event_request(pool.master, input, event_id)?;
    context.validate_pool(pool)?;
    let start = crate::clock::Instant::now();
    let mut out = SkipEventSearchOutcome { completion: Completion::Complete, evaluated: 0, results: Vec::new() };
    crate::search::expectation::visit_physical_decks(pool, &request.constraints, |physical| {
        if request.time_limit.is_some_and(|t| start.elapsed() >= t) {
            out.completion = Completion::TimedOut;
            return Ok(false);
        }
        let (power, score) = crate::search::evaluate(pool, &physical.as_deck(), &request.objective)?;
        let score = score.ok_or_else(|| Error::Input("missing skip score".into()))?;
        let preview = context.preview_event_points(pool, &physical.as_deck(), input, event_id, score)?;
        let conditional_items = item_target
            .map(|_| context.preview_event_items(pool, &physical.as_deck(), input, event_id, score))
            .transpose()?;
        let terminal_payoff = match (item_target, &conditional_items) {
            (Some((ty, id)), Some(items)) => item_payoff(items, event_id, ty, id)?,
            _ => i128::from(preview.points_for(event_id)),
        };
        out.evaluated += 1;
        out.results.push(RankedSkipEventDeck {
            members: physical.members.map(|i| pool.members[i].id),
            snaps: physical.snaps.map(|s| s.map(|i| pool.snaps[i].id)),
            power,
            score,
            event_points: preview.points_for(event_id),
            terminal_payoff,
            conditional_items,
            preview,
        });
        Ok(true)
    })?;
    out.results.sort_by(|a, b| {
        b.terminal_payoff
            .cmp(&a.terminal_payoff)
            .then(b.power.cmp(&a.power))
            .then(a.members.cmp(&b.members))
            .then(a.snaps.cmp(&b.snaps))
    });
    out.results.truncate(request.k);
    Ok(out)
}

impl EventPayoffInput {
    pub fn resolve_event_windows(&self, master: &Master) -> Result<Vec<crate::event::EventWindow>, Error> {
        match (&self.event_windows, self.event_window_adapter.as_deref()) {
            (Some(windows), None) => Ok(windows.clone()),
            (None, Some("canonical-master-no-offset")) => master.events.iter().map(crate::event::event_window_canonical).collect(),
            _ => Err(Error::Input("eventPayoff requires eventWindows (explicit normalized ticks) OR eventWindowAdapter=canonical-master-no-offset; arbitrary InvariantCulture date strings need an external adapter".into())),
        }
    }
}

impl ResolvedContext {
    /// Conditional on explicitly server-selected reward IDs; None is unknown, not an empty drop.
    pub fn preview_event_items(
        &self,
        pool: &crate::search::Pool,
        deck: &crate::search::Deck,
        input: &EventPayoffInput,
        target_event_id: i64,
        final_score: i32,
    ) -> Result<crate::event::EventItemPreview, Error> {
        if input.selected_rewards.is_none() {
            return Err(Error::Unsupported("UnknownServerAuthority: eventPayoff.selectedRewards is required; [] explicitly means no selected rewards".into()));
        }
        let points = self.preview_event_points(pool, deck, input, target_event_id, final_score)?;
        let request = self.event_request(pool.master, input, target_event_id)?;
        let members = deck.members.map(|i| crate::bonus::event_member(pool.master, &pool.members[i]));
        let snaps = deck.snaps.map(|i| i.map(|i| crate::bonus::event_snap(&pool.snaps[i])));
        crate::event::preview_client_event_items(
            pool.master,
            &crate::event::EventItemRequest {
                route: request.route,
                consumed_count: input.consumed_count,
                local_event_ids: points.local_events.iter().map(|l| l.event_id).collect(),
                selected_rewards: input.selected_rewards.clone(),
            },
            &members.each_ref().map(Some),
            Some(&snaps.each_ref().map(|s| s.as_ref())),
        )
    }
}

pub fn item_payoff(
    preview: &crate::event::EventItemPreview,
    event_id: i64,
    resource_type: i64,
    resource_id: i64,
) -> Result<i128, Error> {
    preview
        .rewards
        .iter()
        .filter(|r| r.event_id == event_id && r.resource_type == resource_type && r.resource_id == resource_id)
        .try_fold(0i128, |sum, r| {
            sum.checked_add(i128::from(r.amount))
                .ok_or_else(|| Error::Domain("conditional item payoff overflow".into()))
        })
}
