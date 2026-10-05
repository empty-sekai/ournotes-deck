//! Exact score programs, parameterized only by initial total power.
//!
//! A program belongs to one complete model construction and one declared run. The skill interpreter cannot read
//! total power or score: power initializes the score calculator; score reaches only Gekisou range snapshots and
//! rank-bonus arithmetic. Solo confirms rank 1 independently of the range score. Consequently the executed
//! note kernels and the integer score dataflow are independent of power. We capture the actual binary32 factor
//! state on EVERY execution, including replay drift, and retain the exact previous note expression for undo.
//!
//! Evaluation does not assume monotonicity. An optional certificate proves nondecreasing score only on an
//! explicit power interval after exact integer-expression cancellation and numeric range checks. Signed score
//! sums, range differences, wrapping and positive-infinity conversion otherwise retain their native behavior.
//! Fine-bound classes are not program identities.

use super::{LiveModel, LivePlay};
use crate::error::Error;
use crate::live::random::LiveRandom;
use crate::live::score::{COMBO, GekisouComboInfo, LiveScoreCalculator};
use crate::num::{floor_to_i32, min_ignoring_nan, trunc_to_i32};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub(super) type ValueId = usize;

/// Exact terminal-score dataflow for one fixed complete performer program, chart, play, clock and random state.
/// Construct through [`LiveModel::compile_score_program`]. Only initial total power can be varied; this object
/// supplies no assertion that a different deck has the same program and is not an upper bound.
#[derive(Clone, Debug)]
pub struct ScoreProgram {
    nodes: Vec<Node>,
    result: ValueId,
    origin_power: i32,
    origin_score: i32,
}

impl ScoreProgram {
    /// Evaluate with native binary32 grouping, conversions and wrapping integer arithmetic.
    pub fn evaluate(&self, total_power: i32) -> i32 {
        let mut values = Vec::<i32>::with_capacity(self.nodes.len());
        self.evaluate_into(total_power, &mut values)
    }

    /// Exact evaluation with reusable scratch storage; a cache can evaluate all orders without 120 allocations.
    pub fn evaluate_into(&self, total_power: i32, values: &mut Vec<i32>) -> i32 {
        values.clear();
        for node in &self.nodes {
            let value = match *node {
                Node::Literal(v) => v,
                Node::Note(ref kernel) => kernel.evaluate(total_power),
                Node::Add(a, b) => values[a].wrapping_add(values[b]),
                Node::Sub(a, b) => values[a].wrapping_sub(values[b]),
                Node::RankPercent(input, pct) => ((i128::from(values[input]) * i128::from(pct)) / 100) as i32,
            };
            values.push(value);
        }
        values[self.result]
    }

    pub fn origin_power(&self) -> i32 {
        self.origin_power
    }
    pub fn origin_score(&self) -> i32 {
        self.origin_score
    }
    /// Number of live dataflow nodes after removing unreachable intermediate scores.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Resident owned bytes, including the actual allocated Node capacity (Node contains no heap pointers).
    pub fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.nodes.capacity() * std::mem::size_of::<Node>()
    }

    /// Exact normalization in the ring of native wrapping i32 sums. Each rank-percent input is normalized
    /// separately, then kept as an opaque atom: no integer sum crosses a signed division/truncation boundary.
    /// Note invocations are merged only when every integer field and every binary32 bit pattern agrees.
    /// Coefficients are implemented with wrapping Add/Sub and doubling, never floating multiplication.
    pub fn normalized_additive(&self) -> Self {
        let mut scratch = vec![0u32; self.nodes.len()];
        let final_expression = LinearExpression::read(&self.nodes, self.result, &mut scratch);
        let mut pending = final_expression.ranks(&self.nodes);
        let mut ranks = BTreeMap::new();
        while let Some(rank) = pending.pop() {
            if ranks.contains_key(&rank) {
                continue;
            }
            let Node::RankPercent(input, _) = self.nodes[rank] else { unreachable!("rank atom") };
            let expression = LinearExpression::read(&self.nodes, input, &mut scratch);
            pending.extend(expression.ranks(&self.nodes));
            ranks.insert(rank, expression);
        }
        let mut output = NormalizedBuilder {
            source: &self.nodes,
            nodes: Vec::new(),
            notes: HashMap::new(),
            ranks: HashMap::new(),
            literals: HashMap::new(),
        };
        // Original IDs are topological, so every referenced rank has already been emitted.
        for (old, expression) in ranks {
            let input = output.expression(expression);
            let Node::RankPercent(_, percent) = self.nodes[old] else { unreachable!("rank atom") };
            let value = output.push(Node::RankPercent(input, percent));
            output.ranks.insert(old, value);
        }
        let result = output.expression(final_expression);
        output.nodes.shrink_to_fit();
        Self { nodes: output.nodes, result, origin_power: self.origin_power, origin_score: self.origin_score }
    }

    /// Prove that this exact program's terminal score is nondecreasing at every integer power in `lo..=hi`.
    /// On success, return its exact endpoint scores. Failure means unproved, not decreasing. This certificate
    /// does not identify equivalent decks, certify PT settlement, or allow dropping canonical Top-K ties.
    ///
    /// Add/Sub are identities modulo 2^32, so they can be flattened even when intermediate score sums wrap.
    /// Each surviving Note or RankPercent is kept as an opaque atom. Nonnegative atom coefficients and a
    /// nonnegative constant yield a monotone ordinary sum; checking its endpoints in 0..=i32::MAX establishes
    /// that this sum equals the signed result of the modular expression throughout the interval. Each ranking
    /// atom separately requires this proof for its input before its nonnegative percentage is applied.
    pub fn certify_nondecreasing(&self, lo: i32, hi: i32) -> Option<(i32, i32)> {
        if lo > hi {
            return None;
        }
        // Nodes are topological: every RankPercent input references only earlier atoms. Memoize each atom once,
        // and flatten each rank input and the terminal root once. No per-score-node map copies or recursion.
        let mut atoms = vec![None; self.nodes.len()];
        for (id, node) in self.nodes.iter().enumerate() {
            atoms[id] = match node {
                Node::Note(kernel) => kernel.certify_nondecreasing(lo, hi),
                Node::RankPercent(input, pct) if *pct >= 0 => {
                    self.nonnegative_sum(*input, &atoms).and_then(|(a, b)| {
                        let low = i128::from(a).checked_mul(i128::from(*pct))? / 100;
                        let high = i128::from(b).checked_mul(i128::from(*pct))? / 100;
                        Some((i32::try_from(low).ok()?, i32::try_from(high).ok()?))
                    })
                }
                _ => None,
            };
        }
        let endpoints = self.nonnegative_sum(self.result, &atoms)?;
        // Also verify the normalized endpoint values against the exact evaluator. The interval proof above,
        // rather than these two observations, is what establishes all intermediate powers.
        (endpoints == (self.evaluate(lo), self.evaluate(hi))).then_some(endpoints)
    }

    fn nonnegative_sum(&self, root: ValueId, atoms: &[Option<(i32, i32)>]) -> Option<(i32, i32)> {
        let mut coefficients = vec![0i128; root + 1];
        coefficients[root] = 1;
        let (mut constant, mut low, mut high) = (0i128, 0i128, 0i128);
        for id in (0..=root).rev() {
            let coefficient = coefficients[id];
            if coefficient == 0 {
                continue;
            }
            match self.nodes[id] {
                Node::Add(a, b) => {
                    coefficients[a] = coefficients[a].checked_add(coefficient)?;
                    coefficients[b] = coefficients[b].checked_add(coefficient)?;
                }
                Node::Sub(a, b) => {
                    coefficients[a] = coefficients[a].checked_add(coefficient)?;
                    coefficients[b] = coefficients[b].checked_sub(coefficient)?;
                }
                Node::Literal(value) => {
                    constant = constant.checked_add(coefficient.checked_mul(i128::from(value))?)?;
                }
                Node::Note(_) | Node::RankPercent(_, _) => {
                    if coefficient < 0 {
                        return None;
                    }
                    let (a, b) = atoms[id]?;
                    low = low.checked_add(coefficient.checked_mul(i128::from(a))?)?;
                    high = high.checked_add(coefficient.checked_mul(i128::from(b))?)?;
                }
            }
        }
        if constant < 0 {
            return None;
        }
        low = low.checked_add(constant)?;
        high = high.checked_add(constant)?;
        if low < 0 || high < low {
            return None;
        }
        Some((i32::try_from(low).ok()?, i32::try_from(high).ok()?))
    }
}

