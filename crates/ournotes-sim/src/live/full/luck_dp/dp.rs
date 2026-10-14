//! The lottery-state distribution and its propagation through a transcript.

use super::*;

mod joint;
pub(super) use joint::propagate_joint;
pub(crate) use joint::{JointReader, JointView, LotteryFlags};
mod residues;
pub(super) use residues::propagate_residues;
pub(crate) use residues::{ClassResidue, ResiduePlan};

pub(super) struct DpResult<W> {
    pub(super) probe_transitions: Vec<u8>,
    pub(super) rush_transitions: Vec<u16>,
    pub(super) steps: Vec<(i32, W)>,
    pub(super) frame_queries: Vec<(i32, [W; 3])>,
    pub(super) probes: Vec<bool>,
    pub(super) probe_starts: F64Interval,
    pub(super) range_moments: Vec<LuckRangeMoments>,
    pub(super) peak_states: usize,
    pub(super) transitions: u64,
}

/// The next lottery result of a chain. Nothing reads a drawn result before the chain consumes it, and the lottery it
/// is drawn from reads only the lot table, the buff and the guarantee at the draw. So a chain keeps that lottery
/// rather than one state for each of its outcomes, and the consumption takes the outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Next {
    /// A known result, or -1 before any draw.
    Known(i8),
    /// The outcome of the lottery with this index in [`Dp::lotteries`].
    Drawn(u16),
}

impl Default for Next {
    fn default() -> Self {
        Next::Known(0)
    }
}

