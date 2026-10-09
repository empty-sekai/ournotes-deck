//! A positive conditional basis for all range-start minimum guarantees in one native live.
//!
//! Conditioning on the maximum guarantee at each start leaves the complete remaining controller
//! program unchanged. No range reset is assumed: each term propagates the whole live, including the
//! native COMPLETE/FINISH tails. The weights describe independent nominal conditions, not finite seeds.
use super::*;

const MAX_STARTS: usize = 3;
const MAX_TERMS: usize = 64;
const MAX_MIX_BYTES: usize = 32 << 20;
const BASIS_DOMAIN: u64 = 0x6d696e6261733031;

#[derive(Clone, Copy, Debug)]
struct MinimumTerm {
    choices: [u8; MAX_STARTS],
    weight: ProbabilityMass,
}

/// One admitted native recording with its minimum operators separated into a bounded conditional
/// basis. Each term retains every non-minimum action in its original order and every native frame,
/// note, lottery table and observer. The original chance and row multiplicity affect only weights.
/// There is one owned transcript, mutated sequentially when a term is selected; no transcript is
/// cloned per term. External words or decoded curves cannot construct this capability.
#[derive(Debug)]
pub struct CompiledLuckMinimumBasis {
    program: CompiledLuckProgram,
    slots: [usize; MAX_STARTS],
    starts: usize,
    minimum_actions: usize,
    terms: Vec<MinimumTerm>,
    frame_count: usize,
}

/// A native evaluation of one conditional program. Its complete identity is retained so another
/// admitted basis can reuse it only after full equality. Inspection or serialization of its curve
/// does not provide a constructor for this native response capability.
#[derive(Clone, Debug)]
pub struct LuckMinimumTermResponse {
    identity: Vec<u64>,
    curve: LuckDpCertifiedResult,
}

impl LuckMinimumTermResponse {
    pub fn identity_words(&self) -> &[u64] {
        &self.identity
    }

    pub fn curve(&self) -> &LuckDpCertifiedResult {
        &self.curve
    }

    pub(crate) fn into_curve(self) -> LuckDpCertifiedResult {
        self.curve
    }

    /// Actual retained vector capacities, excluding allocator bookkeeping and caller-owned keys.
    pub fn allocated_bytes(&self) -> Option<usize> {
        size_of::<Self>()
            .checked_add(self.identity.capacity().checked_mul(size_of::<u64>())?)?
            .checked_add(curve_vector_bytes(&self.curve)?)
    }
}

fn curve_vector_bytes(curve: &LuckDpCertifiedResult) -> Option<usize> {
    let mut bytes = 0usize;
    for (capacity, width) in [
        (curve.steps.capacity(), size_of::<(i32, [ProbabilityMass; 4])>()),
        (curve.probe_transitions.capacity(), size_of::<u8>()),
        (curve.probes.capacity(), size_of::<bool>()),
        (curve.range_moments.capacity(), size_of::<LuckRangeMoments>()),
    ] {
        bytes = bytes.checked_add(capacity.checked_mul(width)?)?;
    }
    Some(bytes)
}

fn max_law(actions: &[Action<ProbabilityMass>]) -> Result<[ProbabilityMass; 4], Error> {
    let mut law = [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO];
    for &action in actions {
        let Action::StartMinimum { result, chance } = action else { continue };
        if !(1..=3).contains(&result) {
            return Err(Error::Unsupported("LUCK minimum basis: unknown minimum result".into()));
        }
        let mut next = [ProbabilityMass::ZERO; 4];
        for (minimum, weight) in law.into_iter().enumerate() {
            let enabled = minimum.max(result as usize);
            if enabled == minimum {
                // Both condition outcomes preserve this state; keep their exact partition sum one
                // instead of widening it by independently enclosing q and 1-q.
                next[minimum] = next[minimum].merge_disjoint(weight);
                continue;
            }
            next[enabled] = next[enabled].merge_disjoint(weight.multiply(chance));
            next[minimum] = next[minimum].merge_disjoint(weight.multiply(chance.complement()));
        }
        law = next;
    }
    Ok(law)
}

