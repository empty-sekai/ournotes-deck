//! The native score expectation of one formation and order under the independent nominal lottery law, from the
//! exact factor histories of every lottery path.
//!
//! The recorder plays the formation once and files every lottery-dependent command at each of its possible
//! places. [`ExactReplay`] follows every native factor history these filings admit. A note's native score is a
//! function of its last execution's factor state and combo inputs, of its probe class and of whether Rush is
//! running when it is scored, so each of the four lottery classes has an integer support over those histories
//! (usually a single value). The certified lottery law gives the probability of each class in the phase in which
//! the note is scored: a query at a play frame scores the notes before the frame's time with the classes they
//! keep, and the notes at the frame's own time with the Rush the frame's queries see and the probe class before
//! or after the frame's skill boundary. A score's expectation is linear in these. A rank bonus truncates the
//! product of a snapshot difference and its percentage, so its expectation lies within one of the linear term.

use super::exact_paths::{ExactNote, ExactReplay, NoteValue, Orders, rush_mask, undo_floors};
use super::*;
use crate::live::full::luck_dp::{RUSH_FINISH, RUSH_PROBE};

/// One scored note: the integer support of each lottery class 00/01/10/11 (probe bit 0, Rush bit 1; None when no
/// path reaches the probe class), the class probabilities and the expectation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactNote {
    pub note_id: i32,
    pub time_ms: i32,
    pub buckets: [Option<IntegerBounds>; 4],
    pub probability: [RealBounds; 4],
    pub mean: RealBounds,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactRange {
    pub range: usize,
    pub percent: i64,
    /// The range score: the difference of the range's score snapshots.
    pub mean: RealBounds,
    pub support: IntegerBounds,
    pub bonus_mean: RealBounds,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactScore {
    pub final_mean: RealBounds,
    pub final_note_mean: RealBounds,
    pub final_rank_mean: RealBounds,
    pub final_support: IntegerBounds,
    pub ranges: Vec<LuckExactRange>,
    pub scored_notes: usize,
    /// Final notes with more than one integer score in a lottery class of positive probability.
    pub wide_notes: usize,
    /// The largest such spread.
    pub widest_note: i32,
    pub peak_paths: usize,
    pub queries: usize,
    pub probability_peak_states: usize,
    pub probability_transitions: u64,
    pub profile: LuckExactProfile,
    /// The final notes, when requested.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<LuckExactNote>,
}

/// The work of one evaluation. Timings are filled only in diagnostic builds.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactProfile {
    /// Path histories executed by score queries, and the score frames they undid and executed.
    pub runs: u64,
    pub undone_frames: u64,
    pub executed_frames: u64,
    /// Score queries that executed no frame on any path.
    pub idle_queries: u64,
    pub setup_ms: f64,
    pub law_ms: f64,
    pub record_ms: f64,
    pub replay_ms: f64,
    pub measure_ms: f64,
}

/// Milliseconds between laps in diagnostic builds, zero otherwise.
pub(super) struct Stopwatch {
    #[cfg(feature = "search-diagnostics")]
    at: std::time::Instant,
}

impl Stopwatch {
    pub(super) fn start() -> Self {
        Self {
            #[cfg(feature = "search-diagnostics")]
            at: std::time::Instant::now(),
        }
    }

    pub(super) fn lap(&mut self) -> f64 {
        #[cfg(feature = "search-diagnostics")]
        {
            let now = std::time::Instant::now();
            let ms = (now - self.at).as_secs_f64() * 1e3;
            self.at = now;
            ms
        }
        #[cfg(not(feature = "search-diagnostics"))]
        0.0
    }
}

const NO_LOTTERY: [ProbabilityMass; 4] =
    [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO];

/// The class probabilities a note at `time_ms` keeps once every lottery filing up to its time is made.
fn kept_mass(curve: &LuckDpCertifiedResult, time_ms: i32) -> [ProbabilityMass; 4] {
    let index = curve.steps.partition_point(|(time, _)| *time <= time_ms);
    index.checked_sub(1).map_or(NO_LOTTERY, |index| curve.steps[index].1)
}

