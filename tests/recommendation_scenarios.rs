//! Every exposed legal mode/execution/metric branch and declared network queue behavior.
#[path = "recommend_fixture_export.rs"]
#[allow(dead_code)]
mod fixtures;
use ournotes_deck::{
    Error,
    live::model::{JudgementStream, JustRule},
    scenario::Scenario,
    search::{
        Completion, Constraints, GekisouObjective, Objective, PlayInput, SearchRequest, SeedSet, expectation,
        recommendation::*,
    },
};
use serde_json::{Value, json};

fn json_request(mode: &str, id: i64, kind: &str, gk: bool, metric: Value) -> Value {
    let execution = match kind {
        "power" => json!({"kind":"power"}),
        "skip" => json!({"kind":"skip","scoreId":1004}),
        _ => json!({"kind":"live","scoreId":1004,"gekisou":gk,"play":{"kind":"theoreticalBest"}}),
    };
    let mut q = json!({"format":REQUEST_FORMAT,"execution":execution,"scenario":{"kind":mode,"musicId":id},"context":fixtures::fixture_context(kind=="skip",matches!(mode,"battle"|"arena")),"metric":metric,"constraints":{"leader":1,"noSnaps":true},"k":5,"strategy":{"kind":"exhaustive"},"limits":{"timeLimitMs":null,"maxCandidates":null,"cacheEntries":32}});
    if kind == "live" {
        q["seedLaw"] = json!({"atoms":fixtures::fixture_law().atoms(),"provenance":"deterministic synthetic law; no native population law claimed"});
    }
    if matches!(mode, "battle" | "arena") && kind == "live" && gk {
        let (d, _) = fixtures::small_fixture();
        let dc = d.data_chart(1004).unwrap();
        let chart = d.chart(1004).unwrap();
        let scene =
            if mode == "battle" { Scenario::Battle(id) } else { Scenario::Arena(id) }.resolve(&d.master).unwrap();
        let setup = scene.gekisou_setup(&dc.fevers);
        let rule = JustRule::new(&d.master, &setup).unwrap();
        let stream = JudgementStream::theoretical_best_gekisou(&chart, &dc.judgement_types, &rule).unwrap();
        let factors = ournotes_deck::live::full::gekisou_rank_factors(&d.master, &scene.gekisou_missions).unwrap();
        let frame = stream.frames.len() - 8;
        q["networkConfirmations"] = json!(
            (0..3)
                .map(|range| json!({"frame":frame,"range":range,"rank":2,"percent":factors[range][1]}))
                .collect::<Vec<_>>()
        );
    }
    q
}
fn parsed(v: &Value) -> RecommendationRequest {
    serde_json::from_value(v.clone()).unwrap()
}

fn stream_request_text(play: &str) -> String {
    format!(
        r#"{{"format":"{REQUEST_FORMAT}","execution":{{"play":{play},"gekisou":true,"scoreId":1004,"kind":"live"}},"metric":{{"kind":"score"}}}}"#
    )
}
fn request_stream(r: RecommendationRequest) -> JudgementStream {
    match r.execution {
        Execution::Live { play: PlayPolicy::Stream { stream }, .. } => stream,
        _ => panic!("expected a stream request"),
    }
}

