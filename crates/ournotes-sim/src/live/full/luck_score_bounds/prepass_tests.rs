use super::super::prepass::terminal_notes;
use super::*;
use crate::live::random::LiveRandom;
use std::collections::BTreeMap;
use std::sync::Arc;

#[path = "record_only_tests.rs"]
mod record_only_tests;

#[path = "family_tests.rs"]
mod family_tests;

#[path = "terminal_expectation_tests.rs"]
mod terminal_expectation_tests;

#[path = "rank_residue_tests.rs"]
mod rank_residue_tests;

#[path = "terminal_cache_tests.rs"]
mod terminal_cache_tests;

#[path = "score_cap_identity_tests.rs"]
mod score_cap_identity_tests;

#[derive(Clone)]
struct RushCase {
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

impl RushCase {
    fn new(gap: i32, mission_gate: i64, critical_weight: i64) -> Self {
        let (mut master, _, mut params, mut setup, _, _) = fixture();
        for row in &mut master.live_settings {
            match row.key.as_str() {
                "gekisou_luck_gauge_max" | "gekisou_luck_gauge_max_rush" => row.value = "10".into(),
                "gekisou_luck_rush_score_bonus_percent" => row.value = "47".into(),
                _ => {}
            }
        }
        master.gekisou_luck_base_points[0].base_point = 10;
        master.gekisou_luck_bonus_lots = (0..5)
            .flat_map(|kind| {
                [(0, 1), (3, critical_weight)].map(move |(result, weight)| crate::master::LuckBonusLotRow {
                    id: kind * 4 + result + 1,
                    chance_lot_type: kind,
                    lot_result: result,
                    weight,
                })
            })
            .collect();
        master.gekisou_skills.push(crate::master::SkillRow {
            id: 901,
            gekisou_mission_type: mission_gate,
            ..Default::default()
        });
        master.gekisou_support_skills.push(crate::master::SkillRow {
            id: 902,
            gekisou_mission_type: mission_gate,
            ..Default::default()
        });
        master.skill_conditions.push(crate::master::SkillConditionRow {
            id: 7021,
            condition_type: 7021,
            condition_values: Vec::new(),
            condition_target_ids: Vec::new(),
            is_positive: true,
        });
        master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
            id: 7021,
            group: 7021,
            condition_ids: vec![7021],
        });
        master.gekisou_support_skill_effects.push(crate::master::GekisouSkillEffectRow {
            id: 902,
            skill_id: 902,
            level: 1,
            skill_trigger_type: SUSTAINED,
            skill_trigger_condition_group: 7021,
            skill_effect_type: 2000,
            effect_value: 5500,
            ..Default::default()
        });
        master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
            id: 903,
            live_skill_id: 903,
            level: 1,
            skill_effect_type: 2000,
            effect_value: 12345,
            activation_time_second: 0.35,
            ..Default::default()
        });
        master.gekisou_ranking_score_bonuses = (1..=3)
            .map(|count| crate::master::GekisouRankingBonusRow {
                id: count,
                mission_pattern: gekisou::mission_pattern(2, 2, 2),
                rank: 1,
                count,
                score_bonus_percent: 23,
            })
            .collect();
        master.reindex().unwrap();
        setup.fevers = vec![(100, 160), (gap + 100, gap + 160), (2 * gap + 100, 2 * gap + 160)];
        // The first two notes share their first declared play frame and can file Critical then Miss there.
        // Notes between ranges also observe the score command retained while its mission gate is closed.
        let notes: Vec<_> = [110, 120, gap - 100, gap + 100, 2 * gap - 100, 2 * gap + 100]
            .into_iter()
            .enumerate()
            .map(|(note_id, time_ms)| LiveNote {
                note_id: note_id as i32,
                note_operate_type: 1,
                judgement_type: 1,
                time_ms,
            })
            .collect();
        params.converted_note_count = notes.len() as i32;
        params.music_length_ms = 2 * gap + 2400;
        let frames: Vec<_> = (0..=params.music_length_ms / 100)
            .map(|frame| PlayFrame {
                time_ms: frame * 100,
                judged: notes
                    .iter()
                    .filter(|note| (note.time_ms + 99) / 100 == frame)
                    .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
                    .collect(),
            })
            .collect();
        let delta = vec![0.1; frames.len()];
        Self {
            master,
            notes,
            params,
            setup,
            play: LivePlay { frames, base_seed: 0 },
            delta,
            deck: vec![Performer {
                character_id: 7,
                live_skill: Some((903, 1)),
                gekisou_skill: Some((901, 1)),
                gekisou_support_skills: vec![(902, 1)],
                ..Default::default()
            }],
            events: vec![(0, 90), (0, gap + 90)],
            ranking: None,
        }
    }

    fn prepare(&self, cache: Option<&mut LuckDpCache>, cancelled: impl FnMut() -> bool) -> LuckRushPreparation {
        let skills = luck_skills(&self.master).unwrap();
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
        session.rush_cap_preparation(&self.deck, cache, cancelled)
    }

    fn ready(&self, cache: Option<&mut LuckDpCache>) -> LuckTerminalRush {
        match self.prepare(cache, || false) {
            LuckRushPreparation::Ready(capability) => capability,
            result => panic!("synthetic terminal Rush preparation must succeed: {result:?}"),
        }
    }

    fn native(&self) -> LiveModel {
        assert!(self.ranking.is_none());
        LiveModel::new_gekisou(&self.master, &self.deck, &self.notes, &self.events, self.params, &self.setup).unwrap()
    }
}

/// An independent small rational accumulator. Neither the probability DP nor its interval/mass operations
/// supplies the branch weights or the expected values of this oracle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fraction {
    numerator: u128,
    denominator: u128,
}

impl Fraction {
    const ZERO: Self = Self { numerator: 0, denominator: 1 };
    const ONE: Self = Self { numerator: 1, denominator: 1 };

