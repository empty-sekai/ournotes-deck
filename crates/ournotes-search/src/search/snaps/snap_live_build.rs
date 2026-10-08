//! Construction of the Snap Live objective and its bound envelopes.
use super::*;

impl<'a> SnapLive<'a> {
    /// Prepares the objective for the allowed members and snaps (`t.snaps`); rejects cards the simulation cannot run
    /// and inputs outside the domain of the bounds.
    pub fn new(
        pool: &Pool<'a>,
        t: &Tables,
        allowed_members: &[bool],
        setup: &'a FullSetup,
    ) -> Result<SnapLive<'a>, Error> {
        let terminal_caps_admitted = !ablated(
            ablate::RANK_BONUS
                | ablate::LUCK
                | ablate::GEKISOU_COMBO
                | ablate::EARLY_STOP_EQUAL
                | ablate::OBSERVED_MAX
                | ablate::CLASS_KEY
                | ablate::PREFIX_LATE,
        );
        let master: &'a Master = pool.master;
        let settings = LiveScoreSettings::from_master(master)?;
        let combo = ComboTable::from_master(master)?;
        if setup.play.frames.iter().any(|frame| frame.time_ms < 0)
            || setup.notes.iter().any(|note| note.time_ms < 0)
            || setup.events.iter().any(|&(_, time)| time < 0)
        {
            return Err(Error::Domain("live score proof requires nonnegative clock timestamps".into()));
        }
        // A finish may be clamped to the music length even when its start was
        // later. Its command must be beyond every note's native score frame:
        // an earlier timestamp in the same frame can still share rounding and undo state.
        if setup.params.music_length_ms > 0 {
            let frames = ScoreFrames::new(&setup.params);
            let finish = frames.at(setup.params.music_length_ms as i64);
            if setup.notes.iter().any(|note| frames.at(note.time_ms as i64) >= finish) {
                return Err(Error::Domain("score note reaches the music-length finish clamp frame".into()));
            }
        }
        let setting = |key: &str| -> Result<i64, Error> {
            let r = master
                .live_settings
                .iter()
                .find(|r| r.key == key)
                .ok_or_else(|| Error::Master(format!("MasterLiveSettings {key} missing")))?;
            r.value
                .trim()
                .parse::<i64>()
                .map_err(|_| Error::Master(format!("MasterLiveSettings {key} is not an integer")))
        };
        let base = setting("life_base")?;
        setting("life_denger")?;
        if base <= 0 || base > (1 << 29) {
            return Err(Error::Domain(format!("life_base {base} outside the modelled range")));
        }
        let mut damage: HashMap<i64, i64> = HashMap::new();
        for r in &master.judgement_parameters {
            if damage.insert(r.note_simulate_judgement, r.damage).is_some() {
                return Err(Error::Master("duplicate judgement parameter".into()));
            }
            if r.damage < 0 || r.damage > i32::MAX as i64 {
                return Err(Error::Domain("judgement damage outside the modelled range".into()));
            }
        }
        let notes: HashMap<i32, LiveNote> = setup.notes.iter().map(|n| (n.note_id, *n)).collect();
        // stream entries: (frame index, note, raw judgement)
        let mut entries = Vec::new();
        for (fi, f) in setup.play.frames.iter().enumerate() {
            for j in &f.judged {
                let n = *notes.get(&j.note_id).ok_or_else(|| Error::Input(format!("unknown note {}", j.note_id)))?;
                // Count-triggered effects may use this chart time as their
                // execution timestamp. A future start can otherwise be followed
                // by a release before that start, creating a negative window.
                if n.time_ms > f.time_ms {
                    return Err(Error::Domain("score note is judged before its chart time".into()));
                }
                entries.push((fi, n, j.judgement));
            }
        }
        let mut raw: Vec<i32> = entries.iter().map(|e| e.2).collect();
        raw.sort_unstable();
        raw.dedup();

