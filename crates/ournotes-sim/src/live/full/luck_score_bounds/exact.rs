//! The native score expectation of one formation and order under the independent nominal lottery law, from the
//! exact factor histories of every lottery path.
//!
//! The recorder plays the formation once and files every lottery-dependent command at each of its possible
//! places. [`ExactReplay`] follows every native factor history these filings admit. A note's native score is a
//! function of its last execution's factor state and combo inputs, of its probe class and of whether Rush is
//! running when it is scored, so each of the four lottery classes has an integer support over those histories
//! (usually a single value). The certified lottery law gives the probability of each class in the phase in which
//! the note is scored: a query at a play frame scores the notes before the frame's time with the classes they
//! keep, and the notes at the frame's own time with the Rush the frame's queries see and the probe class before
//! or after the frame's skill boundary. A score's expectation is linear in these. A rank bonus truncates the
//! product of a snapshot difference and its percentage, so its expectation lies within one of the linear term.
//!
//! The note expectations of one score are summed on a fixed-point grid, outward, so a sum does not depend on the
//! order of its terms and the orders of a joint replay sum exactly as each order alone.

use super::exact_paths::{
    ExactNote, ExactReplay, NoteValue, Orders, each, first_orders, refine, rush_mask, undo_floors,
};
use super::*;
use crate::live::full::luck_dp::{RUSH_FINISH, RUSH_PROBE};

/// One scored note: the integer support of each lottery class 00/01/10/11 (probe bit 0, Rush bit 1; None when no
/// path reaches the probe class), the class probabilities and the expectation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactNote {
    pub note_id: i32,
    pub time_ms: i32,
    pub buckets: [Option<IntegerBounds>; 4],
    pub probability: [RealBounds; 4],
    pub mean: RealBounds,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactRange {
    pub range: usize,
    pub percent: i64,
    /// The range score: the difference of the range's score snapshots.
    pub mean: RealBounds,
    pub support: IntegerBounds,
    pub bonus_mean: RealBounds,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactScore {
    pub final_mean: RealBounds,
    pub final_note_mean: RealBounds,
    pub final_rank_mean: RealBounds,
    pub final_support: IntegerBounds,
    /// The terminal life, which is the same on every lottery path, lies within these bounds.
    pub final_life: IntegerBounds,
    pub ranges: Vec<LuckExactRange>,
    pub scored_notes: usize,
    /// Final notes with more than one integer score in a lottery class of positive probability.
    pub wide_notes: usize,
    /// The largest such spread.
    pub widest_note: i32,
    pub peak_paths: usize,
    pub queries: usize,
    pub probability_peak_states: usize,
    pub probability_transitions: u64,
    pub profile: LuckExactProfile,
    /// The final notes, when requested.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<LuckExactNote>,
}

/// The work of one evaluation. Timings are filled only in diagnostic builds.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactProfile {
    /// Path histories executed by score queries, and the score frames they undid and executed.
    pub runs: u64,
    pub undone_frames: u64,
    pub executed_frames: u64,
    /// Score queries that executed no frame on any path.
    pub idle_queries: u64,
    pub setup_ms: f64,
    pub law_ms: f64,
    pub record_ms: f64,
    pub replay_ms: f64,
    pub measure_ms: f64,
}

/// Milliseconds between laps in diagnostic builds, zero otherwise.
pub(super) struct Stopwatch {
    #[cfg(feature = "search-diagnostics")]
    at: std::time::Instant,
}

impl Stopwatch {
    pub(super) fn start() -> Self {
        Self {
            #[cfg(feature = "search-diagnostics")]
            at: std::time::Instant::now(),
        }
    }

    pub(super) fn lap(&mut self) -> f64 {
        #[cfg(feature = "search-diagnostics")]
        {
            let now = std::time::Instant::now();
            let ms = (now - self.at).as_secs_f64() * 1e3;
            self.at = now;
            ms
        }
        #[cfg(not(feature = "search-diagnostics"))]
        0.0
    }
}

const NO_LOTTERY: [ProbabilityMass; 4] =
    [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO];

/// The class probabilities a note at `time_ms` keeps once every lottery filing up to its time is made.
fn kept_mass(curve: &LuckDpCertifiedResult, time_ms: i32) -> [ProbabilityMass; 4] {
    let index = curve.steps.partition_point(|(time, _)| *time <= time_ms);
    index.checked_sub(1).map_or(NO_LOTTERY, |index| curve.steps[index].1)
}

/// How a query reads a note's lottery class: the class the note keeps once every lottery filing up to its chart time
/// is made, or the class a score query of the note's own play frame reads on its side of the frame's skill boundary,
/// by the phase's index in [`LuckDpCertifiedResult::frame_queries`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ClassRead {
    Kept(i32),
    Query { query_ms: i32, phase: usize },
}

/// The class read of a note at `note_ms` by a query at play frame `query_ms`. Notes of earlier frames keep their
/// filed lottery state; notes of the query's own frame read the state before that frame's lottery, at their own time,
/// on the side of the frame's skill boundary the query runs.
pub(super) fn class_read(
    frame_times: &[i32],
    probability_ready: i32,
    skills_at: i32,
    query_ms: i32,
    note_ms: i32,
) -> Result<ClassRead, Error> {
    if note_ms <= probability_ready {
        return Ok(ClassRead::Kept(note_ms));
    }
    let frame = frame_times.binary_search(&query_ms).map_err(|_| refuse("a score snapshot is not at a play frame"))?;
    let previous = frame.checked_sub(1).map_or(i32::MIN, |frame| frame_times[frame]);
    if previous > probability_ready || note_ms <= previous || note_ms > query_ms {
        return Err(refuse("a score snapshot reads a note outside its frame's lottery state"));
    }
    let phase = if note_ms < query_ms {
        0
    } else if skills_at == query_ms {
        1
    } else {
        2
    };
    Ok(ClassRead::Query { query_ms, phase })
}

/// The class probabilities a note reads when a query at play frame `query_ms` scores it ([`class_read`]).
pub(super) fn query_mass(
    curve: &LuckDpCertifiedResult,
    frame_times: &[i32],
    probability_ready: i32,
    skills_at: i32,
    query_ms: i32,
    note_ms: i32,
) -> Result<[ProbabilityMass; 4], Error> {
    match class_read(frame_times, probability_ready, skills_at, query_ms, note_ms)? {
        ClassRead::Kept(time) => Ok(kept_mass(curve, time)),
        // A law without lottery steps or query probabilities ([`no_lottery`]) keeps every note in class 00.
        ClassRead::Query { .. } if curve.steps.is_empty() && curve.frame_queries.is_empty() => Ok(NO_LOTTERY),
        ClassRead::Query { query_ms, phase } => {
            let index = curve.frame_queries.partition_point(|(time, _)| *time < query_ms);
            match curve.frame_queries.get(index) {
                Some(&(time, masses)) if time == query_ms => Ok(masses[phase]),
                _ => Err(refuse("a judging frame has no certified query probabilities")),
            }
        }
    }
}

/// The integer support of each lottery class 00/01/10/11 of a note; None where no path reaches the class.
pub(super) type Buckets = [Option<(i32, i32)>; 4];

