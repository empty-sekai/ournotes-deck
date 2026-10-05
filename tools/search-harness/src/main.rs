//! Bounded search-equivalence experiments sharing only the fixed-deck evaluator.
use ournotes_search::{
    auxiliary, handler,
    search::Completion,
    types::{self, RecommendationRequest, RecommendedDeck},
};
use ournotes_sim::{cards::Roster, data::DeckData, pool::Pool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    data: PathBuf,
    roster: PathBuf,
    request: PathBuf,
    oracle_max_candidates: usize,
    #[serde(default)]
    dominance: Vec<Substitution>,
    experiments: Vec<Experiment>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Substitution {
    kind: String,
    from: i64,
    to: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Experiment {
    name: String,
    patch: Value,
    repeats: usize,
    #[serde(default)]
    schedule: ournotes_search::search::diagnostics::Schedule,
}
type Key = ([i64; 5], [Option<i64>; 5]);

// Explicit team identity (canonical layout) and public None-first tie convention.
fn compare(a: &RecommendedDeck, b: &RecommendedDeck) -> Ordering {
    assert_eq!(
        a.expected_payoff.as_ref().expect("exact deterministic fixture").denominator,
        b.expected_payoff.as_ref().expect("exact deterministic fixture").denominator
    );
    let na = a
        .expected_payoff
        .as_ref()
        .expect("exact deterministic fixture")
        .numerator
        .parse::<i128>()
        .expect("i128 core payoff");
    let nb = b
        .expected_payoff
        .as_ref()
        .expect("exact deterministic fixture")
        .numerator
        .parse::<i128>()
        .expect("i128 core payoff");
    nb.cmp(&na)
        .then_with(|| b.power.cmp(&a.power))
        .then_with(|| a.members.cmp(&b.members))
        .then_with(|| a.snaps.cmp(&b.snaps))
}

// No production resolver, enumeration, bounds, matching, cache or Top-K code. A team is enumerated once, in its
// canonical layout: the leader in slot 2, the other members in ascending card ID order.
fn domain(pool: &Pool<'_>, req: &RecommendationRequest, cap: usize) -> Result<Vec<Key>> {
    if cap == 0 {
        return Err("oracleMaxCandidates must be positive".into());
    }
    let c = &req.constraints;
    let mids: Vec<_> =
        pool.members.iter().filter(|m| !c.exclude_members.contains(&m.id)).map(|m| (m.id, m.character_id)).collect();
    let sids: Vec<_> = pool.snaps.iter().filter(|s| !c.exclude_snaps.contains(&s.id)).map(|s| s.id).collect();
    let known: BTreeSet<_> = pool.members.iter().map(|m| m.id).collect();
    if c.include_members.iter().chain(c.leader.iter()).any(|id| !known.contains(id)) {
        return Err("unknown required/leader member in oracle".into());
    }
    fn snaps(
        out: &mut Vec<Key>,
        members: [i64; 5],
        ids: &[i64],
        row: &mut [Option<i64>; 5],
        pos: usize,
        no_snaps: bool,
        cap: usize,
    ) -> Result<()> {
        if pos == 5 {
            if out.len() == cap {
                return Err("oracle domain exceeds explicit cap; no proof issued".into());
            }
            out.push((members, *row));
            return Ok(());
        }
        row[pos] = None;
        snaps(out, members, ids, row, pos + 1, no_snaps, cap)?;
        if !no_snaps {
            for &id in ids {
                if !row[..pos].contains(&Some(id)) {
                    row[pos] = Some(id);
                    snaps(out, members, ids, row, pos + 1, no_snaps, cap)?;
                }
            }
        }
        row[pos] = None;
        Ok(())
    }
    fn members(
        out: &mut Vec<Key>,
        mids: &[(i64, i64)],
        sids: &[i64],
        req: &RecommendationRequest,
        row: &mut [i64; 5],
        chars: &mut Vec<i64>,
        cap: usize,
    ) -> Result<()> {
        let pos = chars.len();
        // Non-leader slots 0, 1, 3, 4 hold ascending member IDs.
        let previous = match pos {
            1 => Some(row[0]),
            3 => Some(row[1]),
            4 => Some(row[3]),
            _ => None,
        };
        if pos == 5 {
            if req.constraints.include_members.iter().all(|id| row.contains(id)) {
                snaps(out, *row, sids, &mut [None; 5], 0, req.constraints.no_snaps, cap)?;
            }
            return Ok(());
        }
        for &(id, ch) in mids {
            if chars.contains(&ch)
                || (pos == 2 && req.constraints.leader.is_some_and(|l| l != id))
                || previous.is_some_and(|p| id <= p)
            {
                continue;
            }
            row[pos] = id;
            chars.push(ch);
            members(out, mids, sids, req, row, chars, cap)?;
            chars.pop();
        }
        Ok(())
    }
    let mut out = Vec::new();
    members(&mut out, &mids, &sids, req, &mut [0; 5], &mut Vec::new(), cap)?;
    Ok(out)
}

fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in b {
                merge(a.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (a, b) => *a = b.clone(),
    }
}

/// The canonical layout of a team key: non-leader (member, Snap) pairs in ascending member ID order.
fn canonical((members, snaps): Key) -> Key {
    let mut pairs: Vec<_> = [0, 1, 3, 4].iter().map(|&s| (members[s], snaps[s])).collect();
    pairs.sort_unstable();
    let (mut m, mut s) = (members, snaps);
    for (&slot, &(id, snap)) in [0, 1, 3, 4].iter().zip(&pairs) {
        (m[slot], s[slot]) = (id, snap);
    }
    (m, s)
}

fn dominance(rows: &[RecommendedDeck], pool: &Pool<'_>, rule: &Substitution) -> Result<Value> {
    if rule.from == rule.to {
        return Err("dominance endpoints must differ".into());
    }
    match rule.kind.as_str() {
        "member" => {
            let a = pool.members.iter().find(|m| m.id == rule.from).ok_or("unknown dominance member")?;
            let b = pool.members.iter().find(|m| m.id == rule.to).ok_or("unknown dominance member")?;
            if a.character_id != b.character_id {
                return Err("member substitution must preserve character".into());
            }
        }
        "snap" => {
            if [rule.from, rule.to].iter().any(|id| !pool.snaps.iter().any(|s| s.id == *id)) {
                return Err("unknown dominance Snap".into());
            }
        }
        _ => return Err("dominance kind must be member or snap".into()),
    }
    let index: BTreeMap<Key, _> = rows.iter().map(|r| ((r.members, r.snaps), r)).collect();
    let (mut compared, mut occupied, mut outside, mut worse) = (0u64, 0u64, 0u64, 0u64);
    let mut witness = None;
    for a in rows {
        let pos = if rule.kind == "member" {
            a.members.iter().position(|id| *id == rule.from)
        } else {
            a.snaps.iter().position(|id| *id == Some(rule.from))
        };
        let Some(pos) = pos else { continue };
        let already =
            if rule.kind == "member" { a.members.contains(&rule.to) } else { a.snaps.contains(&Some(rule.to)) };
        if already {
            occupied += 1;
            continue;
        }
        let mut key = (a.members, a.snaps);
        if rule.kind == "member" {
            key.0[pos] = rule.to;
        } else {
            key.1[pos] = Some(rule.to);
        }
        let key = canonical(key);
        let Some(b) = index.get(&key) else {
            outside += 1;
            continue;
        };
        compared += 1;
        if compare(b, a) == Ordering::Greater {
            worse += 1;
            if witness.is_none() {
                witness = Some(json!({"original":a,"replacement":b}));
            }
        }
    }
    Ok(json!({"rule":rule,"compared":compared,"occupiedReplacement":occupied,
        "outsideDomain":outside,"worseCanonicalResults":worse,
        "allSubstitutionsNonWorse":compared>0 && occupied==0 && outside==0 && worse==0,
        "scope":"exhaustive configured team domain over the 120 performance orders only",
        "authorizesPruning":false,"warning":"Top-K recovery and resource occupancy need separate proof",
        "firstCounterexample":witness}))
}

fn run(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)?;
    let case: Case = serde_json::from_str(&text)?;
    let root = path.parent().unwrap_or(Path::new("."));
    let data = DeckData::from_path(root.join(&case.data))?;
    let roster = Roster::from_json(&fs::read_to_string(root.join(&case.roster))?)?;
    let wire: Value = serde_json::from_str(&fs::read_to_string(root.join(&case.request))?)?;
    let mut exact_wire = wire.clone();
    merge(
        &mut exact_wire,
        &json!({"strategy":{"kind":"exhaustive"},
        "limits":{"timeLimitMs":null,"maxCandidates":null}}),
    );
    let exact: RecommendationRequest = serde_json::from_value(exact_wire.clone())?;
    if !matches!(exact.execution, types::Execution::Live { .. }) {
        return Err("harness primary cases must use Live (Snap skills included)".into());
    }
    if exact.k == 0 || exact.k > types::MAX_K {
        return Err("invalid k".into());
    }
    let pool = Pool::new(&data.master, &roster)?;
    let keys = domain(&pool, &exact, case.oracle_max_candidates)?;
    let begin = Instant::now();
    let compiled = handler::build_card_pool(&data, &roster, &exact)?;
    let mut rows = Vec::with_capacity(keys.len());
    let mut oracle_simulations = 0u64;
    for (members, snaps) in keys {
        let evaluated = auxiliary::evaluate_built(&compiled, members, snaps)?;
        if evaluated.completion != Completion::Complete || evaluated.results.len() != 1 {
            return Err("fixed evaluator failed to finish the oracle candidate".into());
        }
        oracle_simulations += evaluated.telemetry.leaves.simulations;
        rows.push(evaluated.results.into_iter().next().unwrap());
    }
    rows.sort_by(compare);
    let oracle_ms = begin.elapsed().as_secs_f64() * 1000.;
    let mut bounded_request = exact.clone();
    bounded_request.strategy = types::Strategy::BranchAndBound;
    let mut bounded = handler::build_card_pool(&data, &roster, &bounded_request)?;
    // Audit the strongest class cap independently of which search schedule runs.
    ournotes_search::search::diagnostics::prepare_class_audit(&mut bounded)?;
    let member_caps = ournotes_search::search::diagnostics::member_pt_caps(&bounded);
    let mut member_cap_checks = 0u64;
    let mut bound_checks = 0u64;
    let mut unavailable_bounds = 0u64;
    let mut suffix_checks = 0u64;
    let mut unavailable_suffixes = 0u64;
    let mut prefix_scratch = ournotes_search::search::diagnostics::PrefixAuditScratch::default();
    let mut expected_bonus_checks = 0u64;
    let mut composition_checks = 0u64;
    let mut team_checks = 0u64;
    let mut module_checks = 0u64;
    let mut order_cap_checks = 0u64;
    let mut class_checks = 0u64;
    let mut class_binding_checks = 0u64;
    for row in &rows {
        // Every bound is a payoff numerator over the 120 performance orders.
        let numerator = row.expected_payoff.as_ref().expect("exact deterministic fixture").numerator.parse::<i128>()?;
        for bindings in [false, true] {
            for depth in 0..=5 {
                if let Some((cap, power)) = ournotes_search::search::diagnostics::class_prefix_upper(
                    &bounded,
                    row.members,
                    row.snaps,
                    depth,
                    bindings,
                    &mut prefix_scratch,
                )? {
                    if cap < numerator || power < i64::from(row.power) {
                        return Err(format!("inadmissible class bound: bindings={bindings} depth={depth}").into());
                    }
                    if bindings {
                        class_binding_checks += 1;
                    } else {
                        class_checks += 1;
                    }
                }
            }
        }
        for team in [false, true] {
            for depth in usize::from(!team)..=5 {
                if let Some((cap, power)) = ournotes_search::search::diagnostics::split_prefix_upper(
                    &bounded,
                    row.members,
                    row.snaps,
                    depth,
                    team,
                )? {
                    if cap < numerator || power < i64::from(row.power) {
                        return Err(format!("inadmissible split bound: team={team} depth={depth}").into());
                    }
                    if team {
                        team_checks += 1;
                    } else {
                        composition_checks += 1;
                    }
                }
            }
        }
        for depth in 1..=5 {
            for module in
                ournotes_search::search::diagnostics::module_prefix_uppers(&bounded, row.members, row.snaps, depth)?
            {
                let Some(cap) = module.upper else { continue };
                if cap < numerator {
                    return Err(format!(
                        "inadmissible {} bound: depth={depth} snapsPlaced={} deck={:?}",
                        module.name, module.snaps_placed, row.members
                    )
                    .into());
                }
                module_checks += 1;
            }
        }
        for depth in 1..5 {
            if let Some(cap) = ournotes_search::search::diagnostics::bonus_expected_prefix_upper(
                &bounded,
                row.members,
                row.snaps,
                depth,
                &mut prefix_scratch,
            )? {
                if numerator > cap {
                    return Err("inadmissible expected bonus cap".into());
                }
                expected_bonus_checks += 1;
            }
        }
        if let Some(caps) = &member_caps {
            let best = row.best_order.as_ref().ok_or("played result without a best order")?.payoff.parse::<i128>()?;
            for member in row.members {
                let cap = caps.iter().find(|(m, _)| *m == member).expect("original member").1;
                if best > cap {
                    return Err("inadmissible per-member PT cap".into());
                }
                member_cap_checks += 1;
            }
        }
        let orders = ournotes_search::search::diagnostics::audit_order_caps(&bounded, row.members, row.snaps)?;
        if orders["violations"].as_u64().unwrap_or(0) != 0 {
            return Err(format!("inadmissible per-order cap: deck={:?} {}", row.members, orders["first"]).into());
        }
        order_cap_checks += orders["orders"].as_u64().unwrap_or(0);
        for depth in 1..=5 {
            if depth < 5 {
                if let Some(caps) =
                    ournotes_search::search::diagnostics::next_choice_bounds(&bounded, row.members, row.snaps, depth)?
                {
                    for (kind, (payoff, power)) in [("suffix", caps.suffix), ("pair", caps.pair)] {
                        if power < i64::from(row.power) || numerator > payoff {
                            return Err(
                                format!("inadmissible {kind} bound: depth={depth} deck={:?}", row.members).into()
                            );
                        }
                    }
                    suffix_checks += 1;
                } else {
                    unavailable_suffixes += 1;
                }
            }
            if let Some((payoff, power)) = ournotes_search::search::diagnostics::prefix_upper(
                &bounded,
                row.members,
                row.snaps,
                depth,
                &mut prefix_scratch,
            )? {
                if power < i64::from(row.power) || numerator > payoff {
                    return Err(format!("inadmissible joint bound: depth={depth} deck={:?}", row.members).into());
                }
                bound_checks += 1;
            } else {
                unavailable_bounds += 1;
            }
        }
    }
    let reference: Vec<_> = rows.iter().take(exact.k).cloned().collect();
    let mut bound_explanations = Vec::new();
    for deck in &reference {
        if let Some(best) = &deck.best_order {
            bound_explanations.push(json!({"order":best.performance_order,"actualScore":best.score,
                "actualPayoff":best.payoff,"bound":ournotes_search::search::diagnostics::describe_bound(
                    &bounded,deck.members,deck.snaps,best.performance_order)?}));
        }
    }
    let audits: Vec<_> = case.dominance.iter().map(|r| dominance(&rows, &pool, r)).collect::<Result<_>>()?;
    let mut experiments = Vec::new();
    for experiment in case.experiments {
        if experiment.repeats == 0 {
            return Err("experiment repeats must be positive".into());
        }
        // Only traversal, budget, cache and explicit candidate exclusions vary.
        let allowed = ["strategy", "limits", "constraints"];
        let patch = experiment.patch.as_object().ok_or("experiment patch must be an object")?;
        if patch.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err("experiment cannot change scoring inputs".into());
        }
        if let Some(c) = patch.get("constraints") {
            let obj = c.as_object().ok_or("constraints patch must be an object")?;
            if obj.keys().any(|k| !["excludeMembers", "excludeSnaps"].contains(&k.as_str())) {
                return Err("only extra exclusions are experimental".into());
            }
            for key in ["excludeMembers", "excludeSnaps"] {
                if let Some(ids) = obj.get(key) {
                    let old = wire.get("constraints").and_then(|v| v.get(key)).and_then(Value::as_array);
                    let new = ids.as_array().ok_or("exclusions must be arrays")?;
                    if old.is_some_and(|a| a.iter().any(|id| !new.contains(id))) {
                        return Err("cannot widen oracle constraints".into());
                    }
                }
            }
        }
        let mut changed = exact_wire.clone();
        merge(&mut changed, &experiment.patch);
        let request: RecommendationRequest = serde_json::from_value(changed)?;
        let is_reduced = patch.get("constraints").is_some();
        let mut samples = Vec::new();
        for _ in 0..experiment.repeats {
            let start = Instant::now();
            let result = ournotes_search::search::diagnostics::recommend_experiment(
                &data,
                &roster,
                &request,
                experiment.schedule,
            )?;
            let wall_ms = start.elapsed().as_secs_f64() * 1000.;
            let mut returned_verified = true;
            for item in &result.results {
                let source = rows.iter().find(|r| r.members == item.members && r.snaps == item.snaps);
                returned_verified &= source == Some(item);
            }
            let matches_top_k = result.results == reference;
            let wrong_complete = !is_reduced && result.completion == Completion::Complete && !matches_top_k;
            let gap = reference.first().zip(result.results.first()).map(|(a, b)| {
                (a.expected_payoff.as_ref().expect("exact deterministic fixture").numerator.parse::<i128>().unwrap()
                    - b.expected_payoff
                        .as_ref()
                        .expect("exact deterministic fixture")
                        .numerator
                        .parse::<i128>()
                        .unwrap())
                .to_string()
            });
            samples.push(json!({"wallMs":wall_ms,"outcome":result,"returnedValuesVerified":returned_verified,
                "matchesFullDomainTopK":matches_top_k,"wrongComplete":wrong_complete,
                "top1GapNumerator":gap,"gapDenominator":reference.first().map(|r|&r.expected_payoff.as_ref().expect("exact deterministic fixture").denominator)}));
        }
        experiments.push(json!({"name":experiment.name,"patch":experiment.patch,"schedule":experiment.schedule,
            "reducedDomain":is_reduced,"samples":samples}));
    }
    Ok(json!({"format":"ournotes-deck.search-harness/1","case":case.id,
        "oracle":{"completion":"complete","identity":"team","candidates":rows.len(),
            "simulations":oracle_simulations,"wallMs":oracle_ms,"topK":reference},
        "jointBoundAudit":{"checkedCharacterPrefixes":prefix_scratch.checked_character_prefixes,"checkedOrderLeaves":prefix_scratch.checked_order_leaves,"checkedOrderCaps":order_cap_checks,"checkedBoundModules":module_checks,"checkedResourcePrefixes":prefix_scratch.checked_resource_prefixes,"checkedCarrierSplitPrefixes":prefix_scratch.checked_carrier_split_prefixes,"classBoundComputations":prefix_scratch.class_bound_computations,"classBoundCacheHits":prefix_scratch.class_bound_cache_hits,"checkedClasses":class_checks,"checkedClassBindings":class_binding_checks,"checkedCompositions":composition_checks,"checkedTeams":team_checks,"checkedExpectedBonusPrefixes":expected_bonus_checks,"checkedBonusPrefixes":prefix_scratch.checked_bonus_prefixes,"checkedMemberPtCaps":member_cap_checks,"checkedPrefixes":bound_checks,"unavailablePrefixes":unavailable_bounds,"checkedSuffixes":suffix_checks,"checkedNextPairs":suffix_checks,"unavailableSuffixes":unavailable_suffixes},
        "topKBoundExplanations":bound_explanations,
        "dominanceAudits":audits,"experiments":experiments,
        "evidenceScope":"search equivalence under shared model; not native-game validation",
        "caseDefinition":serde_json::from_str::<Value>(&text)?}))
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: ournotes-search-harness CASE.json REPORT.json");
        std::process::exit(2);
    }
    match run(Path::new(&args[0])) {
        Ok(report) => {
            let bad = report["experiments"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|e| e["samples"].as_array().unwrap())
                .any(|s| s["wrongComplete"] == true || s["returnedValuesVerified"] == false);
            fs::write(&args[1], serde_json::to_vec_pretty(&report).unwrap()).expect("write report");
            if bad {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}", json!({"format":"ournotes-deck.search-harness-error/1","error":e.to_string()}));
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(n: &str, m: i64, s: Option<i64>) -> RecommendedDeck {
        RecommendedDeck {
            members: [m, 2, 3, 4, 5],
            snaps: [s, None, None, None, None],
            power: 100,
            expected_score: None,
            expected_payoff: Some(types::Fraction { numerator: n.into(), denominator: "9007199254740997".into() }),
            score_interval: None,
            payoff_interval: None,
            rank_certified: None,
            score_summary: None,
            best_order: None,
            order_outcomes: Vec::new(),
        }
    }
    #[test]
    fn ordering_preserves_large_integer_and_ties() {
        assert_eq!(compare(&row("9007199254740993", 1, None), &row("9007199254740992", 1, None)), Ordering::Less);
        assert_eq!(compare(&row("1", 1, None), &row("1", 1, Some(1))), Ordering::Less);
        assert_eq!(compare(&row("1", 1, None), &row("1", 2, None)), Ordering::Less);
    }
    #[test]
    fn patch_preserves_unspecified_budget_and_semantics() {
        let mut v = json!({"limits":{"cacheEntries":5},"metric":{"kind":"score"}});
        merge(&mut v, &json!({"limits":{"maxCandidates":7}}));
        assert_eq!(v["limits"]["cacheEntries"], 5);
        assert_eq!(v["metric"]["kind"], "score");
    }
}
