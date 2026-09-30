//! Numeric portability harness. Runs the identical production fixed-deck evaluator,
//! with no Instant, OS imports, copied random implementation, or search completion claim.
use ournotes_deck::{
    cards::Roster,
    data::DeckData,
    live::model::{JudgementStream, JustRule},
    replay::RankConfirmation,
    scenario::{ContextInput, Scenario},
    search::{
        GekisouObjective, Objective, PlayInput, SeedSet, expectation,
        recommendation::{SimulationInput, evaluate_declared_context},
    },
};
use serde_json::{Value, json};

pub fn evaluate_json(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let data = DeckData::from_json(v["data"].as_str().ok_or("data JSON text required")?).map_err(|e| e.to_string())?;
    let roster = Roster::from_json(v["roster"].as_str().ok_or("roster JSON text required")?).map_err(|e| e.to_string())?;
    let score_id = v["scoreId"].as_i64().ok_or("scoreId required")?;
    let gk = v["gekisou"].as_bool().ok_or("gekisou required")?;
    let id = v["musicId"].as_i64().ok_or("musicId required")?;
    let scenario = match v["scene"].as_str().ok_or("scene required")? {
        "free" => Scenario::Free(id),
        "mission" => Scenario::Mission(id),
        "battle" => Scenario::Battle(id),
        "arena" => Scenario::Arena(id),
        "challenge" => Scenario::Challenge(id),
        _ => return Err("unknown scene".into()),
    };
    let ci: ContextInput = serde_json::from_value(v["context"].clone()).map_err(|e| e.to_string())?;
    let dc = data.data_chart(score_id).ok_or("chart required")?;
    let chart = data.chart(score_id).map_err(|e| e.to_string())?;
    let ctx = ci.resolve(&data.master, scenario, Some(score_id), &dc.fevers).map_err(|e| e.to_string())?;
    let pool = ctx.pool(&data.master, &roster).map_err(|e| e.to_string())?;
    let stream = if let Some(stream) = v.get("stream") {
        serde_json::from_value(stream.clone()).map_err(|e| e.to_string())?
    } else if gk {
        let rule = JustRule::new(&data.master, &ctx.gekisou).map_err(|e| e.to_string())?;
        JudgementStream::theoretical_best_gekisou(&chart, &dc.judgement_types, &rule).map_err(|e| e.to_string())?
    } else {
        JudgementStream::theoretical_best(&chart)
    };
    let objective = Objective::LiveScore {
        score_id,
        chart,
        play: PlayInput::Stream { stream, judgement_types: dc.judgement_types.clone() },
        event: false,
        exclude_snap_skills: false,
        gekisou: gk.then(|| GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: dc.fevers.clone() }),
    }
    .in_scenario(ctx);
    let members: [i64; 5] = serde_json::from_value(v["members"].clone()).map_err(|e| e.to_string())?;
    let snaps: [Option<i64>; 5] = serde_json::from_value(v["snaps"].clone()).map_err(|e| e.to_string())?;
    let deck = pool.deck(members, snaps, [0, 1, 2, 3, 4]).map_err(|e| e.to_string())?;
    let physical = expectation::PhysicalDeck { members: deck.members, snaps: deck.snaps };
    let input = expectation::context(&pool, &physical, &objective).map_err(|e| e.to_string())?;
    let network: Option<Vec<RankConfirmation>> =
        serde_json::from_value(v.get("networkConfirmations").cloned().unwrap_or(Value::Null)).map_err(|e| e.to_string())?;
    let simulation: SimulationInput =
        serde_json::from_value(v.get("simulation").cloned().unwrap_or_else(|| json!({}))).map_err(|e| e.to_string())?;
    let roots: Vec<i32> = serde_json::from_value(v["roots"].clone()).map_err(|e| e.to_string())?;
    let mut atoms = Vec::new();
    for root in roots {
        let (terminal, applied) =
            evaluate_declared_context(&data.master, &physical, &input, root, network.as_deref(), &simulation)
                .map_err(|e| e.to_string())?;
        let ranges: Vec<_> = terminal.model.gekisou_ranges().iter().map(|r| {
            json!({"mission":r.mission,"state":r.state,"combo":r.combo,"maxCombo":r.max_combo,"justCount":r.just_count,
                "startScore":r.start_score,"endScore":r.end_score,"luckPoints":r.luck_points,"luckGauge":r.luck_gauge,
                "rushCombo":r.rush_combo,"lotResults":r.lot_results,"rankBonus":r.rank_bonus})
        }).collect();
        atoms.push(json!({"root":root,"score":terminal.final_score,"order":terminal.performance_order,
            "draws":terminal.model.draws(),"applications":applied,"ranges":ranges,"trace":terminal.model.trace()}));
    }
    serde_json::to_vec(&json!({"power":input.params.total_power,"atoms":atoms})).map_err(|e| e.to_string())
}

/// One allocation per host request/result; host must release both with their exact length.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()) as *mut u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn release(ptr: *mut u8, len: usize) {
    unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len))) };
}

/// wasm32 packs (result length <<32 | pointer), consumed as BigInt by Node.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn evaluate(ptr: *const u8, len: usize) -> u64 {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let out = evaluate_json(input).unwrap_or_else(|e| serde_json::to_vec(&json!({"error":e})).unwrap());
    let length = out.len() as u64;
    let pointer = Box::into_raw(out.into_boxed_slice()) as *mut u8 as usize;
    (length << 32) | pointer as u64
}

