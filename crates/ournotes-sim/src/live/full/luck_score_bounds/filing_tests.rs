use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn filing_case(gap: i32) -> ProgramCase {
    let mut input = ProgramCase::new(fixture());
    for row in &mut input.master.live_settings {
        if matches!(row.key.as_str(), "gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush") {
            row.value = "10".into();
        }
    }
    input.master.gekisou_luck_base_points[0].base_point = 10;
    input.master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            [0, 3].map(move |result| crate::master::LuckBonusLotRow {
                id: kind * 4 + result + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: 1,
            })
        })
        .collect();
    input.master.gekisou_skills.push(crate::master::SkillRow {
        id: 901,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    input.master.gekisou_support_skills.push(crate::master::SkillRow {
        id: 902,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 7021,
        condition_type: 7021,
        condition_values: Vec::new(),
        condition_target_ids: Vec::new(),
        is_positive: true,
    });
    input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 7021,
        group: 7021,
        condition_ids: vec![7021],
    });
    input.master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
        id: 902,
        skill_id: 902,
        level: 1,
        skill_trigger_type: SUSTAINED,
        skill_trigger_condition_group: 7021,
        skill_effect_type: 2000,
        effect_value: 10000,
        ..Default::default()
    });
    input.master.reindex().unwrap();
    input.deck = vec![Performer {
        character_id: 7,
        gekisou_skill: Some((901, 1)),
        gekisou_support_skills: vec![(902, 1)],
        ..Default::default()
    }];
    input.setup.fevers = vec![(100, 160), (gap + 100, gap + 160), (2 * gap + 100, 2 * gap + 160)];
    input.notes = [110, 120, gap + 100, 2 * gap + 100]
        .into_iter()
        .enumerate()
        .map(|(note_id, time_ms)| LiveNote {
            note_id: note_id as i32,
            note_operate_type: 1,
            judgement_type: 1,
            time_ms,
        })
        .collect();
    input.params.converted_note_count = input.notes.len() as i32;
    input.params.music_length_ms = 2 * gap + 2200;
    let placements = [200, 200, gap + 100, 2 * gap + 100];
    set_filing_frames(&mut input, &placements);
    input
}

