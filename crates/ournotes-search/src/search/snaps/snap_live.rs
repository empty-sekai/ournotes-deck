//! The prepared Snap Live objective: envelopes, gains and the legacy leaf search entry.
use super::*;

/// The live objective with snap skills, prepared for one search.
pub(crate) struct SnapLive<'a> {
    pub(super) master: &'a Master,
    pub(super) setup: &'a FullSetup,
    /// Classes of each pool member (class 0: no active row; empty for members that are not allowed).
    pub(super) classes: Vec<Vec<Class>>,
    /// Class of each allowed snap for each member.
    pub(super) class_of: Vec<Vec<u16>>,
    /// `contrib[m][c][k]`.
    pub(super) contrib: Vec<Vec<[Contrib; 5]>>,
    /// Identity of each member for the simulation (equal ids: interchangeable performers with equal snap classes).
    pub(super) sim_id: Vec<u32>,
    /// Pool-wide id of each member's classes (equal ids: equal class keys).
    pub(super) class_gid: Vec<Vec<u32>>,
    pub(super) coef: Coef,
    pub(super) fine: Fine,
    pub(super) a0: f64,
    pub(super) global: f64,
    /// Whether some entry can read life 0 at a factor below the one of `Coef::z` (the class search then bounds life).
    pub(super) life_bound: bool,
    /// With `life_bound`, for each member and position (`m * 5 + k`): the prefix sums, with `Coef::z` and with the
    /// life-zero factor, of the largest per-entry gain over the member's classes without other life-raising rows
    /// (empty when it has none).
    pub(super) split: Vec<(Vec<f64>, Vec<f64>)>,
    /// With `life_bound`: the largest conversion budget term (`Contrib::budget`) of each member's classes without
    /// other life-raising rows, by `m * 5 + k`.
    pub(super) split_budget: Vec<f64>,
    /// Relative margin of the bounds.
    pub(super) eps: f64,
    /// Optional additive factor-drift envelope for the physical joint solver.
    /// Legacy class/fine/raw bounds and their overflow guard retain `eps`.
    pub(super) joint_additive: Option<(f64, f64, f64)>,
    /// With a Gekisou combo range: the joint envelope of decks with at most `n < 5` combo carriers, in the form of
    /// `joint_additive` when that applies (None: the next level's; empty: none).
    pub(super) carrier_levels: Vec<Option<CarrierLevel>>,
    /// With carrier levels: the envelopes keyed by the carriers a search prefix placed.
    pub(super) carrier_keys: Option<Rc<CarrierKeys>>,
    /// The part of `eps` for the Gekisou factors of the per-note chain (0 with Gekisou off).
    pub(super) chain_extra: f64,
    /// Seeds per deck (1 with Gekisou off): the value of a deck is the sum of its seed scores, and every bound of
    /// one seed's score is multiplied by `n` before it is compared with the cutoff.
    pub(super) n: i64,
    /// With Gekisou on: a play frame no later than the first frame in which any deck can draw a random number.
    pub(super) prefix_frame: usize,
}

/// A snap's class key for one member: per skill with active rows, in the performer's order, (skill kind: 3 snap, 5
/// Gekisou support; its mission gate; its active rows).
pub(super) type ClassKey = Vec<(i64, i64, Vec<RowSig>)>;

/// Relative error added to the per-note chain margin with Gekisou on (the Gekisou combo and luck factors: their
/// binary32 computation and the two extra multiplications, with margin).
pub(super) const GK_CHAIN_EPS: f64 = 1.0 / (1u64 << 21) as f64;

