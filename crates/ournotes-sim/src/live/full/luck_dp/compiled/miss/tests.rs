use super::*;
use crate::live::full::luck_dp::tests::fixture;
use crate::live::full::{LuckSource, luck_skills};

// These are arithmetic and native-interpreter regression fixtures, not catalogue benchmarks or
// owned decks. Published master requests are validated independently by the portable real-data run.
fn master() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let (mut master, notes, params, setup, play, deltas) = fixture(0, 60);
    let originals = master.gekisou_luck_bonus_lots.clone();
    for result in 1..=3 {
        master.gekisou_luck_bonus_lots.extend(originals.iter().cloned().map(|mut row| {
            row.id += result * 100;
            row.lot_result = result;
            row
        }));
    }
    master.skill_conditions.iter_mut().find(|row| row.id == 4011).unwrap().condition_values = vec![50];
    let source = master.gekisou_support_skills.iter().find(|row| row.id == 96).unwrap().clone();
    let effect = master.gekisou_support_skill_effects.iter().find(|row| row.skill_id == 96).unwrap().clone();
    master.gekisou_support_skill_effects.iter_mut().find(|row| row.skill_id == 96).unwrap().effect_value = 500;
    for (id, value) in [(97, 1000), (98, 1500), (99, 0)] {
        let mut row = effect.clone();
        row.id = id;
        row.skill_id = id;
        row.effect_value = value;
        master.gekisou_support_skill_effects.push(row);
        let mut skill = source.clone();
        skill.id = id;
        master.gekisou_support_skills.push(skill);
    }
    master.reindex().unwrap();
    (master, notes, params, setup, play, deltas)
}

fn decks() -> ([Performer; 2], [Performer; 2]) {
    // Minimum and StartGauge rows stay between the two originally ordered Miss sources. Moving
    // their integer-only operator to the end must preserve all skill flags and subsequent draws.
    let a = [
        Performer { gekisou_skill: Some((2, 1)), gekisou_support_skills: vec![(96, 1), (66, 1)], ..Default::default() },
        Performer { gekisou_skill: Some((1, 1)), gekisou_support_skills: vec![(97, 1), (31, 1)], ..Default::default() },
    ];
    let mut b = a.clone();
    b[0].gekisou_support_skills = vec![(66, 1)];
    b[1].gekisou_support_skills = vec![(98, 1), (31, 1)];
    (a, b)
}

fn assert_laws(a: &LuckDpCertifiedResult, b: &LuckDpCertifiedResult) {
    assert_eq!(a.probes, b.probes);
    assert_eq!(a.probe_transitions, b.probe_transitions);
    assert!(a.range_moments.is_empty() && b.range_moments.is_empty());
    assert!(a.steps.iter().any(|(_, joint)| joint[2].interval().upper() + joint[3].interval().upper() > 0.0));
    for time in a.steps.iter().chain(&b.steps).map(|step| step.0) {
        let left = a.steps[a.steps.partition_point(|step| step.0 <= time) - 1].1;
        let right = b.steps[b.steps.partition_point(|step| step.0 <= time) - 1].1;
        for (left, right) in left.into_iter().zip(right) {
            assert!(left.interval().intersect(right.interval()).is_some(), "time={time}, {left:?}, {right:?}");
        }
    }
}

#[test]
fn miss_gauge_native_floor_vectors_preserve_rounding_and_wrapped_lot_count() {
    let values = [500, 1000, 1500, 2000, 2500, 3000, 3500, 4000, 5000, 6000, 7000, 10000];
    assert_ne!(2 * delta(50, 500).unwrap(), delta(50, 1000).unwrap());
    assert_eq!(2 * delta(100, 500).unwrap(), delta(100, 1000).unwrap());
    for maximum in [50, 70, 100, 140] {
        for left in values {
            for right in values {
                let a = delta(maximum, left).unwrap();
                let b = delta(maximum, right).unwrap();
                for gauge in 0..=maximum {
                    let mut sequence = LuckScore::default();
                    sequence.gauge = gauge as i32;
                    sequence.lot_count = i32::MAX - 1;
                    sequence.gauge_max = maximum;
                    let mut summed = sequence.clone();
                    sequence.add_gauge(a as i32).unwrap();
                    sequence.add_gauge(b as i32).unwrap();
                    summed.add_gauge((a + b) as i32).unwrap();
                    assert_eq!(Chain::of(&sequence), Chain::of(&summed));
                }
            }
        }
    }
    assert!(delta(100, -1).is_none());
    assert!(delta(100, i64::MAX).is_none());
}

