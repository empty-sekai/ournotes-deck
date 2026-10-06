//! Live-score objective on top of the power search.
//!
//! Precondition: Gekisou off and snap skills excluded. Then the leader and the snaps change the live score only
//! through the deck power, and for a fixed performance order the score is non-decreasing in the power (every step
//! of the per-note chain multiplies by a fixed non-negative value, rounds and floors). So the best representative of a
//! member set is its highest-power (leader, snaps) choice with its best performance order. This must be revisited
//! when snap skills or Gekisou are modelled.

use std::collections::HashSet;

use crate::search::power::PowerStats;
use ournotes_sim::error::Error;
use ournotes_sim::live::model::LiveModel;
use ournotes_sim::live::score::{GOOD, GREAT, JUST, PERFECT};
use ournotes_sim::live::skill::FactorCommand;
use ournotes_sim::master::Master;
use ournotes_sim::pool::Pool;

/// Relative error bound of the per-note float chain (16 roundings of at most 2^-24 each, with margin).
const CHAIN_EPS: f64 = 2e-6;

fn add_up(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 { a + b } else { (a + b).next_up() }
}

fn mul_up(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 { 0.0 } else { (a * b).next_up() }
}

pub(crate) struct LiveCtx<'a> {
    pub model: &'a LiveModel,
    pub profile_of: Vec<usize>,
    /// cmds[e][p]: commands of the chart's e-th skill event when profile p performs it.
    cmds: Vec<Vec<Vec<FactorCommand>>>,
    /// Event index (performance position) of the chart's e-th skill event.
    event_index: Vec<usize>,
    /// Notes inside some effect window: (coefficient, contrib[e][p]).
    inside: Vec<(f64, Vec<Vec<f64>>)>,
    /// Sum over all notes of coefficient * (1 + drift) * (1 + CHAIN_EPS).
    base: f64,
    global: f64,
    factor_bounds: [f32; 2],
}

fn score_type_of_judgement(j: i32) -> i32 {
    match j {
        6 => JUST,
        5 => PERFECT,
        4 => GREAT,
        3 => GOOD,
        _ => 0,
    }
}

