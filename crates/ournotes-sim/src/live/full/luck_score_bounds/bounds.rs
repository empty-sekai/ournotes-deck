//! The all-path score bounds of one formation from its recording and lottery law.

use super::*;

fn f32_real(value: F32Interval) -> F64Interval {
    F64Interval::new(f64::from(value.lower()), f64::from(value.upper())).expect("ordered finite native support")
}

pub(super) fn checked_support(lower: i64, upper: i64) -> Result<I32Interval, Error> {
    let lo = i32::try_from(lower).map_err(|_| refuse("score support may wrap i32; refine paths"))?;
    let hi = i32::try_from(upper).map_err(|_| refuse("score support may wrap i32; refine paths"))?;
    I32Interval::new(lo, hi)
}

/// A query is the native note sum plus integer multiples of already filed rank bonuses.
/// Keep those bonuses as shared random variables: subtracting two snapshots must cancel a
/// bonus present with the same coefficient, even when its expectation/support is still wide.
#[derive(Clone)]
pub(super) struct QueryParts {
    pub(super) note_mean: F64Interval,
    pub(super) note_support: I32Interval,
    pub(super) fixed_coefficients: Vec<u8>,
    /// The last score frame the query sums.
    pub(super) to: i32,
    /// The first score frame the query executed: the notes of the frames before it kept their native values since
    /// the previous query.
    pub(super) executed_from: i32,
    /// For a rank snapshot, each summed note's score frame, support and mean.
    pub(super) notes: Option<Vec<(i32, I32Interval, F64Interval)>>,
}

/// The note sum and filed bonuses snapshot `end` gained since snapshot `start`, given by their query indices into
/// the parts of every query. The frames that no query after `start` up to `end` executed again hold the same notes
/// with the same native values in both snapshots, which cancel exactly: only the notes of the later frames are
/// enclosed, on each side. Without the notes of both snapshots both whole note sums are enclosed separately.
pub(super) fn snapshot_difference(
    start: Option<usize>,
    end: usize,
    queries: &[QueryParts],
    fixed: &[(i32, u8, F64Interval, I32Interval)],
) -> Result<(F64Interval, I32Interval), Error> {
    let executed = start
        .filter(|&index| index <= end)
        .map(|index| queries[index + 1..=end].iter().map(|q| q.executed_from).min().unwrap_or(i32::MAX));
    snapshot_parts_difference(start.map(|index| &queries[index]), &queries[end], executed, fixed)
}

/// [`snapshot_difference`] of the parts of two snapshots, given the first score frame any query after `start` up to
/// `end` executed (`i32::MAX` when none did), or None when the snapshots share no frames.
fn snapshot_parts_difference(
    start: Option<&QueryParts>,
    end: &QueryParts,
    executed: Option<i32>,
    fixed: &[(i32, u8, F64Interval, I32Interval)],
) -> Result<(F64Interval, I32Interval), Error> {
    // The last frame every query after `start` up to `end` left as `start` summed it.
    let kept = start.zip(executed).map(|(start, executed)| start.to.min(executed.saturating_sub(1)));
    let later = |notes: &[(i32, I32Interval, F64Interval)], kept: i32| {
        notes.iter().filter(|&&(frame, _, _)| frame > kept).fold(
            (F64Interval::ZERO, 0i64, 0i64),
            |(mean, lower, upper), &(_, support, note_mean)| {
                (mean.add(note_mean), lower + i64::from(support.lower()), upper + i64::from(support.upper()))
            },
        )
    };
    let (mut mean, mut lower, mut upper) = match (kept, start.and_then(|v| v.notes.as_deref()), end.notes.as_deref()) {
        (Some(kept), Some(a), Some(b)) => {
            let (a_mean, a_lower, a_upper) = later(a, kept);
            let (b_mean, b_lower, b_upper) = later(b, kept);
            (b_mean.subtract(a_mean), b_lower - a_upper, b_upper - a_lower)
        }
        _ => {
            let a_support = start.map_or(I32Interval::point(0), |v| v.note_support);
            (
                end.note_mean.subtract(start.map_or(F64Interval::ZERO, |v| v.note_mean)),
                i64::from(end.note_support.lower()) - i64::from(a_support.upper()),
                i64::from(end.note_support.upper()) - i64::from(a_support.lower()),
            )
        }
    };
    for (index, &(_, _, bonus_mean, bonus_support)) in fixed.iter().enumerate() {
        let a = start.and_then(|v| v.fixed_coefficients.get(index)).copied().unwrap_or(0);
        let b = end.fixed_coefficients.get(index).copied().unwrap_or(0);
        let coefficient = i64::from(b) - i64::from(a);
        if coefficient == 0 {
            continue;
        }
        mean = mean.add(bonus_mean.scale_integer(i128::from(coefficient)));
        let x = coefficient * i64::from(bonus_support.lower());
        let y = coefficient * i64::from(bonus_support.upper());
        lower += x.min(y);
        upper += x.max(y);
    }
    Ok((mean, checked_support(lower, upper)?))
}

