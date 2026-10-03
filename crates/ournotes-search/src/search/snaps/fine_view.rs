//! Per-note candidate caps and the joint fine-bound view used by physical search.
use super::*;

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
    /// the windows that meet one 40 ms frame) and once in the undo; `ops` counts the executions; each factor's
    /// binary32 representation adds two more roundings per lifetime command
    /// (integer-to-float conversion and division, including saturated mill values). Infinite without a
    /// certificate.
    pub(super) fn cand_drift(&self, parts: [&Contrib; 5], rush_masks: Option<&RushMasks>) -> f64 {
        let (e, n, peak, peak_frame) = self.cand_counts(parts, rush_masks);
        let representation = (2.0 * n).next_up();
        let weight = ((3.0 * e).next_up() + representation).next_up();
        let Some(amplification) = float_margin::amplification(weight, 2f64.powi(-24)) else {
            return f64::INFINITY;
        };
        // A start/end pair at the same timestamp can transiently change a field
        // even when its half-open factor window contains no note. The frame
        // neighborhood also covers those intermediate command states.
        let state = (1.0 + peak.max(peak_frame)).next_up();
        let magnitude = ((e * ((2.0 * state).next_up() + peak_frame).next_up()).next_up()
            + (representation * state).next_up())
        .next_up();
        let drift = ((magnitude * 2f64.powi(-24)).next_up() * amplification).next_up();
        if drift.is_finite() { drift } else { f64::INFINITY }
    }

    /// The relative allowance of the native chain after the score-up factor.
    pub(super) fn chain(&self) -> f64 {
        float_margin::with_chain(0.0, self.chain_extra).unwrap_or(f64::INFINITY)
    }

    /// The counts `cand_drift` reads: command executions, lifetime commands, the largest factor sum at one command
    /// time and within one frame of it.
    fn cand_counts(&self, parts: [&Contrib; 5], rush_masks: Option<&RushMasks>) -> (f64, f64, f64, f64) {
        let mut peak = 0f64;
        let mut peak_frame = 0f64;
        let (mut e, mut n) = (0f64, 0f64);
        let rush_masks = rush_masks.filter(|_| self.fine.rush_eligible);
        for p in parts {
            e = (e + p.ops_plain).next_up();
            n = (n + p.cmds_plain).next_up();
            for row in &p.rush {
                let (ops, cmds) = row.counts(rush_masks);
                e = (e + ops).next_up();
                n = (n + cmds).next_up();
            }
            for &(a, _, _) in &p.spans {
                let (mut at, mut near) = (0f64, 0f64);
                for q in parts {
                    for &(b0, b1, g) in &q.spans {
                        if b0 <= a && a < b1 {
                            at = (at + g).next_up();
                        }
                        if b0 <= a.saturating_add(40) && a.saturating_sub(40) <= b1 {
                            near = (near + g).next_up();
                        }
                    }
                }
                peak = peak.max(at);
                peak_frame = peak_frame.max(near);
            }
        }
        (e, n, peak, peak_frame)
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
        let mut gk_g = std::mem::take(&mut scratch.gk_g);
        if let Some(gc) = &f.gcombo {
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
                    // the step reads every window open; a gate opened by it only adds bonus
                    let step = (1.0 + (cur + running().map(|w| w.2).sum::<f64>()).max(0.0)).floor();
                    cur + running().filter(|w| gate_open(w.3, ri, acc, group, step)).map(|w| w.2).sum::<f64>()
                },
                &mut gk_g,
                &mut scratch.sums,
            );
            scratch.ev = ev;
        }
        // Combo ramps: each entry reads the factor at its range's combo bound (the flat factor where unknown).
        scratch.ramp.clear();
        scratch.ramp.resize(ne, 0.0);
        if let Some(gc) = &f.gcombo
            && !scratch.ramp_windows.is_empty()
        {
            let mut counts = std::mem::take(&mut scratch.counts);
            gc.inclusive_counts(&c.times, &scratch.sums, &mut counts);
            for &(k, w) in &scratch.ramp_windows {
                let r = &parts[k].ramps[w.ramp as usize - 1];
                for e in w.lo as usize..w.hi as usize {
                    let cnt = counts[e];
                    scratch.ramp[e] += if cnt.is_nan() {
                        w.note
                    } else {
                        let steps =
                            (ournotes_sim::num::floor_to_i32(cnt as f32 / r.unit as f32) as i64).clamp(0, r.max_count);
                        r.table[(steps as usize).min(r.table.len() - 1)] * r.mult
                    };
                }
            }
            scratch.counts = counts;
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
            let dead = match life {
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
            if f.rank.is_empty() {
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
            terms.ranges = f.rank_ranges.clone();
        }
        if !f.rank.is_empty() {
            let v = ((ranked + conv) * (1.0 + 1e-12)).ceil();
            total = if v >= i64::MAX as f64 { i64::MAX } else { v as i64 };
        } else if conv > 0.0 {
            total = total.saturating_add(conv.min(i64::MAX as f64) as i64);
        }
        total
    }
}

