//! A lossless identity at the completed deterministic recorder boundary.
//!
//! Equal bytes reconstruct every replay event in its original order. Factor commands are stored with their
//! insertion position in the other event stream, so identical chart/query/combo scaffolding can share storage.
//! No query, readiness event, zero command or signed-zero value is elided. The recorder's observation and filing admission state,
//! and a note's later native accumulator, are not consumers of the independent bounds replay.

use super::*;
use crate::live::score::{ComboTable, LuckWeights, ScoreFactorState};

const MAX_RECORDED_KEY_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum RecordedKeyError {
    TooLarge,
    Cancelled,
}

type KeyResult<T> = Result<T, RecordedKeyError>;

pub(super) struct SharedTrace {
    pub bytes: Vec<u8>,
    pub hash: u64,
}

pub(in super::super) struct RecordedIdentity {
    pub(super) shared: Arc<SharedTrace>,
    pub(super) local: Vec<u8>,
    pub(super) hash: u64,
}

impl RecordedIdentity {
    pub(in super::super) fn same(&self, other: &Self) -> bool {
        self.hash == other.hash && self.local == other.local && self.shared.bytes == other.shared.bytes
    }

    pub(in super::super) fn encoded_len(&self) -> usize {
        self.shared.bytes.len() + self.local.len()
    }
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Writer {
    fn new(limit: usize) -> Self {
        Self { bytes: Vec::new(), limit }
    }

    fn raw(&mut self, bytes: &[u8]) -> KeyResult<()> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(RecordedKeyError::TooLarge);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn byte(&mut self, byte: u8) -> KeyResult<()> {
        self.raw(&[byte])
    }

    /// Unsigned base-128 integers are self-delimiting; signed values use a bijective zigzag mapping.
    fn unsigned(&mut self, mut value: u64) -> KeyResult<()> {
        while value >= 128 {
            self.byte(value as u8 | 128)?;
            value >>= 7;
        }
        self.byte(value as u8)
    }

    fn length(&mut self, value: usize) -> KeyResult<()> {
        self.unsigned(value as u64)
    }

    fn signed(&mut self, value: i64) -> KeyResult<()> {
        self.unsigned(((value as u64) << 1) ^ ((value >> 63) as u64))
    }

    fn integer(&mut self, value: i32) -> KeyResult<()> {
        self.signed(i64::from(value))
    }

    fn float(&mut self, value: f32) -> KeyResult<()> {
        self.raw(&value.to_bits().to_le_bytes())
    }

    fn optional_index(&mut self, value: Option<usize>) -> KeyResult<()> {
        self.byte(u8::from(value.is_some()))?;
        if let Some(value) = value {
            self.length(value)?;
        }
        Ok(())
    }

    fn integer_map(&mut self, values: &std::collections::HashMap<i32, i32>) -> KeyResult<()> {
        // Even the shortest pair needs two bytes. Refuse oversized tables before collecting their entries.
        if values.len() > self.limit.saturating_sub(self.bytes.len()) / 2 {
            return Err(RecordedKeyError::TooLarge);
        }
        self.length(values.len())?;
        let mut values: Vec<_> = values.iter().collect();
        values.sort_unstable_by_key(|&(key, _)| *key);
        for (&key, &value) in values {
            self.integer(key)?;
            self.integer(value)?;
        }
        Ok(())
    }

    fn nested<T>(
        &mut self,
        values: &Option<Vec<Option<Vec<T>>>>,
        mut write: impl FnMut(&mut Self, &T) -> KeyResult<()>,
    ) -> KeyResult<()> {
        self.byte(u8::from(values.is_some()))?;
        if let Some(values) = values {
            self.length(values.len())?;
            for row in values {
                self.byte(u8::from(row.is_some()))?;
                if let Some(row) = row {
                    self.length(row.len())?;
                    for value in row {
                        write(self, value)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn calculator(&mut self, calc: &LiveScoreCalculator) -> KeyResult<()> {
        let LiveScoreCalculator {
            score_adjustment_factor,
            music_difficulty_factor,
            converted_note_count,
            life_onus_factor,
            event_bonus_factor,
            assist_factor,
            note_factor_percent,
            judgement_score_factor_percent,
            state,
            combo_table,
            luck_weight,
        } = calc;
        for value in
            [score_adjustment_factor, music_difficulty_factor, life_onus_factor, event_bonus_factor, assist_factor]
        {
            self.float(*value)?;
        }
        self.integer(*converted_note_count)?;
        self.integer_map(note_factor_percent)?;
        self.integer_map(judgement_score_factor_percent)?;
        let ScoreFactorState {
            band_total_power: _,
            combo_score_up,
            note_score_up,
            just,
            perfect,
            great,
            good,
            added_luck_bonus,
            gekisou_rank_bonus_score,
        } = state;
        // Only initial power is parameterized. Every remaining calculator field retains its exact bits.
        for value in [combo_score_up, note_score_up, just, perfect, great, good] {
            self.float(*value)?;
        }
        self.integer(*added_luck_bonus)?;
        self.integer(*gekisou_rank_bonus_score)?;
        self.byte(u8::from(combo_table.is_some()))?;
        if let Some(ComboTable { thresholds, cumulatives }) = combo_table {
            self.nested(thresholds, |out, value| out.integer(*value))?;
            self.nested(cumulatives, |out, value| out.float(*value))?;
        }
        self.byte(u8::from(luck_weight.is_some()))?;
        if let Some(weights) = luck_weight {
            let LuckWeights { values, steps } = &**weights;
            self.length(values.len())?;
            for value in values {
                self.float(*value)?;
            }
            self.length(steps.len())?;
            for (time, values) in steps {
                self.integer(*time)?;
                self.length(values.len())?;
                for value in values {
                    self.float(*value)?;
                }
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn recorded_identity(
    trace: &BoundsTrace,
    calc: &LiveScoreCalculator,
    rush_percent: i32,
    query_limit: u64,
    final_life: i32,
    capacity: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> KeyResult<RecordedIdentity> {
    if cancelled() {
        return Err(RecordedKeyError::Cancelled);
    }
    let limit = capacity.min(MAX_RECORDED_KEY_BYTES);
    let mut shared = Writer::new(limit);
    // The full replay consumes the dense events; terminal-only gate projection metadata is not an input.
    let BoundsTrace { events, queries, frames, probes, combo: _, has_luck, filing_gate: _, probe_filings: _ } = trace;
    shared.length(*frames)?;
    shared.length(*queries)?;
    shared.byte(u8::from(*has_luck))?;
    shared.length(events.len())?;
    let mut local = Writer::new(limit.saturating_sub(shared.bytes.len()));
    local.calculator(calc)?;
    local.integer(rush_percent)?;
    local.unsigned(query_limit)?;
    local.integer(final_life)?;
    local.length(probes.len())?;
    for ProbeRow { owner, value } in probes {
        local.integer(*owner)?;
        local.float(*value)?;
    }
    local.length(events.iter().filter(|event| matches!(event, BoundsEvent::Factor { .. })).count())?;
    let mut position = 0usize;
    for (index, event) in events.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Err(RecordedKeyError::Cancelled);
        }
        shared.limit = limit.saturating_sub(local.bytes.len());
        local.limit = limit.saturating_sub(shared.bytes.len());
        match event {
            BoundsEvent::Factor { frame, command } => {
                // Equal positions keep their original filing order in this separate command sequence.
                local.length(position)?;
                local.length(*frame)?;
                let FactorCommand {
                    time_ms,
                    owner_id,
                    note_mill,
                    combo_mill,
                    judgement,
                    judge_mill,
                    band_total_power,
                    luck,
                } = command;
                for value in [time_ms, owner_id, note_mill, combo_mill, judgement, judge_mill, band_total_power, luck] {
                    local.integer(*value)?;
                }
                continue;
            }
            BoundsEvent::Note { frame, index, note } => {
                shared.byte(0)?;
                shared.length(*frame)?;
                shared.length(*index)?;
                for value in note.bounds_identity() {
                    shared.integer(value)?;
                }
            }
            BoundsEvent::Potential { frame } => {
                shared.byte(1)?;
                shared.length(*frame)?;
            }
            BoundsEvent::Probe { frame, time_ms } => {
                shared.byte(2)?;
                shared.length(*frame)?;
                shared.integer(*time_ms)?;
            }
            BoundsEvent::Query { time_ms, to } => {
                shared.byte(3)?;
                shared.integer(*time_ms)?;
                shared.integer(*to)?;
            }
            BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                shared.byte(4)?;
                shared.length(*frame)?;
                shared.length(*index)?;
                shared.float(*ordinary)?;
                shared.float(*gekisou)?;
            }
            BoundsEvent::ProbabilityReady(time) => {
                shared.byte(5)?;
                shared.integer(*time)?;
            }
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                shared.byte(6)?;
                shared.length(*range)?;
                shared.integer(*time_ms)?;
                shared.signed(*percent)?;
                shared.optional_index(*start)?;
                shared.optional_index(*end)?;
            }
        }
        position += 1;
    }
    if cancelled() {
        return Err(RecordedKeyError::Cancelled);
    }
    shared.bytes.shrink_to_fit();
    local.bytes.shrink_to_fit();
    let shared = Arc::new(SharedTrace { hash: hash(&shared.bytes), bytes: shared.bytes });
    Ok(RecordedIdentity { hash: hash(&(shared.hash, &local.bytes)), shared, local: local.bytes })
}
