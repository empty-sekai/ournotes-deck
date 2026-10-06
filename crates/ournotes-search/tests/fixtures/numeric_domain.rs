//! Synthetic numeric-domain regressions. These preserve the evaluator's wrapping
//! arithmetic; they do not claim that these extreme bonuses occur in game data.

use super::common::{extend_table, replace_table, set_column};
use super::{
    EVENT_ID, SCORE_ID, context_document, data_document, joint_request_json, roster_document, synthetic_master,
};
use ournotes_search::{
    engine, handler,
    search::{self, Completion, telemetry::Traversal},
    types::{ExitReason, Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, pool::Pool, power::BP_UNIT};
use serde_json::{Value, json};

const WRAPPED_FLAT_BONUS: i64 = 970_298_738_277_122_416;
const TEAMS: usize = 31; // No Snap, one of two Snaps in five slots, or both: 1 + 10 + 20.

fn inputs(wrapped: bool) -> (DeckData, Roster) {
    let mut synth = synthetic_master(5, 2, 5);
    if wrapped {
        set_column(&mut synth, "MasterCharacterRank", &mut |row| {
            if row["_rank"] == 10 {
                row["_bonus"] = json!(WRAPPED_FLAT_BONUS);
            }
        });
    }
    let data = DeckData::from_json(&data_document(&synth, 5, 2, 5).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 2, 5).to_string()).unwrap();
    (data, roster)
}

fn requests() -> [(&'static str, RecommendationRequest); 3] {
    let live = joint_request_json("free", false, json!({"kind":"score"}));
    let mut power = joint_request_json("free", false, json!({"kind":"power"}));
    power["execution"] = json!({"kind":"power","musicId":10,"eventParameter":false});
    power["context"] = Value::Null;
    let mut skip = joint_request_json("free", false, json!({"kind":"clientEventPoints","eventId":EVENT_ID}));
    skip["execution"] = json!({"kind":"skip","scoreId":SCORE_ID});
    skip["context"] = context_document(true, false, false);
    [("live-score", live), ("power", power), ("skip-event-points", skip)].map(|(name, mut value)| {
        value["constraints"]["leader"] = json!(1);
        value["k"] = json!(TEAMS);
        (name, serde_json::from_value(value).unwrap())
    })
}

#[test]
fn nonintegral_flat_bonus_keeps_native_wrapping_and_fractional_carry() {
    let (data, roster) = inputs(true);
    let pool = Pool::new(&data.master, &roster).unwrap();
    let deck = pool.deck([2, 3, 1, 4, 5], [None; 5], [0, 1, 2, 3, 4]).unwrap();
    let song = pool.song(10).unwrap();
    let native = pool.deck_power(&deck, Some(&song), false).unwrap();

    assert_eq!(WRAPPED_FLAT_BONUS.wrapping_mul(BP_UNIT), 9_984);
    for slot in &native.slots {
        assert_eq!(slot.character_rank.to_array(), [9_984; 3]);
        assert_eq!(slot.total.to_array().map(|stat| stat % BP_UNIT), [9_984; 3]);
    }
    // The native evaluator sums five slots per stat before taking whole points.
    // Flooring each slot's three-stat BP sum first loses two carried points.
    let prematurely_floored: i64 = native.slots.iter().map(|slot| slot.total.bp_total() / BP_UNIT).sum();
    assert_eq!(i64::from(native.power()), prematurely_floored + 2);
}

#[test]
fn nonintegral_power_falls_back_on_all_three_routes_without_dropping_teams() {
    let (data, roster) = inputs(true);
    for (name, mut request) in requests() {
        // Build separately so the guard must preserve the actual resource IDs,
        // not merely report unchanged candidate counts in the final telemetry.
        let built = handler::build_card_pool(&data, &roster, &request).unwrap();
        assert!(built.domain().is_feasible(), "{name}");
        assert_eq!(
            built.domain().members().iter().map(|&index| built.pool().members[index].id).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5],
            "{name}"
        );
        assert_eq!(
            built.domain().snaps().iter().map(|&index| built.pool().snaps[index].id).collect::<Vec<_>>(),
            [1, 2],
            "{name}"
        );
        assert_eq!(built.domain().leader().map(|index| built.pool().members[index].id), Some(1), "{name}");
        let bounded = search::recommend_built(&built).unwrap();
        assert_eq!(bounded.completion, Completion::Complete, "{name}");
        assert_eq!(bounded.optimality, Optimality::Proven, "{name}");
        assert_eq!(bounded.exit_reason, ExitReason::Exhausted, "{name}");
        assert_eq!(bounded.telemetry.environment.traversal, Traversal::Exhaustive, "{name}");
        assert!(!bounded.telemetry.environment.bounds.compiled, "{name}");
        assert!(
            bounded
                .telemetry
                .environment
                .bounds
                .fallback
                .as_deref()
                .is_some_and(|reason| reason.contains("nonintegral")),
            "{name}: {:?}",
            bounded.telemetry.environment.bounds.fallback
        );
        assert_eq!(bounded.telemetry.leaves.visited, TEAMS as u64, "{name}");
        assert_eq!(bounded.results.len(), TEAMS, "{name}");
        let domain = bounded.telemetry.environment.domain.as_ref().unwrap();
        assert_eq!((domain.members, domain.snaps, domain.required, domain.leader_fixed), (5, 2, 1, true), "{name}");

        request.strategy = Strategy::Exhaustive;
        let oracle = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(oracle.completion, Completion::Complete, "{name}");
        assert_eq!(oracle.telemetry.leaves.visited, TEAMS as u64, "{name}");
        // K spans the complete domain, including None and both unique Snaps, so
        // equality checks every identity, native value, and tie-breaking rank.
        assert_eq!(bounded.results, oracle.results, "{name}");
    }
}

