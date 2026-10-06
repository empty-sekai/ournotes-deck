//! Canonical member-set Snap ordering across the complete public ID range.

#[path = "../../ournotes-sim/tests/common/mod.rs"]
mod common;

use common::{Rng, roster, synth_snaps};
use ournotes_search::search::{
    Completion, Constraints, Objective, PlayInput, SearchRequest, evaluate, oracle::brute_force_best_order_diagnostic,
    search_best_order_diagnostic,
};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::live::skip::{Chart, ChartNote, SkillEvent};
use ournotes_sim::pool::Pool;

#[test]
fn maximum_snap_id_precedes_empty_slots_in_equal_score_classes() {
    let mut rng = Rng::new(19);
    let mut master = synth_snaps(&mut rng, 5, 1, &[3]).master();
    for member in &mut master.member_cards {
        member.character_id = member.id;
        member.card_type = 2;
        member.leader_skill_id = 4;
        member.live_skill_id = 1;
    }
    for row in &mut master.leader_skill_effects {
        row.effect_value = 0;
    }
    for row in &mut master.live_skill_effects {
        row.effect_value = 0;
    }
    let snap = &mut master.support_cards[0];
    snap.id = i64::MAX;
    snap.card_type = 1;
    snap.performance_power_max = 0;
    snap.technic_power_max = 0;
    snap.visual_power_max = 0;
    snap.support_skill_id_01 = 3;
    snap.support_skill_id_02 = 0;
    master.reindex().unwrap();
    let roster = roster(&mut rng, &master);
    let pool = Pool::new(&master, &roster).unwrap();
    let objective = Objective::LiveScore {
        score_id: 1004,
        chart: Chart {
            notes: vec![ChartNote { id: 1, time_ms: 0, note_type: 1 }],
            skill_events: (0..5).map(|index| SkillEvent { index, time_ms: 100 }).collect(),
            converted_note_count: 1,
            last_timing_note_ms: 0,
        },
        play: PlayInput::Stream {
            stream: JudgementStream {
                frames: vec![0, 100, 200],
                judged: vec![[0, 1, 5, 0]],
                base_seed: 0,
                assist: false,
                delta_times: None,
            },
            judgement_types: vec![1],
        },
        event: false,
        exclude_snap_skills: false,
        gekisou: None,
    };
    let request = SearchRequest {
        objective,
        k: 1,
        constraints: Constraints { leader: Some(1), ..Default::default() },
        time_limit: None,
    };
    let members = [2, 3, 1, 4, 5];
    let order = [0, 1, 2, 3, 4];
    let expected_snaps = [Some(i64::MAX), None, None, None, None];
    let empty = pool.deck(members, [None; 5], order).unwrap();
    let paired = pool.deck(members, expected_snaps, order).unwrap();
    assert_eq!(
        evaluate(&pool, &empty, &request.objective).unwrap(),
        evaluate(&pool, &paired, &request.objective).unwrap()
    );

    let result = search_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(result.completion, Completion::Complete);
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].members, members);
    assert_eq!(result.results[0].snaps, expected_snaps);
    assert_eq!(result.results[0].performance_order, order);
    let (expected, _) = brute_force_best_order_diagnostic(&pool, &request).unwrap();
    assert_eq!(result.results, expected);
}
