//! The Gekisou combo factor: its ramp counts, the carrier factors and the gated bonus windows.

use super::*;

/// What a Gekisou combo factor reads, entries in chart-time order. The factor of a note is `min(table(c), 1) + 1`,
/// `c` the combo count of the first combo range containing its chart time as the range's recount left it at the last
/// of the range's judged notes with a chart time up to the end of the score frame before the note's (the range's
/// history is searched by chart time, which finds that note when the range's notes that enter its history are judged
/// in chart-time order). Each judgement adds at most `floor(1 + the running combo bonuses)` to the count, so `c` is at
/// most the sum of that over the range's entries that can add to it up to that time; the table lookup returns the
/// value of some threshold at most `c`.
#[derive(Clone, Debug)]
pub(crate) struct GkCombo {
    /// Per entry: its combo range (index into the ranges) and the number of that range's `entries` with a chart time
    /// up to the end of the previous score frame.
    pub(crate) range: Vec<Option<u32>>,
    pub(crate) upto: Vec<u32>,
    /// Per range: its entries in chart order that can add to its count (empty for a range that is not a combo
    /// range), and whether its entries that can enter its history are judged in chart-time order (else its notes take
    /// `gmax`).
    pub(crate) entries: Vec<Vec<u32>>,
    pub(crate) ordered: Vec<bool>,
    /// Table thresholds in ascending order, each with the largest factor at a count from 0 up to it.
    pub(crate) tab: Vec<(i64, f64)>,
    pub(crate) gmax: f64,
    /// Per entry: the number of play frames with a time up to its chart time.
    pub(crate) frame: Vec<u32>,
    /// Per play frame: what a combo count updated in the frame's skill phase reads, the playing range's combo after
    /// the judgements of the earlier frames: an index into `fill`'s sums, `READ_ZERO` without a playing range, or
    /// `READ_UNKNOWN` (a range without sums).
    pub(crate) read: Vec<u32>,
    /// Per play frame: the first frame of its run of frames with the same playing range.
    pub(crate) run: Vec<u32>,
}

/// `GkCombo::read` of a frame without a playing range (the count is 0) and of one whose playing range has no sums.
const READ_ZERO: u32 = u32::MAX - 1;

const READ_UNKNOWN: u32 = u32::MAX;

/// The executions one factor window of a row covers: each one's start frame and the earliest time its first factor
/// can be filed at, by that time ascending (`at`); `lo[i]` and `hi[i]` are the least and the greatest start frame
/// among the first `i + 1`.
#[derive(Clone, Debug, Default)]
pub(crate) struct RampStarts {
    pub(crate) at: Vec<i64>,
    pub(crate) lo: Vec<u32>,
    pub(crate) hi: Vec<u32>,
}

impl RampStarts {
    /// From `(earliest filing time, start frame)` pairs.
    pub(crate) fn new(mut starts: Vec<(i64, usize)>) -> RampStarts {
        starts.sort_unstable();
        let mut out = RampStarts::default();
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for (a, f) in starts {
            let f = u32::try_from(f).expect("play frame index fits in u32");
            (lo, hi) = (lo.min(f), hi.max(f));
            out.at.push(a);
            out.lo.push(lo);
            out.hi.push(hi);
        }
        out
    }
}

impl GkCombo {
    /// The largest factor at a count up to `c`.
    pub(crate) fn factor(&self, c: f64) -> f64 {
        let i = self.tab.partition_point(|x| (x.0 as f64) <= c);
        if i == 0 { 1.0 } else { self.tab[i - 1].1 }
    }

    /// The factor bound of every entry, from the largest running combo bonus at the chart time of each entry of a
    /// combo range (`bonus(range, position in its entries, count bound of the earlier entries, entries at its chart
    /// time from it on)`); `sums` is scratch.
    pub(crate) fn fill(
        &self,
        times: &[i32],
        mut bonus: impl FnMut(usize, usize, f64, usize) -> f64,
        out: &mut Vec<f64>,
        sums: &mut Vec<f64>,
    ) {
        let ne = self.range.len();
        out.clear();
        out.resize(ne, 1.0);
        sums.clear();
        let mut at = Vec::with_capacity(self.entries.len());
        for (ri, list) in self.entries.iter().enumerate() {
            at.push(sums.len());
            if !self.ordered[ri] {
                continue;
            }
            let mut acc = 0f64;
            sums.push(0.0);
            for q in 0..list.len() {
                let t = times[list[q] as usize];
                let group = list[q..].iter().take_while(|&&e| times[e as usize] == t).count();
                acc += (1.0 + bonus(ri, q, acc, group).max(0.0)).floor();
                sums.push(acc);
            }
        }
        for e in 0..ne {
            if let Some(ri) = self.range[e] {
                let ri = ri as usize;
                out[e] = if self.ordered[ri] { self.factor(sums[at[ri] + self.upto[e] as usize]) } else { self.gmax };
            }
        }
    }

