//! Command-line front end: `ournotes-deck <power|skip|live> --data FILE --roster FILE [options]`.

use std::process::ExitCode;
use std::time::Duration;

use ournotes_deck::cards::Roster;
use ournotes_deck::data::DeckData;
use ournotes_deck::live::model::Play;
use ournotes_deck::search::{Constraints, Objective, Pool, SearchRequest, search};
use serde_json::json;

const USAGE: &str = "usage:
  ournotes-deck power --data FILE --roster FILE [--music ID] [--event] [common options]
  ournotes-deck skip  --data FILE --roster FILE --score ID [common options]
  ournotes-deck live  --data FILE --roster FILE --score ID --exclude-snap-skills [--play FILE] [--event]
                      [common options]
--data is a deck data file (nnnotes.deck-data/1); --play defaults to the theoretical best play.
common options: -k N (default 10), --leader ID, --include ID[,ID...], --exclude ID[,ID...],
                --exclude-snaps ID[,ID...], --no-snaps, --time-limit-ms N";

fn ids(s: &str) -> Result<Vec<i64>, String> {
    s.split(',').filter(|x| !x.is_empty()).map(|x| x.trim().parse().map_err(|_| format!("bad id {x:?}"))).collect()
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn run(args: &[String]) -> Result<serde_json::Value, String> {
    let cmd = args.first().ok_or(USAGE)?.as_str();
    let mut data = None;
    let mut roster = None;
    let mut music = None;
    let mut score = None;
    let mut play = None;
    let mut event = false;
    let mut exclude_snap_skills = false;
    let mut k = 10usize;
    let mut c = Constraints::default();
    let mut limit = None;
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
            "--music" => music = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--score" => score = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--play" => play = Some(val()?),
            "--event" => event = true,
            "--exclude-snap-skills" => exclude_snap_skills = true,
            "-k" => k = val()?.parse().map_err(|_| "bad -k".to_string())?,
            "--leader" => c.leader = Some(val()?.parse().map_err(|_| "bad --leader".to_string())?),
            "--include" => c.include_members = ids(&val()?)?,
            "--exclude" => c.exclude_members = ids(&val()?)?,
            "--exclude-snaps" => c.exclude_snaps = ids(&val()?)?,
            "--no-snaps" => c.no_snaps = true,
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
    let data = DeckData::from_path(data.ok_or("--data is required")?).map_err(|e| e.to_string())?;
    let roster = Roster::from_json(&read(&roster.ok_or("--roster is required")?)?).map_err(|e| e.to_string())?;
    let objective = match cmd {
        "power" => Objective::Power { music_id: music, event },
        "skip" => {
            let score_id = score.ok_or("--score is required")?;
            Objective::SkipScore { score_id, chart: data.chart(score_id).map_err(|e| e.to_string())? }
        }
        _ => {
            let score_id = score.ok_or("--score is required")?;
            let chart = data.chart(score_id).map_err(|e| e.to_string())?;
            let play = match play {
                Some(p) => serde_json::from_str::<Play>(&read(&p)?).map_err(|e| format!("play: {e}"))?,
                None => Play::theoretical_best(&data.master, &chart).map_err(|e| e.to_string())?,
            };
            Objective::LiveScore { score_id, chart, play, event, exclude_snap_skills }
        }
    };
    let pool = Pool::new(&data.master, &roster).map_err(|e| e.to_string())?;
    let out =
        search(&pool, &SearchRequest { objective, k, constraints: c, time_limit: limit }).map_err(|e| e.to_string())?;
    Ok(json!({
        "completion": out.completion,
        "results": out.results,
        "stats": out.stats,
        "elapsedMs": out.elapsed.as_secs_f64() * 1e3,
    }))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
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
