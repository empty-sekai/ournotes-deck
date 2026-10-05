//! LUCK weight samples of fixed decks: the live at constant weights beside the mean of played lives; with
//! CURVE_RUNS, the live weighted by the deck's own rush samples over that many lives per order; with TABLE_RUNS, the
//! live weighted by the chart's LUCK table entries over that many lives.
use ournotes_search::{handler, search::diagnostics, types::RecommendationRequest};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde::Deserialize;
use std::{env, fs};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deck {
    members: [i64; 5],
    snaps: [Option<i64>; 5],
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if !(6..=10).contains(&args.len()) {
        return Err(
            "luck_coefficient DATA ROSTER REQUEST DECKS SEEDS OUTPUT [CURVE_RUNS [TABLE_RUNS [COMPOSE_POWER [DP]]]]"
                .into(),
        );
    }
    let data = DeckData::from_path(&args[0])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[1])?)?;
    let request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let decks: Vec<Deck> = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let seeds: u32 = args[4].parse()?;
    let curve_runs: u32 = args.get(6).map_or(Ok(0), |a| a.parse())?;
    let table_runs: u32 = args.get(7).map_or(Ok(0), |a| a.parse())?;
    let compose_power: f64 = args.get(8).map_or(Ok(0.0), |a| a.parse())?;
    let use_dp: bool = args.get(9).map_or(Ok(false), |a| a.parse())?;
    let mut values = Vec::new();
    for d in &decks {
        let free = diagnostics::luck_chain_free(&built, d.members, d.snaps);
        values.push(
            match (
                free,
                diagnostics::luck_coefficient_sample(
                    &built,
                    d.members,
                    d.snaps,
                    seeds,
                    curve_runs,
                    table_runs,
                    compose_power,
                    use_dp,
                ),
            ) {
                (Ok(free), Ok(mut v)) => {
                    v["chainFree"] = free.into();
                    v
                }
                (Err(e), _) | (_, Err(e)) => {
                    serde_json::json!({"members":d.members,"snaps":d.snaps,"error":e.to_string()})
                }
            },
        );
    }
    fs::write(&args[5], serde_json::to_vec(&serde_json::json!({"decks":values}))?)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
