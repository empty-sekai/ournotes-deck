//! Exhaustive enumeration of every legal deck, for validating the search on small pools.
//!
//! It evaluates every (member set, leader, snap assignment) with the full deck-power path (and, for a live score,
//! every performance order: with the per-order model, or with the whole-live simulation when snap skills count),
//! keeps the best representative of each member set under the canonical order and sorts the sets. With Gekisou on,
//! every deck-order is simulated on every seed, each run independent, and valued by the sum. It shares no pruning,
//! bound, decomposition, classification, seed-loop or Top-K code with the search; the per-deck evaluation is the
//! crate's regular one.

use std::collections::HashMap;

use crate::search::snaps::deck_performers;
use crate::search::{Objective, RankedDeck, SearchRequest, full_setup, live_model, objective_song, resolve_allowed};
use ournotes_sim::error::Error;
use ournotes_sim::pool::{Deck, Pool};

/// A visitor of one snap assignment (the snap of each slot).
type Visit<'a> = dyn FnMut(&[Option<usize>; 5]) -> Result<(), Error> + 'a;

/// Every legal deck evaluated; the best `k` member sets in canonical order.
pub fn brute_force(pool: &Pool, req: &SearchRequest) -> Result<(Vec<RankedDeck>, u64), Error> {
    if matches!(req.objective.inner(), Objective::LiveScore { .. }) {
        return Err(Error::Input("live oracle requires search::expectation::oracle; best-order enumeration is only brute_force_best_order_diagnostic".into()));
    }
    brute_force_best_order_diagnostic(pool, req)
}

