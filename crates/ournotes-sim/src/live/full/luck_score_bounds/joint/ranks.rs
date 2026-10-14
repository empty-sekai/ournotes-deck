//! The rank bonuses of a joint evaluation: each range score is the difference of two snapshots, and each bonus keeps
//! the enclosure of its truncation around that expectation.

use super::*;
use crate::num::FxHashSet;

/// The rank bonuses of a joint evaluation, within the enclosures of the exact evaluation.
pub(super) struct Ranks<'s> {
    /// The range scores and bonuses of the exact evaluation.
    enclosures: &'s [LuckExactRange],
    /// The rank snapshot queries, and each one's note sum with the coefficients of the filed bonuses there.
    queries: FxHashSet<usize>,
    snapshots: FxHashMap<usize, (F64Interval, Vec<u8>)>,
    /// The filed rank bonuses: frame, coefficient offset and bonus; the bonus the next query files.
    fixed: Vec<(i32, u8, F64Interval)>,
    next: Option<(i32, F64Interval)>,
    /// Each range score and bonus.
    ranges: Vec<(F64Interval, F64Interval)>,
}

impl<'s> Ranks<'s> {
    /// The rank bonuses of `schedule`, enclosed by `enclosures`.
    pub(super) fn new(schedule: &Schedule, enclosures: &'s [LuckExactRange]) -> Self {
        let mut queries = FxHashSet::default();
        for (event, _) in &schedule.events {
            if let BoundsEvent::Rank { start, end: Some(end), .. } = event {
                queries.extend(start.iter().copied().chain([*end]));
            }
        }
        Self { enclosures, queries, snapshots: FxHashMap::default(), fixed: Vec::new(), next: None, ranges: Vec::new() }
    }

    /// Whether a rank bonus reads the note sum of score query `query`.
    pub(super) fn reads(&self, query: usize) -> bool {
        self.queries.contains(&query)
    }

    /// A score query up to frame `to` files the latest rank bonus.
    pub(super) fn file(&mut self, to: i32) {
        if let Some((frame, bonus)) = self.next.take() {
            self.fixed.push((frame, u8::from(frame > to), bonus));
        }
    }

    /// Score query `query` up to frame `to` reads note sum `sum`.
    pub(super) fn snapshot(&mut self, query: usize, to: i32, sum: F64Interval) {
        let coefficients = self.fixed.iter().map(|&(frame, offset, _)| offset + u8::from(frame <= to)).collect();
        self.snapshots.insert(query, (sum, coefficients));
    }

    /// A rank bonus of range `range` filed at `time_ms`: its range score is the difference of its snapshots.
    pub(super) fn rank(
        &mut self,
        range: usize,
        time_ms: i32,
        percent: i64,
        start: Option<usize>,
        end: Option<usize>,
    ) -> Result<(), Error> {
        let measured =
            |query: usize| self.snapshots.get(&query).ok_or_else(|| refuse("a rank snapshot query was not measured"));
        let end = measured(end.ok_or_else(|| refuse("rank end query was not recorded"))?)?;
        let start = start.map(measured).transpose()?;
        let mut mean = start.map_or(end.0, |start| end.0.subtract(start.0));
        for (index, &(_, _, bonus)) in self.fixed.iter().enumerate() {
            let a = start.and_then(|start| start.1.get(index)).copied().unwrap_or(0);
            let b = end.1.get(index).copied().unwrap_or(0);
            let coefficient = i64::from(b) - i64::from(a);
            if coefficient != 0 {
                mean = mean.add(bonus.scale_integer(i128::from(coefficient)));
            }
        }
        let enclosure = self
            .enclosures
            .get(self.ranges.len())
            .filter(|enclosure| enclosure.range == range && enclosure.percent == percent)
            .ok_or_else(|| refuse("the joint rank bonuses do not match the exact ones"))?;
        let old = |bounds: RealBounds| F64Interval::new(bounds.lower, bounds.upper);
        let disagree = || refuse("a joint range score leaves its enclosure");
        let mean = mean.intersect(old(enclosure.mean)?).ok_or_else(disagree)?;
        let support = I32Interval::new(enclosure.support.lower, enclosure.support.upper)?;
        let bonus = if support.lower() == support.upper() {
            old(enclosure.bonus_mean)?
        } else {
            rank_mean_bounds(mean, support, percent)?.intersect(old(enclosure.bonus_mean)?).ok_or_else(disagree)?
        };
        self.next = Some((get_frame(time_ms), bonus));
        self.ranges.push((mean, bonus));
        Ok(())
    }

    /// The rank mean of a final score whose terminal query ends at frame `prev`, and each range score and bonus.
    pub(super) fn finish(&self, prev: i32) -> Result<(F64Interval, Vec<(F64Interval, F64Interval)>), Error> {
        if self.next.is_some() {
            return Err(refuse("terminal query has not filed every rank bonus"));
        }
        let mut rank = F64Interval::ZERO;
        for &(frame, offset, bonus) in &self.fixed {
            let coefficient = i64::from(offset) + i64::from(frame <= prev);
            rank = rank.add(bonus.scale_integer(i128::from(coefficient)));
        }
        Ok((rank, self.ranges.clone()))
    }
}
