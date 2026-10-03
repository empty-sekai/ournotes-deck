//! Opt-in bound auditing and harness ablations, separate from production policy.
use super::expectation::PhysicalDeck;
mod cutoff;
mod luck;
pub use cutoff::audit_cutoff;
pub use luck::{audit_rush_replay, prefix_profile};

/// Harness-only schedules; never part of the player recommendation request.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    #[default]
    Production,
    Classes,
    ClassesWithResource,
}

/// Construct and execute an ablation with the same deadline and evaluator contract.
pub fn recommend_experiment(
    data: &ournotes_sim::data::DeckData,
    roster: &ournotes_sim::cards::Roster,
    request: &crate::types::RecommendationRequest,
    schedule: Schedule,
) -> Result<crate::types::RecommendationOutcome, Error> {
    let start = crate::clock::Instant::now();
    let mut built = crate::handler::build_card_pool(data, roster, request)?;
    configure_schedule(&mut built, schedule)?;
    super::dispatch::execute(&built, None, start, start.elapsed().as_secs_f64() * 1000.0, None)
}

fn configure_schedule(built: &mut BuiltProblem<'_>, schedule: Schedule) -> Result<(), Error> {
    if matches!(schedule, Schedule::Production) {
        return Ok(());
    }
    let plan = &mut built.context.plan;
    let joint = plan.joint.as_mut().ok_or_else(|| Error::Domain("class schedule requires joint bounds".into()))?;
    let start = crate::clock::Instant::now();
    joint.enable_class_search(&built.pool, &plan.domain, matches!(schedule, Schedule::ClassesWithResource))?;
    plan.bound_compile_ms += start.elapsed().as_secs_f64() * 1000.0;
    Ok(())
}

/// Enable the strongest class cap where applicable for independent oracle audits.
pub fn prepare_class_audit(built: &mut BuiltProblem<'_>) -> Result<bool, Error> {
    if !built.context.plan.joint.as_ref().is_some_and(|b| b.has_class_bounds()) {
        return Ok(false);
    }
    configure_schedule(built, Schedule::ClassesWithResource)?;
    Ok(true)
}

/// Explain the complete-deck relaxation without running or changing the scorer.
pub fn describe_bound(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    root: i32,
) -> Result<Option<serde_json::Value>, Error> {
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let positions = super::joint::positions(root)?;
    let Some(b) = built.context.plan.joint.as_ref() else { return Ok(None) };
    let mut oracle = if b.rush_eligible() {
        super::luck::LuckOracle::new(
            &built.pool,
            &built.context.request,
            built.domain(),
            &built.context.law,
            b.luck_life(),
        )?
    } else {
        None
    };
    let masks = match &mut oracle {
        Some(oracle) => oracle.masks(&built.pool, &p, &positions)?,
        None => None,
    };
    Ok(Some(b.describe(&built.pool, built.domain(), &p, &positions, masks.as_ref())))
}
use crate::handler::BuiltProblem;
use ournotes_sim::Error;

