//! Chart statistics measured on the whole-live simulation with Gekisou on and off, on a synthetic deck data file.

mod common;

use ournotes_deck::chartstats::{self, POWER};
use ournotes_deck::data::{DeckData, FORMAT};
use ournotes_deck::live::full::{self, GekisouSetup, LiveNote, LiveParams, Performer};
use ournotes_deck::live::model::{JudgementStream, JustRule, LiveModel, Play};
use ournotes_deck::live::score::{ComboTable, LiveScoreSettings};
use ournotes_deck::live::skip::skip_score;
use ournotes_deck::master::Master;
use serde_json::{Value, json};

fn columns(rows: &Value) -> Value {
    let mut cols: Vec<String> = Vec::new();
    for r in rows.as_array().unwrap() {
        for k in r.as_object().unwrap().keys() {
            if !cols.contains(k) {
                cols.push(k.clone());
            }
        }
    }
    let rows: Vec<Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| Value::Array(cols.iter().map(|c| r.get(c).cloned().unwrap_or(Value::Null)).collect()))
        .collect();
    json!({"columns": cols, "rows": rows})
}

const EVENTS: [i32; 5] = [3000, 5000, 20000, 40000, 58500];

/// A chart of `n` notes with a note exactly at each event time and at each event time + 5000 ms, a slide pair and
/// combo ticks, and these fevers; notes listed out of time order.
fn chart_json_fevers(score_id: i64, n: i32, rng: &mut common::Rng, fevers: &[(i32, i32)]) -> (Value, i32) {
    let (mut ids, mut ops, mut jts, mut times) = (vec![], vec![], vec![], vec![]);
    let mut push = |id: i32, op: i32, t: i32| {
        ids.push(id);
        ops.push(op);
        jts.push(if op == 21 || op == 120 { 21 } else { 1 });
        times.push(t);
    };
    let mut id = 1;
    for &e in &EVENTS {
        push(id, 1, e);
        push(id + 1, 1, e + 5000);
        id += 2;
    }
    for _ in 0..n {
        let op = [1, 1, 1, 20, 21, 40, 120][rng.below(7) as usize];
        push(id, op, rng.range(1000, 60000) as i32);
        id += 1;
    }
    let last = *times.iter().max().unwrap();
    (
        json!({
            "scoreId": score_id,
            "asset": {"key": "Live/MusicScore/x", "sha256": "0".repeat(64)},
            "notes": {"id": ids, "op": ops, "judgementType": jts, "timeMs": times},
            "skillEvents": {"timeMs": EVENTS},
            "fevers": {"startMs": fevers.iter().map(|f| f.0).collect::<Vec<_>>(),
                       "endMs": fevers.iter().map(|f| f.1).collect::<Vec<_>>()},
        }),
        last,
    )
}

