//! Certified note kernels after all power-independent factor, combo and query histories have been replayed.
//!
//! Admission is the parent recorder's exhaustive dependency certificate. Initial total power reaches only the
//! note arithmetic; it cannot change a command, judgement, life value, query, nominal curve or rank arrival.
//! Every new power reevaluates the original binary32 operations, integer floors and signed rank differences.
//! No score scaling, power monotonicity or relationship between different performer programs is assumed.

use super::*;
use crate::num::FxHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

mod recorded;
pub(super) use recorded::{RecordedIdentity, RecordedKeyError, recorded_identity};

const MAX_SCOPE_BYTES: usize = 512 * 1024;
const MAX_ENTRIES: usize = 128;
const ARC_HEADER_BYTES: usize = 2 * size_of::<usize>();

fn hash(value: &impl Hash) -> u64 {
    let mut out = FxHasher::default();
    value.hash(&mut out);
    out.finish()
}

/// Shared inputs absent from the initialized identity, including its extracted chart notes and events.
/// Integers and binary32 clocks keep every bit and field.
pub(super) struct Scope {
    bytes: Vec<u8>,
    hash: u64,
}

pub(super) fn scope(
    skills: &LuckSkills,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    play: &LivePlay,
    deltas: &[f32],
) -> Option<Arc<Scope>> {
    let skills = super::super::luck_exact::state_identity(skills)?;
    let LivePlay { frames, base_seed } = play;
    let mut bytes = Vec::new();
    let mut append = |value: &[u8]| {
        if bytes.len().saturating_add(value.len()) > MAX_SCOPE_BYTES {
            return None;
        }
        bytes.extend_from_slice(value);
        Some(())
    };
    append(&(skills.len() as u64).to_le_bytes())?;
    append(skills.as_bytes())?;
    append(&(notes.len() as u64).to_le_bytes())?;
    for LiveNote { note_id, time_ms, note_operate_type, judgement_type } in notes {
        append(&note_id.to_le_bytes())?;
        append(&time_ms.to_le_bytes())?;
        append(&note_operate_type.to_le_bytes())?;
        append(&judgement_type.to_le_bytes())?;
    }
    append(&(events.len() as u64).to_le_bytes())?;
    for (time, kind) in events {
        append(&time.to_le_bytes())?;
        append(&kind.to_le_bytes())?;
    }
    append(&base_seed.to_le_bytes())?;
    append(&(frames.len() as u64).to_le_bytes())?;
    for PlayFrame { time_ms, judged } in frames {
        append(&time_ms.to_le_bytes())?;
        append(&(judged.len() as u64).to_le_bytes())?;
        for JudgedNote { note_id, judgement, judgement_time_ms } in judged {
            append(&note_id.to_le_bytes())?;
            append(&judgement.to_le_bytes())?;
            append(&judgement_time_ms.to_le_bytes())?;
        }
    }
    append(&(deltas.len() as u64).to_le_bytes())?;
    for delta in deltas {
        append(&delta.to_bits().to_le_bytes())?;
    }
    bytes.shrink_to_fit();
    Some(Arc::new(Scope { hash: hash(&bytes), bytes }))
}

pub(super) struct Identity {
    model: String,
    scope: Arc<Scope>,
    hash: u64,
    rush_percent: i32,
}

pub(super) fn identity(model: &mut LiveModel, scope: &Arc<Scope>, rush_percent: i32) -> Option<Identity> {
    let power = std::mem::replace(&mut model.score.calc.state.band_total_power, 0);
    // Construction copies these exact chart inputs, already retained in the shared scope. The remaining
    // initialized runtime state keeps every performer-dependent value and every derived controller field.
    let notes = std::mem::take(&mut model.notes);
    let events = std::mem::take(&mut model.events);
    let identity = super::super::luck_exact::initialized_identity(model);
    model.notes = notes;
    model.events = events;
    model.score.calc.state.band_total_power = power;
    let model = identity?;
    Some(Identity { hash: hash(&(&model, scope.hash, rush_percent)), model, scope: Arc::clone(scope), rush_percent })
}

