// Differential tests against the original, independently stepped reduced native recorder.
use super::*;
use crate::live::full::luck_score_bounds::{
    LuckRushDecline, LuckRushPreparation, LuckTerminalRush, ProbeRow, check_recorder,
};

#[derive(Clone)]
struct Input {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    delta: Vec<f32>,
    events: Vec<(i32, i32)>,
    deck: Vec<Performer>,
}

impl Input {
    fn new() -> Self {
        let (mut master, notes, params, setup, play, delta) = random_fixture();
        let speed = master.gekisou_skill_effects.iter().find(|effect| effect.id == 1).unwrap().clone();
        let mut deck = Vec::new();
        for member in 0..5i64 {
            let skill = 9500 + member;
            master.gekisou_skills.push(crate::master::SkillRow {
                id: skill,
                gekisou_mission_type: M_LUCK,
                ..Default::default()
            });
            // Multiple same-owner additions and removals exercise native binary32 source order. None of
            // these decimal percentages is dyadic, and each physical owner carries a different pair.
            for (offset, value) in [(0, 997 + member * 17), (1, 331 + member * 13)] {
                let mut effect = speed.clone();
                effect.id = 95_000 + member * 10 + offset;
                effect.skill_id = skill;
                effect.effect_value = value;
                master.gekisou_skill_effects.push(effect);
            }
            deck.push(Performer { character_id: member + 1, gekisou_skill: Some((skill, 1)), ..Default::default() });
        }
        // Range-start actions, previous-result Miss recovery and direct probes remain on one owner while
        // its position moves through all 120 original labels. The chance is the fixture's exact native 25%.
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id: 9509,
            gekisou_mission_type: M_LUCK,
            ..Default::default()
        });
        let mut gauge = master.gekisou_skill_effects.iter().find(|effect| effect.id == 2).unwrap().clone();
        gauge.id = 95_090;
        gauge.skill_id = 9509;
        master.gekisou_support_skill_effects.push(gauge);
        deck[0].gekisou_support_skills = vec![(9509, 1), (31, 1), (96, 1)];
        master.skill_effect_settings.extend([
            crate::master::SkillEffectSettingRow { id: 9501, skill_effect_type: 11001, phase: 2 },
            crate::master::SkillEffectSettingRow { id: 9502, skill_effect_type: 11003, phase: 1 },
        ]);
        // Ordinary effects are deliberately present in the full recording only. Healing changes actual
        // LIFE but cannot affect a controller action in this admitted fixture.
        master.live_skills.push(crate::master::SkillRow { id: 9510, ..Default::default() });
        for (id, kind, value) in [(95_100, 2000, 3333), (95_101, 3001, 17)] {
            master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
                id,
                live_skill_id: 9510,
                level: 1,
                skill_effect_type: kind,
                effect_value: value,
                activation_time_second: 0.3,
                ..Default::default()
            });
        }
        deck[0].live_skill = Some((9510, 1));
        master.judgement_parameters[0].damage = 1;
        master.judgement_parameters.push(
            serde_json::from_value(json!({
                "_id":9506,"_noteSimulateJudgement":6,"_scorePercent":110,"_damage":0
            }))
            .unwrap(),
        );
        master.gekisou_luck_base_points.push(
            serde_json::from_value(json!({
                "_id":9506,"_noteCategory":0,"_noteSimulateJudgement":6,"_weight":1,"_basePoint":60
            }))
            .unwrap(),
        );
        master.live_judgement_timings.push(
            serde_json::from_value(json!({
                "_id":9506,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":0
            }))
            .unwrap(),
        );
        master.reindex().unwrap();
        Self { master, notes, params, setup, play, delta, events: vec![(0, 100)], deck }
    }

    fn reduced(&self) -> Transcript<ProbabilityMass> {
        record_frames(
            &self.master,
            &luck_skills(&self.master).unwrap(),
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.delta,
            &self.deck,
            None,
            None,
            false,
        )
        .unwrap()
    }

    fn start(&self, record_only: bool) -> Result<Option<(LiveModel, fused::Recorder)>, Error> {
        let skills = luck_skills(&self.master)?;
        let mut full =
            LiveModel::new_gekisou(&self.master, &self.deck, &self.notes, &self.events, self.params, &self.setup)?;
        let gate = check_recorder(&full, &skills)?;
        let probes = full
            .luck_score_rows(&skills)
            .into_iter()
            .filter(|row| row.may_hold)
            .map(|row| ProbeRow { owner: row.owner, value: row.value })
            .collect();
        full.set_luck_weights(&skills, Vec::new())?;
        full.score.begin_bounds(probes, true);
        full.score.certify_bounds_filings(gate);
        let admitted = record_only && full.try_enable_bounds_record_only();
        let recording = fused::Recorder::prepare(
            &self.master,
            &skills,
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.delta,
            &self.deck,
            &full,
            admitted,
        )?;
        full.set_seed(self.play.base_seed);
        Ok(recording.map(|recording| (full, recording)))
    }

    fn frames(&self, full: &mut LiveModel, recording: &mut fused::Recorder, count: usize) -> Result<(), Error> {
        for (frame, &delta) in self.play.frames.iter().zip(&self.delta).take(count) {
            full.frame_timed_observed(frame.time_ms, &frame.judged, delta, &mut |model, _, results| {
                recording.observe(model, frame, delta, results)
            })?;
        }
        Ok(())
    }

    fn complete(&self) -> (LiveModel, fused::Recorder) {
        let (mut full, mut recording) = self.start(true).unwrap().expect("the fixture must use the fused route");
        self.frames(&mut full, &mut recording, self.play.frames.len()).unwrap();
        (full, recording)
    }

    fn add_converter(&mut self, raw_target: i64) {
        self.master.skill_targets.push(crate::master::SkillTargetRow {
            id: 9520,
            skill_target_type: 4,
            judgement: raw_target,
            ..Default::default()
        });
        self.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
            id: 95_200,
            live_skill_id: 9510,
            level: 1,
            skill_effect_type: 12006,
            skill_target_ids: vec![9520],
            effect_value: 6,
            activation_time_second: 0.5,
            ..Default::default()
        });
        self.master.reindex().unwrap();
    }

    fn preparation(
        &self,
        fused: bool,
        cache: &mut LuckDpCache,
        cancelled: impl FnMut() -> bool,
    ) -> LuckRushPreparation {
        let skills = luck_skills(&self.master).unwrap();
        let mut session = LuckScoreSession::new(
            &self.master,
            &skills,
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.delta,
            None,
        );
        if fused {
            session.rush_cap_preparation(&self.deck, Some(cache), cancelled)
        } else {
            session.test_rush_cap_preparation_unfused(&self.deck, Some(cache), cancelled)
        }
    }

    fn terminal(&self, fused: bool, cache: &mut LuckDpCache) -> LuckTerminalRush {
        match self.preparation(fused, cache, || false) {
            LuckRushPreparation::Ready(terminal) => terminal,
            other => panic!("complete fixture must retain terminal capabilities: {other:?}"),
        }
    }
}

