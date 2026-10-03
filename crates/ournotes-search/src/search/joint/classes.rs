//! Resource bounds over fine-bound-equivalent Snap choices. Actual candidate
//! scores are never replaced by class representatives or assumed monotone in power.
use super::*;

pub(crate) struct ClassBound {
    pub payoff: i128,
    pub power: i64,
    pub proposal: PhysicalDeck,
    pub resource_checks: u64,
    pub resource_tightened: u64,
}
impl JointBounds {
    /// Feasible incumbent proposals only. The linearized power/gain objective is
    /// never used as a replacement score or as an exclusion certificate.
    pub(crate) fn weighted_layout_seeds(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        orders: &[([usize; 5], u128)],
    ) -> Vec<PhysicalDeck> {
        if domain.snaps().len() > 4096 {
            return Vec::new();
        }
        let profile = self.profile[p.members[2]];
        let mass = orders.iter().map(|(_, w)| *w as f64).sum::<f64>();
        let gains: [Vec<f64>; 5] = std::array::from_fn(|slot| {
            (0..=domain.snaps().len())
                .map(|choice| {
                    orders
                        .iter()
                        .map(|(pos, w)| self.gains[p.members[slot]][choice][pos[slot]] * (*w as f64 / mass))
                        .sum()
                })
                .collect()
        });
        let max_gain = gains.iter().map(|r| r.iter().copied().fold(0.0, f64::max)).sum::<f64>();
        let power = self.layout_power(domain, p, 0).0;
        let scale = (power as f64 / (self.a0 + max_gain).max(1e-100)).clamp(1e-100, 1e100);
        let mut proposals = Vec::new();
        for r in [scale * 0.5, scale, scale * 2.0] {
            let mut weights: [Vec<i64>; 5] = std::array::from_fn(|_| Vec::new());
            let mut valid = true;
            for slot in 0..5 {
                let m = p.members[slot];
                let base = self.a[m] + self.lead[profile][m];
                let none = base as f64 + r * gains[slot][0];
                for choice in 1..gains[slot].len() {
                    let value = (base + self.w[m][choice - 1]) as f64 + r * gains[slot][choice] - none;
                    if !value.is_finite() || value.abs() > 1e15 {
                        valid = false;
                        break;
                    }
                    weights[slot].push(value.round() as i64);
                }
                if !valid {
                    break;
                }
            }
            if !valid {
                continue;
            }
            let (_, binding) = super::super::matching::best_assignment(weights.each_ref().map(|v| v.as_slice()));
            let proposal = PhysicalDeck { members: p.members, snaps: binding.map(|s| s.map(|j| domain.snaps()[j])) };
            if !proposals.contains(&proposal) {
                proposals.push(proposal);
            }
        }
        proposals
    }
    pub(crate) fn effect_groups(&self, member: usize, ordered: &[usize]) -> Vec<Vec<usize>> {
        let fine = self.fine.as_ref().expect("class search requires fine metadata");
        let mut ids = Vec::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &choice in ordered {
            let id = fine.choice_class(member, choice);
            if let Some(at) = ids.iter().position(|&v| v == id) {
                groups[at].push(choice);
            } else {
                ids.push(id);
                groups.push(vec![choice]);
            }
        }
        groups
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn class_bound(
        &self,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        allowed: &[Vec<usize>; 5],
        orders: &[([usize; 5], u128)],
        complete_classes: bool,
        scratch: &mut JointScratch,
    ) -> Result<Option<ClassBound>, Error> {
        debug_assert!(self.points.is_none());
        if allowed.iter().any(Vec::is_empty) {
            return Ok(None);
        }
        let ns = domain.snaps().len();
        let masks: [Vec<bool>; 5] = std::array::from_fn(|slot| {
            let mut row = vec![false; ns];
            for &choice in &allowed[slot] {
                if choice > 0 {
                    row[choice - 1] = true;
                }
            }
            row
        });
        let none = std::array::from_fn(|slot| allowed[slot].contains(&0));
        let weights = p.members.map(|m| self.w[m].as_slice());
        let Some((extra, binding)) =
            super::super::matching::constrained_assignment(weights, masks.each_ref().map(|r| r.as_slice()), none)
        else {
            return Ok(None);
        };
        let profile = self.profile[p.members[2]];
        let power = extra + p.members.iter().map(|&m| self.a[m] + self.lead[profile][m]).sum::<i64>();
        let proposal = PhysicalDeck { members: p.members, snaps: binding.map(|s| s.map(|j| domain.snaps()[j])) };
        let representatives = std::array::from_fn(|slot| allowed[slot][0]);
        if complete_classes {
            let fine = self.fine.as_ref().expect("fine classes");
            debug_assert!((0..5).all(|slot| {
                allowed[slot].iter().all(|&c| {
                    fine.choice_class(p.members[slot], c) == fine.choice_class(p.members[slot], representatives[slot])
                })
            }));
        }
        let mut total = 0i128;
        let (mut resource_checks, mut resource_tightened) = (0, 0);
        for (positions, mass) in orders {
            let gain = (0..5).fold(0.0, |sum, slot| {
                add_up(
                    sum,
                    allowed[slot].iter().map(|&c| self.gains[p.members[slot]][c][positions[slot]]).fold(0.0, f64::max),
                )
            });
            let mut cap = self.payoff_cap(power, gain, 0);
            if complete_classes {
                cap = cap.min(self.fine.as_ref().expect("fine classes").upper(
                    power,
                    p.members,
                    representatives,
                    positions,
                    scratch,
                    None,
                ) as i128);
            }
            if self.class_resource_caps {
                let rows = std::array::from_fn(|slot| {
                    let m = p.members[slot];
                    (0..=ns)
                        .map(|choice| {
                            (
                                self.a[m] + self.lead[profile][m] + if choice == 0 { 0 } else { self.w[m][choice - 1] },
                                self.gains[m][choice][positions[slot]],
                            )
                        })
                        .collect()
                });
                let scale = (power as f64 / add_up(self.a0, gain).max(1e-100)).clamp(1e-90, 1e90);
                let resource = super::resource::product_upper(
                    &rows,
                    &masks,
                    none,
                    self.a0,
                    self.eps,
                    [scale * 0.5, scale, scale * 2.0],
                );
                resource_checks += 1;
                resource_tightened += u64::from(resource < cap);
                cap = cap.min(resource);
            }
            total = total
                .checked_add(
                    cap.checked_mul(i128::try_from(*mass).map_err(|_| unavailable("class mass overflow"))?)
                        .ok_or_else(|| unavailable("class product overflow"))?,
                )
                .ok_or_else(|| unavailable("class sum overflow"))?;
        }
        Ok(Some(ClassBound { payoff: total, power, proposal, resource_checks, resource_tightened }))
    }
}