fn join_buckets(buckets: &mut Buckets, other: &Buckets) {
    for (bucket, other) in buckets.iter_mut().zip(other) {
        if let Some((lo, hi)) = *other {
            *bucket = Some(bucket.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))));
        }
    }
}

/// The integer support of each lottery class of a note's last execution with one value.
pub(super) fn value_classes(
    calc: &LiveScoreCalculator,
    note: &NoteCommand,
    value: &NoteValue,
    rush_percent: i32,
    shared_classes: bool,
) -> Result<Buckets, Error> {
    let mut buckets = [None::<(i32, i32)>; 4];
    let luck = [0, rush_percent].map(|rush| get_luck_factor_percent(rush) as f32 / 100f32);
    let (ordinary, gekisou) = (f32::from_bits(value.combo.0), f32::from_bits(value.combo.1));
    let judge = match note.score_type {
        1 => Some(2),
        2 => Some(3),
        3 => Some(4),
        4 => Some(5),
        _ => None,
    };
    // Native combo and score-up factors at the field bounds; both are monotone in every field.
    let factors = |fields: &[f32; FIELDS]| {
        let combo = gekisou * (fields[0] + ordinary);
        let score_up = fields[1] + judge.map_or(0.0, |field| fields[field]);
        (combo, score_up)
    };
    let (combo_a, up_low) = factors(&value.low);
    let (combo_b, up_high) = factors(&value.high);
    let combos = corners(combo_a.min(combo_b), combo_a.max(combo_b));
    let ups = corners(up_low, up_high);
    let classes = if shared_classes { 0..2 } else { usize::from(value.class)..usize::from(value.class) + 1 };
    for (rush, &luck) in luck.iter().enumerate() {
        for &combo in combos.iter().flatten() {
            for &score_up in ups.iter().flatten() {
                let score = calc.note_score_core(note.life, note.note_type, note.score_type, combo, score_up, luck)?;
                if score == i32::MIN {
                    return Err(refuse("a note score may overflow"));
                }
                for class in classes.clone() {
                    let bucket = &mut buckets[class | rush << 1];
                    *bucket = Some(bucket.map_or((score, score), |(lo, hi)| (lo.min(score), hi.max(score))));
                }
            }
        }
    }
    Ok(buckets)
}

/// The points at which a native note score over `low..=high` of one factor takes its extremes: the score is a
/// rounded product of the combo and score-up factors with constants, so it is monotone in each factor wherever the
/// other keeps its sign, and the extremes lie at the bounds and at a sign change.
fn corners(low: f32, high: f32) -> [Option<f32>; 3] {
    [Some(low), (low < 0.0 && 0.0 < high).then_some(0.0), (high != low).then_some(high)]
}

/// The expectation and support of a note given its class supports and probabilities.
fn note_expectation(buckets: &Buckets, mass: &[ProbabilityMass; 4]) -> Result<(F64Interval, I32Interval), Error> {
    let mut mean = F64Interval::ZERO;
    let (mut lower, mut upper) = (i32::MAX, i32::MIN);
    for (bucket, probability) in buckets.iter().zip(mass) {
        let Some((lo, hi)) = *bucket else {
            if probability.interval().lower() > 0.0 {
                return Err(refuse("a lottery class has no path through the native factor schedule"));
            }
            continue;
        };
        mean = mean.add(probability.interval().multiply(I32Interval::new(lo, hi)?.as_real()));
        if probability.interval().upper() > 0.0 {
            lower = lower.min(lo);
            upper = upper.max(hi);
        }
    }
    let support = I32Interval::new(lower, upper)?;
    let mean = mean.intersect(support.as_real()).ok_or_else(|| refuse("note probability mean misses its support"))?;
    Ok((mean, support))
}

/// 2^64, the inverse grid step of a [`Sum`].
const GRID: f64 = 18_446_744_073_709_551_616.0;

/// A sum of note expectations and supports: the least mean rounded down and the greatest rounded up to multiples of
/// 2^-64, and the integer support bounds. The sums are exact integers, so they do not depend on the order of their
/// terms, and a difference of sums is exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sum {
    low: i128,
    high: i128,
    min: i64,
    max: i64,
}

impl Sum {
    fn of(mean: F64Interval, support: I32Interval) -> Result<Self, Error> {
        let grid = |x: f64, round: fn(f64) -> f64| {
            if x.abs() < 2f64.powi(62) {
                Ok(round(x * GRID) as i128)
            } else {
                Err(refuse("a note expectation leaves the fixed-point range"))
            }
        };
        Ok(Self {
            low: grid(mean.lower(), f64::floor)?,
            high: grid(mean.upper(), f64::ceil)?,
            min: support.lower().into(),
            max: support.upper().into(),
        })
    }

    fn checked(low: Option<i128>, high: Option<i128>, min: Option<i64>, max: Option<i64>) -> Result<Self, Error> {
        let overflow = || refuse("a note sum overflows");
        Ok(Self {
            low: low.ok_or_else(overflow)?,
            high: high.ok_or_else(overflow)?,
            min: min.ok_or_else(overflow)?,
            max: max.ok_or_else(overflow)?,
        })
    }

    fn plus(self, other: Self) -> Result<Self, Error> {
        Self::checked(
            self.low.checked_add(other.low),
            self.high.checked_add(other.high),
            self.min.checked_add(other.min),
            self.max.checked_add(other.max),
        )
    }

    /// The bound-by-bound difference, which undoes [`Self::plus`].
    fn less(self, other: Self) -> Result<Self, Error> {
        Self::checked(
            self.low.checked_sub(other.low),
            self.high.checked_sub(other.high),
            self.min.checked_sub(other.min),
            self.max.checked_sub(other.max),
        )
    }

    /// The bounds of the difference of a value within `self` and a value within `other`.
    fn minus(self, other: Self) -> Result<Self, Error> {
        Self::checked(
            self.low.checked_sub(other.high),
            self.high.checked_sub(other.low),
            self.min.checked_sub(other.max),
            self.max.checked_sub(other.min),
        )
    }

    fn mean(self) -> Result<F64Interval, Error> {
        // A grid value as binary64, rounded down or up.
        let real = |value: i128, up: bool| {
            let near = value as f64;
            let exact = near as i128;
            let bound = if up && exact < value {
                near.next_up()
            } else if !up && exact > value {
                near.next_down()
            } else {
                near
            };
            bound / GRID
        };
        F64Interval::new(real(self.low, false), real(self.high, true))
    }
}

/// Per-order sums: one sum every order shares and each order's difference to it.
#[derive(Clone)]
struct Totals {
    shared: Sum,
    own: Vec<Sum>,
}

impl Totals {
    fn new(lanes: usize) -> Self {
        Self { shared: Sum::default(), own: vec![Sum::default(); lanes] }
    }

    fn add(&mut self, contribution: &Contribution) -> Result<(), Error> {
        self.shared = self.shared.plus(contribution.shared)?;
        for &(orders, difference) in &contribution.own {
            for order in each(orders) {
                self.own[order] = self.own[order].plus(difference)?;
            }
        }
        Ok(())
    }

    /// Takes out a contribution [`Self::add`] put in; the sums are exact, so this restores them.
    fn remove(&mut self, contribution: &Contribution) -> Result<(), Error> {
        self.shared = self.shared.less(contribution.shared)?;
        for &(orders, difference) in &contribution.own {
            for order in each(orders) {
                self.own[order] = self.own[order].less(difference)?;
            }
        }
        Ok(())
    }

