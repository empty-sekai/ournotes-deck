//! Construction of a Live model from a formation, a chart and the master data.

use super::*;

impl LiveModel {
    /// Builds a live without Gekisou: `deck` in skill order, the chart notes, the chart's skill events
    /// `(performer index, time)` in chart order.
    pub fn new(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
    ) -> Result<LiveModel, Error> {
        LiveModel::build(master, deck, notes, skill_events, params, None, false, None, None)
    }

    /// Builds a live with Gekisou (see [`LiveModel::new`]); frame delta times drive the ranges' end delays, see
    /// [`LiveModel::run_timed`].
    pub fn new_gekisou(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
    ) -> Result<LiveModel, Error> {
        LiveModel::build(master, deck, notes, skill_events, params, Some(setup), false, None, None)
    }

    /// Dynamic controller with explicit external ranking. No native three-range network limit is imposed.
    /// This is a simulation adapter, not a claim that the native network calculator accepts fourth ranges.
    pub fn new_gekisou_external(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
    ) -> Result<LiveModel, Error> {
        LiveModel::build(master, deck, notes, skill_events, params, Some(setup), true, None, None)
    }

    /// Counterfactual solo scoring with explicitly queued ranks and percentages. Range scores use
    /// the solo updater's exact start/end timestamp queries. This is an adapter for chart statistics
    /// and rank sensitivity; like solo, only the first completion in a frame is ranked. Native solo
    /// always confirms rank 1, and this does not emulate network play.
    pub fn new_gekisou_ranked(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: &GekisouSetup,
    ) -> Result<LiveModel, Error> {
        let mut model = LiveModel::build(master, deck, notes, skill_events, params, Some(setup), false, None, None)?;
        let gk = model.gk.as_mut().expect("Gekisou setup supplied");
        gk.external_ranking = true;
        gk.solo_score_queries = true;
        Ok(model)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build(
        master: &Master,
        deck: &[Performer],
        notes: &[LiveNote],
        skill_events: &[(i32, i32)],
        params: LiveParams,
        setup: Option<&GekisouSetup>,
        external_ranking: bool,
        luck: Option<luck::SharedScript>,
        lottery: Option<&LuckSkills>,
    ) -> Result<LiveModel, Error> {
        if !matches!(params.skill_target_music_type, 0..=5 | 99) {
            return Err(Error::Input("unknown skill target music type".into()));
        }
        let mut map = FxHashMap::with_capacity_and_hasher(notes.len(), Default::default());
        for n in notes {
            map.insert(n.note_id, *n);
        }
        let gk = match setup {
            None => None,
            Some(s) => Some(LiveModel::gekisou_live(master, s, &map, external_ranking)?),
        };
        let settings = LiveScoreSettings::from_master(master)?;
        let table = ComboTable::from_master(master)?;
        let calc = LiveScoreCalculator::new(
            params.total_power,
            params.music_level,
            params.converted_note_count,
            &settings,
            1.0,
            params.assist_factor,
            Some(table),
        );
        let (base, damage) = life::life_settings(master)?;
        let life_length = notes.iter().map(|n| n.time_ms).max().map_or(1000, |t| t.wrapping_add(1000));
        let life = LifeController::new(base, damage, life_length)?;
        let score_length = match params.score_music_length_ms {
            Some(l) if l != 0 => l,
            _ => params.music_length_ms,
        };
        let have_just: FxHashSet<i64> = master
            .live_judgement_timings
            .iter()
            .filter(|r| r.note_simulate_judgement == 6)
            .map(|r| r.note_judgement_type)
            .collect();
        let no_just: FxHashSet<i64> = master
            .live_judgement_timings
            .iter()
            .map(|r| r.note_judgement_type)
            .filter(|t| !have_just.contains(t))
            .collect();
        let phase_of = effect_phases(master);
        let phase = |t: i64| phase_of.get(&t).copied().unwrap_or(1);
        let factory = Factory::new(master, deck, base, 0, params.skill_target_music_type);

        let mut rows = Vec::new();
        let mut live = Vec::new();
        let mut live_pools = Vec::new();
        for (k, p) in deck.iter().enumerate() {
            let Some((sid, lv)) = p.live_skill else { continue };
            let mut rs: Vec<_> =
                master.live_skill_effects.iter().filter(|r| r.live_skill_id == sid && r.level == lv).collect();
            rs.sort_by_key(|r| r.id);
            let mut effects = Vec::with_capacity(rs.len());
            for r in rs {
                // `EffectLimitCount` is read only by the appliers that use it (12004, 11005, 12006 / 13005,
                // 4000..=4003, 13001), whatever the skill type; no build-time gate exists natively.
                let condition = factory.group(r.skill_condition_group, k)?;
                let release = factory.group(r.skill_release_condition_group, k)?;
                let cumulative = factory.cumulative(r.skill_cumulative_condition_id, k)?;
                let mut state = EffectState { execute_ms: -1, finish_ms: -1, ..Default::default() };
                if let Some(c) = &cumulative {
                    state.cumulative_count = c.init_count();
                    state.cumulative_unit = c.unit();
                    state.cumulative_max = c.max();
                }
                let targets: Result<Vec<i64>, i64> =
                    r.skill_target_ids.iter().map(|&t| master.skill_target(t).map(|x| x.judgement).ok_or(t)).collect();
                rows.push(EffectRow {
                    id: r.id,
                    effect_type: r.skill_effect_type,
                    effect_value: r.effect_value,
                    max_effect_value: r.max_effect_value,
                    effect_limit_count: r.effect_limit_count,
                    applier: applier_plan::ApplierPlan::compile(r.skill_effect_type, targets.is_ok()),
                    targets,
                });
                effects.push(LiveEffect {
                    row: rows.len() - 1,
                    act: r.activation_time_second,
                    phase: phase(r.skill_effect_type),
                    state,
                    condition,
                    release,
                    cumulative,
                });
            }
            let key = sid.wrapping_mul(1000).wrapping_add(100).wrapping_add(k as i64);
            let mut available = VecDeque::new();
            for index in 0..5 {
                available.push_back(live.len());
                live.push(LiveSkill {
                    key,
                    member: k,
                    index,
                    parent_state: STAY,
                    trigger: TriggerResult::default(),
                    effects: effects.clone(),
                });
            }
            live_pools.push(LivePool { key, member: k, available });
        }
        live_pools.sort_by_key(|p| p.key);

        let mut cond = Vec::new();
        for (k, p) in deck.iter().enumerate() {
            for &(sid, lv) in &p.support_skills {
                let rs = master
                    .support_skill_effects
                    .iter()
                    .filter(|r| r.support_skill_id == sid && r.level == lv)
                    .map(CondRow::from)
                    .collect();
                cond.push(condition_skill(
                    master,
                    &factory,
                    &phase,
                    rs,
                    k,
                    SKILL_TYPE_SUPPORT,
                    None,
                    &mut rows,
                    luck.as_ref(),
                )?);
            }
        }
        if gk.is_none() {
            // SkillStatus builds the Gekisou skill dictionary whether or not Gekisou is on, so a mission outside
            // 1..=4 fails there too (see `gekisou_mission_group`).
            for p in deck {
                if let Some(row) = p.gekisou_skill.and_then(|(sid, _)| master.gekisou_skill(sid)) {
                    gekisou_mission_group(row.gekisou_mission_type)?;
                }
            }
        }
        if gk.is_some() {
            gekisou_conditions(&factory, &phase, luck.as_ref(), lottery, &mut cond, &mut rows)?;
        }

        Ok(LiveModel {
            #[cfg(feature = "search-diagnostics")]
            rush_probes: None,
            rush_effect_log: None,
            luck_suppressed: Vec::new(),
            notes: map,
            events: skill_events.to_vec(),
            fired: vec![false; skill_events.len()],
            music_length_ms: params.music_length_ms,
            is_live_finished: false,
            program_has_started: false,
            random: LiveRandom::new(0),
            life,
            phase_life: None,
            combo: combo::ComboCounter::new(notes.len()),
            score: IncrementalCalculator::new(calc, score_length),
            conversion: Conversion::new(no_just),
            rows,
            live,
            live_pools,
            enabled_live: Vec::new(),
            cond,
            guards: FxHashMap::default(),
            life_reductions: FxHashMap::default(),
            life_limits: FxHashMap::default(),
            frame_events: Vec::new(),
            judged: Vec::new(),
            trace: Vec::new(),
            frame_time: 0,
            frame_score: 0,
            prev_confirmed_rank: None,
            simulator_previous_combo: 0,
            current_combo: 0,
            scorectl: ScoreCtl::default(),
            gk,
            gk_appliers: GkAppliers::default(),
            raw_runtime: None,
            raw_pending: None,
            frame_rank_confirmation: None,
            rank_timeline: Vec::new(),
            next_rank_confirmation: 0,
            scratch: FrameScratch::default(),
            touched_events: 0,
            touched_skills: 0,
        })
    }

    fn gekisou_live(
        master: &Master,
        setup: &GekisouSetup,
        notes: &FxHashMap<i32, LiveNote>,
        external_ranking: bool,
    ) -> Result<GekisouLive, Error> {
        // The score keeps min(fever count, 3) Gekisou ranges; every fever still has its fever updater, and a later
        // fever's first state change indexes the range states out of bounds, which the controller reports as an
        // error.
        let n = if external_ranking { setup.fevers.len() } else { setup.fevers.len().min(gekisou::MAX_RANGES) };
        let mut ranges = Vec::with_capacity(n);
        for (i, &(s, e)) in setup.fevers[..n].iter().enumerate() {
            let m = *setup.missions.get(i).ok_or_else(|| Error::Input("fewer missions than fevers".into()))?;
            ranges.push((s, e, m));
        }
        let after = master.live_judgement_timings.iter().map(|r| r.after_ms).max().unwrap_or(0);
        let gauge_max = setting(master, "gekisou_luck_gauge_max")?;
        let gauge_max_rush = setting(master, "gekisou_luck_gauge_max_rush")?;
        let rush_percent = setting(master, "gekisou_luck_rush_score_bonus_percent")?;
        let ctrl = Controller::new(
            ranges,
            notes.values().map(|n| (n.note_id, n.time_ms)),
            master,
            gauge_max,
            gauge_max_rush,
            rush_percent,
            after,
        )?;
        let factors = if external_ranking {
            [[0; 5]; 3]
        } else {
            let [m0, m1, m2] = match setup.missions.get(..3) {
                Some(&[a, b, c]) => [a, b, c],
                _ => return Err(Error::Input("a Gekisou song has three missions".into())),
            };
            gekisou::ranking_factors(master, gekisou::mission_pattern(m0, m1, m2))?
        };
        Ok(GekisouLive {
            ctrl,
            fever: FeverUpdater::new(&setup.fevers),
            factors,
            external_ranking,
            solo_score_queries: false,
            completed_scores: vec![false; n],
            pending_ranks: vec![None; n],
            fever_updates: Vec::new(),
            prev_lots: Vec::new(),
            prev_lot_ms: 0,
            rank_bonus: Vec::new(),
            rank_applications: Vec::new(),
            program_rank_snapshots: vec![(None, None); n],
            rank_snapshot_queries: vec![(None, None); n],
        })
    }
}
