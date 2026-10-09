//! Chart statistics measured on the whole-live simulation with Gekisou on and off, on a synthetic deck data file.

#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

#[path = "chartstats/program_cache.rs"]
mod program_cache;

use ournotes_sim::chartstats::{self, POWER};
use ournotes_sim::data::{DeckData, FORMAT};
use ournotes_sim::live::full::{self, GekisouSetup, LiveNote, LiveParams, Performer};
use ournotes_sim::live::model::{JudgementStream, JustRule, LiveModel, Play};
use ournotes_sim::live::score::{ComboTable, LiveScoreSettings};
use ournotes_sim::live::skip::skip_score;
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
    common::every_table(&mut master);
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

/// The kind of a live skill effect row.
fn kind_of(kinds: &[chartstats::Kind], row: &ournotes_sim::master::LiveSkillEffectRow) -> usize {
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
    assert!((off.score as f64) < s.expectation.as_ref().unwrap().score[0]);
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
                "_skillEffectType":ty,"_activationTimeSecond":if ty == 11002 {0.0} else {1.2},"_effectValue":v}));
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

fn shape_for(shapes: &[chartstats::Shape], source: &str, skill: i64) -> usize {
    shapes.iter().find(|s| s.source == source && s.skills.iter().any(|x| x.id == skill)).unwrap().id
}

