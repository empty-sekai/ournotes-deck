use super::results::{bound, ceil_div, exact, floor_div, teams};
use super::*;

#[test]
fn exact_fraction_rounding_covers_signed_and_unsigned_limits() {
    let half_unsigned = 1u128 << 127;
    for (n, d, lower, upper) in [
        (7, 3, 2, 3),
        (-7, 3, -3, -2),
        (i128::MIN, 1, i128::MIN, i128::MIN),
        (i128::MAX, 1, i128::MAX, i128::MAX),
        (i128::MIN, i128::MAX as u128, -2, -1),
        (i128::MAX, i128::MAX as u128, 1, 1),
        (i128::MIN, half_unsigned, -1, -1),
        (i128::MAX, half_unsigned, 0, 1),
        (1, half_unsigned, 0, 1),
        (-1, half_unsigned, -1, 0),
        (i128::MIN, u128::MAX, -1, 0),
        (i128::MAX, u128::MAX, 0, 1),
        (0, u128::MAX, 0, 0),
    ] {
        assert_eq!(floor_div(n, d), lower, "floor of {n}/{d}");
        assert_eq!(ceil_div(n, d), upper, "ceiling of {n}/{d}");
    }
    assert_eq!(
        exact(&Fraction { numerator: i128::MIN.to_string(), denominator: u128::MAX.to_string() }).unwrap(),
        (i128::MIN, u128::MAX)
    );
    assert!(exact(&Fraction { numerator: "1".into(), denominator: "0".into() }).is_err());
}

#[test]
fn integer_result_bounds_preserve_exact_rounding_at_fraction_limits() {
    let denominator = 1u128 << 127;
    for (numerator, lower, upper) in [(1, 0, 1), (-1, -1, 0)] {
        assert_eq!(bound(&Metric::Score, numerator, denominator, false), json!(lower));
        assert_eq!(bound(&Metric::Score, numerator, denominator, true), json!(upper));
    }
    assert_eq!(bound(&Metric::Score, i128::MIN, 1, true), json!(i128::MIN));
    assert_eq!(bound(&Metric::Score, i128::MAX, 1, false), json!(i128::MAX));
}

#[test]
fn probability_result_bounds_enclose_exact_rationals() {
    use crate::search::{expectation::ExactExpectation, interval_topk::exact_in_interval};
    use ournotes_sim::live::certified::F64Interval;
    for metric in
        [Metric::ScoreAtLeast { threshold: 1 }, Metric::ScoreAndLifeAtLeast { threshold: 1, min_final_life: 1 }]
    {
        for (n, d) in [
            (1, 3),
            (1, 10),
            (1, 2),
            (0, 1),
            (1, 1),
            (0, u128::MAX),
            (i128::MAX, i128::MAX as u128),
            (1, 1u128 << 127),
            (1, u128::MAX),
            (i128::MAX, 1u128 << 127),
            (i128::MAX, u128::MAX),
            (i128::MAX - 1, u128::MAX),
        ] {
            let lower = bound(&metric, n, d, false).as_f64().unwrap();
            let upper = bound(&metric, n, d, true).as_f64().unwrap();
            assert!(
                exact_in_interval(
                    ExactExpectation { numerator: n, denominator: d },
                    F64Interval::new(lower, upper).unwrap()
                )
                .unwrap(),
                "{n}/{d} outside [{lower}, {upper}]"
            );
            if n == 0 || n as u128 == d {
                assert_eq!(lower, upper);
            }
        }
    }
    let metric = Metric::ScoreAtLeast { threshold: 1 };
    assert!(bound(&metric, 1, 3, true).as_f64().unwrap() > 1.0 / 3.0);
    assert!(bound(&metric, 1, 10, false).as_f64().unwrap() < 0.1);
}

#[test]
fn uniform_lottery_average_keeps_an_unsigned_result_denominator() {
    use crate::search::{
        certified_search::{OrderScoreInterval, PayoffMap, aggregate_orders},
        expectation::ExactExpectation,
        uniform,
    };
    use ournotes_sim::live::certified::F64Interval;
    let orders = uniform::all_orders()
        .into_iter()
        .enumerate()
        .map(|(index, order)| OrderScoreInterval {
            order,
            mean: F64Interval::point(if index == 0 { 2f64.powi(-121) } else { 0.0 }).unwrap(),
            support: (0, 1),
            exact_mean: Some(ExactExpectation {
                numerator: i128::from(index == 0),
                denominator: if index == 0 { 1u128 << 121 } else { 1 },
            }),
            final_life: None,
            tails: Default::default(),
            refined_payoff: None,
        })
        .collect();
    let evaluation = aggregate_orders(orders, &PayoffMap::Score).unwrap();
    let fraction = evaluation.exact_score.unwrap();
    assert_eq!(fraction.numerator, 1);
    assert_eq!(fraction.denominator, 120u128 << 121);
    assert!(fraction.denominator > i128::MAX as u128);
    assert_eq!(bound(&Metric::Score, fraction.numerator, fraction.denominator, false), json!(0));
    assert_eq!(bound(&Metric::Score, fraction.numerator, fraction.denominator, true), json!(1));
}

