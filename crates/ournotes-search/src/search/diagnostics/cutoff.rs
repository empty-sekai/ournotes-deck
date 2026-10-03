//! Audit of the simulation cutoff: settle after every frame of complete simulations and check the settled prefixes
//! and caps against the final scores.
use super::PhysicalDeck;
use crate::handler::BuiltProblem;
use ournotes_sim::Error;
use ournotes_sim::live::full::Settled;
use ournotes_sim::live::score::get_frame;

/// For each deck and distinct root: settle after every frame of a complete simulation, then check that
/// - no settled score frame was undone later (the settledness argument held),
/// - every settled total equals the final note and fixed scores of its frames (they never changed afterwards),
/// - every cap (settled total plus the cutoff table's remainder) is at least the final score.
///
/// Also reports how close the cap gets to the final score along the live (the cutoff's reach). The tables use the
/// candidate's exact power and Rush masks, as the search does; the relaxed power of the leaf fine bound must not be
/// below it. Where the LUCK replay has several branches, the maximum of the per-branch tables' caps must also be at
/// least the final score at every check (the per-branch argument of the leaf fine bound).
pub fn audit_cutoff(
    built: &BuiltProblem<'_>,
    decks: &[([i64; 5], [Option<i64>; 5])],
) -> Result<serde_json::Value, Error> {
    let Some(b) = built.context.plan.joint.as_ref() else {
        return Err(Error::Domain("cutoff audit requires joint bounds".into()));
    };
    let mut luck = super::super::luck::LuckOracle::new(
        &built.pool,
        &built.context.request,
        built.domain(),
        &built.context.law,
        built.context.plan.joint.as_ref().and_then(|b| b.luck_life()),
    )?;
    let roots: std::collections::BTreeSet<_> = built.context.law.atoms().iter().map(|&(root, _)| root).collect();
    let mut scratch = super::super::snaps::JointScratch::default();
    let (mut violations, mut checks, mut unavailable, mut relaxed_above) = (0u64, 0u64, 0u64, 0u64);
    let (mut branch_checks, mut branch_violations) = (0u64, 0u64);
    let plan = &built.context.plan;
    let mut out = Vec::new();
    for &(members, snaps) in decks {
        let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
        let p = PhysicalDeck { members: d.members, snaps: d.snaps };
        built.domain().check_fixed(&built.pool, &p)?;
        let input = super::super::expectation::context(&built.pool, &p, &built.context.request.objective)?;
        let power = i64::from(built.pool.deck_power(&p.as_deck(), plan.song.as_ref(), plan.event)?.power());
        for &root in &roots {
            let positions = super::super::joint::positions(root)?;
            let (_, relaxed) = b.upper(&built.pool, built.domain(), &p, 5, &positions);
            if relaxed < power {
                return Err(Error::Domain(format!("relaxed power {relaxed} below exact power {power}")));
            }
            relaxed_above += u64::from(relaxed > power);
            let masks = match (&mut luck, b.rush_eligible()) {
                (Some(oracle), true) => oracle.masks(&built.pool, &p, &positions)?,
                _ => None,
            };
            let Some(table) = b.cutoff_table(built.domain(), &p, power, &positions, &mut scratch, masks.as_ref())
            else {
                unavailable += 1;
                out.push(serde_json::json!({"members":members,"snaps":snaps,"root":root,"table":null}));
                continue;
            };
            let branch_tables = match (&mut luck, b.rush_eligible()) {
                (Some(oracle), true) => oracle.branches(&built.pool, &p, &positions)?.and_then(|list| {
                    list.iter()
                        .map(|m| b.cutoff_table(built.domain(), &p, power, &positions, &mut scratch, Some(m)))
                        .collect::<Option<Vec<_>>>()
                }),
                _ => None,
            };
            let mut settled: Vec<Settled> = Vec::new();
            let outcome = input
                .simulate_with_cutoff(built.pool.master, root, 1, |s| {
                    settled.push(s);
                    false
                })?
                .ok_or_else(|| Error::Game("audit simulation stopped".into()))?;
            let final_score = i64::from(outcome.final_score);
            let undone = outcome.model.settled_violations();
            let (notes, fixed) = outcome.model.filed_scores();
            let mut bad = Vec::new();
            if undone != 0 {
                bad.push(serde_json::json!({"undoneSettledFrames": undone}));
            }
            let mut reach = Vec::new();
            for (i, s) in settled.iter().enumerate() {
                checks += 1;
                let exact: i64 = notes.iter().filter(|n| get_frame(n.0) < s.frame).map(|n| i64::from(n.2)).sum::<i64>()
                    + fixed.iter().filter(|f| f.0 < s.frame).map(|f| i64::from(f.1)).sum::<i64>();
                let cap = table.score_cap(*s);
                if exact != s.total || cap.is_none_or(|c| c < i128::from(final_score)) {
                    if bad.len() < 8 {
                        bad.push(serde_json::json!({"check":i,"frame":s.frame,"settled":s.total,"exact":exact,
                            "cap":cap.map(|c| c.to_string()),"final":final_score}));
                    }
                    violations += 1;
                }
                if let Some(tables) = &branch_tables {
                    branch_checks += 1;
                    let cap = tables.iter().map(|t| t.score_cap(*s)).collect::<Option<Vec<_>>>();
                    if cap.and_then(|v| v.into_iter().max()).is_none_or(|c| c < i128::from(final_score)) {
                        if bad.len() < 8 {
                            bad.push(serde_json::json!({"branchCheck":i,"frame":s.frame,"final":final_score}));
                        }
                        branch_violations += 1;
                    }
                }
                if (i + 1) % (settled.len() / 20).max(1) == 0 {
                    reach.push(serde_json::json!([i, s.frame, cap.map(|c| c.to_string())]));
                }
            }
            let zero = Settled { frame: 0, total: 0, fixed: 0 };
            let branch_full = branch_tables.as_ref().and_then(|tables| {
                tables.iter().map(|t| t.score_cap(zero)).collect::<Option<Vec<_>>>()?.into_iter().max()
            });
            out.push(serde_json::json!({"members":members,"snaps":snaps,"root":root,"final":final_score,
                "fullCap":table.score_cap(Settled { frame: 0, total: 0, fixed: 0 }).map(|c| c.to_string()),
                "checks":settled.len(),"violations":bad,"reach":reach,"rushRefined":masks.is_some(),
                "power":power,"relaxedPower":relaxed,"branches":branch_tables.as_ref().map(Vec::len),
                "branchFullCap":branch_full.map(|c| c.to_string())}));
        }
    }
    Ok(serde_json::json!({"scope":"Settled prefixes after every frame of complete simulations; exact settled totals, \
        no undo of a settled frame, cap at least the final score. Sampled evidence for the settledness argument.",
        "checks":checks,"violations":violations + branch_violations,"unavailableTables":unavailable,
        "relaxedPowerAbove":relaxed_above,"branchChecks":branch_checks,"branchViolations":branch_violations,
        "decks":out}))
}
