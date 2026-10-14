//! The answer of a search outcome: exact bounds, teams, their orders and the status.

use super::*;

pub(super) fn exact(f: &Fraction) -> Result<(i128, u128), Error> {
    let bad = || Error::Domain("result fraction".into());
    let numerator = f.numerator.parse().map_err(|_| bad())?;
    let denominator = f.denominator.parse().map_err(|_| bad())?;
    if denominator == 0 {
        return Err(bad());
    }
    Ok((numerator, denominator))
}

/// Exact rounding for a positive denominator, including denominators above i128::MAX.
pub(super) fn floor_div(n: i128, d: u128) -> i128 {
    match i128::try_from(d) {
        Ok(d) => n.div_euclid(d),
        Err(_) => {
            if n < 0 {
                -1
            } else {
                0
            }
        }
    }
}

pub(super) fn ceil_div(n: i128, d: u128) -> i128 {
    floor_div(n, d) + i128::from(!n.unsigned_abs().is_multiple_of(d))
}

/// Whether the teams are valued by the greatest of their performance orders: Maximum aggregation of a played live.
/// Other executions have one outcome, which the two aggregations value alike.
pub(super) fn maximum_valued(parsed: &Parsed) -> bool {
    parsed.search.aggregation == Aggregation::Maximum && parsed.kind.live()
}

/// Whether a metric's value is a probability.
pub(super) fn probability(metric: &Metric) -> bool {
    matches!(metric, Metric::ScoreAtLeast { .. } | Metric::ScoreAndLifeAtLeast { .. })
}

pub(super) fn bound(metric: &Metric, n: i128, d: u128, up: bool) -> Value {
    if probability(metric) {
        use ournotes_sim::live::certified::F64Interval;
        if n == 0 {
            return json!(0.0);
        }
        if n >= 0 && n as u128 == d {
            return json!(1.0);
        }
        let denominator = match i128::try_from(d) {
            Ok(d) => F64Interval::integer(d),
            // Both integer parts fit i128, and the interval operations preserve the full unsigned value.
            Err(_) => {
                F64Interval::integer((d >> 1) as i128).scale_integer(2).add(F64Interval::integer((d & 1) as i128))
            }
        };
        let value = F64Interval::integer(n).divide(denominator).expect("positive result denominator");
        json!(if up { value.upper() } else { value.lower() })
    } else if up {
        json!(ceil_div(n, d))
    } else {
        json!(floor_div(n, d))
    }
}

/// A team from a searched deck: its key, layout and the map from the deck's slots to the layout's.
pub(super) fn canonical(deck: &RecommendedDeck) -> (Pair, [Pair; 4], [usize; 5]) {
    let pair = |s: usize| Pair { member: deck.members[s], snap: deck.snaps[s] };
    let mut others: Vec<(Pair, usize)> = [0, 1, 3, 4].iter().map(|&s| (pair(s), s)).collect();
    others.sort();
    let mut slot = [2; 5];
    for (i, &(_, s)) in others.iter().enumerate() {
        slot[s] = [0, 1, 3, 4][i];
    }
    (pair(2), [others[0].0, others[1].0, others[2].0, others[3].0], slot)
}

