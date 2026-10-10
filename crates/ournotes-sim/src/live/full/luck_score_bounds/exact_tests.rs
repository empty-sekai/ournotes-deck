//! The exact per-class expectation against the complete tree of nominal lottery outcomes.
use super::exact_paths::{ExactReplay, NoteValue, Orders, first_orders, undo_floors};
use super::*;
use crate::live::full::combo::ComboCounter;
use crate::live::full::luck_dp::RUSH_FINISH;
use crate::live::full::luck_exact::{LuckExactBudget, luck_exact_law_with_ranking};
use crate::live::full::scorecalc::IncrementalCalculator;
use crate::live::full::{JudgedNote, PlayFrame};
use crate::live::score::{LiveScoreCalculator, LiveScoreSettings, get_frame};
use crate::live::skill::FactorCommand;
use crate::num::FxHashMap;
use serde_json::json;

/// Uniform lottery results, a Rush score bonus and a 250% rank bonus per range. `probe` gives the performer a
/// Gekisou score-up that holds while Rush runs. `step` is the play frame length; notes come in triples spread over
/// `spread` ms.
fn fixture(
    probe: Option<i64>,
    step: i32,
    spread: i32,
    times: &[i32],
    fevers: Vec<(i32, i32)>,
) -> (Master, Vec<Performer>, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let lots: Vec<_> = (0..5)
        .flat_map(|kind| {
            (0..4)
                .map(move |result| json!({"_id":kind*4+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1}))
        })
        .collect();
    let mut tables = json!({
        "MasterLiveSettings":[
            {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
            {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
            {"_id":3,"_key":"life_base","_value":"1000"},
            {"_id":4,"_key":"life_denger","_value":"300"},
            {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
            {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
            {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
        "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
        "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
        "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
        "MasterLiveGekisouLuckBasePoint":[{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":60}],
        "MasterLiveGekisouLuckBonusLot":lots,
    });
    if let Some(value) = probe {
        let extra = json!({
            "MasterSkillCondition":[{"_id":1,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}],
            "MasterSkillConditionSet":[{"_id":1,"_group":1,"_conditionIds":[1]}],
            "MasterSkillEffectSetting":[{"_id":1,"_skillEffectType":2000,"_phase":1}],
            "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":2}],
            "MasterGekisouSkillEffect":[{
                "_id":1,"_gekisouSkillID":1,"_level":1,"_skillTriggerType":2,
                "_skillTriggerConditionGroup":1,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
                "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.0,"_effectValue":value,
                "_maxEffectValue":0,"_effectLimitCount":8,"_skillCumulativeConditionID":0,
                "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]
        });
        for (name, rows) in extra.as_object().unwrap() {
            tables[name] = rows.clone();
        }
    }
    let texts: Vec<_> = tables
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
        .collect();
    let mut master =
        Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()))
            .unwrap();
    master.gekisou_ranking_score_bonuses = (1..=3)
        .map(|count| crate::master::GekisouRankingBonusRow {
            id: count,
            mission_pattern: gekisou::mission_pattern(2, 2, 2),
            rank: 1,
            count,
            score_bonus_percent: 250,
        })
        .collect();
    let deck = match probe {
        Some(_) => vec![Performer { gekisou_skill: Some((1, 1)), ..Default::default() }],
        None => Vec::new(),
    };
    let mut note_times: Vec<i32> = times.iter().flat_map(|&t| [0, spread / 2, spread].map(|d| t + d)).collect();
    note_times.sort_unstable();
    let notes: Vec<_> = note_times
        .iter()
        .enumerate()
        .map(|(i, &time_ms)| LiveNote { note_id: i as i32 + 1, note_operate_type: 1, judgement_type: 1, time_ms })
        .collect();
    let music_length_ms = fevers.last().unwrap().1 + 3000;
    let params = LiveParams {
        skill_target_music_type: 0,
        total_power: 1000,
        music_level: 20,
        converted_note_count: notes.len() as i32,
        music_length_ms,
        score_music_length_ms: None,
        assist_factor: 1.0,
    };
    let setup = GekisouSetup { fevers, missions: vec![2, 2, 2] };
    let frames: Vec<_> = (0..=music_length_ms / step)
        .map(|i| PlayFrame {
            time_ms: i * step,
            judged: notes
                .iter()
                .filter(|note| note.time_ms > (i - 1) * step && note.time_ms <= i * step)
                .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                .collect(),
        })
        .collect();
    let delta = vec![step as f32 / 1000.0; frames.len()];
    (master, deck, notes, params, setup, LivePlay { frames, base_seed: 0 }, delta)
}

type Case = (Master, Vec<Performer>, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>);

fn cases() -> Vec<(&'static str, Case)> {
    vec![
        ("one range", fixture(None, 100, 0, &[100, 200, 300], vec![(100, 400)])),
        ("in-frame notes, probe", fixture(Some(8000), 100, 20, &[100, 200, 300], vec![(100, 400)])),
        ("fine frames, probe", fixture(Some(8000), 10, 20, &[100, 200, 300], vec![(100, 400)])),
        ("negative probe", fixture(Some(-5000), 10, 20, &[100, 200, 300], vec![(100, 400)])),
        ("two ranges, probe", fixture(Some(8000), 10, 20, &[100, 2100, 2200], vec![(100, 300), (2000, 2300)])),
        ("notes at the range end, probe", fixture(Some(8000), 10, 20, &[100, 200, 380], vec![(100, 400)])),
    ]
}

/// The nominal expectation and the scores of the complete outcome tree.
fn nominal(name: &str, case: &Case, ranking: Option<&[crate::replay::RankConfirmation]>) -> (f64, Vec<i32>) {
    let (master, deck, notes, params, setup, play, delta) = case;
    let attempt = luck_exact_law_with_ranking(
        master,
        deck,
        notes,
        &[],
        *params,
        setup,
        play,
        delta,
        ranking,
        &mut LuckExactBudget::default(),
        || false,
    )
    .unwrap();
    let law = attempt.law.unwrap_or_else(|| panic!("{name}: the nominal tree completes: {:?}", attempt.decline));
    let atoms = law.atoms();
    let mean = atoms
        .iter()
        .map(|atom| f64::from(atom.score) * atom.mass.numerator as f64 / atom.mass.denominator as f64)
        .sum();
    (mean, atoms.iter().map(|atom| atom.score).collect())
}

fn exact(case: &Case, ranking: Option<&[crate::replay::RankConfirmation]>) -> LuckExactScore {
    let (master, deck, notes, params, setup, play, delta) = case;
    let skills = luck_skills(master).unwrap();
    luck_exact_score(master, &skills, deck, notes, &[], *params, setup, play, delta, ranking, None, true).unwrap()
}

#[test]
fn exact_expectations_contain_the_complete_nominal_law_within_the_rank_truncation() {
    for (name, case) in cases() {
        let confirmations: Vec<_> = (0..case.4.fevers.len())
            .map(|range| crate::replay::RankConfirmation { frame: 0, range, rank: 1, percent: 250 })
            .collect();
        for ranking in [None, Some(confirmations.as_slice())] {
            let (expected, scores) = nominal(name, &case, ranking);
            let score = exact(&case, ranking);
            let width = score.final_mean.upper - score.final_mean.lower;
            let slack = 1e-9 * expected.abs();
            assert!(
                score.final_mean.lower - slack <= expected && expected <= score.final_mean.upper + slack,
                "{name} network={}: nominal {expected} outside {:?}",
                ranking.is_some(),
                score.final_mean
            );
            // Each rank bonus truncates once; the note sum is a point.
            assert!(width <= score.ranges.len() as f64 + 1e-6, "{name}: width {width} {score:?}");
            assert_eq!(score.wide_notes, 0, "{name}: {score:?}");
            for &native in &scores {
                assert!(
                    score.final_support.lower <= native && native <= score.final_support.upper,
                    "{name}: {native} outside {:?}",
                    score.final_support
                );
            }
            let distinct: std::collections::BTreeSet<_> = scores.iter().collect();
            assert!(distinct.len() > 1, "{name}: the lottery must change the score");
        }
    }
}

#[test]
fn probe_cases_hold_the_score_up_on_some_paths_only() {
    for (name, case) in cases().into_iter().filter(|(name, _)| name.contains("probe")) {
        let score = exact(&case, None);
        // Some note is scored with both probe classes possible.
        let mixed = score.notes.iter().any(|note| {
            let possible = |bucket: usize| note.probability[bucket].upper > 0.0 && note.buckets[bucket].is_some();
            (possible(0) || possible(2)) && (possible(1) || possible(3))
        });
        assert!(mixed, "{name}: {:?}", score.notes);
        // The held score-up changes the integer score of such a note.
        let changed = score
            .notes
            .iter()
            .any(|note| matches!((note.buckets[2], note.buckets[3]), (Some(off), Some(on)) if off.lower != on.lower));
        assert!(changed, "{name}");
    }
}

#[test]
fn every_seed_scores_inside_the_exact_support() {
    for (name, case) in cases() {
        let (master, deck, notes, params, setup, play, delta) = &case;
        let score = exact(&case, None);
        for seed in -8..24 {
            let mut native = LiveModel::new_gekisou(master, deck, notes, &[], *params, setup).unwrap();
            native.set_seed(seed);
            for (frame, &dt) in play.frames.iter().zip(delta) {
                native.frame_timed(frame.time_ms, &frame.judged, dt).unwrap();
            }
            assert!(
                score.final_support.lower <= native.score() && native.score() <= score.final_support.upper,
                "{name} seed={seed}: {} outside {:?}",
                native.score(),
                score.final_support
            );
        }
    }
}

#[test]
fn undo_floors_keep_every_frame_a_later_query_executes_again() {
    let note = NoteCommand::new(0, 1, 0, 1, 1);
    let events = vec![
        BoundsEvent::Note { frame: 2, index: 0, note },
        BoundsEvent::Query { time_ms: 120, to: 3 },
        BoundsEvent::Query { time_ms: 160, to: 4 },
        BoundsEvent::Potential { frame: 3, start: false },
        BoundsEvent::Query { time_ms: 200, to: 5 },
        BoundsEvent::Query { time_ms: 240, to: 6 },
    ];
    // The third query may rewind to frame 2 and execute from frame 3; the last executes from frame 6 only.
    assert_eq!(super::exact_paths::undo_floors(&events, &[1 << 1]).unwrap(), vec![3, 3, 6, i32::MAX]);
    // A play frame whose lotteries admit no Rush change files nothing at its potentials.
    assert_eq!(super::exact_paths::undo_floors(&events, &[1 | 1 << 6]).unwrap(), vec![4, 5, 6, i32::MAX]);
    assert!(super::exact_paths::undo_floors(&events, &[]).is_err());
}

/// The class supports of a note whose factor fields span a box are the least and the greatest native score over
/// the box, including boxes on which a factor changes sign.
#[test]
fn note_classes_take_the_native_score_extremes_over_the_factor_box() {
    let calc = calculator().calc;
    let rush_percent = 10;
    let luck = [0, rush_percent].map(|rush| get_luck_factor_percent(rush) as f32 / 100f32);
    let ordinary = 1.0f32;
    // (combo field, note field) bounds.
    let boxes = [((0.0f32, 0.0f32), (1.0f32, 1.25f32)), ((-1.5, 0.5), (-0.25, 0.75)), ((0.1, 0.3), (-2.0, -1.0))];
    let samples = |a: f32, b: f32| (0..=64).map(move |i| if i == 64 { b } else { a + (b - a) * i as f32 / 64.0 });
    for ((combo_low, combo_high), (up_low, up_high)) in boxes {
        for score_type in 1..=4 {
            let note = NoteCommand::new(0, 100, 1, 1, score_type);
            let (mut low, mut high) = ([0f32; FIELDS], [0f32; FIELDS]);
            (low[0], high[0], low[1], high[1]) = (combo_low, combo_high, up_low, up_high);
            let value = NoteValue { class: 0, combo: (ordinary.to_bits(), 1f32.to_bits()), low, high };
            let buckets = super::exact::note_classes(&calc, &note, &[value], rush_percent, false).unwrap();
            for (rush, &luck) in luck.iter().enumerate() {
                let (mut least, mut greatest) = (i32::MAX, i32::MIN);
                for field in samples(combo_low, combo_high) {
                    for up in samples(up_low, up_high) {
                        let combo = 1f32 * (field + ordinary);
                        let score = calc.note_score_core(note.life, 1, score_type, combo, up + 0.0, luck).unwrap();
                        (least, greatest) = (least.min(score), greatest.max(score));
                    }
                }
                assert_eq!(buckets[rush << 1], Some((least, greatest)), "score type {score_type} rush {rush}");
                assert_eq!(buckets[1 | rush << 1], None);
            }
        }
    }
}

fn calculator() -> IncrementalCalculator {
    let settings = LiveScoreSettings {
        score_adjustment_factor: 3.0,
        life_onus_factor: 0.5,
        note_factor_percent: [(1, 100)].into(),
        judgement_score_factor_percent: [(1, 100), (2, 90), (3, 70), (4, 50)].into(),
    };
    IncrementalCalculator::new(LiveScoreCalculator::new(100_000, 20, 64, &settings, 1.0, 1.0, None), 1000)
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

#[derive(Clone)]
enum Step {
    Note(NoteCommand),
    Command(FactorCommand),
    /// A command in the schedules of some orders only.
    Partial(FactorCommand, Orders),
    /// A play frame's start at which a range FINISH disables a running Rush.
    Finish(i32),
    /// A skill boundary; with `true` the new probe class equals the Rush.
    Probe(i32, bool),
    /// The lotteries of the play frame at this time, which may file Rush commands at the listed chart times.
    Lotteries(i32, Vec<i32>),
    Query(i32),
}

const ROWS: [(i32, i32); 2] = [(101, 50_000), (201, 80_000)];

/// Play frames 13 ms apart, each with a skill boundary, its score queries and its lotteries. With more than one
/// order, some commands belong to random subsets of the orders.
fn schedule(rng: &mut Rng, orders: usize) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut note_id = 0;
    let mut t = 0;
    while t < 900 {
        if rng.chance(8) {
            steps.push(Step::Finish(t));
        }
        let mut judged = vec![t];
        if rng.chance(25) {
            steps.push(Step::Command(FactorCommand {
                time_ms: (t - rng.below(40) as i32).max(0),
                // Owner 101 also holds a probe row: equal keys have no recorded filing order.
                owner_id: [1, 101, 150, 300][rng.below(4) as usize],
                note_mill: [12_345, -12_345, 50_000, 3_333, -7_777, 0][rng.below(6) as usize],
                combo_mill: [0, 1_200, -1_200, 4_321][rng.below(4) as usize],
                judgement: 3 + rng.below(4) as i32,
                judge_mill: [0, 777, -777, 20_000][rng.below(4) as usize],
                ..Default::default()
            }));
        }
        if orders > 1 && rng.chance(40) {
            let command = FactorCommand {
                time_ms: (t - rng.below(40) as i32).max(0),
                owner_id: [1, 101, 150, 300][rng.below(4) as usize],
                note_mill: [12_345, -12_345, 50_000, 3_333, 0][rng.below(5) as usize],
                combo_mill: [0, 1_200, -4_321][rng.below(3) as usize],
                judgement: 3 + rng.below(4) as i32,
                judge_mill: [0, 777, -20_000][rng.below(3) as usize],
                ..Default::default()
            };
            let mask = 1 + rng.below(first_orders(orders) as u64 - 1) as Orders;
            steps.push(Step::Partial(command, mask));
        }
        if rng.chance(50) {
            let time = (t - rng.below(13) as i32).max(0);
            steps.push(Step::Note(NoteCommand::new(time, 100, note_id, 1, 1 + rng.below(4) as i32)));
            judged.push(time);
            note_id += 1;
        }
        steps.push(Step::Query(t));
        steps.push(Step::Probe(t, rng.chance(20)));
        steps.push(Step::Query(t));
        steps.push(Step::Lotteries(t, judged));
        if rng.chance(5) {
            // A solo rank snapshot rewinds the calculator to an earlier time.
            steps.push(Step::Query((t - rng.below(40) as i32).max(0)));
        }
        t += 13;
    }
    steps.push(Step::Query(1000));
    steps
}

fn rush_command(time_ms: i32) -> FactorCommand {
    FactorCommand { time_ms, owner_id: -1, luck: 10, ..Default::default() }
}

/// The exact replay of `lanes` orders through `steps`. `select` gives the orders of this replay that hold a
/// partial command, if any. Returns the replay and each note's index in its notes.
fn replay_schedule(
    steps: &[Step],
    lanes: usize,
    select: impl Fn(Orders) -> Option<Orders>,
) -> (ExactReplay, Vec<usize>) {
    let rows: Vec<_> = ROWS.iter().map(|&(owner, m)| ProbeRow { owner, value: m as f32 / 100000f32 }).collect();
    let frames = calculator().executed_states().0;
    let frame_of = |time: i32| (get_frame(time).max(0) as usize).min(frames - 1);
    let every = first_orders(lanes);
    let command = |step: &Step| match step {
        Step::Command(command) => Some((*command, every)),
        Step::Partial(command, mask) => select(*mask).map(|orders| (*command, orders)),
        _ => None,
    };
    let mut events = Vec::new();
    let mut transitions = Vec::new();
    let mut finish = false;
    for step in steps {
        match step {
            Step::Note(note) => {
                events.push(BoundsEvent::Note { frame: frame_of(note.time_ms), index: 0, note: *note });
            }
            Step::Command(_) | Step::Partial(..) => {
                if let Some((command, _)) = command(step) {
                    events.push(BoundsEvent::Factor { frame: frame_of(command.time_ms), command });
                }
            }
            Step::Finish(t) => {
                events.push(BoundsEvent::Potential { frame: frame_of(*t), start: true });
                finish = true;
            }
            Step::Probe(t, _) => events.push(BoundsEvent::Probe { frame: frame_of(*t), time_ms: *t }),
            Step::Lotteries(t, times) => {
                events.extend(times.iter().map(|&time| BoundsEvent::Potential { frame: frame_of(time), start: false }));
                events.push(BoundsEvent::ProbabilityReady(*t));
                transitions.push(0xff | if finish { RUSH_FINISH } else { 0 });
                finish = false;
            }
            Step::Query(t) => events.push(BoundsEvent::Query { time_ms: *t, to: frame_of(*t) as i32 }),
        }
    }
    let mut replay = ExactReplay::new(frames, &rows, undo_floors(&events, &transitions).unwrap(), lanes).unwrap();
    let mut combos = FxHashMap::default();
    let mut index = Vec::new();
    for step in steps {
        match step {
            Step::Note(note) => {
                let key = (frame_of(note.time_ms), note.note_id as usize);
                combos.insert(key, (1f32, 1f32));
                index.push(replay.file_note(key.0, note.time_ms, note.note_id, key).unwrap());
            }
            Step::Command(_) | Step::Partial(..) => {
                if let Some((command, orders)) = command(step) {
                    replay.file_command(frame_of(command.time_ms), &command, orders).unwrap();
                }
            }
            Step::Finish(t) => replay.finish(frame_of(*t)).unwrap(),
            Step::Probe(t, follows) => replay.probe(frame_of(*t), *t, 0b1111, *follows).unwrap(),
            Step::Lotteries(_, times) => {
                let places: Vec<i32> = times.iter().map(|&time| frame_of(time) as i32).collect();
                replay.lotteries(&places, 0xff).unwrap();
            }
            Step::Query(t) => {
                replay.query(frame_of(*t) as i32, &combos).unwrap();
            }
        }
    }
    (replay, index)
}

/// Every native history of a schedule (probe switches, also tied to the Rush; Rush commands at a range FINISH and
/// at any lottery place; owner-local filing orders; rank rewinds) executes each note with a factor state inside
/// the exact replay's bounds for the note's probe class.
#[test]
fn exact_replay_bounds_every_native_history_of_random_schedules() {
    let (mut checked, mut spread) = (0, 0);
    for seed in 1..=12u64 {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x2545_f491_4f6c_dd1d));
        let steps = schedule(&mut rng, 1);
        let (replay, index) = replay_schedule(&steps, 1, |_| None);
        for _ in 0..40 {
            let mut native = calculator();
            let combo = ComboCounter::new(64);
            let (mut on, mut rush) = (false, false);
            let mut switches = Vec::new();
            for step in &steps {
                match step {
                    Step::Note(note) => native.add_note(*note),
                    Step::Command(command) => native.add_factor(*command),
                    Step::Partial(..) => unreachable!("one order"),
                    Step::Finish(t) => {
                        if rush {
                            rush = false;
                            native.add_factor(rush_command(*t));
                        }
                    }
                    Step::Probe(t, follows) => {
                        let next = if *follows { rush } else { on ^ rng.chance(30) };
                        if next != on {
                            on = next;
                            switches.push(*t);
                            for &(owner, m) in &ROWS {
                                let note_mill = if on { m } else { -m };
                                native.add_factor(FactorCommand {
                                    time_ms: *t,
                                    owner_id: owner,
                                    note_mill,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                    Step::Lotteries(_, times) => {
                        for _ in 0..2 {
                            if rng.chance(25) {
                                rush = !rush;
                                native.add_factor(rush_command(times[rng.below(times.len() as u64) as usize]));
                            }
                        }
                    }
                    Step::Query(t) => {
                        native.calculate(*t, &combo, None).unwrap();
                    }
                }
            }
            for (note_id, state) in native.executed_states().1 {
                let note = &replay.notes[index[note_id as usize]];
                let class = (switches.iter().filter(|&&t| t <= note.time_ms).count() % 2) as u8;
                let value =
                    note.values_of(0).find(|value| value.class == class).expect("every path class is reachable");
                for (field, actual) in state.into_iter().enumerate() {
                    assert!(
                        value.low[field] <= actual && actual <= value.high[field],
                        "seed {seed} note {note_id} class {class} field {field}: {actual} outside {}..={}",
                        value.low[field],
                        value.high[field]
                    );
                }
                checked += 1;
            }
        }
        spread +=
            replay.notes.iter().flat_map(|note| note.values_of(0)).filter(|value| value.low != value.high).count();
    }
    assert!(checked > 10_000, "{checked}");
    // Merged histories with different binary32 states occur, so the bounds are exercised beyond single points.
    assert!(spread > 0);
}

/// A replay of several orders, whose schedules differ in commands that belong to some of the orders, gives every
/// note of every order exactly the values of a replay of that order alone.
#[test]
fn several_orders_replay_as_each_order_alone() {
    const LANES: usize = 3;
    let values = |replay: &ExactReplay, note: usize, order: usize| {
        let mut values: Vec<_> = replay.notes[note]
            .values_of(order)
            .map(|value| (value.class, value.combo, value.low.map(f32::to_bits), value.high.map(f32::to_bits)))
            .collect();
        values.sort_unstable();
        values
    };
    let (mut compared, mut differing) = (0, 0);
    for seed in 1..=12u64 {
        let mut rng = Rng(0x0051_7cc1_b727_220a ^ seed.wrapping_mul(0x2545_f491_4f6c_dd1d));
        let steps = schedule(&mut rng, LANES);
        let (joint, joint_index) = replay_schedule(&steps, LANES, Some);
        let alone: Vec<_> = (0..LANES)
            .map(|order| replay_schedule(&steps, 1, move |mask| (mask >> order & 1 != 0).then_some(1)))
            .collect();
        for (note, &at) in joint_index.iter().enumerate() {
            let per_order: Vec<_> = alone.iter().map(|(replay, index)| values(replay, index[note], 0)).collect();
            for (order, expected) in per_order.iter().enumerate() {
                assert_eq!(&values(&joint, at, order), expected, "seed {seed} note {note} order {order}");
                compared += 1;
            }
            differing += usize::from(per_order.iter().any(|values| values != &per_order[0]));
        }
    }
    assert!(compared > 1_000, "{compared}");
    // The orders' schedules make their note values differ.
    assert!(differing > 0);
}

/// A query at a play frame reads the notes before the frame's time in the state before the frame, the notes at
/// its time on the side of the frame's skill boundary it runs, and the earlier notes in their filed state.
#[test]
fn queries_read_frame_notes_on_their_side_of_the_skill_boundary() {
    let mass = |class: usize| {
        let mut mass = [ProbabilityMass::ZERO; 4];
        mass[class] = ProbabilityMass::ONE;
        mass
    };
    let curve = LuckDpCertifiedResult {
        probe_transitions: vec![1; 2],
        rush_transitions: vec![65; 2],
        steps: Vec::new(),
        frame_queries: vec![(100, [mass(1), mass(2), mass(3)])],
        probes: Vec::new(),
        range_moments: Vec::new(),
        peak_states: 1,
        transitions: 0,
    };
    let read = |skills_at, note_ms| super::exact::query_mass(&curve, &[50, 100], 50, skills_at, 100, note_ms).unwrap();
    assert_eq!(read(50, 80), mass(1));
    assert_eq!(read(100, 80), mass(1));
    assert_eq!(read(100, 100), mass(2));
    assert_eq!(read(50, 100), mass(3));
    assert_eq!(read(100, 40), mass(0));
}