impl Next {
    /// Distinct words for distinct values.
    fn word(self) -> u64 {
        match self {
            Next::Known(result) => u64::from(result as u8),
            Next::Drawn(index) => (u64::from(index) + 1) << 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Chain {
    pub(super) gauge: i32,
    pub(super) lots: i32,
    pub(super) next: Next,
    pub(super) rush: u8,
    pub(super) maximum: i64,
}

impl Chain {
    pub(super) fn of(score: &LuckScore) -> Self {
        Self {
            gauge: score.gauge,
            lots: score.lot_count,
            next: Next::Known(score.next as i8),
            // From the fourth Critical onwards the table is always CHANCE_LOW. No retained mechanism reads
            // the exact count; saturation avoids retaining an unbounded, irrelevant history.
            rush: score.rush_combo.min(4) as u8,
            maximum: score.gauge_max,
        }
    }

    /// The chain of `score` with this chain's next result: no step of a score reads its next result.
    fn with(self, score: &LuckScore) -> Self {
        Self { next: self.next, ..Self::of(score) }
    }

    /// The score of `template` with this chain's gauge, lots and table, and its next result when known.
    pub(super) fn score(self, template: &LuckScore) -> LuckScore {
        let mut score = template.clone();
        score.gauge = self.gauge;
        score.lot_count = self.lots;
        if let Next::Known(result) = self.next {
            score.next = i64::from(result);
        }
        score.rush_combo = i32::from(self.rush);
        score.gauge_max = self.maximum;
        score
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct State {
    /// At most one Luck range can be active in the supported scheduling domain. Finished ranges are
    /// reset before the next range starts, and future ranges still have their deterministic templates.
    /// Keeping only the active chain therefore forgets no information used by a future transition.
    pub(super) chain: Chain,
    /// All supported guarantees last one draw and share their release. A draw consumes every enabled one,
    /// so their maximum is a sufficient state (not an independent guarantee for each holder).
    pub(super) minimum: i8,
    pub(super) miss_used: bool,
    pub(super) previous_miss: bool,
    pub(super) frame_lot: bool,
    pub(super) frame_miss: bool,
    pub(super) previous_critical: bool,
    pub(super) frame_critical: bool,
    pub(super) rush: bool,
    /// Chart-time rush while scanning this frame's note times. A FINISH disable at frame time must not
    /// remove the rush from earlier chart times in that same frame.
    pub(super) query_rush: bool,
    pub(super) score: bool,
    pub(super) score_before: bool,
    /// Within a frame: the Rush once the frame's start is processed, and whether a lottery of the frame changed it.
    pub(super) rush_start: bool,
    pub(super) rush_filed: bool,
    /// The residue of the integer scores a [`ResiduePlan`] has read on the state's lottery path; zero without one.
    pub(super) residue: u8,
}

impl std::hash::Hash for State {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        // Preserve every Eq field but feed the hasher three words instead of separately hashing each
        // byte-sized flag. The signed fields keep all their original bits; this imposes no gauge or lot cap.
        hasher.write_i64(self.chain.maximum);
        hasher.write_u64(u64::from(self.chain.gauge as u32) | (u64::from(self.chain.lots as u32) << 32));
        hasher.write_u64(
            self.chain.next.word()
                | (u64::from(self.chain.rush) << 32)
                | (u64::from(self.minimum as u8) << 40)
                | ((self.miss_used as u64) << 48)
                | ((self.previous_miss as u64) << 49)
                | ((self.frame_lot as u64) << 50)
                | ((self.frame_miss as u64) << 51)
                | ((self.rush as u64) << 52)
                | ((self.query_rush as u64) << 53)
                | ((self.score as u64) << 54)
                | ((self.score_before as u64) << 55)
                | (u64::from(self.residue) << 56),
        );
        if self.previous_critical || self.frame_critical {
            hasher.write_u8(u8::from(self.previous_critical) | (u8::from(self.frame_critical) << 1));
        }
    }
}

/// The lottery state a note's score reads, relative to its native frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// A note before its frame's time, or a note at the frame's time scored before the frame's skill boundary: the
    /// Rush seen by the frame's score queries and the probe class before the frame.
    BeforeFrame,
    /// A note at its frame's time once the frame's lottery has filed its commands: the Rush and probe class after
    /// the frame.
    AtFrame,
    /// A note at its frame's time scored by a query of that frame before its skill boundary and lottery: the Rush
    /// at the frame's time and the probe class before the frame.
    BeforeSkills,
}

impl Phase {
    pub(super) fn reads(self, state: &State) -> (bool, bool) {
        match self {
            Phase::BeforeFrame => (state.query_rush, state.score_before),
            Phase::AtFrame => (state.rush, state.score),
            Phase::BeforeSkills => (state.rush, state.score_before),
        }
    }
}

/// A state with its gauge zeroed: the part of a state that the states of one block of the distribution share.
pub(super) type RestId = u32;

/// The masses of the states of one rest by gauge: `masses[k]` is that of gauge `lo + k`. A gauge whose mass is not
/// possible holds no state, and its mass is zero.
pub(super) struct Block<M> {
    rest: RestId,
    lo: i32,
    masses: Vec<M>,
    /// The number of states, or `usize::MAX` while a step files masses into the block.
    held: usize,
    /// Once read, the merged mass of the states ([`Mass::total`]).
    total: Option<Option<M>>,
}

impl<M: Mass> Block<M> {
    /// The merged mass of the block's states, read once.
    fn total(&mut self) -> Option<M> {
        *self.total.get_or_insert_with(|| M::total(&self.masses))
    }
}

/// A state a step takes a state to: its rest, how much it moves the gauge, its probability, and the lottery result
/// it draws, which the range moments count.
#[derive(Clone, Copy)]
struct Branch<M> {
    rest: RestId,
    shift: i32,
    probability: M,
    draw: Option<u8>,
}

impl<M: Mass> Branch<M> {
    fn same(&self, other: &Self) -> bool {
        (self.rest, self.shift, self.draw) == (other.rest, other.shift, other.draw)
            && self.probability.bits() == other.probability.bits()
    }
}

/// The gauges `from..to` of a rest whose states a step takes to the same branches, `branches` of its table.
struct Run {
    from: i32,
    to: i32,
    branches: std::ops::Range<usize>,
}

/// What a step does to the states of one rest, read at the gauges `from..to`, in the longest runs of increasing gauges
/// with the same branches.
struct Table<M> {
    from: i32,
    to: i32,
    runs: Vec<Run>,
    branches: Vec<Branch<M>>,
}

impl<M: Mass> Table<M> {
    fn empty(at: i32) -> Self {
        Self { from: at, to: at, runs: Vec::new(), branches: Vec::new() }
    }

    /// Append the branches of gauge `gauge`, the gauge after the last one read.
    fn push(&mut self, gauge: i32, branches: &[Branch<M>]) {
        let extends = self.runs.last().is_some_and(|run| {
            run.to == gauge
                && run.branches.len() == branches.len()
                && self.branches[run.branches.clone()].iter().zip(branches).all(|(a, b)| a.same(b))
        });
        if extends {
            self.runs.last_mut().expect("a run").to = gauge + 1;
        } else {
            let start = self.branches.len();
            self.branches.extend_from_slice(branches);
            self.runs.push(Run { from: gauge, to: gauge + 1, branches: start..self.branches.len() });
        }
        self.to = gauge + 1;
    }

    /// The runs of `self` after those of `below`, which ends where `self` starts, the two runs that meet there as one
    /// when they have the same branches.
    fn after(mut self, mut below: Table<M>) -> Self {
        let offset = below.branches.len();
        let meet = match (below.runs.last(), self.runs.first()) {
            (Some(low), Some(high)) => {
                low.to == high.from
                    && low.branches.len() == high.branches.len()
                    && below.branches[low.branches.clone()]
                        .iter()
                        .zip(&self.branches[high.branches.clone()])
                        .all(|(a, b)| a.same(b))
            }
            _ => false,
        };
        below.branches.append(&mut self.branches);
        let mut runs = self.runs.into_iter();
        if meet {
            let high = runs.next().expect("a run");
            below.runs.last_mut().expect("a run").to = high.to;
        }
        below
            .runs
            .extend(runs.map(|run| Run { branches: run.branches.start + offset..run.branches.end + offset, ..run }));
        below.to = self.to;
        below
    }
}

/// What a step reads besides the state it takes: steps with the same key take each state to the same branches. A
/// residue read point of a [`ResiduePlan`] has its own key in each propagation, so only that propagation's own step
/// tables hold its tables ([`propagate_residues`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum StepKey {
    FrameStart { start: Option<usize>, finish: bool, gate: bool, current_luck: bool, complete: bool },
    Action { target: usize, words: [u64; 4] },
    MissUsed,
    Note { range: usize, note_type: i32, judgement: i32, buff: i32, speed: u32, consumes: bool },
    Pending { range: usize, buff: i32 },
    FrameEnd { finish: bool },
    Residue { point: u32 },
}

/// Everything the branches of a step read besides the state and what a [`StepTables`] holds for all its propagations
/// (the range templates and the lottery machine): the step key, a note's gauge speed replaced by the gauge points it
/// gives each base point, which is all a note's branches read of the speed, and whether the lotteries mark a
/// Critical of the frame.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TableKey {
    step: TableStep,
    critical: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TableStep {
    Step(StepKey),
    /// A note step; `points` numbers the list of its gauge points in [`StepTables::points`].
    Note {
        range: usize,
        note_type: i32,
        judgement: i32,
        buff: i32,
        points: u32,
        consumes: bool,
    },
}

/// The branches a step files for one state: each state with its probability and the lottery result it draws.
type Filed<M> = Vec<(State, M, Option<u8>)>;

/// What reads each run of states a step takes: the rest taken, the rest reached and the run's total mass, given on
/// demand.
type Observe<'o, M> = &'o mut dyn FnMut(&State, &State, &mut dyn FnMut() -> M);

/// What the branches of a step read besides the state: the range templates, the lottery machine and the lotteries
/// met so far.
pub(super) struct Lotteries<M: Mass> {
    pub(super) templates: Vec<LuckScore>,
    pub(super) machine: LotteryMachine,
    /// The index of each lottery drawn, by its lot table, buff and guarantee, and the outcomes of each.
    lotteries: FxHashMap<(usize, i32, i8), u16>,
    outcomes: Vec<Draws<M>>,
    track_critical: bool,
}

impl<M: Mass> Lotteries<M> {
    /// The index of the lottery of lot table `kind` with `buff` and the guarantee `minimum`.
    fn lottery(&mut self, kind: usize, buff: i32, minimum: i8) -> Result<u16, Error> {
        let key = (kind, buff, minimum);
        if let Some(&index) = self.lotteries.get(&key) {
            return Ok(index);
        }
        let index = u16::try_from(self.outcomes.len())
            .map_err(|_| Error::Capacity("LUCK DP exceeds 65536 distinct lotteries".into()))?;
        self.outcomes.push(Draws::of(&M::bonus(&self.machine, kind, buff, minimum)?)?);
        self.lotteries.insert(key, index);
        Ok(index)
    }

    fn consume(&mut self, mut state: State, range: usize, buff: i32, p: M, out: &mut Filed<M>) -> Result<(), Error> {
        state.chain.lots -= 1;
        let first = match state.chain.next {
            Next::Known(-1) => {
                let index = self.lottery(0, buff, state.minimum)?;
                state.minimum = 0;
                self.outcomes[usize::from(index)]
            }
            Next::Known(result) => Draws::one(i64::from(result)),
            Next::Drawn(index) => self.outcomes[usize::from(index)],
        };
        for (pn, result) in first.iter() {
            let mut next = state;
            let mut score = next.chain.score(&self.templates[range]);
            if score.rush_combo == 0 || result != 3 {
                next.rush = result == 3;
                next.query_rush = next.rush;
            }
            // Native files a Rush command exactly when a lottery adds or disables the Rush handle.
            next.rush_filed |= next.rush != state.rush;
            score.add_score(result)?;
            next.frame_lot = true;
            next.frame_miss |= result == 0;
            next.frame_critical |= self.track_critical && result == 3;
            let lottery = self.lottery(score.current_lot_type(), buff, next.minimum)?;
            next.chain = Chain { next: Next::Drawn(lottery), ..Chain::of(&score) };
            next.minimum = 0;
            let draw = u8::try_from(result).map_err(|_| Error::Game("lottery result out of range".into()))?;
            out.push((next, p.multiply(pn), Some(draw)));
        }
        Ok(())
    }
}

/// The step tables of propagations under the same range templates and lottery machine, and the rests and lotteries
/// they number.
///
/// A step takes each state to branches that depend on the state and the step's table key alone ([`TableKey`]), so a
/// table read for one propagation holds, at each gauge it was read at, the branches every propagation that meets the
/// same rest under the same key reads there. A propagation reads such a table at the gauges it would have read its own
/// table at ([`Reads`]), and the tables grow to the gauges their propagations ask for. Numbering the rests and the
/// lotteries for all of them gives each state of each propagation one number, which no step reads.
pub(super) struct StepTables<M: Mass> {
    /// The words of the range templates and the lottery machine the tables read.
    identity: Vec<u64>,
    lotteries: Option<Lotteries<M>>,
    rests: Vec<State>,
    rest_ids: FxHashMap<State, RestId>,
    keys: FxHashMap<TableKey, u32>,
    /// The lists of gauge points of note steps, and the number of the list of each note type, judgement and binary32
    /// gauge speed.
    points: FxHashMap<Vec<i32>, u32>,
    note_points: FxHashMap<(i32, i32, u32), u32>,
    /// The index in `tables` of each rest's table under each table key, by their numbers.
    ids: FxHashMap<(RestId, u32), u32>,
    tables: Vec<Table<M>>,
    /// The branches the tables hold.
    branches: usize,
}

impl<M: Mass> Default for StepTables<M> {
    fn default() -> Self {
        Self {
            identity: Vec::new(),
            lotteries: None,
            rests: Vec::new(),
            rest_ids: FxHashMap::default(),
            keys: FxHashMap::default(),
            points: FxHashMap::default(),
            note_points: FxHashMap::default(),
            ids: FxHashMap::default(),
            tables: Vec::new(),
            branches: 0,
        }
    }
}

/// The sizes from which [`StepTables::enter`] reads its tables anew.
const TABLE_BRANCHES: usize = 1 << 22;
const TABLE_RESTS: usize = 1 << 16;
const TABLE_LOTTERIES: usize = 1 << 15;

impl<M: Mass> StepTables<M> {
    /// Make the tables those of propagations under `templates` and `machine` whose lotteries mark a Critical of the
    /// frame when `track_critical`: tables read under other templates or another machine are dropped, and so are
    /// tables that grew past their size.
    pub(super) fn enter(&mut self, templates: &[LuckScore], machine: &LotteryMachine, track_critical: bool) {
        let mut identity = Vec::with_capacity(self.identity.len());
        identity.push(templates.len() as u64);
        templates.iter().for_each(|template| template.push_words(&mut identity));
        machine.push_words(&mut identity);
        let full = self.branches > TABLE_BRANCHES
            || self.rests.len() > TABLE_RESTS
            || self.lotteries.as_ref().is_some_and(|lotteries| lotteries.outcomes.len() > TABLE_LOTTERIES);
        if self.lotteries.is_none() || identity != self.identity || full {
            *self = Self {
                identity,
                lotteries: Some(Lotteries {
                    templates: templates.to_vec(),
                    machine: machine.clone(),
                    lotteries: FxHashMap::default(),
                    outcomes: Vec::new(),
                    track_critical,
                }),
                ..Self::default()
            };
        }
        self.lotteries_mut().track_critical = track_critical;
    }

    /// [`Self::enter`] for the propagation of `transcript`.
    fn enter_transcript(&mut self, transcript: &Transcript<M>) {
        let critical = transcript.actions.iter().any(|a| matches!(a, Action::CriticalPoints { .. }));
        self.enter(&transcript.templates, &transcript.machine, critical);
    }

    fn lotteries(&self) -> &Lotteries<M> {
        self.lotteries.as_ref().expect("entered step tables")
    }

    fn lotteries_mut(&mut self) -> &mut Lotteries<M> {
        self.lotteries.as_mut().expect("entered step tables")
    }

    /// The id of the rest of `state`.
    fn rest(&mut self, state: State) -> RestId {
        rest_id(&mut self.rests, &mut self.rest_ids, state)
    }

    /// The number of `step` under the current Critical marking.
    fn key(&mut self, step: TableStep) -> u32 {
        let key = TableKey { step, critical: self.lotteries().track_critical };
        let next = self.keys.len() as u32;
        *self.keys.entry(key).or_insert(next)
    }

    /// The number of the gauge points a note of `note_type` judged `judgement` adds at the binary32 gauge speed
    /// `speed` for each base point, as [`Dp::note`] adds them.
    fn note_points(&mut self, note_type: i32, judgement: i32, speed: f32) -> Result<u32, Error> {
        let at = (note_type, judgement, speed.to_bits());
        if let Some(&points) = self.note_points.get(&at) {
            return Ok(points);
        }
        let base = M::base(&self.lotteries().machine, note_type, judgement)?;
        let gauges: Vec<i32> = base.iter().map(|&(_, point)| floor_to_i32((speed + 1f32) * point as f32)).collect();
        let next = self.points.len() as u32;
        let points = *self.points.entry(gauges).or_insert(next);
        self.note_points.insert(at, points);
        Ok(points)
    }

    /// The index of the table of `rest` under the table key numbered `key`.
    fn table(&mut self, rest: RestId, key: u32) -> u32 {
        let next = self.tables.len() as u32;
        let index = *self.ids.entry((rest, key)).or_insert(next);
        if index == next {
            self.tables.push(Table::empty(0));
        }
        index
    }

    /// Read the table `index` of `rest` at the gauges `lo..hi` it does not hold, with the branches `filed` files.
    #[allow(clippy::too_many_arguments)]
    fn cover(
        &mut self,
        index: u32,
        rest: RestId,
        lo: i32,
        hi: i32,
        filed: &mut impl FnMut(&mut Lotteries<M>, State, &mut Filed<M>) -> Result<(), Error>,
        scratch: &mut Filed<M>,
        branches: &mut Vec<Branch<M>>,
    ) -> Result<(), Error> {
        let held = &self.tables[index as usize];
        if !held.runs.is_empty() && held.from <= lo && hi <= held.to {
            return Ok(());
        }
        let mut table = std::mem::replace(&mut self.tables[index as usize], Table::empty(0));
        if table.runs.is_empty() {
            table = Table::empty(lo);
        }
        let before = table.branches.len();
        if lo < table.from {
            let mut below = Table::empty(lo);
            self.read(rest, lo, table.from, &mut below, filed, scratch, branches)?;
            table = table.after(below);
        }
        if hi > table.to {
            let to = table.to;
            self.read(rest, to, hi, &mut table, filed, scratch, branches)?;
        }
        self.branches += table.branches.len() - before;
        self.tables[index as usize] = table;
        Ok(())
    }

    /// Append to `table` the branches `filed` files for the states of `rest` at the gauges `from..to`.
    #[allow(clippy::too_many_arguments)]
    fn read(
        &mut self,
        rest: RestId,
        from: i32,
        to: i32,
        table: &mut Table<M>,
        filed: &mut impl FnMut(&mut Lotteries<M>, State, &mut Filed<M>) -> Result<(), Error>,
        scratch: &mut Filed<M>,
        branches: &mut Vec<Branch<M>>,
    ) -> Result<(), Error> {
        let lotteries = self.lotteries.as_mut().expect("entered step tables");
        for gauge in from..to {
            let mut state = self.rests[rest as usize];
            state.chain.gauge = gauge;
            scratch.clear();
            filed(lotteries, state, scratch)?;
            branches.clear();
            for &(state, probability, draw) in scratch.iter() {
                if probability.possible() {
                    let rest = rest_id(&mut self.rests, &mut self.rest_ids, state);
                    branches.push(Branch { rest, shift: state.chain.gauge.wrapping_sub(gauge), probability, draw });
                }
            }
            table.push(gauge, branches);
        }
        Ok(())
    }
}

/// The id of the rest of `state` among `rests`.
fn rest_id(rests: &mut Vec<State>, ids: &mut FxHashMap<State, RestId>, mut state: State) -> RestId {
    state.chain.gauge = 0;
    if let Some(&id) = ids.get(&state) {
        return id;
    }
    let id = rests.len() as RestId;
    rests.push(state);
    ids.insert(state, id);
    id
}

/// The gauges at which a propagation reads the table of one rest under one step key: `from..to`, the gauges its own
/// table of them would hold, and `breaks`, the gauges where that table would end a run whatever its branches, in
/// decreasing order.
///
/// A propagation's own table is read at the gauges of the first block it takes, and grows to those of the later
/// blocks: at higher gauges it appends their branches, extending its last run when they are the same, and at lower
/// gauges it puts a table of them before its runs, a run of which ends where the old table starts. So its runs are
/// the longest runs of the branches at its gauges, ended at its breaks as well; a step adds the masses of a run to the
/// masses of each of its branches in turn, and the order in which one mass meets several branches follows its runs.
///
/// `whole` holds, once read for the current gauges, the rest the shared table takes every state at them to with the
/// same gauge and certainty, if it does: then it holds them and its single run there has that one branch.
struct Reads {
    from: i32,
    to: i32,
    table: u32,
    breaks: Vec<i32>,
    whole: Option<Option<RestId>>,
}

/// The rest `table` takes every state at the gauges `from..to` to with the same gauge and certainty, if it holds them
/// in one run whose only branch does so.
fn whole_run<M: Mass>(table: &Table<M>, from: i32, to: i32) -> Option<RestId> {
    let run = table.runs.get(table.runs.partition_point(|run| run.to <= from))?;
    match &table.branches[run.branches.clone()] {
        &[branch]
            if run.from <= from
                && to <= run.to
                && branch.shift == 0
                && branch.draw.is_none()
                && branch.probability.bits() == M::ONE.bits() =>
        {
            Some(branch.rest)
        }
        _ => None,
    }
}

/// The lottery-state distribution in blocks of states with one rest, and the tables of the steps it took.
///
/// A step takes each state to branches that depend on the state and the step's key alone. The states of one rest at
/// consecutive gauges whose branches reach the same rests with the same gauge moves and probabilities form a run, and
/// the step adds each run's masses times each branch's probability to the masses of the branch's rest at the moved
/// gauges. A table holds the runs of one rest under one step key, read from the step's branches at each gauge once.
pub(super) struct Dp<'t, M: Mass> {
    tables: &'t mut StepTables<M>,
    active_range: Option<usize>,
    dist: Vec<Block<M>>,
    out: Vec<Block<M>>,
    /// The index in `out` of the block of each rest, or `u32::MAX`.
    slots: Vec<u32>,
    spare: Vec<Vec<M>>,
    /// The number of each step key met, and by that number the number of its table key and the index in `reads` of
    /// the reads of each rest's table (`u32::MAX` before any).
    steps: FxHashMap<StepKey, u32>,
    step_reads: Vec<(u32, Vec<u32>)>,
    reads: Vec<Reads>,
    peak: usize,
    pub(super) transitions: u64,
    pub(super) range_moments: Vec<LuckRangeMoments>,
    /// The joint distribution with a reader's labels, once begun ([`Dp::begin_joint`]).
    joint: Option<joint::JointMasses<M>>,
}

impl<'t, M: Mass> Dp<'t, M> {
    /// The distribution before the first frame, under entered step tables ([`StepTables::enter`]).
    pub(super) fn new(tables: &'t mut StepTables<M>, collect_moments: bool) -> Self {
        let ranges = tables.lotteries().templates.len();
        let rest = tables.rest(State::default());
        let mut dp = Self {
            range_moments: if collect_moments { vec![LuckRangeMoments::default(); ranges] } else { Vec::new() },
            tables,
            active_range: None,
            dist: Vec::new(),
            out: Vec::new(),
            slots: Vec::new(),
            spare: Vec::new(),
            steps: FxHashMap::default(),
            step_reads: Vec::new(),
            reads: Vec::new(),
            peak: 1,
            transitions: 0,
            joint: None,
        };
        let block = Block { rest, lo: State::default().chain.gauge, masses: vec![M::ONE], held: 1, total: None };
        dp.dist.push(block);
        dp.fit_slots();
        dp
    }

