//! Compare deterministic idle scheduling against the unchanged updater path.
use ournotes_search::{
    auxiliary, handler,
    search::{Completion, diagnostics},
    types::RecommendationRequest,
};
use ournotes_sim::{
    cards::Roster,
    data::DeckData,
    live::full::{take_idle_plan_stats, with_idle_plan_disabled},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{env, fs, time::Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deck {
    members: [i64; 5],
    snaps: [Option<i64>; 5],
}

/// Every wall-clock key of the outcome and its telemetry ends in `Ms`.
fn semantic(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object.iter().filter(|(k, _)| !k.ends_with("Ms")).map(|(k, v)| (k.clone(), semantic(v))).collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(semantic).collect()),
        other => other.clone(),
    }
}

fn measured(
    disabled: bool,
    evaluate: impl FnOnce() -> Result<ournotes_search::types::RecommendationOutcome, ournotes_sim::Error>,
) -> Result<Value, Box<dyn std::error::Error>> {
    take_idle_plan_stats();
    let started = Instant::now();
    let outcome = if disabled { with_idle_plan_disabled(evaluate)? } else { evaluate()? };
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(json!({"wallMs":wall_ms,"idlePlan":take_idle_plan_stats(),"outcome":outcome}))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if !(args.len() == 5 || (args.len() == 6 && args[5] == "--search")) {
        return Err("idle_audit DATA ROSTER REQUEST DECKS OUTPUT [--search]".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    if args.len() == 6 && request.limits.time_limit_ms.is_some() {
        return Err("search A/B requires timeLimitMs:null; maxCandidates may bound equal deterministic work".into());
    }
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    if decks.is_empty() {
        return Err("idle audit needs at least one physical deck".into());
    }
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let mut violations = 0;
    let mut comparisons = Vec::new();
    for deck in decks {
        let reference = measured(true, || auxiliary::evaluate_built(&built, deck.members, deck.snaps))?;
        let planned = measured(false, || auxiliary::evaluate_built(&built, deck.members, deck.snaps))?;
        let complete = reference["outcome"]["completion"] == serde_json::to_value(Completion::Complete)?
            && planned["outcome"]["completion"] == serde_json::to_value(Completion::Complete)?;
        // Includes every physical slot, root atom, payoff, order, life/conversion
        // observation and all non-timing counters; not merely expected score.
        let equal = semantic(&reference["outcome"]) == semantic(&planned["outcome"]);
        violations += usize::from(!complete || !equal);
        comparisons.push(json!({"members":deck.members,"snaps":deck.snaps,
            "complete":complete,"semanticAndCountersEqual":equal,"reference":reference,"planned":planned}));
    }
    let search = if args.len() == 6 {
        let evaluate =
            || diagnostics::recommend_experiment(&data, &roster, &request, diagnostics::Schedule::Production);
        let reference = measured(true, evaluate)?;
        let planned = measured(false, evaluate)?;
        let equal = semantic(&reference["outcome"]) == semantic(&planned["outcome"]);
        violations += usize::from(!equal);
        Some(json!({"semanticAndCountersEqual":equal,"reference":reference,"planned":planned}))
    } else {
        None
    };
    let report = json!({"scope":"declared physical decks and root atoms; optional deterministic search A/B",
        "comparison":"all outcome fields except keys ending in Ms (wall-clock times)",
        "timing":"one sequential reference/planned sample; diagnostic counters enabled; not a speedup claim",
        "violations":violations,"decks":comparisons,"search":search});
    fs::write(&args[4], serde_json::to_vec_pretty(&report)?)?;
    if violations != 0 {
        return Err("idle execution audit differs or a fixed evaluation is incomplete; inspect saved report".into());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