#[test]
fn nonintegral_power_fallback_respects_zero_budget_on_all_three_routes() {
    let (data, roster) = inputs(true);
    for (name, mut request) in requests() {
        request.limits.time_limit_ms = Some(0);
        let stopped = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(stopped.completion, Completion::TimedOut, "{name}");
        assert_eq!(stopped.optimality, Optimality::Unproven, "{name}");
        assert_eq!(stopped.exit_reason, ExitReason::TimeLimit, "{name}");
        assert!(!stopped.telemetry.proof.complete, "{name}");
        assert!(!stopped.telemetry.environment.bounds.compiled, "{name}");
        assert!(
            stopped
                .telemetry
                .environment
                .bounds
                .fallback
                .as_deref()
                .is_some_and(|reason| reason.contains("nonintegral")),
            "{name}: {:?}",
            stopped.telemetry.environment.bounds.fallback
        );
        assert_eq!(stopped.telemetry.leaves.visited, 0, "{name}");
        assert!(stopped.results.is_empty(), "{name}");
    }
}

#[test]
fn integral_resolved_power_keeps_all_three_optimized_routes() {
    let (data, roster) = inputs(false);
    for (name, request) in requests() {
        let bounded = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(bounded.completion, Completion::Complete, "{name}");
        assert_eq!(bounded.optimality, Optimality::Proven, "{name}");
        assert!(bounded.telemetry.environment.bounds.compiled, "{name}");
        assert!(
            bounded.telemetry.environment.bounds.fallback.is_none(),
            "{name}: {:?}",
            bounded.telemetry.environment.bounds.fallback
        );
        assert_eq!(bounded.results.len(), TEAMS, "{name}");
        // Skip event points use the separate deck-payoff builder; ordinary Live
        // score and Power exercise the Joint and TeamPower builders respectively.
        assert_eq!(bounded.telemetry.environment.bounds.deck_payoff.is_some(), name == "skip-event-points", "{name}");
    }
}

