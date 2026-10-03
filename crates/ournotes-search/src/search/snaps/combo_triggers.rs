//! Necessary 7005 trigger conditions from judgements processed before the skill phase.
use super::*;

const MAX_CELLS: usize = 2_000_000;

/// The largest exact integer 12000 bonus of any admitted five-member deck.
/// Conditions, character uniqueness and Snap uniqueness are relaxed. A member
/// has its own rows plus at most one of the allowed Snap row lists.
pub(super) fn maximum_bonus(members: &[Vec<Row>], snaps: &[Vec<Row>]) -> Option<i64> {
    fn one(rows: &[Row]) -> Option<i64> {
        let mut sum = 0i64;
        for r in rows {
            // Fixed count additions are not bounded by a per-judgement bonus.
            if matches!(r.effect_type, 12001 | 12002 | 12003 | 12005) {
                return None;
            }
            if r.effect_type != 12000 {
                continue;
            }
            if !r.gk
                || r.value < 0
                || !matches!(r.trigger_type, 1 | 2)
                || r.trigger_type == 2 && has_activation_time(r.act)
            {
                return None;
            }
            let copies = if r.trigger_type == 2 { 1 } else { POOL as i64 };
            sum = sum.checked_add(r.value.checked_mul(copies)?)?;
        }
        Some(sum)
    }
    let snap = snaps.iter().map(|r| one(r)).collect::<Option<Vec<_>>>()?.into_iter().max().unwrap_or(0);
    let mut members = members.iter().map(|r| one(r)?.checked_add(snap)).collect::<Option<Vec<_>>>()?;
    members.sort_unstable_by(|a, b| b.cmp(a));
    let total = members.into_iter().take(5).try_fold(0i64, |a, b| a.checked_add(b))?;
    // Every cb_stack prefix contains paired nonnegative add/remove commands of
    // at most the updater pool capacity. Integer arithmetic in binary32 is exact
    // throughout [0, 2^24]; include the initial one. No float drift is assumed.
    (total < 1 << 24).then_some(total)
}

#[derive(Debug)]
pub(super) struct ComboTriggers {
    /// None: the threshold cannot hold. Some(t): any successful direct 7005
    /// trigger's effective timestamp is at least t, including no-override fallback.
    by_threshold: HashMap<i64, Vec<Option<i64>>>,
}

impl ComboTriggers {
    pub(super) fn compile(
        master: &Master,
        g: &GkFrames,
        entries: &[(usize, LiveNote, i32)],
        reach: &[u8; 8],
        max_bonus: i64,
    ) -> Option<Self> {
        let mut thresholds: Vec<i64> = master
            .skill_conditions
            .iter()
            .filter(|c| c.condition_type == 7005 && c.is_positive)
            .filter_map(|c| c.condition_values.first().copied())
            .filter(|&t| t > 0)
            .collect();
        thresholds.sort_unstable();
        thresholds.dedup();
        if thresholds.is_empty()
            || !(0..1 << 24).contains(&max_bonus)
            || thresholds.len().checked_mul(g.times.len())? > MAX_CELLS
            || g.ranges.len().checked_mul(entries.len())? > MAX_CELLS
            || entries.iter().any(|&(f, _, j)| f >= g.times.len() || !(0..8).contains(&j))
            || entries.windows(2).any(|w| w[0].0 > w[1].0)
        {
            return None;
        }
        let lists: Vec<Vec<(usize, i32)>> = g
            .ranges
            .iter()
            .map(|r| {
                entries
                    .iter()
                    .filter_map(|&(f, n, j)| {
                        (r.start <= n.time_ms && n.time_ms <= r.end && reach[j as usize] & 0b0111_1000 != 0)
                            .then_some((f, n.time_ms))
                    })
                    .collect()
            })
            .collect();
        let mut by_threshold = HashMap::new();
        for threshold in thresholds {
            by_threshold.insert(threshold, threshold_frames(&g.times, &g.current, &lists, threshold, max_bonus + 1)?);
        }
        Some(Self { by_threshold })
    }

    pub(super) fn possible(&self, threshold: i64, frame: usize) -> Option<bool> {
        self.by_threshold.get(&threshold)?.get(frame).map(Option::is_some)
    }

