//! Mutable windows and 4000..4003 callback state, separate from already-judged `full`.
//! Covers the applier factory, the window appliers and the window controller.
use super::raw::{TimingUnit, enhance_percent};
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Debug)]
pub struct TimingSet {
    pub judgement_type: i32,
    pub units: Vec<TimingUnit>,
}
#[derive(Clone, Debug)]
struct Change {
    judgement: i32,
    before: i32,
    after: i32,
}
#[derive(Clone, Debug, Default)]
pub struct WindowController {
    pub timings: Vec<TimingSet>,
    /// Assist per-level controllers mirror additive handles, but not inherited percent effects.
    pub replicas: Vec<Vec<TimingSet>>,
    next_id: i32,
    changes: BTreeMap<i32, Change>,
    pending: Vec<i32>,
}
impl WindowController {
    pub fn new(timings: Vec<TimingSet>) -> Self {
        Self { timings, ..Default::default() }
    }
    /// Every matching unit across every type; no Miss cap for additive changes.
    pub fn enhance(&mut self, judgement: i32, before: i32, after: i32) -> i32 {
        self.next_id = self.next_id.wrapping_add(1);
        for set in self.timings.iter_mut().chain(self.replicas.iter_mut().flatten()) {
            for u in &mut set.units {
                if u.judgement == judgement {
                    u.add(before, after);
                }
            }
        }
        self.changes.insert(self.next_id, Change { judgement, before, after });
        self.next_id
    }
    /// Subtraction is immediate; dictionary removal waits for frame finish.
    /// Native double-disable before frame finish subtracts twice, and so does this API.
    pub fn disable(&mut self, id: i32) -> Result<(), &'static str> {
        let c = self.changes.get(&id).ok_or("unknown window handle")?;
        for set in self.timings.iter_mut().chain(self.replicas.iter_mut().flatten()) {
            for u in &mut set.units {
                if u.judgement == c.judgement {
                    u.add(c.before.wrapping_neg(), c.after.wrapping_neg());
                }
            }
        }
        self.pending.push(id);
        Ok(())
    }
    pub fn callback_handles(&self) -> Vec<i32> {
        self.changes.keys().copied().filter(|id| !self.pending.contains(id)).collect()
    }
    pub fn frame_finish(&mut self) {
        for id in self.pending.drain(..) {
            self.changes.remove(&id);
        }
    }
    /// Percent deltas are snapshotted and have no judgement callbacks.
    pub fn percent(&mut self, targets: &[i32], factor: f32) -> Vec<(usize, usize, i32, i32)> {
        let mut out = Vec::new();
        for (set, s) in self.timings.iter_mut().enumerate() {
            out.extend(enhance_percent(&mut s.units, targets, factor).into_iter().map(|(u, b, a)| (set, u, b, a)));
        }
        out
    }
    pub fn disable_percent(&mut self, deltas: &[(usize, usize, i32, i32)]) {
        for &(s, u, b, a) in deltas {
            self.timings[s].units[u].add(b.wrapping_neg(), a.wrapping_neg());
        }
    }
}
/// Applier grades are 6,5,4,3, not Bad/Miss/Pass. Only the first target is used.
pub fn effect_targets(effect_type: i32, first_target: Option<i32>) -> Result<Vec<i32>, &'static str> {
    if !(4000..=4003).contains(&effect_type) {
        return Err("not a millisecond window effect");
    }
    if effect_type != 4000 && first_target.is_none() {
        return Err("target list is empty");
    }
    let target = first_target.unwrap_or_default();
    Ok([6, 5, 4, 3]
        .into_iter()
        .filter(|&j| match effect_type {
            4000 => true,
            4001 => j == target,
            4002 => j >= target,
            4003 => j <= target,
            _ => false,
        })
        .collect())
}
#[derive(Clone, Debug)]
struct Limited {
    owner: i64,
    count: i32,
    limit: i32,
}
#[derive(Clone, Debug)]
pub struct NoteWindowApplier {
    pub effect_type: i32,
    handles: BTreeMap<i64, Vec<i32>>,
    limited: Vec<Limited>,
    seen: HashSet<i32>,
    finished: BTreeMap<i64, i32>,
}
impl NoteWindowApplier {
    /// One independent instance per effect type, as in the client factory.
    pub fn new(effect_type: i32) -> Result<Self, &'static str> {
        if !(4000..=4003).contains(&effect_type) {
            return Err("not a millisecond window effect");
        }
        Ok(Self {
            effect_type,
            handles: BTreeMap::new(),
            limited: Vec::new(),
            seen: HashSet::new(),
            finished: BTreeMap::new(),
        })
    }
    /// Only positive-limit parameters have callbacks; empty target sets register none.
    pub fn has_callbacks(&self) -> bool {
        self.limited.iter().any(|p| self.handles.get(&p.owner).is_some_and(|ids| !ids.is_empty()))
    }
    /// EACH Update clears dedup. Deliver the returned time to LimitFinishedHandler
    /// BEFORE reading the possibly mutated effect phase and calling execute/end.
    pub fn begin_update(&mut self, owner: i64) -> Option<i32> {
        self.seen.clear();
        self.finished.remove(&owner)
    }
    /// ExecuteFrame only. Duplicate owner is an error (native Dictionary.Add).
    pub fn execute(
        &mut self,
        owner: i64,
        value_ms: i32,
        limit_count: i32,
        first_target: Option<i32>,
        controller: &mut WindowController,
    ) -> Result<(), &'static str> {
        if self.handles.contains_key(&owner) {
            return Err("window owner already registered");
        }
        let targets = effect_targets(self.effect_type, first_target)?;
        let ids = targets.into_iter().map(|j| controller.enhance(j, value_ms, value_ms)).collect();
        self.handles.insert(owner, ids);
        if limit_count > 0 {
            self.limited.push(Limited { owner, count: 0, limit: limit_count });
        }
        Ok(())
    }
    pub fn end(&mut self, owner: i64, controller: &mut WindowController) -> Result<(), &'static str> {
        if let Some(ids) = self.handles.remove(&owner) {
            for id in ids {
                controller.disable(id)?;
            }
        }
        self.limited.retain(|p| p.owner != owner);
        Ok(())
    }
    /// Registered callbacks dedup note IDs across handles. All limited owners count
    /// the note, regardless of the judgement grade; exhausted windows disappear now.
    pub fn on_judgement(
        &mut self,
        note_id: i32,
        judgement_time_ms: i32,
        controller: &mut WindowController,
    ) -> Result<(), &'static str> {
        if !self.seen.insert(note_id) {
            return Ok(());
        }
        for i in (0..self.limited.len()).rev() {
            self.limited[i].count = self.limited[i].count.wrapping_add(1);
            if self.limited[i].count >= self.limited[i].limit {
                let owner = self.limited.remove(i).owner;
                if let Some(ids) = self.handles.remove(&owner) {
                    for id in ids {
                        controller.disable(id)?;
                    }
                }
                self.finished.insert(owner, judgement_time_ms);
            }
        }
        Ok(())
    }
}
/// Resolves an assisted judgement. The current windows are indexed by the base length.
/// Excludes Just6; EasyFlick fallback can return Great even for an original Perfect.
pub fn resolve_assist(
    original: i32,
    diff: i32,
    easy_flick: bool,
    base: &[TimingUnit],
    current: &[TimingUnit],
) -> Result<(i32, bool), &'static str> {
    if current.len() < base.len() {
        return Err("current assist timing array is shorter than base");
    }
    let grade = |units: &[TimingUnit]| {
        let mut grade = original;
        for u in units.iter().rev() {
            if original < u.judgement
                && u.judgement < 6
                && ((u.before().wrapping_neg() <= diff && diff < 0) || (diff >= 0 && diff <= u.after()))
            {
                grade = u.judgement;
            }
        }
        grade
    };
    let base_grade = grade(base);
    let current_grade = grade(&current[..base.len()]);
    Ok(if current_grade > base_grade {
        (current_grade, true)
    } else if current_grade != original {
        (current_grade, false)
    } else if easy_flick {
        (4, true)
    } else {
        (original, false)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn controller() -> WindowController {
        WindowController::new(vec![TimingSet {
            judgement_type: 0,
            units: vec![TimingUnit::new(6, 10, 10), TimingUnit::new(5, 30, 30), TimingUnit::new(1, 100, 100)],
        }])
    }
    /// Reference vectors recorded from the client; see `tests/reference/mod.rs`.
    #[cfg(feature = "native-fixtures")]
    #[test]
    fn client_limit_reference_vectors() {
        let dir = std::env::var_os("OURNOTES_FIXTURES")
            .expect("the native-fixtures feature reads its fixtures from the directory in OURNOTES_FIXTURES");
        let path = std::path::PathBuf::from(dir).join("raw_window_limit_native.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
        assert_eq!(rows.len(), 16);
        for sequence in rows.chunks(4) {
            let mut controller = controller();
            let mut applier = NoteWindowApplier::new(4000).unwrap();
            let limits = &sequence[0]["limits"];
            for (i, owner) in [10, 20].into_iter().enumerate() {
                applier.execute(owner, 5, limits[i].as_i64().unwrap() as i32, None, &mut controller).unwrap();
            }
            for row in sequence {
                applier
                    .on_judgement(
                        row["note_id"].as_i64().unwrap() as i32,
                        row["time"].as_i64().unwrap() as i32,
                        &mut controller,
                    )
                    .unwrap();
                let limited: Vec<_> =
                    applier.limited.iter().map(|p| serde_json::json!([p.owner, p.count, p.limit])).collect();
                let actual =
                    serde_json::json!({"limited":limited,"finished":applier.finished,"disabled":controller.pending});
                assert_eq!(actual, row["result"]);
            }
        }
    }
    #[test]
    fn target_modes() {
        assert_eq!(effect_targets(4000, None).unwrap(), vec![6, 5, 4, 3]);
        assert_eq!(effect_targets(4001, Some(2)).unwrap(), Vec::<i32>::new());
        assert_eq!(effect_targets(4002, Some(4)).unwrap(), vec![6, 5, 4]);
        assert_eq!(effect_targets(4003, Some(4)).unwrap(), vec![4, 3]);
    }
    #[test]
    fn limit_dedup_and_immediate_window_removal() {
        let mut c = controller();
        let mut a = NoteWindowApplier::new(4000).unwrap();
        a.begin_update(1);
        a.execute(1, 5, 2, None, &mut c).unwrap();
        assert_eq!(c.timings[0].units[0].before(), 15);
        a.on_judgement(1, 1000, &mut c).unwrap();
        a.on_judgement(1, 1000, &mut c).unwrap();
        assert_eq!(c.callback_handles().len(), 4);
        a.on_judgement(2, 1100, &mut c).unwrap();
        assert_eq!(c.timings[0].units[0].before(), 10);
        assert!(c.callback_handles().is_empty());
        assert_eq!(a.begin_update(1), Some(1100));
        assert_eq!(a.begin_update(1), None);
        c.frame_finish();
        assert!(c.disable(1).is_err());
    }
    #[test]
    fn assist_excludes_just_and_marks_only_extra_assistance() {
        let base = vec![TimingUnit::new(6, 10, 10), TimingUnit::new(5, 30, 30), TimingUnit::new(4, 60, 60)];
        let mut current = base.clone();
        current[1].add(20, 20);
        assert_eq!(resolve_assist(3, 40, false, &base, &current), Ok((5, true)));
        assert_eq!(resolve_assist(3, 20, false, &base, &current), Ok((5, false)));
        assert_eq!(resolve_assist(5, 0, false, &base, &current), Ok((5, false)));
        assert_eq!(resolve_assist(5, 0, true, &base, &current), Ok((4, true)));
    }
}
