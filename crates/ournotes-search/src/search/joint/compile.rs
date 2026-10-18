//! Compilation of the joint bounds from the pool, the domain and the objective.

use super::*;

/// Every gain row replaced by its rounded-up position mean (see the module documentation).
fn mean_table(gains: &[Vec<[f64; 5]>]) -> Vec<Vec<[f64; 5]>> {
    gains.iter().map(|rows| rows.iter().map(crate::search::uniform::mean_row).collect()).collect()
}

/// The pairs of the choice order whose per-position gains make the position potentials of
/// [`JointBounds::order_maximum`].
const POTENTIAL_PAIRS: usize = 64;

/// How far a row's largest per-position gain lies above its position mean (`mean[0]`, every entry the same), rounded
/// up so that `add_up(mean, spread)` is at least the largest gain.
pub(super) fn row_spread(row: &[f64; 5], mean: &[f64; 5]) -> f64 {
    (row.iter().copied().fold(0.0, f64::max) - mean[0]).next_up().max(0.0)
}

/// The spread of every member and choice (see `JointBounds::spread`), the sums of the largest member spreads and the
/// largest per-position gains.
fn spread_tables(
    order_gains: &[Vec<[f64; 5]>],
    gains: &[Vec<[f64; 5]>],
    domain: &CandidateDomain,
) -> (Vec<Vec<f64>>, [f64; 6], [f64; 5]) {
    let spread: Vec<Vec<f64>> = order_gains
        .iter()
        .zip(gains)
        .map(|(rows, means)| rows.iter().zip(means).map(|(row, mean)| row_spread(row, mean)).collect())
        .collect();
    let mut largest: Vec<f64> =
        domain.members().iter().map(|&m| spread[m].iter().copied().fold(0.0, f64::max)).collect();
    largest.sort_by(|a, b| b.total_cmp(a));
    let mut top = [0.0; 6];
    for k in 1..6 {
        top[k] = add_up(top[k - 1], largest.get(k - 1).copied().unwrap_or(0.0));
    }
    let mut column = [0.0f64; 5];
    for &m in domain.members() {
        for row in &order_gains[m] {
            for (c, &g) in column.iter_mut().zip(row) {
                *c = c.max(g);
            }
        }
    }
    (spread, top, column)
}

