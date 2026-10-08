//! Program-addressed inspection storage. A hash or imported response is never a native proof capability.
use super::*;
use ournotes_sim::chartstats::luck_response::{EntryKey, ResponseContext};
use std::collections::{BTreeMap, BTreeSet};

const INDEX_FORMAT: &str = "ournotes-deck.luck-program-index/1";
const REPORT_FORMAT: &str = "ournotes-deck.luck-response-programs/1";
const ALGORITHM: &str = "ournotes-luck-program-response/1";

fn digest(value: &Value, field: &str) -> Result<String> {
    let text = value[field].as_str().ok_or_else(|| format!("missing {field}"))?;
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(format!("invalid {field}").into());
    }
    Ok(text.to_owned())
}

fn context(fingerprint: &str, source: &str) -> ResponseContext {
    ResponseContext { fingerprint: fingerprint.to_owned(), algorithm_version: format!("{ALGORITHM}/{source}") }
}

/// One record per complete native program, independent of the number of skill-combination aliases.
pub(super) fn pack(args: &[String]) -> Result<()> {
    let raw = fs::read(&args[0])?;
    let report: Value = serde_json::from_slice(&raw)?;
    if report["format"] != REPORT_FORMAT {
        return Err("expected native program response report".into());
    }
    let source = digest(&report, "sourceVersion")?;
    let quantization = mode(&args[1])?;
    let directory = Path::new(&args[2]);
    fs::create_dir_all(directory.join("blobs"))?;
    let programs = report["programs"].as_array().ok_or("missing programs")?;
    let jobs = report["jobs"].as_array().ok_or("missing original job coverage")?;
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(programs.len());
    let mut complete = !jobs.is_empty();
    for program in programs {
        let fingerprint = digest(program, "fingerprint")?;
        if digest(program, "sourceVersion")? != source || !seen.insert(fingerprint.clone()) {
            return Err("program source differs or duplicate program fingerprint".into());
        }
        let packed = (|| -> Result<Value> {
            let response: Response = serde_json::from_value(program["response"].clone())?;
            let success = matches!(response, Response::Success { .. });
            let table = ResponseTable {
                context: context(&fingerprint, &source),
                entries: vec![ResponseEntry { key: vec![], response }],
            };
            let bytes = table.encode(quantization, ResponseLimits::default())?;
            let mut archive = inspect(&table, &bytes)?;
            let file = format!("blobs/{}.onlrsp", response_fingerprint(&bytes));
            fs::write(directory.join(&file), &bytes)?;
            archive["path"] = json!(file);
            Ok(json!({"fingerprint":fingerprint,"sourceVersion":source,
                "status":if success {"success"} else {"unsupported"},"archive":archive}))
        })();
        let row = packed.unwrap_or_else(|error| {
            json!({"fingerprint":fingerprint,"sourceVersion":source,
            "status":"error","error":error.to_string()})
        });
        complete &= row["status"] == "success";
        rows.push(row);
    }
    for job in jobs {
        complete &= job["status"] == "success"
            && job["programIndex"].as_u64().and_then(|i| usize::try_from(i).ok()).is_some_and(|i| i < rows.len());
    }
    let index = json!({"format":INDEX_FORMAT,"mode":quantization,"sourceVersion":source,
        "complete":complete,"inputReportSha256":response_fingerprint(&raw),
        "requestedJobs":jobs.len(),"programs":rows,"nativeExpectationProven":false,"rankingProven":false,
        "scope":"Complete recorded native controller identities address probability response data; hashes and imported archives confer no native proof capability."});
    let temporary = directory.join("program-index.json.tmp");
    write_json(&temporary, &index)?;
    fs::rename(temporary, directory.join("program-index.json"))?;
    println!("{}", serde_json::to_string(&index)?);
    if !complete {
        return Err("program response coverage or archive packing is incomplete".into());
    }
    Ok(())
}

