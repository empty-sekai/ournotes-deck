use super::*;
use crate::master::{GekisouSkillEffectRow, SkillRow};

/// The lottery fixture with skills that convert, read or damage the life, read the formation or the holder, or are
/// refused, and gauge speeds of 0.1 (809) and of 0.5 beside a range-start gauge that a chance gates (810). The life
/// starts at 1000 and its play, all Perfect, deals no note damage; its notes have a Just timing, and a Just deals no
/// note damage either. The Gekisou support skills 800, 812, 813, 816 and 817 convert a Perfect to a Great once their
/// range starts: 800 always, 812 for a holder of mission 2, 813 for such a holder while it reads a life of at least
/// 500, 816 on a Just of the frame and 817 on a Great of the frame. The Gekisou support skill 814 converts every
/// Perfect to a Just once its range starts.
fn kinds_fixture() -> (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>) {
    let (mut master, notes, params, setup, play, delta) = random_fixture();
    let effect = |id, key: &str, skill, effect, condition| {
        let converts = matches!(effect, 12006 | 13005);
        let mut value = row(id, key, skill, effect, 10000, if converts { 0 } else { 7010 }, condition, 7013);
        if converts {
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
    // Gekisou support skills that convert (800, 812, 813, 814, 816, 817) and one outside the lottery (999).
    for (skill, kind, condition) in
        [(800, 12006, 0), (812, 12006, 5000), (813, 12006, 2002), (814, 13005, 0), (816, 12006, 1006), (817, 12006, 1004)]
    {
        let mut converter = effect(skill, "_gekisouSkillID", skill, kind, condition);
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
    for skill in [800, 808, 812, 813, 814, 816, 817, 999] {
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
    for (id, judgement) in [(57, 5), (58, 6), (59, 4)] {
        let target = json!({"_id":id,"_skillTargetType":2,"_judgement":judgement});
        master.skill_targets.push(serde_json::from_value(target).unwrap());
    }
    let just = json!({"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":0});
    master.live_judgement_timings.push(serde_json::from_value(just).unwrap());
    let just = json!({"_id":2,"_noteSimulateJudgement":6,"_scorePercent":100,"_damage":0});
    master.judgement_parameters.push(serde_json::from_value(just).unwrap());
    for (id, kind, values, targets) in [
        (2000, 2000, vec![500], vec![]),
        (2001, 2001, vec![1500], vec![]),
        (5000, 5000, vec![], vec![56]),
        (1006, 1000, vec![], vec![58]),
        (1004, 1000, vec![], vec![59]),
    ] {
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

/// The native transcript of `deck`, with the actions filed in each frame in the order of their words.
fn native_transcript(live: &LotteryLive<'_>, deck: &[Performer]) -> Transcript<ProbabilityMass> {
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
    transcript
}

/// The words of [`native_transcript`].
fn native_words(live: &LotteryLive<'_>, deck: &[Performer]) -> Vec<u64> {
    native_transcript(live, deck).key().unwrap()
}

/// The transcript `kinds` composes for `deck` in its order, or None when it composes none.
fn composed_transcript(
    kinds: &mut LotteryKinds<'_>,
    live: &LotteryLive<'_>,
    deck: &[Performer],
) -> Option<Transcript<ProbabilityMass>> {
    let at = std::array::from_fn(|position| match kinds.role(live, &deck[position]) {
        LotteryRole::Kind(kind) => (kind, position),
        LotteryRole::Breaks(why) => panic!("no kind: {why:?}"),
    });
    let composition = kinds.composition(live, at);
    kinds.transcript(composition)
}

/// The words of [`composed_transcript`].
fn composed_words(kinds: &mut LotteryKinds<'_>, live: &LotteryLive<'_>, deck: &[Performer]) -> Option<Vec<u64>> {
    composed_transcript(kinds, live, deck).map(|transcript| transcript.key().unwrap())
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
            let converts = play_conversions(&master, deck, &notes, &play) == PlayConversions::Change
                && quiet_conversions(&master, &performer, &notes, &[], params, &setup, &play, &[0.1; 21], None)
                    .is_none();
            let refused = !matches!(luck::luck_signature(&master, &performer), Ok(Some(_)))
                || check_held_state_identities(&master, deck).is_err()
                || converts
                || leaves_life_range(&master, &performer, (1000, 2000));
            assert_eq!(kinds.admit(&live, &performer).is_err(), refused, "{performer:?}");
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

#[test]
fn conversions_between_perfect_and_just_compose_the_native_laws() {
    let (master, notes, params, setup, play, delta) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let live = lottery_live(&master, &skills, (&notes, params, &setup, &play, &delta));
    let changes = |master: &Master, performer: &Performer| {
        play_conversions(master, std::slice::from_ref(performer), &notes, &play)
    };
    let relabel = holding(Some(2), &[31, 814, 96]);
    let (reads, blind) = (holding(Some(3), &[816]), holding(Some(3), &[66, 817]));
    assert_eq!(changes(&master, &relabel), PlayConversions::Relabel);
    assert_eq!(changes(&master, &reads), PlayConversions::Change);
    assert_eq!(changes(&master, &holding(Some(1), &[])), PlayConversions::None);
    // A conversion to a Just that deals note damage is another change.
    let mut hurt = master.clone();
    hurt.judgement_parameters[1].damage = 10;
    assert_eq!(changes(&hurt, &relabel), PlayConversions::Change);
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    let mut conversions = |performer: &Performer| match kinds.role(&live, performer) {
        LotteryRole::Kind(kind) => (kind, kinds.entries[kind.index()].conversions),
        LotteryRole::Breaks(why) => panic!("no kind: {why:?}"),
    };
    // The conversions set a kind apart from that of the same contributions without them. A conversion that changes
    // no judgement alone reads a Just of the frame (816) or reads a Great alike for Perfects and Justs (817).
    let (plain, none) = conversions(&holding(Some(2), &[31, 96]));
    let (relabelling, relabels) = conversions(&relabel);
    assert_ne!(plain, relabelling);
    assert_eq!((none, relabels), (Conversions::default(), Conversions { relabels: true, read_relabels: false }));
    assert_eq!(conversions(&reads).1, Conversions { relabels: false, read_relabels: true });
    assert_eq!(conversions(&blind).1, Conversions::default());
    // The native transcript of a formation with the relabelling performer has Justs, its composed transcript has
    // Perfects in their place, and the two propagate to one law.
    let mut curves = LuckDpCache::new(usize::MAX);
    let decks = [
        [relabel.clone(), holding(Some(1), &[66]), blind.clone(), holding(Some(3), &[66, 96]), holding(Some(2), &[31, 96])],
        [blind.clone(), holding(Some(810), &[]), relabel.clone(), holding(Some(803), &[]), holding(None, &[66])],
    ];
    for deck in &decks {
        let mut native = native_transcript(&live, deck);
        let law = curve_words(&curves.law_of(&native, &mut || false).unwrap().unwrap());
        assert!(native.notes.iter().any(|note| note.judgement == 6), "{deck:?}");
        native.notes.iter_mut().filter(|note| note.judgement == 6).for_each(|note| note.judgement = 5);
        let composed = composed_transcript(&mut kinds, &live, deck).unwrap();
        assert_eq!(composed.key(), native.key(), "{deck:?}");
        assert_eq!(curve_words(&curves.law_of(&composed, &mut || false).unwrap().unwrap()), law, "{deck:?}");
        let orders = kinds.order_laws(&live, deck, &[[0, 1, 2, 3, 4]], &mut curves).unwrap();
        assert!(orders.is_some(), "{deck:?}");
    }
    // A formation where the relabelling conversion meets one that reads a Just takes no composed law.
    let deck = [relabel, reads, blind, holding(Some(1), &[66]), holding(None, &[66])];
    assert!(kinds.order_laws(&live, &deck, &[[0, 1, 2, 3, 4]], &mut curves).unwrap().is_none());
    assert_eq!(kinds.stats().relabel_conflicts, 1);
}

#[test]
fn formations_that_differ_only_in_their_score_probes_share_one_propagation() {
    let (master, notes, params, setup, play, delta) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let live = lottery_live(&master, &skills, (&notes, params, &setup, &play, &delta));
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    let mut curves = LuckDpCache::new(usize::MAX);
    // The support skill 31 is a direct Rush score probe and nothing else.
    let probing = [
        holding(Some(2), &[31, 96]),
        holding(Some(3), &[66, 96]),
        holding(Some(1), &[66]),
        holding(None, &[66]),
        holding(None, &[]),
    ];
    let plain = probing.clone().map(|performer| Performer {
        gekisou_support_skills: performer.gekisou_support_skills.iter().copied().filter(|&(id, _)| id != 31).collect(),
        ..performer
    });
    let order = [[0, 1, 2, 3, 4]];
    let with = kinds.order_laws(&live, &probing, &order, &mut curves).unwrap().unwrap();
    let without = kinds.order_laws(&live, &plain, &order, &mut curves).unwrap().unwrap();
    assert_eq!((kinds.stats().composed, kinds.stats().probe_variants), (1, 1));
    assert_ne!(with[0].probes, without[0].probes);
    // The two laws share their curves.
    assert!(std::sync::Arc::ptr_eq(&with[0].steps, &without[0].steps));
    assert!(std::sync::Arc::ptr_eq(&with[0].frame_queries, &without[0].frame_queries));
    for (deck, law) in [(&probing, &with[0]), (&plain, &without[0])] {
        let native = curves.law_of(&native_transcript(&live, deck), &mut || false).unwrap().unwrap();
        assert_eq!(curve_words(law), curve_words(&native), "{deck:?}");
    }
}

#[test]
fn a_census_gives_each_law_of_the_orders_of_its_formations_once() {
    let (master, notes, params, setup, play, delta) = kinds_fixture();
    let skills = luck_skills(&master).unwrap();
    let live = lottery_live(&master, &skills, (&notes, params, &setup, &play, &delta));
    let mut kinds = LotteryKinds::new(&master, usize::MAX);
    let mut curves = LuckDpCache::new(usize::MAX);
    let first = [
        holding(Some(2), &[31, 96]),
        holding(Some(3), &[66, 96]),
        holding(Some(1), &[66]),
        holding(None, &[66]),
        holding(None, &[]),
    ];
    let second = [
        holding(Some(2), &[31, 96]),
        holding(Some(2), &[31, 96]),
        holding(Some(1), &[66]),
        holding(None, &[66]),
        holding(Some(3), &[66, 96]),
    ];
    let orders = [[0, 1, 2, 3, 4], [1, 0, 2, 3, 4], [2, 3, 4, 0, 1], [4, 3, 2, 1, 0], [0, 2, 1, 4, 3]];
    let kind = |kinds: &mut LotteryKinds<'_>, performer: &Performer| match kinds.role(&live, performer) {
        LotteryRole::Kind(kind) => kind,
        LotteryRole::Breaks(why) => panic!("no kind: {why:?}"),
    };
    let formations: Vec<[LotteryKind; POSITIONS]> =
        [&first, &second, &first].iter().map(|deck| std::array::from_fn(|k| kind(&mut kinds, &deck[k]))).collect();
    let mut census = kinds.census(&live).unwrap();
    let mut read = Vec::new();
    for &formation in &formations[..2] {
        read.extend(census.formation_laws(formation, &orders, &mut curves).unwrap().unwrap());
    }
    // A formation read again reads no new law.
    assert!(census.formation_laws(formations[2], &orders, &mut curves).unwrap().unwrap().is_empty());
    let mut all = Vec::new();
    for deck in [&first, &second] {
        all.extend(kinds.order_laws(&live, deck, &orders, &mut curves).unwrap().unwrap());
    }
    let addresses = |laws: &[std::sync::Arc<LuckDpCertifiedResult>]| {
        laws.iter().map(std::sync::Arc::as_ptr).collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(addresses(&read).len(), read.len());
    assert_eq!(addresses(&read), addresses(&all));
    assert!(read.len() >= 2, "{}", read.len());
}