struct Entry {
    identity: Option<Identity>,
    recorded: Option<RecordedIdentity>,
    program: Arc<Program>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(in super::super) struct CacheStats {
    pub lookups: u64,
    pub hits: u64,
    pub recorded_lookups: u64,
    pub recorded_hits: u64,
    pub recorded_key_declines: u64,
    pub recorded_peak_key_bytes: usize,
    pub compilations: u64,
    pub evictions: u64,
    pub peak_entries: usize,
    pub peak_bytes: usize,
}

#[derive(Default)]
pub(in super::super) struct ProgramCache {
    entries: VecDeque<Entry>,
    capacity: usize,
    pub(in super::super) stats: CacheStats,
}

impl ProgramCache {
    pub(in super::super) fn limit(&mut self, capacity: usize) {
        if self.capacity == capacity {
            return;
        }
        self.capacity = capacity;
        while self.allocated_bytes() > capacity {
            self.entries.pop_front();
            self.stats.evictions += 1;
            if self.entries.is_empty() {
                self.entries.shrink_to_fit();
            }
        }
    }

    pub(super) fn get(&mut self, identity: &Identity, curve: &Arc<LuckDpCertifiedResult>) -> Option<Arc<Program>> {
        self.stats.lookups += 1;
        let found = self.entries.iter().find(|entry| {
            entry.identity.as_ref().is_some_and(|old| {
                old.hash == identity.hash
                    && old.model == identity.model
                    && old.scope.bytes == identity.scope.bytes
                    && old.rush_percent == identity.rush_percent
            }) && Arc::ptr_eq(&entry.program.curve, curve)
        });
        self.stats.hits += u64::from(found.is_some());
        found.map(|entry| Arc::clone(&entry.program))
    }

    pub(super) fn get_recorded(
        &mut self,
        identity: &RecordedIdentity,
        curve: &Arc<LuckDpCertifiedResult>,
    ) -> Option<Arc<Program>> {
        self.stats.recorded_lookups += 1;
        self.stats.recorded_peak_key_bytes = self.stats.recorded_peak_key_bytes.max(identity.encoded_len());
        let found = self.entries.iter().find(|entry| {
            entry.recorded.as_ref().is_some_and(|old| old.same(identity)) && Arc::ptr_eq(&entry.program.curve, curve)
        });
        self.stats.recorded_hits += u64::from(found.is_some());
        found.map(|entry| Arc::clone(&entry.program))
    }

    pub(super) fn decline_recorded_key(&mut self) {
        self.stats.recorded_key_declines += 1;
    }

    pub(super) fn insert(
        &mut self,
        mut identity: Option<Identity>,
        mut recorded: Option<RecordedIdentity>,
        program: Program,
    ) {
        self.stats.compilations += 1;
        if let Some(identity) = &mut identity
            && let Some(old) = self
                .entries
                .iter()
                .filter_map(|entry| entry.identity.as_ref())
                .find(|old| old.scope.hash == identity.scope.hash && old.scope.bytes == identity.scope.bytes)
        {
            identity.scope = Arc::clone(&old.scope);
        }
        if let Some(recorded) = &mut recorded
            && let Some(old) = self
                .entries
                .iter()
                .filter_map(|entry| entry.recorded.as_ref())
                .find(|old| old.shared.hash == recorded.shared.hash && old.shared.bytes == recorded.shared.bytes)
        {
            recorded.shared = Arc::clone(&old.shared);
        }
        let program = Arc::new(program);
        let own = size_of::<Entry>()
            + identity.as_ref().map_or(0, |identity| identity.model.capacity())
            + recorded.as_ref().map_or(0, |recorded| recorded.local.capacity())
            + program.allocated_bytes()
            + ARC_HEADER_BYTES;
        let shared = identity
            .as_ref()
            .map_or(0, |identity| identity.scope.bytes.capacity() + size_of::<Scope>() + ARC_HEADER_BYTES)
            + recorded.as_ref().map_or(0, |recorded| {
                recorded.shared.bytes.capacity() + size_of::<recorded::SharedTrace>() + ARC_HEADER_BYTES
            })
            + curve_bytes(&program.curve);
        if own.saturating_add(shared) > self.capacity || self.capacity == 0 {
            return;
        }
        while self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_front();
            self.stats.evictions += 1;
        }
        self.entries.push_back(Entry { identity, recorded, program });
        while self.allocated_bytes() > self.capacity {
            self.entries.pop_front();
            self.stats.evictions += 1;
            if self.entries.is_empty() {
                self.entries.shrink_to_fit();
            }
        }
        self.stats.peak_entries = self.stats.peak_entries.max(self.entries.len());
        self.stats.peak_bytes = self.stats.peak_bytes.max(self.allocated_bytes());
    }

