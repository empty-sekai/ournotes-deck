//! Independent native nominal trees validate historical probe filings, not merely terminal DP readiness.
use super::*;

#[derive(Debug, PartialEq, Eq)]
enum QueryClock {
    Ready(i32),
    Query(i32, i32),
    Rank(usize, i32, i64, Option<usize>, Option<usize>),
}

fn query_clock(trace: &BoundsTrace) -> Vec<QueryClock> {
    trace
        .events
        .iter()
        .filter_map(|event| match event {
            BoundsEvent::ProbabilityReady(time) => Some(QueryClock::Ready(*time)),
            BoundsEvent::Query { time_ms, to } => Some(QueryClock::Query(*time_ms, *to)),
            BoundsEvent::Rank { range, time_ms, percent, start, end } => {
                Some(QueryClock::Rank(*range, *time_ms, *percent, *start, *end))
            }
            _ => None,
        })
        .collect()
}

fn empty_geometry(input: &RushCase) -> BoundsTrace {
    let skills = luck_skills(&input.master).unwrap();
    let mut params = input.params;
    params.total_power = 0;
    let deck: [Performer; 5] = std::array::from_fn(|_| Performer::default());
    let mut native =
        LiveModel::new_gekisou(&input.master, &deck, &input.notes, &input.events, params, &input.setup).unwrap();
    check_recorder(&native, &skills).unwrap();
    native.set_luck_weights(&skills, Vec::new()).unwrap();
    native.score.begin_bounds(Vec::new(), true);
    native.score.certify_bounds_filings(Some(gekisou::M_LUCK));
    assert!(native.try_enable_bounds_record_only());
    for (frame, &dt) in input.play.frames.iter().zip(&input.delta) {
        native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
    }
    assert_eq!(native.draws(), 0);
    native.score.bounds_trace.take().unwrap()
}

fn context<'a>(fixture: &'a FamilyFixture, skills: &'a LuckSkills) -> LuckFamilyContext<'a> {
    let input = &fixture.input;
    LuckFamilyContext::new(
        &input.master,
        skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .unwrap()
    .unwrap()
}

/// Values are exact nominal probabilities of the ACTUAL signed probe command prefix at a rank query,
/// keyed by (range, original note id, chart time). No probability curve or proposed cap is consulted here.
fn native_history(
    input: &RushCase,
    deck: &[Performer],
    geometry: &BoundsTrace,
) -> BTreeMap<(usize, i32, i32), Fraction> {
    let skills = luck_skills(&input.master).unwrap();
    let fresh = || {
        LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup).unwrap()
    };
    let probes = fresh().luck_score_rows(&skills);
    let owners: Vec<_> = probes.iter().filter(|row| row.may_hold).map(|row| row.owner).collect();
    let amplitude = probes.iter().filter(|row| row.may_hold).map(|row| (row.value * 100000f32) as i64).sum::<i64>();
    assert!(amplitude > 0);
    let clock = query_clock(geometry);
    let mut pending = vec![(Vec::new(), Fraction::ONE)];
    let mut history = BTreeMap::<(usize, i32, i32), Fraction>::new();
    let mut completed = Fraction::ZERO;
    let mut visits = 0;
    while let Some((prefix, mass)) = pending.pop() {
        visits += 1;
        assert!(visits <= 8192 && prefix.len() <= 16, "complete the entire independent native nominal tree");
        let mut native = fresh();
        native.score.begin_bounds(Vec::new(), false);
        let result = native.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix.clone()));
        assert!(native.random.nominal_covers_draws());
        if let Some(outcomes) = native.random.nominal_branch() {
            assert!(result.is_err());
            let total = outcomes[0].total;
            assert_eq!(outcomes.iter().map(|outcome| outcome.weight).sum::<u64>(), total);
            for (choice, outcome) in outcomes.iter().enumerate() {
                assert_eq!(outcome.total, total);
                let mut next = prefix.clone();
                next.push(choice);
                pending.push((next, mass.times(u128::from(outcome.weight), u128::from(total))));
            }
            continue;
        }
        result.unwrap();
        assert!(native.random.nominal_prefix_consumed());
        assert!(native.gk.as_ref().unwrap().ctrl.states.iter().all(|state| state.state == gekisou::S_FINISH));
        let trace = native.score.bounds_trace.take().unwrap();
        assert_eq!(query_clock(&trace), clock, "ordinary/probe commands cannot create or remove rank queries");
        let queries: Vec<_> = trace
            .events
            .iter()
            .enumerate()
            .filter_map(
                |(index, event)| {
                    if let BoundsEvent::Query { to, .. } = event { Some((index, *to)) } else { None }
                },
            )
            .collect();
        let signed = |end: usize, time: i32| {
            trace.events[..end]
                .iter()
                .filter_map(|event| {
                    if let BoundsEvent::Factor { command, .. } = event
                        && owners.contains(&command.owner_id)
                        && command.time_ms <= time
                    {
                        Some(i64::from(command.note_mill))
                    } else {
                        None
                    }
                })
                .sum::<i64>()
        };
        for event in &trace.events {
            let BoundsEvent::Rank { range, start: Some(start), end: Some(end), .. } = event else { continue };
            let (start_event, from) = queries[*start];
            let (end_event, to) = queries[*end];
            for (ordinal, event) in trace.events.iter().enumerate() {
                let BoundsEvent::Note { frame, note, .. } = event else { continue };
                if from < *frame as i32 && *frame as i32 <= to {
                    assert!(ordinal < start_event, "every included native note was already filed");
                    let before = signed(start_event, note.time_ms);
                    let historical = signed(end_event, note.time_ms);
                    let terminal = signed(trace.events.len(), note.time_ms);
                    assert_eq!(before, historical, "same signed probe prefix at adjacent rank queries");
                    assert_eq!(historical, terminal, "no future native filing rewrites this ideal probe prefix");
                    assert!(historical == 0 || historical == amplitude, "test holders have no ordinary score rows");
                    let value = history.entry((*range, note.note_id, note.time_ms)).or_insert(Fraction::ZERO);
                    if historical != 0 {
                        *value = value.plus(mass);
                    }
                }
            }
        }
        completed = completed.plus(mass);
    }
    assert_eq!(completed, Fraction::ONE);
    assert!(!history.is_empty());
    history
}

