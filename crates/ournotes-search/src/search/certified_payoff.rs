//! Exact native terminal-payoff steps on a caller-proved, closed score support.
//!
//! Rank-comparison boundaries establish constancy before native endpoint checks. Matching endpoint rewards alone
//! are never evidence that an interval is constant: interior ranks can pay differently or fail native settlement.
use super::certified_search::{PayoffMap, PayoffStep};
use super::expectation::PhysicalDeck;
use super::{Pool, SearchRequest};
use crate::types::Metric;
use ournotes_sim::Error;
use ournotes_sim::event;
use ournotes_sim::scenario::{EventPayoffInput, MultiplayerScorePolicy, ResolvedContext, Scenario, item_payoff};

fn invalid(message: &str) -> Error {
    Error::Domain(format!("certified payoff: {message}"))
}

/// Compile only the supplied possible score interval. A native error anywhere inside it is an error, not zero.
/// Primitive score/life maps retain their dedicated certified aggregation semantics.
pub(crate) fn payoff_map(
    pool: &Pool,
    request: &SearchRequest,
    metric: &Metric,
    event_input: Option<&EventPayoffInput>,
    physical: &PhysicalDeck,
    power: i32,
    support: (i32, i32),
) -> Result<PayoffMap, Error> {
    if support.0 > support.1 {
        return Err(invalid("reversed score support"));
    }
    match *metric {
        Metric::Score => return Ok(PayoffMap::Score),
        Metric::BestOrderExpectedScore => return Ok(PayoffMap::BestOrderExpectedScore),
        Metric::ScoreAtLeast { threshold } => return Ok(PayoffMap::ScoreAtLeast { threshold }),
        Metric::CappedScore { threshold } => return Ok(PayoffMap::CappedScore { threshold }),
        Metric::ScoreAndLifeAtLeast { threshold, min_final_life } => {
            return Ok(PayoffMap::ScoreAndLifeAtLeast { threshold, min_final_life });
        }
        Metric::Power => {
            return Ok(PayoffMap::NativeSteps(vec![PayoffStep {
                lower: support.0,
                upper: support.1,
                value: i128::from(power),
            }]));
        }
        _ => {}
    }
    let context =
        request.objective.context().ok_or_else(|| Error::Input("event payoff requires resolved context".into()))?;
    let input = event_input.ok_or_else(|| Error::Input("event payoff requires event input".into()))?;
    let event_id = metric.event().ok_or_else(|| invalid("unknown event metric"))?;
    // Validate route/event identity, clocks, and mutually exclusive room adapters before building any cuts.
    context.validate_pool(pool)?;
    context.event_request(pool.master, input, event_id)?;
    let deck = physical.as_deck();
    pool.check_deck(&deck)?;
    let cuts = rank_cuts(pool, context, input, support)?;
    let value_at = |score| match *metric {
        Metric::ClientEventPoints { event_id } => {
            Ok(i128::from(context.preview_event_points(pool, &deck, input, event_id, score)?.points_for(event_id)))
        }
        Metric::ClientChallengePoints { event_id } => Ok(i128::from(
            context.preview_event_points(pool, &deck, input, event_id, score)?.challenge_points_for(event_id),
        )),
        Metric::ConditionalClientEventItems { event_id, resource_type, resource_id } => {
            let preview = context.preview_event_items(pool, &deck, input, event_id, score)?;
            item_payoff(&preview, event_id, resource_type, resource_id)
        }
        _ => Err(invalid("non-event metric reached native settlement")),
    };
    native_steps(&cuts, value_at)
}

fn push_cut(cuts: &mut Vec<i64>, boundary: i128, support: (i32, i32)) {
    if i128::from(support.0) < boundary && boundary <= i128::from(support.1) {
        cuts.push(boundary as i64);
    }
}

/// Every native total is affine with a positive slope. Checking both support endpoints through the original
/// checked-add loop proves each intermediate sum valid throughout the interval, including panel prefix sums.
struct Room {
    slope: i64,
    offset: i64,
    players: i64,
}

