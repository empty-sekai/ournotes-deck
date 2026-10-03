//! Raw-input judgement primitives, separate from `full`'s judged stream.
//! Callers supply client timing units and geometry; no default windows are invented.
//! Stateful NoteLine and the frame scheduler live in `raw_scheduler`, cached Flick/Trace derivations in `raw_updaters`.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum InputState {
    None = 0,
    Enter = 1,
    Press = 2,
    Exit = 3,
}
/// EnhancedTouch phase, not the legacy TouchPhase enum.
pub fn input_state(phase: i32) -> InputState {
    match phase {
        1 => InputState::Enter,
        2 | 5 => InputState::Press,
        3 | 4 => InputState::Exit,
        _ => InputState::None,
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// Lane coordinate 0..count-1, -1 outside tolerances. Invalid geometry is rejected.
pub fn judgement_lane(p: Vec2, positions: &[Vec2], tolerance: Vec2) -> Result<f32, &'static str> {
    if positions.is_empty()
        || !p.x.is_finite()
        || !p.y.is_finite()
        || !tolerance.x.is_finite()
        || !tolerance.y.is_finite()
        || positions.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
        || positions.windows(2).any(|p| p[0].x >= p[1].x)
    {
        return Err("invalid judgement geometry");
    }
    if tolerance.y < (p.y - positions[0].y).abs()
        || tolerance.x < positions[0].x - p.x
        || tolerance.x < p.x - positions[positions.len() - 1].x
    {
        return Ok(-1.0);
    }
    for (i, v) in positions.iter().enumerate() {
        if p.x - v.x <= 0.0 {
            return Ok(if i == 0 {
                0.0
            } else {
                (p.x - positions[i - 1].x) / (v.x - positions[i - 1].x) + (i - 1) as f32
            });
        }
    }
    Ok((positions.len() - 1) as f32)
}
/// IsTargetLane uses note lane-count; simulator broad phase uses simulator lane-count.
pub fn target_lane(lane: f32, min: f32, max: f32, count: i32, offset: f32) -> bool {
    let margin = offset * (count as f32 / 24.0);
    (min - margin) - 0.5 <= lane && lane <= (margin + max) + 0.5
}
/// Original displacement, not normalized velocity. Equality is not a flick.
pub fn flick_state(current: Vec2, previous: Vec2, dt: f32, threshold_px: f32) -> (bool, Vec2) {
    let dx = current.x - previous.x;
    let dy = current.y - previous.y;
    let mut scale = dt / 0.016666668_f32;
    if scale <= 0.0 {
        scale = 1.0;
    }
    let vx = dx / scale;
    let vy = dy / scale;
    let speed_sq = vx * vx + vy * vy;
    let threshold_sq = threshold_px * threshold_px;
    (threshold_sq < speed_sq, if speed_sq <= threshold_sq { Vec2::default() } else { Vec2 { x: dx, y: dy } })
}
pub fn music_time(input_ms: i32, real_frame_ms: i32, music_frame_ms: i32) -> i32 {
    input_ms.wrapping_sub(real_frame_ms).wrapping_add(music_frame_ms)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingUnit {
    pub judgement: i32,
    pub enabled: bool,
    pub base_before_ms: i32,
    pub base_after_ms: i32,
    pub before_ms: i32,
    pub after_ms: i32,
}
impl TimingUnit {
    pub fn new(judgement: i32, before: i32, after: i32) -> Self {
        Self {
            judgement,
            enabled: true,
            base_before_ms: before,
            base_after_ms: after,
            before_ms: before,
            after_ms: after,
        }
    }
    pub fn before(&self) -> i32 {
        if self.enabled { self.before_ms } else { -1 }
    }
    pub fn after(&self) -> i32 {
        if self.enabled { self.after_ms } else { -1 }
    }
    pub fn contains(&self, diff: i32) -> bool {
        diff.wrapping_neg() <= self.before() && diff <= self.after()
    }
    pub fn add(&mut self, before: i32, after: i32) {
        self.before_ms = self.before_ms.wrapping_add(before);
        self.after_ms = self.after_ms.wrapping_add(after);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgementInfo {
    pub judgement_type: i32,
    pub judgement: i32,
    pub time_ms: i32,
    /// 0: |diff|<2, 1: early, 2: late, 4: no matching unit. Not a Just flag.
    pub timing: i32,
    pub diff_ms: i32,
}
/// Units in client priority order, not numerical judgement order.
pub fn note_judgement(chart_ms: i32, input_music_ms: i32, judgement_type: i32, units: &[TimingUnit]) -> JudgementInfo {
    let diff = input_music_ms.wrapping_sub(chart_ms);
    let (judgement, timing) = match units.iter().find(|u| u.contains(diff)) {
        None => (1, 4),
        Some(u) => (
            u.judgement,
            if diff.wrapping_abs() < 2 {
                0
            } else if diff > 0 {
                2
            } else {
                1
            },
        ),
    };
    JudgementInfo { judgement_type, judgement, time_ms: input_music_ms, timing, diff_ms: diff }
}
/// 4004 adds roundEven(BASE*factor), capped by remaining Miss headroom.
/// Supply verified RelaxTargetJudgements; this API deliberately has no guessed default.
/// Returned deltas support exact removal without recomputing against changed windows.
pub fn enhance_percent(units: &mut [TimingUnit], targets: &[i32], factor: f32) -> Vec<(usize, i32, i32)> {
    let (mb, ma) = units.iter().find(|u| u.judgement == 1).map(|u| (u.before(), u.after())).unwrap_or((0, 0));
    let mut applied = Vec::new();
    for &target in targets {
        if let Some((i, u)) = units.iter_mut().enumerate().find(|(_, u)| u.judgement == target) {
            let b = (u.base_before_ms as f32 * factor).round_ties_even() as i32;
            let a = (u.base_after_ms as f32 * factor).round_ties_even() as i32;
            let b = if b.wrapping_add(u.before()) <= mb { b } else { mb.wrapping_sub(u.before()) }.max(0);
            let a = if a.wrapping_add(u.after()) <= ma { a } else { ma.wrapping_sub(u.after()) }.max(0);
            if b > 0 || a > 0 {
                u.add(b, a);
                applied.push((i, b, a));
            }
        }
    }
    applied
}
/// 13001 base_factor is supplied by LiveSettings, not current Just window width.
pub fn just_expansion(value: i64, base_factor: f32) -> i32 {
    ((value as f32 / 10000.0_f32) * base_factor).round_ties_even() as i32
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum NoteState {
    Wait = 0,
    First = 1,
    Before = 2,
    Just = 3,
    After = 4,
    Last = 5,
    Done = 6,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Derivation {
    Normal,
    SlideBegin,
    SlideEnd,
    Combo,
    Connection,
    TracePredicate,
    ReservedTracePredicate,
}
/// Trace predicates are not replacements for the stateful trace reservation updater.
pub fn is_judgement(kind: Derivation, input: InputState, state: NoteState, diff: i32, before: i32, after: i32) -> bool {
    if state == NoteState::Done {
        return false;
    }
    match kind {
        Derivation::Normal | Derivation::SlideBegin => input == InputState::Enter,
        Derivation::SlideEnd => {
            (diff <= 0 && before.wrapping_neg() <= diff && input == InputState::Exit)
                || (diff >= 0 && diff <= after && matches!(input, InputState::Press | InputState::Exit))
        }
        Derivation::Combo | Derivation::Connection | Derivation::TracePredicate => match state {
            NoteState::Just | NoteState::After => input != InputState::None,
            NoteState::Before => matches!(input, InputState::Enter | InputState::Exit),
            _ => false,
        },
        Derivation::ReservedTracePredicate => {
            matches!(state, NoteState::Just | NoteState::After) && input != InputState::None
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputUnit {
    pub index: usize,
    pub state: InputState,
    pub lane: f32,
    pub time_ms: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlickUnit {
    pub active: bool,
    pub lane: f32,
    pub delta: Vec2,
}
/// GetUseSimulateInputUnitPair/GetHigherPriorityInputUnit. Returns slice index and
/// presence of selected flick object. Non-flick reduction drops flick after a second match.
pub fn select_input(
    operate_type: i32,
    min: f32,
    max: f32,
    inputs: &[(InputUnit, FlickUnit)],
) -> Result<Option<(usize, bool)>, &'static str> {
    let flick = matches!(operate_type, 40..=42 | 102);
    let priority = match operate_type {
        1 | 20 | 101 => 1,
        21 | 22 | 60..=63 | 104 | 105 | 120 => 2,
        40..=42 | 102 => 3,
        80 | 82 | 100 | 103 | 121..=123 => 0,
        _ => return Err("unknown operate type"),
    };
    let mut chosen: Option<(usize, bool)> = None;
    for (i, (u, f)) in inputs.iter().enumerate() {
        if !(min <= u.lane && u.lane <= max || flick && f.active && min <= f.lane && f.lane <= max) {
            continue;
        }
        chosen = Some(match chosen {
            None => (i, true),
            Some((j, _)) if priority == 3 => {
                if inputs[j].1.active {
                    (j, true)
                } else if f.active {
                    (i, true)
                } else {
                    (j, false)
                }
            }
            Some((j, _)) => (if u.state as i32 == priority && priority != 0 { i } else { j }, false),
        });
    }
    Ok(chosen)
}
/// Stable winner: strict smaller |OffsetJudgementTimeMs| replaces, ties retain first.
pub fn priority_note(first: Option<(usize, i32)>, second: (usize, i32)) -> (usize, i32) {
    match first {
        Some(x) if second.1.wrapping_abs() >= x.1.wrapping_abs() => x,
        _ => second,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoInput {
    pub enabled: bool,
    pub use_timing: bool,
    pub judgement: i32,
    pub minimum: i32,
}
impl Default for AutoInput {
    fn default() -> Self {
        Self { enabled: false, use_timing: true, judgement: 5, minimum: -1 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteResult {
    pub origin: i32,
    pub judgement: i32,
    pub judgement_type: i32,
    pub timing: i32,
    pub time_ms: i32,
    pub origin_diff_ms: i32,
    pub diff_ms: i32,
}
/// UpdaterBase auto override/minimum filter before derived editing.
pub fn finish_judgement(info: JudgementInfo, auto: AutoInput) -> Option<NoteResult> {
    let timing_based = !auto.enabled || auto.use_timing;
    let j = if timing_based { info.judgement } else { auto.judgement };
    if timing_based && auto.minimum != -1 && j != 0 && j < auto.minimum {
        return None;
    }
    let diff = if timing_based { info.diff_ms } else { i32::MAX };
    Some(NoteResult {
        origin: j,
        judgement: j,
        judgement_type: info.judgement_type,
        timing: info.timing,
        time_ms: info.time_ms,
        origin_diff_ms: diff,
        diff_ms: diff,
    })
}
/// Native ordering; lastJust/history read converted judgement, not origin.
pub fn convert_result(result: &mut NoteResult, judgement: impl FnOnce(i32) -> i32, diff: impl FnOnce(i32, i32) -> i32) {
    result.judgement = judgement(result.judgement);
    result.diff_ms = diff(result.judgement, result.diff_ms);
}
/// Raw input endpoint for normal/slide stateless derivation, NOT a whole-chart replay.
/// Scheduler state/frame offset must be supplied separately until NoteLine is ported.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawNote {
    pub chart_ms: i32,
    pub judgement_type: i32,
    pub kind: Derivation,
    pub state: NoteState,
    pub frame_diff_ms: i32,
    pub slide_before_ms: i32,
    pub slide_after_ms: i32,
    pub units: Vec<TimingUnit>,
}
impl RawNote {
    pub fn judge_input(
        &mut self,
        input: InputUnit,
        real_frame_ms: i32,
        music_frame_ms: i32,
        auto: AutoInput,
    ) -> Result<Option<NoteResult>, &'static str> {
        if !matches!(self.kind, Derivation::Normal | Derivation::SlideBegin | Derivation::SlideEnd) {
            return Err("stateful updater derivation not implemented");
        }
        let accepted = if auto.enabled {
            matches!(self.state, NoteState::Before | NoteState::Just | NoteState::After)
        } else {
            is_judgement(
                self.kind,
                input.state,
                self.state,
                self.frame_diff_ms,
                self.slide_before_ms,
                self.slide_after_ms,
            )
        };
        if !accepted {
            return Ok(None);
        }
        let time = if !auto.enabled && self.kind == Derivation::SlideEnd && input.state != InputState::Exit {
            self.chart_ms
        } else {
            music_time(input.time_ms, real_frame_ms, music_frame_ms)
        };
        let result = finish_judgement(note_judgement(self.chart_ms, time, self.judgement_type, &self.units), auto);
        if result.is_some() {
            self.state = NoteState::Done;
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn units() -> Vec<TimingUnit> {
        vec![
            TimingUnit::new(6, 10, 10),
            TimingUnit::new(5, 30, 30),
            TimingUnit::new(4, 60, 60),
            TimingUnit::new(1, 100, 100),
        ]
    }
    #[test]
    fn boundaries_and_priority() {
        let u = units();
        for (d, j) in [
            (-101, 1),
            (-100, 1),
            (-61, 1),
            (-60, 4),
            (-31, 4),
            (-30, 5),
            (-11, 5),
            (-10, 6),
            (0, 6),
            (10, 6),
            (11, 5),
            (101, 1),
        ] {
            assert_eq!(note_judgement(1000, 1000 + d, 0, &u).judgement, j);
        }
        assert_eq!(note_judgement(0, 1, 0, &[TimingUnit::new(5, 30, 30)]).timing, 0);
        assert_eq!(note_judgement(0, 1, 0, &[TimingUnit::new(5, 30, 30)]).judgement, 5);
        assert_eq!(note_judgement(0, 101, 0, &u).timing, 4);
        let mut disabled = TimingUnit::new(6, 10, 10);
        disabled.enabled = false;
        assert_eq!(note_judgement(0, 0, 0, &[disabled]).judgement, 1);
    }
    #[test]
    fn phases_geometry_and_flick() {
        assert_eq!((0..=6).map(input_state).map(|x| x as i32).collect::<Vec<_>>(), vec![0, 1, 2, 3, 3, 2, 0]);
        let p = [Vec2 { x: 100., y: 50. }, Vec2 { x: 200., y: 50. }];
        let t = Vec2 { x: 20., y: 5. };
        assert_eq!(judgement_lane(Vec2 { x: 150., y: 55. }, &p, t), Ok(0.5));
        assert_eq!(judgement_lane(Vec2 { x: 150., y: 55.1 }, &p, t), Ok(-1.));
        assert_eq!(judgement_lane(Vec2 { x: 80., y: 50. }, &p, t), Ok(0.));
        assert_eq!(flick_state(Vec2 { x: 10., y: 0. }, Vec2::default(), 1. / 60., 10.), (false, Vec2::default()));
        assert!(flick_state(Vec2 { x: 10.1, y: 0. }, Vec2::default(), 1. / 60., 10.).0);
        assert!(!flick_state(Vec2 { x: 10.1, y: 0. }, Vec2::default(), 1. / 30., 10.).0);
    }
    #[test]
    fn increment_not_current_multiplier() {
        let mut u = vec![TimingUnit::new(5, 5, 7), TimingUnit::new(1, 10, 10)];
        assert_eq!(enhance_percent(&mut u, &[5], 0.5), vec![(0, 2, 3)]);
        assert_eq!((u[0].before_ms, u[0].after_ms), (7, 10));
        assert_eq!(enhance_percent(&mut u, &[5], 0.5), vec![(0, 2, 0)]);
        assert_eq!(just_expansion(5000, 5.), 2);
        assert_eq!(just_expansion(5000, 7.), 4);
    }
    #[test]
    fn raw_replay_and_conversion_order() {
        let mut n = RawNote {
            chart_ms: 1000,
            judgement_type: 0,
            kind: Derivation::Normal,
            state: NoteState::Before,
            frame_diff_ms: -20,
            slide_before_ms: 30,
            slide_after_ms: 30,
            units: units(),
        };
        let mut input = InputUnit { index: 0, state: InputState::Press, lane: 0., time_ms: 4980 };
        assert!(n.judge_input(input, 5000, 1000, AutoInput::default()).unwrap().is_none());
        input.state = InputState::Enter;
        let mut result = n.judge_input(input, 5000, 1000, AutoInput::default()).unwrap().unwrap();
        assert_eq!((result.origin, result.diff_ms), (5, -20));
        convert_result(&mut result, |_| 6, |j, d| if j == 6 { 0 } else { d });
        assert_eq!((result.origin, result.judgement, result.origin_diff_ms, result.diff_ms), (5, 6, -20, 0));
        assert!(n.judge_input(input, 5000, 1000, AutoInput::default()).unwrap().is_none());
    }
    #[test]
    fn slide_release_and_hold_have_different_time() {
        let mut n = RawNote {
            chart_ms: 1000,
            judgement_type: 0,
            kind: Derivation::SlideEnd,
            state: NoteState::After,
            frame_diff_ms: 20,
            slide_before_ms: 30,
            slide_after_ms: 30,
            units: units(),
        };
        let i = InputUnit { index: 0, state: InputState::Press, lane: 0., time_ms: 5020 };
        assert_eq!(n.judge_input(i, 5000, 1000, AutoInput::default()).unwrap().unwrap().diff_ms, 0);
        n.state = NoteState::After;
        assert_eq!(
            n.judge_input(InputUnit { state: InputState::Exit, ..i }, 5000, 1000, AutoInput::default())
                .unwrap()
                .unwrap()
                .diff_ms,
            20
        );
    }
    #[test]
    fn automatic_minimum_and_equal_priority() {
        let info = note_judgement(0, 50, 0, &units());
        assert!(finish_judgement(info, AutoInput { minimum: 5, ..Default::default() }).is_none());
        assert_eq!(
            finish_judgement(info, AutoInput { enabled: true, use_timing: false, judgement: 3, minimum: 5 })
                .unwrap()
                .diff_ms,
            i32::MAX
        );
        assert_eq!(priority_note(Some((0, -10)), (1, 10)), (0, -10));
        assert_eq!(music_time(i32::MIN, 1, 1), i32::MIN);
    }
}