    /// After `fill` (`sums`): an upper bound on the combo count whose cumulative factor entry `e` (chart time `t`)
    /// reads from an execution of a window with starts `starts`, NaN where unknown. An updater counts in the skill
    /// phase of every frame it runs in and files a changed factor at the frame's time; its first factor may be
    /// backdated to its trigger time. Its end lands no later than the time of the frame that processes it, so an
    /// execution started in frame `f` whose factor the entry reads still runs in the last frame `F` with a time up
    /// to `t`, and the entry reads the count of frame `max(f, F)`. Within a run of frames with the same playing
    /// range, the count does not decrease.
    pub(crate) fn ramp_count(&self, sums: &[f64], starts: &RampStarts, e: usize, t: i64) -> f64 {
        let i = starts.at.partition_point(|&a| a <= t);
        if i == 0 {
            return 0.0;
        }
        let (lo, hi) = (starts.lo[i - 1] as i64, starts.hi[i - 1] as i64);
        let last = self.frame[e] as i64 - 1;
        let count = |f: usize| match self.read[f] {
            READ_ZERO => 0.0,
            READ_UNKNOWN => f64::NAN,
            k => sums[k as usize],
        };
        let mut best = 0f64;
        if lo <= last {
            best = count(last as usize);
            if best.is_nan() {
                return f64::NAN;
            }
        }
        // the backdated first factors of executions started after `last`
        let floor = lo.max(last + 1);
        let mut f = hi;
        while f >= floor {
            let c = count(f as usize);
            if c.is_nan() {
                return f64::NAN;
            }
            best = best.max(c);
            f = self.run[f as usize] as i64 - 1;
        }
        best
    }

    /// The read data (`frame`, `read`, `run`) of the combo ranges `entries`/`ordered` (as in `fill`'s sums) for
    /// entries at `times` judged in the play frames `judged` (frame times `frames`) with the playing range of every
    /// frame `current`.
    pub(crate) fn reads(
        entries: &[Vec<u32>],
        ordered: &[bool],
        times: &[i32],
        judged: &[usize],
        frames: &[i32],
        current: &[Option<usize>],
    ) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let frame = times
            .iter()
            .map(|&t| u32::try_from(frames.partition_point(|&x| x <= t)).expect("play frame count fits in u32"))
            .collect();
        // per ordered combo range: its offset in the sums and, by judgement frame, the shortest chart-order prefix
        // of its entries holding every entry judged up to that frame
        let mut offset = vec![None; entries.len()];
        let mut prefix: Vec<Vec<(usize, usize)>> = vec![Vec::new(); entries.len()];
        let mut at = 0usize;
        for (ri, list) in entries.iter().enumerate() {
            if !ordered[ri] {
                continue;
            }
            offset[ri] = Some(at);
            at += list.len() + 1;
            let mut by: Vec<(usize, usize)> =
                list.iter().enumerate().map(|(q, &e)| (judged[e as usize], q + 1)).collect();
            by.sort_unstable();
            let mut k = 0;
            for x in by.iter_mut() {
                k = k.max(x.1);
                x.1 = k;
            }
            prefix[ri] = by;
        }
        let read = (0..frames.len())
            .map(|f| match current[f] {
                None => READ_ZERO,
                Some(r) => match offset[r] {
                    None => READ_UNKNOWN,
                    Some(at) => {
                        let p = &prefix[r];
                        let i = p.partition_point(|x| x.0 < f);
                        let k = if i == 0 { 0 } else { p[i - 1].1 };
                        u32::try_from(at + k).expect("sums index fits in u32")
                    }
                },
            })
            .collect();
        let mut run = Vec::with_capacity(frames.len());
        for f in 0..frames.len() {
            run.push(if f > 0 && current[f] == current[f - 1] { run[f - 1] } else { f as u32 });
        }
        (frame, read, run)
    }
}

/// The Gekisou combo factor bound of every deck whose members bring combo bonus windows in at most `n` slots (its
/// carriers; `member_cb`: each allowed member's distinct window lists over its classes). The running bonus of such a
/// deck at a time is at most the sum of the `n` largest members' there (each the largest over its lists), and the
/// count it builds, which opens the gates, is at most the one this bonus builds. `n = 5` covers every deck. `sums`
/// receives the same decks' combo count bounds (`GkCombo::fill`'s sums, which `GkCombo::ramp_count` reads).
pub(crate) fn carrier_factors(
    gc: &GkCombo,
    times: &[i32],
    member_cb: &[Vec<Vec<ComboBonusRow>>],
    n: usize,
    out: &mut Vec<f64>,
    sums: &mut Vec<f64>,
) {
    keyed_factors(gc, times, member_cb, &[], n, out, sums);
}