fn certified(transcript: &Transcript<ProbabilityMass>) -> LuckDpCertifiedResult {
    let result = propagate(transcript).unwrap();
    LuckDpCertifiedResult {
        probe_transitions: result.probe_transitions,
        range_moments: result.range_moments,
        steps: result.steps,
        probes: result.probes,
        peak_states: result.peak_states,
        transitions: result.transitions,
    }
}

fn orders() -> Vec<[usize; 5]> {
    fn visit(prefix: &mut Vec<usize>, used: u8, out: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            out.push(prefix.as_slice().try_into().unwrap());
            return;
        }
        for member in 0..5 {
            if used & (1 << member) == 0 {
                prefix.push(member);
                visit(prefix, used | (1 << member), out);
                prefix.pop();
            }
        }
    }
    let mut out = Vec::new();
    visit(&mut Vec::new(), 0, &mut out);
    out
}

#[test]
fn fused_native_recording_matches_complete_reduced_transcripts_for_all_120_orders() {
    let mut input = Input::new();
    let original = input.deck.clone();
    let mut cache = LuckDpCache::new(1 << 20);
    let labels = orders();
    assert_eq!(labels.len(), 120);
    assert!(input.play.frames.iter().any(|frame| frame.judged.len() > 1));
    let mut observed_nontrivial_mass = false;
    for order in labels {
        input.deck = order.iter().map(|&member| original[member].clone()).collect();
        let reduced = input.reduced();
        assert!(reduced.failure.is_none());
        assert!(reduced.frames.iter().any(|frame| frame.finish));
        assert!(reduced.frames.iter().any(|frame| frame.repeat > 1));
        assert!(reduced.actions.iter().any(|action| matches!(action, Action::MissGauge { .. })));
        assert!(!reduced.pending.is_empty());
        let expected = certified(&reduced);
        observed_nontrivial_mass |= expected
            .steps
            .iter()
            .any(|(_, joint)| joint.iter().any(|mass| *mass != ProbabilityMass::ZERO && *mass != ProbabilityMass::ONE));
        let (full, recording) = input.complete();
        assert_eq!(recording.test_complete_key(&full).unwrap(), reduced.key().unwrap(), "order={order:?}");
        let actual = cache.certified_fused(recording, &full, &mut || false).unwrap().unwrap();
        assert_eq!(curve_words(&actual), curve_words(&expected), "order={order:?}");
    }
    assert!(observed_nontrivial_mass, "the native 25% action and 2:1 integer lottery must remain probabilistic");
}

