//! Rank truncation checked against independently enumerated native nominal branches.
use super::*;
use crate::live::full::luck_dp::rank_residues as residue_dp;

fn detailed(input: &RushCase, refine: bool) -> LuckScoreBounds {
    let skills = luck_skills(&input.master).unwrap();
    super::super::super::luck_score_bounds_internal_policy(
        &input.master,
        &skills,
        &input.deck,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        input.ranking.as_deref(),
        true,
        refine,
        None,
        None,
        None,
        None,
        &mut || false,
        false,
        None,
    )
    .unwrap()
    .unwrap()
}

fn rank_case(critical_weight: i64, member_probe: bool) -> RushCase {
    let mut input = four_bucket_case(2400, critical_weight, member_probe);
    input.params.total_power = 997;
    for row in &mut input.master.gekisou_ranking_score_bonuses {
        row.score_bonus_percent = 10;
    }
    input.master.reindex().unwrap();
    input
}

fn session<'a>(input: &'a RushCase, skills: &'a LuckSkills) -> LuckScoreSession<'a> {
    LuckScoreSession::new(
        &input.master,
        skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        input.ranking.as_deref(),
    )
}

fn summary_bits(summary: &LuckScoreSummary) -> (u64, u64, i32, i32, Option<i32>, Option<i32>) {
    (
        summary.final_mean.lower.to_bits(),
        summary.final_mean.upper.to_bits(),
        summary.final_support.lower,
        summary.final_support.upper,
        summary.exact_constant_score,
        summary.exact_final_life,
    )
}

fn assert_native_mean(summary: &LuckScoreSummary, branches: &[NativeBranch]) {
    let mean = branches.iter().fold(Fraction::ZERO, |mean, branch| {
        assert!(branch.total_score >= 0);
        for &(range, _, bonus, percent) in &branch.rank_bonuses {
            let (start, end) = branch.rank_ranges[range];
            let score = end.wrapping_sub(start);
            assert!(score >= 0);
            assert_eq!(i128::from(bonus), i128::from(score) * i128::from(percent) / 100);
        }
        mean.plus(branch.mass.times(branch.total_score as u128, 1))
    });
    assert!(mean.at_least(summary.final_mean.lower), "native mean {mean:?} below {summary:?}");
    assert!(mean.at_most(summary.final_mean.upper), "native mean {mean:?} above {summary:?}");
    for branch in branches {
        assert!(summary.final_support.lower <= branch.total_score);
        assert!(branch.total_score <= summary.final_support.upper);
        assert_eq!(summary.exact_final_life, Some(branch.final_life));
    }
}

fn assert_refined(original: &LuckScoreSummary, refined: &LuckScoreSummary) {
    assert!(original.final_mean.lower <= refined.final_mean.lower);
    assert!(refined.final_mean.upper <= original.final_mean.upper);
    let before = original.final_mean.upper - original.final_mean.lower;
    let after = refined.final_mean.upper - refined.final_mean.lower;
    assert!(after + 0.1 < before, "the fixture must actually refine a rank: {before} -> {after}");
    assert_eq!(original.final_support.lower, refined.final_support.lower);
    assert_eq!(original.final_support.upper, refined.final_support.upper);
    assert_eq!(original.exact_final_life, refined.exact_final_life);
    assert_eq!(original.exact_constant_score, refined.exact_constant_score);
}

#[test]
fn rank_residue_summary_encloses_complete_native_means_with_both_probe_sources() {
    for (critical_weight, member_probe) in [(1, false), (2, true)] {
        let input = rank_case(critical_weight, member_probe);
        let branches = native_branches(&input);
        assert!(branches.iter().any(|branch| branch.depth >= 6));
        for index in [1, 3, 5] {
            let mut classes = [false; 4];
            for branch in &branches {
                classes[branch.buckets[index]] = true;
            }
            assert!(classes.into_iter().all(|seen| seen));
        }
        let skills = luck_skills(&input.master).unwrap();
        let mut results = Vec::new();
        for capacity in [0, 8 << 20] {
            let mut curves = LuckDpCache::new(capacity);
            let mut session = session(&input, &skills);
            let original = session.summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
            let refined = session.rank_summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
            assert_refined(&original, &refined);
            assert_native_mean(&refined, &branches);
            results.push(summary_bits(&refined));
        }
        assert_eq!(results[0], results[1], "storage cannot change the nominal rank correction");
    }
}