#[test]
fn decimal_delta_times_use_direct_f32_parsing_and_reject_non_numeric_or_invalid_steps() {
    let play = r#"{"stream":{"frames":[0,10,20,30,40,50,60],"deltaTimes":[0.016666666666666666,0.016666668,0,0.001,0.037000001,1.0000000596046448,1]},"kind":"stream"}"#;
    let text = stream_request_text(play);
    let value: Value = serde_json::from_str(&text).unwrap();
    let direct: JudgementStream = serde_json::from_str(&value["execution"]["play"]["stream"].to_string()).unwrap();
    let got = request_stream(serde_json::from_str(&text).unwrap());
    let got_bits: Vec<_> = got.delta_times().unwrap().iter().map(|v| v.to_bits()).collect();
    let expected_bits: Vec<_> = direct.delta_times().unwrap().iter().map(|v| v.to_bits()).collect();
    assert_eq!(got_bits, expected_bits);
    assert_eq!(&got_bits[..2], &[0x3c888889, 0x3c888889]);
    let from_value = request_stream(serde_json::from_value(value.clone()).unwrap());
    let direct_value: JudgementStream = serde_json::from_value(value["execution"]["play"]["stream"].clone()).unwrap();
    assert_eq!(from_value, direct_value);

    for invalid in [
        r#"{"numerator":1,"denominator":60}"#,
        r#"{"$serde_json::private::Number":"0.01"}"#,
        r#""0.016666668""#,
        "true",
        "null",
        "NaN",
        "Infinity",
    ] {
        let play = format!(r#"{{"kind":"stream","stream":{{"frames":[0],"deltaTimes":[{invalid}]}}}}"#);
        assert!(serde_json::from_str::<RecommendationRequest>(&stream_request_text(&play)).is_err(), "{invalid}");
    }
    for invalid in ["-0.016666668", "1e40", "1e1000"] {
        let play = format!(r#"{{"kind":"stream","stream":{{"frames":[0],"deltaTimes":[{invalid}]}}}}"#);
        match serde_json::from_str::<RecommendationRequest>(&stream_request_text(&play)) {
            Err(_) => {}
            Ok(r) => assert!(request_stream(r).delta_times().is_err(), "{invalid}"),
        }
    }
    let duplicate = r#"{"kind":"stream","stream":{"frames":[0],"deltaTimes":[0.01],"deltaTimes":[0.02]}}"#;
    assert!(serde_json::from_str::<RecommendationRequest>(&stream_request_text(duplicate)).is_err());
}

#[test]
fn execution_and_play_wire_keep_variant_field_presence_and_duplicate_guards() {
    for valid in [r#"{"kind":"power"}"#, r#"{"musicId":null,"kind":"power"}"#] {
        assert!(matches!(
            serde_json::from_str::<Execution>(valid).unwrap(),
            Execution::Power { music_id: None, event_parameter: false }
        ));
    }
    for invalid in [
        r#"{"kind":"power","eventParameter":null}"#,
        r#"{"kind":"power","scoreId":1004}"#,
        r#"{"kind":"skip","scoreId":1004,"musicId":null}"#,
        r#"{"kind":"live","scoreId":1004,"gekisou":true,"play":{"kind":"theoreticalBest"},"musicId":null}"#,
        r#"{"kind":"live","scoreId":1004,"play":{"kind":"theoreticalBest"}}"#,
        r#"{"kind":"power","kind":"power"}"#,
        r#"{"kind":"unknown"}"#,
    ] {
        assert!(serde_json::from_str::<Execution>(invalid).is_err(), "{invalid}");
    }
    for invalid in
        [r#"{"kind":"theoreticalBest","stream":null}"#, r#"{"kind":"stream"}"#, r#"{"kind":"stream","stream":null}"#]
    {
        assert!(serde_json::from_str::<PlayPolicy>(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn explicit_variable_delta_times_json_matches_typed_small_pool_oracle() {
    let (data, roster) = fixtures::small_fixture();
    let law = fixtures::fixture_law();
    let dc = data.data_chart(1004).unwrap();
    let chart = data.chart(1004).unwrap();
    let ctx = fixtures::fixture_context(false, false)
        .resolve(&data.master, Scenario::Free(10), Some(1004), &dc.fevers)
        .unwrap();
    let pool = ctx.pool(&data.master, &roster).unwrap();
    let mut stream = JudgementStream::theoretical_best_gekisou(
        &chart,
        &dc.judgement_types,
        &JustRule::new(&data.master, &ctx.gekisou).unwrap(),
    )
    .unwrap();
    stream.delta_times =
        Some((0..stream.frames.len()).map(|i| [1.0f32 / 30.0, 1.0 / 120.0, 0.016666668][i % 3]).collect());
    let q = SearchRequest {
        objective: Objective::LiveScore {
            score_id: 1004,
            chart,
            play: PlayInput::Stream { stream: stream.clone(), judgement_types: dc.judgement_types.clone() },
            event: false,
            exclude_snap_skills: false,
            gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: dc.fevers.clone() }),
        }
        .in_scenario(ctx),
        k: 5,
        constraints: Constraints { leader: Some(1), no_snaps: true, ..Default::default() },
        time_limit: None,
    };
    let oracle = expectation::oracle(&pool, &q, &law).unwrap();
    let mut value = json_request("free", 10, "live", true, json!({"kind":"score"}));
    value["execution"]["play"] = json!({"kind":"stream","stream":stream});
    let json_request: RecommendationRequest = serde_json::from_str(&value.to_string()).unwrap();
    assert_eq!(request_stream(json_request.clone()).delta_times, stream.delta_times);
    let result = recommend(&data, &roster, &json_request).unwrap();
    assert_eq!((result.completion, result.optimality), (Completion::Complete, Optimality::Proven));
    for (got, want) in result.results.iter().zip(&oracle.results) {
        assert_eq!((got.members, got.snaps, got.power), (want.members, want.snaps, want.power));
        assert_eq!(got.expected_score, Some(want.evaluation.expected_score.into()));
        for (a, b) in got.atoms.iter().zip(&want.evaluation.outcomes) {
            assert_eq!((a.root_seed, a.score, a.performance_order), (b.root_seed, b.final_score, b.performance_order));
        }
    }
}

#[test]
fn all_legal_mode_execution_metric_branches_return_auditable_conditional_results() {
    let (data, roster) = fixtures::small_fixture();
    let mut denominator = 0;
    let metrics = [
        json!({"kind":"score"}),
        json!({"kind":"scoreAtLeast","threshold":100000}),
        json!({"kind":"clientEventPoints","eventId":7}),
        json!({"kind":"conditionalClientEventItems","eventId":7,"resourceType":11,"resourceId":9}),
    ];
    for (mode, id) in
        [("free", 10), ("mission", 10), ("battle", 10), ("arena", 80), ("challenge", 70), ("challenge", 71)]
    {
        let power =
            recommend(&data, &roster, &parsed(&json_request(mode, id, "power", false, json!({"kind":"power"}))))
                .unwrap();
        assert_eq!(power.completion, Completion::Complete);
        assert_eq!(power.result_identity, "canonicalMemberSet");
        denominator += 1;
        if matches!(mode, "free" | "challenge") {
            for metric in &metrics {
                let out =
                    recommend(&data, &roster, &parsed(&json_request(mode, id, "skip", false, metric.clone()))).unwrap();
                assert_eq!(out.completion, Completion::Complete);
                assert!(!out.results.is_empty());
                denominator += 1;
            }
        }
        for gk in [false, true] {
            if !gk && matches!(mode, "mission" | "battle" | "arena") {
                continue;
            }
            for metric in &metrics {
                let out =
                    recommend(&data, &roster, &parsed(&json_request(mode, id, "live", gk, metric.clone()))).unwrap();
                assert_eq!(
                    (out.completion, out.optimality, out.exit_reason),
                    (Completion::Complete, Optimality::Proven, ExitReason::Exhausted)
                );
                assert!(!out.results.is_empty());
                assert_eq!(out.stats.visited_candidates, 24);
                assert!(out.stats.peak_retained_decks <= 5);
                assert!(out.probability_law["provenance"].is_string());
                assert!(out.resolved_context["dataProvenance"]["synthetic"].as_bool().unwrap());
                denominator += 1;
            }
        }
    }
    assert_eq!(
        denominator, 54,
        "six power contexts + three skip contexts x4 metrics + nine played contexts x4 metrics"
    );
}

#[test]
fn solo_gekisou_score_and_terminal_event_payoffs_match_unpruned_oracle() {
    let (data, roster) = fixtures::small_fixture();
    let law = fixtures::fixture_law();
    for (mode, id, scene) in [
        ("free", 10, Scenario::Free(10)),
        ("mission", 10, Scenario::Mission(10)),
        ("challenge", 70, Scenario::Challenge(70)),
        ("challenge", 71, Scenario::Challenge(71)),
    ] {
        let dc = data.data_chart(1004).unwrap();
        let chart = data.chart(1004).unwrap();
        let ctx = fixtures::fixture_context(false, false).resolve(&data.master, scene, Some(1004), &dc.fevers).unwrap();
        let pool = ctx.pool(&data.master, &roster).unwrap();
        let stream = JudgementStream::theoretical_best_gekisou(
            &chart,
            &dc.judgement_types,
            &JustRule::new(&data.master, &ctx.gekisou).unwrap(),
        )
        .unwrap();
        let q = SearchRequest {
            objective: Objective::LiveScore {
                score_id: 1004,
                chart,
                play: PlayInput::Stream { stream, judgement_types: dc.judgement_types.clone() },
                event: false,
                exclude_snap_skills: false,
                gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: dc.fevers.clone() }),
            }
            .in_scenario(ctx.clone()),
            k: 5,
            constraints: Constraints { leader: Some(1), no_snaps: true, ..Default::default() },
            time_limit: None,
        };
        for metric in [
            Metric::Score,
            Metric::ClientEventPoints { event_id: 7 },
            Metric::ConditionalClientEventItems { event_id: 7, resource_type: 11, resource_id: 9 },
        ] {
            let input = fixtures::fixture_context(false, false).event_payoff.unwrap();
            let oracle = expectation::oracle_with_payoff_factory(
                &pool,
                &q,
                &law,
                || Ok(()),
                |physical, o, _| match metric {
                    Metric::Score => Ok(o.final_score as i128),
                    Metric::ClientEventPoints { .. } => Ok(ctx
                        .preview_event_points(&pool, &physical.as_deck(), &input, 7, o.final_score)?
                        .points_for(7) as i128),
                    _ => ournotes_deck::scenario::item_payoff(
                        &ctx.preview_event_items(&pool, &physical.as_deck(), &input, 7, o.final_score)?,
                        7,
                        11,
                        9,
                    ),
                },
            )
            .unwrap();
            let out = recommend(
                &data,
                &roster,
                &parsed(&json_request(mode, id, "live", true, serde_json::to_value(&metric).unwrap())),
            )
            .unwrap();
            for (a, b) in out.results.iter().zip(&oracle.results) {
                assert_eq!((a.members, a.snaps, a.power), (b.members, b.snaps, b.power));
                assert_eq!(a.expected_score, Some(b.evaluation.expected_score.into()));
                assert_eq!(a.expected_payoff, Fraction::from(b.evaluation.expected_payoff));
            }
        }
    }
}

#[test]
fn batched_network_packets_are_consumed_one_eligible_range_per_frame() {
    let (data, roster) = fixtures::small_fixture();
    for (mode, id) in [("battle", 10), ("arena", 80)] {
        let batch = json_request(mode, id, "live", true, json!({"kind":"score"}));
        let mut serialized = batch.clone();
        for (range, c) in serialized["networkConfirmations"].as_array_mut().unwrap().iter_mut().enumerate() {
            c["frame"] = json!(c["frame"].as_u64().unwrap() + range as u64);
        }
        let a = recommend(&data, &roster, &parsed(&batch)).unwrap();
        let b = recommend(&data, &roster, &parsed(&serialized)).unwrap();
        assert_eq!(a.results, b.results);
        let frame = batch["networkConfirmations"][0]["frame"].as_u64().unwrap() as usize;
        for result in &a.results {
            for atom in &result.atoms {
                assert_eq!(atom.network_applications, vec![(0, frame), (1, frame + 1), (2, frame + 2)]);
            }
        }
        let mut early = batch.clone();
        for c in early["networkConfirmations"].as_array_mut().unwrap() {
            c["frame"] = json!(0);
        }
        let early = recommend(&data, &roster, &parsed(&early)).unwrap();
        for result in &early.results {
            for atom in &result.atoms {
                assert!(atom.network_applications[0].1 > 0);
                assert!(atom.network_applications.windows(2).all(|w| w[1].1 > w[0].1));
            }
        }
    }
}

#[test]
fn invalid_native_modes_and_missing_authority_are_explicit_errors() {
    let (data, roster) = fixtures::small_fixture();
    for (mode, id) in [("mission", 10), ("battle", 10), ("arena", 80)] {
        assert!(matches!(
            recommend(&data, &roster, &parsed(&json_request(mode, id, "live", false, json!({"kind":"score"})))),
            Err(Error::Game(_))
        ));
        assert!(matches!(
            recommend(&data, &roster, &parsed(&json_request(mode, id, "skip", false, json!({"kind":"score"})))),
            Err(Error::Input(_))
        ));
    }
    let mut network = json_request("battle", 10, "live", true, json!({"kind":"score"}));
    network.as_object_mut().unwrap().remove("networkConfirmations");
    assert!(matches!(recommend(&data, &roster, &parsed(&network)), Err(Error::Unsupported(_))));
    let mut q = json_request("free", 10, "live", false, json!({"kind":"score"}));
    q.as_object_mut().unwrap().remove("seedLaw");
    assert!(matches!(recommend(&data, &roster, &parsed(&q)), Err(Error::Input(_))));
    let mut q = json_request(
        "free",
        10,
        "skip",
        false,
        json!({"kind":"conditionalClientEventItems","eventId":7,"resourceType":11,"resourceId":9}),
    );
    q["context"]["eventPayoff"]["selectedRewards"] = Value::Null;
    assert!(matches!(recommend(&data, &roster, &parsed(&q)), Err(Error::Unsupported(_))));
}

#[test]
fn unknown_nested_constraints_fail_at_all_serde_boundaries() {
    assert!(serde_json::from_value::<Constraints>(json!({"leader":1,"requiredSnaps":[1]})).is_err());
    let mut q = json_request("free", 10, "live", false, json!({"kind":"score"}));
    q["constraints"]["fixedSlots"] = json!([1, 2, 3, 4, 5]);
    let error = serde_json::from_value::<RecommendationRequest>(q).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}