fn link_note_probability(
    note: &mut LuckNoteBounds,
    curve: &LuckDpCertifiedResult,
) -> Result<(F64Interval, I32Interval), Error> {
    let index = curve.steps.partition_point(|(time, _)| *time <= note.time_ms);
    let mass = index
        .checked_sub(1)
        .map_or([ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO], |index| {
            curve.steps[index].1
        });
    let mut mean = F64Interval::ZERO;
    let (mut lower, mut upper) = (i32::MAX, i32::MIN);
    for (bucket, probability) in note.buckets.iter().zip(mass) {
        let possible = probability.interval().upper() > 0.0;
        // No native path reaches this probe class at the note, so its true mass is zero.
        let Some(bucket) = bucket else {
            if probability.interval().lower() > 0.0 {
                return Err(refuse("a lottery state has no path through the native factor schedule"));
            }
            continue;
        };
        mean = mean.add(probability.interval().multiply(I32Interval::new(bucket.lower, bucket.upper)?.as_real()));
        if possible {
            lower = lower.min(bucket.lower);
            upper = upper.max(bucket.upper);
        }
    }
    let support = I32Interval::new(lower, upper)?;
    mean = mean.intersect(support.as_real()).ok_or_else(|| refuse("note probability mean misses its support"))?;
    note.probability = Some(mass.map(|probability| probability.interval().into()));
    note.mean = Some(mean.into());
    Ok((mean, support))
}

pub(super) fn note_bounds(
    calc: &LiveScoreCalculator,
    note: &NoteCommand,
    executed: &Classes,
    combo: F32Interval,
    gekisou: F32Interval,
    rush_percent: i32,
) -> Result<(LuckNoteBounds, I32Interval), Error> {
    let point = F32Interval::point;
    let note_percent =
        *calc.note_factor_percent.get(&note.note_type).ok_or_else(|| refuse("unknown score note type"))?;
    let judge_percent =
        *calc.judgement_score_factor_percent.get(&note.score_type).ok_or_else(|| refuse("unknown score judgement"))?;
    let t = point(calc.score_adjustment_factor)?
        .multiply(point(calc.state.band_total_power as f32)?)?
        .multiply(point(calc.music_difficulty_factor)?)?;
    let b = point(note_percent as f32 / 100f32)?.multiply(t)?.multiply(point(judge_percent as f32 / 100f32)?)?;
    let mut buckets = [None; 4];
    let mut combo_hull = None::<F32Interval>;
    let mut lower = i32::MAX;
    let mut upper = i32::MIN;
    for (bucket, out) in buckets.iter_mut().enumerate() {
        let Some(fields) = executed[bucket & 1] else { continue };
        let combo_factor = gekisou.multiply(fields[0].add(combo)?)?;
        if !combo_factor.lower().is_finite() || !combo_factor.upper().is_finite() {
            return Err(refuse("nonfinite combo-factor support"));
        }
        combo_hull = Some(combo_hull.map_or(combo_factor, |h| h.hull(combo_factor)));
        let judge = match note.score_type {
            1 => fields[2],
            2 => fields[3],
            3 => fields[4],
            4 => fields[5],
            _ => point(0.0)?,
        };
        let up = fields[1].add(judge)?;
        let luck = get_luck_factor_percent(if bucket & 2 == 0 { 0 } else { rush_percent }) as f32 / 100f32;
        let x = b
            .multiply(combo_factor)?
            .multiply(up)?
            .multiply(point(luck)?)?
            .divide(point(calc.converted_note_count as f32)?)?;
        let life = if note.life > 0 { 1.0 } else { calc.life_onus_factor };
        let integer = point(calc.assist_factor)?
            .multiply(point(life)?.multiply(x.native_floor_as_float().multiply(point(calc.event_bonus_factor)?)?)?)?
            .native_floor();
        lower = lower.min(integer.lower());
        upper = upper.max(integer.upper());
        *out = Some(integer.into());
    }
    let combo = combo_hull.ok_or_else(|| refuse("a filed note has no native execution"))?;
    Ok((
        LuckNoteBounds {
            note_id: note.note_id,
            time_ms: note.time_ms,
            life: note.life,
            buckets,
            combo: f32_real(combo).into(),
            probability: None,
            mean: None,
        },
        I32Interval::new(lower, upper)?,
    ))
}