    /// Only a sole positive checker guarantees the group's override comes from
    /// this 7005. Compound/negative triggers keep the original timestamp envelope.
    pub(super) fn trigger_time(&self, env: &Env, group: i64, frame: usize) -> Option<i64> {
        let sets = env.sets.get(&group)?;
        if sets.len() != 1 || sets[0].len() != 1 {
            return None;
        }
        let c = env.master.skill_condition(sets[0][0])?;
        if c.condition_type != 7005 || !c.is_positive {
            return None;
        }
        self.by_threshold.get(c.condition_values.first()?)?.get(frame).copied().flatten()
    }
}

/// An actual combo cannot exceed good_count * max_increment: breaks/protection
/// only remove increments, and 12000 has no direct count addition. Thus reaching
/// T needs at least ceil(T/max_increment) Good..Just judgements. The last actual
/// good judgement must be among eligible entries at or after that ordinal.
/// Taking their minimum timestamp is safe even for delayed/out-of-order notes.
/// Entries in frame f are deliberately admitted only at f+1: Controller.update
/// and its recount run after both skill phases. Adding a 12000 command only flags
/// that later recount; it cannot bootstrap another 7005 inside the same frame.
fn threshold_frames(
    times: &[i32],
    current: &[Option<usize>],
    lists: &[Vec<(usize, i32)>],
    threshold: i64,
    max_increment: i64,
) -> Option<Vec<Option<i64>>> {
    if threshold <= 0 || max_increment <= 0 || current.len() != times.len() {
        return None;
    }
    let needed = threshold / max_increment + i64::from(threshold % max_increment != 0);
    let mut used = vec![0usize; lists.len()];
    let mut lower = vec![i32::MAX; lists.len()];
    let mut out = Vec::with_capacity(times.len());
    for (f, &time) in times.iter().enumerate() {
        for (ri, entries) in lists.iter().enumerate() {
            while used[ri] < entries.len() && entries[used[ri]].0 < f {
                let (_, chart_time) = entries[used[ri]];
                used[ri] += 1;
                if used[ri] as u128 >= needed as u128 {
                    lower[ri] = lower[ri].min(chart_time);
                }
            }
        }
        out.push(match current[f] {
            Some(ri) if ri < lists.len() && used[ri] as u128 >= needed as u128 => {
                // A negative last_combo_ms disables the native override. The
                // frame time is its fallback, so include both possibilities.
                Some(i64::from(lower[ri].min(time)))
            }
            _ => None,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(value: i64, trigger_type: i64) -> Row {
        Row {
            trigger: 0,
            trigger_type,
            condition: 0,
            release: 0,
            reset: 0,
            cumulative: 0,
            effect_type: 12000,
            value,
            act: 0.0,
            limit: 0,
            execute_limit: 0,
            targets: Vec::new(),
            max_value: 0,
            gk: true,
            gate: 1,
        }
    }

    #[test]
    fn physical_capacity_relaxes_unique_resources_but_keeps_five_members() {
        let members: Vec<_> = [6, 5, 4, 4, 4, 3].map(|n| vec![row(n, 2)]).into();
        assert_eq!(maximum_bonus(&members, &[]), Some(23));
        assert_eq!(maximum_bonus(&members, &[vec![row(2, 1)]]), Some(73));
        assert_eq!(maximum_bonus(&[vec![row(-1, 2)]], &[]), None);
        assert_eq!(maximum_bonus(&[vec![row(1 << 24, 2)]], &[]), None);
        let mut direct_addition = row(1, 1);
        direct_addition.effect_type = 12002;
        assert_eq!(maximum_bonus(&[vec![direct_addition]], &[]), None);
    }

    #[test]
    fn threshold_waits_for_prior_frames_and_keeps_all_current_frame_judgements_out() {
        let times = [0, 40, 80, 120];
        let current = [Some(0); 4];
        let list = vec![vec![(1, 11), (1, 23), (2, 61)]];
        assert_eq!(threshold_frames(&times, &current, &list, 2, 1).unwrap(), [None, None, Some(23), Some(23)]);
        // The real Lv5 threshold25 cannot hold with one prior judgement when
        // the relaxed full-pool increment is24, and its override is not range0.
        let list = vec![vec![(1, 11), (2, 61)]];
        assert_eq!(threshold_frames(&times, &current, &list, 25, 24).unwrap(), [None, None, None, Some(61)]);
    }

    #[test]
    fn delayed_notes_range_switches_and_fallback_time_are_conservative() {
        let times = [0, 40, 80, 120, 160];
        let current = [None, Some(0), Some(0), Some(1), Some(0)];
        let lists = vec![vec![(0, 90), (1, 60), (3, -10)], vec![(1, 100)]];
        assert_eq!(threshold_frames(&times, &current, &lists, 2, 1).unwrap(), [None, None, Some(60), None, Some(-10)]);
    }

    #[test]
    fn exhaustive_good_subsets_fit_count_and_override_necessary_conditions() {
        let times = [0, 40, 80, 120, 160, 200];
        let current = [Some(0); 6];
        let entries = [(0, 30), (1, 10), (1, 35), (3, 150), (4, 100)];
        for increment in 1..=5i64 {
            for threshold in 1..=27i64 {
                let bound = threshold_frames(&times, &current, &[entries.to_vec()], threshold, increment).unwrap();
                for subset in 0..1usize << entries.len() {
                    for (f, &time) in times.iter().enumerate() {
                        let selected: Vec<_> = entries
                            .iter()
                            .enumerate()
                            .filter(|&(i, e)| subset & (1 << i) != 0 && e.0 < f)
                            .map(|(_, e)| e.1)
                            .collect();
                        if selected.len() as i64 * increment >= threshold {
                            let actual = selected.last().copied().filter(|&t| t >= 0).unwrap_or(time);
                            assert!(bound[f].is_some_and(|lo| lo <= actual as i64));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn compiled_7005_window_moves_past_range_start_but_compound_override_stays_wide() {
        let master = Master::from_json_tables(|name| match name {
            "MasterSkillCondition" => Some(
                r#"{"_allData":[
                {"_id":1,"_conditionType":7005,"_conditionValues":[25],"_isPositive":true},
                {"_id":2,"_conditionType":5000,"_isPositive":true}]}"#,
            ),
            "MasterSkillConditionSet" => Some(
                r#"{"_allData":[
                {"_id":1,"_group":1,"_conditionIds":[1]},
                {"_id":2,"_group":2,"_conditionIds":[1,2]}]}"#,
            ),
            _ => None,
        })
        .unwrap();
        let frames = [0, 40, 80, 120, 160];
        let schedule = Schedule {
            states: vec![vec![RS_START], vec![RS_PLAYING], vec![RS_PLAYING], vec![RS_PLAYING], vec![RS_PLAYING]],
            ranges: vec![RangeFacts {
                start: 0,
                end: 200,
                mission: MISSION_COMBO,
                pct: 0,
                f_start: Some(0),
                f_complete: None,
                f_finish: None,
            }],
        };
        let entries = [
            (1, LiveNote { note_id: 0, time_ms: 11, note_operate_type: 1, judgement_type: 1 }, 5),
            (2, LiveNote { note_id: 1, time_ms: 61, note_operate_type: 1, judgement_type: 1 }, 5),
        ];
        let reach = std::array::from_fn(|j| 1u8 << j);
        let mut g = GkFrames::new(&schedule, &frames, &entries);
        g.combo_triggers = ComboTriggers::compile(&master, &g, &entries, &reach, 23);
        let env = Env {
            master: &master,
            events: &[],
            sets: master.skill_condition_sets.iter().map(|s| (s.group, vec![s.condition_ids.as_slice()])).collect(),
            life_lo: 0,
            life_hi: 1000,
            life_rigid: false,
            raw: vec![5],
            count_reach: reach,
            entry_reach: Vec::new(),
            gk: None,
            gkf: Some(Rc::new(g)),
            rush_cache: RefCell::new(HashMap::new()),
            gk_cache: RefCell::new(HashMap::new()),
            budget_cache: RefCell::new(HashMap::new()),
            ramp_cache: RefCell::new(HashMap::new()),
        };
        let mut direct = row(5, 2);
        direct.trigger = 1;
        let window = gk_row_timing(&env, &direct);
        assert_eq!(window.starts, [3, 4]);
        assert_eq!(window.win, [(61, i64::MAX, 1.0)]);
        // Count necessary conditions are safe within AND, but another checker
        // can supply the compound group's override. Do not narrow that timestamp.
        direct.trigger = 2;
        assert_eq!(gk_row_timing(&env, &direct).win, [(0, i64::MAX, 1.0)]);
    }

    #[test]
    fn complete_controller_recounts_only_after_both_skill_phases() {
        use ournotes_sim::live::full::JudgedNote;
        use serde_json::json;
        let effect = |id, skill, kind, trigger| {
            json!({
                "_id":id,"_gekisouSkillID":skill,"_level":1,"_skillTriggerType":2,
                "_skillTriggerConditionGroup":trigger,"_skillConditionGroup":0,"_skillReleaseConditionGroup":0,
                "_skillTargetIDs":[],"_skillEffectType":kind,"_activationTimeSecond":0.0,
                "_effectValue":if kind == 12000 {4} else {10},"_maxEffectValue":0,"_effectLimitCount":0,
                "_skillCumulativeConditionID":0,"_effectExecuteLimitCount":0,"_effectExecuteLimitResetConditionGroup":0
            })
        };
        let tables = json!({
            "MasterLiveSettings":[
                {"_id":1,"_key":"note_score_adjustment_factor","_value":"3"},
                {"_id":2,"_key":"note_score_life_onus_factor","_value":"0.5"},
                {"_id":3,"_key":"life_base","_value":"1000"},
                {"_id":4,"_key":"life_denger","_value":"300"},
                {"_id":5,"_key":"gekisou_luck_gauge_max","_value":"140"},
                {"_id":6,"_key":"gekisou_luck_gauge_max_rush","_value":"70"},
                {"_id":7,"_key":"gekisou_luck_rush_score_bonus_percent","_value":"10"}],
            "MasterLiveJudgementParameter":[{"_id":1,"_noteSimulateJudgement":6,"_damage":0}],
            "MasterSkillEffectSetting":[{"_id":1,"_skillEffectType":12000,"_phase":1},
                                         {"_id":2,"_skillEffectType":13000,"_phase":2}],
            "MasterSkillCondition":[{"_id":1,"_conditionType":7005,"_conditionValues":[2],"_isPositive":true},
                                    {"_id":2,"_conditionType":7005,"_conditionValues":[3],"_isPositive":true}],
            "MasterSkillConditionSet":[{"_id":1,"_group":1,"_conditionIds":[1]},
                                       {"_id":2,"_group":2,"_conditionIds":[2]}],
            "MasterGekisouSkill":[{"_id":1,"_gekisouMissionType":1},{"_id":2,"_gekisouMissionType":1}],
            "MasterGekisouSkillEffect":[effect(1,1,12000,1),effect(2,2,13000,2)]
        });
        let texts: Vec<_> =
            tables.as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!({"_allData":v}).to_string())).collect();
        let master =
            Master::from_json_tables(|name| texts.iter().find(|(n, _)| n == name).map(|(_, s)| s.as_str())).unwrap();
        // Unscored operations still enter the actual judgement/controller path.
        let notes: Vec<_> = [40, 80, 120]
            .into_iter()
            .enumerate()
            .map(|(i, t)| LiveNote { note_id: i as i32, time_ms: t, note_operate_type: 122, judgement_type: 1 })
            .collect();
        let deck = [
            Performer { gekisou_skill: Some((1, 1)), ..Default::default() },
            Performer { gekisou_skill: Some((2, 1)), ..Default::default() },
        ];
        let params = LiveParams {
            total_power: 1,
            music_level: 1,
            converted_note_count: 3,
            music_length_ms: 1000,
            score_music_length_ms: None,
            skill_target_music_type: 0,
            assist_factor: 1.0,
        };
        let setup = GekisouSetup { fevers: vec![(0, 500)], missions: vec![1, 1, 1] };
        let mut model = LiveModel::new_gekisou(&master, &deck, &notes, &[], params, &setup).unwrap();
        model.frame_timed(0, &[], 0.0).unwrap();
        for (i, t) in [40, 80].into_iter().enumerate() {
            model
                .frame_timed(t, &[JudgedNote { note_id: i as i32, judgement: 6, judgement_time_ms: t }], 0.04)
                .unwrap();
            assert_eq!(model.gekisou_ranges()[0].combo, (i + 1) as i32);
        }
        // Prior combo2 permits phase1's +4; it is backdated to the last good
        // note80 and recounts that note too. Phase2 still reads old combo2, not
        // the new count from note120 or from the queued phase1 bonus.
        model.frame_timed(120, &[JudgedNote { note_id: 2, judgement: 6, judgement_time_ms: 120 }], 0.04).unwrap();
        assert_eq!((model.gekisou_ranges()[0].combo, model.gekisou_ranges()[0].just_count), (11, 3));
        // The next frame may activate the phase2 Just bonus, even with no new
        // judgement: the complete controller replays the existing history.
        model.frame_timed(160, &[], 0.04).unwrap();
        assert_eq!((model.gekisou_ranges()[0].combo, model.gekisou_ranges()[0].just_count), (11, 13));
    }
}
