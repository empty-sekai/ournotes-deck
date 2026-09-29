//! Chart statistics measured on the whole-live simulation with Gekisou on, on a synthetic deck data file.

mod common;

use ournotes_deck::chartstats::{self, POWER};
use ournotes_deck::data::{DeckData, FORMAT};
use ournotes_deck::live::full::{self, GekisouSetup, LiveNote, LiveParams, Performer};
use ournotes_deck::live::model::{JudgementStream, JustRule, LiveModel, Play};
use ournotes_deck::live::score::{ComboTable, LiveScoreSettings};
use ournotes_deck::live::skip::skip_score;
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

fn document(charts: Vec<Value>, full_combo: i64) -> Value {
    let mut rng = common::Rng::new(7);
    let s = common::synth(&mut rng, 12, 4);
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
    let mut rng = common::Rng::new(11);
    let (chart, last) = chart_json_fevers(1004, n, &mut rng, fevers);
    let ops = chart["notes"]["op"].as_array().unwrap();
    let judged = ops.iter().filter(|o| ![0, 80, 82, 100, 103, 121, 122, 123].contains(&o.as_i64().unwrap())).count();
    (DeckData::from_json(&document(vec![chart], judged as i64).to_string()).unwrap(), last)
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
}

#[test]
fn more_than_three_fevers_cannot_be_played() {
    let fevers = [(8000, 10000), (20000, 22000), (30000, 32000), (40000, 42000)];
    let (d, _) = data_fevers(300, &fevers);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &chartstats::kinds(&d.master), 2).unwrap();
    assert!(s.seeds.is_empty());
    assert_eq!(s.ranges.len(), 3);
    assert!(s.unplayable.as_deref().unwrap().contains("fourth fever"));
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
    let c = &v["charts"][0];
    assert_eq!(c["scoreId"], 1004);
    assert_eq!(c["events"].as_array().unwrap().len(), 5);
    assert_eq!(c["seeds"].as_array().unwrap().len(), 2);
    assert_eq!(c["seeds"][0]["weights"].as_array().unwrap().len(), 4);
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
