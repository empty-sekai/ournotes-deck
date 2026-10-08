//! Terminal joint Rush/probe probabilities and completed native expectation enclosures.
//!
//! This runs the admitted deterministic recorder and the certified lottery DP, but never the factor-history
//! replay. It certifies which joint law belongs to the terminal notes; the caller still has to prove the supplied
//! integer note caps. The optional native kernel independently certifies both expectation endpoints, integer
//! support and every historical rank contribution before it supplies a completed score summary.

use super::*;
use std::mem::size_of;
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
    /// Established only by a completed admitted recorder, independently of terminal note projection.
    exact_final_life: Option<i32>,
}

/// Completed power-independent recording and factor preparation. Construction requires the structural
/// recorder's no-score-feedback admission and a complete prefix proof that rejects every power command.
/// The retained base has no native integer caps or score expectation: those are rebuilt at the caller's P.
pub(super) struct TerminalRecipe {
    trace: BoundsTrace,
    ingredients: terminal_prefix::TerminalIngredients,
    linked: bool,
    base: LuckTerminalRush,
}

impl TerminalRecipe {
    pub(super) fn curve(&self) -> &Arc<LuckDpCertifiedResult> {
        &self.base.probability
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        // ComboObserver is recording-only scratch and is reset before publication. Every retained event,
        // probe, terminal vector and historical prefix allocation is charged; the curve is shared separately.
        size_of::<Self>()
            + self.trace.events.capacity() * size_of::<BoundsEvent>()
            + self.trace.probes.capacity() * size_of::<ProbeRow>()
            + self.ingredients.allocated_bytes()
            - size_of::<terminal_prefix::TerminalIngredients>()
            + self.base.cache_allocation_bytes()
            - size_of::<LuckTerminalRush>()
    }

    fn evaluate(
        &self,
        calc: &LiveScoreCalculator,
        power: i32,
        rush: i32,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<LuckTerminalRush> {
        if cancelled() {
            return None;
        }
        let mut terminal = self.base.cache_clone();
        if !apply_kernel(calc, power, rush, &self.trace, &self.ingredients, self.linked, &mut terminal, cancelled) {
            return None;
        }
        (!cancelled()).then_some(terminal)
    }
}

/// Power enters only the original native note arithmetic and rank/support checks. In particular a failure
/// at one power is not a cached failure at another; every recipe evaluation repeats this entire kernel.
#[allow(clippy::too_many_arguments)]
fn apply_kernel(
    calc: &LiveScoreCalculator,
    power: i32,
    rush: i32,
    trace: &BoundsTrace,
    ingredients: &terminal_prefix::TerminalIngredients,
    linked: bool,
    terminal: &mut LuckTerminalRush,
    cancelled: &mut impl FnMut() -> bool,
) -> bool {
    match terminal_kernel::build(
        calc,
        power,
        rush,
        trace,
        ingredients,
        linked,
        &terminal.note_times,
        &terminal.probability,
        &mut *cancelled,
    ) {
        Ok(native) => terminal.native_notes = Some(native),
        Err(trace_drift::Decline::Cancelled) => return false,
        Err(error) => {
            #[cfg(feature = "search-diagnostics")]
            profile::record(LuckScoreProfile {
                terminal_kernel_refusals: 1,
                terminal_capacity_refusals: u64::from(error == trace_drift::Decline::Capacity),
                ..Default::default()
            });
            #[cfg(not(feature = "search-diagnostics"))]
            let _ = error;
        }
    }
    !cancelled()
}

impl LuckTerminalRush {
    pub(super) fn cache_curve(&self) -> &Arc<LuckDpCertifiedResult> {
        &self.probability
    }

    /// Owned result storage only; the cache separately charges each distinct shared probability curve.
    pub(super) fn cache_allocation_bytes(&self) -> usize {
        size_of::<Self>()
            + self.note_times.capacity() * size_of::<i32>()
            + self.note_factors.as_ref().map_or(0, |factors| factors.capacity() * size_of::<[f64; 2]>())
            + self.native_notes.as_ref().map_or(0, |native| native.caps.capacity() * size_of::<[i32; 4]>())
    }