/// The widest field enclosure of a note's last execution over its classes.
fn factor_widths(executed: &Classes, widths: &mut [f64; FIELDS]) {
    for fields in executed.iter().flatten() {
        for (width, field) in widths.iter_mut().zip(fields) {
            *width = width.max(f64::from(field.upper()) - f64::from(field.lower()));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn luck_score_bounds_internal(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    details: bool,
    curves: Option<&mut LuckDpCache>,
    recordings: Option<&mut luck_dp::RecordingCache>,
    cancelled: &mut impl FnMut() -> bool,
    counterfactual_solo: bool,
    probability: Option<std::sync::Arc<LuckDpCertifiedResult>>,
) -> Result<Option<LuckScoreBounds>, Error> {
    if cancelled() {
        return Ok(None);
    }
    #[cfg(feature = "search-diagnostics")]
    let phase_start = std::time::Instant::now();
    let mut model = if let Some(ranking) = ranking {
        let mut model = if counterfactual_solo {
            LiveModel::new_gekisou_ranked(master, deck, notes, events, params, setup)?
        } else {
            LiveModel::new_gekisou_external(master, deck, notes, events, params, setup)?
        };
        model.set_rank_confirmation_timeline(ranking)?;
        model
    } else {
        LiveModel::new_gekisou(master, deck, notes, events, params, setup)?
    };
    check_recorder(&model, skills)?;
    #[cfg(feature = "search-diagnostics")]
    let model_setup_ms = phase_start.elapsed().as_secs_f64() * 1e3;
    #[cfg(feature = "search-diagnostics")]
    let phase_start = std::time::Instant::now();
    let has_luck = setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission == gekisou::M_LUCK);
    // Without a LUCK range, Controller::judge/update_luck and pending_lots never consume a lottery. Chain
    // probability predicates may still draw SKILL values, but their writes are confined to the unused luck
    // machine/gauge/points. check_recorder excludes any other stochastic writer and direct-7021 probes remain
    // false. Consequently these draws cannot reach score, judgements, life or fixed rank arrivals; this is a
    // deterministic score law even when native.draws() > 0. It is not a fixed-seed approximation.
    let collect_moments = details || counterfactual_solo;
    let probability = if let Some(probability) = probability {
        probability
    } else if !has_luck {
        std::sync::Arc::new(LuckDpCertifiedResult {
            probe_transitions: vec![1; play.frames.len()],
            rush_transitions: vec![65; play.frames.len()],
            steps: Vec::new(),
            frame_queries: Vec::new(),
            probes: vec![false; skills.shapes.len()],
            probe_starts: crate::live::certified::F64Interval::ZERO,
            range_moments: if collect_moments {
                vec![super::luck_dp::LuckRangeMoments::default(); setup.fevers.len()]
            } else {
                Vec::new()
            },
            peak_states: 1,
            transitions: 0,
        })
    } else {
        let mut empty = LuckDpCache::new(0);
        let curves = curves.unwrap_or(&mut empty);
        let Some(probability) = curves.certified_cancellable_mode(
            master,
            skills,
            notes,
            events,
            params,
            setup,
            play,
            delta_times,
            deck,
            None,
            ranking,
            collect_moments,
            recordings,
            cancelled,
        )?
        else {
            return Ok(None);
        };
        probability
    };
    #[cfg(feature = "search-diagnostics")]
    let curve_dp_ms = phase_start.elapsed().as_secs_f64() * 1e3;
    let probes = probes_in_native_order(&model, skills)?;
    if probes.iter().any(|row| !row.value.is_finite() || row.value <= i32::MIN as f32 / 100000f32) {
        return Err(refuse("a direct score command cannot be safely paired with its signed inverse"));
    }
    let calc = model.score.calc.clone();
    if calc.converted_note_count <= 0
        || ![
            calc.score_adjustment_factor,
            calc.music_difficulty_factor,
            calc.life_onus_factor,
            calc.event_bonus_factor,
            calc.assist_factor,
        ]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(refuse("nonfinite score constants or nonpositive note count"));
    }
    let rush_percent = i32::try_from(setting(master, "gekisou_luck_rush_score_bonus_percent")?)
        .map_err(|_| refuse("Rush percent exceeds i32"))?;
    if 100i32.checked_add(rush_percent).is_none() {
        return Err(refuse("Rush factor may wrap"));
    }
    model.set_luck_weights(skills, Vec::new())?;
    let probe_phase_bound = bind_probe_phase(&model, skills);
    model.score.begin_bounds(
        probes,
        setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission == gekisou::M_LUCK),
    );
    #[cfg(feature = "search-diagnostics")]
    let phase_start = std::time::Instant::now();
    if delta_times.len() != play.frames.len() {
        return Err(Error::Input("one delta time per frame".into()));
    }
    model.random.set_seed(play.base_seed);
    for (index, (frame, &delta)) in play.frames.iter().zip(delta_times).enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        model.frame_timed(frame.time_ms, &frame.judged, delta)?;
    }
    #[cfg(feature = "search-diagnostics")]
    let recorder_run_ms = phase_start.elapsed().as_secs_f64() * 1e3;
    #[cfg(feature = "search-diagnostics")]
    let phase_start = std::time::Instant::now();
    if model.random.draws() != 0 {
        return Err(refuse("the supposedly deterministic recorder consumed random draws"));
    }
    let result = complete_bounds_recording(
        model,
        calc,
        rush_percent,
        probability,
        play,
        probe_phase_bound,
        setup.fevers.len(),
        ranking.is_none() || counterfactual_solo,
        details,
        has_luck,
        cancelled,
    )?;
    #[cfg(feature = "search-diagnostics")]
    profile::record(LuckScoreProfile {
        evaluations: 1,
        model_setup_ms,
        curve_dp_ms,
        recorder_run_ms,
        bound_replay_ms: phase_start.elapsed().as_secs_f64() * 1e3,
    });
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn complete_bounds_recording(
    mut model: LiveModel,
    calc: LiveScoreCalculator,
    rush_percent: i32,
    probability: std::sync::Arc<LuckDpCertifiedResult>,
    play: &LivePlay,
    probe_phase_bound: bool,
    ranges_len: usize,
    rank_queries: bool,
    details: bool,
    has_luck: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<LuckScoreBounds>, Error> {
    if model.gk.as_ref().is_none_or(|g| g.ctrl.states.iter().any(|state| state.state != gekisou::S_FINISH)) {
        return Err(refuse("terminal query precedes a range FINISH"));
    }
    let trace = model.score.bounds_trace.take().expect("bounds recorder enabled");
    let frame_times: Vec<_> = model.trace.iter().map(|&(time, _)| time).collect();
    if !frame_times.iter().copied().eq(play.frames.iter().map(|frame| frame.time_ms)) {
        return Err(refuse("the completed recorder frame clock differs from the probability recording"));
    }
    check_probe_music_boundary(&frame_times, &probability, model.music_length_ms, probe_phase_bound, &trace)?;
    let query_limit = (play.frames.len() as u64)
        .checked_mul(2)
        .and_then(|value| value.checked_add(if rank_queries { 2 * ranges_len as u64 } else { 0 }))
        .ok_or_else(|| Error::Capacity("score query count overflow".into()))?;
    if trace.queries as u64 > query_limit {
        return Err(refuse("unaccounted native calculate entry point"));
    }
    let mut replay = Replay::new(trace.frames, &trace.probes);
    let (mut prev, mut added) = (-1i32, None::<i32>);
    let mut mandatory_added = None::<i32>;
    let mut probability_ready = i32::MIN;
    // (frame, index in the frame, note, index in `replay.notes`)
    let mut filed = Vec::<(usize, usize, NoteCommand, usize)>::new();
    // (frame, position in `filed`) by ascending frame: a query's combo window is a contiguous run.
    let mut by_frame = Vec::<(usize, usize)>::new();
    let mut current_combos = FxHashMap::<(usize, usize), (F32Interval, F32Interval)>::default();
    let mut retained_combos = FxHashMap::<(usize, usize), (F32Interval, F32Interval)>::default();
    let mut queries = Vec::<LuckScoreQueryBounds>::new();
    let detailed_queries: FxHashSet<usize> = trace
        .events
        .iter()
        .flat_map(|event| match event {
            BoundsEvent::Rank { start, end, .. } => [*start, *end],
            _ => [None, None],
        })
        .flatten()
        .collect();
    let mut query_means = Vec::<F64Interval>::new();
    let mut query_supports = Vec::<I32Interval>::new();
    let mut query_parts = Vec::<QueryParts>::new();
    let mut ranges = Vec::new();
    // Pending fixed scores add immediately even outside the score-frame array. Later undo/execute only
    // changes their in-prefix contribution; the filing offset is permanent.
    let mut fixed = Vec::<(i32, u8, F64Interval, I32Interval)>::new();
    let mut pending = None;
    for (event_index, event) in trace.events.iter().enumerate() {
        if event_index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        match event {
            BoundsEvent::Note { frame, index, note } => {
                let at = replay.file_note(*frame, note.time_ms, note.note_id)?;
                by_frame.insert(by_frame.partition_point(|&(f, _)| f <= *frame), (*frame, filed.len()));
                filed.push((*frame, *index, *note, at));
                added = Some(added.map_or(*frame as i32, |old| old.min(*frame as i32)));
                mandatory_added = Some(mandatory_added.map_or(*frame as i32, |old| old.min(*frame as i32)));
            }
            BoundsEvent::Factor { frame, command } => {
                if command.band_total_power != 0 {
                    return Err(refuse("recorder produced an unproved power command"));
                }
                replay.file_command(*frame, command)?;
                added = Some(added.map_or(*frame as i32, |old| old.min(*frame as i32)));
                if command.luck == 0 {
                    mandatory_added = Some(mandatory_added.map_or(*frame as i32, |old| old.min(*frame as i32)));
                }
            }
            BoundsEvent::Potential { frame, .. } => {
                replay.potential(*frame);
                added = Some(added.map_or(*frame as i32, |old| old.min(*frame as i32)));
            }
            BoundsEvent::Probe { frame, time_ms } => {
                replay.probe(*frame, *time_ms)?;
                added = Some(added.map_or(*frame as i32, |old| old.min(*frame as i32)));
            }
            BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                current_combos
                    .insert((*frame, *index), (F32Interval::point(*ordinary)?, F32Interval::point(*gekisou)?));
            }
            BoundsEvent::ProbabilityReady(time) => {
                probability_ready = probability_ready.max(*time);
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                let Some(end) = *end else {
                    return Err(refuse("rank end query was not recorded"));
                };
                // Network FEVER_START may snapshot the initial zero before any score calculation.
                // Both snapshots refer to the very same filed bonus values. Cancel their integer
                // coefficients before enclosing the remaining difference; never assume independence.
                let (mean, support) = snapshot_difference(*start, end, &query_parts, &fixed)?;
                let x = i128::from(support.lower()) * i128::from(*percent) / 100;
                let y = i128::from(support.upper()) * i128::from(*percent) / 100;
                let bonus_support = I32Interval::new(
                    i32::try_from(x.min(y)).map_err(|_| refuse("rank bonus wraps"))?,
                    i32::try_from(x.max(y)).map_err(|_| refuse("rank bonus wraps"))?,
                )?;
                let bonus = if support.lower() == support.upper() {
                    bonus_support.as_real()
                } else {
                    rank_mean_bounds(mean, support, *percent)?
                };
                pending = Some((get_frame(*time_ms), bonus, bonus_support));
                ranges.push(LuckRangeScoreBounds {
                    range: *range,
                    start_query: *start,
                    end_query: end,
                    percent: *percent,
                    mean: mean.into(),
                    support: support.into(),
                    bonus_mean: bonus.into(),
                    bonus_support: bonus_support.into(),
                    luck_points_mean: probability.range_moments.get(*range).map(|m| m.luck_points.into()),
                    lot_results_mean: probability.range_moments.get(*range).map(|m| m.lot_results.map(Into::into)),
                });
            }
            BoundsEvent::Query { time_ms, to } => {
                // The admitted controller/ordinary predicates never read score. Only native rank
                // snapshots and the terminal result observe an integer score; those query IDs are
                // all retained. Other queries still execute every factor diff/undo count and combo
                // history update below. Their intermediate integer sums may wrap modulo 2^32, but
                // do not change a later sum or any control-flow input: native addition/subtraction
                // are modular, and each actually observed snapshot is separately proved in i32.
                // Thus production need not re-enclose every note thousands of unused times.
                let measure =
                    details || detailed_queries.contains(&query_means.len()) || query_means.len() + 1 == trace.queries;
                replay.query(*to)?;
                let u = added.map_or(*to, |frame| (*to).min(frame - 1));
                let start = if u < prev { u + 1 } else { prev + 1 };
                let mandatory_u = mandatory_added.map_or(*to, |frame| (*to).min(frame - 1));
                let mandatory_start = if mandatory_u < prev { mandatory_u + 1 } else { prev + 1 };
                let mut factor_width = [0.0; FIELDS];
                let mut measured = Vec::new();
                let retain_notes = details && detailed_queries.contains(&query_means.len());
                let mut note_count = 0;
                let (mut lo, mut hi) = (0i64, 0i64);
                let mut mean = F64Interval::ZERO;
                let mut rank_notes = detailed_queries.contains(&query_means.len()).then(Vec::new);
                // A note first enters a query at or after `start` (its filing lowers `added`, and an unqueried
                // note lies after `prev`), so every note's observation is checked in this window once.
                let first = by_frame.partition_point(|&(f, _)| (f as i32) < start);
                for &(frame, at) in by_frame[first..].iter().take_while(|&&(f, _)| f as i32 <= *to) {
                    let key = (frame, filed[at].1);
                    let current =
                        *current_combos.get(&key).ok_or_else(|| refuse("note combo observation is missing"))?;
                    if frame as i32 >= mandatory_start {
                        retained_combos.insert(key, current);
                    } else {
                        retained_combos
                            .entry(key)
                            .and_modify(|old| {
                                old.0 = old.0.hull(current.0);
                                old.1 = old.1.hull(current.1);
                            })
                            .or_insert(current);
                    }
                }
                // Measured sums keep the filing order of their outward-rounded additions.
                let measured_notes: &[_] = if measure { &filed } else { &[] };
                for &(frame, index, ref note, at) in
                    measured_notes.iter().filter(|(frame, _, _, _)| *frame as i32 <= *to)
                {
                    let key = (frame, index);
                    let (combo, gekisou_combo) =
                        *retained_combos.get(&key).ok_or_else(|| refuse("note has no possible execution"))?;
                    let executed = &replay.notes[at].executed;
                    factor_widths(executed, &mut factor_width);
                    let (mut bounds, mut support) =
                        note_bounds(&calc, note, executed, combo, gekisou_combo, rush_percent)?;
                    let note_mean = if note.time_ms <= probability_ready {
                        let (mean, linked_support) = link_note_probability(&mut bounds, &probability)?;
                        support = linked_support;
                        mean
                    } else {
                        support.as_real()
                    };
                    mean = mean.add(note_mean);
                    lo += i64::from(support.lower());
                    hi += i64::from(support.upper());
                    if let Some(notes) = &mut rank_notes {
                        notes.push((frame as i32, support, note_mean));
                    }
                    note_count += 1;
                    if retain_notes {
                        measured.push(bounds);
                    }
                }
                let note_support = checked_support(lo, hi)?;
                let note_mean = mean;
                if let Some((frame, bonus, support)) = pending.take() {
                    fixed.push((frame, u8::from(frame > *to), bonus, support));
                }
                let mut fixed_coefficients = Vec::with_capacity(fixed.len());
                for &(frame, offset, bonus, support) in &fixed {
                    let coefficient = i64::from(offset) + i64::from(frame <= *to);
                    fixed_coefficients.push(coefficient as u8);
                    mean = mean.add(bonus.scale_integer(i128::from(coefficient)));
                    lo += coefficient * i64::from(support.lower());
                    hi += coefficient * i64::from(support.upper());
                }
                let support = if measure { checked_support(lo, hi)? } else { I32Interval::point(0) };
                query_means.push(mean);
                query_supports.push(support);
                query_parts.push(QueryParts {
                    note_mean,
                    note_support,
                    fixed_coefficients,
                    to: *to,
                    executed_from: start,
                    notes: rank_notes,
                });
                if details {
                    queries.push(LuckScoreQueryBounds {
                        time_ms: *time_ms,
                        score_frame: *to,
                        mean: mean.into(),
                        support: support.into(),
                        factor_width,
                        note_count,
                        notes: measured,
                    });
                }
                prev = *to;
                added = None;
                mandatory_added = None;
            }
        }
    }
    if pending.is_some() || query_means.is_empty() {
        return Err(refuse("terminal query has not filed every rank bonus"));
    }
    let mut final_mean = F64Interval::ZERO;
    let mut final_notes = Vec::new();
    for (note_index, &(frame, index, ref note, at)) in
        filed.iter().filter(|(frame, _, _, _)| *frame as i32 <= prev).enumerate()
    {
        if note_index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        let (combo, gekisou_combo) =
            *retained_combos.get(&(frame, index)).ok_or_else(|| refuse("final note combo is missing"))?;
        let (mut bounds, _) = note_bounds(&calc, note, &replay.notes[at].executed, combo, gekisou_combo, rush_percent)?;
        if note.time_ms > probability_ready {
            return Err(refuse("final note has pending lottery commands"));
        }
        let (mean, _) = link_note_probability(&mut bounds, &probability)?;
        final_mean = final_mean.add(mean);
        if details {
            final_notes.push(bounds);
        }
    }
    let final_note_mean = final_mean;
    let mut final_rank_mean = F64Interval::ZERO;
    for &(frame, offset, bonus, _) in &fixed {
        let coefficient = i128::from(offset) + i128::from(frame <= prev);
        final_rank_mean = final_rank_mean.add(bonus.scale_integer(coefficient));
    }
    final_mean = final_mean.add(final_rank_mean);
    let final_support =
        if has_luck { *query_supports.last().expect("terminal query") } else { I32Interval::point(model.score()) };
    if !has_luck {
        final_mean = F64Interval::integer(model.score() as i128);
    }
    Ok(Some(LuckScoreBounds {
        model: "independent nominal draws; all-path native arithmetic enclosure, not an exact expectation or search completion; note probabilities link after their native lottery commands are filed",
        final_support: final_support.into(),
        exact_final_life: Some(model.current_life()),
        final_mean: final_mean.into(),
        final_note_mean: final_note_mean.into(),
        final_rank_mean: final_rank_mean.into(),
        final_notes,
        queries,
        ranges,
        query_limit,
        actual_queries: trace.queries,
        probability_peak_states: probability.peak_states,
        probability_transitions: probability.transitions,
    }))
}
