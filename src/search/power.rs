//! Exact Top-K of deck power, and of the live scores built on it.
//!
//! For each leader card, the four other members are chosen by a depth-first search over characters with a
//! prefix-sum bound; each complete member set gets its exact leader terms and its best snap assignment (with snap
//! skills: its best snap placement and performance order, searched together). See `docs/search.md` for the
//! admissibility argument.

use std::time::Instant;

use crate::error::Error;
use crate::search::live::LiveCtx;
use crate::search::matching::best_assignment;
use crate::search::pool::Pool;
use crate::search::snaps::SnapLive;
use crate::search::tables::Tables;
use crate::search::topk::{Entry, NO_SNAP, TopK};

/// Members and snaps a search may use.
#[derive(Clone, Debug)]
pub(crate) struct Allowed {
    pub members: Vec<bool>,
    /// Required member indexes (besides the leader, if the leader is one of them).
    pub required: Vec<usize>,
    /// Fixed leader index.
    pub leader: Option<usize>,
}

/// Work counters of a search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct PowerStats {
    pub leaders: u64,
    pub nodes: u64,
    /// Complete member sets reached.
    pub leaves: u64,
    /// Snap assignments solved.
    pub matchings: u64,
    /// Deck-orders evaluated exactly (live score): orders of the per-order model, or whole-live simulations with
    /// snap skills (with Gekisou on: deck-orders with at least one seed simulated).
    pub orders: u64,
    /// With snap skills: class choices (an order and a class for each slot) reached in the class search, and
    /// candidates queued for simulation.
    pub class_choices: u64,
    pub candidates: u64,
    /// With Gekisou on: whole-live simulations of one seed.
    pub seed_sims: u64,
    /// With Gekisou on: candidates dropped after some of their seeds, and the seed simulations this saved.
    pub early_stops: u64,
    pub seeds_saved: u64,
    /// With Gekisou on: play frames not simulated again, thanks to the shared start of the seed runs.
    pub prefix_frames_saved: u64,
    /// With Gekisou on: seed scores above their candidate's per-seed bound (always 0 unless the bound is wrong).
    pub bound_violations: u64,
}

/// The live objective of a search, if any.
#[derive(Clone, Copy)]
pub(crate) enum LiveMode<'a> {
    /// Deck power or skip score.
    None,
    /// Live score with live skills only (per-order model).
    Order(&'a LiveCtx<'a>),
    /// Live score with snap skills (whole-live simulation).
    Snaps(&'a SnapLive<'a>),
}

pub(crate) struct PowerSearch<'a, 'm> {
    pub pool: &'a Pool<'m>,
    pub t: &'a Tables<'m>,
    pub allowed: &'a Allowed,
    pub top: TopK,
    pub deadline: Option<Instant>,
    pub timed_out: bool,
    pub stats: PowerStats,
    pub error: Option<Error>,
    pub live: LiveMode<'a>,
    /// With snap skills: the largest gain of each member at each position (`SnapLive::position_gains`).
    pub gains: Vec<[f64; 5]>,
}

/// With snap skills: what the gain bound of a leader's depth-first search reads.
struct LeaderGains {
    /// The largest gain at each position of the leader and the fixed members, and the sum of each one's largest.
    fixed_pos: [f64; 5],
    fixed_sum: f64,
    /// For each start `q` (up to the number of characters): the largest gain at each position over the cards of
    /// `chars[q..]`, and the four largest single-card gains of distinct characters there, descending.
    suf_pos: Vec<[f64; 5]>,
    suf_top: Vec<[f64; 4]>,
}

struct Leader {
    leader: usize,
    /// Characters available for the other four, best-first: (best u, cards sorted by u desc as (u, index)).
    chars: Vec<(i64, Vec<(i64, usize)>)>,
    /// prefix[q] = sum of chars[0..q].best
    prefix: Vec<i64>,
    /// Fixed members (other than the leader) and the sum of their u plus the leader's u.
    fixed: Vec<usize>,
    fixed_u: i64,
    picks: usize,
    bound: i64,
    gains: Option<LeaderGains>,
}

impl<'a, 'm> PowerSearch<'a, 'm> {
    fn u(&self, profile: usize, m: usize) -> i64 {
        self.t.a[m] + self.t.lead[profile][m] + self.t.wmax[m]
    }