/// Pair the fine cap's per-entry terms with one actual simulation of the same deck and root. Each note's last
/// executed score and the filed fixed scores come from the scorer; the bound terms are not themselves a bound.
pub fn slack_profile(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
) -> Result<Vec<serde_json::Value>, Error> {
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let Some(b) = built.context.plan.joint.as_ref() else { return Ok(Vec::new()) };
    let input = super::expectation::context(&built.pool, &p, &built.context.request.objective)?;
    let roots: std::collections::BTreeSet<_> = built.context.law.atoms().iter().map(|&(root, _)| root).collect();
    let life = built.context.plan.joint.as_ref().and_then(|b| b.luck_life());
    let mut luck =
        super::luck::LuckOracle::new(&built.pool, &built.context.request, built.domain(), &built.context.law, life)?;
    let plan = &built.context.plan;
    // The search's leaf fine bound reads the relaxed power; its cutoff tables read the exact one.
    let power = i64::from(built.pool.deck_power(&p.as_deck(), plan.song.as_ref(), plan.event)?.power());
    let mut scratch = super::snaps::JointScratch::default();
    let mut out = Vec::new();
    for root in roots {
        let positions = super::joint::positions(root)?;
        let (_, relaxed) = b.upper(&built.pool, built.domain(), &p, 5, &positions);
        let masks = if b.rush_eligible() {
            match &mut luck {
                Some(oracle) => oracle.masks(&built.pool, &p, &positions)?,
                None => None,
            }
        } else {
            None
        };
        let Some((fine, terms)) = b.fine_trace(built.domain(), &p, power, &positions, masks.as_ref()) else { continue };
        let leaf = b.fine_upper(built.domain(), &p, relaxed, &positions, &mut scratch, masks.as_ref());
        let outcome = input.simulate(built.pool.master, root)?;
        let (notes, fixed) = outcome.model.filed_scores();
        let bound_cb = b.fine_cb_windows(built.domain(), &p, &positions);
        let actual_cb = outcome.model.gk_combo_bonus_commands();
        out.push(serde_json::json!({"root":root,"positions":positions,"powerUpper":relaxed,"power":power,
            "boundComboWindows":bound_cb,"actualComboBonus":actual_cb,"leafFineUpper":leaf.map(|v| v.to_string()),
            "actualPower":input.params.total_power,"actualScore":outcome.final_score,"fineUpper":fine,
            "boundTerms":terms,"actualNotes":notes,"actualFixed":fixed,"rushRefined":masks.is_some()}));
    }
    Ok(out)
}

/// Compare one exact recorded program with fresh complete simulations at other
/// initial powers. Only power varies: this does not merge physical decks, prove
/// monotonicity, settle PT, or authorize any pruning.
pub fn audit_score_program(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    powers: &[i32],
) -> Result<serde_json::Value, Error> {
    use super::expectation::{context, native_member_order};
    use crate::clock::Instant;
    use ournotes_sim::live::full::LiveModel;
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let physical = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &physical)?;
    let mut input = context(&built.pool, &physical, &built.context.request.objective)?;
    let spec = &built.context.spec;
    if spec.network_confirmations.is_some() || spec.simulation.live_finished_from_frame.is_some() {
        return Err(Error::Unsupported("score programs require the declared solo lifecycle".into()));
    }
    if let Some(v) = spec.simulation.music_length_ms {
        input.params.music_length_ms = v;
    }
    if let Some(v) = spec.simulation.score_music_length_ms {
        input.params.score_music_length_ms = Some(v);
    }
    let roots: std::collections::BTreeSet<_> = built.context.law.atoms().iter().map(|&(root, _)| root).collect();
    let mut results = Vec::new();
    for root in roots {
        let (order, random) = native_member_order(root)?;
        let performers = order.map(|slot| input.performers[slot].clone());
        let model = |power| {
            let params = ournotes_sim::live::full::LiveParams { total_power: power, ..input.params };
            match &input.gekisou {
                Some(g) => {
                    LiveModel::new_gekisou(built.pool.master, &performers, &input.notes, &input.events, params, g)
                }
                None => LiveModel::new(built.pool.master, &performers, &input.notes, &input.events, params),
            }
        };
        let start = Instant::now();
        let (program, origin) =
            model(input.params.total_power)?.compile_score_program(&input.play, &input.delta_times, random.clone())?;
        let compile_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let certified = program.certify_nondecreasing(0, 2_000_000);
        let certificate_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut samples = Vec::new();
        for &power in powers {
            let start = Instant::now();
            let score = program.evaluate(power);
            let program_ms = start.elapsed().as_secs_f64() * 1000.0;
            let start = Instant::now();
            let mut fresh = model(power)?;
            let expected = fresh.run_with_random(&input.play, &input.delta_times, random.clone())?;
            let native_ms = start.elapsed().as_secs_f64() * 1000.0;
            if score != expected
                || origin.current_life() != fresh.current_life()
                || origin.converted_judgements() != fresh.converted_judgements()
            {
                return Err(Error::Game(format!("score program differs at root {root}, power {power}")));
            }
            if (0..=2_000_000).contains(&power)
                && certified.is_some_and(|(lo, hi)| {
                    score < lo || score > hi || power == 0 && score != lo || power == 2_000_000 && score != hi
                })
            {
                return Err(Error::Game("score program certificate disagrees with evaluation".into()));
            }
            samples.push(
                serde_json::json!({"power":power,"score":score,"programMs":program_ms,"freshSimulationMs":native_ms}),
            );
        }
        results.push(serde_json::json!({"root":root,"nativeOrder":order,"nodes":program.node_count(),
            "compileMs":compile_ms,"originPower":program.origin_power(),"originScore":program.origin_score(),"samples":samples,
            "certificatePowerRange":[0,2_000_000],"certifiedScoreRange":certified,"certificateMs":certificate_ms}));
    }
    Ok(
        serde_json::json!({"scope":"Same complete performer programs and declared clocks/root; only total power varies. Exact score replay; optional checked monotonicity only within certificatePowerRange, no cross-deck or PT certificate.",
        "members":members,"snaps":snaps,"allComparedValuesEqual":true,"roots":results}),
    )
}

