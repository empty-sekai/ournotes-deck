//! The physical deck search: the power and skip search and the exact search of played lives.

use super::*;

/// Low-level exact-model search; request.time_limit is respected in addition to Limits.
/// Played input is COMPLETE declared judgement/clock data; a played live is valued by its mean payoff over the
/// 120 performance orders (`crate::search::uniform`).
#[allow(clippy::too_many_arguments)] // The audit inputs stay separately borrowed; JSON callers use RecommendationRequest.
pub fn solve_physical(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    limits: &Limits,
    strategy: &Strategy,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
) -> Result<RecommendationOutcome, Error> {
    solve_physical_with_aggregation(
        pool,
        request,
        metric,
        event_input,
        limits,
        strategy,
        network,
        simulation,
        Aggregation::Expected,
    )
}

/// [`solve_physical`] with an explicit aggregation of the performance orders of a played live: their mean
/// (`Expected`) or their greatest value (`Maximum`), each order worth the terminal payoff at its expected score over
/// its random draws.
#[allow(clippy::too_many_arguments)]
pub fn solve_physical_with_aggregation(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    limits: &Limits,
    strategy: &Strategy,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
    aggregation: Aggregation,
) -> Result<RecommendationOutcome, Error> {
    let origin = Instant::now();
    solve_physical_impl(
        pool,
        request,
        metric,
        event_input,
        limits,
        strategy,
        network,
        simulation,
        aggregation,
        None,
        &[],
        None,
        origin,
        None,
        None,
    )
}

