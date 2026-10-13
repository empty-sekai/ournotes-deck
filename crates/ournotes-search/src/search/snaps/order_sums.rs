//! The fine cap of a complete team summed over performance orders, from terms of each member and class.
//!
//! The sum is that of [`FineView::fine_bound`] over the orders, without conversion sources, a known life or Rush
//! masks, in expected mode. Without conversions an entry has its own judgement `x` in every order (mask
//! `m = 1 << x`), and its expected bound is linear in the factors of the windows open at it:
//! - a window's note and judgement factors count by their positive parts, which bound the clamped sums;
//! - `max(mjp[m] B, jp4[m][j] (B + A_j)) <= mjp[m] B + Σ_j jp4[m][j] A_j`, as `0 <= jp4[m][j] <= mjp[m]` and
//!   `A_j >= 0`, an equality for a single judgement;
//! - the combo, its breaks and the Gekisou combo factors are the same in every order (the combo bonus windows of a
//!   class do not depend on its position).
//!
//! With `c_e` the power-free coefficient of entry `e` (its note factor, combo factor bound, life factor and rank
//! factor), the sum over `n` orders is `P (1 + chain) ((n + drift) Σ_e c_e mjp_e + Σ w (N + J))`, summed over the
//! parts, `w` the number of orders that place a part at its position: `N` is the part's note factors, each weighed
//! by `Σ c_e mjp_e` over its window (a combo ramp window's factor at each entry, read at the combo counts of the
//! team's carriers), and `J` its judgement factors, weighed by `Σ c_e jp4_e[j]`. The coefficients depend on the team
//! only through the combo bonus windows of its Gekisou combo carriers, so the terms of every part are computed once
//! per set of carriers. `drift` sums each order's drift ([`FineView::cand_drift`]) from its own command counts, at
//! the factor peaks of every slot at any of its positions, at least that order's peaks.
//!
//! Every input is nonnegative. The window weights are differences of prefix sums, which carry the absolute
//! error bound of the summation; every other sum and product rounds up, so the result is at least the exact sum.
use super::*;
use ournotes_sim::num::FxHashMap;
use std::cell::OnceCell;

/// Sets of carriers whose coefficients one bound keeps.
const CARRIER_SETS: usize = 16;

/// The carriers of a team in slot order, `(member, class)`, then `(u32::MAX, u32::MAX)`: the combo bonus windows its
/// combo factors read. Without a Gekisou combo no team has any.
type CarrierKey = [(u32, u32); 5];

/// The terms `(N, J)` of one member and class of one bound, at each position.
type SlotTermTable = [(f64, f64); 5];

/// The order-sum terms of one bound: those of each member and class apart from the combo factors, on first use, and
/// the coefficients of the sets of carriers met last.
pub(super) struct OrderSums {
    slots: Vec<Vec<OnceCell<Box<SlotTerms>>>>,
    coefficients: RefCell<Vec<(CarrierKey, Rc<Coefficients>)>>,
}

/// What the order sum reads of one member and class apart from the combo factors.
struct SlotTerms {
    /// Each position's command counts `[fixed, run, most]` (see `CandCounts`).
    counts: [[(f64, f64); 3]; 5],
    /// The componentwise largest of `counts`.
    widest: [(f64, f64); 3],
    /// Whether one of its parts has judgement factors.
    judge: bool,
    /// The largest factor sum of its parts, by chart time and by score frame.
    peak: Peaks,
    peak_frame: Peaks,
}

/// The largest factor sum of one slot's parts at any of its positions as a step function of time: `steps[i] =
/// (t, v)` holds from time `t` on (0 before the first step), each part's sum over its spans containing the time
/// rounded up. A team's peak reads the slots at their span starts, `starts` ascending; `most` is the largest value
/// at one of them.
struct Peaks {
    steps: Vec<(i64, f64)>,
    starts: Vec<i64>,
    most: f64,
}