/// `SnapLive::split`: for each member and position, the largest gain of each entry over the member's classes without
/// other life-raising rows (the value of its windows containing the entry, before the factor after the floor), summed
/// with `Coef::z` and with the life-zero factor.
pub(super) fn split_envelopes(contrib: &[Vec<[Contrib; 5]>], fine: &Fine, coef: &Coef) -> Vec<(Vec<f64>, Vec<f64>)> {
    let ne = coef.times.len();
    let mut out = vec![(Vec::new(), Vec::new()); contrib.len() * 5];
    let mut v = vec![0f64; ne];
    let mut best = vec![0f64; ne];
    for (m, per) in contrib.iter().enumerate() {
        for k in 0..5 {
            let mut any = false;
            best.fill(0.0);
            for (c, arr) in per.iter().enumerate() {
                if fine.life[m][c] == LifeKind::Other {
                    continue;
                }
                any = true;
                let ws = &arr[k].windows;
                for w in ws {
                    for e in w.lo as usize..w.hi as usize {
                        let mut x = w.note * coef.k[e] * coef.max_jp[e];
                        for j in 0..4 {
                            if w.judge[j] != 0.0 {
                                x += w.judge[j] * coef.k[e] * coef.jp[e][j];
                            }
                        }
                        v[e] += x;
                    }
                }
                for w in ws {
                    for e in w.lo as usize..w.hi as usize {
                        best[e] = best[e].max(v[e]);
                    }
                }
                for w in ws {
                    v[w.lo as usize..w.hi as usize].fill(0.0);
                }
            }
            if !any {
                continue;
            }
            let (mut pz, mut pd) = (vec![0f64; ne + 1], vec![0f64; ne + 1]);
            for e in 0..ne {
                pz[e + 1] = pz[e] + coef.z[e] * best[e];
                pd[e + 1] = pd[e] + fine.z_dead * best[e];
            }
            out[m * 5 + k] = (pz, pd);
        }
    }
    out
}

/// The sum of the `n` largest values (all of them when fewer); reorders `v`.
pub(super) fn top_sum(v: &mut [f64], n: f64) -> f64 {
    let n = if n >= v.len() as f64 { v.len() } else { n.max(0.0) as usize };
    if n == 0 {
        return 0.0;
    }
    if n < v.len() {
        v.select_nth_unstable_by(n - 1, |a, b| b.total_cmp(a));
    }
    v[..n].iter().sum()
}

/// The largest `sum_i g[i][k_i]` over the assignments of the five slots to distinct positions.
pub(super) fn best_assignment(g: &[[f64; 5]; 5]) -> f64 {
    let mut best = [f64::MIN; 32];
    best[0] = 0.0;
    for mask in 0usize..31 {
        if best[mask] == f64::MIN {
            continue;
        }
        let i = mask.count_ones() as usize;
        for k in 0..5 {
            if mask & (1 << k) == 0 {
                let v = best[mask] + g[i][k];
                let next = mask | (1 << k);
                if v > best[next] {
                    best[next] = v;
                }
            }
        }
    }
    best[31]
}

pub(super) fn ub(power: i64, a: f64, eps: f64) -> i64 {
    let v = (power.max(0) as f64) * a * (1.0 + eps);
    if v >= i64::MAX as f64 { i64::MAX } else { v.ceil() as i64 }
}

/// Factor-state drift is absolute: every command touches one field, and the
/// total operation count bounds the sum of errors in note plus any one judgement
/// field. A nonzero frame undo has a corresponding command, so its subtraction
/// is already charged in the three operations per command execution. The final
/// binary32 addition of those two fields is still covered by the chain margin.
///
/// `delta` conservatively includes the legacy chain allowances as well as drift.
/// We add delta*B once, where B uses ALL judgement percentages: `a0` omits
/// budgeted conversions and cannot serve as the factor-error sensitivity.
/// The bootstrap gate keeps the rounding amplification below the existing1.01
/// reserve. Failure leaves the original relative envelope intact.
pub(super) fn additive_joint_envelope(
    a0: f64,
    global: f64,
    delta: f64,
    roundings: f64,
    sensitivity: f64,
    chain_extra: f64,
) -> Option<(f64, f64, f64)> {
    if [a0, global, delta, roundings, sensitivity, chain_extra].iter().any(|v| !v.is_finite() || *v < 0.0)
        // The ideal note-plus-judgement factor is at least one. Retain a
        // positive actual factor throughout the certified float chain.
        || delta >= 0.5
        || (roundings * 2f64.powi(-24)).next_up() > 1.0 / 256.0
    {
        return None;
    }
    let mut chain = 1.0f64;
    for allowance in [2f64.powi(-22), CHAIN_EPS, 2f64.powi(-19), chain_extra] {
        chain = (chain * (1.0 + allowance).next_up()).next_up();
    }
    let offset = (delta * sensitivity).next_up();
    let result = ((a0 + offset).next_up(), (global + offset).next_up(), (chain - 1.0).next_up());
    [result.0, result.1, result.2].iter().all(|v| v.is_finite()).then_some(result)
}

