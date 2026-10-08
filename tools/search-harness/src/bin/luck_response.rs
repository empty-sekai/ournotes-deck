//! Reproducible isolated LUCK responses from an unchanged real request, and portable archive inspection.
use ournotes_search::{
    handler,
    owned_snapshot::{GoalDependencies, OwnedSnapshot},
    search::diagnostics::{LuckResponseSpec, ResponseDeck, generate_luck_response, predict_luck_response},
    types::RecommendationRequest,
};
use ournotes_sim::{
    cards::Roster,
    chartstats::luck_response::{
        Quantization, Response, ResponseArchive, ResponseEntry, ResponseLimits, ResponseTable, response_fingerprint,
    },
    data::DeckData,
};
use serde_json::{Value, json};
use std::{env, fs, path::Path, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[path = "luck_response/minimum_basis.rs"]
mod minimum_basis;
#[path = "luck_response/programs.rs"]
mod programs;

fn mode(name: &str) -> Result<Quantization> {
    match name {
        "lossless" => Ok(Quantization::Lossless),
        "u16" => Ok(Quantization::U16),
        "u24" => Ok(Quantization::U24),
        "u32" => Ok(Quantization::U32),
        _ => Err(format!("unknown archive quantization {name}").into()),
    }
}

fn table(path: &str) -> Result<ResponseTable> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    Ok(serde_json::from_value(value.get("table").unwrap_or(&value).clone())?)
}

fn equal_pair(a: [f64; 2], b: [f64; 2]) -> bool {
    a.map(f64::to_bits) == b.map(f64::to_bits)
}

/// Check decoded inspection data against its source. This does not grant native admission or score proof.
fn check_response(source: &Response, decoded: &Response, lossless: bool) -> Result<f64> {
    let mut width = 0f64;
    match (source, decoded) {
        (Response::Unsupported { reason: a }, Response::Unsupported { reason: b }) if a == b => {}
        (Response::Success { curve: a }, Response::Success { curve: b }) => {
            if a.steps.len() != b.steps.len()
                || a.probes != b.probes
                || a.probe_transitions != b.probe_transitions
                || a.peak_states != b.peak_states
                || a.transitions != b.transitions
                || a.range_moments.len() != b.range_moments.len()
            {
                return Err("decoded response metadata differs".into());
            }
            for (a, b) in a.range_moments.iter().zip(&b.range_moments) {
                if !equal_pair(a.luck_points, b.luck_points)
                    || a.lot_results.into_iter().zip(b.lot_results).any(|(a, b)| !equal_pair(a, b))
                {
                    return Err("decoded range moments differ".into());
                }
            }
            for (a, b) in a.steps.iter().zip(&b.steps) {
                if a.time_ms != b.time_ms {
                    return Err("decoded response time differs".into());
                }
                for (a, b) in a.buckets.into_iter().zip(b.buckets) {
                    if (lossless && !equal_pair(a, b)) || b[0] > a[0] || b[1] < a[1] {
                        return Err("decoded probability fails exact/enclosing comparison".into());
                    }
                    width = width.max((a[0] - b[0]).max(b[1] - a[1]));
                }
            }
        }
        _ => return Err("decoded response status differs".into()),
    }
    Ok(width)
}