struct LinearExpression {
    constant: i32,
    atoms: Vec<(ValueId, u32)>,
}

impl LinearExpression {
    fn read(nodes: &[Node], root: ValueId, coefficients: &mut [u32]) -> Self {
        coefficients.fill(0);
        coefficients[root] = 1;
        let mut expression = Self { constant: 0, atoms: Vec::new() };
        for id in (0..=root).rev() {
            let coefficient = coefficients[id];
            if coefficient == 0 {
                continue;
            }
            match nodes[id] {
                Node::Add(a, b) => {
                    coefficients[a] = coefficients[a].wrapping_add(coefficient);
                    coefficients[b] = coefficients[b].wrapping_add(coefficient);
                }
                Node::Sub(a, b) => {
                    coefficients[a] = coefficients[a].wrapping_add(coefficient);
                    coefficients[b] = coefficients[b].wrapping_sub(coefficient);
                }
                Node::Literal(value) => {
                    expression.constant = expression.constant.wrapping_add(value.wrapping_mul(coefficient as i32));
                }
                Node::Note(_) | Node::RankPercent(_, _) => expression.atoms.push((id, coefficient)),
            }
        }
        expression
    }

    fn ranks(&self, nodes: &[Node]) -> Vec<ValueId> {
        self.atoms.iter().filter_map(|&(id, _)| matches!(nodes[id], Node::RankPercent(_, _)).then_some(id)).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AtomKey {
    Note([u32; 12]),
    Rank(ValueId),
}

struct NormalizedBuilder<'a> {
    source: &'a [Node],
    nodes: Vec<Node>,
    notes: HashMap<[u32; 12], ValueId>,
    ranks: HashMap<ValueId, ValueId>,
    literals: HashMap<i32, ValueId>,
}

impl NormalizedBuilder<'_> {
    fn push(&mut self, node: Node) -> ValueId {
        let id = self.nodes.len();
        self.nodes.push(node);
        id
    }

    fn literal(&mut self, value: i32) -> ValueId {
        if let Some(&id) = self.literals.get(&value) {
            return id;
        }
        let id = self.push(Node::Literal(value));
        self.literals.insert(value, id);
        id
    }

    fn weighted(&mut self, atom: ValueId, mut coefficient: u32) -> ValueId {
        debug_assert_ne!(coefficient, 0);
        let mut multiple = atom;
        let mut sum = None;
        loop {
            if coefficient & 1 != 0 {
                sum = Some(match sum {
                    None => multiple,
                    Some(sum) => self.push(Node::Add(sum, multiple)),
                });
            }
            coefficient >>= 1;
            if coefficient == 0 {
                return sum.expect("positive coefficient");
            }
            multiple = self.push(Node::Add(multiple, multiple));
        }
    }

    fn expression(&mut self, expression: LinearExpression) -> ValueId {
        // Combine equal note bit patterns before emitting anything, so canceled duplicate notes disappear too.
        let mut coefficients = BTreeMap::<AtomKey, (u32, ValueId)>::new();
        for (id, coefficient) in expression.atoms {
            let key = match &self.source[id] {
                Node::Note(kernel) => AtomKey::Note(kernel.bits()),
                Node::RankPercent(_, _) => AtomKey::Rank(id),
                _ => unreachable!("linear atom"),
            };
            let value = coefficients.entry(key).or_insert((0, id));
            value.0 = value.0.wrapping_add(coefficient);
        }
        let mut sum = (expression.constant != 0).then(|| self.literal(expression.constant));
        for (key, (coefficient, original)) in coefficients {
            if coefficient == 0 {
                continue;
            }
            let atom = match key {
                AtomKey::Rank(id) => *self.ranks.get(&id).expect("earlier rank atom"),
                AtomKey::Note(bits) => {
                    if let Some(&id) = self.notes.get(&bits) {
                        id
                    } else {
                        let id = self.push(self.source[original].clone());
                        self.notes.insert(bits, id);
                        id
                    }
                }
            };
            let signed = coefficient as i32;
            let term = self.weighted(atom, signed.unsigned_abs());
            sum = Some(match (sum, signed > 0) {
                (None, true) => term,
                (Some(sum), true) => self.push(Node::Add(sum, term)),
                (sum, false) => {
                    let sum = sum.unwrap_or_else(|| self.literal(0));
                    self.push(Node::Sub(sum, term))
                }
            });
        }
        sum.unwrap_or_else(|| self.literal(0))
    }
}

