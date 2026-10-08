//! Joint member/Snap relaxation of the declared performance-order objective.
//!
//! The cheap envelope bounds the score of a team in one performance order by `P * min(A0 + sum of the slots' position
//! gains, G) * (1 + eps)`. Its uniform expectation over the 120 orders is at most the same expression with every gain
//! replaced by its mean over the five positions (linearity of the gain sum, Jensen for the concave `min`), so every
//! node bound reads position-mean gains, and a cap read at [`super::uniform::MEAN_ORDERS`] bounds the sum over the
//! orders. The per-note fine and raw caps keep their per-order structure: a complete team gets one of each per
//! order, and their sum bounds its value.
//!
//! For the best-order expectation, each node gain row instead repeats its largest position gain. A 32-mask
//! assignment bound places selected rows in distinct positions and intersects that relaxation. Both bounds
//! include every original order separately. The existing 120-unit numerator grid bounds 120 times the maximum
//! conditional expectation. Complete-team caps retain their original order labels and use their maximum on
//! that grid. Controller-family means and carrier-split means apply only to the uniform objective.
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
mod family;
pub(crate) use family::FamilyBoundResult;
#[cfg(test)]
pub(crate) use family::tests::{family_choices as reward_family_choices, fixture as reward_family_fixture};
mod lambda;
mod point_route;
mod prefix_character;
mod prefix_resource;
#[cfg(test)]
mod raw_pass_tests;
mod relax_tables;
mod resource;
mod split_tables;
pub(crate) use bonus::BonusScratch;

/// Leader first, then the other physical slots. Performance orders are evaluated separately.
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
    /// The concave majorants of the step multiplier truncated at each tier: `hulls[k]` is that of `score_tiers[..=k]`
    /// (see `uniform::concave_majorant`), which equals the step multiplier's majorant on the scores below tier `k + 1`.
    hulls: Option<Vec<Vec<(i64, i64)>>>,
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
    spread: Vec<f64>,
    /// Per position, the suffix maxima of the per-position gains (`order_gains`).
    order_gain: [Vec<f64>; 5],
}

/// One prefix's unconstrained remaining slots, excluding the next pair being enumerated.
pub(crate) struct TailState {
    profile: usize,
    /// `A0` of the envelope the rows read (see `Keyed`).
    a0: f64,
    /// The spread of the rows' slots (see `JointBounds::order_gain_bounds`).
    spread: f64,
    /// Per position `p`, at least the gain sum of the rows' slots in every order that leaves `p` to the next slot.
    beside: [f64; 5],
    rows: Vec<(i64, f64, i64, usize, u128)>,
}

/// What the order-step bound of a node (see [`JointBounds::order_steps_prepare`]) reads of its placed slots, and the
/// open pairs of each suffix of the candidate order from the first offset a check of the node's choice loop asked for
/// (the loop asks for ascending offsets, so one backward pass collects them all).
#[derive(Default)]
pub(crate) struct OrderSteps {
    ready: bool,
    depth: usize,
    profile: usize,
    /// The relaxed power and event bonus maxima of the node's completions (`JointBounds::relax`).
    caps: (i64, i64),
    /// The placed slots' power and event bonus.
    placed: (i64, i64),
    a0: f64,
    /// Per performance order, the placed slots' gain sum in slot order, rounded up.
    gains: Vec<f64>,
    /// The placed characters, and per choice whether a placed slot holds its Snap.
    characters: Vec<i64>,
    used: Vec<bool>,
    /// The suffix summaries from candidate `first` on: per offset the largest per-position gains of the open pairs
    /// and the top of `stack`, a persistent stack of (power, bonus, parent) whose path from a top holds that suffix's
    /// Pareto frontier.
    first: usize,
    free: Vec<[f64; 5]>,
    top: Vec<u32>,
    stack: Vec<(i64, i64, u32)>,
    singles: Vec<(i64, i64)>,
    sums: Vec<(i64, i64)>,
    factors: Vec<f64>,
    steps: Vec<(f64, i128)>,
}

/// The empty path of `OrderSteps::stack`.
const NO_PAIR: u32 = u32::MAX;

impl OrderSteps {
    /// Starts a node: the next `JointBounds::order_steps_prepare` prepares it anew.
    pub(crate) fn reset(&mut self) {
        self.ready = false;
    }

