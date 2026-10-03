//! Envelopes for member compositions before physical layouts and Snap bindings.
use super::*;

#[derive(Clone, Copy, Default)]
struct Envelope {
    power: i64,
    gain: f64,
    bonus: i64,
    weighted: [f64; 3],
}
impl Envelope {
    fn merge(&mut self, r: Self) {
        self.power = self.power.max(r.power);
        self.gain = self.gain.max(r.gain);
        self.bonus = self.bonus.max(r.bonus);
        for i in 0..3 {
            self.weighted[i] = self.weighted[i].max(r.weighted[i]);
        }
    }
    fn add(&mut self, r: Self) {
        self.power += r.power;
        self.gain = add_up(self.gain, r.gain);
        self.bonus += r.bonus;
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
    let top_int = |f: fn(&Envelope) -> i64| {
        let mut v: Vec<_> = rows.iter().map(f).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.into_iter().take(take).sum::<i64>()
    };
    let top_float = |f: &dyn Fn(&Envelope) -> f64| {
        let mut v: Vec<_> = rows.iter().map(f).collect();
        v.sort_by(|a, b| b.total_cmp(a));
        v.into_iter().take(take).fold(0.0, add_up)
    };
    total.power += top_int(|r| r.power);
    total.bonus += top_int(|r| r.bonus);
    total.gain = add_up(total.gain, top_float(&|r| r.gain));
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
                                    bonus,
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
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        k: usize,
    ) -> Vec<PhysicalDeck> {
        super::super::matching::best_k_assignments(p.members.map(|m| self.w[m].as_slice()), k)
            .into_iter()
            .map(|(_, binding)| PhysicalDeck {
                members: p.members,
                snaps: binding.map(|s| s.map(|j| domain.snaps()[j])),
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
        let mut used = HashSet::new();
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
        let characters: HashSet<_> =
            SLOTS[..member_depth].iter().map(|&s| pool.members[p.members[s]].character_id).collect();
        let used: HashSet<_> = SLOTS[..snap_depth].iter().filter_map(|&s| p.snaps[s]).collect();
        let row = |m: usize, choice: usize, slot: Option<usize>| {
            let power = self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] };
            let gain = if let Some(slot) = slot {
                self.gains[m][choice][positions[slot]]
            } else {
                SLOTS[1..].iter().map(|&s| self.gains[m][choice][positions[s]]).fold(0.0, f64::max)
            };
            let bonus =
                self.points.as_ref().map_or(0, |pt| pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] });
            Envelope {
                power,
                gain,
                bonus,
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
        let mut remaining = HashMap::<i64, Envelope>::new();
        if member_depth < 5 {
            for &m in domain.members() {
                let c = pool.members[m].character_id;
                if !characters.contains(&c) {
                    remaining.entry(c).or_default().merge(best(m, None));
                }
            }
        }
        add_remaining(&mut total, &remaining.into_values().collect::<Vec<_>>(), 5 - member_depth);
        self.composition_cap(total, power_cap)
    }
    fn composition_cap(&self, total: Envelope, power_cap: Option<i64>) -> (i128, i64) {
        let power = power_cap.map_or(total.power, |v| v.min(total.power));
        let mut score =
            ((power as f64) * add_up(self.a0, total.gain).min(self.global) * (1.0 + self.eps)).ceil() as i128;
        for (i, &r) in self.correlation_scales.iter().enumerate() {
            let w = add_up(total.weighted[i], (r * self.a0).next_up());
            score = score.min(
                ((((w * w).next_up() / (4.0 * r)).next_up() * (1.0 + self.eps).next_up()).next_up().ceil()) as i128,
            );
        }
        (
            self.points.as_ref().map_or(score, |pt| ((10000 + total.bonus) * pt.multiplier_at(score) / 10000) as i128),
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
        let mut total = 0i128;
        let mut power = 0;
        for (positions, weight) in orders {
            let (cap, p) =
                self.composition_upper_at(pool, domain, p, member_depth, snap_depth, free_order, positions, power_cap);
            power = p;
            total = total
                .checked_add(
                    cap.checked_mul(i128::try_from(*weight).map_err(|_| unavailable("composition mass overflow"))?)
                        .ok_or_else(|| unavailable("composition product overflow"))?,
                )
                .ok_or_else(|| unavailable("composition sum overflow"))?;
        }
        Ok((total, power))
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
