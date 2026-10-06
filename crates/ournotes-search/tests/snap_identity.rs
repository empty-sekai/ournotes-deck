//! Canonical member-set ordering over the complete signed Snap-ID domain.

#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, roster, set_column, synth_snaps};
use ournotes_search::search::oracle::brute_force_best_order_diagnostic;
use ournotes_search::search::{
    Completion, Constraints, Objective, PlayInput, SearchRequest, search_best_order_diagnostic,
};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::live::skip::{Chart, ChartNote, SkillEvent};
use ournotes_sim::pool::Pool;
use serde_json::json;

fn check(objective: Objective) {
    let mut rng = Rng::new(17);
    let mut data = synth_snaps(&mut rng, 5, 1, &[3]);
    set_column(&mut data, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    set_column(&mut data, "MasterSupportCard", &mut |row| {
        row["_id"] = json!(i64::MAX);
        row["_performancePowerMax"] = json!(0);
        row["_technicPowerMax"] = json!(0);
        row["_visualPowerMax"] = json!(0);
    });
    set_column(&mut data, "MasterParameter", &mut |row| {
        if row["_id"] == "type_link_base_bonus_rate" {
            row["_value"] = json!("0");
        }
    });
    set_column(&mut data, "MasterSupportCardRank", &mut |row| row["_cardTypeLinkBonusRate"] = json!(0));
    for table in ["MasterLiveSkillEffect", "MasterSupportSkillEffect"] {
        set_column(&mut data, table, &mut |row| row["_effectValue"] = json!(0));
    }
    let master = data.master();
    let roster = roster(&mut rng, &master);
    let pool = Pool::new(&master, &roster).unwrap();
    assert_eq!(pool.snaps[0].id, i64::MAX);
    let request = SearchRequest {
        objective,
        k: 1,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    };
    let got = search_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(got.completion, Completion::Complete);
    assert_eq!(got.results.len(), 1);
    let expected = [Some(i64::MAX), None, None, None, None];
    assert_eq!(got.results[0].snaps, expected);
    let (oracle, _) = brute_force_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(oracle[0].snaps, expected);
    assert_eq!(got.results, oracle);
}

#[test]
fn power_places_maximum_snap_id_before_an_empty_slot() {
    check(Objective::Power { music_id: None, event: false });
}

#[test]
fn live_places_maximum_snap_id_before_an_empty_slot() {
    let chart = Chart {
        converted_note_count: 1,
        last_timing_note_ms: 100,
        notes: vec![ChartNote { id: 1, time_ms: 100, note_type: 1 }],
        skill_events: (0..5).map(|index| SkillEvent { index, time_ms: 10 * index }).collect(),
    };
    let stream = JudgementStream::theoretical_best(&chart);
    check(Objective::LiveScore {
        score_id: 1004,
        chart,
        play: PlayInput::Stream { stream, judgement_types: vec![1] },
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    });
}
