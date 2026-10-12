//! The recording of a formation: the native run of its factor skills and the filing of its actions.

use super::*;

pub(super) fn consume_bound(frames: usize, mut judgements: impl Iterator<Item = usize>) -> Result<(), Error> {
    let total = judgements.try_fold(frames, |sum, count| sum.checked_add(count));
    if total.is_none_or(|total| total > i32::MAX as usize) {
        return Err(Error::Capacity("LUCK DP cannot prove native rush_combo avoids signed wrapping".into()));
    }
    Ok(())
}

/// Watches a reduced recording for effects whose application order follows the performance order (see
/// [`LuckDpCache::certified_order_class`]).
#[derive(Default)]
struct OrderWatch {
    /// The performers whose effects the current phase applied: every effect but the score-ups, result guarantees and
    /// lot and gauge factors.
    pub(super) members: u8,
    /// The performers whose lot or gauge factors the current phase filed.
    factor_members: u8,
    /// The pairs of performers whose effects one phase applied, and once the recording finishes, the pairs whose lot
    /// or gauge factors one phase filed when their sums may round.
    pub(super) pairs: PositionPairs,
    /// The pairs of performers whose lot or gauge factors one phase filed.
    factor_pairs: PositionPairs,
    broken: Option<&'static str>,
}

impl OrderWatch {
    pub(super) fn phase(&mut self) {
        (self.members, self.factor_members) = (0, 0);
    }

    pub(super) fn apply(&mut self, model: &LiveModel, item: Listed) {
        let Listed::Cond { updater, u } = item else {
            self.broken = Some("a live skill applies in the reduced recording");
            return;
        };
        let condition = &model.cond[updater];
        let member = condition.member;
        if member >= PAIR_POSITIONS {
            self.broken = Some("a performer position has no pairs");
            return;
        }
        match model.rows[condition.updater.effect(condition.updater.updaters[u].effect).row].effect_type {
            // Score-ups go to the reduced calculator, which the recording never reads. Result guarantees go to the
            // lottery machine's guarantee list, which only a lottery draw reads, and the recording makes none.
            2000 | 2001 | 2005 | 11005 => {}
            11000 | 11001 => {
                self.factor_pairs |= pairs_with(member, self.factor_members);
                self.factor_members |= 1 << member;
            }
            _ => {
                self.pairs |= pairs_with(member, self.members);
                self.members |= 1 << member;
            }
        }
    }

    pub(super) fn finish(&mut self, model: &LiveModel) {
        if model.random.draws() != 0 {
            self.broken = Some("the recording draws a random value");
        } else if !model.gk.as_ref().is_some_and(|gk| gk.ctrl.factor_sums_exact()) {
            self.pairs |= self.factor_pairs;
        }
    }
}

/// Run a native weighted frame's BEFORE/skills phase. Weighted mode draws no lottery, but records deterministic
/// gauge-speed commands with the exact updater lifecycle and binary32 filing order.
fn record_before(
    model: &mut LiveModel,
    time: i32,
    delta: f32,
    mut watch: Option<&mut OrderWatch>,
) -> Result<(), Error> {
    model.frame_time = time;
    let gk = model.gk.as_mut().expect("DP requires Gekisou");
    gk.fever.update(time, &mut gk.fever_updates);
    let mut handle = Handle { sc: &mut model.scorectl, score: &mut model.score };
    let mut env = Env { random: &mut model.random, handle: &mut handle };
    gk.ctrl.before_update(delta, time, &gk.fever_updates, 0, &mut env)?;
    let input = FrameInput { time_ms: time, music_length_ms: model.music_length_ms, is_live_finished: false };
    for condition in &mut model.cond {
        condition.updater.begin_frame();
    }
    // The live's per-frame buffers: no model frame is open while the recorder runs its phases.
    let mut listed = std::mem::take(&mut model.scratch.listed);
    let mut updated = std::mem::take(&mut model.scratch.updated);
    for phase in PHASES {
        let gk =
            model.gk.as_ref().map(|g| GkView { ctrl: &g.ctrl, prev_lots: &g.prev_lots, prev_lot_ms: g.prev_lot_ms });
        let mut context = CheckCtx {
            life: &mut model.life,
            random: &mut model.random,
            frame_time: time,
            current_combo: 0,
            judged: &[],
            events: &[],
            gk,
            prev_confirmed_rank: None,
        };
        listed.clear();
        for (updater, condition) in model.cond.iter_mut().enumerate() {
            condition.updater.update_into(phase, input, &mut context, &mut updated)?;
            for &u in &updated {
                if condition.updater.updaters[u].state.state != STAY {
                    listed.push(Listed::Cond { updater, u });
                }
            }
        }
        if let Some(watch) = watch.as_deref_mut() {
            watch.phase();
        }
        for &item in &listed {
            if let Some(watch) = watch.as_deref_mut() {
                watch.apply(model, item);
            }
            model.apply(item)?;
        }
    }
    (model.scratch.listed, model.scratch.updated) = (listed, updated);
    Ok(())
}