/// The class probabilities a note reads when a query at play frame `query_ms` scores it. Notes of earlier frames
/// keep their filed lottery state; notes of the query's own frame read the state before that frame's lottery, at
/// their own time, on the side of the frame's skill boundary the query runs.
pub(super) fn query_mass(
    curve: &LuckDpCertifiedResult,
    frame_times: &[i32],
    probability_ready: i32,
    skills_at: i32,
    query_ms: i32,
    note_ms: i32,
) -> Result<[ProbabilityMass; 4], Error> {
    if note_ms <= probability_ready {
        return Ok(kept_mass(curve, note_ms));
    }
    let frame = frame_times.binary_search(&query_ms).map_err(|_| refuse("a score snapshot is not at a play frame"))?;
    let previous = frame.checked_sub(1).map_or(i32::MIN, |frame| frame_times[frame]);
    if previous > probability_ready || note_ms <= previous || note_ms > query_ms {
        return Err(refuse("a score snapshot reads a note outside its frame's lottery state"));
    }
    let index = curve.frame_queries.partition_point(|(time, _)| *time < query_ms);
    let masses = match curve.frame_queries.get(index) {
        Some(&(time, masses)) if time == query_ms => masses,
        _ => return Err(refuse("a judging frame has no certified query probabilities")),
    };
    Ok(if note_ms < query_ms {
        masses[0]
    } else if skills_at == query_ms {
        masses[1]
    } else {
        masses[2]
    })
}

/// The integer support of each lottery class of a note's last execution.
pub(super) fn note_classes(
    calc: &LiveScoreCalculator,
    note: &NoteCommand,
    values: &[NoteValue],
    rush_percent: i32,
    shared_classes: bool,
) -> Result<[Option<(i32, i32)>; 4], Error> {
    let mut buckets = [None::<(i32, i32)>; 4];
    let luck = [0, rush_percent].map(|rush| get_luck_factor_percent(rush) as f32 / 100f32);
    for value in values {
        let (ordinary, gekisou) = (f32::from_bits(value.combo.0), f32::from_bits(value.combo.1));
        let judge = match note.score_type {
            1 => Some(2),
            2 => Some(3),
            3 => Some(4),
            4 => Some(5),
            _ => None,
        };
        // Native combo and score-up factors at the field bounds; both are monotone in every field.
        let factors = |fields: &[f32; FIELDS]| {
            let combo = gekisou * (fields[0] + ordinary);
            let score_up = fields[1] + judge.map_or(0.0, |field| fields[field]);
            (combo, score_up)
        };
        let (combo_a, up_low) = factors(&value.low);
        let (combo_b, up_high) = factors(&value.high);
        let combos = corners(combo_a.min(combo_b), combo_a.max(combo_b));
        let ups = corners(up_low, up_high);
        let classes = if shared_classes { 0..2 } else { usize::from(value.class)..usize::from(value.class) + 1 };
        for (rush, &luck) in luck.iter().enumerate() {
            for &combo in combos.iter().flatten() {
                for &score_up in ups.iter().flatten() {
                    let score =
                        calc.note_score_core(note.life, note.note_type, note.score_type, combo, score_up, luck)?;
                    if score == i32::MIN {
                        return Err(refuse("a note score may overflow"));
                    }
                    for class in classes.clone() {
                        let bucket = &mut buckets[class | rush << 1];
                        *bucket = Some(bucket.map_or((score, score), |(lo, hi)| (lo.min(score), hi.max(score))));
                    }
                }
            }
        }
    }
    Ok(buckets)
}

/// The points at which a native note score over `low..=high` of one factor takes its extremes: the score is a
/// rounded product of the combo and score-up factors with constants, so it is monotone in each factor wherever the
/// other keeps its sign, and the extremes lie at the bounds and at a sign change.
fn corners(low: f32, high: f32) -> [Option<f32>; 3] {
    [Some(low), (low < 0.0 && 0.0 < high).then_some(0.0), (high != low).then_some(high)]
}

