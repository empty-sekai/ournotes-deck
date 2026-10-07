//! Synthetic node-bound oracle. Descendants and exact native probability laws are enumerated independently
//! of the family-cache traversal; none of the proposed caps supplies an oracle score or probability.
use super::*;
use crate::search::expectation::{self, PhysicalDeck};
use crate::search::gate_tests::common::{Rng, extend_table, replace_table, roster, set_column, synth_snaps};
use crate::search::physical::family_nodes::{FamilyNodeCache, FamilyNodeOutcome};
use crate::search::{Constraints, GekisouObjective, Objective, PlayInput, SearchRequest, SeedSet};
use ournotes_sim::cards::Roster;
use ournotes_sim::live::full::{
    LuckDpCache, LuckExactBudget, LuckExactSession, LuckFamilyChoice, LuckFamilyContext, LuckFamilyLimits, luck_skills,
};
use ournotes_sim::live::model::JudgementStream;
use ournotes_sim::live::score::LiveScoreSettings;
use ournotes_sim::live::skip::{Chart, ChartNote, SkillEvent};
use ournotes_sim::master::Master;
use ournotes_sim::scenario::{ContextInput, PowerSnapshotInput, Scenario};
use serde_json::json;
use std::collections::BTreeMap;

fn fixture() -> (Master, Roster, SearchRequest) {
    let mut source = synth_snaps(&mut Rng::new(961), 6, 2, &[3]);
    set_column(&mut source, "MasterMemberCard", &mut |row| {
        let id = row["_id"].as_i64().unwrap();
        row["_characterID"] = json!(id);
        row["_leaderSkillID"] = json!(0);
        row["_liveSkillID"] = json!(if matches!(id, 1 | 3) { 9400 + id } else { 0 });
        row["_gekisouSkillID"] = json!(if matches!(id, 1 | 2) { 9300 + id } else { 0 });
        // The sixth member is a separate legal incumbent outside the low-power suffix. All uncertainty and
        // nonconstant rewards remain present; a power gap makes the prune witness robust to conservative slack.
        let power = if id == 6 { 50_000 } else { 1_000 + id * 40 };
        for key in ["_performancePowerMax", "_technicPowerMax", "_visualPowerMax"] {
            row[key] = json!(power);
        }
    });
    set_column(&mut source, "MasterSupportCard", &mut |row| {
        let id = row["_id"].as_i64().unwrap();
        row["_supportSkillId01"] = json!(if id == 2 { 9701 } else { 0 });
        row["_supportSkillId02"] = json!(if id == 2 { 9702 } else { 0 });
        row["_gekisouSupportSkillId01"] = json!(if id == 1 { 9601 } else { 0 });
    });
    set_column(&mut source, "MasterSupportCardRank", &mut |row| {
        row["_supportSkill01Level"] = json!(1);
        row["_supportSkill02Level"] = json!(1);
        row["_gekisouSupportSkill01Level"] = json!(1);
    });
    set_column(&mut source, "MasterLiveMusic", &mut |row| {
        row["_gekisouMission1"] = json!(2);
        row["_gekisouMission2"] = json!(2);
        row["_gekisouMission3"] = json!(2);
    });
    extend_table(
        &mut source,
        "MasterLiveSettings",
        vec![
            json!({"_id":30,"_key":"gekisou_luck_gauge_max","_value":"40"}),
            json!({"_id":31,"_key":"gekisou_luck_gauge_max_rush","_value":"20"}),
            json!({"_id":32,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"47"}),
        ],
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouLuckBasePoint",
        json!([
            {"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":20},
            {"_id":2,"_noteCategory":0,"_noteSimulateJudgement":4,"_weight":1,"_basePoint":5}
        ]),
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
        "MasterSkillTarget",
        vec![json!({"_id":9250,"_skillTargetType":5,"_gekisouMissionType":2})],
    );
    extend_table(
        &mut source,
        "MasterSkillCondition",
        vec![
            json!({"_id":9201,"_conditionType":7010,"_conditionValues":[],"_conditionTargetIDs":[9250],"_isPositive":true}),
            json!({"_id":9202,"_conditionType":7013,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}),
            json!({"_id":9203,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}),
            json!({"_id":9204,"_conditionType":4010,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}),
            json!({"_id":9205,"_conditionType":4011,"_conditionValues":[50],"_conditionTargetIDs":[],"_isPositive":true}),
            json!({"_id":9206,"_conditionType":2001,"_conditionValues":[900],"_conditionTargetIDs":[],"_isPositive":true}),
        ],
    );
    extend_table(
        &mut source,
        "MasterSkillConditionSet",
        (9201..=9206).map(|id| json!({"_id":id,"_group":id,"_conditionIds":[id]})).collect(),
    );
    replace_table(
        &mut source,
        "MasterLiveSkillEffect",
        json!([
            {"_id":9401,"_liveSkillID":9401,"_level":1,"_skillEffectType":2000,"_effectValue":3500,"_activationTimeSecond":0.12},
            {"_id":9403,"_liveSkillID":9403,"_level":1,"_skillEffectType":2000,"_effectValue":9000,"_activationTimeSecond":0.2}
        ]),
    );
    replace_table(
        &mut source,
        "MasterGekisouSkill",
        json!([
            {"_id":9301,"_gekisouMissionType":2}, {"_id":9302,"_gekisouMissionType":2}
        ]),
    );
    replace_table(
        &mut source,
        "MasterGekisouSkillEffect",
        json!([
            {"_id":9301,"_gekisouSkillID":9301,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":9201,
             "_skillEffectType":11001,"_effectValue":10000,"_activationTimeSecond":0.2},
            {"_id":9302,"_gekisouSkillID":9302,"_level":1,"_skillTriggerType":2,"_skillTriggerConditionGroup":9203,
             "_skillEffectType":2000,"_effectValue":7000}
        ]),
    );
    replace_table(&mut source, "MasterGekisouSupportSkill", json!([{"_id":9601,"_gekisouMissionType":2}]));
    replace_table(
        &mut source,
        "MasterGekisouSupportSkillEffect",
        json!([
            {"_id":9601,"_gekisouSupportSkillID":9601,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":9201,
             "_skillConditionGroup":9205,"_skillReleaseConditionGroup":9202,"_skillEffectType":11005,
             "_effectValue":4,"_effectLimitCount":1}
        ]),
    );
    replace_table(
        &mut source,
        "MasterSupportSkillEffect",
        json!([
            {"_id":9701,"_supportSkillID":9701,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":9204,
             "_skillEffectType":15000,"_effectValue":240},
            {"_id":9702,"_supportSkillID":9702,"_level":1,"_skillTriggerType":1,"_skillTriggerConditionGroup":9204,
             "_skillEffectType":2000,"_effectValue":1200,"_activationTimeSecond":0.1}
        ]),
    );
    replace_table(
        &mut source,
        "MasterLiveGekisouRankingScoreBonus",
        json!([
            {"_id":1,"_missionPattern":1,"_count":1,"_rank":1,"_scoreBonusPercent":25}
        ]),
    );
    extend_table(
        &mut source,
        "MasterSkillEffectSetting",
        vec![
            json!({"_id":9101,"_skillEffectType":11001,"_phase":2}),
            json!({"_id":9102,"_skillEffectType":11005,"_phase":2}),
        ],
    );
    let master = source.master();
    let mut owned = roster(&mut Rng::new(962), &master);
    for member in &mut owned.members {
        member.level = Some(40);
        member.awake = 1;
        member.rank = 1;
        member.live_skill_level = 1;
        member.gekisou_skill_level = 1;
    }
    for snap in &mut owned.snaps {
        snap.level = Some(30);
        snap.rank = 1;
    }
    let chart = Chart::from_notes(
        [120, 180, 320, 380, 1500]
            .into_iter()
            .enumerate()
            .map(|(id, time_ms)| ChartNote { id: id as i32, time_ms, note_type: 1 })
            .collect(),
        (0..5).map(|index| SkillEvent { index, time_ms: 80 + index * 60 }).collect(),
        &LiveScoreSettings::from_master(&master).unwrap(),
    )
    .unwrap();
    let stream = JudgementStream::theoretical_best(&chart);
    let scene = ContextInput {
        power_snapshot: PowerSnapshotInput { event_ids: vec![], captured_jst_ticks: None },
        result_clock: None,
        event_payoff: None,
    }
    .resolve(&master, Scenario::Mission(10), Some(1004), &[(100, 420)])
    .unwrap();
    let objective = Objective::LiveScore {
        score_id: 1004,
        chart,
        play: PlayInput::Stream { stream, judgement_types: vec![1; 5] },
        event: false,
        exclude_snap_skills: false,
        gekisou: Some(GekisouObjective { seeds: SeedSet::List(vec![0]), fevers: vec![(100, 420)] }),
    }
    .in_scenario(scene);
    (
        master,
        owned,
        SearchRequest {
            objective,
            constraints: Constraints { leader: Some(1), include_members: vec![1, 2, 3, 4], ..Default::default() },
            k: 3,
            time_limit: None,
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rational(i128, i128);

impl Rational {
    const ZERO: Self = Self(0, 1);
    const ONE: Self = Self(1, 1);

    fn new(numerator: i128, denominator: i128) -> Self {
        assert!(denominator > 0);
        let (mut a, mut b) = (numerator.abs(), denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self(numerator / a, denominator / a)
    }

    fn add(self, other: Self) -> Self {
        Self::new(
            self.0.checked_mul(other.1).unwrap().checked_add(other.0.checked_mul(self.1).unwrap()).unwrap(),
            self.1.checked_mul(other.1).unwrap(),
        )
    }

    fn at_most_integer(self, upper: i128) -> bool {
        self.0 <= upper.checked_mul(self.1).unwrap()
    }
}

fn independent_orders() -> Vec<[usize; 5]> {
    fn visit(prefix: &mut Vec<usize>, out: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            out.push(prefix.as_slice().try_into().unwrap());
            return;
        }
        for next in 0..5 {
            if !prefix.contains(&next) {
                prefix.push(next);
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

fn legal_descendants(pool: &Pool, domain: &CandidateDomain) -> Vec<PhysicalDeck> {
    // Enumerate declared resources directly; do not use the new cache's prefix/suffix expansion, classes,
    // profiles, or representatives. The fixture fixes cards 1..4 and admits exactly cards 5 or 6 as the fifth.
    let mut out = Vec::new();
    for last in [4, 5] {
        let members = [1, 2, 0, 3, last];
        fn bind(
            pool: &Pool,
            domain: &CandidateDomain,
            deck: &mut PhysicalDeck,
            slot: usize,
            out: &mut Vec<PhysicalDeck>,
        ) {
            if slot == 5 {
                domain.check_fixed(pool, deck).unwrap();
                out.push(*deck);
                return;
            }
            deck.snaps[slot] = None;
            bind(pool, domain, deck, slot + 1, out);
            for &resource in domain.snaps() {
                if deck.snaps[..slot].contains(&Some(resource)) {
                    continue;
                }
                deck.snaps[slot] = Some(resource);
                bind(pool, domain, deck, slot + 1, out);
            }
        }
        bind(pool, domain, &mut PhysicalDeck { members, snaps: [None; 5] }, 0, &mut out);
    }
    assert_eq!(out.len(), 62, "two complete member families, each with all 31 legal Snap assignments");
    out
}

#[derive(Clone, Debug)]
struct NativeCandidate {
    physical: PhysicalDeck,
    sum_of_order_means: Rational,
    order_means: Vec<Rational>,
    nonconstant: bool,
}

fn native_candidates(pool: &Pool, request: &SearchRequest, domain: &CandidateDomain) -> Vec<NativeCandidate> {
    let orders = independent_orders();
    legal_descendants(pool, domain)
        .into_iter()
        .map(|physical| {
            let input = expectation::context(pool, &physical, &request.objective).unwrap();
            let mut native = LuckExactSession::new(
                pool.master,
                &input.notes,
                &input.events,
                input.params,
                input.gekisou.as_ref().unwrap(),
                &input.play,
                &input.delta_times,
                None,
                0,
            )
            .unwrap();
            let mut sum_of_order_means = Rational::ZERO;
            let mut order_means = Vec::new();
            let mut nonconstant = false;
            for order in &orders {
                let deck = order.map(|slot| input.performers[slot].clone());
                let attempt = native.law(&deck, &mut LuckExactBudget::default(), || false).unwrap();
                let law = attempt.law.unwrap_or_else(|| {
                    panic!(
                        "complete native oracle declined {:?}; stats={:?}; members={:?}; snaps={:?}; order={order:?}",
                        attempt.decline, attempt.stats, physical.members, physical.snaps
                    )
                });
                let mut mass = Rational::ZERO;
                let mut mean = Rational::ZERO;
                nonconstant |= law.atoms().windows(2).any(|pair| pair[0].score != pair[1].score);
                for atom in law.atoms() {
                    let numerator = i128::try_from(atom.mass.numerator).unwrap();
                    let denominator = i128::try_from(atom.mass.denominator).unwrap();
                    mass = mass.add(Rational::new(numerator, denominator));
                    mean = mean.add(Rational::new(numerator * i128::from(atom.score), denominator));
                }
                assert_eq!(mass, Rational::ONE);
                sum_of_order_means = sum_of_order_means.add(mean);
                order_means.push(mean);
            }
            assert_eq!(order_means.len(), 120);
            NativeCandidate { physical, sum_of_order_means, order_means, nonconstant }
        })
        .collect()
}

fn family_choices(pool: &Pool, domain: &CandidateDomain, members: [usize; 5]) -> [Vec<LuckFamilyChoice>; 5] {
    std::array::from_fn(|slot| {
        std::iter::once(None)
            .chain(domain.snaps().iter().copied().map(Some))
            .map(|resource| LuckFamilyChoice {
                resource,
                performer: crate::search::snaps::performer(
                    &pool.members[members[slot]],
                    resource.map(|s| &pool.snaps[s]),
                )
                .unwrap(),
            })
            .collect()
    })
}

fn prefix_of(physical: &PhysicalDeck) -> [Option<usize>; 4] {
    std::array::from_fn(|index| physical.snaps[SLOTS[index]])
}

fn last_choice(domain: &CandidateDomain, physical: &PhysicalDeck) -> usize {
    physical.snaps[SLOTS[4]].map_or(0, |snap| domain.snaps().iter().position(|&item| item == snap).unwrap() + 1)
}

fn same_prefix(left: &PhysicalDeck, right: &PhysicalDeck) -> bool {
    SLOTS[..4].iter().all(|&slot| left.members[slot] == right.members[slot] && left.snaps[slot] == right.snaps[slot])
}

#[test]
fn family_node_masks_and_real_depth_four_suffix_bound_every_native_descendant() {
    let (master, owned, request) = fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let oracles = native_candidates(&pool, &request, &domain);
    assert_eq!(oracles.len(), 62);
    assert_eq!(oracles.iter().map(|oracle| oracle.order_means.len()).sum::<usize>(), 62 * 120);
    assert!(oracles.iter().any(|oracle| oracle.nonconstant));
    let input = expectation::context(&pool, &oracles[0].physical, &request.objective).unwrap();
    let skills = luck_skills(&master).unwrap();
    let context = LuckFamilyContext::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        || false,
    )
    .unwrap()
    .unwrap();
    let mut curves = LuckDpCache::new(8 * 1024 * 1024);
    let mut prefixes = BTreeMap::new();
    for oracle in &oracles {
        prefixes.entry(prefix_of(&oracle.physical)).or_insert(oracle.physical);
    }
    assert_eq!(prefixes.len(), 21);
    let template = bounds.family_reward_template().expect("eligible Score-only reward template");
    let mut checked_masks = 0;
    let mut strictly_tightened = 0;
    let mut prune_witnesses = 0;
    let witnesses: Vec<_> = oracles.iter().filter(|oracle| oracle.physical.members[4] == 5).take(request.k).collect();
    assert_eq!(witnesses.len(), request.k);
    assert_eq!(
        witnesses.iter().map(|oracle| oracle.physical).collect::<std::collections::BTreeSet<_>>().len(),
        request.k,
        "Top-K exclusion needs K distinct legal physical candidates, including tied identities"
    );
    for last in [4, 5] {
        let members = [1, 2, 0, 3, last];
        let family = context
            .prepare(
                &family_choices(&pool, &domain, members),
                Some(&mut curves),
                LuckFamilyLimits {
                    max_pair_models: 64,
                    max_profiles: 6,
                    max_order_evaluations: 720,
                    max_frame_work: 1_000_000,
                    max_retained_bytes: 32 * 1024 * 1024,
                },
                || false,
            )
            .unwrap()
            .unwrap();
        let table = template.bind(members, &family, &mut || false).unwrap();
        for physical in prefixes.values() {
            let mut physical = *physical;
            physical.members[4] = last;
            for allowed in [&[0, 1, 2][..], &[0][..], &[1][..], &[2][..], &[0, 1][..], &[0, 2][..], &[1, 2][..]] {
                let descendants: Vec<_> = oracles
                    .iter()
                    .filter(|oracle| {
                        oracle.physical.members == members
                            && same_prefix(&physical, &oracle.physical)
                            && allowed.contains(&last_choice(&domain, &oracle.physical))
                    })
                    .collect();
                let cap = bounds.family_mask_upper(&domain, &physical, &table, allowed);
                if descendants.is_empty() {
                    assert!(cap.is_none(), "an infeasible physical assignment has no numeric witness");
                    continue;
                }
                let cap = cap.expect("complete family and a nonempty feasible mask");
                for oracle in descendants {
                    assert!(
                        oracle.sum_of_order_means.at_most_integer(cap),
                        "cap {cap} omitted descendant {:?}: {:?}",
                        oracle.physical,
                        oracle.sum_of_order_means
                    );
                }
                // Neither a prior leaf's valid tail Snap nor a completely unrelated stale index is assigned.
                let mut stale = physical;
                for tail in [None, Some(0), Some(1), Some(usize::MAX)] {
                    stale.snaps[SLOTS[4]] = tail;
                    assert_eq!(bounds.family_mask_upper(&domain, &stale, &table, allowed), Some(cap));
                }
                if last == 4
                    && witnesses.iter().all(|witness| {
                        cap.checked_mul(witness.sum_of_order_means.1).unwrap() < witness.sum_of_order_means.0
                    })
                {
                    assert!(
                        witnesses.iter().all(|witness| witness.nonconstant),
                        "all K pruning witnesses retain a nonconstant native law"
                    );
                    prune_witnesses += 1;
                }
                if allowed == [0] && prefix_of(&physical) == [None; 4] {
                    let original = bounds.upper_keyed(&pool, &domain, &physical, 5, &[0, 1, 2, 3, 4], None).0 * 120;
                    strictly_tightened += usize::from(cap < original);
                }
                checked_masks += 1;
            }
        }
    }
    assert!(checked_masks > 0 && strictly_tightened > 0 && prune_witnesses > 0);

    let mut cache = FamilyNodeCache::new(Some(&context), 16, 32 * 1024 * 1024, 6);
    let mut checked_nodes = 0;
    let mut checked_descendants = 0;
    // Include every original suffix boundary, not a chosen convenient member slice.
    for physical in prefixes.values() {
        for start in 0..=bounds.choices.len() {
            let descendants: Vec<_> = oracles
                .iter()
                .filter(|oracle| {
                    same_prefix(physical, &oracle.physical)
                        && bounds.choices[start..]
                            .contains(&(oracle.physical.members[SLOTS[4]], last_choice(&domain, &oracle.physical)))
                })
                .collect();
            let outcome = cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                physical,
                start,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || false,
            );
            let FamilyNodeOutcome::Upper(cap) = outcome else {
                panic!("every family is admitted in this fixture; start={start}, outcome={outcome:?}");
            };
            if descendants.is_empty() {
                assert_eq!(cap, 0);
            }
            for oracle in descendants {
                assert!(
                    oracle.sum_of_order_means.at_most_integer(cap),
                    "depth-four suffix {start} excluded {:?}",
                    oracle.physical
                );
                checked_descendants += 1;
            }
            let mut stale = *physical;
            stale.members[SLOTS[4]] = usize::MAX;
            stale.snaps[SLOTS[4]] = Some(usize::MAX);
            assert_eq!(
                cache.upper_at_depth_four(
                    &pool,
                    &domain,
                    &bounds,
                    &stale,
                    start,
                    &crate::search::uniform::MEAN_ORDERS,
                    &mut curves,
                    &mut || false
                ),
                outcome,
                "the unassigned physical tail is not part of the node identity"
            );
            checked_nodes += 1;
        }
    }
    assert_eq!(checked_nodes, prefixes.len() * (bounds.choices.len() + 1));
    assert!(checked_descendants >= 62);
    assert_eq!(cache.stats().prepared_families, 2);
    assert!(cache.stats().family_hits > 0);
}

#[test]
fn family_node_disabled_cache_capacity_profile_budget_and_cancellation_keep_real_states() {
    let (master, owned, request) = fixture();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let physical = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [None; 5] };
    let input = expectation::context(&pool, &physical, &request.objective).unwrap();
    let skills = luck_skills(&master).unwrap();
    let context = LuckFamilyContext::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        || false,
    )
    .unwrap()
    .unwrap();
    let mut curves = LuckDpCache::new(0);
    for (entries, bytes, profiles) in [(0, 32 << 20, 6), (16, 0, 6), (16, 32 << 20, 0), (16, 1, 6), (16, 32 << 20, 5)] {
        let mut cache = FamilyNodeCache::new(Some(&context), entries, bytes, profiles);
        assert_eq!(
            cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &physical,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || false
            ),
            FamilyNodeOutcome::Unavailable
        );
        assert_eq!(cache.stats().prepared_families, 0, "no incomplete profile table is counted as prepared");
        assert_eq!(cache.stats().order_laws, 0, "no failed partial profile/order product is published");
    }
    let mut cache = FamilyNodeCache::new(Some(&context), 16, 32 << 20, 6);
    assert_eq!(
        cache.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &crate::search::uniform::MEAN_ORDERS,
            &mut curves,
            &mut || true
        ),
        FamilyNodeOutcome::Stopped
    );
    let polls = std::cell::Cell::new(0);
    assert_eq!(
        cache.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &crate::search::uniform::MEAN_ORDERS,
            &mut curves,
            &mut || {
                polls.set(polls.get() + 1);
                polls.get() >= 16
            }
        ),
        FamilyNodeOutcome::Stopped
    );
    assert_eq!(cache.stats().prepared_families, 0);
    let uncached_curves = cache.upper_at_depth_four(
        &pool,
        &domain,
        &bounds,
        &physical,
        0,
        &crate::search::uniform::MEAN_ORDERS,
        &mut curves,
        &mut || false,
    );
    assert!(matches!(uncached_curves, FamilyNodeOutcome::Upper(_)));
    assert_eq!(cache.stats().prepared_families, 2);
    let mut cached_curves = LuckDpCache::new(8 << 20);
    let mut second = FamilyNodeCache::new(Some(&context), 16, 32 << 20, 6);
    assert_eq!(
        second.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &crate::search::uniform::MEAN_ORDERS,
            &mut cached_curves,
            &mut || false
        ),
        uncached_curves
    );
    // An expectation cap is only valid for the declared uniform 120-order target.
    assert_eq!(
        second.upper_at_depth_four(
            &pool,
            &domain,
            &bounds,
            &physical,
            0,
            &[([0, 1, 2, 3, 4], 1)],
            &mut cached_curves,
            &mut || false
        ),
        FamilyNodeOutcome::Unavailable
    );
    for metric in [
        Metric::ScoreAtLeast { threshold: 10_000 },
        Metric::CappedScore { threshold: 10_000 },
        Metric::ScoreAndLifeAtLeast { threshold: 10_000, min_final_life: 900 },
    ] {
        let nonlinear =
            JointBounds::compile(&pool, &request, &domain, &metric, None, &SimulationInput::default()).unwrap();
        assert_eq!(
            second.upper_at_depth_four(
                &pool,
                &domain,
                &nonlinear,
                &physical,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut cached_curves,
                &mut || false
            ),
            FamilyNodeOutcome::Unavailable,
            "a Score mean is never a nonlinear-payoff certificate"
        );
    }
}

