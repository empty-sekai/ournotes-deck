//! Production JSON boundary: one recommendation request in, one recommendation result out.
use ournotes_search::{engine::recommend, types::RecommendationRequest};
use ournotes_sim::{Error, cards::Roster, data::DeckData};
use std::{path::PathBuf, process::ExitCode};
fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let (mut data, mut roster, mut request, mut output) = (None, None, None, None);
    while let Some(a) = args.next() {
        if a == "--help" || a == "-h" {
            println!(
                "ournotes-recommend --data DECK_DATA.json --roster ROSTER.json --request REQUEST.json [-o RESULT.json]"
            );
            return Ok(());
        }
        let v = args.next().ok_or_else(|| Error::Input(format!("{a} requires a path")))?;
        match a.as_str() {
            "--data" => data = Some(v),
            "--roster" => roster = Some(v),
            "--request" => request = Some(v),
            "-o" | "--out" => output = Some(PathBuf::from(v)),
            _ => return Err(Error::Input(format!("unknown option {a}"))),
        }
    }
    let read = |p: String| std::fs::read_to_string(&p).map_err(|e| Error::Input(format!("{p}: {e}")));
    let d = DeckData::from_path(data.ok_or_else(|| Error::Input("--data is required".into()))?)?;
    let roster = roster.ok_or_else(|| Error::Input("--roster is required".into()))?;
    let text = read(roster.clone())?;
    check_mock_source(&roster, &text, &d)?;
    let r = Roster::from_json(&text)?;
    let q: RecommendationRequest =
        serde_json::from_str(&read(request.ok_or_else(|| Error::Input("--request is required".into()))?)?)
            .map_err(|e| Error::Input(format!("request: {e}")))?;
    let value = recommend(&d, &r, &q)?;
    let text = serde_json::to_string_pretty(&value).map_err(|e| Error::Domain(format!("result JSON: {e}")))?;
    if let Some(p) = output {
        std::fs::write(&p, text).map_err(|e| Error::Input(format!("{}: {e}", p.display())))?
    } else {
        println!("{text}")
    };
    Ok(())
}
/// A mock roster (tools/search-harness/mock_rosters.py) records the provenance hashes of the master tables it was
/// generated from, null for a table the data did not have; the deck data must have the same.
fn check_mock_source(path: &str, text: &str, data: &DeckData) -> Result<(), Error> {
    let roster: serde_json::Value = serde_json::from_str(text).map_err(|e| Error::Input(format!("roster: {e}")))?;
    let Some(source) = roster.get("mockSource") else { return Ok(()) };
    let tables =
        source["tables"].as_object().ok_or_else(|| Error::Input(format!("{path}: mockSource without tables")))?;
    for (name, expected) in tables {
        let actual = &data.provenance["master"]["tables"][name]["sha256"];
        if actual != expected {
            return Err(Error::Input(format!(
                "{path}: mock roster generated from {name} {expected}, the deck data has {actual}; regenerate the mock rosters"
            )));
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let code = match &e {
                Error::Input(_) => "Input",
                Error::Master(_) => "Master",
                Error::Game(_) => "Game",
                Error::Unsupported(_) => "Unsupported",
                Error::Domain(_) => "Domain",
                Error::Capacity(_) => "Capacity",
            };
            eprintln!(
                "{}",
                serde_json::json!({"format":"ournotes-deck.recommendation-error/1","error":{"code":code,"message":e.to_string()}})
            );
            ExitCode::from(2)
        }
    }
}