/// Historical order-optimized diagnostic, not native expectation.
pub fn brute_force_best_order_diagnostic(pool: &Pool, req: &SearchRequest) -> Result<(Vec<RankedDeck>, u64), Error> {
    let (allowed, snaps) = resolve_allowed(pool, &req.constraints)?;
    let (song, event, skip) = objective_song(pool, &req.objective)?;
    let live = matches!(req.objective.inner(), Objective::LiveScore { .. });
    let model = live_model(pool, &req.objective)?;
    let full = full_setup(pool, &req.objective)?;
    let n = pool.members.len();
    let cand: Vec<usize> = (0..n).filter(|&i| allowed.members[i]).collect();
    let mut skip_cache: HashMap<i32, i32> = HashMap::new();
    let mut live_cache: HashMap<(i32, Vec<(i64, i64)>), i32> = HashMap::new();
    // key: (objective, power, sorted ids, leader id, snap ids) with "better" = larger objective and power, then
    // smaller ids / leader / snaps
    // Snap keys sort real IDs ascending, then empty slots.
    type Key = (i64, i64, [i64; 5], i64, [(bool, i64); 5], [usize; 5]);
    type Candidate = (Key, Deck, i32, Option<i32>, Option<Vec<i32>>);
    let better = |a: &Key, b: &Key| (b.0, b.1, a.2, a.3, a.4, a.5) < (a.0, a.1, b.2, b.3, b.4, b.5);
    let mut best: HashMap<[i64; 5], Candidate> = HashMap::new();
    let mut evaluated = 0u64;
    let mut set = Vec::with_capacity(5);
    fn subsets(cand: &[usize], start: usize, set: &mut Vec<usize>, out: &mut Vec<[usize; 5]>) {
        if set.len() == 5 {
            out.push([set[0], set[1], set[2], set[3], set[4]]);
            return;
        }
        for i in start..cand.len() {
            set.push(cand[i]);
            subsets(cand, i + 1, set, out);
            set.pop();
        }
    }
    let mut sets = Vec::new();
    subsets(&cand, 0, &mut set, &mut sets);
    for s in sets {
        let mut chars: Vec<i64> = s.iter().map(|&m| pool.members[m].character_id).collect();
        chars.sort_unstable();
        chars.dedup();
        if chars.len() < 5 || !allowed.required.iter().all(|r| s.contains(r)) {
            continue;
        }
        let mut ids = s.map(|m| pool.members[m].id);
        ids.sort_unstable();
        for &leader in &s {
            if allowed.leader.is_some_and(|l| l != leader) {
                continue;
            }
            let mut others: Vec<usize> = s.iter().copied().filter(|&m| m != leader).collect();
            others.sort_by_key(|&m| pool.members[m].id);
            let members = [others[0], others[1], leader, others[2], others[3]];
            let mut assign = [None; 5];
            // enumerate every injection of snaps (or none) into the five slots
            fn rec(
                slot: usize,
                snaps: &[usize],
                assign: &mut [Option<usize>; 5],
                f: &mut Visit<'_>,
            ) -> Result<(), Error> {
                if slot == 5 {
                    return f(assign);
                }
                assign[slot] = None;
                rec(slot + 1, snaps, assign, f)?;
                for &s in snaps {
                    if assign[..slot].contains(&Some(s)) {
                        continue;
                    }
                    assign[slot] = Some(s);
                    rec(slot + 1, snaps, assign, f)?;
                }
                assign[slot] = None;
                Ok(())
            }
            let mut f = |a: &[Option<usize>; 5]| -> Result<(), Error> {
                let deck0 = Deck { members, snaps: *a, performance_order: [0, 1, 2, 3, 4] };
                let p = pool.deck_power(&deck0, song.as_ref(), event)?.power();
                let mut order = [0usize, 1, 2, 3, 4];
                loop {
                    evaluated += 1;
                    let deck = Deck { members, snaps: *a, performance_order: order };
                    let mut seeds = None;
                    let score = if let Some(f) = full.as_ref().filter(|f| f.gk.is_some()) {
                        let v = f.seed_scores(pool.master, &deck_performers(pool, &deck)?, p)?;
                        let m = crate::search::mean_floor(&v);
                        seeds = Some(v);
                        Some(m)
                    } else if let Some(f) = &full {
                        Some(f.score(pool.master, &deck_performers(pool, &deck)?, p)?)
                    } else if live {
                        let perf: Vec<(i64, i64)> = order
                            .iter()
                            .map(|&s| {
                                (pool.members[members[s]].live_skill_id, pool.members[members[s]].live_skill_level)
                            })
                            .collect();
                        let key = (p, perf);
                        Some(match live_cache.get(&key) {
                            Some(&x) => x,
                            None => {
                                let m = model.as_ref().expect("live model");
                                let x = m.score(p, &m.commands(pool.master, &key.1)?);
                                live_cache.insert(key, x);
                                x
                            }
                        })
                    } else {
                        match &skip {
                            None => None,
                            Some(m) => Some(match skip_cache.get(&p) {
                                Some(&x) => x,
                                None => {
                                    let x = m.score(p)?;
                                    skip_cache.insert(p, x);
                                    x
                                }
                            }),
                        }
                    };
                    let value = match &seeds {
                        Some(v) => v.iter().map(|&x| x as i64).sum(),
                        None => score.unwrap_or(p) as i64,
                    };
                    let key: Key = (
                        value,
                        p as i64,
                        ids,
                        pool.members[leader].id,
                        a.map(|s| (s.is_none(), s.map_or(0, |i| pool.snaps[i].id))),
                        order,
                    );
                    let replace = match best.get(&ids) {
                        None => true,
                        Some((k, ..)) => better(&key, k),
                    };
                    if replace {
                        best.insert(ids, (key, deck, p, score, seeds));
                    }
                    if !live || !crate::search::live::next_permutation(&mut order) {
                        break;
                    }
                }
                Ok(())
            };
            rec(0, &snaps, &mut assign, &mut f)?;
        }
    }
    let mut all: Vec<Candidate> = best.into_values().collect();
    all.sort_by(|a, b| {
        if better(&a.0, &b.0) {
            std::cmp::Ordering::Less
        } else if better(&b.0, &a.0) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    all.truncate(req.k);
    let results = all
        .into_iter()
        .map(|(k, d, p, score, seeds)| RankedDeck {
            members: d.members.map(|i| pool.members[i].id),
            snaps: d.snaps.map(|s| s.map(|i| pool.snaps[i].id)),
            performance_order: d.performance_order,
            power: p,
            score,
            score_sum: seeds.as_ref().map(|_| k.0),
            seed_scores: seeds,
        })
        .collect();
    Ok((results, evaluated))
}
