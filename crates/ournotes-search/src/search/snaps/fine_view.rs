//! Per-note candidate caps and the joint fine-bound view used by physical search.
use super::*;

/// Command counts of one candidate, each `(executions, lifetime commands)`.
struct CandCounts {
    /// Whatever the starts of the direct-probe class; without certified laws, all of them.
    fixed: (f64, f64),
    /// For each start of the direct-probe class.
    run: (f64, f64),
    /// The most of one play.
    most: (f64, f64),
    /// The largest factor sum at one command time, and intersecting one native score frame.
    peak: f64,
    peak_frame: f64,
}

/// Immutable per-note envelope shared by the legacy compiler and physical native-root search.
pub(super) struct FineView<'a> {
    pub(super) coef: &'a Coef,
    pub(super) fine: &'a Fine,
    pub(super) chain_extra: f64,
}
impl FineView<'_> {
    /// The drift of one candidate: a bound on the absolute binary32 error of the score-up fields
    /// (`note_score_up` and the judgement factors, summed) from this candidate's own commands. Each execution of
    /// a score frame holding one of its commands rounds the state once when applying it (including transient
    /// same-timestamp start/end pairs), once in the frame difference (below the factors of
    /// the windows that meet one native score frame) and once in the undo; `ops` counts the executions; each factor's
    /// binary32 representation adds two more roundings per lifetime command
    /// (integer-to-float conversion and division, including saturated mill values). Infinite without a
    /// certificate.
    ///
    /// With certified lottery laws the bound is an expectation over the lottery, and the drift is the expected one
    /// (see `ProbeStarts::expected_drift`): the drift of a play is convex in its starts of the direct-probe class.
    pub(super) fn cand_drift(&self, parts: [&Contrib; 5], rush_masks: Option<&RushMasks>) -> f64 {
        self.counts_drift(&self.cand_counts(parts, rush_masks))
    }

    /// The drift of a candidate with these counts.
    fn counts_drift(&self, counts: &CandCounts) -> f64 {
        let drift = |(e, n): (f64, f64)| self.drift_of(e, n, counts.peak, counts.peak_frame);
        let most = drift(counts.most);
        match &self.fine.probe_starts {
            Some(starts) => {
                let linear =
                    (starts.at_most(counts.fixed.0, counts.run.0), starts.at_most(counts.fixed.1, counts.run.1));
                starts.expected_drift(drift(counts.fixed), drift(linear)).min(most)
            }
            None => most,
        }
    }

    /// The drift of `e` command executions and `n` lifetime commands.
    fn drift_of(&self, e: f64, n: f64, peak: f64, peak_frame: f64) -> f64 {
        let representation = (2.0 * n).next_up();
        let weight = ((3.0 * e).next_up() + representation).next_up();
        let Some(amplification) = float_margin::amplification(weight, 2f64.powi(-24)) else {
            return f64::INFINITY;
        };
        // A start/end pair at the same timestamp can transiently change a field
        // even when its half-open factor window contains no note. Closed native
        // frame spans also cover those intermediate command states.
        let state = (1.0 + peak.max(peak_frame)).next_up();
        let magnitude = ((e * ((2.0 * state).next_up() + peak_frame).next_up()).next_up()
            + (representation * state).next_up())
        .next_up();
        let drift = ((magnitude * 2f64.powi(-24)).next_up() * amplification).next_up();
        if drift.is_finite() { float_margin::snapshot_allowance(drift, self.chain_extra > 0.0) } else { f64::INFINITY }
    }

    /// The relative allowance of the native chain after the score-up factor.
    pub(super) fn chain(&self) -> f64 {
        float_margin::with_chain(0.0, self.chain_extra).unwrap_or(f64::INFINITY)
    }

    /// The counts `cand_drift` reads.
    fn cand_counts(&self, parts: [&Contrib; 5], rush_masks: Option<&RushMasks>) -> CandCounts {
        let mut peak = 0f64;
        let mut peak_frame = 0f64;
        let mut counts = [(0f64, 0f64); 3];
        for p in parts {
            self.add_part_counts(p, rush_masks, &mut counts);
            for &(a, _, _) in &p.spans {
                let frame = self.fine.score_frames.at(a);
                let (mut at, mut near) = (0f64, 0f64);
                for q in parts {
                    for &(b0, b1, g) in &q.spans {
                        if b0 <= a && a < b1 {
                            at = (at + g).next_up();
                        }
                        if self.fine.score_frames.meets(b0, b1, frame) {
                            near = (near + g).next_up();
                        }
                    }
                }
                peak = peak.max(at);
                peak_frame = peak_frame.max(near);
            }
        }
        let [fixed, run, most] = counts;
        CandCounts { fixed, run, most, peak, peak_frame }
    }

    /// Adds the command counts of one part to the `[fixed, run, most]` counts of [`CandCounts`].
    fn add_part_counts(&self, p: &Contrib, rush_masks: Option<&RushMasks>, counts: &mut [(f64, f64); 3]) {
        let add = |sum: &mut (f64, f64), x: (f64, f64)| *sum = ((sum.0 + x.0).next_up(), (sum.1 + x.1).next_up());
        let [fixed, run, most] = counts;
        let rush_masks = rush_masks.filter(|_| self.fine.rush_eligible);
        add(fixed, (p.ops_plain, p.cmds_plain));
        add(most, (p.ops_plain, p.cmds_plain));
        for row in &p.rush {
            let counted = row.counts(rush_masks);
            match &self.fine.probe_starts {
                Some(starts) => {
                    let (row_fixed, row_run) = row.by_start(rush_masks);
                    add(fixed, row_fixed);
                    add(run, row_run);
                    add(
                        most,
                        (
                            starts.at_most(row_fixed.0, row_run.0).min(counted.0),
                            starts.at_most(row_fixed.1, row_run.1).min(counted.1),
                        ),
                    );
                }
                None => {
                    add(fixed, counted);
                    add(most, counted);
                }
            }
        }
    }

    /// The Gekisou combo factors of a candidate whose parts hold these combo bonus windows, into `scratch.gk_g`
    /// (and their combo sums into `scratch.sums`); nothing without a Gekisou combo.
    fn fill_combo_factors(&self, parts: [&Contrib; 5], scratch: &mut Scratch) {
        let (c, f) = (&self.coef, &self.fine);
        let Some(gc) = &f.gcombo else { return };
        let mut ev = std::mem::take(&mut scratch.ev);
        let plain: Vec<(i64, i64, f64)> =
            parts.iter().flat_map(|q| q.cb.iter()).filter(|w| w.3.is_none()).map(|w| (w.0, w.1, w.2)).collect();
        let gated: Vec<ComboBonusRow> =
            parts.iter().flat_map(|q| q.cb.iter()).filter(|w| w.3.is_some()).copied().collect();
        bonus_at(&plain, &mut ev);
        let (mut ri0, mut i, mut cur) = (usize::MAX, 0usize, 0f64);
        gc.fill(
            &c.times,
            |ri, q, acc, group| {
                if ri != ri0 {
                    (ri0, i, cur) = (ri, 0, 0.0);
                }
                let t = c.times[gc.entries[ri][q] as usize] as i64;
                while i < ev.len() && ev[i].0 <= t {
                    cur += ev[i].1;
                    i += 1;
                }
                if gated.is_empty() {
                    return cur;
                }
                let running = || gated.iter().filter(|w| w.0 <= t && t <= w.1);
                // the step reads every other window open; a gate opened by it only adds bonus
                let all = cur + running().map(|w| w.2).sum::<f64>();
                cur + running()
                    .filter(|w| gate_open(w.3, ri, acc, group, gate_step(all, w.2)))
                    .map(|w| w.2)
                    .sum::<f64>()
            },
            &mut scratch.gk_g,
            &mut scratch.sums,
        );
        scratch.ev = ev;
    }

    /// Per-note bound of one candidate (conversion source `src[k]` at position `k`): the sum over the stream of each
    /// note's floored bound, with the judgements and combo breaks of the candidate's own conversions, and the
    /// life-zero factor where `life` shows that the life is 0.
    pub(super) fn fine_bound(
        &self,
        power: i64,
        parts: [&Contrib; 5],
        src: [u32; 5],
        life: CandLife,
        scratch: &mut Scratch,
        rush_masks: Option<&RushMasks>,
    ) -> i64 {
        let margin = (self.cand_drift(parts, rush_masks), self.chain());
        self.fine_bound_with_eps(power, parts, src, life, scratch, margin, rush_masks)
    }

    /// The production caller supplies the certified candidate margin `(drift, chain)`: the absolute drift of the
    /// score-up factor and the relative allowance of the rest of the chain. A note's binary32 score-up factor is at
    /// most its exact value plus the drift, so its score is at most `p * k * jp * (S + drift) * (1 + chain)` with
    /// `S` the envelope's score-up factor. Diagnostics may set both to zero to measure envelope slack; that result
    /// is not a bound.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fine_bound_with_eps(
        &self,
        power: i64,
        parts: [&Contrib; 5],
        src: [u32; 5],
        life: CandLife,
        scratch: &mut Scratch,
        (drift, chain): (f64, f64),
        rush_masks: Option<&RushMasks>,
    ) -> i64 {
        if !drift.is_finite() || !chain.is_finite() {
            return i64::MAX;
        }
        let ne = self.coef.times.len();
        let judge = parts.iter().any(|p| p.judge);
        scratch.note.clear();
        scratch.note.resize(ne + 1, 0.0);
        if judge {
            for v in scratch.judge.iter_mut() {
                v.clear();
                v.resize(ne + 1, 0.0);
            }
        }
        scratch.ramp_windows.clear();
        let rush_masks = rush_masks.filter(|_| self.fine.rush_eligible);
        let mut rounding = rush_masks.map(|_| float_margin::WindowRoundoff::default());
        let mut refined = false;
        for (k, p) in parts.iter().enumerate() {
            scratch.rush_replacements.clear();
            for row in &p.rush {
                scratch.rush_replacements.push(rush_masks.and_then(|masks| {
                    rush::cached_windows(&mut scratch.rush_cache, &row.spec, masks, &self.coef.times)
                }));
            }
            for w in &p.windows {
                if w.rush != 0 && scratch.rush_replacements[w.rush as usize - 1].is_some() {
                    refined = true;
                    continue;
                }
                if w.ramp != 0 && self.fine.gcombo.is_some() {
                    scratch.ramp_windows.push((k, *w));
                    continue;
                }
                if let Some(bound) = &mut rounding {
                    bound.add(w.note, w.judge, 1.0);
                }
                scratch.note[w.lo as usize] += w.note;
                scratch.note[w.hi as usize] -= w.note;
                if judge {
                    for j in 0..4 {
                        scratch.judge[j][w.lo as usize] += w.judge[j];
                        scratch.judge[j][w.hi as usize] -= w.judge[j];
                    }
                }
            }
            for (row, replacement) in p.rush.iter().zip(&scratch.rush_replacements) {
                if let Some(windows) = replacement {
                    for &(lo, hi, mult) in windows.iter() {
                        if let Some(bound) = &mut rounding {
                            bound.add(row.note, row.judge, mult);
                        }
                        scratch.note[lo as usize] += row.note * mult;
                        scratch.note[hi as usize] -= row.note * mult;
                        if judge {
                            for j in 0..4 {
                                scratch.judge[j][lo as usize] += row.judge[j] * mult;
                                scratch.judge[j][hi as usize] -= row.judge[j] * mult;
                            }
                        }
                    }
                }
            }
        }
        // The envelope-computation error of the refined windows is absolute in score-up units, as the drift.
        let drift = if refined {
            let Some(error) = rounding.as_ref().and_then(|r| r.absolute_error()) else { return i64::MAX };
            let drift = (drift + error).next_up();
            if !drift.is_finite() {
                return i64::MAX;
            }
            drift
        } else {
            drift
        };
        let p = power.max(0) as f64;
        let mut acc = 0f64;
        let mut accj = [0f64; 4];
        let mut total = 0i64;
        let mut ranked = 0f64;
        let c = &self.coef;
        let f = &self.fine;
        let extra: [Option<&[u8]>; 5] =
            std::array::from_fn(|k| (src[k] != 0).then(|| &f.extra[src[k] as usize][k][..]));
        let extra_v: [Option<&[u8]>; 5] =
            std::array::from_fn(|k| (src[k] != 0).then(|| &f.extra_v[src[k] as usize][k][..]));
        // the candidate's own Gekisou combo factors, from its combo bonus windows
        self.fill_combo_factors(parts, scratch);
        let gk_g = std::mem::take(&mut scratch.gk_g);
        // Combo ramps: each entry reads the factor at the count of the frame it reads (the flat factor where unknown).
        scratch.ramp.clear();
        scratch.ramp.resize(ne, 0.0);
        if let Some(gc) = &f.gcombo {
            let reads = RampReads { gc, sums: &scratch.sums, times: &c.times };
            for &(k, w) in &scratch.ramp_windows {
                let r = &parts[k].ramps[w.ramp as usize - 1];
                for e in w.lo as usize..w.hi as usize {
                    scratch.ramp[e] += reads.factor(&w, r, e);
                }
            }
        }
        // the candidate's rows with a conversion budget
        scratch.brow.clear();
        for &sid in src.iter().filter(|&&x| x != 0) {
            for r in 0..f.budget[sid as usize].len() {
                scratch.brow.push((sid, r, 0));
            }
        }
        if let Some(terms) = scratch.terms.as_mut() {
            terms.rows = scratch.brow.iter().map(|&(sid, r, _)| (f.budget[sid as usize][r].1, Vec::new())).collect();
        }
        while scratch.bterms.len() < scratch.brow.len() {
            scratch.bterms.push(Vec::new());
        }
        for t in scratch.bterms.iter_mut() {
            t.clear();
        }
        // the combo is counted from the first entry at the time of the last entry that breaks it
        let (mut from, mut broke, mut group) = (0usize, None::<usize>, usize::MAX);
        for e in 0..ne {
            let gs = f.group[e] as usize;
            if gs != group {
                if let Some(b) = broke {
                    from = b;
                }
                group = gs;
            }
            let (mut mask, mut vmask) = (1u8 << f.raw[e], 1u8 << f.raw[e]);
            for x in extra.iter().flatten() {
                mask |= x[e];
            }
            for x in extra_v.iter().flatten() {
                vmask |= x[e];
            }
            if f.breaks[mask as usize] {
                broke = Some(gs);
            }
            let since = if f.nobreak.get(e).copied().unwrap_or(false) { gs } else { gs - from };
            let combo = f.combo_max.get(since).copied().unwrap_or(f64::INFINITY);
            // the luck rush factor only where some rush score bonus command of the root can cover the entry
            let pre = match rush_masks {
                Some(m) if !f.pre_plain.is_empty() && !m.rush_possible(c.times[e]) => f.pre_plain[e],
                _ => f.pre[e],
            };
            let k = if f.gcombo.is_some() { pre * gk_g[e] * combo / f.cnc } else { pre * combo / f.cnc };
            acc += scratch.note[e];
            let acc_e = acc + scratch.ramp[e];
            if judge {
                for j in 0..4 {
                    accj[j] += scratch.judge[j][e];
                }
            }
            let dead = !f.network_ranking
                && match life {
                    CandLife::NoRise => f.dead[e],
                    CandLife::ZeroFrom(t0) => t0 <= c.times[e] as i64 && t0 < f.until[e],
                    CandLife::Unknown => false,
                };
            let ze = if dead { f.z_dead } else { c.z[e] };
            let zval = |m: usize| {
                let base = 1.0 + acc_e.max(0.0) + drift;
                let mut v = f.mjp[m] * base;
                if judge {
                    // One note has ONE final judgement. P/Just possibilities are alternatives,
                    // so do not add their mutually exclusive judgement-specific bonuses.
                    for j in 0..4 {
                        v = v.max(f.jp4[m][j] * (base + accj[j].max(0.0)));
                    }
                }
                let x = p * k * v * (1.0 + chain);
                if f.expected {
                    return if ze == 1.0 { x } else { ze * x * (1.0 + 2f64.powi(-20)) };
                }
                let y = x.floor();
                if ze == 1.0 { y } else { (ze * y * (1.0 + 2f64.powi(-20))).floor() }
            };
            let z = zval(vmask as usize);
            let rk = if f.rank.is_empty() { 1.0 } else { f.rank[e] };
            #[cfg(feature = "search-diagnostics")]
            if let Some(trace) = scratch.trace.as_mut() {
                let g = if f.gcombo.is_some() { gk_g[e] } else { 1.0 };
                trace.push([c.times[e] as f64, z, rk, k, 1.0 + acc_e.max(0.0), ze, accj[3], g]);
            }
            if let Some(terms) = scratch.terms.as_mut() {
                terms.entries.push((c.times[e], z, rk));
            }
            if f.expected && f.rank.is_empty() {
                ranked += z;
            } else if f.rank.is_empty() {
                total = total.saturating_add(z as i64);
            } else {
                // a rank bonus adds its percent of the range score, the sum of these entries' scores
                ranked += z * rk;
            }
            // what converting this entry would add, for each budget row that can see it
            for (bi, (sid, r, next)) in scratch.brow.iter_mut().enumerate() {
                let (to, _, elig) = &f.budget[*sid as usize][*r];
                let to = *to;
                if *next < elig.len() && elig[*next] as usize == e {
                    *next += 1;
                    if vmask & (1 << to) == 0 {
                        let d = (zval((vmask | (1 << to)) as usize) - z) * rk;
                        if d > 0.0 {
                            scratch.bterms[bi].push(d);
                            if let Some(terms) = scratch.terms.as_mut() {
                                terms.rows[bi].1.push((e as u32, d));
                            }
                        }
                    }
                }
            }
        }
        // each budget row converts at most its budget of these entries
        let mut conv = 0f64;
        for (bi, &(sid, r, _)) in scratch.brow.iter().enumerate() {
            conv += top_sum(&mut scratch.bterms[bi], f.budget[sid as usize][r].1);
        }
        scratch.gk_g = gk_g;
        if let Some(terms) = scratch.terms.as_mut() {
            terms.conv = conv;
            terms.ranked = !f.rank.is_empty();
            terms.network_ranking = f.network_ranking;
            terms.ranges = f.rank_ranges.clone();
        }
        if f.expected || !f.rank.is_empty() {
            let v = ((ranked + conv) * (1.0 + 1e-12)).ceil();
            total = if v >= i64::MAX as f64 { i64::MAX } else { v as i64 };
        } else if conv > 0.0 {
            total = total.saturating_add(conv.min(i64::MAX as f64) as i64);
        }
        total
    }

    /// The sum of [`Self::fine_bound`] over performance orders, without conversion sources, a known life or Rush
    /// masks, in expected mode; None in other modes. `slots[s][k]` is the part of slot `s` at position `k` and each
    /// of `orders` holds the positions of the slots. Without conversions an entry has its own judgement `x` in every
    /// order (mask `m = 1 << x`), and its expected bound is linear in the factors of the windows open at it:
    /// - a window's note and judgement factors count by their positive parts, which bound the clamped sums;
    /// - `max(mjp[m] B, jp4[m][j] (B + A_j)) <= mjp[m] B + Σ_j jp4[m][j] A_j`, as `0 <= jp4[m][j] <= mjp[m]` and
    ///   `A_j >= 0`, an equality for a single judgement;
    /// - the combo, its breaks and the Gekisou combo factors are the same in every order (the combo bonus windows
    ///   of a class do not depend on its position).
    ///
    /// So the sum reads each part's windows once, weighted by the orders that place it there, and each order's
    /// drift from its own command counts at the factor peaks of every slot at any of its positions
    /// ([`slot_peak`]), at least that order's peaks.
    pub(super) fn fine_bound_orders(
        &self,
        power: i64,
        slots: [&[Contrib; 5]; 5],
        orders: &[[usize; 5]],
        scratch: &mut Scratch,
    ) -> Option<i64> {
        let (c, f) = (&self.coef, &self.fine);
        if !f.expected {
            return None;
        }
        let (peak, peak_frame) = (slot_peak(slots, None), slot_peak(slots, Some(f.score_frames)));
        let mut drift = 0f64;
        let mut weight = [[0f64; 5]; 5];
        for positions in orders {
            let mut counts = [(0f64, 0f64); 3];
            for s in 0..5 {
                self.add_part_counts(&slots[s][positions[s]], None, &mut counts);
                weight[s][positions[s]] += 1.0;
            }
            let [fixed, run, most] = counts;
            drift = (drift + self.counts_drift(&CandCounts { fixed, run, most, peak, peak_frame })).next_up();
        }
        let chain = self.chain();
        if !drift.is_finite() || !chain.is_finite() {
            return Some(i64::MAX);
        }
        self.fill_combo_factors(slots.map(|parts| &parts[0]), scratch);
        let ne = c.times.len();
        let judge = slots.iter().flat_map(|parts| parts.iter()).any(|p| p.judge);
        scratch.note.clear();
        scratch.note.resize(ne + 1, 0.0);
        if judge {
            for v in scratch.judge.iter_mut() {
                v.clear();
                v.resize(ne + 1, 0.0);
            }
        }
        scratch.ramp.clear();
        scratch.ramp.resize(ne, 0.0);
        let reads = f.gcombo.as_ref().map(|gc| RampReads { gc, sums: &scratch.sums, times: &c.times });
        for (parts, weight) in slots.iter().zip(&weight) {
            for (part, &n) in parts.iter().zip(weight).filter(|(_, n)| **n > 0.0) {
                for w in &part.windows {
                    let (lo, hi) = (w.lo as usize, w.hi as usize);
                    if let Some(reads) = reads.as_ref().filter(|_| w.ramp != 0) {
                        let ramp = &part.ramps[w.ramp as usize - 1];
                        for e in lo..hi {
                            scratch.ramp[e] += n * reads.factor(w, ramp, e).max(0.0);
                        }
                        continue;
                    }
                    scratch.note[lo] += n * w.note.max(0.0);
                    scratch.note[hi] -= n * w.note.max(0.0);
                    if judge {
                        for j in 0..4 {
                            scratch.judge[j][lo] += n * w.judge[j].max(0.0);
                            scratch.judge[j][hi] -= n * w.judge[j].max(0.0);
                        }
                    }
                }
            }
        }
        let p = power.max(0) as f64;
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
            let since = if f.nobreak.get(e).copied().unwrap_or(false) { gs } else { gs - from };
            let combo = f.combo_max.get(since).copied().unwrap_or(f64::INFINITY);
            let k =
                if f.gcombo.is_some() { f.pre[e] * scratch.gk_g[e] * combo / f.cnc } else { f.pre[e] * combo / f.cnc };
            acc += scratch.note[e];
            let mut v = f.mjp[m] * (n + (acc + scratch.ramp[e]).max(0.0) + drift);
            if judge {
                for j in 0..4 {
                    accj[j] += scratch.judge[j][e];
                    v += f.jp4[m][j] * accj[j].max(0.0);
                }
            }
            let x = p * k * v * (1.0 + chain);
            let z = if c.z[e] == 1.0 { x } else { c.z[e] * x * (1.0 + 2f64.powi(-20)) };
            ranked += if f.rank.is_empty() { z } else { z * f.rank[e] };
        }
        let v = (ranked * (1.0 + 1e-12)).ceil();
        Some(if v >= i64::MAX as f64 { i64::MAX } else { v as i64 })
    }
}

