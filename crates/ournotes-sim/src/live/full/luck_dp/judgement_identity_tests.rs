use super::*;

const CONVERTER: i64 = 9810;
const RECOVERY: i64 = 9811;
const GREAT: i64 = 9812;
const PERFECT: i64 = 9813;

#[derive(Clone)]
struct Input {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    deltas: Vec<f32>,
    deck: Vec<Performer>,
}

impl Input {
    fn new() -> Self {
        let (mut master, notes, params, setup, play, deltas) = random_fixture();
        for (id, judgement) in [(GREAT, 4), (PERFECT, 5)] {
            master.skill_targets.push(
                serde_json::from_value(json!({
                    "_id":id,"_skillTargetType":4,"_judgement":judgement
                }))
                .unwrap(),
            );
        }
        for grade in [1, 4, 6] {
            master.judgement_parameters.push(
                serde_json::from_value(json!({
                    "_id":100+grade,"_noteSimulateJudgement":grade,"_scorePercent":100,"_damage":10
                }))
                .unwrap(),
            );
            master.gekisou_luck_base_points.push(
                serde_json::from_value(json!({
                    "_id":100+grade,"_noteCategory":0,"_noteSimulateJudgement":grade,"_weight":1,"_basePoint":30
                }))
                .unwrap(),
            );
        }
        let mut converter = row(CONVERTER, "_supportSkillID", CONVERTER, 12006, 5, 7010, 0, 0);
        converter["_skillTargetIDs"] = json!([GREAT]);
        converter["_activationTimeSecond"] = json!(0.7);
        converter["_effectLimitCount"] = json!(2);
        master.support_skill_effects.push(serde_json::from_value(converter).unwrap());
        master
            .support_skill_effects
            .push(serde_json::from_value(row(RECOVERY, "_supportSkillID", RECOVERY, 3001, 100, 7010, 0, 0)).unwrap());
        master.reindex().unwrap();
        let deck = vec![
            Performer {
                gekisou_skill: Some((2, 1)),
                gekisou_support_skills: vec![(31, 1)],
                support_skills: vec![(CONVERTER, 1), (RECOVERY, 1)],
                ..Default::default()
            },
            Performer { gekisou_skill: Some((1, 1)), ..Default::default() },
            Performer { gekisou_skill: Some((3, 1)), ..Default::default() },
            Performer { gekisou_skill: Some((2, 1)), ..Default::default() },
            Performer { gekisou_skill: Some((1, 1)), ..Default::default() },
        ];
        Self { master, notes, params, setup, play, deltas, deck }
    }

    fn prepared(&self) -> PreparedRecording<ProbabilityMass> {
        prepare_recording(
            &self.master,
            &luck_skills(&self.master).unwrap(),
            &self.notes,
            &[],
            self.params,
            &self.setup,
            &self.play,
            &self.deltas,
            &self.deck,
            None,
            None,
            false,
        )
        .unwrap()
    }

    fn complete_life(&self) -> LiveModel {
        life_recorder(&self.master, &self.deck, &self.notes, &[], self.params, &self.setup, None).unwrap()
    }

    fn transcript(&self, complete_life: bool) -> Vec<u64> {
        let mut prepared = self.prepared();
        if complete_life {
            prepared.life = Some(self.complete_life());
            prepared.life_deck = Some(self.deck.clone());
        }
        record_prepared(prepared, &self.notes, &self.play, &self.deltas, &mut || false).unwrap().unwrap().key().unwrap()
    }

