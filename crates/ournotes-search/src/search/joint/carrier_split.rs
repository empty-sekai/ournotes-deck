//! Node bounds split by the Gekisou combo carriers of the slots to fill. Every completion of a prefix whose placed
//! carriers bring the window lists `S` brings the lists `S` and `T` for the multiset `T` of its other carriers' lists,
//! and no other carrier. Its cheap bound is at most the one of a complete deck under the envelope keyed by exactly `S`
//! and `T`, every slot reading its gains under that envelope. For each `T` of at most the slots to fill, the carriers of
//! `T` take the best powers and gains of their lists' characters and the other slots the table relaxation of the pairs
//! that are no carrier, with gains under the same envelope; the node bound is the largest of these. The slots to fill
//! of a node take candidates from a choice index on (ascending below the leader), so every table is also kept for the
//! pairs from each of a few suffix starts, and a bound reads the latest start at most its index.
use super::relax_tables::{RelaxTables, Top};
use super::*;
use crate::search::snaps::{CarrierKeys, KeyedEnvelope};
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

/// The most distinct carrier lists split over; with more, the multisets to visit grow past a cheap node check.
const MAX_LISTS: usize = 8;
/// The most envelopes kept at a time.
const ENV_CACHE: usize = 512;
/// The first suffix start after 0; each later one is about half again the previous.
const FIRST_START: usize = 8;
/// The ratio between neighbouring weights `λ` of the coupled bound: a weight off the best one by at most half a step
/// loses at most `((s + 1/s)/2)² - 1` with `s = √WEIGHT_STEP`, under 0.1%.
const WEIGHT_STEP: f64 = 1.08;

pub(super) struct CarrierSplit {
    keys: Rc<CarrierKeys>,
    lists: usize,
    /// The choice index (see `JointBounds::choices`) of every pool member and choice, `u32::MAX` outside them.
    index: Vec<Vec<u32>>,
    /// `[pool member]`: the offset of the member's classes in an envelope's gains read.
    class_at: Vec<usize>,
    /// The classes of all pool members.
    classes: usize,
    /// Suffix starts, ascending from 0: the tables at start `k` cover the pairs from choice index `starts[k]` on.
    starts: Vec<usize>,
    /// `[list]`: the list's pairs (pool member, choice) with their character.
    pairs: Vec<Vec<(usize, usize, i64)>>,
    /// `[start][profile][list]`: the powers of the list's pairs by character.
    power: Vec<Vec<Vec<ByCharacter<i64>>>>,
    /// `[r]`: the multisets of at most `r` lists.
    sets: Vec<Vec<Vec<u16>>>,
    /// Envelopes by sorted carrier lists.
    envs: RefCell<HashMap<Vec<u16>, Rc<SplitEnv>>>,
}

struct SplitEnv {
    env: KeyedEnvelope,
    /// The position-mean gain of every member's class (at `class_at[member] + class`), NaN until read.
    read: RefCell<Vec<f64>>,
    /// `[start][list]`: the gains of the list's pairs by character (filled on first use).
    gain: Vec<Vec<OnceCell<ByCharacter<f64>>>>,
    /// `[start]`: the relaxation tables of the pairs that are no carrier, with their gains (filled on first use; None
    /// when a table value is not finite).
    plain: Vec<OnceCell<Option<RelaxTables>>>,
    /// The coupled tables by (start, leader profile, weight index), filled on first use.
    coupled: RefCell<HashMap<(usize, usize, i32), Rc<Coupled>>>,
}

/// The tables of one weight `λ`: every pair reads `λ·power + gain/λ`, rounded up.
struct Coupled {
    /// `[list]`: the values of the list's pairs from the start on by character.
    lists: Vec<ByCharacter<f64>>,
    /// The relaxation tables of the pairs from the start on that are no carrier, reading the value as their gain
    /// (None when a value is not finite).
    plain: Option<RelaxTables>,
}

/// One carrier list's pairs by character: the best value of its pairs without a Snap and the best values per Snap.
struct ByCharacter<T> {
    rows: Vec<(i64, Option<T>, Top<T>)>,
}

