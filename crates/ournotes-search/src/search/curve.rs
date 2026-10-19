//! The shape of a score-dependent payoff: how far one order's payoff steps with its score, and the score cap the
//! step is read at.
use crate::search::bonus::unavailable;
use crate::search::reward::points_payoff;
use ournotes_sim::Error;

/// One step of a played Live's per-order payoff, from its lowest score on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// Event or challenge points `multiplier * (bonus + 10000) / 10000`, the multiplier being the rate times the
    /// largest reward value at or below the step's score (challenge points read no card bonus).
    Points(i64),
    /// A score target: reached (1) or not (0).
    Target(bool),
}

/// The per-order payoff of a played Live steps with the order's score. Every order of a deck with power `p`
/// scores at most `ceil(p * global * (1 + eps))` (the global coefficient of the joint envelope with its margin, see
/// `JointBounds::score_steps`), so the step of that cap bounds each of its orders.
#[derive(Clone, Debug)]
pub(crate) struct ScoreSteps {
    global: f64,
    eps: f64,
    /// (lowest score, step) by ascending score, each step at least the previous one.
    steps: Vec<(i128, Step)>,
}

impl ScoreSteps {
    pub(crate) fn new(global: f64, eps: f64, steps: Vec<(i128, Step)>) -> Self {
        Self { global, eps, steps }
    }

    /// The score cap of every order of a deck with power at most `power`.
    pub(crate) fn score_cap(&self, power: i64) -> i128 {
        ((power as f64) * self.global * (1.0 + self.eps)).ceil() as i128
    }

    pub(crate) fn value(&self, bonus: i64, power: i64) -> Result<i128, Error> {
        let cap = self.score_cap(power);
        let at = self.steps.partition_point(|&(score, _)| score <= cap);
        // The joint bound's compilation proved the native products nonwrapping up to the largest bonus.
        match at.checked_sub(1).map(|i| self.steps[i].1) {
            None => Err(unavailable("score cap below the lowest step")),
            Some(Step::Points(multiplier)) => Ok(points_payoff(bonus, multiplier)),
            Some(Step::Target(reached)) => Ok(i128::from(reached)),
        }
    }

    /// The largest multiplier of any step; it only orders the members a search visits.
    pub(crate) fn largest_step(&self) -> f64 {
        self.steps.iter().fold(0.0, |best, &(_, step)| match step {
            Step::Points(multiplier) => best.max(multiplier as f64),
            Step::Target(_) => best,
        })
    }
}

/// The steps of one term of a combined metric, with its weight and its member and Snap bonuses.
pub(crate) type TermSteps<'a> = (i128, &'a [i64], &'a [i64], ScoreSteps);