pub(super) fn orders(
    deck: &RecommendedDeck,
    slot: &[usize; 5],
    layout: &Layout,
    score_metric: bool,
) -> Result<Orders, Error> {
    let bad = || Error::Domain("a played live result lacks a performance order".into());
    let mut values = [None; ORDERS];
    for &(order, score, payoff) in &deck.order_outcomes {
        let order = order.map(|s| slot[s]);
        let payoff = i64::try_from(payoff).map_err(|_| bad())?;
        let at = &mut values[order_index(&order)];
        if at.is_some() {
            return Err(bad());
        }
        *at = Some((i64::from(score), payoff));
    }
    let values: Vec<(i64, i64)> = values.iter().map(|v| v.ok_or_else(bad)).collect::<Result<_, _>>()?;
    let members = |i: usize| order_of_index(i).map(|s| layout.members[s]);
    let point = |i: usize| OrderPoint { score: values[i].0, order: members(i) };
    let mut by_score: Vec<usize> = (0..ORDERS).collect();
    by_score.sort_by_key(|&i| (values[i].0, i));
    let max = (0..ORDERS).max_by_key(|&i| (values[i].0, std::cmp::Reverse(i))).expect("orders");
    let best = (0..ORDERS).max_by_key(|&i| (values[i].1, values[i].0, std::cmp::Reverse(i))).expect("orders");
    Ok(Orders {
        count: ORDERS,
        min: point(by_score[0]),
        median: point(by_score[ORDERS / 2 - 1]),
        max: point(max),
        best: BestOrder {
            score: values[best].0,
            payoff: (!score_metric).then_some(values[best].1),
            order: members(best),
        },
        values: values.iter().map(|v| v.0).collect(),
        payoff_values: (!score_metric).then(|| values.iter().map(|v| v.1).collect()),
    })
}