    /// The distribution of `states`, whose masses add up per state.
    #[cfg(test)]
    pub(super) fn set_states(&mut self, states: Vec<(State, M)>) {
        let old = std::mem::take(&mut self.dist);
        self.spare.extend(old.into_iter().map(|block| block.masses));
        for (state, mass) in states {
            let rest = self.tables.rest(state);
            self.fit_slots();
            let gauge = state.chain.gauge;
            let block = block_at(&mut self.out, &mut self.slots, &mut self.spare, rest, gauge, gauge + 1);
            let at = (gauge - block.lo) as usize;
            block.masses[at] = block.masses[at].merge(mass);
        }
        self.settle().expect("a small distribution");
    }

    /// The states of the distribution, each with its mass's bit pattern.
    #[cfg(test)]
    pub(super) fn states(&self) -> Vec<(State, i32, [u64; 2])> {
        let mut states = Vec::new();
        for block in &self.dist {
            for (k, mass) in block.masses.iter().enumerate() {
                states.push((self.tables.rests[block.rest as usize], block.lo + k as i32, mass.bits()));
            }
        }
        states
    }

    /// Steps of the range `range` from now on.
    #[cfg(test)]
    pub(super) fn set_active_range(&mut self, range: usize) {
        self.active_range = Some(range);
    }

