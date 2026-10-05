//! Joint member/Snap relaxation of the uniform member-order target (`super::uniform`).
//!
//! The cheap envelope bounds the score of a team in one performance order by `P * min(A0 + sum of the slots' position
//! gains, G) * (1 + eps)`. Its expectation over the 120 orders is at most the same expression with every gain
//! replaced by its mean over the five positions (linearity of the gain sum, Jensen for the concave `min`), so every
//! node bound reads position-mean gains, and a cap read at [`super::uniform::MEAN_ORDERS`] bounds the sum over the
//! orders. The per-note fine and raw caps keep their per-order structure: a complete team gets one of each per
//! order, and their sum bounds its value.
use super::expectation::PhysicalDeck;
use super::{
    Objective, Pool, SearchRequest,
    budget::SearchBudget,
    snaps::{CarrierKeys, JointFineBounds, JointScratch, SnapLive},
    tables::Tables,
};
use crate::{
    clock::Instant,
    domain::CandidateDomain,
    types::{Metric, SimulationInput},
};
use ournotes_sim::Error;
use ournotes_sim::scenario::EventPayoffInput;
use std::collections::{HashMap, HashSet};

mod bonus;
mod carrier_split;
mod classes;
mod composition;
mod cutoff;
mod lambda;
mod point_route;
mod prefix_character;
mod prefix_resource;
mod relax_tables;
mod resource;
mod split_tables;
pub(crate) use bonus::BonusScratch;

/// Leader first, then the other physical slots. Performance order is never a decision.
pub(crate) const SLOTS: [usize; 5] = [2, 0, 1, 3, 4];

/// A bound module: an upper bound of the payoff numerator, over the performance-order masses `orders`, of every
/// completion of a partial team. `p` holds the members of `SLOTS[..depth]` (the leader first) and, with
/// `snaps_placed`, the Snaps of those slots (joint traversal); otherwise their Snaps are still free (composition
/// traversal). A complete team (`depth == 5`) is bounded over its Snap pairings. None when the module does not apply
/// to the node. Both traversals prune a node whose module bound is below the K-th payoff, or equal to it with a smaller
/// power bound; the modules of a compiled `JointBounds` are listed by [`JointBounds::modules`].
pub(crate) trait NodeBound {
    /// The module's key in the telemetry (`joint.modules`, `composition.modules`).
    fn name(&self) -> &'static str;
    fn node_upper(
        &self,
        pool: &Pool,
        p: &PhysicalDeck,
        depth: usize,
        snaps_placed: bool,
        orders: &[([usize; 5], u128)],
    ) -> Option<i128>;
}

#[derive(Clone)]
struct PointBound {
    member: Vec<i64>,
    snap: Vec<i64>,
    multiplier: i64,
    /// Prefix maximum reward multiplier at reachable score thresholds (solo rank rules only).
    score_tiers: Option<Vec<(i64, i64)>>,
    /// The concave majorant of the step multiplier `score_tiers` (see `uniform::concave_majorant`).
    hull: Option<Vec<(i64, i64)>>,
    target: Option<ScoreTarget>,
}

#[derive(Clone, Copy)]
enum ScoreTarget {
    AtLeast(i32),
    Capped(i32),
}

struct TailTables {
    power: Vec<Vec<i64>>,
    gain: [Vec<f64>; 5],
    bonus: Vec<i64>,
}

/// One prefix's unconstrained remaining slots, excluding the next pair being enumerated.
pub(crate) struct TailState {
    profile: usize,
    /// `A0` of the envelope the rows read (see `Keyed`).
    a0: f64,
    rows: Vec<(i64, f64, i64, usize, u128)>,
}

/// Per physical slot choice masks (index = domain Snap index plus one, 0 = None) for a domain partition. A forced
/// slot must take a choice from its mask and the prefix relaxation charges it with those choices only. An excluded
/// mask only filters enumeration; the relaxation keeps every choice there.
#[derive(Clone, Debug, Default)]
pub(crate) struct SlotRules {
    pub(crate) forced: [Option<Vec<bool>>; 5],
    pub(crate) excluded: [Option<Vec<bool>>; 5],
}

pub(crate) struct JointBounds {
    a: Vec<i64>,
    w: Vec<Vec<i64>>,
    lead: Vec<Vec<i64>>,
    profile: Vec<usize>,
    /// Position-mean gains (every position of a row holds the row's mean): the gains of every node bound.
    gains: Vec<Vec<[f64; 5]>>,
    /// The per-position gains of the pool-wide envelope, for the per-order caps of complete teams (empty in carrier
    /// levels).
    order_gains: Vec<Vec<[f64; 5]>>,
    a0: f64,
    global: f64,
    eps: f64,
    points: Option<PointBound>,
    fine: Option<JointFineBounds>,
    correlation_scales: [f64; 3],
    tails: Option<TailTables>,
    composition: Option<composition::CompositionTables>,
    /// Bound modules both traversals consult after the built-in bounds (see `NodeBound`).
    modules: Vec<Box<dyn NodeBound>>,
    gekisou: bool,
    class_search: bool,
    class_resource_caps: bool,
    prefix_resource: Option<prefix_resource::PrefixResourceTables>,
    prefix_character: Option<prefix_character::PrefixCharacterTables>,
    /// Optional per-slot choice rules of a domain partition; see `SlotRules`.
    rules: Option<SlotRules>,
    /// Table form of the free-slot relaxation (identical values); None keeps the member/Snap scan.
    relax_tables: Option<relax_tables::RelaxTables>,
    /// Table form of each forced slot's term for the current rules.
    forced_tables: [Option<relax_tables::ForcedTable>; 5],
    /// (pool member, choice index: 0=None, j+1=domain.snaps[j]). Ordering only.
    pub(crate) choices: Vec<(usize, usize)>,
    /// With a Gekisou combo range: the cheap bounds of decks with few combo carriers (see `CarrierLevels`).
    carrier_levels: Option<CarrierLevels>,
    /// With carrier keys: the node bounds split by the carriers of the slots to fill (see `carrier_split`).
    carrier_split: Option<carrier_split::CarrierSplit>,
}

/// The cheap bounds of decks with at most `n < 5` Gekisou combo carriers (a slot whose member and Snap bring combo
/// bonus windows). Such a deck's Gekisou combo factor is at most the one the `n` largest members' bonuses build, so
/// every level is the whole cheap bound family recompiled from a linear envelope at most the pool-wide one. A node
/// with `c` carriers placed and `r` slots to fill covers decks with at most `c + r` carriers.
struct CarrierLevels {
    /// `carrier[m][choice]` (choice: domain Snap index plus one, 0 = None).
    carrier: Vec<Vec<bool>>,
    /// `at[n]` for `n < 5`; None reads the next level, finally the pool-wide bounds.
    at: Vec<Option<Box<JointBounds>>>,
    /// The envelopes keyed by the carriers a prefix placed (see `CarrierKeys`).
    keys: Option<std::rc::Rc<CarrierKeys>>,
}

/// What a node's placed Gekisou combo carriers tell its completions (see `CarrierKeys`): their envelope's `A0` and
/// the gains of the placed slots by slot, position-mean (`placed`, for node bounds) and by position (`order_placed`,
/// for the per-order caps of a complete team). The slots to fill keep the gains of the bounds that read it.
pub(crate) struct Keyed {
    a0: f64,
    placed: [[f64; 5]; 5],
    order_placed: [[f64; 5]; 5],
}

fn unavailable(message: &str) -> Error {
    Error::Domain(message.into())
}
fn add_up(a: f64, b: f64) -> f64 {
    (a + b).next_up()
}
/// The pairs no other pair dominates in both parts (ties keep one).
fn pareto_pairs(mut pairs: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    pairs.sort_unstable_by(|a, b| b.cmp(a));
    let mut out: Vec<(i64, i64)> = Vec::new();
    for pair in pairs {
        if out.last().is_none_or(|last| pair.1 > last.1) {
            out.push(pair);
        }
    }
    out
}
/// Every gain row replaced by its rounded-up position mean (see the module documentation).
fn mean_table(gains: &[Vec<[f64; 5]>]) -> Vec<Vec<[f64; 5]>> {
    gains.iter().map(|rows| rows.iter().map(super::uniform::mean_row).collect()).collect()
}

