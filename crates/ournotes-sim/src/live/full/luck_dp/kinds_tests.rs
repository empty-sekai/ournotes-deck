use super::*;
use crate::master::{GekisouSkillEffectRow, SkillRow};

/// The lottery fixture with skills that convert, read or damage the life, read the formation or the holder, or are
/// refused, and gauge speeds of 0.1 (809) and of 0.5 beside a range-start gauge that a chance gates (810). The life
/// starts at 1000 and its play, all Perfect, deals no note damage. The Gekisou support skills 800, 812 and 813 convert
/// a Perfect to a Great once their range starts: 800 always, 812 for a holder of mission 2, and 813 for such a holder
/// while it reads a life of at least 500.
fn kinds_fixture() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let (mut master, notes, params, setup, play, delta) = random_fixture();
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
    // Range-start gauges conditioned on a life above 500 (803), on the holder (804) and on a life of 1500 (805).
    for (skill, condition) in [(803, 2000), (804, 5000), (805, 2001)] {
        master.gekisou_skill_effects.push(gekisou(skill, skill, 11003, condition));
        master.gekisou_skills.push(serde_json::from_value(json!({"_id":skill,"_gekisouMissionType":2})).unwrap());
    }
    // Gauge speeds for 0.2 s from a range start.
    for (skill, value) in [(809, 1000), (810, 5000)] {
        let mut speed = row(skill, "_gekisouSkillID", skill, 11001, value, 7010, 0, 0);
        speed["_activationTimeSecond"] = json!(0.2);
        master.gekisou_skill_effects.push(serde_json::from_value(speed).unwrap());
        master.gekisou_skills.push(serde_json::from_value(json!({"_id":skill,"_gekisouMissionType":2})).unwrap());
    }
    master.gekisou_skill_effects.push(gekisou(811, 810, 11003, 4011));
    // A support skill that damages the life (902).
    master.support_skill_effects.push(serde_json::from_value(effect(902, "_supportSkillID", 902, 3002, 0)).unwrap());
    // Gekisou support skills that convert (800, 812, 813) and one outside the lottery (999).
    for (skill, condition) in [(800, 0), (812, 5000), (813, 2002)] {
        let mut converter = effect(skill, "_gekisouSkillID", skill, 12006, condition);
        converter["_skillTriggerConditionGroup"] = json!(7010);
        (converter["_effectValue"], converter["_activationTimeSecond"]) = (json!(4), json!(99999.0));
        master.gekisou_support_skill_effects.push(serde_json::from_value(converter).unwrap());
    }
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
    for skill in [800, 808, 812, 813, 999] {
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
    for (id, kind, values, targets) in
        [(2000, 2000, vec![500], vec![]), (2001, 2001, vec![1500], vec![]), (5000, 5000, vec![], vec![56])]
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
    let both = json!({"_id":2002,"_group":2002,"_conditionIds":[2000, 5000]});
    master.skill_condition_sets.push(serde_json::from_value(both).unwrap());
    master.reindex().unwrap();
    (master, notes, params, setup, play, delta)
}

fn lottery_live<'a>(
    master: &'a Master,
    skills: &'a LuckSkills,
    (notes, params, setup, play, delta): (&'a [LiveNote], LiveParams, &'a GekisouSetup, &'a LivePlay, &'a [f32]),
) -> LotteryLive<'a> {
    LotteryLive { master, skills, notes, events: &[], params, setup, play, delta_times: delta, ranking: None }
}

fn holding(skill: Option<i64>, supports: &[i64]) -> Performer {
    Performer {
        gekisou_skill: skill.map(|skill| (skill, 1)),
        gekisou_support_skills: supports.iter().map(|&support| (support, 1)).collect(),
        ..Default::default()
    }
}

/// The words of the native transcript of `deck`, with the actions filed in each frame in the order of their words.
fn native_words(live: &LotteryLive<'_>, deck: &[Performer]) -> Vec<u64> {
    let LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking } = *live;
    let prepared =
        prepare_recording::<ProbabilityMass>(master, skills, notes, params, play, delta_times, deck, None, false)
            .unwrap();
    let life = prepared.life_deck.as_deref();
    let skeleton = record_skeleton(
        master,
        skills,
        notes,
        events,
        params,
        setup,
        play,
        delta_times,
        &prepared.reduced,
        life,
        ranking,
        &mut || false,
    )
    .unwrap()
    .unwrap();
    let mut transcript = file(&skeleton, &prepared.plan, false);
    let mut from = 0;
    for frame in &transcript.frames {
        transcript.actions[from..frame.actions].sort_by_key(action_words);
        from = frame.actions;
    }
    transcript.key().unwrap()
}