fn fixed_minimum_terms(
    starts: usize,
    actions: &[Action<ProbabilityMass>],
    max_terms: usize,
) -> Result<Vec<MinimumTerm>, Error> {
    let allowance = max_terms.min(MAX_TERMS);
    if allowance == 0 || starts > MAX_STARTS {
        return Err(Error::Capacity("LUCK minimum family has no valid term allowance".into()));
    }
    let law = max_law(actions)?;
    let support = law.iter().filter(|weight| weight.interval().upper() > 0.0).count();
    let mut terms = vec![MinimumTerm { choices: [0; MAX_STARTS], weight: ProbabilityMass::ONE }];
    for start in 0..starts {
        let count = terms
            .len()
            .checked_mul(support)
            .filter(|&count| count <= allowance)
            .ok_or_else(|| Error::Capacity("LUCK minimum family exceeds its term allowance".into()))?;
        let mut next = Vec::new();
        // Charge one fixed maximum-sized support allocation to every retained family. Subsequent
        // reweighting cannot grow its retained payload beyond the initial cache admission.
        next.try_reserve_exact(allowance).map_err(|_| Error::Capacity("LUCK minimum family term allocation".into()))?;
        for term in &terms {
            for (minimum, weight) in law.into_iter().enumerate() {
                if weight.interval().upper() == 0.0 {
                    continue;
                }
                let mut selected = *term;
                selected.choices[start] = minimum as u8;
                selected.weight = selected.weight.multiply(weight);
                next.push(selected);
            }
        }
        debug_assert_eq!(next.len(), count);
        terms = next;
    }
    Ok(terms)
}

impl CompiledLuckProgram {
    /// Separate a complete, originally ordered native recording into whole-live conditional programs.
    /// The allowance is capped at 64 terms, and the native game's three starts remain an explicit
    /// limit. Additive moments and already-canonicalized recordings are refused. No DP runs here.
    pub fn start_minimum_basis(self, max_terms: usize) -> Result<CompiledLuckMinimumBasis, Error> {
        CompiledLuckMinimumBasis::new(self, max_terms)
    }
}

impl CompiledLuckMinimumBasis {
    fn new(mut program: CompiledLuckProgram, max_terms: usize) -> Result<Self, Error> {
        if program.transcript.collect_moments {
            return Err(Error::Unsupported("LUCK minimum basis does not collect additive moments".into()));
        }
        if program.canonical_minimum.is_some() {
            return Err(Error::Unsupported("LUCK minimum basis requires the original ordered action recording".into()));
        }
        if let Some(error) = &program.transcript.failure {
            return Err(error.clone());
        }
        let allowance = max_terms.min(MAX_TERMS);
        if allowance == 0 {
            return Err(Error::Capacity("LUCK minimum basis has no term allowance".into()));
        }
        let mut slots = [0; MAX_STARTS];
        let (mut starts, mut minimum_actions, mut frame_count) = (0usize, 0usize, 0usize);
        let mut terms = vec![MinimumTerm { choices: [0; MAX_STARTS], weight: ProbabilityMass::ONE }];
        let original = std::mem::take(&mut program.transcript.actions);
        let mut rewritten = Vec::with_capacity(original.len().saturating_add(MAX_STARTS));
        let mut begin = 0usize;
        for frame in &mut program.transcript.frames {
            frame_count = frame_count
                .checked_add(frame.repeat as usize)
                .ok_or_else(|| Error::Capacity("LUCK minimum basis frame count overflow".into()))?;
            let end = frame.actions;
            let actions = original
                .get(begin..end)
                .ok_or_else(|| Error::Input("LUCK minimum basis has invalid native action offsets".into()))?;
            let mut minimum_count = 0usize;
            for &action in actions {
                match action {
                    Action::StartMinimum { .. } => minimum_count += 1,
                    Action::StartGauge { .. } | Action::MissGauge { .. } => rewritten.push(action),
                    Action::CriticalPoints { .. } | Action::StartPoints { .. } => {
                        return Err(Error::Unsupported(
                            "LUCK minimum basis encountered an additive-point action".into(),
                        ));
                    }
                }
            }
            if minimum_count != 0 && frame.start.is_none() {
                return Err(Error::Unsupported(
                    "LUCK minimum basis found a guarantee outside a native start frame".into(),
                ));
            }
            minimum_actions = minimum_actions
                .checked_add(minimum_count)
                .ok_or_else(|| Error::Capacity("LUCK minimum basis action count overflow".into()))?;
            if frame.start.is_some() {
                if frame.repeat != 1 || starts == MAX_STARTS {
                    return Err(Error::Unsupported(
                        "LUCK minimum basis needs at most three unrepeated start frames".into(),
                    ));
                }
                // All these independent actions finish before any note/pending draw. Minimum writes
                // only state.minimum; Gauge/Miss read and write chain plus Miss eligibility flags.
                // Those flags are committed after all actions, so moving minimum to the end commutes.
                let law = max_law(actions)?;
                let support = law.iter().filter(|weight| weight.interval().upper() > 0.0).count();
                let count = terms
                    .len()
                    .checked_mul(support)
                    .filter(|&count| count <= allowance)
                    .ok_or_else(|| Error::Capacity("LUCK minimum basis exceeds its term allowance".into()))?;
                let mut next = Vec::with_capacity(count);
                for term in &terms {
                    for (minimum, weight) in law.into_iter().enumerate() {
                        if weight.interval().upper() == 0.0 {
                            continue;
                        }
                        let mut selected = *term;
                        selected.choices[starts] = minimum as u8;
                        selected.weight = selected.weight.multiply(weight);
                        next.push(selected);
                    }
                }
                terms = next;
                slots[starts] = rewritten.len();
                starts += 1;
                // Include a placeholder even for a start with no guarantee. This makes the fully
                // conditioned base and a zero-outcome skill term share the same complete program.
                rewritten.push(Action::StartMinimum { result: 0, chance: ProbabilityMass::ONE });
            }
            begin = end;
            frame.actions = rewritten.len();
        }
        if begin != original.len() {
            return Err(Error::Input("LUCK minimum basis has unframed native actions".into()));
        }
        program.transcript.actions = rewritten;
        Ok(Self { program, slots, starts, minimum_actions, terms, frame_count })
    }