/// The coefficients of the order sum under the combo factors of one set of carriers.
struct Coefficients {
    /// At least `Σ_e c_e mjp_e`; None when one of these coefficients is negative or not finite (the sum is then not
    /// bounded).
    base: Option<f64>,
    /// Prefix sums of `c_e mjp_e` (index 0) and of `c_e jp4_e[j]` (index `1 + j`), and an absolute bound of the
    /// error of each (infinite when one of its coefficients is negative or not finite).
    prefix: [Vec<f64>; 5],
    error: [f64; 5],
    /// `c_e mjp_e` as summed, and the factor by which their sums over entries bound the exact ones.
    note: Vec<f64>,
    lift: f64,
    /// The combo count bounds of the carriers (`GkCombo::fill`), which combo ramp windows read.
    sums: Vec<f64>,
    /// Per member and class, each position's terms `(N, J)`, on first use.
    terms: RefCell<FxHashMap<(u32, u32), SlotTermTable>>,
}

/// `k u / (1 - k u)` rounded up, `u` the unit roundoff: bounds the relative error of `k` rounded operations on
/// nonnegative values.
fn gamma(k: usize) -> f64 {
    let ku = ((k as f64) * 2f64.powi(-53)).next_up();
    (ku / (1.0 - ku).next_down()).next_up()
}

impl OrderSums {
    pub(super) fn new(contrib: &[Vec<[Contrib; 5]>]) -> Self {
        OrderSums {
            slots: contrib.iter().map(|classes| classes.iter().map(|_| OnceCell::new()).collect()).collect(),
            coefficients: RefCell::default(),
        }
    }

