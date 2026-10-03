//! Best snap assignment for five slots.
//!
//! Maximises the total snap term with each snap used at most once (a slot may stay empty, worth 0). Among
//! assignments of equal total the lexicographically smallest in slot order wins, comparing snap ids with "no snap"
//! after every snap. The tie-break is folded into the weights (`w * B - rank penalty`, with a penalty that is a
//! base-`n+1` number of the slot ranks and `B` above every penalty), which makes the optimum unique; a Hungarian
//! assignment then finds it exactly.

/// `w[i][j]`: term of snap `j` (allowed snaps sorted by id) in slot `i`. Returns the total and the snap of each slot.
pub(crate) fn best_assignment(w: [&[i64]; 5]) -> (i64, [Option<usize>; 5]) {
    let ns = w[0].len();
    if ns == 0 {
        return (0, [None; 5]);
    }
    let out = solve(ns, |i, j| if j < ns { Some(w[i][j]) } else { Some(0) });
    let total = (0..5).map(|i| out[i].map_or(0, |j| w[i][j])).sum();
    (total, out)
}

pub(crate) type PowerAssignment = (i64, [Option<usize>; 5]);

/// Exact residual assignment for at most two rows, with optional empty slots.
/// At most one real resource conflicts with the other row, so its two best
/// choices suffice. None can be shared. Ties here do not certify physical Top-K.
pub(crate) fn best_small_assignment(w: &[&[i64]]) -> (i64, Vec<Option<usize>>) {
    assert!(w.len() <= 2);
    if w.is_empty() {
        return (0, Vec::new());
    }
    let choices: Vec<_> = w
        .iter()
        .map(|row| {
            let mut best = vec![(0, None)];
            for (j, &value) in row.iter().enumerate() {
                best.push((value, Some(j)));
                best.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                best.truncate(2);
            }
            best
        })
        .collect();
    if w.len() == 1 {
        return (choices[0][0].0, vec![choices[0][0].1]);
    }
    let mut best = (0, vec![None; 2]);
    for &(a, sa) in &choices[0] {
        for &(b, sb) in &choices[1] {
            if sa.is_some() && sa == sb {
                continue;
            }
            if a + b > best.0 {
                best = (a + b, vec![sa, sb]);
            }
        }
    }
    best
}

/// Canonical None-first Top-K assignments. Snap columns must be public-ID sorted.
/// Scanning a resource once makes future extensions depend only on occupied slots;
/// retaining K partial bindings per mask preserves both value and lexicographic order.
pub(crate) fn best_k_assignments(w: [&[i64]; 5], k: usize) -> Vec<PowerAssignment> {
    let mut states: Vec<Vec<PowerAssignment>> = vec![Vec::new(); 32];
    states[0].push((0, [None; 5]));
    let rank = |a: &PowerAssignment, b: &PowerAssignment| b.0.cmp(&a.0).then(a.1.cmp(&b.1));
    for snap in 0..w[0].len() {
        let mut next = states.clone();
        for (mask, rows) in states.iter().enumerate() {
            for slot in 0..5 {
                if mask & (1 << slot) != 0 {
                    continue;
                }
                for &(power, binding) in rows {
                    let mut binding = binding;
                    binding[slot] = Some(snap);
                    next[mask | (1 << slot)].push((power + w[slot][snap], binding));
                }
            }
        }
        for rows in &mut next {
            rows.sort_unstable_by(rank);
            rows.truncate(k);
        }
        states = next;
    }
    let mut rows: Vec<_> = states.into_iter().flatten().collect();
    rows.sort_unstable_by(rank);
    rows.truncate(k);
    rows
}

