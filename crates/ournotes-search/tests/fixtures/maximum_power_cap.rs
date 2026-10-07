//! A globally power-ranked prefix proves capped utilities only through attained terminal values.
use super::common::{replace_table, set_column};
use super::{SCORE_ID, data_document, joint_request, roster_document, synthetic_master};
use ournotes_search::{
    engine,
    search::Completion,
    types::{Aggregation, Execution, ExitReason, Metric, Optimality, PlayPolicy, RecommendationRequest, Strategy},
};
use ournotes_sim::{cards::Roster, data::DeckData, live::model::JudgementStream};
use serde_json::{Value, json};

fn fixture(members: i64, snaps: i64, tied: bool) -> (DeckData, Roster) {
    let mut synth = synthetic_master(members, snaps, members);
    replace_table(&mut synth, "MasterEventEffect", json!([]));
    if tied {
        set_column(&mut synth, "MasterMemberCard", &mut |row| {
            for field in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                row[field] = json!(10000);
            }
            row["_memberCardLevelGroup"] = json!(1);
            row["_cardType"] = json!(1);
            row["_bestMusicTagIDs"] = json!([1]);
        });
        set_column(&mut synth, "MasterLeaderSkillEffect", &mut |row| row["_effectValue"] = json!(0));
        set_column(&mut synth, "MasterSupportCard", &mut |row| {
            for field in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
                row[field] = json!(0);
            }
        });
        // Type links multiply the member's power independently of the Snap's own power percentage.
        set_column(&mut synth, "MasterParameter", &mut |row| {
            if row["_id"] == "type_link_base_bonus_rate" {
                row["_value"] = json!("0");
            }
        });
        set_column(&mut synth, "MasterSupportCardRank", &mut |row| {
            row["_cardTypeLinkBonusRate"] = json!(0);
        });
    }
    (
        DeckData::from_json(&data_document(&synth, members, snaps, members).to_string()).unwrap(),
        Roster::from_json(&roster_document(members, snaps, members).to_string()).unwrap(),
    )
}

fn request(data: &DeckData, metric: Value) -> RecommendationRequest {
    let mut request = joint_request("free", false, metric);
    request.aggregation = Aggregation::Maximum;
    request.execution = Execution::Live {
        score_id: SCORE_ID,
        gekisou: false,
        play: PlayPolicy::Stream { stream: JudgementStream::theoretical_best(&data.chart(SCORE_ID).unwrap()) },
    };
    request
}

fn count_phase(result: &ournotes_search::types::RecommendationOutcome, label: &str) -> usize {
    result
        .telemetry
        .phases
        .iter()
        .filter(|phase| phase.name == "search" && phase.label.as_deref() == Some(label))
        .count()
}

#[test]
fn maximum_power_prefix_proves_attained_caps_across_full_constraints() {
    let (data, roster) = fixture(6, 1, false);
    let constraints = [
        json!({"leader":3}),
        json!({"leader":3,"includeMembers":[1],"excludeMembers":[6],"excludeSnaps":[1]}),
        json!({"includeMembers":[6],"excludeMembers":[1]}),
        json!({"leader":6,"noSnaps":true}),
    ];
    for metric in [
        json!({"kind":"scoreAtLeast","threshold":1}),
        json!({"kind":"cappedScore","threshold":1}),
        json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":1000}),
    ] {
        for constraints in &constraints {
            let mut current = request(&data, metric.clone());
            current.constraints = serde_json::from_value(constraints.clone()).unwrap();
            current.strategy = Strategy::Exhaustive;
            current.k = 100;
            let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
            assert_eq!(exhaustive.completion, Completion::Complete);
            assert!(!exhaustive.results.is_empty());
            assert_eq!(count_phase(&exhaustive, "maximumPowerCap"), 0);
            assert!(exhaustive.results.iter().all(|team| team.objective_value.as_ref().unwrap().numerator == "1"));
            for (k, cache) in [(1, 0), (3, 32), (5, 0)] {
                current.strategy = Strategy::BranchAndBound;
                current.k = k;
                current.limits.cache_entries = cache;
                let actual = engine::recommend(&data, &roster, &current).unwrap();
                assert_eq!(actual.completion, Completion::Complete);
                assert_eq!(actual.optimality, Optimality::Proven);
                assert_eq!(actual.results, exhaustive.results[..k.min(exhaustive.results.len())]);
                assert_eq!(actual.telemetry.leaves.evaluated, k.min(exhaustive.results.len()) as u64);
                assert_eq!(count_phase(&actual, "maximumPowerCap"), 1);
                assert_eq!(actual.telemetry.environment.target.as_ref().unwrap().denominator, "1");
            }
        }
    }
}

#[test]
fn maximum_power_prefix_keeps_member_leader_and_empty_snap_ties() {
    let (data, roster) = fixture(5, 1, true);
    let mut current = request(&data, json!({"kind":"scoreAtLeast","threshold":1}));
    current.constraints.leader = None;
    current.k = 100;
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(exhaustive.results.len(), 30);
    assert!(
        exhaustive.results.windows(2).all(|pair| pair[0].power == pair[1].power
            && (pair[0].members, pair[0].snaps) < (pair[1].members, pair[1].snaps))
    );
    assert_eq!(exhaustive.results[0].snaps, [None; 5]);
    for k in [1, 5, 12] {
        current.k = k;
        current.strategy = Strategy::BranchAndBound;
        let actual = engine::recommend(&data, &roster, &current).unwrap();
        assert_eq!(actual.completion, Completion::Complete);
        assert_eq!(actual.results, exhaustive.results[..k]);
        assert_eq!(actual.telemetry.leaves.evaluated, k as u64);
    }
}