#[test]
fn aptitude_shapes_deduplicate_effects_and_abstract_bands() {
    let d = aptitude_data(&APT_FEVERS);
    let h = chartstats::aptitude_header(&d.master, &chartstats::kinds(&d.master));
    assert_eq!(h.plain_kind, Some(0));
    assert_eq!(h.law, "independent nominal lottery and skill probabilities");
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
    let doc = chartstats::document_with(&empty, &chartstats::Options::default()).unwrap();
    assert!(doc["charts"].as_array().unwrap().is_empty());
    assert_eq!(doc["gekisouAptitude"]["law"], h.law);
    assert_eq!(doc["gekisouAptitude"]["shapes"], serde_json::to_value(h.shapes).unwrap());
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

#[derive(Clone, Copy, Debug)]
enum ChanceGate {
    Single(i64, bool),
    Both,
    Either,
    Never,
}

/// Each leaf specifies the actual SKILL comparisons visited by the Boolean expression. Rates are the
/// binary32 values of the condition contract; leaf scores come from complete ordinary engine runs.
fn chance_leaves(gate: ChanceGate) -> Vec<(Vec<(i64, bool)>, f64)> {
    let p = |percent| f64::from(percent as f32 / 100.0);
    match gate {
        ChanceGate::Single(percent, _) => {
            vec![(vec![(percent, false)], 1.0 - p(percent)), (vec![(percent, true)], p(percent))]
        }
        ChanceGate::Both => vec![
            (vec![(50, false)], 0.5),
            (vec![(50, true), (25, false)], 0.5 * 0.75),
            (vec![(50, true), (25, true)], 0.5 * 0.25),
        ],
        ChanceGate::Either => vec![
            (vec![(50, true)], 0.5),
            (vec![(50, false), (25, false)], 0.5 * 0.75),
            (vec![(50, false), (25, true)], 0.5 * 0.25),
        ],
        ChanceGate::Never => vec![(vec![], 1.0)],
    }
    .into_iter()
    .filter(|(_, mass)| *mass > 0.0)
    .collect()
}

fn chance_data(fevers: &[(i32, i32)], gate: ChanceGate, missions: [i64; 3]) -> DeckData {
    let mut d = aptitude_data(fevers);
    for (id, percent, positive) in match gate {
        ChanceGate::Single(percent, positive) => vec![(900, percent, positive)],
        _ => vec![(900, 50, true), (901, 25, true)],
    } {
        d.master.skill_conditions.push(
            serde_json::from_value(json!({"_id":id,"_conditionType":4011,
                "_conditionValues":[percent],"_conditionTargetIDs":[],"_isPositive":positive}))
            .unwrap(),
        );
    }
    d.master
        .skill_conditions
        .push(serde_json::from_value(json!({"_id":902,"_conditionType":8000,"_isPositive":true})).unwrap());
    let sets = match gate {
        ChanceGate::Single(..) => vec![vec![80, 900]],
        ChanceGate::Both => vec![vec![80, 900, 901]],
        ChanceGate::Either => vec![vec![80, 900], vec![80, 901]],
        ChanceGate::Never => vec![vec![80, 902, 900]],
    };
    for (i, ids) in sets.into_iter().enumerate() {
        d.master
            .skill_condition_sets
            .push(serde_json::from_value(json!({"_id":900+i,"_group":900,"_conditionIds":ids})).unwrap());
    }
    d.master.gekisou_skill_effects.retain(|row| row.skill_id == 1);
    d.master.gekisou_support_skill_effects.clear();
    for row in &mut d.master.gekisou_skill_effects {
        row.skill_trigger_condition_group = 900;
        row.skill_effect_type = 2000;
        row.effect_value = 10000;
        row.activation_time_second = 5.0;
    }
    for music in &mut d.master.live_musics {
        [music.gekisou_mission_1, music.gekisou_mission_2, music.gekisou_mission_3] = missions;
    }
    // A deterministic Critical lottery enters Rush, so probability-gated score commands overlap Rush
    // without adding lottery branches to the independent SKILL event tree.
    for row in &mut d.master.gekisou_luck_base_points {
        row.base_point = 70;
    }
    for row in &mut d.master.gekisou_luck_bonus_lots {
        row.weight = i64::from(row.lot_result == 3);
    }
    d.master.live_skills.push(serde_json::from_value(json!({"_id":903,"_skillCategories":[1]})).unwrap());
    d.master.live_skill_effects.push(
        serde_json::from_value(json!({"_id":903,"_liveSkillID":903,"_level":1,
            "_skillEffectType":2000,"_activationTimeSecond":5.0,"_effectValue":10000}))
        .unwrap(),
    );
    d.master.reindex().unwrap();
    d
}

#[derive(Clone, Debug)]
struct ChanceMean {
    score: f64,
    ranges: Vec<[f64; 3]>,
}

impl ChanceMean {
    fn tail(&self) -> f64 {
        self.score - self.ranges.iter().map(|range| range[0] + range[1]).sum::<f64>()
    }
}

/// Select a native stream whose next values force this finite comparison prefix. The nominal mass
/// comes from the specified leaves, never from the frequency of seeds that realize a prefix.
fn chance_seed(samples: &[(i64, bool)]) -> i32 {
    use ournotes_sim::live::random::{LiveRandom, SKILL};
    (0..65536)
        .find(|&seed| {
            let mut random = LiveRandom::new(seed);
            samples.iter().all(|&(percent, success)| (random.value(SKILL) < percent as f32 / 100.0) == success)
        })
        .expect("a native stream realizes this finite comparison prefix")
}

/// Enumerate one gate at each mission-1 Start frame, retaining complete native updater state between
/// frames. Checking the per-frame draw count rejects unexpected checks, including failed short circuits.
fn chance_mean(
    d: &DeckData,
    gate: ChanceGate,
    missions: [i64; 3],
    with_shape: bool,
    plain_position: Option<usize>,
    perfect: bool,
) -> ChanceMean {
    use ournotes_sim::live::random::LiveRandom;
    let chart = d.chart(1004).unwrap();
    let notes = notes_of(d);
    let events: Vec<_> = chart.skill_events.iter().map(|event| (event.index, event.time_ms)).collect();
    let setup = GekisouSetup { fevers: d.charts[0].fevers.clone(), missions: missions.into() };
    let rule = JustRule::new(&d.master, &setup).unwrap();
    let mut stream = JudgementStream::theoretical_best_gekisou(&chart, &d.charts[0].judgement_types, &rule).unwrap();
    if perfect {
        for judgement in &mut stream.judged {
            if judgement[2] == 6 {
                judgement[2] = 5;
            }
        }
    }
    let play = stream.to_live_play().unwrap();
    let dt = stream.delta_times().unwrap();
    let params = LiveParams {
        total_power: POWER,
        music_level: 24,
        converted_note_count: chart.converted_note_count,
        music_length_ms: chart.last_timing_note_ms + 1000,
        score_music_length_ms: None,
        assist_factor: 1.0,
        skill_target_music_type: 1,
    };
    let mut recorder = full::LiveModel::new_gekisou(&d.master, &[], &notes, &events, params, &setup).unwrap();
    let starts: Vec<_> = recorder
        .record_range_frames(&play, &dt)
        .unwrap()
        .iter()
        .zip(missions)
        .filter_map(|(frames, mission)| (mission == 1).then_some(frames.start))
        .collect();
    assert_eq!(starts.len(), setup.fevers.iter().zip(missions).filter(|(_, mission)| *mission == 1).count());
    let leaves: Vec<_> = chance_leaves(gate)
        .into_iter()
        .map(|(samples, mass)| (chance_seed(&samples), samples.len() as u64, mass))
        .collect();
    assert!((leaves.iter().map(|leaf| leaf.2).sum::<f64>() - 1.0).abs() < 1e-15);
    let mut baseline = full::LiveModel::new_gekisou(&d.master, &[], &notes, &events, params, &setup).unwrap();
    let mut native_draws = Vec::with_capacity(play.frames.len());
    for (frame, &delta) in play.frames.iter().zip(&dt) {
        baseline.set_random(LiveRandom::new(0));
        baseline.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
        native_draws.push(baseline.draws());
    }
    if missions[..setup.fevers.len()].contains(&2) {
        assert!(!baseline.rush_command_spans().is_empty());
    }
    let mut deck = vec![Performer::default(); 5];
    if with_shape {
        deck[0].gekisou_skill = Some((1, 3));
        deck[0].gekisou_mission_type = 1;
    }
    if let Some(position) = plain_position {
        deck[position].live_skill = Some((903, 1));
    }
    let initial = full::LiveModel::new_gekisou(&d.master, &deck, &notes, &events, params, &setup).unwrap();
    let mut paths = vec![(initial, 1.0)];
    for (i, (frame, &delta)) in play.frames.iter().zip(&dt).enumerate() {
        if with_shape && starts.contains(&i) {
            let mut next = Vec::new();
            for (state, mass) in paths {
                for &(seed, checks, chance) in &leaves {
                    let mut branch = state.clone();
                    branch.set_random(LiveRandom::new(seed));
                    branch.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
                    assert_eq!(branch.draws(), native_draws[i] + checks, "frame {i}, gate {gate:?}");
                    next.push((branch, mass * chance));
                }
            }
            paths = next;
        } else {
            for (state, _) in &mut paths {
                state.set_random(LiveRandom::new(0));
                state.frame_timed(frame.time_ms, &frame.judged, delta).unwrap();
                assert_eq!(state.draws(), native_draws[i], "frame {i}, gate {gate:?}");
            }
        }
    }
    assert_eq!(paths.len(), if with_shape { leaves.len().pow(starts.len() as u32) } else { 1 });
    assert!((paths.iter().map(|path| path.1).sum::<f64>() - 1.0).abs() < 1e-14);
    let mut result = ChanceMean { score: 0.0, ranges: vec![[0.0; 3]; setup.fevers.len()] };
    for (state, mass) in paths {
        result.score += mass * f64::from(state.score());
        for (mean, range) in result.ranges.iter_mut().zip(state.gekisou_ranges()) {
            mean[0] += mass * f64::from(range.end_score - range.start_score);
            mean[1] += mass * f64::from(range.rank_bonus.unwrap());
            mean[2] += mass * f64::from(range.luck_points);
        }
    }
    result
}

fn encloses_chance(estimate: [f64; 2], expected: f64) {
    assert!(
        estimate[0] - estimate[1] - 1e-8 <= expected && expected <= estimate[0] + estimate[1] + 1e-8,
        "expected {expected}, measured {estimate:?}"
    );
}

fn check_chance_aptitude(d: &DeckData, gate: ChanceGate, missions: [i64; 3], check_cross: bool) {
    let kinds = chartstats::kinds(&d.master);
    let plain = kinds.iter().find(|kind| kind.effect_type == 2000 && kind.skill_condition_group == 0).unwrap();
    let stats = chartstats::chart_stats_with(
        &d.master,
        &d.charts[0],
        std::slice::from_ref(plain),
        &chartstats::Options { replay_seeds: 1, ..Default::default() },
    )
    .unwrap();
    let aptitude = stats.gekisou_aptitude.as_ref().unwrap();
    assert_eq!(aptitude.variants.len(), 1);
    let variant = &aptitude.variants[0];
    checked(&variant.check);
    assert!(variant.range_weights.is_some());
    assert_eq!(variant.converted, [0.0; 2]);
    assert!(variant.ranges.iter().all(|range| range.max_combo == [0.0; 2] && range.just_count == [0.0; 2]));
    for perfect in [false, true] {
        let baseline = chance_mean(d, gate, missions, false, None, perfect);
        let with = chance_mean(d, gate, missions, true, None, perfect);
        encloses_chance(if perfect { variant.score_perfect } else { variant.score }, with.score - baseline.score);
        encloses_chance(if perfect { variant.tail_perfect } else { variant.tail }, with.tail() - baseline.tail());
        for ((actual, with), base) in variant.ranges.iter().zip(&with.ranges).zip(&baseline.ranges) {
            encloses_chance(if perfect { actual.range_score_perfect } else { actual.range_score }, with[0] - base[0]);
            encloses_chance(if perfect { actual.rank_bonus_perfect } else { actual.rank_bonus }, with[1] - base[1]);
            encloses_chance(actual.luck_points, with[2] - base[2]);
        }
        if check_cross && !perfect {
            for position in 0..5 {
                let base_cross = chance_mean(d, gate, missions, false, Some(position), false);
                let cross = chance_mean(d, gate, missions, true, Some(position), false);
                encloses_chance(
                    variant.weights.as_ref().unwrap()[position],
                    (cross.score - with.score - base_cross.score + baseline.score) / f64::from(POWER),
                );
                for (i, ((cross_range, base_cross_range), (with_range, base_range))) in cross
                    .ranges
                    .iter()
                    .zip(&base_cross.ranges)
                    .zip(with.ranges.iter().zip(&baseline.ranges))
                    .enumerate()
                {
                    encloses_chance(
                        variant.range_weights.as_ref().unwrap()[position][i],
                        (cross_range[0] - with_range[0] - base_cross_range[0] + base_range[0]) / f64::from(POWER),
                    );
                }
            }
        }
    }
}

#[test]
fn aptitude_probability_score_gates_match_complete_native_expectations() {
    for gate in [
        ChanceGate::Single(0, true),
        ChanceGate::Single(1, true),
        ChanceGate::Single(100, true),
        ChanceGate::Single(1, false),
        ChanceGate::Both,
        ChanceGate::Either,
        ChanceGate::Never,
    ] {
        let d = chance_data(&APT_FEVERS, gate, [1, 2, 3]);
        check_chance_aptitude(&d, gate, [1, 2, 3], matches!(gate, ChanceGate::Single(1, true)));
    }
}

#[test]
fn aptitude_probability_checks_repeat_independently_at_separate_range_starts() {
    let gate = ChanceGate::Single(50, true);
    let mut d = chance_data(&APT_FEVERS[..2], gate, [1, 1, 3]);
    for row in &mut d.master.gekisou_skill_effects {
        row.activation_time_second = 1.2;
    }
    check_chance_aptitude(&d, gate, [1, 1, 3], true);
    let baseline = chance_mean(&d, gate, [1, 1, 3], false, None, false);
    let with = chance_mean(&d, gate, [1, 1, 3], true, None, false);
    assert!(with.ranges.iter().zip(baseline.ranges).all(|(with, base)| with[0] > base[0]));
}

#[test]
fn rank_triggered_aptitude_encloses_the_complete_solo_measurement() {
    let mut d = chance_data(&APT_FEVERS, ChanceGate::Single(100, true), [1, 2, 3]);
    let condition = d.master.skill_conditions.iter_mut().find(|row| row.id == 900).unwrap();
    condition.condition_type = 7012;
    condition.condition_values = vec![1];
    for set in d.master.skill_condition_sets.iter_mut().filter(|set| set.group == 900) {
        set.condition_ids = vec![900];
    }
    d.master.gekisou_skills.iter_mut().find(|skill| skill.id == 1).unwrap().gekisou_mission_type = 4;
    d.master.reindex().unwrap();
    let kinds = chartstats::kinds(&d.master);
    let plain = kinds.iter().find(|kind| kind.effect_type == 2000 && kind.skill_condition_group == 0).unwrap();
    let stats = chartstats::chart_stats_with(
        &d.master,
        &d.charts[0],
        std::slice::from_ref(plain),
        &chartstats::Options { replay_seeds: 1, ..Default::default() },
    )
    .unwrap();
    let variant = &stats.gekisou_aptitude.as_ref().unwrap().variants[0];
    checked(&variant.check);
    assert!(variant.weights.is_some());
    assert!(variant.range_weights.is_none());
    assert_eq!(variant.check.ranks, Some(vec![1; APT_FEVERS.len()]));
    for perfect in [false, true] {
        let baseline = aptitude_run(&d, None, 0, perfect).0;
        let measured = aptitude_run(
            &d,
            Some(Performer { gekisou_skill: Some((1, 3)), gekisou_mission_type: 4, ..Default::default() }),
            0,
            perfect,
        )
        .0;
        assert!(measured > baseline);
        encloses_chance(if perfect { variant.score_perfect } else { variant.score }, f64::from(measured - baseline));
    }
}

/// A complete LUCK law after replacing the one SKILL comparison with a fixed Boolean answer. Both
/// forms of condition 8000 have hit count zero, preserving the probability predicate's trigger result.
fn chance_lottery_mean(d: &DeckData, active: Option<bool>, plain_position: Option<usize>) -> f64 {
    let mut master = d.master.clone();
    let condition = master.skill_conditions.iter_mut().find(|row| row.id == 900).unwrap();
    condition.condition_type = 8000;
    condition.is_positive = !active.unwrap_or(false);
    condition.condition_values.clear();
    master.reindex().unwrap();
    let chart = d.chart(1004).unwrap();
    let notes = notes_of(d);
    let events: Vec<_> = chart.skill_events.iter().map(|event| (event.index, event.time_ms)).collect();
    let setup = GekisouSetup { fevers: d.charts[0].fevers.clone(), missions: vec![2, 1, 3] };
    let rule = JustRule::new(&master, &setup).unwrap();
    let stream = JudgementStream::theoretical_best_gekisou(&chart, &d.charts[0].judgement_types, &rule).unwrap();
    assert!(stream.judged.iter().all(|judgement| judgement[2] != 6));
    let play = stream.to_live_play().unwrap();
    let dt = stream.delta_times().unwrap();
    let params = LiveParams {
        total_power: POWER,
        music_level: 24,
        converted_note_count: chart.converted_note_count,
        music_length_ms: chart.last_timing_note_ms + 1000,
        score_music_length_ms: None,
        assist_factor: 1.0,
        skill_target_music_type: 1,
    };
    let mut recorder = full::LiveModel::new_gekisou(&master, &[], &notes, &events, params, &setup).unwrap();
    let frames = recorder.record_range_frames(&play, &dt).unwrap();
    assert_eq!(frames.len(), 1);
    assert!(frames[0].finish < play.frames.len());
    let mut deck = vec![Performer::default(); 5];
    if active.is_some() {
        deck[0].gekisou_skill = Some((1, 3));
        deck[0].gekisou_mission_type = 2;
    }
    if let Some(position) = plain_position {
        deck[position].live_skill = Some((903, 1));
    }
    let attempt = full::luck_exact_law_with_ranking(
        &master,
        &deck,
        &notes,
        &events,
        params,
        &setup,
        &play,
        &dt,
        None,
        &mut full::LuckExactBudget::default(),
        || false,
    )
    .unwrap();
    assert_eq!(attempt.decline, None);
    let law = attempt.law.expect("every positive-mass LUCK path terminates");
    assert!(law.atoms().len() >= 2, "Miss and Critical yield different complete scores");
    assert!(attempt.stats.terminal_paths >= 4, "the consumed lot and its prefetched successor are both enumerated");
    let mut mass = 0.0;
    let mut mean = 0.0;
    for atom in law.atoms() {
        assert!(atom.mass.numerator > 0 && atom.mass.denominator > 0);
        let chance = atom.mass.numerator as f64 / atom.mass.denominator as f64;
        mass += chance;
        mean += chance * f64::from(atom.score);
    }
    assert!((mass - 1.0).abs() < 1e-15);
    mean
}

#[test]
fn aptitude_probability_and_nondegenerate_lottery_match_complete_joint_score_laws() {
    use ournotes_sim::live::skip::ChartNote;
    let mut d = chance_data(&[(1000, 1150)], ChanceGate::Single(1, true), [2, 1, 3]);
    d.charts[0].notes = [900, 1100, 1300, 2000]
        .into_iter()
        .enumerate()
        .map(|(i, time_ms)| ChartNote { id: i as i32 + 1, time_ms, note_type: 1 })
        .collect();
    d.charts[0].judgement_types = vec![1; 4];
    d.charts[0].skill_event_ms = vec![700, 800, 900, 1200, 1500];
    assert_eq!(d.charts[0].notes.iter().filter(|note| 1000 < note.time_ms && note.time_ms <= 1150).count(), 1);
    d.master.live_music_scores.iter_mut().find(|row| row.id == 1004).unwrap().full_combo_count = 4;
    d.master.gekisou_skills.iter_mut().find(|row| row.id == 1).unwrap().gekisou_mission_type = 2;
    d.master.skill_conditions.iter_mut().find(|row| row.id == 80).unwrap().condition_target_ids = vec![56];
    for row in &mut d.master.gekisou_luck_base_points {
        row.base_point = 140;
    }
    for row in &mut d.master.gekisou_luck_bonus_lots {
        row.weight = i64::from(matches!(row.lot_result, 0 | 3));
    }
    d.master.reindex().unwrap();
    let kinds = chartstats::kinds(&d.master);
    let plain = kinds.iter().find(|kind| kind.effect_type == 2000 && kind.skill_condition_group == 0).unwrap();
    let stats = chartstats::chart_stats_with(
        &d.master,
        &d.charts[0],
        std::slice::from_ref(plain),
        &chartstats::Options { replay_seeds: 1, ..Default::default() },
    )
    .unwrap();
    let baseline = chance_lottery_mean(&d, None, None);
    let probability = f64::from(1f32 / 100.0);
    let nominal = |position| {
        (1.0 - probability) * chance_lottery_mean(&d, Some(false), position)
            + probability * chance_lottery_mean(&d, Some(true), position)
    };
    let expected = nominal(None);
    let aptitude = stats.gekisou_aptitude.as_ref().unwrap();
    assert_eq!(aptitude.variants.len(), 1);
    let variant = &aptitude.variants[0];
    encloses_chance(stats.expectation.as_ref().unwrap().score, baseline);
    encloses_chance(variant.score, expected - baseline);
    encloses_chance(variant.score_perfect, expected - baseline);
    assert!(expected > baseline);
    for position in 0..5 {
        let base_cross = chance_lottery_mean(&d, None, Some(position));
        encloses_chance(
            variant.weights.as_ref().unwrap()[position],
            (nominal(Some(position)) - expected - base_cross + baseline) / f64::from(POWER),
        );
    }
    checked(&variant.check);
}

fn contains(estimate: [f64; 2], value: f64) -> bool {
    estimate[0] - estimate[1] <= value && value <= estimate[0] + estimate[1]
}

fn checked(check: &chartstats::ExpectationCheck) {
    let error = (check.expected[0] - check.predicted[0]).abs() + check.expected[1] + check.predicted[1];
    assert!(error <= check.bound + 1e-8, "{check:?}");
}

#[test]
fn expectation_without_ranges_matches_deterministic_free_score() {
    let (d, _) = data(120);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 8).unwrap();
    let e = s.expectation.as_ref().unwrap();
    assert_eq!(s.replay_seeds, [0]);
    assert!(e.ranges.is_empty() && s.ranges.is_empty());
    assert_eq!(e.score, e.score_perfect);
    assert!(contains(e.score, s.off_seeds[0].score as f64));
    for (on, off) in e.weights.iter().zip(&s.off_seeds[0].weights) {
        for (&on, &off) in on.iter().zip(off.as_ref().unwrap()) {
            assert!(contains(on, off));
        }
    }
    assert!(e.rank_check.is_none());
    checked(&e.check);
}