fn set_filing_frames(input: &mut ProgramCase, placements: &[i32]) {
    let end = input.setup.fevers.iter().map(|&(_, end)| end).max().unwrap() + 2200;
    input.play.frames = (0..=end / 100 + 1)
        .map(|frame| PlayFrame {
            time_ms: frame * 100,
            judged: input
                .notes
                .iter()
                .zip(placements)
                .filter(|&(_, &at)| at == frame * 100)
                .map(|(note, _)| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    input.delta = vec![0.1; input.play.frames.len()];
}

fn filing_model(input: &ProgramCase) -> LiveModel {
    let mut model = if input.ranking.is_some() {
        LiveModel::new_gekisou_external(
            &input.master,
            &input.deck,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
        )
    } else {
        LiveModel::new_gekisou(&input.master, &input.deck, &input.notes, &input.events, input.params, &input.setup)
    }
    .unwrap();
    if let Some(ranking) = &input.ranking {
        model.set_rank_confirmation_timeline(ranking).unwrap();
    }
    model
}

fn filing_recording(input: &ProgramCase, certified: bool) -> BoundsTrace {
    let skills = luck_skills(&input.master).unwrap();
    let mut model = filing_model(input);
    let gate = check_recorder(&model, &skills).unwrap();
    let probes = model
        .luck_score_rows(&skills)
        .into_iter()
        .filter(|row| row.may_hold)
        .map(|row| ProbeRow { owner: row.owner, value: row.value })
        .collect();
    model.set_luck_weights(&skills, Vec::new()).unwrap();
    model.score.begin_bounds(probes, true);
    if certified {
        model.score.certify_bounds_filings(gate);
    }
    model.run_with_random(&input.play, &input.delta, LiveRandom::new(input.play.base_seed)).unwrap();
    assert_eq!(model.random.draws(), 0);
    model.score.bounds_trace.take().unwrap()
}

#[derive(Default)]
struct FilingSet {
    // A Rush command changes no float factor: its relevant observation is its rewind frame.
    rush: BTreeSet<(usize, usize)>,
    probes: BTreeSet<(usize, usize, i32)>,
    queries: Vec<(i32, i32)>,
}

fn possible_filings(trace: &BoundsTrace) -> FilingSet {
    let mut out = FilingSet::default();
    for event in &trace.events {
        match event {
            BoundsEvent::Potential { frame } => {
                out.rush.insert((out.queries.len(), *frame));
            }
            BoundsEvent::Factor { frame, command } if command.luck != 0 => {
                out.rush.insert((out.queries.len(), *frame));
            }
            BoundsEvent::Probe { frame, time_ms } => {
                out.probes.insert((out.queries.len(), *frame, *time_ms));
            }
            BoundsEvent::Query { time_ms, to } => out.queries.push((*time_ms, *to)),
            _ => {}
        }
    }
    out
}

#[derive(Default)]
struct Coverage {
    paths: usize,
    rush: usize,
    probes: usize,
    same_query_rush_on_off: bool,
    clamped_probe_end: bool,
    rush_end_times: BTreeSet<i32>,
}

/// A fixed binary Cartesian-prefix enumeration, replayed fresh from the native model each time. The admitted
/// fixture has equiprobable Critical/Miss bonus tables and deterministic base points; no DP, checkpoint or
/// exact-law enumerator supplies either its paths or its mass check.
fn every_native_filing_is_possible(input: &ProgramCase, allow_dp_refusal: bool) -> Coverage {
    const DEPTH: usize = 8;
    let trace = filing_recording(input, true);
    let possible = possible_filings(&trace);
    let owners: BTreeSet<_> = trace.probes.iter().map(|row| row.owner).collect();
    assert!(!owners.is_empty());
    let mut coverage = Coverage::default();
    let mut mass = 0usize;
    let mut score_mass = 0i128;
    let mut terminals = Vec::new();
    for length in 0..=DEPTH {
        for bits in 0..1usize << length {
            let prefix = (0..length).map(|bit| (bits >> bit) & 1).collect();
            let mut native = filing_model(input);
            // Record actual native commands. No potential hooks are enabled for this unweighted oracle.
            native.score.begin_bounds(Vec::new(), false);
            let result = native.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix));
            assert!(native.random.nominal_covers_draws());
            if let Some(branch) = native.random.nominal_branch() {
                assert!(result.is_err());
                assert!(length < DEPTH, "the fixed Cartesian-prefix domain must exhaust the fixture");
                assert_eq!(branch.len(), 2);
                assert!(branch.iter().all(|outcome| 2 * outcome.weight == outcome.total));
                continue;
            }
            result.unwrap();
            if !native.random.nominal_prefix_consumed() {
                continue;
            }
            let weight = 1usize << (DEPTH - length);
            mass += weight;
            score_mass += weight as i128 * i128::from(native.score());
            terminals.push((native.score(), native.current_life()));
            coverage.paths += 1;
            let native_trace = native.score.bounds_trace.take().unwrap();
            let mut queries = Vec::new();
            let mut rush_signs = BTreeMap::<usize, u8>::new();
            for event in &native_trace.events {
                match event {
                    BoundsEvent::Factor { frame, command } if command.luck != 0 => {
                        assert!(
                            possible.rush.contains(&(queries.len(), *frame)),
                            "native Rush filing at query {}, frame {frame}, time {} is absent",
                            queries.len(),
                            command.time_ms,
                        );
                        *rush_signs.entry(queries.len()).or_default() |= if command.luck > 0 { 1 } else { 2 };
                        if command.luck < 0 {
                            coverage.rush_end_times.insert(command.time_ms);
                        }
                        coverage.rush += 1;
                    }
                    BoundsEvent::Factor { frame, command }
                        if owners.contains(&command.owner_id) && command.note_mill != 0 =>
                    {
                        assert!(
                            possible.probes.contains(&(queries.len(), *frame, command.time_ms)),
                            "native probe filing at query {}, frame {frame}, time {} is absent",
                            queries.len(),
                            command.time_ms,
                        );
                        coverage.clamped_probe_end |=
                            command.note_mill < 0 && command.time_ms == input.params.music_length_ms;
                        coverage.probes += 1;
                    }
                    BoundsEvent::Query { time_ms, to } => queries.push((*time_ms, *to)),
                    _ => {}
                }
            }
            assert_eq!(queries, possible.queries, "all lottery paths retain the recorded query schedule");
            coverage.same_query_rush_on_off |= rush_signs.values().any(|&mask| mask == 3);
        }
    }
    assert_eq!(mass, 1 << DEPTH, "the terminal native branches partition probability one");
    match input.run(None, || false) {
        Ok(Some(summary)) => {
            for &(score, life) in &terminals {
                assert!(summary.final_support.lower <= score && score <= summary.final_support.upper);
                assert_eq!(summary.exact_final_life, Some(life));
            }
            let mean = score_mass as f64 / mass as f64;
            assert!(summary.final_mean.lower <= mean && mean <= summary.final_mean.upper);
        }
        Err(Error::Unsupported(_)) if allow_dp_refusal => {}
        result => panic!("unexpected score-certificate result: {result:?}"),
    }
    coverage
}

#[test]
fn certified_filing_hooks_cover_every_native_branch_across_ranges_and_quiet_gaps() {
    for gap in [2400, 12_000] {
        let input = filing_case(gap);
        let fallback = possible_filings(&filing_recording(&input, false));
        let certified = possible_filings(&filing_recording(&input, true));
        assert_eq!(certified.queries, fallback.queries);
        assert_eq!(certified.probes, fallback.probes);
        assert!(certified.rush.len() < fallback.rush.len());
        let coverage = every_native_filing_is_possible(&input, false);
        assert!(coverage.paths > 1 && coverage.rush > 0 && coverage.probes > 0);
        assert!(coverage.same_query_rush_on_off, "the same frame must exercise Critical then Miss");
    }
}

#[test]
fn certified_filing_hooks_cover_pending_late_notes_and_music_length_clamping() {
    let mut pending = filing_case(2400);
    set_filing_frames(&mut pending, &[0, 200, 2500, 4900]);
    let coverage = every_native_filing_is_possible(&pending, true);
    assert!(coverage.paths > 1 && coverage.rush > 0);

    let mut late = filing_case(2400);
    set_filing_frames(&mut late, &[2200, 2200, 4700, 7100]);
    let coverage = every_native_filing_is_possible(&late, true);
    assert!(coverage.paths > 0);

    let mut clamped = filing_case(2400);
    clamped.setup.fevers.truncate(1);
    clamped.notes.truncate(2);
    clamped.params.converted_note_count = 2;
    clamped.params.music_length_ms = 300;
    set_filing_frames(&mut clamped, &[200, 200]);
    let coverage = every_native_filing_is_possible(&clamped, true);
    assert!(coverage.clamped_probe_end, "an untimed score probe must file its signed end at the music length");
}

#[test]
fn a_non_luck_range_finish_keeps_rush_filings_after_the_weighted_handle_ended() {
    let mut input = filing_case(2400);
    input.setup.fevers = vec![(100, 160), (200, 6000), (2500, 2560)];
    input.setup.missions = vec![1, 2, 3];
    input.notes = [210, 220, 3000, 3100]
        .into_iter()
        .enumerate()
        .map(|(note_id, time_ms)| LiveNote {
            note_id: note_id as i32,
            note_operate_type: 1,
            judgement_type: 1,
            time_ms,
        })
        .collect();
    input.params.music_length_ms = 8200;
    set_filing_frames(&mut input, &[300, 300, 3000, 3100]);
    // The first non-LUCK finish clears the weighted handle. A later native Miss then Critical can create
    // another Rush, which the next non-LUCK finish disables while the long LUCK range is still playing.
    let coverage = every_native_filing_is_possible(&input, true);
    assert!(coverage.rush_end_times.iter().any(|&time| 3100 < time && time < 6000));
}
