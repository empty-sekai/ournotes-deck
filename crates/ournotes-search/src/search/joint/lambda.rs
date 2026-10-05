//! Member-additive bound of the position-mean cheap envelope.
//!
//! A team with power `P` and position-mean envelope coefficient `T = A0 + sum of its pairs' mean gains` has a mean
//! score over the performance orders of at most `P * min(T, G) * (1 + eps)` (module documentation of `joint`). For
//! every `lambda > 0`, `P * T <= (P + lambda * T)^2 / (4 * lambda)` because `(P - lambda * T)^2 >= 0`, and
//! `P + lambda * T = lambda * A0 + sum over the five (member, Snap) pairs of (p + lambda * g)` is additive. Each pair
//! term is at most the largest `p + lambda * g` over the Pareto-optimal `(g, p)` points of its member's choices, and
//! each slot still to fill takes the largest such term of one character not yet in the team, a distinct character
//! per slot. Snap uniqueness, required members and the order of the traversal are relaxed, which only enlarges the
//! completion set. Power and gain stay bound to the same pair, which the separate maxima of the cheap relaxation
//! give up. The bound holds for every `lambda`; a golden-section search over `ln(sqrt(lambda))` only tightens it
//! (`C(lambda) / sqrt(lambda)` is convex in `sqrt(lambda)`, hence unimodal in its logarithm).
use super::*;

/// Golden-section steps of the `lambda` search.
const STEPS: usize = 24;
/// Half width of the search interval in `ln(sqrt(lambda))` around the separate-maxima optimum.
const SPAN: f64 = 4.0;

/// Pareto-optimal `(gain, power)` points: no other point has both a gain and a power at least as large.
fn pareto(mut points: Vec<(f64, i64)>) -> Vec<(f64, i64)> {
    points.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut out = Vec::new();
    let mut best = i64::MIN;
    for (g, p) in points {
        if p > best {
            best = p;
            out.push((g, p));
        }
    }
    out
}

/// The largest `p + lambda * g` over the points, rounded up.
fn value(points: &[(f64, i64)], lambda: f64) -> f64 {
    points.iter().map(|&(g, p)| add_up(p as f64, (lambda * g).next_up())).fold(0.0, f64::max)
}

/// `min over lambda of C(lambda)^2 / (4 * lambda) * (1 + eps)`, rounded up, with
/// `C(lambda) = lambda * a0 + sum of the placed values + the `free` largest character values` at `lambda`.
pub(super) fn lambda_cap(
    a0: f64,
    eps: f64,
    placed: &[&[(f64, i64)]],
    characters: &[&[(f64, i64)]],
    free: usize,
) -> f64 {
    let cap = |lambda: f64| {
        let mut c = (lambda * a0).next_up();
        for points in placed {
            c = add_up(c, value(points, lambda));
        }
        let mut best: Vec<f64> = characters.iter().map(|points| value(points, lambda)).collect();
        if free < best.len() {
            best.select_nth_unstable_by(free, |a, b| b.total_cmp(a));
            best.truncate(free);
        }
        for v in best {
            c = add_up(c, v);
        }
        ((((c * c).next_up() / (4.0 * lambda)).next_up()) * (1.0 + eps).next_up()).next_up()
    };
    // Start at the optimum of the separate maxima: lambda = P / T.
    let max_g = |points: &[(f64, i64)]| points.iter().map(|e| e.0).fold(0.0, f64::max);
    let max_p = |points: &[(f64, i64)]| points.iter().map(|e| e.1).max().unwrap_or(0) as f64;
    let mut g_free: Vec<f64> = characters.iter().map(|c| max_g(c)).collect();
    let mut p_free: Vec<f64> = characters.iter().map(|c| max_p(c)).collect();
    g_free.sort_by(|a, b| b.total_cmp(a));
    p_free.sort_by(|a, b| b.total_cmp(a));
    let power = placed.iter().map(|c| max_p(c)).sum::<f64>() + p_free.iter().take(free).sum::<f64>();
    let gain = a0 + placed.iter().map(|c| max_g(c)).sum::<f64>() + g_free.iter().take(free).sum::<f64>();
    if !(power > 0.0 && gain > 0.0 && (power / gain).is_normal()) {
        return f64::INFINITY;
    }
    let centre = 0.5 * (power / gain).ln();
    let at = |u: f64| cap((2.0 * u).exp());
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (centre - SPAN, centre + SPAN);
    let (mut x1, mut x2) = (hi - ratio * (hi - lo), lo + ratio * (hi - lo));
    let (mut f1, mut f2) = (at(x1), at(x2));
    let mut best = f1.min(f2);
    for _ in 0..STEPS {
        if f1 <= f2 {
            hi = x2;
            (x2, f2) = (x1, f1);
            x1 = hi - ratio * (hi - lo);
            f1 = at(x1);
            best = best.min(f1);
        } else {
            lo = x1;
            (x1, f1) = (x2, f2);
            x2 = lo + ratio * (hi - lo);
            f2 = at(x2);
            best = best.min(f2);
        }
    }
    best
}