#[test]
fn luck_expectations_keep_replay_seeds_and_additive_indicator_contracts() {
    let (d, _) = data_fevers(220, &FEVERS);
    let kinds = chartstats::kinds(&d.master);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 3).unwrap();
    let e = s.expectation.as_ref().unwrap();
    assert_eq!(s.replay_seeds, ournotes_sim::live::seeds::published_seeds(3));
    assert_eq!((s.positions, e.ranges.len(), e.weights.len()), (5, 3, kinds.len()));
    for (i, r) in e.ranges.iter().enumerate() {
        assert!(r.range_score[0] > 0.0 && r.max_combo > 0);
        if i != 1 {
            assert_eq!(r.luck_points, [0.0; 2]);
            assert_eq!(r.lot_results, [[0.0; 2]; 4]);
        }
        let [_, hit, super_hit, critical] = r.lot_results;
        let points = 5.0 * hit[0] + 10.0 * (super_hit[0] + critical[0]);
        let radius = 5.0 * hit[1] + 10.0 * (super_hit[1] + critical[1]);
        assert!((points - r.luck_points[0]).abs() <= radius + r.luck_points[1] + 1e-8);
    }
    assert!(e.ranges[1].luck_points[0] > 0.0);
    assert_eq!(e.ranges[2].just_count, s.just_notes);
    assert!(e.score_perfect[0] < e.score[0]);
    checked(&e.check);
    checked(e.rank_check.as_ref().unwrap());
    let doc = chartstats::document(&d, Some(3)).unwrap();
    assert_eq!(doc["format"], "ournotes-deck.chart-stats/3");
    assert_eq!(doc["charts"][0]["replaySeeds"], serde_json::to_value(s.replay_seeds).unwrap());
    assert_eq!(doc["charts"][0]["expectation"], serde_json::to_value(e).unwrap());
}

