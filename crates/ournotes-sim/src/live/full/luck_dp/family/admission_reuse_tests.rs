use super::*;
use serde_json::json;

struct Fixture {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    deltas: Vec<f32>,
    base: [Performer; SLOTS],
    choices: [Vec<LuckFamilyChoice>; SLOTS],
}

impl Fixture {
    fn new() -> Self {
        let tables = json!({
            "MasterLiveSettings":[
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"},
                {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
                {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
                {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}
            ],
            "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
            "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
            "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
            "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":60}],
            "MasterLiveGekisouLuckBonusLot":(0..5).flat_map(|kind| [0,3].map(move |result| json!({
                "_id":kind*10+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1
            }))).collect::<Vec<_>>(),
            "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2}],
            "MasterGekisouSupportSkill":[
                {"_id":2,"_gekisouMissionType":1},{"_id":3,"_gekisouMissionType":2},
                {"_id":4,"_gekisouMissionType":2},{"_id":5,"_gekisouMissionType":2}
            ],
            "MasterGekisouSkillEffect":[{
                "_id":1,"_gekisouSkillID":1,"_level":1,"_skillEffectType":11001,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_effectValue":5000,"_activationTimeSecond":0.2
            }],
            "MasterGekisouSupportSkillEffect":[
                {"_id":2,"_gekisouSupportSkillID":2,"_level":1,"_skillEffectType":12000,
                 "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_effectValue":1,"_activationTimeSecond":0.2},
                {"_id":3,"_gekisouSupportSkillID":3,"_level":1,"_skillEffectType":11003,
                 "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillReleaseConditionGroup":3,"_effectValue":2500},
                {"_id":4,"_gekisouSupportSkillID":4,"_level":1,"_skillEffectType":11005,
                 "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillReleaseConditionGroup":3,"_effectValue":3,"_effectLimitCount":1},
                {"_id":5,"_gekisouSupportSkillID":5,"_level":1,"_skillEffectType":11003,
                 "_skillTriggerType":1,"_skillTriggerConditionGroup":1,"_skillReleaseConditionGroup":3,"_skillConditionGroup":2,"_effectValue":2500}
            ],
            "MasterSkillConditionSet":[
                {"_id":1,"_group":1,"_conditionIds":[1]},
                {"_id":2,"_group":2,"_conditionIds":[2]},
                {"_id":3,"_group":3,"_conditionIds":[3]}
            ],
            "MasterSkillCondition":[
                {"_id":1,"_conditionType":7010,"_conditionTargetIDs":[1],"_isPositive":true},
                {"_id":2,"_conditionType":2001,"_conditionValues":[900],"_isPositive":true},
                {"_id":3,"_conditionType":7013,"_isPositive":true}
            ],
            "MasterSkillTarget":[{"_id":1,"_skillTargetType":5,"_gekisouMissionType":2}]
        });
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        let master =
            Master::from_json_tables(|name| texts.iter().find(|(n, _)| n == name).map(|(_, text)| text.as_str()))
                .unwrap();
        let notes: Vec<_> = [100, 110, 200, 300, 400, 700]
            .into_iter()
            .enumerate()
            .map(|(id, time_ms)| LiveNote { note_id: id as i32, time_ms, note_operate_type: 1, judgement_type: 1 })
            .collect();
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 1000,
            music_level: 20,
            converted_note_count: notes.len() as i32,
            music_length_ms: 3000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let setup = GekisouSetup { fevers: vec![(100, 500)], missions: vec![2, 2, 2] };
        let mut frames: Vec<_> = (0..=30).map(|i| PlayFrame { time_ms: i * 100, judged: Vec::new() }).collect();
        for note in &notes {
            frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
                note_id: note.note_id,
                judgement: 5,
                judgement_time_ms: note.time_ms,
            });
        }
        let play = LivePlay { frames, base_seed: 0 };
        let deltas = vec![0.1; play.frames.len()];
        let base: [Performer; SLOTS] = std::array::from_fn(|owner| Performer {
            character_id: owner as i64 + 1,
            band_id: 1,
            tag_ids: vec![owner as i64],
            gekisou_skill: Some((1, 1)),
            ..Default::default()
        });
        let choices = std::array::from_fn(|owner| {
            std::iter::once(None)
                .chain((0..12).map(Some))
                .map(|resource| {
                    let mut performer = base[owner].clone();
                    if resource.is_some() {
                        performer.gekisou_support_skills.push((2, 1));
                    }
                    LuckFamilyChoice { resource, performer }
                })
                .collect()
        });
        Self { master, notes, params, setup, play, deltas, base, choices }
    }

    fn context(&self) -> LuckFamilyContext<'_> {
        // The context's original recorder catalogue is immutable and empty for this bounded fixture.
        static SKILLS: std::sync::LazyLock<LuckSkills> = std::sync::LazyLock::new(LuckSkills::default);
        LuckFamilyContext::new(
            &self.master,
            &SKILLS,
            &self.notes,
            &[],
            self.params,
            &self.setup,
            &self.play,
            &self.deltas,
            || false,
        )
        .unwrap()
        .unwrap()
    }
}