fn release_support_inputs(act: f64, release: i64, conversion: bool) -> (DeckData, Roster, RecommendationRequest) {
    let mut synth = synthetic_master(5, 1, 5);
    replace_table(&mut synth, "MasterLiveSkillEffect", json!([]));
    set_column(&mut synth, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(3);
        row["_supportSkillId02"] = json!(0);
    });
    set_column(&mut synth, "MasterSupportSkillEffect", &mut |row| {
        if row["_supportSkillID"] == 3 {
            // Group 53 is the paired member's live event; group 63 is Fixed(false).
            row["_skillTriggerConditionGroup"] = json!(53);
            row["_skillConditionGroup"] = json!(0);
            row["_skillReleaseConditionGroup"] = json!(release);
            row["_activationTimeSecond"] = json!(act);
            row["_effectValue"] = json!(if conversion { 5 } else { 10_000 });
            if conversion {
                row["_skillEffectType"] = json!(12006);
                row["_skillTargetIDs"] = json!([43]); // Good -> Perfect.
            }
        }
    });
    set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(2));
    let mut data = data_document(&synth, 5, 1, 5);
    data["charts"][0]["notes"] = json!({"id":[1,2],"op":[1,1],"judgementType":[1,1],"timeMs":[400,400]});
    data["charts"][0]["skillEvents"] = json!({"timeMs":[0,0,0,0,0]});
    let frames = if act == 0.0 { vec![0, 40, 400] } else { vec![0, 400] };
    let last = frames.len() - 1;
    let judgement = if conversion { 3 } else { 5 };
    let mut request = joint_request_json("free", false, json!({"kind":"score"}));
    request["constraints"]["leader"] = json!(1);
    request["k"] = json!(6);
    request["execution"]["play"] = json!({"kind":"stream","stream":{
        "frames":frames,"judged":[[last,1,judgement,400],[last,2,judgement,400]]}});
    (
        DeckData::from_json(&data.to_string()).unwrap(),
        Roster::from_json(&roster_document(5, 1, 5).to_string()).unwrap(),
        serde_json::from_value(request).unwrap(),
    )
}

#[test]
fn release_condition_keeps_held_support_factors_in_every_exact_team() {
    for act in [0.0, 0.001] {
        let (data, roster, mut request) = release_support_inputs(act, 63, false);
        let bounded = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(bounded.completion, Completion::Complete);
        assert!(bounded.telemetry.environment.bounds.compiled);
        assert_eq!(bounded.results.len(), 6);
        request.strategy = Strategy::Exhaustive;
        let exact = engine::recommend(&data, &roster, &request).unwrap();
        assert_eq!(exact.completion, Completion::Complete);
        assert_eq!(bounded.results, exact.results, "activation {act}");

        let (ordinary, ordinary_roster, mut ordinary_request) = release_support_inputs(act, 0, false);
        ordinary_request.strategy = Strategy::Exhaustive;
        let ordinary = engine::recommend(&ordinary, &ordinary_roster, &ordinary_request).unwrap();
        for held in exact.results.iter().filter(|deck| deck.snaps.iter().any(Option::is_some)) {
            let ended =
                ordinary.results.iter().find(|deck| deck.members == held.members && deck.snaps == held.snaps).unwrap();
            assert_eq!(held.power, ended.power);
            assert_eq!(held.order_outcomes.len(), 120);
            assert_eq!(ended.order_outcomes.len(), 120);
            assert!(held.order_outcomes.iter().zip(&ended.order_outcomes).all(|(a, b)| a.0 == b.0 && a.1 > b.1));
        }
    }
}

#[cfg(feature = "search-diagnostics")]
#[test]
fn release_support_windows_bound_all_performance_orders() {
    for act in [0.0, 0.001] {
        let (data, roster, request) = release_support_inputs(act, 63, false);
        let built = handler::build_card_pool(&data, &roster, &request).unwrap();
        let result = search::recommend_built(&built).unwrap();
        assert!(result.telemetry.environment.bounds.compiled);
        for deck in result.results.iter().filter(|deck| deck.snaps.iter().any(Option::is_some)) {
            let audit = search::diagnostics::audit_order_caps(&built, deck.members, deck.snaps).unwrap();
            assert_eq!(audit["orders"], 120);
            assert_eq!(audit["violations"], 0, "activation {act}: {}", audit["first"]);
        }
    }
}