    pub fn start_count(&self) -> usize {
        self.starts
    }

    pub fn minimum_action_count(&self) -> usize {
        self.minimum_actions
    }

    pub(super) fn matches_fixed_minimum_actions(
        &self,
        actions: &[Action<ProbabilityMass>],
        max_terms: usize,
    ) -> Result<bool, Error> {
        if actions.len().checked_mul(self.starts) != Some(self.minimum_actions) {
            return Ok(false);
        }
        let terms = fixed_minimum_terms(self.starts, actions, max_terms)?;
        Ok(terms.len() == self.terms.len()
            && terms.iter().zip(&self.terms).all(|(a, b)| a.choices == b.choices && a.weight == b.weight))
    }

    pub(super) fn reweight_fixed_minimum_actions(
        &mut self,
        actions: &[Action<ProbabilityMass>],
        max_terms: usize,
    ) -> Result<(), Error> {
        let terms = fixed_minimum_terms(self.starts, actions, max_terms)?;
        let count = actions
            .len()
            .checked_mul(self.starts)
            .ok_or_else(|| Error::Capacity("LUCK minimum family action count overflow".into()))?;
        let allowance = max_terms.min(MAX_TERMS);
        if self.terms.capacity() < allowance {
            self.terms
                .try_reserve_exact(allowance.saturating_sub(self.terms.len()))
                .map_err(|_| Error::Capacity("LUCK minimum family retained term allocation".into()))?;
        }
        // Reuse the initially charged buffer. An allocator's excess capacity on a temporary PMF
        // allocation cannot grow the retained family on a later cache hit.
        self.terms.clear();
        self.terms.extend(terms);
        self.minimum_actions = count;
        Ok(())
    }

    pub fn term_count(&self) -> usize {
        self.terms.len()
    }

    fn term(&self, index: usize) -> Result<&MinimumTerm, Error> {
        self.terms.get(index).ok_or_else(|| Error::Input("LUCK minimum basis term index is outside its support".into()))
    }

    pub fn term_weight(&self, index: usize) -> Result<ProbabilityMass, Error> {
        Ok(self.term(index)?.weight)
    }

    pub fn term_choices(&self, index: usize) -> Result<&[u8], Error> {
        Ok(&self.term(index)?.choices[..self.starts])
    }

    fn select(&mut self, index: usize) -> Result<(), Error> {
        let choices = self.term(index)?.choices;
        for (&slot, &minimum) in self.slots[..self.starts].iter().zip(&choices) {
            self.program.transcript.actions[slot] =
                Action::StartMinimum { result: minimum as i8, chance: ProbabilityMass::ONE };
        }
        Ok(())
    }

    /// Full conditioned-program identity, including the deterministic selected minima. Row counts,
    /// skill labels and original probabilities are absent; they do not affect a conditional curve.
    pub fn term_identity_words(&mut self, index: usize) -> Result<Vec<u64>, Error> {
        self.select(index)?;
        let mut identity = self
            .program
            .identity_words()
            .ok_or_else(|| Error::Input("LUCK minimum basis has no complete native identity".into()))?;
        identity.extend([BASIS_DOMAIN, self.starts as u64]);
        for &slot in &self.slots[..self.starts] {
            let normalized = self
                .program
                .identity_action_slot(slot)
                .ok_or_else(|| Error::Input("LUCK minimum basis has no canonical action slot".into()))?;
            identity.push(normalized as u64);
        }
        Ok(identity)
    }