    /// The fine cap of a complete team (`members` with `classes`, slot order) summed over `orders`, each holding the
    /// positions of the slots: the lesser of two caps. The first holds each slot's widest command counts and largest
    /// factor peak in every order, which bound that order's own; the second each order's own counts at the team's
    /// peaks. With `below` (monotone: it holds for any total at most one it holds for) the cap only needs to decide
    /// it, and the first is the answer when `below` holds for it or fails even without any drift, as a cap only
    /// grows with the drift.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn upper(
        &self,
        view: &FineView<'_>,
        contrib: &[Vec<[Contrib; 5]>],
        power: i64,
        members: [usize; 5],
        classes: [usize; 5],
        orders: &[[usize; 5]],
        scratch: &mut Scratch,
        below: Option<&dyn Fn(i64) -> bool>,
    ) -> i64 {
        let chain = view.chain();
        if !chain.is_finite() {
            return i64::MAX;
        }
        let slots: [&SlotTerms; 5] = std::array::from_fn(|s| {
            let (m, c) = (members[s], classes[s]);
            &**self.slots[m][c].get_or_init(|| Box::new(SlotTerms::new(view, &contrib[m][c])))
        });
        let coefficients = self.coefficients(view, contrib, members, classes, scratch);
        let Some(base) = coefficients.base else { return i64::MAX };
        let mut weight = [[0f64; 5]; 5];
        for positions in orders {
            for s in 0..5 {
                weight[s][positions[s]] += 1.0;
            }
        }
        let judge = slots.iter().any(|t| t.judge);
        let mut linear = 0f64;
        for s in 0..5 {
            let terms = coefficients.slot_terms(view, &contrib[members[s]][classes[s]], (members[s], classes[s]));
            for (&n, &(note, judgement)) in weight[s].iter().zip(&terms) {
                if n > 0.0 {
                    let term = if judge { (note + judgement).next_up() } else { note };
                    linear = (linear + (n * term).next_up()).next_up();
                }
            }
        }
        let n = orders.len() as f64;
        let p = power.max(0) as f64;
        let total = |drift: f64| -> i64 {
            if !drift.is_finite() {
                return i64::MAX;
            }
            let inner = (((n + drift).next_up() * base).next_up() + linear).next_up();
            let ranked = ((p * inner).next_up() * (1.0 + chain).next_up()).next_up();
            let v = (ranked * (1.0 + 1e-12)).ceil();
            if v >= i64::MAX as f64 || v.is_nan() { i64::MAX } else { v as i64 }
        };
        // Any order's counts are at most the sum of the slots' widest counts, and its peaks at most the sum of the
        // slots' largest peaks: a drift certificate from larger counts and peaks still bounds its drift.
        let mut widest = [(0f64, 0f64); 3];
        let (mut peak, mut peak_frame) = (0f64, 0f64);
        for t in &slots {
            add_counts(&mut widest, &t.widest);
            peak = (peak + t.peak.most).next_up();
            peak_frame = (peak_frame + t.peak_frame.most).next_up();
        }
        let [fixed, run, most] = widest;
        let each = view.counts_drift(&CandCounts { fixed, run, most, peak, peak_frame });
        let quick = total((n * each).next_up());
        if below.is_some_and(|below| below(quick) || !below(total(0.0))) {
            return quick;
        }
        let (peak, peak_frame) = (team_peak(slots.map(|t| &t.peak)), team_peak(slots.map(|t| &t.peak_frame)));
        let mut drift = 0f64;
        for positions in orders {
            let mut counts = [(0f64, 0f64); 3];
            for s in 0..5 {
                add_counts(&mut counts, &slots[s].counts[positions[s]]);
            }
            let [fixed, run, most] = counts;
            drift = (drift + view.counts_drift(&CandCounts { fixed, run, most, peak, peak_frame })).next_up();
        }
        total(drift).min(quick)
    }

    /// The coefficients under the combo factors of the team's carriers, kept for the sets of carriers met last.
    fn coefficients(
        &self,
        view: &FineView<'_>,
        contrib: &[Vec<[Contrib; 5]>],
        members: [usize; 5],
        classes: [usize; 5],
        scratch: &mut Scratch,
    ) -> Rc<Coefficients> {
        let mut key = [(u32::MAX, u32::MAX); 5];
        if view.fine.gcombo.is_some() {
            let carriers = (0..5).filter(|&s| !contrib[members[s]][classes[s]][0].cb.is_empty());
            for (at, s) in carriers.enumerate() {
                key[at] = (members[s] as u32, classes[s] as u32);
            }
        }
        let mut kept = self.coefficients.borrow_mut();
        if let Some((_, coefficients)) = kept.iter().rev().find(|(k, _)| *k == key) {
            return coefficients.clone();
        }
        view.fill_combo_factors(std::array::from_fn(|s| &contrib[members[s]][classes[s]][0]), scratch);
        let coefficients = Rc::new(match &view.fine.gcombo {
            Some(_) => Coefficients::new(view, Some(&scratch.gk_g), scratch.sums.clone()),
            None => Coefficients::new(view, None, Vec::new()),
        });
        if kept.len() == CARRIER_SETS {
            kept.remove(0);
        }
        kept.push((key, coefficients.clone()));
        coefficients
    }
}

/// Adds command counts, each sum rounded up.
fn add_counts(sum: &mut [(f64, f64); 3], x: &[(f64, f64); 3]) {
    for (s, x) in sum.iter_mut().zip(x) {
        *s = ((s.0 + x.0).next_up(), (s.1 + x.1).next_up());
    }
}

impl SlotTerms {
    fn new(view: &FineView<'_>, parts: &[Contrib; 5]) -> Self {
        let counts: [[(f64, f64); 3]; 5] = std::array::from_fn(|k| {
            let mut counts = [(0f64, 0f64); 3];
            view.add_part_counts(&parts[k], None, &mut counts);
            counts
        });
        let mut widest = [(0f64, 0f64); 3];
        for position in &counts {
            for (w, c) in widest.iter_mut().zip(position) {
                *w = (w.0.max(c.0), w.1.max(c.1));
            }
        }
        SlotTerms {
            counts,
            widest,
            judge: parts.iter().any(|p| p.judge),
            peak: Peaks::new(parts, None),
            peak_frame: Peaks::new(parts, Some(view.fine.score_frames)),
        }
    }
}

