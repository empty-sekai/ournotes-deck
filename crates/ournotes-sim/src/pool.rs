//! The resolved card pool of one roster.

use std::collections::HashSet;

use crate::calc::DeckPower;
use crate::cards::{MemberView, Player, Roster, SnapView, SongView};
use crate::deck::PowerContext;
use crate::error::Error;
use crate::master::Master;

/// The largest pool the search indexes (member cards or snaps).
pub const MAX_POOL: usize = u16::MAX as usize;

/// A roster resolved against the master: every card's stats, the player bonuses that do not depend on the deck.
#[derive(Clone, Debug)]
pub struct Pool<'m> {
    pub master: &'m Master,
    pub player: Player,
    pub members: Vec<MemberView>,
    pub snaps: Vec<SnapView>,
    pub(crate) power_event_snapshot: Vec<i64>,
    pub power: PowerContext<'m>,
}

/// A deck by pool indexes: members in slot order (slot 2 is the leader), the snap of each slot, and the
/// performance order (a permutation of the slots: `performance_order[k]` is the slot that performs at position k).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Deck {
    pub members: [usize; 5],
    pub snaps: [Option<usize>; 5],
    pub performance_order: [usize; 5],
}

impl<'m> Pool<'m> {
    /// Resolves every card. Each member card id and snap id may appear once.
    pub fn new(master: &'m Master, roster: &Roster) -> Result<Pool<'m>, Error> {
        let mut seen = HashSet::new();
        for m in &roster.members {
            if !seen.insert(m.id) {
                return Err(Error::Input(format!("member card {} listed twice", m.id)));
            }
        }
        seen.clear();
        for s in &roster.snaps {
            if !seen.insert(s.id) {
                return Err(Error::Input(format!("snap {} listed twice", s.id)));
            }
        }
        if roster.members.len() > MAX_POOL || roster.snaps.len() > MAX_POOL {
            return Err(Error::Capacity(format!("at most {MAX_POOL} member cards and {MAX_POOL} snaps")));
        }
        let mut player = roster.player.clone();
        if player.owned_member_card_ids.is_none() {
            player.owned_member_card_ids = Some(roster.members.iter().map(|m| m.id).collect());
        }
        if player.owned_support_card_ids.is_none() {
            player.owned_support_card_ids = Some(roster.snaps.iter().map(|s| s.id).collect());
        }
        let members =
            roster.members.iter().map(|m| MemberView::resolve(master, &player, m)).collect::<Result<Vec<_>, _>>()?;
        let snaps = roster.snaps.iter().map(|s| SnapView::resolve(master, s)).collect::<Result<Vec<_>, _>>()?;
        let power = PowerContext::new(master, &player)?;
        Ok(Pool { master, power_event_snapshot: player.events.clone(), player, members, snaps, power })
    }

    pub fn member_index(&self, id: i64) -> Option<usize> {
        self.members.iter().position(|m| m.id == id)
    }

    pub fn snap_index(&self, id: i64) -> Option<usize> {
        self.snaps.iter().position(|s| s.id == id)
    }

    /// A deck from card ids: members in slot order (slot 2 is the leader), the snap of each slot, and the
    /// performance order (`performance_order[k]` is the slot performing at position k).
    pub fn deck(
        &self,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
        performance_order: [usize; 5],
    ) -> Result<Deck, Error> {
        let mut m = [0usize; 5];
        for (i, id) in members.iter().enumerate() {
            m[i] =
                self.member_index(*id).ok_or_else(|| Error::Input(format!("member card {id} is not in the roster")))?;
        }
        let mut s = [None; 5];
        for (i, id) in snaps.iter().enumerate() {
            if let Some(id) = id {
                s[i] =
                    Some(self.snap_index(*id).ok_or_else(|| Error::Input(format!("snap {id} is not in the roster")))?);
            }
        }
        let deck = Deck { members: m, snaps: s, performance_order };
        self.check_deck(&deck)?;
        Ok(deck)
    }

    /// The song view of a live music id.
    pub fn song(&self, music_id: i64) -> Result<SongView, Error> {
        self.master
            .live_music(music_id)
            .map(SongView::from_row)
            .ok_or_else(|| Error::Input(format!("unknown live music {music_id}")))
    }

    /// Checks the deck rules: five distinct characters, each snap at most once.
    pub fn check_deck(&self, deck: &Deck) -> Result<(), Error> {
        let mut chars = HashSet::new();
        for &m in &deck.members {
            let v = self.members.get(m).ok_or_else(|| Error::Input(format!("member index {m}")))?;
            if !chars.insert(v.character_id) {
                return Err(Error::Input(format!("character {} appears twice", v.character_id)));
            }
        }
        let mut snaps = HashSet::new();
        for s in deck.snaps.iter().flatten() {
            if *s >= self.snaps.len() || !snaps.insert(*s) {
                return Err(Error::Input(format!("snap index {s} invalid or repeated")));
            }
        }
        let mut order = deck.performance_order;
        order.sort_unstable();
        if order != [0, 1, 2, 3, 4] {
            return Err(Error::Input("performance order is not a permutation of the slots".into()));
        }
        Ok(())
    }

    /// Deck power with the game's own evaluation path.
    pub fn deck_power(&self, deck: &Deck, song: Option<&SongView>, event: bool) -> Result<DeckPower, Error> {
        self.check_deck(deck)?;
        let m = deck.members.map(|i| &self.members[i]);
        let s = deck.snaps.map(|s| s.map(|i| &self.snaps[i]));
        self.power.deck_power(&self.player, &m, &s, song, event)
    }
}
