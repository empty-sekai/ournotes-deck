//! Complete theoretical streams retain structural chart geometry without inventing judgement inputs.
use super::*;
use crate::live::full::LuckFamilyError;
use crate::live::model::{JudgementStream, JustRule};
use crate::live::score::LiveScoreSettings;
use crate::live::skip::{Chart, ChartNote, SkillEvent, is_judgement_note};

const STRUCTURAL_TAIL: i32 = 1200;

fn structural_fixture() -> FamilyFixture {
    let mut fixture = FamilyFixture::new();
    let input = &mut fixture.input;
    // Every non-judgement operate type is represented. The final structural node is later than every judged
    // note, skill event and fever, so filtering it before constructing the clock would change the geometry.
    for (index, (kind, time_ms)) in
        [(0, 100), (80, 140), (82, 200), (100, 240), (103, 260), (121, 340), (122, 400), (123, STRUCTURAL_TAIL)]
            .into_iter()
            .enumerate()
    {
        assert!(!is_judgement_note(kind));
        input.notes.push(LiveNote {
            note_id: 9800 + index as i32,
            time_ms,
            note_operate_type: kind,
            judgement_type: 1,
        });
    }
    input.notes.sort_by_key(|note| (note.time_ms, note.note_id));
    let chart = Chart::from_notes(
        input
            .notes
            .iter()
            .map(|note| ChartNote { id: note.note_id, time_ms: note.time_ms, note_type: note.note_operate_type })
            .collect(),
        input.events.iter().map(|&(index, time_ms)| SkillEvent { index, time_ms }).collect(),
        &LiveScoreSettings::from_master(&input.master).unwrap(),
    )
    .unwrap();
    let types: Vec<_> = input.notes.iter().map(|note| note.judgement_type).collect();
    let rule = JustRule::new(&input.master, &input.setup).unwrap();
    let stream = JudgementStream::theoretical_best_gekisou(&chart, &types, &rule).unwrap();
    input.play = stream.to_live_play().unwrap();
    input.delta = stream.delta_times().unwrap();
    input.params.converted_note_count = chart.converted_note_count;
    input.params.music_length_ms = chart.last_timing_note_ms + 1000;
    assert_eq!(input.params.converted_note_count, 4);
    assert_eq!(input.params.music_length_ms, STRUCTURAL_TAIL + 1000);
    assert_eq!(input.play.frames.last().unwrap().time_ms, STRUCTURAL_TAIL + 2000);
    assert_eq!(input.play.frames.iter().map(|frame| frame.judged.len()).sum::<usize>(), 4);
    fixture
}

fn structural_context_error(fixture: &FamilyFixture) -> LuckFamilyError {
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    LuckFamilyContext::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .expect_err("the optional family must retain an explicit refusal")
}

