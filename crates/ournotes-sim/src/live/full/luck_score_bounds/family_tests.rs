//! Synthetic controller-family checks against complete native nominal branches.
//!
//! Included as a child of `prepass_tests`, so the independent rational accumulator
//! and the small native fixture builder remain test-only.
use super::*;
use crate::live::full::{
    LuckControllerFamily, LuckFamilyChoice, LuckFamilyContext, LuckFamilyDecline, LuckFamilyLimits,
};

const FAMILY_START: i64 = 9201;
const FAMILY_FINISH: i64 = 9202;
const FAMILY_PROBE: i64 = 9203;
const FAMILY_LIVE: i64 = 9204;
const FAMILY_LIFE: i64 = 9205;
const FAMILY_CHANCE: i64 = 9206;
const FAMILY_SPEED: i64 = 9301;
const FAMILY_SCORE: i64 = 9302;
const FAMILY_GUARANTEE: i64 = 9303;
const FAMILY_EXTEND: i64 = 9304;
const FAMILY_REWARD: i64 = 9305;
const FAMILY_CONVERT: i64 = 9306;

#[derive(Clone)]
struct FamilyFixture {
    input: RushCase,
    // These are physical held-card choices, independently enumerated below.
    // Resource identities apply to Snaps, and `None` consumes no resource.
    choices: [Vec<(Option<usize>, Performer)>; 5],
}