impl LiveModel {
    /// Start an exact power-parameterized recording on a fresh native model. Clones share immutable recorded
    /// prefixes; only completed OrderedLive visits may be inserted into a complete-order cache.
    pub fn begin_score_program_recording(&mut self) -> Result<(), Error> {
        if self.program_has_started {
            return Err(Error::Input("score program requires a fresh LiveModel".into()));
        }
        if self.raw_runtime.is_some()
            || self.raw_pending.is_some()
            || self.is_live_finished
            || self.prev_confirmed_rank.is_some()
            || self.frame_rank_confirmation.is_some()
        {
            return Err(Error::Unsupported(
                "score program requires a declared judged-stream run without raw controls".into(),
            ));
        }
        self.score.begin_program()
    }

    /// Export the current exact recorded score expression. This preserves the original model and memoizes the
    /// compact program, so completed equivalent orders share one allocation. None means recording was disabled.
    pub fn recorded_score_program(&self) -> Result<Option<Arc<ScoreProgram>>, Error> {
        self.score.export_program()
    }

    /// Record one complete declared run of a fresh model and return its exact power-parameterized score program
    /// and the normally executed terminal model. `random` is the complete post-shuffle state, exactly as in
    /// [`LiveModel::run_with_random`]; roots with the same member order can still have different skill/luck draws.
    ///
    /// Recording accepts judged-stream ordinary Live, native solo Gekisou and declared external rank timelines.
    /// Raw callbacks and already-started lifecycle state are rejected. The returned model is the origin
    /// execution only; its score snapshots must not be treated as belonging to another evaluated power.
    pub fn compile_score_program(
        mut self,
        play: &LivePlay,
        delta_times: &[f32],
        random: LiveRandom,
    ) -> Result<(ScoreProgram, Self), Error> {
        self.begin_score_program_recording()?;
        self.run_with_random(play, delta_times, random)?;
        let program = self.score.finish_program()?;
        Ok((program, self))
    }
}

#[derive(Clone, Debug)]
enum Node {
    Literal(i32),
    Note(Kernel),
    Add(ValueId, ValueId),
    Sub(ValueId, ValueId),
    RankPercent(ValueId, i64),
}

/// A note invocation after all control flow and factor-state operations have completed. Floating factors are
/// copied without reassociation or quantization. `power_delta` is modular because factor commands wrap i32.
#[derive(Clone, Debug)]
pub(super) struct Kernel {
    power_delta: i32,
    adjustment: f32,
    difficulty: f32,
    note_pct: i32,
    judge_pct: i32,
    combo: f32,
    score_up: f32,
    luck: f32,
    count: i32,
    assist: f32,
    life: f32,
    event: f32,
}