/// The words of the transcript `kinds` composes for `deck` in its order, or None when it composes none.
fn composed_words(kinds: &mut LotteryKinds<'_>, live: &LotteryLive<'_>, deck: &[Performer]) -> Option<Vec<u64>> {
    let at = std::array::from_fn(|position| match kinds.role(live, &deck[position]) {
        LotteryRole::Kind(kind) => (kind, position),
        LotteryRole::Breaks(why) => panic!("no kind: {why:?}"),
    });
    let composition = kinds.composition(live, at);
    kinds.transcript(composition).map(|transcript| transcript.key().unwrap())
}

#[test]
fn performers_of_equal_kinds_compose_the_native_transcripts_of_their_formations() {
    let (master, notes, params, setup, play, delta) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let live = lottery_live(&master, &skills, (&notes, params, &setup, &play, &delta));
    let unrelated = master.gekisou_support_skill_effects.iter().find(|r| r.id == 999).unwrap();
    assert!(!skills.related(LuckSource::GekisouSupport, unrelated));
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    let speed = holding(Some(1), &[66]);
    let performers = [
        speed.clone(),
        holding(Some(2), &[31, 96]),
        holding(Some(3), &[66, 96]),
        // The member's attributes and its live and support skills leave the kind alone.
        Performer { band_id: 7, live_skill: Some((900, 1)), support_skills: vec![(901, 1)], ..speed.clone() },
        // So does a Gekisou support skill outside the lottery, and a conversion that changes no judgement: its holder
        // is not of mission 2, and its native recording runs the life recorder.
        holding(Some(2), &[31, 999, 96]),
        holding(Some(2), &[31, 812, 96]),
        holding(None, &[66]),
        // The holder's own mission decides its condition.
        Performer { gekisou_mission_type: 2, ..holding(Some(804), &[]) },
        holding(Some(804), &[]),
        // The life range of the play answers this life condition as the holder's mission answers the other.
        holding(Some(803), &[]),
        holding(Some(809), &[]),
        holding(Some(810), &[]),
    ];
    let kind = |role: LotteryRole| match role {
        LotteryRole::Kind(kind) => kind,
        LotteryRole::Breaks(why) => panic!("no kind: {why:?}"),
    };
    let ids: Vec<_> = performers.iter().map(|p| kind(kinds.role(&live, p))).collect();
    assert_eq!((ids[3], ids[4], ids[5]), (ids[0], ids[1], ids[1]));
    assert_ne!(ids[7], ids[8]);
    assert_eq!(ids[9], ids[7]);
    assert_eq!(kinds.len(), 8);
    // Every formation composes the native transcript of its performers, with each frame's actions in the order of
    // their words, unless it holds the gauge speed of 0.1, whose sum with its removal is not shown to be exact.
    let tenths = |deck: &[Performer]| deck.iter().any(|p| p.gekisou_skill == Some((809, 1)));
    let (mut seed, mut composed, mut rounding) = (7u64, 0, 0);
    for _ in 0..400 {
        let deck: Vec<Performer> = (0..5)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                performers[(seed >> 33) as usize % performers.len()].clone()
            })
            .collect();
        match composed_words(&mut kinds, &live, &deck) {
            Some(words) => {
                assert!(!tenths(&deck), "{deck:?}");
                assert_eq!(words, native_words(&live, &deck), "{deck:?}");
                composed += 1;
            }
            None => {
                assert!(tenths(&deck), "{deck:?}");
                rounding += 1;
            }
        }
    }
    assert!(composed > 100 && rounding > 100, "{composed} {rounding}");
    assert_eq!(kinds.stats().factor_recordings, 4 * 5);
}