#[test]
fn release_condition_keeps_late_support_conversions_in_caps() {
    let (data, roster, mut request) = release_support_inputs(0.0, 63, true);
    let bounded = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(bounded.completion, Completion::Complete);
    assert!(bounded.telemetry.environment.bounds.compiled);
    assert_eq!(bounded.results.len(), 6);
    request.strategy = Strategy::Exhaustive;
    let exact = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(bounded.results, exact.results);

    let (ordinary, ordinary_roster, mut ordinary_request) = release_support_inputs(0.0, 0, true);
    ordinary_request.strategy = Strategy::Exhaustive;
    let ordinary = engine::recommend(&ordinary, &ordinary_roster, &ordinary_request).unwrap();
    for held in exact.results.iter().filter(|deck| deck.snaps.iter().any(Option::is_some)) {
        let ended =
            ordinary.results.iter().find(|deck| deck.members == held.members && deck.snaps == held.snaps).unwrap();
        assert_eq!(held.power, ended.power);
        assert_eq!(held.order_outcomes.len(), 120);
        assert_eq!(ended.order_outcomes.len(), 120);
        assert!(held.order_outcomes.iter().zip(&ended.order_outcomes).all(|(a, b)| a.0 == b.0 && a.1 > b.1));
    }
    #[cfg(feature = "search-diagnostics")]
    {
        request.strategy = Strategy::BranchAndBound;
        let built = handler::build_card_pool(&data, &roster, &request).unwrap();
        for deck in &exact.results {
            let audit = search::diagnostics::audit_order_caps(&built, deck.members, deck.snaps).unwrap();
            assert_eq!(audit["orders"], 120);
            assert_eq!(audit["violations"], 0, "{}", audit["first"]);
        }
    }
}

fn snap_numeric_inputs(extension: i64) -> (DeckData, Roster, ournotes_sim::live::model::JudgementStream) {
    let mut synth = synthetic_master(5, 1, 5);
    set_column(&mut synth, "MasterMemberCard", &mut |row| row["_liveSkillID"] = json!(1));
    set_column(&mut synth, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(3);
        row["_supportSkillId02"] = json!(0);
    });
    replace_table(
        &mut synth,
        "MasterLiveSkillEffect",
        json!(
            (1..=5)
                .map(|level| json!({"_id":level,"_liveSkillID":1,"_level":level,
            "_skillEffectType":2000,"_effectValue":20_000,"_activationTimeSecond":1.0}))
                .collect::<Vec<_>>()
        ),
    );
    extend_table(&mut synth, "MasterSkillTarget", vec![json!({"_id":900001,"_skillTargetType":4,"_judgement":5})]);
    extend_table(
        &mut synth,
        "MasterSkillCondition",
        vec![json!({"_id":900001,"_conditionType":1030,"_conditionValues":[1],
            "_isPositive":true,"_conditionTargetIDs":[900001]})],
    );
    extend_table(
        &mut synth,
        "MasterSkillConditionSet",
        vec![json!({"_id":900001,"_group":900001,"_conditionIds":[900001]})],
    );
    replace_table(
        &mut synth,
        "MasterSupportSkillEffect",
        json!(
            (1..=5)
                .map(|level| json!({"_id":level,"_supportSkillID":3,"_level":level,
            "_skillTriggerType":1,"_skillTriggerConditionGroup":900001,"_skillEffectType":15000,
            "_effectValue":extension,"_activationTimeSecond":0.0}))
                .collect::<Vec<_>>()
        ),
    );
    set_column(&mut synth, "MasterLiveMusicScore", &mut |row| row["_fullComboCount"] = json!(1));
    let mut document = data_document(&synth, 5, 1, 5);
    document["charts"][0]["notes"] = json!({"id":[1],"op":[1],"judgementType":[1],"timeMs":[750]});
    document["charts"][0]["skillEvents"] = json!({"timeMs":[1000,1000,1000,1000,1000]});
    document["charts"][0]["fevers"] = json!({"startMs":[],"endMs":[]});
    let stream = serde_json::from_value(json!({"frames":[0,1000,1016,2000,2016],"judged":[[3,1,5,750]]})).unwrap();
    (
        DeckData::from_json(&document.to_string()).unwrap(),
        Roster::from_json(&roster_document(5, 1, 5).to_string()).unwrap(),
        stream,
    )
}

