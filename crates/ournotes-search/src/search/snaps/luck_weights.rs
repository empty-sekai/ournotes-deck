//! Expected lottery weights of the per-note bound under certified lottery laws.
//!
//! A note's score is its deterministic part times its luck factor `L` (1, or the Rush factor `l` while a Rush runs),
//! and the score ups of direct-Rush rows add to it only while their probe observes a Rush. Lottery outcomes change
//! nothing else in the score. With `a` the other score ups and `r` the direct-Rush ones,
//! `E[L (1 + a + r probe)] = E[L] (1 + a) + E[L probe] r`, so a law that bounds the note's class masses bounds its
//! expected score by `luck (1 + a) + probe_luck r` with `luck >= E[L]` and `probe_luck >= E[L probe]`.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use ournotes_sim::live::certified::ProbabilityMass;
use ournotes_sim::live::full::LuckDpCertifiedResult;

use super::Window;

/// The classes of [`LuckDpCertifiedResult::steps`]: neither, probe only, Rush only, Rush and probe.
const RUSH: [bool; 4] = [false, false, true, true];
/// The classes by their coefficient in `E[L probe]` for a Rush luck factor `l >= 1`: Rush and probe, probe only,
/// then the rest.
const PROBE_FIRST: [usize; 4] = [3, 1, 0, 2];

/// The certified laws that cover every team the bound describes, in every order.
pub(crate) struct LuckLaws<'l> {
    laws: &'l [Arc<LuckDpCertifiedResult>],
}

/// The luck weights last read and the laws and notes they were read for, so that bounds compiled again under the same
/// laws read them once.
#[derive(Default)]
pub(crate) struct WeightsMemo {
    held: RefCell<Option<Held>>,
}

/// Laws, the notes read under them and the weights read.
type Held = (Vec<Arc<LuckDpCertifiedResult>>, Vec<(i32, f64)>, Rc<[(f64, f64)]>);