/// The synthetic master; with `extra`, also live skill 4 (2000 on the confirmed rank, condition 7012) and 5 (2000
/// on a Gekisou combo, condition 7005), one level each.
fn document_with(charts: Vec<Value>, full_combo: i64, extra: bool) -> Value {
    let mut rng = common::Rng::new(7);
    let mut s = common::synth(&mut rng, 12, 4);
    if extra {
        let mut push = |table: &str, rows: Value| {
            let t = s.tables.iter_mut().find(|(n, _)| n == table).unwrap();
            t.1.as_array_mut().unwrap().extend(rows.as_array().unwrap().iter().cloned());
        };
        push(
            "MasterSkillCondition",
            json!([{"_id": 20, "_conditionType": 7012, "_conditionValues": [1], "_isPositive": true, "_conditionTargetIDs": []},
                   {"_id": 21, "_conditionType": 7005, "_conditionValues": [1], "_isPositive": true, "_conditionTargetIDs": []}]),
        );
        push(
            "MasterSkillConditionSet",
            json!([{"_id": 20, "_group": 20, "_conditionIds": [20]}, {"_id": 21, "_group": 21, "_conditionIds": [21]}]),
        );
        push("MasterLiveSkill", json!([{"_id": 4, "_skillCategories": [1]}, {"_id": 5, "_skillCategories": [1]}]));
        push(
            "MasterLiveSkillEffect",
            json!([{"_id": 1000, "_liveSkillID": 4, "_level": 1, "_skillConditionGroup": 20, "_skillTargetIDs": [],
                    "_skillEffectType": 2000, "_activationTimeSecond": 5.0, "_effectValue": 3000},
                   {"_id": 1001, "_liveSkillID": 5, "_level": 1, "_skillConditionGroup": 21, "_skillTargetIDs": [],
                    "_skillEffectType": 2000, "_activationTimeSecond": 5.0, "_effectValue": 3000}]),
        );
    }
    let mut master = serde_json::Map::new();
    for (name, rows) in &s.tables {
        master.insert(name.clone(), columns(rows));
    }
    master.insert(
        "MasterLiveMusicScore".into(),
        columns(&json!([{"_id": 1004, "_musicScoreTextFileName": "x", "_musicScoreLevel": 24, "_fullComboCount": full_combo}])),
    );
    master.insert(
        "MasterLiveComboScoreBonus".into(),
        columns(&json!([{"_id": 1, "_comboBonusType": 0, "_requiredComboCount": 10, "_bonusFactor": 0.01},
                         {"_id": 2, "_comboBonusType": 0, "_requiredComboCount": 100, "_bonusFactor": 0.05},
                         {"_id": 3, "_comboBonusType": 0, "_requiredComboCount": 300, "_bonusFactor": 0.1},
                         {"_id": 4, "_comboBonusType": 1, "_requiredComboCount": 10, "_bonusFactor": 0.02},
                         {"_id": 5, "_comboBonusType": 1, "_requiredComboCount": 30, "_bonusFactor": 0.05}])),
    );
    master.insert(
        "MasterLiveJudgementParameter".into(),
        columns(&json!([{"_id": 1, "_noteSimulateJudgement": 5, "_scorePercent": 100, "_damage": 0},
                         {"_id": 2, "_noteSimulateJudgement": 4, "_scorePercent": 50, "_damage": 0},
                         {"_id": 3, "_noteSimulateJudgement": 6, "_scorePercent": 120, "_damage": 0}])),
    );
    let mut settings = s.tables.iter().find(|(n, _)| n == "MasterLiveSettings").unwrap().1.clone();
    settings.as_array_mut().unwrap().push(json!({"_id": 9, "_key": "life_base", "_value": "1000"}));
    for (id, key, value) in [
        (10, "gekisou_luck_gauge_max", "140"),
        (11, "gekisou_luck_gauge_max_rush", "70"),
        (12, "gekisou_luck_rush_score_bonus_percent", "10"),
        (13, "life_denger", "300"),
    ] {
        settings.as_array_mut().unwrap().push(json!({"_id": id, "_key": key, "_value": value}));
    }
    // Gekisou: the song's missions combo, luck, Just (pattern 2), a Just timing row, rank bonuses, luck tables
    let mut musics = s.tables.iter().find(|(n, _)| n == "MasterLiveMusic").unwrap().1.clone();
    for m in musics.as_array_mut().unwrap() {
        m["_gekisouMission1"] = json!(1);
        m["_gekisouMission2"] = json!(2);
        m["_gekisouMission3"] = json!(3);
    }
    master.insert("MasterLiveMusic".into(), columns(&musics));
    let mut timing = s.tables.iter().find(|(n, _)| n == "MasterLiveJudgementTiming").map_or(json!([]), |t| t.1.clone());
    if !timing.as_array().unwrap().iter().any(|r| r["_noteJudgementType"] == 1 && r["_noteSimulateJudgement"] == 6) {
        timing.as_array_mut().unwrap().push(json!({"_id": 90, "_noteJudgementType": 1, "_noteSimulateJudgement": 6}));
    }
    master.insert("MasterLiveJudgementTiming".into(), columns(&timing));
    let ranks: Vec<Value> = (1..=3)
        .map(|c| json!({"_id": c, "_missionPattern": 2, "_count": c, "_rank": 1, "_scoreBonusPercent": 10 * c}))
        .collect();
    master.insert("MasterLiveGekisouRankingScoreBonus".into(), columns(&Value::Array(ranks)));
    master.insert(
        "MasterLiveGekisouLuckBasePoint".into(),
        columns(&json!([{"_id": 1, "_noteCategory": 0, "_noteSimulateJudgement": 5, "_weight": 1, "_basePoint": 10},
                         {"_id": 2, "_noteCategory": 0, "_noteSimulateJudgement": 6, "_weight": 1, "_basePoint": 10},
                         {"_id": 3, "_noteCategory": 1, "_noteSimulateJudgement": 5, "_weight": 1, "_basePoint": 2}])),
    );
    let lots: Vec<Value> = (0..5)
        .flat_map(|k| {
            (0..4).map(move |r| json!({"_id": 1 + 4 * k + r, "_chanceLotType": k, "_lotResult": r, "_weight": 1}))
        })
        .collect();
    master.insert("MasterLiveGekisouLuckBonusLot".into(), columns(&Value::Array(lots)));
    master.insert("MasterLiveSettings".into(), columns(&settings));
    json!({
        "format": FORMAT,
        "provenance": {"region": "test", "master": {"source": "api", "version": "v1"},
                       "exporter": {"name": "test", "version": "0", "chartFormat": "test"}},
        "master": master,
        "charts": charts,
    })
}

