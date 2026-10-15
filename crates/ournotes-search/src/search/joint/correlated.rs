//! The correlated bound: the power and skill of a remaining pair coupled before maximizing.

use super::*;

impl JointBounds {
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
            if self.tightens(joint, cheap, orders) {
                return Ok(true);
            }
            probes += 1;
            if probes == 16 {
                break;
            }
        }
        Ok(false)
    }

    /// Whether a bound `cap` of a probe's payoff sum over `orders` is enough below the bound `preceding` to spend a
    /// stage on it: by 3%, or for a ranked point bound, whose reward weight dwarfs any score gap, by a lower reward or
    /// by 3% of the expected score at an equal reward.
    pub(super) fn tightens(&self, cap: i128, preceding: i128, orders: &[([usize; 5], u128)]) -> bool {
        if self.points.as_ref().is_some_and(|pt| pt.ranked) {
            let mass = orders.iter().map(|&(_, w)| w).sum::<u128>();
            let unit =
                crate::search::certified_search::REWARD_WEIGHT.saturating_mul(mass.try_into().unwrap_or(i128::MAX));
            return match cap.div_euclid(unit).cmp(&preceding.div_euclid(unit)) {
                std::cmp::Ordering::Equal => (cap.rem_euclid(unit) as f64) < preceding.rem_euclid(unit) as f64 * 0.97,
                order => order.is_lt(),
            };
        }
        (cap as f64) < preceding as f64 * 0.97
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
        // The character tables bound the mean score of every completion: a score payoff reads it, a point bound
        // reads its rewards at the least of it and the scale caps below.
        let prefix =
            self.prefix_character.as_ref().and_then(|t| t.upper(self, pool, domain, p, depth, positions, keyed));
        if let Some(cap) = prefix.filter(|_| self.points.is_none()) {
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
            .expect("three scales")
            .min(prefix.unwrap_or(i128::MAX));
        self.points.as_ref().map_or(score_cap, |pt| {
            let mut powers: Vec<_> = remaining.values().map(|r| r.2).collect();
            powers.sort_unstable_by(|a, b| b.cmp(a));
            power_sum += powers.iter().take(5 - depth).sum::<i64>();
            let mut gains: Vec<_> = remaining.values().map(|r| r.3).collect();
            gains.sort_by(|a, b| b.total_cmp(a));
            gain_sum = gains.iter().take(5 - depth).fold(gain_sum, |sum, &g| add_up(sum, g));
            let gain = self.node_order_gain(domain, p, depth, gain_sum, keyed);
            let max_cap = ((power_sum as f64) * add_up(a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
            pt.mean_payoff_with(bonus, &self.other_bonuses(pool, domain, p, depth), score_cap, max_cap)
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
}