/// One compiled problem only; recreate when changing BuiltProblem or its domain.
#[derive(Default)]
pub struct PrefixAuditScratch {
    bonus: super::joint::BonusScratch,
    classes: super::snaps::JointScratch,
    class_caps: std::collections::HashMap<ClassAuditKey, (i128, i64)>,
    pub class_bound_computations: u64,
    pub class_bound_cache_hits: u64,
    pub checked_bonus_prefixes: u64,
    pub checked_resource_prefixes: u64,
    pub checked_character_prefixes: u64,
    pub checked_raw_leaves: u64,
    luck: Option<super::luck::LuckOracle>,
    luck_initialized: bool,
    rush_fine: super::snaps::JointScratch,
    pub checked_rush_leaves: u64,
    rush_prefix_caps: std::collections::HashMap<(PhysicalDeck, [usize; 5]), Option<super::joint::RushCaps>>,
    pub checked_rush_prefixes: u64,
}
type ClassAuditKey = ([i64; 5], [Vec<usize>; 5], [usize; 5], bool);

/// A class-prefix relaxation or a physical Snap prefix within the complete class vector.
#[allow(clippy::too_many_arguments)]
pub fn class_prefix_upper(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    depth: usize,
    root: i32,
    bindings: bool,
    scratch: &mut PrefixAuditScratch,
) -> Result<Option<(i128, i64)>, Error> {
    if depth > 5 {
        return Err(Error::Input("class depth must be 0..=5".into()));
    }
    let Some(b) = built.context.plan.joint.as_ref().filter(|b| b.has_class_bounds()) else {
        return Ok(None);
    };
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let all: Vec<_> = (0..=built.domain().snaps().len()).collect();
    let mut allowed = std::array::from_fn(|_| all.clone());
    for (at, &slot) in super::joint::SLOTS.iter().enumerate() {
        let choice = p.snaps[slot].map_or(0, |s| built.domain().snaps().iter().position(|&v| v == s).unwrap() + 1);
        if bindings && at < depth {
            allowed[slot] = vec![choice];
        } else if bindings || at < depth {
            allowed[slot] =
                b.effect_groups(p.members[slot], &all).into_iter().find(|g| g.contains(&choice)).expect("actual class");
        }
    }
    let positions = super::joint::positions(root)?;
    let key = (members, allowed.clone(), positions, bindings || depth == 5);
    if let Some(&value) = scratch.class_caps.get(&key) {
        scratch.class_bound_cache_hits += 1;
        return Ok(Some(value));
    }
    let orders = [(positions, 1)];
    let value = b
        .class_bound(built.domain(), &p, &allowed, &orders, bindings || depth == 5, &mut scratch.classes)?
        .ok_or_else(|| Error::Domain("actual legal completion lost by class matching".into()))?;
    scratch.class_bound_computations += 1;
    let value = (value.payoff, value.power);
    if scratch.class_caps.len() < 100_000 {
        scratch.class_caps.insert(key, value);
    }
    Ok(Some(value))
}