    fn allocated_bytes(&self) -> usize {
        let mut curves = FxHashSet::default();
        let mut scopes = FxHashSet::default();
        let mut traces = FxHashSet::default();
        self.entries.iter().fold(self.entries.capacity() * size_of::<Entry>(), |mut bytes, entry| {
            bytes += entry.program.allocated_bytes() + ARC_HEADER_BYTES;
            if curves.insert(Arc::as_ptr(&entry.program.curve)) {
                bytes += curve_bytes(&entry.program.curve);
            }
            if let Some(identity) = &entry.identity {
                bytes += identity.model.capacity();
                if scopes.insert(Arc::as_ptr(&identity.scope)) {
                    bytes += size_of::<Scope>() + identity.scope.bytes.capacity() + ARC_HEADER_BYTES;
                }
            }
            if let Some(recorded) = &entry.recorded {
                bytes += recorded.local.capacity();
                if traces.insert(Arc::as_ptr(&recorded.shared)) {
                    bytes += size_of::<recorded::SharedTrace>() + recorded.shared.bytes.capacity() + ARC_HEADER_BYTES;
                }
            }
            bytes
        })
    }
}

fn curve_bytes(curve: &LuckDpCertifiedResult) -> usize {
    ARC_HEADER_BYTES
        + size_of::<LuckDpCertifiedResult>()
        + curve.steps.capacity() * size_of::<(i32, [ProbabilityMass; 4])>()
        + curve.probes.capacity() * size_of::<bool>()
}

/// Power-independent factors in the original grouping. Note identity/time select a probability, not native
/// note arithmetic. Different histories with the same endpoint bits need only one arithmetic kernel.
struct Kernel {
    note_type: i32,
    score_type: i32,
    alive: bool,
    factors: NoteFactors,
}

impl Kernel {
    fn key(&self) -> [u32; 13] {
        let mut key = [0; 13];
        key[..3].copy_from_slice(&[self.note_type as u32, self.score_type as u32, u32::from(self.alive)]);
        for (class, factors) in self.factors.iter().enumerate() {
            if let Some(factors) = factors {
                let start = 3 + class * 5;
                key[start] = 1;
                for (field, value) in factors.iter().enumerate() {
                    key[start + 1 + field * 2] = value.lower().to_bits();
                    key[start + 2 + field * 2] = value.upper().to_bits();
                }
            }
        }
        key
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Link {
    kernel: u32,
    /// None: probability commands are not filed; Some(None): before the curve's first step.
    mass: Option<Option<usize>>,
}

pub(super) struct Query {
    pub original: usize,
    pub to: i32,
    pub executed_from: i32,
    pub notes: Vec<(i32, u32)>,
    pub fixed: Vec<u8>,
    pub rank_notes: bool,
}

struct Rank {
    range: usize,
    start: Option<usize>,
    end: usize,
    start_slot: Option<usize>,
    end_slot: usize,
    kept: Option<i32>,
    percent: i64,
}

enum Event {
    Query(Query),
    Rank(Rank),
    FileRank(usize),
}

pub(super) struct Builder {
    capacity: usize,
    bytes: usize,
    valid: bool,
    kernels: Vec<Kernel>,
    kernel_ids: FxHashMap<[u32; 13], u32>,
    links: Vec<Link>,
    link_ids: FxHashMap<Link, u32>,
    events: Vec<Event>,
    queries: FxHashMap<usize, usize>,
    final_notes: Vec<u32>,
    final_fixed: Vec<u8>,
    ranks: usize,
    pending_rank: Option<usize>,
}

impl Builder {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: 0,
            valid: true,
            kernels: Vec::new(),
            kernel_ids: FxHashMap::default(),
            links: Vec::new(),
            link_ids: FxHashMap::default(),
            events: Vec::new(),
            queries: FxHashMap::default(),
            final_notes: Vec::new(),
            final_fixed: Vec::new(),
            ranks: 0,
            pending_rank: None,
        }
    }