/// Pareto points `(gain, power)` of one member's or one character's choices.
type Points = Vec<(f64, i64)>;

/// The Pareto points of every member's choices and of every character's members, by leader profile, with the
/// envelope's `A0` and float margin: the member-additive bound module.
pub(super) struct LambdaTables {
    /// `member[profile][m]`, empty outside the domain.
    member: Vec<Vec<Points>>,
    /// `character[profile]`: (character id, points of all its members).
    character: Vec<Vec<(i64, Points)>>,
    /// The leader profile of every pool member.
    profile: Vec<usize>,
    a0: f64,
    eps: f64,
}

impl LambdaTables {
    /// None for PT objectives (score only) or beyond bounded table capacity.
    pub(super) fn compile(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        if b.points.is_some()
            || b.lead.len().saturating_mul(domain.members().len()).saturating_mul(domain.snaps().len() + 1) > 4_000_000
        {
            return None;
        }
        let mut member = Vec::with_capacity(b.lead.len());
        let mut character = Vec::with_capacity(b.lead.len());
        for profile in 0..b.lead.len() {
            let mut rows = vec![Vec::new(); pool.members.len()];
            let mut by_character = std::collections::BTreeMap::<i64, Vec<(f64, i64)>>::new();
            for &m in domain.members() {
                let points: Vec<_> = (0..=domain.snaps().len())
                    .map(|choice| {
                        let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                        // position-mean rows: every position holds the mean
                        (b.gains[m][choice][0], power)
                    })
                    .collect();
                rows[m] = pareto(points);
                by_character.entry(pool.members[m].character_id).or_default().extend_from_slice(&rows[m]);
            }
            member.push(rows);
            character.push(by_character.into_iter().map(|(c, points)| (c, pareto(points))).collect());
        }
        Some(Self { member, character, profile: b.profile.clone(), a0: b.a0, eps: b.eps })
    }
}

