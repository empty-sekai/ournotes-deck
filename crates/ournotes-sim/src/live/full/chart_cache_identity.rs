//! Source-versioned identities for the complete compiled programs used by chart statistics.
//!
//! A master/table digest is deliberately absent. The native constructors resolve targets, checker
//! topology, cumulative counters, phases, score constants and lottery tables before lookup. The
//! complete derived Debug representation keeps newly added model fields in the identity. Finite
//! Rust float Debug is round-tripping (including signed zero); nonfinite and opaque states decline
//! caching. The caller additionally namespaces these fingerprints by cache schema and simulator
//! source identity; this is not a stable serialization or a cross-engine equivalence claim.
//!
//! Raw row/skill identifiers have two observable roles: equality of state handles and execution
//! order. A private snapshot relabels handles bijectively, retaining aliases and the original order,
//! including the wrapped live-pool execution keys. The actual simulation is never relabeled.

use std::collections::BTreeMap;
use std::fmt::{self, Write};

use sha2::{Digest, Sha256};

use super::{GekisouSetup, LiveModel, LiveNote, LiveParams, LivePlay, LuckScoreShape, LuckSource, Performer};
use crate::{Error, master::Master, replay::RankConfirmation};

const IDENTITY_FORMAT: &str = "ournotes-deck.compiled-chart-program/1";

/// Hash Debug incrementally: even long charts' initialized frame storage does not allocate a huge
/// intermediate string or silently exceed a short-state cache limit.
struct IdentityWriter {
    hash: Sha256,
    tail: Vec<u8>,
}

impl IdentityWriter {
    fn new() -> Self {
        Self { hash: Sha256::new(), tail: Vec::new() }
    }

    fn finish(self) -> Vec<u8> {
        self.hash.finalize().to_vec()
    }
}

impl Write for IdentityWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        // Tokens can straddle formatter writes. Refuse scripted checkers because their Rc alias
        // topology is not represented by Debug; native chart constructors never create them.
        const OPAQUE: [&[u8]; 4] = [b"NaN", b"inf", b"<borrowed>", b"Scripted"];
        let bytes = value.as_bytes();
        let mut boundary = self.tail.clone();
        boundary.extend_from_slice(&bytes[..bytes.len().min(9)]);
        if OPAQUE.iter().any(|token| {
            bytes.windows(token.len()).any(|part| part == *token)
                || boundary.windows(token.len()).any(|part| part == *token)
        }) {
            return Err(fmt::Error);
        }
        self.hash.update(bytes);
        if bytes.len() >= 9 {
            self.tail.clear();
            self.tail.extend_from_slice(&bytes[bytes.len() - 9..]);
        } else {
            self.tail.extend_from_slice(bytes);
            if self.tail.len() > 9 {
                self.tail.drain(..self.tail.len() - 9);
            }
        }
        Ok(())
    }
}

fn label(labels: &mut BTreeMap<i64, i64>, value: i64) -> i64 {
    let next = labels.len() as i64;
    *labels.entry(value).or_insert(next)
}

fn write_schedule(hash: &mut Sha256, play: &LivePlay, delta_times: Option<&[f32]>) {
    // Keep integer and float bits in a length-prefixed schedule. Exhaustive patterns make an added
    // play/frame/judgement field a compile error rather than a silently omitted cache dependency.
    let LivePlay { frames, base_seed } = play;
    hash.update(base_seed.to_le_bytes());
    hash.update((frames.len() as u64).to_le_bytes());
    for frame in frames {
        let super::PlayFrame { time_ms, judged } = frame;
        hash.update(time_ms.to_le_bytes());
        hash.update((judged.len() as u64).to_le_bytes());
        for note in judged {
            let super::JudgedNote { note_id, judgement, judgement_time_ms } = note;
            hash.update(note_id.to_le_bytes());
            hash.update(judgement.to_le_bytes());
            hash.update(judgement_time_ms.to_le_bytes());
        }
    }
    match delta_times {
        None => hash.update([0]),
        Some(values) => {
            hash.update([1]);
            hash.update((values.len() as u64).to_le_bytes());
            for value in values {
                hash.update(value.to_bits().to_le_bytes());
            }
        }
    }
}