    /// One fresh DP over the whole conditioned native live, with no recording or admission repeated.
    pub fn certified_term(&mut self, index: usize) -> Result<LuckMinimumTermResponse, Error> {
        let identity = self.term_identity_words(index)?;
        let curve = self.program.certified()?;
        Ok(LuckMinimumTermResponse { identity, curve })
    }

    pub fn observer_contract(&self) -> &'static str {
        self.program.observer_contract()
    }

    pub fn operator_contract(&self) -> &'static str {
        if self.program.operator_contract() == "canonical-miss-gauge-deltas/1" {
            "whole-live-start-minimum-basis+canonical-miss-gauge-deltas/1"
        } else {
            "whole-live-start-minimum-basis/1"
        }
    }

    /// Reconstruct the independent nominal law from complete native responses, including responses
    /// reused from another admitted basis. Every full term identity must match before its curve is
    /// accepted. The result's peak/transition fields describe conditional graph work; they need not
    /// equal a direct propagation's execution counts. No whole-score certificate is produced.
    pub fn reconstruct(&mut self, responses: &[&LuckMinimumTermResponse]) -> Result<LuckDpCertifiedResult, Error> {
        if responses.len() != self.terms.len() {
            return Err(Error::Input("LUCK minimum basis needs exactly one response per positive term".into()));
        }
        for (index, response) in responses.iter().enumerate() {
            if response.identity != self.term_identity_words(index)? {
                return Err(Error::Input("LUCK minimum basis response has a different complete program".into()));
            }
        }
        let curves: Vec<_> = responses.iter().map(|response| &response.curve).collect();
        self.reconstruct_validated(&curves, &self.program.transcript.probes)
    }

    /// The chartstats wrapper first checks its opaque complete multi-batch identities. It may then
    /// pass its merged native curves here with the fully covered probe flags. This is not a decoder.
    pub(crate) fn reconstruct_validated(
        &self,
        curves: &[&LuckDpCertifiedResult],
        probes: &[bool],
    ) -> Result<LuckDpCertifiedResult, Error> {
        if curves.len() != self.terms.len() {
            return Err(Error::Input("LUCK minimum basis has incomplete conditional responses".into()));
        }
        if self.frame_count.checked_add(probes.len()).is_none_or(|bytes| bytes > MAX_MIX_BYTES) {
            return Err(Error::Capacity("LUCK minimum basis frame/probe metadata exceeds 32 MiB".into()));
        }
        let mut result = LuckDpCertifiedResult {
            probe_transitions: vec![0; self.frame_count],
            steps: Vec::new(),
            probes: probes.to_vec(),
            range_moments: Vec::new(),
            peak_states: 0,
            transitions: 0,
        };
        let mut total = ProbabilityMass::ZERO;
        for (term, &curve) in self.terms.iter().zip(curves) {
            self.validate_curve(curve, probes)?;
            total = total.merge_disjoint(term.weight);
            result.steps = merge_steps(&result.steps, &curve.steps, term.weight, self.program.transcript.notes.len())?;
            for (target, &mask) in result.probe_transitions.iter_mut().zip(&curve.probe_transitions) {
                *target |= mask;
            }
            result.peak_states = result.peak_states.max(curve.peak_states);
            result.transitions = result
                .transitions
                .checked_add(curve.transitions)
                .ok_or_else(|| Error::Capacity("LUCK minimum basis transition work overflow".into()))?;
            if curve_vector_bytes(&result)
                .and_then(|bytes| bytes.checked_add(size_of::<LuckDpCertifiedResult>()))
                .is_none_or(|bytes| bytes > MAX_MIX_BYTES)
            {
                return Err(Error::Capacity("LUCK minimum basis reconstructed curve exceeds 32 MiB".into()));
            }
        }
        if !total.interval().contains(1.0) {
            return Err(Error::Input("LUCK minimum basis term masses do not enclose a complete law".into()));
        }
        Ok(result)
    }

    fn validate_curve(&self, curve: &LuckDpCertifiedResult, probes: &[bool]) -> Result<(), Error> {
        let notes = &self.program.transcript.notes;
        if !curve.range_moments.is_empty()
            || curve.probes != probes
            || curve.probe_transitions.len() != self.frame_count
            || curve.probe_transitions.iter().any(|&mask| mask == 0 || mask & !15 != 0)
            || curve.peak_states == 0
            || curve.peak_states > 2_000_000
            || curve.steps.first().map(|step| step.0) != notes.first().map(|note| note.time_ms)
            || curve.steps.len() > notes.len()
        {
            return Err(Error::Input("LUCK minimum basis response geometry or metadata differs".into()));
        }
        let mut previous = None;
        for &(time, masses) in &curve.steps {
            if previous.is_some_and(|previous| time <= previous)
                || notes.binary_search_by_key(&time, |note| note.time_ms).is_err()
                || !masses.iter().fold(F64Interval::ZERO, |sum, mass| sum.add(mass.interval())).contains(1.0)
            {
                return Err(Error::Input("LUCK minimum basis response has invalid note probabilities".into()));
            }
            previous = Some(time);
        }
        Ok(())
    }

    /// Actual retained program and small term-vector capacities. This excludes identity buffers,
    /// supplied curves, reconstructed output and the native DP's separate workspace.
    pub fn allocated_bytes(&self) -> Option<usize> {
        size_of::<Self>()
            .checked_add(self.program.allocated_bytes()?.checked_sub(size_of::<CompiledLuckProgram>())?)?
            .checked_add(self.terms.capacity().checked_mul(size_of::<MinimumTerm>())?)
    }
}

