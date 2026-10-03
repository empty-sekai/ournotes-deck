//! Seed set, Gekisou default play, Just rule and Gekisou support skills of snaps, on synthetic data.

use ournotes_sim::Error;
use ournotes_sim::cards::{OwnedSnap, SnapView};
use ournotes_sim::live::full::GekisouSetup;
use ournotes_sim::live::model::{JudgementStream, JustRule, THEORETICAL_DT};
use ournotes_sim::live::random::{LiveRandom, NetRandom};
use ournotes_sim::live::seeds::{published_seeds, seed_candidate, seed_key, seeds_from};
use ournotes_sim::live::skip::{Chart, ChartNote, SkillEvent};
use ournotes_sim::master::Master;
use serde_json::{Value, json};

fn master_from(tables: &[(&str, Value)]) -> Master {
    let texts: Vec<(String, String)> =
        tables.iter().map(|(k, v)| (k.to_string(), json!({ "_allData": v }).to_string())).collect();
    Master::from_json_tables(|n| texts.iter().find(|(k, _)| k == n).map(|(_, t)| t.as_str())).unwrap()
}

/// Judgement type 1 has a Just timing row, type 2 does not.
fn timing_master() -> Master {
    master_from(&[(
        "MasterLiveJudgementTiming",
        json!([
            {"_id": 1, "_noteJudgementType": 1, "_noteSimulateJudgement": 6, "_afterMs": 40},
            {"_id": 2, "_noteJudgementType": 1, "_noteSimulateJudgement": 5, "_afterMs": 80},
            {"_id": 3, "_noteJudgementType": 2, "_noteSimulateJudgement": 5, "_afterMs": 80},
        ]),
    )])
}

fn setup(fevers: &[(i32, i32)], missions: &[i64]) -> GekisouSetup {
    GekisouSetup { fevers: fevers.to_vec(), missions: missions.to_vec() }
}

fn chart(notes: &[(i32, i32)], events: &[i32]) -> Chart {
    Chart {
        converted_note_count: notes.len() as i32,
        last_timing_note_ms: notes.iter().map(|n| n.1).max().unwrap_or(0),
        notes: notes.iter().map(|&(id, time_ms)| ChartNote { id, time_ms, note_type: 1 }).collect(),
        skill_events: events.iter().enumerate().map(|(i, &t)| SkillEvent { index: i as i32, time_ms: t }).collect(),
    }
}

#[test]
fn published_seeds_are_fixed_and_nested() {
    let s8 = published_seeds(8);
    assert_eq!(s8, [-70152769, -452351740, 156766337, -1696681451, -1283484205, -895566507, 322491774, 2099122494]);
    let s64 = published_seeds(64);
    assert_eq!(&s64[..8], &s8[..]);
    assert_eq!(&published_seeds(200)[..64], &s64[..]);
    assert_eq!(s8[0], seed_candidate(0));
}

#[test]
fn seeds_from_keeps_candidates_with_new_keys() {
    let start = 1u64 << 20;
    let s = seeds_from(start, 3000);
    assert_eq!(s.len(), 3000);
    let mut keys: Vec<(i32, i32)> = s.iter().map(|&x| seed_key(x)).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 3000);
    // a subsequence of the candidates from `start` on, in order
    let mut index = start;
    for &x in &s {
        while seed_candidate(index) != x {
            index += 1;
            assert!(index < start + 10_000);
        }
        index += 1;
    }
    assert_eq!(s[0], seed_candidate(start));
}

#[test]
fn seed_key_is_the_pair_of_effective_stream_seeds() {
    let mut seeds = vec![0, 1, -1, i32::MIN, i32::MAX, i32::MIN + 1, 0x9E37_79B9_u32 as i32, 12345, -12345];
    seeds.extend(published_seeds(50));
    for b in seeds {
        let (skill, luck) = seed_key(b);
        assert!(skill >= 0 && luck >= 0);
        for (stream, eff) in [(0, skill), (1, luck)] {
            let sub = LiveRandom::derive_sub_seed(b, stream);
            let mut a = NetRandom::new(sub);
            let mut e = NetRandom::new(eff);
            for _ in 0..8 {
                assert_eq!(a.next_double().to_bits(), e.next_double().to_bits(), "seed {b} stream {stream}");
            }
        }
    }
    assert_eq!(seed_key(i32::MIN).0, i32::MAX);
    assert_eq!(seed_key(5).0, seed_key(-5).0);
}