/// The drift of the factor state of a deck whose performers file at most `n_cmd` factor commands with factors
/// summing to at most `f_tot`, each frame executing at most `e_max` times up to the last note: every float operation
/// on the state rounds by at most 2^-24 of a value below `1 + f_tot`; each execution applies the commands (two
/// roundings each: state and frame diff) and each undo one more; each mill value is also cast to binary32 then
/// divided, two representation roundings. None without a finite certificate.
pub(super) fn factor_drift(e_max: f64, n_cmd: f64, f_tot: f64) -> Option<f64> {
    let ops = (((3.0 * e_max).next_up() * n_cmd).next_up() + (2.0 * n_cmd).next_up()).next_up();
    let amplification = float_margin::amplification(ops, 2f64.powi(-24))?;
    Some((((ops * 2f64.powi(-24)).next_up() * (1.0 + f_tot).next_up()).next_up() * amplification).next_up())
}

/// Outward coefficient norm for an additive error in the final score-up factor.
/// k already includes the pool combo/luck and rank envelopes; z includes the
/// applicable life/assist envelope. Budgeted judgement conversions are covered by
/// the settings-wide maximum rather than the narrower Coef::max_jp.
pub(super) fn factor_error_sensitivity(coef: &Coef, judgement_max: f64) -> Option<f64> {
    if !judgement_max.is_finite() || judgement_max < 0.0 || coef.k.len() != coef.z.len() {
        return None;
    }
    let mut sum = 0.0f64;
    for (&k, &z) in coef.k.iter().zip(&coef.z) {
        if !k.is_finite() || !z.is_finite() || k < 0.0 || z < 0.0 {
            return None;
        }
        let term = (((k * z).next_up()) * judgement_max).next_up();
        sum = (sum + term).next_up();
    }
    sum.is_finite().then_some(sum)
}

impl<'a> SnapLive<'a> {
    /// A root-independent relaxation, not an optimized performance order or a class reduction.
    /// Every physical pair and all five position gains are retained for native-root bounds.
    pub(crate) fn joint_envelope(&self) -> (f64, f64, f64, Vec<Vec<[f64; 5]>>) {
        let gains = self
            .class_of
            .iter()
            .enumerate()
            .map(|(m, classes)| {
                if self.contrib[m].is_empty() {
                    return Vec::new();
                }
                std::iter::once(0)
                    .chain(classes.iter().map(|&c| c as usize))
                    .map(|c| self.contrib[m][c].each_ref().map(|p| p.gain))
                    .collect()
            })
            .collect();
        let (a0, global, eps) = self.joint_additive.unwrap_or((self.a0, self.global, self.eps));
        (a0, global, eps, gains)
    }

