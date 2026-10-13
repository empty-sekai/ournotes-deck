//! The residue law of a signed sum of the integer scores that notes read in their lottery classes.
//!
//! Each state of the propagation carries the residue of the scores its lottery path has read so far. A propagation
//! step takes each lottery state with its residue to the same branches as without one, so the states of one residue
//! keep the masses of their lottery paths. At a read point every state adds the score of the class it reads there,
//! the class whose weights [`LuckDpCertifiedResult`] gives that point, so the masses by residue after the last read
//! point are the law of the sum's residue.

use super::*;

/// What the notes of one read point add to a lottery class's residue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClassResidue {
    /// No native path reaches the class at the point, so it has no mass there.
    Unreached,
    /// Some note's score in the class is not a single integer.
    Several,
    /// The residue of the notes' integer scores, below the plan's modulus.
    Value(u8),
}

/// [`ClassResidue`] of each lottery class 00/01/10/11 (probe bit 0, Rush bit 1).
pub(crate) type ClassResidues = [ClassResidue; 4];

/// A signed sum of note scores whose residue law [`propagate_residues`] gives: the residues the notes add, at the
/// points of the propagation where the notes read their lottery classes.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResiduePlan {
    /// The modulus, from 2 to 255.
    pub(crate) modulus: u8,
    /// Notes that read the class they keep once the lotteries up to their chart time are filed, by chart time: the
    /// state after the hits of that time when it precedes its play frame's time, else the state at the end of the
    /// play frame, where [`LuckDpCertifiedResult::steps`] takes their weights.
    pub(crate) kept: std::collections::BTreeMap<i32, ClassResidues>,
    /// Notes read by a score query of their own play frame, by the frame's time: per phase in the order of
    /// [`LuckDpCertifiedResult::frame_queries`], the state where that entry takes its weights.
    pub(crate) queries: std::collections::BTreeMap<i32, [ClassResidues; 3]>,
}

/// The law of the residue of `plan`'s sum: the mass of each residue below the modulus. The propagation reads its own
/// step tables, whose read steps are those of this plan alone.
pub(in crate::live::full::luck_dp) fn propagate_residues<M: Mass>(
    transcript: &Transcript<M>,
    plan: &ResiduePlan,
) -> Result<Vec<M>, Error> {
    if plan.modulus < 2 {
        return Err(Error::Input("a residue law needs a modulus of at least two".into()));
    }
    let mut tables = StepTables::default();
    tables.enter_transcript(transcript);
    let mut reads = ResidueReads::new(plan);
    propagate_frames(transcript, &mut tables, &mut || false, Some(&mut reads))?;
    // The recording's failure after its last frame fails its law, and so its residue law.
    if let Some(failure) = &transcript.failure {
        return Err(failure.clone());
    }
    reads.masses()
}

/// The read points of a [`ResiduePlan`] met so far, and the masses by residue once the propagation closed them.
pub(super) struct ResidueReads<'p, M: Mass> {
    plan: &'p ResiduePlan,
    kept: std::collections::BTreeSet<i32>,
    queries: std::collections::BTreeSet<i32>,
    point: u32,
    masses: Option<Vec<M>>,
}

impl<'p, M: Mass> ResidueReads<'p, M> {
    fn new(plan: &'p ResiduePlan) -> Self {
        Self { plan, kept: Default::default(), queries: Default::default(), point: 0, masses: None }
    }

    /// Whether every read point of the plan was met.
    fn complete(&self) -> bool {
        self.kept.len() == self.plan.kept.len() && self.queries.len() == self.plan.queries.len()
    }

    fn read(&mut self, dp: &mut Dp<'_, M>, reads: &[(Phase, ClassResidues)]) -> Result<bool, Error> {
        self.point += 1;
        dp.add_residues(self.plan.modulus, reads, self.point)?;
        Ok(self.complete())
    }

    /// Reads the notes of the plan that keep their class at chart time `time` from the state of `dp`, in `phase`;
    /// true once every read point of the plan was met.
    pub(super) fn kept(&mut self, dp: &mut Dp<'_, M>, time: i32, phase: Phase) -> Result<bool, Error> {
        let Some(&residues) = self.plan.kept.get(&time) else { return Ok(false) };
        // Every note of the plan reads its class at one point; a time met twice has no single such point.
        if !self.kept.insert(time) {
            return Err(Error::Unsupported("LUCK residue law: a note time is read twice".into()));
        }
        self.read(dp, &[(phase, residues)])
    }

    /// Reads the notes of the plan that the score queries of the frame at `time`, repeated `repeat` times, read from
    /// the state of `dp`; true once every read point of the plan was met.
    pub(super) fn queries(&mut self, dp: &mut Dp<'_, M>, time: i32, repeat: u32) -> Result<bool, Error> {
        let Some(&[before, at, skills]) = self.plan.queries.get(&time) else { return Ok(false) };
        if repeat != 1 || !self.queries.insert(time) {
            return Err(Error::Unsupported("LUCK residue law: a query frame is read twice".into()));
        }
        self.read(dp, &[(Phase::BeforeFrame, before), (Phase::AtFrame, at), (Phase::BeforeSkills, skills)])
    }

    /// Takes the masses by residue of the states of `dp`.
    pub(super) fn close(&mut self, dp: &mut Dp<'_, M>) {
        let mut masses = vec![M::ZERO; usize::from(self.plan.modulus)];
        for (state, total) in dp.totals() {
            let slot = &mut masses[usize::from(state.residue)];
            *slot = slot.merge(total);
        }
        self.masses = Some(masses);
    }

    fn masses(self) -> Result<Vec<M>, Error> {
        match self.masses {
            Some(masses) if self.complete() => Ok(masses),
            _ => Err(Error::Unsupported("LUCK residue law: a read point is not in the transcript".into())),
        }
    }
}

impl<M: Mass> Dp<'_, M> {
    /// Add to the residue of every state the residues `reads` give the lottery classes the state reads, each in its
    /// phase. A possible state that reads a class without a residue fails the plan.
    fn add_residues(&mut self, modulus: u8, reads: &[(Phase, ClassResidues)], point: u32) -> Result<(), Error> {
        let class = |state: &State, phase: Phase| {
            let (rush, score) = phase.reads(state);
            2 * usize::from(rush) + usize::from(score)
        };
        let readable = |state: &State| {
            reads.iter().all(|&(phase, residues)| matches!(residues[class(state, phase)], ClassResidue::Value(_)))
        };
        if !self.totals().all(|(state, _)| readable(state)) {
            return Err(Error::Unsupported(
                "LUCK residue law: a possible lottery class reads no single integer score".into(),
            ));
        }
        // Only states without possible mass read a class without a residue; their residue is never read.
        self.map(StepKey::Residue { point }, |mut state| {
            let mut residue = u16::from(state.residue);
            for &(phase, residues) in reads {
                if let ClassResidue::Value(value) = residues[class(&state, phase)] {
                    residue += u16::from(value);
                }
            }
            state.residue = (residue % u16::from(modulus)) as u8;
            Ok(state)
        })
    }
}