pub(super) fn record_after(model: &mut LiveModel, time: i32, judged: &[gekisou::GkNote]) -> Result<(), Error> {
    let gk = model.gk.as_mut().expect("Gekisou checked");
    let mut handle = Handle { sc: &mut model.scorectl, score: &mut model.score };
    let mut env = Env { random: &mut model.random, handle: &mut handle };
    gk.ctrl.update(time, judged, &gk.fever_updates, 0, &mut env)
}

/// Diagnostic totals of the recording pass of the calling thread (diagnostic builds only; zero elsewhere).
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckRecordProfile {
    pub calls: u64,
    /// Recordings without a lottery condition skill, and recordings that ran the life recorder.
    pub without_skills: u64,
    pub with_life: u64,
    pub total_ms: f64,
    pub life_setup_ms: f64,
    pub life_frames_ms: f64,
    pub before_ms: f64,
    pub after_ms: f64,
}

#[cfg(feature = "search-diagnostics")]
thread_local! {
    static RECORD_PROFILE: std::cell::RefCell<LuckRecordProfile> = std::cell::RefCell::new(LuckRecordProfile::default());
}

#[cfg(feature = "search-diagnostics")]
pub(super) fn timed<T>(slot: fn(&mut LuckRecordProfile) -> &mut f64, run: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let out = run();
    let ms = started.elapsed().as_secs_f64() * 1e3;
    RECORD_PROFILE.with(|profile| *slot(&mut profile.borrow_mut()) += ms);
    out
}

#[cfg(feature = "search-diagnostics")]
fn count(slot: fn(&mut LuckRecordProfile) -> &mut u64) {
    RECORD_PROFILE.with(|profile| *slot(&mut profile.borrow_mut()) += 1);
}

#[cfg(not(feature = "search-diagnostics"))]
#[inline(always)]
fn count(_: fn(&mut LuckRecordProfile) -> &mut u64) {}

#[cfg(not(feature = "search-diagnostics"))]
#[inline(always)]
pub(super) fn timed<T>(_: fn(&mut LuckRecordProfile) -> &mut f64, run: impl FnOnce() -> T) -> T {
    run()
}

/// Return and reset the calling thread's recording totals. These are measurements only.
pub fn take_luck_record_profile() -> LuckRecordProfile {
    #[cfg(feature = "search-diagnostics")]
    {
        RECORD_PROFILE.with(|profile| std::mem::take(&mut *profile.borrow_mut()))
    }
    #[cfg(not(feature = "search-diagnostics"))]
    {
        LuckRecordProfile::default()
    }
}

/// Run the reduced native interpreter (and the deterministic life recorder when a retained row reads life or a
/// judgement can convert) and record the propagation's inputs. Failures before the first frame return at once; a
/// failure inside the frame loop ends the transcript (see [`Transcript::failure`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn record<M: Mass>(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    collect_moments: bool,
) -> Result<Transcript<M>, Error> {
    timed(
        |p| &mut p.total_ms,
        || {
            record_frames(
                master,
                skills,
                notes,
                skill_events,
                params,
                setup,
                play,
                delta_times,
                deck,
                probes,
                ranking,
                collect_moments,
            )
        },
    )
}