impl<T: Copy + PartialOrd> ByCharacter<T> {
    fn compile(pairs: &[(usize, usize, i64)], value: impl Fn(usize, usize) -> T) -> Self {
        let larger = |a: T, b: T| if b > a { b } else { a };
        let mut by: HashMap<i64, (Option<T>, HashMap<u32, T>)> = HashMap::new();
        for &(m, c, ch) in pairs {
            let v = value(m, c);
            let row = by.entry(ch).or_default();
            if c == 0 {
                row.0 = Some(row.0.map_or(v, |x| larger(x, v)));
            } else {
                let x = row.1.entry((c - 1) as u32).or_insert(v);
                *x = larger(*x, v);
            }
        }
        let mut rows: Vec<_> = by
            .into_iter()
            .map(|(ch, (base, snaps))| (ch, base, Top::from_best(snaps.into_iter().map(|(j, v)| (v, j)))))
            .collect();
        rows.sort_by_key(|row| row.0);
        ByCharacter { rows }
    }

    /// The `k` largest best values of characters outside `taken` (character IDs) without a Snap or with one outside
    /// `taken_snaps`, summed in descending order with `add`; None with fewer such characters.
    fn top(&self, taken: &[i64], taken_snaps: &[bool], k: usize, zero: T, add: impl Fn(T, T) -> T) -> Option<T> {
        if k == 0 {
            return Some(zero);
        }
        let mut best: Vec<T> = self
            .rows
            .iter()
            .filter(|row| !taken.contains(&row.0))
            .filter_map(|(_, base, snaps)| match (*base, snaps.first(|j| taken_snaps[j as usize])) {
                (Some(a), Some(b)) => Some(if b > a { b } else { a }),
                (a, b) => a.or(b),
            })
            .collect();
        if best.len() < k {
            return None;
        }
        best.sort_by(|a, b| b.partial_cmp(a).expect("finite split value"));
        Some(best[..k].iter().fold(zero, |sum, &v| add(sum, v)))
    }
}