impl Kernel {
    fn bits(&self) -> [u32; 12] {
        // Exhaustive destructuring makes a newly added kernel field a compile error until its identity is added.
        let Self {
            power_delta,
            adjustment,
            difficulty,
            note_pct,
            judge_pct,
            combo,
            score_up,
            luck,
            count,
            assist,
            life,
            event,
        } = self;
        [
            *power_delta as u32,
            adjustment.to_bits(),
            difficulty.to_bits(),
            *note_pct as u32,
            *judge_pct as u32,
            combo.to_bits(),
            score_up.to_bits(),
            luck.to_bits(),
            *count as u32,
            assist.to_bits(),
            life.to_bits(),
            event.to_bits(),
        ]
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn capture(
        calc: &LiveScoreCalculator,
        combo_count: i32,
        current_life: i32,
        time_ms: i32,
        note_type: i32,
        score_type: i32,
        gekisou: Option<&dyn GekisouComboInfo>,
        origin_power: i32,
    ) -> Result<Self, Error> {
        let cum = match &calc.combo_table {
            Some(table) => table.get_cumulative_factor(COMBO, combo_count)?,
            None => 0.0,
        };
        let gk = calc.gekisou_combo_bonus_factor(gekisou, time_ms)?;
        let (score_up, luck) = calc.score_up_and_luck(score_type, time_ms);
        Ok(Self {
            power_delta: calc.state.band_total_power.wrapping_sub(origin_power),
            adjustment: calc.score_adjustment_factor,
            difficulty: calc.music_difficulty_factor,
            note_pct: *calc
                .note_factor_percent
                .get(&note_type)
                .ok_or_else(|| Error::Game(format!("note type {note_type} has no score percent")))?,
            judge_pct: *calc
                .judgement_score_factor_percent
                .get(&score_type)
                .ok_or_else(|| Error::Game(format!("score type {score_type} has no score percent")))?,
            combo: gk * (calc.state.combo_score_up + (min_ignoring_nan(cum, 1f32) + 1f32)),
            score_up,
            luck,
            count: calc.converted_note_count,
            assist: calc.assist_factor,
            life: if current_life > 0 { 1f32 } else { calc.life_onus_factor },
            event: calc.event_bonus_factor,
        })
    }

    fn evaluate(&self, power: i32) -> i32 {
        // Keep this sequence identical to LiveScoreCalculator::note_score_core, including both floors.
        let t = (self.adjustment * power.wrapping_add(self.power_delta) as f32) * self.difficulty;
        let a = (self.note_pct as f32 / 100f32) * t;
        let b = (self.judge_pct as f32 / 100f32) * a;
        let c = (b * self.combo) * self.score_up;
        let d = self.luck * c;
        let x = d / self.count as f32;
        let f = x.floor();
        let y = if f == f32::INFINITY { -2147483648f32 } else { trunc_to_i32(f) as f32 };
        let z = self.assist * (self.life * (y * self.event));
        floor_to_i32(z)
    }

    fn certify_nondecreasing(&self, lo: i32, hi: i32) -> Option<(i32, i32)> {
        if self.count <= 0
            || self.note_pct < 0
            || self.judge_pct < 0
            || [
                self.adjustment,
                self.difficulty,
                self.combo,
                self.score_up,
                self.luck,
                self.assist,
                self.life,
                self.event,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return None;
        }
        // The captured power delta is modular. Reject an interval crossing its signed addition boundary;
        // restrict to nonnegative effective power so every float intermediate is a nonnegative monotone map.
        let low_power = lo.checked_add(self.power_delta)?;
        let high_power = hi.checked_add(self.power_delta)?;
        if low_power < 0 || high_power < low_power {
            return None;
        }
        let endpoint = |power: i32| {
            // Same parenthesization as evaluate, exposing all intermediates for finite/nonnegative checks.
            let t0 = self.adjustment * power as f32;
            let t = t0 * self.difficulty;
            let a = (self.note_pct as f32 / 100f32) * t;
            let b = (self.judge_pct as f32 / 100f32) * a;
            let c0 = b * self.combo;
            let c = c0 * self.score_up;
            let d = self.luck * c;
            let x = d / self.count as f32;
            // x is required finite; consequently floor_as_float's special +infinity branch is unreachable.
            let y = trunc_to_i32(x.floor()) as f32;
            let z0 = y * self.event;
            let z1 = self.life * z0;
            let z = self.assist * z1;
            if [t0, t, a, b, c0, c, d, x, y, z0, z1, z].iter().any(|v| !v.is_finite() || *v < 0.0) {
                return None;
            }
            Some(floor_to_i32(z))
        };
        let low = endpoint(low_power)?;
        let high = endpoint(high_power)?;
        // Rounding to binary32, finite saturation, both floors and multiplication/division by the admitted
        // constants are monotone. Endpoint finiteness bounds every intermediate at all powers in between.
        (low >= 0 && high >= low).then_some((low, high))
    }
}

#[derive(Clone, Debug)]
struct NodeChunk {
    previous: Option<Arc<NodeChunk>>,
    nodes: Box<[Node]>,
}

#[derive(Debug, Default)]
struct NodeStorage {
    head: Option<Arc<NodeChunk>>,
    tail: Vec<Node>,
    len: usize,
}

/// Model clones seal only their current tail and share immutable prefix chunks. Subsequent branches append
/// to their own tails, avoiding a full history copy on the first write after each OrderedLive prefix clone.
#[derive(Debug, Default)]
struct NodeLog(RefCell<NodeStorage>);

impl Clone for NodeLog {
    fn clone(&self) -> Self {
        let mut storage = self.0.borrow_mut();
        if !storage.tail.is_empty() {
            storage.head = Some(Arc::new(NodeChunk {
                previous: storage.head.take(),
                nodes: std::mem::take(&mut storage.tail).into_boxed_slice(),
            }));
        }
        Self(RefCell::new(NodeStorage { head: storage.head.clone(), tail: Vec::new(), len: storage.len }))
    }
}

impl NodeLog {
    fn push(&mut self, node: Node) -> ValueId {
        let storage = self.0.get_mut();
        let id = storage.len;
        storage.tail.push(node);
        storage.len += 1;
        id
    }

    fn into_nodes(self) -> Vec<Node> {
        let storage = self.0.into_inner();
        let mut chunks = Vec::new();
        let mut head = storage.head;
        while let Some(chunk) = head {
            head = chunk.previous.clone();
            chunks.push(chunk);
        }
        let mut nodes = Vec::with_capacity(storage.len);
        for chunk in chunks.into_iter().rev() {
            nodes.extend(chunk.nodes.iter().cloned());
        }
        nodes.extend(storage.tail);
        nodes
    }
}

#[derive(Clone, Debug)]
pub(super) struct Recorder {
    nodes: NodeLog,
    score: ValueId,
    origin_power: i32,
    notes: Vec<Vec<Option<ValueId>>>,
    fixed: Vec<(i32, ValueId)>,
    pending: Option<ValueId>,
    finished: OnceCell<Arc<ScoreProgram>>,
}

impl Recorder {
    pub(super) fn new(origin_power: i32, frames: usize) -> Self {
        let mut nodes = NodeLog::default();
        nodes.push(Node::Literal(0));
        Self {
            nodes,
            score: 0,
            origin_power,
            notes: vec![Vec::new(); frames],
            fixed: Vec::new(),
            pending: None,
            finished: OnceCell::new(),
        }
    }
    pub(super) fn origin_power(&self) -> i32 {
        self.origin_power
    }
    pub(super) fn snapshot(&self) -> ValueId {
        self.score
    }
    fn push(&mut self, node: Node) -> ValueId {
        self.finished.take();
        self.nodes.push(node)
    }
    pub(super) fn add_note(&mut self, frame: usize) {
        self.notes[frame].push(None);
    }
    pub(super) fn execute_note(&mut self, frame: usize, index: usize, kernel: Kernel) {
        let note = self.push(Node::Note(kernel));
        self.notes[frame][index] = Some(note);
        self.score = self.push(Node::Add(self.score, note));
    }
    pub(super) fn undo(&mut self, frame: usize) {
        for index in 0..self.notes[frame].len() {
            if let Some(note) = self.notes[frame][index] {
                self.score = self.push(Node::Sub(self.score, note));
            }
        }
        if let Some((_, value)) = self.fixed.iter().find(|x| x.0 == frame as i32) {
            self.score = self.push(Node::Sub(self.score, *value));
        }
    }
    pub(super) fn pending_literal(&mut self, value: i32) {
        self.pending = Some(self.push(Node::Literal(value)));
    }
    pub(super) fn pending_rank(&mut self, start: Option<ValueId>, end: Option<ValueId>, pct: i64) -> Result<(), Error> {
        let (Some(start), Some(end)) = (start, end) else {
            return Err(Error::Game("score program is missing rank snapshots".into()));
        };
        if self.pending.is_none() {
            return Err(Error::Game("score program is missing pending fixed score".into()));
        }
        let range = self.push(Node::Sub(end, start));
        self.pending = Some(self.push(Node::RankPercent(range, pct)));
        Ok(())
    }
    pub(super) fn file_fixed(&mut self, frame: i32) -> Result<(), Error> {
        let value = self.pending.take().ok_or_else(|| Error::Game("score program is missing fixed score".into()))?;
        self.fixed.push((frame, value));
        self.score = self.push(Node::Add(self.score, value));
        Ok(())
    }
    pub(super) fn execute_fixed(&mut self, frame: i32) {
        if let Some((_, value)) = self.fixed.iter().find(|x| x.0 == frame) {
            self.score = self.push(Node::Add(self.score, *value));
        }
    }
    pub(super) fn finish(self, origin_score: i32) -> Result<ScoreProgram, Error> {
        // The modular normalizer removes both unreachable nodes and replay contributions that are reachable
        // but cancel exactly. Work on the original log directly; no intermediate full-size compact clone.
        let source = ScoreProgram {
            nodes: self.nodes.into_nodes(),
            result: self.score,
            origin_power: self.origin_power,
            origin_score,
        };
        let program = source.normalized_additive();
        if program.evaluate(self.origin_power) != origin_score {
            return Err(Error::Game("score program differs from its recorded execution".into()));
        }
        Ok(program)
    }

