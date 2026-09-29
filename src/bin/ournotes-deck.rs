//! Command-line front end: `ournotes-deck <power|skip|live> --data FILE --roster FILE [options]`, and
//! `ournotes-deck chart-stats --data FILE`.

use std::process::ExitCode;
use std::time::Duration;

use ournotes_deck::cards::Roster;
use ournotes_deck::data::DeckData;
use ournotes_deck::live::model::{JudgementStream, JustRule, Play};
use ournotes_deck::scenario::{ContextInput, PowerSnapshotInput, Scenario};
use ournotes_deck::search::{
    Constraints, GekisouObjective, Objective, PlayInput, Pool, SearchRequest, SeedSet, music_of_score, search,
};
use serde_json::json;

const USAGE: &str = "usage:
  ournotes-deck power --data FILE --roster FILE [--music ID] [--event] [common options]
  ournotes-deck skip  --data FILE --roster FILE --score ID [common options]
  ournotes-deck live  --data FILE --roster FILE --score ID [--exclude-snap-skills] [--play FILE] [--event]
                      [--gekisou (--seeds N | --seed-list S[,S...])] [common options]
  ournotes-deck chart-stats --data FILE [--seeds N] [--formation-seeds N | --no-gekisou-skills]
                      [--charts ID[,ID...]] [--jobs N] [-o FILE]
--data is a deck data file (nnnotes.deck-data/1). live scores the whole-live simulation with snap skills, where
--play is a judgement stream; with --exclude-snap-skills it scores live skills only, where --play is a per-note
play. --play defaults to the theoretical best play. --gekisou plays the live with Gekisou on and ranks by the sum
of the scores over a seed set only in diagnostic mode (--seeds/--seed-list). Native expectation uses --seed-law.
live expectation: --expectation finite --seed-law FILE (JSON [[rootSeed,positiveWeight],...])
legacy diagnostic only: --diagnostic-best-order (not native expected score)
objectives: --objective power|score|client-event-points|conditional-client-event-items [--event-id ID]
conditional items also require --resource-type ID --resource-id ID and context.eventPayoff.selectedRewards
scenario options: --scenario free|mission|battle|arena|challenge --scenario-music ID --context FILE
--scenario-music is the special row ID for arena/challenge; --score always denotes the base chart.
--context uses explicit powerSnapshot.eventIds and separate resultClock normalized DateTime ticks.
chart-stats measures every chart on the whole-live simulation (ournotes-deck.chart-stats/3): the no-skill score and
the weight of every score-up kind at every position, with Gekisou on per seed (--seeds N for charts with a luck range,
default 8; rank 1, range weights for the other ranks, the Perfect play's scores) and with Gekisou off (offSeeds); and
each chart's best Gekisou skill formation measured on --formation-seeds N seeds of a luck chart (default 32, the
first --seeds of them the chart's seeds). --charts keeps only these score ids; --jobs N measures N charts at once.
common options: -k N (default 10), --leader ID, --include ID[,ID...], --exclude ID[,ID...],
                --exclude-snaps ID[,ID...], --no-snaps, --time-limit-ms N";

fn ids(s: &str) -> Result<Vec<i64>, String> {
    s.split(',').filter(|x| !x.is_empty()).map(|x| x.trim().parse().map_err(|_| format!("bad id {x:?}"))).collect()
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn chart_stats(args: &[String]) -> Result<Option<serde_json::Value>, String> {
    let (mut data, mut seeds, mut out) = (None, None, None);
    let mut options = ournotes_deck::chartstats::Options::default();
    let (mut only, mut jobs): (Option<Vec<i64>>, usize) = (None, 1);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut val = || -> Result<String, String> {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{a} needs a value"))
        };
        match a {
            "--data" => data = Some(val()?),
            "--seeds" => seeds = Some(val()?.trim().parse::<usize>().map_err(|_| "bad --seeds".to_string())?),
            "--formation-seeds" => {
                let n = val()?.trim().parse::<usize>().map_err(|_| "bad --formation-seeds".to_string())?;
                options.formation_seeds = Some(n);
            }
            "--no-gekisou-skills" => options.formation_seeds = None,
            "--charts" => only = Some(ids(&val()?)?),
            "--jobs" => jobs = val()?.trim().parse::<usize>().map_err(|_| "bad --jobs".to_string())?.max(1),
            "-o" | "--out" => out = Some(val()?),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
        i += 1;
    }
    let mut data = DeckData::from_path(data.ok_or("--data is required")?).map_err(|e| e.to_string())?;
    if let Some(only) = &only {
        data.charts.retain(|c| only.contains(&c.score_id));
    }
    if let Some(n) = seeds {
        options.seeds = n;
    }
    let doc = if jobs <= 1 {
        ournotes_deck::chartstats::document_with(&data, &options).map_err(|e| e.to_string())?
    } else {
        chart_stats_parallel(data, &options, jobs)?
    };
    match out {
        Some(path) => {
            let mut text = serde_json::to_string(&doc).expect("json");
            text.push('\n');
            std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
            Ok(None)
        }
        None => Ok(Some(doc)),
    }
}

/// The chart-stats document with `jobs` charts measured at once (the same document as one at a time).
fn chart_stats_parallel(
    mut data: DeckData,
    options: &ournotes_deck::chartstats::Options,
    jobs: usize,
) -> Result<serde_json::Value, String> {
    use ournotes_deck::chartstats;
    let charts = std::mem::take(&mut data.charts);
    let mut doc = chartstats::document_with(&data, options).map_err(|e| e.to_string())?;
    let kinds = chartstats::kinds(&data.master);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Result<chartstats::ChartStats, String>>>> =
        charts.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(charts.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(c) = charts.get(i) else { break };
                    let t = std::time::Instant::now();
                    let r = chartstats::chart_stats_with(&data.master, c, &kinds, options)
                        .map_err(|e| format!("chart {}: {e}", c.score_id));
                    eprintln!(
                        "chart {} {:.1} s{}",
                        c.score_id,
                        t.elapsed().as_secs_f64(),
                        if r.is_err() { " FAILED" } else { "" }
                    );
                    *results[i].lock().expect("lock") = Some(r);
                }
            });
        }
    });
    let mut out = Vec::with_capacity(charts.len());
    for r in results {
        let r = r.into_inner().expect("lock").ok_or("a chart was not measured")?;
        out.push(serde_json::to_value(r?).expect("json"));
    }
    doc["charts"] = serde_json::Value::Array(out);
    Ok(doc)
}

