//! Mix imported conditional response data; no external data constructs a native proof capability.
use super::*;
use ournotes_sim::chartstats::luck_response::{EntryKey, ResponseContext, ResponseCurve, ResponseStep};
use ournotes_sim::live::certified::{F64Interval, ProbabilityMass};
use std::collections::{BTreeMap, BTreeSet};

fn mixture(terms: &[(ProbabilityMass, ResponseCurve)]) -> Result<ResponseCurve> {
    let (_, first) = terms.first().ok_or("empty conditional mixture")?;
    if terms.len() > 64
        || terms.iter().any(|(_, curve)| {
            !curve.range_moments.is_empty()
                || curve.probes != first.probes
                || curve.probe_transitions.len() != first.probe_transitions.len()
        })
    {
        return Err("conditional response geometry, probes or moments differ".into());
    }
    let total = terms.iter().fold(F64Interval::ZERO, |sum, (weight, _)| sum.add(weight.interval()));
    if !total.contains(1.0) {
        return Err("conditional weights do not enclose unit mass".into());
    }
    let mut masks = vec![0; first.probe_transitions.len()];
    let mut transitions = 0u64;
    let mut peak_states = 0;
    let mut times = BTreeSet::new();
    for (weight, curve) in terms {
        if weight.interval().upper() == 0.0 {
            return Err("zero-weight conditional component".into());
        }
        if curve.steps.windows(2).any(|pair| pair[0].time_ms >= pair[1].time_ms) {
            return Err("conditional response times are not strictly ordered".into());
        }
        times.extend(curve.steps.iter().map(|step| step.time_ms));
        for (mask, &value) in masks.iter_mut().zip(&curve.probe_transitions) {
            *mask |= value;
        }
        transitions = transitions.checked_add(curve.transitions).ok_or("conditional transition count overflow")?;
        peak_states = peak_states.max(curve.peak_states);
    }
    let mut positions = vec![0usize; terms.len()];
    let mut current =
        vec![[ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO]; terms.len()];
    let mut steps = Vec::with_capacity(times.len());
    for time_ms in times {
        let mut buckets = [ProbabilityMass::ZERO; 4];
        for (index, (weight, curve)) in terms.iter().enumerate() {
            while positions[index] < curve.steps.len() && curve.steps[positions[index]].time_ms <= time_ms {
                for (target, pair) in current[index].iter_mut().zip(curve.steps[positions[index]].buckets) {
                    *target = ProbabilityMass::from_bounds(pair[0], pair[1])?;
                }
                positions[index] += 1;
            }
            for (target, value) in buckets.iter_mut().zip(current[index]) {
                *target = target.merge_disjoint(weight.multiply(value));
            }
        }
        steps.push(ResponseStep {
            time_ms,
            buckets: buckets.map(|value| {
                let bounds = value.interval();
                [bounds.lower(), bounds.upper()]
            }),
        });
    }
    Ok(ResponseCurve {
        steps,
        probe_transitions: masks,
        probes: first.probes.clone(),
        range_moments: Vec::new(),
        peak_states,
        transitions,
    })
}

