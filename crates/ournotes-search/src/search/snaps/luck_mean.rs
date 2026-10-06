//! Expected-score caps from terminal joint Rush/probe probabilities and conditional native note envelopes.
use super::*;
use ournotes_sim::live::{certified::F64Interval, full::LuckTerminalRush};

type RushNoteCaps = Vec<(i32, [i64; 4])>;

impl JointFineBounds {
    /// A Score-only upper endpoint. It is not a per-path score support and cannot replace a payoff law.
    pub(crate) fn rush_mean_upper(
        &self,
        power: i64,
        members: [usize; 5],
        choices: [usize; 5],
        positions: &[usize; 5],
        scratch: &mut JointScratch,
        terminal: &LuckTerminalRush,
    ) -> Option<f64> {
        let (total, terms) = self.rush_cap_terms_with_trace(power, members, choices, positions, scratch, terminal)?;
        let (caps, remainder) = split_cap(total, &terms)?;
        let note_upper = terminal.weighted_note_bucket_upper(&caps)?;
        let upper = F64Interval::integer(remainder).add(F64Interval::point(note_upper).ok()?).upper();
        // The original pathwise cap also bounds its expectation, including probability-rounding slack.
        upper.is_finite().then_some(upper.min(total as f64))
    }
}

/// Preserve the old unconditional rank/conversion remainder, including its original roundoff inflation.
/// If `N = sum_e z_e`, the ranked cap is `ceil((N + sum_e z_e*(r_e-1) + conv) * (1+eps))`.
/// Without ranks it is `N + floor(conv)` (integer gains). The exact integer difference `C-N` retains the
/// original rank/conversion allowance, including the ranked case's roundoff inflation and final ceiling.
/// Notes with an eligible budget target outside the nonbudget judgement mask retain the old cap in all four
/// cells. Covered targets add no judgement alternative in any cell, so `conv` needs no new floor-difference or
/// stochastic allocation argument. External snapshot differences keep the previous complete scorer.
fn split_cap(total: i64, terms: &CapTerms) -> Option<(RushNoteCaps, i128)> {
    if !(0..=i64::from(i32::MAX)).contains(&total)
        || terms.network_ranking
        || !terms.conv.is_finite()
        || terms.conv < 0.0
    {
        return None;
    }
    let off = terms.rush_off.as_ref()?;
    if off.len() != terms.entries.len() {
        return None;
    }
    if terms.probe_off.as_ref().is_some_and(|probe| probe.len() != terms.entries.len())
        || terms.native_note_caps.as_ref().is_some_and(|caps| caps.len() != terms.entries.len())
        || terms.native_actual_caps.as_ref().is_some_and(|caps| caps.len() != terms.entries.len())
    {
        return None;
    }
    let integer = |value: f64| {
        (value.is_finite() && (0.0..=f64::from(i32::MAX)).contains(&value) && value.fract() == 0.0)
            .then_some(value as i64)
    };
    let mut notes = 0i128;
    let mut caps = Vec::with_capacity(off.len());
    for (index, (&(time, on, rank), &off)) in terms.entries.iter().zip(off).enumerate() {
        if !rank.is_finite() || rank < 1.0 {
            return None;
        }
        let (off, on) = (integer(off)?, integer(on)?);
        if off > on {
            return None;
        }
        notes = notes.checked_add(i128::from(on))?;
        let [probe_plain, probe_rush] = terms.probe_off.as_ref().map_or([off as f64, on as f64], |probe| probe[index]);
        let (probe_plain, probe_rush) = (integer(probe_plain)?, integer(probe_rush)?);
        if probe_plain > on || probe_rush > on {
            return None;
        }
        let mut buckets = [probe_plain, off, probe_rush, on];
        if let Some(native) = &terms.native_note_caps {
            for bucket in 0..4 {
                buckets[bucket] = buckets[bucket].min(integer(native[index][bucket])?);
            }
        }
        if let Some(native) = &terms.native_actual_caps {
            for bucket in 0..4 {
                let upper = native[index][bucket];
                if upper < 0 {
                    return None;
                }
                // This cap contains the actual converted terminal note T, unlike the field-only vmask
                // cap above. If T > old z, min(z, upper) = z; otherwise both terms are at least T.
                // Thus only the old positive conversion excess can remain, already paid by R=C-N.
                buckets[bucket] = buckets[bucket].min(i64::from(upper));
            }
        }
        caps.push((time, buckets));
    }
    let remainder = i128::from(total).checked_sub(notes)?;
    (remainder >= 0).then_some((caps, remainder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_and_conversion_remainder_keeps_original_integer_allowance() {
        let terms = CapTerms {
            entries: vec![(100, 10.0, 1.25), (200, 20.0, 1.5)],
            rush_off: Some(vec![9.0, 18.0]),
            conv: 3.0,
            ranked: true,
            ..Default::default()
        };
        // N=30; rank extra=12.5; conversion=3; old roundoff/ceiling gives C=46.
        let (caps, remainder) = split_cap(46, &terms).unwrap();
        assert_eq!(caps, [(100, [9, 9, 10, 10]), (200, [18, 18, 20, 20])]);
        assert_eq!(remainder, 16);
        assert!(remainder as f64 >= 12.5 + 3.0);
    }

    #[test]
    fn incomplete_noninteger_overflow_and_network_caps_do_not_become_mean_bounds() {
        let base = CapTerms { entries: vec![(100, 10.0, 1.0)], rush_off: Some(vec![9.0]), ..Default::default() };
        assert!(split_cap(10, &base).is_some());
        assert!(split_cap(9, &base).is_none());
        assert!(split_cap(i64::MAX, &base).is_none());
        for modified in [
            CapTerms { rush_off: None, ..base.clone() },
            CapTerms { rush_off: Some(vec![]), ..base.clone() },
            CapTerms { rush_off: Some(vec![9.5]), ..base.clone() },
            CapTerms { rush_off: Some(vec![11.0]), ..base.clone() },
            CapTerms { conv: f64::NAN, ..base.clone() },
            CapTerms { network_ranking: true, ..base },
        ] {
            assert!(split_cap(10, &modified).is_none());
        }
    }

    #[test]
    fn conversion_floor_gain_cannot_be_scaled_by_rush_probability() {
        // This is why an uncovered budget target keeps its old cap even when its all-on gain is zero.
        let native_floor = |value: f32, rush: f32| (value * rush).floor() as i32;
        let gain = |rush| native_floor(1.0, rush) - native_floor(0.99, rush);
        assert_eq!(gain(1.1), 0);
        assert_eq!(gain(1.0), 1);
    }

    fn conversion_bound() -> JointFineBounds {
        use ournotes_sim::live::score::{JUST, LiveScoreCalculator, PERFECT};
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: 1,
            music_level: 5,
            converted_note_count: 1,
            music_length_ms: 1000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let settings = LiveScoreSettings {
            score_adjustment_factor: 1.0,
            life_onus_factor: 1.0,
            note_factor_percent: HashMap::from([(1, 100)]),
            judgement_score_factor_percent: HashMap::from([(PERFECT, 190), (JUST, 205)]),
        };
        let calc = LiveScoreCalculator::new(1, 5, 1, &settings, 1.0, 1.0, None);
        let native = |judgement, probe, rush| calc.note_score_core(1000, 1, judgement, 1.0, probe, rush).unwrap();
        assert_eq!((native(PERFECT, 1.0, 1.1), native(JUST, 1.0, 1.1)), (2, 2));
        assert_eq!((native(PERFECT, 1.0, 1.0), native(JUST, 1.0, 1.0)), (1, 2));
        assert_eq!((native(PERFECT, 1.1, 1.1), native(JUST, 1.1, 1.1)), (2, 2));
        let mut fine = Fine {
            score_frames: ScoreFrames::new(&params),
            rush_eligible: true,
            raw: vec![5, 5],
            group: vec![0, 1],
            pre: vec![1.1, 1.1],
            pre_plain: vec![1.0, 1.0],
            cnc: 1.0,
            combo_max: vec![1.0; 2],
            mjp: vec![0.0; 128],
            jp4: vec![[0.0; 4]; 128],
            breaks: vec![false; 128],
            src: vec![vec![1], vec![0], vec![0], vec![0], vec![0]],
            extra: vec![std::array::from_fn(|_| vec![0; 2]), std::array::from_fn(|_| vec![1 << 6, 0])],
            extra_v: vec![std::array::from_fn(|_| vec![0; 2]); 2],
            budget: vec![vec![], vec![(6, 1.0, vec![0])]],
            gcombo: None,
            dead: vec![false; 2],
            rank: vec![1.0; 2],
            rank_ranges: vec![],
            network_ranking: false,
            nobreak: vec![false; 2],
            z_dead: 1.0,
            life: vec![vec![LifeKind::None]; 5],
            base: 1000,
            #[cfg(feature = "search-diagnostics")]
            exec_profile: vec![],
            slot_end: vec![],
            slot_dmg: vec![],
            slot_dmg_final: vec![],
            life_rows_listed: true,
            ev_slot: Default::default(),
            until: vec![i64::MAX; 2],
            until_min: vec![i64::MAX; 2],
            dead_from: 2,
        };
        for mask in 0..128 {
            if mask & (1 << 5) != 0 {
                fine.mjp[mask] = 1.9;
            }
            if mask & (1 << 6) != 0 {
                fine.mjp[mask] = 2.05;
            }
        }
        JointFineBounds {
            coef: Coef { times: vec![100, 200], z: vec![1.0; 2], ..Default::default() },
            fine,
            chain_extra: 0.0,
            contrib: vec![vec![std::array::from_fn(|_| Contrib::default())]; 5],
            class_of: vec![vec![]; 5],
            terminal_caps_admitted: true,
            raw: None,
        }
    }

    #[test]
    fn zero_on_rush_conversion_gain_still_keeps_the_eligible_note_unweighted() {
        let mut bound = conversion_bound();
        let mut scratch = JointScratch::default();
        let mut caps = |bound: &JointFineBounds| {
            bound.rush_cap_terms(1, [0, 1, 2, 3, 4], [0; 5], &[0, 1, 2, 3, 4], &mut scratch, None).unwrap()
        };
        let (old_cap, guarded) = caps(&bound);
        assert_eq!(guarded.entries.iter().map(|entry| entry.1).collect::<Vec<_>>(), [2.0, 2.0]);
        assert_eq!(guarded.conv, 0.0);
        assert_eq!(guarded.rows, [(1.0, vec![])]);
        // The first entry is eligible even though the all-Rush conversion gain is zero. The other entry
        // can still use the smaller off cap. Moving the guard under `d > 0` would return [1,1] and fail here.
        assert_eq!(guarded.rush_off.as_deref(), Some([2.0, 1.0].as_slice()));
        assert!(split_cap(old_cap, &guarded).is_some());

        // An already-reachable target adds no gain in either class; these small native floors still coincide.
        bound.fine.extra_v[1][0][0] = 1 << 6;
        let (_, reachable) = caps(&bound);
        assert_eq!(reachable.conv, 0.0);
        assert_eq!(reachable.rush_off.as_deref(), Some([2.0, 1.0].as_slice()));

        // Without the budget row, the base note is safe to tighten and the same implementation does so.
        bound.fine.extra_v[1][0][0] = 0;
        bound.fine.budget[1].clear();
        let (_, ordinary) = caps(&bound);
        assert_eq!(ordinary.rush_off.as_deref(), Some([1.0, 1.0].as_slice()));
    }

    fn add_probe(bound: &mut JointFineBounds, value: f64) {
        let part = &mut bound.contrib[0][0][0];
        part.windows.push(Window { lo: 0, hi: 2, note: value, judge: [0.0; 4], ramp: 0, rush: 1 });
        part.rush.push(rush::test_probe(value));
        part.spans.push((0, 300, value));
    }

    fn four_caps(bound: &JointFineBounds, gate: Option<i64>) -> (i64, CapTerms, RushNoteCaps) {
        let mut scratch = JointScratch::default();
        let (total, terms) =
            bound.rush_cap_terms(1, [0, 1, 2, 3, 4], [0; 5], &[0, 1, 2, 3, 4], &mut scratch, gate).unwrap();
        let (caps, _) = split_cap(total, &terms).unwrap();
        (total, terms, caps)
    }

    #[test]
    fn conversion_budget_keeps_all_four_caps_when_an_uncovered_target_has_zero_gain() {
        let mut bound = conversion_bound();
        add_probe(&mut bound, 0.1);
        let (old, ordinary, _) = four_caps(&bound, None);
        let (total, terms, caps) = four_caps(&bound, Some(MISSION_LUCK));
        assert_eq!((total, &terms.entries, terms.conv), (old, &ordinary.entries, ordinary.conv));
        assert_eq!(terms.conv, 0.0);
        assert_eq!(caps, [(100, [2; 4]), (200, [1, 2, 2, 2])]);
        // With both amplitudes absent the native PERFECT/Just floors are 1 and 2. The old all-on
        // gain is zero, so moving any of these four guards under `gain > 0` loses a possible point.
        assert_eq!(terms.rows, [(1.0, vec![])]);
        bound.fine.budget[1].push((6, 1.0, vec![0]));
        let (_, duplicated, caps) = four_caps(&bound, Some(MISSION_LUCK));
        assert_eq!(caps[0].1, [2; 4]);
        assert_eq!(duplicated.rows.len(), 2);
        bound.fine.extra_v[1][0][0] = 1 << 6;
        let (_, reachable, caps) = four_caps(&bound, Some(MISSION_LUCK));
        assert_eq!(reachable.conv, 0.0);
        assert_eq!(caps[0].1, [2; 4]);
        bound.fine.extra_v[1][0][0] = 0;
        bound.fine.budget[1].clear();
        assert_eq!(four_caps(&bound, Some(MISSION_LUCK)).2, [(100, [1, 2, 2, 2]), (200, [1, 2, 2, 2])]);
    }

    #[test]
    fn budget_targets_already_in_vmask_keep_conditional_judgement_caps() {
        let mut bound = conversion_bound();
        add_probe(&mut bound, 0.5);
        bound.fine.extra_v[1][0][0] = 1 << 6;
        let rows = std::mem::take(&mut bound.fine.budget[1]);
        let (old, expected_terms, expected_caps) = four_caps(&bound, Some(MISSION_LUCK));
        assert!(expected_caps[0].1[0] < expected_caps[0].1[3]);
        for rows in [rows.clone(), [rows.clone(), rows].concat()] {
            bound.fine.budget[1] = rows;
            let (total, terms, caps) = four_caps(&bound, Some(MISSION_LUCK));
            assert_eq!(caps, expected_caps);
            assert_eq!(total, old);
            assert_eq!(terms.entries, expected_terms.entries);
            assert_eq!(terms.conv, 0.0);
            assert!(terms.rows.iter().all(|(_, gains)| gains.is_empty()));
        }
    }

    #[test]
    fn any_uncovered_budget_target_keeps_all_caps_regardless_of_row_order() {
        let mut bound = conversion_bound();
        add_probe(&mut bound, 0.5);
        // Another performer's nonbudget source covers Just. Membership uses the complete candidate's vmask,
        // not only this budget row's source or the raw Perfect judgement.
        let extra = std::array::from_fn(|position| if position == 1 { vec![1 << 6, 0] } else { vec![0; 2] });
        bound.fine.extra.push(extra.clone());
        bound.fine.extra_v.push(extra);
        bound.fine.budget.push(Vec::new());
        bound.fine.src[1] = vec![2];
        let inside = (6, 1.0, vec![0]);
        let outside = (4, 1.0, vec![0]);
        let (_, _, covered_caps) = four_caps(&bound, Some(MISSION_LUCK));
        assert!(covered_caps[0].1[0] < covered_caps[0].1[3]);
        bound.fine.extra[1][0][0] |= 1 << 4;
        for rows in [
            vec![inside.clone(), outside.clone()],
            vec![outside.clone(), inside.clone()],
            vec![inside.clone(), outside.clone(), inside.clone()],
            vec![outside.clone(), inside.clone(), outside],
        ] {
            bound.fine.budget[1] = rows;
            let (_, terms, caps) = four_caps(&bound, Some(MISSION_LUCK));
            assert_eq!(caps[0].1, [terms.entries[0].1 as i64; 4]);
            assert_eq!(caps[1], covered_caps[1]);
            // Great adds no all-on gain to a mask that already contains Just, but its absent bit still requires
            // the conservative guard. A covered row encountered later cannot undo that whole-note decision.
            assert_eq!(terms.conv, 0.0);
            assert!(terms.rows.iter().all(|(_, gains)| gains.is_empty()));
        }
    }

    #[test]
    fn probe_off_keeps_unmatched_windows_and_the_complete_history_margin() {
        let mut bound = conversion_bound();
        bound.fine.budget[1].clear();
        add_probe(&mut bound, 0.5);
        let part = &mut bound.contrib[0][0][0];
        part.windows.push(Window { lo: 0, hi: 2, note: 0.4, judge: [0.0; 4], ramp: 0, rush: 0 });
        part.windows.push(Window { lo: 0, hi: 2, note: 0.3, judge: [0.0; 4], ramp: 0, rush: 2 });
        let mut unknown = rush::test_probe(0.3);
        unknown.effect_type = 2004;
        part.rush.push(unknown);
        part.ops_plain = 40.0;
        part.cmds_plain = 2.0;
        part.spans.extend([(0, 300, 0.4), (0, 300, 0.3)]);
        let view = FineView { coef: &bound.coef, fine: &bound.fine, chain_extra: bound.chain_extra };
        let parts = std::array::from_fn(|i| &bound.contrib[i][0][i]);
        let original_drift = view.cand_drift(parts, None);
        assert!(original_drift > 0.0 && original_drift.is_finite());
        let (old, ordinary, old_caps) = four_caps(&bound, None);
        let (total, conditional, caps) = four_caps(&bound, Some(MISSION_LUCK));
        assert_eq!((total, &conditional.entries, conditional.conv), (old, &ordinary.entries, ordinary.conv));
        assert!(caps.iter().zip(&old_caps).all(|(cap, old)| cap.1[0] < old.1[0] && cap.1[2] < old.1[2]));

        // Independently remove only that ideal window from an otherwise unchanged fine envelope.
        // Its RushRef, executions and spans remain, so the old complete history allowance is still read.
        let matched = bound.contrib[0][0][0].windows.remove(0);
        let view = FineView { coef: &bound.coef, fine: &bound.fine, chain_extra: bound.chain_extra };
        let parts = std::array::from_fn(|i| &bound.contrib[i][0][i]);
        assert_eq!(view.cand_drift(parts, None), original_drift);
        let (_, retained, _) = four_caps(&bound, None);
        for (index, (_, cap)) in caps.iter().enumerate() {
            assert_eq!(cap[0] as f64, retained.rush_off.as_ref().unwrap()[index]);
            assert_eq!(cap[2] as f64, retained.entries[index].1);
        }
        bound.contrib[0][0][0].windows.insert(0, matched);
        // Without authority, or with a ref whose shape is not the admitted direct note probe, no ideal
        // amplitude may disappear. A later expansion of native admission must keep these fallbacks.
        for gate in [None, Some(1), Some(3), Some(4)] {
            assert_eq!(four_caps(&bound, gate).2, old_caps);
        }
        bound.contrib[0][0][0].rush[0].effect_type = 2004;
        assert_eq!(four_caps(&bound, Some(MISSION_LUCK)).2, old_caps);
        bound.contrib[0][0][0].rush[0].effect_type = 2000;
        bound.contrib[0][0][0].windows[0].ramp = 1;
        assert_eq!(four_caps(&bound, Some(MISSION_LUCK)).2, old_caps);
    }

    #[test]
    fn mask_replacement_cannot_reintroduce_a_matched_probe() {
        let mut bound = conversion_bound();
        bound.fine.budget[1].clear();
        add_probe(&mut bound, 1.0);
        let masks = Rc::new(ournotes_sim::live::full::RushMasks {
            flags: std::array::from_fn(|_| vec![true; 11]),
            max_runs: [1; 4],
            spans: None,
        });
        for kind in [2000, 2004] {
            bound.contrib[0][0][0].rush[0].effect_type = kind;
            let parts = std::array::from_fn(|index| &bound.contrib[index][0][index]);
            let view = FineView { coef: &bound.coef, fine: &bound.fine, chain_extra: bound.chain_extra };
            let mut scratch = Scratch {
                terms: Some(CapTerms {
                    rush_off: Some(Vec::new()),
                    probe_off: Some(Vec::new()),
                    probe_gate: Some(MISSION_LUCK),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let total = view.fine_bound(1, parts, [1, 0, 0, 0, 0], CandLife::Unknown, &mut scratch, Some(&masks));
            assert_eq!(scratch.rush_cache.usage.lookups, 1, "the replacement path must actually run");
            let (caps, _) = split_cap(total, &scratch.terms.take().unwrap()).unwrap();
            let expected = if kind == 2000 { [1, 3, 2, 4] } else { [3, 3, 4, 4] };
            assert_eq!(caps, [(100, expected), (200, expected)]);
        }
    }

    #[test]
    fn four_bucket_assembly_rejects_incomplete_and_unbounded_conditional_terms() {
        let base = CapTerms {
            entries: vec![(100, 10.0, 1.25)],
            rush_off: Some(vec![9.0]),
            probe_off: Some(vec![[4.0, 5.0]]),
            ..Default::default()
        };
        assert_eq!(split_cap(13, &base).unwrap(), (vec![(100, [4, 9, 5, 10])], 3));
        for conditional in [vec![], vec![[f64::NAN, 5.0]], vec![[-1.0, 5.0]], vec![[4.0, 10.5]], vec![[11.0, 5.0]]] {
            assert!(split_cap(13, &CapTerms { probe_off: Some(conditional), ..base.clone() }).is_none());
        }
        let mut bound = conversion_bound();
        bound.terminal_caps_admitted = false;
        assert!(!bound.supports_rush_mean_upper());
        assert!(
            bound
                .rush_cap_terms(
                    1,
                    [0, 1, 2, 3, 4],
                    [0; 5],
                    &[0, 1, 2, 3, 4],
                    &mut JointScratch::default(),
                    Some(MISSION_LUCK)
                )
                .is_none()
        );
    }
}