/// Opaque identity of a freshly constructed direct run. `None` means to run independently. The
/// seed is part of `play`; `None` delta times means `LiveModel::run`'s zero-delta frame schedule.
/// Queued ranks and all constructor-resolved state are already held by `model`.
pub(crate) fn initialized_chart_program_identity(
    model: &mut LiveModel,
    play: &LivePlay,
    delta_times: Option<&[f32]>,
) -> Option<Vec<u8>> {
    initialized_identity(model, play, delta_times, &())
}

fn initialized_identity(
    model: &LiveModel,
    play: &LivePlay,
    delta_times: Option<&[f32]>,
    context: &impl fmt::Debug,
) -> Option<Vec<u8>> {
    if model.program_has_started
        || model.raw_runtime.is_some()
        || !model.enabled_live.is_empty()
        || !model.guards.is_empty()
        || !model.life_reductions.is_empty()
        || !model.life_limits.is_empty()
        || !model.gk_appliers.ids.is_empty()
        || !model.gk_appliers.exhausted.is_empty()
        || !model.gk_appliers.limit_finished.is_empty()
        || !model.luck_suppressed.is_empty()
        || model.score.calc.luck_weight.is_some()
        || delta_times.is_some_and(|dt| dt.len() != play.frames.len() || dt.iter().any(|dt| !dt.is_finite()))
    {
        return None;
    }
    let mut snapshot = model.clone();
    let empty_score_frames = snapshot.score.compact_cache_snapshot()?;

    // Preserve the total preorder used by enabled_live.sort_by_key, including wrap-around and
    // equal sort keys. Sorting just the raw skill ids would lose this observable native behavior.
    let execution_keys: Vec<_> =
        snapshot.live.iter().map(|skill| skill.key.wrapping_mul(10).wrapping_add(skill.index as i64)).collect();
    let mut distinct = execution_keys.clone();
    distinct.sort_unstable();
    distinct.dedup();
    let execution_order: Vec<_> =
        execution_keys.iter().map(|key| distinct.binary_search(key).expect("key was collected")).collect();
    let mut row_labels = BTreeMap::new();
    for row in &mut snapshot.rows {
        row.id = label(&mut row_labels, row.id);
    }
    let mut live_labels = BTreeMap::new();
    for skill in &mut snapshot.live {
        skill.key = label(&mut live_labels, skill.key);
    }
    // Native pool order is retained, along with equality between each pool and its instances.
    for pool in &mut snapshot.live_pools {
        pool.key = label(&mut live_labels, pool.key);
    }
    let mut condition_labels = BTreeMap::new();
    for skill in &mut snapshot.cond {
        skill.updater.canonicalize_cache_ids(&mut |id| label(&mut condition_labels, id));
    }

    // These are the only randomized lookup maps in a fresh judged-stream model. Sort their full
    // contents for the key, keeping the caller's original storage and execution untouched.
    let notes = std::mem::take(&mut snapshot.score.calc.note_factor_percent);
    let judgements = std::mem::take(&mut snapshot.score.calc.judgement_score_factor_percent);
    let ordered_notes: BTreeMap<_, _> = notes.into_iter().collect();
    let ordered_judgements: BTreeMap<_, _> = judgements.into_iter().collect();
    let mut out = IdentityWriter::new();
    write!(
        out,
        "{IDENTITY_FORMAT}/{:?}",
        (snapshot, empty_score_frames, ordered_notes, ordered_judgements, execution_order, context)
    )
    .ok()?;
    write_schedule(&mut out.hash, play, delta_times);
    Some(out.finish())
}

/// The extra master-dependent classification used by the nominal expectation/DP proof. The native
/// model intentionally omits ignored reset groups; `luck_skills` still classifies those raw groups.
/// Also retain DP's first raw-row mission lookup when a source contains repeated row identifiers.
/// Global shape numbers and probe representatives are replaced by the selected shape's contents.
type LotteryProfile<'a> = (Vec<(usize, LuckSource, Option<&'a LuckScoreShape>, Option<i64>)>, bool);