fn inspect(source: &ResponseTable, bytes: &[u8]) -> Result<Value> {
    let open = Instant::now();
    let archive = ResponseArchive::open(bytes, ResponseLimits::default())?;
    let open_ms = open.elapsed().as_secs_f64() * 1000.0;
    if archive.context() != &source.context || archive.keys().len() != source.entries.len() {
        return Err("archive context or key count differs from source".into());
    }
    // A deterministic shuffle exercises independent random lookup, retaining every original key exactly once.
    let mut order: Vec<_> = (0..source.entries.len()).collect();
    let mut state = 0x43525553485f4450u64;
    for end in (1..order.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        order.swap(end, (state % (end as u64 + 1)) as usize);
    }
    let begin = Instant::now();
    let mut decoded_peak = 0usize;
    let mut widening = 0f64;
    for index in order {
        let entry = &source.entries[index];
        let decoded = archive.lookup(&entry.key)?.ok_or("source key missing from archive")?;
        widening =
            widening.max(check_response(&entry.response, &decoded, archive.quantization() == Quantization::Lossless)?);
        decoded_peak = decoded_peak.max(decoded.allocated_bytes().ok_or("decoded byte count overflow")?);
    }
    Ok(json!({"mode":archive.quantization(),"bytes":bytes.len(),"sha256":response_fingerprint(bytes),
        "indexBytes":archive.index_bytes(),"decodedPeakBytes":decoded_peak,"openMs":open_ms,
        "lookupMs":begin.elapsed().as_secs_f64()*1000.0,"verifiedEntries":source.entries.len(),
        "maximumEndpointWidening":widening,"verification":"complete-source-comparison-in-shuffled-key-order",
        "nativeScoreCertificate":false}))
}