impl FamilyFixture {
    fn new() -> Self {
        let (mut master, _, mut params, _, _, _) = fixture();
        for row in &mut master.live_settings {
            match row.key.as_str() {
                "gekisou_luck_gauge_max" => row.value = "40".into(),
                "gekisou_luck_gauge_max_rush" => row.value = "20".into(),
                "gekisou_luck_rush_score_bonus_percent" => row.value = "47".into(),
                _ => {}
            }
        }
        master.gekisou_luck_base_points[0].base_point = 20;
        master.gekisou_luck_base_points.push(crate::master::LuckBasePointRow {
            id: 2,
            note_category: 0,
            note_simulate_judgement: 4,
            weight: 1,
            base_point: 5,
        });
        master.gekisou_luck_bonus_lots = (0..5)
            .flat_map(|kind| {
                [0, 3].map(move |result| crate::master::LuckBonusLotRow {
                    id: kind * 10 + result + 1,
                    chance_lot_type: kind,
                    lot_result: result,
                    weight: 1,
                })
            })
            .collect();
        master.judgement_parameters.extend([
            serde_json::from_value(json!({"_id":2,"_noteSimulateJudgement":6,"_scorePercent":190,"_damage":0}))
                .unwrap(),
            serde_json::from_value(json!({"_id":3,"_noteSimulateJudgement":4,"_scorePercent":70,"_damage":0})).unwrap(),
        ]);
        master.live_judgement_timings.extend([
            serde_json::from_value(json!({"_id":2,"_noteJudgementType":1,"_noteSimulateJudgement":6,"_afterMs":0}))
                .unwrap(),
            serde_json::from_value(json!({"_id":3,"_noteJudgementType":1,"_noteSimulateJudgement":4,"_afterMs":0}))
                .unwrap(),
        ]);
        master
            .skill_targets
            .push(serde_json::from_value(json!({"_id":9250,"_skillTargetType":5,"_gekisouMissionType":2})).unwrap());
        master
            .skill_targets
            .push(serde_json::from_value(json!({"_id":9251,"_skillTargetType":4,"_judgement":5})).unwrap());
        master
            .skill_targets
            .push(serde_json::from_value(json!({"_id":9252,"_skillTargetType":4,"_judgement":4})).unwrap());
        for (id, kind, values, targets) in [
            (FAMILY_START, 7010, vec![], vec![9250]),
            (FAMILY_FINISH, 7013, vec![], vec![]),
            (FAMILY_PROBE, 7021, vec![], vec![]),
            (FAMILY_LIVE, 4010, vec![], vec![]),
            (FAMILY_LIFE, 2001, vec![900], vec![]),
            (FAMILY_CHANCE, 4011, vec![50], vec![]),
        ] {
            master.skill_conditions.push(crate::master::SkillConditionRow {
                id,
                condition_type: kind,
                condition_values: values,
                condition_target_ids: targets,
                is_positive: true,
            });
            master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
                id,
                group: id,
                condition_ids: vec![id],
            });
        }
        for effect in [2000, 11001, 11005, 12006, 15000] {
            master
                .skill_effect_settings
                .push(serde_json::from_value(json!({"_id":effect,"_skillEffectType":effect,"_phase":2})).unwrap());
        }
        for id in [FAMILY_SPEED, FAMILY_SCORE] {
            master.gekisou_skills.push(crate::master::SkillRow { id, gekisou_mission_type: 2, ..Default::default() });
        }
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id: FAMILY_GUARANTEE,
            gekisou_mission_type: 2,
            ..Default::default()
        });
        master.gekisou_skill_effects.extend([
            crate::master::GekisouSkillEffectRow {
                id: FAMILY_SPEED,
                skill_id: FAMILY_SPEED,
                level: 1,
                skill_trigger_type: ONE_SHOT,
                skill_trigger_condition_group: FAMILY_START,
                skill_effect_type: 11001,
                effect_value: 10000,
                activation_time_second: 0.2,
                ..Default::default()
            },
            crate::master::GekisouSkillEffectRow {
                id: FAMILY_SCORE,
                skill_id: FAMILY_SCORE,
                level: 1,
                skill_trigger_type: SUSTAINED,
                skill_trigger_condition_group: FAMILY_PROBE,
                skill_effect_type: 2000,
                effect_value: 7000,
                ..Default::default()
            },
        ]);
        master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id: FAMILY_GUARANTEE,
            skill_id: FAMILY_GUARANTEE,
            level: 1,
            skill_trigger_type: ONE_SHOT,
            skill_trigger_condition_group: FAMILY_START,
            skill_condition_group: FAMILY_CHANCE,
            skill_release_condition_group: FAMILY_FINISH,
            skill_effect_type: 11005,
            effect_value: 4,
            effect_limit_count: 1,
            ..Default::default()
        });
        for (id, value, duration) in [(9401, 3500, 0.12), (9402, 9000, 0.2)] {
            master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
                id,
                live_skill_id: id,
                level: 1,
                skill_effect_type: 2000,
                effect_value: value,
                activation_time_second: duration,
                ..Default::default()
            });
        }
        master.support_skill_effects.extend([
            serde_json::from_value(json!({
                "_id":FAMILY_EXTEND,"_supportSkillID":FAMILY_EXTEND,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":FAMILY_LIVE,
                "_skillEffectType":15000,"_effectValue":240
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "_id":FAMILY_REWARD,"_supportSkillID":FAMILY_REWARD,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":FAMILY_LIVE,
                "_skillEffectType":2000,"_effectValue":1200,"_activationTimeSecond":0.1
            }))
            .unwrap(),
        ]);
        master.gekisou_ranking_score_bonuses = vec![crate::master::GekisouRankingBonusRow {
            id: 1,
            mission_pattern: gekisou::mission_pattern(2, 2, 2),
            rank: 1,
            count: 1,
            score_bonus_percent: 25,
        }];
        master.reindex().unwrap();
        let notes: Vec<_> = [120, 180, 320, 380]
            .into_iter()
            .enumerate()
            .map(|(id, time_ms)| LiveNote { note_id: id as i32, time_ms, note_operate_type: 1, judgement_type: 1 })
            .collect();
        params.total_power = 12_000;
        params.converted_note_count = notes.len() as i32;
        params.music_length_ms = 1600;
        let frames: Vec<_> = (0..=80)
            .map(|index| PlayFrame {
                time_ms: index * 20,
                judged: notes
                    .iter()
                    .filter(|note| note.time_ms == index * 20)
                    .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                    .collect(),
            })
            .collect();
        let deck: Vec<_> = (0..5)
            .map(|owner| Performer {
                character_id: owner + 1,
                live_skill: match owner {
                    0 => Some((9401, 1)),
                    2 => Some((9402, 1)),
                    _ => None,
                },
                gekisou_skill: match owner {
                    0 => Some((FAMILY_SPEED, 1)),
                    1 => Some((FAMILY_SCORE, 1)),
                    _ => None,
                },
                ..Default::default()
            })
            .collect();
        let choices = std::array::from_fn(|slot| {
            let none = deck[slot].clone();
            let mut writer = none.clone();
            writer.gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
            let mut reward = none.clone();
            reward.support_skills.extend([(FAMILY_EXTEND, 1), (FAMILY_REWARD, 1)]);
            vec![(None, none), (Some(0), writer), (Some(1), reward)]
        });
        Self {
            input: RushCase {
                master,
                notes,
                params,
                setup: GekisouSetup { fevers: vec![(100, 420)], missions: vec![2, 2, 2] },
                delta: vec![0.02; frames.len()],
                play: LivePlay { frames, base_seed: 0 },
                deck,
                events: (0..5).map(|position| (position, 80 + position * 60)).collect(),
                ranking: None,
            },
            choices,
        }
    }

    fn life_reading_writer(&mut self) {
        self.input.master.gekisou_support_skill_effects[0].skill_condition_group = FAMILY_LIFE;
        self.input.master.reindex().unwrap();
    }

    fn conversion(&mut self, from: i32, to: i64) {
        self.input.master.support_skill_effects.push(
            serde_json::from_value(json!({
                "_id":FAMILY_CONVERT,"_supportSkillID":FAMILY_CONVERT,"_level":1,
                "_skillTriggerType":1,"_skillTriggerConditionGroup":FAMILY_LIVE,
                "_skillEffectType":12006,"_effectValue":to,"_effectLimitCount":2,
                "_activationTimeSecond":0.05,"_skillTargetIDs":[if from == 5 { 9251 } else { 9252 }]
            }))
            .unwrap(),
        );
        for choices in &mut self.choices {
            choices[2].1.support_skills.push((FAMILY_CONVERT, 1));
        }
        for frame in &mut self.input.play.frames {
            for note in &mut frame.judged {
                note.judgement = from;
            }
        }
        self.input.master.reindex().unwrap();
    }
}

