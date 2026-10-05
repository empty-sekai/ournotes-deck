//! Table form of the cheap prefix relaxation. A prefix fixes at most four characters and four Snaps, so for every
//! maximum the relaxation reads it suffices to keep the five best entries with distinct excluded keys: the first
//! entry whose key is still free is the exact maximum over the free keys. Values and their floating operations are
//! the same as in the member/Snap scan, so the relaxation returns identical numbers.
use super::*;

const KEEP: usize = 5;

/// Up to KEEP (value, key) entries with distinct keys, best first.
#[derive(Clone, Debug, Default)]
pub(super) struct Top<T> {
    entries: Vec<(T, u32)>,
}
impl<T: Copy + PartialOrd> Top<T> {
    /// The KEEP best entries, by value then key, as a full sort would order them.
    pub(super) fn from_best(best: impl Iterator<Item = (T, u32)>) -> Self {
        let before = |a: &(T, u32), b: &(T, u32)| match a.0.partial_cmp(&b.0).expect("finite table value") {
            std::cmp::Ordering::Equal => a.1 < b.1,
            order => order.is_gt(),
        };
        let mut entries: Vec<(T, u32)> = Vec::with_capacity(KEEP);
        for e in best {
            if entries.len() == KEEP {
                if !before(&e, &entries[KEEP - 1]) {
                    continue;
                }
                entries.pop();
            }
            let at = entries.partition_point(|x| before(x, &e));
            entries.insert(at, e);
        }
        Top { entries }
    }
    /// The best value whose key is free, if any.
    pub(super) fn first(&self, taken: impl Fn(u32) -> bool) -> Option<T> {
        self.entries.iter().find(|e| !taken(e.1)).map(|e| e.0)
    }
}

/// The `take` (at most 5) largest values pushed so far, largest first.
struct Largest<T> {
    values: [T; 5],
    len: usize,
    take: usize,
}

impl<T: Copy + PartialOrd> Largest<T> {
    fn new(take: usize, zero: T) -> Self {
        assert!(take <= 5, "at most five slots");
        Largest { values: [zero; 5], len: 0, take }
    }

    fn push(&mut self, x: T) {
        let mut i = if self.len < self.take {
            self.len += 1;
            self.len - 1
        } else if x > self.values[self.take - 1] {
            self.take - 1
        } else {
            return;
        };
        while i > 0 && x > self.values[i - 1] {
            self.values[i] = self.values[i - 1];
            i -= 1;
        }
        self.values[i] = x;
    }
}

impl Largest<i64> {
    fn sum(&self) -> i64 {
        self.values[..self.len].iter().sum()
    }
}

impl Largest<f64> {
    /// The sum in descending order, rounded up.
    fn sum_up(&self) -> f64 {
        self.values[..self.len].iter().fold(0.0, |sum, &x| add_up(sum, x))
    }
}

pub(super) struct RelaxTables {
    /// Domain characters (distinct, any order).
    characters: Vec<i64>,
    /// Whether a character has a member with an allowed pair (every character without a filter).
    present: Vec<bool>,
    /// [profile][character]: best member-only power, and best power per Snap (key = domain Snap index).
    base_power: Vec<Vec<i64>>,
    snap_power_by_character: Vec<Vec<Top<i64>>>,
    /// [character][position]: best member-only gain, and best gain per Snap.
    base_gain: Vec<[f64; 5]>,
    snap_gain_by_character: Vec<[Top<f64>; 5]>,
    /// [character]: best member-only bonus, and best bonus per Snap (PT only).
    base_bonus: Vec<i64>,
    snap_bonus_by_character: Vec<Top<i64>>,
    /// [Snap]: best Snap power increment per character, and best gain increment per position and character.
    increment_power: Vec<Top<i64>>,
    increment_gain: Vec<[Top<f64>; 5]>,
}

impl RelaxTables {
    /// None when a table value is not finite; the scan then remains the relaxation.
    pub(super) fn compile(b: &JointBounds, pool: &Pool, domain: &CandidateDomain) -> Option<Self> {
        Self::compile_where(b, pool, domain, &|_, _| true, &|m, c| b.gains[m][c])
    }

