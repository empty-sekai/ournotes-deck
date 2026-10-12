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
mod compile;
mod composition;
mod correlated;
mod cutoff;
mod lambda;
#[cfg(test)]
mod network_point_tests;
mod order_steps;
mod orders;
mod point_bound;
mod point_route;
mod points;
mod prefix_character;
mod prefix_resource;
#[cfg(test)]
mod ranked_item_tests;
mod relax;
mod relax_tables;
mod resource;
mod split_tables;
mod tail;
pub(crate) use bonus::BonusScratch;
use compile::row_spread;
pub(crate) use order_steps::OrderSteps;
pub(crate) use point_bound::term_cap;
use point_bound::{PointBound, ScoreTarget};
use relax::order_gain_bound;
pub(crate) use tail::TailState;
use tail::TailTables;

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
    /// The lottery laws the bounds weigh (see [`JointBounds::compile_with_laws`]).
    laws: Vec<std::sync::Arc<ournotes_sim::live::full::LuckDpCertifiedResult>>,
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

    /// The cheap bounds of every deck with at most `n` Gekisou combo carriers (the pool-wide ones from 5 on).
    pub(crate) fn carrier_level(&self, n: usize) -> &JointBounds {
        match &self.carrier_levels {
            Some(levels) => levels.at.iter().skip(n).flatten().next().map_or(self, |b| b),
            None => self,
        }
    }

    /// The lottery laws the bounds weigh; empty when every note takes its largest luck factor.
    pub(crate) fn laws(&self) -> &[std::sync::Arc<ournotes_sim::live::full::LuckDpCertifiedResult>] {
        &self.laws
    }

    /// Whether the bounds cover a team whose performance orders have the lottery laws `own`: they weigh no law, or
    /// each of `own` is among their laws.
    pub(crate) fn cover_laws(&self, own: &[std::sync::Arc<ournotes_sim::live::full::LuckDpCertifiedResult>]) -> bool {
        crate::search::snaps::LuckLaws::new(&self.laws).is_none_or(|laws| laws.cover(own))
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

    /// The bound modules of these bounds (see `NodeBound`).
    pub(crate) fn modules(&self) -> &[Box<dyn NodeBound>] {
        &self.modules
    }

    pub(crate) fn has_fine(&self) -> bool {
        self.fine.is_some()
    }
}