    fn new(numerator: u128, denominator: u128) -> Self {
        assert!(denominator > 0);
        let (mut a, mut b) = (numerator, denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self { numerator: numerator / a, denominator: denominator / a }
    }

    fn times(self, numerator: u128, denominator: u128) -> Self {
        Self::new(self.numerator.checked_mul(numerator).unwrap(), self.denominator.checked_mul(denominator).unwrap())
    }

    fn plus(self, other: Self) -> Self {
        Self::new(
            self.numerator
                .checked_mul(other.denominator)
                .unwrap()
                .checked_add(other.numerator.checked_mul(self.denominator).unwrap())
                .unwrap(),
            self.denominator.checked_mul(other.denominator).unwrap(),
        )
    }

    fn approximate(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    /// Compare to the exact binary value of an upper endpoint, without rounding the rational oracle first.
    fn at_most(self, upper: f64) -> bool {
        assert!(upper.is_finite() && upper >= 0.0);
        if upper == 0.0 {
            return self.numerator == 0;
        }
        let bits = upper.to_bits();
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let mantissa = u128::from((bits & ((1u64 << 52) - 1)) | (1u64 << 52));
        // Every positive test cap is a normal number. The shifts remain checked fixture-size limits.
        assert!(exponent_bits > 0);
        let exponent = exponent_bits - 1023 - 52;
        if exponent >= 0 {
            self.numerator <= self.denominator.checked_mul(mantissa).unwrap().checked_shl(exponent as u32).unwrap()
        } else {
            self.numerator.checked_shl((-exponent) as u32).unwrap() <= self.denominator.checked_mul(mantissa).unwrap()
        }
    }

    /// Compare a lower endpoint with the exact rational branch sum using integer arithmetic.
    fn at_least(self, lower: f64) -> bool {
        assert!(lower.is_finite() && lower >= 0.0);
        if lower == 0.0 {
            return true;
        }
        let bits = lower.to_bits();
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let mantissa = u128::from((bits & ((1u64 << 52) - 1)) | (1u64 << 52));
        assert!(exponent_bits > 0);
        let exponent = exponent_bits - 1023 - 52;
        if exponent >= 0 {
            self.numerator >= self.denominator.checked_mul(mantissa).unwrap().checked_shl(exponent as u32).unwrap()
        } else {
            self.numerator.checked_shl((-exponent) as u32).unwrap() >= self.denominator.checked_mul(mantissa).unwrap()
        }
    }
}

struct NativeBranch {
    mass: Fraction,
    // Native final calculator value and actual range snapshots, observed without score-bound replay.
    total_score: i32,
    final_life: i32,
    rank_bonuses: Vec<(usize, i32, i32, i64)>,
    rank_ranges: Vec<(i32, i32)>,
    // Every terminal note in (chart time, note id) order: (time, id, actual score, native Rush class).
    notes: Vec<(i32, i32, i32, bool)>,
    // Independently observed joint class from the native integer command log, in the same note order.
    buckets: Vec<usize>,
    // The field saved by each note's last native execution, not the final calculator state.
    note_fields: Vec<f32>,
    // Actual frozen (operation type, converted score type, life), in the same note order.
    note_inputs: Vec<(i32, i32, i32)>,
    combo_history_changed: bool,
    final_note_field: f32,
    retained_notes_at_last_query: bool,
    late_tied_ordinary_factor: bool,
    same_owner_ordinary_ties: bool,
    clamped_probe_end: bool,
    depth: usize,
    probe_commands: usize,
    rush_on_and_off_before_one_query: bool,
}

/// Restart the full native interpreter for every selected prefix. Only the native nominal draw site's integer
/// weights decide the next branches. No DP states, score bounds, path checkpoints or exact-law walker are used.
fn native_branches(input: &RushCase) -> Vec<NativeBranch> {
    let skills = luck_skills(&input.master).unwrap();
    let score_rows = input.native().luck_score_rows(&skills);
    let owners: Vec<_> = score_rows.iter().map(|row| row.owner).collect();
    // These fixtures isolate direct probes by owner: ordinary live effects use member owners when the probes
    // use Snap owners; fixtures with member probes omit those live effects. Summing their integer commands
    // identifies the native ideal amplitude without using DP state or assuming float additions cancel exactly.
    assert!(input.deck.iter().all(|performer| performer.support_skills.is_empty()));
    assert!(
        owners.iter().all(|owner| owner % 100 == crate::live::skill::OWNER_SNAP)
            || input.deck.iter().all(|performer| performer.live_skill.is_none())
    );
    let probe_mill: i64 = score_rows.iter().filter(|row| row.may_hold).map(|row| (row.value * 100000f32) as i64).sum();
    let rush = setting(&input.master, "gekisou_luck_rush_score_bonus_percent").unwrap() as i32;
    let mut pending = vec![(Vec::new(), Fraction::ONE)];
    let mut branches = Vec::new();
    let mut visited = 0;
    while let Some((prefix, mass)) = pending.pop() {
        visited += 1;
        assert!(visited <= 4096 && prefix.len() <= 16, "the synthetic branch domain must finish in full");
        let depth = prefix.len();
        let mut native = input.native();
        native.score.begin_bounds(Vec::new(), false);
        let result = native.run_with_random(&input.play, &input.delta, LiveRandom::with_nominal_prefix(prefix.clone()));
        assert!(native.random.nominal_covers_draws());
        if let Some(outcomes) = native.random.nominal_branch() {
            assert!(result.is_err());
            let total = outcomes[0].total;
            assert_eq!(outcomes.iter().map(|outcome| outcome.weight).sum::<u64>(), total);
            for (index, outcome) in outcomes.iter().enumerate() {
                assert_eq!(outcome.total, total);
                let mut next = prefix.clone();
                next.push(index);
                pending.push((next, mass.times(u128::from(outcome.weight), u128::from(total))));
            }
            continue;
        }
        result.unwrap();
        assert!(native.random.nominal_prefix_consumed());
        assert!(native.gk.as_ref().unwrap().ctrl.states.iter().all(|state| state.state == gekisou::S_FINISH));
        let trace = native.score.bounds_trace.take().unwrap();
        let (frames, states) = native.score.executed_states();
        let states: BTreeMap<_, _> = states.into_iter().collect();
        let saved_scores: BTreeMap<_, _> = native.score.executed_note_scores().into_iter().collect();
        assert_eq!(saved_scores.len(), input.notes.len(), "these oracle fixtures use unique note IDs");
        let mut notes = Vec::new();
        let mut buckets = Vec::new();
        let mut note_fields = Vec::new();
        let mut note_inputs = Vec::new();
        let mut combos = BTreeMap::new();
        let mut combo_history_changed = false;
        let mut previous_query = None;
        let mut first_note_frame = None;
        let mut retained_notes_at_last_query = false;
        let mut filed_since_query = false;
        let mut late_tied_ordinary_factor = false;
        let mut ordinary_times = BTreeMap::<(i32, i32), usize>::new();
        let mut clamped_probe_end = false;
        let mut probe_commands = 0;
        let mut rush_signs = BTreeMap::<usize, u8>::new();
        let mut queries = 0;
        for event in &trace.events {
            match event {
                BoundsEvent::Factor { frame, command } => {
                    filed_since_query = true;
                    if command.luck != 0 {
                        *rush_signs.entry(queries).or_default() |= if command.luck > 0 { 1 } else { 2 };
                    }
                    if command.note_mill != 0 && owners.contains(&command.owner_id) {
                        probe_commands += 1;
                        clamped_probe_end |= command.note_mill < 0 && command.time_ms == input.params.music_length_ms;
                    } else if command.note_mill != 0 {
                        *ordinary_times.entry((command.time_ms, command.owner_id)).or_default() += 1;
                        late_tied_ordinary_factor |= previous_query.is_some_and(|to| *frame as i32 <= to)
                            && input.notes.iter().any(|note| note.time_ms == command.time_ms);
                    }
                }
                BoundsEvent::Query { to, .. } => {
                    retained_notes_at_last_query = previous_query.is_some_and(|previous| {
                        !filed_since_query && *to >= previous && first_note_frame.is_some_and(|frame| frame <= previous)
                    });
                    previous_query = Some(*to);
                    filed_since_query = false;
                    queries += 1;
                }
                BoundsEvent::Note { frame, note, .. } => {
                    filed_since_query = true;
                    first_note_frame.get_or_insert(*frame as i32);
                    assert!(*frame < frames - 1, "all oracle notes are in the settled frame prefix");
                    let added_luck: i32 = trace
                        .events
                        .iter()
                        .filter_map(|event| match event {
                            BoundsEvent::Factor { command, .. } if command.time_ms <= note.time_ms => {
                                Some(command.luck)
                            }
                            _ => None,
                        })
                        .sum();
                    assert!(added_luck == 0 || added_luck == rush, "one native Rush handle defines both classes");
                    let added_probe: i64 = trace
                        .events
                        .iter()
                        .filter_map(|event| match event {
                            BoundsEvent::Factor { command, .. }
                                if command.time_ms <= note.time_ms && owners.contains(&command.owner_id) =>
                            {
                                Some(i64::from(command.note_mill))
                            }
                            _ => None,
                        })
                        .sum();
                    assert!(
                        added_probe == 0 || added_probe == probe_mill,
                        "all held direct probes share the same ideal class, including paired end commands"
                    );
                    buckets.push((
                        note.time_ms,
                        note.note_id,
                        usize::from(added_probe != 0) + 2 * usize::from(added_luck != 0),
                    ));
                    let state = states[&note.note_id];
                    note_fields.push((note.time_ms, note.note_id, state[1]));
                    note_inputs.push((note.time_ms, note.note_id, (note.note_type, note.score_type, note.life)));
                    let mut calc = native.score.calc.clone();
                    assert!(calc.luck_weight.is_none());
                    calc.state.combo_score_up = state[0];
                    calc.state.note_score_up = state[1];
                    calc.state.just = state[2];
                    calc.state.perfect = state[3];
                    calc.state.great = state[4];
                    calc.state.good = state[5];
                    calc.state.added_luck_bonus = added_luck;
                    let score = saved_scores[&note.note_id];
                    if input.master.combo_score_bonuses.is_empty() {
                        // With an empty table this second observation independently reconstructs the stored
                        // integer. Nontrivial combo histories use the native stored value directly: the final
                        // controller's combo is not the input of an older retained execution.
                        let reconstructed =
                            calc.note_score(0, note.life, note.time_ms, note.note_type, note.score_type, None).unwrap();
                        assert_eq!(reconstructed, score);
                    }
                    assert!(score >= 0);
                    notes.push((note.time_ms, note.note_id, score, added_luck != 0));
                }
                BoundsEvent::Combo { frame, index, ordinary, gekisou } => {
                    let bits = (ordinary.to_bits(), gekisou.to_bits());
                    combo_history_changed |= combos.insert((*frame, *index), bits).is_some_and(|old| old != bits);
                }
                _ => {}
            }
        }
        notes.sort_unstable_by_key(|&(time, id, _, _)| (time, id));
        buckets.sort_unstable_by_key(|&(time, id, _)| (time, id));
        note_fields.sort_unstable_by_key(|&(time, id, _)| (time, id));
        note_inputs.sort_unstable_by_key(|&(time, id, _)| (time, id));
        assert_eq!(notes.len(), input.notes.len());
        let reconstructed: i64 = notes.iter().map(|note| i64::from(note.2)).sum();
        let (settled, fixed) = native.score.clone().settle(usize::MAX);
        assert_eq!(
            reconstructed,
            settled - fixed,
            "match the actual terminal note contribution, excluding rank bonuses"
        );
        #[cfg(feature = "search-diagnostics")]
        {
            let observed: BTreeMap<_, _> =
                native.score.filed_scores().0.into_iter().map(|(_, id, score, _)| (id, score)).collect();
            for &(_, id, score, _) in &notes {
                assert_eq!(observed[&id], score, "each reconstructed note must match its stored native score");
            }
        }
        branches.push(NativeBranch {
            mass,
            total_score: native.score(),
            final_life: native.current_life(),
            rank_bonuses: native.gekisou_rank_bonuses().to_vec(),
            rank_ranges: native.gekisou_ranges().iter().map(|range| (range.start_score, range.end_score)).collect(),
            notes,
            buckets: buckets.into_iter().map(|(_, _, bucket)| bucket).collect(),
            note_fields: note_fields.into_iter().map(|(_, _, field)| field).collect(),
            note_inputs: note_inputs.into_iter().map(|(_, _, inputs)| inputs).collect(),
            combo_history_changed,
            final_note_field: native.factor_state().note_score_up,
            retained_notes_at_last_query,
            late_tied_ordinary_factor,
            same_owner_ordinary_ties: ordinary_times.values().any(|&count| count > 1),
            clamped_probe_end,
            depth,
            probe_commands,
            rush_on_and_off_before_one_query: rush_signs.values().any(|&signs| signs == 3),
        });
    }
    assert_eq!(branches.iter().fold(Fraction::ZERO, |mass, branch| mass.plus(branch.mass)), Fraction::ONE);
    branches
}

fn observed_caps(branches: &[NativeBranch]) -> Vec<(i32, i64, i64)> {
    let mut maxima = vec![[0i64; 2]; branches[0].notes.len()];
    for branch in branches {
        for (index, &(time, id, score, on)) in branch.notes.iter().enumerate() {
            assert_eq!((time, id), (branches[0].notes[index].0, branches[0].notes[index].1));
            maxima[index][usize::from(on)] = maxima[index][usize::from(on)].max(i64::from(score));
        }
    }
    maxima.into_iter().enumerate().map(|(index, [off, on])| (branches[0].notes[index].0, off, on.max(off))).collect()
}

fn assert_native_mean_is_bounded(branches: &[NativeBranch], caps: &[(i32, i64, i64)], upper: f64) {
    let (mut actual_mean, mut cap_mean) = (Fraction::ZERO, Fraction::ZERO);
    for branch in branches {
        let (mut score, mut cap_sum) = (0u128, 0u128);
        for (&(time, _, native, on), &(cap_time, off_cap, on_cap)) in branch.notes.iter().zip(caps) {
            assert_eq!(time, cap_time);
            let cap = if on { on_cap } else { off_cap };
            assert!(i64::from(native) <= cap);
            score += native as u128;
            cap_sum += cap as u128;
        }
        actual_mean = actual_mean.plus(branch.mass.times(score, 1));
        cap_mean = cap_mean.plus(branch.mass.times(cap_sum, 1));
    }
    assert!(actual_mean.at_most(upper), "native mean {actual_mean:?} exceeds {upper}");
    assert!(cap_mean.at_most(upper), "native Rush cap mean {cap_mean:?} exceeds {upper}");
    assert!(upper - cap_mean.approximate() < 1e-7, "only probability-rounding slack may separate the two cap means");
}

fn observed_bucket_caps(branches: &[NativeBranch]) -> Vec<(i32, [i64; 4])> {
    let mut maxima = vec![[0i64; 4]; branches[0].notes.len()];
    for branch in branches {
        assert_eq!(branch.notes.len(), branch.buckets.len());
        for (index, (&(time, id, score, _), &bucket)) in branch.notes.iter().zip(&branch.buckets).enumerate() {
            assert_eq!((time, id), (branches[0].notes[index].0, branches[0].notes[index].1));
            maxima[index][bucket] = maxima[index][bucket].max(i64::from(score));
        }
    }
    maxima
        .into_iter()
        .enumerate()
        .map(|(index, mut caps)| {
            // Bucket 3 also retains the unconditional cap. No monotonicity of native conversion or rounding
            // gains is assumed by the independent cap oracle.
            caps[3] = *caps.iter().max().unwrap();
            (branches[0].notes[index].0, caps)
        })
        .collect()
}

fn assert_native_bucket_mean_is_bounded(branches: &[NativeBranch], caps: &[(i32, [i64; 4])], upper: f64) {
    let (mut actual_mean, mut cap_mean) = (Fraction::ZERO, Fraction::ZERO);
    for branch in branches {
        let (mut score, mut cap_sum) = (0u128, 0u128);
        for ((&(time, _, native, _), &bucket), &(cap_time, by_bucket)) in
            branch.notes.iter().zip(&branch.buckets).zip(caps)
        {
            assert_eq!(time, cap_time);
            let cap = by_bucket[bucket];
            assert!(i64::from(native) <= cap, "every native branch is covered before taking its expectation");
            score += native as u128;
            cap_sum += cap as u128;
        }
        actual_mean = actual_mean.plus(branch.mass.times(score, 1));
        cap_mean = cap_mean.plus(branch.mass.times(cap_sum, 1));
    }
    assert!(actual_mean.at_most(upper), "native mean {actual_mean:?} exceeds {upper}");
    assert!(cap_mean.at_most(upper), "native joint cap mean {cap_mean:?} exceeds {upper}");
    assert!(upper - cap_mean.approximate() < 1e-7, "only outward probability slack may separate the cap means");
}

fn four_bucket_case(gap: i32, critical_weight: i64, member_probe: bool) -> RushCase {
    let mut input = RushCase::new(gap, gekisou::M_LUCK, critical_weight);
    input.setup.fevers = (0..3).map(|range| (range * gap + 100, range * gap + 360)).collect();
    // Each range has notes in two different native frames. The second note is exactly at its frame time, so it
    // includes that frame's probe command, reflecting the preceding draw, while Rush reflects the new draw.
    // All four joint classes therefore have positive mass at the same terminal note.
    for (note, time) in input.notes.iter_mut().zip([110, 300, gap + 110, gap + 300, 2 * gap + 110, 2 * gap + 300]) {
        note.time_ms = time;
    }
    for frame in &mut input.play.frames {
        frame.judged = input
            .notes
            .iter()
            .filter(|note| (note.time_ms + 99) / 100 == frame.time_ms / 100)
            .map(|note| JudgedNote { note_id: note.note_id, judgement: 5, judgement_time_ms: note.time_ms })
            .collect();
    }
    input.master.gekisou_support_skill_effects.last_mut().unwrap().effect_value = 2500;
    let mut second = input.master.gekisou_support_skill_effects.last().unwrap().clone();
    second.id = 904;
    second.effect_value = 1250;
    input.master.gekisou_support_skill_effects.push(second);
    let mut performer = input.deck[0].clone();
    performer.character_id = 8;
    performer.live_skill = None;
    input.deck.push(performer);
    if member_probe {
        input.events.clear();
        for performer in &mut input.deck {
            performer.live_skill = None;
        }
        let mut row = input.master.gekisou_support_skill_effects[0].clone();
        row.id = 905;
        row.skill_id = 901;
        row.effect_value = 3750;
        input.master.gekisou_skill_effects.push(row);
    }
    input.master.reindex().unwrap();
    input
}

#[test]
fn terminal_joint_caps_match_native_branch_classes_with_multiple_owners_and_sources() {
    for (gap, power, critical_weight, member_probe) in [(2400, 999, 1, false), (12_000, 1001, 2, true)] {
        let mut input = four_bucket_case(gap, critical_weight, member_probe);
        input.params.total_power = power;
        let branches = native_branches(&input);
        assert!(branches.iter().any(|branch| branch.depth >= 6));
        for index in [1, 3, 5] {
            let mut mass = [Fraction::ZERO; 4];
            for branch in &branches {
                let bucket = branch.buckets[index];
                mass[bucket] = mass[bucket].plus(branch.mass);
            }
            assert!(mass.iter().all(|p| p.numerator > 0), "every joint class must be observed at the same note");
            assert_eq!(mass.into_iter().fold(Fraction::ZERO, Fraction::plus), Fraction::ONE);
        }
        let caps = observed_bucket_caps(&branches);
        let collapsed: Vec<_> = caps.iter().map(|&(time, c)| (time, c[0].max(c[1]), c[2].max(c[3]))).collect();
        let mut answers = Vec::new();
        for capacity in [0, 8 << 20] {
            let mut cache = LuckDpCache::new(capacity);
            let terminal = input.ready(Some(&mut cache));
            assert_eq!(terminal.probe_gate(), Some(gekisou::M_LUCK));
            let upper = terminal.weighted_note_bucket_upper(&caps).unwrap();
            assert_native_bucket_mean_is_bounded(&branches, &caps, upper);
            let two_class_upper = terminal.weighted_note_upper(&collapsed).unwrap();
            assert!(upper < two_class_upper, "a positive off-probe mass must actually tighten these caps");
            let equivalent: Vec<_> = collapsed.iter().map(|&(time, off, on)| (time, [off, off, on, on])).collect();
            assert_eq!(
                terminal.weighted_note_bucket_upper(&equivalent).unwrap().to_bits(),
                two_class_upper.to_bits(),
                "the public two-class interface retains its exact accumulation order"
            );
            assert_eq!(cache.stats().program_compilations, 0, "joint preparation does not replay score history");
            answers.push(upper.to_bits());
        }
        assert_eq!(answers[0], answers[1]);
    }
}

#[test]
fn terminal_rush_caps_cover_independent_native_branches_with_gates_and_multiple_ranges() {
    for (gap, gate, critical_weight) in [(2400, 2, 1), (12_000, 2, 2), (2400, 2, 3)] {
        for power in [999, 1001] {
            let mut input = RushCase::new(gap, gate, critical_weight);
            input.params.total_power = power;
            let branches = native_branches(&input);
            assert!(branches.len() >= 4 && branches.iter().any(|branch| branch.depth >= 3));
            assert!(branches.iter().any(|branch| branch.rush_on_and_off_before_one_query));
            assert_eq!(branches.iter().any(|branch| branch.probe_commands > 0), gate == 2);
            let caps = observed_caps(&branches);
            assert!(caps.iter().any(|&(_, off, on)| off < on));
            let mut answers = Vec::new();
            for capacity in [0, 8 << 20] {
                let mut cache = LuckDpCache::new(capacity);
                let upper = input.ready(Some(&mut cache)).weighted_note_upper(&caps).unwrap();
                assert_native_mean_is_bounded(&branches, &caps, upper);
                assert!(upper < caps.iter().map(|cap| cap.2 as f64).sum::<f64>());
                assert_eq!(cache.stats().program_compilations, 0, "preparation never runs factor-history replay");
                answers.push(upper.to_bits());
            }
            assert_eq!(answers[0], answers[1], "curve caching cannot change the upper certificate");
        }
    }
}

#[test]
fn terminal_rush_caps_cover_both_base_point_and_bonus_lotteries() {
    let mut input = RushCase::new(2400, 2, 2);
    let base = input.master.gekisou_luck_base_points[0].clone();
    input.master.gekisou_luck_base_points = [(6, 1), (14, 2)]
        .into_iter()
        .enumerate()
        .map(|(index, (value, weight))| {
            let mut row = base.clone();
            row.id = index as i64 + 1;
            row.base_point = value;
            row.weight = weight;
            row
        })
        .collect();
    input.master.reindex().unwrap();
    let branches = native_branches(&input);
    assert!(branches.iter().any(|branch| branch.depth >= 5));
    let caps = observed_caps(&branches);
    let upper = input.ready(None).weighted_note_upper(&caps).unwrap();
    assert_native_mean_is_bounded(&branches, &caps, upper);
    assert!(upper < caps.iter().map(|cap| cap.2 as f64).sum::<f64>());
}

fn trace_for_terminal(events: Vec<BoundsEvent>, queries: usize) -> BoundsTrace {
    BoundsTrace {
        events,
        queries,
        frames: 16,
        probes: Vec::new(),
        combo: ComboObserver::default(),
        has_luck: true,
        filing_gate: None,
        probe_filings: None,
    }
}

fn constant_curve() -> Arc<LuckDpCertifiedResult> {
    Arc::new(LuckDpCertifiedResult {
        probe_transitions: Vec::new(),
        steps: Vec::new(),
        probes: Vec::new(),
        range_moments: Vec::new(),
        peak_states: 1,
        transitions: 0,
    })
}

#[test]
fn terminal_rush_readiness_must_precede_the_last_query() {
    let note = BoundsEvent::Note { frame: 3, index: 0, note: NoteCommand::new(100, 1000, 1, 1, 3) };
    let query = BoundsEvent::Query { time_ms: 200, to: 5 };
    let ready = BoundsEvent::ProbabilityReady(200);
    let curve = constant_curve();
    let too_late = trace_for_terminal(vec![note.clone(), query.clone(), ready.clone()], 1);
    assert!(matches!(terminal_notes(&too_late, &curve, &mut || false), Err((LuckRushDecline::TerminalQuery, _))));
    let in_time = trace_for_terminal(vec![note.clone(), ready.clone(), query.clone()], 1);
    assert!(terminal_notes(&in_time, &curve, &mut || false).unwrap().is_some());
    let later_query = trace_for_terminal(vec![note.clone(), query.clone(), ready.clone(), query.clone()], 2);
    assert!(terminal_notes(&later_query, &curve, &mut || false).unwrap().is_some());
    let mut partial_ready = in_time.clone();
    partial_ready.events[1] = BoundsEvent::ProbabilityReady(99);
    assert!(matches!(terminal_notes(&partial_ready, &curve, &mut || false), Err((LuckRushDecline::TerminalQuery, _))));
    let mut pending_rank = in_time.clone();
    pending_rank.events.push(BoundsEvent::Rank { range: 0, time_ms: 200, percent: 23, start: None, end: Some(0) });
    assert!(matches!(terminal_notes(&pending_rank, &curve, &mut || false), Err((LuckRushDecline::TerminalQuery, _))));
    pending_rank.events.push(query);
    pending_rank.queries += 1;
    assert!(terminal_notes(&pending_rank, &curve, &mut || false).unwrap().is_some());
    let no_query = trace_for_terminal(vec![note, ready], 0);
    assert!(matches!(terminal_notes(&no_query, &curve, &mut || false), Err((LuckRushDecline::TerminalQuery, _))));
    let mut wrong_count = in_time;
    wrong_count.queries += 1;
    assert!(matches!(terminal_notes(&wrong_count, &curve, &mut || false), Err((LuckRushDecline::TerminalQuery, _))));
    assert!(terminal_notes(&later_query, &curve, &mut || true).unwrap().is_none());
}

#[test]
fn terminal_rush_note_multiset_and_integer_caps_must_match() {
    let mut events = Vec::new();
    for (id, time) in [(1, 100), (2, 100), (3, 200)] {
        events.push(BoundsEvent::Note {
            frame: time as usize / 40 + 1,
            index: 0,
            note: NoteCommand::new(time, 1000, id, 1, 3),
        });
    }
    events.push(BoundsEvent::ProbabilityReady(200));
    events.push(BoundsEvent::Query { time_ms: 200, to: 6 });
    let terminal = terminal_notes(&trace_for_terminal(events, 1), &constant_curve(), &mut || false).unwrap().unwrap();
    let caps = vec![(100, 2, 9), (100, 4, 12), (200, 6, 19)];
    let upper = terminal.weighted_note_upper(&caps).unwrap();
    assert!(Fraction::new(12, 1).at_most(upper) && upper - 12.0 < 1e-10);
    for malformed in [
        vec![(100, 2, 9), (200, 6, 19)],
        vec![(100, 2, 9), (100, 4, 12), (100, 4, 12), (200, 6, 19)],
        vec![(200, 6, 19), (100, 2, 9), (100, 4, 12)],
        vec![(100, 2, 9), (101, 4, 12), (200, 6, 19)],
        vec![(100, -1, 9), (100, 4, 12), (200, 6, 19)],
        vec![(100, 10, 9), (100, 4, 12), (200, 6, 19)],
        vec![(100, 2, i64::from(i32::MAX) + 1), (100, 4, 12), (200, 6, 19)],
    ] {
        assert!(terminal.weighted_note_upper(&malformed).is_none());
    }
}

#[test]
fn terminal_probe_conditioning_needs_a_held_probe_and_matching_recorded_gate() {
    let note = BoundsEvent::Note { frame: 3, index: 0, note: NoteCommand::new(100, 1000, 1, 1, 3) };
    let ready = BoundsEvent::ProbabilityReady(200);
    let query = BoundsEvent::Query { time_ms: 200, to: 5 };
    let curve = constant_curve();
    for gate in [None, Some(None), Some(Some(1)), Some(Some(gekisou::M_LUCK)), Some(Some(3)), Some(Some(4))] {
        for held in [false, true] {
            let mut trace = trace_for_terminal(vec![note.clone(), ready.clone(), query.clone()], 1);
            trace.filing_gate = gate;
            if held {
                trace.probes.push(ProbeRow { owner: crate::live::skill::OWNER_SNAP, value: 0.25 });
            }
            let terminal = terminal_notes(&trace, &curve, &mut || false).unwrap().unwrap();
            let authorized = held && gate == Some(Some(gekisou::M_LUCK));
            assert_eq!(terminal.probe_gate(), authorized.then_some(gekisou::M_LUCK));
            assert_eq!(terminal.weighted_note_bucket_upper(&[(100, [2, 7, 5, 11])]).is_some(), authorized);
            assert_eq!(terminal.weighted_note_bucket_upper(&[(100, [2, 2, 5, 11])]).is_some(), authorized);
            assert_eq!(terminal.weighted_note_bucket_upper(&[(100, [2, 7, 11, 11])]).is_some(), authorized);
            let ordinary = terminal.weighted_note_upper(&[(100, 2, 11)]).unwrap();
            assert_eq!(
                terminal.weighted_note_bucket_upper(&[(100, [2, 2, 11, 11])]).unwrap().to_bits(),
                ordinary.to_bits(),
                "lack of probe authority still permits the existing Rush-only certificate"
            );
            // Probe metadata cannot repair a probability readiness event filed after the terminal query.
            trace.events = vec![note.clone(), query.clone(), ready.clone()];
            assert!(matches!(terminal_notes(&trace, &curve, &mut || false), Err((LuckRushDecline::TerminalQuery, _))));
        }
    }
}

#[test]
fn terminal_joint_caps_keep_the_exact_multiset_and_integer_endpoint_contract() {
    let mut events = Vec::new();
    for (id, time) in [(1, 100), (2, 100), (3, 200)] {
        events.push(BoundsEvent::Note {
            frame: time as usize / 40 + 1,
            index: 0,
            note: NoteCommand::new(time, 1000, id, 1, 3),
        });
    }
    events.push(BoundsEvent::ProbabilityReady(200));
    events.push(BoundsEvent::Query { time_ms: 200, to: 6 });
    let mut trace = trace_for_terminal(events, 1);
    trace.filing_gate = Some(Some(gekisou::M_LUCK));
    trace.probes.push(ProbeRow { owner: crate::live::skill::OWNER_SNAP, value: 0.25 });
    let terminal = terminal_notes(&trace, &constant_curve(), &mut || false).unwrap().unwrap();
    let valid = vec![(100, [2, 5, 7, 11]), (100, [3, 6, 8, 12]), (200, [4, 7, 9, 13])];
    let upper = terminal.weighted_note_bucket_upper(&valid).unwrap();
    assert!(Fraction::new(9, 1).at_most(upper) && upper - 9.0 < 1e-10);
    for replacement in [
        [-1, 5, 7, 11],
        [2, -1, 7, 11],
        [2, 5, -1, 11],
        [2, 5, 7, -1],
        [12, 5, 7, 11],
        [2, 12, 7, 11],
        [2, 5, 12, 11],
        [2, 5, 7, i64::from(i32::MAX) + 1],
    ] {
        let mut malformed = valid.clone();
        malformed[0].1 = replacement;
        assert!(terminal.weighted_note_bucket_upper(&malformed).is_none());
    }
    let mut nonmonotone = valid.clone();
    nonmonotone[0].1 = [8, 3, 6, 11];
    assert!(terminal.weighted_note_bucket_upper(&nonmonotone).is_some());
    let mut wrong_time = valid.clone();
    wrong_time[1].0 += 1;
    assert!(terminal.weighted_note_bucket_upper(&wrong_time).is_none());
    let mut wrong_order = valid.clone();
    wrong_order.swap(0, 2);
    assert!(terminal.weighted_note_bucket_upper(&wrong_order).is_none());
    assert!(terminal.weighted_note_bucket_upper(&valid[..2]).is_none());
    let mut duplicate = valid.clone();
    duplicate.insert(1, valid[0]);
    assert!(terminal.weighted_note_bucket_upper(&duplicate).is_none());
}

#[test]
fn terminal_probe_authority_is_absent_when_every_matching_row_is_statically_unheld() {
    let mut input = RushCase::new(2400, gekisou::M_LUCK, 2);
    input.master.skill_targets.push(crate::master::SkillTargetRow { id: 904, character_id: 999, ..Default::default() });
    input.master.skill_conditions.push(crate::master::SkillConditionRow {
        id: 904,
        condition_type: 5000,
        condition_values: vec![],
        is_positive: true,
        condition_target_ids: vec![904],
    });
    input.master.skill_condition_sets.push(crate::master::SkillConditionSetRow {
        id: 904,
        group: 904,
        condition_ids: vec![904],
    });
    for row in &mut input.master.gekisou_support_skill_effects {
        row.skill_condition_group = 904;
    }
    input.master.reindex().unwrap();
    let skills = luck_skills(&input.master).unwrap();
    let rows = input.native().luck_score_rows(&skills);
    assert!(!rows.is_empty() && rows.iter().all(|row| !row.may_hold));
    for held_rows_absent in [false, true] {
        if held_rows_absent {
            input.deck[0].gekisou_support_skills.clear();
        }
        let terminal = input.ready(None);
        assert_eq!(terminal.probe_gate(), None);
        let branches = native_branches(&input);
        assert!(branches.iter().all(|branch| branch.probe_commands == 0));
        let caps = observed_caps(&branches);
        let upper = terminal.weighted_note_upper(&caps).unwrap();
        assert_native_mean_is_bounded(&branches, &caps, upper);
        let equal: Vec<_> = caps.iter().map(|&(time, off, on)| (time, [off, off, on, on])).collect();
        assert_eq!(terminal.weighted_note_bucket_upper(&equal).unwrap().to_bits(), upper.to_bits());
        let mut conditional = equal;
        // This change is within the old unconditional cap, but its probe bit still lacks authority.
        conditional[0].1[0] = 0;
        conditional[0].1[1] = conditional[0].1[3].max(1);
        conditional[0].1[3] = conditional[0].1[1];
        assert!(terminal.weighted_note_bucket_upper(&conditional).is_none());
    }
}

#[test]
fn terminal_rush_preparation_keeps_cancellation_and_completed_cache_boundaries() {
    let input = RushCase::new(2400, 2, 3);
    let branches = native_branches(&input);
    let caps = observed_caps(&branches);
    let bucket_caps = observed_bucket_caps(&branches);
    let mut checks = 0;
    let mut reference_cache = LuckDpCache::new(8 << 20);
    let LuckRushPreparation::Ready(reference) = input.prepare(Some(&mut reference_cache), || {
        checks += 1;
        false
    }) else {
        panic!("uncancelled preparation must finish")
    };
    let expected = reference.weighted_note_upper(&caps).unwrap().to_bits();
    let expected_buckets = reference.weighted_note_bucket_upper(&bucket_caps).unwrap().to_bits();
    let times: Vec<_> = caps.iter().map(|&(time, _, _)| time).collect();
    let expected_fields = field_bits(reference.note_score_up_upper(&times).unwrap());
    let expected_native =
        reference.native_note_bucket_caps(i64::from(input.params.total_power), &times).unwrap().to_vec();
    let expected_total = reference.native_score_mean_upper(i64::from(input.params.total_power)).unwrap().to_bits();
    assert!(checks > 4);
    for stop in [1, 2, 3, checks / 2, checks] {
        let mut cache = LuckDpCache::new(8 << 20);
        let mut seen = 0;
        let outcome = input.prepare(Some(&mut cache), || {
            seen += 1;
            seen >= stop
        });
        assert!(matches!(outcome, LuckRushPreparation::Stopped), "checkpoint {stop}: {outcome:?}");
        assert_eq!(cache.stats().program_compilations, 0);
        let resumed = input.ready(Some(&mut cache));
        assert_eq!(resumed.weighted_note_upper(&caps).unwrap().to_bits(), expected);
        assert_eq!(
            resumed.native_score_mean_upper(i64::from(input.params.total_power)).unwrap().to_bits(),
            expected_total
        );
        assert_eq!(resumed.weighted_note_bucket_upper(&bucket_caps).unwrap().to_bits(), expected_buckets);
        assert_eq!(field_bits(resumed.note_score_up_upper(&times).unwrap()), expected_fields);
        assert_eq!(
            resumed.native_note_bucket_caps(i64::from(input.params.total_power), &times).unwrap(),
            expected_native
        );
    }
}

fn assert_declined(input: &RushCase, expected: LuckRushDecline) {
    let mut cache = LuckDpCache::new(8 << 20);
    match input.prepare(Some(&mut cache), || false) {
        LuckRushPreparation::Unavailable { reason, .. } => assert_eq!(reason, expected),
        result => panic!("unproved preparation became {result:?}"),
    }
    assert_eq!(cache.stats().program_compilations, 0);
}

#[test]
fn terminal_rush_preparation_declines_unfinished_external_and_unadmitted_inputs() {
    let base = RushCase::new(2400, 2, 3);
    let mut input = base.clone();
    input.ranking = Some(Vec::new());
    assert_declined(&input, LuckRushDecline::ExternalRanking);
    input = base.clone();
    input.setup.missions.fill(1);
    assert_declined(&input, LuckRushDecline::NoLuckRange);
    input = base.clone();
    input.params.assist_factor = -1.0;
    assert_declined(&input, LuckRushDecline::ScoreArithmetic);
    for rush in [-1, i64::from(i32::MAX)] {
        input = base.clone();
        input
            .master
            .live_settings
            .iter_mut()
            .find(|row| row.key == "gekisou_luck_rush_score_bonus_percent")
            .unwrap()
            .value = rush.to_string();
        input.master.reindex().unwrap();
        assert_declined(&input, LuckRushDecline::ScoreArithmetic);
    }
    input = base.clone();
    input.master.live_skill_effects[0].skill_effect_type = 3000;
    input.master.reindex().unwrap();
    assert_declined(&input, LuckRushDecline::RecorderAdmission);
    input = base.clone();
    input.setup.fevers[1] = (150, 260);
    assert_declined(&input, LuckRushDecline::ProbabilityDomain);
    // A direct probe from another mission is outside the reduced DP's admitted dependency closure.
    assert_declined(&RushCase::new(2400, 1, 3), LuckRushDecline::ProbabilityDomain);
    input = base;
    let last_note = input.notes.iter().map(|note| note.time_ms).max().unwrap();
    let last = input.play.frames.iter().position(|frame| frame.time_ms >= last_note).unwrap() + 1;
    input.play.frames.truncate(last);
    input.delta.truncate(last);
    assert_declined(&input, LuckRushDecline::UnfinishedRanges);
}

fn field_bits(fields: &[[f64; 2]]) -> Vec<[u64; 2]> {
    fields.iter().map(|&field| field.map(f64::to_bits)).collect()
}

/// The native interpreter supplies both the terminal saved field and its ideal direct-probe class. The
/// certificate's drift, operation counts, prefix builder and replay implementation supply neither observation.
fn assert_terminal_fields_cover_native_branches(input: &RushCase, conditional: bool) -> Vec<NativeBranch> {
    let branches = native_branches(input);
    let times: Vec<_> = branches[0].notes.iter().map(|note| note.0).collect();
    let mut answers = Vec::new();
    for capacity in [0, 8 << 20] {
        let mut cache = LuckDpCache::new(capacity);
        let terminal = input.ready(Some(&mut cache));
        let fields = terminal.note_score_up_upper(&times).expect("this admitted ordinary history has a field bound");
        assert_eq!(fields.len(), times.len());
        assert!(fields.iter().flatten().all(|field| field.is_finite()));
        if conditional {
            assert!(fields.iter().any(|field| field[0] < field[1]));
        } else {
            assert!(fields.iter().all(|field| field[0].to_bits() == field[1].to_bits()));
        }
        for branch in &branches {
            assert_eq!(branch.note_fields.len(), fields.len());
            for (index, ((&actual, &bucket), upper)) in
                branch.note_fields.iter().zip(&branch.buckets).zip(fields).enumerate()
            {
                assert!(
                    f64::from(actual) <= upper[bucket & 1],
                    "note {index}, native class {bucket}: saved field {actual:?} exceeds {upper:?}"
                );
            }
        }
        let mut wrong = times.clone();
        wrong[0] += 1;
        assert!(terminal.note_score_up_upper(&wrong).is_none());
        assert!(terminal.note_score_up_upper(&times[..times.len() - 1]).is_none());
        wrong = times.clone();
        wrong.insert(0, times[0]);
        assert!(terminal.note_score_up_upper(&wrong).is_none());
        answers.push(field_bits(fields));
        assert_eq!(cache.stats().program_compilations, 0, "the optional prefix proof does not run factor replay");
    }
    assert_eq!(answers[0], answers[1]);
    assert!(branches.iter().all(|branch| branch.retained_notes_at_last_query));
    assert!(
        branches.iter().any(|branch| branch.note_fields.iter().any(|&field| field != branch.final_note_field)),
        "the fixture must distinguish saved note fields from the final calculator state"
    );
    branches
}

#[test]
fn terminal_note_fields_cover_every_native_class_and_retained_execution() {
    for (gap, power, critical_weight, member_probe) in [(2400, 999, 1, false), (12_000, 1001, 2, true)] {
        let mut input = four_bucket_case(gap, critical_weight, member_probe);
        input.params.total_power = power;
        let branches = assert_terminal_fields_cover_native_branches(&input, true);
        assert!(branches.iter().any(|branch| branch.depth >= 6));
        for index in [1, 3, 5] {
            let mut mass = [Fraction::ZERO; 4];
            for branch in &branches {
                mass[branch.buckets[index]] = mass[branch.buckets[index]].plus(branch.mass);
            }
            assert!(mass.iter().all(|mass| mass.numerator > 0));
            assert_eq!(mass.into_iter().fold(Fraction::ZERO, Fraction::plus), Fraction::ONE);
        }
    }
}

#[test]
fn terminal_note_fields_keep_late_tied_ordinary_commands_and_signed_cancellation() {
    let mut input = four_bucket_case(2400, 2, false);
    input.deck[1].live_skill = Some((903, 1));
    // Both owners fire positive and negative ordinary rows at exactly a note's chart time. That score frame
    // was already queried by the preceding play frame; stable owner/filing order and factor-before-note ties
    // remain part of the real interpreter's replay, including the later inverse commands.
    input.events = vec![(0, 110), (1, 110), (0, 2510)];
    let mut down = input.master.live_skill_effects[0].clone();
    down.id = 906;
    down.skill_effect_type = 2005;
    down.effect_value = 1200;
    input.master.live_skill_effects.push(down);
    input.master.reindex().unwrap();
    let branches = assert_terminal_fields_cover_native_branches(&input, true);
    assert!(branches.iter().all(|branch| branch.late_tied_ordinary_factor && branch.same_owner_ordinary_ties));
}

#[test]
fn terminal_note_fields_use_unconditional_probe_bounds_for_finish_clamps_and_negative_rows() {
    let mut clamped = four_bucket_case(2400, 2, false);
    // All judged notes precede this positive music length. The range continues afterward, so possible probe
    // times include raw frame time followed by the earlier finish clamp, giving multiple monotone runs.
    clamped.params.music_length_ms = clamped.notes.iter().map(|note| note.time_ms).max().unwrap() + 1;
    let branches = assert_terminal_fields_cover_native_branches(&clamped, false);
    assert!(branches.iter().any(|branch| branch.clamped_probe_end));

    let mut negative = four_bucket_case(2400, 2, false);
    for row in &mut negative.master.gekisou_support_skill_effects {
        row.effect_value = -row.effect_value;
    }
    negative.master.reindex().unwrap();
    let branches = assert_terminal_fields_cover_native_branches(&negative, false);
    assert!(branches.iter().any(|branch| branch.buckets.iter().any(|bucket| bucket & 1 != 0)));
}

#[test]
fn terminal_note_field_refusals_preserve_the_completed_probability_capability() {
    let mut input = four_bucket_case(2400, 2, false);
    input.master.live_skill_effects[0].skill_effect_type = 2002;
    input.master.reindex().unwrap();
    let branches = native_branches(&input);
    let caps = observed_bucket_caps(&branches);
    let times: Vec<_> = caps.iter().map(|&(time, _)| time).collect();
    let terminal = input.ready(None);
    assert!(terminal.note_score_up_upper(&times).is_none());
    assert!(terminal.native_note_bucket_caps(i64::from(input.params.total_power), &times).is_none());
    assert_eq!(terminal.probe_gate(), Some(gekisou::M_LUCK));
    assert_native_bucket_mean_is_bounded(&branches, &caps, terminal.weighted_note_bucket_upper(&caps).unwrap());

    let note = BoundsEvent::Note { frame: 3, index: 0, note: NoteCommand::new(100, 1000, 1, 1, 3) };
    let mut trace = trace_for_terminal(
        vec![note, BoundsEvent::ProbabilityReady(200), BoundsEvent::Query { time_ms: 200, to: 5 }],
        1,
    );
    trace.filing_gate = Some(None);
    let plain = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    let build = |trace: &BoundsTrace, initial| {
        super::super::terminal_prefix::from_trace(trace, initial, &[100], None, &mut || false)
    };
    let reference = build(&trace, plain).unwrap();
    let mut signed_zero = plain;
    signed_zero[0] = -0.0;
    assert_eq!(field_bits(&build(&trace, signed_zero).unwrap().factors), field_bits(&reference.factors));
    for combo in [-0.25, 0.25] {
        let mut initial = plain;
        initial[0] = combo;
        assert!(matches!(build(&trace, initial), Err(super::super::trace_drift::Decline::Magnitude)));
    }
    for (combo, power) in [(1, 0), (-1, 0), (0, 1)] {
        let mut unsupported = trace.clone();
        unsupported.events.insert(
            0,
            BoundsEvent::Factor {
                frame: 2,
                command: FactorCommand {
                    time_ms: 80,
                    combo_mill: combo,
                    band_total_power: power,
                    ..Default::default()
                },
            },
        );
        assert!(matches!(build(&unsupported, plain), Err(super::super::trace_drift::Decline::Magnitude)));
        let probability = terminal_notes(&unsupported, &constant_curve(), &mut || false).unwrap().unwrap();
        assert!(probability.note_score_up_upper(&[100]).is_none());
        assert!(probability.weighted_note_upper(&[(100, 2, 5)]).is_some());
    }
    let mut unadmitted = trace.clone();
    unadmitted.filing_gate = None;
    assert!(matches!(build(&unadmitted, plain), Err(super::super::trace_drift::Decline::Magnitude)));
    assert!(matches!(
        super::super::terminal_prefix::from_trace(&trace, plain, &[100], None, &mut || true),
        Err(super::super::trace_drift::Decline::Cancelled)
    ));
}

fn assert_native_kernel_caps(input: &RushCase) -> Vec<NativeBranch> {
    let branches = native_branches(input);
    let times: Vec<_> = branches[0].notes.iter().map(|note| note.0).collect();
    let power = i64::from(input.params.total_power);
    let mut answers = Vec::new();
    for capacity in [0, 8 << 20] {
        let mut cache = LuckDpCache::new(capacity);
        let terminal = input.ready(Some(&mut cache));
        let native = terminal.native_note_bucket_caps(power, &times).expect("the full native kernel is certified");
        let caps: Vec<_> = times.iter().zip(native).map(|(&time, &row)| (time, row.map(i64::from))).collect();
        for branch in &branches {
            assert_eq!(branch.note_inputs, branches[0].note_inputs, "conversion and frozen life are deterministic");
            for (index, (&(_, _, actual, _), &bucket)) in branch.notes.iter().zip(&branch.buckets).enumerate() {
                assert!(
                    actual <= native[index][bucket],
                    "stored note {index} in class {bucket}: {actual} > {:?}",
                    native[index]
                );
            }
        }
        assert_native_bucket_mean_is_bounded(&branches, &caps, terminal.weighted_note_bucket_upper(&caps).unwrap());
        // The cap's native power and exact time multiset are part of the capability. Different equal-time
        // identities need no guessed correspondence because every member receives the complete group's max.
        assert!(terminal.native_note_bucket_caps(power + 1, &times).is_none());
        assert!(terminal.native_note_bucket_caps(-1, &times).is_none());
        assert!(terminal.native_note_bucket_caps(i64::MAX, &times).is_none());
        assert!(terminal.native_note_bucket_caps(power, &times[..times.len() - 1]).is_none());
        let mut wrong = times.clone();
        wrong[0] += 1;
        assert!(terminal.native_note_bucket_caps(power, &wrong).is_none());
        wrong = times.clone();
        wrong.insert(0, times[0]);
        assert!(terminal.native_note_bucket_caps(power, &wrong).is_none());
        answers.push(native.to_vec());
        assert_eq!(cache.stats().program_compilations, 0);
    }
    assert_eq!(answers[0], answers[1]);
    branches
}

fn rebuild_rush_frames(input: &mut RushCase, judgement: impl Fn(&LiveNote) -> i32) {
    for frame in &mut input.play.frames {
        frame.judged = input
            .notes
            .iter()
            .filter(|note| (note.time_ms + 99) / 100 == frame.time_ms / 100)
            .map(|note| JudgedNote {
                note_id: note.note_id,
                judgement: judgement(note),
                judgement_time_ms: note.time_ms,
            })
            .collect();
    }
    input.params.converted_note_count = input.notes.len() as i32;
}

#[test]
fn terminal_native_kernels_enclose_saved_scores_on_complete_short_and_long_draw_domains() {
    for (gap, power, weight, member_probe) in [(2400, 999, 1, false), (12_000, 1001, 2, true)] {
        let mut input = four_bucket_case(gap, weight, member_probe);
        input.params.total_power = power;
        let branches = assert_native_kernel_caps(&input);
        assert!(branches.iter().any(|branch| branch.depth >= 6));
        for index in [1, 3, 5] {
            let mut seen = [false; 4];
            for branch in &branches {
                seen[branch.buckets[index]] = true;
            }
            assert!(seen.into_iter().all(|seen| seen));
        }
    }
}

#[test]
fn terminal_native_kernels_preserve_tied_time_types_judgements_life_and_occurrence_order() {
    let mut input = four_bucket_case(2400, 2, false);
    // Input stream order is ID 90 then ID 1, opposite to the native prefix's same-time ID order. The first
    // note is alive and has the larger type percentage; the second freezes zero life after its own damage.
    input.notes[0].note_id = 90;
    input.notes[0].note_operate_type = 2;
    input.notes[0].time_ms = 300;
    input.master.note_parameters.push(crate::master::NoteParameterRow {
        id: 2,
        note_operate_type: 2,
        score_percent: 170,
    });
    input.master.judgement_parameters.push(crate::master::JudgementParameterRow {
        id: 4,
        note_simulate_judgement: 4,
        score_percent: 65,
        damage: 1000,
    });
    let mut great = input.master.gekisou_luck_base_points[0].clone();
    great.id = 4;
    great.note_simulate_judgement = 4;
    input.master.gekisou_luck_base_points.push(great);
    rebuild_rush_frames(&mut input, |note| if note.note_id == 1 { 4 } else { 5 });
    input.master.reindex().unwrap();
    let branches = assert_native_kernel_caps(&input);
    let notes = &branches[0].notes;
    assert_eq!((notes[0].0, notes[0].1, notes[1].0, notes[1].1), (300, 1, 300, 90));
    assert_eq!(branches[0].note_inputs[0], (1, crate::live::score::GREAT, 0));
    assert_eq!(branches[0].note_inputs[1], (2, crate::live::score::PERFECT, 1000));
    assert!(branches.iter().any(|branch| branch.notes[0].2 != branch.notes[1].2));
    let times: Vec<_> = notes.iter().map(|note| note.0).collect();
    let terminal = input.ready(None);
    let caps = terminal.native_note_bucket_caps(i64::from(input.params.total_power), &times).unwrap();
    assert_eq!(caps[0], caps[1], "every equal-time occurrence receives the full componentwise group maximum");
    // Applying the same public cap array in the input's opposite ID order must still cover both native notes.
    for branch in &branches {
        for (cap_index, native_index) in [(0, 1), (1, 0)] {
            assert!(branch.notes[native_index].2 <= caps[cap_index][branch.buckets[native_index]]);
        }
    }
}

#[test]
fn terminal_native_kernels_use_actual_budget_conversions_and_all_historical_combo_observations() {
    let mut converted = four_bucket_case(2400, 2, false);
    converted.master.live_judgement_timings.push(crate::master::LiveJudgementTimingRow {
        id: 6,
        assist_level: 0,
        judgement_priority: 0,
        note_judgement_type: 1,
        note_simulate_judgement: 6,
        before_ms: 0,
        after_ms: 0,
    });
    converted.master.judgement_parameters.push(crate::master::JudgementParameterRow {
        id: 6,
        note_simulate_judgement: 6,
        score_percent: 137,
        damage: 0,
    });
    converted.master.skill_targets.push(crate::master::SkillTargetRow {
        id: 920,
        skill_target_type: 4,
        judgement: 5,
        ..Default::default()
    });
    converted.master.live_skill_effects.push(crate::master::LiveSkillEffectRow {
        id: 920,
        live_skill_id: 903,
        level: 1,
        skill_effect_type: 12006,
        effect_value: 6,
        effect_limit_count: 1,
        skill_target_ids: vec![920],
        activation_time_second: 0.5,
        ..Default::default()
    });
    converted.master.reindex().unwrap();
    let branches = assert_native_kernel_caps(&converted);
    assert_eq!(branches[0].note_inputs[0].1, crate::live::score::JUST);
    assert_eq!(branches[0].note_inputs[1].1, crate::live::score::PERFECT);
    assert!(converted.play.frames.iter().flat_map(|frame| &frame.judged).all(|note| note.judgement == 5));

    let mut combo = four_bucket_case(2400, 2, false);
    combo.setup.missions[0] = 1;
    for row in &mut combo.master.gekisou_ranking_score_bonuses {
        row.mission_pattern = gekisou::mission_pattern(1, 2, 2);
    }
    combo.master.combo_score_bonuses = [(0, 1, 0.125), (0, 2, 0.25), (1, 1, 0.25), (1, 2, 0.375)]
        .into_iter()
        .enumerate()
        .map(|(index, (kind, count, bonus))| crate::master::ComboScoreBonusRow {
            id: index as i64 + 1,
            combo_bonus_type: kind,
            required_combo_count: count,
            bonus_factor: bonus,
        })
        .collect();
    // The 280 and 300 ms notes are both first consumed in frame 300. Its initial score query precedes the
    // controller's consumption of 280; later queries observe a changed Gekisou combo for the 300 ms note.
    combo.notes.insert(1, LiveNote { note_id: 99, note_operate_type: 1, judgement_type: 1, time_ms: 280 });
    rebuild_rush_frames(&mut combo, |_| 5);
    combo.master.reindex().unwrap();
    let branches = assert_native_kernel_caps(&combo);
    assert!(branches.iter().all(|branch| branch.combo_history_changed));
    assert!(branches.iter().any(|branch| branch.depth >= 4));
}

#[test]
fn terminal_native_kernel_refusals_and_fixed_predicate_overapproximation_keep_conservative_bounds() {
    let mut negative_power = four_bucket_case(2400, 2, false);
    negative_power.params.total_power = -1;
    let times: Vec<_> = negative_power.notes.iter().map(|note| note.time_ms).collect();
    let terminal = negative_power.ready(None);
    assert!(terminal.note_score_up_upper(&times).is_some());
    assert!(terminal.native_note_bucket_caps(-1, &times).is_none());
    let ordinary: Vec<_> = times.iter().map(|&time| (time, 10, 20)).collect();
    assert!(terminal.weighted_note_upper(&ordinary).is_some());

    let input = four_bucket_case(2400, 2, false);
    let mut model = input.native();
    // Admission allows fixed boolean trees, while may_hold deliberately overapproximates this false tree.
    // A retained row therefore grants an upper amplitude; it cannot grant a positive native lower amplitude.
    let mut predicate = Checker::Not(Box::new(Checker::And {
        items: vec![Checker::Fixed(true), Checker::Fixed(true)],
        resettable: vec![false, false],
    }));
    assert!(fixed_predicate(&predicate) && predicate.may_hold());
    {
        let mut context = CheckCtx {
            life: &mut model.life,
            random: &mut model.random,
            frame_time: 0,
            current_combo: 0,
            judged: &[],
            events: &[],
            gk: None,
            prev_confirmed_rank: None,
        };
        assert!(!predicate.check(&mut context).unwrap().0);
    }
    let note = NoteCommand::new(100, 1000, 1, 1, crate::live::score::PERFECT);
    let mut trace = trace_for_terminal(
        vec![
            BoundsEvent::Probe { frame: 2, time_ms: 80 },
            BoundsEvent::Note { frame: 3, index: 0, note },
            BoundsEvent::Combo { frame: 3, index: 0, ordinary: 1.0, gekisou: 1.0 },
            BoundsEvent::ProbabilityReady(200),
            BoundsEvent::Query { time_ms: 200, to: 5 },
        ],
        1,
    );
    trace.filing_gate = Some(Some(gekisou::M_LUCK));
    trace.probes.push(ProbeRow { owner: crate::live::skill::OWNER_SNAP, value: 0.25 });
    let initial = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    let prefix =
        super::super::terminal_prefix::from_trace(&trace, initial, &[100], Some(gekisou::M_LUCK), &mut || false)
            .unwrap();
    assert!(prefix.linked);
    let classes = prefix.ingredients.fields(0, prefix.linked).unwrap();
    assert!(classes[1].unwrap()[1].contains(1.0), "an overapproximated but unheld row leaves native field one");
    let kernel = |trace: &BoundsTrace| {
        super::super::terminal_kernel::build(
            &model.score.calc,
            input.params.total_power,
            47,
            trace,
            &prefix.ingredients,
            prefix.linked,
            &[100],
            constant_curve().as_ref(),
            || false,
        )
    };
    let caps = kernel(&trace).unwrap();
    let actual = model.score.calc.note_score(0, 1000, 100, 1, crate::live::score::PERFECT, None).unwrap();
    assert!(actual <= caps.caps[0][1]);
    let mut missing_combo = trace.clone();
    missing_combo.events[2] = BoundsEvent::ProbabilityReady(100);
    assert!(matches!(kernel(&missing_combo), Err(super::super::trace_drift::Decline::Incomplete)));
    let mut nonfinite_combo = trace.clone();
    if let BoundsEvent::Combo { ordinary, .. } = &mut nonfinite_combo.events[2] {
        *ordinary = f32::NAN;
    }
    assert!(matches!(kernel(&nonfinite_combo), Err(super::super::trace_drift::Decline::Magnitude)));
    assert!(matches!(
        super::super::terminal_kernel::build(
            &model.score.calc,
            input.params.total_power,
            47,
            &trace,
            &prefix.ingredients,
            prefix.linked,
            &[100],
            constant_curve().as_ref(),
            || true,
        ),
        Err(super::super::trace_drift::Decline::Cancelled)
    ));
}

fn assert_native_total_mean(input: &RushCase) -> Vec<NativeBranch> {
    let branches = native_branches(input);
    let mean = branches.iter().fold(Fraction::ZERO, |mean, branch| {
        assert!(branch.total_score >= 0);
        assert_eq!(branch.rank_bonuses.len(), input.setup.fevers.len());
        for &(range, _, bonus, percent) in &branch.rank_bonuses {
            let (start, end) = branch.rank_ranges[range];
            let score = end.wrapping_sub(start);
            assert!(score >= 0 && bonus >= 0 && percent >= 0);
            assert_eq!(i128::from(bonus), i128::from(score) * i128::from(percent) / 100);
        }
        mean.plus(branch.mass.times(branch.total_score as u128, 1))
    });
    let power = i64::from(input.params.total_power);
    let mut answers = Vec::new();
    for capacity in [0, 8 << 20] {
        let mut cache = LuckDpCache::new(capacity);
        let capability = input.ready(Some(&mut cache));
        let upper = capability.native_score_mean_upper(power).expect("every native rank window is certified");
        assert!(mean.at_most(upper), "complete native mean {mean:?} exceeds whole-score cap {upper}");
        let summary = capability.terminal_summary(power).expect("both endpoints of the complete score are certified");
        assert!(mean.at_least(summary.final_mean.lower), "terminal lower exceeds native mean {mean:?}: {summary:?}");
        assert!(mean.at_most(summary.final_mean.upper), "terminal upper misses native mean {mean:?}: {summary:?}");
        assert_eq!(summary.final_mean.upper.to_bits(), upper.to_bits());
        for branch in &branches {
            assert!(
                summary.final_support.lower <= branch.total_score && branch.total_score <= summary.final_support.upper
            );
            assert_eq!(summary.exact_final_life, Some(branch.final_life));
        }
        assert!(capability.terminal_summary(power + 1).is_none());
        assert!(capability.native_score_mean_upper(power + 1).is_none());
        assert!(capability.native_score_mean_upper(-1).is_none());
        assert!(capability.native_score_mean_upper(i64::MAX).is_none());
        assert_eq!(cache.stats().program_compilations, 0);
        answers.push(upper.to_bits());
    }
    assert_eq!(answers[0], answers[1]);
    branches
}

#[test]
fn native_total_means_enclose_all_short_and_long_nominal_rank_branches() {
    for (gap, power, weight, member_probe, percent) in [(2400, 997, 1, false, 250), (12_000, 1003, 2, true, 333)] {
        let mut input = four_bucket_case(gap, weight, member_probe);
        input.params.total_power = power;
        for row in &mut input.master.gekisou_ranking_score_bonuses {
            row.score_bonus_percent = percent;
        }
        input.master.reindex().unwrap();
        let branches = assert_native_total_mean(&input);
        assert!(branches.iter().any(|branch| branch.depth >= 6));
        assert!(branches.iter().all(|branch| branch.rank_bonuses.iter().all(|bonus| bonus.3 == percent)));
        assert!(branches.iter().any(|branch| branch.total_score != branches[0].total_score));
        assert!(branches.iter().any(|branch| branch.rank_bonuses.iter().any(|bonus| bonus.2 > 0)));
        for index in [1, 3, 5] {
            let mut seen = [false; 4];
            for branch in &branches {
                seen[branch.buckets[index]] = true;
            }
            assert!(seen.into_iter().all(|seen| seen));
        }
    }
}

#[test]
fn native_total_means_keep_late_signed_tied_filings_and_retained_native_notes() {
    let mut input = four_bucket_case(2400, 2, false);
    input.deck[1].live_skill = Some((903, 1));
    input.events = vec![(0, 110), (1, 110), (0, 2510)];
    let mut down = input.master.live_skill_effects[0].clone();
    down.id = 906;
    down.skill_effect_type = 2005;
    down.effect_value = 1200;
    input.master.live_skill_effects.push(down);
    for row in &mut input.master.gekisou_ranking_score_bonuses {
        row.score_bonus_percent = 333;
    }
    input.master.reindex().unwrap();
    let branches = assert_native_total_mean(&input);
    assert!(branches.iter().all(|branch| branch.late_tied_ordinary_factor && branch.same_owner_ordinary_ties));
    assert!(branches.iter().any(|branch| branch.retained_notes_at_last_query));
}

#[test]
fn native_total_refuses_final_signed_wrap_even_when_notes_and_each_rank_fit() {
    let mut input = four_bucket_case(2400, 2, false);
    input.params.total_power = 100_000_000;
    // This boundary isolates the final fixed-rank sum: every note and every individual rank has a small
    // independent support, while three large rank bonuses can make the complete native sum wrap.
    input.events.clear();
    for performer in &mut input.deck {
        performer.live_skill = None;
        performer.gekisou_support_skills.clear();
    }
    for row in &mut input.master.gekisou_ranking_score_bonuses {
        row.score_bonus_percent = 1000;
    }
    input.master.reindex().unwrap();
    let branches = native_branches(&input);
    let mut wrapped = false;
    for branch in &branches {
        let notes: i64 = branch.notes.iter().map(|note| i64::from(note.2)).sum();
        assert!((0..=i64::from(i32::MAX)).contains(&notes));
        let mut mathematical = notes;
        for &(range, _, bonus, percent) in &branch.rank_bonuses {
            let (start, end) = branch.rank_ranges[range];
            let range_score = end.wrapping_sub(start);
            assert!(range_score >= 0 && bonus >= 0);
            let exact_bonus = i128::from(range_score) * i128::from(percent) / 100;
            assert_eq!(exact_bonus, i128::from(bonus));
            mathematical += i64::from(bonus);
        }
        // These separated native ranges each retain exactly one filed bonus. The final signed cast is the
        // observed wrapping sum, even though every note, range and individual bonus is a positive i32.
        assert_eq!(branch.total_score, mathematical as i32);
        wrapped |= mathematical > i64::from(i32::MAX) && branch.total_score < 0;
    }
    assert!(wrapped, "the native interpreter must actually exercise final signed wrap");
    let times: Vec<_> = branches[0].notes.iter().map(|note| note.0).collect();
    for capacity in [0, 8 << 20] {
        let mut cache = LuckDpCache::new(capacity);
        let capability = input.ready(Some(&mut cache));
        let caps = capability.native_note_bucket_caps(i64::from(input.params.total_power), &times).unwrap();
        let note_support: i64 = caps.iter().map(|caps| i64::from(*caps.iter().max().unwrap())).sum();
        assert!(note_support <= i64::from(i32::MAX), "the refusal must include the rank contribution");
        assert!(capability.native_score_mean_upper(i64::from(input.params.total_power)).is_none());
        let caps: Vec<_> = times.iter().zip(caps).map(|(&time, &caps)| (time, caps.map(i64::from))).collect();
        assert!(capability.weighted_note_bucket_upper(&caps).is_some());
    }
}

#[derive(Clone, Copy, Debug)]
enum NativeRankCase {
    Ordinary,
    SameFrame,
    LateReduction,
    CoefficientZero,
    CoefficientTwo,
    OutsideFixedFrame,
    OverwrittenPending,
    ChangedSharedCoefficient,
    LateReadiness,
    NegativePercent,
    UnfiledPending,
}

struct NativeRankRecording {
    input: RushCase,
    trace: BoundsTrace,
    total: i32,
    notes: i64,
    bonuses: Vec<i32>,
}

fn native_rank_query(model: &mut LiveModel, time_ms: i32) -> (i32, usize) {
    let value = model.score.calculate(time_ms, &model.combo, None).unwrap();
    (value, model.score.bounds_last_query().unwrap())
}

fn native_rank_filing(
    model: &mut LiveModel,
    range: usize,
    time_ms: i32,
    start: (i32, usize),
    end: (i32, usize),
    percent: i64,
) -> i32 {
    let bonus = (i128::from(end.0.wrapping_sub(start.0)) * i128::from(percent) / 100) as i32;
    model.score.add_fixed(time_ms, bonus);
    model.score.bounds_rank(range, time_ms, percent, Some(start.1), Some(end.1));
    bonus
}

/// Deterministic native calculator programs exercise query algebra separately from the lottery branch oracle.
/// They use the real execute/undo/pending-fixed operations and a constant, zero-draw probability law. No proposed
/// rank plan, cap, ideal-prefix builder or interval operation supplies their observed integer score.
fn native_rank_recording(case: NativeRankCase) -> NativeRankRecording {
    let input = four_bucket_case(2400, 2, false);
    let mut model = input.native();
    model.score.begin_bounds(Vec::new(), true);
    model.score.certify_bounds_filings(None);
    model.score.bounds_probability_ready(if matches!(case, NativeRankCase::LateReadiness) { 99 } else { 1000 });
    for (id, time) in [(1, 100), (2, 300), (3, 340)] {
        model.score.add_note(NoteCommand::new(time, 1000, id, 1, crate::live::score::PERFECT));
    }
    native_rank_query(&mut model, 400);
    // Two late, same-owner ordinary commands force the next timestamp query to replay the old frames. A later
    // negative filing below additionally makes the terminal ordinary prefix smaller than the historical rank.
    for note_mill in [120_000, -20_000] {
        model.score.add_factor(FactorCommand { time_ms: 100, owner_id: 7, note_mill, ..Default::default() });
    }
    let (start_ms, end_ms) = if matches!(case, NativeRankCase::SameFrame) { (281, 319) } else { (80, 120) };
    let start = native_rank_query(&mut model, start_ms);
    let end = native_rank_query(&mut model, end_ms);
    let percent = if matches!(case, NativeRankCase::NegativePercent) { -1 } else { 333 };
    let fixed_time = match case {
        NativeRankCase::CoefficientTwo | NativeRankCase::ChangedSharedCoefficient => 200,
        NativeRankCase::OutsideFixedFrame => i32::MAX,
        _ => end_ms,
    };
    let mut bonuses = vec![native_rank_filing(&mut model, 0, fixed_time, start, end, percent)];
    if matches!(case, NativeRankCase::SameFrame) {
        assert_eq!(end.0.wrapping_sub(start.0), 0, "the two chart times address the same native score frame");
    }
    if matches!(case, NativeRankCase::OverwrittenPending) {
        bonuses.push(native_rank_filing(&mut model, 1, 160, start, end, 75));
    }
    if matches!(case, NativeRankCase::LateReadiness) {
        model.score.bounds_probability_ready(1000);
    }
    if matches!(case, NativeRankCase::LateReduction) {
        model.score.add_factor(FactorCommand { time_ms: 100, owner_id: 7, note_mill: -90_000, ..Default::default() });
    }
    if matches!(case, NativeRankCase::CoefficientTwo | NativeRankCase::ChangedSharedCoefficient) {
        let a = native_rank_query(&mut model, 80);
        if matches!(case, NativeRankCase::ChangedSharedCoefficient) {
            let b = native_rank_query(&mut model, 240);
            bonuses.push(native_rank_filing(&mut model, 1, 280, a, b, 333));
        }
    }
    if !matches!(case, NativeRankCase::UnfiledPending) {
        native_rank_query(&mut model, 400);
    }
    if matches!(case, NativeRankCase::CoefficientZero) {
        native_rank_query(&mut model, 80);
    }
    let total = model.score();
    let notes = model.score.executed_note_scores().iter().map(|(_, score)| i64::from(*score)).sum();
    let trace = model.score.bounds_trace.take().unwrap();
    NativeRankRecording { input, trace, total, notes, bonuses }
}

fn kernel_for_native_rank(
    recording: &NativeRankRecording,
    cancelled: impl FnMut() -> bool,
) -> Result<super::super::terminal_kernel::Prepared, super::super::trace_drift::Decline> {
    let trace = &recording.trace;
    let to = trace
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            BoundsEvent::Query { to, .. } => Some(*to),
            _ => None,
        })
        .unwrap();
    let mut times: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            BoundsEvent::Note { frame, note, .. } if *frame as i32 <= to => Some(note.time_ms),
            _ => None,
        })
        .collect();
    times.sort_unstable();
    let prefix =
        super::super::terminal_prefix::from_trace(trace, [0.0, 1.0, 0.0, 0.0, 0.0, 0.0], &times, None, &mut || false)
            .unwrap();
    let model = recording.input.native();
    super::super::terminal_kernel::build(
        &model.score.calc,
        recording.input.params.total_power,
        47,
        trace,
        &prefix.ingredients,
        false,
        &times,
        constant_curve().as_ref(),
        cancelled,
    )
}