fn replace_clock(fixture: &mut FamilyFixture, times: Vec<i32>, dt: f32) {
    let mut previous = -1;
    fixture.input.play.frames = times
        .into_iter()
        .map(|time_ms| {
            let judged = fixture
                .input
                .notes
                .iter()
                .filter(|note| previous < note.time_ms && note.time_ms <= time_ms)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect();
            previous = time_ms;
            PlayFrame { time_ms, judged }
        })
        .collect();
    fixture.input.delta = vec![dt; fixture.input.play.frames.len()];
}

#[test]
fn profile_rank_probe_history_matches_every_native_branch_and_label_in_both_probe_phases() {
    let mut checked = 0;
    let mut nontrivial = 0;
    for grouped in [false, true] {
        let mut fixture = FamilyFixture::new();
        // Repeated chart times remain separate note occurrences. In both clocks the notes at 300 are
        // exactly at their original frame boundary; in the coarse clock 120/180 are earlier in one frame.
        for (note, time) in fixture.input.notes.iter_mut().zip([120, 180, 300, 300]) {
            note.time_ms = time;
        }
        let step = if grouped { 100 } else { 20 };
        replace_clock(&mut fixture, (0..=1800).step_by(step).collect(), step as f32 / 1000.0);
        if grouped {
            for row in &mut fixture.input.master.skill_effect_settings {
                if row.skill_effect_type == 2000 {
                    row.phase = 1;
                }
            }
            fixture.input.master.reindex().unwrap();
            fixture.input.events = vec![(0, 80), (0, 80), (1, 100), (2, 140), (3, 140), (4, 200)];
        }
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
        let absent = domain.profile_for(&[None; 5]).unwrap();
        let profile = context.prepare_profile(&domain, absent, None, || false).unwrap().unwrap();
        let geometry = empty_geometry(&fixture.input);
        let mut physical: Vec<_> = fixture.choices.iter().map(|choices| choices[0].1.clone()).collect();
        physical[2] = fixture.choices[2][2].1.clone(); // Ordinary extension + timed reward remain present.
        for (label, order) in physical_orders().into_iter().enumerate() {
            assert!(profile.order_rank_probe_history_ready(label), "grouped={grouped} order={order:?}");
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let history = native_history(&fixture.input, &deck, &geometry);
            for ((_, _, time), mass) in history {
                let joint = profile.orders()[label].joint_at(time);
                assert_probability_contains(mass, joint[1].merge_disjoint(joint[3]));
                nontrivial += usize::from(mass.numerator > 0 && mass.numerator < mass.denominator);
            }
            checked += 1;
        }
        assert!(!profile.order_rank_probe_history_ready(120));
        assert!(profile.order_probe_certificates(120).is_none());
    }
    assert_eq!(checked, 2 * 120);
    assert!(nontrivial > 0, "history weighting must exercise both active and inactive positive-mass native paths");
}

#[test]
fn profile_rank_probe_history_refuses_a_real_note_not_filed_at_the_native_historical_query() {
    let mut fixture = FamilyFixture::new();
    fixture.input.notes.push(LiveNote { note_id: 4, time_ms: 415, note_operate_type: 1, judgement_type: 1 });
    fixture.input.params.converted_note_count = 5;
    fixture.input.setup.fevers[0].1 = 401;
    let mut clock: Vec<_> = (0..=400).step_by(20).collect();
    clock.extend([401, 402, 403, 404, 405]);
    clock.extend((420..=1800).step_by(20));
    replace_clock(&mut fixture, clock, 1.0);
    let geometry = empty_geometry(&fixture.input);
    let input = &fixture.input;
    let mut actual =
        LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup)
            .unwrap();
    actual.score.begin_bounds(Vec::new(), false);
    actual.run_with_random(&input.play, &input.delta, LiveRandom::new(19)).unwrap();
    let actual = actual.score.bounds_trace.take().unwrap();
    assert_eq!(query_clock(&actual), query_clock(&geometry));
    for trace in [&geometry, &actual] {
        let rank = trace.events.iter().position(|event| matches!(event, BoundsEvent::Rank { .. })).unwrap();
        let late = trace
            .events
            .iter()
            .position(|event| matches!(event, BoundsEvent::Note { note, .. } if note.note_id == 4))
            .unwrap();
        assert!(
            rank < late,
            "elapsed-time completion can precede the next chart-time judgement, including in the full scored native run"
        );
    }
    assert_eq!(crate::live::score::get_frame(401), crate::live::score::get_frame(415));
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
    let profile = context.prepare_profile(&domain, 0, None, || false).unwrap().unwrap();
    assert_eq!(profile.orders().len(), 120, "complete controller law remains available");
    assert!((0..120).all(|order| profile.order_probe_run_bound(order).is_some()));
    assert!(
        (0..120).all(|order| !profile.order_rank_probe_history_ready(order)),
        "one missing historical filing refuses the entire optional term"
    );
}