    fn of(&self, order: usize) -> Result<Sum, Error> {
        self.shared.plus(self.own[order])
    }
}

/// A note's expectation in each order: the orders with equal sums, the sum of the most orders, and the
/// difference to it of each other sum.
#[derive(Clone, Default)]
struct Contribution {
    blocks: Vec<(Orders, Sum)>,
    shared: Sum,
    own: Vec<(Orders, Sum)>,
}

impl Contribution {
    fn of(blocks: impl IntoIterator<Item = (Orders, Sum)>) -> Result<Self, Error> {
        let mut merged = Vec::<(Orders, Sum)>::new();
        for (orders, sum) in blocks {
            match merged.iter_mut().find(|(_, other)| *other == sum) {
                Some((all, _)) => *all |= orders,
                None => merged.push((orders, sum)),
            }
        }
        let Some(most) = (0..merged.len()).max_by_key(|&index| merged[index].0.count_ones()) else {
            return Ok(Self::default());
        };
        let shared = merged[most].1;
        let mut own = Vec::with_capacity(merged.len() - 1);
        for (index, &(orders, sum)) in merged.iter().enumerate() {
            if index != most {
                own.push((orders, sum.less(shared)?));
            }
        }
        Ok(Self { blocks: merged, shared, own })
    }
}

/// A filed note's class supports for one revision of its values, and its expectations under the class
/// probabilities last read.
#[derive(Default)]
struct MeasuredNote {
    revision: Option<u32>,
    /// The values of the revision and their class supports.
    values: Vec<(NoteValue, Buckets)>,
    /// Orders with the same values, and the class supports of those values.
    classes: Vec<(Orders, Buckets)>,
    masses: Vec<[u64; 8]>,
    /// For each class supports and law with common orders: the orders, the class supports, the law and the
    /// expectation.
    blocks: Vec<(Orders, usize, usize, F64Interval, I32Interval)>,
    contribution: Contribution,
    /// Whether the running note sums hold `contribution`, and whether the class probabilities last read are the
    /// note's final ones, which no later query changes.
    counted: bool,
    settled: bool,
}

fn mass_bits(mass: &[ProbabilityMass; 4]) -> [u64; 8] {
    let mut bits = [0; 8];
    for (index, probability) in mass.iter().enumerate() {
        bits[2 * index] = probability.interval().lower().to_bits();
        bits[2 * index + 1] = probability.interval().upper().to_bits();
    }
    bits
}

/// The calculator of a formation at another band power, and whether every note value scored so far has one integer
/// score in each lottery class, the same at both powers.
pub(super) struct PowerTwin {
    power: i32,
    calc: Option<LiveScoreCalculator>,
    equal: bool,
}

impl PowerTwin {
    pub(super) fn new(power: i32) -> Self {
        Self { power, calc: None, equal: true }
    }

    /// Whether every note value compared so far scores the same single integer in each lottery class at both powers.
    pub(super) fn equal(&self) -> bool {
        self.equal
    }

    /// Forgets the comparisons made so far.
    pub(super) fn restart(&mut self) {
        (self.calc, self.equal) = (None, true);
    }

    /// Takes the calculator of the formation before it plays: the twin is the same calculator at the other power.
    fn bind(&mut self, calc: &LiveScoreCalculator) -> Result<(), Error> {
        if calc.state != crate::live::score::ScoreFactorState::new(calc.state.band_total_power) {
            return Err(refuse("a power twin needs the calculator before any command"));
        }
        let mut twin = calc.clone();
        twin.state = crate::live::score::ScoreFactorState::new(self.power);
        self.calc = Some(twin);
        Ok(())
    }

    /// Compares the class supports of one note value at the formation's power with those at the twin's. The first
    /// difference ends the evaluation: no later value can make the scores agree.
    fn check(
        &mut self,
        buckets: &Buckets,
        note: &NoteCommand,
        value: &NoteValue,
        rush_percent: i32,
        shared_classes: bool,
    ) -> Result<(), Error> {
        let calc = self.calc.as_ref().ok_or_else(|| refuse("a power twin has no calculator"))?;
        let other = value_classes(calc, note, value, rush_percent, shared_classes)?;
        self.equal &= other == *buckets && buckets.iter().flatten().all(|(lo, hi)| lo == hi);
        if !self.equal {
            return Err(refuse("a note value may score differently at the other power"));
        }
        Ok(())
    }
}

/// One rank bonus of a one-order schedule: its percentage, the enclosures of its snapshot difference and of its
/// bonus, and the difference as a signed sum of note scores, or None when a filed bonus with several possible values
/// enters it.
pub(super) struct RankDifference {
    pub(super) percent: i64,
    pub(super) mean: F64Interval,
    pub(super) support: I32Interval,
    pub(super) bonus: F64Interval,
    pub(super) terms: Option<RankTerms>,
}

/// A snapshot difference as each of its notes, with how its snapshot reads the note's lottery class, the note's
/// class supports there and its sign, plus a constant.
pub(super) struct RankTerms {
    pub(super) notes: Vec<(ClassRead, Buckets, i64)>,
    pub(super) constant: i64,
}

/// What the rank snapshots of a one-order schedule read: per snapshot query, each of its notes with its class read
/// and class supports; each rank bonus as a [`RankDifference`] in filing order; and the coefficient of each filed
/// bonus in the final score.
#[derive(Default)]
pub(super) struct RankReads {
    snapshots: FxHashMap<usize, Vec<(ClassRead, Buckets)>>,
    pub(super) ranks: Vec<RankDifference>,
    pub(super) coefficients: Vec<i64>,
}

/// The snapshot difference of a rank bonus as a signed sum of note scores, from the notes `reads` holds for its end
/// and start snapshots, plus the filed bonuses whose coefficients differ between the snapshots; None when such a bonus
/// has several possible values. A note whose every class has a single score is a function of the class it reads at
/// its read point alone, so equal reads of such notes add up and opposite ones cancel; any other note keeps its own
/// term.
fn rank_terms(
    reads: &RankReads,
    start: Option<(usize, &Snapshot)>,
    end: (usize, &Snapshot),
    fixed: &[Bonus],
) -> Result<Option<RankTerms>, Error> {
    let mut points = std::collections::BTreeMap::<(ClassRead, Buckets), i64>::new();
    let mut others = Vec::new();
    let mut add = |query: usize, sign: i64| -> Result<(), Error> {
        let notes = reads.snapshots.get(&query).ok_or_else(|| refuse("a rank snapshot query was not read"))?;
        for &(read, buckets) in notes {
            if buckets.iter().flatten().all(|(lo, hi)| lo == hi) {
                *points.entry((read, buckets)).or_default() += sign;
            } else {
                others.push((read, buckets, sign));
            }
        }
        Ok(())
    };
    add(end.0, 1)?;
    if let Some((start, _)) = start {
        add(start, -1)?;
    }
    let mut constant = 0i64;
    for (index, (_, _, bonuses)) in fixed.iter().enumerate() {
        let a = start.and_then(|(_, start)| start.coefficients.get(index)).copied().unwrap_or(0);
        let b = end.1.coefficients.get(index).copied().unwrap_or(0);
        let coefficient = i64::from(b) - i64::from(a);
        if coefficient == 0 {
            continue;
        }
        let support = bonuses.first().ok_or_else(|| refuse("a filed rank bonus has no order"))?.1;
        if support.lower() != support.upper() {
            return Ok(None);
        }
        constant += coefficient * i64::from(support.lower());
    }
    let mut notes: Vec<_> = points
        .into_iter()
        .filter(|&(_, coefficient)| coefficient != 0)
        .map(|((read, buckets), coefficient)| (read, buckets, coefficient))
        .collect();
    notes.extend(others);
    Ok(Some(RankTerms { notes, constant }))
}