/// The expectation and support of a note given its class supports and probabilities.
fn note_expectation(
    buckets: &[Option<(i32, i32)>; 4],
    mass: &[ProbabilityMass; 4],
) -> Result<(F64Interval, I32Interval), Error> {
    let mut mean = F64Interval::ZERO;
    let (mut lower, mut upper) = (i32::MAX, i32::MIN);
    for (bucket, probability) in buckets.iter().zip(mass) {
        let Some((lo, hi)) = *bucket else {
            if probability.interval().lower() > 0.0 {
                return Err(refuse("a lottery class has no path through the native factor schedule"));
            }
            continue;
        };
        mean = mean.add(probability.interval().multiply(I32Interval::new(lo, hi)?.as_real()));
        if probability.interval().upper() > 0.0 {
            lower = lower.min(lo);
            upper = upper.max(hi);
        }
    }
    let support = I32Interval::new(lower, upper)?;
    let mean = mean.intersect(support.as_real()).ok_or_else(|| refuse("note probability mean misses its support"))?;
    Ok((mean, support))
}

/// The class supports of one note in each order of a replay. Orders whose last executions of the note have the
/// same values share their supports.
struct OrderClasses {
    /// For each order, the index of its supports in `distinct`.
    of: Vec<usize>,
    distinct: Vec<[Option<(i32, i32)>; 4]>,
}

fn order_classes(
    calc: &LiveScoreCalculator,
    note: &NoteCommand,
    exact: &ExactNote,
    rush_percent: i32,
    shared_classes: bool,
    lanes: usize,
) -> Result<OrderClasses, Error> {
    if lanes == 1 {
        let values: Vec<_> = exact.values_of(0).collect();
        let buckets = note_classes(calc, note, &values, rush_percent, shared_classes)?;
        return Ok(OrderClasses { of: vec![0], distinct: vec![buckets] });
    }
    let mut seen = FxHashMap::<Vec<u32>, usize>::default();
    let mut classes = OrderClasses { of: Vec::with_capacity(lanes), distinct: Vec::new() };
    let mut key = Vec::new();
    for order in 0..lanes {
        key.clear();
        for value in exact.values_of(order) {
            key.extend([u32::from(value.class), value.combo.0, value.combo.1]);
            key.extend(value.low.map(f32::to_bits));
            key.extend(value.high.map(f32::to_bits));
        }
        let index = match seen.get(key.as_slice()) {
            Some(&index) => index,
            None => {
                let values: Vec<_> = exact.values_of(order).collect();
                classes.distinct.push(note_classes(calc, note, &values, rush_percent, shared_classes)?);
                seen.insert(key.clone(), classes.distinct.len() - 1);
                classes.distinct.len() - 1
            }
        };
        classes.of.push(index);
    }
    Ok(classes)
}

/// The recorded command and query schedule of one or more orders of a formation.
pub(super) struct Schedule {
    /// The events of every order in filing order, each with the orders whose schedule holds it. Only a factor
    /// command may belong to some of the orders.
    pub events: Vec<(BoundsEvent, Orders)>,
    /// The calculator's score frames, the score queries made and the probe rows.
    pub frames: usize,
    pub queries: usize,
    pub probes: Vec<ProbeRow>,
    /// The completed native frame clock and the music length.
    pub frame_times: Vec<i32>,
    pub music_length_ms: i32,
    pub lanes: usize,
}