/// `origin` is the request start; telemetry phases and incumbents are timed from it. `progress` receives reports
/// (`progress.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_physical_impl(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    limits: &Limits,
    strategy: &Strategy,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
    aggregation: Aggregation,
    fixed: Option<PhysicalDeck>,
    initial: &[PhysicalDeck],
    compiled: Option<&crate::handler::ExecutionPlan>,
    origin: Instant,
    progress: Option<ProgressHook<'_>>,
    floor: Option<i128>,
) -> Result<RecommendationOutcome, Error> {
    reject_unsupported_lifecycle(network, simulation.live_finished_from_frame)?;
    let start = Instant::now();
    let mut limits = limits.clone();
    if let Some(d) = request.time_limit {
        let ms = d.as_millis().min(u64::MAX as u128) as u64;
        limits.time_limit_ms = Some(limits.time_limit_ms.map_or(ms, |m| m.min(ms)));
    }
    validate(request.k, &limits, strategy)?;
    let mut normalized;
    let request = if compiled.is_none() {
        normalized =
            SearchRequest { objective: expectation::normalized_objective(&request.objective), ..request.clone() };
        if let Some(confirmations) = network {
            let Objective::InScenario { context, .. } = &mut normalized.objective else {
                return Err(Error::Input("network ranking requires a resolved scenario".into()));
            };
            context.rank_confirmations = Some(confirmations.to_vec());
        }
        &normalized
    } else {
        request
    };
    let owned_plan;
    let plan = match compiled {
        Some(plan) => plan,
        None => {
            owned_plan = crate::handler::compile_execution(
                pool,
                request,
                metric,
                event_input,
                network,
                simulation,
                strategy,
                aggregation,
            )?;
            &owned_plan
        }
    };
    let metric = plan.metric.as_ref().unwrap_or(metric);
    let candidates = plan.domain.members();
    let required = plan.domain.required();
    let snaps = plan.domain.snaps();
    let leader = plan.domain.leader();
    for p in fixed.iter().chain(initial) {
        plan.domain.check_fixed(pool, p)?;
    }
    let feasible = plan.domain.is_feasible();
    let song = &plan.song;
    let event = plan.event;
    let skip = &plan.skip;
    let mut tel = Telemetry::default();
    let env = &mut tel.environment;
    env.k = request.k;
    env.time_limit_ms = limits.time_limit_ms;
    env.max_candidates = limits.max_candidates;
    env.cache_entries = limits.cache_entries;
    let live = matches!(request.objective.inner(), Objective::LiveScore { .. });
    let orders = if live { ORDERS } else { 1 };
    env.target = Some(telemetry::Target {
        orders,
        denominator: if plan.aggregation == Aggregation::Maximum { "1".into() } else { orders.to_string() },
    });
    env.domain = Some(telemetry::Domain {
        members: candidates.len(),
        snaps: snaps.len(),
        required: required.len(),
        leader_fixed: leader.is_some(),
    });
    env.bounds.compiled = plan.joint.is_some() || plan.deck_payoff.is_some() || plan.team_power.is_some();
    env.bounds.fallback = plan.bound_fallback.clone();
    env.bounds.compile_ms = plan.bound_compile_ms;
    if let Some(b) = &plan.joint {
        env.bounds.choices = b.choices.len();
        env.bounds.fine = b.has_fine();
        env.bounds.class_search = b.uses_class_search();
    }
    if let Some(b) = &plan.deck_payoff {
        if plan.joint.is_none() {
            env.bounds.choices = b.members.len();
        }
        env.bounds.deck_payoff = Some(telemetry::DeckPayoffSetup {
            score_cap: b.power_cap(pool, &plan.domain).and_then(|power| b.score_cap(power)).map(|v| v.to_string()),
            ..Default::default()
        });
    } else if let Some(refusal) = &plan.deck_payoff_refusal {
        env.bounds.deck_payoff =
            Some(telemetry::DeckPayoffSetup { refusal: Some(refusal.clone()), ..Default::default() });
    }
    if let Some(b) = &plan.team_power {
        env.bounds.choices = b.members.len();
    }
    let lottery = certified_engine::lottery_mode(pool, request, &plan.domain)?;
    if lottery == certified_engine::LotteryMode::Certified {
        // The ranking value of an order weighs its folded reward by the reward weight, and the rational enclosures
        // of the ranking values sum it over the performance orders.
        let cap = metric.levels().and_then(|levels| levels.first()).map_or(0, |level| level.scale.cap);
        let room = (crate::search::certified_search::REWARD_WEIGHT * ORDERS as i128) << 8;
        if cap > i128::MAX / room {
            return Err(Error::Input(
                "the weights and payoff ranges of the combined metric's levels exceed the exact integer range of a \
                 LUCK live"
                    .into(),
            ));
        }
        if certified_engine::converts_great(pool, &request.objective, &plan.domain)? {
            return Err(Error::Unsupported(
                "plays with Great judgements that a held judgement conversion can change are not supported with LUCK \
                 lotteries"
                    .into(),
            ));
        }
    }
    let mut engine = Engine {
        aggregation: plan.aggregation,
        pool,
        request,
        metric,
        event_input,
        simulation,
        limits: &limits,
        budget: crate::search::budget::SearchBudget::new(start, limits.time_limit_ms.map(Duration::from_millis))?,
        stop: None,
        tel,
        rec: Recorder::new(origin),
        correlated: false,
        correlated_recent: [[0; 2]; 5],
        resource: false,
        bound_scratch: crate::search::snaps::JointScratch::default(),
        bonus_scratch: crate::search::joint::BonusScratch::default(),
        order_steps: Default::default(),
        top: Vec::new(),
        certified: if lottery == certified_engine::LotteryMode::Certified {
            Some(
                CertifiedState::new(
                    request.k,
                    if limits.cache_entries == 0 { 0 } else { certified_engine::LUCK_CURVE_CACHE_BYTES },
                    match plan.aggregation {
                        Aggregation::Expected => crate::search::certified_search::Reduction::Mean,
                        Aggregation::Maximum => crate::search::certified_search::Reduction::Maximum,
                    },
                )
                .map(|mut state| {
                    state.floor = floor;
                    state
                })?,
            )
        } else {
            None
        },
        // A combined metric settles every term on one terminal result, so its lives without a LUCK range play
        // lottery-free too.
        lottery_free: if lottery == certified_engine::LotteryMode::Free
            || (live && lottery == certified_engine::LotteryMode::Absent && matches!(metric, Metric::Combined { .. }))
        {
            Some(std::sync::Arc::new(ournotes_sim::live::full::luck_skills(pool.master)?))
        } else {
            None
        },
        splice_skills: None,
        seen: HashSet::new(),
        fifo: VecDeque::new(),
        team_scores: team_scores::TeamScores::new(limits.cache_entries),
        programs: program_cache::ProgramCache::new(if limits.cache_entries == 0 {
            0
        } else {
            program_cache::DEFAULT_PROGRAM_CACHE_BYTES
        }),
        song: song.as_ref(),
        event,
        skip: skip.as_ref(),
        live,
        orders: uniform::all_orders(),
        positions: uniform::order_positions().into_iter().map(|(p, _)| p).collect(),
        seeded: HashSet::new(),
        root_order: None,
        warm: None,
        progress: progress.map(|hook| progress::Reporter::new(hook.interval, hook.report, strategy, start)),
        offered: None,
    };
    if let Some(p) = fixed {
        engine.tel.environment.traversal = Traversal::Fixed;
        engine.rec.begin(&mut engine.tel, "evaluate", None);
        engine.consider(p)?;
        engine.rec.end(&mut engine.tel);
    } else if feasible {
        if !initial.is_empty() {
            // Supplied incumbents, evaluated like warm-start decks: the traversal treats them as considered.
            engine.rec.begin(&mut engine.tel, "initial", None);
            for p in initial {
                let cut = plan.joint.as_ref().map(|b| (b, &plan.domain));
                if !engine.consider_with(*p, cut)? {
                    break;
                }
                let p = if engine.team_identity() { uniform::canonical(pool, p) } else { *p };
                engine.seeded.insert(p);
            }
            engine.rec.end(&mut engine.tel);
        }
        match strategy {
            // A certified frontier ranks by the best order through its own reduction.
            Strategy::Exhaustive | Strategy::BranchAndBound
                if live && plan.aggregation == Aggregation::Maximum && engine.certified.is_none() =>
            {
                // A payoff that the deck alone determines ranks the same teams under either aggregation. Each
                // retained team still passes through the maximum evaluation, which materializes its best order.
                let mut ranked = if let Some(bounds) = plan.deck_payoff.as_ref().filter(|bounds| !bounds.bounded_only())
                {
                    engine.tel.environment.traversal = Traversal::Joint;
                    engine.rec.begin(&mut engine.tel, "search", Some("deckPayoff".into()));
                    let closed = deck_payoff_search::solve(&plan.domain, bounds, &mut engine, false)?;
                    engine.rec.end(&mut engine.tel);
                    closed
                } else {
                    false
                };
                // A target or cap bounds every order's payoff by one common value: teams ranked by power then
                // settle the search as soon as K of them attain it.
                if !ranked
                    && matches!(strategy, Strategy::BranchAndBound)
                    && matches!(
                        metric,
                        Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. } | Metric::CappedScore { .. }
                    )
                    && !engine.expired()
                {
                    engine.rec.begin(&mut engine.tel, "setup", Some("maximumPowerCap".into()));
                    let bounds = crate::search::deck_payoff::DeckPayoffBounds::compile_upper_only(
                        pool,
                        request,
                        &plan.domain,
                        metric,
                    );
                    engine.rec.end(&mut engine.tel);
                    if let Ok(bounds) = bounds {
                        engine.tel.environment.traversal = Traversal::Joint;
                        engine.rec.begin(&mut engine.tel, "search", Some("maximumPowerCap".into()));
                        ranked = deck_payoff_search::solve_upper_only(&plan.domain, &bounds, &mut engine)?;
                        engine.rec.end(&mut engine.tel);
                    }
                }
                if !ranked {
                    engine.rec.frontier.clear();
                    engine.tel.environment.traversal =
                        if plan.joint.is_some() { Traversal::Joint } else { Traversal::Exhaustive };
                    engine.rec.begin(&mut engine.tel, "search", None);
                    maximum::search(&mut engine, &plan.domain, plan.joint.as_ref())?;
                    engine.rec.end(&mut engine.tel);
                }
            }
            Strategy::Exhaustive | Strategy::BranchAndBound => {
                let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
                // The deck payoff ranking either settles the Top-K or hands the search to the joint traversal.
                let mut joint_next = true;
                if let Some(bounds) = &plan.team_power {
                    engine.tel.environment.traversal = Traversal::Joint;
                    engine.rec.begin(&mut engine.tel, "search", Some("teamPower".into()));
                    team_power_search::solve(&plan.domain, bounds, &mut engine)?;
                    engine.rec.end(&mut engine.tel);
                    joint_next = false;
                } else if let Some(bounds) = plan.deck_payoff.as_ref().filter(|_| {
                    plan.joint.is_none() || !crate::search::snaps::ablated(crate::search::ablate::NO_DECK_PAYOFF)
                }) {
                    engine.tel.environment.traversal = Traversal::Joint;
                    engine.rec.begin(&mut engine.tel, "search", Some("deckPayoff".into()));
                    joint_next = !deck_payoff_search::solve(&plan.domain, bounds, &mut engine, plan.joint.is_some())?;
                    engine.rec.end(&mut engine.tel);
                    if joint_next {
                        engine.rec.frontier.clear();
                    }
                }
                if joint_next && let Some(bounds) = &plan.joint {
                    engine.rec.begin(&mut engine.tel, "setup", None);
                    // Node bounds read position-mean gains: one pseudo-order carrying the mass of all of them.
                    let orders = uniform::MEAN_ORDERS.to_vec();
                    engine.correlated = bounds.correlation_worthwhile(pool, &plan.domain, &orders)?;
                    engine.resource = !bounds.prefers_compositions()
                        && bounds.resource_worthwhile(pool, &plan.domain, &orders, engine.correlated)?;
                    engine.warm = warm::Warm::new(&plan.domain, bounds, &orders);
                    engine.rec.end(&mut engine.tel);
                    if bounds.is_pt() && !bounds.prefers_compositions() {
                        engine.tel.environment.traversal = Traversal::Joint;
                        // The maximum-bonus warmup fills the Top-K; one already full (from the deck payoff ranking)
                        // leaves it nothing to do, and its pruned walk to a first leaf would repeat the search's.
                        if engine.safe_cutoff().is_none() {
                            engine.rec.begin(&mut engine.tel, "ptWarmStart", None);
                            let seed_bonus = bounds.bonus_upper(pool, &plan.domain, &physical, 0);
                            joint_rec(0, 0, &mut physical, &plan.domain, bounds, &orders, &mut engine, seed_bonus)?;
                            engine.rec.end(&mut engine.tel);
                        }
                        // After the maximum-bonus warmup, whose full Top-K ends it.
                        warm::seed(&mut engine)?;
                        // A certified search drops the point tiers that no performance order reaches.
                        let tier_cap = if engine.certified.is_some() && !engine.expired() {
                            engine.rec.begin(&mut engine.tel, "tierProbe", None);
                            let cap = tier_cap(&mut engine, bounds, network, simulation)?;
                            engine.rec.end(&mut engine.tel);
                            if let Some(state) = engine.certified.as_mut() {
                                state.tier_cap = cap;
                            }
                            cap
                        } else {
                            None
                        };
                        let mut searched = false;
                        if !engine.expired() {
                            engine.rec.begin(&mut engine.tel, "ptRegimeCompile", None);
                            let restricted = if let Some((numerator, _)) = engine.safe_cutoff() {
                                bounds.qualifying_pt_domain(pool, &plan.domain, numerator, ORDERS as u128, tier_cap)?
                            } else {
                                None
                            };
                            let mut regime = telemetry::PtRegime::default();
                            // A certified LUCK live searches the bounds that weigh the lottery laws of its domain,
                            // cluster by cluster of those laws, and its leaves read the laws of their own teams from
                            // the census.
                            let weighed = if engine.certified.is_some() {
                                let domain = restricted.as_ref().unwrap_or(&plan.domain);
                                let mut census = engine.lottery_census(domain);
                                let weighed =
                                    census.as_mut().and_then(|census| engine.weighed_bounds(census, domain, true));
                                if let Some(state) = engine.certified.as_mut() {
                                    state.census = census;
                                }
                                weighed
                            } else {
                                None
                            };
                            let mut compiled = if weighed.is_some() {
                                weighed
                            } else if let Some(domain) = &restricted {
                                let prepare = Instant::now();
                                let result = crate::search::joint::JointBounds::compile(
                                    pool,
                                    request,
                                    domain,
                                    metric,
                                    event_input,
                                    simulation,
                                );
                                regime.compile_ms = prepare.elapsed().as_secs_f64() * 1000.0;
                                match result {
                                    Ok(b) => Some(b),
                                    Err(error) => {
                                        regime.fallback = Some(error.to_string());
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            if let Some(threshold) = tier_cap {
                                let domain = restricted.as_ref().unwrap_or(&plan.domain);
                                if compiled.is_none() {
                                    compiled = crate::search::joint::JointBounds::compile(
                                        pool,
                                        request,
                                        domain,
                                        metric,
                                        event_input,
                                        simulation,
                                    )
                                    .ok();
                                }
                                if let Some(compiled) = &mut compiled {
                                    compiled.cap_scores(pool, domain, threshold, &orders);
                                }
                            }
                            engine.rec.end(&mut engine.tel);
                            engine.rec.begin(&mut engine.tel, "search", None);
                            engine.rec.frontier.clear();
                            (engine.rec.tracked, engine.rec.bounded, engine.rec.unexplored) = (true, true, None);
                            searched = true;
                            if let Some(refined) = &compiled {
                                let domain = restricted.as_ref().unwrap_or(&plan.domain);
                                regime.members_removed = plan.domain.members().len() - domain.members().len();
                                engine.correlated = refined.correlation_worthwhile(pool, domain, &orders)?;
                                engine.tel.environment.bounds.pt_regime = Some(regime);
                                traverse(&mut physical, domain, refined, &orders, &mut engine)?;
                            } else {
                                engine.tel.environment.bounds.pt_regime = Some(regime);
                                ordered_root(&mut physical, &plan.domain, bounds, &orders, &mut engine)?;
                            }
                            engine.rec.end(&mut engine.tel);
                        }
                        if engine.stop.is_some() && !searched {
                            // The warm start's bonus filter skipped prefixes: the whole domain remains to be proven.
                            engine.rec.frontier.clear();
                            (engine.rec.tracked, engine.rec.bounded) = (true, true);
                            let started = engine.bound_start();
                            let upper = joint_root_upper(pool, &plan.domain, bounds, &orders, 0)?;
                            engine.fold_unexplored(upper, started);
                        }
                    } else if bounds.prefers_compositions() {
                        engine.tel.environment.traversal = Traversal::Composition;
                        warm::seed(&mut engine)?;
                        engine.rec.begin(&mut engine.tel, "search", None);
                        (engine.rec.tracked, engine.rec.bounded) = (true, true);
                        composition::solve(&plan.domain, bounds, &orders, &mut engine)?;
                        engine.rec.end(&mut engine.tel);
                    } else {
                        engine.tel.environment.traversal = Traversal::Joint;
                        warm::seed(&mut engine)?;
                        joint_regimes(&mut physical, plan, bounds, &orders, &mut engine)?;
                    }
                    engine.tel.environment.bounds.correlated = engine.correlated;
                    engine.tel.environment.bounds.resource = engine.resource;
                } else if joint_next {
                    engine.tel.environment.traversal = Traversal::Exhaustive;
                    engine.rec.begin(&mut engine.tel, "search", None);
                    engine.rec.tracked = true;
                    members_rec(0, 0, &mut physical, candidates, required, leader, snaps, &mut engine)?;
                    engine.rec.end(&mut engine.tel);
                }
            }
            Strategy::Candidate { power_seeds, proposals, proposal_seed } => {
                engine.tel.environment.traversal = Traversal::Candidate;
                engine.rec.begin(&mut engine.tel, "warmStart", None);
                if *power_seeds > 0 && !engine.expired() {
                    let power = Objective::Power { music_id: None, event };
                    let power = match request.objective.context() {
                        Some(c) => power.in_scenario(c.clone()),
                        None => match &engine.song {
                            Some(s) => Objective::Power { music_id: Some(s.id), event },
                            None => power,
                        },
                    };
                    let duration = limits.time_limit_ms.map(|m| Duration::from_millis((m / 4).min(1000)));
                    let seeds = crate::search::search(
                        pool,
                        &SearchRequest {
                            objective: power,
                            k: *power_seeds,
                            constraints: request.constraints.clone(),
                            time_limit: duration,
                        },
                    )?;
                    let seeds = seeds
                        .results
                        .into_iter()
                        .map(|seed| pool.deck(seed.members, seed.snaps, [0, 1, 2, 3, 4]))
                        .collect::<Result<Vec<_>, _>>()?;
                    // Cover every power seed BEFORE refining any one's physical layout.
                    // The former 120 permutations of seed 0 could consume the entire
                    // browser budget before another member set or snap layout was tried.
                    // Five rounds cap warmup at 5 * powerSeeds; the remaining budget
                    // explores skills, pairings and member substitutions below.
                    'seeds: for round in 0..5 {
                        for d in &seeds {
                            let mut p = PhysicalDeck { members: d.members, snaps: d.snaps };
                            match round {
                                0 => {}
                                1 => p.snaps = [None; 5],
                                _ => {
                                    let nonleaders = [0, 1, 3, 4];
                                    for (at, &slot) in nonleaders.iter().enumerate() {
                                        let from = nonleaders[(at + round - 1) % 4];
                                        p.members[slot] = d.members[from];
                                        p.snaps[slot] = d.snaps[from];
                                    }
                                }
                            }
                            engine.tel.candidate.warmup_proposals += 1;
                            if !engine.consider(p)? {
                                break 'seeds;
                            }
                            if round == 0 {
                                engine.tel.candidate.warmup_member_sets += 1;
                            }
                        }
                    }
                }
                engine.rec.end(&mut engine.tel);
                engine.rec.begin(&mut engine.tel, "proposals", None);
                let mut rng = ProposalRandom(*proposal_seed | 1);
                for n in 0..*proposals {
                    if engine.expired() {
                        break;
                    }
                    engine.tel.candidate.exploration_proposals += 1;
                    let mut p = if n % 3 != 0 && !engine.top.is_empty() {
                        let mut p = engine.top[rng.index(engine.top.len())].physical;
                        if rng.index(2) == 0 {
                            let slot = rng.index(5);
                            let m = candidates[rng.index(candidates.len())];
                            if (slot != 2 || leader.is_none()) && !required.contains(&p.members[slot]) {
                                p.members[slot] = m;
                            }
                        } else {
                            let slot = rng.index(5);
                            let s = rng.index(snaps.len() + 1);
                            p.snaps[slot] = if s == snaps.len() { None } else { Some(snaps[s]) };
                        }
                        p
                    } else {
                        random_deck(pool, candidates, required, leader, snaps, &mut rng)
                    };
                    // Additional physical swaps preserve member/snap pairs and fixed leader.
                    if n % 4 == 0 {
                        let a = rng.index(5);
                        let b = rng.index(5);
                        if leader.is_none() || (a != 2 && b != 2) {
                            p.members.swap(a, b);
                            p.snaps.swap(a, b);
                        }
                    }
                    if pool.check_deck(&p.as_deck()).is_err() {
                        continue;
                    }
                    if !engine.consider(p)? {
                        break;
                    }
                }
                engine.rec.end(&mut engine.tel);
                if engine.stop.is_none() {
                    engine.stop = Some(ExitReason::ProposalLimit)
                }
            }
        }
    }
    engine.tel.incumbents.warm_start.final_top_k = engine.seeded_in_top();
    // A search for a floor only asks whether a team reaches it: the ranking below it needs no refinement.
    if engine.certified.as_ref().is_some_and(|state| state.floor.is_none()) && engine.stop.is_none() {
        engine.refine_certified_frontier()?;
    }
    let standing = engine.standing();
    engine.rec.begin(&mut engine.tel, "finish", None);
    let results = if engine.certified.is_some() {
        let (results, proof) = engine.certified_results(engine.stop.is_none())?;
        if engine.stop.is_none() && !proof.complete {
            engine.stop = Some(ExitReason::RefinementRequired);
        }
        results
    } else {
        std::mem::take(&mut engine.top)
            .into_iter()
            .map(|e| e.wire(metric, plan.aggregation, live))
            .collect::<Result<Vec<_>, _>>()?
    };
    let exit_reason = engine.stop.unwrap_or(ExitReason::Exhausted);
    engine.rec.end(&mut engine.tel);
    engine.finish_telemetry(standing);
    let telemetry = std::mem::take(&mut engine.tel);
    Ok(engine.outcome(strategy, exit_reason, fixed.is_some(), results, telemetry, start.elapsed()))
}

/// The answer of [`orders_below`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Probe {
    /// No performance order of any team reaches the threshold.
    Below,
    /// A team's order reaches it.
    Reached,
    /// The search did not finish.
    Open,
}