/// Audit either a leader/nonleader composition (depth members, all nonleader
/// positions and Snaps free), or a fixed physical layout (depth Snaps assigned).
pub fn split_prefix_upper(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    depth: usize,
    root: i32,
    layout: bool,
) -> Result<Option<(i128, i64)>, Error> {
    if depth > 5 || (!layout && depth == 0) {
        return Err(Error::Input("invalid split audit depth".into()));
    }
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let positions = super::joint::positions(root)?;
    Ok(built.context.plan.joint.as_ref().map(|b| {
        let member_depth = if layout { 5 } else { depth };
        let snap_depth = if layout { depth } else { 0 };
        let power_cap = (member_depth == 5).then(|| b.layout_power(built.domain(), &p, snap_depth).0);
        b.composition_upper_at(
            &built.pool,
            built.domain(),
            &p,
            member_depth,
            snap_depth,
            !layout,
            &positions,
            power_cap,
        )
    }))
}

/// A bonus-conditioned cap on the complete finite-law payoff numerator.
pub fn bonus_expected_prefix_upper(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    depth: usize,
    scratch: &mut PrefixAuditScratch,
) -> Result<Option<i128>, Error> {
    if !(1..5).contains(&depth) {
        return Err(Error::Input("bonus audit depth must be 1..=4".into()));
    }
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let Some(bounds) = &built.context.plan.joint else {
        return Ok(None);
    };
    let orders = super::joint::order_law(&built.context.law)?;
    bounds.bonus_expected_upper(&built.pool, built.domain(), &p, depth, &orders, &mut scratch.bonus)
}

/// Bound the leader-first prefix of a complete legal deck. The caller independently
/// enumerates completions and checks every realized score/PT against this per-atom cap.
pub fn prefix_upper(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    depth: usize,
    root: i32,
    scratch: &mut PrefixAuditScratch,
) -> Result<Option<(i128, i64)>, Error> {
    if !(1..=5).contains(&depth) {
        return Err(Error::Input("bound audit depth must be 1..=5".into()));
    }
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let positions = super::joint::positions(root)?;
    if let Some(b) = built.context.plan.joint.as_ref() {
        b.check_relax_tables(&built.pool, built.domain(), &p, depth, &super::joint::SLOTS[depth..], &positions)?;
        if depth < 4 {
            b.check_relax_tables(
                &built.pool,
                built.domain(),
                &p,
                depth,
                &super::joint::SLOTS[depth + 1..],
                &positions,
            )?;
        }
    }
    if depth >= 4 && built.context.plan.joint.as_ref().is_some_and(|b| b.rush_eligible()) && !scratch.luck_initialized {
        let life = built.context.plan.joint.as_ref().and_then(|b| b.luck_life());
        scratch.luck = super::luck::LuckOracle::new(
            &built.pool,
            &built.context.request,
            built.domain(),
            &built.context.law,
            life,
        )?;
        scratch.luck_initialized = true;
    }
    let masks = if depth == 5 && built.context.plan.joint.as_ref().is_some_and(|b| b.rush_eligible()) {
        match &mut scratch.luck {
            Some(oracle) => oracle.masks(&built.pool, &p, &positions)?,
            None => None,
        }
    } else {
        None
    };
    scratch.checked_rush_leaves += u64::from(masks.is_some());
    let rush_prefix = if depth == 4 && built.context.plan.joint.as_ref().is_some_and(|b| b.rush_eligible()) {
        let mut prefix = p;
        prefix.members[4] = usize::MAX;
        prefix.snaps[4] = None;
        let key = (prefix, positions);
        if !scratch.rush_prefix_caps.contains_key(&key) {
            let caps = match (built.context.plan.joint.as_ref(), scratch.luck.as_mut()) {
                (Some(b), Some(oracle)) => b.rush_prefix_caps(
                    &built.pool,
                    built.domain(),
                    &p,
                    &[(positions, 1)],
                    oracle,
                    super::budget::SearchBudget::new(crate::clock::Instant::now(), None)?,
                )?,
                _ => None,
            };
            scratch.rush_prefix_caps.insert(key, caps);
        }
        let variant = scratch.luck.as_ref().and_then(|o| o.variant(p.members[4], p.snaps[4]));
        variant.and_then(|v| scratch.rush_prefix_caps.get(&key).and_then(|c| c.as_ref())?.variants[v as usize])
    } else {
        None
    };
    scratch.checked_rush_prefixes += u64::from(rush_prefix.is_some());
    Ok(built.context.plan.joint.as_ref().map(|b| {
        let (mut cap, power) = b.upper(&built.pool, built.domain(), &p, depth, &positions);
        if let Some((rush, _)) = rush_prefix {
            cap = cap.min(rush);
        }
        if let Some(bonus) = b.bonus_upper_at(&built.pool, built.domain(), &p, depth, &positions, &mut scratch.bonus) {
            cap = cap.min(bonus);
            scratch.checked_bonus_prefixes += 1;
        }
        if depth < 5 {
            if let Some(character) = b.character_prefix_upper(&built.pool, built.domain(), &p, depth, &positions) {
                cap = cap.min(character);
                scratch.checked_character_prefixes += 1;
            } else {
                cap = cap.min(b.correlated_upper(&built.pool, built.domain(), &p, depth, &positions));
            }
            if let Some(resource) = b.resource_prefix_upper(&built.pool, built.domain(), &p, depth, &positions) {
                cap = cap.min(resource);
                scratch.checked_resource_prefixes += 1;
            }
        }
        if depth == 5
            && let Some(raw) = b.raw_upper(built.domain(), &p, power, &positions)
        {
            cap = cap.min(raw);
            scratch.checked_raw_leaves += 1;
        }
        if depth == 5
            && let Some(fine) =
                b.fine_upper(built.domain(), &p, power, &positions, &mut scratch.rush_fine, masks.as_ref())
        {
            cap = cap.min(fine);
        }
        let power = b
            .assignment_power_upper(&built.pool, built.domain(), &p, depth)
            .map_or(power, |matching| power.min(matching));
        (cap, power)
    }))
}

