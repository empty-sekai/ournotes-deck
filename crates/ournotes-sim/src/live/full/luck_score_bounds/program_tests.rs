use super::*;
use crate::live::random::LiveRandom;

mod replay_program_tests {
    include!("replay_program_tests.rs");
}

type ProgramFixture = (Master, Vec<LiveNote>, LiveParams, GekisouSetup, LivePlay, Vec<f32>);

#[derive(Clone)]
struct ProgramCase {
    master: Master,
    notes: Vec<LiveNote>,
    params: LiveParams,
    setup: GekisouSetup,
    play: LivePlay,
    delta: Vec<f32>,
    deck: Vec<Performer>,
    events: Vec<(i32, i32)>,
    ranking: Option<Vec<crate::replay::RankConfirmation>>,
}

impl ProgramCase {
    fn new((master, notes, params, setup, play, delta): ProgramFixture) -> Self {
        Self { master, notes, params, setup, play, delta, deck: Vec::new(), events: Vec::new(), ranking: None }
    }

    fn run(
        &self,
        cache: Option<&mut LuckDpCache>,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Option<LuckScoreSummary>, Error> {
        let skills = luck_skills(&self.master)?;
        let mut session = LuckScoreSession::new(
            &self.master,
            &skills,
            &self.notes,
            &self.events,
            self.params,
            &self.setup,
            &self.play,
            &self.delta,
            self.ranking.as_deref(),
        );
        session.summary(&self.deck, cache, cancelled)
    }

    fn direct(&self) -> LuckScoreSummary {
        self.run(None, || false).unwrap().unwrap()
    }

    fn cached(&self, cache: &mut LuckDpCache) -> LuckScoreSummary {
        self.run(Some(cache), || false).unwrap().unwrap()
    }
}

fn assert_program_summary(actual: &LuckScoreSummary, expected: &LuckScoreSummary) {
    let words = |value: &LuckScoreSummary| {
        (
            value.final_mean.lower.to_bits(),
            value.final_mean.upper.to_bits(),
            value.final_support.lower,
            value.final_support.upper,
            value.exact_constant_score,
            value.exact_final_life,
            value.probability_peak_states,
            value.probability_transitions,
        )
    };
    assert_eq!(words(actual), words(expected));
}

#[test]
fn power_programs_recompute_native_rounding_after_late_rank_queries() {
    for fine in [false, true] {
        let mut input = ProgramCase::new(lottery_wide_ranges(fine));
        let mut cache = LuckDpCache::new(8 << 20);
        let powers = [1000, 0, 1, 999, 1001, 16_777_215, 16_777_217];
        for power in powers {
            input.params.total_power = power;
            let expected = input.direct();
            let actual = input.cached(&mut cache);
            assert_program_summary(&actual, &expected);
        }
        let stats = cache.stats();
        assert!(stats.program_compilations > 0);
        assert!(stats.program_hits >= powers.len() as u64 - 1, "fine={fine}: {stats:?}");
        assert!(stats.program_peak_entries > 0 && stats.program_peak_bytes <= 8 << 20);
    }
}

#[test]
fn power_programs_keep_only_the_last_rank_pending_before_a_query() {
    let mut input = ProgramCase::new(lottery_wide_ranges(false));
    input.ranking = Some(
        [23, 57, 91]
            .into_iter()
            .enumerate()
            .map(|(range, percent)| crate::replay::RankConfirmation {
                frame: 100,
                range,
                rank: range as i32 + 1,
                percent,
            })
            .collect(),
    );
    let mut cache = LuckDpCache::new(8 << 20);
    for power in [1000, 1001, 999] {
        input.params.total_power = power;
        assert_program_summary(&input.cached(&mut cache), &input.direct());
    }
    assert!(cache.stats().program_hits >= 2);
}

#[test]
fn complete_order_cycles_reuse_programs_at_the_next_power() {
    let mut input = ProgramCase::new(fixture());
    for id in 901..=905 {
        input.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
            id,
            live_skill_id: id,
            level: 1,
            skill_effect_type: 2000,
            effect_value: 1000 + (id - 901) * 777,
            activation_time_second: 0.2,
            ..Default::default()
        });
    }
    input.master.reindex().unwrap();
    input.events = [80, 120, 200, 260, 300].into_iter().enumerate().map(|(slot, time)| (slot as i32, time)).collect();
    let performers: Vec<_> =
        (901..=905).map(|id| Performer { live_skill: Some((id, 1)), ..Default::default() }).collect();
    fn extend(prefix: &mut Vec<usize>, used: u8, orders: &mut Vec<[usize; 5]>) {
        if prefix.len() == 5 {
            orders.push(prefix.as_slice().try_into().unwrap());
        } else {
            for slot in 0..5 {
                if used & (1 << slot) == 0 {
                    prefix.push(slot);
                    extend(prefix, used | (1 << slot), orders);
                    prefix.pop();
                }
            }
        }
    }
    let mut orders = Vec::new();
    extend(&mut Vec::new(), 0, &mut orders);
    assert_eq!(orders.len(), 120);
    let mut cache = LuckDpCache::new(32 << 20);
    for order in &orders {
        input.deck = order.iter().map(|&slot| performers[slot].clone()).collect();
        input.cached(&mut cache);
    }
    let first = cache.stats();
    assert_eq!(first.program_compilations, 120, "all distinct order programs must be recorded");
    assert_eq!(first.program_hits, 0);
    input.params.total_power += 1;
    for order in &orders {
        input.deck = order.iter().map(|&slot| performers[slot].clone()).collect();
        assert_program_summary(&input.cached(&mut cache), &input.direct());
    }
    let second = cache.stats();
    assert_eq!(second.program_hits - first.program_hits, 120, "the full order cycle must remain reusable");
    assert_eq!(second.program_compilations, first.program_compilations);
    assert!(second.program_peak_entries >= 120 && second.program_peak_entries <= 128);
    assert!(second.program_peak_bytes <= 32 << 20);
}