#[cfg(test)]
mod canonical_frontier_tests {
    use super::{PowerAssignment, best_k_assignments};
    #[test]
    fn small_residual_matches_hungarian_and_returns_a_legal_binding() {
        for columns in 0..12 {
            for offset in 0..7 {
                for count in 0..=2 {
                    let weights: [Vec<i64>; 5] = std::array::from_fn(|row| {
                        (0..columns)
                            .map(|s| if row < count { ((row * 3 + s * 7 + offset) % 11) as i64 - 5 } else { 0 })
                            .collect()
                    });
                    let rows: Vec<_> = weights[..count].iter().map(|v| v.as_slice()).collect();
                    let (value, binding) = super::best_small_assignment(&rows);
                    assert_eq!(value, super::best_assignment(weights.each_ref().map(|v| v.as_slice())).0);
                    assert_eq!(
                        value,
                        binding.iter().enumerate().map(|(row, s)| s.map_or(0, |s| weights[row][s])).sum::<i64>()
                    );
                    assert!(binding.len() < 2 || binding[0].is_none() || binding[0] != binding[1]);
                }
            }
        }
    }
    #[test]
    fn resource_dp_matches_exhaustive_bindings_with_empty_negative_and_tied_edges() {
        for n in 0..=4 {
            let weights: [Vec<i64>; 5] =
                std::array::from_fn(|slot| (0..n).map(|s| ((slot * 7 + s * 3) % 9) as i64 - 4).collect());
            let mut all = Vec::new();
            fn enumerate(slot: usize, w: &[Vec<i64>; 5], b: &mut [Option<usize>; 5], all: &mut Vec<PowerAssignment>) {
                if slot == 5 {
                    all.push(((0..5).map(|i| b[i].map_or(0, |s| w[i][s])).sum(), *b));
                    return;
                }
                b[slot] = None;
                enumerate(slot + 1, w, b, all);
                for s in 0..w[0].len() {
                    if b[..slot].contains(&Some(s)) {
                        continue;
                    }
                    b[slot] = Some(s);
                    enumerate(slot + 1, w, b, all);
                }
            }
            enumerate(0, &weights, &mut [None; 5], &mut all);
            all.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for k in [1, 3, 100] {
                assert_eq!(best_k_assignments(weights.each_ref().map(|v| v.as_slice()), k), all[..k.min(all.len())]);
            }
        }
    }
}

/// [`best_assignment`] with slot `i` limited to the snaps `j` where `allowed[i][j]`, and to "no snap" only when
/// `none_ok[i]`; the same tie-break among the assignments that respect the limits. `None` when none does.
pub(crate) fn constrained_assignment(
    w: [&[i64]; 5],
    allowed: [&[bool]; 5],
    none_ok: [bool; 5],
) -> Option<(i64, [Option<usize>; 5])> {
    let ns = w[0].len();
    let ok = |i: usize, j: usize| if j < ns { allowed[i][j] } else { none_ok[i] };
    let out = solve(ns, |i, j| {
        if !ok(i, j) {
            None
        } else if j < ns {
            Some(w[i][j])
        } else {
            Some(0)
        }
    });
    for i in 0..5 {
        if !ok(i, out[i].unwrap_or(ns)) {
            return None;
        }
    }
    let total = (0..5).map(|i| out[i].map_or(0, |j| w[i][j])).sum();
    Some((total, out))
}