fn data(n: i32) -> (DeckData, i32) {
    data_fevers(n, &[])
}

fn data_fevers(n: i32, fevers: &[(i32, i32)]) -> (DeckData, i32) {
    data_with(n, fevers, false)
}

fn data_with(n: i32, fevers: &[(i32, i32)], extra: bool) -> (DeckData, i32) {
    let mut rng = common::Rng::new(11);
    let (chart, last) = chart_json_fevers(1004, n, &mut rng, fevers);
    let ops = chart["notes"]["op"].as_array().unwrap();
    let judged = ops.iter().filter(|o| ![0, 80, 82, 100, 103, 121, 122, 123].contains(&o.as_i64().unwrap())).count();
    (DeckData::from_json(&document_with(vec![chart], judged as i64, extra).to_string()).unwrap(), last)
}

const FEVERS: [(i32, i32); 3] = [(8000, 16000), (24000, 32000), (42000, 50000)];

fn notes_of(d: &DeckData) -> Vec<LiveNote> {
    let chart = d.chart(1004).unwrap();
    chart
        .notes
        .iter()
        .zip(&d.charts[0].judgement_types)
        .map(|(n, &jt)| LiveNote {
            note_id: n.id,
            time_ms: n.time_ms,
            note_operate_type: n.note_type,
            judgement_type: jt,
        })
        .collect()
}

#[test]
fn kinds_group_master_rows_by_shape() {
    let (d, _) = data(50);
    let kinds = chartstats::kinds(&d.master);
    // skill 1: 2000 for 5 s; skill 2: 2004 on two targets for 4.5 s; skill 3: two conditioned 2000 rows for 6 s
    assert_eq!(kinds.len(), 4);
    assert_eq!((kinds[0].effect_type, kinds[0].duration_ms, kinds[0].rows), (2000, 5000, 5));
    assert_eq!(kinds[0].values, vec![850, 1000, 1150, 1300, 1450]);
    assert_eq!((kinds[1].effect_type, kinds[1].skill_target_ids.clone()), (2004, vec![12, 13]));
    assert_eq!((kinds[2].skill_condition_group, kinds[3].skill_condition_group), (4, 5));
    assert_eq!(chartstats::kind_factor(2000, 10000), 1.0);
    assert_eq!(chartstats::kind_factor(2005, 10000), -1.0);
    assert_eq!(chartstats::kind_factor(2004, 10000), 1.0);
}