#[test]
fn native_total_historical_queries_preserve_real_fixed_coefficients_and_frame_boundaries() {
    for case in [
        NativeRankCase::Ordinary,
        NativeRankCase::SameFrame,
        NativeRankCase::LateReduction,
        NativeRankCase::CoefficientZero,
        NativeRankCase::CoefficientTwo,
        NativeRankCase::OutsideFixedFrame,
        NativeRankCase::OverwrittenPending,
    ] {
        let recording = native_rank_recording(case);
        let bonus = match case {
            NativeRankCase::CoefficientZero => 0,
            NativeRankCase::CoefficientTwo => 2 * i64::from(recording.bonuses[0]),
            NativeRankCase::OverwrittenPending => i64::from(recording.bonuses[1]),
            _ => i64::from(recording.bonuses[0]),
        };
        assert_eq!(i64::from(recording.total) - recording.notes, bonus, "native fixed coefficient in {case:?}");
        let kernel = kernel_for_native_rank(&recording, || false).unwrap();
        let upper = kernel.mean_upper.unwrap();
        assert!(Fraction::new(recording.total as u128, 1).at_most(upper), "{case:?}: {} > {upper}", recording.total);
        let enclosure = kernel.expectation.unwrap();
        assert!(
            enclosure.mean.contains(f64::from(recording.total)),
            "historical lower endpoint in {case:?}: {enclosure:?}"
        );
        assert!(enclosure.support.contains(recording.total));
    }
}