pub(super) fn materialize(args: &[String]) -> Result<()> {
    let identification: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let index_file = Path::new(&args[1]);
    let index: Value = serde_json::from_slice(&fs::read(index_file)?)?;
    if identification["format"] != "ournotes-deck.luck-response-basis/1"
        || identification["identificationComplete"] != true
        || index["format"] != "ournotes-deck.luck-program-index/1"
    {
        return Err("expected complete native basis identification and program index".into());
    }
    let source = programs::digest(&identification, "sourceVersion")?;
    if programs::digest(&index, "sourceVersion")? != source {
        return Err("conditional program source differs".into());
    }
    let mut lookup = BTreeMap::new();
    for row in index["programs"].as_array().ok_or("missing indexed programs")? {
        if lookup.insert(programs::digest(row, "fingerprint")?, row).is_some() {
            return Err("duplicate indexed conditional program".into());
        }
    }
    let identified = identification["programs"].as_array().ok_or("missing conditional identities")?;
    let jobs = identification["jobs"].as_array().ok_or("missing conditional jobs")?;
    let limits = ResponseLimits::default();
    if jobs.is_empty() || jobs.len() > limits.max_entries {
        return Err("invalid conditional job count".into());
    }
    let mut entries = BTreeMap::new();
    let mut retained_bytes = 0usize;
    for job in jobs {
        if job["status"] != "identified" && job["status"] != "success" {
            return Err("unidentified conditional job".into());
        }
        let key: EntryKey = serde_json::from_value(job["entries"].clone())?;
        let components = job["components"].as_array().ok_or("missing conditional components")?;
        if components.is_empty() || components.len() > 64 {
            return Err("invalid conditional component count".into());
        }
        let key_text = serde_json::to_string(&key)?;
        let descriptor = serde_json::to_string(components)?;
        if let Some((previous, _)) = entries.get(&key_text) {
            if previous != &descriptor {
                return Err("duplicate source key identifies different conditional mixtures".into());
            }
            continue;
        }
        let mut terms = Vec::with_capacity(components.len());
        let mut decoded_bytes = retained_bytes;
        let mut seen = BTreeSet::new();
        for component in components {
            let ordinal = component["programIndex"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("missing conditional index")?;
            let program = identified.get(ordinal).ok_or("invalid conditional index")?;
            let fingerprint = programs::digest(program, "fingerprint")?;
            if programs::digest(program, "sourceVersion")? != source
                || component["programFingerprint"] != fingerprint
                || !seen.insert(fingerprint.clone())
            {
                return Err("conditional source, identity mapping or multiplicity differs".into());
            }
            let weight: [f64; 2] = serde_json::from_value(component["weight"].clone())?;
            let weight = ProbabilityMass::from_bounds(weight[0], weight[1])?;
            let row = lookup.get(&fingerprint).ok_or_else(|| format!("missing conditional program {fingerprint}"))?;
            let response = programs::read_response(index_file, row, &source, &index["mode"])?;
            decoded_bytes = decoded_bytes
                .checked_add(response.allocated_bytes().ok_or("conditional response allocation")?)
                .ok_or("conditional response allocation overflow")?;
            if decoded_bytes > limits.max_decoded_bytes {
                return Err("conditional decoding exceeds its response allowance".into());
            }
            let Response::Success { curve } = response else {
                return Err("unavailable conditional response".into());
            };
            terms.push((weight, curve));
        }
        let response = Response::Success { curve: mixture(&terms)? };
        retained_bytes = retained_bytes
            .checked_add(response.allocated_bytes().ok_or("mixed response allocation")?)
            .ok_or("mixed response allocation overflow")?;
        if retained_bytes > limits.max_decoded_bytes {
            return Err("materialized responses exceed their allowance".into());
        }
        entries.insert(key_text, (descriptor, ResponseEntry { key, response }));
    }
    let context: ResponseContext = serde_json::from_value(identification["context"].clone())?;
    let table = ResponseTable { context, entries: entries.into_values().map(|(_, entry)| entry).collect() };
    write_json(
        &args[2],
        &json!({"format":"ournotes-deck.luck-response-materialized/1","table":table,
        "sourceVersion":source,"operatorContract":"conditional-start-minimum/1",
        "nativeExpectationProven":false,"rankingProven":false,
        "retainedResponseBytes":retained_bytes,"usesMonteCarlo":false,"propagationCalls":0}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(time: i32, bucket: usize) -> ResponseCurve {
        let mut buckets = [[0.0, 0.0]; 4];
        buckets[bucket] = [1.0, 1.0];
        ResponseCurve {
            steps: vec![ResponseStep { time_ms: time, buckets }],
            probe_transitions: vec![1, 2],
            probes: vec![true],
            range_moments: vec![],
            peak_states: 1,
            transitions: 1,
        }
    }

    #[test]
    fn conditional_import_mixes_union_times_and_requires_unit_mass() {
        let half = ProbabilityMass::from_f32(0.5).unwrap();
        let result = mixture(&[(half, curve(10, 1)), (half, curve(20, 3))]).unwrap();
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.steps[0].buckets, [[0.5, 0.5], [0.5, 0.5], [0.0, 0.0], [0.0, 0.0]]);
        assert_eq!(result.steps[1].buckets, [[0.0, 0.0], [0.5, 0.5], [0.0, 0.0], [0.5, 0.5]]);
        assert!(mixture(&[(half, curve(10, 1))]).is_err());
        let mut different = curve(20, 3);
        different.probes.push(true);
        assert!(mixture(&[(half, curve(10, 1)), (half, different)]).is_err());
    }
}