fn snap_numeric_request(data: &DeckData, stream: ournotes_sim::live::model::JudgementStream) -> search::SearchRequest {
    let settings = ournotes_sim::live::score::LiveScoreSettings::from_master(&data.master).unwrap();
    let source = &data.charts[0];
    search::SearchRequest {
        objective: search::Objective::LiveScore {
            score_id: SCORE_ID,
            chart: source.chart(&settings).unwrap(),
            play: search::PlayInput::Stream { stream, judgement_types: source.judgement_types.clone() },
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        },
        k: 1,
        constraints: search::Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    }
}

#[test]
fn negative_duration_extension_reverses_native_power_order_and_forces_fallback() {
    use ournotes_sim::Error;
    use ournotes_sim::live::full::{LiveModel, LiveNote, LiveParams, Performer};

    let (data, roster, stream) = snap_numeric_inputs(-1500);
    // At 1000 the live adds two. It is still executing at 2000, when a
    // delayed judgement triggers -1500ms. The next frame files its end at
    // 1000 + ceil(1000 - 1500) = 500: the note at 750 now reads 1 - 2.
    let performer = Performer { live_skill: Some((1, 4)), support_skills: vec![(3, 3)], ..Default::default() };
    let notes = [LiveNote { note_id: 1, time_ms: 750, note_operate_type: 1, judgement_type: 1 }];
    let score = |power| {
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: power,
            music_level: 24,
            converted_note_count: 1,
            music_length_ms: 1750,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let mut model =
            LiveModel::new(&data.master, std::slice::from_ref(&performer), &notes, &[(0, 1000)], params).unwrap();
        let value = model.run(&stream.to_live_play().unwrap()).unwrap();
        #[cfg(feature = "search-diagnostics")]
        assert_eq!(model.filed_scores().0[0].3[2], -1.0);
        value
    };
    assert!(score(100_000) < 0);
    assert!(score(100_000) > score(200_000));

    let pool = Pool::new(&data.master, &roster).unwrap();
    let diagnostic = search::search_best_order_diagnostic(&pool, &snap_numeric_request(&data, stream.clone()));
    assert!(matches!(diagnostic, Err(Error::Domain(message)) if message.contains("negative live-skill duration")));

    let mut wire = joint_request_json("free", false, json!({"kind":"score"}));
    wire["constraints"]["leader"] = json!(1);
    wire["k"] = json!(6);
    wire["execution"]["play"] = json!({"kind":"stream","stream":stream});
    let mut request: RecommendationRequest = serde_json::from_value(wire).unwrap();
    let fallback = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(fallback.completion, Completion::Complete);
    assert!(!fallback.telemetry.environment.bounds.compiled);
    assert!(
        fallback.telemetry.environment.bounds.fallback.as_deref().unwrap().contains("negative live-skill duration")
    );
    request.strategy = Strategy::Exhaustive;
    let exact = engine::recommend(&data, &roster, &request).unwrap();
    assert_eq!(fallback.results.len(), 6);
    assert_eq!(fallback.results, exact.results);
}

