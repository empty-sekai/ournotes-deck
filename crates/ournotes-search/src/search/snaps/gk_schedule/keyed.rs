//! The envelopes keyed by the Gekisou combo carriers a prefix placed.

use super::*;

/// What the Gekisou combo carriers a search prefix placed tell the linear envelope of its completions. With carriers
/// of the window lists `S` placed and `r` slots to fill, a completion's combo factor is at most `keyed_factors(S, r)`
/// and at most the count level `|S| + r`'s; the envelope rebuilt with the smaller of the two (as
/// `carrier_level_envelopes`) bounds its score. The bounds read its `A0` and the gains of the placed slots; the slots to
/// fill keep the count level's gains, which are at least these (every coefficient is).
pub(crate) struct CarrierKeys {
    gc: GkCombo,
    member_cb: Vec<Vec<Vec<ComboBonusRow>>>,
    /// The distinct carrier window lists; the list of every pool member and choice (0 = None, `j + 1` = Snap `j`).
    lists: Vec<Vec<ComboBonusRow>>,
    list_of: Vec<Vec<Option<u16>>>,
    /// The class of every pool member and choice, and the windows, combo ramps and conversion budget of every class by
    /// position.
    class_of: Vec<Vec<usize>>,
    windows: Vec<Vec<ClassWindows>>,
    /// Times, `z`, `max_jp` and `jp` of the entries, with `terms` and `cnc` as in `carrier_level_envelopes`.
    coef: Coef,
    terms: Vec<(f64, f64, f64)>,
    cnc: f64,
    /// The combo factor bound of decks with at most `n` carriers, `n` in 0..=5 (5: every deck).
    levels: Vec<Vec<f64>>,
    /// The combo count bounds of the same decks (`GkFactors::sums`).
    level_sums: Vec<Vec<f64>>,
    /// With the additive drift envelope, what its offset reads; the factor commands and their largest factor of every
    /// pool member and choice at any position.
    additive: Option<KeyedDrift>,
    commands: Vec<Vec<(f64, f64)>>,
    /// The envelopes built so far by sorted placed lists and slots to fill (at most `KEYED_CACHE`).
    cache: RefCell<HashMap<KeyedKey, Rc<KeyedEnvelope>>>,
}

/// The windows, combo ramps and conversion budget of a member's class at each position (the rest default).
type ClassWindows = [Contrib; 5];

/// A keyed envelope's key: the sorted placed lists (zero-padded), their count and the slots to fill.
type KeyedKey = ([u16; 5], usize, usize);

/// The most keyed envelopes kept at a time; a search visits few placed carrier lists below each prefix, so dropping
/// them all when full rebuilds few.
const KEYED_CACHE: usize = 256;

/// What the additive drift offset of a keyed envelope reads (see `SnapLive::joint_additive`): the pool-wide rounding
/// count, the largest judgement factor, the chain allowance, the frame executions and the per-position command and
/// factor maxima in descending order.
pub(crate) struct KeyedDrift {
    pub(crate) roundings: f64,
    pub(crate) judgement_max: f64,
    pub(crate) chain_extra: f64,
    pub(crate) e_max: f64,
    /// Every prefix's legal completions also satisfy these pool-wide limits.
    pub(crate) execution_limit: f64,
    pub(crate) command_limit: f64,
    pub(crate) factor_limit: f64,
    pub(crate) cmd_top: [f64; 5],
    pub(crate) fac_top: [f64; 5],
}

impl KeyedDrift {
    pub(super) fn drift(&self, n_cmd: f64, f_tot: f64) -> Option<f64> {
        let commands = n_cmd.min(self.command_limit);
        let executions = (self.e_max * commands).next_up().min(self.execution_limit);
        factor_drift(executions, commands, f_tot.min(self.factor_limit))
    }
}

/// The envelope of the completions of one placed carrier lists and count of slots to fill (see `CarrierKeys`): `A0`
/// without the drift offset and the factor error sensitivity of its coefficients.
pub(crate) struct KeyedEnvelope {
    a0: f64,
    sensitivity: f64,
    pc: Vec<f64>,
    pj: [Vec<f64>; 4],
    /// The combo count bounds (`GkCombo::fill`'s sums).
    sums: Vec<f64>,
    gains: RefCell<HashMap<(usize, usize), [f64; 5]>>,
}

impl KeyedEnvelope {
    /// The approximate bytes the envelope keeps.
    pub(crate) fn bytes(&self) -> usize {
        let values = self.pc.len() + self.pj.iter().map(Vec::len).sum::<usize>() + self.sums.len();
        values * std::mem::size_of::<f64>()
            + self.gains.borrow().len() * std::mem::size_of::<((usize, usize), [f64; 5])>()
    }
}