fn native(context: &LuckFamilyContext<'_>, projected: &[Performer; SLOTS]) -> PreparedRecording<ProbabilityMass> {
    prepare_recording(
        context.master,
        &context.writer_skills,
        context.notes,
        context.events,
        context.params,
        context.setup,
        context.play,
        context.deltas,
        projected,
        None,
        None,
        false,
    )
    .unwrap()
}

fn working<'a>(
    fixture: &'a Fixture,
    allowed: &'a [Vec<Option<usize>>; SLOTS],
    writers: &'a Vec<usize>,
    profiles: &'a Vec<[Option<usize>; SLOTS]>,
) -> WorkingSet<'a> {
    WorkingSet { base: &fixture.base, choices: &fixture.choices, allowed, writers, profiles, resources_capacity: 65 }
}

#[test]
fn family_admission_projection_reuses_only_complete_native_success_with_all_120_labels() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let allowed = std::array::from_fn(|_| std::iter::once(None).chain((0..12).map(Some)).collect());
    let writers = Vec::new();
    let profiles = vec![[None; SLOTS]];
    let mut previous =
        PreviousProjection::new(&context, working(&fixture, &allowed, &writers, &profiles), 1 << 20).unwrap();
    let mut checked = 0;
    for owner in 0..SLOTS {
        for choice in &fixture.choices[owner] {
            let original =
                std::array::from_fn(|slot| if slot == owner { &choice.performer } else { &fixture.base[slot] });
            let deck = original.map(Clone::clone);
            let model = LiveModel::new_gekisou(
                &fixture.master,
                &deck,
                context.notes,
                context.events,
                context.params,
                context.setup,
            )
            .unwrap();
            context.admit_pair(&model).unwrap();
            let projected = deck.map(|performer| projected(&fixture.master, &performer));
            let prepared = native(&context, &projected);
            assert!(prepared.life.is_none() && prepared.life_deck.is_none());
            previous.validate(original).unwrap();
            checked += 1;
        }
    }
    assert_eq!((checked, previous.compilations, previous.hits), (65, 1, 64));
    // Original full models and each complete native recording remain independent of the optional memo.
    // A completed profile retains every original label even when all 65 physical-pair projections coincide.
    VALIDATIONS.with(|counts| counts.set((0, 0)));
    let mut domain =
        context.admit_profile_domain(&fixture.choices, LuckFamilyLimits::default(), || false).unwrap().unwrap();
    assert_eq!(
        VALIDATIONS.with(std::cell::Cell::get),
        (65, 1),
        "the real admission entry still audits every original pair"
    );
    assert_eq!(domain.profile_count(), 1);
    let profile = context.prepare_budgeted_profile(&mut domain, 0, None, || false).unwrap().unwrap();
    assert_eq!(profile.orders().len(), ORDERS);
    let mut labels = BTreeSet::new();
    let mut order = [0, 1, 2, 3, 4];
    let mut baseline = None;
    for ordinal in 0..ORDERS {
        assert!(labels.insert(order));
        let deck = order.map(|slot| projected(&fixture.master, &fixture.base[slot]));
        let prepared = native(&context, &deck);
        let plan = RecordingCache::key(&prepared);
        let tape = record_prepared(prepared, context.notes, context.play, context.deltas, &mut || false)
            .unwrap()
            .unwrap()
            .key()
            .unwrap();
        let observed = (plan, tape);
        if let Some(expected) = &baseline {
            assert_eq!(&observed, expected);
        } else {
            baseline = Some(observed);
        }
        let mut positions = [0; SLOTS];
        for (position, &slot) in order.iter().enumerate() {
            positions[slot] = position;
        }
        assert_eq!(profile.orders()[ordinal].positions, positions);
        assert_eq!(next_order(&mut order), ordinal + 1 < ORDERS);
    }
    assert_eq!(labels.len(), ORDERS);
}