    /// `joint_envelope` for the decks with at most `n < 5` Gekisou combo carriers: per `n`, `(A0, global, gains)`
    /// with gains by member and choice (None: the next level's, finally the pool-wide one), and whether each member
    /// and choice is a carrier. None without a combo range.
    #[allow(clippy::type_complexity)]
    pub(crate) fn joint_carrier_levels(&self) -> Option<(Vec<Option<(f64, f64, Vec<Vec<[f64; 5]>>)>>, Vec<Vec<bool>>)> {
        if self.carrier_levels.iter().all(Option::is_none) {
            return None;
        }
        let by_choice = |m: usize| std::iter::once(0).chain(self.class_of[m].iter().map(|&c| c as usize));
        let levels = self
            .carrier_levels
            .iter()
            .map(|level| {
                let level = level.as_ref()?;
                let gains = (0..self.class_of.len())
                    .map(|m| {
                        if level.gains[m].is_empty() {
                            return Vec::new();
                        }
                        by_choice(m).map(|c| level.gains[m][c]).collect()
                    })
                    .collect();
                Some((level.a0, level.global, gains))
            })
            .collect();
        let carrier = (0..self.class_of.len())
            .map(|m| {
                if self.contrib[m].is_empty() {
                    return Vec::new();
                }
                by_choice(m).map(|c| is_carrier_class(&self.contrib, m, c)).collect()
            })
            .collect();
        Some((levels, carrier))
    }

    /// The envelopes keyed by the Gekisou combo carriers a search prefix placed (see `CarrierKeys`).
    pub(crate) fn carrier_keys(&self) -> Option<Rc<CarrierKeys>> {
        self.carrier_keys.clone()
    }

    /// An upper bound of the live score of any deck with power at most `power`.
    pub fn score_bound(&self, power: i64) -> i64 {
        ub(power, self.global, self.eps).saturating_mul(self.n)
    }