/// `carrier_factors` of the decks with carriers of the window lists `placed` and at most `r` other carriers: the
/// running bonus of such a deck at a time is at most `placed`'s there plus the `r` largest members'.
pub(crate) fn keyed_factors(
    gc: &GkCombo,
    times: &[i32],
    member_cb: &[Vec<Vec<ComboBonusRow>>],
    placed: &[&[ComboBonusRow]],
    r: usize,
    out: &mut Vec<f64>,
    sums: &mut Vec<f64>,
) {
    // each running bonus is at least a candidate's, so each running count is at least its count
    gc.fill(
        times,
        |ri, q, acc, group| {
            let t = times[gc.entries[ri][q] as usize] as i64;
            let running = |l: &[ComboBonusRow], open: &dyn Fn(&ComboBonusRow) -> bool| {
                l.iter().filter(|w| w.0 <= t && t <= w.1 && open(w)).map(|w| w.2).sum::<f64>()
            };
            let top = |open: &dyn Fn(&ComboBonusRow) -> bool| {
                let mut vals: Vec<f64> =
                    member_cb.iter().map(|lists| lists.iter().map(|l| running(l, open)).fold(0f64, f64::max)).collect();
                vals.sort_by(|a, b| b.total_cmp(a));
                placed.iter().map(|l| running(l, open)).sum::<f64>() + vals.iter().take(r).sum::<f64>()
            };
            // the windows a deck's gates open are among those the gates open with these larger bonuses and counts
            gated_bonus(ri, acc, group, top)
        },
        out,
        sums,
    );
}

/// The note factor a combo ramp window `w` adds at an entry whose playing range's combo count is at most `count`
/// (NaN where no ramp may be read: the flat factor). The table does not decrease with the count.
pub(crate) fn ramp_factor(w: &Window, ramp: &ComboRamp, count: f64) -> f64 {
    if count.is_nan() {
        return w.note;
    }
    let steps = (ournotes_sim::num::floor_to_i32(count as f32 / ramp.unit as f32) as i64).clamp(0, ramp.max_count);
    ramp.table[(steps as usize).min(ramp.table.len() - 1)] * ramp.mult
}

/// Whether a combo bonus window with `gate` counts at an entry of combo range `ri`: always without a gate or when it
/// names another range, else once the range's combo can reach the threshold by the entry's chart time, when the count
/// its trigger reads there is at most `reach` (see [`gated_bonus`]).
pub(crate) fn gate_open(gate: Option<(i64, u32)>, ri: usize, reach: f64) -> bool {
    match gate {
        None => true,
        Some((threshold, range)) => range as usize != ri || reach >= threshold as f64,
    }
}

/// The most one judgement at a gated window's chart time adds to the count its trigger reads, when the running
/// bonuses of the other windows there total at most `others`. An entry counts the bonus only once the first of the
/// window's executions has started, at a trigger time no later than the entry's: at an earlier time the count is at
/// most the earlier entries' bound, and at the same time it reads judgements none of those executions reached.
pub(crate) fn gate_step(others: f64) -> f64 {
    (1.0 + others.max(0.0)).floor()
}

/// The running combo bonus at an entry of combo range `ri`, from `total`, the running bonus of the windows an `open`
/// predicate admits there: `acc` bounds the count of the earlier entries and `group` is the number of entries at the
/// entry's chart time from it on.
///
/// A window gated on the range's count counts once its trigger can read the threshold. The trigger reads the earlier
/// entries' count and, at the entry's chart time, judgements that each add at most `gate_step` of the bonuses of the
/// windows started before it. The windows counted are the least fixed point from the ungated ones: those whose
/// threshold `acc + group * gate_step(bonus)` reaches, `bonus` the running bonus of the windows counted so far. Every
/// window running at a judgement of a play is among them: ordered by their triggers, each one's trigger reads
/// judgements that ran with the windows started before it, which the fixed point holds by then.
pub(crate) fn gated_bonus(
    ri: usize,
    acc: f64,
    group: usize,
    total: impl Fn(&dyn Fn(&ComboBonusRow) -> bool) -> f64,
) -> f64 {
    let mut reach = f64::NEG_INFINITY;
    loop {
        let bonus = total(&|w| gate_open(w.3, ri, reach));
        let next = acc + group as f64 * gate_step(bonus);
        if next <= reach {
            return bonus;
        }
        reach = next;
    }
}

/// The running combo bonus of a list of bonus windows at each time of a non-decreasing sequence (`windows`: `(start,
/// end, bonus)`, closed; `ev` is scratch).
pub(crate) fn bonus_at(windows: &[(i64, i64, f64)], ev: &mut Vec<(i64, f64)>) {
    ev.clear();
    for &(a, b, v) in windows {
        ev.push((a, v));
        ev.push((b.saturating_add(1), -v));
    }
    ev.sort_by_key(|x| x.0);
}
