//! Card stats from master rows and the per-slot / deck power arithmetic.
//!
//! The slot computation takes plain views of its inputs ([`SlotMember`], [`SlotSupport`], [`SlotMusic`],
//! [`BonusData`]) so it can be driven from resolved cards or from arbitrary values.

use std::collections::HashMap;

use crate::error::Error;
use crate::master::{LevelRow, Master};
use crate::num::floor_to_i32;
use crate::power::CardPower;

/// The "all types" value of a song's or card's music type.
pub const MUSIC_TYPE_ALL: i64 = 99;

/// Member level stats: per stat `floor(f32(rate * max) / 10000f)` with a 64-bit product, as whole points.
pub fn member_level_power(rates: [i64; 3], max: [i64; 3]) -> CardPower {
    let one = |rate: i64, mx: i64| floor_to_i32(rate.wrapping_mul(mx) as f32 / 10000f32) as i64;
    CardPower::points(one(rates[0], max[0]), one(rates[1], max[1]), one(rates[2], max[2]))
}

/// Awake bonus: per stat `floor((f32(rate) / 10000f) * f32(max))`, as whole points.
pub fn member_awake_bonus(rates: [i64; 3], max: [i64; 3]) -> CardPower {
    let one = |rate: i64, mx: i64| floor_to_i32((rate as f32 / 10000f32) * mx as f32) as i64;
    CardPower::points(one(rates[0], max[0]), one(rates[1], max[1]), one(rates[2], max[2]))
}

/// Rank bonus: per stat `floor(f32(rate * max) / 10000f)` with a wrapping 32-bit product, as whole points.
pub fn member_rank_bonus(rates: [i64; 3], max: [i64; 3]) -> CardPower {
    let one = |rate: i64, mx: i64| floor_to_i32((rate as i32).wrapping_mul(mx as i32) as f32 / 10000f32) as i64;
    CardPower::points(one(rates[0], max[0]), one(rates[1], max[1]), one(rates[2], max[2]))
}

/// Snap power bonus percentage: per stat `floor(f32(rate * max) / 10000f)` with a wrapping 32-bit product, in BP
/// (a percentage).
pub fn support_level_percent(rates: [i64; 3], max: [i64; 3]) -> CardPower {
    let one = |rate: i64, mx: i64| floor_to_i32((rate as i32).wrapping_mul(mx as i32) as f32 / 10000f32) as i64;
    CardPower::bp(one(rates[0], max[0]), one(rates[1], max[1]), one(rates[2], max[2]))
}

pub(crate) fn level_rates(r: &LevelRow) -> [i64; 3] {
    [r.performance_rate, r.technic_rate, r.visual_rate]
}

/// The member-card values a slot reads.
#[derive(Clone, Copy, Debug)]
pub struct SlotMember<'a> {
    /// The card's own power (level, awake, rank and memory terms).
    pub power: CardPower,
    pub character_rank: i64,
    pub character_total_rank: i64,
    /// Card type, compared with the snap's for the type link.
    pub card_type: i64,
    /// Music type, compared with the song's (the card type for member cards).
    pub music_type: i64,
    pub best_music_tag_ids: Option<&'a [i64]>,
    pub rank_group: i64,
    pub rank: i64,
}

/// The snap values a slot reads.
#[derive(Clone, Copy, Debug)]
pub struct SlotSupport {
    /// The snap's power bonus percentage (BP).
    pub power_bonus_percent: CardPower,
    pub card_type: i64,
    pub rank_group: i64,
    pub rank: i64,
}

/// The song values a slot reads.
#[derive(Clone, Copy, Debug)]
pub struct SlotMusic<'a> {
    pub music_type: i64,
    pub best_music_tag_ids: Option<&'a [i64]>,
    /// Extra type bonus rate of the song (0 for regular songs).
    pub type_bonus_rate: i64,
    /// Extra tag bonus rate of the song (0 for regular songs).
    pub tag_bonus_rate: i64,
}

/// Per-slot player bonuses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BonusData {
    pub band_item_bonus: CardPower,
    pub leader_skill_bonus: CardPower,
    pub music_memory_bonus: i64,
    pub character_memory_bonus: i64,
    pub member_event_bonus: CardPower,
    pub snap_event_bonus: CardPower,
    pub vip_bonus: i64,
}

/// Every term of one slot, in the order they are added, plus the percentages used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotPower {
    pub base_power: CardPower,
    pub character_rank: CardPower,
    pub character_total_rank: CardPower,
    pub support: CardPower,
    pub band_item: CardPower,
    pub type_link: CardPower,
    pub music_type: CardPower,
    pub music_tag: CardPower,
    pub leader_skill: CardPower,
    pub memory: CardPower,
    pub vip: CardPower,
    pub pct_support: CardPower,
    pub pct_type_link: CardPower,
    pub pct_music_type: CardPower,
    pub pct_music_tag: CardPower,
    pub total: CardPower,
}

