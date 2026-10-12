//! The order-step bound: a node bound that keeps the payoff step of each performance order.

use super::*;

/// What the order-step bound of a node (see [`JointBounds::order_steps_prepare`]) reads of its placed slots, and the
/// open pairs of each suffix of the candidate order from the first offset a check of the node's choice loop asked for
/// (the loop asks for ascending offsets, so one backward pass collects them all).
#[derive(Default)]
pub(crate) struct OrderSteps {
    ready: bool,
    depth: usize,
    profile: usize,
    /// The relaxed power and event bonus maxima of the node's completions (`JointBounds::relax`).
    caps: (i64, i64),
    /// The placed slots' power and event bonus.
    placed: (i64, i64),
    a0: f64,
    /// Per performance order, the placed slots' gain sum in slot order, rounded up.
    gains: Vec<f64>,
    /// The placed characters, and per choice whether a placed slot holds its Snap.
    characters: Vec<i64>,
    used: Vec<bool>,
    /// The suffix summaries from candidate `first` on: per offset the largest per-position gains of the open pairs
    /// and the top of `stack`, a persistent stack of (power, bonus, parent) whose path from a top holds that suffix's
    /// Pareto frontier.
    first: usize,
    free: Vec<[f64; 5]>,
    top: Vec<u32>,
    stack: Vec<(i64, i64, u32)>,
    singles: Vec<(i64, i64)>,
    sums: Vec<(i64, i64)>,
    factors: Vec<f64>,
    steps: Vec<(f64, i128)>,
}

/// The empty path of `OrderSteps::stack`.
const NO_PAIR: u32 = u32::MAX;

impl OrderSteps {
    /// Starts a node: the next `JointBounds::order_steps_prepare` prepares it anew.
    pub(crate) fn reset(&mut self) {
        self.ready = false;
    }

    /// Whether an open slot of the node may take a member and choice: no placed character or Snap.
    fn open(&self, pool: &Pool, m: usize, choice: usize) -> bool {
        !self.characters.contains(&pool.members[m].character_id) && !self.used[choice]
    }
}

/// The pairs no other pair dominates in both parts (ties keep one).
pub(super) fn pareto_pairs(mut pairs: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    pairs.sort_unstable_by(|a, b| b.cmp(a));
    let mut out: Vec<(i64, i64)> = Vec::new();
    for pair in pairs {
        if out.last().is_none_or(|last| pair.1 > last.1) {
            out.push(pair);
        }
    }
    out
}

