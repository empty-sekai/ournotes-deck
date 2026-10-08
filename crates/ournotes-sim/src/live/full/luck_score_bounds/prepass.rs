//! Terminal joint Rush/probe probabilities for conservative expected-score caps.
//!
//! This runs the admitted deterministic recorder and the certified lottery DP, but never the factor-history
//! replay. It certifies which joint law belongs to the terminal notes; the caller still has to prove the supplied
//! integer note caps. No probability-weighted cap is a score support, a candidate value or an exact score law.

use super::*;
use std::sync::Arc;

/// Outcome of preparing a cheap terminal-note bound. A declined preparation leaves the full scorer available.
#[derive(Debug)]
pub enum LuckRushPreparation {
    Ready(LuckTerminalRush),
    Unavailable { reason: LuckRushDecline, error: Error },
    Stopped,
}

/// A conservative preparation refusal; the complete scorer remains responsible for validating the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuckRushDecline {
    NoLuckRange,
    ExternalRanking,
    RecorderAdmission,
    ScoreArithmetic,
    ProbabilityDomain,
    UnfinishedRanges,
    TerminalQuery,
}

type PreparationResult = Result<Option<LuckTerminalRush>, (LuckRushDecline, Error)>;

fn declined(reason: LuckRushDecline, message: &str) -> (LuckRushDecline, Error) {
    (reason, refuse(message))
}

/// The chart-time Rush law after an admitted native recorder has filed every terminal note's lottery commands.
/// Construction is private: a raw DP curve alone does not establish terminal query readiness.
#[derive(Debug)]
pub struct LuckTerminalRush {
    note_times: Vec<i32>,
    probability: Arc<LuckDpCertifiedResult>,
    /// Held direct score probes whose common native gate matches the certified DP's LUCK gate.
    probe_gate: Option<i64>,
    /// Native terminal note-score-up field uppers, including initial one and complete replay drift.
    note_factors: Option<Vec<[f64; 2]>>,
    /// Complete native terminal-note uppers at this preparation's exact power.
    native_notes: Option<terminal_kernel::Prepared>,
}

impl LuckTerminalRush {
    /// A whole native-score expectation upper at this preparation's exact power. Every terminal note and
    /// every rank bonus has an independent nonnegative, nonwrapping integer support proof. This is only an
    /// exclusion upper; it supplies no candidate value, nonlinear payoff, score law or completed ranking.
    /// Any unsupported rank query, readiness gap or arithmetic domain leaves the existing caps available.
    pub fn native_score_mean_upper(&self, total_power: i64) -> Option<f64> {
        let native = self.native_notes.as_ref()?;
        (total_power == i64::from(native.power)).then_some(())?;
        native.mean_upper
    }

    /// Native integer terminal-note caps for this exact power and complete sorted note-time multiset.
    /// These include each note's actual conversion and frozen life, every recorded combo observation,
    /// both native floors and the full certified factor history. Same-time notes share their largest cap
    /// to make every input-occurrence ordering safe. Rank bonuses remain outside these per-note caps.
    /// An unavailable optional kernel leaves the existing field and probability capabilities valid.
    pub fn native_note_bucket_caps(&self, total_power: i64, times: &[i32]) -> Option<&[[i32; 4]]> {
        let native = self.native_notes.as_ref()?;
        (total_power == i64::from(native.power) && times == self.note_times).then_some(native.caps.as_slice())
    }

    /// Native terminal note-score-up field uppers in probe-off/on order, for this exact complete sorted
    /// note-time multiset. Each includes the initial one and all history drift; the selected judgement factor,
    /// combo, life and native note arithmetic remain the caller's responsibility. An unavailable optional
    /// factor certificate leaves the ordinary terminal probability capability valid.
    pub fn note_score_up_upper(&self, times: &[i32]) -> Option<&[[f64; 2]]> {
        (times == self.note_times).then_some(())?;
        self.note_factors.as_deref()
    }

    /// Some(LUCK=2) only when the completed recorder admits held direct score probes under that same gate.
    /// Other or unobserved gates grant no permission to condition a score window on the DP's probe bit.
    pub fn probe_gate(&self) -> Option<i64> {
        self.probe_gate
    }

    /// Upper bound on the expected sum of terminal note contributions, from `(chart time, Rush-off cap,
    /// Rush-on cap)` in ascending chart time. The caller must separately prove a decomposition
    /// `native score <= sum_e cap_e(Rush_e) + retained remainder`. Individual caps may omit conversion gains
    /// only when that unchanged remainder covers them in both Rush classes. This method certifies just the
    /// expectation of the supplied cap sum. The complete terminal note-time multiset must match.
    ///
    /// The DP's bucket bit 2 is the native chart-time Rush multiplier; its score-probe bit is deliberately not
    /// used. The two off buckets and two on buckets are accumulated directly, never inferred by subtracting
    /// rounded marginals. No independence between different notes is assumed.
    pub fn weighted_note_upper(&self, caps: &[(i32, i64, i64)]) -> Option<f64> {
        self.weighted_caps(caps.iter().map(|&(time, off, on)| (time, [off, off, on, on])))
    }