/// Hungarian assignment of the five slots to `ns` snap columns and five "no snap" columns (`term(i, ns)` is the
/// "no snap" term, `None` a forbidden pair). Forbidden pairs cost more than any difference between two assignments
/// without them, so the result avoids them whenever an assignment without them exists.
fn solve(ns: usize, term: impl Fn(usize, usize) -> Option<i64>) -> [Option<usize>; 5] {
    let base = (ns + 1) as i128;
    let place = [base.pow(4), base.pow(3), base.pow(2), base, 1];
    let big = base.pow(5);
    let mut max_abs = 0i128;
    for i in 0..5 {
        for j in 0..=ns {
            if let Some(v) = term(i, j) {
                max_abs = max_abs.max((v as i128).abs());
            }
        }
    }
    let forbidden = (max_abs + 2) * big * 16;
    // columns: ns snaps, then 5 "no snap" columns; minimise cost = -(w * big - penalty)
    let m = ns + 5;
    let cost = |i: usize, j: usize| -> i128 {
        let (jj, penalty) = if j < ns { (j, j as i128 * place[i]) } else { (ns, ns as i128 * place[i]) };
        match term(i, jj) {
            None => forbidden,
            Some(v) => -(v as i128 * big - penalty),
        }
    };
    // Hungarian algorithm (rows 1..=5, columns 1..=m, potentials u, v).
    const INF: i128 = i128::MAX / 4;
    let n = 5;
    let mut u = vec![0i128; n + 1];
    let mut v = vec![0i128; m + 1];
    let mut p = vec![0usize; m + 1];
    let mut way = vec![0usize; m + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![INF; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = INF;
            let mut j1 = 0usize;
            for j in 1..=m {
                if !used[j] {
                    let cur = cost(i0 - 1, j - 1) - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = [None; 5];
    for j in 1..=m {
        if p[j] != 0 && j - 1 < ns {
            out[p[j] - 1] = Some(j - 1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute(w: [&[i64]; 5]) -> (i64, [Option<usize>; 5]) {
        let ns = w[0].len();
        let mut best: Option<(i64, [usize; 5])> = None;
        let none = usize::MAX;
        let mut cur = [none; 5];
        fn rec(i: usize, w: [&[i64]; 5], ns: usize, cur: &mut [usize; 5], best: &mut Option<(i64, [usize; 5])>) {
            if i == 5 {
                let t: i64 = (0..5).map(|k| if cur[k] == usize::MAX { 0 } else { w[k][cur[k]] }).sum();
                let better = match best {
                    None => true,
                    Some((bt, ba)) => t > *bt || (t == *bt && *cur < *ba),
                };
                if better {
                    *best = Some((t, *cur));
                }
                return;
            }
            for j in (0..ns).chain(std::iter::once(usize::MAX)) {
                if j != usize::MAX && cur[..i].contains(&j) {
                    continue;
                }
                cur[i] = j;
                rec(i + 1, w, ns, cur, best);
            }
            cur[i] = usize::MAX;
        }
        rec(0, w, ns, &mut cur, &mut best);
        let (t, a) = best.unwrap();
        (t, a.map(|j| if j == usize::MAX { None } else { Some(j) }))
    }

    fn brute_constrained(
        w: [&[i64]; 5],
        allowed: &[Vec<bool>],
        none_ok: [bool; 5],
    ) -> Option<(i64, [Option<usize>; 5])> {
        let ns = w[0].len();
        let mut best: Option<(i64, [usize; 5])> = None;
        let mut cur = [usize::MAX; 5];
        #[allow(clippy::too_many_arguments)]
        fn rec(
            i: usize,
            w: [&[i64]; 5],
            ns: usize,
            allowed: &[Vec<bool>],
            none_ok: [bool; 5],
            cur: &mut [usize; 5],
            best: &mut Option<(i64, [usize; 5])>,
        ) {
            if i == 5 {
                let t: i64 = (0..5).map(|k| if cur[k] == usize::MAX { 0 } else { w[k][cur[k]] }).sum();
                if best.is_none_or(|(bt, ba)| t > bt || (t == bt && *cur < ba)) {
                    *best = Some((t, *cur));
                }
                return;
            }
            for j in (0..ns).chain(std::iter::once(usize::MAX)) {
                let ok = if j == usize::MAX { none_ok[i] } else { allowed[i][j] && !cur[..i].contains(&j) };
                if !ok {
                    continue;
                }
                cur[i] = j;
                rec(i + 1, w, ns, allowed, none_ok, cur, best);
            }
            cur[i] = usize::MAX;
        }
        rec(0, w, ns, allowed, none_ok, &mut cur, &mut best);
        best.map(|(t, a)| (t, a.map(|j| if j == usize::MAX { None } else { Some(j) })))
    }

    #[test]
    fn constrained_matches_brute_force() {
        let mut s = 0x0fed_cba9_8765_4321u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..600 {
            let ns = (next() % 7) as usize;
            let rows: Vec<Vec<i64>> = (0..5).map(|_| (0..ns).map(|_| (next() % 6) as i64 - 1).collect()).collect();
            let allowed: Vec<Vec<bool>> = (0..5).map(|_| (0..ns).map(|_| next() % 3 != 0).collect()).collect();
            let none_ok = [0, 1, 2, 3, 4].map(|_| next() % 2 == 0);
            let w = [&rows[0][..], &rows[1][..], &rows[2][..], &rows[3][..], &rows[4][..]];
            let a = [&allowed[0][..], &allowed[1][..], &allowed[2][..], &allowed[3][..], &allowed[4][..]];
            assert_eq!(constrained_assignment(w, a, none_ok), brute_constrained(w, &allowed, none_ok), "{rows:?}");
        }
    }

    #[test]
    fn matches_brute_force() {
        let mut s = 0x1234_5678_9abc_def0u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..300 {
            let ns = (next() % 7) as usize;
            let rows: Vec<Vec<i64>> = (0..5).map(|_| (0..ns).map(|_| (next() % 6) as i64 - 1).collect()).collect();
            let w = [&rows[0][..], &rows[1][..], &rows[2][..], &rows[3][..], &rows[4][..]];
            assert_eq!(best_assignment(w), brute(w), "{rows:?}");
        }
    }
}