#[test]
fn power_programs_keep_every_declared_run_input_in_the_cache_scope() {
    let base = ProgramCase::new(fixture());
    let mut cache = LuckDpCache::new(8 << 20);
    assert_program_summary(&base.cached(&mut cache), &base.direct());
    assert!(cache.stats().program_peak_entries > 0);
    for change in 0..8 {
        let mut input = base.clone();
        input.params.total_power = 1001;
        match change {
            0 => input.params.assist_factor = 0.75,
            1 => input.master.note_parameters[0].score_percent += 1,
            2 => input.delta[0] = f32::from_bits(input.delta[0].to_bits() + 1),
            3 => input.play.base_seed = 19,
            4 => {
                input.play.frames.push(PlayFrame { time_ms: 2100, judged: Vec::new() });
                input.delta.push(0.1);
            }
            5 => input.events.push((0, 90)),
            6 => input.setup.fevers[0].0 = 0,
            7 => {
                input.ranking =
                    Some(vec![crate::replay::RankConfirmation { frame: 0, range: 0, rank: 2, percent: 75 }]);
            }
            _ => unreachable!(),
        }
        input.master.reindex().unwrap();
        let before = cache.stats().program_hits;
        assert_program_summary(&input.cached(&mut cache), &input.direct());
        assert_eq!(cache.stats().program_hits, before, "changed run input {change} reused a prior program");
        let first = cache.stats();
        input.params.total_power += 1;
        assert_program_summary(&input.cached(&mut cache), &input.direct());
        let second = cache.stats();
        assert_eq!(
            second.program_hits + second.program_recorded_hits,
            first.program_hits + first.program_recorded_hits + 1,
            "new scope {change} did not reuse its complete program",
        );
    }
}