/// What a recording takes besides the live: the reduced performers, their lottery condition skills with the effect
/// rows ([`lottery_conditions`]) and plan, and the complete formation when the recording reads its life.
pub(super) struct PreparedRecording<M> {
    pub(super) reduced: Vec<Performer>,
    pub(super) cond: Vec<CondSkill>,
    pub(super) rows: Vec<EffectRow>,
    pub(super) plan: Plan<M>,
    pub(super) life_deck: Option<Vec<Performer>>,
    pub(super) collect_moments: bool,
}

/// The checks of a recording that precede its reduced model.
pub(super) fn check_recording(
    master: &Master,
    skills: &LuckSkills,
    play: &LivePlay,
    delta_times: &[f32],
    probes: Option<&[Option<usize>]>,
    deck: &[Performer],
) -> Result<(), Error> {
    // Initial/precomputed draws choose `next` without adding score. Each consumed result calls add_score
    // once; non-overlapping Luck ranges consume at most once per note plus one pending result per frame.
    // Thus saturation at four Criticals cannot hide a later native i32 wrap back to zero.
    consume_bound(play.frames.len(), play.frames.iter().map(|frame| frame.judged.len()))?;
    if delta_times.len() != play.frames.len() {
        return Err(Error::Input("LUCK DP needs one delta time per frame".into()));
    }
    if probes.is_some_and(|probes| probes.len() != skills.shapes.len()) {
        return Err(Error::Input(format!("LUCK DP needs one probe per shape ({})", skills.shapes.len())));
    }
    for performer in deck {
        if luck::luck_signature(master, performer)?.is_none() {
            return Err(Error::Unsupported(
                "LUCK DP: an ordinary lottery effect or cumulative lottery condition".into(),
            ));
        }
    }
    check_held_state_identities(master, deck)
}

/// The performers of a reduced recording: without their live and support skills.
pub(super) fn reduced_deck(deck: &[Performer]) -> Vec<Performer> {
    deck.iter()
        .cloned()
        .map(|mut performer| {
            performer.live_skill = None;
            performer.support_skills.clear();
            performer
        })
        .collect()
}

/// The lottery condition skills of the reduced performers of `deck` with their plan, and whether the recording reads
/// the life of the complete formation: when an action reads the life, or a held conversion can change a judgement of
/// the play. Otherwise every judgement reaches the controller as declared and no action reads a phase life.
///
/// The life comparisons of the conditions are folded over the life range of the formation when it holds one
/// ([`life_range`]). The reduced model plays the same judgements with a part of the formation's skills, so its life
/// stays in that range too, and its native run answers a folded comparison as the fold does.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_recording<M: Mass>(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    params: LiveParams,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    collect_moments: bool,
) -> Result<PreparedRecording<M>, Error> {
    check_recording(master, skills, play, delta_times, probes, deck)?;
    let reduced = reduced_deck(deck);
    let (mut cond, rows) = lottery_conditions(master, &reduced, params.skill_target_music_type, skills)?;
    let converts = luck_play_converts(master, deck, notes, play);
    fold_life_conditions(&mut cond, &rows, || if converts { None } else { life_range(master, deck, play) });
    let plan = compile::<M>(master, &cond, &rows, skills, probes, collect_moments)?;
    count(|p| &mut p.calls);
    if cond.is_empty() {
        count(|p| &mut p.without_skills);
    }
    let life = converts || plan.actions.iter().any(|(_, _, checker)| checker.as_ref().is_some_and(reads_life));
    if life {
        count(|p| &mut p.with_life);
    }
    Ok(PreparedRecording { reduced, cond, rows, plan, life_deck: life.then(|| deck.to_vec()), collect_moments })
}

