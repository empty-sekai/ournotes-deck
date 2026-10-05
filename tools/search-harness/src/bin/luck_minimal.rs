//! A closed five-member/no-Snap production domain, or a separately labelled five-pair leaf microbenchmark.
use ournotes_search::{
    engine,
    types::{DeckInput, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, live::full::take_luck_score_profile};
use std::{env, fs};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 6 || !matches!(args[0].as_str(), "closed" | "pairs") {
        return Err("luck_minimal closed|pairs DATA ROSTER REQUEST DECKS OUTPUT".into());
    }
    let data = DeckData::from_path(&args[1])?;
    let mut roster = Roster::from_json(&fs::read_to_string(&args[2])?)?;
    let mut request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let decks: Vec<DeckInput> = serde_json::from_str(&fs::read_to_string(&args[4])?)?;
    let deck = decks.first().ok_or("no declared formation")?;
    let closed = args[0] == "closed";
    request.k = 5;
    request.constraints.leader = None;
    request.constraints.include_members = deck.members.to_vec();
    request.initial_decks.clear();
    request.limits.time_limit_ms = Some(60_000);
    request.limits.max_candidates = None;
    request.limits.cache_entries = request.limits.cache_entries.max(64);
    if closed {
        roster.members.retain(|member| deck.members.contains(&member.id));
        if roster.members.len() != 5 {
            return Err("the closed domain needs exactly the five real owned member cards".into());
        }
        roster.snaps.clear();
        request.constraints.no_snaps = true;
        request.constraints.exclude_snaps.clear();
        request.strategy = Strategy::BranchAndBound;
    } else {
        request.strategy = Strategy::Candidate { power_seeds: 0, proposals: 0, proposal_seed: 1 };
        for leader in 0..5 {
            let mut candidate = deck.clone();
            candidate.members.swap(2, leader);
            candidate.snaps.swap(2, leader);
            request.initial_decks.push(candidate);
        }
    }
    take_luck_score_profile();
    let started = std::time::Instant::now();
    let result = engine::recommend(&data, &roster, &request);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1e3;
    let profile = take_luck_score_profile();
    let result = match result {
        Ok(outcome) => serde_json::json!({"status":"evaluated","outcome":outcome}),
        Err(error) => serde_json::json!({"status":"error","error":error.to_string()}),
    };
    let output = serde_json::json!({
        "format":"ournotes-deck.luck-minimal-profile/1",
        "scope":if closed {
            "Formal branch-and-bound request: five real owned members, no Snaps, five leaders, all 120 internal orders; no candidate cap"
        } else {
            "Diagnostic only: evaluate the five declared member/Snap-pair leaders in one production Engine and its real cache; no claim about the original global candidate domain"
        },
        "mode":args[0],"selectedMembers":deck.members,"requestTimeLimitMs":60_000,
        "elapsedMs":elapsed_ms,"profile":profile,"result":result,
    });
    fs::write(&args[5], serde_json::to_vec_pretty(&output)?)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