/// Every range mission, measured weights and the seed checks; then real decks of the master's own skills, simulated
/// directly, against the prediction from the statistics.
#[test]
fn measured_weights_predict_whole_live_simulations() {
    let (d, _) = data_fevers(700, &FEVERS);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 3).unwrap();
    assert!(s.unplayable.is_none());
    assert_eq!(s.missions, [1, 2, 3]);
    assert_eq!(s.seeds.iter().map(|x| x.seed).collect::<Vec<_>>(), ournotes_deck::live::seeds::published_seeds(3));
    assert_eq!(s.positions, 5);
    for (i, r) in s.ranges.iter().enumerate() {
        assert_eq!((r.index, r.mission, r.start_ms, r.end_ms), (i, i as i64 + 1, FEVERS[i].0, FEVERS[i].1));
        assert_eq!(r.rank_bonus_percent, 10 * (i as i64 + 1));
        // the synthetic table has rank 1 rows only
        assert_eq!(r.rank_bonus_percents, [10 * (i as i64 + 1), 0, 0, 0, 0]);
    }
    for seed in &s.seeds {
        assert_eq!(seed.ranges.len(), 3);
        for (i, r) in seed.ranges.iter().enumerate() {
            assert!(r.range_score > 0 && r.max_combo > 0);
            assert_eq!(r.rank_bonus as i64, r.range_score as i64 * s.ranges[i].rank_bonus_percent / 100);
            if i != 1 {
                assert_eq!(r.lot_results, [0; 4]);
            }
        }
        assert_eq!((seed.ranges[0].just_count, seed.ranges[1].just_count), (0, 0));
        assert_eq!(seed.ranges[2].just_count, s.just_notes);
        assert!(s.just_notes > 0);
        // the Perfect play: the ranges before the Just-count range play the same, the Just-count range scores less
        for r in &seed.ranges[..2] {
            assert_eq!(r.range_score_perfect, r.range_score);
        }
        assert!(seed.ranges[2].range_score_perfect < seed.ranges[2].range_score);
        assert!(seed.score_perfect < seed.score);
        assert!((seed.check.exact as f64 - seed.check.predicted).abs() <= seed.check.bound);
        assert_eq!(seed.weights.len(), kinds.len());
        // an unconditioned score-up raises the score wherever its position fires
        assert!(seed.weights[0].iter().all(|&w| w > 0.0), "{:?}", seed.weights[0]);
    }
    assert!(s.seeds[0].ranges[1].lot_results.iter().sum::<i32>() > 0);

    // real decks: the master's live skills 1 (2000) and 2 (2004) at their levels, simulated directly
    let chart = d.chart(1004).unwrap();
    let notes = notes_of(&d);
    let events: Vec<(i32, i32)> = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
    let setup = GekisouSetup { fevers: FEVERS.to_vec(), missions: vec![1, 2, 3] };
    let rule = JustRule::new(&d.master, &setup).unwrap();
    let stream = JudgementStream::theoretical_best_gekisou(&chart, &d.charts[0].judgement_types, &rule).unwrap();
    let dt = stream.delta_times().unwrap();
    let mut rng = common::Rng::new(9);
    for (si, seed) in s.seeds.iter().enumerate() {
        for _ in 0..6 {
            let power = rng.range(100_000, 900_000) as i32;
            let perf: Vec<Option<(i64, i64)>> = (0..5)
                .map(|_| if rng.below(5) == 0 { None } else { Some((rng.range(1, 2), rng.range(1, 5))) })
                .collect();
            let mut predicted = seed.score as f64 / POWER as f64;
            let mut gain = 0.0;
            for (k, p) in perf.iter().enumerate() {
                if let Some((id, lv)) = *p {
                    let row =
                        d.master.live_skill_effects.iter().find(|r| r.live_skill_id == id && r.level == lv).unwrap();
                    let kind = kinds
                        .iter()
                        .position(|k| {
                            k.effect_type == row.skill_effect_type
                                && k.skill_target_ids == row.skill_target_ids
                                && k.skill_condition_group == row.skill_condition_group
                        })
                        .unwrap();
                    let x = chartstats::kind_factor(row.skill_effect_type, row.effect_value);
                    predicted += x * seed.weights[kind][k];
                    gain += x;
                }
            }
            let deck: Vec<Performer> =
                perf.iter().map(|&p| Performer { live_skill: p, ..Default::default() }).collect();
            let params = LiveParams {
                skill_target_music_type: 1,
                total_power: power,
                music_level: 24,
                converted_note_count: chart.converted_note_count,
                music_length_ms: chart.last_timing_note_ms + 1000,
                score_music_length_ms: None,
                assist_factor: 1.0,
            };
            let mut play = stream.to_live_play().unwrap();
            play.base_seed = seed.seed;
            let exact = full::LiveModel::new_gekisou(&d.master, &deck, &notes, &events, params, &setup)
                .unwrap()
                .run_timed(&play, &dt)
                .unwrap() as f64;
            let p = power as f64 * predicted;
            let scale = power as f64 / POWER as f64;
            let bound = (s.judged_notes as f64 + 3.0) * (1.0 + scale * (1.0 + 2.0 * gain)) + 4e-6 * p;
            assert!((exact - p).abs() <= bound, "seed {si} power {power} deck {perf:?}: exact {exact} predicted {p}");
        }
    }
}

#[test]
fn a_chart_without_luck_ranges_has_one_seed() {
    let (d, _) = data(400);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 8).unwrap();
    assert!(s.ranges.is_empty());
    assert_eq!(s.seeds.len(), 1);
    assert_eq!(s.seeds[0].seed, 0);
    assert_eq!(s.just_notes, 0);
    // no Just: the Perfect play is the play; no ranges: nothing to rank
    assert_eq!(s.seeds[0].score_perfect, s.seeds[0].score);
    let rw = s.seeds[0].range_weights.as_ref().unwrap();
    assert!(rw.iter().all(|k| k.as_ref().unwrap().iter().all(|p| p.is_empty())));
    assert!(s.seeds[0].rank_check.is_none());
    assert_eq!(s.seeds[0].score_at_ranks(&s.ranges, &[]).unwrap(), s.seeds[0].score);
    // Gekisou off without fevers: the same live without a Gekisou controller
    assert_eq!(s.off_seeds.len(), 1);
    assert_eq!(s.off_seeds[0].score, s.seeds[0].score);
    for (on, off) in s.seeds[0].weights.iter().zip(&s.off_seeds[0].weights) {
        assert_eq!(Some(on), off.as_ref());
    }
}

