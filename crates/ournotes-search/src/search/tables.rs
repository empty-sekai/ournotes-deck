//! Per-request tables: every slot term that depends on one member, or on one member and one snap.
//!
//! Slot total (points) = `a[m] + lead_L(m) + w[m][s]` exactly (see `docs/search.md`), where
//! `a` holds every term that depends on the member alone, `lead_L` the leader-skill term under leader profile `L`,
//! and `w` the snap terms (the snap's own percentage plus its event bonus, and the type link). No snap: `w = 0`.

use std::collections::HashMap;

use crate::search::budget::SearchBudget;
use crate::search::power::Allowed;
use ournotes_sim::bonus::{LeaderProfile, leader_skill_bonuses, member_event_bonus, snap_event_bonus};
use ournotes_sim::calc::BonusData;
use ournotes_sim::cards::{MemberView, SongView};
use ournotes_sim::error::Error;
use ournotes_sim::pool::Pool;
use ournotes_sim::power::CardPower;

/// Points of a slot term (every term is a whole number of points on each stat).
#[inline]
pub(crate) fn points(c: CardPower) -> i64 {
    (c.performance + c.technique + c.visual) / 10_000
}

#[inline]
pub(crate) fn term(base: CardPower, pct: CardPower) -> i64 {
    points(base.mul(pct).to_floor())
}

/// Largest BP value the tables accept for a base or a percentage; keeps every product and sum far from wrapping.
const BP_LIMIT: i64 = 1 << 40;
const PCT_LIMIT: i64 = 1 << 20;

fn in_domain(c: CardPower, limit: i64) -> bool {
    c.to_array().iter().all(|&x| (0..limit).contains(&x))
}

#[derive(Clone, Debug)]
pub(crate) struct Tables<'m> {
    pub song: Option<SongView>,
    /// Base `B` of each member (the value every percentage multiplies).
    pub base: Vec<CardPower>,
    /// Member-only terms, points.
    pub a: Vec<i64>,
    /// Snap terms `w[m][s]`, points (every allowed snap of the pool).
    pub w: Vec<Vec<i64>>,
    /// Allowed snaps (pool indexes).
    pub snaps: Vec<usize>,
    /// `max(0, max_s w[m][s])`.
    pub wmax: Vec<i64>,
    pub profiles: Vec<LeaderProfile<'m>>,
    /// Profile index of each member as leader.
    pub profile_of: Vec<usize>,
    /// `lead[p][m]`: exact leader term for simple profiles, an upper bound otherwise.
    pub lead: Vec<Vec<i64>>,
}

