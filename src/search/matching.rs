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
    let base = (ns + 1) as i128;
    let place = [base.pow(4), base.pow(3), base.pow(2), base, 1];
    let big = base.pow(5);
    // columns: ns snaps, then 5 "no snap" columns; minimise cost = -(w * big - penalty)
    let m = ns + 5;
    let cost = |i: usize, j: usize| -> i128 {
        if j < ns { -(w[i][j] as i128 * big - j as i128 * place[i]) } else { ns as i128 * place[i] }
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
    let mut total = 0i64;
    for j in 1..=m {
        if p[j] != 0 && j - 1 < ns {
            out[p[j] - 1] = Some(j - 1);
            total += w[p[j] - 1][j - 1];
        }
    }
    (total, out)
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