/// Audit the smallest candidate suffix containing this completion's next pair.
/// Unlike prefix_upper, this cap certifies a suffix-restricted completion set.
pub struct ChoiceBounds {
    pub suffix: (i128, i64),
    pub pair: (i128, i64),
}

pub fn next_choice_bounds(
    built: &BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    depth: usize,
    root: i32,
) -> Result<Option<ChoiceBounds>, Error> {
    if !(1..5).contains(&depth) {
        return Err(Error::Input("suffix audit depth must be 1..=4".into()));
    }
    let d = built.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
    let p = PhysicalDeck { members: d.members, snaps: d.snaps };
    built.domain().check_fixed(&built.pool, &p)?;
    let Some(bounds) = &built.context.plan.joint else {
        return Ok(None);
    };
    let positions = super::joint::positions(root)?;
    let Some(state) = bounds.tail_state(&built.pool, built.domain(), &p, depth, &[(positions, 1)]) else {
        return Ok(None);
    };
    let slot = super::joint::SLOTS[depth];
    let choice =
        p.snaps[slot].map_or(0, |s| built.domain().snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
    let offset = bounds.choices.iter().position(|&x| x == (p.members[slot], choice)).expect("compiled pair");
    Ok(Some(ChoiceBounds {
        suffix: bounds.tail_upper(&state, offset)?,
        pair: bounds.pair_upper(&state, p.members[slot], choice)?,
    }))
}

/// Original-domain per-member PT caps for independent membership-filter auditing.
pub fn member_pt_caps(built: &BuiltProblem<'_>) -> Option<Vec<(i64, i128)>> {
    let bounds = built.context.plan.joint.as_ref()?;
    Some(
        bounds
            .member_pt_caps(&built.pool, built.domain())?
            .into_iter()
            .map(|(m, cap)| (built.pool.members[m].id, cap))
            .collect(),
    )
}
