//! Complete terminal payoffs of one admitted native program, without enumerating its hidden RNG leaves.
//!
//! The controller DP retains every original hidden state until termination, then sums the disjoint
//! masses of identical observable timelines. Each timeline is folded through this program's native
//! score tape before its integer payoff is taken. No coupling to another deck is assumed.
use super::*;

/// The complete mapping of the terminal native score and LIFE. Thresholds remain signed native integers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LuckTerminalPayoff {
    ScoreAtLeast { threshold: i32 },
    CappedScore { threshold: i32 },
    ScoreAndLifeAtLeast { threshold: i32, min_final_life: i32 },
}

impl LuckTerminalPayoff {
    fn value(self, score: i32, life: i32) -> i32 {
        match self {
            Self::ScoreAtLeast { threshold } => i32::from(score >= threshold),
            Self::CappedScore { threshold } => score.min(threshold),
            Self::ScoreAndLifeAtLeast { threshold, min_final_life } => {
                i32::from(score >= threshold && life >= min_final_life)
            }
        }
    }
}

/// Constructed only after every supported terminal of the supplied physical order has been evaluated.
/// This enclosure belongs to that order, complete mapping and this session's immutable native inputs.
/// General weighted results are outward enclosures. An exact constant is exposed only when the same
/// native integer payoff was observed at every terminal; neither result is a score-law equality proof.
#[derive(Clone, Debug)]
pub struct LuckTerminalPayoffBounds {
    map: LuckTerminalPayoff,
    bounds: F64Interval,
    exact_constant: Option<i32>,
}

impl LuckTerminalPayoffBounds {
    pub fn map(&self) -> LuckTerminalPayoff {
        self.map
    }

    pub fn bounds(&self) -> F64Interval {
        self.bounds
    }

    /// Complete-support constancy, never inferred from a rounded interval endpoint.
    pub fn exact_constant(&self) -> Option<i32> {
        self.exact_constant
    }
}

#[derive(Debug)]
pub struct LuckTerminalPayoffAttempt {
    pub payoff: Option<LuckTerminalPayoffBounds>,
    /// The same strict recorder, original-clock and native-emission admissions as the paired score fold.
    pub decline: Option<LuckScoreEquivalenceDecline>,
    pub recording_runs: u64,
    pub recording_frames: u64,
    pub timeline_paths: u64,
    /// Transitions of completed controller traversals only.
    pub timeline_transitions: u64,
    /// Actual native score queries, including those performed before a cancellation or refusal.
    pub score_fold_queries: u64,
}

/// Request-owned immutable context and shared bounded score-fold work. No program or probability result
/// is cached: each order supplies its original complete Performer array and its own controller transcript.
pub struct LuckTerminalPayoffSession<'a> {
    context: Context<'a>,
    fold_work: score_fold::Work,
}