impl<'a> LiveCtx<'a> {
    pub fn new(master: &'a Master, pool: &Pool, model: &'a LiveModel) -> Result<LiveCtx<'a>, Error> {
        let mut profiles: Vec<(i64, i64)> = Vec::new();
        let mut profile_of = Vec::with_capacity(pool.members.len());
        for m in &pool.members {
            let k = (m.live_skill_id, m.live_skill_level);
            let p = match profiles.iter().position(|&x| x == k) {
                Some(p) => p,
                None => {
                    profiles.push(k);
                    profiles.len() - 1
                }
            };
            profile_of.push(p);
        }
        let mut event_index = Vec::new();
        for &(idx, _) in &model.events {
            if !(0..5).contains(&idx) {
                return Err(Error::Input(format!("skill event index {idx} outside the five performance positions")));
            }
            event_index.push(idx as usize);
        }
        let mut cmds = Vec::with_capacity(model.events.len());
        for &(idx, t) in &model.events {
            let mut per = Vec::with_capacity(profiles.len());
            for &prof in &profiles {
                let perf = [prof; 5];
                let c = ournotes_sim::live::skill::live_skill_commands(
                    master,
                    &[(idx, t)],
                    &perf,
                    &model.life_at_event,
                    model.music_length_ms,
                    None,
                )?;
                if c.iter().any(|x| x.combo_mill != 0 || x.luck != 0 || x.band_total_power != 0) {
                    return Err(Error::Unsupported("live skill changing combo, luck or power".into()));
                }
                if !c.len().is_multiple_of(2)
                    || c.as_chunks::<2>().0.iter().any(|pair| {
                        let (start, end) = (&pair[0], &pair[1]);
                        start.note_mill < 0
                            || start.judge_mill < 0
                            || end.time_ms < start.time_ms
                            || end.owner_id != start.owner_id
                            || end.judgement != start.judgement
                            || end.note_mill != -start.note_mill
                            || end.judge_mill != -start.judge_mill
                    })
                {
                    return Err(Error::Domain("live search requires paired nonnegative score-up effects".into()));
                }
                per.push(c);
            }
            cmds.push(per);
        }
        // Two representation roundings and one addition per command, plus the
        // final note/judgement field addition. gamma bounds rounding feedback.
        let mut n_max = 0usize;
        let mut f_max = 0f64;
        for per in &cmds {
            n_max = n_max
                .checked_add(per.iter().map(Vec::len).max().unwrap_or(0))
                .ok_or_else(|| Error::Domain("live factor command count exceeds its proof range".into()))?;
            let f = per
                .iter()
                .map(|c| {
                    c.iter()
                        .map(|x| {
                            let magnitude = f64::from(x.note_mill).abs() + f64::from(x.judge_mill).abs();
                            if magnitude == 0.0 { 0.0 } else { (magnitude / 1e5).next_up() }
                        })
                        .fold(0.0, add_up)
                })
                .fold(0f64, f64::max);
            f_max = add_up(f_max, f);
        }
        let alpha = mul_up(add_up(mul_up(3.0, (n_max as f64).next_up()), 1.0), 2f64.powi(-24));
        if !alpha.is_finite() || alpha >= 1.0 {
            return Err(Error::Domain("live factor command count has no finite rounding certificate".into()));
        }
        let gamma = (alpha / (1.0 - alpha).next_down()).next_up();
        let drift = if n_max == 0 { 0.0 } else { mul_up(gamma, add_up(1.0, f_max)) };
        if !drift.is_finite() || drift >= 0.5 {
            return Err(Error::Domain("live factor state has no positive finite certificate".into()));
        }
        let factor_bounds =
            [((1.0 - drift).next_down() as f32).next_down(), (add_up(add_up(1.0, f_max), drift) as f32).next_up()];
        let coef = model.note_coefficients();
        if coef.iter().any(|(_, _, k)| !k.is_finite() || *k < 0.0) {
            return Err(Error::Domain("live search requires finite nonnegative note coefficients".into()));
        }
        let mut base = 0f64;
        let mut inside = Vec::new();
        for &(time, st, k) in &coef {
            base = add_up(base, mul_up(mul_up(k, add_up(1.0, drift)), add_up(1.0, CHAIN_EPS)));
            let mut contrib = vec![vec![0f64; profiles.len()]; cmds.len()];
            let mut any = false;
            for (e, per) in cmds.iter().enumerate() {
                for (p, c) in per.iter().enumerate() {
                    let mut v = 0f64;
                    for pair in c.chunks(2) {
                        let (s, f) = (&pair[0], &pair[1]);
                        if s.time_ms <= time && time < f.time_ms {
                            if s.note_mill != 0 {
                                v = add_up(v, (f64::from(s.note_mill) / 1e5).next_up());
                            }
                            if score_type_of_judgement(s.judgement) == st && st != 0 && s.judge_mill != 0 {
                                v = add_up(v, (f64::from(s.judge_mill) / 1e5).next_up());
                            }
                        }
                    }
                    if v != 0.0 {
                        any = true;
                    }
                    contrib[e][p] = v;
                }
            }
            if any {
                inside.push((k, contrib));
            }
        }
        let mut ctx = LiveCtx { model, profile_of, cmds, event_index, inside, base, global: 0.0, factor_bounds };
        ctx.global = ctx.a_plus_all();
        if !ctx.global.is_finite() {
            return Err(Error::Domain("live score envelope has no finite coefficient".into()));
        }
        Ok(ctx)
    }

    pub fn prove_score_domain(&self, power_upper_bound: i64, cancelled: impl FnMut() -> bool) -> Result<bool, Error> {
        self.model.prove_search_domain(power_upper_bound, self.factor_bounds, cancelled)
    }

    /// Linear gains of the bound: `g[e][i]` is the extra score per unit of power when profile `ps[i]` performs the
    /// chart's e-th skill event. The bound of an assignment is `base + sum over events of g`.
    fn gains(&self, ps: &[usize]) -> Vec<Vec<f64>> {
        let mut g = vec![vec![0f64; ps.len()]; self.event_index.len()];
        for (k, contrib) in &self.inside {
            for (e, per) in contrib.iter().enumerate() {
                for (i, &p) in ps.iter().enumerate() {
                    g[e][i] = add_up(g[e][i], mul_up(*k, per[p]));
                }
            }
        }
        for row in &mut g {
            for x in row.iter_mut() {
                *x = mul_up(*x, add_up(1.0, CHAIN_EPS));
            }
        }
        g
    }

    fn a_plus_all(&self) -> f64 {
        let n = self.cmds.first().map_or(0, Vec::len);
        let all: Vec<usize> = (0..n).collect();
        let g = self.gains(&all);
        g.iter().map(|row| row.iter().copied().fold(0.0, f64::max)).fold(self.base, add_up)
    }

    fn ub(power: i64, a: f64) -> i64 {
        let v = mul_up(power.max(0) as f64, a);
        if v >= i64::MAX as f64 { i64::MAX } else { v.ceil() as i64 }
    }

    /// An upper bound of the live score of any deck with power at most `power`.
    pub fn score_bound(&self, power: i64) -> i64 {
        Self::ub(power, self.global)
    }

    /// The best performance order of five members at a power: (score, order), ties to the smallest order; `None`
    /// when no order can reach `threshold`. Orders are evaluated in descending order of their bound and the search
    /// stops at the first bound below the best score found.
    pub fn best_order(
        &self,
        power: i64,
        members: &[usize; 5],
        threshold: i64,
        stats: &mut PowerStats,
    ) -> Result<Option<(i64, [usize; 5])>, Error> {
        let prof: [usize; 5] = members.map(|m| self.profile_of[m]);
        let mut ps: Vec<usize> = prof.to_vec();
        ps.sort_unstable();
        ps.dedup();
        let g = self.gains(&ps);
        let slot_of = |p: usize| ps.iter().position(|&x| x == p).expect("profile");
        let mut seen = HashSet::new();
        let mut cands: Vec<(i64, [usize; 5])> = Vec::with_capacity(120);
        let mut order = [0usize, 1, 2, 3, 4];
        loop {
            // orders that give every skill event the same live skill score the same; keep the first
            let key: Vec<usize> = self.event_index.iter().map(|&idx| prof[order[idx]]).collect();
            if seen.insert(key) {
                let mut a = self.base;
                for (e, &idx) in self.event_index.iter().enumerate() {
                    a = add_up(a, g[e][slot_of(prof[order[idx]])]);
                }
                cands.push((Self::ub(power, a), order));
            }
            if !next_permutation(&mut order) {
                break;
            }
        }
        // stable: equal bounds keep the lexicographic order
        cands.sort_by_key(|x| std::cmp::Reverse(x.0));
        if cands[0].0 < threshold {
            return Ok(None);
        }
        let mut best: Option<(i64, [usize; 5])> = None;
        for (bound, order) in cands {
            if let Some(b) = best
                && bound < b.0
            {
                break;
            }
            let mut cmds = Vec::new();
            for (e, &idx) in self.event_index.iter().enumerate() {
                cmds.extend_from_slice(&self.cmds[e][prof[order[idx]]]);
            }
            stats.orders += 1;
            let s = self.model.score(power as i32, &cmds) as i64;
            if best.is_none_or(|b| s > b.0 || (s == b.0 && order < b.1)) {
                best = Some((s, order));
            }
        }
        Ok(best)
    }

    /// The commands of a deck with this performance order.
    pub fn commands(&self, members: &[usize; 5], order: &[usize; 5]) -> Vec<FactorCommand> {
        let mut cmds = Vec::new();
        for (e, &idx) in self.event_index.iter().enumerate() {
            cmds.extend_from_slice(&self.cmds[e][self.profile_of[members[order[idx]]]]);
        }
        cmds
    }
}

/// Lexicographic next permutation; false after the last one.
pub(crate) fn next_permutation(a: &mut [usize; 5]) -> bool {
    let n = a.len();
    let mut i = n - 1;
    while i > 0 && a[i - 1] >= a[i] {
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    let mut j = n - 1;
    while a[j] <= a[i - 1] {
        j -= 1;
    }
    a.swap(i - 1, j);
    a[i..].reverse();
    true
}