    pub(super) fn export(&self, origin_score: i32) -> Result<Arc<ScoreProgram>, Error> {
        if let Some(program) = self.finished.get() {
            return Ok(program.clone());
        }
        let program = Arc::new(self.clone().finish(origin_score)?);
        let _ = self.finished.set(program.clone());
        Ok(program)
    }
}

#[cfg(test)]
mod tests {
    use super::super::combo::ComboCounter;
    use super::super::scorecalc::{IncrementalCalculator, NoteCommand};
    use super::super::{GekisouSetup, JudgedNote, LiveNote, LiveParams, Performer, PlayFrame};
    use super::*;
    use crate::live::score::{JUST, LiveScoreSettings};
    use crate::live::skill::FactorCommand;
    use crate::master::Master;
    use serde_json::json;

    fn calculator(power: i32) -> LiveScoreCalculator {
        LiveScoreCalculator::new(
            power,
            27,
            7,
            &LiveScoreSettings {
                score_adjustment_factor: 3.0,
                life_onus_factor: 0.5,
                note_factor_percent: [(1, 113)].into(),
                judgement_score_factor_percent: [(JUST, 197)].into(),
            },
            1.17,
            0.73,
            None,
        )
    }

    fn replay(power: i32, record: bool) -> (i32, Option<ScoreProgram>) {
        let mut score = IncrementalCalculator::new(calculator(power), 1000);
        if record {
            score.begin_program().unwrap();
        }
        let mut combo = ComboCounter::new(4);
        for (i, t) in [100, 200, 300, 400].into_iter().enumerate() {
            combo.add_judgement(t, 6).unwrap();
            score.add_note(NoteCommand::new(t, if i == 3 { 0 } else { 1000 }, i as i32, 1, JUST));
        }
        score.calculate(400, &combo, None).unwrap();
        // Backdated commands force undo and replay; preserve the actual f32 state after each undo.
        for (t, delta, offset) in [(80, 55_551, 0), (160, -33_333, 17), (200, 12_345, i32::MAX)] {
            score.add_factor(FactorCommand {
                time_ms: t,
                owner_id: 1,
                note_mill: delta,
                band_total_power: offset,
                ..Default::default()
            });
            score.calculate(400, &combo, None).unwrap();
        }
        let s0 = score.calculate(120, &combo, None).unwrap();
        let start = score.program_snapshot();
        let s1 = score.calculate(320, &combo, None).unwrap();
        let end = score.program_snapshot();
        let rank = (i128::from(s1.wrapping_sub(s0)) * 77 / 100) as i32;
        score.add_fixed(320, rank);
        score.record_rank_bonus(start, end, 77).unwrap();
        score.calculate(400, &combo, None).unwrap();
        // The fixed bonus still refers to the old range expressions after later replay changes note factors.
        score.add_factor(FactorCommand { time_ms: 180, owner_id: 2, note_mill: 77_777, ..Default::default() });
        score.calculate(400, &combo, None).unwrap();
        let value = score.score;
        let program = if record { Some(score.finish_program().unwrap()) } else { None };
        (value, program)
    }

    #[test]
    fn replayed_notes_and_fixed_bonuses_retain_their_original_expressions() {
        let (origin, Some(program)) = replay(123_457, true) else { panic!("recorded program") };
        assert_eq!(program.origin_score(), origin);
        assert!(program.nodes.iter().any(|n| matches!(n, Node::RankPercent(_, 77))));
        for power in [i32::MIN, -123_456, -1, 0, 1, 99_999, 123_457, 16_777_217, i32::MAX] {
            assert_eq!(program.evaluate(power), replay(power, false).0, "power {power}");
            assert_eq!(
                program.normalized_additive().evaluate(power),
                replay(power, false).0,
                "normalized power {power}"
            );
        }
    }

    #[test]
    fn normalization_cancels_replay_and_keeps_signed_rank_boundaries() {
        let mut nodes = vec![Node::Note(identity_kernel()), Node::Literal(i32::MAX)];
        let mut score = 0;
        // Native replay histories can contain an arbitrarily long chain of canceled old note contributions.
        for _ in 0..4000 {
            nodes.push(Node::Add(score, 0));
            nodes.push(Node::Sub(nodes.len() - 1, 0));
            score = nodes.len() - 1;
        }
        nodes.push(Node::Add(score, 1));
        let input = nodes.len() - 1;
        nodes.push(Node::RankPercent(input, -77));
        nodes.push(Node::Sub(input, nodes.len() - 1));
        let program = test_program(nodes);
        let normalized = program.normalized_additive();
        assert!(normalized.node_count() < 10, "{}", normalized.node_count());
        for power in [i32::MIN, i32::MIN + 1, -100_003, -1, 0, 1, 317, 16_777_217, i32::MAX] {
            assert_eq!(normalized.evaluate(power), program.evaluate(power), "power {power}");
        }
        // Flattening through the opaque rank node would evaluate a different signed input after wrap.
        assert!(normalized.nodes.iter().any(|node| matches!(node, Node::RankPercent(_, -77))));
    }

