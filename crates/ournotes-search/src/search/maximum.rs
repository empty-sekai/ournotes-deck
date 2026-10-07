//! Maximum reachable terminal payoff, with canonical team identity and per-order score bounds.
use super::*;
use crate::domain::CandidateDomain;
use crate::search::expectation::ExactExpectation;
use crate::search::joint::{JointBounds, SLOTS};
use ournotes_sim::live::full::{LuckExactBudget, LuckExactDecline, LuckExactSession, LuckScoreSession};

impl Engine<'_, '_> {
    pub(super) fn evaluate_maximum(
        &mut self,
        physical: &PhysicalDeck,
        power: i32,
        cut: Option<(&JointBounds, &CandidateDomain)>,
    ) -> Result<Leaf, Error> {
        let caps =
            cut.map(|(bounds, domain)| bounds.order_cheap_caps(domain, physical, i64::from(power), &self.positions));
        if let Some(caps) = &caps
            && self.top.len() == self.request.k
            && let Some(last) = self.top.last()
        {
            let upper = caps.iter().copied().max().expect("performance orders");
            if upper < last.evaluation.expected_payoff.numerator
                || (upper == last.evaluation.expected_payoff.numerator && power < last.power)
            {
                self.tel.leaves.cheap_pruned += 1;
                return Ok(Leaf::Pruned);
            }
        }
        let mut input = expectation::context(self.pool, physical, &self.request.objective)?;
        if let Some(value) = self.simulation.music_length_ms {
            input.params.music_length_ms = value;
        }
        if let Some(value) = self.simulation.score_music_length_ms {
            input.params.score_music_length_ms = Some(value);
        }
        let master = self.pool.master;
        let mut exact = match &input.gekisou {
            Some(setup) => LuckExactSession::new(
                master,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
                self.limits.cache_entries,
            )?,
            None => LuckExactSession::new_live(
                master,
                &input.notes,
                &input.events,
                input.params,
                &input.play,
                &input.delta_times,
                self.limits.cache_entries,
            )?,
        };
        let skills = input.gekisou.as_ref().and_then(|_| ournotes_sim::live::full::luck_skills(master).ok());
        let mut score_bounds = input.gekisou.as_ref().zip(skills.as_ref()).map(|(setup, skills)| {
            LuckScoreSession::new(
                master,
                skills,
                &input.notes,
                &input.events,
                input.params,
                setup,
                &input.play,
                &input.delta_times,
                input.rank_confirmations.as_deref(),
            )
        });
        let score_metric = matches!(self.metric, Metric::Score);
        let mut budget = LuckExactBudget::default();
        let mut outcomes = Vec::with_capacity(ORDERS);
        let mut support = BTreeMap::new();
        let mut best = None::<i128>;
        self.tel.leaves.started += 1;
        let (_, resume) = self.rec.clock.lap(slot::SIMULATION);
        for (index, order) in uniform::all_orders().into_iter().enumerate() {
            if self.expired() {
                self.rec.clock.lap(resume);
                return Ok(Leaf::Stopped);
            }
            if score_metric && best.is_some_and(|best| caps.as_ref().is_some_and(|caps| caps[index] < best)) {
                continue;
            }
            let performers = order.map(|slot| input.performers[slot].clone());
            let mut ceiling =
                if score_metric { caps.as_ref().and_then(|caps| i32::try_from(caps[index]).ok()) } else { None };
            if score_metric && let Some(bounds) = &mut score_bounds {
                // Optional bounds are used only to close the search when a simulated path attains them.
                if let Ok(Some(summary)) = bounds.summary(&performers, None, || self.expired()) {
                    ceiling = Some(ceiling.map_or(summary.final_support.upper, |c| c.min(summary.final_support.upper)));
                }
                if self.expired() {
                    self.rec.clock.lap(resume);
                    return Ok(Leaf::Stopped);
                }
            }
            let result = exact.support(&performers, &mut budget, ceiling, || self.expired())?;
            self.tel.leaves.simulations += result.stats.replay_runs;
            if let Some(reason) = result.decline {
                if reason != LuckExactDecline::Cancelled || self.stop.is_none() {
                    self.stop = Some(ExitReason::RefinementRequired);
                }
                self.rec.clock.lap(resume);
                return Ok(Leaf::Stopped);
            }
            let mut chosen = None;
            for (score, life) in result.outcomes {
                support.insert(score, 1);
                let payoff = self.payoff(physical, score, power, Some(life))?;
                if chosen.is_none_or(|(previous, previous_score)| (payoff, score) > (previous, previous_score)) {
                    chosen = Some((payoff, score));
                }
            }
            let (payoff, score) = chosen.ok_or_else(|| Error::Domain("reachable outcome set is empty".into()))?;
            best = Some(best.map_or(payoff, |current| current.max(payoff)));
            outcomes.push(SeedOutcome {
                root_seed: 0,
                weight: 1,
                performance_order: order,
                final_score: score,
                terminal_payoff: payoff,
            });
        }
        self.rec.clock.lap(resume);
        let mut evaluation = expectation::aggregate(outcomes)?;
        evaluation.expected_payoff = ExactExpectation { numerator: best.expect("performance orders"), denominator: 1 };
        evaluation.score_mass = support;
        Ok(Leaf::Evaluated(evaluation))
    }
}

