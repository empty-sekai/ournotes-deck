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

/// The judged notes of a chart (its full combo).
fn judged_of(chart: &Value) -> i64 {
    let ops = chart["notes"]["op"].as_array().unwrap();
    ops.iter().filter(|o| ![0, 80, 82, 100, 103, 121, 122, 123].contains(&o.as_i64().unwrap())).count() as i64
}

/// The synthetic master and these charts; with `extra`, also live skill 4 (2000 on the confirmed rank, condition
/// 7012) and 5 (2000 on a Gekisou combo, condition 7005), one level each.
fn document_with(charts: Vec<Value>, extra: bool) -> Value {
    let mut rng = common::Rng::new(7);
    document_from(charts, extra, common::synth(&mut rng, 12, 4))
}

fn document_from(charts: Vec<Value>, extra: bool, mut s: common::Synth) -> Value {
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
    let scores: Vec<Value> = charts
        .iter()
        .map(|c| {
            json!({"_id": c["scoreId"], "_musicScoreTextFileName": "x", "_musicScoreLevel": 24,
                   "_fullComboCount": judged_of(c)})
        })
        .collect();
    master.insert("MasterLiveMusicScore".into(), columns(&Value::Array(scores)));
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
    (DeckData::from_json(&document_with(vec![chart], extra).to_string()).unwrap(), last)
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
            // the luck points (the luck mission's rank figure) come from the lottery alone without skills: 5 per
            // Hit, 10 per Super Hit or Critical
            let [_, hit, super_hit, critical] = r.lot_results;
            assert_eq!(r.luck_points, 5 * hit + 10 * (super_hit + critical), "range {i}");
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
    assert!(s.seeds.iter().all(|x| x.ranges[1].luck_points > 0 && x.ranges[0].luck_points == 0));

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

/// A combo range alone: nothing is drawn, so one seed; no Just-count range, so the Perfect play is the play; no luck
/// range, so no luck point and no lottery result.
#[test]
fn a_combo_range_alone_has_one_seed_without_just_or_luck() {
    let (d, _) = data_fevers(400, &FEVERS[..1]);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 8).unwrap();
    assert_eq!((s.ranges.len(), s.ranges[0].mission, s.just_notes), (1, 1, 0));
    assert_eq!(s.seeds.iter().map(|x| x.seed).collect::<Vec<_>>(), [0]);
    let seed = &s.seeds[0];
    assert_eq!(seed.score_perfect, seed.score);
    let r = &seed.ranges[0];
    assert!(r.range_score > 0 && r.max_combo > 0);
    assert_eq!((r.range_score_perfect, r.just_count), (r.range_score, 0));
    assert_eq!((r.luck_points, r.lot_results), (0, [0; 4]));
    assert!((seed.check.exact as f64 - seed.check.predicted).abs() <= seed.check.bound);
    let rc = seed.rank_check.as_ref().expect("a rank check");
    assert!((rc.exact as f64 - rc.predicted).abs() <= rc.bound, "{rc:?}");
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
    assert_eq!(v["format"], chartstats::FORMAT);
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
    // the luck points of every range, with or without a luck mission
    assert!(seed["ranges"].as_array().unwrap().iter().all(|r| r["luckPoints"].is_i64()));
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

/// The command line: `--charts` keeps these score ids, `--jobs N` measures N charts at once and writes the same
/// document as one at a time, the library's; `-o` writes it to a file.
#[test]
fn the_command_line_measures_the_charts_it_keeps() {
    let mut rng = common::Rng::new(11);
    let charts = vec![
        chart_json_fevers(1002, 120, &mut rng, &[]).0,
        chart_json_fevers(1003, 120, &mut rng, &FEVERS[..1]).0,
        chart_json_fevers(1004, 120, &mut rng, &FEVERS).0,
    ];
    let text = document_with(charts, false).to_string();
    let dir = std::env::temp_dir().join(format!("chart-stats-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("data.json");
    std::fs::write(&file, &text).unwrap();
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_ournotes-deck"))
            .arg("chart-stats")
            .arg("--data")
            .arg(&file)
            .args(extra)
            .output()
            .unwrap()
    };
    let doc = |extra: &[&str]| -> Value {
        let o = run(extra);
        assert!(o.status.success(), "{extra:?}: {}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let one = doc(&["--seeds", "2"]);
    assert_eq!(one["format"], chartstats::FORMAT);
    let ids: Vec<i64> = one["charts"].as_array().unwrap().iter().map(|c| c["scoreId"].as_i64().unwrap()).collect();
    assert_eq!(ids, [1002, 1003, 1004]);
    let data = DeckData::from_json(&text).unwrap();
    let options = chartstats::Options { seeds: 2, ..chartstats::Options::default() };
    assert_eq!(one, chartstats::document_with(&data, &options).unwrap());
    assert_eq!(doc(&["--seeds", "2", "--jobs", "2"]), one);
    // --charts keeps the listed charts in the file's order; unknown ids keep nothing
    let kept = doc(&["--seeds", "2", "--charts", "1004,1002,999", "--jobs", "3"]);
    assert_eq!(kept["charts"].as_array().unwrap(), &[one["charts"][0].clone(), one["charts"][2].clone()]);
    assert!(doc(&["--charts", "999"])["charts"].as_array().unwrap().is_empty());
    // -o writes the document and prints nothing
    let out = dir.join("out.json");
    let o = run(&["--seeds", "2", "--charts", "1003", "-o", out.to_str().unwrap()]);
    assert!(o.status.success() && o.stdout.is_empty(), "{}", String::from_utf8_lossy(&o.stderr));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written["charts"].as_array().unwrap(), &[one["charts"][1].clone()]);
    // bad values fail
    for bad in [&["--seeds", "0"][..], &["--jobs", "x"], &["--charts", "a,b"], &["--x"]] {
        let o = run(bad);
        assert!(!o.status.success() && o.stdout.is_empty(), "{bad:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
// Aptitude fixtures: short charts and synthetic skills, levels and bands only.
const APT_FEVERS: [(i32, i32); 3] = [(1000, 1800), (2800, 3600), (4600, 5400)];

fn aptitude_json(fevers: &[(i32, i32)]) -> Value {
    let mut s = common::synth(&mut common::Rng::new(7), 12, 6);
    common::extend_table(
        &mut s,
        "MasterSkillTarget",
        vec![
            json!({"_id":55,"_skillTargetType":5,"_gekisouMissionType":1}),
            json!({"_id":56,"_skillTargetType":5,"_gekisouMissionType":2}),
            json!({"_id":57,"_skillTargetType":5,"_gekisouMissionType":3}),
            json!({"_id":58,"_skillTargetType":3,"_bandID":1}),
            json!({"_id":59,"_skillTargetType":3,"_bandID":2}),
            json!({"_id":60,"_skillTargetType":4,"_judgement":4}),
        ],
    );
    common::extend_table(
        &mut s,
        "MasterSkillCondition",
        (0..5)
            .map(|i| {
                json!({
                    "_id":80+i,"_conditionType":if i<3 {7010} else {5000},"_conditionValues":[],
                    "_isPositive":true,"_conditionTargetIDs":[55+i]
                })
            })
            .collect(),
    );
    common::extend_table(
        &mut s,
        "MasterSkillConditionSet",
        (0..5)
            .map(|i| {
                json!({
                    "_id":250+i,"_group":250+i,"_conditionIds":[80+i]
                })
            })
            .collect(),
    );
    // 1 and 2 have identical highest-level effects, but different level numbers and lower-level effects.
    let skills =
        [(1, 1, 3, 12000, 8), (2, 1, 5, 12000, 8), (3, 2, 2, 11001, 30000), (4, 3, 2, 13000, 2), (5, 2, 2, 11002, 17)];
    common::replace_table(
        &mut s,
        "MasterGekisouSkill",
        skills.iter().map(|&(id, m, _, _, _)| json!({"_id":id,"_gekisouMissionType":m})).collect(),
    );
    let mut rows = Vec::new();
    for &(id, mission, level, ty, value) in &skills {
        for (lv, v) in [(1, 1), (level, value)] {
            rows.push(json!({"_id":rows.len()+1,"_gekisouSkillID":id,"_level":lv,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":249+mission,
                "_skillEffectType":ty,"_activationTimeSecond":1.2,"_effectValue":v}));
        }
    }
    common::replace_table(&mut s, "MasterGekisouSkillEffect", Value::Array(rows));
    common::set_column(&mut s, "MasterMemberCard", &mut |r| {
        r["_gekisouSkillID"] = json!(1 + (r["_id"].as_i64().unwrap() - 1) % 5);
    });
    // Support 1 and 2 differ only in band. Rank 5 chooses level 3, NOT the highest effect level 5.
    common::replace_table(
        &mut s,
        "MasterGekisouSupportSkill",
        (1..=5).map(|id| json!({"_id":id,"_gekisouMissionType":if id==5 {3} else {1}})).collect(),
    );
    let mut rows = Vec::new();
    for id in 1..=5 {
        for level in [1, 3, 5] {
            rows.push(json!({"_id":rows.len()+1,"_gekisouSupportSkillID":id,"_level":level,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":if id==5 {252} else {250},
                "_skillConditionGroup":if id<=2 {252+id} else {0},
                "_skillEffectType":match id {1|2=>2000,3=>12004,4=>12006,_=>4004},
                "_activationTimeSecond":1.2,"_effectValue":if id<=2 {1000*level} else {5},
                "_effectLimitCount":if id==3 || id==4 {3} else {0},
                "_skillTargetIDs":if id==3 || id==4 {vec![60]} else {vec![]}}));
        }
    }
    common::replace_table(&mut s, "MasterGekisouSupportSkillEffect", Value::Array(rows));
    common::set_column(&mut s, "MasterSupportCard", &mut |r| {
        r["_gekisouSupportSkillId01"] = json!(1 + (r["_id"].as_i64().unwrap() - 1) % 5);
    });
    common::set_column(&mut s, "MasterSupportCardRank", &mut |r| {
        r["_gekisouSupportSkill01Level"] = json!(if r["_rank"] == 5 { 3 } else { 1 });
    });
    let chart = json!({"scoreId":1004,"asset":{"key":"synthetic","sha256":"0".repeat(64)},
        "notes":{"id":(1..=64).collect::<Vec<_>>(),"op":vec![1;64],
            "judgementType":(1..=64).map(|i|if i%4==0 {21} else {1}).collect::<Vec<_>>(),
            "timeMs":(1..=64).map(|i|i*100).collect::<Vec<_>>()},
        "skillEvents":{"timeMs":[900,1200,2700,4500,5400]},
        "fevers":{"startMs":fevers.iter().map(|f|f.0).collect::<Vec<_>>(),
            "endMs":fevers.iter().map(|f|f.1).collect::<Vec<_>>()}});
    document_from(vec![chart], false, s)
}

fn aptitude_data(fevers: &[(i32, i32)]) -> DeckData {
    DeckData::from_json(&aptitude_json(fevers).to_string()).unwrap()
}

fn aptitude_options(max_seeds: usize, cross_seeds: usize) -> chartstats::Options {
    chartstats::Options { seeds: 4, aptitude: Some(chartstats::AptitudeOptions { max_seeds, cross_seeds }) }
}

fn shape_for(shapes: &[chartstats::Shape], source: &str, skill: i64) -> usize {
    shapes.iter().find(|s| s.source == source && s.skills.iter().any(|x| x.id == skill)).unwrap().id
}

#[test]
fn aptitude_shapes_deduplicate_effects_and_abstract_bands() {
    let d = aptitude_data(&APT_FEVERS);
    let h = chartstats::aptitude_header(&d.master, &chartstats::kinds(&d.master), &Default::default());
    assert_eq!(h.plain_kind, Some(0));
    assert_eq!(h.seed_rule.deterministic_test, 4);
    assert_eq!(h.seed_rule.batches, [32, 64, 128, 256, 512, 1024]);
    assert_eq!(h.seed_rule.cross_seeds, 64);
    assert_eq!((h.seed_rule.relative, h.seed_rule.baseline), (0.01, 0.001));
    assert_eq!(h.shapes.len(), 8);
    assert_eq!(h.shapes.iter().map(|s| s.id).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
    let member = &h.shapes[shape_for(&h.shapes, "member", 1)];
    assert_eq!(member.skills.iter().map(|s| (s.id, s.level)).collect::<Vec<_>>(), [(1, 3), (2, 5)]);
    assert!(!member.band_condition);
    assert!(member.skills.iter().all(|s| s.member_target_ids.is_none() && s.band_ids.is_none()));
    let support = &h.shapes[shape_for(&h.shapes, "support", 1)];
    assert_eq!(support.id, shape_for(&h.shapes, "support", 2));
    assert!(support.band_condition);
    assert_eq!(support.skills.iter().map(|s| (s.id, s.level)).collect::<Vec<_>>(), [(1, 3), (2, 3)]);
    for (i, s) in support.skills.iter().enumerate() {
        assert_eq!(s.member_target_ids, Some(vec![58 + i as i64]));
        assert_eq!(s.band_ids, Some(vec![1 + i as i64]));
    }
    assert_eq!(support.effects[0].condition[0][0].condition_type, 5000);
    assert!(support.effects[0].condition[0][0].target_ids.is_none());
    assert_eq!(support.effects[0].effect_value, 3000);
    let mut empty = d;
    empty.charts.clear();
    let doc = chartstats::document_with(&empty, &aptitude_options(48, 7)).unwrap();
    assert!(doc["charts"].as_array().unwrap().is_empty());
    assert_eq!(doc["gekisouAptitude"]["seedRule"]["batches"], json!([32, 48]));
    assert_eq!(doc["gekisouAptitude"]["shapes"], serde_json::to_value(h.shapes).unwrap());
}

#[test]
fn aptitude_variants_gate_missions_measure_bands_and_zero_effects() {
    let d = aptitude_data(&APT_FEVERS[..1]);
    let shapes = chartstats::shapes(&d.master);
    let s =
        chartstats::chart_stats_with(&d.master, &d.charts[0], &chartstats::kinds(&d.master), &aptitude_options(32, 4))
            .unwrap();
    let a = s.gekisou_aptitude.as_ref().unwrap();
    assert_eq!(a.variants.len(), 5);
    let keys: Vec<_> = a.variants.iter().map(|v| (v.shape, v.band_match)).collect();
    let support = shape_for(&shapes, "support", 1);
    assert_eq!(
        keys,
        vec![
            (shape_for(&shapes, "member", 1), None),
            (support, Some(true)),
            (support, Some(false)),
            (shape_for(&shapes, "support", 3), None),
            (shape_for(&shapes, "support", 4), None)
        ]
    );
    for v in &a.variants {
        assert_eq!(shapes[v.shape].mission, 1);
        assert!(v.deterministic && v.se_target_met);
        assert_eq!((v.seeds, v.cross_seeds, v.check.seed), (1, 1, 0));
        assert_eq!(v.score, v.score_perfect);
        assert_eq!(v.tail, v.tail_perfect);
        assert_eq!(v.score[1], 0.0);
        assert_eq!(v.converted, [0.0, 0.0]);
        assert_eq!(v.ranges[0].luck_points, [0.0, 0.0]);
        assert!(
            (v.score[0] - v.tail[0] - v.ranges.iter().map(|r| r.range_score[0] + r.rank_bonus[0]).sum::<f64>()).abs()
                < 1e-9
        );
        assert!((v.check.exact as f64 - v.check.predicted).abs() <= v.check.bound, "{v:?}");
        assert!(v.check.ranks.iter().all(|r| (1..=5).contains(r)));
        assert_eq!(v.weights.as_ref().unwrap().len(), 5);
        assert_eq!(v.range_weights.as_ref().unwrap().len(), 5);
        if v.band_match == Some(false) || [12004, 12006].contains(&shapes[v.shape].effects[0].effect_type) {
            assert_eq!(v.score, [0.0, 0.0]);
            assert_eq!(v.tail, [0.0, 0.0]);
            assert!(v.weights.as_ref().unwrap().iter().all(|w| *w == [0.0, 0.0]));
        }
    }
    assert!(a.variants.iter().find(|v| v.shape == support && v.band_match == Some(true)).unwrap().score[0] > 0.0);
    assert_eq!((a.factors[0].just_notes, a.factors[0].perfect_notes, a.factors[0].lotteries), (0, 0, [0.0, 0.0]));
}

/// Direct engine run, assembling a performer independently of the aptitude implementation.
fn aptitude_run(
    d: &DeckData,
    performer: Option<Performer>,
    seed: i32,
    perfect: bool,
) -> (i32, Vec<full::GekisouRange>) {
    let chart = d.chart(1004).unwrap();
    let setup = GekisouSetup { fevers: d.charts[0].fevers.clone(), missions: vec![1, 2, 3] };
    let rule = JustRule::new(&d.master, &setup).unwrap();
    let mut stream = JudgementStream::theoretical_best_gekisou(&chart, &d.charts[0].judgement_types, &rule).unwrap();
    if perfect {
        for j in &mut stream.judged {
            if j[2] == 6 {
                j[2] = 5;
            }
        }
    }
    let dt = stream.delta_times().unwrap();
    let mut play = stream.to_live_play().unwrap();
    play.base_seed = seed;
    let params = LiveParams {
        total_power: POWER,
        music_level: 24,
        converted_note_count: chart.converted_note_count,
        music_length_ms: chart.last_timing_note_ms + 1000,
        score_music_length_ms: None,
        assist_factor: 1.0,
        skill_target_music_type: 1,
    };
    let mut deck: Vec<_> = performer.into_iter().collect();
    deck.resize(5, Performer::default());
    let events: Vec<_> = chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect();
    let mut model = full::LiveModel::new_gekisou(&d.master, &deck, &notes_of(d), &events, params, &setup).unwrap();
    let score = model.run_timed(&play, &dt).unwrap();
    (score, model.gekisou_ranges())
}

#[test]
fn aptitude_support_uses_an_empty_host_and_exact_tail() {
    let mut d = aptitude_data(&APT_FEVERS[..1]);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats_with(&d.master, &d.charts[0], &kinds, &aptitude_options(32, 4)).unwrap();
    let shape = shape_for(&chartstats::shapes(&d.master), "support", 1);
    let v = s
        .gekisou_aptitude
        .as_ref()
        .unwrap()
        .variants
        .iter()
        .find(|v| v.shape == shape && v.band_match == Some(true))
        .unwrap();
    // A different synthetic id: the host's identity is irrelevant; its lack of effects is not.
    d.master.gekisou_skills.push(ournotes_deck::master::SkillRow {
        id: -999,
        gekisou_mission_type: 1,
        ..Default::default()
    });
    d.master.reindex().unwrap();
    let host = Performer {
        band_id: 1,
        gekisou_skill: Some((-999, 1)),
        gekisou_support_skills: vec![(1, 3)],
        ..Default::default()
    };
    let (base, br) = aptitude_run(&d, None, 0, false);
    let (with, wr) = aptitude_run(&d, Some(host.clone()), 0, false);
    let rs = |r: &full::GekisouRange| r.end_score - r.start_score;
    assert_eq!(v.score, [f64::from(with - base), 0.0]);
    assert_eq!(v.ranges[0].range_score, [f64::from(rs(&wr[0]) - rs(&br[0])), 0.0]);
    assert_eq!(v.tail[0], f64::from(with - base) - v.ranges[0].range_score[0] - v.ranges[0].rank_bonus[0]);
    assert!(v.tail[0] > 0.0, "skill lasts beyond the range end");
    let (without_host, _) = aptitude_run(&d, Some(Performer { gekisou_skill: None, ..host.clone() }), 0, false);
    assert_eq!(without_host, base, "support is gated off without a member skill");
    let (real_host, _) = aptitude_run(&d, Some(Performer { gekisou_skill: Some((1, 3)), ..host }), 0, false);
    assert_ne!(real_host, with, "borrowing a real card contaminates the single-skill measurement");
}

fn test_mean_se(values: &[f64]) -> [f64; 2] {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    [
        mean,
        if n == 1.0 { 0.0 } else { (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n * (n - 1.0))).sqrt() },
    ]
}

#[test]
fn aptitude_seed_statistics_match_paired_full_runs() {
    let d = aptitude_data(&APT_FEVERS);
    let shapes = chartstats::shapes(&d.master);
    let options = aptitude_options(64, 4);
    let s = chartstats::chart_stats_with(&d.master, &d.charts[0], &[], &options).unwrap();
    let a = s.gekisou_aptitude.as_ref().unwrap();
    let v = a.variants.iter().find(|v| v.shape == shape_for(&shapes, "member", 3)).unwrap();
    assert!(!v.deterministic);
    assert!([32, 64].contains(&v.seeds));
    assert!(v.weights.is_none() && v.range_weights.is_none());
    assert!(v.check.deck.iter().all(Option::is_none));
    let mut scores = Vec::new();
    let mut bases = Vec::new();
    let mut tails = Vec::new();
    for seed in ournotes_deck::live::seeds::published_seeds(v.seeds) {
        let (base, br) = aptitude_run(&d, None, seed, false);
        let (with, wr) =
            aptitude_run(&d, Some(Performer { gekisou_skill: Some((3, 2)), ..Default::default() }), seed, false);
        scores.push(f64::from(with - base));
        bases.push(f64::from(base));
        let range_delta: i32 = wr
            .iter()
            .zip(br)
            .map(|(x, y)| {
                x.end_score - x.start_score - y.end_score + y.start_score + x.rank_bonus.unwrap_or(0)
                    - y.rank_bonus.unwrap_or(0)
            })
            .sum();
        tails.push(f64::from(with - base - range_delta));
    }
    for (reported, actual) in [(v.score, test_mean_se(&scores)), (v.tail, test_mean_se(&tails))] {
        for i in 0..2 {
            assert!((reported[i] - actual[i]).abs() <= 0.000501, "{reported:?} {actual:?}");
        }
    }
    let met = |scores: &[f64], bases: &[f64]| {
        let [m, se] = test_mean_se(scores);
        se <= (m.abs() * 0.01).max(test_mean_se(bases)[0] * 0.001)
    };
    assert_eq!(v.se_target_met, met(&scores, &bases));
    if v.seeds == 64 {
        assert!(!met(&scores[..32], &bases[..32]));
    }
    assert!(
        (v.score[0] - v.tail[0] - v.ranges.iter().map(|r| r.range_score[0] + r.rank_bonus[0]).sum::<f64>()).abs()
            < 0.004
    );
    for v in &a.variants {
        assert!((v.check.exact as f64 - v.check.predicted).abs() <= v.check.bound);
        if [13000, 11002, 4004].contains(&shapes[v.shape].effects[0].effect_type) {
            assert_eq!(v.score, [0.0, 0.0]);
        }
        if v.deterministic {
            assert_eq!((v.seeds, v.se_target_met, v.score[1]), (1, true, 0.0));
        }
    }
    let just = a.variants.iter().find(|v| v.shape == shape_for(&shapes, "member", 4)).unwrap();
    assert!(just.ranges[2].just_count[0] > 0.0);
    let points = a.variants.iter().find(|v| v.shape == shape_for(&shapes, "member", 5)).unwrap();
    assert_eq!(points.ranges[1].luck_points, [17.0, 0.0]);
    // Factors' lottery moments use the baseline seeds, not the adaptive variant seed count.
    let lots: Vec<_> = s.seeds.iter().map(|s| s.ranges[1].lot_results.iter().sum::<i32>() as f64).collect();
    assert_eq!(a.factors[1].lotteries, test_mean_se(&lots));
    assert_eq!(a.factors[2].judged_notes, a.factors[2].just_notes + a.factors[2].perfect_notes);
    assert!(a.factors[2].just_notes > 0 && a.factors[2].perfect_notes > 0);
}

fn assert_keys(v: &Value, keys: &[&str]) {
    let actual: std::collections::BTreeSet<_> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(actual, keys.iter().copied().collect());
}

#[test]
fn aptitude_document_serialization_and_null_cases() {
    let d = aptitude_data(&APT_FEVERS[..1]);
    let options = aptitude_options(32, 4);
    let v = chartstats::document_with(&d, &options).unwrap();
    assert_eq!(v["format"], "ournotes-deck.chart-stats/2");
    assert!(v["model"]["gekisouAptitude"].as_str().is_some_and(|s| !s.contains("  ")));
    let h = &v["gekisouAptitude"];
    assert_keys(h, &["plainKind", "host", "seedRule", "shapes"]);
    assert_keys(&h["seedRule"], &["deterministicTest", "batches", "relative", "baseline", "crossSeeds"]);
    let shape = &h["shapes"][0];
    assert_keys(shape, &["id", "source", "mission", "bandCondition", "effects", "skills"]);
    assert_keys(&shape["skills"][0], &["id", "level", "memberTargetIds", "bandIds"]);
    assert_keys(
        &shape["effects"][0],
        &[
            "effectType",
            "triggerType",
            "activationTimeSecond",
            "effectValue",
            "maxEffectValue",
            "effectLimitCount",
            "effectExecuteLimitCount",
            "skillTargetIds",
            "trigger",
            "condition",
            "release",
            "reset",
            "cumulative",
        ],
    );
    assert_keys(&shape["effects"][0]["trigger"][0][0], &["type", "values", "positive", "targetIds"]);
    let a = &v["charts"][0]["gekisouAptitude"];
    assert_keys(a, &["factors", "variants"]);
    assert_keys(
        &a["factors"][0],
        &["judgedNotes", "justNotes", "perfectNotes", "tailNotes", "comboAtStart", "lotteries"],
    );
    let x = &a["variants"][0];
    assert_keys(
        x,
        &[
            "shape",
            "bandMatch",
            "deterministic",
            "seeds",
            "seTargetMet",
            "crossSeeds",
            "score",
            "scorePerfect",
            "tail",
            "tailPerfect",
            "converted",
            "ranges",
            "weights",
            "rangeWeights",
            "check",
        ],
    );
    assert_keys(
        &x["ranges"][0],
        &["rangeScore", "rankBonus", "rangeScorePerfect", "maxCombo", "justCount", "luckPoints"],
    );
    assert_keys(&x["check"], &["seed", "ranks", "deck", "exact", "predicted", "bound"]);
    for key in ["score", "scorePerfect", "tail", "tailPerfect", "converted"] {
        assert_eq!(x[key].as_array().unwrap().len(), 2);
    }
    assert!(v["charts"][0].get("gekisou").is_none());
    let disabled = chartstats::document_with(&d, &chartstats::Options { aptitude: None, ..options }).unwrap();
    assert_eq!(disabled.get("gekisouAptitude"), Some(&Value::Null));
    assert_eq!(disabled["charts"][0].get("gekisouAptitude"), Some(&Value::Null));
    assert_eq!(v["charts"][0]["seeds"], disabled["charts"][0]["seeds"]);
    assert_eq!(v["charts"][0]["offSeeds"], disabled["charts"][0]["offSeeds"]);
    for fevers in [vec![], vec![(1000, 1800), (2800, 3600), (4600, 5400), (5800, 6000)]] {
        let data = aptitude_data(&fevers);
        let result = chartstats::document_with(&data, &options).unwrap();
        assert_eq!(result["charts"][0].get("gekisouAptitude"), Some(&Value::Null));
    }
    let (plain, _) = data_fevers(40, &FEVERS);
    let no_skills = chartstats::document_with(&plain, &options).unwrap();
    assert_eq!(no_skills["charts"][0].get("gekisouAptitude"), Some(&Value::Null));
}

#[test]
fn aptitude_rare_probability_is_not_proven_deterministic_by_four_equal_samples() {
    let mut d = aptitude_data(&APT_FEVERS[..1]);
    // A 1% score-up on range start: the first four published seeds happen to miss it.
    d.master.skill_conditions.push(
        serde_json::from_value(json!({"_id":900,"_conditionType":4011,
        "_conditionValues":[1],"_conditionTargetIDs":[],"_isPositive":true}))
        .unwrap(),
    );
    d.master
        .skill_condition_sets
        .push(serde_json::from_value(json!({"_id":900,"_group":900,"_conditionIds":[80,900]})).unwrap());
    d.master.gekisou_skill_effects.retain(|r| r.skill_id == 1);
    d.master.gekisou_support_skill_effects.clear();
    for r in &mut d.master.gekisou_skill_effects {
        r.skill_trigger_condition_group = 900;
        r.skill_effect_type = 2000;
        r.effect_value = 10000;
    }
    d.master.reindex().unwrap();
    for seed in ournotes_deck::live::seeds::published_seeds(4) {
        let (base, _) = aptitude_run(&d, None, seed, false);
        let (with, _) =
            aptitude_run(&d, Some(Performer { gekisou_skill: Some((1, 3)), ..Default::default() }), seed, false);
        assert_eq!(with, base, "fixture must have four equal zero increments");
    }
    let s = chartstats::chart_stats_with(&d.master, &d.charts[0], &[], &aptitude_options(32, 4)).unwrap();
    let v = &s.gekisou_aptitude.as_ref().unwrap().variants[0];
    assert!(!v.deterministic, "four equal samples do not prove a probability condition deterministic");
    assert_eq!(v.seeds, 32);
}

#[test]
fn aptitude_cli_flags_disable_and_bound_measurements() {
    let dir = std::env::temp_dir().join(format!("aptitude-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.json");
    std::fs::write(&path, aptitude_json(&APT_FEVERS).to_string()).unwrap();
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_ournotes-deck"))
            .args(["chart-stats", "--data"])
            .arg(&path)
            .args(extra)
            .output()
            .unwrap()
    };
    let o = run(&["--no-gekisou-aptitude"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let off: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(off.get("gekisouAptitude"), Some(&Value::Null));
    assert_eq!(off["charts"][0].get("gekisouAptitude"), Some(&Value::Null));
    let o = run(&["--aptitude-max-seeds", "32", "--aptitude-cross-seeds", "2"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["gekisouAptitude"]["seedRule"]["batches"], json!([32]));
    assert_eq!(v["gekisouAptitude"]["seedRule"]["crossSeeds"], 2);
    for x in v["charts"][0]["gekisouAptitude"]["variants"].as_array().unwrap() {
        let seeds = x["seeds"].as_u64().unwrap();
        assert!(seeds == 1 || seeds == 32);
        assert_eq!(x["crossSeeds"].as_u64().unwrap(), seeds.min(2));
        assert!(x["check"]["predicted"].is_number());
    }
    for args in [
        ["--aptitude-max-seeds", "0"],
        ["--aptitude-max-seeds", "1"],
        ["--aptitude-max-seeds", "x"],
        ["--aptitude-cross-seeds", "0"],
        ["--aptitude-cross-seeds", "x"],
    ] {
        assert!(!run(&args).status.success());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn aptitude_rejects_empty_sampling_at_both_library_entry_points() {
    let d = aptitude_data(&APT_FEVERS[..1]);
    let kinds = chartstats::kinds(&d.master);
    for options in [
        aptitude_options(32, 0),
        aptitude_options(0, 4),
        aptitude_options(1, 4),
        chartstats::Options { seeds: 0, ..aptitude_options(32, 4) },
    ] {
        assert!(chartstats::document_with(&d, &options).is_err(), "{options:?}");
        assert!(chartstats::chart_stats_with(&d.master, &d.charts[0], &kinds, &options).is_err(), "{options:?}");
        let mut empty = d.clone();
        empty.charts.clear();
        assert!(chartstats::document_with(&empty, &options).is_err(), "empty file: {options:?}");
    }
}

#[test]
fn aptitude_without_plain_kind_has_no_cross_terms() {
    let d = aptitude_data(&APT_FEVERS[..1]);
    let s = chartstats::chart_stats_with(&d.master, &d.charts[0], &[], &aptitude_options(32, 4)).unwrap();
    for v in &s.gekisou_aptitude.as_ref().unwrap().variants {
        assert!(v.weights.is_none() && v.range_weights.is_none());
        assert_eq!(v.cross_seeds, 0);
        assert!(v.check.deck.iter().all(Option::is_none));
        assert!(v.check.predicted.is_finite() && v.check.bound.is_finite() && v.check.bound >= 0.0);
    }
}

#[test]
fn aptitude_cumulative_just_bonus_changes_indicators_not_score() {
    let mut d = aptitude_data(&APT_FEVERS);
    d.master.gekisou_skill_effects.retain(|r| r.skill_id == 4);
    d.master.gekisou_support_skill_effects.clear();
    d.master.cumulative_conditions.push(
        serde_json::from_value(json!({"_id":900,
        "_skillCumulativeConditionType":7000,"_conditionValues":[2],"_conditionTargetIDs":[],
        "_maxCumulativeCount":100}))
        .unwrap(),
    );
    for r in &mut d.master.gekisou_skill_effects {
        r.skill_effect_type = 13002;
        r.skill_cumulative_condition_id = 900;
        r.effect_value = 1;
        r.max_effect_value = 100;
    }
    d.master.reindex().unwrap();
    let h = chartstats::aptitude_header(&d.master, &[], &Default::default());
    let cumulative = serde_json::to_value(&h.shapes[0].effects[0].cumulative).unwrap();
    assert_keys(&cumulative, &["type", "values", "targetIds", "maxCumulativeCount"]);
    assert_eq!(cumulative["values"], json!([2]));
    let s = chartstats::chart_stats_with(&d.master, &d.charts[0], &[], &aptitude_options(32, 4)).unwrap();
    let v = &s.gekisou_aptitude.as_ref().unwrap().variants[0];
    assert_eq!(v.score, [0.0, 0.0]);
    assert_eq!(v.score_perfect, [0.0, 0.0]);
    assert!(v.ranges[2].just_count[0] > 0.0);
}