    #[test]
    fn normalization_uses_wrapping_coefficients_and_exact_kernel_bits() {
        let mut nodes = vec![Node::Note(identity_kernel())];
        for _ in 0..31 {
            let last = nodes.len() - 1;
            nodes.push(Node::Add(last, last));
        }
        let min_coefficient = test_program(nodes.clone());
        let last = nodes.len() - 1;
        nodes.push(Node::Add(last, last)); // 2^32 copies cancel modulo 2^32.
        let zero = test_program(nodes).normalized_additive();
        assert_eq!(zero.node_count(), 1);
        for power in [i32::MIN, -17, -1, 0, 1, 319, i32::MAX] {
            assert_eq!(min_coefficient.normalized_additive().evaluate(power), min_coefficient.evaluate(power));
            assert_eq!(zero.evaluate(power), 0);
        }
        let mut positive_zero = identity_kernel();
        positive_zero.score_up = 0.0;
        let mut negative_zero = positive_zero.clone();
        negative_zero.score_up = -0.0;
        assert_ne!(positive_zero.bits(), negative_zero.bits());
        let duplicate =
            test_program(vec![Node::Note(identity_kernel()), Node::Note(identity_kernel()), Node::Sub(0, 1)])
                .normalized_additive();
        assert_eq!(duplicate.node_count(), 1);
        let distinct = test_program(vec![Node::Note(positive_zero), Node::Note(negative_zero), Node::Add(0, 1)])
            .normalized_additive();
        assert_eq!(distinct.nodes.iter().filter(|node| matches!(node, Node::Note(_))).count(), 2);
    }

    #[test]
    fn kernels_preserve_cast_saturation_infinity_and_nan_semantics() {
        for adjustment in [3.0, f32::MAX, f32::INFINITY, f32::NAN] {
            let mut calc = calculator(231);
            calc.score_adjustment_factor = adjustment;
            calc.state.note_score_up = 1.234_567;
            let kernel = Kernel::capture(&calc, 7, 0, 100, 1, JUST, None, 231).unwrap();
            for power in [i32::MIN, -1, 0, 1, 231, 16_777_217, i32::MAX] {
                calc.state.band_total_power = power;
                assert_eq!(kernel.evaluate(power), calc.note_score(7, 0, 100, 1, JUST, None).unwrap());
            }
        }
    }

    fn identity_kernel() -> Kernel {
        Kernel {
            power_delta: 0,
            adjustment: 1.0,
            difficulty: 1.0,
            note_pct: 100,
            judge_pct: 100,
            combo: 1.0,
            score_up: 1.0,
            luck: 1.0,
            count: 1,
            assist: 1.0,
            life: 1.0,
            event: 1.0,
        }
    }

    fn test_program(nodes: Vec<Node>) -> ScoreProgram {
        ScoreProgram { result: nodes.len() - 1, nodes, origin_power: 0, origin_score: 0 }
    }

    #[test]
    fn certificate_cancels_wrapped_intermediate_sums_and_keeps_rank_atoms() {
        let program = test_program(vec![
            Node::Note(identity_kernel()), // P
            Node::Literal(i32::MAX),
            Node::Add(0, 1),          // wraps for P > 0
            Node::Sub(2, 1),          // exact P again
            Node::RankPercent(3, 77), // floor(77P/100)
            Node::Add(3, 4),
        ]);
        assert_eq!(program.certify_nondecreasing(0, 10_000), Some((0, 17_700)));
        let mut previous = program.evaluate(0);
        for power in 1..=10_000 {
            let score = program.evaluate(power);
            assert_eq!(score, power + (i64::from(power) * 77 / 100) as i32);
            assert!(score >= previous);
            previous = score;
        }
        // A canceled atom need not itself be monotone or finite: identical stored integer values cancel.
        let mut invalid = identity_kernel();
        invalid.adjustment = f32::INFINITY;
        let cancelled = test_program(vec![Node::Note(invalid), Node::Sub(0, 0)]);
        assert_eq!(cancelled.certify_nondecreasing(0, 100), Some((0, 0)));
    }

    #[test]
    fn certificate_rejects_unproved_signed_wrap_and_nonfinite_cases() {
        let negative = test_program(vec![Node::Note(identity_kernel()), Node::Literal(100), Node::Sub(1, 0)]);
        assert_eq!(negative.certify_nondecreasing(0, 100), None);
        // Two individually increasing snapshots do not make their difference increasing.
        let mut faster = identity_kernel();
        faster.score_up = 1.625;
        let mut slower = identity_kernel();
        slower.score_up = 1.5;
        let difference = test_program(vec![Node::Note(faster), Node::Note(slower), Node::Sub(0, 1)]);
        assert_eq!((difference.evaluate(5), difference.evaluate(6)), (1, 0));
        assert_eq!(difference.certify_nondecreasing(0, 100), None);
        let negative_constant = test_program(vec![Node::Note(identity_kernel()), Node::Literal(-1), Node::Add(0, 1)]);
        assert_eq!(negative_constant.certify_nondecreasing(1, 100), None);
        let negative_rank = test_program(vec![Node::Note(identity_kernel()), Node::RankPercent(0, -10)]);
        assert_eq!(negative_rank.certify_nondecreasing(0, 100), None);
        let wrapped_rank = test_program(vec![Node::Note(identity_kernel()), Node::RankPercent(0, 200)]);
        assert_eq!(wrapped_rank.certify_nondecreasing(0, i32::MAX), None);
        let wrapped_sum = test_program(vec![Node::Note(identity_kernel()), Node::Add(0, 0)]);
        assert_eq!(wrapped_sum.certify_nondecreasing(0, i32::MAX), None);
        let mut offset = identity_kernel();
        offset.power_delta = i32::MAX;
        assert_eq!(test_program(vec![Node::Note(offset)]).certify_nondecreasing(0, 2), None);
        for adjustment in [-1.0, f32::INFINITY, f32::NAN, f32::MAX] {
            let mut kernel = identity_kernel();
            kernel.adjustment = adjustment;
            assert_eq!(test_program(vec![Node::Note(kernel)]).certify_nondecreasing(0, 100), None);
        }
        let mut zero_count = identity_kernel();
        zero_count.count = 0;
        assert_eq!(test_program(vec![Node::Note(zero_count)]).certify_nondecreasing(0, 100), None);
        assert_eq!(test_program(vec![Node::Note(identity_kernel())]).certify_nondecreasing(2, 1), None);
        // Exact coefficient arithmetic is bounded; even a mathematically zero DAG is declined on overflow.
        let mut huge = vec![Node::Literal(0)];
        for _ in 0..128 {
            let prior = huge.len() - 1;
            huge.push(Node::Add(prior, prior));
        }
        assert_eq!(test_program(huge).certify_nondecreasing(0, 100), None);
    }