fn write_json(path: impl AsRef<Path>, value: &impl serde::Serialize) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn basename(path: &str) -> String {
    Path::new(path).file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn load_inputs(
    data_path: &str,
    roster_path: &str,
    request_path: &str,
) -> Result<(DeckData, Roster, RecommendationRequest, Value)> {
    let data = DeckData::from_path(data_path)?;
    let roster_text = fs::read_to_string(roster_path)?;
    let request_text = fs::read_to_string(request_path)?;
    let request: RecommendationRequest = serde_json::from_str(&request_text)?;
    let roster = if let Ok(snapshot) = OwnedSnapshot::from_json(&roster_text) {
        let resolution = snapshot.resolve_data(
            &data,
            data.sha256.as_deref().unwrap_or_default(),
            GoalDependencies::of(&request.execution),
        );
        resolution
            .resolved
            .ok_or_else(|| format!("snapshot unresolved: {:?} {:?}", resolution.missing, resolution.errors))?
            .diagnostic_projection()
            .clone()
    } else {
        Roster::from_json(&roster_text)?
    };
    let provenance = json!({"datasetSha256":data.sha256,"inputs":{
        "data":basename(data_path),"roster":basename(roster_path),"request":basename(request_path),
        "rosterSha256":response_fingerprint(roster_text.as_bytes()),
        "requestSha256":response_fingerprint(request_text.as_bytes())}});
    Ok((data, roster, request, provenance))
}

fn generate(args: &[String]) -> Result<()> {
    let (data, roster, request, provenance) = load_inputs(&args[0], &args[1], &args[2])?;
    let spec_text = fs::read_to_string(&args[3])?;
    let spec: LuckResponseSpec = serde_json::from_str(&spec_text)?;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let mut output = generate_luck_response(&built, &spec)?;
    for (key, value) in provenance.as_object().expect("input provenance object") {
        output["provenance"][key] = value.clone();
    }
    output["provenance"]["inputs"]["spec"] = json!(basename(&args[3]));
    output["provenance"]["inputs"]["specSha256"] = json!(response_fingerprint(spec_text.as_bytes()));
    // Preserve every completed job before optional packing. One archive failure cannot erase its DP output.
    write_json(&args[4], &output)?;
    if output["mode"] == "generate" {
        let table: ResponseTable = serde_json::from_value(output["table"].clone())?;
        let mut archives = Vec::new();
        for name in ["lossless", "u16", "u24", "u32"] {
            let path = format!("{}.{}.onlrsp", args[4], name);
            let packed = (|| -> Result<Value> {
                let began = Instant::now();
                let bytes = table.encode(mode(name)?, ResponseLimits::default())?;
                let encode_ms = began.elapsed().as_secs_f64() * 1000.0;
                let mut metadata = inspect(&table, &bytes)?;
                fs::write(&path, bytes)?;
                metadata["path"] = json!(path);
                metadata["encodeMs"] = json!(encode_ms);
                metadata["status"] = json!("success");
                Ok(metadata)
            })();
            archives.push(
                packed.unwrap_or_else(
                    |error| json!({"mode":name,"path":path,"status":"error","error":error.to_string()}),
                ),
            );
            output["archives"] = json!(archives);
            write_json(&args[4], &output)?;
        }
    }
    Ok(())
}

fn predict(args: &[String]) -> Result<()> {
    let began = Instant::now();
    let read = Instant::now();
    let bytes = fs::read(&args[0])?;
    let read_ms = read.elapsed().as_secs_f64() * 1000.0;
    let open = Instant::now();
    let archive = ResponseArchive::open(&bytes, ResponseLimits::default())?;
    let open_ms = open.elapsed().as_secs_f64() * 1000.0;
    let (data, roster, request, mut provenance) = load_inputs(&args[1], &args[2], &args[3])?;
    let deck_text = fs::read_to_string(&args[4])?;
    let decks: Vec<ResponseDeck> = serde_json::from_str(&deck_text)?;
    provenance["inputs"]["decks"] = json!(basename(&args[4]));
    provenance["inputs"]["decksSha256"] = json!(response_fingerprint(deck_text.as_bytes()));
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let mut output = predict_luck_response(&built, &archive, &decks)?;
    output["archive"] = json!({"name":basename(&args[0]),"sha256":response_fingerprint(&bytes),
        "bytes":bytes.len(),"readMs":read_ms,"openMs":open_ms,"indexBytes":archive.index_bytes()});
    output["provenance"] = provenance;
    output["totalElapsedMs"] = json!(began.elapsed().as_secs_f64() * 1000.0);
    write_json(&args[5], &output)
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("program-pack") if args.len() == 4 => programs::pack(&args[1..]),
        Some("program-materialize") if args.len() == 4 => programs::materialize(&args[1..]),
        Some("basis-materialize") if args.len() == 4 => minimum_basis::materialize(&args[1..]),
        Some("predict") if args.len() == 7 => predict(&args[1..]),
        Some("pack") if args.len() == 4 => {
            let table = table(&args[1])?;
            let bytes = table.encode(mode(&args[2])?, ResponseLimits::default())?;
            let mut report = inspect(&table, &bytes)?;
            fs::write(&args[3], bytes)?;
            report["path"] = json!(args[3]);
            println!("{}", serde_json::to_string(&report)?);
            Ok(())
        }
        Some("unpack") if args.len() == 3 => {
            let bytes = fs::read(&args[1])?;
            let archive = ResponseArchive::open(&bytes, ResponseLimits::default())?;
            let entries = archive
                .keys()
                .map(|key| {
                    Ok(ResponseEntry { key: key.clone(), response: archive.lookup(key)?.ok_or("indexed key missing")? })
                })
                .collect::<Result<Vec<_>>>()?;
            write_json(&args[2], &ResponseTable { context: archive.context().clone(), entries })
        }
        Some("verify") if args.len() == 4 => {
            let source = table(&args[1])?;
            let report = inspect(&source, &fs::read(&args[2])?)?;
            write_json(&args[3], &report)
        }
        _ if args.len() == 5 => generate(&args),
        _ => Err("luck_response DATA SNAPSHOT_OR_ROSTER REQUEST SPEC OUTPUT\n\
            luck_response program-pack PROGRAM_REPORT lossless|u16|u24|u32 OUTPUT_DIRECTORY\n\
            luck_response program-materialize IDENTIFICATION PROGRAM_INDEX OUTPUT_TABLE\n\
            luck_response predict ARCHIVE DATA SNAPSHOT_OR_ROSTER REQUEST DECKS OUTPUT\n\
            luck_response pack GENERATION_JSON lossless|u16|u24|u32 ARCHIVE\n\
            luck_response unpack ARCHIVE OUTPUT\n\
            luck_response verify GENERATION_JSON ARCHIVE OUTPUT"
            .into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