#[test]
fn fourth_fever_preserves_free_live_measurements() {
    let (d, _) = data_fevers(120, &[(8000, 16000), (24000, 32000), (42000, 50000), (55000, 57000)]);
    let s = chartstats::chart_stats(&d.master, &d.charts[0], &chartstats::kinds(&d.master), 3).unwrap();
    assert!(s.unplayable.is_some());
    assert!(s.expectation.is_none() && s.replay_seeds.is_empty());
    assert_eq!(s.off_seeds.len(), 1);
    assert!(s.off_seeds[0].score > 0);
}

#[test]
fn confirmed_rank_conditions_keep_explicit_range_weight_domains() {
    let (d, _) = data_with(120, &FEVERS, true);
    let kinds = chartstats::kinds(&d.master);
    let stats = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 2).unwrap();
    let weights = stats.expectation.as_ref().unwrap().range_weights.as_ref().unwrap();
    let rank = kinds.iter().position(|k| k.skill_condition_group == 20).unwrap();
    let combo = kinds.iter().position(|k| k.skill_condition_group == 21).unwrap();
    assert!(weights[rank].is_none());
    assert!(weights[combo].is_some());
    assert!(stats.off_seeds[0].weights[rank].as_ref().unwrap().iter().all(|&w| w == 0.0));
    assert!(stats.off_seeds[0].weights[combo].is_none());
}

