//! The point and score-step parts of the joint bounds.

use super::*;

impl JointBounds {
    /// The per-order payoff steps of a score target or of points stepped by the local score, with this envelope's
    /// global score cap (`payoff_cap_from` with every gain at the global coefficient); None for score payoffs, capped
    /// scores, multiplayer rewards without score tiers and combined metrics.
    pub(crate) fn score_steps(&self) -> Option<crate::search::deck_payoff::ScoreSteps> {
        self.steps_of(self.points.as_ref().filter(|pt| !pt.is_combined())?)
    }

    /// The steps of every term of a combined metric with the term's weight and event bonuses; None when a term has
    /// no steps.
    pub(crate) fn term_steps(&self) -> Option<Vec<crate::search::deck_payoff::TermSteps<'_>>> {
        let pt = self.points.as_ref()?;
        std::iter::once((pt.weight, pt))
            .chain(pt.others.iter().map(|(weight, _, part)| (*weight, part)))
            .map(|(weight, part)| Some((weight, part.member.as_slice(), part.snap.as_slice(), self.steps_of(part)?)))
            .collect()
    }

    /// The steps of one term's own payoff.
    fn steps_of(&self, pt: &PointBound) -> Option<crate::search::deck_payoff::ScoreSteps> {
        use crate::search::deck_payoff::Step;
        let steps = match (pt.target, &pt.score_tiers) {
            (Some(ScoreTarget::AtLeast(threshold)), _) => {
                vec![(i128::MIN, Step::Target(false)), (i128::from(threshold), Step::Target(true))]
            }
            (Some(ScoreTarget::Capped(_)), _) | (None, None) => return None,
            // `multiplier_at`: the prefix maximum of the tiers at or below a score.
            (None, Some(tiers)) => {
                let mut best = 0;
                tiers
                    .iter()
                    .map(|&(score, multiplier)| {
                        best = best.max(multiplier);
                        (i128::from(score), Step::Points(best))
                    })
                    .collect()
            }
        };
        Some(crate::search::deck_payoff::ScoreSteps::new(self.global, self.eps, steps))
    }

    /// Every deck containing member m earns at most this many PT in every performance order.
    /// Distinct-character bonus maxima and distinct-Snap maxima are independent,
    /// so ignoring their pairing, leader and required-member conflicts is optimistic.
    pub(crate) fn member_pt_caps(&self, pool: &Pool, domain: &CandidateDomain) -> Option<Vec<(usize, i128)>> {
        let pt = self.points.as_ref()?;
        if pt.target.is_some() {
            return None;
        }
        let mut chars = HashMap::<i64, i64>::new();
        for &m in domain.members() {
            let v = chars.entry(pool.members[m].character_id).or_default();
            *v = (*v).max(pt.member[m]);
        }
        let mut snaps = pt.snap.clone();
        snaps.sort_unstable_by(|a, b| b.cmp(a));
        let snap_bonus: i64 = snaps.iter().take(5).sum();
        Some(
            domain
                .members()
                .iter()
                .map(|&m| {
                    let mut rest: Vec<_> =
                        chars.iter().filter(|(c, _)| **c != pool.members[m].character_id).map(|(_, b)| *b).collect();
                    rest.sort_unstable_by(|a, b| b.cmp(a));
                    let bonus = pt.member[m] + snap_bonus + rest.iter().take(4).sum::<i64>();
                    (m, pt.maximum(bonus))
                })
                .collect(),
        )
    }

    /// Optimistic total event bonus for a legal prefix, including all still-required
    /// members. Used to seed from the maximum-bonus regime; score is evaluated exactly.
    pub(crate) fn bonus_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> Option<i64> {
        let pt = self.points.as_ref()?;
        if pt.target.is_some() {
            return None;
        }
        let mut chars = HashSet::new();
        let mut used = HashSet::new();
        let mut sum = 0;
        for &slot in &SLOTS[..depth] {
            let m = p.members[slot];
            chars.insert(pool.members[m].character_id);
            sum += pt.member[m];
            if let Some(s) = p.snaps[slot] {
                used.insert(s);
                sum += pt.snap[domain.snaps().iter().position(|&v| v == s).expect("compiled Snap")];
            }
        }
        let mut count = 0;
        for &m in domain.required() {
            if !SLOTS[..depth].iter().any(|&slot| p.members[slot] == m) {
                chars.insert(pool.members[m].character_id);
                sum += pt.member[m];
                count += 1;
            }
        }
        let mut remaining = HashMap::<i64, i64>::new();
        for &m in domain.members() {
            let c = pool.members[m].character_id;
            if !chars.contains(&c) {
                let row = remaining.entry(c).or_default();
                *row = (*row).max(pt.member[m]);
            }
        }
        let mut bonuses: Vec<_> = remaining.into_values().collect();
        bonuses.sort_unstable_by(|a, b| b.cmp(a));
        sum += bonuses.iter().take((5 - depth).saturating_sub(count)).sum::<i64>();
        let mut snaps: Vec<_> =
            domain.snaps().iter().enumerate().filter(|(_, s)| !used.contains(*s)).map(|(j, _)| pt.snap[j]).collect();
        snaps.sort_unstable_by(|a, b| b.cmp(a));
        sum += snaps.iter().take(5 - depth).sum::<i64>();
        Some(sum)
    }

    pub(crate) fn qualifying_pt_domain(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        numerator: i128,
        mass: u128,
    ) -> Result<Option<CandidateDomain>, Error> {
        let Some(caps) = self.member_pt_caps(pool, domain) else {
            return Ok(None);
        };
        let mass = i128::try_from(mass).map_err(|_| unavailable("PT domain mass overflow"))?;
        let mut keep = HashSet::new();
        for (m, cap) in caps {
            if cap.checked_mul(mass).ok_or_else(|| unavailable("PT domain payoff overflow"))? >= numerator {
                keep.insert(m);
            }
        }
        Ok((keep.len() < domain.members().len()).then(|| domain.retain_proven_members(pool, &keep)))
    }

    /// Diagnostics only: the PT score tiers, their concave majorant and the constant multiplier.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn point_tiers(&self) -> serde_json::Value {
        self.points.as_ref().map_or(
            serde_json::Value::Null,
            |pt| serde_json::json!({"tiers":pt.score_tiers,"hull":pt.hulls.as_ref().and_then(|h| h.last()),"multiplier":pt.multiplier}),
        )
    }
}