impl NodeBound for LambdaTables {
    fn name(&self) -> &'static str {
        "memberAdditive"
    }

    /// Every placed member counts with all its Snap choices, so placed Snaps only enlarge the covered set.
    fn node_upper(
        &self,
        pool: &Pool,
        p: &PhysicalDeck,
        depth: usize,
        _snaps_placed: bool,
        orders: &[([usize; 5], u128)],
    ) -> Option<i128> {
        let profile = self.profile[p.members[2]];
        let placed: Vec<&[(f64, i64)]> =
            SLOTS[..depth].iter().map(|&s| self.member[profile][p.members[s]].as_slice()).collect();
        let taken: Vec<i64> = SLOTS[..depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let characters: Vec<&[(f64, i64)]> = self.character[profile]
            .iter()
            .filter(|(c, _)| !taken.contains(c))
            .map(|(_, points)| points.as_slice())
            .collect();
        let cap = lambda_cap(self.a0, self.eps, &placed, &characters, 5 - depth);
        if !cap.is_finite() || cap >= i64::MAX as f64 {
            return None;
        }
        let mass: u128 = orders.iter().map(|(_, w)| *w).sum();
        (cap.ceil() as i128).checked_mul(i128::try_from(mass).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random values for the tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn points(&mut self, n: usize) -> Vec<(f64, i64)> {
            (0..n).map(|_| ((self.next() % 1000) as f64 / 997.0, (self.next() % 5000) as i64)).collect()
        }
    }

    #[test]
    fn pareto_keeps_exactly_the_undominated_points() {
        let points = vec![(1.0, 5), (2.0, 3), (1.5, 3), (2.0, 4), (0.5, 9), (0.5, 8), (3.0, 1)];
        let kept = pareto(points.clone());
        assert_eq!(kept, vec![(3.0, 1), (2.0, 4), (1.0, 5), (0.5, 9)]);
        for &(g, p) in &points {
            assert!(kept.iter().any(|&(kg, kp)| kg >= g && kp >= p));
        }
    }

    #[test]
    fn lambda_cap_covers_every_completion() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for case in 0..200 {
            let placed_count = case % 4;
            let free = (1 + case % 3).min(5 - placed_count);
            let placed: Vec<Vec<(f64, i64)>> = (0..placed_count).map(|_| rng.points(3)).collect();
            let characters: Vec<Vec<(f64, i64)>> = (0..free + 2).map(|_| rng.points(4)).collect();
            let (a0, eps) = (0.25 + (case % 7) as f64 * 0.1, 1e-9);
            let placed_ref: Vec<&[(f64, i64)]> = placed.iter().map(|v| v.as_slice()).collect();
            let char_ref: Vec<&[(f64, i64)]> = characters.iter().map(|v| v.as_slice()).collect();
            let cap = lambda_cap(a0, eps, &placed_ref, &char_ref, free);
            // Every completion: one point per placed slot, `free` distinct characters with one point each.
            let mut best = 0.0f64;
            let mut stack = vec![(0usize, 0i64, a0)];
            while let Some((i, p, t)) = stack.pop() {
                if i < placed.len() {
                    for &(g, q) in &placed[i] {
                        stack.push((i + 1, p + q, t + g));
                    }
                    continue;
                }
                for subset in 0u32..(1 << characters.len()) {
                    if subset.count_ones() as usize != free {
                        continue;
                    }
                    let chosen: Vec<usize> = (0..characters.len()).filter(|&c| subset & (1 << c) != 0).collect();
                    let mut picks = vec![(p, t)];
                    for &c in &chosen {
                        picks = picks
                            .iter()
                            .flat_map(|&(pp, tt)| characters[c].iter().map(move |&(g, q)| (pp + q, tt + g)))
                            .collect();
                    }
                    for (pp, tt) in picks {
                        best = best.max(pp as f64 * tt);
                    }
                }
            }
            assert!(cap >= best, "case {case}: cap {cap} below completion {best}");
            // and it is not looser than the separate maxima
            let sep_p: i64 = placed.iter().map(|v| v.iter().map(|e| e.1).max().unwrap()).sum::<i64>() + {
                let mut v: Vec<i64> = characters.iter().map(|c| c.iter().map(|e| e.1).max().unwrap()).collect();
                v.sort_unstable_by(|a, b| b.cmp(a));
                v.iter().take(free).sum::<i64>()
            };
            let sep_t = a0 + placed.iter().map(|v| v.iter().map(|e| e.0).fold(0.0, f64::max)).sum::<f64>() + {
                let mut v: Vec<f64> = characters.iter().map(|c| c.iter().map(|e| e.0).fold(0.0, f64::max)).collect();
                v.sort_by(|a, b| b.total_cmp(a));
                v.iter().take(free).sum::<f64>()
            };
            assert!(cap <= sep_p as f64 * sep_t * (1.0 + 1e-6), "case {case}: cap {cap} above separate maxima");
        }
    }
}
