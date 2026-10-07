use super::*;

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

    /// Both held probability gates have rate 1/2 and are checked once. Fix their four Boolean leaves
    /// independently while preserving every effect row, owner and updater pool. The lottery tables
    /// are single-outcome, so they introduce no additional leaf.
    fn native_fixed_gate_leaves(&self, deck: &[Performer]) -> (u64, u64) {
        assert!(self.master.gekisou_luck_bonus_lots.iter().all(|row| row.lot_result == 3));
        let (mut completed, mut duplicate) = (0u64, 0u64);
        for leaf in 0..4 {
            let mut master = self.master.clone();
            let mut fixed_deck = deck.to_vec();
            let mut gates = Vec::new();
            for performer in &mut fixed_deck {
                for (id, level) in &mut performer.gekisou_support_skills {
                    let mut skill = self.master.gekisou_support_skills.iter().find(|row| row.id == *id).unwrap().clone();
                    let rows: Vec<_> = self.master.gekisou_support_skill_effects.iter()
                        .filter(|row| row.skill_id == *id && row.level == *level).cloned().collect();
                    assert_eq!(rows.len(), 1);
                    let mut effect = rows[0].clone();
                    let rate = self.master.skill_condition(effect.skill_condition_group).unwrap();
                    assert_eq!((rate.condition_type, rate.condition_values.as_slice()), (4011, &[50][..]));
                    let index = gates.len();
                    let hit = leaf & (1 << index) != 0;
                    let group = 9000 + index as i64;
                    skill.id = 10000 + index as i64;
                    *id = skill.id;
                    effect.skill_id = skill.id;
                    effect.skill_condition_group = group;
                    master.gekisou_support_skills.push(skill);
                    master.gekisou_support_skill_effects.push(effect);
                    master.skill_conditions.push(serde_json::from_value(json!({
                        "_id":group,"_conditionType":4011,"_conditionValues":[if hit { 100 } else { 0 }],
                        "_conditionTargetIDs":[],"_isPositive":true
                    })).unwrap());
                    master.skill_condition_sets.push(serde_json::from_value(json!({
                        "_id":group,"_group":group,"_conditionIds":[group]
                    })).unwrap());
                    gates.push((group, hit));
                }
            }
            assert_eq!(gates.len(), 2);
            master.reindex().unwrap();
            let mut fixed = master.clone();
            for &(group, hit) in &gates {
                let condition = fixed.skill_conditions.iter_mut().find(|row| row.id == group).unwrap();
                condition.condition_type = 8000;
                condition.condition_values.clear();
                condition.is_positive = !hit;
            }
            fixed.reindex().unwrap();
            let mut model = LiveModel::new_gekisou(
                &master, &fixed_deck, &self.notes, &[], self.params, &self.setup,
            ).unwrap();
            let mut known = LiveModel::new_gekisou(
                &fixed, &fixed_deck, &self.notes, &[], self.params, &self.setup,
            ).unwrap();
            let result = model.run_timed(&self.play, &self.deltas);
            let fixed_result = known.run_timed(&self.play, &self.deltas);
            assert_eq!(result, fixed_result);
            assert_eq!(model.frames_played(), known.frames_played());
            assert_eq!(model.current_life(), known.current_life());
            // Single-outcome LUCK consumes the same draws in both runs. Each rate gate contributes
            // exactly one SKILL comparison; the matching fixed predicate consumes none.
            assert_eq!(model.draws() - known.draws(), 2);
            match result {
                Ok(_) => {
                    assert!(model.score() > 0);
                    completed += 1;
                }
                Err(failure) => {
                    assert_eq!(failure, Error::Game("effect state registered twice".into()));
                    assert_eq!(leaf, 3);
                    duplicate += 1;
                }
            }
        }
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
    assert_eq!(input.native_fixed_gate_leaves(&deck), (3, 1));

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
    assert!(matches!(&expected, Error::Unsupported(reason) if reason.contains("distinct effect state identities")));
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
            assert_eq!(after.recording_hits, before.recording_hits);
            assert_eq!(after.lookups, before.lookups);
            assert_eq!(after.hits, before.hits);
            assert_eq!(after.peak_entries, before.peak_entries);
        }
    }
}

#[test]
fn certified_curves_keep_equal_raw_ids_in_distinct_source_and_member_scopes() {
    let mut input = IdentityCase::new();
    input.master.gekisou_skill_effects.iter_mut().find(|row| row.skill_id == 3).unwrap().id = 66;
    input.master.reindex().unwrap();
    let deck = [holder(vec![(66, 1)]), holder(vec![(66, 1)])];
    assert_eq!(input.native_fixed_gate_leaves(&deck), (4, 0));
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