    /// Whether an open slot of the node may take a member and choice: no placed character or Snap.
    fn open(&self, pool: &Pool, m: usize, choice: usize) -> bool {
        !self.characters.contains(&pool.members[m].character_id) && !self.used[choice]
    }
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
    /// Each row repeats its position mean, or its position maximum for the best-order objective.
    gains: Vec<Vec<[f64; 5]>>,
    best_order: bool,
    /// The per-position gains of the envelope, for the per-order caps of complete teams and the order-step bound.
    order_gains: Vec<Vec<[f64; 5]>>,
    /// Per member and choice, at least how far its largest per-position gain lies above its position-mean gain: with
    /// them a node's position-mean gain sum bounds the gain sum of every order too (see `PointBound::mean_payoff`).
    spread: Vec<Vec<f64>>,
    /// `spread_top[k]`: at least the sum of the largest member spreads of any `k` distinct domain members.
    spread_top: [f64; 6],
    /// Per position, the largest per-position gain of any domain member and choice.
    column: [f64; 5],
    a0: f64,
    global: f64,
    eps: f64,
    points: Option<PointBound>,
    /// The least final life of a score and life target: an order whose final life cap is below it pays nothing.
    min_final_life: Option<i64>,
    fine: Option<JointFineBounds>,
    correlation_scales: [f64; 3],
    tails: Option<TailTables>,
    composition: Option<composition::CompositionTables>,
    /// Bound modules both traversals consult after the built-in bounds (see `NodeBound`).
    modules: Vec<Box<dyn NodeBound>>,
    /// Reward-only coefficients for a native-certified fixed-member LUCK family.
    family_rewards: Option<std::rc::Rc<super::snaps::ProfileRewardTemplate>>,
    family_template: Option<super::telemetry::FamilyTemplateSetup>,
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

fn node_gain_row(row: &[f64; 5], best_order: bool) -> [f64; 5] {
    if best_order { [row.iter().copied().fold(0.0, f64::max); 5] } else { super::uniform::mean_row(row) }
}

fn node_gain_table(gains: &[Vec<[f64; 5]>], best_order: bool) -> Vec<Vec<[f64; 5]>> {
    if !best_order {
        return mean_table(gains);
    }
    gains.iter().map(|rows| rows.iter().map(|row| node_gain_row(row, true)).collect()).collect()
}
/// How far a row's largest per-position gain lies above its position mean (`mean[0]`, every entry the same), rounded
/// up so that `add_up(mean, spread)` is at least the largest gain.
fn row_spread(row: &[f64; 5], mean: &[f64; 5]) -> f64 {
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
/// The largest gain sums of per-position gain rows over their placements on distinct positions: `best[mask]` over
/// the placements onto exactly the positions in `mask` (negative infinity for the other masks), rounded up.
fn placement_sums<'a>(rows: impl Iterator<Item = &'a [f64; 5]>) -> [f64; 32] {
    let mut best = [f64::NEG_INFINITY; 32];
    best[0] = 0.0;
    for row in rows {
        let mut next = [f64::NEG_INFINITY; 32];
        for (mask, &sum) in best.iter().enumerate() {
            if sum == f64::NEG_INFINITY {
                continue;
            }
            for (pos, &g) in row.iter().enumerate() {
                if mask & (1 << pos) == 0 {
                    next[mask | (1 << pos)] = next[mask | (1 << pos)].max(add_up(sum, g));
                }
            }
        }
        best = next;
    }
    best
}
/// At least the gain sum of every order of five slots: the placed rows with their placement sums `best`, every other
/// slot at most `column[p]` at its position `p`, and the positions in `excluded` taken by none of them.
fn order_gain_bound(best: &[f64; 32], column: &[f64; 5], excluded: usize) -> f64 {
    let mut bound = f64::NEG_INFINITY;
    for (mask, &sum) in best.iter().enumerate() {
        if mask & excluded != 0 || sum == f64::NEG_INFINITY {
            continue;
        }
        bound = bound
            .max((0..5).filter(|&pos| (mask | excluded) & (1 << pos) == 0).fold(sum, |s, pos| add_up(s, column[pos])));
    }
    bound
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
                    | Metric::BestOrderExpectedScore
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
        let (family_rewards, family_template) = if matches!(metric, Metric::Score) {
            let result = super::snaps::ProfileRewardTemplate::compile(&envelope);
            let diagnostics = super::telemetry::FamilyTemplateSetup {
                admitted: result.is_ok(),
                refusal: result.as_ref().err().copied(),
                factor_envelope: envelope.factor_diagnostics(),
            };
            (result.ok(), Some(diagnostics))
        } else {
            (None, None)
        };
        let (a0, global, eps, order_gains) = envelope.joint_envelope();
        let levels = if setup.gk.is_some() { envelope.joint_carrier_levels() } else { None };
        let keys = levels.as_ref().and_then(|_| envelope.carrier_keys());
        if ![a0, global, eps].iter().all(|v| v.is_finite() && *v >= 0.0)
            || order_gains.iter().flatten().flatten().any(|g| !g.is_finite() || *g < 0.0)
        {
            return Err(unavailable("nonfinite/negative score relaxation"));
        }
        let best_order = matches!(metric, Metric::BestOrderExpectedScore);
        let gains = node_gain_table(&order_gains, best_order);
        let (spread, spread_top, column) = spread_tables(&order_gains, &gains, domain);
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
                    eps < 1.0,
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
            best_order,
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
            family_rewards,
            family_template,
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
            if !best_order {
                compiled.carrier_split = carrier_split::CarrierSplit::compile(&compiled, pool, domain);
            }
        }
        Ok(compiled)
    }

    /// These cheap bounds with another linear envelope (per-position `gains`): the same power tables and choice order,
    /// no fine bound.
    fn level(&self, pool: &Pool, domain: &CandidateDomain, a0: f64, global: f64, gains: Vec<Vec<[f64; 5]>>) -> Self {
        let means = node_gain_table(&gains, self.best_order);
        let (spread, spread_top, column) = spread_tables(&gains, &means, domain);
        let mut b = Self {
            a: self.a.clone(),
            w: self.w.clone(),
            lead: self.lead.clone(),
            profile: self.profile.clone(),
            gains: means,
            best_order: self.best_order,
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
            family_rewards: None,
            family_template: None,
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
        let placed = order_placed.map(|g| node_gain_row(&g, self.best_order));
        let a0 = keys.a0(&env, SLOTS[..depth].iter().map(|&slot| (p.members[slot], choices[slot])), free);
        Some(Keyed { a0, placed, order_placed })
    }

    /// The number of carrier levels compiled apart from the pool-wide bounds.
    pub(crate) fn carrier_level_count(&self) -> usize {
        self.carrier_levels.as_ref().map_or(0, |l| l.at.iter().flatten().count())
    }

    /// Snaps whose compiled per-entry reach supplies useful conversion partition boundaries.
    pub(crate) fn conversion_snaps(&self, domain: &CandidateDomain) -> Vec<usize> {
        self.fine.as_ref().map_or_else(Vec::new, |fine| fine.conversion_snaps(domain.members(), domain.snaps()))
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
        let max_gain = self.node_order_gain(domain, p, depth, gain, keyed);
        (self.payoff_cap_from(keyed.map_or(self.a0, |k| k.a0), power, gain, bonus, max_gain), power)
    }

    /// Diagnostics only: the PT score tiers, their concave majorant and the constant multiplier.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn point_tiers(&self) -> serde_json::Value {
        self.points.as_ref().map_or(
            serde_json::Value::Null,
            |pt| serde_json::json!({"tiers":pt.score_tiers,"hull":pt.hulls.as_ref().and_then(|h| h.last()),"multiplier":pt.multiplier}),
        )
    }

    /// Diagnostics only: the relaxation behind a node's cheap bound (keyed when given) with its mean score cap, and
    /// the PT read at that cap on the concave majorant and on the step function.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn node_score_profile(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        keyed: Option<&Keyed>,
    ) -> serde_json::Value {
        let (power, gain, bonus) = self.relax(pool, domain, p, depth, &SLOTS[depth..], &[0, 1, 2, 3, 4], keyed);
        let (spread, best) = self.order_gain_bounds(domain, p, depth, 5 - depth, keyed);
        let max_gain = self.node_order_gain(domain, p, depth, gain, keyed);
        let a0 = keyed.map_or(self.a0, |k| k.a0);
        let cap = |gain: f64| ((power as f64) * add_up(a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
        let (score_cap, max_cap) = (cap(gain), cap(max_gain));
        serde_json::json!({"power":power,"gain":gain,"spread":spread,"a0":a0,"global":self.global,"eps":self.eps,
            "placementGain":order_gain_bound(&best, &self.column, 0),"maxGain":max_gain,
            "bonus":bonus,"meanScoreCap":score_cap.to_string(),"maxScoreCap":max_cap.to_string(),
            "payoff":self.points.as_ref().map(|pt| pt.mean_payoff(bonus, score_cap, max_cap).to_string()),
            "fullHullPayoff":self.points.as_ref().map(|pt| pt.mean_payoff(bonus, score_cap, i128::MAX).to_string()),
            "stepPayoff":self.points.as_ref().map(|pt| pt.order_payoff(bonus, score_cap).to_string())})
    }

    /// Diagnostics only: the cheap score caps of a complete team per order ([`JointBounds::order_cheap_caps`]
    /// before the PT step).
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn order_score_caps(
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
        orders
            .iter()
            .map(|positions| {
                let gain = (0..5).fold(0.0, |gain, slot| {
                    add_up(
                        gain,
                        keyed
                            .as_ref()
                            .map_or(level.order_gains[p.members[slot]][choices[slot]][positions[slot]], |k| {
                                k.order_placed[slot][positions[slot]]
                            }),
                    )
                });
                ((power as f64) * add_up(a0, gain).min(level.global) * (1.0 + level.eps)).ceil() as i128
            })
            .collect()
    }

    fn payoff_cap(&self, power: i64, gain: f64, bonus: i64) -> i128 {
        self.payoff_cap_from(self.a0, power, gain, bonus, f64::INFINITY)
    }

    /// `payoff_cap` with the `A0` of some envelope at most this one's. Uniform expectations use position-mean
    /// `gain`; the best-order objective also intersects its row-maximum gain with the assignment cap `max_gain`.
    /// Both caps include every order, with each selected row assigned to a distinct performance position.
    fn payoff_cap_from(&self, a0: f64, power: i64, gain: f64, bonus: i64, max_gain: f64) -> i128 {
        let cap = |gain: f64| ((power as f64) * add_up(a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
        let score_cap = cap(if self.best_order { gain.min(max_gain) } else { gain });
        self.points.as_ref().map_or(score_cap, |pt| pt.mean_payoff(bonus, score_cap, cap(max_gain)))
    }

    /// Two bounds of the gain sum of any completion of a prefix in any order (in the keyed envelope, if any, for the
    /// placed slots): the spread above its position-mean gain sum, of its placed slots and the largest member spreads
    /// for its `free` slots to fill; and the placement sums of its placed slots' per-position rows (see
    /// `order_gain_bound`, whose other slots take the largest per-position gains).
    fn order_gain_bounds(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        free: usize,
        keyed: Option<&Keyed>,
    ) -> (f64, [f64; 32]) {
        let mut spread = self.spread_top[free];
        let mut rows = [[0.0; 5]; 5];
        for (row, &slot) in rows.iter_mut().zip(&SLOTS[..depth]) {
            let placed = match keyed {
                Some(k) => {
                    *row = k.order_placed[slot];
                    row_spread(&k.order_placed[slot], &k.placed[slot])
                }
                None => {
                    let choice = p.snaps[slot]
                        .map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
                    *row = self.order_gains[p.members[slot]][choice];
                    self.spread[p.members[slot]][choice]
                }
            };
            spread = add_up(spread, placed);
        }
        (spread, placement_sums(rows[..depth].iter()))
    }

    /// At least the gain sum of every order of every completion of a prefix whose node gain sum is at most
    /// `gain` (see `order_gain_bounds`). Used by stepped payoffs and the best-order score objective.
    fn node_order_gain(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        gain: f64,
        keyed: Option<&Keyed>,
    ) -> f64 {
        if self.points.is_none() && !self.best_order {
            return f64::INFINITY;
        }
        let (spread, best) = self.order_gain_bounds(domain, p, depth, 5 - depth, keyed);
        add_up(gain, spread).min(order_gain_bound(&best, &self.column, 0))
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
        let (spread, beside) = if self.points.is_some() {
            let (spread, best) = self.order_gain_bounds(domain, p, depth, 4 - depth, keyed);
            (spread, std::array::from_fn(|pos| order_gain_bound(&best, &self.column, 1 << pos)))
        } else {
            (f64::INFINITY, [f64::INFINITY; 5])
        };
        Some(TailState {
            profile: self.profile[p.members[2]],
            a0: keyed.map_or(self.a0, |k| k.a0),
            spread,
            beside,
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
        let placed = (0..5).map(|pos| add_up(t.order_gain[pos][offset], state.beside[pos])).fold(f64::MIN, f64::max);
        let mut total = 0i128;
        let mut power = 0;
        for &(p, g, b, pos, weight) in &state.rows {
            power = p + t.power[state.profile][offset];
            let gain = add_up(g, t.gain[pos][offset]);
            let max_gain = add_up(gain, add_up(state.spread, t.spread[offset])).min(placed);
            let payoff = self.payoff_cap_from(state.a0, power, gain, b + t.bonus[offset], max_gain);
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
        let row = &self.order_gains[member][choice];
        let placed = (0..5).map(|pos| add_up(row[pos], state.beside[pos])).fold(f64::MIN, f64::max);
        let mut total = 0i128;
        let mut power = 0;
        for &(p, g, b, pos, weight) in &state.rows {
            power = p + added_power;
            let gain = add_up(g, self.gains[member][choice][pos]);
            let max_gain = add_up(gain, add_up(state.spread, self.spread[member][choice])).min(placed);
            let payoff = self.payoff_cap_from(state.a0, power, gain, b + added_bonus, max_gain);
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
        // The uncoupled maxima of power and gain for the per-order score cap of PT.
        let (mut bonus, mut power_sum, mut gain_sum) = (0, 0i64, 0.0);
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
            power_sum += power;
            gain_sum = add_up(gain_sum, gain);
        }
        let mut remaining = HashMap::<i64, ([f64; 3], i64, i64, f64)>::new();
        for &m in domain.members() {
            let c = pool.members[m].character_id;
            if characters.contains(&c) {
                continue;
            }
            let row = remaining.entry(c).or_insert(([0.0; 3], 0, 0, 0.0));
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
                row.2 = row.2.max(power);
                row.3 = row.3.max(gain);
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
        self.points.as_ref().map_or(score_cap, |pt| {
            let mut powers: Vec<_> = remaining.values().map(|r| r.2).collect();
            powers.sort_unstable_by(|a, b| b.cmp(a));
            power_sum += powers.iter().take(5 - depth).sum::<i64>();
            let mut gains: Vec<_> = remaining.values().map(|r| r.3).collect();
            gains.sort_by(|a, b| b.total_cmp(a));
            gain_sum = gains.iter().take(5 - depth).fold(gain_sum, |sum, &g| add_up(sum, g));
            let gain = self.node_order_gain(domain, p, depth, gain_sum, keyed);
            let max_cap = ((power_sum as f64) * add_up(a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
            pt.mean_payoff(bonus, score_cap, max_cap)
        })
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

    /// Whether the request's static fine-bound shape can admit a terminal Rush mean cap.
    /// Completed recorder and probability checks remain necessary for each actual order.
    pub(crate) fn supports_rush_mean_upper(&self) -> bool {
        self.points.is_none()
            && self.min_final_life.is_none()
            && self.fine.as_ref().is_some_and(|fine| fine.supports_rush_mean_upper())
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

    /// An expected-score cap for the same completed terminal recorder and physical performance order.
    /// This never maps a mean through an event or threshold payoff; those keep their full probability scorer.
    pub(crate) fn rush_mean_upper(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        positions: &[usize; 5],
        scratch: &mut JointScratch,
        terminal: &ournotes_sim::live::full::LuckTerminalRush,
    ) -> Option<f64> {
        if self.points.is_some() || self.min_final_life.is_some() {
            return None;
        }
        if let Some(upper) = terminal.native_score_mean_upper(power) {
            return Some(upper);
        }
        let choices = Self::prefix_choices(domain, p, 5);
        self.fine.as_ref()?.rush_mean_upper(power, p.members, choices, positions, scratch, terminal)
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
                if self.short_of_final_life(p, &choices, positions) {
                    return 0;
                }
                level.points.as_ref().map_or(score_cap, |pt| pt.order_payoff(bonus, score_cap))
            })
            .collect()
    }

    /// Diagnostics only: the final life cap of a complete team in the order with these positions.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn final_life_cap(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        positions: &[usize; 5],
    ) -> Option<i64> {
        self.fine.as_ref()?.final_life_cap(p.members, Self::prefix_choices(domain, p, 5), positions)
    }

    /// Diagnostics only: [`JointBounds::short_of_final_life`] of a complete team.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn order_short_of_final_life(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        positions: &[usize; 5],
    ) -> bool {
        self.short_of_final_life(p, &Self::prefix_choices(domain, p, 5), positions)
    }

    /// Whether a score and life target pays nothing for a complete team in the order with these positions because
    /// its final life cap is below the target's least final life.
    fn short_of_final_life(&self, p: &PhysicalDeck, choices: &[usize; 5], positions: &[usize; 5]) -> bool {
        let (Some(least), Some(fine)) = (self.min_final_life, &self.fine) else { return false };
        fine.final_life_cap(p.members, *choices, positions).is_some_and(|cap| cap < least)
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
    /// Best-order selection instead tests the maximum of all caps, scaled by the unchanged order denominator.
    /// Every available constant-cost raw cap is applied before any per-note fine cap. Each unfinished order
    /// retains its own bound throughout both passes; no order or probability mass is removed.
    /// `computed` counts the orders whose fine caps it computed.
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
        if self.best_order {
            let maximum =
                |caps: &[i128]| caps.iter().copied().max().unwrap_or(0).saturating_mul(super::uniform::ORDERS as i128);
            if self.fine.is_none() || below(maximum(caps)) {
                return below(maximum(caps));
            }
            for (index, positions) in orders.iter().enumerate() {
                let cap = &mut caps[index];
                if let Some(raw) = self.raw_upper(domain, p, power, positions) {
                    *cap = (*cap).min(raw);
                }
            }
            if below(maximum(caps)) {
                return true;
            }
            for (index, positions) in orders.iter().enumerate() {
                let cap = &mut caps[index];
                *cap = (*cap).min(self.fine_upper(domain, p, power, positions, scratch).expect("compiled fine bound"));
                *computed += 1;
                // Every unfinished order retains its previous cap. A small sum or a completed promising
                // order alone cannot exclude the best-order objective.
                if below(maximum(caps)) {
                    return true;
                }
            }
            return false;
        }
        let Some(mut sum) = caps.iter().try_fold(0i128, |a, &c| a.checked_add(c)) else {
            self.tighten_order_caps(domain, p, power, orders, caps, scratch);
            *computed += orders.len() as u64;
            return below(caps.iter().fold(0i128, |a, &c| a.saturating_add(c)));
        };
        if self.fine.is_none() {
            return below(sum);
        }
        for (positions, cap) in orders.iter().zip(caps.iter_mut()) {
            if let Some(raw) = self.raw_upper(domain, p, power, positions) {
                let lowered = (*cap).min(raw);
                sum -= *cap - lowered;
                *cap = lowered;
            }
        }
        if below(sum) {
            return true;
        }
        for (positions, cap) in orders.iter().zip(caps.iter_mut()) {
            let lowered =
                (*cap).min(self.fine_upper(domain, p, power, positions, scratch).expect("compiled fine bound"));
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

    /// The order-step bound: a node bound that keeps the payoff step of each performance order (`positions`, one per
    /// order). The placed slots read their gains at the order's positions (keyed when the node has a keyed envelope)
    /// and each slot to fill the largest gain an open pair (a remaining member and choice the open slots may take) has
    /// at its position. Power and event bonus stay paired: a completion's pair is at most the placed one plus, per
    /// slot to fill, the pair of some open member and choice (characters and Snaps may repeat), so it is dominated by
    /// a point of the Pareto frontier of those sums, and each part is also at most its relaxed maximum of
    /// [`JointBounds::relax`]. An order's payoff is nondecreasing in its score cap and its bonus, so the sum over the
    /// orders of the frontier's best payoff bounds the payoff numerator of every completion without the concave
    /// majorant that the position-mean bound needs. With no open pair the prefix has no completion and the bound is
    /// `i128::MIN`.
    ///
    /// This prepares the node's part in `steps` (once per node; [`OrderSteps::reset`] starts a node). False without
    /// a point or score-target payoff, or at the root (`depth == 0`), where the node has no such bound.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn order_steps_prepare(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[[usize; 5]],
        keyed: Option<&Keyed>,
        steps: &mut OrderSteps,
    ) -> bool {
        if steps.ready {
            return true;
        }
        let Some(pt) = &self.points else { return false };
        if depth == 0 {
            return false;
        }
        let (power, _, bonus) = self.relax(pool, domain, p, depth, &SLOTS[depth..], &[0, 1, 2, 3, 4], keyed);
        let profile = self.profile[p.members[2]];
        let choices = Self::prefix_choices(domain, p, depth);
        let (mut placed_power, mut placed_bonus) = (0i64, 0i64);
        steps.characters.clear();
        steps.used.clear();
        steps.used.resize(domain.snaps().len() + 1, false);
        for &slot in &SLOTS[..depth] {
            let (m, choice) = (p.members[slot], choices[slot]);
            placed_power += self.pair_power(profile, m, choice);
            placed_bonus += pt.pair_bonus(m, choice);
            steps.characters.push(pool.members[m].character_id);
        }
        for (choice, used) in steps.used.iter_mut().enumerate().skip(1) {
            *used = SLOTS[..depth].iter().any(|&slot| p.snaps[slot] == Some(domain.snaps()[choice - 1]));
        }
        steps.gains.clear();
        steps.gains.extend(positions.iter().map(|order| {
            SLOTS[..depth].iter().fold(0.0, |gain, &slot| {
                add_up(
                    gain,
                    keyed.map_or(self.order_gains[p.members[slot]][choices[slot]][order[slot]], |placed| {
                        placed.order_placed[slot][order[slot]]
                    }),
                )
            })
        }));
        steps.depth = depth;
        steps.profile = profile;
        steps.caps = (power, bonus);
        steps.placed = (placed_power, placed_bonus);
        steps.a0 = keyed.map_or(self.a0, |k| k.a0);
        steps.first = usize::MAX;
        steps.ready = true;
        true
    }

    /// The power of a member and choice under a leader profile.
    fn pair_power(&self, profile: usize, m: usize, choice: usize) -> i64 {
        self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] }
    }

    /// The order-step bound of the prepared node's completions whose open slots take pairs of `open` (one pass over
    /// them). None on overflow.
    pub(crate) fn order_steps_open(
        &self,
        pool: &Pool,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        open: &[(usize, usize)],
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        let mut free = [0f64; 5];
        // The Pareto frontier of the open pairs in one pass while their bonuses do not increase (the candidate order
        // sorts by bonus first), else by a sort.
        let (mut ordered, mut any) = (true, false);
        let mut run = i64::MIN;
        steps.singles.clear();
        for &(m, choice) in open {
            if !steps.open(pool, m, choice) {
                continue;
            }
            any = true;
            for (position, g) in free.iter_mut().enumerate() {
                *g = g.max(self.order_gains[m][choice][position]);
            }
            let pair = (self.pair_power(steps.profile, m, choice), pt.pair_bonus(m, choice));
            let single = &mut steps.singles;
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
        if !any {
            return Some((i128::MIN, i64::MIN));
        }
        if !ordered {
            steps.singles = pareto_pairs(std::mem::take(&mut steps.singles));
        }
        self.order_steps_total(pt, positions, steps, &free, None)
    }

    /// The order-step bound of the prepared node's completions whose open slots take pairs from candidate `offset`
    /// on. The first call of a node collects the suffixes from its offset on; later calls ask for later suffixes.
    pub(crate) fn order_steps_suffix(
        &self,
        pool: &Pool,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        offset: usize,
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        let free = self.order_steps_singles(pool, steps, offset);
        if steps.singles.is_empty() {
            return Some((i128::MIN, i64::MIN));
        }
        self.order_steps_total(pt, positions, steps, &free, None)
    }

    /// The order-step bound of the prepared node's completions whose next slot (`SLOTS[depth]`) takes the pair `m`,
    /// `choice` of candidate `offset` and whose other open slots take pairs from `offset + 1` on. The child's own
    /// character and Snap stay open to those slots, which only enlarges the completion set.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn order_steps_pair(
        &self,
        pool: &Pool,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        offset: usize,
        m: usize,
        choice: usize,
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        let free = self.order_steps_singles(pool, steps, offset + 1);
        if steps.singles.is_empty() {
            return Some((i128::MIN, i64::MIN));
        }
        self.order_steps_total(pt, positions, steps, &free, Some((m, choice)))
    }

    /// The largest per-position gains of the open pairs from `offset` on, with their (power, bonus) Pareto frontier
    /// in `steps.singles` (bonus descending). Collects the suffix summaries from `offset` on when the node has none
    /// that far back: one backward pass keeps the frontier of each suffix as the top of a persistent stack.
    fn order_steps_singles(&self, pool: &Pool, steps: &mut OrderSteps, offset: usize) -> [f64; 5] {
        let pt = self.points.as_ref().expect("prepared order steps");
        let n = self.choices.len();
        let offset = offset.min(n);
        if offset < steps.first {
            steps.first = offset;
            steps.free.clear();
            steps.free.resize(n + 1 - offset, [0.0; 5]);
            steps.top.clear();
            steps.top.resize(n + 1 - offset, NO_PAIR);
            steps.stack.clear();
            let (mut free, mut top) = ([0f64; 5], NO_PAIR);
            for i in (offset..n).rev() {
                let (m, choice) = self.choices[i];
                if steps.open(pool, m, choice) {
                    for (position, g) in free.iter_mut().enumerate() {
                        *g = g.max(self.order_gains[m][choice][position]);
                    }
                    let (power, bonus) = (self.pair_power(steps.profile, m, choice), pt.pair_bonus(m, choice));
                    // Pairs come in nondecreasing bonus order: the new one removes the frontier points it dominates
                    // from the top, unless the top dominates it. Out of that order the stack may keep dominated
                    // points too, which no bound reads as better than the frontier.
                    let mut below = top;
                    while below != NO_PAIR {
                        let (p, b, parent) = steps.stack[below as usize];
                        if p > power || b > bonus {
                            break;
                        }
                        below = parent;
                    }
                    top = match steps.stack.get(below as usize) {
                        Some(&(p, b, _)) if p >= power && b >= bonus => below,
                        _ => {
                            steps.stack.push((power, bonus, below));
                            (steps.stack.len() - 1) as u32
                        }
                    };
                }
                steps.free[i - offset] = free;
                steps.top[i - offset] = top;
            }
        }
        steps.singles.clear();
        let (mut at, mut ordered) = (steps.top[offset - steps.first], true);
        while at != NO_PAIR {
            let (power, bonus, parent) = steps.stack[at as usize];
            if let Some(&(p, b)) = steps.singles.last() {
                ordered &= bonus < b && power > p;
            }
            steps.singles.push((power, bonus));
            at = parent;
        }
        if !ordered {
            steps.singles = pareto_pairs(std::mem::take(&mut steps.singles));
        }
        steps.free[offset - steps.first]
    }

    /// The order-step bound from the prepared node, an optional child pair of the next slot and the open pairs'
    /// per-position gains (`free`) and frontier (`steps.singles`).
    fn order_steps_total(
        &self,
        pt: &PointBound,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        free: &[f64; 5],
        child: Option<(usize, usize)>,
    ) -> Option<(i128, i64)> {
        let (power, bonus) = steps.caps;
        let (mut placed_power, mut placed_bonus) = steps.placed;
        let mut placed = steps.depth;
        if let Some((m, choice)) = child {
            placed_power += self.pair_power(steps.profile, m, choice);
            placed_bonus += pt.pair_bonus(m, choice);
            placed += 1;
        }
        steps.sums.clear();
        steps.sums.push((placed_power, placed_bonus));
        for _ in placed..5 {
            let singles = &steps.singles;
            let cross = steps.sums.iter().flat_map(|&(a, b)| singles.iter().map(move |&(c, d)| (a + c, b + d)));
            steps.sums = pareto_pairs(cross.collect());
        }
        for sum in &mut steps.sums {
            *sum = (sum.0.min(power), sum.1.min(bonus));
        }
        let child_slot = SLOTS[steps.depth];
        let (a0, global, eps) = (steps.a0, self.global, self.eps);
        steps.factors.clear();
        steps.factors.extend(positions.iter().zip(&steps.gains).map(|(order, &placed_gain)| {
            let mut gain = placed_gain;
            if let Some((m, choice)) = child {
                gain = add_up(gain, self.order_gains[m][choice][order[child_slot]]);
            }
            for &slot in &SLOTS[placed..] {
                gain = add_up(gain, free[order[slot]]);
            }
            add_up(a0, gain).min(global) * (1.0 + eps)
        }));
        Some((pt.order_payoff_sum(&steps.sums, &steps.factors, &mut steps.steps)?, power))
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
            hulls: None,
            target: Some(target),
        }
    }
    /// The event bonus of a member and choice.
    fn pair_bonus(&self, m: usize, choice: usize) -> i64 {
        self.member[m] + if choice == 0 { 0 } else { self.snap[choice - 1] }
    }

    /// At least the sum over performance orders (one `factors` entry each) of the largest PT of one order over
    /// (power, bonus) pairs whose score caps are `ceil(power * factor)`. With score tiers that largest PT is a step
    /// function of the factor: a pair reaches a tier from some factor on, and then pays at least the tier's PT. The
    /// cap reaches `score` only when the rounded product exceeds `score - 1`, which needs the factor above
    /// `(score - 1) / power` less a relative rounding error, so each step starts no later than the threshold taken a
    /// few ulps low (`steps` holds the steps between the smallest and the largest factor). Without tiers, or with a
    /// score target, each order reads the pairs.
    fn order_payoff_sum(&self, pairs: &[(i64, i64)], factors: &[f64], steps: &mut Vec<(f64, i128)>) -> Option<i128> {
        let best = |factor: f64| {
            pairs.iter().map(|&(power, bonus)| self.order_payoff(bonus, ((power as f64) * factor).ceil() as i128)).max()
        };
        let (Some(tiers), None) = (&self.score_tiers, self.target) else {
            return factors.iter().try_fold(0i128, |total, &factor| total.checked_add(best(factor)?));
        };
        let (low, high) = factors.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &f| (l.min(f), h.max(f)));
        // A cap below every tier pays nothing.
        let mut base = 0i128;
        steps.clear();
        for &(power, bonus) in pairs {
            for &(score, _) in tiers {
                let from = if score <= 0 {
                    f64::NEG_INFINITY
                } else if power > 0 {
                    (score - 1) as f64 / power as f64 * (1.0 - 4.0 * f64::EPSILON)
                } else {
                    continue;
                };
                let paid = self.order_payoff(bonus, i128::from(score));
                if from <= low {
                    base = base.max(paid);
                } else if from <= high {
                    steps.push((from, paid));
                }
            }
        }
        steps.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut run = base;
        for step in steps.iter_mut() {
            run = run.max(step.1);
            step.1 = run;
        }
        factors.iter().try_fold(0i128, |total, &factor| {
            let at = steps.partition_point(|&(from, _)| from <= factor);
            total.checked_add(if at == 0 { base } else { steps[at - 1].1 })
        })
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
    /// `score_cap` and whose score in every order is at most `max_cap`. The PT of an order is
    /// `(bonus + 10000) * m(S) / 10000` with `m` the step multiplier, which is not concave. Every score is at most
    /// `max_cap`, where `m` equals the step of the tiers at or below `max_cap`; the concave majorant `M` of that
    /// truncated step gives `E[m(S)] <= E[M(S)] <= M(E[S]) <= M(score_cap)` (Jensen, `M` non-decreasing), and the
    /// rounding goes up. The PT of every order is also at most the PT at `max_cap`. Without score tiers the multiplier
    /// is constant.
    fn mean_payoff(&self, bonus: i64, score_cap: i128, max_cap: i128) -> i128 {
        if let Some(target) = self.target {
            return match target {
                // A bound on E[S] alone cannot rule out S >= threshold, especially for signed network scores; the
                // per-order cap can.
                ScoreTarget::AtLeast(threshold) => i128::from(max_cap >= i128::from(threshold)),
                // min(S,t) is nondecreasing and concave on the entire signed score domain.
                ScoreTarget::Capped(threshold) => score_cap.min(i128::from(threshold)),
            };
        }
        match (&self.hulls, &self.score_tiers) {
            (Some(hulls), Some(tiers)) => {
                let k = tiers.partition_point(|&(score, _)| i128::from(score) <= max_cap).max(1) - 1;
                super::uniform::concave_value_ceil(&hulls[k], score_cap, i128::from(bonus + 10000), 10000)
                    .expect("PT values in the nonwrapping domain")
                    .min(self.order_payoff(bonus, max_cap))
            }
            _ => ((bonus + 10000) * self.multiplier / 10000) as i128,
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
        nonnegative_scores: bool,
    ) -> Result<Self, Error> {
        use ournotes_sim::event::{self, EventCard};
        let context = request.objective.context().ok_or_else(|| unavailable("PT needs resolved context"))?;
        let q = context.event_request(pool.master, input, event_id)?;
        let route = point_route::PointRoute::compile(pool.master, &q, event_id, challenge_points)?;
        if !nonnegative_scores && route.value_at_rank(pool.master, event::RANK_NONE).is_err() {
            return Err(unavailable("signed score domain requires a defined NONE payoff"));
        }
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
        if multiplayer || !nonnegative_scores {
            values.extend(signed_network_reward_values(pool.master, &route, group));
        }
        let rate = route.rate();
        if !(0..=i32::MAX as i64).contains(&rate) || values.iter().any(|v| !(0..=i32::MAX as i64).contains(v)) {
            return Err(unavailable("PT rate/value outside nonwrapping domain"));
        }
        let maximum_bonus = super::team_power::largest_team_bonus(
            domain.members().iter().map(|&m| (pool.members[m].character_id, member[m])),
            &snap,
        );
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
        // Score-tier hulls require a certified nonnegative score domain and, for
        // multiplayer, a declared nonnegative room-score mapping. A signed score
        // domain uses the maximum defined reward over the native rank domain.
        let signed = context.rank_confirmations.iter().flatten().any(|c| c.percent < 0);
        let score_tiers: Option<Vec<(i64, i64)>> = (nonnegative_scores
            && (!multiplayer || (room.is_some() && !signed)))
            .then(|| scores.into_iter().zip(values).map(|(s, v)| (s, v * rate)).collect());
        let hulls = score_tiers
            .as_deref()
            .map(|tiers| (1..=tiers.len()).map(|k| super::uniform::concave_majorant(&tiers[..k])).collect());
        Ok(Self { member, snap, multiplier, score_tiers, hulls, target: None })
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
    fn maximum_position_rows_enclose_every_order_with_distinct_position_peaks() {
        let rows = [
            [1000.0, 1.0, 2.0, 3.0, 4.0],
            [5.0, 2000.0, 6.0, 7.0, 8.0],
            [9.0, 10.0, 3000.0, 11.0, 12.0],
            [13.0, 14.0, 15.0, 4000.0, 16.0],
            [17.0, 18.0, 19.0, 20.0, 5000.0],
        ];
        let gains: Vec<_> = rows.into_iter().map(|row| vec![row]).collect();
        let uniform = node_gain_table(&gains, false);
        let maximum = node_gain_table(&gains, true);
        let uniform_cap: f64 = uniform.iter().map(|row| row[0][0]).sum();
        let maximum_cap: f64 = maximum.iter().map(|row| row[0][0]).sum();
        let mut largest = 0.0f64;
        for order in super::super::uniform::all_orders() {
            let score: f64 = order.iter().enumerate().map(|(position, &slot)| rows[slot][position]).sum();
            largest = largest.max(score);
            assert!(score <= maximum_cap);
        }
        assert_eq!(largest, 15_000.0);
        assert!(largest > uniform_cap * 4.0);
        assert!(maximum.iter().flatten().all(|row| row.iter().all(|&gain| gain == row[0])));
    }

    #[test]
    fn best_order_assignment_cap_prevents_members_from_sharing_the_same_peak_position() {
        // The first three members all want position zero. A bound must retain their alternate positions,
        // but no complete order can collect all three row maxima at once.
        let rows = [
            [1000.0, 10.0, 20.0, 0.0, 0.0],
            [2000.0, 30.0, 40.0, 0.0, 0.0],
            [3000.0, 50.0, 60.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 4000.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 5000.0],
        ];
        let column = std::array::from_fn(|position| rows.iter().map(|row| row[position]).fold(0.0, f64::max));
        let row_maxima: f64 = rows.iter().map(|row| row.iter().copied().fold(0.0, f64::max)).sum();
        let actual = super::super::uniform::all_orders()
            .iter()
            .map(|order| order.iter().enumerate().map(|(position, &slot)| rows[slot][position]).sum::<f64>())
            .fold(0.0, f64::max);
        assert_eq!(actual, 12_050.0);
        assert_eq!(row_maxima, 15_000.0);
        let mut strictly_tighter = false;
        for depth in 0..=5 {
            let cap = order_gain_bound(&placement_sums(rows[..depth].iter()), &column, 0);
            // Unselected rows still use per-column maxima, so every prefix covers every completion.
            assert!(actual <= cap, "depth {depth}: {actual} > {cap}");
            strictly_tighter |= cap < row_maxima;
            if depth == 5 {
                assert!(cap - actual < 1e-8, "complete assignment cap {cap}");
            }
        }
        assert!(strictly_tighter);
    }

    #[test]
    fn best_order_assignment_encloses_integer_ulp_boundaries_and_positive_overflow() {
        // Individual terms are exact, while their sums cross the binary64 integer-spacing boundary.
        let rows: [[f64; 5]; 5] = std::array::from_fn(|member| {
            std::array::from_fn(|position| ((1u64 << 52) + (member * 7 + position * 3 + 1) as u64) as f64)
        });
        let column = std::array::from_fn(|position| rows.iter().map(|row| row[position]).fold(0.0, f64::max));
        for depth in 0..=5 {
            let cap = order_gain_bound(&placement_sums(rows[..depth].iter()), &column, 0);
            assert!(cap.is_finite());
            for order in super::super::uniform::all_orders() {
                let exact: i128 = order.iter().enumerate().map(|(position, &slot)| rows[slot][position] as i128).sum();
                assert!(cap as i128 >= exact, "depth {depth}, order {order:?}: {cap} < {exact}");
            }
        }
        let large = [[f64::MAX; 5]; 5];
        let cap = order_gain_bound(&placement_sums(large.iter()), &[f64::MAX; 5], 0);
        assert_eq!(cap, f64::INFINITY, "positive overflow must retain a conservative upper bound");
    }

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
                        2 * probability.mean_payoff(0, mean_ceiling, a.max(b))
                            >= i128::from(a >= threshold as i128) + i128::from(b >= threshold as i128)
                    );
                    assert!(
                        2 * capped.mean_payoff(0, mean_ceiling, a.max(b))
                            >= a.min(threshold as i128) + b.min(threshold as i128)
                    );
                }
            }
        }
    }

    #[test]
    fn truncated_majorants_bound_the_mean_step_payoff_below_the_order_cap() {
        let tiers =
            vec![(0, 15), (2_104_068, 25), (5_025_129, 35), (11_281_724, 50), (15_052_045, 75), (20_496_573, 100)];
        let hulls = (1..=tiers.len()).map(|k| super::super::uniform::concave_majorant(&tiers[..k])).collect();
        let bound = PointBound {
            member: Vec::new(),
            snap: Vec::new(),
            multiplier: 100,
            score_tiers: Some(tiers),
            hulls: Some(hulls),
            target: None,
        };
        // An xorshift sequence of order scores; each set's mean and maximum bound its mean step payoff.
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..2000 {
            let centre = (next() % 22_000_000) as i128;
            let width = (next() % 3_000_000) as i128;
            let scores: Vec<i128> = (0..120).map(|_| (centre + (next() % (width as u64 + 1)) as i128).max(0)).collect();
            let bonus = (next() % 20_000) as i64;
            let max = *scores.iter().max().unwrap();
            let sum: i128 = scores.iter().sum();
            let mean_ceiling = (sum + 119) / 120;
            let actual: i128 = scores.iter().map(|&s| bound.order_payoff(bonus, s)).sum();
            assert!(actual <= 120 * bound.mean_payoff(bonus, mean_ceiling, max), "{centre} {width}");
            // the truncation is never looser than the full majorant
            assert!(bound.mean_payoff(bonus, mean_ceiling, max) <= bound.mean_payoff(bonus, mean_ceiling, i128::MAX));
        }
        // Every order below the 75 tier: the bound is the 50 step, not the chord to the 100 tier.
        assert_eq!(bound.mean_payoff(8500, 12_105_470, 12_818_652), 92);
        assert_eq!(bound.mean_payoff(8500, 12_105_470, i128::MAX), 122);
    }

    #[test]
    fn payoff_steps_sum_the_best_order_payoffs_and_never_fall_below_at_tier_edges() {
        let tiers = vec![(0, 15), (935_655, 25), (2_076_590, 35), (4_741_282, 50), (6_360_627, 75), (8_551_127, 100)];
        let bound = PointBound {
            member: Vec::new(),
            snap: Vec::new(),
            multiplier: 100,
            score_tiers: Some(tiers.clone()),
            hulls: None,
            target: None,
        };
        let exact = |pairs: &[(i64, i64)], factors: &[f64]| -> i128 {
            factors
                .iter()
                .map(|&f| {
                    pairs.iter().map(|&(p, b)| bound.order_payoff(b, ((p as f64) * f).ceil() as i128)).max().unwrap()
                })
                .sum()
        };
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut steps = Vec::new();
        for _ in 0..2000 {
            let pairs: Vec<(i64, i64)> =
                (0..1 + next() % 6).map(|_| ((next() % 400_000) as i64, (next() % 30_000) as i64)).collect();
            let factors: Vec<f64> = (0..120).map(|_| 5.0 + (next() % 1_000_000) as f64 / 25_000.0).collect();
            // Random factors never land within a few ulps of a tier edge.
            assert_eq!(bound.order_payoff_sum(&pairs, &factors, &mut steps), Some(exact(&pairs, &factors)));
        }
        // Factors around each edge: the steps start no later than the caps reach the tier.
        for &(score, _) in &tiers[1..] {
            for power in [1, 3, 997, 123_457, 399_989] {
                let edge = (score - 1) as f64 / power as f64;
                let mut factors = vec![edge];
                for _ in 0..4 {
                    factors.push(factors.last().unwrap().next_up());
                    factors.insert(0, factors[0].next_down());
                }
                let pairs = [(power, 8500)];
                for f in &factors {
                    let one = std::slice::from_ref(f);
                    assert!(bound.order_payoff_sum(&pairs, one, &mut steps).unwrap() >= exact(&pairs, one));
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
                hulls: None,
                target: None,
            };
            assert_eq!(route.value_at_rank(&master, event::RANK_NONE).unwrap(), 100);
            assert_eq!(bound.order_payoff(0, 10), 100);
            assert_eq!(bound.mean_payoff(0, 10, 10), 100);
            assert_eq!(bound.mean_payoff(0, -1, -1), 100);
        }
    }
}
