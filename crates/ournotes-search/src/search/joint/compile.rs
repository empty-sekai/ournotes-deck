//! Compilation of the joint bounds from the pool, the domain and the objective.

use super::*;

/// Every gain row replaced by its rounded-up position mean (see the module documentation).
fn mean_table(gains: &[Vec<[f64; 5]>]) -> Vec<Vec<[f64; 5]>> {
    gains.iter().map(|rows| rows.iter().map(crate::search::uniform::mean_row).collect()).collect()
}

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
    ) -> Result<Self, Error> {
        Self::compile_with_laws(pool, request, domain, metric, input, simulation, &[], None)
    }

    /// [`JointBounds::compile`] with the certified lottery laws of every team of the domain in every order: the
    /// per-note bounds then weigh the Rush factor and the direct-Rush score ups by their certified class masses
    /// (see `LuckLaws`). No law: every note takes its largest luck factor.
    ///
    /// A weighted bound caps the expected score of an order, which is the order's expected payoff only when the
    /// payoff is the score itself, so laws compile for no other metric. The luck weights are read from `memo` when it
    /// holds those of the same laws and notes.
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
    ) -> Result<Self, Error> {
        if !domain.is_feasible() {
            return Err(unavailable("empty legal domain"));
        }
        if !laws.is_empty() && !matches!(metric, Metric::Score) {
            return Err(unavailable("lottery laws weigh only a score objective"));
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
        let budget = SearchBudget::new(Instant::now(), None)?;
        let t = Tables::new(pool, song, event, domain.snaps(), budget)?.expect("unlimited preparation");
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
        let envelope = SnapLive::new(pool, &t, &allowed, &setup, laws, memo)?;
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
                        eps < 1.0,
                    )?)
                }
                Metric::RankedEventItems { event_id, resource_type, resource_id } => {
                    let cap = ((maximum_slot * 5) as f64 * global * (1.0 + eps)).ceil();
                    if !cap.is_finite() {
                        return Err(unavailable("ranked-item score support is not finite"));
                    }
                    let support = (if eps < 1.0 { 0 } else { i32::MIN }, cap.clamp(0.0, i32::MAX as f64) as i32);
                    Some(PointBound::compile_items(
                        pool,
                        request,
                        domain,
                        input.ok_or_else(|| unavailable("missing event input"))?,
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
        // Per-note caps of complete teams: one per performance order, and the cutoff tables of the simulations.
        let fine = Some(envelope.into_joint_fine());
        // This estimate only orders branches. It never removes a pair or claims a native order.
        let priority = |(m, s): (usize, usize)| {
            let power =
                t.a[m] + t.lead.iter().map(|r| r[m]).max().unwrap_or(0) + if s == 0 { 0 } else { t.w[m][s - 1] };
            match &points {
                Some(pt) => (pt.member[m] + if s == 0 { 0 } else { pt.snap[s - 1] }) as f64 * 1e12 + power as f64,
                None => power as f64 * (a0 / 5.0 + gains[m][s].iter().copied().fold(0.0, f64::max)),
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
            pb.total_cmp(&pa).then_with(|| pool.members[a.0].id.cmp(&pool.members[b.0].id)).then(a.1.cmp(&b.1))
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
            min_final_life: match metric {
                Metric::ScoreAndLifeAtLeast { min_final_life, .. } => Some(i64::from(*min_final_life)),
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
            prefix_resource: None,
            prefix_character: None,
            choices,
            carrier_levels: None,
            carrier_split: None,
            laws: laws.to_vec(),
            law_clusters: None,
        };
        compiled.compile_tables(pool, domain);
        if !compiled.gekisou {
            compiled.composition = composition::CompositionTables::compile(&compiled, pool, domain);
            if let Some(tables) = lambda::LambdaTables::compile(&compiled, pool, domain) {
                compiled.modules.push(Box::new(tables));
            }
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
            prefix_resource: None,
            prefix_character: None,
            choices: self.choices.clone(),
            carrier_levels: None,
            carrier_split: None,
            laws: Vec::new(),
            law_clusters: None,
        };
        b.compile_tables(pool, domain);
        b
    }

    /// The optional tables of the cheap bounds: the choice-suffix maxima, the relaxation tables and, for Gekisou
    /// score, the prefix resource and character tables.
    fn compile_tables(&mut self, pool: &Pool, domain: &CandidateDomain) {
        // Bounded storage; an unavailable optional table never truncates the search domain.
        self.tails = ((self.lead.len() + 6).saturating_mul(self.choices.len() + 1) <= 1_000_000).then(|| {
            let n = self.choices.len();
            let mut power = vec![vec![0; n + 1]; self.lead.len()];
            let mut gain: [Vec<f64>; 5] = std::array::from_fn(|_| vec![0.0; n + 1]);
            let mut bonus = vec![0; n + 1];
            let mut spread = vec![0.0f64; n + 1];
            let mut order_gain: [Vec<f64>; 5] = std::array::from_fn(|_| vec![0.0; n + 1]);
            for i in (0..n).rev() {
                let (m, choice) = self.choices[i];
                spread[i] = spread[i + 1].max(self.spread[m][choice]);
                for (pos, column) in order_gain.iter_mut().enumerate() {
                    column[i] = column[i + 1].max(self.order_gains[m][choice][pos]);
                }
                for (profile, row) in power.iter_mut().enumerate() {
                    let p = self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
                    row[i] = row[i + 1].max(p);
                }
                for pos in 0..5 {
                    gain[pos][i] = gain[pos][i + 1].max(self.gains[m][choice][pos]);
                }
                if let Some(pt) = &self.points {
                    bonus[i] = bonus[i + 1].max(pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
                }
            }
            TailTables { power, gain, bonus, spread, order_gain }
        });
        self.relax_tables = relax_tables::RelaxTables::compile(self, pool, domain);
        if self.gekisou && self.points.is_none() {
            (self.prefix_resource, self.prefix_character) = prefix_character::compile_prefix_tables(self, pool, domain);
        }
    }
}
