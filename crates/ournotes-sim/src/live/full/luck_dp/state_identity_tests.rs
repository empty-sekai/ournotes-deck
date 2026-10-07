use super::*;

struct DependencyCase {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    deltas: Vec<f32>,
    deck: [Performer; 1],
}

impl DependencyCase {
    fn new(aliased: bool) -> Self {
        let (mut master, _, mut params, _, _, _) = fixture(3, 60);
        let condition = master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
        condition.condition_type = 2001;
        condition.condition_values = vec![700];
        master.skill_targets.push(serde_json::from_value(json!({
            "_id":57,"_skillTargetType":5,"_gekisouMissionType":3
        })).unwrap());
        for (id, kind, values, targets) in [(9001, 7010, vec![], vec![57]), (9002, 7003, vec![2], vec![])] {
            master.skill_conditions.push(serde_json::from_value(json!({
                "_id":id,"_conditionType":kind,"_conditionValues":values,
                "_conditionTargetIDs":targets,"_isPositive":true
            })).unwrap());
            master.skill_condition_sets.push(serde_json::from_value(json!({
                "_id":id,"_group":id,"_conditionIds":[id]
            })).unwrap());
        }
        master.cumulative_conditions.push(serde_json::from_value(json!({
            "_id":9010,"_skillCumulativeConditionType":7000,"_conditionValues":[1],
            "_conditionTargetIDs":[],"_maxCumulativeCount":100
        })).unwrap());
        let mut score = row(1, "_supportSkillID", 901, 2001, 10000, 9001, 0, 0);
        score["_activationTimeSecond"] = json!(10.0);
        let rule_id = if aliased { 1 + (1i64 << 62) } else { 2 };
        let mut rule = row(rule_id, "_supportSkillID", 902, 13002, 1, 9001, 0, 0);
        rule["_activationTimeSecond"] = json!(10.0);
        rule["_skillCumulativeConditionID"] = json!(9010);
        let mut heal = row(3, "_supportSkillID", 903, 3001, 500, 9002, 0, 0);
        heal["_effectExecuteLimitCount"] = json!(1);
        master.support_skill_effects.extend([score, rule, heal].into_iter().map(|row| {
            serde_json::from_value(row).unwrap()
        }));
        for (id, grade, damage) in [(2, 1, 700), (3, 6, 0)] {
            master.judgement_parameters.push(serde_json::from_value(json!({
                "_id":id,"_noteSimulateJudgement":grade,"_scorePercent":100,"_damage":damage
            })).unwrap());
        }
        master.reindex().unwrap();
        let notes: Vec<_> = [50, 150, 1500, 1600, 3000].into_iter().enumerate().map(|(index, time_ms)| {
            LiveNote { note_id: index as i32, time_ms, note_operate_type: 1, judgement_type: 1 }
        }).collect();
        let mut frames: Vec<_> = (0..=42).map(|index| PlayFrame { time_ms: index * 100, judged: Vec::new() }).collect();
        for (note, judgement) in notes.iter().zip([1, 6, 5, 5, 5]) {
            frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
                note_id: note.note_id, judgement, judgement_time_ms: note.time_ms,
            });
        }
        let deltas = vec![0.1; frames.len()];
        params.converted_note_count = notes.len() as i32;
        params.music_length_ms = 4200;
        Self {
            master, notes, params,
            setup: GekisouSetup { fevers: vec![(100, 400), (1400, 1900), (2800, 3200)], missions: vec![3, 2, 1] },
            play: LivePlay { frames, base_seed: 0 }, deltas,
            deck: [Performer {
                support_skills: vec![(901, 1), (902, 1), (903, 1)],
                gekisou_skill: Some((2, 1)), ..Default::default()
            }],
        }
    }

    fn model(&self, master: &Master) -> LiveModel {
        LiveModel::new_gekisou(master, &self.deck, &self.notes, &[], self.params, &self.setup).unwrap()
    }

    fn observations(&self, master: &Master) -> Vec<(i32, i32, [i32; 2])> {
        let mut model = self.model(master);
        model.set_luck_weights(&luck_skills(master).unwrap(), Vec::new()).unwrap();
        model.phase_life = Some([0; 2]);
        self.play.frames.iter().zip(&self.deltas).map(|(frame, &delta)| {
            model.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
            (frame.time_ms, model.gk.as_ref().unwrap().ctrl.states[0].just, model.phase_life.unwrap())
        }).collect()
    }

    fn filtered(&self) -> Master {
        let mut master = self.master.clone();
        master.support_skill_effects.retain(|row| row.skill_effect_type != 2001);
        master.reindex().unwrap();
        master
    }

    fn certified(&self) -> Result<LuckDpCertifiedResult, Error> {
        luck_rush_dp_certified(
            &self.master, &luck_skills(&self.master).unwrap(), &self.notes, self.params, &self.setup,
            &self.play, &self.deltas, &self.deck, None,
        )
    }

    fn bounds(&self) -> Result<super::super::super::luck_score_bounds::LuckScoreBounds, Error> {
        super::super::super::luck_score_bounds::luck_score_bounds(
            &self.master, &self.deck, &self.notes, &[], self.params, &self.setup, &self.play, &self.deltas,
        )
    }
}

