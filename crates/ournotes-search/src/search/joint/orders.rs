//! The per-order caps of a complete team: cheap, raw and fine, and their sum over the performance orders.

use super::*;

/// The caps of a complete team that showed it below a cutoff (see [`JointBounds::team_below`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TeamCaps {
    /// Its cheap caps.
    Cheap,
    /// Its raw and fine caps.
    Fine,
}

impl JointBounds {
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
        Some(
            self.points
                .as_ref()
                .map_or(score_cap, |pt| pt.team_order_payoff(&Self::team_pairs(p, &choices), score_cap)),
        )
    }

    /// At least the sum of [`JointBounds::fine_upper`] over the performance orders `orders` of a complete deck, as one
    /// cap ([`JointFineBounds::upper_orders`]), for a score payoff without a least final life; None otherwise. With
    /// `below` the cap only needs to decide it: whether it holds for the cap is whether it holds for the tightest one.
    pub(crate) fn fine_upper_orders(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        orders: &[[usize; 5]],
        scratch: &mut JointScratch,
        below: Option<&dyn Fn(i128) -> bool>,
    ) -> Option<i128> {
        if self.points.is_some() || self.min_final_life.is_some() {
            return None;
        }
        let choices = Self::prefix_choices(domain, p, 5);
        let below = below.map(|below| move |cap: i64| below(i128::from(cap)));
        let below = below.as_ref().map(|below| below as &dyn Fn(i64) -> bool);
        self.fine.as_ref()?.upper_orders(power, p.members, choices, orders, scratch, below).map(i128::from)
    }

    /// Which caps of a complete team over the performance orders `orders` show `below` for its payoff numerator, if
    /// any: first its fine caps summed at once ([`JointBounds::fine_upper_orders`]) where that applies, then the sum
    /// of its cheap caps ([`JointBounds::order_cheap_caps`]), then, where no fine sum applies, those caps lowered to
    /// their raw and fine caps order by order ([`JointBounds::tighten_order_caps_until`]). `below` holds for a total
    /// at most one it holds for, so it holds for the least of these sums exactly when it holds for one of them, in
    /// any order of the checks. `summed` counts the teams whose fine caps were summed at once, `computed` the orders
    /// whose caps were computed one by one.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn team_below(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        orders: &[[usize; 5]],
        scratch: &mut JointScratch,
        below: impl Fn(i128) -> bool,
        (summed, computed): (&mut u64, &mut u64),
    ) -> Option<TeamCaps> {
        let sum = self.fine_upper_orders(domain, p, power, orders, scratch, Some(&below));
        if let Some(sum) = sum {
            *summed += 1;
            if below(sum) {
                return Some(TeamCaps::Fine);
            }
        }
        let mut caps = self.order_cheap_caps(domain, p, power, orders);
        if below(caps.iter().fold(0i128, |a, &c| a.saturating_add(c))) {
            return Some(TeamCaps::Cheap);
        }
        (sum.is_none()
            && self.fine.is_some()
            && self.tighten_order_caps_until(domain, p, power, orders, &mut caps, scratch, below, computed))
        .then_some(TeamCaps::Fine)
    }

    /// Whether a complete team's best performance order is below a cutoff by `below`, for the maximum aggregation:
    /// its cheap cap in every order (`orders` holds the positions of each), then the fine cap of each order whose
    /// cheap cap is not below, until one is not below either. `computed` counts the orders whose fine caps were read.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn best_order_below(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        orders: &[[usize; 5]],
        scratch: &mut JointScratch,
        below: impl Fn(i128) -> bool,
        computed: &mut u64,
    ) -> Option<TeamCaps> {
        let caps = self.order_cheap_caps(domain, p, power, orders);
        if caps.iter().all(|&cap| below(cap)) {
            return Some(TeamCaps::Cheap);
        }
        self.fine.as_ref()?;
        for (positions, &cap) in orders.iter().zip(&caps) {
            if below(cap) {
                continue;
            }
            *computed += 1;
            if !self.fine_upper(domain, p, power, positions, scratch).is_some_and(&below) {
                return None;
            }
        }
        Some(TeamCaps::Fine)
    }

    /// The member and choice pairs of a complete deck by slot.
    fn team_pairs(p: &PhysicalDeck, choices: &[usize; 5]) -> [(usize, usize); 5] {
        std::array::from_fn(|s| (p.members[s], choices[s]))
    }

    /// The event bonus of a complete deck (0 without a PT objective).
    pub(super) fn bonus_of(&self, p: &PhysicalDeck, choices: &[usize; 5]) -> i64 {
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
        Some(self.points.as_ref().map_or(cap, |pt| pt.team_order_payoff(&Self::team_pairs(p, &choices), cap)))
    }

    /// The cheap payoff cap of a complete team in each performance order (`orders` holds the positions of each), at
    /// its exact power: the envelope of its Gekisou combo carrier level, keyed by its carriers, read at the order's
    /// positions with the per-position gains. Their sum bounds the team's payoff numerator over the orders and is at
    /// most [`crate::search::uniform::ORDERS`] times its position-mean cheap bound.
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
        let a0 = level.less_offset(keyed.as_ref().map_or(level.a0, |k| k.a0));
        let pairs = Self::team_pairs(p, &choices);
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
                level.points.as_ref().map_or(score_cap, |pt| pt.team_order_payoff(&pairs, score_cap))
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
        self.expected_upper_reading(pool, domain, p, depth, orders, keyed, true)
    }

    /// `expected_upper_keyed`, the other terms of a combined metric at their bonus bounds of the node (`per_term`) or
    /// at their largest bonuses of a team.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn expected_upper_reading(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        orders: &[([usize; 5], u128)],
        keyed: Option<&Keyed>,
        per_term: bool,
    ) -> Result<(i128, i64), Error> {
        let mut total = 0i128;
        let mut power = 0;
        for (positions, weight) in orders {
            let (payoff, p) = self.upper_reading(pool, domain, p, depth, positions, keyed, per_term);
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
}