impl JointBounds {
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn describe(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        deck: &PhysicalDeck,
        positions: &[usize; 5],
    ) -> serde_json::Value {
        let (upper, power) = self.upper(pool, domain, deck, 5, positions);
        let gains: Vec<_> = (0..5)
            .map(|slot| {
                let choice =
                    deck.snaps[slot].map_or(0, |snap| domain.snaps().iter().position(|&s| s == snap).unwrap() + 1);
                self.gains[deck.members[slot]][choice][positions[slot]]
            })
            .collect();
        serde_json::json!({"payoffUpper":upper.to_string(),"powerUpper":power,"baseCoefficient":self.a0,
            "marginDiagnostic":self.fine.as_ref().map(|fine| fine.margin_diagnostic(power, deck.members,
                deck.snaps.map(|s| s.map_or(0, |snap| domain.snaps().iter().position(|&v| v == snap).expect("compiled Snap") + 1)), positions, None)),
            "finePayoffUpper":self.fine_upper(domain,deck,power,positions,&mut JointScratch::default()).map(|v|v.to_string()),"globalCoefficient":self.global,"relativeMargin":self.eps,"pairGains":gains,"positions":positions})
    }
    pub(crate) fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
        input: Option<&EventPayoffInput>,
        simulation: &SimulationInput,
    ) -> Result<Self, Error> {
        if !domain.is_feasible() {
            return Err(unavailable("empty legal domain"));
        }
        if !matches!(request.objective.inner(), Objective::LiveScore { .. })
            || !matches!(
                metric,
                Metric::Score
                    | Metric::ClientEventPoints { .. }
                    | Metric::ClientChallengePoints { .. }
                    | Metric::ScoreAtLeast { .. }
                    | Metric::CappedScore { .. }
                    | Metric::ScoreAndLifeAtLeast { .. }
            )
        {
            return Err(unavailable("joint bounds require a Live score, score target, or point objective"));
        }
        if simulation.music_length_ms.is_some()
            || simulation.score_music_length_ms.is_some()
            || matches!(request.objective.inner(), Objective::LiveScore {play:super::PlayInput::Stream {stream,..},..} if stream.delta_times.is_some())
        {
            return Err(unavailable("explicit duration/delta clocks require exhaustive fallback"));
        }
        let n = pool.members.len();
        if n.saturating_mul(n.saturating_add(domain.snaps().len() + 1)) > 1_000_000 {
            return Err(unavailable("joint preparation exceeds bounded table capacity"));
        }
        let (song, event, _) = super::objective_song(pool, &request.objective)?;
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
        let setup = super::full_setup(pool, &request.objective)?.ok_or_else(|| unavailable("missing Live setup"))?;
        let envelope = SnapLive::new(pool, &t, &allowed, &setup)?;
        let (a0, global, eps, order_gains) = envelope.joint_envelope();
        let levels = if setup.gk.is_some() { envelope.joint_carrier_levels() } else { None };
        let keys = levels.as_ref().and_then(|_| envelope.carrier_keys());
        if ![a0, global, eps].iter().all(|v| v.is_finite() && *v >= 0.0)
            || order_gains.iter().flatten().flatten().any(|g| !g.is_finite() || *g < 0.0)
        {
            return Err(unavailable("nonfinite/negative score relaxation"));
        }
        let gains = mean_table(&order_gains);
        let points = match metric {
            Metric::ScoreAtLeast { threshold } | Metric::ScoreAndLifeAtLeast { threshold, .. } => Some(
                PointBound::score_target(pool.members.len(), domain.snaps().len(), ScoreTarget::AtLeast(*threshold)),
            ),
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
                )?)
            }
            _ => None,
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
            a0,
            global,
            eps,
            points,
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
        let mut b = Self {
            a: self.a.clone(),
            w: self.w.clone(),
            lead: self.lead.clone(),
            profile: self.profile.clone(),
            gains: mean_table(&gains),
            order_gains: gains,
            a0,
            global,
            eps: self.eps,
            points: self.points.clone(),
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
            for i in (0..n).rev() {
                let (m, choice) = self.choices[i];
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
            TailTables { power, gain, bonus }
        });
        self.relax_tables = relax_tables::RelaxTables::compile(self, pool, domain);
        if self.gekisou && self.points.is_none() {
            (self.prefix_resource, self.prefix_character) = prefix_character::compile_prefix_tables(self, pool, domain);
        }
    }

    /// The cheap bounds of every deck with at most `n` Gekisou combo carriers (the pool-wide ones from 5 on).
    pub(crate) fn carrier_level(&self, n: usize) -> &JointBounds {
        match &self.carrier_levels {
            Some(levels) => levels.at.iter().skip(n).flatten().next().map_or(self, |b| b),
            None => self,
        }
    }

    /// Whether a member and choice of the domain is a Gekisou combo carrier (false without carrier levels).
    pub(crate) fn is_carrier(&self, member: usize, choice: usize) -> bool {
        self.carrier_levels.as_ref().is_some_and(|l| l.carrier[member][choice])
    }

    /// The choice of each of the first `depth` search slots of a prefix, by slot (0 = None, `j + 1` = Snap `j`).
    pub(crate) fn prefix_choices(domain: &CandidateDomain, p: &PhysicalDeck, depth: usize) -> [usize; 5] {
        let mut choices = [0; 5];
        for &slot in &SLOTS[..depth] {
            choices[slot] =
                p.snaps[slot].map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
        }
        choices
    }

    /// The Gekisou combo carriers among the first `depth` search slots of a prefix (0 without carrier levels).
    pub(crate) fn carriers_placed(&self, p: &PhysicalDeck, depth: usize, choices: &[usize; 5]) -> usize {
        SLOTS[..depth].iter().filter(|&&slot| self.is_carrier(p.members[slot], choices[slot])).count()
    }

    /// The keyed envelope of the completions of a prefix with at most `r` more carriers among its `free` slots to
    /// fill (see `CarrierKeys`).
    pub(crate) fn keyed(
        &self,
        p: &PhysicalDeck,
        depth: usize,
        choices: &[usize; 5],
        r: usize,
        free: usize,
    ) -> Option<Keyed> {
        let keys = self.carrier_levels.as_ref()?.keys.as_ref()?;
        let mut ids = [0u16; 5];
        let mut n = 0;
        for &slot in &SLOTS[..depth] {
            if let Some(id) = keys.list(p.members[slot], choices[slot]) {
                ids[n] = id;
                n += 1;
            }
        }
        let env = keys.envelope(&ids[..n], r);
        let mut order_placed = [[0f64; 5]; 5];
        for &slot in &SLOTS[..depth] {
            order_placed[slot] = keys.gains(&env, p.members[slot], choices[slot]);
        }
        let placed = order_placed.map(|g| super::uniform::mean_row(&g));
        let a0 = keys.a0(&env, SLOTS[..depth].iter().map(|&slot| (p.members[slot], choices[slot])), free);
        Some(Keyed { a0, placed, order_placed })
    }

    /// The number of carrier levels compiled apart from the pool-wide bounds.
    pub(crate) fn carrier_level_count(&self) -> usize {
        self.carrier_levels.as_ref().map_or(0, |l| l.at.iter().flatten().count())
    }

    /// Restrict the choices of physical slots for the following traversal (`None` lifts the rules).
    pub(crate) fn set_rules(&mut self, pool: &Pool, domain: &CandidateDomain, rules: Option<SlotRules>) {
        if let Some(levels) = &mut self.carrier_levels {
            for b in levels.at.iter_mut().flatten() {
                b.set_rules(pool, domain, rules.clone());
            }
        }
        self.forced_tables = Default::default();
        if let (Some(r), Some(t)) = (&rules, &self.relax_tables) {
            for slot in 0..5 {
                if let Some(mask) = &r.forced[slot] {
                    self.forced_tables[slot] = Some(relax_tables::ForcedTable::compile(self, pool, domain, t, mask));
                }
            }
        }
        self.rules = rules;
    }
    /// Whether the rules allow `choice` (domain Snap index plus one, 0 = None) in physical `slot`.
    pub(crate) fn allows(&self, slot: usize, choice: usize) -> bool {
        self.rules.as_ref().is_none_or(|r| {
            r.forced[slot].as_ref().is_none_or(|m| m[choice]) && r.excluded[slot].as_ref().is_none_or(|m| !m[choice])
        })
    }

    pub(crate) fn is_pt(&self) -> bool {
        self.points.as_ref().is_some_and(|pt| pt.target.is_none())
    }

    /// The per-order payoff steps of a score target or of points stepped by the local score, with this envelope's
    /// global score cap (`payoff_cap_from` with every gain at the global coefficient); None for score payoffs, capped
    /// scores and multiplayer rewards without score tiers.
    pub(crate) fn score_steps(&self) -> Option<super::deck_payoff::ScoreSteps> {
        use super::deck_payoff::Step;
        let pt = self.points.as_ref()?;
        let steps = match (pt.target, &pt.score_tiers) {
            (Some(ScoreTarget::AtLeast(threshold)), _) => {
                vec![(i128::MIN, Step::Target(false)), (i128::from(threshold), Step::Target(true))]
            }
            (Some(ScoreTarget::Capped(_)), _) | (None, None) => return None,
            // `multiplier_at`: the prefix maximum of the tiers at or below a score.
            (None, Some(tiers)) => {
                let mut best = 0;
                tiers
                    .iter()
                    .map(|&(score, multiplier)| {
                        best = best.max(multiplier);
                        (i128::from(score), Step::Points(best))
                    })
                    .collect()
            }
        };
        Some(super::deck_payoff::ScoreSteps::new(self.global, self.eps, steps))
    }

    /// A bounded terminal objective may close its remaining Snap assignments once a power-ranked proposal
    /// actually attains the whole composition's primary cap. The same proof covers PT and score targets.
    pub(crate) fn has_terminal_payoff_cap(&self) -> bool {
        self.points.is_some()
    }

    pub(crate) fn prefers_compositions(&self) -> bool {
        !self.gekisou || self.class_search
    }

    pub(crate) fn uses_class_search(&self) -> bool {
        self.class_search
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn enable_class_search(
        &mut self,
        pool: &Pool,
        domain: &CandidateDomain,
        resource_caps: bool,
    ) -> Result<(), Error> {
        if !self.gekisou || self.points.is_some() || self.fine.is_none() {
            return Err(unavailable("class schedule requires a compiled Gekisou score objective"));
        }
        self.class_search = true;
        self.class_resource_caps = resource_caps;
        self.composition = composition::CompositionTables::compile(self, pool, domain);
        Ok(())
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn has_class_bounds(&self) -> bool {
        self.gekisou && self.points.is_none() && self.fine.is_some()
    }

    /// Every deck containing member m earns at most this many PT in every performance order.
    /// Distinct-character bonus maxima and distinct-Snap maxima are independent,
    /// so ignoring their pairing, leader and required-member conflicts is optimistic.
    pub(crate) fn member_pt_caps(&self, pool: &Pool, domain: &CandidateDomain) -> Option<Vec<(usize, i128)>> {
        let pt = self.points.as_ref()?;
        if pt.target.is_some() {
            return None;
        }
        let mut chars = HashMap::<i64, i64>::new();
        for &m in domain.members() {
            let v = chars.entry(pool.members[m].character_id).or_default();
            *v = (*v).max(pt.member[m]);
        }
        let mut snaps = pt.snap.clone();
        snaps.sort_unstable_by(|a, b| b.cmp(a));
        let snap_bonus: i64 = snaps.iter().take(5).sum();
        Some(
            domain
                .members()
                .iter()
                .map(|&m| {
                    let mut rest: Vec<_> =
                        chars.iter().filter(|(c, _)| **c != pool.members[m].character_id).map(|(_, b)| *b).collect();
                    rest.sort_unstable_by(|a, b| b.cmp(a));
                    let bonus = pt.member[m] + snap_bonus + rest.iter().take(4).sum::<i64>();
                    (m, ((bonus + 10000) * pt.multiplier / 10000) as i128)
                })
                .collect(),
        )
    }

    /// Optimistic total event bonus for a legal prefix, including all still-required
    /// members. Used to seed from the maximum-bonus regime; score is evaluated exactly.
    pub(crate) fn bonus_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> Option<i64> {
        let pt = self.points.as_ref()?;
        if pt.target.is_some() {
            return None;
        }
        let mut chars = HashSet::new();
        let mut used = HashSet::new();
        let mut sum = 0;
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            chars.insert(pool.members[m].character_id);
            sum += pt.member[m];
            if let Some(s) = p.snaps[slot] {
                used.insert(s);
                sum += pt.snap[domain.snaps().iter().position(|&v| v == s).expect("compiled Snap")];
            }
        }
        let mut count = 0;
        for &m in domain.required() {
            if !SLOTS[..depth].iter().any(|&slot| p.members[slot] == m) {
                chars.insert(pool.members[m].character_id);
                sum += pt.member[m];
                count += 1;
            }
        }
        let mut remaining = HashMap::<i64, i64>::new();
        for &m in domain.members() {
            let c = pool.members[m].character_id;
            if !chars.contains(&c) {
                let row = remaining.entry(c).or_default();
                *row = (*row).max(pt.member[m]);
            }
        }
        let mut bonuses: Vec<_> = remaining.into_values().collect();
        bonuses.sort_unstable_by(|a, b| b.cmp(a));
        sum += bonuses.iter().take((5 - depth).saturating_sub(count)).sum::<i64>();
        let mut snaps: Vec<_> =
            domain.snaps().iter().enumerate().filter(|(_, s)| !used.contains(*s)).map(|(j, _)| pt.snap[j]).collect();
        snaps.sort_unstable_by(|a, b| b.cmp(a));
        sum += snaps.iter().take(5 - depth).sum::<i64>();
        Some(sum)
    }

    pub(crate) fn qualifying_pt_domain(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        numerator: i128,
        mass: u128,
    ) -> Result<Option<CandidateDomain>, Error> {
        let Some(caps) = self.member_pt_caps(pool, domain) else {
            return Ok(None);
        };
        let mass = i128::try_from(mass).map_err(|_| unavailable("PT domain mass overflow"))?;
        let mut keep = HashSet::new();
        for (m, cap) in caps {
            if cap.checked_mul(mass).ok_or_else(|| unavailable("PT domain payoff overflow"))? >= numerator {
                keep.insert(m);
            }
        }
        Ok((keep.len() < domain.members().len()).then(|| domain.retain_proven_members(pool, &keep)))
    }

    /// Optimistic (payoff, power) for every legal completion of this prefix, read at `positions`: with the
    /// position-mean gains, at any positions, a bound of the mean payoff over the performance orders.
    /// Remaining characters are distinct; their maxima may reuse a Snap or choose different
    /// members for power/gain/PT. Those relaxations only enlarge the completion set.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
    ) -> (i128, i64) {
        self.upper_keyed(pool, domain, p, depth, positions, None)
    }

    /// The cheap bound, with the envelope of a node's placed carriers, if any.
    fn upper_keyed(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
        keyed: Option<&Keyed>,
    ) -> (i128, i64) {
        let (power, gain, bonus) = self.relax(pool, domain, p, depth, &SLOTS[depth..], positions, keyed);
        (self.payoff_cap_from(keyed.map_or(self.a0, |k| k.a0), power, gain, bonus), power)
    }

    fn payoff_cap(&self, power: i64, gain: f64, bonus: i64) -> i128 {
        self.payoff_cap_from(self.a0, power, gain, bonus)
    }

    /// `payoff_cap` with the `A0` of some envelope at most this one's: a bound of the mean payoff over the orders when
    /// `gain` sums position-mean gains.
    fn payoff_cap_from(&self, a0: f64, power: i64, gain: f64, bonus: i64) -> i128 {
        let score_cap = ((power as f64) * add_up(a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
        self.points.as_ref().map_or(score_cap, |pt| pt.mean_payoff(bonus, score_cap))
    }

    #[allow(clippy::too_many_arguments)]
    fn relax(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        remaining_slots: &[usize],
        positions: &[usize; 5],
        keyed: Option<&Keyed>,
    ) -> (i64, f64, i64) {
        let profile = self.profile[p.members[2]];
        let forced: Vec<(usize, &[bool])> = remaining_slots
            .iter()
            .filter_map(|&slot| Some((slot, self.rules.as_ref()?.forced[slot].as_deref()?)))
            .collect();
        let free: Vec<usize> =
            remaining_slots.iter().copied().filter(|&s| !forced.iter().any(|&(slot, _)| slot == s)).collect();
        let remaining_slots = &free[..];
        let mut characters = HashSet::new();
        let mut used = HashSet::new();
        let (mut power, mut gain, mut bonus) = (0i64, 0.0, 0i64);
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            characters.insert(pool.members[m].character_id);
            let j = p.snaps[slot].map(|s| {
                used.insert(s);
                domain.snaps().iter().position(|&v| v == s).expect("compiled snap")
            });
            power += self.a[m] + self.lead[profile][m] + j.map_or(0, |j| self.w[m][j]);
            let placed = keyed
                .map_or(self.gains[m][j.map_or(0, |j| j + 1)][positions[slot]], |k| k.placed[slot][positions[slot]]);
            gain = add_up(gain, placed);
            if let Some(pt) = &self.points {
                bonus += pt.member[m] + j.map_or(0, |j| pt.snap[j]);
            }
        }
        let taken = self.relax_tables.as_ref().map(|t| t.taken(pool, domain, p, depth));
        let (free_power, free_gain, free_bonus) = match (&self.relax_tables, &taken) {
            (Some(t), Some((taken_characters, taken_snaps))) => {
                t.free_part(self, profile, remaining_slots, positions, taken_characters, taken_snaps)
            }
            _ => self.scan_free(pool, domain, profile, &characters, &used, remaining_slots, positions),
        };
        power += free_power;
        gain = add_up(gain, free_gain);
        bonus += free_bonus;
        // Each forced slot takes an allowed, still unused choice with some unused-character member. Its maxima may
        // use different members and choices, and may share a character or Snap with the other remaining slots;
        // this only enlarges the completion set. Free slots stay relaxed to every choice.
        for (slot, mask) in forced {
            if let (Some(table), Some((taken_characters, taken_snaps))) = (&self.forced_tables[slot], &taken) {
                let (fp, fg, fb) =
                    table.best(profile, positions[slot], self.points.is_some(), taken_characters, taken_snaps);
                power += fp;
                gain = add_up(gain, fg);
                bonus += fb;
                continue;
            }
            let (mut fp, mut fg, mut fb) = (0i64, 0.0f64, 0i64);
            for &m in domain.members() {
                if characters.contains(&pool.members[m].character_id) {
                    continue;
                }
                for choice in (0..mask.len()).filter(|&c| mask[c]) {
                    if choice > 0 && used.contains(&domain.snaps()[choice - 1]) {
                        continue;
                    }
                    fp =
                        fp.max(self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] });
                    fg = fg.max(self.gains[m][choice][positions[slot]]);
                    if let Some(pt) = &self.points {
                        fb = fb.max(pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
                    }
                }
            }
            power += fp;
            gain = add_up(gain, fg);
            bonus += fb;
        }
        (power, gain, bonus)
    }

    /// Diagnostics: the table relaxation must equal the member/Snap scan bit for bit.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn check_relax_tables(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        remaining_slots: &[usize],
        positions: &[usize; 5],
    ) -> Result<(), Error> {
        let Some(t) = &self.relax_tables else { return Ok(()) };
        let profile = self.profile[p.members[2]];
        let characters: HashSet<i64> =
            SLOTS[..depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let used: HashSet<usize> = SLOTS[..depth].iter().filter_map(|&s| p.snaps[s]).collect();
        let (taken_characters, taken_snaps) = t.taken(pool, domain, p, depth);
        let table = t.free_part(self, profile, remaining_slots, positions, &taken_characters, &taken_snaps);
        let scan = self.scan_free(pool, domain, profile, &characters, &used, remaining_slots, positions);
        if table.0 != scan.0 || table.1.to_bits() != scan.1.to_bits() || table.2 != scan.2 {
            return Err(Error::Game(format!("relax table {table:?} differs from scan {scan:?}")));
        }
        Ok(())
    }

    /// The free-slot part of the cheap relaxation by scanning every member/Snap pair.
    #[allow(clippy::too_many_arguments)]
    fn scan_free(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        profile: usize,
        characters: &HashSet<i64>,
        used: &HashSet<usize>,
        remaining_slots: &[usize],
        positions: &[usize; 5],
    ) -> (i64, f64, i64) {
        let (mut power, mut bonus) = (0i64, 0i64);
        let mut remaining = HashMap::<i64, (i64, f64, i64)>::new();
        // A second relaxation accounts for scarce, unique Snap resources: choose member-only
        // maxima by character, then at most one optimistic increment from each available Snap.
        let mut bases = HashMap::<i64, (i64, f64, i64)>::new();
        let mut snap_power = vec![0i64; domain.snaps().len()];
        let mut snap_gain = vec![0.0f64; domain.snaps().len()];
        let mut snap_bonus = vec![0i64; domain.snaps().len()];
        if !remaining_slots.is_empty() {
            for &m in domain.members() {
                let c = pool.members[m].character_id;
                if characters.contains(&c) {
                    continue;
                }
                let remaining_gain = |choice: usize| {
                    remaining_slots.iter().map(|&slot| self.gains[m][choice][positions[slot]]).fold(0.0, f64::max)
                };
                let mut best = (
                    self.a[m] + self.lead[profile][m],
                    remaining_gain(0),
                    self.points.as_ref().map_or(0, |pt| pt.member[m]),
                );
                let base = bases.entry(c).or_insert(best);
                base.0 = base.0.max(best.0);
                base.1 = base.1.max(best.1);
                base.2 = base.2.max(best.2);
                for (j, &s) in domain.snaps().iter().enumerate() {
                    if used.contains(&s) {
                        continue;
                    }
                    snap_power[j] = snap_power[j].max(self.w[m][j]);
                    let delta = remaining_slots
                        .iter()
                        .map(|&slot| {
                            (self.gains[m][j + 1][positions[slot]] - self.gains[m][0][positions[slot]]).next_up()
                        })
                        .fold(0.0, f64::max);
                    snap_gain[j] = snap_gain[j].max(delta);
                    if let Some(pt) = &self.points {
                        snap_bonus[j] = pt.snap[j].max(0);
                    }
                    best.0 = best.0.max(self.a[m] + self.lead[profile][m] + self.w[m][j]);
                    best.1 = best.1.max(remaining_gain(j + 1));
                    if let Some(pt) = &self.points {
                        best.2 = best.2.max(pt.member[m] + pt.snap[j]);
                    }
                }
                let row = remaining.entry(c).or_insert(best);
                row.0 = row.0.max(best.0);
                row.1 = row.1.max(best.1);
                row.2 = row.2.max(best.2);
            }
        }
        let mut powers: Vec<_> = remaining.values().map(|r| r.0).collect();
        powers.sort_unstable_by(|a, b| b.cmp(a));
        let mut gains: Vec<_> = remaining.values().map(|r| r.1).collect();
        gains.sort_by(|a, b| b.total_cmp(a));
        let mut bonuses: Vec<_> = remaining.values().map(|r| r.2).collect();
        bonuses.sort_unstable_by(|a, b| b.cmp(a));
        let take = remaining_slots.len();
        let top_int = |mut v: Vec<i64>| {
            v.sort_unstable_by(|a, b| b.cmp(a));
            v.iter().take(take).sum::<i64>()
        };
        let top_float = |mut v: Vec<f64>| {
            v.sort_by(|a, b| b.total_cmp(a));
            v.iter().take(take).fold(0.0, |sum, &x| add_up(sum, x))
        };
        power += powers
            .iter()
            .take(take)
            .sum::<i64>()
            .min(top_int(bases.values().map(|r| r.0).collect()) + top_int(snap_power));
        let gain_by_character = gains.iter().take(take).fold(0.0, |sum, &x| add_up(sum, x));
        let gain_by_snaps = add_up(top_float(bases.values().map(|r| r.1).collect()), top_float(snap_gain));
        // The caller adds this part to the prefix with the one outward rounding of the original relaxation.
        let gain = gain_by_character.min(gain_by_snaps);
        bonus += bonuses
            .iter()
            .take(take)
            .sum::<i64>()
            .min(top_int(bases.values().map(|r| r.2).collect()) + top_int(snap_bonus));
        (power, gain, bonus)
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn tail_state(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
    ) -> Option<TailState> {
        self.tail_state_keyed(pool, domain, p, depth, orders, None)
    }

    /// `tail_state` with the envelope of the children's placed carriers, if any.
    pub(crate) fn tail_state_keyed(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        keyed: Option<&Keyed>,
    ) -> Option<TailState> {
        if self.tails.is_none() || !(1..5).contains(&depth) {
            return None;
        }
        Some(TailState {
            profile: self.profile[p.members[2]],
            a0: keyed.map_or(self.a0, |k| k.a0),
            rows: orders
                .iter()
                .map(|(positions, weight)| {
                    let (power, gain, bonus) =
                        self.relax(pool, domain, p, depth, &SLOTS[depth + 1..], positions, keyed);
                    (power, gain, bonus, positions[SLOTS[depth]], *weight)
                })
                .collect(),
        })
    }

    /// Bounds every legal child at or after this choice offset, without visiting them.
    /// The next pair uses suffix maxima; the other remaining slots use an independent
    /// character/resource relaxation. Overlap between those sets is deliberately allowed.
    pub(crate) fn tail_upper(&self, state: &TailState, offset: usize) -> Result<(i128, i64), Error> {
        let t = self.tails.as_ref().expect("compiled tail table");
        let mut total = 0i128;
        let mut power = 0;
        for &(p, g, b, pos, weight) in &state.rows {
            power = p + t.power[state.profile][offset];
            let payoff = self.payoff_cap_from(state.a0, power, add_up(g, t.gain[pos][offset]), b + t.bonus[offset]);
            total = total
                .checked_add(
                    payoff
                        .checked_mul(i128::try_from(weight).map_err(|_| unavailable("bound mass overflow"))?)
                        .ok_or_else(|| unavailable("tail bound product overflow"))?,
                )
                .ok_or_else(|| unavailable("tail bound sum overflow"))?;
        }
        Ok((total, power))
    }

    /// Constant-cost check for one next pair using its parent's already prepared residual.
    /// Reusing the parent's available resources in the residual is optimistic; surviving
    /// children subsequently receive the stronger resource-aware prefix check.
    pub(crate) fn pair_upper(&self, state: &TailState, member: usize, choice: usize) -> Result<(i128, i64), Error> {
        let added_power = self.a[member]
            + self.lead[state.profile][member]
            + if choice == 0 { 0 } else { self.w[member][choice - 1] };
        let added_bonus =
            self.points.as_ref().map_or(0, |pt| pt.member[member] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
        let mut total = 0i128;
        let mut power = 0;
        for &(p, g, b, pos, weight) in &state.rows {
            power = p + added_power;
            let payoff =
                self.payoff_cap_from(state.a0, power, add_up(g, self.gains[member][choice][pos]), b + added_bonus);
            total = total
                .checked_add(
                    payoff
                        .checked_mul(i128::try_from(weight).map_err(|_| unavailable("bound mass overflow"))?)
                        .ok_or_else(|| unavailable("pair bound product overflow"))?,
                )
                .ok_or_else(|| unavailable("pair bound sum overflow"))?;
        }
        Ok((total, power))
    }

    /// If every remaining character must be used, maximize the residual power as
    /// a small exact bipartite assignment. Each character/Snap edge may choose its
    /// own best member; ignoring required-card constraints only raises this cap.
    /// The matching's tie policy is irrelevant: only its optimum VALUE is used.
    pub(crate) fn assignment_power_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> Option<i64> {
        if !(1..4).contains(&depth) {
            return None;
        }
        let profile = self.profile[p.members[2]];
        let mut chars = HashSet::new();
        let mut used = HashSet::new();
        let mut fixed = 0;
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            chars.insert(pool.members[m].character_id);
            let extra = p.snaps[slot].map_or(0, |s| {
                used.insert(s);
                self.w[m][domain.snaps().iter().position(|&v| v == s).expect("compiled Snap")]
            });
            fixed += self.a[m] + self.lead[profile][m] + extra;
        }
        let mut groups = std::collections::BTreeMap::<i64, Vec<usize>>::new();
        for &m in domain.members() {
            let c = pool.members[m].character_id;
            if !chars.contains(&c) {
                groups.entry(c).or_default().push(m);
            }
        }
        if groups.len() != 5 - depth {
            return None;
        }
        let snaps: Vec<_> =
            domain.snaps().iter().enumerate().filter(|(_, s)| !used.contains(*s)).map(|(j, _)| j).collect();
        let mut weights: [Vec<i64>; 5] = std::array::from_fn(|_| vec![0; snaps.len()]);
        for (row, members) in groups.values().enumerate() {
            let base = members.iter().map(|&m| self.a[m] + self.lead[profile][m]).max().expect("character members");
            fixed += base;
            for (col, &s) in snaps.iter().enumerate() {
                weights[row][col] = members
                    .iter()
                    .map(|&m| self.a[m] + self.lead[profile][m] + self.w[m][s])
                    .max()
                    .expect("character members")
                    - base;
            }
        }
        let (extra, _) = super::matching::best_assignment(weights.each_ref().map(|r| r.as_slice()));
        Some(fixed + extra)
    }

    /// Preparation probe only chooses whether to spend time on a valid extra bound.
    /// Skipping it changes neither the candidate pool nor the completion certificate.
    pub(crate) fn correlation_worthwhile(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        orders: &[([usize; 5], u128)],
    ) -> Result<bool, Error> {
        let mut probes = 0;
        for &(m, choice) in &self.choices {
            if domain.leader().is_some_and(|l| l != m) {
                continue;
            }
            let mut p = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
            p.members[2] = m;
            p.snaps[2] = if choice == 0 { None } else { Some(domain.snaps()[choice - 1]) };
            let cheap = self.expected_upper(pool, domain, &p, 1, orders)?.0;
            let joint = self.correlated_expected_upper(pool, domain, &p, 1, orders)?;
            if (joint as f64) < (cheap as f64) * 0.97 {
                return Ok(true);
            }
            probes += 1;
            if probes == 16 {
                break;
            }
        }
        Ok(false)
    }

    /// Power and skill from a remaining pair are coupled before maximizing. For any r>0,
    /// P*A <= (P+r*A)^2/(4*r). Distinct-character maxima relax positions and Snap reuse,
    /// but cannot combine one card's power with another card's skill inside a weighted term.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn correlated_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
    ) -> i128 {
        self.correlated_upper_keyed(pool, domain, p, depth, positions, None)
    }

    /// `correlated_upper` with the envelope of a node's placed carriers, if any.
    fn correlated_upper_keyed(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
        keyed: Option<&Keyed>,
    ) -> i128 {
        if let Some(cap) =
            self.prefix_character.as_ref().and_then(|t| t.upper(self, pool, domain, p, depth, positions, keyed))
        {
            return cap;
        }
        let profile = self.profile[p.members[2]];
        let mut characters = HashSet::new();
        let mut used = HashSet::new();
        let a0 = keyed.map_or(self.a0, |k| k.a0);
        let mut totals = self.correlation_scales.map(|r| (r * a0).next_up());
        let mut bonus = 0;
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            characters.insert(pool.members[m].character_id);
            let j = p.snaps[slot].map(|s| {
                used.insert(s);
                domain.snaps().iter().position(|&v| v == s).expect("compiled Snap")
            });
            let power = self.a[m] + self.lead[profile][m] + j.map_or(0, |j| self.w[m][j]);
            let gain = keyed
                .map_or(self.gains[m][j.map_or(0, |j| j + 1)][positions[slot]], |k| k.placed[slot][positions[slot]]);
            for (i, &r) in self.correlation_scales.iter().enumerate() {
                totals[i] = add_up(totals[i], add_up(power as f64, (r * gain).next_up()));
            }
            if let Some(pt) = &self.points {
                bonus += pt.member[m] + j.map_or(0, |j| pt.snap[j]);
            }
        }
        let mut remaining = HashMap::<i64, ([f64; 3], i64)>::new();
        for &m in domain.members() {
            let c = pool.members[m].character_id;
            if characters.contains(&c) {
                continue;
            }
            let row = remaining.entry(c).or_insert(([0.0; 3], 0));
            for choice in 0..=domain.snaps().len() {
                if choice > 0 && used.contains(&domain.snaps()[choice - 1]) {
                    continue;
                }
                let power = self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
                let gain =
                    SLOTS[depth..].iter().map(|&slot| self.gains[m][choice][positions[slot]]).fold(0.0, f64::max);
                for (i, &r) in self.correlation_scales.iter().enumerate() {
                    row.0[i] = row.0[i].max(add_up(power as f64, (r * gain).next_up()));
                }
                if let Some(pt) = &self.points {
                    row.1 = row.1.max(pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
                }
            }
        }
        for (i, total) in totals.iter_mut().enumerate() {
            let mut values: Vec<_> = remaining.values().map(|r| r.0[i]).collect();
            values.sort_by(|a, b| b.total_cmp(a));
            for &v in values.iter().take(5 - depth) {
                *total = add_up(*total, v);
            }
        }
        let mut bonuses: Vec<_> = remaining.values().map(|r| r.1).collect();
        bonuses.sort_unstable_by(|a, b| b.cmp(a));
        bonus += bonuses.iter().take(5 - depth).sum::<i64>();
        let score_cap = totals
            .iter()
            .zip(self.correlation_scales)
            .map(|(&w, r)| {
                (((w * w).next_up() / (4.0 * r)).next_up() * (1.0 + self.eps).next_up()).next_up().ceil() as i128
            })
            .min()
            .expect("three scales");
        self.points.as_ref().map_or(score_cap, |pt| pt.mean_payoff(bonus, score_cap))
    }

    pub(crate) fn correlated_expected_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
    ) -> Result<i128, Error> {
        self.correlated_expected_upper_keyed(pool, domain, p, depth, orders, None)
    }

    /// `correlated_expected_upper` with the envelope of a node's placed carriers, if any.
    pub(crate) fn correlated_expected_upper_keyed(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        keyed: Option<&Keyed>,
    ) -> Result<i128, Error> {
        let mut total = 0i128;
        for (positions, weight) in orders {
            let cap = self.correlated_upper_keyed(pool, domain, p, depth, positions, keyed);
            total = total
                .checked_add(
                    cap.checked_mul(i128::try_from(*weight).map_err(|_| unavailable("bound mass overflow"))?)
                        .ok_or_else(|| unavailable("correlated bound product overflow"))?,
                )
                .ok_or_else(|| unavailable("correlated bound sum overflow"))?;
        }
        Ok(total)
    }

    /// The bound modules of these bounds (see `NodeBound`).
    pub(crate) fn modules(&self) -> &[Box<dyn NodeBound>] {
        &self.modules
    }

    pub(crate) fn has_fine(&self) -> bool {
        self.fine.is_some()
    }

    /// The fine payoff cap of a complete deck in the performance order with these positions.
    pub(crate) fn fine_upper(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        positions: &[usize; 5],
        scratch: &mut JointScratch,
    ) -> Option<i128> {
        let fine = self.fine.as_ref()?;
        let choices = Self::prefix_choices(domain, p, 5);
        let score_cap = fine.upper(power, p.members, choices, positions, scratch, None) as i128;
        Some(self.points.as_ref().map_or(score_cap, |pt| pt.order_payoff(self.bonus_of(p, &choices), score_cap)))
    }

    /// The event bonus of a complete deck (0 without a PT objective).
    fn bonus_of(&self, p: &PhysicalDeck, choices: &[usize; 5]) -> i64 {
        self.points.as_ref().map_or(0, |pt| {
            (0..5).map(|s| pt.member[p.members[s]] + if choices[s] == 0 { 0 } else { pt.snap[choices[s] - 1] }).sum()
        })
    }

    /// Diagnostics only: the fine cap's per-entry terms at one native order.
    /// Diagnostics only: the Gekisou combo bonus windows of a candidate's fine bound.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn fine_cb_windows(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        positions: &[usize; 5],
    ) -> Option<Vec<(usize, i64, i64, f64)>> {
        let choices =
            p.snaps.map(|s| s.map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1));
        Some(self.fine.as_ref()?.cb_windows(p.members, choices, positions))
    }

    /// Diagnostics only: a complete team's cheap per-order terms (base coefficient, each slot's gain, cap, margin)
    /// beside its fine cap with every slot, with none and without each slot, at one set of positions.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn cheap_fine_attribution(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        positions: &[usize; 5],
    ) -> serde_json::Value {
        let choices = Self::prefix_choices(domain, p, 5);
        let level = self.carrier_level(self.carriers_placed(p, 5, &choices));
        let keyed = self.keyed(p, 5, &choices, 0, 0);
        let gains: Vec<f64> = (0..5)
            .map(|slot| {
                keyed.as_ref().map_or(level.order_gains[p.members[slot]][choices[slot]][positions[slot]], |k| {
                    k.order_placed[slot][positions[slot]]
                })
            })
            .collect();
        let notes = self.carrier_levels.as_ref().and_then(|l| l.keys.as_ref()).map(|keys| {
            let ids: Vec<u16> = (0..5).filter_map(|slot| keys.list(p.members[slot], choices[slot])).collect();
            let env = keys.envelope(&ids, 0);
            let placed: Vec<_> = (0..5).map(|slot| (p.members[slot], choices[slot], positions[slot])).collect();
            let (coef, term, budget) = keys.note_terms(&env, &placed);
            let trace = self.fine.as_ref().map(|f| f.fine_trace(power, p.members, choices, positions, None).1);
            serde_json::json!({"coef":coef,"term":term,"budget":budget,"trace":trace})
        });
        serde_json::json!({"notes":notes,"keyed":keyed.is_some(),"a0":keyed.as_ref().map_or(level.a0, |k| k.a0),
            "levelA0":level.a0,"poolA0":self.a0,"gains":gains,"global":level.global,"eps":level.eps,
            "cheapCap":self.order_cheap_caps(domain, p, power, &[*positions])[0].to_string(),
            "fine":self.fine.as_ref().map(|f| f.slot_attribution(power, p.members, choices, positions))})
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn fine_trace(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        positions: &[usize; 5],
    ) -> Option<(i64, Vec<[f64; 8]>)> {
        let choices = Self::prefix_choices(domain, p, 5);
        Some(self.fine.as_ref()?.fine_trace(power, p.members, choices, positions, None))
    }

    pub(crate) fn raw_upper(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        positions: &[usize; 5],
    ) -> Option<i128> {
        let choices = Self::prefix_choices(domain, p, 5);
        let cap = self.fine.as_ref()?.raw_upper(power, p.members, choices, positions)?;
        Some(self.points.as_ref().map_or(cap, |pt| pt.order_payoff(self.bonus_of(p, &choices), cap)))
    }

    /// The cheap payoff cap of a complete team in each performance order (`orders` holds the positions of each), at
    /// its exact power: the envelope of its Gekisou combo carrier level, keyed by its carriers, read at the order's
    /// positions with the per-position gains. Their sum bounds the team's payoff numerator over the orders and is at
    /// most [`super::uniform::ORDERS`] times its position-mean cheap bound.
    pub(crate) fn order_cheap_caps(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        orders: &[[usize; 5]],
    ) -> Vec<i128> {
        let choices = Self::prefix_choices(domain, p, 5);
        let level = self.carrier_level(self.carriers_placed(p, 5, &choices));
        let keyed = self.keyed(p, 5, &choices, 0, 0);
        let a0 = keyed.as_ref().map_or(level.a0, |k| k.a0);
        let bonus = self.bonus_of(p, &choices);
        orders
            .iter()
            .map(|positions| {
                let mut gain = 0.0;
                for slot in 0..5 {
                    let g = keyed
                        .as_ref()
                        .map_or(level.order_gains[p.members[slot]][choices[slot]][positions[slot]], |k| {
                            k.order_placed[slot][positions[slot]]
                        });
                    gain = add_up(gain, g);
                }
                let score_cap =
                    ((power as f64) * add_up(a0, gain).min(level.global) * (1.0 + level.eps)).ceil() as i128;
                level.points.as_ref().map_or(score_cap, |pt| pt.order_payoff(bonus, score_cap))
            })
            .collect()
    }

    /// Lowers each per-order cap of a complete team (see [`JointBounds::order_cheap_caps`]) to its raw and fine caps
    /// at that order, when compiled.
    pub(crate) fn tighten_order_caps(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        orders: &[[usize; 5]],
        caps: &mut [i128],
        scratch: &mut JointScratch,
    ) {
        if self.fine.is_none() {
            return;
        }
        for (positions, cap) in orders.iter().zip(caps.iter_mut()) {
            if let Some(raw) = self.raw_upper(domain, p, power, positions) {
                *cap = (*cap).min(raw);
            }
            *cap = (*cap).min(self.fine_upper(domain, p, power, positions, scratch).expect("compiled fine bound"));
        }
    }

    /// [`JointBounds::tighten_order_caps`] that stops once `below` holds for the sum of the caps, the orders done
    /// at their lowered caps and the others at their caps so far; true when it stopped so, or when `below` holds
    /// for the sum of all lowered caps. A cap only goes down, so the sum of the lowered caps is at most any such
    /// partial sum and the answer is the one of the complete sum when `below` holds for every smaller sum too.
    /// `computed` counts the orders whose raw and fine caps it computed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tighten_order_caps_until(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        orders: &[[usize; 5]],
        caps: &mut [i128],
        scratch: &mut JointScratch,
        below: impl Fn(i128) -> bool,
        computed: &mut u64,
    ) -> bool {
        let Some(mut sum) = caps.iter().try_fold(0i128, |a, &c| a.checked_add(c)) else {
            self.tighten_order_caps(domain, p, power, orders, caps, scratch);
            *computed += orders.len() as u64;
            return below(caps.iter().fold(0i128, |a, &c| a.saturating_add(c)));
        };
        if self.fine.is_none() {
            return below(sum);
        }
        for (positions, cap) in orders.iter().zip(caps.iter_mut()) {
            let mut lowered = *cap;
            if let Some(raw) = self.raw_upper(domain, p, power, positions) {
                lowered = lowered.min(raw);
            }
            lowered = lowered.min(self.fine_upper(domain, p, power, positions, scratch).expect("compiled fine bound"));
            *computed += 1;
            sum -= *cap - lowered;
            *cap = lowered;
            if below(sum) {
                return true;
            }
        }
        false
    }

    /// Sum integer per-order caps using exact masses; no floating expectation or free order selection.
    pub(crate) fn expected_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
    ) -> Result<(i128, i64), Error> {
        self.expected_upper_keyed(pool, domain, p, depth, orders, None)
    }

    /// `expected_upper` with the envelope of a node's placed carriers, if any.
    pub(crate) fn expected_upper_keyed(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        keyed: Option<&Keyed>,
    ) -> Result<(i128, i64), Error> {
        let mut total = 0i128;
        let mut power = 0;
        for (positions, weight) in orders {
            let (payoff, p) = self.upper_keyed(pool, domain, p, depth, positions, keyed);
            power = p;
            total = total
                .checked_add(
                    payoff
                        .checked_mul(i128::try_from(*weight).map_err(|_| unavailable("bound mass overflow"))?)
                        .ok_or_else(|| unavailable("bound product overflow"))?,
                )
                .ok_or_else(|| unavailable("bound sum overflow"))?;
        }
        Ok((total, power))
    }

    /// A node bound that keeps the payoff step of each performance order (`positions`, one per order). The placed
    /// slots read their gains at the order's positions (keyed when the node has a keyed envelope) and each slot to
    /// fill the largest gain a remaining pair of `open` (the members and choices the open slots may take) has at its
    /// position. Power and event bonus stay paired: a completion's pair is at most the placed one plus, per slot to
    /// fill, the pair of some remaining member and choice (characters and Snaps may repeat), so it is dominated by a
    /// point of the Pareto frontier of those sums, and each part is also at most its relaxed maximum of
    /// [`JointBounds::relax`]. An order's payoff is nondecreasing in its score cap and its bonus, so the sum over the
    /// orders of the frontier's best payoff bounds the payoff numerator of every completion without the concave
    /// majorant that the position-mean bound needs. With no remaining pair the prefix has no completion and the
    /// bound is `i128::MIN`. Needs the leader placed (`depth > 0`). None without a point or score-target payoff, or
    /// on overflow.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn order_step_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[[usize; 5]],
        keyed: Option<&Keyed>,
        open: &[(usize, usize)],
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        if depth == 0 {
            return None;
        }
        let (power, _, bonus) = self.relax(pool, domain, p, depth, &SLOTS[depth..], &[0, 1, 2, 3, 4], keyed);
        let profile = self.profile[p.members[2]];
        let choices = Self::prefix_choices(domain, p, depth);
        let characters = SLOTS.map(|slot| pool.members[p.members[slot]].character_id);
        let characters = &characters[..depth];
        let used = SLOTS.map(|slot| p.snaps[slot]);
        let used = &used[..depth];
        let (mut placed_power, mut placed_bonus) = (0i64, 0i64);
        for &slot in &SLOTS[..depth] {
            let (m, choice) = (p.members[slot], choices[slot]);
            placed_power += self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
            placed_bonus += pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] };
        }
        let mut free = [0f64; 5];
        // The Pareto frontier of the remaining pairs in one pass while their bonuses do not increase (the candidate
        // order sorts by bonus first), else by a sort.
        let (mut single, mut ordered, mut any) = (Vec::new(), true, false);
        let mut run = i64::MIN;
        for &(m, choice) in open {
            if characters.contains(&pool.members[m].character_id)
                || choice > 0 && used.contains(&Some(domain.snaps()[choice - 1]))
            {
                continue;
            }
            any = true;
            for (position, g) in free.iter_mut().enumerate() {
                *g = g.max(self.order_gains[m][choice][position]);
            }
            let pair = (
                self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] },
                pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] },
            );
            if !ordered {
                single.push(pair);
                continue;
            }
            match single.last() {
                Some(&(_, b)) if pair.1 > b => {
                    ordered = false;
                    single.push(pair);
                }
                _ if pair.0 <= run => {}
                Some(&(_, b)) if pair.1 == b => *single.last_mut().expect("nonempty") = pair,
                _ => single.push(pair),
            }
            run = run.max(pair.0);
        }
        if !any && depth < 5 {
            return Some((i128::MIN, i64::MIN));
        }
        let single = if ordered { single } else { pareto_pairs(single) };
        let mut sums = vec![(placed_power, placed_bonus)];
        for _ in depth..5 {
            sums = pareto_pairs(
                sums.iter().flat_map(|&(a, b)| single.iter().map(move |&(c, d)| (a + c, b + d))).collect(),
            );
        }
        let sums: Vec<(i64, i64)> = sums.into_iter().map(|(a, b)| (a.min(power), b.min(bonus))).collect();
        let a0 = keyed.map_or(self.a0, |k| k.a0);
        let mut factors: Vec<f64> = positions
            .iter()
            .map(|order| {
                let mut gain = 0.0;
                for (k, &slot) in SLOTS.iter().enumerate() {
                    let g = if k < depth {
                        keyed.map_or(self.order_gains[p.members[slot]][choices[slot]][order[slot]], |placed| {
                            placed.order_placed[slot][order[slot]]
                        })
                    } else {
                        free[order[slot]]
                    };
                    gain = add_up(gain, g);
                }
                add_up(a0, gain).min(self.global) * (1.0 + self.eps)
            })
            .collect();
        // Orders with the same factor share their payoff.
        factors.sort_unstable_by(f64::total_cmp);
        let mut total = 0i128;
        for run in factors.chunk_by(|a, b| a.to_bits() == b.to_bits()) {
            let best = sums
                .iter()
                .map(|&(sum_power, sum_bonus)| pt.order_payoff(sum_bonus, ((sum_power as f64) * run[0]).ceil() as i128))
                .max()?;
            total = total.checked_add(best.checked_mul(run.len() as i128)?)?;
        }
        Some((total, power))
    }
}

