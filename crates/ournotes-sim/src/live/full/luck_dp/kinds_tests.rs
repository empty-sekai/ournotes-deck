use super::*;
use crate::master::{GekisouSkillEffectRow, SkillRow};

/// The lottery fixture with skills that convert, read the life, read the formation or the holder, or are refused.
fn kinds_fixture() -> (Master, Vec<LiveNote>, LivePlay, Vec<f32>) {
    let (mut master, notes, _, _, play, delta) = random_fixture();
    let effect = |id, key: &str, skill, effect, condition| {
        let mut value = row(id, key, skill, effect, 10000, if effect == 12006 { 0 } else { 7010 }, condition, 7013);
        if effect == 12006 {
            value["_skillTargetIDs"] = json!([57]);
            value["_skillReleaseConditionGroup"] = json!(0);
        }
        value
    };
    let gekisou = |id, skill, kind, condition| -> GekisouSkillEffectRow {
        serde_json::from_value(effect(id, "_gekisouSkillID", skill, kind, condition)).unwrap()
    };
    // Range-start gauges conditioned on the life (803) and the holder (804).
    for (skill, condition) in [(803, 2000), (804, 5000)] {
        master.gekisou_skill_effects.push(gekisou(skill, skill, 11003, condition));
        master.gekisou_skills.push(serde_json::from_value(json!({"_id":skill,"_gekisouMissionType":2})).unwrap());
    }
    // A Gekisou support skill that converts Perfect to Just (800) and one outside the lottery (999).
    master.gekisou_support_skill_effects.push(gekisou(800, 800, 12006, 0));
    let mut unrelated = gekisou(999, 999, 2000, 0);
    unrelated.skill_trigger_condition_group = 0;
    unrelated.skill_release_condition_group = 0;
    master.gekisou_support_skill_effects.push(unrelated);
    // A direct Rush score-up counted over the formation (808).
    let mut counted = gekisou(808, 808, 2000, 0);
    (counted.skill_trigger_type, counted.skill_trigger_condition_group) = (2, 7021);
    (counted.skill_release_condition_group, counted.skill_cumulative_condition_id) = (0, 3000);
    master.gekisou_support_skill_effects.push(counted);
    master.cumulative_conditions.push(
        serde_json::from_value(json!({"_id":3000,"_skillCumulativeConditionType":3000,"_conditionTargetIDs":[56],
            "_maxCumulativeCount":5}))
        .unwrap(),
    );
    for skill in [800, 808, 999] {
        let row: SkillRow = serde_json::from_value(json!({"_id":skill,"_gekisouMissionType":2})).unwrap();
        master.gekisou_support_skills.push(row);
    }
    // A Gekisou skill (806) and a Gekisou support skill (807) with a cumulative lottery condition.
    let mut cumulative = gekisou(806, 806, 11003, 0);
    cumulative.skill_cumulative_condition_id = 1;
    master.gekisou_skill_effects.push(cumulative.clone());
    master.gekisou_skills.push(serde_json::from_value(json!({"_id":806,"_gekisouMissionType":2})).unwrap());
    (cumulative.id, cumulative.skill_id) = (807, 807);
    master.gekisou_support_skill_effects.push(cumulative);
    master.gekisou_support_skills.push(serde_json::from_value(json!({"_id":807,"_gekisouMissionType":2})).unwrap());
    master.skill_targets.push(serde_json::from_value(json!({"_id":57,"_skillTargetType":2,"_judgement":5})).unwrap());
    for (id, kind, values, targets) in [(2000, 2000, vec![500], vec![]), (5000, 5000, vec![], vec![56])]
    {
        master.skill_conditions.push(
            serde_json::from_value(json!({"_id":id,"_conditionType":kind,"_conditionValues":values,
                "_conditionTargetIDs":targets,"_isPositive":true}))
            .unwrap(),
        );
        master
            .skill_condition_sets
            .push(serde_json::from_value(json!({"_id":id,"_group":id,"_conditionIds":[id]})).unwrap());
    }
    master.reindex().unwrap();
    (master, notes, play, delta)
}

fn holding(skill: Option<i64>, supports: &[i64]) -> Performer {
    Performer {
        gekisou_skill: skill.map(|skill| (skill, 1)),
        gekisou_support_skills: supports.iter().map(|&support| (support, 1)).collect(),
        ..Default::default()
    }
}