    fn charge(&mut self, bytes: usize) -> bool {
        self.bytes = self.bytes.saturating_add(bytes);
        self.valid &= self.bytes <= self.capacity;
        self.valid
    }

    pub(super) fn note(
        &mut self,
        note: &NoteCommand,
        executed: Classes,
        combo: F32Interval,
        gekisou: F32Interval,
        probability: &LuckDpCertifiedResult,
        ready: bool,
    ) -> Option<u32> {
        if !self.valid {
            return None;
        }
        let factors = match note_factors(note.score_type, executed, combo, gekisou) {
            Ok(factors) => factors,
            Err(_) => {
                // The preceding native enclosure normally proves these same operations. An optional
                // compilation must still be abandoned if a future caller supplies an invalid kernel.
                self.valid = false;
                return None;
            }
        };
        let kernel = Kernel { note_type: note.note_type, score_type: note.score_type, alive: note.life > 0, factors };
        let key = kernel.key();
        let kernel_id = if let Some(&id) = self.kernel_ids.get(&key) {
            id
        } else {
            // Includes transient interning tables and geometric Vec/HashMap growth, not just retained payload.
            if !self.charge(3 * (size_of::<Kernel>() + size_of::<([u32; 13], u32)>())) {
                return None;
            }
            let Ok(id) = u32::try_from(self.kernels.len()) else {
                self.valid = false;
                return None;
            };
            self.kernels.push(kernel);
            self.kernel_ids.insert(key, id);
            id
        };
        let mass = ready.then(|| probability.steps.partition_point(|(time, _)| *time <= note.time_ms).checked_sub(1));
        let link = Link { kernel: kernel_id, mass };
        if let Some(&id) = self.link_ids.get(&link) {
            return Some(id);
        }
        if !self.charge(3 * (size_of::<Link>() + size_of::<(Link, u32)>())) {
            return None;
        }
        let Ok(id) = u32::try_from(self.links.len()) else {
            self.valid = false;
            return None;
        };
        self.links.push(link);
        self.link_ids.insert(link, id);
        Some(id)
    }

    pub(super) fn query(&mut self, query: Query) {
        if self.charge(
            2 * (size_of::<Event>() + query.notes.capacity() * size_of::<(i32, u32)>() + query.fixed.capacity()),
        ) {
            self.queries.insert(query.original, self.queries.len());
            self.events.push(Event::Query(query));
        }
    }

    pub(super) fn rank(
        &mut self,
        range: usize,
        start: Option<usize>,
        end: usize,
        percent: i64,
        queries: &[QueryParts],
    ) {
        if self.charge(2 * size_of::<Event>()) {
            self.pending_rank = Some(self.ranks);
            self.ranks += 1;
            self.events.push(Event::Rank(Rank {
                range,
                start,
                end,
                start_slot: start.map(|index| self.queries[&index]),
                end_slot: self.queries[&end],
                kept: kept_prefix(start, end, queries),
                percent,
            }));
        }
    }