#[test]
fn native_total_declines_unproved_ranks_without_losing_note_caps_or_hiding_cancellation() {
    for case in [
        NativeRankCase::ChangedSharedCoefficient,
        NativeRankCase::LateReadiness,
        NativeRankCase::NegativePercent,
        NativeRankCase::UnfiledPending,
    ] {
        let recording = native_rank_recording(case);
        let result = kernel_for_native_rank(&recording, || false).unwrap();
        assert!(result.mean_upper.is_none(), "unproved rank became a whole-score certificate: {case:?}");
        assert!(result.expectation.is_none(), "unproved rank became a completed expectation: {case:?}");
        assert!(!result.caps.is_empty());
    }
    let recording = native_rank_recording(NativeRankCase::Ordinary);
    let mut checks = 0;
    let reference = kernel_for_native_rank(&recording, || {
        checks += 1;
        false
    })
    .unwrap();
    assert!(checks > 3 && reference.mean_upper.is_some());
    for stop in [1, checks / 2, checks] {
        let mut seen = 0;
        let result = kernel_for_native_rank(&recording, || {
            seen += 1;
            seen >= stop
        });
        assert!(matches!(result, Err(super::super::trace_drift::Decline::Cancelled)));
        let resumed = kernel_for_native_rank(&recording, || false).unwrap();
        assert_eq!(resumed.caps, reference.caps);
        assert_eq!(resumed.mean_upper.map(f64::to_bits), reference.mean_upper.map(f64::to_bits));
    }
}

