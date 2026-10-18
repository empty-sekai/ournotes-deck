//! Envelopes for member compositions before physical layouts and Snap bindings.
use super::*;
use crate::types::MAX_METRIC_TERMS;

#[derive(Clone, Copy, Default)]
struct Envelope {
    power: i64,
    gain: f64,
    /// At least the spread of the gain sum (see `JointBounds::spread`).
    spread: f64,
    bonus: i64,
    /// At least the event bonus under each other term of a combined metric's point bound, in the terms' order, of
    /// every deck the envelope covers. Only the first `n_others` entries are read; a term past them reads its largest
    /// bonus of a team. The composition tables carry none.
    others: [i64; MAX_METRIC_TERMS],
    n_others: u8,
    weighted: [f64; 3],
}
impl Envelope {
    fn merge(&mut self, r: Self) {
        self.power = self.power.max(r.power);
        self.gain = self.gain.max(r.gain);
        self.spread = self.spread.max(r.spread);
        self.bonus = self.bonus.max(r.bonus);
        self.n_others = self.n_others.max(r.n_others);
        for i in 0..usize::from(self.n_others) {
            self.others[i] = self.others[i].max(r.others[i]);
        }
        for i in 0..3 {
            self.weighted[i] = self.weighted[i].max(r.weighted[i]);
        }
    }
    fn add(&mut self, r: Self) {
        self.power += r.power;
        self.gain = add_up(self.gain, r.gain);
        self.spread = add_up(self.spread, r.spread);
        self.bonus += r.bonus;
        self.n_others = self.n_others.max(r.n_others);
        for i in 0..usize::from(self.n_others) {
            self.others[i] += r.others[i];
        }
        for i in 0..3 {
            self.weighted[i] = add_up(self.weighted[i], r.weighted[i]);
        }
    }
}
struct Layer {
    member: Vec<Envelope>,
    leader: Vec<Envelope>,
    characters: Vec<(i64, Envelope)>,
}
pub(super) struct CompositionTables {
    layers: Vec<[Layer; 5]>,
}
fn add_remaining(total: &mut Envelope, rows: &[Envelope], take: usize) {
    let top_int = |f: &dyn Fn(&Envelope) -> i64| {
        let mut v: Vec<_> = rows.iter().map(f).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.into_iter().take(take).sum::<i64>()
    };
    let top_float = |f: &dyn Fn(&Envelope) -> f64| {
        let mut v: Vec<_> = rows.iter().map(f).collect();
        v.sort_by(|a, b| b.total_cmp(a));
        v.into_iter().take(take).fold(0.0, add_up)
    };
    total.power += top_int(&|r| r.power);
    total.bonus += top_int(&|r| r.bonus);
    let n_others = rows.iter().map(|r| r.n_others).fold(total.n_others, u8::max);
    for i in 0..usize::from(n_others) {
        total.others[i] += top_int(&|r| r.others[i]);
    }
    total.n_others = n_others;
    total.gain = add_up(total.gain, top_float(&|r| r.gain));
    total.spread = add_up(total.spread, top_float(&|r| r.spread));
    for i in 0..3 {
        total.weighted[i] = add_up(total.weighted[i], top_float(&|r| r.weighted[i]));
    }
}
impl CompositionTables {
    pub(super) fn compile(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        if b.lead.len().saturating_mul(5).saturating_mul(pool.members.len()) > 100_000 {
            return None;
        }
        let layers = (0..b.lead.len())
            .map(|profile| {
                std::array::from_fn(|leader_pos| {
                    let mut member = vec![Envelope::default(); pool.members.len()];
                    let mut leader = member.clone();
                    let mut characters = std::collections::BTreeMap::<i64, Envelope>::new();
                    for &m in domain.members() {
                        for choice in 0..=domain.snaps().len() {
                            let power = b.a[m] + b.lead[profile][m] + if choice == 0 { 0 } else { b.w[m][choice - 1] };
                            let bonus = b
                                .points
                                .as_ref()
                                .map_or(0, |pt| pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
                            for (is_leader, target) in [(false, &mut member[m]), (true, &mut leader[m])] {
                                let gain = if is_leader {
                                    b.gains[m][choice][leader_pos]
                                } else {
                                    (0..5)
                                        .filter(|&p| p != leader_pos)
                                        .map(|p| b.gains[m][choice][p])
                                        .fold(0.0, f64::max)
                                };
                                target.merge(Envelope {
                                    power,
                                    gain,
                                    spread: b.spread[m][choice],
                                    bonus,
                                    others: [0; MAX_METRIC_TERMS],
                                    n_others: 0,
                                    weighted: b.correlation_scales.map(|r| add_up(power as f64, (r * gain).next_up())),
                                });
                            }
                        }
                        characters.entry(pool.members[m].character_id).or_default().merge(member[m]);
                    }
                    Layer { member, leader, characters: characters.into_iter().collect() }
                })
            })
            .collect();
        Some(Self { layers })
    }
    fn envelope(
        &self,
        b: &JointBounds,
        pool: &Pool,
        p: &PhysicalDeck,
        depth: usize,
        positions: &[usize; 5],
    ) -> Envelope {
        let layer = &self.layers[b.profile[p.members[2]]][positions[2]];
        let mut total = Envelope::default();
        let mut excluded = Vec::with_capacity(depth);
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            excluded.push(pool.members[m].character_id);
            total.add(if slot == 2 { layer.leader[m] } else { layer.member[m] });
        }
        let rows: Vec<_> = layer.characters.iter().filter(|(c, _)| !excluded.contains(c)).map(|(_, r)| *r).collect();
        add_remaining(&mut total, &rows, 5 - depth);
        total
    }
}
impl JointBounds {
    /// For a fixed member layout, leader/member-only terms are constant across
    /// bindings. The validated table's exact Snap increments therefore rank true
    /// power even when a complex leader profile's constant term is only bounded.
    pub(crate) fn layout_power_frontier(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        k: usize,
    ) -> Vec<PhysicalDeck> {
        // The composition uses heuristic member order, but the frontier's ties
        // must use public-ID canonical order before retaining only K bindings.
        let mut nonleader = super::super::uniform::NONLEADER;
        nonleader.sort_unstable_by_key(|&slot| pool.members[p.members[slot]].id);
        let slots = [nonleader[0], nonleader[1], 2, nonleader[2], nonleader[3]];
        super::super::matching::best_k_assignments(slots.map(|slot| self.w[p.members[slot]].as_slice()), k)
            .into_iter()
            .map(|(_, binding)| {
                // Keep proposals in the caller's layout for its local identity
                // set and for the remaining Snap traversal.
                let mut snaps = [None; 5];
                for (canonical_slot, &slot) in slots.iter().enumerate() {
                    snaps[slot] = binding[canonical_slot].map(|j| domain.snaps()[j]);
                }
                PhysicalDeck { members: p.members, snaps }
            })
            .collect()
    }
    /// Branch ordering only. Every allowed member appears exactly once.
    pub(crate) fn member_order(&self, domain: &CandidateDomain) -> Vec<usize> {
        let mut seen = HashSet::new();
        self.choices
            .iter()
            .filter_map(|&(m, _)| (domain.members().contains(&m) && seen.insert(m)).then_some(m))
            .collect()
    }

