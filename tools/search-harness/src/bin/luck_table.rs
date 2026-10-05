//! The lottery-related skills of a deck data file's master and the LUCK tables of some of its charts, with the time
//! each table took.
use ournotes_sim::chartstats::{self, LuckOptions};
use ournotes_sim::data::DeckData;
use ournotes_sim::live::full::luck_skills;
use std::{env, fs, time::Instant};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("luck_table DATA SCORE_IDS(comma-separated) RUNS ALL_POSITIONS(0|1) OUTPUT".into());
    }
    let data = DeckData::from_path(&args[0])?;
    let ids: Vec<i64> = args[1].split(',').map(str::parse).collect::<Result<_, _>>()?;
    let runs: usize = args[2].parse()?;
    let all_positions = args[3] == "1";
    let skills = luck_skills(&data.master)?;
    let options = LuckOptions { runs, all_positions };
    let mut charts = Vec::new();
    for id in ids {
        let chart = data.data_chart(id).ok_or_else(|| format!("no chart {id}"))?;
        let t = Instant::now();
        let stats = chartstats::diagnostic_luck_table(&data.master, chart, &options);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        charts.push(match stats {
            Ok(table) => serde_json::json!({"scoreId": id, "ms": ms, "luck": table}),
            Err(e) => serde_json::json!({"scoreId": id, "ms": ms, "error": e.to_string()}),
        });
    }
    fs::write(
        &args[4],
        serde_json::to_vec(
            &serde_json::json!({"model":chartstats::LUCK_TABLE_MODEL,"skills": skills, "charts": charts}),
        )?,
    )?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