impl Room {
    fn checked(
        support: (i32, i32),
        slope: i64,
        mut native: impl FnMut(i32) -> Result<(i32, i64), Error>,
    ) -> Result<Self, Error> {
        let (lower, players) = native(support.0)?;
        let (upper, other_players) = native(support.1)?;
        if slope <= 0 || players != other_players {
            return Err(invalid("room score adapter is not a fixed positive affine map"));
        }
        let offset = i64::from(lower) - slope * i64::from(support.0);
        if slope * i64::from(support.1) + offset != i64::from(upper) {
            return Err(invalid("native room score differs from its proved affine map"));
        }
        Ok(Self { slope, offset, players })
    }

    fn threshold_preimage(&self, total_threshold: i32) -> i128 {
        let numerator = i128::from(total_threshold) - i128::from(self.offset);
        let divisor = i128::from(self.slope);
        numerator.div_euclid(divisor) + i128::from(numerator.rem_euclid(divisor) != 0)
    }
}

fn rank_cuts(
    pool: &Pool,
    context: &ResolvedContext,
    input: &EventPayoffInput,
    support: (i32, i32),
) -> Result<Vec<i64>, Error> {
    // An i64 exclusive end also represents the integer just after i32::MAX.
    let mut cuts = vec![i64::from(support.0), i64::from(support.1) + 1];
    let multiplayer = matches!(context.scenario, Scenario::Battle(_) | Scenario::Arena(_));
    let room = if multiplayer {
        if let Some(policy) = &input.multiplayer_score_policy {
            let slope = match *policy {
                MultiplayerScorePolicy::SameScore { players } => players,
                MultiplayerScorePolicy::FixedOthersAverage { .. } => 1,
            };
            Some(Room::checked(support, slope, |score| policy.total_and_count(score))?)
        } else if let Some(panel) = &input.multiplayer_result_panel {
            Some(Room::checked(support, 1, |score| panel.total_and_count(score))?)
        } else if let Some(ranks) = &input.multiplayer_ranks {
            let mut scores: Vec<_> = ranks
                .iter()
                .map(|row| row.local_final_score)
                .filter(|&score| support.0 <= score && score <= support.1)
                .collect();
            scores.sort_unstable();
            let needed = i64::from(support.1) - i64::from(support.0) + 1;
            if scores.len() as u64 != needed as u64
                || scores.first() != Some(&support.0)
                || scores.last() != Some(&support.1)
                || scores.windows(2).any(|pair| i64::from(pair[1]) != i64::from(pair[0]) + 1)
            {
                return Err(Error::Input(
                    "explicit multiplayerRanks do not cover every score in certified support".into(),
                ));
            }
            // No interpolation is justified for externally supplied ranks; every admitted score is a singleton.
            cuts.extend(scores.into_iter().map(i64::from));
            cuts.sort_unstable();
            cuts.dedup();
            return Ok(cuts);
        } else {
            return Err(Error::Input("multiplayer payoff requires an explicit rank or room-score adapter".into()));
        }
    } else {
        None
    };
    let group = pool
        .master
        .live_music(context.resolved.live_music_id)
        .ok_or_else(|| Error::Master("resolved event-payoff music is missing".into()))?
        .live_score_rank_group;
    for row in event::rank_rows_of_group(pool.master, group) {
        let boundary = match &room {
            Some(room) => {
                room.threshold_preimage(event::battle_required_score(row.battle_live_required_score, room.players))
            }
            None => i128::from(row.required_score),
        };
        push_cut(&mut cuts, boundary, support);
    }
    // Retain every comparison boundary even if rank rows or their rewards are nonmonotone. Native row ordering,
    // early exit, E-to-D mapping, and NONE are constant inside each resulting cell. Negative scores are included.
    // Skip may now select one fixed master rank; these extra boundaries are harmless and equal values merge later.
    cuts.sort_unstable();
    cuts.dedup();
    Ok(cuts)
}