#[test]
fn deterministic_skill_gains_match_independent_full_runs_and_exact_tail() {
    let mut d = aptitude_data(&APT_FEVERS[..1]);
    let shapes = chartstats::shapes(&d.master);
    let stats = chartstats::chart_stats(&d.master, &d.charts[0], &chartstats::kinds(&d.master), 4).unwrap();
    let variants = &stats.gekisou_aptitude.as_ref().unwrap().variants;
    assert_eq!(variants.len(), 5);
    for variant in variants {
        assert_eq!(shapes[variant.shape].mission, 1);
        checked(&variant.check);
        assert_eq!(variant.converted, [0.0; 2]);
        let sum = variant.tail[0] + variant.ranges.iter().map(|r| r.range_score[0] + r.rank_bonus[0]).sum::<f64>();
        assert!((variant.score[0] - sum).abs() < 1e-8);
        assert_eq!(variant.weights.as_ref().unwrap().len(), 5);
    }
    d.master.gekisou_skills.push(ournotes_sim::master::SkillRow {
        id: -71,
        gekisou_mission_type: 1,
        ..Default::default()
    });
    d.master.reindex().unwrap();
    let support = shape_for(&shapes, "support", 1);
    for band in [false, true] {
        let v = variants.iter().find(|v| v.shape == support && v.band_match == Some(band)).unwrap();
        let p = Performer {
            gekisou_skill: Some((-71, 1)),
            gekisou_mission_type: 1,
            gekisou_support_skills: vec![(1, 3)],
            band_id: if band { 1 } else { 0 },
            ..Default::default()
        };
        let (base, base_ranges) = aptitude_run(&d, None, 0, false);
        let (with, ranges) = aptitude_run(&d, Some(p), 0, false);
        let gain = with as f64 - base as f64;
        assert!(contains(v.score, gain), "band {band}: {gain} vs {:?}", v.score);
        let tail = gain
            - ranges
                .iter()
                .zip(base_ranges)
                .map(|(r, b)| {
                    (r.end_score - r.start_score - b.end_score + b.start_score) as f64
                        + (r.rank_bonus.unwrap() - b.rank_bonus.unwrap()) as f64
                })
                .sum::<f64>();
        assert!(contains(v.tail, tail));
    }
}