fn physical_bindings(fixture: &FamilyFixture) -> Vec<([Option<usize>; 5], Vec<Performer>)> {
    fn visit(
        fixture: &FamilyFixture,
        slot: usize,
        resources: &mut [Option<usize>; 5],
        deck: &mut Vec<Performer>,
        out: &mut Vec<([Option<usize>; 5], Vec<Performer>)>,
    ) {
        if slot == 5 {
            out.push((*resources, deck.clone()));
            return;
        }
        for (resource, performer) in &fixture.choices[slot] {
            if resource.is_some() && resources[..slot].contains(resource) {
                continue;
            }
            resources[slot] = *resource;
            deck.push(performer.clone());
            visit(fixture, slot + 1, resources, deck, out);
            deck.pop();
        }
    }
    let mut out = Vec::new();
    visit(fixture, 0, &mut [None; 5], &mut Vec::new(), &mut out);
    assert_eq!(out.len(), 31, "all zero-, one-, and two-Snap injective assignments");
    out
}

fn physical_orders() -> Vec<[usize; 5]> {
    fn visit(prefix: &mut Vec<usize>, out: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            out.push(prefix.as_slice().try_into().unwrap());
            return;
        }
        for physical in 0..5 {
            if !prefix.contains(&physical) {
                prefix.push(physical);
                visit(prefix, out);
                prefix.pop();
            }
        }
    }
    let mut out = Vec::new();
    visit(&mut Vec::new(), &mut out);
    assert_eq!(out.len(), 120);
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FamilyNativeOracle {
    // Terminal native command classes, keyed by exact chart time. This is not a DP curve.
    joint: BTreeMap<i32, [Fraction; 4]>,
    scores: BTreeMap<i32, Fraction>,
    ordinary_end_times: Vec<i32>,
    paths: usize,
}