#[allow(clippy::too_many_arguments)]
fn record_frames<M: Mass>(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    probes: Option<&[Option<usize>]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    collect_moments: bool,
) -> Result<Transcript<M>, Error> {
    let prepared =
        prepare_recording::<M>(master, skills, notes, params, play, delta_times, deck, probes, collect_moments)?;
    let skeleton = record_skeleton(
        master,
        skills,
        notes,
        skill_events,
        params,
        setup,
        play,
        delta_times,
        &prepared.reduced,
        prepared.life_deck.as_deref(),
        ranking,
        &mut || false,
    )?
    .expect("an uncancelled recording");
    Ok(file(&skeleton, &prepared.plan, collect_moments))
}

/// Keep the condition skills that hold a lot or gauge factor (11000, 11001), with the effect rows they read
/// renumbered in the order they are first read.
fn factor_skills(cond: &mut Vec<CondSkill>, rows: &mut Vec<EffectRow>) {
    cond.retain(|skill| {
        skill.updater.effects().iter().any(|effect| matches!(rows[effect.row].effect_type, 11000 | 11001))
    });
    let mut number = vec![usize::MAX; rows.len()];
    let mut kept = Vec::new();
    for skill in cond.iter_mut() {
        skill.updater.renumber_rows(|row| {
            if number[row] == usize::MAX {
                number[row] = kept.len();
                kept.push(rows[row].clone());
            }
            number[row]
        });
    }
    *rows = kept;
}

/// The identity of the skeleton of a prepared recording without a life recorder: its factor skills with their rows
/// ([`factor_skills`]). The live and the reduced model's other inputs are those of the classes that hold it.
pub(super) fn skeleton_key<M>(prepared: &PreparedRecording<M>) -> Vec<u8> {
    let (mut cond, mut rows) = (prepared.cond.clone(), prepared.rows.clone());
    factor_skills(&mut cond, &mut rows);
    format!("{rows:?}/{cond:?}").into_bytes()
}

/// The skeleton of a recording of the reduced performers `reduced`: the native run of their model with only the
/// condition skills that hold a lot or gauge factor ([`factor_skills`]), and of the life recorder of the complete
/// formation `life_deck` when the recording reads it. None when cancelled.
///
/// The recording reads the controller's range states, which follow the fevers and the frame times, its lot and
/// gauge factor commands, which only the factor effects file, the judgements the life recorder converts and the
/// phase lives the actions read. The other effects of the lottery condition skills write the luck gauge and lot
/// count of a range, its bonus points, the lottery machine's result guarantees and the reduced score calculator:
/// a weighted controller draws no lot, so no reader of the gauge or the lot count runs, the machine is copied before
/// the first frame, and no condition reads the bonus points or the score. Their actions are filed from the plan
/// ([`file`]). Only the range-start gauge can fail natively, on a Luck gauge maximum below 1, which is refused here.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_skeleton(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    skill_events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    reduced: &[Performer],
    life_deck: Option<&[Performer]>,
    ranking: Option<&[crate::replay::RankConfirmation]>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Skeleton>, Error> {
    let mut model =
        LiveModel::build(master, reduced, notes, &[], params, Some(setup), ranking.is_some(), None, Some(skills))?;
    if let Some(ranking) = ranking {
        model.set_rank_confirmation_timeline(ranking)?;
    }
    let LiveModel { cond, rows, .. } = &mut model;
    factor_skills(cond, rows);
    let life = life_deck
        .map(|deck| {
            timed(|p| &mut p.life_setup_ms, || life_recorder(master, deck, notes, skill_events, params, setup, ranking))
        })
        .transpose()?;
    record_native(model, life, notes, play, delta_times, cancelled)
}