#[test]
fn snap_numeric_certificate_keeps_late_frames_but_rejects_noncausal_note_times() {
    use ournotes_sim::Error;

    let (data, roster, stream) = snap_numeric_inputs(0);
    let pool = Pool::new(&data.master, &roster).unwrap();
    let request = snap_numeric_request(&data, stream.clone());
    let ordinary = search::search_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(ordinary.completion, Completion::Complete);
    assert!(ordinary.results[0].score.unwrap() > 0);
    // The stream ends beyond the default 1750ms music length; that ordinary
    // tail is safe because every note is strictly before the finish clamp.
    for last_note in [-500, -250] {
        let mut clamped = request.clone();
        let search::Objective::LiveScore { chart, .. } = &mut clamped.objective else { unreachable!() };
        chart.last_timing_note_ms = last_note; // derived clamp 500 or 750
        let error = search::search_best_order_diagnostic(&pool, &clamped);
        assert!(matches!(error, Err(Error::Domain(message)) if message.contains("music-length finish clamp")));
    }
    let mut early = stream.clone();
    early.judged[0][0] = 0; // chart time 750, played at 0
    let error = search::search_best_order_diagnostic(&pool, &snap_numeric_request(&data, early));
    assert!(matches!(error, Err(Error::Domain(message)) if message.contains("before its chart time")));

    for clock in 0..3 {
        let mut negative = request.clone();
        let search::Objective::LiveScore { chart, play, .. } = &mut negative.objective else { unreachable!() };
        match clock {
            0 => {
                let search::PlayInput::Stream { stream, .. } = play else { unreachable!() };
                stream.frames[0] = -1;
            }
            1 => chart.notes[0].time_ms = -1,
            _ => chart.skill_events[0].time_ms = -1,
        }
        let error = search::search_best_order_diagnostic(&pool, &negative);
        assert!(matches!(error, Err(Error::Domain(message)) if message.contains("nonnegative clock")));
    }
}

fn music_length_frame_inputs(last_timing_note_ms: i32) -> (DeckData, Roster, search::SearchRequest) {
    let (mut data, roster, _) = snap_numeric_inputs(0);
    for row in &mut data.master.live_skill_effects {
        row.activation_time_second = 0.05;
    }
    let stream = serde_json::from_value(json!({
        "frames":[0,1039,1100,1101,1200],"judged":[[1,1,5,1039]]
    }))
    .unwrap();
    let mut request = snap_numeric_request(&data, stream);
    let search::Objective::LiveScore { chart, .. } = &mut request.objective else { unreachable!() };
    chart.last_timing_note_ms = last_timing_note_ms;
    chart.notes[0].time_ms = 1039;
    for event in &mut chart.skill_events {
        event.time_ms = 1100;
    }
    (data, roster, request)
}

#[test]
fn music_length_finish_clamp_rejects_a_note_in_the_same_native_frame() {
    use ournotes_sim::{Error, live::score::get_frame};

    let (data, roster, request) = music_length_frame_inputs(40);
    let pool = Pool::new(&data.master, &roster).unwrap();
    // The note precedes the 1040ms clamp, but both occupy native frame 26.
    // The effect starts at 1100 and its timed end at 1150 is clamped to 1040.
    let search::Objective::LiveScore { chart, .. } = &request.objective else { unreachable!() };
    let (note, length) = (chart.notes[0].time_ms, chart.last_timing_note_ms + 1000);
    assert!(note < length);
    assert_eq!(get_frame(note), get_frame(length));
    let result = search::search_best_order_diagnostic(&pool, &request);
    assert!(matches!(result, Err(Error::Domain(message)) if message.contains("music-length finish clamp frame")));
}

#[test]
fn music_length_finish_clamp_keeps_distinct_frames_and_late_play_updates() {
    use ournotes_sim::live::score::get_frame;

    let (data, roster, request) = music_length_frame_inputs(41);
    let pool = Pool::new(&data.master, &roster).unwrap();
    assert!(get_frame(1039) < get_frame(1041));
    let result = search::search_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert!(result.results[0].score.unwrap() > 0);
    // Skill starts and the final update remain beyond the music length.
    let (exact, _) = search::oracle::brute_force_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(result.results, exact);
}