/// The teams of a search outcome in their canonical layout, best first, at most `k`.
pub(super) fn teams(outcome: &RecommendationOutcome, parsed: &Parsed, with_orders: bool) -> Result<Vec<Team>, Error> {
    let metric = &parsed.search.metric;
    let score_metric = matches!(metric, Metric::Score);
    let maximum = maximum_valued(parsed);
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for deck in &outcome.results {
        let (leader, others, slot) = canonical(deck);
        if !seen.insert((leader, others)) {
            continue;
        }
        let mut layout = Layout { members: [0; 5], snaps: [None; 5] };
        for s in 0..5 {
            layout.members[slot[s]] = deck.members[s];
            layout.snaps[slot[s]] = deck.snaps[s];
        }
        let value = if parsed.kind == GoalKind::Power {
            None
        } else if maximum && deck.rank_certified.is_some() {
            // A certified team: its value is that of its best order, known exactly or within the proved interval;
            // the score is the best order's expected score once that order is proved.
            let payoff = if score_metric {
                None
            } else {
                let value = match (&deck.objective_value, &deck.payoff_interval) {
                    (Some(value), _) => {
                        let (n, d) = exact(value)?;
                        Some(n as f64 / d as f64)
                    }
                    (None, Some(interval)) => Some(interval.lower_f64()),
                    (None, None) => None,
                };
                value.map(|score| PayoffValue {
                    score,
                    exact: deck.objective_value.clone(),
                    interval: deck.payoff_interval.clone(),
                })
            };
            let score = match (&deck.best_order, &deck.score_interval) {
                (Some(best), _) => Some(i64::from(best.score)),
                (None, Some(interval)) => Some(interval.lower_f64().floor() as i64),
                (None, None) => None,
            };
            score.map(|score| TeamValue { score, exact: None, interval: deck.score_interval.clone(), payoff })
        } else if maximum {
            // The team's value is that of its best order, in which the score is that order's expected score.
            let payoff = if score_metric {
                None
            } else {
                deck.objective_value
                    .as_ref()
                    .map(|value| {
                        let (n, d) = exact(value)?;
                        Ok::<_, Error>(PayoffValue {
                            score: n as f64 / d as f64,
                            exact: Some(value.clone()),
                            interval: None,
                        })
                    })
                    .transpose()?
            };
            deck.best_order.as_ref().map(|best| TeamValue {
                score: i64::from(best.score),
                exact: Some(Fraction { numerator: best.score.to_string(), denominator: "1".into() }),
                interval: None,
                payoff,
            })
        } else {
            let score = match (&deck.expected_score, &deck.score_interval) {
                (Some(score), _) => {
                    let (n, d) = exact(score)?;
                    Some(i64::try_from(floor_div(n, d)).map_err(|_| Error::Domain("result score exceeds i64".into()))?)
                }
                (None, Some(interval)) => Some(interval.lower_f64().floor() as i64),
                (None, None) => None,
            };
            let payoff = if score_metric {
                None
            } else {
                let value = match (&deck.expected_payoff, &deck.payoff_interval) {
                    (Some(payoff), _) => {
                        let (n, d) = exact(payoff)?;
                        Some(n as f64 / d as f64)
                    }
                    (None, Some(interval)) => Some(interval.lower_f64()),
                    (None, None) => None,
                };
                value.map(|score| PayoffValue {
                    score,
                    exact: deck.expected_payoff.clone(),
                    interval: deck.payoff_interval.clone(),
                })
            };
            score.map(|score| TeamValue {
                score,
                exact: deck.expected_score.clone(),
                interval: deck.score_interval.clone(),
                payoff,
            })
        };
        let orders = if with_orders && parsed.kind.live() && deck.order_outcomes.len() == ORDERS {
            Some(orders(deck, &slot, &layout, score_metric)?)
        } else {
            None
        };
        let exact_value = |fraction: &Fraction| -> Result<PayoffValue, Error> {
            let (n, d) = exact(fraction)?;
            Ok(PayoffValue { score: n as f64 / d as f64, exact: Some(fraction.clone()), interval: None })
        };
        let levels = match (metric.levels(), &deck.level_payoffs, &deck.term_payoffs) {
            (Some(levels), Some(values), Some(terms)) => {
                let mut terms = terms.iter();
                let rows = levels.iter().zip(values).map(|(level, value)| {
                    Ok(TeamLevel {
                        value: exact_value(value)?,
                        terms: terms
                            .by_ref()
                            .take(level.terms.len())
                            .map(|term| term.as_ref().map(exact_value).transpose())
                            .collect::<Result<_, Error>>()?,
                    })
                });
                Some(rows.collect::<Result<Vec<_>, Error>>()?)
            }
            _ => None,
        };
        // A Challenge-point priority reads its three levels: Challenge points, then the two rewards in its order.
        let event_rewards = match (&levels, parsed.metric.as_ref().and_then(|echo| echo["secondaryPriority"].as_str()))
        {
            (Some(levels), Some(priority)) if levels.len() == 3 => {
                let (points, items) = if priority == "eventPointsFirst" { (1, 2) } else { (2, 1) };
                Some(TeamEventRewards {
                    challenge_points: levels[0].value.clone(),
                    event_points: levels[points].value.clone(),
                    event_items: levels[items].value.clone(),
                })
            }
            _ => None,
        };
        let best_order = if maximum {
            deck.best_order
                .as_ref()
                .map(|best| {
                    let payoff = if score_metric {
                        None
                    } else {
                        Some(
                            best.payoff
                                .parse::<i64>()
                                .map_err(|_| Error::Domain("result payoff exceeds i64".into()))?,
                        )
                    };
                    Ok::<_, Error>(BestOrder { score: i64::from(best.score), payoff, order: best.members })
                })
                .transpose()?
        } else {
            None
        };
        rows.push(Team {
            rank: 0,
            leader,
            others,
            power: deck.power,
            value,
            orders,
            best_order,
            layout,
            rank_certified: deck.rank_certified,
            event_rewards,
            levels,
        });
    }
    // Preserve the search ordering and its proof. Changing the tie objective here would not recover
    // teams already pruned by the solver.
    rows.truncate(parsed.k);
    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(i, mut team)| {
            team.rank = i + 1;
            team
        })
        .collect())
}

/// The memory of the solver: on WebAssembly the linear memory, which never shrinks, so its size is also the peak.
pub(super) fn memory() -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = core::arch::wasm32::memory_size(0) as u64 * 65536;
        json!({"currentBytes": bytes, "peakBytes": bytes})
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        json!({"currentBytes": null, "peakBytes": null})
    }
}