#[test]
fn fused_native_recording_keeps_identity_conversion_and_zero_capacity_exact() {
    let mut input = Input::new();
    // All declared grades are Perfect. A Great-to-Just converter exists and registers, but its complete
    // original target relation proves that it cannot change one of those grades.
    input.add_converter(4);
    let expected = certified(&input.reduced());
    for phase in [1, 2] {
        input.master.skill_effect_settings.iter_mut().find(|row| row.skill_effect_type == 11001).unwrap().phase = phase;
        let reduced = input.reduced();
        let (full, recording) = input.complete();
        assert_eq!(recording.test_complete_key(&full).unwrap(), reduced.key().unwrap());
        let mut disabled = LuckDpCache::new(0);
        let actual = disabled.certified_fused(recording, &full, &mut || false).unwrap().unwrap();
        assert_eq!(curve_words(&actual), curve_words(&certified(&reduced)));
        assert_eq!(disabled.stats().peak_entries, 0);
        assert_eq!(disabled.stats().peak_key_bytes, 0);
        assert!(disabled.entries.is_empty());
        // Phase changes can alter the law; the original full phase-2 configuration must still reproduce
        // its exact earlier endpoints, not merely a containing interval.
        if phase == 2 {
            assert_eq!(curve_words(&actual), curve_words(&expected));
        }
    }
}