#[test]
fn just_rule_windows_follow_the_fever_steps() {
    let m = timing_master();
    let rule = JustRule::new(&m, &setup(&[(100, 200), (300, 320), (1000, 1100)], &[3, 1, 3])).unwrap();
    let frames = [0, 50, 100, 150, 199, 200, 250, 330, 340];
    assert_eq!(rule.windows(&frames), [(2, 5)]);
    assert!(rule.allows(1));
    assert!(!rule.allows(2));
    // start and end reached in the same frame: the fever turns off one frame later
    let rule = JustRule::new(&m, &setup(&[(10, 20)], &[3, 3, 3])).unwrap();
    assert_eq!(rule.windows(&[0, 30, 60]), [(1, 2)]);
    // a fever still on at the last frame
    let rule = JustRule::new(&m, &setup(&[(40, 5000)], &[3, 3, 3])).unwrap();
    assert_eq!(rule.windows(&[0, 30, 60]), [(2, 3)]);
    // no Just-count range
    let rule = JustRule::new(&m, &setup(&[(10, 20)], &[1, 2, 1])).unwrap();
    assert!(rule.windows(&[0, 30, 60]).is_empty());
    assert!(matches!(
        JustRule::new(&m, &setup(&[(1, 2), (3, 4), (5, 6), (7, 8)], &[3, 3, 3])),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(JustRule::new(&m, &setup(&[(1, 2), (3, 4)], &[3])), Err(Error::Input(_))));
}

#[test]
fn gekisou_default_play_judges_just_by_frame() {
    let m = timing_master();
    // note ids and chart times; judgement types below
    let c = chart(&[(1, 500), (2, 1000), (3, 995), (4, 1500), (5, 1990), (6, 1983), (7, 3000)], &[700]);
    let types = [1, 1, 1, 2, 1, 1, 1];
    let rule = JustRule::new(&m, &setup(&[(1000, 2000)], &[3, 1, 2])).unwrap();
    let s = JudgementStream::theoretical_best_gekisou(&c, &types, &rule).unwrap();
    // frames to 3000 + 2000 at 60 fps
    assert_eq!(s.frames.len(), 301);
    assert_eq!(*s.frames.last().unwrap(), 5000);
    assert_eq!(s.frames[59], 983);
    assert_eq!(s.frames[60], 1000);
    assert_eq!(s.frames[119], 1983);
    assert_eq!(s.frames[120], 2000);
    assert_eq!(rule.windows(&s.frames), [(60, 120)]);
    let got: Vec<(i32, i32, i32)> = s.judged.iter().map(|r| (r[1], r[0], r[2])).collect();
    assert_eq!(
        got,
        [
            (1, 30, 5),  // before the range
            (3, 60, 6),  // before Start, judged in the frame the range starts
            (2, 60, 6),  // at Start
            (4, 90, 5),  // no Just timing row
            (6, 119, 6), // last frame of the range
            (5, 120, 5), // before End, judged in the frame the range ends
            (7, 180, 5), // after the range
        ]
    );
    assert!(s.judged.iter().all(|r| r[3] == c.notes.iter().find(|n| n.id == r[1]).unwrap().time_ms));
    assert_eq!(s.base_seed, 0);
    assert_eq!(s.delta_times, None);
    assert_eq!(s.delta_times().unwrap(), vec![THEORETICAL_DT; 301]);
    s.check_just(&c, &types, &rule).unwrap();

    // a fever ending after the last note extends the play
    let rule = JustRule::new(&m, &setup(&[(1000, 2000), (2500, 6000)], &[3, 1, 2])).unwrap();
    let s = JudgementStream::theoretical_best_gekisou(&c, &types, &rule).unwrap();
    assert_eq!(*s.frames.last().unwrap(), 8000);

    // without fevers it is the Gekisou-off default play
    let rule = JustRule::new(&m, &setup(&[], &[1, 1, 1])).unwrap();
    let s = JudgementStream::theoretical_best_gekisou(&c, &types, &rule).unwrap();
    assert_eq!(s, JudgementStream::theoretical_best(&c));

    assert!(matches!(JudgementStream::theoretical_best_gekisou(&c, &types[..3], &rule), Err(Error::Input(_))));

    // a trailing unjudged note (type 103) is not judged but the clock still covers it, as a complete play requires
    let mut tail = c.clone();
    tail.notes.push(ChartNote { id: 99, time_ms: 9000, note_type: 103 });
    let mut tail_types = types.to_vec();
    tail_types.push(0);
    let s = JudgementStream::theoretical_best(&tail);
    assert_eq!(*s.frames.last().unwrap(), 11000);
    assert!(s.judged.iter().all(|r| r[1] != 99));
    let s = JudgementStream::theoretical_best_gekisou(&tail, &tail_types, &rule).unwrap();
    assert_eq!(*s.frames.last().unwrap(), 11000);
    assert!(s.judged.iter().all(|r| r[1] != 99));
}

#[test]
fn raw_just_is_checked_against_the_rule() {
    let m = timing_master();
    let c = chart(&[(1, 500), (2, 1000), (3, 1500), (4, 1990)], &[]);
    let types = [1, 1, 2, 1];
    let rule = JustRule::new(&m, &setup(&[(1000, 2000)], &[3, 3, 3])).unwrap();
    let base = JudgementStream::theoretical_best_gekisou(&c, &types, &rule).unwrap();
    let with = |note: i32, frame: Option<i32>, judgement: i32| {
        let mut s = base.clone();
        let r = s.judged.iter_mut().find(|r| r[1] == note).unwrap();
        r[2] = judgement;
        if let Some(f) = frame {
            r[0] = f;
        }
        s
    };
    base.check_just(&c, &types, &rule).unwrap();
    // any judgement other than Just is not checked
    with(2, None, 4).check_just(&c, &types, &rule).unwrap();
    // Just outside the window
    assert!(matches!(with(1, None, 6).check_just(&c, &types, &rule), Err(Error::Input(_))));
    assert!(matches!(with(4, None, 6).check_just(&c, &types, &rule), Err(Error::Input(_))));
    // Just on a type without a Just row
    assert!(matches!(with(3, None, 6).check_just(&c, &types, &rule), Err(Error::Input(_))));
    // a late frame inside the window is fine
    with(1, Some(70), 6).check_just(&c, &types, &rule).unwrap();
    // frame out of range, unknown note
    assert!(matches!(with(2, Some(100_000), 6).check_just(&c, &types, &rule), Err(Error::Input(_))));
    let mut s = base.clone();
    s.judged.push([60, 99, 6, 1000]);
    assert!(matches!(s.check_just(&c, &types, &rule), Err(Error::Input(_))));
}

#[test]
fn delta_times_default_and_checks() {
    let mut s: JudgementStream = serde_json::from_str(r#"{"frames": [0, 16, 33], "judged": []}"#).unwrap();
    assert_eq!(s.delta_times, None);
    assert_eq!(s.delta_times().unwrap(), [THEORETICAL_DT; 3]);
    assert!(!serde_json::to_string(&s).unwrap().contains("deltaTimes"));
    let t: JudgementStream =
        serde_json::from_str(r#"{"frames": [0, 16, 33], "deltaTimes": [0.0, 0.016, 0.017]}"#).unwrap();
    assert_eq!(t.delta_times().unwrap(), [0.0, 0.016, 0.017]);
    assert!(serde_json::to_string(&t).unwrap().contains("\"deltaTimes\""));
    s.delta_times = Some(vec![0.016, 0.016]);
    assert!(matches!(s.delta_times(), Err(Error::Input(_))));
    s.delta_times = Some(vec![0.016, -0.001, 0.016]);
    assert!(matches!(s.delta_times(), Err(Error::Input(_))));
    s.delta_times = Some(vec![0.016, f32::NAN, 0.016]);
    assert!(matches!(s.delta_times(), Err(Error::Input(_))));
    // the simulation's play does not depend on the delta times
    s.delta_times = Some(vec![0.02; 3]);
    let mut u = s.clone();
    u.delta_times = None;
    assert_eq!(s.to_live_play().unwrap(), u.to_live_play().unwrap());
}

fn snap_master(ids: [i64; 2], levels: bool) -> Master {
    let ranks: Vec<Value> = (1..=5)
        .map(|r| {
            let mut row = json!({"_id": r, "_group": 1, "_rank": r, "_limitLevel": 30, "_cardTypeLinkBonusRate": 0,
                "_supportSkill01Level": r, "_supportSkill02Level": r});
            if levels {
                row["_gekisouSupportSkill01Level"] = json!(r);
                row["_gekisouSupportSkill02Level"] = json!(10 + r);
            }
            row
        })
        .collect();
    master_from(&[
        (
            "MasterSupportCard",
            json!([{"_id": 7, "_characterIDs": [1], "_rarity": 3, "_cardType": 1, "_performancePowerMax": 500,
                "_technicPowerMax": 500, "_visualPowerMax": 500, "_supportCardLevelGroup": 1, "_supportCardRankGroup": 1,
                "_supportSkillId01": 0, "_supportSkillId02": 0,
                "_gekisouSupportSkillId01": ids[0], "_gekisouSupportSkillId02": ids[1]}]),
        ),
        (
            "MasterSupportCardLevel",
            json!([{"_id": 1, "_group": 1, "_level": 1, "_exp": 0, "_performanceRate": 5000, "_technicRate": 5000,
                "_visualRate": 5000}]),
        ),
        ("MasterSupportCardRank", Value::Array(ranks)),
    ])
}

#[test]
fn snap_gekisou_support_skills_come_with_rank_levels() {
    let owned = OwnedSnap { id: 7, level: Some(1), exp: None, rank: 3 };
    let v = SnapView::resolve(&snap_master([501, 502], true), &owned).unwrap();
    assert_eq!(v.gekisou_support_skill_levels, Some([3, 13]));
    assert_eq!(v.gekisou_support_skills().unwrap(), [(501, 3), (502, 13)]);
    assert!(v.support_skills().unwrap().is_empty());
    let v = SnapView::resolve(&snap_master([0, 502], true), &owned).unwrap();
    assert_eq!(v.gekisou_support_skills().unwrap(), [(502, 13)]);
    let v = SnapView::resolve(&snap_master([0, 0], true), &owned).unwrap();
    assert!(v.gekisou_support_skills().unwrap().is_empty());
    // a rank without a row
    let owned = OwnedSnap { id: 7, level: Some(1), exp: None, rank: 9 };
    let v = SnapView::resolve(&snap_master([501, 0], true), &owned).unwrap();
    assert_eq!(v.gekisou_support_skill_levels, None);
    assert!(matches!(v.gekisou_support_skills(), Err(Error::Input(_))));
}
