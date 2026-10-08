//! One successful reduced-constructor proof inside one complete physical-pair admission.
use super::*;

#[cfg(test)]
std::thread_local! {
    static VALIDATIONS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
pub(super) fn count_original() {
    VALIDATIONS.with(|counts| {
        let (original, projected) = counts.get();
        counts.set((original + 1, projected));
    });
}

#[cfg(test)]
pub(super) fn count_projected() {
    VALIDATIONS.with(|counts| {
        let (original, projected) = counts.get();
        counts.set((original, projected + 1));
    });
}

/// Containers already retained by the current admission. Choice inputs remain borrowed; only the largest
/// projected comparison scratch is charged for them. All accounting uses actual capacities, not lengths.
pub(super) struct WorkingSet<'a> {
    pub(super) base: &'a [Performer; SLOTS],
    pub(super) choices: &'a [Vec<LuckFamilyChoice>; SLOTS],
    pub(super) allowed: &'a [Vec<Option<usize>>; SLOTS],
    pub(super) writers: &'a Vec<usize>,
    pub(super) profiles: &'a Vec<[Option<usize>; SLOTS]>,
    pub(super) resources_capacity: usize,
}

fn performer_payload(performer: &Performer) -> Option<usize> {
    performer
        .support_skills
        .capacity()
        .checked_add(performer.gekisou_support_skills.capacity())?
        .checked_mul(size_of::<(i64, i64)>())?
        .checked_add(
            performer
                .tag_ids
                .capacity()
                .checked_add(performer.live_skill_categories.capacity())?
                .checked_add(performer.gekisou_skill_categories.capacity())?
                .checked_mul(size_of::<i64>())?,
        )
}

impl WorkingSet<'_> {
    fn allocated_bytes(&self) -> Option<usize> {
        let mut bytes = size_of::<AdmittedFamilyInputs>()
            .checked_add(size_of::<Vec<(usize, &Performer)>>())?
            .checked_add(self.resources_capacity.checked_mul(size_of::<(usize, &Performer)>())?)?
            .checked_add(self.writers.capacity().checked_mul(size_of::<usize>())?)?
            .checked_add(self.profiles.capacity().checked_mul(size_of::<[Option<usize>; SLOTS]>())?)?;
        for allowed in self.allowed {
            bytes = bytes.checked_add(allowed.capacity().checked_mul(size_of::<Option<usize>>())?)?;
        }
        let mut scratch_payload = 0;
        for performer in self.base {
            let payload = performer_payload(performer)?;
            bytes = bytes.checked_add(payload)?;
            scratch_payload = scratch_payload.max(payload);
        }
        for choice in self.choices.iter().flatten() {
            scratch_payload = scratch_payload.max(performer_payload(&choice.performer)?);
        }
        // Comparing a borrowed previous input reconstructs one Performer at a time. The current complete
        // projected deck is the same scratch used by the original nonmemoized constructor path.
        bytes.checked_add(size_of::<Performer>())?.checked_add(scratch_payload)
    }
}

/// The immutable references identify the complete original pair that passed `admit_projection`; they are
/// not a hash or a member/source signature. A hit reconstructs and compares every full projected Performer.
/// This instance borrows one fixed context and cannot outlive its admission's immutable base/choice inputs.
pub(super) struct PreviousProjection<'a, 'm> {
    context: &'a LuckFamilyContext<'m>,
    previous: Option<[&'a Performer; SLOTS]>,
    #[cfg(test)]
    pub(super) compilations: usize,
    #[cfg(test)]
    pub(super) hits: usize,
}

impl<'a, 'm> PreviousProjection<'a, 'm> {
    pub(super) fn new(context: &'a LuckFamilyContext<'m>, working: WorkingSet<'_>, capacity: usize) -> Option<Self> {
        let bytes = working.allocated_bytes()?.checked_add(size_of::<Self>())?;
        if bytes > capacity {
            return None;
        }
        Some(Self {
            context,
            previous: None,
            #[cfg(test)]
            compilations: 0,
            #[cfg(test)]
            hits: 0,
        })
    }

    pub(super) fn validate(&mut self, original: [&'a Performer; SLOTS]) -> Result<(), LuckFamilyError> {
        let current = original.map(|performer| projected(self.context.master, performer));
        if self.previous.as_ref().is_some_and(|previous| {
            previous
                .iter()
                .zip(&current)
                .all(|(performer, current)| projected(self.context.master, performer) == *current)
        }) {
            #[cfg(test)]
            {
                self.hits += 1;
            }
            return Ok(());
        }
        #[cfg(test)]
        {
            self.compilations += 1;
        }
        self.context.admit_projection(&current)?;
        // No temporary model escapes the original validation: only successful absence of a LIFE interpreter
        // is reused. Errors retain the preceding success and never authorize this new pair or a partial family.
        self.previous = Some(original);
        Ok(())
    }
}

#[cfg(test)]
#[path = "admission_reuse_tests.rs"]
mod tests;
