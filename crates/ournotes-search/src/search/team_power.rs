//! Native power and monotone Skip-score bounds over leader/member-Snap teams.
//! No event payoff, Skip result-rank parameter, or played-live setup is needed.
use super::{Objective, Pool, SearchRequest, budget::SearchBudget, expectation::PhysicalDeck, tables::Tables};
use crate::{clock::Instant, domain::CandidateDomain, types::Metric};
use ournotes_sim::{Error, live::skip::SkipEvaluator};
use std::collections::{HashMap, HashSet};

pub(crate) const SLOTS: [usize; 5] = [2, 0, 1, 3, 4];

pub(crate) fn applies(objective: &Objective, metric: &Metric) -> bool {
    matches!(
        (objective.inner(), metric),
        (Objective::Power { .. }, Metric::Power)
            | (Objective::SkipScore { .. }, Metric::Score | Metric::ScoreAtLeast { .. } | Metric::CappedScore { .. })
    )
}

fn unavailable(message: &str) -> Error {
    Error::Unsupported(format!("team power bound unavailable: {message}"))
}

pub(crate) fn candidates(
    pool: &Pool,
    domain: &CandidateDomain,
    p: &PhysicalDeck,
    depth: usize,
) -> Option<(Vec<usize>, HashSet<i64>)> {
    let used: HashSet<_> = SLOTS[..depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
    let floor = (depth >= 2).then(|| pool.members[p.members[SLOTS[depth - 1]]].id);
    let required: Vec<_> =
        domain.required().iter().copied().filter(|m| !SLOTS[..depth].iter().any(|&s| p.members[s] == *m)).collect();
    if required.len() > 5 - depth
        || required
            .iter()
            .any(|&m| used.contains(&pool.members[m].character_id) || floor.is_some_and(|id| pool.members[m].id <= id))
    {
        return None;
    }
    let available: Vec<_> = domain
        .members()
        .iter()
        .copied()
        .filter(|&m| {
            let card = &pool.members[m];
            !used.contains(&card.character_id)
                && floor.is_none_or(|id| card.id > id)
                && required.iter().all(|&r| r == m || pool.members[r].character_id != card.character_id)
        })
        .collect();
    if available.iter().map(|&m| pool.members[m].character_id).collect::<HashSet<_>>().len() < 5 - depth {
        return None;
    }
    Some((available, required.iter().map(|&m| pool.members[m].character_id).collect()))
}

/// The largest event bonus of any team of five `members` (character, bonus) and up to five of the `snap` bonuses:
/// the five largest per-character maxima of the member bonuses plus the five largest Snap bonuses. A team's members
/// have distinct characters and its Snaps are distinct, so no team sums more. Every bonus is nonnegative.
pub(crate) fn largest_team_bonus(members: impl IntoIterator<Item = (i64, i64)>, snap: &[i64]) -> i64 {
    let mut rows = HashMap::<i64, i64>::new();
    for (character, bonus) in members {
        let best = rows.entry(character).or_insert(0);
        *best = (*best).max(bonus);
    }
    let top = |mut values: Vec<i64>| {
        values.sort_unstable_by(|a, b| b.cmp(a));
        values.into_iter().take(5).sum::<i64>()
    };
    top(rows.into_values().collect()) + top(snap.to_vec())
}

pub(crate) fn sum_rows(rows: &HashMap<i64, i64>, required: &HashSet<i64>, count: usize) -> i64 {
    let fixed: i64 = required.iter().map(|c| rows[c]).sum();
    let mut optional: Vec<_> = rows.iter().filter(|(c, _)| !required.contains(c)).map(|(_, &v)| v).collect();
    optional.sort_unstable_by(|a, b| b.cmp(a));
    fixed + optional.iter().take(count - required.len()).sum::<i64>()
}

pub(crate) struct TeamPowerBounds {
    pub(crate) members: Vec<usize>,
    a: Vec<i64>,
    /// `w[m][column]`: the exact power increment of Snap column `column` paired with member `m`.
    pub(crate) w: Vec<Vec<i64>>,
    lead: Vec<Vec<i64>>,
    profile: Vec<usize>,
    row_max: Vec<i64>,
    maximum: i64,
    skip: Option<SkipEvaluator>,
}

impl TeamPowerBounds {
    pub(crate) fn compile(
        pool: &Pool,
        request: &SearchRequest,
        domain: &CandidateDomain,
        metric: &Metric,
    ) -> Result<Self, Error> {
        if !applies(&request.objective, metric) {
            return Err(unavailable("not a power/monotone Skip objective"));
        }
        Self::tables(pool, request, domain)
    }

    /// The power part alone: per-slot terms of the domain's canonical teams, with a certified nonwrapping power
    /// interval (and, for Skip, a score monotone across it).
    pub(crate) fn tables(pool: &Pool, request: &SearchRequest, domain: &CandidateDomain) -> Result<Self, Error> {
        if !domain.is_feasible() {
            return Err(unavailable("infeasible domain"));
        }
        // Refusal disables this algorithm, never resources or output identities.
        if request.k.saturating_mul(domain.snaps().len().max(1)) > 4096
            || pool.members.len().saturating_mul(pool.members.len().saturating_add(domain.snaps().len() + 1))
                > 1_000_000
        {
            return Err(unavailable("bounded assignment/table work limit"));
        }
        let (song, event, skip) = super::objective_song(pool, &request.objective)?;
        let t = Tables::new(pool, song, event, domain.snaps(), SearchBudget::new(Instant::now(), None)?)?
            .ok_or_else(|| unavailable("power preparation interrupted"))?;
        let leaders = domain.leader().map_or_else(|| domain.members().to_vec(), |m| vec![m]);
        let profiles: HashSet<_> = leaders.iter().map(|&m| t.profile_of[m]).collect();
        let mut max_slot = 0i64;
        for &profile in &profiles {
            for &m in domain.members() {
                if t.member_power_lower_bound(pool, profile, m)? < 0 {
                    return Err(unavailable("negative slot power is not certified"));
                }
                max_slot = max_slot.max(t.a[m] + t.lead[profile][m] + t.wmax[m]);
            }
        }
        let maximum = max_slot
            .checked_mul(5)
            .filter(|v| (0..=i64::from(i32::MAX)).contains(v))
            .ok_or_else(|| unavailable("native power nonwrapping domain not certified"))?;
        let skip = skip.map(|model| model.fast);
        if let Some(skip) = &skip
            && !skip.prove_monotone_through(maximum, || false)?
        {
            return Err(unavailable("Skip monotonicity proof interrupted"));
        }
        let mut members = domain.members().to_vec();
        let priority = |m: usize| t.a[m] + profiles.iter().map(|&p| t.lead[p][m]).max().unwrap_or(0) + t.wmax[m];
        members.sort_unstable_by(|&a, &b| {
            priority(b).cmp(&priority(a)).then_with(|| pool.members[a].id.cmp(&pool.members[b].id))
        });
        Ok(Self { members, a: t.a, w: t.w, lead: t.lead, profile: t.profile_of, row_max: t.wmax, maximum, skip })
    }

    /// On the certified power interval, every supported Skip utility is
    /// nondecreasing. Utility then power then IDs has exactly power then IDs order.
    pub(crate) fn primary_upper(&self, power: i64, metric: &Metric) -> Result<i128, Error> {
        if !(0..=self.maximum).contains(&power) {
            return Err(unavailable("power escaped compiled interval"));
        }
        let score = self.skip.as_ref().map_or(power as i32, |s| s.score(power as i32).0);
        match *metric {
            Metric::Power => Ok(i128::from(power)),
            Metric::Score => Ok(i128::from(score)),
            Metric::ScoreAtLeast { threshold } => Ok(i128::from(score >= threshold)),
            Metric::CappedScore { threshold } => Ok(i128::from(score.min(threshold))),
            _ => Err(unavailable("metric is not monotone in power")),
        }
    }

    /// A fixed member layout and leader has one Snap-independent constant,
    /// including complex leader effects. The checked table edges are its exact
    /// additive Snap increments; its first K bindings therefore dominate all
    /// omitted bindings of that layout, even when the leader bound is loose.
    /// The evaluator, never the upper-bound leader constant, supplies results.
    pub(crate) fn frontier(&self, domain: &CandidateDomain, p: &PhysicalDeck, k: usize) -> Vec<PhysicalDeck> {
        super::matching::best_k_assignments(p.members.map(|m| self.w[m].as_slice()), k)
            .into_iter()
            .map(|(_, snaps)| PhysicalDeck { members: p.members, snaps: snaps.map(|s| s.map(|j| domain.snaps()[j])) })
            .collect()
    }

    pub(crate) fn upper(&self, pool: &Pool, domain: &CandidateDomain, p: &PhysicalDeck, depth: usize) -> Option<i64> {
        let (available, required) = candidates(pool, domain, p, depth)?;
        let remaining = 5 - depth;
        let leaders = if depth > 0 {
            vec![p.members[2]]
        } else {
            domain.leader().map_or_else(|| domain.members().to_vec(), |m| vec![m])
        };
        let profiles: HashSet<_> = leaders.iter().map(|&m| self.profile[m]).collect();
        let mut maximum = 0;
        for profile in profiles {
            let fixed_base: i64 =
                SLOTS[..depth].iter().map(|&s| self.a[p.members[s]] + self.lead[profile][p.members[s]]).sum();
            if remaining == 0 {
                maximum = maximum
                    .max(fixed_base + super::matching::best_assignment(p.members.map(|m| self.w[m].as_slice())).0);
                continue;
            }
            let mut base = HashMap::<i64, i64>::new();
            let mut paired = HashMap::<i64, i64>::new();
            for &m in &available {
                let c = pool.members[m].character_id;
                let b = self.a[m] + self.lead[profile][m];
                base.entry(c).and_modify(|v| *v = (*v).max(b)).or_insert(b);
                let w = b + self.row_max[m];
                paired.entry(c).and_modify(|v| *v = (*v).max(w)).or_insert(w);
            }
            let paired = fixed_base
                + SLOTS[..depth].iter().map(|&s| self.row_max[p.members[s]]).sum::<i64>()
                + sum_rows(&paired, &required, remaining);
            let mut resources: Vec<_> = (0..domain.snaps().len())
                .map(|column| {
                    SLOTS[..depth]
                        .iter()
                        .map(|&s| p.members[s])
                        .chain(available.iter().copied())
                        .map(|m| self.w[m][column])
                        .max()
                        .unwrap_or(0)
                        .max(0)
                })
                .collect();
            resources.sort_unstable_by(|a, b| b.cmp(a));
            let resource = fixed_base + sum_rows(&base, &required, remaining) + resources.iter().take(5).sum::<i64>();
            maximum = maximum.max(paired.min(resource));
        }
        Some(maximum)
    }

    pub(crate) fn least_key(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> Option<([i64; 5], [Option<i64>; 5])> {
        if depth == 0 {
            return None;
        }
        let mut deck = *p;
        for at in depth..5 {
            let (mut available, _) = candidates(pool, domain, &deck, at)?;
            available.sort_unstable_by_key(|&m| pool.members[m].id);
            let member = available.into_iter().find(|&m| {
                deck.members[SLOTS[at]] = m;
                candidates(pool, domain, &deck, at + 1).is_some()
            })?;
            deck.members[SLOTS[at]] = member;
        }
        Some((deck.members.map(|m| pool.members[m].id), [None; 5]))
    }

    pub(crate) fn seeds(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        mut cancelled: impl FnMut() -> bool,
    ) -> Vec<PhysicalDeck> {
        let leaders = domain.leader().map_or_else(|| self.members.clone(), |m| vec![m]);
        let mut seeds = Vec::new();
        for leader in leaders {
            if cancelled() {
                break;
            }
            let profile = self.profile[leader];
            let value = |m: usize| self.a[m] + self.lead[profile][m] + self.row_max[m];
            let mut selected = vec![leader];
            let mut chars = HashSet::from([pool.members[leader].character_id]);
            let mut valid = true;
            for &m in domain.required() {
                if m == leader {
                    continue;
                }
                if !chars.insert(pool.members[m].character_id) {
                    valid = false;
                    break;
                }
                selected.push(m);
            }
            if !valid || selected.len() > 5 {
                continue;
            }
            let mut groups = HashMap::<i64, usize>::new();
            for &m in &self.members {
                let c = pool.members[m].character_id;
                if chars.contains(&c) {
                    continue;
                }
                groups
                    .entry(c)
                    .and_modify(|old| {
                        if value(m) > value(*old)
                            || (value(m) == value(*old) && pool.members[m].id < pool.members[*old].id)
                        {
                            *old = m;
                        }
                    })
                    .or_insert(m);
            }
            let mut rest: Vec<_> = groups.into_values().collect();
            rest.sort_unstable_by(|&a, &b| {
                value(b).cmp(&value(a)).then_with(|| pool.members[a].id.cmp(&pool.members[b].id))
            });
            selected.extend(rest.into_iter().take(5 - selected.len()));
            if selected.len() != 5 {
                continue;
            }
            let mut others: Vec<_> = selected.into_iter().filter(|&m| m != leader).collect();
            others.sort_unstable_by_key(|&m| pool.members[m].id);
            let p = PhysicalDeck { members: [others[0], others[1], leader, others[2], others[3]], snaps: [None; 5] };
            seeds.extend(self.frontier(domain, &p, 1));
        }
        seeds
    }
}

#[cfg(test)]
mod tests {
    use super::largest_team_bonus;

    /// Every team of five distinct-character members and at most five distinct Snaps, by enumeration.
    fn enumerated(members: &[(i64, i64)], snaps: &[i64]) -> Option<i64> {
        let n = members.len();
        let mut best_members = None;
        for mask in 0u32..1 << n {
            if mask.count_ones() != 5 {
                continue;
            }
            let chosen: Vec<_> = (0..n).filter(|&i| mask & (1 << i) != 0).map(|i| members[i]).collect();
            let mut characters: Vec<_> = chosen.iter().map(|&(c, _)| c).collect();
            characters.sort_unstable();
            characters.dedup();
            if characters.len() == 5 {
                let sum = chosen.iter().map(|&(_, b)| b).sum::<i64>();
                best_members = Some(best_members.map_or(sum, |b: i64| b.max(sum)));
            }
        }
        let mut best_snaps = 0;
        for mask in 0u32..1 << snaps.len() {
            if mask.count_ones() <= 5 {
                let sum = (0..snaps.len()).filter(|&j| mask & (1 << j) != 0).map(|j| snaps[j]).sum::<i64>();
                best_snaps = best_snaps.max(sum);
            }
        }
        best_members.map(|m| m + best_snaps)
    }

    #[test]
    fn largest_team_bonus_equals_the_best_enumerated_team() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut checked = 0;
        for _ in 0..400 {
            let n = 5 + next(8) as usize;
            let characters = 3 + next(6) as i64;
            let members: Vec<_> = (0..n).map(|_| (next(characters as u64) as i64, next(4) as i64 * 2500)).collect();
            let snaps: Vec<_> = (0..next(9)).map(|_| next(5) as i64 * 700).collect();
            let Some(expected) = enumerated(&members, &snaps) else { continue };
            assert_eq!(largest_team_bonus(members.iter().copied(), &snaps), expected, "{members:?} {snaps:?}");
            checked += 1;
        }
        assert!(checked > 100);
    }
}
