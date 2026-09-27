//! The deck data reader and the default play, on synthetic files.

mod common;

use ournotes_deck::Error;
use ournotes_deck::data::{DeckData, FORMAT};
use ournotes_deck::live::model::{JudgementStream, Play};
use ournotes_deck::live::score::PERFECT;
use serde_json::{Value, json};

/// Row objects to the file's `{"columns", "rows"}` form (columns in first-seen order).
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

/// Enumeration order (not time order): ids 1..=6; type 122 has no score percent and is not judged.
fn chart_json(score_id: i64) -> Value {
    json!({
        "scoreId": score_id,
        "asset": {"key": "Live/MusicScore/x", "sha256": "0".repeat(64)},
        "notes": {
            "id": [1, 2, 3, 5, 4, 6],
            "op": [1, 1, 21, 120, 1, 122],
            "judgementType": [1, 1, 21, 21, 1, 1],
            "timeMs": [1000, 1000, 1500, 3000, 2000, 3500],
        },
        "skillEvents": {"timeMs": [1200, 900, 2500, 2600, 3100]},
        "fevers": {"startMs": [800, 2400], "endMs": [1600, 3200]},
    })
}

fn document(full_combo: i64, charts: Vec<Value>) -> Value {
    let mut rng = common::Rng::new(7);
    let s = common::synth(&mut rng, 12, 4);
    let mut master = serde_json::Map::new();
    for (name, rows) in &s.tables {
        master.insert(name.clone(), columns(rows));
    }
    master.insert(
        "MasterLiveMusicScore".into(),
        columns(&json!([{"_id": 1004, "_musicScoreTextFileName": "x", "_musicScoreLevel": 24, "_fullComboCount": full_combo},
                         {"_id": 2003, "_musicScoreTextFileName": "y", "_musicScoreLevel": 18, "_fullComboCount": 0}])),
    );
    master.insert(
        "MasterLiveComboScoreBonus".into(),
        columns(&json!([{"_id": 1, "_comboBonusType": 0, "_requiredComboCount": 10, "_bonusFactor": 0.1},
                         {"_id": 2, "_comboBonusType": 1, "_requiredComboCount": 10, "_bonusFactor": 0.123456}])),
    );
    master.insert(
        "MasterLiveJudgementParameter".into(),
        columns(&json!([{"_id": 1, "_noteSimulateJudgement": 5, "_scorePercent": 100, "_damage": 0},
                         {"_id": 2, "_noteSimulateJudgement": 1, "_scorePercent": 0, "_damage": 100}])),
    );
    let mut settings = s.tables.iter().find(|(n, _)| n == "MasterLiveSettings").unwrap().1.clone();
    settings.as_array_mut().unwrap().push(json!({"_id": 9, "_key": "life_base", "_value": "1000"}));
    master.insert("MasterLiveSettings".into(), columns(&settings));
    json!({
        "format": FORMAT,
        "provenance": {"region": "test", "exporter": {"name": "test", "version": "0", "chartFormat": "test"}},
        "master": master,
        "charts": charts,
        "futureKey": 1,
    })
}

fn load(v: &Value) -> Result<DeckData, Error> {
    DeckData::from_json(&v.to_string())
}

#[test]
fn reads_a_file_and_computes_the_chart_counts() {
    let d = load(&document(5, vec![chart_json(1004)])).unwrap();
    let c = d.data_chart(1004).unwrap();
    assert_eq!(c.notes.len(), 6);
    assert_eq!(c.notes[3].id, 5);
    assert_eq!(c.fevers, vec![(800, 1600), (2400, 3200)]);
    let chart = d.chart(1004).unwrap();
    // score percents: 1 -> 100 (x3), 21 -> 15, 120 -> 5, 122 -> none: 320 / 100 -> ceil 3.2 = 4
    assert_eq!(chart.converted_note_count, 4);
    assert_eq!(chart.last_timing_note_ms, 3500);
    assert_eq!(chart.skill_events.iter().map(|e| (e.index, e.time_ms)).collect::<Vec<_>>()[1], (1, 900));
    assert!(d.chart(2003).is_err());
    assert!(d.chart(9).is_err());
}