#[test]
fn lottery_kinds_refuse_performers_whose_contributions_depend_on_other_performers() {
    let (master, notes, params, setup, play, _) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let live = lottery_live(&master, &skills, (&notes, params, &setup, &play, &[0.1; 21]));
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    let unsupported = |role: LotteryRole| matches!(role, LotteryRole::Breaks(LotteryBreak::Unsupported(_)));
    let converts = LotteryRole::Breaks(LotteryBreak::Converts);
    assert_eq!(kinds.role(&live, &holding(Some(2), &[800])), converts);
    // A conversion that changes no judgement alone takes a kind; one that reads the life does not.
    assert!(matches!(kinds.role(&live, &holding(Some(2), &[812])), LotteryRole::Kind(_)));
    let mission = |performer: Performer| Performer { gekisou_mission_type: 2, ..performer };
    assert_eq!(kinds.role(&live, &mission(holding(Some(2), &[812]))), converts);
    assert_eq!(kinds.role(&live, &holding(Some(2), &[813])), converts);
    assert!(unsupported(kinds.role(&live, &holding(Some(806), &[]))));
    assert!(unsupported(kinds.role(&live, &holding(Some(2), &[807]))));
    assert_eq!(kinds.role(&live, &holding(Some(2), &[808])), LotteryRole::Breaks(LotteryBreak::ReadsFormation));
    assert_eq!(kinds.role(&live, &holding(Some(805), &[])), LotteryRole::Breaks(LotteryBreak::ReadsLife));
    let damaging = Performer { support_skills: vec![(902, 1)], ..holding(Some(2), &[]) };
    assert_eq!(kinds.role(&live, &damaging), LotteryRole::Breaks(LotteryBreak::LeavesLifeRange));
    // Gekisou support skills run only beside a Gekisou skill.
    assert!(matches!(kinds.role(&live, &holding(None, &[800])), LotteryRole::Kind(_)));
    // The checks read apart for the member and the Snap are those of the performer.
    let members = [
        holding(Some(2), &[]),
        holding(None, &[]),
        holding(Some(806), &[]),
        holding(Some(1), &[]),
    ];
    let snaps: [(&[i64], &[i64]); 10] = [
        (&[], &[]),
        (&[], &[800]),
        (&[], &[812]),
        (&[], &[813]),
        (&[], &[807]),
        (&[], &[66]),
        (&[], &[999]),
        (&[901], &[807, 800]),
        (&[902], &[]),
        (&[901, 902], &[66]),
    ];
    for member in &members {
        for &(supports, gekisou) in &snaps {
            let performer = Performer {
                support_skills: supports.iter().map(|&s| (s, 1)).collect(),
                gekisou_support_skills: gekisou.iter().map(|&s| (s, 1)).collect(),
                ..member.clone()
            };
            let deck = std::slice::from_ref(&performer);
            let converts = luck_play_converts(&master, deck, &notes, &play)
                && conversions_reach_play(&master, &performer, &notes, &[], params, &setup, &play, &[0.1; 21], None);
            let refused = !matches!(luck::luck_signature(&master, &performer), Ok(Some(_)))
                || check_held_state_identities(&master, deck).is_err()
                || converts
                || leaves_life_range(&master, &performer, (1000, 2000));
            assert_eq!(kinds.breaks(&live, &performer).is_some(), refused, "{performer:?}");
        }
    }
    // A live whose recording without performers fails composes no law.
    let short = lottery_live(&master, &skills, (&notes, params, &setup, &play, &[0.1; 20]));
    assert!(unsupported(kinds.role(&short, &holding(Some(2), &[]))));
    assert_eq!(kinds.stats().lives, 2);
}

#[test]
fn lottery_kinds_fold_no_life_condition_of_a_play_with_note_damage() {
    let (mut master, notes, params, setup, play, delta) = kinds_fixture();
    master.judgement_parameters[0].damage = 300;
    let skills = luck_skills(&master).unwrap();
    let live = lottery_live(&master, &skills, (&notes, params, &setup, &play, &delta));
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    assert_eq!(kinds.role(&live, &holding(Some(803), &[])), LotteryRole::Breaks(LotteryBreak::ReadsLife));
    let damaging = Performer { support_skills: vec![(902, 1)], ..holding(Some(2), &[]) };
    assert!(matches!(kinds.role(&live, &damaging), LotteryRole::Kind(_)));
}


