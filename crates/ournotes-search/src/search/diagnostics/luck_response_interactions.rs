//! Compile complete native controller programs before grouping interacting skill combinations.
//!
//! No response is composed from single-skill curves. An alias must match the complete ordered
//! transcripts, including their native version. Fingerprints are addresses, never proof capabilities.
use super::luck_response::ResponseJob;
use super::{Error, LuckInput};
use ournotes_sim::chartstats::luck_response::{Response, ResponseCurve, response_fingerprint};
use ournotes_sim::chartstats::{LuckTableProgramIdentity, luck_table_program};
use ournotes_sim::live::full::LuckSkills;
use ournotes_sim::master::Master;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::mem::size_of;
use std::time::Instant;

const MAX_BYTES: usize = 256 << 20;
const FINGERPRINT_DOMAIN: &[u8] = b"ournotes-luck-controller-program\0v1\0";

/// Stable address of a complete native program. Lengths separate the source, batches and words;
/// the exact-word comparison below remains authoritative when assigning aliases.
pub(super) fn program_fingerprint(identity: &LuckTableProgramIdentity) -> Option<String> {
    fingerprint_with_workspace(identity).map(|(fingerprint, _)| fingerprint)
}

fn fingerprint_with_workspace(identity: &LuckTableProgramIdentity) -> Option<(String, usize)> {
    let bytes = FINGERPRINT_DOMAIN.len().checked_add(16)?.checked_add(identity.source_version.len())?;
    let bytes = identity
        .batches
        .iter()
        .try_fold(bytes, |sum, batch| sum.checked_add(8)?.checked_add(batch.len().checked_mul(size_of::<u64>())?))?;
    let mut encoded = Vec::new();
    encoded.try_reserve_exact(bytes).ok()?;
    encoded.extend_from_slice(FINGERPRINT_DOMAIN);
    encoded.extend_from_slice(&u64::try_from(identity.source_version.len()).ok()?.to_le_bytes());
    encoded.extend_from_slice(identity.source_version.as_bytes());
    encoded.extend_from_slice(&u64::try_from(identity.batches.len()).ok()?.to_le_bytes());
    for batch in &identity.batches {
        encoded.extend_from_slice(&u64::try_from(batch.len()).ok()?.to_le_bytes());
        for word in batch {
            encoded.extend_from_slice(&word.to_le_bytes());
        }
    }
    Some((response_fingerprint(&encoded), encoded.capacity()))
}

struct RetainedIdentity {
    identity: LuckTableProgramIdentity,
    fingerprint: String,
    program: usize,
}

/// Metadata is reserved once and its actual capacity is charged before any key is admitted. There
/// is no unaccounted tree/hash allocation or growth; the sorted vector moves ownership, not payloads.
struct IdentityRegistry {
    entries: Vec<RetainedIdentity>,
    bytes: usize,
    limit: usize,
}

impl IdentityRegistry {
    fn new(jobs: usize, limit: usize) -> Self {
        let mut entries = Vec::new();
        let slots = jobs.min(limit / size_of::<RetainedIdentity>());
        if entries.try_reserve_exact(slots).is_err() {
            return Self { entries: Vec::new(), bytes: 0, limit };
        }
        let bytes = entries.capacity().checked_mul(size_of::<RetainedIdentity>());
        match bytes.filter(|&bytes| bytes <= limit) {
            Some(bytes) => Self { entries, bytes, limit },
            None => Self { entries: Vec::new(), bytes: 0, limit },
        }
    }

    fn get(&self, fingerprint: &str, identity: &LuckTableProgramIdentity) -> Option<usize> {
        let start = self.entries.partition_point(|entry| entry.fingerprint.as_str() < fingerprint);
        self.entries[start..]
            .iter()
            .take_while(|entry| entry.fingerprint == fingerprint)
            .find(|entry| entry.identity == *identity)
            .map(|entry| entry.program)
    }