    /// Fixed-member additive power assignment. Returned bindings are proposals;
    /// numeric bounds use the value, and all equal canonical variants are recovered.
    pub(crate) fn layout_power(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        snap_depth: usize,
    ) -> (i64, PhysicalDeck) {
        let profile = self.profile[p.members[2]];
        let mut used = ournotes_sim::num::FxHashSet::default();
        let mut base = 0;
        for (depth, &slot) in SLOTS.iter().enumerate() {
            let m = p.members[slot];
            base += self.a[m] + self.lead[profile][m];
            if depth < snap_depth
                && let Some(s) = p.snaps[slot]
            {
                used.insert(s);
                base += self.w[m][domain.snaps().iter().position(|&v| v == s).expect("compiled Snap")];
            }
        }
        if snap_depth == 5 {
            return (base, *p);
        }
        let available: Vec<_> =
            domain.snaps().iter().enumerate().filter(|(_, s)| !used.contains(*s)).map(|(j, _)| j).collect();
        if snap_depth >= 3 {
            let weights: Vec<Vec<i64>> = SLOTS[snap_depth..]
                .iter()
                .map(|&slot| available.iter().map(|&j| self.w[p.members[slot]][j]).collect())
                .collect();
            let rows: Vec<_> = weights.iter().map(|v| v.as_slice()).collect();
            let (extra, binding) = super::super::matching::best_small_assignment(&rows);
            let mut out = *p;
            for (row, snap) in binding.into_iter().enumerate() {
                out.snaps[SLOTS[row + snap_depth]] = snap.map(|j| domain.snaps()[available[j]]);
            }
            return (base + extra, out);
        }
        let weights: [Vec<i64>; 5] = std::array::from_fn(|row| {
            if row + snap_depth >= 5 {
                vec![0; available.len()]
            } else {
                let m = p.members[SLOTS[row + snap_depth]];
                available.iter().map(|&j| self.w[m][j]).collect()
            }
        });
        let (extra, binding) = super::super::matching::best_assignment(weights.each_ref().map(|v| v.as_slice()));
        let mut out = *p;
        for row in 0..5 - snap_depth {
            out.snaps[SLOTS[row + snap_depth]] = binding[row].map(|j| domain.snaps()[available[j]]);
        }
        (base + extra, out)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composition_upper_at(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        member_depth: usize,
        snap_depth: usize,
        free_order: bool,
        positions: &[usize; 5],
        power_cap: Option<i64>,
    ) -> (i128, i64) {
        debug_assert!(member_depth >= 1 && snap_depth <= member_depth && (!free_order || snap_depth == 0));
        if free_order && let Some(table) = &self.composition {
            return self.composition_cap(table.envelope(self, pool, p, member_depth, positions), power_cap);
        }
        let profile = self.profile[p.members[2]];
        let characters: ournotes_sim::num::FxHashSet<_> =
            SLOTS[..member_depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let used: ournotes_sim::num::FxHashSet<_> = SLOTS[..snap_depth].iter().filter_map(|&s| p.snaps[s]).collect();
        // The other terms of a combined metric, in their order, whose event bonuses the envelope bounds per slot.
        let other_terms: &[(i128, i64, PointBound)] = self.points.as_ref().map_or(&[][..], |pt| pt.others.as_slice());
        let n_others = other_terms.len().min(MAX_METRIC_TERMS) as u8;
        let row = |m: usize, choice: usize, slot: Option<usize>| {
            let power = self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
            let gain = if let Some(slot) = slot {
                self.gains[m][choice][positions[slot]]
            } else {
                SLOTS[1..].iter().map(|&s| self.gains[m][choice][positions[s]]).fold(0.0, f64::max)
            };
            let bonus =
                self.points.as_ref().map_or(0, |pt| pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
            let others =
                std::array::from_fn(|i| other_terms.get(i).map_or(0, |(_, _, part)| part.pair_bonus(m, choice)));
            Envelope {
                power,
                gain,
                spread: self.spread[m][choice],
                bonus,
                others,
                n_others,
                weighted: self.correlation_scales.map(|r| add_up(power as f64, (r * gain).next_up())),
            }
        };
        let best = |m: usize, slot: Option<usize>| {
            let mut v = row(m, 0, slot);
            for (j, &s) in domain.snaps().iter().enumerate() {
                if !used.contains(&s) {
                    v.merge(row(m, j + 1, slot));
                }
            }
            v
        };
        let mut total = Envelope::default();
        for (depth, &slot) in SLOTS[..member_depth].iter().enumerate() {
            let m = p.members[slot];
            let position = if free_order && slot != 2 { None } else { Some(slot) };
            if depth < snap_depth {
                let choice = p.snaps[slot]
                    .map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
                total.add(row(m, choice, position));
            } else {
                total.add(best(m, position));
            }
        }
        let mut remaining = ournotes_sim::num::FxHashMap::<i64, Envelope>::default();
        if member_depth < 5 {
            for &m in domain.members() {
                let c = pool.members[m].character_id;
                if !characters.contains(&c) {
                    remaining.entry(c).or_default().merge(best(m, None));
                }
            }
        }
        add_remaining(&mut total, &remaining.into_values().collect::<Vec<_>>(), 5 - member_depth);
        // A team's event bonus under a term is at most the term's largest bonus of a team.
        if let Some(pt) = &self.points {
            for (i, (_, largest, _)) in pt.others.iter().enumerate().take(MAX_METRIC_TERMS) {
                total.others[i] = total.others[i].min(*largest);
            }
        }
        self.composition_cap(total, power_cap)
    }
    fn composition_cap(&self, total: Envelope, power_cap: Option<i64>) -> (i128, i64) {
        let power = power_cap.map_or(total.power, |v| v.min(total.power));
        let cap =
            |gain: f64| ((power as f64) * add_up(self.a0, gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
        let mut score = cap(total.gain);
        for (i, &r) in self.correlation_scales.iter().enumerate() {
            let w = add_up(total.weighted[i], (r * self.a0).next_up());
            score = score.min(
                ((((w * w).next_up() / (4.0 * r)).next_up() * (1.0 + self.eps).next_up()).next_up().ceil()) as i128,
            );
        }
        (
            self.points.as_ref().map_or(score, |pt| {
                pt.mean_payoff_with(
                    total.bonus,
                    &total.others[..usize::from(total.n_others)],
                    score,
                    cap(add_up(total.gain, total.spread)),
                )
            }),
            power,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composition_expected_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        member_depth: usize,
        snap_depth: usize,
        free_order: bool,
        orders: &[([usize; 5], u128)],
    ) -> Result<(i128, i64), Error> {
        let power_cap = (member_depth == 5).then(|| self.layout_power(domain, p, snap_depth).0);
        // The fold is non-decreasing in every recovery: with none it bounds no team below the least final life, and
        // with the largest at every position it bounds every order of every team.
        let life = self
            .node_life(pool, domain, p, member_depth, snap_depth, free_order)
            .filter(|&(fine, least, _)| fine.final_life_reach([0; 5]) < least);
        if let Some((fine, least, recovery)) = life
            && fine.final_life_reach([recovery.into_iter().max().unwrap_or(0); 5]) < least
            && let Some((positions, _)) = orders.last()
        {
            let (_, power) =
                self.composition_upper_at(pool, domain, p, member_depth, snap_depth, free_order, positions, power_cap);
            return Ok((0, power));
        }
        let mut short: Vec<([i64; 5], bool)> = Vec::new();
        let mut total = 0i128;
        let mut power = 0;
        for (positions, weight) in orders {
            let (mut cap, p) =
                self.composition_upper_at(pool, domain, p, member_depth, snap_depth, free_order, positions, power_cap);
            power = p;
            if let Some((fine, least, recovery)) = life {
                let mut rec = [0i64; 5];
                for (&position, &r) in positions.iter().zip(&recovery) {
                    rec[position] = r;
                }
                let is_short = match short.iter().find(|(r, _)| *r == rec) {
                    Some(&(_, s)) => s,
                    None => {
                        let s = fine.final_life_reach(rec) < least;
                        short.push((rec, s));
                        s
                    }
                };
                if is_short {
                    cap = 0;
                }
            }
            total = total
                .checked_add(
                    cap.checked_mul(i128::try_from(*weight).map_err(|_| unavailable("composition mass overflow"))?)
                        .ok_or_else(|| unavailable("composition product overflow"))?,
                )
                .ok_or_else(|| unavailable("composition sum overflow"))?;
        }
        Ok((total, power))
    }

    /// For a score and life target: the fine bounds, its least final life and, per slot of `p`, the most life the
    /// slot's performer recovers at its own skill events in any team of the node (`member_depth` members and the
    /// Snaps of `SLOTS[..snap_depth]` assigned; with `free_order` the nonleader slots take their largest value, their
    /// positions being free). None when some team of the node may have another life-raising row.
    fn node_life(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        member_depth: usize,
        snap_depth: usize,
        free_order: bool,
    ) -> Option<(&JointFineBounds, i64, [i64; 5])> {
        let least = self.min_final_life?;
        let fine = self.fine.as_ref()?;
        let used: ournotes_sim::num::FxHashSet<_> = SLOTS[..snap_depth].iter().filter_map(|&s| p.snaps[s]).collect();
        let open = |m: usize| {
            let snaps = domain.snaps().iter().enumerate().filter(|(_, s)| !used.contains(s)).map(|(j, _)| j + 1);
            fine.recovery_reach(m, std::iter::once(0).chain(snaps))
        };
        let mut recovery = [0i64; 5];
        for (depth, &slot) in SLOTS[..member_depth].iter().enumerate() {
            let m = p.members[slot];
            recovery[slot] = if depth < snap_depth {
                let choice = p.snaps[slot]
                    .map_or(0, |s| domain.snaps().iter().position(|&v| v == s).expect("compiled Snap") + 1);
                fine.recovery_reach(m, [choice])?
            } else {
                open(m)?
            };
        }
        if member_depth < 5 {
            let characters: ournotes_sim::num::FxHashSet<_> =
                SLOTS[..member_depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
            let mut rest = 0i64;
            for &m in domain.members() {
                if !characters.contains(&pool.members[m].character_id) {
                    rest = rest.max(open(m)?);
                }
            }
            for &slot in &SLOTS[member_depth..] {
                recovery[slot] = rest;
            }
        }
        if free_order {
            let most = SLOTS[1..].iter().map(|&s| recovery[s]).max().unwrap_or(0);
            for &slot in &SLOTS[1..] {
                recovery[slot] = most;
            }
        }
        Some((fine, least, recovery))
    }

    pub(crate) fn slot_choices(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        slot: usize,
        orders: &[([usize; 5], u128)],
    ) -> Vec<usize> {
        let m = p.members[slot];
        let profile = self.profile[p.members[2]];
        let mass = orders.iter().map(|(_, w)| *w as f64).sum::<f64>();
        let priority = |choice: usize| {
            let power = self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
            if let Some(pt) = &self.points {
                return (if choice == 0 { 0 } else { pt.snap[choice - 1] }) as f64 * 1e12 + power as f64;
            }
            let gain = orders.iter().map(|(pos, w)| self.gains[m][choice][pos[slot]] * (*w as f64 / mass)).sum::<f64>();
            power as f64 * (self.a0 / 5.0 + gain)
        };
        let mut choices: Vec<_> = (0..=domain.snaps().len()).collect();
        choices.sort_by(|&a, &b| priority(b).total_cmp(&priority(a)).then(a.cmp(&b)));
        choices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bounds whose score caps are zero, holding `points` as the point bound.
    fn bounds(points: PointBound) -> JointBounds {
        JointBounds {
            a: vec![0; 5],
            w: vec![Vec::new(); 5],
            lead: vec![vec![0; 5]],
            profile: vec![0; 5],
            gains: vec![vec![[0.0; 5]; 1]; 5],
            order_gains: vec![vec![[0.0; 5]; 1]; 5],
            spread: vec![vec![0.0; 1]; 5],
            spread_top: [0.0; 6],
            column: [0.0; 5],
            a0: 0.0,
            global: 1.0,
            eps: 0.0,
            points: Some(points),
            min_final_life: None,
            fine: None,
            correlation_scales: [1.0; 3],
            tails: None,
            composition: None,
            modules: Vec::new(),
            gekisou: false,
            class_search: false,
            class_resource_caps: false,
            prefix_resource: None,
            prefix_character: None,
            rules: None,
            relax_tables: None,
            forced_tables: Default::default(),
            step_pairs: None,
            choices: Vec::new(),
            carrier_levels: None,
            carrier_split: None,
            laws: Vec::new(),
            law_clusters: None,
            part: None,
            order_potentials: None,
            order_offset: 0.0,
            root_keys: None,
        }
    }

    /// A bounded payoff of weight 2 plus one other term of weight 3 and largest bonus 1500.
    fn combined() -> PointBound {
        let mut own = PointBound::plain(vec![0; 5], Vec::new(), 100, None, None, None);
        own.weight = 2;
        own.others =
            vec![(3, 1_500, PointBound::plain(vec![100, 200, 300, 400, 500], Vec::new(), 100, None, None, None))];
        own
    }

    #[test]
    fn a_fixed_composition_caps_each_other_term_at_the_teams_own_bonus() {
        let pt = combined();
        let bound = bounds(pt.clone());
        let (own, other) = (700, 900);
        let mut total = Envelope { bonus: own, n_others: 1, ..Envelope::default() };
        total.others[0] = other;
        let (tight, _) = bound.composition_cap(total, None);
        let (loose, _) = bound.composition_cap(Envelope { bonus: own, ..Envelope::default() }, None);
        assert_eq!(tight, pt.mean_payoff_with(own, &[other], 0, 0));
        assert_eq!(loose, pt.mean_payoff(own, 0, 0));
        assert!(tight < loose);
    }

    #[test]
    fn other_term_bonuses_merge_sum_and_take_the_top_slots() {
        let mut a = Envelope { n_others: 2, ..Envelope::default() };
        a.others[0] = 7;
        a.others[1] = 5;
        let mut b = Envelope { n_others: 2, ..Envelope::default() };
        b.others[0] = 9;
        b.others[1] = 2;
        a.merge(b);
        let mut total = Envelope::default();
        total.add(a);
        assert_eq!(&total.others[..2], &[9, 5]);
        let mut rows = [Envelope::default(); 3];
        for (row, values) in rows.iter_mut().zip([[1, 5], [4, 0], [2, 3]]) {
            row.n_others = 2;
            row.others[0] = values[0];
            row.others[1] = values[1];
        }
        add_remaining(&mut total, &rows, 2);
        assert_eq!(&total.others[..2], &[9 + 4 + 2, 5 + 5 + 3]);
        assert_eq!(total.n_others, 2);
    }
}