#[test]
fn more_than_three_fevers_cannot_be_played() {
    let fevers = [(8000, 10000), (20000, 22000), (30000, 32000), (40000, 42000)];
    let (d, _) = data_fevers(300, &fevers);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 2).unwrap();
    assert!(s.seeds.is_empty());
    assert_eq!(s.ranges.len(), 3);
    assert!(s.unplayable.as_deref().unwrap().contains("fourth fever"));
    // without Gekisou the game plays it
    let off = &s.off_seeds[..];
    assert_eq!(off.len(), 1);
    assert!(off[0].score > 0 && off[0].weights.len() == kinds.len());
    assert!(off[0].weights.iter().all(|w| w.as_ref().is_some_and(|w| w.len() == s.positions)));
    assert!((off[0].check.exact as f64 - off[0].check.predicted).abs() <= off[0].check.bound);
    let chart = d.chart(1004).unwrap();
    let play = Play::theoretical_best(&d.master, &chart).unwrap();
    let model = LiveModel::new(&d.master, 24, &chart, &play).unwrap();
    assert_eq!(model.score(POWER, &[]), off[0].score);
}

#[test]
fn skip_coefficient_bounds_the_skip_score() {
    let (d, _) = data(500);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &[], 1).unwrap();
    let chart = d.chart(1004).unwrap();
    let settings = LiveScoreSettings::from_master(&d.master).unwrap();
    let combo = ComboTable::from_master(&d.master).unwrap();
    for power in [1000, POWER, 2_000_000] {
        let exact =
            skip_score(power, 24, &chart, &settings, &settings.valid_note_types(), Some(&combo)).unwrap() as f64;
        let p = power as f64 * s.skip;
        assert!(exact <= p * (1.0 + 4e-6) && exact >= p * (1.0 - 4e-6) - chart.notes.len() as f64, "{exact} {p}");
    }
}

#[test]
fn document_lists_kinds_and_charts() {
    let (d, _) = data_fevers(80, &FEVERS);
    let v = chartstats::document(&d, Some(2)).unwrap();
    assert_eq!(v["format"], "ournotes-deck.chart-stats/2");
    assert_eq!(v["source"]["region"], "test");
    assert_eq!(v["model"]["power"], POWER);
    assert_eq!(v["kinds"].as_array().unwrap().len(), 4);
    for key in ["ranks", "perfect", "off"] {
        assert!(v["model"][key].as_str().is_some_and(|s| !s.contains("  ")), "{key}");
    }
    let c = &v["charts"][0];
    assert_eq!(c["scoreId"], 1004);
    assert_eq!(c["events"].as_array().unwrap().len(), 5);
    assert_eq!(c["ranges"][0]["rankBonusPercents"].as_array().unwrap().len(), 5);
    assert_eq!(c["seeds"].as_array().unwrap().len(), 2);
    let seed = &c["seeds"][0];
    assert_eq!(seed["weights"].as_array().unwrap().len(), 4);
    assert!(seed["scorePerfect"].is_i64() && seed["ranges"][2]["rangeScorePerfect"].is_i64());
    // rangeWeights[kind][position][range]
    let rw = seed["rangeWeights"].as_array().unwrap();
    assert_eq!((rw.len(), rw[0].as_array().unwrap().len(), rw[0][0].as_array().unwrap().len()), (4, 5, 3));
    assert_eq!(seed["rankCheck"]["ranks"].as_array().unwrap().len(), 3);
    let off = c["offSeeds"].as_array().unwrap();
    assert_eq!(off.len(), 1);
    assert_eq!(off[0].as_object().unwrap().keys().collect::<Vec<_>>(), ["check", "score", "seed", "weights"]);
    assert!(c.get("unplayable").is_none());
    assert!(chartstats::document(&d, Some(0)).is_err());
}

