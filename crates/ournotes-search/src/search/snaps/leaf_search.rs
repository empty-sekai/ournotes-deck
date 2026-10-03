//! Legacy leaf search over performance orders and Snap classes.
use super::*;

/// The state of one leaf search.
pub(super) struct Leaf<'s, 'a, 'm> {
    pub(super) sl: &'s SnapLive<'a>,
    pub(super) pool: &'s Pool<'m>,
    pub(super) t: &'s Tables<'m>,
    pub(super) members: [usize; 5],
    pub(super) fixed: i64,
    pub(super) wb: Vec<Vec<i64>>,
    pub(super) wbmax: [i64; 5],
    pub(super) threshold: i64,
    /// Best candidate simulated so far and its score.
    pub(super) best: Option<(Cand, i64)>,
    pub(super) pending: Vec<Cand>,
    /// Constrained matching of each class assignment.
    pub(super) matched: HashMap<[usize; 5], Option<Assignment>>,
    pub(super) scratch: Scratch,
    /// Scores simulated in this leaf by performer identities and power.
    pub(super) simulated: HashMap<([(u32, u32); 5], i64), i64>,
    /// `Fine::zero_from` of each recovery vector met in this leaf.
    pub(super) zero_from: HashMap<[i64; 5], i64>,
    pub(super) deadline: Option<Instant>,
    pub(super) timed_out: bool,
    pub(super) sims: u64,
    /// Nodes of the class search (the deadline is checked every 1024).
    pub(super) nodes: u64,
    /// Per slot, over the classes whose life-raising rows are only recoveries at skill events (`n`) and over the
    /// others (`o`): the largest snap weight, the largest gain at each position, the largest recovery (`n` only),
    /// and whether there is an other class.
    pub(super) wbn: [i64; 5],
    pub(super) wbo: [i64; 5],
    pub(super) gn: [[f64; 5]; 5],
    pub(super) go: [[f64; 5]; 5],
    pub(super) recn: [i64; 5],
    pub(super) has_o: [bool; 5],
    /// Seeds per deck (`SnapLive::n`).
    pub(super) n: i64,
    /// With Gekisou on: the seed scores simulated so far (a prefix of the seed set) by performer identities and power.
    pub(super) seeded: HashMap<SeededDeckKey, Vec<i32>>,
    pub(super) count: LeafCounts,
}

/// Work counters of one leaf (see [`PowerStats`](crate::search::PowerStats)).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LeafCounts {
    pub(super) seed_sims: u64,
    pub(super) early_stops: u64,
    pub(super) seeds_saved: u64,
    pub(super) prefix_frames_saved: u64,
    pub(super) violations: u64,
    pub(super) class_choices: u64,
    pub(super) candidates: u64,
    /// The largest seed score simulated in the leaf (read only by an ablation).
    pub(super) observed_max: i64,
}

/// The part of the class search's life bound that depends on the chosen classes, by the recovery of a node's slot:
/// (recovery, start of the entries at the life-zero factor, rest of the bound).
pub(super) struct LifeMemo {
    pub(super) n: usize,
    pub(super) at: [(i64, usize, f64); 8],
}

/// Remaining sums of the class search of one order, from each slot on: largest snap weights and gains over all
/// classes (`w`, `g`) and over the classes without other life-raising rows (`wn`, `gn`).
pub(super) struct Rests {
    pub(super) w: [i64; 6],
    pub(super) g: [f64; 6],
    pub(super) wn: [i64; 6],
    pub(super) gn: [f64; 6],
}