impl CarrierKeys {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::search::snaps) fn new(
        gc: &GkCombo,
        coef: &Coef,
        terms: Vec<(f64, f64, f64)>,
        cnc: f64,
        levels: Vec<Vec<f64>>,
        level_sums: Vec<Vec<f64>>,
        members: &[usize],
        class_of: &[Vec<u16>],
        contrib: &[Vec<[Contrib; 5]>],
        additive: Option<KeyedDrift>,
    ) -> CarrierKeys {
        let mut lists: Vec<Vec<ComboBonusRow>> = Vec::new();
        let mut list_of = vec![Vec::new(); contrib.len()];
        let mut choice_class = vec![Vec::new(); contrib.len()];
        let mut windows = vec![Vec::new(); contrib.len()];
        let mut commands = vec![Vec::new(); contrib.len()];
        let mut member_cb = Vec::with_capacity(members.len());
        for &m in members {
            let classes: Vec<usize> = std::iter::once(0).chain(class_of[m].iter().map(|&c| c as usize)).collect();
            let mut own: Vec<Vec<ComboBonusRow>> = Vec::new();
            list_of[m] = classes
                .iter()
                .map(|&c| {
                    let cb = &contrib[m][c][0].cb;
                    if cb.is_empty() {
                        return None;
                    }
                    if !own.contains(cb) {
                        own.push(cb.clone());
                    }
                    let id = lists.iter().position(|l| l == cb).unwrap_or_else(|| {
                        lists.push(cb.clone());
                        lists.len() - 1
                    });
                    Some(u16::try_from(id).expect("at most 65536 distinct carrier lists"))
                })
                .collect();
            member_cb.push(own);
            commands[m] = classes
                .iter()
                .map(|&c| {
                    let arr = &contrib[m][c];
                    (arr.iter().map(|x| x.cmds).fold(0f64, f64::max), arr.iter().map(|x| x.fac).fold(0f64, f64::max))
                })
                .collect();
            choice_class[m] = classes;
            windows[m] = contrib[m]
                .iter()
                .map(|arr| {
                    arr.each_ref().map(|x| Contrib {
                        windows: x.windows.clone(),
                        budget: x.budget,
                        ramps: x.ramps.clone(),
                        ..Default::default()
                    })
                })
                .collect();
        }
        CarrierKeys {
            gc: gc.clone(),
            member_cb,
            lists,
            list_of,
            class_of: choice_class,
            windows,
            coef: Coef {
                times: coef.times.clone(),
                z: coef.z.clone(),
                max_jp: coef.max_jp.clone(),
                jp: coef.jp.clone(),
                ..Default::default()
            },
            terms,
            cnc,
            levels,
            level_sums,
            additive,
            commands,
            cache: RefCell::default(),
        }
    }

    /// The window list of a pool member and choice when it is a carrier.
    pub(crate) fn list(&self, m: usize, choice: usize) -> Option<u16> {
        self.list_of[m][choice]
    }

    /// The class of a pool member and choice: pairs of one member and class have the same gains under every envelope.
    pub(crate) fn class(&self, m: usize, choice: usize) -> usize {
        self.class_of[m][choice]
    }

    /// The number of classes of a pool member (every class below it).
    pub(crate) fn classes(&self, m: usize) -> usize {
        self.windows[m].len()
    }

    /// The number of distinct carrier window lists.
    pub(crate) fn list_count(&self) -> usize {
        self.lists.len()
    }

    /// The envelope of the completions of a prefix with carriers of the lists `placed` and `r` slots to fill.
    pub(crate) fn envelope(&self, placed: &[u16], r: usize) -> Rc<KeyedEnvelope> {
        let mut ids = placed.to_vec();
        ids.sort_unstable();
        let mut key: KeyedKey = ([0u16; 5], ids.len(), r);
        key.0[..ids.len()].copy_from_slice(&ids);
        if let Some(e) = self.cache.borrow().get(&key) {
            return e.clone();
        }
        let e = Rc::new(self.build_envelope(&ids, r));
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= KEYED_CACHE {
            cache.clear();
        }
        cache.insert(key, e.clone());
        e
    }

    /// `envelope` without the cache: the envelope of the decks with carriers of the lists `ids` and at most `r` other
    /// carriers.
    pub(crate) fn build_envelope(&self, ids: &[u16], r: usize) -> KeyedEnvelope {
        let lists: Vec<&[ComboBonusRow]> = ids.iter().map(|&i| &self.lists[i as usize][..]).collect();
        let (mut g, mut sums) = (Vec::new(), Vec::new());
        keyed_factors(&self.gc, &self.coef.times, &self.member_cb, &lists, r, &mut g, &mut sums);
        let n = (ids.len() + r).min(5);
        let level = &self.levels[n];
        for (c, &l) in sums.iter_mut().zip(&self.level_sums[n]) {
            *c = c.min(l);
        }
        let (ks, pc, pj) = factor_sums(&self.coef, &self.terms, self.cnc, |e| g[e].min(level[e]));
        let a0 = pc[ks.len()];
        // the completions read coefficients at most these, so their factor error sensitivity is at most theirs
        let sensitivity = self.additive.as_ref().map_or(0.0, |d| {
            let coef = Coef { k: ks, z: self.coef.z.clone(), ..Default::default() };
            factor_error_sensitivity(&coef, d.judgement_max).expect("keyed sensitivity at most the pool-wide one")
        });
        KeyedEnvelope { a0, sensitivity, pc, pj, sums, gains: RefCell::default() }
    }

    /// `A0` of an envelope for completions of the placed performers `placed` (pool member, choice) with `free` slots to
    /// fill. With the additive drift envelope, its offset reads the drift of these performers' commands and of the
    /// most any performers file in the other slots (`factor_drift`), at most the pool-wide one.
    pub(crate) fn a0(&self, env: &KeyedEnvelope, placed: impl Iterator<Item = (usize, usize)>, free: usize) -> f64 {
        self.a0_of(env, self.commands_of(placed, free))
    }

    /// The factor commands and the factor total of the placed performers `placed` (pool member, choice) and of the
    /// most any performers file in `free` other slots.
    pub(crate) fn commands_of(&self, placed: impl Iterator<Item = (usize, usize)>, free: usize) -> (f64, f64) {
        let (mut n_cmd, mut f_tot) = (0f64, 0f64);
        for (m, choice) in placed {
            (n_cmd, f_tot) = self.with_performer((n_cmd, f_tot), m, choice);
        }
        if let Some(d) = &self.additive {
            for k in 0..free {
                n_cmd = (n_cmd + d.cmd_top[k]).next_up();
                f_tot = (f_tot + d.fac_top[k]).next_up();
            }
        }
        (n_cmd, f_tot)
    }

    /// `commands_of` with one more performer.
    pub(crate) fn with_performer(&self, (n_cmd, f_tot): (f64, f64), m: usize, choice: usize) -> (f64, f64) {
        let (cmds, fac) = self.commands[m][choice];
        ((n_cmd + cmds).next_up(), (f_tot + fac).next_up())
    }

    /// `A0` of an envelope for completions whose performers file at most `n_cmd` factor commands with factors summing
    /// to at most `f_tot` (`commands_of`).
    pub(crate) fn a0_of(&self, env: &KeyedEnvelope, (n_cmd, f_tot): (f64, f64)) -> f64 {
        let Some(d) = &self.additive else { return env.a0 };
        let drift = d.drift(n_cmd, f_tot).expect("prefix counts bounded by the admitted pool certificate");
        let delta = float_margin::with_chain(drift, d.chain_extra).expect("finite prefix drift");
        additive_joint_envelope(env.a0, env.a0, delta, d.roundings, env.sensitivity, d.chain_extra)
            .expect("prefix envelope at most the pool-wide one")
            .0
    }

    /// The gains of a placed pool member and choice under an envelope, by position.
    pub(crate) fn gains(&self, env: &KeyedEnvelope, m: usize, choice: usize) -> [f64; 5] {
        if let Some(g) = env.gains.borrow().get(&(m, choice)) {
            return *g;
        }
        let g = self.gains_uncached(env, m, choice);
        env.gains.borrow_mut().insert((m, choice), g);
        g
    }

    /// `gains` without storing them in the envelope.
    pub(crate) fn gains_uncached(&self, env: &KeyedEnvelope, m: usize, choice: usize) -> [f64; 5] {
        let class = self.class_of[m][choice];
        let reads = RampReads { gc: &self.gc, sums: &env.sums, times: &self.coef.times };
        self.windows[m][class].each_ref().map(|c| window_gain(c, &env.pc, &env.pj, Some(reads)))
    }

    /// Diagnostics only: each entry's envelope coefficient, its term in the cheap sum with the performers `placed`
    /// (pool member, choice, position) and their conversion budgets.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn note_terms(
        &self,
        env: &KeyedEnvelope,
        placed: &[(usize, usize, usize)],
    ) -> (Vec<f64>, Vec<f64>, f64) {
        let ne = env.pc.len() - 1;
        let coef: Vec<f64> = (0..ne).map(|e| env.pc[e + 1] - env.pc[e]).collect();
        let mut term = coef.clone();
        let mut budget = 0f64;
        let reads = RampReads { gc: &self.gc, sums: &env.sums, times: &self.coef.times };
        for &(m, choice, k) in placed {
            let c = &self.windows[m][self.class_of[m][choice]][k];
            budget += c.budget;
            for x in &c.windows {
                for e in x.lo as usize..x.hi as usize {
                    let note = if x.ramp == 0 { x.note } else { reads.factor(x, &c.ramps[x.ramp as usize - 1], e) };
                    term[e] += note * coef[e];
                    for j in 0..4 {
                        term[e] += x.judge[j] * (env.pj[j][e + 1] - env.pj[j][e]);
                    }
                }
            }
        }
        (coef, term, budget)
    }
}