    /// A slot for every rest the tables number.
    fn fit_slots(&mut self) {
        if self.slots.len() < self.tables.rests.len() {
            self.slots.resize(self.tables.rests.len(), u32::MAX);
        }
    }

    /// The rests of the distribution's states, each with its total mass.
    fn totals(&mut self) -> impl Iterator<Item = (&State, M)> + '_ {
        // Read the totals not read yet up to four side by side ([`Mass::totals`]).
        let mut unread = self.dist.iter_mut().filter(|block| block.total.is_none());
        while let Some(a) = unread.next() {
            match (unread.next(), unread.next(), unread.next()) {
                (Some(b), Some(c), Some(d)) => {
                    let totals = M::totals([&a.masses[..], &b.masses[..], &c.masses[..], &d.masses[..]]);
                    [a, b, c, d].into_iter().zip(totals).for_each(|(block, total)| block.total = Some(total));
                }
                (Some(b), Some(c), None) => {
                    let totals = M::totals([&a.masses[..], &b.masses[..], &c.masses[..]]);
                    [a, b, c].into_iter().zip(totals).for_each(|(block, total)| block.total = Some(total));
                }
                (Some(b), None, _) => {
                    let totals = M::totals([&a.masses[..], &b.masses[..]]);
                    [a, b].into_iter().zip(totals).for_each(|(block, total)| block.total = Some(total));
                }
                (None, ..) => {
                    a.total();
                }
            }
        }
        let rests = &self.tables.rests;
        self.dist.iter_mut().filter_map(|block| block.total().map(|total| (&rests[block.rest as usize], total)))
    }