    pub(super) fn file_rank(&mut self) {
        if self.charge(2 * size_of::<Event>()) {
            self.events.push(Event::FileRank(self.pending_rank.take().expect("pending rank calculation")));
        }
    }

    pub(super) fn final_note(&mut self, index: u32) {
        if self.charge(2 * size_of::<u32>()) {
            self.final_notes.push(index);
        }
    }

    pub(super) fn final_fixed(&mut self, fixed: Vec<u8>) {
        self.final_fixed = fixed;
    }

    pub(super) fn finish(
        self,
        mut calc: LiveScoreCalculator,
        rush_percent: i32,
        curve: Arc<LuckDpCertifiedResult>,
        bounds: &LuckScoreBounds,
    ) -> Option<Program> {
        if !self.valid {
            return None;
        }
        // These fields are not read by the explicit factor/combo kernel; do not keep unrelated table storage.
        let LiveScoreCalculator {
            score_adjustment_factor: _,
            music_difficulty_factor: _,
            converted_note_count: _,
            life_onus_factor: _,
            event_bonus_factor: _,
            assist_factor: _,
            note_factor_percent: _,
            judgement_score_factor_percent: _,
            state: _,
            combo_table,
            luck_weight,
        } = &mut calc;
        *combo_table = None;
        *luck_weight = None;
        let mut program = Program {
            calc,
            rush_percent,
            curve,
            kernels: self.kernels,
            links: self.links,
            events: self.events,
            final_notes: self.final_notes,
            final_fixed: self.final_fixed,
            model: bounds.model,
            final_life: bounds.exact_final_life,
            query_limit: bounds.query_limit,
            actual_queries: bounds.actual_queries,
            allocation_bytes: 0,
        };
        // Program storage is immutable after construction; include this cached-size field itself through
        // size_of::<Self>() and measure nested query vectors only once.
        program.allocation_bytes = program.measure_allocated_bytes();
        Some(program)
    }
}

pub(super) struct Program {
    calc: LiveScoreCalculator,
    rush_percent: i32,
    curve: Arc<LuckDpCertifiedResult>,
    kernels: Vec<Kernel>,
    links: Vec<Link>,
    events: Vec<Event>,
    final_notes: Vec<u32>,
    final_fixed: Vec<u8>,
    model: &'static str,
    final_life: Option<i32>,
    query_limit: u64,
    actual_queries: usize,
    allocation_bytes: usize,
}

impl Program {
    fn allocated_bytes(&self) -> usize {
        self.allocation_bytes
    }

    fn measure_allocated_bytes(&self) -> usize {
        size_of::<Self>()
            + self.kernels.capacity() * size_of::<Kernel>()
            + self.links.capacity() * size_of::<Link>()
            + self.events.capacity() * size_of::<Event>()
            + self.final_notes.capacity() * size_of::<u32>()
            + self.final_fixed.capacity()
            + (self.calc.note_factor_percent.capacity() + self.calc.judgement_score_factor_percent.capacity())
                * (size_of::<(i32, i32)>() + size_of::<usize>() + 1)
            + self
                .events
                .iter()
                .map(|event| match event {
                    Event::Query(query) => query.notes.capacity() * size_of::<(i32, u32)>() + query.fixed.capacity(),
                    Event::Rank(_) | Event::FileRank(_) => 0,
                })
                .sum::<usize>()
    }

