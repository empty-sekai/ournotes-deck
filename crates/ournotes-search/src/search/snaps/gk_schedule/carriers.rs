//! The envelopes of decks with few Gekisou combo carriers and the window gains they read.

use super::*;

/// The linear envelope of decks with at most `n < 5` carriers: `A0`, `global` and the gain of every member, class
/// and position (`gains[m][c][k]`, empty for members that are not allowed).
#[derive(Clone, Debug)]
pub(crate) struct CarrierLevel {
    pub(crate) a0: f64,
    pub(crate) global: f64,
    pub(crate) gains: Vec<Vec<[f64; 5]>>,
    /// The level's coefficient of each entry (`Coef::k` with the level's combo factor).
    pub(crate) k: Vec<f64>,
}

/// `terms[e] = (pre, combo, rank)` of each entry, so that the pool-wide coefficient is
/// `pre * G_e * combo / cnc * rank`; `factors[n]` the combo factor bound of decks with at most `n` carriers and
/// `sums[n]` their combo count bounds (`GkFactors::sums`, `n = 5` for every deck). A level whose factor and count
/// bounds equal the next one's (or the pool-wide ones) is `None`: the next level covers it unchanged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn carrier_level_envelopes(
    coef: &Coef,
    terms: &[(f64, f64, f64)],
    cnc: f64,
    pool_factor: &[f64],
    factors: &[Vec<f64>],
    gc: &GkCombo,
    sums: &[Vec<f64>],
    members: &[usize],
    contrib: &[Vec<[Contrib; 5]>],
) -> Vec<Option<CarrierLevel>> {
    let ne = coef.times.len();
    debug_assert!(terms.len() == ne && pool_factor.len() == ne && factors.iter().all(|g| g.len() == ne));
    let mut out: Vec<Option<CarrierLevel>> = vec![None; factors.len()];
    for n in 0..factors.len() {
        let g = &factors[n];
        let next = factors.get(n + 1).map_or(pool_factor, |v| &v[..]);
        if g == next && same_bits(&sums[n], &sums[n + 1]) {
            continue;
        }
        let (ks, pc, pj) = factor_sums(coef, terms, cnc, |e| g[e]);
        let a0 = pc[ne];
        let mut best = [0f64; 5];
        let mut gains: Vec<Vec<[f64; 5]>> = vec![Vec::new(); contrib.len()];
        for &m in members {
            gains[m] = contrib[m]
                .iter()
                .map(|arr| {
                    std::array::from_fn(|k| {
                        let reads = RampReads { gc, sums: &sums[n], times: &coef.times };
                        let v = window_gain(&arr[k], &pc, &pj, Some(reads));
                        best[k] = best[k].max(v);
                        v
                    })
                })
                .collect();
        }
        let global = best.iter().fold(a0, |sum, &g| sum + g);
        out[n] = Some(CarrierLevel { a0, global, gains, k: ks });
    }
    out
}

/// The coefficients of the combo factor bound `g` (`terms` as in `carrier_level_envelopes`) and their prefix sums
/// `(k, pc, pj)` as in `Coef`, by the operations of the pool-wide coefficients.
pub(super) fn factor_sums(
    coef: &Coef,
    terms: &[(f64, f64, f64)],
    cnc: f64,
    g: impl Fn(usize) -> f64,
) -> (Vec<f64>, Vec<f64>, [Vec<f64>; 4]) {
    let ne = terms.len();
    let mut pc = vec![0f64; ne + 1];
    let mut pj = [vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1]];
    let mut ks = Vec::with_capacity(ne);
    for (e, &(pre, combo, rank)) in terms.iter().enumerate() {
        // the same operations as the pool-wide coefficient, with a factor at most its `G_e`
        let k = pre * g(e) * combo / cnc * rank;
        ks.push(k);
        pc[e + 1] = pc[e] + coef.z[e] * k * coef.max_jp[e];
        for j in 0..4 {
            pj[j][e + 1] = pj[j][e] + coef.z[e] * k * coef.jp[e][j];
        }
    }
    (ks, pc, pj)
}

/// What a combo ramp window reads at an entry: the combo count bounds `sums` of some decks (`GkCombo::fill`'s) at
/// the count `GkCombo::ramp_count` finds for the entry's chart time (`times`).
#[derive(Clone, Copy)]
pub(crate) struct RampReads<'a> {
    pub(crate) gc: &'a GkCombo,
    pub(crate) sums: &'a [f64],
    pub(crate) times: &'a [i32],
}

impl RampReads<'_> {
    /// The note factor ramp window `w` (with its ramp `ramp`) adds at entry `e`.
    pub(crate) fn factor(&self, w: &Window, ramp: &ComboRamp, e: usize) -> f64 {
        ramp_factor(w, ramp, self.gc.ramp_count(self.sums, &ramp.starts, e, self.times[e] as i64))
    }
}

/// The gain of a member, class and position (its windows and conversion budget) under the prefix sums of some
/// coefficients, by the operations of the pool-wide gains. The conversion budget read the pool-wide coefficients,
/// never smaller. With combo count bounds `reads`, a combo ramp window adds its factor at each entry's count
/// (`ramp_factor`), as the candidate cap reads it.
pub(crate) fn window_gain(c: &Contrib, pc: &[f64], pj: &[Vec<f64>; 4], reads: Option<RampReads<'_>>) -> f64 {
    let mut v = 0f64;
    for x in &c.windows {
        let (lo, hi) = (x.lo as usize, x.hi as usize);
        match reads.filter(|_| x.ramp != 0) {
            Some(reads) => {
                let ramp = &c.ramps[x.ramp as usize - 1];
                for e in lo..hi {
                    v += reads.factor(x, ramp, e) * (pc[e + 1] - pc[e]);
                }
            }
            None => v += x.note * (pc[hi] - pc[lo]),
        }
        for j in 0..4 {
            if x.judge[j] != 0.0 {
                v += x.judge[j] * (pj[j][hi] - pj[j][lo]);
            }
        }
    }
    v + c.budget
}

/// Whether two count bounds hold the same values.
fn same_bits(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// Whether member `m`'s class `c` brings Gekisou combo bonus windows (the windows do not depend on the position).
pub(crate) fn is_carrier_class(contrib: &[Vec<[Contrib; 5]>], m: usize, c: usize) -> bool {
    contrib[m].get(c).is_some_and(|arr| !arr[0].cb.is_empty())
}
