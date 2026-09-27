//! Deck power of a whole deck: the standard per-slot player bonuses and the five-slot sum.

use crate::bonus::{BandItemMaps, leader_skill_bonuses, member_event_bonus, snap_event_bonus, vip_bonus};
use crate::calc::{BonusData, DeckPower, PowerCalculator};
use crate::cards::{MemberView, Player, SnapView, SongView};
use crate::error::Error;
use crate::master::Master;

/// Everything the deck power needs that does not depend on the deck.
#[derive(Clone, Debug)]
pub struct PowerContext<'m> {
    pub master: &'m Master,
    pub calc: PowerCalculator,
    pub band_items: BandItemMaps<'m>,
    pub vip_bonus: i64,
}

impl<'m> PowerContext<'m> {
    pub fn new(master: &'m Master, player: &Player) -> Result<PowerContext<'m>, Error> {
        Ok(PowerContext {
            master,
            calc: PowerCalculator::from_master(master)?,
            band_items: BandItemMaps::build(master, player)?,
            vip_bonus: vip_bonus(master, player),
        })
    }

    /// The per-slot bonuses: band items, leader skill, VIP and, when `event` is set, the event parameter bonuses.
    pub fn player_bonuses(
        &self,
        player: &Player,
        members: &[&MemberView; 5],
        snaps: &[Option<&SnapView>; 5],
        music: Option<&SongView>,
        event: bool,
    ) -> Result<[BonusData; 5], Error> {
        let leader = leader_skill_bonuses(self.master, members, music)?;
        let mut out = [BonusData::default(); 5];
        for i in 0..5 {
            out[i] = BonusData {
                band_item_bonus: self.band_items.bonus(members[i]),
                leader_skill_bonus: leader[i],
                music_memory_bonus: 0,
                character_memory_bonus: 0,
                member_event_bonus: if event {
                    member_event_bonus(self.master, player, members[i])?
                } else {
                    crate::power::CardPower::EMPTY
                },
                snap_event_bonus: if event {
                    snap_event_bonus(self.master, player, snaps[i])?
                } else {
                    crate::power::CardPower::EMPTY
                },
                vip_bonus: self.vip_bonus,
            };
        }
        Ok(out)
    }

    /// Deck power of members in slot order (slot 2 is the leader) and their snaps.
    pub fn deck_power(
        &self,
        player: &Player,
        members: &[&MemberView; 5],
        snaps: &[Option<&SnapView>; 5],
        music: Option<&SongView>,
        event: bool,
    ) -> Result<DeckPower, Error> {
        let bonuses = self.player_bonuses(player, members, snaps, music, event)?;
        let ms = members.map(|m| m.slot());
        let ss = snaps.map(|s| s.map(|s| s.slot()));
        let mu = music.map(|m| m.slot());
        self.calc.deck_power(
            [&ms[0], &ms[1], &ms[2], &ms[3], &ms[4]],
            [ss[0].as_ref(), ss[1].as_ref(), ss[2].as_ref(), ss[3].as_ref(), ss[4].as_ref()],
            &bonuses,
            mu.as_ref(),
        )
    }
}