/// The expected native score of `deck` in its given order under the independent nominal lottery law, with every
/// path's exact factor history. `skills` is [`luck_skills`] of `master`; `ranking` declares an external rank
/// confirmation timeline, otherwise ranks follow the solo rank queries. The lottery law comes from, and enters,
/// `curves` when given. `details` retains the final notes.
#[allow(clippy::too_many_arguments)]
pub fn luck_exact_score(
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
    curves: Option<&mut LuckDpCache>,
    details: bool,
) -> Result<LuckExactScore, Error> {
    let mut watch = Stopwatch::start();
    let mut profile = LuckExactProfile::default();
    let mut model = exact_model(master, deck, notes, events, params, setup, ranking)?;
    check_recorder(&model, skills)?;
    profile.setup_ms = watch.lap();
    let has_luck = has_luck(setup);
    let probability = if has_luck {
        let mut empty = LuckDpCache::new(0);
        let curves = curves.unwrap_or(&mut empty);
        lottery_law(master, skills, notes, events, params, setup, play, delta_times, deck, ranking, curves)?
    } else {
        no_lottery(play, skills)
    };
    profile.law_ms = watch.lap();
    let probes = probes_in_native_order(&model, skills)?;
    check_probes(&probes)?;
    let calc = model.score.calc.clone();
    let rush_percent = rush_percent(master, &calc)?;
    model.set_luck_weights(skills, Vec::new())?;
    let probe_phase_bound = bind_probe_phase(&model, skills);
    let trace = record_exact(&mut model, probes, has_luck, play, delta_times)?;
    let schedule = Schedule {
        events: trace.events.into_iter().map(|event| (event, 1)).collect(),
        frames: trace.frames,
        queries: trace.queries,
        probes: trace.probes,
        frame_times: model.trace.iter().map(|&(time, _)| time).collect(),
        music_length_ms: model.music_length_ms,
        lanes: 1,
    };
    profile.record_ms = watch.lap();
    let deterministic = [model.score()];
    let mut scores = exact_schedule(
        &schedule,
        &calc,
        rush_percent,
        &[probability],
        play,
        probe_phase_bound,
        setup.fevers.len(),
        ranking.is_none(),
        (!has_luck).then_some(&deterministic[..]),
        details,
        profile,
        watch,
    )?;
    Ok(scores.pop().expect("one order"))
}

