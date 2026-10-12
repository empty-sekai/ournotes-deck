//! The solo rank bonus of a range and the network Gekisou ranking.

use super::*;

/// The song's mission pattern: 0 when a mission is missing, 1 all the same, 2 all different, 3 otherwise.
pub(crate) fn mission_pattern(t0: i64, t1: i64, t2: i64) -> i64 {
    if t0 == 0 || t1 == 0 || t2 == 0 {
        return 0;
    }
    if t0 == t1 {
        return if t0 == t2 { 1 } else { 3 };
    }
    if t1 != t2 && t0 != t2 {
        return 2;
    }
    3
}

/// Rank bonus percentages `[range][rank - 1]` of a mission pattern.
pub(crate) fn ranking_factors(master: &Master, pattern: i64) -> Result<[[i64; 5]; 3], Error> {
    let mut f = [[0i64; 5]; 3];
    for r in &master.gekisou_ranking_score_bonuses {
        let (c, k) = (r.count.wrapping_sub(1), r.rank.wrapping_sub(1));
        if r.mission_pattern == pattern && (c as u32) < 3 && (k as u32) <= 4 {
            let cell = usize::try_from(c)
                .ok()
                .zip(usize::try_from(k).ok())
                .and_then(|(c, k)| f.get_mut(c).and_then(|row| row.get_mut(k)))
                .ok_or_else(|| game("rank bonus row out of range"))?;
            *cell = r.score_bonus_percent;
        }
    }
    Ok(f)
}

/// The solo rank bonus of a completed range: `(rank, bonus, percent)`; a solo player is rank 1.
pub(crate) fn solo_rank_bonus(range_index: usize, range_score: i32, factors: &[[i64; 5]; 3]) -> (i32, i32, i64) {
    let rank = 1;
    if range_index >= factors.len() {
        return (rank, 0, 0);
    }
    let pct = factors[range_index][0];
    let p = range_score as i128 * pct as i128;
    let q = p.abs() / 100;
    (rank, (if p >= 0 { q } else { -q }) as i32, pct)
}

impl Controller {
    /// The score of a range (end score minus start score).
    pub(crate) fn range_score(&self, idx: usize) -> i32 {
        self.states[idx].score()
    }
}

/// Player-indexed network input. The calculator reads the mission from the song, not this row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkGekisouResult {
    pub range_index: i32,
    pub combo: i32,
    pub luck_total_point: i32,
    pub just_count: i32,
    pub perfect_count: i32,
    pub score: i32,
    pub has_no_input: bool,
}

/// The native network calculator's failures, distinct from a dynamic controller's range capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrokenGekisouRanking {
    NoValidData,
    RangeOutOfBounds(i32),
    InvalidMission(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkGekisouRanking {
    pub groups: Vec<Vec<usize>>,
    pub broken: Option<BrokenGekisouRanking>,
    /// Confirmed ranks use competition ranking, unlike GetPlayerRank's group ordinals.
    pub confirmed_ranks: Vec<i32>,
}

impl NetworkGekisouRanking {
    pub fn player_rank(&self, player: usize) -> i32 {
        if self.broken.is_some() {
            return 1;
        }
        self.groups.iter().position(|g| g.contains(&player)).map_or(0, |i| i as i32 + 1)
    }
    pub fn confirmed_rank(&self, player: usize) -> i32 {
        self.confirmed_ranks.get(player).copied().unwrap_or(5)
    }
}

/// A no-input flag alone is not sufficient; skill-added counts can make a player eligible.
pub fn gekisou_no_input(result: Option<&NetworkGekisouResult>) -> bool {
    result.is_none_or(|r| r.has_no_input && r.just_count == 0 && r.combo == 0 && r.luck_total_point == 0)
}

fn compare_network_result(
    mission: i64,
    a: Option<&NetworkGekisouResult>,
    b: Option<&NetworkGekisouResult>,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let point = |r: Option<&NetworkGekisouResult>| {
        r.map_or(0, |r| match mission {
            1 => r.combo,
            2 => r.luck_total_point,
            _ => r.just_count,
        })
    };
    // The primary comparison precedes null handling; a negative primary value sorts AFTER a missing player.
    let primary = point(b).cmp(&point(a));
    if primary != Ordering::Equal {
        return primary;
    }
    match (a, b) {
        (Some(a), Some(b)) => b.score.cmp(&a.score).then_with(|| b.perfect_count.cmp(&a.perfect_count)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Uses the explicit configured player count and player-indexed results.
/// The first non-null row supplies the range index. Other rows are not silently re-keyed by token/index.
/// A missing song mission is an input error (native list-index exception), not a Broken ranking.
/// Tied players' order within a group is not part of this API's contract.
pub fn calculate_network_gekisou_ranking(
    missions: &[i64],
    player_count: usize,
    results: &[Option<NetworkGekisouResult>],
) -> Result<NetworkGekisouRanking, Error> {
    let broken =
        |reason| NetworkGekisouRanking { groups: Vec::new(), broken: Some(reason), confirmed_ranks: vec![1; 5] };
    let Some(first) = results.iter().flatten().next() else {
        return Ok(broken(BrokenGekisouRanking::NoValidData));
    };
    if !(0..3).contains(&first.range_index) {
        return Ok(broken(BrokenGekisouRanking::RangeOutOfBounds(first.range_index)));
    }
    let mission = *missions
        .get(first.range_index as usize)
        .ok_or_else(|| Error::Input("network ranking mission index out of range".into()))?;
    if !(1..=3).contains(&mission) {
        return Ok(broken(BrokenGekisouRanking::InvalidMission(mission)));
    }
    if player_count == 0 {
        return Err(Error::Input("native network ranking requires a non-empty player array".into()));
    }
    let row = |i: usize| results.get(i).and_then(Option::as_ref);
    let mut sorted: Vec<usize> = (0..player_count).collect();
    sorted.sort_by(|&a, &b| compare_network_result(mission, row(a), row(b)));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in sorted {
        if groups.last().is_some_and(|g| compare_network_result(mission, row(g[0]), row(i)).is_eq()) {
            groups.last_mut().unwrap().push(i);
        } else {
            groups.push(vec![i]);
        }
    }
    let mut confirmed_ranks = vec![5; results.len().max(player_count)];
    // The constructor selects its filtering branch by scanning the result list, not the configured player slots.
    let filter = results.iter().any(|r| gekisou_no_input(r.as_ref()));
    let mut next = 1i32;
    for group in &groups {
        let eligible: Vec<_> = group.iter().copied().filter(|&i| !filter || !gekisou_no_input(row(i))).collect();
        for &i in &eligible {
            confirmed_ranks[i] = next;
        }
        next = next.wrapping_add(eligible.len() as i32);
    }
    Ok(NetworkGekisouRanking { groups, broken: None, confirmed_ranks })
}