#[test]
fn packed_terminal_trace_preserves_original_native_rank_kernels_and_refusals() {
    for case in [
        NativeRankCase::Ordinary,
        NativeRankCase::SameFrame,
        NativeRankCase::LateReduction,
        NativeRankCase::CoefficientZero,
        NativeRankCase::CoefficientTwo,
        NativeRankCase::OutsideFixedFrame,
        NativeRankCase::OverwrittenPending,
        NativeRankCase::ChangedSharedCoefficient,
        NativeRankCase::LateReadiness,
        NativeRankCase::NegativePercent,
        NativeRankCase::UnfiledPending,
    ] {
        // The reference's commands and score come from the original native calculator, before the codec is
        // called. Both the successful proof and every structural refusal must survive the storage change.
        let mut recording = native_rank_recording(case);
        let expected = kernel_for_native_rank(&recording, || false).unwrap();
        let expected_plan = super::super::rank_trace::compile(&recording.trace, || false);
        recording.trace = super::super::prepass::test_packed_trace(&recording.trace);
        let actual = kernel_for_native_rank(&recording, || false).unwrap();
        assert_eq!(super::super::rank_trace::compile(&recording.trace, || false), expected_plan, "{case:?}");
        assert_eq!(actual.power, expected.power, "{case:?}");
        assert_eq!(actual.caps, expected.caps, "{case:?}");
        assert_eq!(actual.mean_upper.map(f64::to_bits), expected.mean_upper.map(f64::to_bits), "{case:?}");
        let bits = |value: Option<super::super::native_total::ScoreEnclosure>| {
            value.map(|value| {
                (
                    value.mean.lower().to_bits(),
                    value.mean.upper().to_bits(),
                    value.support.lower(),
                    value.support.upper(),
                )
            })
        };
        assert_eq!(bits(actual.expectation), bits(expected.expectation), "{case:?}");
        if let Some(enclosure) = actual.expectation {
            assert!(enclosure.mean.contains(f64::from(recording.total)), "native mean in {case:?}");
            assert!(enclosure.support.contains(recording.total), "native score in {case:?}");
        }
    }
}