#[test]
fn family_admission_projection_compares_full_fields_order_and_never_retains_a_failure() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let allowed = std::array::from_fn(|_| vec![None]);
    let writers = Vec::new();
    let profiles = vec![[None; SLOTS]];
    let mut changed = fixture.base.clone();
    changed[2].band_id = 9; // Unread here; complete equality must nevertheless refuse this hit.
    let mut forward = fixture.base.clone();
    // Both native range-start actions use their required Complete release; their
    // distinct Plan action order makes reversal an observable recording change.
    forward[0].gekisou_support_skills = vec![(3, 1), (4, 1)];
    let mut reverse = forward.clone();
    reverse[0].gekisou_support_skills.reverse();
    let mut life = fixture.base.clone();
    life[0].gekisou_support_skills = vec![(5, 1)];
    let mut missing = fixture.base.clone();
    missing[0].gekisou_skill = Some((99999, 1));
    let mut previous =
        PreviousProjection::new(&context, working(&fixture, &allowed, &writers, &profiles), 1 << 20).unwrap();
    for deck in [&fixture.base, &changed, &forward, &reverse] {
        previous.validate(deck.each_ref()).unwrap();
    }
    assert_eq!((previous.compilations, previous.hits), (4, 0));
    assert_ne!(RecordingCache::key(&native(&context, &forward)), RecordingCache::key(&native(&context, &reverse)));
    for _ in 0..2 {
        assert_eq!(previous.validate(life.each_ref()).unwrap_err().reason, LuckFamilyDecline::LifeFeedback);
        assert!(previous.validate(missing.each_ref()).is_err());
    }
    assert_eq!((previous.compilations, previous.hits), (8, 0));
    previous.validate(reverse.each_ref()).unwrap();
    assert_eq!((previous.compilations, previous.hits), (8, 1), "failures preserve only the preceding success");
    // A new family/context never inherits the old success.
    let other_context = fixture.context();
    let mut fresh =
        PreviousProjection::new(&other_context, working(&fixture, &allowed, &writers, &profiles), 1 << 20).unwrap();
    fresh.validate(reverse.each_ref()).unwrap();
    assert_eq!((fresh.compilations, fresh.hits), (1, 0));
}

#[test]
fn family_admission_projection_keeps_original_pair_rejections_cancellation_and_work_guards() {
    let mut fixture = Fixture::new();
    fixture.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":20,"_supportSkillID":20,"_level":1,"_skillEffectType":2000,
            "_skillTriggerType":1,"_activationTimeSecond":-0.5,"_effectValue":100
        }))
        .unwrap(),
    );
    fixture.master.reindex().unwrap();
    let context = fixture.context();
    let mut bad = fixture.choices.clone();
    for choices in &mut bad {
        choices.last_mut().unwrap().performer.support_skills.push((20, 1));
    }
    assert_eq!(
        projected(&fixture.master, &bad[0].last().unwrap().performer),
        projected(&fixture.master, &fixture.base[0])
    );
    assert_eq!(
        context.admit_profile_domain(&bad, LuckFamilyLimits::default(), || false).unwrap_err().reason,
        LuckFamilyDecline::RecorderAdmission
    );
    let mut polls = 0;
    context
        .admit_profile_domain(&fixture.choices, LuckFamilyLimits::default(), || {
            polls += 1;
            false
        })
        .unwrap()
        .unwrap();
    for stop in [1, 10, polls - 1, polls] {
        let mut calls = 0;
        assert!(
            context
                .admit_profile_domain(&fixture.choices, LuckFamilyLimits::default(), || {
                    calls += 1;
                    calls >= stop
                })
                .unwrap()
                .is_none(),
            "poll {stop}"
        );
    }
    for case in 0..3 {
        let mut limits = LuckFamilyLimits::default();
        match case {
            0 => limits.max_pair_models = 64,
            1 => limits.max_order_evaluations = 119,
            _ => limits.max_frame_work = context.mapping.frame_work * 121 - 1,
        }
        assert_eq!(
            context.admit_profile_domain(&fixture.choices, limits, || false).unwrap_err().reason,
            LuckFamilyDecline::Budget
        );
    }
}