#[test]
fn rank_residue_summary_cache_does_not_replace_the_original_summary_mode() {
    let input = rank_case(2, false);
    let skills = luck_skills(&input.master).unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let mut session = session(&input, &skills);
    let original = session.summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
    let refined = session.rank_summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
    assert_refined(&original, &refined);
    let ordinary_again = session.summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
    let refined_again = session.rank_summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
    assert_eq!(summary_bits(&ordinary_again), summary_bits(&original));
    assert_eq!(summary_bits(&refined_again), summary_bits(&refined));
}

#[test]
fn rank_residue_summary_cancellation_publishes_no_partial_refinement_or_cache_value() {
    let input = rank_case(2, false);
    let skills = luck_skills(&input.master).unwrap();
    for capacity in [0, 8 << 20] {
        let mut curves = LuckDpCache::new(capacity);
        let mut checks = 0usize;
        let reference = session(&input, &skills)
            .rank_summary(&input.deck, Some(&mut curves), || {
                checks += 1;
                false
            })
            .unwrap()
            .unwrap();
        assert!(checks > 4);
        for stop in [1, checks / 2, checks] {
            let mut session = session(&input, &skills);
            let mut curves = LuckDpCache::new(capacity);
            let mut seen = 0usize;
            assert!(
                session
                    .rank_summary(&input.deck, Some(&mut curves), || {
                        seen += 1;
                        seen >= stop
                    })
                    .unwrap()
                    .is_none()
            );
            let restored = session.rank_summary(&input.deck, Some(&mut curves), || false).unwrap().unwrap();
            assert_eq!(summary_bits(&restored), summary_bits(&reference));
        }
    }
}

#[test]
fn rank_residues_include_scored_notes_when_the_controller_marginal_does_not_change() {
    let mut input = rank_case(2, false);
    // Both additions are real scoring notes in the first rank window. Native type 122 adds no Luck
    // gauge, so the already random joint class is unchanged at these two chart-time observations.
    input.master.note_parameters.push(crate::master::NoteParameterRow {
        id: 122,
        note_operate_type: 122,
        score_percent: 123,
    });
    input.notes.extend([
        LiveNote { note_id: 90, note_operate_type: 122, judgement_type: 1, time_ms: 310 },
        LiveNote { note_id: 91, note_operate_type: 122, judgement_type: 1, time_ms: 320 },
    ]);
    input.notes.sort_by_key(|note| (note.time_ms, note.note_id));
    rebuild_rush_frames(&mut input, |_| 5);
    input.master.reindex().unwrap();
    let skills = luck_skills(&input.master).unwrap();
    let curve = crate::live::full::luck_rush_dp_certified_with_events(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        &input.deck,
        None,
    )
    .unwrap();
    let joint_at = |time| {
        let index = curve.steps.partition_point(|step| step.0 <= time);
        curve.steps[index - 1].1
    };
    assert_eq!(joint_at(310), joint_at(320));
    assert!(joint_at(310).iter().all(|mass| mass.interval().lower() > 0.0));
    assert!(curve.steps.iter().all(|step| step.0 != 320));
    let branches = native_branches(&input);
    assert!(
        branches.iter().all(|branch| {
            [90, 91].into_iter().all(|id| branch.notes.iter().any(|note| note.1 == id && note.2 > 0))
        })
    );
    let mut session = session(&input, &skills);
    let original = session.summary(&input.deck, None, || false).unwrap().unwrap();
    let refined = session.rank_summary(&input.deck, None, || false).unwrap().unwrap();
    assert_refined(&original, &refined);
    assert_native_mean(&refined, &branches);
}

#[test]
fn rank_residues_group_distinct_native_notes_at_the_same_chart_time() {
    let mut input = rank_case(2, true);
    input.notes[0].time_ms = input.notes[1].time_ms;
    rebuild_rush_frames(&mut input, |_| 5);
    let branches = native_branches(&input);
    assert!(branches.iter().all(|branch| branch.buckets[0] == branch.buckets[1]));
    let skills = luck_skills(&input.master).unwrap();
    let mut session = session(&input, &skills);
    let original = session.summary(&input.deck, None, || false).unwrap().unwrap();
    let refined = session.rank_summary(&input.deck, None, || false).unwrap().unwrap();
    assert_refined(&original, &refined);
    assert_native_mean(&refined, &branches);
}

