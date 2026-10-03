//! The frames of a play where each Gekisou range changes state.

use serde::Serialize;

use super::gekisou::{S_COMPLETE, S_DELAY, S_END, S_FINISH, S_STANDBY, S_START};
use super::{LiveModel, LivePlay};
use crate::error::Error;

/// Frame indices (into the play's frames) where a Gekisou range changed its state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeFrames {
    /// Wait -> Standby (`None` when the range never waited: it started in the first frame).
    pub standby: Option<usize>,
    /// The fever's start: Start (the next frame is Playing).
    pub start: usize,
    /// The fever's end: End.
    pub end: usize,
    /// The end delay (the longest judgement window after the end, counted with the frame delta times): Delay.
    pub delay: usize,
    /// 500 ms later: Complete (the range's scores are taken and its rank bonus added).
    pub complete: usize,
    /// The next frame: Finish (the luck rush ends).
    pub finish: usize,
}

impl LiveModel {
    /// Plays every frame of `play` (delta times `dt`, the play's seed) and records the frames where each Gekisou
    /// range changed state.
    pub fn record_range_frames(&mut self, play: &LivePlay, dt: &[f32]) -> Result<Vec<RangeFrames>, Error> {
        if dt.len() != play.frames.len() {
            return Err(Error::Input("one delta time per frame".into()));
        }
        self.random.set_seed(play.base_seed);
        let n = self.gk.as_ref().map_or(0, |g| g.ctrl.ranges.len());
        let mut seen = vec![[None::<usize>; 9]; n];
        for (i, (f, &d)) in play.frames.iter().zip(dt).enumerate() {
            self.frame_timed(f.time_ms, &f.judged, d)?;
            let g = self.gk.as_ref().ok_or_else(|| Error::Input("range frames without Gekisou".into()))?;
            for &idx in &g.ctrl.state_updates {
                let s = g.ctrl.states[idx].state as usize;
                if let Some(slot) = seen[idx].get_mut(s) {
                    slot.get_or_insert(i);
                }
            }
        }
        seen.iter()
            .enumerate()
            .map(|(i, s)| {
                let need = |state: u8| {
                    s[state as usize]
                        .ok_or_else(|| Error::Game(format!("Gekisou range {i} never reached state {state}")))
                };
                Ok(RangeFrames {
                    standby: s[S_STANDBY as usize],
                    start: need(S_START)?,
                    end: need(S_END)?,
                    delay: need(S_DELAY)?,
                    complete: need(S_COMPLETE)?,
                    finish: need(S_FINISH)?,
                })
            })
            .collect()
    }
}
