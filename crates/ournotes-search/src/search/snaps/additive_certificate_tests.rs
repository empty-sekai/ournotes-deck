//! Native arithmetic checks for the additive factor-drift certificate.

use super::*;
use ournotes_sim::live::score::{JUST, LiveScoreCalculator, PERFECT};
use ournotes_sim::live::skill::{FactorCommand, apply_factor};

#[test]
fn additive_large_counts_cover_native_replay_and_budgeted_just() {
    let cycles = 20_000usize;
    let repeats = 2usize;
    // Two start and two finish commands per pulse, in distinct frames. Each
    // frame is undone once and replayed. Every command touches only one field.
    let commands = (4 * cycles) as f64;
    let executions = commands * repeats as f64;
    let roundings = factor_roundings(executions, commands);
    // Exact integer-mill prefixes never exceed 1_000_001 mill above the initial
    // factor, including partial frame differences. 11 bounds that norm.
    let norm = 11.0;
    let drift = factor_drift(executions, commands, norm).unwrap();
    let delta = float_margin::with_chain(drift, GK_CHAIN_EPS).unwrap();
    assert!((roundings * 2f64.powi(-24)).next_up() > 1.0 / 256.0);
    assert!(float_margin::amplification(roundings, 2f64.powi(-24)).unwrap() > 1.01);
    assert!(delta >= drift && delta < 0.5);

    let settings = LiveScoreSettings {
        score_adjustment_factor: 1.0,
        life_onus_factor: 1.0,
        note_factor_percent: HashMap::from([(1, 100)]),
        judgement_score_factor_percent: HashMap::from([(JUST, 150), (PERFECT, 100)]),
    };
    // Perfect is the baseline judgement; a budgeted conversion may produce
    // Just. The absolute sensitivity must include its larger percentage.
    // These coefficients also include the native 150% LUCK and post-floor assist.
    let coef = Coef { k: vec![1.5], z: vec![1.125], max_jp: vec![1.0], ..Default::default() };
    let coefficient = (coef.k[0] * coef.z[0]).next_up();
    let sensitivity = factor_error_sensitivity(&coef, 1.5).unwrap();
    let ideal_global = (coefficient * (1.5 * (1.0 + norm)).next_up()).next_up();
    let (base, global, chain) =
        additive_joint_envelope(coefficient, ideal_global, delta, roundings, sensitivity, GK_CHAIN_EPS)
            .expect("finite amplified drift below the positive-factor limit");

    let observe = |calc: &LiveScoreCalculator, reference: [i64; 2]| {
        // The reference accumulates and undoes exact integer mills, independently
        // of binary32 application and of the frame's binary32 difference sum.
        let error = (calc.state.note_score_up as f64 - reference[0] as f64 / 100000.0).abs()
            + (calc.state.just as f64 - reference[1] as f64 / 100000.0).abs();
        assert!(error <= drift, "factor error {error} exceeds {drift}");
        let scores = [PERFECT, JUST].map(|score_type| {
            let integer_factor = reference[0] + if score_type == JUST { reference[1] } else { 0 };
            assert!((100_000..=1_100_001).contains(&integer_factor));
            let ideal_factor = (integer_factor as f64 / 100000.0).next_up();
            let judgement = if score_type == JUST { 1.5 } else { 1.0 };
            let gain = (coefficient * ((ideal_factor * judgement).next_up() - 1.0).next_up()).next_up();
            let cap = ub(i64::from(calc.state.band_total_power), (base + gain).next_up().min(global), chain);
            let actual = i64::from(calc.note_score(0, 1000, 0, 1, score_type, None).unwrap());
            assert!(calc.score_up_and_luck(score_type, 0).0 > 0.5);
            assert!(cap >= actual, "cap {cap} misses native score {actual}");
            actual
        });
        assert!(scores[1] > scores[0], "the budgeted conversion changes the native payoff");
        error
    };

    for power in [100_003, 1_000_000] {
        let mut calc = LiveScoreCalculator::new(power, 5, 1, &settings, 1.0, 1.125, None);
        calc.state.added_luck_bonus = 50;
        let mut reference = [100_000i64, 0i64];
        let mut applied = 0usize;
        let mut undos = 0usize;
        let mut maximum_error = observe(&calc, reference);
        for cycle in 0..cycles {
            // Alternating fields also exercises the final note-plus-Just f32 add.
            let field = cycle % 2;
            for mills in [[1_000_000, 1], [-1_000_000, -1]] {
                let mut difference = 0f32;
                let mut integer_difference = 0i64;
                for replay in 0..repeats {
                    if replay != 0 {
                        if field == 0 {
                            calc.state.note_score_up -= difference;
                        } else {
                            calc.state.just -= difference;
                        }
                        reference[field] -= integer_difference;
                        difference = 0.0;
                        integer_difference = 0;
                        undos += 1;
                        maximum_error = maximum_error.max(observe(&calc, reference));
                    }
                    for mill in mills {
                        let command = if field == 0 {
                            FactorCommand { note_mill: mill, ..Default::default() }
                        } else {
                            FactorCommand { judgement: 6, judge_mill: mill, ..Default::default() }
                        };
                        apply_factor(&mut calc.state, &command);
                        difference += mill as f32 / 100000f32;
                        integer_difference += i64::from(mill);
                        reference[field] += i64::from(mill);
                        applied += 1;
                        maximum_error = maximum_error.max(observe(&calc, reference));
                    }
                }
            }
            assert_eq!(reference, [100_000, 0]);
        }
        assert_eq!(applied as f64, executions);
        assert_eq!(undos, 2 * cycles * (repeats - 1));
        assert!(maximum_error > 0.0, "signed frame cancellation must exercise real rounding");
    }

    // A finite feedback certificate alone is insufficient: more replays can
    // lose the positive-factor margin, which must still decline this envelope.
    let larger_executions = 200_000.0;
    let larger_drift = factor_drift(larger_executions, commands, norm).unwrap();
    let larger_delta = float_margin::with_chain(larger_drift, GK_CHAIN_EPS).unwrap();
    assert!(larger_delta >= 0.5);
    assert!(
        additive_joint_envelope(
            coefficient,
            ideal_global,
            larger_delta,
            factor_roundings(larger_executions, commands),
            sensitivity,
            GK_CHAIN_EPS,
        )
        .is_none()
    );
}

#[test]
fn additive_small_counts_keep_the_same_float_envelope() {
    // Fixed bit patterns of the outward envelope in the small-count domain.
    // This delta bounds the fully amplified drift and chain for every row below.
    let delta = 2f64.powi(-10);
    for (extra, expected_chain) in [(0.0, 0x3ed1_637d_1e78_0001), (GK_CHAIN_EPS, 0x3ed3_637d_a994_0001)] {
        for commands in [0.0, 1.0, 20.0, 200.0] {
            let roundings = factor_roundings(commands, commands);
            let drift = factor_drift(commands, commands, 11.0).unwrap();
            assert!(float_margin::with_chain(drift, extra).unwrap() <= delta);
            assert!((roundings * 2f64.powi(-24)).next_up() <= 1.0 / 256.0);
            let (base, global, chain) = additive_joint_envelope(1.0, 2.0, delta, roundings, 1.5, extra).unwrap();
            assert_eq!(
                [base.to_bits(), global.to_bits(), chain.to_bits()],
                [0x3ff0_0600_0000_0001, 0x4000_0300_0000_0001, expected_chain]
            );
        }
    }
}