/// The native model of `deck` in its given order, with the external rank timeline when given.
pub(super) fn exact_model(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LiveModel, Error> {
    Ok(if let Some(ranking) = ranking {
        let mut model = LiveModel::new_gekisou_external(master, deck, notes, events, params, setup)?;
        model.set_rank_confirmation_timeline(ranking)?;
        model
    } else {
        LiveModel::new_gekisou(master, deck, notes, events, params, setup)?
    })
}

/// Plays `model` once with the bounds recorder, which files every lottery-dependent command at each of its
/// possible places, and returns the completed recording.
pub(super) fn record_exact(
    model: &mut LiveModel,
    probes: Vec<ProbeRow>,
    has_luck: bool,
    play: &LivePlay,
    delta_times: &[f32],
) -> Result<BoundsTrace, Error> {
    model.score.begin_bounds(probes, has_luck);
    if delta_times.len() != play.frames.len() {
        return Err(Error::Input("one delta time per frame".into()));
    }
    model.random.set_seed(play.base_seed);
    for (frame, &delta) in play.frames.iter().zip(delta_times) {
        model.frame_timed(frame.time_ms, &frame.judged, delta)?;
    }
    if model.random.draws() != 0 {
        return Err(refuse("the supposedly deterministic recorder consumed random draws"));
    }
    if model.gk.as_ref().is_none_or(|g| g.ctrl.states.iter().any(|state| state.state != gekisou::S_FINISH)) {
        return Err(refuse("terminal query precedes a range FINISH"));
    }
    Ok(model.score.bounds_trace.take().expect("bounds recorder enabled"))
}

/// Whether a range of the live is a LUCK range.
pub(super) fn has_luck(setup: &GekisouSetup) -> bool {
    setup.missions.iter().take(setup.fevers.len()).any(|&mission| mission == gekisou::M_LUCK)
}

/// The lottery law of a live without a LUCK range: every note in class 00.
pub(super) fn no_lottery(play: &LivePlay, skills: &LuckSkills) -> std::sync::Arc<LuckDpCertifiedResult> {
    std::sync::Arc::new(LuckDpCertifiedResult {
        probe_transitions: vec![1; play.frames.len()],
        rush_transitions: vec![65; play.frames.len()],
        steps: Vec::new(),
        frame_queries: Vec::new(),
        probes: vec![false; skills.shapes.len()],
        range_moments: Vec::new(),
        peak_states: 1,
        transitions: 0,
    })
}

/// The certified lottery law of `deck` in its given order, through `curves`.
#[allow(clippy::too_many_arguments)]
pub(super) fn lottery_law(
    master: &Master,
    skills: &LuckSkills,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    deck: &[Performer],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    curves: &mut LuckDpCache,
) -> Result<std::sync::Arc<LuckDpCertifiedResult>, Error> {
    Ok(curves
        .certified_cancellable_mode(
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
            false,
            None,
            &mut || false,
        )?
        .expect("an uncancelled lottery law"))
}

/// Refuses probe rows the replay cannot pair with their signed inverses.
pub(super) fn check_probes(probes: &[ProbeRow]) -> Result<(), Error> {
    if probes.iter().any(|row| !row.value.is_finite() || row.value <= i32::MIN as f32 / 100000f32) {
        return Err(refuse("a direct score command cannot be safely paired with its signed inverse"));
    }
    Ok(())
}

/// The Rush score bonus percent, checked against the calculator's arithmetic.
pub(super) fn rush_percent(master: &Master, calc: &LiveScoreCalculator) -> Result<i32, Error> {
    if calc.converted_note_count <= 0 {
        return Err(refuse("nonpositive note count"));
    }
    let rush_percent = i32::try_from(setting(master, "gekisou_luck_rush_score_bonus_percent")?)
        .map_err(|_| refuse("Rush percent exceeds i32"))?;
    if 100i32.checked_add(rush_percent).is_none() {
        return Err(refuse("Rush factor may wrap"));
    }
    Ok(rush_percent)
}

/// The score queries and rank bonuses of one order of a schedule.
#[derive(Default)]
struct Lane {
    parts: Vec<QueryParts>,
    fixed: Vec<(i32, u8, F64Interval, I32Interval)>,
    pending: Option<(i32, F64Interval, I32Interval)>,
    ranges: Vec<LuckExactRange>,
}

/// The exact expectation of every order of `schedule`, each under its own lottery law in `laws`. The laws share
/// their possible probe and Rush transitions, which shape the paths. `deterministic` gives the constant final score
/// of each order of a live without a LUCK range.
#[allow(clippy::too_many_arguments)]
pub(super) fn exact_schedule(
    schedule: &Schedule,
    calc: &LiveScoreCalculator,
    rush_percent: i32,
    laws: &[std::sync::Arc<LuckDpCertifiedResult>],
    play: &LivePlay,
    probe_phase_bound: bool,
    ranges_len: usize,
    rank_queries: bool,
    deterministic: Option<&[i32]>,
    details: bool,
    mut profile: LuckExactProfile,
    mut watch: Stopwatch,
) -> Result<Vec<LuckExactScore>, Error> {
    let lanes = schedule.lanes;
    if laws.len() != lanes {
        return Err(Error::Input("one lottery law per order".into()));
    }
    // The distinct laws, and the law of each order.
    let mut distinct_laws = Vec::<&LuckDpCertifiedResult>::new();
    let mut law_of = Vec::with_capacity(lanes);
    for law in laws {
        let index = distinct_laws.iter().position(|&other| std::ptr::eq(other, &**law)).unwrap_or_else(|| {
            distinct_laws.push(law);
            distinct_laws.len() - 1
        });
        law_of.push(index);
    }
    let probability = distinct_laws[0];
    if distinct_laws.iter().any(|law| {
        law.probe_transitions != probability.probe_transitions || law.rush_transitions != probability.rush_transitions
    }) {
        return Err(Error::Input("the lottery laws of a joint replay differ in their possible transitions".into()));
    }
    let frame_times = &schedule.frame_times;
    if !frame_times.iter().copied().eq(play.frames.iter().map(|frame| frame.time_ms)) {
        return Err(refuse("the completed recorder frame clock differs from the probability recording"));
    }
    let events = || schedule.events.iter().map(|(event, _)| event);
    check_probe_filings(
        frame_times,
        probability,
        schedule.music_length_ms,
        probe_phase_bound,
        !schedule.probes.is_empty(),
        schedule.frames,
        events(),
    )?;
    if !schedule.probes.is_empty() && probability.probe_transitions.len() != frame_times.len() {
        return Err(refuse("the probe transitions do not cover every skill boundary"));
    }
    if probability.rush_transitions.len() != frame_times.len() {
        return Err(refuse("the Rush transitions do not cover every play frame"));
    }
    let query_limit = (play.frames.len() as u64)
        .checked_mul(2)
        .and_then(|value| value.checked_add(if rank_queries { 2 * ranges_len as u64 } else { 0 }))
        .ok_or_else(|| Error::Capacity("score query count overflow".into()))?;
    if schedule.queries as u64 > query_limit {
        return Err(refuse("unaccounted native calculate entry point"));
    }
    let floors = undo_floors(events(), &probability.rush_transitions)?;
    let mut replay = ExactReplay::new(schedule.frames, &schedule.probes, floors, lanes)?;
    // (frame, note, index in `replay.notes`)
    let mut filed = Vec::<(usize, NoteCommand, usize)>::new();
    let mut combos = FxHashMap::<(usize, usize), (f32, f32)>::default();
    let snapshots: FxHashSet<usize> = events()
        .flat_map(|event| match event {
            BoundsEvent::Rank { start, end, .. } => [*start, *end],
            _ => [None, None],
        })
        .flatten()
        .collect();
    let mut probability_ready = i32::MIN;
    // Play frames whose lotteries have completed, and the lottery filing places of the current play frame.
    let mut ready = 0usize;
    let mut lottery_frames = Vec::new();
    // The next skill boundary's play frame, and the time of the latest skill boundary.
    let (mut boundary, mut skills_at) = (0usize, i32::MIN);
    let mut measured: Vec<Lane> = (0..lanes).map(|_| Lane::default()).collect();
    let mut queries = 0usize;
    for (event, orders) in &schedule.events {
        match event {
            BoundsEvent::Note { frame, index, note } => {
                let at = replay.file_note(*frame, note.time_ms, note.note_id, (*frame, *index))?;
                filed.push((*frame, *note, at));
            }
            BoundsEvent::Factor { frame, command } => {
                if command.band_total_power != 0 {
                    return Err(refuse("recorder produced an unproved power command"));
                }
                replay.file_command(*frame, command, *orders)?;
            }
            BoundsEvent::Potential { frame, start: true } => {
                if rush_mask(&probability.rush_transitions, ready)? & RUSH_FINISH != 0 {
                    replay.finish(*frame)?;
                }
            }
            BoundsEvent::Potential { frame, start: false } => lottery_frames.push(*frame as i32),
            BoundsEvent::Probe { frame, time_ms } => {
                // One boundary per play frame; a frame after music end files its clamped inverse at music end.
                let index = if frame_times.get(boundary) == Some(time_ms) {
                    boundary += 1;
                    boundary - 1
                } else if *time_ms == schedule.music_length_ms && boundary > 0 && frame_times[boundary - 1] > *time_ms {
                    boundary - 1
                } else {
                    return Err(refuse("a probe filing is not at a native skill boundary"));
                };
                let follows = probability.rush_transitions[index] & RUSH_PROBE != 0;
                replay.probe(*frame, *time_ms, probability.probe_transitions[index], follows)?;
                skills_at = frame_times[index];
            }
            BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                combos.insert((*frame, *index), (*ordinary, *gekisou));
            }
            BoundsEvent::ProbabilityReady(time) => {
                if frame_times.get(ready) != Some(time) {
                    return Err(refuse("a lottery completion is not at its play frame"));
                }
                replay.lotteries(&lottery_frames, rush_mask(&probability.rush_transitions, ready)? & 0xff)?;
                lottery_frames.clear();
                ready += 1;
                probability_ready = probability_ready.max(*time);
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                let Some(end) = *end else {
                    return Err(refuse("rank end query was not recorded"));
                };
                for lane in &mut measured {
                    let (mean, support) = snapshot_difference(*start, end, &lane.parts, &lane.fixed)?;
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
                    lane.pending = Some((get_frame(*time_ms), bonus, bonus_support));
                    lane.ranges.push(LuckExactRange {
                        range: *range,
                        percent: *percent,
                        mean: mean.into(),
                        support: support.into(),
                        bonus_mean: bonus.into(),
                    });
                }
            }
            BoundsEvent::Query { time_ms, to } => {
                if !lottery_frames.is_empty() {
                    return Err(refuse("a score query precedes the completion of a play frame's lotteries"));
                }
                let mut query_watch = Stopwatch::start();
                let executed_from = replay.query(*to, &combos)?;
                profile.replay_ms += query_watch.lap();
                let snapshot = snapshots.contains(&queries);
                queries += 1;
                let mut sums = vec![(F64Interval::ZERO, 0i64, 0i64); lanes];
                let mut notes = vec![snapshot.then(Vec::new); lanes];
                if snapshot {
                    for &(frame, ref note, at) in filed.iter().filter(|(frame, _, _)| *frame as i32 <= *to) {
                        let masses = distinct_laws
                            .iter()
                            .map(|law| {
                                query_mass(law, frame_times, probability_ready, skills_at, *time_ms, note.time_ms)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let classes =
                            order_classes(calc, note, &replay.notes[at], rush_percent, replay.shared_classes(), lanes)?;
                        // By (class supports, law).
                        let mut expectations = vec![None; classes.distinct.len() * masses.len()];
                        for (lane, &index) in classes.of.iter().enumerate() {
                            let (mean, support) = match &mut expectations[index * masses.len() + law_of[lane]] {
                                Some(expectation) => *expectation,
                                slot => {
                                    *slot.insert(note_expectation(&classes.distinct[index], &masses[law_of[lane]])?)
                                }
                            };
                            let sum = &mut sums[lane];
                            sum.0 = sum.0.add(mean);
                            sum.1 += i64::from(support.lower());
                            sum.2 += i64::from(support.upper());
                            notes[lane].as_mut().expect("snapshot notes").push((frame as i32, support, mean));
                        }
                    }
                }
                for (order, (lane, ((note_mean, lo, hi), notes))) in
                    measured.iter_mut().zip(sums.into_iter().zip(notes)).enumerate()
                {
                    let note_support = checked_support(lo, hi)?;
                    if let Some((frame, bonus, support)) = lane.pending.take() {
                        lane.fixed.push((frame, u8::from(frame > *to), bonus, support));
                    }
                    // Only the rank snapshots read a query's coefficients.
                    let fixed_coefficients = if snapshot {
                        lane.fixed.iter().map(|&(frame, offset, _, _)| offset + u8::from(frame <= *to)).collect()
                    } else {
                        Vec::new()
                    };
                    lane.parts.push(QueryParts {
                        note_mean,
                        note_support,
                        fixed_coefficients,
                        to: *to,
                        executed_from: executed_from[order],
                        notes,
                    });
                }
            }
        }
    }
    if !lottery_frames.is_empty() || ready != frame_times.len() {
        return Err(refuse("a play frame's lotteries did not complete"));
    }
    let Some(prev) = measured[0].parts.last().map(|last| last.to) else {
        return Err(refuse("the recorder made no score query"));
    };
    if measured.iter().any(|lane| lane.pending.is_some()) {
        return Err(refuse("terminal query has not filed every rank bonus"));
    }
    if deterministic.is_some_and(|scores| scores.len() != lanes) {
        return Err(Error::Input("one deterministic score per order".into()));
    }
    // (note mean, support lower, support upper) and (wide notes, widest note) per order.
    let mut sums = vec![(F64Interval::ZERO, 0i64, 0i64); lanes];
    let mut wide = vec![(0usize, 0i32); lanes];
    let mut final_notes = vec![Vec::new(); lanes];
    let mut scored_notes = 0;
    for &(_, ref note, at) in filed.iter().filter(|(frame, _, _)| *frame as i32 <= prev) {
        if note.time_ms > probability_ready {
            return Err(refuse("final note has pending lottery commands"));
        }
        let masses: Vec<_> = distinct_laws.iter().map(|law| kept_mass(law, note.time_ms)).collect();
        let classes = order_classes(calc, note, &replay.notes[at], rush_percent, replay.shared_classes(), lanes)?;
        // (mean, support, spread) by (class supports, law).
        let mut expectations = vec![None; classes.distinct.len() * masses.len()];
        scored_notes += 1;
        for (order, &index) in classes.of.iter().enumerate() {
            let (buckets, mass) = (&classes.distinct[index], &masses[law_of[order]]);
            let (mean, support, spread) = match &mut expectations[index * masses.len() + law_of[order]] {
                Some(expectation) => *expectation,
                slot => {
                    let (mean, support) = note_expectation(buckets, mass)?;
                    let spread = buckets
                        .iter()
                        .zip(mass)
                        .filter(|(_, p)| p.interval().upper() > 0.0)
                        .filter_map(|(bucket, _)| bucket.map(|(lo, hi)| hi - lo))
                        .max()
                        .unwrap_or(0);
                    *slot.insert((mean, support, spread))
                }
            };
            if spread > 0 {
                wide[order].0 += 1;
                wide[order].1 = wide[order].1.max(spread);
            }
            let sum = &mut sums[order];
            sum.0 = sum.0.add(mean);
            sum.1 += i64::from(support.lower());
            sum.2 += i64::from(support.upper());
            if details {
                final_notes[order].push(LuckExactNote {
                    note_id: note.note_id,
                    time_ms: note.time_ms,
                    buckets: buckets.map(|bucket| bucket.map(|(lower, upper)| IntegerBounds { lower, upper })),
                    probability: mass.map(|p| p.interval().into()),
                    mean: mean.into(),
                });
            }
        }
    }
    profile.measure_ms = watch.lap() - profile.replay_ms;
    profile.runs = replay.runs;
    profile.undone_frames = replay.undone_frames;
    profile.executed_frames = replay.executed_frames;
    profile.idle_queries = replay.idle_queries;
    let mut scores = Vec::with_capacity(lanes);
    for (order, (((lane, (final_note_mean, mut lo, mut hi)), (wide_notes, widest_note)), notes)) in
        measured.into_iter().zip(sums).zip(wide).zip(final_notes).enumerate()
    {
        let mut final_rank_mean = F64Interval::ZERO;
        for &(frame, offset, bonus, support) in &lane.fixed {
            let coefficient = i64::from(offset) + i64::from(frame <= prev);
            final_rank_mean = final_rank_mean.add(bonus.scale_integer(i128::from(coefficient)));
            lo += coefficient * i64::from(support.lower());
            hi += coefficient * i64::from(support.upper());
        }
        let (final_mean, final_support) = match deterministic {
            Some(scores) => (F64Interval::integer(scores[order] as i128), I32Interval::point(scores[order])),
            None => (final_note_mean.add(final_rank_mean), checked_support(lo, hi)?),
        };
        scores.push(LuckExactScore {
            final_mean: final_mean.into(),
            final_note_mean: final_note_mean.into(),
            final_rank_mean: final_rank_mean.into(),
            final_support: final_support.into(),
            ranges: lane.ranges,
            scored_notes,
            wide_notes,
            widest_note,
            peak_paths: replay.peak_paths,
            queries: schedule.queries,
            probability_peak_states: laws[order].peak_states,
            probability_transitions: laws[order].transitions,
            profile,
            notes,
        });
    }
    Ok(scores)
}
