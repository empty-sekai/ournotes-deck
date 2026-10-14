//! The lottery-state distribution jointly with labels a reader of the propagation attaches to the lottery paths.
//!
//! Each mass of the joint distribution belongs to a lottery state, a group and a member of the group. A propagation
//! step takes each lottery state to branches that depend on the state and the step alone, so the joint masses of a
//! state take the step's branches of that state and keep their group and member: the masses of each group and member
//! are those of the lottery paths the label holds. Between steps the reader relabels the masses by their states'
//! lottery flags, which moves each mass to one group and member and merges the masses that meet.

use super::*;

/// The lottery flags of a state: everything a joint reader reads of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LotteryFlags {
    pub(crate) rush: bool,
    pub(crate) query_rush: bool,
    pub(crate) score: bool,
    pub(crate) score_before: bool,
    pub(crate) rush_start: bool,
    pub(crate) rush_filed: bool,
}

impl LotteryFlags {
    fn of(state: &State) -> Self {
        Self {
            rush: state.rush,
            query_rush: state.query_rush,
            score: state.score,
            score_before: state.score_before,
            rush_start: state.rush_start,
            rush_filed: state.rush_filed,
        }
    }

    /// The lottery class 00/01/10/11 (probe bit 0, Rush bit 1) a note reads in `phase`.
    pub(crate) fn class(self, phase: Phase) -> u8 {
        let (rush, score) = match phase {
            Phase::BeforeFrame => (self.query_rush, self.score_before),
            Phase::AtFrame => (self.rush, self.score),
            Phase::BeforeSkills => (self.rush, self.score_before),
        };
        2 * u8::from(rush) + u8::from(score)
    }

    /// Distinct words for distinct flags.
    pub(crate) fn word(self) -> u8 {
        u8::from(self.rush)
            | u8::from(self.query_rush) << 1
            | u8::from(self.score) << 2
            | u8::from(self.score_before) << 3
            | u8::from(self.rush_start) << 4
            | u8::from(self.rush_filed) << 5
    }
}

/// The joint masses of one rest and group by gauge and member: `masses[k * width + m]` is that of gauge `lo + k` and
/// member `m`.
pub(crate) struct JointBlock<M> {
    rest: RestId,
    group: u32,
    width: u32,
    lo: i32,
    masses: Vec<M>,
}

impl<M> JointBlock<M> {
    fn rows(&self) -> usize {
        self.masses.len() / self.width as usize
    }
}

/// The most joint masses a propagation holds.
const JOINT_MASSES: usize = 1 << 25;

/// The joint distribution in blocks of one rest and group.
pub(super) struct JointMasses<M> {
    blocks: Vec<JointBlock<M>>,
    out: Vec<JointBlock<M>>,
    /// The index in `out` of the block of each rest and group.
    slots: FxHashMap<(RestId, u32), u32>,
    spare: Vec<Vec<M>>,
}

impl<M: Mass> JointMasses<M> {
    /// Every mass in group 0, member 0 of a group of one member.
    pub(super) fn new(rest: RestId, gauge: i32) -> Self {
        let block = JointBlock { rest, group: 0, width: 1, lo: gauge, masses: vec![M::ONE] };
        Self { blocks: vec![block], out: Vec::new(), slots: FxHashMap::default(), spare: Vec::new() }
    }

    /// The block of `rest` and `group` in `out`, holding the gauges `from..to`, to file masses into.
    fn block_at(&mut self, rest: RestId, group: u32, width: u32, from: i32, to: i32) -> &mut JointBlock<M> {
        let w = width as usize;
        let index = match self.slots.get(&(rest, group)) {
            Some(&index) => index as usize,
            None => {
                let mut masses = self.spare.pop().unwrap_or_default();
                masses.clear();
                masses.resize((to - from) as usize * w, M::ZERO);
                self.slots.insert((rest, group), self.out.len() as u32);
                self.out.push(JointBlock { rest, group, width, lo: from, masses });
                self.out.len() - 1
            }
        };
        let block = &mut self.out[index];
        if from < block.lo {
            let grow = (block.lo - from) as usize * w;
            block.masses.splice(0..0, std::iter::repeat_n(M::ZERO, grow));
            block.lo = from;
        }
        let end = (to - block.lo) as usize * w;
        if end > block.masses.len() {
            block.masses.resize(end, M::ZERO);
        }
        block
    }

    /// Files `block` into `out`, merging it into the block of its rest and group there.
    fn put(&mut self, block: JointBlock<M>) {
        if !self.slots.contains_key(&(block.rest, block.group)) {
            self.slots.insert((block.rest, block.group), self.out.len() as u32);
            self.out.push(block);
            return;
        }
        let (lo, rows) = (block.lo, block.rows() as i32);
        let target = self.block_at(block.rest, block.group, block.width, lo, lo + rows);
        let offset = (lo - target.lo) as usize * block.width as usize;
        M::accumulate(&mut target.masses[offset..offset + block.masses.len()], &block.masses, M::ONE);
        self.spare.push(block.masses);
    }

