//! Model-only exporter/verifier for an independent Python PT plateau certificate.
//! No production search, bound, Hungarian matcher, or ranking code is used here.
use ournotes_search::{
    auxiliary, handler,
    types::{Execution, Metric, RecommendationRequest, Scene, Strategy},
};
use ournotes_sim::{bonus, cards::Roster, data::DeckData, event, scenario::Scenario};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn integer(v: &Value) -> Result<i128> {
    Ok(v.as_str().ok_or("expected integer string")?.parse()?)
}
fn permutations(a: &mut [i64; 5], depth: usize, out: &mut Vec<[i64; 5]>) {
    if depth == 5 {
        out.push(*a);
        return;
    }
    for j in depth..5 {
        a.swap(depth, j);
        permutations(a, depth + 1, out);
        a.swap(depth, j);
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 6 && args.len() != 7 {
        return Err(
            "export DATA ROSTER REQUEST INCUMBENT OUT | verify DATA ROSTER REQUEST INCUMBENT PROPOSALS OUT".into()
        );
    }
    let data = DeckData::from_path(&args[1])?;
    let roster = Roster::from_json(&fs::read_to_string(&args[2])?)?;
    let mut request: RecommendationRequest = serde_json::from_str(&fs::read_to_string(&args[3])?)?;
    let incumbent: Value = serde_json::from_str(&fs::read_to_string(&args[4])?)?;
    request.strategy = Strategy::Exhaustive;
    request.limits.time_limit_ms = None;
    request.limits.max_candidates = None;
    let built = handler::build_card_pool(&data, &roster, &request)?;
    let result = incumbent["results"].as_array().ok_or("missing results")?;
    if result.len() != request.k {
        return Err("certificate needs K incumbents".into());
    }
    if args[0] == "verify" {
        if args.len() != 7 {
            return Err("verify needs proposal and output paths".into());
        }
        let proposals: Value = serde_json::from_str(&fs::read_to_string(&args[5])?)?;
        let cap = integer(&proposals["globalPtCap"])?;
        let mut evaluated = Vec::new();
        for row in proposals["results"].as_array().ok_or("missing proposals")? {
            let members: [i64; 5] = serde_json::from_value(row["members"].clone())?;
            let snaps: [Option<i64>; 5] = serde_json::from_value(row["snaps"].clone())?;
            let out = auxiliary::evaluate_built(&built, members, snaps)?;
            let deck = out.results.first().ok_or("fixed evaluator incomplete")?;
            if i128::from(deck.power) != row["power"].as_i64().ok_or("power")? as i128
                || deck.expected_payoff.numerator.parse::<i128>()?
                    != cap * deck.expected_payoff.denominator.parse::<i128>()?
            {
                return Err("power-leading assignment does not attain global PT cap".into());
            }
            evaluated.push(serde_json::to_value(deck)?);
        }
        if &evaluated != result {
            return Err("independent power Top-K differs from incumbent".into());
        }
        fs::write(
            &args[6],
            serde_json::to_vec_pretty(&json!({"verified":true,"globalPtCap":cap.to_string(),
            "topK":evaluated,"scope":"original-domain PT ceiling plus independent complete power assignment DP; fixed scorer shared"}))?,
        )?;
        return Ok(());
    }
    if args[0] != "export" || args.len() != 6 {
        return Err("unknown mode".into());
    }
    let event_id = match &request.metric {
        Metric::ClientEventPoints { event_id } => *event_id,
        _ => return Err("PT only".into()),
    };
    let scenario = match &request.scenario {
        Some(Scene::Mission { music_id }) => Scenario::Mission(*music_id),
        Some(Scene::Free { music_id }) => Scenario::Free(*music_id),
        _ => return Err("solo normal-played certificate only".into()),
    };
    let sid = match &request.execution {
        Execution::Live { score_id, .. } => *score_id,
        _ => return Err("played Live only".into()),
    };
    let chart = data.charts.iter().find(|c| c.score_id == sid).ok_or("chart")?;
    let input = request.context.as_ref().ok_or("context")?;
    let context = input.resolve(&data.master, scenario, Some(sid), &chart.fevers)?;
    let q = context.event_request(&data.master, input.event_payoff.as_ref().ok_or("event input")?, event_id)?;
    if !matches!(q.route, event::EventResultRoute::NormalPlayed) || q.holding_event_ids != [event_id] {
        return Err("single normal-played event only".into());
    }
    let pool = built.pool();
    // Resolve the original hard domain independently from the raw pool IDs.
    let member_indexes: Vec<_> = (0..pool.members.len())
        .filter(|&m| !request.constraints.exclude_members.contains(&pool.members[m].id))
        .collect();
    let snap_indexes: Vec<_> = (0..pool.snaps.len())
        .filter(|&s| !request.constraints.no_snaps && !request.constraints.exclude_snaps.contains(&pool.snaps[s].id))
        .collect();
    let mut required = request.constraints.include_members.clone();
    if let Some(m) = request.constraints.leader
        && !required.contains(&m)
    {
        required.push(m);
    }

    let effects = [event::event_effects(&data.master, event_id)];
    let members: Vec<_> = member_indexes
        .iter()
        .map(|&m| {
            let card = &pool.members[m];
            let b = event::total_effect_10000(
                &effects,
                Some(event::EventCard::Member(&bonus::event_member(&data.master, card))),
                event::EVENT_POINT,
            )?;
            Ok((card.id, card.character_id, i64::from(b)))
        })
        .collect::<Result<_>>()?;
    let snaps: Vec<_> = snap_indexes
        .iter()
        .map(|&s| {
            let card = &pool.snaps[s];
            let b = event::total_effect_10000(
                &effects,
                Some(event::EventCard::Snap(&bonus::event_snap(card))),
                event::EVENT_POINT,
            )?;
            Ok((card.id, i64::from(b)))
        })
        .collect::<Result<_>>()?;
    if members.iter().any(|m| m.2 < 0) || snaps.iter().any(|s| s.1 < 0) {
        return Err("negative bonuses outside certificate".into());
    }
    let group = data.master.event(event_id).ok_or("event")?.live_event_point_group;
    // A maximum over ALL rows is conservative, even if some rows are unreachable or shadowed.
    let reward =
        data.master.live_event_points.iter().filter(|r| r.group == group).map(|r| r.value).max().ok_or("rewards")?;
    let rate = event::boost_bonus(&data.master, i64::from(q.consumed_count))?[4];
    let max_bonus = 5 * (members.iter().map(|m| m.2).max().unwrap_or(0) + snaps.iter().map(|s| s.1).max().unwrap_or(0));
    if rate < 0
        || reward < 0
        || (i128::from(max_bonus) + 10000) * i128::from(rate) * i128::from(reward) > i128::from(i32::MAX)
        || (i128::from(max_bonus) + 10000) * i128::from(rate) > i128::from(i32::MAX)
    {
        return Err("wrapping outside certificate".into());
    }
    let mut chars = BTreeMap::<i64, i64>::new();
    for &(_, c, b) in &members {
        let x = chars.entry(c).or_default();
        *x = (*x).max(b);
    }
    let mut sb: Vec<_> = snaps.iter().map(|s| s.1).collect();
    sb.sort_unstable_by(|a, b| b.cmp(a));
    let sb: i64 = sb.iter().take(5).sum();
    let caps: Vec<_> = members
        .iter()
        .map(|&(m, c, b)| {
            let mut others: Vec<_> = chars.iter().filter(|(ch, _)| **ch != c).map(|(_, v)| *v).collect();
            others.sort_unstable_by(|a, b| b.cmp(a));
            (m, (b + sb + others.iter().take(4).sum::<i64>() + 10000) * rate * reward / 10000)
        })
        .collect();
    let numerator = integer(&result.last().ok_or("K")?["expectedPayoff"]["numerator"])?;
    let denominator = integer(&result.last().ok_or("K")?["expectedPayoff"]["denominator"])?;
    let selected: Vec<_> =
        caps.iter().filter(|(_, c)| i128::from(*c) * denominator >= numerator).map(|(m, _)| *m).collect();
    if selected.len() != 5 {
        return Err("certificate requires exactly five qualifying members".into());
    }
    let mut a: [i64; 5] = selected.clone().try_into().map_err(|_| "five")?;
    let mut layouts = Vec::new();
    permutations(&mut a, 0, &mut layouts);
    layouts.sort_unstable();
    let mut matrices = Vec::new();
    for layout in layouts {
        if request.constraints.leader.is_some_and(|m| m != layout[2]) {
            continue;
        }
        if required.iter().any(|m| !layout.contains(m)) {
            continue;
        }
        let base = pool.deck_power(
            &pool.deck(layout, [None; 5], [0, 1, 2, 3, 4])?,
            Some(&context.resolved.power_music),
            context.resolved.calc_event_parameter,
        )?;
        let points = |v: ournotes_sim::power::CardPower| -> Result<i64> {
            let values = [v.performance, v.technique, v.visual];
            if values.iter().any(|&x| x < 0 || x % 10000 != 0 || x > i64::from(i32::MAX) * 10000) {
                return Err("nonintegral/negative slot outside certificate".into());
            }
            Ok(values.iter().map(|x| x / 10000).sum())
        };
        let none: Vec<_> = base.slots.iter().map(|s| points(s.total)).collect::<Result<_>>()?;
        let mut matrix = vec![Vec::new(); 5];
        for slot in 0..5 {
            for &(id, _) in &snaps {
                let mut binding = [None; 5];
                binding[slot] = Some(id);
                let value = pool.deck_power(
                    &pool.deck(layout, binding, [0, 1, 2, 3, 4])?,
                    Some(&context.resolved.power_music),
                    context.resolved.calc_event_parameter,
                )?;
                if (0..5).any(|s| s != slot && value.slots[s].total != base.slots[s].total) {
                    return Err("cross-slot power dependence".into());
                }
                matrix[slot].push(points(value.slots[slot].total)?);
            }
        }
        if (0..5).map(|s| matrix[s].iter().copied().fold(none[s], i64::max)).sum::<i64>() > i64::from(i32::MAX) {
            return Err("power sum may wrap".into());
        }
        matrices.push(json!({"members":layout,"none":none,"slotPower":matrix}));
    }
    if matrices.is_empty() || a.iter().collect::<HashSet<_>>().len() != 5 {
        return Err("no layouts".into());
    }
    fs::write(
        &args[5],
        serde_json::to_vec_pretty(&json!({"format":"ournotes-deck.pt-power-certificate-input/1",
        "members":members,"snaps":snaps,"rewardUpper":reward,"rate":rate,"selectedMembers":selected,
        "incumbentNumerator":numerator.to_string(),"incumbentDenominator":denominator.to_string(),
        "k":request.k,"allowedLeaders":selected.iter().filter(|&&id|request.constraints.leader.is_none_or(|m|m==id)).copied().collect::<Vec<_>>(),
        "requiredMembers":required,"layouts":matrices,"modelBoundary":"fixed member set, per-slot integral nonnegative power with checked nonwrapping sum; normal-played PT only"}))?,
    )?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