#[test]
fn aptitude_all_missions_publish_nominal_checks_and_complete_cross_terms() {
    let d = aptitude_data(&APT_FEVERS);
    let stats = chartstats::chart_stats(&d.master, &d.charts[0], &chartstats::kinds(&d.master), 4).unwrap();
    let aptitude = stats.gekisou_aptitude.as_ref().unwrap();
    assert_eq!(aptitude.factors.len(), 3);
    assert_eq!(aptitude.variants.len(), 9);
    for v in &aptitude.variants {
        assert_eq!(v.ranges.len(), 3);
        assert_eq!(v.weights.as_ref().unwrap().len(), 5);
        assert!(v.range_weights.as_ref().unwrap().iter().all(|r| r.len() == 3));
        checked(&v.check);
    }
    let baseline_lots = stats.expectation.as_ref().unwrap().ranges[1].lot_results.iter().map(|v| v[0]).sum::<f64>();
    assert!(contains(aptitude.factors[1].lotteries, baseline_lots));
    let disabled =
        chartstats::document_with(&d, &chartstats::Options { aptitude: false, ..Default::default() }).unwrap();
    assert_eq!(disabled["gekisouAptitude"], Value::Null);
    assert_eq!(disabled["charts"][0]["gekisouAptitude"], Value::Null);
    assert_eq!(disabled["charts"][0]["expectation"], serde_json::to_value(stats.expectation.unwrap()).unwrap());
}