fn run(args: &[String]) -> Result<serde_json::Value, String> {
    let cmd = args.first().ok_or(USAGE)?.as_str();
    if cmd == "chart-stats" {
        return chart_stats(&args[1..]).map(|v| v.unwrap_or(serde_json::Value::Null));
    }
    let mut data = None;
    let mut roster = None;
    let mut music = None;
    let mut scenario_name = None;
    let mut scenario_music = None;
    let mut context_file = None;
    let mut score = None;
    let mut play = None;
    let mut event = false;
    let mut objective_name = None;
    let mut target_event_id = None;
    let mut resource_type = None;
    let mut resource_id = None;
    let mut expectation = None;
    let mut seed_law = None;
    let mut diagnostic_best_order = false;
    let mut exclude_snap_skills = false;
    let mut k = 10usize;
    let mut c = Constraints::default();
    let mut limit = None;
    let mut gekisou = false;
    let mut seeds: Option<SeedSet> = None;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        let mut val = || -> Result<String, String> {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{a} needs a value"))
        };
        match a {
            "--data" => data = Some(val()?),
            "--roster" => roster = Some(val()?),
            "--scenario" => scenario_name = Some(val()?),
            "--scenario-music" => scenario_music = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--context" => context_file = Some(val()?),
            "--music" => music = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--score" => score = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--play" => play = Some(val()?),
            "--objective" => objective_name = Some(val()?),
            "--resource-type" => resource_type = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--resource-id" => resource_id = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--event-id" => target_event_id = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--expectation" => expectation = Some(val()?),
            "--seed-law" => seed_law = Some(val()?),
            "--diagnostic-best-order" => diagnostic_best_order = true,
            "--event" => event = true,
            "--exclude-snap-skills" => exclude_snap_skills = true,
            "-k" => k = val()?.parse().map_err(|_| "bad -k".to_string())?,
            "--leader" => c.leader = Some(val()?.parse().map_err(|_| "bad --leader".to_string())?),
            "--include" => c.include_members = ids(&val()?)?,
            "--exclude" => c.exclude_members = ids(&val()?)?,
            "--exclude-snaps" => c.exclude_snaps = ids(&val()?)?,
            "--no-snaps" => c.no_snaps = true,
            "--gekisou" => gekisou = true,
            "--seeds" => seeds = Some(SeedSet::Published(val()?.parse().map_err(|_| "bad --seeds".to_string())?)),
            "--seed-list" => {
                let v = val()?;
                let list = v
                    .split(',')
                    .filter(|x| !x.is_empty())
                    .map(|x| x.trim().parse::<i32>().map_err(|_| format!("bad seed {x:?}")))
                    .collect::<Result<Vec<_>, _>>()?;
                seeds = Some(SeedSet::List(list));
            }
            "--time-limit-ms" => {
                limit = Some(Duration::from_millis(val()?.parse().map_err(|_| "bad --time-limit-ms".to_string())?))
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
        i += 1;
    }
    if !matches!(cmd, "power" | "skip" | "live") {
        return Err(USAGE.into());
    }
    if cmd == "live" {
        if !diagnostic_best_order && expectation.as_deref() != Some("finite") {
            return Err("live requires --expectation finite --seed-law FILE: native MemberShuffle is random, not a deck decision. Use --diagnostic-best-order only for the historical order-optimized diagnostic".into());
        }
        if diagnostic_best_order && (expectation.is_some() || seed_law.is_some()) {
            return Err("--diagnostic-best-order cannot be combined with an expectation law".into());
        }
        if expectation.is_some() && exclude_snap_skills {
            return Err("native expectation needs the full simulation including snap skills".into());
        }
    } else if expectation.is_some() || seed_law.is_some() || diagnostic_best_order {
        return Err("expectation and diagnostic order options apply only to live".into());
    }
    if cmd != "live" && (gekisou || play.is_some() || exclude_snap_skills || seeds.is_some()) {
        return Err("play/Gekisou/seed options apply only to live".into());
    }
    if cmd != "power" && music.is_some() {
        return Err("--music applies only to legacy abstract power; use --scenario-music".into());
    }
    if seeds.is_some() && (expectation.is_some() || !gekisou) {
        return Err("--seeds/--seed-list are only for diagnostic Gekisou; native expectation uses --seed-law".into());
    }
    if event && (cmd == "skip" || (cmd == "live" && !diagnostic_best_order)) {
        return Err(
            "--event is only a legacy power/order-diagnostic override; choose an explicit native scenario instead"
                .into(),
        );
    }
    let law = match expectation.as_deref() {
        Some("finite") => {
            let file = seed_law.ok_or("--expectation finite requires --seed-law FILE containing [[rootSeed,positiveWeight],...] (an explicit finite law, not the unknown real server seed distribution)")?;
            let atoms: Vec<(i32, u64)> = serde_json::from_str(&read(&file)?).map_err(|e| format!("seed law: {e}"))?;
            Some(ournotes_deck::search::expectation::FiniteSeedLaw::new(atoms).map_err(|e| e.to_string())?)
        }
        Some(name) => return Err(format!("unsupported expectation law {name:?}; supply finite with explicit masses")),
        None => None,
    };
    let data = DeckData::from_path(data.ok_or("--data is required")?).map_err(|e| e.to_string())?;
    let roster = Roster::from_json(&read(&roster.ok_or("--roster is required")?)?).map_err(|e| e.to_string())?;
    if scenario_name.is_none() && scenario_music.is_some() {
        return Err("--scenario-music requires --scenario".into());
    }
    if scenario_name.is_some() && music.is_some() {
        return Err("--music is a legacy abstract-power input; use --scenario-music with --scenario".into());
    }
    if scenario_name.is_some() && event {
        return Err("--event is a legacy abstract override; an explicit scenario determines event parameters".into());
    }
    let objective_name = objective_name.as_deref().unwrap_or(if cmd == "power" { "power" } else { "score" });
    if !matches!(objective_name, "power" | "score" | "client-event-points" | "conditional-client-event-items") {
        return Err(format!(
            "unknown objective {objective_name:?}; server-selected item rewards require an explicit reward adapter, never guessed drops"
        ));
    }
    let item_objective = objective_name == "conditional-client-event-items";
    let event_objective = objective_name == "client-event-points" || item_objective;
    let item_target = if item_objective {
        Some((
            resource_type.ok_or("conditional items require --resource-type")?,
            resource_id.ok_or("conditional items require --resource-id")?,
        ))
    } else {
        if resource_type.is_some() || resource_id.is_some() {
            return Err("resource target options require conditional-client-event-items".into());
        }
        None
    };
    if (objective_name == "power") != (cmd == "power") {
        return Err("power objective requires power command; score/event objectives require skip or live".into());
    }
    if !event_objective && target_event_id.is_some() {
        return Err("--event-id requires --objective client-event-points".into());
    }
    if event_objective && diagnostic_best_order {
        return Err("event payoff cannot optimize legacy performance_order; supply a finite root law".into());
    }
    let context_input: ContextInput = match context_file {
        Some(p) => serde_json::from_str(&read(&p)?)
            .map_err(|e| format!("context: {e}; dates must be normalized DateTime ticks, not assumed-JST strings"))?,
        None => ContextInput {
            power_snapshot: PowerSnapshotInput { event_ids: roster.player.events.clone(), captured_jst_ticks: None },
            result_clock: None,
            event_payoff: None,
        },
    };
    let explicit_scenario = scenario_name.is_some();
    let selected_scenario = match scenario_name.as_deref() {
        Some(name) => {
            let id = scenario_music
                .ok_or("--scenario requires --scenario-music ID (special-table ID for arena/challenge)")?;
            Some(match name {
                "free" => Scenario::Free(id),
                "mission" => Scenario::Mission(id),
                "battle" => Scenario::Battle(id),
                "arena" => Scenario::Arena(id),
                "challenge" => Scenario::Challenge(id),
                _ => return Err(format!("unknown scenario {name:?}; expected free|mission|battle|arena|challenge")),
            })
        }
        None if cmd != "power" && !event => Some(Scenario::Free(
            music_of_score(&data.master, score.ok_or("--score is required")?).map_err(|e| e.to_string())?,
        )),
        _ => None,
    };
    let context = selected_scenario
        .map(|scenario| {
            let fevers = match score {
                Some(id) => data.data_chart(id).map(|c| c.fevers.as_slice()).unwrap_or(&[]),
                None => &[],
            };
            context_input.resolve(&data.master, scenario, score, fevers).map_err(|e| e.to_string())
        })
        .transpose()?;
    if let Some(clock) = &context_input.result_clock
        && matches!(
            (cmd, clock),
            ("skip", ournotes_deck::scenario::ResultClockInput::Played { .. })
                | ("live", ournotes_deck::scenario::ResultClockInput::Skip { .. })
        )
    {
        return Err("resultClock execution does not match the command".into());
    }
    let objective = match cmd {
        "power" => Objective::Power { music_id: music, event },
        "skip" => {
            let score_id = score.ok_or("--score is required")?;
            Objective::SkipScore { score_id, chart: data.chart(score_id).map_err(|e| e.to_string())? }
        }
        _ => {
            let score_id = score.ok_or("--score is required")?;
            let chart = data.chart(score_id).map_err(|e| e.to_string())?;
            let data_chart = data.data_chart(score_id).ok_or_else(|| format!("no chart for score id {score_id}"))?;
            let gk = if gekisou {
                let seeds = if law.is_some() {
                    SeedSet::List(vec![0])
                } else {
                    seeds.clone().ok_or("diagnostic --gekisou needs --seeds or --seed-list")?
                };
                let setup = match &context {
                    Some(ctx) => ctx.gekisou.clone(),
                    None => Scenario::Free(music_of_score(&data.master, score_id).map_err(|e| e.to_string())?)
                        .resolve(&data.master)
                        .map_err(|e| e.to_string())?
                        .gekisou_setup(&data_chart.fevers),
                };
                Some((GekisouObjective { seeds, fevers: data_chart.fevers.clone() }, setup))
            } else {
                None
            };
            let play = if exclude_snap_skills {
                PlayInput::Notes(match play {
                    Some(p) => serde_json::from_str::<Play>(&read(&p)?).map_err(|e| format!("play: {e}"))?,
                    None => Play::theoretical_best(&data.master, &chart).map_err(|e| e.to_string())?,
                })
            } else {
                let judgement_types = data_chart.judgement_types.clone();
                let stream = match (play, &gk) {
                    (Some(p), _) => {
                        serde_json::from_str::<JudgementStream>(&read(&p)?).map_err(|e| format!("play: {e}"))?
                    }
                    (None, None) => JudgementStream::theoretical_best(&chart),
                    (None, Some((_, setup))) => {
                        let rule = JustRule::new(&data.master, setup).map_err(|e| e.to_string())?;
                        JudgementStream::theoretical_best_gekisou(&chart, &judgement_types, &rule)
                            .map_err(|e| e.to_string())?
                    }
                };
                PlayInput::Stream { stream, judgement_types }
            };
            Objective::LiveScore { score_id, chart, play, event, exclude_snap_skills, gekisou: gk.map(|g| g.0) }
        }
    };
    let pool = match &context {
        Some(ctx) => ctx.pool(&data.master, &roster),
        None => {
            let mut frozen = roster.clone();
            frozen.player.events = context_input.power_snapshot.event_ids.clone();
            Pool::new(&data.master, &frozen)
        }
    }
    .map_err(|e| e.to_string())?;
    let objective = match &context {
        Some(ctx) => objective.in_scenario(ctx.clone()),
        None => objective,
    };
    let resolved_output = context.as_ref().map(|ctx| json!({
        "scenario": format!("{:?}",ctx.scenario), "explicitScenario":explicit_scenario,
        "baseLiveMusicId":ctx.resolved.live_music_id, "scoreId":ctx.score_id,
        "chartAsset": ctx.score_id.and_then(|id| data.data_chart(id)).map(|c| json!({"key":c.asset_key,"sha256":c.asset_sha256})),
        "powerMusic": {"id":ctx.resolved.power_music.id,"musicType":ctx.resolved.power_music.music_type,
            "bestMusicTagIds":ctx.resolved.power_music.best_music_tag_ids,
            "typeBonusRate":ctx.resolved.power_music.type_bonus_rate,"tagBonusRate":ctx.resolved.power_music.tag_bonus_rate},
        "calcEventParameter":ctx.resolved.calc_event_parameter,
        "skillTargetMusicType":ctx.resolved.skill_target_music_type,
        "gekisouMissions":ctx.resolved.gekisou_missions,"fevers":ctx.gekisou.fevers,
        "clocks":context_input,"resultJstTicks":ctx.result_clock.map(|c| c.jst_ticks()),
        "eligibility":"not evaluated; player progression and unlock eligibility are separate inputs",
        "serverRewards":"unknown; no server-selected rewards inferred"
    }));
    if let Some(law) = law {
        let request = SearchRequest { objective, k, constraints: c, time_limit: limit };
        let out = if event_objective {
            let ctx =
                context.as_ref().ok_or("event payoff needs a resolved scenario, not --event abstract override")?;
            let event_id = target_event_id.ok_or("client-event-points requires --event-id")?;
            let event_input =
                context_input.event_payoff.as_ref().ok_or("client-event-points requires context.eventPayoff")?;
            ctx.event_request(&data.master, event_input, event_id).map_err(|e| e.to_string())?;
            if item_objective && event_input.selected_rewards.is_none() {
                return Err("UnknownServerAuthority: context.eventPayoff.selectedRewards is required; [] means explicitly no selected rewards".into());
            }
            ournotes_deck::search::expectation::oracle_with_payoff_factory(
                &pool,
                &request,
                &law,
                || Ok(()),
                |physical, terminal, _| {
                    if let Some((ty,id))=item_target {
                        let items=ctx.preview_event_items(&pool,&physical.as_deck(),event_input,event_id,terminal.final_score)?;
                        ournotes_deck::scenario::item_payoff(&items,event_id,ty,id)
                    } else {
                        Ok(ctx.preview_event_points(&pool, &physical.as_deck(), event_input, event_id, terminal.final_score)?.points_for(event_id) as i128)
                    }
                },
            )
        } else {
            ournotes_deck::search::expectation::oracle(&pool, &request, &law)
        }
        .map_err(|e| e.to_string())?;
        let mut previews = Vec::new();
        let mut item_previews = Vec::new();
        if event_objective {
            let ctx = context.as_ref().expect("validated context");
            let input = context_input.event_payoff.as_ref().expect("validated event input");
            for result in &out.results {
                let terminal_previews = result
                    .evaluation
                    .outcomes
                    .iter()
                    .map(|o| {
                        ctx.preview_event_points(
                            &pool,
                            &result.physical.as_deck(),
                            input,
                            target_event_id.expect("validated event id"),
                            o.final_score,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                if item_objective {
                    item_previews.push(
                        result
                            .evaluation
                            .outcomes
                            .iter()
                            .map(|o| {
                                ctx.preview_event_items(
                                    &pool,
                                    &result.physical.as_deck(),
                                    input,
                                    target_event_id.expect("validated event id"),
                                    o.final_score,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|e| e.to_string())?,
                    );
                }
                previews.push(terminal_previews);
            }
        }
        return Ok(
            json!({"resolvedContext":resolved_output,"objective":objective_name,"targetEventId":target_event_id,
            "jsonNumberPolicy":"exact JSON integers; parse with arbitrary-precision integers (not JavaScript Number)","probabilityLaw":"explicit-finite-native-root-law","legacyStreamBaseSeed":"ignored; finite law is authoritative","arithmetic":"client-f32-i32; checked-i128-u128-expectation",
            "proofScope":"conditional on supplied finite root law and simulator supported domain; not inferred TickCount distribution",
            "clientCounterPreviews":previews,"conditionalItemPreviews":item_previews,"itemTarget":item_target,"search":out}),
        );
    }
    if event_objective {
        let event_id = target_event_id.ok_or("client-event-points requires --event-id")?;
        let input = context_input.event_payoff.as_ref().ok_or("client-event-points requires context.eventPayoff")?;
        let request = SearchRequest { objective, k, constraints: c, time_limit: limit };
        let out = ournotes_deck::scenario::search_skip_event_payoff(&pool, &request, input, event_id, item_target)
            .map_err(|e| e.to_string())?;
        return Ok(json!({"resolvedContext":resolved_output,"objective":objective_name,"targetEventId":event_id,
            "proofScope":"exhaustive physical decks; client counter preview only, no server award authority","search":out}));
    }
    let request = SearchRequest { objective, k, constraints: c, time_limit: limit };
    let out = if diagnostic_best_order {
        ournotes_deck::search::search_best_order_diagnostic(&pool, &request)
    } else {
        search(&pool, &request)
    }
    .map_err(|e| e.to_string())?;
    let mut v = json!({
        "objective": if diagnostic_best_order {"diagnostic-best-performance-order-not-native-expectation"} else {cmd},
        "resolvedContext": resolved_output,
        "compatibility": if context.is_some() {"resolved-client-scenario"} else {"legacy-abstract-power-override"},
        "completion": out.completion,
        "results": out.results,
        "stats": out.stats,
        "elapsedMs": out.elapsed.as_secs_f64() * 1e3,
        "verifyElapsedMs": out.verify_elapsed.as_secs_f64() * 1e3,
    });
    if let Some(s) = out.seeds {
        v["seeds"] = json!(s);
    }
    Ok(v)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(serde_json::Value::Null) => ExitCode::SUCCESS,
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).expect("json"));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