#[test]
fn one_refused_member_family_prevents_a_partial_depth_four_cap() {
    let (mut master, owned, request) = fixture();
    master.member_cards[5].gekisou_skill_id = 9801;
    master.gekisou_skills.push(ournotes_sim::master::SkillRow {
        id: 9801,
        gekisou_mission_type: 2,
        ..Default::default()
    });
    master.gekisou_skill_effects.push(ournotes_sim::master::GekisouSkillEffectRow {
        id: 9801,
        skill_id: 9801,
        level: 1,
        skill_trigger_type: 1,
        skill_trigger_condition_group: 9201,
        skill_condition_group: 9206,
        skill_effect_type: 11001,
        effect_value: 10000,
        activation_time_second: 0.2,
        ..Default::default()
    });
    master.reindex().unwrap();
    let pool = Pool::new(&master, &owned).unwrap();
    let domain = CandidateDomain::build(&pool, &request.constraints).unwrap();
    let bounds =
        JointBounds::compile(&pool, &request, &domain, &Metric::Score, None, &SimulationInput::default()).unwrap();
    let physical = PhysicalDeck { members: [1, 2, 0, 3, 4], snaps: [None; 5] };
    let input = expectation::context(&pool, &physical, &request.objective).unwrap();
    let skills = luck_skills(&master).unwrap();
    let context = LuckFamilyContext::new(
        &master,
        &skills,
        &input.notes,
        &input.events,
        input.params,
        input.gekisou.as_ref().unwrap(),
        &input.play,
        &input.delta_times,
        || false,
    )
    .unwrap()
    .unwrap();
    let mut curves = LuckDpCache::new(8 << 20);
    let mut cache = FamilyNodeCache::new(Some(&context), 16, 32 << 20, 6);
    for _ in 0..2 {
        assert_eq!(
            cache.upper_at_depth_four(
                &pool,
                &domain,
                &bounds,
                &physical,
                0,
                &crate::search::uniform::MEAN_ORDERS,
                &mut curves,
                &mut || false
            ),
            FamilyNodeOutcome::Unavailable,
            "the successful fifth-member alternative cannot conceal a refused sixth-member alternative"
        );
    }
    assert_eq!(cache.stats().prepared_families, 1);
    assert!(cache.stats().preparation_refusals > 0 && cache.stats().refused_hits > 0);
}

#[path = "family_node_e2e_tests.rs"]
mod end_to_end;