/// Run `model` (and the life recorder `life`) through the frames of the play and record what the transcript reads
/// of them. Failures before the first frame return at once; a failure inside the frame loop ends the skeleton.
pub(super) fn record_native(
    mut model: LiveModel,
    mut life: Option<LiveModel>,
    notes: &[LiveNote],
    play: &LivePlay,
    delta_times: &[f32],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Skeleton>, Error> {
    let gk = model.gk.as_mut().expect("Gekisou setup supplied");
    let ranges: Vec<_> = gk.ctrl.ranges.iter().map(|range| (range.start_ms, range.end_ms, range.mission)).collect();
    if gk.ctrl.states.iter().zip(&ranges).any(|(state, range)| range.2 == M_LUCK && state.luck.gauge_max < 1) {
        return Err(Error::Unsupported("LUCK DP: a Luck gauge maximum below 1".into()));
    }
    let templates = gk.ctrl.states.iter().map(|state| state.luck.clone()).collect();
    let machine = gk.ctrl.machine.clone();
    gk.ctrl.luck_weighted = true;
    let note_map: FxHashMap<_, _> = notes.iter().map(|note| (note.note_id, note)).collect();
    let mut skeleton = Skeleton {
        templates,
        machine,
        luck: ranges.iter().map(|range| range.2 == M_LUCK).collect(),
        frames: Vec::new(),
        notes: Vec::new(),
        hits: Vec::new(),
        pending: Vec::new(),
        failure: None,
        pairs: None,
    };
    let mut watch = OrderWatch::default();
    let mut previous_frame = i32::MIN;
    for (index, (frame, &delta)) in play.frames.iter().zip(delta_times).enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        let recorded = record_frame(
            &mut model,
            life.as_mut(),
            &ranges,
            &note_map,
            frame,
            delta,
            previous_frame,
            &mut skeleton,
            &mut watch,
        );
        if let Err(error) = recorded {
            skeleton.failure = Some(error);
            break;
        }
        previous_frame = frame.time_ms;
    }
    watch.finish(&model);
    skeleton.pairs = watch.broken.is_none().then_some(watch.pairs);
    Ok(Some(skeleton))
}