    /// The rests of the distribution's states.
    pub(super) fn held(&self) -> impl Iterator<Item = &State> + '_ {
        self.dist
            .iter()
            .filter(|block| block.masses.iter().any(|mass| mass.possible()))
            .map(|block| &self.tables.rests[block.rest as usize])
    }

    /// The index in `reads` of the reads of the table of `rest` under the step numbered `step`, whose table key is
    /// numbered `key`, once they hold the gauges `lo..hi`.
    fn reads(&mut self, step: usize, key: u32, rest: RestId, lo: i32, hi: i32) -> usize {
        let held = &mut self.step_reads[step].1;
        if held.len() <= rest as usize {
            held.resize(rest as usize + 1, u32::MAX);
        }
        match held[rest as usize] {
            u32::MAX => {
                held[rest as usize] = self.reads.len() as u32;
                let table = self.tables.table(rest, key);
                self.reads.push(Reads { from: lo, to: hi, table, breaks: Vec::new(), whole: None });
                self.reads.len() - 1
            }
            index => {
                let reads = &mut self.reads[index as usize];
                if lo < reads.from {
                    reads.breaks.push(reads.from);
                    reads.from = lo;
                    reads.whole = None;
                }
                if hi > reads.to {
                    reads.to = hi;
                    reads.whole = None;
                }
                index as usize
            }
        }
    }

    /// Take every state to the branches `filed` files for it, reading each rest's gauges at most once under `key`
    /// (`table` once the shared tables are read). `observe` takes, for each run of states and each branch, the rest
    /// taken, the rest reached and the run's total mass; the range moments of `moments` count the lottery results
    /// drawn.
    ///
    /// A run that takes every state of a block to the same state of another rest, with the same gauge and certainty,
    /// adds each mass of the block to a mass of that rest, or makes it that mass when the rest holds no state yet: the
    /// block becomes the rest's.
    fn step(
        &mut self,
        key: StepKey,
        table: TableStep,
        moments: Option<usize>,
        mut filed: impl FnMut(&mut Lotteries<M>, State, &mut Filed<M>) -> Result<(), Error>,
        mut observe: Option<Observe<'_, M>>,
    ) -> Result<(), Error> {
        let next = self.steps.len() as u32;
        let step = *self.steps.entry(key).or_insert(next) as usize;
        if step == self.step_reads.len() {
            let number = self.tables.key(table);
            self.step_reads.push((number, Vec::new()));
        }
        let number = self.step_reads[step].0;
        let dist = std::mem::take(&mut self.dist);
        let mut scratch = Filed::new();
        let mut branches = Vec::new();
        for mut block in dist {
            let (lo, hi) = (block.lo, block.lo + block.masses.len() as i32);
            let reads = self.reads(step, number, block.rest, lo, hi);
            let index = self.reads[reads].table;
            // The shared table holds the gauges of every block this rest took under the step since `whole` was read.
            let whole = match self.reads[reads].whole {
                Some(whole) => whole,
                None => {
                    self.tables.cover(index, block.rest, lo, hi, &mut filed, &mut scratch, &mut branches)?;
                    self.fit_slots();
                    let Reads { from, to, .. } = self.reads[reads];
                    let whole = whole_run(&self.tables.tables[index as usize], from, to);
                    self.reads[reads].whole = Some(whole);
                    whole
                }
            };
            let Self { tables, out, slots, spare, reads: all_reads, transitions, range_moments, .. } = self;
            let (table, breaks) = (&tables.tables[index as usize], &all_reads[reads].breaks);
            let rests = &tables.rests;
            if let Some(reached) = whole.or_else(|| whole_run(table, lo, hi))
                && block.masses.first().is_some_and(|mass| mass.possible())
                && block.masses.last().is_some_and(|mass| mass.possible())
            {
                if block.held == usize::MAX {
                    block.held = block.masses.iter().filter(|mass| mass.possible()).count();
                }
                *transitions = transitions
                    .checked_add(block.held as u64)
                    .ok_or_else(|| Error::Capacity("LUCK DP transitions overflow".into()))?;
                if let Some(observe) = observe.as_mut() {
                    let (taken, reached) = (&rests[block.rest as usize], &rests[reached as usize]);
                    let mut from = lo;
                    for to in pieces(lo, hi, breaks) {
                        let masses = &block.masses[(from - lo) as usize..(to - lo) as usize];
                        if masses.iter().any(|mass| mass.possible()) {
                            observe(taken, reached, &mut || M::total(masses).expect("a possible mass"));
                        }
                        from = to;
                    }
                }
                if slots[reached as usize] == u32::MAX {
                    slots[reached as usize] = out.len() as u32;
                    out.push(Block { rest: reached, ..block });
                } else {
                    let target = block_at(out, slots, spare, reached, lo, hi);
                    let offset = (lo - target.lo) as usize;
                    M::accumulate(&mut target.masses[offset..offset + block.masses.len()], &block.masses, M::ONE);
                    spare.push(block.masses);
                }
                continue;
            }
            // In a block whose masses are all possible, every piece holds its states from end to end.
            let dense = block.held == block.masses.len();
            for run in &table.runs[table.runs.partition_point(|run| run.to <= lo)..] {
                if run.from >= hi {
                    break;
                }
                let (start, end) = (run.from.max(lo), run.to.min(hi));
                let mut from = start;
                for to in pieces(start, end, breaks) {
                    let piece = from;
                    from = to;
                    let masses = &block.masses[(piece - lo) as usize..(to - lo) as usize];
                    let (mut first, mut last, mut possible) = (usize::MAX, 0, 0u64);
                    if dense {
                        (first, last, possible) = (0, masses.len() - 1, masses.len() as u64);
                    } else {
                        for (k, mass) in masses.iter().enumerate() {
                            if mass.possible() {
                                first = first.min(k);
                                last = k;
                                possible += 1;
                            }
                        }
                    }
                    if possible == 0 {
                        continue;
                    }
                    let (masses, piece) = (&masses[first..=last], piece + first as i32);
                    let counted = moments.is_some_and(|range| range < range_moments.len());
                    let total = (counted || observe.is_some()).then(|| M::total(masses).expect("a possible mass"));
                    for branch in &table.branches[run.branches.clone()] {
                        let at = piece.wrapping_add(branch.shift);
                        let target = block_at(out, slots, spare, branch.rest, at, at + masses.len() as i32);
                        let offset = (at - target.lo) as usize;
                        M::accumulate(&mut target.masses[offset..offset + masses.len()], masses, branch.probability);
                        *transitions = transitions
                            .checked_add(possible)
                            .ok_or_else(|| Error::Capacity("LUCK DP transitions overflow".into()))?;
                        if let (Some(total), Some(draw)) = (total, branch.draw)
                            && let Some(moments) = moments.and_then(|range| range_moments.get_mut(range))
                        {
                            let mass = total.multiply(branch.probability).interval();
                            let result = usize::from(draw);
                            moments.lot_results[result] = moments.lot_results[result].add(mass);
                            let points =
                                if result == 0 { 0 } else { super::gekisou::BONUS_POINT_BY_RESULT[result - 1] };
                            moments.luck_points =
                                moments.luck_points.add(mass.multiply(F64Interval::integer(i128::from(points))));
                        }
                        if let (Some(total), Some(observe)) = (total, observe.as_mut()) {
                            observe(&rests[block.rest as usize], &rests[branch.rest as usize], &mut || total);
                        }
                    }
                }
            }
            spare.push(block.masses);
        }
        self.step_joint(step)?;
        self.settle()
    }

    /// Make the blocks a step filed the distribution.
    fn settle(&mut self) -> Result<(), Error> {
        let mut states = 0;
        for block in &mut self.out {
            self.slots[block.rest as usize] = u32::MAX;
            if block.held == usize::MAX {
                block.held = block.masses.iter().filter(|mass| mass.possible()).count();
            }
            states += block.held;
        }
        self.peak = self.peak.max(states);
        if states > 2_000_000 {
            return Err(Error::Capacity("LUCK DP exceeds two million states".into()));
        }
        self.dist = std::mem::take(&mut self.out);
        Ok(())
    }

    /// Take every state to `transform` of it.
    fn map(&mut self, key: StepKey, mut transform: impl FnMut(State) -> Result<State, Error>) -> Result<(), Error> {
        self.step(
            key,
            TableStep::Step(key),
            None,
            |_, state, out| {
                out.push((transform(state)?, M::ONE, None));
                Ok(())
            },
            None,
        )
    }

    pub(super) fn action(&mut self, action: Action<M>, target: usize) -> Result<(), Error> {
        debug_assert_eq!(self.active_range, Some(target));
        if let Action::CriticalPoints { value, chance } | Action::StartPoints { value, chance } = action {
            let mass = self
                .totals()
                .filter(|(state, _)| matches!(action, Action::StartPoints { .. }) || state.previous_critical)
                .fold(M::ZERO, |sum, (_, probability)| sum.merge(probability));
            let points = mass.multiply(chance).interval().multiply(F64Interval::integer(i128::from(value)));
            self.range_moments[target].luck_points = self.range_moments[target].luck_points.add(points);
            return Ok(());
        }
        let key = StepKey::Action { target, words: action_words(&action) };
        self.step(
            key,
            TableStep::Step(key),
            None,
            |lotteries, state, out| {
                // All accepted Miss rows share the same once/reset trigger. Leave their used flag untouched
                // until every row has run so that multiple eligible rows each apply once in native order.
                if matches!(action, Action::MissGauge { .. }) && (!state.previous_miss || state.miss_used) {
                    out.push((state, M::ONE, None));
                    return Ok(());
                }
                let mut next = state;
                let chance = match action {
                    Action::StartMinimum { result, chance } => {
                        next.minimum = next.minimum.max(result);
                        chance
                    }
                    Action::StartGauge { value, .. } | Action::MissGauge { value } => {
                        let mut score = state.chain.score(&lotteries.templates[target]);
                        let gauge = (score.gauge_max as i128 * value as i128) as i32;
                        score.add_gauge(floor_to_i32(gauge as f32 / 10000f32))?;
                        next.chain = state.chain.with(&score);
                        if let Action::StartGauge { chance, .. } = action { chance } else { M::ONE }
                    }
                    Action::CriticalPoints { .. } | Action::StartPoints { .. } => unreachable!(),
                };
                out.push((next, chance, None));
                let complement = chance.complement();
                if complement.possible() {
                    out.push((state, complement, None));
                }
                Ok(())
            },
            None,
        )
    }

    pub(super) fn note(
        &mut self,
        range: usize,
        note_type: i32,
        judgement: i32,
        buff: i32,
        speed: f32,
        consumes: bool,
    ) -> Result<(), Error> {
        if matches!(judgement, 0 | 7) {
            return Ok(());
        }
        debug_assert_eq!(self.active_range, Some(range));
        let key = StepKey::Note { range, note_type, judgement, buff, speed: speed.to_bits(), consumes };
        let points = self.tables.note_points(note_type, judgement, speed)?;
        let table = TableStep::Note { range, note_type, judgement, buff, points, consumes };
        let mut base: Option<Vec<(M, i32)>> = None;
        self.step(
            key,
            table,
            Some(range),
            |lotteries, state, out| {
                if base.is_none() {
                    base = Some(
                        M::base(&lotteries.machine, note_type, judgement)?
                            .into_iter()
                            .map(|(probability, point)| (probability, floor_to_i32((speed + 1f32) * point as f32)))
                            .collect(),
                    );
                }
                for &(pb, gauge) in base.as_deref().expect("base points") {
                    let mut next = state;
                    let mut score = next.chain.score(&lotteries.templates[range]);
                    score.add_gauge(gauge)?;
                    next.chain = next.chain.with(&score);
                    if consumes && score.lot_count > 0 {
                        lotteries.consume(next, range, buff, pb, out)?;
                    } else {
                        out.push((next, pb, None));
                    }
                }
                Ok(())
            },
            None,
        )
    }

    pub(super) fn pending(&mut self, range: usize, buff: i32) -> Result<(), Error> {
        debug_assert_eq!(self.active_range, Some(range));
        let key = StepKey::Pending { range, buff };
        self.step(
            key,
            TableStep::Step(key),
            Some(range),
            |lotteries, state, out| {
                if state.chain.lots > 0 && !state.frame_lot {
                    lotteries.consume(state, range, buff, M::ONE, out)
                } else {
                    out.push((state, M::ONE, None));
                    Ok(())
                }
            },
            None,
        )
    }

    pub(super) fn weights(&mut self, probes: &[bool], phase: Phase) -> M::Weights {
        M::weights(self.totals(), probes, phase)
    }
}