impl JointBounds {
    pub(crate) fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
        input: Option<&EventPayoffInput>,
        simulation: &SimulationInput,
        budget: SearchBudget,
    ) -> Result<Self, Error> {
        Self::compile_with_laws(pool, request, domain, metric, input, simulation, &[], None, budget)
    }

    /// [`JointBounds::compile`] with the certified lottery laws of every team of the domain in every order: the
    /// per-note bounds then weigh the Rush factor and the direct-Rush score ups by their certified class masses
    /// (see `LuckLaws`). No law: every note takes its largest luck factor.
    ///
    /// A weighted bound caps the expected score of an order, which a certified LUCK live ranks its orders by
    /// ([`PointBound::ranked`]). The luck weights are read from `memo` when it holds those of the same laws and notes.
    /// `budget` is the request's: preparation that would run past its deadline fails as unavailable instead of
    /// starting another step, and the caller then searches without these bounds.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compile_with_laws(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
        input: Option<&EventPayoffInput>,
        simulation: &SimulationInput,
        laws: &[std::sync::Arc<ournotes_sim::live::full::LuckDpCertifiedResult>],
        memo: Option<&crate::search::snaps::WeightsMemo>,
        budget: SearchBudget,
    ) -> Result<Self, Error> {
        if budget.expired() {
            return Err(unavailable("the request deadline was reached before the bounds compiled"));
        }
        if !domain.is_feasible() {
            return Err(unavailable("empty legal domain"));
        }
        if !laws.is_empty() && !metric.weighs_laws() {
            return Err(unavailable("lottery laws weigh no score target"));
        }
        if !matches!(request.objective.inner(), Objective::LiveScore { .. })
            || !matches!(
                metric,
                Metric::Score
                    | Metric::ClientEventPoints { .. }
                    | Metric::ClientChallengePoints { .. }
                    | Metric::RankedEventItems { .. }
                    | Metric::ScoreAtLeast { .. }
                    | Metric::CappedScore { .. }
                    | Metric::ScoreAndLifeAtLeast { .. }
                    | Metric::Combined { .. }
            )
        {
            return Err(unavailable("joint bounds require a Live score, score target, or event objective"));
        }
        if simulation.music_length_ms.is_some()
            || simulation.score_music_length_ms.is_some()
            || matches!(request.objective.inner(), Objective::LiveScore {play: crate::search::PlayInput::Stream {stream,..},..} if stream.delta_times.is_some())
        {
            return Err(unavailable("explicit duration/delta clocks require exhaustive fallback"));
        }
        let n = pool.members.len();
        if n.saturating_mul(n.saturating_add(domain.snaps().len() + 1)) > 1_000_000 {
            return Err(unavailable("joint preparation exceeds bounded table capacity"));
        }
        let (song, event, _) = crate::search::objective_song(pool, &request.objective)?;
        let t = Tables::new(pool, song, event, domain.snaps(), budget)?
            .ok_or_else(|| unavailable("the request deadline was reached while the power tables built"))?;
        let mut allowed = vec![false; n];
        for &m in domain.members() {
            allowed[m] = true;
        }
        // A stronger gate than the power solver: each slot is nonnegative for every profile.
        for profile in 0..t.profiles.len() {
            for &m in domain.members() {
                if t.member_power_lower_bound(pool, profile, m)? < 0 {
                    return Err(unavailable("joint bound cannot certify nonnegative slot power"));
                }
            }
        }
        let maximum_slot = domain
            .members()
            .iter()
            .map(|&m| t.a[m] + t.lead.iter().map(|r| r[m]).max().unwrap_or(0) + t.wmax[m])
            .max()
            .unwrap_or(0);
        if maximum_slot.checked_mul(5).is_none_or(|v| v >= i32::MAX as i64) {
            return Err(unavailable("joint power relaxation exceeds nonwrapping domain"));
        }
        let setup =
            crate::search::full_setup(pool, &request.objective)?.ok_or_else(|| unavailable("missing Live setup"))?;
        if budget.expired() {
            return Err(unavailable("the request deadline was reached while the bounds compiled"));
        }
        let envelope = SnapLive::new(pool, &t, &allowed, &setup, laws, memo)?;
        let nonnegative_scores = envelope.nonnegative_scores();
        let (a0, global, eps, order_gains) = envelope.joint_envelope();
        let levels = if setup.gk.is_some() { envelope.joint_carrier_levels() } else { None };
        let keys = levels.as_ref().and_then(|_| envelope.carrier_keys());
        if ![a0, global, eps].iter().all(|v| v.is_finite() && *v >= 0.0)
            || order_gains.iter().flatten().flatten().any(|g| !g.is_finite() || *g < 0.0)
        {
            return Err(unavailable("nonfinite/negative score relaxation"));
        }
        let gains = mean_table(&order_gains);
        let (spread, spread_top, column) = spread_tables(&order_gains, &gains, domain);
        let term_points = |metric: &Metric| -> Result<Option<PointBound>, Error> {
            Ok(match metric {
                Metric::ScoreAtLeast { threshold } | Metric::ScoreAndLifeAtLeast { threshold, .. } => {
                    Some(PointBound::score_target(
                        pool.members.len(),
                        domain.snaps().len(),
                        ScoreTarget::AtLeast(*threshold),
                    ))
                }
                Metric::CappedScore { threshold } => Some(PointBound::score_target(
                    pool.members.len(),
                    domain.snaps().len(),
                    ScoreTarget::Capped(*threshold),
                )),
                Metric::ClientEventPoints { event_id } | Metric::ClientChallengePoints { event_id } => {
                    Some(PointBound::compile(
                        pool,
                        request,
                        domain,
                        input.ok_or_else(|| unavailable("missing event input"))?,
                        *event_id,
                        matches!(metric, Metric::ClientChallengePoints { .. }),
                        nonnegative_scores,
                    )?)
                }
                Metric::RankedEventItems { event_id, resource_type, resource_id } => {
                    // Under laws the envelope caps an expected score, which is the score a certified LUCK live reads
                    // its grade at.
                    let cap = ((maximum_slot * 5) as f64 * global * (1.0 + eps)).ceil();
                    if !cap.is_finite() {
                        return Err(unavailable("ranked-item score support is not finite"));
                    }
                    let input = input.ok_or_else(|| unavailable("missing event input"))?;
                    let context =
                        request.objective.context().ok_or_else(|| unavailable("items need resolved context"))?;
                    let route = context.event_request(pool.master, input, *event_id)?.route;
                    let nonnegative = nonnegative_scores
                        || point_bound::negative_scores_unpaid(pool, context, input, |rank| {
                            ournotes_sim::event::ranked_event_reward(pool.master, *event_id, &route, rank).is_ok()
                        });
                    let support = (if nonnegative { 0 } else { i32::MIN }, cap.clamp(0.0, i32::MAX as f64) as i32);
                    Some(PointBound::compile_items(
                        pool,
                        request,
                        domain,
                        input,
                        (*event_id, *resource_type, *resource_id),
                        support,
                    )?)
                }
                _ => None,
            })
        };
        let points = match metric.folded_terms() {
            // A term without a bound of its own pays its score, which no score exceeds.
            Some(terms) => {
                let parts = terms
                    .map(|(weight, term)| {
                        let part = term_points(term)?.unwrap_or_else(|| {
                            PointBound::score_target(
                                pool.members.len(),
                                domain.snaps().len(),
                                ScoreTarget::Capped(i32::MAX),
                            )
                        });
                        Ok((weight, part))
                    })
                    .collect::<Result<Vec<_>, Error>>()?;
                Some(PointBound::combined(pool, domain, parts)?)
            }
            None => term_points(metric)?,
        };
        // A certified LUCK live ranks its orders by their rewards at their expected scores and then by those scores.
        use crate::search::physical::certified_engine::{LotteryMode, lottery_mode};
        let certified = lottery_mode(pool, request, domain)? == LotteryMode::Certified;
        let points = match points {
            Some(pt) if certified => Some(pt.ranked()),
            points => points,
        };
        // Per-note caps of complete teams: one per performance order, and the cutoff tables of the simulations.
        let fine = Some(envelope.into_joint_fine());
        // This estimate only orders branches. It never removes a pair or claims a native order.
        // A combined metric whose first level reads no event bonus is decided by the score first: the term that reads
        // the bonuses only orders the pairs whose power is equal.
        let power_first = points.as_ref().is_some_and(|pt| pt.is_combined() && pt.weight < metric.primary_unit());
        let priority = |(m, s): (usize, usize)| {
            let power =
                t.a[m] + t.lead.iter().map(|r| r[m]).max().unwrap_or(0) + if s == 0 { 0 } else { t.w[m][s - 1] };
            match &points {
                Some(pt) => {
                    let bonus = (pt.member[m] + if s == 0 { 0 } else { pt.snap[s - 1] }) as f64;
                    if power_first { (power as f64, bonus) } else { (bonus * 1e12 + power as f64, 0.0) }
                }
                None => (power as f64 * (a0 / 5.0 + gains[m][s].iter().copied().fold(0.0, f64::max)), 0.0),
            }
        };
        // Each pair's estimate is taken once, before the sort; the stable sort and its comparison are unchanged.
        let mut keyed: Vec<_> = domain
            .members()
            .iter()
            .flat_map(|&m| (0..=domain.snaps().len()).map(move |s| (m, s)))
            .map(|c| (priority(c), c))
            .collect();
        keyed.sort_by(|&(pa, a), &(pb, b)| {
            pb.0.total_cmp(&pa.0)
                .then_with(|| pb.1.total_cmp(&pa.1))
                .then_with(|| pool.members[a.0].id.cmp(&pool.members[b.0].id))
                .then(a.1.cmp(&b.1))
        });
        let choices: Vec<_> = keyed.into_iter().map(|(_, c)| c).collect();
        let scale =
            2.0f64.powf(((maximum_slot.max(1) as f64 * 5.0) / global.max(1e-100)).log2().round().clamp(-500.0, 500.0));
        let correlation_scales = [scale * 0.25, scale, scale * 4.0];
        let mut compiled = Self {
            a: t.a,
            w: t.w,
            lead: t.lead,
            profile: t.profile_of,
            gains,
            order_gains,
            spread,
            spread_top,
            column,
            a0,
            global,
            eps,
            points,
            // An order short of the least final life pays nothing, but under LUCK it still ranks by its expected score.
            min_final_life: match metric {
                Metric::ScoreAndLifeAtLeast { min_final_life, .. } if !certified => Some(i64::from(*min_final_life)),
                _ => None,
            },
            fine,
            correlation_scales,
            tails: None,
            composition: None,
            modules: Vec::new(),
            gekisou: setup.gk.is_some(),
            class_search: false,
            class_resource_caps: false,
            rules: None,
            relax_tables: None,
            forced_tables: Default::default(),
            step_pairs: None,
            prefix_resource: None,
            prefix_character: None,
            choices,
            carrier_levels: None,
            carrier_split: None,
            laws: laws.to_vec(),
            law_clusters: None,
            part: None,
            order_potentials: None,
            order_offset: 0.0,
            root_keys: None,
        };
        compiled.compile_tables(pool, domain);
        if budget.expired() {
            return Err(unavailable("the request deadline was reached while the bounds compiled"));
        }
        if !compiled.gekisou {
            compiled.composition = composition::CompositionTables::compile(&compiled, pool, domain);
            if let Some(tables) = lambda::LambdaTables::compile(&compiled, pool, domain) {
                compiled.modules.push(Box::new(tables));
            }
        }
        if budget.expired() {
            return Err(unavailable("the request deadline was reached while the bounds compiled"));
        }
        if let Some((levels, carrier)) = levels {
            let at = levels
                .into_iter()
                .map(|level| {
                    level.map(|(a0, global, gains)| {
                        // every level is at most the pool-wide envelope checked above
                        assert!(
                            [a0, global].iter().all(|v| v.is_finite() && *v >= 0.0)
                                && gains.iter().flatten().flatten().all(|g| g.is_finite() && *g >= 0.0),
                            "nonfinite/negative carrier level"
                        );
                        Box::new(compiled.level(pool, domain, a0, global, gains))
                    })
                })
                .collect();
            compiled.carrier_levels = Some(CarrierLevels { carrier, at, keys });
            compiled.carrier_split = carrier_split::CarrierSplit::compile(&compiled, pool, domain);
        }
        Ok(compiled)
    }

    /// Turns these bounds into bounds of the payoff of each performance order, for the maximum aggregation. For
    /// position potentials `v`, a team's gain sum in an order is at most `sum(v)` plus, for each of its slots, the
    /// largest excess of the slot's gains over `v` (the LP dual of the assignment of slots to positions): each slot
    /// takes one position and the positions are taken once each. So every node bound reads each row's excess in place
    /// of its position mean, and its base coefficient holds `sum(v)`: the cheap envelope read at any positions with
    /// mass 1 bounds the score of every order of every completion, and so does each relaxation of it that reads these
    /// gains and coefficients (the correlated, character, resource, tail and pair bounds). The per-position gains keep
    /// their meaning; the per-order caps of complete teams and the placement sums read them with the base coefficient
    /// less `sum(v)`. The order-step and carrier-split bounds bound sums over the orders: the carrier split is dropped,
    /// and a traversal under the maximum aggregation reads no order-step bound.
    ///
    /// Any potentials keep the bounds valid; these are the mean gains at each position of the first pairs of the
    /// choice order, the strongest by its estimate, so that a strong team's excesses are small.
    pub(crate) fn order_maximum(&mut self, pool: &Pool, domain: &CandidateDomain) {
        let n = self.choices.len().min(POTENTIAL_PAIRS);
        let mut v = [0.0f64; 5];
        for &(m, choice) in &self.choices[..n] {
            for (sum, &g) in v.iter_mut().zip(&self.order_gains[m][choice]) {
                *sum += g;
            }
        }
        v = v.map(|sum| sum / n.max(1) as f64);
        self.order_maximum_with(pool, domain, v);
        if let Some(clusters) = &mut self.law_clusters {
            for bounds in &mut clusters.bounds {
                bounds.order_maximum(pool, domain);
            }
        }
    }

    /// The score thresholds of the point bound's tiers, ascending and distinct (empty without tiers).
    pub(crate) fn tier_scores(&self) -> Vec<i64> {
        let mut scores = Vec::new();
        if let Some(pt) = &self.points {
            pt.tier_scores(&mut scores);
        }
        scores.sort_unstable();
        scores.dedup();
        scores
    }

    /// Drops the point tiers at and above `threshold`, which no performance order of a team of the domain reaches in
    /// expected score, from these bounds and their law clusters ([`JointBounds::drop_tiers`]). The root loop of each
    /// keeps the order of the leader pairs' keys before the cap, read with `orders` (see [`JointBounds::root_key`]):
    /// the dropped tiers bound no reward, but they still rank first the leaders whose teams reach the higher scores.
    pub(crate) fn cap_scores(
        &mut self,
        pool: &Pool,
        domain: &CandidateDomain,
        threshold: i64,
        orders: &[([usize; 5], u128)],
    ) {
        if self.points.is_none() {
            return;
        }
        let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        let keys = self
            .choices
            .iter()
            .map(|&(m, choice)| {
                p.members[SLOTS[0]] = m;
                p.snaps[SLOTS[0]] = (choice > 0).then(|| domain.snaps()[choice - 1]);
                self.node_bound_reading(pool, domain, &p, 1, orders, false).ok()
            })
            .collect();
        self.root_keys = Some(keys);
        self.drop_tiers(pool, domain, threshold);
        if let Some(clusters) = &mut self.law_clusters {
            for bounds in &mut clusters.bounds {
                bounds.cap_scores(pool, domain, threshold, orders);
            }
        }
    }

    /// Drops the point tiers at and above `threshold`, which no performance order of a team of the domain reaches in
    /// expected score, from these bounds and their carrier levels, and rebuilds the tables that read them.
    pub(crate) fn drop_tiers(&mut self, pool: &Pool, domain: &CandidateDomain, threshold: i64) {
        let Some(pt) = &mut self.points else { return };
        pt.cap_scores(threshold - 1);
        self.compile_tables(pool, domain);
        if let Some(levels) = &mut self.carrier_levels {
            for level in levels.at.iter_mut().flatten() {
                level.drop_tiers(pool, domain, threshold);
            }
        }
        if self.carrier_split.is_some() {
            self.carrier_split = carrier_split::CarrierSplit::compile(self, pool, domain);
        }
        if self.rules.is_some() {
            let rules = self.rules.take();
            self.set_rules(pool, domain, rules);
        }
    }

    /// [`JointBounds::order_maximum`] with the potentials `v`, for these bounds and their carrier levels.
    fn order_maximum_with(&mut self, pool: &Pool, domain: &CandidateDomain, v: [f64; 5]) {
        self.order_offset = v.iter().fold(0.0, |sum, &x| add_up(sum, x));
        self.order_potentials = Some(v);
        self.a0 = add_up(self.a0, self.order_offset);
        self.gains = self.order_gains.iter().map(|rows| rows.iter().map(|row| excess_row(row, &v)).collect()).collect();
        // the excesses bound every position's gain over the potentials: no spread is left above them
        self.spread = self.order_gains.iter().map(|rows| vec![0.0; rows.len()]).collect();
        self.spread_top = [0.0; 6];
        self.compile_tables(pool, domain);
        if !self.gekisou {
            self.composition = composition::CompositionTables::compile(self, pool, domain);
            self.modules.clear();
            if let Some(tables) = lambda::LambdaTables::compile(self, pool, domain) {
                self.modules.push(Box::new(tables));
            }
        }
        self.carrier_split = None;
        if let Some(levels) = &mut self.carrier_levels {
            for level in levels.at.iter_mut().flatten() {
                level.order_maximum_with(pool, domain, v);
            }
        }
        // the forced slots' tables read the relaxation tables
        if self.rules.is_some() {
            let rules = self.rules.take();
            self.set_rules(pool, domain, rules);
        }
    }

    /// These cheap bounds with another linear envelope (per-position `gains`): the same power tables and choice order,
    /// no fine bound.
    fn level(&self, pool: &Pool, domain: &CandidateDomain, a0: f64, global: f64, gains: Vec<Vec<[f64; 5]>>) -> Self {
        let means = mean_table(&gains);
        let (spread, spread_top, column) = spread_tables(&gains, &means, domain);
        let mut b = Self {
            a: self.a.clone(),
            w: self.w.clone(),
            lead: self.lead.clone(),
            profile: self.profile.clone(),
            gains: means,
            order_gains: gains,
            spread,
            spread_top,
            column,
            a0,
            global,
            eps: self.eps,
            points: self.points.clone(),
            min_final_life: self.min_final_life,
            fine: None,
            correlation_scales: self.correlation_scales,
            tails: None,
            composition: None,
            modules: Vec::new(),
            gekisou: self.gekisou,
            class_search: false,
            class_resource_caps: false,
            rules: None,
            relax_tables: None,
            forced_tables: Default::default(),
            step_pairs: None,
            prefix_resource: None,
            prefix_character: None,
            choices: self.choices.clone(),
            carrier_levels: None,
            carrier_split: None,
            laws: Vec::new(),
            law_clusters: None,
            part: None,
            order_potentials: None,
            order_offset: 0.0,
            root_keys: None,
        };
        b.compile_tables(pool, domain);
        b
    }

    /// The optional tables of the cheap bounds: the choice-suffix maxima, the relaxation tables, the order-step
    /// candidates and, for Gekisou score, the prefix resource and character tables.
    fn compile_tables(&mut self, pool: &Pool, domain: &CandidateDomain) {
        // Bounded storage; an unavailable optional table never truncates the search domain.
        self.tails = ((self.lead.len() + 6).saturating_mul(self.choices.len() + 1) <= 1_000_000).then(|| {
            let n = self.choices.len();
            let others = self.points.as_ref().map_or(0, |pt| pt.others.len());
            let mut suffix = tail::PairMaxima::zeros(self.lead.len(), others, n + 1);
            let mut blocks = tail::PairMaxima::zeros(self.lead.len(), others, n.div_ceil(tail::PAIR_BLOCK));
            for i in (0..n).rev() {
                let (m, choice) = self.choices[i];
                suffix.merge(i, i + 1);
                suffix.add(self, i, m, choice);
                blocks.add(self, i / tail::PAIR_BLOCK, m, choice);
            }
            TailTables { suffix, blocks }
        });
        self.relax_tables = relax_tables::RelaxTables::compile(self, pool, domain);
        self.step_pairs = self.points.as_ref().map(|pt| order_steps::StepPairs::compile(self, pool, pt));
        if self.gekisou {
            let (resource, character) = prefix_character::compile_prefix_tables(self, pool, domain);
            // The resource caps bound a score payoff; the character caps also bound the mean score a points bound reads.
            self.prefix_resource = resource.filter(|_| self.points.is_none());
            self.prefix_character = character;
        }
    }
}