    /// Expected sum of terminal note caps in the four joint classes: probe mask 1 and native Rush mask 2.
    /// The caller proves the same complete-score decomposition as `weighted_note_upper`, retaining every
    /// history/rank/conversion remainder. A bucket may differ across the probe bit only with `probe_gate()==Some(2)`.
    /// Each cap is nonnegative and no larger than the old unconditional cap in bucket 3. These are expectation
    /// upper bounds, never conditional score values, supports or a complete probability law.
    pub fn weighted_note_bucket_upper(&self, caps: &[(i32, [i64; 4])]) -> Option<f64> {
        self.weighted_caps(caps.iter().copied())
    }

    fn weighted_caps(&self, caps: impl ExactSizeIterator<Item = (i32, [i64; 4])>) -> Option<f64> {
        if caps.len() != self.note_times.len() {
            return None;
        }
        let mut mean = F64Interval::ZERO;
        for ((time, caps), &expected) in caps.zip(&self.note_times) {
            if time != expected
                || caps[3] > i64::from(i32::MAX)
                || caps.iter().any(|&cap| cap < 0 || cap > caps[3])
                || self.probe_gate != Some(gekisou::M_LUCK) && (caps[0] != caps[1] || caps[2] != caps[3])
            {
                return None;
            }
            let index = self.probability.steps.partition_point(|(at, _)| *at <= time);
            let joint = note_mass(&self.probability, index.checked_sub(1));
            for (bucket, mass) in joint.into_iter().enumerate() {
                mean = mean.add(mass.interval().multiply(F64Interval::integer(i128::from(caps[bucket]))));
            }
        }
        mean.upper().is_finite().then_some(mean.upper())
    }
}

impl LuckScoreSession<'_> {
    /// Prepare terminal joint probabilities without factor-history replay. A caller may condition its native
    /// Rush multiplier, and matching direct probe amplitudes only under the observed LUCK gate, while retaining
    /// all ordinary effects and complete history/rank/conversion allowances. The recorder/DP admission is shared
    /// with `summary`.
    ///
    /// Unsupported shapes and unproved terminal mappings return `Unavailable`; cancellation returns `Stopped`.
    /// Neither is a completed score evaluation. Curves enter the existing cache only after complete propagation.
    pub fn rush_cap_preparation(
        &mut self,
        deck: &[Performer],
        curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> LuckRushPreparation {
        match prepare(self, deck, curves, &mut cancelled) {
            Ok(Some(capability)) => LuckRushPreparation::Ready(capability),
            Ok(None) => LuckRushPreparation::Stopped,
            Err((reason, error)) => LuckRushPreparation::Unavailable { reason, error },
        }
    }
}