    #[test]
    fn certificate_accepts_finite_saturation_and_nonwrapping_power_offsets() {
        let mut kernel = identity_kernel();
        kernel.score_up = 100.0;
        let saturated = test_program(vec![Node::Note(kernel)]);
        assert_eq!(saturated.certify_nondecreasing(0, i32::MAX), Some((0, i32::MAX)));
        let mut kernel = identity_kernel();
        kernel.power_delta = 7;
        let shifted = test_program(vec![Node::Note(kernel)]);
        assert_eq!(shifted.certify_nondecreasing(0, 100), Some((7, 107)));
        assert_eq!(shifted.certify_nondecreasing(42, 42), Some((49, 49)));
    }

    #[test]
    fn certificates_cover_recorded_snap_replay_and_all_solo_missions() {
        for mission in [0, 1, 2, 3] {
            let (model, play, dt) = declared_model(200_000, mission, false);
            let (program, _) = model.compile_score_program(&play, &dt, LiveRandom::new(7)).unwrap();
            // A complete run may retain no subtraction nodes. The direct replay test separately requires
            // actual undo and a frozen rank expression; here every mode must pass the certificate itself.
            let endpoints = program
                .certify_nondecreasing(0, 2_000_000)
                .unwrap_or_else(|| panic!("nonnegative fixture program was not certified: mission={mission}"));
            assert_eq!(endpoints, (program.evaluate(0), program.evaluate(2_000_000)), "mission={mission}");
            let mut previous = program.evaluate(0);
            for power in 1..=2_000 {
                let current = program.evaluate(power);
                assert!(current >= previous, "mission={mission} power={power}");
                previous = current;
            }
            for power in [0, 1, 39, 123_457, 1_999_999, 2_000_000] {
                let (mut model, play, dt) = declared_model(power, mission, false);
                assert_eq!(
                    program.evaluate(power),
                    model.run_with_random(&play, &dt, LiveRandom::new(7)).unwrap(),
                    "mission={mission} power={power}"
                );
            }
        }
    }