/// Optional records of an exact evaluation.
#[derive(Default)]
pub(super) struct Observers<'a> {
    pub(super) reads: Option<&'a mut RankReads>,
    pub(super) twin: Option<&'a mut PowerTwin>,
}

impl MeasuredNote {
    /// Whether the entry is counted with the current values of `exact` and with final class probabilities.
    fn current(&self, exact: &ExactNote) -> bool {
        self.counted && self.settled && self.revision == Some(exact.revision)
    }

    /// Brings the entry to the current values of `exact` and the class probabilities `masses` of the laws of
    /// orders `laws`, and counts its contribution in `totals`. `twin` compares each new value's class supports with
    /// those at its power.
    #[allow(clippy::too_many_arguments)]
    fn measure(
        &mut self,
        calc: &LiveScoreCalculator,
        note: &NoteCommand,
        exact: &ExactNote,
        rush_percent: i32,
        shared_classes: bool,
        every: Orders,
        laws: &[Orders],
        masses: &[[ProbabilityMass; 4]],
        totals: &mut Totals,
        twin: Option<&mut PowerTwin>,
    ) -> Result<(), Error> {
        let previous = self.update(calc, note, exact, rush_percent, shared_classes, every, laws, masses, twin)?;
        if !self.counted {
            totals.add(&self.contribution)?;
            self.counted = true;
        } else if let Some(previous) = previous {
            totals.remove(&previous)?;
            totals.add(&self.contribution)?;
        }
        Ok(())
    }

    /// Takes the entry's contribution out of `totals` where they hold it.
    fn uncount(&mut self, totals: &mut Totals) -> Result<(), Error> {
        if self.counted {
            totals.remove(&self.contribution)?;
            self.counted = false;
        }
        Ok(())
    }

    /// Brings the entry to the current values of `exact` and the class probabilities `masses` of the laws of
    /// orders `laws`; returns the previous contribution when it recomputed it.
    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        calc: &LiveScoreCalculator,
        note: &NoteCommand,
        exact: &ExactNote,
        rush_percent: i32,
        shared_classes: bool,
        every: Orders,
        laws: &[Orders],
        masses: &[[ProbabilityMass; 4]],
        mut twin: Option<&mut PowerTwin>,
    ) -> Result<Option<Contribution>, Error> {
        let fresh = self.revision != Some(exact.revision);
        if fresh {
            self.revision = Some(exact.revision);
            // A value of the previous revision keeps its class supports.
            let previous = std::mem::take(&mut self.values);
            let bits = |value: &NoteValue| {
                (value.class, value.combo, value.low.map(f32::to_bits), value.high.map(f32::to_bits))
            };
            for value in &exact.values {
                let buckets = match previous.iter().find(|(old, _)| bits(old) == bits(value)) {
                    Some(&(_, buckets)) => buckets,
                    None => {
                        let buckets = value_classes(calc, note, value, rush_percent, shared_classes)?;
                        if let Some(twin) = twin.as_deref_mut() {
                            twin.check(&buckets, note, value, rush_percent, shared_classes)?;
                        }
                        buckets
                    }
                };
                self.values.push((*value, buckets));
            }
            let mut cells = vec![every];
            for value in &exact.values {
                refine(&mut cells, value.orders);
            }
            self.classes.clear();
            for cell in cells {
                let order = cell.trailing_zeros() as usize;
                let mut buckets = [None; 4];
                for (value, classes) in &self.values {
                    if value.orders >> order & 1 != 0 {
                        join_buckets(&mut buckets, classes);
                    }
                }
                self.classes.push((cell, buckets));
            }
        }
        if fresh
            || self.masses.len() != masses.len()
            || !self.masses.iter().zip(masses).all(|(a, b)| *a == mass_bits(b))
        {
            self.masses.clear();
            self.masses.extend(masses.iter().map(mass_bits));
            self.blocks.clear();
            let mut sums = Vec::new();
            for (index, (cell, buckets)) in self.classes.iter().enumerate() {
                for (law, &orders) in laws.iter().enumerate() {
                    let orders = cell & orders;
                    if orders != 0 {
                        let (mean, support) = note_expectation(buckets, &masses[law])?;
                        self.blocks.push((orders, index, law, mean, support));
                        sums.push((orders, Sum::of(mean, support)?));
                    }
                }
            }
            return Ok(Some(std::mem::replace(&mut self.contribution, Contribution::of(sums)?)));
        }
        Ok(None)
    }
}

/// The least first score frame each order executed over a run of queries.
#[derive(Clone)]
struct Executed {
    every: i32,
    own: Vec<i32>,
}

impl Executed {
    fn new() -> Self {
        Self { every: i32::MAX, own: Vec::new() }
    }

    /// Takes in one query's first frames by orders, which [`ExactReplay::query`] returns.
    fn add(&mut self, firsts: &[(i32, Orders)], lanes: usize) {
        // Every order executed from the latest of them or earlier.
        let latest = firsts.iter().map(|&(first, _)| first).max().unwrap_or(i32::MAX);
        self.every = self.every.min(latest);
        for &(first, orders) in firsts.iter().filter(|&&(first, _)| first < latest) {
            if self.own.is_empty() {
                self.own = vec![i32::MAX; lanes];
            }
            for order in each(orders) {
                self.own[order] = self.own[order].min(first);
            }
        }
    }

    fn of(&self, order: usize) -> i32 {
        self.own.get(order).map_or(self.every, |&own| own.min(self.every))
    }
}

/// A filed rank bonus: its score frame, its coefficient offset and the bonus of each order.
type Bonus = (i32, u8, Vec<(F64Interval, I32Interval)>);

/// A rank snapshot query: its last score frame, the note sums of each order, and the notes of the frames its ranks
/// read separately with their expectation in each order.
struct Snapshot {
    to: i32,
    /// For a rank that starts here: no later query executes a frame before `low`, and the notes before it read
    /// their final lottery state.
    low: i32,
    totals: Totals,
    /// The frames of `notes`, and the notes by frame.
    window: (i32, i32),
    notes: Vec<(i32, Vec<(Orders, Sum)>)>,
    /// The coefficient of each filed rank bonus.
    coefficients: Vec<u8>,
}

