//! Candidate LUCK replay masks refine only direct positive Rush triggers.
use super::*;

/// Possible values of the direct 7021 checker at the skill phase, for gates1..4.
/// Each vector follows the declared play-frame order. The replay supplies a union
/// over every admitted uncertain branch; a failed replay supplies no masks.
pub(crate) type RushMasks = Rc<ournotes_sim::live::full::RushMasks>;

#[derive(Debug)]
pub(super) struct RushSpec {
    frames: Rc<GkFrames>,
    pub(super) gate: i64,
    trigger_type: i64,
    act: f32,
    release: i64,
    released_on_complete: bool,
}

#[derive(Clone, Debug)]
pub(super) struct RushRef {
    pub(super) spec: Rc<RushSpec>,
    pub(super) note: f64,
    pub(super) judge: [f64; 4],
    pub(super) run_cap: bool,
    pub(super) ops: f64,
    pub(super) ops_per_run: f64,
    pub(super) cmds: f64,
    pub(super) cmds_per_run: f64,
    pub(super) max_runs: f64,
}

impl RushRef {
    pub(super) fn counts(&self, masks: Option<&RushMasks>) -> (f64, f64) {
        let Some(masks) = masks.filter(|m| self.run_cap && self.spec.valid(m)) else {
            return (self.ops, self.cmds);
        };
        let runs = (masks.max_runs[(self.spec.gate - 1) as usize] as f64).next_up().min(self.max_runs);
        ((self.ops_per_run * runs).next_up().min(self.ops), (self.cmds_per_run * runs).next_up().min(self.cmds))
    }
}

/// For an untimed sustained fixed-factor row, a formation condition is constant
/// and cannot cause a second activation inside one direct-Rush true run. Keeping
/// releases absent avoids extending this command certificate to other lifecycles.
pub(super) fn run_cap_eligible(env: &Env, r: &Row) -> bool {
    r.trigger_type == 2
        && !has_activation_time(r.act)
        && r.release == 0
        && matches!(r.effect_type, 2000 | 2004)
        && (r.condition == 0
            || env.sets.get(&r.condition).is_some_and(|sets| {
                sets.iter().flat_map(|set| set.iter()).all(|&cid| {
                    env.master.skill_condition(cid).is_some_and(|c| matches!(c.condition_type, 0 | 3000 | 3001 | 5000))
                })
            }))
}

/// A skipped 7021 checker can retain a stale true flag. An always-observed probe
/// therefore cannot bound a Rush checker buried in And/Or, nor its negation.
/// Admit exactly one set containing exactly one positive7021. For that form the
/// updater checks its trigger on every open-gate frame, before pool/limit checks.
fn direct_trigger(master: &Master, sets: &[&[i64]]) -> bool {
    sets.len() == 1
        && sets[0].len() == 1
        && master.skill_condition(sets[0][0]).is_some_and(|c| c.condition_type == 7021 && c.is_positive)
}

pub(super) fn spec(env: &Env, r: &Row) -> Option<Rc<RushSpec>> {
    if !r.gk || !matches!(r.effect_type, 2000 | 2004) || !(1..=4).contains(&r.gate) {
        return None;
    }
    let key = (r.trigger, r.trigger_type, r.gate, r.act.to_bits(), r.release);
    if let Some(found) = env.rush_cache.borrow().get(&key) {
        return found.clone();
    }
    let result = (|| {
        if !direct_trigger(env.master, env.sets.get(&r.trigger)?)
            || !matches!(r.trigger_type, 1 | 2)
            || (r.trigger_type == 2 && has_activation_time(r.act))
        {
            return None;
        }
        Some(Rc::new(RushSpec {
            frames: env.gkf.as_ref()?.clone(),
            gate: r.gate,
            trigger_type: r.trigger_type,
            act: r.act,
            release: r.release,
            released_on_complete: GkFrames::released_on_complete(env, r),
        }))
    })();
    env.rush_cache.borrow_mut().insert(key, result.clone());
    result
}

