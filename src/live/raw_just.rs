//! Effect 13001: BPJustJudgementTimingExpansionPercent, unpatched 1.0.1.
//! VA 0x5605410 (Update), 0x5605cc0 (OnJudgement), 0x5605b8c (remaining).
use super::raw::{NoteResult, just_expansion};
use super::raw_windows::WindowController;
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Debug)]
struct Limit {
    owner: i64,
    count: i32,
    max: i32,
}
#[derive(Clone, Debug)]
pub struct JustWindowApplier {
    pub base_expansion_ms: i32,
    pub original_just_before_ms: i32,
    handles: BTreeMap<i64, i32>,
    limited: Vec<Limit>,
    seen: HashSet<i32>,
    finished: BTreeMap<i64, i32>,
    remaining: i32,
}
impl JustWindowApplier {
    pub fn new(base_expansion_ms: i32, original_just_before_ms: i32) -> Self {
        Self {
            base_expansion_ms,
            original_just_before_ms,
            handles: BTreeMap::new(),
            limited: Vec::new(),
            seen: HashSet::new(),
            finished: BTreeMap::new(),
            remaining: 0,
        }
    }
    fn update_remaining(&mut self) {
        self.remaining = self.limited.iter().fold(0i32, |v, p| v.wrapping_sub(p.count).wrapping_add(p.max));
    }
    pub fn remaining(&self) -> i32 {
        self.remaining
    }
    /// Every Update clears note dedup before reporting LimitFinishedHandler.
    pub fn begin_update(&mut self, owner: i64) -> Option<i32> {
        self.seen.clear();
        self.finished.remove(&owner)
    }
    /// Unlike 4000..4003, limit<=0 is STILL registered, not treated as unlimited.
    pub fn execute(
        &mut self,
        owner: i64,
        value: i64,
        limit: i32,
        controller: &mut WindowController,
    ) -> Result<(), &'static str> {
        if self.handles.contains_key(&owner) {
            return Err("duplicate Just window owner");
        }
        let ms = just_expansion(value, self.base_expansion_ms as f32);
        let handle = controller.enhance(6, ms, ms);
        self.limited.push(Limit { owner, count: 0, max: limit });
        self.handles.insert(owner, handle);
        self.update_remaining();
        Ok(())
    }
    pub fn end(&mut self, owner: i64, controller: &mut WindowController) -> Result<(), &'static str> {
        let Some(handle) = self.handles.remove(&owner) else {
            return Ok(());
        };
        controller.disable(handle)?;
        if let Some(i) = self.limited.iter().position(|p| p.owner == owner) {
            self.limited.remove(i);
        }
        self.update_remaining();
        Ok(())
    }
    /// The slot7 diff is converted JudgementDiffTimeMs, not OriginJudgementDiffTimeMs.
    pub fn on_judgement(
        &mut self,
        note_id: i32,
        result: &NoteResult,
        controller: &mut WindowController,
    ) -> Result<(), &'static str> {
        if result.judgement != 6 {
            return Ok(());
        }
        if self.original_just_before_ms > 0 && result.diff_ms.wrapping_abs() <= self.original_just_before_ms {
            return Ok(());
        }
        if !self.seen.insert(note_id) {
            return Ok(());
        }
        for i in (0..self.limited.len()).rev() {
            self.limited[i].count = self.limited[i].count.wrapping_add(1);
            if self.limited[i].max <= self.limited[i].count {
                let owner = self.limited.remove(i).owner;
                let handle = self.handles.remove(&owner).ok_or("missing Just window owner")?;
                controller.disable(handle)?;
                self.finished.insert(owner, result.time_ms);
            }
        }
        self.update_remaining();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::raw::TimingUnit;
    use crate::live::raw_windows::TimingSet;
    fn result(j: i32, d: i32) -> NoteResult {
        NoteResult {
            origin: 5,
            judgement: j,
            judgement_type: 1,
            timing: 2,
            time_ms: 1000,
            origin_diff_ms: 40,
            diff_ms: d,
        }
    }
    #[test]
    fn converted_diff_boundary_and_note_dedup() {
        let mut c =
            WindowController::new(vec![TimingSet { judgement_type: 1, units: vec![TimingUnit::new(6, 10, 10)] }]);
        let mut a = JustWindowApplier::new(50, 10);
        a.execute(1, 5000, 2, &mut c).unwrap();
        assert_eq!(c.timings[0].units[0].before(), 35);
        for r in [result(5, 40), result(6, 0), result(6, 10), result(6, -10)] {
            a.on_judgement(1, &r, &mut c).unwrap();
        }
        assert_eq!(a.remaining(), 2);
        a.on_judgement(1, &result(6, 11), &mut c).unwrap();
        a.on_judgement(1, &result(6, 11), &mut c).unwrap();
        assert_eq!(a.remaining(), 1);
        a.on_judgement(2, &result(6, -11), &mut c).unwrap();
        assert_eq!(a.remaining(), 0);
        assert_eq!(a.begin_update(1), Some(1000));
        assert_eq!(c.timings[0].units[0].before(), 10);
    }
    #[test]
    fn zero_limit_consumed_on_first_qualifying_just() {
        let mut c = WindowController::new(vec![]);
        let mut a = JustWindowApplier::new(5, 0);
        a.execute(7, 5000, 0, &mut c).unwrap();
        a.on_judgement(1, &result(6, 0), &mut c).unwrap();
        assert_eq!(a.begin_update(7), Some(1000));
        assert!(c.callback_handles().is_empty());
    }
}