    /// The tables of the pairs (pool member, choice) that `allowed` admits, with the gains `gains` reads. A member
    /// with an admitted pair keeps its member-only power and gain as the base of its Snap increments, so each admitted
    /// pair is at most its character's base plus its Snap's increment, as in the unfiltered tables.
    pub(super) fn compile_where(
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        allowed: &dyn Fn(usize, usize) -> bool,
        gains: &dyn Fn(usize, usize) -> [f64; 5],
    ) -> Option<Self> {
        let all: Vec<usize> = domain.members().to_vec();
        let members: Vec<usize> =
            all.iter().copied().filter(|&m| (0..=domain.snaps().len()).any(|c| allowed(m, c))).collect();
        let members = &members[..];
        let snaps = domain.snaps().len();
        // [member index][choice]
        let g: Vec<Vec<[f64; 5]>> = members.iter().map(|&m| (0..=snaps).map(|c| gains(m, c)).collect()).collect();
        let mut characters: Vec<i64> = Vec::new();
        let member_character: Vec<usize> = members
            .iter()
            .map(|&m| {
                let c = pool.members[m].character_id;
                characters.iter().position(|&x| x == c).unwrap_or_else(|| {
                    characters.push(c);
                    characters.len() - 1
                })
            })
            .collect();
        let mut present = vec![false; characters.len()];
        for &c in &member_character {
            present[c] = true;
        }
        // characters of the domain with no admitted pair stay absent
        for &m in &all {
            let c = pool.members[m].character_id;
            if !characters.contains(&c) {
                characters.push(c);
                present.push(false);
            }
        }
        let nc = characters.len();
        let profiles = b.lead.len();
        if g.iter().flatten().flatten().any(|g| !g.is_finite()) {
            return None;
        }
        // [member index][Snap]: whether the member's pair with the Snap is admitted
        let admitted: Vec<Vec<bool>> = members.iter().map(|&m| (1..=snaps).map(|c| allowed(m, c)).collect()).collect();
        // [character]: its member indices
        let mut of_character = vec![Vec::new(); nc];
        for (i, &c) in member_character.iter().enumerate() {
            of_character[c].push(i);
        }
        // the gains read the same at every position: compute the first and copy it
        let same = g.iter().flatten().all(|row| row.iter().all(|&x| x.to_bits() == row[0].to_bits()));
        let positions = if same { 1 } else { 5 };
        // Maxima over the members of one character, keyed by Snap; then the KEEP best Snaps.
        let by_character = |c: usize, value: &dyn Fn(usize, usize) -> f64| -> Vec<f64> {
            let mut best = vec![f64::NEG_INFINITY; snaps];
            for &i in &of_character[c] {
                for (j, slot) in best.iter_mut().enumerate() {
                    if admitted[i][j] {
                        *slot = slot.max(value(i, j));
                    }
                }
            }
            best
        };
        let mut base_power = vec![vec![i64::MIN; nc]; profiles];
        let mut snap_power_by_character = vec![vec![Top::default(); nc]; profiles];
        for profile in 0..profiles {
            for c in 0..nc {
                let mut best = vec![i64::MIN; snaps];
                for &i in &of_character[c] {
                    let m = members[i];
                    let own = b.a[m] + b.lead[profile][m];
                    base_power[profile][c] = base_power[profile][c].max(own);
                    for (j, slot) in best.iter_mut().enumerate() {
                        if admitted[i][j] {
                            *slot = (*slot).max(own + b.w[m][j]);
                        }
                    }
                }
                snap_power_by_character[profile][c] = Top::from_best(best.into_iter().zip(0..));
            }
        }
        let mut base_gain = vec![[f64::NEG_INFINITY; 5]; nc];
        let mut snap_gain_by_character: Vec<[Top<f64>; 5]> = vec![Default::default(); nc];
        for c in 0..nc {
            for pos in 0..positions {
                for &i in &of_character[c] {
                    base_gain[c][pos] = base_gain[c][pos].max(g[i][0][pos]);
                }
                let best = by_character(c, &|i, j| g[i][j + 1][pos]);
                snap_gain_by_character[c][pos] = Top::from_best(best.into_iter().zip(0..));
            }
            for pos in positions..5 {
                base_gain[c][pos] = base_gain[c][0];
                snap_gain_by_character[c][pos] = snap_gain_by_character[c][0].clone();
            }
        }
        let mut base_bonus = vec![0i64; nc];
        let mut snap_bonus_by_character = vec![Top::default(); nc];
        if let Some(pt) = &b.points {
            for c in 0..nc {
                let mut best = vec![i64::MIN; snaps];
                base_bonus[c] = i64::MIN;
                for &i in &of_character[c] {
                    let m = members[i];
                    base_bonus[c] = base_bonus[c].max(pt.member[m]);
                    for (j, slot) in best.iter_mut().enumerate() {
                        if admitted[i][j] {
                            *slot = (*slot).max(pt.member[m] + pt.snap[j]);
                        }
                    }
                }
                snap_bonus_by_character[c] = Top::from_best(best.into_iter().zip(0..));
            }
        }
        let mut increment_power = Vec::with_capacity(snaps);
        let mut increment_gain = Vec::with_capacity(snaps);
        for j in 0..snaps {
            let mut power = vec![i64::MIN; nc];
            let mut gain = [(); 5].map(|_| vec![f64::NEG_INFINITY; nc]);
            for (i, &m) in members.iter().enumerate() {
                if !admitted[i][j] {
                    continue;
                }
                let c = member_character[i];
                power[c] = power[c].max(b.w[m][j]);
                for (pos, row) in gain.iter_mut().enumerate().take(positions) {
                    row[c] = row[c].max((g[i][j + 1][pos] - g[i][0][pos]).next_up());
                }
            }
            increment_power.push(Top::from_best(power.into_iter().zip(0..)));
            let mut tops: [Top<f64>; 5] = Default::default();
            for (pos, row) in gain.into_iter().enumerate().take(positions) {
                tops[pos] = Top::from_best(row.into_iter().zip(0..));
            }
            for pos in positions..5 {
                tops[pos] = tops[0].clone();
            }
            increment_gain.push(tops);
        }
        Some(RelaxTables {
            characters,
            present,
            base_power,
            snap_power_by_character,
            base_gain,
            snap_gain_by_character,
            base_bonus,
            snap_bonus_by_character,
            increment_power,
            increment_gain,
        })
    }