#[test]
fn fused_native_recording_declines_life_actions_changed_grades_and_non_recording_models() {
    let input = Input::new();
    assert!(input.start(false).unwrap().is_none());

    let mut life = input.clone();
    let condition = life.master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
    condition.condition_type = 2001;
    condition.condition_values = vec![990];
    assert!(life.start(true).unwrap().is_none(), "phase LIFE must remain on the original reduced+LIFE route");
    assert!(life.reduced().key().is_some(), "the narrower fusion refusal must not remove an admitted old route");

    let mut conversion = input.clone();
    conversion.add_converter(5);
    assert!(conversion.start(true).unwrap().is_none(), "the real converter can change declared Perfect grades");
    let mut native = LiveModel::new_gekisou(
        &conversion.master,
        &conversion.deck,
        &conversion.notes,
        &conversion.events,
        conversion.params,
        &conversion.setup,
    )
    .unwrap();
    let mut converted = false;
    for (frame, &delta) in conversion.play.frames.iter().zip(&conversion.delta) {
        native.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
        converted |= native.frame_judgements().iter().any(|&(_, grade, _)| grade == 6);
    }
    assert!(converted, "an independent complete native life must witness the declined grade change");

    let mut mixed = input.clone();
    mixed.setup.fevers.push((1200, 1500));
    mixed.setup.missions[1] = 1;
    assert!(mixed.start(true).unwrap().is_none(), "a second non-LUCK range is not an admitted fused controller");

    let mut unsupported = input;
    unsupported
        .master
        .gekisou_skill_effects
        .iter_mut()
        .find(|row| row.skill_id == 9500)
        .unwrap()
        .activation_time_second = 0.0;
    assert!(unsupported.start(true).is_err(), "unknown native speed lifecycles must not acquire a transcript");
}

#[test]
fn fused_native_recording_declines_wrapped_ordinary_and_retained_effect_key_aliases() {
    let mut input = Input::new();
    // Native keys are wrapping row*100 + source_kind*10 + member. The support kind3 and GK support kind5
    // differ by20, and this positive row difference contributes20 after the multiplication wraps five times.
    let collision = 95_090 + 922_337_203_685_477_581i64;
    assert_eq!(collision.wrapping_mul(100).wrapping_add(30), 95_090i64.wrapping_mul(100).wrapping_add(50));
    input
        .master
        .support_skill_effects
        .push(serde_json::from_value(row(collision, "_supportSkillID", 9530, 2000, 1, 7010, 0, 0)).unwrap());
    input.deck[0].support_skills.push((9530, 1));
    input.master.reindex().unwrap();
    let full =
        LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup)
            .unwrap();
    let effects: Vec<_> = full.cond.iter().flat_map(|skill| skill.updater.effects()).collect();
    let ids: crate::num::FxHashSet<_> = effects.iter().map(|effect| effect.effect_id).collect();
    assert!(ids.len() < effects.len(), "the actual native constructor must create the collision");
    assert!(input.start(true).unwrap().is_none());
}

#[test]
fn fused_prepass_keeps_every_terminal_endpoint_and_scope_bit_identical_to_two_pass_recording() {
    let input = Input::new();
    let times: Vec<_> = input.notes.iter().map(|note| note.time_ms).collect();
    let power = i64::from(input.params.total_power);
    for capacity in [0, 1 << 20] {
        let mut original_cache = LuckDpCache::new(capacity);
        let original = input.terminal(false, &mut original_cache);
        let mut fused_cache = LuckDpCache::new(capacity);
        let actual = input.terminal(true, &mut fused_cache);
        assert_eq!(actual.probe_gate(), original.probe_gate());
        assert_eq!(
            actual.native_note_bucket_caps(power, &times).unwrap(),
            original.native_note_bucket_caps(power, &times).unwrap(),
        );
        let bits = |fields: &[[f64; 2]]| fields.iter().map(|field| field.map(f64::to_bits)).collect::<Vec<_>>();
        assert_eq!(
            bits(actual.note_score_up_upper(&times).unwrap()),
            bits(original.note_score_up_upper(&times).unwrap()),
        );
        assert_eq!(
            actual.native_score_mean_upper(power).unwrap().to_bits(),
            original.native_score_mean_upper(power).unwrap().to_bits(),
        );
        assert_eq!(
            summary_words(&actual.terminal_summary(power).unwrap()),
            summary_words(&original.terminal_summary(power).unwrap()),
        );
        let caps: Vec<_> = times.iter().map(|&time| (time, [11, 17, 19, 23])).collect();
        assert_eq!(
            actual.weighted_note_bucket_upper(&caps).unwrap().to_bits(),
            original.weighted_note_bucket_upper(&caps).unwrap().to_bits(),
        );
        assert!(actual.native_note_bucket_caps(power + 1, &times).is_none());
        assert!(actual.native_score_mean_upper(power + 1).is_none());
        assert!(actual.terminal_summary(power + 1).is_none());
        let mut wrong_times = times.clone();
        wrong_times[0] += 1;
        assert!(actual.native_note_bucket_caps(power, &wrong_times).is_none());
        assert!(actual.note_score_up_upper(&wrong_times).is_none());
        assert!(actual.weighted_note_bucket_upper(&caps[..caps.len() - 1]).is_none());
        if capacity == 0 {
            assert_eq!(fused_cache.stats().peak_entries, 0);
            assert_eq!(fused_cache.stats().peak_key_bytes, 0);
            assert_eq!(fused_cache.stats().program_compilations, 0);
        }
    }
}