impl SlotPower {
    /// The 16 fields in declaration order.
    pub fn fields(&self) -> [CardPower; 16] {
        [
            self.base_power,
            self.character_rank,
            self.character_total_rank,
            self.support,
            self.band_item,
            self.type_link,
            self.music_type,
            self.music_tag,
            self.leader_skill,
            self.memory,
            self.vip,
            self.pct_support,
            self.pct_type_link,
            self.pct_music_type,
            self.pct_music_tag,
            self.total,
        ]
    }
}

/// The five slots and their sum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeckPower {
    pub slots: [SlotPower; 5],
    pub total: CardPower,
}

impl DeckPower {
    /// The deck power as displayed and as used by the live: the total's point sum.
    pub fn power(&self) -> i32 {
        self.total.total()
    }
}

/// The tables the slot computation reads, prepared once from the master.
#[derive(Clone, Debug)]
pub struct PowerCalculator {
    pub music_type_base: i64,
    pub music_tag_base: i64,
    pub type_link_base: i64,
    /// `(rank, bonus)` in ascending rank order.
    pub character_rank_bonus: Vec<(i64, i64)>,
    /// `(total rank, bonus)` in ascending total-rank order.
    pub character_total_rank_bonus: Vec<(i64, i64)>,
    /// `(group, rank)` -> `(music type bonus rate, music tag bonus rate)`.
    pub member_rank: HashMap<(i64, i64), (i64, i64)>,
    /// `(group, rank)` -> type link bonus rate.
    pub support_rank: HashMap<(i64, i64), i64>,
}

fn parse_i64(master: &Master, key: &str) -> Result<i64, Error> {
    let v = master.parameter(key).ok_or_else(|| Error::Master(format!("MasterParameter {key} missing")))?;
    v.trim().parse::<i64>().map_err(|_| Error::Master(format!("MasterParameter {key} = {v:?} is not an integer")))
}

impl PowerCalculator {
    pub fn from_master(master: &Master) -> Result<PowerCalculator, Error> {
        let mut cr: Vec<(i64, i64)> = master.character_ranks.iter().map(|r| (r.rank, r.bonus)).collect();
        cr.sort_by_key(|x| x.0);
        let mut ctr: Vec<(i64, i64)> = master.character_total_ranks.iter().map(|r| (r.total_rank, r.bonus)).collect();
        ctr.sort_by_key(|x| x.0);
        let mut member_rank = HashMap::new();
        for r in &master.member_card_ranks {
            member_rank.insert((r.group, r.rank), (r.music_type_bonus_rate, r.music_tag_bonus_rate));
        }
        let mut support_rank = HashMap::new();
        for r in &master.support_card_ranks {
            support_rank.insert((r.group, r.rank), r.card_type_link_bonus_rate);
        }
        Ok(PowerCalculator {
            music_type_base: parse_i64(master, "music_type_base_bonus_rate")?,
            music_tag_base: parse_i64(master, "music_tag_base_bonus_rate")?,
            type_link_base: parse_i64(master, "type_link_base_bonus_rate")?,
            character_rank_bonus: cr,
            character_total_rank_bonus: ctr,
            member_rank,
            support_rank,
        })
    }

    fn last_le(pairs: &[(i64, i64)], key: i64) -> i64 {
        let mut v = 0;
        for &(k, b) in pairs {
            if k <= key {
                v = b;
            }
        }
        v
    }

    /// Flat points of a character rank: the bonus of the last rank row `<= rank`, 0 if none.
    pub fn character_rank_bonus(&self, rank: i64) -> CardPower {
        CardPower::points_single(Self::last_le(&self.character_rank_bonus, rank))
    }

    /// Flat points of the total character rank.
    pub fn character_total_rank_bonus(&self, total_rank: i64) -> CardPower {
        CardPower::points_single(Self::last_le(&self.character_total_rank_bonus, total_rank))
    }

    /// Type link percentage: same card type and a snap rank row give `base + rate`, else empty.
    pub fn type_link_percent(&self, member: &SlotMember, support: Option<&SlotSupport>) -> CardPower {
        let Some(s) = support else { return CardPower::EMPTY };
        if member.card_type != s.card_type {
            return CardPower::EMPTY;
        }
        match self.support_rank.get(&(s.rank_group, s.rank)) {
            Some(&rate) => CardPower::bp_single(self.type_link_base.wrapping_add(rate)),
            None => CardPower::EMPTY,
        }
    }

