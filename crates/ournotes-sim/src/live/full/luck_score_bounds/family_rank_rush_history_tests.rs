//! Complete native nominal trees observe signed historical Rush/probe prefixes without using the DP as oracle.
use super::*;
use std::collections::BTreeSet;

type HistoricalJoint = BTreeMap<(usize, i32, i32), [Fraction; 4]>;

fn clock(trace: &BoundsTrace) -> Vec<(u8, i32, i32)> {
    trace
        .events
        .iter()
        .filter_map(|event| match event {
            BoundsEvent::ProbabilityReady(time) => Some((0, *time, 0)),
            BoundsEvent::Query { time_ms, to } => Some((1, *time_ms, *to)),
            BoundsEvent::Rank { range, time_ms, .. } => Some((2, *time_ms, *range as i32)),
            _ => None,
        })
        .collect()
}

fn geometry(input: &RushCase) -> BoundsTrace {
    geometry_with_non_luck_finishes(input).0
}

fn geometry_with_non_luck_finishes(input: &RushCase) -> (BoundsTrace, Vec<i32>) {
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
    let mut non_luck_finishes = Vec::new();
    for (frame, &dt) in input.play.frames.iter().zip(&input.delta) {
        native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
        let controller = &native.gk.as_ref().unwrap().ctrl;
        if controller.state_updates.iter().any(|&range| {
            controller.ranges[range].mission != gekisou::M_LUCK && controller.states[range].state == gekisou::S_FINISH
        }) {
            non_luck_finishes.push(frame.time_ms);
        }
    }
    assert_eq!(native.draws(), 0);
    (native.score.bounds_trace.take().unwrap(), non_luck_finishes)
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

fn native_joint_history(input: &RushCase, deck: &[Performer], geometry: &BoundsTrace) -> (HistoricalJoint, bool) {
    let skills = luck_skills(&input.master).unwrap();
    let fresh = || {
        LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup).unwrap()
    };
    let probes = fresh().luck_score_rows(&skills);
    let owners: Vec<_> = probes.iter().filter(|row| row.may_hold).map(|row| row.owner).collect();
    let amplitude = probes.iter().filter(|row| row.may_hold).map(|row| (row.value * 100000f32) as i64).sum::<i64>();
    assert!(amplitude > 0);
    let mut query = 0;
    let mut possible = BTreeSet::new();
    for event in &geometry.events {
        match event {
            BoundsEvent::Query { .. } => query += 1,
            BoundsEvent::Potential { frame } => {
                possible.insert((query, *frame));
            }
            BoundsEvent::Factor { frame, command } if command.luck != 0 => {
                possible.insert((query, *frame));
            }
            _ => {}
        }
    }
    let mut pending = vec![(Vec::new(), Fraction::ONE)];
    let mut joint = HistoricalJoint::new();
    let mut completed = Fraction::ZERO;
    let mut visits = 0;
    let mut same_frame_switches = false;
    while let Some((prefix, mass)) = pending.pop() {
        visits += 1;
        assert!(visits <= 8192 && prefix.len() <= 16, "enumerate the entire independent nominal tree");
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
        assert_eq!(clock(&trace), clock(geometry));
        let mut queries = Vec::new();
        let mut signs = BTreeMap::<(usize, usize), u8>::new();
        for (event_index, event) in trace.events.iter().enumerate() {
            match event {
                BoundsEvent::Query { to, .. } => queries.push((event_index, *to)),
                BoundsEvent::Factor { frame, command } if command.luck != 0 => {
                    assert!(
                        possible.contains(&(queries.len(), *frame)),
                        "empty geometry must cover actual Rush filings"
                    );
                    *signs.entry((queries.len(), *frame)).or_default() |= if command.luck > 0 { 1 } else { 2 };
                }
                _ => {}
            }
        }
        same_frame_switches |= signs.values().any(|sign| *sign == 3);
        let signed = |end: usize, time: i32| {
            let mut sums = (0i64, 0i64);
            for event in &trace.events[..end] {
                if let BoundsEvent::Factor { command, .. } = event
                    && command.time_ms <= time
                {
                    sums.0 += i64::from(command.luck);
                    if owners.contains(&command.owner_id) {
                        sums.1 += i64::from(command.note_mill);
                    }
                }
            }
            sums
        };
        for event in &trace.events {
            let BoundsEvent::Rank { range, start: Some(start), end: Some(end), .. } = event else { continue };
            let (a, from) = queries[*start];
            let (b, to) = queries[*end];
            for (ordinal, event) in trace.events.iter().enumerate() {
                let BoundsEvent::Note { frame, note, .. } = event else { continue };
                if from < *frame as i32 && *frame as i32 <= to {
                    assert!(ordinal < a);
                    let historical = signed(a, note.time_ms);
                    assert_eq!(historical, signed(b, note.time_ms), "the adjacent signed prefixes agree");
                    assert_eq!(
                        historical,
                        signed(trace.events.len(), note.time_ms),
                        "future filings cannot rewrite history"
                    );
                    assert!(matches!(historical.0, 0 | 47));
                    assert!(historical.1 == 0 || historical.1 == amplitude);
                    let bucket = 2 * usize::from(historical.0 != 0) + usize::from(historical.1 != 0);
                    let values = joint.entry((*range, note.note_id, note.time_ms)).or_insert([Fraction::ZERO; 4]);
                    values[bucket] = values[bucket].plus(mass);
                }
            }
        }
        completed = completed.plus(mass);
    }
    assert_eq!(completed, Fraction::ONE);
    assert!(!joint.is_empty());
    for values in joint.values() {
        assert_eq!(values.iter().fold(Fraction::ZERO, |sum, value| sum.plus(*value)), Fraction::ONE);
    }
    (joint, same_frame_switches)
}