    pub(super) fn cache_clone(&self) -> Self {
        Self {
            note_times: self.note_times.clone(),
            probability: Arc::clone(&self.probability),
            probe_gate: self.probe_gate,
            note_factors: self.note_factors.clone(),
            native_notes: self.native_notes.as_ref().map(|native| terminal_kernel::Prepared {
                power: native.power,
                caps: native.caps.clone(),
                mean_upper: native.mean_upper,
                expectation: native.expectation,
            }),
            exact_final_life: self.exact_final_life,
        }
    }

    fn recipe_base(&self) -> Self {
        Self {
            note_times: self.note_times.clone(),
            probability: Arc::clone(&self.probability),
            probe_gate: self.probe_gate,
            note_factors: self.note_factors.clone(),
            native_notes: None,
            exact_final_life: self.exact_final_life,
        }
    }

    /// A complete two-sided expectation for this exact power, together with an independent native integer
    /// support and deterministic terminal life. Every terminal note and historical rank snapshot is enclosed.
    /// A nonsingleton interval remains subject to comparison/refinement; it is neither an exact rational mean
    /// nor a score distribution for nonlinear payoffs. Refusal leaves `LuckScoreSession::summary` available.
    pub fn terminal_summary(&self, total_power: i64) -> Option<LuckScoreSummary> {
        let native = self.native_notes.as_ref()?;
        (total_power == i64::from(native.power)).then_some(())?;
        let expectation = native.expectation?;
        let life = self.exact_final_life?;
        #[cfg(feature = "search-diagnostics")]
        profile::record(LuckScoreProfile { evaluations: 1, terminal_evaluations: 1, ..Default::default() });
        Some(LuckScoreSummary {
            final_mean: expectation.mean.into(),
            final_support: expectation.support.into(),
            exact_constant_score: (expectation.support.lower() == expectation.support.upper())
                .then_some(expectation.support.lower()),
            exact_final_life: Some(life),
            probability_peak_states: self.probability.peak_states,
            probability_transitions: self.probability.transitions,
        })
    }

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
    #[cfg(test)]
    pub(crate) fn test_rush_cap_preparation_unfused(
        &mut self,
        deck: &[Performer],
        curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> LuckRushPreparation {
        match prepare_policy(self, deck, curves, &mut cancelled, false) {
            Ok(Some(value)) => LuckRushPreparation::Ready(value),
            Ok(None) => LuckRushPreparation::Stopped,
            Err((reason, error)) => LuckRushPreparation::Unavailable { reason, error },
        }
    }

    /// Complete a native expected-score enclosure with the terminal kernel when its full history proof is
    /// available, otherwise run the factor-history evaluator. Cancellation never becomes a fallback result.
    /// Callers that already prepared this deck may reuse `LuckTerminalRush::terminal_summary` directly and use
    /// `summary` after a refusal, so no successful preparation is recorded again.
    pub fn summary_or_terminal(
        &mut self,
        deck: &[Performer],
        mut curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckScoreSummary>, Error> {
        match self.rush_cap_preparation(deck, curves.as_deref_mut(), &mut cancelled) {
            LuckRushPreparation::Ready(terminal) => {
                if cancelled() {
                    #[cfg(feature = "search-diagnostics")]
                    profile::record(LuckScoreProfile { terminal_cancellations: 1, ..Default::default() });
                    return Ok(None);
                }
                if let Some(summary) = terminal.terminal_summary(i64::from(self.params.total_power)) {
                    return Ok(Some(summary));
                }
            }
            LuckRushPreparation::Unavailable { .. } => {}
            LuckRushPreparation::Stopped => return Ok(None),
        }
        #[cfg(feature = "search-diagnostics")]
        profile::record(LuckScoreProfile { terminal_replay_fallbacks: 1, ..Default::default() });
        self.summary(deck, curves, cancelled)
    }

    /// Prepare terminal joint probabilities without factor-history replay. A caller may condition its native
    /// Rush multiplier, and matching direct probe amplitudes only under the observed LUCK gate, while retaining
    /// all ordinary effects and complete history/rank/conversion allowances. The recorder/DP admission is shared
    /// with `summary`.
    ///
    /// Unsupported shapes and unproved terminal mappings return `Unavailable`; cancellation returns `Stopped`.
    /// Neither is a completed score evaluation. Curves enter the existing cache only after complete propagation.
    /// A completed capability can reuse the bounded program cache under its full initialized state and exact
    /// power; an equal result then needs no new probability recording, native history or terminal kernel.
    pub fn rush_cap_preparation(
        &mut self,
        deck: &[Performer],
        curves: Option<&mut LuckDpCache>,
        mut cancelled: impl FnMut() -> bool,
    ) -> LuckRushPreparation {
        let result = match prepare(self, deck, curves, &mut cancelled) {
            Ok(Some(capability)) => LuckRushPreparation::Ready(capability),
            Ok(None) => LuckRushPreparation::Stopped,
            Err((reason, error)) => LuckRushPreparation::Unavailable { reason, error },
        };
        #[cfg(feature = "search-diagnostics")]
        profile::record(LuckScoreProfile {
            terminal_cancellations: u64::from(matches!(result, LuckRushPreparation::Stopped)),
            terminal_capacity_refusals: u64::from(matches!(
                result,
                LuckRushPreparation::Unavailable { error: Error::Capacity(_), .. }
            )),
            ..Default::default()
        });
        result
    }
}

pub(super) fn fixed_truth(checker: &Checker) -> Option<bool> {
    match checker {
        Checker::Fixed(value) => Some(*value),
        Checker::And { items, .. } => items.iter().try_fold(true, |value, item| Some(value & fixed_truth(item)?)),
        Checker::Or(items) => items.iter().try_fold(false, |value, item| Some(value | fixed_truth(item)?)),
        Checker::Not(inner) => fixed_truth(inner).map(|value| !value),
        _ => None,
    }
}

/// Every retained positive row whose complete fixed condition is true follows the common untimed direct
/// trigger and gate. The recorder admission proves that neither a timer, reset nor a release can separate it.
/// Row indices are unique native effect instances; equal owners retain all of their individual contributions.
pub(super) fn required_probe_lower(
    model: &LiveModel,
    rows: &[LuckScoreRow],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<f64, trace_drift::Decline> {
    use trace_drift::Decline;
    let mut by_row = FxHashMap::default();
    by_row.try_reserve(rows.len()).map_err(|_| Decline::Capacity)?;
    for (index, row) in rows.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Err(Decline::Cancelled);
        }
        if by_row.insert(row.row, row).is_some() {
            return Err(Decline::Incomplete);
        }
    }
    let mut sum = F64Interval::ZERO;
    let mut index = 0usize;
    for skill in &model.cond {
        for effect in skill.updater.effects() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Decline::Cancelled);
            }
            index = index.checked_add(1).ok_or(Decline::CountOverflow)?;
            let Some(row) = by_row.remove(&effect.row) else { continue };
            if !row.may_hold || row.value < 0.0 {
                continue;
            }
            if effect.condition.as_ref().map_or(Some(true), fixed_truth).ok_or(Decline::Incomplete)? {
                sum = sum.add(F64Interval::point(f64::from(row.value)).map_err(|_| Decline::Nonfinite)?);
            }
        }
    }
    if cancelled() {
        return Err(Decline::Cancelled);
    }
    if !by_row.is_empty() {
        return Err(Decline::Incomplete);
    }
    sum.lower().is_finite().then_some(sum.lower().max(0.0)).ok_or(Decline::Nonfinite)
}

