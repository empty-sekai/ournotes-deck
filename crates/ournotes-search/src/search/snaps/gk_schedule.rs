//! Deck-independent Gekisou range schedule, combo and factor envelopes.
use super::*;
mod carriers;
mod combo;
mod factors;
mod keyed;
use carriers::factor_sums;
pub(super) use carriers::{CarrierLevel, RampReads, carrier_level_envelopes, is_carrier_class, window_gain};
pub(super) use combo::{GkCombo, RampStarts, bonus_at, carrier_factors, gated_bonus, keyed_factors, ramp_factor};
pub(super) use factors::GkFactors;
pub(super) use keyed::KeyedDrift;
pub(crate) use keyed::{CarrierKeys, KeyedEnvelope};

/// An integer live setting.
pub(super) fn int_setting(master: &Master, key: &str) -> Result<i64, Error> {
    let v = master.live_setting(key).ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
    v.trim().parse::<i64>().map_err(|_| Error::Master(format!("MasterLiveSettings {key} is not an integer")))
}

/// The song's mission pattern (as the rank bonus table keys it): 0 when a mission is missing, 1 all the same, 2 all
/// different, 3 otherwise.
pub(super) fn mission_pattern(m: &[i64]) -> i64 {
    let (a, b, c) = (m.first().copied().unwrap_or(0), m.get(1).copied().unwrap_or(0), m.get(2).copied().unwrap_or(0));
    if a == 0 || b == 0 || c == 0 {
        return 0;
    }
    if a == b {
        return if a == c { 1 } else { 3 };
    }
    if b != c && a != c {
        return 2;
    }
    3
}

/// One Gekisou range of a live and its schedule.
#[derive(Clone, Copy, Debug)]
pub(super) struct RangeFacts {
    pub(super) start: i32,
    pub(super) end: i32,
    pub(super) mission: i64,
    /// Rank bonus percent (a solo player is rank 1).
    pub(super) pct: i64,
    /// The first play frame in which the range is in the state Start (its fever turned on), Complete (its rank bonus
    /// is confirmed there) and Finish.
    pub(super) f_start: Option<usize>,
    pub(super) f_complete: Option<usize>,
    pub(super) f_finish: Option<usize>,
}

/// The schedule of the Gekisou ranges: the state of every range after each play frame. The range state machine reads
/// only the frame times, the delta times and the fevers, so the schedule is the same for every deck; it is taken from
/// a run of the live without skills.
pub(super) struct Schedule {
    pub(super) states: Vec<Vec<u8>>,
    pub(super) ranges: Vec<RangeFacts>,
}

impl Schedule {
    pub(super) fn new(master: &Master, setup: &FullSetup, g: &GkPlay) -> Result<Schedule, Error> {
        let perf = vec![Performer::default(); 5];
        let params = LiveParams { total_power: 0, ..setup.params };
        let mut lm = if let Some(confirmations) = &g.confirmations {
            let mut lm = LiveModel::new_gekisou_external(master, &perf, &setup.notes, &setup.events, params, &g.setup)?;
            lm.set_rank_confirmation_timeline(confirmations)?;
            lm
        } else {
            LiveModel::new_gekisou(master, &perf, &setup.notes, &setup.events, params, &g.setup)?
        };
        let mut states = Vec::with_capacity(setup.play.frames.len());
        for (f, &dt) in setup.play.frames.iter().zip(&g.dt) {
            lm.frame_timed(f.time_ms, &f.judged, dt)?;
            states.push(lm.gekisou_ranges().iter().map(|r| r.state).collect::<Vec<u8>>());
        }
        let pattern = mission_pattern(&g.setup.missions);
        let mut ranges = Vec::with_capacity(g.setup.fevers.len());
        for (i, &(start, end)) in g.setup.fevers.iter().enumerate() {
            let first = |s: u8| states.iter().position(|x: &Vec<u8>| x.get(i).is_some_and(|&v| v >= s));
            let mut pct = 0;
            for r in &master.gekisou_ranking_score_bonuses {
                if r.mission_pattern == pattern && r.count == i as i64 + 1 && r.rank == 1 {
                    pct = r.score_bonus_percent;
                }
            }
            if let Some(confirmations) = &g.confirmations {
                pct = confirmations.iter().find(|c| c.range == i).map_or(0, |c| c.percent);
            }
            let mission = *g.setup.missions.get(i).ok_or_else(|| Error::Input("fewer missions than fevers".into()))?;
            ranges.push(RangeFacts {
                start,
                end,
                mission,
                pct,
                f_start: first(RS_START),
                f_complete: first(RS_COMPLETE),
                f_finish: first(RS_FINISH),
            });
        }
        Ok(Schedule { states, ranges })
    }

    /// A play frame no later than the first one in which a deck of these rows can draw a random number: the first
    /// frame of a luck range in the state Start (its lottery), and for a row with a probability condition the first
    /// frame in which it can be asked (a Gekisou row gated by a mission only once some range has left Wait).
    pub(super) fn first_draw<'r>(&self, env: &Env, rows: impl Iterator<Item = &'r Row>) -> usize {
        let nf = self.states.len();
        let mut f0 =
            self.ranges.iter().filter(|r| r.mission == MISSION_LUCK).filter_map(|r| r.f_start).min().unwrap_or(nf);
        let awake = self.states.iter().position(|s| s.iter().any(|&x| x >= RS_STANDBY)).unwrap_or(nf);
        for r in rows {
            if [r.trigger, r.condition, r.release, r.reset].iter().any(|&g| env.draws(g)) {
                f0 = f0.min(if r.gk && r.gate != MISSION_ALL { awake } else { 0 });
            }
        }
        f0
    }
}

// Linear envelopes by Gekisou combo carrier count.
//
// A slot is a carrier when its member and class bring Gekisou combo bonus windows (`Contrib::cb`). The pool-wide
// Gekisou combo factor `G_e` of `Coef::k` reads the five largest members' bonuses, but a deck with at most `n`
// carriers counts at most the `n` largest (`GkFactors::carriers`). Its score is then bounded by the linear envelope
// rebuilt with that factor: every coefficient, window and conversion budget is the pool-wide one except the factor,
// which is at most `G_e`, so each level's `A0`, gains and `global` are at most the pool-wide ones.

#[cfg(test)]
mod keyed_drift_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timer_floor_tests;