impl<'m> Tables<'m> {
    pub fn new(
        pool: &Pool<'m>,
        song: Option<SongView>,
        event: bool,
        allowed_snaps: &[usize],
        budget: SearchBudget,
    ) -> Result<Option<Tables<'m>>, Error> {
        if budget.expired() {
            return Ok(None);
        }
        let master = pool.master;
        let calc = &pool.power.calc;
        let n = pool.members.len();
        let mut base = Vec::with_capacity(n);
        let mut a = Vec::with_capacity(n);
        for m in &pool.members {
            #[cfg(test)]
            super::budget::test_clock::stage("table-member");
            if budget.expired() {
                return Ok(None);
            }
            let bonus = BonusData {
                band_item_bonus: pool.power.band_items.bonus(m),
                leader_skill_bonus: CardPower::EMPTY,
                music_memory_bonus: 0,
                character_memory_bonus: 0,
                member_event_bonus: if event { member_event_bonus(master, &pool.player, m)? } else { CardPower::EMPTY },
                snap_event_bonus: CardPower::EMPTY,
                vip_bonus: pool.power.vip_bonus,
            };
            let slot = m.slot();
            let mu = song.as_ref().map(|s| s.slot());
            let r = calc.slot_power(Some(&slot), None, mu.as_ref(), Some(&bonus))?;
            let b = r.base_power.add(r.character_rank).add(r.character_total_rank).add(r.memory);
            if !in_domain(b, BP_LIMIT) || !in_domain(r.total, BP_LIMIT) {
                return Err(Error::Domain(format!("member card {}: negative or oversized stats", m.id)));
            }
            for p in [bonus.band_item_bonus, r.pct_music_type, r.pct_music_tag, CardPower::bp_single(bonus.vip_bonus)] {
                if !in_domain(p, PCT_LIMIT) {
                    return Err(Error::Domain(format!("member card {}: negative or oversized percentage", m.id)));
                }
            }
            base.push(b);
            a.push(points(r.total));
        }
        let mut w = vec![vec![0i64; allowed_snaps.len()]; n];
        let mut wmax = vec![0i64; n];
        for (j, &si) in allowed_snaps.iter().enumerate() {
            if budget.expired() {
                return Ok(None);
            }
            let s = &pool.snaps[si];
            let ss = s.slot();
            let pct = s.power_bonus_percent.add(if event {
                snap_event_bonus(master, &pool.player, Some(s))?
            } else {
                CardPower::EMPTY
            });
            if !in_domain(pct, PCT_LIMIT) {
                return Err(Error::Domain(format!("snap {}: negative or oversized percentage", s.id)));
            }
            for (mi, m) in pool.members.iter().enumerate() {
                if budget.expired() {
                    return Ok(None);
                }
                let link = calc.type_link_percent(&m.slot(), Some(&ss));
                if !in_domain(link, PCT_LIMIT) {
                    return Err(Error::Domain(format!("snap {}: negative or oversized type link", s.id)));
                }
                let v = term(base[mi], pct) + term(base[mi], link);
                w[mi][j] = v;
                wmax[mi] = wmax[mi].max(v);
            }
        }
        let mut profiles: Vec<LeaderProfile<'m>> = Vec::new();
        let mut key: HashMap<(i64, i64), usize> = HashMap::new();
        let mut profile_of = Vec::with_capacity(n);
        for m in &pool.members {
            if budget.expired() {
                return Ok(None);
            }
            let k = (m.leader_skill_id, m.leader_skill_level);
            let p = match key.get(&k) {
                Some(&p) => p,
                None => {
                    let prof = LeaderProfile::new(master, k.0, k.1)?;
                    if prof.max_abs_value() >= (1 << 28) {
                        return Err(Error::Domain(format!("leader skill {}: oversized effect value", k.0)));
                    }
                    profiles.push(prof);
                    key.insert(k, profiles.len() - 1);
                    profiles.len() - 1
                }
            };
            profile_of.push(p);
        }
        let mut lead = Vec::with_capacity(profiles.len());
        for prof in &profiles {
            if budget.expired() {
                return Ok(None);
            }
            let mut row = Vec::with_capacity(n);
            for (mi, m) in pool.members.iter().enumerate() {
                if budget.expired() {
                    return Ok(None);
                }
                let pct = if prof.simple { prof.simple_percent(m) } else { prof.percent_bound(master, m) };
                if !in_domain(pct, PCT_LIMIT) {
                    return Err(Error::Domain(format!(
                        "leader skill {}: negative or oversized percentage",
                        prof.leader_skill_id
                    )));
                }
                row.push(term(base[mi], pct));
            }
            lead.push(row);
        }
        Ok(Some(Tables { song, base, a, w, snaps: allowed_snaps.to_vec(), wmax, profiles, profile_of, lead }))
    }

    pub(crate) fn member_power_lower_bound(&self, pool: &Pool, profile: usize, member: usize) -> Result<i64, Error> {
        let pct = self.profiles[profile].percent_lower_bound(pool.master, &pool.members[member])?;
        if !pct.to_array().iter().all(|value| value.unsigned_abs() < PCT_LIMIT as u64) {
            return Err(Error::Domain("leader lower bound percentage outside the proven product range".into()));
        }
        Ok(self.a[member] + term(self.base[member], pct))
    }

    /// A/W were checked nonnegative above. Preserve negative-effect profiles when
    /// the target-aware lower bound plus the guaranteed member-only term suffices;
    /// otherwise this solver has no nonnegative-power proof for the entire domain.
    pub fn prove_nonnegative_power(
        &self,
        pool: &Pool,
        leaders: &[usize],
        allowed: &Allowed,
        budget: SearchBudget,
    ) -> Result<bool, Error> {
        let mut lower_by_profile = HashMap::new();
        for &leader in leaders {
            let profile = self.profile_of[leader];
            let character = pool.members[leader].character_id;
            let fixed: Vec<_> = allowed.required.iter().copied().filter(|&m| m != leader).collect();
            let fixed_chars: Vec<_> = fixed.iter().map(|&m| pool.members[m].character_id).collect();
            let picks = 4 - fixed.len(); // prepared leaders already proved feasibility
            let lower = lower_by_profile.entry(profile).or_insert_with(|| vec![None; pool.members.len()]);
            for (member, card) in pool.members.iter().enumerate() {
                if budget.expired() {
                    return Ok(false);
                }
                let available = picks > 0
                    && allowed.members[member]
                    && card.character_id != character
                    && !fixed_chars.contains(&card.character_id);
                if (member == leader || fixed.contains(&member) || available) && lower[member].is_none() {
                    lower[member] = Some(self.member_power_lower_bound(pool, profile, member)?);
                }
            }
            let mut by_character: HashMap<i64, i64> = HashMap::new();
            for (member, card) in pool.members.iter().enumerate() {
                if budget.expired() {
                    return Ok(false);
                }
                if picks == 0
                    || !allowed.members[member]
                    || card.character_id == character
                    || fixed_chars.contains(&card.character_id)
                {
                    continue;
                }
                let value = lower[member].expect("available member checked");
                by_character.entry(card.character_id).and_modify(|best| *best = (*best).min(value)).or_insert(value);
            }
            let mut remaining: Vec<_> = by_character.into_values().collect();
            remaining.sort_unstable();
            let bound = lower[leader].expect("leader checked")
                + fixed.iter().map(|&m| lower[m].expect("required member checked")).sum::<i64>()
                + remaining[..picks].iter().sum::<i64>();
            if bound < 0 {
                return Err(Error::Domain("skip cannot prove nonnegative power for every feasible deck".into()));
            }
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn deck_power_bounds(&self, pool: &Pool, members: [usize; 5]) -> Result<(i64, i64), Error> {
        let profile = self.profile_of[members[2]];
        let lower = members
            .iter()
            .map(|&m| self.member_power_lower_bound(pool, profile, m))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .sum();
        let upper = members.iter().map(|&m| self.a[m] + self.lead[profile][m] + self.wmax[m]).sum();
        Ok((lower, upper))
    }

    /// The exact leader terms of five members in slot order (leader at slot 2).
    pub fn exact_lead(&self, pool: &Pool, members: [usize; 5]) -> Result<[i64; 5], Error> {
        let p = self.profile_of[members[2]];
        if self.profiles[p].simple {
            return Ok(members.map(|m| self.lead[p][m]));
        }
        let views: [&MemberView; 5] = members.map(|m| &pool.members[m]);
        let pct = leader_skill_bonuses(pool.master, &views, self.song.as_ref())?;
        let mut out = [0i64; 5];
        for i in 0..5 {
            if !in_domain(pct[i], PCT_LIMIT) {
                return Err(Error::Domain("leader skill percentage out of range".into()));
            }
            out[i] = term(self.base[members[i]], pct[i]);
        }
        Ok(out)
    }
}