    /// Characters (table index) and Snaps (domain index) fixed by the prefix.
    pub(super) fn taken(
        &self,
        pool: &Pool,
        domain: &CandidateDomain,
        p: &PhysicalDeck,
        depth: usize,
    ) -> (Vec<bool>, Vec<bool>) {
        let mut characters = vec![false; self.characters.len()];
        let mut snaps = vec![false; domain.snaps().len()];
        for &slot in &SLOTS[..depth] {
            let c = pool.members[p.members[slot]].character_id;
            if let Some(i) = self.characters.iter().position(|&x| x == c) {
                characters[i] = true;
            }
            if let Some(s) = p.snaps[slot] {
                snaps[domain.snaps().iter().position(|&v| v == s).expect("compiled Snap")] = true;
            }
        }
        (characters, snaps)
    }

    /// The free-slot part of the relaxation: (power, gain, bonus) added for `free` slots (no forced rules), with
    /// `taken_characters[c]` and `taken_snaps[j]` marking the prefix. Same values as the member/Snap scan.
    pub(super) fn free_part(
        &self,
        b: &JointBounds,
        profile: usize,
        free: &[usize],
        positions: &[usize; 5],
        taken_characters: &[bool],
        taken_snaps: &[bool],
    ) -> (i64, f64, i64) {
        let take = free.len();
        if take == 0 {
            return (0, 0.0, 0);
        }
        // the distinct positions of the free slots
        let (mut at, mut distinct) = ([0usize; 5], 0);
        for &s in free {
            if !at[..distinct].contains(&positions[s]) {
                at[distinct] = positions[s];
                distinct += 1;
            }
        }
        let at = &at[..distinct];
        let snap_taken = |j: u32| taken_snaps[j as usize];
        let character_taken = |c: u32| taken_characters[c as usize];
        let points = b.points.is_some();
        let (mut powers, mut gains, mut bonuses) =
            (Largest::new(take, 0), Largest::new(take, 0.0), Largest::new(take, 0));
        let (mut base_powers, mut base_gains, mut base_bonuses) =
            (Largest::new(take, 0), Largest::new(take, 0.0), Largest::new(take, 0));
        let mut any_character = false;
        for c in (0..self.characters.len()).filter(|&c| self.present[c] && !taken_characters[c]) {
            any_character = true;
            let base_p = self.base_power[profile][c];
            let base_g = at.iter().map(|&pos| self.base_gain[c][pos]).fold(0.0, f64::max);
            let base_b = if points { self.base_bonus[c] } else { 0 };
            let best_p = self.snap_power_by_character[profile][c].first(snap_taken).map_or(base_p, |v| v.max(base_p));
            let best_g = at
                .iter()
                .map(|&pos| self.snap_gain_by_character[c][pos].first(snap_taken).map_or(f64::NEG_INFINITY, |v| v))
                .fold(base_g, f64::max);
            let best_b = if points {
                self.snap_bonus_by_character[c].first(snap_taken).map_or(base_b, |v| v.max(base_b))
            } else {
                0
            };
            powers.push(best_p);
            gains.push(best_g);
            bonuses.push(best_b);
            base_powers.push(base_p);
            base_gains.push(base_g);
            base_bonuses.push(base_b);
        }
        // every Snap counts, a taken one with nothing
        let (mut snap_power, mut snap_gain, mut snap_bonus) =
            (Largest::new(take, 0), Largest::new(take, 0.0), Largest::new(take, 0));
        for j in 0..self.increment_power.len() {
            let (mut p, mut g, mut bonus) = (0i64, 0.0f64, 0i64);
            if !taken_snaps[j] {
                if let Some(v) = self.increment_power[j].first(character_taken) {
                    p = p.max(v);
                }
                for &pos in at {
                    if let Some(v) = self.increment_gain[j][pos].first(character_taken) {
                        g = g.max(v);
                    }
                }
                if let Some(pt) = &b.points
                    && any_character
                {
                    bonus = pt.snap[j].max(0);
                }
            }
            snap_power.push(p);
            snap_gain.push(g);
            snap_bonus.push(bonus);
        }
        let power = powers.sum().min(base_powers.sum() + snap_power.sum());
        let gain = gains.sum_up().min(add_up(base_gains.sum_up(), snap_gain.sum_up()));
        let bonus = bonuses.sum().min(base_bonuses.sum() + snap_bonus.sum());
        (power, gain, bonus)
    }
}

