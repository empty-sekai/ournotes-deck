//! ScreenTouchInputProvider.Update port. Device geometry and blocker outcome are
//! explicit inputs (not a guessed camera projection/UI hit-test).
//! VA 0x60afa8c, constructor 0x60ae688, input ctor 0x6a603a0, Clear 0x6a60768.
use super::raw::{FlickUnit, InputState, InputUnit, Vec2, flick_state, input_state, judgement_lane};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Touch {
    pub finger: i32,
    pub touch_id: i32,
    pub phase: i32,
    pub position: Vec2,
    pub time_seconds: f64,
    /// Only evaluated at Began or changed touch ID; later frame values are ignored.
    pub blocked_at_start: bool,
}
#[derive(Clone, Debug)]
pub struct ScreenFrame {
    pub real_time_ms: i32,
    pub music_time_ms: i32,
    pub units: Vec<(InputUnit, FlickUnit)>,
    pub finger_indices: Vec<i32>,
}
#[derive(Clone, Debug)]
pub struct ScreenProvider {
    pub positions: Vec<Vec2>,
    pub tolerance: Vec2,
    pub threshold_px: f32,
    pub offset_ms: f64,
    previous: [Vec2; 5],
    blocked: [bool; 5],
    touch_ids: [i32; 5],
}
/// FCVTZS finite int32 domain is enforced instead of pretending out-of-range Rust
/// saturating casts are the client's conversion. Ordinary game times are in range.
fn trunc_i32(value: f64) -> Result<i32, &'static str> {
    let t = value.trunc();
    if !t.is_finite() || t < i32::MIN as f64 || t > i32::MAX as f64 {
        return Err("time conversion outside int32 domain");
    }
    Ok(t as i32)
}
/// Verified frintm at 0x60afed4 followed by fadd offset and fcvtzs at 0x60afee0.
pub fn touch_time_ms(phase: i32, time_seconds: f64, real_frame_ms: i32, offset_ms: f64) -> Result<i32, &'static str> {
    if phase == 5 {
        Ok(real_frame_ms.wrapping_add(trunc_i32(offset_ms)?))
    } else {
        trunc_i32((time_seconds * 1000.0).floor() + offset_ms)
    }
}
impl ScreenProvider {
    pub fn new(positions: Vec<Vec2>, tolerance: Vec2, threshold_px: f32, offset_ms: f64) -> Result<Self, &'static str> {
        judgement_lane(Vec2::default(), &positions, tolerance)?;
        if !threshold_px.is_finite() || !offset_ms.is_finite() {
            return Err("nonfinite input configuration");
        }
        Ok(Self {
            positions,
            tolerance,
            threshold_px,
            offset_ms,
            previous: [Vec2::default(); 5],
            blocked: [false; 5],
            touch_ids: [-1; 5],
        })
    }
    /// Active touches remain in caller-provided order. Finger >=5 is ignored; blocked
    /// fingers update previous position but do not consume a unit slot. Unit index is
    /// compacted and is NOT the finger index. Capacity equals laneCount, not touchMax.
    pub fn update(
        &mut self,
        music_time_ms: i32,
        realtime_seconds: f64,
        dt: f32,
        touches: &[Touch],
    ) -> Result<ScreenFrame, &'static str> {
        let real_time_ms = trunc_i32((realtime_seconds * 1000.0).floor())?;
        let empty_flick = FlickUnit { active: false, lane: -1.0, delta: Vec2::default() };
        let mut frame = ScreenFrame {
            real_time_ms,
            music_time_ms,
            units: (0..self.positions.len())
                .map(|index| (InputUnit { index, state: InputState::None, lane: -1.0, time_ms: -1 }, empty_flick))
                .collect(),
            finger_indices: vec![-1; self.positions.len()],
        };
        let mut next = 0;
        for t in touches {
            if t.finger >= 5 {
                continue;
            }
            if t.finger < 0 {
                return Err("negative finger index");
            }
            if next >= frame.units.len() {
                break;
            }
            let finger = t.finger as usize;
            if t.phase == 1 || self.touch_ids[finger] != t.touch_id {
                self.touch_ids[finger] = t.touch_id;
                self.blocked[finger] = t.blocked_at_start;
            }
            if !self.blocked[finger] {
                let lane = judgement_lane(t.position, &self.positions, self.tolerance)?;
                let time_ms = touch_time_ms(t.phase, t.time_seconds, real_time_ms, self.offset_ms)?;
                let mut flick = empty_flick;
                if t.phase == 2 || t.phase == 3 {
                    let (active, delta) = flick_state(t.position, self.previous[finger], dt, self.threshold_px);
                    if active {
                        flick = FlickUnit {
                            active,
                            delta,
                            lane: judgement_lane(self.previous[finger], &self.positions, self.tolerance)?,
                        };
                    }
                }
                frame.units[next] = (InputUnit { index: next, state: input_state(t.phase), lane, time_ms }, flick);
                frame.finger_indices[next] = t.finger;
                next += 1;
            }
            self.previous[finger] = t.position;
        }
        Ok(frame)
    }
}
/// Original CanAsyncJudgement 0x6a38df0, including slide ends and trace begins.
pub fn can_async(operate_type: i32) -> bool {
    matches!(operate_type, 21 | 22 | 60..=63 | 104 | 105 | 120)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider() -> ScreenProvider {
        ScreenProvider::new(vec![Vec2 { x: 100., y: 50. }, Vec2 { x: 200., y: 50. }], Vec2 { x: 20., y: 10. }, 5., 0.75)
            .unwrap()
    }
    fn touch(finger: i32, id: i32, phase: i32, x: f32, blocked: bool) -> Touch {
        Touch {
            finger,
            touch_id: id,
            phase,
            position: Vec2 { x, y: 50. },
            time_seconds: 1.0009,
            blocked_at_start: blocked,
        }
    }
    #[test]
    fn timestamp_floor_then_offset_and_stationary() {
        assert_eq!(touch_time_ms(2, -0.0001, 0, 0.), Ok(-1));
        assert_eq!(touch_time_ms(2, 1.0009, 2000, 0.75), Ok(1000));
        assert_eq!(touch_time_ms(5, 1.0009, 2000, 0.75), Ok(2000));
        assert_eq!(touch_time_ms(2, 1.0009, 2000, -0.75), Ok(999));
        assert_eq!(touch_time_ms(5, 1.0009, 2000, -0.75), Ok(2000));
    }
    #[test]
    fn ui_block_and_touch_id_reuse() {
        let mut p = provider();
        let f = p.update(1000, 5., 1. / 60., &[touch(0, 7, 1, 100., true), touch(1, 8, 1, 200., false)]).unwrap();
        assert_eq!(f.finger_indices, vec![1, -1]);
        assert_eq!(f.units[0].0.state, InputState::Enter);
        let f = p.update(1016, 5.016, 1. / 60., &[touch(0, 7, 2, 110., false)]).unwrap();
        assert_eq!(f.finger_indices, vec![-1, -1]);
        let f = p.update(1032, 5.032, 1. / 60., &[touch(0, 9, 2, 115., false)]).unwrap();
        assert_eq!(f.finger_indices, vec![0, -1]);
        assert!(!f.units[0].1.active); // previous was updated even while UI blocked
    }
    #[test]
    fn previous_position_flick_and_canceled_no_flick() {
        let mut p = provider();
        p.update(0, 0., 1. / 60., &[touch(0, 1, 1, 100., false)]).unwrap();
        let f = p.update(16, 0.016, 1. / 60., &[touch(0, 1, 2, 120., false)]).unwrap();
        assert!(f.units[0].1.active);
        assert_eq!(f.units[0].1.lane, 0.);
        let f = p.update(32, 0.032, 1. / 60., &[touch(0, 1, 4, 150., false)]).unwrap();
        assert_eq!(f.units[0].0.state, InputState::Exit);
        assert!(!f.units[0].1.active);
    }
}