impl Leaf<'_, '_, '_> {
    pub(super) fn cutoff(&self) -> i64 {
        match &self.best {
            None => self.threshold,
            Some((_, s)) => self.threshold.max(*s),
        }
    }

    /// A per-seed bound scaled to the value of a deck (the sum over the seeds).
    pub(super) fn sc(&self, bound: i64) -> i64 {
        bound.saturating_mul(self.n)
    }

    /// Whether a candidate with this bound, power and identity could still beat the best simulated one.
    pub(super) fn could_beat(&self, c: &Cand) -> bool {
        self.reaches(c, self.sc(c.bound), false)
    }

    /// Whether a candidate whose value is at most `v` could still enter the Top-K and beat the leaf's best deck
    /// (`stop`: the early stop of the seed loop, where an ablation also drops equal values).
    pub(super) fn reaches(&self, c: &Cand, v: i64, stop: bool) -> bool {
        let strict = stop && ablated(ablate::EARLY_STOP_EQUAL);
        if v < self.threshold || (strict && v == self.threshold) {
            return false;
        }
        match &self.best {
            None => true,
            Some((b, s)) => {
                v > *s || (v == *s && !strict && (c.power, b.snap_ids, b.order) > (b.power, c.snap_ids, c.order))
            }
        }
    }

    pub(super) fn matching(&mut self, cs: [usize; 5]) -> Option<Assignment> {
        if let Some(x) = self.matched.get(&cs) {
            return *x;
        }
        let t = self.t;
        let sl = self.sl;
        let masks: Vec<Vec<bool>> =
            (0..5).map(|i| sl.class_of[self.members[i]].iter().map(|&c| c as usize == cs[i]).collect()).collect();
        let none_ok = cs.map(|c| c == 0);
        let m = self.members;
        let r = constrained_assignment(
            [&t.w[m[0]][..], &t.w[m[1]][..], &t.w[m[2]][..], &t.w[m[3]][..], &t.w[m[4]][..]],
            [&masks[0][..], &masks[1][..], &masks[2][..], &masks[3][..], &masks[4][..]],
            none_ok,
        );
        self.matched.insert(cs, r);
        r
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn dfs(
        &mut self,
        o: [usize; 5],
        pos: [usize; 5],
        i: usize,
        s1: i64,
        s2: f64,
        rest: &Rests,
        other: bool,
        cs: &mut [usize; 5],
    ) -> Result<(), Error> {
        self.nodes += 1;
        if self.nodes.is_multiple_of(1024)
            && let Some(d) = self.deadline
            && Instant::now() >= d
        {
            self.timed_out = true;
        }
        if self.timed_out {
            return Ok(());
        }
        if i == 5 {
            self.count.class_choices += 1;
            return self.consider(o, *cs, pos);
        }
        let m = self.members[i];
        let ncl = self.sl.classes[m].len();
        let life_on = !other && self.sl.life_bound;
        let mut memo = LifeMemo { n: 0, at: [(0, 0, 0.0); 8] };
        for c in 0..ncl {
            let w = self.wb[i][c];
            if w <= i64::MIN / 8 {
                continue;
            }
            let g = self.sl.contrib[m][c][pos[i]].gain;
            let (n1, n2) = (s1 + w, s2 + g);
            if self.sc(ub(n1 + rest.w[i + 1], n2 + rest.g[i + 1], self.sl.eps)) < self.cutoff() {
                continue;
            }
            cs[i] = c;
            let kind = self.sl.fine.life[m][c];
            if life_on && kind != LifeKind::Other && !self.class_open(pos, i, n1, n2, rest, cs, &mut memo) {
                continue;
            }
            self.dfs(o, pos, i + 1, n1, n2, rest, other || kind == LifeKind::Other, cs)?;
        }
        Ok(())
    }

    /// The start of the entries at the life-zero factor, and the rest of the class search's life bound there (`A0`
    /// and the gains of the chosen slots before `upto - 1` split at the start, plus a bound of the split gains of the
    /// remaining slots' classes without other life-raising rows), for the chosen classes `cs[..upto]` and the largest
    /// recovery of each remaining slot. The rest is 0 when no entry is at the life-zero factor.
    pub(super) fn life_rest(&mut self, pos: [usize; 5], upto: usize, cs: &[usize; 5]) -> (usize, f64) {
        let sl = self.sl;
        let mut rec = [0i64; 5];
        for j in 0..5 {
            rec[pos[j]] = if j < upto {
                match sl.fine.life[self.members[j]][cs[j]] {
                    LifeKind::Recovery(r) => r,
                    _ => 0,
                }
            } else {
                self.recn[j]
            };
        }
        // the fold is non-decreasing in each recovery, so the largest ones give the latest `t0`
        let life = if rec == [0; 5] {
            CandLife::NoRise
        } else {
            let f = &sl.fine;
            CandLife::ZeroFrom(*self.zero_from.entry(rec).or_insert_with(|| f.zero_from(rec)))
        };
        let start = sl.dead_start(life);
        if start >= sl.coef.times.len() {
            return (start, 0.0);
        }
        let mut a = sl.a0_from(start);
        for j in upto..5 {
            a += sl.split_gain(self.members[j], pos[j], start, self.gn[j][pos[j]]);
        }
        for j in 0..upto.saturating_sub(1) {
            a += sl.gain_from(&sl.contrib[self.members[j]][cs[j]][pos[j]], start);
        }
        (start, a)
    }

    /// Whether the completions of the chosen classes `cs[..upto]` (power up to `n1`, gains `n2`) with another
    /// life-raising class at some remaining slot can reach the cutoff (bounded without life).
    pub(super) fn other_open(&self, pos: [usize; 5], upto: usize, n1: i64, n2: f64, rest: &Rests) -> bool {
        let cutoff = self.cutoff();
        (upto..5).any(|j| {
            self.has_o[j] && {
                let p = n1 + rest.w[upto] - self.wbmax[j] + self.wbo[j];
                let (gm, go) = (rest.g[j] - rest.g[j + 1], self.go[j][pos[j]]);
                self.sc(ub(p, n2 + rest.g[upto] - gm + go, self.sl.eps)) >= cutoff
            }
        })
    }

    /// Whether the leaf can reach the cutoff under the class search's life bound, before any order: its candidates with
    /// another life-raising class at some slot, bounded without life, and the others with the largest recovery of
    /// any slot at every position; gains by the best assignment of slots to positions (`gmax`: the largest gain of
    /// each slot at each position).
    pub(super) fn leaf_open(&mut self, gmax: &[[f64; 5]; 5]) -> bool {
        let sl = self.sl;
        let cutoff = self.cutoff();
        let wsum: i64 = self.wbmax.iter().sum();
        for j in 0..5 {
            if self.has_o[j] {
                let mut g = *gmax;
                g[j] = self.go[j];
                let p = self.fixed + wsum - self.wbmax[j] + self.wbo[j];
                if self.sc(ub(p, sl.a0 + best_assignment(&g), sl.eps)) >= cutoff {
                    return true;
                }
            }
        }
        let r = self.recn.iter().copied().max().unwrap_or(0);
        let life = if r <= 0 {
            CandLife::NoRise
        } else {
            let (f, rec) = (&sl.fine, [r; 5]);
            CandLife::ZeroFrom(*self.zero_from.entry(rec).or_insert_with(|| f.zero_from(rec)))
        };
        let start = sl.dead_start(life);
        let power = self.wbn.iter().fold(self.fixed, |a, &w| a.saturating_add(w));
        if start >= sl.coef.times.len() {
            return self.sc(ub(power, sl.a0 + best_assignment(&self.gn), sl.eps)) >= cutoff;
        }
        let g: [[f64; 5]; 5] =
            std::array::from_fn(|j| std::array::from_fn(|k| sl.split_gain(self.members[j], k, start, self.gn[j][k])));
        self.sc(ub(power, sl.a0_from(start) + best_assignment(&g), sl.eps)) >= cutoff
    }

    /// Whether an order can still reach the cutoff under the class search's life bound, before any class is chosen.
    pub(super) fn order_open(&mut self, pos: [usize; 5], rest: &Rests) -> bool {
        let (fixed, a0) = (self.fixed, self.sl.a0);
        if self.other_open(pos, 0, fixed, a0, rest) {
            return true;
        }
        let (start, a) = self.life_rest(pos, 0, &[0; 5]);
        let a = if start >= self.sl.coef.times.len() { a0 + rest.gn[0] } else { a };
        self.sc(ub(fixed.saturating_add(rest.wn[0]), a, self.sl.eps)) >= self.cutoff()
    }

    /// Whether a node of the class search whose slots up to `i` have classes `cs[..=i]` without other life-raising
    /// rows (power up to `n1`, gains `n2`) can still reach the cutoff: its completions with such classes only, whose
    /// recoveries are at most the largest of each slot, read life 0 from the start their fold gives (`A0` and the
    /// chosen gains split there, the remaining gains unchanged); its completions with another class at some slot are
    /// bounded without life. `memo` keeps the start and the rest of the bound by the recovery of slot `i`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn class_open(
        &mut self,
        pos: [usize; 5],
        i: usize,
        n1: i64,
        n2: f64,
        rest: &Rests,
        cs: &[usize; 5],
        memo: &mut LifeMemo,
    ) -> bool {
        if self.other_open(pos, i + 1, n1, n2, rest) {
            return true;
        }
        let sl = self.sl;
        let r = match sl.fine.life[self.members[i]][cs[i]] {
            LifeKind::Recovery(r) => r,
            _ => 0,
        };
        let (start, a) = match memo.at[..memo.n].iter().find(|x| x.0 == r) {
            Some(&(_, start, a)) => (start, a),
            None => {
                let (start, a) = self.life_rest(pos, i + 1, cs);
                if memo.n < memo.at.len() {
                    memo.at[memo.n] = (r, start, a);
                    memo.n += 1;
                }
                (start, a)
            }
        };
        let power = n1.saturating_add(rest.wn[i + 1]);
        let cutoff = self.cutoff();
        if start >= sl.coef.times.len() {
            return self.sc(ub(power, n2 + rest.gn[i + 1], sl.eps)) >= cutoff;
        }
        // the split gain is at most the plain one
        let part = &sl.contrib[self.members[i]][cs[i]][pos[i]];
        if self.sc(ub(power, a + part.gain, sl.eps)) < cutoff {
            return false;
        }
        self.sc(ub(power, a + sl.gain_from(part, start), sl.eps)) >= cutoff
    }

    /// Bounds one (order, class assignment) and queues it when it can still win.
    pub(super) fn consider(&mut self, o: [usize; 5], cs: [usize; 5], pos: [usize; 5]) -> Result<(), Error> {
        let Some((w, snaps_j)) = self.matching(cs) else { return Ok(()) };
        let power = self.fixed + w;
        let sl = self.sl;
        let parts: [&Contrib; 5] = std::array::from_fn(|i| &sl.contrib[self.members[i]][cs[i]][pos[i]]);
        let s2 = sl.a0 + parts.iter().map(|p| p.gain).sum::<f64>();
        if self.sc(ub(power, s2, sl.eps)) < self.cutoff() {
            return Ok(());
        }
        let life = self.cand_life(o, cs);
        let start = sl.dead_start(life);
        if start < sl.coef.times.len() && self.sc(ub(power, sl.life_sum(parts, start), sl.eps)) < self.cutoff() {
            return Ok(());
        }
        let snaps = snaps_j.map(|x| x.map(|j| self.t.snaps[j]));
        let snap_ids = snaps.map(|x| x.map_or(NO_SNAP, |i| self.pool.snaps[i].id));
        let mut scratch = std::mem::take(&mut self.scratch);
        let src: [u32; 5] = std::array::from_fn(|k| sl.fine.src[self.members[o[k]]][cs[o[k]]]);
        let bound = sl.fine_bound(power, parts, src, life, &mut scratch);
        self.scratch = scratch;
        let c = Cand { bound, power, snaps, snap_ids, order: o, classes: cs };
        if !self.could_beat(&c) {
            return Ok(());
        }
        self.count.candidates += 1;
        self.pending.push(c);
        if self.pending.len() >= FLUSH {
            self.flush(false)?;
        }
        Ok(())
    }

    /// The life bound of the candidate with order `o` and classes `cs`: from the recovery of the performer at each
    /// position, when every life-raising row is a recovery at the performer's own skill events.
    pub(super) fn cand_life(&mut self, o: [usize; 5], cs: [usize; 5]) -> CandLife {
        let f = &self.sl.fine;
        let mut rec = [0i64; 5];
        for k in 0..5 {
            match f.life[self.members[o[k]]][cs[o[k]]] {
                LifeKind::None => {}
                LifeKind::Recovery(r) => rec[k] = r,
                LifeKind::Other => return CandLife::Unknown,
            }
        }
        if rec == [0; 5] {
            return CandLife::NoRise;
        }
        CandLife::ZeroFrom(*self.zero_from.entry(rec).or_insert_with(|| f.zero_from(rec)))
    }

    /// Simulates pending candidates in order of their bound: all of them that can still win when `all`, else the
    /// first one (to raise the cutoff).
    pub(super) fn flush(&mut self, all: bool) -> Result<(), Error> {
        self.pending
            .sort_by(|a, b| (b.bound, b.power, a.snap_ids, a.order).cmp(&(a.bound, a.power, b.snap_ids, b.order)));
        let pending = std::mem::take(&mut self.pending);
        let mut rest = Vec::new();
        let mut done = 0usize;
        for c in pending {
            if !self.could_beat(&c) {
                continue;
            }
            if self.timed_out || (!all && done >= 1) {
                rest.push(c);
                continue;
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.timed_out = true;
                rest.push(c);
                continue;
            }
            let Some(score) = self.evaluate(&c)? else {
                done += 1;
                continue;
            };
            done += 1;
            let better = match &self.best {
                None => true,
                Some((b, s)) => (score, c.power, b.snap_ids, b.order) > (*s, b.power, c.snap_ids, c.order),
            };
            if better {
                self.best = Some((c, score));
            }
        }
        self.pending = rest;
        Ok(())
    }

    /// The value of a candidate: its simulated score, or with Gekisou on the sum of its seed scores. With Gekisou on
    /// the seeds run in order and the candidate is dropped (`None`) as soon as its partial sum plus the per-seed bound
    /// for each remaining seed cannot reach the cutoff, or when the deadline passes; a value is returned only when
    /// every seed has been simulated.
    pub(super) fn evaluate(&mut self, c: &Cand) -> Result<Option<i64>, Error> {
        let sl = self.sl;
        let Some(g) = sl.setup.gk.as_ref() else { return Ok(Some(self.simulate(c)? as i64)) };
        let key: [(u32, u32); 5] = std::array::from_fn(|k| {
            let slot = c.order[k];
            let m = self.members[slot];
            (sl.sim_id[m], sl.class_gid[m][c.classes[slot]])
        });
        let mut done = self.seeded.remove(&(key, c.power)).unwrap_or_default();
        let n = g.seeds.len();
        let mut sum: i64 = done.iter().map(|&x| x as i64).sum();
        let mut runner: Option<SeedRunner> = None;
        let mut out = None;
        loop {
            let j = done.len();
            if j == n {
                out = Some(sum);
                break;
            }
            let per = if ablated(ablate::OBSERVED_MAX) { self.count.observed_max } else { c.bound };
            let most = sum.saturating_add(per.saturating_mul((n - j) as i64));
            if j > 0 && !ablated(ablate::NO_EARLY_STOP) && !self.reaches(c, most, true) {
                self.count.early_stops += 1;
                self.count.seeds_saved += (n - j) as u64;
                break;
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.timed_out = true;
                break;
            }
            if runner.is_none() {
                if done.is_empty() {
                    self.sims += 1;
                }
                runner = Some(SeedRunner::new(self, c)?);
            }
            let r = runner.as_mut().expect("runner");
            let x = r.run(sl, g.seeds[j], &mut self.count)?;
            self.count.seed_sims += 1;
            if x as i64 > c.bound {
                self.count.violations += 1;
            }
            self.count.observed_max = self.count.observed_max.max(x as i64);
            done.push(x);
            sum += x as i64;
        }
        self.seeded.insert((key, c.power), done);
        Ok(out)
    }

    pub(super) fn simulate(&mut self, c: &Cand) -> Result<i32, Error> {
        // performers with equal identities and snap classes give the same simulation
        let sl = self.sl;
        let key: [(u32, u32); 5] = std::array::from_fn(|k| {
            let slot = c.order[k];
            let m = self.members[slot];
            (sl.sim_id[m], sl.class_gid[m][c.classes[slot]])
        });
        if let Some(&s) = self.simulated.get(&(key, c.power)) {
            return Ok(s as i32);
        }
        let s = self.simulate_deck(c)?;
        self.simulated.insert((key, c.power), s as i64);
        Ok(s)
    }

    pub(super) fn simulate_deck(&mut self, c: &Cand) -> Result<i32, Error> {
        self.sims += 1;
        let (perf, power) = self.performers(c)?;
        self.sl.setup.score(self.sl.master, &perf, power)
    }

    /// The performers of a candidate, in performance order, and its power.
    pub(super) fn performers(&self, c: &Cand) -> Result<(Vec<Performer>, i32), Error> {
        let pool = self.pool;
        let perf = c
            .order
            .iter()
            .map(|&slot| performer(&pool.members[self.members[slot]], c.snaps[slot].map(|s| &pool.snaps[s])))
            .collect::<Result<Vec<_>, _>>()?;
        let power = i32::try_from(c.power).map_err(|_| Error::Domain("deck power exceeds the 32-bit range".into()))?;
        Ok((perf, power))
    }
}