/// The best pair of one forced slot whose choice lies in a fixed mask, per character (keys = choice, 0 = None).
/// With at most four Snaps fixed by a prefix, five distinct choices per character keep the exact maximum.
pub(super) struct ForcedTable {
    power: Vec<Vec<Top<i64>>>,
    gain: Vec<[Top<f64>; 5]>,
    bonus: Vec<Top<i64>>,
}

impl ForcedTable {
    pub(super) fn compile(
        b: &JointBounds,
        pool: &Pool,
        domain: &CandidateDomain,
        tables: &RelaxTables,
        mask: &[bool],
    ) -> Self {
        let nc = tables.characters.len();
        let character = |m: usize| {
            let c = pool.members[m].character_id;
            tables.characters.iter().position(|&x| x == c).expect("domain character")
        };
        let choices: Vec<usize> = (0..mask.len()).filter(|&c| mask[c]).collect();
        let keys = || choices.iter().map(|&c| c as u32);
        let best_float = |value: &dyn Fn(usize, usize) -> f64| -> Vec<Top<f64>> {
            let mut best = vec![vec![f64::NEG_INFINITY; choices.len()]; nc];
            for &m in domain.members() {
                let c = character(m);
                for (i, &choice) in choices.iter().enumerate() {
                    best[c][i] = best[c][i].max(value(m, choice));
                }
            }
            best.into_iter().map(|row| Top::from_best(row.into_iter().zip(keys()))).collect()
        };
        let best_int = |value: &dyn Fn(usize, usize) -> i64| -> Vec<Top<i64>> {
            let mut best = vec![vec![i64::MIN; choices.len()]; nc];
            for &m in domain.members() {
                let c = character(m);
                for (i, &choice) in choices.iter().enumerate() {
                    best[c][i] = best[c][i].max(value(m, choice));
                }
            }
            best.into_iter().map(|row| Top::from_best(row.into_iter().zip(keys()))).collect()
        };
        let snap_w = |m: usize, choice: usize| if choice == 0 { 0 } else { b.w[m][choice - 1] };
        let power = (0..b.lead.len())
            .map(|profile| best_int(&|m, choice| b.a[m] + b.lead[profile][m] + snap_w(m, choice)))
            .collect();
        let per_position: Vec<Vec<Top<f64>>> =
            (0..5).map(|pos| best_float(&|m, choice| b.gains[m][choice][pos])).collect();
        let gain = (0..nc).map(|c| std::array::from_fn(|pos| per_position[pos][c].clone())).collect();
        let bonus = match &b.points {
            Some(pt) => best_int(&|m, choice| pt.member[m] + if choice == 0 { 0 } else { pt.snap[choice - 1] }),
            None => vec![Top::default(); nc],
        };
        ForcedTable { power, gain, bonus }
    }

    /// The forced slot's (power, gain, bonus) maxima over free characters and free choices, each at least 0.
    pub(super) fn best(
        &self,
        profile: usize,
        position: usize,
        points: bool,
        taken_characters: &[bool],
        taken_snaps: &[bool],
    ) -> (i64, f64, i64) {
        let choice_taken = |choice: u32| choice > 0 && taken_snaps[choice as usize - 1];
        let (mut fp, mut fg, mut fb) = (0i64, 0.0f64, 0i64);
        for c in (0..taken_characters.len()).filter(|&c| !taken_characters[c]) {
            if let Some(v) = self.power[profile][c].first(choice_taken) {
                fp = fp.max(v);
            }
            if let Some(v) = self.gain[c][position].first(choice_taken) {
                fg = fg.max(v);
            }
            if points && let Some(v) = self.bonus[c].first(choice_taken) {
                fb = fb.max(v);
            }
        }
        (fp, fg, fb)
    }
}