#[test]
fn float_columns_are_binary32() {
    let text = document(5, vec![chart_json(1004)]).to_string().replace("0.123456", "1e999");
    let d = DeckData::from_json(&text).unwrap();
    assert_eq!(d.master.combo_score_bonuses[0].bonus_factor, 0.1f32);
    assert_eq!(d.master.combo_score_bonuses[1].bonus_factor, f32::INFINITY);
}

#[test]
fn rejects_bad_files() {
    let bad = |v: Value| load(&v).map(|_| ()).unwrap_err().to_string();
    let mut v = document(5, vec![chart_json(1004)]);
    v["format"] = json!("nnnotes.deck-data/2");
    assert!(bad(v).contains("not supported"));
    let mut v = document(5, vec![chart_json(1004)]);
    v["format"] = json!("something else");
    assert!(bad(v).contains("not a deck data file"));
    assert!(bad(document(6, vec![chart_json(1004)])).contains("full combo"));
    let mut v = document(5, vec![chart_json(1004)]);
    v["charts"][0]["notes"]["id"][1] = json!(1);
    assert!(bad(v).contains("listed twice"));
    let mut v = document(5, vec![chart_json(1004)]);
    v["charts"][0]["notes"]["timeMs"].as_array_mut().unwrap().pop();
    assert!(bad(v).contains("differ in length"));
    let mut v = document(5, vec![chart_json(1004)]);
    v["master"]["MasterLiveNoteParameter"]["rows"][0].as_array_mut().unwrap().pop();
    assert!(bad(v).contains("values for"));
    let mut v = document(5, vec![chart_json(1004)]);
    v["charts"][0]["fevers"]["startMs"] = json!([2400, 800]);
    assert!(bad(v).contains("sorted by start"));
    let mut two = chart_json(2003);
    two["notes"] = json!({"id": [1], "op": [122], "judgementType": [1], "timeMs": [5]});
    assert!(load(&document(5, vec![chart_json(1004), two.clone()])).is_ok());
    assert!(bad(document(5, vec![two, chart_json(1004)])).contains("sorted by score id"));
}

#[test]
fn theoretical_best_play() {
    let d = load(&document(5, vec![chart_json(1004)])).unwrap();
    let chart = d.chart(1004).unwrap();
    let p = Play::theoretical_best(&d.master, &chart).unwrap();
    // judged notes in chart time order; the combo counts the judged notes at earlier times
    let got: Vec<(i32, i32, i32)> = p.notes.iter().map(|n| (n.note_id, n.time_ms, n.combo)).collect();
    assert_eq!(got, vec![(1, 1000, 0), (2, 1000, 0), (3, 1500, 2), (4, 2000, 3), (5, 3000, 4)]);
    assert!(p.notes.iter().all(|n| n.score_type == PERFECT && n.life == 1000));
    assert_eq!(p.life_at_event, vec![1000; 5]);
    // a Perfect that costs life is not modelled
    let mut v = document(5, vec![chart_json(1004)]);
    let t = &mut v["master"]["MasterLiveJudgementParameter"];
    let col = t["columns"].as_array().unwrap().iter().position(|c| c == "_damage").unwrap();
    t["rows"][0][col] = json!(10);
    let d = load(&v).unwrap();
    assert!(matches!(Play::theoretical_best(&d.master, &chart), Err(Error::Unsupported(_))));
}

#[test]
fn theoretical_best_stream() {
    let d = load(&document(5, vec![chart_json(1004)])).unwrap();
    let chart = d.chart(1004).unwrap();
    let s = JudgementStream::theoretical_best(&chart);
    // 60 fps frames until 2 s after the last judged note or skill event (the event at 3100 ms)
    assert_eq!(&s.frames[..4], &[0, 16, 33, 50]);
    assert_eq!((s.frames.len(), *s.frames.last().unwrap()), (307, 5100));
    // every judged note Perfect in the first frame reaching its chart time, in (time, id) order
    assert_eq!(
        s.judged,
        vec![[60, 1, 5, 1000], [60, 2, 5, 1000], [90, 3, 5, 1500], [120, 4, 5, 2000], [180, 5, 5, 3000]]
    );
    assert_eq!((s.base_seed, s.assist), (0, false));
    let play = s.to_live_play().unwrap();
    assert_eq!(play.frames.iter().map(|f| f.judged.len()).sum::<usize>(), 5);
}
