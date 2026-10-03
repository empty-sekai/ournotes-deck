//! Independent fixed-deck comparisons of full-engine Rush probes and optional LUCK replay.
use crate::clock::Instant;
use crate::handler::BuiltProblem;
use crate::search::expectation::{PhysicalDeck, context, native_member_order};
use ournotes_sim::Error;
use ournotes_sim::live::full::{LiveModel, LuckSignature, RushMasks, luck_signature, rush_frames};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

type AuditDeck = ([i64; 5], [Option<i64>; 5]);
type ReplayKey = (i32, Vec<LuckSignature>);

fn active_counts(masks: &[Vec<bool>; 4]) -> [usize; 4] {
    std::array::from_fn(|gate| masks[gate].iter().filter(|&&active| active).count())
}

/// Audit every distinct declared root of every supplied physical deck. This never
/// changes the search domain or enables a production bound. With no compiled
/// eligibility proof, zero observed conversions admits only this recorded root,
/// not another root or deck. Same ordered signature/root replays are compared
/// exactly across decks within this one frozen problem.
pub fn audit_rush_replay(built: &BuiltProblem<'_>, decks: &[AuditDeck], max_runs: usize) -> Result<Value, Error> {
    if max_runs == 0 {
        return Err(Error::Input("Rush replay audit requires a positive run limit".into()));
    }
    let spec = &built.context.spec;
    if spec.network_confirmations.is_some() || spec.simulation.live_finished_from_frame.is_some() {
        return Err(Error::Unsupported("Rush replay audit requires the declared solo lifecycle".into()));
    }
    let mut roots = BTreeMap::<i32, u128>::new();
    for &(root, mass) in built.context.law.atoms() {
        *roots.entry(root).or_default() += u128::from(mass);
    }
    let compiled_eligible = built.context.plan.joint.as_ref().is_some_and(|b| b.rush_eligible());
    let life = built.context.plan.joint.as_ref().and_then(|b| b.luck_life());
    let mut groups = Vec::new();
    let mut cache = HashMap::<ReplayKey, (usize, Option<RushMasks>)>::new();
    let mut deck_reports = Vec::new();
    let mut compared = 0usize;
    let mut unsupported = 0usize;
    let mut violations = 0usize;
    let mut signature_comparisons = 0usize;
    for &(members, snaps) in decks {
        let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
        let physical = PhysicalDeck { members: d.members, snaps: d.snaps };
        built.domain().check_fixed(&built.pool, &physical)?;
        let mut input = context(&built.pool, &physical, &built.context.request.objective)?;
        if let Some(v) = spec.simulation.music_length_ms {
            input.params.music_length_ms = v;
        }
        if let Some(v) = spec.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(v);
        }
        let setup = input.gekisou.as_ref().ok_or_else(|| Error::Unsupported("Rush audit requires Gekisou".into()))?;
        let mut root_reports = Vec::new();
        for (&root, &mass) in &roots {
            let (order, random) = native_member_order(root)?;
            let performers = order.map(|slot| input.performers[slot].clone());
            let signature = performers
                .iter()
                .map(|p| luck_signature(built.pool.master, p))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .collect::<Option<Vec<_>>>();
            let start = Instant::now();
            let mut actual_model = LiveModel::new_gekisou(
                built.pool.master,
                &performers,
                &input.notes,
                &input.events,
                input.params,
                setup,
            )?;
            let (score, actual) = actual_model.run_with_rush_masks(&input.play, &input.delta_times, random)?;
            let actual_ms = start.elapsed().as_secs_f64() * 1000.0;
            let start = Instant::now();
            let plain = input.simulate(built.pool.master, root)?;
            let plain_ms = start.elapsed().as_secs_f64() * 1000.0;
            let transparent = score == plain.final_score
                && actual_model.current_life() == plain.model.current_life()
                && actual_model.current_combo() == plain.model.current_combo()
                && actual_model.converted_judgements() == plain.model.converted_judgements()
                && actual_model.draws() == plain.model.draws()
                && actual_model.trace() == plain.model.trace()
                && actual_model.gekisou_rank_bonuses() == plain.model.gekisou_rank_bonuses();
            if !transparent {
                violations += 1;
            }
            let start = Instant::now();
            let replay = rush_frames(
                built.pool.master,
                &input.notes,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                &performers,
                root,
                max_runs,
                life,
            )?;
            let replay_ms = start.elapsed().as_secs_f64() * 1000.0;
            let (group, same_signature) = match signature {
                None => (None, None),
                Some(signature) => {
                    let key = (root, signature);
                    if let Some((id, previous)) = cache.get(&key) {
                        signature_comparisons += 1;
                        let equal = previous == &replay;
                        if !equal {
                            violations += 1;
                        }
                        (Some(*id), Some(equal))
                    } else {
                        let id = groups.len();
                        groups.push(json!({"id":id,"root":root,"orderedSignature":format!("{:?}", key.1)}));
                        cache.insert(key, (id, replay.clone()));
                        (Some(id), None)
                    }
                }
            };
            let conversions = actual_model.converted_judgements();
            let admission = if compiled_eligible {
                Some("compiledDomain")
            } else if conversions == 0 {
                Some("observedUnconvertedRootOnly")
            } else {
                None
            };
            let mut missing = Vec::new();
            let mut compared_cells = 0usize;
            let mut shape_matches = None;
            let status = if let (Some(_), Some(mask)) = (admission, replay.as_ref()) {
                compared += 1;
                shape_matches = Some(true);
                for gate in 0..4 {
                    if actual[gate].len() != input.play.frames.len() || mask.flags[gate].len() != actual[gate].len() {
                        shape_matches = Some(false);
                        violations += 1;
                        continue;
                    }
                    compared_cells += actual[gate].len();
                    for (frame, (&observed, &allowed)) in actual[gate].iter().zip(&mask.flags[gate]).enumerate() {
                        if observed && !allowed {
                            missing
                                .push(json!({"gate":gate+1,"frame":frame,"timeMs":input.play.frames[frame].time_ms}));
                            violations += 1;
                        }
                    }
                }
                "compared"
            } else {
                unsupported += 1;
                "unsupported"
            };
            let reason = if replay.is_none() {
                Some("Replay unavailable: unsupported effects/conditions or run capacity; legacy bound required")
            } else if admission.is_none() {
                Some("No compiled raw-judgement equivalence proof and this root executed conversions")
            } else {
                None
            };
            root_reports.push(json!({
                "root":root,"mass":mass.to_string(),"nativeOrder":order,"status":status,
                "admission":admission,"unsupportedReason":reason,"compiledDomainEligible":compiled_eligible,
                "actualScore":score,"plainScore":plain.final_score,"probeTransparent":transparent,
                "actualConversions":conversions,"actualDraws":actual_model.draws(),
                "actualWallMs":actual_ms,"plainWallMs":plain_ms,"replayWallMs":replay_ms,
                "actualActiveFrames":active_counts(&actual),"replayActiveFrames":replay.as_ref().map(|m| active_counts(&m.flags)),
                "replayMaxRuns":replay.as_ref().map(|m| m.max_runs),
                "frameCounts":actual.each_ref().map(|row| row.len()),"shapeMatches":shape_matches,
                "replayFrameCounts":replay.as_ref().map(|mask| mask.flags.each_ref().map(|row| row.len())),
                "comparedMaskCells":compared_cells,"missingFrames":missing,
                "signatureGroup":group,"sameSignatureReplayEqual":same_signature,
            }));
        }
        deck_reports.push(json!({"members":members,"snaps":snaps,"roots":root_reports}));
    }
    Ok(json!({
        "format":"ournotes-deck.rush-replay-audit/1",
        "scope":"Full-engine probe transparency and fixed-deck/root mask inclusion. Observed-unconverted admission proves only each recorded root; it never enables pruning or certifies another root. Unsupported inputs remain explicit.",
        "maxRuns":max_runs,"declaredRootAtoms":built.context.law.atoms(),
        "distinctRoots":roots.len(),"decks":deck_reports,"signatureGroups":groups,
        "comparedRoots":compared,"unsupportedRoots":unsupported,"signatureComparisons":signature_comparisons,
        "violations":violations,"allComparedRootsPassed":compared > 0 && violations == 0,
        "allRootsCovered":!decks.is_empty() && unsupported == 0 && compared == decks.len() * roots.len(),
    }))
}