#[test]
fn rank_residue_bounds_use_each_actual_native_rank_snapshot() {
    let input = rank_case(2, true);
    let branches = native_branches(&input);
    let original = detailed(&input, false);
    let refined = detailed(&input, true);
    assert_eq!(original.ranges.len(), refined.ranges.len());
    let mut narrowed = 0;
    for (old, new) in original.ranges.iter().zip(&refined.ranges) {
        assert_eq!((old.range, old.start_query, old.end_query), (new.range, new.start_query, new.end_query));
        assert_eq!((old.mean.lower, old.mean.upper), (new.mean.lower, new.mean.upper));
        assert_eq!((old.support.lower, old.support.upper), (new.support.lower, new.support.upper));
        assert_eq!(
            (old.bonus_support.lower, old.bonus_support.upper),
            (new.bonus_support.lower, new.bonus_support.upper)
        );
        let expected = branches.iter().fold(Fraction::ZERO, |sum, branch| {
            let &(_, _, bonus, _) = branch.rank_bonuses.iter().find(|row| row.0 == new.range).unwrap();
            sum.plus(branch.mass.times(bonus as u128, 1))
        });
        assert!(expected.at_least(new.bonus_mean.lower));
        assert!(expected.at_most(new.bonus_mean.upper));
        let before = old.bonus_mean.upper - old.bonus_mean.lower;
        let after = new.bonus_mean.upper - new.bonus_mean.lower;
        narrowed += usize::from(after + 0.1 < before);
    }
    assert!(narrowed > 0, "the native snapshot fixture must admit a residue refinement");
    assert_eq!(serde_json::to_vec(&original.queries).unwrap(), serde_json::to_vec(&refined.queries).unwrap());
    assert_eq!(
        (original.final_note_mean.lower, original.final_note_mean.upper),
        (refined.final_note_mean.lower, refined.final_note_mean.upper),
    );
}

fn requests(bounds: &LuckScoreBounds) -> Vec<residue_dp::Request> {
    bounds
        .ranges
        .iter()
        .map(|range| {
            let start = bounds.queries[range.start_query.unwrap()].score_frame;
            let end = &bounds.queries[range.end_query];
            let mut rewards = BTreeMap::<i32, [Option<u8>; 4]>::new();
            for note in &end.notes {
                let frame = crate::live::score::get_frame(note.time_ms);
                if start < frame && frame <= end.score_frame {
                    assert!(note.probability.is_some());
                    let group = rewards.entry(note.time_ms).or_insert([Some(0); 4]);
                    for (class, bucket) in note.buckets.iter().enumerate() {
                        group[class] = group[class].zip(*bucket).and_then(|(sum, bucket)| {
                            (bucket.lower == bucket.upper)
                                .then_some((i32::from(sum) + bucket.lower.rem_euclid(10)).rem_euclid(10) as u8)
                        });
                    }
                }
            }
            assert!(!rewards.is_empty());
            residue_dp::Request { modulus: 10, rewards }
        })
        .collect()
}

fn class_at(branch: &NativeBranch, time: i32) -> usize {
    let mut classes =
        branch.notes.iter().zip(&branch.buckets).filter_map(|(note, &class)| (note.0 == time).then_some(class));
    let class = classes.next().expect("a requested chart-time group is actually scored");
    assert!(classes.all(|other| other == class));
    class
}

