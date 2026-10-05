//! Explicit complete play declarations. Patterns expand once through the native accuracy builder.

use super::{Accuracy, DeckData, Error, GoalKind, Issues, JudgementStream, JustRule, PlayPolicy, Value};
use ournotes_sim::scenario::Scenario;
use serde_json::json;

#[derive(Clone, Copy, Debug)]
struct Pattern {
    accuracy: Accuracy,
    miss_every: u64,
}

impl Pattern {
    fn parse(raw: &Value, kind: GoalKind, issues: &mut Issues) -> Option<Self> {
        let start = issues.0.len();
        for field in raw.as_object()?.keys() {
            if !matches!(field.as_str(), "kind" | "greatFraction" | "justFraction" | "missEvery") {
                issues.add(format!("goal.play.{field}"), "input", "field does not apply to pattern play");
            }
        }
        let mut share = |field: &str| {
            let path = format!("goal.play.{field}");
            match raw.get(field).and_then(Value::as_f64) {
                Some(value) => super::fraction(issues, &path, value),
                None => {
                    issues.add(path, "input", "pattern play requires an explicit numeric fraction in [0, 1]");
                    None
                }
            }
        };
        let great = share("greatFraction");
        let just = share("justFraction");
        if !kind.gekisou() && just.is_some_and(|value| value != 0.0) {
            issues.add("goal.play.justFraction", "input", "Gekisou off requires justFraction 0");
        }
        let miss_every = raw.get("missEvery").and_then(Value::as_u64);
        if miss_every.is_none() {
            issues.add(
                "goal.play.missEvery",
                "input",
                "pattern play requires an explicit nonnegative integer; 0 declares no Miss",
            );
        }
        if issues.0.len() != start {
            return None;
        }
        Some(Self { accuracy: Accuracy { great_fraction: great?, just_fraction: just? }, miss_every: miss_every? })
    }

    fn expand(self, data: &DeckData, kind: GoalKind, scene_id: i64, score_id: i64) -> Result<JudgementStream, Error> {
        let chart = data.chart(score_id)?;
        let dc = data.data_chart(score_id).ok_or_else(|| Error::Input("chart is absent".into()))?;
        let rule = if kind.gekisou() {
            let scene = match kind {
                GoalKind::BattleLive => Scenario::Battle(scene_id),
                GoalKind::ArenaLive => Scenario::Arena(scene_id),
                _ => Scenario::Mission(scene_id),
            };
            let setup = scene.resolve(&data.master)?.gekisou_setup(&dc.fevers);
            Some(JustRule::new(&data.master, &setup)?)
        } else {
            None
        };
        let (mut stream, _) =
            JudgementStream::with_accuracy(&chart, &dc.judgement_types, rule.as_ref(), self.accuracy)?;
        if self.miss_every > 0 {
            for (index, row) in stream.judged.iter_mut().enumerate() {
                if (index as u64 + 1) % self.miss_every == 0 {
                    row[2] = 1;
                }
            }
        }
        Ok(stream)
    }
}