/// Record one native frame. Every check that precedes the frame's propagation fails before the frame is appended;
/// the native AFTER phase runs once it is appended, as in the interleaved computation.
#[allow(clippy::too_many_arguments)]
fn record_frame(
    model: &mut LiveModel,
    life: Option<&mut LiveModel>,
    ranges: &[(i32, i32, i64)],
    note_map: &FxHashMap<i32, &LiveNote>,
    frame: &PlayFrame,
    delta: f32,
    previous_frame: i32,
    out: &mut Skeleton,
    watch: &mut OrderWatch,
) -> Result<(), Error> {
    if frame.time_ms <= previous_frame || !delta.is_finite() || delta < 0.0 {
        return Err(Error::Input("LUCK DP needs increasing frames and finite nonnegative deltas".into()));
    }
    let mut judged = Vec::with_capacity(frame.judged.len());
    let mut previous_note = previous_frame;
    for judgement in &frame.judged {
        let note = note_map
            .get(&judgement.note_id)
            .ok_or_else(|| Error::Input(format!("unknown note {}", judgement.note_id)))?;
        if note.time_ms <= previous_frame
            || note.time_ms > frame.time_ms
            || note.time_ms < previous_note
            || judgement.judgement_time_ms != note.time_ms
        {
            return Err(Error::Unsupported(
                "LUCK DP requires notes in their first chart-time frame, in chart-time order".into(),
            ));
        }
        if luck::luck_judgement_class(judgement.judgement).is_none() {
            return Err(Error::Unsupported(format!("LUCK DP judgement {}", judgement.judgement)));
        }
        previous_note = note.time_ms;
        judged.push((note.note_id, note.note_operate_type, note.time_ms, judgement.judgement));
    }
    let phase_life = if let Some(life) = life {
        timed(|p| &mut p.life_frames_ms, || life.frame_timed(frame.time_ms, &frame.judged, delta))?;
        if life.draws() != 0 {
            return Err(Error::Unsupported("LUCK life recorder consumed a random value".into()));
        }
        for (judged, &(_, converted, _)) in judged.iter_mut().zip(life.frame_judgements()) {
            if converted != judged.3 {
                watch.broken = Some("a judgement is converted");
            }
            judged.3 = converted;
        }
        life.phase_life.expect("life recorder phase trace")
    } else {
        [model.life.current_life; 2]
    };
    timed(|p| &mut p.before_ms, || record_before(model, frame.time_ms, delta, Some(watch)))?;
    let controller = &model.gk.as_ref().expect("Gekisou checked").ctrl;
    let states = |range: usize| controller.states[range].state;
    let updates = &controller.state_updates;
    let current = controller.current_playing_index;
    let target = controller.dp_playing_range_index();
    let current_luck = current >= 0 && ranges[current as usize].2 == M_LUCK;
    let gate = updates.iter().any(|&i| ranges[i].2 == M_LUCK) || current_luck;
    let (mut active, mut active_luck) = (0usize, false);
    for (range, state) in controller.states.iter().enumerate() {
        if (S_START..S_FINISH).contains(&state.state) {
            active += 1;
            active_luck |= ranges[range].2 == M_LUCK;
        }
    }
    if active > 1 && active_luck {
        return Err(Error::Unsupported("LUCK DP: a Luck range overlaps another active range".into()));
    }
    let start =
        updates.iter().find_map(|&range| (ranges[range].2 == M_LUCK && states(range) == S_START).then_some(range));
    let complete = updates.iter().any(|&i| ranges[i].2 == M_LUCK && states(i) == S_COMPLETE);
    let finish = updates.iter().any(|&i| ranges[i].2 == M_LUCK && states(i) == S_FINISH);
    if finish && updates.iter().any(|&i| states(i) == S_START) {
        // The old rush is closed at frame time, while a new range's note can file a command at
        // an EARLIER chart time. That needs overlapping historical span state, outside this prototype.
        return Err(Error::Unsupported("LUCK DP: another range starts in a Luck finish frame".into()));
    }
    for &(_, note_type, note_time, judgement) in &judged {
        for (range, &(begin, end, mission)) in ranges.iter().enumerate() {
            if mission == M_LUCK && begin <= note_time && note_time <= end {
                let (buff, speed) = controller.dp_factors_at(note_time);
                out.hits.push(Hit { range, buff, speed, consumes: states(range) <= S_END });
            }
        }
        out.notes.push(Judged { time_ms: note_time, note_type, judgement, hits: out.hits.len() });
    }
    for (range, state) in controller.states.iter().enumerate() {
        if ranges[range].2 == M_LUCK && state.state == S_PLAYING {
            out.pending.push((range, controller.dp_factors_at(frame.time_ms).0));
        }
    }
    let recorded = NativeFrame {
        time_ms: frame.time_ms,
        repeat: 1,
        start,
        complete,
        finish,
        gate,
        current_luck,
        target,
        phase_life,
        notes: out.notes.len(),
        pending: out.pending.len(),
    };
    out.push_frame(recorded);
    timed(|p| &mut p.after_ms, || record_after(model, frame.time_ms, &judged))
}

/// The transcript of a recording: the native frames of `skeleton`, with the actions of `plan` filed in every gated
/// frame with a playing range. A range-start action is filed only in the frame where a Luck range starts.
pub(super) fn file<M: Mass>(skeleton: &Skeleton, plan: &Plan<M>, collect_moments: bool) -> Transcript<M> {
    let mut out = Transcript {
        collect_moments,
        templates: skeleton.templates.clone(),
        machine: skeleton.machine.clone(),
        luck: skeleton.luck.clone(),
        probes: plan.probes.clone(),
        miss_rows: plan.actions.iter().any(|(_, action, _)| matches!(action, Action::MissGauge { .. })),
        frames: Vec::with_capacity(skeleton.frames.len()),
        notes: skeleton.notes.clone(),
        hits: skeleton.hits.clone(),
        actions: Vec::new(),
        pending: Vec::new(),
        failure: skeleton.failure.clone(),
    };
    let mut pending_from = 0;
    for frame in &skeleton.frames {
        if frame.gate && frame.target >= 0 {
            for (phase, action, condition) in &plan.actions {
                let chance = condition
                    .as_ref()
                    .map_or(Some(M::ONE), |checker| chance::<M>(checker, frame.phase_life[(*phase - 1) as usize]))
                    .expect("compiled start condition");
                match *action {
                    Action::StartGauge { value, .. } if frame.start.is_some() => {
                        out.actions.push(Action::StartGauge { value, chance })
                    }
                    Action::StartMinimum { result, .. } if frame.start.is_some() => {
                        out.actions.push(Action::StartMinimum { result, chance })
                    }
                    Action::MissGauge { value } => out.actions.push(Action::MissGauge { value }),
                    Action::CriticalPoints { value, .. } => out.actions.push(Action::CriticalPoints { value, chance }),
                    Action::StartPoints { value, .. } if frame.start.is_some() => {
                        out.actions.push(Action::StartPoints { value, chance })
                    }
                    _ => {}
                }
            }
        }
        out.pending.extend_from_slice(&skeleton.pending[pending_from..frame.pending]);
        pending_from = frame.pending;
        out.push_frame(Frame {
            time_ms: frame.time_ms,
            repeat: frame.repeat,
            start: frame.start,
            complete: frame.complete,
            finish: frame.finish,
            gate: frame.gate,
            current_luck: frame.current_luck,
            target: frame.target,
            notes: frame.notes,
            actions: out.actions.len(),
            pending: out.pending.len(),
        });
    }
    out
}