#[test]
fn rank_residue_known_and_unresolved_bins_match_independent_native_path_masses() {
    let input = rank_case(2, true);
    let branches = native_branches(&input);
    let bounds = detailed(&input, false);
    let original_requests = requests(&bounds);
    let skills = luck_skills(&input.master).unwrap();
    for make_unresolved in [false, true] {
        let mut requests = original_requests.clone();
        if make_unresolved {
            // Removing a known reward is a conservative loss of information, never an extra draw. Native
            // paths reaching this class must retain all their mass in the single unresolved partition.
            let (&time, rewards) = requests[0].rewards.iter_mut().next().unwrap();
            let class = (0..4)
                .find(|&class| {
                    rewards[class].is_some()
                        && branches.iter().any(|branch| class_at(branch, time) == class)
                        && branches.iter().any(|branch| class_at(branch, time) != class)
                })
                .expect("the first observation has a nontrivial known class");
            rewards[class] = None;
        }
        let output = residue_dp::probabilities(
            &input.master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
            &input.play,
            &input.delta,
            &input.deck,
            None,
            &requests,
            || false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(output.laws.len(), requests.len());
        for (index, ((request, range), law)) in requests.iter().zip(&bounds.ranges).zip(&output.laws).enumerate() {
            let law = law.as_ref().expect("the requested native note times must all be observed");
            assert_eq!(law.residues.len(), 10);
            let mut expected = [Fraction::ZERO; 10];
            let mut unresolved = Fraction::ZERO;
            for branch in &branches {
                let mut residue = Some(0u8);
                for (&time, rewards) in &request.rewards {
                    residue = residue.zip(rewards[class_at(branch, time)]).map(|(sum, value)| (sum + value) % 10);
                }
                if let Some(residue) = residue {
                    let (start, end) = branch.rank_ranges[range.range];
                    let native = end.wrapping_sub(start).rem_euclid(10) as usize;
                    assert_eq!(native, usize::from(residue), "query rewards must reconstruct the actual rank input");
                    expected[native] = expected[native].plus(branch.mass);
                } else {
                    unresolved = unresolved.plus(branch.mass);
                }
            }
            assert_eq!(expected.iter().copied().fold(unresolved, Fraction::plus), Fraction::ONE);
            for (&expected, actual) in expected.iter().zip(&law.residues) {
                assert!(expected.at_least(actual.interval().lower()));
                assert!(expected.at_most(actual.interval().upper()));
            }
            assert!(unresolved.at_least(law.unresolved.interval().lower()));
            assert!(unresolved.at_most(law.unresolved.interval().upper()));
            if make_unresolved && index == 0 {
                assert!(unresolved.numerator > 0 && unresolved.numerator < unresolved.denominator);
            }
        }
    }
}

#[test]
fn rank_residue_request_requires_its_own_ready_query_and_unchanged_prefix() {
    use super::super::super::rank_residues::request;
    use super::super::super::rank_trace::{RankQuery, RankWindow};
    let point = |value| -> IntegerBounds { I32Interval::point(value).into() };
    let note = LuckNoteBounds {
        note_id: 1,
        time_ms: 100,
        life: 1000,
        buckets: [Some(point(10)); 4],
        combo: F64Interval::ONE.into(),
        probability: Some([
            F64Interval::ONE.into(),
            F64Interval::ZERO.into(),
            F64Interval::ZERO.into(),
            F64Interval::ZERO.into(),
        ]),
        mean: Some(F64Interval::integer(10).into()),
    };
    let mut notes = FxHashMap::default();
    notes.insert(1, vec![(3, note)]);
    let mut queries = vec![
        QueryParts {
            note_mean: F64Interval::ZERO,
            note_support: I32Interval::point(0),
            fixed_coefficients: vec![],
            to: 1,
            executed_from: 0,
            notes: Some(vec![]),
        },
        QueryParts {
            note_mean: F64Interval::integer(10),
            note_support: I32Interval::point(10),
            fixed_coefficients: vec![],
            to: 3,
            executed_from: 2,
            notes: Some(vec![(3, I32Interval::point(10), F64Interval::integer(10))]),
        },
    ];
    let mut window = RankWindow {
        id: 0,
        event: 5,
        range: 0,
        percent: 10,
        frame: 4,
        final_coefficient: 1,
        start: RankQuery { ordinal: 0, event: 0, time_ms: 40, to: 1, probability_ready: 40 },
        end: RankQuery { ordinal: 1, event: 4, time_ms: 120, to: 3, probability_ready: 120 },
    };
    let range = LuckRangeScoreBounds {
        range: 0,
        start_query: Some(0),
        end_query: 1,
        percent: 10,
        mean: F64Interval::integer(10).into(),
        support: point(10),
        bonus_mean: F64Interval::ONE.into(),
        bonus_support: point(1),
        luck_points_mean: None,
        lot_results_mean: None,
    };
    assert!(request(&window, &range, &queries, &notes).is_some());
    window.end.probability_ready = 99;
    assert!(request(&window, &range, &queries, &notes).is_none(), "a future Ready cannot authorize this query");
    window.end.probability_ready = 120;
    queries[1].executed_from = 1;
    assert!(request(&window, &range, &queries, &notes).is_none(), "recomputed prefix values cannot cancel");
    queries[1].executed_from = 2;
    notes.get_mut(&1).unwrap()[0].1.probability = None;
    assert!(request(&window, &range, &queries, &notes).is_none(), "unlinked classes have no residue authority");
    notes.clear();
    assert!(request(&window, &range, &queries, &notes).is_none(), "missing details are not an empty score window");
}