impl WeightsMemo {
    /// [`LuckLaws::weights`] of `notes`, those last read when the laws (by address, in order) and notes are the same.
    pub(crate) fn weights(&self, laws: &LuckLaws<'_>, notes: &[(i32, f64)]) -> Rc<[(f64, f64)]> {
        if let Some((held, read, weights)) = &*self.held.borrow()
            && held.len() == laws.laws.len()
            && held.iter().zip(laws.laws).all(|(a, b)| Arc::ptr_eq(a, b))
            && read.len() == notes.len()
            && read.iter().zip(notes).all(|(a, b)| a.0 == b.0 && a.1.to_bits() == b.1.to_bits())
        {
            return weights.clone();
        }
        let weights: Rc<[(f64, f64)]> = laws.weights(notes).into();
        *self.held.borrow_mut() = Some((laws.laws.to_vec(), notes.to_vec(), weights.clone()));
        weights
    }
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
        let mut read = Vec::new();
        own.iter().all(|law| {
            if read.contains(&Arc::as_ptr(law)) {
                return true;
            }
            read.push(Arc::as_ptr(law));
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

    /// The luck weights `(luck, probe_luck)` of notes at the times `notes.0`, in nondecreasing order, whose luck
    /// factor in a Rush is `notes.1 >= 1`: the largest `E[L]` and `E[L probe]` over the distributions of its classes
    /// the note reads in each law. A note at a time reads one distribution: the state its time keeps once its lotteries
    /// are filed, or what a score query of its own judging frame (the first one at or after its time) reads before that
    /// frame's lottery, on the side of the frame's time the note is. With Rush mass `R`, `E[L] = 1 + (l - 1) R`; with
    /// probe masses `a` (probe only) and `b` (Rush and probe), `E[L probe] = a + l b`. A team whose law has no probe
    /// holder runs no direct-Rush score up, so `probe_luck` reads the laws with one; without any it is `luck`.
    ///
    /// Laws that share their curves read alike, so each curve is read once, in one pass over the notes, as a law with
    /// a probe holder when one of its laws has one.
    pub(crate) fn weights(&self, notes: &[(i32, f64)]) -> Vec<(f64, f64)> {
        assert!(notes.is_sorted_by_key(|&(time, _)| time), "notes in time order");
        let mut rush = vec![0f64; notes.len()];
        let held = |law: &LuckDpCertifiedResult| law.probes.iter().any(|&p| p);
        let mut probe = self.laws.iter().any(|law| held(law)).then(|| vec![0f64; notes.len()]);
        let mut curves: Vec<_> = self
            .laws
            .iter()
            .map(|law| ((law.steps.as_ptr(), law.frame_queries.as_ptr()), !held(law), &**law))
            .collect();
        curves.sort_unstable_by_key(|&(curve, bare, _)| (curve, bare));
        curves.dedup_by_key(|&mut (curve, _, _)| curve);
        let before = [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO];
        // The distributions a note reads at the current state of the pass: the Rush weight and the probe masses (probe
        // only, Rush and probe) of each.
        let mut reads_at: Vec<(f64, f64, f64)> = Vec::with_capacity(3);
        for (_, bare, law) in curves {
            let (mut step, mut query, mut state) = (0, 0, None);
            for (k, &(time, l)) in notes.iter().enumerate() {
                while law.steps.get(step).is_some_and(|(at, _)| *at <= time) {
                    step += 1;
                }
                while law.frame_queries.get(query).is_some_and(|(at, _)| *at < time) {
                    query += 1;
                }
                let read = law.frame_queries.get(query).map(|(at, reads)| (*at > time, reads));
                let key = (step, query, read.map(|(early, _)| early));
                if state != Some(key) {
                    state = Some(key);
                    let masses = step.checked_sub(1).map_or(&before, |index| &law.steps[index].1);
                    let reads: &[[ProbabilityMass; 4]] = match read {
                        Some((true, reads)) => &reads[..1],
                        Some((false, reads)) => &reads[1..],
                        None => &[],
                    };
                    reads_at.clear();
                    for masses in std::iter::once(masses).chain(reads) {
                        let (lo, hi) = (masses.map(|m| m.interval().lower()), masses.map(|m| m.interval().upper()));
                        let m = fill(lo, hi, PROBE_FIRST);
                        reads_at.push((lp_max(lo, hi, RUSH.map(f64::from)), m[1], m[3]));
                    }
                }
                for &(weight, a, b) in &reads_at {
                    rush[k] = rush[k].max(weight);
                    if let Some(probe) = probe.as_mut().filter(|_| !bare) {
                        probe[k] = probe[k].max((a + (l * b).next_up()).next_up());
                    }
                }
            }
        }
        notes
            .iter()
            .enumerate()
            .map(|(k, &(_, l))| {
                let luck = (1.0 + ((l - 1.0).next_up() * rush[k]).next_up()).next_up().min(l);
                (luck, probe.as_ref().map_or(luck, |probe| probe[k].min(luck)))
            })
            .collect()
    }
}

/// Direct-Rush windows (zero `ramp`) with their factors weighted per entry by `weight` (each at most 1): a window
/// splits into the runs of its entries that have one weight, each at that weight, and entries of no weight leave it.
/// Other windows stay.
pub(super) fn weigh_rush_windows(windows: Vec<Window>, weight: &[f64]) -> Vec<Window> {
    if windows.iter().all(|w| w.rush == 0 || w.ramp != 0) {
        return windows;
    }
    let scale = |factor: f64, r: f64| if factor == 0.0 { 0.0 } else { (factor * r).next_up() };
    let mut out = Vec::with_capacity(windows.len());
    for w in windows {
        if w.rush == 0 || w.ramp != 0 {
            out.push(w);
            continue;
        }
        let (mut lo, end) = (w.lo as usize, w.hi as usize);
        while lo < end {
            let r = weight[lo];
            let hi = lo + weight[lo..end].iter().take_while(|x| x.to_bits() == r.to_bits()).count();
            if r > 0.0 {
                out.push(Window {
                    lo: lo as u32,
                    hi: hi as u32,
                    note: scale(w.note, r),
                    judge: w.judge.map(|j| scale(j, r)),
                    ramp: 0,
                    rush: w.rush,
                });
            }
            lo = hi;
        }
    }
    out
}

/// An upper bound of `sum_b c_b m_b` over the masses `lo <= m <= hi` summing to one (`c >= 0`): the masses of
/// [`fill`] with the largest coefficients first. Rounding moves outward.
fn lp_max(lo: [f64; 4], hi: [f64; 4], c: [f64; 4]) -> f64 {
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&a, &b| c[b].total_cmp(&c[a]));
    let m = fill(lo, hi, order);
    (0..4).fold(0f64, |value, b| (value + (c[b] * m[b]).next_up()).next_up())
}

/// Masses at least `lo` that put the rest of the unit mass above the lower ends on the classes in `order`, each up
/// to its upper end `hi`. For every `c >= 0` whose coefficients do not increase along `order`, `sum_b c_b m_b` is at
/// least its largest value over the masses `lo <= m <= hi` summing to one. Rounding moves each mass up.
fn fill(lo: [f64; 4], hi: [f64; 4], order: [usize; 4]) -> [f64; 4] {
    let floor = lo.iter().fold(0f64, |sum, &x| (sum + x).next_down()).max(0.0);
    let mut rest = (1.0 - floor).next_up();
    let mut m = lo;
    for b in order {
        let add = (hi[b] - lo[b]).next_up().min(rest).max(0.0);
        m[b] = (m[b] + add).next_up();
        rest = (rest - add).next_up();
    }
    m
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

    fn tenths(tenths: [u64; 4]) -> [ProbabilityMass; 4] {
        tenths.map(|weight| ProbabilityMass::from_ratio(weight, 10).unwrap())
    }

    /// A law with one step at time 0 whose classes have the masses `tenths / 10`.
    fn law(tenths: [u64; 4], probe: bool, probe_transitions: Vec<u8>, starts: f64) -> Arc<LuckDpCertifiedResult> {
        timed(vec![(0, tenths)], Vec::new(), probe, probe_transitions, starts)
    }

    /// A law with the class masses `tenths / 10` of its steps and of its judging frames' score queries.
    fn timed(
        steps: Vec<(i32, [u64; 4])>,
        queries: Vec<(i32, [[u64; 4]; 3])>,
        probe: bool,
        probe_transitions: Vec<u8>,
        starts: f64,
    ) -> Arc<LuckDpCertifiedResult> {
        use ournotes_sim::live::certified::F64Interval;
        Arc::new(LuckDpCertifiedResult {
            probe_transitions,
            rush_transitions: Vec::new(),
            steps: steps.into_iter().map(|(time, masses)| (time, tenths(masses))).collect(),
            frame_queries: queries.into_iter().map(|(time, reads)| (time, reads.map(tenths))).collect(),
            probes: vec![probe],
            probe_starts: F64Interval::point(starts).unwrap(),
            range_moments: Vec::new(),
            peak_states: 0,
            transitions: 0,
        })
    }

    #[test]
    fn the_weights_take_the_largest_expectation_of_one_distribution() {
        // One law puts the probe on with the Rush, the other one without it; no distribution has both halves.
        let laws = [law([5, 0, 0, 5], true, Vec::new(), 0.0), law([0, 5, 5, 0], true, Vec::new(), 0.0)];
        let (luck, probe_luck) = LuckLaws::new(&laws).unwrap().weights(&[(1, 2.0)])[0];
        // E[L] = 1 + 0.5 in both; E[L probe] is 2 * 0.5 and 0.5
        assert!((1.5..1.5 + 1e-9).contains(&luck), "{luck}");
        assert!((1.0..1.0 + 1e-9).contains(&probe_luck), "{probe_luck}");
        // every distribution stays below them
        for (masses, held) in [([0.5, 0.0, 0.0, 0.5], true), ([0.0, 0.5, 0.5, 0.0], true)] {
            assert!(luck >= masses[0] + masses[1] + 2.0 * (masses[2] + masses[3]));
            assert!(!held || probe_luck >= masses[1] + 2.0 * masses[3]);
        }
    }

    #[test]
    fn a_note_reads_the_query_of_its_own_frame_on_its_side_of_the_frame_time() {
        // A judging frame at 10 whose query reads Rush only before its time, neither at it after the skill
        // boundary, and probe only at it before the boundary.
        let laws = [timed(
            vec![(0, [10, 0, 0, 0])],
            vec![(10, [[0, 0, 10, 0], [10, 0, 0, 0], [0, 10, 0, 0]])],
            true,
            Vec::new(),
            0.0,
        )];
        let weights = LuckLaws::new(&laws).unwrap().weights(&[(5, 2.0), (10, 2.0), (11, 2.0)]);
        let (luck, probe_luck) = weights[0];
        assert!((2.0..2.0 + 1e-9).contains(&luck) && probe_luck < 1e-9, "{luck} {probe_luck}");
        let (luck, probe_luck) = weights[1];
        assert!((1.0..1.0 + 1e-9).contains(&luck) && (1.0..1.0 + 1e-9).contains(&probe_luck), "{luck} {probe_luck}");
        // after the last judging frame: the kept state
        let (luck, probe_luck) = weights[2];
        assert!((1.0..1.0 + 1e-9).contains(&luck) && probe_luck < 1e-9, "{luck} {probe_luck}");
    }

    #[test]
    fn each_time_reads_the_state_its_own_steps_and_query_give() {
        // Rush only from 0, a query at 10 that reads probe only before its time, then neither from 11.
        let laws = [timed(
            vec![(0, [0, 0, 10, 0]), (11, [10, 0, 0, 0])],
            vec![(10, [[0, 10, 0, 0], [0, 0, 10, 0], [0, 0, 10, 0]])],
            true,
            Vec::new(),
            0.0,
        )];
        // (time, Rush luck factor), then the expected luck and probe weights
        let cases = [
            ((1, 2.0), 2.0, 1.0),
            ((5, 2.0), 2.0, 1.0),
            ((5, 1.5), 1.5, 1.0),
            ((10, 2.0), 2.0, 0.0),
            ((11, 2.0), 1.0, 0.0),
            ((12, 2.0), 1.0, 0.0),
        ];
        let weights = LuckLaws::new(&laws).unwrap().weights(&cases.map(|(note, _, _)| note));
        for (&(note, luck, probe_luck), &(l, p)) in cases.iter().zip(&weights) {
            assert!(
                (luck..luck + 1e-9).contains(&l) && (probe_luck..probe_luck + 1e-9).contains(&p),
                "{note:?} {l} {p}"
            );
        }
    }

    #[test]
    fn laws_that_share_their_curves_read_as_a_probe_holder_when_one_has_one() {
        let held = law([5, 1, 1, 3], true, Vec::new(), 0.0);
        let bare = Arc::new(LuckDpCertifiedResult { probes: vec![false], ..(*held).clone() });
        let other = law([2, 0, 8, 0], false, Vec::new(), 0.0);
        for laws in [[bare.clone(), held.clone(), other.clone()], [held.clone(), other.clone(), bare.clone()]] {
            let (luck, probe_luck) = LuckLaws::new(&laws).unwrap().weights(&[(10, 2.0)])[0];
            assert!((luck - 1.8).abs() < 1e-9 && (probe_luck - 0.7).abs() < 1e-9, "{luck} {probe_luck}");
        }
        let (luck, probe_luck) = LuckLaws::new(&[bare, other]).unwrap().weights(&[(10, 2.0)])[0];
        assert_eq!(luck, probe_luck);
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
        let (luck, probe_luck) = LuckLaws::new(&both).unwrap().weights(&[(10, 2.0)])[0];
        // the larger expected luck factor is the bare law's 0.2 + 2 * 0.8
        assert!((luck - 1.8).abs() < 1e-9, "{luck}");
        // the probe classes of the law with a holder: 0.1 + 2 * 0.3
        assert!((probe_luck - 0.7).abs() < 1e-9, "{probe_luck}");
        let starts = LuckLaws::new(&both).unwrap().probe_starts();
        assert_eq!((starts.most, starts.expected, starts.luck), (2.0, 1.25, 1.0));
        // no law with a holder: the probe weight is the luck weight and no start counts
        let bare = [bare];
        let (luck, probe_luck) = LuckLaws::new(&bare).unwrap().weights(&[(10, 2.0)])[0];
        assert_eq!(luck, probe_luck);
        let starts = LuckLaws::new(&bare).unwrap().probe_starts();
        assert_eq!((starts.most, starts.expected), (0.0, 0.0));
        // before the first step the lottery has not run
        let (luck, probe_luck) = LuckLaws::new(&[held]).unwrap().weights(&[(-1, 2.0), (10, 2.0)])[0];
        assert!((1.0..1.0 + 1e-9).contains(&luck) && (0.0..1e-9).contains(&probe_luck), "{luck} {probe_luck}");
    }

    #[test]
    fn direct_rush_windows_take_the_weight_of_each_entry() {
        let window = |rush| Window { lo: 1, hi: 7, note: 0.5, judge: [0.0, 0.25, 0.0, 0.0], ramp: 0, rush };
        let weight = [0.9, 0.0, 0.0, 0.3, 0.3, 0.6, 0.3, 1.0];
        let out = weigh_rush_windows(vec![window(1), window(0)], &weight);
        // Entries 1 and 2 weigh nothing, 3 and 4 share a weight, and 5 and 6 each have their own.
        let rush: Vec<_> = out.iter().filter(|w| w.rush != 0).collect();
        assert_eq!(rush.iter().map(|w| (w.lo, w.hi)).collect::<Vec<_>>(), [(3, 5), (5, 6), (6, 7)]);
        for w in rush {
            let r = weight[w.lo as usize];
            assert!(w.note >= 0.5 * r && w.note - 0.5 * r < 1e-15, "{w:?}");
            assert!(w.judge[1] >= 0.25 * r && w.judge[1] - 0.25 * r < 1e-15, "{w:?}");
            assert_eq!([w.judge[0], w.judge[2], w.judge[3]], [0.0; 3]);
        }
        // An ordinary window stays as it is.
        assert!(out.iter().any(|w| w.rush == 0 && (w.lo, w.hi, w.note) == (1, 7, 0.5)));
    }

    #[test]
    fn point_masses_give_their_expectation_or_more() {
        let m = [0.5, 0.1, 0.25, 0.15];
        let value = lp_max(m, m, [1.0, 1.0, 1.5, 1.5]);
        let exact = 0.5 + 0.1 + 1.5 * 0.4;
        assert!(value >= exact && value - exact < 1e-12, "{value}");
    }
}