#[test]
fn power_programs_preserve_the_member_paired_with_a_conditional_snap() {
    let mut input = ProgramCase::new(fixture());
    input
        .master
        .skill_targets
        .push(serde_json::from_value(json!({"_id":901,"_skillTargetType":1,"_characterID":7})).unwrap());
    for (id, kind, targets) in [(901, 5000, vec![901]), (902, 4010, Vec::new())] {
        input.master.skill_conditions.push(
            serde_json::from_value(json!({"_id":id,"_conditionType":kind,"_conditionValues":[],
                "_conditionTargetIDs":targets,"_isPositive":true}))
            .unwrap(),
        );
        input
            .master
            .skill_condition_sets
            .push(serde_json::from_value(json!({"_id":id,"_group":id,"_conditionIds":[id]})).unwrap());
    }
    input
        .master
        .skill_effect_settings
        .push(serde_json::from_value(json!({"_id":901,"_skillEffectType":2000,"_phase":2})).unwrap());
    input.master.support_skill_effects.push(
        serde_json::from_value(json!({"_id":901,"_supportSkillID":901,"_level":1,
            "_skillTriggerType":1,"_skillTriggerConditionGroup":902,"_skillConditionGroup":901,
            "_skillEffectType":2000,"_effectValue":5000,"_activationTimeSecond":0.4}))
        .unwrap(),
    );
    input.master.reindex().unwrap();
    input.events = vec![(0, 80), (1, 90)];
    let mut cache = LuckDpCache::new(8 << 20);
    let mut means = Vec::new();
    for (owner, power) in [(0, 1000), (1, 1000), (0, 1001), (1, 1001)] {
        input.params.total_power = power;
        input.deck = vec![
            Performer { character_id: 7, ..Default::default() },
            Performer { character_id: 8, ..Default::default() },
        ];
        input.deck[owner].support_skills.push((901, 1));
        let before = cache.stats().program_hits;
        let actual = input.cached(&mut cache);
        assert_program_summary(&actual, &input.direct());
        assert_eq!(cache.stats().program_hits - before, u64::from(power == 1001));
        means.push(actual.final_mean);
    }
    assert!(means[0].lower > means[1].upper, "the paired-member predicate must change the score");
}

#[test]
fn power_programs_preserve_cancellation_numeric_refusal_and_zero_capacity() {
    let mut input = ProgramCase::new(fixture());
    let mut cache = LuckDpCache::new(8 << 20);
    let original = input.cached(&mut cache);
    let before = cache.stats();
    input.params.total_power = 1001;
    assert!(input.run(Some(&mut cache), || true).unwrap().is_none());
    assert_eq!(cache.stats().program_hits, before.program_hits);
    assert_eq!(cache.stats().program_compilations, before.program_compilations);

    let mut checks = 0;
    let complete = input
        .run(Some(&mut cache), || {
            checks += 1;
            false
        })
        .unwrap()
        .unwrap();
    assert_program_summary(&complete, &input.direct());
    assert!(checks > 1);
    let mut interrupted = 0;
    assert!(
        input
            .run(Some(&mut cache), || {
                interrupted += 1;
                interrupted >= checks - 1
            })
            .unwrap()
            .is_none()
    );
    assert_program_summary(&input.cached(&mut cache), &complete);

    input.params.total_power = i32::MAX;
    let expected_error = input.run(None, || false).unwrap_err().to_string();
    let actual_error = input.run(Some(&mut cache), || false).unwrap_err().to_string();
    assert_eq!(actual_error, expected_error);
    input.params.total_power = 1000;
    assert_program_summary(&input.cached(&mut cache), &original);
    for capacity in [0, 1] {
        let mut disabled = LuckDpCache::new(capacity);
        for power in [1000, 1001] {
            input.params.total_power = power;
            assert_program_summary(&input.cached(&mut disabled), &input.direct());
        }
        assert_eq!(disabled.stats().program_hits, 0);
        assert_eq!(disabled.stats().program_peak_entries, 0);
        assert_eq!(disabled.stats().program_peak_bytes, 0);
    }
    input.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
        id: 901,
        live_skill_id: 901,
        level: 1,
        skill_effect_type: 3000,
        effect_value: 100,
        ..Default::default()
    });
    input.master.reindex().unwrap();
    input.deck = vec![Performer { live_skill: Some((901, 1)), ..Default::default() }];
    let before = cache.stats().program_hits;
    assert!(matches!(input.run(Some(&mut cache), || false), Err(Error::Unsupported(_))));
    assert_eq!(cache.stats().program_hits, before);
}