/// The note sum and filed bonuses of `order` that snapshot `end` gained since snapshot `start`, given the first
/// score frame any query after `start` up to `end` executed in the order (`i32::MAX` when none did), or None when
/// the snapshots share no frames. The notes of the frames before that first frame, and before the start's `low`,
/// are the same native values in both snapshots and read the same lottery state, so they cancel exactly: only the
/// later notes are enclosed, on each side. Otherwise both whole note sums are enclosed separately.
fn rank_difference(
    start: Option<&Snapshot>,
    end: &Snapshot,
    executed: Option<i32>,
    order: usize,
    fixed: &[Bonus],
) -> Result<(F64Interval, I32Interval), Error> {
    let sum = match (start, executed) {
        (Some(start), Some(executed)) => {
            let kept = start.to.min(executed.saturating_sub(1));
            let (low, high) = (start.low, start.to);
            let holds = |window: (i32, i32)| window.0 <= low && high <= window.1;
            if kept < low - 1 || end.to < high || !holds(start.window) || !holds(end.window) {
                return Err(refuse("a rank snapshot difference reads notes its snapshots did not keep"));
            }
            let value = |blocks: &[(Orders, Sum)]| {
                blocks
                    .iter()
                    .find(|(orders, _)| orders >> order & 1 != 0)
                    .map(|&(_, sum)| sum)
                    .ok_or_else(|| refuse("a kept note has no expectation in an order"))
            };
            let (mut window, mut later) = (Sum::default(), Sum::default());
            for (frame, blocks) in start.notes.iter().filter(|(frame, _)| (low..=high).contains(frame)) {
                let sum = value(blocks)?;
                window = window.plus(sum)?;
                if *frame > kept {
                    later = later.plus(sum)?;
                }
            }
            let mut early = Sum::default();
            for (_, blocks) in end.notes.iter().filter(|(frame, _)| (low..=kept).contains(frame)) {
                early = early.plus(value(blocks)?)?;
            }
            // The end's notes after `kept`: its whole sum less the notes before `low`, which are the start's, and
            // the notes from `low` to `kept`.
            let before = start.totals.of(order)?.less(window)?;
            end.totals.of(order)?.less(before)?.less(early)?.minus(later)?
        }
        _ => end.totals.of(order)?.minus(match start {
            Some(start) => start.totals.of(order)?,
            None => Sum::default(),
        })?,
    };
    let mut mean = sum.mean()?;
    let (mut lower, mut upper) = (sum.min, sum.max);
    for (index, (_, _, bonuses)) in fixed.iter().enumerate() {
        let a = start.and_then(|start| start.coefficients.get(index)).copied().unwrap_or(0);
        let b = end.coefficients.get(index).copied().unwrap_or(0);
        let coefficient = i64::from(b) - i64::from(a);
        if coefficient == 0 {
            continue;
        }
        let (bonus_mean, bonus_support) = bonuses[order];
        mean = mean.add(bonus_mean.scale_integer(i128::from(coefficient)));
        let x = coefficient * i64::from(bonus_support.lower());
        let y = coefficient * i64::from(bonus_support.upper());
        lower += x.min(y);
        upper += x.max(y);
    }
    Ok((mean, checked_support(lower, upper)?))
}

/// The recorded command and query schedule of one or more orders of a formation.
pub(super) struct Schedule {
    /// The events of every order in filing order, each with the orders whose schedule holds it. Only a factor
    /// command may belong to some of the orders.
    pub events: Vec<(BoundsEvent, Orders)>,
    /// The calculator's score frames and the score queries made.
    pub frames: usize,
    pub queries: usize,
    /// The probe rows of each order, by the orders that have them.
    pub configs: Vec<(Orders, Vec<ProbeRow>)>,
    /// The completed native frame clock and the music length.
    pub frame_times: Vec<i32>,
    pub music_length_ms: i32,
    pub lanes: usize,
    /// The bounds of each order's terminal life.
    pub final_lives: Vec<IntegerBounds>,
}

/// The expected native score of `deck` in its given order under the independent nominal lottery law, with every
/// path's exact factor history. `skills` is [`luck_skills`] of `master`; `ranking` declares an external rank
/// confirmation timeline, otherwise ranks follow the solo rank queries. The lottery law comes from, and enters,
/// `curves` when given. `details` retains the final notes.
#[allow(clippy::too_many_arguments)]
pub fn luck_exact_score(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: Option<&mut LuckDpCache>,
    details: bool,
) -> Result<LuckExactScore, Error> {
    let live = LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking };
    exact_score(&live, deck, None, curves, details, Observers::default())
}

/// [`luck_exact_score`] in `live`, under `law` when given: the lottery law of `deck` in its order. `observers` see the
/// evaluation ([`exact_schedule`]).
pub(super) fn exact_score(
    live: &LotteryLive<'_>,
    deck: &[Performer],
    law: Option<std::sync::Arc<LuckDpCertifiedResult>>,
    curves: Option<&mut LuckDpCache>,
    details: bool,
    observers: Observers<'_>,
) -> Result<LuckExactScore, Error> {
    let LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking } = *live;
    let mut watch = Stopwatch::start();
    let mut profile = LuckExactProfile::default();
    let mut model = exact_model(master, deck, notes, events, params, setup, ranking)?;
    check_recorder(&model, skills)?;
    profile.setup_ms = watch.lap();
    let has_luck = has_luck(setup);
    let probability = match law {
        Some(law) if has_luck => law,
        _ if has_luck => {
            let mut empty = LuckDpCache::new(0);
            let curves = curves.unwrap_or(&mut empty);
            lottery_law(master, skills, notes, events, params, setup, play, delta_times, deck, ranking, curves)?
        }
        _ => no_lottery(play, skills),
    };
    profile.law_ms = watch.lap();
    let probes = probes_in_native_order(&model, skills)?;
    check_probes(&probes)?;
    let calc = model.score.calc.clone();
    let rush_percent = rush_percent(master, &calc)?;
    model.set_luck_weights(skills, Vec::new())?;
    let probe_phase_bound = bind_probe_phase(&model, skills);
    let trace = record_exact(&mut model, probes, has_luck, play, delta_times)?;
    let schedule = Schedule {
        events: trace.events.into_iter().map(|event| (event, 1)).collect(),
        frames: trace.frames,
        queries: trace.queries,
        configs: vec![(1, trace.probes)],
        frame_times: model.trace.iter().map(|&(time, _)| time).collect(),
        music_length_ms: model.music_length_ms,
        lanes: 1,
        // Life is the same on every lottery path (`check_recorder`), so the recording ends at the terminal life.
        final_lives: vec![I32Interval::point(model.current_life()).into()],
    };
    profile.record_ms = watch.lap();
    let deterministic = [model.score()];
    let mut scores = exact_schedule(
        &schedule,
        &calc,
        rush_percent,
        &[probability],
        play,
        probe_phase_bound,
        setup.fevers.len(),
        ranking.is_none(),
        (!has_luck).then_some(&deterministic[..]),
        details,
        profile,
        watch,
        observers,
    )?;
    Ok(scores.pop().expect("one order"))
}

