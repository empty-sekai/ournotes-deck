use super::*;

#[derive(Clone)]
struct RecordingInput {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    deltas: Vec<f32>,
    deck: Vec<Performer>,
    events: Vec<(i32, i32)>,
    probes: Option<Vec<Option<usize>>>,
    ranking: Option<Vec<crate::replay::RankConfirmation>>,
}

impl RecordingInput {
    fn new() -> Self {
        let (master, notes, params, setup, play, mut deltas) = random_fixture();
        deltas[0] = 0.0;
        Self {
            master,
            notes,
            params,
            setup,
            play,
            deltas,
            deck: vec![Performer {
                gekisou_skill: Some((2, 1)),
                gekisou_support_skills: vec![(31, 1)],
                ..Default::default()
            }],
            events: Vec::new(),
            probes: None,
            ranking: None,
        }
    }

    fn prepared(&self) -> PreparedRecording<ProbabilityMass> {
        let skills = luck_skills(&self.master).unwrap();
        prepare_recording(
            &self.master,
            &skills,
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.deltas,
            &self.deck,
            self.probes.as_deref(),
            self.ranking.as_deref(),
        )
        .unwrap()
    }

    fn raw_key(&self) -> Vec<u8> {
        RecordingCache::key(&self.prepared())
    }

    fn transcript(&self) -> Vec<u64> {
        record_prepared(self.prepared(), &self.notes, &self.play, &self.deltas, &mut || false)
            .unwrap()
            .unwrap()
            .key()
            .expect("complete transcript")
    }

    fn independent(&self) -> LuckDpCertifiedResult {
        let skills = luck_skills(&self.master).unwrap();
        luck_rush_dp_certified_with_ranking(
            &self.master,
            &skills,
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.deltas,
            &self.deck,
            self.probes.as_deref(),
            self.ranking.as_deref(),
        )
        .unwrap()
    }

    fn cached(
        &self,
        curves: &mut LuckDpCache,
        recordings: Option<&mut RecordingCache>,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        let skills = luck_skills(&self.master)?;
        curves.certified_cancellable(
            &self.master,
            &skills,
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.deltas,
            &self.deck,
            self.probes.as_deref(),
            self.ranking.as_deref(),
            recordings,
            cancelled,
        )
    }

    fn fresh_session(&self, curves: &mut LuckDpCache) -> std::sync::Arc<LuckDpCertifiedResult> {
        self.cached(curves, Some(&mut RecordingCache::default()), &mut || false).unwrap().unwrap()
    }

    fn add_ranges(&mut self, distance: i32) {
        let original = self.notes.clone();
        for offset in [distance, distance * 2] {
            self.setup.fevers.push((100 + offset, 500 + offset));
            for note in &original {
                self.notes.push(LiveNote { note_id: self.notes.len() as i32, time_ms: note.time_ms + offset, ..*note });
            }
        }
        self.params.music_length_ms = distance * 2 + 2000;
        self.params.converted_note_count = self.notes.len() as i32;
        self.play.frames = (0..=self.params.music_length_ms / 100)
            .map(|frame| PlayFrame { time_ms: frame * 100, judged: Vec::new() })
            .collect();
        for note in &self.notes {
            self.play.frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap().judged.push(JudgedNote {
                note_id: note.note_id,
                judgement: 5,
                judgement_time_ms: note.time_ms,
            });
        }
        self.deltas = vec![0.1; self.play.frames.len()];
        self.deltas[0] = 0.0;
    }
}

fn permutations() -> Vec<[usize; 5]> {
    fn extend(prefix: &mut Vec<usize>, used: u8, out: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            out.push(prefix.as_slice().try_into().unwrap());
            return;
        }
        for slot in 0..5 {
            if used & (1 << slot) == 0 {
                prefix.push(slot);
                extend(prefix, used | (1 << slot), out);
                prefix.pop();
            }
        }
    }
    let mut out = Vec::new();
    extend(&mut Vec::new(), 0, &mut out);
    out
}