    /// Whether a branch whose power is at most `power_bound` can still reach the Top-K.
    #[inline]
    fn open(&self, power_bound: i64) -> bool {
        let th = self.top.threshold();
        match self.live {
            LiveMode::None => power_bound >= th,
            LiveMode::Order(l) => th == i64::MIN || l.score_bound(power_bound) >= th,
            LiveMode::Snaps(l) => th == i64::MIN || l.score_bound(power_bound) >= th,
        }
    }

    fn prepare(&self, leader: usize) -> Option<Leader> {
        let pool = self.pool;
        let profile = self.t.profile_of[leader];
        let lc = pool.members[leader].character_id;
        let mut fixed: Vec<usize> = Vec::new();
        for &r in &self.allowed.required {
            if r == leader {
                continue;
            }
            let rc = pool.members[r].character_id;
            if rc == lc || fixed.iter().any(|&f| pool.members[f].character_id == rc) {
                return None;
            }
            fixed.push(r);
        }
        if fixed.len() > 4 {
            return None;
        }
        let fixed_chars: Vec<i64> = fixed.iter().map(|&f| pool.members[f].character_id).collect();
        let mut by_char: Vec<(i64, Vec<(i64, usize)>)> = Vec::new();
        let mut char_ids: Vec<i64> = Vec::new();
        for (m, v) in pool.members.iter().enumerate() {
            if !self.allowed.members[m] || m == leader || v.character_id == lc || fixed_chars.contains(&v.character_id)
            {
                continue;
            }
            let u = self.u(profile, m);
            match char_ids.iter().position(|&c| c == v.character_id) {
                Some(p) => by_char[p].1.push((u, m)),
                None => {
                    char_ids.push(v.character_id);
                    by_char.push((0, vec![(u, m)]));
                }
            }
        }
        for c in &mut by_char {
            c.1.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
            c.0 = c.1[0].0;
        }
        by_char.sort_by_key(|x| std::cmp::Reverse(x.0));
        let picks = 4 - fixed.len();
        if by_char.len() < picks {
            return None;
        }
        let mut prefix = vec![0i64; by_char.len() + 1];
        for (q, c) in by_char.iter().enumerate() {
            prefix[q + 1] = prefix[q] + c.0;
        }
        let fixed_u = self.u(profile, leader) + fixed.iter().map(|&f| self.u(profile, f)).sum::<i64>();
        let bound = fixed_u + prefix[picks];
        let gains = self.leader_gains(leader, &fixed, &by_char);
        Some(Leader { leader, chars: by_char, prefix, fixed, fixed_u, picks, bound, gains })
    }

    /// The gains of a leader's members and characters (with snap skills).
    fn leader_gains(&self, leader: usize, fixed: &[usize], chars: &[(i64, Vec<(i64, usize)>)]) -> Option<LeaderGains> {
        if !matches!(self.live, LiveMode::Snaps(_)) {
            return None;
        }
        let top = |g: &[f64; 5]| g.iter().copied().fold(0f64, f64::max);
        let mut fixed_pos = [0f64; 5];
        let mut fixed_sum = 0f64;
        for &m in std::iter::once(&leader).chain(fixed) {
            let g = self.gains[m];
            for k in 0..5 {
                fixed_pos[k] = fixed_pos[k].max(g[k]);
            }
            fixed_sum += top(&g);
        }
        let n = chars.len();
        let mut suf_pos = vec![[0f64; 5]; n + 1];
        let mut suf_top = vec![[0f64; 4]; n + 1];
        for q in (0..n).rev() {
            let mut pos = suf_pos[q + 1];
            let mut best = 0f64;
            for &(_, m) in &chars[q].1 {
                let g = self.gains[m];
                for k in 0..5 {
                    pos[k] = pos[k].max(g[k]);
                }
                best = best.max(top(&g));
            }
            let mut t = suf_top[q + 1];
            if best > t[3] {
                t[3] = best;
                let mut i = 3;
                while i > 0 && t[i] > t[i - 1] {
                    t.swap(i, i - 1);
                    i -= 1;
                }
            }
            suf_pos[q] = pos;
            suf_top[q] = t;
        }
        Some(LeaderGains { fixed_pos, fixed_sum, suf_pos, suf_top })
    }

    /// With snap skills: whether a node whose power is at most `power` can still reach the Top-K, from the gains of
    /// the members chosen (`sel_pos` at each position, `sel_sum` of each one's largest) and of the `r` members still to
    /// choose among `chars[q..]`.
    fn open_gain(&self, power: i64, g: &LeaderGains, q: usize, r: usize, sel_pos: [f64; 5], sel_sum: f64) -> bool {
        let LiveMode::Snaps(sl) = self.live else { return true };
        let th = self.top.threshold();
        if th == i64::MIN {
            return true;
        }
        let by_pos: f64 = (0..5).map(|k| sel_pos[k].max(g.suf_pos[q][k])).sum();
        let by_member = sel_sum + g.suf_top[q][..r.min(4)].iter().sum::<f64>();
        sl.gain_bound(power, by_pos.min(by_member)) >= th
    }