#[test]
fn maximum_power_prefix_yields_to_lower_power_higher_payoff() {
    let mut synth = synthetic_master(6, 0, 6);
    set_column(&mut synth, "MasterMemberCard", &mut |row| {
        let strong_skill = row["_id"] == 6;
        for field in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
            row[field] = json!(if strong_skill { 1000 } else { 10000 });
        }
        row["_liveSkillID"] = json!(if strong_skill { 2 } else { 1 });
    });
    set_column(&mut synth, "MasterLeaderSkillEffect", &mut |row| row["_effectValue"] = json!(0));
    set_column(&mut synth, "MasterLiveSkillEffect", &mut |row| {
        row["_skillConditionGroup"] = json!(0);
        row["_skillTargetIDs"] = json!([]);
        row["_skillEffectType"] = json!(2000);
        row["_activationTimeSecond"] = json!(5.0);
        row["_effectValue"] = json!(if row["_liveSkillID"] == 2 { 100000 } else { 0 });
    });
    replace_table(&mut synth, "MasterEventEffect", json!([]));
    let data = DeckData::from_json(&data_document(&synth, 6, 0, 6).to_string()).unwrap();
    let roster = Roster::from_json(&roster_document(6, 0, 6).to_string()).unwrap();
    let mut current = request(&data, json!({"kind":"score"}));
    current.k = 100;
    current.strategy = Strategy::Exhaustive;
    let scores = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(scores.completion, Completion::Complete);
    assert_eq!(scores.results.len(), 5);
    let strongest = scores.results.iter().max_by_key(|team| team.power).unwrap();
    assert!(!strongest.members.contains(&6));
    let low = strongest.maximum_score.unwrap();
    let high = scores.results[0].maximum_score.unwrap();
    assert!(high > low && scores.results[0].power < strongest.power);
    let threshold = ((i64::from(low) + i64::from(high)) / 2) as i32;
    current.metric = Metric::ScoreAtLeast { threshold };
    current.k = 3;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(exhaustive.results[0].objective_value.as_ref().unwrap().numerator, "1");
    current.strategy = Strategy::BranchAndBound;
    current.limits.cache_entries = 0;
    let actual = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(actual.completion, Completion::Complete);
    assert_eq!(actual.results, exhaustive.results);
    assert_eq!(count_phase(&actual, "maximumPowerCap"), 1);
    assert_eq!(actual.telemetry.joint.modules["maximumPowerCap"].pruned, 0);
    assert!(actual.results.iter().all(|team| team.members.contains(&6)));
}

#[test]
fn maximum_power_prefix_exhausts_small_domains_even_below_the_cap() {
    let (data, roster) = fixture(5, 0, false);
    let mut current = request(&data, json!({"kind":"scoreAtLeast","threshold":2147483647}));
    current.k = 5;
    current.strategy = Strategy::Exhaustive;
    let exhaustive = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(exhaustive.completion, Completion::Complete);
    assert_eq!(exhaustive.results.len(), 1);
    assert_eq!(exhaustive.results[0].objective_value.as_ref().unwrap().numerator, "0");
    current.strategy = Strategy::BranchAndBound;
    let actual = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(actual.completion, Completion::Complete);
    assert_eq!(actual.results, exhaustive.results);
    assert_eq!(actual.telemetry.leaves.evaluated, 1);
    assert_eq!(actual.telemetry.joint.modules["maximumPowerCap"].pruned, 1);
}

#[test]
fn maximum_power_prefix_stops_without_promoting_an_upper_bound() {
    let (data, roster) = fixture(6, 1, false);
    let mut current = request(&data, json!({"kind":"scoreAndLifeAtLeast","threshold":1,"minFinalLife":1000}));
    current.k = 5;
    for limit in [0, 2] {
        current.limits.max_candidates = Some(limit);
        let result = engine::recommend(&data, &roster, &current).unwrap();
        assert_eq!(result.completion, Completion::TimedOut);
        assert_eq!(result.exit_reason, ExitReason::CandidateLimit);
        assert_eq!(result.optimality, Optimality::Unproven);
        assert!(!result.telemetry.proof.complete);
        assert_eq!(result.results.len(), limit as usize);
        assert_eq!(result.telemetry.proof.global_upper_bound.as_deref(), Some("1"));
        assert_eq!(result.telemetry.environment.target.as_ref().unwrap().denominator, "1");
        assert_eq!(result.telemetry.joint.modules["maximumPowerCap"].pruned, 0);
    }
    current.limits.max_candidates = None;
    current.limits.time_limit_ms = Some(0);
    let stopped = engine::recommend(&data, &roster, &current).unwrap();
    assert_eq!(stopped.completion, Completion::TimedOut);
    assert_eq!(stopped.exit_reason, ExitReason::TimeLimit);
    assert_eq!(stopped.optimality, Optimality::Unproven);
    assert!(stopped.results.is_empty());
}