#[test]
fn snap_numeric_certificate_requires_bounded_drift_and_finite_native_chain() {
    use ournotes_sim::Error;

    let (mut data, roster, stream) = snap_numeric_inputs(0);
    let mut row = data.master.live_skill_effects.iter().find(|row| row.level == 4).unwrap().clone();
    data.master.live_skill_effects.clear();
    row.effect_value = 1_000_000_000;
    for id in 1..=10 {
        row.id = id;
        data.master.live_skill_effects.push(row.clone());
    }
    data.master.reindex().unwrap();
    let pool = Pool::new(&data.master, &roster).unwrap();
    let error = search::search_best_order_diagnostic(&pool, &snap_numeric_request(&data, stream));
    assert!(matches!(error, Err(Error::Domain(message)) if message.contains("factor drift")));

    let (mut data, roster, mut stream) = snap_numeric_inputs(0);
    for setting in &mut data.master.live_settings {
        match setting.key.as_str() {
            "note_score_adjustment_factor" => setting.value = "3e38".into(),
            "assist_score_percent" => setting.value = "1e-38".into(),
            _ => {}
        }
    }
    stream.assist = true;
    let pool = Pool::new(&data.master, &roster).unwrap();
    let error = search::search_best_order_diagnostic(&pool, &snap_numeric_request(&data, stream));
    assert!(matches!(error, Err(Error::Domain(message)) if message.contains("finite normal certificate")));
}

#[test]
fn snap_numeric_certificate_rejects_negative_native_recovery() {
    use ournotes_sim::Error;

    let (mut data, roster, stream) = snap_numeric_inputs(0);
    for row in &mut data.master.support_skill_effects {
        row.skill_effect_type = 3001;
        row.effect_value = -1;
    }
    let pool = Pool::new(&data.master, &roster).unwrap();
    let error = search::search_best_order_diagnostic(&pool, &snap_numeric_request(&data, stream));
    assert!(matches!(error, Err(Error::Domain(message)) if message.contains("life recovery outside")));
}

#[test]
fn snap_diagnostic_proves_nonnegative_power_before_choosing_representatives() {
    use ournotes_sim::Error;

    let (mut data, roster, stream) = snap_numeric_inputs(0);
    for row in &mut data.master.leader_skill_effects {
        if row.leader_skill_id == 4 {
            row.skill_effect_type = 1500;
            row.skill_cumulative_condition_id = 0;
            row.skill_condition_group = 0;
            row.skill_target_ids.clear();
            row.effect_value = -100_000;
        }
    }
    let pool = Pool::new(&data.master, &roster).unwrap();
    let deck = pool.deck([2, 3, 1, 4, 5], [None; 5], [0, 1, 2, 3, 4]).unwrap();
    assert!(pool.deck_power(&deck, Some(&pool.song(10).unwrap()), false).unwrap().power() < 0);
    let error = search::search_best_order_diagnostic(&pool, &snap_numeric_request(&data, stream));
    assert!(matches!(error, Err(Error::Domain(message)) if message.contains("nonnegative power")));
}

#[test]
fn best_order_diagnostic_rejects_external_rank_snapshots() {
    use ournotes_sim::{
        Error,
        scenario::{ResolvedContext, Scenario},
    };

    let (data, roster, stream) = snap_numeric_inputs(0);
    let pool = Pool::new(&data.master, &roster).unwrap();
    let mut request = snap_numeric_request(&data, stream);
    let search::Objective::LiveScore { gekisou, .. } = &mut request.objective else { unreachable!() };
    *gekisou = Some(search::GekisouObjective { seeds: search::SeedSet::List(vec![0]), fevers: Vec::new() });
    let mut context =
        ResolvedContext::resolve(&data.master, Scenario::Free(10), Some(SCORE_ID), &[], pool.player.events.clone())
            .unwrap();
    context.rank_confirmations = Some(Vec::new());
    request.objective = request.objective.in_scenario(context);
    let error = search::search_best_order_diagnostic(&pool, &request);
    assert!(matches!(error, Err(Error::Unsupported(message)) if message.contains("external rank snapshots")));
}
