//! Expected lottery weights of the per-note bound under certified lottery laws.
//!
//! A note's score is its deterministic part times its luck factor `L` (1, or the Rush factor `l` while a Rush runs),
//! and the score ups of direct-Rush rows add to it only while their probe observes a Rush. Lottery outcomes change
//! nothing else in the score. With `a` the other score ups and `r` the direct-Rush ones,
//! `E[L (1 + a + r probe)] = E[L] (1 + a) + E[L probe] r`, so a law that bounds the note's class masses bounds its
//! expected score by `luck (1 + a) + probe_luck r` with `luck >= E[L]` and `probe_luck >= E[L probe]`.
use std::sync::Arc;

use ournotes_sim::live::full::LuckDpCertifiedResult;

use super::Window;

/// The most pieces one direct-Rush window splits into, each at its largest weight.
const PIECES: usize = 16;

/// The classes of [`LuckDpCertifiedResult::steps`]: neither, probe only, Rush only, Rush and probe.
const PROBE: [bool; 4] = [false, true, false, true];
const RUSH: [bool; 4] = [false, false, true, true];

/// The certified laws that cover every team the bound describes, in every order.
pub(crate) struct LuckLaws<'l> {
    laws: &'l [Arc<LuckDpCertifiedResult>],
}

/// The play frames whose skill boundary turns the direct-probe class on, over the laws. A direct-Rush row that
/// activates once per such frame files its start and end commands once for each.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProbeStarts {
    /// The most of them in one play.
    pub(crate) most: f64,
    /// The largest expected number of them.
    pub(crate) expected: f64,
    /// The largest ratio of an entry's Rush luck factor to its expected luck factor.
    pub(crate) luck: f64,
}

impl ProbeStarts {
    /// An upper bound of `E[L drift(n)] / luck` for an entry with luck factor `L` and `luck >= E[L]`, `n` the starts
    /// of the play and `drift` convex and nondecreasing with `fixed = drift(0)` and `most = drift(self.most)`. A
    /// convex function is below its chord, `drift(n) - fixed <= n / self.most * (most - fixed)`, so
    /// `E[L drift(n)] <= E[L] fixed + max(L) E[n] / self.most * (most - fixed)`, and `max(L) <= self.luck * luck`.
    pub(crate) fn expected_drift(&self, fixed: f64, most: f64) -> f64 {
        if !(fixed.is_finite() && most.is_finite() && self.luck.is_finite()) || most <= fixed || self.most <= 0.0 {
            return most.max(fixed);
        }
        let share = (self.expected / self.most).next_up().min(1.0);
        let rise = ((most - fixed).next_up() * share).next_up();
        (fixed + (rise * self.luck).next_up()).next_up()
    }

    /// Command counts after the most starts of one play, `run` counts for each start on top of `fixed` ones.
    pub(crate) fn at_most(&self, fixed: f64, run: f64) -> f64 {
        if run == 0.0 { fixed } else { (fixed + (run * self.most).next_up()).next_up() }
    }
}

impl<'l> LuckLaws<'l> {
    /// None without a law: every bound then takes the largest luck factor.
    pub(crate) fn new(laws: &'l [Arc<LuckDpCertifiedResult>]) -> Option<Self> {
        (!laws.is_empty()).then_some(Self { laws })
    }

    /// Whether bounds under these laws cover a team whose performance orders have the laws `own`: each of them is
    /// one of these laws, or equal to one in everything the bounds read.
    pub(crate) fn cover(&self, own: &[Arc<LuckDpCertifiedResult>]) -> bool {
        let same = |a: &LuckDpCertifiedResult, b: &LuckDpCertifiedResult| {
            (a.steps == b.steps && a.frame_queries == b.frame_queries && a.probes == b.probes)
                && (a.probe_transitions == b.probe_transitions && a.probe_starts == b.probe_starts)
        };
        own.iter().all(|law| {
            self.laws.iter().any(|known| Arc::ptr_eq(known, law)) || self.laws.iter().any(|known| same(known, law))
        })
    }

    /// The starts of the direct-probe class over the laws with a probe holder, with a unit luck ratio: a team
    /// whose law has none runs no direct-Rush score up.
    pub(crate) fn probe_starts(&self) -> ProbeStarts {
        let (mut most, mut expected) = (0usize, 0f64);
        for law in self.laws.iter().filter(|law| law.probes.iter().any(|&p| p)) {
            most = most.max(law.probe_transitions.iter().filter(|&&edges| edges & 0b10 != 0).count());
            expected = expected.max(law.probe_starts.upper());
        }
        let most = most as f64;
        ProbeStarts { most, expected: expected.min(most), luck: 1.0 }
    }