#[test]
fn family_admission_projection_capacity_counts_spare_slots_and_falls_back_without_refusal() {
    let fixture = Fixture::new();
    let context = fixture.context();
    let mut allowed = std::array::from_fn(|_| vec![None]);
    let mut writers = Vec::new();
    let mut profiles = vec![[None; SLOTS]];
    let before = working(&fixture, &allowed, &writers, &profiles).allocated_bytes().unwrap();
    allowed[0].reserve_exact(16);
    writers.reserve_exact(16);
    profiles.reserve_exact(16);
    let required = working(&fixture, &allowed, &writers, &profiles).allocated_bytes().unwrap()
        + size_of::<PreviousProjection<'_, '_>>();
    assert!(required > before + size_of::<PreviousProjection<'_, '_>>());
    assert!(
        PreviousProjection::new(&context, working(&fixture, &allowed, &writers, &profiles), required - 1).is_none()
    );
    let mut previous =
        PreviousProjection::new(&context, working(&fixture, &allowed, &writers, &profiles), required).unwrap();
    previous.validate(fixture.base.each_ref()).unwrap();
    assert_eq!(previous.compilations, 1);
    assert!(PreviousProjection::new(&context, working(&fixture, &allowed, &writers, &profiles), 0).is_none());
    // This is the caller's unchanged fallback when the optional memo has no allowance.
    context.admit_projection(&fixture.base.clone().map(|performer| projected(&fixture.master, &performer))).unwrap();
}

#[test]
fn family_admission_bounded_index_and_fallback_keep_all_pairs_errors_and_cancel_polls() {
    let mut fixture = Fixture::new();
    // Unselected rows make the full immutable index larger than the small allowance. They are deliberately
    // invalid as native effects: building an index must not validate anything the original deck never reads.
    let template = fixture.master.gekisou_support_skill_effects[0].clone();
    for id in 100_000..104_096 {
        let mut row = template.clone();
        row.id = id;
        row.skill_id = id;
        row.skill_effect_type = 999_999;
        fixture.master.gekisou_support_skill_effects.push(row);
    }
    fixture.master.support_skill_effects.push(
        serde_json::from_value(json!({
            "_id":20,"_supportSkillID":20,"_level":1,"_skillEffectType":2000,
            "_skillTriggerType":1,"_activationTimeSecond":-0.5,"_effectValue":100
        }))
        .unwrap(),
    );
    fixture.master.reindex().unwrap();
    let context = fixture.context();
    let indexed_limits = LuckFamilyLimits::default();
    let fallback_limits = LuckFamilyLimits { max_retained_bytes: 64 * 1024, ..indexed_limits };
    assert!(BuildContext::try_bounded(&fixture.master, fallback_limits.max_retained_bytes,).is_none());
    assert!(BuildContext::try_bounded(&fixture.master, indexed_limits.max_retained_bytes / 2,).is_some());
    let run = |choices: &[Vec<LuckFamilyChoice>; SLOTS], limits, stop: Option<usize>| {
        VALIDATIONS.with(|counts| counts.set((0, 0)));
        let mut polls = 0;
        let result = context.admit_profile_domain(choices, limits, || {
            polls += 1;
            stop.is_some_and(|stop| polls >= stop)
        });
        (result, polls, VALIDATIONS.with(std::cell::Cell::get))
    };
    let (indexed, polls, counts) = run(&fixture.choices, indexed_limits, None);
    let (fallback, fallback_polls, fallback_counts) = run(&fixture.choices, fallback_limits, None);
    let indexed = indexed.unwrap().unwrap();
    let fallback = fallback.unwrap().unwrap();
    assert_eq!((counts, fallback_counts), ((65, 1), (65, 1)));
    assert_eq!(polls, fallback_polls);
    assert_eq!(indexed.note_times(), fallback.note_times());
    assert_eq!(indexed.work(), fallback.work());
    let a = indexed.bindings().unwrap();
    let b = fallback.bindings().unwrap();
    assert_eq!((&a.allowed, &a.writers, &a.profiles), (&b.allowed, &b.writers, &b.profiles));
    assert_eq!(a.profiles.len(), 1);

    for stop in [1, 10, polls - 1, polls] {
        let (a, a_polls, a_counts) = run(&fixture.choices, indexed_limits, Some(stop));
        let (b, b_polls, b_counts) = run(&fixture.choices, fallback_limits, Some(stop));
        assert!(a.unwrap().is_none() && b.unwrap().is_none());
        assert_eq!((a_polls, a_counts), (b_polls, b_counts), "cancel poll {stop}");
    }
    let mut bad = fixture.choices.clone();
    for choices in &mut bad {
        choices.last_mut().unwrap().performer.support_skills.push((20, 1));
    }
    let (a, a_polls, a_counts) = run(&bad, indexed_limits, None);
    let (b, b_polls, b_counts) = run(&bad, fallback_limits, None);
    let error = a.unwrap_err();
    assert_eq!(error.reason, LuckFamilyDecline::RecorderAdmission);
    assert_eq!(error, b.unwrap_err());
    assert_eq!((a_polls, a_counts), (b_polls, b_counts));
    assert!(a_counts.0 > 1 && a_counts.0 < 65, "the late bad physical pair is still audited");
}