fn prepare(
    session: &mut LuckScoreSession<'_>,
    deck: &[Performer],
    curves: Option<&mut LuckDpCache>,
    cancelled: &mut impl FnMut() -> bool,
) -> PreparationResult {
    if cancelled() {
        return Ok(None);
    }
    if session.ranking.is_some() {
        return Err(declined(
            LuckRushDecline::ExternalRanking,
            "terminal Rush caps retain the full external-rank scorer",
        ));
    }
    if !session.setup.missions.iter().take(session.setup.fevers.len()).any(|&mission| mission == gekisou::M_LUCK) {
        return Err(declined(LuckRushDecline::NoLuckRange, "terminal Rush caps require a LUCK range"));
    }
    #[cfg(feature = "search-diagnostics")]
    let mut timing = Timing::default();
    let mut model =
        LiveModel::new_gekisou(session.master, deck, session.notes, session.events, session.params, session.setup)
            .map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
    let probe_gate =
        check_recorder(&model, session.skills).map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
    let probes: Vec<_> = model
        .luck_score_rows(session.skills)
        .into_iter()
        .filter(|row| row.may_hold)
        .map(|row| ProbeRow { owner: row.owner, value: row.value })
        .collect();
    if probes.iter().any(|row| !row.value.is_finite() || row.value <= i32::MIN as f32 / 100000f32) {
        return Err(declined(
            LuckRushDecline::RecorderAdmission,
            "a direct score command cannot be safely paired with its signed inverse",
        ));
    }
    let calc = &model.score.calc;
    let native_power = calc.state.band_total_power;
    let initial_fields = [
        calc.state.combo_score_up,
        calc.state.note_score_up,
        calc.state.just,
        calc.state.perfect,
        calc.state.great,
        calc.state.good,
    ];
    if calc.converted_note_count <= 0
        || ![
            calc.score_adjustment_factor,
            calc.music_difficulty_factor,
            calc.life_onus_factor,
            calc.event_bonus_factor,
            calc.assist_factor,
        ]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
    {
        return Err(declined(
            LuckRushDecline::ScoreArithmetic,
            "terminal Rush caps require nonnegative finite score constants",
        ));
    }
    let rush = i32::try_from(
        setting(session.master, "gekisou_luck_rush_score_bonus_percent")
            .map_err(|error| (LuckRushDecline::ScoreArithmetic, error))?,
    )
    .map_err(|_| declined(LuckRushDecline::ScoreArithmetic, "Rush factor exceeds i32"))?;
    if rush < 0 || 100i32.checked_add(rush).is_none() {
        return Err(declined(
            LuckRushDecline::ScoreArithmetic,
            "terminal Rush caps require a nonnegative nonwrapping multiplier",
        ));
    }
    if cancelled() {
        return Ok(None);
    }
    #[cfg(feature = "search-diagnostics")]
    timing.next(Phase::Curve);
    let mut empty = LuckDpCache::new(0);
    let curves = curves.unwrap_or(&mut empty);
    let Some(probability) = curves
        .certified_cancellable(
            session.master,
            session.skills,
            session.notes,
            session.events,
            session.params,
            session.setup,
            session.play,
            session.delta_times,
            deck,
            None,
            None,
            Some(&mut session.recordings),
            cancelled,
        )
        .map_err(|error| (LuckRushDecline::ProbabilityDomain, error))?
    else {
        return Ok(None);
    };
    #[cfg(feature = "search-diagnostics")]
    timing.next(Phase::Recorder);
    model.set_luck_weights(session.skills, Vec::new()).map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
    let probe_phase_bound = bind_probe_phase(&model, session.skills);
    model.score.begin_bounds(probes, true);
    model.score.certify_bounds_filings(probe_gate);
    model.try_enable_bounds_record_only();
    if session.delta_times.len() != session.play.frames.len() {
        return Err((LuckRushDecline::RecorderAdmission, Error::Input("one delta time per frame".into())));
    }
    model.random.set_seed(session.play.base_seed);
    for (index, (frame, &delta)) in session.play.frames.iter().zip(session.delta_times).enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        model
            .frame_timed(frame.time_ms, &frame.judged, delta)
            .map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
    }
    if cancelled() {
        return Ok(None);
    }
    if model.random.draws() != 0 {
        return Err(declined(
            LuckRushDecline::RecorderAdmission,
            "the supposedly deterministic recorder consumed random draws",
        ));
    }
    if model.gk.as_ref().is_none_or(|g| g.ctrl.states.iter().any(|state| state.state != gekisou::S_FINISH)) {
        return Err(declined(LuckRushDecline::UnfinishedRanges, "terminal query precedes a range FINISH"));
    }
    let mut trace = model.score.bounds_trace.take().expect("bounds recorder enabled");
    let frame_times: Vec<_> = model.trace.iter().map(|&(time, _)| time).collect();
    if !frame_times.iter().copied().eq(session.play.frames.iter().map(|frame| frame.time_ms)) {
        return Err(declined(
            LuckRushDecline::RecorderAdmission,
            "the completed recorder frame clock differs from the probability recording",
        ));
    }
    let probe_lifetime_bound =
        check_probe_music_boundary(&frame_times, &probability, model.music_length_ms, probe_phase_bound, &trace)
            .is_ok();
    trace.project_probe_filings().map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
    let query_limit = (session.play.frames.len() as u64)
        .checked_mul(2)
        .and_then(|value| value.checked_add(2 * session.setup.fevers.len() as u64))
        .ok_or_else(|| (LuckRushDecline::TerminalQuery, Error::Capacity("score query count overflow".into())))?;
    if trace.queries as u64 > query_limit {
        return Err(declined(LuckRushDecline::TerminalQuery, "unaccounted native calculate entry point"));
    }
    let Some(mut terminal) = terminal_notes(&trace, &probability, cancelled)? else {
        return Ok(None);
    };
    // A backdated probe end can disagree with the chart-time probe bit. The independent native envelope
    // still covers its signed filings; only the conditional probe permission must be withheld.
    if !probe_lifetime_bound {
        terminal.probe_gate = None;
    }
    #[cfg(feature = "search-diagnostics")]
    timing.next(Phase::Factors);
    match terminal_prefix::from_trace(&trace, initial_fields, &terminal.note_times, terminal.probe_gate, cancelled) {
        Ok(prepared) => {
            #[cfg(feature = "search-diagnostics")]
            profile::record(prepared.profile);
            #[cfg(feature = "search-diagnostics")]
            timing.next(Phase::Kernel);
            match terminal_kernel::build(
                &model.score.calc,
                native_power,
                rush,
                &trace,
                &prepared.ingredients,
                prepared.linked,
                &terminal.note_times,
                &probability,
                &mut *cancelled,
            ) {
                Ok(native) => terminal.native_notes = Some(native),
                Err(trace_drift::Decline::Cancelled) => return Ok(None),
                Err(_) => {
                    #[cfg(feature = "search-diagnostics")]
                    profile::record(LuckScoreProfile { terminal_kernel_refusals: 1, ..Default::default() });
                }
            }
            terminal.note_factors = Some(prepared.factors);
        }
        Err(trace_drift::Decline::Cancelled) => return Ok(None),
        Err(_) => {
            #[cfg(feature = "search-diagnostics")]
            profile::record(LuckScoreProfile { terminal_factor_refusals: 1, ..Default::default() });
        }
    }
    Ok(Some(terminal))
}