impl Peaks {
    /// The step function of the spans of `parts`: in chart time (`frames` None), or in score frames, each span
    /// covering the frames its closed ends meet.
    fn new(parts: &[Contrib; 5], frames: Option<ScoreFrames>) -> Self {
        let mut events = Vec::new();
        let mut starts = Vec::new();
        for (k, part) in parts.iter().enumerate() {
            for &(a, b, g) in &part.spans {
                starts.push(frames.map_or(a, |frames| i64::from(frames.at(a))));
                // the half-open span of chart times, or of score frames, it adds `g` at
                let (lo, hi) = match frames {
                    Some(frames) => match frames.closed(a, b) {
                        Some((lo, hi)) => (lo, hi + 1),
                        None => continue,
                    },
                    None => (a, b),
                };
                if lo < hi {
                    events.push((lo, k, g));
                    events.push((hi, k, -g));
                }
            }
        }
        events.sort_unstable_by_key(|e| e.0);
        starts.sort_unstable();
        starts.dedup();
        let (mut sums, mut steps, mut i) = ([0f64; 5], Vec::new(), 0);
        while i < events.len() {
            let t = events[i].0;
            while i < events.len() && events[i].0 == t {
                let (_, k, g) = events[i];
                sums[k] = (sums[k] + g).next_up();
                i += 1;
            }
            steps.push((t, sums.iter().copied().fold(f64::MIN, f64::max)));
        }
        let (mut most, mut at, mut value) = (0f64, 0, 0f64);
        for &t in &starts {
            while at < steps.len() && steps[at].0 <= t {
                value = steps[at].1;
                at += 1;
            }
            most = most.max(value);
        }
        Peaks { steps, starts, most }
    }
}

/// The largest sum over the slots of their factor sums at a span start of any of them, each sum rounded up: at least
/// the `peak` (or `peak_frame`) of [`CandCounts`] for the parts of every order, each slot at one of its positions.
fn team_peak(slots: [&Peaks; 5]) -> f64 {
    let (mut start, mut step, mut value, mut best) = ([0usize; 5], [0usize; 5], [0f64; 5], 0f64);
    while let Some(t) = (0..5).filter_map(|s| slots[s].starts.get(start[s]).copied()).min() {
        for (s, peaks) in slots.iter().enumerate() {
            while start[s] < peaks.starts.len() && peaks.starts[start[s]] <= t {
                start[s] += 1;
            }
            while step[s] < peaks.steps.len() && peaks.steps[step[s]].0 <= t {
                value[s] = peaks.steps[step[s]].1;
                step[s] += 1;
            }
        }
        best = best.max(value.iter().fold(0f64, |sum, &v| (sum + v).next_up()));
    }
    best
}

impl Coefficients {
    /// The coefficients with the Gekisou combo factors `gk_g` (None without a Gekisou combo) and the combo count
    /// bounds `sums` of one set of carriers. Each `c_e mjp_e` and `c_e jp4_e[j]` takes at most eight rounded
    /// operations from the inputs and each prefix sum at most `ne` more, so with `γ = gamma(ne + 8)` a prefix sum is
    /// within `γ` of the exact one relatively, and every one within `γ` of the exact total over all entries, which
    /// is at most the computed total over `1 - γ`.
    fn new(view: &FineView<'_>, gk_g: Option<&[f64]>, sums: Vec<f64>) -> Self {
        let (c, f) = (view.coef, view.fine);
        let ne = c.times.len();
        let mut prefix: [Vec<f64>; 5] = std::array::from_fn(|_| {
            let mut v = Vec::with_capacity(ne + 1);
            v.push(0.0);
            v
        });
        let mut note = Vec::with_capacity(ne);
        let mut valid = [true; 5];
        let (mut from, mut broke, mut group) = (0usize, None::<usize>, usize::MAX);
        for e in 0..ne {
            let gs = f.group[e] as usize;
            if gs != group {
                if let Some(b) = broke {
                    from = b;
                }
                group = gs;
            }
            let m = 1usize << f.raw[e];
            if f.breaks[m] {
                broke = Some(gs);
            }
            let since = if f.nobreak.get(e).copied().unwrap_or(false) { gs } else { gs - from };
            let combo = f.combo_max.get(since).copied().unwrap_or(f64::INFINITY);
            let g = gk_g.map_or(1.0, |g| g[e]);
            let z = if c.z[e] == 1.0 { 1.0 } else { c.z[e] * (1.0 + 2f64.powi(-20)) };
            let rank = if f.rank.is_empty() { 1.0 } else { f.rank[e] };
            let coefficient = f.pre[e] * g * combo / f.cnc * z * rank;
            let factors = [f.pre[e], g, combo, f.cnc, z, rank].iter().all(|v| *v >= 0.0);
            for (i, sum) in prefix.iter_mut().enumerate() {
                let x = coefficient * if i == 0 { f.mjp[m] } else { f.jp4[m][i - 1] };
                valid[i] &= factors && x.is_finite() && x >= 0.0;
                if i == 0 {
                    note.push(x);
                }
                sum.push(sum[e] + x);
            }
        }
        let gamma = gamma(ne + 8);
        let total = |sum: &Vec<f64>| (sum[ne] / (1.0 - gamma).next_down()).next_up();
        let error: [f64; 5] = std::array::from_fn(|i| {
            let error = (2.0 * (gamma * total(&prefix[i])).next_up()).next_up();
            if valid[i] && error.is_finite() { error } else { f64::INFINITY }
        });
        let base = error[0].is_finite().then(|| total(&prefix[0]));
        let lift = (1.0 / (1.0 - gamma).next_down()).next_up();
        Coefficients { base, prefix, error, note, lift, sums, terms: RefCell::default() }
    }

