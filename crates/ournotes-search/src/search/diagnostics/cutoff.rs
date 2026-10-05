//! Audit of the simulation cutoff: settle after every frame of complete simulations and check the settled prefixes
//! and caps against the final scores.
use crate::handler::BuiltProblem;
use ournotes_sim::Error;
use ournotes_sim::live::full::Settled;
use ournotes_sim::live::score::get_frame;

/// For each deck and each audited performance order (`super::audit_orders`): settle after every frame of a complete
/// simulation, then check that
/// - no settled score frame was undone later (the settledness argument held),
/// - every settled total equals the final note and fixed scores of its frames (they never changed afterwards),
/// - every cap (settled total plus the cutoff table's remainder) is at least the final score.
///
/// Also reports how close the cap gets to the final score along the live (the cutoff's reach). The tables use the
/// candidate's exact power, as the search does.
pub fn audit_cutoff(
    built: &BuiltProblem<'_>,
    decks: &[([i64; 5], [Option<i64>; 5])],
) -> Result<serde_json::Value, Error> {
    let Some(b) = built.context.plan.joint.as_ref() else {
        return Err(Error::Domain("cutoff audit requires joint bounds".into()));
    };
    let mut scratch = super::super::snaps::JointScratch::default();
    let (mut violations, mut checks, mut unavailable) = (0u64, 0u64, 0u64);
    let mut out = Vec::new();
    for &(members, snaps) in decks {
        let p = super::physical(built, members, snaps)?;
        let input = super::super::expectation::context(&built.pool, &p, &built.context.request.objective)?;
        let power = super::power_of(built, &p)?;
        for order in super::audit_orders() {
            let positions = super::uniform::positions_of(&order);
            let Some(table) = b.cutoff_table(built.domain(), &p, power, &positions, &mut scratch) else {
                unavailable += 1;
                out.push(serde_json::json!({"members":members,"snaps":snaps,"order":order,"table":null}));
                continue;
            };
            let mut settled: Vec<Settled> = Vec::new();
            let outcome = input
                .simulate_performance_order_with_cutoff(built.pool.master, order, 1, |s| {
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
                if (i + 1) % (settled.len() / 20).max(1) == 0 {
                    reach.push(serde_json::json!([i, s.frame, cap.map(|c| c.to_string())]));
                }
            }
            out.push(serde_json::json!({"members":members,"snaps":snaps,"order":order,"final":final_score,
                "fullCap":table.score_cap(Settled { frame: 0, total: 0, fixed: 0 }).map(|c| c.to_string()),
                "checks":settled.len(),"violations":bad,"reach":reach,"power":power}));
        }
    }
    Ok(serde_json::json!({"scope":"Settled prefixes after every frame of complete simulations at the audited \
        performance orders; exact settled totals, no undo of a settled frame, cap at least the final score. Sampled \
        evidence for the settledness argument.",
        "checks":checks,"violations":violations,"unavailableTables":unavailable,"decks":out}))
}
