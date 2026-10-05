//! Simulation cutoff tables. For one candidate and performance order, the fine cap is split by score frame: the entries of
//! frames a running live has not settled keep their whole term, the settled ones are replaced by their final scores,
//! and the rank bonuses and conversion gains shrink with them. A live's final score is then at most its settled score
//! plus that remainder, so a candidate whose expected payoff cannot reach the Top-K cutoff stops simulating as soon as
//! the settled prefix shows it.

use super::JointBounds;
use crate::domain::CandidateDomain;
use crate::search::expectation::PhysicalDeck;
use crate::search::snaps::JointScratch;
use ournotes_sim::live::full::Settled;
use ournotes_sim::live::score::get_frame;

/// The cap of one candidate at one performance order, by settled score frame.
pub(crate) struct CutoffTable {
    /// Historical network snapshots keep their candidate/order cap after notes have settled.
    constant: Option<i128>,
    /// Score frames of the cap's entries, ascending, and their floored bounds and rank factors.
    frames: Vec<i32>,
    z: Vec<f64>,
    rk: Vec<f64>,
    /// `suffix[i]`: the rank-weighted terms of entries `i..`.
    suffix: Vec<f64>,
    /// Completing ranges with a rank bonus: (score frame of the range end, percent / 100, entries).
    ranges: Vec<(i32, f64, Vec<u32>)>,
    rows: Vec<Row>,
    /// The candidate's PT bonus when the payoff is event points.
    points_bonus: Option<i64>,
}

/// One conversion budget row: at most `count` conversions, each entry's rank-weighted gain (0 where it cannot
/// convert), and `tail[i]`: the largest `count` gains of entries `i..`.
struct Row {
    count: usize,
    gain: Vec<f64>,
    tail: Vec<f64>,
}

/// The sum of the `n` largest values (all of them when fewer).
fn top_sum(v: &mut [f64], n: usize) -> f64 {
    let n = n.min(v.len());
    if n == 0 {
        return 0.0;
    }
    if n < v.len() {
        v.select_nth_unstable_by(n - 1, |a, b| b.total_cmp(a));
    }
    v[..n].iter().sum()
}

impl CutoffTable {
    /// An upper bound on the final score once score frames below `s.frame` hold `s.total`.
    ///
    /// Each entry's term bounds its note's final score whatever happens later, plus a conversion gain when a budget
    /// row converts it; an entry in a settled frame already counts in `s.total` with its final score. A rank bonus
    /// is a percentage of its range's entries, filed at the range end: once that frame is settled it counts in
    /// `s.total`; before, an unsettled entry carries its share in its rank factor and a settled one adds
    /// `z * percent` (and that share of its conversion gain). Each row converts at most its count among the
    /// unsettled entries and as many among the settled ones, a bound on its conversions among all of them.
    pub(crate) fn score_cap(&self, s: Settled) -> Option<i128> {
        if let Some(cap) = self.constant {
            return Some(cap);
        }
        let i = self.frames.partition_point(|&f| f < s.frame);
        let mut rest = self.suffix[i];
        let mut live: Vec<(u32, f64)> = Vec::new();
        for (frame, pct, entries) in &self.ranges {
            if *frame < s.frame {
                continue;
            }
            for &e in entries.iter().filter(|&&e| (e as usize) < i) {
                rest += self.z[e as usize] * pct;
                live.push((e, *pct));
            }
        }
        // one conversion of an entry in two ranges carries both shares
        live.sort_by_key(|x| x.0);
        live.dedup_by(|b, a| {
            let same = a.0 == b.0;
            if same {
                a.1 += b.1;
            }
            same
        });
        let mut gains = Vec::new();
        for row in &self.rows {
            rest += row.tail[i];
            if !live.is_empty() {
                gains.clear();
                gains.extend(
                    live.iter().map(|&(e, pct)| row.gain[e as usize] * pct / self.rk[e as usize]).filter(|&g| g > 0.0),
                );
                rest += top_sum(&mut gains, row.count);
            }
        }
        let rest = (rest * (1.0 + 1e-9)).ceil();
        if !rest.is_finite() || rest >= i64::MAX as f64 {
            return None;
        }
        Some(i128::from(s.total) + rest as i128)
    }

    /// An upper bound on the payoff of this order once the live settled `s`.
    pub(crate) fn payoff_cap(&self, bounds: &JointBounds, s: Settled) -> Option<i128> {
        let cap = self.score_cap(s)?;
        Some(bounds.payoff_of(self.points_bonus, cap))
    }

