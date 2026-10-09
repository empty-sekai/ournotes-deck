use super::*;
use std::sync::Arc;

/// A complete enclosure of reachable native terminal scores. Endpoints need not be attained.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckMaximumSupport {
    pub final_support: IntegerBounds,
    pub exact_constant_score: Option<i32>,
    /// Some only when every admitted path has the same terminal LIFE.
    pub exact_final_life: Option<i32>,
    pub reachability_peak_states: usize,
    pub reachability_transitions: u64,
}

/// Support evaluations for one immutable chart, native frame schedule and rank-arrival scenario.
pub struct LuckMaximumSession<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    notes: &'a [LiveNote],
    events: &'a [(i32, i32)],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    delta_times: &'a [f32],
    ranking: Option<&'a [crate::replay::RankConfirmation]>,
}

impl<'a> LuckMaximumSession<'a> {
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
        ranking: Option<&'a [crate::replay::RankConfirmation]>,
    ) -> Self {
        Self { master, skills, notes, events, params, setup, play, delta_times, ranking }
    }

    /// Native terminal-score enclosure, or `None` on cancellation. Unsupported arithmetic returns an error.
    pub fn support(
        &mut self,
        deck: &[Performer],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckMaximumSupport>, Error> {
        let value = score_bounds_mode::<false>(
            self.master,
            self.skills,
            deck,
            self.notes,
            self.events,
            self.params,
            self.setup,
            self.play,
            self.delta_times,
            self.ranking,
            false,
            None,
            None,
            &mut cancelled,
            false,
            None,
        )?;
        Ok(value.map(|value| match value {
            ComputedBounds::Maximum(support) => support,
            ComputedBounds::Expected(_) => unreachable!("support evaluation has no expectation result"),
        }))
    }
}

pub(super) trait CurveSupport {
    fn probe_transitions(&self) -> &[u8];
    fn rush_filings(&self) -> &[luck_dp::LuckRushFrameSupport];
}

impl CurveSupport for LuckDpCertifiedResult {
    fn probe_transitions(&self) -> &[u8] {
        &self.probe_transitions
    }
    fn rush_filings(&self) -> &[luck_dp::LuckRushFrameSupport] {
        &self.rush_filings
    }
}

pub(super) enum ScoreCurve {
    Expected(Arc<LuckDpCertifiedResult>),
    Support(luck_dp::LuckDpSupportResult),
}

impl CurveSupport for ScoreCurve {
    fn probe_transitions(&self) -> &[u8] {
        match self {
            Self::Expected(v) => &v.probe_transitions,
            Self::Support(v) => &v.probe_transitions,
        }
    }
    fn rush_filings(&self) -> &[luck_dp::LuckRushFrameSupport] {
        match self {
            Self::Expected(v) => &v.rush_filings,
            Self::Support(v) => &v.rush_filings,
        }
    }
}

impl ScoreCurve {
    pub(super) fn expected(&self) -> &LuckDpCertifiedResult {
        match self {
            Self::Expected(value) => value,
            Self::Support(_) => unreachable!("support curve carries no nominal mass"),
        }
    }
    pub(super) fn stats(&self) -> (usize, u64) {
        match self {
            Self::Expected(v) => (v.peak_states, v.transitions),
            Self::Support(v) => (v.peak_states, v.transitions),
        }
    }
    pub(super) fn note_support(&self, note: &LuckNoteBounds) -> Result<I32Interval, Error> {
        let Self::Support(curve) = self else { unreachable!("reachability curve required") };
        let index = curve.steps.partition_point(|(time, _)| *time <= note.time_ms);
        let possible = index.checked_sub(1).map_or([true, false, false, false], |index| curve.steps[index].1);
        let (mut lower, mut upper) = (i32::MAX, i32::MIN);
        for (bucket, possible) in note.buckets.iter().zip(possible) {
            if !possible {
                continue;
            }
            let Some(bucket) = bucket else {
                return Err(refuse("a reachable lottery class has no native factor path"));
            };
            lower = lower.min(bucket.lower);
            upper = upper.max(bucket.upper);
        }
        I32Interval::new(lower, upper)
    }
}

pub(super) enum ComputedBounds {
    Expected(LuckScoreBounds),
    Maximum(LuckMaximumSupport),
}

impl ComputedBounds {
    pub(super) fn expected(self) -> LuckScoreBounds {
        match self {
            Self::Expected(value) => value,
            Self::Maximum(_) => unreachable!("expectation evaluation requires nominal masses"),
        }
    }
}