fn native_steps(cuts: &[i64], mut value_at: impl FnMut(i32) -> Result<i128, Error>) -> Result<PayoffMap, Error> {
    let mut steps: Vec<PayoffStep> = Vec::new();
    for cell in cuts.windows(2) {
        let lower = i32::try_from(cell[0]).map_err(|_| invalid("partition lower endpoint is outside i32"))?;
        let upper = i32::try_from(cell[1] - 1).map_err(|_| invalid("partition upper endpoint is outside i32"))?;
        if lower > upper {
            return Err(invalid("empty or reversed native partition"));
        }
        let value = value_at(lower)?;
        if value_at(upper)? != value {
            return Err(invalid("native payoff differs inside a proved rank-constant partition"));
        }
        if let Some(previous) = steps.last_mut() {
            if i64::from(previous.upper) + 1 != i64::from(lower) {
                return Err(invalid("gap or overlap in native payoff support"));
            }
            if previous.value == value {
                previous.upper = upper;
                continue;
            }
        }
        steps.push(PayoffStep { lower, upper, value });
    }
    if steps.is_empty() {
        return Err(invalid("empty native payoff support"));
    }
    Ok(PayoffMap::NativeSteps(steps))
}

#[cfg(test)]
use super::gate_tests::common;

#[cfg(test)]
mod tests {
    use super::common::{Rng, Synth, replace_table, roster, set_column, short_chart, synth};
    use super::*;
    use crate::search::{Constraints, Objective, PlayInput};
    use ournotes_sim::live::model::JudgementStream;
    use ournotes_sim::scenario::ContextInput;
    use serde_json::{Value, json};

    fn fixture() -> Synth {
        let mut data = synth(&mut Rng::new(7301), 5, 0);
        set_column(&mut data, "MasterMemberCard", &mut |row| {
            row["_characterID"] = row["_id"].clone();
            row["_leaderSkillID"] = json!(4);
        });
        set_column(&mut data, "MasterLiveMusic", &mut |row| row["_liveScoreRankGroup"] = json!(1));
        replace_table(
            &mut data,
            "MasterChallengeMusic",
            json!([
                {"_id":70,"_eventId":7,"_liveMusicId":10,"_musicType":1}
            ]),
        );
        replace_table(&mut data, "MasterArenaMusic", json!([{"_id":80,"_liveMusicId":10,"_liveMusicType":1}]));
        replace_table(
            &mut data,
            "MasterEvent",
            json!([
                {"_id":7,"_liveEventPointGroup":1,"_challengeLiveEventPointGroup":2}
            ]),
        );
        replace_table(
            &mut data,
            "MasterLiveScoreRank",
            json!([
                {"_id":1,"_group":1,"_liveScoreRank":0,"_requiredScore":-10,"_battleLiveRequiredScore":0},
                {"_id":2,"_group":1,"_liveScoreRank":2,"_requiredScore":0,"_battleLiveRequiredScore":0},
                {"_id":3,"_group":1,"_liveScoreRank":3,"_requiredScore":10,"_battleLiveRequiredScore":10},
                {"_id":4,"_group":1,"_liveScoreRank":4,"_requiredScore":20,"_battleLiveRequiredScore":20}
            ]),
        );
        for (table, group, values) in [
            ("MasterLiveEventPoint", 1, [70, 4, 1, 12]),
            ("MasterChallengeLiveEventPoint", 2, [300, 20, 3, 60]),
            ("MasterLiveChallengePoint", 0, [30, 2, 9, 1]),
        ] {
            replace_table(
                &mut data,
                table,
                Value::Array(
                    [0, 2, 3, 4]
                        .into_iter()
                        .zip(values)
                        .enumerate()
                        .map(|(i, (rank, value))| json!({"_id":i+1,"_group":group,"_scoreRank":rank,"_value":value}))
                        .collect(),
                ),
            );
        }
        for table in ["MasterLiveEventReward", "MasterChallengeLiveEventReward"] {
            replace_table(&mut data, table, json!([{"_id":1,"_resourceType":4,"_resourceId":88,"_resourceCount":7}]));
        }
        if let Some((_, rows)) = data.tables.iter_mut().find(|(name, _)| name == "MasterParameter") {
            let rows = rows.as_array_mut().unwrap();
            rows.retain(|row| row["_id"] != "live_skip_result_score_rank");
            rows.push(json!({"_id":"live_skip_result_score_rank","_type":"String","_value":"C"}));
            assert_eq!(rows.iter().filter(|row| row["_id"] == "live_skip_result_score_rank").count(), 1);
        }
        data
    }

