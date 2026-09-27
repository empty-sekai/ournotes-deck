//! Exhaustive enumeration of every legal deck, for validating the search on small pools.
//!
//! It evaluates every (member set, leader, snap assignment) with the full deck-power path, keeps the best
//! representative of each member set under the canonical order and sorts the sets. It shares no pruning, bound,
//! decomposition or Top-K code with the search; the per-deck evaluation is the crate's regular one.

use std::collections::HashMap;

use crate::error::Error;
use crate::search::pool::{Deck, Pool};
use crate::search::{Objective, RankedDeck, SearchRequest, live_model, objective_song, resolve_allowed};

/// A visitor of one snap assignment (the snap of each slot).
type Visit<'a> = dyn FnMut(&[Option<usize>; 5]) -> Result<(), Error> + 'a;

/// Every legal deck evaluated; the best `k` member sets in canonical order.
pub fn brute_force(pool: &Pool, req: &SearchRequest) -> Result<(Vec<RankedDeck>, u64), Error> {
    let (allowed, snaps) = resolve_allowed(pool, &req.constraints)?;
    let (music_id, event, skip) = objective_song(pool, &req.objective)?;
    let live = matches!(req.objective, Objective::LiveScore { .. });
    let model = live_model(pool, &req.objective)?;
    let song = music_id.map(|id| pool.song(id)).transpose()?;
    let n = pool.members.len();
    let cand: Vec<usize> = (0..n).filter(|&i| allowed.members[i]).collect();
    let mut skip_cache: HashMap<i32, i32> = HashMap::new();
    let mut live_cache: HashMap<(i32, Vec<(i64, i64)>), i32> = HashMap::new();
    // key: (objective, power, sorted ids, leader id, snap ids) with "better" = larger objective and power, then
    // smaller ids / leader / snaps
    type Key = (i64, i64, [i64; 5], i64, [i64; 5], [usize; 5]);
    let better = |a: &Key, b: &Key| (b.0, b.1, a.2, a.3, a.4, a.5) < (a.0, a.1, b.2, b.3, b.4, b.5);
    let mut best: HashMap<[i64; 5], (Key, Deck, i32, Option<i32>)> = HashMap::new();
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
                    let score = if live {
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
                    let key: Key = (
                        score.unwrap_or(p) as i64,
                        p as i64,
                        ids,
                        pool.members[leader].id,
                        a.map(|s| s.map_or(i64::MAX, |i| pool.snaps[i].id)),
                        order,
                    );
                    let replace = match best.get(&ids) {
                        None => true,
                        Some((k, ..)) => better(&key, k),
                    };
                    if replace {
                        best.insert(ids, (key, deck, p, score));
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
    let mut all: Vec<(Key, Deck, i32, Option<i32>)> = best.into_values().collect();
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
        .map(|(_, d, p, score)| RankedDeck {
            members: d.members.map(|i| pool.members[i].id),
            snaps: d.snaps.map(|s| s.map(|i| pool.snaps[i].id)),
            performance_order: d.performance_order,
            power: p,
            score,
        })
        .collect();
    Ok((results, evaluated))
}