#[test]
fn profile_rank_rush_history_matches_complete_native_joint_mass_at_all_120_labels() {
    let mut checked = 0;
    let mut nontrivial_rush = false;
    let mut same_frame_switches = false;
    for grouped in [false, true] {
        let mut fixture = FamilyFixture::new();
        for (note, time) in fixture.input.notes.iter_mut().zip([120, 180, 300, 300]) {
            note.time_ms = time;
        }
        // A non-dyadic law exercises rational weighting without normalizing accumulated endpoint masses.
        for row in &mut fixture.input.master.gekisou_luck_bonus_lots {
            row.weight = if row.lot_result == 3 { 2 } else { 1 };
        }
        fixture.input.master.reindex().unwrap();
        let step = if grouped { 100 } else { 20 };
        replace_clock(&mut fixture, (0..=1800).step_by(step).collect(), step as f32 / 1000.0);
        let skills = luck_skills(&fixture.input.master).unwrap();
        let context = context(&fixture, &skills);
        let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
        let resources = if grouped { [Some(0), None, None, None, None] } else { [None; 5] };
        let selected = domain.profile_for(&resources).unwrap();
        let profile = context.prepare_profile(&domain, selected, None, || false).unwrap().unwrap();
        let geometry = geometry(&fixture.input);
        let mut physical: Vec<_> = fixture.choices.iter().map(|choices| choices[0].1.clone()).collect();
        if grouped {
            physical[0] = fixture.choices[0][1].1.clone();
        }
        physical[2] = fixture.choices[2][2].1.clone(); // Keep actual timed ordinary rewards and extension.
        for (label, order) in physical_orders().into_iter().enumerate() {
            assert!(profile.order_rank_rush_history_ready(label));
            assert!(profile.order_rank_probe_history_ready(label));
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let (history, switched) = native_joint_history(&fixture.input, &deck, &geometry);
            same_frame_switches |= switched;
            for ((_, _, time), masses) in history {
                let law = profile.orders()[label].joint_at(time);
                for (mass, enclosure) in masses.into_iter().zip(law) {
                    assert_probability_contains(mass, enclosure);
                }
                let rush = masses[2].plus(masses[3]);
                nontrivial_rush |= rush.numerator > 0 && rush.numerator < rush.denominator;
            }
            checked += 1;
        }
        assert!(!profile.order_rank_rush_history_ready(120));
        assert!(!profile.order_rank_rush_history_ready(usize::MAX));
        assert!(context.prepare_profile(&domain, selected, None, || true).unwrap().is_none());
    }
    assert_eq!(checked, 240);
    assert!(nontrivial_rush);
    assert!(same_frame_switches, "multiple native Rush switches in one closed score frame must remain covered");
}

#[test]
fn profile_rank_rush_history_refuses_a_real_note_filed_after_its_historical_query() {
    let mut fixture = FamilyFixture::new();
    fixture.input.notes.push(LiveNote { note_id: 4, time_ms: 415, note_operate_type: 1, judgement_type: 1 });
    fixture.input.params.converted_note_count = 5;
    fixture.input.setup.fevers[0].1 = 401;
    let mut times: Vec<_> = (0..=400).step_by(20).collect();
    times.extend([401, 402, 403, 404, 405]);
    times.extend((420..=1800).step_by(20));
    replace_clock(&mut fixture, times, 1.0);
    let trace = geometry(&fixture.input);
    let rank = trace.events.iter().position(|event| matches!(event, BoundsEvent::Rank { .. })).unwrap();
    let late = trace
        .events
        .iter()
        .position(|event| matches!(event, BoundsEvent::Note { note, .. } if note.note_id == 4))
        .unwrap();
    assert!(rank < late);
    assert_eq!(crate::live::score::get_frame(401), crate::live::score::get_frame(415));
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
    let profile = context.prepare_profile(&domain, 0, None, || false).unwrap().unwrap();
    assert_eq!(profile.orders().len(), 120);
    assert!((0..120).all(|order| !profile.order_rank_rush_history_ready(order)));
}

