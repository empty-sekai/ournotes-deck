//! Candidate competition for supported Normal/SlideBegin/SlideEnd raw updaters.
//! FTLiveSimulator.JudgementNearPositionNote 0x6a5add4 and GetPriorityNote 0x6a5f284.
//! Input is ScreenFrame, not a judgement stream. NoteLine must still supply note state.
use super::raw::{
    AutoInput, Derivation, InputState, NoteResult, RawNote, is_judgement, music_time, priority_note, select_input,
};
use super::raw_input::{ScreenFrame, can_async};
#[derive(Clone, Debug)]
pub struct Candidate {
    pub note_id: i32,
    pub operate_type: i32,
    pub note: RawNote,
    /// Broad phase bounds, already expanded using simulator laneCount/24 offset.
    pub min_lane: f32,
    pub max_lane: f32,
    /// Current effective broad phase window; result lookup still uses note.units (base).
    pub before_max_ms: i32,
    pub after_max_ms: i32,
}
/// Candidate order must be native frame NearNoteIds order. Unit groups execute by
/// unit index: one synchronous winner in slot zero, then asynchronous notes in order.
/// This does not implement chart loading, NoteLine state evolution or autoplay timing.
pub fn judge_candidates(
    candidates: &mut [Candidate],
    frame: &ScreenFrame,
    auto: AutoInput,
) -> Result<Vec<(i32, NoteResult)>, &'static str> {
    if auto.enabled {
        return Err("automatic timing scheduler not implemented");
    }
    // Reject unsupported updaters before producing partial results.
    for c in candidates.iter() {
        let expected = match c.operate_type {
            1 => Derivation::Normal,
            20 => Derivation::SlideBegin,
            22 => Derivation::SlideEnd,
            _ => return Err("candidate updater not implemented"),
        };
        if c.note.kind != expected {
            return Err("operate type and updater mismatch");
        }
    }
    let mut groups: Vec<CandidateGroup> = vec![(None, Vec::new()); frame.units.len()];
    let mut selected = vec![None; candidates.len()];
    for (i, c) in candidates.iter().enumerate() {
        let chart_minus_frame = c.note.chart_ms.wrapping_sub(frame.music_time_ms);
        if chart_minus_frame > c.before_max_ms {
            continue;
        }
        if chart_minus_frame < c.after_max_ms.wrapping_neg() && c.note.state == super::raw::NoteState::Done {
            continue;
        }
        let Some((u, _)) = select_input(c.operate_type, c.min_lane, c.max_lane, &frame.units)? else { continue };
        let input = frame.units[u].0;
        let time = music_time(input.time_ms, frame.real_time_ms, frame.music_time_ms);
        let diff = c.note.chart_ms.wrapping_sub(time);
        if diff > c.before_max_ms || diff < c.after_max_ms.wrapping_neg() {
            continue;
        }
        if !is_judgement(
            c.note.kind,
            input.state,
            c.note.state,
            c.note.frame_diff_ms,
            c.note.slide_before_ms,
            c.note.slide_after_ms,
        ) {
            continue;
        }
        let group = groups.get_mut(input.index).ok_or("input unit index outside frame capacity")?;
        selected[i] = Some(u);
        if can_async(c.operate_type) {
            group.1.push(i);
        } else {
            group.0 = Some(priority_note(group.0, (i, c.note.frame_diff_ms)));
        }
    }
    let mut out = Vec::new();
    for (unit, (winner, asynchronous)) in groups.into_iter().enumerate() {
        if frame.units[unit].0.state == InputState::None {
            continue;
        }
        for i in winner.map(|x| x.0).into_iter().chain(asynchronous) {
            let input = frame.units[selected[i].unwrap()].0;
            let c = &mut candidates[i];
            if let Some(result) = c.note.judge_input(input, frame.real_time_ms, frame.music_time_ms, auto)? {
                out.push((c.note_id, result));
            }
        }
    }
    Ok(out)
}
/// Native unit grouping: stable strict competition, sync slot before async,
/// and no loser fallback when minimum filters the winning result.
#[derive(Clone, Debug)]
pub struct CandidateGroups {
    groups: Vec<CandidateGroup>,
}
/// One input unit's synchronous winner (note, frame diff) and its asynchronous notes in order.
type CandidateGroup = (Option<(usize, i32)>, Vec<usize>);
impl CandidateGroups {
    pub fn new(capacity: usize) -> Self {
        Self { groups: vec![(None, Vec::new()); capacity] }
    }
    pub fn insert(&mut self, unit: usize, note: usize, diff: i32, asynchronous: bool) -> Result<(), &'static str> {
        let group = self.groups.get_mut(unit).ok_or("candidate group exceeds simulator capacity")?;
        if asynchronous {
            group.1.push(note);
        } else {
            group.0 = Some(priority_note(group.0, (note, diff)));
        }
        Ok(())
    }
    pub fn ordered(self) -> impl Iterator<Item = (usize, Vec<usize>)> {
        self.groups
            .into_iter()
            .enumerate()
            .map(|(i, (winner, tail))| (i, winner.map(|w| w.0).into_iter().chain(tail).collect()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::raw::{NoteState, TimingUnit, Vec2};
    use crate::live::raw_input::{ScreenProvider, Touch};
    fn candidate(id: i32, time: i32, operate_type: i32) -> Candidate {
        Candidate {
            note_id: id,
            operate_type,
            note: RawNote {
                chart_ms: time,
                judgement_type: 0,
                kind: if operate_type == 22 { Derivation::SlideEnd } else { Derivation::Normal },
                state: NoteState::Before,
                frame_diff_ms: 1000 - time,
                slide_before_ms: 100,
                slide_after_ms: 100,
                units: vec![TimingUnit::new(6, 10, 10), TimingUnit::new(5, 30, 30), TimingUnit::new(1, 100, 100)],
            },
            min_lane: -0.5,
            max_lane: 1.5,
            before_max_ms: 100,
            after_max_ms: 100,
        }
    }
    #[test]
    fn screen_to_judgement_competition_and_stable_tie() {
        let mut provider = ScreenProvider::new(
            vec![Vec2 { x: 100., y: 50. }, Vec2 { x: 200., y: 50. }],
            Vec2 { x: 10., y: 10. },
            5.,
            0.,
        )
        .unwrap();
        let frame = provider
            .update(
                1000,
                5.,
                1. / 60.,
                &[Touch {
                    finger: 0,
                    touch_id: 1,
                    phase: 1,
                    position: Vec2 { x: 100., y: 50. },
                    time_seconds: 5.,
                    blocked_at_start: false,
                }],
            )
            .unwrap();
        let mut notes = vec![candidate(1, 990, 1), candidate(2, 1010, 1), candidate(3, 1000, 1)];
        let out = judge_candidates(&mut notes, &frame, AutoInput::default()).unwrap();
        assert_eq!(out.iter().map(|v| v.0).collect::<Vec<_>>(), vec![3]);
        assert_eq!(out[0].1.judgement, 6);
        let mut notes = vec![candidate(1, 990, 1), candidate(2, 1010, 1)];
        let out = judge_candidates(&mut notes, &frame, AutoInput::default()).unwrap();
        assert_eq!(out[0].0, 1);
    }
    #[test]
    fn minimum_filter_does_not_fall_back_to_other_candidate() {
        let mut provider =
            ScreenProvider::new(vec![Vec2 { x: 100., y: 50. }], Vec2 { x: 10., y: 10. }, 5., 0.).unwrap();
        let frame = provider
            .update(
                1000,
                5.,
                1. / 60.,
                &[Touch {
                    finger: 0,
                    touch_id: 1,
                    phase: 1,
                    position: Vec2 { x: 100., y: 50. },
                    time_seconds: 5.,
                    blocked_at_start: false,
                }],
            )
            .unwrap();
        let mut notes = vec![candidate(1, 1015, 1), candidate(2, 1020, 1)];
        assert!(
            judge_candidates(&mut notes, &frame, AutoInput { minimum: 6, ..Default::default() }).unwrap().is_empty()
        );
    }
}
