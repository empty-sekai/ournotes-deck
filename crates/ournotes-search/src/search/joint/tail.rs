//! Bounds of the children of a node at and after a choice offset, read without visiting them.

use super::*;

/// The choices of the choice order a block of [`TailTables::blocks`] holds.
pub(crate) const PAIR_BLOCK: usize = 16;

/// The maxima over sets of next pairs that bound them all at once: the suffixes of the choice order from each offset
/// on, and its blocks of [`PAIR_BLOCK`] choices.
pub(super) struct TailTables {
    pub(super) suffix: PairMaxima,
    pub(super) blocks: PairMaxima,
}

/// Per set of pairs (by index), the largest of each quantity a next pair adds to its parent's residual.
pub(super) struct PairMaxima {
    /// Per leader profile, the added power.
    pub(super) power: Vec<Vec<i64>>,
    /// Per position, the position-mean gain.
    pub(super) gain: [Vec<f64>; 5],
    pub(super) bonus: Vec<i64>,
    /// Per other term of a combined metric's point bound, the bonus under the term.
    pub(super) others: Vec<Vec<i64>>,
    pub(super) spread: Vec<f64>,
    /// Per position, the per-position gains (`order_gains`).
    pub(super) order_gain: [Vec<f64>; 5],
}

impl PairMaxima {
    /// The maxima of `sets` empty sets of pairs, at least 0 each.
    pub(super) fn zeros(profiles: usize, others: usize, sets: usize) -> PairMaxima {
        PairMaxima {
            power: vec![vec![0; sets]; profiles],
            gain: std::array::from_fn(|_| vec![0.0; sets]),
            bonus: vec![0; sets],
            others: vec![vec![0; sets]; others],
            spread: vec![0.0; sets],
            order_gain: std::array::from_fn(|_| vec![0.0; sets]),
        }
    }

    /// Takes pair `(m, choice)` of `b` into set `set`.
    pub(super) fn add(&mut self, b: &JointBounds, set: usize, m: usize, choice: usize) {
        self.spread[set] = self.spread[set].max(b.spread[m][choice]);
        for pos in 0..5 {
            self.order_gain[pos][set] = self.order_gain[pos][set].max(b.order_gains[m][choice][pos]);
            self.gain[pos][set] = self.gain[pos][set].max(b.gains[m][choice][pos]);
        }
        for (profile, row) in self.power.iter_mut().enumerate() {
            let p = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
            row[set] = row[set].max(p);
        }
        if let Some(pt) = &b.points {
            self.bonus[set] = self.bonus[set].max(pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
            for (row, (_, _, part)) in self.others.iter_mut().zip(&pt.others) {
                row[set] = row[set].max(part.pair_bonus(m, choice));
            }
        }
    }

    /// Takes set `from` into set `set`.
    pub(super) fn merge(&mut self, set: usize, from: usize) {
        self.spread[set] = self.spread[set].max(self.spread[from]);
        self.bonus[set] = self.bonus[set].max(self.bonus[from]);
        for row in &mut self.others {
            row[set] = row[set].max(row[from]);
        }
        for pos in 0..5 {
            self.order_gain[pos][set] = self.order_gain[pos][set].max(self.order_gain[pos][from]);
            self.gain[pos][set] = self.gain[pos][set].max(self.gain[pos][from]);
        }
        for row in &mut self.power {
            row[set] = row[set].max(row[from]);
        }
    }
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
    /// Per other term of a combined metric's point bound, at least the bonus under the term of the placed slots and
    /// of the rows' slots (see [`JointBounds::other_bonuses`]); the next pair's adds to it.
    others: Vec<i64>,
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
        let (spread, beside) = if self.points.is_some() || self.is_order_maximum() {
            let (spread, best) = self.order_gain_bounds(domain, p, depth, 4 - depth, keyed);
            (spread, std::array::from_fn(|pos| self.less_offset(order_gain_bound(&best, &self.column, 1 << pos))))
        } else {
            (f64::INFINITY, [f64::INFINITY; 5])
        };
        Some(TailState {
            profile: self.profile[p.members[2]],
            a0: keyed.map_or(self.a0, |k| k.a0),
            spread,
            beside,
            others: self.other_bonuses_free(pool, domain, p, depth, 4 - depth),
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
        self.maxima_upper(state, &t.suffix, offset)
    }

    /// Bounds every legal child whose next pair lies in block `block` of the choice order (see [`PAIR_BLOCK`]), as
    /// [`JointBounds::tail_upper`] does with the block's maxima.
    pub(crate) fn block_upper(&self, state: &TailState, block: usize) -> Result<(i128, i64), Error> {
        let t = self.tails.as_ref().expect("compiled tail table");
        self.maxima_upper(state, &t.blocks, block)
    }

    fn maxima_upper(&self, state: &TailState, t: &PairMaxima, set: usize) -> Result<(i128, i64), Error> {
        let placed = (0..5).map(|pos| add_up(t.order_gain[pos][set], state.beside[pos])).fold(f64::MIN, f64::max);
        let others = self.tail_others(state, |i, _| t.others[i][set]);
        let mut total = 0i128;
        let mut power = 0;
        for &(p, g, b, pos, weight) in &state.rows {
            power = p + t.power[state.profile][set];
            let gain = add_up(g, t.gain[pos][set]);
            let max_gain = add_up(gain, add_up(state.spread, t.spread[set])).min(placed);
            let payoff = self.payoff_cap_with(state.a0, power, gain, b + t.bonus[set], &others, max_gain);
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
        let others = self.tail_others(state, |_, part| part.pair_bonus(member, choice));
        let mut total = 0i128;
        let mut power = 0;
        for &(p, g, b, pos, weight) in &state.rows {
            power = p + added_power;
            let gain = add_up(g, self.gains[member][choice][pos]);
            let max_gain = add_up(gain, add_up(state.spread, self.spread[member][choice])).min(placed);
            let payoff = self.payoff_cap_with(state.a0, power, gain, b + added_bonus, &others, max_gain);
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

    /// The other terms' bonuses of a tail state's children: the state's plus the next pair's, `next(i, term)` for the
    /// `i`-th other term, at most the term's largest bonus of a team.
    fn tail_others(&self, state: &TailState, next: impl Fn(usize, &PointBound) -> i64) -> Vec<i64> {
        let Some(pt) = &self.points else { return Vec::new() };
        state
            .others
            .iter()
            .zip(&pt.others)
            .enumerate()
            .map(|(i, (&bonus, (_, largest, part)))| (bonus + next(i, part)).min(*largest))
            .collect()
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
