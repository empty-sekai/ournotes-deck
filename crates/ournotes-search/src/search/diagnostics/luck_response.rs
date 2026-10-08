//! Offline inspection data from the real resolved request. No result grants search admission or a score proof.
use super::{BuiltProblem, Error, LuckInput, luck_input};
use ournotes_sim::chartstats::luck_response::{
    EntryKey, Response, ResponseArchive, ResponseContext, ResponseCurve, ResponseEntry, ResponseTable,
    response_fingerprint,
};
use ournotes_sim::chartstats::{
    luck_neutral, luck_table_dp_certified_cached, luck_table_steps, luck_table_validate, luck_table_validate_virtual,
};
use ournotes_sim::live::full::{
    LuckDpCache, LuckSkillKey, LuckSkills, LuckSource, Performer, luck_rush_dp_certified_with_ranking,
    luck_rush_dp_with_ranking, luck_skill_key, luck_skills,
};
use ournotes_sim::live::random::LiveRandom;
use ournotes_sim::master::Master;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const ALGORITHM: &str = "ournotes-luck-response/1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseDeck {
    #[serde(default)]
    pub name: String,
    pub members: [i64; 5],
    pub snaps: [Option<i64>; 5],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseJob {
    pub name: String,
    pub entries: EntryKey,
    #[serde(default)]
    pub mc_runs: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseMode {
    Plan,
    Identify,
    Programs,
    BasisIdentify,
    BasisPrograms,
    #[default]
    Generate,
}

fn default_cache_entries() -> usize {
    4096
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LuckResponseSpec {
    #[serde(default)]
    pub mode: ResponseMode,
    #[serde(default)]
    pub context_deck: Option<ResponseDeck>,
    pub jobs: Vec<ResponseJob>,
    #[serde(default)]
    pub validation_decks: Vec<ResponseDeck>,
    #[serde(default)]
    pub mc_runs: u32,
    #[serde(default)]
    pub score_samples: u32,
    #[serde(default = "default_cache_entries")]
    pub cache_entries: usize,
    /// Explicit byte allowance takes precedence over the compatibility entry estimate.
    #[serde(default)]
    pub cache_bytes: Option<usize>,
    /// Separate budgets for complete identities and the current compiled controller.
    #[serde(default)]
    pub identity_bytes: Option<usize>,
    #[serde(default)]
    pub program_bytes: Option<usize>,
    /// Experimental exact-CDF quotient for contiguous nominal start-minimum operators.
    #[serde(default)]
    pub canonical_start_minimum: bool,
    /// Optional conditional-minimum experiment; the native implementation always caps this at 64.
    #[serde(default)]
    pub basis_max_terms: Option<usize>,
    #[serde(default)]
    pub basis_response_bytes: Option<usize>,
    /// In basisPrograms mode propagate only these already identified term addresses; None means all.
    #[serde(default)]
    pub basis_programs: Option<Vec<String>>,
    /// Independent original-program comparison is separately counted and never hidden in timings.
    #[serde(default)]
    pub verify_basis: bool,
    /// Optional inspection output; persistent queries need only the reusable conditional curves.
    #[serde(default)]
    pub basis_reconstruct: bool,
}

fn fingerprint(value: &Value) -> String {
    response_fingerprint(&serde_json::to_vec(value).expect("JSON dependency value"))
}

fn response_context(shared: &str, entries: BTreeMap<String, String>) -> ResponseContext {
    ResponseContext {
        fingerprint: fingerprint(&json!({"shared":shared,"entries":entries})),
        algorithm_version: format!("{ALGORITHM}/{}", ournotes_sim::SOURCE_SHA256),
    }
}

fn validate_entries(
    master: &Master,
    skills: &LuckSkills,
    neutral: Option<(i64, i64)>,
    entries: &EntryKey,
) -> Result<(), Error> {
    if entries.iter().any(|(key, _)| !skills.chain.contains(key)) {
        return Err(Error::Input("response entry is outside the LUCK chain catalogue".into()));
    }
    luck_table_validate(master, skills, neutral, entries)
}

fn first_deck(built: &BuiltProblem<'_>) -> Result<ResponseDeck, Error> {
    let domain = built.domain();
    let pool = &built.pool;
    let mut chosen = domain.required().to_vec();
    for &member in domain.members() {
        if chosen.len() == 5 {
            break;
        }
        if chosen.iter().all(|&other| pool.members[other].character_id != pool.members[member].character_id) {
            chosen.push(member);
        }
    }
    let mut members: [usize; 5] = chosen.try_into().map_err(|_| Error::Input("no legal context deck".into()))?;
    if let Some(leader) = domain.leader() {
        let index = members
            .iter()
            .position(|&member| member == leader)
            .ok_or_else(|| Error::Input("context deck omitted required leader".into()))?;
        members.swap(index, 2);
    }
    Ok(ResponseDeck {
        name: "firstLegalContext".into(),
        members: members.map(|m| pool.members[m].id),
        snaps: [None; 5],
    })
}

/// Preserve selected source rows, native source order, and the complete condition/target closure. Debug
/// images are version-bound by SOURCE_SHA256; explicit activation bits retain signed zero/nonfinite bits.
fn source_dependencies(master: &Master, entries: &EntryKey) -> Value {
    let mut groups = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut cumulative = BTreeSet::new();
    let sources: Vec<_> = entries
        .iter()
        .map(|(key, position)| {
            let metadata = match key.source {
                LuckSource::Gekisou => master.gekisou_skill(key.id),
                LuckSource::GekisouSupport => master.gekisou_support_skill(key.id),
            };
            let rows: Vec<_> = key
                .source
                .rows(master)
                .iter()
                .filter(|row| row.skill_id == key.id && row.level == key.level)
                .map(|row| {
                    groups.extend([
                        row.skill_trigger_condition_group,
                        row.skill_condition_group,
                        row.skill_release_condition_group,
                        row.effect_execute_limit_reset_condition_group,
                    ]);
                    targets.extend(row.skill_target_ids.iter().copied());
                    cumulative.insert(row.skill_cumulative_condition_id);
                    json!({"row":format!("{row:?}"),"activationTimeBits":row.activation_time_second.to_bits()})
                })
                .collect();
            json!({"key":key,"position":position,"metadata":format!("{metadata:?}"),"rows":rows})
        })
        .collect();
    let sets: Vec<_> = master.skill_condition_sets.iter().filter(|set| groups.contains(&set.group)).collect();
    let condition_ids: BTreeSet<_> = sets.iter().flat_map(|set| set.condition_ids.iter().copied()).collect();
    let conditions: Vec<_> = condition_ids
        .into_iter()
        .map(|id| {
            let condition = master.skill_condition(id);
            if let Some(condition) = condition {
                targets.extend(condition.condition_target_ids.iter().copied());
            }
            json!({"id":id,"row":format!("{condition:?}")})
        })
        .collect();
    // Cumulative target fields can be read by Factory even when the DP ultimately refuses that row.
    let cumulatives: Vec<_> = cumulative
        .into_iter()
        .filter(|&id| id != 0)
        .map(|id| {
            let row = master.cumulative_condition(id);
            if let Some(row) = row {
                targets.extend(row.condition_target_ids.iter().copied());
            }
            json!({"id":id,"row":format!("{row:?}")})
        })
        .collect();
    let target_rows: Vec<_> =
        targets.into_iter().map(|id| json!({"id":id,"row":format!("{:?}",master.skill_target(id))})).collect();
    json!({"sources":sources,"groups":groups,"sets":format!("{sets:?}"),
        "conditions":conditions,"cumulatives":cumulatives,"targets":target_rows})
}

fn dependencies(master: &Master, input: &LuckInput, skills: &LuckSkills, neutral: Option<(i64, i64)>) -> Value {
    let (input, setup) = input;
    let mut probes: EntryKey = skills.shapes.iter().map(|shape| (shape.probe, 0)).collect();
    if let Some((id, level)) = neutral {
        probes.push((LuckSkillKey { source: LuckSource::Gekisou, id, level, matched: None }, 0));
    }
    let notes: Vec<_> =
        input.notes.iter().map(|n| [n.note_id, n.time_ms, n.note_operate_type, n.judgement_type]).collect();
    let frames: Vec<_> = input
        .play
        .frames
        .iter()
        .map(|frame| {
            json!([
                frame.time_ms,
                frame
                    .judged
                    .iter()
                    .map(|note| [note.note_id, note.judgement, note.judgement_time_ms])
                    .collect::<Vec<_>>()
            ])
        })
        .collect();
    let mut params = input.params;
    params.total_power = 0;
    let rows: Vec<_> = skills.rows.iter().collect();
    json!({
        "algorithm":{"name":ALGORITHM,"simSourceSha256":ournotes_sim::SOURCE_SHA256,
            "law":"independentNominal","value":"kernelProbability","nativeScoreCertificate":false},
        "resolved":{"notes":notes,"events":input.events,"frames":frames,"baseSeed":input.play.base_seed,
            "deltaBits":input.delta_times.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            "params":format!("{params:?}"),"assistBits":params.assist_factor.to_bits(),
            "fevers":setup.fevers,"missions":setup.missions,"rankConfirmations":input.rank_confirmations},
        "sharedMaster":{
            "liveSettings":format!("{:?}",master.live_settings),
            "parameters":format!("{:?}",master.parameters),
            "noteParameters":format!("{:?}",master.note_parameters),
            "judgementParameters":format!("{:?}",master.judgement_parameters),
            "comboScoreBonuses":master.combo_score_bonuses.iter().map(|r| json!({"row":format!("{r:?}"),"factorBits":r.bonus_factor.to_bits()})).collect::<Vec<_>>(),
            "effectSettings":format!("{:?}",master.skill_effect_settings),
            "judgementTimings":format!("{:?}",master.live_judgement_timings),
            "luckBasePoints":format!("{:?}",master.gekisou_luck_base_points),
            "luckBonusLots":format!("{:?}",master.gekisou_luck_bonus_lots),
            "rankingBonuses":format!("{:?}",master.gekisou_ranking_score_bonuses)},
        "catalogue":{"shapes":skills.shapes,"rowShapes":rows},"neutral":neutral,
        "probes":source_dependencies(master,&probes)
    })
}

fn error_status(error: &Error) -> &'static str {
    match error {
        Error::Unsupported(_) => "unsupported",
        Error::Capacity(_) => "capacity",
        _ => "error",
    }
}

fn actual_entries(master: &Master, skills: &LuckSkills, performers: &[Performer; 5]) -> Result<EntryKey, Error> {
    let mut entries = Vec::new();
    for (position, performer) in performers.iter().enumerate() {
        let Some((id, level)) = performer.gekisou_skill else { continue };
        for (source, (id, level)) in std::iter::once((LuckSource::Gekisou, (id, level)))
            .chain(performer.gekisou_support_skills.iter().copied().map(|skill| (LuckSource::GekisouSupport, skill)))
        {
            let key = luck_skill_key(master, source, id, level, performer)?;
            if skills.chain.contains(&key) {
                entries.push((key, position));
            }
        }
    }
    Ok(entries)
}

fn at(curve: &ResponseCurve, time: i32) -> [[f64; 2]; 4] {
    let index = curve.steps.partition_point(|step| step.time_ms <= time);
    index.checked_sub(1).map_or([[1.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]], |i| curve.steps[i].buckets)
}

fn compare(actual: &ResponseCurve, basis: &ResponseCurve) -> Value {
    let times: BTreeSet<_> = actual.steps.iter().chain(&basis.steps).map(|step| step.time_ms).collect();
    let mut overlap = true;
    let mut same = true;
    let mut maximum_gap = 0f64;
    for time in times {
        for (a, b) in at(actual, time).into_iter().zip(at(basis, time)) {
            same &= a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits();
            overlap &= a[0] <= b[1] && b[0] <= a[1];
            maximum_gap = maximum_gap.max((a[0] - b[1]).max(b[0] - a[1]).max(0.0));
        }
    }
    json!({"sameJointCurve":same,"intervalsOverlap":overlap,"maximumSeparatedGap":maximum_gap,
        "sameTransitionMasks":actual.probe_transitions==basis.probe_transitions,
        "sameProbes":actual.probes==basis.probes,"sameRangeMoments":actual.range_moments==basis.range_moments,
        "isProgramEquivalenceProof":false})
}

fn lookup_weights(curve: &ResponseCurve, shapes: usize) -> Result<Vec<(i32, Vec<f32>)>, Error> {
    if curve.probes.len() != shapes || curve.probes.iter().any(|enabled| !enabled) {
        return Err(Error::Input("lookup response does not cover every score shape".into()));
    }
    Ok(curve
        .steps
        .iter()
        .map(|step| {
            let midpoint = step.buckets.map(|[lo, hi]| lo + (hi - lo) * 0.5);
            let rush = (midpoint[2] + midpoint[3]).clamp(0.0, 1.0) as f32;
            let score = (midpoint[1] + midpoint[3]).clamp(0.0, 1.0) as f32;
            let both = midpoint[3].clamp(0.0, 1.0) as f32;
            let mut weights = Vec::with_capacity(1 + 2 * shapes);
            weights.push(rush);
            for _ in 0..shapes {
                weights.extend([score, both]);
            }
            (step.time_ms, weights)
        })
        .collect())
}

fn score_lookup(
    master: &Master,
    input: &super::super::expectation::FiniteSeedContext,
    skills: &LuckSkills,
    performers: &[Performer; 5],
    curve: &ResponseCurve,
) -> Result<i32, Error> {
    let mut model = input.model(master, performers)?;
    model.set_luck_weights(skills, lookup_weights(curve, skills.shapes.len())?)?;
    model.run_with_random(&input.play, &input.delta_times, LiveRandom::new(0))
}

#[allow(clippy::too_many_arguments)]
fn scoring(
    master: &Master,
    input: &super::super::expectation::FiniteSeedContext,
    setup: &ournotes_sim::live::full::GekisouSetup,
    skills: &LuckSkills,
    performers: &[Performer; 5],
    basis: Option<&ResponseCurve>,
    ordinal: usize,
    samples: u32,
) -> Value {
    let started = Instant::now();
    let result = (|| -> Result<Value, Error> {
        let width = 1 + 2 * skills.shapes.len();
        let mut ends = Vec::new();
        for (rush, rest) in [(0f32, 0f32), (1.0, 0.0), (1.0, 1.0)] {
            let mut weights = vec![rest; width];
            weights[0] = rush;
            let mut model = input.model(master, performers)?;
            model.set_luck_weights(skills, vec![(i32::MIN, weights)])?;
            ends.push(model.run_with_random(&input.play, &input.delta_times, LiveRandom::new(0))?);
        }
        let dp = luck_rush_dp_with_ranking(
            master,
            skills,
            &input.notes,
            &input.events,
            input.params,
            setup,
            &input.play,
            &input.delta_times,
            performers,
            None,
            input.rank_confirmations.as_deref(),
        )?;
        let mut model = input.model(master, performers)?;
        model.set_luck_weights(skills, dp.steps)?;
        let weighted = model.run_with_random(&input.play, &input.delta_times, LiveRandom::new(0))?;
        let (lookup, lookup_error) = match basis {
            None => (None, None),
            Some(curve) => match score_lookup(master, input, skills, performers, curve) {
                Ok(score) => (Some(score), None),
                Err(error) => (None, Some(error.to_string())),
            },
        };
        let samples = if samples == 0 {
            Value::Null
        } else {
            let began = Instant::now();
            let (mut sum, mut sum_sq) = (0f64, 0f64);
            for sample in 0..samples {
                let mut model = input.model(master, performers)?;
                let value = f64::from(model.run_with_random(
                    &input.play,
                    &input.delta_times,
                    super::luck_seed(0, samples, ordinal, sample),
                )?);
                sum += value;
                sum_sq += value * value;
            }
            let n = f64::from(samples);
            let mean = sum / n;
            let se = (samples > 1).then(|| ((sum_sq / n - mean * mean).max(0.0) / (n - 1.0)).sqrt());
            json!({"count":samples,"sum":sum,"sumSquares":sum_sq,"mean":mean,"se":se,
                "elapsedMs":began.elapsed().as_secs_f64()*1000.0,"statisticalOnly":true})
        };
        Ok(json!({"status":"success","s0":ends[0],"sBonus":ends[1],"s1":ends[2],
            "scoreAtMean":weighted,"scoreAtMeanIsExactExpectation":false,
            "scoreAtLookup":lookup,"lookupError":lookup_error,"lookupIsScorePrediction":true,"samples":samples}))
    })();
    let mut out = result.unwrap_or_else(|error| json!({"status":error_status(&error),"error":error.to_string()}));
    out["elapsedMs"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    out
}

/// Plan performs no recording, DP or simulation. Generation produces isolated real-skill response data;
/// optional validation retains every original order even if extraction, DP, lookup or scoring fails.
pub fn generate_luck_response(built: &BuiltProblem<'_>, spec: &LuckResponseSpec) -> Result<Value, Error> {
    let started = Instant::now();
    if spec.jobs.len() > 65_536 || spec.jobs.iter().any(|job| job.entries.len() > 15) {
        return Err(Error::Capacity("response job/key limit".into()));
    }
    let mut names = BTreeSet::new();
    if spec.jobs.iter().any(|job| job.name.is_empty() || !names.insert(&job.name)) {
        return Err(Error::Input("response job names must be nonempty and unique".into()));
    }
    let anchor = match &spec.context_deck {
        Some(deck) => deck.clone(),
        None => first_deck(built)?,
    };
    let master = built.pool.master;
    let mut input = luck_input(built, anchor.members, anchor.snaps)?;
    // This isolated controller is not a scored physical deck. Power has no consumer in its native DP.
    let anchor_power = input.0.params.total_power;
    input.0.params.total_power = 0;
    let skills = luck_skills(master)?;
    let neutral = luck_neutral(master, &skills);
    let compiled_mode = matches!(
        spec.mode,
        ResponseMode::Identify | ResponseMode::Programs | ResponseMode::BasisIdentify | ResponseMode::BasisPrograms
    );
    for job in spec.jobs.iter().filter(|_| !compiled_mode) {
        // The native holder is authoritative for formation compatibility and all positional limits.
        // This constructs no LiveModel and performs no recording, propagation or simulation.
        validate_entries(master, &skills, neutral, &job.entries)?;
    }
    let shared = dependencies(master, &input, &skills, neutral);
    let shared_fingerprint = fingerprint(&shared);
    let job_dependencies: Vec<_> = spec.jobs.iter().map(|job| source_dependencies(master, &job.entries)).collect();
    let mut archive_dependencies: BTreeMap<_, _> = spec
        .jobs
        .iter()
        .zip(&job_dependencies)
        .map(|(job, deps)| (serde_json::to_string(&job.entries).expect("entry JSON"), fingerprint(deps)))
        .collect();
    let mut context = response_context(&shared_fingerprint, archive_dependencies.clone());
    if compiled_mode {
        let capacity = spec.cache_bytes.unwrap_or_else(|| spec.cache_entries.saturating_mul(8192));
        let basis_mode = matches!(spec.mode, ResponseMode::BasisIdentify | ResponseMode::BasisPrograms);
        let mut report = if basis_mode {
            super::luck_response_basis::identify_basis(
                master,
                &input,
                &skills,
                neutral,
                &spec.jobs,
                super::luck_response_basis::BasisOptions {
                    identity_bytes: spec.identity_bytes.unwrap_or(capacity),
                    program_bytes: spec.program_bytes.unwrap_or(capacity),
                    response_bytes: spec.basis_response_bytes.unwrap_or(capacity),
                    max_terms: spec.basis_max_terms.unwrap_or(64),
                    propagate: matches!(spec.mode, ResponseMode::BasisPrograms),
                    selected: spec.basis_programs.as_deref(),
                    verify: spec.verify_basis,
                    reconstruct: spec.basis_reconstruct,
                },
            )
        } else {
            super::luck_response_interactions::identify_interactions(
                master,
                &input,
                &skills,
                neutral,
                &spec.jobs,
                spec.identity_bytes.unwrap_or(capacity),
                spec.program_bytes.unwrap_or(capacity),
                matches!(spec.mode, ResponseMode::Programs),
                spec.canonical_start_minimum,
            )
        };
        report["format"] = json!(if basis_mode {
            "ournotes-deck.luck-response-basis/1"
        } else {
            "ournotes-deck.luck-response-programs/1"
        });
        report["mode"] = json!(spec.mode);
        report["sourceVersion"] = json!(ournotes_sim::SOURCE_SHA256);
        report["context"] = json!(context);
        report["sharedFingerprint"] = json!(shared_fingerprint);
        report["dependencyDescriptor"] = shared;
        report["capabilities"] = json!({"chain":skills.chain});
        report["provenance"] = json!({"contextDeck":anchor,"contextDeckPower":anchor_power});
        report["elapsedMs"] = json!(started.elapsed().as_secs_f64() * 1000.0);
        return Ok(report);
    }
    let mut table = ResponseTable { context: context.clone(), entries: Vec::new() };
    // Existing request-sized allowance, with an explicit zero-cache experiment. No process-global cache.
    let capacity = spec.cache_bytes.unwrap_or_else(|| spec.cache_entries.saturating_mul(8192)).min(32 << 20);
    let mut cache = LuckDpCache::new(capacity);
    let mut jobs = Vec::new();
    for (job, deps) in spec.jobs.iter().zip(job_dependencies) {
        let begin = Instant::now();
        let before = cache.stats();
        let mut report = json!({"name":job.name,"entries":job.entries,"dependencyFingerprint":fingerprint(&deps),
            "dependencyDescriptor":deps,"status":"planned"});
        if matches!(spec.mode, ResponseMode::Generate) {
            if let Some(index) = table.entries.iter().position(|entry| entry.key == job.entries) {
                report["entryIndex"] = json!(index);
                report["reusedExactEntry"] = json!(true);
                report["status"] = json!(match &table.entries[index].response {
                    Response::Success { .. } => "success",
                    Response::Unsupported { .. } => "unsupported",
                });
            } else {
                match luck_table_dp_certified_cached(
                    master,
                    &skills,
                    neutral,
                    &input.0.notes,
                    input.0.params,
                    &input.1,
                    &input.0.play,
                    &input.0.delta_times,
                    &job.entries,
                    &mut cache,
                ) {
                    Ok(result) => {
                        report["status"] = json!("success");
                        report["entryIndex"] = json!(table.entries.len());
                        report["peakStates"] = json!(result.peak_states);
                        report["transitions"] = json!(result.transitions);
                        table.entries.push(ResponseEntry {
                            key: job.entries.clone(),
                            response: Response::Success { curve: ResponseCurve::from_certified(&result) },
                        });
                    }
                    Err(error) => {
                        report["status"] = json!(error_status(&error));
                        report["error"] = json!(error.to_string());
                        if matches!(&error, Error::Unsupported(_)) {
                            report["entryIndex"] = json!(table.entries.len());
                            table.entries.push(ResponseEntry {
                                key: job.entries.clone(),
                                response: Response::Unsupported { reason: error.to_string() },
                            });
                        }
                    }
                }
            }
            let runs = job.mc_runs.unwrap_or(spec.mc_runs);
            if runs > 0 {
                let mc_start = Instant::now();
                let seeds: Vec<_> = (0..u64::from(runs)).map(ournotes_sim::live::seeds::seed_candidate).collect();
                report["mc"] = match luck_table_steps(
                    master,
                    &skills,
                    neutral,
                    &input.0.notes,
                    input.0.params,
                    &input.1,
                    &input.0.play,
                    &input.0.delta_times,
                    &job.entries,
                    &seeds,
                ) {
                    Ok(steps) => json!({"status":"sampled","runs":runs,"steps":steps,"statisticalOnly":true}),
                    Err(error) => json!({"status":error_status(&error),"error":error.to_string(),"runs":runs}),
                };
                report["mc"]["elapsedMs"] = json!(mc_start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        report["elapsedMs"] = json!(begin.elapsed().as_secs_f64() * 1000.0);
        report["cacheBefore"] = json!(before);
        report["cacheAfter"] = json!(cache.stats());
        jobs.push(report);
    }
    if matches!(spec.mode, ResponseMode::Generate) {
        // Only encoded entries bind the archive. Capacity/error jobs remain in the report and are retryable.
        let stored: BTreeSet<_> =
            table.entries.iter().map(|entry| serde_json::to_string(&entry.key).expect("entry JSON")).collect();
        archive_dependencies.retain(|key, _| stored.contains(key));
        context = response_context(&shared_fingerprint, archive_dependencies);
        table.context = context.clone();
    }
    let mut validations = Vec::new();
    if matches!(spec.mode, ResponseMode::Generate) {
        for deck in &spec.validation_decks {
            let began = Instant::now();
            let validation = luck_input(built, deck.members, deck.snaps);
            let mut orders = Vec::with_capacity(120);
            for (ordinal, order) in super::super::uniform::all_orders().into_iter().enumerate() {
                let begin = Instant::now();
                let mut row = json!({"order":order,"ordinal":ordinal,"basisEntryIndex":null,"comparison":null});
                match &validation {
                    Err(error) => {
                        row["actual"] = json!({"status":error_status(error),"error":error.to_string()});
                    }
                    Ok((input, setup)) => {
                        let performers = order.map(|slot| input.performers[slot].clone());
                        let entries = actual_entries(master, &skills, &performers);
                        let basis = match entries {
                            Ok(entries) => {
                                let found = table.entries.iter().position(|entry| entry.key == entries);
                                row["entries"] = json!(entries);
                                row["basisEntryIndex"] = json!(found);
                                found
                            }
                            Err(error) => {
                                row["entryError"] = json!(error.to_string());
                                None
                            }
                        };
                        match luck_rush_dp_certified_with_ranking(
                            master,
                            &skills,
                            &input.notes,
                            &input.events,
                            input.params,
                            setup,
                            &input.play,
                            &input.delta_times,
                            &performers,
                            None,
                            input.rank_confirmations.as_deref(),
                        ) {
                            Ok(result) => {
                                let curve = ResponseCurve::from_certified(&result);
                                if let Some(ResponseEntry { response: Response::Success { curve: basis }, .. }) =
                                    basis.map(|i| &table.entries[i])
                                {
                                    row["comparison"] = compare(&curve, basis);
                                }
                                row["actual"] = json!(Response::Success { curve });
                            }
                            Err(error) => {
                                row["actual"] = json!({"status":error_status(&error),"error":error.to_string()})
                            }
                        }
                        let lookup = basis.and_then(|index| match &table.entries[index].response {
                            Response::Success { curve } => Some(curve),
                            Response::Unsupported { .. } => None,
                        });
                        row["scoring"] =
                            scoring(master, input, setup, &skills, &performers, lookup, ordinal, spec.score_samples);
                    }
                }
                row["elapsedMs"] = json!(begin.elapsed().as_secs_f64() * 1000.0);
                orders.push(row);
            }
            validations.push(json!({"name":deck.name,"members":deck.members,"snaps":deck.snaps,
                "orders":orders,"elapsedMs":began.elapsed().as_secs_f64()*1000.0}));
        }
    }
    Ok(json!({"format":"ournotes-deck.luck-response-generation/1","kind":"isolatedRealSkillKernelProbability",
        "scope":"Independent nominal lottery response on the resolved real chart. Not full native score expectation, finite-seed mean, program equivalence, admission or ranking certificate.",
        "mode":spec.mode,"context":context,"sharedFingerprint":shared_fingerprint,"dependencyDescriptor":shared,
        "capabilities":{"chain":skills.chain},"provenance":{"contextDeck":anchor,"contextDeckPower":anchor_power},"jobs":jobs,"table":table,
        "validationDecks":validations,"cacheEntriesCompatibilityEstimateBytes":8192,"cacheBytes":capacity,"cacheStats":cache.stats(),"archives":[],
        "elapsedMs":started.elapsed().as_secs_f64()*1000.0}))
}

/// Read an archive as an explicitly approximate score predictor. Context identity is checked before any
/// payload is decoded. Each lookup owns at most one response; no decoded table or native DP cache is built.
/// A source-key match is not a proof that the original deck's LIFE/conversion inputs equal isolated holders.
pub fn predict_luck_response(
    built: &BuiltProblem<'_>,
    archive: &ResponseArchive<'_>,
    decks: &[ResponseDeck],
) -> Result<Value, Error> {
    let started = Instant::now();
    let master = built.pool.master;
    let expected = (|| -> Result<_, Error> {
        let anchor = first_deck(built)?;
        let input = luck_input(built, anchor.members, anchor.snaps)?;
        let skills = luck_skills(master)?;
        let neutral = luck_neutral(master, &skills);
        let shared = fingerprint(&dependencies(master, &input, &skills, neutral));
        let mut entries = BTreeMap::new();
        for key in archive.keys() {
            if (0..5).all(|position| key.iter().any(|(_, held)| *held == position)) {
                if key.iter().any(|(source, _)| !skills.chain.contains(source)) {
                    return Err(Error::Input("response entry is outside the LUCK chain catalogue".into()));
                }
                luck_table_validate_virtual(master, &skills, neutral, key)?;
            } else {
                validate_entries(master, &skills, neutral, key)?;
            }
            entries.insert(
                serde_json::to_string(key).expect("entry JSON"),
                fingerprint(&source_dependencies(master, key)),
            );
        }
        let context = response_context(&shared, entries);
        Ok((skills, shared, context))
    })();
    let (skills, shared, expected) = match expected {
        Ok(value) => value,
        Err(error) => {
            return Ok(json!({"format":"ournotes-deck.luck-response-prediction/1",
            "status":"contextMismatch","reason":error.to_string(),"archiveContext":archive.context(),
            "isScorePrediction":true,"nativeExpectationProven":false,"rankingProven":false,
            "ordersScored":0,"elapsedMs":started.elapsed().as_secs_f64()*1000.0}));
        }
    };
    if archive.context() != &expected {
        return Ok(json!({"format":"ournotes-deck.luck-response-prediction/1","status":"contextMismatch",
            "reason":"resolved native context or selected source dependencies differ",
            "archiveContext":archive.context(),"expectedContext":expected,"sharedFingerprint":shared,
            "isScorePrediction":true,"nativeExpectationProven":false,"rankingProven":false,
            "ordersScored":0,"elapsedMs":started.elapsed().as_secs_f64()*1000.0}));
    }
    let context_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut used = vec![false; archive.keys().len()];
    let (mut decode_calls, mut decoded_peak, mut orders_scored) = (0usize, 0usize, 0usize);
    let (mut decode_ms, mut score_ms) = (0f64, 0f64);
    let mut results = Vec::with_capacity(decks.len());
    let mut complete = true;
    for deck in decks {
        let began = Instant::now();
        let input = luck_input(built, deck.members, deck.snaps);
        let mut orders = Vec::with_capacity(120);
        let (mut sum, mut success) = (0i64, 0usize);
        for (ordinal, order) in super::super::uniform::all_orders().into_iter().enumerate() {
            let mut row = json!({"ordinal":ordinal,"order":order,"scoreAtLookup":null});
            let prepared = (|| -> Result<_, Error> {
                let (input, _) = input.as_ref().map_err(Clone::clone)?;
                let performers = order.map(|slot| input.performers[slot].clone());
                let key = actual_entries(master, &skills, &performers)?;
                Ok((input, performers, key))
            })();
            match prepared {
                Err(error) => {
                    row["status"] = json!(error_status(&error));
                    row["error"] = json!(error.to_string());
                }
                Ok((input, performers, key)) => {
                    row["entries"] = json!(key);
                    if let Some(index) = archive.key_index(&key) {
                        decode_calls += 1;
                        let began = Instant::now();
                        let response = archive.lookup(&key);
                        let elapsed = began.elapsed().as_secs_f64() * 1000.0;
                        decode_ms += elapsed;
                        row["decodeMs"] = json!(elapsed);
                        match response {
                            Err(error) => {
                                row["status"] = json!("decodeError");
                                row["error"] = json!(error.to_string());
                            }
                            Ok(None) => {
                                row["status"] = json!("missing");
                            }
                            Ok(Some(response)) => {
                                used[index] = true;
                                let bytes = response
                                    .allocated_bytes()
                                    .ok_or_else(|| Error::Capacity("decoded response byte count overflow".into()))?;
                                decoded_peak = decoded_peak.max(bytes);
                                row["decodedBytes"] = json!(bytes);
                                match response {
                                    Response::Unsupported { reason } => {
                                        row["status"] = json!("unsupported");
                                        row["error"] = json!(reason);
                                    }
                                    Response::Success { curve } => {
                                        let began = Instant::now();
                                        let score = score_lookup(master, input, &skills, &performers, &curve);
                                        let elapsed = began.elapsed().as_secs_f64() * 1000.0;
                                        score_ms += elapsed;
                                        row["scoreMs"] = json!(elapsed);
                                        match score {
                                            Ok(score) => {
                                                row["status"] = json!("success");
                                                row["scoreAtLookup"] = json!(score);
                                                sum += i64::from(score);
                                                success += 1;
                                                orders_scored += 1;
                                            }
                                            Err(error) => {
                                                row["status"] = json!(error_status(&error));
                                                row["error"] = json!(error.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        row["status"] = json!("missing");
                    }
                }
            }
            orders.push(row);
        }
        let all = success == 120;
        complete &= all;
        results.push(json!({"name":deck.name,"members":deck.members,"snaps":deck.snaps,
            "status":if all {"success"} else {"partial"},"orders":orders,"ordersScored":success,
            "uniform120ProxySum":all.then_some(sum),"uniform120ProxyMean":all.then_some(sum as f64 / 120.0),
            "elapsedMs":began.elapsed().as_secs_f64()*1000.0}));
    }
    Ok(json!({"format":"ournotes-deck.luck-response-prediction/1","status":if complete {"success"} else {"partial"},
        "isScorePrediction":true,"nativeExpectationProven":false,"rankingProven":false,
        "scope":"Archive identity validates isolated real-skill response data. Native weighted score replay is a predictor; full-deck probability equivalence and expected score are not certified.",
        "context":expected,"sharedFingerprint":shared,"quantization":archive.quantization(),"decks":results,
        "ordersScored":orders_scored,"contextMs":context_ms,"decodeMs":decode_ms,"scoreMs":score_ms,
        "decodeCalls":decode_calls,"uniqueKeysDecoded":used.iter().filter(|&&value|value).count(),
        "decodedCacheEntries":0,"decodedCacheBytes":0,"decodedPeakBytes":decoded_peak,
        "archiveIndexBytes":archive.index_bytes(),"uniqueKeyTrackerBytes":used.capacity()*std::mem::size_of::<bool>(),
        "memoryScope":"Decoded response ownership only; encoded bytes, archive index, native model and weight replay storage are separate.",
        "elapsedMs":started.elapsed().as_secs_f64()*1000.0}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ournotes_sim::chartstats::luck_response::ResponseStep;

    #[test]
    fn response_comparison_preserves_step_semantics_and_separate_metadata() {
        let mut a = ResponseCurve {
            steps: vec![ResponseStep { time_ms: 10, buckets: [[0.4, 0.6], [0.0, 0.0], [0.4, 0.6], [0.0, 0.0]] }],
            probe_transitions: vec![1],
            probes: vec![true],
            range_moments: vec![],
            peak_states: 1,
            transitions: 2,
        };
        let mut b = a.clone();
        b.steps.push(ResponseStep { time_ms: 20, buckets: a.steps[0].buckets });
        b.probes[0] = false;
        let result = compare(&a, &b);
        assert_eq!(result["sameJointCurve"], true);
        assert_eq!(result["sameProbes"], false);
        a.steps[0].buckets[0] = [0.7, 0.8];
        let result = compare(&a, &b);
        assert_eq!(result["intervalsOverlap"], false);
        assert_eq!(result["isProgramEquivalenceProof"], false);
    }

    #[test]
    fn response_spec_defaults_do_not_request_monte_carlo() {
        let spec: LuckResponseSpec = serde_json::from_value(json!({"jobs":[{"name":"base","entries":[]}]})).unwrap();
        assert_eq!(spec.mc_runs, 0);
        assert_eq!(spec.score_samples, 0);
        assert!(spec.validation_decks.is_empty());
    }

    #[test]
    fn response_context_binds_every_stored_source_and_shared_input() {
        let entries =
            BTreeMap::from([("first-key".into(), "source-a".into()), ("second-key".into(), "source-b".into())]);
        let expected = response_context("chart-and-master", entries.clone());
        let reversed = entries.iter().rev().map(|(key, value)| (key.clone(), value.clone())).collect();
        assert_eq!(expected, response_context("chart-and-master", reversed));
        assert_ne!(expected, response_context("changed-chart", entries.clone()));
        let mut changed = entries.clone();
        changed.insert("second-key".into(), "changed-source".into());
        assert_ne!(expected, response_context("chart-and-master", changed));
        let mut missing = entries;
        missing.remove("second-key");
        assert_ne!(expected, response_context("chart-and-master", missing));
    }

    #[test]
    fn response_lookup_weights_keep_joint_terms_and_refuse_unobserved_shapes() {
        let mut curve = ResponseCurve {
            steps: vec![ResponseStep {
                time_ms: 17,
                buckets: [[0.125, 0.125], [0.25, 0.25], [0.375, 0.375], [0.25, 0.25]],
            }],
            probe_transitions: vec![],
            probes: vec![true, true],
            range_moments: vec![],
            peak_states: 1,
            transitions: 1,
        };
        assert_eq!(lookup_weights(&curve, 2).unwrap(), vec![(17, vec![0.625, 0.5, 0.25, 0.5, 0.25])]);
        assert!(lookup_weights(&curve, 1).is_err());
        curve.probes[1] = false;
        assert!(lookup_weights(&curve, 2).is_err());
    }
}