/// The native model of `deck` in its given order, with the external rank timeline when given.
pub(super) fn exact_model(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LiveModel, Error> {
    Ok(if let Some(ranking) = ranking {
        let mut model = LiveModel::new_gekisou_external(master, deck, notes, events, params, setup)?;
        model.set_rank_confirmation_timeline(ranking)?;
        model
    } else {
        LiveModel::new_gekisou(master, deck, notes, events, params, setup)?
    })
}

/// Plays `model` once with the bounds recorder, which files every lottery-dependent command at each of its
/// possible places, and returns the completed recording.
pub(super) fn record_exact(
    model: &mut LiveModel,
    probes: Vec<ProbeRow>,
    has_luck: bool,
    play: &LivePlay,
    delta_times: &[f32],
) -> Result<BoundsTrace, Error> {
    model.score.begin_bounds(probes, has_luck);
    if delta_times.len() != play.frames.len() {
        return Err(Error::Input("one delta time per frame".into()));
    }
    model.random.set_seed(play.base_seed);
    for (frame, &delta) in play.frames.iter().zip(delta_times) {
        model.frame_timed(frame.time_ms, &frame.judged, delta)?;
    }
    if model.random.draws() != 0 {
        return Err(refuse("the supposedly deterministic recorder consumed random draws"));
    }
    if model.gk.as_ref().is_none_or(|g| g.ctrl.states.iter().any(|state| state.state != gekisou::S_FINISH)) {
        return Err(refuse("terminal query precedes a range FINISH"));
    }
    Ok(model.score.bounds_trace.take().expect("bounds recorder enabled"))
}

/// Whether a range of the live is a LUCK range.
pub(super) fn has_luck(setup: &GekisouSetup) -> bool {
    setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission == gekisou::M_LUCK)
}

/// The lottery law of a live without a LUCK range: every note in class 00.
pub(super) fn no_lottery(play: &LivePlay, skills: &LuckSkills) -> std::sync::Arc<LuckDpCertifiedResult> {
    std::sync::Arc::new(LuckDpCertifiedResult {
        probe_transitions: vec![1; play.frames.len()],
        rush_transitions: vec![65; play.frames.len()],
        steps: Vec::new().into(),
        frame_queries: Vec::new().into(),
        probes: vec![false; skills.shapes.len()],
        probe_starts: crate::live::certified::F64Interval::ZERO,
        range_moments: Vec::new(),
        peak_states: 1,
        transitions: 0,
    })
}

/// The certified lottery law of `deck` in its given order, through `curves`.
#[allow(clippy::too_many_arguments)]
pub(super) fn lottery_law(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: &mut LuckDpCache,
) -> Result<std::sync::Arc<LuckDpCertifiedResult>, Error> {
    Ok(curves
        .certified_cancellable_mode(
            master,
            skills,
            notes,
            events,
            params,
            setup,
            play,
            delta_times,
            deck,
            None,
            ranking,
            false,
            None,
            &mut || false,
        )?
        .expect("an uncancelled lottery law"))
}

/// Refuses probe rows the replay cannot pair with their signed inverses.
pub(super) fn check_probes(probes: &[ProbeRow]) -> Result<(), Error> {
    if probes.iter().any(|row| !row.value.is_finite() || row.value <= i32::MIN as f32 / 100000f32) {
        return Err(refuse("a direct score command cannot be safely paired with its signed inverse"));
    }
    Ok(())
}

/// The Rush score bonus percent, checked against the calculator's arithmetic.
pub(super) fn rush_percent(master: &Master, calc: &LiveScoreCalculator) -> Result<i32, Error> {
    if calc.converted_note_count <= 0 {
        return Err(refuse("nonpositive note count"));
    }
    let rush_percent = i32::try_from(setting(master, "gekisou_luck_rush_score_bonus_percent")?)
        .map_err(|_| refuse("Rush percent exceeds i32"))?;
    if 100i32.checked_add(rush_percent).is_none() {
        return Err(refuse("Rush factor may wrap"));
    }
    Ok(rush_percent)
}