    /// The number of snap classes of each allowed member `(card id, classes)`.
    pub fn class_counts(&self, pool: &Pool) -> Vec<(i64, u32)> {
        self.classes
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_empty())
            .map(|(m, c)| (pool.members[m].id, c.len() as u32))
            .collect()
    }

    /// The largest gain of member `m` at each position over its classes (`G(k, m, c)`; 0 for members that are not
    /// allowed).
    pub fn position_gains(&self, m: usize) -> [f64; 5] {
        let mut g = [0f64; 5];
        for c in &self.contrib[m] {
            for (k, x) in g.iter_mut().enumerate() {
                *x = x.max(c[k].gain);
            }
        }
        g
    }

    /// An upper bound of the live score of any deck with power at most `power` whose positions' gains sum to at
    /// most `gain`.
    pub fn gain_bound(&self, power: i64, gain: f64) -> i64 {
        ub(power, (self.a0 + gain).min(self.global), self.eps).saturating_mul(self.n)
    }

    /// The first entry (chart-time order) from which every entry reads life 0 under `life` (the number of entries
    /// when there is none).
    pub(super) fn dead_start(&self, life: CandLife) -> usize {
        let f = &self.fine;
        match life {
            CandLife::NoRise => f.dead_from,
            CandLife::ZeroFrom(t0) => {
                let a = self.coef.times.partition_point(|&t| (t as i64) < t0);
                a.max(f.until_min.partition_point(|&u| u <= t0))
            }
            CandLife::Unknown => self.coef.times.len(),
        }
    }

    /// `A0` plus the gains of `parts`, with the entries from `start` on at the life-zero factor.
    pub(super) fn life_sum(&self, parts: [&Contrib; 5], start: usize) -> f64 {
        self.a0_from(start) + parts.iter().map(|p| self.gain_from(p, start)).sum::<f64>()
    }

    /// At least the gain of every class of member `m` without other life-raising rows at position `k`, with the
    /// entries from `start` on at the life-zero factor (`plain`: at least the gain of every such class).
    pub(super) fn split_gain(&self, m: usize, k: usize, start: usize, plain: f64) -> f64 {
        match self.split.get(m * 5 + k) {
            Some((pz, pd)) if !pz.is_empty() => {
                let ne = self.coef.times.len();
                let s = start.min(ne);
                plain.min(pz[s] + (pd[ne] - pd[s]) + self.split_budget.get(m * 5 + k).copied().unwrap_or(0.0))
            }
            _ => plain,
        }
    }

    /// `A0` with the entries from `start` on at the life-zero factor.
    pub(super) fn a0_from(&self, start: usize) -> f64 {
        let c = &self.coef;
        let ne = c.times.len();
        c.pc[start.min(ne)] + (c.pcd[ne] - c.pcd[start.min(ne)])
    }

    /// The gain of one part with the entries from `start` on at the life-zero factor (at most its `gain`).
    pub(super) fn gain_from(&self, part: &Contrib, start: usize) -> f64 {
        let c = &self.coef;
        let seg = |p: &[f64], d: &[f64], lo: usize, hi: usize| -> f64 {
            (p[hi.min(start)] - p[lo.min(start)]) + (d[hi.max(start)] - d[lo.max(start)])
        };
        let mut total = part.budget;
        for w in &part.windows {
            let (lo, hi) = (w.lo as usize, w.hi as usize);
            total += w.note * seg(&c.pc, &c.pcd, lo, hi);
            for j in 0..4 {
                if w.judge[j] != 0.0 {
                    total += w.judge[j] * seg(&c.pj[j], &c.pjd[j], lo, hi);
                }
            }
        }
        total
    }

    pub(super) fn fine_bound(
        &self,
        power: i64,
        parts: [&Contrib; 5],
        src: [u32; 5],
        life: CandLife,
        scratch: &mut Scratch,
    ) -> i64 {
        FineView { coef: &self.coef, fine: &self.fine, chain_extra: self.chain_extra }
            .fine_bound(power, parts, src, life, scratch, None)
    }

    /// Move the compiled per-note data into the native-root joint solver. No scorer state is retained.
    pub(crate) fn into_joint_fine(self) -> JointFineBounds {
        let raw = raw::RawEnvelope::compile(&self.coef, &self.fine, &self.contrib, self.eps, self.chain_extra);
        JointFineBounds {
            coef: self.coef,
            fine: self.fine,
            chain_extra: self.chain_extra,
            contrib: self.contrib,
            class_of: self.class_of,
            raw,
        }
    }

    /// The best representative (score, power, snaps, order) of the member set `members` (slot order, leader at slot
    /// 2) whose member-only power is `fixed`; `None` when no deck of the set reaches `threshold`. The second value
    /// is true when the deadline passed (the result is then the best deck found so far).
    #[allow(clippy::too_many_arguments)]
    pub fn best<'m>(
        &self,
        pool: &Pool<'m>,
        t: &Tables<'m>,
        members: [usize; 5],
        fixed: i64,
        threshold: i64,
        deadline: Option<Instant>,
        stats: &mut PowerStats,
    ) -> Result<(Option<LeafBest>, bool), Error> {
        let cls: [&Vec<Class>; 5] = members.map(|m| &self.classes[m]);
        let wb: Vec<Vec<i64>> = (0..5)
            .map(|i| {
                cls[i]
                    .iter()
                    .enumerate()
                    .map(|(c, class)| {
                        let best = class.snaps.iter().map(|&j| t.w[members[i]][j]).max();
                        if c == 0 { best.unwrap_or(0).max(0) } else { best.unwrap_or(i64::MIN / 4) }
                    })
                    .collect()
            })
            .collect();
        let wbmax: [i64; 5] = std::array::from_fn(|i| wb[i].iter().copied().max().unwrap_or(0));
        let gmax: [[f64; 5]; 5] = std::array::from_fn(|i| {
            std::array::from_fn(|k| self.contrib[members[i]].iter().map(|c| c[k].gain).fold(0f64, f64::max))
        });
        let s1max = fixed + wbmax.iter().sum::<i64>();
        let eps = self.eps;
        // every order with its bound, best first (ties keep the lexicographic order)
        let mut orders: Vec<(i64, [usize; 5])> = Vec::with_capacity(120);
        let mut o = [0usize, 1, 2, 3, 4];
        loop {
            let s2 = self.a0 + (0..5).map(|k| gmax[o[k]][k]).sum::<f64>();
            orders.push((ub(s1max, s2, eps), o));
            if !crate::search::live::next_permutation(&mut o) {
                break;
            }
        }
        orders.sort_by_key(|x| std::cmp::Reverse(x.0));
        if orders[0].0.saturating_mul(self.n) < threshold {
            return Ok((None, false));
        }
        let mut lf = Leaf {
            sl: self,
            pool,
            t,
            members,
            fixed,
            wb,
            wbmax,
            threshold,
            best: None,
            pending: Vec::new(),
            matched: HashMap::new(),
            scratch: Scratch::default(),
            simulated: HashMap::new(),
            zero_from: HashMap::new(),
            deadline,
            timed_out: false,
            sims: 0,
            nodes: 0,
            wbn: [i64::MIN / 4; 5],
            wbo: [i64::MIN / 4; 5],
            gn: [[0f64; 5]; 5],
            go: [[0f64; 5]; 5],
            recn: [0; 5],
            has_o: [false; 5],
            n: self.n,
            seeded: HashMap::new(),
            count: LeafCounts::default(),
        };
        for i in 0..5 {
            let m = members[i];
            for c in 0..cls[i].len() {
                let w = lf.wb[i][c];
                if w <= i64::MIN / 8 {
                    continue;
                }
                let g: [f64; 5] = std::array::from_fn(|k| self.contrib[m][c][k].gain);
                match self.fine.life[m][c] {
                    LifeKind::Other => {
                        lf.has_o[i] = true;
                        lf.wbo[i] = lf.wbo[i].max(w);
                        for k in 0..5 {
                            lf.go[i][k] = lf.go[i][k].max(g[k]);
                        }
                    }
                    kind => {
                        lf.wbn[i] = lf.wbn[i].max(w);
                        for k in 0..5 {
                            lf.gn[i][k] = lf.gn[i][k].max(g[k]);
                        }
                        if let LifeKind::Recovery(r) = kind {
                            lf.recn[i] = lf.recn[i].max(r);
                        }
                    }
                }
            }
        }
        if self.life_bound && !lf.leaf_open(&gmax) {
            return Ok((None, false));
        }
        // a first candidate: the best-bound order with each slot's class of largest linearised gain
        {
            let (_, o0) = orders[0];
            let mut pos = [0usize; 5];
            for (k, &s) in o0.iter().enumerate() {
                pos[s] = k;
            }
            let s2max = self.a0 + (0..5).map(|i| gmax[i][pos[i]]).sum::<f64>();
            let mut cs = [0usize; 5];
            for i in 0..5 {
                let mut bestv = f64::MIN;
                for c in 0..cls[i].len() {
                    let g = self.contrib[members[i]][c][pos[i]].gain;
                    let v = lf.wb[i][c] as f64 * s2max + g * s1max as f64;
                    if v > bestv {
                        bestv = v;
                        cs[i] = c;
                    }
                }
            }
            if lf.matching(cs).is_none() {
                cs = [0; 5];
            }
            lf.consider(o0, cs, pos)?;
            lf.flush(true)?;
        }
        for &(bound, o) in &orders {
            if lf.timed_out || lf.sc(bound) < lf.cutoff() {
                break;
            }
            let mut pos = [0usize; 5];
            for (k, &s) in o.iter().enumerate() {
                pos[s] = k;
            }
            let rest2: [f64; 6] = {
                let mut r = [0f64; 6];
                for i in (0..5).rev() {
                    r[i] = r[i + 1] + gmax[i][pos[i]];
                }
                r
            };
            let rest1: [i64; 6] = {
                let mut r = [0i64; 6];
                for i in (0..5).rev() {
                    r[i] = r[i + 1] + lf.wbmax[i];
                }
                r
            };
            let mut rest = Rests { w: rest1, g: rest2, wn: [0; 6], gn: [0f64; 6] };
            for i in (0..5).rev() {
                rest.wn[i] = rest.wn[i + 1].saturating_add(lf.wbn[i]);
                rest.gn[i] = rest.gn[i + 1] + lf.gn[i][pos[i]];
            }
            let mut cs = [0usize; 5];
            if self.life_bound && !lf.order_open(pos, &rest) {
                continue;
            }
            lf.dfs(o, pos, 0, fixed, self.a0, &rest, false, &mut cs)?;
            lf.flush(false)?;
        }
        lf.flush(true)?;
        stats.orders += lf.sims;
        let c = lf.count;
        stats.seed_sims += c.seed_sims;
        stats.early_stops += c.early_stops;
        stats.seeds_saved += c.seeds_saved;
        stats.prefix_frames_saved += c.prefix_frames_saved;
        stats.bound_violations += c.violations;
        stats.class_choices += c.class_choices;
        stats.candidates += c.candidates;
        let timed_out = lf.timed_out;
        Ok((lf.best.map(|(c, score)| LeafBest { score, power: c.power, snaps: c.snaps, order: c.order }), timed_out))
    }
}
#[cfg(test)]
mod additive_drift_tests {
    use super::*;
    use ournotes_sim::live::score::{JUST, LiveScoreCalculator};