/// The ends of the pieces of `from..to` that `breaks` (in decreasing order) cut, in increasing order.
fn pieces(from: i32, to: i32, breaks: &[i32]) -> impl Iterator<Item = i32> + '_ {
    breaks.iter().rev().copied().filter(move |&at| from < at && at < to).chain(std::iter::once(to))
}

/// The block of `rest` in `out`, holding the gauges `from..to`, to file masses into.
fn block_at<'b, M: Mass>(
    out: &'b mut Vec<Block<M>>,
    slots: &mut [u32],
    spare: &mut Vec<Vec<M>>,
    rest: RestId,
    from: i32,
    to: i32,
) -> &'b mut Block<M> {
    let slot = &mut slots[rest as usize];
    if *slot == u32::MAX {
        *slot = out.len() as u32;
        let mut masses = spare.pop().unwrap_or_default();
        masses.clear();
        masses.resize((to - from) as usize, M::ZERO);
        out.push(Block { rest, lo: from, masses, held: usize::MAX, total: None });
    }
    let block = &mut out[*slot as usize];
    (block.held, block.total) = (usize::MAX, None);
    if from < block.lo {
        let grow = (block.lo - from) as usize;
        block.masses.splice(0..0, std::iter::repeat_n(M::ZERO, grow));
        block.lo = from;
    }
    let end = (to - block.lo) as usize;
    if end > block.masses.len() {
        block.masses.resize(end, M::ZERO);
    }
    block
}

