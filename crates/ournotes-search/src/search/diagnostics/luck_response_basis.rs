//! Conditional responses for interacting nominal minimum guarantees, with original native admission.
use super::luck_response::ResponseJob;
use super::luck_response_interactions::{IdentityRegistry, error_status, fingerprint_with_workspace};
use super::{Error, LuckInput};
use ournotes_sim::chartstats::luck_response::{Response, ResponseCurve};
use ournotes_sim::chartstats::{LuckTableMinimumTermResponse, luck_table_program, luck_table_program_virtual};
use ournotes_sim::live::full::LuckSkills;
use ournotes_sim::master::Master;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

pub(super) struct BasisOptions<'a> {
    pub identity_bytes: usize,
    pub program_bytes: usize,
    pub response_bytes: usize,
    pub max_terms: usize,
    pub propagate: bool,
    pub selected: Option<&'a [String]>,
    pub verify: bool,
}

/// Independent enclosures for the same law must intersect at every event time. Equality of rounded
/// endpoints is not required when arithmetic is reassociated. This diagnostic is not the algebraic proof.
fn compare(reference: &ResponseCurve, mixed: &ResponseCurve) -> Value {
    let times: BTreeSet<_> = reference.steps.iter().chain(&mixed.steps).map(|step| step.time_ms).collect();
    let mut positions = [0usize; 2];
    let mut current = [[[1.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]; 2];
    let (mut disjoint, mut compared, mut max_endpoint_difference) = (0usize, 0usize, 0f64);
    for time in times {
        for (side, curve) in [reference, mixed].into_iter().enumerate() {
            while positions[side] < curve.steps.len() && curve.steps[positions[side]].time_ms <= time {
                current[side] = curve.steps[positions[side]].buckets;
                positions[side] += 1;
            }
        }
        for (left, right) in current[0].into_iter().zip(current[1]) {
            compared += 1;
            disjoint += usize::from(left[0] > right[1] || right[0] > left[1]);
            max_endpoint_difference =
                max_endpoint_difference.max((left[0] - right[0]).abs()).max((left[1] - right[1]).abs());
        }
    }
    let probes_equal = reference.probes == mixed.probes;
    let masks_equal = reference.probe_transitions == mixed.probe_transitions;
    json!({"status":if disjoint == 0 && probes_equal && masks_equal {"success"} else {"mismatch"},
        "comparedBuckets":compared,"disjointIntervals":disjoint,"maximumEndpointDifference":max_endpoint_difference,
        "probesEqual":probes_equal,"probeTransitionsEqual":masks_equal,
        "comparison":"independent native joint law at the union of original event times"})
}

pub(super) fn identify_basis(
    master: &Master,
    input: &LuckInput,
    skills: &LuckSkills,
    neutral: Option<(i64, i64)>,
    jobs: &[ResponseJob],
    options: BasisOptions<'_>,
) -> Value {
    let began = Instant::now();
    let identity_limit = options.identity_bytes.min(256 << 20);
    let program_limit = options.program_bytes.min(256 << 20);
    let response_limit = options.response_bytes.min(256 << 20);
    let max_terms = options.max_terms.min(64);
    let mut registry = IdentityRegistry::new(jobs.len().saturating_mul(max_terms), identity_limit);
    let mut programs: Vec<Value> = Vec::new();
    let mut responses: Vec<Option<Arc<LuckTableMinimumTermResponse>>> = Vec::new();
    let mut rows = Vec::with_capacity(jobs.len());
    let (mut compiled, mut term_references, mut aliases, mut propagations, mut reconstructions) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut response_bytes, mut program_peak, mut identity_peak, mut fingerprint_peak) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut compile_ms, mut propagation_ms, mut mixture_ms, mut verification_ms) = (0f64, 0f64, 0f64, 0f64);
    let mut verification_calls = 0usize;
    for (ordinal, job) in jobs.iter().enumerate() {
        let started = Instant::now();
        let mut row =
            json!({"jobIndex":ordinal,"name":job.name,"entries":job.entries,"components":[],"status":"error"});
        let result = (|| -> Result<(), Error> {
            if job.entries.iter().any(|(key, _)| !skills.chain.contains(key)) {
                return Err(Error::Input("response entry is outside the LUCK chain catalogue".into()));
            }
            let before = Instant::now();
            let compile = if (0..5).all(|position| job.entries.iter().any(|(_, held)| *held == position)) {
                luck_table_program_virtual
            } else {
                luck_table_program
            };
            let program = compile(
                master,
                skills,
                neutral,
                &input.0.notes,
                input.0.params,
                &input.1,
                &input.0.play,
                &input.0.delta_times,
                &job.entries,
            )?;
            compiled += 1;
            compile_ms += before.elapsed().as_secs_f64() * 1000.0;
            let original_bytes =
                program.allocated_bytes().ok_or_else(|| Error::Capacity("original program allocation".into()))?;
            if original_bytes > program_limit {
                return Err(Error::Capacity("original program exceeds compiled program allowance".into()));
            }
            row["observerContract"] = json!(program.observer_contract());
            let reference = if options.verify && options.propagate {
                let before = Instant::now();
                verification_calls += 1;
                let curve = ResponseCurve::from_certified(&program.certified()?);
                verification_ms += before.elapsed().as_secs_f64() * 1000.0;
                Some(curve)
            } else {
                None
            };
            let before = Instant::now();
            let mut basis = program.start_minimum_basis(max_terms)?;
            compile_ms += before.elapsed().as_secs_f64() * 1000.0;
            let bytes =
                basis.allocated_bytes().ok_or_else(|| Error::Capacity("conditional basis allocation".into()))?;
            program_peak = program_peak.max(bytes);
            if bytes > program_limit {
                return Err(Error::Capacity("conditional basis exceeds compiled program allowance".into()));
            }
            row["startCount"] = json!(basis.start_count());
            row["termCount"] = json!(basis.term_count());
            row["compiledBytes"] = json!(bytes);
            let mut components = Vec::with_capacity(basis.term_count());
            let mut selected_indices = Vec::with_capacity(basis.term_count());
            for term in 0..basis.term_count() {
                term_references += 1;
                let identity = basis.term_identity(term)?;
                identity_peak = identity_peak.max(
                    identity
                        .allocated_bytes()
                        .ok_or_else(|| Error::Capacity("basis term identity allocation".into()))?,
                );
                let (fingerprint, workspace) = fingerprint_with_workspace(&identity)
                    .ok_or_else(|| Error::Capacity("basis term address allocation".into()))?;
                fingerprint_peak = fingerprint_peak.max(workspace);
                let index = if let Some(index) = registry.get(&fingerprint, &identity) {
                    aliases += 1;
                    index
                } else {
                    let index = programs.len();
                    let identity_version = identity.source_version.clone();
                    if !registry.insert(identity, fingerprint.clone(), index) {
                        return Err(Error::Capacity("conditional term identity allowance exhausted".into()));
                    }
                    let mut report = json!({"programIndex":index,"fingerprint":fingerprint,
                        "sourceVersion":ournotes_sim::SOURCE_SHA256,"identityVersion":identity_version,
                        "representativeJobIndex":ordinal,"representativeTermIndex":term,
                        "operatorContract":"conditional-start-minimum/1","status":"identified","response":null});
                    let propagate =
                        options.propagate && options.selected.is_none_or(|selected| selected.contains(&fingerprint));
                    let mut retained = None;
                    if propagate {
                        let before = Instant::now();
                        propagations += 1;
                        let generated = basis.certified_term(term);
                        let elapsed = before.elapsed().as_secs_f64() * 1000.0;
                        propagation_ms += elapsed;
                        report["propagationMs"] = json!(elapsed);
                        match generated {
                            Ok(response) => {
                                let bytes = response
                                    .allocated_bytes()
                                    .ok_or_else(|| Error::Capacity("conditional response allocation".into()))?;
                                if response_bytes.checked_add(bytes).is_none_or(|total| total > response_limit) {
                                    report["status"] = json!("capacity");
                                    report["error"] = json!("conditional response allowance exhausted");
                                } else {
                                    response_bytes += bytes;
                                    report["status"] = json!("success");
                                    report["response"] = json!(Response::Success {
                                        curve: ResponseCurve::from_certified(response.curve())
                                    });
                                    retained = Some(Arc::new(response));
                                }
                            }
                            Err(error) => {
                                report["status"] = json!(error_status(&error));
                                report["error"] = json!(error.to_string());
                            }
                        }
                    }
                    programs.push(report);
                    responses.push(retained);
                    index
                };
                let weight = basis.term_weight(term)?.interval();
                components.push(json!({"programIndex":index,"programFingerprint":fingerprint,
                    "weight":[weight.lower(),weight.upper()],"choices":basis.term_choices(term)?}));
                selected_indices.push(index);
            }
            row["components"] = json!(components);
            row["status"] = json!("identified");
            if selected_indices.iter().all(|&index| responses[index].is_some()) {
                let before = Instant::now();
                let terms: Vec<_> = selected_indices
                    .iter()
                    .map(|&index| responses[index].as_deref().expect("complete terms"))
                    .collect();
                let result = basis.reconstruct(&terms)?;
                mixture_ms += before.elapsed().as_secs_f64() * 1000.0;
                reconstructions += 1;
                let curve = ResponseCurve::from_certified(&result);
                if let Some(reference) = reference {
                    row["verification"] = compare(&reference, &curve);
                }
                row["response"] = json!(Response::Success { curve });
                row["status"] = json!("success");
            }
            Ok(())
        })();
        if let Err(error) = result {
            row["status"] = json!(error_status(&error));
            row["error"] = json!(error.to_string());
        }
        row["elapsedMs"] = json!(started.elapsed().as_secs_f64() * 1000.0);
        rows.push(row);
    }
    let identification_complete = rows.iter().all(|row| row["status"] == "identified" || row["status"] == "success");
    let missing_selected: Vec<_> = options
        .selected
        .into_iter()
        .flatten()
        .filter(|fingerprint| !programs.iter().any(|row| row["fingerprint"] == **fingerprint))
        .cloned()
        .collect();
    let propagation_complete = options.propagate
        && missing_selected.is_empty()
        && programs
            .iter()
            .filter(|row| {
                options
                    .selected
                    .is_none_or(|selected| row["fingerprint"].as_str().is_some_and(|f| selected.iter().any(|s| s == f)))
            })
            .all(|row| row["status"] == "success");
    let verification_requested = options.verify && options.propagate;
    let verified_jobs = rows.iter().filter(|row| row.get("verification").is_some()).count();
    let verification_complete = !verification_requested
        || (verified_jobs == jobs.len() && rows.iter().all(|row| row["verification"]["status"] == "success"));
    json!({"format":"ournotes-deck.luck-response-basis/1",
        "identificationComplete":identification_complete,"probabilityComplete":propagation_complete,
        "complete":identification_complete && missing_selected.is_empty() && (!options.propagate || propagation_complete) && verification_complete,
        "operatorContract":"conditional-start-minimum/1","law":"independentNominal",
        "nativeExpectationProven":false,"rankingProven":false,"usesMonteCarlo":false,
        "usesSingleSkillResponseComposition":false,"allSkillCombinationsPrecomputed":false,
        "missingSelectedPrograms":missing_selected,"jobs":rows,"programs":programs,
        "verificationRequested":verification_requested,"verificationComplete":verification_complete,
        "memoryScope":"Separate allowances for retained identities, one compiled basis and native response payloads. Response-index capacity and Arc counters are reported separately and excluded from the response payload allowance. Inputs, returned JSON and native DP workspace are not a total RSS bound.",
        "stats":{"requestedJobs":jobs.len(),"compiledJobs":compiled,"basisTermReferences":term_references,
            "uniquePrograms":responses.len(),"exactTermAliases":aliases,"propagationCalls":propagations,
            "reconstructedJobs":reconstructions,"compileMs":compile_ms,"propagationMs":propagation_ms,
            "mixtureMs":mixture_ms,"verificationCalls":verification_calls,"verifiedJobs":verified_jobs,"verificationMs":verification_ms,
            "identityBudgetBytes":identity_limit,"programBudgetBytes":program_limit,"responseBudgetBytes":response_limit,
            "retainedIdentityBytes":registry.bytes,"retainedResponseBytes":response_bytes,
            "responseIndexBytes":responses.capacity() * std::mem::size_of::<Option<Arc<LuckTableMinimumTermResponse>>>(),
            "responseArcCounterBytes":responses.iter().filter(|response| response.is_some()).count() * 2 * std::mem::size_of::<usize>(),
            "compiledProgramPeakBytes":program_peak,"temporaryIdentityPeakBytes":identity_peak,
            "fingerprintWorkspacePeakBytes":fingerprint_peak,"elapsedMs":began.elapsed().as_secs_f64()*1000.0}})
}