    #[test]
    fn additive_margin_covers_budgeted_judgement_gain_in_real_score_chain() {
        let coef = Coef { k: vec![1.0], z: vec![1.0], max_jp: vec![1.0], ..Default::default() };
        let sensitivity = factor_error_sensitivity(&coef, 1.5).unwrap();
        let settings = LiveScoreSettings {
            score_adjustment_factor: 1.0,
            life_onus_factor: 1.0,
            note_factor_percent: HashMap::from([(1, 100)]),
            judgement_score_factor_percent: HashMap::from([(1, 150), (2, 100)]),
        };
        // The baseline coefficient sees Perfect only; a budgeted converter can
        // produce Just. Exercise the native binary32 chain near integer floors.
        let delta = 0.04;
        for power in [100_003, 1_000_000] {
            let calc = LiveScoreCalculator::new(power, 5, 1, &settings, 1.0, 1.0, None);
            for skill in [1.0, 3.0, 16.0] {
                let gains = skill + 0.5 * (1.0 + skill);
                let (base, _, chain) =
                    additive_joint_envelope(1.0, 1.0 + gains, delta, 1000.0, sensitivity, 0.0).unwrap();
                let actual =
                    calc.note_score_core(1000, 1, JUST, 1.0, (1.0 + skill + delta) as f32, 1.0).unwrap() as i64;
                let cap = ub(power as i64, base + gains, chain);
                assert!(cap >= actual);
                assert!(cap < ub(power as i64, 1.0 + gains, delta));
                // Charging delta against a0 alone misses the converted note's
                // larger judgement percentage and fails even this one-note case.
                assert!(ub(power as i64, 1.0 + delta + gains, chain) < actual);
            }
        }
    }

    #[test]
    fn additive_refinement_falls_back_outside_its_numeric_certificate() {
        assert_eq!(additive_joint_envelope(1.0, 2.0, 0.5, 100.0, 1.0, 0.0), None);
        assert_eq!(additive_joint_envelope(1.0, 2.0, 0.01, 1_000_000.0, 1.0, 0.0), None);
        assert_eq!(additive_joint_envelope(1.0, 2.0, 0.01, 100.0, f64::INFINITY, 0.0), None);
        let coef = Coef { k: vec![1.0], z: vec![-1.0], ..Default::default() };
        assert_eq!(factor_error_sensitivity(&coef, 1.5), None);
    }
}
