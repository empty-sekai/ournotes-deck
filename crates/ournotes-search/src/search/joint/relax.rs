//! The cheap relaxation of a node: the position-mean payoff cap of every completion of a prefix.

use super::*;

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
pub(super) fn order_gain_bound(best: &[f64; 32], column: &[f64; 5], excluded: usize) -> f64 {
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
    pub(super) fn upper_keyed(
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

    pub(super) fn payoff_cap(&self, power: i64, gain: f64, bonus: i64) -> i128 {
        self.payoff_cap_from(self.a0, power, gain, bonus, f64::INFINITY)
    }

    /// `payoff_cap` with the `A0` of some envelope at most this one's: a bound of the mean payoff over the orders when
    /// `gain` sums position-mean gains and `max_gain` bounds the gain sum of every order (an infinite `max_gain`
    /// leaves the global coefficient).
    pub(super) fn payoff_cap_from(&self, a0: f64, power: i64, gain: f64, bonus: i64, max_gain: f64) -> i128 {
        let cap = |gain: f64| ((power as f64) * add_up(a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
        let score_cap = cap(gain);
        self.points.as_ref().map_or(score_cap, |pt| pt.mean_payoff(bonus, score_cap, cap(max_gain)))
    }

    /// Two bounds of the gain sum of any completion of a prefix in any order (in the keyed envelope, if any, for the
    /// placed slots): the spread above its position-mean gain sum, of its placed slots and the largest member spreads
    /// for its `free` slots to fill; and the placement sums of its placed slots' per-position rows (see
    /// `order_gain_bound`, whose other slots take the largest per-position gains).
    pub(super) fn order_gain_bounds(
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

    /// At least the gain sum of every order of every completion of a prefix whose position-mean gain sum is at most
    /// `gain` (see `order_gain_bounds`); infinite without points.
    pub(super) fn node_order_gain(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        gain: f64,
        keyed: Option<&Keyed>,
    ) -> f64 {
        if self.points.is_none() {
            return f64::INFINITY;
        }
        let (spread, best) = self.order_gain_bounds(domain, p, depth, 5 - depth, keyed);
        add_up(gain, spread).min(order_gain_bound(&best, &self.column, 0))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn relax(
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
}