    /// The class mass enclosure of a note at `time_ms`, over every reading of the note, of the laws `held` keeps:
    /// the state its time keeps once its lotteries are filed, and the state a score query of the first judging
    /// frame at or after its time reads before that frame's lottery. None when `held` keeps no law.
    fn hull(&self, time_ms: i32, held: impl Fn(&LuckDpCertifiedResult) -> bool) -> Option<([f64; 4], [f64; 4])> {
        let (mut lo, mut hi) = ([1f64; 4], [0f64; 4]);
        let mut any = false;
        let mut take = |masses: &[ournotes_sim::live::certified::ProbabilityMass; 4]| {
            for (b, mass) in masses.iter().enumerate() {
                let interval = mass.interval();
                lo[b] = lo[b].min(interval.lower());
                hi[b] = hi[b].max(interval.upper());
            }
        };
        for law in self.laws.iter().filter(|law| held(law)) {
            any = true;
            let index = law.steps.partition_point(|(time, _)| *time <= time_ms);
            match index.checked_sub(1) {
                Some(index) => take(&law.steps[index].1),
                None => take(&[
                    ournotes_sim::live::certified::ProbabilityMass::ONE,
                    ournotes_sim::live::certified::ProbabilityMass::ZERO,
                    ournotes_sim::live::certified::ProbabilityMass::ZERO,
                    ournotes_sim::live::certified::ProbabilityMass::ZERO,
                ]),
            }
            let index = law.frame_queries.partition_point(|(time, _)| *time < time_ms);
            if let Some((_, reads)) = law.frame_queries.get(index) {
                for masses in reads {
                    take(masses);
                }
            }
        }
        any.then_some((lo, hi))
    }

    /// `(luck, probe_luck)` of a note at `time_ms` whose luck factor in a Rush is `l >= 1`. A team whose law has no
    /// probe holder runs no direct-Rush score up, so `probe_luck` reads the laws with one; without any it is `luck`.
    pub(crate) fn entry(&self, time_ms: i32, l: f64) -> (f64, f64) {
        let factor = RUSH.map(|rush| if rush { l } else { 1.0 });
        let luck = match self.hull(time_ms, |_| true) {
            Some((lo, hi)) => lp_max(lo, hi, factor).min(l),
            None => l,
        };
        let probe_factor = std::array::from_fn(|b| if PROBE[b] { factor[b] } else { 0.0 });
        match self.hull(time_ms, |law| law.probes.iter().any(|&p| p)) {
            Some((lo, hi)) => (luck, lp_max(lo, hi, probe_factor).min(luck)),
            None => (luck, luck),
        }
    }
}

/// Direct-Rush windows (zero `ramp`) with their factors weighted per entry by `weight` (each at most 1): a window
/// splits into at most [`PIECES`] runs of entries, each at the largest weight of its entries. Other windows stay.
pub(super) fn weigh_rush_windows(windows: Vec<Window>, weight: &[f64]) -> Vec<Window> {
    if windows.iter().all(|w| w.rush == 0 || w.ramp != 0) {
        return windows;
    }
    let mut out = Vec::with_capacity(windows.len());
    for w in windows {
        if w.rush == 0 || w.ramp != 0 {
            out.push(w);
            continue;
        }
        let step = ((w.hi - w.lo) as usize).div_ceil(PIECES).max(1);
        let mut lo = w.lo as usize;
        while lo < w.hi as usize {
            let hi = (lo + step).min(w.hi as usize);
            let r = weight[lo..hi].iter().copied().fold(0f64, f64::max);
            if r > 0.0 {
                out.push(Window {
                    lo: lo as u32,
                    hi: hi as u32,
                    note: (w.note * r).next_up(),
                    judge: w.judge.map(|j| (j * r).next_up()),
                    ramp: 0,
                    rush: w.rush,
                });
            }
            lo = hi;
        }
    }
    out
}