impl CarrierSplit {
    pub(super) fn compile(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        if !b.gekisou || b.points.is_some() {
            return None;
        }
        let keys = b.carrier_levels.as_ref()?.keys.clone()?;
        let lists = keys.list_count();
        if lists == 0 || lists > MAX_LISTS {
            return None;
        }
        let mut index = vec![Vec::new(); pool.members.len()];
        for &m in domain.members() {
            index[m] = vec![u32::MAX; domain.snaps().len() + 1];
        }
        for (i, &(m, c)) in b.choices.iter().enumerate() {
            index[m][c] = u32::try_from(i).ok()?;
        }
        let mut class_at = Vec::with_capacity(pool.members.len());
        let mut classes = 0;
        for m in 0..pool.members.len() {
            class_at.push(classes);
            classes += keys.classes(m);
        }
        let mut starts = vec![0];
        let mut next = FIRST_START;
        while next < b.choices.len() {
            starts.push(next);
            next += next.div_ceil(2);
        }
        let mut pairs = vec![Vec::new(); lists];
        for &m in domain.members() {
            for c in 0..=domain.snaps().len() {
                if let Some(id) = keys.list(m, c) {
                    pairs[id as usize].push((m, c, pool.members[m].character_id));
                }
            }
        }
        let power = starts
            .iter()
            .map(|&from| {
                (0..b.lead.len())
                    .map(|profile| {
                        pairs
                            .iter()
                            .map(|list| {
                                let list: Vec<_> =
                                    list.iter().copied().filter(|&(m, c, _)| index[m][c] as usize >= from).collect();
                                ByCharacter::compile(&list, |m, c| {
                                    b.a[m] + b.lead[profile][m] + if c == 0 { 0 } else { b.w[m][c - 1] }
                                })
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        let sets = (0..5).map(|r| multisets(lists, r)).collect();
        Some(CarrierSplit {
            keys,
            lists,
            index,
            class_at,
            classes,
            starts,
            pairs,
            power,
            sets,
            envs: RefCell::default(),
        })
    }

    /// The latest suffix start at most `from`.
    fn start(&self, from: usize) -> usize {
        self.starts.partition_point(|&s| s <= from) - 1
    }

    fn env(&self, ids: &[u16]) -> Rc<SplitEnv> {
        if let Some(e) = self.envs.borrow().get(ids) {
            return e.clone();
        }
        let e = Rc::new(SplitEnv {
            env: self.keys.build_envelope(ids, 0),
            read: RefCell::new(vec![f64::NAN; self.classes]),
            gain: self.starts.iter().map(|_| (0..self.lists).map(|_| OnceCell::new()).collect()).collect(),
            plain: self.starts.iter().map(|_| OnceCell::new()).collect(),
            coupled: RefCell::default(),
        });
        let mut envs = self.envs.borrow_mut();
        if envs.len() >= ENV_CACHE {
            envs.clear();
        }
        envs.insert(ids.to_vec(), e.clone());
        e
    }

    /// The position-mean gain of a pool member and choice under an envelope (read once per member and class).
    fn gain(&self, e: &SplitEnv, m: usize, c: usize) -> f64 {
        let at = self.class_at[m] + self.keys.class(m, c);
        let read = e.read.borrow()[at];
        if !read.is_nan() {
            return read;
        }
        let g = super::super::uniform::mean_up(&self.keys.gains_uncached(&e.env, m, c));
        e.read.borrow_mut()[at] = g;
        g
    }

    /// The gains of a list's pairs from suffix start `k` on by character under an envelope.
    fn gains<'a>(&self, e: &'a SplitEnv, k: usize, list: usize) -> &'a ByCharacter<f64> {
        e.gain[k][list].get_or_init(|| {
            let from = self.starts[k];
            let list: Vec<_> =
                self.pairs[list].iter().copied().filter(|&(m, c, _)| self.index[m][c] as usize >= from).collect();
            ByCharacter::compile(&list, |m, c| self.gain(e, m, c))
        })
    }

    /// The relaxation tables of the pairs from suffix start `k` on that are no carrier, under an envelope.
    fn plain<'a>(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        e: &'a SplitEnv,
        k: usize,
    ) -> Option<&'a RelaxTables> {
        e.plain[k]
            .get_or_init(|| {
                let (keys, from) = (&self.keys, self.starts[k]);
                RelaxTables::compile_where(
                    b,
                    pool,
                    domain,
                    &|m, c| keys.list(m, c).is_none() && self.index[m][c] as usize >= from,
                    &|m, c| [self.gain(e, m, c); 5],
                )
            })
            .as_ref()
    }

    /// The coupled tables of weight `WEIGHT_STEP^j` for the pairs from suffix start `k` on under an envelope.
    #[allow(clippy::too_many_arguments)]
    fn coupled(
        &self,
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        e: &SplitEnv,
        k: usize,
        profile: usize,
        j: i32,
    ) -> Rc<Coupled> {
        if let Some(c) = e.coupled.borrow().get(&(k, profile, j)) {
            return c.clone();
        }
        let (lambda, from) = (WEIGHT_STEP.powi(j), self.starts[k]);
        let value = |m: usize, c: usize| {
            let power = b.a[m] + b.lead[profile][m] + if c == 0 { 0 } else { b.w[m][c - 1] };
            add_up((lambda * power as f64).next_up(), (self.gain(e, m, c) / lambda).next_up())
        };
        let lists = self
            .pairs
            .iter()
            .map(|list| {
                let list: Vec<_> =
                    list.iter().copied().filter(|&(m, c, _)| self.index[m][c] as usize >= from).collect();
                ByCharacter::compile(&list, value)
            })
            .collect();
        let plain = RelaxTables::compile_where(
            b,
            pool,
            domain,
            &|m, c| self.keys.list(m, c).is_none() && self.index[m][c] as usize >= from,
            &|m, c| [value(m, c); 5],
        );
        let c = Rc::new(Coupled { lists, plain });
        e.coupled.borrow_mut().insert((k, profile, j), c.clone());
        c
    }
}

/// The index `j` of the weight `WEIGHT_STEP^j` nearest `√(q/power)`, the weight at which `(λ·power + q/λ)²/4` equals
/// `power·q`.
fn weight(q: f64, power: i64) -> i32 {
    ((q / power as f64).sqrt().ln() / WEIGHT_STEP.ln()).round() as i32
}

/// Every multiset of at most `r` of `0..lists`, as nondecreasing sequences.
fn multisets(lists: usize, r: usize) -> Vec<Vec<u16>> {
    let mut out = vec![Vec::new()];
    let mut frontier: Vec<Vec<u16>> = vec![Vec::new()];
    for _ in 0..r {
        let mut next = Vec::new();
        for t in &frontier {
            for l in t.last().map_or(0, |&x| x as usize)..lists {
                let mut u = t.clone();
                u.push(l as u16);
                next.push(u);
            }
        }
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

/// The lists of a nondecreasing multiset with their counts.
fn runs(t: &[u16]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        let list = *t.get(i)?;
        let k = t[i..].iter().take_while(|&&x| x == list).count();
        i += k;
        Some((list as usize, k))
    })
}

impl JointBounds {
    /// The carrier split bound, as a payoff numerator over the order masses `orders` (position-mean gains read the
    /// same at every position), of the completions of a prefix at `depth` (1..5) whose slots to fill take candidates
    /// from choice index `from` on. Stops at the first multiset whose bound exceeds `threshold`, returning that bound;
    /// otherwise the largest. None without the split tables. A multiset whose bound exceeds `threshold` is first
    /// coupled (see `split_upper`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn carrier_split_expected_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        from: usize,
        orders: &[([usize; 5], u128)],
        threshold: i128,
    ) -> Option<i128> {
        self.split_upper(pool, domain, p, depth, from, orders, threshold, threshold)
    }

    /// `carrier_split_expected_upper` without a threshold, every multiset coupled.
    #[cfg(feature = "search-diagnostics")]
    pub(crate) fn carrier_split_coupled_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        from: usize,
        orders: &[([usize; 5], u128)],
    ) -> Option<i128> {
        self.split_upper(pool, domain, p, depth, from, orders, i128::MAX, i128::MIN)
    }