    fn insert(&mut self, identity: LuckTableProgramIdentity, fingerprint: String, program: usize) -> bool {
        if self.entries.len() == self.entries.capacity() {
            return false;
        }
        let Some(bytes) = identity
            .allocated_bytes()
            .and_then(|n| n.checked_sub(size_of::<LuckTableProgramIdentity>()))
            .and_then(|n| n.checked_add(fingerprint.capacity()))
            .and_then(|n| n.checked_add(self.bytes))
            .filter(|&n| n <= self.limit)
        else {
            return false;
        };
        let position = self.entries.partition_point(|entry| entry.fingerprint < fingerprint);
        self.entries.insert(position, RetainedIdentity { identity, fingerprint, program });
        self.bytes = bytes;
        true
    }
}

fn error_status(error: &Error) -> &'static str {
    match error {
        Error::Unsupported(_) => "unsupported",
        Error::Capacity(_) => "capacity",
        _ => "error",
    }
}

/// `input` is the resolved isolated response context, with its power normalized by the caller.
/// Both allowances are explicit: retained exact identities and one compiled native program. They
/// exclude caller-owned inputs, the returned JSON, and the simulator's separately bounded DP work.
/// A compiled program is measured before propagation; compilation keeps its original native guards.
#[allow(clippy::too_many_arguments)]
pub(super) fn identify_interactions(
    master: &Master,
    input: &LuckInput,
    skills: &LuckSkills,
    neutral: Option<(i64, i64)>,
    jobs: &[ResponseJob],
    identity_budget_bytes: usize,
    program_budget_bytes: usize,
    propagate_representatives: bool,
) -> Value {
    let started = Instant::now();
    let identity_limit = identity_budget_bytes.min(MAX_BYTES);
    let program_limit = program_budget_bytes.min(MAX_BYTES);
    let mut registry = IdentityRegistry::new(jobs.len(), identity_limit);
    let mut reports = Vec::with_capacity(jobs.len());
    let mut programs: Vec<Value> = Vec::new();
    let mut source_keys = BTreeSet::new();
    let (mut compiled, mut aliases, mut propagation_calls) = (0usize, 0usize, 0usize);
    let (mut compiled_peak, mut identity_peak, mut fingerprint_peak) = (0usize, 0usize, 0usize);
    let (mut compile_ms, mut propagation_ms) = (0f64, 0f64);
    for (ordinal, job) in jobs.iter().enumerate() {
        source_keys.insert(serde_json::to_string(&job.entries).expect("entry key JSON"));
        let begin = Instant::now();
        let mut row = json!({"jobIndex":ordinal,"name":job.name,"entries":job.entries,
            "programIndex":null,"isExactNativeExpectation":false,"isRankingCertificate":false});
        let result = (|| -> Result<(), Error> {
            if job.entries.iter().any(|(key, _)| !skills.chain.contains(key)) {
                return Err(Error::Input("response entry is outside the LUCK chain catalogue".into()));
            }
            let before = Instant::now();
            let program = luck_table_program(
                master,
                skills,
                neutral,
                &input.0.notes,
                input.0.params,
                &input.1,
                &input.0.play,
                &input.0.delta_times,
                &job.entries,
            );
            let elapsed = before.elapsed().as_secs_f64() * 1000.0;
            compile_ms += elapsed;
            row["compileMs"] = json!(elapsed);
            let program = program?;
            compiled += 1;
            let bytes =
                program.allocated_bytes().ok_or_else(|| Error::Capacity("compiled program byte count".into()))?;
            compiled_peak = compiled_peak.max(bytes);
            row["compiledBytes"] = json!(bytes);
            if bytes > program_limit {
                return Err(Error::Capacity("compiled program exceeds the interaction program allowance".into()));
            }
            let identity =
                program.identity().ok_or_else(|| Error::Capacity("complete program identity unavailable".into()))?;
            let identity_bytes =
                identity.allocated_bytes().ok_or_else(|| Error::Capacity("program identity byte count".into()))?;
            identity_peak = identity_peak.max(identity_bytes);
            row["identityBytes"] = json!(identity_bytes);
            let (fingerprint, fingerprint_bytes) = fingerprint_with_workspace(&identity)
                .ok_or_else(|| Error::Capacity("program fingerprint allocation".into()))?;
            fingerprint_peak = fingerprint_peak.max(fingerprint_bytes);
            row["programFingerprint"] = json!(fingerprint);
            if let Some(index) = registry.get(&fingerprint, &identity) {
                aliases += 1;
                row["programIndex"] = json!(index);
                row["representativeJobIndex"] = programs[index]["representativeJobIndex"].clone();
                row["status"] = programs[index]["status"].clone();
                row["reusedCompleteProgram"] = json!(true);
                if let Some(error) = programs[index].get("error") {
                    row["error"] = error.clone();
                }
                return Ok(());
            }
            let index = programs.len();
            let mut report = json!({"programIndex":index,"fingerprint":fingerprint,
                "sourceVersion":ournotes_sim::SOURCE_SHA256,"identityVersion":identity.source_version,
                "representativeJobIndex":ordinal,
                "identityBatchCount":identity.batches.len(),
                "identityWordCount":identity.batches.iter().map(Vec::len).sum::<usize>(),
                "identityBytes":identity_bytes,"compiledBytes":bytes,"status":"identified","response":null,
                "isExactNativeExpectation":false,"isRankingCertificate":false});
            if !registry.insert(identity, fingerprint, index) {
                return Err(Error::Capacity(
                    "complete program identity does not fit the interaction identity allowance".into(),
                ));
            }
            row["programIndex"] = json!(index);
            row["representativeJobIndex"] = json!(ordinal);
            row["reusedCompleteProgram"] = json!(false);
            if propagate_representatives {
                let before = Instant::now();
                propagation_calls += 1;
                match program.certified() {
                    Ok(result) => {
                        report["status"] = json!("success");
                        report["response"] = json!(Response::Success { curve: ResponseCurve::from_certified(&result) });
                    }
                    Err(error) => {
                        report["status"] = json!(error_status(&error));
                        report["error"] = json!(error.to_string());
                        if matches!(error, Error::Unsupported(_)) {
                            report["response"] = json!(Response::Unsupported { reason: error.to_string() });
                        }
                    }
                }
                let elapsed = before.elapsed().as_secs_f64() * 1000.0;
                propagation_ms += elapsed;
                report["propagationMs"] = json!(elapsed);
            }
            row["status"] = report["status"].clone();
            if let Some(error) = report.get("error") {
                row["error"] = error.clone();
            }
            programs.push(report);
            Ok(())
        })();
        if let Err(error) = result {
            row["status"] = json!(error_status(&error));
            row["error"] = json!(error.to_string());
        }
        row["elapsedMs"] = json!(begin.elapsed().as_secs_f64() * 1000.0);
        reports.push(row);
    }
    let identification_complete = reports.iter().all(|row| row["programIndex"].is_number());
    let probability_complete = propagate_representatives && reports.iter().all(|row| row["status"] == "success");
    let complete = if propagate_representatives { probability_complete } else { identification_complete };
    json!({"format":"ournotes-luck-interactions/1",
        "mode":if propagate_representatives {"programs"} else {"identify"},
        "complete":complete,"identificationComplete":identification_complete,"probabilityComplete":probability_complete,
        "value":"isolatedJointKernelProbability","law":"independentNominal",
        "isExactNativeExpectation":false,"isRankingCertificate":false,
        "hashAddressIsEquivalenceProof":false,
        "memoryScope":"Separate retained-identity and compiled-program allowances; temporary identity and fingerprint buffers, returned JSON, inputs and native DP workspace are reported separately or governed by their original limits, not a total RSS bound.",
        "usesSingleSkillResponseComposition":false,"usesMonteCarlo":false,
        "jobs":reports,"programs":programs,
        "stats":{"sourceJobs":jobs.len(),"distinctSourceKeys":source_keys.len(),
            "compiledJobs":compiled,"uniqueRetainedPrograms":registry.entries.len(),"exactProgramAliases":aliases,
            "propagationCalls":propagation_calls,"compileMs":compile_ms,"propagationMs":propagation_ms,
            "identityBudgetBytes":identity_limit,"programBudgetBytes":program_limit,
            "retainedIdentityBytes":registry.bytes,"temporaryIdentityPeakBytes":identity_peak,
            "fingerprintWorkspacePeakBytes":fingerprint_peak,
            "compiledProgramPeakBytes":compiled_peak,"elapsedMs":started.elapsed().as_secs_f64()*1000.0}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(batches: &[&[u64]]) -> LuckTableProgramIdentity {
        LuckTableProgramIdentity {
            source_version: "native-v1".into(),
            batches: batches.iter().map(|b| b.to_vec()).collect(),
        }
    }

    #[test]
    fn interaction_program_fingerprint_preserves_source_boundaries_order_and_native_bits() {
        let original = identity(&[&[1, 2], &[3]]);
        let expected = program_fingerprint(&original).unwrap();
        assert_eq!(Some(expected.clone()), program_fingerprint(&original));
        for changed in [
            identity(&[&[1], &[2, 3]]),
            identity(&[&[3], &[1, 2]]),
            identity(&[&[1, 2], &[3, 0]]),
            identity(&[&[1, 2], &[3], &[]]),
            identity(&[&[1, 2], &[3 ^ (1 << 63)]]),
        ] {
            assert_ne!(Some(&expected), program_fingerprint(&changed).as_ref());
        }
        let mut changed = original;
        changed.source_version.push('x');
        assert_ne!(Some(expected), program_fingerprint(&changed));
    }

    #[test]
    fn interaction_program_aliases_require_complete_identity_even_with_hash_collision() {
        let mut registry = IdentityRegistry::new(3, 8192);
        let first = identity(&[&[1, 2]]);
        let other = identity(&[&[2, 1]]);
        assert!(registry.insert(first.clone(), "forced-collision".into(), 10));
        assert_eq!(registry.get("forced-collision", &first), Some(10));
        assert_eq!(registry.get("forced-collision", &other), None);
        assert!(registry.insert(other.clone(), "forced-collision".into(), 20));
        assert_eq!(registry.get("forced-collision", &first), Some(10));
        assert_eq!(registry.get("forced-collision", &other), Some(20));
        let mut version = first;
        version.source_version.push('x');
        assert_eq!(registry.get("forced-collision", &version), None);
    }

    #[test]
    fn interaction_program_registry_charges_spare_capacity_and_preserves_prior_keys_on_refusal() {
        let mut registry = IdentityRegistry::new(2, 8192);
        let first = identity(&[&[1]]);
        let mut spare = identity(&[&[2]]);
        spare.batches[0].reserve_exact(100);
        let mut refused = identity(&[&[2]]);
        refused.batches[0].reserve_exact(100);
        let extra = refused.allocated_bytes().unwrap() - size_of::<LuckTableProgramIdentity>() + 1;
        assert!(registry.insert(first.clone(), "a".into(), 0));
        registry.limit = registry.bytes + extra - 1;
        assert!(!registry.insert(refused, "b".into(), 1));
        assert_eq!(registry.get("a", &first), Some(0));
        assert_eq!(registry.get("b", &spare), None);
        registry.limit = registry.bytes + spare.allocated_bytes().unwrap() - size_of::<LuckTableProgramIdentity>() + 1;
        assert!(registry.insert(spare, "b".into(), 1));
        assert_eq!(registry.bytes, registry.limit);
        assert!(!IdentityRegistry::new(1, 0).insert(first, "a".into(), 0));
    }
}