    /// Entries are `(chart time, floored bound, rank factor)`, ranges `(end time, percent / 100, entries)` and rows
    /// `(most conversions, (entry, rank-weighted gain))`, entries indexed in the given order.
    fn new(
        entries: &[(i32, f64, f64)],
        ranges: &[(i32, f64, Vec<u32>)],
        rows: &[(f64, Vec<(u32, f64)>)],
        points_bonus: Option<i64>,
    ) -> Option<CutoffTable> {
        if entries.iter().any(|&(_, z, rk)| !z.is_finite() || !rk.is_finite() || z < 0.0 || rk < 1.0) {
            return None;
        }
        let n = entries.len();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&e| get_frame(entries[e].0));
        let mut at = vec![0u32; n];
        for (k, &e) in order.iter().enumerate() {
            at[e] = k as u32;
        }
        let frames: Vec<i32> = order.iter().map(|&e| get_frame(entries[e].0)).collect();
        let z: Vec<f64> = order.iter().map(|&e| entries[e].1).collect();
        let rk: Vec<f64> = order.iter().map(|&e| entries[e].2).collect();
        let mut suffix = vec![0.0; n + 1];
        for k in (0..n).rev() {
            suffix[k] = suffix[k + 1] + z[k] * rk[k];
        }
        let mut cut_ranges = Vec::with_capacity(ranges.len());
        for (end, pct, members) in ranges {
            if !pct.is_finite() || *pct < 0.0 || members.iter().any(|&e| e as usize >= n) {
                return None;
            }
            cut_ranges.push((get_frame(*end), *pct, members.iter().map(|&e| at[e as usize]).collect()));
        }
        let mut cut_rows = Vec::with_capacity(rows.len());
        for (budget, gains) in rows {
            if budget.is_nan() || gains.iter().any(|&(e, g)| e as usize >= n || !g.is_finite() || g < 0.0) {
                return None;
            }
            let count = if *budget >= gains.len() as f64 { gains.len() } else { budget.max(0.0) as usize };
            let mut gain = vec![0.0; n];
            for &(e, g) in gains {
                gain[at[e as usize] as usize] += g;
            }
            // the largest `count` gains of each suffix: a min-heap of the kept ones (bits order non-negative floats)
            let mut tail = vec![0.0; n + 1];
            let mut heap = std::collections::BinaryHeap::new();
            let mut sum = 0.0;
            for k in (0..n).rev() {
                if gain[k] > 0.0 && count > 0 {
                    heap.push(std::cmp::Reverse(gain[k].to_bits()));
                    sum += gain[k];
                    if heap.len() > count
                        && let Some(std::cmp::Reverse(b)) = heap.pop()
                    {
                        sum -= f64::from_bits(b);
                    }
                }
                tail[k] = f64::max(sum, 0.0);
            }
            cut_rows.push(Row { count, gain, tail });
        }
        Some(CutoffTable { constant: None, frames, z, rk, suffix, ranges: cut_ranges, rows: cut_rows, points_bonus })
    }
}

impl JointBounds {
    fn payoff_of(&self, points_bonus: Option<i64>, score_cap: i128) -> i128 {
        match (&self.points, points_bonus) {
            (Some(pt), Some(bonus)) => ((bonus + 10000) * pt.multiplier_at(score_cap) / 10000) as i128,
            _ => score_cap,
        }
    }

    /// The cutoff table of one candidate at one performance order (None without a finite fine cap).
    pub(crate) fn cutoff_table(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        power: i64,
        positions: &[usize; 5],
        scratch: &mut JointScratch,
    ) -> Option<CutoffTable> {
        let fine = self.fine.as_ref()?;
        let choices = Self::prefix_choices(domain, p, 5);
        let (total, terms) = fine.cap_terms(power, p.members, choices, positions, scratch, None);
        if total == i64::MAX || !terms.conv.is_finite() {
            return None;
        }
        let points_bonus = self.points.as_ref().map(|_| self.bonus_of(p, &choices));
        if terms.network_ranking {
            let mut table = CutoffTable::new(&[], &[], &[], points_bonus)?;
            table.constant = Some(total as i128);
            return Some(table);
        }
        let entries: Vec<(i32, f64, f64)> =
            terms.entries.iter().map(|&(t, z, rk)| (t, z, if terms.ranked { rk } else { 1.0 })).collect();
        let ranges: &[(i32, f64, Vec<u32>)] = if terms.ranked { &terms.ranges } else { &[] };
        CutoffTable::new(&entries, ranges, &terms.rows, points_bonus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // entries at chart times in score frames 1, 2, 2 and 5 (40 ms frames) with terms 100, 50, 30 and 20
    const TIMES: [i32; 4] = [40, 80, 80, 200];

    #[test]
    fn unsettled_entries_keep_their_terms_and_gains() {
        let entries: Vec<_> = TIMES.iter().zip([100.0, 50.0, 30.0, 20.0]).map(|(&t, z)| (t, z, 1.0)).collect();
        // one conversion among entries 0 (gain 7) and 3 (gain 5)
        let t = CutoffTable::new(&entries, &[], &[(1.0, vec![(0, 7.0), (3, 5.0)])], None).unwrap();
        // 200 + 7, and the relative float margin rounds an exact integer up by one
        assert_eq!(t.score_cap(Settled { frame: 0, total: 0, fixed: 0 }), Some(208));
        // frames 1 and 2 settled at 170 points: the frame-5 entry and its gain remain
        assert_eq!(t.score_cap(Settled { frame: 3, total: 170, fixed: 0 }), Some(196));
        // everything settled: the cap is the settled score
        assert_eq!(t.score_cap(Settled { frame: 6, total: 190, fixed: 0 }), Some(190));
    }

    #[test]
    fn settled_rank_bonuses_leave_the_cap() {
        // entries 0..3 form a 10% range ending in frame 2; entry 3 is outside
        let rk = [1.1, 1.1, 1.1, 1.0];
        let entries: Vec<_> =
            TIMES.iter().zip([100.0, 50.0, 30.0, 20.0]).zip(rk).map(|((&t, z), r)| (t, z, r)).collect();
        let ranges = [(80, 0.1, vec![0, 1, 2])];
        let t = CutoffTable::new(&entries, &ranges, &[(1.0, vec![(1, 11.0), (0, 2.2)])], None).unwrap();
        // 218 + 11, rounded up past the margin
        assert_eq!(t.score_cap(Settled { frame: 0, total: 0, fixed: 0 }), Some(230));
        // frame 1 settled at 100: entry 0 adds its share 10, and its share 0.2 of a conversion besides entry 1's 11
        assert_eq!(t.score_cap(Settled { frame: 2, total: 100, fixed: 0 }), Some(230));
        // the range's bonus settled in frame 2 with the 190 points: only entry 3 remains
        assert_eq!(t.score_cap(Settled { frame: 3, total: 190, fixed: 18 }), Some(211));
    }
}
