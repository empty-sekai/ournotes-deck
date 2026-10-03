//! Stateful, converter-free frame scheduler: updater base and note-line state machines.
//! One state dispatch per frame, NOT a window classifier or catch-up loop.
use super::raw::{self, AutoInput, InputState, InputUnit, JudgementInfo, NoteResult, NoteState, TimingUnit};
use super::raw_candidates::CandidateGroups;
use super::raw_input::{ScreenFrame, can_async};
use super::raw_updaters::{DerivedJudgement, RawUpdater, UpdaterContext, UpdaterJudgement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Chart/settings data, never frame state. Vector order is native updater order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduledNote {
    pub id: i32,
    pub operate_type: i32,
    pub chart_ms: i32,
    pub judgement_type: i32,
    pub lane_min: f32,
    pub lane_max: f32,
    pub lane_count: i32,
    /// Construction-time cached timing maxima, not skill-mutated effective bounds.
    pub cached_before_ms: i32,
    pub cached_after_ms: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduledLine {
    pub id: i32,
    pub start_note_id: i32,
    pub start_ms: i32,
    pub end_ms: i32,
    pub lane_min: i32,
    pub lane_max: i32,
    /// Native INoteLineState.Notes order; only current-frame judged IDs affect missed.
    pub note_ids: Vec<i32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchedulerSettings {
    pub before_playing_ms: i32,
    pub input_timing_ms: i32,
    pub simulator_lane_count: i32,
    pub area_offset: f32,
    pub music_end_ms: i32,
    pub input_capacity: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchedulerTiming {
    pub base: Vec<TimingUnit>,
    pub effective_before_ms: i32,
    pub effective_after_ms: i32,
}
pub type SchedulerTimings = BTreeMap<i32, SchedulerTiming>;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct FrameOptions {
    pub auto: AutoInput,
    pub easy_flick: bool,
    /// Native GetAutoTimingDiffMs caches the first sampled override per note.
    pub auto_timing_override_ms: Option<i32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScheduledJudgement {
    pub note_id: i32,
    pub judgement: UpdaterJudgement,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteFrameState {
    pub note_id: i32,
    pub state: NoteState,
    pub frame_diff_ms: i32,
    pub progress: f32,
    /// Converted callback feedback, distinct from raw output.
    pub result: Option<NoteResult>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineFrameState {
    pub line_id: i32,
    /// 0 Wait, 1 Playing, 2 Done (not NoteState values).
    pub state: i32,
    pub enabled: bool,
    pub missed: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RawFrameOutput {
    pub music_time_ms: i32,
    pub near_note_ids: Vec<i32>,
    pub changed_note_ids: Vec<i32>,
    pub judged_note_ids: Vec<i32>,
    pub updated_line_ids: Vec<i32>,
    pub changed_line_ids: Vec<i32>,
    pub enabled_line_ids: Vec<i32>,
    pub judgements: Vec<RawScheduledJudgement>,
    pub notes: Vec<NoteFrameState>,
    pub lines: Vec<LineFrameState>,
    pub all_notes_done: bool,
    /// Native Update retains frame data after end+2000 (no reset or dispatch).
    pub beyond_end: bool,
}
#[derive(Clone, Debug)]
pub struct RawScheduler {
    settings: SchedulerSettings,
    notes: Vec<ScheduledNote>,
    lines: Vec<ScheduledLine>,
    states: Vec<NoteFrameState>,
    line_states: Vec<LineFrameState>,
    updaters: Vec<RawUpdater>,
    indices: BTreeMap<i32, usize>,
    auto_times: BTreeMap<i32, i32>,
    last_frame: RawFrameOutput,
    pub timings: SchedulerTimings,
}
fn add_id(ids: &mut Vec<i32>, id: i32) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}
/// Mathf.Approximately used by Wait; integer equality differs at large times.
pub fn approximately_time(a: i32, b: i32) -> bool {
    let (a, b) = (a as f32, b as f32);
    (a - b).abs() < (1e-6_f32 * a.abs().max(b.abs())).max(f32::from_bits(1) * 8.)
}
pub fn judgement_progress(now: i32, chart: i32, offset: i32, before: i32) -> f32 {
    let p = now.wrapping_sub(chart.wrapping_add(offset)).wrapping_add(before) as f32 / before as f32;
    if p <= 0. { 0. } else { p }
}
pub fn is_last_timing(diff: i32, after: i32, progress: f32) -> bool {
    after < diff && progress >= 1.1_f32
}
/// NoteLine UpdateJudgement / TryGetEnableLine. None preserves both previous flags.
/// Holding (not Exit) overrides a same-frame Miss; lane bounds have no margin.
#[allow(clippy::too_many_arguments)]
pub fn line_judgement_decision(
    now: i32,
    start: i32,
    start_grade: i32,
    auto: bool,
    recent_grades: &[i32],
    inputs: &[(InputUnit, raw::FlickUnit)],
    lane_min: i32,
    lane_max: i32,
) -> Option<(bool, bool)> {
    if if now.wrapping_sub(100) < start { (3..=6).contains(&start_grade) } else { auto } {
        return Some((true, false));
    }
    if inputs.iter().any(|(u, _)| {
        matches!(u.state, InputState::Enter | InputState::Press)
            && lane_min as f32 <= u.lane
            && u.lane <= lane_max as f32
    }) {
        return Some((true, false));
    }
    recent_grades.iter().any(|j| matches!(j, 1 | 2)).then_some((false, true))
}
/// Clamps an auto timing sample to the non-Miss window (base parameter, not effective).
pub fn clamp_auto_timing(sample: i32, units: &[TimingUnit]) -> i32 {
    let before = units.iter().filter(|u| u.judgement != 1).map(TimingUnit::before).max().unwrap_or(0).max(0);
    let after = units.iter().filter(|u| u.judgement != 1).map(TimingUnit::after).max().unwrap_or(0).max(0);
    sample.clamp(-before, after)
}
impl RawScheduler {
    pub fn new(
        notes: Vec<ScheduledNote>,
        lines: Vec<ScheduledLine>,
        settings: SchedulerSettings,
        timings: SchedulerTimings,
    ) -> Result<Self, &'static str> {
        if settings.before_playing_ms <= 0 || settings.simulator_lane_count <= 0 || !settings.area_offset.is_finite() {
            return Err("invalid scheduler settings");
        }
        let mut indices = BTreeMap::new();
        let mut updaters = Vec::new();
        for (i, n) in notes.iter().enumerate() {
            if n.id < 0 || indices.insert(n.id, i).is_some() {
                return Err("negative or duplicate note ID");
            }
            if !n.lane_min.is_finite() || !n.lane_max.is_finite() || n.lane_min > n.lane_max || n.lane_count <= 0 {
                return Err("invalid chart lane");
            }
            if !timings.contains_key(&n.judgement_type) {
                return Err("missing judgement timing type");
            }
            updaters.push(RawUpdater::new(n.operate_type)?);
        }
        let mut line_ids = Vec::new();
        for l in &lines {
            if l.end_ms < l.start_ms || l.lane_min > l.lane_max || line_ids.contains(&l.id) {
                return Err("invalid or duplicate note line");
            }
            line_ids.push(l.id);
            if !indices.contains_key(&l.start_note_id) || l.note_ids.iter().any(|id| !indices.contains_key(id)) {
                return Err("line references missing note");
            }
        }
        let states = notes
            .iter()
            .map(|n| NoteFrameState {
                note_id: n.id,
                state: NoteState::Wait,
                frame_diff_ms: 0,
                progress: 0.,
                result: None,
            })
            .collect();
        let line_states =
            lines.iter().map(|l| LineFrameState { line_id: l.id, state: 0, enabled: false, missed: false }).collect();
        Ok(Self {
            settings,
            notes,
            lines,
            states,
            line_states,
            updaters,
            indices,
            auto_times: BTreeMap::new(),
            last_frame: RawFrameOutput::default(),
            timings,
        })
    }
    pub fn note_states(&self) -> &[NoteFrameState] {
        &self.states
    }
    pub fn line_states(&self) -> &[LineFrameState] {
        &self.line_states
    }
    pub fn updater(&self, id: i32) -> Option<&RawUpdater> {
        self.indices.get(&id).map(|&i| &self.updaters[i])
    }
    fn state(&mut self, i: usize, state: NoteState, out: &mut RawFrameOutput) {
        if self.states[i].state != state {
            self.states[i].state = state;
            add_id(&mut out.near_note_ids, self.notes[i].id);
            add_id(&mut out.changed_note_ids, self.notes[i].id);
        }
    }
    fn context(
        &self,
        i: usize,
        input_ms: i32,
        now: i32,
        options: FrameOptions,
    ) -> Result<UpdaterContext, &'static str> {
        let n = &self.notes[i];
        let s = &self.states[i];
        let timing = self.timings.get(&n.judgement_type).ok_or("missing current timing type")?;
        Ok(UpdaterContext {
            chart_ms: n.chart_ms,
            judgement_type: n.judgement_type,
            state: s.state,
            frame_diff_ms: s.frame_diff_ms,
            input_music_ms: input_ms,
            frame_music_ms: now,
            before_ms: n.cached_before_ms,
            after_ms: n.cached_after_ms,
            perfect_after_ms: timing.base.iter().find(|u| u.judgement == 5).map_or(-1, TimingUnit::after),
            lane_min: n.lane_min,
            lane_max: n.lane_max,
            lane_count: n.lane_count,
            area_offset: self.settings.area_offset,
            easy_flick: options.easy_flick,
        })
    }
    fn emit<F>(
        &mut self,
        i: usize,
        judgement: UpdaterJudgement,
        out: &mut RawFrameOutput,
        callback: &mut F,
    ) -> Result<(), &'static str>
    where
        F: FnMut(RawScheduledJudgement) -> Result<NoteResult, &'static str>,
    {
        let event = RawScheduledJudgement { note_id: self.notes[i].id, judgement };
        add_id(&mut out.near_note_ids, event.note_id);
        let converted = callback(event)?;
        self.states[i].result = Some(converted);
        add_id(&mut out.judged_note_ids, event.note_id);
        out.judgements.push(event);
        self.state(i, NoteState::Done, out);
        Ok(())
    }
    fn force_inner<F>(
        &mut self,
        i: usize,
        judgement: i32,
        now: i32,
        out: &mut RawFrameOutput,
        callback: &mut F,
    ) -> Result<(), &'static str>
    where
        F: FnMut(RawScheduledJudgement) -> Result<NoteResult, &'static str>,
    {
        self.state(i, NoteState::Done, out);
        add_id(&mut out.near_note_ids, self.notes[i].id);
        add_id(&mut out.changed_note_ids, self.notes[i].id);
        self.emit(
            i,
            RawUpdater::forced_result(JudgementInfo {
                judgement_type: 0,
                judgement,
                time_ms: now,
                timing: 6,
                diff_ms: i32::MAX,
            }),
            out,
            callback,
        )
    }
    /// Explicit ForceJudgement may rejudge Done; it is not inferred from touch input.
    pub fn force_with<F>(
        &mut self,
        id: i32,
        judgement: i32,
        now: i32,
        mut callback: F,
    ) -> Result<RawFrameOutput, &'static str>
    where
        F: FnMut(RawScheduledJudgement) -> Result<NoteResult, &'static str>,
    {
        let i = *self.indices.get(&id).ok_or("unknown force note")?;
        let mut out = RawFrameOutput { music_time_ms: now, ..Default::default() };
        self.force_inner(i, judgement, now, &mut out, &mut callback)?;
        self.snapshot(&mut out);
        Ok(out)
    }
    fn snapshot(&self, out: &mut RawFrameOutput) {
        out.notes = self.states.clone();
        out.lines = self.line_states.clone();
        out.all_notes_done = !self.states.is_empty() && self.states.iter().all(|s| s.state == NoteState::Done);
    }
    pub fn step(&mut self, frame: &ScreenFrame, options: FrameOptions) -> Result<RawFrameOutput, &'static str> {
        self.step_with(frame, options, |e| Ok(e.judgement.result))
    }
    /// Callback runs converter -> DiffConverter and returns the converted result.
    /// Executor window callbacks run AFTER this FT frame, not here.
    /// Errors stop the frame; callbacks already executed are not rolled back.
    pub fn step_with<F>(
        &mut self,
        frame: &ScreenFrame,
        options: FrameOptions,
        mut callback: F,
    ) -> Result<RawFrameOutput, &'static str>
    where
        F: FnMut(RawScheduledJudgement) -> Result<NoteResult, &'static str>,
    {
        let now = frame.music_time_ms;
        if now > self.settings.music_end_ms.wrapping_add(2000) {
            let mut out = self.last_frame.clone();
            out.music_time_ms = now;
            out.beyond_end = true;
            return Ok(out);
        }
        if frame.units.iter().any(|(u, _)| u.index >= self.settings.input_capacity) {
            return Err("input unit exceeds simulator capacity");
        }
        let mut out = RawFrameOutput { music_time_ms: now, ..Default::default() };
        // Priority zero: lines, including end forcing the start note.
        for j in 0..self.lines.len() {
            if self.line_states[j].state == 2 {
                continue;
            }
            let l = self.lines[j].clone();
            if l.start_ms >= now.wrapping_add(self.settings.before_playing_ms) {
                if self.line_states[j].state != 0 {
                    self.line_states[j].state = 0;
                    add_id(&mut out.updated_line_ids, l.id);
                    add_id(&mut out.changed_line_ids, l.id);
                }
                continue;
            }
            if l.end_ms < now {
                self.line_states[j].state = 2;
                add_id(&mut out.updated_line_ids, l.id);
                add_id(&mut out.changed_line_ids, l.id);
                let i = self.indices[&l.start_note_id];
                let grade = self.states[i].result.map_or(-1, |r| r.judgement);
                if grade != -1 && grade != 0 {
                    self.state(i, NoteState::Done, &mut out);
                    add_id(&mut out.near_note_ids, l.start_note_id);
                    add_id(&mut out.changed_note_ids, l.start_note_id);
                } else {
                    let op = self.notes[i].operate_type;
                    let is_judgement = !matches!(op, 0 | 80 | 82 | 100 | 103 | 121..=123);
                    self.force_inner(i, if is_judgement { 1 } else { 7 }, now, &mut out, &mut callback)?;
                }
            } else {
                add_id(&mut out.updated_line_ids, l.id);
                if self.line_states[j].state == 0 {
                    self.line_states[j].state = 1;
                    add_id(&mut out.changed_line_ids, l.id);
                }
            }
        }
        // Priority one: one state dispatch, then Flick self-judgement.
        for i in 0..self.notes.len() {
            let n = self.notes[i].clone();
            if n.chart_ms >= now.wrapping_add(self.settings.before_playing_ms) {
                self.state(i, NoteState::Wait, &mut out);
            } else {
                let diff = now.wrapping_sub(n.chart_ms).wrapping_sub(self.settings.input_timing_ms);
                let progress = judgement_progress(now, n.chart_ms, 0, self.settings.before_playing_ms);
                let jp =
                    judgement_progress(now, n.chart_ms, self.settings.input_timing_ms, self.settings.before_playing_ms);
                let after =
                    self.timings.get(&n.judgement_type).ok_or("missing current timing type")?.effective_after_ms;
                let old = self.states[i].state;
                if old == NoteState::Last {
                    let r = self.updaters[i].last_judgement(n.operate_type, n.judgement_type, now)?;
                    self.emit(i, r, &mut out, &mut callback)?;
                } else if old != NoteState::Done {
                    if old == NoteState::Wait && is_last_timing(diff, after, jp) {
                        self.state(i, NoteState::Last, &mut out);
                    } else {
                        add_id(&mut out.near_note_ids, n.id);
                        self.states[i].progress = progress;
                        self.states[i].frame_diff_ms = diff;
                        let next = match old {
                            NoteState::Wait => {
                                let target = n.chart_ms.wrapping_add(self.settings.input_timing_ms);
                                if target < now {
                                    NoteState::After
                                } else if approximately_time(target, now) {
                                    NoteState::Just
                                } else {
                                    NoteState::First
                                }
                            }
                            NoteState::First => NoteState::Before,
                            NoteState::Before if jp >= 1. => NoteState::Just,
                            NoteState::Just => NoteState::After,
                            NoteState::After if is_last_timing(diff, after, jp) => NoteState::Last,
                            _ => old,
                        };
                        self.state(i, next, &mut out);
                    }
                }
            }
            let c = self.context(i, now, now, options)?;
            if let Some(d) = self.updaters[i].reserved_flick_candidate(&c, options.auto) {
                let units = &self.timings[&n.judgement_type].base;
                if let Some(r) = self.updaters[i].finish_candidate(d, &c, units, options.auto) {
                    self.emit(i, r, &mut out, &mut callback)?;
                }
            }
        }
        // Prepare ALL candidates (and cache side effects) before committing winners.
        let near = out.near_note_ids.clone();
        let mut groups = CandidateGroups::new(self.settings.input_capacity);
        let mut prepared: Vec<Option<DerivedJudgement>> = vec![None; self.notes.len()];
        let mut auto_index = 0;
        for id in near {
            let i = self.indices[&id];
            let n = &self.notes[i];
            let t = &self.timings[&n.judgement_type];
            let chart_minus_frame = n.chart_ms.wrapping_sub(now);
            if chart_minus_frame > t.effective_before_ms
                || (chart_minus_frame < t.effective_after_ms.wrapping_neg() && self.states[i].state == NoteState::Done)
            {
                continue;
            }
            let margin = self.settings.area_offset * (self.settings.simulator_lane_count as f32 / 24.);
            let selection =
                raw::select_input(n.operate_type, n.lane_min - margin - 0.5, n.lane_max + margin + 0.5, &frame.units)?;
            if selection.is_none() && !options.auto.enabled {
                continue;
            }
            let input = selection.map(|(u, _)| frame.units[u].0).unwrap_or(InputUnit {
                index: 0,
                state: InputState::None,
                lane: -1.,
                time_ms: frame.real_time_ms,
            });
            let flick = selection.and_then(|(u, has)| has.then_some(frame.units[u].1));
            let d = if let Some(sample) = options.auto_timing_override_ms {
                *self.auto_times.entry(id).or_insert_with(|| clamp_auto_timing(sample, &t.base))
            } else {
                0
            };
            let input_ms = if options.auto.enabled {
                let time = n.chart_ms.wrapping_add(d);
                if now < time.wrapping_add(self.settings.input_timing_ms) {
                    continue;
                }
                time
            } else {
                raw::music_time(input.time_ms, frame.real_time_ms, now)
            };
            let cd = n.chart_ms.wrapping_sub(input_ms);
            if cd > t.effective_before_ms || cd < t.effective_after_ms.wrapping_neg() {
                continue;
            }
            let c = self.context(i, input_ms, now, options)?;
            if let Some(d) = self.updaters[i].prepare_candidate(input, flick, &c, options.auto) {
                let group = if options.auto.enabled { auto_index } else { input.index };
                auto_index += 1;
                groups.insert(group, i, self.states[i].frame_diff_ms, can_async(self.notes[i].operate_type))?;
                prepared[i] = Some(d);
            }
        }
        for (group, items) in groups.ordered() {
            if group >= frame.units.len() {
                continue;
            }
            if !options.auto.enabled && frame.units[group].0.state == InputState::None {
                continue;
            }
            for i in items {
                let d = prepared[i].ok_or("missing prepared candidate")?;
                let c = self.context(i, d.time_ms, now, options)?;
                if let Some(r) = self.updaters[i].finish_candidate(
                    d,
                    &c,
                    &self.timings[&self.notes[i].judgement_type].base,
                    options.auto,
                ) {
                    self.emit(i, r, &mut out, &mut callback)?;
                }
            }
        }
        // Line judgement reads CONVERTED results and current-frame judged IDs.
        for j in 0..self.lines.len() {
            if self.line_states[j].state != 1 {
                continue;
            }
            let l = &self.lines[j];
            let start_grade = self.states[self.indices[&l.start_note_id]].result.map_or(-1, |r| r.judgement);
            let recent_grades: Vec<i32> = l
                .note_ids
                .iter()
                .filter(|id| out.judged_note_ids.contains(id))
                .filter_map(|id| self.states[self.indices[id]].result.map(|r| r.judgement))
                .collect();
            let decision = line_judgement_decision(
                now,
                l.start_ms,
                start_grade,
                options.auto.enabled,
                &recent_grades,
                &frame.units,
                l.lane_min,
                l.lane_max,
            );
            if let Some((enabled, missed)) = decision {
                let s = &mut self.line_states[j];
                if s.enabled != enabled {
                    s.enabled = enabled;
                    add_id(&mut out.enabled_line_ids, l.id);
                    add_id(&mut out.updated_line_ids, l.id);
                }
                if s.missed != missed {
                    add_id(&mut out.updated_line_ids, l.id);
                    s.missed = missed;
                }
            }
        }
        self.snapshot(&mut out);
        self.last_frame = out.clone();
        Ok(out)
    }
}
