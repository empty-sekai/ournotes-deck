//! The event-bonus arithmetic the payoff bounds share: audited native products and card-bonus sums, and the largest
//! bonus a team can reach. Every value is a nonnegative Int32 unless a function says otherwise; the audits refuse a
//! computation whose native intermediate would leave that range, so a bound never wraps into a smaller number.
use ournotes_sim::{
    Error,
    event::{self, EventCard},
    master::EventEffectRow,
};
use std::collections::{HashMap, HashSet};

/// The refusal of a payoff computation whose native intermediates would leave the supported range.
pub(crate) fn unavailable(message: &str) -> Error {
    Error::Unsupported(format!("deck payoff bound unavailable: {message}"))
}

pub(super) fn nonnegative_i32(value: i64) -> Result<i64, Error> {
    if (0..=i64::from(i32::MAX)).contains(&value) {
        Ok(value)
    } else {
        Err(unavailable("negative or wrapping native Int32 value"))
    }
}

/// Audit the native order `value * (bonus + 10000) * rate / 10000`.
/// The first product must fit even when the final rate is zero.
pub(super) fn point_product(value: i64, bonus: i64, rate: i64) -> Result<i128, Error> {
    nonnegative_i32(value)?;
    nonnegative_i32(bonus)?;
    nonnegative_i32(rate)?;
    let base = nonnegative_i32(bonus.checked_add(10000).ok_or_else(|| unavailable("bonus overflow"))?)?;
    let first = nonnegative_i32(value.checked_mul(base).ok_or_else(|| unavailable("first product overflow"))?)?;
    let last = nonnegative_i32(first.checked_mul(rate).ok_or_else(|| unavailable("second product overflow"))?)?;
    Ok(i128::from(last / 10000))
}

/// Challenge-point earnings have no event bonus or 10000-scale intermediate.
pub(super) fn challenge_product(value: i64, rate: i64) -> Result<i128, Error> {
    nonnegative_i32(value)?;
    nonnegative_i32(rate)?;
    let product = value.checked_mul(rate).ok_or_else(|| unavailable("CP product overflow"))?;
    Ok(i128::from(nonnegative_i32(product)?))
}

/// Reproduce the native card's sum of `bonus_type` effects only after proving every cast/addition safe.
pub(super) fn card_bonus(effects: &[&EventEffectRow], card: EventCard<'_>, bonus_type: i64) -> Result<i64, Error> {
    let mut sum = 0i64;
    for effect in effects.iter().filter(|e| e.event_bonus_type == bonus_type) {
        let (hit, rank) = match card {
            EventCard::Member(m) => (event::is_member_target(effect, m)?, m.rank),
            EventCard::Snap(s) => (event::is_snap_target(effect, s), s.rank),
        };
        if hit {
            let value = nonnegative_i32(event::rank_effect_value(effect, rank)?)?;
            sum = nonnegative_i32(sum + value)?;
        }
    }
    Ok(sum)
}

/// The largest event bonus of any team of five `members` (character, bonus) and up to five of the `snap` bonuses:
/// the five largest per-character maxima of the member bonuses plus the five largest Snap bonuses. A team's members
/// have distinct characters and its Snaps are distinct, so no team sums more. Every bonus is nonnegative.
pub(super) fn largest_team_bonus(members: impl IntoIterator<Item = (i64, i64)>, snap: &[i64]) -> i64 {
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

/// The largest sum of `count` completed characters' bonuses, each a row keyed by character: the required characters
/// plus the largest optional rows. Callers pass the rows of the characters a prefix can still use.
pub(super) fn sum_rows(rows: &HashMap<i64, i64>, required: &HashSet<i64>, count: usize) -> i64 {
    let fixed: i64 = required.iter().map(|c| rows[c]).sum();
    let mut optional: Vec<_> = rows.iter().filter(|(c, _)| !required.contains(c)).map(|(_, &v)| v).collect();
    optional.sort_unstable_by(|a, b| b.cmp(a));
    fixed + optional.iter().take(count - required.len()).sum::<i64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_intermediate_wrapping_is_refused_even_at_zero_rate() {
        assert!(point_product(i64::from(i32::MAX), 0, 0).is_err());
        assert!(point_product(1, i64::from(i32::MAX), 0).is_err());
        assert!(point_product(1, 0, i64::from(i32::MAX)).is_err());
        assert!(point_product(-1, 0, 1).is_err());
        assert_eq!(point_product(100, 20000, 0).unwrap(), 0);
        assert_eq!(challenge_product(i64::from(i32::MAX), 1).unwrap(), i128::from(i32::MAX));
        assert_eq!(challenge_product(i64::from(i32::MAX), 0).unwrap(), 0);
        assert!(challenge_product(i64::from(i32::MAX), 2).is_err());
    }

    #[test]
    fn effect_casts_and_each_addition_must_fit_before_native_wrapping() {
        let member =
            event::EventMember { id: 1, character_id: 1, band_id: Some(1), card_type: 1, tags: vec![], rank: 1 };
        let mut effect = EventEffectRow {
            event_bonus_type: event::EVENT_POINT,
            resource_type_constraint: 2,
            rank1_effect_value: i64::from(i32::MAX),
            ..Default::default()
        };
        let card = EventCard::Member(&member);
        assert_eq!(card_bonus(&[&effect], card, event::EVENT_POINT).unwrap(), i64::from(i32::MAX));
        assert_eq!(card_bonus(&[&effect], card, event::EVENT_ITEM).unwrap(), 0, "another bonus type is not read");
        assert!(card_bonus(&[&effect, &effect], card, event::EVENT_POINT).is_err());
        effect.rank1_effect_value = 1i64 << 32;
        assert!(card_bonus(&[&effect], card, event::EVENT_POINT).is_err());
        effect.rank1_effect_value = -1;
        assert!(card_bonus(&[&effect], card, event::EVENT_POINT).is_err());
    }

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