    fn master() -> Master {
        let lots: Vec<_> = (0..5).map(|k| json!({"_id":k+1,"_chanceLotType":k,"_lotResult":3,"_weight":1})).collect();
        let tables = json!({
            "MasterLiveSettings": [
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"},
                {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
                {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
                {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
            "MasterLiveNoteParameter": [{"_id":1,"_noteOperateType":1,"_scorePercent":113}],
            "MasterLiveJudgementParameter": [
                {"_id":1,"_noteSimulateJudgement":6,"_scorePercent":197,"_damage":0}],
            "MasterLiveJudgementTiming": [{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":100}],
            "MasterLiveComboScoreBonus": [
                {"_id":1,"_comboBonusType":0,"_requiredComboCount":1,"_bonusFactor":0.013},
                {"_id":2,"_comboBonusType":1,"_requiredComboCount":1,"_bonusFactor":0.017}],
            "MasterLiveGekisouRankingScoreBonus": [
                {"_id":1,"_missionPattern":1,"_count":1,"_rank":1,"_scoreBonusPercent":77}],
            "MasterLiveGekisouLuckBasePoint": [
                {"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":100}],
            "MasterLiveGekisouLuckBonusLot": lots,
            "MasterSkillTarget": [
                {"_id":46,"_skillTargetType":4,"_judgement":6},
                {"_id":57,"_skillTargetType":5,"_gekisouMissionType":3}],
            "MasterSkillCondition": [
                {"_id":61,"_conditionType":4010,"_conditionValues":[],"_isPositive":true,"_conditionTargetIDs":[]},
                {"_id":81,"_conditionType":7010,"_conditionValues":[],"_isPositive":true,"_conditionTargetIDs":[57]}],
            "MasterSkillConditionSet": [
                {"_id":10,"_group":53,"_conditionIds":[61]},
                {"_id":21,"_group":254,"_conditionIds":[81]}],
            "MasterSkillCumulativeCondition": [
                {"_id":21,"_skillCumulativeConditionType":1000,"_conditionValues":[1],
                 "_conditionTargetIDs":[46],"_maxCumulativeCount":100}],
            "MasterLiveSkillEffect": [{"_id":1,"_liveSkillID":1,"_level":1,
                "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
                "_skillEffectType":2000,"_activationTimeSecond":0.31,"_effectValue":5555,
                "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
                "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}],
            "MasterSupportSkillEffect": [{"_id":2,"_supportSkillID":2,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":53,
                "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
                "_skillEffectType":2000,"_activationTimeSecond":0.17,"_effectValue":3333,
                "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
                "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}],
            "MasterGekisouSkill": [{"_id":3,"_gekisouMissionType":3}],
            "MasterGekisouSkillEffect": [{"_id":3,"_gekisouSkillID":3,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":254,
                "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
                "_skillEffectType":13000,"_activationTimeSecond":0.0,"_effectValue":1,
                "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
                "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}],
            "MasterGekisouSupportSkill": [{"_id":4,"_gekisouMissionType":3}],
            "MasterGekisouSupportSkillEffect": [{"_id":4,"_gekisouSupportSkillID":4,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":254,
                "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
                "_skillEffectType":2001,"_activationTimeSecond":0.45,"_effectValue":100,
                "_maxEffectValue":1000,"_effectLimitCount":0,"_skillCumulativeConditionID":21,
                "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]
        });
        let texts: Vec<_> =
            tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
        Master::from_json_tables(|name| texts.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())).unwrap()
    }

    fn declared_model(power: i32, mission: i64, external: bool) -> (LiveModel, LivePlay, Vec<f32>) {
        let master = master();
        let notes: Vec<_> = (0..9)
            .map(|i| LiveNote { note_id: i, time_ms: 80 + i * 80, note_operate_type: 1, judgement_type: 1 })
            .collect();
        let params = LiveParams {
            total_power: power,
            music_level: 27,
            converted_note_count: 9,
            music_length_ms: 2000,
            score_music_length_ms: None,
            skill_target_music_type: 0,
            assist_factor: 0.73,
        };
        let deck = [Performer {
            live_skill: Some((1, 1)),
            support_skills: vec![(2, 1)],
            gekisou_skill: Some((3, 1)),
            gekisou_support_skills: vec![(4, 1)],
            ..Default::default()
        }];
        let setup = GekisouSetup { fevers: vec![(120, 600)], missions: vec![mission; 3] };
        let model = if external {
            LiveModel::new_gekisou_external(&master, &deck, &notes, &[(0, 150)], params, &setup)
        } else if mission != 0 {
            LiveModel::new_gekisou(&master, &deck, &notes, &[(0, 150)], params, &setup)
        } else {
            LiveModel::new(&master, &deck, &notes, &[(0, 150)], params)
        }
        .unwrap();
        let mut frames: Vec<_> = (0..=125).map(|i| PlayFrame { time_ms: i * 16, judged: Vec::new() }).collect();
        for n in &notes {
            // Judgements arrive after their score frame, and the live-skill expiration causes another replay.
            let frame = frames.iter_mut().find(|f| f.time_ms >= n.time_ms + 32).unwrap();
            frame.judged.push(JudgedNote { note_id: n.note_id, judgement: 6, judgement_time_ms: n.time_ms + 32 });
        }
        let dt = vec![0.016; frames.len()];
        (model, LivePlay { frames, base_seed: 7 }, dt)
    }

    #[test]
    fn declared_live_and_solo_gekisou_programs_match_fresh_runs_at_other_powers() {
        for mission in [0, 1, 2, 3] {
            let (model, play, dt) = declared_model(200_000, mission, false);
            let (program, terminal) = model.compile_score_program(&play, &dt, LiveRandom::new(7)).unwrap();
            if mission != 0 {
                assert!(terminal.gk.as_ref().unwrap().rank_bonus.iter().any(|r| r.2 > 0));
                assert!(program.nodes.iter().any(|n| matches!(n, Node::RankPercent(_, 77))));
            }
            if mission == 2 {
                assert!(terminal.gekisou_ranges()[0].lot_results.iter().sum::<i32>() > 0);
            }
            for power in [i32::MIN, -123_457, -1, 0, 1, 123_457, 200_000, 1_000_003, 16_777_217, i32::MAX] {
                let (mut model, play, dt) = declared_model(power, mission, false);
                assert_eq!(
                    program.evaluate(power),
                    model.run_with_random(&play, &dt, LiveRandom::new(7)).unwrap(),
                    "mission={mission} power={power}"
                );
                assert_eq!(terminal.current_life(), model.current_life());
                assert_eq!(program.normalized_additive().evaluate(power), model.score());
                assert_eq!(terminal.converted_judgements(), model.converted_judgements());
            }
        }
    }

    #[test]
    fn recording_rejects_started_models_and_finished_controls() {
        let (mut model, play, dt) = declared_model(100, 0, false);
        model.frame(0, &[]).unwrap();
        assert!(matches!(model.compile_score_program(&play, &dt, LiveRandom::new(0)), Err(Error::Input(_))));
        let (mut model, play, dt) = declared_model(100, 0, false);
        model.set_live_finished(true);
        assert!(matches!(model.compile_score_program(&play, &dt, LiveRandom::new(0)), Err(Error::Unsupported(_))));
    }

    #[test]
    fn prefix_logs_share_chunks_but_keep_independent_branch_scores() {
        let mut root = Recorder::new(100, 2);
        root.pending_literal(5);
        root.file_fixed(0).unwrap();
        let mut left = root.clone();
        let mut right = root.clone();
        assert!(Arc::ptr_eq(
            root.nodes.0.borrow().head.as_ref().unwrap(),
            left.nodes.0.borrow().head.as_ref().unwrap()
        ));
        left.pending_literal(7);
        left.file_fixed(1).unwrap();
        right.pending_literal(11);
        right.file_fixed(1).unwrap();
        let first = left.export(12).unwrap();
        let second = left.export(12).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.evaluate(1000), 12);
        assert_eq!(right.finish(16).unwrap().evaluate(-17), 16);
        assert_eq!(root.finish(5).unwrap().evaluate(0), 5);
        assert!(first.allocated_bytes() >= first.node_count() * std::mem::size_of::<Node>());
    }

    #[test]
    fn external_rank_program_records_actual_frame_snapshots_at_other_powers() {
        let confirmations = [crate::replay::RankConfirmation { frame: 0, range: 0, rank: 1, percent: 77 }];
        let (mut origin, play, delta) = declared_model(200_000, 3, true);
        origin.set_rank_confirmation_timeline(&confirmations).unwrap();
        let (program, terminal) = origin.compile_score_program(&play, &delta, LiveRandom::new(7)).unwrap();
        assert!(program.nodes.iter().any(|node| matches!(node, Node::RankPercent(_, 77))));
        for power in [i32::MIN, -123_457, -1, 0, 17, 200_000, 1_000_003, i32::MAX] {
            let (mut fresh, play, delta) = declared_model(power, 3, true);
            fresh.set_rank_confirmation_timeline(&confirmations).unwrap();
            assert_eq!(program.evaluate(power), fresh.run_with_random(&play, &delta, LiveRandom::new(7)).unwrap());
            assert_eq!(program.normalized_additive().evaluate(power), fresh.score());
            assert_eq!(terminal.current_life(), fresh.current_life());
        }
    }
}