pub(super) fn search(
    engine: &mut Engine<'_, '_>,
    domain: &CandidateDomain,
    bounds: Option<&JointBounds>,
) -> Result<(), Error> {
    if let Some(bounds) = bounds {
        let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        visit(0, None, &mut physical, domain, bounds, engine)?;
    } else {
        let mut physical = PhysicalDeck { members: [0; 5], snaps: [None; 5] };
        members_rec(0, 0, &mut physical, domain.members(), domain.required(), domain.leader(), domain.snaps(), engine)?;
    }
    Ok(())
}

fn visit(
    depth: usize,
    after: Option<i64>,
    physical: &mut PhysicalDeck,
    domain: &CandidateDomain,
    bounds: &JointBounds,
    engine: &mut Engine<'_, '_>,
) -> Result<bool, Error> {
    engine.tel.nodes += 1;
    if engine.expired() {
        return Ok(false);
    }
    let selected = &SLOTS[..depth];
    if domain
        .required()
        .iter()
        .filter(|&&member| !selected.iter().any(|&slot| physical.members[slot] == member))
        .count()
        > 5 - depth
    {
        return Ok(true);
    }
    if depth > 0 && engine.top.len() == engine.request.k {
        let (upper, power) = bounds.maximum_upper(engine.pool, domain, physical, depth);
        if let Some(last) = engine.top.last()
            && (upper < last.evaluation.expected_payoff.numerator
                || (upper == last.evaluation.expected_payoff.numerator && power < i64::from(last.power)))
        {
            return Ok(true);
        }
    }
    if depth == 5 {
        return engine.consider_with(*physical, Some((bounds, domain)));
    }
    let slot = SLOTS[depth];
    for &(member, choice) in &bounds.choices {
        let card = &engine.pool.members[member];
        if (depth == 0 && domain.leader().is_some_and(|leader| leader != member))
            || (depth > 0 && after.is_some_and(|after| card.id <= after))
            || selected
                .iter()
                .any(|&slot| engine.pool.members[physical.members[slot]].character_id == card.character_id)
            || domain
                .required()
                .iter()
                .any(|&r| r != member && engine.pool.members[r].character_id == card.character_id)
        {
            continue;
        }
        let snap = (choice > 0).then(|| domain.snaps()[choice - 1]);
        if snap.is_some() && selected.iter().any(|&slot| physical.snaps[slot] == snap) {
            continue;
        }
        physical.members[slot] = member;
        physical.snaps[slot] = snap;
        if !visit(depth + 1, if depth == 0 { None } else { Some(card.id) }, physical, domain, bounds, engine)? {
            return Ok(false);
        }
    }
    Ok(true)
}