fn unavailable(result: LuckRushPreparation) -> (LuckRushDecline, Error) {
    match result {
        LuckRushPreparation::Unavailable { reason, error } => (reason, error),
        other => panic!("the invalid input must be refused: {other:?}"),
    }
}

fn ordinary_pool_exhaustion() -> Input {
    let mut input = Input::new();
    for row in &mut input.master.live_skill_effects {
        if row.live_skill_id == 9510 {
            row.activation_time_second = 10.0;
        }
    }
    input.events = (0..6).map(|frame| (0, frame * 100)).collect();
    input.master.reindex().unwrap();
    input
}

/// Observe the real failing native frame, independently of the prepass's error classification.
fn first_fused_frame_error(input: &Input) -> (Error, bool) {
    let (mut full, mut recording) = input.start(true).unwrap().expect("the test must enter fused recording");
    for (frame, &delta) in input.play.frames.iter().zip(&input.delta) {
        let mut observed = false;
        let result = full.frame_timed_observed(frame.time_ms, &frame.judged, delta, &mut |model, _, results| {
            observed = true;
            recording.observe(model, frame, delta, results)
        });
        if let Err(error) = result {
            return (error, observed);
        }
    }
    panic!("the native frame sequence unexpectedly completed");
}

#[test]
fn fused_prepass_preserves_two_pass_failure_precedence_before_and_inside_the_observer() {
    let mut cases = Vec::new();
    let mut overlap = Input::new();
    overlap.setup.fevers.push((150, 260));
    cases.push((
        "overlapping LUCK ranges",
        overlap,
        Error::Unsupported("LUCK coefficient: a luck range starts during a rush".into()),
        false,
    ));
    let mut unknown = Input::new();
    unknown.play.frames[2].judged[0].note_id = i32::MAX;
    cases.push(("unknown note", unknown.clone(), Error::Input(format!("unknown note {}", i32::MAX)), false));
    let mut negative_delta = Input::new();
    negative_delta.delta[2] = -0.1;
    cases.push((
        "negative delta",
        negative_delta,
        Error::Input("LUCK DP needs increasing frames and finite nonnegative deltas".into()),
        true,
    ));
    let mut late_time = Input::new();
    late_time.play.frames[2].judged[0].judgement_time_ms += 1;
    cases.push((
        "non-chart judgement time",
        late_time,
        Error::Unsupported("LUCK DP requires notes in their first chart-time frame, in chart-time order".into()),
        true,
    ));
    for (label, input, error, observed) in &cases {
        assert_eq!(first_fused_frame_error(input), (error.clone(), *observed), "{label}");
        for capacity in [0, 1 << 20] {
            for fused in [false, true] {
                let mut cache = LuckDpCache::new(capacity);
                assert_eq!(
                    unavailable(input.preparation(fused, &mut cache, || false)),
                    (LuckRushDecline::ProbabilityDomain, error.clone()),
                    "{label}, fused={fused}, capacity={capacity}",
                );
                assert!(cache.entries.is_empty(), "{label}: failed DP cannot publish a partial curve");
                assert_eq!(cache.stats().recording_peak_entries, 0);
                assert_eq!(cache.stats().shared_recording_peak_entries, 0);
                assert_eq!(cache.stats().program_peak_entries, 0);
            }
        }
    }

    // The full model sees the missing note first. The earlier reduced pass rejects the repeated frame
    // before looking up that note, so compatibility includes the original error payload, not just its tag.
    unknown.play.frames[2].time_ms = unknown.play.frames[1].time_ms;
    assert_eq!(first_fused_frame_error(&unknown), (Error::Input(format!("unknown note {}", i32::MAX)), false));
    for fused in [false, true] {
        let mut cache = LuckDpCache::new(1 << 20);
        assert_eq!(
            unavailable(unknown.preparation(fused, &mut cache, || false)),
            (
                LuckRushDecline::ProbabilityDomain,
                Error::Input("LUCK DP needs increasing frames and finite nonnegative deltas".into()),
            ),
        );
        assert!(cache.entries.is_empty());
    }

    // Ordinary live rows and their event-triggered pool are absent from the reduced controller. A
    // successful DP followed by the sixth overlapping ordinary activation is still RecorderAdmission.
    let ordinary = ordinary_pool_exhaustion();
    let error = Error::Game("live skill pool is empty".into());
    assert_eq!(first_fused_frame_error(&ordinary), (error.clone(), false));
    assert!(ordinary.reduced().key().is_some());
    for capacity in [0, 1 << 20] {
        for fused in [false, true] {
            let mut cache = LuckDpCache::new(capacity);
            assert_eq!(
                unavailable(ordinary.preparation(fused, &mut cache, || false)),
                (LuckRushDecline::RecorderAdmission, error.clone()),
            );
            assert!(cache.stats().transitions > 0, "the original complete DP must precede the ordinary refusal");
            assert_eq!(cache.stats().program_peak_entries, 0);
            assert_eq!(cache.stats().terminal_builds, 0);
            assert_eq!(cache.entries.is_empty(), capacity == 0);
        }
    }
}