    pub(super) fn evaluate(
        &self,
        power: i32,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<LuckScoreBounds>, Error> {
        let mut kernels = Vec::with_capacity(self.kernels.len());
        for (index, kernel) in self.kernels.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            let note = NoteCommand::new(0, i32::from(kernel.alive), 0, kernel.note_type, kernel.score_type);
            let (bounds, support) = note_bounds_with_factors(&self.calc, power, &note, self.rush_percent, |class| {
                Ok(kernel.factors[class])
            })?;
            kernels.push((bounds.buckets, support));
        }
        let mut links = Vec::with_capacity(self.links.len());
        for (index, link) in self.links.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            let (buckets, support) = &kernels[link.kernel as usize];
            links.push(match link.mass {
                Some(mass) => link_buckets(buckets, note_mass(&self.curve, mass))?,
                None => (support.as_real(), *support),
            });
        }
        let mut queries = Vec::<QueryParts>::new();
        let mut fixed = Vec::<(i32, u8, F64Interval, I32Interval)>::new();
        let mut rank_values = Vec::new();
        let mut ranges = Vec::new();
        let mut final_support = I32Interval::point(0);
        for event in &self.events {
            if cancelled() {
                return Ok(None);
            }
            match event {
                Event::Query(query) => {
                    let mut mean = F64Interval::ZERO;
                    let (mut lower, mut upper) = (0i64, 0i64);
                    let mut notes = query.rank_notes.then(Vec::new);
                    for (index, &(frame, link)) in query.notes.iter().enumerate() {
                        if index.is_multiple_of(64) && cancelled() {
                            return Ok(None);
                        }
                        let (note_mean, support) = links[link as usize];
                        mean = mean.add(note_mean);
                        lower += i64::from(support.lower());
                        upper += i64::from(support.upper());
                        if let Some(notes) = &mut notes {
                            notes.push((frame, support, note_mean));
                        }
                    }
                    let part = QueryParts {
                        note_mean: mean,
                        note_support: checked_support(lower, upper)?,
                        fixed_coefficients: query.fixed.clone(),
                        to: query.to,
                        executed_from: query.executed_from,
                        notes,
                    };
                    for (&coefficient, &(_, _, _, support)) in query.fixed.iter().zip(&fixed) {
                        lower += i64::from(coefficient) * i64::from(support.lower());
                        upper += i64::from(coefficient) * i64::from(support.upper());
                    }
                    final_support = checked_support(lower, upper)?;
                    queries.push(part);
                }
                Event::Rank(rank) => {
                    let (mean, support) = snapshot_difference_with_kept(
                        rank.start_slot.map(|index| &queries[index]),
                        &queries[rank.end_slot],
                        rank.kept,
                        &fixed,
                    )?;
                    let (bonus, bonus_support) = rank_bonus_bounds(mean, support, rank.percent)?;
                    rank_values.push((0, 0, bonus, bonus_support));
                    ranges.push(LuckRangeScoreBounds {
                        range: rank.range,
                        start_query: rank.start,
                        end_query: rank.end,
                        percent: rank.percent,
                        mean: mean.into(),
                        support: support.into(),
                        bonus_mean: bonus.into(),
                        bonus_support: bonus_support.into(),
                    });
                }
                Event::FileRank(index) => {
                    // Only the last pending bonus is filed by the next original query. Several external
                    // confirmations before that query overwrite the pending value, just as the native recorder.
                    fixed.push(rank_values[*index]);
                }
            }
        }
        let mut final_note_mean = F64Interval::ZERO;
        for (index, &link) in self.final_notes.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            final_note_mean = final_note_mean.add(links[link as usize].0);
        }
        let mut final_rank_mean = F64Interval::ZERO;
        for (&coefficient, &(_, _, bonus, _)) in self.final_fixed.iter().zip(&fixed) {
            final_rank_mean = final_rank_mean.add(bonus.scale_integer(i128::from(coefficient)));
        }
        Ok(Some(LuckScoreBounds {
            model: self.model,
            queries: Vec::new(),
            ranges,
            final_mean: final_note_mean.add(final_rank_mean).into(),
            final_note_mean: final_note_mean.into(),
            final_rank_mean: final_rank_mean.into(),
            final_support: final_support.into(),
            exact_final_life: self.final_life,
            final_notes: Vec::new(),
            query_limit: self.query_limit,
            actual_queries: self.actual_queries,
            probability_peak_states: self.curve.peak_states,
            probability_transitions: self.curve.transitions,
        }))
    }
}