fn merge_steps(
    accumulated: &[(i32, [ProbabilityMass; 4])],
    curve: &[(i32, [ProbabilityMass; 4])],
    weight: ProbabilityMass,
    maximum_steps: usize,
) -> Result<Vec<(i32, [ProbabilityMass; 4])>, Error> {
    let capacity = accumulated.len().saturating_add(curve.len()).min(maximum_steps);
    if capacity.checked_mul(size_of::<(i32, [ProbabilityMass; 4])>()).is_none_or(|bytes| bytes > MAX_MIX_BYTES) {
        return Err(Error::Capacity("LUCK minimum basis union-time workspace exceeds 32 MiB".into()));
    }
    let mut out = Vec::with_capacity(capacity);
    let (mut a, mut b) = (0usize, 0usize);
    let (mut left, mut right) = ([ProbabilityMass::ZERO; 4], [ProbabilityMass::ZERO; 4]);
    while a < accumulated.len() || b < curve.len() {
        let time = match (accumulated.get(a), curve.get(b)) {
            (Some(a), Some(b)) => a.0.min(b.0),
            (Some(a), None) => a.0,
            (None, Some(b)) => b.0,
            (None, None) => unreachable!(),
        };
        if accumulated.get(a).is_some_and(|step| step.0 == time) {
            left = accumulated[a].1;
            a += 1;
        }
        if curve.get(b).is_some_and(|step| step.0 == time) {
            right = curve[b].1.map(|mass| mass.multiply(weight));
            b += 1;
        }
        let joint = std::array::from_fn(|i| left[i].merge_disjoint(right[i]));
        if out.last().is_none_or(|last: &(i32, [ProbabilityMass; 4])| last.1 != joint) {
            out.push((time, joint));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimum(result: i8, probability: f32) -> Action<ProbabilityMass> {
        Action::StartMinimum { result, chance: ProbabilityMass::from_f32(probability).unwrap() }
    }

    #[test]
    fn minimum_basis_max_law_keeps_joint_outcomes_and_ten_small_chances() {
        let law = max_law(&[minimum(1, 0.25), minimum(3, 0.50), minimum(2, 0.75)]).unwrap();
        for (mass, numerator) in law.into_iter().zip([6, 2, 24, 32]) {
            assert!(mass.interval().contains(f64::from(numerator) / 64.0));
        }
        let law = max_law(&[minimum(2, 0.01); 10]).unwrap();
        assert!(law[0].interval().upper() > 0.0);
        assert_eq!(law[1], ProbabilityMass::ZERO);
        assert!(law[2].interval().upper() > 0.0);
        assert_eq!(law[3], ProbabilityMass::ZERO);
        assert!(law.iter().fold(F64Interval::ZERO, |sum, mass| sum.add(mass.interval())).contains(1.0));
    }

    #[test]
    fn minimum_basis_union_times_forward_fill_and_do_not_zip_curves() {
        let neither = [ProbabilityMass::ONE, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO];
        let both = [ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ZERO, ProbabilityMass::ONE];
        let half = ProbabilityMass::from_f32(0.5).unwrap();
        let first = merge_steps(&[], &[(0, neither), (20, both)], half, 4).unwrap();
        let mixed = merge_steps(&first, &[(0, neither), (10, both), (30, neither)], half, 4).unwrap();
        assert_eq!(mixed.iter().map(|step| step.0).collect::<Vec<_>>(), [0, 10, 20, 30]);
        for (index, expected) in [0.0, 0.5, 1.0, 0.5].into_iter().enumerate() {
            assert!(mixed[index].1[3].interval().contains(expected));
            assert!(mixed[index].1[0].interval().contains(1.0 - expected));
        }
    }
}