impl PointBound {
    fn score_target(members: usize, snaps: usize, target: ScoreTarget) -> Self {
        Self {
            member: vec![0; members],
            snap: vec![0; snaps],
            multiplier: match target {
                ScoreTarget::AtLeast(_) => 1,
                ScoreTarget::Capped(t) => i64::from(t),
            },
            score_tiers: None,
            hull: None,
            target: Some(target),
        }
    }
    /// The PT of one performance order of a deck with this event bonus whose score is at most `score_cap`.
    fn order_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        if let Some(target) = self.target {
            return match target {
                ScoreTarget::AtLeast(threshold) => i128::from(score_cap >= i128::from(threshold)),
                ScoreTarget::Capped(threshold) => score_cap.min(i128::from(threshold)),
            };
        }
        ((bonus + 10000) * self.multiplier_at(score_cap) / 10000) as i128
    }

    /// A bound of the mean PT over the performance orders of a deck with this event bonus whose mean score is at most
    /// `score_cap`. The PT of an order is `(bonus + 10000) * m(S) / 10000` with `m` the step multiplier, which is not
    /// concave; its concave majorant `M` gives `E[m(S)] <= E[M(S)] <= M(E[S]) <= M(score_cap)` (Jensen, `M`
    /// non-decreasing), and the rounding goes up. Without score tiers the multiplier is constant.
    fn mean_payoff(&self, bonus: i64, score_cap: i128) -> i128 {
        if let Some(target) = self.target {
            return match target {
                // A bound on E[S] alone cannot rule out S >= threshold, especially for signed network scores.
                // Per-order caps above can still prove impossibility without exchanging expectation and cutoff.
                ScoreTarget::AtLeast(_) => 1,
                // min(S,t) is nondecreasing and concave on the entire signed score domain.
                ScoreTarget::Capped(threshold) => score_cap.min(i128::from(threshold)),
            };
        }
        match &self.hull {
            None => ((bonus + 10000) * self.multiplier / 10000) as i128,
            Some(hull) => super::uniform::concave_value_ceil(hull, score_cap, i128::from(bonus + 10000), 10000)
                .expect("PT values in the nonwrapping domain"),
        }
    }

    fn multiplier_at(&self, score_cap: i128) -> i64 {
        self.score_tiers.as_ref().map_or(self.multiplier, |tiers| {
            tiers
                .iter()
                .take_while(|(score, _)| i128::from(*score) <= score_cap)
                .map(|(_, reward)| *reward)
                .max()
                .unwrap_or(0)
        })
    }

    fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        input: &EventPayoffInput,
        event_id: i64,
        challenge_points: bool,
    ) -> Result<Self, Error> {
        use ournotes_sim::event::{self, EventCard};
        let context = request.objective.context().ok_or_else(|| unavailable("PT needs resolved context"))?;
        let q = context.event_request(pool.master, input, event_id)?;
        let route = point_route::PointRoute::compile(pool.master, &q, event_id, challenge_points)?;
        let effects = [event::event_effects(pool.master, event_id)];
        let mut member = vec![0; pool.members.len()];
        for &m in domain.members() {
            if challenge_points {
                continue;
            }
            member[m] = event::total_effect_10000(
                &effects,
                Some(EventCard::Member(&ournotes_sim::bonus::event_member(pool.master, &pool.members[m]))),
                event::EVENT_POINT,
            )? as i64;
        }
        let snap = domain
            .snaps()
            .iter()
            .map(|&s| {
                if challenge_points {
                    return Ok(0);
                }
                event::total_effect_10000(
                    &effects,
                    Some(EventCard::Snap(&ournotes_sim::bonus::event_snap(&pool.snaps[s]))),
                    event::EVENT_POINT,
                )
                .map(i64::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if member.iter().chain(&snap).any(|&v| v < 0) {
            return Err(unavailable("negative PT bonus outside relaxation domain"));
        }
        let group = pool
            .master
            .live_music(context.resolved.live_music_id)
            .ok_or_else(|| unavailable("missing music"))?
            .live_score_rank_group;
        let multiplayer = matches!(
            context.scenario,
            ournotes_sim::scenario::Scenario::Battle(_) | ournotes_sim::scenario::Scenario::Arena(_)
        );
        let room = if multiplayer { input.multiplayer_score_policy.as_ref() } else { None };
        // Include score zero and every local-score preimage of a native result-rank threshold. Room totals must
        // be calculated for each outcome; a rank at mean score is not the mean of the per-order PT payoff.
        let mut scores = vec![0];
        scores.extend(
            event::rank_rows_of_group(pool.master, group)
                .iter()
                .map(|r| match room {
                    Some(policy) => {
                        let threshold =
                            event::battle_required_score(r.battle_live_required_score, policy.players()) as i64;
                        match *policy {
                            ournotes_sim::scenario::MultiplayerScorePolicy::SameScore { players } => {
                                (threshold.max(0) + players - 1) / players
                            }
                            ournotes_sim::scenario::MultiplayerScorePolicy::FixedOthersAverage { players, score } => {
                                (threshold - (players - 1) * score as i64).max(0)
                            }
                        }
                    }
                    None => r.required_score,
                })
                .filter(|&v| (0..=i32::MAX as i64).contains(&v)),
        );
        scores.sort_unstable();
        scores.dedup();
        let mut values = Vec::new();
        for &score in &scores {
            let rank = match room {
                Some(policy) => {
                    let total = match *policy {
                        ournotes_sim::scenario::MultiplayerScorePolicy::SameScore { players } => score * players,
                        ournotes_sim::scenario::MultiplayerScorePolicy::FixedOthersAverage { players, score: peer } => {
                            score + (players - 1) * peer as i64
                        }
                    };
                    event::battle_score_rank(pool.master, group, total, policy.players())
                }
                None => event::score_rank(pool.master, group, score)?,
            };
            values.push(route.value_at_rank(pool.master, rank)?);
        }
        if multiplayer {
            values.extend(signed_network_reward_values(pool.master, &route, group));
        }
        let rate = route.rate();
        if !(0..=i32::MAX as i64).contains(&rate) || values.iter().any(|v| !(0..=i32::MAX as i64).contains(v)) {
            return Err(unavailable("PT rate/value outside nonwrapping domain"));
        }
        let maximum_bonus = 5 * (member.iter().copied().max().unwrap_or(0) + snap.iter().copied().max().unwrap_or(0));
        let multiplier = rate
            .checked_mul(values.iter().copied().max().unwrap_or(0))
            .ok_or_else(|| unavailable("PT product overflow"))?;
        let base = maximum_bonus + 10000;
        if (challenge_points && multiplier > i32::MAX as i64)
            || (!challenge_points
                && (base > i32::MAX as i64
                    || base.checked_mul(rate).is_none_or(|v| v > i32::MAX as i64)
                    || base.checked_mul(multiplier).is_none_or(|v| v > i32::MAX as i64)))
        {
            return Err(unavailable("PT intermediate wrapping requires exhaustive fallback"));
        }
        // A negative network rank bonus can produce a negative terminal score. A concave hull built only on
        // nonnegative score tiers cannot bound rewards at negative scores from E[S]; such a room, and a multiplayer
        // result without a declared room score policy, keeps the maximum defined native reward over the entire i32
        // rank domain. Every confirmed bonus nonnegative, the terminal score is nonnegative and the declared room
        // total is nondecreasing in it, so the local-score preimages step the reward as in a solo Live.
        let signed = context.rank_confirmations.iter().flatten().any(|c| c.percent < 0);
        let score_tiers: Option<Vec<(i64, i64)>> = (!multiplayer || (room.is_some() && !signed))
            .then(|| scores.into_iter().zip(values).map(|(s, v)| (s, v * rate)).collect());
        let hull = score_tiers.as_deref().map(super::uniform::concave_majorant);
        Ok(Self { member, snap, multiplier, score_tiers, hull, target: None })
    }
}

fn signed_network_reward_values(
    master: &ournotes_sim::master::Master,
    route: &point_route::PointRoute,
    group: i64,
) -> Vec<i64> {
    use ournotes_sim::event;
    let mut ranks = vec![event::RANK_NONE];
    ranks.extend(
        event::rank_rows_of_group(master, group)
            .iter()
            .map(|row| if row.live_score_rank == event::RANK_E { event::RANK_D } else { row.live_score_rank }),
    );
    ranks.sort_unstable();
    ranks.dedup();
    // A missing native reward row has no terminal payoff to bound: exact settlement still reports its Game error.
    // Defined NONE rows must be included even though the first ordinary threshold is at score zero.
    ranks.into_iter().filter_map(|rank| route.value_at_rank(master, rank).ok()).collect()
}

#[cfg(test)]
mod network_point_tests {
    use super::*;
    use ournotes_sim::{event, master::Master};
    use serde_json::json;

    #[test]
    fn score_target_caps_preserve_signed_means_and_per_order_cutoffs() {
        for threshold in [1, 10, 100] {
            let probability = PointBound::score_target(0, 0, ScoreTarget::AtLeast(threshold));
            let capped = PointBound::score_target(0, 0, ScoreTarget::Capped(threshold));
            for a in [-1000i128, -10, 0, 1, 10, 100, 1000] {
                assert_eq!(probability.order_payoff(0, a), i128::from(a >= threshold as i128));
                assert_eq!(capped.order_payoff(0, a), a.min(threshold as i128));
                for b in [-1000i128, -10, 0, 1, 10, 100, 1000] {
                    let mean_ceiling = (a + b).div_euclid(2) + i128::from((a + b).rem_euclid(2) != 0);
                    assert!(
                        2 * probability.mean_payoff(0, mean_ceiling)
                            >= i128::from(a >= threshold as i128) + i128::from(b >= threshold as i128)
                    );
                    assert!(
                        2 * capped.mean_payoff(0, mean_ceiling) >= a.min(threshold as i128) + b.min(threshold as i128)
                    );
                }
            }
        }
    }

    #[test]
    fn pareto_pairs_keep_exactly_the_undominated_pairs() {
        let pairs = vec![(5, 1), (3, 4), (5, 2), (1, 9), (3, 3), (2, 4), (1, 9), (0, 10)];
        assert_eq!(pareto_pairs(pairs), vec![(5, 2), (3, 4), (1, 9), (0, 10)]);
    }

    #[test]
    fn negative_network_score_none_reward_is_in_ep_and_cp_caps() {
        let tables = json!({
            "MasterEvent":[{"_id":7,"_liveEventPointGroup":1}],
            "MasterLiveScoreRank":[{"_id":1,"_group":1,"_liveScoreRank":2,"_requiredScore":0,"_battleLiveRequiredScore":0}],
            "MasterLiveEventPoint":[{"_id":1,"_group":1,"_scoreRank":0,"_value":100},{"_id":2,"_group":1,"_scoreRank":2,"_value":1}],
            "MasterLiveChallengePoint":[{"_id":1,"_scoreRank":0,"_value":100},{"_id":2,"_scoreRank":2,"_value":1}],
            "MasterLiveMusicBoostBonus":[{"_id":1,"_consumedLiveBoostCount":0,"_eventPointRate":1}]
        });
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        let master =
            Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, text)| text.as_str()))
                .unwrap();
        let request = event::EventPointRequest {
            route: event::EventResultRoute::NormalPlayed,
            holding_event_ids: vec![7],
            consumed_count: 0,
            local_events: vec![event::LocalEvent { event_id: 7, points: 0, challenge_points: 0, added: Vec::new() }],
        };
        assert_eq!(event::battle_score_rank(&master, 1, -1, 1), event::RANK_NONE);
        assert_eq!(event::battle_score_rank(&master, 1, 0, 1), event::RANK_D);
        for cp in [false, true] {
            let route = point_route::PointRoute::compile(&master, &request, 7, cp).unwrap();
            let multiplier = signed_network_reward_values(&master, &route, 1).into_iter().max().unwrap() * route.rate();
            let bound = PointBound {
                member: Vec::new(),
                snap: Vec::new(),
                multiplier,
                score_tiers: None,
                hull: None,
                target: None,
            };
            assert_eq!(route.value_at_rank(&master, event::RANK_NONE).unwrap(), 100);
            assert_eq!(bound.order_payoff(0, 10), 100);
            assert_eq!(bound.mean_payoff(0, 10), 100);
            assert_eq!(bound.mean_payoff(0, -1), 100);
        }
    }
}