fn prepare(
    session: &mut LuckScoreSession<'_>,
    deck: &[Performer],
    curves: Option<&mut LuckDpCache>,
    cancelled: &mut impl FnMut() -> bool,
) -> PreparationResult {
    prepare_policy(session, deck, curves, cancelled, true)
}

fn separate_probability(
    session: &mut LuckScoreSession<'_>,
    deck: &[Performer],
    curves: &mut LuckDpCache,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Arc<LuckDpCertifiedResult>>, (LuckRushDecline, Error)> {
    curves
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
        .map_err(|error| (LuckRushDecline::ProbabilityDomain, error))
}

fn prepare_policy(
    session: &mut LuckScoreSession<'_>,
    deck: &[Performer],
    curves: Option<&mut LuckDpCache>,
    cancelled: &mut impl FnMut() -> bool,
    allow_fused: bool,
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
    let score_rows = model.luck_score_rows(session.skills);
    let required_probe_lower = match required_probe_lower(&model, &score_rows, cancelled) {
        Ok(lower) => Some(lower),
        Err(trace_drift::Decline::Cancelled) => return Ok(None),
        Err(error) => {
            #[cfg(feature = "search-diagnostics")]
            profile::record(LuckScoreProfile {
                terminal_capacity_refusals: u64::from(error == trace_drift::Decline::Capacity),
                ..Default::default()
            });
            #[cfg(not(feature = "search-diagnostics"))]
            let _ = error;
            None
        }
    };
    let probes: Vec<_> = score_rows
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
    let mut empty = LuckDpCache::new(0);
    let curves = curves.unwrap_or(&mut empty);
    let capacity = curves.program_capacity();
    curves.programs.limit(capacity);
    if !session.program_scope_ready && capacity > 0 {
        session.program_scope =
            program::scope(session.skills, session.notes, session.events, session.play, session.delta_times);
        session.program_scope_ready = true;
    }
    let identity = session
        .program_scope
        .as_ref()
        .filter(|_| capacity > 0)
        .and_then(|scope| program::identity(&mut model, scope, rush));
    // The identity above describes the fresh native model. Every request, including recipe hits, repeats
    // ordinary/full-model admission and the private no-score-feedback gate before skipping any recording.
    model.set_luck_weights(session.skills, Vec::new()).map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
    model.score.begin_bounds(probes, true);
    model.score.certify_bounds_filings(probe_gate);
    let record_only = model.try_enable_bounds_record_only();
    if session.delta_times.len() != session.play.frames.len() {
        return Err((LuckRushDecline::RecorderAdmission, Error::Input("one delta time per frame".into())));
    }
    if cancelled() {
        return Ok(None);
    }
    let cached = identity.as_ref().and_then(|identity| curves.programs.get_terminal(identity, native_power));
    if cancelled() {
        return Ok(None);
    }
    if let Some(cached) = cached {
        let terminal = cached.cache_clone();
        return Ok((!cancelled()).then_some(terminal));
    }
    let recipe =
        identity.as_ref().filter(|_| record_only).and_then(|identity| curves.programs.get_terminal_recipe(identity));
    if cancelled() {
        return Ok(None);
    }
    if let Some(recipe) = recipe {
        #[cfg(feature = "search-diagnostics")]
        timing.next(Phase::Kernel);
        let Some(terminal) = recipe.evaluate(&model.score.calc, native_power, rush, cancelled) else {
            return Ok(None);
        };
        if let Some(identity) = identity
            && curves.programs.insert_terminal(identity, native_power, &terminal, cancelled).is_none()
        {
            return Ok(None);
        }
        return Ok(Some(terminal));
    }
    #[cfg(feature = "search-diagnostics")]
    timing.next(Phase::Curve);
    let mut fused = if allow_fused {
        super::super::luck_dp::fused::Recorder::prepare(
            session.master,
            session.skills,
            session.notes,
            session.events,
            session.params,
            session.setup,
            session.play,
            session.delta_times,
            deck,
            &model,
            record_only,
        )
        .map_err(|error| (LuckRushDecline::ProbabilityDomain, error))?
    } else {
        None
    };
    #[cfg(feature = "search-diagnostics")]
    if fused.is_none() {
        timing.value.fused_refusals += 1;
    }
    let mut probability = if fused.is_none() {
        let Some(probability) = separate_probability(session, deck, curves, cancelled)? else {
            return Ok(None);
        };
        Some(probability)
    } else {
        None
    };
    #[cfg(feature = "search-diagnostics")]
    timing.next(Phase::Recorder);
    model.random.set_seed(session.play.base_seed);
    for (index, (frame, &delta)) in session.play.frames.iter().zip(session.delta_times).enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        if let Some(recording) = &mut fused {
            #[cfg(feature = "search-diagnostics")]
            {
                timing.value.fused_frames += 1;
            }
            let mut observer_failed = false;
            let result = model.frame_timed_observed(frame.time_ms, &frame.judged, delta, &mut |model, _, results| {
                let result = recording.observe(model, frame, delta, results);
                observer_failed = result.is_err();
                result
            });
            if let Err(error) = result {
                if observer_failed {
                    return Err((LuckRushDecline::ProbabilityDomain, error));
                }
                // The separate route completes the reduced DP before any full score frames. A native
                // controller error can precede the observer and still belong to that probability domain.
                // Recover the original error precedence only on failure, using a fresh reduced model.
                drop(fused);
                drop(model);
                #[cfg(feature = "search-diagnostics")]
                timing.next(Phase::Curve);
                return match separate_probability(session, deck, curves, cancelled)? {
                    Some(_) => Err((LuckRushDecline::RecorderAdmission, error)),
                    None => Ok(None),
                };
            }
        } else {
            model
                .frame_timed(frame.time_ms, &frame.judged, delta)
                .map_err(|error| (LuckRushDecline::RecorderAdmission, error))?;
        }
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
    if let Some(recording) = fused {
        #[cfg(feature = "search-diagnostics")]
        {
            timing.value.fused_recordings += 1;
            timing.next(Phase::Curve);
        }
        probability = curves
            .certified_fused(recording, &model, cancelled)
            .map_err(|error| (LuckRushDecline::ProbabilityDomain, error))?;
        if probability.is_none() {
            return Ok(None);
        }
    }
    let probability = probability.expect("complete original or fused probability recorder");
    let trace = model.score.bounds_trace.take().expect("bounds recorder enabled");
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
    terminal.exact_final_life = Some(model.current_life());
    #[cfg(feature = "search-diagnostics")]
    timing.next(Phase::Factors);
    let mut recipe_prefix = None;
    match terminal_prefix::from_trace(&trace, initial_fields, &terminal.note_times, terminal.probe_gate, cancelled) {
        Ok(mut prepared) => {
            if prepared.linked
                // Foreign range finishes can alter the global Rush handle while another mission runs.
                // Their positive probe lower retains zero until that activity correspondence is certified.
                && session.setup.missions.iter().take(session.setup.fevers.len()).all(|&mission| mission == gekisou::M_LUCK)
                && let Some(lower) = required_probe_lower
            {
                // Refusing an optional tightening leaves its previously proved zero lower amplitude.
                let _ = prepared.ingredients.certify_linked_probe_lower(lower);
            }
            #[cfg(feature = "search-diagnostics")]
            profile::record(prepared.profile);
            #[cfg(feature = "search-diagnostics")]
            timing.next(Phase::Kernel);
            if !apply_kernel(
                &model.score.calc,
                native_power,
                rush,
                &trace,
                &prepared.ingredients,
                prepared.linked,
                &mut terminal,
                &mut *cancelled,
            ) {
                return Ok(None);
            }
            terminal.note_factors = Some(prepared.factors);
            // from_trace checked every original Factor command, including zero-valued ones, and refuses
            // band_total_power writes. The structural gate proves the remaining history cannot read P.
            if record_only && identity.is_some() {
                recipe_prefix = Some((prepared.ingredients, prepared.linked));
            }
        }
        Err(trace_drift::Decline::Cancelled) => return Ok(None),
        Err(error) => {
            #[cfg(feature = "search-diagnostics")]
            profile::record(LuckScoreProfile {
                terminal_factor_refusals: 1,
                terminal_capacity_refusals: u64::from(error == trace_drift::Decline::Capacity),
                ..Default::default()
            });
            #[cfg(not(feature = "search-diagnostics"))]
            let _ = error;
        }
    }
    if cancelled() {
        return Ok(None);
    }
    if let Some(identity) = identity {
        let retained = if let Some((ingredients, linked)) = recipe_prefix {
            // No terminal consumer reads the recorder's ComboObserver scratch; Combo events already retain
            // the complete observation history. Drop that scratch instead of silently omitting its charge.
            let mut trace = trace;
            trace.combo = ComboObserver::default();
            let recipe = TerminalRecipe { trace, ingredients, linked, base: terminal.recipe_base() };
            curves.programs.insert_terminal_recipe(identity, native_power, &terminal, recipe, cancelled)
        } else {
            curves.programs.insert_terminal(identity, native_power, &terminal, cancelled)
        };
        if retained.is_none() {
            return Ok(None);
        }
    }
    curves.programs.stats.terminal_builds += 1;
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
        exact_final_life: None,
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
        // Keep preparation work even when no caller consumes its optional completed score summary.
        profile::record(self.value);
    }
}
