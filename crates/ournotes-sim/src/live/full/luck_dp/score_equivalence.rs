//! A narrow pathwise equality certificate for two complete uniform-order score objectives.
//!
//! A common native lottery transcript couples the entire transition history, not merely its marginals.
//! The complete admitted ordinary recorder then supplies identical queries, command filings, combo inputs
//! and rank snapshots. A separate emission guard fixes actual same-owner probe/ordinary ordering; the
//! recorder's overapproximate Probe/Potential events alone would not establish that ordering.
use super::super::luck_score_bounds::{BoundsEvent, BoundsTrace, ProbeRow, check_recorder};
use super::*;
use crate::live::full::LuckExactBudget;
use std::mem::size_of;

mod score_fold;
mod terminal_payoff;
pub use terminal_payoff::{
    LuckTerminalPayoff, LuckTerminalPayoffAttempt, LuckTerminalPayoffBounds, LuckTerminalPayoffSession,
};

const ORDERS: usize = 120;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_KEY_WORDS: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LuckScoreEquivalenceDecline {
    Cancelled,
    WorkBudget,
    Capacity,
    Context,
    RecorderAdmission,
    ProbabilityDomain,
    ProbePredicate,
    ProbeOwner,
    ProbePhase,
    OrdinaryTie,
    ActionChance,
    Unfinished,
    ScoreTrace,
    ControllerTrace,
}

/// Constructed only after all 120 original labelled orders have completed both native admissions and matched.
/// This relates only the two supplied decks in this call's immutable context and exact initial power.
#[derive(Debug)]
pub struct LuckScoreEquivalence {
    _complete: (),
}

#[derive(Debug)]
pub struct LuckScoreEquivalenceAttempt {
    pub certificate: Option<LuckScoreEquivalence>,
    pub decline: Option<LuckScoreEquivalenceDecline>,
    pub recording_runs: u64,
    pub recording_frames: u64,
    pub orders_compared: usize,
    pub timeline_orders: u64,
    pub timeline_paths: u64,
    pub timeline_transitions: u64,
    pub score_fold_queries: u64,
}

enum Failure {
    Decline(LuckScoreEquivalenceDecline),
    Native(Error),
}

type Checked<T> = Result<T, Failure>;

fn declined<T>(reason: LuckScoreEquivalenceDecline) -> Checked<T> {
    Err(Failure::Decline(reason))
}

fn native<T>(result: Result<T, Error>, refusal: LuckScoreEquivalenceDecline) -> Checked<T> {
    result.map_err(|error| match error {
        Error::Unsupported(_) => Failure::Decline(refusal),
        Error::Capacity(_) => Failure::Decline(LuckScoreEquivalenceDecline::Capacity),
        error => Failure::Native(error),
    })
}

fn poll(cancelled: &mut impl FnMut() -> bool) -> Checked<()> {
    if cancelled() { declined(LuckScoreEquivalenceDecline::Cancelled) } else { Ok(()) }
}

fn start_runs(budget: &mut LuckExactBudget, count: u64) -> Checked<()> {
    if budget.remaining_runs < count || budget.remaining_frames == 0 {
        return declined(LuckScoreEquivalenceDecline::WorkBudget);
    }
    budget.remaining_runs -= count;
    Ok(())
}

fn frames(budget: &mut LuckExactBudget, count: u64) -> Checked<()> {
    if budget.remaining_frames < count {
        return declined(LuckScoreEquivalenceDecline::WorkBudget);
    }
    budget.remaining_frames -= count;
    Ok(())
}

/// Sorting chooses only a correspondence between two independently complete sets of 120 labels. It never
/// authorizes a semantic projection: native construction below still receives every original source/field.
fn member_basis(deck: &[Performer; 5]) -> [Performer; 5] {
    let member = deck.clone().map(|mut performer| {
        performer.support_skills.clear();
        performer.gekisou_support_skills.clear();
        performer
    });
    let mut order = [0, 1, 2, 3, 4];
    order.sort_by(|&a, &b| member[a].cmp(&member[b]));
    order.map(|slot| deck[slot].clone())
}

fn next_order(order: &mut [usize; 5]) -> bool {
    let Some(i) = (0..4).rev().find(|&i| order[i] < order[i + 1]) else { return false };
    let j = (i + 1..5).rev().find(|&j| order[i] < order[j]).expect("successor");
    order.swap(i, j);
    order[i + 1..].reverse();
    true
}

