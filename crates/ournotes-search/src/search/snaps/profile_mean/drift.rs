//! Fixed-binding command history under one complete native controller profile.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct PairWork {
    original: [f64; 2],
    fixed: [f64; 2],
    probe: [f64; 2],
    per_run: [f64; 2],
    factor: f64,
}

impl PairWork {
    pub(super) fn new(part: &Contrib) -> Option<Self> {
        let mut work = Self {
            original: [part.cmds, part.ops],
            fixed: [part.cmds_plain, part.ops_plain],
            probe: [0.0; 2],
            per_run: [0.0; 2],
            factor: part.fac,
        };
        for row in &part.rush {
            let counts = [row.cmds, row.ops];
            if row.run_cap && row.terminal_probe(Some(MISSION_LUCK)) {
                for i in 0..2 {
                    work.probe[i] = (work.probe[i] + counts[i]).next_up();
                    let per_run = if i == 0 { row.cmds_per_run } else { row.ops_per_run };
                    work.per_run[i] = (work.per_run[i] + per_run).next_up();
                }
            } else {
                for (fixed, count) in work.fixed.iter_mut().zip(counts) {
                    *fixed = (*fixed + count).next_up();
                }
            }
        }
        work.original
            .into_iter()
            .chain(work.fixed)
            .chain(work.probe)
            .chain(work.per_run)
            .chain([work.factor])
            .all(|value| value.is_finite() && value >= 0.0)
            .then_some(work)
    }

    /// Every eligible row starts at most once per observed direct-probe true run. Fixed-false rows may never
    /// start, which only decreases work. The native profile separately proves common phase and the inactive
    /// tail; ordinary rows, timed/compound triggers and unmatched gates stay in `fixed`. The native full-pair
    /// admission does not grant this capability to probes with reset/limit lifecycles.
    /// Both the old total and the sum of independently bounded components are universal upper bounds.
    fn counts(self, runs: u64) -> Option<[f64; 3]> {
        let mut counts = [0.0, 0.0, self.factor];
        for (i, count) in counts[..2].iter_mut().enumerate() {
            let by_runs = (self.per_run[i] * runs as f64).next_up();
            *count = (self.fixed[i] + self.probe[i].min(by_runs)).next_up().min(self.original[i]);
        }
        counts.iter().all(|value| value.is_finite() && *value >= 0.0).then_some(counts)
    }
}

pub(super) struct DriftTemplate {
    sensitivity: f64,
    chain_extra: f64,
}

impl DriftTemplate {
    pub(super) fn new(live: &SnapLive<'_>) -> Option<Self> {
        // Recompute from the same immutable score settings; telemetry is never proof authority.
        let settings = LiveScoreSettings::from_master(live.master).ok()?;
        let judgement = settings.judgement_score_factor_percent.values().copied().max()?;
        let judgement = (judgement as f64 / 100.0).next_up();
        Some(Self { sensitivity: factor_error_sensitivity(&live.coef, judgement)?, chain_extra: live.chain_extra })
    }
}

impl ProfileRewardTemplate {
    /// The four fixed bindings and each legal fifth binding keep their own whole 120-order controller law.
    /// No probability scales the command history. Maxima over a pair's five positions bound every shuffle;
    /// summing those maxima only relaxes the distinct-position constraint. No physical resource is replaced.
    pub(crate) fn binding_drift_a0(
        &self,
        members: [usize; 5],
        profile: &FamilyProfileReward,
        choices: &[usize; 5],
    ) -> Option<f64> {
        let runs = profile.max_probe_runs?;
        let template = self.drift.as_ref()?;
        let mut totals = [0.0f64; 3];
        for (slot, &member) in members.iter().enumerate() {
            let &class = self.class_of.get(member)?.get(choices[slot])?;
            let mut maximum = [0.0f64; 3];
            for pair in self.pairs.get(member)?.get(class)? {
                let counts = pair.work?.counts(runs)?;
                for (upper, count) in maximum.iter_mut().zip(counts) {
                    *upper = upper.max(count);
                }
            }
            for (sum, maximum) in totals.iter_mut().zip(maximum) {
                *sum = (*sum + maximum).next_up();
            }
        }
        let [commands, executions, factor] = totals;
        let drift = factor_drift(executions, commands, factor)?;
        let delta = float_margin::with_chain(drift, template.chain_extra)?;
        let roundings = factor_roundings(executions, commands);
        let (a0, _, _) = additive_joint_envelope(
            profile.base_a0,
            profile.base_a0,
            delta,
            roundings,
            template.sensitivity,
            template.chain_extra,
        )?;
        // The old profile cap remains independently valid even when these relaxed position maxima are wider.
        finite_nonnegative(a0).map(|a0| a0.min(profile.a0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_probe_runs_reduce_only_the_separately_admitted_probe_commands() {
        let mut part = Contrib {
            cmds: 18.0,
            ops: 92.0,
            cmds_plain: 2.0,
            ops_plain: 12.0,
            fac: 0.7,
            rush: vec![rush::test_probe(0.7)],
            ..Default::default()
        };
        let reduced = PairWork::new(&part).unwrap().counts(2).unwrap();
        assert!((6.0..6.000001).contains(&reduced[0]));
        assert!((32.0..32.000001).contains(&reduced[1]));
        assert_eq!(reduced[2], 0.7);
        assert_eq!(PairWork::new(&part).unwrap().counts(100).unwrap(), [18.0, 92.0, 0.7]);
        part.rush[0].run_cap = false;
        assert_eq!(PairWork::new(&part).unwrap().counts(0).unwrap(), [18.0, 92.0, 0.7]);
        part.rush[0].run_cap = true;
        part.rush[0].effect_type = 2004;
        assert_eq!(PairWork::new(&part).unwrap().counts(0).unwrap(), [18.0, 92.0, 0.7]);
        part.rush[0].effect_type = 2000;
        part.rush[0].judge[0] = 0.1;
        assert_eq!(PairWork::new(&part).unwrap().counts(0).unwrap(), [18.0, 92.0, 0.7]);
        part.ops = f64::NAN;
        assert!(PairWork::new(&part).is_none());
    }
}