    /// The carrier split bound; a multiset whose bound exceeds `couple_above` also takes the coupled bound: every
    /// completion's `power·(A0 + gain)` is at most `(λ·power + (A0 + gain)/λ)²/4` for any weight `λ > 0`, and the
    /// slots to fill relax `λ·power + gain/λ` as one value per pair, so a pair's power and gain come from the same
    /// pair. The weight is the step nearest the one at which the bound meets the uncoupled one at its terms.
    #[allow(clippy::too_many_arguments)]
    fn split_upper(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        from: usize,
        orders: &[([usize; 5], u128)],
        threshold: i128,
        couple_above: i128,
    ) -> Option<i128> {
        let split = self.carrier_split.as_ref()?;
        if !(1..5).contains(&depth) {
            return None;
        }
        let k = split.start(from);
        let mass = i128::try_from(orders.iter().map(|o| o.1).sum::<u128>()).ok()?;
        let keys = &split.keys;
        let profile = self.profile[p.members[2]];
        let choices = Self::prefix_choices(domain, p, depth);
        let r = 5 - depth;
        let mut taken = Vec::with_capacity(depth);
        let mut taken_snaps = vec![false; domain.snaps().len()];
        let mut placed_ids = Vec::with_capacity(depth);
        let mut p0 = 0i64;
        for &slot in &SLOTS[..depth] {
            let (m, c) = (p.members[slot], choices[slot]);
            taken.push(pool.members[m].character_id);
            if c > 0 {
                taken_snaps[c - 1] = true;
            }
            if let Some(id) = keys.list(m, c) {
                placed_ids.push(id);
            }
            p0 += self.a[m] + self.lead[profile][m] + if c == 0 { 0 } else { self.w[m][c - 1] };
        }
        let placed = || SLOTS[..depth].iter().map(|&slot| (p.members[slot], choices[slot]));
        // the split's tables read the same gain at every position
        let positions = [0; 5];
        let mut plain_taken = None;
        let mut best = i128::MIN;
        let mut ids = Vec::with_capacity(5);
        'sets: for t in &split.sets[r] {
            // the carriers of `T`: per list, its best characters (characters may repeat across lists, Snaps across
            // lists and slots)
            let mut power = p0;
            for (list, n) in runs(t) {
                match split.power[k][profile][list].top(&taken, &taken_snaps, n, 0, |a, b| a + b) {
                    Some(v) => power += v,
                    None => continue 'sets,
                }
            }
            ids.clear();
            ids.extend_from_slice(&placed_ids);
            ids.extend_from_slice(t);
            ids.sort_unstable();
            let e = split.env(&ids);
            let a0 = keys.a0(&e.env, placed(), r);
            let mut gain = 0f64;
            for (m, c) in placed() {
                gain = add_up(gain, super::super::uniform::mean_up(&keys.gains(&e.env, m, c)));
            }
            let placed_gain = gain;
            for (list, n) in runs(t) {
                gain = add_up(gain, split.gains(&e, k, list).top(&taken, &taken_snaps, n, 0.0, add_up)?);
            }
            let k0 = r - t.len();
            let free = &SLOTS[depth..depth + k0];
            if k0 > 0 {
                let tables = split.plain(self, pool, domain, &e, k)?;
                let (taken_characters, taken_snaps) =
                    plain_taken.get_or_insert_with(|| tables.taken(pool, domain, p, depth));
                let (pp, pg, _) = tables.free_part(self, profile, free, &positions, taken_characters, taken_snaps);
                power += pp;
                gain = add_up(gain, pg);
            }
            let level = self.carrier_level(ids.len());
            let mut numerator = level.payoff_cap_from(a0, power, gain, 0).checked_mul(mass)?;
            if numerator > couple_above {
                let j = weight(add_up(a0, gain), power);
                let lambda = WEIGHT_STEP.powi(j);
                let c = split.coupled(self, pool, domain, &e, k, profile, j);
                // the prefix's terms, then the slots to fill (one value per pair)
                let mut sum = add_up((lambda * p0 as f64).next_up(), (add_up(a0, placed_gain) / lambda).next_up());
                let mut complete = true;
                for (list, n) in runs(t) {
                    match c.lists[list].top(&taken, &taken_snaps, n, 0.0, add_up) {
                        Some(v) => sum = add_up(sum, v),
                        None => complete = false,
                    }
                }
                if k0 > 0 {
                    match (&c.plain, &plain_taken) {
                        (Some(tables), Some((taken_characters, taken_snaps))) => {
                            let (_, v, _) =
                                tables.free_part(self, profile, free, &positions, taken_characters, taken_snaps);
                            sum = add_up(sum, v);
                        }
                        _ => complete = false,
                    }
                }
                if complete {
                    let product = (sum * sum).next_up() / 4.0;
                    let cap = (product.min(power as f64 * level.global) * (1.0 + level.eps)).ceil() as i128;
                    numerator = numerator.min(cap.checked_mul(mass)?);
                }
            }
            best = best.max(numerator);
            if numerator > threshold {
                return Some(numerator);
            }
        }
        Some(best)
    }

    /// Diagnostics only: the bound the carrier split relaxes, taken over the completions of a prefix at `depth`
    /// (2..5) one by one. Each completion takes pairs from choice index `from` on in ascending order, with characters
    /// and Snaps free and distinct, and reads its own powers and gains under the envelope of its own carrier lists.
    /// `A0` reads the slots to fill as free, as the split does; `ownA0` reads the completion's performers. None
    /// without the split tables or past `limit` completions.
    #[cfg(feature = "search-diagnostics")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn carrier_split_completions(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
        from: usize,
        orders: &[([usize; 5], u128)],
        limit: u64,
    ) -> Option<serde_json::Value> {
        let split = self.carrier_split.as_ref()?;
        if !(2..5).contains(&depth) {
            return None;
        }
        let mass = i128::try_from(orders.iter().map(|o| o.1).sum::<u128>()).ok()?;
        let keys = &split.keys;
        let profile = self.profile[p.members[2]];
        let choices = Self::prefix_choices(domain, p, depth);
        let r = 5 - depth;
        let placed: Vec<(usize, usize)> = SLOTS[..depth].iter().map(|&s| (p.members[s], choices[s])).collect();
        let power = |m: usize, c: usize| self.a[m] + self.lead[profile][m] + if c == 0 { 0 } else { self.w[m][c - 1] };
        let character = |m: usize| pool.members[m].character_id;
        let p0: i64 = placed.iter().map(|&(m, c)| power(m, c)).sum();
        let candidates: Vec<(usize, usize)> = self.choices[from.min(self.choices.len())..]
            .iter()
            .copied()
            .filter(|&(m, c)| placed.iter().all(|&(pm, pc)| character(pm) != character(m) && (c == 0 || pc != c)))
            .collect();
        struct Best {
            numerator: i128,
            own: i128,
            pick: Vec<(usize, usize)>,
            power: i64,
            gain: f64,
            a0: f64,
            lists: Vec<u16>,
        }
        let mut best = Best {
            numerator: i128::MIN,
            own: i128::MIN,
            pick: Vec::new(),
            power: 0,
            gain: 0.0,
            a0: 0.0,
            lists: Vec::new(),
        };
        let mut count = 0u64;
        let mut visit = |pick: &[(usize, usize)]| {
            count += 1;
            let mut ids: Vec<u16> = placed.iter().chain(pick).filter_map(|&(m, c)| keys.list(m, c)).collect();
            ids.sort_unstable();
            let e = split.env(&ids);
            let mut gain = 0f64;
            for &(m, c) in &placed {
                gain = add_up(gain, super::super::uniform::mean_up(&keys.gains(&e.env, m, c)));
            }
            let mut pw = p0;
            for &(m, c) in pick {
                pw += power(m, c);
                gain = add_up(gain, split.gain(&e, m, c));
            }
            let level = self.carrier_level(ids.len());
            let a0 = keys.a0(&e.env, placed.iter().copied(), r);
            let numerator = level.payoff_cap_from(a0, pw, gain, 0).saturating_mul(mass);
            let own = keys.a0(&e.env, placed.iter().chain(pick).copied(), 0);
            best.own = best.own.max(level.payoff_cap_from(own, pw, gain, 0).saturating_mul(mass));
            if numerator > best.numerator {
                best = Best { numerator, own: best.own, pick: pick.to_vec(), power: pw, gain, a0, lists: ids };
            }
        };
        type Pair = (usize, usize);
        type Clash<'a> = dyn Fn(&[Pair], Pair) -> bool + 'a;
        type Visit<'a> = dyn FnMut(&[Pair]) + 'a;
        fn walk(
            at: usize,
            pick: &mut Vec<Pair>,
            r: usize,
            candidates: &[Pair],
            clash: &Clash,
            visit: &mut Visit,
            budget: &mut u64,
        ) {
            if pick.len() == r {
                *budget = budget.saturating_sub(1);
                visit(pick);
                return;
            }
            for i in at..candidates.len() {
                if *budget == 0 {
                    return;
                }
                if !clash(pick, candidates[i]) {
                    pick.push(candidates[i]);
                    walk(i + 1, pick, r, candidates, clash, visit, budget);
                    pick.pop();
                }
            }
        }
        let clash = |pick: &[(usize, usize)], (m, c): (usize, usize)| {
            pick.iter().any(|&(pm, pc)| character(pm) == character(m) || (c != 0 && pc == c))
        };
        let mut budget = limit;
        walk(0, &mut Vec::with_capacity(r), r, &candidates, &clash, &mut visit, &mut budget);
        if budget == 0 {
            return None;
        }
        let s = |v: i128| v.to_string();
        Some(serde_json::json!({
            "candidates": candidates.len(),
            "completions": count,
            "upper": s(best.numerator),
            "ownA0": s(best.own),
            "argmax": best.pick.iter().map(|&(m, c)| [m, c]).collect::<Vec<_>>(),
            "lists": best.lists,
            "power": best.power,
            "gain": best.gain,
            "a0": best.a0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multisets_cover_every_count_of_every_list_once() {
        let all = multisets(3, 2);
        assert_eq!(all.len(), 1 + 3 + 6);
        for t in &all {
            assert!(t.windows(2).all(|w| w[0] <= w[1]));
        }
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
        assert_eq!(multisets(6, 4).len(), 1 + 6 + 21 + 56 + 126);
        assert_eq!(runs(&[0, 0, 2, 3, 3]).collect::<Vec<_>>(), [(0, 2), (2, 1), (3, 2)]);
    }

    #[test]
    fn the_nearest_weight_meets_the_product_within_its_step() {
        for (q, power) in [(1387.0, 290_825i64), (2.5, 7), (0.01, 1_000_000)] {
            let lambda = WEIGHT_STEP.powi(weight(q, power));
            let bound = (lambda * power as f64 + q / lambda).powi(2) / 4.0;
            let product = q * power as f64;
            assert!(bound >= product * (1.0 - 1e-12));
            assert!(bound <= product * 1.001);
        }
    }

    #[test]
    fn carriers_take_free_characters_with_a_free_snap() {
        // (member, choice, character): character 1 without a Snap 5 or with Snap 0 9; character 2 with Snap 1 7;
        // character 3 with Snap 0 8
        let pairs = [(0, 0, 1), (0, 1, 1), (1, 2, 2), (2, 1, 3)];
        let value = |m: usize, c: usize| [[5, 9, 0], [0, 0, 7], [0, 8, 0]][m][c];
        let table = ByCharacter::compile(&pairs, value);
        let add = |a: i64, b: i64| a + b;
        assert_eq!(table.top(&[], &[false, false], 2, 0, add), Some(17));
        // Snap 0 taken: character 1 keeps its pair without a Snap, character 3 has none left
        assert_eq!(table.top(&[], &[true, false], 2, 0, add), Some(12));
        assert_eq!(table.top(&[], &[true, false], 3, 0, add), None);
        assert_eq!(table.top(&[2], &[false, false], 2, 0, add), Some(17));
        assert_eq!(table.top(&[1], &[false, true], 2, 0, add), None);
        assert_eq!(table.top(&[1, 2, 3], &[false, false], 0, 0, add), Some(0));
    }
}