/// Whether a performance order of a team of the request's domain has an expected score of `threshold` or more: a
/// certified search of the teams' best orders by expected score that closes every node whose bound lies below
/// `threshold` and stops at the first team whose best order reaches it, within `time_ms`. The domain is exhausted
/// when the search ends without a stop, ranked or not; then every evaluated team still on its frontier lies below
/// the threshold, and every other one lies below one of them.
#[allow(clippy::too_many_arguments)]
fn orders_below(
    pool: &Pool,
    request: &SearchRequest,
    limits: &Limits,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
    threshold: i64,
    time_ms: Option<u64>,
) -> Result<Probe, Error> {
    let probe = SearchRequest { k: 1, time_limit: None, ..request.clone() };
    let limits = Limits { time_limit_ms: time_ms, ..limits.clone() };
    let outcome = match solve_physical_impl(
        pool,
        &probe,
        &Metric::Score,
        None,
        &limits,
        &Strategy::BranchAndBound,
        network,
        simulation,
        Aggregation::Maximum,
        None,
        &[],
        None,
        Instant::now(),
        None,
        Some(i128::from(threshold)),
    ) {
        Ok(outcome) => outcome,
        Err(Error::Unsupported(_) | Error::Capacity(_)) => return Ok(Probe::Open),
        Err(error) => return Err(error),
    };
    let reaches = |deck: &RecommendedDeck| {
        deck.payoff_interval.as_ref().is_some_and(|payoff| payoff.lower_f64() >= threshold as f64)
    };
    let below = |deck: &RecommendedDeck| {
        deck.payoff_interval.as_ref().is_some_and(|payoff| payoff.upper_f64() < threshold as f64)
    };
    Ok(if outcome.results.iter().any(reaches) {
        Probe::Reached
    } else if matches!(outcome.exit_reason, ExitReason::Exhausted | ExitReason::RefinementRequired)
        && outcome.results.iter().all(below)
    {
        Probe::Below
    } else {
        Probe::Open
    })
}