#[test]
fn miss_gauge_complete_native_identity_joins_source_counts_and_keeps_original_propagation() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let (a, b) = decks();
    let compile = |deck: &[Performer]| {
        compile_luck_program(&master, &skills, &notes, &[], params, &setup, &play, &deltas, deck, None, None, false)
            .unwrap()
    };
    let (original_a, original_b) = (compile(&a), compile(&b));
    assert_ne!(original_a.identity_words(), original_b.identity_words());
    let expected_a = original_a.certified().unwrap();
    let expected_b = original_b.certified().unwrap();
    let a = original_a.canonicalize_miss_gauge().unwrap();
    let b = original_b.canonicalize_miss_gauge().unwrap();
    assert_eq!(a.operator_contract(), "canonical-miss-gauge-deltas/1");
    assert_eq!(a.canonical_miss.as_ref().unwrap().maxima, [50, 70, 100, 140]);
    assert_eq!(a.identity_words(), b.identity_words());
    assert_eq!(expected_a.steps, a.certified().unwrap().steps);
    assert_eq!(expected_b.steps, b.certified().unwrap().steps);
    assert_laws(&expected_a, &expected_b);
    // The selected minimum is a separate field; its native slot moves when Miss multiplicity is
    // normalized, so a basis's private marker must use the same normalized action coordinates.
    let mut a = a.start_minimum_basis(64).unwrap();
    let mut b = b.start_minimum_basis(64).unwrap();
    assert_eq!(a.operator_contract(), "whole-live-start-minimum-basis+canonical-miss-gauge-deltas/1");
    assert_eq!(a.term_count(), b.term_count());
    let mut terms = Vec::new();
    for index in 0..a.term_count() {
        assert_eq!(a.term_identity_words(index).unwrap(), b.term_identity_words(index).unwrap());
        terms.push(a.certified_term(index).unwrap());
    }
    let terms = terms.iter().collect::<Vec<_>>();
    assert_laws(&expected_a, &a.reconstruct(&terms).unwrap());
    assert_laws(&expected_b, &b.reconstruct(&terms).unwrap());
}

#[test]
fn miss_gauge_zero_rows_keep_once_trigger_and_every_other_identity_input() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let (mut deck, _) = decks();
    deck[0].gekisou_support_skills = vec![(99, 1), (66, 1)];
    deck[1].gekisou_support_skills = vec![(31, 1)];
    let compile = |deck: &[Performer]| {
        compile_luck_program(&master, &skills, &notes, &[], params, &setup, &play, &deltas, deck, None, None, false)
            .unwrap()
    };
    let zero = compile(&deck).canonicalize_miss_gauge().unwrap();
    deck[0].gekisou_support_skills = vec![(66, 1)];
    let absent = compile(&deck).canonicalize_miss_gauge().unwrap();
    assert_ne!(zero.identity_words(), absent.identity_words());
    assert!(zero.transcript.miss_rows && !absent.transcript.miss_rows);
    let mut other = compile(&deck).canonicalize_miss_gauge().unwrap();
    other.transcript.hits[0].speed = f32::from_bits(other.transcript.hits[0].speed.to_bits() + 1);
    assert_ne!(absent.identity_words(), other.identity_words());
    // Probe/source geometry is still in the complete key, even when the gauge operator is empty.
    let mut other = compile(&deck).canonicalize_miss_gauge().unwrap();
    other.transcript.probes[0] = false;
    assert_ne!(absent.identity_words(), other.identity_words());
}

#[test]
fn miss_gauge_quotient_refuses_unknown_negative_overflow_and_other_quotient_domains() {
    let (master, notes, params, setup, play, deltas) = master();
    let skills = luck_skills(&master).unwrap();
    let (deck, _) = decks();
    let compile = |moments| {
        compile_luck_program(&master, &skills, &notes, &[], params, &setup, &play, &deltas, &deck, None, None, moments)
            .unwrap()
    };
    assert!(matches!(compile(true).canonicalize_miss_gauge(), Err(Error::Unsupported(_))));
    assert!(matches!(
        compile(false).canonicalize_start_minimum().canonicalize_miss_gauge(),
        Err(Error::Unsupported(_))
    ));
    for value in [-1, i64::MAX] {
        let mut program = compile(false);
        *program.transcript.actions.iter_mut().find(|action| matches!(action, Action::MissGauge { .. })).unwrap() =
            Action::MissGauge { value };
        assert!(matches!(program.canonicalize_miss_gauge(), Err(Error::Unsupported(_))));
    }
    for speed in [-2.0, f32::MAX] {
        let mut program = compile(false);
        program.transcript.hits[0].speed = speed;
        assert!(matches!(program.canonicalize_miss_gauge(), Err(Error::Unsupported(_))));
    }
    let mut program = compile(false);
    program.transcript.templates[0].gauge = -1;
    assert!(matches!(program.canonicalize_miss_gauge(), Err(Error::Unsupported(_))));
    let mut program = compile(false);
    program.transcript.templates[0].gauge_max = i64::from(i32::MAX) - 1;
    for action in &mut program.transcript.actions {
        match action {
            Action::StartGauge { value, .. } => *value = 0,
            Action::MissGauge { value } => *value = 1,
            _ => {}
        }
    }
    let error = program.canonicalize_miss_gauge().unwrap_err();
    assert!(error.to_string().contains("all-enabled frame gauge sum"));
    let mut program = compile(false);
    program.transcript.frames.iter_mut().find(|frame| frame.start.is_some()).unwrap().start = None;
    assert!(matches!(program.canonicalize_miss_gauge(), Err(Error::Unsupported(_))));
    assert!(skills.rows.contains_key(&(LuckSource::GekisouSupport, 31)));
}