fn family_native_oracle(input: &RushCase, deck: &[Performer]) -> FamilyNativeOracle {
    let skills = luck_skills(&input.master).unwrap();
    let fresh = || {
        LiveModel::new_gekisou(&input.master, deck, &input.notes, &input.events, input.params, &input.setup).unwrap()
    };
    let probe_rows = fresh().luck_score_rows(&skills);
    let probe_owners: Vec<_> = probe_rows.iter().filter(|row| row.may_hold).map(|row| row.owner).collect();
    let probe_mill =
        probe_rows.iter().filter(|row| row.may_hold).map(|row| (row.value * 100000f32) as i64).sum::<i64>();
    let rush = setting(&input.master, "gekisou_luck_rush_score_bonus_percent").unwrap() as i32;
    let mut pending = vec![(Vec::new(), Fraction::ONE)];
    let mut joint = BTreeMap::<i32, [Fraction; 4]>::new();
    let mut scores = BTreeMap::<i32, Fraction>::new();
    let mut ordinary_end_times = None;
    let mut paths = 0;
    let mut visits = 0;
    while let Some((prefix, mass)) = pending.pop() {
        visits += 1;
        assert!(visits <= 4096 && prefix.len() <= 16, "finish the entire synthetic nominal tree");
        let mut native = fresh();
        native.score.begin_bounds(Vec::new(), false);
        let result = native.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix.clone()));
        assert!(
            native.random.nominal_covers_draws(),
            "native oracle has an unhandled draw: deck={deck:?}; prefix={prefix:?}; draws={}; playback={result:?}",
            native.draws()
        );
        if let Some(outcomes) = native.random.nominal_branch() {
            assert!(result.is_err());
            let total = outcomes[0].total;
            assert_eq!(outcomes.iter().map(|outcome| outcome.weight).sum::<u64>(), total);
            for (choice, outcome) in outcomes.iter().enumerate() {
                assert_eq!(outcome.total, total);
                let mut next = prefix.clone();
                next.push(choice);
                pending.push((next, mass.times(u128::from(outcome.weight), u128::from(total))));
            }
            continue;
        }
        result.unwrap();
        assert!(native.random.nominal_prefix_consumed());
        assert!(native.gk.as_ref().unwrap().ctrl.states.iter().all(|state| state.state == gekisou::S_FINISH));
        paths += 1;
        scores.entry(native.score()).and_modify(|old| *old = old.plus(mass)).or_insert(mass);
        let trace = native.score.bounds_trace.take().unwrap();
        let mut ends = Vec::new();
        for event in &trace.events {
            if let BoundsEvent::Factor { command, .. } = event
                && command.note_mill < 0
                && !probe_owners.contains(&command.owner_id)
            {
                ends.push(command.time_ms);
            }
        }
        ends.sort_unstable();
        if let Some(previous) = &ordinary_end_times {
            assert_eq!(&ends, previous, "ordinary lifetime commands do not depend on lottery outcomes");
        } else {
            ordinary_end_times = Some(ends);
        }
        for note in &input.notes {
            let mut added_rush = 0;
            let mut added_probe = 0;
            for event in &trace.events {
                if let BoundsEvent::Factor { command, .. } = event
                    && command.time_ms <= note.time_ms
                {
                    added_rush += command.luck;
                    if probe_owners.contains(&command.owner_id) {
                        // Probe owners in this fixture have no ordinary live or support score row.
                        added_probe += i64::from(command.note_mill);
                    }
                }
            }
            assert!(added_rush == 0 || added_rush == rush);
            assert!(added_probe == 0 || added_probe == probe_mill);
            let bucket = usize::from(added_probe != 0) + 2 * usize::from(added_rush != 0);
            let masses = joint.entry(note.time_ms).or_insert([Fraction::ZERO; 4]);
            masses[bucket] = masses[bucket].plus(mass);
        }
    }
    assert_eq!(scores.values().copied().fold(Fraction::ZERO, Fraction::plus), Fraction::ONE);
    for masses in joint.values() {
        assert_eq!(masses.iter().copied().fold(Fraction::ZERO, Fraction::plus), Fraction::ONE);
    }
    FamilyNativeOracle { joint, scores, ordinary_end_times: ordinary_end_times.unwrap(), paths }
}