/// The lowest score threshold of a point tier of `bounds` that no performance order of a team of the domain reaches
/// in expected score ([`orders_below`]), asked for the thresholds above the greatest order score evaluated so far in
/// ascending order until one is not reached; each search takes at most a quarter of the time left. None when no
/// threshold is proved unreached.
fn tier_cap(
    engine: &mut Engine<'_, '_>,
    bounds: &crate::search::joint::JointBounds,
    network: Option<&[RankConfirmation]>,
    simulation: &SimulationInput,
) -> Result<Option<i64>, Error> {
    let seen = engine.certified.as_ref().expect("certified request").order_score_seen.max(0.0);
    for threshold in bounds.tier_scores().into_iter().filter(|&score| score as f64 > seen) {
        if engine.expired() {
            break;
        }
        let started = Instant::now();
        let time_ms = engine.budget.deadline().map(|deadline| {
            (deadline.saturating_duration_since(started).as_millis() / 4).min(u128::from(u64::MAX)) as u64
        });
        let probe = orders_below(engine.pool, engine.request, engine.limits, network, simulation, threshold, time_ms)?;
        engine.tel.environment.bounds.tier_probes.push(telemetry::TierProbe {
            threshold,
            outcome: match probe {
                Probe::Below => "below",
                Probe::Reached => "reached",
                Probe::Open => "open",
            },
            ms: started.elapsed().as_secs_f64() * 1000.0,
        });
        match probe {
            Probe::Below => return Ok(Some(threshold)),
            Probe::Reached => {}
            Probe::Open => return Ok(None),
        }
    }
    Ok(None)
}