    pub fn run(&mut self) {
        let n = self.pool.members.len();
        if let LiveMode::Snaps(sl) = self.live {
            self.gains = (0..n).map(|m| sl.position_gains(m)).collect();
        }
        let mut leaders: Vec<Leader> = (0..n)
            .filter(|&l| self.allowed.members[l] && self.allowed.leader.is_none_or(|x| x == l))
            .filter_map(|l| self.prepare(l))
            .collect();
        leaders.sort_by(|x, y| y.bound.cmp(&x.bound).then(x.leader.cmp(&y.leader)));
        for l in &leaders {
            if !self.open(l.bound) {
                break;
            }
            self.stats.leaders += 1;
            let mut picked = Vec::with_capacity(4);
            picked.extend_from_slice(&l.fixed);
            let (sel_pos, sel_sum) = l.gains.as_ref().map_or(([0f64; 5], 0f64), |g| (g.fixed_pos, g.fixed_sum));
            self.dfs(l, 0, l.picks, l.fixed_u, &mut picked, sel_pos, sel_sum);
            if self.timed_out || self.error.is_some() {
                return;
            }
        }
    }

    fn tick(&mut self) -> bool {
        self.stats.nodes += 1;
        if self.stats.nodes % 256 == 0 {
            if let Some(d) = self.deadline {
                if Instant::now() >= d {
                    self.timed_out = true;
                }
            }
        }
        !self.timed_out && self.error.is_none()
    }

    #[allow(clippy::too_many_arguments)]
    fn dfs(
        &mut self,
        l: &Leader,
        j: usize,
        r: usize,
        cur: i64,
        picked: &mut Vec<usize>,
        sel_pos: [f64; 5],
        sel_sum: f64,
    ) {
        if !self.tick() {
            return;
        }
        if r == 0 {
            self.leaf(l, picked, cur);
            return;
        }
        let nch = l.chars.len();
        let mut q = j;
        while q + r <= nch {
            let p = cur + l.prefix[q + r] - l.prefix[q];
            if !self.open(p) {
                break;
            }
            // both the power bound and the gain bound fall as `q` grows
            if let Some(g) = &l.gains {
                if !self.open_gain(p, g, q, r, sel_pos, sel_sum) {
                    break;
                }
            }
            let rest = l.prefix[q + r] - l.prefix[q + 1];
            for &(u, m) in &l.chars[q].1 {
                if !self.open(cur + u + rest) {
                    break;
                }
                let gm = self.gains.get(m).copied().unwrap_or([0f64; 5]);
                let mut np = sel_pos;
                for k in 0..5 {
                    np[k] = np[k].max(gm[k]);
                }
                let ns = sel_sum + gm.iter().copied().fold(0f64, f64::max);
                if let Some(g) = &l.gains {
                    if !self.open_gain(cur + u + rest, g, q + 1, r - 1, np, ns) {
                        continue;
                    }
                }
                picked.push(m);
                self.dfs(l, q + 1, r - 1, cur + u, picked, np, ns);
                picked.pop();
                if self.timed_out || self.error.is_some() {
                    return;
                }
            }
            q += 1;
        }
    }