#[test]
fn performers_of_equal_lottery_kinds_give_formations_the_same_key() {
    let (master, notes, play, delta) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let unrelated = master.gekisou_support_skill_effects.iter().find(|r| r.id == 999).unwrap();
    assert!(!skills.related(LuckSource::GekisouSupport, unrelated));
    let mut kinds = LotteryKinds::new(&master, &skills, &notes, &play, 0);
    let speed = holding(Some(1), &[66]);
    let gauge = holding(Some(2), &[31, 96]);
    let performers = [
        speed.clone(),
        gauge.clone(),
        holding(Some(3), &[66, 96]),
        // The member's attributes and its live and support skills leave the kind alone.
        Performer { band_id: 7, live_skill: Some((900, 1)), support_skills: vec![(901, 1)], ..speed.clone() },
        // So does a Gekisou support skill outside the lottery.
        holding(Some(2), &[31, 999, 96]),
        holding(None, &[66]),
        // The holder's own mission decides its condition.
        Performer { gekisou_mission_type: 2, ..holding(Some(804), &[]) },
        holding(Some(804), &[]),
    ];
    let roles: Vec<_> = performers.iter().map(|p| kinds.role(p)).collect();
    let kind = |role: &LotteryRole| match role {
        LotteryRole::Kind(kind) => *kind,
        LotteryRole::Breaks(why) => panic!("no kind: {why:?}"),
    };
    let ids: Vec<_> = roles.iter().map(kind).collect();
    assert_eq!((ids[3], ids[4]), (ids[0], ids[1]));
    assert_ne!(ids[6], ids[7]);
    assert_eq!(kinds.len(), 6);
    // Formations of the same kinds at every position take one key, which every formation takes.
    let mut keys = FxHashMap::<Vec<LotteryKind>, String>::default();
    let (mut seed, mut repeated) = (7u64, 0);
    for _ in 0..3000 {
        let deck: Vec<Performer> = (0..5)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                performers[(seed >> 33) as usize % performers.len()].clone()
            })
            .collect();
        assert!(check_recording(&master, &skills, &play, &delta, None, &deck).is_ok());
        assert!(!luck_play_converts(&master, &deck, &notes, &play));
        let text = lottery_conditions_text(&master, &reduced_deck(&deck), 0, &skills).unwrap();
        let tuple: Vec<_> = deck.iter().map(|p| kind(&kinds.role(p))).collect();
        match keys.get(&tuple) {
            Some(seen) => {
                assert_eq!(*seen, text, "{tuple:?}");
                repeated += 1;
            }
            None => {
                keys.insert(tuple, text);
            }
        }
    }
    assert!(repeated > 500, "{repeated}");
}

#[test]
fn lottery_kinds_refuse_performers_whose_formations_take_no_key_of_their_own() {
    let (master, notes, play, _) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let mut kinds = LotteryKinds::new(&master, &skills, &notes, &play, 0);
    let unsupported = |role: LotteryRole| matches!(role, LotteryRole::Breaks(LotteryBreak::Unsupported(_)));
    assert_eq!(kinds.role(&holding(Some(2), &[800])), LotteryRole::Breaks(LotteryBreak::Converts));
    assert!(unsupported(kinds.role(&holding(Some(806), &[]))));
    assert!(unsupported(kinds.role(&holding(Some(2), &[807]))));
    assert_eq!(kinds.role(&holding(Some(2), &[808])), LotteryRole::Breaks(LotteryBreak::ReadsFormation));
    assert_eq!(kinds.role(&holding(Some(803), &[])), LotteryRole::Breaks(LotteryBreak::ReadsLife));
    // Gekisou support skills run only beside a Gekisou skill.
    assert!(matches!(kinds.role(&holding(None, &[800])), LotteryRole::Kind(_)));
    // The checks read apart for the member and the Snap are those of the performer.
    let members = [
        holding(Some(2), &[]),
        holding(None, &[]),
        holding(Some(806), &[]),
        holding(Some(1), &[]),
    ];
    let snaps: [(&[i64], &[i64]); 6] = [(&[], &[]), (&[], &[800]), (&[], &[807]), (&[], &[66]), (&[], &[999]), (&[901], &[807, 800])];
    for member in &members {
        for &(supports, gekisou) in &snaps {
            let performer = Performer {
                support_skills: supports.iter().map(|&s| (s, 1)).collect(),
                gekisou_support_skills: gekisou.iter().map(|&s| (s, 1)).collect(),
                ..member.clone()
            };
            let deck = std::slice::from_ref(&performer);
            let refused = !matches!(luck::luck_signature(&master, &performer), Ok(Some(_)))
                || check_held_state_identities(&master, deck).is_err()
                || luck_play_converts(&master, deck, &notes, &play);
            assert_eq!(kinds.breaks(&performer).is_some(), refused, "{performer:?}");
        }
    }
}
