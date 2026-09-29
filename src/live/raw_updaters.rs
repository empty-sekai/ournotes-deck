//! Unpatched 1.0.1 updater dispatch. Scheduling/conversion belong to callers.
//! Candidates ARE mutating: losers keep Flick/Trace reservations.
//! Evidence: factory 0x6a5e1b8, Flick 0x6a66db4/0x6a669b8, Trace 0x6a67edc.
use super::raw::{
    self, AutoInput, Derivation, FlickUnit, InputState, InputUnit, JudgementInfo, NoteResult, NoteState, TimingUnit,
    Vec2,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdaterKind {
    Normal,
    SlideBegin,
    SlideEnd,
    Flick,
    Trace,
    Connection,
    ConnectionTrace,
    Combo,
    ComboSkip,
    Hidden,
    GuideBegin,
    GuideEnd,
}
impl UpdaterKind {
    /// Factory throws for 0, 123 and unknown values. No separate HoldUpdater exists.
    pub fn from_operate_type(t: i32) -> Result<Self, &'static str> {
        Ok(match t {
            1 | 101 => Self::Normal,
            20 => Self::SlideBegin,
            22 => Self::SlideEnd,
            40..=42 | 102 => Self::Flick,
            60..=62 | 104 | 105 => Self::Trace,
            21 => Self::Connection,
            63 => Self::ConnectionTrace,
            120 => Self::Combo,
            121 => Self::ComboSkip,
            80 | 82 | 122 => Self::Hidden,
            100 => Self::GuideBegin,
            103 => Self::GuideEnd,
            _ => return Err("native NoteUpdaterFactory rejects operate type"),
        })
    }
    pub fn is_auto_judgement_note(self) -> bool {
        !matches!(self, Self::Hidden | Self::ComboSkip)
    }
    fn trace(self) -> bool {
        matches!(self, Self::Trace | Self::Connection | Self::ConnectionTrace | Self::Combo)
    }
}
/// Actual native values from scheduler; no inferred window defaults.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct UpdaterContext {
    pub chart_ms: i32,
    pub judgement_type: i32,
    pub state: NoteState,
    pub frame_diff_ms: i32,
    pub input_music_ms: i32,
    pub frame_music_ms: i32,
    /// UpdaterBase construction-time cached Before/After (0x58/0x5c).
    pub before_ms: i32,
    pub after_ms: i32,
    /// Retained Perfect unit current After getter (judgement 5).
    pub perfect_after_ms: i32,
    pub lane_min: f32,
    pub lane_max: f32,
    pub lane_count: i32,
    pub area_offset: f32,
    /// AutoInput.IsEasyFlick independent of auto.enabled.
    pub easy_flick: bool,
}
impl UpdaterContext {
    fn target(self, lane: f32) -> bool {
        raw::target_lane(lane, self.lane_min, self.lane_max, self.lane_count, self.area_offset)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedJudgement {
    pub time_ms: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdaterJudgement {
    pub result: NoteResult,
    /// No direction comparison in these unpatched methods.
    pub direction_mismatch: bool,
    pub is_easy_flick: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawUpdater {
    pub kind: UpdaterKind,
    pub judgement_time_ms: i32,
    pub trace_reserved: bool,
    pub near_flick: bool,
    pub near_flick_time_ms: i32,
    pub latest_flick_vector: Vec2,
    pub latest_flick_lane: f32,
    pub easy_flicked: bool,
    pub easy_flick_time_ms: i32,
}
impl RawUpdater {
    pub fn new(operate_type: i32) -> Result<Self, &'static str> {
        Ok(Self {
            kind: UpdaterKind::from_operate_type(operate_type)?,
            judgement_time_ms: 0,
            trace_reserved: false,
            near_flick: false,
            near_flick_time_ms: 0,
            latest_flick_vector: Vec2::default(),
            latest_flick_lane: -1.,
            easy_flicked: false,
            easy_flick_time_ms: 0,
        })
    }
    /// IsJudgement 0x6a68348 plus virtual derivation. Call ONCE per candidate,
    /// including losers. Do not re-run when committing the winner.
    pub fn prepare_candidate(
        &mut self,
        input: InputUnit,
        flick: Option<FlickUnit>,
        c: &UpdaterContext,
        auto: AutoInput,
    ) -> Option<DerivedJudgement> {
        let (accepted, time) = if auto.enabled {
            (
                self.kind.is_auto_judgement_note()
                    && matches!(c.state, NoteState::Before | NoteState::Just | NoteState::After),
                c.input_music_ms,
            )
        } else if self.kind == UpdaterKind::Flick {
            self.flick_candidate(input, flick, c)
        } else if self.kind.trace() {
            self.trace_candidate(input, c)
        } else {
            match self.kind {
                UpdaterKind::Normal | UpdaterKind::SlideBegin => (
                    raw::is_judgement(
                        Derivation::Normal,
                        input.state,
                        c.state,
                        c.frame_diff_ms,
                        c.before_ms,
                        c.after_ms,
                    ),
                    c.input_music_ms,
                ),
                UpdaterKind::SlideEnd => (
                    raw::is_judgement(
                        Derivation::SlideEnd,
                        input.state,
                        c.state,
                        c.frame_diff_ms,
                        c.before_ms,
                        c.after_ms,
                    ),
                    if input.state == InputState::Exit { c.input_music_ms } else { c.chart_ms },
                ),
                _ => (false, -1),
            }
        };
        self.judgement_time_ms = time;
        accepted.then_some(DerivedJudgement { time_ms: time })
    }
    fn trace_candidate(&mut self, input: InputUnit, c: &UpdaterContext) -> (bool, i32) {
        if c.state == NoteState::Before {
            if input.state != InputState::None {
                self.trace_reserved = true;
            }
            return if input.state == InputState::Exit { (true, c.input_music_ms) } else { (false, -1) };
        }
        if c.state == NoteState::First {
            self.trace_reserved = false;
        }
        let time = if input.state == InputState::Exit { c.input_music_ms } else { c.chart_ms };
        if self.trace_reserved
            && raw::is_judgement(
                Derivation::ReservedTracePredicate,
                input.state,
                c.state,
                c.frame_diff_ms,
                c.before_ms,
                c.after_ms,
            )
        {
            return (true, time);
        }
        let kind = match self.kind {
            UpdaterKind::Combo => Derivation::Combo,
            UpdaterKind::Connection => Derivation::Connection,
            _ => Derivation::TracePredicate,
        };
        (raw::is_judgement(kind, input.state, c.state, c.frame_diff_ms, c.before_ms, c.after_ms), time)
    }
    fn flick_candidate(&mut self, input: InputUnit, flick: Option<FlickUnit>, c: &UpdaterContext) -> (bool, i32) {
        if c.state == NoteState::Done {
            return (false, -1);
        }
        if c.state == NoteState::First {
            self.near_flick = false;
            self.latest_flick_vector = Vec2::default();
            self.latest_flick_lane = -1.;
            self.easy_flicked = false;
        }
        let diff = c.input_music_ms.wrapping_sub(c.chart_ms);
        let in_easy_window = diff.wrapping_neg() <= c.before_ms && diff <= c.perfect_after_ms;
        if in_easy_window && c.easy_flick {
            self.easy_flicked = input.state != InputState::None && c.target(input.lane);
            self.easy_flick_time_ms = c.input_music_ms;
        }
        // IsNearPositionFlickNote only checks active + not Done, NOT lane/finger.
        if let Some(f) = flick.filter(|f| f.active) {
            self.near_flick = true;
            self.near_flick_time_ms = c.input_music_ms;
            self.latest_flick_vector = f.delta;
            self.latest_flick_lane = f.lane;
        }
        if self.near_flick
            && c.target(self.latest_flick_lane)
            && (input.state == InputState::Exit
                || (input.state == InputState::Press
                    && matches!(c.state, NoteState::Just | NoteState::After | NoteState::Last)))
        {
            return (true, self.near_flick_time_ms);
        }
        if self.easy_flicked
            && ((input.state == InputState::Exit && in_easy_window)
                || (matches!(input.state, InputState::Enter | InputState::Press) && !in_easy_window))
        {
            return (true, self.easy_flick_time_ms);
        }
        (false, -1)
    }
    /// After generic Update; no current input required. Generic Last may already
    /// have transitioned to Done before this callback (0x6a66600).
    pub fn reserved_flick_candidate(&mut self, c: &UpdaterContext, auto: AutoInput) -> Option<DerivedJudgement> {
        if self.kind == UpdaterKind::Flick
            && self.near_flick
            && !auto.enabled
            && matches!(c.state, NoteState::Just | NoteState::After | NoteState::Last)
            && c.target(self.latest_flick_lane)
        {
            self.judgement_time_ms = self.near_flick_time_ms;
            Some(DerivedJudgement { time_ms: self.near_flick_time_ms })
        } else {
            None
        }
    }
    /// Uses BASE timing units (not effective candidate windows). No conversion,
    /// Assist or scheduler-state mutation. Minimum applies before derived edits.
    pub fn finish_candidate(
        &self,
        candidate: DerivedJudgement,
        c: &UpdaterContext,
        units: &[TimingUnit],
        auto: AutoInput,
    ) -> Option<UpdaterJudgement> {
        let mut result =
            raw::finish_judgement(raw::note_judgement(c.chart_ms, candidate.time_ms, c.judgement_type, units), auto)?;
        if matches!(self.kind, UpdaterKind::GuideBegin | UpdaterKind::GuideEnd) {
            result.origin = 7;
            result.judgement = 7;
        }
        Some(UpdaterJudgement {
            result,
            direction_mismatch: false,
            is_easy_flick: self.kind == UpdaterKind::Flick && !auto.enabled && !self.near_flick && self.easy_flicked,
        })
    }
    /// CURRENT operate type can be mutated to 122. Last bypasses auto/minimum/edit.
    /// Scheduler applies the native timing gate then transitions to Done.
    pub fn last_judgement(
        &self,
        current_operate_type: i32,
        judgement_type: i32,
        frame_music_ms: i32,
    ) -> Result<UpdaterJudgement, &'static str> {
        let judgement = match self.kind {
            UpdaterKind::Hidden | UpdaterKind::ComboSkip | UpdaterKind::GuideBegin | UpdaterKind::GuideEnd => 7,
            UpdaterKind::Combo => match current_operate_type {
                60 | 120 => 1,
                122 => 7,
                _ => return Err("native ComboUpdater Last rejects changed kind"),
            },
            UpdaterKind::Connection => match current_operate_type {
                21 => 1,
                122 => 7,
                _ => return Err("native ConnectionUpdater Last rejects changed kind"),
            },
            UpdaterKind::ConnectionTrace => match current_operate_type {
                63 => 1,
                122 => 7,
                _ => return Err("native ConnectionTraceUpdater Last rejects changed kind"),
            },
            _ => 1,
        };
        Ok(Self::forced_result(JudgementInfo {
            judgement_type,
            judgement,
            time_ms: frame_music_ms,
            timing: 5,
            diff_ms: i32::MAX,
        }))
    }
    /// Line owns force-condition selection. SetLastJudgement takes existing info,
    /// bypassing UpdateJudgement auto/minimum/derived edits.
    pub fn forced_result(info: JudgementInfo) -> UpdaterJudgement {
        UpdaterJudgement {
            result: raw::finish_judgement(info, AutoInput::default()).expect("no minimum"),
            direction_mismatch: false,
            is_easy_flick: false,
        }
    }
}
