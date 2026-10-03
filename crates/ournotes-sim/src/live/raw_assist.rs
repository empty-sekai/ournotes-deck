//! Assist executor semantics, not a score-only multiplier: level update, converted-grade hook and offset
//! provider.
use super::raw::{NoteResult, TimingUnit, Vec2};
use super::raw_windows::{TimingSet, resolve_assist};
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Debug)]
pub struct AssistLevelAdjuster {
    pub level: i32,
    pub point: i32,
    pub perfect_continue: i32,
    pub gauge_max: i32,
    pub level_max: i32,
    pub level_down_count: i32,
    pub judgement_points: BTreeMap<i32, i32>,
    pub fixed_level: Option<i32>,
}
impl AssistLevelAdjuster {
    pub fn update(&mut self, notes: &[(i32, i32)]) -> Result<(i32, i32, i32), &'static str> {
        if let Some(level) = self.fixed_level {
            self.level = level;
            self.point = 0;
            self.perfect_continue = 0;
            return Ok((level, 0, 0));
        }
        let initial = self.level;
        let mut level = self.level;
        let mut point = self.point;
        let mut perfect = self.perfect_continue;
        let bound = |v: i32| if v < 0 { 0 } else { v.min(self.level_max) };
        for &(judgement, operate) in notes {
            if (1..=4).contains(&judgement) {
                point = point
                    .wrapping_add(*self.judgement_points.get(&judgement).ok_or("missing assist judgement points")?);
                perfect = 0;
            } else if (5..=6).contains(&judgement) && !matches!(operate, 21 | 60..=63 | 104 | 105 | 120) {
                perfect = perfect.wrapping_add(1);
                if self.level_down_count <= perfect {
                    level = bound(level.wrapping_sub(1));
                    perfect = 0;
                }
            }
        }
        if point < 1 {
            point = 0;
        } else {
            let quotient = if self.gauge_max == 0 { 0 } else { point.wrapping_div(self.gauge_max) };
            point = point.wrapping_sub(quotient.wrapping_mul(self.gauge_max));
            level = bound(level.wrapping_add(quotient));
        }
        if level != initial {
            point = 0;
        }
        self.level = level;
        self.point = point;
        self.perfect_continue = perfect;
        Ok((level, point, perfect))
    }
}
#[derive(Clone, Debug)]
pub struct AssistExecutor {
    pub adjuster: AssistLevelAdjuster,
    pub current_level: i32,
    pub current_point: i32,
    pub play_max_level: i32,
    /// One ten-entry offset dictionary per level (omitted keys are zeros).
    pub offset_levels: Vec<[Vec2; 10]>,
    pub small_note_width: f32,
    pub large_note_width: f32,
    pub assisted_notes: HashSet<i32>,
    offsets: [Vec2; 10],
    offsets_enabled: bool,
    previous_offset_level: i32,
}
impl AssistExecutor {
    pub fn new(
        adjuster: AssistLevelAdjuster,
        offset_levels: Vec<[Vec2; 10]>,
        small_note_width: f32,
        large_note_width: f32,
    ) -> Self {
        Self {
            adjuster,
            current_level: 0,
            current_point: 0,
            play_max_level: 0,
            offset_levels,
            small_note_width,
            large_note_width,
            assisted_notes: HashSet::new(),
            offsets: [Vec2::default(); 10],
            offsets_enabled: false,
            previous_offset_level: -1,
        }
    }
    /// LiveExecutor.OnUpdate runs after FT results but before UpdateCurrentFrameParameters.
    pub fn on_update(&mut self, notes: &[(i32, i32)], timing_level_count: usize) -> Result<(), &'static str> {
        let (level, point, _) = self.adjuster.update(notes)?;
        self.current_level = level;
        self.current_point = point;
        self.play_max_level = self.play_max_level.max(level);
        if level < 0 || level as usize >= timing_level_count {
            return Err("assist level outside timing controllers");
        }
        if level != self.previous_offset_level {
            if let Some(offsets) = self.offset_levels.get(level as usize) {
                self.offsets = *offsets;
                self.offsets_enabled = true;
            }
            self.previous_offset_level = level;
        }
        Ok(())
    }
    /// This hook is registered at the converter tail; caller skips it if an earlier
    /// skill converter already changed the grade. Only grades1..5 are considered.
    pub fn convert(
        &mut self,
        note_id: i32,
        result: &NoteResult,
        is_easy_flick: bool,
        levels: &[Vec<TimingSet>],
    ) -> Result<i32, &'static str> {
        let original = result.judgement;
        if !(-1..=7).contains(&original) {
            return Err("invalid judgement in Assist converter");
        }
        if !(1..=5).contains(&original) || self.current_level == 0 {
            return Ok(original);
        }
        if result.judgement_type == 0 {
            return Ok(original);
        }
        if !matches!(result.judgement_type, 1 | 2 | 5 | 10 | 11 | 12 | 15 | 21 | 22) {
            return Err("invalid Assist judgement type");
        }
        let find = |level: usize| -> Result<&[TimingUnit], &'static str> {
            Ok(&levels
                .get(level)
                .ok_or("missing Assist level")?
                .iter()
                .find(|s| s.judgement_type == result.judgement_type)
                .ok_or("missing Assist timing type")?
                .units)
        };
        let (grade, assisted) =
            resolve_assist(original, result.diff_ms, is_easy_flick, find(0)?, find(self.current_level as usize)?)?;
        if assisted {
            self.assisted_notes.insert(note_id);
        }
        Ok(grade)
    }
    pub fn offset(&self, offset_type: usize, note_width: f32) -> Result<Vec2, &'static str> {
        if !self.offsets_enabled {
            return Ok(Vec2::default());
        }
        if offset_type != 1 {
            return self.offsets.get(offset_type).copied().ok_or("invalid Assist offset type");
        }
        let t = if self.small_note_width == self.large_note_width {
            0.0
        } else {
            {
                let value = (note_width - self.small_note_width) / (self.large_note_width - self.small_note_width);
                if value >= 0.0 { if value <= 1.0 { value } else { 1.0 } } else { 0.0 }
            }
        };
        Ok(Vec2 { x: self.offsets[6].x + (self.offsets[7].x - self.offsets[6].x) * t, y: self.offsets[6].y })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn adjuster() -> AssistLevelAdjuster {
        AssistLevelAdjuster {
            level: 1,
            point: 0,
            perfect_continue: 0,
            gauge_max: 10,
            level_max: 3,
            level_down_count: 2,
            judgement_points: BTreeMap::from([(1, 10), (2, 5), (3, 2), (4, 1)]),
            fixed_level: None,
        }
    }
    #[test]
    fn ignore_types_and_final_level_change_reset() {
        let mut a = adjuster();
        assert_eq!(a.update(&[(5, 21), (6, 60)]), Ok((1, 0, 0)));
        assert_eq!(a.update(&[(5, 22), (6, 1)]), Ok((0, 0, 0))); // slide end counts, connection does not
        assert_eq!(a.update(&[(2, 1), (2, 1), (4, 1)]), Ok((1, 0, 0)));
        assert_eq!(a.update(&[(3, 1)]), Ok((1, 2, 0)));
    }
    #[test]
    fn fixed_level_and_offset_interpolation() {
        let mut a = adjuster();
        a.fixed_level = Some(2);
        let mut offsets = [Vec2::default(); 10];
        offsets[6] = Vec2 { x: 1., y: 4. };
        offsets[7] = Vec2 { x: 3., y: 9. };
        let mut e = AssistExecutor::new(a, vec![offsets; 3], 1., 3.);
        e.on_update(&[], 3).unwrap();
        assert_eq!(e.current_level, 2);
        assert_eq!(e.offset(1, 2.), Ok(Vec2 { x: 2., y: 4. }));
    }
}