#[test]
fn shared_recordings_reuse_every_native_order_after_the_score_session_ends() {
    let mut input = RecordingInput::new();
    let template = input.master.gekisou_skill_effects.iter().find(|row| row.id == 1).unwrap().clone();
    let performers: Vec<_> = (0..5i64)
        .map(|member| {
            let id = 501 + member;
            input
                .master
                .gekisou_skills
                .push(serde_json::from_value(json!({"_id":id,"_gekisouMissionType":2})).unwrap());
            for variant in 0..3i64 {
                let mut row = template.clone();
                row.id = 5_010 + member * 10 + variant;
                row.skill_id = id;
                row.effect_value = 333 + member * 111 + variant * 7;
                input.master.gekisou_skill_effects.push(row);
            }
            Performer {
                character_id: member + 1,
                gekisou_skill: Some((id, 1)),
                gekisou_support_skills: if member == 0 { vec![(31, 1), (66, 1)] } else { Vec::new() },
                ..Default::default()
            }
        })
        .collect();
    input.master.reindex().unwrap();
    let orders = permutations();
    let mut curves = LuckDpCache::new(1 << 20);
    let mut keys = crate::num::FxHashSet::default();
    let mut expected = Vec::new();
    {
        let mut first_session = RecordingCache::default();
        for order in &orders {
            input.deck = order.iter().map(|&member| performers[member].clone()).collect();
            assert!(input.prepared().life.is_none());
            assert!(keys.insert(input.raw_key()), "every physical order has a distinct original recording key");
            let direct = curve_words(&input.independent());
            let value = input.cached(&mut curves, Some(&mut first_session), &mut || false).unwrap().unwrap();
            assert_eq!(curve_words(&value), direct);
            expected.push(direct);
        }
    }
    assert_eq!(keys.len(), 120);
    assert_eq!(curves.shared_recordings.retained().0, 120);
    assert!(curves.shared_recordings.retained().1 <= 1 << 20);
    let before = curves.stats();
    assert_eq!(before.shared_recording_scope_builds, 1, "one immutable session serializes its scope once");
    input.params.total_power = 16_777_217;
    for (order, expected) in orders.iter().zip(expected) {
        input.deck = order.iter().map(|&member| performers[member].clone()).collect();
        assert!(keys.contains(&input.raw_key()));
        let value = input.fresh_session(&mut curves);
        assert_eq!(curve_words(&value), expected);
        assert_eq!(curve_words(&input.independent()), expected);
    }
    let after = curves.stats();
    assert_eq!(after.shared_recording_hits - before.shared_recording_hits, 120);
    assert_eq!(after.recording_hits, before.recording_hits, "none of the old session-local entries survives");
    assert_eq!(after.propagated_curves, before.propagated_curves);
    assert_eq!(after.transitions, before.transitions);
    assert_eq!(after.shared_recording_peak_entries, 120);
    assert!(after.shared_recording_peak_bytes <= 1 << 20);
}

#[test]
fn shared_recording_power_projection_preserves_complete_short_and_long_transcripts() {
    for distance in [2400, 20_000] {
        let mut input = RecordingInput::new();
        input.deck[0].gekisou_support_skills.extend([(66, 1), (96, 1)]);
        input.add_ranges(distance);
        let original_key = input.raw_key();
        let original_transcript = input.transcript();
        let expected = curve_words(&input.independent());
        let mut curves = LuckDpCache::new(1 << 20);
        for power in [1000, 0, 1, -1, i32::MIN, i32::MAX] {
            input.params.total_power = power;
            assert_eq!(input.raw_key(), original_key);
            assert_eq!(input.transcript(), original_transcript, "distance={distance}, power={power}");
            assert_eq!(curve_words(&input.fresh_session(&mut curves)), expected);
        }
        assert_eq!(curves.stats().shared_recording_hits, 5);
        assert_eq!(curves.stats().propagated_curves, 1);
        assert!(curves.stats().shared_recording_peak_bytes <= 1 << 20);
    }
}