pub(super) fn result_of(
    outcome: &RecommendationOutcome,
    parsed: &Parsed,
    scope: &Value,
    start: Instant,
    is_final: bool,
) -> Result<AnswerResult, Error> {
    let teams = teams(outcome, parsed, is_final)?;
    let metric = &parsed.search.metric;
    let live = parsed.kind.live();
    let proof = &outcome.telemetry.proof;
    let proven = outcome.optimality == Optimality::Proven;
    let best_deck = outcome.results.first();
    let best =
        best_deck.and_then(|d| d.objective_value.as_ref().or(d.expected_payoff.as_ref())).map(exact).transpose()?;
    let lower_bound = best.map(|(n, d)| bound(metric, n, d, false)).or_else(|| {
        best_deck
            .and_then(|d| d.payoff_interval.as_ref())
            .map(|v| if probability(metric) { json!(v.lower_f64()) } else { json!(v.lower_f64().floor() as i64) })
    });
    let upper_bound = if proven {
        best.map(|(n, d)| bound(metric, n, d, true)).or_else(|| {
            best_deck
                .and_then(|d| d.payoff_interval.as_ref())
                .map(|v| if probability(metric) { json!(v.upper_f64()) } else { json!(v.upper_f64().ceil() as i64) })
        })
    } else {
        // Payoff numerators of the proof are over the target's denominator.
        // upper_bound covers only unexplored branches. It cannot replace a missing global bound: retained
        // interval candidates can exceed it, and a displayed Top-K does not certify the rest of that frontier.
        let target = outcome.telemetry.environment.target.as_ref().and_then(|t| t.denominator.parse::<u128>().ok());
        let denominator = target.unwrap_or(if live && !maximum_valued(parsed) { ORDERS as u128 } else { 1 });
        proof
            .global_upper_bound
            .as_deref()
            .and_then(|u| u.parse::<i128>().ok())
            .map(|n| bound(metric, n, denominator, true))
    };
    let mut telemetry = outcome.telemetry.clone();
    if !is_final {
        telemetry.incumbents.timeline.clear();
    }
    let mut telemetry = serde_json::to_value(&telemetry).map_err(|e| Error::Domain(format!("telemetry: {e}")))?;
    telemetry["memory"] = memory();
    let mut account = scope.clone();
    account["goal"] = json!(parsed.kind.name());
    Ok(AnswerResult {
        goal: parsed.goal.clone(),
        metric: parsed.metric.clone(),
        aggregation: parsed.search.aggregation,
        maximum_model: (parsed.search.aggregation == Aggregation::Maximum)
            .then(|| MaximumModel::for_execution(parsed.kind.live())),
        phase: if is_final { "done" } else { "search" },
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        optimality: OptimalityReport {
            proven,
            lower_bound,
            upper_bound,
            best_gap: if proven { Some(0.0) } else { proof.best_gap },
            kth_gap: if proven { Some(0.0) } else { proof.kth_gap },
            fraction: proof.fraction,
        },
        teams,
        account,
        telemetry,
    })
}

pub(super) fn issue(path: &str, error: Error) -> Issue {
    let (code, message) = match error {
        Error::Master(m) => ("master", m),
        Error::Input(m) => ("input", m),
        Error::Game(m) => ("game", m),
        Error::Unsupported(m) => ("unsupported", m),
        Error::Domain(m) => ("domain", m),
        Error::Capacity(m) => ("capacity", m),
    };
    Issue { path: path.into(), code: code.into(), message }
}

/// The status of an answer with these issues: computation-side errors (`unsupported`, `master`, `game`, `domain`,
/// `capacity`) alone fail it, any other error makes it invalid, missing facts alone make it incomplete.
pub(super) fn status_of(missing: &[Issue], errors: &[Issue]) -> Status {
    let computation = |i: &Issue| matches!(i.code.as_str(), "unsupported" | "master" | "game" | "domain" | "capacity");
    if errors.is_empty() {
        if missing.is_empty() { Status::Ok } else { Status::Incomplete }
    } else if errors.iter().all(computation) {
        Status::Failed
    } else {
        Status::Invalid
    }
}