/// The exact expectation of every order of `schedule`, each under its own lottery law in `laws`. The laws share
/// their possible probe and Rush transitions, which shape the paths. `deterministic` gives the constant final score
/// of each order of a live without a LUCK range. `observers` record what the rank snapshots read, which needs a
/// one-order schedule, and compare the note scores with those at another power.
#[allow(clippy::too_many_arguments)]
pub(super) fn exact_schedule(
    schedule: &Schedule,
    calc: &LiveScoreCalculator,
    rush_percent: i32,
    laws: &[std::sync::Arc<LuckDpCertifiedResult>],
    play: &LivePlay,
    probe_phase_bound: bool,
    ranges_len: usize,
    rank_queries: bool,
    deterministic: Option<&[i32]>,
    details: bool,
    mut profile: LuckExactProfile,
    mut watch: Stopwatch,
    observers: Observers<'_>,
) -> Result<Vec<LuckExactScore>, Error> {
    let lanes = schedule.lanes;
    if laws.len() != lanes {
        return Err(Error::Input("one lottery law per order".into()));
    }
    let Observers { mut reads, mut twin } = observers;
    if reads.is_some() && lanes != 1 {
        return Err(Error::Input("rank reads need a one-order schedule".into()));
    }
    if let Some(twin) = twin.as_deref_mut() {
        twin.bind(calc)?;
    }
    let every = first_orders(lanes);
    // The distinct laws and the orders of each.
    let mut distinct_laws = Vec::<&LuckDpCertifiedResult>::new();
    let mut law_orders = Vec::<Orders>::new();
    for (order, law) in laws.iter().enumerate() {
        let index = distinct_laws.iter().position(|&other| std::ptr::eq(other, &**law)).unwrap_or_else(|| {
            distinct_laws.push(law);
            law_orders.push(0);
            distinct_laws.len() - 1
        });
        law_orders[index] |= 1 << order;
    }
    let probability = distinct_laws[0];
    if distinct_laws.iter().any(|law| {
        law.probe_transitions != probability.probe_transitions || law.rush_transitions != probability.rush_transitions
    }) {
        return Err(Error::Input("the lottery laws of a joint replay differ in their possible transitions".into()));
    }
    let frame_times = &schedule.frame_times;
    if !frame_times.iter().copied().eq(play.frames.iter().map(|frame| frame.time_ms)) {
        return Err(refuse("the completed recorder frame clock differs from the probability recording"));
    }
    let events = || schedule.events.iter().map(|(event, _)| event);
    let rows = schedule.configs.iter().any(|(_, rows)| !rows.is_empty());
    check_probe_filings(
        frame_times,
        probability,
        schedule.music_length_ms,
        probe_phase_bound,
        rows,
        schedule.frames,
        events(),
    )?;
    if rows && probability.probe_transitions.len() != frame_times.len() {
        return Err(refuse("the probe transitions do not cover every skill boundary"));
    }
    if probability.rush_transitions.len() != frame_times.len() {
        return Err(refuse("the Rush transitions do not cover every play frame"));
    }
    let query_limit = (play.frames.len() as u64)
        .checked_mul(2)
        .and_then(|value| value.checked_add(if rank_queries { 2 * ranges_len as u64 } else { 0 }))
        .ok_or_else(|| Error::Capacity("score query count overflow".into()))?;
    if schedule.queries as u64 > query_limit {
        return Err(refuse("unaccounted native calculate entry point"));
    }
    let floors = undo_floors(events(), &probability.rush_transitions)?;
    let mut replay = ExactReplay::new(schedule.frames, &schedule.configs, floors, lanes)?;
    // (frame, note, index in `replay.notes`)
    let mut filed = Vec::<(usize, NoteCommand, usize)>::new();
    let mut combos = FxHashMap::<(usize, usize), (f32, f32)>::default();
    // The (start, end) snapshot queries of each rank bonus.
    let pairs: Vec<(Option<usize>, usize)> = events()
        .filter_map(|event| match event {
            BoundsEvent::Rank { start, end: Some(end), .. } => Some((*start, *end)),
            _ => None,
        })
        .collect();
    let starts: FxHashSet<usize> = pairs.iter().filter_map(|&(start, _)| start).collect();
    let mut snapshots = FxHashMap::<usize, Snapshot>::default();
    // From each rank start on, the first frames the later queries executed; frozen at each end.
    let mut executing = Vec::<(usize, Executed)>::new();
    let mut executed = FxHashMap::<(usize, usize), Executed>::default();
    let mut probability_ready = i32::MIN;
    // Play frames whose lotteries have completed, and the lottery filing places of the current play frame.
    let mut ready = 0usize;
    let mut lottery_frames = Vec::new();
    // The next skill boundary's play frame, and the time of the latest skill boundary.
    let (mut boundary, mut skills_at) = (0usize, i32::MIN);
    // The filed rank bonuses: frame, coefficient offset and each order's bonus; the bonus the next query files.
    let mut fixed = Vec::<Bonus>::new();
    let mut pending = None::<(i32, Vec<(F64Interval, I32Interval)>)>;
    let mut ranges = vec![Vec::new(); lanes];
    // By note index in `replay.notes`.
    let mut measured_notes = Vec::<MeasuredNote>::new();
    // The note sums of the counted entries of `measured_notes`.
    let mut running = Totals::new(lanes);
    let mut masses = Vec::with_capacity(distinct_laws.len());
    let (mut queries, mut last_to) = (0usize, None);
    for (event, orders) in &schedule.events {
        match event {
            BoundsEvent::Note { frame, index, note } => {
                let at = replay.file_note(*frame, note.time_ms, note.note_id, (*frame, *index))?;
                filed.push((*frame, *note, at));
                measured_notes.push(MeasuredNote::default());
            }
            BoundsEvent::Factor { frame, command } => {
                if command.band_total_power != 0 {
                    return Err(refuse("recorder produced an unproved power command"));
                }
                replay.file_command(*frame, command, *orders)?;
            }
            BoundsEvent::Potential { frame, start: true } => {
                if rush_mask(&probability.rush_transitions, ready)? & RUSH_FINISH != 0 {
                    replay.finish(*frame)?;
                }
            }
            BoundsEvent::Potential { frame, start: false } => lottery_frames.push(*frame as i32),
            BoundsEvent::Probe { frame, time_ms } => {
                // One boundary per play frame; a frame after music end files its clamped inverse at music end.
                let index = if frame_times.get(boundary) == Some(time_ms) {
                    boundary += 1;
                    boundary - 1
                } else if *time_ms == schedule.music_length_ms && boundary > 0 && frame_times[boundary - 1] > *time_ms {
                    boundary - 1
                } else {
                    return Err(refuse("a probe filing is not at a native skill boundary"));
                };
                let follows = probability.rush_transitions[index] & RUSH_PROBE != 0;
                replay.probe(*frame, *time_ms, probability.probe_transitions[index], follows)?;
                skills_at = frame_times[index];
            }
            BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                combos.insert((*frame, *index), (*ordinary, *gekisou));
            }
            BoundsEvent::ProbabilityReady(time) => {
                if frame_times.get(ready) != Some(time) {
                    return Err(refuse("a lottery completion is not at its play frame"));
                }
                replay.lotteries(&lottery_frames, rush_mask(&probability.rush_transitions, ready)? & 0xff)?;
                lottery_frames.clear();
                ready += 1;
                probability_ready = probability_ready.max(*time);
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                let Some(end) = *end else {
                    return Err(refuse("rank end query was not recorded"));
                };
                let measured = |index: usize| {
                    snapshots.get(&index).ok_or_else(|| refuse("a rank snapshot query was not measured"))
                };
                let (start_snapshot, end_snapshot) = (start.map(measured).transpose()?, measured(end)?);
                let since = match *start {
                    Some(start) if start < end => Some(
                        executed.get(&(start, end)).ok_or_else(|| refuse("a rank snapshot query was not measured"))?,
                    ),
                    _ => None,
                };
                let shared = matches!(*start, Some(start) if start <= end);
                let mut bonuses = Vec::with_capacity(lanes);
                let mut first = None;
                for (order, ranges) in ranges.iter_mut().enumerate() {
                    let first_frame = shared.then(|| since.map_or(i32::MAX, |since| since.of(order)));
                    let (mean, support) = rank_difference(start_snapshot, end_snapshot, first_frame, order, &fixed)?;
                    let x = i128::from(support.lower()) * i128::from(*percent) / 100;
                    let y = i128::from(support.upper()) * i128::from(*percent) / 100;
                    let bonus_support = I32Interval::new(
                        i32::try_from(x.min(y)).map_err(|_| refuse("rank bonus wraps"))?,
                        i32::try_from(x.max(y)).map_err(|_| refuse("rank bonus wraps"))?,
                    )?;
                    let bonus = if support.lower() == support.upper() {
                        bonus_support.as_real()
                    } else {
                        rank_mean_bounds(mean, support, *percent)?
                    };
                    bonuses.push((bonus, bonus_support));
                    if order == 0 {
                        first = Some((mean, support, bonus));
                    }
                    ranges.push(LuckExactRange {
                        range: *range,
                        percent: *percent,
                        mean: mean.into(),
                        support: support.into(),
                        bonus_mean: bonus.into(),
                    });
                }
                if let Some(reads) = reads.as_deref_mut() {
                    let (mean, support, bonus) = first.ok_or_else(|| refuse("a rank bonus has no order"))?;
                    let terms = rank_terms(reads, start.zip(start_snapshot), (end, end_snapshot), &fixed)?;
                    reads.ranks.push(RankDifference { percent: *percent, mean, support, bonus, terms });
                }
                pending = Some((get_frame(*time_ms), bonuses));
            }
            BoundsEvent::Query { time_ms, to } => {
                if !lottery_frames.is_empty() {
                    return Err(refuse("a score query precedes the completion of a play frame's lotteries"));
                }
                let mut query_watch = Stopwatch::start();
                let firsts = replay.query(*to, &combos)?;
                for (_, since) in &mut executing {
                    since.add(firsts, lanes);
                }
                profile.replay_ms += query_watch.lap();
                let query = queries;
                queries += 1;
                last_to = Some(*to);
                for &(start, end) in &pairs {
                    if let Some(start) = start.filter(|&start| start < query && end == query) {
                        let since = executing.iter().find(|(at, _)| *at == start).map(|(_, since)| since.clone());
                        executed
                            .insert((start, end), since.ok_or_else(|| refuse("a rank start query was not measured"))?);
                    }
                }
                if starts.contains(&query) {
                    executing.push((query, Executed::new()));
                }
                if let Some((frame, bonuses)) = pending.take() {
                    fixed.push((frame, u8::from(frame > *to), bonuses));
                }
                if !starts.contains(&query) && !pairs.iter().any(|&(_, end)| end == query) {
                    continue;
                }
                // Only the rank snapshots read a query's sums. The running sums hold the notes up to the previous
                // snapshot; only the notes whose values or class probabilities changed since are measured again.
                let mut unsettled = i32::MAX;
                for &(frame, ref note, at) in &filed {
                    let entry = &mut measured_notes[at];
                    if frame as i32 > *to {
                        entry.uncount(&mut running)?;
                        continue;
                    }
                    let settled = note.time_ms <= probability_ready;
                    if !settled {
                        unsettled = unsettled.min(frame as i32);
                    }
                    if entry.current(&replay.notes[at]) {
                        continue;
                    }
                    masses.clear();
                    for law in &distinct_laws {
                        masses.push(query_mass(
                            law,
                            frame_times,
                            probability_ready,
                            skills_at,
                            *time_ms,
                            note.time_ms,
                        )?);
                    }
                    entry.measure(
                        calc,
                        note,
                        &replay.notes[at],
                        rush_percent,
                        replay.shared_classes(),
                        every,
                        &law_orders,
                        &masses,
                        &mut running,
                        twin.as_deref_mut(),
                    )?;
                    entry.settled = settled;
                }
                if let Some(reads) = reads.as_deref_mut() {
                    // The snapshot holds every note filed up to its last score frame, each reading its class where
                    // `query_mass` took its probabilities above.
                    let mut notes = Vec::new();
                    for &(_, ref note, at) in filed.iter().filter(|&&(frame, _, _)| frame as i32 <= *to) {
                        let read = class_read(frame_times, probability_ready, skills_at, *time_ms, note.time_ms)?;
                        let buckets = measured_notes[at]
                            .classes
                            .iter()
                            .find(|(orders, _)| orders & 1 != 0)
                            .ok_or_else(|| refuse("a snapshot note has no class supports"))?
                            .1;
                        notes.push((read, buckets));
                    }
                    reads.snapshots.insert(query, notes);
                }
                let totals = running.clone();
                let low = replay.base().min(unsettled);
                let mut window = if starts.contains(&query) { (low, *to) } else { (i32::MAX, i32::MIN) };
                for &(start, end) in &pairs {
                    if let Some(start) = start.filter(|&start| start < query && end == query) {
                        let start =
                            snapshots.get(&start).ok_or_else(|| refuse("a rank start query was not measured"))?;
                        window = (window.0.min(start.low), window.1.max(start.to));
                    }
                }
                let notes = filed
                    .iter()
                    .filter(|&&(frame, _, _)| (window.0..=window.1.min(*to)).contains(&(frame as i32)))
                    .map(|&(frame, _, at)| (frame as i32, measured_notes[at].contribution.blocks.clone()))
                    .collect();
                let coefficients = fixed.iter().map(|&(frame, offset, _)| offset + u8::from(frame <= *to)).collect();
                snapshots.insert(query, Snapshot { to: *to, low, totals, window, notes, coefficients });
            }
        }
    }
    if !lottery_frames.is_empty() || ready != frame_times.len() {
        return Err(refuse("a play frame's lotteries did not complete"));
    }
    let Some(prev) = last_to else {
        return Err(refuse("the recorder made no score query"));
    };
    if pending.is_some() {
        return Err(refuse("terminal query has not filed every rank bonus"));
    }
    if deterministic.is_some_and(|scores| scores.len() != lanes) {
        return Err(Error::Input("one deterministic score per order".into()));
    }
    // (wide notes, widest note) per order.
    let mut wide = vec![(0usize, 0i32); lanes];
    let mut final_notes = vec![Vec::new(); lanes];
    let mut scored_notes = 0;
    for &(frame, ref note, at) in &filed {
        let entry = &mut measured_notes[at];
        if frame as i32 > prev {
            entry.uncount(&mut running)?;
            continue;
        }
        if note.time_ms > probability_ready {
            return Err(refuse("final note has pending lottery commands"));
        }
        masses.clear();
        masses.extend(distinct_laws.iter().map(|law| kept_mass(law, note.time_ms)));
        if !entry.current(&replay.notes[at]) {
            entry.measure(
                calc,
                note,
                &replay.notes[at],
                rush_percent,
                replay.shared_classes(),
                every,
                &law_orders,
                &masses,
                &mut running,
                twin.as_deref_mut(),
            )?;
            entry.settled = true;
        }
        scored_notes += 1;
        for &(orders, classes, law, mean, _) in &entry.blocks {
            let (buckets, mass) = (&entry.classes[classes].1, &masses[law]);
            let spread = buckets
                .iter()
                .zip(mass)
                .filter(|(_, p)| p.interval().upper() > 0.0)
                .filter_map(|(bucket, _)| bucket.map(|(lo, hi)| hi - lo))
                .max()
                .unwrap_or(0);
            if spread > 0 {
                for order in each(orders) {
                    wide[order].0 += 1;
                    wide[order].1 = wide[order].1.max(spread);
                }
            }
            if details {
                for order in each(orders) {
                    final_notes[order].push(LuckExactNote {
                        note_id: note.note_id,
                        time_ms: note.time_ms,
                        buckets: buckets.map(|bucket| bucket.map(|(lower, upper)| IntegerBounds { lower, upper })),
                        probability: mass.map(|p| p.interval().into()),
                        mean: mean.into(),
                    });
                }
            }
        }
    }
    profile.measure_ms = watch.lap() - profile.replay_ms;
    profile.runs = replay.runs;
    profile.undone_frames = replay.undone_frames;
    profile.executed_frames = replay.executed_frames;
    profile.idle_queries = replay.idle_queries;
    if let Some(reads) = reads {
        reads.coefficients =
            fixed.iter().map(|&(frame, offset, _)| i64::from(offset) + i64::from(frame <= prev)).collect();
    }
    let mut scores = Vec::with_capacity(lanes);
    for (order, ((ranges, (wide_notes, widest_note)), notes)) in
        ranges.into_iter().zip(wide).zip(final_notes).enumerate()
    {
        let note_sum = running.of(order)?;
        let final_note_mean = note_sum.mean()?;
        let (mut lo, mut hi) = (note_sum.min, note_sum.max);
        let mut final_rank_mean = F64Interval::ZERO;
        for &(frame, offset, ref bonuses) in &fixed {
            let (bonus, support) = bonuses[order];
            let coefficient = i64::from(offset) + i64::from(frame <= prev);
            final_rank_mean = final_rank_mean.add(bonus.scale_integer(i128::from(coefficient)));
            lo += coefficient * i64::from(support.lower());
            hi += coefficient * i64::from(support.upper());
        }
        let (final_mean, final_support) = match deterministic {
            Some(scores) => (F64Interval::integer(scores[order] as i128), I32Interval::point(scores[order])),
            None => (final_note_mean.add(final_rank_mean), checked_support(lo, hi)?),
        };
        scores.push(LuckExactScore {
            final_mean: final_mean.into(),
            final_note_mean: final_note_mean.into(),
            final_rank_mean: final_rank_mean.into(),
            final_support: final_support.into(),
            final_life: schedule.final_lives[order],
            ranges,
            scored_notes,
            wide_notes,
            widest_note,
            peak_paths: replay.peak_paths,
            queries: schedule.queries,
            probability_peak_states: laws[order].peak_states,
            probability_transitions: laws[order].transitions,
            profile,
            notes,
        });
    }
    Ok(scores)
}
