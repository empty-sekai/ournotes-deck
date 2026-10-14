use super::super::gate_tests::common::{Rng, Synth, replace_table, roster, set_column, short_chart, synth};
use super::*;
use ournotes_sim::scenario::{ContextInput, Scenario};
use serde_json::{Value, json};

fn fixture() -> Synth {
    let mut data = synth(&mut Rng::new(8171), 5, 0);
    set_column(&mut data, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    set_column(&mut data, "MasterLiveMusic", &mut |row| row["_liveScoreRankGroup"] = json!(1));
    replace_table(&mut data, "MasterEvent", json!([{"_id":7,"_liveEventRewardGroup":37}]));
    replace_table(
        &mut data,
        "MasterLiveScoreRank",
        Value::Array(
            [0, 10, 20]
                .into_iter()
                .enumerate()
                .map(|(i, score)| json!({"_id":i+1,"_group":1,"_liveScoreRank":i+2,"_requiredScore":score}))
                .collect(),
        ),
    );
    replace_table(
        &mut data,
        "MasterLiveEventReward",
        Value::Array(
            [7, 29, 3]
                .into_iter()
                .enumerate()
                .map(|(i, count)| {
                    json!({"_id":i+1,"_eventGroup":37,"_group":5,"_scoreRank":i+2,"_probability":10000,
            "_resourceType":4,"_resourceId":if i == 2 {89} else {88},"_resourceCount":count})
                })
                .collect(),
        ),
    );
    replace_table(
        &mut data,
        "MasterLiveMusicBoostBonus",
        json!([
            {"_id":1,"_consumedLiveBoostCount":1,"_liveMusicRewardRate":3,"_eventPointRate":17}
        ]),
    );
    replace_table(
        &mut data,
        "MasterEventEffect",
        json!([
            {"_id":1,"_eventId":7,"_eventBonusType":1,"_resourceTypeConstraint":2,"_memberCardId":1,
             "_rank1EffectValue":2500,"_rank2EffectValue":2500,"_rank3EffectValue":2500,
             "_rank4EffectValue":2500,"_rank5EffectValue":2500}
        ]),
    );
    data
}

fn compile(data: &Synth, support: (i32, i32), active: bool) -> Result<PointBound, Error> {
    let master = data.master();
    let owned = roster(&mut Rng::new(8172), &master);
    let input: ContextInput = serde_json::from_value(json!({
        "powerSnapshot":{"eventIds":[7]},
        "resultClock":{"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100},
        "eventPayoff":{"consumedCount":1,"eventWindows":if active {
            json!([{"eventId":7,"startJstTicks":90,"endJstTicks":110}])
        } else {json!([])}}
    }))
    .unwrap();
    let context = input.resolve(&master, Scenario::Free(10), Some(1004), &[])?;
    let pool = context.pool(&master, &owned)?;
    let (chart, judgement_types) = short_chart(&mut Rng::new(8173), 2, false);
    let request = SearchRequest {
        objective: Objective::LiveScore {
            score_id: 1004,
            play: super::super::PlayInput::Stream {
                stream: ournotes_sim::live::model::JudgementStream::theoretical_best(&chart),
                judgement_types,
            },
            chart,
            event: false,
            exclude_snap_skills: false,
            gekisou: None,
        }
        .in_scenario(context),
        k: 1,
        constraints: super::super::Constraints::default(),
        time_limit: None,
    };
    let domain = CandidateDomain::build(&pool, &request.constraints)?;
    PointBound::compile_items(&pool, &request, &domain, input.event_payoff.as_ref().unwrap(), (7, 4, 88), support)
}

#[test]
fn item_prefix_maxima_and_mean_hulls_cover_each_outcome_before_averaging() {
    let bound = compile(&fixture(), (0, 30), true).unwrap();
    assert_eq!(bound.multiplier_at(9), 21);
    assert_eq!(bound.multiplier_at(10), 87);
    assert_eq!(bound.multiplier_at(30), 87);
    assert_eq!(bound.order_payoff(2500, 9), 26);
    let exact = |score| match score {
        0..=9 => 26,
        10..=19 => 108,
        _ => 0,
    };
    for a in 0..=30 {
        assert!(bound.order_payoff(2500, a) >= exact(a));
        for b in 0..=30 {
            let mean_ceiling = (a + b + 1) / 2;
            assert!(2 * bound.mean_payoff(2500, mean_ceiling, a.max(b)) >= exact(a) + exact(b));
        }
    }
}

#[test]
fn every_rank_checks_native_products_before_resource_filtering_or_zero_rate() {
    for count in [-1, i64::from(i32::MAX), 1i64 << 32] {
        for rate in [0, 3] {
            let mut data = fixture();
            set_column(&mut data, "MasterLiveEventReward", &mut |row| {
                if row["_scoreRank"] == 3 {
                    row["_resourceCount"] = json!(count);
                    row["_resourceId"] = json!(89);
                }
            });
            set_column(&mut data, "MasterLiveMusicBoostBonus", &mut |row| row["_liveMusicRewardRate"] = json!(rate));
            assert!(compile(&data, (0, 30), true).is_err(), "count={count}, rate={rate}");
            compile(&data, (0, 9), true).unwrap();
        }
    }
    for rate in [-1, i64::from(i32::MAX), 1i64 << 32] {
        let mut data = fixture();
        set_column(&mut data, "MasterLiveMusicBoostBonus", &mut |row| row["_liveMusicRewardRate"] = json!(rate));
        assert!(compile(&data, (0, 30), true).is_err());
    }
    let mut data = fixture();
    set_column(&mut data, "MasterEventEffect", &mut |row| {
        for rank in 1..=5 {
            row[format!("_rank{rank}EffectValue")] = json!(1i64 << 32);
        }
    });
    assert!(compile(&data, (0, 30), true).is_err());
}

#[test]
fn unsupported_interior_item_grades_are_not_hidden_by_equal_end_rewards() {
    for mutation in ["missing", "probability", "duplicate"] {
        let mut data = fixture();
        let (_, rows) = data.tables.iter_mut().find(|(name, _)| name == "MasterLiveEventReward").unwrap();
        let rows = rows.as_array_mut().unwrap();
        match mutation {
            "missing" => {
                rows.remove(1);
            }
            "probability" => rows[1]["_probability"] = json!(9999),
            _ => {
                let mut row = rows[1].clone();
                row["_id"] = json!(99);
                row["_group"] = json!(6);
                row["_resourceId"] = json!(89);
                rows.push(row);
            }
        }
        assert!(compile(&data, (0, 30), true).is_err(), "{mutation}");
        compile(&data, (0, 9), true).unwrap();
    }
    let mut data = fixture();
    replace_table(&mut data, "MasterLiveEventReward", json!([]));
    assert_eq!(compile(&data, (0, 30), false).unwrap().order_payoff(0, 30), 0);
    assert!(compile(&data, (-1, 30), false).is_err(), "inactive events still resolve the result rank");
}