#[test]
fn certified_team_keeps_rational_bounds_without_fabricating_exact_values_or_orders() {
    let search: RecommendationRequest = serde_json::from_value(json!({
        "format":crate::types::REQUEST_FORMAT,"execution":{"kind":"power"},"metric":{"kind":"score"}
    }))
    .unwrap();
    let parsed = Parsed {
        kind: GoalKind::MissionLive,
        goal: Value::Null,
        metric: None,
        exclusions: Exclusions::default(),
        k: 5,
        search,
    };
    let outcome = RecommendationOutcome {
        format: crate::types::RESULT_FORMAT,
        completion: crate::search::Completion::Complete,
        optimality: Optimality::Proven,
        exit_reason: crate::types::ExitReason::Exhausted,
        result_identity: "team",
        metric: Metric::Score,
        aggregation: crate::types::Aggregation::Expected,
        maximum_model: None,
        player_goal: None,
        strategy: Strategy::Exhaustive,
        probability_law: Value::Null,
        proof_scope: "fixture",
        resolved_context: Value::Null,
        telemetry: Default::default(),
        elapsed_ms: 0.0,
        results: vec![RecommendedDeck {
            members: [1, 2, 3, 4, 5],
            snaps: [None; 5],
            power: 100,
            objective_value: None,
            expected_score: None,
            expected_payoff: None,
            term_payoffs: None,
            level_payoffs: None,
            score_interval: Some(FractionInterval::from_f64(150.5, 150.75).unwrap()),
            payoff_interval: Some(FractionInterval::from_f64(150.5, 150.75).unwrap()),
            rank_certified: Some(true),
            score_summary: None,
            best_order: None,
            order_outcomes: vec![],
        }],
    };
    let rows = teams(&outcome, &parsed, true).unwrap();
    let wire = serde_json::to_value(&rows[0]).unwrap();
    assert!(wire["value"]["exact"].is_null());
    assert!(wire["orders"].is_null());
    assert_eq!(wire["rankCertified"], true);
    assert_eq!(wire["value"]["interval"]["lower"]["numerator"], "301");
    assert_eq!(wire["value"]["interval"]["lower"]["denominator"], "2");
    assert_eq!(wire["value"]["score"], 150);
}

#[test]
fn every_order_index_round_trips() {
    let mut seen = BTreeSet::new();
    for i in 0..ORDERS {
        let order = order_of_index(i);
        assert_eq!(order_index(&order), i);
        assert!(seen.insert(order));
    }
}

#[test]
fn challenge_points_and_song_ranking_have_distinct_native_payoffs() {
    let value = capabilities();
    for kind in [GoalKind::FreeLive, GoalKind::MissionLive, GoalKind::BattleLive, GoalKind::ArenaLive, GoalKind::Skip] {
        assert!(kind.metrics().contains(&"challengePoints"));
        assert_eq!(support(kind, "challengePoints"), Support::Proven);
    }
    assert!(!GoalKind::ChallengeLive.metrics().contains(&"challengePoints"));
    assert_eq!(metric_fields("challengePoints"), Some(["eventId", "consumption"].as_slice()));
    assert_eq!(value["eventMusicRanking"]["goal"], "challengeLive");
    assert_eq!(value["eventMusicRanking"]["metric"], "score");
    assert_eq!(value["eventMusicRanking"]["serverRankPrediction"], false);
}

#[test]
fn frontend_result_clock_and_room_are_accepted_without_rounding_ticks() {
    let raw = r#"{"format":"ournotes-deck.recommendation-request/2","goal":{"kind":"battleLive","musicId":10,"difficulty":"expert","rank":1},"metric":{"kind":"eventPoints","eventId":7,"consumption":1},"room":{"players":5,"othersAverageScore":null},"eventContext":{"resultClock":{"kind":"played","liveStartJstTicks":null,"serverNowJstTicks":639268416000000001}},"limits":{"timeLimitMs":null}}"#;
    let parsed: RequestWire = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed.event_context.unwrap().result_clock.unwrap().server_now_jst_ticks, 639268416000000001);
    assert_eq!(parsed.room.unwrap().players, 5);
}

#[test]
fn capabilities_report_missing_domains_without_calling_them_complete() {
    let value = capabilities();
    assert_eq!(value["support"]["battleLive"]["score"], "proven");
    assert_eq!(value["support"]["arenaLive"]["eventPoints"], "proven");
    assert_eq!(value["rankConfirmation"], "onCompletion");
    assert_eq!(value["support"]["freeLive"]["score"], "proven");
    assert_eq!(value["luckMissions"], true);
    assert_eq!(value["luckGreatConversions"], false);
    assert_eq!(value["requestFormat"], REQUEST_FORMAT);
    assert_eq!(value["support"]["freeLive"]["scoreAndLife"], "proven");
    assert_eq!(value["scoreAndLife"]["requiresCompleteJudgementStream"], true);
    assert_eq!(value["skip"]["musicIdOrChallengeMusicId"], true);
    assert_eq!(value["eventIds"]["skip"], "challengeMusicId");
}