#[test]
fn fused_prepass_cold_failure_replay_keeps_cancellation_and_complete_cache_boundaries() {
    let input = ordinary_pool_exhaustion();
    let mut baseline_cache = LuckDpCache::new(1 << 20);
    let mut checks = 0usize;
    let expected = unavailable(input.preparation(true, &mut baseline_cache, || {
        checks += 1;
        false
    }));
    assert_eq!(expected, (LuckRushDecline::RecorderAdmission, Error::Game("live skill pool is empty".into())));
    assert!(checks > 1 && baseline_cache.stats().transitions > 0 && !baseline_cache.entries.is_empty());

    // The final uncancelled poll is the reduced DP's last publication barrier. Work observed here can
    // only come from the cold replay: the fused full recording already failed before curve propagation.
    let mut cache = LuckDpCache::new(1 << 20);
    let mut seen = 0usize;
    let stopped = input.preparation(true, &mut cache, || {
        seen += 1;
        seen >= checks
    });
    assert!(matches!(stopped, LuckRushPreparation::Stopped), "{stopped:?}");
    assert_eq!(seen, checks);
    assert!(cache.stats().transitions > 0, "cancellation must occur inside the cold probability replay");
    assert!(cache.entries.is_empty());
    assert_eq!(cache.words, 0);
    assert_eq!(cache.stats().recording_peak_entries, 0);
    assert_eq!(cache.stats().shared_recording_peak_entries, 0);
    assert_eq!(cache.stats().program_peak_entries, 0);
    assert_eq!(cache.stats().terminal_builds, 0);

    assert_eq!(unavailable(input.preparation(true, &mut cache, || false)), expected);
    assert!(!cache.entries.is_empty(), "resuming may publish the complete reduced curve only");
    assert_eq!(cache.stats().program_peak_entries, 0);
    assert_eq!(cache.stats().terminal_builds, 0);
}