    /// One slot. `member == None` gives an empty result; a member rank without a rank row is a game error.
    pub fn slot_power(
        &self,
        member: Option<&SlotMember>,
        support: Option<&SlotSupport>,
        music: Option<&SlotMusic>,
        bonus: Option<&BonusData>,
    ) -> Result<SlotPower, Error> {
        let Some(m) = member else { return Ok(SlotPower::default()) };
        let default_bonus = BonusData::default();
        let pb = bonus.unwrap_or(&default_bonus);
        let p0 = m.power;
        let base = p0.add(p0.mul(pb.member_event_bonus).to_floor());
        let &(rank_type_rate, rank_tag_rate) = self
            .member_rank
            .get(&(m.rank_group, m.rank))
            .ok_or_else(|| Error::Game(format!("no member rank bonus for group {} rank {}", m.rank_group, m.rank)))?;
        let cr = self.character_rank_bonus(m.character_rank);
        let ctr = self.character_total_rank_bonus(m.character_total_rank);
        let memory_points = (pb.character_memory_bonus as i32).wrapping_add(pb.music_memory_bonus as i32);
        let mem = CardPower::points_single(memory_points as i64);
        let b = base.add(cr).add(ctr).add(mem);
        let pct_sup = support.map_or(CardPower::EMPTY, |s| s.power_bonus_percent).add(pb.snap_event_bonus);
        let sup = b.mul(pct_sup).to_floor();
        let band = b.mul(pb.band_item_bonus).to_floor();
        let lead = b.mul(pb.leader_skill_bonus).to_floor();
        let pct_link = self.type_link_percent(m, support);
        let link = b.mul(pct_link).to_floor();
        let mut pct_mtype = CardPower::EMPTY;
        let mut pct_mtag = CardPower::EMPTY;
        if let Some(mu) = music {
            let (mt, st) = (m.music_type, mu.music_type);
            if mt == MUSIC_TYPE_ALL || st == MUSIC_TYPE_ALL || mt == st {
                pct_mtype = CardPower::bp_single(
                    self.music_type_base.wrapping_add(rank_type_rate).wrapping_add(mu.type_bonus_rate),
                );
            }
            if let (Some(mtags), Some(stags)) = (m.best_music_tag_ids, mu.best_music_tag_ids) {
                if mtags.iter().any(|a| stags.contains(a)) {
                    pct_mtag = CardPower::bp_single(
                        self.music_tag_base.wrapping_add(rank_tag_rate).wrapping_add(mu.tag_bonus_rate),
                    );
                }
            }
        }
        let mtype = b.mul(pct_mtype).to_floor();
        let mtag = b.mul(pct_mtag).to_floor();
        let vip = b.mul(CardPower::bp_single(pb.vip_bonus)).to_floor();
        let total = base.add(cr).add(ctr).add(sup).add(band).add(link).add(mtype).add(mtag).add(lead).add(mem).add(vip);
        Ok(SlotPower {
            base_power: base,
            character_rank: cr,
            character_total_rank: ctr,
            support: sup,
            band_item: band,
            type_link: link,
            music_type: mtype,
            music_tag: mtag,
            leader_skill: lead,
            memory: mem,
            vip,
            pct_support: pct_sup,
            pct_type_link: pct_link,
            pct_music_type: pct_mtype,
            pct_music_tag: pct_mtag,
            total,
        })
    }

    /// Five slots; slot `i` uses `supports[i]`; every term is summed over the slots.
    pub fn deck_power(
        &self,
        members: [&SlotMember; 5],
        supports: [Option<&SlotSupport>; 5],
        bonuses: &[BonusData; 5],
        music: Option<&SlotMusic>,
    ) -> Result<DeckPower, Error> {
        let mut out = DeckPower::default();
        for i in 0..5 {
            let r = self.slot_power(Some(members[i]), supports[i], music, Some(&bonuses[i]))?;
            out.total = out.total.add(r.total);
            out.slots[i] = r;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_scalar_points_wrap_before_becoming_power() {
        let calc = PowerCalculator {
            music_type_base: 0,
            music_tag_base: 0,
            type_link_base: 0,
            character_rank_bonus: vec![],
            character_total_rank_bonus: vec![],
            member_rank: HashMap::from([((0, 0), (0, 0))]),
            support_rank: HashMap::new(),
        };
        let member = SlotMember {
            power: CardPower::EMPTY,
            character_rank: 0,
            character_total_rank: 0,
            card_type: 0,
            music_type: 0,
            best_music_tag_ids: None,
            rank_group: 0,
            rank: 0,
        };
        for (a, b, expected) in [
            (i32::MAX as i64, 1, i32::MIN as i64),
            (i32::MIN as i64, -1, i32::MAX as i64),
            (17, 23, 40),
            (0, 0, 0),
            (i32::MAX as i64, i32::MAX as i64, -2),
            (i32::MIN as i64, i32::MIN as i64, 0),
        ] {
            let bonus = BonusData { character_memory_bonus: a, music_memory_bonus: b, ..Default::default() };
            let result = calc.slot_power(Some(&member), None, None, Some(&bonus)).unwrap();
            assert_eq!(result.memory, CardPower::points_single(expected));
        }
    }
}