/// The native part of a recording ([`record_native`]): everything its transcript reads except the actions.
pub(super) struct Skeleton {
    pub(super) templates: Vec<LuckScore>,
    pub(super) machine: LotteryMachine,
    /// One flag per range: whether it is a Luck range.
    pub(super) luck: Vec<bool>,
    pub(super) frames: Vec<NativeFrame>,
    pub(super) notes: Vec<Judged>,
    pub(super) hits: Vec<Hit>,
    /// `(range, lot buff at the frame time)` of every playing Luck range, in range order.
    pub(super) pending: Vec<(usize, i32)>,
    /// A recording failure after the last recorded frame.
    pub(super) failure: Option<Error>,
    /// The pairs of performers whose effects one phase applied and may not commute ([`OrderWatch`]); None when the
    /// recording shows no order class.
    pub(super) pairs: Option<PositionPairs>,
}

/// One native frame, or `repeat` consecutive quiet frames (no note, no Luck range transition) with equal records.
#[derive(Clone, Copy, Debug)]
pub(super) struct NativeFrame {
    pub(super) time_ms: i32,
    pub(super) repeat: u32,
    /// The Luck range that starts in this frame.
    pub(super) start: Option<usize>,
    pub(super) complete: bool,
    pub(super) finish: bool,
    pub(super) gate: bool,
    /// Whether the current playing range is a Luck range.
    pub(super) current_luck: bool,
    pub(super) target: i32,
    /// The life each phase's actions read.
    pub(super) phase_life: [i32; 2],
    /// Exclusive ends of the frame's entries in the note and pending lists.
    pub(super) notes: usize,
    pub(super) pending: usize,
}

impl NativeFrame {
    pub(super) fn quiet(&self, notes_from: usize) -> bool {
        self.notes == notes_from && self.start.is_none() && !self.complete && !self.finish
    }
}

impl Skeleton {
    /// Append a native frame, merged into the previous frame when both are quiet with equal records: the merged
    /// frames file the same actions.
    pub(super) fn push_frame(&mut self, frame: NativeFrame) {
        let n = self.frames.len();
        if let Some(&previous) = self.frames.last() {
            let (previous_notes, previous_pending) =
                if n == 1 { (0, 0) } else { (self.frames[n - 2].notes, self.frames[n - 2].pending) };
            if previous.quiet(previous_notes)
                && frame.quiet(previous.notes)
                && previous.repeat < u32::MAX
                && (previous.gate, previous.current_luck, previous.target, previous.phase_life)
                    == (frame.gate, frame.current_luck, frame.target, frame.phase_life)
                && self.pending[previous_pending..previous.pending] == self.pending[previous.pending..frame.pending]
            {
                self.pending.truncate(previous.pending);
                self.frames[n - 1].repeat += 1;
                return;
            }
        }
        self.frames.push(frame);
    }
}