        let members: Vec<usize> = (0..pool.members.len()).filter(|&m| allowed_members[m]).collect();
        fn attr(m: &MemberView) -> Attr<'_> {
            m
        }
        let mut sets: HashMap<i64, Vec<&[i64]>> = HashMap::new();
        for s in &master.skill_condition_sets {
            sets.entry(s.group).or_default().push(&s.condition_ids);
        }
        let mut env = Env {
            master,
            events: &setup.events,
            sets,
            life_lo: 0,
            life_hi: 2 * base,
            life_rigid: false,
            raw,
            count_reach: std::array::from_fn(|j| 1u8 << j),
            entry_reach: Vec::new(),
            gk: None,
            gkf: None,
            rush_cache: RefCell::new(HashMap::new()),
            gk_cache: RefCell::new(HashMap::new()),
            budget_cache: RefCell::new(HashMap::new()),
            ramp_cache: RefCell::new(HashMap::new()),
        };
        // every row an allowed card can bring
        let mut live: HashMap<(i64, i64), Vec<Row>> = HashMap::new();
        for &m in &members {
            let v = &pool.members[m];
            let k = (v.live_skill_id, v.live_skill_level);
            if let std::collections::hash_map::Entry::Vacant(e) = live.entry(k) {
                e.insert(live_rows(&env, k.0, k.1)?);
            }
        }
        let mut snap_rows: Vec<Vec<Vec<Row>>> = Vec::with_capacity(t.snaps.len());
        for &s in &t.snaps {
            let mut per = Vec::new();
            for (id, lv) in pool.snaps[s].support_skills()? {
                per.push(support_rows(&env, id, lv)?);
            }
            check_snap_program_identities(per.iter().flatten())?;
            snap_rows.push(per);
        }
        // with Gekisou on: the Gekisou support skills of each snap (they run only for a member with a Gekisou skill)
        // and the Gekisou skill of each member, each gated by its mission
        let gk_on = setup.gk.is_some();
        let mut snap_gk_rows: Vec<Vec<Vec<Row>>> = vec![Vec::new(); t.snaps.len()];
        let mut member_gk: HashMap<(i64, i64), Vec<Row>> = HashMap::new();
        if gk_on {
            for (j, &s) in t.snaps.iter().enumerate() {
                for (id, lv) in pool.snaps[s].gekisou_support_skills()? {
                    let row = master
                        .gekisou_support_skill(id)
                        .ok_or_else(|| Error::Master(format!("unknown Gekisou support skill {id}")))?;
                    let table = &master.gekisou_support_skill_effects;
                    snap_gk_rows[j].push(gekisou_rows(
                        &env,
                        table,
                        RowSource::GekisouSupport,
                        id,
                        lv,
                        row.gekisou_mission_type,
                    )?);
                }
                check_snap_program_identities(snap_gk_rows[j].iter().flatten())?;
            }
            for &m in &members {
                let v = &pool.members[m];
                let k = (v.gekisou_skill_id, v.gekisou_skill_level);
                if k.0 == 0 || member_gk.contains_key(&k) {
                    continue;
                }
                let row =
                    master.gekisou_skill(k.0).ok_or_else(|| Error::Master(format!("unknown Gekisou skill {}", k.0)))?;
                if !(1..=3).contains(&row.gekisou_mission_type) {
                    return Err(Error::Unsupported(format!("Gekisou mission type {}", row.gekisou_mission_type)));
                }
                let table = &master.gekisou_skill_effects;
                member_gk.insert(k, gekisou_rows(&env, table, RowSource::Gekisou, k.0, k.1, row.gekisou_mission_type)?);
            }
        }
        let all_rows = || {
            live.values()
                .flatten()
                .chain(snap_rows.iter().flatten().flatten())
                .chain(snap_gk_rows.iter().flatten().flatten())
                .chain(member_gk.values().flatten())
        };
        check_row_identities(all_rows())?;
        // Once a live effect is EXECUTING, a negative extension can move its
        // timed finish before its start. The resulting subtraction can make an
        // earlier note's factor negative, invalidating power representatives.
        if all_rows().any(|row| row.effect_type == 15000 && row.value < 0) {
            return Err(Error::Domain("negative live-skill duration extension".into()));
        }
        let command_floors = command_floor_times(setup, all_rows(), |r| env.range_start_only(r.trigger));
        // A conversion can only enlarge this closure. Relax all targets/conditions/windows;
        // use a transitive closure so chains of multiple converters cannot escape it.
        for r in all_rows().filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            let to = convert_to(r.effect_type, r.value);
            if (0..8).contains(&to) {
                for &from in &r.targets {
                    if (0..8).contains(&from) {
                        env.count_reach[from as usize] |= 1u8 << to;
                    }
                }
            }
        }
        for via in 0..8 {
            let reachable = env.count_reach[via];
            for from in 0..8 {
                if env.count_reach[from] & (1u8 << via) != 0 {
                    env.count_reach[from] |= reachable;
                }
            }
        }
        // life: the judgements each stream entry can reach (conversions of any allowed card registered when the
        // entry is judged), damage and recovery
        let frames: Vec<i32> = setup.play.frames.iter().map(|f| f.time_ms).collect();
        // with Gekisou on: the ranges' schedule, which does not depend on the deck
        let sched = match &setup.gk {
            None => None,
            Some(g) => Some(Schedule::new(master, setup, g)?),
        };
        if let Some(sc) = &sched {
            let mut g = GkFrames::new(sc, &frames, &entries);
            // Each physical member brings its own GK rows and at most one Snap.
            // Relax the Snap choice independently per member and then take the
            // five largest member capacities; no candidate is discarded here.
            let member_combo_rows: Vec<Vec<Row>> = members
                .iter()
                .map(|&m| {
                    let v = &pool.members[m];
                    member_gk.get(&(v.gekisou_skill_id, v.gekisou_skill_level)).cloned().unwrap_or_default()
                })
                .collect();
            let snap_combo_rows: Vec<Vec<Row>> =
                snap_gk_rows.iter().map(|per| per.iter().flatten().cloned().collect()).collect();
            // Ordinary controller bonus rows are not represented by the GK-only
            // capacities above. Fixed additions and unknown combo-effect kinds
            // also need their own proof.
            // 12004 only protects breaks, while 12006/13005 conversions already
            // participate in the whole-pool judgement closure used below.
            let combo_domain = all_rows().all(|r| match r.effect_type {
                12000 => r.gk,
                12001 | 12002 | 12003 | 12005 => false,
                _ => true,
            });
            let combo_bonus =
                combo_domain.then(|| combo_triggers::maximum_bonus(&member_combo_rows, &snap_combo_rows)).flatten();
            // Every COMBO count refinement uses integer 12000 increments. The
            // native stack uses binary32, whose upward rounding can cross a
            // combo-table threshold even when a relative score margin is used.
            // Share the exact-stack certificate with the 7005 timing bound;
            // without it this optional objective falls back to exact traversal.
            if sc.ranges.iter().any(|r| r.mission == MISSION_COMBO) && combo_bonus.is_none() {
                return Err(Error::Domain("Gekisou COMBO bonus arithmetic outside the certified integer range".into()));
            }
            if let Some(bonus) = combo_bonus {
                g.combo_triggers =
                    combo_triggers::ComboTriggers::compile(master, &g, &entries, &env.count_reach, bonus);
            }
            env.gkf = Some(Rc::new(g));
        } else {
            // Ordinary judgement counters read the same clock and entry stream without any mission gate.
            // Keep the actual Gekisou schedule absent: an empty range timeline supplies only the shared
            // count/start-time evidence, including backdated notes and the first two frames' open lower bound.
            let ordinary = Schedule { states: vec![Vec::new(); frames.len()], ranges: Vec::new() };
            env.gkf = Some(Rc::new(GkFrames::new(&ordinary, &frames, &entries)));
        }
        let fire: Vec<usize> = setup
            .events
            .iter()
            .filter(|e| (0..5).contains(&e.0))
            .filter_map(|e| {
                let i = frames.partition_point(|&x| x < e.1);
                (i < frames.len()).then_some(i)
            })
            .collect();
        let mut convs: Vec<(Conv, bool)> = Vec::new();
        let whole = vec![(i64::MIN, i64::MAX)];
        for r in live.values().flatten().filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            convs.push(((convert_to(r.effect_type, r.value), r.targets.clone(), whole.clone()), false));
        }
        let cond_rows = snap_rows.iter().flatten().flatten();
        let cond_rows = cond_rows.chain(snap_gk_rows.iter().flatten().flatten()).chain(member_gk.values().flatten());
        for r in cond_rows.filter(|r| matches!(r.effect_type, 12006 | 13005)) {
            let (w, budget) = if r.gk {
                let g = gk_row(&env, r);
                (g.conv.clone(), gk_budget(&env, r, &g).is_some())
            } else if r.release == 0 && env.event_only(r.trigger) {
                // registered in the start frame, it converts the notes judged in the next frames up to the frame that
                // processes its end
                (fire.iter().map(|&i0| (i0 as i64, register_end(&frames, i0, r.act))).collect(), false)
            } else {
                (whole.clone(), false)
            };
            convs.push(((convert_to(r.effect_type, r.value), r.targets.clone(), w), budget));
        }
        convs.retain(|c| c.0.0 != -1);
        // `all`: every conversion; else without the rows with a conversion budget (their score is bounded apart)
        let reach = |j: i32, t: i64, all: bool| -> Vec<i32> {
            let mut v = vec![j];
            for ((to, targets, w), budget) in &convs {
                if (all || !budget)
                    && *to != j
                    && targets.contains(&(j as i64))
                    && !v.contains(to)
                    && w.iter().any(|&(a, b)| a < t && t <= b)
                {
                    v.push(*to);
                }
            }
            v
        };
        let reached: Vec<Vec<i32>> = entries.iter().map(|&(fi, _, j)| reach(j, fi as i64, true)).collect();
        // Count conditions read the final judgement of each judged entry. Close every conversion registered for its
        // frame transitively; outside every registration window an entry keeps its raw judgement. The windows were
        // derived with the whole-pool closure, so they only over-approximate.
        env.entry_reach = entries
            .iter()
            .map(|&(fi, _, j)| {
                let t = fi as i64;
                let mut mask = if (0..8).contains(&j) { 1u8 << j } else { 0 };
                loop {
                    let mut next = mask;
                    for ((to, targets, w), _) in &convs {
                        if (0..8).contains(to)
                            && w.iter().any(|&(a, b)| a < t && t <= b)
                            && targets.iter().any(|&x| (0..8).contains(&x) && mask & (1u8 << x) != 0)
                        {
                            next |= 1u8 << to;
                        }
                    }
                    if next == mask {
                        break mask;
                    }
                    mask = next;
                }
            })
            .collect();
        // the judgements the score bounds read outside the conversion budgets
        let reached_v: Vec<Vec<i32>> = entries.iter().map(|&(fi, _, j)| reach(j, fi as i64, false)).collect();
        if let Some(sc) = &sched {
            env.gk = Some(GkEnv {
                missions: sc.ranges.iter().map(|r| r.mission).collect(),
                completes: sc.ranges.iter().any(|r| r.f_complete.is_some()),
                breaks: reached.iter().any(|r| r.iter().any(|&j| j == 1 || j == 2)),
            });
        }
        let mut damaging = false;
        for (e, r) in entries.iter().zip(&reached) {
            if !damage.contains_key(&(e.2 as i64)) {
                return Err(Error::Game(format!("judgement {} has no damage entry", e.2)));
            }
            if r.iter().any(|&x| damage.get(&(x as i64)).is_none_or(|&d| d > 0)) {
                damaging = true;
            }
        }
        // With ordinary damage, every life a note reads is at most max(0, base - filed damage). The filed damage
        // comes from entries already judged in earlier frames or earlier in the same frame, including the note
        // itself, with chart times up to the note's: damage only lowers life, a life query folds every filed
        // command up to its time at least once (the frame cache can fold some twice), and the floor is 0. The damage
        // of an entry is at least the smallest damage of its reachable judgements.
        let dmin: Vec<i64> = reached
            .iter()
            .map(|r| r.iter().map(|&x| damage.get(&(x as i64)).copied().unwrap_or(0).max(0)).min().unwrap_or(0))
            .collect();
        let mut tr: Vec<i32> = entries.iter().map(|e| e.1.time_ms).collect();
        tr.sort_unstable();
        tr.dedup();
        let mut bit = vec![0i64; tr.len() + 1];
        let mut dead_stream = Vec::with_capacity(entries.len());
        for (e, &d) in entries.iter().zip(&dmin) {
            let r = tr.partition_point(|&x| x < e.1.time_ms) + 1;
            let mut i = r;
            while i <= tr.len() {
                bit[i] = bit[i].saturating_add(d);
                i += i & i.wrapping_neg();
            }
            let (mut sum, mut i) = (0i64, r);
            while i > 0 {
                sum = sum.saturating_add(bit[i]);
                i -= i & i.wrapping_neg();
            }
            dead_stream.push(base.saturating_sub(sum) <= 0);
        }
        // The simulation recovers by the native 32-bit amount. It must not lower
        // the life, and below 2^30 its sum with life <= 2*base <= 2^30 cannot wrap.
        if all_rows().any(|r| r.effect_type == 3001 && !(0..1 << 30).contains(&(r.value as i32))) {
            return Err(Error::Domain("life recovery outside the modelled range".into()));
        }
        let recovery = all_rows().any(|r| r.effect_type == 3001 && (r.value as i32) > 0);
        env.life_lo = if damaging { 0 } else { base };
        env.life_hi = if recovery { 2 * base } else { base };
        let onus = settings.life_onus_factor;
        // life rigidity: every life condition any allowed card can ask is decided on [lo, hi]
        let mut groups: Vec<i64> = all_rows().flat_map(|r| [r.trigger, r.condition, r.release, r.reset]).collect();
        groups.sort_unstable();
        groups.dedup();
        let (lo, hi) = (env.life_lo, env.life_hi);
        let mut rigid = env.life_lo > 0 || onus == 1.0;
        for s in master.skill_condition_sets.iter().filter(|s| s.group != 0 && groups.binary_search(&s.group).is_ok()) {
            for &cid in &s.condition_ids {
                let Some(c) = master.skill_condition(cid) else { continue };
                let Some(&v) = c.condition_values.first() else { continue };
                let decided = match c.condition_type {
                    2001 => hi < v || lo >= v,
                    2003 => lo > v || hi <= v,
                    _ => true,
                };
                rigid &= decided;
            }
        }
        env.life_rigid = rigid;

        // monotonicity preconditions
        let positive = |x: f32| x.is_finite() && x > 0.0;
        let level = setup.params.music_level;
        let adj = settings.score_adjustment_factor;
        let mdf = get_music_score_level_factor(level);
        let cnc = setup.params.converted_note_count;
        let assist = setup.params.assist_factor;
        if !(positive(adj) && positive(mdf) && cnc > 0 && assist.is_finite() && assist >= 0.0)
            || onus.is_nan()
            || onus < 0.0
        {
            return Err(Error::Domain("live score settings are not all positive".into()));
        }
        if all_rows().any(|r| matches!(r.effect_type, 2000 | 2004) && r.value < 0) {
            return Err(Error::Domain("negative score factor".into()));
        }

        // classes
        let n = pool.members.len();
        let mut classes: Vec<Vec<Class>> = vec![Vec::new(); n];
        let mut class_of: Vec<Vec<u16>> = vec![Vec::new(); n];
        let mut member_live: Vec<Vec<LiveRow>> = vec![Vec::new(); n];
        let mut gids: HashMap<ClassKey, u32> = HashMap::new();
        let mut class_gid: Vec<Vec<u32>> = vec![Vec::new(); n];
        for &m in &members {
            let v = &pool.members[m];
            let a = attr(v);
            for r in &live[&(v.live_skill_id, v.live_skill_level)] {
                let o = env.group(r.condition, a)?;
                member_live[m].push(LiveRow { row: r.clone(), out: o, partner: None });
            }
            // rows whose conditions negate each other start at most one of the two at an event
            let lr = &mut member_live[m];
            for i in 0..lr.len() {
                for j in i + 1..lr.len() {
                    if lr[i].partner.is_none()
                        && lr[j].partner.is_none()
                        && lr[i].row.effect_type == lr[j].row.effect_type
                        && lr[i].row.act.to_bits() == lr[j].row.act.to_bits()
                        && env.negations(lr[i].row.condition, lr[j].row.condition)
                    {
                        lr[i].partner = Some(j);
                        lr[j].partner = Some(i);
                    }
                }
            }
            // the member's own Gekisou skill: the same for every snap, so it joins every class's rows (not its key)
            let mut own = Vec::new();
            if let Some(rs) = member_gk.get(&(v.gekisou_skill_id, v.gekisou_skill_level)) {
                for r in rs {
                    let (st, can_start, event_bound) = support_status(&env, r, a)?;
                    if st == Status::Active {
                        own.push(active_row(&env, r, can_start, event_bound)?);
                    }
                }
            }
            let has_gk = gk_on && v.gekisou_skill_id != 0;
            let mut keys: HashMap<ClassKey, usize> = HashMap::new();
            let mut cl = vec![Class { snaps: Vec::new(), rows: own.clone() }];
            keys.insert(Vec::new(), 0);
            let next = gids.len() as u32;
            let mut cg = vec![*gids.entry(Vec::new()).or_insert(next)];
            let mut of = Vec::with_capacity(t.snaps.len());
            for (j, per) in snap_rows.iter().enumerate() {
                let mut key: ClassKey = Vec::new();
                let mut rows = Vec::new();
                let gk_skills = if has_gk { &snap_gk_rows[j][..] } else { &[][..] };
                for (kind, skill) in per.iter().map(|x| (3, x)).chain(gk_skills.iter().map(|x| (5, x))) {
                    let mut sk = Vec::new();
                    for r in skill {
                        let (st, can_start, event_bound) = support_status(&env, r, a)?;
                        if st != Status::Active {
                            continue;
                        }
                        let cond = env.group(r.condition, a)?;
                        sk.push(RowSig {
                            trigger_type: r.trigger_type,
                            trigger: r.trigger,
                            condition: if cond.is_none_or(|c| c.decided_true()) { 0 } else { r.condition },
                            release: r.release,
                            reset: r.reset,
                            cumulative: r.cumulative,
                            effect_type: r.effect_type,
                            value: r.value,
                            act: r.act.to_bits(),
                            limit: r.limit,
                            execute_limit: r.execute_limit,
                            targets: r.targets.clone(),
                            max_value: if r.gk { r.max_value } else { 0 },
                        });
                        rows.push(active_row(&env, r, can_start, event_bound)?);
                    }
                    if !(sk.is_empty() || kind == 5 && ablated(ablate::CLASS_KEY)) {
                        key.push((kind, skill.first().map_or(0, |r| r.gate), sk));
                    }
                }
                let c = match keys.get(&key) {
                    Some(&c) => c,
                    None => {
                        rows.extend(own.iter().cloned());
                        cl.push(Class { snaps: Vec::new(), rows });
                        let next = gids.len() as u32;
                        cg.push(*gids.entry(key.clone()).or_insert(next));
                        keys.insert(key, cl.len() - 1);
                        cl.len() - 1
                    }
                };
                if cl.len() > u16::MAX as usize {
                    return Err(Error::Capacity("too many snap classes".into()));
                }
                cl[c].snaps.push(j);
                of.push(c as u16);
            }
            classes[m] = cl;
            class_of[m] = of;
            class_gid[m] = cg;
        }
        // Do not merge different members while live targets can also read tags and categories.
        // Member identity is a conservative key; completeness takes precedence over this reduction.
        let mut ids: HashMap<SimulationMemberKey, u32> = HashMap::new();
        let mut sim_id = vec![u32::MAX; n];
        for &m in &members {
            let v = &pool.members[m];
            let (gs, gl) = if gk_on { (v.gekisou_skill_id, v.gekisou_skill_level) } else { (0, 0) };
            let key = (v.live_skill_id, v.live_skill_level, v.band_id, v.card_type, v.character_id, gs, gl, m);
            let next = ids.len() as u32;
            sim_id[m] = *ids.entry(key).or_insert(next);
        }

        // per-entry coefficients, entries in chart-time order
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|&i| (entries[i].1.time_ms, i));
        let times_all: Vec<i32> = order.iter().map(|&i| entries[i].1.time_ms).collect();
        // times of the entries that break the combo whatever the conversions (Miss or Bad, nothing to reach)
        let breakers: Vec<i32> =
            order.iter().filter(|&&i| reached[i].iter().all(|&j| j < 3)).map(|&i| entries[i].1.time_ms).collect();
        let mut combo_max: Vec<f64> = Vec::new();
        let mut best_combo = 0f64;
        let life_f = if env.life_lo > 0 { 1.0 } else { (onus as f64).max(1.0) };
        let pool_life_up = all_rows().any(|r| matches!(r.effect_type, 3001 | 3003 | 3004));
        let mut coef = Coef::default();
        let adj64 = adj as f64;
        let mdf64 = mdf as f64;
        let mut pre: Vec<f64> = Vec::with_capacity(order.len());
        let mut pre_plain: Vec<f64> = Vec::with_capacity(order.len());
        // with a Gekisou combo range: each coefficient around its combo factor (see `carrier_level_envelopes`)
        let mut level_terms: Vec<(f64, f64, f64)> = Vec::new();
        // with Gekisou on: the Gekisou combo, luck and rank bonus factors of each entry (chart-time order)
        let gkf = match (&sched, &setup.gk) {
            (Some(sc), Some(_)) => {
                // the trigger kinds of the Gekisou score rows whose trigger time can precede the frame's
                let kinds = |ty: i64| {
                    all_rows().filter(|r| r.gk && matches!(r.effect_type, 2000 | 2001 | 2004)).any(|r| {
                        env.sets.get(&r.trigger).is_some_and(|v| {
                            v.iter()
                                .flat_map(|s| s.iter())
                                .any(|&c| master.skill_condition(c).is_some_and(|x| x.condition_type == ty))
                        })
                    })
                };
                let overrides = (kinds(7020), kinds(7005));
                // each allowed member's Gekisou combo bonus windows, one list per distinct list over its classes
                let member_cb: Vec<Vec<Vec<ComboBonusRow>>> = members
                    .iter()
                    .map(|&m| {
                        let mut lists: Vec<Vec<ComboBonusRow>> = Vec::new();
                        for c in &classes[m] {
                            let l = combo_windows(&c.rows);
                            if !l.is_empty() && !lists.contains(&l) {
                                lists.push(l);
                            }
                        }
                        lists
                    })
                    .collect();
                let current = &env.gkf.as_ref().expect("Gekisou frames").current;
                Some(GkFactors::new(
                    master,
                    setup,
                    sc,
                    &entries,
                    &order,
                    &frames,
                    &reached,
                    overrides,
                    current,
                    &member_cb,
                    &command_floors,
                )?)
            }
            _ => None,
        };
        for (pos, &i) in order.iter().enumerate() {
            let (_, n, _) = entries[i];
            // the combo a note reads counts the entries at earlier chart times since the last one that breaks it
            // (with Gekisou on, the combo a rank bonus reads can miss breaks judged later: counted from the start)
            let b = breakers.partition_point(|&x| x < n.time_ms);
            let from = if b == 0 || gkf.as_ref().is_some_and(|g| g.nobreak[pos]) {
                0
            } else {
                times_all.partition_point(|&x| x < breakers[b - 1])
            };
            let before = times_all.partition_point(|&x| x < n.time_ms) - from;
            while combo_max.len() <= before {
                let c = combo_max.len() as i32;
                let cum = combo.get_cumulative_factor(COMBO, c)?;
                let f = ournotes_sim::num::min_ignoring_nan(cum, 1f32) + 1f32;
                if f.is_nan() || f < 1.0 {
                    return Err(Error::Domain("combo bonus table is not non-negative".into()));
                }
                best_combo = best_combo.max(f as f64);
                combo_max.push(best_combo);
            }
            let note_pct = *settings
                .note_factor_percent
                .get(&n.note_operate_type)
                .ok_or_else(|| Error::Game(format!("note type {} has no score percent", n.note_operate_type)))?;
            if note_pct < 0 {
                return Err(Error::Domain("negative note score percent".into()));
            }
            let mut jp = [0f64; 4];
            let mut max_jp = 0f64;
            let mut vmask = 0u8;
            for &x in &reached_v[i] {
                vmask |= 1 << x;
                let st = convert_score_type(x as i64)?;
                let p = *settings
                    .judgement_score_factor_percent
                    .get(&st)
                    .ok_or_else(|| Error::Game(format!("score type {st} has no score percent")))?;
                if p < 0 {
                    return Err(Error::Domain("negative judgement score percent".into()));
                }
                let p = p as f64 / 100.0;
                max_jp = max_jp.max(p);
                if (3..=6).contains(&x) {
                    jp[(x - 3) as usize] = p;
                }
            }
            let mut pre_e = adj64 * mdf64 * (note_pct as f64 / 100.0);
            let mut plain_e = pre_e;
            let mut rank = 1.0;
            let mut gpool = 1.0;
            if let Some(g) = &gkf {
                if g.combo.is_some() {
                    // the candidate bound reads its own Gekisou combo factor
                    pre_e *= g.l[pos];
                    gpool = g.g[pos];
                } else {
                    plain_e *= g.g[pos];
                    pre_e *= g.g[pos] * g.l[pos];
                }
                rank = g.r[pos];
            }
            pre.push(pre_e);
            pre_plain.push(plain_e);
            if gkf.as_ref().is_some_and(|g| g.combo.is_some() && !g.carriers.is_empty()) {
                level_terms.push((pre_e, combo_max[before], rank));
            }
            let terminal = pre_e * gpool * combo_max[before] / cnc as f64;
            let k = terminal * rank;
            if setup.gk.as_ref().is_some_and(|g| g.setup.missions.contains(&MISSION_LUCK)) {
                coef.family_terminal.push([plain_e * gpool * combo_max[before] / cnc as f64, terminal]);
            }
            coef.times.push(n.time_ms);
            coef.k.push(k);
            coef.max_jp.push(max_jp);
            coef.jp.push(jp);
            coef.vmask.push(vmask);
            // Recovery, guard and damage reduction use the pool-wide life factor. Ordinary damage can establish
            // life zero at an entry from the damage already filed there.
            coef.z.push(
                if pool_life_up || setup.gk.as_ref().is_some_and(|g| g.confirmations.is_some()) || !dead_stream[i] {
                    assist as f64 * life_f
                } else {
                    assist as f64 * onus as f64
                },
            );
        }
        let ne = coef.times.len();
        coef.pc = vec![0f64; ne + 1];
        coef.pj = [vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1]];
        coef.pcd = vec![0f64; ne + 1];
        coef.pjd = [vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1], vec![0f64; ne + 1]];
        let z_dead = assist as f64 * onus as f64;
        for e in 0..ne {
            coef.pc[e + 1] = coef.pc[e] + coef.z[e] * coef.k[e] * coef.max_jp[e];
            coef.pcd[e + 1] = coef.pcd[e] + z_dead * coef.k[e] * coef.max_jp[e];
            for j in 0..4 {
                coef.pj[j][e + 1] = coef.pj[j][e] + coef.z[e] * coef.k[e] * coef.jp[e][j];
                coef.pjd[j][e + 1] = coef.pjd[j][e] + z_dead * coef.k[e] * coef.jp[e][j];
            }
        }
        let a0 = coef.pc[ne];
        let mut jp_of = [f64::INFINITY; 7];
        for (x, slot) in jp_of.iter_mut().enumerate().skip(1) {
            if let Ok(st) = convert_score_type(x as i64)
                && let Some(&p) = settings.judgement_score_factor_percent.get(&st)
                && p >= 0
            {
                *slot = p as f64 / 100.0;
            }
        }
        let mut fine = Fine {
            score_frames: ScoreFrames::new(&setup.params),
            rush_eligible: rush::eligible(&env, &entries),
            raw: order.iter().map(|&i| entries[i].2 as u8).collect(),
            group: Vec::with_capacity(ne),
            pre_plain: if gkf.as_ref().is_some_and(|g| g.l.iter().any(|&l| l != 1.0)) { pre_plain } else { Vec::new() },
            pre,
            cnc: cnc as f64,
            combo_max: combo_max.clone(),
            mjp: vec![0f64; 128],
            jp4: vec![[0f64; 4]; 128],
            breaks: vec![false; 128],
            src: vec![Vec::new(); n],
            extra: vec![Default::default()],
            extra_v: vec![Default::default()],
            budget: vec![Vec::new()],
            gcombo: gkf.as_ref().and_then(|g| g.combo.clone()),
            dead: order.iter().map(|&i| dead_stream[i]).collect(),
            rank: gkf.as_ref().map(|g| g.r.clone()).unwrap_or_default(),
            rank_ranges: gkf.as_ref().map(|g| g.ranks.clone()).unwrap_or_default(),
            network_ranking: setup.gk.as_ref().is_some_and(|g| g.confirmations.is_some()),
            nobreak: gkf.as_ref().map(|g| g.nobreak.clone()).unwrap_or_default(),
            z_dead: assist as f64 * onus as f64,
            life: vec![Vec::new(); n],
            base,
            #[cfg(feature = "search-diagnostics")]
            exec_profile: Vec::new(),
            slot_end: Vec::new(),
            slot_dmg: Vec::new(),
            slot_dmg_final: Vec::new(),
            life_rows_listed: !env.life_rigid,
            ev_slot: Default::default(),
            until: Vec::new(),
            until_min: Vec::new(),
            dead_from: 0,
        };
        for e in 0..ne {
            let g = if e > 0 && coef.times[e] == coef.times[e - 1] { fine.group[e - 1] } else { e as u32 };
            fine.group.push(g);
        }
        for mask in 0..128usize {
            for (x, &p) in jp_of.iter().enumerate().skip(1) {
                if (mask >> x) & 1 == 1 {
                    fine.mjp[mask] = fine.mjp[mask].max(p);
                    if x >= 3 {
                        fine.jp4[mask][x - 3] = p;
                    }
                }
            }
            fine.breaks[mask] = (mask & 0b111_1000) == 0;
        }
        // conversion sources: the conversions of each (member, class), and what each adds at each position
        let mut fire_k: [Vec<usize>; 5] = Default::default();
        for &(idx, time) in &setup.events {
            if (0..5).contains(&idx) {
                let i = frames.partition_point(|&x| x < time);
                if i < frames.len() {
                    fire_k[idx as usize].push(i);
                }
            }
        }
        // life bound of recovering candidates
        {
            let mut after = vec![i64::MAX; entries.len()];
            for i in (0..entries.len().saturating_sub(1)).rev() {
                after[i] = after[i + 1].min(entries[i + 1].1.time_ms as i64);
            }
            fine.until = order.iter().map(|&i| after[i]).collect();
            fine.until_min = fine.until.clone();
            for e in (0..fine.until_min.len().saturating_sub(1)).rev() {
                fine.until_min[e] = fine.until_min[e].min(fine.until_min[e + 1]);
            }
            fine.dead_from = fine.dead.len();
            while fine.dead_from > 0 && fine.dead[fine.dead_from - 1] {
                fine.dead_from -= 1;
            }
            // Life frames and the frame cache (see `docs/search.md`): a life query at life frame `q` leaves the cache
            // complete up to at most `q - 1`; a command filed at a life frame `f` up to the cache folds the frames from
            // `f` to the cache once more at the next query. Queries happen at each play frame's time and at each
            // judged entry's chart time; commands are the entries' damage (filed before that entry's query) and the
            // skill events' recoveries (at the time of the frame where the event fires, after that frame's entries).
            let lmax = get_frame(setup.params.music_length_ms).wrapping_add(2);
            let lf = |ms: i32| {
                let f = get_frame(ms);
                if lmax <= f { lmax.wrapping_sub(1) } else { f }
            };
            let pf: Vec<i32> = frames.iter().map(|&t| lf(t)).collect();
            // the windows `[f, q - 1]` that a command can fold again: `f` its life frame, `q` the largest life frame
            // queried before it is filed
            let mut windows: Vec<(i32, i32)> = Vec::new();
            let mut mq_note = i32::MIN;
            for e in &entries {
                let q = if e.0 == 0 { i32::MIN } else { pf[e.0 - 1] }.max(mq_note);
                let f = lf(e.1.time_ms);
                if f < q {
                    windows.push((f, q - 1));
                }
                mq_note = mq_note.max(f);
            }
            let mut note_upto = vec![i32::MIN; frames.len()];
            let (mut m, mut q) = (i32::MIN, 0usize);
            for (i, slot) in note_upto.iter_mut().enumerate() {
                while q < entries.len() && entries[q].0 <= i {
                    m = m.max(lf(entries[q].1.time_ms));
                    q += 1;
                }
                *slot = m;
            }
            for &i1 in fire_k.iter().flatten() {
                let q = pf[i1].max(note_upto[i1]);
                if pf[i1] < q {
                    windows.push((pf[i1], q - 1));
                }
            }
            windows.sort_unstable();
            let mut runs: Vec<(i32, i32)> = Vec::new();
            for &(a, b) in &windows {
                match runs.last_mut() {
                    Some(r) if a <= r.1 => r.1 = r.1.max(b),
                    _ => runs.push((a, b)),
                }
            }
            // slot key: (first life frame, time); a run is one slot, a time outside the runs is one slot
            let key = |ms: i32| -> (i32, i64) {
                let f = lf(ms);
                let r = runs.partition_point(|x| x.1 < f);
                match runs.get(r) {
                    Some(&(a, _)) if a <= f => (a, i64::MIN),
                    _ => (f, ms as i64),
                }
            };
            let end_of = |k: (i32, i64)| -> i64 {
                if k.1 != i64::MIN {
                    return k.1;
                }
                let b = runs[runs.partition_point(|x| x.0 < k.0)].1;
                if b >= lmax.wrapping_sub(1) { i64::MAX } else { 40 * b as i64 }
            };
            let mut keys: Vec<(i32, i64)> = entries.iter().map(|e| key(e.1.time_ms)).collect();
            keys.extend(fire_k.iter().flatten().map(|&i| key(frames[i])));
            keys.sort_unstable();
            keys.dedup();
            fine.slot_end = keys.iter().map(|&k| end_of(k)).collect();
            fine.slot_dmg = vec![0i64; keys.len()];
            fine.slot_dmg_final = vec![0i64; keys.len()];
            let last_frame = frames.last().copied().unwrap_or(i32::MIN);
            for (e, &d) in entries.iter().zip(&dmin) {
                let slot = keys.binary_search(&key(e.1.time_ms)).expect("slot of an entry");
                fine.slot_dmg[slot] = fine.slot_dmg[slot].saturating_add(d);
                if e.1.time_ms <= last_frame {
                    fine.slot_dmg_final[slot] = fine.slot_dmg_final[slot].saturating_add(d);
                }
            }
            for k in 0..5 {
                fine.ev_slot[k] = fire_k[k]
                    .iter()
                    .map(|&i| {
                        let slot = keys.binary_search(&key(frames[i])).expect("slot of a recovery");
                        let times = 1 + windows.iter().filter(|w| w.0 <= pf[i] && pf[i] <= w.1).count() as i64;
                        (slot, times)
                    })
                    .collect();
            }
        }
        let ent_frame: Vec<i64> = order.iter().map(|&i| entries[i].0 as i64).collect();
        let mut sources: HashMap<ConvSource, u32> = HashMap::new();
        for &m in &members {
            let live_conv: Vec<(i32, Vec<i64>)> = member_live[m]
                .iter()
                .filter(|x| matches!(x.row.effect_type, 12006 | 13005) && x.out.is_none_or(|o| o.yes))
                .map(|x| (convert_to(x.row.effect_type, x.row.value), x.row.targets.clone()))
                .filter(|c| c.0 != -1)
                .collect();
            let live_life = member_live[m]
                .iter()
                .any(|x| matches!(x.row.effect_type, 3001 | 3003 | 3004) && x.out.is_none_or(|o| o.yes));
            fine.life[m] = classes[m]
                .iter()
                .map(|c| {
                    let (mut up, mut other) = (0i64, live_life);
                    for r in c.rows.iter().filter(|r| r.can_start) {
                        match r.effect_type {
                            3001 if r.event_bound => up += (r.value as i32).max(0) as i64,
                            3001 | 3003 | 3004 => other = true,
                            _ => {}
                        }
                    }
                    if other {
                        LifeKind::Other
                    } else if up > 0 {
                        LifeKind::Recovery(up)
                    } else {
                        LifeKind::None
                    }
                })
                .collect();
            let mut per = Vec::with_capacity(classes[m].len());
            for c in &classes[m] {
                let snap_conv: Vec<SnapConv> = c
                    .rows
                    .iter()
                    .filter(|r| matches!(r.effect_type, 12006 | 13005) && r.can_start)
                    .map(|r| {
                        let to = convert_to(r.effect_type, r.value);
                        let b = r.budget.map(f64::to_bits);
                        (to, r.targets.clone(), r.act.to_bits(), r.event_bound, r.gk_conv.clone(), b)
                    })
                    .filter(|c| c.0 != -1)
                    .collect();
                if live_conv.is_empty() && snap_conv.is_empty() {
                    per.push(0);
                    continue;
                }
                let key = (live_conv.clone(), snap_conv);
                if let Some(&id) = sources.get(&key) {
                    per.push(id);
                    continue;
                }
                let id = fine.extra.len() as u32;
                let mut ex: [Vec<u8>; 5] = Default::default();
                let mut ev: [Vec<u8>; 5] = Default::default();
                let mut bud: Vec<(u8, f64, Vec<u32>)> =
                    key.1.iter().filter_map(|c| c.5.map(|b| (c.0 as u8, f64::from_bits(b), Vec::new()))).collect();
                for k in 0..5 {
                    let (mut out, mut out_v) = (Vec::with_capacity(ne), Vec::with_capacity(ne));
                    for e in 0..ne {
                        let (j, fi) = (fine.raw[e] as i32, ent_frame[e]);
                        let (mut mask, mut mask_v) = (0u8, 0u8);
                        for (to, tg) in &key.0 {
                            if *to != j && tg.contains(&(j as i64)) {
                                mask |= 1 << to;
                                mask_v |= 1 << to;
                            }
                        }
                        let mut b = 0usize;
                        for (to, tg, act, eb, gw, budget) in &key.1 {
                            let seen = match gw {
                                Some(w) => w.iter().any(|&(a, b)| a < fi && fi <= b),
                                None => {
                                    !eb || fire_k[k].iter().any(|&i0| {
                                        (i0 as i64) < fi && fi <= register_end(&frames, i0, f32::from_bits(*act))
                                    })
                                }
                            };
                            let hit = *to != j && tg.contains(&(j as i64)) && seen;
                            if hit {
                                mask |= 1 << to;
                            }
                            if budget.is_some() {
                                // a budget row's frames do not depend on the position
                                if hit && k == 0 {
                                    bud[b].2.push(e as u32);
                                }
                                b += 1;
                            } else if hit {
                                mask_v |= 1 << to;
                            }
                        }
                        out.push(mask);
                        out_v.push(mask_v);
                    }
                    ex[k] = out;
                    ev[k] = out_v;
                }
                fine.extra.push(ex);
                fine.extra_v.push(ev);
                fine.budget.push(bud);
                sources.insert(key, id);
                per.push(id);
            }
            fine.src[m] = per;
        }

        // events of each position
        let mut ev_by_k: [Vec<i32>; 5] = Default::default();
        for &(idx, time) in &setup.events {
            if (0..5).contains(&idx) {
                ev_by_k[idx as usize].push(time);
            }
        }
        let exec = Exec::new(setup, &coef.times, gkf.as_ref(), &command_floors);
        #[cfg(feature = "search-diagnostics")]
        {
            fine.exec_profile = exec.e.clone();
        }
        let snapshot_frame_limit = fine.network_ranking.then_some(fine.score_frames.last());
        let geo = Geo {
            frames: &frames,
            times: &coef.times,
            exec: &exec,
            music_length_ms: setup.params.music_length_ms,
            snapshot_frame_limit,
        };
        let mut contrib: Vec<Vec<[Contrib; 5]>> = vec![Vec::new(); n];
        // command and factor totals for the drift margin: per position, the largest over members and classes
        let mut cmd_k = [0f64; 5];
        let mut executions_k = [0f64; 5];
        let mut fac_k = [0f64; 5];
        let mut frame_norm_k = [0f64; 5];
        // Complete physical resource limits are an optional tightening for certified LUCK domains. Other
        // domains retain the original position relaxation, as do diagnostic class-key ablations.
        let mut factor_resources = (terminal_caps_admitted
            && gkf.is_some()
            && setup
                .gk
                .as_ref()
                .is_some_and(|g| g.confirmations.is_none() && g.setup.missions.contains(&MISSION_LUCK)))
        .then(|| factor_resources::FactorResources::new(t.snaps.len()))
        .flatten();
        let forward_clamp = setup.params.music_length_ms > 0
            && setup.play.frames.iter().all(|frame| frame.time_ms <= setup.params.music_length_ms)
            && setup.events.iter().all(|&(_, time)| time <= setup.params.music_length_ms);
        // with a combo range, every deck's combo count bounds: a combo ramp window adds its factor at them
        let pool_reads = match gkf.as_ref() {
            Some(GkFactors { combo: Some(gc), sums, .. }) => {
                sums.get(5).map(|sums| RampReads { gc, sums, times: &coef.times })
            }
            _ => None,
        };
        for &m in &members {
            let mut per = Vec::with_capacity(classes[m].len());
            for (class, c) in classes[m].iter().enumerate() {
                let mut arr: [Contrib; 5] = Default::default();
                for k in 0..5 {
                    let (w, cmds, fac, ops, spans, ramps, rush, ops_plain, cmds_plain) =
                        windows(&geo, k, &member_live[m], &c.rows, &ev_by_k[k]);
                    if let Some(resources) = &mut factor_resources
                        && fac.iter().any(|v| !v.is_finite() || *v < 0.0)
                    {
                        resources.invalidate();
                    }
                    let fac = fac.iter().copied().fold(0f64, f64::max);
                    let judge = w.iter().any(|x| x.judge.iter().any(|&j| j != 0.0));
                    cmd_k[k] = cmd_k[k].max(cmds);
                    executions_k[k] = executions_k[k].max(if ops.is_nan() { f64::INFINITY } else { ops });
                    fac_k[k] = fac_k[k].max(fac);
                    let frame_norm = if forward_clamp && spans.iter().all(|&(start, end, _)| start <= end) {
                        raw::certified_frame_peak(&spans, fine.score_frames).unwrap_or(f64::INFINITY)
                    } else {
                        fac
                    };
                    frame_norm_k[k] = frame_norm_k[k].max(frame_norm);
                    if let Some(resources) = &mut factor_resources {
                        resources.observe(
                            pool.members[m].character_id,
                            class == 0,
                            &c.snaps,
                            k,
                            [cmds, ops, fac, frame_norm],
                        );
                    }
                    let cb = if fine.gcombo.is_some() { combo_windows(&c.rows) } else { Vec::new() };
                    arr[k] = Contrib {
                        windows: w,
                        gain: 0.0,
                        judge,
                        ops,
                        ops_plain,
                        cmds,
                        cmds_plain,
                        fac,
                        spans,
                        budget: 0.0,
                        cb,
                        ramps,
                        rush,
                    };
                    arr[k].gain = window_gain(&arr[k], &coef.pc, &coef.pj, pool_reads);
                }
                per.push(arr);
            }
            contrib[m] = per;
        }
        // Conversion budgets in the linear bound: a conversion of entry `e` to `to` (not in the entry's reach of
        // `Coef`) adds at most `z k ((jp(to) - max_jp)^+ (1 + NF_e) + jp(to) JF_e)`, `NF_e` and `JF_e` the largest note
        // and `to` judgement factors any allowed performer adds at `e`, summed over the positions. A row adds at most
        // its budget's largest such terms over the entries it can convert.
        if fine.budget.iter().any(|b| !b.is_empty()) {
            let mut nfx = vec![0f64; ne];
            let mut jfx = [vec![0f64; ne], vec![0f64; ne], vec![0f64; ne], vec![0f64; ne]];
            let mut d = vec![[0f64; 5]; ne + 1];
            for k in 0..5 {
                let (mut nk, mut jk) =
                    (vec![0f64; ne], [vec![0f64; ne], vec![0f64; ne], vec![0f64; ne], vec![0f64; ne]]);
                for &m in &members {
                    for c in &contrib[m] {
                        let ws = &c[k].windows;
                        if ws.is_empty() {
                            continue;
                        }
                        for w in ws {
                            let (lo, hi) = (w.lo as usize, w.hi as usize);
                            d[lo][0] += w.note;
                            d[hi][0] -= w.note;
                            for j in 0..4 {
                                d[lo][j + 1] += w.judge[j];
                                d[hi][j + 1] -= w.judge[j];
                            }
                        }
                        let lo = ws.iter().map(|w| w.lo as usize).min().unwrap_or(0);
                        let hi = ws.iter().map(|w| w.hi as usize).max().unwrap_or(0);
                        let mut acc = [0f64; 5];
                        for e in lo..hi {
                            for (a, x) in acc.iter_mut().zip(&d[e]) {
                                *a += x;
                            }
                            nk[e] = nk[e].max(acc[0]);
                            for j in 0..4 {
                                jk[j][e] = jk[j][e].max(acc[j + 1]);
                            }
                        }
                        for x in d[lo..=hi].iter_mut() {
                            *x = [0f64; 5];
                        }
                    }
                }
                for e in 0..ne {
                    nfx[e] += nk[e].max(0.0);
                    for j in 0..4 {
                        jfx[j][e] += jk[j][e].max(0.0);
                    }
                }
            }
            let mut terms: Vec<f64> = Vec::new();
            let per_src: Vec<f64> = fine
                .budget
                .iter()
                .map(|rows| {
                    let mut sum = 0f64;
                    for (to, n, elig) in rows {
                        let to = *to as usize;
                        let jt = jp_of[to];
                        terms.clear();
                        for &e in elig {
                            let e = e as usize;
                            if coef.vmask[e] & (1 << to) != 0 {
                                continue;
                            }
                            let jf = if to >= 3 { jfx[to - 3][e] } else { 0.0 };
                            let add = (jt - coef.max_jp[e]).max(0.0) * (1.0 + nfx[e]) + jt * jf;
                            terms.push(coef.z[e] * coef.k[e] * add);
                        }
                        sum += top_sum(&mut terms, *n);
                    }
                    sum
                })
                .collect();
            for &m in &members {
                for (c, arr) in contrib[m].iter_mut().enumerate() {
                    let b = per_src[fine.src[m][c] as usize];
                    for x in arr.iter_mut() {
                        x.budget = b;
                        x.gain += b;
                    }
                }
            }
        }
        let mut global = a0;
        for k in 0..5 {
            let mut best = 0f64;
            for &m in &members {
                for c in &contrib[m] {
                    best = best.max(c[k].gain);
                }
            }
            global += best;
        }
        // Each position chooses one member/class. Independent maxima over those
        // choices bound every team's lifetime commands, command executions and factor norm.
        let e_max = exec.max as f64;
        let n_cmd = cmd_k.iter().fold(0.0f64, |sum, &x| (sum + x).next_up());
        let f_tot = fac_k.iter().fold(0.0f64, |sum, &x| (sum + x).next_up());
        let frame_norm = frame_norm_k.iter().fold(0.0f64, |sum, &x| (sum + x).next_up()).min(f_tot);
        let executions = executions_k.iter().fold(0.0f64, |sum, &x| (sum + x).next_up()).min((e_max * n_cmd).next_up());
        let position_limits = [n_cmd, executions, f_tot, frame_norm];
        let resource_limits = factor_resources.and_then(|resources| resources.finish());
        let (n_cmd, executions, f_tot, frame_norm) = match resource_limits {
            Some(limits) => {
                // Both assignments contain every legal completion. Intersect their independent universal
                // upper bounds; no probability scales the command history or its rounding error.
                let tighten = |i: usize, original: f64| original.min(limits.characters[i]).min(limits.snaps[i]);
                let commands = tighten(0, n_cmd);
                let executions = tighten(1, executions).min((e_max * commands).next_up());
                let factors = tighten(2, f_tot);
                let frame_norm = tighten(3, frame_norm).min(factors);
                (commands, executions, factors, frame_norm)
            }
            None => (n_cmd, executions, f_tot, frame_norm),
        };
        let drift = factor_drift(executions, n_cmd, frame_norm)
            .ok_or_else(|| Error::Domain("factor command count has no finite drift certificate".into()))?;
        // with Gekisou on, the chain also multiplies by the Gekisou combo and luck factors, each computed in binary32
        let chain_extra = if gk_on { GK_CHAIN_EPS } else { 0.0 };
        let eps = float_margin::with_chain(drift, chain_extra)
            .ok_or_else(|| Error::Domain("nonfinite factor drift certificate".into()))?;
        // The positive comparison chain bounds either sign of the factor state.
        // Representatives separately certify the observed lower endpoint when
        // the pool drift does not provide it.
        let normal = |v: f32, low: f32, high: f32| v.is_finite() && low <= v && v <= high;
        let post_normal = |v: f32| v == 0.0 || normal(v, 2f32.powi(-16), 2f32.powi(16));
        let normal_chain = normal(adj, 2f32.powi(-16), 2f32.powi(16))
            && normal(mdf, 2f32.powi(-8), 2f32.powi(8))
            && post_normal(assist)
            && post_normal(onus)
            && f_tot.is_finite()
            && (0.0..=65536.0).contains(&f_tot)
            && ((1.0 + f_tot).next_up() + drift).next_up() < 131072.0
            && settings
                .note_factor_percent
                .values()
                .chain(settings.judgement_score_factor_percent.values())
                .all(|p| (0..=1_000_000).contains(p));
        if !normal_chain {
            return Err(Error::Domain("live score chain is outside the finite normal certificate".into()));
        }
        let judgement_max =
            settings.judgement_score_factor_percent.values().copied().max().map(|p| (p as f64 / 100.0).next_up());
        let roundings = factor_roundings(executions, n_cmd);
        let additive = judgement_max.and_then(|j| factor_error_sensitivity(&coef, j)).map(|b| (roundings, b));
        let joint_additive =
            additive.and_then(|(roundings, b)| additive_joint_envelope(a0, global, eps, roundings, b, chain_extra));
        // Observe the existing decision after it has been made. Keep the source delta (eps), raw drift and
        // outward feedback alpha separate: they have different roles in the certificate.
        let feedback_alpha = (roundings * 2f64.powi(-24)).next_up();
        let sensitivity = additive.map(|(_, b)| b);
        let positive_factor_admitted = eps.is_finite() && (0.0..0.5).contains(&eps);
        let feedback_admitted = float_margin::amplification(roundings, 2f64.powi(-24)).is_some();
        let additive_refusal = {
            use crate::search::telemetry::AdditiveEnvelopeRefusal as Refusal;
            if joint_additive.is_some() {
                None
            } else if judgement_max.is_none() {
                Some(Refusal::JudgementSettings)
            } else if sensitivity.is_none() {
                Some(Refusal::Sensitivity)
            } else if [a0, global, eps, roundings, sensitivity.unwrap_or(0.0), chain_extra]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0)
            {
                Some(Refusal::InputDomain)
            } else if !positive_factor_admitted {
                Some(Refusal::PositiveFactor)
            } else if !feedback_admitted {
                Some(Refusal::RoundingFeedback)
            } else {
                Some(Refusal::NonfiniteOutput)
            }
        };
        let factor_diagnostics = crate::search::telemetry::FactorEnvelopeDiagnostics {
            maximum_command_executions: e_max,
            commands_by_position: cmd_k,
            executions_by_position: executions_k,
            factor_norm_by_position: fac_k,
            frame_norm_by_position: frame_norm_k,
            position_limits,
            character_limits: resource_limits.map(|limits| limits.characters),
            snap_limits: resource_limits.map(|limits| limits.snaps),
            commands: n_cmd,
            executions,
            factor_norm: f_tot,
            frame_norm,
            drift,
            delta_with_chain: eps,
            roundings,
            feedback_alpha,
            positive_factor_admitted,
            feedback_admitted,
            judgement_max,
            sensitivity,
            a0,
            global,
            chain_extra,
            admitted: joint_additive.is_some(),
            refusal: additive_refusal,
        };
        let carrier_levels = match gkf.as_ref() {
            Some(g @ GkFactors { combo: Some(gc), .. }) if !level_terms.is_empty() => {
                carrier_level_envelopes(
                    &coef,
                    &level_terms,
                    cnc as f64,
                    &g.g,
                    &g.carriers,
                    gc,
                    &g.sums,
                    &members,
                    &contrib,
                )
                .into_iter()
                .map(|level| {
                    let mut level = level?;
                    if joint_additive.is_some() {
                        // a deck of the level reads coefficients at most the level's, so its factor error
                        // sensitivity is at most theirs; every input is at most the pool-wide one, whose
                        // envelope exists
                        let (roundings, pool_b) = additive.expect("pool-wide additive envelope");
                        let level_coef = Coef { k: level.k.clone(), z: coef.z.clone(), ..Default::default() };
                        let b = factor_error_sensitivity(&level_coef, judgement_max.expect("judgement factors"))
                            .expect("level sensitivity at most the pool-wide one")
                            .min(pool_b);
                        let (a, top, _) =
                            additive_joint_envelope(level.a0, level.global, eps, roundings, b, chain_extra)
                                .expect("level envelope at most the pool-wide one");
                        (level.a0, level.global) = (a, top);
                    }
                    Some(level)
                })
                .collect()
            }
            _ => Vec::new(),
        };
        let carrier_keys = match (gkf.as_ref(), &fine.gcombo) {
            (Some(g), Some(gc)) if !level_terms.is_empty() => {
                let mut levels = g.carriers.clone();
                levels.push(g.g.clone());
                let additive = joint_additive.map(|_| {
                    let (roundings, _) = additive.expect("pool-wide additive envelope");
                    let mut cmd_top = cmd_k;
                    let mut fac_top = fac_k;
                    cmd_top.sort_by(|a, b| b.total_cmp(a));
                    fac_top.sort_by(|a, b| b.total_cmp(a));
                    KeyedDrift {
                        roundings,
                        judgement_max: judgement_max.expect("judgement factors"),
                        chain_extra,
                        e_max,
                        execution_limit: executions,
                        command_limit: n_cmd,
                        factor_limit: frame_norm,
                        cmd_top,
                        fac_top,
                    }
                });
                Some(Rc::new(CarrierKeys::new(
                    gc,
                    &coef,
                    level_terms.clone(),
                    cnc as f64,
                    levels,
                    g.sums.clone(),
                    &members,
                    &class_of,
                    &contrib,
                    additive,
                )))
            }
            _ => None,
        };
        // the class search bounds life only when some entry can read life 0 (when the fold without recoveries never
        // reaches 0, no fold with recoveries does) at a factor below `Coef::z`
        let life_bound = !fine.network_ranking
            && env.life_lo <= 0
            && (fine.dead_from < coef.times.len() || fine.zero_from([0; 5]) != i64::MAX)
            && (0..coef.times.len()).any(|e| fine.z_dead < coef.z[e]);
        let split = if life_bound { split_envelopes(&contrib, &fine, &coef) } else { Vec::new() };
        let split_budget = if life_bound {
            (0..n * 5)
                .map(|mk| {
                    let (m, k) = (mk / 5, mk % 5);
                    contrib[m]
                        .iter()
                        .enumerate()
                        .filter(|(c, _)| fine.life[m][*c] != LifeKind::Other)
                        .map(|(_, a)| a[k].budget)
                        .fold(0f64, f64::max)
                })
                .collect()
        } else {
            Vec::new()
        };
        let first_draw = match &sched {
            None => 0,
            Some(sc) => sc.first_draw(&env, all_rows()),
        };
        let sl = SnapLive {
            master,
            setup,
            classes,
            class_of,
            terminal_caps_admitted,
            contrib,
            sim_id,
            class_gid,
            coef,
            fine,
            a0,
            global,
            life_bound,
            split,
            split_budget,
            eps,
            joint_additive,
            factor_diagnostics,
            carrier_levels,
            carrier_keys,
            chain_extra,
            n: setup.gk.as_ref().map_or(1, |g| g.seeds.len() as i64),
            prefix_frame: first_draw,
        };
        // Absolute scores include the asymmetric rounding of a negative note:
        // the first floor contributes at most one post-floor unit and the final
        // floor at most one integer unit. Rank multipliers cover these units at
        // every score snapshot as well as at the terminal score.
        let mut u: Vec<i64> = members
            .iter()
            .map(|&m| t.a[m] + t.lead.iter().map(|row| row[m]).max().unwrap_or(0).max(0) + t.wmax[m])
            .collect();
        u.sort_unstable_by(|x, y| y.cmp(x));
        let p_max: i64 = u.iter().take(5).sum();
        let chain = float_margin::with_chain(0.0, 0.0).expect("finite post-floor chain");
        let rounding_reserve = if sl.eps < 0.5 {
            0.0
        } else {
            sl.coef.z.iter().enumerate().fold(0.0f64, |sum, (entry, &post)| {
                let rank = sl.fine.rank.get(entry).copied().unwrap_or(1.0);
                let rank = if gk_on && sl.eps >= 1.0 { (2.0 * rank).next_up() } else { rank };
                let unit = ((post * (1.0 + chain).next_up()).next_up() + 1.0).next_up();
                (sum + (unit * rank).next_up()).next_up()
            })
        };
        let absolute_cap = (ub(p_max, sl.global, sl.eps) as f64 + rounding_reserve).next_up();
        if !absolute_cap.is_finite() || absolute_cap >= (i32::MAX as f64 / 2.0).next_down() {
            return Err(Error::Domain("live score bound exceeds the 32-bit range".into()));
        }
        Ok(sl)
    }
}

#[cfg(test)]
#[path = "terminal_cap_tests.rs"]
mod terminal_cap_tests;