fn read_response(index_file: &Path, row: &Value, expected_source: &str, expected_mode: &Value) -> Result<Response> {
    let fingerprint = digest(row, "fingerprint")?;
    if digest(row, "sourceVersion")? != expected_source || row["status"] != "success" {
        return Err("program response source or status differs".into());
    }
    let archive = &row["archive"];
    if &archive["mode"] != expected_mode {
        return Err("program record quantization differs from index".into());
    }
    let sha = digest(archive, "sha256")?;
    let relative = format!("blobs/{sha}.onlrsp");
    if archive["path"].as_str() != Some(relative.as_str()) {
        return Err("noncanonical program blob path".into());
    }
    let path = index_file.parent().unwrap_or_else(|| Path::new(".")).join(relative);
    let size = fs::metadata(&path)?.len();
    let limits = ResponseLimits::default();
    if size > limits.max_archive_bytes as u64 || archive["bytes"].as_u64() != Some(size) {
        return Err("program blob size differs or exceeds limit".into());
    }
    let bytes = fs::read(path)?;
    if response_fingerprint(&bytes) != sha {
        return Err("program blob checksum differs".into());
    }
    let decoded = ResponseArchive::open(&bytes, limits)?;
    if decoded.context() != &context(&fingerprint, expected_source)
        || decoded.keys().len() != 1
        || json!(decoded.quantization()) != *expected_mode
    {
        return Err("program blob identity differs".into());
    }
    decoded.lookup(&vec![])?.ok_or_else(|| "program response key missing".into())
}

/// Materialize only requested aliases after native identification. This is a convenient bridge to the
/// existing key-addressed prediction reader, not a global enumeration of possible combinations.
pub(super) fn materialize(args: &[String]) -> Result<()> {
    let identification: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let index_file = Path::new(&args[1]);
    let index: Value = serde_json::from_slice(&fs::read(index_file)?)?;
    if identification["format"] != REPORT_FORMAT || index["format"] != INDEX_FORMAT {
        return Err("expected native identification and program index".into());
    }
    let source = digest(&identification, "sourceVersion")?;
    if digest(&index, "sourceVersion")? != source {
        return Err("program index uses a different native source version".into());
    }
    let mut lookup = BTreeMap::new();
    for row in index["programs"].as_array().ok_or("missing indexed programs")? {
        let fingerprint = digest(row, "fingerprint")?;
        if lookup.insert(fingerprint, row).is_some() {
            return Err("duplicate indexed program".into());
        }
    }
    let programs = identification["programs"].as_array().ok_or("missing identified programs")?;
    let jobs = identification["jobs"].as_array().ok_or("missing identified jobs")?;
    let limits = ResponseLimits::default();
    if jobs.len() > limits.max_entries {
        return Err("too many requested program aliases".into());
    }
    let mut entries: BTreeMap<String, (String, ResponseEntry)> = BTreeMap::new();
    let mut total_decoded_bytes = 0usize;
    for job in jobs {
        let number = job["programIndex"].as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("unidentified job")?;
        let program = programs.get(number).ok_or("invalid program index")?;
        let fingerprint = digest(program, "fingerprint")?;
        if digest(program, "sourceVersion")? != source {
            return Err("identified program source differs".into());
        }
        let key: EntryKey = serde_json::from_value(job["entries"].clone())?;
        let key_text = serde_json::to_string(&key)?;
        if let Some((previous, _)) = entries.get(&key_text) {
            if previous != &fingerprint {
                return Err("duplicate source key identifies different programs".into());
            }
            continue;
        }
        let row = lookup.get(&fingerprint).ok_or_else(|| format!("missing program {fingerprint}"))?;
        let response = read_response(index_file, row, &source, &index["mode"])?;
        total_decoded_bytes = total_decoded_bytes
            .checked_add(response.allocated_bytes().ok_or("decoded size overflow")?)
            .ok_or("materialized size overflow")?;
        if total_decoded_bytes > limits.max_decoded_bytes {
            return Err("requested alias materialization exceeds decoded allowance".into());
        }
        entries.insert(key_text, (fingerprint, ResponseEntry { key, response }));
    }
    let table = ResponseTable {
        context: serde_json::from_value(identification["context"].clone())?,
        entries: entries.into_values().map(|(_, entry)| entry).collect(),
    };
    write_json(
        &args[2],
        &json!({"format":"ournotes-deck.luck-response-materialization/1",
        "table":table,"isScorePrediction":true,"nativeExpectationProven":false,"rankingProven":false,
        "scope":"Requested native-identified aliases from persistent program responses; no DP or Monte Carlo during materialization.",
        "decodedBytes":total_decoded_bytes}),
    )
}