#[test]
fn controller_family_keeps_structural_nodes_in_complete_native_order_laws() {
    let fixture = structural_fixture();
    let input = &fixture.input;
    let native =
        LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup)
            .unwrap();
    assert_eq!(native.notes.len(), input.notes.len());
    assert!(input.notes.iter().all(|note| native.notes.get(&note.note_id) == Some(note)));
    assert!(
        input.notes.iter().filter(|note| (100..=420).contains(&note.time_ms)).count()
            > input.params.converted_note_count as usize
    );

    let family = prepare_family(&fixture, 8 * 1024 * 1024);
    let uncached = prepare_family(&fixture, 0);
    let expected_times: Vec<_> =
        input.notes.iter().filter(|note| is_judgement_note(note.note_operate_type)).map(|note| note.time_ms).collect();
    assert_eq!(family.note_times(), expected_times);
    assert_eq!(uncached.note_times(), family.note_times());
    assert_eq!(family.orders().len(), 6 * 120);
    assert_eq!(uncached.orders().len(), family.orders().len());

    // Validate all legal bindings. Then take one binding of each controller profile, including an ordinary
    // reward Snap, and independently enumerate every nominal branch in every original performance order.
    let mut representatives = BTreeMap::new();
    for (resources, physical) in physical_bindings(&fixture) {
        let profile = family.profile_for(&resources).expect("every original physical binding remains legal");
        assert_eq!(uncached.profile_for(&resources), Some(profile));
        representatives.insert(profile, physical);
    }
    assert_eq!(representatives.len(), family.profile_count());
    let mut checked = 0;
    let mut nonconstant = 0;
    for (profile, physical) in representatives {
        for order in physical_orders() {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let matching: Vec<_> =
                family.orders().iter().filter(|law| law.profile == profile && law.positions == positions).collect();
            assert_eq!(matching.len(), 1);
            let without_cache =
                uncached.orders().iter().find(|law| law.profile == profile && law.positions == positions).unwrap();
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let oracle = family_native_oracle(input, &deck);
            nonconstant += usize::from(oracle.scores.len() > 1);
            for &time in family.note_times() {
                let actual = matching[0].joint_at(time);
                assert_eq!(without_cache.joint_at(time), actual);
                for (mass, enclosure) in oracle.joint[&time].into_iter().zip(actual) {
                    assert_probability_contains(mass, enclosure);
                }
            }
            let bounds = crate::live::full::luck_score_bounds(
                &input.master,
                &deck,
                &input.notes,
                &input.events,
                input.params,
                &input.setup,
                &input.play,
                &input.delta,
            )
            .unwrap();
            let mean = native_expected_score(&oracle);
            // These binary lotteries yield a small dyadic rational, exactly representable in binary64.
            assert!(mean.denominator.is_power_of_two() && mean.numerator < (1u128 << 53));
            let expected = mean.approximate();
            assert!(bounds.final_mean.lower <= expected && expected <= bounds.final_mean.upper);
            assert!(
                oracle
                    .scores
                    .keys()
                    .all(|&score| { bounds.final_support.lower <= score && score <= bounds.final_support.upper })
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 6 * 120);
    assert!(nonconstant > 0, "the mixed chart must keep a nonconstant native terminal score law");
}

#[test]
fn controller_family_still_requires_every_judgement_note() {
    let mut fixture = structural_fixture();
    let required = fixture.input.notes.iter().find(|note| is_judgement_note(note.note_operate_type)).unwrap().note_id;
    for frame in &mut fixture.input.play.frames {
        frame.judged.retain(|judgement| judgement.note_id != required);
    }
    let error = structural_context_error(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::Context);
    assert!(error.error.to_string().contains("unjudged judgement notes"));
}

#[test]
fn controller_family_declines_explicit_structural_results_instead_of_dropping_them() {
    for judgement in [5, 7] {
        let mut fixture = structural_fixture();
        let note = *fixture.input.notes.iter().find(|note| note.note_operate_type == 0).unwrap();
        let frame = fixture.input.play.frames.iter_mut().find(|frame| frame.time_ms >= note.time_ms).unwrap();
        assert!(frame.judged.is_empty());
        frame.judged.push(JudgedNote { note_id: note.note_id, judgement, judgement_time_ms: note.time_ms });
        let error = structural_context_error(&fixture);
        assert_eq!(error.reason, LuckFamilyDecline::Context);
        assert!(error.error.to_string().contains("declared structural note judgement"));
    }
}

#[test]
fn controller_family_structural_tail_keeps_full_clock_and_identity_guards() {
    let mut fixture = structural_fixture();
    fixture.input.params.music_length_ms = STRUCTURAL_TAIL;
    let error = structural_context_error(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::TerminalMapping);
    assert!(error.error.to_string().contains("finish clamp"));

    let mut fixture = structural_fixture();
    fixture.input.play.frames.retain(|frame| frame.time_ms < STRUCTURAL_TAIL);
    fixture.input.delta.truncate(fixture.input.play.frames.len());
    let error = structural_context_error(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::TerminalMapping);
    assert!(error.error.to_string().contains("complete empty tail"));

    let mut fixture = structural_fixture();
    let structural = *fixture.input.notes.iter().find(|note| note.note_operate_type == 0).unwrap();
    fixture.input.notes.push(structural);
    let error = structural_context_error(&fixture);
    assert_eq!(error.reason, LuckFamilyDecline::Context);
    assert!(error.error.to_string().contains("duplicate chart note identity"));
}