#[derive(Debug)]
struct EffectiveProbe {
    owner: i32,
    phase: i64,
    gate: Option<i64>,
    source: usize,
    effect: usize,
    effect_id: i64,
    row: i64,
    kind: i64,
    raw_value: i64,
    mill: i32,
}

/// Native processes PHASES in their declared order, and within each phase all live sources precede
/// conditional sources, whose complete execution lists are applied in model.cond order. A conditional
/// source's old instances and new triggers may interleave, so same-phase rows in the probe's own source
/// are deliberately not ordered by effect ID. The prefix fold may append a probe only after producers
/// that this total source order proves earlier, including every replacement and removal they emit.
fn ordinary_before_probe(phase: i64, condition_source: Option<usize>, probe: &EffectiveProbe) -> bool {
    let (Some(ordinary_phase), Some(probe_phase)) =
        (PHASES.iter().position(|&value| value == phase), PHASES.iter().position(|&value| value == probe.phase))
    else {
        return false;
    };
    ordinary_phase < probe_phase
        || (ordinary_phase == probe_phase && condition_source.is_none_or(|source| source < probe.source))
}

/// Keep even zero-valued effective probes: their native filings still invalidate score frames. False fixed
/// predicates are evaluated exactly rather than through may_hold's conservative boolean approximation.
fn probes(model: &LiveModel, skills: &LuckSkills) -> Checked<(Vec<ProbeRow>, String, Vec<EffectiveProbe>)> {
    let related: FxHashSet<_> = model.luck_score_rows(skills).iter().map(|row| row.row).collect();
    let mut active = Vec::<EffectiveProbe>::new();
    let mut rows = Vec::new();
    for (source, skill) in model.cond.iter().enumerate() {
        for (index, effect) in skill.updater.effects().iter().enumerate() {
            if !related.contains(&effect.row) {
                continue;
            }
            if !matches!(effect.trigger, Some(Checker::LuckRushPlaying(_)))
                || effect.trigger_type != SUSTAINED
                || effect.act != 0.0
                || effect.cumulative.is_some()
                || effect.execute_limit != 0
                || effect.reset.is_some()
                || skill.updater.updaters.iter().any(|u| u.effect == index && u.release.is_some())
            {
                return declined(LuckScoreEquivalenceDecline::ProbePredicate);
            }
            let truth = match &effect.condition {
                None => true,
                Some(condition) => {
                    fixed_condition(condition).ok_or(Failure::Decline(LuckScoreEquivalenceDecline::ProbePredicate))?
                }
            };
            if !truth {
                continue;
            }
            if !PHASES.contains(&effect.phase) {
                return declined(LuckScoreEquivalenceDecline::ProbePhase);
            }
            let owner_type = if skill.skill_type == SKILL_TYPE_GEKISOU { OWNER_MEMBER } else { OWNER_SNAP };
            let owner = (skill.member as i32).wrapping_mul(100).wrapping_add(owner_type);
            if owner == -1 || active.iter().any(|probe| probe.owner == owner) {
                return declined(LuckScoreEquivalenceDecline::ProbeOwner);
            }
            let row = &model.rows[effect.row];
            let divisor = match row.effect_type {
                2000 => 10000.0f32,
                2005 => -10000.0f32,
                _ => return declined(LuckScoreEquivalenceDecline::ProbePredicate),
            };
            let mill = note_factor_mill(row.effect_value as f32 / divisor);
            if mill == i32::MIN {
                return declined(LuckScoreEquivalenceDecline::ProbePredicate);
            }
            let value = if mill != 0 { mill as f32 / 100000.0 } else { 0.0 };
            rows.push(ProbeRow { owner, value });
            active.push(EffectiveProbe {
                owner,
                phase: effect.phase,
                gate: skill.updater.gate_mission(),
                source,
                effect: index,
                effect_id: effect.effect_id,
                row: row.id,
                kind: row.effect_type,
                raw_value: row.effect_value,
                mill,
            });
        }
    }
    // Every admitted same-owner ordinary producer must precede the reconstructed probe in native order.
    for probe in &active {
        for skill in &model.live {
            let owner = (skill.member as i32).wrapping_mul(100).wrapping_add(OWNER_MEMBER);
            if owner != probe.owner {
                continue;
            }
            for effect in &skill.effects {
                if matches!(model.rows[effect.row].effect_type, 2000..=2005)
                    && !ordinary_before_probe(effect.phase, None, probe)
                {
                    return declined(LuckScoreEquivalenceDecline::OrdinaryTie);
                }
            }
        }
        for (source, skill) in model.cond.iter().enumerate() {
            let owner_type = if skill.skill_type == SKILL_TYPE_GEKISOU { OWNER_MEMBER } else { OWNER_SNAP };
            let owner = (skill.member as i32).wrapping_mul(100).wrapping_add(owner_type);
            if owner == probe.owner
                && skill.updater.effects().iter().any(|effect| {
                    !related.contains(&effect.row)
                        && matches!(model.rows[effect.row].effect_type, 2000..=2005)
                        && !ordinary_before_probe(effect.phase, Some(source), probe)
                })
            {
                return declined(LuckScoreEquivalenceDecline::OrdinaryTie);
            }
        }
    }
    let descriptors: Vec<_> = active
        .iter()
        .map(|probe| {
            (
                probe.owner,
                probe.phase,
                probe.gate,
                probe.source,
                probe.effect,
                probe.effect_id,
                probe.row,
                probe.kind,
                probe.raw_value,
                probe.mill,
            )
        })
        .collect();
    let key = super::super::luck_exact::state_identity(&descriptors)
        .ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Capacity))?;
    Ok((rows, key, active))
}