pub(super) fn parse(
    data: &DeckData,
    kind: GoalKind,
    scene_id: Option<i64>,
    score_id: Option<i64>,
    raw: &Value,
    issues: &mut Issues,
) -> Option<(PlayPolicy, Value)> {
    let (stream, mut echo, path) = match raw.get("kind").and_then(Value::as_str) {
        Some("pattern") => {
            let pattern = Pattern::parse(raw, kind, issues)?;
            let stream = match pattern.expand(data, kind, scene_id?, score_id?) {
                Ok(stream) => stream,
                Err(error) => {
                    issues.0.push(super::issue("goal.play", error));
                    return None;
                }
            };
            (
                stream,
                json!({"kind":"pattern","greatFraction":pattern.accuracy.great_fraction,
                "justFraction":pattern.accuracy.just_fraction,"missEvery":pattern.miss_every,
                "accuracyBeforeMisses":true}),
                "goal.play",
            )
        }
        Some("stream") => {
            if raw.get("stream").is_none_or(Value::is_null) {
                issues.add("goal.play.stream", "input", "explicit play requires a complete judgement stream");
                return None;
            }
            match serde_json::from_value::<PlayPolicy>(raw.clone()) {
                Ok(PlayPolicy::Stream { stream }) => (stream, json!({"kind":"stream"}), "goal.play.stream"),
                Ok(_) => unreachable!("stream kind checked"),
                Err(error) => {
                    issues.add("goal.play.stream", "input", error.to_string());
                    return None;
                }
            }
        }
        _ => {
            issues.add("goal.play.kind", "input", "explicit play requires kind pattern or stream");
            return None;
        }
    };
    if let (Some(scene_id), Some(score_id)) = (scene_id, score_id) {
        if let Err(error) = super::complete_stream(data, kind, scene_id, score_id, &stream) {
            issues.0.push(super::issue(path, error));
            return None;
        }
    }
    echo["complete"] = json!(true);
    echo["frames"] = json!(stream.frames.len());
    echo["judged"] = json!(stream.judged.len());
    for (name, grade) in [("misses", 1), ("greats", 4), ("perfects", 5), ("justs", 6)] {
        echo[name] = json!(stream.judged.iter().filter(|row| row[2] == grade).count());
    }
    echo["meaning"] =
        json!("declared complete play before skill conversion; terminal life is not survival probability");
    Some((PlayPolicy::Stream { stream }, echo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ournotes_sim::data::DataChart;
    use ournotes_sim::live::skip::ChartNote;
    use ournotes_sim::master::Master;

    fn data() -> DeckData {
        let tables = json!({
            "MasterLiveSettings":[
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"}],
            "MasterLiveNoteParameter":[{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
            "MasterLiveJudgementTiming":[{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":6}],
            "MasterLiveMusic":[{"_id":10,"_expertID":1004,"_gekisouMission1":3,
                "_gekisouMission2":1,"_gekisouMission3":1}],
            "MasterLiveMusicScore":[{"_id":1004,"_level":20}],
            "MasterArenaMusic":[{"_id":20,"_liveMusicId":10,"_gekisouMission1":3}],
            "MasterChallengeMusic":[{"_id":30,"_liveMusicId":10}]
        });
        let texts: Vec<_> = tables
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
            .collect();
        let master =
            Master::from_json_tables(|name| texts.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()))
                .unwrap();
        DeckData {
            provenance: Value::Null,
            sha256: None,
            master,
            charts: vec![DataChart {
                score_id: 1004,
                asset_key: String::new(),
                asset_sha256: String::new(),
                notes: vec![(4, 300, 1), (2, 200, 1), (1, 200, 1), (5, 500, 1), (3, 250, 1), (99, 10000, 0)]
                    .into_iter()
                    .map(|(id, time_ms, note_type)| ChartNote { id, time_ms, note_type })
                    .collect(),
                judgement_types: vec![1, 1, 1, 2, 1, 1],
                skill_event_ms: vec![14000],
                fevers: vec![(150, 16000)],
            }],
        }
    }

    fn pattern(great: f64, just: f64, miss_every: u64) -> Value {
        json!({"kind":"pattern","greatFraction":great,"justFraction":just,"missEvery":miss_every})
    }

    #[test]
    fn pattern_matches_native_accuracy_then_overrides_every_nth_judged_note() {
        let data = data();
        for (kind, id, just) in [
            (GoalKind::FreeLive, 10, 0.0),
            (GoalKind::ChallengeLive, 30, 0.0),
            (GoalKind::MissionLive, 10, 0.5),
            (GoalKind::BattleLive, 10, 0.5),
            (GoalKind::ArenaLive, 20, 0.5),
        ] {
            let chart = data.chart(1004).unwrap();
            let dc = data.data_chart(1004).unwrap();
            let rule = kind.gekisou().then(|| {
                JustRule::new(
                    &data.master,
                    &ournotes_sim::live::full::GekisouSetup { fevers: dc.fevers.clone(), missions: vec![3, 1, 1] },
                )
                .unwrap()
            });
            let (base, _) = JudgementStream::with_accuracy(
                &chart,
                &dc.judgement_types,
                rule.as_ref(),
                Accuracy { great_fraction: 0.4, just_fraction: just },
            )
            .unwrap();
            assert_eq!(base.judged.iter().map(|row| row[1]).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
            for every in [0, 1, 2, 6, u64::MAX] {
                let mut issues = Issues(vec![]);
                let (policy, echo) =
                    parse(&data, kind, Some(id), Some(1004), &pattern(0.4, just, every), &mut issues).unwrap();
                assert!(issues.0.is_empty(), "{:?}", issues.0);
                let PlayPolicy::Stream { stream } = policy else { panic!("expanded once to stream") };
                let mut expected = base.clone();
                for (index, row) in expected.judged.iter_mut().enumerate() {
                    if every != 0 && (index as u64 + 1) % every == 0 {
                        row[2] = 1;
                    }
                }
                assert_eq!(stream, expected);
                assert_eq!(echo["misses"].as_u64().unwrap(), if every == 0 { 0 } else { 5 / every });
                assert_eq!(echo["judged"], 5);
                assert!(echo["frames"].as_u64().unwrap() > 0);
                assert_eq!(echo["kind"], "pattern");
                assert_eq!(echo["missEvery"], every);
                assert!(echo.get("stream").is_none());
                assert!(stream.frames.last().unwrap() >= &14000);
                if kind.gekisou() {
                    assert!(stream.frames.last().unwrap() > &16000);
                }
            }
        }
    }

    #[test]
    fn pattern_requires_each_explicit_valid_field_and_rejects_unknown_fields() {
        for field in ["greatFraction", "justFraction", "missEvery"] {
            for value in [None, Some(Value::Null), Some(json!("0")), Some(json!(false)), Some(json!(-1))] {
                let mut raw = pattern(0.0, 0.0, 0);
                if let Some(value) = value {
                    raw[field] = value;
                } else {
                    raw.as_object_mut().unwrap().remove(field);
                }
                let mut issues = Issues(vec![]);
                assert!(Pattern::parse(&raw, GoalKind::FreeLive, &mut issues).is_none());
                assert!(issues.0.iter().any(|issue| issue.path == format!("goal.play.{field}")));
            }
        }
        for (field, value) in [
            ("greatFraction", json!(1.01)),
            ("justFraction", json!(1.01)),
            ("missEvery", json!(1.5)),
            ("missEvery", json!(1.0)),
            ("extra", json!(0)),
            ("stream", json!({})),
        ] {
            let mut raw = pattern(0.0, 0.0, 0);
            raw[field] = value;
            let mut issues = Issues(vec![]);
            assert!(Pattern::parse(&raw, GoalKind::FreeLive, &mut issues).is_none());
            assert!(issues.0.iter().any(|issue| issue.path == format!("goal.play.{field}")));
        }
        for fraction in [0.0, 1.0] {
            let mut issues = Issues(vec![]);
            assert!(Pattern::parse(&pattern(fraction, fraction, 0), GoalKind::MissionLive, &mut issues).is_some());
        }
        let mut issues = Issues(vec![]);
        assert!(Pattern::parse(&pattern(0.0, 1.0, 0), GoalKind::FreeLive, &mut issues).is_none());
        assert_eq!(issues.0[0].path, "goal.play.justFraction");
    }

    #[test]
    fn formal_request_accepts_pattern_life_and_preserves_stream_and_conflict_validation() {
        let data = data();
        let mut request = json!({"format":super::super::REQUEST_FORMAT,
            "goal":{"kind":"freeLive","musicId":10,"difficulty":"expert","play":pattern(0.4,0.0,2)},
            "metric":{"kind":"scoreAndLife","threshold":1,"minFinalLife":1}});
        let parse_request = |raw: Value| {
            let mut issues = Issues(vec![]);
            let parsed = super::super::parse_request(&data, serde_json::from_value(raw).unwrap(), &mut issues);
            (parsed, issues)
        };
        let (parsed, issues) = parse_request(request.clone());
        assert!(issues.0.is_empty(), "{:?}", issues.0);
        let parsed = parsed.unwrap();
        assert_eq!(parsed.goal["play"]["kind"], "pattern");
        assert_eq!(parsed.goal["play"]["misses"], 2);
        assert_eq!(parsed.goal["play"]["greatFraction"], 0.4);
        assert!(parsed.goal.get("accuracy").is_none());
        let crate::types::Execution::Live { play: PlayPolicy::Stream { stream }, .. } = parsed.search.execution else {
            panic!("stream expected")
        };
        request["goal"]["play"] = json!({"kind":"stream","stream":stream});
        let (parsed, issues) = parse_request(request.clone());
        assert!(issues.0.is_empty(), "{:?}", issues.0);
        assert_eq!(parsed.unwrap().goal["play"]["misses"], 2);
        request["goal"]["play"] = pattern(0.0, 0.0, 0);
        request["goal"]["accuracy"] = json!({"greatFraction":0.0,"justFraction":0.0});
        let (_, issues) = parse_request(request.clone());
        assert!(issues.0.iter().any(|issue| issue.path == "goal.accuracy"));
        request["goal"].as_object_mut().unwrap().remove("accuracy");
        request["goal"]["play"]["kind"] = json!("unknown");
        let (_, issues) = parse_request(request);
        assert!(issues.0.iter().any(|issue| issue.path == "goal.play.kind"));
        let caps = super::super::capabilities();
        assert_eq!(caps["patternPlay"]["accuracyBeforeMisses"], true);
        assert_eq!(caps["patternPlay"]["survivalProbability"], false);
    }
}
