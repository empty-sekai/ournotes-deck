//! The entries of the result list and their ranking.

use super::*;

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) physical: PhysicalDeck,
    pub(super) members: [i64; 5],
    pub(super) snaps: [Option<i64>; 5],
    pub(super) power: i32,
    pub(super) evaluation: FiniteEvaluation,
    /// A combined metric: the expectation numerator of each term's payoff over the evaluation's denominator.
    pub(super) terms: Option<Vec<Option<i128>>>,
}

pub(super) fn compare(a: &Entry, b: &Entry) -> Ordering {
    b.evaluation
        .expected_payoff
        .numerator
        .cmp(&a.evaluation.expected_payoff.numerator)
        .then_with(|| b.power.cmp(&a.power))
        .then_with(|| a.members.cmp(&b.members))
        .then_with(|| a.snaps.cmp(&b.snaps))
}

impl Entry {
    /// The wire result. Maximum aggregation of a played live reports the greatest order value as `objective_value`
    /// and the order that attains it; the mean-based fields belong to Expected aggregation. A deterministic execution
    /// has one outcome that both aggregations report alike.
    pub(super) fn wire(self, metric: &Metric, aggregation: Aggregation, live: bool) -> Result<RecommendedDeck, Error> {
        let maximum = aggregation == Aggregation::Maximum && live;
        let denominator = self.evaluation.expected_payoff.denominator;
        // A combined metric reports its levels' own expectations; the payoff of the deck and of each order is that
        // of the first level.
        let unit = metric.primary_unit();
        let fraction = |value: i128| Fraction { numerator: value.to_string(), denominator: denominator.to_string() };
        let level_payoffs = crate::search::levels::unfold(metric, self.evaluation.expected_payoff.numerator)
            .map(|levels| levels.into_iter().map(fraction).collect::<Vec<_>>());
        let expected_payoff = match &level_payoffs {
            Some(levels) => levels[0].clone(),
            None => self.evaluation.expected_payoff.into(),
        };
        let term_payoffs = self.terms.map(|terms| {
            terms
                .into_iter()
                .map(|term| {
                    term.map(|value| Fraction { numerator: value.to_string(), denominator: denominator.to_string() })
                })
                .collect()
        });
        if matches!(metric, Metric::Power) {
            return Ok(RecommendedDeck {
                members: self.members,
                snaps: self.snaps,
                power: self.power,
                objective_value: None,
                expected_score: None,
                expected_payoff: Some(expected_payoff),
                term_payoffs,
                level_payoffs,
                score_interval: None,
                payoff_interval: None,
                rank_certified: None,
                score_summary: None,
                best_order: None,
                order_outcomes: Vec::new(),
            });
        }
        let score_summary = score_summary(&self.evaluation.score_mass, metric.target())?;
        // Expected aggregation: the first order with the highest payoff, then the highest score (orders in
        // lexicographic order). Maximum aggregation: the first order with the highest payoff; orders omitted by a
        // strict bound cannot tie it.
        let best_order = (self.evaluation.outcomes.len() == ORDERS || maximum)
            .then(|| {
                self.evaluation.outcomes.iter().reduce(|best, o| {
                    let better = if maximum {
                        o.terminal_payoff > best.terminal_payoff
                    } else {
                        (o.terminal_payoff, o.final_score) > (best.terminal_payoff, best.final_score)
                    };
                    if better { o } else { best }
                })
            })
            .flatten()
            .map(|o| OrderResult {
                performance_order: o.performance_order,
                members: o.performance_order.map(|slot| self.members[slot]),
                score: o.final_score,
                payoff: o.terminal_payoff.div_euclid(unit).to_string(),
            });
        Ok(RecommendedDeck {
            members: self.members,
            snaps: self.snaps,
            power: self.power,
            objective_value: maximum.then(|| expected_payoff.clone()),
            expected_score: (!maximum).then(|| self.evaluation.expected_score.into()),
            expected_payoff: (!maximum).then_some(expected_payoff),
            term_payoffs,
            level_payoffs,
            score_interval: None,
            payoff_interval: None,
            rank_certified: None,
            score_summary: (!maximum).then_some(score_summary),
            best_order,
            order_outcomes: if !maximum && self.evaluation.outcomes.len() == ORDERS {
                self.evaluation
                    .outcomes
                    .iter()
                    .map(|o| (o.performance_order, o.final_score, o.terminal_payoff.div_euclid(unit)))
                    .collect()
            } else {
                Vec::new()
            },
        })
    }
}