#[test]
fn shared_recording_scope_keeps_inputs_omitted_by_the_original_session_key() {
    type Change = (&'static str, fn(&mut RecordingInput));
    let changes: [Change; 11] = [
        ("play seed", |input| input.play.base_seed = 19),
        ("delta bits", |input| input.deltas[0] = -0.0),
        ("skill events", |input| input.events.push((0, 100))),
        ("music length", |input| input.params.music_length_ms += 100),
        ("score frame length", |input| input.params.score_music_length_ms = Some(3000)),
        ("assist bits", |input| input.params.assist_factor = f32::from_bits(1.0f32.to_bits() + 1)),
        ("music level", |input| input.params.music_level += 1),
        ("converted count", |input| input.params.converted_note_count += 1),
        ("initial life", |input| {
            input.master.live_settings.iter_mut().find(|row| row.key == "life_base").unwrap().value = "1001".into();
        }),
        ("damage table", |input| input.master.judgement_parameters[0].damage = 1),
        ("score lookup table", |input| input.master.note_parameters[0].score_percent = 101),
    ];
    for (name, change) in changes {
        let initial = RecordingInput::new();
        let mut changed = initial.clone();
        change(&mut changed);
        changed.master.reindex().unwrap();
        assert_eq!(initial.raw_key(), changed.raw_key(), "scope-only change: {name}");
        let mut curves = LuckDpCache::new(1 << 20);
        initial.fresh_session(&mut curves);
        let before = curves.stats();
        let expected = curve_words(&changed.independent());
        // The public direct call has no session memo and must compare owned scope bytes.
        let value = changed.cached(&mut curves, None, &mut || false).unwrap().unwrap();
        assert_eq!(curve_words(&value), expected, "{name}");
        assert_eq!(curves.stats().shared_recording_hits, before.shared_recording_hits, "{name}");
        assert!(curves.stats().shared_recording_lookups > before.shared_recording_lookups, "{name}");
        drop(initial);
        let separately_owned = changed.clone();
        drop(changed);
        assert_eq!(curve_words(&separately_owned.fresh_session(&mut curves)), expected);
        assert_eq!(curves.stats().shared_recording_hits, before.shared_recording_hits + 1, "{name}");
    }
}

#[test]
fn shared_recording_scope_does_not_replace_the_original_rows_owners_or_probe_key() {
    type Change = (&'static str, fn(&mut RecordingInput));
    let changes: [Change; 4] = [
        ("start chance", |input| {
            input.master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![75];
        }),
        ("gauge row", |input| {
            input.master.gekisou_skill_effects.iter_mut().find(|row| row.id == 2).unwrap().effect_value = 7500;
        }),
        ("owner position", |input| input.deck.insert(0, Performer::default())),
        ("probe flags", |input| input.probes = Some(vec![None; luck_skills(&input.master).unwrap().shapes.len()])),
    ];
    for (name, change) in changes {
        let initial = RecordingInput::new();
        let mut changed = initial.clone();
        change(&mut changed);
        changed.master.reindex().unwrap();
        assert_ne!(initial.raw_key(), changed.raw_key(), "{name}");
        let mut curves = LuckDpCache::new(1 << 20);
        initial.fresh_session(&mut curves);
        let before = curves.stats();
        let expected = curve_words(&changed.independent());
        assert_eq!(curve_words(&changed.fresh_session(&mut curves)), expected, "{name}");
        assert_eq!(curves.stats().shared_recording_hits, before.shared_recording_hits, "{name}");
        assert_eq!(curve_words(&changed.fresh_session(&mut curves)), expected, "{name}");
        assert_eq!(curves.stats().shared_recording_hits, before.shared_recording_hits + 1, "{name}");
    }
}

#[test]
fn shared_recording_scope_preserves_chart_frame_and_rank_arrival_fields() {
    type Change = (&'static str, fn(&mut RecordingInput));
    let changes: [Change; 5] = [
        ("note input order", |input| input.notes.reverse()),
        ("note judgement type", |input| input.notes[0].judgement_type = 2),
        ("quiet frame time", |input| input.play.frames[0].time_ms = 50),
        ("tied note times", |input| {
            let id = input.notes[2].note_id;
            let time = input.notes[1].time_ms;
            input.notes[2].time_ms = time;
            input
                .play
                .frames
                .iter_mut()
                .flat_map(|frame| &mut frame.judged)
                .find(|note| note.note_id == id)
                .unwrap()
                .judgement_time_ms = time;
        }),
        ("range end", |input| input.setup.fevers[0].1 += 10),
    ];
    for (name, change) in changes {
        let initial = RecordingInput::new();
        let mut changed = initial.clone();
        change(&mut changed);
        let mut curves = LuckDpCache::new(1 << 20);
        initial.fresh_session(&mut curves);
        let hits = curves.stats().shared_recording_hits;
        let expected = curve_words(&changed.independent());
        assert_eq!(curve_words(&changed.fresh_session(&mut curves)), expected, "{name}");
        assert_eq!(curves.stats().shared_recording_hits, hits, "{name}");
    }
    let mut initial = RecordingInput::new();
    initial.ranking = Some(Vec::new());
    let mut changed = initial.clone();
    changed.ranking = Some(vec![crate::replay::RankConfirmation { frame: 1, range: 0, rank: 2, percent: 175 }]);
    assert_eq!(initial.raw_key(), changed.raw_key(), "arrivals live outside the original key's Gekisou state");
    let mut curves = LuckDpCache::new(1 << 20);
    initial.fresh_session(&mut curves);
    let hits = curves.stats().shared_recording_hits;
    let expected = curve_words(&changed.independent());
    assert_eq!(curve_words(&changed.fresh_session(&mut curves)), expected);
    assert_eq!(curves.stats().shared_recording_hits, hits);
    assert_eq!(curve_words(&changed.fresh_session(&mut curves)), expected);
    assert_eq!(curves.stats().shared_recording_hits, hits + 1);
}

#[test]
fn shared_recording_reuse_does_not_admit_the_optional_life_interpreter() {
    let initial = RecordingInput::new();
    let mut changed = initial.clone();
    let condition = changed.master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap();
    condition.condition_type = 2001;
    condition.condition_values = vec![700];
    changed.master.judgement_parameters[0].damage = 600;
    changed.master.reindex().unwrap();
    let prepared = changed.prepared();
    assert!(prepared.life.is_some() && prepared.life_deck.is_some());
    let mut curves = LuckDpCache::new(1 << 20);
    let original = curve_words(&initial.fresh_session(&mut curves));
    let before = curves.stats();
    for power in [1000, 2000] {
        changed.params.total_power = power;
        let expected = curve_words(&changed.independent());
        assert_eq!(curve_words(&changed.fresh_session(&mut curves)), expected);
        assert_eq!(curves.stats().shared_recording_lookups, before.shared_recording_lookups);
        assert_eq!(curves.stats().shared_recording_scope_builds, before.shared_recording_scope_builds);
        assert_eq!(curves.stats().shared_recording_hits, before.shared_recording_hits);
    }
    assert_eq!(curve_words(&initial.fresh_session(&mut curves)), original);
    assert_eq!(curves.stats().shared_recording_hits, before.shared_recording_hits + 1);

    let mut converted = initial.clone();
    converted.master.live_skills.push(serde_json::from_value(json!({"_id":710})).unwrap());
    converted.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
        id: 710,
        live_skill_id: 710,
        level: 1,
        skill_effect_type: 12006,
        effect_value: 6,
        effect_limit_count: 1,
        skill_target_ids: vec![710],
        activation_time_second: 0.5,
        ..Default::default()
    });
    converted.master.skill_targets.push(crate::master::SkillTargetRow {
        id: 710,
        skill_target_type: 4,
        judgement: 5,
        ..Default::default()
    });
    converted.master.live_judgement_timings.push(crate::master::LiveJudgementTimingRow {
        id: 6,
        assist_level: 0,
        judgement_priority: 0,
        note_judgement_type: 1,
        note_simulate_judgement: 6,
        before_ms: 0,
        after_ms: 0,
    });
    converted.master.judgement_parameters.push(crate::master::JudgementParameterRow {
        id: 6,
        note_simulate_judgement: 6,
        score_percent: 100,
        damage: 0,
    });
    converted.master.reindex().unwrap();
    converted.deck[0].live_skill = Some((710, 1));
    converted.events.push((0, 0));
    let prepared = converted.prepared();
    assert!(prepared.life_deck.is_some());
    let mut native_life = prepared.life.unwrap();
    native_life.run_timed(&converted.play, &converted.deltas).unwrap();
    assert_eq!(native_life.conversion.converted, 1, "the optional interpreter must observe an actual conversion");
    let before = curves.stats();
    assert_eq!(curve_words(&converted.fresh_session(&mut curves)), curve_words(&converted.independent()));
    assert_eq!(curves.stats().shared_recording_lookups, before.shared_recording_lookups);
    assert_eq!(curves.stats().shared_recording_scope_builds, before.shared_recording_scope_builds);
}

#[test]
fn shared_recording_stops_and_refusals_never_publish_a_complete_identity() {
    let input = RecordingInput::new();
    let expected = curve_words(&input.independent());
    for warm in [false, true] {
        let mut measured = LuckDpCache::new(1 << 20);
        if warm {
            input.fresh_session(&mut measured);
        }
        let mut complete_checks = 0;
        input
            .cached(&mut measured, Some(&mut RecordingCache::default()), &mut || {
                complete_checks += 1;
                false
            })
            .unwrap()
            .unwrap();
        assert!(complete_checks >= 3);
        for stop_at in 1..=complete_checks {
            let mut curves = LuckDpCache::new(1 << 20);
            if warm {
                input.fresh_session(&mut curves);
            }
            let retained = curves.shared_recordings.retained();
            let hits = curves.stats().shared_recording_hits;
            let mut checks = 0;
            let stopped = input
                .cached(&mut curves, Some(&mut RecordingCache::default()), &mut || {
                    checks += 1;
                    checks == stop_at
                })
                .unwrap();
            assert!(stopped.is_none(), "warm={warm}, stop={stop_at}/{complete_checks}");
            assert_eq!(curves.shared_recordings.retained(), retained);
            assert_eq!(curves.stats().shared_recording_hits, hits);
            assert_eq!(curve_words(&input.fresh_session(&mut curves)), expected);
        }
    }
    let mut curves = LuckDpCache::new(1 << 20);
    input.fresh_session(&mut curves);
    let retained = curves.shared_recordings.retained();
    let mut invalid = input.clone();
    invalid.play.frames.iter_mut().find(|frame| !frame.judged.is_empty()).unwrap().judged[0].judgement_time_ms += 1;
    assert!(invalid.cached(&mut curves, Some(&mut RecordingCache::default()), &mut || false).is_err());
    assert_eq!(curves.shared_recordings.retained(), retained);
    let hits = curves.stats().shared_recording_hits;
    assert_eq!(curve_words(&input.fresh_session(&mut curves)), expected);
    assert_eq!(curves.stats().shared_recording_hits, hits + 1);

    for capacity in [0, 8] {
        let mut curves = LuckDpCache::new(capacity);
        for _ in 0..2 {
            assert_eq!(curve_words(&input.fresh_session(&mut curves)), expected);
        }
        assert_eq!(curves.shared_recordings.retained(), (0, 0));
        assert_eq!(curves.stats().shared_recording_hits, 0);
        assert_eq!(curves.stats().shared_recording_lookups, 0);
        if capacity == 0 {
            assert_eq!(curves.stats().shared_recording_scope_builds, 0);
            assert_eq!(curves.stats().shared_recording_scope_declines, 0);
        } else {
            assert!(curves.stats().shared_recording_key_declines > 0);
        }
    }

    let mut opaque = input.clone();
    opaque.params.assist_factor = f32::from_bits(0x7fc0_1234);
    let before = curves.stats();
    assert_eq!(curve_words(&opaque.fresh_session(&mut curves)), expected);
    assert_eq!(curves.stats().shared_recording_lookups, before.shared_recording_lookups);
    assert!(curves.stats().shared_recording_scope_declines > before.shared_recording_scope_declines);
    assert_eq!(curves.shared_recordings.retained(), retained);
}
