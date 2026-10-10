//! Combined metrics over complete synthetic domains, against the weighted sums of independently evaluated terms.
use super::{
    EVENT_ID, SCORE_ID, account_recommendation_fixture, account_request, context_document, data_document,
    joint_request_json, replace_table, roster_document, set_column, synthetic_master,
};
use ournotes_search::{
    auxiliary::evaluate_built,
    engine,
    handler::build_card_pool,
    recommendation::{Status, capabilities},
    search::Completion,
    types::{Fraction, Optimality, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData};
use serde_json::{Value, json};

fn inputs() -> (DeckData, Roster) {
    let mut synth = synthetic_master(6, 2, 5);
    // A lower grade can pay more of one reward and less of another.
    set_column(&mut synth, "MasterLiveEventPoint", &mut |row| {
        row["_value"] = json!(match row["_scoreRank"].as_i64().unwrap() {
            2 => 300,
            3 => 900,
            4 => 20,
            _ => 80,
        });
    });
    set_column(&mut synth, "MasterLiveChallengePoint", &mut |row| {
        row["_value"] = json!(match row["_scoreRank"].as_i64().unwrap() {
            2 => 7,
            3 => 2,
            4 => 40,
            _ => 11,
        });
    });
    replace_table(
        &mut synth,
        "MasterEventEffect",
        json!([
            {"_id":1,"_eventId":EVENT_ID,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":1,
             "_rank1EffectValue":10000,"_rank2EffectValue":10000,"_rank3EffectValue":10000,
             "_rank4EffectValue":10000,"_rank5EffectValue":10000},
            {"_id":2,"_eventId":EVENT_ID,"_eventBonusType":0,"_resourceTypeConstraint":2,"_memberCardId":4,
             "_rank1EffectValue":2500,"_rank2EffectValue":2500,"_rank3EffectValue":2500,
             "_rank4EffectValue":2500,"_rank5EffectValue":2500},
            {"_id":3,"_eventId":EVENT_ID,"_eventBonusType":1,"_resourceTypeConstraint":2,"_memberCardId":6,
             "_rank1EffectValue":20000,"_rank2EffectValue":20000,"_rank3EffectValue":20000,
             "_rank4EffectValue":20000,"_rank5EffectValue":20000}
        ]),
    );
    (
        DeckData::from_json(&data_document(&synth, 6, 2, 5).to_string()).unwrap(),
        Roster::from_json(&roster_document(6, 2, 5).to_string()).unwrap(),
    )
}

fn request(mode: &str, gekisou: bool, skip: bool, metric: Value) -> RecommendationRequest {
    let mut value = joint_request_json(mode, gekisou, metric);
    if skip {
        value["execution"] = json!({"kind":"skip","scoreId":SCORE_ID});
        value["context"] = context_document(true, false, false);
    }
    value["k"] = json!(100);
    value["strategy"] = json!({"kind":"exhaustive"});
    serde_json::from_value(value).unwrap()
}

fn combined(terms: &[(Value, u32)]) -> Value {
    let terms: Vec<_> = terms.iter().map(|(metric, weight)| json!({"metric":metric,"weight":weight})).collect();
    json!({"kind":"combined","levels":[{"terms":terms}]})
}

fn numerator(value: &Fraction) -> i128 {
    value.numerator.parse().unwrap()
}

fn points() -> Value {
    json!({"kind":"clientEventPoints","eventId":EVENT_ID})
}
fn challenge_points() -> Value {
    json!({"kind":"clientChallengePoints","eventId":EVENT_ID})
}
fn items() -> Value {
    json!({"kind":"rankedEventItems","eventId":EVENT_ID,"resourceType":11,"resourceId":9})
}

type Team = ([i64; 5], [Option<i64>; 5], i32);

/// Every team of the complete domain with the independently evaluated expectation numerator of each term.
struct Domain {
    terms: Vec<Value>,
    teams: Vec<(Team, Vec<i128>)>,
}
impl Domain {
    fn new(data: &DeckData, roster: &Roster, base: &RecommendationRequest, terms: Vec<Value>) -> Self {
        let complete = engine::recommend(data, roster, base).unwrap();
        assert_eq!(complete.completion, Completion::Complete);
        assert!(complete.results.len() < 100, "the reference holds the whole domain");
        let problems: Vec<_> = terms
            .iter()
            .map(|metric| {
                let mut term = base.clone();
                term.metric = serde_json::from_value(metric.clone()).unwrap();
                build_card_pool(data, roster, &term).unwrap()
            })
            .collect();
        let teams = complete
            .results
            .iter()
            .map(|row| {
                let values = problems
                    .iter()
                    .map(|problem| {
                        let value = evaluate_built(problem, row.members, row.snaps).unwrap();
                        numerator(value.results[0].expected_payoff.as_ref().unwrap())
                    })
                    .collect();
                ((row.members, row.snaps, row.power), values)
            })
            .collect();
        Self { terms, teams }
    }

    fn metric(&self, weights: &[(usize, u32)]) -> Value {
        let terms: Vec<_> = weights.iter().map(|&(term, weight)| (self.terms[term].clone(), weight)).collect();
        combined(&terms)
    }

    /// The teams in the order of their weighted sums, then power and canonical identity.
    fn ranked(&self, weights: &[(usize, u32)]) -> Vec<(Team, i128)> {
        let mut rows: Vec<_> = self
            .teams
            .iter()
            .map(|(team, values)| {
                (*team, weights.iter().map(|&(term, weight)| i128::from(weight) * values[term]).sum::<i128>())
            })
            .collect();
        rows.sort_by(|(a, x), (b, y)| y.cmp(x).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        rows
    }

    fn value(&self, team: &Team, term: usize) -> i128 {
        self.teams.iter().find(|(candidate, _)| candidate == team).expect("a team of the domain").1[term]
    }

    fn best(&self, term: usize) -> i128 {
        self.teams.iter().map(|(_, values)| values[term]).max().expect("a nonempty domain")
    }
}

const POINTS: usize = 0;
const CHALLENGE: usize = 1;
const ITEMS: usize = 2;
const SCORE: usize = 3;
const AT_LEAST: usize = 4;
const CAPPED: usize = 5;

fn domain(data: &DeckData, roster: &Roster, mode: &str, gekisou: bool, skip: bool) -> (RecommendationRequest, Domain) {
    let score = json!({"kind":"score"});
    let base = request(mode, gekisou, skip, score.clone());
    let scores = engine::recommend(data, roster, &base).unwrap();
    let middle = scores.results[scores.results.len() / 2].expected_score.as_ref().unwrap();
    let threshold = (numerator(middle) / middle.denominator.parse::<i128>().unwrap()) as i32;
    let terms = vec![
        points(),
        challenge_points(),
        items(),
        score,
        json!({"kind":"scoreAtLeast","threshold":threshold}),
        json!({"kind":"cappedScore","threshold":threshold}),
    ];
    let domain = Domain::new(data, roster, &base, terms);
    (base, domain)
}

#[test]
fn combined_metrics_rank_by_the_weighted_sum_of_independent_terms() {
    let (data, roster) = inputs();
    for (mode, gekisou, skip) in [("free", false, false), ("mission", true, false), ("free", false, true)] {
        let (base, domain) = domain(&data, &roster, mode, gekisou, skip);
        let cases: [&[(usize, u32)]; 9] = [
            &[(POINTS, 2), (CHALLENGE, 35)],
            &[(CHALLENGE, 35), (POINTS, 2)],
            &[(POINTS, 1), (ITEMS, 3), (CHALLENGE, 1)],
            &[(ITEMS, 5), (POINTS, 1)],
            &[(SCORE, 1), (CHALLENGE, 1000)],
            &[(AT_LEAST, 1_000_000), (POINTS, 1)],
            &[(CAPPED, 1), (ITEMS, 7)],
            &[(CHALLENGE, 3)],
            &[(SCORE, 2), (CAPPED, 1)],
        ];
        for weights in cases {
            let expected = domain.ranked(weights);
            let mut search = base.clone();
            search.metric = serde_json::from_value(domain.metric(weights)).unwrap();
            search.k = 5;
            for (strategy, cache_entries) in
                [(Strategy::Exhaustive, 0), (Strategy::BranchAndBound, 0), (Strategy::BranchAndBound, 64)]
            {
                let bounded = matches!(strategy, Strategy::BranchAndBound);
                search.strategy = strategy;
                search.limits.cache_entries = cache_entries;
                let actual = engine::recommend(&data, &roster, &search).unwrap();
                let label = format!("{mode} skip={skip} {weights:?} bounded={bounded}");
                assert_eq!(actual.completion, Completion::Complete, "{label}");
                assert_eq!(actual.optimality, Optimality::Proven, "{label}");
                if bounded && !skip {
                    assert!(actual.telemetry.environment.bounds.compiled, "{label}");
                    assert!(actual.telemetry.environment.bounds.fallback.is_none(), "{label}");
                }
                assert_eq!(actual.results.len(), expected.len().min(5), "{label}");
                for (row, (team, total)) in actual.results.iter().zip(&expected) {
                    assert_eq!((row.members, row.snaps, row.power), *team, "{label}");
                    assert_eq!(numerator(row.expected_payoff.as_ref().unwrap()), *total, "{label}");
                    let terms: Vec<_> = row
                        .term_payoffs
                        .as_ref()
                        .expect("term payoffs")
                        .iter()
                        .map(|term| numerator(term.as_ref().expect("a term without terminal life")))
                        .collect();
                    let expected: Vec<_> = weights.iter().map(|&(term, _)| domain.value(team, term)).collect();
                    assert_eq!(terms, expected, "{label}");
                }
            }
        }
    }
}

#[test]
fn combined_metric_weights_move_the_best_team_between_rewards() {
    let (data, roster) = inputs();
    let (_, domain) = domain(&data, &roster, "free", false, false);
    let toward_points = domain.ranked(&[(POINTS, 1_000_000), (CHALLENGE, 1)])[0].0;
    let toward_challenge = domain.ranked(&[(POINTS, 1), (CHALLENGE, 1_000_000)])[0].0;
    assert_eq!(domain.value(&toward_points, POINTS), domain.best(POINTS));
    assert_eq!(domain.value(&toward_challenge, CHALLENGE), domain.best(CHALLENGE));
    // No team earns the most of both rewards: the weights decide.
    assert!(domain.value(&toward_points, CHALLENGE) < domain.best(CHALLENGE));
    assert!(domain.value(&toward_challenge, POINTS) < domain.best(POINTS));
}

#[test]
fn combined_metric_inputs_are_validated() {
    let (data, roster) = inputs();
    for (metric, message) in [
        (json!({"kind":"combined","levels":[]}), "exactly one level"),
        (json!({"kind":"combined","levels":[{"terms":[]}]}), "terms"),
        (combined(&[(points(), 0)]), "weight"),
        (combined(&[(points(), 1_000_001)]), "weight"),
        (combined(&[(json!({"kind":"power"}), 1)]), ""),
        (combined(&[(combined(&[(points(), 1)]), 1)]), ""),
        (combined(&[(json!({"kind":"scoreAtLeast","threshold":0}), 1)]), "target"),
        (
            json!({"kind":"combined","levels":[{"terms":[{"metric":points(),"weight":1}]},
                {"terms":[{"metric":challenge_points(),"weight":1}]}]}),
            "exactly one level",
        ),
        (combined(&(0..9).map(|_| (points(), 1)).collect::<Vec<_>>()), "terms"),
    ] {
        let request = request("free", false, false, metric.clone());
        let error = engine::recommend(&data, &roster, &request).err().unwrap_or_else(|| panic!("{metric}"));
        assert!(error.to_string().contains(message), "{metric}: {error}");
    }
}

#[test]
fn active_luck_ranges_reject_combined_metrics() {
    let mut synth = synthetic_master(5, 0, 5);
    set_column(&mut synth, "MasterLiveMusic", &mut |row| row["_gekisouMission1"] = json!(2));
    let mut document = data_document(&synth, 5, 0, 5);
    document["charts"][0]["fevers"] = json!({"startMs":[150],"endMs":[400]});
    let data = DeckData::from_json(&document.to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(5, 0, 5).to_string()).unwrap();
    let metric = combined(&[(points(), 1), (challenge_points(), 1)]);
    let request = super::joint_request("mission", true, metric);
    let error = engine::recommend(&data, &roster, &request).unwrap_err();
    assert!(matches!(error, ournotes_sim::Error::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("lottery-free terminal outcomes"), "{error}");
}

fn account_combined(terms: Value) -> Value {
    let mut request = account_request(json!({"kind":"freeLive","musicId":10,"difficulty":"expert"}));
    request["metric"] = json!({"kind":"combined","consumption":1,"terms":terms});
    request["eventContext"] = json!({"rewardProjection":true,
        "resultClock":{"kind":"played","serverNowJstTicks":150},
        "eventWindows":[{"eventId":EVENT_ID,"startJstTicks":100,"endJstTicks":200}]});
    request
}

#[test]
fn account_combined_metric_contract_echoes_terms_and_rejects_incomplete_fields() {
    let (data, account) = account_recommendation_fixture();
    assert_eq!(
        capabilities()["combinedMetric"],
        json!({
            "kind":"combined","field":"metric.terms","objective":"weightedExpectedSum","termMetrics":"metrics",
            "weights":"integer","maxTerms":8,"maxWeight":1_000_000,"consumption":"metric.consumption",
            "lotteryFree":true
        })
    );
    let terms = json!([
        {"kind":"eventPoints","eventId":EVENT_ID,"weight":2},
        {"kind":"challengePoints","eventId":EVENT_ID,"weight":35},
        {"kind":"eventItems","eventId":EVENT_ID,"resourceType":11,"resourceId":9,"weight":1},
        {"kind":"cappedScore","threshold":1000,"weight":3}
    ]);
    let weights = [2i128, 35, 1, 3];
    let request = account_combined(terms.clone());
    let answer = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
    assert!(matches!(answer.status, Status::Ok), "{:?}", answer.errors);
    let value = serde_json::to_value(answer).unwrap();
    assert_eq!(value["result"]["metric"]["kind"], "combined");
    assert_eq!(value["result"]["metric"]["consumption"], 1);
    assert_eq!(value["result"]["metric"]["terms"], terms);
    assert_eq!(value["result"]["optimality"]["proven"], true);
    let teams = value["result"]["teams"].as_array().unwrap();
    assert!(!teams.is_empty() && teams.len() <= 5);
    for team in teams {
        let exact = |value: &Value| -> (i128, i128) {
            (
                value["exact"]["numerator"].as_str().unwrap().parse().unwrap(),
                value["exact"]["denominator"].as_str().unwrap().parse().unwrap(),
            )
        };
        let (total, denominator) = exact(&team["value"]["payoff"]);
        let parts = team["terms"].as_array().unwrap();
        assert_eq!(parts.len(), weights.len());
        let sum: i128 = parts
            .iter()
            .zip(weights)
            .map(|(part, weight)| {
                let (numerator, same) = exact(part);
                assert_eq!(same, denominator);
                assert!(part["score"].is_number() && part["interval"].is_null());
                weight * numerator
            })
            .sum();
        assert_eq!(sum, total);
    }

    // A score-only combination takes neither consumption nor an event context.
    let mut request = account_request(json!({"kind":"freeLive","musicId":10,"difficulty":"expert"}));
    request["metric"] = json!({"kind":"combined","terms":[
        {"kind":"score","weight":1},{"kind":"scoreAtLeast","threshold":1000,"weight":1000}]});
    let answer = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
    assert!(matches!(answer.status, Status::Ok), "{:?}", answer.errors);

    let term = |term: Value| account_combined(json!([term]));
    let points = json!({"kind":"eventPoints","eventId":EVENT_ID,"weight":1});
    let mut invalid = Vec::new();
    let mut request = account_combined(json!([]));
    invalid.push((request, "metric.terms"));
    request = account_combined(Value::Array(vec![points.clone(); 9]));
    invalid.push((request, "metric.terms"));
    request = account_combined(json!([points]));
    request["metric"].as_object_mut().unwrap().remove("terms");
    invalid.push((request, "metric.terms"));
    request = account_combined(json!([points]));
    request["metric"].as_object_mut().unwrap().remove("consumption");
    invalid.push((request, "metric.consumption"));
    request = account_combined(json!([{"kind":"score","weight":1}]));
    invalid.push((request, "metric.consumption"));
    request = account_combined(json!([points]));
    request["metric"]["eventId"] = json!(EVENT_ID);
    invalid.push((request, "metric.eventId"));
    request = account_combined(json!([points]));
    request["metric"]["secondaryPriority"] = json!("eventPointsFirst");
    invalid.push((request, "metric.secondaryPriority"));
    invalid.push((term(json!({"kind":"eventPoints","eventId":EVENT_ID})), "metric.terms[0].weight"));
    invalid.push((term(json!({"kind":"eventPoints","eventId":EVENT_ID,"weight":0})), "metric.terms[0].weight"));
    invalid.push((term(json!({"kind":"eventPoints","eventId":EVENT_ID,"weight":1_000_001})), "metric.terms[0].weight"));
    invalid.push((term(json!({"kind":"eventPoints","weight":1})), "metric.terms[0].eventId"));
    invalid.push((
        term(json!({"kind":"eventPoints","eventId":EVENT_ID,"threshold":5,"weight":1})),
        "metric.terms[0].threshold",
    ));
    invalid.push((term(json!({"kind":"combined","weight":1})), "metric.terms[0].kind"));
    invalid.push((term(json!({"kind":"power","weight":1})), "metric.terms[0].kind"));
    request = account_combined(json!([points, {"kind":"eventItems","eventId":EVENT_ID,"resourceType":11,"weight":1}]));
    invalid.push((request, "metric.terms[1].resourceId"));
    request = account_request(json!({"kind":"freeLive","musicId":10,"difficulty":"expert"}));
    request["metric"] = json!({"kind":"score","terms":[{"kind":"score","weight":1}]});
    invalid.push((request, "metric.terms"));
    for (request, path) in invalid {
        let answer = engine::recommend_account(&data, &account.to_string(), &request.to_string(), None);
        assert!(matches!(answer.status, Status::Invalid), "{path}: {request}");
        assert!(answer.errors.iter().any(|issue| issue.path == path), "{path}: {:?}", answer.errors);
    }
}
