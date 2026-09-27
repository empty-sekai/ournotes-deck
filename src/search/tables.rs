//! Per-request tables: every slot term that depends on one member, or on one member and one snap.
//!
//! Slot total (points) = `a[m] + lead_L(m) + w[m][s]` exactly (see `docs/search.md`), where
//! `a` holds every term that depends on the member alone, `lead_L` the leader-skill term under leader profile `L`,
//! and `w` the snap terms (the snap's own percentage plus its event bonus, and the type link). No snap: `w = 0`.

use std::collections::HashMap;

use crate::bonus::{LeaderProfile, leader_skill_bonuses, member_event_bonus, snap_event_bonus};
use crate::calc::BonusData;
use crate::cards::{MemberView, SongView};
use crate::error::Error;
use crate::power::CardPower;
use crate::search::pool::Pool;

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
    ) -> Result<Tables<'m>, Error> {
        let master = pool.master;
        let calc = &pool.power.calc;
        let n = pool.members.len();
        let mut base = Vec::with_capacity(n);
        let mut a = Vec::with_capacity(n);
        for m in &pool.members {
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
            let mut row = Vec::with_capacity(n);
            for (mi, m) in pool.members.iter().enumerate() {
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
        Ok(Tables { song, base, a, w, snaps: allowed_snaps.to_vec(), wmax, profiles, profile_of, lead })
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