fn assert_probability_contains(mass: Fraction, enclosure: ProbabilityMass) {
    let interval = enclosure.interval();
    assert!(mass.at_most(interval.upper()), "exact native mass {mass:?} exceeds {interval:?}");
    let lower = interval.lower();
    assert!(lower.is_finite() && lower >= 0.0);
    if lower != 0.0 {
        let bits = lower.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        assert!(exponent > 0, "all positive fixture endpoints are normal");
        let significand = u128::from((bits & ((1u64 << 52) - 1)) | (1u64 << 52));
        let shift = exponent - 1023 - 52;
        let (numerator, denominator) = if shift >= 0 {
            (significand.checked_shl(shift as u32).unwrap(), 1)
        } else {
            (significand, 1u128.checked_shl((-shift) as u32).unwrap())
        };
        assert!(
            numerator.checked_mul(mass.denominator).unwrap() <= mass.numerator.checked_mul(denominator).unwrap(),
            "lower endpoint exceeds the exact native mass {mass:?}: {interval:?}"
        );
    }
}

fn native_expected_score(oracle: &FamilyNativeOracle) -> Fraction {
    oracle.scores.iter().fold(Fraction::ZERO, |total, (&score, &mass)| {
        assert!(score >= 0);
        total.plus(mass.times(score as u128, 1))
    })
}

#[test]
fn family_fixture_extension_changes_ordinary_lifetime_without_extending_the_gk_writer() {
    let mut fixture = FamilyFixture::new();
    fixture.input.events = (0..5).map(|position| (position, 160 + position * 60)).collect();
    for choices in &mut fixture.choices {
        choices[2].1.support_skills.retain(|&(id, _)| id == FAMILY_EXTEND);
    }
    let plain: Vec<_> = fixture.choices.iter().map(|choices| choices[0].1.clone()).collect();
    let mut extended = plain.clone();
    extended[0] = fixture.choices[0][2].1.clone();
    let before = family_native_oracle(&fixture.input, &plain);
    let after = family_native_oracle(&fixture.input, &extended);
    assert!(before.paths > 1 && before.scores.len() > 1, "the native lottery payoff is not constant");
    assert_eq!(before.joint, after.joint, "15000 extends running ordinary live effects, not 11001 GK writers");
    assert_ne!(before.ordinary_end_times, after.ordinary_end_times, "the ordinary extension actually executed");
    assert!(
        after.ordinary_end_times.iter().max() > before.ordinary_end_times.iter().max(),
        "only the extension differs, and it starts while both the ordinary and GK writer lifetimes are active"
    );
    assert_ne!(native_expected_score(&before), native_expected_score(&after));
}

fn family_choices(fixture: &FamilyFixture) -> [Vec<LuckFamilyChoice>; 5] {
    std::array::from_fn(|slot| {
        fixture.choices[slot]
            .iter()
            .map(|(resource, performer)| LuckFamilyChoice { resource: *resource, performer: performer.clone() })
            .collect()
    })
}

fn family_limits() -> LuckFamilyLimits {
    LuckFamilyLimits {
        max_pair_models: 64,
        max_profiles: 31,
        max_order_evaluations: 3720,
        max_frame_work: 1_000_000,
        max_retained_bytes: 8 * 1024 * 1024,
    }
}

fn prepare_family(fixture: &FamilyFixture, capacity: usize) -> LuckControllerFamily {
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    let context = LuckFamilyContext::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .unwrap()
    .expect("a padded native terminal query mapping");
    let mut curves = LuckDpCache::new(capacity);
    context
        .prepare(&family_choices(fixture), Some(&mut curves), family_limits(), || false)
        .unwrap()
        .expect("all writer profiles and orders finish")
}

