//! Construction-time provenance of terminal conditional caps, using made-up card and skill tables.
use super::*;
use crate::search::budget::SearchBudget;
use crate::search::gate_tests::common::{Rng, extend_table, replace_table, roster, set_column, synth_snaps};
use ournotes_sim::cards::Roster;
use ournotes_sim::live::skip::{ChartNote, SkillEvent};
use serde_json::json;

struct ResetAblation;
impl Drop for ResetAblation {
    fn drop(&mut self) {
        set_bound_ablation(0);
    }
}

fn fixture() -> (Master, Roster, FullSetup) {
    let mut source = synth_snaps(&mut Rng::new(751), 5, 2, &[3]);
    set_column(&mut source, "MasterMemberCard", &mut |row| row["_characterID"] = row["_id"].clone());
    replace_table(&mut source, "MasterLiveSkillEffect", json!([]));
    set_column(&mut source, "MasterSupportCard", &mut |row| {
        row["_supportSkillId01"] = json!(3);
        row["_supportSkillId02"] = json!(0);
        row["_gekisouSupportSkillId01"] = row["_id"].clone();
    });
    set_column(&mut source, "MasterSupportCardRank", &mut |row| {
        row["_supportSkill01Level"] = json!(1);
        row["_supportSkill02Level"] = json!(0);
        row["_gekisouSupportSkill01Level"] = json!(1);
    });
    extend_table(
        &mut source,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"40"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"20"}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}),
        ],
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBasePoint",
        json!([{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":10}]),
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBonusLot",
        json!(
            (0..5)
                .flat_map(|kind| [0, 3].map(move |result| json!({
                    "_id":kind*10+result+1,"_chanceLotType":kind,"_lotResult":result,"_weight":1
                })))
                .collect::<Vec<_>>()
        ),
    );
    extend_table(
        &mut source,
        "MasterSkillCondition",
        vec![
            json!({"_id":901,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}),
        ],
    );
    extend_table(&mut source, "MasterSkillConditionSet", vec![json!({"_id":901,"_group":901,"_conditionIds":[901]})]);
    replace_table(
        &mut source,
        "MasterGekisouSupportSkill",
        json!([{"_id":1,"_gekisouMissionType":2},{"_id":2,"_gekisouMissionType":2}]),
    );
    replace_table(
        &mut source,
        "MasterGekisouSupportSkillEffect",
        json!(
            (1..=2)
                .map(|id| json!({
                    "_id":id,"_gekisouSupportSkillID":id,"_level":1,"_skillTriggerType":2,
                    "_skillTriggerConditionGroup":901,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
                    "_skillTargetIDs":[],"_skillEffectType":2000,"_activationTimeSecond":0.0,"_effectValue":1000*id,
                    "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
                    "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
                }))
                .collect::<Vec<_>>()
        ),
    );
    let master = source.master();
    let owned = roster(&mut Rng::new(752), &master);
    let chart = Chart::from_notes(
        (1..=3).map(|id| ChartNote { id, time_ms: id * 100, note_type: 1 }).collect(),
        vec![SkillEvent { index: 0, time_ms: 0 }],
        &LiveScoreSettings::from_master(&master).unwrap(),
    )
    .unwrap();
    let stream = JudgementStream::theoretical_best(&chart);
    let mut setup = FullSetup::new(&master, 24, &chart, &stream, &[1; 3]).unwrap();
    setup.set_gekisou(
        GekisouSetup { fevers: vec![(50, 350)], missions: vec![MISSION_LUCK; 3] },
        vec![0.04; setup.play.frames.len()],
        vec![0],
    );
    (master, owned, setup)
}

#[test]
fn terminal_cap_provenance_is_captured_before_any_ablation_reset_or_view_conversion() {
    let _reset = ResetAblation;
    set_bound_ablation(0);
    let (master, owned, setup) = fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let tables =
        Tables::new(&pool, None, false, &[0, 1], SearchBudget::new(Instant::now(), None).unwrap()).unwrap().unwrap();
    for bits in [
        ablate::RANK_BONUS,
        ablate::LUCK,
        ablate::GEKISOU_COMBO,
        ablate::EARLY_STOP_EQUAL,
        ablate::OBSERVED_MAX,
        ablate::CLASS_KEY,
        ablate::PREFIX_LATE,
    ] {
        set_bound_ablation(bits);
        let envelope = SnapLive::new(&pool, &tables, &[true; 5], &setup).unwrap();
        if bits == ablate::CLASS_KEY {
            // Their active support programs differ only in GK note-probe rows; this switch really merged them.
            assert_eq!(envelope.class_of[0][0], envelope.class_of[0][1]);
        }
        set_bound_ablation(0);
        assert!(!envelope.terminal_caps_admitted);
        let fine = envelope.into_joint_fine();
        assert!(!fine.supports_rush_mean_upper());
        assert!(
            fine.rush_cap_terms(
                100_000,
                [0, 1, 2, 3, 4],
                [1, 0, 0, 0, 0],
                &[0, 1, 2, 3, 4],
                &mut JointScratch::default(),
                Some(MISSION_LUCK),
            )
            .is_none()
        );
    }
    let envelope = SnapLive::new(&pool, &tables, &[true; 5], &setup).unwrap();
    assert_ne!(envelope.class_of[0][0], envelope.class_of[0][1]);
    assert!(envelope.terminal_caps_admitted);
    // Changing the switch after construction does not damage already proved class signatures either.
    set_bound_ablation(ablate::CLASS_KEY);
    let fine = envelope.into_joint_fine();
    assert!(fine.supports_rush_mean_upper());
    let (_, terms) = fine
        .rush_cap_terms(
            100_000,
            [0, 1, 2, 3, 4],
            [1, 0, 0, 0, 0],
            &[0, 1, 2, 3, 4],
            &mut JointScratch::default(),
            Some(MISSION_LUCK),
        )
        .unwrap();
    assert_eq!(terms.probe_off.as_ref().unwrap().len(), 3);
}

#[test]
fn terminal_conditional_caps_preserve_the_music_length_finish_frame_gate() {
    let _reset = ResetAblation;
    set_bound_ablation(0);
    let (master, owned, mut setup) = fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let tables =
        Tables::new(&pool, None, false, &[0, 1], SearchBudget::new(Instant::now(), None).unwrap()).unwrap().unwrap();
    setup.params.music_length_ms = 300;
    assert!(matches!(SnapLive::new(&pool, &tables, &[true; 5], &setup), Err(Error::Domain(message))
        if message == "score note reaches the music-length finish clamp frame"));
    setup.params.music_length_ms = 360;
    assert!(SnapLive::new(&pool, &tables, &[true; 5], &setup).unwrap().into_joint_fine().supports_rush_mean_upper());
}