pub(crate) struct JointFineBounds {
    pub(super) rush_eligible: bool,
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
    /// The domain preserves LUCK judgement semantics and has a Rush score factor or refinable Rush
    /// score windows. Avoid invoking the replay oracle for unrelated charts.
    pub(crate) fn rush_eligible(&self) -> bool {
        self.rush_eligible
    }

    /// Every life a live of the compiled domain can reach, at any time.
    pub(crate) fn life_range(&self) -> (i64, i64) {
        self.fine.life_range
    }

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
        let (executions, commands, peak, peak_frame) = view.cand_counts(parts, rush_masks);
        let exec = &self.fine.exec_profile;
        let per_second: Vec<u32> = exec.chunks(25).map(|c| c.iter().copied().max().unwrap_or(0)).collect();
        serde_json::json!({
            "executionsMaxPerSecond": per_second,
            "countedExecutions": executions, "countedCommands": commands, "peakFactor": peak,
            "peakFactorNearFrame": peak_frame,
            "candidateDrift": view.cand_drift(parts, rush_masks), "chainMargin": view.chain(),
            "fineScoreUpper": fine_upper.to_string(),
            "diagnosticScoreWithZeroMargin": zero_margin.to_string(),
            "zeroMarginIsAdmissible": false,
            "rushMasksProvided": rush_masks.is_some(),
            "slots": slots,
        })
    }

    /// Diagnostics: the linear relaxation `a0 + sum of slot gains` of one complete candidate at one native order,
    /// split into its terms: the base coefficient with the luck rush factor on every LUCK entry, on none, and only
    /// where the masks allow a rush bonus; each slot's plain windows and Rush rows over their full windows and over
    /// the replayed spans. The restricted values are not admissible bounds by themselves.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn linear_profile(
        &self,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
        rush_masks: Option<&RushMasks>,
    ) -> serde_json::Value {
        let (c, f) = (&self.coef, &self.fine);
        let ne = c.times.len();
        let masks = rush_masks.filter(|_| f.rush_eligible);
        let plain_ratio =
            |e: usize| if f.pre_plain.is_empty() || f.pre[e] == 0.0 { 1.0 } else { f.pre_plain[e] / f.pre[e] };
        let inside = |e: usize| masks.is_none_or(|m| m.rush_possible(c.times[e]));
        // Prefix sums of z k jp with k reading the luck factor nowhere (0) or only inside the masks (1).
        let mut pc = [vec![0f64; ne + 1], vec![0f64; ne + 1]];
        let mut pj = [vec![[0f64; 4]; ne + 1], vec![[0f64; 4]; ne + 1]];
        let (mut luck_entries, mut luck_inside) = (0usize, 0usize);
        for e in 0..ne {
            let r = plain_ratio(e);
            if r != 1.0 {
                luck_entries += 1;
                luck_inside += usize::from(masks.is_some() && inside(e));
            }
            let ks = [c.k[e] * r, if inside(e) { c.k[e] } else { c.k[e] * r }];
            for v in 0..2 {
                pc[v][e + 1] = pc[v][e] + c.z[e] * ks[v] * c.max_jp[e];
                for j in 0..4 {
                    pj[v][e + 1][j] = pj[v][e][j] + c.z[e] * ks[v] * c.jp[e][j];
                }
            }
        }
        let gain = |lo: usize, hi: usize, note: f64, judge: [f64; 4], v: Option<usize>| -> f64 {
            let mut g = match v {
                None => note * (c.pc[hi] - c.pc[lo]),
                Some(v) => note * (pc[v][hi] - pc[v][lo]),
            };
            for j in 0..4 {
                if judge[j] != 0.0 {
                    g += judge[j]
                        * match v {
                            None => c.pj[j][hi] - c.pj[j][lo],
                            Some(v) => pj[v][hi][j] - pj[v][lo][j],
                        };
                }
            }
            g
        };
        let mut sums = [0f64; 6];
        let mut slots = Vec::new();
        for s in 0..5 {
            let class = self.choice_class(members[s], choices[s]);
            let part = &self.contrib[members[s]][class][positions[s]];
            let replaced: Vec<Option<Vec<(usize, usize, f64)>>> = part
                .rush
                .iter()
                .map(|row| {
                    let spans = row.spec.spans(masks?)?;
                    Some(
                        spans
                            .into_iter()
                            .filter_map(|(a, b, mult)| {
                                let lo = c.times.partition_point(|&t| (t as i64) < a);
                                let hi = c.times.partition_point(|&t| (t as i64) < b);
                                (lo < hi).then_some((lo, hi, mult))
                            })
                            .collect(),
                    )
                })
                .collect();
            // table gain, plain windows, Rush windows full, both with the masked luck factor, Rush windows masked
            let mut t = [part.gain, 0.0, 0.0, 0.0, 0.0, 0.0];
            for w in &part.windows {
                let (lo, hi) = (w.lo as usize, w.hi as usize);
                if w.rush == 0 {
                    t[1] += gain(lo, hi, w.note, w.judge, None);
                    t[3] += gain(lo, hi, w.note, w.judge, Some(1));
                } else {
                    t[2] += gain(lo, hi, w.note, w.judge, None);
                    t[4] += gain(lo, hi, w.note, w.judge, Some(1));
                    if replaced[w.rush as usize - 1].is_none() {
                        t[5] += gain(lo, hi, w.note, w.judge, Some(1));
                    }
                }
            }
            for (row, windows) in part.rush.iter().zip(&replaced) {
                for &(lo, hi, mult) in windows.iter().flatten() {
                    t[5] += gain(lo, hi, row.note * mult, row.judge.map(|x| x * mult), Some(1));
                }
            }
            for i in 0..6 {
                sums[i] += t[i];
            }
            slots.push(serde_json::json!({
                "memberIndex": members[s], "choiceIndex": choices[s], "nativePosition": positions[s],
                "gain": t[0], "plainWindows": t[1], "rushWindowsFull": t[2], "plainWindowsMaskedLuck": t[3],
                "rushWindowsFullMaskedLuck": t[4], "rushWindowsMasked": t[5], "rushRows": part.rush.len(),
                "rushRowsReplaced": replaced.iter().filter(|r| r.is_some()).count(),
            }));
        }
        serde_json::json!({
            "entries": ne, "luckEntries": luck_entries, "luckEntriesInMasks": luck_inside,
            "a0": c.pc[ne], "a0NoLuck": pc[0][ne], "a0MaskedLuck": pc[1][ne],
            "gainSum": sums[0], "plainWindows": sums[1], "rushWindowsFull": sums[2],
            "plainWindowsMaskedLuck": sums[3], "rushWindowsFullMaskedLuck": sums[4], "rushWindowsMasked": sums[5],
            "linearFull": c.pc[ne] + sums[0], "linearMasked": pc[1][ne] + sums[3] + sums[5],
            "rushMasksProvided": masks.is_some(), "slots": slots,
        })
    }

    /// Diagnostics: the per-position maxima the pool-wide drift reads (command counts, executions, and the sum of a
    /// performer's span factors as a stand-in for its factor total), with the performer that attains each count.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn pool_margin_profile(&self) -> serde_json::Value {
        let mut positions = Vec::new();
        let (mut commands, mut plain, mut factors) = (0f64, 0f64, 0f64);
        for k in 0..5 {
            let mut best = (0f64, usize::MAX, usize::MAX);
            let (mut best_plain, mut best_ops, mut best_fac) = (0f64, 0f64, 0f64);
            for (m, classes) in self.contrib.iter().enumerate() {
                for (class, arr) in classes.iter().enumerate() {
                    let p = &arr[k];
                    if p.cmds > best.0 {
                        best = (p.cmds, m, class);
                    }
                    best_plain = best_plain.max(p.cmds_plain);
                    best_ops = best_ops.max(p.ops);
                    best_fac = best_fac.max(p.spans.iter().map(|s| s.2).sum::<f64>());
                }
            }
            commands += best.0;
            plain += best_plain;
            factors += best_fac;
            let argmax = (best.1 != usize::MAX).then(|| {
                let p = &self.contrib[best.1][best.2][k];
                serde_json::json!({"memberIndex": best.1, "classIndex": best.2, "commands": p.cmds,
                    "plainCommands": p.cmds_plain, "spanCount": p.spans.len(),
                    "rushRows": p.rush.iter().map(|r| serde_json::json!({"commands": r.cmds,
                        "commandsPerRun": r.cmds_per_run, "maxRuns": r.max_runs, "runCap": r.run_cap}))
                        .collect::<Vec<_>>()})
            });
            positions.push(serde_json::json!({"position": k, "maxCommands": best.0, "maxPlainCommands": best_plain,
                "maxExecutions": best_ops, "maxSpanFactorSum": best_fac, "argmax": argmax}));
        }
        let e_max = self.fine.exec_profile.iter().copied().max().unwrap_or(0) as f64;
        // The pool drift formula with the span-factor stand-in (an over-estimate of the factor total).
        let ops = (3.0 * e_max + 2.0) * commands;
        let ops_plain = (3.0 * e_max + 2.0) * plain;
        let drift = |ops: f64| ops * 2f64.powi(-24) * (1.0 + factors) / (1.0 - ops * 2f64.powi(-24)).max(1e-9);
        serde_json::json!({"executionMax": e_max, "commands": commands, "plainCommands": plain,
            "spanFactorTotal": factors, "operations": ops, "approxDrift": drift(ops),
            "approxDriftPlainOnly": drift(ops_plain), "positions": positions})
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
}

/// The per-entry terms of one candidate cap: `(chart time, floored bound, rank factor)` per entry, the conversion
/// budget gain and whether rank bonuses apply. The cap is the entries' sum (rank weighted, plus the gain, rounded up).
#[derive(Clone, Debug, Default)]
pub(crate) struct CapTerms {
    pub(crate) entries: Vec<(i32, f64, f64)>,
    pub(crate) conv: f64,
    pub(crate) ranked: bool,
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
    /// Combo ramp windows of the candidate (part, window), their per-entry note factors and range counts.
    pub(super) ramp_windows: Vec<(usize, Window)>,
    pub(super) ramp: Vec<f64>,
    pub(super) counts: Vec<f64>,
    /// Per-entry terms of the last bound, when requested (search cutoff tables).
    pub(super) terms: Option<CapTerms>,
    /// Diagnostics only: per entry `[time, floored bound, rank factor, k, 1 + note factors, life factor, Just
    /// factors, Gekisou combo factor]`.
    #[cfg(feature = "search-diagnostics")]
    pub(super) trace: Option<Vec<[f64; 8]>>,
}