    /// At least the exact sum of the coefficients `i` over the entries `lo..hi` (infinite when they are not bounded).
    fn window(&self, i: usize, lo: usize, hi: usize) -> f64 {
        if !self.error[i].is_finite() {
            return f64::INFINITY;
        }
        ((self.prefix[i][hi] - self.prefix[i][lo]).next_up() + self.error[i]).next_up().max(0.0)
    }

    /// The terms `(N, J)` of member and class `key` with its parts `parts` at each position.
    fn slot_terms(&self, view: &FineView<'_>, parts: &[Contrib; 5], key: (usize, usize)) -> SlotTermTable {
        let key = (key.0 as u32, key.1 as u32);
        if let Some(terms) = self.terms.borrow().get(&key) {
            return *terms;
        }
        let terms = std::array::from_fn(|k| self.part_terms(view, &parts[k]));
        self.terms.borrow_mut().insert(key, terms);
        terms
    }

    /// The terms `(N, J)` of one part (its windows of a team without a conversion).
    fn part_terms(&self, view: &FineView<'_>, part: &Contrib) -> (f64, f64) {
        let reads = view.fine.gcombo.as_ref().map(|gc| RampReads { gc, sums: &self.sums, times: &view.coef.times });
        let (mut note, mut ramp, mut judge) = (0f64, 0f64, 0f64);
        for w in part.unconverted.as_ref().unwrap_or(&part.windows) {
            let (lo, hi) = (w.lo as usize, w.hi as usize);
            if let Some(reads) = reads.filter(|_| w.ramp != 0) {
                let r = &part.ramps[w.ramp as usize - 1];
                for e in lo..hi {
                    let factor = reads.factor(w, r, e).max(0.0);
                    if factor > 0.0 {
                        ramp = (ramp + (factor * self.note[e]).next_up()).next_up();
                    }
                }
                continue;
            }
            if w.note > 0.0 {
                note = (note + (w.note * self.window(0, lo, hi)).next_up()).next_up();
            }
            for j in 0..4 {
                if w.judge[j] > 0.0 {
                    judge = (judge + (w.judge[j] * self.window(1 + j, lo, hi)).next_up()).next_up();
                }
            }
        }
        // a ramp window's entries weigh their coefficients as summed, each within the prefix sums' relative error
        ((note + (ramp * self.lift).next_up()).next_up(), judge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator of test inputs.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn unit(&mut self) -> f64 {
            self.below(1 << 20) as f64 / (1u64 << 20) as f64
        }
    }

    fn params() -> LiveParams {
        LiveParams {
            skill_target_music_type: 0,
            total_power: 0,
            music_level: 0,
            converted_note_count: 0,
            music_length_ms: 60_000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        }
    }

    /// Entries at increasing chart times (some sharing one), with breaking and non-breaking judgements, life and rank
    /// factors, judgement percents as the envelope compiles them, and optionally certified probe starts.
    fn envelope(rng: &mut Lcg, ne: usize, members: usize, probe: bool) -> (Coef, Fine) {
        let mut times = Vec::with_capacity(ne);
        let mut t = 1_000i32;
        for _ in 0..ne {
            t += [0, 120, 250, 500][rng.below(4) as usize];
            times.push(t);
        }
        let z: Vec<f64> = (0..ne).map(|_| if rng.below(4) == 0 { 0.5 + 0.5 * rng.unit() } else { 1.0 }).collect();
        let raw: Vec<u8> =
            (0..ne).map(|_| if rng.below(10) == 0 { 1 + rng.below(2) as u8 } else { 3 + rng.below(4) as u8 }).collect();
        let group = (0..ne).map(|e| times.partition_point(|&x| x < times[e]) as u32).collect();
        let jp_of = [0.0, 0.0, 0.1, 0.5, 0.8, 1.0, 1.1];
        let mut mjp = vec![0f64; 128];
        let mut jp4 = vec![[0f64; 4]; 128];
        let mut breaks = vec![false; 128];
        for mask in 0..128usize {
            for (x, &p) in jp_of.iter().enumerate().skip(1) {
                if (mask >> x) & 1 == 1 {
                    mjp[mask] = mjp[mask].max(p);
                    if x >= 3 {
                        jp4[mask][x - 3] = p;
                    }
                }
            }
            breaks[mask] = (mask & 0b111_1000) == 0;
        }
        let coef = Coef { times, z, ..Default::default() };
        let fine = Fine {
            score_frames: ScoreFrames::new(&params()),
            rush_eligible: false,
            expected: true,
            probe_starts: probe.then_some(ProbeStarts { most: 3.0, expected: 1.5, luck: 1.2 }),
            raw,
            group,
            pre: (0..ne).map(|_| 0.5 + rng.unit()).collect(),
            pre_plain: Vec::new(),
            cnc: 3.0,
            combo_max: (0..=ne).map(|c| 1.0 + (c / 10) as f64 * 0.05).collect(),
            mjp,
            jp4,
            breaks,
            src: vec![vec![0; 2]; members],
            extra: vec![Default::default()],
            extra_v: vec![Default::default()],
            budget: vec![Vec::new()],
            gcombo: None,
            dead: vec![false; ne],
            rank: (0..ne).map(|_| 1.0 + 0.3 * rng.unit()).collect(),
            rank_ranges: Vec::new(),
            network_ranking: false,
            nobreak: Vec::new(),
            z_dead: 0.0,
            life: vec![vec![LifeKind::None; 2]; members],
            base: 0,
            #[cfg(feature = "search-diagnostics")]
            exec_profile: Vec::new(),
            slot_end: Vec::new(),
            slot_dmg: Vec::new(),
            slot_dmg_final: Vec::new(),
            life_rows_listed: false,
            ev_slot: Default::default(),
            until: Vec::new(),
            until_min: Vec::new(),
            dead_from: 0,
        };
        (coef, fine)
    }

    /// Two classes of each member, each position's part with windows (some with negative factors), factor spans and
    /// command counts.
    fn parts(rng: &mut Lcg, ne: usize, members: usize) -> Vec<Vec<[Contrib; 5]>> {
        let part = |rng: &mut Lcg| {
            let windows = (0..1 + rng.below(6))
                .map(|_| {
                    let lo = rng.below(ne as u64) as u32;
                    let hi = (lo + 1 + rng.below(12) as u32).min(ne as u32);
                    let judge = std::array::from_fn(|_| if rng.below(3) == 0 { 0.3 * rng.unit() } else { 0.0 });
                    Window { lo, hi, note: rng.unit() - 0.1, judge, ramp: 0, rush: 0 }
                })
                .collect::<Vec<_>>();
            let spans = (0..rng.below(4))
                .map(|_| {
                    let a = 1_000 + rng.below(20_000) as i64;
                    (a, a + rng.below(3_000) as i64, rng.unit())
                })
                .collect();
            Contrib {
                judge: windows.iter().any(|w| w.judge.iter().any(|&j| j != 0.0)),
                windows,
                spans,
                ops_plain: rng.below(200) as f64,
                cmds_plain: rng.below(20) as f64,
                ..Default::default()
            }
        };
        (0..members).map(|_| (0..2).map(|_| std::array::from_fn(|_| part(rng))).collect()).collect()
    }

    /// The largest sum over the slots of the largest factor sum of one slot's part at any of its positions, over the
    /// span starts of all of them, each sum rounded up, from all the spans at once.
    fn peak_of_all(slots: [&[Contrib; 5]; 5], frames: Option<ScoreFrames>) -> f64 {
        let mut events = Vec::new();
        let mut starts = Vec::new();
        for (s, parts) in slots.iter().enumerate() {
            for (k, part) in parts.iter().enumerate() {
                for &(a, b, g) in &part.spans {
                    starts.push(frames.map_or(a, |frames| i64::from(frames.at(a))));
                    let (lo, hi) = match frames {
                        Some(frames) => match frames.closed(a, b) {
                            Some((lo, hi)) => (lo, hi + 1),
                            None => continue,
                        },
                        None => (a, b),
                    };
                    if lo < hi {
                        events.push((lo, 5 * s + k, g));
                        events.push((hi, 5 * s + k, -g));
                    }
                }
            }
        }
        events.sort_unstable_by_key(|e| e.0);
        starts.sort_unstable();
        starts.dedup();
        let (mut sums, mut i, mut best) = ([0f64; 25], 0, 0f64);
        for at in starts {
            while i < events.len() && events[i].0 <= at {
                let (_, part, g) = events[i];
                sums[part] = (sums[part] + g).next_up();
                i += 1;
            }
            let total =
                sums.chunks(5).fold(0f64, |t, slot| (t + slot.iter().copied().fold(f64::MIN, f64::max)).next_up());
            best = best.max(total);
        }
        best
    }

    /// The sum over the orders of the per-entry expected bound, entry by entry.
    fn per_entry(view: &FineView<'_>, power: i64, slots: [&[Contrib; 5]; 5], orders: &[[usize; 5]]) -> i64 {
        let (c, f) = (view.coef, view.fine);
        let (peak, peak_frame) = (peak_of_all(slots, None), peak_of_all(slots, Some(f.score_frames)));
        let mut drift = 0f64;
        let mut weight = [[0f64; 5]; 5];
        for positions in orders {
            let mut counts = [(0f64, 0f64); 3];
            for s in 0..5 {
                view.add_part_counts(&slots[s][positions[s]], None, &mut counts);
                weight[s][positions[s]] += 1.0;
            }
            let [fixed, run, most] = counts;
            drift = (drift + view.counts_drift(&CandCounts { fixed, run, most, peak, peak_frame })).next_up();
        }
        let chain = view.chain();
        let ne = c.times.len();
        let judge = slots.iter().flat_map(|parts| parts.iter()).any(|p| p.judge);
        let mut note = vec![0f64; ne + 1];
        let mut judgements: [Vec<f64>; 4] = std::array::from_fn(|_| vec![0f64; ne + 1]);
        for (parts, weight) in slots.iter().zip(&weight) {
            for (part, &n) in parts.iter().zip(weight).filter(|(_, n)| **n > 0.0) {
                for w in &part.windows {
                    let (lo, hi) = (w.lo as usize, w.hi as usize);
                    note[lo] += n * w.note.max(0.0);
                    note[hi] -= n * w.note.max(0.0);
                    if judge {
                        for j in 0..4 {
                            judgements[j][lo] += n * w.judge[j].max(0.0);
                            judgements[j][hi] -= n * w.judge[j].max(0.0);
                        }
                    }
                }
            }
        }
        let p = power as f64;
        let n = orders.len() as f64;
        let (mut acc, mut accj, mut ranked) = (0f64, [0f64; 4], 0f64);
        let (mut from, mut broke, mut group) = (0usize, None::<usize>, usize::MAX);
        for e in 0..ne {
            let gs = f.group[e] as usize;
            if gs != group {
                if let Some(b) = broke {
                    from = b;
                }
                group = gs;
            }
            let m = 1usize << f.raw[e];
            if f.breaks[m] {
                broke = Some(gs);
            }
            let k = f.pre[e] * f.combo_max[gs - from] / f.cnc;
            acc += note[e];
            let mut v = f.mjp[m] * (n + acc.max(0.0) + drift);
            if judge {
                for j in 0..4 {
                    accj[j] += judgements[j][e];
                    v += f.jp4[m][j] * accj[j].max(0.0);
                }
            }
            let x = p * k * v * (1.0 + chain);
            let z = if c.z[e] == 1.0 { x } else { c.z[e] * x * (1.0 + 2f64.powi(-20)) };
            ranked += z * f.rank[e];
        }
        (ranked * (1.0 + 1e-12)).ceil() as i64
    }

    #[test]
    fn order_sums_cover_the_per_entry_sum_over_the_orders() {
        let mut rng = Lcg(7);
        let all: Vec<[usize; 5]> =
            crate::search::uniform::all_orders().iter().map(crate::search::uniform::positions_of).collect();
        for probe in [false, true] {
            for ne in [1, 7, 60] {
                let (coef, fine) = envelope(&mut rng, ne, 8, probe);
                let contrib = parts(&mut rng, ne, 8);
                let view = FineView { coef: &coef, fine: &fine, chain_extra: 0.0 };
                let sums = OrderSums::new(&contrib);
                let mut scratch = Scratch::default();
                for team in 0..12 {
                    let mut pool: Vec<usize> = (0..8).collect();
                    let members: [usize; 5] =
                        std::array::from_fn(|_| pool.remove(rng.below(pool.len() as u64) as usize));
                    let classes = std::array::from_fn(|_| rng.below(2) as usize);
                    let slots = std::array::from_fn(|s| &contrib[members[s]][classes[s]]);
                    let orders = if team % 3 == 0 { &all[..] } else { &all[..1 + rng.below(119) as usize] };
                    let power = 100_000 + rng.below(900_000) as i64;
                    let reference = per_entry(&view, power, slots, orders);
                    let mut upper = |below: Option<&dyn Fn(i64) -> bool>| {
                        sums.upper(&view, &contrib, power, members, classes, orders, &mut scratch, below)
                    };
                    let (exact, quick) = (upper(None), upper(Some(&|_: i64| true)));
                    let name = format!("probe {probe}, {ne} entries, team {team}: {reference} {exact} {quick}");
                    // The same sum, rounded outward instead of to nearest.
                    assert!(exact >= reference - 1 && exact - reference <= reference / 1_000_000_000 + 1, "{name}");
                    if !probe {
                        // Without probe starts the drift grows with the counts and peaks.
                        assert!(quick >= exact, "{name}");
                    }
                }
            }
        }
    }

    #[test]
    fn team_peak_reads_every_slot_at_every_span_start() {
        let mut rng = Lcg(11);
        let frames = ScoreFrames::new(&params());
        for ne in [5, 40] {
            let contrib = parts(&mut rng, ne, 6);
            for _ in 0..20 {
                let slots: [&[Contrib; 5]; 5] =
                    std::array::from_fn(|_| &contrib[rng.below(6) as usize][rng.below(2) as usize]);
                for frames in [None, Some(frames)] {
                    let peaks: Vec<Peaks> = slots.iter().map(|parts| Peaks::new(parts, frames)).collect();
                    let peak = team_peak(std::array::from_fn(|s| &peaks[s]));
                    let all = peak_of_all(slots, frames);
                    assert!(peak >= all * (1.0 - 1e-12) && peak <= all * (1.0 + 1e-12), "{peak} {all}");
                    let most = peaks.iter().fold(0f64, |sum, p| (sum + p.most).next_up());
                    assert!(most >= peak, "{most} {peak}");
                }
            }
        }
    }
}