pub(super) fn terminal_notes(
    trace: &BoundsTrace,
    probability: &Arc<LuckDpCertifiedResult>,
    cancelled: &mut impl FnMut() -> bool,
) -> PreparationResult {
    let mut ready = i32::MIN;
    let mut last_query = None;
    let mut pending_rank = false;
    let mut query_count = 0;
    let mut notes = Vec::new();
    for (index, event) in trace.events.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        match event {
            BoundsEvent::Note { frame, note, .. } => notes.push((*frame, note.time_ms)),
            BoundsEvent::ProbabilityReady(time) => ready = ready.max(*time),
            BoundsEvent::Query { to, .. } => {
                last_query = Some((*to, ready));
                pending_rank = false;
                query_count += 1;
            }
            BoundsEvent::Rank { .. } => pending_rank = true,
            _ => {}
        }
    }
    let Some((to, ready_at_query)) = last_query else {
        return Err(declined(LuckRushDecline::TerminalQuery, "terminal Rush cap has no native score query"));
    };
    if pending_rank || query_count != trace.queries {
        return Err(declined(LuckRushDecline::TerminalQuery, "terminal query has not filed every rank bonus"));
    }
    let mut note_times: Vec<_> =
        notes.into_iter().filter(|&(frame, _)| (frame as i32) <= to).map(|(_, time)| time).collect();
    if note_times.iter().any(|&time| time > ready_at_query) {
        return Err(declined(
            LuckRushDecline::TerminalQuery,
            "terminal note was queried before its lottery commands were ready",
        ));
    }
    note_times.sort_unstable();
    // A common arbitrary gate is insufficient: the current DP's score-probe flag is updated under LUCK.
    // No held probes, or a trace without the recorder's admission certificate, grants no probe conditioning.
    let probe_gate = if trace.probes.is_empty() {
        None
    } else {
        trace.filing_gate.flatten().filter(|&gate| gate == gekisou::M_LUCK)
    };
    Ok(Some(LuckTerminalRush {
        note_times,
        probability: Arc::clone(probability),
        probe_gate,
        note_factors: None,
        native_notes: None,
    }))
}

#[cfg(feature = "search-diagnostics")]
#[derive(Clone, Copy)]
enum Phase {
    Setup,
    Curve,
    Recorder,
    Factors,
    Kernel,
}

#[cfg(feature = "search-diagnostics")]
struct Timing {
    start: std::time::Instant,
    phase: Phase,
    value: LuckScoreProfile,
}

#[cfg(feature = "search-diagnostics")]
impl Default for Timing {
    fn default() -> Self {
        Self { start: std::time::Instant::now(), phase: Phase::Setup, value: LuckScoreProfile::default() }
    }
}

#[cfg(feature = "search-diagnostics")]
impl Timing {
    fn next(&mut self, phase: Phase) {
        let elapsed = self.start.elapsed().as_secs_f64() * 1e3;
        match self.phase {
            Phase::Setup => self.value.model_setup_ms += elapsed,
            Phase::Curve => self.value.curve_dp_ms += elapsed,
            Phase::Recorder => self.value.recorder_run_ms += elapsed,
            Phase::Factors => self.value.terminal_factor_ms += elapsed,
            Phase::Kernel => self.value.terminal_kernel_ms += elapsed,
        }
        self.start = std::time::Instant::now();
        self.phase = phase;
    }
}

#[cfg(feature = "search-diagnostics")]
impl Drop for Timing {
    fn drop(&mut self) {
        self.next(self.phase);
        // A prepass is not a completed score evaluation; only its actual phase work is recorded.
        profile::record(self.value);
    }
}