    /// Makes the blocks filed the distribution.
    fn settle(&mut self) -> Result<(), Error> {
        self.slots.clear();
        if self.out.iter().map(|block| block.masses.len()).sum::<usize>() > JOINT_MASSES {
            return Err(Error::Capacity("LUCK DP joint masses exceed their budget".into()));
        }
        self.blocks = std::mem::take(&mut self.out);
        Ok(())
    }
}

fn unaligned() -> Error {
    Error::Domain("LUCK DP: a joint mass has no lottery state of the propagation".into())
}

impl<M: Mass> Dp<'_, M> {
    /// Hold the joint distribution from now on: every state's mass in group 0, member 0. Needs the distribution of one
    /// state.
    pub(super) fn begin_joint(&mut self) -> Result<(), Error> {
        match self.dist.as_slice() {
            [block] if block.masses.len() == 1 => {
                self.joint = Some(JointMasses::new(block.rest, block.lo));
                Ok(())
            }
            _ => Err(Error::Domain("LUCK DP: a joint distribution starts from one state".into())),
        }
    }

    /// Take the joint masses through the step numbered `step`, which the distribution just took: each mass goes, in
    /// its group and member, to the branches the step's table holds for its rest and gauge.
    pub(super) fn step_joint(&mut self, step: usize) -> Result<(), Error> {
        let Self { tables, step_reads, reads, joint, .. } = self;
        let Some(joint) = joint.as_mut() else { return Ok(()) };
        let held = &step_reads[step].1;
        for block in std::mem::take(&mut joint.blocks) {
            let width = block.width as usize;
            let (lo, hi) = (block.lo, block.lo + block.rows() as i32);
            let read = held.get(block.rest as usize).copied().filter(|&read| read != u32::MAX).ok_or_else(unaligned)?;
            let table = &tables.tables[reads[read as usize].table as usize];
            if lo < table.from || hi > table.to {
                return Err(unaligned());
            }
            if let Some(reached) = whole_run(table, lo, hi) {
                joint.put(JointBlock { rest: reached, ..block });
                continue;
            }
            for run in &table.runs[table.runs.partition_point(|run| run.to <= lo)..] {
                if run.from >= hi {
                    break;
                }
                let (start, end) = (run.from.max(lo), run.to.min(hi));
                let masses = &block.masses[(start - lo) as usize * width..(end - lo) as usize * width];
                let Some(first) = masses.iter().position(|mass| mass.possible()) else { continue };
                let last = masses.iter().rposition(|mass| mass.possible()).expect("a possible mass");
                let (from, to) = (first / width, last / width + 1);
                let masses = &masses[from * width..to * width];
                let piece = start + from as i32;
                for branch in &table.branches[run.branches.clone()] {
                    let at = piece.wrapping_add(branch.shift);
                    let target = joint.block_at(branch.rest, block.group, block.width, at, at + (to - from) as i32);
                    let offset = (at - target.lo) as usize * width;
                    M::accumulate(&mut target.masses[offset..offset + masses.len()], masses, branch.probability);
                }
            }
            joint.spare.push(block.masses);
        }
        joint.settle()
    }

    /// The joint blocks: each one's lottery flags, group, width and masses by gauge and member.
    pub(super) fn joint_blocks(&self) -> impl Iterator<Item = (LotteryFlags, u32, usize, &[M])> + '_ {
        let rests = &self.tables.rests;
        self.joint.iter().flat_map(|joint| &joint.blocks).map(move |block| {
            (LotteryFlags::of(&rests[block.rest as usize]), block.group, block.width as usize, &block.masses[..])
        })
    }

    /// Relabel the joint masses: `relabel` gives, for the flags and group of each block with possible mass, the block's
    /// new group and the index in `maps` of the new member of each of its members, `u32::MAX` keeping their places
    /// among the first members of the new group; `widths` gives the members of each new group. Masses that meet merge;
    /// blocks without possible mass go.
    pub(super) fn remap_joint(
        &mut self,
        widths: &[u32],
        maps: &[Vec<u32>],
        mut relabel: impl FnMut(LotteryFlags, u32) -> Result<(u32, u32), Error>,
    ) -> Result<(), Error> {
        let Self { tables, joint, .. } = self;
        let Some(joint) = joint.as_mut() else { return Ok(()) };
        let shape = || Error::Domain("LUCK DP: a joint relabelling does not fit its groups".into());
        for block in std::mem::take(&mut joint.blocks) {
            // A block without possible mass holds no lottery path, so no label.
            if !block.masses.iter().any(|mass| mass.possible()) {
                joint.spare.push(block.masses);
                continue;
            }
            let (group, map) = relabel(LotteryFlags::of(&tables.rests[block.rest as usize]), block.group)?;
            let width = *widths.get(group as usize).ok_or_else(shape)?;
            if map == u32::MAX && width == block.width {
                joint.put(JointBlock { group, ..block });
                continue;
            }
            let map = match map {
                u32::MAX if block.width < width => None,
                u32::MAX => return Err(shape()),
                map => Some(maps.get(map as usize).filter(|map| map.len() == block.width as usize).ok_or_else(shape)?),
            };
            if map.is_some_and(|map| map.iter().any(|&member| member >= width)) {
                return Err(shape());
            }
            let (old, new, rows) = (block.width as usize, width as usize, block.rows());
            let target = joint.block_at(block.rest, group, width, block.lo, block.lo + rows as i32);
            let offset = (block.lo - target.lo) as usize;
            for row in 0..rows {
                for member in 0..old {
                    let mass = block.masses[row * old + member];
                    if mass.possible() {
                        let to = map.map_or(member, |map| map[member] as usize);
                        let slot = &mut target.masses[(offset + row) * new + to];
                        *slot = slot.merge(mass);
                    }
                }
            }
            joint.spare.push(block.masses);
        }
        joint.settle()
    }
}

