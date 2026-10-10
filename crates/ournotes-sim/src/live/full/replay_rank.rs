//! Deterministic judged-stream domain for uniform skill-order replay analysis.

use super::luck_score_bounds::{deterministic, deterministic_cumulative};
use super::{EffectRow, LiveModel, conditions};
use crate::Error;

impl LiveModel {
    /// Establish that changing the root seed cannot change this declared run's score program.
    /// The clock, judgements, formation and rank inputs remain fixed while skill event order varies.
    pub(crate) fn check_replay_order_domain(&self) -> Result<(), Error> {
        if self.gekisou_ranges().iter().any(|range| range.mission == 2) {
            return Err(Error::Unsupported("replay rank analysis: LUCK ranges are unsupported".into()));
        }
        let check = |row: &EffectRow,
                     cumulative: Option<&conditions::Cumulative>,
                     checkers: &[&Option<conditions::Checker>]| {
            if !matches!(row.effect_type,
                0 | 1000..=1003 | 1500..=1503 | 2000..=2005 | 3000..=3004 | 4004
                | 11000..=11005 | 12000 | 12002..=12004 | 12006 | 13000 | 13002..=13005 | 15000)
                || cumulative.is_some_and(|value| !deterministic_cumulative(value))
                || checkers.iter().any(|checker| checker.as_ref().is_some_and(|value| !deterministic(value)))
            {
                return Err(Error::Unsupported(format!(
                    "replay rank analysis: effect row {} has an unproved deterministic schedule",
                    row.id
                )));
            }
            Ok(())
        };
        for skill in &self.live {
            for effect in &skill.effects {
                check(&self.rows[effect.row], effect.cumulative.as_ref(), &[&effect.condition, &effect.release])?;
            }
        }
        for skill in &self.cond {
            for effect in skill.updater.effects() {
                check(
                    &self.rows[effect.row],
                    effect.cumulative.as_ref(),
                    &[&effect.trigger, &effect.condition, &effect.reset],
                )?;
            }
            for updater in &skill.updater.updaters {
                if updater.release.as_ref().is_some_and(|value| !deterministic(value)) {
                    return Err(Error::Unsupported(
                        "replay rank analysis: an effect release has an unproved deterministic schedule".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