/// The seed runs of one candidate. Before the first frame in which a draw can happen, nothing in the live depends
/// on the seed, so the state there is the same for every seed: the first run keeps a copy of it (after checking
/// that nothing has been drawn), and every later run starts from that copy with its own seed.
pub(super) struct SeedRunner {
    pub(super) fresh: LiveModel,
    pub(super) prefix: Option<LiveModel>,
}

impl SeedRunner {
    pub(super) fn new(leaf: &Leaf, c: &Cand) -> Result<SeedRunner, Error> {
        let (perf, power) = leaf.performers(c)?;
        Ok(SeedRunner { fresh: leaf.sl.setup.gekisou_model(leaf.sl.master, &perf, power)?, prefix: None })
    }

    pub(super) fn run(&mut self, sl: &SnapLive, seed: i32, count: &mut LeafCounts) -> Result<i32, Error> {
        let setup = sl.setup;
        let late = ablated(ablate::PREFIX_LATE);
        let f0 =
            if ablated(ablate::NO_PREFIX) { 0 } else { (sl.prefix_frame + late as usize).min(setup.play.frames.len()) };
        if let Some(p) = &self.prefix {
            let mut lm = p.clone();
            lm.set_seed(seed);
            count.prefix_frames_saved += f0 as u64;
            return setup.play_from(&mut lm, f0);
        }
        let mut lm = self.fresh.clone();
        lm.set_seed(seed);
        if f0 == 0 {
            return setup.play_from(&mut lm, 0);
        }
        let g = setup.gk.as_ref().ok_or_else(|| Error::Game("a Gekisou live without a Gekisou setup".into()))?;
        for (f, &dt) in setup.play.frames[..f0].iter().zip(&g.dt[..f0]) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
        }
        if lm.draws() != 0 && !late {
            return Err(Error::Game("a random draw before the first frame that can draw".into()));
        }
        self.prefix = Some(lm.clone());
        setup.play_from(&mut lm, f0)
    }
}