/// The per-order model and the whole-live simulation give the same score on the theoretical best play (Gekisou off)
/// with live skills: 2000, 2004 and life-conditioned 2000 at every event.
#[test]
fn per_order_model_matches_the_whole_live_simulation_with_live_skills() {
    let (d, _) = data(700);
    let chart = d.chart(1004).unwrap();
    let dc = &d.charts[0];
    let play = Play::theoretical_best(&d.master, &chart).unwrap();
    let model = LiveModel::new(&d.master, 24, &chart, &play).unwrap();
    let notes: Vec<LiveNote> = chart
        .notes
        .iter()
        .zip(&dc.judgement_types)
        .map(|(n, &jt)| LiveNote {
            note_id: n.id,
            time_ms: n.time_ms,
            note_operate_type: n.note_type,
            judgement_type: jt,
        })
        .collect();
    let events: Vec<(i32, i32)> = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
    let stream = JudgementStream::theoretical_best(&chart).to_live_play().unwrap();
    let mut rng = common::Rng::new(5);
    let mut compared = 0;
    for _ in 0..40 {
        let perf: Vec<(i64, i64)> = (0..5).map(|_| (rng.range(1, 3), rng.range(1, 5))).collect();
        let power = rng.range(50_000, 900_000) as i32;
        let cmds = model.commands(&d.master, &perf).unwrap();
        let per_order = model.score(power, &cmds);
        let performers: Vec<Performer> =
            perf.iter().map(|&s| Performer { live_skill: Some(s), ..Default::default() }).collect();
        let params = LiveParams {
            skill_target_music_type: 0,
            total_power: power,
            music_level: 24,
            converted_note_count: chart.converted_note_count,
            music_length_ms: chart.last_timing_note_ms + 1000,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let whole =
            full::LiveModel::new(&d.master, &performers, &notes, &events, params).unwrap().run(&stream).unwrap();
        assert_eq!(per_order, whole, "skills {perf:?} power {power}");
        compared += 1;
    }
    assert_eq!(compared, 40);
}

/// A Gekisou live with explicit ranks: the chart's notes, events, setup and default play.
struct Ranked {
    notes: Vec<LiveNote>,
    events: Vec<(i32, i32)>,
    setup: GekisouSetup,
    play: ournotes_deck::live::full::LivePlay,
    dt: Vec<f32>,
    converted: i32,
    length: i32,
}

impl Ranked {
    fn new(d: &DeckData) -> Ranked {
        let chart = d.chart(1004).unwrap();
        let setup = GekisouSetup { fevers: FEVERS.to_vec(), missions: vec![1, 2, 3] };
        let rule = JustRule::new(&d.master, &setup).unwrap();
        let stream = JudgementStream::theoretical_best_gekisou(&chart, &d.charts[0].judgement_types, &rule).unwrap();
        Ranked {
            notes: notes_of(d),
            events: chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect(),
            setup,
            play: stream.to_live_play().unwrap(),
            dt: stream.delta_times().unwrap(),
            converted: chart.converted_note_count,
            length: chart.last_timing_note_ms + 1000,
        }
    }

    /// The score of a deck at a power and seed, range `i` confirmed at rank `ranks[i]` with its percentage.
    fn run(&self, master: &Master, perf: &[Option<(i64, i64)>], power: i32, seed: i32, ranks: &[(i32, i64)]) -> i32 {
        let deck: Vec<Performer> = perf.iter().map(|&p| Performer { live_skill: p, ..Default::default() }).collect();
        let params = LiveParams {
            skill_target_music_type: 1,
            total_power: power,
            music_level: 24,
            converted_note_count: self.converted,
            music_length_ms: self.length,
            score_music_length_ms: None,
            assist_factor: 1.0,
        };
        let mut lm =
            full::LiveModel::new_gekisou_external(master, &deck, &self.notes, &self.events, params, &self.setup)
                .unwrap();
        for (i, &(rank, pct)) in ranks.iter().enumerate() {
            lm.queue_gekisou_rank_confirmation(i, rank, pct).unwrap();
        }
        let mut play = self.play.clone();
        play.base_seed = seed;
        lm.run_timed(&play, &self.dt).unwrap()
    }
}

/// The kind of a live skill effect row.
fn kind_of(kinds: &[chartstats::Kind], row: &ournotes_deck::master::LiveSkillEffectRow) -> usize {
    kinds
        .iter()
        .position(|k| {
            k.effect_type == row.skill_effect_type
                && k.activation_time_second == row.activation_time_second
                && k.skill_target_ids == row.skill_target_ids
                && k.skill_condition_group == row.skill_condition_group
        })
        .unwrap()
}

/// Rank bonus percentages for every rank of every range, so that each rank moves the score.
fn rank_table(d: &mut DeckData) {
    d.master.gekisou_ranking_score_bonuses.clear();
    let mut id = 1;
    for count in 1..=3 {
        for rank in 1..=5 {
            d.master.gekisou_ranking_score_bonuses.push(ournotes_deck::master::GekisouRankingBonusRow {
                id,
                mission_pattern: 2,
                rank,
                count,
                score_bonus_percent: [30, 22, 15, 9, 4][rank as usize - 1] + 5 * count,
            });
            id += 1;
        }
    }
}

/// At random ranks the no-skill score from the statistics is exact, a unit effect's weight is within two points per
/// range and real decks at another power are within the bound, against plays through the explicit rank
/// confirmations.
#[test]
fn ranks_follow_linearly_on_the_rank_confirmation_path() {
    let (mut d, _) = data_fevers(700, &FEVERS);
    rank_table(&mut d);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 3).unwrap();
    for (i, r) in s.ranges.iter().enumerate() {
        assert_eq!(r.rank_bonus_percents[0], r.rank_bonus_percent);
        assert_eq!(r.rank_bonus_percents[4], 4 + 5 * (i as i64 + 1));
    }
    let live = Ranked::new(&d);
    // kind 0 (2000 for 5 s, no condition) at factor 1 as live skill 99
    let mut unit = d.master.clone();
    let mut row = d.master.live_skill_effects.iter().find(|r| r.live_skill_id == 1 && r.level == 1).unwrap().clone();
    assert_eq!(kind_of(&kinds, &row), 0);
    (row.id, row.live_skill_id, row.effect_value) = (100_000, 99, 10000);
    unit.live_skill_effects.push(row);
    let none = vec![None; 5];
    let mut rng = common::Rng::new(21);
    let mut checked = 0;
    for seed in &s.seeds {
        let rc = seed.rank_check.as_ref().expect("a rank check");
        assert!((rc.exact as f64 - rc.predicted).abs() <= rc.bound, "{rc:?}");
        assert!(rc.ranks.iter().all(|r| (1..=5).contains(r)));
        // rank 1 everywhere: the statistics themselves
        assert_eq!(seed.score_at_ranks(&s.ranges, &[1, 1, 1]).unwrap(), seed.score);
        let w1 = seed.weights_at_ranks(&s.ranges, &[1, 1, 1]).unwrap().unwrap();
        assert_eq!(w1, seed.weights.iter().cloned().map(Some).collect::<Vec<_>>());
        assert!(
            seed.score_at_ranks(&s.ranges, &[1, 1]).is_err() && seed.score_at_ranks(&s.ranges, &[1, 6, 1]).is_err()
        );
        for _ in 0..4 {
            let ranks: Vec<i32> = (0..3).map(|_| rng.range(1, 5) as i32).collect();
            let confirmed: Vec<(i32, i64)> =
                ranks.iter().zip(&s.ranges).map(|(&r, info)| (r, info.percent(r).unwrap())).collect();
            let exact0 = live.run(&d.master, &none, POWER, seed.seed, &confirmed);
            assert_eq!(exact0, seed.score_at_ranks(&s.ranges, &ranks).unwrap(), "ranks {ranks:?}");
            let w = seed.weights_at_ranks(&s.ranges, &ranks).unwrap().unwrap();
            for k in 0..5 {
                let mut perf = none.clone();
                perf[k] = Some((99, 1));
                let exact = live.run(&unit, &perf, POWER, seed.seed, &confirmed) as f64;
                let predicted = exact0 as f64 + POWER as f64 * w[0].as_ref().unwrap()[k];
                assert!((exact - predicted).abs() < 6.0 + 1e-6, "ranks {ranks:?} k {k}: {exact} {predicted}");
            }
            // real decks of the master's skills 1 (2000) and 2 (2004) at another power
            let power = rng.range(100_000, 900_000) as i32;
            let perf: Vec<Option<(i64, i64)>> = (0..5)
                .map(|_| if rng.below(5) == 0 { None } else { Some((rng.range(1, 2), rng.range(1, 5))) })
                .collect();
            let mut predicted = exact0 as f64 / POWER as f64;
            let mut gain = 0.0;
            for (k, p) in perf.iter().enumerate() {
                if let Some((id, lv)) = *p {
                    let row =
                        d.master.live_skill_effects.iter().find(|r| r.live_skill_id == id && r.level == lv).unwrap();
                    let x = chartstats::kind_factor(row.skill_effect_type, row.effect_value);
                    predicted += x * w[kind_of(&kinds, row)].as_ref().unwrap()[k];
                    gain += x;
                }
            }
            let exact = live.run(&d.master, &perf, power, seed.seed, &confirmed) as f64;
            let p = power as f64 * predicted;
            let scale = power as f64 / POWER as f64;
            let bound =
                (s.judged_notes as f64 + 3.0) * (1.0 + scale * (1.0 + 2.0 * gain)) + 6.0 * scale * gain + 4e-6 * p;
            assert!(
                (exact - p).abs() <= bound,
                "ranks {ranks:?} power {power} deck {perf:?}: exact {exact} predicted {p}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 12);
    // the lower ranks score less
    let seed = &s.seeds[0];
    assert!(seed.score_at_ranks(&s.ranges, &[5, 5, 5]).unwrap() < seed.score);
}

/// Gekisou off: the no-skill score is the per-order model's, and decks of the master's skills (live-conditioned
/// ones included) predicted from the off weights are within the flooring bound of the per-order model.
#[test]
fn off_seeds_match_the_per_order_model() {
    let (d, _) = data_fevers(700, &FEVERS);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 2).unwrap();
    let off = &s.off_seeds[0];
    assert_eq!((s.off_seeds.len(), off.seed), (1, chartstats::OFF_SEED));
    assert!(off.weights.iter().all(|w| w.as_ref().is_some_and(|w| w.len() == 5)));
    assert!((off.check.exact as f64 - off.check.predicted).abs() <= off.check.bound);
    // no Just, no rank bonus: less than Gekisou on
    assert!(s.seeds.iter().all(|x| off.score < x.score));
    let chart = d.chart(1004).unwrap();
    let play = Play::theoretical_best(&d.master, &chart).unwrap();
    let model = LiveModel::new(&d.master, 24, &chart, &play).unwrap();
    assert_eq!(model.score(POWER, &[]), off.score);
    let mut rng = common::Rng::new(13);
    for _ in 0..40 {
        let power = rng.range(50_000, 900_000) as i32;
        let perf: Vec<(i64, i64)> =
            (0..5).map(|_| if rng.below(5) == 0 { (0, 0) } else { (rng.range(1, 3), rng.range(1, 5)) }).collect();
        let exact = model.score(power, &model.commands(&d.master, &perf).unwrap()) as f64;
        let mut predicted = off.score as f64 / POWER as f64;
        let mut gain = 0.0;
        for (k, &(id, lv)) in perf.iter().enumerate() {
            for row in d.master.live_skill_effects.iter().filter(|r| r.live_skill_id == id && r.level == lv) {
                let x = chartstats::kind_factor(row.skill_effect_type, row.effect_value);
                predicted += x * off.weights[kind_of(&kinds, row)].as_ref().unwrap()[k];
                gain += x;
            }
        }
        let p = power as f64 * predicted;
        let scale = power as f64 / POWER as f64;
        let bound = s.judged_notes as f64 * (1.0 + scale * (1.0 + 2.0 * gain)) + 4e-6 * p;
        assert!((exact - p).abs() <= bound, "power {power} deck {perf:?}: exact {exact} predicted {p}");
    }
}

/// A kind on the confirmed rank has no range weights (the other kinds keep theirs); a kind on the Gekisou state has
/// no weights with Gekisou off and stays out of that check deck.
#[test]
fn kinds_on_the_rank_or_the_gekisou_state() {
    let (d, _) = data_with(500, &FEVERS, true);
    let kinds = chartstats::kinds(&d.master);
    assert_eq!(kinds.len(), 6);
    assert_eq!((kinds[4].skill_condition_group, kinds[5].skill_condition_group), (20, 21));
    let reads: Vec<bool> = kinds.iter().map(|k| k.reads_rank(&d.master)).collect();
    assert_eq!(reads, [false, false, false, false, true, false]);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 2).unwrap();
    for seed in &s.seeds {
        let rw = seed.range_weights.as_ref().unwrap();
        assert!(rw[4].is_none() && rw.iter().enumerate().all(|(i, w)| i == 4 || w.is_some()));
        assert!((seed.check.exact as f64 - seed.check.predicted).abs() <= seed.check.bound);
        let w = seed.weights_at_ranks(&s.ranges, &[2, 3, 4]).unwrap().unwrap();
        assert!(w[4].is_none() && w[5].is_some());
        if seed.check.deck.iter().flatten().any(|&(ki, _)| ki == 4) {
            assert!(seed.rank_check.is_none());
        }
    }
    let off = &s.off_seeds[0];
    assert!(off.weights[5].is_none() && off.weights.iter().enumerate().all(|(i, w)| i == 5 || w.is_some()));
    assert!(off.check.deck.iter().flatten().all(|&(ki, _)| ki != 5));
    assert!((off.check.exact as f64 - off.check.predicted).abs() <= off.check.bound);
}