/// The raw-judgement replay is valid only when all permitted conversions preserve
/// the controller's LUCK input semantics. In particular0/7 skip judge(), whereas
/// -1/1 may consume a pending lottery, despite both having zero base points.
pub(super) fn eligible(env: &Env, entries: &[(usize, LiveNote, i32)]) -> bool {
    let Some(g) = env.gkf.as_ref() else { return false };
    if !g.ranges.iter().any(|r| r.mission == MISSION_LUCK) {
        return false;
    }
    entries.iter().enumerate().all(|(i, &(_, note, raw))| {
        if !g.ranges.iter().any(|r| r.mission == MISSION_LUCK && r.start <= note.time_ms && note.time_ms <= r.end) {
            return true;
        }
        // Negative judgements are not represented by the conversion reach mask.
        let Some(&mask) = env.entry_reach.get(i).filter(|&&m| m != 0) else { return false };
        same_luck_class(raw, mask)
    })
}

fn same_luck_class(raw: i32, mask: u8) -> bool {
    let Some(raw_class) = ournotes_sim::live::full::luck_judgement_class(raw) else { return false };
    (0..8).contains(&raw)
        && mask & (1 << raw) != 0
        && (0..8)
            .filter(|&j| mask & (1 << j) != 0)
            .all(|j| ournotes_sim::live::full::luck_judgement_class(j) == Some(raw_class))
}

impl RushSpec {
    fn valid(&self, masks: &RushMasks) -> bool {
        masks.flags.iter().all(|m| m.len() == self.frames.times.len())
    }

    pub(super) fn spans(&self, masks: &RushMasks) -> Option<Vec<(i64, i64, f64)>> {
        let g = &self.frames;
        let nf = g.times.len();
        if !self.valid(masks) {
            return None;
        }
        let possible = &masks.flags[(self.gate - 1) as usize];
        let starts: Vec<_> = (0..nf).filter(|&f| possible[f] && g.gate_open(self.gate, f)).collect();
        let mut next_false = vec![None; nf];
        let mut next = None;
        for f in (0..nf).rev() {
            next_false[f] = next;
            if g.gate_open(self.gate, f) && !possible[f] {
                next = Some(f);
            }
        }
        let mut spans = Vec::with_capacity(starts.len());
        for f in starts {
            let end = if self.trigger_type == 2 {
                // Untimed sustained effects have at most one active updater and
                // must end at the first open-gate frame where Rush is certainly
                // false. Conditions/releases can only remove activity earlier.
                next_false[f].map_or(i64::MAX, |i| g.times[i] as i64)
            } else {
                let timed = if self.act > 0.0 || self.act.is_nan() || self.release == 0 {
                    frame_end(&g.times, g.times[f], f, self.act)
                } else {
                    i64::MAX
                };
                if self.release != 0 && self.released_on_complete {
                    // Release is first queried in the second frame after start.
                    timed.min(g.completion_from(f + 2).map_or(i64::MAX, |i| g.times[i] as i64))
                } else if self.release != 0 && self.act <= 0.0 {
                    i64::MAX
                } else {
                    timed
                }
            };
            // A direct7021 has no trigger-time override. Unlike a generic count
            // or range trigger, its factor cannot be backdated before this frame.
            let begin = g.times[f] as i64;
            if begin < end {
                spans.push((begin, end));
            }
        }
        Some(envelope(spans, self.trigger_type == 2))
    }
}

fn envelope(mut spans: Vec<(i64, i64)>, sustained: bool) -> Vec<(i64, i64, f64)> {
    if sustained {
        spans.sort_unstable();
        let mut union: Vec<(i64, i64, f64)> = Vec::new();
        for (a, b) in spans {
            match union.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => union.push((a, b, 1.0)),
            }
        }
        union
    } else if spans.len() <= 8 {
        spans.into_iter().map(|(a, b)| (a, b, 1.0)).collect()
    } else {
        let a = spans.iter().map(|s| s.0).min().expect("nonempty spans");
        let b = spans.iter().map(|s| s.1).max().expect("nonempty spans");
        vec![(a, b, POOL)]
    }
}

