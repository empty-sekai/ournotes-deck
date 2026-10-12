//! Bounds of the children of a node at and after a choice offset, read without visiting them.

use super::*;

pub(super) struct TailTables {
    pub(super) power: Vec<Vec<i64>>,
    pub(super) gain: [Vec<f64>; 5],
    pub(super) bonus: Vec<i64>,
    pub(super) spread: Vec<f64>,
    /// Per position, the suffix maxima of the per-position gains (`order_gains`).
    pub(super) order_gain: [Vec<f64>; 5],
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

impl JointBounds {
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
        let (extra, _) = crate::search::matching::best_assignment(weights.each_ref().map(|r| r.as_slice()));
        Some(fixed + extra)
    }
}