impl JointBounds {
    /// The order-step bound: a node bound that keeps the payoff step of each performance order (`positions`, one per
    /// order). The placed slots read their gains at the order's positions (keyed when the node has a keyed envelope)
    /// and each slot to fill the largest gain an open pair (a remaining member and choice the open slots may take) has
    /// at its position. Power and event bonus stay paired: a completion's pair is at most the placed one plus, per
    /// slot to fill, the pair of some open member and choice (characters and Snaps may repeat), so it is dominated by
    /// a point of the Pareto frontier of those sums, and each part is also at most its relaxed maximum of
    /// [`JointBounds::relax`]. An order's payoff is nondecreasing in its score cap and its bonus, so the sum over the
    /// orders of the frontier's best payoff bounds the payoff numerator of every completion without the concave
    /// majorant that the position-mean bound needs. With no open pair the prefix has no completion and the bound is
    /// `i128::MIN`.
    ///
    /// This prepares the node's part in `steps` (once per node; [`OrderSteps::reset`] starts a node). False without
    /// a point or score-target payoff, or at the root (`depth == 0`), where the node has no such bound.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn order_steps_prepare(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[[usize; 5]],
        keyed: Option<&Keyed>,
        steps: &mut OrderSteps,
    ) -> bool {
        if steps.ready {
            return true;
        }
        let Some(pt) = &self.points else { return false };
        if depth == 0 {
            return false;
        }
        let (power, _, bonus) = self.relax(pool, domain, p, depth, &SLOTS[depth..], &[0, 1, 2, 3, 4], keyed);
        let profile = self.profile[p.members[2]];
        let choices = Self::prefix_choices(domain, p, depth);
        let (mut placed_power, mut placed_bonus) = (0i64, 0i64);
        steps.characters.clear();
        steps.used.clear();
        steps.used.resize(domain.snaps().len() + 1, false);
        for &slot in &SLOTS[..depth] {
            let (m, choice) = (p.members[slot], choices[slot]);
            placed_power += self.pair_power(profile, m, choice);
            placed_bonus += pt.pair_bonus(m, choice);
            steps.characters.push(pool.members[m].character_id);
        }
        for (choice, used) in steps.used.iter_mut().enumerate().skip(1) {
            *used = SLOTS[..depth].iter().any(|&slot| p.snaps[slot] == Some(domain.snaps()[choice - 1]));
        }
        steps.gains.clear();
        steps.gains.extend(positions.iter().map(|order| {
            SLOTS[..depth].iter().fold(0.0, |gain, &slot| {
                add_up(
                    gain,
                    keyed.map_or(self.order_gains[p.members[slot]][choices[slot]][order[slot]], |placed| {
                        placed.order_placed[slot][order[slot]]
                    }),
                )
            })
        }));
        steps.depth = depth;
        steps.profile = profile;
        steps.caps = (power, bonus);
        steps.placed = (placed_power, placed_bonus);
        steps.a0 = keyed.map_or(self.a0, |k| k.a0);
        steps.first = usize::MAX;
        steps.ready = true;
        true
    }

    /// The power of a member and choice under a leader profile.
    fn pair_power(&self, profile: usize, m: usize, choice: usize) -> i64 {
        self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] }
    }

    /// The order-step bound of the prepared node's completions whose open slots take pairs of `open` (one pass over
    /// them). None on overflow.
    pub(crate) fn order_steps_open(
        &self,
        pool: &Pool,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        open: &[(usize, usize)],
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        let mut free = [0f64; 5];
        // The Pareto frontier of the open pairs in one pass while their bonuses do not increase (the candidate order
        // sorts by bonus first), else by a sort.
        let (mut ordered, mut any) = (true, false);
        let mut run = i64::MIN;
        steps.singles.clear();
        for &(m, choice) in open {
            if !steps.open(pool, m, choice) {
                continue;
            }
            any = true;
            for (position, g) in free.iter_mut().enumerate() {
                *g = g.max(self.order_gains[m][choice][position]);
            }
            let pair = (self.pair_power(steps.profile, m, choice), pt.pair_bonus(m, choice));
            let single = &mut steps.singles;
            if !ordered {
                single.push(pair);
                continue;
            }
            match single.last() {
                Some(&(_, b)) if pair.1 > b => {
                    ordered = false;
                    single.push(pair);
                }
                _ if pair.0 <= run => {}
                Some(&(_, b)) if pair.1 == b => *single.last_mut().expect("nonempty") = pair,
                _ => single.push(pair),
            }
            run = run.max(pair.0);
        }
        if !any {
            return Some((i128::MIN, i64::MIN));
        }
        if !ordered {
            steps.singles = pareto_pairs(std::mem::take(&mut steps.singles));
        }
        self.order_steps_total(pt, positions, steps, &free, None)
    }

    /// The order-step bound of the prepared node's completions whose open slots take pairs from candidate `offset`
    /// on. The first call of a node collects the suffixes from its offset on; later calls ask for later suffixes.
    pub(crate) fn order_steps_suffix(
        &self,
        pool: &Pool,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        offset: usize,
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        let free = self.order_steps_singles(pool, steps, offset);
        if steps.singles.is_empty() {
            return Some((i128::MIN, i64::MIN));
        }
        self.order_steps_total(pt, positions, steps, &free, None)
    }

    /// The order-step bound of the prepared node's completions whose next slot (`SLOTS[depth]`) takes the pair `m`,
    /// `choice` of candidate `offset` and whose other open slots take pairs from `offset + 1` on. The child's own
    /// character and Snap stay open to those slots, which only enlarges the completion set.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn order_steps_pair(
        &self,
        pool: &Pool,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        offset: usize,
        m: usize,
        choice: usize,
    ) -> Option<(i128, i64)> {
        let pt = self.points.as_ref()?;
        let free = self.order_steps_singles(pool, steps, offset + 1);
        if steps.singles.is_empty() {
            return Some((i128::MIN, i64::MIN));
        }
        self.order_steps_total(pt, positions, steps, &free, Some((m, choice)))
    }

    /// The largest per-position gains of the open pairs from `offset` on, with their (power, bonus) Pareto frontier
    /// in `steps.singles` (bonus descending). Collects the suffix summaries from `offset` on when the node has none
    /// that far back: one backward pass keeps the frontier of each suffix as the top of a persistent stack.
    fn order_steps_singles(&self, pool: &Pool, steps: &mut OrderSteps, offset: usize) -> [f64; 5] {
        let pt = self.points.as_ref().expect("prepared order steps");
        let n = self.choices.len();
        let offset = offset.min(n);
        if offset < steps.first {
            steps.first = offset;
            steps.free.clear();
            steps.free.resize(n + 1 - offset, [0.0; 5]);
            steps.top.clear();
            steps.top.resize(n + 1 - offset, NO_PAIR);
            steps.stack.clear();
            let (mut free, mut top) = ([0f64; 5], NO_PAIR);
            for i in (offset..n).rev() {
                let (m, choice) = self.choices[i];
                if steps.open(pool, m, choice) {
                    for (position, g) in free.iter_mut().enumerate() {
                        *g = g.max(self.order_gains[m][choice][position]);
                    }
                    let (power, bonus) = (self.pair_power(steps.profile, m, choice), pt.pair_bonus(m, choice));
                    // Pairs come in nondecreasing bonus order: the new one removes the frontier points it dominates
                    // from the top, unless the top dominates it. Out of that order the stack may keep dominated
                    // points too, which no bound reads as better than the frontier.
                    let mut below = top;
                    while below != NO_PAIR {
                        let (p, b, parent) = steps.stack[below as usize];
                        if p > power || b > bonus {
                            break;
                        }
                        below = parent;
                    }
                    top = match steps.stack.get(below as usize) {
                        Some(&(p, b, _)) if p >= power && b >= bonus => below,
                        _ => {
                            steps.stack.push((power, bonus, below));
                            (steps.stack.len() - 1) as u32
                        }
                    };
                }
                steps.free[i - offset] = free;
                steps.top[i - offset] = top;
            }
        }
        steps.singles.clear();
        let (mut at, mut ordered) = (steps.top[offset - steps.first], true);
        while at != NO_PAIR {
            let (power, bonus, parent) = steps.stack[at as usize];
            if let Some(&(p, b)) = steps.singles.last() {
                ordered &= bonus < b && power > p;
            }
            steps.singles.push((power, bonus));
            at = parent;
        }
        if !ordered {
            steps.singles = pareto_pairs(std::mem::take(&mut steps.singles));
        }
        steps.free[offset - steps.first]
    }

    /// The order-step bound from the prepared node, an optional child pair of the next slot and the open pairs'
    /// per-position gains (`free`) and frontier (`steps.singles`).
    fn order_steps_total(
        &self,
        pt: &PointBound,
        positions: &[[usize; 5]],
        steps: &mut OrderSteps,
        free: &[f64; 5],
        child: Option<(usize, usize)>,
    ) -> Option<(i128, i64)> {
        let (power, bonus) = steps.caps;
        let (mut placed_power, mut placed_bonus) = steps.placed;
        let mut placed = steps.depth;
        if let Some((m, choice)) = child {
            placed_power += self.pair_power(steps.profile, m, choice);
            placed_bonus += pt.pair_bonus(m, choice);
            placed += 1;
        }
        steps.sums.clear();
        steps.sums.push((placed_power, placed_bonus));
        for _ in placed..5 {
            let singles = &steps.singles;
            let cross = steps.sums.iter().flat_map(|&(a, b)| singles.iter().map(move |&(c, d)| (a + c, b + d)));
            steps.sums = pareto_pairs(cross.collect());
        }
        for sum in &mut steps.sums {
            *sum = (sum.0.min(power), sum.1.min(bonus));
        }
        let child_slot = SLOTS[steps.depth];
        let (a0, global, eps) = (steps.a0, self.global, self.eps);
        steps.factors.clear();
        steps.factors.extend(positions.iter().zip(&steps.gains).map(|(order, &placed_gain)| {
            let mut gain = placed_gain;
            if let Some((m, choice)) = child {
                gain = add_up(gain, self.order_gains[m][choice][order[child_slot]]);
            }
            for &slot in &SLOTS[placed..] {
                gain = add_up(gain, free[order[slot]]);
            }
            add_up(a0, gain).min(global) * (1.0 + eps)
        }));
        Some((pt.order_payoff_sum(&steps.sums, &steps.factors, &mut steps.steps)?, power))
    }
}