#[test]
fn aptitude_without_plain_kind_keeps_expected_gains() {
    let d = aptitude_data(&APT_FEVERS[..1]);
    let stats = chartstats::chart_stats(&d.master, &d.charts[0], &[], 4).unwrap();
    for v in &stats.gekisou_aptitude.unwrap().variants {
        assert!(v.weights.is_none() && v.range_weights.is_none());
        checked(&v.check);
    }
}

#[test]
fn replay_count_does_not_change_nominal_expectations() {
    let (d, _) = data_fevers(120, &FEVERS);
    let kinds = chartstats::kinds(&d.master);
    let a = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 2).unwrap();
    let b = chartstats::chart_stats(&d.master, &d.charts[0], &kinds, 8).unwrap();
    assert_eq!(a.expectation, b.expectation);
    assert_eq!(a.gekisou_aptitude, b.gekisou_aptitude);
    assert_eq!(a.replay_seeds.len(), 2);
    assert_eq!(b.replay_seeds.len(), 8);
}

#[test]
fn command_line_selects_charts_and_replay_count() {
    let dir = std::env::temp_dir().join(format!("chart-expectation-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.json");
    let mut rng = common::Rng::new(17);
    let charts = [1002, 1004].into_iter().map(|id| chart_json_fevers(id, 40, &mut rng, &FEVERS).0).collect();
    std::fs::write(&path, document_with(charts, false).to_string()).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_ournotes-deck"))
            .args(["chart-stats", "--data", path.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };
    let one = run(&["--seeds", "2"]);
    assert!(one.status.success(), "{}", String::from_utf8_lossy(&one.stderr));
    let one: Value = serde_json::from_slice(&one.stdout).unwrap();
    let parallel = run(&["--seeds", "2", "--jobs", "2"]);
    assert!(parallel.status.success(), "{}", String::from_utf8_lossy(&parallel.stderr));
    assert_eq!(one, serde_json::from_slice::<Value>(&parallel.stdout).unwrap());
    let kept = run(&["--seeds", "2", "--charts", "1004"]);
    let kept: Value = serde_json::from_slice(&kept.stdout).unwrap();
    assert_eq!(kept["charts"].as_array().unwrap().len(), 1);
    assert_eq!(kept["charts"][0]["replaySeeds"].as_array().unwrap().len(), 2);
    assert!(!run(&["--seeds", "0"]).status.success());
    std::fs::remove_dir_all(dir).unwrap();
}