#[test]
fn power_program_storage_obeys_its_byte_limit_and_eviction_keeps_results() {
    let base = ProgramCase::new(fixture());
    let mut probe = LuckDpCache::new(8 << 20);
    base.cached(&mut probe);
    let bytes = probe.stats().program_peak_bytes;
    assert!(bytes > 0);
    let capacity = bytes + bytes / 4;
    let mut cache = LuckDpCache::new(capacity);
    for assist_factor in [1.0, 0.75, 0.5, 1.0] {
        let mut input = base.clone();
        input.params.assist_factor = assist_factor;
        assert_program_summary(&input.cached(&mut cache), &input.direct());
    }
    let stats = cache.stats();
    assert!(stats.program_evictions > 0, "bounded programs must evict earlier scopes: {stats:?}");
    assert!(stats.program_peak_entries > 0);
    assert!(stats.program_peak_bytes <= capacity);
}

#[test]
fn compiled_power_bounds_cover_the_complete_weighted_cartesian_law() {
    let mut input = ProgramCase::new(fixture());
    for row in &mut input.master.live_settings {
        if matches!(row.key.as_str(), "gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush") {
            row.value = "10".into();
        }
    }
    input.master.gekisou_luck_base_points[0].base_point = 10;
    input.master.gekisou_luck_bonus_lots = (0..5)
        .flat_map(|kind| {
            [0, 3].map(move |result| crate::master::LuckBonusLotRow {
                id: kind * 4 + result + 1,
                chance_lot_type: kind,
                lot_result: result,
                weight: if result == 3 { 3 } else { 1 },
            })
        })
        .collect();
    input.master.reindex().unwrap();
    input.notes = vec![LiveNote { note_id: 1, note_operate_type: 1, judgement_type: 1, time_ms: 100 }];
    input.params.converted_note_count = 1;
    input.params.music_length_ms = 3200;
    input.setup.fevers = vec![(0, 150)];
    input.play.frames = (0..=30)
        .map(|index| PlayFrame {
            time_ms: index * 100,
            judged: if index == 1 {
                vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 100 }]
            } else {
                Vec::new()
            },
        })
        .collect();
    input.delta = vec![0.1; input.play.frames.len()];
    input.ranking = Some(vec![crate::replay::RankConfirmation { frame: 0, range: 0, rank: 1, percent: 23 }]);
    let mut cache = LuckDpCache::new(8 << 20);
    for power in [999, 1000, 16_777_217] {
        input.params.total_power = power;
        let summary = input.cached(&mut cache);
        assert_program_summary(&summary, &input.direct());
        let mut scores = std::collections::BTreeSet::new();
        let (mut mass, mut weighted_score) = (0i128, 0i128);
        // The two bonus draws use Critical/Miss weights 3/1. The second draw is the next prepared bonus.
        // Enumerate the fixed Cartesian domain independently of both the DP and exact-law tree walker.
        for first in 0..2 {
            for next in 0..2 {
                let weight = [3i128, 1][first] * [3i128, 1][next];
                let mut native =
                    LiveModel::new_gekisou_external(&input.master, &[], &input.notes, &[], input.params, &input.setup)
                        .unwrap();
                native.set_rank_confirmation_timeline(input.ranking.as_ref().unwrap()).unwrap();
                native
                    .run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(vec![first, next]))
                    .unwrap();
                assert!(native.random.nominal_prefix_consumed() && native.random.nominal_covers_draws());
                assert!(summary.final_support.lower <= native.score() && native.score() <= summary.final_support.upper);
                assert_eq!(summary.exact_final_life, Some(native.current_life()));
                mass += weight;
                weighted_score += weight * i128::from(native.score());
                scores.insert(native.score());
            }
        }
        assert_eq!(mass, 16);
        assert!(scores.len() > 1);
        let mean = weighted_score as f64 / mass as f64;
        assert!(summary.final_mean.lower <= mean && mean <= summary.final_mean.upper);
    }
    assert!(cache.stats().program_hits >= 2);
}