impl<'a> LuckTerminalPayoffSession<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        master: &'a Master,
        skills: &'a LuckSkills,
        notes: &'a [LiveNote],
        events: &'a [(i32, i32)],
        params: LiveParams,
        setup: &'a GekisouSetup,
        play: &'a LivePlay,
        delta_times: &'a [f32],
    ) -> Self {
        Self {
            context: Context { master, skills, notes, events, params, setup, play, delta: delta_times },
            fold_work: score_fold::Work::default(),
        }
    }

    /// Fold the true terminal mapping of this one fixed order under its complete nominal lottery law.
    /// All native recording work uses the caller's existing request budget. Interrupted or capacity-limited
    /// supports publish no enclosure; previously completed orders remain valid in the caller.
    pub fn payoff(
        &mut self,
        performers: &[Performer; 5],
        map: LuckTerminalPayoff,
        budget: &mut LuckExactBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<LuckTerminalPayoffAttempt, Error> {
        let before = (budget.remaining_runs, budget.remaining_frames, self.fold_work.queries);
        let mut timeline_paths = 0;
        let mut timeline_transitions = 0;
        let mut run = || -> Checked<LuckTerminalPayoffBounds> {
            poll(&mut cancelled)?;
            if budget.exhausted() {
                return declined(LuckScoreEquivalenceDecline::WorkBudget);
            }
            let context = &self.context;
            if context.setup.fevers.is_empty()
                || context.setup.missions.len() < context.setup.fevers.len()
                || context.setup.missions.iter().take(context.setup.fevers.len()).any(|&mission| mission != M_LUCK)
                || context.delta.len() != context.play.frames.len()
            {
                return declined(LuckScoreEquivalenceDecline::Context);
            }
            let recording = context.fingerprint(performers, budget, &mut cancelled)?;
            let recipe = recording.recipe?;
            let clock: Vec<_> = context.play.frames.iter().map(|frame| frame.time_ms).collect();
            let support = native(
                super::super::timeline_support::complete_timeline_support(
                    &recording.controller,
                    &clock,
                    recipe.has_probes(),
                    &mut cancelled,
                ),
                LuckScoreEquivalenceDecline::ProbabilityDomain,
            )?
            .ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Cancelled))?;
            timeline_transitions = support.transitions;
            let mut visited = vec![false; support.len()];
            let mut bounds = F64Interval::ZERO;
            let (mut minimum, mut maximum) = (i32::MAX, i32::MIN);
            score_fold::evaluate_support(
                [&recipe],
                &support,
                &mut self.fold_work,
                &mut cancelled,
                |index, [score]| {
                    let seen = visited.get_mut(index).ok_or_else(|| {
                        Failure::Native(Error::Domain("terminal payoff index outside complete support".into()))
                    })?;
                    if std::mem::replace(seen, true) {
                        return Err(Failure::Native(Error::Domain("terminal payoff visited twice".into())));
                    }
                    let mass = native(support.mass(index), LuckScoreEquivalenceDecline::ProbabilityDomain)?;
                    // Ordinary LIFE is deterministic under the recorder admission. A random LIFE effect
                    // would require a joint LIFE timeline and is rejected before this point.
                    let value = map.value(score, recording.final_life);
                    bounds = bounds.add(mass.weighted_integer(value));
                    minimum = minimum.min(value);
                    maximum = maximum.max(value);
                    timeline_paths += 1;
                    Ok(())
                },
            )?;
            poll(&mut cancelled)?;
            if visited.is_empty() || visited.iter().any(|&seen| !seen) {
                return Err(Failure::Native(Error::Domain("terminal payoff omitted a supported timeline".into())));
            }
            // The mapping's attained integer range is another proof, including constant zero/one/cap
            // payoffs. Complete native transition partitions already establish unit probability mass:
            // if every terminal has value c, E[c] = c independently of interval rounding. Intersect
            // the weighted sum with that range; never normalize the outward terminal masses.
            let range = native(
                F64Interval::new(f64::from(minimum), f64::from(maximum)),
                LuckScoreEquivalenceDecline::ScoreTrace,
            )?;
            let bounds = bounds.intersect(range).ok_or_else(|| {
                Failure::Native(Error::Domain("complete terminal payoff contradicts its attained range".into()))
            })?;
            Ok(LuckTerminalPayoffBounds { map, bounds, exact_constant: (minimum == maximum).then_some(minimum) })
        };
        let (payoff, decline) = match run() {
            Ok(payoff) => (Some(payoff), None),
            Err(Failure::Decline(reason)) => (None, Some(reason)),
            Err(Failure::Native(error)) => return Err(error),
        };
        Ok(LuckTerminalPayoffAttempt {
            payoff,
            decline,
            recording_runs: before.0 - budget.remaining_runs,
            recording_frames: before.1 - budget.remaining_frames,
            timeline_paths,
            timeline_transitions,
            score_fold_queries: self.fold_work.queries - before.2,
        })
    }
}