/// What a joint reader sees of the propagation at its read points.
pub(crate) struct JointView<'v, 't> {
    dp: &'v mut Dp<'t, ProbabilityMass>,
}

impl JointView<'_, '_> {
    /// Each block of the joint distribution: its lottery flags, group, number of members and masses, `masses[k * width
    /// + m]` that of its `k`th gauge and member `m`.
    pub(crate) fn blocks(&self) -> impl Iterator<Item = (LotteryFlags, u32, usize, &[ProbabilityMass])> + '_ {
        self.dp.joint_blocks()
    }

    /// See [`Dp::remap_joint`].
    pub(crate) fn remap(
        &mut self,
        widths: &[u32],
        maps: &[Vec<u32>],
        relabel: impl FnMut(LotteryFlags, u32) -> Result<(u32, u32), Error>,
    ) -> Result<(), Error> {
        self.dp.remap_joint(widths, maps, relabel)
    }
}

/// A reader of a joint propagation. It sees the joint distribution after the start of each play frame (`started`),
/// at each read point of a note's kept lottery class (`kept`), once each play frame's lotteries are drawn
/// (`lotteries`), and at the end. A play frame the propagation skips (`quiet`) changes no state.
pub(crate) trait JointReader {
    fn started(&mut self, view: &mut JointView<'_, '_>, play: usize, quiet: bool) -> Result<(), Error>;
    fn kept(&mut self, view: &mut JointView<'_, '_>, time: i32, phase: Phase) -> Result<(), Error>;
    fn lotteries(&mut self, view: &mut JointView<'_, '_>, play: usize, quiet: bool) -> Result<(), Error>;
    fn close(&mut self, view: &mut JointView<'_, '_>) -> Result<(), Error>;
}

/// The hooks of a joint propagation, which hand the read points to its reader.
struct JointHooks<'r, R>(&'r mut R);

impl<R: JointReader> Hooks<ProbabilityMass> for JointHooks<'_, R> {
    fn joint(&self) -> bool {
        true
    }

    fn started(&mut self, dp: &mut Dp<'_, ProbabilityMass>, play: usize, quiet: bool) -> Result<bool, Error> {
        self.0.started(&mut JointView { dp }, play, quiet)?;
        Ok(false)
    }

    fn kept(&mut self, dp: &mut Dp<'_, ProbabilityMass>, time: i32, phase: Phase) -> Result<bool, Error> {
        self.0.kept(&mut JointView { dp }, time, phase)?;
        Ok(false)
    }

    fn lotteries(&mut self, dp: &mut Dp<'_, ProbabilityMass>, play: usize, quiet: bool) -> Result<bool, Error> {
        self.0.lotteries(&mut JointView { dp }, play, quiet)?;
        Ok(false)
    }

    fn close(&mut self, dp: &mut Dp<'_, ProbabilityMass>) -> Result<(), Error> {
        self.0.close(&mut JointView { dp })
    }
}

/// Propagate the certified lottery law of `transcript` jointly with the labels of `reader`, on its own step tables.
pub(in crate::live::full::luck_dp) fn propagate_joint(
    transcript: &Transcript<ProbabilityMass>,
    reader: &mut impl JointReader,
) -> Result<(), Error> {
    let mut tables = StepTables::default();
    tables.enter_transcript(transcript);
    propagate_frames(transcript, &mut tables, &mut || false, &mut JointHooks(reader))?;
    // The recording's failure after its last frame fails its law, and so the joint law.
    if let Some(failure) = &transcript.failure {
        return Err(failure.clone());
    }
    Ok(())
}