/// The largest sum over the slots of the largest factor sum of one slot's part at any of its positions (`slots[s][k]`
/// at position `k`), over the span starts of all of them: of the spans containing that chart time (`frames` None),
/// or meeting its native score frame. At least the `peak` (or `peak_frame`) of [`CandCounts`] for the parts of every
/// order, each slot at one of its positions; each sum rounds up.
fn slot_peak(slots: [&[Contrib; 5]; 5], frames: Option<ScoreFrames>) -> f64 {
    let mut events = Vec::new();
    let mut starts = Vec::new();
    for (s, parts) in slots.iter().enumerate() {
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
        let total = sums.chunks(5).fold(0f64, |t, slot| (t + slot.iter().copied().fold(f64::MIN, f64::max)).next_up());
        best = best.max(total);
    }
    best
}

pub(crate) struct JointFineBounds {
    pub(super) coef: Coef,
    pub(super) fine: Fine,
    pub(super) chain_extra: f64,
    pub(super) contrib: Vec<Vec<[Contrib; 5]>>,
    pub(super) class_of: Vec<Vec<u16>>,
    pub(super) raw: Option<raw::RawEnvelope>,
}
#[derive(Default)]
pub(crate) struct JointScratch(pub(super) Scratch);
impl JointScratch {
    pub(crate) fn rush_windows(&self) -> crate::search::telemetry::CacheUse {
        self.0.rush_cache.usage
    }
}
impl JointFineBounds {
    /// Inspect the candidate's own float margin separately from coefficient and
    /// window relaxations. The zero-margin value is diagnostic only and must never
    /// be used for pruning, objective evaluation, or an optimality certificate.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn margin_diagnostic(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
        rush_masks: Option<&RushMasks>,
    ) -> serde_json::Value {
        let classes = std::array::from_fn::<_, 5, _>(|s| self.choice_class(members[s], choices[s]));
        let parts = std::array::from_fn(|s| &self.contrib[members[s]][classes[s]][positions[s]]);
        let mut src = [0; 5];
        for s in 0..5 {
            src[positions[s]] = self.fine.src[members[s]][classes[s]];
        }
        let view = FineView { coef: &self.coef, fine: &self.fine, chain_extra: self.chain_extra };
        let mut scratch = Scratch::default();
        let fine_upper = view.fine_bound(power, parts, src, CandLife::Unknown, &mut scratch, rush_masks);
        let zero_margin =
            view.fine_bound_with_eps(power, parts, src, CandLife::Unknown, &mut scratch, (0.0, 0.0), rush_masks);
        let slots: Vec<_> = (0..5)
            .map(|s| {
                let p = parts[s];
                serde_json::json!({
                    "memberIndex": members[s], "choiceIndex": choices[s],
                    "classIndex": classes[s], "nativePosition": positions[s],
                    "commandExecutions": p.ops, "spanCount": p.spans.len(),
                    "coefficientWindowCount": p.windows.len(), "linearGain": p.gain,
                    "rushRowCount": p.rush.len(),
                    "factorSpans": p.spans,
                    "plainExecutions": p.ops_plain, "plainCommands": p.cmds_plain,
                    "rushRows": p.rush.iter().map(|r| {
                        let masks = rush_masks.filter(|_| self.fine.rush_eligible);
                        let exec = &self.fine.exec_profile;
                        let at = |t: i64| (get_frame(t.clamp(0, i32::MAX as i64) as i32).max(0) as usize).min(exec.len());
                        let spans = masks.and_then(|m| r.spec.spans(m)).unwrap_or_default();
                        let in_spans: Vec<u32> = spans
                            .iter()
                            .map(|&(a, b, _)| exec[at(a)..(at(b) + 1).min(exec.len())].iter().copied().max().unwrap_or(0))
                            .collect();
                        serde_json::json!({"runCap": r.run_cap, "executions": r.ops, "executionsPerRun": r.ops_per_run,
                            "commands": r.cmds, "commandsPerRun": r.cmds_per_run, "maxRuns": r.max_runs,
                            "counted": r.counts(masks), "maskSpans": spans, "maxExecutionsInMaskSpans": in_spans})
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        let CandCounts { fixed, run, most: (executions, commands), peak, peak_frame } =
            view.cand_counts(parts, rush_masks);
        let exec = &self.fine.exec_profile;
        let per_second: Vec<u32> = exec.chunks(25).map(|c| c.iter().copied().max().unwrap_or(0)).collect();
        serde_json::json!({
            "executionsMaxPerSecond": per_second,
            "countedExecutions": executions, "countedCommands": commands, "peakFactor": peak,
            "fixedCounts": fixed, "countsPerProbeStart": run,
            "probeStarts": self.fine.probe_starts.map(|s| [s.most, s.expected, s.luck]),
            "peakFactorNearFrame": peak_frame,
            "candidateDrift": view.cand_drift(parts, rush_masks), "chainMargin": view.chain(),
            "fineScoreUpper": fine_upper.to_string(),
            "diagnosticScoreWithZeroMargin": zero_margin.to_string(),
            "zeroMarginIsAdmissible": false,
            "rushMasksProvided": rush_masks.is_some(),
            "slots": slots,
        })
    }

    /// Diagnostics only: the candidate cap with every slot, with none, and without each slot, beside each slot's
    /// linear gain and the envelope's base coefficient, at one set of positions.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn slot_attribution(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
    ) -> serde_json::Value {
        let classes = std::array::from_fn::<_, 5, _>(|s| self.choice_class(members[s], choices[s]));
        let empty = Contrib::default();
        let view = FineView { coef: &self.coef, fine: &self.fine, chain_extra: self.chain_extra };
        let mut scratch = Scratch::default();
        let mut cap = |keep: [bool; 5]| {
            let parts = std::array::from_fn(|k| {
                let s = positions.iter().position(|&p| p == k).expect("positions are a permutation");
                if keep[s] { &self.contrib[members[s]][classes[s]][k] } else { &empty }
            });
            let mut src = [0; 5];
            for s in (0..5).filter(|&s| keep[s]) {
                src[positions[s]] = self.fine.src[members[s]][classes[s]];
            }
            view.fine_bound(power, parts, src, CandLife::Unknown, &mut scratch, None)
        };
        let all = cap([true; 5]);
        let none = cap([false; 5]);
        let without: Vec<i64> = (0..5).map(|s| cap(std::array::from_fn(|t| t != s))).collect();
        let only: Vec<i64> = (0..5).map(|s| cap(std::array::from_fn(|t| t == s))).collect();
        let gains: Vec<f64> = (0..5).map(|s| self.contrib[members[s]][classes[s]][positions[s]].gain).collect();
        let ne = self.coef.times.len();
        serde_json::json!({"power":power,"all":all,"none":none,"without":without,"only":only,"gains":gains,
            "baseCoefficient":self.coef.pc[ne]})
    }

    /// The per-entry terms of the candidate cap, for slack attribution against a simulation. Not a bound by itself.
    /// Diagnostics only: the Gekisou combo bonus windows `(slot, start, end, bonus)` the bound reads for a candidate.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn cb_windows(
        &self,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
    ) -> Vec<(usize, i64, i64, f64)> {
        let mut out = Vec::new();
        for s in 0..5 {
            let class = self.choice_class(members[s], choices[s]);
            for &(a, b, v, _) in &self.contrib[members[s]][class][positions[s]].cb {
                out.push((s, a, b, v));
            }
        }
        out
    }

    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn fine_trace(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
        rush_masks: Option<&RushMasks>,
    ) -> (i64, Vec<[f64; 8]>) {
        let classes = std::array::from_fn::<_, 5, _>(|s| self.choice_class(members[s], choices[s]));
        let parts = std::array::from_fn(|s| &self.contrib[members[s]][classes[s]][positions[s]]);
        let mut src = [0; 5];
        for s in 0..5 {
            src[positions[s]] = self.fine.src[members[s]][classes[s]];
        }
        let view = FineView { coef: &self.coef, fine: &self.fine, chain_extra: self.chain_extra };
        let mut scratch = Scratch { trace: Some(Vec::new()), ..Scratch::default() };
        let total = view.fine_bound(power, parts, src, CandLife::Unknown, &mut scratch, rush_masks);
        (total, scratch.trace.take().unwrap_or_default())
    }

    pub(crate) fn raw_upper(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
    ) -> Option<i128> {
        let classes = std::array::from_fn(|s| self.choice_class(members[s], choices[s]));
        self.raw.as_ref()?.upper(power, members, classes, positions)
    }
    /// This is bound-metadata identity, not a claim of native simulation equivalence.
    pub(crate) fn choice_class(&self, member: usize, choice: usize) -> usize {
        if choice == 0 { 0 } else { self.class_of[member][choice - 1] as usize }
    }

    /// At least the final life of a complete team in the performance order with these positions, when nothing but
    /// recoveries at its performers' own skill events can raise its life (see `Fine::final_life`); None otherwise.
    pub(crate) fn final_life_cap(
        &self,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
    ) -> Option<i64> {
        if !self.fine.life_rows_listed {
            return None;
        }
        let mut rec = [0i64; 5];
        for s in 0..5 {
            match self.fine.life[members[s]][self.choice_class(members[s], choices[s])] {
                LifeKind::None => {}
                LifeKind::Recovery(r) => rec[positions[s]] = r,
                LifeKind::Other => return None,
            }
        }
        Some(self.fine.final_life(rec))
    }

    /// The most life a performer recovers at its own skill events with any of these choices (see
    /// [`JointFineBounds::final_life_cap`]); None when one of them has another row that can raise the life, or when
    /// the life rows are not listed.
    pub(crate) fn recovery_reach(&self, member: usize, choices: impl IntoIterator<Item = usize>) -> Option<i64> {
        if !self.fine.life_rows_listed {
            return None;
        }
        let mut reach = 0i64;
        for choice in choices {
            match self.fine.life[member][self.choice_class(member, choice)] {
                LifeKind::None => {}
                LifeKind::Recovery(r) => reach = reach.max(r),
                LifeKind::Other => return None,
            }
        }
        Some(reach)
    }

    /// At least the final life of every team whose performer at position `k` has no life-raising row but recoveries
    /// at its own skill events of at most `rec[k]` in total: the fold of [`JointFineBounds::final_life_cap`] is
    /// non-decreasing in each recovery (every step `clamp(x + r - d, 0, 2 * base)` is, and 0 stays 0).
    pub(crate) fn final_life_reach(&self, rec: [i64; 5]) -> i64 {
        self.fine.final_life(rec)
    }
    /// The per-entry terms of [`JointFineBounds::upper`] for the same candidate and order.
    pub(crate) fn cap_terms(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
        scratch: &mut JointScratch,
        rush_masks: Option<&RushMasks>,
    ) -> (i64, CapTerms) {
        scratch.0.terms = Some(CapTerms::default());
        let total = self.upper(power, members, choices, positions, scratch, rush_masks);
        (total, scratch.0.terms.take().unwrap_or_default())
    }

    /// Choices are domain Snap indexes plus one (zero means no Snap). Positions are fixed by the native root.
    pub(crate) fn upper(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
        scratch: &mut JointScratch,
        rush_masks: Option<&RushMasks>,
    ) -> i64 {
        let classes: [usize; 5] =
            std::array::from_fn(
                |s| if choices[s] == 0 { 0 } else { self.class_of[members[s]][choices[s] - 1] as usize },
            );
        let parts = std::array::from_fn(|s| &self.contrib[members[s]][classes[s]][positions[s]]);
        let mut src = [0; 5];
        for s in 0..5 {
            src[positions[s]] = self.fine.src[members[s]][classes[s]];
        }
        FineView { coef: &self.coef, fine: &self.fine, chain_extra: self.chain_extra }.fine_bound(
            power,
            parts,
            src,
            CandLife::Unknown,
            &mut scratch.0,
            rush_masks,
        )
    }

    /// The sum of [`JointFineBounds::upper`] (without Rush masks) over performance orders, each of `orders` holding
    /// the positions of the slots, as one bound ([`FineView::fine_bound_orders`]): None when a slot has a conversion
    /// source or the bounds are not expected ones.
    pub(crate) fn upper_orders(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        orders: &[[usize; 5]],
        scratch: &mut JointScratch,
    ) -> Option<i64> {
        let classes: [usize; 5] = std::array::from_fn(|s| self.choice_class(members[s], choices[s]));
        if (0..5).any(|s| self.fine.src[members[s]][classes[s]] != 0) {
            return None;
        }
        let slots = std::array::from_fn(|s| &self.contrib[members[s]][classes[s]]);
        FineView { coef: &self.coef, fine: &self.fine, chain_extra: self.chain_extra }.fine_bound_orders(
            power,
            slots,
            orders,
            &mut scratch.0,
        )
    }
}

/// The per-entry terms of one candidate cap: `(chart time, floored bound, rank factor)` per entry, the conversion
/// budget gain and whether rank bonuses apply. The cap is the entries' sum (rank weighted, plus the gain, rounded up).
#[derive(Clone, Debug, Default)]
pub(crate) struct CapTerms {
    pub(crate) entries: Vec<(i32, f64, f64)>,
    pub(crate) conv: f64,
    pub(crate) ranked: bool,
    pub(crate) network_ranking: bool,
    /// Per conversion budget row: (most conversions, `(entry, rank-weighted gain)` of the entries it can convert).
    pub(crate) rows: Vec<(f64, Vec<(u32, f64)>)>,
    /// Per completing range with a rank bonus: (range end time, percent / 100, its entries).
    pub(crate) ranges: Vec<(i32, f64, Vec<u32>)>,
}

#[derive(Default)]
pub(super) struct Scratch {
    pub(super) rush_cache: rush::WindowCache,
    pub(super) rush_replacements: Vec<Option<Rc<rush::EntryWindows>>>,
    pub(super) note: Vec<f64>,
    pub(super) judge: [Vec<f64>; 4],
    /// Gekisou combo factors, bonus events and sums of one candidate.
    pub(super) gk_g: Vec<f64>,
    pub(super) ev: Vec<(i64, f64)>,
    pub(super) sums: Vec<f64>,
    /// Budget rows of one candidate (source, row, next eligible entry) and their conversion gains.
    pub(super) brow: Vec<(u32, usize, usize)>,
    pub(super) bterms: Vec<Vec<f64>>,
    /// Combo ramp windows of the candidate (part, window) and their per-entry note factors.
    pub(super) ramp_windows: Vec<(usize, Window)>,
    pub(super) ramp: Vec<f64>,
    /// Per-entry terms of the last bound, when requested (search cutoff tables).
    pub(super) terms: Option<CapTerms>,
    /// Diagnostics only: per entry `[time, floored bound, rank factor, k, 1 + note factors, life factor, Just
    /// factors, Gekisou combo factor]`.
    #[cfg(feature = "search-diagnostics")]
    pub(super) trace: Option<Vec<[f64; 8]>>,
}