    fn leaf(&mut self, l: &Leader, picked: &[usize], bound: i64) {
        if !self.open(bound) {
            return;
        }
        let pool = self.pool;
        let t = self.t;
        let mut others: Vec<usize> = picked.to_vec();
        others.sort_by_key(|&m| pool.members[m].id);
        let members = [others[0], others[1], l.leader, others[2], others[3]];
        // with snap skills: the best assignment of these members' gains to the positions
        let gain = match self.live {
            LiveMode::Snaps(sl) => {
                let g = self.assigned_gain(members);
                let th = self.top.threshold();
                if th != i64::MIN && sl.gain_bound(bound, g) < th {
                    return;
                }
                Some(g)
            }
            _ => None,
        };
        self.stats.leaves += 1;
        let lead = match t.exact_lead(pool, members) {
            Ok(x) => x,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let fixed_part: i64 = (0..5).map(|i| t.a[members[i]] + lead[i]).sum();
        let wmax_sum: i64 = members.iter().map(|&m| t.wmax[m]).sum();
        if !self.open(fixed_part + wmax_sum) {
            return;
        }
        if let (LiveMode::Snaps(sl), Some(g)) = (self.live, gain) {
            let th = self.top.threshold();
            if th != i64::MIN && sl.gain_bound(fixed_part + wmax_sum, g) < th {
                return;
            }
        }
        if let LiveMode::Snaps(sl) = self.live {
            self.leaf_snaps(sl, l.leader, members, fixed_part);
            return;
        }
        self.stats.matchings += 1;
        let (wsum, assign) = best_assignment([
            &t.w[members[0]][..],
            &t.w[members[1]][..],
            &t.w[members[2]][..],
            &t.w[members[3]][..],
            &t.w[members[4]][..],
        ]);
        let power = fixed_part + wsum;
        let snaps = assign.map(|a| a.map(|j| t.snaps[j]));
        let mut ids = members.map(|m| pool.members[m].id);
        ids.sort_unstable();
        let (value, order) = match self.live {
            LiveMode::None | LiveMode::Snaps(_) => (power, [0, 1, 2, 3, 4]),
            LiveMode::Order(lc) => {
                let th = self.top.threshold();
                match lc.best_order(power, &members, th, &mut self.stats) {
                    Ok(Some(x)) => x,
                    Ok(None) => return,
                    Err(e) => {
                        self.error = Some(e);
                        return;
                    }
                }
            }
        };
        self.top.insert(Entry {
            value,
            power,
            ids,
            leader_id: pool.members[l.leader].id,
            snap_ids: snaps.map(|s| s.map_or(NO_SNAP, |i| pool.snaps[i].id)),
            order,
            members,
            snaps,
        });
    }

    /// The largest sum of the members' gains over the assignments of the five members to the five positions.
    fn assigned_gain(&self, members: [usize; 5]) -> f64 {
        let mut best = [f64::MIN; 32];
        best[0] = 0.0;
        for mask in 0usize..32 {
            if best[mask] == f64::MIN {
                continue;
            }
            let k = mask.count_ones() as usize;
            if k == 5 {
                continue;
            }
            for (i, &m) in members.iter().enumerate() {
                if mask & (1 << i) == 0 {
                    let v = best[mask] + self.gains[m][k];
                    let next = mask | (1 << i);
                    if v > best[next] {
                        best[next] = v;
                    }
                }
            }
        }
        best[31]
    }

    /// Leaf of the live objective with snap skills. The leader changes only the deck power (the performers follow
    /// the members, whatever their slots), so the set's best representative has the leader of largest exact
    /// member-only power (smallest id on ties); the other leaders' visits return at once.
    fn leaf_snaps(&mut self, sl: &SnapLive, leader: usize, members: [usize; 5], fixed_part: i64) {
        let pool = self.pool;
        let t = self.t;
        let mut best: Option<(i64, i64)> = None;
        for &x in &members {
            if !self.allowed.members[x] || self.allowed.leader.is_some_and(|l| l != x) {
                continue;
            }
            let mut others: Vec<usize> = members.iter().copied().filter(|&m| m != x).collect();
            others.sort_by_key(|&m| pool.members[m].id);
            let set = [others[0], others[1], x, others[2], others[3]];
            let fp = match t.exact_lead(pool, set) {
                Ok(lead) => (0..5).map(|i| t.a[set[i]] + lead[i]).sum::<i64>(),
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            };
            let id = pool.members[x].id;
            if best.is_none_or(|(bf, bid)| fp > bf || (fp == bf && id < bid)) {
                best = Some((fp, id));
            }
        }
        if best.is_none_or(|(_, id)| id != pool.members[leader].id) {
            return;
        }
        self.stats.matchings += 1;
        let th = self.top.threshold();
        let r = sl.best(pool, t, members, fixed_part, th, self.deadline, &mut self.stats);
        let (found, timed_out) = match r {
            Ok(x) => x,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        if timed_out {
            self.timed_out = true;
        }
        let Some(b) = found else { return };
        let mut ids = members.map(|m| pool.members[m].id);
        ids.sort_unstable();
        self.top.insert(Entry {
            value: b.score,
            power: b.power,
            ids,
            leader_id: pool.members[leader].id,
            snap_ids: b.snaps.map(|s| s.map_or(NO_SNAP, |i| pool.snaps[i].id)),
            order: b.order,
            members,
            snaps: b.snaps,
        });
    }
}
