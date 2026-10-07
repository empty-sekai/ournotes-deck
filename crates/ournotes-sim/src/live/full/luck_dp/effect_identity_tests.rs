use super::*;
use crate::live::random::LiveRandom;

struct IdentityCase {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    deltas: Vec<f32>,
}

impl IdentityCase {
    fn new() -> Self {
        let (mut master, notes, params, setup, play, deltas) = fixture(3, 60);
        master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id: 67,
            gekisou_mission_type: M_LUCK,
            ..Default::default()
        });
        let mut second = master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 66).unwrap().clone();
        second.skill_id = 67;
        // Each source has one row; only their actual held execution keys collide.
        master.gekisou_support_skill_effects.push(second);
        master.reindex().unwrap();
        Self { master, notes, params, setup, play, deltas }
    }

    fn fresh(&self, deck: &[Performer]) -> LiveModel {
        LiveModel::new_gekisou(&self.master, deck, &self.notes, &[], self.params, &self.setup).unwrap()
    }

    fn cached(
        &self,
        deck: &[Performer],
        curves: &mut LuckDpCache,
        recordings: Option<&mut RecordingCache>,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        curves.certified_cancellable(
            &self.master,
            &luck_skills(&self.master).unwrap(),
            &self.notes,
            &[],
            self.params,
            &self.setup,
            &self.play,
            &self.deltas,
            deck,
            None,
            None,
            recordings,
            &mut || false,
        )
    }

    /// Enumerate fresh native executions from the nominal draw sites, including terminal errors. The
    /// two Bernoulli draws form four equiprobable leaves; deterministic lotteries create no extra choice.
    fn native_mass_quarters(&self, deck: &[Performer]) -> (u64, u64) {
        let mut pending = vec![(Vec::<usize>::new(), 1u64, 1u64)];
        let (mut completed, mut duplicate) = (0u64, 0u64);
        let mut leaves = 0;
        while let Some((prefix, numerator, denominator)) = pending.pop() {
            let mut model = self.fresh(deck);
            model.set_random(LiveRandom::with_nominal_prefix(prefix.clone()));
            let mut error = None;
            for (frame, &delta) in self.play.frames.iter().zip(&self.deltas) {
                if let Err(failure) = model.frame_timed(frame.time_ms, &frame.judged, delta) {
                    error = Some(failure);
                    break;
                }
            }
            assert!(model.random.nominal_covers_draws());
            if let Some(branch) = model.random.nominal_branch() {
                assert!(matches!(error, Some(Error::Unsupported(_))));
                assert!(prefix.len() < 2);
                assert_eq!(branch.len(), 2);
                for (choice, outcome) in branch.iter().enumerate() {
                    assert_eq!((outcome.weight, outcome.total, outcome.value), (1, 2, choice as i64));
                    let mut next = prefix.clone();
                    next.push(choice);
                    pending.push((next, numerator * outcome.weight, denominator * outcome.total));
                }
                continue;
            }
            assert!(model.random.nominal_prefix_consumed());
            assert_eq!(prefix.len(), 2);
            assert_eq!((numerator, denominator), (1, 4));
            leaves += 1;
            match error {
                None => {
                    assert!(model.score() > 0);
                    completed += numerator;
                }
                Some(failure) => {
                    assert_eq!(failure, Error::Game("effect state registered twice".into()));
                    assert_eq!(prefix, [1, 1]);
                    duplicate += numerator;
                }
            }
        }
        assert_eq!(leaves, 4);
        assert_eq!(completed + duplicate, 4);
        (completed, duplicate)
    }
}

fn holder(supports: Vec<(i64, i64)>) -> Performer {
    Performer { gekisou_skill: Some((3, 1)), gekisou_support_skills: supports, ..Default::default() }
}

#[test]
fn certified_curves_reject_cross_updater_identity_errors_before_cache_lookup() {
    let input = IdentityCase::new();
    let deck = [holder(vec![(66, 1), (67, 1)])];
    assert_eq!(input.native_mass_quarters(&deck), (3, 1));

    let skills = luck_skills(&input.master).unwrap();
    let expected = luck_rush_dp_certified(
        &input.master,
        &skills,
        &input.notes,
        input.params,
        &input.setup,
        &input.play,
        &input.deltas,
        &deck,
        None,
    )
    .unwrap_err();
    assert!(matches!(&expected, Error::Unsupported(reason) if reason.contains("repeated condition effect state")));
    let fast = luck_rush_dp(
        &input.master,
        &skills,
        &input.notes,
        input.params,
        &input.setup,
        &input.play,
        &input.deltas,
        &deck,
        None,
    )
    .unwrap_err();
    assert_eq!(fast, expected);

    for capacity in [0, 1 << 20] {
        for local in [false, true] {
            let mut curves = LuckDpCache::new(capacity);
            let mut recordings = RecordingCache::default();
            let valid = [holder(vec![(66, 1)])];
            for _ in 0..2 {
                input.cached(&valid, &mut curves, local.then_some(&mut recordings)).unwrap().unwrap();
            }
            let before = curves.stats();
            for _ in 0..2 {
                let error = input.cached(&deck, &mut curves, local.then_some(&mut recordings)).unwrap_err();
                assert_eq!(error, expected);
            }
            let after = curves.stats();
            assert_eq!(after.recording_lookups, before.recording_lookups);
            assert_eq!(after.shared_recording_lookups, before.shared_recording_lookups);
            assert_eq!(after.lookups, before.lookups);
            assert_eq!(after.propagated_curves, before.propagated_curves);
            assert_eq!(after.transitions, before.transitions);
        }
    }
}

#[test]
fn certified_curves_keep_equal_raw_ids_in_distinct_source_and_member_scopes() {
    let mut input = IdentityCase::new();
    input.master.gekisou_skill_effects.iter_mut().find(|row| row.skill_id == 3).unwrap().id = 66;
    input.master.reindex().unwrap();
    let deck = [holder(vec![(66, 1)]), holder(vec![(66, 1)])];
    assert_eq!(input.native_mass_quarters(&deck), (4, 0));
    let native = input.fresh(&deck);
    let rows: Vec<_> = native.cond.iter().flat_map(|skill| skill.updater.effects()).collect();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|effect| native.rows[effect.row].id == 66));
    let ids: crate::num::FxHashSet<_> = rows.iter().map(|effect| effect.effect_id).collect();
    assert_eq!(ids.len(), 4);
    for capacity in [0, 1 << 20] {
        let mut curves = LuckDpCache::new(capacity);
        let result = input.cached(&deck, &mut curves, None).unwrap().unwrap();
        for (_, joint) in &result.steps {
            let mass = joint.iter().fold(F64Interval::ZERO, |sum, p| sum.add(p.interval()));
            assert!(mass.contains(1.0));
        }
    }
}