struct Fingerprint {
    controller: Transcript<ProbabilityMass>,
    controller_key: Vec<u64>,
    score: String,
    probes: String,
    final_life: i32,
    recipe: Checked<score_fold::Recipe>,
}

struct Context<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    notes: &'a [LiveNote],
    events: &'a [(i32, i32)],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    delta: &'a [f32],
}

fn transcript_bytes(transcript: &Transcript<ProbabilityMass>) -> usize {
    transcript
        .frames
        .capacity()
        .saturating_mul(size_of::<Frame>())
        .saturating_add(transcript.notes.capacity().saturating_mul(size_of::<Judged>()))
        .saturating_add(transcript.hits.capacity().saturating_mul(size_of::<Hit>()))
        .saturating_add(transcript.actions.capacity().saturating_mul(size_of::<Action<ProbabilityMass>>()))
        .saturating_add(transcript.pending.capacity().saturating_mul(size_of::<(usize, i32)>()))
}

impl Context<'_> {
    fn controller(
        &self,
        deck: &[Performer; 5],
        budget: &mut LuckExactBudget,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Checked<Transcript<ProbabilityMass>> {
        poll(cancelled)?;
        let prepared = native(
            prepare_recording::<ProbabilityMass>(
                self.master,
                self.skills,
                self.notes,
                self.events,
                self.params,
                self.setup,
                self.play,
                self.delta,
                deck,
                None,
                None,
                false,
            ),
            LuckScoreEquivalenceDecline::ProbabilityDomain,
        )?;
        let PreparedRecording { mut model, mut life, plan, .. } = prepared;
        poll(cancelled)?;
        let runs = 1 + u64::from(life.is_some());
        start_runs(budget, runs)?;
        let gk = model.gk.as_mut().expect("Gekisou constructed");
        let ranges: Vec<_> = gk.ctrl.ranges.iter().map(|range| (range.start_ms, range.end_ms, range.mission)).collect();
        let mut transcript = Transcript {
            collect_moments: false,
            templates: gk.ctrl.states.iter().map(|state| state.luck.clone()).collect(),
            machine: gk.ctrl.machine.clone(),
            luck: ranges.iter().map(|range| range.2 == M_LUCK).collect(),
            probes: plan.probes,
            miss_rows: plan.actions.iter().any(|(_, a, _)| matches!(a, Action::MissGauge { .. })),
            frames: Vec::new(),
            notes: Vec::new(),
            hits: Vec::new(),
            actions: Vec::new(),
            pending: Vec::new(),
            failure: None,
        };
        gk.ctrl.luck_weighted = true;
        let note_map: FxHashMap<_, _> = self.notes.iter().map(|note| (note.note_id, note)).collect();
        let mut previous = i32::MIN;
        for (frame, &delta) in self.play.frames.iter().zip(self.delta) {
            poll(cancelled)?;
            frames(budget, runs)?;
            native(
                record_frame(
                    &mut model,
                    life.as_mut(),
                    None,
                    None,
                    &plan.actions,
                    &ranges,
                    &note_map,
                    frame,
                    delta,
                    previous,
                    &mut transcript,
                ),
                LuckScoreEquivalenceDecline::ProbabilityDomain,
            )?;
            previous = frame.time_ms;
            if transcript_bytes(&transcript) > MAX_RECORD_BYTES {
                return declined(LuckScoreEquivalenceDecline::Capacity);
            }
        }
        // Equal outward chance enclosures are not equal exact laws. This narrow certificate accepts only
        // certain/never action predicates; the machine still retains every exact random base/bonus table.
        for action in &transcript.actions {
            let chance = match action {
                Action::StartGauge { chance, .. } | Action::StartMinimum { chance, .. } => *chance,
                Action::MissGauge { .. } => ProbabilityMass::ONE,
                Action::StartPoints { .. } | Action::CriticalPoints { .. } => {
                    return declined(LuckScoreEquivalenceDecline::ProbabilityDomain);
                }
            };
            if chance != ProbabilityMass::ZERO && chance != ProbabilityMass::ONE {
                return declined(LuckScoreEquivalenceDecline::ActionChance);
            }
        }
        Ok(transcript)
    }

    fn fingerprint(
        &self,
        deck: &[Performer; 5],
        budget: &mut LuckExactBudget,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Checked<Fingerprint> {
        poll(cancelled)?;
        let mut model = native(
            LiveModel::new_gekisou(self.master, deck, self.notes, self.events, self.params, self.setup),
            LuckScoreEquivalenceDecline::RecorderAdmission,
        )?;
        let gate = native(check_recorder(&model, self.skills), LuckScoreEquivalenceDecline::RecorderAdmission)?;
        let (probe_rows, probes, active_probes) = probes(&model, self.skills)?;
        model.score.begin_bounds(probe_rows, true);
        // The empty recorder exposes the calculator's actual native frame allocation. Temporarily take
        // it out so the optional clone remains an unobserved, unweighted calculator; oversized instances
        // keep the original strict fingerprint route without allocating a second native frame array.
        let initial_trace = model.score.bounds_trace.take().expect("fresh recorder");
        let initial_score = score_fold::Recipe::admits_frames(initial_trace.frames).then(|| model.score.clone());
        model.score.bounds_trace = Some(initial_trace);
        native(model.set_luck_weights(self.skills, Vec::new()), LuckScoreEquivalenceDecline::RecorderAdmission)?;
        model.score.certify_bounds_filings(gate);
        if !model.try_enable_bounds_record_only() {
            return declined(LuckScoreEquivalenceDecline::RecorderAdmission);
        }
        poll(cancelled)?;
        start_runs(budget, 1)?;
        model.random.set_seed(self.play.base_seed);
        for (frame, &delta) in self.play.frames.iter().zip(self.delta) {
            poll(cancelled)?;
            frames(budget, 1)?;
            native(
                model.frame_timed(frame.time_ms, &frame.judged, delta),
                LuckScoreEquivalenceDecline::RecorderAdmission,
            )?;
            if model
                .score
                .bounds_trace
                .as_ref()
                .is_none_or(|trace| trace.events.capacity().saturating_mul(size_of::<BoundsEvent>()) > MAX_RECORD_BYTES)
            {
                return declined(LuckScoreEquivalenceDecline::Capacity);
            }
        }
        poll(cancelled)?;
        if model.random.draws() != 0 {
            return declined(LuckScoreEquivalenceDecline::RecorderAdmission);
        }
        if model.gk.as_ref().is_none_or(|g| g.ctrl.states.iter().any(|state| state.state != S_FINISH)) {
            return declined(LuckScoreEquivalenceDecline::Unfinished);
        }
        let mut trace = model.score.bounds_trace.take().expect("private recorder");
        // Exhaustive event matching: no factor owner, command, filing, query, combo or rank is projected away.
        for event in &mut trace.events {
            match event {
                BoundsEvent::Note { note, .. } => note.life = i32::from(note.life > 0),
                BoundsEvent::Factor { .. }
                | BoundsEvent::Potential { .. }
                | BoundsEvent::Probe { .. }
                | BoundsEvent::Query { .. }
                | BoundsEvent::Combo { .. }
                | BoundsEvent::ProbabilityReady(_)
                | BoundsEvent::Rank { .. } => {}
            }
        }
        let BoundsTrace {
            events,
            queries,
            frames,
            probes: trace_probes,
            has_luck,
            filing_gate,
            combo: _,
            probe_filings: _,
        } = trace;
        // Both models share the exact immutable calculator inputs and initial power supplied by this Context.
        // Final LIFE is retained too, so the capability never silently transports a different outcome field.
        let final_life = model.current_life();
        let score = super::super::luck_exact::state_identity(&(
            &events,
            queries,
            frames,
            trace_probes,
            has_luck,
            filing_gate,
            final_life,
        ))
        .ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Capacity))?;
        let clock: Vec<_> = self.play.frames.iter().map(|frame| frame.time_ms).collect();
        let rush_percent = native(
            setting(self.master, "gekisou_luck_rush_score_bonus_percent"),
            LuckScoreEquivalenceDecline::Context,
        )? as i32;
        // A recipe refusal does not narrow the original exact-fingerprint admission. It only declines
        // the optional native-fold route when the two ordinary recordings differ.
        let recipe = initial_score.ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Capacity)).and_then(|initial| {
            score_fold::Recipe::new(
                initial,
                events,
                frames,
                queries,
                active_probes,
                self.params.music_length_ms,
                rush_percent,
                &clock,
            )
        });
        let controller = self.controller(deck, budget, cancelled)?;
        let controller_key =
            controller.key().ok_or(Failure::Decline(LuckScoreEquivalenceDecline::ProbabilityDomain))?;
        if controller_key.capacity() > MAX_KEY_WORDS {
            return declined(LuckScoreEquivalenceDecline::Capacity);
        }
        Ok(Fingerprint { controller, controller_key, score, probes, final_life, recipe })
    }
}