/// What a propagation hands its read points to, besides the weights it files. Each hook may end the propagation by
/// returning true.
pub(super) trait Hooks<M: Mass> {
    /// Whether the propagation holds the joint distribution ([`Dp::begin_joint`]).
    fn joint(&self) -> bool {
        false
    }

    /// After the start of play frame `play`, or at a play frame the propagation skips (`quiet`), whose states keep
    /// their values.
    fn started(&mut self, _dp: &mut Dp<'_, M>, _play: usize, _quiet: bool) -> Result<bool, Error> {
        Ok(false)
    }

    /// Where the score queries of the frame at `time`, repeated `repeat` times, read their weights.
    fn queries(&mut self, _dp: &mut Dp<'_, M>, _time: i32, _repeat: u32) -> Result<bool, Error> {
        Ok(false)
    }

    /// Where the notes at chart time `time` read the class they keep, in `phase`.
    fn kept(&mut self, _dp: &mut Dp<'_, M>, _time: i32, _phase: Phase) -> Result<bool, Error> {
        Ok(false)
    }

    /// Once the lotteries of play frame `play` are drawn, before its end; `quiet` as for [`Self::started`].
    fn lotteries(&mut self, _dp: &mut Dp<'_, M>, _play: usize, _quiet: bool) -> Result<bool, Error> {
        Ok(false)
    }

    /// After the last frame the propagation takes.
    fn close(&mut self, _dp: &mut Dp<'_, M>) -> Result<(), Error> {
        Ok(())
    }
}

/// A propagation with no read points besides its weights.
struct NoHooks;

impl<M: Mass> Hooks<M> for NoHooks {}

/// Propagate the lottery-state distribution through a transcript.
pub(super) fn propagate<M: Mass>(transcript: &Transcript<M>) -> Result<DpResult<M::Weights>, Error> {
    Ok(propagate_cancellable(transcript, &mut || false)?.expect("complete propagation"))
}