#[test]
fn condition_state_aliases_are_part_of_life_dependencies() {
    let input = DependencyCase::new(true);
    let skills = luck_skills(&input.master).unwrap();
    assert!(luck::luck_signature(&input.master, &input.deck[0]).unwrap().is_some());
    let full = input.model(&input.master);
    let expected = Error::Unsupported("LUCK certificates require distinct effect state identities".into());
    assert_eq!(super::super::super::luck_score_bounds::check_recorder(&full, &skills).unwrap_err(), expected);
    assert_eq!(super::super::super::luck_score_bounds::check_conditioned_recorder(&full, &skills).unwrap_err(), expected);
    let first = &full.cond[0].updater;
    let second = &full.cond[1].updater;
    assert_eq!(first.effects()[0].effect_id, 130);
    assert_eq!(first.effects()[0].effect_id, second.effects()[0].effect_id);
    // Separate condition groups may share the same wrapped key and pooled instance index.
    assert_eq!(first.updaters.last().unwrap().index, second.updaters.last().unwrap().index);
    let mut reduced_deck = input.deck.clone();
    reduced_deck[0].support_skills.clear();
    let mut reduced = LiveModel::build(
        &input.master, &reduced_deck, &input.notes, &[], input.params, Some(&input.setup), false, None, Some(&skills),
    ).unwrap();
    assert!(compile::<ProbabilityMass>(&input.master, &mut reduced, &skills, None, false).is_ok());

    let actual = input.observations(&input.master);
    let filtered = input.observations(&input.filtered());
    assert_eq!(actual.iter().find(|row| row.0 == 200).unwrap().1, 1);
    assert_eq!(filtered.iter().find(|row| row.0 == 200).unwrap().1, 2);
    assert_eq!(actual.iter().find(|row| row.0 == 1400).unwrap().2, [300, 300]);
    assert_eq!(filtered.iter().find(|row| row.0 == 1400).unwrap().2, [800, 800]);

    assert_eq!(input.certified().unwrap_err(), expected);
    assert_eq!(luck_rush_dp(
        &input.master, &skills, &input.notes, input.params, &input.setup, &input.play, &input.deltas, &input.deck, None,
    ).unwrap_err(), expected);
    assert_eq!(input.bounds().unwrap_err(), expected);
}

#[test]
fn distinct_condition_states_preserve_life_dependent_lottery_curves() {
    let input = DependencyCase::new(false);
    let skills = luck_skills(&input.master).unwrap();
    assert!(super::super::super::luck_score_bounds::check_recorder(&input.model(&input.master), &skills).is_ok());
    let actual = input.observations(&input.master);
    assert_eq!(actual, input.observations(&input.filtered()));
    assert_eq!(actual.iter().find(|row| row.0 == 1400).unwrap().2, [800, 800]);
    let certified = input.certified().unwrap();
    let nominal = luck_rush_dp(
        &input.master, &luck_skills(&input.master).unwrap(), &input.notes, input.params, &input.setup,
        &input.play, &input.deltas, &input.deck, None,
    ).unwrap();
    assert_certified_encloses_nominal(&certified, &nominal);
    assert_eq!(input.bounds().unwrap().exact_final_life, Some(800));
}

#[test]
fn score_bounds_require_distinct_condition_states_without_life_predicates() {
    let mut input = DependencyCase::new(true);
    let condition = input.master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
    condition.condition_type = 4011;
    condition.condition_values = vec![100];
    input.master.reindex().unwrap();
    assert_eq!(
        input.bounds().unwrap_err(),
        Error::Unsupported("LUCK certificates require distinct effect state identities".into())
    );
}

#[test]
fn unheld_levels_and_support_rows_keep_the_selected_state_namespace() {
    let mut input = DependencyCase::new(false);
    let mut unheld = input.master.support_skill_effects.iter().find(|row| row.support_skill_id == 902).unwrap().clone();
    unheld.id = 1 + (1i64 << 62);
    unheld.level = 2;
    input.master.support_skill_effects.push(unheld);
    input.master.reindex().unwrap();
    assert!(check_held_state_identities(&input.master, &input.deck).is_ok());
    assert!(check_model_state_identities(&input.model(&input.master)).is_ok());
    assert!(input.certified().is_ok());
}