/// Optional equality proof for all 120 uniformly weighted, labelled member orders. Both sides use the same
/// complete immutable native context and exact power. All recording work, including stopped/refused attempts,
/// is deducted from the supplied request-wide budget; genuine native errors preserve those deductions.
#[allow(clippy::too_many_arguments)]
pub fn certify_uniform_score_equivalence(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    left: &[Performer; 5],
    right: &[Performer; 5],
    budget: &mut LuckExactBudget,
    mut cancelled: impl FnMut() -> bool,
) -> Result<LuckScoreEquivalenceAttempt, Error> {
    let before = (budget.remaining_runs, budget.remaining_frames);
    let mut orders_compared = 0;
    let mut timeline_orders = 0u64;
    let mut timeline_paths = 0u64;
    let mut timeline_transitions = 0u64;
    let mut fold_work = score_fold::Work::default();
    let mut run = || -> Checked<()> {
        poll(&mut cancelled)?;
        if setup.fevers.is_empty()
            || setup.missions.len() < setup.fevers.len()
            || setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission != M_LUCK)
            || delta_times.len() != play.frames.len()
        {
            return declined(LuckScoreEquivalenceDecline::Context);
        }
        let context = Context { master, skills, notes, events, params, setup, play, delta: delta_times };
        let left = member_basis(left);
        let right = member_basis(right);
        let mut order = [0, 1, 2, 3, 4];
        for ordinal in 0..ORDERS {
            poll(&mut cancelled)?;
            let left = order.map(|slot| left[slot].clone());
            let right = order.map(|slot| right[slot].clone());
            let left = context.fingerprint(&left, budget, &mut cancelled)?;
            let right = context.fingerprint(&right, budget, &mut cancelled)?;
            if left.controller_key != right.controller_key {
                return declined(LuckScoreEquivalenceDecline::ControllerTrace);
            }
            if left.probes != right.probes || left.final_life != right.final_life {
                return declined(LuckScoreEquivalenceDecline::ScoreTrace);
            }
            if left.score == right.score {
                // Complete original stochastic-domain admission, independently of equal marginal values.
                if native(
                    propagate_cancellable(&left.controller, &mut cancelled, None),
                    LuckScoreEquivalenceDecline::ProbabilityDomain,
                )?
                .is_none()
                {
                    return declined(LuckScoreEquivalenceDecline::Cancelled);
                }
            } else {
                let left_recipe = left.recipe?;
                let right_recipe = right.recipe?;
                let clock: Vec<_> = play.frames.iter().map(|frame| frame.time_ms).collect();
                let support = native(
                    super::timeline_support::complete_timeline_support(
                        &left.controller,
                        &clock,
                        left_recipe.has_probes(),
                        &mut cancelled,
                    ),
                    LuckScoreEquivalenceDecline::ProbabilityDomain,
                )?
                .ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Cancelled))?;
                timeline_transitions = timeline_transitions.saturating_add(support.transitions);
                score_fold::evaluate_support(
                    [&left_recipe, &right_recipe],
                    &support,
                    &mut fold_work,
                    &mut cancelled,
                    |_, [left_score, right_score]| {
                        timeline_paths += 1;
                        if left_score == right_score {
                            Ok(())
                        } else {
                            declined(LuckScoreEquivalenceDecline::ScoreTrace)
                        }
                    },
                )?;
                timeline_orders += 1;
            }
            poll(&mut cancelled)?;
            orders_compared += 1;
            if next_order(&mut order) != (ordinal + 1 < ORDERS) {
                return declined(LuckScoreEquivalenceDecline::Context);
            }
        }
        poll(&mut cancelled)
    };
    let result = run();
    let (certificate, decline) = match result {
        Ok(()) => (Some(LuckScoreEquivalence { _complete: () }), None),
        Err(Failure::Decline(reason)) => (None, Some(reason)),
        Err(Failure::Native(error)) => return Err(error),
    };
    Ok(LuckScoreEquivalenceAttempt {
        certificate,
        decline,
        recording_runs: before.0 - budget.remaining_runs,
        recording_frames: before.1 - budget.remaining_frames,
        orders_compared,
        timeline_orders,
        timeline_paths,
        timeline_transitions,
        score_fold_queries: fold_work.queries,
    })
}