/// Every prefix cap along one fixed deck's search path, read from the bounds of the Gekisou conversion regime part
/// that holds the deck (as the physical search partitions the domain), next to the deck's own evaluation. The LUCK
/// replay oracle is the search's, built for the whole domain. Diagnostics only; nothing here prunes.
pub fn prefix_profile(built: &BuiltProblem<'_>, members: [i64; 5], snaps: [Option<i64>; 5]) -> Result<Value, Error> {
    use crate::search::joint::{JointBounds, SLOTS, SlotRules, order_law};
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let plan = &built.context.plan;
    let Some(whole) = plan.joint.as_ref() else { return Ok(Value::Null) };
    let orders = order_law(&built.context.law)?;
    let mut oracle = crate::search::luck::LuckOracle::new(
        &built.pool,
        &built.context.request,
        &plan.domain,
        &built.context.law,
        whole.luck_life(),
    )?;
    let converting = crate::search::snaps::conversion_snaps(&built.pool, plan.domain.snaps())?;
    let held: Vec<usize> = (0..5).filter(|&i| p.snaps[SLOTS[i]].is_some_and(|s| converting.contains(&s))).collect();
    let mask = |domain: &crate::domain::CandidateDomain, keep: &dyn Fn(usize) -> bool| -> Vec<bool> {
        std::iter::once(false).chain(domain.snaps().iter().map(|&s| keep(s))).collect()
    };
    let (regime, domain, rules) = if converting.is_empty() {
        ("whole", plan.domain.clone(), None)
    } else if held.is_empty() {
        ("conversionFree", plan.domain.retain_snaps(|s| !converting.contains(&s)), None)
    } else if held.len() == 1 {
        let c = p.snaps[SLOTS[held[0]]].expect("held converting Snap");
        let domain = plan.domain.retain_snaps(|s| !converting.contains(&s) || s == c);
        let only = mask(&domain, &|s| s == c);
        let mut r = SlotRules::default();
        for &other in &SLOTS {
            if other == SLOTS[held[0]] {
                r.forced[other] = Some(only.clone());
            } else {
                r.excluded[other] = Some(only.clone());
            }
        }
        ("oneConverting", domain, Some(r))
    } else {
        let conv = mask(&plan.domain, &|s| converting.contains(&s));
        let (i, j) = (held[0], held[1]);
        let mut r = SlotRules::default();
        r.forced[SLOTS[i]] = Some(conv.clone());
        r.forced[SLOTS[j]] = Some(conv.clone());
        for k in (0..j).filter(|&k| k != i) {
            r.excluded[SLOTS[k]] = Some(conv.clone());
        }
        ("twoConverting", plan.domain.clone(), Some(r))
    };
    let compiled;
    let bounds: &JointBounds = if regime == "whole" {
        whole
    } else {
        let spec = &built.context.spec;
        let mut b = JointBounds::compile(
            &built.pool,
            &built.context.request,
            &domain,
            &spec.metric,
            built.context.context_input.event_payoff.as_ref(),
            &spec.simulation,
        )?;
        b.set_rules(&built.pool, &domain, rules);
        compiled = b;
        &compiled
    };
    let profile = bounds.prefix_profile(&built.pool, &domain, &p, &orders, oracle.as_mut())?;
    let outcome = crate::auxiliary::evaluate_built(built, members, snaps)?;
    Ok(json!({"regime": regime, "evaluation": outcome.results, "profile": profile}))
}