#[test]
fn fused_native_recording_never_publishes_partial_or_cancelled_curves() {
    let input = Input::new();
    let (mut full, mut recording) = input.start(true).unwrap().unwrap();
    input.frames(&mut full, &mut recording, input.play.frames.len() - 1).unwrap();
    assert!(recording.test_complete_key(&full).is_err());
    let mut partial = LuckDpCache::new(1 << 20);
    assert!(partial.certified_fused(recording, &full, &mut || false).is_err());
    assert!(partial.entries.is_empty());

    let (full, recording) = input.complete();
    let mut polls = 0usize;
    let mut completed = LuckDpCache::new(1 << 20);
    let expected = completed
        .certified_fused(recording, &full, &mut || {
            polls += 1;
            false
        })
        .unwrap()
        .unwrap();
    assert!(polls >= 4);
    for stop_at in [1, 2, polls / 2, polls] {
        let (full, recording) = input.complete();
        let mut cache = LuckDpCache::new(1 << 20);
        let mut seen = 0usize;
        assert!(
            cache
                .certified_fused(recording, &full, &mut || {
                    seen += 1;
                    seen == stop_at
                })
                .unwrap()
                .is_none(),
            "cancel at check {stop_at}"
        );
        assert!(cache.entries.is_empty(), "cancelled propagation must publish no cache value");
        let (full, recording) = input.complete();
        let resumed = cache.certified_fused(recording, &full, &mut || false).unwrap().unwrap();
        assert_eq!(curve_words(&resumed), curve_words(&expected));
    }
}

#[test]
fn fused_native_recording_capacity_refusal_preserves_completed_cache_and_can_resume() {
    let mut cache = LuckDpCache::new(1 << 20);
    let small = Input::new();
    let (full, recording) = small.complete();
    let retained = cache.certified_fused(recording, &full, &mut || false).unwrap().unwrap();
    let retained_words = curve_words(&retained);
    let before = (cache.entries.len(), cache.words, cache.stats().peak_entries, cache.stats().peak_key_bytes);
    assert!(before.0 > 0);

    const NOTES: i32 = 30_000;
    let mut large = Input::new();
    // All notes have unique IDs and belong to their first processing frame, inside the LUCK range.
    // Zero damage avoids unrelated quadratic LIFE-log folding of thousands of same-time damage commands.
    // Both complete native note filing and the real fused observer still consume every Perfect normally.
    large.master.judgement_parameters.iter_mut().find(|row| row.note_simulate_judgement == 5).unwrap().damage = 0;
    large.notes =
        (0..NOTES).map(|note_id| LiveNote { note_id, time_ms: 100, note_operate_type: 1, judgement_type: 1 }).collect();
    large.params.converted_note_count = NOTES;
    for frame in &mut large.play.frames {
        frame.judged.clear();
    }
    large.play.frames.iter_mut().find(|frame| frame.time_ms == 100).unwrap().judged = large
        .notes
        .iter()
        .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
        .collect();
    let (mut full, mut recording) = large.start(true).unwrap().expect("capacity fixture passes structural admission");
    let error = large.frames(&mut full, &mut recording, large.play.frames.len()).unwrap_err();
    assert_eq!(error, Error::Capacity("fused LUCK recording transcript cap".into()));
    assert_eq!(full.frame_judgements().len(), NOTES as usize, "the full native frame must reach the observer");
    assert!(recording.test_complete_key(&full).is_err());
    assert!(cache.certified_fused(recording, &full, &mut || false).is_err());
    assert_eq!(
        (cache.entries.len(), cache.words, cache.stats().peak_entries, cache.stats().peak_key_bytes),
        before,
        "an oversized transcript must neither publish a partial curve nor evict a completed one",
    );

    let resumed = Input::new();
    let (full, recording) = resumed.complete();
    let actual = cache.certified_fused(recording, &full, &mut || false).unwrap().unwrap();
    assert_eq!(curve_words(&actual), retained_words);
    assert!(std::sync::Arc::ptr_eq(&actual, &retained), "the earlier completed cache entry remains usable");
}