/// Test-only audit surface: the very same independently recorded recipe and complete support used by
/// the certificate, exposed as plain edge tuples so native test oracles need not access private DP state.
#[cfg(test)]
pub(crate) type ScoreFoldAuditTimeline = Vec<(usize, u8, i32, bool, bool)>;

#[cfg(test)]
pub(crate) struct ScoreFoldAudit {
    pub paths: Vec<(ScoreFoldAuditTimeline, i32)>,
    pub shared_queries: u64,
    pub independent_queries: u64,
    pub checkpoint_peak_bytes: usize,
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn audit_score_fold(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer; 5],
    checkpoint_limit: Option<usize>,
) -> Result<ScoreFoldAudit, Error> {
    let audit = || -> Checked<ScoreFoldAudit> {
        let context = Context { master, skills, notes, events, params, setup, play, delta: delta_times };
        let mut budget = LuckExactBudget { remaining_runs: 10, remaining_frames: 1_000_000 };
        let fingerprint = context.fingerprint(deck, &mut budget, &mut || false)?;
        let recipe = fingerprint.recipe?;
        let clock: Vec<_> = play.frames.iter().map(|frame| frame.time_ms).collect();
        let support = native(
            super::timeline_support::complete_timeline_support(
                &fingerprint.controller,
                &clock,
                recipe.has_probes(),
                &mut || false,
            ),
            LuckScoreEquivalenceDecline::ProbabilityDomain,
        )?
        .ok_or(Failure::Decline(LuckScoreEquivalenceDecline::Cancelled))?;
        let mut shared_work = score_fold::Work::default();
        let mut shared_scores = vec![None; support.len()];
        let visit = |index: usize, [score]: [i32; 1]| {
            if shared_scores[index].replace(score).is_some() {
                return Err(Failure::Native(Error::Domain("score-fold terminal was visited twice".into())));
            }
            Ok(())
        };
        if let Some(limit) = checkpoint_limit {
            score_fold::evaluate_support_with_limit(
                [&recipe],
                &support,
                &mut shared_work,
                &mut || false,
                limit,
                visit,
            )?;
        } else {
            score_fold::evaluate_support([&recipe], &support, &mut shared_work, &mut || false, visit)?;
        }
        let mut work = score_fold::Work::default();
        let mut paths = Vec::new();
        for index in 0..support.len() {
            let path = native(support.path(index), LuckScoreEquivalenceDecline::Capacity)?;
            let score = recipe.evaluate(&path, &mut work, &mut || false)?;
            if shared_scores[index] != Some(score) {
                return Err(Failure::Native(Error::Domain(
                    "shared-prefix score differs from complete native tape".into(),
                )));
            }
            let key = path
                .into_iter()
                .map(|edge| {
                    (
                        edge.frame,
                        edge.stage,
                        edge.chart_time,
                        edge.kind == super::timeline_support::TimelineKind::Rush,
                        edge.on,
                    )
                })
                .collect();
            paths.push((key, score));
        }
        Ok(ScoreFoldAudit {
            paths,
            shared_queries: shared_work.queries,
            independent_queries: work.queries,
            checkpoint_peak_bytes: shared_work.checkpoint_peak_bytes,
        })
    };
    audit().map_err(|failure| match failure {
        Failure::Decline(reason) => Error::Unsupported(format!("score-fold audit declined: {reason:?}")),
        Failure::Native(error) => error,
    })
}
