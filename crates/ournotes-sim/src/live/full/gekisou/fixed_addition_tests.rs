use super::*;
fn controller() -> Controller {
    Controller::new(vec![(0, 1000, 1), (1100, 2000, 3)], std::iter::empty(), &Master::default(), 100, 100, 0, 0)
        .unwrap()
}

#[test]
fn factor_sums_are_exact_within_2_pow_24_units_of_one_power_of_two() {
    let mut c = controller();
    assert!(c.factor_sums_exact());
    let one = c.add_gauge_up(100, 1.0);
    c.add_gauge_up(100, 2.0);
    c.subtract_gauge_up(500, one).unwrap();
    // One factor of any value is exact on its own.
    c.add_lot_probability_up(100, 0.1);
    assert!(c.factor_sums_exact());
    // 0.1 and 0.2 are 13421773 units of 2^-27 and 2^-26: together more than 2^24 units of 2^-27.
    c.add_lot_probability_up(100, 0.2);
    assert!(!c.factor_sums_exact());
    let mut c = controller();
    c.add_gauge_up(100, 1.0);
    c.add_gauge_up(100, f32::powi(2.0, -23));
    assert!(c.factor_sums_exact());
    // 1 + 2^-23 + 2^-24 needs 25 significant bits.
    c.add_gauge_up(100, f32::powi(2.0, -24));
    assert!(!c.factor_sums_exact());
}

#[test]
fn fixed_additions_capture_range_and_feed_inflated_not_raw_just() {
    let mut c = controller();
    c.playing.push(0);
    c.history[0] = vec![(100, J_JUST, 0), (200, J_JUST, 1)];
    c.history[1] = vec![(1200, J_JUST, 2)];
    c.add_just_count(150, 5);
    c.add_gekisou_combo(150, 7);
    c.recalculate();
    assert_eq!((c.states[0].just, c.states[0].raw_just, c.states[0].cum_base, c.states[0].combo), (7, 2, 7, 9));
    assert_eq!((c.states[1].just, c.states[1].combo), (1, 1));
    assert_eq!(c.snapshot[0], [1, 9]);
}

#[test]
fn trailing_commands_are_visible_but_not_repeated_in_resume_or_score_snapshot() {
    let mut c = controller();
    c.playing.push(0);
    c.history[0] = vec![(100, J_JUST, 0)];
    c.add_just_count(150, 4);
    c.add_gekisou_combo(150, 6);
    c.recalculate();
    assert_eq!((c.states[0].just, c.states[0].combo), (5, 7));
    assert_eq!(c.snapshot[0], [1]);
    c.recalculate();
    assert_eq!((c.states[0].just, c.states[0].combo), (5, 7));
    c.history[0].push((200, J_JUST, 1));
    c.recalculate();
    assert_eq!((c.states[0].just, c.states[0].combo), (6, 8));
    assert_eq!(c.snapshot[0], [1, 8]);
}

#[test]
fn outside_range_is_wildcard_and_empty_history_ignores_command_until_note_exists() {
    let mut c = controller();
    c.add_just_count(0, 3);
    c.add_gekisou_combo(0, 4);
    c.recalculate();
    assert_eq!(c.states[0].just, 0);
    c.history[0].push((100, J_JUST, 0));
    c.history[1].push((1200, J_JUST, 1));
    c.recalculate();
    for s in &c.states {
        assert_eq!((s.just, s.combo), (4, 5));
    }
}

#[test]
fn fixed_command_uses_binary32_delta_and_cumulative_base_without_changing_last_just() {
    let mut c = controller();
    c.playing.push(0);
    c.history[0].push((100, J_JUST, 0));
    c.add_just_count(100, 16_777_217);
    c.recalculate();
    assert_eq!(c.states[0].just, 16_777_217); // f32 command is 16_777_216, then the note adds one
    assert_eq!(c.states[0].raw_just, 1);
    assert_eq!(c.states[0].last_just_ms, -1);
}

#[test]
fn bonus_changes_filed_together_fold_alike_unless_a_judgement_falls_between_their_times() {
    // Two combo bonus changes filed after the first judgement, at different times, in either filing order.
    let recount = |judgements: &[i32], first: (i32, f32), second: (i32, f32)| {
        let mut c = controller();
        c.playing.push(0);
        c.history[0] = vec![(100, J_JUST, 0)];
        c.seq = 1;
        c.add_combo_bonus(first.0, first.1);
        c.add_combo_bonus(second.0, second.1);
        for (&time, sequence) in judgements.iter().zip(1..) {
            c.history[0].push((time, J_JUST, sequence));
        }
        c.recalculate();
        (c.bonus_changes_fold_alike(), c.states[0].combo)
    };
    let (early, late) = ((150, 1.0), (300, 2.0));
    // No judgement between 150 and 300: either order takes both at 400.
    assert_eq!(recount(&[400], early, late), (true, 5));
    assert_eq!(recount(&[400], late, early), (true, 5));
    // A judgement at 200 takes the early change only when it was filed first.
    assert_eq!(recount(&[200, 400], early, late), (false, 7));
    assert_eq!(recount(&[200, 400], late, early), (false, 6));
}