    fn input(adapter: Value) -> ContextInput {
        let mut value = json!({
            "powerSnapshot":{"eventIds":[7],"capturedJstTicks":50},
            "resultClock":{"execution":"played","savedStartJstTicks":99,"serverNowJstTicks":100},
            "eventPayoff":{"consumedCount":0,"localEvents":[{"eventId":7,"points":0,"challengePoints":10,"added":[]}],
                "eventWindows":[{"eventId":7,"startJstTicks":90,"endJstTicks":110}]}
        });
        for (name, content) in adapter.as_object().unwrap() {
            value["eventPayoff"][name] = content.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    fn invoke(
        data: &Synth,
        scenario: Scenario,
        input: &ContextInput,
        metric: &Metric,
        support: (i32, i32),
        enumerate: bool,
    ) -> Result<PayoffMap, Error> {
        let master = data.master();
        let owned = roster(&mut Rng::new(7302), &master);
        let context = input.resolve(&master, scenario, Some(1004), &[])?;
        let pool = context.pool(&master, &owned)?;
        let (chart, judgement_types) = short_chart(&mut Rng::new(7303), 2, false);
        let objective = if matches!(context.result_clock, Some(event::EventResultClock::Skip { .. })) {
            Objective::SkipScore { score_id: 1004, chart }
        } else {
            Objective::LiveScore {
                score_id: 1004,
                play: PlayInput::Stream { stream: JudgementStream::theoretical_best(&chart), judgement_types },
                chart,
                event: false,
                exclude_snap_skills: false,
                gekisou: None,
            }
        }
        .in_scenario(context.clone());
        let request = SearchRequest { objective, k: 5, constraints: Constraints::default(), time_limit: None };
        let physical = PhysicalDeck { members: [0, 1, 2, 3, 4], snaps: [None; 5] };
        let map = payoff_map(&pool, &request, metric, input.event_payoff.as_ref(), &physical, 123, support)?;
        if let PayoffMap::NativeSteps(steps) = &map {
            assert_eq!(steps.first().unwrap().lower, support.0);
            assert_eq!(steps.last().unwrap().upper, support.1);
            assert!(
                steps
                    .windows(2)
                    .all(|w| i64::from(w[0].upper) + 1 == i64::from(w[1].lower) && w[0].value != w[1].value)
            );
        }
        if enumerate {
            assert!(i64::from(support.1) - i64::from(support.0) <= 1000);
            let deck = physical.as_deck();
            for score in support.0..=support.1 {
                let event_input = input.event_payoff.as_ref().unwrap();
                let expected = match *metric {
                    Metric::ClientEventPoints { event_id } => i128::from(
                        context.preview_event_points(&pool, &deck, event_input, event_id, score)?.points_for(event_id),
                    ),
                    Metric::ClientChallengePoints { event_id } => i128::from(
                        context
                            .preview_event_points(&pool, &deck, event_input, event_id, score)?
                            .challenge_points_for(event_id),
                    ),
                    Metric::ConditionalClientEventItems { event_id, resource_type, resource_id } => item_payoff(
                        &context.preview_event_items(&pool, &deck, event_input, event_id, score)?,
                        event_id,
                        resource_type,
                        resource_id,
                    )?,
                    _ => unreachable!(),
                };
                assert_eq!(lookup(&map, score), expected, "{scenario:?}, score={score}");
            }
        }
        Ok(map)
    }

    fn lookup(map: &PayoffMap, score: i32) -> i128 {
        let PayoffMap::NativeSteps(steps) = map else { panic!("expected native steps") };
        steps.iter().find(|step| step.lower <= score && score <= step.upper).unwrap().value
    }

    #[test]
    fn primitive_maps_and_whole_i32_constant_support_are_preserved() {
        let data = fixture();
        let input = input(json!({}));
        for (metric, expected) in [
            (Metric::Score, PayoffMap::Score),
            (Metric::ScoreAtLeast { threshold: -1 }, PayoffMap::ScoreAtLeast { threshold: -1 }),
            (Metric::CappedScore { threshold: 7 }, PayoffMap::CappedScore { threshold: 7 }),
            (
                Metric::ScoreAndLifeAtLeast { threshold: 10, min_final_life: 4 },
                PayoffMap::ScoreAndLifeAtLeast { threshold: 10, min_final_life: 4 },
            ),
        ] {
            assert_eq!(
                invoke(&data, Scenario::Free(10), &input, &metric, (i32::MIN, i32::MAX), false).unwrap(),
                expected
            );
        }
        assert_eq!(
            invoke(&data, Scenario::Free(10), &input, &Metric::Power, (i32::MIN, i32::MAX), false).unwrap(),
            PayoffMap::NativeSteps(vec![PayoffStep { lower: i32::MIN, upper: i32::MAX, value: 123 }])
        );
        assert!(invoke(&data, Scenario::Free(10), &input, &Metric::Score, (1, 0), false).is_err());
    }

    #[test]
    fn solo_negative_none_and_nonmonotone_ep_cp_steps_match_every_native_score() {
        let data = fixture();
        for scenario in [Scenario::Free(10), Scenario::Challenge(70)] {
            for metric in [Metric::ClientEventPoints { event_id: 7 }, Metric::ClientChallengePoints { event_id: 7 }] {
                let map = invoke(&data, scenario, &input(json!({})), &metric, (-10, 30), true).unwrap();
                if matches!(metric, Metric::ClientEventPoints { .. }) {
                    assert!(lookup(&map, -1) > lookup(&map, 0));
                }
            }
        }
        assert!(
            invoke(
                &data,
                Scenario::Free(10),
                &input(json!({})),
                &Metric::ClientEventPoints { event_id: 7 },
                (-11, 30),
                false
            )
            .is_err()
        );
    }

    #[test]
    fn only_real_support_is_required_and_interior_native_errors_are_not_hidden_by_equal_endpoints() {
        let mut data = fixture();
        replace_table(
            &mut data,
            "MasterLiveEventPoint",
            json!([
                {"_id":1,"_group":1,"_scoreRank":2,"_value":5}, {"_id":2,"_group":1,"_scoreRank":4,"_value":5}
            ]),
        );
        let metric = Metric::ClientEventPoints { event_id: 7 };
        assert!(invoke(&data, Scenario::Free(10), &input(json!({})), &metric, (0, 30), false).is_err());
        assert_eq!(
            invoke(&data, Scenario::Free(10), &input(json!({})), &metric, (0, 9), true).unwrap(),
            PayoffMap::NativeSteps(vec![PayoffStep { lower: 0, upper: 9, value: 5 }])
        );
    }

    #[test]
    fn room_preimages_cover_negative_scores_ceil_boundaries_and_nonmonotone_rank_rows() {
        let mut data = fixture();
        for policy in
            [json!({"kind":"sameScore","players":2}), json!({"kind":"fixedOthersAverage","players":3,"score":7})]
        {
            let input = input(json!({"multiplayerScorePolicy":policy}));
            for scenario in [Scenario::Battle(10), Scenario::Arena(80)] {
                invoke(&data, scenario, &input, &Metric::ClientEventPoints { event_id: 7 }, (-30, 70), true).unwrap();
                invoke(&data, scenario, &input, &Metric::ClientChallengePoints { event_id: 7 }, (-30, 70), true)
                    .unwrap();
            }
        }
        set_column(&mut data, "MasterLiveScoreRank", &mut |row| {
            if row["_liveScoreRank"] == 3 {
                row["_battleLiveRequiredScore"] = json!(30);
            }
            if row["_liveScoreRank"] == 4 {
                row["_battleLiveRequiredScore"] = json!(10);
            }
        });
        invoke(
            &data,
            Scenario::Battle(10),
            &input(json!({"multiplayerScorePolicy":{"kind":"sameScore","players":1}})),
            &Metric::ClientEventPoints { event_id: 7 },
            (-5, 100),
            true,
        )
        .unwrap();
    }

    #[test]
    fn checked_room_domain_is_not_interpolated_across_overflow_and_i32_extremes_are_retained() {
        let data = fixture();
        let metric = Metric::ClientEventPoints { event_id: 7 };
        let two = input(json!({"multiplayerScorePolicy":{"kind":"sameScore","players":2}}));
        assert!(invoke(&data, Scenario::Battle(10), &two, &metric, (0, i32::MAX), false).is_err());
        let one = input(json!({"multiplayerScorePolicy":{"kind":"sameScore","players":1}}));
        let map = invoke(&data, Scenario::Battle(10), &one, &metric, (i32::MIN, i32::MAX), false).unwrap();
        assert_eq!(lookup(&map, i32::MIN), 70);
        assert_eq!(lookup(&map, i32::MAX), 12);
    }

    #[test]
    fn signed_affine_preimages_are_exact_first_crossings() {
        for slope in 1..=5 {
            for offset in [-17, 0, 23] {
                let room = Room { slope, offset, players: 1 };
                for threshold in -40..=40 {
                    let first = room.threshold_preimage(threshold);
                    let total = |score: i128| i128::from(slope) * score + i128::from(offset);
                    assert!(total(first - 1) < i128::from(threshold));
                    assert!(total(first) >= i128::from(threshold));
                }
            }
        }
    }

    #[test]
    fn panel_preserves_disconnections_local_position_and_checked_prefix_sums() {
        let data = fixture();
        let metric = Metric::ClientEventPoints { event_id: 7 };
        for local in 0..=2 {
            let panel = json!({"localPlayerIndex":local,"localDisconnected":false,
                "otherPlayers":[{"finalScore":10,"disconnected":true},{"finalScore":-3,"disconnected":false}]});
            invoke(
                &data,
                Scenario::Arena(80),
                &input(json!({"multiplayerResultPanel":panel})),
                &metric,
                (-30, 80),
                true,
            )
            .unwrap();
        }
        let overflow = json!({"localPlayerIndex":2,"localDisconnected":false,
            "otherPlayers":[{"finalScore":i32::MAX,"disconnected":false},{"finalScore":i32::MAX,"disconnected":false}]});
        assert!(
            invoke(
                &data,
                Scenario::Battle(10),
                &input(json!({"multiplayerResultPanel":overflow})),
                &metric,
                (i32::MIN, i32::MIN),
                false
            )
            .is_err()
        );
        let nobody = json!({"localPlayerIndex":0,"localDisconnected":true,"otherPlayers":[]});
        let map = invoke(
            &data,
            Scenario::Battle(10),
            &input(json!({"multiplayerResultPanel":nobody})),
            &metric,
            (i32::MAX - 1, i32::MAX),
            true,
        )
        .unwrap();
        assert_eq!(lookup(&map, i32::MAX - 1), 70);
        assert_eq!(lookup(&map, i32::MAX), 12);
    }

    #[test]
    fn sparse_external_ranks_require_every_supported_integer_without_filling_holes() {
        let data = fixture();
        let metric = Metric::ClientEventPoints { event_id: 7 };
        let ranks = json!([
            {"localFinalScore":-2,"resolvedTotalScoreRank":0},
            {"localFinalScore":-1,"resolvedTotalScoreRank":3},
            {"localFinalScore":0,"resolvedTotalScoreRank":2}
        ]);
        invoke(&data, Scenario::Battle(10), &input(json!({"multiplayerRanks":ranks})), &metric, (-2, 0), true).unwrap();
        let gap = input(json!({"multiplayerRanks":[
            {"localFinalScore":-2,"resolvedTotalScoreRank":2},{"localFinalScore":0,"resolvedTotalScoreRank":2}
        ]}));
        assert!(invoke(&data, Scenario::Battle(10), &gap, &metric, (-2, 0), false).is_err());
        invoke(&data, Scenario::Battle(10), &gap, &metric, (0, 0), true).unwrap();
    }

    #[test]
    fn items_merge_constant_steps_but_still_execute_point_validation_in_every_rank_cell() {
        let mut data = fixture();
        let input = input(json!({"selectedRewards":[{"eventId":7,"rewardId":1}]}));
        let metric = Metric::ConditionalClientEventItems { event_id: 7, resource_type: 4, resource_id: 88 };
        assert_eq!(
            invoke(&data, Scenario::Free(10), &input, &metric, (-10, 30), true).unwrap(),
            PayoffMap::NativeSteps(vec![PayoffStep { lower: -10, upper: 30, value: 7 }])
        );
        replace_table(
            &mut data,
            "MasterLiveEventPoint",
            json!([
                {"_id":1,"_group":1,"_scoreRank":2,"_value":5},{"_id":2,"_group":1,"_scoreRank":4,"_value":5}
            ]),
        );
        assert!(invoke(&data, Scenario::Free(10), &input, &metric, (0, 30), false).is_err());
    }

    #[test]
    fn native_skip_fixed_rank_merges_after_exp_guard_and_retains_parse_failure_none_reward() {
        use ournotes_sim::scenario::ResultClockInput;
        let mut data = fixture();
        let mut input = input(json!({}));
        input.result_clock = Some(ResultClockInput::Skip { server_now_jst_ticks: 100 });
        for (scenario, metric, value) in [
            (Scenario::Free(10), Metric::ClientEventPoints { event_id: 7 }, 1),
            (Scenario::Free(10), Metric::ClientChallengePoints { event_id: 7 }, 9),
            (Scenario::Challenge(70), Metric::ClientEventPoints { event_id: 7 }, 3),
        ] {
            // Fixed-rank counter rewards do not bypass the preceding score-derived EXP rank lookup.
            assert!(invoke(&data, scenario, &input, &metric, (i32::MIN, i32::MAX), false).is_err());
            assert_eq!(
                invoke(&data, scenario, &input, &metric, (-10, i32::MAX), false).unwrap(),
                PayoffMap::NativeSteps(vec![PayoffStep { lower: -10, upper: i32::MAX, value }])
            );
        }
        set_column(&mut data, "MasterParameter", &mut |row| {
            if row["_id"] == "live_skip_result_score_rank" {
                row["_value"] = json!("invalid");
            }
        });
        assert!(
            invoke(&data, Scenario::Free(10), &input, &Metric::ClientEventPoints { event_id: 7 }, (-11, 50), false)
                .is_err()
        );
        assert_eq!(
            invoke(&data, Scenario::Free(10), &input, &Metric::ClientEventPoints { event_id: 7 }, (-10, 50), true)
                .unwrap(),
            PayoffMap::NativeSteps(vec![PayoffStep { lower: -10, upper: 50, value: 70 }])
        );
    }

    #[test]
    fn undefined_items_and_native_reward_wrapping_are_not_replaced_with_guessed_values() {
        let mut data = fixture();
        let item = Metric::ConditionalClientEventItems { event_id: 7, resource_type: 4, resource_id: 88 };
        assert!(invoke(&data, Scenario::Free(10), &input(json!({})), &item, (0, 30), false).is_err());
        set_column(&mut data, "MasterLiveEventPoint", &mut |row| {
            row["_value"] = json!(i64::from(i32::MAX));
        });
        // Native value*10000 wraps before division even with no deck bonus. Preserve that signed payoff.
        let map = invoke(
            &data,
            Scenario::Free(10),
            &input(json!({})),
            &Metric::ClientEventPoints { event_id: 7 },
            (-10, 30),
            true,
        )
        .unwrap();
        assert_eq!(lookup(&map, 0), -1);
    }
}