/// Compile a chart's nominal expectation program before a cache lookup. This validates the complete
/// lottery catalog on every request, then retains only the held rows' resolved classification in the
/// key. Direct replay/off callers should use [`initialized_chart_program_identity`] on the model
/// they will execute, without invoking nominal-law admission for a seeded run.
#[allow(clippy::too_many_arguments)]
pub(crate) fn chart_program_identity(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: Option<&GekisouSetup>,
    play: &LivePlay,
    delta_times: Option<&[f32]>,
    ranking: Option<&[RankConfirmation]>,
) -> Result<Option<Vec<u8>>, Error> {
    if delta_times.is_some_and(|dt| dt.len() != play.frames.len()) {
        return Err(Error::Input("one delta time per frame".into()));
    }
    let mut model = match (setup, ranking) {
        (None, None) => LiveModel::new(master, deck, notes, events, params)?,
        (None, Some(_)) => return Err(Error::Input("ranks without Gekisou".into())),
        (Some(setup), None) => LiveModel::new_gekisou(master, deck, notes, events, params, setup)?,
        (Some(setup), Some(ranking)) => {
            let mut model = LiveModel::new_gekisou_ranked(master, deck, notes, events, params, setup)?;
            model.set_rank_confirmation_timeline(ranking)?;
            model
        }
    };
    let Some(setup) = setup else {
        return Ok(initialized_chart_program_identity(&mut model, play, delta_times));
    };
    let skills = super::luck_skills(master)?;
    let mut rows = Vec::new();
    for condition in &model.cond {
        let source = match condition.skill_type {
            super::SKILL_TYPE_GEKISOU => LuckSource::Gekisou,
            super::SKILL_TYPE_GEKISOU_SUPPORT => LuckSource::GekisouSupport,
            _ => continue,
        };
        for effect in condition.updater.effects() {
            let row = &model.rows[effect.row];
            let shape = skills.rows.get(&(source, row.id)).map(|&index| &skills.shapes[index].shape);
            let mission = if shape.is_some() || super::is_luck_chain(row.effect_type) {
                let native = source.rows(master).iter().find(|native| native.id == row.id);
                native
                    .and_then(|native| match source {
                        LuckSource::Gekisou => master.gekisou_skill(native.skill_id),
                        LuckSource::GekisouSupport => master.gekisou_support_skill(native.skill_id),
                    })
                    .map(|skill| skill.gekisou_mission_type)
            } else {
                None
            };
            rows.push((effect.row, source, shape, mission));
        }
    }
    let profile: LotteryProfile<'_> = (rows, super::has_nominal_score_probabilities(master, deck, &skills));
    // Full setup also covers adapter admission beyond the native three-range storage.
    Ok(initialized_identity(&model, play, delta_times, &(setup, profile)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::full::{JudgedNote, PlayFrame};
    use serde_json::json;

    struct Input {
        master: Master,
        deck: Vec<Performer>,
        notes: Vec<LiveNote>,
        events: Vec<(i32, i32)>,
        params: LiveParams,
        setup: GekisouSetup,
        play: LivePlay,
        dt: Vec<f32>,
    }

    impl Input {
        fn new() -> Self {
            let lots: Vec<_> =
                (0..5).map(|kind| json!({"_id":kind + 1,"_chanceLotType":kind,"_lotResult":3,"_weight":1})).collect();
            let tables = json!({
                "MasterLiveSettings": [
                    {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                    {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                    {"_id":3,"_key":"life_base","_value":"1000"},
                    {"_id":4,"_key":"life_denger","_value":"300"},
                    {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
                    {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
                    {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
                "MasterLiveNoteParameter": [{"_id":1,"_noteOperateType":1,"_scorePercent":100}],
                "MasterLiveJudgementParameter": [{"_id":1,"_noteSimulateJudgement":5,"_scorePercent":100,"_damage":0}],
                "MasterLiveJudgementTiming": [{"_id":1,"_noteJudgementType":1,"_noteSimulateJudgement":5,"_afterMs":0}],
                "MasterLiveGekisouLuckBasePoint": [{"_id":1,"_noteCategory":0,"_noteSimulateJudgement":5,"_weight":1,"_basePoint":60}],
                "MasterLiveGekisouLuckBonusLot": lots,
                "MasterSkillCondition": [
                    {"_id":1,"_conditionType":5000,"_conditionValues":[],"_conditionTargetIDs":[1],"_isPositive":true},
                    {"_id":2,"_conditionType":7021,"_conditionValues":[],"_conditionTargetIDs":[],"_isPositive":true}],
                "MasterSkillConditionSet": [
                    {"_id":1,"_group":1,"_conditionIds":[1]},
                    {"_id":2,"_group":2,"_conditionIds":[2]}],
                "MasterSkillTarget": [{"_id":1,"_skillTargetType":1,"_bandID":1}],
                "MasterGekisouSkill": [{"_id":10,"_gekisouMissionType":2}],
                "MasterGekisouSkillEffect": [{"_id":100,"_gekisouSkillID":10,"_level":1,
                    "_skillTriggerType":2,"_skillTriggerConditionGroup":2,"_skillConditionGroup":1,
                    "_skillReleaseConditionGroup":0,"_skillTargetIDs":[],"_skillEffectType":2000,
                    "_activationTimeSecond":0.0,"_effectValue":10000,"_maxEffectValue":0,
                    "_effectLimitCount":0,"_skillCumulativeConditionID":0,"_effectExecuteLimitCount":0,
                    "_effectExecuteLimitResetConditionGroup":0}],
                "MasterLiveSkillEffect": [{"_id":1000,"_liveSkillID":-1000000,"_level":1,
                    "_skillConditionGroup":0,"_skillReleaseConditionGroup":0,"_skillTargetIDs":[],
                    "_skillEffectType":2000,"_activationTimeSecond":5.0,"_effectValue":10000,
                    "_maxEffectValue":0,"_effectLimitCount":0,"_skillCumulativeConditionID":0,
                    "_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0}]
            });
            let texts: Vec<_> = tables
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, rows)| (name.clone(), json!({"_allData":rows}).to_string()))
                .collect();
            let master = Master::from_json_tables(|name| {
                texts.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
            })
            .unwrap();
            let notes = vec![LiveNote { note_id: 1, time_ms: 100, note_operate_type: 1, judgement_type: 1 }];
            Self {
                master,
                deck: vec![Performer { band_id: 1, gekisou_skill: Some((10, 1)), ..Default::default() }],
                notes,
                events: vec![(0, 0)],
                params: LiveParams {
                    total_power: 300000,
                    music_level: 20,
                    converted_note_count: 1,
                    music_length_ms: 1000,
                    score_music_length_ms: None,
                    assist_factor: 1.0,
                    skill_target_music_type: 0,
                },
                setup: GekisouSetup { fevers: vec![(0, 200)], missions: vec![2, 2, 2] },
                play: LivePlay {
                    base_seed: 0,
                    frames: vec![
                        PlayFrame { time_ms: 0, judged: vec![] },
                        PlayFrame {
                            time_ms: 100,
                            judged: vec![JudgedNote { note_id: 1, judgement: 5, judgement_time_ms: 100 }],
                        },
                        PlayFrame { time_ms: 1200, judged: vec![] },
                    ],
                },
                dt: vec![0.1; 3],
            }
        }

        fn identity(&self) -> Vec<u8> {
            chart_program_identity(
                &self.master,
                &self.deck,
                &self.notes,
                &self.events,
                self.params,
                Some(&self.setup),
                &self.play,
                Some(&self.dt),
                None,
            )
            .unwrap()
            .unwrap()
        }

        fn model(&self) -> LiveModel {
            LiveModel::new_gekisou(&self.master, &self.deck, &self.notes, &self.events, self.params, &self.setup)
                .unwrap()
        }
    }

    #[test]
    fn unrelated_master_rows_and_new_shapes_do_not_invalidate_a_held_program() {
        let mut input = Input::new();
        let expected = input.identity();
        let mut row = input.master.gekisou_skill_effects[0].clone();
        row.id = 1;
        row.skill_id = 1;
        row.activation_time_second = 1.0;
        input.master.gekisou_skills.push(crate::master::SkillRow {
            id: 1,
            gekisou_mission_type: 2,
            ..Default::default()
        });
        input.master.gekisou_skill_effects.push(row);
        let mut ordinary = input.master.live_skill_effects[0].clone();
        ordinary.id = 999999;
        ordinary.live_skill_id = 99;
        ordinary.effect_value = 70000;
        input.master.live_skill_effects.push(ordinary);
        input.master.reindex().unwrap();
        assert_eq!(input.identity(), expected);
        assert_eq!(super::super::luck_skills(&input.master).unwrap().shapes.len(), 2);
    }

    #[test]
    fn representative_ids_and_synthetic_kind_indices_are_not_measurement_dependencies() {
        let mut input = Input::new();
        input.deck[0].live_skill = Some((-1000000, 1));
        let expected = input.identity();
        input.master.gekisou_skills[0].id = 900;
        input.master.gekisou_skill_effects[0].skill_id = 900;
        input.master.gekisou_skill_effects[0].id = 9000;
        input.deck[0].gekisou_skill = Some((900, 1));
        input.master.live_skill_effects[0].live_skill_id = -1000003;
        input.master.live_skill_effects[0].id = 40000;
        input.deck[0].live_skill = Some((-1000003, 1));
        input.master.reindex().unwrap();
        assert_eq!(input.identity(), expected);
    }

    #[test]
    fn resolved_targets_conditions_and_phase_changes_invalidate() {
        let mut input = Input::new();
        let expected = input.identity();
        input.master.skill_targets[0].band_id = 2;
        input.master.reindex().unwrap();
        assert_ne!(input.identity(), expected);
        input.master.skill_targets[0].band_id = 1;
        input.master.skill_conditions[0].is_positive = false;
        input.master.reindex().unwrap();
        assert_ne!(input.identity(), expected);
        input.master.skill_conditions[0].is_positive = true;
        input
            .master
            .skill_effect_settings
            .push(serde_json::from_value(json!({"_id":1,"_skillEffectType":2000,"_phase":2})).unwrap());
        input.master.reindex().unwrap();
        assert_ne!(input.identity(), expected);
    }

    #[test]
    fn ignored_native_reset_groups_still_change_the_lottery_proof_identity() {
        let mut input = Input::new();
        input.master.gekisou_skill_effects[0].skill_trigger_condition_group = 0;
        input.master.reindex().unwrap();
        let expected = input.identity();
        let mut before = input.model();
        let direct = initialized_chart_program_identity(&mut before, &input.play, Some(&input.dt));
        input.master.gekisou_skill_effects[0].effect_execute_limit_reset_condition_group = 2;
        input.master.reindex().unwrap();
        let mut after = input.model();
        assert_eq!(initialized_chart_program_identity(&mut after, &input.play, Some(&input.dt)), direct);
        assert_ne!(input.identity(), expected);
    }

    #[test]
    fn nominal_identity_keeps_the_dp_first_source_row_lookup() {
        let mut input = Input::new();
        let expected = input.identity();
        let mut before = input.model();
        let direct = initialized_chart_program_identity(&mut before, &input.play, Some(&input.dt));
        // The held skill still compiles exactly the same. DP additionally finds the first raw row
        // by id in the whole source and then reads that row's skill mission; do not skip that read.
        input.master.gekisou_skills.push(crate::master::SkillRow {
            id: 1,
            gekisou_mission_type: 1,
            ..Default::default()
        });
        let mut other = input.master.gekisou_skill_effects[0].clone();
        other.skill_id = 1;
        input.master.gekisou_skill_effects.insert(0, other);
        input.master.reindex().unwrap();
        let mut after = input.model();
        assert_eq!(initialized_chart_program_identity(&mut after, &input.play, Some(&input.dt)), direct);
        assert_ne!(input.identity(), expected);
    }

    #[test]
    fn native_effect_order_and_counterfactual_rank_inputs_are_retained() {
        let mut input = Input::new();
        let mut second = input.master.gekisou_skill_effects[0].clone();
        second.id += 1;
        second.effect_value += 5000;
        input.master.gekisou_skill_effects.push(second);
        input.master.reindex().unwrap();
        let expected = input.identity();
        input.master.gekisou_skill_effects[0].id += 2;
        assert_ne!(input.identity(), expected, "raw id ordering determines native effect order");
        let ranked = |rank, percent| {
            chart_program_identity(
                &input.master,
                &input.deck,
                &input.notes,
                &input.events,
                input.params,
                Some(&input.setup),
                &input.play,
                Some(&input.dt),
                Some(&[RankConfirmation { frame: 0, range: 0, rank, percent }]),
            )
            .unwrap()
            .unwrap()
        };
        assert_ne!(ranked(1, 10), ranked(2, 10));
        assert_ne!(ranked(1, 10), ranked(1, 20));
        assert_ne!(ranked(1, 10), input.identity());
    }

    #[test]
    fn row_aliases_and_wrapped_execution_order_are_retained() {
        let mut input = Input::new();
        input.deck[0].live_skill = Some((-1000000, 1));
        input.deck.push(Performer { live_skill: Some((11, 1)), ..Default::default() });
        let mut second = input.master.live_skill_effects[0].clone();
        second.live_skill_id = 11;
        second.id = 1001;
        input.master.live_skill_effects.push(second);
        input.master.reindex().unwrap();
        let expected = input.identity();
        input.master.live_skill_effects[1].id = 1000;
        assert_ne!(input.identity(), expected, "row-id equality is shared by conversion contexts");
        input.master.live_skill_effects[1].id = 1001;
        input.master.live_skill_effects[1].live_skill_id = -1000001;
        input.deck[1].live_skill = Some((-1000001, 1));
        assert_ne!(input.identity(), expected, "live-pool order is observable");

        let mut a = input.model();
        let mut b = a.clone();
        // Keep pool membership/order identical but change the wrapped enabled-live order only.
        for skill in b.live.iter_mut().filter(|skill| skill.member == 0) {
            skill.key = i64::MAX / 10 + 1;
        }
        for pool in b.live_pools.iter_mut().filter(|pool| pool.member == 0) {
            pool.key = i64::MAX / 10 + 1;
        }
        assert_ne!(
            initialized_chart_program_identity(&mut a, &input.play, Some(&input.dt)),
            initialized_chart_program_identity(&mut b, &input.play, Some(&input.dt))
        );
    }

    #[test]
    fn finite_float_bits_and_full_schedule_are_preserved_nonfinite_states_decline() {
        let input = Input::new();
        let mut model = input.model();
        let mut keys = std::collections::BTreeSet::new();
        for value in
            [0.0f32, -0.0, f32::from_bits(1), f32::MIN_POSITIVE, 1.0, f32::from_bits(1.0f32.to_bits() + 1), f32::MAX]
        {
            model.score.calc.assist_factor = value;
            assert!(keys.insert(initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt)).unwrap()));
        }
        for value in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, f32::from_bits(0xffc00001)] {
            model.score.calc.assist_factor = value;
            assert!(initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt)).is_none());
        }
        model.score.calc.assist_factor = 1.0;
        let expected = initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt));
        let mut play = input.play.clone();
        play.base_seed = 1;
        assert_ne!(initialized_chart_program_identity(&mut model, &play, Some(&input.dt)), expected);
        play = input.play.clone();
        play.frames[1].judged[0].judgement = 6;
        assert_ne!(initialized_chart_program_identity(&mut model, &play, Some(&input.dt)), expected);
        let mut dt = input.dt.clone();
        dt[0] = f32::from_bits(dt[0].to_bits() + 1);
        assert_ne!(initialized_chart_program_identity(&mut model, &input.play, Some(&dt)), expected);
        dt[0] = f32::NAN;
        assert!(initialized_chart_program_identity(&mut model, &input.play, Some(&dt)).is_none());
    }

    #[test]
    fn random_lookup_iteration_is_sorted_and_execution_model_is_untouched() {
        let input = Input::new();
        let mut model = input.model();
        model.score.calc.note_factor_percent.extend([(2, 90), (3, 80)]);
        let before = format!("{model:?}");
        let expected = initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt));
        assert_eq!(format!("{model:?}"), before);
        let entries: Vec<_> = model.score.calc.note_factor_percent.drain().collect();
        model.score.calc.note_factor_percent.extend(entries.into_iter().rev());
        assert_eq!(initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt)), expected);
        model.score.calc.note_factor_percent.insert(2, 91);
        assert_ne!(initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt)), expected);
        model.frame_timed(0, &[], 0.1).unwrap();
        assert!(initialized_chart_program_identity(&mut model, &input.play, Some(&input.dt)).is_none());
    }

    #[test]
    fn opaque_tokens_split_between_formatter_writes_decline() {
        for parts in [["N", "aN"], ["-i", "nf"], ["<bor", "rowed>"], ["Scr", "ipted"]] {
            let mut out = IdentityWriter::new();
            out.write_str(parts[0]).unwrap();
            assert!(out.write_str(parts[1]).is_err());
        }
    }
}