pub(super) fn propagate_cancellable<M: Mass>(
    transcript: &Transcript<M>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<DpResult<M::Weights>>, Error> {
    let mut tables = StepTables::default();
    tables.enter_transcript(transcript);
    let result = propagate_frames(transcript, &mut tables, cancelled, &mut NoHooks)?;
    finish(transcript, result)
}

/// [`propagate_cancellable`] reading the step tables of `tables`, which keeps the tables it reads for later
/// propagations under the same range templates and lottery machine.
///
/// Each step reads, at each gauge, the branches the propagation would read on its own, and in the runs its own tables
/// would hold ([`Reads`]), so it files every mass as the propagation alone files it, in the same order: the result is
/// the same. A propagation that fails on shared tables, which number the lotteries of other propagations as well, is
/// propagated on its own tables, which report its own failure.
pub(super) fn propagate_sharing<M: Mass>(
    transcript: &Transcript<M>,
    tables: &mut StepTables<M>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<DpResult<M::Weights>>, Error> {
    tables.enter_transcript(transcript);
    match propagate_frames(transcript, tables, cancelled, &mut NoHooks) {
        Ok(result) => finish(transcript, result),
        Err(_) => {
            *tables = StepTables::default();
            propagate_cancellable(transcript, cancelled)
        }
    }
}

/// The result of a complete propagation of `transcript`, or the failure its recording met after its last frame.
fn finish<W>(transcript: &Transcript<impl Mass>, result: Option<DpResult<W>>) -> Result<Option<DpResult<W>>, Error> {
    match (&transcript.failure, result) {
        (_, None) => Ok(None),
        (Some(failure), Some(_)) => Err(failure.clone()),
        (None, Some(result)) => Ok(Some(result)),
    }
}

/// The propagation of `transcript`, handing its read points to `hooks`. A hook that ends the propagation ends it
/// there: the result then covers the frames up to that point.
fn propagate_frames<M: Mass>(
    transcript: &Transcript<M>,
    tables: &mut StepTables<M>,
    cancelled: &mut impl FnMut() -> bool,
    hooks: &mut impl Hooks<M>,
) -> Result<Option<DpResult<M::Weights>>, Error> {
    let t = transcript;
    let mut dp = Dp::<M>::new(tables, t.collect_moments);
    if hooks.joint() {
        dp.begin_joint()?;
    }
    // Each note hit and each pending frame consumes at most one result. Critical point commands fire at
    // most once per recorded action. These bounds prove that additive indicators cannot wrap native i32.
    if t.collect_moments {
        let draws = t.hits.len() as i128 + t.frames.iter().map(|f| i128::from(f.repeat)).sum::<i128>();
        let mut lower = 0i128;
        let mut upper = 10 * draws;
        let mut from = 0;
        for frame in &t.frames {
            for action in &t.actions[from..frame.actions] {
                if let Action::CriticalPoints { value, chance } | Action::StartPoints { value, chance } = *action
                    && chance.possible()
                {
                    let value = i128::from(value) * i128::from(frame.repeat);
                    lower += value.min(0);
                    upper += value.max(0);
                }
            }
            from = frame.actions;
        }
        if t.templates.iter().any(|s| {
            i128::from(s.total_bonus_point) + lower < i128::from(i32::MIN)
                || i128::from(s.total_bonus_point) + upper > i128::from(i32::MAX)
        }) {
            return Err(Error::Unsupported("LUCK DP: additive lottery points may wrap".into()));
        }
    }
    let mut steps: Vec<(i32, M::Weights)> = Vec::new();
    let mut frame_queries: Vec<(i32, [M::Weights; 3])> = Vec::new();
    let mut probe_transitions = Vec::new();
    let mut probe_starts = F64Interval::ZERO;
    let mut rush_transitions = Vec::new();
    let mut identity = 1u8;
    // The Rush transitions of a frame in which no lottery runs: every state keeps its Rush.
    let mut kept = 1u16;
    let mut previous_lot = false;
    let mut queued = vec![false; t.luck.len()];
    let (mut notes_from, mut actions_from, mut pending_from) = (0usize, 0usize, 0usize);
    // The number of the current play frame.
    let mut play = 0usize;
    'frames: for frame in &t.frames {
        if cancelled() {
            return Ok(None);
        }
        let hits_from = notes_from.checked_sub(1).map_or(0, |i| t.notes[i].hits);
        let notes = &t.notes[notes_from..frame.notes];
        let actions = &t.actions[actions_from..frame.actions];
        let pending = &t.pending[pending_from..frame.pending];
        (notes_from, actions_from, pending_from) = (frame.notes, frame.actions, frame.pending);
        let (finish, complete, gate, current_luck) = (frame.finish, frame.complete, frame.gate, frame.current_luck);
        for repeat in 0..frame.repeat {
            if repeat.is_multiple_of(64) && cancelled() {
                return Ok(None);
            }
            if notes.is_empty()
                && frame.start.is_none()
                && !complete
                && !finish
                && !previous_lot
                && !pending.iter().any(|&(range, _)| queued[range])
            {
                // No lottery or dependent skill state can change here. A consumed lot forces the FOLLOWING
                // frame through the DP for 7021 and previous-frame 7000.
                probe_transitions.push(identity);
                rush_transitions.push(kept);
                if hooks.started(&mut dp, play, true)? || hooks.lotteries(&mut dp, play, true)? {
                    break 'frames;
                }
                play += 1;
                continue;
            }
            if let Some(range) = frame.start {
                debug_assert!(dp.active_range.is_none());
                dp.active_range = Some(range);
            }
            let starting_chain = frame.start.map(|range| Chain::of(&dp.tables.lotteries().templates[range]));
            let mut edges = 0u8;
            let mut follows = true;
            let mut started = M::ZERO;
            let key = StepKey::FrameStart { start: frame.start, finish, gate, current_luck, complete };
            // The certified distribution retains every state with positive possible probability.
            dp.step(
                key,
                TableStep::Step(key),
                None,
                |_, mut state, out| {
                    if let Some(chain) = starting_chain {
                        state.chain = chain;
                    }
                    state.query_rush = state.rush;
                    state.score_before = state.score;
                    state.frame_lot = false;
                    state.frame_miss = false;
                    state.frame_critical = false;
                    if finish {
                        state.rush = false;
                    }
                    if gate {
                        if current_luck {
                            state.score = state.chain.rush != 0;
                        }
                        if complete || finish {
                            state.score = false;
                        }
                    }
                    state.rush_start = state.rush;
                    state.rush_filed = false;
                    if complete {
                        state.minimum = 0;
                        state.miss_used = false;
                    }
                    out.push((state, M::ONE, None));
                    Ok(())
                },
                Some(&mut |before: &State, after: &State, probability: &mut dyn FnMut() -> M| {
                    edges |= 1 << (2 * usize::from(before.score) + usize::from(after.score));
                    if !before.score && after.score {
                        started = started.merge(probability());
                    }
                    follows &= after.score == after.rush;
                }),
            )?;
            probe_transitions.push(edges);
            probe_starts = probe_starts.add(started.interval());
            // Subsequent actions and lotteries do not alter this direct-probe class.
            identity = u8::from(edges & 0b0101 != 0) | (u8::from(edges & 0b1010 != 0) << 3);
            // The frame's score queries run after the FINISH disable and the skill boundary, before any lottery.
            if !notes.is_empty() {
                frame_queries.push((
                    frame.time_ms,
                    [Phase::BeforeFrame, Phase::AtFrame, Phase::BeforeSkills].map(|phase| dp.weights(&t.probes, phase)),
                ));
                if hooks.queries(&mut dp, frame.time_ms, frame.repeat)? {
                    break 'frames;
                }
            }
            if hooks.started(&mut dp, play, false)? {
                break 'frames;
            }
            if gate && frame.target >= 0 {
                for &action in actions {
                    dp.action(action, frame.target as usize)?;
                }
                if t.miss_rows {
                    dp.map(StepKey::MissUsed, |mut state| {
                        state.miss_used |= state.previous_miss;
                        Ok(state)
                    })?;
                }
            }
            let mut hit = hits_from;
            let mut i = 0;
            while i < notes.len() {
                let time = notes[i].time_ms;
                let mut end = i + 1;
                while end < notes.len() && notes[end].time_ms == time {
                    end += 1;
                }
                for note in &notes[i..end] {
                    for h in &t.hits[hit..note.hits] {
                        dp.note(h.range, note.note_type, note.judgement, h.buff, h.speed, h.consumes)?;
                    }
                    hit = note.hits;
                }
                if time < frame.time_ms {
                    let values = dp.weights(&t.probes, Phase::BeforeFrame);
                    if steps.last().is_none_or(|last| last.1 != values) {
                        steps.push((time, values));
                    }
                    if hooks.kept(&mut dp, time, Phase::BeforeFrame)? {
                        break 'frames;
                    }
                }
                i = end;
            }
            for &(range, buff) in pending {
                dp.pending(range, buff)?;
            }
            let mut lottery = 0u16;
            for state in dp.held() {
                lottery |=
                    1 << (4 * u16::from(state.rush_start) + 2 * u16::from(state.rush) + u16::from(state.rush_filed));
            }
            rush_transitions
                .push(lottery | if finish { RUSH_FINISH } else { 0 } | if follows { RUSH_PROBE } else { 0 });
            kept = u16::from(lottery & 0x33 != 0) | u16::from(lottery & 0xcc != 0) << 6;
            if notes.last().is_some_and(|note| note.time_ms == frame.time_ms) {
                let values = dp.weights(&t.probes, Phase::AtFrame);
                if steps.last().is_none_or(|last| last.1 != values) {
                    steps.push((frame.time_ms, values));
                }
                if hooks.kept(&mut dp, frame.time_ms, Phase::AtFrame)? {
                    break 'frames;
                }
            }
            if hooks.lotteries(&mut dp, play, false)? {
                break 'frames;
            }
            previous_lot = dp.held().any(|state| state.frame_lot);
            dp.map(StepKey::FrameEnd { finish }, |mut state| {
                state.previous_miss = state.frame_miss;
                state.previous_critical = state.frame_critical;
                state.query_rush = state.rush;
                state.score_before = state.score;
                state.frame_miss = false;
                state.frame_critical = false;
                state.frame_lot = false;
                state.rush_start = false;
                state.rush_filed = false;
                if finish {
                    state.chain = Chain::default();
                }
                Ok(state)
            })?;
            if finish {
                dp.active_range = None;
            }
            queued.fill(false);
            if let Some(range) = dp.active_range {
                queued[range] = dp.held().any(|state| state.chain.lots > 0);
            }
            play += 1;
        }
    }
    hooks.close(&mut dp)?;
    Ok(Some(DpResult {
        probe_transitions,
        rush_transitions,
        steps,
        frame_queries,
        probes: t.probes.clone(),
        probe_starts,
        range_moments: std::mem::take(&mut dp.range_moments),
        peak_states: dp.peak,
        transitions: dp.transitions,
    }))
}
