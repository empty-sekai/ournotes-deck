//! Raw result/window bridge for LiveModel. Timings are explicit client inputs.
use super::engine::{END_FRAME, EXECUTE_FRAME};
use super::{EffectRow, StateKey};
use crate::error::Error;
use crate::live::raw::NoteResult;
use crate::live::raw_assist::AssistExecutor;
use crate::live::raw_just::JustWindowApplier;
use crate::live::raw_windows::{NoteWindowApplier, TimingSet, WindowController};
use std::collections::{BTreeMap, HashMap};
/// Recovered from metadata-decoded.dat+0x1165240; SHA256 equals its metadata field name.
pub const RELAX_TARGET_JUDGEMENTS: [i32; 4] = [6, 5, 4, 3];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawJudgedNote {
    pub note_id: i32,
    pub result: NoteResult,
    pub direction_mismatch: bool,
    pub is_easy_flick: bool,
}
/// Mutable window state; the default client diff converter is identity (0x6a69d34).
#[derive(Clone, Debug)]
pub struct RawJudgementRuntime {
    pub windows: WindowController,
    pub just: JustWindowApplier,
    pub assist: Option<AssistExecutor>,
    pub diff_converter: fn(i32, i32) -> i32,
    millis: BTreeMap<i32, NoteWindowApplier>,
    percent: HashMap<i64, Vec<(usize, usize, i32, i32)>>,
    owners: HashMap<StateKey, i64>,
    next_owner: i64,
}
fn game(error: &str) -> Error {
    Error::Game(error.into())
}
impl RawJudgementRuntime {
    pub fn new(timings: Vec<TimingSet>, just_base_expansion_ms: i32, original_just_before_ms: i32) -> Self {
        Self {
            windows: WindowController::new(timings),
            just: JustWindowApplier::new(just_base_expansion_ms, original_just_before_ms),
            diff_converter: |_, d| d,
            assist: None,
            millis: (4000..=4003).map(|t| (t, NoteWindowApplier::new(t).unwrap())).collect(),
            percent: HashMap::new(),
            owners: HashMap::new(),
            next_owner: 0,
        }
    }
    /// Initialize all Assist-level controllers before registering any skills.
    pub fn enable_assist(&mut self, executor: AssistExecutor, levels: Vec<Vec<TimingSet>>) -> Result<(), Error> {
        if !self.owners.is_empty() {
            return Err(game("Assist must be configured before skill updates"));
        }
        if levels.is_empty() {
            return Err(game("Assist needs a level-zero timing controller"));
        }
        self.windows.replicas = levels;
        self.assist = Some(executor);
        Ok(())
    }
    /// Effective eligibility windows are level-specific; base grade lookup remains separate.
    pub fn effective_timings(&self) -> &[TimingSet] {
        match &self.assist {
            Some(a) => &self.windows.replicas[a.current_level as usize],
            None => &self.windows.timings,
        }
    }
    pub(super) fn convert_assist(&mut self, note: &mut RawJudgedNote, unchanged_by_skill: bool) -> Result<(), Error> {
        if unchanged_by_skill {
            if let Some(a) = self.assist.as_mut() {
                note.result.judgement =
                    a.convert(note.note_id, &note.result, note.is_easy_flick, &self.windows.replicas).map_err(game)?;
            }
        }
        Ok(())
    }
    pub(super) fn after_ft(&mut self, judged: &[(super::LiveNote, i32)]) -> Result<(), Error> {
        if let Some(a) = self.assist.as_mut() {
            let notes: Vec<_> = judged.iter().map(|(n, j)| (*j, n.note_operate_type)).collect();
            a.on_update(&notes, self.windows.replicas.len()).map_err(game)?;
        }
        Ok(())
    }
    pub(super) fn update_effect(&mut self, key: StateKey, phase: u8, row: &EffectRow) -> Result<Option<i32>, Error> {
        let owner = *self.owners.entry(key).or_insert_with(|| {
            self.next_owner = self.next_owner.wrapping_add(1);
            self.next_owner
        });
        let value = row.effect_value as i32;
        let limit = row.effect_limit_count as i32;
        match row.effect_type {
            4000..=4003 => {
                let a = self.millis.get_mut(&(row.effect_type as i32)).unwrap();
                let finished = a.begin_update(owner);
                let phase = if finished.is_some() { END_FRAME } else { phase };
                if phase == END_FRAME {
                    a.end(owner, &mut self.windows).map_err(game)?;
                } else if phase == EXECUTE_FRAME {
                    let target = if row.effect_type == 4000 { None } else { row.targets()?.first().map(|&v| v as i32) };
                    a.execute(owner, value, limit, target, &mut self.windows).map_err(game)?;
                }
                Ok(finished)
            }
            13001 => {
                let finished = self.just.begin_update(owner);
                let phase = if finished.is_some() { END_FRAME } else { phase };
                if phase == END_FRAME {
                    self.just.end(owner, &mut self.windows).map_err(game)?;
                } else if phase == EXECUTE_FRAME {
                    self.just.execute(owner, value as i64, limit, &mut self.windows).map_err(game)?;
                }
                Ok(finished)
            }
            4004 => {
                if phase == EXECUTE_FRAME {
                    if self.percent.contains_key(&owner) {
                        return Err(game("duplicate percent window owner"));
                    }
                    let minimum = row.targets()?.first().copied().unwrap_or(3) as i32;
                    let targets: Vec<_> = RELAX_TARGET_JUDGEMENTS.into_iter().filter(|&v| v >= minimum).collect();
                    self.percent.insert(owner, self.windows.percent(&targets, value as f32 / 10000.0));
                } else if phase == END_FRAME {
                    if let Some(delta) = self.percent.remove(&owner) {
                        self.windows.disable_percent(&delta);
                    }
                }
                Ok(None)
            }
            t => Err(Error::Unsupported(format!("raw window effect {t}"))),
        }
    }
    /// LiveExecutor.UpdateCurrentFrameParameters, AFTER FT finishes all note updaters.
    pub(super) fn on_executor_judgement(&mut self, note: RawJudgedNote) -> Result<(), Error> {
        for a in self.millis.values_mut() {
            if a.has_callbacks() {
                a.on_judgement(note.note_id, note.result.time_ms, &mut self.windows).map_err(game)?;
            }
        }
        self.just.on_judgement(note.note_id, &note.result, &mut self.windows).map_err(game)?;
        Ok(())
    }
    pub(super) fn frame_finish(&mut self) {
        self.windows.frame_finish();
    }
}