#[test]
fn profile_rank_rush_history_does_not_require_the_optional_common_probe_phase() {
    let mut fixture = FamilyFixture::new();
    let mut probe =
        fixture.input.master.gekisou_skill_effects.iter().find(|row| row.skill_id == FAMILY_SCORE).unwrap().clone();
    probe.id = 9959;
    probe.skill_effect_type = 2005;
    probe.effect_value = -7000;
    fixture.input.master.gekisou_skill_effects.push(probe);
    fixture.input.master.skill_effect_settings.retain(|row| row.skill_effect_type != 2005);
    fixture.input.master.skill_effect_settings.push(
        serde_json::from_value(json!({
            "_id":9959, "_skillEffectType":2005, "_phase":1
        }))
        .unwrap(),
    );
    fixture.input.master.reindex().unwrap();
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
    let profile = context.prepare_profile(&domain, 0, None, || false).unwrap().unwrap();
    assert_eq!(profile.orders().len(), 120);
    assert!((0..120).all(|order| profile.order_rank_rush_history_ready(order)));
    assert!((0..120).all(|order| profile.order_probe_run_bound(order).is_none()));
    assert!((0..120).all(|order| !profile.order_rank_probe_history_ready(order)));
    let mut insufficient = family_limits();
    insufficient.max_order_evaluations = 119;
    assert!(context.admit_domain(&family_choices(&fixture), insufficient, || false).is_err());
}

#[test]
fn profile_rank_rush_history_cannot_publish_before_overlapping_non_luck_finish_profile_is_rejected() {
    let mut fixture = FamilyFixture::new();
    fixture.input.setup.fevers = vec![(0, 140), (100, 420)];
    fixture.input.setup.missions = vec![gekisou::M_COMBO, gekisou::M_LUCK, gekisou::M_LUCK];
    fixture.input.master.gekisou_ranking_score_bonuses[0].mission_pattern = gekisou::mission_pattern(1, 2, 2);
    fixture.input.master.reindex().unwrap();
    let (trace, non_luck_finishes) = geometry_with_non_luck_finishes(&fixture.input);
    // A non-LUCK range's FINISH can disable the global Rush handle while a LUCK range remains active.
    // Its before-frame Potential covers that opportunity even if the weighted path's handle is already
    // absent. Geometry still cannot supply a controller law outside the complete overlap admission.
    assert!(!non_luck_finishes.is_empty());
    for &time in &non_luck_finishes {
        let frame = crate::live::score::get_frame(time).min(trace.frames as i32 - 1) as usize;
        assert!(
            trace
                .events
                .iter()
                .any(|event| matches!(event, BoundsEvent::Potential { frame: possible } if *possible == frame))
        );
    }
    assert!(trace.events.iter().any(|event| matches!(event, BoundsEvent::Factor { command, .. }
        if command.luck < 0 && non_luck_finishes.contains(&command.time_ms))));
    // Independently find a complete positive-mass native branch with this non-LUCK inverse. This is a
    // refusal counterexample, not a published probability estimate, so finding one branch is sufficient.
    let input = &fixture.input;
    let mut pending = vec![Vec::new()];
    let mut actual_inverse = false;
    let mut visits = 0;
    while let Some(prefix) = pending.pop() {
        visits += 1;
        assert!(visits <= 8192);
        let mut native =
            LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup)
                .unwrap();
        native.score.begin_bounds(Vec::new(), false);
        let result = native.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix.clone()));
        if let Some(outcomes) = native.random.nominal_branch() {
            assert!(result.is_err());
            for (choice, outcome) in outcomes.iter().enumerate() {
                assert!(outcome.weight > 0);
                let mut next = prefix.clone();
                next.push(choice);
                pending.push(next);
            }
            continue;
        }
        result.unwrap();
        assert!(native.random.nominal_prefix_consumed());
        if native.score.bounds_trace.as_ref().unwrap().events.iter().any(|event| {
            matches!(event, BoundsEvent::Factor { command, .. }
                if command.luck < 0 && non_luck_finishes.contains(&command.time_ms))
        }) {
            actual_inverse = true;
            break;
        }
    }
    assert!(actual_inverse, "the complete native path must actually exercise the non-LUCK inverse");
    let skills = luck_skills(&fixture.input.master).unwrap();
    let context = context(&fixture, &skills);
    // Pair-domain admission does not record any complete controller law. It may retain a legal physical
    // domain with unavailable historical geometry; only profile preparation reaches the native overlap
    // guard. Its refusal must prevent both the complete profile and its history capability from escaping.
    let domain = context.admit_domain(&family_choices(&fixture), family_limits(), || false).unwrap().unwrap();
    let error = context.prepare_profile(&domain, 0, None, || false).unwrap_err();
    assert_eq!(error.reason, LuckFamilyDecline::ProbabilityDomain);
    assert!(error.error.to_string().contains("overlaps another active range"), "{error:?}");
}
