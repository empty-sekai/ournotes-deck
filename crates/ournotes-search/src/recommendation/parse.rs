//! Parsing and validation of a request against the account and the master data.

use super::*;

/// A request checked and mapped to the search's request.
pub(super) struct Parsed {
    pub(super) kind: GoalKind,
    pub(super) goal: Value,
    pub(super) metric: Option<Value>,
    pub(super) exclusions: Exclusions,
    pub(super) k: usize,
    pub(super) search: RecommendationRequest,
}

pub(super) struct Issues(pub(super) Vec<Issue>);

impl Issues {
    pub(super) fn add(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.0.push(Issue { path: path.into(), code: code.into(), message: message.into() });
    }
}

/// A fraction field of `accuracy`: in `[0, 1]`.
pub(super) fn fraction(issues: &mut Issues, path: &str, value: f64) -> Option<f64> {
    if (0.0..=1.0).contains(&value) {
        Some(value)
    } else {
        issues.add(path, "input", format!("{value} is not in [0, 1]"));
        None
    }
}

/// The score id of a difficulty of a base song.
pub(super) fn score_id(data: &DeckData, issues: &mut Issues, music_id: i64, difficulty: Option<&str>) -> Option<i64> {
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

/// Validate the declared play before account resolution/search, retaining its public input path.
pub(super) fn complete_stream(
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

pub(super) fn parse_request(data: &DeckData, w: RequestWire, issues: &mut Issues) -> Option<Parsed> {
    let master = &data.master;
    if w.format != REQUEST_FORMAT {
        issues.add("format", "unsupported_format", format!("expected {REQUEST_FORMAT}"));
    }
    let aggregation = match w.aggregation.as_deref().unwrap_or("expected") {
        "expected" => Aggregation::Expected,
        "maximum" => Aggregation::Maximum,
        other => {
            issues.add("aggregation", "input", format!("{other} is not one of expected, maximum"));
            return None;
        }
    };
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
        (_, Some(m)) if m.kind == "combined" => {
            metric = combined_metric(kind, m, challenge_skip, explicit_play.is_some(), issues);
            consumption = m.consumption;
        }
        (_, Some(m)) => match metric_fields(&m.kind) {
            None => issues.add("metric.kind", "input", format!("{} is not a metric", m.kind)),
            Some(_) if !kind.metrics().contains(&m.kind.as_str()) => {
                issues.add("metric.kind", "input", format!("{} does not apply to {}", m.kind, kind.name()))
            }
            Some(fields) => {
                let mut complete = true;
                // Whether event points come before event items among the teams with the most Challenge points.
                let secondary_priority = match m.secondary_priority.as_deref() {
                    None => None,
                    Some("eventPointsFirst") => Some(true),
                    Some("eventItemsFirst") => Some(false),
                    Some(_) => {
                        complete = false;
                        issues.add("metric.secondaryPriority", "input", "choose eventPointsFirst or eventItemsFirst");
                        None
                    }
                };
                let fields = if m.kind == "challengePoints" && m.secondary_priority.is_some() {
                    &["eventId", "consumption", "secondaryPriority", "resourceType", "resourceId"][..]
                } else {
                    fields
                };
                for (name, present) in [
                    ("secondaryPriority", m.secondary_priority.is_some()),
                    ("threshold", m.threshold.is_some()),
                    ("minFinalLife", m.min_final_life.is_some()),
                    ("eventId", m.event_id.is_some()),
                    ("resourceType", m.resource_type.is_some()),
                    ("resourceId", m.resource_id.is_some()),
                    ("consumption", m.consumption.is_some()),
                    ("levels", m.levels.is_some()),
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
                    metric = Some(match (m.kind.as_str(), secondary_priority) {
                        ("challengePoints", Some(points_first)) => {
                            let event_id = m.event_id.unwrap_or_default();
                            let points = Metric::ClientEventPoints { event_id };
                            let items = Metric::RankedEventItems {
                                event_id,
                                resource_type: m.resource_type.expect("complete resource type"),
                                resource_id: m.resource_id.expect("complete resource ID"),
                            };
                            let (second, third) = if points_first { (points, items) } else { (items, points) };
                            Metric::Combined {
                                levels: [Metric::ClientChallengePoints { event_id }, second, third]
                                    .into_iter()
                                    .map(|metric| MetricLevel::new(vec![MetricTerm { metric, weight: 1 }]))
                                    .collect(),
                            }
                        }
                        (kind, _) => simple_metric(
                            kind,
                            m.threshold,
                            m.min_final_life,
                            m.event_id,
                            (m.resource_type, m.resource_id),
                        ),
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
    let event_metric = metric.as_ref().is_some_and(|m| m.any(|term| term.event().is_some()));
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
            if c.reward_projection && !c.local_events.is_empty() {
                issues.add(
                    "eventContext.localEvents",
                    "input",
                    "rewardProjection and explicit localEvents are mutually exclusive",
                );
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
        if let Some(priority) = metric_wire.and_then(|m| m.secondary_priority.as_ref()) {
            echo["secondaryPriority"] = json!(priority);
        }
        if let Some(levels) = metric_wire.and_then(|m| m.levels.as_ref()) {
            let term = |term: &MetricTermWire| {
                let mut echo = json!({"kind": term.kind, "weight": term.weight});
                for (name, value) in [
                    ("threshold", term.threshold.map(i64::from)),
                    ("minFinalLife", term.min_final_life.map(i64::from)),
                    ("eventId", term.event_id),
                    ("resourceType", term.resource_type),
                    ("resourceId", term.resource_id),
                ] {
                    if let Some(v) = value {
                        echo[name] = json!(v);
                    }
                }
                echo
            };
            echo["levels"] = levels.iter().map(|terms| terms.iter().map(term).collect::<Value>()).collect();
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
        aggregation,
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