/// An upper bound of `sum_b c_b m_b` over the masses `lo <= m <= hi` summing to one (`c >= 0`): the lower ends,
/// then the rest of the unit mass to the largest coefficients first. Rounding moves outward.
fn lp_max(lo: [f64; 4], hi: [f64; 4], c: [f64; 4]) -> f64 {
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&a, &b| c[b].total_cmp(&c[a]));
    let floor = lo.iter().fold(0f64, |sum, &x| (sum + x).next_down()).max(0.0);
    let mut rest = (1.0 - floor).next_up();
    let mut value = 0f64;
    for (b, &x) in lo.iter().enumerate() {
        value = (value + (c[b] * x).next_up()).next_up();
    }
    for b in order {
        let add = (hi[b] - lo[b]).next_up().min(rest).max(0.0);
        value = (value + (c[b] * add).next_up()).next_up();
        rest = (rest - add).next_up();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_linear_bound_puts_free_mass_on_the_largest_coefficients() {
        let lo = [0.2, 0.0, 0.1, 0.0];
        let hi = [0.9, 0.0, 0.5, 0.3];
        // Free mass 0.7 goes to class 3 (0.3) and class 2 (0.4).
        let value = lp_max(lo, hi, [1.0, 1.0, 2.0, 2.0]);
        assert!((value - (0.2 + 2.0 * 0.5 + 2.0 * 0.3)).abs() < 1e-12, "{value}");
        assert!(value >= 0.2 + 2.0 * 0.5 + 2.0 * 0.3);
    }

    #[test]
    fn the_expected_drift_stays_above_the_mean_of_a_convex_drift() {
        // drift(n) = n^2 on 0..=4 with every count equally likely and a luck factor of 1 or 1.5 at each count.
        let starts = ProbeStarts { most: 4.0, expected: 2.0, luck: 1.5 };
        let bound = starts.expected_drift(0.0, 16.0);
        let weighted: f64 = (0..=4).map(|n| 1.5 * (n * n) as f64 / 5.0).sum();
        assert!(bound >= weighted, "{bound} {weighted}");
        assert!(bound <= 16.0 * 1.5);
        // without a certificate at the most starts there is none
        assert_eq!(starts.expected_drift(1.0, f64::INFINITY), f64::INFINITY);
        assert!(starts.at_most(3.0, 2.0) >= 11.0 && starts.at_most(3.0, 2.0) < 11.001);
        assert_eq!(starts.at_most(3.0, 0.0), 3.0);
        // no possible start: the fixed drift
        let none = ProbeStarts { most: 0.0, expected: 0.0, luck: 1.5 };
        assert_eq!(none.expected_drift(0.25, 0.25), 0.25);
    }

    /// A law with one step at time 0 whose classes have the masses `tenths / 10`.
    fn law(tenths: [u64; 4], probe: bool, probe_transitions: Vec<u8>, starts: f64) -> Arc<LuckDpCertifiedResult> {
        use ournotes_sim::live::certified::{F64Interval, ProbabilityMass};
        Arc::new(LuckDpCertifiedResult {
            probe_transitions,
            rush_transitions: Vec::new(),
            steps: vec![(0, tenths.map(|weight| ProbabilityMass::from_ratio(weight, 10).unwrap()))],
            frame_queries: Vec::new(),
            probes: vec![probe],
            probe_starts: F64Interval::point(starts).unwrap(),
            range_moments: Vec::new(),
            peak_states: 0,
            transitions: 0,
        })
    }

    #[test]
    fn laws_cover_a_team_whose_laws_they_hold_or_read_alike() {
        let (a, b) = (law([5, 1, 1, 3], true, vec![0b10], 1.0), law([6, 1, 1, 2], false, vec![0b00], 0.0));
        let laws = [a.clone(), b];
        let laws = LuckLaws::new(&laws).unwrap();
        assert!(laws.cover(&[a.clone(), a.clone()]));
        // Another law that reads as one of them in everything the bounds read.
        let mut alike = (*a).clone();
        (alike.peak_states, alike.transitions, alike.rush_transitions) = (7, 9, vec![1]);
        assert!(laws.cover(&[Arc::new(alike), a.clone()]));
        // A law with other masses, probes, probe frames or starts.
        assert!(!laws.cover(&[a, law([5, 2, 1, 2], true, vec![0b10], 1.0)]));
        assert!(!laws.cover(&[law([5, 1, 1, 3], false, vec![0b10], 1.0)]));
        assert!(!laws.cover(&[law([5, 1, 1, 3], true, vec![0b11], 1.0)]));
        assert!(!laws.cover(&[law([5, 1, 1, 3], true, vec![0b10], 0.5)]));
    }

    #[test]
    fn the_probe_weight_reads_the_laws_with_a_probe_holder() {
        let held = law([5, 1, 1, 3], true, vec![0b10, 0b01, 0b10], 1.25);
        let bare = law([2, 0, 8, 0], false, vec![0b10; 7], 3.0);
        let both = [held.clone(), bare.clone()];
        let (luck, probe_luck) = LuckLaws::new(&both).unwrap().entry(10, 2.0);
        // the larger expected luck factor is the bare law's 0.2 + 2 * 0.8
        assert!((luck - 1.8).abs() < 1e-9, "{luck}");
        // the probe classes of the law with a holder: 0.1 + 2 * 0.3
        assert!((probe_luck - 0.7).abs() < 1e-9, "{probe_luck}");
        let starts = LuckLaws::new(&both).unwrap().probe_starts();
        assert_eq!((starts.most, starts.expected, starts.luck), (2.0, 1.25, 1.0));
        // no law with a holder: the probe weight is the luck weight and no start counts
        let bare = [bare];
        let (luck, probe_luck) = LuckLaws::new(&bare).unwrap().entry(10, 2.0);
        assert_eq!(luck, probe_luck);
        let starts = LuckLaws::new(&bare).unwrap().probe_starts();
        assert_eq!((starts.most, starts.expected), (0.0, 0.0));
        // before the first step the lottery has not run
        let (luck, probe_luck) = LuckLaws::new(&[held]).unwrap().entry(-1, 2.0);
        assert!((1.0..1.0 + 1e-9).contains(&luck) && (0.0..1e-9).contains(&probe_luck), "{luck} {probe_luck}");
    }

    #[test]
    fn point_masses_give_their_expectation_or_more() {
        let m = [0.5, 0.1, 0.25, 0.15];
        let value = lp_max(m, m, [1.0, 1.0, 1.5, 1.5]);
        let exact = 0.5 + 0.1 + 1.5 * 0.4;
        assert!(value >= exact && value - exact < 1e-12, "{value}");
    }
}