#[test]
fn controller_family_covers_every_physical_binding_and_every_native_nominal_order() {
    let fixture = FamilyFixture::new();
    let family = prepare_family(&fixture, 8 * 1024 * 1024);
    let orders = physical_orders();
    assert_eq!(family.note_times(), fixture.input.notes.iter().map(|note| note.time_ms).collect::<Vec<_>>());
    let mut profiles = std::collections::BTreeSet::new();
    let mut checked = 0;
    let mut nonconstant = 0;
    let mut observed_buckets = [false; 4];
    for (resources, physical) in physical_bindings(&fixture) {
        let profile = family.profile_for(&resources).expect("no legal held-card assignment is omitted");
        profiles.insert(profile);
        for order in &orders {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let matching: Vec<_> =
                family.orders().iter().filter(|law| law.profile == profile && law.positions == positions).collect();
            assert_eq!(matching.len(), 1, "one exact original order for this declared writer profile");
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let oracle = family_native_oracle(&fixture.input, &deck);
            nonconstant += usize::from(oracle.scores.len() > 1);
            for (&time, masses) in &oracle.joint {
                let actual = matching[0].joint_at(time);
                for bucket in 0..4 {
                    assert_probability_contains(masses[bucket], actual[bucket]);
                    observed_buckets[bucket] |= masses[bucket] != Fraction::ZERO;
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 31 * 120);
    assert_eq!(profiles.len(), 6, "no writer plus each of its five physical owners");
    assert_eq!(family.orders().len(), profiles.len() * 120, "Ready covers the complete profile/order product");
    assert!(nonconstant > 0, "a constant-payoff oracle cannot exercise the probability reward bound");
    assert!(observed_buckets.iter().filter(|&&present| present).count() >= 3);
}

#[test]
fn controller_family_cache_zero_preserves_the_same_complete_certificate() {
    let fixture = FamilyFixture::new();
    let cached = prepare_family(&fixture, 8 * 1024 * 1024);
    let uncached = prepare_family(&fixture, 0);
    assert_eq!(cached.note_times(), uncached.note_times());
    assert_eq!(cached.orders().len(), uncached.orders().len());
    for (resources, _) in physical_bindings(&fixture) {
        assert_eq!(cached.profile_for(&resources), uncached.profile_for(&resources));
    }
    for left in cached.orders() {
        let right = uncached
            .orders()
            .iter()
            .find(|law| law.profile == left.profile && law.positions == left.positions)
            .expect("cache-off retains every original profile/order");
        for &time in cached.note_times() {
            assert_eq!(left.joint_at(time), right.joint_at(time));
        }
    }
    assert!(cached.profile_for(&[Some(0), Some(0), None, None, None]).is_none());
    assert!(cached.profile_for(&[Some(usize::MAX), None, None, None, None]).is_none());
}

#[test]
fn controller_family_virtual_probe_agrees_with_an_independent_native_holder() {
    let mut fixture = FamilyFixture::new();
    // A member without any GK skill does not construct its GK Snap supports. Keep an empty same-mission
    // skill here so adding the score observer cannot also enable a previously absent controller writer.
    let empty_skill = 9307;
    fixture.input.master.gekisou_skills.push(crate::master::SkillRow {
        id: empty_skill,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    fixture.input.master.reindex().unwrap();
    assert!(!fixture.input.master.gekisou_skill_effects.iter().any(|row| row.skill_id == empty_skill));
    for choice in &mut fixture.choices[1] {
        choice.1.gekisou_skill = Some((empty_skill, 1));
    }
    fixture.input.deck[1].gekisou_skill = Some((empty_skill, 1));
    let family = prepare_family(&fixture, 8 * 1024 * 1024);
    assert_eq!(family.probe_gate(), Some(2));
    assert_eq!(family.profile_count(), 6);
    let mut observed_active = false;
    let mut checked = 0;
    for (resources, mut physical) in physical_bindings(&fixture) {
        if resources.contains(&Some(1)) {
            continue;
        }
        let profile = family.profile_for(&resources).unwrap();
        let unobserved = physical.clone();
        // This additional native observer writes only score, so it does not alter any admitted controller
        // transition. Its real integer command log observes the virtual bit without reading DP state.
        physical[1].gekisou_skill = Some((FAMILY_SCORE, 1));
        for order in [[0, 1, 2, 3, 4], [4, 3, 2, 1, 0], [2, 4, 1, 0, 3]] {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let law = family.orders().iter().find(|law| law.profile == profile && law.positions == positions).unwrap();
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let plain_deck: Vec<_> = order.iter().map(|&slot| unobserved[slot].clone()).collect();
            let without_holder = family_native_oracle(&fixture.input, &plain_deck);
            let oracle = family_native_oracle(&fixture.input, &deck);
            assert_eq!(without_holder.paths, oracle.paths, "a score observer preserves the native branch tree");
            for (&time, masses) in &oracle.joint {
                let plain = without_holder.joint[&time];
                assert_eq!(plain[1], Fraction::ZERO);
                assert_eq!(plain[3], Fraction::ZERO);
                assert_eq!(
                    [plain[0], plain[2]],
                    [masses[0].plus(masses[1]), masses[2].plus(masses[3])],
                    "adding only the native holder preserves the Rush marginal: resources={resources:?}; order={order:?}; time={time}"
                );
                observed_active |= masses[1] != Fraction::ZERO || masses[3] != Fraction::ZERO;
                for (bucket, (mass, enclosed)) in masses.iter().zip(law.joint_at(time)).enumerate() {
                    let check = std::panic::catch_unwind(|| assert_probability_contains(*mass, enclosed));
                    assert!(
                        check.is_ok(),
                        "virtual probe enclosure: resources={resources:?}; profile={profile}; order={order:?}; time={time}; bucket={bucket}; native={mass:?}; enclosure={:?}",
                        enclosed.interval()
                    );
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 6 * 3, "every absent/writer-owner profile in each selected original order");
    assert!(observed_active, "the virtual probe comparison must include a positive native on mass");
}

#[test]
fn controller_family_rejects_life_feedback_cross_category_conversion_and_a_third_writer() {
    for kind in 0..3 {
        let mut fixture = FamilyFixture::new();
        match kind {
            0 => fixture.life_reading_writer(),
            1 => fixture.conversion(4, 5),
            2 => {
                // Resource identity remains distinct even when all three select the same source row.
                for choices in &mut fixture.choices {
                    choices[2].1.gekisou_support_skills.push((FAMILY_GUARANTEE, 1));
                    choices.push((Some(2), choices[1].1.clone()));
                }
            }
            _ => unreachable!(),
        }
        let input = &fixture.input;
        let mut witnessed = input.deck.clone();
        match kind {
            0 => witnessed[0] = fixture.choices[0][1].1.clone(),
            1 => witnessed[0] = fixture.choices[0][2].1.clone(),
            2 => {
                witnessed[0] = fixture.choices[0][1].1.clone();
                witnessed[2] = fixture.choices[2][2].1.clone();
            }
            _ => unreachable!(),
        }
        assert!(
            !family_native_oracle(input, &witnessed).scores.is_empty(),
            "the rejected alternative remains a valid native input, not a missing-table fixture error"
        );
        let skills = luck_skills(&input.master).unwrap();
        let context = LuckFamilyContext::new(
            &input.master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
            &input.play,
            &input.delta,
            || false,
        )
        .unwrap()
        .unwrap();
        let error = context.prepare(&family_choices(&fixture), None, family_limits(), || false).unwrap_err();
        let expected =
            [LuckFamilyDecline::LifeFeedback, LuckFamilyDecline::JudgementFeedback, LuckFamilyDecline::WriterProfiles]
                [kind];
        assert_eq!(error.reason, expected, "one unsupported reachable choice rejects the whole family: case={kind}");
    }
}

#[test]
fn controller_family_same_luck_category_conversion_keeps_all_native_descendants() {
    let mut fixture = FamilyFixture::new();
    let mut observed_deck = fixture.input.deck.clone();
    observed_deck[0] = fixture.choices[0][2].1.clone();
    let before_conversion = family_native_oracle(&fixture.input, &observed_deck);
    fixture.conversion(5, 6);
    observed_deck[0] = fixture.choices[0][2].1.clone();
    let after_conversion = family_native_oracle(&fixture.input, &observed_deck);
    assert_ne!(before_conversion.scores, after_conversion.scores, "the same-class conversion really executes");
    assert_eq!(before_conversion.joint, after_conversion.joint, "native controller classes are unchanged");
    let family = prepare_family(&fixture, 8 * 1024 * 1024);
    // Every owner of the conversion Snap, with and without the independent writer resource. The main oracle
    // already checks all orders; these extremal orders specifically move event ownership across LUCK notes.
    let orders = [[0, 1, 2, 3, 4], [4, 3, 2, 1, 0]];
    let mut checked = 0;
    for (resources, physical) in physical_bindings(&fixture) {
        if !resources.contains(&Some(1)) {
            continue;
        }
        let profile = family.profile_for(&resources).unwrap();
        for order in &orders {
            let mut positions = [0; 5];
            for (position, &slot) in order.iter().enumerate() {
                positions[slot] = position;
            }
            let law = family.orders().iter().find(|law| law.profile == profile && law.positions == positions).unwrap();
            let deck: Vec<_> = order.iter().map(|&slot| physical[slot].clone()).collect();
            let oracle = family_native_oracle(&fixture.input, &deck);
            for (&time, masses) in &oracle.joint {
                for (mass, enclosed) in masses.iter().zip(law.joint_at(time)) {
                    assert_probability_contains(*mass, enclosed);
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 25 * 2);
}

#[test]
fn controller_family_cancellation_and_every_partial_budget_leave_no_ready_prefix() {
    let fixture = FamilyFixture::new();
    let input = &fixture.input;
    let skills = luck_skills(&input.master).unwrap();
    assert!(
        LuckFamilyContext::new(
            &input.master,
            &skills,
            &input.notes,
            &input.events,
            input.params,
            &input.setup,
            &input.play,
            &input.delta,
            || true
        )
        .unwrap()
        .is_none()
    );
    let context = LuckFamilyContext::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .unwrap()
    .unwrap();
    let choices = family_choices(&fixture);
    for limit in 0..5 {
        let mut limits = family_limits();
        match limit {
            0 => limits.max_pair_models = 1,
            1 => limits.max_profiles = 5,
            2 => limits.max_order_evaluations = 6 * 120 - 1,
            3 => limits.max_frame_work = 1,
            4 => limits.max_retained_bytes = 1,
            _ => unreachable!(),
        }
        let error = context.prepare(&choices, None, limits, || false).unwrap_err();
        assert_eq!(
            error.reason,
            if limit == 4 { LuckFamilyDecline::Capacity } else { LuckFamilyDecline::Budget },
            "partial profile/order work cannot become a family cap: limit={limit}"
        );
    }
    let polls = std::cell::Cell::new(0);
    let stopped = context
        .prepare(&choices, None, family_limits(), || {
            let next = polls.get() + 1;
            polls.set(next);
            next >= 16
        })
        .unwrap();
    assert!(stopped.is_none() && polls.get() >= 16, "mid-preparation cancellation is not a partial Ready");
    let complete = context.prepare(&choices, None, family_limits(), || false).unwrap().unwrap();
    assert_eq!(complete.orders().len(), 6 * 120, "a stopped/failed build cannot poison a later complete request");
}

#[test]
fn controller_family_requires_a_completed_empty_terminal_tail() {
    let mut fixture = FamilyFixture::new();
    let input = &mut fixture.input;
    let last = input.play.frames.last_mut().unwrap();
    let id = input.notes.len() as i32;
    input.notes.push(LiveNote { note_id: id, time_ms: last.time_ms, note_operate_type: 1, judgement_type: 1 });
    last.judged.push(JudgedNote { note_id: id, judgement: 5, judgement_time_ms: last.time_ms });
    input.params.converted_note_count += 1;
    input.params.music_length_ms = last.time_ms + 200;
    let skills = luck_skills(&input.master).unwrap();
    let error = LuckFamilyContext::new(
        &input.master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        &input.setup,
        &input.play,
        &input.delta,
        || false,
    )
    .expect_err("no universal terminal-query certificate");
    assert_eq!(error.reason, LuckFamilyDecline::TerminalMapping);
}

#[path = "family_admission_tests.rs"]
mod admission_tests;

#[path = "family_structural_note_tests.rs"]
mod structural_note_tests;

#[path = "two_writer_family_tests.rs"]
mod two_writer_tests;

#[path = "family_input_reuse_tests.rs"]
mod input_reuse_tests;

#[path = "family_lazy_profile_tests.rs"]
mod lazy_profile_tests;

#[path = "family_program_tests.rs"]
mod program_tests;

#[path = "score_equivalence_tests.rs"]
mod score_equivalence_tests;