pub(super) type EntryWindows = Vec<(u32, u32, f64)>;
/// Entry windows by (spec, masks) pointer identity, with its use for telemetry.
#[derive(Default)]
pub(super) struct WindowCache {
    map: HashMap<(usize, usize), CachedWindows>,
    pub(super) usage: crate::search::telemetry::CacheUse,
}
pub(super) struct CachedWindows {
    // Retain both allocations while pointer identities are cache keys; a reused
    // address must never retrieve windows compiled for an earlier mask/spec.
    _spec: Rc<RushSpec>,
    _masks: RushMasks,
    windows: Rc<EntryWindows>,
}

pub(super) fn cached_windows(
    cache: &mut WindowCache,
    spec: &Rc<RushSpec>,
    masks: &RushMasks,
    times: &[i32],
) -> Option<Rc<EntryWindows>> {
    let key = (Rc::as_ptr(spec) as usize, Rc::as_ptr(masks) as usize);
    cache.usage.lookups += 1;
    if let Some(found) = cache.map.get(&key) {
        cache.usage.hits += 1;
        return Some(found.windows.clone());
    }
    let windows: Rc<EntryWindows> = Rc::new(
        spec.spans(masks)?
            .into_iter()
            .filter_map(|(a, b, mult)| {
                let lo = times.partition_point(|&t| (t as i64) < a);
                let hi = times.partition_point(|&t| (t as i64) < b);
                (lo < hi).then_some((lo as u32, hi as u32, mult))
            })
            .collect(),
    );
    if cache.map.len() >= 1024 {
        cache.usage.evictions += cache.map.len() as u64;
        cache.map.clear();
    }
    cache.map.insert(key, CachedWindows { _spec: spec.clone(), _masks: masks.clone(), windows: windows.clone() });
    cache.usage.peak_entries = cache.usage.peak_entries.max(cache.map.len());
    Some(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(flags: Vec<bool>) -> RushMasks {
        let runs = flags.iter().enumerate().filter(|(i, v)| **v && (*i == 0 || !flags[*i - 1])).count() as u64;
        Rc::new(ournotes_sim::live::full::RushMasks {
            flags: std::array::from_fn(|_| flags.clone()),
            max_runs: [runs; 4],
            spans: None,
        })
    }

    fn frames(gate: Vec<bool>) -> Rc<GkFrames> {
        let nf = gate.len();
        Rc::new(GkFrames {
            times: (0..nf).map(|i| i as i32 * 40).collect(),
            gate: [vec![false; nf], gate, vec![false; nf]],
            current: vec![None; nf],
            start: Vec::new(),
            complete: vec![false; nf],
            ranges: Vec::new(),
            states: vec![Vec::new(); nf],
            wlo: vec![-1000; nf],
            next_complete: vec![None; nf + 3],
            ent: Vec::new(),
            combo_triggers: None,
        })
    }

    #[test]
    fn singleton_admission_rejects_negation_and_short_circuit_groups() {
        let master = Master::from_json_tables(|name| {
            (name == "MasterSkillCondition").then_some(
                r#"{"_allData":[{"_id":1,"_conditionType":7021,"_isPositive":true},
               {"_id":2,"_conditionType":7021,"_isPositive":false},
               {"_id":3,"_conditionType":5000,"_isPositive":true}]}"#,
            )
        })
        .unwrap();
        assert!(direct_trigger(&master, &[&[1]]));
        assert!(!direct_trigger(&master, &[&[2]]));
        assert!(!direct_trigger(&master, &[&[3, 1]]));
        assert!(!direct_trigger(&master, &[&[1], &[3]]));
        assert!(!direct_trigger(&master, &[&[]]));
    }

    #[test]
    fn sustained_union_preserves_closed_gate_stickiness_without_backdating() {
        let spec = RushSpec {
            frames: frames(vec![true, true, false, false, true, true, true]),
            gate: 2,
            trigger_type: 2,
            act: 0.0,
            release: 0,
            released_on_complete: false,
        };
        let flags = vec![false, true, false, false, false, true, false];
        let masks = mask(flags);
        assert_eq!(spec.spans(&masks).unwrap(), vec![(40, 160, 1.0), (200, 240, 1.0)]);
    }

    #[test]
    fn one_shot_duration_continues_after_rush_turns_off_and_keeps_overlap() {
        let spec = RushSpec {
            frames: frames(vec![true; 8]),
            gate: 2,
            trigger_type: 1,
            act: 0.12,
            release: 0,
            released_on_complete: false,
        };
        let flags = vec![false, true, true, false, false, false, false, false];
        let masks = mask(flags);
        assert_eq!(spec.spans(&masks).unwrap(), vec![(40, 160, 1.0), (80, 200, 1.0)]);
        assert_eq!(envelope((0..9).map(|i| (i, i + 2)).collect(), false), vec![(0, 10, POOL)]);
    }

    #[test]
    fn invalid_mask_shape_falls_back_instead_of_erasing_wide_windows() {
        let spec = RushSpec {
            frames: frames(vec![true; 3]),
            gate: 2,
            trigger_type: 2,
            act: 0.0,
            release: 0,
            released_on_complete: false,
        };
        assert!(spec.spans(&mask(vec![false; 2])).is_none());
    }

    #[test]
    fn reach_certificate_distinguishes_pending_lottery_consumption() {
        assert!(same_luck_class(5, (1 << 5) | (1 << 6)));
        assert!(same_luck_class(0, (1 << 0) | (1 << 7)));
        assert!(!same_luck_class(1, (1 << 0) | (1 << 1)));
        assert!(!same_luck_class(4, (1 << 4) | (1 << 5)));
        assert!(!same_luck_class(-1, 0));
        assert!(!same_luck_class(5, 1 << 6));
    }

    #[test]
    fn cached_masks_keep_distinct_windows_and_half_open_entry_edges() {
        let spec = Rc::new(RushSpec {
            frames: frames(vec![true; 5]),
            gate: 2,
            trigger_type: 2,
            act: 0.0,
            release: 0,
            released_on_complete: false,
        });
        let masks = mask(vec![false, true, true, false, false]);
        let other = mask(vec![false; 5]);
        let mut cache = WindowCache::default();
        let times = [39, 40, 80, 119, 120];
        assert_eq!(&*cached_windows(&mut cache, &spec, &masks, &times).unwrap(), &vec![(1, 4, 1.0)]);
        assert!(cached_windows(&mut cache, &spec, &other, &times).unwrap().is_empty());
        assert_eq!(&*cached_windows(&mut cache, &spec, &masks, &times).unwrap(), &vec![(1, 4, 1.0)]);
        assert_eq!((cache.usage.lookups, cache.usage.hits, cache.usage.peak_entries), (3, 1, 2));
    }

    #[test]
    fn command_margin_uses_branch_run_count_not_union_components() {
        let row = RushRef {
            spec: Rc::new(RushSpec {
                frames: frames(vec![true; 8]),
                gate: 2,
                trigger_type: 2,
                act: 0.0,
                release: 0,
                released_on_complete: false,
            }),
            note: 1.4,
            judge: [0.0; 4],
            run_cap: true,
            ops: 80.0,
            ops_per_run: 10.0,
            cmds: 16.0,
            cmds_per_run: 2.0,
            max_runs: 8.0,
        };
        let masks = Rc::new(ournotes_sim::live::full::RushMasks {
            flags: std::array::from_fn(|_| vec![true; 8]),
            max_runs: [3; 4],
            spans: None,
        });
        let (ops, cmds) = row.counts(Some(&masks));
        assert!((30.0..31.0).contains(&ops));
        assert!((6.0..7.0).contains(&cmds));
        assert_eq!(row.counts(None), (80.0, 16.0));
    }
}