    fn cached(
        &self,
        cache: &mut LuckDpCache,
        recordings: &mut RecordingCache,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Option<std::sync::Arc<LuckDpCertifiedResult>> {
        cache
            .certified_cancellable(
                &self.master,
                &luck_skills(&self.master).unwrap(),
                &self.notes,
                &[],
                self.params,
                &self.setup,
                &self.play,
                &self.deltas,
                &self.deck,
                None,
                None,
                Some(recordings),
                cancelled,
            )
            .unwrap()
    }

    /// The full native interpreter retains all skills and executes its actual registered converters.
    fn native_grades(&self, seed: i32) -> Vec<i32> {
        let mut model =
            LiveModel::new_gekisou(&self.master, &self.deck, &self.notes, &[], self.params, &self.setup).unwrap();
        model.set_seed(seed);
        let mut grades = Vec::new();
        for (frame, &delta) in self.play.frames.iter().zip(&self.deltas) {
            model.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
            grades.extend(model.frame_judgements().iter().map(|&(_, grade, _)| grade));
        }
        grades
    }
}

#[test]
fn unchanged_declared_grades_share_every_native_order_without_life_history() {
    fn visit(input: &mut Input, originals: &[Performer], order: &mut Vec<usize>, used: u8, count: &mut usize) {
        if order.len() < originals.len() {
            for slot in 0..originals.len() {
                if used & (1 << slot) == 0 {
                    order.push(slot);
                    visit(input, originals, order, used | (1 << slot), count);
                    order.pop();
                }
            }
            return;
        }
        input.deck = order.iter().map(|&slot| originals[slot].clone()).collect();
        assert!(luck_has_judgement_conversion(&input.master, &input.deck));
        assert!(input.prepared().life.is_none());
        assert_eq!(input.transcript(false), input.transcript(true), "order={order:?}");
        assert_eq!(input.native_grades(7), vec![5; input.notes.len()]);
        *count += 1;
    }
    let mut input = Input::new();
    let originals = input.deck.clone();
    let mut count = 0;
    visit(&mut input, &originals, &mut Vec::new(), 0, &mut count);
    assert_eq!(count, 120);
}

#[test]
fn a_possible_raw_grade_change_retains_native_conversion_limits() {
    let mut input = Input::new();
    for note in input.play.frames.iter_mut().skip(2).flat_map(|frame| &mut frame.judged) {
        note.judgement = 4;
    }
    assert!(input.prepared().life.is_some());
    assert_eq!(input.transcript(false), input.transcript(true));
    let grades = input.native_grades(7);
    assert_eq!(&grades[..4], &[5, 5, 5, 4], "the actual converter reaches its two-note limit");
    assert!(grades[3..].contains(&4));
}

#[test]
fn raw_conversion_target_aliases_cannot_authorize_identity() {
    let mut input = Input::new();
    input.master.support_skill_effects.iter_mut().find(|row| row.id == CONVERTER).unwrap().skill_target_ids =
        vec![PERFECT];
    // Perfect -> Perfect alone never changes a grade, but initializes the native raw-ID target memo.
    let second = 9820;
    input
        .master
        .gekisou_support_skills
        .push(serde_json::from_value(json!({"_id":second,"_gekisouMissionType":2})).unwrap());
    let mut converter = row(CONVERTER, "_gekisouSupportSkillID", second, 12006, 1, 7010, 0, 0);
    converter["_skillTargetIDs"] = json!([GREAT]);
    converter["_activationTimeSecond"] = json!(0.7);
    converter["_effectLimitCount"] = json!(2);
    input.master.gekisou_support_skill_effects.push(serde_json::from_value(converter).unwrap());
    input.deck[1].gekisou_support_skills.push((second, 1));
    input.master.reindex().unwrap();
    assert!(!unchanged_judgements(&input.complete_life(), &input.play));
    assert!(input.prepared().life.is_some());
    assert_eq!(input.transcript(false), input.transcript(true));
    assert!(input.native_grades(7).contains(&1), "the second converter borrows the first raw-ID target vector");
}

#[test]
fn identity_admission_preserves_native_value_casts_and_note_types() {
    let mut input = Input::new();
    let converter = input.master.support_skill_effects.iter_mut().find(|row| row.id == CONVERTER).unwrap();
    converter.skill_target_ids = vec![PERFECT];
    converter.effect_value = (1i64 << 32) + 5;
    input.master.reindex().unwrap();
    assert!(input.prepared().life.is_none(), "native i64-to-i32 conversion resolves to the unchanged grade");
    input.master.support_skill_effects.iter_mut().find(|row| row.id == CONVERTER).unwrap().effect_value =
        (1i64 << 32) + 1;
    input.master.reindex().unwrap();
    assert!(input.prepared().life.is_some());
    assert!(input.native_grades(7).contains(&1));

    input.master.support_skill_effects.iter_mut().find(|row| row.id == CONVERTER).unwrap().skill_effect_type = 13005;
    input.master.reindex().unwrap();
    assert!(input.prepared().life.is_some(), "Just exclusions are not assumed by the identity certificate");
    assert_eq!(input.native_grades(7), vec![5; input.notes.len()]);
    input.master.live_judgement_timings.push(
        serde_json::from_value(json!({
            "_id":9800,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":0
        }))
        .unwrap(),
    );
    input.master.reindex().unwrap();
    assert!(input.prepared().life.is_some());
    assert!(input.native_grades(7).contains(&6));
}

#[test]
fn unresolved_conversion_targets_keep_the_original_native_error_path() {
    let mut input = Input::new();
    input.master.support_skill_effects.iter_mut().find(|row| row.id == CONVERTER).unwrap().skill_target_ids =
        vec![i64::MAX];
    input.master.reindex().unwrap();
    let prepared = input.prepared();
    assert!(prepared.life.is_some());
    let mut native = prepared.life.unwrap();
    let error = input
        .play
        .frames
        .iter()
        .zip(&input.deltas)
        .find_map(|(frame, &delta)| native.frame_timed(frame.time_ms, &frame.judged, delta).err());
    assert!(error.is_some(), "the held converter's missing target remains a native runtime error");
}

#[test]
fn lottery_life_reads_keep_the_complete_history_even_with_identity_conversion() {
    let mut input = Input::new();
    let condition = input.master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
    condition.condition_type = 2001;
    condition.condition_values = vec![700];
    input.master.judgement_parameters.iter_mut().find(|row| row.note_simulate_judgement == 5).unwrap().damage = 60;
    input.master.reindex().unwrap();
    assert!(unchanged_judgements(&input.complete_life(), &input.play));
    assert!(input.prepared().life.is_some());
    assert_eq!(input.transcript(false), input.transcript(true));
}

#[test]
fn projected_curve_keys_reuse_ordinary_programs_and_keep_cancellation_and_zero_capacity() {
    let input = Input::new();
    let mut different = input.clone();
    different.deck[0].support_skills.retain(|&(id, _)| id != RECOVERY);
    assert!(input.prepared().life.is_none() && different.prepared().life.is_none());
    assert_eq!(RecordingCache::key(&input.prepared()), RecordingCache::key(&different.prepared()));
    let mut cache = LuckDpCache::new(1 << 20);
    let expected = input.cached(&mut cache, &mut RecordingCache::default(), &mut || false).unwrap();
    let before = cache.stats();
    let reused = different.cached(&mut cache, &mut RecordingCache::default(), &mut || false).unwrap();
    assert!(std::sync::Arc::ptr_eq(&expected, &reused));
    assert_eq!(cache.stats().shared_recording_hits, before.shared_recording_hits + 1);
    assert_eq!(cache.stats().life_recording_lookups, 0);

    let mut disabled = LuckDpCache::new(0);
    let separate = different.cached(&mut disabled, &mut RecordingCache::default(), &mut || false).unwrap();
    assert_eq!(curve_words(&expected), curve_words(&separate));
    assert_eq!(disabled.stats().recording_lookups, 0);
    assert_eq!(disabled.stats().shared_recording_lookups, 0);

    let mut checks = 0;
    input
        .cached(&mut LuckDpCache::new(1 << 20), &mut RecordingCache::default(), &mut || {
            checks += 1;
            false
        })
        .unwrap();
    for stop_at in [1, checks / 2, checks] {
        let mut cache = LuckDpCache::new(1 << 20);
        let mut recordings = RecordingCache::default();
        let mut count = 0;
        assert!(
            input
                .cached(&mut cache, &mut recordings, &mut || {
                    count += 1;
                    count == stop_at
                })
                .is_none()
        );
        assert_eq!(recordings.storage.retained(), (0, 0));
        assert_eq!(cache.shared_recordings.retained(), (0, 0));
        let completed = input.cached(&mut cache, &mut recordings, &mut || false).unwrap();
        assert_eq!(curve_words(&completed), curve_words(&expected));
    }
}
