//! Select domain partitions from the compiled per-entry conversion reach.
use super::*;

impl JointFineBounds {
    /// Domain Snap indexes whose compiled conversion masks add a judgement to some member-only source.
    /// Each source includes the member's rows and, for a Snap choice, its active support rows. The masks retain
    /// the mission gates, condition reach and registration windows of the compiled envelope.
    ///
    /// This selects the partition boundaries. Every allowed Snap remains in the searched domain, and every
    /// part's envelope is compiled from all of that part's choices.
    pub(crate) fn conversion_snaps(&self, members: &[usize], snaps: &[usize]) -> Vec<usize> {
        let mut adds = HashMap::new();
        snaps
            .iter()
            .enumerate()
            .filter_map(|(choice, &snap)| {
                members
                    .iter()
                    .any(|&member| {
                        let base = self.fine.src[member][0];
                        let source = self.fine.src[member][self.choice_class(member, choice + 1)];
                        *adds.entry((base, source)).or_insert_with(|| {
                            source != base
                                && self.fine.extra[source as usize].iter().zip(&self.fine.extra[base as usize]).any(
                                    |(extra, own)| {
                                        extra
                                            .iter()
                                            .enumerate()
                                            .any(|(e, &mask)| mask & !own.get(e).copied().unwrap_or(0) != 0)
                                    },
                                )
                        })
                    })
                    .then_some(snap)
            })
            .collect()
    }
}
